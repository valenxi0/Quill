//! Voice insights (section 4.10): counts only, never text. Stored in
//! `stats.json`.

use std::collections::{BTreeMap, HashMap, VecDeque};

use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::text::tokens::word_count;

const FILLERS: &[&str] = &["um", "umm", "uh", "uhh", "uhm", "er", "erm", "hmm", "mm", "ah"];
pub const TYPING_WPM: f64 = 40.0;
const MAX_LATENCIES: usize = 200;

pub fn filler_count(text: &str) -> u64 {
    text.split_whitespace()
        .filter(|w| {
            let w = w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
            FILLERS.contains(&w.as_str())
        })
        .count() as u64
}

/// Typing time at 40 wpm minus speaking time, in seconds, never below 0.
pub fn time_saved_seconds(words: u64, seconds_spoken: f64) -> f64 {
    (words as f64 / TYPING_WPM * 60.0 - seconds_spoken).max(0.0)
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DayStats {
    pub words: u64,
    pub seconds: f64,
    pub sessions: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppStats {
    pub name: String,
    pub words: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stats {
    /// Keyed by local `yyyy-MM-dd`.
    pub days: BTreeMap<String, DayStats>,
    /// Keyed by `yyyy-MM`, then by exe.
    pub months: BTreeMap<String, BTreeMap<String, AppStats>>,
    pub fillers_removed: u64,
    pub dictionary_hits: HashMap<Uuid, u64>,
    /// Release-to-paste seconds, newest last.
    pub latencies: VecDeque<f64>,
    pub since: Option<NaiveDate>,
}

/// One finished dictation, as the stats see it.
pub struct Session<'a> {
    pub date: NaiveDate,
    pub raw: &'a str,
    pub text: &'a str,
    pub seconds: f64,
    pub app_exe: &'a str,
    pub app_name: &'a str,
    pub hits: &'a [Uuid],
    pub latency: Option<f64>,
}

fn day_key(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn month_key(d: NaiveDate) -> String {
    d.format("%Y-%m").to_string()
}

impl Stats {
    pub fn record(&mut self, s: Session) {
        let words = word_count(s.text) as u64;
        let day = self.days.entry(day_key(s.date)).or_default();
        day.words += words;
        day.seconds += s.seconds;
        day.sessions += 1;
        let app = self
            .months
            .entry(month_key(s.date))
            .or_default()
            .entry(s.app_exe.to_lowercase())
            .or_default();
        app.name = s.app_name.to_string();
        app.words += words;
        self.fillers_removed += filler_count(s.raw).saturating_sub(filler_count(s.text));
        for id in s.hits {
            *self.dictionary_hits.entry(*id).or_default() += 1;
        }
        if let Some(l) = s.latency {
            self.latencies.push_back(l);
            while self.latencies.len() > MAX_LATENCIES {
                self.latencies.pop_front();
            }
        }
        self.since = Some(self.since.map_or(s.date, |d| d.min(s.date)));
    }

    pub fn is_empty(&self) -> bool {
        self.days.values().all(|d| d.sessions == 0)
    }

    fn day(&self, d: NaiveDate) -> Option<&DayStats> {
        self.days.get(&day_key(d)).filter(|s| s.sessions > 0)
    }

    pub fn median_latency(&self) -> Option<f64> {
        if self.latencies.is_empty() {
            return None;
        }
        let mut v: Vec<f64> = self.latencies.iter().copied().collect();
        v.sort_by(|a, b| a.total_cmp(b));
        let n = v.len();
        Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
    }

    /// Current streak (ending today, or yesterday if today has none yet) and
    /// the longest streak.
    pub fn streaks(&self, today: NaiveDate) -> (u32, u32) {
        let mut current = 0;
        let mut d = if self.day(today).is_some() { today } else { today - Duration::days(1) };
        while self.day(d).is_some() {
            current += 1;
            d -= Duration::days(1);
        }
        let mut longest = 0;
        let mut run = 0;
        let mut prev: Option<NaiveDate> = None;
        for key in self.days.keys() {
            let Ok(date) = NaiveDate::parse_from_str(key, "%Y-%m-%d") else { continue };
            if self.day(date).is_none() {
                continue;
            }
            run = if prev.is_some_and(|p| p + Duration::days(1) == date) { run + 1 } else { 1 };
            longest = longest.max(run);
            prev = Some(date);
        }
        (current, longest.max(current))
    }

    pub fn insights(&self, today: NaiveDate) -> Insights {
        let monday = today - Duration::days(today.weekday().num_days_from_monday() as i64);
        let (mut week_words, mut week_seconds) = (0, 0.0);
        let (mut month_words, mut month_seconds) = (0, 0.0);
        let (mut total_words, mut total_seconds) = (0, 0.0);
        let this_month = month_key(today);
        for (key, s) in &self.days {
            let Ok(date) = NaiveDate::parse_from_str(key, "%Y-%m-%d") else { continue };
            total_words += s.words;
            total_seconds += s.seconds;
            if date >= monday && date <= today {
                week_words += s.words;
                week_seconds += s.seconds;
            }
            if month_key(date) == this_month {
                month_words += s.words;
                month_seconds += s.seconds;
            }
        }
        let wpm = |w: u64, s: f64| if s > 0.0 { w as f64 / (s / 60.0) } else { 0.0 };
        let sparkline: Vec<f64> = self
            .days
            .values()
            .filter(|d| d.sessions > 0 && d.seconds > 0.0)
            .map(|d| wpm(d.words, d.seconds))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .take(14)
            .rev()
            .collect();
        let mut apps: Vec<AppInsight> = self
            .months
            .get(&this_month)
            .map(|m| {
                m.iter()
                    .map(|(exe, a)| AppInsight { exe: exe.clone(), name: a.name.clone(), words: a.words })
                    .collect()
            })
            .unwrap_or_default();
        apps.sort_by(|a, b| b.words.cmp(&a.words));

        // 15 weeks, Monday-first, ending with the current week.
        let first = monday - Duration::weeks(14);
        let heatmap: Vec<HeatCell> = (0..15 * 7)
            .map(|i| {
                let date = first + Duration::days(i);
                let words = self.day(date).map_or(0, |d| d.words);
                HeatCell { date: day_key(date), words, future: date > today }
            })
            .collect();
        let (current_streak, longest_streak) = self.streaks(today);
        Insights {
            is_empty: self.is_empty(),
            week_words,
            week_saved_seconds: time_saved_seconds(week_words, week_seconds),
            wpm: wpm(total_words, total_seconds),
            sparkline,
            month_saved_seconds: time_saved_seconds(month_words, month_seconds),
            total_words,
            since: self.since.map(day_key),
            apps,
            current_streak,
            longest_streak,
            heatmap,
            fillers_removed: self.fillers_removed,
            dictionary_expansions: self.dictionary_hits.values().sum(),
            median_latency: self.median_latency(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInsight {
    pub exe: String,
    pub name: String,
    pub words: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeatCell {
    pub date: String,
    pub words: u64,
    pub future: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Insights {
    pub is_empty: bool,
    pub week_words: u64,
    pub week_saved_seconds: f64,
    pub wpm: f64,
    pub sparkline: Vec<f64>,
    pub month_saved_seconds: f64,
    pub total_words: u64,
    pub since: Option<String>,
    pub apps: Vec<AppInsight>,
    pub current_streak: u32,
    pub longest_streak: u32,
    pub heatmap: Vec<HeatCell>,
    pub fillers_removed: u64,
    pub dictionary_expansions: u64,
    pub median_latency: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn session<'a>(date: &str, raw: &'a str, text: &'a str, secs: f64, latency: f64) -> Session<'a> {
        Session {
            date: d(date),
            raw,
            text,
            seconds: secs,
            app_exe: "Notepad.exe",
            app_name: "Notepad",
            hits: &[],
            latency: Some(latency),
        }
    }

    #[test]
    fn fillers() {
        assert_eq!(filler_count("Um, so uh I think, erm... hmm"), 4);
        let mut s = Stats::default();
        s.record(session("2026-09-21", "um so uh yes", "So yes.", 2.0, 0.3));
        assert_eq!(s.fillers_removed, 2);
        s.record(session("2026-09-21", "yes", "Um, yes.", 1.0, 0.3));
        assert_eq!(s.fillers_removed, 2, "never below zero");
    }

    #[test]
    fn streaks() {
        let mut s = Stats::default();
        for day in ["2026-09-01", "2026-09-02", "2026-09-03", "2026-09-10", "2026-09-11"] {
            s.record(session(day, "a b", "a b", 1.0, 0.2));
        }
        assert_eq!(s.streaks(d("2026-09-11")), (2, 3));
        assert_eq!(s.streaks(d("2026-09-12")), (2, 3), "yesterday still counts");
        assert_eq!(s.streaks(d("2026-09-13")), (0, 3));
    }

    #[test]
    fn median() {
        let mut s = Stats::default();
        assert_eq!(s.median_latency(), None);
        for l in [0.5, 0.1, 0.3] {
            s.record(session("2026-09-01", "", "x", 1.0, l));
        }
        assert_eq!(s.median_latency(), Some(0.3));
        s.record(session("2026-09-01", "", "x", 1.0, 0.9));
        assert_eq!(s.median_latency(), Some(0.4));
        for _ in 0..300 {
            s.record(session("2026-09-01", "", "x", 1.0, 1.0));
        }
        assert_eq!(s.latencies.len(), 200);
    }

    #[test]
    fn time_saved() {
        // 80 words take 2 minutes to type; spoken in 30 s saves 90 s.
        assert_eq!(time_saved_seconds(80, 30.0), 90.0);
        assert_eq!(time_saved_seconds(2, 30.0), 0.0);
    }

    #[test]
    fn insights_summary() {
        let mut s = Stats::default();
        let text = "word ".repeat(80);
        s.record(session("2026-09-21", &text, &text, 30.0, 0.2)); // Monday
        s.record(session("2026-09-20", "a b", "a b", 3.0, 0.2)); // Sunday, last week
        let i = s.insights(d("2026-09-25"));
        assert_eq!(i.week_words, 80);
        assert_eq!(i.week_saved_seconds, 90.0);
        assert_eq!(i.total_words, 82);
        assert_eq!(i.since.as_deref(), Some("2026-09-20"));
        assert_eq!(i.apps[0].words, 82);
        assert_eq!(i.heatmap.len(), 105);
        assert_eq!(i.heatmap.iter().filter(|c| c.future).count(), 2);
        assert_eq!(i.current_streak, 0);
        assert_eq!(i.longest_streak, 2);
    }
}
