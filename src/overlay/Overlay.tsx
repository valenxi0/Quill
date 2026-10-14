// The status pill. Redraws only on level updates; nothing animates while idle.
import { useEffect, useState } from "react";
import { api, on, type AppContext, type CorrectionPair, type DictationView, type UpdateInfo } from "../lib/api";

export default function Overlay({ preview }: { preview?: DictationView }) {
  const [view, setView] = useState<DictationView | null>(preview ?? null);
  const [suggestion, setSuggestion] = useState<CorrectionPair | null>(null);
  const [reduced, setReduced] = useState(false);
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [heard, setHeard] = useState("");

  useEffect(() => {
    const subs = [
      on<DictationView>("dictation", (v) => {
        setView(v);
        if (v.phase !== "idle") setSuggestion(null);
        if (v.phase !== "recording") setHeard("");
        if (v.phase === "recording") api.animationsEnabled().then((a) => setReduced(!a));
      }),
      on<CorrectionPair>("suggestion", (p) => {
        setSuggestion(p);
        api.animationsEnabled().then((a) => setReduced(!a));
      }),
      on("suggestion-dismissed", () => setSuggestion(null)),
      on<UpdateInfo>("update-available", setUpdate),
      on("update-dismissed", () => setUpdate(null)),
      on<string>("preview", setHeard),
    ];
    return () => subs.forEach((p) => p.then((f) => f()));
  }, []);

  useEffect(() => {
    document.documentElement.classList.toggle("reduced-motion", reduced);
  }, [reduced]);

  const phase = view?.phase ?? "idle";
  if (update && phase === "idle") return <UpdatePrompt info={update} onDone={() => setUpdate(null)} />;
  if (suggestion && phase === "idle") return <Suggestion pair={suggestion} onDone={() => setSuggestion(null)} />;
  if (!view || phase === "idle") return null;

  return (
    <div className="flex h-screen flex-col items-center justify-end gap-1.5 pb-2">
      {phase === "recording" && heard && <Caption text={heard} />}
      <Pill recording={phase === "recording"}>
        {phase === "recording" && <Listening context={view.context} />}
        {(phase === "transcribing" || phase === "cleaning") && (
          <>
            <Orb />
            <span>{phase === "transcribing" ? "Transcribing" : "Cleaning up"}</span>
          </>
        )}
        {phase === "completed" && (
          <>
            <CheckGlyph />
            <span>{view.outcome ?? "Paste sent"}</span>
          </>
        )}
        {phase === "failed" && (
          <>
            <AlertGlyph />
            <span>{view.errorTitle ?? "Something went wrong"}</span>
          </>
        )}
        {phase === "cancelled" && (
          <>
            <span style={{ animation: "shake 0.45s ease-in-out" }}>
              <MicGlyph slashed />
            </span>
            <span>Cancelled</span>
          </>
        )}
      </Pill>
    </div>
  );
}

function Pill({ children, recording }: { children: React.ReactNode; recording?: boolean }) {
  return (
    <div
      className="flex h-[40px] items-center gap-2 rounded-full px-3 text-[12.5px] font-medium text-white"
      style={{
        background: "rgba(10, 10, 10, 0.92)",
        border: recording ? "1px solid #ff453a" : "1px solid rgba(255,255,255,0.12)",
        animation: "rise-in 0.16s ease-out",
      }}
    >
      {children}
    </div>
  );
}

/** The end of the live preview: the words just spoken matter most. */
function Caption({ text }: { text: string }) {
  const MAX = 140;
  const shown = text.length > MAX ? `…${text.slice(text.length - MAX).replace(/^\S*\s/, "")}` : text;
  return (
    <p
      className="max-w-[350px] rounded-[12px] px-3 py-1.5 text-center text-[12px] leading-[1.4] text-white/85"
      style={{ background: "rgba(10, 10, 10, 0.92)", border: "1px solid rgba(255,255,255,0.12)", textWrap: "balance", animation: "rise-in 0.16s ease-out" }}
    >
      {shown}
    </p>
  );
}

/** Shown while Quill transcribes and cleans up: a small breathing light. */
function Orb() {
  return (
    <span
      aria-hidden="true"
      className="h-[16px] w-[16px] shrink-0 rounded-full"
      style={{
        background: "radial-gradient(circle at 35% 30%, #ffffff 0%, #7cc0ff 22%, #0a84ff 55%, #063c80 100%)",
        boxShadow: "0 0 10px rgb(10 132 255 / 0.6)",
        animation: "orb-breathe 1.6s ease-in-out infinite",
      }}
    />
  );
}

const BARS = 12;

