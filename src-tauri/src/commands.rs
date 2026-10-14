//! Commands the Settings and overlay windows call.

use std::sync::Arc;

use quill_core::corrections::CorrectionPair;
use quill_core::engines::{AssetId, EngineId};
use quill_core::history::HistoryEntry;
use quill_core::settings::{CleanupLevel, Hotkey, Settings};
use quill_core::stats::{Insights, Stats};
use quill_core::text::vocabulary::{apply, parse_list_for_app, protect_snippets, EntryKind, Piece, VocabEntry};
use tauri::{AppHandle, State};

use crate::app::{self, notify, Quill, UiState};

type Q<'a> = State<'a, Arc<Quill>>;

#[tauri::command]
pub fn get_state(q: Q) -> UiState {
    q.ui_state()
}

#[tauri::command]
pub fn set_settings(app: AppHandle, q: Q, settings: Settings) {
    q.apply_settings(&app, settings);
}

#[tauri::command]
pub fn download_asset(app: AppHandle, q: Q, id: AssetId) {
    let models = q.models.clone();
    tauri::async_runtime::spawn(async move {
        models.download(id).await;
        notify(&app);
    });
}

#[tauri::command]
pub fn download_engine(app: AppHandle, q: Q, engine: EngineId) {
    let models = q.models.clone();
    tauri::async_runtime::spawn(async move {
        for id in engine.info().required {
            if !models.is_ready(*id) {
                models.clone().download(*id).await;
                if !models.is_ready(*id) {
                    break;
                }
            }
        }
        notify(&app);
    });
}

#[tauri::command]
pub fn cancel_download(q: Q, engine: EngineId) {
    for id in engine.info().required.iter().chain(engine.info().optional) {
        q.models.cancel(*id);
    }
}

#[tauri::command]
pub fn cancel_asset(q: Q, id: AssetId) {
    q.models.cancel(id);
}

#[tauri::command]
pub fn remove_asset(app: AppHandle, q: Q, id: AssetId) -> Result<(), String> {
    for e in quill_core::engines::ENGINES.iter().filter(|e| e.required.contains(&id) || e.optional.contains(&id)) {
        q.engines.evict(e.id);
    }
    if id == AssetId::S1Mini {
        q.engines.evict_cleaner();
    }
    let r = q.models.remove(id).map_err(|e| e.to_string());
    notify(&app);
    r
}

#[tauri::command]
pub fn remove_engine(app: AppHandle, q: Q, engine: EngineId) -> Result<(), String> {
    q.engines.evict(engine);
    if engine.info().has_cleanup {
        q.engines.evict_cleaner();
    }
    let info = engine.info();
    for id in info.required.iter().chain(info.optional) {
        q.models.remove(*id).map_err(|e| e.to_string())?;
    }
    notify(&app);
    Ok(())
}

#[tauri::command]
pub fn load_engine(q: Q) {
    q.controller.warm_selected();
}

#[tauri::command]
pub fn begin_hotkey_capture(app: AppHandle, q: Q) {
    *q.capture_error.lock() = None;
    app::begin_capture(&q);
    notify(&app);
}

#[tauri::command]
pub fn cancel_hotkey_capture(app: AppHandle, q: Q) {
    *q.capture_error.lock() = None;
    app::end_capture(&q);
    notify(&app);
}

#[tauri::command]
pub fn reset_hotkey(app: AppHandle, q: Q) {
    let mut s = q.stores.settings.get();
    s.voice_dictation.hotkey = Hotkey::default();
    q.apply_settings(&app, s);
}

#[tauri::command]
pub fn open_microphone_settings() {
    open_uri("ms-settings:privacy-microphone");
}

#[tauri::command]
pub fn open_sound_settings() {
    open_uri("ms-settings:sound");
}

fn open_uri(uri: &str) {
    let _ = std::process::Command::new("explorer.exe").arg(uri).spawn();
}

