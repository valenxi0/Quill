//! Small spoken commands applied to finished text: emoji names, a closing
//! "press enter", and splitting long pastes for terminal apps.

/// Spoken names, longest first within a family.
const EMOJI: &[(&str, &str)] = &[
    ("crying laughing", "😂"), ("laughing crying", "😂"), ("tears of joy", "😂"), ("laughing", "😂"), ("joy", "😂"),
    ("loudly crying", "😭"), ("sob", "😭"), ("crying", "😢"), ("sad", "😢"),
    ("red heart", "❤️"), ("heart", "❤️"), ("heart eyes", "😍"), ("broken heart", "💔"),
    ("thumbs up", "👍"), ("thumbs down", "👎"), ("ok hand", "👌"), ("okay hand", "👌"),
    ("clapping", "👏"), ("clap", "👏"), ("praying hands", "🙏"), ("folded hands", "🙏"), ("pray", "🙏"),
    ("raised hands", "🙌"), ("waving hand", "👋"), ("wave", "👋"), ("muscle", "💪"), ("flexed biceps", "💪"),
    ("smiling face", "😊"), ("smiley", "😊"), ("smile", "😊"), ("grinning", "😀"), ("wink", "😉"), ("winking face", "😉"),
    ("thinking face", "🤔"), ("thinking", "🤔"), ("eyes", "👀"), ("sunglasses", "😎"), ("cool", "😎"),
    ("upside down face", "🙃"), ("upside down", "🙃"), ("grimacing", "😬"), ("grimace", "😬"), ("melting face", "🫠"),
    ("saluting face", "🫡"), ("salute", "🫡"), ("pleading face", "🥺"), ("pleading", "🥺"), ("smirk", "😏"),
    ("shrug", "🤷"), ("facepalm", "🤦"), ("face palm", "🤦"), ("angry", "😠"), ("rage", "😡"), ("nerd face", "🤓"), ("nerd", "🤓"),
    ("skull", "💀"), ("ghost", "👻"), ("robot", "🤖"), ("poop", "💩"), ("brain", "🧠"), ("bug", "🐛"),
    ("fire", "🔥"), ("rocket", "🚀"), ("party popper", "🎉"), ("party", "🎉"), ("tada", "🎉"), ("sparkles", "✨"), ("star", "⭐"),
    ("hundred", "💯"), ("one hundred", "💯"), ("100", "💯"), ("check mark", "✅"), ("checkmark", "✅"), ("check", "✅"),
    ("cross mark", "❌"), ("warning", "⚠️"), ("light bulb", "💡"), ("lightbulb", "💡"), ("trophy", "🏆"),
    ("coffee", "☕"), ("pizza", "🍕"), ("beer", "🍺"), ("cake", "🎂"), ("gift", "🎁"), ("money", "💰"),
    ("chart", "📈"), ("calendar", "📅"), ("memo", "📝"), ("pin", "📌"), ("pushpin", "📌"), ("link", "🔗"),
    ("lock", "🔒"), ("key", "🔑"), ("bell", "🔔"), ("sun", "☀️"), ("rainbow", "🌈"), ("dog", "🐶"), ("cat", "🐱"),
];

/// Words before a name that make it a mention, not a request ("the fire emoji").
const DETERMINERS: &[&str] = &["the", "a", "an", "this", "that", "these", "those", "my", "your", "his", "her", "their", "our", "its"];

fn bare(word: &str) -> String {
    word.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

fn emoji_line(line: &str) -> String {
    let words: Vec<&str> = line.split(' ').collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        let is_emoji = bare(word) == "emoji" && word.trim_start().chars().next().is_some_and(|c| c.is_alphabetic());
        if !is_emoji {
            out.push(word.to_string());
            i += 1;
            continue;
        }
        // The longest name ending right before "emoji", without punctuation inside it.
        let mut matched = None;
        for len in (1..=3).rev() {
            if out.len() < len {
                continue;
            }
            let name_words = &out[out.len() - len..];
            let clean = name_words.iter().enumerate().all(|(k, w)| {
                let core = w.trim_start_matches(|c: char| !c.is_alphanumeric());
                core.chars().all(|c| c.is_alphanumeric()) && (k == 0 || core.len() == w.len())
            });
            if !clean {
                continue;
            }
            let name = name_words.iter().map(|w| bare(w)).collect::<Vec<_>>().join(" ");
            if let Some((_, e)) = EMOJI.iter().find(|(n, _)| *n == name) {
                matched = Some((len, *e));
                break;
            }
        }
        match matched {
            Some((len, e)) if !out.get(out.len().wrapping_sub(len + 1)).is_some_and(|w| DETERMINERS.contains(&bare(w).as_str())) => {
                let first = &out[out.len() - len];
                let lead: String = first.chars().take_while(|c| !c.is_alphanumeric()).collect();
                out.truncate(out.len() - len);
                let trail: String = word.chars().skip_while(|c| c.is_alphanumeric()).collect();
                out.push(format!("{lead}{e}{trail}"));
            }
            _ => out.push(word.to_string()),
        }
        i += 1;
    }
    out.join(" ")
}

