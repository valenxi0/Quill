//! What to do to other apps' audio while recording, and what to put back
//! afterwards. Quill restores only what it changed, and leaves alone anything
//! the user changed in the meantime.
//!
//! Windows remembers each app's volume, so an app that closes while lowered
//! comes back lowered. A change that finds no session to restore is kept, and
//! applied when a session of the same app next appears still reading what
//! Quill set.

use serde::{Deserialize, Serialize};

use crate::settings::OtherAudio;

/// One per-app audio session (WASAPI) as read now.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionReading {
    /// Unique per session and process: the session instance identifier.
    pub key: String,
    /// The same for every run of one app on one device: the session
    /// identifier. Empty when unknown.
    pub app: String,
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
    #[serde(default)]
    pub app: String,
    pub pid: u32,
    pub from_volume: f32,
    pub from_muted: bool,
    pub to_volume: f32,
    pub to_muted: bool,
    /// When Quill made the change, in seconds since 1970. Missing means
    /// unknown, which counts as expired.
    #[serde(default)]
    pub changed_at: u64,
}

/// How long Quill keeps a change whose app never comes back.
pub const KEEP_FOR_SECS: u64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RestorePlan {
    pub restores: Vec<Restore>,
    /// Changes with no session to restore yet: the app is closed.
    pub unresolved: Vec<VolumeChange>,
}

/// What to set on one session to put it back.
#[derive(Debug, Clone, PartialEq)]
pub struct Restore {
    pub key: String,
    pub volume: Option<f32>,
    pub muted: Option<bool>,
    /// The change this undoes, kept if setting fails.
    pub change: VolumeChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaStatus {
    Playing,
    Paused,
    Other,
}

const SAME: f32 = 0.01;

/// The volume changes to make. Sessions already in `held` keep the original
/// recorded when they were first changed.
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
            Some(VolumeChange {
                key: s.key.clone(),
                app: s.app.clone(),
                pid: s.pid,
                from_volume: s.volume,
                from_muted: s.muted,
                to_volume,
                to_muted,
                changed_at: 0,
            })
        })
        .collect()
}

/// What to put back. A session is restored only where it still reads what
/// Quill set; one the user changed is left alone. When the session itself
/// is gone, another session of the same app is restored if it reads what
/// Quill set; with no such session, the change stays unresolved. A change
/// without an app id can only ever match its own session, so it is dropped
/// once that session is gone.
pub fn restore(changes: &[VolumeChange], now: &[SessionReading]) -> RestorePlan {
    let mut plan = RestorePlan::default();
    for c in changes {
        if let Some(s) = now.iter().find(|s| s.key == c.key && s.pid == c.pid) {
            plan.restores.extend(undo(c, s));
            continue;
        }
        if c.app.is_empty() {
            continue;
        }
        let mut same_app = now.iter().filter(|s| s.app == c.app).peekable();
        if same_app.peek().is_none() {
            plan.unresolved.push(c.clone());
            continue;
        }
        plan.restores.extend(same_app.filter_map(|s| undo(c, s)));
    }
    plan
}

fn undo(c: &VolumeChange, s: &SessionReading) -> Option<Restore> {
    let volume = ((c.to_volume - c.from_volume).abs() >= SAME && (s.volume - c.to_volume).abs() < SAME).then_some(c.from_volume);
    let muted = (c.to_muted != c.from_muted && s.muted == c.to_muted).then_some(c.from_muted);
    (volume.is_some() || muted.is_some()).then(|| Restore { key: s.key.clone(), volume, muted, change: c.clone() })
}

/// Drop changes older than `KEEP_FOR_SECS`: an app that hasn't run for a
/// month has likely been removed, and restoring it later would surprise.
pub fn expire(changes: Vec<VolumeChange>, now_secs: u64) -> Vec<VolumeChange> {
    changes.into_iter().filter(|c| c.changed_at != 0 && now_secs.saturating_sub(c.changed_at) <= KEEP_FOR_SECS).collect()
}

/// The change as far as it applied, when only part of it did.
pub fn applied(c: &VolumeChange, volume_ok: bool, mute_ok: bool) -> Option<VolumeChange> {
    let volume_ok = volume_ok && (c.to_volume - c.from_volume).abs() >= SAME;
    let mute_ok = mute_ok && c.to_muted != c.from_muted;
    if !volume_ok && !mute_ok {
        return None;
    }
    let mut c = c.clone();
    if !volume_ok {
        c.to_volume = c.from_volume;
    }
    if !mute_ok {
        c.to_muted = c.from_muted;
    }
    Some(c)
}

