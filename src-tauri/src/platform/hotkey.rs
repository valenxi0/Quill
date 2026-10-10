//! Global press-and-release hotkey through a low-level keyboard hook
//! (section 5.2). `RegisterHotKey` has no release event.
//!
//! The hook runs on its own thread with a message loop. The callback only
//! reads atomics and posts to a channel: Windows silently removes slow hooks.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::SeqCst};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::OnceLock;

use super::escape::EscapeSequence;

use quill_core::settings::{is_modifier_vk, modifiers, Hotkey};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount;
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
    modifier_keys: AtomicU32,
    capture_key: AtomicU32,
    /// Escape is intercepted only while dictation is busy.
    escape: EscapeSequence,
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
            modifier_keys: AtomicU32::new(initial_modifier_keys()),
            capture_key: AtomicU32::new(0),
            escape: EscapeSequence::default(),
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
        Self::shared().escape.set_active(active);
    }
}

fn pressed(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0 }
}

const MODIFIER_KEYS: [u32; 11] = [0x11, 0xA2, 0xA3, 0x12, 0xA4, 0xA5, 0x10, 0xA0, 0xA1, 0x5B, 0x5C];

fn initial_modifier_keys() -> u32 {
    MODIFIER_KEYS.iter().enumerate().fold(0, |bits, (i, &vk)| {
        // Side-specific keys seed the state before the hook starts.
        if i != 0 && i != 3 && i != 6 && pressed(VIRTUAL_KEY(vk as u16)) { bits | (1 << i) } else { bits }
    })
}

fn current_modifiers(keys: u32) -> u32 {
    let mut m = 0;
    if keys & 0b111 != 0 {
        m |= modifiers::CTRL;
    }
    if keys & (0b111 << 3) != 0 {
        m |= modifiers::ALT;
    }
    if keys & (0b111 << 6) != 0 {
        m |= modifiers::SHIFT;
    }
    if keys & (0b11 << 9) != 0 {
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

    if let Some(i) = MODIFIER_KEYS.iter().position(|&key| key == vk) {
        if is_down { s.modifier_keys.fetch_or(1 << i, SeqCst); }
        if is_up { s.modifier_keys.fetch_and(!(1 << i), SeqCst); }
    }
    let mods = current_modifiers(s.modifier_keys.load(SeqCst));
    if s.capture_key.load(SeqCst) == vk && vk != 0 {
        if is_up { s.capture_key.store(0, SeqCst); }
        return true;
    }

    if s.capturing.load(SeqCst) {
        if is_down && !is_modifier_vk(vk) {
            // Keep repeats and key-up out of both capture and dictation.
            s.capture_key.store(vk, SeqCst);
            let event = if vk == VK_ESCAPE.0 as u32 {
                HookEvent::CaptureCancelled
            } else {
                HookEvent::Captured(Hotkey { vk, modifiers: mods })
            };
            let _ = s.tx.send(event);
            return true;
        }
        return false;
    }

    if vk == VK_ESCAPE.0 as u32 && s.escape.intercepts() {
        if is_down && s.escape.key_down(unsafe { GetTickCount() }) {
            let _ = s.tx.send(HookEvent::Escape);
        }
        if is_up {
            s.escape.key_up();
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
        if is_down && mods == want_mods {
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
    if is_up && s.down.load(SeqCst) && modifier_bit(vk) & want_mods != 0 && mods & want_mods != want_mods {
        s.down.store(false, SeqCst);
        let _ = s.tx.send(HookEvent::Released);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared() -> (Shared, Receiver<HookEvent>) {
        let (tx, rx) = channel();
        (Shared {
            vk: AtomicU32::new(0x4D),
            mods: AtomicU32::new(modifiers::CTRL | modifiers::SHIFT),
            down: AtomicBool::new(false),
            latched: AtomicBool::new(false),
            suspended: AtomicBool::new(false),
            capturing: AtomicBool::new(false),
            modifier_keys: AtomicU32::new(0),
            capture_key: AtomicU32::new(0),
            escape: EscapeSequence::default(),
            tx,
        }, rx)
    }

    #[test]
    fn captures_right_side_modifiers_from_events() {
        let (s, rx) = shared();
        s.capturing.store(true, SeqCst);
        handle(&s, 0xA3, WM_KEYDOWN);
        handle(&s, 0xA5, WM_SYSKEYDOWN);
        assert!(handle(&s, 0x4B, WM_SYSKEYDOWN));
        assert_eq!(rx.try_recv().unwrap(), HookEvent::Captured(Hotkey {
            vk: 0x4B, modifiers: modifiers::CTRL | modifiers::ALT,
        }));
    }

    #[test]
    fn captured_key_cannot_repeat_or_start_dictation_after_save() {
        let (s, rx) = shared();
        s.capturing.store(true, SeqCst);
        handle(&s, 0xA2, WM_KEYDOWN);
        handle(&s, 0xA0, WM_KEYDOWN);
        handle(&s, 0x4D, WM_KEYDOWN);
        assert!(matches!(rx.try_recv(), Ok(HookEvent::Captured(_))));
        assert!(handle(&s, 0x4D, WM_KEYDOWN));
        s.capturing.store(false, SeqCst);
        assert!(handle(&s, 0x4D, WM_KEYDOWN));
        assert!(handle(&s, 0x4D, WM_KEYUP));
        assert!(rx.try_recv().is_err());
        assert!(handle(&s, 0x4D, WM_KEYDOWN));
        assert_eq!(rx.try_recv().unwrap(), HookEvent::Pressed);
    }

    #[test]
    fn releasing_one_of_two_ctrl_keys_keeps_chord_active() {
        let (s, rx) = shared();
        handle(&s, 0xA2, WM_KEYDOWN);
        handle(&s, 0xA3, WM_KEYDOWN);
        handle(&s, 0xA0, WM_KEYDOWN);
        handle(&s, 0x4D, WM_KEYDOWN);
        assert_eq!(rx.try_recv().unwrap(), HookEvent::Pressed);
        handle(&s, 0xA2, WM_KEYUP);
        assert!(rx.try_recv().is_err());
        handle(&s, 0xA3, WM_KEYUP);
        assert_eq!(rx.try_recv().unwrap(), HookEvent::Released);
        assert!(handle(&s, 0x4D, WM_KEYUP));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn invalid_capture_can_be_retried_after_release() {
        let (s, rx) = shared();
        s.capturing.store(true, SeqCst);
        handle(&s, 0x4B, WM_KEYDOWN);
        let HookEvent::Captured(key) = rx.try_recv().unwrap() else { panic!("capture expected") };
        assert!(key.problem().is_some());
        handle(&s, 0x4B, WM_KEYUP);
        handle(&s, 0xA2, WM_KEYDOWN);
        handle(&s, 0x4B, WM_KEYDOWN);
        let HookEvent::Captured(key) = rx.try_recv().unwrap() else { panic!("capture expected") };
        assert!(key.problem().is_none());
    }

    #[test]
    fn escape_cancels_capture_once_without_leaking_key_up() {
        let (s, rx) = shared();
        s.capturing.store(true, SeqCst);
        assert!(handle(&s, 0x1B, WM_KEYDOWN));
        assert_eq!(rx.try_recv().unwrap(), HookEvent::CaptureCancelled);
        s.capturing.store(false, SeqCst);
        assert!(handle(&s, 0x1B, WM_KEYDOWN));
        assert!(handle(&s, 0x1B, WM_KEYUP));
        assert!(rx.try_recv().is_err());
    }
}
