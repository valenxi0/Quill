# Benchmarks

Recorded on 2026-09-25; these are historical measurements, not a performance guarantee.

Machine: AMD Ryzen 7 7800X3D (8 cores, SMT off, so 8 inference threads), NVIDIA GeForce RTX 3070 Ti (driver 32.0.15.9636), 32 GB RAM, Windows 11 Home 26200. Release build.

Eval set: `scripts/make-eval-set.ps1` produces 12 clips from Windows speech synthesis, 2.9–30.7 s long (11 of them are 10 s or less). Latency is warm, measured from audio to text on the clipboard through the same pipeline a dictation uses.

| Engine | Accelerator | Load | p50 / p95 (clips ≤ 10 s) | 30.7 s clip | WER raw / final |
| --- | --- | --- | --- | --- | --- |
| Parakeet + S1-mini | ONNX Runtime CPU + llama.cpp Vulkan | 5.3 s | **0.36 / 0.48 s** | 5.1 s* | 9.0% / 10.5%** |
| Cohere Transcribe | ONNX Runtime CPU | 3.9 s | 0.69 / 1.31 s | 3.3 s | 6.6% / 6.6% |
| Whisper Turbo | Vulkan | 3.8 s | 0.27 / 0.28 s | 0.54 s | 6.8% / 6.8% |
| Whisper Base | Vulkan | 0.18 s | **0.037 / 0.046 s** | 0.21 s | 9.4% / 9.4% |
| Whisper Tiny | Vulkan | 0.13 s | **0.027 / 0.036 s** | 0.15 s | 8.9% / 8.9% |

- S1-mini on its own cleans an 83-word transcript in 0.30 s on the GPU (`quill-bench --s1`).
- \* The bench sends the 30 s clip to Parakeet in one piece. In the app, dictations longer than 12 s are cut into early segments while you're still talking, so only the tail is left after release.
- \*\* The references keep the spoken "um"/"uh", which S1-mini removes on purpose, so final WER goes up slightly. Synthetic speech is cleaner than a real microphone; add your own `name.wav` + `name.txt` pairs to `%LOCALAPPDATA%\Quill\Eval` for realistic numbers.
- Peak working set is 1.8 GB after Parakeet + S1-mini (ONNX Runtime's CPU buffers are much larger than Core ML's). It reaches 3.6 GB after Cohere, which dominates, because the bench measures one process across all engines. A per-engine measurement is still to do.
- Full JSON: `%TEMP%\quill\bench.json` (`quill-bench [engineId…]`).


Run instructions are in [DEVELOPMENT.md](DEVELOPMENT.md#benchmark).
