//! Delivery: the clipboard is authoritative, the paste is best effort
//! (section 4.2, step 6).

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{GlobalFree, HANDLE, HWND};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;
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
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    if !open_clipboard() {
        return false;
    }
    let ok = unsafe {
        let _ = EmptyClipboard();
        match GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
            Ok(mem) => {
                let ptr = GlobalLock(mem) as *mut u16;
                if ptr.is_null() {
                    let _ = GlobalFree(Some(mem));
                    false
                } else {
                    std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                    let _ = GlobalUnlock(mem);
                    if SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(mem.0))).is_ok() {
                        true
                    } else {
                        let _ = GlobalFree(Some(mem));
                        false
                    }
                }
            }
            Err(_) => false,
        }
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
                dwExtraInfo: 0,
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

/// Send Ctrl+V to the captured target, or the current foreground window if
/// the target is gone. Returns whether a paste was sent.
pub fn paste(target: &Target, own_windows: &[isize]) -> bool {
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
    if !bring_to_front(hwnd) {
        return false;
    }
    wait_for_modifiers_up(Duration::from_millis(600));
    // Anything still held is released first.
    let mut inputs: Vec<INPUT> = MODIFIERS.iter().filter(|m| is_down(**m)).map(|m| key(*m, true)).collect();
    inputs.extend([key(VK_CONTROL, false), key(VK_V, false), key(VK_V, true), key(VK_CONTROL, true)]);
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    sent as usize == inputs.len()
}
