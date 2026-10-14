//! Fitting a dictation into the text around the caret: casing, spaces, and
//! the closing full stop. Runs last, on the final text.

/// Text read from the focused field at recording start.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Surroundings {
    /// Up to a few hundred characters before the caret or selection.
    pub before: String,
    /// A few characters after it.
    pub after: String,
    /// The selected text the dictation replaces, if any.
    pub selection: String,
}

const OPENERS: &[char] = &['(', '[', '{', '"', '\'', '“', '‘', '/', '@', '#', '$', '-', '_', '`'];
const CLOSERS: &[char] = &['.', ',', ';', ':', '!', '?', ')', ']', '}', '"', '\'', '”', '’', '%'];
const SENTENCE_ENDS: &[char] = &['.', '!', '?', '…'];
/// Words that keep their capital wherever they land.
const ALWAYS_CAPITAL: &[&str] = &[
    "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday", "January", "February",
    "March", "April", "June", "July", "August", "September", "October", "November", "December", "English",
];

/// Whether `before` leaves the caret inside a sentence.
fn continues_sentence(before: &str) -> bool {
    let trimmed = before.trim_end_matches([' ', '\t']);
    // An opening quote starts quoted speech, which takes a capital.
    if trimmed.is_empty() || trimmed.ends_with(['\n', '\r', '"', '“', '‘']) {
        return false;
    }
    let core = trimmed.trim_end_matches(['(', '[']);
    match core.chars().last() {
        Some(c) => !SENTENCE_ENDS.contains(&c) && c != ':',
        // Only an opening bracket or quote before the caret.
        None => false,
    }
}

/// The first word, and whether lowercasing its initial is safe: an ordinary
/// capitalized word, not "I", an acronym, a weekday, or a word the dictation
/// capitalizes again mid-sentence (likely a name).
fn lowercasable(text: &str, protected: &[String]) -> bool {
    let word: String = text.chars().take_while(|c| c.is_alphanumeric() || *c == '\'' || *c == '’').collect();
    let letters: Vec<char> = word.chars().filter(|c| c.is_alphabetic()).collect();
    let Some(first) = letters.first() else { return false };
    if !first.is_uppercase() || letters[1..].iter().any(|c| c.is_uppercase()) {
        return false;
    }
    let bare = word.trim_end_matches(['\'', '’', 's']);
    if word == "I" || word.starts_with("I'") || word.starts_with("I’") || ALWAYS_CAPITAL.contains(&bare) {
        return false;
    }
    if protected.iter().any(|p| p.split_whitespace().next() == Some(word.as_str())) {
        return false;
    }
    // The same word capitalized after the first word: a name.
    !text
        .split_whitespace()
        .skip(1)
        .zip(text.split_whitespace())
        .any(|(w, prev)| {
            w.trim_matches(|c: char| !c.is_alphanumeric()) == word && !prev.ends_with(SENTENCE_ENDS)
        })
}

fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// One sentence ending in a single full stop.
fn single_sentence_with_stop(text: &str) -> bool {
    let t = text.trim_end();
    if !t.ends_with('.') || t.ends_with("..") || t.contains('\n') {
        return false;
    }
    let body = &t[..t.len() - 1];
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if SENTENCE_ENDS.contains(&c) && chars.peek().is_some_and(|n| n.is_whitespace()) {
            return false;
        }
    }
    true
}

