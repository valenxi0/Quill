//! Parakeet TDT 0.6B v3 and Cohere Transcribe through sherpa-onnx (ONNX
//! Runtime, CPU).

use std::path::{Path, PathBuf};

use quill_core::text::finish::strip_special_tokens;
use sherpa_onnx::{OfflineCohereTranscribeModelConfig, OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

use super::{SpeechEngine, TranscribeRequest, Transcript};

fn file(dir: &Path, name: &str) -> Result<Option<String>, String> {
    let p = dir.join(name);
    if !p.exists() {
        return Err(format!("{} is missing. Download the engine again.", p.display()));
    }
    Ok(Some(p.to_string_lossy().into_owned()))
}

fn decode(rec: &OfflineRecognizer, samples: &[f32], language: Option<&str>) -> Result<String, String> {
    let stream = rec.create_stream();
    if let Some(lang) = language {
        stream.set_option("language", lang);
    }
    stream.accept_waveform(16_000, samples);
    rec.decode(&stream);
    let result = stream.get_result().ok_or("The recognizer returned no result.")?;
    Ok(result.text.trim().to_string())
}

pub struct Parakeet {
    dir: PathBuf,
    threads: usize,
    rec: Option<OfflineRecognizer>,
}

impl Parakeet {
    pub fn new(dir: PathBuf, threads: usize) -> Self {
        Self { dir, threads, rec: None }
    }
}

impl SpeechEngine for Parakeet {
    fn load(&mut self) -> Result<(), String> {
        if self.rec.is_some() {
            return Ok(());
        }
        let mut c = OfflineRecognizerConfig::default();
        c.model_config.transducer = OfflineTransducerModelConfig {
            encoder: file(&self.dir, "encoder.int8.onnx")?,
            decoder: file(&self.dir, "decoder.int8.onnx")?,
            joiner: file(&self.dir, "joiner.int8.onnx")?,
        };
        c.model_config.tokens = file(&self.dir, "tokens.txt")?;
        c.model_config.model_type = Some("nemo_transducer".into());
        c.model_config.num_threads = self.threads as i32;
        c.model_config.provider = Some("cpu".into());
        c.decoding_method = Some("greedy_search".into());
        let rec = OfflineRecognizer::create(&c).ok_or("Parakeet could not start. The model files may be damaged.")?;
        // Warm up so the first dictation doesn't pay for graph initialization.
        let _ = decode(&rec, &vec![0.0; 16_000], None);
        self.rec = Some(rec);
        Ok(())
    }

    fn is_loaded(&self) -> bool {
        self.rec.is_some()
    }

    fn transcribe(&mut self, samples: &[f32], _request: &TranscribeRequest) -> Result<Transcript, String> {
        let rec = self.rec.as_ref().ok_or("Parakeet is not loaded.")?;
        // Parakeet detects the language itself and takes no prompt; the
        // dictionary is applied deterministically afterwards.
        Ok(Transcript { text: decode(rec, samples, None)?, language: None, boosted_terms: vec![] })
    }

    fn unload(&mut self) {
        self.rec = None;
    }

    fn accelerator(&self) -> String {
        "CPU (ONNX Runtime)".into()
    }
}

pub struct Cohere {
    dir: PathBuf,
    threads: usize,
    rec: Option<OfflineRecognizer>,
}

impl Cohere {
    pub fn new(dir: PathBuf, threads: usize) -> Self {
        Self { dir, threads, rec: None }
    }
}

impl SpeechEngine for Cohere {
    fn load(&mut self) -> Result<(), String> {
        if self.rec.is_some() {
            return Ok(());
        }
        let mut c = OfflineRecognizerConfig::default();
        c.model_config.cohere_transcribe = OfflineCohereTranscribeModelConfig {
            encoder: file(&self.dir, "encoder.int8.onnx")?,
            decoder: file(&self.dir, "decoder.int8.onnx")?,
            language: Some("en".into()),
            use_punct: true,
            use_itn: true,
        };
        file(&self.dir, "encoder.int8.onnx.data")?;
        c.model_config.tokens = file(&self.dir, "tokens.txt")?;
        c.model_config.num_threads = self.threads as i32;
        c.model_config.provider = Some("cpu".into());
        let rec = OfflineRecognizer::create(&c).ok_or("Cohere Transcribe could not start. The model files may be damaged.")?;
        // Warm with 1 s of silence, twice.
        for _ in 0..2 {
            let _ = decode(&rec, &vec![0.0; 16_000], Some("en"));
        }
        self.rec = Some(rec);
        Ok(())
    }

    fn is_loaded(&self) -> bool {
        self.rec.is_some()
    }

    fn transcribe(&mut self, samples: &[f32], request: &TranscribeRequest) -> Result<Transcript, String> {
        let rec = self.rec.as_ref().ok_or("Cohere Transcribe is not loaded.")?;
        let lang = request.language.clone().unwrap_or_else(|| "en".into());
        let text = strip_special_tokens(&decode(rec, samples, Some(&lang))?);
        Ok(Transcript { text, language: Some(lang), boosted_terms: vec![] })
    }

    fn unload(&mut self) {
        self.rec = None;
    }

    fn accelerator(&self) -> String {
        "CPU (ONNX Runtime)".into()
    }
}
