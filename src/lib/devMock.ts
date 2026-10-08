// Browser-only preview of the UI with a fake backend (`npm run dev`, then open
// http://localhost:1420). Never loaded inside the Tauri app.
import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type { Insights, Settings, UiState } from "./api";

const settings: Settings = {
  enabled: true,
  launchAtLogin: true,
  historyExpanded: true,
  onboarded: new URLSearchParams(location.search).has("onboarded"),
  voiceDictation: {
    engine: "parakeetS1",
    activationMode: "pushToTalk",
    hotkey: { vk: 0x4d, modifiers: 3 },
    cleanupLevel: "full",
    formalRegister: false,
    layoutCues: true,
    language: "auto",
    writesNumbers: true,
    adaptsToApp: true,
    learnsCorrections: true,
    historyEnabled: true,
    insightsEnabled: true,
    vocabulary: [
      { id: "a", phrase: "mac island", replacement: "Nano", kind: "vocabulary", isEnabled: true },
      { id: "b", phrase: "my sign off", replacement: "Best,\nJorge", kind: "snippet", isEnabled: true },
    ],
  },
};

const asset = (id: string, title: string, size: number, state: unknown, optional = false, available = true) => ({
  id, title, size, license: "MIT", licenseNote: `${title} license note.`, runtime: "whisper.cpp", optional, available, state, hasData: false,
});

const state: UiState = {
  settings,
  status: { kind: "ready", dot: "green", title: "Ready · Ctrl+Alt+M", detail: "Hold the shortcut while you speak. Release to finish.", hotkeyProblem: null, engineLoaded: true, engineLoading: false, warmError: null },
  engines: [
    { id: "parakeetS1", title: "Parakeet + S1-mini", badge: "Best quality · Heaviest", badgeAccent: true, summary: "Best English results. Parakeet recognizes speech; S1-mini by Superwhisper removes fillers and false starts and writes numbers, dates, and emails properly.", languageNote: "Parakeet hears 25 European languages on its own. S1-mini cleanup runs on English; other languages are written as recognized.", recognizer: "parakeet", hasCleanup: true, showsLanguagePicker: false, englishOnly: false, supported: true, ready: true, busy: false, hasData: true, updateAvailable: false,
      assets: [asset("parakeetTdt06bV3", "Parakeet TDT 0.6B v3", 670478772, { state: "downloaded" }), asset("s1Mini", "S1-mini", 484232348, { state: "downloaded" }), asset("parakeetVocabularyBooster", "Vocabulary booster (Parakeet CTC 110M)", 0, { state: "unavailable" }, true, false)] },
    { id: "cohereTranscribe", title: "Cohere Transcribe", badge: "Experimental · Slow", badgeAccent: false, summary: "Experimental. Strong on German, French, Spanish…", languageNote: "Cohere cannot detect the language: speech in another one comes out translated. Automatic follows your PC's language.", recognizer: "cohere", hasCleanup: false, showsLanguagePicker: true, englishOnly: false, supported: true, ready: false, busy: true, hasData: true, updateAvailable: false,
      assets: [asset("cohereTranscribe", "Cohere Transcribe 03-2026", 2888151330, { state: "downloading", fraction: 0.42, bytes: 1213023558, total: 2888151330 })] },
    { id: "whisperTiny", title: "Whisper Tiny", badge: "Fastest · Lightest", badgeAccent: false, summary: "Fastest and easiest on your PC, but trades recognition accuracy for its small footprint.", languageNote: "English-only model (base.en / tiny.en). Use Whisper Turbo or Cohere Transcribe for other languages.", recognizer: "whisper", hasCleanup: false, showsLanguagePicker: false, englishOnly: true, supported: true, ready: false, busy: false, hasData: false, updateAvailable: false,
      assets: [asset("whisperTinyEnglish", "Whisper tiny.en", 77704715, { state: "failed", message: "ggml-tiny.en.bin did not match its pinned checksum. Retry, or report the mismatch." })] },
  ] as UiState["engines"],
  languages: [["en", "English"], ["de", "German"], ["fr", "French"]],
  cohereAutoLanguage: "English",
  hotkeyDisplay: "Ctrl+Alt+M",
  hotkeyIsDefault: true,
  capturingHotkey: false,
  captureError: null,
  itnAvailable: true,
  suggestions: [{ from: "Tori", to: "Tauri", count: 2, dismissed: false }],
  correctionsSeen: 5,
  historyCount: 2,
  hasDictated: true,
  dictionaryHits: { a: 12, b: 3 },
  dictation: { phase: "idle", outcome: null, errorTitle: null, errorDetail: null, context: "general" },
  hasLastTranscript: true,
  version: "0.1.0",
  update: { version: "0.2.0", current: "0.1.0", notes: "Faster Parakeet on long dictations." },
  updating: false,
  updateError: null,
};

const today = new Date();
const iso = (d: Date) => d.toISOString().slice(0, 10);
const monday = new Date(today);
monday.setDate(today.getDate() - ((today.getDay() + 6) % 7) - 14 * 7);
const insights: Insights = {
  isEmpty: false, weekWords: 1840, weekSavedSeconds: 1980, wpm: 152, sparkline: [120, 140, 131, 160, 152, 171, 149],
  monthSavedSeconds: 7400, totalWords: 12480, since: "2026-08-02",
  apps: [{ exe: "slack.exe", name: "Slack", words: 5200 }, { exe: "code.exe", name: "Visual Studio Code", words: 3100 }, { exe: "outlook.exe", name: "Outlook", words: 1400 }],
  currentStreak: 6, longestStreak: 11,
  heatmap: Array.from({ length: 105 }, (_, i) => {
    const d = new Date(monday); d.setDate(monday.getDate() + i);
    return { date: iso(d), words: d > today ? 0 : Math.max(0, Math.round(Math.sin(i * 1.7) * 300)), future: d > today };
  }),
  fillersRemoved: 214, dictionaryExpansions: 15, medianLatency: 0.42,
};

export function installDevMock() {
  mockIPC((cmd) => {
    switch (cmd) {
      case "get_state": return state;
      case "get_insights": return insights;
      case "get_history": return [
        { id: "1", text: "Can we move the meeting to Thursday afternoon instead?", engine: "parakeetS1", appName: "Slack", appExe: "slack.exe", secondsSpoken: 3.1, releaseToPasteMs: 410, createdAt: new Date().toISOString() },
        { id: "2", text: "git commit -m \"fix the build\"", engine: "parakeetS1", appName: "Windows Terminal", appExe: "WindowsTerminal.exe", secondsSpoken: 2.2, releaseToPasteMs: 180, createdAt: new Date().toISOString() },
      ];
      case "animations_enabled": return true;
      case "parse_vocabulary_list": return [];
      case "test_vocabulary_entry": return "";
      default: return null;
    }
  }, { shouldMockEvents: true });
  // Drive the overlay from the console: __quillEmit("dictation", {...}).
  (window as unknown as Record<string, unknown>).__quillEmit = (event: string, payload: unknown) => emit(event, payload);
}
