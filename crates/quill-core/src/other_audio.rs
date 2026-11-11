//! What to do to other apps' audio while recording, and what to put back
//! afterwards. Quill restores only what it changed, and leaves alone anything
//! the user changed in the meantime.

use serde::{Deserialize, Serialize};

use crate::settings::OtherAudio;

/// One per-app audio session (WASAPI) as read now.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionReading {
    /// Unique per session and process: the session instance identifier.
    pub key: String,
    pub pid: u32,
    pub volume: f32,
    pub muted: bool,
    /// The session has a running stream.
    pub active: bool,
}

/// A change Quill made to one session: what it was, and what Quill set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeChange {
    pub key: String,
    pub pid: u32,
    pub from_volume: f32,
    pub from_muted: bool,
    pub to_volume: f32,
    pub to_muted: bool,
}

/// What to set on one session to put it back.
#[derive(Debug, Clone, PartialEq)]
pub struct Restore {
    pub key: String,
    pub volume: Option<f32>,
    pub muted: Option<bool>,
}

/// A media player (Windows media session) as read now.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaReading {
    /// The player's app id.
    pub key: String,
    pub status: MediaStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaStatus {
    Playing,
    Paused,
    Other,
}

const SAME: f32 = 0.01;

/// The volume changes to make at the start of a recording. Sessions already
/// in `held` keep the original recorded when they were first changed.
pub fn plan(mode: OtherAudio, lower_to_percent: u8, own_pid: u32, sessions: &[SessionReading], held: &[VolumeChange]) -> Vec<VolumeChange> {
    let fraction = f32::from(lower_to_percent.min(100)) / 100.0;
    sessions
        .iter()
        // pid 0 is Windows' system sounds, shared by every app.
        .filter(|s| s.active && s.pid != own_pid && s.pid != 0 && !s.muted)
        .filter(|s| !held.iter().any(|h| h.key == s.key))
        .filter_map(|s| {
            let (to_volume, to_muted) = match mode {
                OtherAudio::Lower => {
                    let to = s.volume * fraction;
                    if s.volume - to < SAME {
                        return None;
                    }
                    (to, false)
                }
                OtherAudio::Mute => (s.volume, true),
                OtherAudio::Nothing | OtherAudio::Pause => return None,
            };
            Some(VolumeChange { key: s.key.clone(), pid: s.pid, from_volume: s.volume, from_muted: s.muted, to_volume, to_muted })
        })
        .collect()
}

/// What to put back. A session is restored only where it still reads what
/// Quill set; one the user changed, or that closed, is left alone.
pub fn restore(changes: &[VolumeChange], now: &[SessionReading]) -> Vec<Restore> {
    changes
        .iter()
        .filter_map(|c| {
            let s = now.iter().find(|s| s.key == c.key && s.pid == c.pid)?;
            let volume = ((c.to_volume - c.from_volume).abs() >= SAME && (s.volume - c.to_volume).abs() < SAME).then_some(c.from_volume);
            let muted = (c.to_muted != c.from_muted && s.muted == c.to_muted).then_some(c.from_muted);
            (volume.is_some() || muted.is_some()).then(|| Restore { key: c.key.clone(), volume, muted })
        })
        .collect()
}

/// Players to pause at the start of a recording.
pub fn plan_pause(media: &[MediaReading]) -> Vec<String> {
    let mut keys: Vec<String> = media.iter().filter(|m| m.status == MediaStatus::Playing).map(|m| m.key.clone()).collect();
    keys.dedup();
    keys
}

