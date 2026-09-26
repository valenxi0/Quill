//! Engines behind two small traits (section 2.2). The controller only sees
//! `SpeechEngine` and `TextCleaner`; adding an engine is a data change.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
#[allow(unused_imports)]
use quill_core::engines::{AssetId, EngineId, Recognizer};
use quill_core::settings::CleanupLevel;

use crate::models::ModelManager;

#[cfg(feature = "whisper")]
mod whisper;
#[cfg(feature = "sherpa")]
mod sherpa;
#[cfg(feature = "llama")]
mod s1;

#[derive(Debug, Clone, Default)]
pub struct TranscribeRequest {
    pub vocabulary_terms: Vec<String>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Transcript {
    pub text: String,
    pub language: Option<String>,
    pub boosted_terms: Vec<String>,
}

pub trait SpeechEngine: Send {
    /// Idempotent; the cold path.
    fn load(&mut self) -> Result<(), String>;
    fn is_loaded(&self) -> bool;
    fn transcribe(&mut self, samples: &[f32], request: &TranscribeRequest) -> Result<Transcript, String>;
    fn unload(&mut self);
    /// "Vulkan", "CPU", "DirectML"…
    fn accelerator(&self) -> String;
}

pub trait TextCleaner: Send {
    fn load(&mut self) -> Result<(), String>;
    fn is_loaded(&self) -> bool;
    fn clean(&mut self, text: &str, level: CleanupLevel) -> Result<String, String>;
    fn unload(&mut self);
}

/// Unload engines after this long without a dictation.
pub const IDLE_UNLOAD: Duration = Duration::from_secs(10 * 60);

type Shared<T> = Arc<Mutex<Option<Box<T>>>>;

/// Owns engine instances. Each sits behind its own mutex, so a warm-up and a
/// dictation that arrive together share one load instead of loading twice.
pub struct EngineHost {
    models: Arc<ModelManager>,
    pub threads: usize,
    recognizers: Mutex<HashMap<EngineId, Shared<dyn SpeechEngine>>>,
    cleaner: Shared<dyn TextCleaner>,
    last_used: Mutex<Instant>,
    /// Engines whose recognizer is loaded, and ones loading right now.
    loaded: Mutex<HashSet<EngineId>>,
    loading: Mutex<HashSet<EngineId>>,
}

impl EngineHost {
    pub fn new(models: Arc<ModelManager>, threads: usize) -> Arc<Self> {
        Arc::new(Self {
            models,
            threads,
            recognizers: Mutex::new(HashMap::new()),
            cleaner: Arc::new(Mutex::new(None)),
            last_used: Mutex::new(Instant::now()),
            loaded: Mutex::new(HashSet::new()),
            loading: Mutex::new(HashSet::new()),
        })
    }

    /// Whether this build can run the engine at all.
    pub fn supports(engine: EngineId) -> bool {
        match engine.info().recognizer {
            Recognizer::Whisper => cfg!(feature = "whisper"),
            Recognizer::Parakeet => cfg!(feature = "sherpa") && cfg!(feature = "llama"),
            Recognizer::Cohere => cfg!(feature = "sherpa"),
        }
    }

    fn create(&self, engine: EngineId) -> Result<Box<dyn SpeechEngine>, String> {
        let m = &self.models;
        let _ = (m, self.threads);
        match engine {
            #[cfg(feature = "whisper")]
            EngineId::WhisperTiny => Ok(Box::new(whisper::Whisper::new(m.file_path(AssetId::WhisperTinyEnglish, "ggml-tiny.en.bin"), self.threads, false))),
            #[cfg(feature = "whisper")]
            EngineId::WhisperBase => Ok(Box::new(whisper::Whisper::new(m.file_path(AssetId::WhisperBaseEnglish, "ggml-base.en.bin"), self.threads, false))),
            #[cfg(feature = "whisper")]
            EngineId::WhisperTurbo => Ok(Box::new(whisper::Whisper::new(
                m.file_path(AssetId::WhisperLargeV3Turbo, "ggml-large-v3-turbo-q5_0.bin"),
                self.threads,
                true,
            ))),
            #[cfg(feature = "sherpa")]
            EngineId::ParakeetS1 => Ok(Box::new(sherpa::Parakeet::new(m.dir(AssetId::ParakeetTdt06bV3), self.threads))),
            #[cfg(feature = "sherpa")]
            EngineId::CohereTranscribe => Ok(Box::new(sherpa::Cohere::new(m.dir(AssetId::CohereTranscribe), self.threads))),
            #[allow(unreachable_patterns)]
            _ => Err(format!("{} is not included in this build.", engine.info().title)),
        }
    }

