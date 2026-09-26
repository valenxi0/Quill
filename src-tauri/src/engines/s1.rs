//! S1-mini by Superwhisper through llama.cpp (section 2.2).
//!
//! The system turn is decoded once and kept in the KV cache; each cleanup
//! drops everything after it and decodes only the user turn.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::OnceLock;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use quill_core::settings::CleanupLevel;
use quill_core::text::finish::tidy_s1;

use super::TextCleaner;

const N_CTX: u32 = 4096;
const N_BATCH: u32 = 2048;
const MAX_OUTPUT: usize = 1024;

const SYSTEM: &str = "<|im_start|>system\nYou are a text normalizer for speech-to-text transcripts. The input begins with a control line specifying the styling, structure, and context settings; clean the transcript to match those settings and output only the cleaned text.<|im_end|>\n";

/// The user turn. Only the trained control values are valid.
pub fn user_turn(styling: &str, transcript: &str) -> String {
    format!(
        "<|im_start|>user\n[Styling: {styling}] [Structure: prose] [Context: general]\n{transcript}<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n"
    )
}

/// The backend can only be initialized once per process.
fn backend() -> Result<&'static LlamaBackend, String> {
    static BACKEND: OnceLock<Result<LlamaBackend, String>> = OnceLock::new();
    BACKEND
        .get_or_init(|| {
            let mut b = LlamaBackend::init().map_err(|e| e.to_string())?;
            b.void_logs();
            Ok(b)
        })
        .as_ref()
        .map_err(Clone::clone)
}

struct Loaded {
    // Field order matters: the context borrows the model and drops first.
    ctx: LlamaContext<'static>,
    model: Box<LlamaModel>,
    prefix_len: usize,
}

pub struct S1Mini {
    path: PathBuf,
    threads: usize,
    loaded: Option<Loaded>,
}

// SAFETY: a llama.cpp context may move between threads as long as only one
// thread uses it at a time. `EngineHost` keeps the cleaner behind a mutex.
unsafe impl Send for S1Mini {}

impl S1Mini {
    pub fn new(path: PathBuf, threads: usize) -> Self {
        Self { path, threads, loaded: None }
    }

    fn load_model(&self, gpu: bool) -> Result<Box<LlamaModel>, String> {
        let params = LlamaModelParams::default().with_n_gpu_layers(if gpu { 999 } else { 0 });
        LlamaModel::load_from_file(backend()?, &self.path, &params).map(Box::new).map_err(|e| e.to_string())
    }
}

fn decode_tokens(ctx: &mut LlamaContext, tokens: &[llama_cpp_2::token::LlamaToken], start: usize) -> Result<(), String> {
    let mut batch = LlamaBatch::new(N_BATCH as usize, 1);
    for chunk_start in (0..tokens.len()).step_by(N_BATCH as usize) {
        batch.clear();
        let chunk = &tokens[chunk_start..(chunk_start + N_BATCH as usize).min(tokens.len())];
        for (i, t) in chunk.iter().enumerate() {
            let pos = (start + chunk_start + i) as i32;
            let last = chunk_start + i == tokens.len() - 1;
            batch.add(*t, pos, &[0], last).map_err(|e| e.to_string())?;
        }
        ctx.decode(&mut batch).map_err(|e| e.to_string())?;
    }
    Ok(())
}

impl TextCleaner for S1Mini {
    fn load(&mut self) -> Result<(), String> {
        if self.loaded.is_some() {
            return Ok(());
        }
        if !self.path.exists() {
            return Err(format!("{} is missing. Download the engine again.", self.path.display()));
        }
        let gpu_wanted = cfg!(feature = "vulkan");
        let model = match self.load_model(gpu_wanted) {
            Ok(m) => m,
            Err(e) if gpu_wanted => {
                log::warn!("S1-mini GPU load failed ({e}); using the CPU");
                self.load_model(false)?
            }
            Err(e) => return Err(e),
        };
        let params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(N_CTX))
            .with_n_batch(N_BATCH)
            .with_n_threads(self.threads as i32)
            .with_n_threads_batch(self.threads as i32);
        // SAFETY: the model is boxed (stable address) and outlives the
        // context: `Loaded` drops `ctx` before `model`.
        let model_ref: &'static LlamaModel = unsafe { &*(model.as_ref() as *const LlamaModel) };
        let mut ctx = model_ref.new_context(backend()?, params).map_err(|e| e.to_string())?;
        // Prime the prefix.
        let prefix = model.str_to_token(SYSTEM, AddBos::Never).map_err(|e| e.to_string())?;
        decode_tokens(&mut ctx, &prefix, 0)?;
        self.loaded = Some(Loaded { ctx, model, prefix_len: prefix.len() });
        // Warm the generation path once.
        let _ = self.clean("okay so this is a test", CleanupLevel::Full);
        Ok(())
    }

    fn is_loaded(&self) -> bool {
        self.loaded.is_some()
    }

    fn clean(&mut self, text: &str, level: CleanupLevel) -> Result<String, String> {
        let Some(styling) = level.styling() else { return Ok(text.to_string()) };
        let l = self.loaded.as_mut().ok_or("S1-mini is not loaded.")?;
        let model: &LlamaModel = &l.model;
        l.ctx.clear_kv_cache_seq(Some(0), Some(l.prefix_len as u32), None).map_err(|e| e.to_string())?;

        let budget = {
            let n = model.str_to_token(text, AddBos::Never).map_err(|e| e.to_string())?.len();
            ((n as f64 * 1.3 + 32.0) as usize).min(MAX_OUTPUT)
        };
        let user = model.str_to_token(&user_turn(styling, text), AddBos::Never).map_err(|e| e.to_string())?;
        if l.prefix_len + user.len() + budget > N_CTX as usize {
            return Err("The text is too long for S1-mini.".into());
        }
        decode_tokens(&mut l.ctx, &user, l.prefix_len)?;

        let mut sampler = LlamaSampler::greedy();
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut out = String::new();
        let mut pos = l.prefix_len + user.len();
        let mut batch = LlamaBatch::new(1, 1);
        // Logits of the prompt's last token, in the last decoded batch.
        let mut logits_index = ((user.len() - 1) % N_BATCH as usize) as i32;
        for _ in 0..budget {
            let token = sampler.sample(&l.ctx, logits_index);
            sampler.accept(token);
            if model.is_eog_token(token) {
                break;
            }
            out.push_str(&model.token_to_piece(token, &mut decoder, true, None).map_err(|e| e.to_string())?);
            if out.contains("<|im_end|>") {
                break;
            }
            batch.clear();
            batch.add(token, pos as i32, &[0], true).map_err(|e| e.to_string())?;
            l.ctx.decode(&mut batch).map_err(|e| e.to_string())?;
            pos += 1;
            logits_index = 0;
        }
        Ok(tidy_s1(&out))
    }

    fn unload(&mut self) {
        self.loaded = None;
    }
}
