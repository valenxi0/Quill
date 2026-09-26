// Typed bridge to the Rust side. Shapes mirror the serde types exactly.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type EngineId = "parakeetS1" | "cohereTranscribe" | "whisperTurbo" | "whisperBase" | "whisperTiny";
export type AssetId =
  | "parakeetTdt06bV3"
  | "parakeetVocabularyBooster"
  | "s1Mini"
  | "cohereTranscribe"
  | "whisperLargeV3Turbo"
  | "whisperBaseEnglish"
  | "whisperTinyEnglish";
export type ActivationMode = "pushToTalk" | "toggle";
export type CleanupLevel = "verbatim" | "light" | "full";
export type EntryKind = "vocabulary" | "snippet";
export type Phase = "idle" | "recording" | "transcribing" | "cleaning" | "completed" | "failed" | "cancelled";
export type AppContext = "general" | "chat" | "terminal";

export interface Hotkey {
  vk: number;
  modifiers: number;
}

export interface VocabEntry {
  id: string;
  phrase: string;
  replacement: string;
  kind: EntryKind;
  isEnabled: boolean;
}

export interface VoiceDictation {
  engine: EngineId;
  activationMode: ActivationMode;
  hotkey: Hotkey;
  cleanupLevel: CleanupLevel;
  language: string;
  writesNumbers: boolean;
  adaptsToApp: boolean;
  learnsCorrections: boolean;
  historyEnabled: boolean;
  insightsEnabled: boolean;
  vocabulary: VocabEntry[];
}

export interface Settings {
  enabled: boolean;
  voiceDictation: VoiceDictation;
  launchAtLogin: boolean;
  historyExpanded: boolean;
  onboarded: boolean;
}

export type ModelState =
  | { state: "notDownloaded" }
  | { state: "updateAvailable" }
  | { state: "downloading"; fraction: number; bytes: number; total: number }
  | { state: "verifying" }
  | { state: "downloaded" }
  | { state: "failed"; message: string }
  | { state: "unavailable" };

export interface AssetView {
  id: AssetId;
  title: string;
  size: number;
  license: string;
  licenseNote: string;
  runtime: string;
  optional: boolean;
  available: boolean;
  state: ModelState;
  hasData: boolean;
}

export interface EngineView {
  id: EngineId;
  title: string;
  badge: string;
  badgeAccent: boolean;
  summary: string;
  languageNote: string;
  recognizer: "parakeet" | "whisper" | "cohere";
  hasCleanup: boolean;
  showsLanguagePicker: boolean;
  englishOnly: boolean;
  supported: boolean;
  assets: AssetView[];
  ready: boolean;
  busy: boolean;
  hasData: boolean;
  updateAvailable: boolean;
}

export interface Status {
  kind: "ready" | "off" | "unsupported" | "modelsMissing" | "micDenied" | "micUnavailable";
  dot: "green" | "yellow" | "orange" | "gray";
  title: string;
  detail: string;
  hotkeyProblem: string | null;
  engineLoaded: boolean;
  engineLoading: boolean;
  warmError: string | null;
}

export interface CorrectionPair {
  from: string;
  to: string;
  count: number;
  dismissed: boolean;
}

export interface DictationView {
  phase: Phase;
  outcome: string | null;
  errorTitle: string | null;
  errorDetail: string | null;
  context: AppContext;
}

export interface UpdateInfo {
  version: string;
  current: string;
  notes: string | null;
}

export interface UiState {
  settings: Settings;
  status: Status;
  engines: EngineView[];
  languages: [string, string][];
  cohereAutoLanguage: string;
  hotkeyDisplay: string;
  hotkeyIsDefault: boolean;
  capturingHotkey: boolean;
  captureError: string | null;
  itnAvailable: boolean;
  suggestions: CorrectionPair[];
  correctionsSeen: number;
  historyCount: number;
  hasDictated: boolean;
  dictionaryHits: Record<string, number>;
  dictation: DictationView;
  hasLastTranscript: boolean;
  version: string;
  update: UpdateInfo | null;
  updating: boolean;
  updateError: string | null;
}

export interface HistoryEntry {
  id: string;
  text: string;
  engine: EngineId;
  appName: string;
  appExe: string;
  secondsSpoken: number;
  releaseToPasteMs: number;
  createdAt: string;
}

export interface Insights {
  isEmpty: boolean;
  weekWords: number;
  weekSavedSeconds: number;
  wpm: number;
  sparkline: number[];
  monthSavedSeconds: number;
  totalWords: number;
  since: string | null;
  apps: { exe: string; name: string; words: number }[];
  currentStreak: number;
  longestStreak: number;
  heatmap: { date: string; words: number; future: boolean }[];
  fillersRemoved: number;
  dictionaryExpansions: number;
  medianLatency: number | null;
}

export const api = {
  getState: () => invoke<UiState>("get_state"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  downloadEngine: (engine: EngineId) => invoke<void>("download_engine", { engine }),
  downloadAsset: (id: AssetId) => invoke<void>("download_asset", { id }),
  cancelDownload: (engine: EngineId) => invoke<void>("cancel_download", { engine }),
  cancelAsset: (id: AssetId) => invoke<void>("cancel_asset", { id }),
  removeAsset: (id: AssetId) => invoke<void>("remove_asset", { id }),
  removeEngine: (engine: EngineId) => invoke<void>("remove_engine", { engine }),
  loadEngine: () => invoke<void>("load_engine"),
  beginHotkeyCapture: () => invoke<void>("begin_hotkey_capture"),
  cancelHotkeyCapture: () => invoke<void>("cancel_hotkey_capture"),
  resetHotkey: () => invoke<void>("reset_hotkey"),
  openMicrophoneSettings: () => invoke<void>("open_microphone_settings"),
  copyText: (text: string) => invoke<boolean>("copy_text", { text }),
  getHistory: () => invoke<HistoryEntry[]>("get_history"),
  clearHistory: () => invoke<void>("clear_history"),
  getInsights: () => invoke<Insights>("get_insights"),
  resetInsights: () => invoke<void>("reset_insights"),
  parseVocabularyList: (text: string) => invoke<VocabEntry[]>("parse_vocabulary_list", { text }),
  testVocabularyEntry: (entry: VocabEntry, sample: string) =>
    invoke<string>("test_vocabulary_entry", { entry, sample }),
  acceptSuggestion: (from: string, to: string) => invoke<void>("accept_suggestion", { from, to }),
  dismissSuggestion: (from: string, to: string) => invoke<void>("dismiss_suggestion", { from, to }),
  forgetCorrections: () => invoke<void>("forget_corrections"),
  hideOverlay: () => invoke<void>("hide_overlay"),
  animationsEnabled: () => invoke<boolean>("animations_enabled"),
  checkForUpdates: () => invoke<UpdateInfo | null>("check_for_updates"),
  installUpdate: () => invoke<void>("install_update"),
  pendingSuggestion: () => invoke<CorrectionPair | null>("pending_suggestion"),
};

export function on<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => handler(e.payload));
}

export const LINKS = ["quill://start", "quill://stop", "quill://toggle", "quill://cancel"] as const;
