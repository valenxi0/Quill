//! Launchers (section 4.10): `quill://start|stop|toggle|cancel|paste-last|sketch` and the same
//! commands as CLI flags, for Stream Deck, AutoHotkey, PowerToys and friends.

pub const COMMANDS: [&str; 6] = ["start", "stop", "toggle", "cancel", "paste-last", "sketch"];

/// The dictation command in a list of args or URLs, if any.
pub fn command_from_args<S: AsRef<str>>(args: &[S]) -> Option<&'static str> {
    args.iter().find_map(|a| {
        let a = a.as_ref().trim().trim_end_matches('/').to_ascii_lowercase();
        let name = a.strip_prefix("quill://").or_else(|| a.strip_prefix("--"))?;
        COMMANDS.iter().copied().find(|c| *c == name)
    })
}

/// Launched by Windows at login: stay in the background.
pub fn is_autostart<S: AsRef<str>>(args: &[S]) -> bool {
    args.iter().any(|a| a.as_ref() == "--autostart")
}

fn autostart_command(exe: &std::path::Path) -> String {
    format!("\"{}\" --autostart", exe.display())
}

/// auto-launch 0.5 writes an unquoted path. Correct it after enabling.
pub fn quote_autostart_path(app_name: &str) -> Result<(), String> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let command: Vec<u16> = autostart_command(&exe).encode_utf16().chain([0]).collect();
    let name: Vec<u16> = app_name.encode_utf16().chain([0]).collect();
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            PCWSTR(name.as_ptr()),
            REG_SZ.0,
            Some(command.as_ptr().cast()),
            (command.len() * 2) as u32,
        )
        .ok()
        .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_command_quotes_executable_paths() {
        assert_eq!(
            autostart_command(std::path::Path::new(r"C:\Users\A User\Quill\Quill.exe")),
            r#""C:\Users\A User\Quill\Quill.exe" --autostart"#
        );
        assert_eq!(command_from_args(&["Quill.exe", "--autostart"]), None);
    }

    #[test]
    fn parses_links_and_flags() {
        assert_eq!(command_from_args(&["Quill.exe", "--toggle"]), Some("toggle"));
        assert_eq!(command_from_args(&["Quill.exe", "quill://start/"]), Some("start"));
        assert_eq!(command_from_args(&["Quill.exe", "QUILL://Cancel"]), Some("cancel"));
        assert_eq!(command_from_args(&["Quill.exe", "--nope"]), None);
        assert_eq!(command_from_args::<&str>(&[]), None);
        assert!(is_autostart(&["Quill.exe", "--autostart"]));
    }
}
