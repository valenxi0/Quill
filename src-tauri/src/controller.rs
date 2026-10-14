//! The dictation controller (section 4): hotkey events in, pasted text out.
//!
//! A session generation counter guards every async step. A cancelled or
//! stale session never delivers text, updates the phase, records stats, or
//! learns anything.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use quill_core::context::{classify, AppContext};
use quill_core::corrections::{diff, Anchor, CorrectionPair};
use quill_core::engines::{recognition_language, EngineId, Recognizer};
use quill_core::errors::DictationError;
use quill_core::history::HistoryEntry;
use quill_core::pipeline::{prepare, process_segment, protected_terms, Cleaner, DeliveryOptions, Recipe, SegmentResult};
use quill_core::segments::{pad, split, EarlySegmenter, SAMPLE_RATE};
use quill_core::text::directive::Directive;
use quill_core::settings::{level_for_app, CleanupLevel};
use quill_core::state::{action, Action, HotkeyEvent, Phase};
use quill_core::stats::Session as StatSession;
use quill_core::text::finish::{has_speech, join, peak, rms_dbfs, MIN_SPEECH_PEAK};
use quill_core::text::numbers::Itn;
use quill_core::text::spoken::paste_chunks;
use quill_core::text::vocabulary::{for_app, vocabulary_terms};
use serde::Serialize;

use crate::audio::{AudioError, Recording};
use crate::engines::{EngineHost, TranscribeRequest};
use crate::platform::hotkey::HotkeyHook;
use crate::platform::system::{microphone_permission, MicPermission};
use crate::platform::{paste, target, uia};
use crate::shell::Shell;
use crate::stores::Stores;
use crate::vad::Vad;

/// What the overlay and tray show.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DictationView {
    pub phase: Phase,
    /// "Paste sent" / "Copied only" after completion.
    pub outcome: Option<String>,
    pub error_title: Option<String>,
    pub error_detail: Option<String>,
    pub context: AppContext,
}

impl Default for DictationView {
    fn default() -> Self {
        Self { phase: Phase::Idle, outcome: None, error_title: None, error_detail: None, context: AppContext::General }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub audio_seconds: f64,
    pub model_load_ms: Option<u64>,
    pub transcription_ms: u64,
    pub cleanup_ms: u64,
    pub post_processing_ms: u64,
    pub stop_to_clipboard_ms: u64,
    pub cold_start: bool,
    pub accelerator: String,
    pub cleanup_fallbacks: usize,
    pub guard_restores: usize,
}

struct Session {
    generation: u64,
    recording: Option<Recording>,
    segmenter: EarlySegmenter,
    worker: Sender<Job>,
    /// Fed by the monitor; `None` until it loads, or if it can't.
    vad: Option<Vad>,
    preview: Option<Preview>,
}

/// Live preview state for one recording.
struct Preview {
    engine: EngineId,
    request: TranscribeRequest,
    /// Raw text of early segments, appended by the worker.
    committed: Arc<Mutex<Vec<String>>>,
    busy: Arc<AtomicBool>,
    next_at: Instant,
    /// Audio already covered by the last preview.
    covered: usize,
}

/// The longest stretch of audio a preview decodes.
const PREVIEW_WINDOW: usize = SAMPLE_RATE * 15;

enum Job {
    Segment(Vec<f32>),
    Finish { stopped: Instant, audio_seconds: f64, rms_dbfs: f32 },
}

struct SessionConfig {
    generation: u64,
    engine: EngineId,
    language: Option<String>,
    vocabulary_terms: Vec<String>,
    recipe: Recipe,
    level: CleanupLevel,
    target: target::Target,
    history_at_start: bool,
    spoken_enter: bool,
    splits_terminal_pastes: bool,
    /// Vocabulary output whose casing survives fitting to the cursor.
    protected: Vec<String>,
    preview_committed: Option<Arc<Mutex<Vec<String>>>>,
}

pub struct Controller {
    pub stores: Arc<Stores>,
    pub engines: Arc<EngineHost>,
    pub models: Arc<crate::models::ModelManager>,
    pub hook: Option<HotkeyHook>,
    shell: Arc<dyn Shell>,
    itn: Option<Box<dyn Itn>>,
    preferred_languages: Vec<String>,
    generation: AtomicU64,
    phase: Mutex<DictationView>,
    session: Mutex<Option<Session>>,
    /// Bumped by each outcome, so a stale hold timer can't reset a newer one.
    outcome_seq: AtomicU64,
    watch_generation: AtomicU64,
    starting: std::sync::atomic::AtomicBool,
    pub last_transcript: Mutex<Option<String>>,
    /// Where the last transcript went, for "Paste last transcript".
    last_target: Mutex<Option<target::Target>>,
    pub last_metrics: Mutex<Option<Metrics>>,
    pub warm_error: Mutex<Option<String>>,
}

impl Controller {
    pub fn new(
        stores: Arc<Stores>,
        engines: Arc<EngineHost>,
        models: Arc<crate::models::ModelManager>,
        hook: Option<HotkeyHook>,
        shell: Arc<dyn Shell>,
        itn: Option<Box<dyn Itn>>,
        preferred_languages: Vec<String>,
    ) -> Arc<Self> {
        Arc::new(Self {
            stores,
            engines,
            models,
            hook,
            shell,
            itn,
            preferred_languages,
            generation: AtomicU64::new(0),
            phase: Mutex::new(DictationView::default()),
            session: Mutex::new(None),
            outcome_seq: AtomicU64::new(0),
            watch_generation: AtomicU64::new(0),
            starting: std::sync::atomic::AtomicBool::new(false),
            last_transcript: Mutex::new(None),
            last_target: Mutex::new(None),
            last_metrics: Mutex::new(None),
            warm_error: Mutex::new(None),
        })
    }

