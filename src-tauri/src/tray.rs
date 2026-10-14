//! Tray icon and menu (section 5.4). Left-click opens Settings; the icon
//! tints red while recording.

use std::sync::Arc;

use quill_core::engines::ENGINES;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::app::Quill;

pub const ID: &str = "main";

/// A 32×32 waveform glyph, drawn in code so the tray needs no asset files.
pub fn icon(recording: bool) -> Image<'static> {
    const S: usize = 32;
    let mut px = vec![0u8; S * S * 4];
    let color: [u8; 3] = if recording { [0xFF, 0x45, 0x3A] } else { [0xF2, 0xF2, 0xF7] };
    // Five rounded bars, tallest in the middle.
    let bars = [(6usize, 10usize), (11, 18), (16, 26), (21, 18), (26, 10)];
    for (cx, h) in bars {
        let top = (S - h) / 2;
        for y in top..top + h {
            for x in cx - 1..=cx + 1 {
                let edge = x == cx - 1 || x == cx + 1;
                let cap = y == top || y == top + h - 1;
                let alpha = if edge && cap { 0 } else if edge || cap { 170 } else { 255 };
                let i = (y * S + x) * 4;
                px[i..i + 3].copy_from_slice(&color);
                px[i + 3] = alpha;
            }
        }
    }
    Image::new_owned(px, S as u32, S as u32)
}

fn build_menu(app: &AppHandle, quill: &Quill) -> tauri::Result<Menu<Wry>> {
    let recording = quill.controller.phase() == quill_core::state::Phase::Recording;
    let toggle = MenuItem::with_id(app, "toggle", if recording { "Stop dictation" } else { "Start dictation" }, true, None::<&str>)?;
    let has_last = quill.controller.last_transcript.lock().is_some();
    let copy = MenuItem::with_id(app, "copy-last", "Copy last transcript", has_last, None::<&str>)?;
    let paste_last = MenuItem::with_id(app, "paste-last", "Paste last transcript again", has_last && !recording, None::<&str>)?;
    let current = quill.controller.stores.settings.read(|s| s.voice_dictation.engine);
    let engine_items: Vec<CheckMenuItem<Wry>> = ENGINES
        .iter()
        .map(|e| {
            let id = format!("engine:{}", serde_json::to_value(e.id).unwrap().as_str().unwrap());
            CheckMenuItem::with_id(app, id, e.title, true, e.id == current, None::<&str>)
        })
        .collect::<tauri::Result<_>>()?;
    let refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = engine_items.iter().map(|i| i as _).collect();
    let engines = Submenu::with_items(app, format!("Engine: {}", current.info().title), true, &refs)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep = || PredefinedMenuItem::separator(app);
    match quill.updates.info() {
        Some(info) => {
            let label = if quill.updates.installing() {
                format!("Updating to {}…", info.version)
            } else {
                format!("Update to Quill {}", info.version)
            };
            let update = MenuItem::with_id(app, "update", label, !quill.updates.installing(), None::<&str>)?;
            Menu::with_items(app, &[&update, &sep()?, &toggle, &paste_last, &copy, &sep()?, &engines, &sep()?, &settings, &quit])
        }
        None => Menu::with_items(app, &[&toggle, &paste_last, &copy, &sep()?, &engines, &sep()?, &settings, &quit]),
    }
}

pub fn create(app: &AppHandle, quill: &Quill) -> tauri::Result<()> {
    let menu = build_menu(app, quill)?;
    TrayIconBuilder::with_id(ID)
        .icon(icon(false))
        .tooltip("Quill")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let quill = app.state::<Arc<Quill>>().inner().clone();
            let id = event.id().as_ref().to_string();
            match id.as_str() {
                "toggle" => quill.controller.command("toggle"),
                "paste-last" => quill.controller.command("paste-last"),
                "copy-last" => {
                    if let Some(t) = quill.controller.last_transcript.lock().clone() {
                        crate::platform::paste::set_clipboard_text(&t);
                    }
                }
                "settings" => crate::app::open_settings(app),
                "update" => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = crate::updates::install(&app).await;
                    });
                }
                "quit" => app.exit(0),
                other => {
                    if let Some(engine) = other.strip_prefix("engine:") {
                        if let Ok(engine) = serde_json::from_value(serde_json::Value::String(engine.into())) {
                            quill.set_engine(engine);
                        }
                    }
                }
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                crate::app::open_settings(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Refresh the icon tint and the menu (labels, engine checkmarks).
pub fn refresh(app: &AppHandle, quill: &Quill) {
    let Some(tray) = app.tray_by_id(ID) else { return };
    let recording = quill.controller.phase() == quill_core::state::Phase::Recording;
    let _ = tray.set_icon(Some(icon(recording)));
    let _ = tray.set_tooltip(Some(if recording { "Quill · Listening" } else { "Quill" }));
    if let Ok(menu) = build_menu(app, quill) {
        let _ = tray.set_menu(Some(menu));
    }
}
