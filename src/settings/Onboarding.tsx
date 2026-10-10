// First-run setup: how it works → shortcut and mode → pick and download an
// engine (required) → done.
import { useState, type ReactNode } from "react";
import { api, type ActivationMode, type EngineView } from "../lib/api";
import { bytes } from "../lib/format";
import type { Ctx } from "./App";
import { CLEANUP, ShortcutButton } from "./SetupTab";
import { Button, Capsule, cx, Icon, Progress, Segmented, Switch, Warning } from "./ui";

const STEPS = ["Welcome", "Shortcut", "Engine", "Done"] as const;

export function Onboarding({ ctx }: { ctx: Ctx }) {
  const [step, setStep] = useState(0);
  const vd = ctx.state.settings.voiceDictation;
  const engine = ctx.state.engines.find((e) => e.id === vd.engine);
  const engineChosen = !!engine && (engine.ready || engine.busy);
  const finish = () => {
    if (ctx.state.capturingHotkey) api.cancelHotkeyCapture();
    ctx.save({ ...ctx.state.settings, onboarded: true });
  };

  const canContinue = step === 1 ? !ctx.state.capturingHotkey : step === 2 ? engineChosen : true;
  return (
    <div className="flex h-screen flex-col bg-bg">
      <div className="flex items-center gap-2 px-8 pt-6">
        {STEPS.map((label, i) => (
          <div key={label} className="flex items-center gap-2">
            <span
              className={cx(
                "flex h-[22px] w-[22px] items-center justify-center rounded-full text-[11px] font-semibold",
                i < step ? "bg-accent text-white" : i === step ? "bg-accent-soft text-accent ring-1 ring-accent" : "bg-fill text-text-3",
              )}
            >
              {i < step ? <Icon.Check size={12} /> : i + 1}
            </span>
            <span className={cx("text-[12px]", i === step ? "font-semibold" : "text-text-3")}>{label}</span>
            {i < STEPS.length - 1 && <span className="mx-1 h-px w-8 bg-hairline" />}
          </div>
        ))}
      </div>

      <main className="flex-1 overflow-y-auto px-8 pt-6 pb-4">
        {step === 0 && <Welcome />}
        {step === 1 && <ShortcutStep ctx={ctx} />}
        {step === 2 && <EngineStep ctx={ctx} />}
        {step === 3 && <DoneStep ctx={ctx} engine={engine} />}
      </main>

      <footer className="flex items-center justify-between border-t border-hairline px-8 py-4">
        <div>
          {step > 0 && (
            <Button onClick={() => setStep(step - 1)} disabled={ctx.state.capturingHotkey}>
              Back
            </Button>
          )}
        </div>
        <div className="flex items-center gap-3">
          {step === 2 && !engineChosen && <span className="text-[12px] text-text-3">Download an engine to continue.</span>}
          {step < 3 ? (
            <Button kind="primary" disabled={!canContinue} onClick={() => setStep(step + 1)}>
              Continue
            </Button>
          ) : (
            <Button kind="primary" onClick={finish}>
              Finish
            </Button>
          )}
        </div>
      </footer>
    </div>
  );
}

function Title({ children, sub }: { children: ReactNode; sub: ReactNode }) {
  return (
    <div className="mb-5">
      <h1 className="text-[22px] font-semibold">{children}</h1>
      <p className="mt-1 text-[13px] text-text-2">{sub}</p>
    </div>
  );
}

function Welcome() {
  const points: [ReactNode, string, string][] = [
    [<Icon.Mic size={18} />, "Hold a shortcut and speak", "In any app: email, chat, documents, even the terminal."],
    [<Icon.Waveform size={18} />, "Let go, and it's typed for you", "Quill writes what you said, cleans up the ums and false starts, and pastes it where you were typing."],
    [<Icon.Check size={18} />, "Everything stays on this PC", "Speech is recognized on your computer. No audio or text is ever sent anywhere."],
  ];
  return (
    <>
      <div className="mb-5 flex h-14 w-14 items-center justify-center rounded-[14px] bg-accent text-white">
        <Icon.Waveform size={30} />
      </div>
      <Title sub="Dictate anywhere on Windows. Setup takes about a minute.">Welcome to Quill</Title>
      <div className="flex flex-col gap-3">
        {points.map(([icon, title, detail]) => (
          <div key={title} className="flex gap-3 rounded-[12px] border border-card-border bg-card px-4 py-3">
            <span className="mt-[1px] text-accent">{icon}</span>
            <div>
              <p className="text-[13.5px] font-semibold">{title}</p>
              <p className="text-[12.5px] text-text-2">{detail}</p>
            </div>
          </div>
        ))}
      </div>
    </>
  );
}

