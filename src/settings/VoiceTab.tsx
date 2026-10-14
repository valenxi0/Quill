import { useEffect, useState } from "react";
import { api, type EntryKind, type Insights, type VocabEntry } from "../lib/api";
import { AppPicker, AppScope, scopeName, useKnownApps } from "./AppScope";
import { compact } from "../lib/format";
import type { Ctx } from "./App";
import { HistoryCard } from "./History";
import { Button, Capsule, Card, CardHeader, Divider, Icon, IconButton, Segmented, Sheet, Switch, TextField } from "./ui";

export function useInsights(dep: unknown): Insights | null {
  const [insights, setInsights] = useState<Insights | null>(null);
  useEffect(() => {
    api.getInsights().then(setInsights);
  }, [dep]);
  return insights;
}

function Stat({ title, value, detail }: { title: string; value: string; detail: string }) {
  return (
    <Card className="flex-1">
      <p className="text-[12px] text-text-2">{title}</p>
      <p className="mt-1 text-[24px] font-semibold tabular-nums">{value}</p>
      <p className="text-[11.5px] text-text-3">{detail}</p>
    </Card>
  );
}

export function VoiceTab({ ctx }: { ctx: Ctx }) {
  const insights = useInsights(ctx.state);
  const vocab = ctx.state.settings.voiceDictation.vocabulary;
  const median = insights?.medianLatency;
  return (
    <div className="flex flex-col gap-[10px]">
      <div className="flex gap-[10px]">
        <Stat title="Filler words removed" value={compact(insights?.fillersRemoved ?? 0)} detail="Um, uh, and erm cut by cleanup" />
        <Stat
          title="Dictionary expansions"
          value={compact(insights?.dictionaryExpansions ?? 0)}
          detail={`From ${vocab.length} ${vocab.length === 1 ? "entry" : "entries"}`}
        />
        <Stat title="Release to paste" value={median != null ? `${median.toFixed(2)}s` : "--"} detail="Median of your last 200 dictations" />
      </div>
      <VocabularyCard ctx={ctx} />
      <CorrectionsCard ctx={ctx} />
      <HistoryCard ctx={ctx} />
    </div>
  );
}

function CorrectionsCard({ ctx }: { ctx: Ctx }) {
  const { suggestions, correctionsSeen, settings } = ctx.state;
  const apps = useKnownApps();
  const [adding, setAdding] = useState<(typeof suggestions)[number] | null>(null);
  return (
    <Card>
      <CardHeader
        title="Learn from corrections"
        detail="After a paste, Quill rereads the field. A word you fix the same way twice shows up here to add to your dictionary. Only word pairs and their app are kept, on this PC."
        right={
          <Switch
            label="Learn from corrections"
            checked={settings.voiceDictation.learnsCorrections}
            onChange={(v) => ctx.saveVd({ learnsCorrections: v })}
          />
        }
      />
      {suggestions.length > 0 && (
        <div className="mt-3 flex flex-col gap-1.5">
          {suggestions.map((s) => (
            <div key={`${s.appExe ?? "global"}:${s.from}→${s.to}`} className="flex items-center gap-2 rounded-[8px] bg-fill px-3 py-2">
              <span className="text-[12.5px]">
                <span className="text-text-2">{s.from}</span> <span className="text-text-3">→</span>{" "}
                <span className="font-semibold">{s.to}</span>
              </span>
              <span className="text-[11.5px] text-text-3">fixed {s.count}×</span>
              <Capsule>{scopeName(s.appExe, apps)}</Capsule>
              <span className="flex-1" />
              <Button small kind="plain" onClick={() => api.dismissSuggestion(s.from, s.to, s.appExe ?? null)}>
                Not now
              </Button>
              <Button small kind="primary" onClick={() => setAdding(s)}>
                Add
              </Button>
            </div>
          ))}
        </div>
      )}
      {adding && <EntrySheet entry={{ id: crypto.randomUUID(), phrase: adding.from, replacement: adding.to, kind: "vocabulary", isEnabled: true, appExe: adding.appExe ?? null }}
        onClose={() => setAdding(null)} onSave={(entry) => {
          ctx.saveVd({ vocabulary: [...settings.voiceDictation.vocabulary, entry] });
          api.dismissSuggestion(adding.from, adding.to, adding.appExe ?? null);
          setAdding(null);
        }} />}
      <div className="mt-3 flex items-center justify-between">
        <span className="text-[12px] text-text-3">
          {correctionsSeen} {correctionsSeen === 1 ? "correction" : "corrections"} seen
        </span>
        <Button small kind="destructive" disabled={correctionsSeen === 0} onClick={() => api.forgetCorrections()}>
          Forget all
        </Button>
      </div>
    </Card>
  );
}

