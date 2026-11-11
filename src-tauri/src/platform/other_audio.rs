//! Other apps' audio while recording: lower or mute their WASAPI sessions,
//! or pause media players, then put everything back.
//!
//! One worker thread owns all of it. Commands run in order, so a recording
//! that starts right after another ends reads the restored volumes, never
//! Quill's lowered ones. What Quill changed is written to a file before the
//! first change and stays there until it is put back: after a crash, or when
//! an app closed while lowered (Windows keeps its lowered volume for its next
//! run), Quill restores it when a session of that app next appears.

use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use quill_core::other_audio::{applied, expire, plan, restore, should_resume, MediaStatus, Restore, SessionReading, VolumeChange};
use quill_core::settings::OtherAudio;
use windows::core::{implement, Interface, Ref};
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as MediaSession,
    GlobalSystemMediaTransportControlsSessionManager as MediaManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Playback, PlaybackInfoChangedEventArgs,
};
use windows::Win32::Media::Audio::{
    eRender, AudioSessionStateActive, AudioSessionStateExpired, IAudioSessionControl, IAudioSessionControl2, IAudioSessionManager2,
    IAudioSessionNotification, IAudioSessionNotification_Impl, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator,
    DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED};

/// How often a recording that lowers or mutes looks for audio that started
/// after it did.
const RECORDING_RESCAN: Duration = Duration::from_millis(500);
/// A second look after a session appears: its volume and state can settle
/// a moment after creation.
const SECOND_LOOK: Duration = Duration::from_secs(1);
/// How often Quill checks for a playback device plugged in since it started
/// watching, while it still has volumes to put back.
const DEVICE_CHECK: Duration = Duration::from_secs(5);
/// How long a short recording waits for its pause to land before resuming.
const PAUSE_LANDING: Duration = Duration::from_millis(300);

enum Cmd {
    Begin { generation: u64, mode: OtherAudio, percent: u8 },
    End(u64),
    /// Restore everything now, then acknowledge.
    Flush(Sender<()>),
    RestoreFile,
    SessionCreated,
}

static WORKER: OnceLock<Sender<Cmd>> = OnceLock::new();

/// Tests touch only their own player's sessions, never the user's apps.
#[cfg(test)]
static ONLY_APP: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

fn send(cmd: Cmd) {
    let tx = WORKER.get_or_init(|| {
        let (tx, rx) = channel();
        let _ = std::thread::Builder::new().name("quill-other-audio".into()).spawn(move || run(rx));
        tx
    });
    let _ = tx.send(cmd);
}

/// Held by a recording. Dropping it puts other apps' audio back.
pub struct Hold {
    generation: u64,
}

impl Drop for Hold {
    fn drop(&mut self) {
        send(Cmd::End(self.generation));
    }
}

/// Apply `mode` for the recording `generation`. Returns at once: the work
/// happens on the worker, off the microphone's start path.
pub fn hold(generation: u64, mode: OtherAudio, percent: u8) -> Option<Hold> {
    if mode == OtherAudio::Nothing {
        return None;
    }
    send(Cmd::Begin { generation, mode, percent });
    Some(Hold { generation })
}

/// Put everything back before Quill exits.
pub fn restore_all() {
    if WORKER.get().is_none() {
        return;
    }
    let (tx, rx) = channel();
    send(Cmd::Flush(tx));
    let _ = rx.recv_timeout(Duration::from_secs(2));
}

/// Restore what an earlier run left changed, now and as its apps reappear.
pub fn restore_pending() {
    if crate::paths::audio_restore_file().exists() {
        send(Cmd::RestoreFile);
    }
}

#[implement(IAudioSessionNotification)]
struct Notifier;

impl IAudioSessionNotification_Impl for Notifier_Impl {
    fn OnSessionCreated(&self, _session: Ref<IAudioSessionControl>) -> windows::core::Result<()> {
        if let Some(tx) = WORKER.get() {
            let _ = tx.send(Cmd::SessionCreated);
        }
        Ok(())
    }
}

