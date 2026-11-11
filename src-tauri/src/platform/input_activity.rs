//!

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::SeqCst};

use quill_core::settings::is_modifier_vk;

use super::clicks;

pub const QUILL_INPUT: usize = 0x5155_494C;

static KEYBOARD_HOOKED: AtomicBool = AtomicBool::new(false);
static KEYS: AtomicU32 = AtomicU32::new(0);
static POINTER: AtomicU32 = AtomicU32::new(0);
static SHORTCUT_BUTTONS: AtomicU32 = AtomicU32::new(0);
/// Watches that ended. Pointer input between two watches goes unseen, so
/// each end moves the count: a later watch never matches one from before.
static WATCH_ENDS: AtomicU32 = AtomicU32::new(0);

/// The keyboard hook is running. Windows can still remove a hook it finds
/// slow without saying so; that isn't detected.
pub fn keyboard_hooked() {
    KEYBOARD_HOOKED.store(true, SeqCst);
}

pub fn key_down(vk: u32, extra: usize) {
    if extra != QUILL_INPUT && !is_modifier_vk(vk) {
        KEYS.fetch_add(1, SeqCst);
        clicks::stop_watching();
    }
}

pub fn pointer_input() {
    POINTER.fetch_add(1, SeqCst);
}

pub fn shortcut_button_down() {
    SHORTCUT_BUTTONS.fetch_add(1, SeqCst);
}

pub fn watch_ended() {
    WATCH_ENDS.fetch_add(1, SeqCst);
}

pub fn watch() {
    clicks::watch();
}

pub fn stop() {
    clicks::stop_watching();
}

pub fn count() -> Option<u32> {
    (KEYBOARD_HOOKED.load(SeqCst) && clicks::watching())
        .then(|| combine(KEYS.load(SeqCst), POINTER.load(SeqCst), SHORTCUT_BUTTONS.load(SeqCst), WATCH_ENDS.load(SeqCst)))
}

fn combine(keys: u32, pointer: u32, shortcut_buttons: u32, watch_ends: u32) -> u32 {
    keys.wrapping_add(pointer).wrapping_sub(shortcut_buttons).wrapping_add(watch_ends)
}

#[cfg(test)]
mod tests {
    use super::combine;

    #[test]
    fn a_side_button_shortcut_is_not_user_input() {
        let before = combine(40, 7, 2, 5);
        assert_eq!(combine(40, 8, 3, 5), before, "a shortcut press, seen by Raw Input too");
        assert_eq!(combine(40, 8, 2, 5), before + 1, "a real click");
        assert_eq!(combine(41, 7, 2, 5), before + 1, "a key");
        assert_eq!(combine(0, 0, 1, 0), u32::MAX, "wraps like the counts it combines");
    }

    #[test]
    fn a_gap_between_watches_never_matches() {
        let at_paste = combine(40, 7, 2, 5);
        assert_ne!(combine(40, 7, 2, 6), at_paste, "the watch ended and another began, with no input seen");
        assert_ne!(combine(40, 8, 2, 6), at_paste, "a click ends the watch too");
    }
}
