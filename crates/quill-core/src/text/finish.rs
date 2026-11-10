//! Joining, chat style, hallucination filter, speech gate, and recognizer
//! output tidying (sections 2.2, 4.2).

const CLOSING: &[char] = &['.', ',', ';', ':', '!', '?', ')', ']', '}'];

/// Collapse runs of spaces and tabs; newlines (from snippets) survive.
fn collapse_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for c in s.chars() {
        if c == '\n' || c == '\r' {
            while out.ends_with(' ') {
                out.pop();
            }
            out.push(c);
            pending_space = false;
        } else if c.is_whitespace() {
            pending_space = true;
        } else {
            if pending_space && !out.is_empty() && !out.ends_with('\n') && !CLOSING.contains(&c) {
                out.push(' ');
            }
            pending_space = false;
            out.push(c);
        }
    }
    out
}

fn list_start(text: &str) -> bool {
    if text.starts_with("- ") { return true; }
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0 && (text[digits..].starts_with(". ") || text[digits..].starts_with(") "))
}

/// Join a segment while retaining edge breaks for the final cross-segment join.
pub fn join_segment(pieces: &[String]) -> String {
    let mut out = String::new();
    let mut pending = 0;
    for piece in pieces {
        let p = piece.trim_matches(|c: char| c.is_whitespace() && c != '\n');
        let leading = p.chars().take_while(|c| c.is_whitespace()).filter(|c| *c == '\n').count().min(2);
        pending = pending.max(leading);
        let text = p.trim();
        if text.is_empty() { continue; }
        if pending > 0 {
            out.push_str(&"\n".repeat(pending));
        } else if !out.is_empty() && !text.starts_with(CLOSING) {
            out.push(if list_start(text) { '\n' } else { ' ' });
        }
        out.push_str(text);
        pending = p.chars().rev().take_while(|c| c.is_whitespace()).filter(|c| *c == '\n').count().min(2);
    }
    out.push_str(&"\n".repeat(pending));
    collapse_spaces(&out)
}

/// Final join drops breaks outside the neighbouring text.
pub fn join(pieces: &[String]) -> String {
    join_segment(pieces).trim().to_string()
}

/// Join pieces of one dictation cut apart at the speaker's pauses. A pause
/// is not a sentence end: a piece that stops without one carries on in
/// lowercase, and a full stop before a lowercase piece goes.
pub fn join_seams(pieces: &[String]) -> String {
    let mut out: Vec<String> = Vec::new();
    for piece in pieces {
        let mut text = piece.trim().to_string();
        if text.is_empty() {
            continue;
        }
        if let Some(previous) = out.last_mut() {
            if continues_sentence(previous) {
                text = lowercasing_first_word(&text);
            } else if text.starts_with(char::is_lowercase) && ends_with_plain_full_stop(previous) {
                previous.pop();
            }
        }
        out.push(text);
    }
    join(&out)
}

/// Sounds that are never words. S1-mini's casual style keeps them.
const HESITATIONS: &[&str] = &["um", "umm", "uh", "uhh", "uhm", "erm"];

/// Drop "um" and "uh", with the comma after one, keeping a capital that
/// started the sentence.
pub fn strip_hesitations(text: &str) -> String {
    text.split('\n').map(strip_line).collect::<Vec<_>>().join("\n")
}

fn strip_line(line: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut capitalize = false;
    for word in line.split(' ').filter(|w| !w.is_empty()) {
        let bare = word.trim_end_matches([',', '.', '!', '?']);
        if !HESITATIONS.contains(&bare.to_lowercase().as_str()) {
            let mut word = word.to_string();
            if std::mem::take(&mut capitalize) {
                word = crate::text::layout::capitalize(&word);
            }
            out.push(word);
            continue;
        }
        capitalize |= bare.starts_with(char::is_uppercase);
        // "so, um." keeps the sentence end: "so."
        let mark = word[bare.len()..].chars().find(|c| ".!?".contains(*c));
        if let (Some(last), Some(mark)) = (out.last_mut(), mark) {
            if !last.ends_with(['.', '!', '?']) {
                last.truncate(last.trim_end_matches(',').len());
                last.push(mark);
            }
        }
    }
    let indent = &line[..line.len() - line.trim_start_matches(' ').len()];
    format!("{indent}{}", out.join(" "))
}

/// A full stop that ends a sentence, not an abbreviation ("e.g.", "Dr.") or "...".
fn ends_with_plain_full_stop(text: &str) -> bool {
    let Some(body) = text.strip_suffix('.') else { return false };
    let last = body.rsplit(char::is_whitespace).next().unwrap_or(body);
    !last.is_empty() && !last.contains('.') && !ABBREVIATIONS.contains(&last.to_lowercase().as_str())
}

