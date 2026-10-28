//! Where Quill keeps things. Settings roam (`%APPDATA%`); models are large
//! and must not (`%LOCALAPPDATA%`).

use std::path::PathBuf;

fn env_dir(var: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// `%APPDATA%\Quill`: settings, history, stats, corrections.
pub fn config_dir() -> PathBuf {
    env_dir("APPDATA").join("Quill")
}

/// `%LOCALAPPDATA%\Quill\Models`.
pub fn models_dir() -> PathBuf {
    env_dir("LOCALAPPDATA").join("Quill").join("Models")
}

/// `%LOCALAPPDATA%\Quill\Sketches`: Sketch screenshots, kept for a day.
pub fn sketches_dir() -> PathBuf {
    env_dir("LOCALAPPDATA").join("Quill").join("Sketches")
}

/// `%LOCALAPPDATA%\Quill\Eval`: benchmark clips.
pub fn eval_dir() -> PathBuf {
    env_dir("LOCALAPPDATA").join("Quill").join("Eval")
}

pub fn settings_file() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn history_file() -> PathBuf {
    config_dir().join("history.json")
}

pub fn stats_file() -> PathBuf {
    config_dir().join("stats.json")
}

pub fn corrections_file() -> PathBuf {
    config_dir().join("corrections.json")
}
