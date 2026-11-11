//! Delivery: the clipboard is authoritative, the paste is best effort
//! (section 4.2, step 6).

use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use windows::Win32::Foundation::{GetLastError, GlobalFree, SetLastError, HANDLE, HGLOBAL, HWND, WIN32_ERROR};
use windows::core::w;
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardFormatNameW, GetClipboardSequenceNumber,
    OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{CF_DIB, CF_UNICODETEXT};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, SetForegroundWindow,
    ShowWindow, SW_RESTORE,
};

use super::target::Target;

fn open_clipboard() -> bool {
    // Another app may hold it briefly.
    for _ in 0..10 {
        if unsafe { OpenClipboard(None) }.is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

pub fn set_clipboard_text(text: &str) -> bool {
    write_clipboard_text(text).is_some()
}

pub fn write_clipboard_text(text: &str) -> Option<u32> {
    try_write_clipboard_text(text).ok()
}

/// How far a failed clipboard write got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteFailed {
    /// Another app held the clipboard: it is untouched.
    NotOpened,
    /// The clipboard was emptied, then the text didn't go on. `sequence` is
    /// the clipboard's sequence number right after.
    Emptied { sequence: u32 },
}

pub fn try_write_clipboard_text(text: &str) -> Result<u32, WriteFailed> {
    let bytes: Vec<u8> = text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
    if !open_clipboard() {
        return Err(WriteFailed::NotOpened);
    }
    let ok = unsafe {
        let _ = EmptyClipboard();
        mark_private();
        put(CF_UNICODETEXT.0 as u32, &bytes)
    };
    unsafe {
        let _ = CloseClipboard();
    }
    let sequence = unsafe { GetClipboardSequenceNumber() };
    if ok { Ok(sequence) } else { Err(WriteFailed::Emptied { sequence }) }
}

/// Keep what Quill puts on the clipboard out of Win+V history and cloud
/// clipboard sync. The clipboard must be open.
unsafe fn mark_private() {
    unsafe {
        for name in [w!("CanIncludeInClipboardHistory"), w!("CanUploadToCloudClipboard")] {
            let format = RegisterClipboardFormatW(name);
            if format != 0 {
                put(format, &0u32.to_le_bytes());
            }
        }
    }
}

/// Copy `bytes` into a movable global block and hand it to the clipboard,
/// which must be open.
unsafe fn put(format: u32, bytes: &[u8]) -> bool {
    unsafe {
        let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) else { return false };
        let ptr = GlobalLock(mem) as *mut u8;
        if ptr.is_null() {
            let _ = GlobalFree(Some(mem));
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        let _ = GlobalUnlock(mem);
        if SetClipboardData(format, Some(HANDLE(mem.0))).is_ok() {
            true
        } else {
            let _ = GlobalFree(Some(mem));
            false
        }
    }
}

struct Snapshot {
    formats: Vec<(u32, Vec<u8>)>,
}

const MAX_SNAPSHOT_BYTES: usize = 64 << 20;
/// Reading the clipboard can wait on the app that copied (delayed
/// rendering). No format is read once this has passed, and the snapshot is
/// dropped, so the transcript stays. One slow format still blocks for as
/// long as the app takes to render it.
const SNAPSHOT_BUDGET: Duration = Duration::from_millis(150);
const ERROR_CLIPBOARD_NOT_OPEN: WIN32_ERROR = WIN32_ERROR(1418);

/// Formats whose data is a GDI or private handle rather than memory, so it
/// can't be copied. Windows makes CF_BITMAP and the metafile formats again
/// from the DIB and text formats that are copied.
fn handle_format(format: u32) -> bool {
    const BITMAP: u32 = 2;
    const METAFILE_PICT: u32 = 3;
    const PALETTE: u32 = 9;
    const ENH_METAFILE: u32 = 14;
    const OWNER_DISPLAY: u32 = 0x80;
    const DSP_BITMAP: u32 = 0x82;
    const DSP_METAFILE_PICT: u32 = 0x83;
    const DSP_ENH_METAFILE: u32 = 0x8E;
    const PRIVATE_AND_GDI_OBJECTS: std::ops::RangeInclusive<u32> = 0x200..=0x3FF;
    matches!(format, BITMAP | METAFILE_PICT | PALETTE | ENH_METAFILE | OWNER_DISPLAY | DSP_BITMAP | DSP_METAFILE_PICT | DSP_ENH_METAFILE)
        || PRIVATE_AND_GDI_OBJECTS.contains(&format)
}

/// OLE embedding formats: large, slow to render, and useless without the
/// app that made them.
const OLE_FORMATS: [&str; 7] =
    ["Embed Source", "Link Source", "Native", "ObjectLink", "OwnerLink", "Object Descriptor", "Link Source Descriptor"];

const TEXT: u32 = 1;
const OEM_TEXT: u32 = 7;
const DIB: u32 = 8;
const UNICODE_TEXT: u32 = 13;
const LOCALE: u32 = 16;
const DIB_V5: u32 = 17;

/// The formats worth saving, from those listed with their names. Windows
/// derives the rest of each family (the other text encodings, the other DIB
/// header) from the one kept.
fn choose_formats(listed: &[(u32, Option<String>)]) -> Vec<u32> {
    let ids = || listed.iter().map(|(f, _)| *f);
    let text = if ids().any(|f| f == UNICODE_TEXT) { Some(UNICODE_TEXT) } else { ids().find(|f| matches!(*f, TEXT | OEM_TEXT)) };
    let image = ids().find(|f| matches!(*f, DIB | DIB_V5));
    listed
        .iter()
        .filter(|(f, name)| {
            if handle_format(*f) || name.as_deref().is_some_and(|n| OLE_FORMATS.contains(&n)) {
                return false;
            }
            match *f {
                TEXT | OEM_TEXT | UNICODE_TEXT => text == Some(*f),
                DIB | DIB_V5 => image == Some(*f),
                // Windows turns the kept text back into 8-bit text with the
                // locale on the clipboard; without it, Quill's own is used.
                LOCALE => true,
                _ => true,
            }
        })
        .map(|(f, _)| *f)
        .collect()
}

fn format_name(format: u32) -> Option<String> {
    if format < 0xC000 {
        return None;
    }
    let mut name = [0u16; 256];
    let len = unsafe { GetClipboardFormatNameW(format, &mut name) };
    (len > 0).then(|| String::from_utf16_lossy(&name[..len as usize]))
}

unsafe fn read_snapshot() -> Option<Snapshot> {
    unsafe {
        let started = Instant::now();
        SetLastError(WIN32_ERROR(0));
        let mut listed = Vec::new();
        let mut format = 0;
        loop {
            format = EnumClipboardFormats(format);
            if format == 0 {
                break;
            }
            listed.push((format, format_name(format)));
        }
        if GetLastError() != WIN32_ERROR(0) {
            return None;
        }
        let mut formats = Vec::new();
        let mut total = 0usize;
        for format in choose_formats(&listed) {
            if started.elapsed() > SNAPSHOT_BUDGET {
                return None;
            }
            let h = match GetClipboardData(format) {
                Ok(h) => h,
                Err(e) if e.code() == ERROR_CLIPBOARD_NOT_OPEN.to_hresult() => return None,
                Err(_) => continue,
            };
            let mem = HGLOBAL(h.0);
            let size = GlobalSize(mem);
            if size == 0 {
                continue;
            }
            total += size;
            if total > MAX_SNAPSHOT_BYTES {
                return None;
            }
            let ptr = GlobalLock(mem) as *const u8;
            if ptr.is_null() {
                continue;
            }
            formats.push((format, std::slice::from_raw_parts(ptr, size).to_vec()));
            let _ = GlobalUnlock(mem);
        }
        (started.elapsed() <= SNAPSHOT_BUDGET).then_some(Snapshot { formats })
    }
}

fn take_snapshot() -> Option<Snapshot> {
    if !open_clipboard() {
        return None;
    }
    let snapshot = unsafe { read_snapshot() };
    unsafe {
        let _ = CloseClipboard();
    }
    snapshot
}

fn put_back(snapshot: &Snapshot, sequence: u32) -> bool {
    for _ in 0..10 {
        if unsafe { GetClipboardSequenceNumber() } != sequence {
            return false;
        }
        if !open_clipboard() {
            continue;
        }
        // Nothing else can change the clipboard while it's open.
        let unchanged = unsafe { GetClipboardSequenceNumber() } == sequence;
        if unchanged {
            unsafe {
                let _ = EmptyClipboard();
                // The original copy is already in Win+V history; don't add it twice.
                // The snapshot's own markers, if any, replace these.
                mark_private();
                for (format, bytes) in &snapshot.formats {
                    put(*format, bytes);
                }
            }
        }
        unsafe {
            let _ = CloseClipboard();
        }
        return unchanged;
    }
    false
}

struct Pending {
    snapshot: Arc<Snapshot>,
    sequence: u32,
    id: u64,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static NEXT_RESTORE: AtomicU64 = AtomicU64::new(0);

pub struct SavedClipboard {
    snapshot: Arc<Snapshot>,
    adopted: Option<u32>,
}

impl SavedClipboard {
    pub fn take() -> Option<Self> {
        if let Some(p) = PENDING.lock().take() {
            if p.sequence == unsafe { GetClipboardSequenceNumber() } {
                return Some(Self { snapshot: p.snapshot, adopted: Some(p.sequence) });
            }
        }
        take_snapshot().map(|s| Self { snapshot: Arc::new(s), adopted: None })
    }

    pub fn restore_now(self, sequence: u32) -> bool {
        put_back(&self.snapshot, sequence)
    }

    pub fn restore_after_failed_write(self, failed: WriteFailed) -> bool {
        match failed {
            WriteFailed::Emptied { sequence } => put_back(&self.snapshot, sequence),
            WriteFailed::NotOpened => self.adopted.is_some_and(|sequence| put_back(&self.snapshot, sequence)),
        }
    }

    pub fn restore_after(self, delay: Duration, sequence: u32) -> std::thread::JoinHandle<bool> {
        let id = NEXT_RESTORE.fetch_add(1, SeqCst) + 1;
        *PENDING.lock() = Some(Pending { snapshot: self.snapshot, sequence, id });
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            let mut pending = PENDING.lock();
            match pending.take() {
                Some(p) if p.id == id => put_back(&p.snapshot, p.sequence),
                other => {
                    *pending = other;
                    false
                }
            }
        })
    }
}

