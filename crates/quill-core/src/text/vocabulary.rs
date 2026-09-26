//! Vocabulary entries and snippets: matching, replacement, protection, and
//! list import (section 4.8).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::tokens::{detokenize, normalize, tokenize, Token};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    Vocabulary,
    Snippet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VocabEntry {
    pub id: Uuid,
    pub phrase: String,
    pub replacement: String,
    pub kind: EntryKind,
    #[serde(default = "yes")]
    pub is_enabled: bool,
}

fn yes() -> bool {
    true
}

impl VocabEntry {
    pub fn new(kind: EntryKind, phrase: impl Into<String>, replacement: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            phrase: phrase.into(),
            replacement: replacement.into(),
            kind,
            is_enabled: true,
        }
    }

    /// Valid: the phrase has something to match and the replacement is non-empty.
    pub fn is_valid(&self) -> bool {
        !normalize(&self.phrase).is_empty() && !self.replacement.trim().is_empty()
    }

    pub fn is_active(&self) -> bool {
        self.is_enabled && self.is_valid()
    }

    fn key(&self) -> String {
        normalize(&self.phrase)
    }

    fn phrase_words(&self) -> usize {
        self.phrase.split_whitespace().count().max(1)
    }
}

/// Terms that can be offered to a recognizer as vocabulary hints.
pub fn vocabulary_terms(entries: &[VocabEntry]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    entries
        .iter()
        .filter(|e| e.is_active() && e.kind == EntryKind::Vocabulary)
        .map(|e| e.replacement.trim().to_string())
        .filter(|t| seen.insert(t.to_lowercase()))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Match {
    start: usize,
    end: usize,
    entry: usize,
}

/// Non-overlapping matches, scanning left to right; at each position the
/// longest window wins.
fn find_matches(tokens: &[Token], entries: &[&VocabEntry]) -> Vec<Match> {
    let keys: Vec<String> = entries.iter().map(|e| e.key()).collect();
    let norms: Vec<String> = tokens.iter().map(|t| normalize(&t.core)).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let mut best: Option<Match> = None;
        for (ei, entry) in entries.iter().enumerate() {
            let max_window = entry.phrase_words() + 2;
            let mut joined = String::new();
            for len in 1..=max_window {
                let j = i + len;
                if j > tokens.len() {
                    break;
                }
                joined.push_str(&norms[j - 1]);
                if joined.len() > keys[ei].len() {
                    break;
                }
                // A window must start and end on a real word.
                if joined == keys[ei] && !norms[i].is_empty() && !norms[j - 1].is_empty() {
                    let better = best.map_or(true, |b| len > b.end - b.start);
                    if better {
                        best = Some(Match { start: i, end: j, entry: ei });
                    }
                    break;
                }
            }
        }
        match best {
            Some(m) => {
                out.push(m);
                i = m.end;
            }
            None => i += 1,
        }
    }
    out
}

/// Result of applying vocabulary entries to text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applied {
    pub text: String,
    pub hits: Vec<Uuid>,
}

/// Replace every matched phrase, keeping the first token's leading and the
/// last token's trailing punctuation around the replacement.
pub fn apply(text: &str, entries: &[VocabEntry], kind: Option<EntryKind>) -> Applied {
    let active: Vec<&VocabEntry> = entries
        .iter()
        .filter(|e| e.is_active() && kind.map_or(true, |k| e.kind == k))
        .collect();
    if active.is_empty() {
        return Applied { text: text.to_string(), hits: vec![] };
    }
    let tokens = tokenize(text);
    let matches = find_matches(&tokens, &active);
    if matches.is_empty() {
        return Applied { text: text.to_string(), hits: vec![] };
    }
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut hits = Vec::new();
    let mut i = 0;
    for m in matches {
        out.extend_from_slice(&tokens[i..m.start]);
        let entry = active[m.entry];
        out.push(Token {
            sep: tokens[m.start].sep.clone(),
            lead: tokens[m.start].lead.clone(),
            core: entry.replacement.clone(),
            trail: tokens[m.end - 1].trail.clone(),
        });
        hits.push(entry.id);
        i = m.end;
    }
    out.extend_from_slice(&tokens[i..]);
    Applied { text: detokenize(&out), hits }
}

/// A piece of a transcript after snippet protection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    /// Speech the cleaner may see.
    Speech(String),
    /// A snippet expansion, spliced back verbatim.
    Snippet { text: String, id: Uuid },
}

