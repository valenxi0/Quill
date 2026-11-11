use std::sync::atomic::Ordering::SeqCst;
use std::sync::Arc;
use std::time::Duration;

use quill_core::corrections::{diff, Anchor, CorrectionPair};
use quill_core::history::HistoryEntry;
use quill_core::pipeline::{cleanup_note, SegmentResult};
use quill_core::stats::Session as StatSession;

use crate::platform::uia;

use super::{Controller, Metrics, SessionConfig};

impl Controller {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn record(
        self: &Arc<Self>,
        cfg: &SessionConfig,
        results: &[SegmentResult],
        raw: &str,
        text: &str,
        metrics: &Metrics,
        release_to_paste: Duration,
        pasted: bool,
    ) {
        let settings = self.stores.settings.get();
        let vd = &settings.voice_dictation;
        let app_name = if cfg.target.app_name.is_empty() { "Unknown app".to_string() } else { cfg.target.app_name.clone() };
        if cfg.history_at_start && vd.history_enabled {
            self.stores.history.update(|h| {
                h.add(HistoryEntry {
                    id: uuid::Uuid::new_v4(),
                    text: text.to_string(),
                    raw: raw.to_string(),
                    level: Some(cfg.level),
                    context: cfg.recipe.context,
                    engine: cfg.engine,
                    app_name: app_name.clone(),
                    app_exe: cfg.target.exe.clone(),
                    seconds_spoken: metrics.audio_seconds,
                    release_to_paste_ms: release_to_paste.as_millis() as u64,
                    created_at: chrono::Utc::now(),
                    cleanup_note: cleanup_note(results),
                })
            });
            self.shell.history_changed();
        }
        if vd.insights_enabled {
            let icon = crate::platform::icons::file_icon(&cfg.target.exe_path);
            let hits: Vec<uuid::Uuid> = results.iter().flat_map(|r| r.hits.iter().copied()).collect();
            let latency = (!metrics.cold_start).then(|| release_to_paste.as_secs_f64());
            self.stores.stats.update(|s| {
                s.record(StatSession {
                    date: chrono::Local::now().date_naive(),
                    raw,
                    text,
                    seconds: metrics.audio_seconds,
                    app_exe: &cfg.target.exe,
                    app_name: &app_name,
                    app_icon: icon.as_deref(),
                    hits: &hits,
                    latency,
                })
            });
        }
        if pasted && vd.learns_corrections && !cfg.target.is_password {
            if let Some(id) = cfg.target.element_id {
                self.watch_corrections(id, text.to_string(), cfg.target.exe.clone());
            }
        }
    }

    fn watch_corrections(self: &Arc<Self>, element: u64, pasted: String, app_exe: String) {
        let watch = self.watch_generation.fetch_add(1, SeqCst) + 1;
        let me = self.clone();
        std::thread::spawn(move || {
            let read = move || uia::run(1500, move |u| u.read_text(element)).flatten();
            std::thread::sleep(Duration::from_millis(400));
            let Some(anchor) = read().and_then(|field| Anchor::new(&field, &pasted)) else { return };
            let mut latest: Option<String> = None;
            let mut waited = Duration::from_millis(400);
            for at in [Duration::from_secs(12), Duration::from_secs(45)] {
                std::thread::sleep(at - waited);
                waited = at;
                if me.watch_generation.load(SeqCst) != watch {
                    return;
                }
                match read() {
                    Some(field) if !field.trim().is_empty() => {
                        if let Some(region) = anchor.locate(&field) {
                            latest = Some(region.to_string());
                        }
                    }
                    _ => {}
                }
            }
            let _ = uia::run(500, move |u| u.forget(element));
            if me.watch_generation.load(SeqCst) != watch {
                return;
            }
            let Some(edited) = latest else { return };
            let fixes = diff(&anchor.pasted, &edited);
            if fixes.is_empty() {
                return;
            }
            let vocabulary = me.stores.settings.read(|s| s.voice_dictation.vocabulary.clone());
            let promoted: Vec<CorrectionPair> = me.stores.corrections.update(|c| c.record_for_app(&fixes, (!app_exe.is_empty()).then_some(app_exe.as_str())));
            let visible: Vec<CorrectionPair> = me
                .stores
                .corrections
                .read(|c| c.suggestions(&vocabulary))
                .into_iter()
                .filter(|s| promoted.iter().any(|p| p.from == s.from && p.to == s.to && p.app_exe == s.app_exe))
                .collect();
            if let Some(first) = visible.first() {
                me.shell.suggest(first);
            }
            me.shell.state_changed();
        });
    }
}
