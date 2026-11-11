# Development

## Layout

- `crates/quill-core`: platform-free logic: the text pipeline (vocabulary, snippets, cleanup guard, chunker, command formatter, number formatter), state machine, pause finder, correction diff, history, stats, settings, engine and model tables, the 16 kHz resampler, and Sketch's screenshot picking, text and rendering (`sketch/`). Most unit tests live here.
- `src-tauri`: the Windows app: keyboard hook, audio capture, UI Automation, clipboard and paste, overlay, tray, downloads, engines, the dictation controller, and updates (`updates.rs`). `src/bin/quill-bench.rs` is the benchmark.
- `src`: the Settings window (setup wizard, then Home, Dictation, Cleanup, Vocabulary, Sketch, Privacy, and General), the overlay pill, and the Sketch ink layer and bar (React, TypeScript, Tailwind). Running `npm run dev` in a plain browser previews both against a fake backend (`src/lib/devMock.ts`). The preview uses sample counts and extracts icons from running Windows apps through `scripts/preview-app-icons.ps1`; it does not download logos.
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

## Recovery files

Two files in `%LOCALAPPDATA%\Quill` let Quill recover from a crash on its next launch:

- `gpu-starting.json` exists while an engine starts on the GPU. If Quill finds one from the same version at launch, a GPU start never returned, so this run uses the CPU and Settings offers "Try the GPU again", which deletes the file and restarts Quill. A new version deletes it. Debug builds abort during the GPU start when `QUILL_SIMULATE_GPU_CRASH` is set, to test this.
- `audio-restore.json` lists other apps' volumes and mutes that Quill changed and hasn't put back yet. Quill restores each one when that app's audio session next appears, and drops entries older than 30 days.

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

### The installer's look

The dark installer lives in `src-tauri/windows`:

- `installer.nsi` is a copy of Tauri's NSIS template (from tauri-cli 2.11.5), with Quill's changes marked `Quill:`. When you update `@tauri-apps/cli`, compare it with the new template in `crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi` and merge Tauri's changes.
- `dark.nsh` places the Welcome and Finish pictures and calls the theme plugin on every page.
- `theme/quill_theme.c` is a small NSIS plugin that paints the buttons, check boxes, labels and fields. The built DLL is committed in `plugins/x86-unicode`; after changing the C file, run `theme/build.cmd`, which needs Visual Studio Build Tools.
- `art/art.html` is the source of the Welcome, Finish and header pictures. After editing it, run `python scripts/installer-art.py` (Chrome and Pillow) to render the `.bmp` files at 1x, 1.5x and 2x.

## Launchers

`quill://start`, `quill://stop`, `quill://toggle`, `quill://cancel`, `quill://paste-last`, or the same as flags: `Quill.exe --toggle`. `paste-last` pastes the last transcript again into the app it went to. They work with Stream Deck, AutoHotkey, PowerToys, and similar tools.

## Benchmark

`scripts/make-eval-set.ps1` writes 12 synthetic clips with transcripts to `%LOCALAPPDATA%\Quill\Eval`. Add `name.wav` and `name.txt` pairs to test your own recordings. `quill-bench [engineId…]` runs every installed engine through recognition, cleanup, and clipboard delivery. It writes results to `%TEMP%\quill\bench.json`. Keep recordings and generated reports outside the repository.

### Parakeet hotwords

With Vocabulary, "Listen for these terms" on (`vocabularyBoost`, off by default), Parakeet gets the dictation's vocabulary terms as sherpa-onnx hotwords. Terms for the target app come first (Whisper keeps entry order, since it drops the start of a long prompt), terms with characters the model cannot spell are dropped, and at most 26 are passed, the largest list measured. Hotwords need modified beam search, so while the switch is on and any vocabulary term is active, Parakeet is rebuilt for beam search on its next use and stays that way until the switch goes off or the idle unload. That holds for every app, including apps where no term applies: one decision for all apps avoids rebuilding the recognizer when you move between them. With the switch off it never leaves greedy search. If hotwords cannot start (no ASCII path to `bpe.vocab`, or beam search fails to load), Parakeet stays greedy and the Vocabulary page shows "Not available on this PC".

The switch stays off by default because sherpa-onnx 1.13.8's beam search for TDT models drops speech after pauses. sherpa-onnx PR #4084 measured this Parakeet export on 20 real-speech clips: 40.9% WER with beam search against 17.2% greedy, with 14 clips losing over 20% of their words after 0.8 to 2.5 s pauses. The synthetic clips below do not reach that case. #4084 was merged after 1.13.8 shipped, so re-measure when the crate moves past it.

Hotwords also need a sentencepiece vocabulary. The Parakeet download has only `tokens.txt`, so Quill writes `bpe.vocab` beside it, scoring each piece by minus its id. That split differs from the model's own BPE for some words (11 of the 26 test terms), and those terms get less help. sherpa-onnx issues #4088 and #3980 add a way to pass a term as the model's own pieces. sherpa-onnx opens `bpe.vocab` as a narrow string and exits the process when that fails, so Quill passes an ASCII path only: the full path, or its 8.3 short form. With neither, Parakeet stays on greedy search.