/// Split a transcript at snippet trigger phrases so the cleaner only ever
/// sees speech. Each expansion keeps the trigger's surrounding punctuation.
pub fn protect_snippets(text: &str, entries: &[VocabEntry]) -> Vec<Piece> {
    let active: Vec<&VocabEntry> = entries
        .iter()
        .filter(|e| e.is_active() && e.kind == EntryKind::Snippet)
        .collect();
    if active.is_empty() {
        return vec![Piece::Speech(text.to_string())];
    }
    let tokens = tokenize(text);
    let matches = find_matches(&tokens, &active);
    if matches.is_empty() {
        return vec![Piece::Speech(text.to_string())];
    }
    let mut pieces = Vec::new();
    let mut i = 0;
    for m in matches {
        if m.start > i {
            pieces.push(Piece::Speech(detokenize(&tokens[i..m.start]).trim().to_string()));
        }
        let entry = active[m.entry];
        let first = &tokens[m.start];
        let last = &tokens[m.end - 1];
        pieces.push(Piece::Snippet {
            text: format!("{}{}{}", first.lead, entry.replacement, last.trail),
            id: entry.id,
        });
        i = m.end;
    }
    if i < tokens.len() {
        pieces.push(Piece::Speech(detokenize(&tokens[i..]).trim().to_string()));
    }
    pieces
}

/// Parse the "Paste List" sheet. One term per line or comma-separated;
/// `spoken → written` (or `->`, `=>`, tab, `=`) sets how a phrase is written,
/// and a bare term is written exactly as typed. Duplicates, including ones
/// already in `existing`, are skipped by normalized phrase.
pub fn parse_list(input: &str, existing: &[VocabEntry]) -> Vec<VocabEntry> {
    let mut seen: std::collections::HashSet<String> =
        existing.iter().map(|e| e.key()).collect();
    let mut out = Vec::new();
    for line in input.lines() {
        let items: Vec<&str> = if find_arrow(line).is_some() {
            vec![line]
        } else {
            line.split(',').collect()
        };
        for item in items {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            let (phrase, replacement) = match find_arrow(item) {
                Some((at, len)) => (item[..at].trim(), item[at + len..].trim()),
                None => (item, item),
            };
            let entry = VocabEntry::new(EntryKind::Vocabulary, phrase, replacement);
            if !entry.is_valid() || !seen.insert(entry.key()) {
                continue;
            }
            out.push(entry);
        }
    }
    out
}

/// The first arrow in a line: byte offset and length.
fn find_arrow(s: &str) -> Option<(usize, usize)> {
    for arrow in ["→", "->", "=>", "\t", "="] {
        if let Some(at) = s.find(arrow) {
            return Some((at, arrow.len()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocab(p: &str, r: &str) -> VocabEntry {
        VocabEntry::new(EntryKind::Vocabulary, p, r)
    }

    #[test]
    fn compound_matching() {
        let e = [vocab("Mac Island", "Nano")];
        for input in ["open mac island now", "open Mac-Island now", "open MacIsland now"] {
            assert_eq!(apply(input, &e, None).text, "open Nano now", "{input}");
        }
    }

    #[test]
    fn no_match_inside_longer_word() {
        let e = [vocab("nano", "Nano")];
        assert_eq!(apply("one nanosecond", &e, None).text, "one nanosecond");
        assert_eq!(apply("use nano.", &e, None).text, "use Nano.");
    }

    #[test]
    fn keeps_punctuation() {
        let e = [vocab("mac island", "Nano")];
        let r = apply("I like (mac island), really.", &e, None);
        assert_eq!(r.text, "I like (Nano), really.");
        assert_eq!(r.hits, vec![e[0].id]);
    }

    #[test]
    fn longest_match_wins() {
        let e = [vocab("super", "Super"), vocab("super whisper", "Superwhisper")];
        assert_eq!(apply("try super whisper", &e, None).text, "try Superwhisper");
    }

    #[test]
    fn disabled_and_invalid_entries_are_ignored() {
        let mut off = vocab("nano", "Nano");
        off.is_enabled = false;
        let blank = vocab("  ", "X");
        let empty = vocab("foo", " ");
        assert_eq!(apply("nano foo", &[off, blank, empty], None).text, "nano foo");
    }

    #[test]
    fn snippet_protection() {
        let s = VocabEntry::new(EntryKind::Snippet, "my sign off", "Best,\nJorge");
        let pieces = protect_snippets("thanks so much, my sign off.", &[s.clone()]);
        assert_eq!(
            pieces,
            vec![
                Piece::Speech("thanks so much,".into()),
                Piece::Snippet { text: "Best,\nJorge.".into(), id: s.id },
            ]
        );
    }

    #[test]
    fn list_import() {
        let existing = [vocab("nano", "Nano")];
        let parsed = parse_list(
            "Kubernetes, Tauri\nmac island → Nano\nsuper whisper -> Superwhisper\nk eight s => k8s\nfoo\tbar\nnano\nTauri\n",
            &existing,
        );
        let pairs: Vec<(&str, &str)> =
            parsed.iter().map(|e| (e.phrase.as_str(), e.replacement.as_str())).collect();
        assert_eq!(
            pairs,
            vec![
                ("Kubernetes", "Kubernetes"),
                ("Tauri", "Tauri"),
                ("mac island", "Nano"),
                ("super whisper", "Superwhisper"),
                ("k eight s", "k8s"),
                ("foo", "bar"),
            ]
        );
    }

    #[test]
    fn equals_arrow() {
        let parsed = parse_list("gee pee tee = GPT", &[]);
        assert_eq!(parsed[0].phrase, "gee pee tee");
        assert_eq!(parsed[0].replacement, "GPT");
    }
}
