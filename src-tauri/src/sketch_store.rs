//! Saved Sketch screenshots, in `%LOCALAPPDATA%\Quill\Sketches`, one folder
//! per Sketch, kept for a day so an agent can open them from the pasted text.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use quill_core::sketch::{render::encode_png, Image};

const KEEP: Duration = Duration::from_secs(24 * 60 * 60);

/// One image to paste and save.
pub struct Shot {
    pub name: String,
    pub image: Image,
    pub png: Vec<u8>,
}

pub struct Saved {
    pub folder: PathBuf,
    /// In the order given.
    pub images: Vec<PathBuf>,
}

/// Writes `sheet.png` and every shot into a new folder, and clears out
/// folders older than a day. `None` if the folder can't be written.
pub fn save(sheet: Option<&Image>, shots: &[Shot]) -> Option<Saved> {
    let root = crate::paths::sketches_dir();
    purge(&root);
    let folder = root.join(format!("sketch-{}", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    std::fs::create_dir_all(&folder).ok()?;
    if let Some(sheet) = sheet {
        std::fs::write(folder.join("sheet.png"), encode_png(sheet)).ok()?;
    }
    let mut images = Vec::new();
    for shot in shots {
        let path = folder.join(&shot.name);
        std::fs::write(&path, &shot.png).ok()?;
        images.push(path);
    }
    Some(Saved { folder, images })
}

fn purge(root: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    let cutoff = SystemTime::now() - KEEP;
    for entry in entries.flatten() {
        let old = entry.metadata().and_then(|m| m.modified()).is_ok_and(|t| t < cutoff);
        if old && entry.file_name().to_string_lossy().starts_with("sketch-") {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}
