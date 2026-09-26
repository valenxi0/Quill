//! Cleanup guard (section 4.5): less polished beats a changed meaning.

const NEGATIONS: &[&str] = &[
    "not", "never", "cannot", "neither", "nor", "none", "nothing", "nobody", "nowhere",
];

const BARE_CONTRACTIONS: &[&str] = &[
    "dont", "doesnt", "didnt", "cant", "couldnt", "wont", "wouldnt", "shouldnt", "isnt",
    "arent", "wasnt", "werent", "hasnt", "havent", "hadnt", "mustnt", "neednt", "aint",
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
        .filter(|w| {
            NEGATIONS.contains(&w.as_str())
                || w.ends_with("n't")
                || BARE_CONTRACTIONS.contains(&w.as_str())
        })
        .count()
}

/// Returns the text to keep for one chunk, and whether the raw text was restored.
pub fn guard<'a>(raw: &'a str, cleaned: &'a str) -> (&'a str, bool) {
    let raw_words = raw.split_whitespace().count();
    if cleaned.trim().is_empty() && raw_words >= 12 {
        return (raw, true);
    }
    if negation_count(cleaned) < negation_count(raw) {
        return (raw, true);
    }
    (cleaned, false)
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
