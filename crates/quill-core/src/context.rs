//! App context: terminal, chat, or general prose (section 4.3).

use serde::Serialize;

use crate::settings::CleanupLevel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum AppContext {
    #[default]
    General,
    Chat,
    Terminal,
}

const TERMINALS: &[&str] = &[
    "WindowsTerminal.exe", "OpenConsole.exe", "conhost.exe", "cmd.exe", "powershell.exe",
    "pwsh.exe", "wezterm-gui.exe", "alacritty.exe", "Tabby.exe", "Hyper.exe", "mintty.exe",
    "WarpTerminal.exe", "kitty.exe",
];

const CHATS: &[&str] = &[
    "slack.exe", "Discord.exe", "Telegram.exe", "WhatsApp.exe", "Signal.exe", "ms-teams.exe",
    "Teams.exe", "Messenger.exe",
];

/// Packaged (Store) apps whose AUMID identifies a chat client.
const CHAT_AUMID_PREFIXES: &[&str] = &[
    "5319275A.WhatsAppDesktop", "MSTeams", "Microsoft.Teams", "TelegramMessengerLLP.TelegramDesktop",
    "FACEBOOK.317180B0BB486", "Facebook.Messenger",
];

fn has_word_terminal(name: &str) -> bool {
    name.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|w| w == "terminal")
}

/// Classify once, at recording start.
pub fn classify(exe: &str, focused_name: &str, aumid: Option<&str>, adapts_to_app: bool) -> AppContext {
    if !adapts_to_app {
        return AppContext::General;
    }
    let is = |list: &[&str]| list.iter().any(|e| e.eq_ignore_ascii_case(exe));
    if is(TERMINALS) || has_word_terminal(focused_name) {
        return AppContext::Terminal;
    }
    if is(CHATS) || aumid.is_some_and(|a| CHAT_AUMID_PREFIXES.iter().any(|p| a.starts_with(p))) {
        return AppContext::Chat;
    }
    AppContext::General
}

/// Chat turns `full` into `light`; a terminal turns any level into `verbatim`.
pub fn effective_level(level: CleanupLevel, context: AppContext) -> CleanupLevel {
    match (context, level) {
        (AppContext::Terminal, _) => CleanupLevel::Verbatim,
        (AppContext::Chat, CleanupLevel::Full) => CleanupLevel::Light,
        _ => level,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies() {
        assert_eq!(classify("WindowsTerminal.exe", "", None, true), AppContext::Terminal);
        assert_eq!(classify("pwsh.EXE", "", None, true), AppContext::Terminal);
        assert_eq!(classify("Code.exe", "Terminal 1, pwsh", None, true), AppContext::Terminal);
        assert_eq!(classify("Code.exe", "terminals.ts", None, true), AppContext::General);
        assert_eq!(classify("slack.exe", "", None, true), AppContext::Chat);
        assert_eq!(classify("ApplicationFrameHost.exe", "", Some("5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App"), true), AppContext::Chat);
        assert_eq!(classify("notepad.exe", "Text editor", None, true), AppContext::General);
        assert_eq!(classify("cmd.exe", "", None, false), AppContext::General);
    }

    #[test]
    fn overrides() {
        use CleanupLevel::*;
        assert_eq!(effective_level(Full, AppContext::Chat), Light);
        assert_eq!(effective_level(Light, AppContext::Chat), Light);
        assert_eq!(effective_level(Full, AppContext::Terminal), Verbatim);
        assert_eq!(effective_level(Full, AppContext::General), Full);
    }
}
