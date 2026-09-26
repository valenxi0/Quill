//! The English gate for English-only stages (section 4.2, step 4.2).

/// Whether English-only stages (S1-mini, number formatting) should run.
/// A language the engine reported is trusted. Otherwise only a confident
/// non-English reading turns them off: at least 3 words and confidence of at
/// least 0.8. Short or ambiguous text counts as English.
pub fn is_english(text: &str, reported: Option<&str>) -> bool {
    if let Some(lang) = reported.filter(|l| !l.is_empty() && *l != "auto") {
        return lang.eq_ignore_ascii_case("en") || lang.to_ascii_lowercase().starts_with("en-");
    }
    if text.split_whitespace().count() < 3 {
        return true;
    }
    match whatlang::detect(text) {
        Some(info) if info.confidence() >= 0.8 => info.lang() == whatlang::Lang::Eng,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate() {
        assert!(is_english("Hallo Welt", None), "too short to judge");
        assert!(!is_english("Ich möchte morgen früh mit dem Zug nach Berlin fahren, weil ich dort arbeite.", None));
        assert!(is_english("I would like to take the train to Berlin tomorrow morning.", None));
        assert!(!is_english("I would like to take the train", Some("de")));
        assert!(is_english("Ich möchte morgen", Some("en")));
    }
}
