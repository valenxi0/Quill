# Quill launch film: director's brief

**Logline.** You want to dictate on Windows, but every good dictation app is Mac-only. Then Quill arrives: offline, fast, open source, built for Windows.

**Format.** 1920×1080, 60 fps, 36 s, H.264 + AAC for X. Score is synthesized on the same timeline at 120 BPM (one beat = 0.5 s), and every cut lands on a beat.

**Look.** Quill's own UI language. Near-black `#09090b`, Quill blue `#0a84ff`, recording red `#ff453a`, success green `#32d74b`. Inter Display for headlines, Inter for UI, JetBrains Mono for code. Film grain and a soft blue bloom. No centered-text-on-gradient defaults, and nothing fades in by itself: every entrance is a blurred rise or a spring.

**Motion.** Uses the transitions.dev motion tokens scaled up for video: smooth ease-out `cubic-bezier(0.22, 1, 0.36, 1)` for moves, blurred rise for text, text-state swap and card resize for the recording pill, error shake for the Mac-only buttons, and a success check on paste. Closed-form springs keep `seek(t)` a pure function of time.

## Beat sheet

| Time | Scene | What happens |
| --- | --- | --- |
| 0.0–3.0 | Typing | Giant type, a typo and backspace: "Typing every word by hand is slow." |
| 3.0–8.0 | Search | "voice dictation app for windows" → four results, all macOS. Download buttons shake red: "Mac only". Headline: "Every good one is Mac-only." |
| 8.0–9.0 | Turn | "Until now." Hard stop in the music. |
| 9.0–12.5 | Reveal | Five bars bounce like a waveform, the blue tile springs in behind them, then the Quill wordmark and tagline. Music drops. |
| 12.5–19.5 | Demo | Windows desktop with Slack open in #launch (recreated from Slack's current desktop layout: aubergine frame, icon rail, channel sidebar, white message pane, formatting composer). Quill's red pill records and the spoken words appear with fillers. Then Transcribing → Cleaning up → Paste sent. Fillers dissolve, the clean sentence lands in the composer, the send button turns green, and the message posts. No keycaps on screen. |
| 19.5–30.5 | Features | Works in airplane mode (Windows quick settings flips to airplane mode, Wi-Fi and Bluetooth go off, Quill still dictates and pastes) · Fast · Four writing styles · Learns your words · Open source · Made for Windows. |
| 30.5–36.0 | End card | Lockup, "Offline voice dictation for Windows.", Open source · Runs offline · Free, github.com/valenxi0/Quill. |

## Claims we can make

Only what the README and changelog support: offline recognition and cleanup, audio discarded after transcription, Windows 10 and 11, installs without an admin prompt, MIT licensed, Parakeet + S1-mini default with Whisper and Cohere, four cleanup styles, correction suggestions. No latency numbers: the 0.42 s in the screenshots is sample data.

## Pipeline

- `timeline.js`: shared timing (scene windows, typing schedules, sound cues), loaded by the page and the audio script.
- `index.html`: the stage. `window.seek(t)` paints the exact frame for time `t`.
- `audio.mjs`: synthesizes the score and sound effects to `out/score.wav`.
- `render.mjs`: Playwright walks time in parallel workers, screenshots each frame, ffmpeg encodes and muxes.
- `stills.mjs`: renders key frames and a contact sheet for the critique loop.

## Rendering

```
npm i playwright            # or link a global install into node_modules
node audio.mjs              # out/score.wav
node stills.mjs 9.8 17.2    # key frames to out/stills, then python3 sheet.py out/stills
node render.mjs             # out/quill-launch.mp4 (1080p60, H.264 + AAC)
```

The film is a pure function of time, so any frame can be re-rendered on its own after an edit.
