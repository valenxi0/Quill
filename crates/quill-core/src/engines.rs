//! Engines, their model assets, and language resolution (section 2).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineId {
    ParakeetS1,
    CohereTranscribe,
    WhisperTurbo,
    WhisperBase,
    WhisperTiny,
}

impl Default for EngineId {
    fn default() -> Self {
        Self::ParakeetS1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AssetId {
    ParakeetTdt06bV3,
    ParakeetVocabularyBooster,
    S1Mini,
    CohereTranscribe,
    WhisperLargeV3Turbo,
    WhisperBaseEnglish,
    WhisperTinyEnglish,
}

impl AssetId {
    pub const ALL: [AssetId; 7] = [
        AssetId::ParakeetTdt06bV3,
        AssetId::ParakeetVocabularyBooster,
        AssetId::S1Mini,
        AssetId::CohereTranscribe,
        AssetId::WhisperLargeV3Turbo,
        AssetId::WhisperBaseEnglish,
        AssetId::WhisperTinyEnglish,
    ];

    /// Folder name under `%LOCALAPPDATA%\Quill\Models`.
    pub fn key(self) -> &'static str {
        match self {
            AssetId::ParakeetTdt06bV3 => "parakeetTDT06BV3",
            AssetId::ParakeetVocabularyBooster => "parakeetVocabularyBooster",
            AssetId::S1Mini => "s1Mini",
            AssetId::CohereTranscribe => "cohereTranscribe",
            AssetId::WhisperLargeV3Turbo => "whisperLargeV3Turbo",
            AssetId::WhisperBaseEnglish => "whisperBaseEnglish",
            AssetId::WhisperTinyEnglish => "whisperTinyEnglish",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Recognizer {
    Parakeet,
    Whisper,
    Cohere,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub id: EngineId,
    pub title: &'static str,
    pub badge: &'static str,
    pub badge_accent: bool,
    pub summary: &'static str,
    pub language_note: &'static str,
    pub recognizer: Recognizer,
    /// Assets needed to run.
    pub required: &'static [AssetId],
    /// Assets that only improve results.
    pub optional: &'static [AssetId],
    pub has_cleanup: bool,
    pub shows_language_picker: bool,
    pub english_only: bool,
}

/// The order they appear in the UI.
pub const ENGINES: [EngineInfo; 5] = [
    EngineInfo {
        id: EngineId::ParakeetS1,
        title: "Parakeet + S1-mini",
        badge: "Best quality · Heaviest",
        badge_accent: true,
        summary: "Best English results. Parakeet recognizes speech; S1-mini by Superwhisper removes fillers and false starts and writes numbers, dates, and emails properly.",
        language_note: "Parakeet hears 25 European languages on its own. S1-mini cleanup runs on English; other languages are written as recognized.",
        recognizer: Recognizer::Parakeet,
        required: &[AssetId::ParakeetTdt06bV3, AssetId::S1Mini],
        optional: &[AssetId::ParakeetVocabularyBooster],
        has_cleanup: true,
        shows_language_picker: false,
        english_only: false,
    },
    EngineInfo {
        id: EngineId::CohereTranscribe,
        title: "Cohere Transcribe",
        badge: "Experimental · Slow",
        badge_accent: false,
        summary: "Experimental. Strong on German, French, Spanish, Italian, Portuguese, Dutch, Polish, Greek, Arabic, Japanese, Chinese, Korean and Vietnamese, but takes several seconds per dictation on this PC. Pick your language below; Whisper Turbo is the faster multilingual choice.",
        language_note: "Cohere cannot detect the language: speech in another one comes out translated. Automatic follows your PC's language.",
        recognizer: Recognizer::Cohere,
        required: &[AssetId::CohereTranscribe],
        optional: &[],
        has_cleanup: false,
        shows_language_picker: true,
        english_only: false,
    },
    EngineInfo {
        id: EngineId::WhisperTurbo,
        title: "Whisper Turbo",
        badge: "Detects language · Large",
        badge_accent: false,
        summary: "Hears which language you speak and writes it, across 99 languages. The multilingual pick: about two seconds per dictation, and a good fit if you switch languages.",
        language_note: "Automatic detects the language of each dictation.",
        recognizer: Recognizer::Whisper,
        required: &[AssetId::WhisperLargeV3Turbo],
        optional: &[],
        has_cleanup: false,
        shows_language_picker: true,
        english_only: false,
    },
    EngineInfo {
        id: EngineId::WhisperBase,
        title: "Whisper Base",
        badge: "Balanced · Lighter",
        badge_accent: false,
        summary: "Good accuracy for clear English speech with less memory and processing than the best-quality option.",
        language_note: "English-only model (base.en / tiny.en). Use Whisper Turbo or Cohere Transcribe for other languages.",
        recognizer: Recognizer::Whisper,
        required: &[AssetId::WhisperBaseEnglish],
        optional: &[],
        has_cleanup: false,
        shows_language_picker: false,
        english_only: true,
    },
    EngineInfo {
        id: EngineId::WhisperTiny,
        title: "Whisper Tiny",
        badge: "Fastest · Lightest",
        badge_accent: false,
        summary: "Fastest and easiest on your PC, but trades recognition accuracy for its small footprint.",
        language_note: "English-only model (base.en / tiny.en). Use Whisper Turbo or Cohere Transcribe for other languages.",
        recognizer: Recognizer::Whisper,
        required: &[AssetId::WhisperTinyEnglish],
        optional: &[],
        has_cleanup: false,
        shows_language_picker: false,
        english_only: true,
    },
];

impl EngineId {
    pub fn info(self) -> &'static EngineInfo {
        ENGINES.iter().find(|e| e.id == self).expect("every engine has info")
    }
}

/// Languages offered by the picker, in order.
pub const LANGUAGES: [(&str, &str); 14] = [
    ("en", "English"),
    ("de", "German"),
    ("fr", "French"),
    ("es", "Spanish"),
    ("it", "Italian"),
    ("pt", "Portuguese"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("el", "Greek"),
    ("ar", "Arabic"),
    ("ja", "Japanese"),
    ("zh", "Chinese"),
    ("ko", "Korean"),
    ("vi", "Vietnamese"),
];

pub fn language_name(code: &str) -> Option<&'static str> {
    LANGUAGES.iter().find(|(c, _)| *c == code).map(|(_, n)| *n)
}

/// Cohere's "Automatic": the first preferred UI language Cohere knows, else
/// English. `preferred` holds BCP-47 tags like `de-DE`.
pub fn cohere_auto_language(preferred: &[String]) -> &'static str {
    preferred
        .iter()
        .filter_map(|tag| {
            let primary = tag.split(['-', '_']).next()?.to_ascii_lowercase();
            LANGUAGES.iter().find(|(c, _)| *c == primary).map(|(c, _)| *c)
        })
        .next()
        .unwrap_or("en")
}

/// The language passed to the recognizer. `setting` is `"auto"` or an ISO
/// 639-1 code.
pub fn recognition_language(engine: EngineId, setting: &str, preferred: &[String]) -> Option<String> {
    let pinned = (setting != "auto" && language_name(setting).is_some()).then(|| setting.to_string());
    match engine {
        EngineId::ParakeetS1 => None,
        EngineId::WhisperBase | EngineId::WhisperTiny => Some("en".into()),
        EngineId::WhisperTurbo => pinned,
        EngineId::CohereTranscribe => Some(pinned.unwrap_or_else(|| cohere_auto_language(preferred).into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn language_resolution() {
        let none = prefs(&[]);
        assert_eq!(recognition_language(EngineId::ParakeetS1, "de", &none), None);
        assert_eq!(recognition_language(EngineId::WhisperBase, "de", &none).as_deref(), Some("en"));
        assert_eq!(recognition_language(EngineId::WhisperTiny, "auto", &none).as_deref(), Some("en"));
        assert_eq!(recognition_language(EngineId::WhisperTurbo, "auto", &none), None);
        assert_eq!(recognition_language(EngineId::WhisperTurbo, "fr", &none).as_deref(), Some("fr"));
        assert_eq!(recognition_language(EngineId::CohereTranscribe, "ja", &none).as_deref(), Some("ja"));
    }

    #[test]
    fn cohere_falls_back_through_preferred_languages() {
        let p = prefs(&["sv-SE", "de-DE", "en-US"]);
        assert_eq!(recognition_language(EngineId::CohereTranscribe, "auto", &p).as_deref(), Some("de"));
        assert_eq!(cohere_auto_language(&prefs(&["sv-SE", "fi-FI"])), "en");
        assert_eq!(cohere_auto_language(&prefs(&["zh-Hans-CN"])), "zh");
    }

    #[test]
    fn engine_table() {
        assert_eq!(ENGINES[0].id, EngineId::default());
        assert!(ENGINES.iter().filter(|e| e.badge_accent).count() == 1);
        assert!(ENGINES.iter().all(|e| !e.summary.contains("Mac")));
    }
}