function ModeCard({ selected, title, detail, onClick }: { selected: boolean; title: string; detail: string; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className={cx(
        "flex-1 rounded-[12px] border px-4 py-3 text-left transition-colors",
        selected ? "border-accent bg-accent-soft" : "border-card-border bg-card hover:bg-fill",
      )}
    >
      <p className="text-[13.5px] font-semibold">{title}</p>
      <p className="mt-0.5 text-[12.5px] text-text-2">{detail}</p>
    </button>
  );
}

function ShortcutStep({ ctx }: { ctx: Ctx }) {
  const vd = ctx.state.settings.voiceDictation;
  const setMode = (activationMode: ActivationMode) => ctx.saveVd({ activationMode });
  return (
    <>
      <Title sub="Pick the keys you'll press to dictate, and how they work.">Your shortcut</Title>
      <div className="flex items-center gap-3 rounded-[12px] border border-card-border bg-card px-4 py-3.5">
        <div className="flex-1">
          <p className="text-[13.5px] font-semibold">Shortcut</p>
          <p className="text-[12.5px] text-text-2">
            {ctx.state.capturingHotkey
              ? "Hold Ctrl, Alt, Shift or Win, then press another key. Esc cancels."
              : "Click to change it. Use at least one of Ctrl, Alt, Shift or Win plus a key."}
          </p>
        </div>
        <ShortcutButton ctx={ctx} />
      </div>
      {ctx.state.captureError && <Warning>{ctx.state.captureError}</Warning>}
      <p className="mt-5 mb-2 text-[13px] font-semibold">How should it work?</p>
      <div className="flex gap-3">
        <ModeCard
          selected={vd.activationMode === "pushToTalk"}
          title="Push to talk"
          detail="Hold the shortcut while you speak. Release to finish."
          onClick={() => setMode("pushToTalk")}
        />
        <ModeCard
          selected={vd.activationMode === "toggle"}
          title="Toggle"
          detail="Press the shortcut once to start and again to finish."
          onClick={() => setMode("toggle")}
        />
      </div>
      <p className="mt-3 text-[12px] text-text-3">You can change both later in Settings. Press Esc twice within a second to cancel dictation.</p>
    </>
  );
}

function engineSize(e: EngineView) {
  return e.assets.filter((a) => !a.optional).reduce((n, a) => n + a.size, 0);
}

function EngineStep({ ctx }: { ctx: Ctx }) {
  const vd = ctx.state.settings.voiceDictation;
  const engines = ctx.state.engines.filter((e) => e.supported);
  return (
    <>
      <Title sub="Quill needs one speech engine. It downloads once and then runs offline. You can add or switch engines later.">
        Choose an engine
      </Title>
      <div className="flex flex-col gap-2">
        {engines.map((e) => (
          <EngineChoice key={e.id} engine={e} selected={e.id === vd.engine} onSelect={() => ctx.saveVd({ engine: e.id })} />
        ))}
      </div>
      <div className="mt-5">
        <p className="mb-2 text-[13px] font-semibold">Cleanup style</p>
        <Segmented
          disabled={!engines.find((e) => e.id === vd.engine)?.hasCleanup}
          value={vd.cleanupLevel}
          onChange={(v) => ctx.saveVd({ cleanupLevel: v })}
          options={CLEANUP.map((c) => ({ value: c.value, label: c.label }))}
        />
        <p className="mt-2 text-[12px] text-text-2">{CLEANUP.find((c) => c.value === vd.cleanupLevel)?.detail}</p>
      </div>
    </>
  );
}

