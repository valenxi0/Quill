//! Where speech is, as found by the voice activity detector. Sample indices
//! are at 16 kHz mono, from the start of the recording.

use std::ops::Range;

use crate::segments::SAMPLE_RATE;

/// Kept before the first and after the last detected speech, so soft onsets
/// and trailing consonants reach the recognizer.
pub const PAD_BEFORE: usize = SAMPLE_RATE * 300 / 1000;
pub const PAD_AFTER: usize = SAMPLE_RATE * 450 / 1000;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpeechMap {
    /// Finished speech stretches, in order.
    pub intervals: Vec<Range<usize>>,
    /// Speech still going on, started no later than this sample.
    pub open_from: Option<usize>,
}

impl SpeechMap {
    pub fn push(&mut self, interval: Range<usize>) {
        if !interval.is_empty() {
            self.intervals.push(interval);
        }
    }

    fn overlapping(&self, range: &Range<usize>) -> impl Iterator<Item = Range<usize>> + '_ {
        let open = self.open_from.map(|s| s..usize::MAX);
        let range = range.clone();
        self.intervals
            .iter()
            .cloned()
            .chain(open)
            .filter(move |i| i.start < range.end && range.start < i.end)
            .map(move |i| i.start.max(range.start)..i.end.min(range.end))
    }

    pub fn has_speech(&self, range: &Range<usize>) -> bool {
        self.overlapping(range).next().is_some()
    }

    /// The part of `range` worth recognizing: first to last speech, padded,
    /// or `None` when it holds no speech.
    pub fn crop(&self, range: &Range<usize>) -> Option<Range<usize>> {
        let mut found = self.overlapping(range);
        let first = found.next()?;
        let (start, end) = found.fold((first.start, first.end), |(s, e), i| (s.min(i.start), e.max(i.end)));
        Some(start.saturating_sub(PAD_BEFORE).max(range.start)..(end + PAD_AFTER).min(range.end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: usize = SAMPLE_RATE;

    #[test]
    fn empty_map_has_no_speech() {
        assert!(!SpeechMap::default().has_speech(&(0..10 * S)));
        assert_eq!(SpeechMap::default().crop(&(0..10 * S)), None);
    }

    #[test]
    fn crops_to_padded_speech_within_the_range() {
        let mut m = SpeechMap::default();
        m.push(2 * S..3 * S);
        m.push(5 * S..6 * S);
        assert_eq!(m.crop(&(0..10 * S)), Some(2 * S - PAD_BEFORE..6 * S + PAD_AFTER));
        assert_eq!(m.crop(&(0..4 * S)), Some(2 * S - PAD_BEFORE..3 * S + PAD_AFTER));
        assert_eq!(m.crop(&(3 * S..5 * S)), None);
        // Padding never leaves the range.
        assert_eq!(m.crop(&(2 * S..6 * S)), Some(2 * S..6 * S));
    }

    #[test]
    fn open_speech_counts_to_the_end() {
        let m = SpeechMap { intervals: vec![], open_from: Some(4 * S) };
        assert!(m.has_speech(&(3 * S..5 * S)));
        assert!(!m.has_speech(&(0..4 * S)));
        assert_eq!(m.crop(&(0..8 * S)), Some(4 * S - PAD_BEFORE..8 * S));
    }
}
