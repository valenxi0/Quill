//! What the controller needs from the UI shell, kept behind a trait so the
//! controller never touches windows directly.

use quill_core::corrections::CorrectionPair;

use crate::controller::DictationView;

pub trait Shell: Send + Sync {
    fn dictation_changed(&self, view: &DictationView);
    fn level(&self, level: f32);
    fn state_changed(&self);
    fn history_changed(&self);
    fn suggest(&self, pair: &CorrectionPair);
    /// Quill's own windows, which are never paste targets.
    fn own_windows(&self) -> Vec<isize>;
}
