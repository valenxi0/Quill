//! Split long transcripts into cleaner-sized chunks (section 4.2).

pub const WORD_BUDGET: usize = 400;

fn words(s: &str) -> usize {
    s.split_whitespace().count()
}

/// Split after any char in `ends` that is followed by whitespace (or the end).
fn split_after(text: &str, ends: &[char]) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        cur.push(c);
        if ends.contains(&c) && chars.peek().map_or(true, |n| n.is_whitespace()) {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

fn hard_split(text: &str, budget: usize) -> Vec<String> {
    let ws: Vec<&str> = text.split_whitespace().collect();
    ws.chunks(budget).map(|c| c.join(" ")).collect()
}

/// Greedily pack units into chunks of at most `budget` words.
fn pack(units: Vec<String>, budget: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cur_words = 0;
    for u in units {
        let n = words(&u);
        if cur_words > 0 && cur_words + n > budget {
            out.push(std::mem::take(&mut cur));
            cur_words = 0;
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(&u);
        cur_words += n;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Chunk at sentence ends first, then clause punctuation, then hard word
/// boundaries, so each chunk has at most `budget` words.
pub fn chunk(text: &str, budget: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return vec![];
    }
    if words(text) <= budget {
        return vec![text.to_string()];
    }
    let mut units = Vec::new();
    for sentence in split_after(text, &['.', '!', '?']) {
        if words(&sentence) <= budget {
            units.push(sentence);
            continue;
        }
        for clause in split_after(&sentence, &[',', ';', ':']) {
            if words(&clause) <= budget {
                units.push(clause);
            } else {
                units.extend(hard_split(&clause, budget));
            }
        }
    }
    pack(units, budget)
}

/// Split at the sentence end nearest the middle, so a rejected cleanup can
/// be retried on smaller pieces. `None` for a single sentence.
pub fn halve(text: &str) -> Option<(String, String)> {
    let sentences = split_after(text, &['.', '!', '?']);
    if sentences.len() < 2 {
        return None;
    }
    let half = words(text) / 2;
    let mut seen = 0;
    let mut cut = 1;
    let mut best = usize::MAX;
    for (i, s) in sentences.iter().enumerate().take(sentences.len() - 1) {
        seen += words(s);
        if seen.abs_diff(half) < best {
            best = seen.abs_diff(half);
            cut = i + 1;
        }
    }
    Some((sentences[..cut].join(" "), sentences[cut..].join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_at_the_middle_sentence() {
        assert_eq!(halve("a b. c d. e f g h."), Some(("a b. c d.".into(), "e f g h.".into())));
        assert_eq!(halve("one sentence, with a clause"), None);
    }

    #[test]
    fn under_budget_is_one_chunk() {
        assert_eq!(chunk("one two three.", 400), vec!["one two three."]);
        assert!(chunk("  ", 400).is_empty());
    }

    #[test]
    fn splits_at_sentences() {
        let c = chunk("a b c. d e f. g h.", 4);
        assert_eq!(c, vec!["a b c.", "d e f.", "g h."]);
        let c = chunk("a b. c d. e f g h i.", 4);
        assert_eq!(c, vec!["a b. c d.", "e f g h", "i."]);
    }

    #[test]
    fn falls_back_to_clauses_then_words() {
        let c = chunk("a b, c d; e f g", 3);
        assert_eq!(c, vec!["a b,", "c d;", "e f g"]);
        let c = chunk("a b c d e f g", 3);
        assert_eq!(c, vec!["a b c", "d e f", "g"]);
        for part in chunk(&"word ".repeat(1000), WORD_BUDGET) {
            assert!(words(&part) <= WORD_BUDGET);
        }
    }
}
