//! The status overlay window (section 5.3): a small always-on-top pill at the
//! bottom centre of the monitor holding the foreground window. It must never
//! take focus, or the paste lands in the wrong place.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE, HWND_TOPMOST,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST,
};

pub const LABEL: &str = "overlay";
/// Logical size of the overlay window; the pill is drawn inside it. Wide
/// enough for the waveform plus a line of live words.
const WIDTH: f64 = 440.0;
const HEIGHT: f64 = 72.0;
/// Gap above the taskbar, in logical pixels.
const MARGIN: f64 = 24.0;

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("overlay.html".into()))
        .title("Quill")
        .inner_size(WIDTH, HEIGHT)
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
    apply_styles(HWND(window.hwnd()?.0));
    // Click-through (layered + transparent), managed by the window layer.
    window.set_ignore_cursor_events(true)?;
    Ok(())
}

/// No-activate, tool window, topmost. Re-applied on every show in case the
/// window layer reset them.
fn apply_styles(hwnd: HWND) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let next = ex | (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST).0 as isize;
        if next != ex {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next);
            let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
        }
    }
}

fn hwnd(app: &AppHandle) -> Option<HWND> {
    let w = app.get_webview_window(LABEL)?;
    Some(HWND(w.hwnd().ok()?.0))
}

pub fn own_hwnd(app: &AppHandle) -> Option<isize> {
    hwnd(app).map(|h| h.0 as isize)
}

/// Show without activating, at the bottom centre of the foreground monitor.
pub fn show(app: &AppHandle) {
    let Some(h) = hwnd(app) else { return };
    apply_styles(h);
    let Some(window) = app.get_webview_window(LABEL) else { return };
    let scale = window.scale_factor().unwrap_or(1.0);
    unsafe {
        let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let work: RECT = info.rcWork;
            let w = (WIDTH * scale) as i32;
            let hgt = (HEIGHT * scale) as i32;
            let x = work.left + (work.right - work.left - w) / 2;
            let y = work.bottom - hgt - (MARGIN * scale) as i32;
            let _ = SetWindowPos(h, Some(HWND_TOPMOST), x, y, w, hgt, SWP_NOACTIVATE);
        }
        let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
    }
}

pub fn hide(app: &AppHandle) {
    if let Some(h) = hwnd(app) {
        unsafe {
            let _ = ShowWindow(h, SW_HIDE);
        }
    }
}

/// Click-through everywhere, except while a correction suggestion offers its
/// Add / Not now buttons.
pub fn set_interactive(app: &AppHandle, interactive: bool) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.set_ignore_cursor_events(!interactive);
        if let Ok(h) = w.hwnd() {
            apply_styles(HWND(h.0));
        }
    }
}
