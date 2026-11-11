//! Counts mouse button presses, touches and pen input anywhere, through Raw
//! Input on a hidden message-only window. Unlike a low-level mouse hook,
//! nothing here sits in the path of the cursor, so a slow moment can't lag
//! the mouse.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering::SeqCst};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::{
    GetRawInputData, GetRegisteredRawInputDevices, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE,
    RAWINPUTHEADER, RIDEV_INPUTSINK, RID_HEADER, RID_INPUT, RIM_TYPEHID, RIM_TYPEMOUSE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, TranslateMessage, HWND_MESSAGE, MSG,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_INPUT, WNDCLASSW,
};

use super::hotkey::QUILL_INPUT;

static RUNNING: AtomicBool = AtomicBool::new(false);
static STARTED: AtomicBool = AtomicBool::new(false);
static CLICKS: AtomicU32 = AtomicU32::new(0);
/// The hidden window Raw Input reports to.
static WINDOW: AtomicIsize = AtomicIsize::new(0);

const GENERIC_DESKTOP: u16 = 0x01;
const MOUSE: u16 = 0x02;
const DIGITIZER: u16 = 0x0D;
const PEN: u16 = 0x02;
const TOUCH_SCREEN: u16 = 0x04;
#[cfg(test)]
static LAST_EXTRA: AtomicU32 = AtomicU32::new(0);

/// Left, right, middle, and the two side buttons going down.
const BUTTONS_DOWN: u16 = 0x0001 | 0x0004 | 0x0010 | 0x0040 | 0x0100;

fn users_button_down(button_flags: u16, extra: u32) -> bool {
    button_flags & BUTTONS_DOWN != 0 && extra != QUILL_INPUT as u32
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_INPUT {
        let raw = HRAWINPUT(lparam.0 as *mut _);
        let header_size = std::mem::size_of::<RAWINPUTHEADER>() as u32;
        let mut header = RAWINPUTHEADER::default();
        let mut size = header_size;
        let read = unsafe { GetRawInputData(raw, RID_HEADER, Some(&mut header as *mut RAWINPUTHEADER as *mut _), &mut size, header_size) };
        if read != u32::MAX && header.dwType == RIM_TYPEHID.0 {
            // A touch or pen report.
            CLICKS.fetch_add(1, SeqCst);
        } else if read != u32::MAX && header.dwType == RIM_TYPEMOUSE.0 {
            let mut input = RAWINPUT::default();
            let mut size = std::mem::size_of::<RAWINPUT>() as u32;
            let read = unsafe { GetRawInputData(raw, RID_INPUT, Some(&mut input as *mut RAWINPUT as *mut _), &mut size, header_size) };
            if read != u32::MAX {
                let mouse = unsafe { input.data.mouse };
                let flags = unsafe { mouse.Anonymous.Anonymous.usButtonFlags };
                #[cfg(test)]
                LAST_EXTRA.store(mouse.ulExtraInformation, SeqCst);
                if users_button_down(flags, mouse.ulExtraInformation) {
                    CLICKS.fetch_add(1, SeqCst);
                }
            }
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Start counting on a thread of its own. Returns whether counting runs.
pub fn start() -> bool {
    if STARTED.swap(true, SeqCst) {
        return RUNNING.load(SeqCst);
    }
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("quill-clicks".into()).spawn(move || unsafe {
        let ok = (|| {
            let instance = GetModuleHandleW(None).ok()?;
            let class = WNDCLASSW { lpfnWndProc: Some(window_proc), hInstance: instance.into(), lpszClassName: w!("QuillClicks"), ..Default::default() };
            if RegisterClassW(&class) == 0 {
                return None;
            }
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("QuillClicks"),
                w!(""),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance.into()),
                None,
            )
            .ok()?;
            let device = |page, usage| RAWINPUTDEVICE { usUsagePage: page, usUsage: usage, dwFlags: RIDEV_INPUTSINK, hwndTarget: hwnd };
            let size = std::mem::size_of::<RAWINPUTDEVICE>() as u32;
            RegisterRawInputDevices(&[device(GENERIC_DESKTOP, MOUSE)], size).ok()?;
            WINDOW.store(hwnd.0 as isize, SeqCst);
            // Best effort: not every PC has a touch screen or pen.
            let _ = RegisterRawInputDevices(&[device(DIGITIZER, TOUCH_SCREEN), device(DIGITIZER, PEN)], size);
            Some(())
        })()
        .is_some();
        RUNNING.store(ok, SeqCst);
        let _ = ready_tx.send(ok);
        if !ok {
            return;
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        RUNNING.store(false, SeqCst);
    });
    spawned.is_ok() && ready_rx.recv().unwrap_or(false)
}

/// Whether the mouse still reports to the hidden window. A process has one
/// Raw Input target per device type, so a later registration elsewhere in
/// Quill (tao's device events) takes the mouse away.
fn still_registered() -> bool {
    let size = std::mem::size_of::<RAWINPUTDEVICE>() as u32;
    let mut count = 0u32;
    unsafe { GetRegisteredRawInputDevices(None, &mut count, size) };
    let mut devices = vec![RAWINPUTDEVICE::default(); count as usize];
    let read = unsafe { GetRegisteredRawInputDevices(Some(devices.as_mut_ptr()), &mut count, size) };
    if read == u32::MAX {
        return false;
    }
    let window = WINDOW.load(SeqCst);
    devices[..read as usize]
        .iter()
        .any(|d| d.usUsagePage == GENERIC_DESKTOP && d.usUsage == MOUSE && d.hwndTarget.0 as isize == window)
}

/// How many mouse buttons, touches and pen reports there have been; `None`
/// unless counting runs and still has the mouse.
pub fn clicks() -> Option<u32> {
    (RUNNING.load(SeqCst) && still_registered()).then(|| CLICKS.load(SeqCst))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT};

    #[test]
    fn counts_button_presses_but_not_quills_own() {
        assert!(users_button_down(0x0001, 0), "left down");
        assert!(users_button_down(0x0004, 0) && users_button_down(0x0010, 7) && users_button_down(0x0040, 0) && users_button_down(0x0100, 0));
        assert!(!users_button_down(0x0002, 0), "left up");
        assert!(!users_button_down(0x0400, 0), "wheel");
        assert!(!users_button_down(0, 0), "move");
        assert!(!users_button_down(0x0001, QUILL_INPUT as u32), "Quill's own");
    }

    #[test]
    #[ignore = "moves the mouse one pixel and back"]
    fn the_hidden_window_receives_mouse_input() {
        assert!(start(), "raw input registered");
        assert!(clicks().is_some(), "the mouse reports to the hidden window");
        let tag = 0x0BAD_F00D_u32;
        let step = |dx: i32| INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy: 0, mouseData: 0, dwFlags: MOUSEEVENTF_MOVE, time: 0, dwExtraInfo: tag as usize } },
        };
        let sent = unsafe { SendInput(&[step(1), step(-1)], std::mem::size_of::<INPUT>() as i32) };
        assert_eq!(sent, 2, "moves sent");
        let start = Instant::now();
        while LAST_EXTRA.load(SeqCst) != tag && start.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(10));
        }
        println!("raw input saw extra info {:#x} after {:?}", LAST_EXTRA.load(SeqCst), start.elapsed());
        assert_eq!(LAST_EXTRA.load(SeqCst), tag, "raw input arrived with its tag");
    }
}
