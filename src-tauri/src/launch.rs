//! Launchers (section 4.10): `quill://start|stop|toggle|cancel` and the same
//! commands as CLI flags, for Stream Deck, AutoHotkey, PowerToys and friends.

pub const COMMANDS: [&str; 4] = ["start", "stop", "toggle", "cancel"];

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

#[cfg(test)]
mod tests {
    use super::*;

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
