//! Whether to start the GPU backend, given what an earlier start left behind.
//!
//! A driver crash kills the process and cannot be caught. So a marker is
//! written before each GPU start and removed when the start returns. A marker
//! from this app version found at launch means a start never returned: Quill
//! keeps the marker and stays on the CPU until the user asks to try the GPU
//! again, or a new version clears it.

use serde::{Deserialize, Serialize};

/// The marker's contents: the app version that was starting the GPU.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuMarker {
    pub version: String,
}

impl GpuMarker {
    pub fn new(version: &str) -> Self {
        Self { version: version.into() }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a string field always serializes")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuChoice {
    Gpu,
    /// A GPU start in this version never returned.
    CpuAfterCrash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuPlan {
    pub choice: GpuChoice,
    /// The marker is stale (another version) or unreadable: delete it.
    pub remove_marker: bool,
}

/// `marker`: the marker file's text, if the file exists. An unreadable marker
/// can only come from a write cut short, which happens before the GPU starts.
pub fn gpu_plan(marker: Option<&str>, version: &str) -> GpuPlan {
    match marker.map(serde_json::from_str::<GpuMarker>) {
        None => GpuPlan { choice: GpuChoice::Gpu, remove_marker: false },
        Some(Ok(m)) if m.version == version => GpuPlan { choice: GpuChoice::CpuAfterCrash, remove_marker: false },
        Some(_) => GpuPlan { choice: GpuChoice::Gpu, remove_marker: true },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V: &str = "0.3.2";

    #[test]
    fn no_marker_uses_the_gpu() {
        assert_eq!(gpu_plan(None, V), GpuPlan { choice: GpuChoice::Gpu, remove_marker: false });
    }

    #[test]
    fn marker_from_this_version_keeps_the_cpu_and_stays() {
        let text = GpuMarker::new(V).to_json();
        assert_eq!(gpu_plan(Some(&text), V), GpuPlan { choice: GpuChoice::CpuAfterCrash, remove_marker: false });
    }

    #[test]
    fn a_new_version_clears_the_marker_and_tries_the_gpu() {
        let text = GpuMarker::new("0.3.1").to_json();
        assert_eq!(gpu_plan(Some(&text), V), GpuPlan { choice: GpuChoice::Gpu, remove_marker: true });
    }

    #[test]
    fn unreadable_marker_is_cleared() {
        for text in ["", "{\"vers", "null", "{}"] {
            assert_eq!(gpu_plan(Some(text), V), GpuPlan { choice: GpuChoice::Gpu, remove_marker: true }, "{text:?}");
        }
    }

    #[test]
    fn marker_json_shape() {
        assert_eq!(GpuMarker::new(V).to_json(), r#"{"version":"0.3.2"}"#);
    }
}