function VocabularyCard({ ctx }: { ctx: Ctx }) {
  const vd = ctx.state.settings.voiceDictation;
  const apps = useKnownApps();
  const [filter, setFilter] = useState("*");
  const [editing, setEditing] = useState<VocabEntry | null>(null);
  const [pasting, setPasting] = useState(false);
  const engine = ctx.state.engines.find((e) => e.id === vd.engine);
  const booster = engine?.assets.find((a) => a.id === "parakeetVocabularyBooster");

  const setEntries = (vocabulary: VocabEntry[]) => ctx.saveVd({ vocabulary });
  const upsert = (entry: VocabEntry) => {
    const exists = vd.vocabulary.some((e) => e.id === entry.id);
    setEntries(exists ? vd.vocabulary.map((e) => (e.id === entry.id ? entry : e)) : [...vd.vocabulary, entry]);
  };

  return (
    <Card>
      <CardHeader
        title="Vocabulary & snippets"
        detail={'Teach Quill a spelling, such as "mac island" written as "Nano", or a phrase like "my sign off" that expands to a full closing.'}
        right={
          <>
            <Button onClick={() => setPasting(true)}>Paste list</Button>
            <Button
              kind="primary"
              onClick={() =>
                setEditing({ id: crypto.randomUUID(), phrase: "", replacement: "", kind: "vocabulary", isEnabled: true, appExe: filter === "*" || filter === "" ? null : filter })
              }
            >
              <Icon.Plus /> Add
            </Button>
          </>
        }
      />
      <div className="mt-3 max-w-[300px]">
        <AppPicker label="Show vocabulary for" value={filter} onChange={setFilter} options={[
          { value: "*", label: "All entries" },
          { value: "", label: "All apps" },
          ...[...new Set(vd.vocabulary.flatMap((entry) => entry.appExe ? [entry.appExe] : []))].map((exe) => ({
            value: exe, label: scopeName(exe, apps), app: apps.find((app) => app.exe.toLowerCase() === exe.toLowerCase()),
          })),
        ]} />
      </div>
      {vd.vocabulary.length > 0 && (
        <div className="mt-3 flex flex-col">
          {vd.vocabulary.filter((e) => filter === "*" || (e.appExe ?? "") === filter).map((e) => (
            <div key={e.id} className="flex items-center gap-2 border-t border-hairline py-1.5 first:border-t-0">
              <div className="min-w-0 flex-1 truncate text-[12.5px]">
                <span className="text-text-2">{e.phrase}</span> <span className="text-text-3">→</span>{" "}
                <span className="font-medium">{e.replacement.replace(/\n/g, " ⏎ ")}</span>
              </div>
              {e.appExe && <Capsule>{scopeName(e.appExe, apps)}</Capsule>}
              <Capsule>{e.kind === "snippet" ? "Snippet" : "Vocabulary"}</Capsule>
              <span className="w-8 text-right text-[11.5px] text-text-3 tabular-nums">{ctx.state.dictionaryHits[e.id] ?? 0}×</span>
              <Switch label={`Enable ${e.phrase}`} checked={e.isEnabled} onChange={(v) => upsert({ ...e, isEnabled: v })} />
              <Button small kind="plain" onClick={() => setEditing(e)}>
                Edit
              </Button>
              <IconButton label={`Delete ${e.phrase}`} onClick={() => setEntries(vd.vocabulary.filter((x) => x.id !== e.id))}>
                <Icon.Trash />
              </IconButton>
            </div>
          ))}
        </div>
      )}
      <Divider />
      {engine?.recognizer === "parakeet" && (
        <p className="text-[12px] text-text-3">
          Quill writes these terms into Parakeet's transcript right after it recognizes.
          {booster && booster.available && booster.state.state !== "downloaded" && (
            <>
              {" "}
              <button className="text-accent" onClick={() => api.downloadAsset(booster.id)}>
                Download the vocabulary booster
              </button>{" "}
              to catch them more often.
            </>
          )}
        </p>
      )}
      {engine?.recognizer === "whisper" && (
        <p className="text-[12px] text-text-3">Whisper is primed with these terms before it listens.</p>
      )}
      {engine?.recognizer === "cohere" && (
        <p className="text-[12px] text-text-3">Entries are applied to Cohere's transcript after it recognizes.</p>
      )}
      {editing && <EntrySheet entry={editing} onClose={() => setEditing(null)} onSave={(e) => (upsert(e), setEditing(null))} />}
      {pasting && (
        <PasteListSheet
          onClose={() => setPasting(false)}
          onImport={(entries) => {
            setEntries([...vd.vocabulary, ...entries]);
            setPasting(false);
          }}
        />
      )}
    </Card>
  );
}