`scripts/make-hotword-set.ps1` writes 20 rare-term clips to `%LOCALAPPDATA%\Quill\EvalTerms` and four clips with 2.5 s pauses to `EvalPauses`. `quill-bench --hotwords <dir> [--runs N] [--vocab label=path]` builds recognizers directly and reports term hits, WER on the standard set, and latency per decoding setup, using the same vocabulary file and term list as a dictation. `--paired` alternates greedy and hotwords clip by clip, so other load on the machine hits both. Results from 2026-11-11 (sherpa-onnx 1.13.8, CPU, synthetic voices, all 26 terms active on every clip):

| Setup | Term hits | Standard WER | Term clip WER |
|---|---|---|---|
| Greedy (no terms) | 7/26 | 8.95% | 25.9% |
| Beam search, no terms | 7/26 | 9.71% | 24.2% |
| Hotwords, score 1.5 (the switch) | 15/26 | 9.71% | 18.4% |
| Hotwords, score 3.0 | 17/26 | 8.95% | 16.9% |
| Hotwords, score 1.5, model's own split | 20/26 | 9.71% | 11.5% |

The 0.76 point WER gap is one word: "4.30pm" against "4.30 pm". Score 3.0 capitalized words that start like a term ("Ship", "Three Things", "Wrote"), which WER does not see, so Quill uses 1.5. No speech was dropped after the synthetic pauses. Paired over 5 passes, hotwords added a median of 55 ms per clip (15%; per-clip medians 34 to 75 ms).

## Cleanup and model evaluation

Parakeet recognizes speech. S1-mini cleans English through llama.cpp. Other languages bypass cleanup. Preserve the S1-mini system prompt, chat template, greedy sampler, and output budget: `min(1024, floor(input_tokens * 1.3 + 32))`. Retain the cleanup guards and fixture rubrics.

Set `QUILL_CLEANUP_EVAL=1`, then run `cargo test -p quill --release --locked real_model_cleanup_eval -- --ignored --nocapture` with the release environment above. The test uses installed model weights and checks 65 fixtures. Rubrics grade what Quill would deliver: the transcript goes through the same pipeline as a dictation, guard retries on halves included. The guard decision is for one cleanup of the whole transcript. It writes outputs, timing, guard decisions, and rubric misses to `%LOCALAPPDATA%\Quill\Eval\cleanup-report.json`. Do not commit generated reports.

The cleanup guard keeps the words as heard when S1-mini drops a negation, writes a figure nobody said, drops a sentence, a clause, most of a short dictation, or an opening instruction, writes much more than was said, or answers a question. A clause taken back is allowed to go: a false start ending on a word no sentence ends on ("I think we should."), or one a retraction follows ("Friday. Sorry, I meant Monday."). "No", "sorry", "actually", and "wait" alone don't take anything back. Fillers, hesitations, and unit words ("dollars", "per hour") never count as lost. In the eval, S1-mini never answered a dictated question, but it follows dictated instructions such as "Translate this into French," by deleting them; the `obeyed-*` fixtures cover that.

Cleanup has two time limits, after which the words go out as heard. One S1-mini call stops after 5 s plus 10 ms per prompt token plus 80 ms per output-budget token. A whole dictation, timed from when S1-mini is loaded, starts no new cleanup call, retries included, after 20 s plus 100 ms per word. On a Ryzen 7 7800X3D, `quill-bench --s1` cleans its 83-word text in 0.27 to 0.31 s on an RTX 3070 Ti and in 2.0 to 2.1 s on the CPU alone (`GGML_VK_VISIBLE_DEVICES=` hides the GPU), so the limits are about 10 times the CPU time for that text. With other builds saturating the CPU, the same clean took 125 s and once 1,283 s.

Known gaps: the deadline doesn't bound loading S1-mini or a llama decode already in flight, so one slow step can run past it. An answer appended after a question ("What is the capital of France? Paris.") passes the guard; S1-mini never wrote one in the eval.

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
| Hex by Christopher Langton (Dot scope, ported in `src/overlay/dotScope` for the Dots pill) | MIT |
| Scrawl by Kit Langton (idea and interaction for Sketch; no code copied) | MIT |

## Contributing

Bug reports, documentation fixes, and focused pull requests are welcome. For a bug, include your Quill version, Windows version, selected engine, and steps to reproduce. Share only audio or transcripts you're comfortable making public. For a larger feature, open an issue first.

Before a pull request, run `npm run build` and `cargo test -p quill-core --locked`, and include screenshots for UI changes. Keep model licenses and attribution. Cleanup changes must keep the S1-mini prompt, chat template, greedy decoding, output budget, guards, and evaluation rubrics.
