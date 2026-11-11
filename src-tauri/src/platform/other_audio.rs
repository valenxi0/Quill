//! Other apps' audio while recording: lower or mute their WASAPI sessions,
//! or pause media players, then put everything back.
//!
//! One worker thread owns all of it. Commands run in order, so a recording
//! that starts right after another ends reads the restored volumes, never
//! Quill's lowered ones. What Quill changed is written to a file before the
//! first change, so a crash is undone on the next launch.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::OnceLock;
use std::time::Duration;

use quill_core::other_audio::{plan, plan_pause, plan_resume, restore, MediaReading, MediaStatus, SessionReading, VolumeChange};
use quill_core::settings::OtherAudio;
use windows::core::Interface;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as MediaSession,
    GlobalSystemMediaTransportControlsSessionManager as MediaManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Playback,
};
use windows::Win32::Media::Audio::{
    eRender, AudioSessionStateActive, AudioSessionStateExpired, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
    ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED};

enum Cmd {
    Begin { generation: u64, mode: OtherAudio, percent: u8 },
    End(u64),
    /// Restore everything now, then acknowledge.
    Flush(Sender<()>),
    RestoreFile,
}

/// What Quill changed for the recording `generation`.
struct Active {
    generation: u64,
    changes: Vec<VolumeChange>,
    paused: Vec<String>,
}

static WORKER: OnceLock<Sender<Cmd>> = OnceLock::new();

/// Tests touch only their own player's session, never the user's apps.
#[cfg(test)]
static ONLY_PID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

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

/// Undo what a crashed run left changed.
pub fn restore_pending() {
    if pending_file().exists() {
        send(Cmd::RestoreFile);
    }
}

fn pending_file() -> PathBuf {
    crate::paths::models_dir().with_file_name("audio-restore.json")
}

fn save_pending(changes: &[VolumeChange]) {
    let path = pending_file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&path, serde_json::to_vec(changes).unwrap_or_default()) {
        log::warn!("other audio: could not save the restore file: {e}");
    }
}

fn clear_pending() {
    let _ = std::fs::remove_file(pending_file());
}

fn run(rx: Receiver<Cmd>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let mut active: Option<Active> = None;
    for cmd in rx {
        match cmd {
            Cmd::Begin { generation, mode, percent } => {
                // A recording whose end never arrived keeps its originals.
                let mut a = active.take().unwrap_or(Active { generation, changes: Vec::new(), paused: Vec::new() });
                a.generation = generation;
                begin(&mut a, mode, percent);
                active = Some(a);
            }
            Cmd::End(generation) => {
                if active.as_ref().is_some_and(|a| a.generation == generation) {
                    finish(active.take().unwrap());
                }
            }
            Cmd::Flush(ack) => {
                if let Some(a) = active.take() {
                    finish(a);
                }
                let _ = ack.send(());
            }
            Cmd::RestoreFile => {
                if active.is_none() {
                    let changes: Vec<VolumeChange> =
                        std::fs::read(pending_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
                    put_back(&changes);
                    clear_pending();
                }
            }
        }
    }
}

fn begin(a: &mut Active, mode: OtherAudio, percent: u8) {
    match mode {
        OtherAudio::Lower | OtherAudio::Mute => {
            let sessions = sessions();
            let readings: Vec<SessionReading> = sessions.iter().map(|(r, _)| r.clone()).collect();
            let new = plan(mode, percent, std::process::id(), &readings, &a.changes);
            if new.is_empty() {
                return;
            }
            let all: Vec<VolumeChange> = a.changes.iter().chain(&new).cloned().collect();
            save_pending(&all);
            for c in new {
                let Some((_, volume)) = sessions.iter().find(|(r, _)| r.key == c.key) else { continue };
                if set(volume, Some(c.to_volume), Some(c.to_muted)) {
                    a.changes.push(c);
                }
            }
        }
        OtherAudio::Pause => {
            let media = media_sessions();
            let readings: Vec<MediaReading> = media.iter().map(|(r, _)| r.clone()).collect();
            for key in plan_pause(&readings) {
                let mut paused = false;
                for (r, s) in media.iter().filter(|(r, _)| r.key == key && r.status == MediaStatus::Playing) {
                    let ok = s.TryPauseAsync().and_then(|op| op.join()).unwrap_or(false);
                    if !ok {
                        log::warn!("other audio: {} did not pause", r.key);
                    }
                    paused |= ok;
                }
                if paused && !a.paused.contains(&key) {
                    a.paused.push(key);
                }
            }
        }
        OtherAudio::Nothing => {}
    }
}

fn finish(a: Active) {
    if !a.changes.is_empty() {
        put_back(&a.changes);
        clear_pending();
    }
    if !a.paused.is_empty() {
        let media = media_sessions();
        let readings: Vec<MediaReading> = media.iter().map(|(r, _)| r.clone()).collect();
        for key in plan_resume(&a.paused, &readings) {
            for (_, s) in media.iter().filter(|(r, _)| r.key == key && r.status == MediaStatus::Paused) {
                let _ = s.TryPlayAsync().and_then(|op| op.join());
            }
        }
    }
}

fn put_back(changes: &[VolumeChange]) {
    if changes.is_empty() {
        return;
    }
    let sessions = sessions();
    let readings: Vec<SessionReading> = sessions.iter().map(|(r, _)| r.clone()).collect();
    for r in restore(changes, &readings) {
        if let Some((_, volume)) = sessions.iter().find(|(s, _)| s.key == r.key) {
            set(volume, r.volume, r.muted);
        }
    }
}

fn set(volume: &ISimpleAudioVolume, level: Option<f32>, muted: Option<bool>) -> bool {
    unsafe {
        let mut ok = true;
        if let Some(level) = level {
            ok &= volume.SetMasterVolume(level.clamp(0.0, 1.0), std::ptr::null()).is_ok();
        }
        if let Some(muted) = muted {
            ok &= volume.SetMute(muted, std::ptr::null()).is_ok();
        }
        ok
    }
}

/// Every audio session on every active playback device.
pub(crate) fn sessions() -> Vec<(SessionReading, ISimpleAudioVolume)> {
    let mut out = Vec::new();
    unsafe {
        let Ok(e) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return out };
        let Ok(devices) = e.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return out };
        for i in 0..devices.GetCount().unwrap_or(0) {
            let Ok(device) = devices.Item(i) else { continue };
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
            let Ok(list) = manager.GetSessionEnumerator() else { continue };
            for j in 0..list.GetCount().unwrap_or(0) {
                let Ok(control) = list.GetSession(j) else { continue };
                let Ok(state) = control.GetState() else { continue };
                if state == AudioSessionStateExpired {
                    continue;
                }
                let Ok(control2) = control.cast::<IAudioSessionControl2>() else { continue };
                let Ok(volume) = control.cast::<ISimpleAudioVolume>() else { continue };
                let Ok(id) = control2.GetSessionInstanceIdentifier() else { continue };
                let key = id.to_string().unwrap_or_default();
                CoTaskMemFree(Some(id.0 as _));
                let pid = control2.GetProcessId().unwrap_or(0);
                #[cfg(test)]
                if pid != ONLY_PID.load(std::sync::atomic::Ordering::SeqCst) {
                    continue;
                }
                let (Ok(level), Ok(muted)) = (volume.GetMasterVolume(), volume.GetMute()) else { continue };
                let reading = SessionReading { key, pid, volume: level, muted: muted.as_bool(), active: state == AudioSessionStateActive };
                out.push((reading, volume));
            }
        }
    }
    out
}

