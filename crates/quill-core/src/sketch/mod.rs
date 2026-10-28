//! Sketch: talk while marking up the screen, then send the screenshots with
//! what was said at each one. The idea comes from Scrawl by Kit Langton
//! (github.com/kitlangton/scrawl, MIT); this follows Nano's version.
//!
//! Points are physical pixels from the captured monitor's top-left, y down.
//! Sample indexes are 16 kHz mono.

pub mod render;

use std::ops::Range;
use std::sync::Arc;

use crate::segments::{find_pause, SAMPLE_RATE};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// One ink stroke.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub points: Vec<Point>,
    /// When the stroke began: its time, and where it falls in the transcript.
    pub start_sample: usize,
    pub number: usize,
}

/// Where a moment happened: the app in front, its window title, and for a
/// click, the element under the pointer.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Place {
    pub app: Option<String>,
    pub window: Option<String>,
    pub element: Option<String>,
}

impl Place {
    /// "in Chrome “GitHub”", or None when nothing is known.
    pub fn description(&self) -> Option<String> {
        let app = self.app.as_deref()?;
        let window = self.window.as_deref().map(str::trim).unwrap_or("");
        if window.is_empty() || window == app {
            return Some(format!("in {app}"));
        }
        let window = if window.chars().count() > 60 {
            format!("{}…", window.chars().take(59).collect::<String>())
        } else {
            window.to_string()
        };
        Some(format!("in {app} “{window}”"))
    }
}

/// Why a screenshot was taken, from least to most telling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MomentKind {
    /// The screen when recording began.
    Start,
    /// The screen changed and then held still.
    Change,
    /// A click; the screenshot is what was clicked.
    Click,
    /// One or more marks drawn on the same screen.
    Marks,
}

/// RGBA8, opaque.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A screenshot worth keeping, and why.
#[derive(Debug, Clone)]
pub struct Moment {
    pub kind: MomentKind,
    pub sample: usize,
    pub image: Arc<Image>,
    pub strokes: Vec<Stroke>,
    /// Where a click landed.
    pub click: Option<Point>,
    pub place: Place,
}

impl Moment {
    pub fn tag(&self) -> Option<String> {
        match self.kind {
            MomentKind::Click => Some(self.place.element.as_ref().map_or("[clicked]".into(), |e| format!("[clicked {e}]"))),
            MomentKind::Change => Some("[screen changed]".into()),
            MomentKind::Start | MomentKind::Marks => None,
        }
    }
}

/// One place in the speech: where a screenshot begins, or a numbered mark
/// within one.
#[derive(Debug, Clone, PartialEq)]
pub struct Anchor {
    pub sample: usize,
    pub frame: usize,
    pub label: Option<String>,
}

/// Where the recording is cut so each anchor lands between words. `marks`
/// are indexes into the anchors.
#[derive(Debug, Clone, PartialEq)]
pub struct Cut {
    pub sample: usize,
    pub marks: Vec<usize>,
}

/// What a line of the text says about its screenshot.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameLine {
    pub time: f64,
    pub tag: Option<String>,
    pub place: Option<String>,
}

/// Groups of marks kept per session.
pub const MAXIMUM_FRAMES: usize = 8;
/// Shorter paths are a key press while typing, not a mark.
pub const MINIMUM_STROKE_LENGTH: f32 = 12.0;

pub fn stroke_length(points: &[Point]) -> f32 {
    points.windows(2).map(|w| (w[1].x - w[0].x).hypot(w[1].y - w[0].y)).sum()
}

// Comparing screens

pub const GRID_COLUMNS: usize = 32;
pub const GRID_ROWS: usize = 20;

