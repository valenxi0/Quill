//! Numbers as figures, English only (section 4.7).
//!
//! Works span by span: only runs of number words are converted, so the rest
//! of the sentence can never be rewritten. Never run ITN on a whole sentence.

use super::tokens::{detokenize, tokenize, Token};

/// Inverse text normalization of one spoken number expression.
pub trait Itn: Send + Sync {
    fn normalize(&self, spoken: &str) -> String;
}

/// NeMo ITN through `text-processing-rs`.
#[cfg(feature = "itn")]
pub struct NemoItn;

#[cfg(feature = "itn")]
impl Itn for NemoItn {
    fn normalize(&self, spoken: &str) -> String {
        std::panic::catch_unwind(|| text_processing_rs::normalize(spoken))
            .unwrap_or_else(|_| spoken.to_string())
    }
}

/// The ITN backend compiled into this build, if any.
pub fn default_itn() -> Option<Box<dyn Itn>> {
    #[cfg(feature = "itn")]
    {
        Some(Box::new(NemoItn))
    }
    #[cfg(not(feature = "itn"))]
    {
        None
    }
}

pub const ITN_UNAVAILABLE: &str = "The number formatter is not available in this build.";

// ---------------------------------------------------------------------------
// Word tables

fn digit(w: &str) -> Option<u64> {
    Some(match w {
        "zero" => 0,
        "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        _ => return None,
    })
}

/// A single spoken digit, where "oh" means zero.
fn spoken_digit(w: &str) -> Option<u64> {
    if w == "oh" {
        Some(0)
    } else {
        digit(w)
    }
}

fn teen(w: &str) -> Option<u64> {
    Some(match w {
        "ten" => 10,
        "eleven" => 11,
        "twelve" => 12,
        "thirteen" => 13,
        "fourteen" => 14,
        "fifteen" => 15,
        "sixteen" => 16,
        "seventeen" => 17,
        "eighteen" => 18,
        "nineteen" => 19,
        _ => return None,
    })
}

fn tens(w: &str) -> Option<u64> {
    Some(match w {
        "twenty" => 20,
        "thirty" => 30,
        "forty" => 40,
        "fifty" => 50,
        "sixty" => 60,
        "seventy" => 70,
        "eighty" => 80,
        "ninety" => 90,
        _ => return None,
    })
}

fn scale(w: &str) -> Option<u64> {
    Some(match w {
        "hundred" => 100,
        "thousand" => 1_000,
        "million" => 1_000_000,
        "billion" => 1_000_000_000,
        _ => return None,
    })
}

fn ordinal(w: &str) -> Option<u64> {
    Some(match w {
        "first" => 1,
        "second" => 2,
        "third" => 3,
        "fourth" => 4,
        "fifth" => 5,
        "sixth" => 6,
        "seventh" => 7,
        "eighth" => 8,
        "ninth" => 9,
        "tenth" => 10,
        "eleventh" => 11,
        "twelfth" => 12,
        "thirteenth" => 13,
        "fourteenth" => 14,
        "fifteenth" => 15,
        "sixteenth" => 16,
        "seventeenth" => 17,
        "eighteenth" => 18,
        "nineteenth" => 19,
        "twentieth" => 20,
        "thirtieth" => 30,
        _ => return None,
    })
}

fn is_number_word(w: &str) -> bool {
    digit(w).is_some() || teen(w).is_some() || tens(w).is_some() || scale(w).is_some()
}

fn month(w: &str) -> Option<&'static str> {
    const MONTHS: [&str; 12] = [
        "January", "February", "March", "April", "May", "June", "July", "August", "September",
        "October", "November", "December",
    ];
    MONTHS.iter().copied().find(|m| m.eq_ignore_ascii_case(w))
}

enum Unit {
    Dollar,
    Cent,
    Percent,
    Euro,
    Pound,
}

fn unit(w: &str) -> Option<Unit> {
    Some(match w {
        "dollar" | "dollars" => Unit::Dollar,
        "cent" | "cents" => Unit::Cent,
        "percent" => Unit::Percent,
        "euro" | "euros" => Unit::Euro,
        "pound" | "pounds" => Unit::Pound,
        _ => return None,
    })
}

