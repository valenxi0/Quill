//! Whether to start the GPU backend, given what an earlier start left behind.
//!
//! A driver crash kills the process and cannot be caught. So a marker is
//! written before each GPU start and removed when the start returns. A marker
//! found later means a start never returned. Quill then stays on the CPU for
//! the rest of that app version, or until the user asks to try the GPU again.

use serde::{Deserialize, Serialize};

/// What the marker and the disabled record hold: which app version and
/// engine were starting the GPU.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuRecord {
    pub version: String,
    pub engine: String,
}

impl GpuRecord {
    pub fn new(version: &str, engine: &str) -> Self {
        Self { version: version.into(), engine: engine.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuChoice {
    Gpu,
    /// `crash` names the version and engine whose GPU start never returned.
    CpuAfterCrash { crash: GpuRecord },
}

/// The choice, and the file changes that make the stored state agree with
/// it. Applying them twice, or after a crash partway, ends in the same state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuPlan {
    pub choice: GpuChoice,
    /// Write this disabled record (before removing the marker).
    pub write_disabled: Option<GpuRecord>,
    pub remove_marker: bool,
    pub remove_disabled: bool,
}

/// `marker`: a GPU start that never returned. `disabled`: an earlier crash
/// already moved Quill to the CPU. Records from another version are dropped.
pub fn gpu_plan(marker: Option<GpuRecord>, disabled: Option<GpuRecord>, version: &str) -> GpuPlan {
    let marker_current = marker.as_ref().is_some_and(|m| m.version == version);
    let disabled_current = disabled.as_ref().is_some_and(|d| d.version == version);
    let has_marker = marker.is_some();
    let has_disabled = disabled.is_some();
    let crash = match (disabled, marker) {
        (Some(d), _) if disabled_current => Some(d),
        (_, Some(m)) if marker_current => Some(m),
        _ => None,
    };
    let write_disabled = match &crash {
        Some(c) if !disabled_current => Some(c.clone()),
        _ => None,
    };
    GpuPlan {
        remove_marker: has_marker,
        // A new record overwrites a stale one, so only remove when not writing.
        remove_disabled: has_disabled && !disabled_current && write_disabled.is_none(),
        write_disabled,
        choice: match crash {
            Some(crash) => GpuChoice::CpuAfterCrash { crash },
            None => GpuChoice::Gpu,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V: &str = "0.3.2";

    fn rec(version: &str, engine: &str) -> Option<GpuRecord> {
        Some(GpuRecord::new(version, engine))
    }

    fn cpu(version: &str, engine: &str) -> GpuChoice {
        GpuChoice::CpuAfterCrash { crash: GpuRecord::new(version, engine) }
    }

    #[test]
    fn nothing_stored_uses_the_gpu() {
        assert_eq!(
            gpu_plan(None, None, V),
            GpuPlan { choice: GpuChoice::Gpu, write_disabled: None, remove_marker: false, remove_disabled: false }
        );
    }

    #[test]
    fn marker_from_this_version_moves_to_the_cpu_and_is_recorded() {
        assert_eq!(
            gpu_plan(rec(V, "S1-mini"), None, V),
            GpuPlan { choice: cpu(V, "S1-mini"), write_disabled: rec(V, "S1-mini"), remove_marker: true, remove_disabled: false }
        );
    }

    #[test]
    fn disabled_record_keeps_the_cpu_across_restarts() {
        assert_eq!(
            gpu_plan(None, rec(V, "Whisper"), V),
            GpuPlan { choice: cpu(V, "Whisper"), write_disabled: None, remove_marker: false, remove_disabled: false }
        );
    }

    #[test]
    fn crash_between_writing_disabled_and_removing_marker_converges() {
        let first = gpu_plan(rec(V, "S1-mini"), None, V);
        let again = gpu_plan(rec(V, "S1-mini"), first.write_disabled.clone(), V);
        assert_eq!(again.choice, first.choice);
        assert!(again.remove_marker);
        assert_eq!(again.write_disabled, None);
        assert_eq!(gpu_plan(None, first.write_disabled, V).choice, first.choice);
    }

    #[test]
    fn existing_disabled_record_wins_over_a_new_marker() {
        let plan = gpu_plan(rec(V, "S1-mini"), rec(V, "Whisper"), V);
        assert_eq!(plan.choice, cpu(V, "Whisper"));
        assert!(plan.remove_marker);
        assert_eq!(plan.write_disabled, None);
    }

    #[test]
    fn a_new_version_tries_the_gpu_again() {
        assert_eq!(
            gpu_plan(None, rec("0.3.1", "S1-mini"), V),
            GpuPlan { choice: GpuChoice::Gpu, write_disabled: None, remove_marker: false, remove_disabled: true }
        );
        assert_eq!(
            gpu_plan(rec("0.3.1", "S1-mini"), None, V),
            GpuPlan { choice: GpuChoice::Gpu, write_disabled: None, remove_marker: true, remove_disabled: false }
        );
        assert_eq!(
            gpu_plan(rec("0.3.1", "S1-mini"), rec("0.3.1", "Whisper"), V),
            GpuPlan { choice: GpuChoice::Gpu, write_disabled: None, remove_marker: true, remove_disabled: true }
        );
    }

    #[test]
    fn old_disabled_record_is_replaced_by_a_crash_in_this_version() {
        let plan = gpu_plan(rec(V, "Whisper"), rec("0.3.1", "S1-mini"), V);
        assert_eq!(plan.choice, cpu(V, "Whisper"));
        assert_eq!(plan.write_disabled, rec(V, "Whisper"));
        assert!(plan.remove_marker);
        assert!(!plan.remove_disabled);
    }

    #[test]
    fn record_round_trips_as_json() {
        let r = GpuRecord::new(V, "S1-mini");
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, r#"{"version":"0.3.2","engine":"S1-mini"}"#);
        assert_eq!(serde_json::from_str::<GpuRecord>(&json).unwrap(), r);
    }
}
