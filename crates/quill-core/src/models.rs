//! Pinned model manifest and model states (section 2.1).

use serde::{Deserialize, Serialize};

use crate::engines::AssetId;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelFile {
    pub path: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: AssetId,
    pub title: &'static str,
    pub repo: &'static str,
    pub revision: &'static str,
    pub files: &'static [ModelFile],
    pub license: &'static str,
    /// Shown before the download starts.
    pub license_note: &'static str,
    pub runtime: &'static str,
    pub optional: bool,
    /// False when no Windows build of the model exists yet.
    pub available: bool,
}

impl Asset {
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    pub fn url(&self, file: &ModelFile) -> String {
        format!("https://huggingface.co/{}/resolve/{}/{}", self.repo, self.revision, file.path)
    }

    pub fn file(&self, path: &str) -> Option<&ModelFile> {
        self.files.iter().find(|f| f.path == path)
    }
}

const fn f(path: &'static str, size: u64, sha256: &'static str) -> ModelFile {
    ModelFile { path, size, sha256 }
}

const WHISPER_REPO: &str = "ggerganov/whisper.cpp";
const WHISPER_REV: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";
const WHISPER_NOTE: &str = "OpenAI Whisper weights, MIT license. Quill keeps the notice.";

pub const ASSETS: [Asset; 7] = [
    // Chosen over istupakov/parakeet-tdt-0.6b-v3-onnx because sherpa-onnx loads
    // it directly (encoder/decoder/joiner + tokens), with the same int8 encoder
    // size, and sherpa-onnx also runs Cohere Transcribe from the same runtime.
    Asset {
        id: AssetId::ParakeetTdt06bV3,
        title: "Parakeet TDT 0.6B v3",
        repo: "csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8",
        revision: "2bda32ec70b097a55adaa07d9a7173915b43cc78",
        files: &[
            f("encoder.int8.onnx", 652184281, "acfc2b4456377e15d04f0243af540b7fe7c992f8d898d751cf134c3a55fd2247"),
            f("decoder.int8.onnx", 11845275, "179e50c43d1a9de79c8a24149a2f9bac6eb5981823f2a2ed88d655b24248db4e"),
            f("joiner.int8.onnx", 6355277, "3164c13fc2821009440d20fcb5fdc78bff28b4db2f8d0f0b329101719c0948b3"),
            f("tokens.txt", 93939, "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d"),
        ],
        license: "CC BY 4.0",
        license_note: "Parakeet TDT 0.6B v3 by NVIDIA, licensed CC BY 4.0. Converted to ONNX (int8) by the sherpa-onnx project.",
        runtime: "ONNX Runtime",
        optional: false,
        available: true,
    },
    // No Windows build pinned yet: the CTC 110M int8 export has no files at
    // its current revision. Vocabulary is still applied deterministically.
    Asset {
        id: AssetId::ParakeetVocabularyBooster,
        title: "Vocabulary booster (Parakeet CTC 110M)",
        repo: "csukuangfj/sherpa-onnx-nemo-parakeet_tdt_ctc_110m-en-36000-int8",
        revision: "a42c4220ca392162c050f2d2c9c982d41a340782",
        files: &[],
        license: "CC BY 4.0",
        license_note: "Parakeet TDT-CTC 110M by NVIDIA, licensed CC BY 4.0.",
        runtime: "ONNX Runtime",
        optional: true,
        available: false,
    },
    Asset {
        id: AssetId::S1Mini,
        title: "S1-mini",
        repo: "superwhisper/s1-mini-GGUF",
        revision: "34add00a48a2e5d24e5a4ee5405a99620a3a240c",
        files: &[
            f("s1-mini-q4_k_m.gguf", 484219808, "3b41ebe2502cbd03e811d5d16b022f5ab551eda58d62597d152f89535003c634"),
            f("LICENSE", 12033, "f715982df6ce767ae64864d74b95644e5d658aab54717dbceeb252eb3cbcb421"),
            f("NOTICE", 507, "6eb1e98a0d0046dd1b71da19d3b64ebccc12622e7d62e5efcad10298b4d66292"),
        ],
        license: "Apache 2.0",
        license_note: "S1-mini by Superwhisper, derived from Qwen3-0.6B. Apache 2.0 with a naming term; the LICENSE and NOTICE are kept with the model.",
        runtime: "llama.cpp",
        optional: false,
        available: true,
    },
    Asset {
        id: AssetId::CohereTranscribe,
        title: "Cohere Transcribe 03-2026",
        repo: "csukuangfj2/sherpa-onnx-cohere-transcribe-14-lang-int8-2026-04-01",
        revision: "156a470cf08eefe706a0004f3c52d9ee567ca7a0",
        files: &[
            f("encoder.int8.onnx", 3090822, "cf704f8cfa90e3f0a76f9ffc05998bdf00ba9ae983192c14a85a3a5eb008b367"),
            f("encoder.int8.onnx.data", 2731503072, "bcf1b7148c8518ae52df1ad2d2fc2b4e89261ea23e6c874eef1d9f55bcbaa4a3"),
            f("decoder.int8.onnx", 153250705, "8372ca6c8ff4db8b916ca3592f5c757a715e691b9edec751ba19b29fc854baf9"),
            f("tokens.txt", 207437, "013ede043ae2480e3a9205cc34550d9686100cc682bacc90f702facdfbb93035"),
            f("README.md", 294, "c25c8573c9e60a28a4a220fefafe3dfc664677d9083338fdeeeb8723433d27ea"),
        ],
        license: "Apache 2.0",
        license_note: "Cohere Transcribe 03-2026 by Cohere Labs, licensed Apache 2.0. Converted to ONNX (int8) by the sherpa-onnx project; the README is kept with the weights.",
        runtime: "ONNX Runtime",
        optional: false,
        available: true,
    },
    Asset {
        id: AssetId::WhisperLargeV3Turbo,
        title: "Whisper large-v3-turbo",
        repo: WHISPER_REPO,
        revision: WHISPER_REV,
        files: &[f("ggml-large-v3-turbo-q5_0.bin", 574041195, "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2")],
        license: "MIT",
        license_note: WHISPER_NOTE,
        runtime: "whisper.cpp",
        optional: false,
        available: true,
    },
    Asset {
        id: AssetId::WhisperBaseEnglish,
        title: "Whisper base.en",
        repo: WHISPER_REPO,
        revision: WHISPER_REV,
        files: &[f("ggml-base.en.bin", 147964211, "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002")],
        license: "MIT",
        license_note: WHISPER_NOTE,
        runtime: "whisper.cpp",
        optional: false,
        available: true,
    },
    Asset {
        id: AssetId::WhisperTinyEnglish,
        title: "Whisper tiny.en",
        repo: WHISPER_REPO,
        revision: WHISPER_REV,
        files: &[f("ggml-tiny.en.bin", 77704715, "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f")],
        license: "MIT",
        license_note: WHISPER_NOTE,
        runtime: "whisper.cpp",
        optional: false,
        available: true,
    },
];

