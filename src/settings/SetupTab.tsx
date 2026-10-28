import { useState } from "react";
import { api, LINKS, type AppStyle, type AssetView, type CleanupLevel, type EngineView } from "../lib/api";
import { bytes } from "../lib/format";
import type { Ctx } from "./App";
import { AppIcon, AppScope, scopeName, useKnownApps } from "./AppScope";
import {
  AppWindow,
  ArrowsClockwise,
  Browsers,
  Command,
  Cpu,
  CursorText,
  HandTap,
  Hash,
  KeyReturn,
  Link,
  MagicWand,
  Power,
  Smiley,
  Subtitles,
  TerminalWindow,
  TextAa,
  Translate,
} from "@phosphor-icons/react";
import {
  Button,
  Capsule,
  Card,
  CardHeader,
  cx,
  Divider,
  Dot,
  Icon,
  IconButton,
  Progress,
  Section,
  Segmented,
  Sheet,
  Spinner,
  Switch,
  useCopied,
  Warning,
} from "./ui";

const ACTIVATION_DETAIL = {
  pushToTalk: "Hold the shortcut while you speak. Release to finish.",
  toggle: "Press the shortcut once to start and again to finish.",
};

export const CLEANUP: { value: CleanupLevel; label: string; detail: string }[] = [
  {
    value: "verbatim",
    label: "Verbatim",
    detail:
      "Keeps every word you said. Punctuation and capitalization only, straight from the recognizer. Lists keep the numbers you spoke.",
  },
  {
    value: "light",
    label: "Casual",
    detail: "Writes like a text message: lowercase, light punctuation, your wording left as spoken. Corrections are still resolved and numbers written as figures.",
  },
  {
    value: "full",
    label: "Polished",
    detail:
      "Removes fillers and false starts, writes numbers, dates, and emails properly, and punctuates as sentences. The default.",
  },
  {
    value: "compose",
    label: "Compose",
    detail: "Polished, and items you count off or list become a numbered or bulleted list.",
  },
];

const EXAMPLES: [string, string][] = [
  ["git commit dash m quote fix the build quote", 'git commit -m "fix the build"'],
  ["npm install dash dash save dash dev", "npm install --save-dev"],
  ["cd tilde slash projects", "cd ~/projects"],
];

export function SetupTab({ ctx }: { ctx: Ctx }) {
  return (
    <>
      <StatusCard ctx={ctx} />
      <Section title="Dictation">
        <ActivationCard ctx={ctx} />
      </Section>
      <Section title="Engine and cleanup">
        <EngineCard ctx={ctx} />
        <CleanupCard ctx={ctx} />
      </Section>
      <Section title="General">
        <Card>
          <CardHeader
            icon={<Power size={19} />}
            title="Start at login"
            detail="Quill waits in the tray, ready for your shortcut."
            right={<Switch label="Start at login" checked={ctx.state.settings.launchAtLogin} onChange={(v) => ctx.save({ ...ctx.state.settings, launchAtLogin: v })} />}
          />
        </Card>
        <UpdatesCard ctx={ctx} />
      </Section>
    </>
  );
}

/** The "Typing into apps" page. */
export function TypingTab({ ctx }: { ctx: Ctx }) {
  return (
    <>
      <FormattingCard ctx={ctx} />
      <Section title="Launchers">
        <LaunchersCard />
      </Section>
    </>
  );
}

