//! Pointer input for `input_activity`: mouse buttons, touches and pen,
//! through Raw Input on a hidden message-only window. Unlike a low-level
//! mouse hook, nothing here sits in the path of the cursor.
//!
//! Raw Input wakes this thread for every mouse move, so the devices are
//! registered only while a continuation is possible (`watch`) and removed at
//! the first input or when the continuation window ends.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering::SeqCst};
use std::time::Duration;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::{
    GetRawInputData, GetRegisteredRawInputDevices, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE,
    RAWINPUTDEVICE_FLAGS, RAWINPUTHEADER, RIDEV_INPUTSINK, RIDEV_REMOVE, RID_INPUT, RIM_TYPEHID, RIM_TYPEMOUSE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, PostMessageW, RegisterClassW, SendMessageW,
    SetTimer, TranslateMessage, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_INPUT, WM_TIMER, WNDCLASSW,
};

use super::input_activity::{self, QUILL_INPUT};

/// The hidden window, once its thread runs.
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static STARTED: AtomicBool = AtomicBool::new(false);
/// The devices are registered to the hidden window.
static WATCHING: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static LAST_EXTRA: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

const GENERIC_DESKTOP: u16 = 0x01;
const MOUSE: u16 = 0x02;
const DIGITIZER: u16 = 0x0D;
const PEN: u16 = 0x02;
const TOUCH_SCREEN: u16 = 0x04;

const WM_WATCH: u32 = WM_APP + 1;
const WM_STOP: u32 = WM_APP + 2;
const WATCH_TIMER: usize = 1;
/// A little past the continuation window, so the watch outlasts it.
const WATCH_FOR: Duration = Duration::from_secs(quill_core::text::continuation::WINDOW.as_secs() + 5);

/// Left, right, middle, and the two side buttons going down.
const BUTTONS_DOWN: u16 = 0x0001 | 0x0004 | 0x0010 | 0x0040 | 0x0100;
const SIDE_BUTTONS_DOWN: u16 = 0x0040 | 0x0100;

fn users_button_down(button_flags: u16, extra: u32) -> bool {
    button_flags & BUTTONS_DOWN != 0 && extra != QUILL_INPUT as u32
}

/// A large enough, aligned buffer for one Raw Input report.
#[repr(C, align(8))]
struct Report([u8; 1024]);

fn devices(hwnd: HWND, flags: RAWINPUTDEVICE_FLAGS) -> [RAWINPUTDEVICE; 3] {
    let device = |page, usage| RAWINPUTDEVICE { usUsagePage: page, usUsage: usage, dwFlags: flags, hwndTarget: hwnd };
    [device(GENERIC_DESKTOP, MOUSE), device(DIGITIZER, TOUCH_SCREEN), device(DIGITIZER, PEN)]
}

fn register(hwnd: HWND) -> bool {
    let size = std::mem::size_of::<RAWINPUTDEVICE>() as u32;
    let [mouse, touch, pen] = devices(hwnd, RIDEV_INPUTSINK);
    let ok = unsafe { RegisterRawInputDevices(&[mouse], size) }.is_ok();
    if ok {
        // Best effort: not every PC has a touch screen or pen.
        let _ = unsafe { RegisterRawInputDevices(&[touch, pen], size) };
    }
    ok
}

fn unregister() {
    let size = std::mem::size_of::<RAWINPUTDEVICE>() as u32;
    // Removing takes no target window.
    for device in devices(HWND::default(), RIDEV_REMOVE) {
        let _ = unsafe { RegisterRawInputDevices(&[device], size) };
    }
}