/// A player Quill paused, and whether its playback changed afterwards.
struct Paused {
    session: MediaSession,
    token: i64,
    seen_paused: Arc<AtomicBool>,
    changed: Arc<AtomicBool>,
}

/// What Quill changed for the recording `generation`.
struct Active {
    generation: u64,
    mode: OtherAudio,
    percent: u8,
    changes: Vec<VolumeChange>,
    paused: Vec<Paused>,
}

impl Active {
    fn lowers(&self) -> bool {
        matches!(self.mode, OtherAudio::Lower | OtherAudio::Mute)
    }
}

#[derive(Default)]
struct Worker {
    active: Option<Active>,
    /// Changes whose app closed before Quill could put them back.
    unresolved: Vec<VolumeChange>,
    second_look: Option<Instant>,
    /// Playback devices by endpoint id, with the notifier registered on each.
    watching: Vec<(String, IAudioSessionManager2, IAudioSessionNotification)>,
}

fn run(rx: Receiver<Cmd>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let mut w = Worker::default();
    w.refresh_watch();
    loop {
        let cmd = match w.next_wake() {
            Some(at) => match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                Ok(cmd) => Some(cmd),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            },
            None => match rx.recv() {
                Ok(cmd) => Some(cmd),
                Err(_) => return,
            },
        };
        match cmd {
            Some(cmd) => w.handle(cmd),
            None => w.rescan(),
        }
    }
}

type Sessions = Vec<(SessionReading, ISimpleAudioVolume)>;

fn readings(sessions: &Sessions) -> Vec<SessionReading> {
    sessions.iter().map(|(r, _)| r.clone()).collect()
}

impl Worker {
    fn next_wake(&self) -> Option<Instant> {
        let recording = self.active.as_ref().filter(|a| a.lowers()).map(|_| Instant::now() + RECORDING_RESCAN);
        let devices = (!self.unresolved.is_empty()).then(|| Instant::now() + DEVICE_CHECK);
        [recording, self.second_look, devices].into_iter().flatten().min()
    }

    /// Watch every active playback device for new sessions, re-registering
    /// when the set of devices changed.
    fn refresh_watch(&mut self) {
        let devices = render_devices();
        let ids: Vec<&String> = devices.iter().map(|(id, _)| id).collect();
        if ids == self.watching.iter().map(|(id, _, _)| id).collect::<Vec<_>>() {
            return;
        }
        for (_, manager, notifier) in self.watching.drain(..) {
            let _ = unsafe { manager.UnregisterSessionNotification(&notifier) };
        }
        for (id, manager) in devices {
            // Notifications only start after the first enumeration.
            if unsafe { manager.GetSessionEnumerator() }.is_err() {
                continue;
            }
            let notifier: IAudioSessionNotification = Notifier.into();
            if unsafe { manager.RegisterSessionNotification(&notifier) }.is_ok() {
                self.watching.push((id, manager, notifier));
            }
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Begin { generation, mode, percent } => {
                // A recording whose end never arrived keeps its originals.
                let mut a = self.active.take().unwrap_or(Active { generation, mode, percent, changes: Vec::new(), paused: Vec::new() });
                a.generation = generation;
                a.mode = mode;
                a.percent = percent;
                self.active = Some(a);
                self.refresh_watch();
                let mut now = sessions();
                if self.resolve(&now) {
                    now = sessions();
                }
                match mode {
                    OtherAudio::Lower | OtherAudio::Mute => self.lower(&now),
                    OtherAudio::Pause => self.pause(),
                    OtherAudio::Nothing => {}
                }
            }
            Cmd::End(generation) => {
                if self.active.as_ref().is_some_and(|a| a.generation == generation) {
                    self.finish();
                }
            }
            Cmd::Flush(ack) => {
                self.finish();
                let _ = ack.send(());
            }
            Cmd::RestoreFile => {
                let path = crate::paths::audio_restore_file();
                let saved: Vec<VolumeChange> = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
                let saved = expire(saved, now_secs());
                let held = self.held_keys();
                for c in saved {
                    if !held.contains(&c.key) && !self.unresolved.iter().any(|u| u.key == c.key) {
                        self.unresolved.push(c);
                    }
                }
                self.resolve(&sessions());
                self.save();
            }
            Cmd::SessionCreated => {
                if !self.unresolved.is_empty() || self.active.as_ref().is_some_and(|a| a.lowers()) {
                    self.rescan();
                    self.second_look = Some(Instant::now() + SECOND_LOOK);
                }
            }
        }
    }