/// Brightness of each cell in a coarse grid, 0...1, from 4 × 4 samples per
/// cell. `channels` gives the byte offsets of red, green and blue.
fn grid_of(pixels: &[u8], width: usize, height: usize, stride: usize, channels: [usize; 3]) -> Vec<f32> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let mut grid = vec![0.0; GRID_COLUMNS * GRID_ROWS];
    for row in 0..GRID_ROWS {
        for column in 0..GRID_COLUMNS {
            let mut sum = 0.0;
            for sy in 0..4 {
                for sx in 0..4 {
                    let x = ((column * 4 + sx) * width / (GRID_COLUMNS * 4) + width / (GRID_COLUMNS * 8)).min(width - 1);
                    let y = ((row * 4 + sy) * height / (GRID_ROWS * 4) + height / (GRID_ROWS * 8)).min(height - 1);
                    let at = y * stride + x * 4;
                    let Some(p) = pixels.get(at..at + 4) else { continue };
                    sum += 0.2126 * p[channels[0]] as f32 + 0.7152 * p[channels[1]] as f32 + 0.0722 * p[channels[2]] as f32;
                }
            }
            grid[row * GRID_COLUMNS + column] = sum / (16.0 * 255.0);
        }
    }
    grid
}

pub fn grid(image: &Image) -> Vec<f32> {
    grid_of(&image.rgba, image.width as usize, image.height as usize, image.width as usize * 4, [0, 1, 2])
}

/// The same grid from top-down BGRA pixels, as a screen capture gives them.
pub fn grid_bgra(pixels: &[u8], width: usize, height: usize, stride: usize) -> Vec<f32> {
    grid_of(pixels, width, height, stride, [2, 1, 0])
}

/// The share of cells whose brightness moved by more than `threshold`.
pub fn share_changed(a: &[f32], b: &[f32], threshold: f32) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 1.0;
    }
    a.iter().zip(b).filter(|(x, y)| (*x - *y).abs() > threshold).count() as f32 / a.len() as f32
}

/// A screen counts as changed when this share of the grid moved.
const CHANGE_SHARE: f32 = 0.04;
/// And as settled after this long without movement.
const SETTLE_SECONDS: f64 = 0.5;

/// Watches grids of the screen for a change that then holds still.
#[derive(Debug, Default)]
pub struct ChangeWatcher {
    reference: Option<Vec<f32>>,
    previous: Option<Vec<f32>>,
    change_start: Option<f64>,
    last_movement: f64,
}

impl ChangeWatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, grid: Vec<f32>, now: f64) {
        let Some(reference) = &self.reference else {
            self.reference = Some(grid.clone());
            self.previous = Some(grid);
            self.last_movement = now;
            return;
        };
        if self.change_start.is_none() && share_changed(reference, &grid, 0.06) >= CHANGE_SHARE {
            self.change_start = Some(now);
        }
        if self.previous.as_ref().is_some_and(|p| share_changed(p, &grid, 0.03) > 0.002) {
            self.last_movement = now;
        }
        self.previous = Some(grid);
    }

    /// The start time of a change that has settled, once.
    pub fn settled(&mut self, now: f64) -> Option<f64> {
        let start = self.change_start?;
        if now - self.last_movement < SETTLE_SECONDS {
            return None;
        }
        self.change_start = None;
        let grid = self.previous.clone()?;
        if share_changed(self.reference.as_deref()?, &grid, 0.06) < CHANGE_SHARE {
            return None;
        }
        self.reference = Some(grid);
        Some(start)
    }
}

// Picking screenshots

/// Screenshots allowed for a recording: 6 up to 40 s, one more per 20 s, at most 10.
pub fn budget(seconds: f64) -> usize {
    (6 + ((seconds - 40.0) / 20.0).max(0.0) as usize).min(10)
}

/// Below this share of changed cells, two screenshots are the same.
const SAME_SCREEN_SHARE: f32 = 0.03;

/// Every mark, then clicks, then settled changes, within the budget. A
/// screenshot that looks like its neighbour is dropped unless both carry
/// marks.
pub fn pick(moments: Vec<Moment>, seconds: f64) -> Vec<Moment> {
    let mut kept = moments;
    kept.sort_by_key(|m| m.sample);
    let mut grids: Vec<Vec<f32>> = kept.iter().map(|m| grid(&m.image)).collect();

    let mut index = 1;
    while index < kept.len() {
        let same = share_changed(&grids[index - 1], &grids[index], 0.06) < SAME_SCREEN_SHARE;
        if same && (kept[index - 1].kind != MomentKind::Marks || kept[index].kind != MomentKind::Marks) {
            let drop = if kept[index - 1].kind >= kept[index].kind { index } else { index - 1 };
            kept.remove(drop);
            grids.remove(drop);
        } else {
            index += 1;
        }
    }

    let marks = kept.iter().filter(|m| m.kind == MomentKind::Marks).count();
    let budget = budget(seconds).max(marks);
    while kept.len() > budget {
        let candidates: Vec<usize> = (0..kept.len()).filter(|&i| kept[i].kind != MomentKind::Marks).collect();
        let Some(lowest) = candidates.iter().map(|&i| kept[i].kind).min() else { break };
        let Some(drop) = candidates.into_iter().filter(|&i| kept[i].kind == lowest).min_by_key(|&i| gap(&kept, i)) else { break };
        kept.remove(drop);
    }
    kept
}