#[tauri::command]
pub fn open_releases() {
    open_uri(crate::updates::RELEASES_URL);
}

#[tauri::command]
pub fn copy_text(text: String) -> bool {
    crate::platform::paste::set_clipboard_text(&text)
}

#[tauri::command]
pub fn copy_last(q: Q) -> bool {
    match q.controller.last_transcript.lock().clone() {
        Some(t) => crate::platform::paste::set_clipboard_text(&t),
        None => false,
    }
}

#[tauri::command]
pub fn paste_last(q: Q) {
    q.controller.paste_last();
}

/// Clean a history entry's raw transcript again in another style, and copy
/// the result.
#[tauri::command(async)]
pub fn redo_history(q: Q, id: uuid::Uuid, level: CleanupLevel) -> Result<String, String> {
    let entry = q
        .stores
        .history
        .read(|h| h.entries.iter().find(|e| e.id == id).cloned())
        .ok_or("This entry has expired.")?;
    let text = q.controller.redo(&entry.raw, level, &entry.app_exe, entry.context)?;
    if !crate::platform::paste::set_clipboard_text(&text) {
        return Err("The clipboard is busy. Try again.".into());
    }
    Ok(text)
}

#[tauri::command]
pub fn get_history(q: Q) -> Vec<HistoryEntry> {
    app::purge_history(&q);
    q.stores.history.read(|h| h.latest(20))
}

#[tauri::command]
pub fn clear_history(app: AppHandle, q: Q) {
    q.stores.history.update(|h| h.clear());
    notify(&app);
}

/// An icon for an app Quill only knows by file name: from an open window,
/// else from wherever the app is installed.
fn icon_for(exe: &str, open: &[crate::platform::target::KnownApp]) -> Option<String> {
    open.iter()
        .find(|a| a.exe.eq_ignore_ascii_case(exe))
        .and_then(|a| a.icon.clone())
        .or_else(|| crate::platform::apps::find_exe(exe).and_then(|path| crate::platform::icons::file_icon(&path)))
}

/// Keep found icons in the stats, so they are looked up once.
fn save_icons(q: &Q, found: &[(String, String)]) {
    if found.is_empty() {
        return;
    }
    q.stores.stats.update(|s| {
        for month in s.months.values_mut() {
            for (exe, icon) in found {
                if let Some(app) = month.get_mut(&exe.to_lowercase()) {
                    app.icon = Some(icon.clone());
                }
            }
        }
    });
}

#[tauri::command(async)]
pub fn get_insights(q: Q) -> Insights {
    let mut insights = q.stores.stats.read(|s| s.insights(chrono::Local::now().date_naive()));
    if insights.apps.iter().all(|a| a.icon.is_some()) {
        return insights;
    }
    let open = crate::platform::target::open_apps();
    let mut found = Vec::new();
    for app in insights.apps.iter_mut().filter(|a| a.icon.is_none()) {
        app.icon = icon_for(&app.exe, &open);
        if let Some(icon) = &app.icon {
            found.push((app.exe.clone(), icon.clone()));
        }
    }
    save_icons(&q, &found);
    insights
}