    pub fn itn_available(&self) -> bool {
        self.itn.is_some()
    }

    pub fn view(&self) -> DictationView {
        self.phase.lock().clone()
    }

    pub fn phase(&self) -> Phase {
        self.phase.lock().phase
    }

    fn is_current(&self, generation: u64) -> bool {
        self.generation.load(SeqCst) == generation
    }

    /// Change the view under the phase lock. `f` returns false to leave it
    /// alone. The generation only changes under this lock too, so a check
    /// inside `f` can't race a cancel. Returns whether the view changed.
    fn transition(&self, f: impl FnOnce(&mut DictationView) -> bool) -> bool {
        let view = {
            let mut v = self.phase.lock();
            let mut next = v.clone();
            if !f(&mut next) || next == *v {
                return false;
            }
            *v = next.clone();
            next
        };
        if let Some(h) = &self.hook {
            h.set_escape_active(view.phase.is_busy());
        }
        self.shell.dictation_changed(&view);
        true
    }

    /// Move a live session from one busy phase to the next.
    fn advance(&self, generation: u64, from: Phase, to: Phase) {
        self.transition(|v| {
            if !self.is_current(generation) || v.phase != from {
                return false;
            }
            v.phase = to;
            true
        });
    }

    /// Show an outcome (only for `generation`, if given), then return to idle
    /// after its hold time. Returns whether it was shown.
    fn outcome(self: &Arc<Self>, generation: Option<u64>, view: DictationView) -> bool {
        let hold = view.phase.hold().unwrap_or(Duration::from_secs(1));
        let mut seq = 0;
        let shown = self.transition(|v| {
            if generation.is_some_and(|g| !self.is_current(g)) {
                return false;
            }
            seq = self.outcome_seq.fetch_add(1, SeqCst) + 1;
            *v = view;
            true
        });
        if shown {
            let me = self.clone();
            std::thread::spawn(move || {
                std::thread::sleep(hold);
                me.transition(|v| {
                    if me.outcome_seq.load(SeqCst) != seq || v.phase.is_busy() {
                        return false;
                    }
                    *v = DictationView::default();
                    true
                });
            });
        }
        shown
    }

    fn error_view(&self, error: &DictationError) -> DictationView {
        DictationView {
            phase: if *error == DictationError::Cancelled { Phase::Cancelled } else { Phase::Failed },
            outcome: None,
            error_title: Some(error.title()),
            error_detail: Some(error.detail()),
            context: self.view().context,
        }
    }

