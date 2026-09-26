# Quill for Windows: handoff

The state of the Windows build against `QUILL_WINDOWS_HANDOFF.md` (the spec), as of 2026-09-25.

## What's built

All five engines run on Windows, and the full dictation loop works: shortcut → capture → recognize → text pipeline → clipboard → paste → overlay → history, stats and correction learning. The Settings UI covers all three tabs.

| Spec milestone | State |
| --- | --- |
| 1. Skeleton (Tauri, tray, single instance, autostart, settings store, Settings shell) | Done |
| 2. Record and paste with Whisper Tiny (LL hook, cpal 16 kHz, pinned downloader, whisper.cpp Vulkan + CPU fallback, clipboard + SendInput, overlay) | Done |
| 3. Full text pipeline with tests, Setup tab | Done |
| 4. Whisper Base/Turbo, Parakeet (ONNX) + S1-mini (llama.cpp), language gate and picker, early segments, warm-up, idle unload | Done |
| 5. Vocabulary UI, booster, correction watcher, Your voice tab | Done, except the booster (see Deviations) |
| 6. Insights, history, launchers (`quill://`, CLI) | Done |
| 7. Cohere | Done: an official sherpa-onnx export exists |
| 8. Benchmark, licenses view, installer, README | Done. `npm run tauri build` produces `Quill_0.1.0_x64-setup.exe` (21 MB, per-user NSIS, unsigned) |

Code map:

