import { useEffect, useState } from "react";
import { api, LINKS, on, type AssetView, type CleanupLevel, type EngineView, type HistoryEntry } from "../lib/api";
import { bytes, timeAgo } from "../lib/format";
import type { Ctx } from "./App";
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

const CLEANUP: { value: CleanupLevel; label: string; detail: string }[] = [
  {
    value: "verbatim",
    label: "Verbatim",
    detail:
      "Keeps every word you said. Punctuation and capitalization only, straight from the recognizer. Lists keep the numbers you spoke.",
  },
  {
    value: "light",
    label: "Light",
    detail: "Removes fillers and false starts and writes numbers properly, but keeps your contractions and casual phrasing.",
  },
  {
    value: "full",
    label: "Full",
    detail:
      "Removes fillers and false starts, writes numbers, dates, and emails properly, and tidies the register. The default.",
  },
];

const EXAMPLES: [string, string][] = [
  ["git commit dash m quote fix the build quote", 'git commit -m "fix the build"'],
  ["npm install dash dash save dash dev", "npm install --save-dev"],
  ["cd tilde slash projects", "cd ~/projects"],
];

export function SetupTab({ ctx }: { ctx: Ctx }) {
  return (
    <div className="flex flex-col gap-[10px]">
      <StatusCard ctx={ctx} />
      <ActivationCard ctx={ctx} />
      <EngineCard ctx={ctx} />
      <CleanupCard ctx={ctx} />
      <FormattingCard ctx={ctx} />
      <LaunchersCard />
      <HistoryCard ctx={ctx} />
      <Card>
        <CardHeader title="Privacy" />
        <p className="mt-1.5 text-[12px] text-text-2">Audio is processed in memory and discarded when the transcript is ready.</p>
        <p className="mt-1 text-[12px] text-text-2">Recognition and cleanup run on this PC. No audio or text is sent anywhere.</p>
      </Card>
      <Card>
        <CardHeader title="Start at login" detail="Quill waits in the tray, ready for your shortcut." right={
          <Switch label="Start at login" checked={ctx.state.settings.launchAtLogin} onChange={(v) => ctx.save({ ...ctx.state.settings, launchAtLogin: v })} />
        } />
      </Card>
      <LicensesCard />
      <p className="px-1 pt-1 text-[11px] text-text-3">Quill {ctx.state.version}</p>
    </div>
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
        <Spinner /> Loading engine… the first load can take a while.
      </span>
    ) : (
      <Button onClick={() => api.loadEngine()}>Load Engine</Button>
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
  const { capturingHotkey, hotkeyDisplay, hotkeyIsDefault, captureError } = ctx.state;
  return (
    <Card>
      <CardHeader
        title="Activation"
        detail="Push-to-talk records while the shortcut is held. Toggle starts on one press and stops on the next."
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
      <p className="mt-2 text-[12px] text-text-3">{ACTIVATION_DETAIL[vd.activationMode]}</p>
      <Divider />
      <CardHeader
        title="Shortcut"
        detail={capturingHotkey ? "Press the new shortcut. Esc cancels." : "Works in every app. Needs at least one modifier key."}
        right={
          <>
            {!hotkeyIsDefault && !capturingHotkey && (
              <Button kind="plain" onClick={() => api.resetHotkey()}>
                Reset
              </Button>
            )}
            <button
              onClick={() => (capturingHotkey ? api.cancelHotkeyCapture() : api.beginHotkeyCapture())}
              className={cx(
                "h-[28px] min-w-[120px] rounded-[6px] border px-3 font-mono text-[12.5px]",
                capturingHotkey ? "border-accent bg-accent-soft text-accent" : "border-card-border bg-fill hover:bg-fill-strong",
              )}
            >
              {capturingHotkey ? "Recording…" : hotkeyDisplay}
            </button>
          </>
        }
      />
      {captureError && <Warning>{captureError}</Warning>}
    </Card>
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
          Ready · {a.license} · {a.runtime}
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
      <CardHeader
        title="Engine"
        detail="Nothing downloads until you ask. Models live in Quill's app data folder and are verified against pinned checksums."
      />
      {error && <Warning>{error}</Warning>}
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
        Remove Engine
      </Button>
    );
  } else {
    actions = (
      <>
        {e.hasData && (
          <Button kind="destructive" onClick={props.onRemove}>
            Remove Engine
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
        title="Cleanup"
        detail={
          hasCleanup
            ? "How much S1-mini may rewrite what you said. It cleans English only; other languages are written as heard. Whatever the level, Quill keeps the recognizer's wording for any sentence where cleanup would drop a negation like “not” or “never”."
            : `${engine?.title ?? "This engine"} has no cleanup stage; transcripts are delivered as recognized. Choose Parakeet + S1-mini to adjust this.`
        }
      />
      <div className="mt-3">
        <Segmented
          disabled={!hasCleanup}
          value={vd.cleanupLevel}
          onChange={(v) => ctx.saveVd({ cleanupLevel: v })}
          options={CLEANUP.map((c) => ({ value: c.value, label: c.label }))}
        />
        <p className="mt-2 text-[12px] text-text-2">{level.detail}</p>
      </div>
    </Card>
  );
}

function FormattingCard({ ctx }: { ctx: Ctx }) {
  const vd = ctx.state.settings.voiceDictation;
  return (
    <Card>
      <CardHeader
        title="Adapt to the app"
        detail="In a terminal, or a terminal pane in VS Code or Cursor, Developer Mode writes spoken commands as commands. In Slack, Teams, Discord and other chat apps, cleanup stays casual and a one-line message drops its full stop."
        right={<Switch label="Adapt to the app" checked={vd.adaptsToApp} onChange={(v) => ctx.saveVd({ adaptsToApp: v })} />}
      />
      {vd.adaptsToApp && (
        <div className="mt-3 flex flex-col gap-1.5 rounded-[8px] bg-accent-soft px-3 py-2.5">
          {EXAMPLES.map(([said, typed]) => (
            <div key={said} className="flex flex-wrap items-baseline gap-x-2 text-[12px]">
              <span className="text-text-2">“{said}”</span>
              <span className="text-text-3">→</span>
              <code className="font-mono text-[12px]">{typed}</code>
            </div>
          ))}
        </div>
      )}
      <Divider />
      <CardHeader
        title="Write numbers as figures"
        detail={
          ctx.state.itnAvailable
            ? "“Twenty five dollars on March third” becomes “$25 on March 3”. Applies to English when S1-mini is not already doing it: Verbatim, the Whisper and Cohere engines, and terminals."
            : "The number formatter is not available in this build."
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

function HistoryCard({ ctx }: { ctx: Ctx }) {
  const { settings, historyCount } = ctx.state;
  const enabled = settings.voiceDictation.historyEnabled;
  const expanded = enabled && settings.historyExpanded;
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  useEffect(() => {
    if (!expanded) return;
    const load = () => api.getHistory().then(setEntries);
    load();
    const un = on("history-changed", load);
    return () => {
      un.then((f) => f());
    };
  }, [expanded, historyCount]);
  return (
    <Card>
      <div className="flex items-start gap-2">
        <button
          aria-label={expanded ? "Collapse" : "Expand"}
          disabled={!enabled}
          onClick={() => ctx.save({ ...settings, historyExpanded: !settings.historyExpanded })}
          className="mt-[2px] text-text-2 disabled:opacity-30"
        >
          <Icon.Chevron open={expanded} />
        </button>
        <div className="flex-1">
          <CardHeader
            title={historyCount > 0 ? `Transcript history · ${historyCount}` : "Transcript history"}
            detail="Off by default. When on, final text is kept on this PC for 24 hours, then deleted. Audio is never stored."
            right={
              <Switch
                label="Transcript history"
                checked={enabled}
                onChange={(v) => ctx.saveVd({ historyEnabled: v })}
              />
            }
          />
        </div>
      </div>
      {expanded && (
        <div className="mt-3">
          {entries.length === 0 ? (
            <p className="py-2 text-[12px] text-text-3">Nothing yet. Transcripts appear here as you dictate.</p>
          ) : (
            <div className="flex flex-col">
              {entries.map((e) => (
                <HistoryRow key={e.id} entry={e} />
              ))}
            </div>
          )}
          <div className="mt-2 flex items-center justify-between">
            <span className="text-[12px] text-text-3">{historyCount} kept</span>
            <Button kind="destructive" small disabled={historyCount === 0} onClick={() => api.clearHistory()}>
              Clear History
            </Button>
          </div>
        </div>
      )}
    </Card>
  );
}

function HistoryRow({ entry }: { entry: HistoryEntry }) {
  const [copied, setCopied] = useCopied();
  return (
    <div className="flex items-start gap-2 border-t border-hairline py-2 first:border-t-0">
      <div className="min-w-0 flex-1">
        <p className="selectable text-[12.5px] whitespace-pre-wrap">{entry.text}</p>
        <p className="mt-0.5 text-[11px] text-text-3">
          {entry.appName} · {timeAgo(entry.createdAt)}
        </p>
      </div>
      <IconButton
        label={copied ? "Copied" : "Copy"}
        onClick={() => {
          api.copyText(entry.text);
          setCopied();
        }}
      >
        {copied ? <Icon.Check size={14} /> : <Icon.Copy />}
      </IconButton>
    </div>
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
];

function LicensesCard() {
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
