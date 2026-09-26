//! Atomic JSON persistence shared by every store.

use std::fs;
use std::io::Write;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Write to a sibling temp file, flush it to disk, then rename over the
/// target, so readers see either the old file or the new one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp-{}",
        path.extension().and_then(|e| e.to_str()).unwrap_or(""),
        std::process::id()
    ));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

pub fn save_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    write_atomic(path, &bytes)
}

/// Load a JSON file, or the default when it is missing or unreadable.
pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("x.json");
        save_json(&path, &vec![1, 2]).unwrap();
        save_json(&path, &vec![3]).unwrap();
        let v: Vec<i32> = load_json(&path);
        assert_eq!(v, vec![3]);
        let names: Vec<_> = fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(names.len(), 1, "no temp files left behind");
        let missing: Vec<i32> = load_json(&dir.path().join("none.json"));
        assert!(missing.is_empty());
    }
}