/// Runs on the hidden window's thread.
fn stop_here(hwnd: HWND) {
    if WATCHING.swap(false, SeqCst) {
        unregister();
    }
    let _ = unsafe { KillTimer(Some(hwnd), WATCH_TIMER) };
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_WATCH => {
            let ok = WATCHING.load(SeqCst) || register(hwnd);
            WATCHING.store(ok, SeqCst);
            if ok {
                unsafe { SetTimer(Some(hwnd), WATCH_TIMER, WATCH_FOR.as_millis() as u32, None) };
            }
            return LRESULT(ok as isize);
        }
        WM_STOP => {
            stop_here(hwnd);
            return LRESULT(0);
        }
        WM_TIMER if wparam.0 == WATCH_TIMER => {
            stop_here(hwnd);
            return LRESULT(0);
        }
        WM_INPUT => {
            let mut report = Report([0; 1024]);
            let mut size = std::mem::size_of::<Report>() as u32;
            let header = std::mem::size_of::<RAWINPUTHEADER>() as u32;
            let read = unsafe {
                GetRawInputData(HRAWINPUT(lparam.0 as *mut _), RID_INPUT, Some(report.0.as_mut_ptr() as *mut _), &mut size, header)
            };
            if read != u32::MAX && read >= header {
                let input = report.0.as_ptr() as *const RAWINPUT;
                let kind = unsafe { (*input).header.dwType };
                if kind == RIM_TYPEHID.0 {
                    // A touch or pen report.
                    input_activity::pointer_input();
                    stop_here(hwnd);
                } else if kind == RIM_TYPEMOUSE.0 {
                    let mouse = unsafe { (*input).data.mouse };
                    let flags = unsafe { mouse.Anonymous.Anonymous.usButtonFlags };
                    #[cfg(test)]
                    LAST_EXTRA.store(mouse.ulExtraInformation, SeqCst);
                    if users_button_down(flags, mouse.ulExtraInformation) {
                        input_activity::pointer_input();
                        // A side button may be Quill's shortcut, taken back
                        // out of the count by the mouse hook: keep watching.
                        if flags & SIDE_BUTTONS_DOWN == 0 {
                            stop_here(hwnd);
                        }
                    }
                }
            }
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn window() -> Option<HWND> {
    let hwnd = WINDOW.load(SeqCst);
    (hwnd != 0).then(|| HWND(hwnd as *mut _))
}

/// Create the hidden window on a thread of its own. Nothing is registered
/// until `watch`. Returns whether the window exists.
pub fn start() -> bool {
    if STARTED.swap(true, SeqCst) {
        return window().is_some();
    }
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("quill-clicks".into()).spawn(move || unsafe {
        let hwnd = (|| {
            let instance = GetModuleHandleW(None).ok()?;
            let class = WNDCLASSW { lpfnWndProc: Some(window_proc), hInstance: instance.into(), lpszClassName: w!("QuillClicks"), ..Default::default() };
            if RegisterClassW(&class) == 0 {
                return None;
            }
            CreateWindowExW(
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
            .ok()
        })();
        if let Some(hwnd) = hwnd {
            WINDOW.store(hwnd.0 as isize, SeqCst);
        }
        let _ = ready_tx.send(hwnd.is_some());
        if hwnd.is_none() {
            return;
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        WINDOW.store(0, SeqCst);
        WATCHING.store(false, SeqCst);
    });
    spawned.is_ok() && ready_rx.recv().unwrap_or(false)
}

/// Register the devices and count pointer input for the continuation
/// window. Waits until they are registered.
pub fn watch() -> bool {
    window().is_some_and(|hwnd| unsafe { SendMessageW(hwnd, WM_WATCH, None, None) }.0 != 0)
}

/// Unregister the devices. Cheap enough to call from the keyboard hook.
pub fn stop_watching() {
    if let (true, Some(hwnd)) = (WATCHING.load(SeqCst), window()) {
        let _ = unsafe { PostMessageW(Some(hwnd), WM_STOP, WPARAM(0), LPARAM(0)) };
    }
}

/// Whether the mouse reports to the hidden window. A process has one Raw
/// Input target per device type, so a later registration elsewhere in
/// Quill (tao's device events) takes the mouse away.
fn still_registered() -> bool {
    let Some(window) = window() else { return false };
    let size = std::mem::size_of::<RAWINPUTDEVICE>() as u32;
    let mut count = 0u32;
    unsafe { GetRegisteredRawInputDevices(None, &mut count, size) };
    let mut devices = vec![RAWINPUTDEVICE::default(); count as usize];
    let read = unsafe { GetRegisteredRawInputDevices(Some(devices.as_mut_ptr()), &mut count, size) };
    if read == u32::MAX {
        return false;
    }
    devices[..read as usize].iter().any(|d| d.usUsagePage == GENERIC_DESKTOP && d.usUsage == MOUSE && d.hwndTarget == window)
}

/// Pointer input is being counted: the devices are registered and still
/// report to the hidden window.
pub fn watching() -> bool {
    WATCHING.load(SeqCst) && still_registered()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
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
    fn the_hidden_window_receives_mouse_input_only_while_watching() {
        assert!(start(), "hidden window created");
        assert!(!watching(), "nothing registered at start");
        assert!(watch(), "raw input registered");
        assert!(watching(), "the mouse reports to the hidden window");
        let tag = 0x0BAD_F00D_u32;
        let step = |dx: i32| INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy: 0, mouseData: 0, dwFlags: MOUSEEVENTF_MOVE, time: 0, dwExtraInfo: tag as usize } },
        };
        let sent = unsafe { SendInput(&[step(1), step(-1)], std::mem::size_of::<INPUT>() as i32) };
        assert_eq!(sent, 2, "moves sent");
        let started = Instant::now();
        while LAST_EXTRA.load(SeqCst) != tag && started.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(10));
        }
        println!("raw input saw extra info {:#x} after {:?}", LAST_EXTRA.load(SeqCst), started.elapsed());
        assert_eq!(LAST_EXTRA.load(SeqCst), tag, "raw input arrived with its tag");

        stop_watching();
        let started = Instant::now();
        while watching() && started.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!watching() && !still_registered(), "unregistered after stop");
        LAST_EXTRA.store(0, SeqCst);
        let sent = unsafe { SendInput(&[step(1), step(-1)], std::mem::size_of::<INPUT>() as i32) };
        assert_eq!(sent, 2, "moves sent");
        std::thread::sleep(Duration::from_millis(200));
        println!("after stop, raw input saw extra info {:#x}", LAST_EXTRA.load(SeqCst));
        assert_eq!(LAST_EXTRA.load(SeqCst), 0, "nothing arrives while not watching");
    }
}
