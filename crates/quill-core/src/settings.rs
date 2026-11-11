//! Persisted settings (section 3), stored at `%APPDATA%\Quill\settings.json`.

use serde::{Deserialize, Serialize};

use crate::engines::EngineId;
use crate::text::vocabulary::{EntryKind, VocabEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ActivationMode {
    #[default]
    PushToTalk,
    Toggle,
    /// Hold to talk, or double-tap to keep recording until the next press.
    HoldOrLock,
}

impl ActivationMode {
    pub fn detail(self) -> &'static str {
        match self {
            Self::PushToTalk => "Hold the shortcut while you speak. Release to finish.",
            Self::Toggle => "Press the shortcut once to start and again to finish.",
            Self::HoldOrLock => "Hold the shortcut while you speak, or tap it twice to keep recording until you press it again.",
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
    Compose,
}

pub mod modifiers {
    pub const CTRL: u32 = 1;
    pub const ALT: u32 = 2;
    pub const SHIFT: u32 = 4;
    pub const WIN: u32 = 8;
}

/// A global shortcut: a Windows virtual-key code plus modifier bits. A
/// mouse side button may have no modifiers, and so may Right Ctrl or Right
/// Alt pressed alone.
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
pub const VK_XBUTTON1: u32 = 0x05;
pub const VK_XBUTTON2: u32 = 0x06;
pub const VK_RCONTROL: u32 = 0xA3;
pub const VK_RMENU: u32 = 0xA5;

impl Hotkey {
    /// A mouse side button, with or without modifiers.
    pub fn is_mouse(&self) -> bool {
        matches!(self.vk, VK_XBUTTON1 | VK_XBUTTON2)
    }

    /// Right Ctrl or Right Alt pressed alone.
    pub fn is_lone_modifier(&self) -> bool {
        self.modifiers == 0 && matches!(self.vk, VK_RCONTROL | VK_RMENU)
    }

    /// Why this chord can't be the dictation shortcut, if it can't.
    pub fn problem(&self) -> Option<String> {
        if self.is_mouse() || self.is_lone_modifier() {
            return None;
        }
        if self.modifiers & 0xF == 0 {
            return Some(
                "The shortcut needs at least one modifier key (Ctrl, Alt, Shift, or Win). Right Ctrl, Right Alt, and the mouse side buttons also work alone."
                    .into(),
            );
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
        if self.is_system_chord() {
            return Some(format!("{} belongs to Windows. Choose another shortcut.", self.display()));
        }
        None
    }

    /// Chords Windows itself uses; taking them would break the desktop.
    fn is_system_chord(&self) -> bool {
        const TAB: u32 = 0x09;
        const F4: u32 = 0x73;
        const L: u32 = 0x4C;
        const D: u32 = 0x44;
        const E: u32 = 0x45;
        const R: u32 = 0x52;
        const DELETE: u32 = 0x2E;
        let (vk, m) = (self.vk, self.modifiers);
        let alt = modifiers::ALT;
        let win = modifiers::WIN;
        (vk == TAB && m & (alt | win) != 0)
            || (vk == F4 && m == alt)
            || (vk == DELETE && m == modifiers::CTRL | alt)
            || (m == win && matches!(vk, L | D | E | R))
    }

    /// Worth knowing about a shortcut that works.
    pub fn note(&self) -> Option<&'static str> {
        (self.is_lone_modifier() && self.vk == VK_RMENU)
            .then_some("On keyboards with AltGr, Right Alt types characters and won't start dictation. Pick Right Ctrl there.")
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
        VK_XBUTTON1 => "Mouse back".into(),
        VK_XBUTTON2 => "Mouse forward".into(),
        VK_RCONTROL => "Right Ctrl".into(),
        VK_RMENU => "Right Alt".into(),
        _ => format!("Key {vk:#04X}"),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VoiceDictation {
    pub engine: EngineId,
    pub activation_mode: ActivationMode,
    pub hotkey: Hotkey,
    /// A second shortcut that dictates without cleanup, in the same mode.
    pub raw_hotkey: Option<Hotkey>,
    pub cleanup_level: CleanupLevel,
    pub formal_register: bool,
    pub layout_cues: bool,
    pub language: String,
    pub writes_numbers: bool,
    pub adapts_to_app: bool,
    pub learns_corrections: bool,
    pub history_enabled: bool,
    pub insights_enabled: bool,
    pub vocabulary: Vec<VocabEntry>,
    /// Cleanup styles for specific apps, overriding `cleanup_level`.
    pub app_styles: Vec<AppStyle>,
    /// Match casing, spaces, and the full stop to the text around the caret.
    pub fits_to_cursor: bool,
    /// A dictation ending in "press enter" pastes, then presses Enter.
    pub spoken_enter: bool,
    /// Paste long text into terminals in pieces, so TUIs don't collapse it.
    pub splits_terminal_pastes: bool,
    /// Show a provisional transcript while recording.
    pub live_preview: bool,
    /// The microphone to record from; `None` follows the Windows default.
    pub input_device: Option<SavedDevice>,
    /// Skip a Bluetooth headset mic when another mic exists: recording from
    /// one switches the headset to low-quality call audio.
    pub avoid_bluetooth_mic: bool,
    /// What happens to other apps' audio while recording.
    pub other_audio: OtherAudio,
    /// Volume other apps drop to, as a percent of their own, for `Lower`.
    pub lower_to_percent: u8,
    /// Parakeet listens for vocabulary terms while it recognizes. Experimental:
    /// the beam search it needs can drop words after long pauses.
    pub vocabulary_boost: bool,
}

impl VoiceDictation {
    /// Parakeet hotwords are on: the Vocabulary switch is on and a vocabulary
    /// term is active in any app. One answer for every app, so moving between
    /// apps never rebuilds the recognizer.
    pub fn wants_hotwords(&self) -> bool {
        self.vocabulary_boost && self.vocabulary.iter().any(|e| e.is_active() && e.kind == EntryKind::Vocabulary)
    }
}

/// A microphone picked in Settings. The name is kept to say which one is
/// missing when it isn't connected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedDevice {
    /// The WASAPI endpoint id.
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum OtherAudio {
    #[default]
    Nothing,
    Lower,
    Mute,
    Pause,
}

pub const DEFAULT_LOWER_TO_PERCENT: u8 = 20;

/// A cleanup style for one app, by executable name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStyle {
    pub app_exe: String,
    pub level: CleanupLevel,
}

/// The style for a dictation into `exe`: its app style, or the default.
pub fn level_for_app(default: CleanupLevel, styles: &[AppStyle], exe: &str) -> CleanupLevel {
    styles.iter().find(|s| !exe.is_empty() && s.app_exe.eq_ignore_ascii_case(exe)).map_or(default, |s| s.level)
}

impl Default for VoiceDictation {
    fn default() -> Self {
        Self {
            engine: EngineId::ParakeetS1,
            activation_mode: ActivationMode::PushToTalk,
            hotkey: Hotkey::default(),
            raw_hotkey: None,
            cleanup_level: CleanupLevel::Full,
            formal_register: false,
            layout_cues: true,
            language: "auto".into(),
            writes_numbers: true,
            adapts_to_app: true,
            learns_corrections: true,
            history_enabled: false,
            insights_enabled: true,
            vocabulary: Vec::new(),
            app_styles: Vec::new(),
            fits_to_cursor: true,
            spoken_enter: false,
            splits_terminal_pastes: true,
            live_preview: false,
            input_device: None,
            avoid_bluetooth_mic: true,
            other_audio: OtherAudio::Nothing,
            lower_to_percent: DEFAULT_LOWER_TO_PERCENT,
            vocabulary_boost: false,
        }
    }
}

/// The key held to draw during a Sketch: alone, with no other modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum DrawKey {
    #[default]
    Shift,
    Ctrl,
    Alt,
}

impl DrawKey {
    pub fn modifier(self) -> u32 {
        match self {
            Self::Shift => modifiers::SHIFT,
            Self::Ctrl => modifiers::CTRL,
            Self::Alt => modifiers::ALT,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Shift => "Shift",
            Self::Ctrl => "Ctrl",
            Self::Alt => "Alt",
        }
    }
}

/// Sketch: talk while marking up the screen, then paste the screenshots
/// and the transcript together.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SketchSettings {
    pub enabled: bool,
    /// Starts and finishes a Sketch.
    pub hotkey: Hotkey,
    pub draw_key: DrawKey,
}

impl SketchSettings {
    /// Ctrl+Alt+S.
    pub fn default_hotkey() -> Hotkey {
        Hotkey { vk: 0x53, modifiers: modifiers::CTRL | modifiers::ALT }
    }

    /// Why `hotkey` can't start a Sketch, if it can't.
    pub fn hotkey_problem(hotkey: &Hotkey, dictation: &Hotkey) -> Option<String> {
        if hotkey == dictation {
            return Some("Sketch needs a different shortcut from dictation.".into());
        }
        if hotkey.is_mouse() || hotkey.is_lone_modifier() {
            return Some("Sketch needs a key with Ctrl, Alt, Shift, or Win.".into());
        }
        hotkey.problem()
    }
}

impl Default for SketchSettings {
    fn default() -> Self {
        Self { enabled: true, hotkey: Self::default_hotkey(), draw_key: DrawKey::Shift }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub enabled: bool,
    pub voice_dictation: VoiceDictation,
    pub sketch: SketchSettings,
    pub launch_at_login: bool,
    /// UI state: whether the history card is expanded.
    pub history_expanded: bool,
    /// The first-run setup (shortcut, mode, engine) is finished.
    pub onboarded: bool,
}

/// The shortcuts Settings can set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutSlot {
    Dictate,
    Raw,
    Sketch,
}

impl Settings {
    /// Why `hotkey` can't be the shortcut for `slot`, given the others.
    pub fn shortcut_problem(&self, slot: ShortcutSlot, hotkey: &Hotkey) -> Option<String> {
        let vd = &self.voice_dictation;
        let others = [
            (ShortcutSlot::Dictate, Some(vd.hotkey), "Dictation"),
            (ShortcutSlot::Raw, vd.raw_hotkey, "Dictate without cleanup"),
            (ShortcutSlot::Sketch, self.sketch.enabled.then_some(self.sketch.hotkey), "Sketch"),
        ];
        for (other, key, name) in others {
            if other != slot && key.as_ref() == Some(hotkey) {
                return Some(format!("{name} already uses this shortcut. Choose another one."));
            }
        }
        if slot == ShortcutSlot::Sketch {
            return SketchSettings::hotkey_problem(hotkey, &vd.hotkey);
        }
        hotkey.problem()
    }

    /// The no-cleanup shortcut to listen for, if one is set and usable.
    pub fn usable_raw_hotkey(&self) -> Option<Hotkey> {
        self.voice_dictation.raw_hotkey.filter(|h| self.shortcut_problem(ShortcutSlot::Raw, h).is_none())
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            voice_dictation: VoiceDictation::default(),
            sketch: SketchSettings::default(),
            launch_at_login: true,
            history_expanded: false,
            onboarded: false,
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
        assert_eq!(json["onboarded"], false);
        let vd = &json["voiceDictation"];
        assert_eq!(vd["engine"], "parakeetS1");
        assert_eq!(vd["activationMode"], "pushToTalk");
        assert_eq!(vd["cleanupLevel"], "full");
        assert_eq!(vd["language"], "auto");
        assert_eq!(vd["historyEnabled"], false);
        assert_eq!(vd["hotkey"]["vk"], 0x4D);
    }

    #[test]
    fn app_styles_override_the_default() {
        let styles = vec![AppStyle { app_exe: "Discord.exe".into(), level: CleanupLevel::Light }];
        assert_eq!(level_for_app(CleanupLevel::Full, &styles, "discord.EXE"), CleanupLevel::Light);
        assert_eq!(level_for_app(CleanupLevel::Full, &styles, "OUTLOOK.EXE"), CleanupLevel::Full);
        assert_eq!(level_for_app(CleanupLevel::Full, &styles, ""), CleanupLevel::Full);
        let s: Settings = serde_json::from_str(r#"{"voiceDictation":{"appStyles":[{"appExe":"slack.exe","level":"verbatim"}]}}"#).unwrap();
        assert_eq!(s.voice_dictation.app_styles[0].level, CleanupLevel::Verbatim);
        assert!(s.voice_dictation.fits_to_cursor && !s.voice_dictation.live_preview && !s.voice_dictation.spoken_enter);
    }

    #[test]
    fn settings_without_vocabulary_boost_keep_it_off() {
        let s: Settings = serde_json::from_str(r#"{"voiceDictation":{"engine":"parakeetS1","livePreview":true}}"#).unwrap();
        assert!(!s.voice_dictation.vocabulary_boost);
        let on: Settings = serde_json::from_str(r#"{"voiceDictation":{"vocabularyBoost":true}}"#).unwrap();
        assert!(on.voice_dictation.vocabulary_boost);
    }

    #[test]
    fn hotwords_need_the_switch_and_an_active_vocabulary_term() {
        let mut vd = VoiceDictation { vocabulary_boost: true, ..Default::default() };
        assert!(!vd.wants_hotwords());
        vd.vocabulary.push(VocabEntry::new(EntryKind::Snippet, "sig", "Best, Jo"));
        let mut off = VocabEntry::new(EntryKind::Vocabulary, "graph ana", "Grafana");
        off.is_enabled = false;
        vd.vocabulary.push(off);
        assert!(!vd.wants_hotwords());
        let mut scoped = VocabEntry::new(EntryKind::Vocabulary, "zus", "Zustand");
        scoped.app_exe = Some("code.exe".into());
        vd.vocabulary.push(scoped);
        assert!(vd.wants_hotwords());
        vd.vocabulary_boost = false;
        assert!(!vd.wants_hotwords());
    }

    #[test]
    fn partial_files_fill_defaults() {
        let s: Settings = serde_json::from_str(r#"{"voiceDictation":{"engine":"whisperTiny"}}"#).unwrap();
        assert_eq!(s.voice_dictation.engine, EngineId::WhisperTiny);
        assert!(s.voice_dictation.writes_numbers);
        assert!(s.enabled);
    }

    #[test]
    fn sketch_defaults_and_shortcut_clash() {
        let json = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(json["sketch"]["enabled"], true);
        assert_eq!(json["sketch"]["drawKey"], "shift");
        let sketch = SketchSettings::default();
        assert_eq!(sketch.hotkey.display(), "Ctrl+Alt+S");
        assert!(SketchSettings::hotkey_problem(&sketch.hotkey, &Hotkey::default()).is_none());
        assert!(SketchSettings::hotkey_problem(&Hotkey::default(), &Hotkey::default()).is_some());
        // Settings saved before Sketch existed still load.
        let old: Settings = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert_eq!(old.sketch, SketchSettings::default());
    }

    #[test]
    fn audio_fields_default_for_old_files() {
        let old: Settings = serde_json::from_str(r#"{"voiceDictation":{"engine":"whisperTiny","livePreview":true}}"#).unwrap();
        let vd = &old.voice_dictation;
        assert_eq!(vd.input_device, None, "follows the system default");
        assert_eq!(vd.other_audio, OtherAudio::Nothing);
        assert!(vd.avoid_bluetooth_mic);
        assert_eq!(vd.lower_to_percent, 20);
        let json = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(json["voiceDictation"]["otherAudio"], "nothing");
        assert_eq!(json["voiceDictation"]["inputDevice"], serde_json::Value::Null);
        let picked: Settings = serde_json::from_str(
            r#"{"voiceDictation":{"inputDevice":{"id":"{0.0.1.00000000}.{abc}","name":"Mic"},"otherAudio":"lower","lowerToPercent":35}}"#,
        )
        .unwrap();
        assert_eq!(picked.voice_dictation.input_device.unwrap().id, "{0.0.1.00000000}.{abc}");
        assert_eq!(picked.voice_dictation.other_audio, OtherAudio::Lower);
        assert_eq!(picked.voice_dictation.lower_to_percent, 35);
    }

    #[test]
    fn hotkey_display_and_problems() {
        assert_eq!(Hotkey::default().display(), "Ctrl+Alt+M");
        assert_eq!(Hotkey { vk: 0x70, modifiers: modifiers::SHIFT | modifiers::WIN }.display(), "Shift+Win+F1");
        assert!(Hotkey::default().problem().is_none());
        assert!(Hotkey { vk: 0x4D, modifiers: 0 }.problem().is_some());
        assert!(Hotkey { vk: 0x48, modifiers: modifiers::WIN }.problem().is_some());
        assert!(Hotkey { vk: 0x11, modifiers: modifiers::ALT }.problem().is_some());
        assert!(Hotkey { vk: 0x09, modifiers: modifiers::ALT }.problem().is_some(), "Alt+Tab");
        assert!(Hotkey { vk: 0x73, modifiers: modifiers::ALT }.problem().is_some(), "Alt+F4");
        assert!(Hotkey { vk: 0x4C, modifiers: modifiers::WIN }.problem().is_some(), "Win+L");
        assert!(Hotkey { vk: 0x4C, modifiers: modifiers::CTRL | modifiers::WIN }.problem().is_none());
    }

    #[test]
    fn mouse_buttons_and_lone_right_modifiers() {
        let back = Hotkey { vk: VK_XBUTTON1, modifiers: 0 };
        assert!(back.problem().is_none() && back.is_mouse());
        assert_eq!(back.display(), "Mouse back");
        assert_eq!(Hotkey { vk: VK_XBUTTON2, modifiers: modifiers::CTRL }.display(), "Ctrl+Mouse forward");
        let rctrl = Hotkey { vk: VK_RCONTROL, modifiers: 0 };
        assert!(rctrl.problem().is_none() && rctrl.is_lone_modifier() && rctrl.note().is_none());
        assert_eq!(rctrl.display(), "Right Ctrl");
        let ralt = Hotkey { vk: VK_RMENU, modifiers: 0 };
        assert_eq!(ralt.display(), "Right Alt");
        assert!(ralt.problem().is_none() && ralt.note().unwrap().contains("AltGr"));
        assert!(Hotkey { vk: VK_RMENU, modifiers: modifiers::SHIFT }.problem().is_some(), "only alone");
        assert!(Hotkey { vk: 0xA2, modifiers: 0 }.problem().is_some(), "left Ctrl alone");
        assert!(Hotkey { vk: 0xA4, modifiers: 0 }.problem().is_some(), "left Alt alone");
        assert!(Hotkey { vk: 0x01, modifiers: 0 }.problem().is_some(), "left button");
        assert!(SketchSettings::hotkey_problem(&back, &Hotkey::default()).is_some());
        assert!(SketchSettings::hotkey_problem(&rctrl, &Hotkey::default()).is_some());
    }

    #[test]
    fn hold_or_lock_and_raw_shortcut_are_opt_in() {
        let old: Settings = serde_json::from_str(
            r#"{"voiceDictation":{"activationMode":"toggle","hotkey":{"vk":77,"modifiers":3},"cleanupLevel":"full"}}"#,
        )
        .unwrap();
        assert_eq!(old.voice_dictation.activation_mode, ActivationMode::Toggle);
        assert_eq!(old.voice_dictation.raw_hotkey, None);
        assert_eq!(old.usable_raw_hotkey(), None);
        let json = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(json["voiceDictation"]["activationMode"], "pushToTalk");
        assert_eq!(json["voiceDictation"]["rawHotkey"], serde_json::Value::Null);
        let new: Settings = serde_json::from_str(
            r#"{"voiceDictation":{"activationMode":"holdOrLock","rawHotkey":{"vk":163,"modifiers":0}}}"#,
        )
        .unwrap();
        assert_eq!(new.voice_dictation.activation_mode, ActivationMode::HoldOrLock);
        assert_eq!(new.usable_raw_hotkey(), Some(Hotkey { vk: VK_RCONTROL, modifiers: 0 }));
    }

    #[test]
    fn shortcuts_cannot_clash() {
        let mut s = Settings::default();
        let main = s.voice_dictation.hotkey;
        let sketch = s.sketch.hotkey;
        let ctrl_shift_space = Hotkey { vk: 0x20, modifiers: modifiers::CTRL | modifiers::SHIFT };
        assert!(s.shortcut_problem(ShortcutSlot::Raw, &main).unwrap().starts_with("Dictation already"));
        assert!(s.shortcut_problem(ShortcutSlot::Raw, &sketch).unwrap().starts_with("Sketch already"));
        assert!(s.shortcut_problem(ShortcutSlot::Raw, &ctrl_shift_space).is_none());
        s.voice_dictation.raw_hotkey = Some(ctrl_shift_space);
        assert_eq!(s.usable_raw_hotkey(), Some(ctrl_shift_space));
        assert!(s.shortcut_problem(ShortcutSlot::Dictate, &ctrl_shift_space).unwrap().starts_with("Dictate without cleanup"));
        assert!(s.shortcut_problem(ShortcutSlot::Sketch, &ctrl_shift_space).is_some());
        assert!(s.shortcut_problem(ShortcutSlot::Dictate, &main).is_none());
        s.sketch.enabled = false;
        assert!(s.shortcut_problem(ShortcutSlot::Raw, &sketch).is_none(), "Sketch off frees its shortcut");
        s.voice_dictation.raw_hotkey = Some(main);
        assert_eq!(s.usable_raw_hotkey(), None);
    }
}
