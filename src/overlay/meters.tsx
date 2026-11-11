// The voice meters the recording pill can show, also previewed in Settings.
// Each takes recent voice readings (0..1, newest last). Nothing here runs
// once the meter unmounts. Under reduced motion the meters still follow the
// voice, but the waves stop travelling on their own.
import { useEffect, useMemo, useRef, useState, type CSSProperties, type ReactElement } from "react";
import { DitherShader } from "@/lib/dither/shader";
import { useDocumentVisible } from "@/lib/dither/motion";
import type { DitherPalette } from "@/lib/dither/tokens";
import type { PillStyle } from "../lib/api";

export interface MeterProps {
  levels: number[];
  reduced: boolean;
}

/** Readings kept for the meters: the Waveform shows them all, Bars the last 12. */
export const METER_HISTORY = 28;

const WIDTH = 64;
const HEIGHT = 26;
/** The wave's phase while it isn't travelling. */
const STILL_TIME = 1.3;

export function BarsMeter({ levels }: MeterProps) {
  return (
    <div className="flex h-6 shrink-0 items-center gap-[2px]">
      {levels.slice(-12).map((v, i) => (
        <span key={i} className="w-[2px] rounded-full bg-white" style={{ height: 2 + v * 18, transition: "height 90ms ease-out" }} />
      ))}
    </div>
  );
}

/** A canvas at the screen's pixel density, drawn in CSS pixels. */
function useCanvas() {
  const ref = useRef<HTMLCanvasElement>(null);
  const begin = () => {
    const canvas = ref.current;
    const g = canvas?.getContext("2d");
    if (!canvas || !g) return null;
    const scale = window.devicePixelRatio || 1;
    const w = Math.round(WIDTH * scale);
    const h = Math.round(HEIGHT * scale);
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
    }
    g.setTransform(scale, 0, 0, scale, 0, 0);
    g.clearRect(0, 0, WIDTH, HEIGHT);
    return { g, scale };
  };
  const element = <canvas ref={ref} aria-hidden="true" className="block shrink-0" style={{ width: WIDTH, height: HEIGHT }} />;
  return { begin, element };
}

// One 30 Hz timer shared by every moving meter. A timer, not
// requestAnimationFrame, so a 240 Hz display doesn't wake the page 240
// times a second for 30 drawn frames.
const TICK_MS = 33;
const tickListeners = new Set<() => void>();
let tickTimer: number | undefined;

function onTick(listener: () => void) {
  tickListeners.add(listener);
  tickTimer ??= window.setInterval(() => tickListeners.forEach((l) => l()), TICK_MS);
  return () => {
    tickListeners.delete(listener);
    if (tickListeners.size === 0) {
      window.clearInterval(tickTimer);
      tickTimer = undefined;
    }
  };
}

/**
 * The wave's phase in seconds and its level. While `moving` (and the window
 * is visible) both update on the shared tick, the level easing a quarter of
 * the way to the newest reading per 60 Hz frame. Otherwise the phase holds
 * and the level is the newest reading. The phase counts from when the wave
 * starts moving: it reaches the shader as a 32-bit float, and seconds since
 * the overlay loaded (days, in a long session) would lose the precision a
 * smooth wave needs.
 */
function useWave(target: number, moving: boolean) {
  const visible = useDocumentVisible();
  const running = moving && visible;
  const [wave, setWave] = useState({ t: STILL_TIME, level: target });
  const targetRef = useRef(target);
  useEffect(() => {
    targetRef.current = target;
  }, [target]);
  useEffect(() => {
    if (!running) return;
    const start = performance.now();
    let last = start;
    return onTick(() => {
      const now = performance.now();
      const frames = Math.min(6, (now - last) / (1000 / 60));
      last = now;
      setWave((w) => ({ t: STILL_TIME + (now - start) / 1000, level: w.level + (targetRef.current - w.level) * (1 - Math.pow(0.75, frames)) }));
    });
  }, [running]);
  return running ? { ...wave, running } : { t: STILL_TIME, level: target, running };
}

/** Thin bars mirrored around the centre line, fading at both ends. Redraws only when a reading arrives. */
export function WaveformMeter({ levels }: MeterProps) {
  const canvas = useCanvas();
  useEffect(() => {
    const c = canvas.begin();
    if (!c) return;
    const { g } = c;
    const n = levels.length;
    const step = WIDTH / n;
    const barWidth = Math.max(0.5, step * 0.55);
    g.fillStyle = "#fff";
    for (let i = 0; i < n; i++) {
      const fade = Math.min(1, Math.min(i, n - 1 - i) / (n * 0.3));
      const height = Math.max(HEIGHT * 0.06, (levels[i] ?? 0) * HEIGHT * 0.92);
      g.globalAlpha = 0.25 + 0.75 * fade;
      g.beginPath();
      g.roundRect(i * step + (step - barWidth) / 2, (HEIGHT - height) / 2, barWidth, height, barWidth / 2);
      g.fill();
    }
    g.globalAlpha = 1;
  }, [levels]);
  return canvas.element;
}