    fn held_keys(&self) -> Vec<String> {
        self.active.iter().flat_map(|a| a.changes.iter().map(|c| c.key.clone())).collect()
    }

    fn rescan(&mut self) {
        self.second_look = None;
        let lowering = self.active.as_ref().is_some_and(|a| a.lowers());
        if self.unresolved.is_empty() && !lowering {
            return;
        }
        self.refresh_watch();
        let mut now = sessions();
        if self.resolve(&now) {
            now = sessions();
        }
        if lowering {
            self.lower(&now);
        }
    }

    /// Restore unresolved changes whose app is back. Sessions the current
    /// recording holds are left to it. Returns whether anything was set.
    fn resolve(&mut self, sessions: &Sessions) -> bool {
        if self.unresolved.is_empty() {
            return false;
        }
        let held = self.held_keys();
        let free: Vec<SessionReading> = readings(sessions).into_iter().filter(|r| !held.contains(&r.key)).collect();
        let before = self.unresolved.clone();
        let result = restore(&expire(std::mem::take(&mut self.unresolved), now_secs()), &free);
        self.unresolved = result.unresolved;
        self.unresolved.extend(put_back(sessions, &result.restores));
        if self.unresolved != before {
            self.save();
        }
        !result.restores.is_empty()
    }

    fn lower(&mut self, sessions: &Sessions) {
        let Some(a) = self.active.as_ref() else { return };
        let mut new = plan(a.mode, a.percent, std::process::id(), &readings(sessions), &a.changes);
        if new.is_empty() {
            return;
        }
        let at = now_secs();
        for c in &mut new {
            c.changed_at = at;
        }
        let mut planned = self.pending();
        planned.extend(new.iter().cloned());
        write(&planned);
        let mut done = Vec::new();
        for c in &new {
            let Some((_, volume)) = sessions.iter().find(|(r, _)| r.key == c.key) else { continue };
            let (volume_ok, mute_ok) = set(volume, Some(c.to_volume), Some(c.to_muted));
            done.extend(applied(c, volume_ok, mute_ok));
        }
        if let Some(a) = self.active.as_mut() {
            a.changes.extend(done);
        }
        self.save();
    }

    fn pause(&mut self) {
        let Some(a) = self.active.as_mut() else { return };
        for (key, status, session) in media_sessions() {
            if status != MediaStatus::Playing {
                continue;
            }
            let changed = Arc::new(AtomicBool::new(false));
            let seen_paused = Arc::new(AtomicBool::new(false));
            let (flag, seen) = (changed.clone(), seen_paused.clone());
            // Events before Quill's pause lands are ignored; any change after
            // it is the user's, and that player is then left alone.
            let handler = TypedEventHandler::<MediaSession, PlaybackInfoChangedEventArgs>::new(move |sender, _| {
                if let Some(s) = sender.as_ref() {
                    if status_of(s) == MediaStatus::Paused {
                        seen.store(true, SeqCst);
                    } else if seen.load(SeqCst) {
                        flag.store(true, SeqCst);
                    }
                }
                Ok(())
            });
            let Ok(token) = session.PlaybackInfoChanged(&handler) else { continue };
            if session.TryPauseAsync().and_then(|op| op.join()).unwrap_or(false) {
                a.paused.push(Paused { session, token, seen_paused, changed });
            } else {
                log::warn!("other audio: {key} did not pause");
                let _ = session.RemovePlaybackInfoChanged(token);
            }
        }
    }

