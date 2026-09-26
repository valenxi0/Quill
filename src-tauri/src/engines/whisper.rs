//! Whisper (tiny.en, base.en, large-v3-turbo) through whisper.cpp.

use std::path::PathBuf;

use quill_core::text::finish::tidy_whisper;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};

use super::{SpeechEngine, TranscribeRequest, Transcript};

pub struct Whisper {
    path: PathBuf,
    threads: usize,
    multilingual: bool,
    // `state` borrows nothing from `ctx` in whisper-rs, but drop it first anyway.
    state: Option<WhisperState>,
    ctx: Option<WhisperContext>,
    accelerator: String,
}

impl Whisper {
    pub fn new(path: PathBuf, threads: usize, multilingual: bool) -> Self {
        Self { path, threads, multilingual, state: None, ctx: None, accelerator: String::new() }
    }

    fn context(&self, gpu: bool) -> Result<WhisperContext, String> {
        let mut params = WhisperContextParameters::default();
        params.use_gpu(gpu).flash_attn(gpu);
        let path = self.path.to_str().ok_or("The model path is not valid Unicode.")?;
        WhisperContext::new_with_params(path, params).map_err(|e| e.to_string())
    }
}

impl SpeechEngine for Whisper {
    fn load(&mut self) -> Result<(), String> {
        if self.state.is_some() {
            return Ok(());
        }
        // Route whisper.cpp's logging into `log` instead of stderr.
        static HOOKS: std::sync::Once = std::sync::Once::new();
        HOOKS.call_once(whisper_rs::install_logging_hooks);
        if !self.path.exists() {
            return Err(format!("{} is missing. Download the engine again.", self.path.display()));
        }
        // GPU first; CPU if the GPU backend can't start.
        let (ctx, accelerator) = match self.context(cfg!(feature = "vulkan")) {
            Ok(c) if cfg!(feature = "vulkan") => (c, "Vulkan"),
            Ok(c) => (c, "CPU"),
            Err(e) => {
                log::warn!("whisper GPU load failed ({e}); using the CPU");
                (self.context(false)?, "CPU")
            }
        };
        let state = ctx.create_state().map_err(|e| e.to_string())?;
        self.ctx = Some(ctx);
        self.state = Some(state);
        self.accelerator = accelerator.into();
        // One silent pass compiles the GPU pipelines now, not mid-dictation.
        let _ = self.transcribe(&vec![0.0; 17_600], &TranscribeRequest { vocabulary_terms: vec![], language: Some("en".into()) });
        Ok(())
    }

    fn is_loaded(&self) -> bool {
        self.state.is_some()
    }

    fn transcribe(&mut self, samples: &[f32], request: &TranscribeRequest) -> Result<Transcript, String> {
        let state = self.state.as_mut().ok_or("Whisper is not loaded.")?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        let language = request.language.clone().unwrap_or_else(|| "auto".into());
        params.set_language(Some(&language));
        let prompt = (!request.vocabulary_terms.is_empty()).then(|| format!("{}.", request.vocabulary_terms.join(", ")));
        if let Some(p) = &prompt {
            params.set_initial_prompt(p);
        }
        params.set_n_threads(self.threads as i32);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        params.set_no_context(true);
        // Never shrink audio_ctx: turbo outputs garbage with a short context.
        state.full(params, samples).map_err(|e| e.to_string())?;

        let mut text = String::new();
        for i in 0..state.full_n_segments() {
            if let Some(segment) = state.get_segment(i) {
                if let Ok(s) = segment.to_str_lossy() {
                    text.push_str(&s);
                }
            }
        }
        let detected = if self.multilingual && request.language.is_none() {
            whisper_rs::get_lang_str(state.full_lang_id_from_state()).map(str::to_string)
        } else {
            request.language.clone()
        };
        Ok(Transcript { text: tidy_whisper(&text), language: detected, boosted_terms: vec![] })
    }

    fn unload(&mut self) {
        self.state = None;
        self.ctx = None;
    }

    fn accelerator(&self) -> String {
        self.accelerator.clone()
    }
}
