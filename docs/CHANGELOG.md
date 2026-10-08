# Changelog

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
