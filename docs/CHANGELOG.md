# Changelog

## [Unreleased]

### Added

- Pick your microphone under Settings → Dictation → Audio. System default follows Windows as it changes. If the mic you picked isn't connected, Quill uses the default and says so.
- Quill skips a Bluetooth headset's mic when another mic exists, because recording from it drops the headset to call quality. It moves to another mic you set as a Windows default, or else to a microphone or headset rather than a line input or virtual cable. On by default; a mic you pick is always used.
- Other audio while dictating: lower other apps to a percent of their volume, mute them, or pause music and videos while you speak. Audio that starts mid-recording is lowered too. Everything goes back when the recording ends, is cancelled, or fails. An app closed while lowered gets its volume back the next time it opens, and after a crash Quill puts things back on its next launch. Off by default.
- A third activation mode, Hold or lock: hold the shortcut to talk, or tap it twice quickly to keep recording until you press it again. A single quick tap is ignored. Push to talk stays the default.
- "Dictate without cleanup": an optional second shortcut that pastes your words as spoken, with vocabulary, snippets, and numbers still applied. Set it under Settings → Dictation.
- The dictation shortcut can be a mouse side button, or Right Ctrl or Right Alt pressed alone. A side button used as the shortcut no longer makes the browser go back. Right Ctrl or Right Alt alone acts after about 0.2 seconds held on its own, or on a clean tap; pressed with another key, a click, or the wheel (Ctrl+C, Ctrl+click) it works as usual and Quill pastes nothing.
- Vocabulary has an experimental "Listen for these terms" switch, off by default. With it on, Parakeet listens for up to 26 of your vocabulary terms while it recognizes, so names and jargon come out spelled your way more often. It can miss words after long pauses.

### Changed

- If the graphics driver crashes Quill while an engine starts on the GPU, Quill uses the CPU from then on instead of crashing at every launch. Settings says so under Engine, with a button to try the GPU again. A new version of Quill tries the GPU again on its own.
- Dictating an instruction such as "Translate this into French:" pastes it as you said it. S1-mini used to delete the instruction. Quill also keeps your words if cleanup answers a question or writes much more than you said.
- Cleanup has a time limit. If S1-mini takes too long, Quill pastes what it heard instead of waiting.
- Dictating only numbers, such as "twenty five", skips S1-mini and goes straight to number formatting.
- Text Quill puts on the clipboard stays out of clipboard history (Win+V) and isn't synced to your other devices.
- Transcript history says why an entry kept your words, such as "Kept your words: cleanup took too long."

## [0.3.1] - 2026-11-09

### Changed

- Pausing mid-sentence no longer ends the sentence. Long dictations are still recognized in pieces while you talk, but S1-mini now cleans the whole dictation at once, so a one or two second pause stays inside the sentence.
- Casual keeps apostrophes, names, and "I", and drops fillers. Before, it wrote everything lowercase and turned "we're" into "were".
- When a cleanup guard objects, only the sentences it objects to stay as heard, not the whole piece. Saying "not" twice in a stutter no longer counts as a dropped negation.
- "Formal wording" is now called "Expand contractions", which is what it does.
- Sketch: drag the bar anywhere, and it stays there for the next Sketch. It fades back while the pointer is elsewhere.
- Sketch: ink follows the pointer more smoothly.
- Settings has new pages: Home, Dictation, Cleanup, Vocabulary, Sketch, Privacy, and General. Cleanup holds the style and all the per-app formatting in one place, transcript history moved to Home, and startup, updates, launchers, and licenses moved to General.
- The installer and uninstaller are dark, in the Settings window's colors. The Welcome page shows Quill typing a reply, and the Finish page shows the shortcut and where Quill sits in the tray. The setup file has Quill's icon, and setup no longer offers a desktop shortcut.

## [0.3.0] - 2026-10-27

### Added

- Sketch, from Nano: press Ctrl+Alt+S and talk while you mark up the screen. Hold Shift and move the pointer to draw, or turn on Draw and drag; clicks still reach your apps. Press Ctrl+Alt+S again and Quill pastes the screenshots, then a line for each with what you said at that moment, into the app you started in. Terminals get the image paths, which Claude Code turns into images. Screenshots are kept for a day. Set the shortcut and draw key on the new Sketch page in Settings. The idea comes from Scrawl by Kit Langton.

- Styles for specific apps: pick Casual in Discord and keep Polished everywhere else.
- Dictating into the middle of a sentence matches the text around the cursor. The first word starts lowercase, a missing space is added, and a full stop the sentence doesn't need is dropped. Names and vocabulary keep their capitals. Turn it off under Setup, Advanced.
- Say an emoji name followed by "emoji", such as "thumbs up emoji", to type the emoji. "The fire emoji" and "the party was fire" stay as words.
- Optional: end a dictation with "press enter" to press Enter after the paste.
- Long pastes into terminals go in pieces, so Claude Code and Codex show the text instead of a "pasted text" placeholder.
- Optional live preview: the words Quill hears appear inside the recording pill, newest on the right. The pasted text still comes from the whole recording.
- Transcript history keeps what Quill heard as well as what it pasted. Redo any entry in another style, and the result is copied. History moved to Your voice.
- Paste last transcript again, from the tray or `quill://paste-last`, into the app it went to.
- App pickers have a search field.

