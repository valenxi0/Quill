//! Low-level mouse hooks: one for Sketch, installed only while one runs, and
//! one for the dictation shortcut's mouse needs. Like the keyboard hook, the
//! callbacks only read state and post to a channel: Windows silently removes
//! slow hooks. Each runs on a raised-priority thread, since every pointer
//! move waits for it.

use std::sync::mpsc::Sender;

use parking_lot::Mutex;
use quill_core::settings::{modifiers, VK_XBUTTON1, VK_XBUTTON2};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_HIGHEST};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HOOKPROC, LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDOWN, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_QUIT, WM_RBUTTONDOWN, WM_XBUTTONDOWN, WM_XBUTTONUP,
    XBUTTON1, XBUTTON2,
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

struct HookThread {
    thread_id: u32,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl HookThread {
    fn start(name: &str, proc: HOOKPROC) -> anyhow::Result<HookThread> {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new().name(name.into()).spawn(move || unsafe {
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
            let module = GetModuleHandleW(None).ok();
            let hook = SetWindowsHookExW(WH_MOUSE_LL, proc, module.map(Into::into), 0);
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
            Some(thread_id) => Ok(HookThread { thread_id, thread: Some(thread) }),
            None => anyhow::bail!("Windows refused the mouse hook"),
        }
    }
}

impl Drop for HookThread {
    fn drop(&mut self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub struct MouseHook {
    _thread: HookThread,
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
        match HookThread::start("quill-sketch-mouse", Some(hook_proc)) {
            Ok(thread) => Ok(MouseHook { _thread: thread }),
            Err(e) => {
                *HOOK.lock() = None;
                Err(e)
            }
        }
    }
}

impl Drop for MouseHook {
    fn drop(&mut self) {
        *HOOK.lock() = None;
    }
}

pub struct ButtonHook {
    _thread: HookThread,
}

unsafe extern "system" fn button_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let msg = wparam.0 as u32;
    if code >= 0 {
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        if info.flags & LLMHF_INJECTED == 0 {
            match msg {
                WM_XBUTTONDOWN | WM_XBUTTONUP => {
                    let vk = match (info.mouseData >> 16) as u16 {
                        XBUTTON1 => VK_XBUTTON1,
                        XBUTTON2 => VK_XBUTTON2,
                        _ => 0,
                    };
                    let down = msg == WM_XBUTTONDOWN;
                    if vk != 0 && super::hotkey::mouse_button(vk, down) {
                        if down {
                            super::input_activity::shortcut_button_down();
                        }
                        return LRESULT(1);
                    }
                }
                WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_MOUSEWHEEL | WM_MOUSEHWHEEL => super::hotkey::mouse_chord(),
                _ => {}
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

impl ButtonHook {
    pub fn install() -> anyhow::Result<ButtonHook> {
        Ok(ButtonHook { _thread: HookThread::start("quill-mouse-shortcut", Some(button_proc))? })
    }
}