function EngineChoice({ engine: e, selected, onSelect }: { engine: EngineView; selected: boolean; onSelect: () => void }) {
  const required = e.assets.filter((a) => !a.optional);
  const downloading = required.filter((a) => a.state.state === "downloading" || a.state.state === "verifying");
  const failed = required.find((a) => a.state.state === "failed");
  const total = engineSize(e);
  const done = required.reduce((n, a) => {
    if (a.state.state === "downloaded") return n + a.size;
    if (a.state.state === "downloading") return n + a.state.bytes;
    return n;
  }, 0);
  return (
    <div
      onClick={onSelect}
      className={cx(
        "rounded-[12px] border px-4 py-3 transition-colors",
        selected ? "border-accent bg-accent-soft" : "border-card-border bg-card hover:bg-fill",
      )}
    >
      <div className="flex items-start gap-3">
        <span
          className={cx(
            "mt-[2px] flex h-[16px] w-[16px] shrink-0 items-center justify-center rounded-full border",
            selected ? "border-accent bg-accent" : "border-text-3",
          )}
        >
          {selected && <span className="h-[6px] w-[6px] rounded-full bg-white" />}
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-[13.5px] font-semibold">{e.title}</span>
            <Capsule accent={e.badgeAccent}>{e.badge}</Capsule>
            {e.id === "parakeetS1" && <Capsule accent>Recommended</Capsule>}
            <span className="text-[12px] text-text-3">{bytes(total)}</span>
          </div>
          <p className="mt-0.5 text-[12.5px] text-text-2">{e.summary}</p>
        </div>
        {e.ready && (
          <span className="flex items-center gap-1 text-[12px] text-ok">
            <Icon.Check size={14} /> Downloaded
          </span>
        )}
      </div>
      {selected && !e.ready && (
        <div className="mt-3 ml-7" onClick={(ev) => ev.stopPropagation()}>
          {downloading.length > 0 ? (
            <div className="flex items-center gap-3">
              <div className="flex-1">
                <p className="mb-1 text-[12px] text-text-2">
                  {downloading.some((a) => a.state.state === "verifying")
                    ? "Verifying…"
                    : `Downloading ${bytes(done)} of ${bytes(total)} (${Math.floor((done / total) * 100)}%). You can continue while it finishes.`}
                </p>
                <Progress fraction={done / total} />
              </div>
              <Button small onClick={() => api.cancelDownload(e.id)}>
                Cancel
              </Button>
            </div>
          ) : (
            <>
              {required.map((a) => (
                <p key={a.id} className="text-[11.5px] text-text-3">
                  {a.title}: {a.license}. {a.licenseNote}
                </p>
              ))}
              {failed && failed.state.state === "failed" && <Warning>{failed.state.message}</Warning>}
              <Button kind="primary" className="mt-2" onClick={() => api.downloadEngine(e.id)}>
                {failed ? "Retry download" : `Download ${bytes(total)}`}
              </Button>
            </>
          )}
        </div>
      )}
    </div>
  );
}

function DoneStep({ ctx, engine }: { ctx: Ctx; engine: EngineView | undefined }) {
  const { status, hotkeyDisplay, settings } = ctx.state;
  const hold = settings.voiceDictation.activationMode === "pushToTalk";
  const ready = engine?.ready ?? false;
  return (
    <>
      <Title sub={ready ? "Quill is ready to use." : `Quill will be ready as soon as ${engine?.title ?? "your engine"} finishes downloading.`}>
        You're all set
      </Title>
      <div className="rounded-[12px] border border-card-border bg-card px-4 py-4">
        <p className="mb-3 text-[13px] font-semibold">Try it now</p>
        <ol className="flex flex-col gap-2 text-[13px] text-text-2">
          <li>1. Click into any text box, like a chat, an email, or Notepad.</li>
          <li>
            2. {hold ? "Hold" : "Press"} <kbd className="rounded-[5px] border border-card-border bg-fill px-1.5 py-0.5 font-mono text-[12px] text-text">{hotkeyDisplay}</kbd>{" "}
            and speak.
          </li>
          <li>3. {hold ? "Let go" : "Press it again"} and your words appear.</li>
        </ol>
        {!ready && engine && engine.busy && (
          <div className="mt-4">
            <Progress
              fraction={
                engine.assets.filter((a) => !a.optional).reduce((n, a) => n + (a.state.state === "downloaded" ? a.size : a.state.state === "downloading" ? a.state.bytes : 0), 0) /
                Math.max(1, engineSize(engine))
              }
            />
          </div>
        )}
      </div>
      {status.kind === "micDenied" && (
        <div className="mt-3 flex items-center gap-3 rounded-[12px] border border-card-border bg-card px-4 py-3">
          <p className="flex-1 text-[12.5px] text-alert">Windows is blocking microphone access for desktop apps.</p>
          <Button onClick={() => api.openMicrophoneSettings()}>Open Windows Settings</Button>
        </div>
      )}
      {status.kind === "micUnavailable" && <Warning>No microphone found. Connect one before dictating.</Warning>}
      <div className="mt-3 flex items-center gap-3 rounded-[12px] border border-card-border bg-card px-4 py-3">
        <div className="flex-1">
          <p className="text-[13px] font-semibold">Start Quill when I sign in</p>
          <p className="text-[12px] text-text-2">Quill waits quietly in the tray (bottom right, near the clock). Click its icon for Settings.</p>
        </div>
        <Switch label="Start at login" checked={settings.launchAtLogin} onChange={(v) => ctx.save({ ...settings, launchAtLogin: v })} />
      </div>
    </>
  );
}