const ABBREVIATIONS: &[&str] = &["dr", "mr", "mrs", "ms", "st", "vs", "etc", "jr", "sr", "inc", "ltd", "co", "no", "approx"];

/// The text stops mid-sentence, with no closing mark. Quotes and brackets
/// after a mark don't count.
pub fn continues_sentence(text: &str) -> bool {
    let body = text.trim().trim_end_matches(['"', '\'', '”', '’', ')', ']', '}']);
    body.chars().last().is_some_and(|c| !".!?:…。！？".contains(c))
}

/// Words that a pause can't make the start of a new sentence on their own,
/// so they are safe to lowercase. Anything else may be a name.
const CONTINUING: &[&str] = &[
    "a", "an", "the", "and", "but", "or", "nor", "so", "then", "to", "of", "in", "on", "at", "for", "with", "from", "by",
    "as", "that", "this", "these", "those", "it", "its", "it's", "we", "you", "they", "he", "she", "my", "our", "your",
    "their", "his", "her", "if", "when", "because", "which", "who", "what", "where", "how", "is", "are", "was", "were",
    "be", "been", "have", "has", "had", "do", "does", "did", "can", "could", "will", "would", "should", "just", "also",
    "not", "maybe", "there", "here", "about", "into", "over", "after", "before", "like", "since", "until", "while",
    "than", "too", "very", "really", "all", "some", "any", "more", "most", "one",
];

/// "This" becomes "this" after a pause; names, "I", and anything not in
/// `CONTINUING` keep their capital.
pub fn lowercasing_first_word(text: &str) -> String {
    let word: String = text.chars().take_while(|c| !c.is_whitespace() && !",.;:!?".contains(*c)).collect();
    let Some(first) = word.chars().next() else { return text.to_string() };
    if !first.is_uppercase() || word.chars().skip(1).any(char::is_uppercase) || !CONTINUING.contains(&word.to_lowercase().replace('’', "'").as_str()) {
        return text.to_string();
    }
    let rest = &text[first.len_utf8()..];
    format!("{}{rest}", first.to_lowercase())
}

/// In a chat app, a one-sentence, one-line message drops its full stop.
pub fn chat_style(text: &str) -> String {
    let t = text.trim_end();
    if t.contains('\n') || !t.ends_with('.') || t.ends_with("..") {
        return text.to_string();
    }
    let body = &t[..t.len() - 1];
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if matches!(c, '.' | '!' | '?') && chars.peek().is_some_and(|n| n.is_whitespace()) {
            return text.to_string();
        }
    }
    body.to_string()
}

const HALLUCINATIONS: &[&str] = &[
    "you", "thank you", "thanks", "thank you for watching", "thanks for watching", "bye", "the",
    "so", "okay", "hmm", "mm",
];

fn normalize_phrase(s: &str) -> String {
    s.split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_alphanumeric() || *c == '\'').collect::<String>().to_lowercase())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whisper invents these on near-silence.
pub fn is_hallucination(text: &str, rms_dbfs: f32) -> bool {
    rms_dbfs < -40.0 && HALLUCINATIONS.contains(&normalize_phrase(text).as_str())
}

pub const MIN_SPEECH_SECONDS: f32 = 0.25;
pub const MIN_SPEECH_PEAK: f32 = 0.01;

/// True when a recording is long and loud enough to be worth recognizing.
pub fn has_speech(samples: &[f32], sample_rate: u32) -> bool {
    let secs = samples.len() as f32 / sample_rate as f32;
    secs >= MIN_SPEECH_SECONDS && peak(samples) >= MIN_SPEECH_PEAK
}

pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

pub fn rms_dbfs(samples: &[f32]) -> f32 {
    20.0 * rms(samples).max(1e-9).log10()
}