export function StatusCard({ ctx }: { ctx: Ctx }) {
  const { status } = ctx.state;
  let right = null;
  if (status.kind === "ready") {
    right = status.engineLoaded ? (
      <span className="flex items-center gap-1.5 text-[12px] text-text-2">
        <Icon.Check size={14} /> Engine loaded
      </span>
    ) : status.engineLoading ? (
      <span className="flex max-w-[260px] items-center gap-2 text-[12px] text-text-2">
        <Spinner /> Loading the engine. The first load takes longest.
      </span>
    ) : (
      <Button onClick={() => api.loadEngine()}>Load engine</Button>
    );
  } else if (status.kind === "micDenied") {
    right = <Button onClick={() => api.openMicrophoneSettings()}>Open Windows Settings</Button>;
  }
  return (
    <Card>
      <div className="flex items-center gap-3">
        <Dot color={status.dot} />
        <div className="min-w-0 flex-1">
          <h3 className="text-[13.5px] font-semibold">{status.title}</h3>
          <p className="text-[12px] text-text-2">{status.detail}</p>
        </div>
        {right}
      </div>
      {status.hotkeyProblem && <Warning>{status.hotkeyProblem}</Warning>}
      {status.kind === "ready" && status.warmError && !status.engineLoading && (
        <Warning>The engine failed to load: {status.warmError}</Warning>
      )}
      {ctx.state.dictation.phase === "failed" && ctx.state.dictation.errorTitle && (
        <Warning>
          {ctx.state.dictation.errorTitle}. {ctx.state.dictation.errorDetail}
        </Warning>
      )}
    </Card>
  );
}

function ActivationCard({ ctx }: { ctx: Ctx }) {
  const vd = ctx.state.settings.voiceDictation;
  const { capturingHotkey, captureError } = ctx.state;
  return (
    <Card>
      <CardHeader
        icon={<HandTap size={19} />}
        title="Activation"
        detail={ACTIVATION_DETAIL[vd.activationMode]}
        right={
          <Segmented
            width={240}
            value={vd.activationMode}
            onChange={(v) => ctx.saveVd({ activationMode: v })}
            options={[
              { value: "pushToTalk", label: "Push to talk" },
              { value: "toggle", label: "Toggle" },
            ]}
          />
        }
      />
      <Divider />
      <CardHeader
        icon={<Command size={19} />}
        title="Shortcut"
        detail={capturingHotkey ? "Hold Ctrl, Alt, Shift or Win, then press another key. Esc cancels." : "Use Ctrl, Alt, Shift or Win plus another key, such as Ctrl+Shift+Space."}
        right={<ShortcutButton ctx={ctx} />}
      />
      {captureError && <Warning>{captureError}</Warning>}
      <Divider />
      <CardHeader
        icon={<Subtitles size={19} />}
        title="Live preview"
        detail="Shows the words Quill hears inside the overlay while you speak. The pasted text still comes from the whole recording. Uses more CPU while you record."
        right={<Switch label="Live preview" checked={vd.livePreview} onChange={(v) => ctx.saveVd({ livePreview: v })} />}
      />
    </Card>
  );
}

/// The shortcut capture button, with Reset when it isn't the default.
/// `sketch` picks the Sketch shortcut instead of dictation's.
export function ShortcutButton({ ctx, sketch = false }: { ctx: Ctx; sketch?: boolean }) {
  const capturingHotkey = sketch ? ctx.state.capturingSketchHotkey : ctx.state.capturingHotkey;
  const hotkeyDisplay = sketch ? ctx.state.sketchHotkeyDisplay : ctx.state.hotkeyDisplay;
  const hotkeyIsDefault = sketch ? ctx.state.sketchHotkeyIsDefault : ctx.state.hotkeyIsDefault;
  return (
    <>
      {!hotkeyIsDefault && !capturingHotkey && (
        <Button kind="plain" onClick={() => api.resetHotkey(sketch)}>
          Reset
        </Button>
      )}
      <button
        aria-label={capturingHotkey ? "Recording a shortcut. Press Esc to cancel." : `Shortcut ${hotkeyDisplay}. Change shortcut`}
        onClick={() => (capturingHotkey ? api.cancelHotkeyCapture() : api.beginHotkeyCapture(sketch))}
        className={cx(
          "flex h-[30px] min-w-[120px] items-center justify-center gap-1 rounded-[7px] px-1 transition-[background-color] duration-100",
          capturingHotkey ? "bg-accent-soft px-3 text-[12.5px] text-accent shadow-[0_0_0_1px_var(--accent)]" : "hover:bg-fill",
        )}
      >
        {capturingHotkey
          ? "Press a shortcut…"
          : hotkeyDisplay.split("+").map((key) => (
              <kbd
                key={key}
                className="inline-flex h-[26px] min-w-[26px] items-center justify-center rounded-[6px] bg-fill-strong px-2 font-sans text-[12.5px] font-semibold shadow-[inset_0_-2px_0_rgb(0_0_0/0.35),0_0_0_1px_var(--card-border)]"
              >
                {key}
              </kbd>
            ))}
      </button>
    </>
  );
}