    fn finish(&mut self) {
        let Some(a) = self.active.take() else { return };
        if !a.changes.is_empty() {
            let now = sessions();
            let result = restore(&a.changes, &readings(&now));
            self.unresolved.extend(result.unresolved);
            self.unresolved.extend(put_back(&now, &result.restores));
            self.save();
        }
        for p in a.paused {
            let until = Instant::now() + PAUSE_LANDING;
            let mut status = status_of(&p.session);
            while status != MediaStatus::Paused && !p.seen_paused.load(SeqCst) && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(20));
                status = status_of(&p.session);
            }
            if should_resume(status, p.changed.load(SeqCst), p.seen_paused.load(SeqCst) || status == MediaStatus::Paused) {
                let _ = p.session.TryPlayAsync().and_then(|op| op.join());
            }
            let _ = p.session.RemovePlaybackInfoChanged(p.token);
        }
    }

    /// Everything Quill still has to put back.
    fn pending(&self) -> Vec<VolumeChange> {
        self.active.iter().flat_map(|a| a.changes.iter().cloned()).chain(self.unresolved.iter().cloned()).collect()
    }

    fn save(&self) {
        write(&self.pending());
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Apply restores; returns the changes whose restore didn't take, to try
/// again later.
fn put_back(sessions: &Sessions, restores: &[Restore]) -> Vec<VolumeChange> {
    let mut failed: Vec<VolumeChange> = Vec::new();
    for r in restores {
        let ok = sessions.iter().find(|(s, _)| s.key == r.key).is_some_and(|(_, v)| {
            let (volume_ok, mute_ok) = set(v, r.volume, r.muted);
            (r.volume.is_none() || volume_ok) && (r.muted.is_none() || mute_ok)
        });
        if !ok && !failed.contains(&r.change) {
            failed.push(r.change.clone());
        }
    }
    failed
}

fn write(changes: &[VolumeChange]) {
    let path = crate::paths::audio_restore_file();
    if changes.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&path, serde_json::to_vec(changes).unwrap_or_default()) {
        log::warn!("other audio: could not save the restore file: {e}");
    }
}

/// Sets what is given; returns which parts applied.
fn set(volume: &ISimpleAudioVolume, level: Option<f32>, muted: Option<bool>) -> (bool, bool) {
    unsafe {
        let volume_ok = level.is_some_and(|l| volume.SetMasterVolume(l.clamp(0.0, 1.0), std::ptr::null()).is_ok());
        let mute_ok = muted.is_some_and(|m| volume.SetMute(m, std::ptr::null()).is_ok());
        (volume_ok, mute_ok)
    }
}

/// Active playback devices: endpoint id and session manager.
fn render_devices() -> Vec<(String, IAudioSessionManager2)> {
    unsafe {
        let Ok(e) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return Vec::new() };
        let Ok(devices) = e.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return Vec::new() };
        (0..devices.GetCount().unwrap_or(0))
            .filter_map(|i| devices.Item(i).ok())
            .filter_map(|d| {
                let id = d.GetId().ok().map(take_string)?;
                Some((id, d.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None).ok()?))
            })
            .collect()
    }
}

fn take_string(p: windows::core::PWSTR) -> String {
    unsafe {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as _));
        s
    }
}

/// Every audio session on every active playback device.
pub(crate) fn sessions() -> Sessions {
    let mut out = Vec::new();
    for (_, manager) in render_devices() {
        unsafe {
            let Ok(list) = manager.GetSessionEnumerator() else { continue };
            for j in 0..list.GetCount().unwrap_or(0) {
                let Ok(control) = list.GetSession(j) else { continue };
                let Ok(state) = control.GetState() else { continue };
                if state == AudioSessionStateExpired {
                    continue;
                }
                let Ok(control2) = control.cast::<IAudioSessionControl2>() else { continue };
                let Ok(volume) = control.cast::<ISimpleAudioVolume>() else { continue };
                let Ok(key) = control2.GetSessionInstanceIdentifier().map(take_string) else { continue };
                let app = control2.GetSessionIdentifier().map(take_string).unwrap_or_default();
                #[cfg(test)]
                {
                    let only = ONLY_APP.lock().unwrap().to_ascii_lowercase();
                    if !only.is_empty() && !app.to_ascii_lowercase().contains(&only) {
                        continue;
                    }
                }
                let pid = control2.GetProcessId().unwrap_or(0);
                let (Ok(level), Ok(muted)) = (volume.GetMasterVolume(), volume.GetMute()) else { continue };
                let reading = SessionReading { key, app, pid, volume: level, muted: muted.as_bool(), active: state == AudioSessionStateActive };
                out.push((reading, volume));
            }
        }
    }
    out
}

