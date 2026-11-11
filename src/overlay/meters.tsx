// The voice meters the recording pill can show, also previewed in Settings.
// Each takes recent voice readings (0..1, newest last). Nothing here runs
// once the meter unmounts, and under reduced motion each draws one still frame.
import { useEffect, useMemo, useRef, type ReactElement } from "react";
import { DitherShader } from "@/lib/dither/shader";
import { useFrameLoop } from "@/lib/dither/motion";
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
// The reduced-motion frame: a voice mid-sentence.
const STILL_LEVEL = 0.45;
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
  return { ref, begin, element };
}

/**
 * Eases toward the newest reading as frames are drawn: a quarter of the way
 * per 60 Hz frame, whatever the actual frame rate.
 */
function useEased(target: number) {
  const state = useRef({ target, value: 0, at: 0 });
  useEffect(() => {
    state.current.target = target;
  }, [target]);
  return useMemo(
    () => () => {
      const s = state.current;
      const now = performance.now();
      const frames = s.at ? Math.min(6, (now - s.at) / (1000 / 60)) : 1;
      s.at = now;
      s.value += (s.target - s.value) * (1 - Math.pow(0.75, frames));
      return s.value;
    },
    [],
  );
}

const STILL_WAVE = Array.from({ length: METER_HISTORY }, (_, i) => {
  const u = i / (METER_HISTORY - 1);
  return 0.12 + 0.6 * Math.sin(Math.PI * u) * Math.abs(Math.sin(i * 0.9 + 0.4));
});

/** Thin bars mirrored around the centre line, fading at both ends. Redraws only when a reading arrives. */
export function WaveformMeter({ levels, reduced }: MeterProps) {
  const canvas = useCanvas();
  const shown = reduced ? STILL_WAVE : levels;
  useEffect(() => {
    const c = canvas.begin();
    if (!c) return;
    const { g } = c;
    const n = shown.length;
    const step = WIDTH / n;
    const barWidth = Math.max(0.5, step * 0.55);
    g.fillStyle = "#fff";
    for (let i = 0; i < n; i++) {
      const fade = Math.min(1, Math.min(i, n - 1 - i) / (n * 0.3));
      const height = Math.max(HEIGHT * 0.06, (shown[i] ?? 0) * HEIGHT * 0.92);
      g.globalAlpha = 0.25 + 0.75 * fade;
      g.beginPath();
      g.roundRect(i * step + (step - barWidth) / 2, (HEIGHT - height) / 2, barWidth, height, barWidth / 2);
      g.fill();
    }
    g.globalAlpha = 1;
  });
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
    float y = 0.5 + amp * sin(uv.x * (7.0 + fk * 3.0) + t * (2.2 + fk * 0.9) + fk * 1.7);
    float d = (uv.y - y) * res.y;
    tone += exp(-(d * d) / (0.55 + 0.25 * pillLevel)) * (1.0 - fk * 0.25);
  }
  return min(1.0, tone * (0.55 + 0.45 * taper));
}
`;

const DITHER_PALETTE: DitherPalette = ["transparent", "#ffffff"];
const DITHER_CELL = 2;
const STILL_UNIFORMS = { pillLevel: STILL_LEVEL };

/**
 * The engine fills whole cells, so neighbouring cells merge into solid
 * lines. This mask leaves one device pixel between cells so they read as
 * dots. The engine rounds a cell to whole device pixels; so does the mask.
 * At 100% scaling a cell is two device pixels and a gap would halve every
 * dot, so cells stay solid there.
 */
function dotMask(): React.CSSProperties {
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

/** A sound wave drawn as dither dots, on the dithercn engine. */
export function DitherMeter({ levels, reduced }: MeterProps) {
  const eased = useEased(levels.at(-1) ?? 0);
  // A getter, so the shader reads the eased level on every frame it draws
  // without re-rendering this component per frame.
  const live = useMemo(
    () => ({
      get pillLevel() {
        return eased();
      },
    }),
    [eased],
  );
  return (
    <DitherShader
      field={DITHER_FIELD}
      uniforms={reduced ? STILL_UNIFORMS : live}
      palette={DITHER_PALETTE}
      matrix="bayer4"
      cell={DITHER_CELL}
      animate={!reduced}
      stillTime={STILL_TIME}
      className="shrink-0"
      style={{ width: WIDTH, height: HEIGHT, ...dotMask() }}
    />
  );
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
  const eased = useEased(levels.at(-1) ?? 0);
  useFrameLoop(
    (time) => {
      const c = canvas.begin();
      if (c) drawGlow(c.g, c.scale, reduced ? STILL_LEVEL : eased(), time);
    },
    { target: canvas.ref, enabled: !reduced, fps: 30, stillTime: STILL_TIME },
  );
  return canvas.element;
}

export const METERS: Record<PillStyle, (props: MeterProps) => ReactElement> = {
  bars: BarsMeter,
  waveform: WaveformMeter,
  dither: DitherMeter,
  glow: GlowMeter,
};

/** A fake voice for previews: syllable bursts with pauses between phrases. */
export function fakeLevel(seconds: number): number {
  const phrase = Math.sin(seconds * 0.9) + Math.sin(seconds * 0.37 + 1) > -0.6 ? 1 : 0.05;
  const syllable = Math.max(0, Math.sin(seconds * 9.5) * 0.55 + Math.sin(seconds * 5.3 + 2) * 0.35 + 0.25);
  return Math.min(1, Math.max(0, phrase * syllable * 0.85));
}