#[tauri::command(async)]
pub fn get_known_apps(q: Q) -> Vec<crate::platform::target::KnownApp> {
    use crate::platform::target::{open_apps, KnownApp};
    let mut apps = std::collections::BTreeMap::new();
    q.stores.stats.read(|s| {
        for month in s.months.values() {
            for (exe, app) in month {
                apps.insert(exe.to_lowercase(), KnownApp { exe: exe.to_lowercase(), name: app.name.clone(), icon: app.icon.clone() });
            }
        }
    });
    q.stores.history.read(|h| {
        for app in &h.entries {
            apps.entry(app.app_exe.to_lowercase()).or_insert(KnownApp { exe: app.app_exe.to_lowercase(), name: app.app_name.clone(), icon: None });
        }
    });
    let open = open_apps();
    for app in &open {
        let known = apps.get(&app.exe).and_then(|a| a.icon.clone());
        let mut app = app.clone();
        app.icon = app.icon.or(known);
        apps.insert(app.exe.clone(), app);
    }
    let mut found = Vec::new();
    for app in apps.values_mut().filter(|a| a.icon.is_none() && !a.exe.is_empty()) {
        app.icon = icon_for(&app.exe, &open);
        if let Some(icon) = &app.icon {
            found.push((app.exe.clone(), icon.clone()));
        }
    }
    save_icons(&q, &found);
    let mut apps: Vec<_> = apps.into_values().filter(|a| !a.exe.is_empty()).collect();
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

#[tauri::command]
pub fn reset_insights(app: AppHandle, q: Q) {
    q.stores.stats.update(|s| *s = Stats::default());
    notify(&app);
}

#[tauri::command]
pub fn parse_vocabulary_list(q: Q, text: String, app_exe: Option<String>) -> Vec<VocabEntry> {
    let existing = q.stores.settings.read(|s| s.voice_dictation.vocabulary.clone());
    parse_list_for_app(&text, &existing, app_exe.as_deref())
}

/// Preview an entry applied to sample text (the editor's Test field).
#[tauri::command]
pub fn test_vocabulary_entry(entry: VocabEntry, sample: String) -> String {
    let entries = [VocabEntry { is_enabled: true, ..entry }];
    match entries[0].kind {
        EntryKind::Vocabulary => apply(&sample, &entries, None).text,
        EntryKind::Snippet => {
            let pieces: Vec<String> = protect_snippets(&sample, &entries)
                .into_iter()
                .map(|p| match p {
                    Piece::Speech(s) => s,
                    Piece::Break(n) => "\n".repeat(n),
                    Piece::Snippet { text, .. } => text,
                })
                .collect();
            quill_core::text::finish::join(&pieces)
        }
    }
}

#[tauri::command]
pub fn accept_suggestion(app: AppHandle, q: Q, from: String, to: String, app_exe: Option<String>) {
    let mut s = q.stores.settings.get();
    let mut entry = VocabEntry::new(EntryKind::Vocabulary, from.clone(), to.clone());
    entry.app_exe = app_exe.clone();
    s.voice_dictation.vocabulary.push(entry);
    q.apply_settings(&app, s);
    q.stores.corrections.update(|c| c.dismiss_for_app(&from, &to, app_exe.as_deref()));
    crate::overlay::set_interactive(&app, false);
    notify(&app);
}

#[tauri::command]
pub fn dismiss_suggestion(app: AppHandle, q: Q, from: String, to: String, app_exe: Option<String>) {
    q.stores.corrections.update(|c| c.dismiss_for_app(&from, &to, app_exe.as_deref()));
    crate::overlay::set_interactive(&app, false);
    notify(&app);
}

#[tauri::command]
pub fn forget_corrections(app: AppHandle, q: Q) {
    q.stores.corrections.update(|c| c.forget_all());
    notify(&app);
}

#[tauri::command]
pub fn hide_overlay(app: AppHandle) {
    crate::overlay::set_interactive(&app, false);
    crate::overlay::hide(&app);
}

#[tauri::command]
pub fn dictation_command(q: Q, command: String) {
    q.controller.command(&command);
}

#[tauri::command]
pub fn animations_enabled() -> bool {
    crate::platform::system::animations_enabled()
}

#[tauri::command]
pub fn last_metrics(q: Q) -> Option<crate::controller::Metrics> {
    q.controller.last_metrics.lock().clone()
}

#[tauri::command]
pub fn pending_suggestion(q: Q) -> Option<CorrectionPair> {
    let vocab = q.stores.settings.read(|s| s.voice_dictation.vocabulary.clone());
    q.stores.corrections.read(|c| c.suggestions(&vocab)).into_iter().next()
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<Option<crate::updates::UpdateInfo>, String> {
    crate::updates::check(&app).await
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    crate::updates::install(&app).await
}
