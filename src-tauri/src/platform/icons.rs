use base64::{engine::general_purpose::STANDARD, Engine};
use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_MASK, DI_NORMAL};

/// Windows owns the source artwork; all returned handles are released here.
pub fn file_icon(path: &str) -> Option<String> {
    static CACHE: std::sync::OnceLock<
        parking_lot::Mutex<std::collections::HashMap<String, String>>,
    > = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(icon) = cache.lock().get(path).cloned() {
        return Some(icon);
    }
    let icon = extract(path)?;
    cache.lock().insert(path.to_string(), icon.clone());
    Some(icon)
}

fn extract(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
    unsafe {
        let mut info = SHFILEINFOW::default();
        if SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        ) == 0
        {
            return None;
        }
        let result = render(info.hIcon);
        let _ = DestroyIcon(info.hIcon);
        result
    }
}

unsafe fn render(icon: windows::Win32::UI::WindowsAndMessaging::HICON) -> Option<String> {
    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.0.is_null() {
        return None;
    }
    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: 32,
        biHeight: -32,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let bitmap = unsafe { CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) };
    let result = if let Ok(bitmap) = bitmap {
        let old = unsafe { SelectObject(dc, bitmap.into()) };
        unsafe {
            std::ptr::write_bytes(bits, 0, 32 * 32 * 4);
        }
        let drawn = unsafe { DrawIconEx(dc, 0, 0, icon, 32, 32, 0, None, DI_NORMAL) }.is_ok();
        unsafe {
            let _ = GdiFlush();
        }
        let mut rgba =
            unsafe { std::slice::from_raw_parts(bits as *const u8, 32 * 32 * 4) }.to_vec();
        if drawn && rgba.chunks_exact(4).all(|p| p[3] == 0) {
            // Older icons use an AND mask rather than an alpha channel.
            if unsafe { DrawIconEx(dc, 0, 0, icon, 32, 32, 0, None, DI_MASK) }.is_ok() {
                unsafe {
                    let _ = GdiFlush();
                }
                let mask = unsafe { std::slice::from_raw_parts(bits as *const u8, 32 * 32 * 4) };
                for (pixel, mask) in rgba.chunks_exact_mut(4).zip(mask.chunks_exact(4)) {
                    pixel[3] = if mask[0] == 0 { 255 } else { 0 };
                }
            }
        }
        unsafe {
            let _ = SelectObject(dc, old);
            let _ = DeleteObject(bitmap.into());
        }
        if drawn {
            // DrawIconEx produces premultiplied BGRA. PNG expects straight RGBA.
            for p in rgba.chunks_exact_mut(4) {
                p.swap(0, 2);
                if p[3] > 0 {
                    for i in 0..3 {
                        p[i] = ((p[i] as u32 * 255) / p[3] as u32).min(255) as u8;
                    }
                }
            }
            encode(&rgba)
        } else {
            None
        }
    } else {
        None
    };
    unsafe {
        let _ = DeleteDC(dc);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_a_visible_png_from_a_real_windows_executable() {
        let path = std::env::current_exe().unwrap();
        let icon = file_icon(path.to_str().unwrap()).expect("Windows executable icon");
        let bytes = STANDARD
            .decode(icon.strip_prefix("data:image/png;base64,").unwrap())
            .unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut pixels).unwrap();
        assert_eq!(
            (frame.width, frame.height, frame.color_type),
            (32, 32, png::ColorType::Rgba)
        );
        assert!(pixels
            .chunks_exact(4)
            .any(|p| p[3] > 0 && p[..3].iter().any(|c| *c > 0)));
        assert_eq!(file_icon(""), None);
    }
}

fn encode(rgba: &[u8]) -> Option<String> {
    let mut data = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut data, 32, 32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().ok()?.write_image_data(rgba).ok()?;
    }
    Some(format!("data:image/png;base64,{}", STANDARD.encode(data)))
}
