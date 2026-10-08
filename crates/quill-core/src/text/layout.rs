//! Spoken layout cues are consumed before cleanup, alongside snippet protection.

use super::tokens::{normalize, Token};

const BLOCKERS: &[&str] = &[
    "a", "an", "the", "this", "that", "another", "each", "every", "one", "any", "no", "per", "my", "your", "our",
    "their", "its", "his", "her", "whole", "entire", "brand",
];
const FOLLOWERS: &[&str] = &["of", "item", "items", "character", "characters", "break", "breaks"];

pub fn cue_at(tokens: &[Token], i: usize) -> Option<(usize, usize)> {
    let first = &tokens[i];
    if !first.lead.is_empty() || first.core.starts_with('@') {
        return None;
    }
    let (end, breaks) = match normalize(&first.core).as_str() {
        "newline" => (i + 1, 1),
        "new" if first.trail.is_empty() && !first.core.ends_with('@') => {
            let next = tokens.get(i + 1)?;
            if !next.lead.is_empty() || next.core.starts_with('@') {
                return None;
            }
            match normalize(&next.core).as_str() {
                "line" => (i + 2, 1),
                "paragraph" => (i + 2, 2),
                _ => return None,
            }
        }
        _ => return None,
    };
    let last = &tokens[end - 1];
    if last.core.ends_with('@') || !last.trail.chars().all(|c| ".,;:!?".contains(c)) {
        return None;
    }
    let previous_punctuated = i > 0 && tokens[i - 1].text().ends_with(|c: char| !c.is_alphanumeric());
    if i > 0 && !previous_punctuated && BLOCKERS.contains(&normalize(&tokens[i - 1].core).as_str()) {
        return None;
    }
    if last.trail.is_empty() && tokens.get(end).is_some_and(|t| FOLLOWERS.contains(&normalize(&t.core).as_str())) {
        return None;
    }
    if breaks == 1 && i > 0 && !previous_punctuated && end < tokens.len() && last.trail.is_empty() {
        return None;
    }
    Some((end, breaks))
}

pub fn capitalize(text: &str) -> String {
    match text.char_indices().find(|(_, c)| c.is_alphabetic()) {
        Some((i, c)) => format!("{}{}{}", &text[..i], c.to_uppercase(), &text[i + c.len_utf8()..]),
        None => text.to_string(),
    }
}
