//! Per-segment text processing and finishing (section 4.2, steps 4–5).

use uuid::Uuid;

use crate::context::AppContext;
use crate::errors::DictationError;
use crate::langid::is_english;
use crate::settings::CleanupLevel;
use crate::text::chunker::{chunk, WORD_BUDGET};
use crate::text::command::format_command;
use crate::text::finish::{chat_style, is_hallucination, join};
use crate::text::guard::guard;
use crate::text::numbers::{format_numbers, Itn};
use crate::text::vocabulary::{apply, protect_snippets, EntryKind, Piece, VocabEntry};

/// Rewrites a chunk of English speech (S1-mini).
pub trait Cleaner {
    fn clean(&mut self, text: &str, level: CleanupLevel) -> Result<String, String>;
}

/// How one dictation is processed, fixed at recording start.
#[derive(Debug, Clone)]
pub struct Recipe {
    /// The cleanup level after context overrides.
    pub level: CleanupLevel,
    pub context: AppContext,
    pub writes_numbers: bool,
    /// Whether the engine has a cleanup stage at all.
    pub has_cleanup: bool,
    pub vocabulary: Vec<VocabEntry>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SegmentResult {
    pub raw: String,
    pub text: String,
    pub english: bool,
    /// Chunks the cleaner failed on, delivered raw.
    pub fallbacks: usize,
    /// Chunks the cleanup guard restored.
    pub restored: usize,
    pub hits: Vec<Uuid>,
    /// Whether the cleaner ran on any chunk.
    pub cleaned: bool,
}

/// Process one recognized segment. `cleaner` is `None` for engines without
/// cleanup; `itn` is `None` when the number formatter isn't in the build.
pub fn process_segment(
    raw: &str,
    reported_language: Option<&str>,
    recipe: &Recipe,
    mut cleaner: Option<&mut dyn Cleaner>,
    itn: Option<&dyn Itn>,
) -> SegmentResult {
    let english = is_english(raw, reported_language);
    let terminal = recipe.context == AppContext::Terminal;
    let cleaner_applies =
        recipe.has_cleanup && recipe.level != CleanupLevel::Verbatim && english && cleaner.is_some();
    let mut out = SegmentResult { raw: raw.to_string(), english, ..Default::default() };
    let mut pieces = Vec::new();

    for piece in protect_snippets(raw, &recipe.vocabulary) {
        let speech = match piece {
            Piece::Snippet { text, id } => {
                out.hits.push(id);
                pieces.push(text);
                continue;
            }
            Piece::Speech(s) => s,
        };
        let mut text = if cleaner_applies {
            let cleaner = cleaner.as_deref_mut().expect("checked above");
            let mut cleaned = Vec::new();
            for c in chunk(&speech, WORD_BUDGET) {
                match cleaner.clean(&c, recipe.level) {
                    Ok(result) => {
                        let (kept, restored) = guard(&c, &result);
                        out.restored += usize::from(restored);
                        cleaned.push(kept.to_string());
                    }
                    Err(_) => {
                        out.fallbacks += 1;
                        cleaned.push(c);
                    }
                }
            }
            out.cleaned = true;
            join(&cleaned)
        } else {
            speech
        };
        if terminal {
            text = format_command(&text);
        }
        if !cleaner_applies && recipe.writes_numbers && english {
            if let Some(itn) = itn {
                text = format_numbers(&text, terminal, itn);
            }
        }
        // Vocabulary last, so nothing re-cases a term.
        let applied = apply(&text, &recipe.vocabulary, Some(EntryKind::Vocabulary));
        out.hits.extend(applied.hits);
        pieces.push(applied.text);
    }
    out.text = join(&pieces);
    out
}

/// Final checks over the whole dictation. Returns the text to deliver.
pub fn finish(raw: &str, text: &str, context: AppContext, rms_dbfs: f32) -> Result<String, DictationError> {
    if raw.trim().is_empty() || text.trim().is_empty() || is_hallucination(raw, rms_dbfs) {
        return Err(DictationError::NoSpeech);
    }
    Ok(if context == AppContext::Chat { chat_style(text) } else { text.to_string() })
}

#[cfg(all(test, feature = "itn"))]
mod tests {
    use super::*;
    use crate::text::numbers::NemoItn;

