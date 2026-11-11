
/// Sounds that are never words. Cleanup strips them.
pub const HESITATIONS: &[&str] = &["um", "umm", "uh", "uhh", "uhm", "erm"];

const MAYBE_HESITATIONS: &[&str] = &["er", "hmm", "mm", "ah"];

/// Lowercase, without punctuation.
pub fn is_hesitation(word: &str) -> bool {
    HESITATIONS.contains(&word) || MAYBE_HESITATIONS.contains(&word)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hesitations() {
        assert!(is_hesitation("um") && is_hesitation("hmm") && is_hesitation("er"));
        assert!(!is_hesitation("umbrella") && !is_hesitation("so"));
        assert!(!HESITATIONS.contains(&"hmm"), "hmm can be meant, so cleanup leaves it");
    }
}
