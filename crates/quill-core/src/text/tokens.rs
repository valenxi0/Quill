//! Word tokens with their surrounding punctuation split off (section 4.8).

/// One whitespace-separated word. `lead` and `trail` hold the punctuation or
/// symbols split off each end; `sep` is the whitespace that preceded it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub sep: String,
    pub lead: String,
    pub core: String,
    pub trail: String,
}

impl Token {
    pub fn text(&self) -> String {
        format!("{}{}{}", self.lead, self.core, self.trail)
    }
}

/// Punctuation or symbol that may be split off a word's ends. `@` stays
/// attached so handles and emails keep their shape.
fn is_edge_punct(c: char) -> bool {
    !c.is_alphanumeric() && !c.is_whitespace() && c != '@'
}

pub fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut sep = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, sep: &mut String, tokens: &mut Vec<Token>| {
        if word.is_empty() {
            return;
        }
        tokens.push(split_word(std::mem::take(sep), word));
        word.clear();
    };
    for c in text.chars() {
        if c.is_whitespace() {
            flush(&mut word, &mut sep, &mut tokens);
            sep.push(c);
        } else {
            word.push(c);
        }
    }
    flush(&mut word, &mut sep, &mut tokens);
    tokens
}

fn split_word(sep: String, word: &str) -> Token {
    let chars: Vec<char> = word.chars().collect();
    let mut start = 0;
    while start < chars.len() && is_edge_punct(chars[start]) {
        start += 1;
    }
    let mut end = chars.len();
    while end > start && is_edge_punct(chars[end - 1]) {
        end -= 1;
    }
    // A word that is only punctuation keeps it all as `lead`.
    Token {
        sep,
        lead: chars[..start].iter().collect(),
        core: chars[start..end].iter().collect(),
        trail: chars[end..].iter().collect(),
    }
}

/// Lowercase and drop punctuation, symbols and whitespace.
pub fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Rebuild text from tokens, preserving the original separators.
pub fn detokenize(tokens: &[Token]) -> String {
    let mut out = String::new();
    for t in tokens {
        out.push_str(&t.sep);
        out.push_str(&t.lead);
        out.push_str(&t.core);
        out.push_str(&t.trail);
    }
    out
}

/// Number of words, counted by whitespace runs.
pub fn word_count(s: &str) -> usize {
    s.split_whitespace().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_edges_but_keeps_at() {
        let t = tokenize("(hello), @nano Node.js!");
        assert_eq!(t[0].lead, "(");
        assert_eq!(t[0].core, "hello");
        assert_eq!(t[0].trail, "),");
        assert_eq!(t[1].core, "@nano");
        assert_eq!(t[2].core, "Node.js");
        assert_eq!(t[2].trail, "!");
        assert_eq!(detokenize(&t), " (hello), @nano Node.js!".trim_start());
    }

    #[test]
    fn normalizes() {
        assert_eq!(normalize("Mac-Island!"), "macisland");
    }
}
