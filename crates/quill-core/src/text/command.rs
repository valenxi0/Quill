//! Developer Mode: spoken shell input becomes a command (section 4.6).

#[derive(Debug, Clone, Copy)]
struct Symbol {
    text: &'static str,
    left: bool,
    right: bool,
}

const fn sym(text: &'static str, left: bool, right: bool) -> Symbol {
    Symbol { text, left, right }
}

/// Spoken phrase → symbol. Two-word phrases are tried before one-word ones.
const SYMBOLS: &[(&str, Symbol)] = &[
    ("dash dash", sym("--", false, true)),
    ("double dash", sym("--", false, true)),
    ("greater than", sym(">", false, false)),
    ("less than", sym("<", false, false)),
    ("at sign", sym("@", true, true)),
    ("dollar sign", sym("$", false, true)),
    ("question mark", sym("?", true, false)),
    ("exclamation mark", sym("!", true, false)),
    ("open paren", sym("(", false, true)),
    ("close paren", sym(")", true, false)),
    ("open bracket", sym("[", false, true)),
    ("close bracket", sym("]", true, false)),
    ("open brace", sym("{", false, true)),
    ("close brace", sym("}", true, false)),
    ("back slash", sym("\\", true, true)),
    ("backslash", sym("\\", true, true)),
    ("dash", sym("-", false, true)),
    ("hyphen", sym("-", false, true)),
    ("slash", sym("/", true, true)),
    ("dot", sym(".", true, true)),
    ("underscore", sym("_", true, true)),
    ("colon", sym(":", true, true)),
    ("semicolon", sym(";", true, false)),
    ("equals", sym("=", true, true)),
    ("tilde", sym("~", false, true)),
    ("pipe", sym("|", false, false)),
    ("ampersand", sym("&", false, false)),
    ("star", sym("*", false, true)),
    ("asterisk", sym("*", false, true)),
    ("comma", sym(",", true, false)),
];

#[derive(Debug)]
enum Item {
    Word(String),
    Sym(Symbol),
    Space,
}

fn clean_word(w: &str) -> &str {
    w.trim_end_matches([',', '.', '?', '!', ';']).trim_start_matches(',')
}

fn capitalize(w: &str) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// "Git" → "git", but "README" and "git" stay.
fn is_title_case(w: &str) -> bool {
    let mut c = w.chars();
    match c.next() {
        Some(f) if f.is_uppercase() => {
            let rest: Vec<char> = c.collect();
            !rest.is_empty() && rest.iter().all(|c| !c.is_alphabetic() || c.is_lowercase())
        }
        _ => false,
    }
}

