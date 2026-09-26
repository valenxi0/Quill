//! In-memory copies of the JSON stores, saved atomically on every change.

use std::path::PathBuf;

use parking_lot::Mutex;
use quill_core::corrections::Corrections;
use quill_core::history::History;
use quill_core::settings::Settings;
use quill_core::stats::Stats;
use quill_core::store::{load_json, save_json};
use serde::de::DeserializeOwned;
use serde::Serialize;

pub struct Store<T> {
    path: PathBuf,
    value: Mutex<T>,
}

impl<T: Serialize + DeserializeOwned + Default + Clone> Store<T> {
    pub fn open(path: PathBuf) -> Self {
        let value = load_json(&path);
        Self { path, value: Mutex::new(value) }
    }

    pub fn get(&self) -> T {
        self.value.lock().clone()
    }

    pub fn read<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&self.value.lock())
    }

    /// Change and save. The lock is held across the write so saves never
    /// interleave.
    pub fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let mut v = self.value.lock();
        let r = f(&mut v);
        if let Err(e) = save_json(&self.path, &*v) {
            log::warn!("could not save {}: {e}", self.path.display());
        }
        r
    }
}

pub struct Stores {
    pub settings: Store<Settings>,
    pub history: Store<History>,
    pub stats: Store<Stats>,
    pub corrections: Store<Corrections>,
}

impl Stores {
    pub fn open() -> Self {
        Self {
            settings: Store::open(crate::paths::settings_file()),
            history: Store::open(crate::paths::history_file()),
            stats: Store::open(crate::paths::stats_file()),
            corrections: Store::open(crate::paths::corrections_file()),
        }
    }
}
