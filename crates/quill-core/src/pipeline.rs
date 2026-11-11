//! Per-segment text processing and finishing (section 4.2, steps 4–5).

use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::context::AppContext;
use crate::errors::DictationError;
use crate::langid::is_english;
use crate::text::directive::Directive;
use crate::text::chunker::{chunk, halve, WORD_BUDGET};
use crate::text::command::format_command;
use crate::text::finish::{chat_style, is_hallucination, join, join_segment, strip_hesitations};
use crate::text::guard::{reason, GuardReason};
use crate::text::insertion::{fit, Surroundings};
use crate::text::numbers::{format_numbers, only_figures, Itn};
use crate::text::spoken::{emoji_names, strip_press_enter};
use crate::text::vocabulary::{apply, protect_layout, EntryKind, Piece, VocabEntry};

/// Rewrites a chunk of English speech (S1-mini), giving up at `deadline`.
pub trait Cleaner {
    fn clean(&mut self, text: &str, directive: Directive, deadline: Instant) -> Result<String, CleanError>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum CleanError {
    TimedOut,
    Failed(String),
}

impl std::fmt::Display for CleanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CleanError::TimedOut => f.write_str("Cleanup timed out."),
            CleanError::Failed(e) => f.write_str(e),
        }
    }
}

/// Cleanup time for a whole dictation of `words` words, retries included.
pub fn cleanup_budget(words: usize) -> Duration {
    CLEANUP_BASE + CLEANUP_PER_WORD * words as u32
}

const CLEANUP_BASE: Duration = Duration::from_secs(20);
const CLEANUP_PER_WORD: Duration = Duration::from_millis(100);

/// Why cleanup kept a chunk as heard.
#[derive(Debug, Clone, PartialEq)]
pub enum KeptReason {
    Guard(GuardReason),
    Cleaner(CleanError),
}

/// How one dictation is processed, fixed at recording start.
#[derive(Debug, Clone)]
pub struct Recipe {
    /// The directive after context overrides; None skips cleanup.
    pub directive: Option<Directive>,
    pub layout_cues: bool,
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
    /// Why each chunk delivered as heard was kept, in order.
    pub kept: Vec<KeptReason>,
    /// Chunks whose cleanup was delivered.
    pub accepted: usize,
    pub hits: Vec<Uuid>,
    /// Whether the cleaner ran on any chunk.
    pub cleaned: bool,
}

impl SegmentResult {
    /// Chunks the cleaner failed on or ran out of time for.
    pub fn fallbacks(&self) -> usize {
        self.kept.iter().filter(|k| matches!(k, KeptReason::Cleaner(_))).count()
    }

    /// Chunks the cleanup guard restored.
    pub fn restored(&self) -> usize {
        self.kept.iter().filter(|k| matches!(k, KeptReason::Guard(_))).count()
    }
}

/// Process one recognized segment. `cleaner` is `None` for engines without
/// cleanup; `itn` is `None` when the number formatter isn't in the build.
pub fn process_segment(
    raw: &str,
    reported_language: Option<&str>,
    recipe: &Recipe,
    cleaner: Option<&mut dyn Cleaner>,
    itn: Option<&dyn Itn>,
) -> SegmentResult {
    let deadline = Instant::now() + cleanup_budget(raw.split_whitespace().count());
    process_segment_until(raw, reported_language, recipe, cleaner, itn, deadline)
}

