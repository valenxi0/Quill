//! Global press-and-release hotkey through a low-level keyboard hook
//! (section 5.2). `RegisterHotKey` has no release event.
//!
//! The hook runs on its own thread with a message loop. The callback only
//! reads atomics and posts to a channel: Windows silently removes slow hooks.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::SeqCst};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::OnceLock;

use quill_core::settings::{is_modifier_vk, modifiers, Hotkey};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN,
    VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN,
    WM_SYSKEYUP,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    Pressed,
    Released,
    Escape,
    Captured(Hotkey),
    CaptureCancelled,
}

struct Shared {
    vk: AtomicU32,
    mods: AtomicU32,
    /// The chord is currently held down.
    down: AtomicBool,
    /// The main key went down as part of the chord and hasn't come up yet.
    /// Its repeats and key-up are swallowed even if a modifier came up first.
    latched: AtomicBool,
    /// Global hook paused (hotkey capture in Settings, or Quill off).
    suspended: AtomicBool,
    /// Settings is capturing a new shortcut.
    capturing: AtomicBool,
    /// Escape is intercepted only while dictation is busy.
    escape_active: AtomicBool,
    tx: Sender<HookEvent>,
}

static SHARED: OnceLock<Shared> = OnceLock::new();

/// Handle to the running hook.
#[derive(Clone, Copy)]
pub struct HotkeyHook;

impl HotkeyHook {
    /// Install the hook on a dedicated thread. Returns the event receiver.
    pub fn install(hotkey: Hotkey) -> anyhow::Result<(HotkeyHook, Receiver<HookEvent>)> {
        let (tx, rx) = channel();
        let shared = Shared {
            vk: AtomicU32::new(hotkey.vk),
            mods: AtomicU32::new(hotkey.modifiers),
            down: AtomicBool::new(false),
            latched: AtomicBool::new(false),
            suspended: AtomicBool::new(false),
            capturing: AtomicBool::new(false),
            escape_active: AtomicBool::new(false),
            tx,
        };
        SHARED.set(shared).map_err(|_| anyhow::anyhow!("hotkey hook already installed"))?;
        let (ready_tx, ready_rx) = channel();
        std::thread::Builder::new().name("quill-hotkey".into()).spawn(move || unsafe {
            let module = GetModuleHandleW(None).ok();
            let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module.map(Into::into), 0);
            let _ = ready_tx.send(hook.is_ok());
            if hook.is_err() {
                return;
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })?;
        if !ready_rx.recv().unwrap_or(false) {
            anyhow::bail!("Windows refused the keyboard hook");
        }
        Ok((HotkeyHook, rx))
    }

    fn shared() -> &'static Shared {
        SHARED.get().expect("hook installed")
    }

    pub fn set_hotkey(&self, hotkey: Hotkey) {
        let s = Self::shared();
        s.vk.store(hotkey.vk, SeqCst);
        s.mods.store(hotkey.modifiers, SeqCst);
        s.down.store(false, SeqCst);
        s.latched.store(false, SeqCst);
    }

    pub fn set_suspended(&self, suspended: bool) {
        let s = Self::shared();
        s.suspended.store(suspended, SeqCst);
        if suspended {
            s.down.store(false, SeqCst);
        }
    }

    pub fn set_capturing(&self, capturing: bool) {
        Self::shared().capturing.store(capturing, SeqCst);
    }

    pub fn set_escape_active(&self, active: bool) {
        Self::shared().escape_active.store(active, SeqCst);
    }
}

fn pressed(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0 }
}

fn current_modifiers() -> u32 {
    let mut m = 0;
    if pressed(VK_CONTROL) {
        m |= modifiers::CTRL;
    }
    if pressed(VK_MENU) {
        m |= modifiers::ALT;
    }
    if pressed(VK_SHIFT) {
        m |= modifiers::SHIFT;
    }
    if pressed(VK_LWIN) || pressed(VK_RWIN) {
        m |= modifiers::WIN;
    }
    m
}

/// Which modifier bit a key belongs to, if it is a modifier.
fn modifier_bit(vk: u32) -> u32 {
    let v = VIRTUAL_KEY(vk as u16);
    if v == VK_CONTROL || v == VK_LCONTROL || v == VK_RCONTROL {
        modifiers::CTRL
    } else if v == VK_MENU || v == VK_LMENU || v == VK_RMENU {
        modifiers::ALT
    } else if v == VK_SHIFT || v == VK_LSHIFT || v == VK_RSHIFT {
        modifiers::SHIFT
    } else if v == VK_LWIN || v == VK_RWIN {
        modifiers::WIN
    } else {
        0
    }
}

/// An unassigned key. Tapping it while a chord with Alt or Win is held keeps
/// the lone modifier release from opening a menu bar or the Start menu.
const VK_MASK: u16 = 0xE8;

fn tap_mask_key() {
    let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: VIRTUAL_KEY(VK_MASK), wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    };
    let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        if let Some(s) = SHARED.get() {
            let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            if info.flags.0 & LLKHF_INJECTED.0 == 0 && handle(s, info.vkCode, wparam.0 as u32) {
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// Returns true to swallow the key.
fn handle(s: &Shared, vk: u32, msg: u32) -> bool {
    let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;

    if s.capturing.load(SeqCst) {
        if is_down && !is_modifier_vk(vk) {
            let event = if vk == VK_ESCAPE.0 as u32 {
                HookEvent::CaptureCancelled
            } else {
                HookEvent::Captured(Hotkey { vk, modifiers: current_modifiers() })
            };
            let _ = s.tx.send(event);
            return true;
        }
        return false;
    }

    if vk == VK_ESCAPE.0 as u32 && s.escape_active.load(SeqCst) {
        if is_down {
            let _ = s.tx.send(HookEvent::Escape);
        }
        return true;
    }

    if s.suspended.load(SeqCst) {
        return false;
    }

    let want_vk = s.vk.load(SeqCst);
    let want_mods = s.mods.load(SeqCst);
    if vk == want_vk {
        if s.latched.load(SeqCst) {
            // Auto-repeat, or the key-up that ends the latch.
            if is_up {
                s.latched.store(false, SeqCst);
                if s.down.swap(false, SeqCst) {
                    let _ = s.tx.send(HookEvent::Released);
                }
            }
            return true;
        }
        if is_down && current_modifiers() == want_mods {
            s.latched.store(true, SeqCst);
            s.down.store(true, SeqCst);
            if want_mods & (modifiers::ALT | modifiers::WIN) != 0 {
                tap_mask_key();
            }
            let _ = s.tx.send(HookEvent::Pressed);
            return true;
        }
        return false;
    }

    // A required modifier going up also ends the chord; let it through.
    if is_up && s.down.load(SeqCst) && modifier_bit(vk) & want_mods != 0 {
        s.down.store(false, SeqCst);
        let _ = s.tx.send(HookEvent::Released);
    }
    false
}
