//! The app's shared state, what Settings shows, and the shell that connects
//! the controller to windows, the tray, and events.

use std::sync::atomic::Ordering::SeqCst;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use quill_core::corrections::CorrectionPair;
use quill_core::engines::{cohere_auto_language, language_name, AssetId, EngineId, EngineInfo, ENGINES, LANGUAGES};
use quill_core::errors::{MIC_DENIED_DETAIL, MIC_UNAVAILABLE_DETAIL};
use quill_core::models::{asset, ModelState};
use quill_core::settings::{Hotkey, Settings, ShortcutSlot, SketchSettings};
use quill_core::state::{Input, Phase};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::controller::{Controller, DictationView};
use crate::engines::EngineHost;
use crate::models::ModelManager;
use crate::platform::hotkey::{now_ms, HookEvent, HotkeyHook};
use crate::platform::mouse::ButtonHook;
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
    /// The shortcut Settings is capturing, if any.
    pub capturing: Mutex<Option<ShortcutSlot>>,
    pub capture_error: Mutex<Option<String>>,
    /// The mouse hook for a side-button shortcut, while one is needed.
    pub buttons: Mutex<Option<ButtonHook>>,
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
        .inner_size(900.0, 660.0)
        .min_inner_size(740.0, 520.0)
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
    /// Parakeet hotwords were turned on but could not start on this PC.
    pub hotwords_unavailable: bool,
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
    pub sketch_hotkey_display: String,
    pub sketch_hotkey_is_default: bool,
    pub sketch_hotkey_problem: Option<String>,
    pub capturing_sketch_hotkey: bool,
    /// The shortcut that dictates without cleanup, if set.
    pub raw_hotkey_display: Option<String>,
    pub raw_hotkey_problem: Option<String>,
    pub capturing_raw_hotkey: bool,
    pub hotkey_note: Option<&'static str>,
    pub raw_hotkey_note: Option<&'static str>,
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
            hotwords_unavailable: self.engines.hotwords_unavailable(),
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
            capturing_hotkey: self.capture_slot() == Some(ShortcutSlot::Dictate),
            sketch_hotkey_display: settings.sketch.hotkey.display(),
            sketch_hotkey_is_default: settings.sketch.hotkey == SketchSettings::default_hotkey(),
            sketch_hotkey_problem: settings
                .sketch
                .enabled
                .then(|| SketchSettings::hotkey_problem(&settings.sketch.hotkey, &vd.hotkey))
                .flatten(),
            capturing_sketch_hotkey: self.capture_slot() == Some(ShortcutSlot::Sketch),
            raw_hotkey_display: vd.raw_hotkey.map(|h| h.display()),
            raw_hotkey_problem: vd.raw_hotkey.and_then(|h| settings.shortcut_problem(ShortcutSlot::Raw, &h)),
            capturing_raw_hotkey: self.capture_slot() == Some(ShortcutSlot::Raw),
            hotkey_note: vd.hotkey.note(),
            raw_hotkey_note: vd.raw_hotkey.and_then(|h| h.note()),
            capture_error: self.capture_error.lock().clone(),
            itn_available: self.controller.itn_available(),
            suggestions,
            corrections_seen: self.stores.corrections.read(|c| c.seen),
            history_count: self.stores.history.read(|h| h.entries.len()),
            has_dictated: self.stores.stats.read(|s| !s.is_empty()),
            dictionary_hits: self.stores.stats.read(|s| s.dictionary_hits.clone()),
            dictation: self.controller.view(),
            has_last_transcript: self.controller.last_text().is_some(),
            version: env!("CARGO_PKG_VERSION"),
            update: self.updates.info(),
            updating: self.updates.installing(),
            update_error: self.updates.error.lock().clone(),
            settings,
        }
    }

    /// Which shortcut Settings is capturing, if any.
    pub fn capture_slot(&self) -> Option<ShortcutSlot> {
        *self.capturing.lock()
    }

    /// Run the mouse hook only while a dictation shortcut needs the mouse (a
    /// side button, or a lone modifier that clicks turn into chords), or
    /// Settings is capturing.
    pub fn sync_buttons(&self) {
        let settings = self.stores.settings.get();
        let needs_mouse = |h: Hotkey| h.is_mouse() || h.is_lone_modifier();
        let wanted = self.hook.is_some()
            && (self.capture_slot().is_some()
                || (settings.enabled
                    && (needs_mouse(settings.voice_dictation.hotkey) || settings.usable_raw_hotkey().is_some_and(needs_mouse))));
        let mut buttons = self.buttons.lock();
        if wanted && buttons.is_none() {
            match ButtonHook::install() {
                Ok(hook) => *buttons = Some(hook),
                Err(e) => log::warn!("mouse shortcut: {e}"),
            }
        } else if !wanted {
            *buttons = None;
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
            hook.set_sketch_hotkey(sketch_hotkey(&next));
            hook.set_raw_hotkey(next.usable_raw_hotkey());
            hook.set_suspended(!next.enabled || self.capture_slot().is_some());
        }
        self.sync_buttons();
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
        if p.history_retention != n.history_retention {
            // Settings asked before shortening; the notify below updates the count.
            purge_history(self);
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
            use std::sync::mpsc::RecvTimeoutError;
            // The shortcut's next timer. Waiting here, instead of on a timer
            // thread, handles a tick only after every key event before it.
            let mut deadline: Option<u64> = None;
            loop {
                let event = match deadline {
                    Some(due) => match rx.recv_timeout(Duration::from_millis(due.saturating_sub(now_ms()))) {
                        Ok(event) => Some(event),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => break,
                    },
                    None => match rx.recv() {
                        Ok(event) => Some(event),
                        Err(_) => break,
                    },
                };
                let Some(q) = app.try_state::<Arc<Quill>>() else { continue };
                let q = q.inner().clone();
                let Some(event) = event else {
                    deadline = q.controller.on_hotkey(Input::Tick, None, now_ms());
                    continue;
                };
                match event {
                    HookEvent::Shortcut { binding, input, at } => deadline = q.controller.on_hotkey(input, Some(binding), at),
                    HookEvent::Escape => deadline = q.controller.on_hotkey(Input::Escape, None, now_ms()),
                    HookEvent::Sketch => q.controller.sketch(),
                    HookEvent::SketchUndo => q.controller.sketch_action("undo"),
                    HookEvent::Captured(hotkey) => {
                        let Some(slot) = q.capture_slot() else { continue };
                        let mut s = q.stores.settings.get();
                        if let Some(problem) = s.shortcut_problem(slot, &hotkey) {
                            *q.capture_error.lock() = Some(problem);
                        } else {
                            *q.capture_error.lock() = None;
                            match slot {
                                ShortcutSlot::Dictate => s.voice_dictation.hotkey = hotkey,
                                ShortcutSlot::Raw => s.voice_dictation.raw_hotkey = Some(hotkey),
                                ShortcutSlot::Sketch => s.sketch.hotkey = hotkey,
                            }
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

/// The Sketch shortcut to listen for, if Sketch is on and its shortcut is usable.
pub fn sketch_hotkey(s: &Settings) -> Option<Hotkey> {
    let sketch = &s.sketch;
    (sketch.enabled && SketchSettings::hotkey_problem(&sketch.hotkey, &s.voice_dictation.hotkey).is_none())
        .then_some(sketch.hotkey)
}

pub fn begin_capture(q: &Quill, slot: ShortcutSlot) {
    if let Some(h) = &q.hook {
        *q.capturing.lock() = Some(slot);
        h.set_suspended(true);
        h.set_capturing(true);
        q.sync_buttons();
    } else {
        *q.capture_error.lock() = Some("Quill could not start the keyboard hook. Quit Quill from the tray and reopen it, then try again.".into());
    }
}

pub fn end_capture(q: &Quill) {
    if let Some(h) = &q.hook {
        *q.capturing.lock() = None;
        h.set_capturing(false);
        let enabled = q.stores.settings.read(|s| s.enabled);
        h.set_suspended(!enabled);
        q.sync_buttons();
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

    fn preview(&self, text: &str) {
        let _ = self.app.emit_to(overlay::LABEL, "preview", text);
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
        let mut v = crate::sketch_windows::own_hwnds(&self.app);
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

    fn sketch_show(&self, monitor: &crate::platform::screen::Monitor, view: &crate::sketch::SketchView) {
        crate::sketch_windows::show(&self.app, monitor, view);
    }

    fn sketch_view(&self, view: &crate::sketch::SketchView) {
        let _ = self.app.emit_to(crate::sketch_windows::BAR, "sketch-view", view);
    }

    fn sketch_live(&self, points: &[(f32, f32)]) {
        self.sketch_ink("sketch-live", points);
    }

    fn sketch_commit(&self, points: &[(f32, f32)], label: &str) {
        self.sketch_ink("sketch-commit", Mark { points, label });
    }

    fn sketch_undo(&self) {
        self.sketch_ink("sketch-undo", ());
    }

    fn sketch_pen(&self, on: bool) {
        crate::sketch_windows::set_pen(&self.app, on);
    }

    fn sketch_bar_rect(&self) -> Option<(i32, i32, i32, i32)> {
        crate::sketch_windows::bar_rect_now(&self.app)
    }

    fn sketch_hide(&self) {
        crate::sketch_windows::hide(&self.app);
    }
}

impl TauriShell {
    fn sketch_ink(&self, event: &str, payload: impl serde::Serialize + Clone) {
        let _ = self.app.emit_to(crate::sketch_windows::INK, event, payload);
    }
}

#[derive(Clone, serde::Serialize)]
struct Mark<'a> {
    points: &'a [(f32, f32)],
    label: &'a str,
}

/// Drop expired history records; writes the file only when one expired.
pub fn purge_history(q: &Quill) -> bool {
    let now = chrono::Utc::now();
    let keep = q.stores.settings.read(|s| s.voice_dictation.history_retention);
    let due = q.stores.history.read(|h| h.next_expiry(keep).is_some_and(|t| t <= now));
    due && q.stores.history.update(|h| h.purge(now, keep))
}