/// `process_segment`, with chunks still uncleaned at `deadline` delivered as heard.
pub fn process_segment_until(
    raw: &str,
    reported_language: Option<&str>,
    recipe: &Recipe,
    mut cleaner: Option<&mut dyn Cleaner>,
    itn: Option<&dyn Itn>,
    deadline: Instant,
) -> SegmentResult {
    let english = is_english(raw, reported_language);
    let terminal = recipe.context == AppContext::Terminal;
    let cleaner_applies =
        recipe.has_cleanup && recipe.directive.is_some() && english && cleaner.is_some();
    let mut out = SegmentResult { raw: raw.to_string(), english, ..Default::default() };
    let mut pieces = Vec::new();
    let mut capitalize_next = false;

    for piece in protect_layout(raw, &recipe.vocabulary, recipe.layout_cues && !terminal) {
        let speech = match piece {
            Piece::Snippet { text, id } => {
                out.hits.push(id);
                pieces.push(text);
                capitalize_next = false;
                continue;
            }
            Piece::Break(n) => {
                pieces.push("\n".repeat(n));
                capitalize_next = true;
                continue;
            }
            Piece::Speech(s) => s,
        };
        let mut text = if cleaner_applies {
            let cleaner = cleaner.as_deref_mut().expect("checked above");
            let directive = recipe.directive.expect("checked above");
            let mut cleaned = Vec::new();
            for c in chunk(&speech, WORD_BUDGET) {
                if let Some(digits) = itn.filter(|_| recipe.writes_numbers).and_then(|itn| only_figures(&c, terminal, itn)) {
                    cleaned.push(digits);
                    continue;
                }
                cleaned.push(clean_guarded(cleaner, &c, directive, itn, deadline, &mut out, RETRIES));
                out.cleaned = true;
            }
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
        if recipe.layout_cues && !terminal {
            text = emoji_names(&text);
        }
        if capitalize_next && !text.is_empty() {
            text = crate::text::layout::capitalize(&text);
            capitalize_next = false;
        }
        // Vocabulary last, so nothing re-cases a term.
        let applied = apply(&text, &recipe.vocabulary, Some(EntryKind::Vocabulary));
        out.hits.extend(applied.hits);
        pieces.push(applied.text);
    }
    out.text = join_segment(&pieces);
    out
}

/// How many times a rejected chunk is halved and retried, bounding the
/// extra cleaner calls after release.
const RETRIES: u32 = 3;

/// Clean one chunk. When the guard rejects the result, retry each half, so
/// only the sentences it objects to go out raw. Nothing new starts after
/// `deadline`, so retries can't stack up past the dictation's budget.
fn clean_guarded(
    cleaner: &mut dyn Cleaner,
    text: &str,
    directive: Directive,
    itn: Option<&dyn Itn>,
    deadline: Instant,
    out: &mut SegmentResult,
    retries: u32,
) -> String {
    let result = if Instant::now() >= deadline { Err(CleanError::TimedOut) } else { cleaner.clean(text, directive, deadline) };
    let result = match result {
        Ok(r) => r,
        Err(e) => {
            out.kept.push(KeptReason::Cleaner(e));
            return text.to_string();
        }
    };
    let Some(why) = reason(text, &result, itn) else {
        out.accepted += 1;
        return strip_hesitations(&result);
    };
    match halve(text).filter(|_| retries > 0) {
        Some((a, b)) => join(&[
            clean_guarded(cleaner, &a, directive, itn, deadline, out, retries - 1),
            clean_guarded(cleaner, &b, directive, itn, deadline, out, retries - 1),
        ]),
        None => {
            out.kept.push(KeptReason::Guard(why));
            text.to_string()
        }
    }
}

/// A short note for history when cleanup kept any words as heard.
pub fn cleanup_note(results: &[SegmentResult]) -> Option<String> {
    let mut why: Vec<&str> = Vec::new();
    for k in results.iter().flat_map(|r| &r.kept) {
        let text = match k {
            KeptReason::Cleaner(CleanError::TimedOut) => "cleanup took too long",
            KeptReason::Cleaner(CleanError::Failed(_)) => "cleanup failed",
            KeptReason::Guard(GuardReason::EmptyOutput) => "cleanup returned nothing",
            KeptReason::Guard(GuardReason::NegationDropped) => "cleanup dropped a \"not\"",
            KeptReason::Guard(GuardReason::UnspokenFigure(_)) => "cleanup added a number you didn't say",
            KeptReason::Guard(GuardReason::DroppedSentence(_)) => "cleanup dropped a sentence",
            KeptReason::Guard(GuardReason::AddedText) => "cleanup added words you didn't say",
            KeptReason::Guard(GuardReason::DroppedWords(_)) => "cleanup dropped words you said",
            KeptReason::Guard(GuardReason::QuestionAnswered) => "cleanup answered instead of tidying",
        };
        if !why.contains(&text) {
            why.push(text);
        }
    }
    if why.is_empty() {
        return None;
    }
    let some = results.iter().any(|r| r.accepted > 0);
    Some(format!("Kept {} words: {}.", if some { "some of your" } else { "your" }, why.join(", ")))
}

/// Final checks over the whole dictation. Returns the text to deliver.
pub fn finish(raw: &str, text: &str, context: AppContext, rms_dbfs: f32) -> Result<String, DictationError> {
    prepare(raw, text, context, rms_dbfs, &DeliveryOptions::default()).map(|d| d.text)
}

/// What to deliver: the text, and whether to press Enter after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub text: String,
    pub press_enter: bool,
}

