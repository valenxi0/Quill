//! Global press-and-release hotkey through a low-level keyboard hook
//! (section 5.2). `RegisterHotKey` has no release event.
//!
//! The hook runs on its own thread with a message loop. The callback only
//! reads atomics and posts to a channel: Windows silently removes slow hooks.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::SeqCst};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::OnceLock;
use std::time::Instant;

use super::escape::EscapeSequence;

use quill_core::settings::{is_modifier_vk, modifiers, Hotkey};
pub use quill_core::state::{Binding, Input};
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
    /// A dictation shortcut went down or up, or a lone-modifier one became a
    /// chord. `at` is `now_ms()` when the hook saw it, so a slow consumer
    /// can't stretch a tap.
    Shortcut { binding: Binding, input: Input, at: u64 },
    /// A timer the controller set is due, at `now_ms()`. It goes through the
    /// same channel so it is handled after every key event before it.
    Tick(u64),
    Escape,
    Captured(Hotkey),
    CaptureCancelled,
    /// The Sketch shortcut went down.
    Sketch,
    /// Ctrl+Z while a Sketch runs.
    SketchUndo,
}

/// One dictation shortcut's state.
struct Trigger {
    /// 0 when unset.
    vk: AtomicU32,
    mods: AtomicU32,
    /// The chord is currently held down.
    down: AtomicBool,
    /// The main key went down as part of the chord and hasn't come up yet.
    /// Its repeats and key-up are swallowed even if a modifier came up first.
    /// For a lone modifier: the key is physically down.
    latched: AtomicBool,
    /// A lone modifier's press already became a chord.
    chorded: AtomicBool,
}

impl Trigger {
    fn new(hotkey: Option<Hotkey>) -> Self {
        Self {
            vk: AtomicU32::new(hotkey.map_or(0, |h| h.vk)),
            mods: AtomicU32::new(hotkey.map_or(0, |h| h.modifiers)),
            down: AtomicBool::new(false),
            latched: AtomicBool::new(false),
            chorded: AtomicBool::new(false),
        }
    }

    fn is_lone(&self) -> bool {
        Hotkey { vk: self.vk.load(SeqCst), modifiers: self.mods.load(SeqCst) }.is_lone_modifier()
    }

    fn set(&self, hotkey: Option<Hotkey>) {
        self.vk.store(hotkey.map_or(0, |h| h.vk), SeqCst);
        self.mods.store(hotkey.map_or(0, |h| h.modifiers), SeqCst);
        self.down.store(false, SeqCst);
        self.latched.store(false, SeqCst);
    }
}

struct Shared {
    dictate: Trigger,
    /// Dictate without cleanup.
    raw: Trigger,
    /// Global hook paused (hotkey capture in Settings, or Quill off).
    suspended: AtomicBool,
    /// Settings is capturing a new shortcut.
    capturing: AtomicBool,
    modifier_keys: AtomicU32,
    capture_key: AtomicU32,
    /// Right Ctrl or Right Alt went down alone during capture; its release
    /// captures it.
    capture_lone: AtomicU32,
    /// Escape is intercepted only while dictation is busy.
    escape: EscapeSequence,
    /// The Sketch shortcut; vk 0 when Sketch is off.
    sketch_vk: AtomicU32,
    sketch_mods: AtomicU32,
    sketch_latched: AtomicBool,
    /// A Sketch is running: Ctrl+Z undoes its last mark.
    sketching: AtomicBool,
    tx: Sender<HookEvent>,
}

static SHARED: OnceLock<Shared> = OnceLock::new();
static EPOCH: OnceLock<Instant> = OnceLock::new();

/// Milliseconds on the clock shortcut events and ticks are stamped with.
pub fn now_ms() -> u64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64
}

fn shortcut(s: &Shared, binding: Binding, input: Input) {
    let _ = s.tx.send(HookEvent::Shortcut { binding, input, at: now_ms() });
}

/// Handle to the running hook.
#[derive(Clone, Copy)]
pub struct HotkeyHook;