pub fn format_command(text: &str) -> String {
    let words: Vec<&str> =
        text.split_whitespace().map(clean_word).filter(|w| !w.is_empty()).collect();
    let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();

    let mut items = Vec::new();
    let mut in_double = false;
    let mut in_single = false;
    let mut capital_next = false;
    let mut i = 0;
    while i < words.len() {
        let two = lower.get(i + 1).map(|n| format!("{} {}", lower[i], n));
        let one = lower[i].as_str();

        // Quotes first; they carry state.
        let quote = match (two.as_deref(), one) {
            (Some("open quote"), _) => Some((2, '"', true)),
            (Some("close quote") | Some("end quote"), _) => Some((2, '"', false)),
            (Some("single quote"), _) => Some((2, '\'', !in_single)),
            (_, "unquote") => Some((1, '"', false)),
            (_, "quote") => Some((1, '"', !in_double)),
            _ => None,
        };
        if let Some((len, ch, open)) = quote {
            if ch == '"' {
                in_double = open;
            } else {
                in_single = open;
            }
            let text = if ch == '"' { "\"" } else { "'" };
            items.push(Item::Sym(Symbol { text, left: !open, right: open }));
            i += len;
            continue;
        }

        if one == "space" {
            items.push(Item::Space);
            i += 1;
            continue;
        }
        if one == "capital" || one == "uppercase" {
            capital_next = true;
            i += 1;
            continue;
        }

        let found = two
            .as_deref()
            .and_then(|t| SYMBOLS.iter().find(|(p, _)| *p == t).map(|(_, s)| (2, *s)))
            .or_else(|| SYMBOLS.iter().find(|(p, _)| *p == one).map(|(_, s)| (1, *s)));
        if let Some((len, s)) = found {
            items.push(Item::Sym(s));
            i += len;
            continue;
        }

        let mut w = words[i].to_string();
        if capital_next {
            w = capitalize(&w);
            capital_next = false;
        } else if w.chars().count() == 1
            && matches!(items.last(), Some(Item::Sym(s)) if s.text == "-" || s.text == "--")
        {
            w = w.to_lowercase();
        }
        items.push(Item::Word(w));
        i += 1;
    }

    // Emit, tracking whether the current chunk (since the last gap) is a flag.
    let mut out = String::new();
    let mut chunk_start = 0;
    let mut glue_next = false;
    let mut forced_gap = false;
    for item in items {
        let (text, left, right) = match &item {
            Item::Space => {
                forced_gap = true;
                glue_next = false;
                continue;
            }
            Item::Word(w) => (w.as_str(), false, false),
            Item::Sym(s) => {
                let chunk = &out[chunk_start..];
                let in_flag = s.text == "-" && chunk.starts_with("--") && chunk.len() > 2
                    && chunk.chars().last().is_some_and(|c| c.is_alphanumeric());
                (s.text, s.left || in_flag, s.right)
            }
        };
        let glued = !out.is_empty() && !forced_gap && (glue_next || left);
        if !out.is_empty() && !glued {
            out.push(' ');
            chunk_start = out.len();
        }
        out.push_str(text);
        glue_next = right;
        forced_gap = false;
    }

    // Lowercase a Capitalized command name.
    if let Some(first) = out.split(' ').next() {
        if is_title_case(first) {
            let lowered = first.to_lowercase();
            out.replace_range(..first.len(), &lowered);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_examples() {
        let cases = [
            ("git commit dash m quote fix the build quote", r#"git commit -m "fix the build""#),
            ("npm install dash dash save dash dev", "npm install --save-dev"),
            ("cd tilde slash projects", "cd ~/projects"),
        ];
        for (said, typed) in cases {
            assert_eq!(format_command(said), typed, "{said}");
        }
    }

    #[test]
    fn recognizer_punctuation_and_case() {
        assert_eq!(format_command("Git status."), "git status");
        assert_eq!(format_command("README dot md"), "README.md");
        assert_eq!(format_command("Git commit dash M, quote, wip, quote."), r#"git commit -m "wip""#);
    }

    #[test]
    fn dash_n_fifty() {
        assert_eq!(format_command("head dash n fifty"), "head -n fifty");
        assert_eq!(format_command("head dash N 50"), "head -n 50");
    }

    #[test]
    fn flags_join_inside() {
        assert_eq!(format_command("git push dash dash no dash verify"), "git push --no-verify");
        assert_eq!(format_command("ls dash la"), "ls -la");
    }

    #[test]
    fn quotes() {
        assert_eq!(format_command("echo open quote hi there close quote"), r#"echo "hi there""#);
        assert_eq!(format_command("echo single quote hi single quote"), "echo 'hi'");
        assert_eq!(format_command("grep quote a b end quote file"), r#"grep "a b" file"#);
        assert_eq!(format_command("echo quote x unquote"), r#"echo "x""#);
    }

    #[test]
    fn capital_and_space() {
        assert_eq!(format_command("git commit dash m quote capital fix quote"), r#"git commit -m "Fix""#);
        assert_eq!(format_command("echo hi space greater than out dot txt"), "echo hi > out.txt");
        assert_eq!(format_command("cd space dot dot slash src"), "cd ../src");
        assert_eq!(format_command("ls dash la dash h"), "ls -la -h");
    }

    #[test]
    fn symbols() {
        assert_eq!(format_command("ls pipe grep foo"), "ls | grep foo");
        assert_eq!(format_command("echo dollar sign home"), "echo $home");
        assert_eq!(format_command("git log open paren x close paren"), "git log (x)");
        assert_eq!(format_command("ssh me at sign host"), "ssh me@host");
        assert_eq!(format_command("rm star dot log"), "rm *.log");
    }
}