function mmss(seconds: number) {
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

function Listening({ context }: { context: AppContext }) {
  // The last 12 voice readings, newest on the right: a rolling waveform.
  const [bars, setBars] = useState<number[]>(() => Array(BARS).fill(0));
  const [seconds, setSeconds] = useState(0);

  useEffect(() => {
    // Track the room's background noise so silence reads as flat. The floor
    // follows quiet moments quickly and creeps up slowly during speech.
    let floor = 1;
    const un = on<number>("level", (level) => {
      floor = level < floor ? level : Math.min(level, floor + 0.004);
      const voice = Math.min(1, Math.max(0, (level - floor - 0.06) / 0.45));
      setBars((prev) => [...prev.slice(1), voice]);
    });
    const timer = setInterval(() => setSeconds((t) => t + 1), 1000);
    return () => {
      un.then((f) => f());
      clearInterval(timer);
    };
  }, []);

  return (
    <>
      <div className="flex items-center gap-2">
        <div className="flex h-6 items-center gap-[2px]">
          {bars.map((v, i) => (
            <span
              key={i}
              className="w-[2px] rounded-full bg-white"
              style={{ height: 2 + v * 18, transition: "height 90ms ease-out" }}
            />
          ))}
        </div>
        <span className="w-10 text-center text-[12px] font-normal tabular-nums text-white/70">{mmss(seconds)}</span>
      </div>
      {(context === "terminal" || context === "email") && <span className="rounded-full bg-white/15 px-1.5 text-[10.5px] text-white/80">{context === "email" ? "Email" : "Terminal"}</span>}
    </>
  );
}

function Dots() {
  return (
    <span className="flex gap-[3px]">
      {[0, 1, 2].map((i) => (
        <span
          key={i}
          className="h-[5px] w-[5px] rounded-full bg-white"
          style={{ animation: `pulse-dots 1.2s ${i * 0.16}s infinite ease-in-out` }}
        />
      ))}
    </span>
  );
}

function UpdatePrompt({ info, onDone }: { info: UpdateInfo; onDone: () => void }) {
  const [installing, setInstalling] = useState(false);
  const later = () => {
    onDone();
    api.hideOverlay();
  };
  return (
    <div className="flex h-screen items-end justify-center pb-2">
      <Pill>
        <span>
          Quill <b>{info.version}</b> is available
        </span>
        {installing ? (
          <span className="flex items-center gap-2 text-white/70">
            <Dots /> Updating
          </span>
        ) : (
          <>
            <button className="rounded-full px-2 py-0.5 text-white/70 hover:bg-white/10" onClick={later}>
              Later
            </button>
            <button
              className="rounded-full bg-[#0a84ff] px-2.5 py-0.5 hover:brightness-110"
              onClick={() => {
                setInstalling(true);
                api.installUpdate().catch(later);
              }}
            >
              Update
            </button>
          </>
        )}
      </Pill>
    </div>
  );
}

function Suggestion({ pair, onDone }: { pair: CorrectionPair; onDone: () => void }) {
  const finish = (accept: boolean) => {
    (accept ? api.acceptSuggestion(pair.from, pair.to, pair.appExe ?? null) : api.dismissSuggestion(pair.from, pair.to, pair.appExe ?? null)).finally(() => {
      onDone();
      api.hideOverlay();
    });
  };
  return (
    <div className="flex h-screen items-end justify-center pb-2">
      <Pill>
        <span className="text-white/70" title={pair.appExe ?? "All apps"}>{pair.appExe ? "Add for this app" : "Add"}</span>
        <span>
          {pair.from} → <b>{pair.to}</b>
        </span>
        <button className="rounded-full px-2 py-0.5 text-white/70 hover:bg-white/10" onClick={() => finish(false)}>
          Not now
        </button>
        <button className="rounded-full bg-[#0a84ff] px-2.5 py-0.5 hover:brightness-110" onClick={() => finish(true)}>
          Add
        </button>
      </Pill>
    </div>
  );
}

const glyph = { width: 16, height: 16, viewBox: "0 0 16 16", fill: "none", stroke: "currentColor", strokeWidth: 1.7, strokeLinecap: "round", strokeLinejoin: "round" } as const;

function MicGlyph({ slashed }: { slashed?: boolean }) {
  return (
    <svg {...glyph} style={{ color: slashed ? "#ff453a" : "white" }}>
      <rect x="5.5" y="1.5" width="5" height="8.5" rx="2.5" />
      <path d="M3 8a5 5 0 0 0 10 0M8 13v1.8" />
      {slashed && <path d="M2 2l12 12" />}
    </svg>
  );
}

function CheckGlyph() {
  return (
    <svg {...glyph} strokeWidth={2.2} style={{ color: "#32d74b" }}>
      <path d="M3.5 8.5l3 3 6-7" />
    </svg>
  );
}

function AlertGlyph() {
  return (
    <svg {...glyph} style={{ color: "#ff9f0a" }}>
      <circle cx="8" cy="8" r="6.2" />
      <path d="M8 4.8v3.6M8 11v.1" />
    </svg>
  );
}