    fn slot(&self, engine: EngineId) -> Shared<dyn SpeechEngine> {
        self.recognizers.lock().entry(engine).or_insert_with(|| Arc::new(Mutex::new(None))).clone()
    }

    /// Run `f` with the engine loaded. Returns the result and the load time
    /// if this call did a cold load.
    pub fn with_recognizer<R>(
        &self,
        engine: EngineId,
        f: impl FnOnce(&mut dyn SpeechEngine) -> R,
    ) -> Result<(R, Option<Duration>), String> {
        let slot = self.slot(engine);
        let mut guard = slot.lock();
        if guard.is_none() {
            *guard = Some(self.create(engine)?);
        }
        let e = guard.as_mut().expect("just created");
        let mut cold = None;
        if !e.is_loaded() {
            self.loading.lock().insert(engine);
            let start = Instant::now();
            let result = e.load();
            self.loading.lock().remove(&engine);
            result?;
            cold = Some(start.elapsed());
            self.loaded.lock().insert(engine);
        }
        *self.last_used.lock() = Instant::now();
        Ok((f(e.as_mut()), cold))
    }

    pub fn recognizer_loaded(&self, engine: EngineId) -> bool {
        self.loaded.lock().contains(&engine)
    }

    pub fn is_loading(&self, engine: EngineId) -> bool {
        self.loading.lock().contains(&engine)
    }

    pub fn accelerator(&self, engine: EngineId) -> Option<String> {
        self.slot(engine).try_lock().and_then(|g| g.as_ref().map(|e| e.accelerator()))
    }

    fn create_cleaner(&self) -> Result<Box<dyn TextCleaner>, String> {
        #[cfg(feature = "llama")]
        {
            Ok(Box::new(s1::S1Mini::new(self.models.file_path(AssetId::S1Mini, "s1-mini-q4_k_m.gguf"), self.threads)))
        }
        #[cfg(not(feature = "llama"))]
        {
            Err("S1-mini is not included in this build.".into())
        }
    }

    pub fn with_cleaner<R>(&self, f: impl FnOnce(&mut dyn TextCleaner) -> R) -> Result<(R, Option<Duration>), String> {
        let mut guard = self.cleaner.lock();
        if guard.is_none() {
            *guard = Some(self.create_cleaner()?);
        }
        let c = guard.as_mut().expect("just created");
        let mut cold = None;
        if !c.is_loaded() {
            let start = Instant::now();
            c.load()?;
            cold = Some(start.elapsed());
        }
        *self.last_used.lock() = Instant::now();
        Ok((f(c.as_mut()), cold))
    }

    /// Load everything `engine` needs.
    pub fn warm(&self, engine: EngineId) -> Result<(), String> {
        self.with_recognizer(engine, |_| ())?;
        if engine.info().has_cleanup {
            self.with_cleaner(|_| ())?;
        }
        Ok(())
    }

    /// Unload everything if idle long enough. Returns whether it did.
    pub fn unload_if_idle(&self) -> bool {
        if self.last_used.lock().elapsed() < IDLE_UNLOAD {
            return false;
        }
        self.unload_all();
        true
    }

    pub fn unload_all(&self) {
        for (engine, slot) in self.recognizers.lock().iter() {
            if let Some(mut g) = slot.try_lock() {
                if let Some(e) = g.as_mut() {
                    e.unload();
                }
                *g = None;
                self.loaded.lock().remove(engine);
            }
        }
        if let Some(mut g) = self.cleaner.try_lock() {
            if let Some(c) = g.as_mut() {
                c.unload();
            }
            *g = None;
        }
    }

    /// Drop one engine (after its model is removed or updated).
    pub fn evict(&self, engine: EngineId) {
        if let Some(slot) = self.recognizers.lock().remove(&engine) {
            // An in-flight dictation keeps its own handle and finishes first.
            if let Some(mut g) = slot.try_lock() {
                if let Some(e) = g.as_mut() {
                    e.unload();
                }
            }
        }
        self.loaded.lock().remove(&engine);
    }

    pub fn evict_cleaner(&self) {
        if let Some(mut g) = self.cleaner.try_lock() {
            if let Some(c) = g.as_mut() {
                c.unload();
            }
            *g = None;
        }
    }
}
