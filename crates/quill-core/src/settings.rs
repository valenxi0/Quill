//! Persisted settings (section 3), stored at `%APPDATA%\Quill\settings.json`.

use serde::{Deserialize, Serialize};

use crate::engines::EngineId;
use crate::text::vocabulary::VocabEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ActivationMode {
    #[default]
    PushToTalk,
    Toggle,
}

impl ActivationMode {
    pub fn detail(self) -> &'static str {
        match self {
            Self::PushToTalk => "Hold the shortcut while you speak. Release to finish.",
            Self::Toggle => "Press the shortcut once to start and again to finish.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum CleanupLevel {
    Verbatim,
    Light,
    #[default]
    Full,
}

impl CleanupLevel {
    /// The S1-mini styling value; only the trained values are valid.
    pub fn styling(self) -> Option<&'static str> {
        match self {
            Self::Verbatim => None,
            Self::Light => Some("casual"),
            Self::Full => Some("semi-formal"),
        }
    }
}

pub mod modifiers {
    pub const CTRL: u32 = 1;
    pub const ALT: u32 = 2;
    pub const SHIFT: u32 = 4;
    pub const WIN: u32 = 8;
}

/// A global shortcut: a Windows virtual-key code plus modifier bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hotkey {
    pub vk: u32,
    pub modifiers: u32,
}

impl Default for Hotkey {
    /// Ctrl+Alt+M.
    fn default() -> Self {
        Self { vk: 0x4D, modifiers: modifiers::CTRL | modifiers::ALT }
    }
}

const VK_H: u32 = 0x48;
const VK_ESCAPE: u32 = 0x1B;

impl Hotkey {
    /// Why this chord can't be the dictation shortcut, if it can't.
    pub fn problem(&self) -> Option<String> {
        if self.modifiers & 0xF == 0 {
            return Some("The shortcut needs at least one modifier key (Ctrl, Alt, Shift, or Win).".into());
        }
        if is_modifier_vk(self.vk) || self.vk == 0 {
            return Some("The shortcut needs a key besides the modifiers.".into());
        }
        if self.vk == VK_ESCAPE {
            return Some("Esc cancels dictation and can't be the shortcut.".into());
        }
        if self.vk == VK_H && self.modifiers == modifiers::WIN {
            return Some("Win+H is Windows' own dictation. Choose another shortcut.".into());
        }
        None
    }

    /// "Ctrl+Alt+M".
    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        for (bit, name) in [
            (modifiers::CTRL, "Ctrl"),
            (modifiers::ALT, "Alt"),
            (modifiers::SHIFT, "Shift"),
            (modifiers::WIN, "Win"),
        ] {
            if self.modifiers & bit != 0 {
                parts.push(name.to_string());
            }
        }
        parts.push(vk_name(self.vk));
        parts.join("+")
    }
}

pub fn is_modifier_vk(vk: u32) -> bool {
    // Shift, Ctrl, Alt (generic and left/right) and the Windows keys.
    matches!(vk, 0x10 | 0x11 | 0x12 | 0xA0..=0xA5 | 0x5B | 0x5C)
}

pub fn vk_name(vk: u32) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from(vk as u8).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x60..=0x69 => format!("Num {}", vk - 0x60),
        0x20 => "Space".into(),
        0x09 => "Tab".into(),
        0x0D => "Enter".into(),
        0x08 => "Backspace".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        0x24 => "Home".into(),
        0x23 => "End".into(),
        0x21 => "Page Up".into(),
        0x22 => "Page Down".into(),
        0x25 => "Left".into(),
        0x26 => "Up".into(),
        0x27 => "Right".into(),
        0x28 => "Down".into(),
        0x13 => "Pause".into(),
        0x91 => "Scroll Lock".into(),
        0xBA => ";".into(),
        0xBB => "=".into(),
        0xBC => ",".into(),
        0xBD => "-".into(),
        0xBE => ".".into(),
        0xBF => "/".into(),
        0xC0 => "`".into(),
        0xDB => "[".into(),
        0xDC => "\\".into(),
        0xDD => "]".into(),
        0xDE => "'".into(),
        _ => format!("Key {vk:#04X}"),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VoiceDictation {
    pub engine: EngineId,
    pub activation_mode: ActivationMode,
    pub hotkey: Hotkey,
    pub cleanup_level: CleanupLevel,
    pub language: String,
    pub writes_numbers: bool,
    pub adapts_to_app: bool,
    pub learns_corrections: bool,
    pub history_enabled: bool,
    pub insights_enabled: bool,
    pub vocabulary: Vec<VocabEntry>,
}

impl Default for VoiceDictation {
    fn default() -> Self {
        Self {
            engine: EngineId::ParakeetS1,
            activation_mode: ActivationMode::PushToTalk,
            hotkey: Hotkey::default(),
            cleanup_level: CleanupLevel::Full,
            language: "auto".into(),
            writes_numbers: true,
            adapts_to_app: true,
            learns_corrections: true,
            history_enabled: false,
            insights_enabled: true,
            vocabulary: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub enabled: bool,
    pub voice_dictation: VoiceDictation,
    pub launch_at_login: bool,
    /// UI state: whether the history card is expanded.
    pub history_expanded: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            voice_dictation: VoiceDictation::default(),
            launch_at_login: true,
            history_expanded: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_key_names() {
        let json = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(json["enabled"], true);
        assert_eq!(json["launchAtLogin"], true);
        let vd = &json["voiceDictation"];
        assert_eq!(vd["engine"], "parakeetS1");
        assert_eq!(vd["activationMode"], "pushToTalk");
        assert_eq!(vd["cleanupLevel"], "full");
        assert_eq!(vd["language"], "auto");
        assert_eq!(vd["historyEnabled"], false);
        assert_eq!(vd["hotkey"]["vk"], 0x4D);
    }

    #[test]
    fn partial_files_fill_defaults() {
        let s: Settings = serde_json::from_str(r#"{"voiceDictation":{"engine":"whisperTiny"}}"#).unwrap();
        assert_eq!(s.voice_dictation.engine, EngineId::WhisperTiny);
        assert!(s.voice_dictation.writes_numbers);
        assert!(s.enabled);
    }

    #[test]
    fn hotkey_display_and_problems() {
        assert_eq!(Hotkey::default().display(), "Ctrl+Alt+M");
        assert_eq!(Hotkey { vk: 0x70, modifiers: modifiers::SHIFT | modifiers::WIN }.display(), "Shift+Win+F1");
        assert!(Hotkey::default().problem().is_none());
        assert!(Hotkey { vk: 0x4D, modifiers: 0 }.problem().is_some());
        assert!(Hotkey { vk: 0x48, modifiers: modifiers::WIN }.problem().is_some());
        assert!(Hotkey { vk: 0x11, modifiers: modifiers::ALT }.problem().is_some());
    }
}
