//! Fitting to the cursor in apps that don't report their caret: a dictation
//! that follows Quill's last paste, in the same window with no typing since,
//! continues that paste.

use std::time::{Duration, Instant};

use super::insertion::Surroundings;

/// How long a paste stays the text before the caret.
pub const WINDOW: Duration = Duration::from_secs(120);
/// How much of the last paste stands in for the text before the caret.
const TAIL_CHARS: usize = 200;

/// A window as Quill saw it: its handle and the app's exe name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub hwnd: isize,
    pub exe: String,
}

impl Window {
    fn same(&self, other: &Window) -> bool {
        self.hwnd != 0 && self.hwnd == other.hwnd && self.exe.eq_ignore_ascii_case(&other.exe)
    }
}

/// Quill's last paste. `typed` is the count of keys the user pressed, read
/// right after the paste; `None` when keys can't be counted.
#[derive(Debug, Clone)]
pub struct LastPaste {
    pub text: String,
    pub window: Window,
    pub at: Instant,
    pub typed: Option<u32>,
}

/// A recording starting now.
#[derive(Debug, Clone)]
pub struct Start {
    pub window: Window,
    pub at: Instant,
    pub typed: Option<u32>,
}

/// The text before the caret, taken from the last paste, when this
/// dictation continues it. Only used when fitting is on and the app gave no
/// text of its own. Mouse clicks are not seen, so a click elsewhere in the
/// same window within the window of time still counts as a continuation.
pub fn continuation(last: Option<&LastPaste>, start: &Start, fits_to_cursor: bool, read: Option<&Surroundings>) -> Option<Surroundings> {
    let last = last?;
    let typed_since = match (last.typed, start.typed) {
        (Some(a), Some(b)) => a != b,
        _ => true,
    };
    let fresh = start.at.checked_duration_since(last.at).is_some_and(|d| d < WINDOW);
    if !fits_to_cursor || read.is_some() || typed_since || !fresh || !last.window.same(&start.window) || last.text.is_empty() {
        return None;
    }
    let skip = last.text.chars().count().saturating_sub(TAIL_CHARS);
    Some(Surroundings { before: last.text.chars().skip(skip).collect(), ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::insertion::fit;

    fn window() -> Window {
        Window { hwnd: 0x1234, exe: "Notepad.exe".into() }
    }

    fn last(text: &str, at: Instant) -> LastPaste {
        LastPaste { text: text.into(), window: window(), at, typed: Some(7) }
    }

    fn start(at: Instant) -> Start {
        Start { window: Window { hwnd: 0x1234, exe: "notepad.exe".into() }, at, typed: Some(7) }
    }

    #[test]
    fn decision_table() {
        let t0 = Instant::now();
        let paste = last("We tested it", t0);
        let now = start(t0 + Duration::from_secs(5));
        let continues = |l: Option<&LastPaste>, s: &Start, fits: bool, read: Option<&Surroundings>| continuation(l, s, fits, read).is_some();

        assert!(continues(Some(&paste), &now, true, None), "same window, no typing, recent");
        assert!(!continues(None, &now, true, None), "nothing pasted yet");
        assert!(!continues(Some(&paste), &now, false, None), "fitting is off");
        assert!(!continues(Some(&paste), &now, true, Some(&Surroundings::default())), "the app reported its caret");
        assert!(!continues(Some(&paste), &Start { typed: Some(8), ..now.clone() }, true, None), "the user typed since");
        assert!(!continues(Some(&paste), &Start { typed: None, ..now.clone() }, true, None), "keys can't be counted");
        assert!(!continues(Some(&LastPaste { typed: None, ..paste.clone() }), &now, true, None), "keys weren't counted at paste");
        assert!(!continues(Some(&paste), &Start { window: Window { hwnd: 0x9999, exe: "notepad.exe".into() }, ..now.clone() }, true, None), "another window");
        assert!(!continues(Some(&paste), &Start { window: Window { hwnd: 0x1234, exe: "code.exe".into() }, ..now.clone() }, true, None), "another process in a reused handle");
        assert!(!continues(Some(&paste), &Start { window: Window { hwnd: 0, exe: String::new() }, ..now.clone() }, true, None), "no window");
        assert!(continues(Some(&paste), &start(t0 + WINDOW - Duration::from_millis(1)), true, None), "just inside the window");
        assert!(!continues(Some(&paste), &start(t0 + WINDOW), true, None), "too long ago");
        assert!(!continues(Some(&LastPaste { at: t0 + Duration::from_secs(10), ..paste.clone() }), &start(t0), true, None), "clock order");
        assert!(!continues(Some(&last("", t0)), &now, true, None), "empty paste");
    }

    #[test]
    fn continues_the_sentence_or_starts_a_new_one() {
        let t0 = Instant::now();
        let now = start(t0 + Duration::from_secs(3));
        let around = |text: &str| continuation(Some(&last(text, t0)), &now, true, None).unwrap();
        assert_eq!(fit("And then we ship", &around("We tested it"), &[]), " and then we ship");
        assert_eq!(fit("And then we ship", &around("We tested it."), &[]), " And then we ship");
        assert_eq!(fit("Then we ship.", &around("Done.\n"), &[]), "Then we ship.");
        assert_eq!(fit("I agree.", &around("Sounds good, and"), &[]), " I agree.");
    }

    #[test]
    fn keeps_only_the_tail() {
        let t0 = Instant::now();
        let long = "é".repeat(500);
        let s = continuation(Some(&last(&long, t0)), &start(t0), true, None).unwrap();
        assert_eq!(s.before.chars().count(), 200);
        assert!(s.after.is_empty() && s.selection.is_empty());
    }
}
