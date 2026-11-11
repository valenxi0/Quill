//! Parakeet TDT 0.6B v3 and Cohere Transcribe through sherpa-onnx (ONNX
//! Runtime, CPU).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;

use quill_core::engines::{ascii_path, bpe_vocab_from_tokens, single_char_pieces, spellable};
use quill_core::text::finish::strip_special_tokens;
use quill_core::text::vocabulary::{hotword_terms, MAX_HOTWORDS};
use sherpa_onnx::{OfflineCohereTranscribeModelConfig, OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

use super::{Hotwords, SpeechEngine, TranscribeRequest, Transcript};

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
pub(crate) const HOTWORDS_SCORE: f32 = 1.5;

#[cfg(windows)]
fn short_path(p: &Path) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetShortPathNameW;
    let wide: Vec<u16> = p.as_os_str().encode_wide().chain([0]).collect();
    let needed = unsafe { GetShortPathNameW(PCWSTR(wide.as_ptr()), None) };
    if needed == 0 {
        return None;
    }
    let mut buf = vec![0u16; needed as usize];
    let len = unsafe { GetShortPathNameW(PCWSTR(wide.as_ptr()), Some(&mut buf)) } as usize;
    (len > 0 && len < buf.len()).then(|| String::from_utf16(&buf[..len]).ok()).flatten()
}

#[cfg(not(windows))]
fn short_path(_: &Path) -> Option<String> {
    None
}

/// Writes the hotword vocabulary beside Parakeet's `tokens.txt` and returns
/// a path sherpa-onnx can open, with the characters the model can spell.
/// `None` when either fails; Parakeet then stays on greedy search.
pub(crate) fn hotword_vocab(dir: &Path) -> Option<(String, HashSet<char>)> {
    let tokens = std::fs::read_to_string(dir.join("tokens.txt")).ok()?;
    let path = dir.join("bpe.vocab");
    if let Err(e) = std::fs::write(&path, bpe_vocab_from_tokens(&tokens)) {
        log::warn!("parakeet hotwords off: {e}");
        return None;
    }
    let Some(usable) = ascii_path(&path.to_string_lossy(), short_path(&path).as_deref()) else {
        log::warn!("parakeet hotwords off: no ASCII path to {}", path.display());
        return None;
    };
    Some((usable, single_char_pieces(&tokens)))
}

/// Parakeet's recognizer settings: greedy search, or with `hotword_vocab`'s
/// path, modified beam search ready for hotwords.
pub(crate) fn parakeet_config(dir: &Path, threads: usize, hotword_vocab: Option<String>) -> Result<OfflineRecognizerConfig, String> {
    let mut c = OfflineRecognizerConfig::default();
    c.model_config.transducer = OfflineTransducerModelConfig {
        encoder: file(dir, "encoder.int8.onnx")?,
        decoder: file(dir, "decoder.int8.onnx")?,
        joiner: file(dir, "joiner.int8.onnx")?,
    };
    c.model_config.tokens = file(dir, "tokens.txt")?;
    c.model_config.model_type = Some("nemo_transducer".into());
    c.model_config.num_threads = threads as i32;
    c.model_config.provider = Some("cpu".into());
    c.decoding_method = Some("greedy_search".into());
    if let Some(vocab) = hotword_vocab {
        c.decoding_method = Some("modified_beam_search".into());
        c.model_config.modeling_unit = Some("bpe".into());
        c.model_config.bpe_vocab = Some(vocab);
        c.hotwords_score = HOTWORDS_SCORE;
    }
    Ok(c)
}

/// How the loaded Parakeet recognizer decodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decoding {
    Greedy,
    /// Modified beam search with each dictation's terms as hotwords.
    Hotwords,
    /// Hotwords were turned on but could not start: greedy until unloaded.
    Unavailable,
}

pub struct Parakeet {
    dir: PathBuf,
    threads: usize,
    rec: Option<OfflineRecognizer>,
    decoding: Decoding,
    hotwords: Hotwords,
    pieces: HashSet<char>,
}

impl Parakeet {
    pub fn new(dir: PathBuf, threads: usize, hotwords: Hotwords) -> Self {
        Self { dir, threads, rec: None, decoding: Decoding::Greedy, hotwords, pieces: HashSet::new() }
    }

    fn target(&self) -> Decoding {
        match (self.hotwords.on.load(Relaxed), self.decoding) {
            // Already greedy, so turning the switch off needs no rebuild.
            (_, Decoding::Unavailable) => Decoding::Unavailable,
            (false, _) => Decoding::Greedy,
            (true, _) => Decoding::Hotwords,
        }
    }

    fn create_hotwords(&mut self) -> Result<Option<OfflineRecognizer>, String> {
        let Some((vocab, pieces)) = hotword_vocab(&self.dir) else { return Ok(None) };
        let rec = OfflineRecognizer::create(&parakeet_config(&self.dir, self.threads, Some(vocab))?);
        if rec.is_none() {
            log::warn!("parakeet hotwords off: beam search did not start");
        }
        self.pieces = pieces;
        Ok(rec)
    }
}

