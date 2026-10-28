// The ink layer: a soft red glow along the edges of the monitor being
// recorded, the stroke being drawn, and finished marks that fade a moment
// after they are drawn. Points arrive from Rust in this window's CSS pixels.
import { useEffect, useRef, useState } from "react";
import { api, on } from "../lib/api";

type Pt = [number, number];
interface Mark {
  id: number;
  points: Pt[];
  label: string;
}

/** How long a finished mark stays before it fades, and how long the fade takes. */
const HOLD_MS = 1400;
const FADE_MS = 500;
const INK = "rgb(255 59 48)";

/** Quadratic curves through the midpoints, like the screenshots get. */
function smoothPath(points: Pt[]): string {
  if (points.length < 2) return "";
  const at = (i: number) => points[i]!;
  let d = `M${at(0)[0]} ${at(0)[1]}`;
  if (points.length === 2) return `${d}L${at(1)[0]} ${at(1)[1]}`;
  for (let i = 1; i < points.length; i++) {
    const [px, py] = at(i - 1);
    const [x, y] = at(i);
    d += `Q${px} ${py} ${(px + x) / 2} ${(py + y) / 2}`;
  }
  const [lx, ly] = at(points.length - 1);
  return `${d}L${lx} ${ly}`;
}

export default function Ink() {
  const [active, setActive] = useState(false);
  const [pen, setPen] = useState(false);
  const [live, setLive] = useState<Pt[]>([]);
  const [marks, setMarks] = useState<Mark[]>([]);
  const nextId = useRef(1);

  useEffect(() => {
    const subs = [
      on("sketch-start", () => {
        setMarks([]);
        setLive([]);
        setPen(false);
        setActive(true);
      }),
      on("sketch-end", () => {
        setActive(false);
        setPen(false);
        setLive([]);
      }),
      on<Pt[]>("sketch-live", setLive),
      on<boolean>("sketch-pen", setPen),
      on<{ points: Pt[]; label: string }>("sketch-commit", ({ points, label }) => {
        const id = nextId.current++;
        setMarks((m) => [...m, { id, points, label }]);
        setTimeout(() => setMarks((m) => m.filter((x) => x.id !== id)), HOLD_MS + FADE_MS);
      }),
      on("sketch-undo", () => setMarks((m) => m.slice(0, -1))),
    ];
    // Opened mid-Sketch (the first one): show at once.
    api.sketchState().then((v) => v && setActive(true));
    return () => subs.forEach((p) => p.then((f) => f()));
  }, []);

  return (
    <div
      className="fixed inset-0"
      style={{
        cursor: pen ? "crosshair" : "default",
        opacity: active ? 1 : 0,
        transition: active ? "opacity 700ms ease-out" : "opacity 350ms ease-in",
      }}
    >
      <Glow strong={pen} />
      <svg className="absolute inset-0 h-full w-full" fill="none" stroke={INK} strokeWidth={4} strokeLinecap="round" strokeLinejoin="round">
        {marks.map((m) => (
          <g key={m.id} style={{ animation: `sketch-mark-fade ${FADE_MS}ms ease-in ${HOLD_MS}ms forwards` }}>
            <path d={smoothPath(m.points)} />
          </g>
        ))}
        {live.length > 1 && <path d={smoothPath(live)} />}
      </svg>
      {marks.map((m) => (
        <Tag key={m.id} at={m.points[0] ?? [0, 0]} text={m.label} />
      ))}
    </div>
  );
}

/** A soft red light fading in from every edge. Stronger while Draw is on,
 * since clicks no longer reach the apps. */
function Glow({ strong }: { strong: boolean }) {
  const depth = strong ? 64 : 44;
  const edge = strong ? 0.42 : 0.26;
  const middle = strong ? 0.12 : 0.07;
  const stops = (dir: string) =>
    `linear-gradient(${dir}, rgb(255 59 48 / ${edge}) 0%, rgb(255 59 48 / ${middle}) 35%, rgb(255 59 48 / 0) 100%)`;
  const base = "absolute transition-[width,height] duration-200";
  return (
    <div className="absolute inset-0" style={{ animation: "sketch-breathe 1.8s ease-in-out infinite alternate" }}>
      <div className={`${base} inset-x-0 top-0`} style={{ height: depth, background: stops("to bottom") }} />
      <div className={`${base} inset-x-0 bottom-0`} style={{ height: depth, background: stops("to top") }} />
      <div className={`${base} inset-y-0 left-0`} style={{ width: depth, background: stops("to right") }} />
      <div className={`${base} inset-y-0 right-0`} style={{ width: depth, background: stops("to left") }} />
    </div>
  );
}

/** The mark's time, up and to the left of where it began. */
function Tag({ at, text }: { at: Pt; text: string }) {
  return (
    <span
      className="absolute rounded-full px-2 py-0.5 text-[12px] font-bold tabular-nums text-white"
      style={{
        left: Math.max(4, at[0] - 52),
        top: Math.max(4, at[1] - 30),
        background: "rgb(20 20 23 / 0.9)",
        animation: `sketch-mark-fade ${FADE_MS}ms ease-in ${HOLD_MS}ms forwards`,
      }}
    >
      {text}
    </span>
  );
}
