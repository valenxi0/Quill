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
    Ignore,
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

    #[test]
    fn holds() {
        assert_eq!(Completed.hold(), Some(Duration::from_millis(1600)));
        assert_eq!(Failed.hold(), Some(Duration::from_secs(3)));
        assert_eq!(Cancelled.hold(), Some(Duration::from_millis(1300)));
        assert_eq!(Recording.hold(), None);
    }
}