pub fn flush_pending_restore() {
    if let Some(p) = PENDING.lock().take() {
        put_back(&p.snapshot, p.sequence);
    }
}

/// Remote Desktop forwards keys and the clipboard to another PC, which
/// needs time to catch up.
pub fn remote_desktop(exe: &str) -> bool {
    ["mstsc.exe", "msrdc.exe"].iter().any(|e| exe.eq_ignore_ascii_case(e))
}

pub fn restore_delay(target: &Target) -> Duration {
    Duration::from_millis(if remote_desktop(&target.exe) { 3000 } else { 500 })
}

/// An image on the clipboard, as PNG (browsers and Electron apps) and as a
/// DIB (everything else). `rgba` is opaque RGBA8, top-down.
pub fn set_clipboard_image(png: &[u8], width: u32, height: u32, rgba: &[u8]) -> bool {
    let mut dib = Vec::with_capacity(40 + rgba.len());
    for v in [40u32, width, height] {
        dib.extend_from_slice(&v.to_le_bytes());
    }
    dib.extend_from_slice(&1u16.to_le_bytes()); // planes
    dib.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
    dib.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    dib.extend_from_slice(&(rgba.len() as u32).to_le_bytes());
    dib.extend_from_slice(&[0u8; 16]); // resolution and palette
    // Bottom-up BGRA rows.
    for row in rgba.chunks_exact(width as usize * 4).rev() {
        for px in row.chunks_exact(4) {
            dib.extend_from_slice(&[px[2], px[1], px[0], 255]);
        }
    }
    if !open_clipboard() {
        return false;
    }
    let ok = unsafe {
        let _ = EmptyClipboard();
        mark_private();
        let png_format = RegisterClipboardFormatW(w!("PNG"));
        let a = png_format != 0 && put(png_format, png);
        let b = put(CF_DIB.0 as u32, &dib);
        a || b
    };
    unsafe {
        let _ = CloseClipboard();
    }
    ok
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: super::input_activity::QUILL_INPUT,
            },
        },
    }
}

