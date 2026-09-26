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

/// Join pieces with single spaces, never before closing punctuation.
pub fn join(pieces: &[String]) -> String {
    let mut out = String::new();
    for p in pieces {
        let p = p.trim_matches(|c: char| c == ' ' || c == '\t');
        if p.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(p);
    }
    collapse_spaces(&out).trim().to_string()
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