/// Whether to resume a player Quill paused: not if its playback changed
/// after Quill's pause took effect. A pause that never showed (a very short
/// recording) is resumed anyway, so the player can't stay paused.
pub fn should_resume(status: MediaStatus, changed_since_pause: bool, seen_paused: bool) -> bool {
    !changed_since_pause && (status == MediaStatus::Paused || !seen_paused)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(key: &str, pid: u32, volume: f32, muted: bool) -> SessionReading {
        SessionReading { key: key.into(), app: format!("app-{key}"), pid, volume, muted, active: true }
    }

    fn sets(plan: &RestorePlan) -> Vec<(&str, Option<f32>, Option<bool>)> {
        plan.restores.iter().map(|r| (r.key.as_str(), r.volume, r.muted)).collect()
    }

    #[test]
    fn lower_scales_each_app_from_its_own_volume() {
        let sessions = [s("spotify", 10, 0.8, false), s("chrome", 11, 0.5, false)];
        let changes = plan(OtherAudio::Lower, 20, 99, &sessions, &[]);
        assert_eq!(changes.len(), 2);
        assert!((changes[0].to_volume - 0.16).abs() < 1e-6);
        assert_eq!(changes[0].from_volume, 0.8);
        assert_eq!(changes[0].app, "app-spotify");
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
        let changes = plan(OtherAudio::Lower, 20, 99, &[s("a", 10, 1.0, false), s("b", 11, 0.5, false)], &[]);
        let now = [s("a", 10, 0.2, false), s("b", 11, 0.9, false)];
        let r = restore(&changes, &now);
        assert_eq!(sets(&r), [("a", Some(1.0), None)]);
        assert_eq!(r.restores[0].change, changes[0]);
        assert!(r.unresolved.is_empty(), "the user's change to b settles it");
    }

    #[test]
    fn restore_unmutes_only_if_still_muted() {
        let changes = plan(OtherAudio::Mute, 20, 99, &[s("a", 10, 0.7, false), s("b", 11, 0.7, false)], &[]);
        let now = [s("a", 10, 0.3, true), s("b", 11, 0.7, false)];
        let r = restore(&changes, &now);
        assert_eq!(sets(&r), [("a", None, Some(false))]);
    }

    #[test]
    fn a_closed_app_is_kept_until_it_comes_back() {
        let changes = plan(OtherAudio::Lower, 20, 99, &[s("a", 10, 1.0, false)], &[]);
        let closed = restore(&changes, &[]);
        assert!(closed.restores.is_empty());
        assert_eq!(closed.unresolved, changes);

        let mut relaunched = s("a2", 77, 0.2, false);
        relaunched.app = "app-a".into();
        let back = restore(&closed.unresolved, std::slice::from_ref(&relaunched));
        assert_eq!(sets(&back), [("a2", Some(1.0), None)]);
        assert!(back.unresolved.is_empty());

        relaunched.volume = 0.6;
        let changed = restore(&closed.unresolved, &[relaunched]);
        assert!(changed.restores.is_empty() && changed.unresolved.is_empty(), "the user set another volume since");
    }

    #[test]
    fn a_closed_muted_app_is_unmuted_when_it_comes_back() {
        let changes = plan(OtherAudio::Mute, 20, 99, &[s("a", 10, 0.7, false)], &[]);
        let mut relaunched = s("a2", 77, 0.7, true);
        relaunched.app = "app-a".into();
        let r = restore(&changes, &[relaunched]);
        assert_eq!(sets(&r), [("a2", None, Some(false))]);
    }

    #[test]
    fn a_change_without_an_app_id_is_dropped_once_its_session_is_gone() {
        let mut changes = plan(OtherAudio::Lower, 20, 99, &[s("a", 10, 1.0, false)], &[]);
        changes[0].app.clear();
        let mut alive = s("a", 10, 0.2, false);
        alive.app.clear();
        assert_eq!(sets(&restore(&changes, &[alive])), [("a", Some(1.0), None)]);
        let mut other = s("other", 77, 0.2, false);
        other.app.clear();
        let gone = restore(&changes, &[other]);
        assert!(gone.restores.is_empty() && gone.unresolved.is_empty());
    }

    #[test]
    fn changes_expire_after_thirty_days() {
        let mut c = plan(OtherAudio::Lower, 20, 99, &[s("a", 10, 1.0, false)], &[])[0].clone();
        let now = 1_800_000_000;
        c.changed_at = now - KEEP_FOR_SECS;
        assert_eq!(expire(vec![c.clone()], now).len(), 1, "exactly 30 days is kept");
        c.changed_at = now - KEEP_FOR_SECS - 1;
        assert!(expire(vec![c.clone()], now).is_empty());
        c.changed_at = 0;
        assert!(expire(vec![c], now).is_empty(), "no timestamp counts as expired");
    }

    #[test]
    fn a_half_applied_change_records_only_what_applied() {
        let c = plan(OtherAudio::Lower, 20, 99, &[s("a", 10, 1.0, false)], &[])[0].clone();
        assert_eq!(applied(&c, true, false), Some(c.clone()), "mute was not part of it");
        assert_eq!(applied(&c, false, true), None, "nothing it needed applied");
        let mut both = c.clone();
        both.to_muted = true;
        let volume_only = applied(&both, true, false).unwrap();
        assert!(!volume_only.to_muted && (volume_only.to_volume - 0.2).abs() < 1e-6);
        let mute_only = applied(&both, false, true).unwrap();
        assert!(mute_only.to_muted && mute_only.to_volume == 1.0);
    }

    #[test]
    fn resume_only_untouched_paused_players() {
        assert!(should_resume(MediaStatus::Paused, false, true));
        assert!(!should_resume(MediaStatus::Paused, true, true), "the user resumed and paused it again");
        assert!(!should_resume(MediaStatus::Playing, false, true), "the user already resumed it");
        assert!(!should_resume(MediaStatus::Other, false, true), "the player stopped");
        assert!(should_resume(MediaStatus::Playing, false, false), "the pause hadn't landed yet");
    }

    #[test]
    fn changes_round_trip_through_json() {
        let changes = plan(OtherAudio::Lower, 20, 99, &[s("a", 10, 1.0, false)], &[]);
        let json = serde_json::to_string(&changes).unwrap();
        assert_eq!(serde_json::from_str::<Vec<VolumeChange>>(&json).unwrap(), changes);
        let old: Vec<VolumeChange> =
            serde_json::from_str(r#"[{"key":"a","pid":1,"fromVolume":1.0,"fromMuted":false,"toVolume":0.2,"toMuted":false}]"#).unwrap();
        assert_eq!((old[0].app.as_str(), old[0].changed_at), ("", 0));
    }
}
