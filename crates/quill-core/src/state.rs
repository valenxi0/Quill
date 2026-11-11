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

/// In hold-or-lock, a press shorter than this is a tap, and a second press
/// this soon after a tap locks the recording.
pub const TAP_MS: u64 = 300;

/// A lone Right Ctrl or Right Alt does nothing until it has been held this
/// long with no other key, or released clean, so Ctrl+C never opens the
/// microphone. Push to talk loses up to this much leading audio.
pub const ARM_MS: u64 = 200;

/// A chord this soon after a lone modifier started recording drops it, as a
/// slow Ctrl+C. Later chords are typing during dictation and are ignored.
pub const CHORD_GRACE_MS: u64 = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Pressed,
    Released,
    Escape,
    /// A timer the caller set from a returned deadline went off.
    Tick,
    /// Another key, a click, or the wheel while a lone-modifier shortcut was held.
    Chord,
}

/// Where a lone-modifier press goes back to if it turns into a chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rest {
    Idle,
    Waiting { until: u64, recording: bool },
    Locked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Stage {
    #[default]
    Idle,
    /// A lone modifier is down with nothing else. Nothing has happened yet.
    Armed { since: u64, from: Rest },
    /// Recording while the shortcut is held, pressed at `since`.
    Held { since: u64 },
    /// After a tap: a second press before `until` locks. `recording`: the
    /// tap started a recording that the timer discards.
    Waiting { until: u64, recording: bool },
    /// Recording hands-free until the next press.
    Locked,
}

impl From<Rest> for Stage {
    fn from(rest: Rest) -> Self {
        match rest {
            Rest::Idle => Stage::Idle,
            Rest::Waiting { until, recording } => Stage::Waiting { until, recording },
            Rest::Locked => Stage::Locked,
        }
    }
}

/// Turns shortcut events into actions. Time comes in as milliseconds
/// stamped when the key event happened, so it stays pure and a slow start
/// can't stretch a tap.
#[derive(Debug, Clone, Default)]
pub struct Activation {
    stage: Stage,
    /// When a lone-modifier press last started a recording.
    lone_start: Option<u64>,
}

type Step = (Action, Option<u64>);

impl Activation {
    /// `lone`: the shortcut is Right Ctrl or Right Alt alone. Returns the
    /// action and, when one is due, the time to send `Tick` at.
    pub fn on(&mut self, mode: ActivationMode, lone: bool, input: Input, now: u64, phase: Phase) -> Step {
        let recording = phase == Phase::Recording;
        if !recording && matches!(self.stage, Stage::Held { .. } | Stage::Locked | Stage::Waiting { recording: true, .. }) {
            self.stage = Stage::Idle;
        }
        match input {
            Input::Escape => {
                *self = Self::default();
                (if phase.is_busy() { Action::Cancel } else { Action::Ignore }, None)
            }
            Input::Chord => self.chord(now, phase),
            Input::Tick => self.tick(mode, now, phase),
            _ if lone => self.lone(mode, input, now, phase),
            _ => self.plain(mode, input, now, phase),
        }
    }

    fn plain(&mut self, mode: ActivationMode, input: Input, now: u64, phase: Phase) -> Step {
        use Action::*;
        let recording = phase == Phase::Recording;
        let pressed = input == Input::Pressed;
        let act = match mode {
            ActivationMode::PushToTalk if pressed => if phase.is_busy() { Ignore } else { Start },
            ActivationMode::PushToTalk => if recording { Stop } else { Ignore },
            ActivationMode::Toggle if pressed => {
                if recording {
                    Stop
                } else if phase.is_busy() {
                    Ignore
                } else {
                    Start
                }
            }
            ActivationMode::Toggle => Ignore,
            ActivationMode::HoldOrLock => return self.hold_or_lock(input, now, phase),
        };
        (act, None)
    }

    fn hold_or_lock(&mut self, input: Input, now: u64, phase: Phase) -> Step {
        use Action::*;
        let recording = phase == Phase::Recording;
        match (self.stage, input) {
            // Started from the tray, a link, or Sketch: a press finishes it.
            (Stage::Idle, Input::Pressed) if recording => (Stop, None),
            (Stage::Idle, Input::Pressed) if !phase.is_busy() => {
                self.stage = Stage::Held { since: now };
                (Start, None)
            }
            (Stage::Held { since }, Input::Released) => self.release_held(since, now),
            (Stage::Waiting { until, .. }, Input::Pressed) => {
                // A late press, before the timer ran, holds the same recording.
                self.stage = if now <= until { Stage::Locked } else { Stage::Held { since: now } };
                (Ignore, None)
            }
            (Stage::Locked, Input::Pressed) => {
                self.stage = Stage::Idle;
                (Stop, None)
            }
            _ => (Ignore, None),
        }
    }