/// How the finished text meets the field.
#[derive(Debug, Clone, Default)]
pub struct DeliveryOptions<'a> {
    pub spoken_enter: bool,
    /// The caret's neighbours, when fitting to the cursor is on and they could be read.
    pub around: Option<&'a Surroundings>,
    /// Vocabulary and snippet output whose casing must survive.
    pub protected: &'a [String],
}

/// `finish`, then the spoken "press enter" and fitting to the cursor.
pub fn prepare(raw: &str, text: &str, context: AppContext, rms_dbfs: f32, options: &DeliveryOptions) -> Result<Delivery, DictationError> {
    if raw.trim().is_empty() || text.trim().is_empty() || is_hallucination(raw, rms_dbfs) {
        return Err(DictationError::NoSpeech);
    }
    let (body, press_enter) = match options.spoken_enter.then(|| strip_press_enter(text)).flatten() {
        Some(rest) => (rest, true),
        None => (text.to_string(), false),
    };
    if body.trim().is_empty() {
        return Ok(Delivery { text: String::new(), press_enter });
    }
    let mut out = if context == AppContext::Chat { chat_style(&body) } else { body };
    if let Some(around) = options.around.filter(|_| context != AppContext::Terminal) {
        out = fit(&out, around, options.protected);
    }
    Ok(Delivery { text: out, press_enter })
}

/// Output words of enabled vocabulary and snippets, for `DeliveryOptions::protected`.
pub fn protected_terms(vocabulary: &[VocabEntry]) -> Vec<String> {
    vocabulary.iter().filter(|e| e.is_enabled && !e.replacement.trim().is_empty()).map(|e| e.replacement.clone()).collect()
}

#[cfg(all(test, feature = "itn"))]
mod tests {
    #[test]
    fn app_scoped_snippets_and_terms_flow_through_the_text_pipeline() {
        use crate::text::vocabulary::for_app;
        let mut term = VocabEntry::new(EntryKind::Vocabulary, "or rook", "Oruk");
        term.app_exe = Some("Code.exe".into());
        let mut snippet = VocabEntry::new(EntryKind::Snippet, "my sign off", "Best,\nDeveloper");
        snippet.app_exe = Some("code.exe".into());
        let entries = vec![term, snippet];
        let mut r = recipe(CleanupLevel::Verbatim, AppContext::General);
        r.vocabulary = for_app(&entries, "CODE.EXE");
        assert_eq!(process_segment("or rook my sign off", Some("en"), &r, None, None).text, "Oruk Best,\nDeveloper");
        r.vocabulary = for_app(&entries, "Discord.exe");
        assert_eq!(process_segment("or rook my sign off", Some("en"), &r, None, None).text, "or rook my sign off");
    }
    use super::*;
    use crate::settings::CleanupLevel;
    use crate::text::numbers::NemoItn;

    /// Removes "um" and capitalizes; fails on "boom"; drops "not" on "drop".
    struct Fake {
        calls: Vec<(String, Directive)>,
    }

