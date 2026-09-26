//! Early segments for long dictations, and the pause-aware splitter
//! (section 4.4). All sample counts are at 16 kHz mono.

use crate::text::finish::rms;

pub const SAMPLE_RATE: usize = 16_000;
/// Uncommitted audio needed before looking for a cut.
pub const EARLY_MIN_SECONDS: f32 = 12.0;
/// How far back from the end to look for a pause.
pub const PAUSE_SEARCH_SECONDS: f32 = 6.0;
const WINDOW: usize = SAMPLE_RATE * 400 / 1000;
const HOP: usize = SAMPLE_RATE / 10;
const QUIET_RMS: f32 = 0.012;
const QUIET_RELATIVE: f32 = 0.15;

/// Centre of the quietest 400 ms window in `samples`, scanned with a 100 ms
/// hop, if it is quiet enough to be a pause: RMS under 0.012, or under 15 % of
/// the stretch's overall RMS. The returned index is relative to `samples`.
pub fn find_pause(samples: &[f32]) -> Option<usize> {
    if samples.len() < WINDOW {
        return None;
    }
    let overall = rms(samples);
    let mut best: Option<(usize, f32)> = None;
    let mut start = 0;
    while start + WINDOW <= samples.len() {
        let r = rms(&samples[start..start + WINDOW]);
        if best.map_or(true, |(_, b)| r < b) {
            best = Some((start, r));
        }
        start += HOP;
    }
    let (start, r) = best?;
    (r < QUIET_RMS || r < overall * QUIET_RELATIVE).then_some(start + WINDOW / 2)
}

/// Decides where to cut committed pieces off a growing recording.
#[derive(Debug, Default, Clone)]
pub struct EarlySegmenter {
    committed: usize,
}

impl EarlySegmenter {
    pub fn committed(&self) -> usize {
        self.committed
    }

    /// Called about once a second with the whole recording so far. Returns
    /// the sample range of a newly committed piece, if there is one.
    pub fn poll(&mut self, samples: &[f32]) -> Option<std::ops::Range<usize>> {
        let pending = samples.len().saturating_sub(self.committed);
        if (pending as f32) < EARLY_MIN_SECONDS * SAMPLE_RATE as f32 {
            return None;
        }
        let search = (PAUSE_SEARCH_SECONDS * SAMPLE_RATE as f32) as usize;
        let from = samples.len() - search.min(pending);
        let cut = from + find_pause(&samples[from..])?;
        let range = self.committed..cut;
        self.committed = cut;
        Some(range)
    }

    /// The uncommitted tail left at release.
    pub fn tail<'a>(&self, samples: &'a [f32]) -> &'a [f32] {
        &samples[self.committed.min(samples.len())..]
    }
}

/// Split into pieces of at most `max_seconds`, cutting each at a pause in its
/// last third, or hard-cutting at the maximum.
pub fn split(samples: &[f32], max_seconds: f32) -> Vec<std::ops::Range<usize>> {
    let max = (max_seconds * SAMPLE_RATE as f32) as usize;
    let mut out = Vec::new();
    let mut start = 0;
    while samples.len() - start > max {
        let end = start + max;
        let third = start + max * 2 / 3;
        let cut = find_pause(&samples[third..end]).map_or(end, |p| third + p);
        out.push(start..cut);
        start = cut;
    }
    if start < samples.len() {
        out.push(start..samples.len());
    }
    out
}

/// Pad with trailing silence up to `min_seconds`.
pub fn pad(samples: &[f32], min_seconds: f32) -> Vec<f32> {
    let min = (min_seconds * SAMPLE_RATE as f32).ceil() as usize;
    let mut out = samples.to_vec();
    if out.len() < min {
        out.resize(min, 0.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(seconds: f32, amp: f32) -> Vec<f32> {
        let n = (seconds * SAMPLE_RATE as f32) as usize;
        (0..n).map(|i| amp * ((i as f32) * 0.07).sin()).collect()
    }

    fn silence(seconds: f32) -> Vec<f32> {
        vec![0.0; (seconds * SAMPLE_RATE as f32) as usize]
    }

    #[test]
    fn pause_finder() {
        let mut s = tone(2.0, 0.3);
        s.extend(silence(0.6));
        s.extend(tone(2.0, 0.3));
        let p = find_pause(&s).unwrap();
        let (lo, hi) = (2 * SAMPLE_RATE, (2.6 * SAMPLE_RATE as f32) as usize);
        assert!(p > lo && p < hi, "pause at {p}");
        assert_eq!(find_pause(&tone(4.0, 0.3)), None, "no pause in steady speech");
        assert_eq!(find_pause(&silence(0.2)), None, "too short to judge");
    }

    #[test]
    fn early_segments_wait_for_twelve_seconds_and_a_pause() {
        let mut seg = EarlySegmenter::default();
        let mut s = tone(8.0, 0.3);
        assert_eq!(seg.poll(&s), None);
        s.extend(silence(0.5));
        s.extend(tone(3.4, 0.3));
        assert_eq!(seg.poll(&s), None, "still under 12 s");
        s.extend(tone(0.2, 0.3));
        let r = seg.poll(&s).expect("cut at the pause");
        assert_eq!(r.start, 0);
        assert!(r.end > 8 * SAMPLE_RATE && r.end < (8.5 * SAMPLE_RATE as f32) as usize);
        assert_eq!(seg.tail(&s).len(), s.len() - r.end);
        // Without a pause, nothing is cut, however long it gets.
        let mut seg = EarlySegmenter::default();
        assert_eq!(seg.poll(&tone(20.0, 0.3)), None);
    }

    #[test]
    fn splitter() {
        let mut s = tone(15.0, 0.3);
        s.extend(silence(0.5));
        s.extend(tone(10.0, 0.3));
        let pieces = split(&s, 20.0);
        assert_eq!(pieces.len(), 2);
        let cut = pieces[0].end;
        assert!(cut > 15 * SAMPLE_RATE && cut < (15.5 * SAMPLE_RATE as f32) as usize);
        assert_eq!(pieces[1].end, s.len());

        let hard = split(&tone(45.0, 0.3), 20.0);
        assert_eq!(hard.iter().map(|r| r.len()).collect::<Vec<_>>(), vec![320_000, 320_000, 80_000]);
        assert_eq!(split(&tone(3.0, 0.3), 20.0), vec![0..48_000]);
    }

    #[test]
    fn padding() {
        assert_eq!(pad(&[0.1; 100], 1.0).len(), 16_000);
        assert_eq!(pad(&[0.1; 100], 1.1).len(), 17_600);
        assert_eq!(pad(&vec![0.1; 20_000], 1.0).len(), 20_000);
    }
}
