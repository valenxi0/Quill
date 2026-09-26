//! Microphone capture: shared-mode WASAPI on the default input, resampled to
//! 16 kHz mono f32 in memory (section 4.2, step 2). Audio never touches disk.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::SeqCst};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample};
use parking_lot::Mutex;
use quill_core::resample::{downmix, Resampler};

/// Recordings stop at 10 minutes.
pub const MAX_SAMPLES: usize = 16_000 * 60 * 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioError {
    NoDevice,
    Denied,
    Failed,
}

pub fn has_input_device() -> bool {
    cpal::default_host().default_input_device().is_some()
}

struct Shared {
    samples: Mutex<Vec<f32>>,
    resampler: Mutex<Option<Resampler>>,
    /// Latest input level, 0..1, as f32 bits.
    level: AtomicU32,
    full: AtomicBool,
}

/// One live recording. Dropping it stops the stream.
pub struct Recording {
    shared: Arc<Shared>,
    stop_tx: Option<Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn level_of(block: &[f32]) -> f32 {
    if block.is_empty() {
        return 0.0;
    }
    let rms = (block.iter().map(|s| s * s).sum::<f32>() / block.len() as f32).sqrt();
    // Map about -50..0 dBFS onto 0..1 for the waveform.
    let db = 20.0 * rms.max(1e-6).log10();
    ((db + 50.0) / 50.0).clamp(0.0, 1.0)
}

fn on_block(shared: &Shared, mono: Vec<f32>) {
    shared.level.store(level_of(&mono).to_bits(), SeqCst);
    let out = match shared.resampler.lock().as_mut() {
        Some(r) => r.process(&mono),
        None => return,
    };
    let mut samples = shared.samples.lock();
    let room = MAX_SAMPLES.saturating_sub(samples.len());
    samples.extend_from_slice(&out[..out.len().min(room)]);
    if samples.len() >= MAX_SAMPLES {
        shared.full.store(true, SeqCst);
    }
}

fn build<T: SizedSample + Send + 'static>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    shared: Arc<Shared>,
    to_f32: fn(T) -> f32,
) -> Result<cpal::Stream, cpal::Error> {
    let channels = config.channels as usize;
    device.build_input_stream(
        config,
        move |data: &[T], _| {
            let floats: Vec<f32> = data.iter().map(|s| to_f32(*s)).collect();
            on_block(&shared, downmix(&floats, channels));
        },
        |err| log::warn!("audio stream error: {err}"),
        Some(Duration::from_secs(3)),
    )
}

impl Recording {
    /// Open the default input and start capturing.
    pub fn start() -> Result<Recording, AudioError> {
        let shared = Arc::new(Shared {
            samples: Mutex::new(Vec::with_capacity(16_000 * 30)),
            resampler: Mutex::new(None),
            level: AtomicU32::new(0),
            full: AtomicBool::new(false),
        });
        let (stop_tx, stop_rx) = channel::<()>();
        let (ready_tx, ready_rx) = channel::<Result<(), AudioError>>();
        let thread_shared = shared.clone();
        // The stream lives on its own thread for its whole life.
        let thread = std::thread::Builder::new()
            .name("quill-audio".into())
            .spawn(move || {
                let host = cpal::default_host();
                let Some(device) = host.default_input_device() else {
                    let _ = ready_tx.send(Err(AudioError::NoDevice));
                    return;
                };
                let supported = match device.default_input_config() {
                    Ok(c) => c,
                    Err(_) => {
                        let _ = ready_tx.send(Err(AudioError::NoDevice));
                        return;
                    }
                };
                *thread_shared.resampler.lock() = Some(Resampler::new(supported.sample_rate()));
                let config = supported.config();
                let s = thread_shared.clone();
                let stream = match supported.sample_format() {
                    SampleFormat::F32 => build::<f32>(&device, config, s, |x| x),
                    SampleFormat::I16 => build::<i16>(&device, config, s, |x| x as f32 / 32768.0),
                    SampleFormat::U16 => build::<u16>(&device, config, s, |x| (x as f32 - 32768.0) / 32768.0),
                    SampleFormat::I32 => build::<i32>(&device, config, s, |x| x as f32 / 2_147_483_648.0),
                    _ => {
                        let _ = ready_tx.send(Err(AudioError::Failed));
                        return;
                    }
                };
                let stream = match stream {
                    Ok(s) => s,
                    Err(e) => {
                        let kind = if matches!(e.kind(), cpal::ErrorKind::PermissionDenied) {
                            AudioError::Denied
                        } else {
                            AudioError::Failed
                        };
                        log::warn!("could not open the microphone: {e}");
                        let _ = ready_tx.send(Err(kind));
                        return;
                    }
                };
                if stream.play().is_err() {
                    let _ = ready_tx.send(Err(AudioError::Failed));
                    return;
                }
                let _ = ready_tx.send(Ok(()));
                let _ = stop_rx.recv();
                drop(stream);
            })
            .map_err(|_| AudioError::Failed)?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Recording { shared, stop_tx: Some(stop_tx), thread: Some(thread) }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(AudioError::Failed),
        }
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.shared.level.load(SeqCst))
    }

    pub fn is_full(&self) -> bool {
        self.shared.full.load(SeqCst)
    }

    /// Look at everything captured so far (16 kHz) without copying it.
    pub fn with_samples<R>(&self, f: impl FnOnce(&[f32]) -> R) -> R {
        f(&self.shared.samples.lock())
    }

    /// Stop the stream and return the complete recording.
    pub fn stop(mut self) -> Vec<f32> {
        self.halt();
        let tail = self.shared.resampler.lock().take().map(|mut r| r.finish()).unwrap_or_default();
        let mut samples = std::mem::take(&mut *self.shared.samples.lock());
        let room = MAX_SAMPLES.saturating_sub(samples.len());
        samples.extend_from_slice(&tail[..tail.len().min(room)]);
        samples
    }

    fn halt(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.halt();
        // Discard audio: nothing outlives the recording.
        self.shared.samples.lock().clear();
    }
}