    impl Cleaner for Fake {
        fn clean(&mut self, text: &str, directive: Directive, _: Instant) -> Result<String, CleanError> {
            self.calls.push((text.to_string(), directive));
            if text.contains("boom") {
                return Err(CleanError::Failed("failed".into()));
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
        Recipe { directive: Directive::for_recording(level, context, false), layout_cues: true, context, writes_numbers: true, has_cleanup: true, vocabulary: vec![] }
    }

    #[test]
    fn cleaner_sees_speech_only_and_vocabulary_runs_last() {
        let snippet = VocabEntry::new(EntryKind::Snippet, "my sign off", "Best,\num Jorge");
        let vocab = VocabEntry::new(EntryKind::Vocabulary, "mac island", "Nano");
        let mut r = recipe(CleanupLevel::Full, AppContext::General);
        r.vocabulary = vec![snippet.clone(), vocab.clone()];
        let mut fake = Fake { calls: vec![] };
        let out = process_segment("um try mac island today. my sign off", None, &r, Some(&mut fake), Some(&NemoItn));
        assert_eq!(fake.calls, vec![("um try mac island today.".to_string(), Directive::default())]);
        assert_eq!(out.text, "Try Nano today. Best,\num Jorge");
        assert_eq!(out.hits, vec![vocab.id, snippet.id]);
        assert!(out.cleaned);
    }

    #[test]
    fn guard_and_fallback() {
        let r = recipe(CleanupLevel::Light, AppContext::General);
        let mut fake = Fake { calls: vec![] };
        let out = process_segment("please do not drop it", None, &r, Some(&mut fake), None);
        assert_eq!((out.text.as_str(), out.restored()), ("please do not drop it", 1));
        let out = process_segment("boom goes it", None, &r, Some(&mut fake), None);
        assert_eq!((out.text.as_str(), out.fallbacks()), ("boom goes it", 1));
        assert_eq!(out.kept, vec![KeptReason::Cleaner(CleanError::Failed("failed".into()))]);
        assert_eq!(cleanup_note(&[out]).as_deref(), Some("Kept your words: cleanup failed."));
    }

    #[test]
    fn past_the_deadline_nothing_is_cleaned() {
        let r = recipe(CleanupLevel::Full, AppContext::General);
        let mut fake = Fake { calls: vec![] };
        let out = process_segment_until("um ship it today", None, &r, Some(&mut fake), None, Instant::now());
        assert!(fake.calls.is_empty());
        assert_eq!((out.text.as_str(), out.fallbacks()), ("um ship it today", 1));
        assert_eq!(out.kept, vec![KeptReason::Cleaner(CleanError::TimedOut)]);
        assert_eq!(cleanup_note(&[out]).as_deref(), Some("Kept your words: cleanup took too long."));
    }

    struct Slow(usize);

    impl Cleaner for Slow {
        fn clean(&mut self, text: &str, _: Directive, _: Instant) -> Result<String, CleanError> {
            self.0 += 1;
            std::thread::sleep(std::time::Duration::from_millis(50));
            Ok(text.replace(" not", ""))
        }
    }

    #[test]
    fn retries_stop_at_the_deadline() {
        let r = recipe(CleanupLevel::Full, AppContext::General);
        let raw = "Do not ship it. Do not call them. Do not move it. Do not stop.";
        let mut slow = Slow(0);
        let out = process_segment_until(raw, None, &r, Some(&mut slow), None, Instant::now() + Duration::from_millis(20));
        assert_eq!(slow.0, 1, "the halves start after the deadline");
        assert_eq!(out.text, raw);
        assert_eq!(out.kept, vec![KeptReason::Cleaner(CleanError::TimedOut); 2]);
        let mut slow = Slow(0);
        let out = process_segment(raw, None, &r, Some(&mut slow), None);
        assert_eq!(slow.0, 7, "within the budget every half is retried");
        assert_eq!(out.restored(), 4);
    }

    #[test]
    fn a_cleaner_timeout_is_a_timeout() {
        struct Late;
        impl Cleaner for Late {
            fn clean(&mut self, _: &str, _: Directive, _: Instant) -> Result<String, CleanError> {
                Err(CleanError::TimedOut)
            }
        }
        let r = recipe(CleanupLevel::Full, AppContext::General);
        let out = process_segment("ship it today", None, &r, Some(&mut Late), None);
        assert_eq!(out.kept, vec![KeptReason::Cleaner(CleanError::TimedOut)]);
    }

    #[test]
    fn notes_say_why_words_were_kept() {
        let r = recipe(CleanupLevel::Full, AppContext::General);
        let mut fake = Fake { calls: vec![] };
        let out = process_segment("um ship it today. um do not drop the table. um call me after.", None, &r, Some(&mut fake), None);
        assert_eq!(out.kept, vec![KeptReason::Guard(GuardReason::NegationDropped)]);
        assert_eq!(cleanup_note(&[out]).as_deref(), Some("Kept some of your words: cleanup dropped a \"not\"."));
        let clean = process_segment("um ship it today", None, &r, Some(&mut fake), None);
        assert_eq!(cleanup_note(&[clean]), None);
        assert_eq!(cleanup_note(&[]), None);
    }

    #[test]
    fn numbers_alone_skip_the_cleaner() {
        let r = recipe(CleanupLevel::Full, AppContext::General);
        let mut fake = Fake { calls: vec![] };
        for (raw, expected) in [
            ("twenty five.", "25."),
            ("twenty five", "25"),
            ("Three hundred and forty-two", "342"),
            ("three point one four", "3.14"),
            ("5,000", "5,000"),
        ] {
            let out = process_segment(raw, None, &r, Some(&mut fake), Some(&NemoItn));
            assert_eq!(out.text, expected, "{raw}");
            assert!(!out.cleaned, "{raw}");
        }
        assert!(fake.calls.is_empty());
        for words in ["one, two, three", "twenty five people"] {
            process_segment(words, None, &r, Some(&mut fake), Some(&NemoItn));
            assert_eq!(fake.calls.last().map(|c| c.0.as_str()), Some(words), "words left after formatting go to the cleaner");
        }
        let mut off = r.clone();
        off.writes_numbers = false;
        process_segment("twenty five.", None, &off, Some(&mut fake), Some(&NemoItn));
        assert_eq!(fake.calls.len(), 3, "with number formatting off, S1-mini still writes the figures");
    }

    #[test]
    fn guard_restores_only_the_sentence_it_rejects() {
        let r = recipe(CleanupLevel::Full, AppContext::General);
        let mut fake = Fake { calls: vec![] };
        let out = process_segment("um ship it today. um do not drop the table. um call me after.", None, &r, Some(&mut fake), None);
        assert_eq!(out.restored(), 1);
        assert!(out.text.starts_with("Ship it today."), "{}", out.text);
        assert!(out.text.contains("um do not drop the table."), "{}", out.text);
        assert!(out.text.ends_with("Call me after."), "{}", out.text);
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
    fn emoji_names_follow_layout_cues() {
        let r = recipe(CleanupLevel::Verbatim, AppContext::General);
        assert_eq!(process_segment("nice work fire emoji", None, &r, None, None).text, "nice work 🔥");
        let mut off = r.clone();
        off.layout_cues = false;
        assert_eq!(process_segment("nice work fire emoji", None, &off, None, None).text, "nice work fire emoji");
        let t = recipe(CleanupLevel::Verbatim, AppContext::Terminal);
        assert!(!process_segment("fire emoji", None, &t, None, None).text.contains('🔥'));
    }

    #[test]
    fn preparing_delivery() {
        let plain = DeliveryOptions::default();
        assert_eq!(prepare("x", "Run it, press enter.", AppContext::General, -20.0, &plain).unwrap().text, "Run it, press enter.");
        let enter = DeliveryOptions { spoken_enter: true, ..Default::default() };
        assert_eq!(
            prepare("x", "Run it, press enter.", AppContext::Terminal, -20.0, &enter).unwrap(),
            Delivery { text: "Run it".into(), press_enter: true }
        );
        assert_eq!(prepare("x", "Press enter.", AppContext::General, -20.0, &enter).unwrap(), Delivery { text: String::new(), press_enter: true });
        let around = Surroundings { before: "We could ".into(), after: String::new(), selection: String::new() };
        let fitted = DeliveryOptions { around: Some(&around), ..Default::default() };
        assert_eq!(prepare("x", "Ship it today.", AppContext::General, -20.0, &fitted).unwrap().text, "ship it today.");
        assert_eq!(prepare("x", "Ship it today.", AppContext::Chat, -20.0, &fitted).unwrap().text, "ship it today");
        assert_eq!(prepare("x", "Ship it today.", AppContext::Terminal, -20.0, &fitted).unwrap().text, "Ship it today.");
        assert_eq!(prepare("", "", AppContext::General, -20.0, &plain), Err(DictationError::NoSpeech));
    }

    #[test]
    fn finishing() {
        assert_eq!(finish("", "", AppContext::General, -20.0), Err(DictationError::NoSpeech));
        assert_eq!(finish("Thank you.", "Thank you.", AppContext::General, -60.0), Err(DictationError::NoSpeech));
        assert_eq!(finish("Thank you.", "Thank you.", AppContext::General, -20.0).unwrap(), "Thank you.");
        assert_eq!(finish("see you soon.", "See you soon.", AppContext::Chat, -20.0).unwrap(), "See you soon");
    }
}
