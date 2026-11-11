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

/// Put `text` on the clipboard. Returns the clipboard's sequence number
/// right after the write, to tell a later copy from this one.
pub fn write_clipboard_text(text: &str) -> Option<u32> {
    let bytes: Vec<u8> = text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
    if !open_clipboard() {
        return None;
    }
    let ok = unsafe {
        let _ = EmptyClipboard();
        mark_private();
        put(CF_UNICODETEXT.0 as u32, &bytes)
    };
    unsafe {
        let _ = CloseClipboard();
    }
    ok.then(|| unsafe { GetClipboardSequenceNumber() })
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

/// The clipboard's contents, format by format.
struct Snapshot {
    formats: Vec<(u32, Vec<u8>)>,
}

/// A snapshot bigger than this isn't taken; the transcript then stays.
const MAX_SNAPSHOT_BYTES: usize = 64 << 20;
/// Reading the clipboard can wait on the app that copied (delayed
/// rendering). Past this, no snapshot is taken, so the paste isn't held up.
const SNAPSHOT_BUDGET: Duration = Duration::from_millis(150);
/// Another app took the clipboard while it was being read.
const ERROR_CLIPBOARD_NOT_OPEN: WIN32_ERROR = WIN32_ERROR(1418);

/// Formats whose data is a GDI or private handle rather than memory, so it
/// can't be copied. Windows makes CF_BITMAP and the metafile formats again
/// from the DIB and text formats that are copied.
fn handle_format(format: u32) -> bool {
    matches!(format, 2 | 3 | 9 | 14 | 0x80 | 0x82 | 0x83 | 0x8E) || (0x200..=0x3FF).contains(&format)
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
                // Only 8-bit text needs its code page.
                LOCALE => text.is_some_and(|t| t != UNICODE_TEXT),
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

/// Read the open clipboard. `None` when it can't be read whole, in time.
unsafe fn read_snapshot() -> Option<Snapshot> {
    unsafe {
        let started = Instant::now();
        // List first, then read: rendering a format can let another app in,
        // which ends a listing early.
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

/// Put `snapshot` back, unless the clipboard changed since `sequence`.
/// Waits up to about 2 s for an app holding the clipboard to let go.
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

/// The restore waiting to run, if any.
static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static NEXT_RESTORE: AtomicU64 = AtomicU64::new(0);

/// What was on the clipboard before Quill wrote a transcript, to put back
/// after the paste.
pub struct SavedClipboard(Arc<Snapshot>);

impl SavedClipboard {
    /// Save the clipboard. Taken while an earlier restore is still waiting,
    /// the clipboard holds Quill's last transcript: that restore is
    /// cancelled and its snapshot kept instead.
    pub fn take() -> Option<Self> {
        if let Some(p) = PENDING.lock().take() {
            if p.sequence == unsafe { GetClipboardSequenceNumber() } {
                return Some(Self(p.snapshot));
            }
        }
        take_snapshot().map(|s| Self(Arc::new(s)))
    }

    /// Put the saved clipboard back after `delay`, unless the clipboard
    /// changed after Quill's own write (`sequence`, from
    /// [`write_clipboard_text`]). Returns whether it was put back.
    pub fn restore_after(self, delay: Duration, sequence: u32) -> std::thread::JoinHandle<bool> {
        let id = NEXT_RESTORE.fetch_add(1, SeqCst) + 1;
        *PENDING.lock() = Some(Pending { snapshot: self.0, sequence, id });
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

/// Remote Desktop forwards keys and the clipboard to another PC, which
/// needs time to catch up.
pub fn remote_desktop(exe: &str) -> bool {
    ["mstsc.exe", "msrdc.exe"].iter().any(|e| exe.eq_ignore_ascii_case(e))
}

/// How long after the paste the clipboard is put back.
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
                dwExtraInfo: super::hotkey::QUILL_INPUT,
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
    use windows::Win32::System::DataExchange::{EnumClipboardFormats, GetClipboardData, GetClipboardFormatNameW};

    /// Each registered format on the clipboard, with its DWORD value.
    fn registered_formats() -> Vec<(String, Option<u32>)> {
        assert!(open_clipboard(), "clipboard opened");
        let mut out = Vec::new();
        let mut format = 0;
        loop {
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                break;
            }
            let mut name = [0u16; 256];
            let len = unsafe { GetClipboardFormatNameW(format, &mut name) };
            if len <= 0 {
                continue;
            }
            let value = unsafe {
                GetClipboardData(format).ok().and_then(|h| {
                    let mem = windows::Win32::Foundation::HGLOBAL(h.0);
                    let ptr = GlobalLock(mem) as *const u32;
                    let v = (!ptr.is_null()).then(|| ptr.read_unaligned());
                    let _ = GlobalUnlock(mem);
                    v
                })
            };
            out.push((String::from_utf16_lossy(&name[..len as usize]), value));
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

    /// A 2x1 DIB: a red and a blue pixel.
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

    /// A copy made by the user, as another app would: text and an image,
    /// no privacy markers.
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
        assert_eq!(choose_formats(&listed), vec![13, 8, 0xC001]);
        let ansi = vec![(1, None), (16, None), (7, None), (17, None), (8, None)];
        assert_eq!(choose_formats(&ansi), vec![1, 16, 17], "8-bit text keeps its code page; the first DIB is kept");
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

    /// The time the clipboard step of a dictation takes, which counts toward
    /// stop_to_clipboard_ms: the write alone (restore off) or the snapshot
    /// plus the write (restore on), with plain text copied before.
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
