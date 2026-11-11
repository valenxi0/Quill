use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use quill_core::context::AppContext;
use quill_core::engines::Recognizer;
use quill_core::errors::DictationError;
use quill_core::pipeline::{cleanup_note, prepare, process_segment, CleanError, Cleaner, DeliveryOptions, Recipe, SegmentResult};
use quill_core::segments::{pad, split};
use quill_core::text::directive::Directive;
use quill_core::settings::CleanupLevel;
use quill_core::state::Phase;
use quill_core::text::finish::join_seams;
use quill_core::text::vocabulary::for_app;

use crate::engines::{EngineHost, TranscribeRequest};

use super::{Controller, Heard, Job, Metrics, SessionConfig};

impl Controller {
    /// One worker per session processes segments one at a time, in order.
    pub(super) fn spawn_worker(self: &Arc<Self>, cfg: SessionConfig) -> Sender<Job> {
        let (tx, rx) = channel::<Job>();
        let me = self.clone();
        std::thread::Builder::new()
            .name("quill-dictation".into())
            .spawn(move || {
                let mut heard: Vec<Heard> = Vec::new();
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
                            match me.recognize(&cfg, &samples, &mut metrics) {
                                Ok(h) => {
                                    if let Some(c) = &cfg.preview_committed {
                                        c.lock().push(h.text.clone());
                                    }
                                    heard.push(h)
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
                            let result = me.clean(&cfg, &heard, &mut metrics);
                            me.deliver(&cfg, result, metrics, stopped, rms_dbfs);
                            return;
                        }
                        Job::Sketch { samples, speech, session, stopped } => {
                            me.deliver_sketch(&cfg, &samples, speech.as_ref(), &session, stopped);
                            return;
                        }
                    }
                }
            })
            .expect("spawn dictation worker");
        tx
    }

    /// Recognize one stretch of audio. Long dictations are recognized in
    /// pieces while they are spoken, cut at pauses.
    pub(super) fn recognize(self: &Arc<Self>, cfg: &SessionConfig, samples: &[f32], metrics: &mut Metrics) -> Result<Heard, DictationError> {
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
                    Ok(crate::engines::Transcript { text: join_seams(&texts), language })
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
        Ok(Heard { text: transcript.text, language: transcript.language })
    }

    /// Clean what was heard as one text, so a pause the audio was cut at
    /// never ends a sentence the speaker meant to continue.
    pub(super) fn clean(self: &Arc<Self>, cfg: &SessionConfig, heard: &[Heard], metrics: &mut Metrics) -> SegmentResult {
        let info = cfg.engine.info();
        let raw = join_seams(&heard.iter().map(|h| h.text.clone()).collect::<Vec<_>>());
        // One language for the whole text only when every piece agrees;
        // otherwise the text itself decides.
        let mut languages = heard.iter().map(|h| h.language.as_deref());
        let first = languages.next().flatten();
        let language = first.filter(|l| languages.all(|o| o == Some(*l)));
        if info.has_cleanup && cfg.recipe.directive.is_some() && quill_core::langid::is_english(&raw, language) {
            if let Ok(((), Some(load))) = self.engines.with_cleaner(|_| ()) {
                metrics.cold_start = true;
                metrics.model_load_ms = Some(metrics.model_load_ms.unwrap_or(0) + load.as_millis() as u64);
            }
        }
        let post_start = Instant::now();
        let mut cleaner = HostCleaner { controller: self, generation: cfg.generation, time: Duration::ZERO, cold: None };
        let result = process_segment(
            &raw,
            language,
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
        metrics.cleanup_fallbacks += result.fallbacks();
        metrics.guard_restores += result.restored();
        result
    }

    /// Run a history entry's raw transcript through cleanup again at
    /// `level`. Uses S1-mini when it is installed, whatever the engine.
    pub fn redo(self: &Arc<Self>, raw: &str, level: CleanupLevel, app_exe: &str, context: AppContext) -> Result<(String, Option<String>), String> {
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
        if has_cleanup && recipe.directive.is_some() && quill_core::langid::is_english(raw, None) {
            let _ = self.engines.with_cleaner(|_| ());
        }
        let mut cleaner = EngineCleaner { engines: &self.engines };
        let cleaner = has_cleanup.then_some(&mut cleaner as &mut dyn Cleaner);
        let result = process_segment(raw, None, &recipe, cleaner, self.itn.as_deref());
        let text = prepare(raw, &result.text, context, 0.0, &DeliveryOptions::default()).map_err(|e| e.to_string())?.text;
        let cleanup_note = cleanup_note(std::slice::from_ref(&result));
        self.schedule_idle_unload();
        Ok((text, cleanup_note))
    }
}

/// S1-mini for a redo, outside any dictation.
struct EngineCleaner<'a> {
    engines: &'a EngineHost,
}

impl Cleaner for EngineCleaner<'_> {
    fn clean(&mut self, text: &str, directive: Directive, deadline: Instant) -> Result<String, CleanError> {
        self.engines.with_cleaner(|c| c.clean(text, directive, deadline)).map_err(CleanError::Failed).and_then(|(r, _)| r)
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
    fn clean(&mut self, text: &str, directive: Directive, deadline: Instant) -> Result<String, CleanError> {
        if !self.controller.is_current(self.generation) {
            return Err(CleanError::Failed("cancelled".into()));
        }
        self.controller.advance(self.generation, Phase::Transcribing, Phase::Cleaning);
        let start = Instant::now();
        let result = self.controller.engines.with_cleaner(|c| c.clean(text, directive, deadline));
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
                Err(CleanError::Failed(e))
            }
        }
    }
}