    /// A failure outside any session (readiness, microphone).
    fn fail(self: &Arc<Self>, error: DictationError) {
        self.outcome(None, self.error_view(&error));
        self.schedule_idle_unload();
    }

    /// A failure of one session; ignored if the session is stale.
    fn fail_session(self: &Arc<Self>, generation: u64, error: DictationError) {
        self.outcome(Some(generation), self.error_view(&error));
        self.schedule_idle_unload();
    }

    pub fn on_hotkey(self: &Arc<Self>, event: HotkeyEvent) {
        let mode = self.stores.settings.read(|s| s.voice_dictation.activation_mode);
        match action(mode, self.phase(), event) {
            Action::Start => self.start(),
            Action::Stop => self.stop(),
            Action::Cancel => self.cancel(),
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
        // One start at a time (the hotkey and a launcher may race).
        if self.phase().is_busy() || self.starting.swap(true, SeqCst) {
            return;
        }
        self.start_inner();
        self.starting.store(false, SeqCst);
    }

    fn start_inner(self: &Arc<Self>) {
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
        let recording = match Recording::start() {
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
        let terms = vocabulary_terms(&vocabulary);
        let protected = protected_terms(&vocabulary);
        let level = level_for_app(vd.cleanup_level, &vd.app_styles, &target.exe);
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
        *self.session.lock() = Some(Session {
            generation,
            recording: Some(recording),
            segmenter: EarlySegmenter::default(),
            worker,
            vad: None,
            preview,
        });

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
                    if last_segment_check.elapsed() >= Duration::from_secs(1) {
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

    /// One worker per session processes segments one at a time, in order.
    fn spawn_worker(self: &Arc<Self>, cfg: SessionConfig) -> Sender<Job> {
        let (tx, rx) = channel::<Job>();
        let me = self.clone();
        std::thread::Builder::new()
            .name("quill-dictation".into())
            .spawn(move || {
                let mut results: Vec<SegmentResult> = Vec::new();
                let mut metrics = Metrics::default();
                let mut failure: Option<DictationError> = None;
                for job in rx {
                    if !me.is_current(cfg.generation) {
                        return;
                    }
                    match job {
                        Job::Segment(samples) => {
                            if failure.is_some() {
                                continue;
                            }
                            match me.process(&cfg, &samples, &mut metrics) {
                                Ok(r) => {
                                    if let Some(c) = &cfg.preview_committed {
                                        c.lock().push(r.raw.clone());
                                    }
                                    results.push(r)
                                }
                                Err(e) => failure = Some(e),
                            }
                        }
                        Job::Finish { stopped, audio_seconds, rms_dbfs } => {
                            metrics.audio_seconds = audio_seconds;
                            if let Some(e) = failure.take() {
                                me.fail_session(cfg.generation, e);
                                return;
                            }
                            me.deliver(&cfg, results, metrics, stopped, rms_dbfs);
                            return;
                        }
                    }
                }
            })
            .expect("spawn dictation worker");
        tx
    }

    /// Recognize and process one segment.
    fn process(self: &Arc<Self>, cfg: &SessionConfig, samples: &[f32], metrics: &mut Metrics) -> Result<SegmentResult, DictationError> {
        let info = cfg.engine.info();
        let request = TranscribeRequest { vocabulary_terms: cfg.vocabulary_terms.clone(), language: cfg.language.clone() };
        let min_seconds = if info.recognizer == Recognizer::Whisper { 1.1 } else { 1.0 };

        let start = Instant::now();
        let (result, cold) = self
            .engines
            .with_recognizer(cfg.engine, |e| {
                if info.recognizer == Recognizer::Cohere {
                    // The decoder cache runs out mid-word past ~25 s.
                    let mut texts = Vec::new();
                    let mut language = None;
                    for range in split(samples, 20.0) {
                        let t = e.transcribe(&pad(&samples[range], min_seconds), &request)?;
                        language = language.or(t.language);
                        texts.push(t.text);
                    }
                    Ok(crate::engines::Transcript { text: join(&texts), language, boosted_terms: vec![] })
                } else {
                    e.transcribe(&pad(samples, min_seconds), &request)
                }
            })
            .map_err(|message| DictationError::ModelLoadFailed { message })?;
        let transcript = result.map_err(|message| DictationError::RecognitionFailed { message })?;
        let elapsed = start.elapsed();
        if let Some(c) = cold {
            metrics.cold_start = true;
            metrics.model_load_ms = Some(metrics.model_load_ms.unwrap_or(0) + c.as_millis() as u64);
            metrics.transcription_ms += (elapsed - c).as_millis() as u64;
        } else {
            metrics.transcription_ms += elapsed.as_millis() as u64;
        }
        metrics.accelerator = self.engines.accelerator(cfg.engine).unwrap_or_default();
        if !self.is_current(cfg.generation) {
            return Err(DictationError::Cancelled);
        }

        let post_start = Instant::now();
        let mut cleaner = HostCleaner { controller: self, generation: cfg.generation, time: Duration::ZERO, cold: None };
        let result = process_segment(
            &transcript.text,
            transcript.language.as_deref(),
            &cfg.recipe,
            if info.has_cleanup { Some(&mut cleaner as &mut dyn Cleaner) } else { None },
            self.itn.as_deref(),
        );
        let cleanup = cleaner.time;
        if let Some(c) = cleaner.cold {
            metrics.cold_start = true;
            metrics.model_load_ms = Some(metrics.model_load_ms.unwrap_or(0) + c.as_millis() as u64);
        }
        metrics.cleanup_ms += cleanup.as_millis() as u64;
        metrics.post_processing_ms += post_start.elapsed().saturating_sub(cleanup).as_millis() as u64;
        metrics.cleanup_fallbacks += result.fallbacks;
        metrics.guard_restores += result.restored;
        Ok(result)
    }

    fn deliver(self: &Arc<Self>, cfg: &SessionConfig, results: Vec<SegmentResult>, mut metrics: Metrics, stopped: Instant, rms: f32) {
        if !self.is_current(cfg.generation) {
            return;
        }
        let raw = join(&results.iter().map(|r| r.raw.clone()).collect::<Vec<_>>());
        let text = join(&results.iter().map(|r| r.text.clone()).collect::<Vec<_>>());
        let options = DeliveryOptions {
            spoken_enter: cfg.spoken_enter,
            around: cfg.target.surroundings.as_ref(),
            protected: &cfg.protected,
        };
        let delivery = match prepare(&raw, &text, cfg.recipe.context, rms, &options) {
            Ok(d) => d,
            Err(e) => return self.fail_session(cfg.generation, e),
        };
        if !self.is_current(cfg.generation) {
            return;
        }
        let own = self.shell.own_windows();
        let still_current = || self.is_current(cfg.generation);

        // Only "press enter" was said: press it and deliver no text.
        if delivery.text.is_empty() {
            let pressed = paste::focus(&cfg.target, &own) && still_current() && paste::press_enter();
            self.outcome(
                Some(cfg.generation),
                DictationView {
                    phase: Phase::Completed,
                    outcome: Some(if pressed { "Enter pressed" } else { "Nothing to paste" }.into()),
                    context: cfg.recipe.context,
                    ..Default::default()
                },
            );
            return;
        }

        // The clipboard is authoritative: write it first.
        let text = delivery.text.trim().to_string();
        *self.last_transcript.lock() = Some(text.clone());
        *self.last_target.lock() = Some(cfg.target.clone());
        if !paste::set_clipboard_text(&delivery.text) {
            self.fail_session(cfg.generation, DictationError::ClipboardFailed);
            return;
        }
        metrics.stop_to_clipboard_ms = stopped.elapsed().as_millis() as u64;
        let chunks = if cfg.recipe.context == AppContext::Terminal && cfg.splits_terminal_pastes {
            paste_chunks(&delivery.text)
        } else {
            vec![delivery.text.clone()]
        };
        // Checked again right before each keystroke: Esc can land while the
        // paste waits for the shortcut's modifiers to come up.
        let pasted = if chunks.len() > 1 {
            let sent = paste_in_pieces(&cfg.target, &own, &chunks, still_current);
            // Leave the whole text on the clipboard, not the last piece.
            paste::set_clipboard_text(&delivery.text);
            sent
        } else {
            paste::paste(&cfg.target, &own, still_current)
        };
        let entered = pasted && delivery.press_enter && still_current() && {
            std::thread::sleep(Duration::from_millis(120));
            paste::press_enter()
        };
        let release_to_paste = stopped.elapsed();

        let outcome = match (pasted, entered) {
            (true, true) => "Pasted and sent",
            (true, false) => "Paste sent",
            _ => "Copied only",
        };
        let shown = self.outcome(
            Some(cfg.generation),
            DictationView {
                phase: Phase::Completed,
                outcome: Some(outcome.into()),
                error_title: None,
                error_detail: None,
                context: cfg.recipe.context,
            },
        );
        if !shown {
            // Cancelled during delivery: nothing is recorded or learned.
            return;
        }
        self.record(cfg, &results, &raw, &text, &metrics, release_to_paste, pasted);
        *self.last_metrics.lock() = Some(metrics);
        self.shell.state_changed();
        self.schedule_idle_unload();
    }

    /// Paste the last transcript again, into the app it went to.
    pub fn paste_last(self: &Arc<Self>) {
        let Some(text) = self.last_transcript.lock().clone() else { return };
        let target = self.last_target.lock().clone().unwrap_or_default();
        let own = self.shell.own_windows();
        std::thread::spawn(move || {
            if paste::set_clipboard_text(&text) {
                paste::paste(&target, &own, || true);
            }
        });
    }

    /// Run a history entry's raw transcript through cleanup again at
    /// `level`. Uses S1-mini when it is installed, whatever the engine.
    pub fn redo(self: &Arc<Self>, raw: &str, level: CleanupLevel, app_exe: &str, context: AppContext) -> Result<String, String> {
        if raw.trim().is_empty() {
            return Err("This entry has no raw transcript to redo.".into());
        }
        let has_cleanup = self.models.is_ready(quill_core::engines::AssetId::S1Mini);
        if level != CleanupLevel::Verbatim && !has_cleanup {
            return Err("Download Parakeet + S1-mini to redo with cleanup.".into());
        }
        let vd = self.stores.settings.get().voice_dictation;
        let recipe = Recipe {
            directive: Directive::for_recording(level, context, vd.formal_register),
            layout_cues: vd.layout_cues,
            context,
            writes_numbers: vd.writes_numbers,
            has_cleanup,
            vocabulary: for_app(&vd.vocabulary, app_exe),
        };
        let mut cleaner = EngineCleaner { engines: &self.engines };
        let cleaner = has_cleanup.then_some(&mut cleaner as &mut dyn Cleaner);
        let result = process_segment(raw, None, &recipe, cleaner, self.itn.as_deref());
        let text = prepare(raw, &result.text, context, 0.0, &DeliveryOptions::default()).map_err(|e| e.to_string())?.text;
        self.schedule_idle_unload();
        Ok(text)
    }

    /// Start a preview decode of the audio not yet committed, if one is due.
    fn poll_preview(self: &Arc<Self>, generation: u64, s: &mut Session) {
        let Some(p) = s.preview.as_mut() else { return };
        let Some(rec) = s.recording.as_ref() else { return };
        if Instant::now() < p.next_at || p.busy.load(SeqCst) || !self.engines.recognizer_loaded(p.engine) {
            return;
        }
        let committed = s.segmenter.committed();
        let (audio, len) = rec.with_samples(|samples| {
            let start = committed.max(samples.len().saturating_sub(PREVIEW_WINDOW));
            (samples[start.min(samples.len())..].to_vec(), samples.len())
        });
        // Skip when nothing new was said since the last preview.
        let fresh = p.covered.max(committed)..len;
        if fresh.is_empty() || s.vad.as_ref().is_some_and(|v| !v.map.has_speech(&fresh)) {
            p.next_at = Instant::now() + Duration::from_millis(300);
            return;
        }
        p.covered = len;
        p.busy.store(true, SeqCst);
        let me = self.clone();
        let (engine, request, earlier, busy) = (p.engine, p.request.clone(), p.committed.clone(), p.busy.clone());
        let started = Instant::now();
        std::thread::spawn(move || {
            let heard = me
                .engines
                .with_recognizer(engine, |e| e.transcribe(&pad(&audio, 1.0), &request))
                .ok()
                .and_then(|(r, _)| r.ok())
                .map(|t| t.text);
            // Wait at least as long as this decode took before the next one.
            let spent = started.elapsed();
            if let Some(s) = me.session.lock().as_mut().filter(|s| s.generation == generation) {
                if let Some(p) = s.preview.as_mut() {
                    p.next_at = Instant::now() + spent.max(Duration::from_millis(700));
                }
            }
            busy.store(false, SeqCst);
            if let Some(heard) = heard {
                if me.is_current(generation) && me.phase() == Phase::Recording {
                    let mut parts = earlier.lock().clone();
                    parts.push(heard);
                    me.shell.preview(&join(&parts));
                }
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        self: &Arc<Self>,
        cfg: &SessionConfig,
        results: &[SegmentResult],
        raw: &str,
        text: &str,
        metrics: &Metrics,
        release_to_paste: Duration,
        pasted: bool,
    ) {
        let settings = self.stores.settings.get();
        let vd = &settings.voice_dictation;
        let app_name = if cfg.target.app_name.is_empty() { "Unknown app".to_string() } else { cfg.target.app_name.clone() };
        if cfg.history_at_start && vd.history_enabled {
            self.stores.history.update(|h| {
                h.add(HistoryEntry {
                    id: uuid::Uuid::new_v4(),
                    text: text.to_string(),
                    raw: raw.to_string(),
                    level: Some(cfg.level),
                    context: cfg.recipe.context,
                    engine: cfg.engine,
                    app_name: app_name.clone(),
                    app_exe: cfg.target.exe.clone(),
                    seconds_spoken: metrics.audio_seconds,
                    release_to_paste_ms: release_to_paste.as_millis() as u64,
                    created_at: chrono::Utc::now(),
                })
            });
            self.shell.history_changed();
        }
        if vd.insights_enabled {
            let icon = crate::platform::icons::file_icon(&cfg.target.exe_path);
            let hits: Vec<uuid::Uuid> = results.iter().flat_map(|r| r.hits.iter().copied()).collect();
            let latency = (!metrics.cold_start).then(|| release_to_paste.as_secs_f64());
            self.stores.stats.update(|s| {
                s.record(StatSession {
                    date: chrono::Local::now().date_naive(),
                    raw,
                    text,
                    seconds: metrics.audio_seconds,
                    app_exe: &cfg.target.exe,
                    app_name: &app_name,
                    app_icon: icon.as_deref(),
                    hits: &hits,
                    latency,
                })
            });
        }
        if pasted && vd.learns_corrections && !cfg.target.is_password {
            if let Some(id) = cfg.target.element_id {
                self.watch_corrections(id, text.to_string(), cfg.target.exe.clone());
            }
        }
    }

    /// Reread the field at 12 s and 45 s and learn misheard-word fixes.
    fn watch_corrections(self: &Arc<Self>, element: u64, pasted: String, app_exe: String) {
        let watch = self.watch_generation.fetch_add(1, SeqCst) + 1;
        let me = self.clone();
        std::thread::spawn(move || {
            let read = move || uia::run(1500, move |u| u.read_text(element)).flatten();
            std::thread::sleep(Duration::from_millis(400));
            let Some(anchor) = read().and_then(|field| Anchor::new(&field, &pasted)) else { return };
            let mut latest: Option<String> = None;
            let mut waited = Duration::from_millis(400);
            for at in [Duration::from_secs(12), Duration::from_secs(45)] {
                std::thread::sleep(at - waited);
                waited = at;
                if me.watch_generation.load(SeqCst) != watch {
                    return;
                }
                // A cleared field means the message was sent: keep the previous read.
                match read() {
                    Some(field) if !field.trim().is_empty() => {
                        if let Some(region) = anchor.locate(&field) {
                            latest = Some(region.to_string());
                        }
                    }
                    _ => {}
                }
            }
            let _ = uia::run(500, move |u| u.forget(element));
            if me.watch_generation.load(SeqCst) != watch {
                return;
            }
            let Some(edited) = latest else { return };
            let fixes = diff(&anchor.pasted, &edited);
            if fixes.is_empty() {
                return;
            }
            let vocabulary = me.stores.settings.read(|s| s.voice_dictation.vocabulary.clone());
            let promoted: Vec<CorrectionPair> = me.stores.corrections.update(|c| c.record_for_app(&fixes, (!app_exe.is_empty()).then_some(app_exe.as_str())));
            let visible: Vec<CorrectionPair> = me
                .stores
                .corrections
                .read(|c| c.suggestions(&vocabulary))
                .into_iter()
                .filter(|s| promoted.iter().any(|p| p.from == s.from && p.to == s.to && p.app_exe == s.app_exe))
                .collect();
            if let Some(first) = visible.first() {
                me.shell.suggest(first);
            }
            me.shell.state_changed();
        });
    }

    fn schedule_idle_unload(self: &Arc<Self>) {
        let me = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(crate::engines::IDLE_UNLOAD + Duration::from_secs(1));
            if !me.phase().is_busy() && me.engines.unload_if_idle() {
                me.shell.state_changed();
            }
        });
    }

    /// Settings → "Load Engine", and the warm-up after a download.
    pub fn warm_selected(self: &Arc<Self>) {
        let engine = self.stores.settings.read(|s| s.voice_dictation.engine);
        let info = engine.info();
        if info.required.iter().any(|a| !self.models.is_ready(*a)) {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            *me.warm_error.lock() = None;
            me.shell.state_changed();
            let result = me.engines.warm(engine);
            *me.warm_error.lock() = result.err();
            me.shell.state_changed();
            me.schedule_idle_unload();
        });
    }
}

/// Pieces of a long terminal paste, one after another. Returns whether all
/// were sent.
fn paste_in_pieces(target: &target::Target, own: &[isize], chunks: &[String], still_current: impl Fn() -> bool) -> bool {
    for (i, chunk) in chunks.iter().enumerate() {
        if i > 0 {
            // Let the app read the clipboard before it changes.
            std::thread::sleep(Duration::from_millis(150));
        }
        if !still_current() || !paste::set_clipboard_text(chunk) || !paste::paste(target, own, &still_current) {
            return false;
        }
    }
    true
}

/// S1-mini for a redo, outside any dictation.
struct EngineCleaner<'a> {
    engines: &'a EngineHost,
}

impl Cleaner for EngineCleaner<'_> {
    fn clean(&mut self, text: &str, directive: Directive) -> Result<String, String> {
        self.engines.with_cleaner(|c| c.clean(text, directive)).and_then(|(r, _)| r)
    }
}

/// Adapts the engine host's S1-mini to the pipeline's `Cleaner`.
struct HostCleaner<'a> {
    controller: &'a Arc<Controller>,
    generation: u64,
    time: Duration,
    cold: Option<Duration>,
}

impl Cleaner for HostCleaner<'_> {
    fn clean(&mut self, text: &str, directive: Directive) -> Result<String, String> {
        if !self.controller.is_current(self.generation) {
            return Err("cancelled".into());
        }
        self.controller.advance(self.generation, Phase::Transcribing, Phase::Cleaning);
        let start = Instant::now();
        let result = self.controller.engines.with_cleaner(|c| c.clean(text, directive));
        let elapsed = start.elapsed();
        match result {
            Ok((r, cold)) => {
                if let Some(c) = cold {
                    self.cold = Some(self.cold.unwrap_or_default() + c);
                    self.time += elapsed.saturating_sub(c);
                } else {
                    self.time += elapsed;
                }
                r
            }
            Err(e) => {
                self.time += elapsed;
                Err(e)
            }
        }
    }
}
