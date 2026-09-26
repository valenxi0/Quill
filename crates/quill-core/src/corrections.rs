//! Learning from corrections (section 4.9): diff a pasted dictation against
//! what the user left in the field, and keep only misheard-word fixes.

use serde::{Deserialize, Serialize};

use crate::text::tokens::normalize;
use crate::text::vocabulary::VocabEntry;

/// Strip punctuation from the ends of a word, keep it inside (`Node.js`).
fn word_key(w: &str) -> &str {
    w.trim_matches(|c: char| !c.is_alphanumeric())
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Spellings that look alike: case-only changes, or letters mostly shared.
fn looks_similar(from: &str, to: &str) -> bool {
    if from.to_lowercase() == to.to_lowercase() {
        return true;
    }
    let a: Vec<char> = normalize(from).chars().collect();
    let b: Vec<char> = normalize(to).chars().collect();
    let longest = a.len().max(b.len());
    if longest == 0 {
        return false;
    }
    1.0 - levenshtein(&a, &b) as f32 / longest as f32 >= 0.5
}

const MAX_SPAN_WORDS: usize = 3;

/// Word pairs `(heard, fixed)` that look like a misheard word. Rewrites,
/// deletions, and edits touching more than half the words are ignored.
pub fn diff(original: &str, edited: &str) -> Vec<(String, String)> {
    let a: Vec<&str> = original.split_whitespace().map(word_key).filter(|w| !w.is_empty()).collect();
    let b: Vec<&str> = edited.split_whitespace().map(word_key).filter(|w| !w.is_empty()).collect();
    if a.is_empty() {
        return vec![];
    }
    // LCS table from the end.
    let mut lcs = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    // Walk it, collecting hunks of deleted and inserted words.
    let mut hunks: Vec<(Vec<&str>, Vec<&str>)> = Vec::new();
    let mut cur: (Vec<&str>, Vec<&str>) = (vec![], vec![]);
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            if !cur.0.is_empty() || !cur.1.is_empty() {
                hunks.push(std::mem::take(&mut cur));
            }
            i += 1;
            j += 1;
        } else if j < b.len() && (i == a.len() || lcs[i][j + 1] >= lcs[i + 1][j]) {
            cur.1.push(b[j]);
            j += 1;
        } else {
            cur.0.push(a[i]);
            i += 1;
        }
    }
    if !cur.0.is_empty() || !cur.1.is_empty() {
        hunks.push(cur);
    }
    let touched: usize = hunks.iter().map(|(d, ins)| d.len().max(ins.len())).sum();
    if touched * 2 > a.len() {
        return vec![];
    }
    hunks
        .into_iter()
        .filter(|(d, ins)| !d.is_empty() && !ins.is_empty())
        .filter(|(d, ins)| d.len() <= MAX_SPAN_WORDS && ins.len() <= MAX_SPAN_WORDS)
        .map(|(d, ins)| (d.join(" "), ins.join(" ")))
        .filter(|(from, to)| looks_similar(from, to))
        .collect()
}

/// Where a dictation sits in a field, by the text just around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    pub before: String,
    pub after: String,
    pub pasted: String,
}

const ANCHOR_CHARS: usize = 24;

impl Anchor {
    /// Anchor on the last occurrence of `pasted` in the field's text.
    pub fn new(field: &str, pasted: &str) -> Option<Anchor> {
        let at = field.rfind(pasted)?;
        let before: String = field[..at].chars().rev().take(ANCHOR_CHARS).collect::<Vec<_>>().into_iter().rev().collect();
        let after: String = field[at + pasted.len()..].chars().take(ANCHOR_CHARS).collect();
        Some(Anchor { before, after, pasted: pasted.to_string() })
    }

    /// The dictation's region in a later read of the field, so edits
    /// elsewhere are ignored.
    pub fn locate<'a>(&self, field: &'a str) -> Option<&'a str> {
        let start = if self.before.is_empty() { 0 } else { field.find(&self.before)? + self.before.len() };
        let rest = &field[start..];
        let end = if self.after.is_empty() { rest.len() } else { rest.find(&self.after)? };
        Some(&rest[..end])
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionPair {
    pub from: String,
    pub to: String,
    pub count: u32,
    #[serde(default)]
    pub dismissed: bool,
}

/// `corrections.json`: word pairs only.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Corrections {
    pub pairs: Vec<CorrectionPair>,
    pub seen: u64,
}

