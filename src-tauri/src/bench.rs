//! Benchmark harness (section 7): every installed engine over the eval set,
//! through the same recognize → pipeline → clipboard path as a dictation.
//! Run with `cargo run --release --bin quill-bench`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use quill_core::context::AppContext;
use quill_core::engines::{EngineId, Recognizer, ENGINES};
use quill_core::pipeline::{cleanup_budget, finish, process_segment, CleanError, Cleaner, Recipe};
use quill_core::resample::{downmix, Resampler};
use quill_core::segments::{pad, split};
use quill_core::text::directive::Directive;
use quill_core::text::finish::{join, rms_dbfs};
use serde::Serialize;

use crate::engines::{EngineHost, TranscribeRequest, Transcript};
use crate::models::ModelManager;

/// Read a PCM16 or float32 WAV as 16 kHz mono.
pub fn read_wav(path: &Path) -> Result<Vec<f32>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let (mut fmt, mut channels, mut rate, mut bits) = (0u16, 1u16, 16_000u32, 16u16);
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let size = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().unwrap()) as usize;
        let body = &bytes[i + 8..(i + 8 + size).min(bytes.len())];
        if id == b"fmt " {
            fmt = u16::from_le_bytes([body[0], body[1]]);
            channels = u16::from_le_bytes([body[2], body[3]]);
            rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
            bits = u16::from_le_bytes([body[14], body[15]]);
            if fmt == 0xFFFE && body.len() >= 26 {
                fmt = u16::from_le_bytes([body[24], body[25]]);
            }
        } else if id == b"data" {
            let samples: Vec<f32> = match (fmt, bits) {
                (1, 16) => body.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect(),
                (3, 32) => body.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect(),
                _ => return Err(format!("unsupported WAV format {fmt}/{bits}-bit")),
            };
            let mono = downmix(&samples, channels as usize);
            let mut r = Resampler::new(rate);
            let mut out = r.process(&mono);
            out.extend(r.finish());
            return Ok(out);
        }
        i += 8 + size + (size & 1);
    }
    Err("no data chunk".into())
}

