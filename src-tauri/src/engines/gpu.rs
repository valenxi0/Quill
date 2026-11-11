//! Remembers a GPU start that never returned (see `quill_core::gpu`).
//!
//! whisper.cpp and llama.cpp share one ggml. ggml creates the Vulkan instance
//! the first time anything asks for its backend list (`LlamaBackend::init`,
//! or whisper's context), and only once per process. So the choice is made
//! once per process, before either engine touches ggml.

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, OnceLock};

use quill_core::gpu::{gpu_plan, GpuChoice, GpuMarker};

use crate::paths;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The marker file, and how many GPU starts are in flight. The file exists
/// while the count is above zero, or after a start never returned.
struct Marker {
    path: PathBuf,
    starting: Mutex<usize>,
}

impl Marker {
    fn new(path: PathBuf) -> Self {
        Self { path, starting: Mutex::new(0) }
    }

    fn count(&self) -> std::sync::MutexGuard<'_, usize> {
        self.starting.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Reads the marker left by an earlier run, deleting it if it is stale.
    fn decide(&self, version: &str) -> GpuChoice {
        let text = std::fs::read_to_string(&self.path).ok();
        let plan = gpu_plan(text.as_deref(), version);
        if plan.remove_marker {
            let _ = self.remove();
        }
        plan.choice
    }

