// The Sketch controls, floating above the dictation pill: time, Draw, a
// hint, Undo, Cancel and Finish. Clicks here never move focus.
import { useEffect, useState } from "react";
import { ArrowUUpLeft, PencilSimple } from "@phosphor-icons/react";
import { api, on, type SketchView } from "../lib/api";

function clock(ms: number) {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export default function Bar() {
  const [view, setView] = useState<SketchView | null>(null);
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    const un = on<SketchView>("sketch-view", setView);
    api.sketchState().then((v) => v && setView(v));
    const timer = setInterval(() => setNow(Date.now()), 500);
    return () => {
      un.then((f) => f());
      clearInterval(timer);
    };
  }, []);

  if (!view) return null;
  const marks = view.marks === 0 ? "" : ` · ${view.marks} mark${view.marks === 1 ? "" : "s"}`;
  const hint = view.penOn ? "Drag to draw" : `Hold ${view.drawKey} and move to draw${marks}`;

  return (
    <div className="flex h-screen items-center p-[2px]">
      <div
        className="flex h-full w-full items-center gap-2.5 rounded-full pr-[11px] pl-[17px] text-white"
        style={{
          background: "rgb(23 23 26 / 0.96)",
          boxShadow: "inset 0 0 0 1px rgb(255 255 255 / 0.1)",
          animation: "rise-in 0.16s ease-out",
        }}
      >
        <span className="flex items-center gap-[7px]">
          <span className="h-[9px] w-[9px] rounded-full bg-[#ff3b30]" style={{ animation: "sketch-breathe 0.9s ease-in-out infinite alternate-reverse" }} />
          <span className="w-[34px] text-[14px] font-semibold tabular-nums">{clock(now - view.startedAt)}</span>
        </span>
        <span className="h-[22px] w-px bg-white/15" />
        <button
          onClick={() => api.sketchAction("pen")}
          title={view.penOn ? "Drag to draw. Click to go back to using the screen." : "Turn on to draw by dragging"}
          className="flex h-[30px] items-center gap-1.5 rounded-full px-2.5 text-[13px] font-semibold transition-colors"
          style={{ background: view.penOn ? "#ff3b30" : "rgb(255 255 255 / 0.1)" }}
        >
          <PencilSimple size={14} weight="bold" />
          Draw
        </button>
        <span className="min-w-0 flex-1 truncate text-[12px] text-white/60">{hint}</span>
        <button
          onClick={() => api.sketchAction("undo")}
          disabled={view.marks === 0}
          title="Undo the last mark (Ctrl+Z)"
          className="flex h-[30px] w-[30px] items-center justify-center rounded-full bg-white/10 transition-opacity disabled:opacity-40"
        >
          <ArrowUUpLeft size={14} weight="bold" />
        </button>
        <button onClick={() => api.sketchAction("cancel")} className="px-1 text-[13px] font-medium text-white/75 hover:text-white">
          Cancel
        </button>
        <button
          onClick={() => api.sketchAction("finish")}
          title={view.finishShortcut ? `Finish (${view.finishShortcut})` : "Finish"}
          className="flex h-[30px] items-center rounded-full bg-[#0a84ff] px-3.5 text-[13px] font-semibold hover:brightness-110"
        >
          Finish
        </button>
      </div>
    </div>
  );
}