fn words(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_alphanumeric() || *c == '\'').collect::<String>().to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Word error rate by edit distance over normalized words.
pub fn wer(reference: &str, hypothesis: &str) -> f64 {
    let (r, h) = (words(reference), words(hypothesis));
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    for (i, rw) in r.iter().enumerate() {
        let mut cur = vec![i + 1; h.len() + 1];
        for (j, hw) in h.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(rw != hw)).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[h.len()] as f64 / r.len() as f64
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClipResult {
    clip: String,
    seconds: f64,
    reference: String,
    raw: String,
    text: String,
    raw_wer: f64,
    final_wer: f64,
    latency_s: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineResult {
    engine: EngineId,
    title: &'static str,
    accelerator: String,
    load_s: f64,
    clips: usize,
    raw_wer: f64,
    final_wer: f64,
    p50_s: f64,
    p95_s: f64,
    /// Over warm clips of 10 s or less.
    p50_short_s: f64,
    p95_short_s: f64,
    peak_working_set_mb: f64,
    results: Vec<ClipResult>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Machine {
    cpu: String,
    gpu: String,
    gpu_driver: String,
    ram_gb: f64,
    os: String,
    inference_threads: usize,
}

struct BenchCleaner<'a>(&'a EngineHost);

impl Cleaner for BenchCleaner<'_> {
    fn clean(&mut self, text: &str, directive: Directive, deadline: Instant) -> Result<String, CleanError> {
        self.0.with_cleaner(|c| c.clean(text, directive, deadline)).map_err(CleanError::Failed).and_then(|(r, _)| r)
    }
}

fn peak_working_set_mb() -> f64 {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut c = PROCESS_MEMORY_COUNTERS { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
    unsafe {
        let _ = GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb);
    }
    c.PeakWorkingSetSize as f64 / 1e6
}

fn powershell(cmd: &str) -> String {
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", cmd])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn machine(threads: usize) -> Machine {
    Machine {
        cpu: powershell("(Get-CimInstance Win32_Processor).Name"),
        gpu: powershell("(Get-CimInstance Win32_VideoController | Select-Object -First 1).Name"),
        gpu_driver: powershell("(Get-CimInstance Win32_VideoController | Select-Object -First 1).DriverVersion"),
        ram_gb: powershell("(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory").parse::<f64>().unwrap_or(0.0) / 1e9,
        os: powershell("(Get-CimInstance Win32_OperatingSystem).Caption + ' ' + (Get-CimInstance Win32_OperatingSystem).BuildNumber"),
        inference_threads: threads,
    }
}

fn transcribe(host: &EngineHost, engine: EngineId, samples: &[f32], request: &TranscribeRequest) -> Result<Transcript, String> {
    let info = engine.info();
    let min = if info.recognizer == Recognizer::Whisper { 1.1 } else { 1.0 };
    host.with_recognizer(engine, |e| {
        if info.recognizer == Recognizer::Cohere {
            let mut texts = Vec::new();
            for r in split(samples, 20.0) {
                texts.push(e.transcribe(&pad(&samples[r], min), request)?.text);
            }
            Ok(Transcript { text: join(&texts), language: request.language.clone() })
        } else {
            e.transcribe(&pad(samples, min), request)
        }
    })
    .map(|(r, _)| r)?
}

pub fn run(only: Option<Vec<String>>) -> Result<PathBuf, String> {
    let eval = crate::paths::eval_dir();
    let mut clips: Vec<(String, Vec<f32>, String)> = Vec::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&eval)
        .map_err(|e| format!("{}: {e}", eval.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav")))
        .collect();
    entries.sort();
    for wav in entries {
        let txt = wav.with_extension("txt");
        let Ok(reference) = std::fs::read_to_string(&txt) else { continue };
        let samples = read_wav(&wav)?;
        clips.push((wav.file_stem().unwrap().to_string_lossy().into_owned(), samples, reference.trim().to_string()));
    }
    if clips.is_empty() {
        return Err(format!("No .wav + .txt pairs in {}", eval.display()));
    }
    println!("{} clips from {}", clips.len(), eval.display());

    let models = ModelManager::new(crate::paths::models_dir(), || {});
    let threads = crate::platform::system::inference_threads();
    let itn = quill_core::text::numbers::default_itn();
    let mut results = Vec::new();
    for info in ENGINES.iter() {
        let key = serde_json::to_value(info.id).unwrap().as_str().unwrap().to_string();
        if only.as_ref().is_some_and(|o| !o.contains(&key)) {
            continue;
        }
        if !EngineHost::supports(info.id) || info.required.iter().any(|a| !models.is_ready(*a)) {
            println!("skip {} (not installed)", info.title);
            continue;
        }
        // A fresh host per engine, so load time and memory are its own.
        let host = EngineHost::new(Arc::clone(&models), threads);
        let load_start = Instant::now();
        host.warm(info.id)?;
        let load = load_start.elapsed();
        let request = TranscribeRequest {
            vocabulary_terms: vec![],
            language: quill_core::engines::recognition_language(info.id, "auto", &crate::platform::system::preferred_languages()),
        };
        let recipe = Recipe {
            directive: Some(Directive::default()),
            layout_cues: true,
            context: AppContext::General,
            writes_numbers: true,
            has_cleanup: info.has_cleanup,
            vocabulary: vec![],
        };
        let mut clip_results = Vec::new();
        for (name, samples, reference) in &clips {
            let start = Instant::now();
            let transcript = transcribe(&host, info.id, samples, &request)?;
            let mut cleaner = BenchCleaner(&host);
            let seg = process_segment(
                &transcript.text,
                transcript.language.as_deref(),
                &recipe,
                if info.has_cleanup { Some(&mut cleaner as &mut dyn Cleaner) } else { None },
                itn.as_deref(),
            );
            let text = finish(&seg.raw, &seg.text, recipe.context, rms_dbfs(samples)).unwrap_or_default();
            crate::platform::paste::set_clipboard_text(&text);
            let latency = start.elapsed();
            let seconds = samples.len() as f64 / 16_000.0;
            println!("  {:<26} {:>5.1}s  {:>6.3}s  {}", name, seconds, latency.as_secs_f64(), text);
            clip_results.push(ClipResult {
                clip: name.clone(),
                seconds,
                raw_wer: wer(reference, &seg.raw),
                final_wer: wer(reference, &text),
                reference: reference.clone(),
                raw: seg.raw,
                text,
                latency_s: latency.as_secs_f64(),
            });
        }
        let mut lat: Vec<f64> = clip_results.iter().map(|c| c.latency_s).collect();
        lat.sort_by(|a, b| a.total_cmp(b));
        let mut short: Vec<f64> = clip_results.iter().filter(|c| c.seconds <= 10.0).map(|c| c.latency_s).collect();
        short.sort_by(|a, b| a.total_cmp(b));
        let n = clip_results.len() as f64;
        let r = EngineResult {
            engine: info.id,
            title: info.title,
            accelerator: host.accelerator(info.id).unwrap_or_default(),
            load_s: load.as_secs_f64(),
            clips: clip_results.len(),
            raw_wer: clip_results.iter().map(|c| c.raw_wer).sum::<f64>() / n,
            final_wer: clip_results.iter().map(|c| c.final_wer).sum::<f64>() / n,
            p50_s: percentile(&lat, 0.5),
            p95_s: percentile(&lat, 0.95),
            p50_short_s: percentile(&short, 0.5),
            p95_short_s: percentile(&short, 0.95),
            peak_working_set_mb: peak_working_set_mb(),
            results: clip_results,
        };
        println!(
            "{}: load {:.2}s, p50 {:.3}s p95 {:.3}s (≤10 s clips: {:.3}/{:.3}), WER raw {:.1}% final {:.1}%, accel {}",
            r.title,
            r.load_s,
            r.p50_s,
            r.p95_s,
            r.p50_short_s,
            r.p95_short_s,
            r.raw_wer * 100.0,
            r.final_wer * 100.0,
            r.accelerator
        );
        host.unload_all();
        drop(host);
        std::thread::sleep(Duration::from_millis(500));
        results.push(r);
    }

    let out_dir = std::env::temp_dir().join("quill");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let path = out_dir.join("bench.json");
    let report = serde_json::json!({
        "machine": machine(threads),
        "date": chrono::Local::now().to_rfc3339(),
        "engines": results,
    });
    std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap()).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_error_rate() {
        assert_eq!(wer("the cat sat", "The cat sat."), 0.0);
        assert!((wer("the cat sat", "the bat sat") - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(wer("a b", ""), 1.0);
    }
}

/// Download assets by id (e.g. `whisperTinyEnglish`) with the app's manager.
pub fn download(ids: &[String]) -> Result<(), String> {
    let models = ModelManager::new(crate::paths::models_dir(), || {});
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    for id in ids {
        let asset: quill_core::engines::AssetId =
            serde_json::from_value(serde_json::Value::String(id.clone())).map_err(|_| format!("unknown asset {id}"))?;
        if models.is_ready(asset) {
            println!("{id}: already installed");
            continue;
        }
        let start = Instant::now();
        rt.block_on(models.clone().download(asset));
        println!("{id}: {:?} in {:.1}s", models.state(asset), start.elapsed().as_secs_f64());
    }
    Ok(())
}

/// Time S1-mini alone on a long transcript (diagnostics).
pub fn s1_speed() -> Result<(), String> {
    let models = ModelManager::new(crate::paths::models_dir(), || {});
    let host = EngineHost::new(models, crate::platform::system::inference_threads());
    let text = "so um I wanted to give everyone a quick update on the project uh the design review went well and the team agreed on the new navigation engineering has started on the settings screen and we expect a first build next week the main risk is the audio pipeline on older laptops so we are adding a lighter engine if you have feedback please send it before wednesday so we can include it in the next sprint thanks everyone for the hard work this month";
    let start = Instant::now();
    host.with_cleaner(|_| ())?;
    println!("load {:.2}s", start.elapsed().as_secs_f64());
    for _ in 0..3 {
        let t = Instant::now();
        let deadline = Instant::now() + cleanup_budget(text.split_whitespace().count());
        let (out, _) = host.with_cleaner(|c| c.clean(text, Directive::default(), deadline))?;
        let out = out.map_err(|e| e.to_string())?;
        println!("clean {:.3}s, {} words out", t.elapsed().as_secs_f64(), out.split_whitespace().count());
    }
    Ok(())
}

/// Hotword experiment for Parakeet: `quill-bench --hotwords <dir> [--runs N]
/// [--vocab label=path]… [--paired]`. `<dir>` holds rare-term clips (`.wav` + `.txt`)
/// and `terms.txt`; the standard eval set guards against regressions. Builds
/// sherpa-onnx recognizers directly so decoding settings can vary.
#[cfg(feature = "sherpa")]
pub fn hotwords(args: &[String]) -> Result<(), String> {
    use sherpa_onnx::OfflineRecognizer;

    let mut it = args.iter();
    let terms_dir = PathBuf::from(it.next().ok_or("usage: --hotwords <dir> [--runs N] [--vocab label=path] [--paired]")?);
    let mut runs = 3usize;
    let mut paired = false;
    let mut extra_vocabs: Vec<(String, PathBuf)> = Vec::new();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--runs" => runs = it.next().and_then(|n| n.parse().ok()).ok_or("--runs needs a number")?,
            "--paired" => paired = true,
            "--vocab" => {
                let v = it.next().ok_or("--vocab needs label=path")?;
                let (label, path) = v.split_once('=').ok_or("--vocab needs label=path")?;
                extra_vocabs.push((label.to_string(), PathBuf::from(path)));
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }

    let load = |dir: &Path| -> Result<Vec<(String, Vec<f32>, String)>, String> {
        let mut wavs: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav")))
            .collect();
        wavs.sort();
        let mut out = Vec::new();
        for wav in wavs {
            let Ok(reference) = std::fs::read_to_string(wav.with_extension("txt")) else { continue };
            let name = wav.file_stem().unwrap().to_string_lossy().into_owned();
            out.push((name, read_wav(&wav)?, reference.trim().trim_start_matches('\u{feff}').to_string()));
        }
        Ok(out)
    };
    let standard = load(&crate::paths::eval_dir())?;
    let term_clips = load(&terms_dir)?;
    let terms: Vec<String> = std::fs::read_to_string(terms_dir.join("terms.txt"))
        .map_err(|e| format!("terms.txt: {e}"))?
        .lines()
        .map(|l| l.trim().trim_start_matches('\u{feff}').to_string())
        .filter(|l| !l.is_empty())
        .collect();
    println!("{} standard clips, {} term clips, {} terms", standard.len(), term_clips.len(), terms.len());

    let models = ModelManager::new(crate::paths::models_dir(), || {});
    let dir = models.dir(quill_core::engines::AssetId::ParakeetTdt06bV3);
    let out_dir = std::env::temp_dir().join("quill");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let shipped = crate::engines::hotword_vocab(&dir);
    match &shipped {
        Some((path, _)) => println!("hotword vocabulary: {path}"),
        None => println!("hotwords unavailable: Parakeet would stay on greedy search"),
    }
    let pieces = shipped.as_ref().map(|(_, p)| p.clone()).unwrap_or_default();
    let hotword_list = quill_core::text::vocabulary::hotword_terms(
        &terms,
        |t| pieces.is_empty() || quill_core::engines::spellable(t, &pieces),
        quill_core::text::vocabulary::MAX_HOTWORDS,
    )
    .join("/");

    struct Config {
        label: String,
        beam: bool,
        vocab: Option<PathBuf>,
        score: f32,
    }
    let mut configs = vec![
        Config { label: "greedy".into(), beam: false, vocab: None, score: 0.0 },
        Config { label: "beam4".into(), beam: true, vocab: None, score: 0.0 },
    ];
    let mut vocabs: Vec<(String, PathBuf)> = shipped.iter().map(|(p, _)| ("shipped".to_string(), PathBuf::from(p))).collect();
    vocabs.extend(extra_vocabs);
    for (label, path) in &vocabs {
        for score in [crate::engines::HOTWORDS_SCORE, 3.0] {
            configs.push(Config { label: format!("beam4+hw[{label}] s={score}"), beam: true, vocab: Some(path.clone()), score });
        }
    }

    let threads = crate::platform::system::inference_threads();
    let contains = |hyp: &str, term: &str| {
        let (h, t) = (words(hyp), words(term));
        !t.is_empty() && h.windows(t.len()).any(|w| w == t.as_slice())
    };
    let all: Vec<&(String, Vec<f32>, String)> = standard.iter().chain(term_clips.iter()).collect();
    let n_std = standard.len();
    let decode_once = |rec: &OfflineRecognizer, cfg: &Config, samples: &[f32]| -> String {
        let stream = if cfg.vocab.is_some() { rec.create_stream_with_hotwords(&hotword_list) } else { rec.create_stream() };
        stream.accept_waveform(16_000, samples);
        rec.decode(&stream);
        stream.get_result().map(|r| r.text.trim().to_string()).unwrap_or_default()
    };

    let build = |cfg: &Config| -> Result<OfflineRecognizer, String> {
        let vocab = cfg.vocab.as_ref().map(|v| v.to_string_lossy().into_owned());
        let mut c = crate::engines::parakeet_config(&dir, threads, vocab)?;
        if cfg.vocab.is_some() {
            c.hotwords_score = cfg.score;
        } else if cfg.beam {
            c.decoding_method = Some("modified_beam_search".into());
        }
        let rec = OfflineRecognizer::create(&c).ok_or_else(|| format!("{}: recognizer failed to start", cfg.label))?;
        decode_once(&rec, cfg, &vec![0.0; 16_000]);
        Ok(rec)
    };
    if paired {
        let (a_cfg, b_cfg) = (&configs[0], configs.get(2).ok_or("--paired needs hotwords, which are unavailable")?);
        let (a, b) = (build(a_cfg)?, build(b_cfg)?);
        let mut deltas: Vec<f64> = Vec::new();
        let mut ratios: Vec<f64> = Vec::new();
        let mut pass_a = Vec::new();
        let mut pass_b = Vec::new();
        for run in 0..runs {
            let (mut ta, mut tb) = (0.0, 0.0);
            for (_, samples, _) in &all {
                let padded = pad(samples, 1.0);
                let time = |rec: &OfflineRecognizer, cfg: &Config| {
                    let start = Instant::now();
                    decode_once(rec, cfg, &padded);
                    start.elapsed().as_secs_f64()
                };
                let (sa, sb) = if run % 2 == 0 {
                    let sa = time(&a, a_cfg);
                    (sa, time(&b, b_cfg))
                } else {
                    let sb = time(&b, b_cfg);
                    (time(&a, a_cfg), sb)
                };
                ta += sa;
                tb += sb;
                deltas.push((sb - sa) * 1000.0);
                ratios.push(sb / sa);
            }
            println!("pass {run}: {} {ta:.3}s, {} {tb:.3}s", a_cfg.label, b_cfg.label);
            pass_a.push(ta);
            pass_b.push(tb);
        }
        let pct = |v: &mut Vec<f64>, p: f64| {
            v.sort_by(|x, y| x.total_cmp(y));
            percentile(v, p)
        };
        println!(
            "paired over {} clip decodes: delta ms p25 {:.1} / p50 {:.1} / p75 {:.1}; ratio p25 {:.3} / p50 {:.3} / p75 {:.3}",
            deltas.len(),
            pct(&mut deltas, 0.25),
            pct(&mut deltas, 0.5),
            pct(&mut deltas, 0.75),
            pct(&mut ratios, 0.25),
            pct(&mut ratios, 0.5),
            pct(&mut ratios, 0.75)
        );
        let doc = serde_json::json!({ "machine": machine(threads), "date": chrono::Local::now().to_rfc3339(), "a": a_cfg.label, "b": b_cfg.label,
            "passA": pass_a, "passB": pass_b, "deltaMs": deltas, "ratio": ratios });
        let path = out_dir.join("hotwords-paired.json");
        std::fs::write(&path, serde_json::to_vec_pretty(&doc).unwrap()).map_err(|e| e.to_string())?;
        println!("wrote {}", path.display());
        return Ok(());
    }

    let mut report = Vec::new();
    for cfg in &configs {
        let rec = build(cfg)?;
        let run_one = |samples: &[f32]| decode_once(&rec, cfg, samples);

        let mut texts: Vec<String> = Vec::new();
        let mut per_clip: Vec<Vec<f64>> = vec![Vec::new(); all.len()];
        let mut pass_totals = Vec::new();
        let mut unstable = 0;
        for run in 0..runs {
            let mut total = 0.0;
            for (i, (_, samples, _)) in all.iter().enumerate() {
                let start = Instant::now();
                let text = run_one(&pad(samples, 1.0));
                let s = start.elapsed().as_secs_f64();
                total += s;
                per_clip[i].push(s);
                if run == 0 {
                    texts.push(text);
                } else if texts[i] != text {
                    unstable += 1;
                }
            }
            pass_totals.push(total);
        }
        let median = |v: &mut Vec<f64>| {
            v.sort_by(|a, b| a.total_cmp(b));
            v[v.len() / 2]
        };
        let mut clip_medians: Vec<f64> = per_clip.iter_mut().map(median).collect();
        let clip_mean_ms = clip_medians.iter().sum::<f64>() / clip_medians.len() as f64 * 1000.0;
        let clip_p50_ms = median(&mut clip_medians) * 1000.0;
        let mut totals = pass_totals.clone();
        let total_med = median(&mut totals);
        let (tmin, tmax) = (totals[0], totals[totals.len() - 1]);

        let std_wer = standard.iter().zip(&texts).map(|((_, _, r), h)| wer(r, h)).sum::<f64>() / n_std as f64;
        let term_wer = term_clips.iter().zip(&texts[n_std..]).map(|((_, _, r), h)| wer(r, h)).sum::<f64>() / term_clips.len() as f64;
        let (mut hits, mut total_terms, mut false_hits) = (0, 0, 0);
        let mut hit_list = Vec::new();
        for ((name, _, r), h) in term_clips.iter().zip(&texts[n_std..]) {
            for t in &terms {
                if contains(r, t) {
                    total_terms += 1;
                    let hit = contains(h, t);
                    hits += usize::from(hit);
                    hit_list.push(serde_json::json!({"clip": name, "term": t, "hit": hit}));
                } else if contains(h, t) {
                    false_hits += 1;
                }
            }
        }
        for ((_, _, r), h) in standard.iter().zip(&texts[..n_std]) {
            false_hits += terms.iter().filter(|t| contains(h, t) && !contains(r, t)).count();
        }
        println!(
            "{:<28} terms {:>2}/{:<2} false+ {}  std WER {:>5.2}%  term WER {:>5.2}%  clip p50 {:>6.1} ms  mean {:>6.1} ms  pass {:.3}s [{:.3}-{:.3}]  unstable {}",
            cfg.label, hits, total_terms, false_hits, std_wer * 100.0, term_wer * 100.0, clip_p50_ms, clip_mean_ms, total_med, tmin, tmax, unstable
        );
        for ((name, _, _), h) in all.iter().zip(&texts) {
            println!("    {name:<16} {h}");
        }
        report.push(serde_json::json!({
            "config": cfg.label,
            "termHits": hits, "termTotal": total_terms, "falseHits": false_hits,
            "standardWer": std_wer, "termWer": term_wer,
            "clipP50Ms": clip_p50_ms, "clipMeanMs": clip_mean_ms,
            "passTotalsS": pass_totals, "unstableOutputs": unstable,
            "hits": hit_list,
            "texts": all.iter().zip(&texts).map(|((n, _, r), h)| serde_json::json!({"clip": n, "reference": r, "text": h})).collect::<Vec<_>>(),
        }));
        drop(rec);
    }
    let path = out_dir.join("hotwords.json");
    let doc = serde_json::json!({ "machine": machine(threads), "date": chrono::Local::now().to_rfc3339(), "runs": runs, "terms": terms, "configs": report });
    std::fs::write(&path, serde_json::to_vec_pretty(&doc).unwrap()).map_err(|e| e.to_string())?;
    println!("wrote {}", path.display());
    Ok(())
}
