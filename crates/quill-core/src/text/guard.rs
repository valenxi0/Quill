//! Cleanup guard (section 4.5): less polished beats a changed meaning.

const NEGATIONS: &[&str] = &["not", "never", "cannot", "neither", "nor", "none", "nothing", "nobody", "nowhere"];

const BARE_CONTRACTIONS: &[&str] = &[
    "dont", "doesnt", "didnt", "cant", "couldnt", "wont", "wouldnt", "shouldnt", "isnt", "arent", "wasnt", "werent",
    "hasnt", "havent", "hadnt", "mustnt", "neednt", "aint",
];

/// Negations in `text`. Bare "no" is a self-correction marker and never counts.
pub fn negation_count(text: &str) -> usize {
    text.split_whitespace()
        .map(|w| {
            w.replace(['\u{2019}', '\u{2018}'], "'")
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
                .trim_matches('\'')
                .to_lowercase()
        })
        .filter(|w| NEGATIONS.contains(&w.as_str()) || w.ends_with("n't") || BARE_CONTRACTIONS.contains(&w.as_str()))
        .count()
}

use super::numbers::{format_numbers, Itn};
use regex::Regex;
use std::{collections::HashSet, sync::LazyLock};

static THOUSANDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([0-9]),([0-9]{3})\b").unwrap());
static DIGITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[0-9]+").unwrap());
static NUMBERING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*\d+[.)]\s").unwrap());
static SENTENCES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[.!?]\s+").unwrap());

const NUMBER_WORDS: &[(&str, &str)] = &[
    ("zero", "0"),
    ("oh", "0"),
    ("one", "1"),
    ("two", "2"),
    ("three", "3"),
    ("four", "4"),
    ("five", "5"),
    ("six", "6"),
    ("seven", "7"),
    ("eight", "8"),
    ("nine", "9"),
    ("ten", "10"),
    ("eleven", "11"),
    ("twelve", "12"),
    ("thirteen", "13"),
    ("fourteen", "14"),
    ("fifteen", "15"),
    ("sixteen", "16"),
    ("seventeen", "17"),
    ("eighteen", "18"),
    ("nineteen", "19"),
    ("first", "1"),
    ("second", "2"),
    ("third", "3"),
    ("fourth", "4"),
    ("fifth", "5"),
    ("sixth", "6"),
    ("seventh", "7"),
    ("eighth", "8"),
    ("ninth", "9"),
    ("tenth", "10"),
    ("eleventh", "11"),
    ("twelfth", "12"),
    ("thirteenth", "13"),
    ("fourteenth", "14"),
    ("fifteenth", "15"),
    ("sixteenth", "16"),
    ("seventeenth", "17"),
    ("eighteenth", "18"),
    ("nineteenth", "19"),
    ("twentieth", "20"),
    ("thirtieth", "30"),
    ("twenty", "20"),
    ("thirty", "30"),
    ("forty", "40"),
    ("fifty", "50"),
    ("sixty", "60"),
    ("seventy", "70"),
    ("eighty", "80"),
    ("ninety", "90"),
    ("hundred", "100"),
    ("thousand", "1000"),
    ("million", "1000000"),
    ("billion", "1000000000"),
    ("half", "5"),
    ("quarter", "25"),
];
const FILLERS: &str = "like know just really actually basically literally kind sort mean well okay yeah right that this with have what when then they them there here were been will would could should about from into your their which because think going gonna wanna thing things some very also still wait sorry scratch dollars dollar percent cents point";

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()).collect()
}

pub fn figures(text: &str) -> Vec<String> {
    // Repeat for adjacent groups such as 1,234,567.
    let mut text = text.to_string();
    loop {
        let next = THOUSANDS.replace_all(&text, "$1$2").into_owned();
        if next == text {
            break;
        }
        text = next;
    }
    DIGITS.find_iter(&text).map(|m| m.as_str().to_string()).collect()
}

