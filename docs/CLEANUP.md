# Cleanup contract and evaluation

Updated October 7, 2026. This documents Quill's Windows cleanup pipeline, settings, and model evaluation.

## Pipeline and model contract

Parakeet recognizes speech; S1-mini by Superwhisper cleans English through llama.cpp. Other languages bypass cleanup. The system prompt, chat template (including the empty thinking block), greedy sampler, and output budget remain unchanged: `min(1024, floor(input_tokens * 1.3 + 32))`.

The cleaner accepts a typed three-field `Directive`, rendered exactly as:

```text
[Styling: <styling>] [Structure: <structure>] [Context: <context>]
```

Trained values are styling `casual | semi-casual | semi-formal | formal`, structure `prose | lists`, context `general | email`. No setting selects semi-casual. No other values are sent.

| Stored cleanup level | Picker title | Base directive |
| --- | --- | --- |
| `verbatim` | Verbatim | No cleaner |
| `light` | Casual | casual / prose / general |
| `full` (default) | Polished | semi-formal / prose / general |
| `compose` | Compose | semi-formal / lists / general |

Persisted old `light` and `full` values retain their raw names. Their displayed titles change.

## Settings

Settings live at `%APPDATA%\Quill\settings.json`. Missing fields are filled with defaults.

| Key | Default | Meaning |
| --- | --- | --- |
| `voiceDictation.cleanupLevel` | `full` | One of the four raw values above |
| `voiceDictation.formalRegister` | `false` | Replace semi-formal with formal; Casual is unaffected |
| `voiceDictation.layoutCues` | `true` | Consume spoken layout cues outside terminals |
| `voiceDictation.adaptsToApp` | `true` | Capture foreground app context at recording start |
| `voiceDictation.writesNumbers` | `true` | English number formatting when S1-mini does not run |
| `launchAtLogin` | `true` | Start the release app silently in the tray |

At recording start, capture settings and foreground context into an immutable recipe:

1. Adaptation off forces general context.
2. Otherwise check terminal executables or a focused terminal pane, then chat executables/packaged app IDs, then email executables.
3. Terminal forces Verbatim (no directive), disables layout cues, and retains Developer Mode command formatting.
4. Chat preserves the selected style. After joining all segments, a one-sentence, one-line message loses its final full stop.
5. For Polished and Compose only, Email sets directive context to `email`; Casual and Verbatim ignore it.
6. Formal wording changes semi-formal to formal. Compose keeps `lists`; other cleaned styles use `prose`.

