import { Fragment, useCallback, useEffect, useState, type ReactNode } from "react";
import { BookOpenText, GearSix, House, MagicWand, Microphone, PencilSimpleLine, ShieldCheck, type Icon as PhosphorIcon } from "@phosphor-icons/react";
import { api, on, type Settings, type UiState, type VoiceDictation } from "../lib/api";
import { Button, Card, cx, Icon, Switch } from "./ui";
import { CleanupTab, DictationTab, GeneralTab, StatusCard } from "./SetupTab";
import { VocabularyTab } from "./VoiceTab";
import { InsightsTab } from "./InsightsTab";
import { PrivacyTab } from "./PrivacyTab";
import { SketchTab } from "./SketchTab";
import { Onboarding } from "./Onboarding";

export type Tab = "home" | "dictation" | "cleanup" | "vocabulary" | "sketch" | "privacy" | "general";

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

// A line in the sidebar follows each page marked `endsGroup`.
const PAGES: { id: Tab; label: string; icon: PhosphorIcon; endsGroup?: boolean }[] = [
  { id: "home", label: "Home", icon: House, endsGroup: true },
  { id: "dictation", label: "Dictation", icon: Microphone },
  { id: "cleanup", label: "Cleanup", icon: MagicWand },
  { id: "vocabulary", label: "Vocabulary", icon: BookOpenText },
  { id: "sketch", label: "Sketch", icon: PencilSimpleLine, endsGroup: true },
  { id: "privacy", label: "Privacy", icon: ShieldCheck },
  { id: "general", label: "General", icon: GearSix },
];

export default function App() {
  const { state, save } = useQuill();
  const [tab, setTab] = useState<Tab | null>(null);

  useEffect(() => {
    api.animationsEnabled().then((on) => document.documentElement.classList.toggle("reduced-motion", !on));
  }, []);

  // Open on Dictation until the first dictation, then on Home.
  useEffect(() => {
    if (state && tab === null) setTab(state.hasDictated ? "home" : "dictation");
  }, [state, tab]);

  if (!state || !tab) return <div className="h-screen bg-bg" />;

  const saveVd = (patch: Partial<VoiceDictation>) =>
    save({ ...state.settings, voiceDictation: { ...state.settings.voiceDictation, ...patch } });
  const ctx: Ctx = { state, save, saveVd };
  if (!state.settings.onboarded) return <Onboarding ctx={ctx} />;
  const enabled = state.settings.enabled;
  const page = PAGES.find((p) => p.id === tab)!;
  const pinStatus = enabled && tab !== "dictation" && (state.status.kind !== "ready" || state.status.hotkeyProblem !== null);

  return (
    <div className="flex h-screen bg-bg">
      <Sidebar ctx={ctx} tab={tab} onChange={setTab} />
      <main className="flex min-w-0 flex-1 flex-col overflow-y-auto px-7 pt-5 pb-7">
        <h1 className="mb-4 text-[22px] font-semibold tracking-[-0.01em]">{page.label}</h1>
        {!enabled ? (
          <Card>
            <h3 className="text-[13.5px] font-semibold">Quill is off</h3>
            <p className="mt-0.5 text-[12px] text-text-2">
              The shortcut does nothing and no model stays loaded. Turn Quill on with the switch in the sidebar to dictate again.
            </p>
          </Card>
        ) : (
          <div className="flex flex-col gap-[10px]">
            {pinStatus && <StatusCard ctx={ctx} />}
            <Page tab={tab} ctx={ctx} />
          </div>
        )}
      </main>
    </div>
  );
}

function Page({ tab, ctx }: { tab: Tab; ctx: Ctx }): ReactNode {
  switch (tab) {
    case "home":
      return <InsightsTab ctx={ctx} />;
    case "dictation":
      return <DictationTab ctx={ctx} />;
    case "cleanup":
      return <CleanupTab ctx={ctx} />;
    case "vocabulary":
      return <VocabularyTab ctx={ctx} />;
    case "sketch":
      return <SketchTab ctx={ctx} />;
    case "privacy":
      return <PrivacyTab ctx={ctx} />;
    case "general":
      return <GeneralTab ctx={ctx} />;
  }
}

function Sidebar({ ctx, tab, onChange }: { ctx: Ctx; tab: Tab; onChange: (t: Tab) => void }) {
  const { state, save } = ctx;
  const enabled = state.settings.enabled;
  const ready = enabled && state.status.kind === "ready";
  const statusLine = !enabled ? "Off" : ready ? `Ready, ${state.hotkeyDisplay}` : state.status.title;
  return (
    <nav className="flex w-[216px] shrink-0 flex-col gap-0.5 bg-sidebar px-2.5 pt-4 pb-3" aria-label="Settings pages">
      <div className="mb-4 flex items-center gap-2.5 px-2">
        <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-[8px] bg-accent text-white">
          <Icon.Waveform size={18} />
        </div>
        <div className="min-w-0 flex-1">
          <p className="text-[15px] leading-tight font-semibold">Quill</p>
          <p className={cx("flex items-center gap-1.5 truncate text-[11.5px]", ready ? "text-ok" : enabled ? "text-alert" : "text-text-3")}>
            <span className={cx("h-1.5 w-1.5 shrink-0 rounded-full", ready ? "bg-ok" : enabled ? "bg-alert" : "bg-text-3")} />
            <span className="truncate">{statusLine}</span>
          </p>
        </div>
        <Switch label="Quill" checked={enabled} onChange={(v) => save({ ...state.settings, enabled: v })} />
      </div>
      {PAGES.map(({ id, label, icon: PageIcon, endsGroup }) => {
        const selected = id === tab;
        return (
          <Fragment key={id}>
            <button
              aria-current={selected ? "page" : undefined}
              onClick={() => onChange(id)}
              className={cx(
                "relative flex h-9 items-center gap-3 rounded-[6px] px-2.5 text-left text-[13px] transition-[background-color,color] duration-100",
                selected ? "bg-fill text-text" : "text-text-2 hover:bg-fill hover:text-text",
              )}
            >
              {selected && <span className="absolute top-2.5 bottom-2.5 left-0 w-[3px] rounded-full bg-accent" />}
              <PageIcon size={18} weight={selected ? "fill" : "regular"} />
              {label}
            </button>
            {endsGroup && <div className="mx-2.5 my-1.5 h-px bg-hairline" />}
          </Fragment>
        );
      })}
      <div className="mt-auto">
        {state.update ? <UpdateCard state={state} /> : <p className="px-2.5 text-[11.5px] text-text-3">Quill {state.version}</p>}
      </div>
    </nav>
  );
}

function UpdateCard({ state }: { state: UiState }) {
  const u = state.update!;
  return (
    <div className="flex flex-col gap-1.5 rounded-[8px] bg-accent-soft p-3">
      <p className="text-[12.5px] font-semibold">Quill {u.version} is ready</p>
      <p className={cx("text-[12px]", state.updateError ? "text-alert" : "text-text-2")}>
        {state.updateError ?? `You have ${u.current}. Updating keeps your settings and models.`}
      </p>
      <div className="mt-1 flex items-center gap-1">
        <Button kind="primary" small disabled={state.updating} onClick={() => api.installUpdate().catch(() => {})}>
          {state.updating ? "Updating…" : "Update"}
        </Button>
        <Button kind="plain" small onClick={() => api.openReleases()}>
          What's new
        </Button>
      </div>
    </div>
  );
}
