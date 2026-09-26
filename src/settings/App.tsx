import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, on, type Settings, type UiState, type VoiceDictation } from "../lib/api";
import { Card, cx, Icon, Switch } from "./ui";
import { StatusCard, SetupTab } from "./SetupTab";
import { VoiceTab } from "./VoiceTab";
import { InsightsTab } from "./InsightsTab";

export type Tab = "insights" | "voice" | "setup";

export interface Ctx {
  state: UiState;
  save: (next: Settings) => void;
  saveVd: (patch: Partial<VoiceDictation>) => void;
}

function useQuill() {
  const [state, setState] = useState<UiState | null>(null);
  const refresh = useCallback(() => {
    api.getState().then(setState).catch(console.error);
  }, []);
  useEffect(() => {
    refresh();
    const un = on("state-changed", refresh);
    // Readiness also depends on things Windows changes (mic permission).
    const onFocus = () => refresh();
    window.addEventListener("focus", onFocus);
    return () => {
      un.then((f) => f());
      window.removeEventListener("focus", onFocus);
    };
  }, [refresh]);
  const save = useCallback((next: Settings) => {
    setState((s) => (s ? { ...s, settings: next } : s));
    api.setSettings(next).catch(console.error);
  }, []);
  return { state, save };
}

const TABS: { id: Tab; label: string }[] = [
  { id: "insights", label: "Insights" },
  { id: "voice", label: "Your voice" },
  { id: "setup", label: "Setup" },
];

export default function App() {
  const { state, save } = useQuill();
  const [tab, setTab] = useState<Tab | null>(null);

  useEffect(() => {
    api.animationsEnabled().then((on) => document.documentElement.classList.toggle("reduced-motion", !on));
  }, []);

  // Open on Setup until the first dictation, then on Insights.
  useEffect(() => {
    if (state && tab === null) setTab(state.hasDictated ? "insights" : "setup");
  }, [state, tab]);

  if (!state || !tab) return <div className="h-screen bg-bg" />;

  const saveVd = (patch: Partial<VoiceDictation>) =>
    save({ ...state.settings, voiceDictation: { ...state.settings.voiceDictation, ...patch } });
  const ctx: Ctx = { state, save, saveVd };
  const enabled = state.settings.enabled;
  const pinStatus =
    enabled && tab !== "setup" && (state.status.kind !== "ready" || state.status.hotkeyProblem !== null);

  return (
    <div className="flex h-screen flex-col bg-bg">
      <header className="flex items-center gap-3 px-6 pt-5 pb-3">
        <div className="flex h-10 w-10 items-center justify-center rounded-[10px] bg-accent text-white">
          <Icon.Waveform size={22} />
        </div>
        <div className="min-w-0 flex-1">
          <h1 className="text-[20px] font-semibold leading-tight">Quill</h1>
          <p className="text-[12.5px] text-text-2">Hold a shortcut, speak, and it pastes. Everything runs on this PC.</p>
        </div>
        <Switch large label="Quill" checked={enabled} onChange={(v) => save({ ...state.settings, enabled: v })} />
      </header>

      {!enabled ? (
        <div className="px-6 pt-2">
          <Card>
            <h3 className="text-[13.5px] font-semibold">Quill is off</h3>
            <p className="mt-0.5 text-[12px] text-text-2">
              The shortcut does nothing and no model stays loaded. Turn Quill on to dictate again.
            </p>
          </Card>
        </div>
      ) : (
        <>
          {pinStatus && (
            <div className="px-6 pb-2">
              <StatusCard ctx={ctx} />
            </div>
          )}
          <TabBar tab={tab} onChange={setTab} />
          <main className="flex-1 overflow-y-auto px-6 pt-3 pb-6">
            {tab === "setup" && <SetupTab ctx={ctx} />}
            {tab === "voice" && <VoiceTab ctx={ctx} />}
            {tab === "insights" && <InsightsTab ctx={ctx} />}
          </main>
        </>
      )}
    </div>
  );
}

function TabBar({ tab, onChange }: { tab: Tab; onChange: (t: Tab) => void }) {
  const refs = useRef<Record<string, HTMLButtonElement | null>>({});
  const [bar, setBar] = useState({ left: 0, width: 0 });
  useLayoutEffect(() => {
    const el = refs.current[tab];
    if (el) setBar({ left: el.offsetLeft, width: el.offsetWidth });
  }, [tab]);
  return (
    <nav className="relative mx-6 flex gap-5 border-b border-hairline" role="tablist">
      {TABS.map((t) => (
        <button
          key={t.id}
          ref={(el) => {
            refs.current[t.id] = el;
          }}
          role="tab"
          aria-selected={t.id === tab}
          onClick={() => onChange(t.id)}
          className={cx("pb-2 text-[13px]", t.id === tab ? "font-semibold text-text" : "text-text-2 hover:text-text")}
        >
          {t.label}
        </button>
      ))}
      <span
        className="absolute -bottom-px h-[2px] rounded-full bg-accent transition-all duration-200"
        style={{ left: bar.left, width: bar.width }}
      />
    </nav>
  );
}