    /// Removes "um" and capitalizes; fails on "boom"; drops "not" on "drop".
    struct Fake {
        calls: Vec<(String, CleanupLevel)>,
    }

    impl Cleaner for Fake {
        fn clean(&mut self, text: &str, level: CleanupLevel) -> Result<String, String> {
            self.calls.push((text.to_string(), level));
            if text.contains("boom") {
                return Err("failed".into());
            }
            let mut t = text.replace("um ", "").replace(" um", "");
            if t.contains("drop") {
                t = t.replace(" not", "");
            }
            let mut c = t.chars();
            Ok(c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default())
        }
    }

    fn recipe(level: CleanupLevel, context: AppContext) -> Recipe {
        Recipe { level, context, writes_numbers: true, has_cleanup: true, vocabulary: vec![] }
    }

    #[test]
    fn cleaner_sees_speech_only_and_vocabulary_runs_last() {
        let snippet = VocabEntry::new(EntryKind::Snippet, "my sign off", "Best,\num Jorge");
        let vocab = VocabEntry::new(EntryKind::Vocabulary, "mac island", "Nano");
        let mut r = recipe(CleanupLevel::Full, AppContext::General);
        r.vocabulary = vec![snippet.clone(), vocab.clone()];
        let mut fake = Fake { calls: vec![] };
        let out = process_segment("um try mac island today. my sign off", None, &r, Some(&mut fake), Some(&NemoItn));
        assert_eq!(fake.calls, vec![("um try mac island today.".to_string(), CleanupLevel::Full)]);
        assert_eq!(out.text, "Try Nano today. Best,\num Jorge");
        assert_eq!(out.hits, vec![vocab.id, snippet.id]);
        assert!(out.cleaned);
    }

    #[test]
    fn guard_and_fallback() {
        let r = recipe(CleanupLevel::Light, AppContext::General);
        let mut fake = Fake { calls: vec![] };
        let out = process_segment("please do not drop it", None, &r, Some(&mut fake), None);
        assert_eq!((out.text.as_str(), out.restored), ("please do not drop it", 1));
        let out = process_segment("boom goes it", None, &r, Some(&mut fake), None);
        assert_eq!((out.text.as_str(), out.fallbacks), ("boom goes it", 1));
    }

    #[test]
    fn numbers_only_without_cleaner() {
        let mut fake = Fake { calls: vec![] };
        let full = recipe(CleanupLevel::Full, AppContext::General);
        let out = process_segment("twenty five people", None, &full, Some(&mut fake), Some(&NemoItn));
        assert_eq!(out.text, "Twenty five people", "S1 handles numbers itself");
        let verbatim = recipe(CleanupLevel::Verbatim, AppContext::General);
        let out = process_segment("twenty five people", None, &verbatim, Some(&mut fake), Some(&NemoItn));
        assert_eq!(out.text, "25 people");
        let mut off = verbatim.clone();
        off.writes_numbers = false;
        assert_eq!(process_segment("twenty five", None, &off, None, Some(&NemoItn)).text, "twenty five");
    }

    #[test]
    fn non_english_skips_english_stages() {
        let mut fake = Fake { calls: vec![] };
        let r = recipe(CleanupLevel::Full, AppContext::General);
        let out = process_segment("um zwanzig Leute", Some("de"), &r, Some(&mut fake), Some(&NemoItn));
        assert!(fake.calls.is_empty());
        assert_eq!(out.text, "um zwanzig Leute");
        assert!(!out.english);
    }

    #[test]
    fn terminal_formats_commands_and_digits() {
        let r = recipe(CleanupLevel::Verbatim, AppContext::Terminal);
        let out = process_segment("Head dash n fifty readme dot md.", None, &r, None, Some(&NemoItn));
        assert_eq!(out.text, "head -n 50 readme.md");
    }

    #[test]
    fn finishing() {
        assert_eq!(finish("", "", AppContext::General, -20.0), Err(DictationError::NoSpeech));
        assert_eq!(finish("Thank you.", "Thank you.", AppContext::General, -60.0), Err(DictationError::NoSpeech));
        assert_eq!(finish("Thank you.", "Thank you.", AppContext::General, -20.0).unwrap(), "Thank you.");
        assert_eq!(finish("see you soon.", "See you soon.", AppContext::Chat, -20.0).unwrap(), "See you soon");
    }
}
