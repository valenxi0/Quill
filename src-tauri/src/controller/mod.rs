//! The dictation controller (section 4): hotkey events in, pasted text out.
//!
//! A session generation counter guards every async step. A cancelled or
//! stale session never delivers text, updates the phase, records stats, or
//! learns anything.

mod deliver;
mod engines;
mod history;
mod preview;
mod process;
mod session;
mod sketch;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use quill_core::context::AppContext;
use quill_core::engines::EngineId;
use quill_core::errors::DictationError;
use quill_core::pipeline::Recipe;
use quill_core::segments::{EarlySegmenter, SAMPLE_RATE};
use quill_core::settings::CleanupLevel;
use quill_core::state::{Activation, Binding, Phase};
use quill_core::text::continuation::LastPaste;
use quill_core::text::numbers::Itn;
use serde::Serialize;

use crate::audio::Recording;
use crate::engines::{EngineHost, TranscribeRequest};
use crate::platform::hotkey::HotkeyHook;
use crate::platform::target;
use crate::shell::Shell;
use crate::stores::Stores;
use crate::vad::Vad;
use quill_core::speech::SpeechMap;

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
    /// A Sketch is cut where its marks were drawn, not at pauses.
    sketch: bool,
    recording: Option<Recording>,
    segmenter: EarlySegmenter,
    worker: Sender<Job>,
    /// Fed by the monitor; `None` until it loads, or if it can't.
    vad: Option<Vad>,
    preview: Option<Preview>,
    _other_audio: Option<crate::platform::other_audio::Hold>,
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

/// What the recognizer heard in one stretch of audio, before cleanup.
struct Heard {
    text: String,
    language: Option<String>,
}

enum Job {
    Segment(Vec<f32>),
    Finish { stopped: Instant, audio_seconds: f64, rms_dbfs: f32 },
    Sketch { samples: Vec<f32>, speech: Option<SpeechMap>, session: Arc<crate::sketch::Session>, stopped: Instant },
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

#[derive(Default)]
struct Shortcut {
    machine: Activation,
    owner: Option<(Binding, u64)>,
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
    /// The Sketch being recorded, if any.
    sketch: Mutex<Option<Arc<crate::sketch::Session>>>,
    /// Bumped by each outcome, so a stale hold timer can't reset a newer one.
    outcome_seq: AtomicU64,
    watch_generation: AtomicU64,
    starting: std::sync::atomic::AtomicBool,
    shortcut: Mutex<Shortcut>,
    last_paste: Mutex<Option<LastPaste<target::Target>>>,
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
            sketch: Mutex::new(None),
            outcome_seq: AtomicU64::new(0),
            watch_generation: AtomicU64::new(0),
            starting: std::sync::atomic::AtomicBool::new(false),
            shortcut: Mutex::new(Shortcut::default()),
            last_paste: Mutex::new(None),
            last_metrics: Mutex::new(None),
            warm_error: Mutex::new(None),
        })
    }

    pub fn last_text(&self) -> Option<String> {
        self.last_paste.lock().as_ref().map(|l| l.text.clone())
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
}
