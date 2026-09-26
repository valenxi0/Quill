//! Downloading, verifying, and installing pinned models (section 2.1).
//!
//! Files download into `.{asset}.partial`, each checked against its pinned
//! size and SHA-256 while it streams. Only when every file matches is the
//! receipt written and the folder renamed into place, so a model is either
//! complete or absent. A retry skips files that already verified.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::Arc;
use std::time::Instant;

use futures_util::StreamExt;
use parking_lot::Mutex;
use quill_core::engines::AssetId;
use quill_core::models::{asset, checksum_mismatch, installed_state, Asset, ModelFile, ModelState, Receipt, RECEIPT_FILE};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

pub struct ModelManager {
    root: PathBuf,
    states: Mutex<HashMap<AssetId, ModelState>>,
    cancels: Mutex<HashMap<AssetId, Arc<AtomicBool>>>,
    on_change: Box<dyn Fn() + Send + Sync>,
    /// Called once when an asset finishes installing.
    on_installed: Mutex<Option<Box<dyn Fn(AssetId) + Send + Sync>>>,
}

impl ModelManager {
    pub fn new(root: PathBuf, on_change: impl Fn() + Send + Sync + 'static) -> Arc<Self> {
        let m = Arc::new(Self {
            root,
            states: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            on_change: Box::new(on_change),
            on_installed: Mutex::new(None),
        });
        m.refresh();
        m
    }

    pub fn set_on_installed(&self, f: impl Fn(AssetId) + Send + Sync + 'static) {
        *self.on_installed.lock() = Some(Box::new(f));
    }

    pub fn dir(&self, id: AssetId) -> PathBuf {
        self.root.join(id.key())
    }

    fn partial_dir(&self, id: AssetId) -> PathBuf {
        self.root.join(format!(".{}.partial", id.key()))
    }

    pub fn file_path(&self, id: AssetId, file: &str) -> PathBuf {
        self.dir(id).join(file)
    }

