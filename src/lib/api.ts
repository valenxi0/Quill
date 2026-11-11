// Typed bridge to the Rust side. Shapes mirror the serde types exactly.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type EngineId = "parakeetS1" | "cohereTranscribe" | "whisperTurbo" | "whisperBase" | "whisperTiny";
export type AssetId =
  | "parakeetTdt06bV3"
  | "s1Mini"
  | "cohereTranscribe"
  | "whisperLargeV3Turbo"
  | "whisperBaseEnglish"
  | "whisperTinyEnglish";
export type ActivationMode = "pushToTalk" | "toggle" | "holdOrLock";
export type CleanupLevel = "verbatim" | "light" | "full" | "compose";
export type EntryKind = "vocabulary" | "snippet";
export type Phase = "idle" | "recording" | "transcribing" | "cleaning" | "completed" | "failed" | "cancelled";
export type AppContext = "general" | "chat" | "terminal" | "email";

export type ShortcutSlot = "dictate" | "raw" | "sketch";

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
  appExe?: string | null;
}

export interface AppStyle {
  appExe: string;
  level: CleanupLevel;
}

export type OtherAudio = "nothing" | "lower" | "mute" | "pause";

/** A microphone picked in Settings, by WASAPI endpoint id. */
export interface SavedDevice {
  id: string;
  name: string;
}

export interface InputDevice {
  id: string;
  name: string;
  bluetooth: boolean;
  /** A microphone or headset, not a line input or virtual cable. */
  isMic: boolean;
}

export interface Microphones {
  devices: InputDevice[];
  /** The mic a recording opens now. */
  using: string | null;
  /** The saved mic, when it isn't connected. */
  missing: string | null;
  /** The Bluetooth default Quill skips. */
  avoided: string | null;
}

export interface VoiceDictation {
  engine: EngineId;
  activationMode: ActivationMode;
  hotkey: Hotkey;
  /** Dictates without cleanup, in the same mode. */
  rawHotkey: Hotkey | null;
  cleanupLevel: CleanupLevel;
  formalRegister: boolean;
  layoutCues: boolean;
  language: string;
  writesNumbers: boolean;
  adaptsToApp: boolean;
  learnsCorrections: boolean;
  historyEnabled: boolean;
  insightsEnabled: boolean;
  vocabulary: VocabEntry[];
  appStyles: AppStyle[];
  fitsToCursor: boolean;
  spokenEnter: boolean;
  splitsTerminalPastes: boolean;
  livePreview: boolean;
  /** `null` follows the Windows default. */
  inputDevice: SavedDevice | null;
  avoidBluetoothMic: boolean;
  otherAudio: OtherAudio;
  lowerToPercent: number;
  vocabularyBoost: boolean;
  /** Put the previous clipboard back after a paste. */
  restoreClipboard: boolean;
  historyRetention: HistoryRetention;
  /** How the recording pill draws your voice. */
  pillStyle: PillStyle;
}

export type PillStyle = "bars" | "waveform" | "dots" | "glow";

export type HistoryRetention = "day" | "week" | "month";

export type DrawKey = "shift" | "ctrl" | "alt";

export interface SketchSettings {
  enabled: boolean;
  hotkey: Hotkey;
  drawKey: DrawKey;
}

/** What the Sketch bar shows. */
export interface SketchView {
  marks: number;
  penOn: boolean;
  drawKey: string;
  finishShortcut: string;
  startedAt: number;
}

export interface Settings {
  enabled: boolean;
  voiceDictation: VoiceDictation;
  sketch: SketchSettings;
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
  hotwordsUnavailable: boolean;
}

export interface KnownApp {
  exe: string;
  name: string;
  icon?: string | null;
}