fn gap(moments: &[Moment], index: usize) -> usize {
    let before = if index > 0 { moments[index].sample - moments[index - 1].sample } else { usize::MAX };
    let after = if index + 1 < moments.len() { moments[index + 1].sample - moments[index].sample } else { usize::MAX };
    before.min(after)
}

/// One anchor per screenshot, and one per mark when a screenshot has
/// several, so the text can say [1] [2] where each was drawn.
pub fn anchors(moments: &[Moment]) -> Vec<Anchor> {
    moments
        .iter()
        .enumerate()
        .flat_map(|(index, moment)| {
            if moment.strokes.len() <= 1 {
                vec![Anchor { sample: moment.sample, frame: index, label: None }]
            } else {
                moment
                    .strokes
                    .iter()
                    .enumerate()
                    .map(|(n, s)| Anchor { sample: s.start_sample, frame: index, label: Some(format!("[{}]", n + 1)) })
                    .collect()
            }
        })
        .collect()
}

// The text

/// A mark is placed at the quietest pause from this long before it to
/// shortly after, so it lands between words.
const LOOK_BACK_SECONDS: f64 = 1.5;
const LOOK_AHEAD_SECONDS: f64 = 0.3;
/// Marks this close together read as one place in the speech.
const MERGE_SECONDS: f64 = 0.4;

/// Where to cut the recording for each `(anchor index, sample)`.
pub fn cuts(marks: &[(usize, usize)], samples: &[f32]) -> Vec<Cut> {
    let rate = SAMPLE_RATE as f64;
    let back = (rate * LOOK_BACK_SECONDS) as usize;
    let ahead = (rate * LOOK_AHEAD_SECONDS) as usize;
    let merge = (rate * MERGE_SECONDS) as i64;
    let edge = (rate * 0.2) as usize;
    let len = samples.len();
    let mut sorted = marks.to_vec();
    sorted.sort_by_key(|&(_, sample)| sample);
    let mut cuts: Vec<Cut> = Vec::new();
    for (number, sample) in sorted {
        let target = sample.min(len);
        let range = target.saturating_sub(back)..(target + ahead).min(len);
        let mut at = find_pause(&samples[range.clone()]).map_or(target, |p| range.start + p);
        if at < edge {
            at = 0;
        }
        if len - at < edge {
            at = len;
        }
        if let Some(last) = cuts.last_mut() {
            if (at as i64 - last.sample as i64) < merge {
                last.marks.push(number);
                continue;
            }
            at = at.max(last.sample);
        }
        cuts.push(Cut { sample: at, marks: vec![number] });
    }
    cuts
}

/// The audio between cuts, one piece more than there are cuts.
pub fn pieces(samples_len: usize, cuts: &[Cut]) -> Vec<Range<usize>> {
    let mut pieces = Vec::with_capacity(cuts.len() + 1);
    let mut start = 0;
    for cut in cuts {
        let end = cut.sample.max(start).min(samples_len);
        pieces.push(start..end);
        start = end;
    }
    pieces.push(start..samples_len.max(start));
    pieces
}

