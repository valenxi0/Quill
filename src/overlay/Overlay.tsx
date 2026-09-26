// The status pill. Redraws only on level updates; nothing animates while idle.
import { useEffect, useRef, useState } from "react";
import { api, on, type CorrectionPair, type DictationView } from "../lib/api";

const BARS = 30;
const STEP = 6; // px per bar (4 wide + 2 gap)

export default function Overlay() {
  const [view, setView] = useState<DictationView | null>(null);
  const [suggestion, setSuggestion] = useState<CorrectionPair | null>(null);
  const [reduced, setReduced] = useState(false);

  useEffect(() => {
    const subs = [
      on<DictationView>("dictation", (v) => {
        setView(v);
        if (v.phase !== "idle") setSuggestion(null);
        if (v.phase === "recording") api.animationsEnabled().then((a) => setReduced(!a));
      }),
      on<CorrectionPair>("suggestion", (p) => {
        setSuggestion(p);
        api.animationsEnabled().then((a) => setReduced(!a));
      }),
      on("suggestion-dismissed", () => setSuggestion(null)),
    ];
    return () => subs.forEach((p) => p.then((f) => f()));
  }, []);

  useEffect(() => {
    document.documentElement.classList.toggle("reduced-motion", reduced);
  }, [reduced]);

  const phase = view?.phase ?? "idle";
  if (suggestion && phase === "idle") return <Suggestion pair={suggestion} onDone={() => setSuggestion(null)} />;
  if (!view || phase === "idle") return null;

  return (
    <div className="flex h-screen items-end justify-center pb-2">
      <Pill cancelled={phase === "cancelled"}>
        {phase === "recording" && <Listening reduced={reduced} terminal={view.context === "terminal"} />}
        {(phase === "transcribing" || phase === "cleaning") && (
          <>
            <Dots />
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

function Pill({ children, cancelled }: { children: React.ReactNode; cancelled?: boolean }) {
  return (
    <div
      className="flex h-[44px] items-center gap-2.5 rounded-full px-4 text-[13px] font-medium text-white"
      style={{
        background: "rgba(22, 22, 24, 0.94)",
        border: "1px solid rgba(255,255,255,0.1)",
        boxShadow: "0 6px 24px rgba(0,0,0,0.35)",
        animation: cancelled ? "flash 1.2s ease-out" : "pop-in 0.14s ease-out",
      }}
    >
      {children}
    </div>
  );
}

function Listening({ reduced, terminal }: { reduced: boolean; terminal: boolean }) {
  const [levels, setLevels] = useState<number[]>(() => Array(BARS).fill(0));
  const strip = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const un = on<number>("level", (level) => {
      setLevels((prev) => [...prev.slice(1), level]);
      // Slide the strip one bar over the ~83 ms until the next update.
      const el = strip.current;
      if (el && !reduced) {
        el.style.transition = "none";
        el.style.transform = `translateX(${STEP}px)`;
        requestAnimationFrame(() => {
          el.style.transition = "transform 83ms linear";
          el.style.transform = "translateX(0)";
        });
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, [reduced]);

  return (
    <>
      <MicGlyph />
      <div className="relative h-[24px] overflow-hidden" style={{ width: BARS * STEP - 2 }}>
        <div ref={strip} className="absolute inset-0 flex items-center gap-[2px]">
          {levels.map((l, i) => {
            const h = 3 + Math.round(l * 21);
            const age = (BARS - 1 - i) / BARS;
            return (
              <span
                key={i}
                className="w-[4px] shrink-0 rounded-full bg-white"
                style={{ height: h, opacity: Math.max(0.15, 1 - age * 0.85) }}
              />
            );
          })}
        </div>
      </div>
      {terminal && <span className="rounded-full bg-white/15 px-1.5 text-[10.5px] text-white/80">Terminal</span>}
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

function Suggestion({ pair, onDone }: { pair: CorrectionPair; onDone: () => void }) {
  const finish = (accept: boolean) => {
    (accept ? api.acceptSuggestion(pair.from, pair.to) : api.dismissSuggestion(pair.from, pair.to)).finally(() => {
      onDone();
      api.hideOverlay();
    });
  };
  return (
    <div className="flex h-screen items-end justify-center pb-2">
      <Pill>
        <span className="text-white/70">Add</span>
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
