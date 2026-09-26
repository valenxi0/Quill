# Quill for Windows

Local push-to-talk dictation for Windows 10/11 (x64). Hold a shortcut (Ctrl+Alt+M by default), speak, release. The cleaned text goes on the clipboard and is pasted into the field you were in. Recognition and cleanup run on your PC; audio never touches disk, and nothing is sent anywhere except the model downloads you ask for.

## Install

**[Download Quill-Setup.exe](https://github.com/valenxi0/Quill/releases/latest/download/Quill-Setup.exe)**, the newest version, always, and run it. You need to be signed in to GitHub with access to this repo. Quill installs for your user only, so there's no admin prompt. [`docs/SETUP-GUIDE.md`](docs/SETUP-GUIDE.md) walks through it step by step.

This repo is private, so Quill can't check for updates by itself. To hear about new versions, click **Watch → Custom → Releases** on this repo. To update, download and run the new `Quill-Setup.exe` over your copy; settings and engines are kept. Settings → Setup → Updates also has a **Releases page** button.

When Quill first opens, a short setup walks through:
1. how Quill works;
2. your shortcut, and **Push to talk** or **Toggle**;
3. which speech engine to download (Parakeet + S1-mini is recommended). You must download one before you can continue.

Nothing downloads before you choose. A friendlier step-by-step guide for users is in [`docs/SETUP-GUIDE.md`](docs/SETUP-GUIDE.md).

![The recording pill](docs/screenshots/recording-pill.png)

## How it works

1. **You press the shortcut.** A low-level keyboard hook watches for your chord (Ctrl+Alt+M by default). In *push-to-talk* mode Quill records while you hold it; in *toggle* mode, press once to start and again to stop. Esc cancels at any point.
2. **It listens.** The microphone is captured and resampled to 16 kHz in memory. A small pill at the bottom of the screen shows that it's recording: a spinning square, bars that follow your voice (flat when you're silent), and the time elapsed. The pill never takes focus, so the paste lands where you were typing. On long dictations, Quill finds natural pauses and starts transcribing earlier segments while you're still talking, so there's little left to do when you let go.
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

Nothing downloads until you pick an engine in the setup or click Download in Settings.

Measured on a Ryzen 7 7800X3D with an RTX 3070 Ti, for clips of 10 seconds or less, from releasing the key to the text on the clipboard (median / 95th percentile): Parakeet + S1-mini 0.36 / 0.48 s, Whisper Tiny 0.03 / 0.04 s, Whisper Base 0.04 / 0.05 s, Whisper Turbo 0.27 / 0.28 s, Cohere 0.69 / 1.31 s. Details are in `HANDOFF.md`. Every file is checked against a pinned size and SHA-256 (`crates/quill-core/src/models.rs`), and models live in `%LOCALAPPDATA%\Quill\Models`.

## Layout

- `crates/quill-core`: platform-free logic: the text pipeline (vocabulary, snippets, cleanup guard, chunker, command formatter, number formatter), state machine, pause finder, correction diff, history, stats, settings, engine and model tables, and the 16 kHz resampler. All unit tests live here.
- `src-tauri`: the Windows app: keyboard hook, audio capture, UI Automation, clipboard and paste, overlay, tray, downloads, engines, the dictation controller, and updates (`updates.rs`). `src/bin/quill-bench.rs` is the benchmark.
- `src`: the Settings window (setup wizard, Insights, Your voice, Setup) and the overlay pill (React, TypeScript, Tailwind). Running `npm run dev` in a plain browser previews both against a fake backend (`src/lib/devMock.ts`).
- `.github/workflows`: `ci.yml` runs tests on every push; `release.yml` builds and publishes a release when you push a version tag.
- `docs/SETUP-GUIDE.md`: the install guide for users.
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
cargo run --release --features bench --bin quill-bench   # benchmark (see below)
```

Release builds need two extra settings on Windows:

- **Short target dir.** Inside a long checkout path, `rc.exe` fails in a nested CMake try-compile for the Vulkan shader generator. Set `$env:CARGO_TARGET_DIR = "E:\quill-target"`, or any short path.
- **Smart App Control.** If SAC is on, it can block freshly built unsigned build scripts (`os error 4551`). Symbol-stripped binaries are always blocked, so the profile doesn't strip. Keeping debug info passes more reliably: `$env:CARGO_PROFILE_RELEASE_DEBUG = "true"; $env:CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_DEBUG = "true"`.

Cargo features: `whisper`, `sherpa`, `llama`, `vulkan` (all on by default). CPU is always the fallback when the GPU backend can't start.

## Releasing and updates

Installed copies of Quill check GitHub for a new release shortly after launch and then once a day. When one exists, Quill offers it:
- once, in the overlay: "Quill 0.2.0 is available · Later · Update";
- and from then on in the tray menu and at the top of Settings.

**Update** downloads the new installer, checks its signature, installs it quietly, and relaunches Quill. Settings, dictionary, and models are kept. Debug builds never check.

### One-time setup

1. Add the update signing key to GitHub. The key pair is in `%USERPROFILE%\.tauri\`:
   - `quill-updater.key` is private. Never commit it, and back it up: if it's lost, installed copies can't be updated any more.
   - `quill-updater.key.pub` is already in `src-tauri/tauri.conf.json`.

   In the repo on GitHub, go to **Settings → Secrets and variables → Actions → New repository secret** and add `TAURI_SIGNING_PRIVATE_KEY` with the contents of `quill-updater.key`. The key has no password, so leave `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` unset.
2. Optional, but needed for PCs with Smart App Control: code signing (see below).

### Each release

1. Bump `version` in the root `Cargo.toml` (`[workspace.package]`). It's the only place the version lives.
2. Commit, then tag and push:
   ```powershell
   git commit -am "Quill 0.2.0"
   git tag v0.2.0
   git push origin main v0.2.0
   ```
3. The **Release** workflow (`.github/workflows/release.yml`) builds the installer on GitHub, signs the update, and publishes a GitHub Release. The release has `Quill_0.2.0_x64-setup.exe` plus `latest.json`, which is the file installed copies check. It takes about 20 minutes.

Updates only reach copies that already have the updater (0.1.0 builds from after it was added). Anyone on an older build installs the next version by hand once.

### Code signing (Smart App Control)

Windows 11's Smart App Control blocks unsigned installers outright. The update signature above only proves an update came from you; it doesn't satisfy Windows. For that, sign with a certificate Windows trusts. The cheapest route is Microsoft's Azure **Trusted Signing** (about $10/month, with an identity check).

Once you have an account, add a `signCommand` under `bundle.windows` in `src-tauri/tauri.conf.json` that calls the Trusted Signing tool, and add its credentials as repository secrets. The release workflow then signs `Quill.exe`, the installer, and the updater artifacts. The four bundled DLLs need signing too: sign them in `scripts/stage-dlls.mjs` before they're packed.

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
