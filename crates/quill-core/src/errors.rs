//! Dictation errors with their user-facing title and detail (section 6).

use serde::Serialize;

pub const MIC_DENIED_DETAIL: &str =
    "Allow Quill in Windows Settings → Privacy & security → Microphone, then try again.";
pub const MIC_UNAVAILABLE_DETAIL: &str =
    "Connect or select an input device in Windows Settings → System → Sound.";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DictationError {
    #[error("Quill is off")]
    FeatureOff,
    #[error("Microphone access denied")]
    MicrophoneDenied,
    #[error("No microphone available")]
    MicrophoneUnavailable,
    #[error("Engine not downloaded")]
    ModelMissing { engine: String, downloads: usize },
    #[error("Engine failed to load")]
    ModelLoadFailed { message: String },
    #[error("Recognition failed")]
    RecognitionFailed { message: String },
    #[error("Cleanup failed")]
    CleanupFailed { message: String },
    #[error("Could not copy to the clipboard")]
    ClipboardFailed,
    #[error("Dictation cancelled")]
    Cancelled,
    #[error("Nothing to transcribe")]
    NoSpeech,
    #[error("Already transcribing")]
    Busy,
}

impl DictationError {
    pub fn title(&self) -> String {
        self.to_string()
    }

    pub fn detail(&self) -> String {
        match self {
            Self::FeatureOff => "Turn Quill on in Settings.".into(),
            Self::MicrophoneDenied => MIC_DENIED_DETAIL.into(),
            Self::MicrophoneUnavailable => MIC_UNAVAILABLE_DETAIL.into(),
            Self::ModelMissing { engine, downloads } => {
                let what = if *downloads == 1 { "a download".to_string() } else { format!("{downloads} downloads") };
                format!("{engine} needs {what} before it can run. Open Settings → Quill to download it.")
            }
            Self::ModelLoadFailed { message }
            | Self::RecognitionFailed { message }
            | Self::CleanupFailed { message } => message.clone(),
            Self::ClipboardFailed => "The transcript is kept in Quill; copy it from the tray menu.".into(),
            Self::Cancelled => "Nothing was transcribed or copied.".into(),
            Self::NoSpeech => {
                "The recording was empty or too short. Hold the shortcut while you speak.".into()
            }
            Self::Busy => "Wait for the current dictation to finish.".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings() {
        let e = DictationError::ModelMissing { engine: "Parakeet + S1-mini".into(), downloads: 2 };
        assert_eq!(e.title(), "Engine not downloaded");
        assert_eq!(
            e.detail(),
            "Parakeet + S1-mini needs 2 downloads before it can run. Open Settings → Quill to download it."
        );
        let one = DictationError::ModelMissing { engine: "Whisper Tiny".into(), downloads: 1 };
        assert!(one.detail().starts_with("Whisper Tiny needs a download before"));
        assert_eq!(DictationError::ClipboardFailed.title(), "Could not copy to the clipboard");
    }
}