fn meridiem(w: &str) -> Option<&'static str> {
    match w {
        "am" | "a.m" => Some("AM"),
        "pm" | "p.m" => Some("PM"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Parsers over lowercase words

/// A strict cardinal: "twenty five", "three hundred and two", "two thousand".
/// Rejects sequences that aren't one number ("one two", "twenty twenty").
pub fn parse_cardinal(words: &[&str]) -> Option<u64> {
    if words.is_empty() {
        return None;
    }
    let (mut total, mut current) = (0u64, 0u64);
    let mut last_scale = u64::MAX;
    // What the previous word was: 0 start/scale, 1 digit, 2 teen, 3 tens, 4 and
    let mut prev = 0u8;
    for (i, w) in words.iter().enumerate() {
        if let Some(d) = digit(w) {
            if prev == 1 || prev == 2 || (d == 0 && (i > 0 || words.len() > 1)) {
                return None;
            }
            current += d;
            prev = 1;
        } else if let Some(t) = teen(w) {
            if prev == 1 || prev == 2 || prev == 3 || current % 100 != 0 {
                return None;
            }
            current += t;
            prev = 2;
        } else if let Some(t) = tens(w) {
            if prev == 1 || prev == 2 || prev == 3 || current % 100 != 0 {
                return None;
            }
            current += t;
            prev = 3;
        } else if *w == "hundred" {
            if !(1..=9).contains(&current) || prev == 4 {
                return None;
            }
            current *= 100;
            prev = 0;
        } else if let Some(s) = scale(w) {
            if current == 0 || current >= 1000 || s >= last_scale || prev == 4 {
                return None;
            }
            total += current * s;
            current = 0;
            last_scale = s;
            prev = 0;
        } else if *w == "and" {
            let after_scale = i > 0 && scale(words[i - 1]).is_some();
            if !after_scale || i + 1 == words.len() {
                return None;
            }
            prev = 4;
        } else {
            return None;
        }
    }
    if prev == 4 {
        return None;
    }
    Some(total + current)
}

/// A two-digit group used in years and minutes: "twenty six", "fifteen".
fn parse_pair(words: &[&str]) -> Option<u64> {
    match words {
        [a] => teen(a).or_else(|| tens(a)),
        [a, b] => Some(tens(a)? + digit(b).filter(|d| *d > 0)?),
        _ => None,
    }
}

/// A 4-digit year from the start of `words`: "twenty twenty six",
/// "nineteen oh five", "nineteen hundred", "two thousand twenty six".
/// Returns the year and the words consumed, preferring the longest reading.
fn parse_year(words: &[&str]) -> Option<(u64, usize)> {
    for n in (2..=words.len().min(5)).rev() {
        let w = &words[..n];
        if w.contains(&"thousand") {
            if let Some(v) = parse_cardinal(w).filter(|v| (1000..=2999).contains(v)) {
                return Some((v, n));
            }
            continue;
        }
        for split in 1..n {
            let (a, b) = w.split_at(split);
            let Some(hi) = parse_pair(a).filter(|v| (10..=29).contains(v)) else { continue };
            let lo = match b {
                ["hundred"] => Some(0),
                ["oh", d] => digit(d).filter(|d| *d > 0),
                _ => parse_pair(b),
            };
            if let Some(lo) = lo {
                return Some((hi * 100 + lo, n));
            }
        }
    }
    None
}

/// A day of the month from the start of `words`: "third", "twenty first",
/// "twenty one", "three". Returns the day, words consumed, and whether it was
/// spoken as an ordinal.
fn parse_day(words: &[&str]) -> Option<(u64, usize, bool)> {
    if let [t, o, ..] = words {
        if let (Some(t), Some(o)) = (tens(t), ordinal(o)) {
            if o < 10 && t + o <= 31 {
                return Some((t + o, 2, true));
            }
        }
    }
    if let Some(o) = words.first().and_then(|w| ordinal(w)) {
        return Some((o, 1, true));
    }
    for n in [2, 1] {
        if words.len() >= n {
            if let Some(v) = parse_cardinal(&words[..n]).filter(|v| (1..=31).contains(v)) {
                return Some((v, n, false));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// The formatter

struct Ctx<'a> {
    toks: &'a [Token],
    words: Vec<String>,
    terminal: bool,
    itn: &'a dyn Itn,
}

impl Ctx<'_> {
    fn word(&self, i: usize) -> &str {
        self.words.get(i).map(String::as_str).unwrap_or("")
    }

    /// Token `k` can continue a span that already covers `k - 1`: spans stop
    /// at any trailing punctuation, and never swallow leading punctuation.
    fn can_extend(&self, k: usize) -> bool {
        k < self.toks.len() && self.toks[k].lead.is_empty() && self.toks[k - 1].trail.is_empty()
    }

    /// Words from `i` onward as long as the span may extend, up to `max`.
    fn run(&self, i: usize, max: usize) -> Vec<&str> {
        let mut out = vec![self.word(i)];
        let mut k = i + 1;
        while out.len() < max && self.can_extend(k) {
            out.push(self.word(k));
            k += 1;
        }
        out
    }

    fn itn(&self, spoken: &str) -> Option<String> {
        let out = self.itn.normalize(spoken).replace(" %", "%");
        accept_itn(spoken, &out).then_some(out)
    }

    fn try_date(&self, i: usize) -> Option<(usize, String)> {
        let m = month(&self.toks[i].core)?;
        if !self.can_extend(i + 1) {
            return None;
        }
        let verb_like = m == "May" || m == "March";
        if verb_like && !self.toks[i].core.starts_with(|c: char| c.is_uppercase()) {
            return None;
        }
        let rest = self.run(i + 1, 6);
        let (day, used, was_ordinal) = parse_day(&rest)?;
        if verb_like && !was_ordinal {
            return None;
        }
        let mut end = i + 1 + used;
        let mut out = format!("{m} {day}");
        if self.can_extend(end) {
            let tail = self.run(end, 5);
            if let Some((year, n)) = parse_year(&tail) {
                out = format!("{out}, {year}");
                end += n;
            }
        }
        Some((end, out))
    }

    fn try_time(&self, i: usize) -> Option<(usize, String)> {
        let words = self.run(i, 4);
        let hour = parse_cardinal(&words[..1]).filter(|h| (1..=12).contains(h))?;
        // Minutes are 0, 1, or 2 words; the meridiem must follow directly.
        for n in [2usize, 1, 0] {
            if words.len() < 1 + n + 1 {
                continue;
            }
            let mins = &words[1..1 + n];
            let minute = match mins {
                [] => Some(0),
                ["oh", d] => digit(d).filter(|d| *d > 0),
                _ => parse_pair(mins).filter(|m| *m < 60),
            };
            let (Some(minute), Some(ampm)) = (minute, meridiem(words[1 + n])) else { continue };
            let out = if minute == 0 {
                format!("{hour} {ampm}")
            } else {
                format!("{hour}:{minute:02} {ampm}")
            };
            return Some((i + 2 + n, out));
        }
        None
    }

    /// The longest valid cardinal (with an optional "point" decimal) at `i`.
    fn cardinal_span(&self, i: usize) -> Option<(usize, u64, Option<String>)> {
        let words = self.run(i, 12);
        if !(digit(words[0]).is_some() || teen(words[0]).is_some() || tens(words[0]).is_some()) {
            return None;
        }
        let limit = words.iter().take_while(|w| is_number_word(w) || **w == "and").count();
        let (n, value) = (1..=limit).rev().find_map(|n| parse_cardinal(&words[..n]).map(|v| (n, v)))?;
        // "point" then digits.
        if words.get(n) == Some(&"point") && words.get(n + 1).and_then(|w| spoken_digit(w)).is_some() {
            let decimals: String = words[n + 1..]
                .iter()
                .map_while(|w| spoken_digit(w))
                .map(|d| char::from(b'0' + d as u8))
                .collect();
            let used = n + 1 + decimals.len();
            return Some((i + used, value, Some(decimals)));
        }
        Some((i + n, value, None))
    }

    fn try_money(&self, i: usize) -> Option<(usize, String)> {
        let (end, value, decimals) = self.cardinal_span(i)?;
        if !self.can_extend(end) {
            return None;
        }
        let u = unit(self.word(end))?;
        let mut stop = end + 1;
        let mut cents = None;
        if matches!(u, Unit::Dollar | Unit::Euro | Unit::Pound)
            && decimals.is_none()
            && self.can_extend(stop)
            && self.word(stop) == "and"
            && self.can_extend(stop + 1)
        {
            if let Some((cend, c, None)) = self.cardinal_span(stop + 1) {
                if c < 100 && self.can_extend(cend) && unit(self.word(cend)).is_some_and(|u| matches!(u, Unit::Cent)) {
                    cents = Some(c);
                    stop = cend + 1;
                }
            }
        }
        let spoken = self.words[i..stop].join(" ");
        if let Some(out) = self.itn(&spoken) {
            return Some((stop, out));
        }
        // ITN declined; format it ourselves.
        let amount = match (&decimals, cents) {
            (Some(d), _) => format!("{value}.{d}"),
            (None, Some(c)) => format!("{value}.{c:02}"),
            (None, None) => value.to_string(),
        };
        let out = match u {
            Unit::Dollar => format!("${amount}"),
            Unit::Euro => format!("€{amount}"),
            Unit::Pound => format!("£{amount}"),
            Unit::Percent => format!("{amount}%"),
            Unit::Cent => format!("{amount}¢"),
        };
        Some((stop, out))
    }

    fn try_phone(&self, i: usize) -> Option<(usize, String)> {
        if self.word(i) == "oh" {
            return None;
        }
        let digits: String = self
            .run(i, 16)
            .iter()
            .map_while(|w| spoken_digit(w))
            .map(|d| char::from(b'0' + d as u8))
            .collect();
        if digits.len() < 3 {
            return None;
        }
        let out = match digits.len() {
            7 => format!("{}-{}", &digits[..3], &digits[3..]),
            10 => format!("{}-{}-{}", &digits[..3], &digits[3..6], &digits[6..]),
            _ => digits.clone(),
        };
        Some((i + digits.len(), out))
    }

    fn try_generic(&self, i: usize) -> Option<(usize, String)> {
        // A year spoken in pairs: "twenty twenty six".
        let words = self.run(i, 5);
        if let Some((year, n)) = parse_year(&words) {
            if parse_cardinal(&words[..n]).is_none() {
                return Some((i + n, year.to_string()));
            }
        }
        let (mut end, value, decimals) = self.cardinal_span(i)?;
        // An ordinal right after a tens word: "twenty first".
        let mut ordinal_follows = false;
        if decimals.is_none() && tens(self.word(end - 1)).is_some() && self.can_extend(end) {
            if ordinal(self.word(end)).is_some_and(|o| o < 10) {
                end += 1;
                ordinal_follows = true;
            }
        }
        let single = end == i + 1;
        if single && value < 10 {
            // A lone digit stays a word in prose ("two kids").
            return self.terminal.then(|| (end, value.to_string()));
        }
        let spoken = self.words[i..end].join(" ");
        match self.itn(&spoken) {
            Some(out) => Some((end, out)),
            None if self.terminal && !ordinal_follows => {
                let out = match decimals {
                    Some(d) => format!("{value}.{d}"),
                    None => value.to_string(),
                };
                Some((end, out))
            }
            None => None,
        }
    }
}

/// Reject ITN output that is unchanged, has no digit, or has letters other
/// than ordinal suffixes.
fn accept_itn(input: &str, out: &str) -> bool {
    let out = out.trim();
    out != input.trim()
        && out.chars().any(|c| c.is_ascii_digit())
        && out.chars().filter(|c| c.is_alphabetic()).all(|c| "stndrh".contains(c))
}

/// "twenty-five" → "twenty five" (also "twenty-first").
fn split_hyphenated_tens(tokens: Vec<Token>) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len());
    for t in tokens {
        let lower = t.core.to_lowercase();
        if let Some((a, b)) = lower.split_once('-') {
            if tens(a).is_some() && (digit(b).is_some_and(|d| d > 0) || ordinal(b).is_some_and(|o| o < 10)) {
                let (ca, cb) = t.core.split_at(a.len());
                out.push(Token { sep: t.sep, lead: t.lead, core: ca.to_string(), trail: String::new() });
                out.push(Token { sep: " ".into(), lead: String::new(), core: cb[1..].to_string(), trail: t.trail });
                continue;
            }
        }
        out.push(t);
    }
    out
}

/// Convert number spans in English text to figures. In a terminal, every
/// number becomes digits.
pub fn format_numbers(text: &str, terminal: bool, itn: &dyn Itn) -> String {
    let toks = split_hyphenated_tens(tokenize(text));
    let words = toks.iter().map(|t| t.core.to_lowercase()).collect();
    let ctx = Ctx { toks: &toks, words, terminal, itn };
    let mut out: Vec<Token> = Vec::with_capacity(toks.len());
    let mut i = 0;
    while i < toks.len() {
        let hit = ctx
            .try_date(i)
            .or_else(|| ctx.try_time(i))
            .or_else(|| ctx.try_money(i))
            .or_else(|| ctx.try_phone(i))
            .or_else(|| ctx.try_generic(i));
        match hit {
            Some((end, core)) if end > i => {
                out.push(Token {
                    sep: toks[i].sep.clone(),
                    lead: toks[i].lead.clone(),
                    core,
                    trail: toks[end - 1].trail.clone(),
                });
                i = end;
            }
            _ => {
                out.push(toks[i].clone());
                i += 1;
            }
        }
    }
    detokenize(&out)
}

#[cfg(all(test, feature = "itn"))]
mod tests {
    use super::*;

    fn fmt(s: &str) -> String {
        format_numbers(s, false, &NemoItn)
    }

    #[test]
    fn prose_is_untouched() {
        assert_eq!(fmt("one of the best"), "one of the best");
        assert_eq!(fmt("I have two kids."), "I have two kids.");
        assert_eq!(fmt("they may one day"), "they may one day");
        assert_eq!(fmt("oh no"), "oh no");
    }

    #[test]
    fn spans() {
        assert_eq!(fmt("twenty five people"), "25 people");
        assert_eq!(fmt("twenty-five people"), "25 people");
        assert_eq!(fmt("about three hundred and twelve rows"), "about 312 rows");
        assert_eq!(fmt("we had fifty, then sixty."), "we had 50, then 60.");
    }

    #[test]
    fn money_and_units() {
        assert_eq!(fmt("twenty five dollars and fifty cents"), "$25.50");
        assert_eq!(fmt("Twenty five dollars on March third"), "$25 on March 3");
        assert_eq!(fmt("up five percent"), "up 5%");
    }

    #[test]
    fn times() {
        assert_eq!(fmt("at four thirty pm"), "at 4:30 PM");
        assert_eq!(fmt("at five p.m."), "at 5 PM.");
        assert_eq!(fmt("by nine oh five am"), "by 9:05 AM");
    }

    #[test]
    fn dates() {
        assert_eq!(fmt("on March third"), "on March 3");
        assert_eq!(fmt("May first works"), "May 1 works");
        assert_eq!(fmt("on March twenty first twenty twenty six"), "on March 21, 2026");
        assert_eq!(fmt("June five"), "June 5");
        assert_eq!(fmt("March on"), "March on");
    }

    #[test]
    fn phones() {
        assert_eq!(fmt("call five five five oh one nine eight"), "call 555-0198");
        assert_eq!(fmt("four one five five five five oh one nine eight"), "415-555-0198");
        assert_eq!(fmt("code one two three four"), "code 1234");
    }

    #[test]
    fn terminal_lone_digits() {
        assert_eq!(format_numbers("head -n fifty", true, &NemoItn), "head -n 50");
        assert_eq!(format_numbers("tail -n one", true, &NemoItn), "tail -n 1");
    }

    #[test]
    fn cardinal_parser() {
        let p = |s: &str| parse_cardinal(&s.split(' ').collect::<Vec<_>>());
        assert_eq!(p("two thousand and twenty six"), Some(2026));
        assert_eq!(p("one two"), None);
        assert_eq!(p("twenty twenty"), None);
        assert_eq!(p("nine hundred ninety nine"), Some(999));
        assert_eq!(p("hundred"), None);
    }
}