impl SpeechEngine for Parakeet {
    fn load(&mut self) -> Result<(), String> {
        if self.is_loaded() {
            return Ok(());
        }
        // Free the old recognizer first so two never sit in memory at once.
        self.rec = None;
        let mut decoding = self.target();
        let mut rec = None;
        if decoding == Decoding::Hotwords {
            rec = self.create_hotwords()?;
            if rec.is_none() {
                decoding = Decoding::Unavailable;
            }
            self.hotwords.unavailable.store(rec.is_none(), Relaxed);
        }
        let rec = match rec {
            Some(rec) => rec,
            None => OfflineRecognizer::create(&parakeet_config(&self.dir, self.threads, None)?)
                .ok_or("Parakeet could not start. The model files may be damaged.")?,
        };
        // Warm up so the first dictation doesn't pay for graph initialization.
        let _ = decode(&rec, &vec![0.0; 16_000], None);
        self.rec = Some(rec);
        self.decoding = decoding;
        Ok(())
    }

    fn is_loaded(&self) -> bool {
        self.rec.is_some() && self.decoding == self.target()
    }

    fn transcribe(&mut self, samples: &[f32], request: &TranscribeRequest) -> Result<Transcript, String> {
        // Parakeet detects the language itself and takes no prompt. With the
        // switch on, terms bias decoding as hotwords; they are always applied
        // to the transcript afterwards.
        let hotwords = match self.decoding {
            Decoding::Hotwords => hotword_terms(&request.vocabulary_terms, |t| spellable(t, &self.pieces), MAX_HOTWORDS),
            _ => Vec::new(),
        };
        let rec = self.rec.as_ref().ok_or("Parakeet is not loaded.")?;
        Ok(Transcript { text: decode_with(rec, samples, None, &hotwords)?, language: None })
    }

    fn unload(&mut self) {
        self.rec = None;
        self.decoding = Decoding::Greedy;
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
        Ok(Transcript { text, language: Some(lang) })
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

    #[test]
    fn decoding_follows_the_switch_but_unavailable_stays_put() {
        let hotwords = Hotwords::default();
        let mut p = Parakeet::new(PathBuf::from("missing"), 1, hotwords.clone());
        assert_eq!(p.target(), Decoding::Greedy);
        hotwords.on.store(true, Relaxed);
        assert_eq!(p.target(), Decoding::Hotwords);
        p.decoding = Decoding::Unavailable;
        assert_eq!(p.target(), Decoding::Unavailable);
        hotwords.on.store(false, Relaxed);
        assert_eq!(p.target(), Decoding::Unavailable, "no rebuild when switching off");
        p.unload();
        assert_eq!(p.target(), Decoding::Greedy);
    }

    /// Needs the installed Parakeet model and `scripts/make-hotword-set.ps1`.
    #[test]
    #[ignore]
    fn installed_parakeet_uses_hotwords_only_when_switched_on() {
        let dir = crate::paths::models_dir().join(quill_core::engines::AssetId::ParakeetTdt06bV3.key());
        let wav = crate::paths::eval_dir().with_file_name("EvalTerms").join("t03-okonkwo.wav");
        if !dir.join("tokens.txt").exists() || !wav.exists() {
            eprintln!("skipped: model or hotword clips missing");
            return;
        }
        let samples = crate::bench::read_wav(&wav).unwrap();
        let hotwords = Hotwords::default();
        let switch = hotwords.on.clone();
        let mut p = Parakeet::new(dir, 4, hotwords.clone());
        let request = TranscribeRequest { vocabulary_terms: vec!["AT&T".into(), "Okonkwo".into(), "Grafana".into()], language: None };

        p.load().unwrap();
        let off = p.transcribe(&samples, &request).unwrap();
        assert_eq!(p.decoding, Decoding::Greedy, "terms alone never start beam search");
        assert!(!off.text.contains("Okonkwo"), "greedy already gets it: {:?}", off.text);

        switch.store(true, Relaxed);
        assert!(!p.is_loaded(), "switching on asks for a reload");
        p.load().unwrap();
        assert_eq!(p.decoding, Decoding::Hotwords);
        assert!(!hotwords.unavailable.load(Relaxed));
        let on = p.transcribe(&samples, &request).unwrap();
        assert!(on.text.contains("Okonkwo"), "greedy: {:?}, hotwords: {:?}", off.text, on.text);

        switch.store(false, Relaxed);
        assert!(!p.is_loaded(), "switching off asks for a reload");
        p.load().unwrap();
        assert_eq!(p.decoding, Decoding::Greedy);
        p.unload();
        assert!(!p.is_loaded());
    }
}
