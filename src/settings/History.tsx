import { useEffect, useState } from "react";
import { api, on, type CleanupLevel, type HistoryEntry } from "../lib/api";
import { timeAgo } from "../lib/format";
import type { Ctx } from "./App";
import { CLEANUP } from "./SetupTab";
import { Button, Capsule, Card, CardHeader, Icon, IconButton, Switch, useCopied, Warning } from "./ui";

export function HistoryCard({ ctx }: { ctx: Ctx }) {
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
          aria-label={expanded ? "Collapse history" : "Expand history"}
          aria-expanded={expanded}
          disabled={!enabled}
          onClick={() => ctx.save({ ...settings, historyExpanded: !settings.historyExpanded })}
          className="mt-[2px] text-text-2 disabled:opacity-30"
        >
          <Icon.Chevron open={expanded} />
        </button>
        <div className="flex-1">
          <CardHeader
            title={historyCount > 0 ? `Transcript history (${historyCount})` : "Transcript history"}
            detail="Off by default. When on, Quill keeps what it heard and what it pasted on this PC for 24 hours, then deletes it. Audio is never stored."
            right={<Switch label="Transcript history" checked={enabled} onChange={(v) => ctx.saveVd({ historyEnabled: v })} />}
          />
        </div>
      </div>
      {expanded && (
        <div className="mt-3">
          {entries.length === 0 ? (
            <p className="py-2 text-[12px] text-text-3">No transcripts yet. Each dictation shows up here for 24 hours.</p>
          ) : (
            <div className="flex flex-col">
              {entries.map((e) => (
                <HistoryRow key={e.id} entry={e} />
              ))}
            </div>
          )}
          <div className="mt-2 flex justify-end">
            <Button kind="destructive" small disabled={historyCount === 0} onClick={() => api.clearHistory()}>
              Clear history
            </Button>
          </div>
        </div>
      )}
    </Card>
  );
}

function HistoryRow({ entry }: { entry: HistoryEntry }) {
  const [copied, setCopied] = useCopied();
  const [redo, setRedo] = useState<{ level: CleanupLevel; text?: string; error?: string } | null>(null);
  const style = CLEANUP.find((c) => c.value === entry.level)?.label;
  const heard = entry.raw.trim();
  const redoAs = (level: CleanupLevel) => {
    setRedo({ level });
    api
      .redoHistory(entry.id, level)
      .then((text) => setRedo({ level, text }))
      .catch((e) => setRedo({ level, error: String(e) }));
  };
  return (
    <div className="border-t border-hairline py-2 first:border-t-0">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <p className="selectable text-[12.5px] whitespace-pre-wrap">{entry.text}</p>
          {heard && heard !== entry.text.trim() && (
            <p className="selectable mt-1 line-clamp-2 text-[11.5px] text-text-3">
              <span className="text-text-2">Heard </span>
              {heard}
            </p>
          )}
          <p className="mt-1 flex items-center gap-1.5 text-[11px] text-text-3">
            <span>
              {entry.appName}, {timeAgo(entry.createdAt)}
            </span>
            {style && <Capsule>{style}</Capsule>}
          </p>
        </div>
        {heard && (
          <select
            aria-label="Redo in another style"
            value=""
            onChange={(e) => e.target.value && redoAs(e.target.value as CleanupLevel)}
            className="h-[24px] rounded-[6px] border border-card-border bg-fill px-1.5 text-[12px] outline-none focus-visible:border-accent"
          >
            <option value="">Redo as</option>
            {CLEANUP.map((c) => (
              <option key={c.value} value={c.value}>
                {c.label}
              </option>
            ))}
          </select>
        )}
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
      {redo && (
        <div className="mt-2 rounded-[8px] bg-accent-soft px-3 py-2" aria-live="polite">
          {redo.error ? (
            <Warning>{redo.error}</Warning>
          ) : redo.text == null ? (
            <p className="text-[12px] text-text-2">Cleaning up as {CLEANUP.find((c) => c.value === redo.level)?.label}…</p>
          ) : (
            <>
              <p className="selectable text-[12.5px] whitespace-pre-wrap">{redo.text}</p>
              <p className="mt-1 text-[11px] text-text-3">
                Copied as {CLEANUP.find((c) => c.value === redo.level)?.label}. Paste it with Ctrl+V.
              </p>
            </>
          )}
        </div>
      )}
    </div>
  );
}