/// Remove `[…]` and `(…)` tags and collapse whitespace.
pub fn tidy_whisper(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth_sq = 0usize;
    let mut depth_paren = 0usize;
    for c in text.chars() {
        match c {
            '[' => depth_sq += 1,
            ']' if depth_sq > 0 => depth_sq -= 1,
            '(' => depth_paren += 1,
            ')' if depth_paren > 0 => depth_paren -= 1,
            _ if depth_sq == 0 && depth_paren == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Strip `<think>…</think>` and `<|im_end|>`, then trim.
pub fn tidy_s1(text: &str) -> String {
    let mut s = text.to_string();
    while let Some(start) = s.find("<think>") {
        match s[start..].find("</think>") {
            Some(end) => s.replace_range(start..start + end + "</think>".len(), ""),
            None => s.truncate(start),
        }
    }
    s.replace("</think>", "").replace("<|im_end|>", "").trim().to_string()
}

/// Strip `<|…|>` special tokens (Cohere).
pub fn strip_special_tokens(text: &str) -> String {
    let mut s = text.to_string();
    while let Some(start) = s.find("<|") {
        match s[start..].find("|>") {
            Some(end) => s.replace_range(start..start + end + 2, ""),
            None => break,
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_spacing() {
        let p = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(join(&p(&["Hello", ", world", "!"])), "Hello, world!");
        assert_eq!(join(&p(&["a  b", "  c "])), "a b c");
        assert_eq!(join(&p(&["Thanks,", "Best,\nJorge", "."])), "Thanks, Best,\nJorge.");
        assert_eq!(join(&p(&["call (", "me", ")"])), "call ( me)");
    }

    #[test]
    fn lowercasing_keeps_names() {
        assert_eq!(lowercasing_first_word("This one"), "this one");
        assert_eq!(lowercasing_first_word("I think"), "I think");
        assert_eq!(lowercasing_first_word("NASA said"), "NASA said");
        assert_eq!(lowercasing_first_word("McDonald"), "McDonald");
        assert_eq!(lowercasing_first_word("I'm here"), "I'm here");
        assert_eq!(lowercasing_first_word("Paris tomorrow"), "Paris tomorrow");
        assert_eq!(lowercasing_first_word("Hund gesehen"), "Hund gesehen");
        assert_eq!(lowercasing_first_word("And then"), "and then");
        assert_eq!(lowercasing_first_word("AND then"), "AND then");
    }

    #[test]
    fn hesitations_go() {
        assert_eq!(strip_hesitations("um we're almost out, uh, so grab some"), "we're almost out, so grab some");
        assert_eq!(strip_hesitations("Um, do not cancel it"), "Do not cancel it");
        assert_eq!(strip_hesitations("ship it, um."), "ship it.");
        assert_eq!(strip_hesitations("the umbrella and the hmm"), "the umbrella and the hmm");
        assert_eq!(strip_hesitations("I think so. Um. Maybe."), "I think so. Maybe.");
        assert_eq!(strip_hesitations("Really? Um."), "Really?");
        assert_eq!(strip_hesitations("Um,  do it"), "Do it");
        assert_eq!(strip_hesitations("list:\nUm, milk"), "list:\nMilk");
        assert_eq!(strip_hesitations("- a\n  - um, b"), "- a\n  - b");
    }

    #[test]
    fn seams_at_pauses_keep_the_sentence() {
        let p = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(join_seams(&p(&["Engineering has started.", "on the settings screen."])), "Engineering has started on the settings screen.");
        assert_eq!(join_seams(&p(&["I think we should", "Then move the launch."])), "I think we should then move the launch.");
        assert_eq!(join_seams(&p(&["So I said", "I'm in, NASA too", ""])), "So I said I'm in, NASA too");
        assert_eq!(join_seams(&p(&["The build passed.", "So we can ship."])), "The build passed. So we can ship.");
        assert_eq!(join_seams(&p(&["Wait...", "okay."])), "Wait... okay.");
        assert_eq!(join_seams(&p(&["Bring fruit, e.g.", "apples."])), "Bring fruit, e.g. apples.");
        assert_eq!(join_seams(&p(&["He said \"stop.\"", "Then we left."])), "He said \"stop.\" Then we left.");
        assert_eq!(join_seams(&p(&["I flew to", "Paris today."])), "I flew to Paris today.");
    }

    #[test]
    fn chat_style_drops_single_full_stop() {
        assert_eq!(chat_style("Sounds good."), "Sounds good");
        assert_eq!(chat_style("Sounds good. See you."), "Sounds good. See you.");
        assert_eq!(chat_style("Wait..."), "Wait...");
        assert_eq!(chat_style("Line one.\nLine two."), "Line one.\nLine two.");
        assert_eq!(chat_style("Really?"), "Really?");
        assert_eq!(chat_style("Version 1.5 is out."), "Version 1.5 is out");
    }

    #[test]
    fn hallucination_filter() {
        assert!(is_hallucination("Thank you.", -55.0));
        assert!(is_hallucination(" you ", -41.0));
        assert!(!is_hallucination("Thank you.", -30.0));
        assert!(!is_hallucination("Thank you so much.", -55.0));
    }

    #[test]
    fn speech_gate() {
        let sr = 16_000;
        assert!(!has_speech(&vec![0.5; 3_000], sr), "too short");
        assert!(!has_speech(&vec![0.005; 16_000], sr), "too quiet");
        assert!(has_speech(&vec![0.02; 16_000], sr));
    }

    #[test]
    fn tidies() {
        assert_eq!(tidy_whisper(" [BLANK_AUDIO] Hello (music)  there "), "Hello there");
        assert_eq!(tidy_s1("<think>\n\n</think>\n\nHello.<|im_end|>"), "Hello.");
        assert_eq!(strip_special_tokens("<|en|><|pnc|> Hi there<|endoftext|>"), "Hi there");
    }
}