    /// Hold-or-lock: a hold finishes, a tap waits for a second one.
    fn release_held(&mut self, since: u64, now: u64) -> Step {
        if now.saturating_sub(since) >= TAP_MS {
            self.stage = Stage::Idle;
            (Action::Stop, None)
        } else {
            let until = now + TAP_MS;
            self.stage = Stage::Waiting { until, recording: true };
            (Action::Ignore, Some(until))
        }
    }

    fn lone(&mut self, mode: ActivationMode, input: Input, now: u64, phase: Phase) -> Step {
        let recording = phase == Phase::Recording;
        match (input, self.stage) {
            (Input::Pressed, Stage::Armed { .. }) => (Action::Ignore, None),
            (Input::Pressed, _) => {
                let busy = if mode == ActivationMode::PushToTalk { phase.is_busy() } else { phase.is_busy() && !recording };
                if busy {
                    return (Action::Ignore, None);
                }
                let from = match self.stage {
                    Stage::Waiting { until, recording } if now <= until => Rest::Waiting { until, recording },
                    Stage::Locked => Rest::Locked,
                    _ => Rest::Idle,
                };
                self.stage = Stage::Armed { since: now, from };
                // Toggle waits for the release.
                (Action::Ignore, (mode != ActivationMode::Toggle).then_some(now + ARM_MS))
            }
            (Input::Released, Stage::Armed { since, from }) => self.fire(mode, since, from, now, phase, true),
            (Input::Released, Stage::Held { since }) if mode == ActivationMode::HoldOrLock => self.release_held(since, now),
            (Input::Released, Stage::Held { .. }) => {
                self.stage = Stage::Idle;
                (if recording { Action::Stop } else { Action::Ignore }, None)
            }
            _ => (Action::Ignore, None),
        }
    }

    /// A lone press takes effect: on a clean release, or once armed.
    fn fire(&mut self, mode: ActivationMode, since: u64, from: Rest, now: u64, phase: Phase, released: bool) -> Step {
        use Action::*;
        let recording = phase == Phase::Recording;
        let busy = phase.is_busy();
        self.stage = Stage::Idle;
        let start = |me: &mut Self, stage: Stage| {
            me.stage = stage;
            me.lone_start = Some(now);
            (Start, None)
        };
        match mode {
            ActivationMode::Toggle if recording => (Stop, None),
            ActivationMode::Toggle if busy => (Ignore, None),
            ActivationMode::Toggle => start(self, Stage::Idle),
            // A tap shorter than the arm delay does nothing.
            ActivationMode::PushToTalk if released || busy => (Ignore, None),
            ActivationMode::PushToTalk => start(self, Stage::Held { since }),
            ActivationMode::HoldOrLock => match from {
                Rest::Locked | Rest::Idle if recording => (Stop, None),
                Rest::Locked => (Ignore, None),
                Rest::Idle if released => {
                    let until = now + TAP_MS;
                    self.stage = Stage::Waiting { until, recording: false };
                    (Ignore, Some(until))
                }
                Rest::Idle if busy => (Ignore, None),
                Rest::Idle => start(self, Stage::Held { since }),
                Rest::Waiting { recording: true, .. } if recording => {
                    self.stage = Stage::Locked;
                    (Ignore, None)
                }
                Rest::Waiting { recording: false, .. } if !busy => start(self, Stage::Locked),
                Rest::Waiting { .. } => (Ignore, None),
            },
        }
    }

    fn tick(&mut self, mode: ActivationMode, now: u64, phase: Phase) -> Step {
        match self.stage {
            Stage::Armed { since, from } if mode != ActivationMode::Toggle => {
                if now >= since + ARM_MS {
                    self.fire(mode, since, from, now, phase, false)
                } else {
                    (Action::Ignore, Some(since + ARM_MS))
                }
            }
            Stage::Waiting { until, recording } => self.expire(until, recording, now, phase),
            _ => (Action::Ignore, None),
        }
    }

    /// The double-tap window closes: a tap's recording is dropped.
    fn expire(&mut self, until: u64, tap_recording: bool, now: u64, phase: Phase) -> Step {
        if now < until {
            return (Action::Ignore, Some(until));
        }
        self.stage = Stage::Idle;
        (if tap_recording && phase == Phase::Recording { Action::Discard } else { Action::Ignore }, None)
    }

