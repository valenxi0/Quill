//! Transcript history (section 4.10): final text only, deleted 24 hours
//! after it was written. Stored in `history.json`.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::engines::EngineId;

pub const TTL_HOURS: i64 = 24;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: Uuid,
    pub text: String,
    pub engine: EngineId,
    pub app_name: String,
    pub app_exe: String,
    pub seconds_spoken: f64,
    pub release_to_paste_ms: u64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct History {
    pub entries: Vec<HistoryEntry>,
}

impl History {
    pub fn add(&mut self, entry: HistoryEntry) {
        self.entries.push(entry);
    }

    /// Drop expired records; returns whether anything was removed.
    pub fn purge(&mut self, now: DateTime<Utc>) -> bool {
        let before = self.entries.len();
        let ttl = Duration::hours(TTL_HOURS);
        self.entries.retain(|e| now - e.created_at < ttl);
        self.entries.len() != before
    }

    /// When the oldest record expires, to schedule the next purge.
    pub fn next_expiry(&self) -> Option<DateTime<Utc>> {
        self.entries.iter().map(|e| e.created_at + Duration::hours(TTL_HOURS)).min()
    }

    /// Newest first.
    pub fn latest(&self, n: usize) -> Vec<HistoryEntry> {
        let mut v = self.entries.clone();
        v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        v.truncate(n);
        v
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{load_json, save_json};

    fn entry(text: &str, at: DateTime<Utc>) -> HistoryEntry {
        HistoryEntry {
            id: Uuid::new_v4(),
            text: text.into(),
            engine: EngineId::WhisperTiny,
            app_name: "Notepad".into(),
            app_exe: "notepad.exe".into(),
            seconds_spoken: 2.5,
            release_to_paste_ms: 180,
            created_at: at,
        }
    }

    #[test]
    fn expiry() {
        let now = Utc::now();
        let mut h = History::default();
        h.add(entry("old", now - Duration::hours(25)));
        h.add(entry("recent", now - Duration::hours(1)));
        h.add(entry("new", now));
        assert_eq!(h.next_expiry(), Some(now - Duration::hours(1)));
        assert!(h.purge(now));
        assert_eq!(h.latest(20).iter().map(|e| e.text.as_str()).collect::<Vec<_>>(), vec!["new", "recent"]);
        assert!(!h.purge(now));
        assert_eq!(h.next_expiry(), Some(now + Duration::hours(23)));
        assert!(h.purge(now + Duration::hours(24)));
        assert!(h.entries.is_empty());
    }

    #[test]
    fn persists_in_a_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        let mut h = History::default();
        h.add(entry("hello", Utc::now()));
        save_json(&path, &h).unwrap();
        let loaded: History = load_json(&path);
        assert_eq!(loaded, h);
    }
}
