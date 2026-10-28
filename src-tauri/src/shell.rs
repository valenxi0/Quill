//! What the controller needs from the UI shell, kept behind a trait so the
//! controller never touches windows directly.

use quill_core::corrections::CorrectionPair;

use crate::controller::DictationView;
use crate::platform::screen::Monitor;
use crate::sketch::SketchView;

pub trait Shell: Send + Sync {
    fn dictation_changed(&self, view: &DictationView);
    fn level(&self, level: f32);
    /// The provisional transcript while recording, when live preview is on.
    fn preview(&self, text: &str);
    fn state_changed(&self);
    fn history_changed(&self);
    fn suggest(&self, pair: &CorrectionPair);
    /// Quill's own windows, which are never paste targets.
    fn own_windows(&self) -> Vec<isize>;

    /// Sketch: show the ink layer and the bar on `monitor`.
    fn sketch_show(&self, monitor: &Monitor, view: &SketchView);
    fn sketch_view(&self, view: &SketchView);
    /// The stroke being drawn, in the ink layer's CSS pixels; empty clears it.
    fn sketch_live(&self, points: &[(f32, f32)]);
    /// A finished mark and its "0:15" tag.
    fn sketch_commit(&self, points: &[(f32, f32)], label: &str);
    fn sketch_undo(&self);
    fn sketch_pen(&self, on: bool);
    fn sketch_hide(&self);
}