function assetStateLine(a: AssetView) {
  const s = a.state;
  switch (s.state) {
    case "downloading":
      return (
        <div className="mt-1 flex flex-col gap-1">
          <span className="text-[11.5px] text-text-2">
            Downloading {bytes(s.bytes)} of {bytes(s.total)} ({Math.floor(s.fraction * 100)}%)
          </span>
          <Progress fraction={s.fraction} />
        </div>
      );
    case "verifying":
      return <span className="text-[11.5px] text-text-2">Verifying checksums…</span>;
    case "downloaded":
      return (
        <span className="text-[11.5px] text-text-2">
          Ready, {a.license}, runs on {a.runtime}
        </span>
      );
    case "failed":
      return <span className="text-[11.5px] text-alert">{s.message}</span>;
    case "updateAvailable":
      return <span className="text-[11.5px] text-text-2">An updated version is available.</span>;
    case "unavailable":
      return <span className="text-[11.5px] text-text-3">Not available on Windows yet</span>;
    default:
      return <span className="text-[11.5px] text-text-3">Not downloaded · {a.license}</span>;
  }
}

function EngineCard({ ctx }: { ctx: Ctx }) {
  const vd = ctx.state.settings.voiceDictation;
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<{ engine?: EngineView; assets: AssetView[] } | null>(null);
  const selected = ctx.state.engines.find((e) => e.id === vd.engine);
  const run = (p: Promise<unknown>) => p.catch((e) => setError(String(e)));

  const startDownload = (engine: EngineView) => {
    const assets = engine.assets.filter((a) => !a.optional && a.state.state !== "downloaded");
    setConfirm({ engine, assets });
  };

  return (
    <Card>
      <button
        type="button"
        aria-expanded={open}
        aria-controls="engine-options"
        onClick={() => setOpen(!open)}
        className="flex w-full items-center gap-3 text-left"
      >
        <span className="flex h-[20px] w-[20px] shrink-0 items-center justify-center text-text-2"><Cpu size={19} /></span>
        <span className="min-w-0 flex-1">
          <span className="block text-[13.5px] font-semibold">Engine</span>
          <span className="block mt-0.5 text-[12px] text-text-2">{selected?.title ?? "Choose an engine"}</span>
        </span>
        {selected && <Capsule accent={selected.ready}>{selected.ready ? "Ready" : selected.busy ? "Preparing…" : "Download needed"}</Capsule>}
        <span className="text-text-3"><Icon.Chevron open={open} /></span>
      </button>
      {error && <Warning>{error}</Warning>}
      <div id="engine-options" hidden={!open} className="ml-8">
        <p className="mt-3 text-[12px] text-text-2">Models download only when you choose them and are verified against pinned checksums.</p>
        {ctx.state.engines.map((e) => (
          <div key={e.id}>
            <Divider />
            <EngineRow
              engine={e}
              selected={e.id === vd.engine}
              onSelect={() => ctx.saveVd({ engine: e.id })}
              onDownload={() => startDownload(e)}
              onCancel={() => run(api.cancelDownload(e.id))}
              onRemove={() => run(api.removeEngine(e.id))}
              onAssetDownload={(a) => setConfirm({ assets: [a] })}
              onAssetRemove={(a) => run(api.removeAsset(a.id))}
              onAssetRetry={(a) => run(api.downloadAsset(a.id))}
            />
          </div>
        ))}
        {selected?.showsLanguagePicker && (
          <>
            <Divider />
            <LanguageRow ctx={ctx} engine={selected} />
          </>
        )}
      </div>
      {confirm && (
        <Sheet
          title="Download models"
          onClose={() => setConfirm(null)}
          footer={
            <>
              <Button onClick={() => setConfirm(null)}>Cancel</Button>
              <Button
                kind="primary"
                onClick={() => {
                  const c = confirm;
                  setConfirm(null);
                  setError(null);
                  if (c.engine) run(api.downloadEngine(c.engine.id));
                  else c.assets.forEach((a) => run(api.downloadAsset(a.id)));
                }}
              >
                Download {bytes(confirm.assets.reduce((n, a) => n + a.size, 0))}
              </Button>
            </>
          }
        >
          <p className="mb-3 text-[12px] text-text-2">
            These files come from Hugging Face and are checked against pinned checksums. Their licenses:
          </p>
          {confirm.assets.map((a) => (
            <div key={a.id} className="mb-2 rounded-[8px] bg-fill px-3 py-2">
              <div className="flex items-center gap-2">
                <span className="font-semibold">{a.title}</span>
                <span className="text-[11.5px] text-text-3">{bytes(a.size)}</span>
                <span className="ml-auto">
                  <Capsule>{a.license}</Capsule>
                </span>
              </div>
              <p className="mt-1 text-[12px] text-text-2">{a.licenseNote}</p>
            </div>
          ))}
        </Sheet>
      )}
    </Card>
  );
}