// Three sine lines that swell in the middle and taper at both ends; the
// voice level sets their height and line weight.
const DITHER_FIELD = `
float field(vec2 uv, vec2 res, float t) {
  float taper = pow(sin(3.14159265 * uv.x), 1.6);
  float tone = 0.0;
  for (int k = 0; k < 3; k++) {
    float fk = float(k);
    float amp = (0.08 + 0.38 * pillLevel) * taper * (1.0 - fk * 0.22);
    float y = 0.5 + amp * sin(uv.x * (7.0 + fk * 3.0) + pillTime * (2.2 + fk * 0.9) + fk * 1.7);
    float d = (uv.y - y) * res.y;
    tone += exp(-(d * d) / (0.55 + 0.25 * pillLevel)) * (1.0 - fk * 0.25);
  }
  return min(1.0, tone * (0.55 + 0.45 * taper));
}
`;

const DITHER_PALETTE: DitherPalette = ["transparent", "#ffffff"];
const DITHER_CELL = 2;

/**
 * The engine fills whole cells, so neighbouring cells merge into solid
 * lines. This mask leaves one device pixel between cells so they read as
 * dots. The engine rounds a cell to whole device pixels; so does the mask.
 * At 100% scaling a cell is two device pixels and a gap would halve every
 * dot, so cells stay solid there.
 */
function dotMask(): CSSProperties {
  const dpr = window.devicePixelRatio || 1;
  const device = Math.max(1, Math.round(DITHER_CELL * dpr));
  if (device < 3) return {};
  const cell = device / dpr;
  const dot = (device - 1) / dpr;
  const image = `linear-gradient(to right, #000 ${dot}px, transparent ${dot}px), linear-gradient(to bottom, #000 ${dot}px, transparent ${dot}px)`;
  return {
    maskImage: image,
    WebkitMaskImage: image,
    maskSize: `${cell}px ${cell}px`,
    WebkitMaskSize: `${cell}px ${cell}px`,
    maskComposite: "intersect",
    WebkitMaskComposite: "source-in",
  };
}

let webgl: boolean | undefined;

function hasWebGL() {
  if (webgl === undefined) {
    const gl = document.createElement("canvas").getContext("webgl");
    webgl = gl !== null;
    gl?.getExtension("WEBGL_lose_context")?.loseContext();
  }
  return webgl;
}

/**
 * A sound wave drawn as dither dots, on the dithercn engine. The engine's
 * own loop stays off: the shared tick sets the phase and level uniforms,
 * and the engine redraws a still shader whenever its props change, so the
 * shader element changes only when the wave does. Without WebGL it shows
 * the Waveform instead.
 */
export function DitherMeter({ levels, reduced }: MeterProps) {
  const gl = hasWebGL();
  const wave = useWave(levels.at(-1) ?? 0, !reduced && gl);
  // While the wave moves, new readings reach the fallback with the next tick.
  const fallbackKey = wave.running ? wave.t : levels;
  const fallback = useMemo(() => <WaveformMeter levels={levels} reduced={reduced} />, [fallbackKey, reduced]);
  const shader = useMemo(
    () => (
      <DitherShader
        field={DITHER_FIELD}
        uniforms={{ pillLevel: wave.level, pillTime: wave.t }}
        palette={DITHER_PALETTE}
        matrix="bayer4"
        cell={DITHER_CELL}
        animate={false}
        fallback={fallback}
        className="pill-dither shrink-0"
        style={{ width: WIDTH, height: HEIGHT, ...dotMask() }}
      />
    ),
    [wave.level, wave.t, fallback],
  );
  return gl ? shader : fallback;
}

const GLOW_COLORS = ["#ff9f0a", "#30d158", "#0a84ff", "#bf5af2"];

function drawGlow(g: CanvasRenderingContext2D, scale: number, level: number, t: number) {
  const mid = HEIGHT / 2;
  g.globalCompositeOperation = "lighter";
  GLOW_COLORS.forEach((color, k) => {
    const amp = (0.06 + 0.42 * level) * HEIGHT * (1 - k * 0.12);
    const wave = (x: number, shift: number) => {
      const u = x / WIDTH;
      return Math.pow(Math.sin(Math.PI * u), 2) * Math.sin(u * (6 + k * 1.5) + t * (2.6 + k * 0.7) + k * 2.1 + shift);
    };
    g.beginPath();
    for (let x = 0; x <= WIDTH; x += 1) g.lineTo(x, mid + amp * wave(x, 0));
    for (let x = WIDTH; x >= 0; x -= 1) g.lineTo(x, mid - amp * 0.55 * wave(x, 0.6));
    g.closePath();
    g.fillStyle = color;
    g.globalAlpha = 0.5;
    // Shadow blur is in canvas pixels, not CSS pixels.
    g.shadowColor = color;
    g.shadowBlur = 4.5 * scale;
    g.fill();
  });
  g.globalAlpha = 0.9;
  g.shadowBlur = 2.5 * scale;
  g.shadowColor = "#fff";
  g.fillStyle = "#fff";
  g.fillRect(WIDTH * 0.18, mid - 0.375, WIDTH * 0.64, 0.75);
  g.globalAlpha = 1;
  g.shadowBlur = 0;
  g.globalCompositeOperation = "source-over";
}

/** A soft multicolour wave, Siri style. */
export function GlowMeter({ levels, reduced }: MeterProps) {
  const canvas = useCanvas();
  const wave = useWave(levels.at(-1) ?? 0, !reduced);
  useEffect(() => {
    const c = canvas.begin();
    if (c) drawGlow(c.g, c.scale, wave.level, wave.t);
  }, [wave.level, wave.t]);
  return canvas.element;
}

export const METERS: Record<PillStyle, (props: MeterProps) => ReactElement> = {
  bars: BarsMeter,
  waveform: WaveformMeter,
  dither: DitherMeter,
  glow: GlowMeter,
};