fn media_sessions() -> Vec<(MediaReading, MediaSession)> {
    let Ok(manager) = MediaManager::RequestAsync().and_then(|op| op.join()) else { return Vec::new() };
    let Ok(list) = manager.GetSessions() else { return Vec::new() };
    list.into_iter()
        .filter_map(|s| {
            let key = s.SourceAppUserModelId().ok()?.to_string();
            let status = match s.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()).ok()? {
                Playback::Playing => MediaStatus::Playing,
                Playback::Paused => MediaStatus::Paused,
                _ => MediaStatus::Other,
            };
            Some((MediaReading { key, status }, s))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    fn reading_for(pid: u32) -> Option<SessionReading> {
        sessions().into_iter().map(|(r, _)| r).find(|r| r.pid == pid && r.active)
    }

    fn wait_for(pid: u32) -> Option<SessionReading> {
        let until = Instant::now() + Duration::from_secs(8);
        while Instant::now() < until {
            if let Some(r) = reading_for(pid) {
                return Some(r);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        None
    }

    #[test]
    #[ignore = "plays a sound and changes its volume on this PC"]
    fn lower_and_restore_a_real_session() {
        let mut player = Command::new("powershell")
            .args(["-NoProfile", "-Command", "(New-Object Media.SoundPlayer 'C:\\Windows\\Media\\Alarm01.wav').PlayLooping(); Start-Sleep 30"])
            .stdout(Stdio::null())
            .spawn()
            .expect("start the player");
        let pid = player.id();
        ONLY_PID.store(pid, std::sync::atomic::Ordering::SeqCst);
        let result = std::panic::catch_unwind(|| {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            let before = wait_for(pid).expect("the player's audio session appears");
            println!("before: volume {:.3} muted {}", before.volume, before.muted);

            let t = Instant::now();
            let lowered = hold(1, OtherAudio::Lower, 20).expect("lower holds");
            println!("hold() returned in {} µs", t.elapsed().as_micros());
            std::thread::sleep(Duration::from_millis(600));
            let during = reading_for(pid).expect("session during");
            println!("during Lower 20%: volume {:.3} muted {}", during.volume, during.muted);
            assert!(pending_file().exists(), "restore file written before the change");

            drop(lowered);
            restore_all();
            let after = reading_for(pid).expect("session after");
            println!("after restore: volume {:.3} muted {}", after.volume, after.muted);
            assert!(!pending_file().exists(), "restore file cleared");
            assert!((during.volume - before.volume * 0.2).abs() < 0.01, "lowered to 20%");
            assert!((after.volume - before.volume).abs() < 0.01, "restored");

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
        let _ = player.kill();
        let _ = player.wait();
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
    }
}
