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
    #[serde(default)]
    pub app_exe: Option<String>,
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
            app_exe: None,
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

/// Select once at recording start; app entries override global phrases.
pub fn for_app(entries: &[VocabEntry], exe: &str) -> Vec<VocabEntry> {
    let scoped: std::collections::HashSet<String> = entries.iter()
        .filter(|e| e.is_active() && e.app_exe.as_deref().is_some_and(|app| app.eq_ignore_ascii_case(exe)))
        .map(VocabEntry::key).collect();
    entries.iter().filter(|e| e.is_active() && match e.app_exe.as_deref() {
        Some(app) => app.eq_ignore_ascii_case(exe),
        None => !scoped.contains(&e.key()),
    }).cloned().collect()
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
    Break(usize),
    /// A snippet expansion, spliced back verbatim.
    Snippet { text: String, id: Uuid },
}

/// Split a transcript at snippet trigger phrases so the cleaner only ever
/// sees speech. Each expansion keeps the trigger's surrounding punctuation.
pub fn protect_snippets(text: &str, entries: &[VocabEntry]) -> Vec<Piece> {
    protect_layout(text, entries, false)
}

pub fn protect_layout(text: &str, entries: &[VocabEntry], layout_cues: bool) -> Vec<Piece> {
    use super::layout::{capitalize, cue_at};
    let active: Vec<&VocabEntry> = entries.iter().filter(|e| e.is_active() && e.kind == EntryKind::Snippet).collect();
    let tokens = tokenize(text);
    let matches = find_matches(&tokens, &active);
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let mut capitalize_next = false;
    let flush = |start: usize, end: usize, capitalize_next: &mut bool, pieces: &mut Vec<Piece>| {
        let text = detokenize(&tokens[start..end]).trim().to_string();
        if !text.is_empty() {
            let text = if *capitalize_next { capitalize(&text) } else { text };
            *capitalize_next = false;
            pieces.push(Piece::Speech(text));
        }
    };
    while i < tokens.len() {
        if let Some(m) = matches.iter().find(|m| m.start == i) {
            flush(start, i, &mut capitalize_next, &mut pieces);
            let entry = active[m.entry];
            pieces.push(Piece::Snippet {
                text: format!("{}{}{}", tokens[i].lead, entry.replacement, tokens[m.end - 1].trail),
                id: entry.id,
            });
            capitalize_next = false;
            i = m.end;
            start = i;
        } else if let Some((end, breaks)) = layout_cues.then(|| cue_at(&tokens, i)).flatten()
            .filter(|(end, _)| !matches.iter().any(|m| m.start > i && m.start < *end)) {
            flush(start, i, &mut capitalize_next, &mut pieces);
            if breaks == 2 {
                if let Some(Piece::Speech(text)) = pieces.last_mut() {
                    if text.ends_with([',', ';', ':']) { text.pop(); text.push('.'); }
                }
            }
            pieces.push(Piece::Break(breaks));
            capitalize_next = true;
            i = end;
            start = i;
        } else {
            i += 1;
        }
    }
    flush(start, tokens.len(), &mut capitalize_next, &mut pieces);
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

pub fn parse_list_for_app(input: &str, existing: &[VocabEntry], app_exe: Option<&str>) -> Vec<VocabEntry> {
    let scoped: Vec<_> = existing.iter().filter(|e| match (e.app_exe.as_deref(), app_exe) {
        (None, None) => true,
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => false,
    }).cloned().collect();
    let mut entries = parse_list(input, &scoped);
    for entry in &mut entries { entry.app_exe = app_exe.map(str::to_lowercase); }
    entries
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
    #[test]
    fn imports_allow_the_same_phrase_in_different_apps() {
        let global = vocab("rust", "rust");
        let code = parse_list_for_app("rust -> Rust", &[global.clone()], Some("Code.exe"));
        assert_eq!(apply("rust", &for_app(&code, "code.exe"), None).text, "Rust");
        let existing = vec![global, code[0].clone()];
        assert_eq!(parse_list_for_app("rust -> Rust", &existing, Some("CODE.EXE")), vec![]);
        let chat = parse_list_for_app("rust -> rust", &existing, Some("discord.exe"));
        assert_eq!(chat[0].app_exe.as_deref(), Some("discord.exe"));
        assert_eq!(apply("rust", &for_app(&chat, "Discord.exe"), None).text, "rust");
    }
    #[test]
    fn app_vocabulary_overrides_globals_without_leaking_to_other_apps() {
        let global = vocab("rust", "rust");
        let mut code = vocab("rust", "Rust");
        code.app_exe = Some("Code.exe".into());
        let mut term = vocab("or rook", "Oruk");
        term.app_exe = Some("code.exe".into());
        let entries = vec![global, code, term];
        let code = for_app(&entries, "CODE.EXE");
        assert_eq!(apply("rust and or rook", &code, None).text, "Rust and Oruk");
        assert_eq!(vocabulary_terms(&code), vec!["Rust", "Oruk"]);
        assert_eq!(apply("rust and or rook", &for_app(&entries, "Discord.exe"), None).text, "rust and or rook");
        assert_eq!(apply("rust", &for_app(&entries, ""), None).text, "rust");
    }

    #[test]
    fn disabled_app_entry_leaves_global_entry_active_and_old_entries_load_globally() {
        let mut scoped = vocab("q", "Quill");
        scoped.app_exe = Some("Code.exe".into());
        scoped.is_enabled = false;
        assert_eq!(apply("q", &for_app(&[vocab("q", "queue"), scoped], "code.exe"), None).text, "queue");
        let old = format!(r#"{{"id":"{}","phrase":"q","replacement":"queue","kind":"vocabulary","isEnabled":true}}"#, uuid::Uuid::new_v4());
        let entry: VocabEntry = serde_json::from_str(&old).unwrap();
        assert_eq!(apply("q", &for_app(&[entry], "Discord.exe"), None).text, "queue");
    }

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
