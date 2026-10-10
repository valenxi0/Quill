use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::SeqCst};

const DOUBLE_PRESS_MS: u32 = 1_000;

#[derive(Default)]
pub struct EscapeSequence {
    active: AtomicBool,
    down: AtomicBool,
    armed: AtomicBool,
    first_press_at: AtomicU32,
}

impl EscapeSequence {
    pub fn set_active(&self, active: bool) {
        if self.active.swap(active, SeqCst) != active || !active {
            self.armed.store(false, SeqCst);
        }
    }

    pub fn intercepts(&self) -> bool {
        self.active.load(SeqCst) || self.down.load(SeqCst)
    }

    pub fn key_down(&self, now: u32) -> bool {
        if self.down.swap(true, SeqCst) || !self.active.load(SeqCst) {
            return false;
        }
        if self.armed.load(SeqCst)
            && now.wrapping_sub(self.first_press_at.load(SeqCst)) <= DOUBLE_PRESS_MS
        {
            self.armed.store(false, SeqCst);
            return true;
        }
        self.first_press_at.store(now, SeqCst);
        self.armed.store(true, SeqCst);
        false
    }

    pub fn key_up(&self) {
        self.down.store(false, SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_distinct_presses_cancel() {
        let escape = EscapeSequence::default();
        escape.set_active(true);
        assert!(!escape.key_down(100));
        escape.key_up();
        assert!(escape.key_down(500));
    }

    #[test]
    fn holding_escape_does_not_cancel() {
        let escape = EscapeSequence::default();
        escape.set_active(true);
        for now in [100, 200, 500, 900, 1_200] {
            assert!(!escape.key_down(now));
        }
    }

    #[test]
    fn late_second_press_starts_a_new_pair() {
        let escape = EscapeSequence::default();
        escape.set_active(true);
        assert!(!escape.key_down(100));
        escape.key_up();
        assert!(!escape.key_down(1_101));
        escape.key_up();
        assert!(escape.key_down(1_400));
    }

    #[test]
    fn new_session_forgets_the_first_press() {
        let escape = EscapeSequence::default();
        escape.set_active(true);
        assert!(!escape.key_down(100));
        escape.key_up();
        escape.set_active(false);
        escape.set_active(true);
        assert!(!escape.key_down(200));
        escape.key_up();
        assert!(escape.key_down(300));
    }

    #[test]
    fn busy_phase_changes_keep_the_first_press() {
        let escape = EscapeSequence::default();
        escape.set_active(true);
        assert!(!escape.key_down(100));
        escape.key_up();
        escape.set_active(true);
        assert!(escape.key_down(300));
    }

    #[test]
    fn key_up_is_swallowed_after_cancellation() {
        let escape = EscapeSequence::default();
        assert!(!escape.intercepts());
        escape.set_active(true);
        assert!(!escape.key_down(100));
        escape.key_up();
        assert!(escape.key_down(300));
        escape.set_active(false);
        assert!(escape.intercepts());
        assert!(!escape.key_down(400));
        escape.key_up();
        assert!(!escape.intercepts());
    }

    #[test]
    fn tick_counter_wrap_does_not_break_the_pair() {
        let escape = EscapeSequence::default();
        escape.set_active(true);
        assert!(!escape.key_down(u32::MAX - 100));
        escape.key_up();
        assert!(escape.key_down(100));
    }
}
