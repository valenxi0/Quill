# Quill launch film: director's brief

**Logline.** You want to dictate on Windows, but every good dictation app is Mac-only. Then Quill arrives: offline, fast, open source, built for Windows.

**Format.** 1920×1080, 60 fps, 31 s, H.264 + AAC for X. The score is synthesized on the same timeline at 120 BPM (one beat = 0.5 s), and every cut lands on a beat.

**Sound.** An A-major IV–V–iii–vi progression (Dmaj9, E6/9, C#m7, F#m9): FM electric piano, sidechained sub bass, a lead hook on the drop and the feature run, and a plucked arp. The music stops dead on "Until now.", drops on the logo, closes to a low-pass while Quill listens, and sweeps open into the paste. Typing uses real keystroke recordings (`sfx/keys`, CC0, Keyboard Soundpack #1 by unicaegames on OpenGameArt). To use a different typing recording, save it as `sfx/typing.wav`; `audio.mjs` detects and slices its keystrokes. The demo line is an ElevenLabs voice ("Hope"), supplied as a file: "Um, I think we should, uh, ship the Windows build on Friday." Its two pauses were shortened to about 60 ms, then it was high-passed, lightly compressed and given a small room. The word times in `timeline.js` were measured from the file with Whisper, and `sfx/voice-env.js` (its loudness envelope) drives the pill's waveform.

**Text effects.** Ported from 21st.dev components into pure functions of time: Text Animate by appica-dev (`rise` for "Until now." and feature headlines, `flip` for the verdict line, `highlight` for the taglines), Rolling Text by hyperiux (the end-card wordmark reels) and Text Shimmer by ibelick (the "Cleaning up" pill).

**Look.** Dark mode throughout, Inter Display headlines with one accent word in Quill blue. After the reference film, the ground changes every beat: each scene draws its own background, ported from 21st.dev (Aurora by pulkitxm for the typing, Noise Dark Blue Gradient with Squares by larsen66 for the search, SpotlightBackground by dhileepkumargm for "Until now.", Beams Background by kokonutd for the reveal, Shader Background wave lines by minhxthanh for the features, Mesh Gradient by adrielzimbril for the end card). A pill label and design-tool selection brackets on the reveal.

**Motion.** Uses the transitions.dev motion tokens scaled up for video: smooth ease-out `cubic-bezier(0.22, 1, 0.36, 1)` for moves, blurred rise for text, text-state swap and card resize for the recording pill, error shake for the Mac-only buttons, and a success check on paste. Closed-form springs keep `seek(t)` a pure function of time.

## Beat sheet

| Time | Scene | What happens |
| --- | --- | --- |
| 1.5–4.5 | Typing | Giant type, a typo and backspace: "Typing every word by hand is slow." |
| 4.5–8.0 | Search | "voice dictation app for windows" → four results. Each denied card lands one reason: "The good ones are Mac-first, cloud-based, or $15 a month." |
| 8.0–9.0 | Turn | "Until now." Hard stop in the music. |
| 9.0–12.5 | Reveal | Five bars bounce like a waveform, the blue tile springs in behind them, then the Quill wordmark and tagline. Music drops. |
| 12.5–19.5 | Demo | Windows desktop with Slack open in #launch (recreated from Slack's current desktop layout: aubergine frame, icon rail, channel sidebar, white message pane, formatting composer). Quill's red pill records and the spoken words appear with fillers. Then Transcribing → Cleaning up → Paste sent. Fillers dissolve, the clean sentence lands in the composer, the send button turns green, and the message posts. No keycaps on screen. |
| 19.5–25.5 | Features | Four writing styles · Learns your words · Made for Windows. |
| 25.5–31.0 | End card | Lockup, "Offline voice dictation for Windows.", Open source · Runs offline · Free, a "Free download for Windows" button, github.com/valenxi0/Quill · link in the post. |

## Claims we can make

Only what the README and changelog support: offline recognition and cleanup, audio discarded after transcription, Windows 10 and 11, installs without an admin prompt, open source, Parakeet + S1-mini default with Whisper and Cohere, four cleanup styles, correction suggestions. No latency numbers: the 0.42 s in the screenshots is sample data.

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
