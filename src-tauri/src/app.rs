//! The app's shared state, what Settings shows, and the shell that connects
//! the controller to windows, the tray, and events.

use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use quill_core::corrections::CorrectionPair;
use quill_core::engines::{cohere_auto_language, language_name, AssetId, EngineId, EngineInfo, ENGINES, LANGUAGES};
use quill_core::errors::{MIC_DENIED_DETAIL, MIC_UNAVAILABLE_DETAIL};
use quill_core::models::{asset, ModelState};
use quill_core::settings::{Hotkey, Settings};
use quill_core::state::Phase;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::controller::{Controller, DictationView};
use crate::engines::EngineHost;
use crate::models::ModelManager;
use crate::platform::hotkey::{HookEvent, HotkeyHook};
use crate::platform::system::{microphone_permission, MicPermission};
use crate::shell::Shell;
use crate::stores::Stores;
use crate::{overlay, tray};

pub struct Quill {
    pub controller: Arc<Controller>,
    pub models: Arc<ModelManager>,
    pub engines: Arc<EngineHost>,
    pub stores: Arc<Stores>,
    pub hook: Option<HotkeyHook>,
    pub hook_error: Option<String>,
    pub capturing: AtomicBool,
    pub capture_error: Mutex<Option<String>>,
    pub preferred_languages: Vec<String>,
    pub updates: crate::updates::Updates,
}

pub const SETTINGS_LABEL: &str = "settings";

