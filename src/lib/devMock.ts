// Browser-only preview of the UI with a fake backend (`npm run dev`, then open
// http://localhost:1420). Never loaded inside the Tauri app.
import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import type { InputDevice, Insights, KnownApp, Microphones, SavedDevice, Settings, UiState } from "./api";
import pkg from "../../package.json";

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
      { id: "b", phrase: "my sign off", replacement: "Best,\nAlex", kind: "snippet", isEnabled: true },
    ],
    appStyles: [{ appExe: "discord.exe", level: "light" }],
    fitsToCursor: true,
    spokenEnter: false,
    splitsTerminalPastes: true,
    livePreview: false,
    // ?missingmic previews a saved mic that isn't plugged in.
    inputDevice: new URLSearchParams(location.search).has("missingmic") ? { id: "{0.0.1.00000000}.{gone}", name: "Yeti Nano" } : null,
    avoidBluetoothMic: true,
    otherAudio: "nothing",
    lowerToPercent: 20,
    vocabularyBoost: false,
    restoreClipboard: false,
    historyRetention: "day",
  },
  sketch: { enabled: true, hotkey: { vk: 0x53, modifiers: 3 }, drawKey: "shift" },
};

const asset = (id: string, title: string, size: number, state: unknown, optional = false, available = true) => ({
  id, title, size, license: "MIT", licenseNote: `${title} license note.`, runtime: "whisper.cpp", optional, available, state, hasData: false,
});

