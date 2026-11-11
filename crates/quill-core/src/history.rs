//! Transcript history (section 4.10): the raw and final text, deleted a
//! day, a week, or a month after it was written. Stored in `history.json`.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::context::AppContext;
use crate::engines::EngineId;
use crate::settings::CleanupLevel;

/// The most records kept; the oldest go first.
pub const MAX_ENTRIES: usize = 10_000;
/// The most records one history request returns.
pub const MAX_RESULTS: usize = 200;

/// How long a record is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Retention {
    #[default]
    Day,
    Week,
    Month,
}

impl Retention {
    pub fn duration(self) -> Duration {
        match self {
            Retention::Day => Duration::hours(24),
            Retention::Week => Duration::days(7),
            Retention::Month => Duration::days(30),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: Uuid,
    pub text: String,
    /// What the recognizer heard, before cleanup. Empty in older records.
    #[serde(default)]
    pub raw: String,
    /// The cleanup style used; `None` in older records.
    #[serde(default)]
    pub level: Option<CleanupLevel>,
    #[serde(default)]
    pub context: AppContext,
    pub engine: EngineId,
    pub app_name: String,
    pub app_exe: String,
    pub seconds_spoken: f64,
    pub release_to_paste_ms: u64,
    pub created_at: DateTime<Utc>,
    /// Why cleanup kept some or all of the words as heard; `None` when it
    /// didn't, and in older records.
    #[serde(default)]
    pub cleanup_note: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct History {
    pub entries: Vec<HistoryEntry>,
}

impl History {
    pub fn add(&mut self, entry: HistoryEntry) {
        self.entries.push(entry);
        if self.entries.len() > MAX_ENTRIES {
            self.entries.sort_by_key(|e| e.created_at);
            let excess = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(..excess);
        }
    }

    /// Drop expired records; returns whether anything was removed.
    pub fn purge(&mut self, now: DateTime<Utc>, keep: Retention) -> bool {
        let before = self.entries.len();
        let ttl = keep.duration();
        self.entries.retain(|e| now - e.created_at < ttl);
        self.entries.len() != before
    }

    /// When the oldest record expires, to schedule the next purge.
    pub fn next_expiry(&self, keep: Retention) -> Option<DateTime<Utc>> {
        self.entries.iter().map(|e| e.created_at + keep.duration()).min()
    }

    /// Records whose text, heard words, or app name contain every word of
    /// `query`, ignoring case; newest first, at most `limit` (capped at
    /// [`MAX_RESULTS`]). An empty query matches everything.
    pub fn search(&self, query: &str, limit: usize) -> Vec<HistoryEntry> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut found: Vec<&HistoryEntry> = self
            .entries
            .iter()
            .filter(|e| {
                if words.is_empty() {
                    return true;
                }
                let hay = format!("{}\n{}\n{}", e.text, e.raw, e.app_name).to_lowercase();
                words.iter().all(|w| hay.contains(w.as_str()))
            })
            .collect();
        found.sort_by_key(|e| std::cmp::Reverse(e.created_at));
        found.into_iter().take(limit.min(MAX_RESULTS)).cloned().collect()
    }

    /// How many records a purge with `keep` would delete now.
    pub fn expiring(&self, now: DateTime<Utc>, keep: Retention) -> usize {
        self.entries.iter().filter(|e| now - e.created_at >= keep.duration()).count()
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
            raw: text.to_lowercase(),
            level: Some(CleanupLevel::Full),
            context: AppContext::General,
            engine: EngineId::WhisperTiny,
            app_name: "Notepad".into(),
            app_exe: "notepad.exe".into(),
            seconds_spoken: 2.5,
            release_to_paste_ms: 180,
            created_at: at,
            cleanup_note: None,
        }
    }

    #[test]
    fn expiry() {
        let now = Utc::now();
        let mut h = History::default();
        h.add(entry("old", now - Duration::hours(25)));
        h.add(entry("recent", now - Duration::hours(1)));
        h.add(entry("new", now));
        assert_eq!(h.next_expiry(Retention::Day), Some(now - Duration::hours(1)));
        assert!(h.purge(now, Retention::Day));
        assert_eq!(h.latest(20).iter().map(|e| e.text.as_str()).collect::<Vec<_>>(), vec!["new", "recent"]);
        assert!(!h.purge(now, Retention::Day));
        assert_eq!(h.next_expiry(Retention::Day), Some(now + Duration::hours(23)));
        assert!(h.purge(now + Duration::hours(24), Retention::Day));
        assert!(h.entries.is_empty());
    }

    fn oldest_first(h: &History) -> Vec<&str> {
        let mut v: Vec<&HistoryEntry> = h.entries.iter().collect();
        v.sort_by_key(|e| e.created_at);
        v.into_iter().map(|e| e.text.as_str()).collect()
    }

    #[test]
    fn retention_choices() {
        let now = Utc::now();
        let ages = [("forty days", Duration::days(40)), ("ten days", Duration::days(10)), ("two days", Duration::days(2)), ("hour", Duration::hours(1))];
        let filled = || {
            let mut h = History::default();
            for (text, age) in ages {
                h.add(entry(text, now - age));
            }
            h
        };
        let mut day = filled();
        assert!(day.purge(now, Retention::Day));
        assert_eq!(oldest_first(&day), vec!["hour"]);
        let mut week = filled();
        assert_eq!(week.next_expiry(Retention::Week), Some(now - Duration::days(33)));
        assert!(week.purge(now, Retention::Week));
        assert_eq!(oldest_first(&week), vec!["two days", "hour"]);
        assert_eq!(week.next_expiry(Retention::Week), Some(now + Duration::days(5)));
        let mut month = filled();
        assert_eq!(month.expiring(now, Retention::Month), 1);
        assert_eq!(month.expiring(now, Retention::Week), 2);
        assert_eq!(month.expiring(now, Retention::Day), 3);
        assert!(month.purge(now, Retention::Month));
        assert_eq!(oldest_first(&month), vec!["ten days", "two days", "hour"]);
        assert!(!month.purge(now, Retention::Month));
        assert!(month.purge(now + Duration::days(29), Retention::Month));
        assert_eq!(oldest_first(&month), vec!["hour"]);
        assert_eq!(Retention::default(), Retention::Day);
        assert_eq!(serde_json::to_string(&Retention::Month).unwrap(), r#""month""#);
    }

    #[test]
    fn keeps_at_most_ten_thousand_records() {
        let now = Utc::now();
        let mut h = History::default();
        for i in 0..MAX_ENTRIES {
            h.add(entry(&format!("n{i}"), now - Duration::seconds((MAX_ENTRIES - i) as i64)));
        }
        h.add(entry("newest", now));
        assert_eq!(h.entries.len(), MAX_ENTRIES);
        assert!(!h.entries.iter().any(|e| e.text == "n0"), "the oldest went");
        assert!(h.entries.iter().any(|e| e.text == "n1") && h.entries.iter().any(|e| e.text == "newest"));
    }

    #[test]
    fn search_matches_text_heard_words_and_app() {
        let now = Utc::now();
        let mut h = History::default();
        h.add(entry("Ship the build today.", now - Duration::minutes(3)));
        let mut slack = entry("Lunch at noon?", now - Duration::minutes(2));
        slack.app_name = "Slack".into();
        slack.raw = "um lunch at noon".into();
        h.add(slack);
        h.add(entry("The BUILD is green.", now - Duration::minutes(1)));
        let found = |q: &str| h.search(q, 20).into_iter().map(|e| e.text).collect::<Vec<_>>();
        assert_eq!(found("build"), vec!["The BUILD is green.", "Ship the build today."], "newest first, any case");
        assert_eq!(found("slack"), vec!["Lunch at noon?"], "app name");
        assert_eq!(found("um lunch"), vec!["Lunch at noon?"], "heard words");
        assert_eq!(found("green build"), vec!["The BUILD is green."], "every word");
        assert!(found("nothing like this").is_empty());
        assert_eq!(found("  ").len(), 3, "blank matches all");
        assert_eq!(h.search("", 1).len(), 1);
        for i in 0..300 {
            h.add(entry(&format!("build {i}"), now));
        }
        assert_eq!(h.search("build", 1000).len(), MAX_RESULTS);
    }

    #[test]
    fn old_history_files_load_and_purge_after_a_day() {
        let old = r#"{"entries":[{"id":"6f2c5a8e-3d1b-4c7a-9e0f-1a2b3c4d5e6f","text":"Ship it today.","engine":"parakeetS1","appName":"Notepad","appExe":"notepad.exe","secondsSpoken":1.5,"releaseToPasteMs":200,"createdAt":"2026-11-09T10:00:00Z"}]}"#;
        let mut h: History = serde_json::from_str(old).unwrap();
        let written: DateTime<Utc> = "2026-11-09T10:00:00Z".parse().unwrap();
        assert!(!h.purge(written + Duration::hours(23), Retention::default()));
        assert!(h.purge(written + Duration::hours(24), Retention::default()));
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

    #[test]
    fn records_without_a_cleanup_note_still_load() {
        let old = r#"{"entries":[{"id":"6f2c5a8e-3d1b-4c7a-9e0f-1a2b3c4d5e6f","text":"Ship it today.","raw":"um ship it today","level":"full","context":"general","engine":"parakeetS1","appName":"Notepad","appExe":"notepad.exe","secondsSpoken":1.5,"releaseToPasteMs":200,"createdAt":"2026-11-09T10:00:00Z"}]}"#;
        let h: History = serde_json::from_str(old).unwrap();
        assert_eq!(h.entries[0].text, "Ship it today.");
        assert_eq!(h.entries[0].cleanup_note, None);
        let mut noted = h.entries[0].clone();
        noted.cleanup_note = Some("Kept your words: cleanup timed out.".into());
        let json = serde_json::to_string(&noted).unwrap();
        assert!(json.contains(r#""cleanupNote":"Kept your words: cleanup timed out.""#), "{json}");
    }
}