/// Fit `text` between the caret's neighbours. `protected` holds vocabulary
/// and snippet output that must keep its casing.
pub fn fit(text: &str, around: &Surroundings, protected: &[String]) -> String {
    let mut out = text.trim().to_string();
    if out.is_empty() {
        return text.to_string();
    }
    let Surroundings { before, after, selection } = around;

    // Casing. A replaced selection that started lowercase continues a sentence.
    let selection_lower = selection.trim_start().chars().next().is_some_and(|c| c.is_lowercase());
    if (continues_sentence(before) || selection_lower) && lowercasable(&out, protected) {
        out = lowercase_first(&out);
    }

    // The closing full stop.
    let next = after.chars().next();
    let continues_after = after.trim_start_matches([' ', '\t']).chars().next().is_some_and(|c| c.is_lowercase() || c.is_ascii_digit());
    let selection_unpunctuated = !selection.trim().is_empty() && !selection.trim_end().ends_with(SENTENCE_ENDS);
    if (continues_after || selection_unpunctuated) && single_sentence_with_stop(&out) {
        out.pop();
    }

    // Spaces at both edges.
    let prev = before.chars().last();
    let first = out.chars().next();
    if let (Some(p), Some(f)) = (prev, first) {
        if !p.is_whitespace() && !OPENERS.contains(&p) && !CLOSERS.contains(&f) {
            out.insert(0, ' ');
        }
    }
    let last = out.chars().last();
    if let (Some(n), Some(l)) = (next, last) {
        if !n.is_whitespace() && !CLOSERS.contains(&n) && !OPENERS.contains(&l) && !l.is_whitespace() {
            out.push(' ');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(before: &str, after: &str) -> Surroundings {
        Surroundings { before: before.into(), after: after.into(), selection: String::new() }
    }

    #[test]
    fn empty_field_is_untouched() {
        assert_eq!(fit("Send it tomorrow.", &at("", ""), &[]), "Send it tomorrow.");
    }

    #[test]
    fn mid_sentence_lowercases_and_spaces() {
        assert_eq!(fit("The report is ready.", &at("I think", ""), &[]), " the report is ready.");
        assert_eq!(fit("The report is ready.", &at("I think ", ""), &[]), "the report is ready.");
        assert_eq!(fit("Tomorrow.", &at("Let's ship it, ", ""), &[]), "tomorrow.");
    }

    #[test]
    fn after_a_sentence_keeps_the_capital_and_adds_a_space() {
        assert_eq!(fit("Then we ship.", &at("Done.", ""), &[]), " Then we ship.");
        assert_eq!(fit("Then we ship.", &at("Done.\n", ""), &[]), "Then we ship.");
        assert_eq!(fit("Then we ship.", &at("Note: ", ""), &[]), "Then we ship.");
    }

    #[test]
    fn names_and_pronouns_keep_their_capital() {
        assert_eq!(fit("I can do it.", &at("and ", ""), &[]), "I can do it.");
        assert_eq!(fit("I'm in.", &at("and ", ""), &[]), "I'm in.");
        assert_eq!(fit("Tuesday works.", &at("maybe ", ""), &[]), "Tuesday works.");
        assert_eq!(fit("NASA called.", &at("then ", ""), &[]), "NASA called.");
        assert_eq!(fit("Sarah said Sarah would come.", &at("and ", ""), &[]), "Sarah said Sarah would come.");
        assert_eq!(fit("Oruk is ready.", &at("and ", ""), &["Oruk".to_string()]), "Oruk is ready.");
        assert_eq!(fit("McDonald called.", &at("then ", ""), &[]), "McDonald called.");
    }

    #[test]
    fn continuing_text_after_drops_the_full_stop() {
        assert_eq!(fit("Next week.", &at("We meet ", " to plan"), &[]), "next week");
        assert_eq!(fit("Next week.", &at("We meet ", "to plan"), &[]), "next week ");
        assert_eq!(fit("Next week.", &at("We meet ", " Then we plan."), &[]), "next week.");
        assert_eq!(fit("Is it done?", &at("", "and more"), &[]), "Is it done? ");
        assert_eq!(fit("One. Two.", &at("", " three"), &[]), "One. Two.");
    }

    #[test]
    fn edges_next_to_punctuation_and_brackets() {
        assert_eq!(fit("Fine.", &at("(", ")"), &[]), "Fine.");
        assert_eq!(fit("Yes", &at("He said \"", "\""), &[]), "Yes");
        assert_eq!(fit("Done.", &at("word", ","), &[]), " done.");
    }

    #[test]
    fn replacing_a_selection() {
        let s = Surroundings { before: "We should ".into(), after: " soon".into(), selection: "ship it".into() };
        assert_eq!(fit("Release it.", &s, &[]), "release it");
        let s = Surroundings { before: "".into(), after: "".into(), selection: "old text".into() };
        assert_eq!(fit("New text.", &s, &[]), "new text");
        let s = Surroundings { before: "".into(), after: "".into(), selection: "Old sentence.".into() };
        assert_eq!(fit("New sentence.", &s, &[]), "New sentence.");
    }

    #[test]
    fn multi_line_text_keeps_its_stop() {
        assert_eq!(fit("Hi.\nBye.", &at("", " more"), &[]), "Hi.\nBye.");
    }
}
