//! Finding an app's executable from its file name, so apps that aren't open
//! still get their icon: Start menu shortcuts, the App Paths and Uninstall
//! registry keys, packaged (Microsoft Store and MSIX) apps, then running
//! processes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use windows::core::{Interface, HSTRING, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, STGM_READ};
use windows::Win32::System::ProcessStatus::EnumProcesses;
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ,
    RRF_RT_REG_SZ,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

fn file_name(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_lowercase()
}

/// The path of a running process with this file name.
fn running(exe: &str) -> Option<String> {
    let mut pids = vec![0u32; 4096];
    let mut needed = 0u32;
    unsafe { EnumProcesses(pids.as_mut_ptr(), (pids.len() * 4) as u32, &mut needed).ok()? };
    pids.truncate(needed as usize / 4);
    pids.into_iter().filter(|p| *p != 0).find_map(|pid| super::target::process_path(pid).filter(|p| file_name(p) == exe))
}

fn reg_string(root: HKEY, key: &str, value: Option<&str>) -> Option<String> {
    let key = HSTRING::from(key);
    let value = value.map(HSTRING::from);
    let name = value.as_ref().map_or(PCWSTR::null(), |v| PCWSTR(v.as_ptr()));
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let rc = unsafe { RegGetValueW(root, &key, name, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut len)) };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let s = String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]);
    (!s.trim().is_empty()).then(|| s.trim().to_string())
}

fn subkeys(root: HKEY, key: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut handle = HKEY::default();
    if unsafe { RegOpenKeyExW(root, &HSTRING::from(key), None, KEY_READ, &mut handle) } != ERROR_SUCCESS {
        return out;
    }
    for i in 0.. {
        let mut name = [0u16; 512];
        let mut len = name.len() as u32;
        let rc = unsafe { RegEnumKeyExW(handle, i, Some(windows::core::PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None) };
        if rc != ERROR_SUCCESS {
            break;
        }
        out.push(String::from_utf16_lossy(&name[..len as usize]));
    }
    unsafe {
        let _ = RegCloseKey(handle);
    }
    out
}

/// `"C:\x\app.exe",0` or `C:\x\app.exe` → the exe path.
fn icon_path(value: &str) -> Option<String> {
    let v = value.trim();
    let v = v.strip_prefix('"').map_or(v, |rest| rest.split('"').next().unwrap_or(rest));
    let v = v.split(',').next()?.trim();
    v.to_lowercase().ends_with(".exe").then(|| v.to_string())
}

fn shortcut_target(link: &IShellLinkW, path: &Path) -> Option<String> {
    let file: IPersistFile = link.cast().ok()?;
    unsafe { file.Load(&HSTRING::from(path.as_os_str()), STGM_READ).ok()? };
    let mut buf = [0u16; 1024];
    unsafe { link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()? };
    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    let target = String::from_utf16_lossy(&buf[..len]);
    target.to_lowercase().ends_with(".exe").then_some(target)
}

fn shortcuts(dir: &Path, link: &IShellLinkW, index: &mut HashMap<String, String>, depth: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && depth < 4 {
            shortcuts(&path, link, index, depth + 1);
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) {
            if let Some(target) = shortcut_target(link, &path) {
                index.entry(file_name(&target)).or_insert(target);
            }
        }
    }
}

/// Executables declared by installed packages, from their manifests.
fn packaged(index: &mut HashMap<String, String>) {
    const PACKAGES: &str = r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages";
    for package in subkeys(HKEY_CURRENT_USER, PACKAGES) {
        // Frameworks and system packages declare no apps worth showing.
        if package.starts_with("Microsoft.VCLibs") || package.starts_with("Microsoft.NET") || package.starts_with("Microsoft.UI.Xaml") {
            continue;
        }
        let Some(root) = reg_string(HKEY_CURRENT_USER, &format!(r"{PACKAGES}\{package}"), Some("PackageRootFolder")) else { continue };
        let Ok(manifest) = std::fs::read_to_string(Path::new(&root).join("AppxManifest.xml")) else { continue };
        for part in manifest.split("Executable=\"").skip(1) {
            let Some(relative) = part.split('"').next() else { continue };
            if relative.to_lowercase().ends_with(".exe") {
                let full = Path::new(&root).join(relative.replace('/', "\\"));
                index.entry(file_name(relative)).or_insert_with(|| full.to_string_lossy().into_owned());
            }
        }
    }
}

/// Every installed app's exe, by lowercase file name. Built once.
fn installed() -> &'static HashMap<String, String> {
    static INDEX: OnceLock<HashMap<String, String>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut index = HashMap::new();
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        if let Ok(link) = unsafe { CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) } {
            let dirs = [
                std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(r"Microsoft\Windows\Start Menu\Programs")),
                std::env::var_os("ProgramData").map(|a| PathBuf::from(a).join(r"Microsoft\Windows\Start Menu\Programs")),
            ];
            for dir in dirs.into_iter().flatten() {
                shortcuts(&dir, &link, &mut index, 0);
            }
        }
        for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            const APP_PATHS: &str = r"Software\Microsoft\Windows\CurrentVersion\App Paths";
            for exe in subkeys(root, APP_PATHS) {
                if let Some(path) = reg_string(root, &format!(r"{APP_PATHS}\{exe}"), None).and_then(|p| icon_path(&p)) {
                    index.entry(exe.to_lowercase()).or_insert(path);
                }
            }
            const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
            for app in subkeys(root, UNINSTALL) {
                if let Some(path) = reg_string(root, &format!(r"{UNINSTALL}\{app}"), Some("DisplayIcon")).and_then(|p| icon_path(&p)) {
                    index.entry(file_name(&path)).or_insert(path);
                }
            }
        }
        packaged(&mut index);
        index.retain(|_, path| Path::new(path).is_file());
        index
    })
}

/// The full path of an app's executable, from its file name.
pub fn find_exe(exe: &str) -> Option<String> {
    let exe = exe.to_lowercase();
    if exe.is_empty() {
        return None;
    }
    if std::env::current_exe().is_ok_and(|p| file_name(&p.to_string_lossy()) == exe) {
        return std::env::current_exe().ok().map(|p| p.to_string_lossy().into_owned());
    }
    // Installed apps first: a console tool can share a desktop app's file
    // name (claude.exe), and dictation targets are windowed apps.
    installed().get(&exe).cloned().or_else(|| running(&exe))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_paths() {
        assert_eq!(icon_path(r#""C:\A B\app.exe",0"#).as_deref(), Some(r"C:\A B\app.exe"));
        assert_eq!(icon_path(r"C:\x\app.exe").as_deref(), Some(r"C:\x\app.exe"));
        assert_eq!(icon_path(r"C:\x\app.ico"), None);
    }

    #[test]
    fn finds_explorer_and_itself() {
        assert!(find_exe("explorer.exe").is_some_and(|p| p.to_lowercase().ends_with(r"\explorer.exe")));
        let me = std::env::current_exe().unwrap();
        let name = me.file_name().unwrap().to_string_lossy().to_string();
        assert!(find_exe(&name).is_some());
        assert_eq!(find_exe(""), None);
    }
}