pub fn asset(id: AssetId) -> &'static Asset {
    ASSETS.iter().find(|a| a.id == id).expect("every asset is in the manifest")
}

pub const RECEIPT_FILE: &str = ".quill-model.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Receipt {
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ModelState {
    NotDownloaded,
    UpdateAvailable,
    Downloading { fraction: f64, bytes: u64, total: u64 },
    Verifying,
    Downloaded,
    Failed { message: String },
    /// No Windows build of this model exists yet.
    Unavailable,
}

impl ModelState {
    pub fn is_ready(&self) -> bool {
        matches!(self, ModelState::Downloaded)
    }

    pub fn is_busy(&self) -> bool {
        matches!(self, ModelState::Downloading { .. } | ModelState::Verifying)
    }
}

/// State of an installed model from its receipt alone.
pub fn installed_state(asset: &Asset, receipt: Option<&Receipt>) -> ModelState {
    if !asset.available {
        return ModelState::Unavailable;
    }
    match receipt {
        None => ModelState::NotDownloaded,
        Some(r) if r.revision == asset.revision => ModelState::Downloaded,
        Some(_) => ModelState::UpdateAvailable,
    }
}

pub fn checksum_mismatch(path: &str) -> String {
    format!("{path} did not match its pinned checksum. Retry, or report the mismatch.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_complete() {
        for id in AssetId::ALL {
            let a = asset(id);
            if a.available {
                assert!(!a.files.is_empty(), "{:?}", id);
                for file in a.files {
                    assert_eq!(file.sha256.len(), 64);
                    assert!(file.size > 0);
                }
            }
        }
        let s1 = asset(AssetId::S1Mini);
        assert_eq!(
            s1.url(&s1.files[0]),
            "https://huggingface.co/superwhisper/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/s1-mini-q4_k_m.gguf"
        );
    }

    #[test]
    fn receipt_states() {
        let a = asset(AssetId::WhisperTinyEnglish);
        assert_eq!(installed_state(a, None), ModelState::NotDownloaded);
        let ok = Receipt { revision: a.revision.into() };
        assert_eq!(installed_state(a, Some(&ok)), ModelState::Downloaded);
        let old = Receipt { revision: "old".into() };
        assert_eq!(installed_state(a, Some(&old)), ModelState::UpdateAvailable);
        let booster = asset(AssetId::ParakeetVocabularyBooster);
        assert_eq!(installed_state(booster, None), ModelState::Unavailable);
    }
}
