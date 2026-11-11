//! Fitting to the cursor in apps that don't report their caret: a dictation
//! that follows Quill's last paste, in the same window with no input since,
//! continues that paste.

use std::time::{Duration, Instant};

use super::insertion::Surroundings;

/// How long a paste stays the text before the caret.
pub const WINDOW: Duration = Duration::from_secs(120);
/// How much of the last paste stands in for the text before the caret.
const TAIL_CHARS: usize = 200;

/// Whether two captured targets are the same window of the same app.
pub trait SameWindow {
    fn same_window(&self, other: &Self) -> bool;
}

/// A window handle and exe name pair is the same window when both match.
/// A zero handle is no window.
pub fn same_window(hwnd: isize, exe: &str, other_hwnd: isize, other_exe: &str) -> bool {
    hwnd != 0 && hwnd == other_hwnd && exe.eq_ignore_ascii_case(other_exe)
}

/// The last transcript Quill delivered, and where it went.
#[derive(Debug, Clone)]
pub struct LastPaste<T> {
    pub text: String,
    pub target: T,
    /// When it was delivered, or last pasted again.
    pub at: Instant,
    /// The user's input count (keys and mouse buttons) right after the
    /// paste; `None` when it can't be counted.
    pub input: Option<u32>,
    /// False when it was only copied: the field doesn't end with it.
    pub pasted: bool,
    /// Enter was pressed after the paste.
    pub entered: bool,
}

/// The text before the caret, taken from the last paste, when a recording
/// starting now into `target` continues it. Only used when fitting is on
/// and the app gave no text of its own (`read`).
pub fn continuation<T: SameWindow>(
    last: Option<&LastPaste<T>>,
    target: &T,
    now: Instant,
    input: Option<u32>,
    fits_to_cursor: bool,
    read: Option<&Surroundings>,
) -> Option<Surroundings> {
    let last = last?;
    let no_input_since = matches!((last.input, input), (Some(a), Some(b)) if a == b);
    let fresh = now.checked_duration_since(last.at).is_some_and(|d| d < WINDOW);
    if !fits_to_cursor || read.is_some() || !last.pasted || !no_input_since || !fresh || !last.target.same_window(target) || last.text.is_empty() {
        return None;
    }
    let skip = last.text.chars().count().saturating_sub(TAIL_CHARS);
    let mut before: String = last.text.chars().skip(skip).collect();
    if last.entered {
        before.push('\n');
    }
    Some(Surroundings { before, ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::insertion::fit;

    #[derive(Debug, Clone)]
    struct Win(isize, &'static str);

    impl SameWindow for Win {
        fn same_window(&self, other: &Self) -> bool {
            same_window(self.0, self.1, other.0, other.1)
        }
    }

    fn last(text: &str, at: Instant) -> LastPaste<Win> {
        LastPaste { text: text.into(), target: Win(0x1234, "Notepad.exe"), at, input: Some(7), pasted: true, entered: false }
    }

    fn here() -> Win {
        Win(0x1234, "notepad.exe")
    }

    #[test]
    fn decision_table() {
        let t0 = Instant::now();
        let paste = last("We tested it", t0);
        let now = t0 + Duration::from_secs(5);
        let continues = |l: &LastPaste<Win>, w: &Win, at: Instant, input: Option<u32>, fits: bool, read: Option<&Surroundings>| {
            continuation(Some(l), w, at, input, fits, read).is_some()
        };

        assert!(continues(&paste, &here(), now, Some(7), true, None), "same window, no input, recent");
        assert!(continuation(None, &here(), now, Some(7), true, None).is_none(), "nothing pasted yet");
        assert!(!continues(&paste, &here(), now, Some(7), false, None), "fitting is off");
        assert!(!continues(&paste, &here(), now, Some(7), true, Some(&Surroundings::default())), "the app reported its caret");
        assert!(!continues(&paste, &here(), now, Some(8), true, None), "the user typed or clicked since");
        assert!(!continues(&paste, &here(), now, None, true, None), "input can't be counted now");
        assert!(!continues(&LastPaste { input: None, ..paste.clone() }, &here(), now, Some(7), true, None), "input wasn't counted at paste");
        assert!(!continues(&LastPaste { pasted: false, ..paste.clone() }, &here(), now, Some(7), true, None), "only copied");
        assert!(!continues(&paste, &Win(0x9999, "notepad.exe"), now, Some(7), true, None), "another window");
        assert!(!continues(&paste, &Win(0x1234, "code.exe"), now, Some(7), true, None), "another app in a reused handle");
        assert!(!continues(&LastPaste { target: Win(0, ""), ..paste.clone() }, &Win(0, ""), now, Some(7), true, None), "no window");
        assert!(continues(&paste, &here(), t0 + WINDOW - Duration::from_millis(1), Some(7), true, None), "just inside the window");
        assert!(!continues(&paste, &here(), t0 + WINDOW, Some(7), true, None), "too long ago");
        assert!(!continues(&LastPaste { at: t0 + Duration::from_secs(10), ..paste.clone() }, &here(), t0, Some(7), true, None), "clock order");
        assert!(!continues(&last("", t0), &here(), now, Some(7), true, None), "empty paste");
    }

    #[test]
    fn continues_the_sentence_or_starts_a_new_one() {
        let t0 = Instant::now();
        let now = t0 + Duration::from_secs(3);
        let around = |l: LastPaste<Win>| continuation(Some(&l), &here(), now, Some(7), true, None).unwrap();
        assert_eq!(fit("And then we ship", &around(last("We tested it", t0)), &[]), " and then we ship");
        assert_eq!(fit("And then we ship", &around(last("We tested it.", t0)), &[]), " And then we ship");
        assert_eq!(fit("Then we ship.", &around(LastPaste { entered: true, ..last("Sent it.", t0) }), &[]), "Then we ship.");
        assert_eq!(fit("I agree.", &around(last("Sounds good, and", t0)), &[]), " I agree.");
    }

    #[test]
    fn keeps_only_the_tail() {
        let t0 = Instant::now();
        let long = "é".repeat(500);
        let s = continuation(Some(&last(&long, t0)), &here(), t0, Some(7), true, None).unwrap();
        assert_eq!(s.before.chars().count(), 200);
        assert!(s.after.is_empty() && s.selection.is_empty());
    }
}
