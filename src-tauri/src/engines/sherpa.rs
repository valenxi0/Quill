//! Parakeet TDT 0.6B v3 and Cohere Transcribe through sherpa-onnx (ONNX
//! Runtime, CPU).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use quill_core::engines::{bpe_vocab_from_tokens, single_char_pieces, spellable};
use quill_core::text::finish::strip_special_tokens;
use quill_core::text::vocabulary::{hotword_terms, terms_in, MAX_HOTWORDS};
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
    decode_with(rec, samples, language, &[])
}

fn decode_with(rec: &OfflineRecognizer, samples: &[f32], language: Option<&str>, hotwords: &[String]) -> Result<String, String> {
    let stream = if hotwords.is_empty() { rec.create_stream() } else { rec.create_stream_with_hotwords(&hotwords.join("/")) };
    if let Some(lang) = language {
        stream.set_option("language", lang);
    }
    stream.accept_waveform(16_000, samples);
    rec.decode(&stream);
    let result = stream.get_result().ok_or("The recognizer returned no result.")?;
    Ok(result.text.trim().to_string())
}

/// Measured on the hotword set (docs/DEVELOPMENT.md): 3.0 caught a few more
/// terms but capitalized ordinary words that start like one.
const HOTWORDS_SCORE: f32 = 1.5;

pub struct Parakeet {
    dir: PathBuf,
    threads: usize,
    rec: Option<OfflineRecognizer>,
    /// The loaded recognizer runs modified beam search, which hotwords need.
    /// Greedy search is used until a dictation has vocabulary terms.
    beam: bool,
    wants_hotwords: bool,
    hotwords_off: bool,
    pieces: HashSet<char>,
}

impl Parakeet {
    pub fn new(dir: PathBuf, threads: usize) -> Self {
        Self { dir, threads, rec: None, beam: false, wants_hotwords: false, hotwords_off: false, pieces: HashSet::new() }
    }

    /// Writes the hotword vocabulary beside the model; `None` keeps greedy search.
    fn hotword_vocab(&mut self) -> Option<String> {
        let tokens = std::fs::read_to_string(self.dir.join("tokens.txt")).ok()?;
        let path = self.dir.join("bpe.vocab");
        if let Err(e) = std::fs::write(&path, bpe_vocab_from_tokens(&tokens)) {
            log::warn!("parakeet hotwords off: {e}");
            return None;
        }
        self.pieces = single_char_pieces(&tokens);
        Some(path.to_string_lossy().into_owned())
    }
}

impl SpeechEngine for Parakeet {
    fn load(&mut self) -> Result<(), String> {
        if self.is_loaded() {
            return Ok(());
        }
        // Free the old recognizer first so two never sit in memory at once.
        self.rec = None;
        let vocab = if self.wants_hotwords { self.hotword_vocab() } else { None };
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
        self.beam = vocab.is_some();
        if let Some(vocab) = vocab {
            c.decoding_method = Some("modified_beam_search".into());
            c.model_config.modeling_unit = Some("bpe".into());
            c.model_config.bpe_vocab = Some(vocab);
            c.hotwords_score = HOTWORDS_SCORE;
        }
        let rec = OfflineRecognizer::create(&c).ok_or("Parakeet could not start. The model files may be damaged.")?;
        // Warm up so the first dictation doesn't pay for graph initialization.
        let _ = decode(&rec, &vec![0.0; 16_000], None);
        self.rec = Some(rec);
        if self.wants_hotwords && !self.beam {
            // The vocabulary could not be written: stay greedy this session.
            self.hotwords_off = true;
            self.wants_hotwords = false;
        }
        Ok(())
    }

    fn is_loaded(&self) -> bool {
        self.rec.is_some() && (self.beam || !self.wants_hotwords)
    }

    fn want_hotwords(&mut self) {
        self.wants_hotwords = !self.hotwords_off;
    }

    fn transcribe(&mut self, samples: &[f32], request: &TranscribeRequest) -> Result<Transcript, String> {
        // Parakeet detects the language itself and takes no prompt. Terms
        // bias decoding as hotwords and are also applied afterwards.
        let mut hotwords = hotword_terms(&request.vocabulary_terms, MAX_HOTWORDS);
        if !hotwords.is_empty() && !self.beam && !self.hotwords_off {
            // Normally done by the warm-up during recording.
            self.want_hotwords();
            self.load()?;
        }
        hotwords.retain(|t| self.beam && spellable(t, &self.pieces));
        let rec = self.rec.as_ref().ok_or("Parakeet is not loaded.")?;
        let text = decode_with(rec, samples, None, &hotwords)?;
        let boosted_terms = terms_in(&text, &hotwords);
        Ok(Transcript { text, language: None, boosted_terms })
    }

    fn unload(&mut self) {
        self.rec = None;
        self.beam = false;
        self.wants_hotwords = false;
        self.hotwords_off = false;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs the installed Parakeet model and `scripts/make-hotword-set.ps1`.
    #[test]
    #[ignore]
    fn installed_parakeet_switches_to_hotwords_only_with_terms() {
        let dir = crate::paths::models_dir().join(quill_core::engines::AssetId::ParakeetTdt06bV3.key());
        let wav = crate::paths::eval_dir().with_file_name("EvalTerms").join("t03-okonkwo.wav");
        if !dir.join("tokens.txt").exists() || !wav.exists() {
            eprintln!("skipped: model or hotword clips missing");
            return;
        }
        let samples = crate::bench::read_wav(&wav).unwrap();
        let mut p = Parakeet::new(dir, 4);
        p.load().unwrap();
        let plain = p.transcribe(&samples, &TranscribeRequest::default()).unwrap();
        assert!(!p.beam, "greedy without terms");
        assert!(plain.boosted_terms.is_empty());

        let terms = vec!["Okonkwo".to_string(), "AT&T".to_string(), "Grafana".to_string()];
        let request = TranscribeRequest { vocabulary_terms: terms, language: None };
        let heard = p.transcribe(&samples, &request).unwrap();
        assert!(p.beam, "beam search once terms arrive");
        assert!(heard.text.contains("Okonkwo"), "greedy: {:?}, hotwords: {:?}", plain.text, heard.text);
        assert_eq!(heard.boosted_terms, vec!["Okonkwo"]);

        let again = p.transcribe(&samples, &TranscribeRequest::default()).unwrap();
        assert!(p.beam, "stays in beam search until unloaded");
        assert!(again.boosted_terms.is_empty());
        p.unload();
        assert!(!p.beam && !p.is_loaded());
    }
}