fn is_down(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0 }
}

const MODIFIERS: [VIRTUAL_KEY; 5] = [VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN, VK_CONTROL];

/// Give the user a moment to let go of the shortcut's modifiers, so Ctrl+V
/// isn't sent as Ctrl+Alt+V.
fn wait_for_modifiers_up(max: Duration) {
    let start = Instant::now();
    while start.elapsed() < max && MODIFIERS.iter().any(|m| is_down(*m)) {
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn bring_to_front(hwnd: HWND) -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg == hwnd {
            return true;
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        // Attach to the foreground thread so Windows lets us switch focus.
        let fg_thread = GetWindowThreadProcessId(fg, None);
        let me = GetCurrentThreadId();
        let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, true).as_bool();
        let _ = BringWindowToTop(hwnd);
        let ok = SetForegroundWindow(hwnd).as_bool();
        if attached {
            let _ = AttachThreadInput(me, fg_thread, false);
        }
        ok || GetForegroundWindow() == hwnd
    }
}

/// Bring the captured target forward, or check the foreground window if the
/// target is gone. False when it can't take a paste.
pub fn focus(target: &Target, own_windows: &[isize]) -> bool {
    if target.elevated {
        return false;
    }
    let hwnd = if target.is_alive() {
        target.hwnd()
    } else {
        let fg = unsafe { GetForegroundWindow() };
        if fg.0.is_null() || own_windows.contains(&(fg.0 as isize)) {
            return false;
        }
        fg
    };
    if !target.editable && target.is_alive() {
        return false;
    }
    bring_to_front(hwnd)
}

