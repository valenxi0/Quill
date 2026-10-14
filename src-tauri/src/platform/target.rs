//! The paste target, captured at recording start (section 4.2, step 1).

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
};

use super::uia;

#[derive(Debug, Clone, Default)]
pub struct Target {
    pub hwnd: isize,
    pub exe: String,
    pub exe_path: String,
    pub app_name: String,
    pub aumid: Option<String>,
    pub focused_name: String,
    pub is_password: bool,
    pub element_id: Option<u64>,
    /// False for the desktop, the taskbar, or no window at all.
    pub editable: bool,
    /// The target runs elevated and Quill doesn't (UIPI blocks paste).
    pub elevated: bool,
    /// The text around the caret at recording start, when read.
    pub surroundings: Option<quill_core::text::insertion::Surroundings>,
}

impl Target {
    pub fn hwnd(&self) -> HWND {
        HWND(self.hwnd as *mut _)
    }

    pub fn is_alive(&self) -> bool {
        self.hwnd != 0 && unsafe { IsWindow(Some(self.hwnd())).as_bool() }
    }
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

struct Process(HANDLE);

impl Drop for Process {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn exe_path(process: &Process) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    unsafe { QueryFullProcessImageNameW(process.0, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).ok()? };
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

fn aumid(process: &Process) -> Option<String> {
    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    let rc = unsafe { GetApplicationUserModelId(process.0, &mut len, Some(PWSTR(buf.as_mut_ptr()))) };
    (rc.0 == 0).then(|| String::from_utf16_lossy(&buf[..len.saturating_sub(1) as usize]))
}

fn token_elevated(process: HANDLE) -> Option<bool> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let mut elevation = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        let _ = CloseHandle(token);
        ok.ok()?;
        Some(elevation.TokenIsElevated != 0)
    }
}

pub fn self_elevated() -> bool {
    token_elevated(unsafe { GetCurrentProcess() }).unwrap_or(false)
}

/// The file description from the exe's version resource ("Notepad").
pub fn file_description(path: &str) -> Option<String> {
    let wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(wide.as_ptr()), None, size, data.as_mut_ptr() as *mut _).ok()?;
        // First translation, then the FileDescription string.
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        let q: Vec<u16> = "\\VarFileInfo\\Translation\0".encode_utf16().collect();
        if !VerQueryValueW(data.as_ptr() as *const _, PCWSTR(q.as_ptr()), &mut ptr, &mut len).as_bool() || len < 4 {
            return None;
        }
        let lang = *(ptr as *const u16);
        let cp = *(ptr as *const u16).add(1);
        let key = format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\FileDescription\0");
        let q: Vec<u16> = key.encode_utf16().collect();
        if !VerQueryValueW(data.as_ptr() as *const _, PCWSTR(q.as_ptr()), &mut ptr, &mut len).as_bool() || len == 0 {
            return None;
        }
        let slice = std::slice::from_raw_parts(ptr as *const u16, len as usize);
        let s = String::from_utf16_lossy(slice).trim_end_matches('\0').trim().to_string();
        (!s.is_empty()).then_some(s)
    }
}

/// The full image path of a running process.
pub(crate) fn process_path(pid: u32) -> Option<String> {
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    exe_path(&Process(h))
}

pub(crate) fn display_name(exe_path: &str, exe: &str) -> String {
    file_description(exe_path)
        .unwrap_or_else(|| exe.trim_end_matches(".exe").trim_end_matches(".EXE").to_string())
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownApp {
    pub exe: String,
    pub name: String,
    pub icon: Option<String>,
}

pub fn open_apps() -> Vec<KnownApp> {
    unsafe extern "system" fn collect(hwnd: HWND, param: LPARAM) -> windows::core::BOOL {
        if !unsafe { IsWindowVisible(hwnd) }.as_bool() { return true.into(); }
        let apps = unsafe { &mut *(param.0 as *mut Vec<KnownApp>) };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)); }
        if let Ok(h) = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
            let process = Process(h);
            if let Some(path) = exe_path(&process) {
                let exe = path.rsplit('\\').next().unwrap_or(&path).to_lowercase();
                if !matches!(exe.as_str(), "quill.exe" | "explorer.exe") && !apps.iter().any(|a| a.exe == exe) {
                    apps.push(KnownApp { name: display_name(&path, &exe), exe, icon: super::icons::file_icon(&path) });
                }
            }
        }
        true.into()
    }
    let mut apps = Vec::new();
    unsafe { let _ = EnumWindows(Some(collect), LPARAM(&mut apps as *mut Vec<KnownApp> as isize)); }
    apps
}

/// Capture the foreground window and its focused element. With `around`,
/// also read the text around the caret.
pub fn capture(around: bool) -> Target {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return Target::default();
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let class = class_name(hwnd);
    let shell = matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd");

    let mut t = Target { hwnd: hwnd.0 as isize, editable: !shell, ..Default::default() };
    match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
        Ok(h) => {
            let process = Process(h);
            if let Some(path) = exe_path(&process) {
                t.exe = path.rsplit('\\').next().unwrap_or(&path).to_string();
                t.app_name = display_name(&path, &t.exe);
                t.exe_path = path;
            }
            t.aumid = aumid(&process);
            // A token we can't query belongs to a more privileged process.
            t.elevated = !self_elevated() && token_elevated(process.0).unwrap_or(true);
        }
        Err(_) => t.elevated = !self_elevated(),
    }
    if let Some(focus) = uia::run(if around { 600 } else { 400 }, move |u| u.focused(around)) {
        // Only trust the focused element if it belongs to the foreground app.
        if focus.pid == pid || focus.pid == 0 {
            t.focused_name = focus.name;
            t.is_password = focus.is_password;
            t.element_id = focus.element_id;
            t.surroundings = focus.surroundings;
        }
    }
    t
}