### Changed

- A built-in voice activity detector (Silero VAD) skips silence. A recording with no speech ends without running the recognizer, and the end of a recording is trimmed to the speech.
- Apps that aren't open, Microsoft Store apps such as Claude, and apps recorded before 0.2.2 now show their icons.
- Clearer Settings copy, sentence-case buttons, and no "All apps" tag on every global vocabulary entry.
- Settings has a new layout: a sidebar with Home, Your voice, Setup, Typing into apps, and Privacy and data, grouped settings with icons, and a Home page that leads with your week. The shortcut shows as keys.
- The overlay shows a breathing light while Quill transcribes and cleans up, and its pills rise in without bouncing. The recording pill no longer shows a timer, and its red outline breathes.
- The update banner no longer shows raw release-note markdown. It says which version you have, with a What's new link to the release notes.
- The README opens with the launch video.
- The release workflow can sign Quill for Smart App Control once a signing account is set up. Releases stay unsigned until then.

### Validation and known limits

- The real S1-mini evaluation now has 35 fixtures, adding self-corrections, negation, hedging, deliberate repetition, emoji, profanity, questions, embedded instructions, file names, and amounts. All pass.
- Fitting to the cursor depends on the app reporting its caret through UI Automation. Apps that don't are left as before.

## [0.2.2] - 2026-10-13

### Added

- Vocabulary and snippets can apply to all apps or one app. App entries override matching global entries, including recognition hints. Learned corrections keep their source app, and list imports support an app scope.
- Where you dictate shows native Windows app icons, with a neutral app glyph when an icon is unavailable. The browser preview extracts icons from locally installed apps too. The app scope picker uses a dark menu with app icons and keyboard navigation.

### Changed

- Quill uses dark mode regardless of the Windows theme, including its window title bar. README screenshots show the dark interface with sample data.

## [0.2.1] - 2026-10-10

### Added

- The installer includes CHANGELOG.md in the Quill installation folder. GitHub release notes and in-app update notes use this changelog.

### Changed

- Setup puts Start at login first and Updates directly after Cleanup. The engine picker collapses to the selected engine and its readiness; less-used controls live under Advanced, with Licenses at the bottom.
- Refreshed the README with sample app screenshots, a prominent installer link, and contribution guidance. Removed internal handoff notes and generated evaluation reports.
- Recording uses a red outline with no drop shadow or rotating square. The waveform and elapsed time remain visible.
- Transcription and cleanup show their status text without the rotating square.
- Cancel dictation with two separate Esc presses within one second. A held key or single press does not cancel.
- Shortcut capture tracks modifier presses directly and ignores held-key repeats. If the keyboard hook fails to start, Settings shows an error.

## [0.2.0] - 2026-10-07

### Added

- Four cleanup styles: Verbatim, Casual, Polished (default), and Compose. Compose turns spoken lists into numbered or bulleted lines.
- Formal wording for Polished and Compose, expanding contractions while leaving Casual unchanged.
- Email formatting in Outlook, Windows Mail, Thunderbird, and eM Client, with greetings and sign-offs on separate lines.
- Spoken line breaks: say “new line” or “new paragraph”. Literal phrases such as “add a new line” remain words; cues are disabled in terminals.
- Cleanup guards for invented figures and dropped sentences, alongside the existing empty-output and negation checks.
- An opt-in real S1-mini evaluation with 23 fixed fixtures and a JSON report of every output.

### Changed

- Existing `light` and `full` preferences now display as Casual and Polished; saved raw values remain compatible.
- Chat apps keep the selected cleanup style and only drop the final full stop from a one-sentence, one-line message.
- Settings and onboarding expose the new styles; snippet expansions stay protected from cleanup and capitalization.

### Fixed

- Login startup now uses the installed release executable with a quoted path. Release launches repair stale development-build startup entries, and debug builds cannot take over login registration.
- Duplicate login launches stay in the background. Normal launches still open Settings, and closing Settings keeps Quill in the tray.
- Spoken layout and piece joining preserve line breaks across segment boundaries without stacking extra blank lines.

### Validation and known limits

- 92 Windows workspace tests passed; all 23 real-model fixtures passed with no guard firing or relaxed rubrics. Frontend production build and installed-app launch checks passed.
- S1-mini's system prompt, chat template, greedy decoding, and token budget are unchanged.
- Lists and emails spanning an early segment cut are formatted per segment. One flagged sentence restores its whole chunk to raw.
- An actual reboot/sign-in and live microphone-to-paste dictation were not exercised.

## [0.1.0] - 2026-09-26

- Initial Windows release with local dictation, Parakeet + S1-mini, Whisper and Cohere engines, guided setup, configurable shortcuts, vocabulary and snippets, optional history and insights, and bundled runtime libraries.