/// A fix seen this many times becomes a suggestion.
pub const SUGGEST_AFTER: u32 = 2;

impl Corrections {
    /// Record fixes; returns pairs that just became suggestions.
    pub fn record(&mut self, fixes: &[(String, String)]) -> Vec<CorrectionPair> {
        let mut promoted = Vec::new();
        for (from, to) in fixes {
            self.seen += 1;
            match self.pairs.iter_mut().find(|p| &p.from == from && &p.to == to) {
                Some(p) => {
                    p.count += 1;
                    if p.count == SUGGEST_AFTER && !p.dismissed {
                        promoted.push(p.clone());
                    }
                }
                None => self.pairs.push(CorrectionPair { from: from.clone(), to: to.clone(), count: 1, dismissed: false }),
            }
        }
        promoted
    }

    /// Suggestions not yet dismissed and not already covered by the dictionary.
    pub fn suggestions(&self, vocabulary: &[VocabEntry]) -> Vec<CorrectionPair> {
        self.pairs
            .iter()
            .filter(|p| p.count >= SUGGEST_AFTER && !p.dismissed)
            .filter(|p| !covered(p, vocabulary))
            .cloned()
            .collect()
    }

    pub fn dismiss(&mut self, from: &str, to: &str) {
        for p in self.pairs.iter_mut().filter(|p| p.from == from && p.to == to) {
            p.dismissed = true;
        }
    }

    pub fn forget_all(&mut self) {
        *self = Self::default();
    }
}

fn covered(pair: &CorrectionPair, vocabulary: &[VocabEntry]) -> bool {
    let key = normalize(&pair.from);
    vocabulary.iter().any(|e| normalize(&e.phrase) == key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::vocabulary::EntryKind;

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn keeps_misheard_words() {
        assert_eq!(
            diff("We deploy with Tori on Windows today.", "We deploy with Tauri on Windows today."),
            pairs(&[("Tori", "Tauri")])
        );
        assert_eq!(diff("ask nano about it now", "ask Nano about it now"), pairs(&[("nano", "Nano")]));
        assert_eq!(
            diff("the node dot js server runs fine here", "the Node.js server runs fine here"),
            pairs(&[("node dot js", "Node.js")]),
            "a spoken spelling collapsing into the written one"
        );
    }

    #[test]
    fn rejects_rewrites_and_big_edits() {
        assert!(diff("that is a big problem for us", "that is a large problem for us").is_empty());
        assert!(diff("one two three four", "five six seven four").is_empty(), "touches over half");
        assert!(diff("please send the file now", "please send now").is_empty(), "deletions ignored");
        assert!(diff("hello there", "hello there friend").is_empty(), "insertions ignored");
    }

    #[test]
    fn word_punctuation() {
        assert_eq!(diff("I use Tori, mostly.", "I use Tauri, mostly."), pairs(&[("Tori", "Tauri")]));
        assert!(diff("Hello, world.", "Hello world").is_empty());
    }

    #[test]
    fn anchors() {
        let field = "Dear team,\nWe deploy with Tori today.\nThanks";
        let a = Anchor::new(field, "We deploy with Tori today.").unwrap();
        let later = "Dear team,\nWe deploy with Tauri today.\nThanks, all";
        assert_eq!(a.locate(later), Some("We deploy with Tauri today."));
        assert_eq!(a.locate("completely different"), None);
        let whole = Anchor::new("hi", "hi").unwrap();
        assert_eq!(whole.locate("hey"), Some("hey"));
    }

    #[test]
    fn store_suggests_after_two_and_hides_covered() {
        let mut c = Corrections::default();
        assert!(c.record(&pairs(&[("Tori", "Tauri")])).is_empty());
        assert!(c.suggestions(&[]).is_empty());
        let promoted = c.record(&pairs(&[("Tori", "Tauri")]));
        assert_eq!(promoted.len(), 1);
        assert_eq!(c.suggestions(&[]).len(), 1);
        assert_eq!(c.seen, 2);
        let entry = VocabEntry::new(EntryKind::Vocabulary, "tori", "Tauri");
        assert!(c.suggestions(&[entry]).is_empty());
        c.dismiss("Tori", "Tauri");
        assert!(c.suggestions(&[]).is_empty());
        c.forget_all();
        assert_eq!(c, Corrections::default());
    }
}