/// One line per screenshot: its number, when it was taken, what happened,
/// and what was said from then until the next one. Words said before the
/// first screenshot belong to it.
pub fn compose(pieces: &[String], cuts: &[Cut], anchors: &[Anchor], frames: &[FrameLine]) -> String {
    if frames.is_empty() {
        return pieces.iter().map(|p| p.trim()).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ");
    }
    let mut lines: Vec<Vec<String>> = vec![Vec::new(); frames.len()];
    let mut current = 0;
    let mut previous = String::new();
    for (index, raw) in pieces.iter().enumerate() {
        if index > 0 {
            for &a in cuts.get(index - 1).map(|c| c.marks.as_slice()).unwrap_or(&[]) {
                let Some(anchor) = anchors.get(a) else { continue };
                if anchor.frame != current && anchor.frame < frames.len() {
                    current = anchor.frame;
                    previous.clear();
                }
                if let Some(label) = &anchor.label {
                    lines[current].push(label.clone());
                }
            }
        }
        let mut text = raw.trim().to_string();
        if text.is_empty() {
            continue;
        }
        if continues_sentence(&previous) {
            text = lowercasing_first_word(&text);
        }
        lines[current].push(text.clone());
        previous = text;
    }
    let mut last_place: Option<&str> = None;
    frames
        .iter()
        .enumerate()
        .map(|(index, frame)| {
            let mut head = format!("- #{} ({})", index + 1, clock(frame.time));
            // Where it happened, only when it moved since the line before.
            if let Some(place) = frame.place.as_deref() {
                if Some(place) != last_place {
                    head.push(' ');
                    head.push_str(place);
                    last_place = Some(place);
                }
            }
            if let Some(tag) = &frame.tag {
                head.push(' ');
                head.push_str(tag);
            }
            let said = lines[index].join(" ");
            if said.is_empty() { head } else { format!("{head} “{said}”") }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// "[Sketch 0:42 · 2560×1440 px display at 1.5x · screenshots in C:\…]"
pub fn header(duration: f64, folder: Option<&str>, display: Option<(u32, u32, f32)>) -> String {
    let mut line = format!("[Sketch {}", clock(duration));
    if let Some((width, height, scale)) = display {
        let scale = format!("{scale:.2}");
        let scale = scale.trim_end_matches('0').trim_end_matches('.');
        line.push_str(&format!(" · {width}×{height} px display at {scale}x"));
    }
    if let Some(folder) = folder {
        line.push_str(&format!(" · screenshots in {folder}"));
    }
    line.push(']');
    line
}

/// Read first by an agent that has no idea what a Sketch is.
pub const PREAMBLE: &str = "Screen markup from Quill Sketch. The screenshots are attached in order, numbered like the lines below, and zoomed crops follow each marked one. Each line is what I said while that screenshot was on screen; red ink is mine.";

/// "0:09", "1:05".
pub fn clock(seconds: f64) -> String {
    let whole = seconds.max(0.0).floor() as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

fn continues_sentence(text: &str) -> bool {
    text.trim().chars().last().is_some_and(|c| !".!?:…".contains(c))
}

/// "This" becomes "this"; "I", "NASA" and "McDonald" stay.
fn lowercasing_first_word(text: &str) -> String {
    let word: String = text.chars().take_while(|c| !c.is_whitespace() && *c != ',' && *c != '.').collect();
    let mut chars = word.chars();
    let Some(first) = chars.next() else { return text.to_string() };
    if word.chars().count() < 2 || !first.is_uppercase() || chars.any(char::is_uppercase) || word.starts_with("I'") || word.starts_with("I’") {
        return text.to_string();
    }
    let rest = &text[first.len_utf8()..];
    format!("{}{rest}", first.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(shade: u8) -> Arc<Image> {
        Arc::new(Image { width: 64, height: 40, rgba: [shade, shade, shade, 255].repeat(64 * 40) })
    }

    fn moment(kind: MomentKind, sample: usize, shade: u8) -> Moment {
        Moment { kind, sample, image: image(shade), strokes: vec![], click: None, place: Place::default() }
    }

    fn stroke(start_sample: usize, number: usize) -> Stroke {
        Stroke { points: vec![Point { x: 1.0, y: 1.0 }, Point { x: 20.0, y: 1.0 }], start_sample, number }
    }

    #[test]
    fn budget_grows_with_length() {
        assert_eq!(budget(10.0), 6);
        assert_eq!(budget(40.0), 6);
        assert_eq!(budget(60.0), 7);
        assert_eq!(budget(79.0), 7);
        assert_eq!(budget(500.0), 10);
    }

    #[test]
    fn pick_drops_repeats_but_keeps_marks() {
        let mut marks_a = moment(MomentKind::Marks, 200, 10);
        marks_a.strokes = vec![stroke(200, 1)];
        let mut marks_b = moment(MomentKind::Marks, 300, 10);
        marks_b.strokes = vec![stroke(300, 2)];
        let picked = pick(
            vec![moment(MomentKind::Start, 0, 10), moment(MomentKind::Click, 100, 10), marks_a, marks_b, moment(MomentKind::Change, 400, 200)],
            10.0,
        );
        let kinds: Vec<_> = picked.iter().map(|m| m.kind).collect();
        assert_eq!(kinds, [MomentKind::Marks, MomentKind::Marks, MomentKind::Change]);
    }

    #[test]
    fn pick_trims_to_budget_least_telling_first() {
        let mut moments = vec![moment(MomentKind::Start, 0, 0)];
        for i in 1..=8 {
            moments.push(moment(MomentKind::Click, i * 1000, (i * 30) as u8));
        }
        moments.push(moment(MomentKind::Change, 9500, 255));
        let picked = pick(moments, 10.0);
        assert_eq!(picked.len(), 6);
        assert!(picked.iter().all(|m| m.kind == MomentKind::Click));
    }

    #[test]
    fn anchors_one_per_frame_or_per_mark() {
        let mut two = moment(MomentKind::Marks, 50, 0);
        two.strokes = vec![stroke(50, 1), stroke(80, 2)];
        let a = anchors(&[moment(MomentKind::Start, 0, 0), two]);
        assert_eq!(
            a,
            [
                Anchor { sample: 0, frame: 0, label: None },
                Anchor { sample: 50, frame: 1, label: Some("[1]".into()) },
                Anchor { sample: 80, frame: 1, label: Some("[2]".into()) },
            ]
        );
    }

    fn speech_with_pause(seconds: f64, pause_at: f64) -> Vec<f32> {
        let len = (seconds * SAMPLE_RATE as f64) as usize;
        let quiet = ((pause_at - 0.25) * SAMPLE_RATE as f64) as usize..((pause_at + 0.25) * SAMPLE_RATE as f64) as usize;
        (0..len).map(|i| if quiet.contains(&i) { 0.0 } else { 0.3 * ((i as f32) * 0.05).sin() }).collect()
    }

    #[test]
    fn cuts_land_on_pauses_and_merge() {
        let samples = speech_with_pause(6.0, 2.5);
        let mark = (3.0 * SAMPLE_RATE as f64) as usize;
        let c = cuts(&[(0, mark), (1, mark + 1000)], &samples);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].marks, [0, 1]);
        let at = c[0].sample as f64 / SAMPLE_RATE as f64;
        assert!((at - 2.5).abs() < 0.2, "cut at {at}");
        // Near the start snaps to 0.
        assert_eq!(cuts(&[(0, 100)], &samples)[0].sample, 0);
    }

    #[test]
    fn pieces_cover_everything() {
        let p = pieces(100, &[Cut { sample: 0, marks: vec![0] }, Cut { sample: 40, marks: vec![1] }]);
        assert_eq!(p, [0..0, 0..40, 40..100]);
    }

    #[test]
    fn compose_lines() {
        let anchors = vec![
            Anchor { sample: 0, frame: 0, label: None },
            Anchor { sample: 10, frame: 1, label: Some("[1]".into()) },
            Anchor { sample: 20, frame: 1, label: Some("[2]".into()) },
        ];
        let cuts = vec![Cut { sample: 0, marks: vec![0] }, Cut { sample: 10, marks: vec![1] }, Cut { sample: 20, marks: vec![2] }];
        let pieces: Vec<String> = ["", "Look here and", "This button", "And that."].map(String::from).to_vec();
        let frames = vec![
            FrameLine { time: 0.0, tag: None, place: Some("in Chrome".into()) },
            FrameLine { time: 9.4, tag: Some("[clicked]".into()), place: Some("in Chrome".into()) },
        ];
        let text = compose(&pieces, &cuts, &anchors, &frames);
        // A new screenshot starts a new sentence; a mark within one doesn't.
        assert_eq!(text, "- #1 (0:00) in Chrome “Look here and”\n- #2 (0:09) [clicked] “[1] This button [2] and that.”");
        assert_eq!(compose(&pieces, &cuts, &anchors, &[]), "Look here and This button And that.");
    }

    #[test]
    fn lowercasing_keeps_names() {
        assert_eq!(lowercasing_first_word("This one"), "this one");
        assert_eq!(lowercasing_first_word("I think"), "I think");
        assert_eq!(lowercasing_first_word("NASA said"), "NASA said");
        assert_eq!(lowercasing_first_word("McDonald"), "McDonald");
        assert_eq!(lowercasing_first_word("I'm here"), "I'm here");
    }

    #[test]
    fn header_and_clock() {
        assert_eq!(clock(9.9), "0:09");
        assert_eq!(clock(65.0), "1:05");
        assert_eq!(clock(-3.0), "0:00");
        assert_eq!(header(42.0, Some("C:\\s"), Some((2560, 1440, 1.5))), "[Sketch 0:42 · 2560×1440 px display at 1.5x · screenshots in C:\\s]");
        assert_eq!(header(5.0, None, Some((1920, 1080, 1.0))), "[Sketch 0:05 · 1920×1080 px display at 1x]");
        assert_eq!(header(5.0, None, Some((1920, 1080, 1.25))), "[Sketch 0:05 · 1920×1080 px display at 1.25x]");
    }

    #[test]
    fn place_description() {
        let p = |app: Option<&str>, window: Option<&str>| Place { app: app.map(Into::into), window: window.map(Into::into), element: None };
        assert_eq!(p(None, Some("x")).description(), None);
        assert_eq!(p(Some("Chrome"), None).description().unwrap(), "in Chrome");
        assert_eq!(p(Some("Chrome"), Some(" Chrome ")).description().unwrap(), "in Chrome");
        assert_eq!(p(Some("Chrome"), Some("GitHub")).description().unwrap(), "in Chrome “GitHub”");
        let long = "x".repeat(70);
        assert_eq!(p(Some("A"), Some(&long)).description().unwrap(), format!("in A “{}…”", "x".repeat(59)));
    }

    #[test]
    fn tags() {
        let mut m = moment(MomentKind::Click, 0, 0);
        assert_eq!(m.tag().unwrap(), "[clicked]");
        m.place.element = Some("button “Save”".into());
        assert_eq!(m.tag().unwrap(), "[clicked button “Save”]");
        assert_eq!(moment(MomentKind::Change, 0, 0).tag().unwrap(), "[screen changed]");
        assert_eq!(moment(MomentKind::Start, 0, 0).tag(), None);
    }

    #[test]
    fn change_watcher_waits_for_still_screen() {
        let a = grid(&image(0));
        let b = grid(&image(200));
        let mut w = ChangeWatcher::new();
        w.feed(a.clone(), 0.0);
        w.feed(a.clone(), 0.1);
        assert_eq!(w.settled(1.0), None);
        w.feed(b.clone(), 1.0);
        assert_eq!(w.settled(1.2), None, "still moving");
        w.feed(b.clone(), 1.3);
        assert_eq!(w.settled(1.6), Some(1.0));
        assert_eq!(w.settled(2.0), None, "reported once");
        // A change that reverts is not reported.
        w.feed(a.clone(), 3.0);
        w.feed(b.clone(), 3.1);
        assert_eq!(w.settled(4.0), None);
    }

    #[test]
    fn grids_from_rgba_and_bgra_agree() {
        let rgba = Image { width: 8, height: 8, rgba: [255, 0, 0, 255].repeat(64) };
        let bgra = [0u8, 0, 255, 255].repeat(64);
        assert_eq!(grid(&rgba), grid_bgra(&bgra, 8, 8, 32));
        assert!((grid(&rgba)[0] - 0.2126).abs() < 1e-4);
        assert_eq!(share_changed(&[0.0], &[0.0, 1.0], 0.1), 1.0);
    }

    #[test]
    fn stroke_length_sums_segments() {
        let p = [Point { x: 0.0, y: 0.0 }, Point { x: 3.0, y: 4.0 }, Point { x: 3.0, y: 10.0 }];
        assert_eq!(stroke_length(&p), 11.0);
        assert_eq!(stroke_length(&p[..1]), 0.0);
    }
}
