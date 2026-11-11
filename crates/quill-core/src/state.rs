//! Dictation phases and how hotkey events map to actions (section 4.1).

use std::time::Duration;

use serde::Serialize;

use crate::settings::ActivationMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Idle,
    Recording,
    Transcribing,
    Cleaning,
    Completed,
    Failed,
    Cancelled,
}

impl Phase {
    pub fn is_busy(self) -> bool {
        matches!(self, Phase::Recording | Phase::Transcribing | Phase::Cleaning)
    }

    /// How long an outcome stays on screen before returning to idle.
    pub fn hold(self) -> Option<Duration> {
        match self {
            Phase::Completed => Some(Duration::from_millis(1600)),
            Phase::Failed => Some(Duration::from_millis(3000)),
            Phase::Cancelled => Some(Duration::from_millis(1300)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    Pressed,
    Released,
    Escape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Start,
    Stop,
    Cancel,
    /// Drop the recording without showing anything.
    Discard,
    Ignore,
}

/// Which shortcut fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    Dictate,
    /// Dictate without cleanup.
    Raw,
}

pub fn action(mode: ActivationMode, phase: Phase, event: HotkeyEvent) -> Action {
    use Action::*;
    match (event, mode) {
        (HotkeyEvent::Escape, _) => {
            if phase.is_busy() {
                Cancel
            } else {
                Ignore
            }
        }
        (HotkeyEvent::Pressed, ActivationMode::PushToTalk) => {
            if phase.is_busy() {
                Ignore
            } else {
                Start
            }
        }
        (HotkeyEvent::Released, ActivationMode::PushToTalk) => {
            if phase == Phase::Recording {
                Stop
            } else {
                Ignore
            }
        }
        (HotkeyEvent::Pressed, ActivationMode::Toggle) => {
            if phase == Phase::Recording {
                Stop
            } else if phase.is_busy() {
                Ignore
            } else {
                Start
            }
        }
        (HotkeyEvent::Released, ActivationMode::Toggle) => Ignore,
        // Hold-or-lock's timing lives in `Activation`; without it, it holds.
        (_, ActivationMode::HoldOrLock) => action(ActivationMode::PushToTalk, phase, event),
    }
}

/// In hold-or-lock, a press shorter than this is a tap, and a second press
/// this soon after a tap locks the recording.
pub const TAP_MS: u64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Pressed,
    Released,
    Escape,
    /// A timer the caller set from a returned deadline went off.
    Tick,
    /// Another key went down while a lone-modifier shortcut was held.
    Chord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Stage {
    #[default]
    Idle,
    /// Recording while the shortcut is held.
    Held { since: u64 },
    /// After a tap: a second press before `until` locks the recording.
    Waiting { until: u64 },
    /// Recording hands-free until the next press.
    Locked,
}

/// Turns shortcut presses into actions, with time passed in so it stays
/// pure. Push to talk and Toggle go through `action` unchanged.
#[derive(Debug, Clone, Default)]
pub struct Activation {
    stage: Stage,
    /// The press now held started or locked the recording.
    fresh: bool,
    /// A lone-modifier press that would stop waits for its release, so
    /// Ctrl+C with Right Ctrl never finishes a dictation.
    deferred_stop: bool,
}

impl Activation {
    /// `lone`: the shortcut is a modifier pressed alone, which can turn out
    /// to be part of a key chord. Returns the action and, while waiting for
    /// a second tap, the time to send `Tick` at.
    pub fn on(&mut self, mode: ActivationMode, lone: bool, input: Input, now: u64, phase: Phase) -> (Action, Option<u64>) {
        if phase != Phase::Recording {
            self.stage = Stage::Idle;
        }
        match input {
            Input::Escape => {
                *self = Self::default();
                (action(mode, phase, HotkeyEvent::Escape), None)
            }
            Input::Chord => {
                let undo = self.fresh && phase == Phase::Recording;
                self.fresh = false;
                self.deferred_stop = false;
                if undo {
                    self.stage = Stage::Idle;
                    (Action::Discard, None)
                } else {
                    (Action::Ignore, None)
                }
            }
            Input::Pressed if lone => {
                let mut next = self.clone();
                let (act, deadline) = next.step(mode, input, now, phase);
                if act == Action::Stop {
                    self.deferred_stop = true;
                    return (Action::Ignore, None);
                }
                next.fresh = act == Action::Start || matches!(self.stage, Stage::Waiting { .. });
                *self = next;
                (act, deadline)
            }
            Input::Released if lone && self.deferred_stop => {
                self.deferred_stop = false;
                self.fresh = false;
                if phase == Phase::Recording {
                    self.step(mode, Input::Pressed, now, phase)
                } else {
                    (Action::Ignore, None)
                }
            }
            _ => {
                if input == Input::Released {
                    self.fresh = false;
                }
                self.step(mode, input, now, phase)
            }
        }
    }

    fn step(&mut self, mode: ActivationMode, input: Input, now: u64, phase: Phase) -> (Action, Option<u64>) {
        if mode != ActivationMode::HoldOrLock {
            let act = match input {
                Input::Pressed => action(mode, phase, HotkeyEvent::Pressed),
                Input::Released => action(mode, phase, HotkeyEvent::Released),
                _ => Action::Ignore,
            };
            return (act, None);
        }
        use Action::*;
        match (self.stage, input) {
            (Stage::Idle, Input::Pressed) if !phase.is_busy() => {
                self.stage = Stage::Held { since: now };
                (Start, None)
            }
            (Stage::Held { since }, Input::Released) => {
                if now.saturating_sub(since) >= TAP_MS {
                    self.stage = Stage::Idle;
                    (Stop, None)
                } else {
                    let until = now + TAP_MS;
                    self.stage = Stage::Waiting { until };
                    (Ignore, Some(until))
                }
            }
            (Stage::Waiting { until }, Input::Pressed) => {
                // A late press, before the timer ran, holds the same recording.
                self.stage = if now <= until { Stage::Locked } else { Stage::Held { since: now } };
                (Ignore, None)
            }
            (Stage::Waiting { until }, Input::Tick) => {
                if now >= until {
                    self.stage = Stage::Idle;
                    (Discard, None)
                } else {
                    (Ignore, Some(until))
                }
            }
            (Stage::Locked, Input::Pressed) => {
                self.stage = Stage::Idle;
                (Stop, None)
            }
            _ => (Ignore, None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ActivationMode::*;
    use HotkeyEvent::*;
    use Phase::*;

    #[test]
    fn activation_table() {
        let cases = [
            (PushToTalk, Idle, Pressed, Action::Start),
            (PushToTalk, Completed, Pressed, Action::Start),
            (PushToTalk, Recording, Pressed, Action::Ignore),
            (PushToTalk, Transcribing, Pressed, Action::Ignore),
            (PushToTalk, Recording, Released, Action::Stop),
            (PushToTalk, Idle, Released, Action::Ignore),
            (PushToTalk, Cleaning, Released, Action::Ignore),
            (Toggle, Idle, Pressed, Action::Start),
            (Toggle, Failed, Pressed, Action::Start),
            (Toggle, Recording, Pressed, Action::Stop),
            (Toggle, Transcribing, Pressed, Action::Ignore),
            (Toggle, Recording, Released, Action::Ignore),
            (PushToTalk, Recording, Escape, Action::Cancel),
            (Toggle, Cleaning, Escape, Action::Cancel),
            (Toggle, Idle, Escape, Action::Ignore),
            (PushToTalk, Cancelled, Escape, Action::Ignore),
        ];
        for (mode, phase, event, want) in cases {
            assert_eq!(action(mode, phase, event), want, "{mode:?} {phase:?} {event:?}");
        }
    }

    /// Feeds `(input, ms, phase, want, deadline)` steps through one machine.
    fn run(mode: ActivationMode, lone: bool, steps: &[(Input, u64, Phase, Action, Option<u64>)]) {
        let mut m = Activation::default();
        for (i, &(input, now, phase, want, deadline)) in steps.iter().enumerate() {
            assert_eq!(m.on(mode, lone, input, now, phase), (want, deadline), "step {i}: {input:?} at {now} in {phase:?}");
        }
    }

    use Input::{Chord, Tick};
    const P: Input = Input::Pressed;
    const R: Input = Input::Released;
    const E: Input = Input::Escape;

    #[test]
    fn hold_or_lock_long_hold_finishes_on_release() {
        run(HoldOrLock, false, &[
            (P, 0, Idle, Action::Start, None),
            (R, 300, Recording, Action::Stop, None),
            (P, 900, Transcribing, Action::Ignore, None),
            (R, 950, Transcribing, Action::Ignore, None),
        ]);
    }

    #[test]
    fn hold_or_lock_quick_tap_is_discarded() {
        run(HoldOrLock, false, &[
            (P, 0, Idle, Action::Start, None),
            (R, 120, Recording, Action::Ignore, Some(420)),
            (Tick, 300, Recording, Action::Ignore, Some(420)),
            (Tick, 420, Recording, Action::Discard, None),
            (Tick, 500, Idle, Action::Ignore, None),
            (P, 2000, Idle, Action::Start, None),
        ]);
    }

    #[test]
    fn hold_or_lock_double_tap_locks_until_the_next_press() {
        run(HoldOrLock, false, &[
            (P, 0, Idle, Action::Start, None),
            (R, 100, Recording, Action::Ignore, Some(400)),
            (P, 250, Recording, Action::Ignore, None),
            (R, 330, Recording, Action::Ignore, None),
            (Tick, 400, Recording, Action::Ignore, None),
            (P, 9000, Recording, Action::Stop, None),
            (R, 9100, Transcribing, Action::Ignore, None),
        ]);
    }

    #[test]
    fn hold_or_lock_second_press_just_after_the_window() {
        // The timer went first: the tap is gone and the press starts afresh.
        run(HoldOrLock, false, &[
            (P, 0, Idle, Action::Start, None),
            (R, 100, Recording, Action::Ignore, Some(400)),
            (Tick, 400, Recording, Action::Discard, None),
            (P, 401, Idle, Action::Start, None),
            (R, 1500, Recording, Action::Stop, None),
        ]);
        // The press beat a late timer: it holds the running recording.
        run(HoldOrLock, false, &[
            (P, 0, Idle, Action::Start, None),
            (R, 100, Recording, Action::Ignore, Some(400)),
            (P, 401, Recording, Action::Ignore, None),
            (Tick, 410, Recording, Action::Ignore, None),
            (R, 1500, Recording, Action::Stop, None),
        ]);
    }

    #[test]
    fn hold_or_lock_escape_in_each_state() {
        run(HoldOrLock, false, &[(E, 0, Idle, Action::Ignore, None), (P, 10, Idle, Action::Start, None)]);
        run(HoldOrLock, false, &[
            (P, 0, Idle, Action::Start, None),
            (E, 100, Recording, Action::Cancel, None),
            (R, 200, Cancelled, Action::Ignore, None),
        ]);
        run(HoldOrLock, false, &[
            (P, 0, Idle, Action::Start, None),
            (R, 100, Recording, Action::Ignore, Some(400)),
            (E, 200, Recording, Action::Cancel, None),
            (Tick, 400, Cancelled, Action::Ignore, None),
        ]);
        run(HoldOrLock, false, &[
            (P, 0, Idle, Action::Start, None),
            (R, 100, Recording, Action::Ignore, Some(400)),
            (P, 200, Recording, Action::Ignore, None),
            (R, 250, Recording, Action::Ignore, None),
            (E, 5000, Recording, Action::Cancel, None),
            (P, 6000, Cancelled, Action::Start, None),
        ]);
        run(HoldOrLock, false, &[(E, 0, Cleaning, Action::Cancel, None)]);
    }

    #[test]
    fn hold_or_lock_ignores_presses_while_busy() {
        for phase in [Transcribing, Cleaning] {
            run(HoldOrLock, false, &[
                (P, 0, phase, Action::Ignore, None),
                (R, 500, phase, Action::Ignore, None),
                (Tick, 900, phase, Action::Ignore, None),
            ]);
        }
        // A recording that ended on its own (the cap, an error) resets the machine.
        run(HoldOrLock, false, &[
            (P, 0, Idle, Action::Start, None),
            (R, 100, Recording, Action::Ignore, Some(400)),
            (P, 200, Recording, Action::Ignore, None),
            (P, 7000, Transcribing, Action::Ignore, None),
            (P, 9000, Completed, Action::Start, None),
        ]);
    }

    #[test]
    fn other_modes_match_the_activation_table() {
        for mode in [PushToTalk, Toggle] {
            for phase in [Idle, Recording, Transcribing, Cleaning, Completed, Failed, Cancelled] {
                for (input, event) in [(P, Pressed), (R, Released), (E, Escape)] {
                    let mut m = Activation::default();
                    assert_eq!(m.on(mode, false, input, 0, phase), (action(mode, phase, event), None), "{mode:?} {phase:?} {event:?}");
                }
                assert_eq!(Activation::default().on(mode, false, Tick, 0, phase), (Action::Ignore, None));
            }
        }
    }

    #[test]
    fn lone_modifier_chord_discards_the_recording_it_started() {
        for mode in [PushToTalk, Toggle, HoldOrLock] {
            run(mode, true, &[
                (P, 0, Idle, Action::Start, None),
                (Chord, 50, Recording, Action::Discard, None),
                (R, 80, Idle, Action::Ignore, None),
                (Tick, 600, Idle, Action::Ignore, None),
            ]);
        }
        // Tapped, then Ctrl+C inside the double-tap window.
        run(HoldOrLock, true, &[
            (P, 0, Idle, Action::Start, None),
            (R, 100, Recording, Action::Ignore, Some(400)),
            (P, 200, Recording, Action::Ignore, None),
            (Chord, 220, Recording, Action::Discard, None),
            (R, 300, Idle, Action::Ignore, None),
        ]);
    }

    #[test]
    fn lone_modifier_stops_on_release_unless_it_was_a_chord() {
        run(Toggle, true, &[
            (P, 0, Idle, Action::Start, None),
            (R, 100, Recording, Action::Ignore, None),
            (P, 3000, Recording, Action::Ignore, None),
            (Chord, 3050, Recording, Action::Ignore, None),
            (R, 3100, Recording, Action::Ignore, None),
            (P, 6000, Recording, Action::Ignore, None),
            (R, 6100, Recording, Action::Stop, None),
        ]);
        run(HoldOrLock, true, &[
            (P, 0, Idle, Action::Start, None),
            (R, 100, Recording, Action::Ignore, Some(400)),
            (P, 200, Recording, Action::Ignore, None),
            (R, 250, Recording, Action::Ignore, None),
            (P, 5000, Recording, Action::Ignore, None),
            (Chord, 5050, Recording, Action::Ignore, None),
            (R, 5100, Recording, Action::Ignore, None),
            (P, 8000, Recording, Action::Ignore, None),
            (R, 8100, Recording, Action::Stop, None),
        ]);
        run(PushToTalk, true, &[
            (P, 0, Idle, Action::Start, None),
            (R, 2000, Recording, Action::Stop, None),
        ]);
    }

    #[test]
    fn holds() {
        assert_eq!(Completed.hold(), Some(Duration::from_millis(1600)));
        assert_eq!(Failed.hold(), Some(Duration::from_secs(3)));
        assert_eq!(Cancelled.hold(), Some(Duration::from_millis(1300)));
        assert_eq!(Recording.hold(), None);
    }
}