/// Send Ctrl+V to the captured target, or the current foreground window if
/// the target is gone. Returns whether a paste was sent.
/// `still_current` is checked right before the keystroke, so a dictation
/// cancelled while we waited never pastes.
pub fn paste(target: &Target, own_windows: &[isize], still_current: impl Fn() -> bool) -> bool {
    if !focus(target, own_windows) {
        return false;
    }
    wait_for_modifiers_up(Duration::from_millis(600));
    if remote_desktop(&target.exe) {
        std::thread::sleep(Duration::from_millis(400));
    }
    if !still_current() {
        return false;
    }
    // Anything still held is released first.
    let mut inputs: Vec<INPUT> = MODIFIERS.iter().filter(|m| is_down(**m)).map(|m| key(*m, true)).collect();
    inputs.extend([key(VK_CONTROL, false), key(VK_V, false), key(VK_V, true), key(VK_CONTROL, true)]);
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    sent as usize == inputs.len()
}

/// Press Enter in the foreground window, after a paste has landed.
pub fn press_enter() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_RETURN;
    wait_for_modifiers_up(Duration::from_millis(600));
    let inputs = [key(VK_RETURN, false), key(VK_RETURN, true)];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    sent as usize == inputs.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registered_formats() -> Vec<(String, Option<u32>)> {
        assert!(open_clipboard(), "clipboard opened");
        let mut out = Vec::new();
        let mut format = 0;
        loop {
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                break;
            }
            let Some(name) = format_name(format) else { continue };
            let value = unsafe {
                GetClipboardData(format).ok().and_then(|h| {
                    let mem = HGLOBAL(h.0);
                    let ptr = GlobalLock(mem) as *const u32;
                    let v = (!ptr.is_null()).then(|| ptr.read_unaligned());
                    let _ = GlobalUnlock(mem);
                    v
                })
            };
            out.push((name, value));
        }
        unsafe {
            let _ = CloseClipboard();
        }
        out
    }

    fn assert_private(formats: &[(String, Option<u32>)]) {
        println!("{formats:?}");
        for name in ["CanIncludeInClipboardHistory", "CanUploadToCloudClipboard"] {
            assert!(formats.iter().any(|(n, v)| n == name && *v == Some(0)), "{name} = 0 in {formats:?}");
        }
        assert!(!formats.iter().any(|(n, _)| n == "ExcludeClipboardContentFromMonitorProcessing"), "{formats:?}");
    }

    #[test]
    #[ignore = "replaces the clipboard's contents"]
    fn clipboard_writes_stay_out_of_history() {
        assert!(set_clipboard_text("Quill clipboard privacy check"), "text written");
        assert_private(&registered_formats());
        let rgba = [255u8, 0, 0, 255];
        assert!(set_clipboard_image(&[], 1, 1, &rgba), "image written");
        assert_private(&registered_formats());
    }

    #[test]
    fn remote_desktop_targets() {
        assert!(remote_desktop("mstsc.exe") && remote_desktop("MSRDC.EXE"));
        assert!(!remote_desktop("notepad.exe"));
        let rdp = Target { exe: "mstsc.exe".into(), ..Default::default() };
        assert_eq!(restore_delay(&rdp), Duration::from_secs(3));
        assert_eq!(restore_delay(&Target::default()), Duration::from_millis(500));
    }

    fn sample_dib() -> Vec<u8> {
        let mut dib = Vec::new();
        for v in [40u32, 2, 1] {
            dib.extend_from_slice(&v.to_le_bytes());
        }
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&32u16.to_le_bytes());
        dib.extend_from_slice(&0u32.to_le_bytes());
        dib.extend_from_slice(&8u32.to_le_bytes());
        dib.extend_from_slice(&[0u8; 16]);
        dib.extend_from_slice(&[0, 0, 255, 255, 255, 0, 0, 255]);
        dib
    }

    fn user_copy(text: &str) {
        let utf16: Vec<u8> = text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
        assert!(open_clipboard(), "clipboard opened");
        unsafe {
            let _ = EmptyClipboard();
            assert!(put(CF_UNICODETEXT.0 as u32, &utf16), "user text written");
            assert!(put(CF_DIB.0 as u32, &sample_dib()), "user image written");
            let _ = CloseClipboard();
        }
    }

    fn read(format: u32) -> Option<Vec<u8>> {
        assert!(open_clipboard(), "clipboard opened");
        let out = unsafe {
            GetClipboardData(format).ok().and_then(|h| {
                let mem = HGLOBAL(h.0);
                let size = GlobalSize(mem);
                let ptr = GlobalLock(mem) as *const u8;
                let v = (!ptr.is_null()).then(|| std::slice::from_raw_parts(ptr, size).to_vec());
                let _ = GlobalUnlock(mem);
                v
            })
        };
        unsafe {
            let _ = CloseClipboard();
        }
        out
    }

    fn text() -> String {
        let bytes = read(CF_UNICODETEXT.0 as u32).unwrap_or_default();
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|u| *u != 0).collect();
        String::from_utf16_lossy(&units)
    }

    fn image() -> Option<Vec<u8>> {
        read(CF_DIB.0 as u32).map(|d| d[..sample_dib().len().min(d.len())].to_vec())
    }

    #[test]
    fn keeps_one_format_per_family_and_no_ole_objects() {
        let named = |f: u32, n: &str| (f, Some(n.to_string()));
        let listed = vec![
            (13, None), (16, None), (1, None), (7, None),
            (8, None), (2, None), (17, None),
            named(0xC001, "HTML Format"), named(0xC002, "Embed Source"), named(0xC003, "Object Descriptor"), named(0xC004, "Native"),
            (0x200, None), (14, None),
        ];
        assert_eq!(choose_formats(&listed), vec![13, 16, 8, 0xC001]);
        // An app that copies 8-bit text: Windows lists its locale and the
        // other encodings beside it.
        let ansi = vec![(1, None), (16, None), (7, None), (13, None), (17, None), (8, None)];
        assert_eq!(choose_formats(&ansi), vec![16, 13, 17], "the locale stays with the text; the first DIB is kept");
        assert_eq!(choose_formats(&[(1, None), (7, None)]), vec![1], "8-bit text alone");
        assert!(choose_formats(&[]).is_empty());
    }

    #[test]
    #[ignore = "replaces the clipboard's contents"]
    fn restores_the_users_clipboard_but_never_over_a_newer_copy() {
        user_copy("The user's own copy");
        let saved = SavedClipboard::take().expect("snapshot taken");
        let written = write_clipboard_text("Quill transcript").expect("transcript written");
        assert_eq!(text(), "Quill transcript");
        assert!(saved.restore_after(Duration::from_millis(50), written).join().unwrap(), "restored");
        println!("after restore: text={:?} image={:?}", text(), image());
        assert_eq!(text(), "The user's own copy");
        assert_eq!(image(), Some(sample_dib()), "image back byte for byte");

        user_copy("The user's own copy");
        let saved = SavedClipboard::take().expect("snapshot taken");
        let written = write_clipboard_text("Quill transcript").expect("transcript written");
        let restore = saved.restore_after(Duration::from_millis(400), written);
        std::thread::sleep(Duration::from_millis(100));
        user_copy("A newer copy");
        assert!(!restore.join().unwrap(), "skipped");
        println!("newer copy kept: text={:?}", text());
        assert_eq!(text(), "A newer copy");

        user_copy("The user's own copy");
        let saved = SavedClipboard::take().expect("snapshot taken");
        let written = write_clipboard_text("Quill transcript").expect("transcript written");
        user_copy("Copied while Quill waited to paste");
        assert!(!saved.restore_after(Duration::from_millis(50), written).join().unwrap(), "skipped");
        println!("copy during the paste waits kept: text={:?}", text());
        assert_eq!(text(), "Copied while Quill waited to paste");

        user_copy("The user's own copy");
        let first = SavedClipboard::take().expect("snapshot taken");
        let written = write_clipboard_text("First transcript").expect("first written");
        let first = first.restore_after(Duration::from_millis(400), written);
        let second = SavedClipboard::take().expect("pending snapshot adopted");
        let written = write_clipboard_text("Second transcript").expect("second written");
        let second = second.restore_after(Duration::from_millis(50), written);
        assert!(second.join().unwrap(), "second restored");
        assert!(!first.join().unwrap(), "first cancelled");
        println!("back-to-back dictations: text={:?} image={:?}", text(), image());
        assert_eq!(text(), "The user's own copy");
        assert_eq!(image(), Some(sample_dib()));
        assert_private(&registered_formats());

        user_copy("The user's own copy");
        let saved = SavedClipboard::take().expect("snapshot taken");
        let written = write_clipboard_text("Cancelled transcript").expect("transcript written");
        assert!(saved.restore_now(written), "given back at once");
        println!("cancelled before the paste: text={:?}", text());
        assert_eq!(text(), "The user's own copy");

        user_copy("The user's own copy");
        let saved = SavedClipboard::take().expect("snapshot taken");
        let written = write_clipboard_text("Transcript as Quill quits").expect("transcript written");
        let waiting = saved.restore_after(Duration::from_secs(3), written);
        flush_pending_restore();
        println!("quit while a restore waited: text={:?}", text());
        assert_eq!(text(), "The user's own copy");
        assert!(!waiting.join().unwrap(), "the waiting restore found nothing left to do");

        user_copy("The user's own copy");
        let saved = SavedClipboard::take().expect("snapshot taken");
        assert!(!saved.restore_after_failed_write(WriteFailed::NotOpened), "an untouched copy isn't rewritten");
        assert_eq!(text(), "The user's own copy");

        user_copy("The user's own copy");
        let saved = SavedClipboard::take().expect("snapshot taken");
        assert!(open_clipboard(), "clipboard opened");
        unsafe {
            let _ = EmptyClipboard();
            let _ = CloseClipboard();
        }
        let emptied = WriteFailed::Emptied { sequence: unsafe { GetClipboardSequenceNumber() } };
        assert!(saved.restore_after_failed_write(emptied), "an emptied clipboard gets the copy back");
        println!("write failed after emptying: text={:?}", text());
        assert_eq!(text(), "The user's own copy");

        user_copy("The user's own copy");
        let first = SavedClipboard::take().expect("snapshot taken");
        let written = write_clipboard_text("First transcript").expect("first written");
        let first = first.restore_after(Duration::from_secs(3), written);
        let adopted = SavedClipboard::take().expect("pending snapshot adopted");
        user_copy("A newer copy");
        assert!(!adopted.restore_after_failed_write(WriteFailed::NotOpened), "never over a newer copy");
        println!("write failed after a newer copy: text={:?}", text());
        assert_eq!(text(), "A newer copy");
        assert!(!first.join().unwrap(), "first cancelled");

        user_copy("The user's own copy");
        let first = SavedClipboard::take().expect("snapshot taken");
        let written = write_clipboard_text("First transcript").expect("first written");
        let first = first.restore_after(Duration::from_secs(3), written);
        let adopted = SavedClipboard::take().expect("pending snapshot adopted");
        assert!(adopted.restore_after_failed_write(WriteFailed::NotOpened), "the earlier transcript gives way");
        println!("write failed over an earlier transcript: text={:?}", text());
        assert_eq!(text(), "The user's own copy");
        assert!(!first.join().unwrap(), "first cancelled");
    }

    fn plain_copy(text: &str) {
        let utf16: Vec<u8> = text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
        assert!(open_clipboard(), "clipboard opened");
        unsafe {
            let _ = EmptyClipboard();
            assert!(put(CF_UNICODETEXT.0 as u32, &utf16), "user text written");
            let _ = CloseClipboard();
        }
    }

    #[test]
    #[ignore = "replaces the clipboard's contents"]
    fn clipboard_step_timing() {
        let median = |mut v: Vec<f64>| {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v[v.len() / 2]
        };
        let mut off = Vec::new();
        let mut on = Vec::new();
        for _ in 0..21 {
            plain_copy("Plain text the user copied");
            let t = Instant::now();
            assert!(set_clipboard_text("Quill transcript"));
            off.push(t.elapsed().as_secs_f64() * 1000.0);

            plain_copy("Plain text the user copied");
            let t = Instant::now();
            let saved = SavedClipboard::take();
            assert!(set_clipboard_text("Quill transcript"));
            on.push(t.elapsed().as_secs_f64() * 1000.0);
            assert!(saved.is_some(), "snapshot taken");
        }
        println!("clipboard step, median of 21: restore off {:.2} ms, restore on {:.2} ms", median(off), median(on));
    }
}