function EntrySheet({ entry, onClose, onSave }: { entry: VocabEntry; onClose: () => void; onSave: (e: VocabEntry) => void }) {
  const [draft, setDraft] = useState(entry);
  const [sample, setSample] = useState("");
  const [preview, setPreview] = useState("");
  useEffect(() => {
    const text = sample || draft.phrase;
    if (!text.trim()) return setPreview("");
    api.testVocabularyEntry(draft, text).then(setPreview);
  }, [draft, sample]);
  const valid = draft.phrase.trim().length > 0 && draft.replacement.trim().length > 0 && (draft.appExe == null || /^[^\\/]+\.exe$/i.test(draft.appExe));
  return (
    <Sheet
      title={entry.phrase ? "Edit entry" : "New entry"}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button kind="primary" disabled={!valid} onClick={() => onSave(draft)}>
            Save
          </Button>
        </>
      }
    >
      <div className="mb-3">
        <span className="mb-1 block text-[12px] text-text-2">Kind</span>
        <Segmented<EntryKind>
          value={draft.kind}
          onChange={(kind) => setDraft({ ...draft, kind })}
          options={[
            { value: "vocabulary", label: "Vocabulary" },
            { value: "snippet", label: "Snippet" },
          ]}
        />
      </div>
      <AppScope value={draft.appExe ?? null} onChange={(appExe) => setDraft({ ...draft, appExe })} />
      <TextField label="Phrase (what you say)" value={draft.phrase} onChange={(phrase) => setDraft({ ...draft, phrase })} placeholder="mac island" />
      <TextField
        label={draft.kind === "snippet" ? "Expands to" : "Written as"}
        value={draft.replacement}
        onChange={(replacement) => setDraft({ ...draft, replacement })}
        placeholder={draft.kind === "snippet" ? "Best,\nJorge" : "Nano"}
        multiline={draft.kind === "snippet"}
      />
      <TextField label="Test" value={sample} onChange={setSample} placeholder={`Try a sentence with "${draft.phrase || "the phrase"}"`} />
      {preview && (
        <div className="selectable rounded-[6px] bg-accent-soft px-2.5 py-1.5 text-[12.5px] whitespace-pre-wrap">{preview}</div>
      )}
    </Sheet>
  );
}

function PasteListSheet({ onClose, onImport }: { onClose: () => void; onImport: (e: VocabEntry[]) => void }) {
  const [text, setText] = useState("");
  const [parsed, setParsed] = useState<VocabEntry[]>([]);
  const [appExe, setAppExe] = useState<string | null>(null);
  useEffect(() => {
    let current = true;
    setParsed([]);
    api.parseVocabularyList(text, appExe).then((entries) => { if (current) setParsed(entries); });
    return () => { current = false; };
  }, [text, appExe]);
  return (
    <Sheet
      title="Paste a list"
      width={500}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button kind="primary" disabled={parsed.length === 0 || (appExe != null && !/^[^\\/]+\.exe$/i.test(appExe))} onClick={() => onImport(parsed)}>
            Add {parsed.length} {parsed.length === 1 ? "term" : "terms"}
          </Button>
        </>
      }
    >
      <p className="mb-2 text-[12px] text-text-2">
        One term per line, or comma-separated. Write <code className="font-mono">spoken → written</code> to set how a phrase is
        written; a bare term is written exactly as typed. Duplicates are skipped.
      </p>
      <AppScope value={appExe} onChange={setAppExe} />
      <TextField label="Terms" value={text} onChange={setText} multiline mono placeholder={"Kubernetes, Tauri\nmac island → Nano"} />
      {parsed.length > 0 && (
        <div className="max-h-[140px] overflow-auto rounded-[6px] bg-fill px-2.5 py-1.5 text-[12px]">
          {parsed.map((e) => (
            <div key={e.id}>
              {e.phrase === e.replacement ? e.phrase : `${e.phrase} → ${e.replacement}`}
            </div>
          ))}
        </div>
      )}
    </Sheet>
  );
}
