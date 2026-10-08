# Quill for Windows

Local push-to-talk dictation for Windows 10/11 (x64). Hold **Ctrl+Alt+M**, speak, and release to paste your words into the app you're using. Recognition and cleanup run on your PC; recorded audio stays in memory and is never uploaded.

## Install

**[Download Quill-Setup.exe](https://github.com/valenxi0/Quill/releases/latest/download/Quill-Setup.exe)** and run it. Sign in to GitHub with access to this repository first. Installation is per-user and needs no admin prompt.

The first-run setup lets you choose your shortcut, push-to-talk or toggle mode, and a speech engine. Models download only when you choose them. **Parakeet + S1-mini** is the default; Cohere Transcribe and Whisper Turbo, Base, and Tiny are also available.

For a walkthrough and troubleshooting, see the [setup guide](docs/SETUP-GUIDE.md).

See the [changelog](docs/CHANGELOG.md) for what's new.

![The recording pill](docs/screenshots/recording-pill.png)

## Use

- Hold your shortcut to speak, then release to paste. In toggle mode, press once to start and again to finish. **Esc** cancels.
- Open Settings from the tray to change engines, add vocabulary and snippets, or view history and stats.
- To update, run the latest installer over your copy. Settings and downloaded engines are kept. Automatic updates cannot access this private repository; use **Watch → Custom → Releases** on GitHub for notifications.

## Development

The app uses Tauri and Rust, with React and TypeScript for Settings and the recording overlay.

- [Build, repository layout, releases, and component licenses](docs/DEVELOPMENT.md)
- [Recorded benchmarks](docs/BENCHMARKS.md)
- [MIT license](LICENSE)
