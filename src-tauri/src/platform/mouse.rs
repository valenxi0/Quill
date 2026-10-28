//! A low-level mouse hook for Sketch, installed only while one runs. Like
//! the keyboard hook, the callback only reads key state and posts to a
//! channel: Windows silently removes slow hooks.

use std::sync::mpsc::Sender;

use parking_lot::Mutex;
use quill_core::settings::modifiers;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, MSG, MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEHWHEEL,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_QUIT, WM_RBUTTONDOWN,
};

/// Positions are physical screen pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MouseEvent {
    /// `draw`: the draw key, and no other modifier, is held.
    Move { x: i32, y: i32, draw: bool },
    LeftDown { x: i32, y: i32 },
    LeftUp { x: i32, y: i32 },
    /// A right or middle click, or a scroll: the screen may change.
    Other,
}

struct Hook {
    tx: Sender<MouseEvent>,
    draw_key: u32,
}

static HOOK: Mutex<Option<Hook>> = Mutex::new(None);

pub struct MouseHook {
    thread_id: u32,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn down(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0 }
}

/// The modifier bits held now.
fn held() -> u32 {
    let mut m = 0;
    if down(VK_CONTROL) {
        m |= modifiers::CTRL;
    }
    if down(VK_MENU) {
        m |= modifiers::ALT;
    }
    if down(VK_SHIFT) {
        m |= modifiers::SHIFT;
    }
    if down(VK_LWIN) || down(VK_RWIN) {
        m |= modifiers::WIN;
    }
    m
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        let (x, y) = (info.pt.x, info.pt.y);
        let event = match wparam.0 as u32 {
            WM_MOUSEMOVE => Some(MouseEvent::Move { x, y, draw: false }),
            WM_LBUTTONDOWN => Some(MouseEvent::LeftDown { x, y }),
            WM_LBUTTONUP => Some(MouseEvent::LeftUp { x, y }),
            WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_MOUSEWHEEL | WM_MOUSEHWHEEL => Some(MouseEvent::Other),
            _ => None,
        };
        if let Some(mut event) = event {
            if let Some(hook) = HOOK.lock().as_ref() {
                if let MouseEvent::Move { draw, .. } = &mut event {
                    *draw = held() == hook.draw_key;
                }
                let _ = hook.tx.send(event);
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

impl MouseHook {
    /// `draw_key` is a single modifier bit (`modifiers::SHIFT` and so on).
    pub fn install(draw_key: u32, tx: Sender<MouseEvent>) -> anyhow::Result<MouseHook> {
        *HOOK.lock() = Some(Hook { tx, draw_key });
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new().name("quill-sketch-mouse".into()).spawn(move || unsafe {
            let module = GetModuleHandleW(None).ok();
            let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(hook_proc), module.map(Into::into), 0);
            let _ = ready_tx.send(hook.as_ref().ok().map(|_| GetCurrentThreadId()));
            let Ok(hook) = hook else { return };
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let _ = UnhookWindowsHookEx(hook);
        })?;
        match ready_rx.recv().ok().flatten() {
            Some(thread_id) => Ok(MouseHook { thread_id, thread: Some(thread) }),
            None => {
                *HOOK.lock() = None;
                anyhow::bail!("Windows refused the mouse hook")
            }
        }
    }
}

impl Drop for MouseHook {
    fn drop(&mut self) {
        *HOOK.lock() = None;
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
