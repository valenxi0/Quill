//! Streaming windowed-sinc resampler to 16 kHz mono (section 4.2, step 2).

use std::f64::consts::PI;

pub const TARGET_RATE: u32 = 16_000;
/// Zero crossings of the sinc kernel on each side.
const ZERO_CROSSINGS: f64 = 16.0;

pub struct Resampler {
    /// Input samples per output sample.
    step: f64,
    /// Low-pass cutoff as a fraction of the input Nyquist rate.
    cutoff: f64,
    half_width: f64,
    buf: Vec<f32>,
    /// Position of the next output sample, in input samples from `buf[0]`.
    pos: f64,
}

impl Resampler {
    pub fn new(in_rate: u32) -> Self {
        let step = in_rate as f64 / TARGET_RATE as f64;
        // Downsampling: cut just below the output Nyquist rate.
        let cutoff = if step > 1.0 { 0.95 / step } else { 1.0 };
        let half_width = ZERO_CROSSINGS / cutoff;
        // Start with silent history so the first output is centred on sample 0.
        let pad = half_width.ceil() as usize;
        Self { step, cutoff, half_width, buf: vec![0.0; pad], pos: pad as f64 }
    }

    fn kernel(&self, x: f64) -> f64 {
        if x.abs() >= self.half_width {
            return 0.0;
        }
        let s = if x == 0.0 { 1.0 } else { (PI * self.cutoff * x).sin() / (PI * self.cutoff * x) };
        // Blackman window.
        let w = 0.42 + 0.5 * (PI * x / self.half_width).cos() + 0.08 * (2.0 * PI * x / self.half_width).cos();
        self.cutoff * s * w
    }

    fn emit(&mut self, out: &mut Vec<f32>, available: usize) {
        let half = self.half_width;
        while self.pos + half < available as f64 {
            let lo = (self.pos - half).ceil().max(0.0) as usize;
            let hi = ((self.pos + half).floor() as usize).min(available - 1);
            let mut acc = 0.0f64;
            for k in lo..=hi {
                acc += self.buf[k] as f64 * self.kernel(self.pos - k as f64);
            }
            out.push(acc as f32);
            self.pos += self.step;
        }
        // Drop history no later output can reach.
        let keep_from = ((self.pos - half).floor().max(0.0) as usize).min(self.buf.len());
        if keep_from > 0 {
            self.buf.drain(..keep_from);
            self.pos -= keep_from as f64;
        }
    }

    /// Feed input; returns the 16 kHz samples that are now complete.
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        self.buf.extend_from_slice(input);
        let mut out = Vec::with_capacity((input.len() as f64 / self.step) as usize + 1);
        let n = self.buf.len();
        self.emit(&mut out, n);
        out
    }

    /// Flush the tail with silence, so every input sample is represented.
    pub fn finish(&mut self) -> Vec<f32> {
        let real_end = self.buf.len() as f64;
        let pad = self.half_width.ceil() as usize + 1;
        self.buf.extend(std::iter::repeat_n(0.0, pad));
        let mut out = Vec::new();
        let n = self.buf.len();
        let half = self.half_width;
        while self.pos < real_end && self.pos + half < n as f64 {
            let lo = (self.pos - half).ceil().max(0.0) as usize;
            let hi = ((self.pos + half).floor() as usize).min(n - 1);
            let mut acc = 0.0f64;
            for k in lo..=hi {
                acc += self.buf[k] as f64 * self.kernel(self.pos - k as f64);
            }
            out.push(acc as f32);
            self.pos += self.step;
        }
        out
    }
}

/// Average interleaved channels into mono.
pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved.chunks_exact(channels).map(|f| f.iter().sum::<f32>() / channels as f32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, freq: f64, seconds: f64) -> Vec<f32> {
        let n = (rate as f64 * seconds) as usize;
        (0..n).map(|i| (2.0 * PI * freq * i as f64 / rate as f64).sin() as f32 * 0.5).collect()
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
    }

    #[test]
    fn length_and_passband() {
        for rate in [48_000, 44_100, 16_000, 8_000] {
            let input = sine(rate, 440.0, 1.0);
            let mut r = Resampler::new(rate);
            let mut out = Vec::new();
            for chunk in input.chunks(480) {
                out.extend(r.process(chunk));
            }
            out.extend(r.finish());
            assert!((out.len() as i64 - 16_000).abs() <= 2, "{rate}: {}", out.len());
            // A 440 Hz tone keeps its level (0.5 / sqrt 2).
            let mid = &out[2_000..14_000];
            assert!((rms(mid) - 0.3536).abs() < 0.01, "{rate}: {}", rms(mid));
        }
    }

    #[test]
    fn stopband_is_attenuated() {
        // 12 kHz can't exist at 16 kHz; it must not alias back in.
        let input = sine(48_000, 12_000.0, 1.0);
        let mut r = Resampler::new(48_000);
        let mut out = r.process(&input);
        out.extend(r.finish());
        assert!(rms(&out[2_000..14_000]) < 0.005);
    }

    #[test]
    fn downmixes() {
        assert_eq!(downmix(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
    }
}
