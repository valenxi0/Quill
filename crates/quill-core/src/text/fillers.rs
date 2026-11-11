//! Word lists shared by cleanup, the cleanup guard, and voice insights.

/// Sounds that are never words. Cleanup strips them.
pub const HESITATIONS: &[&str] = &["um", "umm", "uh", "uhh", "uhm", "erm"];

/// Counted as hesitations, but left in the text: "hmm" and "ah" can be
/// meant, and "er" can be "the ER".
const MAYBE_HESITATIONS: &[&str] = &["er", "hmm", "mm", "ah"];

/// Lowercase, without punctuation.
pub fn is_hesitation(word: &str) -> bool {
    HESITATIONS.contains(&word) || MAYBE_HESITATIONS.contains(&word)
}

/// Words that say little about whether cleanup kept the meaning: fillers,
/// self-correction cues, units the number formatter turns into symbols
/// ("$3.50", "5°C", "20 km/h"), and common short words. Lowercase, with
/// apostrophes dropped.
const LIGHT: &[&str] = &[
    // Fillers and self-corrections.
    "so", "okay", "ok", "well", "like", "basically", "actually", "literally", "honestly", "just", "really", "yeah",
    "right", "maybe", "please", "no", "sorry", "wait", "scratch", "oops", "kinda", "sorta", "know", "mean", "meant", "kind",
    "sort", "think", "thing", "things", "some", "still", "going", "gonna", "wanna",
    // Units.
    "dollar", "dollars", "cent", "cents", "percent", "point", "degree", "degrees", "celsius", "fahrenheit",
    "kilometer", "kilometers", "kilometre", "kilometres", "meter", "meters", "metre", "metres", "mile", "miles", "per",
    "hour", "hours", "minute", "minutes", "seconds", "pound", "pounds", "euro", "euros", "kilogram", "kilograms",
    "kilo", "kilos", "gram", "grams", "inch", "inches", "foot", "feet", "oclock", "pm",
    // Common short words.
    "a", "an", "the", "and", "or", "but", "to", "of", "in", "on", "at", "for", "with", "by", "as", "from", "into",
    "about", "is", "are", "am", "was", "were", "be", "been", "it", "its", "this", "that", "i", "me", "my", "we", "us",
    "our", "you", "your", "he", "she", "they", "them", "their", "there", "here", "do", "not", "have", "has", "will",
    "would", "could", "should", "can", "if", "whether", "then", "when", "what", "which", "because", "also", "very",
];

/// Lowercase, with apostrophes dropped. Single letters ("p m", "e mail")
/// count as light.
pub fn is_light(word: &str) -> bool {
    word.chars().count() == 1 || is_hesitation(word) || LIGHT.contains(&word)
}

/// Two-word fillers.
pub const FILLER_PAIRS: &[(&str, &str)] = &[("you", "know"), ("i", "mean"), ("i", "meant")];

/// Openings that take back what came before: "Friday. Sorry, I meant Monday."
const CORRECTIONS: &[&[&str]] = &[
    &["sorry"],
    &["no"],
    &["actually"],
    &["wait"],
    &["rather"],
    &["i", "mean"],
    &["i", "meant"],
    &["scratch", "that"],
    &["never", "mind"],
    &["let", "me", "start", "over"],
    &["let", "me", "rephrase"],
    &["start", "over"],
];

/// How many of lowercase `words` open a correction, hesitations before it
/// included; 0 if they don't.
pub fn correction_len(words: &[String]) -> usize {
    let start = words.iter().position(|w| !is_hesitation(w)).unwrap_or(words.len());
    let rest = &words[start..];
    CORRECTIONS
        .iter()
        .filter(|cue| rest.len() >= cue.len() && cue.iter().zip(rest).all(|(c, w)| c == w))
        .map(|cue| start + cue.len())
        .max()
        .unwrap_or(0)
}

/// Words a sentence can't end on, so a clause ending on one is a false
/// start: "I think we should." "Can you send me the."
const DANGLING: &[&str] = &[
    "a", "an", "the", "to", "of", "in", "on", "at", "for", "with", "by", "from", "into", "about", "and", "or", "but",
    "because", "so", "than", "that", "if", "should", "could", "would", "will", "can", "must", "might", "may", "shall",
    "is", "are", "was", "were", "be", "am", "do", "does", "did", "have", "has", "had", "my", "your", "our", "their",
    "his", "her", "its", "we", "they", "i", "you", "he", "she", "when", "what", "which", "who", "where", "how", "going",
    "gonna", "wanna", "want", "need",
];

pub fn is_dangling(word: &str) -> bool {
    DANGLING.contains(&word)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn corrections() {
        assert_eq!(correction_len(&words("sorry i meant monday")), 1);
        assert_eq!(correction_len(&words("i meant monday")), 2);
        assert_eq!(correction_len(&words("um let me start over")), 5);
        assert_eq!(correction_len(&words("let me know")), 0);
        assert_eq!(correction_len(&words("")), 0);
    }

    #[test]
    fn light_words() {
        assert!(is_light("p") && is_light("dollars") && is_light("um") && is_light("hmm"));
        assert!(!is_light("monday") && !is_light("translate"));
    }
}
