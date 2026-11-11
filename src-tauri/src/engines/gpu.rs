//! Remembers a GPU start that never returned (see `quill_core::gpu`).
//!
//! whisper.cpp and llama.cpp share one ggml. ggml creates the Vulkan instance
//! the first time anything asks for its backend list (`LlamaBackend::init`,
//! or whisper's context), and only once per process. So the choice is made
//! once per process, before either engine touches ggml.

use std::io::{self, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use quill_core::gpu::{gpu_plan, GpuChoice, GpuRecord};

use crate::paths;

const VERSION: &str = env!("CARGO_PKG_VERSION");

static CHOICE: OnceLock<GpuChoice> = OnceLock::new();
/// GPU starts in flight. The marker exists while this is above zero.
static STARTING: Mutex<usize> = Mutex::new(0);

fn read(path: &Path) -> Option<GpuRecord> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(r) => Some(r),
        Err(_) => {
            let _ = remove(path);
            None
        }
    }
}

/// Written whole or not at all, and flushed: a driver crash can take the
/// whole machine down with it.
fn write(path: &Path, record: &GpuRecord) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(serde_json::to_string(record)?.as_bytes())?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)
}

fn remove(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Keeps ggml from registering its Vulkan backend, so nothing in this
/// process loads the graphics driver.
fn disable_vulkan() {
    // ggml reads this with the C runtime's getenv, which never sees
    // SetEnvironmentVariable (std::env::set_var). _putenv_s updates both.
    #[cfg(windows)]
    {
        extern "C" {
            fn _putenv_s(name: *const std::ffi::c_char, value: *const std::ffi::c_char) -> i32;
        }
        // SAFETY: both arguments are NUL-terminated literals.
        unsafe { _putenv_s(c"GGML_DISABLE_VULKAN".as_ptr(), c"1".as_ptr()) };
    }
    #[cfg(not(windows))]
    std::env::set_var("GGML_DISABLE_VULKAN", "1");
}

/// Decided once per process, on first use.
pub fn choice() -> &'static GpuChoice {
    CHOICE.get_or_init(|| {
        let (marker, disabled) = (paths::gpu_marker_file(), paths::gpu_disabled_file());
        let plan = gpu_plan(read(&marker), read(&disabled), VERSION);
        // The marker goes only once the disabled record is safely written.
        let recorded = plan.write_disabled.as_ref().map_or(Ok(()), |r| write(&disabled, r));
        if let Err(e) = &recorded {
            log::warn!("could not record the GPU crash: {e}");
        }
        if plan.remove_marker && recorded.is_ok() {
            let _ = remove(&marker);
        }
        if plan.remove_disabled {
            let _ = remove(&disabled);
        }
        if let GpuChoice::CpuAfterCrash { crash } = &plan.choice {
            log::warn!("{} {} crashed while starting the GPU; using the CPU", crash.engine, crash.version);
            disable_vulkan();
        }
        plan.choice
    })
}

/// Whether engines should start on the GPU.
pub fn use_gpu() -> bool {
    cfg!(feature = "vulkan") && *choice() == GpuChoice::Gpu
}

/// The crash that moved this process to the CPU, if any.
pub fn fallback() -> Option<GpuRecord> {
    match choice() {
        GpuChoice::CpuAfterCrash { crash } => Some(crash.clone()),
        GpuChoice::Gpu => None,
    }
}

/// Forget the crash. Takes effect at the next launch, since ggml's backend
/// list is fixed for the life of the process.
pub fn retry() -> io::Result<()> {
    remove(&paths::gpu_disabled_file())?;
    remove(&paths::gpu_marker_file())
}

/// Hold for the whole GPU start; the marker is removed when the last one
/// drops, on success or error.
pub struct GpuStart(bool);

pub fn start(engine: &str) -> GpuStart {
    if !use_gpu() {
        return GpuStart(false);
    }
    let mut starting = STARTING.lock().unwrap_or_else(|e| e.into_inner());
    if *starting == 0 {
        if let Err(e) = write(&paths::gpu_marker_file(), &GpuRecord::new(VERSION, engine)) {
            log::warn!("could not write the GPU start marker: {e}");
        }
    }
    *starting += 1;
    drop(starting);
    #[cfg(debug_assertions)]
    if std::env::var_os("QUILL_SIMULATE_GPU_CRASH").is_some() {
        eprintln!("QUILL_SIMULATE_GPU_CRASH: aborting during the {engine} GPU start");
        std::process::abort();
    }
    GpuStart(true)
}

impl Drop for GpuStart {
    fn drop(&mut self) {
        if !self.0 {
            return;
        }
        let mut starting = STARTING.lock().unwrap_or_else(|e| e.into_inner());
        *starting -= 1;
        if *starting == 0 {
            let _ = remove(&paths::gpu_marker_file());
        }
    }
}
