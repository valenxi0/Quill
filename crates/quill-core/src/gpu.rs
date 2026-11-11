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

/// Whether the marker file's text (if the file exists) shows that a GPU
/// start in `version` never returned. Anything else, a marker from another
/// version or one cut short while being written, is stale.
pub fn gpu_crashed(marker: Option<&str>, version: &str) -> bool {
    marker.and_then(|t| serde_json::from_str::<GpuMarker>(t).ok()).is_some_and(|m| m.version == version)
}

#[cfg(test)]
mod tests {
    use super::*;

    const V: &str = "0.3.2";

    #[test]
    fn no_marker_is_no_crash() {
        assert!(!gpu_crashed(None, V));
    }

    #[test]
    fn marker_from_this_version_is_a_crash() {
        assert!(gpu_crashed(Some(&GpuMarker::new(V).to_json()), V));
    }

    #[test]
    fn marker_from_another_version_is_stale() {
        assert!(!gpu_crashed(Some(&GpuMarker::new("0.3.1").to_json()), V));
    }

    #[test]
    fn unreadable_marker_is_stale() {
        for text in ["", "{\"vers", "null", "{}"] {
            assert!(!gpu_crashed(Some(text), V), "{text:?}");
        }
    }

    #[test]
    fn marker_json_shape() {
        assert_eq!(GpuMarker::new(V).to_json(), r#"{"version":"0.3.2"}"#);
    }
}