    fn read_receipt(&self, id: AssetId) -> Option<Receipt> {
        let bytes = std::fs::read(self.dir(id).join(RECEIPT_FILE)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Re-read every asset's state from disk, keeping in-flight downloads.
    pub fn refresh(&self) {
        let mut states = self.states.lock();
        for id in AssetId::ALL {
            if states.get(&id).is_some_and(|s| s.is_busy()) {
                continue;
            }
            let s = installed_state(asset(id), self.read_receipt(id).as_ref());
            // Keep a failure visible until the user retries.
            if matches!(states.get(&id), Some(ModelState::Failed { .. })) && s == ModelState::NotDownloaded {
                continue;
            }
            states.insert(id, s);
        }
    }

    pub fn state(&self, id: AssetId) -> ModelState {
        self.states.lock().get(&id).cloned().unwrap_or(ModelState::NotDownloaded)
    }

    pub fn is_ready(&self, id: AssetId) -> bool {
        self.state(id).is_ready()
    }

    /// Something is on disk: a partial download or an installed folder.
    pub fn has_data(&self, id: AssetId) -> bool {
        self.partial_dir(id).exists() || self.dir(id).exists()
    }

    fn set(&self, id: AssetId, s: ModelState) {
        self.states.lock().insert(id, s);
        (self.on_change)();
    }

    pub fn cancel(&self, id: AssetId) {
        if let Some(flag) = self.cancels.lock().get(&id) {
            flag.store(true, SeqCst);
        }
    }

    pub fn remove(&self, id: AssetId) -> std::io::Result<()> {
        self.cancel(id);
        for dir in [self.dir(id), self.partial_dir(id)] {
            if dir.exists() {
                std::fs::remove_dir_all(&dir)?;
            }
        }
        self.states.lock().insert(id, installed_state(asset(id), None));
        (self.on_change)();
        Ok(())
    }

    /// Download and install one asset. Returns when done, failed, or cancelled.
    pub async fn download(self: Arc<Self>, id: AssetId) {
        let a = asset(id);
        if !a.available || self.state(id).is_busy() {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancels.lock().insert(id, cancel.clone());
        let result = self.clone().install(a, &cancel).await;
        self.cancels.lock().remove(&id);
        match result {
            Ok(()) => {
                self.set(id, ModelState::Downloaded);
                if let Some(f) = self.on_installed.lock().as_ref() {
                    f(id);
                }
            }
            Err(Failure::Cancelled) => {
                let s = installed_state(a, self.read_receipt(id).as_ref());
                self.set(id, s);
            }
            Err(Failure::Message(message)) => {
                log::warn!("download of {} failed: {message}", id.key());
                self.set(id, ModelState::Failed { message });
            }
        }
    }

    async fn install(self: Arc<Self>, a: &'static Asset, cancel: &AtomicBool) -> Result<(), Failure> {
        let partial = self.partial_dir(a.id);
        tokio::fs::create_dir_all(&partial).await.map_err(io)?;
        let total = a.total_bytes();
        let client = reqwest::Client::builder()
            .user_agent(concat!("Quill/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| Failure::Message(e.to_string()))?;

        let mut done = 0u64;
        self.set(a.id, ModelState::Downloading { fraction: 0.0, bytes: 0, total });
        for file in a.files {
            let dest = partial.join(file.path);
            if verify_existing(&dest, file).await {
                done += file.size;
                continue;
            }
            self.fetch(&client, a, file, &dest, cancel, done, total).await?;
            done += file.size;
        }

        self.set(a.id, ModelState::Verifying);
        let receipt = serde_json::to_vec(&Receipt { revision: a.revision.to_string() }).map_err(|e| Failure::Message(e.to_string()))?;
        tokio::fs::write(partial.join(RECEIPT_FILE), receipt).await.map_err(io)?;
        let dir = self.dir(a.id);
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir).await.map_err(io)?;
        }
        tokio::fs::rename(&partial, &dir).await.map_err(io)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn fetch(
        &self,
        client: &reqwest::Client,
        a: &Asset,
        file: &ModelFile,
        dest: &Path,
        cancel: &AtomicBool,
        done: u64,
        total: u64,
    ) -> Result<(), Failure> {
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(io)?;
        }
        let part = dest.with_extension("part");
        let response = client
            .get(a.url(file))
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| Failure::Message(format!("Could not download {}: {e}", file.path)))?;
        let mut out = tokio::fs::File::create(&part).await.map_err(io)?;
        let mut hasher = Sha256::new();
        let mut received = 0u64;
        let mut last_emit = Instant::now();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            if cancel.load(SeqCst) {
                drop(out);
                let _ = tokio::fs::remove_file(&part).await;
                return Err(Failure::Cancelled);
            }
            let chunk = chunk.map_err(|e| Failure::Message(format!("Download of {} was interrupted: {e}", file.path)))?;
            hasher.update(&chunk);
            out.write_all(&chunk).await.map_err(io)?;
            received += chunk.len() as u64;
            if received > file.size {
                break;
            }
            if last_emit.elapsed().as_millis() >= 100 {
                last_emit = Instant::now();
                let bytes = done + received;
                self.set(a.id, ModelState::Downloading { fraction: bytes as f64 / total as f64, bytes, total });
            }
        }
        out.flush().await.map_err(io)?;
        drop(out);
        let digest = hex::encode(hasher.finalize());
        if received != file.size || digest != file.sha256 {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(Failure::Message(checksum_mismatch(file.path)));
        }
        tokio::fs::rename(&part, dest).await.map_err(io)?;
        Ok(())
    }
}

enum Failure {
    Cancelled,
    Message(String),
}

fn io(e: std::io::Error) -> Failure {
    Failure::Message(e.to_string())
}

/// A file left by an earlier attempt counts only if it matches exactly.
async fn verify_existing(path: &Path, file: &ModelFile) -> bool {
    let Ok(meta) = tokio::fs::metadata(path).await else { return false };
    if meta.len() != file.size {
        return false;
    }
    let path = path.to_path_buf();
    let want = file.sha256;
    tokio::task::spawn_blocking(move || {
        let Ok(mut f) = std::fs::File::open(&path) else { return false };
        let mut hasher = Sha256::new();
        if std::io::copy(&mut f, &mut hasher).is_err() {
            return false;
        }
        hex::encode(hasher.finalize()) == want
    })
    .await
    .unwrap_or(false)
}