Email is labeled **Email** in the recording overlay. Exe comparisons are case-insensitive: `OUTLOOK.EXE`, `olk.exe`, `HxOutlook.exe`, `thunderbird.exe`, `MailClient.exe`. These cover classic/new Outlook, legacy Windows Mail, Thunderbird, and eM Client. Sources: [Microsoft Office command lines](https://support.microsoft.com/en-us/office/lifecycle/command-line-switches-for-microsoft-office-products), [Microsoft Windows Mail process evidence](https://learn.microsoft.com/en-us/answers/questions/4510783/faulting-application-name-hxoutlook-exe-when-copyi), [Mozilla troubleshooting](https://wiki.mozilla.org/Thunderbird/Support/Troubleshooting_Checklist), [eM Client deployment guide](https://www.emclient.com/docs/eM%20Client%20Deployment%20automation%20guide.pdf). Browser-based webmail is not classified by URL.

## Snippets, spoken layout, and joining

The token pass protecting snippets also consumes layout cues before cleanup. Matching snippets take precedence, are never cleaned or re-cased, and alone contribute snippet replacement hits. Cues never count as applied replacements.

`new line` and `newline` emit one newline; `new paragraph` emits two. Tokens are whitespace separated with edge punctuation and a lowercase, punctuation-free normalized form. A cue requires:

- No leading punctuation on its first token, only `. , ; : ! ?` in its last token's trailing punctuation, and no punctuation between two cue words.
- The previous token cannot be an unpunctuated `a an the this that another each every one any no per my your our their its his her whole entire brand`.
- If the cue has no trailing punctuation, the next token cannot be `of item items character characters break breaks`.
- A one-newline cue must start/end the text, follow punctuation, or end in punctuation. Paragraph cues do not need this boundary.

When a cue fires, discard it and its punctuation. Flush the preceding speech as a separate piece, then emit the break. Before a paragraph break, change a final comma, semicolon, or colon on speech to a period. Before a line break, preserve it. Capitalize the first letter of the next speech piece. A snippet directly after a cue retains its expansion exactly.

Each speech piece is chunked and cleaned separately. Vocabulary substitutions run afterward. Joining is shared within and across segments:

- Edge newlines and newline-only pieces form one pending break, capped at two newlines; adjacent breaks use the larger break rather than stacking.
- Without a pending break, closing punctuation `. , ; : ! ? ) ] }` attaches directly.
- A piece beginning `- ` or digits followed by `. ` or `) ` begins a new line. `1.5 million` is ordinary prose.
- Otherwise insert one space.
- Segment results preserve edge breaks; the final cross-segment join discards outside breaks.

## Cleanup guard

Run the guard per cleaned chunk, in this order. A failure returns that entire chunk's recognizer text:

1. **Empty output:** retain the existing rule: blank cleanup restores raw when the raw chunk has at least 12 words.
2. **Negation dropped:** preserve the existing negation/contraction counts. Bare `no` is treated as a correction marker, not a negation.
3. **Unspoken figure:** remove line-start numbering with multiline `^\s*\d+[.)]\s`. Extract ASCII digit runs after deleting thousands commas (comma after a digit and before exactly three digits at a word boundary), keeping leading zeroes. Build ordered sources from raw figures, figures from the available number formatter in all-digits mode, and individual spoken number words. Known figures are their union; raw scale words also permit `1`. A figure passes if known, or if it has at least three digits and appears inside the concatenation of any single source. The first failing figure is the reason. Missing figures are allowed.
4. **Dropped sentence:** split raw at whitespace after `. ! ?`. Content words are distinct lowercase letter-only words of at least four characters, excluding apostrophe-containing words, number/negation words, bare negative contractions, and the supplied filler/stop list in `text/guard.rs`. Judge only sentences with at least five distinct content words. Squash cleaned text to lowercase letters/digits. A word survives if found there; words longer than six characters may match their first five characters. Fewer than one quarter surviving triggers restoration.

The individual number-word source covers zero through nineteen, oh, first through nineteenth, twentieth, thirtieth, twenty through ninety, hundred, thousand, million, billion, half=5, quarter=25.

## UI strings

Settings and onboarding offer **Verbatim | Casual | Polished | Compose**.

Cleanup card:

> How S1-mini writes up what you said. It cleans English only; other languages are written as heard. Whatever the style, Quill keeps the recognizer's wording for any stretch where cleanup would drop a “not” or “never”, write a figure you did not say, or lose a sentence.

Picker details:

- **Verbatim:** Keeps every word you said. Punctuation and capitalization only, straight from the recognizer. Lists keep the numbers you spoke.
- **Casual:** Writes like a text message: lowercase, light punctuation, your wording left as spoken. Corrections are still resolved and numbers written as figures.
- **Polished:** Removes fillers and false starts, writes numbers, dates, and emails properly, and punctuates as sentences. The default.
- **Compose:** Polished, and items you count off or list become a numbered or bulleted list.

**Formal wording** (disabled with engines without cleanup):

> Polished and Compose write contractions out: “I do not think so” for “I don't think so”.

**Adapt to the app:**

> In a terminal, or a terminal pane in VS Code or Cursor, Developer Mode writes spoken commands as commands. In Slack, Teams, Discord and other chat apps, a one-line message drops its full stop. In Outlook and other mail apps, a greeting and a sign-off get their own lines.

**Spoken line breaks:**

> Say “new line” or “new paragraph” to break the text. Said as part of a sentence, as in “add a new line”, the words are typed. Off in terminals.

## Verification and installation

Follow [Development](DEVELOPMENT.md) for prerequisites, the short target directory, Smart App Control settings, bundling native DLLs, and the per-user NSIS installer. Full checks:

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
$env:CARGO_TARGET_DIR = 'E:\quill-target'
$env:CARGO_PROFILE_RELEASE_DEBUG = 'true'
$env:CARGO_PROFILE_RELEASE_BUILD_OVERRIDE_DEBUG = 'true'
npm run build
cargo test --workspace --release --locked
$env:QUILL_CLEANUP_EVAL = '1'
cargo test -p quill --release --locked real_model_cleanup_eval -- --ignored --nocapture
npm run tauri build
```

The real-model test is ignored by default and additionally requires `QUILL_CLEANUP_EVAL=1` and installed `%LOCALAPPDATA%\Quill\Models\s1Mini\s1-mini-q4_k_m.gguf`. Missing prerequisites print an explicit skip. Its 23 fixed fixtures cover fillers, figures, numbers, lists, email, technical terms, and a 151-word status update. Each applies the guard and tests case-insensitive kept substrings, whole-word forbidden phrases, multiline case-insensitive regexes, and expected guard behavior. Every fixture expects no guard firing. JSON captures actual model output, delivered output, timing, guard reasons, and every missed rubric at `%LOCALAPPDATA%\Quill\Eval\cleanup-report.json`; override with `QUILL_CLEANUP_REPORT`. Any miss fails the test; do not weaken the guard or rubrics to hide a miss.

Install the generated NSIS setup per-user. Release startup reconciles the saved login preference with the current exe path, quotes the path, and uses `--autostart`. Development builds never change login registration. Login launches stay in the tray after onboarding; another `--autostart` invocation does not open Settings. A normal app launch opens Settings. A true sign-in test still requires a later Windows sign-in.

### Local verification, October 7

- Full Windows workspace suite: 92 passed; the real-model test is ignored in the default suite. The final cue boundary adjustment also passed the complete 88-test core suite.
- Optional-number-formatter-disabled build: 74 tests passed.
- Real S1-mini eval: 23/23 fixtures passed, zero guard restorations, no relaxed rubrics. [Actual outputs and timings](cleanup-eval-2026-10-07.json). Median 107 ms; range 60–3,689 ms on the local RTX 3070 Ti. Phone output was `5550198` without a hyphen and the amount stayed `two and a half`; both satisfy the specified rubrics.
- Frontend production build passed. Browser checks covered style selection, new switches, disabled controls without cleanup, and all four onboarding styles.
- Release built, NSIS bundled, updater signature generated using the existing key, and per-user installation exited successfully. In PowerShell, assigning an empty environment variable may remove it; bundling with `--ci` and an explicitly empty `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` in the child process environment avoids an interactive signing prompt.
- Installed binary matches the release build except for Tauri's expected three-byte NSIS bundle marker, and uses the Windows GUI subsystem. The login registry entry now points to the quoted installed exe. Fresh and duplicate login launches leave Quill's UI hidden; normal launch opens Settings; closing Settings leaves Quill running.
- A real reboot/sign-in and live microphone-to-paste dictation were not exercised.

## Known limits

- A list or email spanning an early segment cut is formatted per segment.
- The guard returns a whole chunk to raw for one flagged sentence.