export interface CorrectionPair {
  appExe?: string | null;
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
  sketchHotkeyDisplay: string;
  sketchHotkeyIsDefault: boolean;
  sketchHotkeyProblem: string | null;
  capturingSketchHotkey: boolean;
  rawHotkeyDisplay: string | null;
  rawHotkeyProblem: string | null;
  capturingRawHotkey: boolean;
  hotkeyNote: string | null;
  rawHotkeyNote: string | null;
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
  /** Empty in entries from before 0.3. */
  raw: string;
  level: CleanupLevel | null;
  context: AppContext;
  engine: EngineId;
  appName: string;
  appExe: string;
  secondsSpoken: number;
  releaseToPasteMs: number;
  createdAt: string;
  /** Why cleanup kept some words as heard. Absent in older entries. */
  cleanupNote?: string | null;
}

/** A history entry cleaned again. */
export interface Redone {
  text: string;
  /** Why cleanup kept some words as heard. */
  cleanupNote: string | null;
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
  apps: (KnownApp & { words: number })[];
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
  /** Whether this run is on the CPU because a GPU start crashed. */
  gpuFallback: () => invoke<boolean>("gpu_fallback"),
  retryGpu: () => invoke<void>("retry_gpu"),
  beginHotkeyCapture: (slot: ShortcutSlot = "dictate") => invoke<void>("begin_hotkey_capture", { slot }),
  cancelHotkeyCapture: () => invoke<void>("cancel_hotkey_capture"),
  /** Back to the default; the no-cleanup shortcut is cleared. */
  resetHotkey: (slot: ShortcutSlot = "dictate") => invoke<void>("reset_hotkey", { slot }),
  sketchAction: (action: "pen" | "undo" | "cancel" | "finish") => invoke<void>("sketch_action", { action }),
  sketchState: () => invoke<SketchView | null>("sketch_state"),
  dictationCommand: (command: string) => invoke<void>("dictation_command", { command }),
  openMicrophoneSettings: () => invoke<void>("open_microphone_settings"),
  getMicrophones: (inputDevice: SavedDevice | null, avoidBluetooth: boolean) =>
    invoke<Microphones>("get_microphones", { inputDevice, avoidBluetooth }),
  copyText: (text: string) => invoke<boolean>("copy_text", { text }),
  pasteLast: () => invoke<void>("paste_last"),
  redoHistory: (id: string, level: CleanupLevel) => invoke<Redone>("redo_history", { id, level }),
  /** Newest first; `query` matches the text, the words heard, and the app. At most 200. */
  getHistory: (query = "", limit = 20) => invoke<HistoryEntry[]>("get_history", { query, limit }),
  clearHistory: () => invoke<void>("clear_history"),
  /** How many entries keeping history for `keep` would delete now. */
  countExpiringHistory: (keep: HistoryRetention) => invoke<number>("count_expiring_history", { keep }),
  getInsights: () => invoke<Insights>("get_insights"),
  getKnownApps: () => invoke<KnownApp[]>("get_known_apps"),
  resetInsights: () => invoke<void>("reset_insights"),
  parseVocabularyList: (text: string, appExe: string | null = null) => invoke<VocabEntry[]>("parse_vocabulary_list", { text, appExe }),
  testVocabularyEntry: (entry: VocabEntry, sample: string) =>
    invoke<string>("test_vocabulary_entry", { entry, sample }),
  acceptSuggestion: (from: string, to: string, appExe: string | null = null) => invoke<void>("accept_suggestion", { from, to, appExe }),
  dismissSuggestion: (from: string, to: string, appExe: string | null = null) => invoke<void>("dismiss_suggestion", { from, to, appExe }),
  forgetCorrections: () => invoke<void>("forget_corrections"),
  hideOverlay: () => invoke<void>("hide_overlay"),
  animationsEnabled: () => invoke<boolean>("animations_enabled"),
  pillStyle: () => invoke<PillStyle>("pill_style"),
  checkForUpdates: () => invoke<UpdateInfo | null>("check_for_updates"),
  installUpdate: () => invoke<void>("install_update"),
  openReleases: () => invoke<void>("open_releases"),
  pendingSuggestion: () => invoke<CorrectionPair | null>("pending_suggestion"),
};

export function on<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => handler(e.payload));
}

export const LINKS = ["quill://start", "quill://stop", "quill://toggle", "quill://cancel", "quill://paste-last"] as const;