pub fn unspoken_figure(raw: &str, cleaned: &str, itn: Option<&dyn Itn>) -> Option<String> {
    let words = words(raw);
    let mut sources = vec![figures(raw)];
    if let Some(itn) = itn {
        sources.push(figures(&format_numbers(raw, true, itn)));
    }
    sources.push(
        words
            .iter()
            .filter_map(|w| NUMBER_WORDS.iter().find(|(word, _)| word == w).map(|(_, n)| n.to_string()))
            .collect(),
    );
    let mut known: HashSet<String> = sources.iter().flatten().cloned().collect();
    if words.iter().any(|w| ["hundred", "thousand", "million", "billion"].contains(&w.as_str())) {
        known.insert("1".into());
    }
    let concatenated: Vec<String> = sources.iter().map(|s| s.concat()).collect();
    figures(&NUMBERING.replace_all(cleaned, ""))
        .into_iter()
        .find(|n| !known.contains(n) && !(n.len() >= 3 && concatenated.iter().any(|source| source.contains(n))))
}

pub fn dropped_sentence(raw: &str, cleaned: &str) -> Option<String> {
    let squashed: String = cleaned.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect();
    SENTENCES
        .split(raw)
        .find(|sentence| {
            let content: HashSet<String> = words(sentence)
                .into_iter()
                .filter(|w| {
                    w.chars().count() >= 4
                        && w.chars().all(char::is_alphabetic)
                        && !NUMBER_WORDS.iter().any(|(word, _)| word == w)
                        && !NEGATIONS.contains(&w.as_str())
                        && !BARE_CONTRACTIONS.contains(&w.as_str())
                        && !FILLERS.split_whitespace().any(|f| f == w)
                })
                .collect();
            if content.len() < 5 {
                return false;
            }
            let kept = content
                .iter()
                .filter(|w| {
                    squashed.contains(w.as_str())
                        || (w.chars().count() > 6 && squashed.contains(&w.chars().take(5).collect::<String>()))
                })
                .count();
            kept * 4 < content.len()
        })
        .map(str::to_string)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardReason {
    EmptyOutput,
    NegationDropped,
    UnspokenFigure(String),
    DroppedSentence(String),
}

pub fn reason(raw: &str, cleaned: &str, itn: Option<&dyn Itn>) -> Option<GuardReason> {
    if cleaned.trim().is_empty() && raw.split_whitespace().count() >= 12 {
        return Some(GuardReason::EmptyOutput);
    }
    if negation_count(cleaned) < negation_count(raw) {
        return Some(GuardReason::NegationDropped);
    }
    if let Some(figure) = unspoken_figure(raw, cleaned, itn) {
        return Some(GuardReason::UnspokenFigure(figure));
    }
    dropped_sentence(raw, cleaned).map(GuardReason::DroppedSentence)
}

/// Returns the text to keep for one chunk, and whether the raw text was restored.
pub fn guard_with_itn<'a>(raw: &'a str, cleaned: &'a str, itn: Option<&dyn Itn>) -> (&'a str, bool) {
    if reason(raw, cleaned, itn).is_some() {
        (raw, true)
    } else {
        (cleaned, false)
    }
}

pub fn guard<'a>(raw: &'a str, cleaned: &'a str) -> (&'a str, bool) {
    guard_with_itn(raw, cleaned, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negation_dropped_restores_raw() {
        let raw = "I do not want the meeting moved.";
        assert_eq!(guard(raw, "I want the meeting moved."), (raw, true));
    }

    #[test]
    fn contraction_wash_is_fine() {
        assert_eq!(guard("I do not know", "I don't know"), ("I don't know", false));
        assert_eq!(guard("I don\u{2019}t know", "I do not know"), ("I do not know", false));
        assert_eq!(guard("it isn't ready", "It is not ready."), ("It is not ready.", false));
    }

    #[test]
    fn apostrophe_less_forms_count() {
        assert_eq!(negation_count("i dont know and it wont work"), 2);
        let raw = "i dont know";
        assert_eq!(guard(raw, "I know."), (raw, true));
    }

    #[test]
    fn empty_output() {
        let long = "so um this is a long rambling sentence that has at least twelve words";
        assert_eq!(guard(long, "  "), (long, true));
        assert_eq!(guard("um uh", ""), ("", false));
    }

    #[test]
    fn bare_no_ignored() {
        assert_eq!(negation_count("no wait, Tuesday"), 0);
        assert_eq!(guard("Monday, no, Tuesday", "Tuesday"), ("Tuesday", false));
    }
}
