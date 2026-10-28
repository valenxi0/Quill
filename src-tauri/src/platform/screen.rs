//! Screens for Sketch: the monitor under the pointer, GDI screenshots of it,
//! and keeping Quill's own windows out of them.

use quill_core::sketch::{grid_bgra, Image, Place};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW, MonitorFromPoint,
    ReleaseDC, SelectObject, SetStretchBltMode, StretchBlt, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT,
    DIB_RGB_COLORS, HALFTONE, HBITMAP, HDC, MONITORINFO, MONITOR_DEFAULTTONEAREST, SRCCOPY,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetCursorPos, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, SetWindowDisplayAffinity,
    WindowFromPoint, GA_ROOT, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
};

use super::{target, uia};

/// One display, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Monitor {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    /// The work area (without the taskbar).
    pub work: (i32, i32, i32, i32),
    /// Physical pixels per logical pixel.
    pub scale: f32,
}

impl Monitor {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && y >= self.top && x < self.left + self.width && y < self.top + self.height
    }
}

pub fn monitor_under_pointer() -> Option<Monitor> {
    unsafe {
        let mut pt = POINT::default();
        GetCursorPos(&mut pt).ok()?;
        let handle = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(handle, &mut info).as_bool() {
            return None;
        }
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        let _ = GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        let r: RECT = info.rcMonitor;
        let w: RECT = info.rcWork;
        Some(Monitor {
            left: r.left,
            top: r.top,
            width: r.right - r.left,
            height: r.bottom - r.top,
            work: (w.left, w.top, w.right, w.bottom),
            scale: dpi_x.max(1) as f32 / 96.0,
        })
    }
}

/// A 32-bit top-down DIB section to draw into, freed on drop.
struct Canvas {
    screen: HDC,
    dc: HDC,
    bitmap: HBITMAP,
    bits: *mut u8,
    width: i32,
    height: i32,
}

impl Canvas {
    fn new(width: i32, height: i32) -> Option<Canvas> {
        unsafe {
            let screen = GetDC(None);
            if screen.is_invalid() {
                return None;
            }
            let dc = CreateCompatibleDC(Some(screen));
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let Ok(bitmap) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
                let _ = DeleteDC(dc);
                ReleaseDC(None, screen);
                return None;
            };
            SelectObject(dc, bitmap.into());
            Some(Canvas { screen, dc, bitmap, bits: bits as *mut u8, width, height })
        }
    }

    fn bgra(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.bits, (self.width * self.height * 4) as usize) }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.dc);
            let _ = DeleteObject(self.bitmap.into());
            ReleaseDC(None, self.screen);
        }
    }
}

/// The whole monitor as it is now. Windows marked with
/// `exclude_from_capture` are left out.
pub fn capture(m: &Monitor) -> Option<Image> {
    let canvas = Canvas::new(m.width, m.height)?;
    unsafe {
        BitBlt(canvas.dc, 0, 0, m.width, m.height, Some(canvas.screen), m.left, m.top, SRCCOPY | CAPTUREBLT).ok()?;
    }
    let mut rgba = canvas.bgra().to_vec();
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
        px[3] = 255;
    }
    Some(Image { width: m.width as u32, height: m.height as u32, rgba })
}

/// The monitor's brightness grid, from a small halftoned copy: cheap enough
/// to take a few times a second.
pub fn grid(m: &Monitor) -> Option<Vec<f32>> {
    const W: i32 = 128;
    const H: i32 = 80;
    let canvas = Canvas::new(W, H)?;
    unsafe {
        SetStretchBltMode(canvas.dc, HALFTONE);
        if !StretchBlt(canvas.dc, 0, 0, W, H, Some(canvas.screen), m.left, m.top, m.width, m.height, SRCCOPY | CAPTUREBLT)
            .as_bool()
        {
            return None;
        }
    }
    Some(grid_bgra(canvas.bgra(), W as usize, H as usize, W as usize * 4))
}

/// Keep a window out of screenshots and screen recordings (Windows 10 2004
/// and later), or let it back in. It still shows on the monitor.
pub fn exclude_from_capture(hwnd: isize) {
    set_capturable(hwnd, false);
}

pub fn set_capturable(hwnd: isize, capturable: bool) {
    let affinity = if capturable { WDA_NONE } else { WDA_EXCLUDEFROMCAPTURE };
    unsafe {
        let _ = SetWindowDisplayAffinity(HWND(hwnd as *mut _), affinity);
    }
}

fn window_title(hwnd: HWND) -> Option<String> {
    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]).trim().to_string();
    (!title.is_empty()).then_some(title)
}

/// The app and title of a top-level window, unless it is Quill's.
fn place_of(hwnd: HWND) -> Option<Place> {
    if hwnd.0.is_null() {
        return None;
    }
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    let root = if root.0.is_null() { hwnd } else { root };
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(root, Some(&mut pid)) };
    if pid == std::process::id() {
        return None;
    }
    Some(Place { app: target::app_name_for_pid(pid), window: window_title(root), element: None })
}

/// The app in front and its window title.
pub fn front_place() -> Place {
    place_of(unsafe { GetForegroundWindow() }).unwrap_or_default()
}

/// The app, window and element under a screen point (physical pixels).
pub fn place_at(x: i32, y: i32) -> Place {
    let hwnd = unsafe { WindowFromPoint(POINT { x, y }) };
    let Some(mut place) = place_of(hwnd) else { return front_place() };
    place.element = uia::run(150, move |u| u.describe_at(x, y)).flatten();
    place
}
