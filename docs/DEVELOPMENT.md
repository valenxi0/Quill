# Development

## Layout

- `crates/quill-core`: platform-free logic: the text pipeline (vocabulary, snippets, cleanup guard, chunker, command formatter, number formatter), state machine, pause finder, correction diff, history, stats, settings, engine and model tables, and the 16 kHz resampler. Most unit tests live here.
- `src-tauri`: the Windows app: keyboard hook, audio capture, UI Automation, clipboard and paste, overlay, tray, downloads, engines, the dictation controller, and updates (`updates.rs`). `src/bin/quill-bench.rs` is the benchmark.
- `src`: the Settings window (setup wizard, Insights, Your voice, Setup) and the overlay pill (React, TypeScript, Tailwind). Running `npm run dev` in a plain browser previews both against a fake backend (`src/lib/devMock.ts`). The preview uses sample counts and extracts icons from running Windows apps through `scripts/preview-app-icons.ps1`; it does not download logos.
- `.github/workflows`: `ci.yml` runs tests on every push; `release.yml` builds and publishes a release when you push a version tag.
- `SETUP-GUIDE.md`: the install guide for users.
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

The voice activity detector tests need the bundled ONNX Runtime beside the test binary. Otherwise Windows loads the older `System32\onnxruntime.dll` and the test process crashes. Copy `target\debug\onnxruntime*.dll` and `target\debug\sherpa-onnx-*.dll` into `target\debug\deps` once; without them, those tests skip.

## Releasing and updates

Release builds check GitHub shortly after launch and once a day. The update feed must be publicly accessible; while the repository is private, install updates manually from GitHub Releases. When an update is available, Quill offers it:
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

1. Bump `version` in the root `Cargo.toml` (`[workspace.package]`) and refresh the workspace entries in `Cargo.lock`. The root workspace version controls the app version. Add a matching `## [version] - YYYY-MM-DD` entry to `docs/CHANGELOG.md`; the workflow uses it as the release notes.
2. Commit, then tag and push:
   ```powershell
   git commit -am "Quill 0.2.0"
   git tag v0.2.0
   git push origin main v0.2.0
   ```
3. The **Release** workflow (`.github/workflows/release.yml`) builds the installer on GitHub, signs the update, and publishes a GitHub Release. The release has `Quill_0.2.0_x64-setup.exe` plus updater artifacts and `latest.json`. The workflow also uploads the stable `Quill-Setup.exe` download.

Updates only reach copies that already have the updater (0.1.0 builds from after it was added). Anyone on an older build installs the next version by hand once.

### Code signing (Smart App Control)

Windows 11's Smart App Control blocks unsigned installers with no way to run them anyway. The update signature above only proves an update came from you; it doesn't satisfy Windows. Signing with a certificate Windows trusts is the only fix that works for new users. Asking them to turn Smart App Control off works, but it lowers their protection for every app.

Options, cheapest first:

- **SignPath Foundation** signs open-source projects for free, but only ones with visible community adoption. It declined Quill in October 2026 and welcomes a new application once the project has more stars, contributors, and outside mentions. Approval would need a code signing policy page and SignPath's `submit-signing-request` GitHub Action in place of the Azure step.
- **Azure Artifact Signing** (formerly Trusted Signing) costs about $10 a month. Individual developers must be in the USA or Canada; organizations can also be in the EU or UK. Identity checks take a few business days.
- **Microsoft Store.** Microsoft signs Store packages, which Smart App Control trusts. This needs an MSIX package instead of the NSIS installer.

Quill's release workflow is ready for Azure Artifact Signing. Add these repository secrets and the next tag is signed: `ARTIFACT_SIGNING_ENDPOINT`, `ARTIFACT_SIGNING_ACCOUNT`, `ARTIFACT_SIGNING_PROFILE`, `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`. The app registration needs the Artifact Signing Certificate Profile Signer role. `scripts/sign.ps1` signs `Quill.exe` and the installer through Tauri's `signCommand`, and `scripts/stage-dlls.mjs` signs the sherpa-onnx DLLs. Without the secrets, both skip signing. This path hasn't run against a real account yet, so check the first signed release with `signtool verify /pa`.

Smart App Control also weighs reputation. A newly signed publisher can still see warnings for its first releases. Keep signing every release with the same identity.

## Launchers

`quill://start`, `quill://stop`, `quill://toggle`, `quill://cancel`, `quill://paste-last`, or the same as flags: `Quill.exe --toggle`. `paste-last` pastes the last transcript again into the app it went to. They work with Stream Deck, AutoHotkey, PowerToys, and similar tools.

## Benchmark

`scripts/make-eval-set.ps1` writes 12 synthetic clips with transcripts to `%LOCALAPPDATA%\Quill\Eval`. Add `name.wav` and `name.txt` pairs to test your own recordings. `quill-bench [engineId…]` runs every installed engine through recognition, cleanup, and clipboard delivery. It writes results to `%TEMP%\quill\bench.json`. Keep recordings and generated reports outside the repository.

## Cleanup and model evaluation

Parakeet recognizes speech. S1-mini cleans English through llama.cpp. Other languages bypass cleanup. Preserve the S1-mini system prompt, chat template, greedy sampler, and output budget: `min(1024, floor(input_tokens * 1.3 + 32))`. Retain the cleanup guards and fixture rubrics.

Set `QUILL_CLEANUP_EVAL=1`, then run `cargo test -p quill --release --locked real_model_cleanup_eval -- --ignored --nocapture` with the release environment above. The test uses installed model weights and checks 23 fixtures. It writes outputs, timing, guard decisions, and rubric misses to `%LOCALAPPDATA%\Quill\Eval\cleanup-report.json`. Do not commit generated reports.

Development builds never register for login startup. Launch the installed release to reconcile the login setting and repair stale paths. Release login launches run quietly in the tray after onboarding.

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
| Silero VAD (built in) | MIT |