pub fn open_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(SETTINGS_LABEL) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App("index.html".into()))
        .title("Quill")
        .theme(Some(tauri::Theme::Dark))
        .inner_size(760.0, 640.0)
        .min_inner_size(600.0, 480.0)
        .resizable(true)
        .center()
        .build();
    if let Err(e) = built {
        log::warn!("could not open Settings: {e}");
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub kind: &'static str,
    pub dot: &'static str,
    pub title: String,
    pub detail: String,
    pub hotkey_problem: Option<String>,
    pub engine_loaded: bool,
    pub engine_loading: bool,
    pub warm_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetView {
    pub id: AssetId,
    pub title: &'static str,
    pub size: u64,
    pub license: &'static str,
    pub license_note: &'static str,
    pub runtime: &'static str,
    pub optional: bool,
    pub available: bool,
    pub state: ModelState,
    pub has_data: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineView {
    #[serde(flatten)]
    pub info: EngineInfo,
    pub supported: bool,
    pub assets: Vec<AssetView>,
    pub ready: bool,
    pub busy: bool,
    pub has_data: bool,
    pub update_available: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiState {
    pub settings: Settings,
    pub status: Status,
    pub engines: Vec<EngineView>,
    pub languages: Vec<(&'static str, &'static str)>,
    pub cohere_auto_language: &'static str,
    pub hotkey_display: String,
    pub hotkey_is_default: bool,
    pub capturing_hotkey: bool,
    pub capture_error: Option<String>,
    pub itn_available: bool,
    pub suggestions: Vec<CorrectionPair>,
    pub corrections_seen: u64,
    pub history_count: usize,
    pub has_dictated: bool,
    pub dictionary_hits: std::collections::HashMap<uuid::Uuid, u64>,
    pub dictation: DictationView,
    pub has_last_transcript: bool,
    pub version: &'static str,
    pub update: Option<crate::updates::UpdateInfo>,
    pub updating: bool,
    pub update_error: Option<String>,
}

impl Quill {
    fn asset_view(&self, id: AssetId) -> AssetView {
        let a = asset(id);
        AssetView {
            id,
            title: a.title,
            size: a.total_bytes(),
            license: a.license,
            license_note: a.license_note,
            runtime: a.runtime,
            optional: a.optional,
            available: a.available,
            state: self.models.state(id),
            has_data: self.models.has_data(id),
        }
    }

    fn engine_view(&self, info: &EngineInfo) -> EngineView {
        let assets: Vec<AssetView> =
            info.required.iter().chain(info.optional.iter()).map(|a| self.asset_view(*a)).collect();
        let required: Vec<&AssetView> = assets.iter().filter(|a| !a.optional).collect();
        EngineView {
            info: info.clone(),
            supported: EngineHost::supports(info.id),
            ready: required.iter().all(|a| a.state.is_ready()),
            busy: assets.iter().any(|a| a.state.is_busy()),
            has_data: required.iter().any(|a| a.has_data),
            update_available: required.iter().any(|a| a.state == ModelState::UpdateAvailable),
            assets,
        }
    }

    pub fn status(&self) -> Status {
        let settings = self.stores.settings.get();
        let vd = &settings.voice_dictation;
        let info = vd.engine.info();
        let hotkey_problem = self.hook_error.clone().or_else(|| vd.hotkey.problem());
        let engine_loading = self.engines.is_loading(vd.engine);
        let base = |kind, dot, title: String, detail: String| Status {
            kind,
            dot,
            title,
            detail,
            hotkey_problem: hotkey_problem.clone(),
            engine_loaded: false,
            engine_loading,
            warm_error: self.controller.warm_error.lock().clone(),
        };
        if !settings.enabled {
            return base("off", "gray", "Quill is off".into(), "Turn Quill on to dictate with your shortcut.".into());
        }
        if !EngineHost::supports(vd.engine) {
            return base(
                "unsupported",
                "yellow",
                format!("{} is not available in this build", info.title),
                "Pick another engine below.".into(),
            );
        }
        let missing: Vec<&str> =
            info.required.iter().filter(|a| !self.models.is_ready(**a)).map(|a| asset(*a).title).collect();
        if !missing.is_empty() {
            return base(
                "modelsMissing",
                "yellow",
                format!("{} is not downloaded", info.title),
                format!("Download {} below, or pick another engine.", missing.join(" and ")),
            );
        }
        if microphone_permission() == MicPermission::Denied {
            return base("micDenied", "orange", "Microphone access is off for Quill".into(), MIC_DENIED_DETAIL.into());
        }
        if !crate::audio::has_input_device() {
            return base("micUnavailable", "orange", "No microphone available".into(), MIC_UNAVAILABLE_DETAIL.into());
        }
        let mut s = base(
            "ready",
            "green",
            format!("Ready · {}", vd.hotkey.display()),
            vd.activation_mode.detail().into(),
        );
        s.engine_loaded = self.engines.recognizer_loaded(vd.engine);
        s
    }

    pub fn ui_state(&self) -> UiState {
        let settings = self.stores.settings.get();
        let vd = &settings.voice_dictation;
        let suggestions = self.stores.corrections.read(|c| c.suggestions(&vd.vocabulary));
        UiState {
            status: self.status(),
            engines: ENGINES.iter().map(|e| self.engine_view(e)).collect(),
            languages: LANGUAGES.to_vec(),
            cohere_auto_language: language_name(cohere_auto_language(&self.preferred_languages)).unwrap_or("English"),
            hotkey_display: vd.hotkey.display(),
            hotkey_is_default: vd.hotkey == Hotkey::default(),
            capturing_hotkey: self.capturing.load(SeqCst),
            capture_error: self.capture_error.lock().clone(),
            itn_available: self.controller.itn_available(),
            suggestions,
            corrections_seen: self.stores.corrections.read(|c| c.seen),
            history_count: self.stores.history.read(|h| h.entries.len()),
            has_dictated: self.stores.stats.read(|s| !s.is_empty()),
            dictionary_hits: self.stores.stats.read(|s| s.dictionary_hits.clone()),
            dictation: self.controller.view(),
            has_last_transcript: self.controller.last_transcript.lock().is_some(),
            version: env!("CARGO_PKG_VERSION"),
            update: self.updates.info(),
            updating: self.updates.installing(),
            update_error: self.updates.error.lock().clone(),
            settings,
        }
    }

    /// Save new settings and apply their side effects.
    pub fn apply_settings(&self, app: &AppHandle, next: Settings) {
        let prev = self.stores.settings.get();
        self.stores.settings.update(|s| *s = next.clone());
        let (p, n) = (&prev.voice_dictation, &next.voice_dictation);
        if let Some(hook) = &self.hook {
            if p.hotkey != n.hotkey {
                hook.set_hotkey(n.hotkey);
            }
            hook.set_suspended(!next.enabled || self.capturing.load(SeqCst));
        }
        if prev.launch_at_login != next.launch_at_login {
            set_autostart(app, next.launch_at_login);
        }
        if p.engine != n.engine {
            // Free the old engine's memory.
            self.engines.evict(p.engine);
            if !n.engine.info().has_cleanup {
                self.engines.evict_cleaner();
            }
            *self.controller.warm_error.lock() = None;
        }
        if p.history_enabled && !n.history_enabled {
            self.stores.history.update(|h| h.clear());
        }
        if !next.enabled && self.controller.phase().is_busy() {
            self.controller.cancel();
        }
        notify(app);
    }

    pub fn set_engine(&self, engine: EngineId) {
        let app = APP.lock().clone();
        let mut s = self.stores.settings.get();
        s.voice_dictation.engine = engine;
        if let Some(app) = app {
            self.apply_settings(&app, s);
        }
    }
}

pub fn set_autostart(app: &AppHandle, on: bool) {
    // Development executables need Vite and must never own the login entry.
    if cfg!(debug_assertions) { return; }
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    let result = if on { launcher.enable() } else { launcher.disable() };
    if let Err(e) = result {
        log::warn!("autostart: {e}");
    } else if on {
        if let Err(e) = crate::launch::quote_autostart_path(&app.package_info().name) {
            log::warn!("autostart path: {e}");
        }
    }
}

static APP: Mutex<Option<AppHandle>> = Mutex::new(None);

pub fn remember_app(app: &AppHandle) {
    *APP.lock() = Some(app.clone());
}

/// Tell Settings to refetch, and refresh the tray.
pub fn notify(app: &AppHandle) {
    let _ = app.emit_to(SETTINGS_LABEL, "state-changed", ());
    if let Some(q) = app.try_state::<Arc<Quill>>() {
        tray::refresh(app, &q);
    }
}

/// Drains hotkey events on a dedicated thread.
pub fn spawn_hook_consumer(app: AppHandle, rx: std::sync::mpsc::Receiver<HookEvent>) {
    std::thread::Builder::new()
        .name("quill-hotkey-events".into())
        .spawn(move || {
            for event in rx {
                let Some(q) = app.try_state::<Arc<Quill>>() else { continue };
                let q = q.inner().clone();
                match event {
                    HookEvent::Pressed => q.controller.on_hotkey(quill_core::state::HotkeyEvent::Pressed),
                    HookEvent::Released => q.controller.on_hotkey(quill_core::state::HotkeyEvent::Released),
                    HookEvent::Escape => q.controller.on_hotkey(quill_core::state::HotkeyEvent::Escape),
                    HookEvent::Captured(hotkey) => {
                        if !q.capturing.load(SeqCst) { continue; }
                        if let Some(problem) = hotkey.problem() {
                            *q.capture_error.lock() = Some(problem);
                        } else {
                            *q.capture_error.lock() = None;
                            let mut s = q.stores.settings.get();
                            s.voice_dictation.hotkey = hotkey;
                            q.apply_settings(&app, s);
                            end_capture(&q);
                        }
                        notify(&app);
                    }
                    HookEvent::CaptureCancelled => {
                        *q.capture_error.lock() = None;
                        end_capture(&q);
                        notify(&app);
                    }
                }
            }
        })
        .expect("spawn hotkey consumer");
}

pub fn begin_capture(q: &Quill) {
    if let Some(h) = &q.hook {
        q.capturing.store(true, SeqCst);
        h.set_suspended(true);
        h.set_capturing(true);
    } else {
        *q.capture_error.lock() = Some("Quill could not start the keyboard hook. Quit Quill from the tray and reopen it, then try again.".into());
    }
}

pub fn end_capture(q: &Quill) {
    if let Some(h) = &q.hook {
        q.capturing.store(false, SeqCst);
        h.set_capturing(false);
        let enabled = q.stores.settings.read(|s| s.enabled);
        h.set_suspended(!enabled);
    }
}

/// Connects the controller to Tauri.
pub struct TauriShell {
    pub app: AppHandle,
    suggestion_seq: Arc<std::sync::atomic::AtomicU64>,
}

impl TauriShell {
    pub fn new(app: AppHandle) -> Self {
        Self { app, suggestion_seq: Arc::new(std::sync::atomic::AtomicU64::new(0)) }
    }
}

impl Shell for TauriShell {
    fn dictation_changed(&self, view: &DictationView) {
        let _ = self.app.emit_to(overlay::LABEL, "dictation", view);
        if view.phase == Phase::Idle {
            overlay::hide(&self.app);
        } else {
            overlay::set_interactive(&self.app, false);
            overlay::show(&self.app);
        }
        notify(&self.app);
    }

    fn level(&self, level: f32) {
        let _ = self.app.emit_to(overlay::LABEL, "level", level);
    }

    fn state_changed(&self) {
        notify(&self.app);
    }

    fn history_changed(&self) {
        let _ = self.app.emit_to(SETTINGS_LABEL, "history-changed", ());
    }

    fn suggest(&self, pair: &CorrectionPair) {
        let seq = self.suggestion_seq.fetch_add(1, SeqCst) + 1;
        let app = self.app.clone();
        let pair = pair.clone();
        // Never interrupt a dictation in progress.
        let q = app.state::<Arc<Quill>>().inner().clone();
        if q.controller.phase() != Phase::Idle {
            return;
        }
        let _ = app.emit_to(overlay::LABEL, "suggestion", &pair);
        overlay::set_interactive(&app, true);
        overlay::show(&app);
        let counter = self.suggestion_seq.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(8));
            let q = app.state::<Arc<Quill>>().inner().clone();
            if counter.load(SeqCst) == seq && q.controller.phase() == Phase::Idle {
                let _ = app.emit_to(overlay::LABEL, "suggestion-dismissed", ());
                overlay::set_interactive(&app, false);
                overlay::hide(&app);
            }
        });
    }

    fn own_windows(&self) -> Vec<isize> {
        let mut v = Vec::new();
        if let Some(h) = overlay::own_hwnd(&self.app) {
            v.push(h);
        }
        if let Some(w) = self.app.get_webview_window(SETTINGS_LABEL) {
            if let Ok(h) = w.hwnd() {
                v.push(h.0 as isize);
            }
        }
        v
    }
}

/// Drop expired history records; writes the file only when one expired.
pub fn purge_history(q: &Quill) -> bool {
    let now = chrono::Utc::now();
    let due = q.stores.history.read(|h| h.next_expiry().is_some_and(|t| t <= now));
    due && q.stores.history.update(|h| h.purge(now))
}