/// Players to resume: those Quill paused that are still paused.
pub fn plan_resume(paused: &[String], now: &[MediaReading]) -> Vec<String> {
    paused.iter().filter(|k| now.iter().any(|m| &m.key == *k && m.status == MediaStatus::Paused)).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(key: &str, pid: u32, volume: f32, muted: bool) -> SessionReading {
        SessionReading { key: key.into(), pid, volume, muted, active: true }
    }

    #[test]
    fn lower_scales_each_app_from_its_own_volume() {
        let sessions = [s("spotify", 10, 0.8, false), s("chrome", 11, 0.5, false)];
        let changes = plan(OtherAudio::Lower, 20, 99, &sessions, &[]);
        assert_eq!(changes.len(), 2);
        assert!((changes[0].to_volume - 0.16).abs() < 1e-6);
        assert_eq!(changes[0].from_volume, 0.8);
        assert!(!changes[0].to_muted);
        assert!((changes[1].to_volume - 0.1).abs() < 1e-6);
    }

    #[test]
    fn skips_quill_system_sounds_idle_and_muted_sessions() {
        let mut idle = s("idle", 12, 1.0, false);
        idle.active = false;
        let sessions = [s("quill", 99, 1.0, false), s("system", 0, 1.0, false), idle, s("muted", 13, 1.0, true), s("silent", 14, 0.0, false)];
        assert!(plan(OtherAudio::Lower, 20, 99, &sessions, &[]).is_empty());
        let muted = plan(OtherAudio::Mute, 20, 99, &sessions, &[]);
        assert_eq!(muted.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(), ["silent"]);
    }

    #[test]
    fn nothing_pause_and_full_volume_change_nothing() {
        let sessions = [s("a", 10, 0.8, false)];
        assert!(plan(OtherAudio::Nothing, 20, 99, &sessions, &[]).is_empty());
        assert!(plan(OtherAudio::Pause, 20, 99, &sessions, &[]).is_empty());
        assert!(plan(OtherAudio::Lower, 100, 99, &sessions, &[]).is_empty());
    }

    #[test]
    fn mute_keeps_volume() {
        let changes = plan(OtherAudio::Mute, 20, 99, &[s("a", 10, 0.6, false)], &[]);
        assert_eq!((changes[0].to_volume, changes[0].to_muted), (0.6, true));
    }

    #[test]
    fn a_held_session_keeps_its_first_original() {
        let first = plan(OtherAudio::Lower, 20, 99, &[s("a", 10, 1.0, false)], &[]);
        let lowered = [s("a", 10, first[0].to_volume, false), s("b", 11, 0.5, false)];
        let again = plan(OtherAudio::Lower, 20, 99, &lowered, &first);
        assert_eq!(again.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(), ["b"]);
    }

    #[test]
    fn restore_puts_back_only_untouched_sessions() {
        let changes = plan(OtherAudio::Lower, 20, 99, &[s("a", 10, 1.0, false), s("b", 11, 0.5, false), s("c", 12, 0.5, false)], &[]);
        let now = [s("a", 10, 0.2, false), s("b", 11, 0.9, false)];
        let r = restore(&changes, &now);
        assert_eq!(r, [Restore { key: "a".into(), volume: Some(1.0), muted: None }]);
    }

    #[test]
    fn restore_unmutes_only_if_still_muted() {
        let changes = plan(OtherAudio::Mute, 20, 99, &[s("a", 10, 0.7, false), s("b", 11, 0.7, false)], &[]);
        let now = [s("a", 10, 0.3, true), s("b", 11, 0.7, false)];
        let r = restore(&changes, &now);
        assert_eq!(r, [Restore { key: "a".into(), volume: None, muted: Some(false) }]);
    }

    #[test]
    fn restore_ignores_a_new_process_reusing_the_key() {
        let changes = plan(OtherAudio::Lower, 20, 99, &[s("a", 10, 1.0, false)], &[]);
        assert!(restore(&changes, &[s("a", 77, 0.2, false)]).is_empty());
    }

    #[test]
    fn pause_and_resume() {
        let m = |k: &str, status| MediaReading { key: k.into(), status };
        let paused = plan_pause(&[m("spotify", MediaStatus::Playing), m("edge", MediaStatus::Paused), m("vlc", MediaStatus::Other)]);
        assert_eq!(paused, ["spotify"]);
        assert_eq!(plan_resume(&paused, &[m("spotify", MediaStatus::Paused)]), ["spotify"]);
        assert!(plan_resume(&paused, &[m("spotify", MediaStatus::Playing)]).is_empty(), "the user already resumed it");
        assert!(plan_resume(&paused, &[]).is_empty(), "the player closed");
    }

    #[test]
    fn changes_round_trip_through_json() {
        let changes = plan(OtherAudio::Lower, 20, 99, &[s("a", 10, 1.0, false)], &[]);
        let json = serde_json::to_string(&changes).unwrap();
        assert_eq!(serde_json::from_str::<Vec<VolumeChange>>(&json).unwrap(), changes);
    }
}