/// "Great job fire emoji" → "Great job 🔥". Only an emoji name directly
/// followed by the word "emoji" changes.
pub fn emoji_names(text: &str) -> String {
    if !text.to_lowercase().contains("emoji") {
        return text.to_string();
    }
    text.split('\n').map(emoji_line).collect::<Vec<_>>().join("\n")
}

/// A dictation ending in "press enter": the text without it. `None` when it
/// doesn't end that way.
pub fn strip_press_enter(text: &str) -> Option<String> {
    let t = text.trim_end().trim_end_matches(['.', '!', ',']);
    let lower = t.to_lowercase();
    if !lower.ends_with("press enter") {
        return None;
    }
    let head = &t[..t.len() - "press enter".len()];
    if head.chars().last().is_some_and(|c| c.is_alphanumeric()) {
        return None;
    }
    Some(head.trim_end().trim_end_matches([',', ';', ':']).trim_end().to_string())
}

/// Terminal apps collapse a long paste into a placeholder. Pastes above
/// these limits are sent in pieces.
pub const CHUNK_CHARS: usize = 500;
const CHUNK_NEWLINES: usize = 2;

/// Pieces that concatenate back to `text` exactly, each at most
/// `CHUNK_CHARS` characters and `CHUNK_NEWLINES` line breaks, split at spaces.
pub fn paste_chunks(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let newlines = chars.iter().filter(|c| **c == '\n').count();
    if chars.len() <= CHUNK_CHARS + 100 && newlines <= CHUNK_NEWLINES {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let mut end = start;
        let mut breaks = 0;
        let mut last_space = None;
        while end < chars.len() && end - start < CHUNK_CHARS {
            if chars[end] == '\n' {
                if breaks == CHUNK_NEWLINES {
                    break;
                }
                breaks += 1;
            }
            if chars[end].is_whitespace() {
                last_space = Some(end);
            }
            end += 1;
        }
        // Cut after the last space, unless the rest fits or there is none.
        if end < chars.len() && chars[end] != '\n' {
            if let Some(s) = last_space.filter(|s| *s > start) {
                end = s + 1;
            }
        }
        chunks.push(chars[start..end].iter().collect());
        start = end;
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_names_become_emoji() {
        assert_eq!(emoji_names("Great job fire emoji."), "Great job 🔥.");
        assert_eq!(emoji_names("Great job, fire emoji"), "Great job, 🔥");
        assert_eq!(emoji_names("Love it heart eyes emoji thumbs up emoji"), "Love it 😍 👍");
        assert_eq!(emoji_names("Fire emoji"), "🔥");
        assert_eq!(emoji_names("Sounds good\nthumbs up emoji"), "Sounds good\n👍");
        // S1-mini's real output for "great job on the launch everyone, fire emoji".
        assert_eq!(emoji_names("Great job on the launch, everyone! Fire emoji."), "Great job on the launch, everyone! 🔥.");
    }

    #[test]
    fn mentions_and_ordinary_words_stay() {
        assert_eq!(emoji_names("The party was fire."), "The party was fire.");
        assert_eq!(emoji_names("Send the fire emoji to Sam."), "Send the fire emoji to Sam.");
        assert_eq!(emoji_names("I like emoji."), "I like emoji.");
        assert_eq!(emoji_names("Add a spreadsheet emoji"), "Add a spreadsheet emoji");
        assert_eq!(emoji_names("Emojis are fun"), "Emojis are fun");
        assert_eq!(emoji_names("The deadline, fire emoji."), "The deadline, 🔥.");
    }

    #[test]
    fn press_enter() {
        assert_eq!(strip_press_enter("Run the tests, press enter."), Some("Run the tests".into()));
        assert_eq!(strip_press_enter("Press enter."), Some(String::new()));
        assert_eq!(strip_press_enter("Ship it. Press Enter"), Some("Ship it.".into()));
        assert_eq!(strip_press_enter("Then press enter to continue."), None);
        assert_eq!(strip_press_enter("Don't depress enter"), None);
    }

    #[test]
    fn short_pastes_stay_whole() {
        assert_eq!(paste_chunks("one two"), vec!["one two"]);
        assert_eq!(paste_chunks("a\nb\nc"), vec!["a\nb\nc"]);
    }

    #[test]
    fn long_pastes_split_at_spaces_and_rejoin_exactly() {
        let text = "word ".repeat(300);
        let chunks = paste_chunks(&text);
        assert!(chunks.len() > 2);
        assert_eq!(chunks.concat(), text);
        for c in &chunks {
            assert!(c.chars().count() <= CHUNK_CHARS);
            assert!(c.ends_with(' ') || c == chunks.last().unwrap());
        }
        let lines = "a\nb\nc\nd\ne\nf";
        let chunks = paste_chunks(lines);
        assert_eq!(chunks.concat(), lines);
        assert!(chunks.iter().all(|c| c.matches('\n').count() <= 2));
    }
}
