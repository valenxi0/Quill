//! In-app updates from GitHub Releases. Quill checks shortly after launch and
//! then daily; a new version is offered once in the overlay and stays offered
//! in the tray and Settings. Installing runs the new installer quietly and
//! relaunches Quill. Updates are signed; unsigned ones are refused.

use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::app::{notify, Quill};

const FIRST_CHECK: Duration = Duration::from_secs(20);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current: String,
    pub notes: Option<String>,
}

#[derive(Default)]
pub struct Updates {
    available: Mutex<Option<Update>>,
    installing: AtomicBool,
    /// Versions already offered in the overlay (offered once each).
    offered: Mutex<Option<String>>,
    pub error: Mutex<Option<String>>,
}

impl Updates {
    pub fn info(&self) -> Option<UpdateInfo> {
        self.available.lock().as_ref().map(|u| UpdateInfo {
            version: u.version.clone(),
            current: u.current_version.clone(),
            notes: u.body.clone().filter(|b| !b.trim().is_empty()),
        })
    }

    pub fn installing(&self) -> bool {
        self.installing.load(SeqCst)
    }
}

/// Ask GitHub for a newer release. Returns the new version, if any.
pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let q = app.state::<Arc<Quill>>().inner().clone();
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    *q.updates.available.lock() = update;
    *q.updates.error.lock() = None;
    notify(app);
    Ok(q.updates.info())
}

/// Check now and then daily; offer each new version once in the overlay.
pub fn spawn_checker(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            match check(&app).await {
                Ok(Some(info)) => offer(&app, &info),
                Ok(None) => {}
                // Offline or GitHub unreachable: quietly try again later.
                Err(e) => log::info!("update check failed: {e}"),
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

/// Show "Quill X is available" once per version, only when idle.
fn offer(app: &AppHandle, info: &UpdateInfo) {
    let q = app.state::<Arc<Quill>>().inner().clone();
    {
        let mut offered = q.updates.offered.lock();
        if offered.as_deref() == Some(info.version.as_str()) {
            return;
        }
        if q.controller.phase() != quill_core::state::Phase::Idle {
            return; // Tray and Settings still offer it.
        }
        *offered = Some(info.version.clone());
    }
    let _ = app.emit_to(crate::overlay::LABEL, "update-available", info);
    crate::overlay::set_interactive(app, true);
    crate::overlay::show(app);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(15));
        let q = app.state::<Arc<Quill>>().inner().clone();
        if q.controller.phase() == quill_core::state::Phase::Idle && !q.updates.installing() {
            let _ = app.emit_to(crate::overlay::LABEL, "update-dismissed", ());
            crate::overlay::set_interactive(&app, false);
            crate::overlay::hide(&app);
        }
    });
}

/// Download, verify, and run the new installer; Quill relaunches after it.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let q = app.state::<Arc<Quill>>().inner().clone();
    if q.controller.phase().is_busy() {
        return Err("Finish the current dictation first.".into());
    }
    let Some(update) = q.updates.available.lock().clone() else {
        return Err("No update is available.".into());
    };
    if q.updates.installing.swap(true, SeqCst) {
        return Ok(());
    }
    notify(app);
    let result = update.download_and_install(|_, _| {}, || {}).await;
    match result {
        Ok(()) => {
            // The installer replaces Quill and starts the new version.
            app.restart();
        }
        Err(e) => {
            q.updates.installing.store(false, SeqCst);
            let message = format!("The update couldn't be installed: {e}");
            *q.updates.error.lock() = Some(message.clone());
            notify(app);
            Err(message)
        }
    }
}