    fn chord(&mut self, now: u64, phase: Phase) -> Step {
        match self.stage {
            Stage::Armed { from, .. } => {
                self.stage = from.into();
                match from {
                    Rest::Waiting { until, recording } => self.expire(until, recording, now, phase),
                    _ => (Action::Ignore, None),
                }
            }
            Stage::Held { .. } | Stage::Locked
                if phase == Phase::Recording && self.lone_start.is_some_and(|t| now.saturating_sub(t) < CHORD_GRACE_MS) =>
            {
                *self = Self::default();
                (Action::Discard, None)
            }
            _ => (Action::Ignore, None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ActivationMode::*;
    use Phase::*;

    use Input::{Chord, Tick};
    const P: Input = Input::Pressed;
    const R: Input = Input::Released;
    const E: Input = Input::Escape;

    fn once(mode: ActivationMode, phase: Phase, input: Input) -> Action {
        let (act, deadline) = Activation::default().on(mode, false, input, 0, phase);
        assert_eq!(deadline, None);
        act
    }

    #[test]
    fn activation_table() {
        let cases = [
            (PushToTalk, Idle, P, Action::Start),
            (PushToTalk, Completed, P, Action::Start),
            (PushToTalk, Recording, P, Action::Ignore),
            (PushToTalk, Transcribing, P, Action::Ignore),
            (PushToTalk, Recording, R, Action::Stop),
            (PushToTalk, Idle, R, Action::Ignore),
            (PushToTalk, Cleaning, R, Action::Ignore),
            (Toggle, Idle, P, Action::Start),
            (Toggle, Failed, P, Action::Start),
            (Toggle, Recording, P, Action::Stop),
            (Toggle, Transcribing, P, Action::Ignore),
            (Toggle, Recording, R, Action::Ignore),
            (PushToTalk, Recording, E, Action::Cancel),
            (Toggle, Cleaning, E, Action::Cancel),
            (Toggle, Idle, E, Action::Ignore),
            (PushToTalk, Cancelled, E, Action::Ignore),
        ];
        for (mode, phase, input, want) in cases {
            assert_eq!(once(mode, phase, input), want, "{mode:?} {phase:?} {input:?}");
        }
        for mode in [PushToTalk, Toggle] {
            for phase in [Idle, Recording, Transcribing, Completed] {
                assert_eq!(once(mode, phase, Tick), Action::Ignore);
            }
        }
    }

    /// Feeds `(input, ms, phase, want, deadline)` steps through one machine.
    fn run(mode: ActivationMode, lone: bool, steps: &[(Input, u64, Phase, Action, Option<u64>)]) {
        let mut m = Activation::default();
        for (i, &(input, now, phase, want, deadline)) in steps.iter().enumerate() {
            assert_eq!(m.on(mode, lone, input, now, phase), (want, deadline), "step {i}: {input:?} at {now} in {phase:?}");
        }
    }

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
    fn tap_timing_uses_event_times_not_when_they_are_handled() {
        // The start took 600 ms, so the release and second press are handled
        // late; their stamps still make a double tap.
        run(HoldOrLock, false, &[
            (P, 0, Idle, Action::Start, None),
            (R, 90, Recording, Action::Ignore, Some(390)),
            (P, 200, Recording, Action::Ignore, None),
            (R, 260, Recording, Action::Ignore, None),
            (Tick, 640, Recording, Action::Ignore, None),
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
        run(HoldOrLock, true, &[
            (P, 0, Idle, Action::Ignore, Some(200)),
            (E, 100, Idle, Action::Ignore, None),
            (Tick, 200, Idle, Action::Ignore, None),
            (R, 300, Idle, Action::Ignore, None),
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
    fn hold_or_lock_stops_a_recording_started_elsewhere() {
        run(HoldOrLock, false, &[
            (P, 0, Recording, Action::Stop, None),
            (R, 80, Transcribing, Action::Ignore, None),
        ]);
        run(HoldOrLock, true, &[
            (P, 0, Recording, Action::Ignore, Some(200)),
            (R, 90, Recording, Action::Stop, None),
        ]);
        run(HoldOrLock, true, &[
            (P, 0, Recording, Action::Ignore, Some(200)),
            (Chord, 50, Recording, Action::Ignore, None),
            (R, 90, Recording, Action::Ignore, None),
        ]);
    }

    #[test]
    fn lone_push_to_talk_starts_once_armed() {
        run(PushToTalk, true, &[
            (P, 0, Idle, Action::Ignore, Some(200)),
            (Tick, 150, Idle, Action::Ignore, Some(200)),
            (Tick, 200, Idle, Action::Start, None),
            (R, 2000, Recording, Action::Stop, None),
        ]);
        // Released clean before arming: a tap does nothing.
        run(PushToTalk, true, &[
            (P, 0, Idle, Action::Ignore, Some(200)),
            (R, 120, Idle, Action::Ignore, None),
            (Tick, 200, Idle, Action::Ignore, None),
        ]);
    }

    #[test]
    fn lone_chord_before_arming_does_nothing() {
        for mode in [PushToTalk, Toggle, HoldOrLock] {
            let deadline = (mode != Toggle).then_some(200);
            run(mode, true, &[
                (P, 0, Idle, Action::Ignore, deadline),
                (Chord, 50, Idle, Action::Ignore, None),
                (Tick, 200, Idle, Action::Ignore, None),
                (R, 400, Idle, Action::Ignore, None),
            ]);
        }
    }

    #[test]
    fn lone_chord_soon_after_start_discards_later_ones_are_ignored() {
        run(PushToTalk, true, &[
            (P, 0, Idle, Action::Ignore, Some(200)),
            (Tick, 200, Idle, Action::Start, None),
            (Chord, 600, Recording, Action::Discard, None),
            (R, 700, Idle, Action::Ignore, None),
        ]);
        run(PushToTalk, true, &[
            (P, 0, Idle, Action::Ignore, Some(200)),
            (Tick, 200, Idle, Action::Start, None),
            (Chord, 30_000, Recording, Action::Ignore, None),
            (R, 31_000, Recording, Action::Stop, None),
        ]);
    }

    #[test]
    fn lone_toggle_acts_on_a_clean_release() {
        run(Toggle, true, &[
            (P, 0, Idle, Action::Ignore, None),
            (R, 80, Idle, Action::Start, None),
            (P, 3000, Recording, Action::Ignore, None),
            (Chord, 3050, Recording, Action::Ignore, None),
            (R, 3100, Recording, Action::Ignore, None),
            (P, 6000, Recording, Action::Ignore, None),
            (R, 6900, Recording, Action::Stop, None),
        ]);
    }

    #[test]
    fn lone_hold_or_lock_tap_then_press_starts_locked() {
        run(HoldOrLock, true, &[
            (P, 0, Idle, Action::Ignore, Some(200)),
            (R, 90, Idle, Action::Ignore, Some(390)),
            (Tick, 200, Idle, Action::Ignore, Some(390)),
            (P, 250, Idle, Action::Ignore, Some(450)),
            (R, 300, Idle, Action::Start, None),
            (Tick, 450, Recording, Action::Ignore, None),
            (P, 5000, Recording, Action::Ignore, Some(5200)),
            (R, 5080, Recording, Action::Stop, None),
        ]);
        // A single clean tap leaves nothing behind.
        run(HoldOrLock, true, &[
            (P, 0, Idle, Action::Ignore, Some(200)),
            (R, 90, Idle, Action::Ignore, Some(390)),
            (Tick, 390, Idle, Action::Ignore, None),
            (P, 1000, Idle, Action::Ignore, Some(1200)),
            (Tick, 1200, Idle, Action::Start, None),
            (R, 1500, Recording, Action::Stop, None),
        ]);
        // Tap, then Ctrl+C: nothing at all.
        run(HoldOrLock, true, &[
            (P, 0, Idle, Action::Ignore, Some(200)),
            (R, 90, Idle, Action::Ignore, Some(390)),
            (P, 150, Idle, Action::Ignore, Some(350)),
            (Chord, 170, Idle, Action::Ignore, Some(390)),
            (R, 220, Idle, Action::Ignore, None),
            (Tick, 390, Idle, Action::Ignore, None),
        ]);
    }

    #[test]
    fn lone_hold_or_lock_press_held_while_locked_stops_once_armed() {
        run(HoldOrLock, true, &[
            (P, 0, Idle, Action::Ignore, Some(200)),
            (Tick, 200, Idle, Action::Start, None),
            (R, 250, Recording, Action::Ignore, Some(550)),
            (P, 400, Recording, Action::Ignore, Some(600)),
            (Tick, 550, Recording, Action::Ignore, Some(600)),
            (Tick, 600, Recording, Action::Ignore, None),
            (R, 700, Recording, Action::Ignore, None),
            (P, 9000, Recording, Action::Ignore, Some(9200)),
            (Tick, 9200, Recording, Action::Stop, None),
            (R, 9400, Transcribing, Action::Ignore, None),
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