fn status_of(s: &MediaSession) -> MediaStatus {
    match s.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()) {
        Ok(Playback::Playing) => MediaStatus::Playing,
        Ok(Playback::Paused) => MediaStatus::Paused,
        _ => MediaStatus::Other,
    }
}

/// Media players: app id, status, session.
fn media_sessions() -> Vec<(String, MediaStatus, MediaSession)> {
    let Ok(manager) = MediaManager::RequestAsync().and_then(|op| op.join()) else { return Vec::new() };
    let Ok(list) = manager.GetSessions() else { return Vec::new() };
    list.into_iter()
        .filter_map(|s| {
            let key = s.SourceAppUserModelId().ok()?.to_string();
            Some((key, status_of(&s), s))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Child, Command, Stdio};

    #[test]
    #[ignore = "helper: plays silence when QUILL_TEST_PLAYER is set"]
    fn silent_player() {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        if std::env::var_os("QUILL_TEST_PLAYER").is_none() {
            return;
        }
        let device = cpal::default_host().default_output_device().expect("an output device");
        let config = device.default_output_config().expect("an output config").config();
        let stream = device
            .build_output_stream(config, |data: &mut [f32], _| data.fill(0.0), |e| eprintln!("player: {e}"), None)
            .expect("an output stream");
        stream.play().expect("playing");
        std::thread::sleep(Duration::from_secs(60));
    }

    fn player() -> Child {
        Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "platform::other_audio::tests::silent_player", "--nocapture"])
            .env("QUILL_TEST_PLAYER", "1")
            .stdout(Stdio::null())
            .spawn()
            .expect("start the player")
    }

    fn reading_for(pid: u32) -> Option<SessionReading> {
        sessions().into_iter().map(|(r, _)| r).find(|r| r.pid == pid && r.active)
    }

    fn wait_for(pid: u32) -> Option<SessionReading> {
        let until = Instant::now() + Duration::from_secs(10);
        while Instant::now() < until {
            if let Some(r) = reading_for(pid) {
                return Some(r);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }

    fn set_for(pid: u32, level: f32, muted: bool) {
        if let Some((_, v)) = sessions().into_iter().find(|(r, _)| r.pid == pid) {
            set(&v, Some(level), Some(muted));
        }
    }

    fn only_test_players() {
        let exe = std::env::current_exe().unwrap();
        *ONLY_APP.lock().unwrap() = exe.file_name().unwrap().to_string_lossy().into_owned();
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
    }

    fn stop(mut child: Child) {
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    #[ignore = "plays silence in a child process and changes that process's volume"]
    fn lower_and_restore_a_real_session() {
        only_test_players();
        let child = player();
        let pid = child.id();
        let result = std::panic::catch_unwind(|| {
            wait_for(pid).expect("the player's audio session appears");
            set_for(pid, 1.0, false);
            let before = reading_for(pid).unwrap();
            println!("before: volume {:.3} muted {}", before.volume, before.muted);

            let t = Instant::now();
            let lowered = hold(1, OtherAudio::Lower, 20).expect("lower holds");
            println!("hold() returned in {} µs", t.elapsed().as_micros());
            std::thread::sleep(Duration::from_millis(600));
            let during = reading_for(pid).expect("session during");
            println!("during Lower 20%: volume {:.3} muted {}", during.volume, during.muted);
            assert!(crate::paths::audio_restore_file().exists(), "restore file written before the change");

            drop(lowered);
            restore_all();
            let after = reading_for(pid).expect("session after");
            println!("after restore: volume {:.3} muted {}", after.volume, after.muted);
            assert!(!crate::paths::audio_restore_file().exists(), "restore file cleared");
            assert!((during.volume - 0.2).abs() < 0.01, "lowered to 20%");
            assert!((after.volume - 1.0).abs() < 0.01, "restored");

            let muting = hold(2, OtherAudio::Mute, 20).expect("mute holds");
            std::thread::sleep(Duration::from_millis(600));
            let muted = reading_for(pid).expect("session muted");
            drop(muting);
            restore_all();
            let unmuted = reading_for(pid).expect("session unmuted");
            println!("Mute: during muted={} after muted={}", muted.muted, unmuted.muted);
            assert!(muted.muted && !unmuted.muted, "muted, then unmuted");
        });
        restore_all();
        stop(child);
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }

    #[test]
    #[ignore = "plays silence in a child process, lowers it, and starts another mid-recording"]
    fn audio_that_starts_mid_recording_is_lowered_too() {
        only_test_players();
        let lowered = hold(20, OtherAudio::Lower, 20).expect("lower holds");
        std::thread::sleep(Duration::from_millis(300));
        let late = player();
        let pid = late.id();
        let result = std::panic::catch_unwind(|| {
            let appeared = wait_for(pid).expect("the late player's session appears");
            std::thread::sleep(Duration::from_millis(1200));
            let during = reading_for(pid).expect("late session during");
            println!("late start: first read {:.3}, during recording {:.3}", appeared.volume, during.volume);
            drop(lowered);
            restore_all();
            let after = reading_for(pid).expect("late session after");
            println!("late start after restore: {:.3}", after.volume);
            assert!((during.volume - 0.2).abs() < 0.01, "lowered while recording");
            assert!((after.volume - 1.0).abs() < 0.01, "restored");
        });
        restore_all();
        stop(late);
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }

    #[test]
    #[ignore = "plays silence in child processes and changes their volume"]
    fn windows_keeps_a_closed_apps_volume() {
        only_test_players();
        let first = player();
        let pid = first.id();
        wait_for(pid).expect("the first player's session appears");
        set_for(pid, 0.2, false);
        stop(first);
        std::thread::sleep(Duration::from_millis(300));
        let second = player();
        let pid2 = second.id();
        let appeared = wait_for(pid2).map(|r| r.volume);
        set_for(pid2, 1.0, false);
        let reset = reading_for(pid2).map(|r| r.volume);
        stop(second);
        println!("set 0.200 by hand, closed, relaunched without Quill: {appeared:?}; reset to {reset:?}");
        assert!(appeared.is_some_and(|v| (v - 0.2).abs() < 0.01), "Windows brings the app back lowered");
    }

    #[test]
    #[ignore = "plays silence in child processes and changes their volume"]
    fn an_app_closed_while_lowered_is_restored_when_it_returns() {
        only_test_players();
        let first = player();
        let pid = first.id();
        wait_for(pid).expect("the first player's session appears");
        set_for(pid, 1.0, false);
        let lowered = hold(10, OtherAudio::Lower, 20).expect("lower holds");
        std::thread::sleep(Duration::from_millis(600));
        let during = reading_for(pid).expect("session during");
        println!("first run during Lower 20%: volume {:.3}", during.volume);
        stop(first);
        std::thread::sleep(Duration::from_millis(300));
        drop(lowered);
        restore_all();
        let kept = crate::paths::audio_restore_file().exists();
        println!("restore file kept after the app closed: {kept}");

        let second = player();
        let pid2 = second.id();
        let result = std::panic::catch_unwind(|| {
            let appeared = wait_for(pid2).expect("the second player's session appears");
            println!("second run first read: volume {:.3}", appeared.volume);
            std::thread::sleep(Duration::from_millis(1500));
            let settled = reading_for(pid2).expect("second session");
            println!("second run after 1.5 s: volume {:.3}", settled.volume);
            assert!((during.volume - 0.2).abs() < 0.01, "lowered to 20%");
            assert!(kept, "the change is kept while the app is closed");
            assert!((settled.volume - 1.0).abs() < 0.01, "restored on relaunch");
            assert!(!crate::paths::audio_restore_file().exists(), "restore file cleared once restored");
        });
        restore_all();
        stop(second);
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }
}
