//! The Sketch windows: a transparent ink layer over one monitor and a small
//! bar with the controls. Both are created on first use, then hidden and
//! reused. Neither takes focus, and both are left out of screenshots.

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE, HWND_TOPMOST, SWP_NOACTIVATE, SW_HIDE,
    SW_SHOWNOACTIVATE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};

use crate::platform::screen::{exclude_from_capture, Monitor};
use crate::sketch::{bar_rect, SketchView};

pub const INK: &str = "sketch-ink";
pub const BAR: &str = "sketch-bar";

fn hwnd(app: &AppHandle, label: &str) -> Option<HWND> {
    let w = app.get_webview_window(label)?;
    Some(HWND(w.hwnd().ok()?.0))
}

pub fn own_hwnds(app: &AppHandle) -> Vec<isize> {
    [INK, BAR].iter().filter_map(|l| hwnd(app, l)).map(|h| h.0 as isize).collect()
}

fn no_activate(h: HWND) {
    unsafe {
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        let next = ex | (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST).0 as isize;
        if next != ex {
            SetWindowLongPtrW(h, GWL_EXSTYLE, next);
        }
    }
}

fn ensure(app: &AppHandle, label: &str, mode: &str) -> tauri::Result<()> {
    if app.get_webview_window(label).is_some() {
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::App(format!("sketch.html?mode={mode}").into()))
        .title("Quill Sketch")
        .inner_size(400.0, 100.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false)
        .visible(false)
        .build()?;
    let h = HWND(window.hwnd()?.0);
    no_activate(h);
    exclude_from_capture(h.0 as isize);
    if label == INK {
        window.set_ignore_cursor_events(true)?;
    }
    Ok(())
}

/// Create both windows ahead of time, so the first Sketch opens at once.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    ensure(app, INK, "ink")?;
    ensure(app, BAR, "bar")
}

fn place(h: HWND, (left, top, right, bottom): (i32, i32, i32, i32)) {
    unsafe {
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), left, top, right - left, bottom - top, SWP_NOACTIVATE);
        let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
    }
}

/// The dictation pill stays out of Sketch screenshots, and only those: the
/// rest of the time it shows in the user's own screenshots and recordings.
fn pill_capturable(app: &AppHandle, capturable: bool) {
    if let Some(h) = crate::overlay::own_hwnd(app) {
        crate::platform::screen::set_capturable(h, capturable);
    }
}

pub fn show(app: &AppHandle, m: &Monitor, view: &SketchView) {
    if let Err(e) = create(app) {
        log::warn!("sketch windows: {e}");
        return;
    }
    pill_capturable(app, false);
    let _ = app.emit_to(INK, "sketch-start", m.scale);
    let _ = app.emit_to(BAR, "sketch-view", view);
    if let Some(w) = app.get_webview_window(INK) {
        let _ = w.set_ignore_cursor_events(true);
    }
    // The bar goes on top of the ink, so it stays clickable while Draw is on.
    if let Some(h) = hwnd(app, INK) {
        no_activate(h);
        place(h, (m.left, m.top, m.left + m.width, m.top + m.height));
    }
    if let Some(h) = hwnd(app, BAR) {
        no_activate(h);
        place(h, bar_rect(m));
    }
}

/// With Draw on, the ink layer takes the clicks (and shows a crosshair), so
/// dragging draws instead of reaching the app underneath.
pub fn set_pen(app: &AppHandle, on: bool) {
    if let Some(w) = app.get_webview_window(INK) {
        let _ = w.set_ignore_cursor_events(!on);
    }
    if let Some(h) = hwnd(app, INK) {
        no_activate(h);
    }
    let _ = app.emit_to(INK, "sketch-pen", on);
}

pub fn hide(app: &AppHandle) {
    pill_capturable(app, true);
    let _ = app.emit_to(INK, "sketch-end", ());
    if let Some(w) = app.get_webview_window(INK) {
        let _ = w.set_ignore_cursor_events(true);
    }
    // The ink fades out first.
    let app = app.clone();
    std::thread::spawn(move || {
        if let Some(h) = hwnd(&app, BAR) {
            unsafe {
                let _ = ShowWindow(h, SW_HIDE);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(380));
        if let Some(h) = hwnd(&app, INK) {
            unsafe {
                let _ = ShowWindow(h, SW_HIDE);
            }
        }
    });
}
