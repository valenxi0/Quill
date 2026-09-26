# Quill for Windows

Local push-to-talk dictation for Windows 10/11 (x64). Hold a shortcut (Ctrl+Alt+M by default), speak, release. The cleaned text goes on the clipboard and is pasted into the field you were in. Recognition and cleanup run on your PC; audio never touches disk, and nothing is sent anywhere except the model downloads you ask for.

## Install

Paste this into **PowerShell** (the default Windows terminal). It downloads the latest installer from GitHub Releases and runs it. Quill installs for your user only, so no admin prompt.

```powershell
$a = (irm https://api.github.com/repos/valenxi0/Quill/releases/latest).assets | ? name -like '*-setup.exe' | select -First 1; iwr $a.browser_download_url -OutFile "$env:TEMP\$($a.name)"; & "$env:TEMP\$($a.name)"
```

Or download `Quill_x.y.z_x64-setup.exe` by hand from the [Releases page](https://github.com/valenxi0/Quill/releases/latest).

After installing, open Settings from the tray icon, go to **Setup**, and download an engine (Parakeet + S1-mini is the recommended default). Windows may ask for microphone access the first time.

## How it works

1. **You press the shortcut.** A low-level keyboard hook watches for your chord (Ctrl+Alt+M by default). In *push-to-talk* mode Quill records while you hold it; in *toggle* mode, press once to start and again to stop. Esc cancels at any point.
2. **It listens.** The microphone is captured and resampled to 16 kHz in memory. A small overlay pill shows that it's recording. On long dictations, Quill finds natural pauses and starts transcribing earlier segments while you're still talking, so there's little left to do when you let go.
3. **It transcribes.** The selected engine turns audio into text on your PC (Parakeet, Cohere, or Whisper; see below). A speech gate and hallucination filter drop silence and phantom phrases.
4. **It cleans up.** The text pipeline applies your vocabulary and snippets, formats numbers ("twenty five dollars" → "$25"), turns spoken symbols into characters when you dictate into a terminal ("dollar sign" → "$"), and, with the default engine, runs S1-mini to remove filler words and fix punctuation. A guard rejects any cleanup that strays too far from what you said.
5. **It pastes.** The result goes on the clipboard and Quill sends Ctrl+V to the field you were in when you started, waiting until your shortcut keys are released so it never becomes Ctrl+Alt+V.
6. **It learns.** If you fix the same word after a paste more than once, Quill notices and suggests adding it to your vocabulary. History (optional, and cleared when you turn it off) and stats such as words dictated, streaks, and time saved appear in the Insights tab.

Everything runs locally. Audio is never written to disk and nothing is uploaded; the only network traffic is the model downloads you start yourself.

## Engines

| Engine | Recognition | Cleanup | Runtime |
| --- | --- | --- | --- |
| Parakeet + S1-mini (default) | Parakeet TDT 0.6B v3 (int8 ONNX) | S1-mini by Superwhisper (Q4_K_M GGUF) | ONNX Runtime (CPU) + llama.cpp (Vulkan) |
| Cohere Transcribe | Cohere Transcribe 03-2026 (int8 ONNX) | none | ONNX Runtime (CPU) |
| Whisper Turbo | whisper large-v3-turbo Q5_0 | none | whisper.cpp (Vulkan) |
| Whisper Base | whisper base.en | none | whisper.cpp (Vulkan) |
| Whisper Tiny | whisper tiny.en | none | whisper.cpp (Vulkan) |

Nothing downloads until you click Download in Settings. Every file is checked against a pinned size and SHA-256 (`crates/quill-core/src/models.rs`), and models live in `%LOCALAPPDATA%\Quill\Models`.

## Layout

- `crates/quill-core`: platform-free logic: the text pipeline (vocabulary, snippets, cleanup guard, chunker, command formatter, number formatter), state machine, pause finder, correction diff, history, stats, settings, engine and model tables, and the 16 kHz resampler. All unit tests live here.
- `src-tauri`: the Windows app: keyboard hook, audio capture, UI Automation, clipboard and paste, overlay, tray, downloads, engines, and the dictation controller. `src/bin/quill-bench.rs` is the benchmark.
- `src`: the Settings window and the status overlay (React, TypeScript, Tailwind).
- `vendor/whisper-rs-sys`: whisper-rs-sys 0.15.0, patched to build whisper.cpp against llama.cpp's ggml, so the program holds one copy of ggml.

## Building

Prerequisites (all via `winget`):

```powershell
winget install Rustlang.Rustup Microsoft.VisualStudio.2022.BuildTools LLVM.LLVM KhronosGroup.VulkanSDK Ninja-build.Ninja
# Build Tools needs the "Desktop development with C++" workload.
npm install
```

`.cargo/config.toml` sets `CMAKE_GENERATOR=Ninja` (MSBuild fails on the long nested CMake paths), `LIBCLANG_PATH`, `VULKAN_SDK`, and `GGML_NATIVE=OFF` so binaries run on any x64 CPU. Open a new terminal after installing Ninja so it's on `PATH`. If a native build ever fails half-way, run `cargo clean -p llama-cpp-sys-2 -p whisper-rs-sys`: CMake caches a bad configure.

```powershell
npm run tauri dev          # run
cargo test --workspace     # unit tests
npm run tauri build        # NSIS installer, per-user (no admin)
cargo run --release --bin quill-bench   # benchmark (see below)
```

Cargo features: `whisper`, `sherpa`, `llama`, `vulkan` (all on by default). CPU is always the fallback when the GPU backend can't start.

## Launchers

`quill://start`, `quill://stop`, `quill://toggle`, `quill://cancel`, or the same as flags: `Quill.exe --toggle`. They work with Stream Deck, AutoHotkey, PowerToys, and similar tools.

## Benchmark

`scripts/make-eval-set.ps1` writes 12 synthetic clips (Windows speech synthesis) with transcripts to `%LOCALAPPDATA%\Quill\Eval`. Add your own `name.wav` + `name.txt` pairs for realistic numbers. `quill-bench [engineId…]` runs every installed engine through the same recognize → pipeline → clipboard path and writes `%TEMP%\quill\bench.json`. Measured numbers are in `HANDOFF.md`.

## Licenses

Quill ships the runtimes; you download the weights. Always written as "S1-mini" by "Superwhisper".

| Component | License |
| --- | --- |
| whisper.cpp, llama.cpp | MIT |
| ONNX Runtime, sherpa-onnx | MIT, Apache 2.0 |
| text-processing-rs | Apache 2.0 |
| Parakeet TDT 0.6B v3 (NVIDIA) | CC BY 4.0 |
| S1-mini by Superwhisper (from Qwen3-0.6B) | Apache 2.0 with a naming term |
| Whisper tiny.en, base.en, large-v3-turbo | MIT |
| Cohere Transcribe 03-2026 | Apache 2.0 |