- `crates/quill-core`: every platform-free rule in the spec, with 78 unit tests (`cargo test -p quill-core`):
  - the text pipeline: vocabulary matching and replacement, snippet protection, list import, cleanup guard, chunker, command formatter, number formatter over NeMo ITN, chat style, hallucination filter, and speech gate;
  - the activation state table, pause finder and splitter, correction diff and anchors, history expiry, and stats (streaks, fillers, median, time saved);
  - settings keys and defaults, the engine table and language resolution (with Cohere's fallback through preferred UI languages), and the pinned model manifest;
  - the 16 kHz resampler.
- `src-tauri`: the Windows app:
  - `platform/hotkey.rs`: `WH_KEYBOARD_LL` on its own thread;
  - `platform/uia.rs`: a UI Automation worker;
  - `platform/paste.rs`, `platform/target.rs`, `platform/system.rs`: clipboard, SendInput, target capture, microphone consent, P-core count, preferred languages;
  - `audio.rs`, `models.rs` (downloader), `engines/*`, `controller.rs` (state machine and pipeline driver), `overlay.rs`, `tray.rs`;
  - `bench.rs` and `bin/quill-bench.rs`.
- `src`: the Settings window (Insights · Your voice · Setup) and the overlay pill. In a plain browser, `npm run dev` previews both against a fake backend (`src/lib/devMock.ts`).

## Measured numbers

Machine: AMD Ryzen 7 7800X3D (8 cores, SMT off, so 8 inference threads), NVIDIA GeForce RTX 3070 Ti (driver 32.0.15.9636), 32 GB RAM, Windows 11 Home 26200. Release build.

Eval set: `scripts/make-eval-set.ps1` produces 12 clips from Windows speech synthesis, 2.9–30.7 s long (11 of them are 10 s or less). Latency is warm, measured from audio to text on the clipboard through the same pipeline a dictation uses.

| Engine | Accelerator | Load | p50 / p95 (clips ≤ 10 s) | 30.7 s clip | WER raw / final |
| --- | --- | --- | --- | --- | --- |
| Parakeet + S1-mini | ONNX Runtime CPU + llama.cpp Vulkan | 5.3 s | **0.36 / 0.48 s** | 5.1 s* | 9.0% / 10.5%** |
| Cohere Transcribe | ONNX Runtime CPU | 3.9 s | 0.69 / 1.31 s | 3.3 s | 6.6% / 6.6% |
| Whisper Turbo | Vulkan | 3.8 s | 0.27 / 0.28 s | 0.54 s | 6.8% / 6.8% |
| Whisper Base | Vulkan | 0.18 s | **0.037 / 0.046 s** | 0.21 s | 9.4% / 9.4% |
| Whisper Tiny | Vulkan | 0.13 s | **0.027 / 0.036 s** | 0.15 s | 8.9% / 8.9% |

- The spec's targets (p50 ≤ 0.7 s, p95 ≤ 1.5 s for Parakeet + S1 and Whisper Base/Tiny) are all met. Whisper Turbo (0.27 s vs 2.1 s on the M4) and Cohere (0.7 s vs 5–20 s) are much faster than on the Mac.
- S1-mini on its own cleans an 83-word transcript in 0.30 s on the GPU (`quill-bench --s1`).
- \* The bench sends the 30 s clip to Parakeet in one piece. In the app, dictations longer than 12 s are cut into early segments while you're still talking, so only the tail is left after release.
- \*\* The references keep the spoken "um"/"uh", which S1-mini removes on purpose, so final WER goes up slightly. Synthetic speech is cleaner than a real microphone; add your own `name.wav` + `name.txt` pairs to `%LOCALAPPDATA%\Quill\Eval` for realistic numbers.
- Peak working set is 1.8 GB after Parakeet + S1-mini (ONNX Runtime's CPU buffers are much larger than Core ML's). It reaches 3.6 GB after Cohere, which dominates, because the bench measures one process across all engines. A per-engine measurement is still to do (see below).
- Full JSON: `%TEMP%\quill\bench.json` (`quill-bench [engineId…]`).

## Deviations from the spec, and why

1. **Parakeet and Cohere run on ONNX Runtime on the CPU, not DirectML.** The `sherpa-onnx` crate only ships CPU-only ONNX Runtime builds. Parakeet still meets the latency target on this CPU. A DirectML or CUDA build is possible through `SHERPA_ONNX_LIB_DIR` pointing at custom-built libraries.
2. **Chosen exports:**
   - Parakeet: `csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8` @ `2bda32ec…`. sherpa-onnx loads it directly, and the same runtime also runs Cohere, so there is one ONNX stack instead of two.
   - Cohere: `csukuangfj2/sherpa-onnx-cohere-transcribe-14-lang-int8-2026-04-01` @ `156a470c…`, an official sherpa-onnx conversion. sherpa-onnx takes the language per stream.
   - SHA-256s for the non-LFS files (`tokens.txt`, `README.md`) were hashed from the pinned revisions.
3. **No Parakeet vocabulary booster.**
   - The CTC 110M int8 export has no files at its current revision, so the asset is shown as "Not available on Windows yet".
   - sherpa-onnx hotwords for NeMo transducers need `modified_beam_search` plus a `bpe.vocab` that the Parakeet package doesn't ship.
   - The deterministic vocabulary pass still applies every entry, and the Your voice footer says so rather than claiming Parakeet listens for the terms.
4. **Cohere's own ITN is on** (`use_itn`). Quill's number formatter still runs afterwards, but it only touches spans of number words.
5. **One copy of ggml.** whisper-rs and llama-cpp-2 each vendor ggml, which gave 533 duplicate symbols at link time. `vendor/whisper-rs-sys` is whisper-rs-sys 0.15.0, patched through `[patch.crates-io]` to build whisper.cpp against llama-cpp-sys-2's installed ggml package (`WHISPER_USE_SYSTEM_GGML`). Its bundled ggml sources were removed.
6. **sherpa-onnx is linked `shared`.** Its static archive uses the static CRT and won't link next to llama.cpp. The installer ships `onnxruntime.dll`, `onnxruntime_providers_shared.dll`, `sherpa-onnx-c-api.dll` and `sherpa-onnx-cxx-api.dll` beside `Quill.exe` through an NSIS hook generated by `scripts/stage-dlls.mjs`.
7. **App icons in "Where you dictate"** are coloured initials, not icons extracted from the exe.
8. **Turning history off clears it.** The spec doesn't say; this keeps the privacy promise simple.
9. **Resampling** uses a small windowed-sinc resampler in `quill-core` instead of `rubato`. rubato 5 changed its API, and 16 kHz downsampling is easy to test directly (passband level, length, and stopband tests).
10. **Hotkey chord and Alt/Win:** on chord-down, the hook taps an unassigned key (0xE8) so that releasing Alt or Win on its own doesn't open a menu bar or the Start menu. Ctrl+V is delayed up to 600 ms until the shortcut's modifiers are up, and any still held are released, so the paste is never Ctrl+Alt+V.

## Review fixes

An independent review of the runtime code found 11 issues, all now fixed:

- A mid-recording early segment moved the phase to Cleaning, so release was ignored. Phase changes are now atomic, and only valid transitions are allowed (Recording → Transcribing → Cleaning).
- A cancel during delivery could still paste and record. The generation is checked right before the keystroke and again before anything is recorded.
- The main key leaked through when a modifier came up first. It is now latched until its own key-up.
- A race between start and cancel, and duplicate starts, are now closed.
- Model updates evict the engines that use the asset, then swap folders by rename.
- Duplicate downloads are prevented, and a stalled download can be cancelled (it times out after 60 s).
- Idle unload now also runs after cancelled or failed sessions.
- The target is captured in parallel with opening the microphone, so the first word isn't clipped.
- Hotkey capture ends when Settings loses focus, and system chords (Alt+Tab, Alt+F4, Win+L…) are refused.
- The overlay uses the window layer's own click-through, and its no-activate styles are re-asserted on each show.
- S1's logits index is now correct for prompts longer than one batch.

## Build notes (things that bit)

- `.cargo/config.toml` sets the build environment:
  - `CMAKE_GENERATOR=Ninja`, because MSBuild's file tracker fails on the long nested CMake paths. Ninja must be on `PATH`.
  - `LIBCLANG_PATH`, `VULKAN_SDK`, and `GGML_NATIVE=OFF` (so no AVX-512 is baked in).
- A failed CMake configure caches an empty `CMAKE_MAKE_PROGRAM`. Run `cargo clean -p llama-cpp-sys-2 -p whisper-rs-sys`.
- **Release paths are too long** inside `E:\Coding Projects\Quill\target`: `rc.exe` fails in a nested try-compile for the Vulkan shader generator. Build release with a short target dir, for example `$env:CARGO_TARGET_DIR="E:\quill-target"`.
- **Smart App Control (on, in enforce mode, on this PC) blocks some freshly built unsigned binaries:**
  - Symbol-stripped ones are always blocked, so the release profile no longer sets `strip = true`.
  - Binaries with debug info have passed reliably (`CARGO_PROFILE_RELEASE_DEBUG=true`), but SAC still occasionally blocks a build script or proc-macro DLL (`os error 4551` in the Code Integrity log).
  - The durable fixes are a code-signing certificate, which distribution needs anyway, or building the installer in CI. Turning SAC off can't be undone without reinstalling Windows, so that's the owner's call.

## Setup wizard and updates (added after the spec)

- **First-run wizard** (`src/settings/Onboarding.tsx`): Welcome → shortcut and Push to talk / Toggle → engine choice. The chosen engine must be downloaded, or be downloading, before Continue unlocks. Then Done, with a "Try it now" section.
  - It shows until `onboarded` is set in `settings.json`, even when Quill is launched at login.
  - Nothing downloads before the user picks an engine.
- **In-app updates** (`src-tauri/src/updates.rs`, Tauri updater plugin):
  - Quill checks `https://github.com/valenxi0/Quill/releases/latest/download/latest.json` 20 s after launch, then daily. Release builds only.
  - A new version is offered once in the overlay, then in the tray menu and a Settings banner.
  - Installing downloads the new installer, verifies its signature, runs it quietly, and relaunches Quill. It never runs during a dictation.
- **Update signing key:** the key pair is in `%USERPROFILE%\.tauri\quill-updater.key(.pub)`. The public key is in `tauri.conf.json`; the private key belongs in the `TAURI_SIGNING_PRIVATE_KEY` GitHub secret.
- **Releases:** `.github/workflows/release.yml` runs on `v*` tags. `ci.yml` runs the core tests and the frontend build on every push. Neither has run on GitHub yet.
- **Version:** the root `Cargo.toml` is the single source; `tauri.conf.json` has no version of its own.
- **Setup guide for users:** `docs/SETUP-GUIDE.md`.

## What's left

- **Code signing:** there's no certificate yet. The NSIS installer builds and installs per user, but it's unsigned, so SmartScreen and Smart App Control will warn or block on other PCs.
- **Real-microphone evaluation:** the WER above is from synthetic speech.
- **Per-engine memory:** measure the working set of each engine in its own process, instead of the cumulative peak.
- **Parakeet booster:** either export a `bpe.vocab` for sherpa-onnx hotwords, or port the CTC keyword-spot + rescore approach once a CTC 110M ONNX export with files exists.
- **DirectML/CUDA build of ONNX Runtime** for Parakeet and Cohere, if CPU latency on slower machines needs it.
- **Real app icons** in Insights → Where you dictate.
- **Packaged Store apps** (UWP) hosted in `ApplicationFrameHost.exe` report the host's exe. Chat detection covers them through the AUMID, but history and stats name the host.
- **Manual QA:** the benchmark and unit tests cover the engines and text rules. The interactive paths (hotkey in elevated apps, the correction watcher across apps, overlay on multiple monitors, reduced motion) need a hands-on pass.