    /// Flushed before the GPU starts: a driver crash can take the whole
    /// machine down with it.
    fn write(&self, version: &str) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut f = std::fs::File::create(&self.path)?;
        f.write_all(GpuMarker::new(version).to_json().as_bytes())?;
        f.sync_all()
    }

    fn remove(&self) -> io::Result<()> {
        match std::fs::remove_file(&self.path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    fn begin(&self, version: &str) {
        let mut n = self.count();
        if *n == 0 {
            if let Err(e) = self.write(version) {
                log::warn!("could not write the GPU start marker: {e}");
            }
        }
        *n += 1;
    }

    fn end(&self) {
        let mut n = self.count();
        *n = n.saturating_sub(1);
        if *n == 0 {
            let _ = self.remove();
        }
    }

    /// Quitting mid-start is not a driver crash.
    fn release_on_exit(&self) {
        if *self.count() > 0 {
            let _ = self.remove();
        }
    }
}

static MARKER: LazyLock<Marker> = LazyLock::new(|| Marker::new(paths::gpu_marker_file()));
static CHOICE: OnceLock<GpuChoice> = OnceLock::new();

/// Keeps ggml from registering its Vulkan backend, so nothing in this
/// process loads the graphics driver. Returns whether that worked.
fn disable_vulkan() -> bool {
    // ggml reads this with the C runtime's getenv, which never sees
    // SetEnvironmentVariable (std::env::set_var). _putenv_s sets both copies.
    // Clearing the OS copy afterwards keeps it out of child processes, so a
    // restarted Quill tries the GPU again.
    #[cfg(windows)]
    {
        extern "C" {
            fn _putenv_s(name: *const std::ffi::c_char, value: *const std::ffi::c_char) -> i32;
        }
        // SAFETY: both arguments are NUL-terminated literals.
        let ok = unsafe { _putenv_s(c"GGML_DISABLE_VULKAN".as_ptr(), c"1".as_ptr()) } == 0;
        std::env::remove_var("GGML_DISABLE_VULKAN");
        ok
    }
    #[cfg(not(windows))]
    false
}

/// Decided once per process, on first use.
pub fn choice() -> GpuChoice {
    *CHOICE.get_or_init(|| match MARKER.decide(VERSION) {
        GpuChoice::CpuAfterCrash if disable_vulkan() => {
            log::warn!("a GPU start crashed Quill {VERSION}; using the CPU");
            GpuChoice::CpuAfterCrash
        }
        GpuChoice::CpuAfterCrash => {
            log::warn!("a GPU start crashed Quill {VERSION}, but the GPU could not be turned off");
            GpuChoice::Gpu
        }
        GpuChoice::Gpu => GpuChoice::Gpu,
    })
}

/// Whether engines should start on the GPU.
pub fn use_gpu() -> bool {
    cfg!(feature = "vulkan") && choice() == GpuChoice::Gpu
}

/// Whether this run of Quill is on the CPU because a GPU start crashed.
pub fn fallback() -> bool {
    choice() == GpuChoice::CpuAfterCrash
}

/// Forget the crash. Takes effect at the next launch, since ggml's backend
/// list is fixed for the life of the process.
pub fn retry() -> io::Result<()> {
    MARKER.remove()
}

/// Call from the app's exit event.
pub fn release_on_exit() {
    MARKER.release_on_exit();
}

/// Hold for the whole GPU start; the marker is removed when the last one
/// drops, on success or error.
pub struct GpuStart(bool);

pub fn start(_engine: &str) -> GpuStart {
    if !use_gpu() {
        return GpuStart(false);
    }
    MARKER.begin(VERSION);
    #[cfg(debug_assertions)]
    if std::env::var_os("QUILL_SIMULATE_GPU_CRASH").is_some() {
        eprintln!("QUILL_SIMULATE_GPU_CRASH: aborting during the {_engine} GPU start");
        std::process::abort();
    }
    GpuStart(true)
}

impl Drop for GpuStart {
    fn drop(&mut self) {
        if self.0 {
            MARKER.end();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempMarker(Marker);

    impl std::ops::Deref for TempMarker {
        type Target = Marker;
        fn deref(&self) -> &Marker {
            &self.0
        }
    }

    impl Drop for TempMarker {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.0.path.parent().unwrap());
        }
    }

    fn marker(name: &str) -> TempMarker {
        let dir = std::env::temp_dir().join(format!("quill-gpu-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        TempMarker(Marker::new(dir.join("gpu-starting.json")))
    }

    #[test]
    fn a_start_that_returns_leaves_nothing() {
        let m = marker("returns");
        m.begin("1.0.0");
        assert_eq!(std::fs::read_to_string(&m.path).unwrap(), r#"{"version":"1.0.0"}"#);
        m.end();
        assert!(!m.path.exists());
        assert_eq!(m.decide("1.0.0"), GpuChoice::Gpu);
    }

    #[test]
    fn overlapping_starts_keep_the_marker_until_the_last_ends() {
        let m = marker("overlap");
        m.begin("1.0.0");
        m.begin("1.0.0");
        m.end();
        assert!(m.path.exists());
        m.end();
        assert!(!m.path.exists());
    }

    #[test]
    fn a_start_that_never_returns_means_cpu_until_retry() {
        let m = marker("crash");
        m.begin("1.0.0");
        let next = Marker::new(m.path.clone());
        assert_eq!(next.decide("1.0.0"), GpuChoice::CpuAfterCrash);
        assert_eq!(Marker::new(m.path.clone()).decide("1.0.0"), GpuChoice::CpuAfterCrash);
        next.remove().unwrap();
        next.remove().unwrap();
        assert_eq!(Marker::new(m.path.clone()).decide("1.0.0"), GpuChoice::Gpu);
    }

    #[test]
    fn a_new_version_deletes_the_marker() {
        let m = marker("version");
        m.begin("1.0.0");
        assert_eq!(Marker::new(m.path.clone()).decide("1.1.0"), GpuChoice::Gpu);
        assert!(!m.path.exists());
    }

    #[test]
    fn quitting_mid_start_is_not_a_crash() {
        let m = marker("quit");
        m.release_on_exit();
        m.begin("1.0.0");
        m.release_on_exit();
        assert_eq!(Marker::new(m.path.clone()).decide("1.0.0"), GpuChoice::Gpu);
    }

    #[test]
    fn exit_without_a_start_keeps_a_crash_marker() {
        let m = marker("cpu-exit");
        m.write("1.0.0").unwrap();
        let cpu_run = Marker::new(m.path.clone());
        assert_eq!(cpu_run.decide("1.0.0"), GpuChoice::CpuAfterCrash);
        cpu_run.release_on_exit();
        assert!(m.path.exists());
    }
}