function EngineRow(props: {
  engine: EngineView;
  selected: boolean;
  onSelect: () => void;
  onDownload: () => void;
  onCancel: () => void;
  onRemove: () => void;
  onAssetDownload: (a: AssetView) => void;
  onAssetRemove: (a: AssetView) => void;
  onAssetRetry: (a: AssetView) => void;
}) {
  const { engine: e, selected } = props;
  const required = e.assets.filter((a) => !a.optional);
  const downloading = required.some((a) => a.state.state === "downloading" || a.state.state === "verifying");
  const failed = required.find((a) => a.state.state === "failed");
  let actions;
  if (!e.supported) {
    actions = <span className="text-[11.5px] text-text-3">Not in this build</span>;
  } else if (downloading) {
    actions = <Button onClick={props.onCancel}>Cancel</Button>;
  } else if (e.ready && !e.updateAvailable) {
    actions = (
      <Button kind="destructive" disabled={e.busy} onClick={props.onRemove}>
        Remove engine
      </Button>
    );
  } else {
    actions = (
      <>
        {e.hasData && (
          <Button kind="destructive" onClick={props.onRemove}>
            Remove engine
          </Button>
        )}
        <Button kind="primary" onClick={props.onDownload} disabled={required.some((a) => !a.available)}>
          {e.updateAvailable ? "Update" : failed ? "Retry" : "Download"}
        </Button>
      </>
    );
  }
  return (
    <div>
      <div className="flex items-start gap-3">
        <button
          role="radio"
          aria-checked={selected}
          aria-label={e.title}
          onClick={props.onSelect}
          className={cx(
            "mt-[2px] flex h-[16px] w-[16px] shrink-0 items-center justify-center rounded-full border",
            selected ? "border-accent bg-accent" : "border-text-3",
          )}
        >
          {selected && <span className="h-[6px] w-[6px] rounded-full bg-white" />}
        </button>
        <div className="min-w-0 flex-1" onClick={props.onSelect}>
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-[13px] font-semibold">{e.title}</span>
            <Capsule accent={e.badgeAccent}>{e.badge}</Capsule>
          </div>
          <p className="mt-0.5 text-[12px] leading-[1.45] text-text-2">{e.summary}</p>
          <p className="mt-0.5 text-[11.5px] text-text-3">{e.languageNote}</p>
        </div>
        <div className="flex shrink-0 items-center gap-2">{actions}</div>
      </div>
      <div className="mt-2 ml-7 flex flex-col gap-2">
        {e.assets.map((a) => (
          <div key={a.id} className="flex items-start gap-2">
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2 text-[12px]">
                <span className="font-medium">{a.title}</span>
                {a.size > 0 && <span className="text-text-3">{bytes(a.size)}</span>}
                {a.optional && <Capsule>Optional</Capsule>}
              </div>
              {assetStateLine(a)}
            </div>
            {a.state.state === "failed" && !a.optional && required.length > 1 && (
              <Button small onClick={() => props.onAssetRetry(a)}>
                Retry
              </Button>
            )}
            {a.optional && a.available && (
              <>
                {a.state.state === "downloading" ? (
                  <Button small onClick={() => api.cancelAsset(a.id)}>
                    Cancel
                  </Button>
                ) : a.state.state === "downloaded" ? (
                  <Button small kind="destructive" onClick={() => props.onAssetRemove(a)}>
                    Remove
                  </Button>
                ) : (
                  <Button small onClick={() => props.onAssetDownload(a)}>
                    Download
                  </Button>
                )}
              </>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

function LanguageRow({ ctx, engine }: { ctx: Ctx; engine: EngineView }) {
  const vd = ctx.state.settings.voiceDictation;
  const auto = engine.id === "cohereTranscribe" ? `Automatic (${ctx.state.cohereAutoLanguage})` : "Automatic";
  return (
    <CardHeader
      icon={<Translate size={19} />}
        title="Language"
      detail={engine.languageNote}
      right={
        <select
          value={vd.language}
          onChange={(e) => ctx.saveVd({ language: e.target.value })}
          className="h-[28px] w-[190px] rounded-[6px] border border-card-border bg-fill px-2 text-[12.5px] outline-none"
        >
          <option value="auto">{auto}</option>
          <option disabled>──────────</option>
          {ctx.state.languages.map(([code, name]) => (
            <option key={code} value={code}>
              {name}
            </option>
          ))}
        </select>
      }
    />
  );
}

function CleanupCard({ ctx }: { ctx: Ctx }) {
  const vd = ctx.state.settings.voiceDictation;
  const engine = ctx.state.engines.find((e) => e.id === vd.engine);
  const hasCleanup = engine?.hasCleanup ?? false;
  const level = CLEANUP.find((c) => c.value === vd.cleanupLevel) ?? CLEANUP[2]!;
  return (
    <Card dim={!hasCleanup}>
      <CardHeader
        icon={<MagicWand size={19} />}
        title="Cleanup"
        detail={
          hasCleanup
            ? "Choose how S1-mini cleans up your English. Other languages are written as heard. Cleanup guards protect your wording, numbers, and negations."
            : `${engine?.title ?? "This engine"} has no cleanup stage; transcripts are delivered as recognized. Choose Parakeet + S1-mini to adjust this.`
        }
      />
      <div className="mt-3 ml-8">
        <Segmented
          disabled={!hasCleanup}
          value={vd.cleanupLevel}
          onChange={(v) => ctx.saveVd({ cleanupLevel: v })}
          options={CLEANUP.map((c) => ({ value: c.value, label: c.label }))}
        />
        <p className="mt-2 text-[12px] text-text-2">{level.detail}</p>
      </div>
      <Divider />
      <AppStyles ctx={ctx} disabled={!hasCleanup} />
      <Divider />
      <CardHeader
        icon={<TextAa size={19} />}
        title="Formal wording"
        detail={'Polished and Compose write contractions out, so "I don\'t think so" becomes "I do not think so".'}
        right={<Switch label="Formal wording" disabled={!hasCleanup} checked={vd.formalRegister} onChange={(v) => ctx.saveVd({ formalRegister: v })} />}
      />
    </Card>
  );
}

const STYLE_SELECT =
  "h-[28px] rounded-[6px] border border-card-border bg-fill px-2 text-[12.5px] outline-none focus-visible:border-accent disabled:opacity-50";

/** Cleanup styles for chosen apps; every other app uses the main style. */
function AppStyles({ ctx, disabled }: { ctx: Ctx; disabled: boolean }) {
  const vd = ctx.state.settings.voiceDictation;
  const apps = useKnownApps();
  const [adding, setAdding] = useState(false);
  const setStyles = (appStyles: AppStyle[]) => ctx.saveVd({ appStyles });
  return (
    <div>
      <CardHeader
        icon={<AppWindow size={19} />}
        title="Styles for specific apps"
        detail="Pick a different style for the apps you choose, such as Casual in Discord. Other apps use the style above."
        right={
          <Button small disabled={disabled} onClick={() => setAdding(true)}>
            <Icon.Plus /> Add app
          </Button>
        }
      />
      {vd.appStyles.length > 0 && (
        <div className="mt-2 ml-8 flex flex-col">
          {vd.appStyles.map((s) => {
            const app = apps.find((a) => a.exe.toLowerCase() === s.appExe.toLowerCase());
            return (
              <div key={s.appExe} className="flex items-center gap-2 border-t border-hairline py-1.5 first:border-t-0">
                <AppIcon app={app ?? { exe: s.appExe, name: s.appExe }} />
                <span className="min-w-0 flex-1 truncate text-[12.5px]">{scopeName(s.appExe, apps)}</span>
                <select
                  aria-label={`Style in ${scopeName(s.appExe, apps)}`}
                  disabled={disabled}
                  value={s.level}
                  onChange={(e) => setStyles(vd.appStyles.map((x) => (x.appExe === s.appExe ? { ...x, level: e.target.value as CleanupLevel } : x)))}
                  className={cx(STYLE_SELECT, "w-[120px]")}
                >
                  {CLEANUP.map((c) => (
                    <option key={c.value} value={c.value}>
                      {c.label}
                    </option>
                  ))}
                </select>
                <IconButton label={`Remove ${scopeName(s.appExe, apps)}`} onClick={() => setStyles(vd.appStyles.filter((x) => x.appExe !== s.appExe))}>
                  <Icon.Trash />
                </IconButton>
              </div>
            );
          })}
        </div>
      )}
      {adding && (
        <AppStyleSheet
          taken={vd.appStyles.map((s) => s.appExe.toLowerCase())}
          initial={vd.cleanupLevel === "light" ? "full" : "light"}
          onClose={() => setAdding(false)}
          onSave={(style) => {
            setStyles([...vd.appStyles, style]);
            setAdding(false);
          }}
        />
      )}
    </div>
  );
}

function AppStyleSheet({ taken, initial, onClose, onSave }: {
  taken: string[];
  initial: CleanupLevel;
  onClose: () => void;
  onSave: (style: AppStyle) => void;
}) {
  const [appExe, setAppExe] = useState<string | null>(null);
  const [level, setLevel] = useState<CleanupLevel>(initial);
  const valid = appExe != null && /^[^\\/]+\.exe$/i.test(appExe) && !taken.includes(appExe.toLowerCase());
  return (
    <Sheet
      title="Style for an app"
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button kind="primary" disabled={!valid} onClick={() => appExe && onSave({ appExe, level })}>
            Save
          </Button>
        </>
      }
    >
      <AppScope value={appExe} onChange={setAppExe} anyApp={false} />
      <span className="mb-1 block text-[12px] text-text-2">Style</span>
      <Segmented value={level} onChange={setLevel} options={CLEANUP.map((c) => ({ value: c.value, label: c.label }))} />
      <p className="mt-2 text-[12px] text-text-2">{CLEANUP.find((c) => c.value === level)?.detail}</p>
      {appExe && taken.includes(appExe.toLowerCase()) && <Warning>This app already has a style. Change it in the list.</Warning>}
    </Sheet>
  );
}

function FormattingCard({ ctx }: { ctx: Ctx }) {
  const vd = ctx.state.settings.voiceDictation;
  return (
    <Card>
      <CardHeader
        icon={<Browsers size={19} />}
        title="Adapt to the app"
        detail="In a terminal, or a terminal pane in VS Code or Cursor, spoken commands are written as commands. In Slack, Teams, Discord and other chat apps, a one-line message drops its full stop. In Outlook and other mail apps, the greeting and sign-off get their own lines."
        right={<Switch label="Adapt to the app" checked={vd.adaptsToApp} onChange={(v) => ctx.saveVd({ adaptsToApp: v })} />}
      />
      {vd.adaptsToApp && (
        <div className="mt-3 ml-8 flex flex-col gap-1.5 rounded-[8px] bg-accent-soft px-3 py-2.5">
          {EXAMPLES.map(([said, typed]) => (
            <div key={said} className="flex flex-wrap items-baseline gap-x-2 text-[12px]">
              <span className="text-text-2">"{said}"</span>
              <span className="text-text-3">→</span>
              <code className="font-mono text-[12px]">{typed}</code>
            </div>
          ))}
        </div>
      )}
      <Divider />
      <CardHeader
        icon={<CursorText size={19} />}
        title="Match the text around the cursor"
        detail="Dictating into the middle of a sentence starts the text in lowercase, adds a missing space, and drops a full stop the sentence doesn't need. Names and your vocabulary keep their capitals."
        right={<Switch label="Match the text around the cursor" checked={vd.fitsToCursor} onChange={(v) => ctx.saveVd({ fitsToCursor: v })} />}
      />
      <Divider />
      <CardHeader
        icon={<Smiley size={19} />}
        title="Spoken line breaks and emoji"
        detail={'Say "new line" or "new paragraph" to break the text, or an emoji name followed by "emoji", such as "thumbs up emoji". Words inside a sentence, as in "add a new line", are typed as words. Off in terminals.'}
        right={<Switch label="Spoken line breaks and emoji" checked={vd.layoutCues} onChange={(v) => ctx.saveVd({ layoutCues: v })} />}
      />
      <Divider />
      <CardHeader
        icon={<KeyReturn size={19} />}
        title={'Say "press enter" to send'}
        detail={'End a dictation with "press enter" and Quill presses Enter after the paste. Handy in chat apps and coding tools.'}
        right={<Switch label="Press Enter on command" checked={vd.spokenEnter} onChange={(v) => ctx.saveVd({ spokenEnter: v })} />}
      />
      <Divider />
      <CardHeader
        icon={<TerminalWindow size={19} />}
        title="Split long pastes in terminals"
        detail="Claude Code, Codex and similar tools fold a long paste into a placeholder. Quill pastes text over 600 characters or with more than 2 line breaks in pieces, so you can read it before you send it."
        right={<Switch label="Split long pastes in terminals" checked={vd.splitsTerminalPastes} onChange={(v) => ctx.saveVd({ splitsTerminalPastes: v })} />}
      />
      <Divider />
      <CardHeader
        icon={<Hash size={19} />}
        title="Write numbers as figures"
        detail={
          ctx.state.itnAvailable
            ? '"Twenty five dollars on March third" becomes "$25 on March 3". Applies to English when S1-mini isn\'t already doing it: in Verbatim, with the Whisper and Cohere engines, and in terminals.'
            : "The number formatter isn't in this build."
        }
        right={
          <Switch
            label="Write numbers as figures"
            disabled={!ctx.state.itnAvailable}
            checked={vd.writesNumbers && ctx.state.itnAvailable}
            onChange={(v) => ctx.saveVd({ writesNumbers: v })}
          />
        }
      />
    </Card>
  );
}

function LinkRow({ link }: { link: string }) {
  const [copied, setCopied] = useCopied();
  return (
    <div className="flex items-center gap-2 rounded-[6px] bg-fill px-2.5 py-1">
      <code className="selectable flex-1 font-mono text-[12px]">{link}</code>
      <IconButton
        label={copied ? "Copied" : `Copy ${link}`}
        onClick={() => {
          api.copyText(link);
          setCopied();
        }}
      >
        {copied ? <Icon.Check size={14} /> : <Icon.Copy />}
      </IconButton>
    </div>
  );
}

function LaunchersCard() {
  return (
    <Card>
      <CardHeader
        icon={<Link size={19} />}
        title="Start from anywhere"
        detail="Stream Deck, AutoHotkey, PowerToys or any launcher can open these links or run Quill with --start, --stop, --toggle or --cancel."
      />
      <div className="mt-3 grid grid-cols-2 gap-1.5">
        {LINKS.map((l) => (
          <LinkRow key={l} link={l} />
        ))}
      </div>
    </Card>
  );
}

const LICENSES: [string, string, string][] = [
  ["whisper.cpp, llama.cpp", "MIT", "Notices kept."],
  ["ONNX Runtime, sherpa-onnx", "MIT, Apache 2.0", "Notices kept."],
  ["text-processing-rs", "Apache 2.0", "LICENSE kept."],
  ["Parakeet TDT 0.6B v3", "CC BY 4.0", "By NVIDIA."],
  ["S1-mini by Superwhisper", "Apache 2.0", "Derived from Qwen3-0.6B. LICENSE and NOTICE are kept with the model."],
  ["Whisper tiny.en, base.en, large-v3-turbo", "MIT", "By OpenAI. Notice kept."],
  ["Cohere Transcribe 03-2026", "Apache 2.0", "By Cohere Labs. README kept with the weights."],
  ["Silero VAD", "MIT", "By Silero. Built into Quill to skip silence."],
];

export function LicensesCard() {
  const [open, setOpen] = useState(false);
  return (
    <Card>
      <div className="flex items-center gap-2">
        <button aria-label={open ? "Collapse" : "Expand"} onClick={() => setOpen(!open)} className="text-text-2">
          <Icon.Chevron open={open} />
        </button>
        <div className="flex-1">
          <CardHeader title="Licenses" detail="Quill ships the runtimes. You download the model weights yourself, on demand." />
        </div>
      </div>
      {open && (
        <div className="mt-3 flex flex-col">
          {LICENSES.map(([name, license, note]) => (
            <div key={name} className="flex items-baseline gap-2 border-t border-hairline py-1.5 text-[12px] first:border-t-0">
              <span className="w-[230px] shrink-0 font-medium">{name}</span>
              <span className="w-[110px] shrink-0 text-text-2">{license}</span>
              <span className="text-text-3">{note}</span>
            </div>
          ))}
        </div>
      )}
    </Card>
  );
}

function UpdatesCard({ ctx }: { ctx: Ctx }) {
  const { version, update, updating } = ctx.state;
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const check = () => {
    setChecking(true);
    setResult(null);
    api
      .checkForUpdates()
      .then((u) => setResult(u ? null : "You have the latest version."))
      .catch((e) => setResult(String(e)))
      .finally(() => setChecking(false));
  };
  return (
    <Card>
      <CardHeader
        icon={<ArrowsClockwise size={19} />}
        title="Updates"
        detail={
          update
            ? `Quill ${update.version} is available. You have ${version}.`
            : result ?? `You have Quill ${version}. Quill checks for updates daily.`
        }
        right={
          update ? (
            <Button kind="primary" disabled={updating} onClick={() => api.installUpdate().catch(() => {})}>
              {updating ? "Updating…" : "Update now"}
            </Button>
          ) : (
            <>
              <Button kind="plain" onClick={() => api.openReleases()}>
                Releases page
              </Button>
              <Button disabled={checking} onClick={check}>
                {checking ? "Checking…" : "Check for updates"}
              </Button>
            </>
          )
        }
      />
    </Card>
  );
}
