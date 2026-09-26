//! Quill for Windows: local push-to-talk dictation.

mod app;
mod audio;
pub mod bench;
mod commands;
mod controller;
mod engines;
mod launch;
mod models;
mod overlay;
pub mod paths;
mod platform;
mod shell;
mod stores;
mod tray;
mod updates;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tauri::{Manager, RunEvent};
use tauri_plugin_autostart::MacosLauncher;

use app::{Quill, TauriShell};
use controller::Controller;
use engines::EngineHost;
use models::ModelManager;
use stores::Stores;

pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    let first_run = !paths::settings_file().exists();

    tauri::Builder::default()
        // A second launch forwards its args here: a launcher command, or open Settings.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            match launch::command_from_args(&argv) {
                Some(cmd) => {
                    if let Some(q) = app.try_state::<Arc<Quill>>() {
                        q.controller.command(cmd);
                    }
                }
                None => app::open_settings(app),
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--autostart"])))
        .setup(move |tauri_app| {
            let handle = tauri_app.handle().clone();
            app::remember_app(&handle);

            let stores = Arc::new(Stores::open());
            if first_run {
                // Persist defaults so the file exists from the first launch.
                stores.settings.update(|_| ());
            }
            let notify_handle = handle.clone();
            let models = ModelManager::new(paths::models_dir(), move || app::notify(&notify_handle));
            let threads = platform::system::inference_threads();
            let engines = EngineHost::new(models.clone(), threads);
            log::info!("inference threads: {threads}");

            let settings = stores.settings.get();
            let (hook, hook_error) = match platform::hotkey::HotkeyHook::install(settings.voice_dictation.hotkey) {
                Ok((hook, rx)) => {
                    hook.set_suspended(!settings.enabled);
                    app::spawn_hook_consumer(handle.clone(), rx);
                    (Some(hook), None)
                }
                Err(e) => (None, Some(format!("The global shortcut is unavailable: {e}"))),
            };

            let preferred_languages = platform::system::preferred_languages();
            let controller = Controller::new(
                stores.clone(),
                engines.clone(),
                models.clone(),
                hook,
                Arc::new(TauriShell::new(handle.clone())),
                quill_core::text::numbers::default_itn(),
                preferred_languages.clone(),
            );
            let quill = Arc::new(Quill {
                controller: controller.clone(),
                models: models.clone(),
                engines,
                stores: stores.clone(),
                hook,
                hook_error,
                capturing: AtomicBool::new(false),
                capture_error: Mutex::new(None),
                preferred_languages,
                updates: updates::Updates::default(),
            });
            tauri_app.manage(quill.clone());

            // Warm once right after a download, so any first-run compile
            // happens while the user is still in Settings.
            let warm = controller.clone();
            models.set_on_installed(move |asset| {
                let engine = warm.stores.settings.read(|s| s.voice_dictation.engine);
                let info = engine.info();
                if info.required.contains(&asset) || info.optional.contains(&asset) {
                    warm.warm_selected();
                }
            });

            // Engines let go of an asset's files before an update replaces them.
            let evict = quill.engines.clone();
            models.set_before_replace(move |asset| {
                for e in quill_core::engines::ENGINES.iter() {
                    if e.required.contains(&asset) || e.optional.contains(&asset) {
                        evict.evict(e.id);
                    }
                }
                if asset == quill_core::engines::AssetId::S1Mini {
                    evict.evict_cleaner();
                }
            });

            overlay::create(&handle)?;
            tray::create(&handle, &quill)?;

            {
                use tauri_plugin_deep_link::DeepLinkExt;
                if let Err(e) = handle.deep_link().register_all() {
                    log::warn!("could not register quill://: {e}");
                }
            }
            if first_run {
                app::set_autostart(&handle, settings.launch_at_login);
            }
            spawn_history_purge(quill.clone(), handle.clone());
            // Debug builds never self-update.
            if !cfg!(debug_assertions) {
                updates::spawn_checker(handle.clone());
            }

            match launch::command_from_args(&args) {
                Some(cmd) => controller.command(cmd),
                // Until setup is finished, Settings opens even at login.
                None if !launch::is_autostart(&args) || !settings.onboarded => app::open_settings(&handle),
                None => {}
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::set_settings,
            commands::download_asset,
            commands::download_engine,
            commands::cancel_download,
            commands::cancel_asset,
            commands::remove_asset,
            commands::remove_engine,
            commands::load_engine,
            commands::begin_hotkey_capture,
            commands::cancel_hotkey_capture,
            commands::reset_hotkey,
            commands::open_microphone_settings,
            commands::open_sound_settings,
            commands::copy_text,
            commands::copy_last,
            commands::get_history,
            commands::clear_history,
            commands::get_insights,
            commands::reset_insights,
            commands::parse_vocabulary_list,
            commands::test_vocabulary_entry,
            commands::accept_suggestion,
            commands::dismiss_suggestion,
            commands::forget_corrections,
            commands::hide_overlay,
            commands::dictation_command,
            commands::animations_enabled,
            commands::last_metrics,
            commands::pending_suggestion,
            commands::check_for_updates,
            commands::install_update,
            commands::open_releases,
        ])
        .on_window_event(|window, event| {
            // Leaving Settings while capturing a shortcut ends the capture, so
            // keys typed in other apps are never swallowed.
            if window.label() == app::SETTINGS_LABEL {
                if matches!(event, tauri::WindowEvent::Destroyed | tauri::WindowEvent::Focused(false)) {
                    if let Some(q) = window.app_handle().try_state::<Arc<Quill>>() {
                        if q.capturing.load(std::sync::atomic::Ordering::SeqCst) {
                            app::end_capture(&q);
                            app::notify(window.app_handle());
                        }
                    }
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Quill")
        .run(|_app, event| {
            // Keep running in the tray when the last window closes.
            if let RunEvent::ExitRequested { api, code: None, .. } = event {
                api.prevent_exit();
            }
        });
}

/// Purge history on launch and whenever the next record expires.
fn spawn_history_purge(quill: Arc<Quill>, app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        if app::purge_history(&quill) {
            app::notify(&app);
        }
        let next = quill.stores.history.read(|h| h.next_expiry());
        let wait = next
            .map(|t| (t - chrono::Utc::now()).to_std().unwrap_or(Duration::ZERO) + Duration::from_secs(1))
            .unwrap_or(Duration::from_secs(3600))
            .min(Duration::from_secs(300));
        std::thread::sleep(wait);
    });
}
