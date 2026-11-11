use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use quill_core::context::classify;
use quill_core::engines::{recognition_language, Recognizer};
use quill_core::errors::DictationError;
use quill_core::pipeline::{protected_terms, Recipe};
use quill_core::segments::{EarlySegmenter, SAMPLE_RATE};
use quill_core::text::directive::Directive;
use quill_core::settings::{level_for_app, CleanupLevel};
use quill_core::state::{Action, Binding, Input, Phase};
use quill_core::text::finish::{has_speech, peak, rms_dbfs, MIN_SPEECH_PEAK};
use quill_core::text::vocabulary::{app_terms_first, for_app, vocabulary_terms};

use crate::audio::{AudioError, Recording};
use crate::engines::TranscribeRequest;
use crate::platform::hotkey::now_ms;
use crate::platform::system::{microphone_permission, MicPermission};
use crate::platform::target;
use crate::vad::Vad;

use super::{Controller, DictationView, Job, Preview, Session, SessionConfig};

impl Controller {
    /// A shortcut event, stamped `at` on the hook's clock. `binding` is
    /// `None` for Escape and timer ticks.
    pub fn on_hotkey(self: &Arc<Self>, input: Input, binding: Option<Binding>, at: u64) {
        let (mode, lone) = self.stores.settings.read(|s| {
            let vd = &s.voice_dictation;
            let key = match binding {
                Some(Binding::Dictate) => Some(vd.hotkey),
                Some(Binding::Raw) => vd.raw_hotkey,
                None => None,
            };
            (vd.activation_mode, key.is_some_and(|k| k.is_lone_modifier()))
        });
        let (act, deadline) = {
            let mut shortcut = self.shortcut.lock();
            let phase = self.phase();
            // While a session runs, only the shortcut that started it drives it.
            if let (Some(b), Some((owner, generation))) = (binding, shortcut.owner) {
                if b != owner && phase.is_busy() && self.is_current(generation) {
                    return;
                }
            }
            shortcut.machine.on(mode, lone, input, at, phase)
        };
        if let (Some(due), Some(hook)) = (deadline, self.hook) {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(due.saturating_sub(now_ms())));
                hook.post_tick();
            });
        }
        match act {
            Action::Start => {
                let raw = binding == Some(Binding::Raw);
                self.start_as(raw);
                if let Some(b) = binding.filter(|_| self.phase() == Phase::Recording) {
                    self.shortcut.lock().owner = Some((b, self.generation.load(SeqCst)));
                }
            }
            Action::Stop => self.stop(),
            Action::Cancel => self.cancel(),
            Action::Discard => self.discard(),
            Action::Ignore => {}
        }
    }

    /// Launcher commands (`quill://…`, `--toggle`, tray).
    pub fn command(self: &Arc<Self>, cmd: &str) {
        let phase = self.phase();
        match cmd {
            "start" if !phase.is_busy() => self.start(),
            "stop" if phase == Phase::Recording => self.stop(),
            "toggle" => {
                if phase == Phase::Recording {
                    self.stop()
                } else if !phase.is_busy() {
                    self.start()
                }
            }
            "cancel" if phase.is_busy() => self.cancel(),
            "paste-last" if !phase.is_busy() => self.paste_last(),
            "sketch" => self.sketch(),
            _ => {}
        }
    }

    /// The first error that stops a dictation from starting, if any.
    pub fn readiness_error(&self) -> Option<DictationError> {
        let settings = self.stores.settings.get();
        if !settings.enabled {
            return Some(DictationError::FeatureOff);
        }
        let info = settings.voice_dictation.engine.info();
        let missing = info.required.iter().filter(|a| !self.models.is_ready(**a)).count();
        if missing > 0 {
            return Some(DictationError::ModelMissing { engine: info.title.to_string(), downloads: missing });
        }
        if microphone_permission() == MicPermission::Denied {
            return Some(DictationError::MicrophoneDenied);
        }
        if !crate::audio::has_input_device() {
            return Some(DictationError::MicrophoneUnavailable);
        }
        None
    }

    pub fn start(self: &Arc<Self>) {
        self.start_as(false);
    }

    /// `raw`: dictate without cleanup, whatever the style.
    fn start_as(self: &Arc<Self>, raw: bool) {
        // One start at a time (the hotkey and a launcher may race).
        if self.phase().is_busy() || self.starting.swap(true, SeqCst) {
            return;
        }
        self.begin(None, raw);
        self.starting.store(false, SeqCst);
    }

    pub(super) fn start_inner(self: &Arc<Self>, sketch: Option<crate::platform::screen::Monitor>) {
        self.begin(sketch, false);
    }

    fn begin(self: &Arc<Self>, sketch: Option<crate::platform::screen::Monitor>, raw: bool) {
        if self.phase().is_busy() {
            return;
        }
        if let Some(e) = self.readiness_error() {
            self.fail(e);
            return;
        }
        // A new dictation stops the current correction watch.
        self.watch_generation.fetch_add(1, SeqCst);
        let settings = self.stores.settings.get();
        let vd = &settings.voice_dictation;

        // Capture the target while the microphone opens, so a slow UI
        // Automation lookup doesn't clip the first word.
        let fits_to_cursor = vd.fits_to_cursor;
        let capture = std::thread::spawn(move || target::capture(fits_to_cursor));
        let mic = crate::audio::Microphone { saved: vd.input_device.clone(), avoid_bluetooth: vd.avoid_bluetooth_mic };
        let recording = match Recording::start(mic) {
            Ok(r) => r,
            Err(e) => {
                let _ = capture.join();
                return self.fail(match e {
                    AudioError::Denied => DictationError::MicrophoneDenied,
                    AudioError::NoDevice | AudioError::Failed => DictationError::MicrophoneUnavailable,
                });
            }
        };
        let target = capture.join().unwrap_or_default();
        let context = classify(&target.exe, &target.focused_name, target.aumid.as_deref(), vd.adapts_to_app);
        let info = vd.engine.info();
        let vocabulary = for_app(&vd.vocabulary, &target.exe);
        // Parakeet caps its hotwords, so this app's terms go first.
        let terms = if info.recognizer == Recognizer::Parakeet { app_terms_first(&vocabulary) } else { vocabulary_terms(&vocabulary) };
        self.engines.set_hotwords(vd.wants_hotwords());
        let protected = protected_terms(&vocabulary);
        let level = if raw { CleanupLevel::Verbatim } else { level_for_app(vd.cleanup_level, &vd.app_styles, &target.exe) };
        let recipe = Recipe {
            directive: Directive::for_recording(level, context, vd.formal_register),
            layout_cues: vd.layout_cues,
            context,
            writes_numbers: vd.writes_numbers,
            has_cleanup: info.has_cleanup,
            vocabulary,
        };

        let mut generation = 0;
        let started = self.transition(|v| {
            if v.phase.is_busy() {
                return false;
            }
            generation = self.generation.fetch_add(1, SeqCst) + 1;
            *v = DictationView { phase: Phase::Recording, context, ..Default::default() };
            true
        });
        if !started {
            return;
        }

        let language = recognition_language(vd.engine, &vd.language, &self.preferred_languages);
        let preview = vd.live_preview.then(|| Preview {
            engine: vd.engine,
            request: TranscribeRequest { vocabulary_terms: terms.clone(), language: language.clone() },
            committed: Arc::new(Mutex::new(Vec::new())),
            busy: Arc::new(AtomicBool::new(false)),
            next_at: Instant::now() + Duration::from_millis(900),
            covered: 0,
        });
        let config = SessionConfig {
            generation,
            engine: vd.engine,
            language,
            vocabulary_terms: terms,
            recipe,
            level: quill_core::context::effective_level(level, context),
            target,
            history_at_start: vd.history_enabled,
            spoken_enter: vd.spoken_enter,
            splits_terminal_pastes: vd.splits_terminal_pastes,
            protected,
            preview_committed: preview.as_ref().map(|p| p.committed.clone()),
        };
        let worker = self.spawn_worker(config);
        let clock = recording.sample_clock();
        *self.session.lock() = Some(Session {
            generation,
            sketch: sketch.is_some(),
            recording: Some(recording),
            segmenter: EarlySegmenter::default(),
            worker,
            vad: None,
            preview,
            _other_audio: crate::platform::other_audio::hold(generation, vd.other_audio, vd.lower_to_percent),
        });

        if let Some(monitor) = sketch {
            let s = &settings.sketch;
            match crate::sketch::Session::start(self.shell.clone(), monitor, s.draw_key, s.hotkey.display(), clock) {
                Ok(session) => {
                    *self.sketch.lock() = Some(session);
                    if let Some(h) = &self.hook {
                        h.set_sketching(true);
                    }
                }
                Err(e) => {
                    log::warn!("sketch: {e}");
                    self.cancel();
                    return;
                }
            }
        }

        // Warm the engine so the load overlaps the speech. A failure is kept
        // for Settings and never flashed while the user is talking.
        let me = self.clone();
        let engine = vd.engine;
        std::thread::spawn(move || {
            if let Err(e) = me.engines.warm(engine) {
                *me.warm_error.lock() = Some(e);
            }
            me.shell.state_changed();
            me.schedule_idle_unload();
        });
        self.spawn_monitor(generation);
    }

    /// Publishes the level ~12 times a second, feeds the voice activity
    /// detector, cuts early segments once a second, runs the live preview,
    /// and stops at the 10-minute cap.
    fn spawn_monitor(self: &Arc<Self>, generation: u64) {
        let me = self.clone();
        std::thread::spawn(move || {
            // Loaded here, so a slow load never delays the recording.
            let vad = Vad::new();
            if let Some(s) = me.session.lock().as_mut().filter(|s| s.generation == generation) {
                s.vad = vad;
            }
            let mut last_segment_check = Instant::now();
            loop {
                std::thread::sleep(Duration::from_millis(83));
                if !me.is_current(generation) || me.phase() != Phase::Recording {
                    return;
                }
                let mut full = false;
                {
                    let mut guard = me.session.lock();
                    let Some(s) = guard.as_mut().filter(|s| s.generation == generation) else { return };
                    let Some(rec) = s.recording.as_ref() else { return };
                    me.shell.level(rec.level());
                    full = full || rec.is_full();
                    if let Some(vad) = s.vad.as_mut() {
                        let fed = vad.fed();
                        let new = rec.with_samples(|samples| samples[fed.min(samples.len())..].to_vec());
                        vad.accept(&new);
                    }
                    if !s.sketch && last_segment_check.elapsed() >= Duration::from_secs(1) {
                        last_segment_check = Instant::now();
                        let segmenter = &mut s.segmenter;
                        let piece = rec.with_samples(|samples| segmenter.poll(samples).map(|r| (samples[r.clone()].to_vec(), r)));
                        if let Some((piece, range)) = piece {
                            // A stretch with no speech is never recognized.
                            if s.vad.as_ref().is_none_or(|v| v.map.has_speech(&range)) {
                                let _ = s.worker.send(Job::Segment(piece));
                            }
                        }
                    }
                    me.poll_preview(generation, s);
                }
                if full {
                    me.stop();
                    return;
                }
            }
        });
    }

    pub fn stop(self: &Arc<Self>) {
        let stopped = Instant::now();
        let Some(mut s) = self.session.lock().take() else { return };
        if s.generation != self.generation.load(SeqCst) {
            return;
        }
        let Some(rec) = s.recording.take() else { return };
        let samples = rec.stop();
        let audio_seconds = samples.len() as f64 / SAMPLE_RATE as f64;
        if s.sketch {
            let Some(session) = self.end_sketch() else {
                return self.fail_session(s.generation, DictationError::Cancelled);
            };
            let speech = s.vad.as_mut().map(|v| v.finish(&samples).clone());
            self.advance(s.generation, Phase::Recording, Phase::Transcribing);
            let _ = s.worker.send(Job::Sketch { samples, speech, session, stopped });
            return;
        }
        if !has_speech(&samples, SAMPLE_RATE as u32) {
            self.fail_session(s.generation, DictationError::NoSpeech);
            return;
        }
        let speech = s.vad.as_mut().map(|v| v.finish(&samples).clone());
        if speech.as_ref().is_some_and(|m| !m.has_speech(&(0..samples.len()))) {
            self.fail_session(s.generation, DictationError::NoSpeech);
            return;
        }
        self.advance(s.generation, Phase::Recording, Phase::Transcribing);
        let early = s.segmenter.committed() > 0;
        let whole_tail = s.segmenter.tail(&samples);
        let tail_range = samples.len() - whole_tail.len()..samples.len();
        // The detector trims silence at both ends of the tail. Without it, a
        // quiet tail after early segments is dropped.
        let tail = match &speech {
            Some(map) => map.crop(&tail_range).map(|r| &samples[r]),
            None => (whole_tail.len() >= SAMPLE_RATE / 4 && peak(whole_tail) >= MIN_SPEECH_PEAK).then_some(whole_tail),
        };
        match tail {
            Some(t) => {
                let _ = s.worker.send(Job::Segment(t.to_vec()));
            }
            None if !early => {
                let _ = s.worker.send(Job::Segment(whole_tail.to_vec()));
            }
            None => {}
        }
        let _ = s.worker.send(Job::Finish { stopped, audio_seconds, rms_dbfs: rms_dbfs(&samples) });
    }

    /// Drop the recording and go straight back to idle, with no outcome on
    /// screen: a quick tap in hold-or-lock, or a lone-modifier shortcut that
    /// turned out to be part of a key chord.
    pub fn discard(self: &Arc<Self>) {
        if self.session.lock().as_ref().is_none_or(|s| s.sketch) {
            return;
        }
        let discarded = self.transition(|v| {
            if v.phase != Phase::Recording {
                return false;
            }
            self.generation.fetch_add(1, SeqCst);
            self.outcome_seq.fetch_add(1, SeqCst);
            *v = DictationView::default();
            true
        });
        if discarded {
            if let Some(s) = self.session.lock().take() {
                drop(s.recording);
            }
            self.schedule_idle_unload();
        }
    }

    pub fn cancel(self: &Arc<Self>) {
        // Invalidate every in-flight step under the phase lock, so no stale
        // step can move the phase afterwards.
        let view = self.error_view(&DictationError::Cancelled);
        let mut seq = 0;
        let cancelled = self.transition(|v| {
            if !v.phase.is_busy() {
                return false;
            }
            self.generation.fetch_add(1, SeqCst);
            seq = self.outcome_seq.fetch_add(1, SeqCst) + 1;
            *v = view;
            true
        });
        if !cancelled {
            return;
        }
        if let Some(sketch) = self.end_sketch() {
            sketch.cancel();
        }
        // Drop the audio.
        if let Some(s) = self.session.lock().take() {
            drop(s.recording);
        }
        let me = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Phase::Cancelled.hold().unwrap_or_default());
            me.transition(|v| {
                if me.outcome_seq.load(SeqCst) != seq || v.phase.is_busy() {
                    return false;
                }
                *v = DictationView::default();
                true
            });
        });
        self.schedule_idle_unload();
    }
}