impl HotkeyHook {
    /// Install the hook on a dedicated thread. Returns the event receiver.
    pub fn install(hotkey: Hotkey) -> anyhow::Result<(HotkeyHook, Receiver<HookEvent>)> {
        let (tx, rx) = channel();
        let shared = Shared {
            dictate: Trigger::new(Some(hotkey)),
            raw: Trigger::new(None),
            suspended: AtomicBool::new(false),
            capturing: AtomicBool::new(false),
            modifier_keys: AtomicU32::new(initial_modifier_keys()),
            capture_key: AtomicU32::new(0),
            capture_lone: AtomicU32::new(0),
            escape: EscapeSequence::default(),
            sketch_vk: AtomicU32::new(0),
            sketch_mods: AtomicU32::new(0),
            sketch_latched: AtomicBool::new(false),
            sketching: AtomicBool::new(false),
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
        Self::shared().dictate.set(Some(hotkey));
    }

    /// The no-cleanup shortcut, or `None` when it isn't set.
    pub fn set_raw_hotkey(&self, hotkey: Option<Hotkey>) {
        Self::shared().raw.set(hotkey);
    }

    pub fn set_suspended(&self, suspended: bool) {
        let s = Self::shared();
        s.suspended.store(suspended, SeqCst);
        if suspended {
            s.dictate.down.store(false, SeqCst);
            s.raw.down.store(false, SeqCst);
        }
    }

    pub fn set_capturing(&self, capturing: bool) {
        let s = Self::shared();
        s.capture_lone.store(0, SeqCst);
        s.capturing.store(capturing, SeqCst);
    }

    pub fn set_escape_active(&self, active: bool) {
        Self::shared().escape.set_active(active);
    }

    /// The Sketch shortcut, or `None` when Sketch is off.
    pub fn set_sketch_hotkey(&self, hotkey: Option<Hotkey>) {
        let s = Self::shared();
        s.sketch_vk.store(hotkey.map_or(0, |h| h.vk), SeqCst);
        s.sketch_mods.store(hotkey.map_or(0, |h| h.modifiers), SeqCst);
        s.sketch_latched.store(false, SeqCst);
    }

    /// Queue a `Tick` behind the key events already sent.
    pub fn post_tick(&self) {
        let _ = Self::shared().tx.send(HookEvent::Tick(now_ms()));
    }

    pub fn set_sketching(&self, active: bool) {
        Self::shared().sketching.store(active, SeqCst);
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

/// A mouse side button from the mouse hook. Returns true to swallow it.
pub fn mouse_button(vk: u32, is_down: bool) -> bool {
    SHARED.get().is_some_and(|s| handle(s, vk, if is_down { WM_KEYDOWN } else { WM_KEYUP }))
}

/// A click or the wheel: Ctrl+click and Ctrl+wheel are chords too.
pub fn mouse_chord() {
    if let Some(s) = SHARED.get() {
        mouse_chord_on(s);
    }
}

fn mouse_chord_on(s: &Shared) {
    if !s.suspended.load(SeqCst) {
        for (t, binding) in [(&s.dictate, Binding::Dictate), (&s.raw, Binding::Raw)] {
            chord(s, t, binding);
        }
    }
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
            s.capture_lone.store(0, SeqCst);
            s.capture_key.store(vk, SeqCst);
            let event = if vk == VK_ESCAPE.0 as u32 {
                HookEvent::CaptureCancelled
            } else {
                HookEvent::Captured(Hotkey { vk, modifiers: mods })
            };
            let _ = s.tx.send(event);
            return true;
        }
        capture_lone_modifier(s, vk, is_down, is_up);
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

    let triggers = [(&s.dictate, Binding::Dictate), (&s.raw, Binding::Raw)];
    // A lone modifier sees every key first, so a chord that Sketch or
    // another shortcut swallows still ends its press.
    for (t, binding) in triggers {
        if t.is_lone() {
            lone_modifier_key(s, t, binding, vk, is_down, is_up);
        }
    }

    let sketch_vk = s.sketch_vk.load(SeqCst);
    if sketch_vk != 0 && vk == sketch_vk {
        if s.sketch_latched.load(SeqCst) {
            if is_up {
                s.sketch_latched.store(false, SeqCst);
            }
            return true;
        }
        let want = s.sketch_mods.load(SeqCst);
        if is_down && mods == want {
            s.sketch_latched.store(true, SeqCst);
            if want & (modifiers::ALT | modifiers::WIN) != 0 {
                tap_mask_key();
            }
            let _ = s.tx.send(HookEvent::Sketch);
            return true;
        }
    }
    const VK_Z: u32 = 0x5A;
    if vk == VK_Z && is_down && mods == modifiers::CTRL && s.sketching.load(SeqCst) {
        let _ = s.tx.send(HookEvent::SketchUndo);
        return true;
    }

    triggers.into_iter().fold(false, |swallow, (t, binding)| trigger_key(s, t, binding, vk, is_down, is_up, mods) || swallow)
}

/// One shortcut's part in a key event. Returns true to swallow the key.
fn trigger_key(s: &Shared, t: &Trigger, binding: Binding, vk: u32, is_down: bool, is_up: bool, mods: u32) -> bool {
    let want_vk = t.vk.load(SeqCst);
    let want_mods = t.mods.load(SeqCst);
    if want_vk == 0 || t.is_lone() {
        return false;
    }
    if vk == want_vk {
        if t.latched.load(SeqCst) {
            // Auto-repeat, or the key-up that ends the latch.
            if is_up {
                t.latched.store(false, SeqCst);
                if t.down.swap(false, SeqCst) {
                    shortcut(s, binding, Input::Released);
                }
            }
            return true;
        }
        if is_down && mods == want_mods {
            t.latched.store(true, SeqCst);
            t.down.store(true, SeqCst);
            if want_mods & (modifiers::ALT | modifiers::WIN) != 0 {
                tap_mask_key();
            }
            shortcut(s, binding, Input::Pressed);
            return true;
        }
        return false;
    }

    // A required modifier going up also ends the chord; let it through.
    if is_up && t.down.load(SeqCst) && modifier_bit(vk) & want_mods != 0 && mods & want_mods != want_mods {
        t.down.store(false, SeqCst);
        shortcut(s, binding, Input::Released);
    }
    false
}

/// Whether a modifier other than `vk` is held.
fn other_modifiers_held(s: &Shared, vk: u32) -> bool {
    let own = MODIFIER_KEYS.iter().position(|&k| k == vk).map_or(0, |i| 1 << i);
    s.modifier_keys.load(SeqCst) & !own != 0
}

/// Right Ctrl or Right Alt as the whole shortcut. The key itself always
/// reaches the app, so Ctrl and Alt shortcuts keep working: another key
/// going down while it is held makes the press a chord.
fn lone_modifier_key(s: &Shared, t: &Trigger, binding: Binding, vk: u32, is_down: bool, is_up: bool) {
    if vk == t.vk.load(SeqCst) {
        if is_down && !t.latched.swap(true, SeqCst) && !other_modifiers_held(s, vk) {
            t.chorded.store(false, SeqCst);
            t.down.store(true, SeqCst);
            if vk == VK_RMENU.0 as u32 {
                // Keeps the release from opening the app's menu bar.
                tap_mask_key();
            }
            shortcut(s, binding, Input::Pressed);
        } else if is_up {
            t.latched.store(false, SeqCst);
            if t.down.swap(false, SeqCst) {
                shortcut(s, binding, Input::Released);
            }
        }
    } else if is_down && vk != VK_MASK as u32 {
        chord(s, t, binding);
    }
}

/// Once per press. The release still follows, to end a recording that
/// went on past the chord.
fn chord(s: &Shared, t: &Trigger, binding: Binding) {
    if t.is_lone() && t.down.load(SeqCst) && !t.chorded.swap(true, SeqCst) {
        shortcut(s, binding, Input::Chord);
    }
}

/// Settings captures Right Ctrl or Right Alt when one is pressed and
/// released with nothing else.
fn capture_lone_modifier(s: &Shared, vk: u32, is_down: bool, is_up: bool) {
    let lone = vk == VK_RCONTROL.0 as u32 || vk == VK_RMENU.0 as u32;
    if is_down && s.capture_lone.load(SeqCst) != vk {
        s.capture_lone.store(if lone && !other_modifiers_held(s, vk) { vk } else { 0 }, SeqCst);
    } else if is_up && lone && s.capture_lone.swap(0, SeqCst) == vk {
        let _ = s.tx.send(HookEvent::Captured(Hotkey { vk, modifiers: 0 }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(binding: Binding, input: Input) -> HookEvent {
        HookEvent::Shortcut { binding, input, at: 0 }
    }

    /// Events with their time stamps zeroed.
    fn drain(rx: &Receiver<HookEvent>) -> Vec<HookEvent> {
        rx.try_iter()
            .map(|e| match e {
                HookEvent::Shortcut { binding, input, .. } => ev(binding, input),
                e => e,
            })
            .collect()
    }

    fn shared() -> (Shared, Receiver<HookEvent>) {
        let (tx, rx) = channel();
        (Shared {
            dictate: Trigger::new(Some(Hotkey { vk: 0x4D, modifiers: modifiers::CTRL | modifiers::SHIFT })),
            raw: Trigger::new(None),
            suspended: AtomicBool::new(false),
            capturing: AtomicBool::new(false),
            modifier_keys: AtomicU32::new(0),
            capture_key: AtomicU32::new(0),
            capture_lone: AtomicU32::new(0),
            escape: EscapeSequence::default(),
            sketch_vk: AtomicU32::new(0x53),
            sketch_mods: AtomicU32::new(modifiers::CTRL | modifiers::SHIFT),
            sketch_latched: AtomicBool::new(false),
            sketching: AtomicBool::new(false),
            tx,
        }, rx)
    }

    #[test]
    fn sketch_shortcut_fires_once_and_undo_only_while_sketching() {
        let (s, rx) = shared();
        handle(&s, 0xA2, WM_KEYDOWN);
        handle(&s, 0xA0, WM_KEYDOWN);
        assert!(handle(&s, 0x53, WM_KEYDOWN));
        assert!(handle(&s, 0x53, WM_KEYDOWN), "repeat swallowed");
        assert!(handle(&s, 0x53, WM_KEYUP));
        assert_eq!(rx.try_recv().unwrap(), HookEvent::Sketch);
        assert!(rx.try_recv().is_err());
        handle(&s, 0xA0, WM_KEYUP);
        // Ctrl+Z belongs to the app unless a Sketch runs.
        assert!(!handle(&s, 0x5A, WM_KEYDOWN));
        s.sketching.store(true, SeqCst);
        assert!(handle(&s, 0x5A, WM_KEYDOWN));
        assert_eq!(rx.try_recv().unwrap(), HookEvent::SketchUndo);
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
        assert_eq!(drain(&rx), [ev(Binding::Dictate, Input::Pressed)]);
    }

    #[test]
    fn releasing_one_of_two_ctrl_keys_keeps_chord_active() {
        let (s, rx) = shared();
        handle(&s, 0xA2, WM_KEYDOWN);
        handle(&s, 0xA3, WM_KEYDOWN);
        handle(&s, 0xA0, WM_KEYDOWN);
        handle(&s, 0x4D, WM_KEYDOWN);
        assert_eq!(drain(&rx), [ev(Binding::Dictate, Input::Pressed)]);
        handle(&s, 0xA2, WM_KEYUP);
        assert!(rx.try_recv().is_err());
        handle(&s, 0xA3, WM_KEYUP);
        assert_eq!(drain(&rx), [ev(Binding::Dictate, Input::Released)]);
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

    const RCTRL: u32 = 0xA3;
    const RALT: u32 = 0xA5;
    const LCTRL: u32 = 0xA2;


    #[test]
    fn raw_shortcut_fires_as_its_own_binding() {
        let (s, rx) = shared();
        s.raw.set(Some(Hotkey { vk: 0x4B, modifiers: modifiers::CTRL | modifiers::SHIFT }));
        handle(&s, 0xA2, WM_KEYDOWN);
        handle(&s, 0xA0, WM_KEYDOWN);
        assert!(handle(&s, 0x4B, WM_KEYDOWN));
        assert!(handle(&s, 0x4B, WM_KEYDOWN), "repeat");
        assert!(handle(&s, 0x4B, WM_KEYUP));
        assert_eq!(drain(&rx), [ev(Binding::Raw, Input::Pressed), ev(Binding::Raw, Input::Released)]);
        assert!(handle(&s, 0x4D, WM_KEYDOWN));
        assert_eq!(drain(&rx), [ev(Binding::Dictate, Input::Pressed)]);
    }

    #[test]
    fn a_chord_another_shortcut_swallows_still_ends_a_lone_press() {
        let (s, rx) = shared();
        s.raw.set(Some(Hotkey { vk: RCTRL, modifiers: 0 }));
        handle(&s, RCTRL, WM_KEYDOWN);
        handle(&s, 0xA0, WM_KEYDOWN);
        assert!(handle(&s, 0x4D, WM_KEYDOWN));
        assert_eq!(drain(&rx), [
            ev(Binding::Raw, Input::Pressed),
            ev(Binding::Raw, Input::Chord),
            ev(Binding::Dictate, Input::Pressed),
        ]);
    }

    #[test]
    fn lone_right_ctrl_starts_on_press_and_never_swallows() {
        let (s, rx) = shared();
        s.dictate.set(Some(Hotkey { vk: RCTRL, modifiers: 0 }));
        assert!(!handle(&s, RCTRL, WM_KEYDOWN));
        assert!(!handle(&s, RCTRL, WM_KEYDOWN), "repeat");
        assert!(!handle(&s, RCTRL, WM_KEYUP));
        assert_eq!(drain(&rx), [ev(Binding::Dictate, Input::Pressed), ev(Binding::Dictate, Input::Released)]);
    }

    #[test]
    fn right_ctrl_plus_c_is_a_chord_and_passes_through() {
        let (s, rx) = shared();
        s.dictate.set(Some(Hotkey { vk: RCTRL, modifiers: 0 }));
        assert!(!handle(&s, RCTRL, WM_KEYDOWN));
        assert!(!handle(&s, 0x43, WM_KEYDOWN));
        assert!(!handle(&s, 0x43, WM_KEYUP));
        assert!(!handle(&s, RCTRL, WM_KEYDOWN), "repeat after the chord");
        assert!(!handle(&s, RCTRL, WM_KEYUP));
        assert_eq!(drain(&rx), [
            ev(Binding::Dictate, Input::Pressed),
            ev(Binding::Dictate, Input::Chord),
            ev(Binding::Dictate, Input::Released),
        ]);
    }

    #[test]
    fn clicks_and_the_wheel_are_chords_for_a_lone_modifier() {
        let (s, rx) = shared();
        mouse_chord_on(&s);
        assert!(drain(&rx).is_empty(), "no lone shortcut held");
        s.raw.set(Some(Hotkey { vk: RCTRL, modifiers: 0 }));
        handle(&s, RCTRL, WM_KEYDOWN);
        mouse_chord_on(&s);
        mouse_chord_on(&s);
        handle(&s, RCTRL, WM_KEYUP);
        mouse_chord_on(&s);
        assert_eq!(drain(&rx), [ev(Binding::Raw, Input::Pressed), ev(Binding::Raw, Input::Chord), ev(Binding::Raw, Input::Released)]);
    }

    #[test]
    fn a_lone_press_sees_the_sketch_shortcut_and_undo_as_chords() {
        let (s, rx) = shared();
        s.dictate.set(Some(Hotkey { vk: RCTRL, modifiers: 0 }));
        s.sketch_mods.store(modifiers::CTRL, SeqCst);
        handle(&s, RCTRL, WM_KEYDOWN);
        assert!(handle(&s, 0x53, WM_KEYDOWN));
        assert_eq!(drain(&rx), [ev(Binding::Dictate, Input::Pressed), ev(Binding::Dictate, Input::Chord), HookEvent::Sketch]);
        handle(&s, 0x53, WM_KEYUP);
        handle(&s, RCTRL, WM_KEYUP);
        drain(&rx);
        s.sketching.store(true, SeqCst);
        handle(&s, RCTRL, WM_KEYDOWN);
        assert!(handle(&s, 0x5A, WM_KEYDOWN));
        assert_eq!(drain(&rx), [ev(Binding::Dictate, Input::Pressed), ev(Binding::Dictate, Input::Chord), HookEvent::SketchUndo]);
    }

    #[test]
    fn shortcut_events_are_stamped_when_the_hook_sees_them() {
        let (s, rx) = shared();
        let before = now_ms();
        handle(&s, 0xA2, WM_KEYDOWN);
        handle(&s, 0xA0, WM_KEYDOWN);
        handle(&s, 0x4D, WM_KEYDOWN);
        std::thread::sleep(std::time::Duration::from_millis(60));
        handle(&s, 0x4D, WM_KEYUP);
        let times: Vec<u64> = rx.try_iter().filter_map(|e| match e { HookEvent::Shortcut { at, .. } => Some(at), _ => None }).collect();
        assert_eq!(times.len(), 2);
        assert!(times[0] >= before && times[0] <= before + 30, "{times:?} from {before}");
        assert!(times[1] >= times[0] + 60, "{times:?}");
    }

    #[test]
    fn right_ctrl_with_another_modifier_held_is_not_the_shortcut() {
        let (s, rx) = shared();
        s.dictate.set(Some(Hotkey { vk: RCTRL, modifiers: 0 }));
        handle(&s, 0xA0, WM_KEYDOWN);
        handle(&s, RCTRL, WM_KEYDOWN);
        handle(&s, 0xA0, WM_KEYUP);
        handle(&s, RCTRL, WM_KEYDOWN);
        handle(&s, RCTRL, WM_KEYUP);
        assert!(drain(&rx).is_empty());
        // AltGr arrives as Left Ctrl, then Right Alt.
        s.dictate.set(Some(Hotkey { vk: RALT, modifiers: 0 }));
        handle(&s, LCTRL, WM_KEYDOWN);
        handle(&s, RALT, WM_SYSKEYDOWN);
        handle(&s, RALT, WM_SYSKEYUP);
        handle(&s, LCTRL, WM_KEYUP);
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn mouse_side_button_is_swallowed_only_when_it_is_the_shortcut() {
        let (s, rx) = shared();
        assert!(!handle(&s, 0x05, WM_KEYDOWN));
        assert!(!handle(&s, 0x05, WM_KEYUP));
        s.raw.set(Some(Hotkey { vk: 0x05, modifiers: 0 }));
        assert!(handle(&s, 0x05, WM_KEYDOWN));
        assert!(handle(&s, 0x05, WM_KEYUP));
        assert!(!handle(&s, 0x06, WM_KEYDOWN), "the other side button");
        assert_eq!(drain(&rx), [ev(Binding::Raw, Input::Pressed), ev(Binding::Raw, Input::Released)]);
        handle(&s, LCTRL, WM_KEYDOWN);
        assert!(!handle(&s, 0x05, WM_KEYDOWN), "Ctrl+button is another shortcut");
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn capture_takes_lone_right_modifiers_and_side_buttons() {
        let (s, rx) = shared();
        s.capturing.store(true, SeqCst);
        assert!(!handle(&s, RCTRL, WM_KEYDOWN));
        assert!(!handle(&s, RCTRL, WM_KEYDOWN));
        assert!(drain(&rx).is_empty(), "nothing until release");
        assert!(!handle(&s, RCTRL, WM_KEYUP));
        assert_eq!(drain(&rx), [HookEvent::Captured(Hotkey { vk: RCTRL, modifiers: 0 })]);
        handle(&s, RALT, WM_SYSKEYDOWN);
        handle(&s, 0x4B, WM_SYSKEYDOWN);
        handle(&s, 0x4B, WM_SYSKEYUP);
        handle(&s, RALT, WM_SYSKEYUP);
        assert_eq!(drain(&rx), [HookEvent::Captured(Hotkey { vk: 0x4B, modifiers: modifiers::ALT })]);
        handle(&s, LCTRL, WM_KEYDOWN);
        handle(&s, LCTRL, WM_KEYUP);
        assert!(drain(&rx).is_empty(), "left Ctrl alone");
        assert!(handle(&s, 0x06, WM_KEYDOWN));
        assert!(handle(&s, 0x06, WM_KEYUP));
        assert_eq!(drain(&rx), [HookEvent::Captured(Hotkey { vk: 0x06, modifiers: 0 })]);
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
