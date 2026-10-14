//! Silero VAD through sherpa-onnx. Fed during recording, so stopping only
//! has a few hundred milliseconds left to check. The model (MIT, 644 KB) is
//! built into the exe and written next to the downloaded models.

use quill_core::speech::SpeechMap;

#[cfg(feature = "sherpa")]
mod imp {
    use super::SpeechMap;
    use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

    static MODEL: &[u8] = include_bytes!("../resources/silero_vad.onnx");

    fn model_path() -> Option<String> {
        let path = crate::paths::models_dir().join("silero_vad.onnx");
        let current = std::fs::metadata(&path).is_ok_and(|m| m.len() == MODEL.len() as u64);
        if !current {
            std::fs::create_dir_all(path.parent()?).ok()?;
            let partial = path.with_extension("onnx.partial");
            std::fs::write(&partial, MODEL).ok()?;
            std::fs::rename(&partial, &path).ok()?;
        }
        Some(path.to_string_lossy().into_owned())
    }

    pub struct Vad {
        detector: VoiceActivityDetector,
        fed: usize,
        pub map: SpeechMap,
    }

    impl Vad {
        pub fn new() -> Option<Self> {
            let config = VadModelConfig {
                silero_vad: SileroVadModelConfig {
                    model: Some(model_path()?),
                    threshold: 0.4,
                    min_silence_duration: 0.3,
                    min_speech_duration: 0.12,
                    window_size: 512,
                    max_speech_duration: 20.0,
                },
                sample_rate: 16_000,
                num_threads: 1,
                provider: Some("cpu".into()),
                ..Default::default()
            };
            let detector = VoiceActivityDetector::create(&config, 30.0)?;
            Some(Self { detector, fed: 0, map: SpeechMap::default() })
        }

        fn drain(&mut self) {
            while let Some(segment) = self.detector.front() {
                let start = segment.start().max(0) as usize;
                self.map.push(start..start + segment.n().max(0) as usize);
                self.detector.pop();
            }
            // Speech in progress began after the last finished stretch.
            self.map.open_from = self
                .detector
                .detected()
                .then(|| self.map.intervals.last().map_or(0, |i| i.end));
        }

        /// Samples fed so far; the next call starts from here.
        pub fn fed(&self) -> usize {
            self.fed
        }

        /// Feed the samples that follow the ones already fed.
        pub fn accept(&mut self, new: &[f32]) {
            if !new.is_empty() {
                self.detector.accept_waveform(new);
                self.fed += new.len();
                self.drain();
            }
        }

        /// Feed the recording's remaining samples and close any open stretch.
        pub fn finish(&mut self, recording: &[f32]) -> &SpeechMap {
            let rest = &recording[self.fed.min(recording.len())..];
            self.accept(rest);
            self.detector.flush();
            self.drain();
            self.map.open_from = None;
            &self.map
        }
    }
}

#[cfg(not(feature = "sherpa"))]
mod imp {
    use super::SpeechMap;

    pub struct Vad {
        pub map: SpeechMap,
    }

    impl Vad {
        pub fn new() -> Option<Self> {
            None
        }
        pub fn fed(&self) -> usize {
            0
        }
        pub fn accept(&mut self, _new: &[f32]) {}
        pub fn finish(&mut self, _recording: &[f32]) -> &SpeechMap {
            &self.map
        }
    }
}

pub use imp::Vad;

#[cfg(all(test, feature = "sherpa"))]
mod tests {
    use super::*;

    /// Windows loads `System32\onnxruntime.dll`, an older version, unless the
    /// bundled one sits next to the test binary (`target\debug\deps`).
    fn runtime_beside_tests() -> bool {
        let here = std::env::current_exe().unwrap().with_file_name("onnxruntime.dll");
        if !here.is_file() {
            eprintln!(r"SKIPPED: copy target\debug\onnxruntime*.dll and sherpa-onnx-*.dll into target\debug\deps");
        }
        here.is_file()
    }

    #[test]
    fn silence_has_no_speech() {
        if !runtime_beside_tests() {
            return;
        }
        let mut vad = Vad::new().expect("VAD loads from the embedded model");
        let silence = vec![0.0f32; 16_000 * 3];
        assert!(!vad.finish(&silence).has_speech(&(0..silence.len())));
    }

    /// Uses a benchmark clip when `scripts/make-eval-set.ps1` has made one.
    #[test]
    fn finds_speech_in_an_eval_clip() {
        if !runtime_beside_tests() {
            return;
        }
        let Some(clip) = std::fs::read_dir(crate::paths::eval_dir()).ok().and_then(|d| {
            d.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "wav"))
        }) else {
            eprintln!("SKIPPED: no eval clips");
            return;
        };
        let samples = crate::bench::read_wav(&clip).expect("readable clip");
        let mut silence_first = vec![0.0f32; 16_000 * 2];
        silence_first.extend(&samples);
        let mut vad = Vad::new().unwrap();
        let map = vad.finish(&silence_first).clone();
        let crop = map.crop(&(0..silence_first.len())).expect("speech in the clip");
        assert!(crop.start >= 16_000, "leading silence trimmed: {crop:?}");
    }
}