const state: UiState = {
  settings,
  status: { kind: "ready", dot: "green", title: "Ready · Ctrl+Alt+M", detail: "Hold the shortcut while you speak. Release to finish.", hotkeyProblem: null, engineLoaded: true, engineLoading: false, warmError: null, hotwordsUnavailable: false },
  engines: [
    { id: "parakeetS1", title: "Parakeet + S1-mini", badge: "Best quality · Heaviest", badgeAccent: true, summary: "Best English results. Parakeet recognizes speech; S1-mini by Superwhisper removes fillers and false starts and writes numbers, dates, and emails properly.", languageNote: "Parakeet hears 25 European languages on its own. S1-mini cleanup runs on English; other languages are written as recognized.", recognizer: "parakeet", hasCleanup: true, showsLanguagePicker: false, englishOnly: false, supported: true, ready: true, busy: false, hasData: true, updateAvailable: false,
      assets: [asset("parakeetTdt06bV3", "Parakeet TDT 0.6B v3", 670478772, { state: "downloaded" }), asset("s1Mini", "S1-mini", 484232348, { state: "downloaded" })] },
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
  sketchHotkeyDisplay: "Ctrl+Alt+S",
  sketchHotkeyIsDefault: true,
  sketchHotkeyProblem: null,
  capturingSketchHotkey: false,
  captureError: null,
  itnAvailable: true,
  suggestions: [{ from: "Tori", to: "Tauri", count: 2, dismissed: false, appExe: "code.exe" }],
  correctionsSeen: 5,
  historyCount: 45,
  hasDictated: true,
  dictionaryHits: { a: 12, b: 3 },
  dictation: { phase: "idle", outcome: null, errorTitle: null, errorDetail: null, context: "general" },
  hasLastTranscript: true,
  version: pkg.version,
  // ?update previews the banner with notes as GitHub sends them.
  update: new URLSearchParams(location.search).has("update")
    ? { version: "0.4.0", current: pkg.version, notes: "## [Download Quill-Setup.exe for Windows](https://github.com/valenxi0/Quill/releases/download/v0.4.0/Quill-Setup.exe)" }
    : null,
  updating: false,
  updateError: null,
};

// ?btdefault previews a Bluetooth headset as the Windows default.
const mics: InputDevice[] = [
  { id: "{0.0.1.00000000}.{457aaca4}", name: "Microphone (Scarlett Solo USB)", bluetooth: false, isMic: true },
  { id: "{0.0.1.00000000}.{9c1e22b0}", name: "Headset (WH-1000XM5 Hands-Free)", bluetooth: true, isMic: true },
];
const defaultMic = new URLSearchParams(location.search).has("btdefault") ? mics[1]! : mics[0]!;

function microphones(saved: SavedDevice | null, avoid: boolean): Microphones {
  const found = saved && mics.find((m) => m.id === saved.id);
  if (found) return { devices: mics, using: found.name, missing: null, avoided: null };
  const missing = saved?.name ?? null;
  const other = mics.find((m) => m.isMic && !m.bluetooth);
  if (avoid && defaultMic.bluetooth && other) return { devices: mics, using: other.name, missing, avoided: defaultMic.name };
  return { devices: mics, using: defaultMic.name, missing, avoided: null };
}

const today = new Date();
const iso = (d: Date) => d.toISOString().slice(0, 10);
const monday = new Date(today);
monday.setDate(today.getDate() - ((today.getDay() + 6) % 7) - 14 * 7);
const insights: Insights = {
  isEmpty: false, weekWords: 1840, weekSavedSeconds: 1980, wpm: 152, sparkline: [120, 140, 131, 160, 152, 171, 149],
  monthSavedSeconds: 7400, totalWords: 12480, since: "2026-08-02",
  apps: [],
  currentStreak: 6, longestStreak: 11,
  heatmap: Array.from({ length: 105 }, (_, i) => {
    const d = new Date(monday); d.setDate(monday.getDate() + i);
    return { date: iso(d), words: d > today ? 0 : Math.max(0, Math.round(Math.sin(i * 1.7) * 300)), future: d > today };
  }),
  fillersRemoved: 214, dictionaryExpansions: 15, medianLatency: 0.42,
};

export async function installDevMock() {
  let apps: KnownApp[] = [];
  try {
    const response = await fetch("/__quill_preview_apps");
    const data: unknown = await response.json();
    if (Array.isArray(data)) {
      apps = data.filter((item): item is KnownApp => item != null && typeof item === "object" &&
        "exe" in item && typeof item.exe === "string" && "name" in item && typeof item.name === "string" &&
        "icon" in item && (item.icon === null || typeof item.icon === "string"));
    }
  } catch {
    console.warn("Installed app icons are unavailable in this browser preview.");
  }
  // Lead with apps people recognize; helper processes make odd sample rows.
  const preferred = ["t3 code (nightly).exe", "chatgpt.exe", "brave.exe", "discord.exe", "claude.exe", "code.exe", "slack.exe", "notepad.exe"];
  const rank = (exe: string) => {
    const k = preferred.indexOf(exe.toLowerCase());
    return k < 0 ? preferred.length : k;
  };
  apps = apps.filter((a) => !/\.|runtime|settings|host/i.test(a.name)).sort((a, b) => rank(a.exe) - rank(b.exe));
  insights.apps = apps.map((app, index) => ({ ...app, words: [5200, 3100, 1400, 960, 300, 156][index] ?? 78 }));
  // Three samples, then filler so "Show more" has something to show.
  const history = [
    { id: "1", text: "Can we move the meeting to Thursday afternoon instead?", raw: "Um, can we move the meeting to, uh, Thursday afternoon instead?", level: "full", context: "chat", engine: "parakeetS1", appName: "Slack", appExe: "slack.exe", secondsSpoken: 3.1, releaseToPasteMs: 410, createdAt: new Date().toISOString() },
    { id: "2", text: "git commit -m \"fix the build\"", raw: "git commit dash m quote fix the build quote", level: "full", context: "terminal", engine: "parakeetS1", appName: "Windows Terminal", appExe: "WindowsTerminal.exe", secondsSpoken: 2.2, releaseToPasteMs: 180, createdAt: new Date().toISOString() },
    { id: "3", text: "Translate this into French: the meeting is moved to Monday.", raw: "Translate this into French: the meeting is moved to Monday.", level: "full", context: "general", engine: "parakeetS1", appName: "Notepad", appExe: "notepad.exe", secondsSpoken: 2.4, releaseToPasteMs: 260, createdAt: new Date().toISOString(), cleanupNote: "Kept your words: cleanup dropped words you said." },
    ...Array.from({ length: 42 }, (_, i) => ({
      id: `f${i}`, text: `Note ${i + 1}: follow up on the release checklist.`, raw: `note ${i + 1} follow up on the release checklist`, level: "light", context: "general", engine: "parakeetS1",
      appName: i % 2 ? "Notepad" : "Outlook", appExe: i % 2 ? "notepad.exe" : "outlook.exe", secondsSpoken: 2, releaseToPasteMs: 220, createdAt: new Date(Date.now() - (i + 1) * 1800_000).toISOString(),
    })),
  ];
  mockIPC((cmd, args) => {
    switch (cmd) {
      case "get_microphones": {
        const a = args as { inputDevice: SavedDevice | null; avoidBluetooth: boolean };
        return microphones(a.inputDevice, a.avoidBluetooth);
      }
      case "get_state": return state;
      case "get_insights": return insights;
      case "get_known_apps": return insights.apps;
      case "get_history": {
        const { query = "", limit = 20 } = (args ?? {}) as { query?: string; limit?: number };
        const words = query.toLowerCase().split(/\s+/).filter(Boolean);
        return history
          .filter((e) => words.every((w) => `${e.text}
${e.raw}
${e.appName}`.toLowerCase().includes(w)))
          .slice(0, Math.min(limit, 200));
      }
      case "animations_enabled": return true;
      // ?gpucrash previews the CPU fallback note.
      case "gpu_fallback": return new URLSearchParams(location.search).has("gpucrash");
      case "parse_vocabulary_list": return [];
      case "test_vocabulary_entry": return "";
      case "redo_history": return { text: "Could we move the meeting to Thursday afternoon?", cleanupNote: "Kept some of your words: cleanup dropped a \"not\"." };
      default: return null;
    }
  }, { shouldMockEvents: true });
  // Drive the overlay from the console: __quillEmit("dictation", {...}).
  (window as unknown as Record<string, unknown>).__quillEmit = (event: string, payload: unknown) => emit(event, payload);
}
