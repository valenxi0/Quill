// Dot scope's plate: Hex's dot field (src/rune_hud.wgsl, MIT, see NOTICE.md)
// drawn with canvas 2D. The plate itself is the element around the canvas,
// so text can sit beside the field on the same plate.
import { useEffect, useLayoutEffect, useRef, type ReactNode } from "react";
import { onTick, useDocumentVisible } from "../ticker";
import { palette, PLATE, RIM, Scope, SHADOW, smoothstep, tuningFor, type Pose, type Stage } from "./scope";

export interface DotScopeProps {
  stage: Stage;
  /** The microphone's loudness now, dBFS; read on every tick. */
  loudness: () => number;
  reduced: boolean;
  /** Shown on the plate, right of the field. */
  children?: ReactNode;
}

const CORE_STEPS = 16;

function roundedBox(px: number, py: number, hx: number, hy: number, r: number) {
  const qx = Math.abs(px) - hx + r;
  const qy = Math.abs(py) - hy + r;
  return Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) + Math.min(Math.max(qx, qy), 0) - r;
}

/** `c` mixed toward `d` by `t`, as a CSS colour. */
function rgb(c: readonly number[], t: number, d: readonly number[], alpha = 1) {
  const channel = (k: number) => Math.round((c[k]! + (d[k]! - c[k]!) * t) * 255);
  return `rgb(${channel(0)} ${channel(1)} ${channel(2)} / ${alpha})`;
}

function draw(canvas: HTMLCanvasElement, plate: HTMLElement, scope: Scope, pose: Pose, hasText: boolean) {
  const dpr = window.devicePixelRatio || 1;
  const { width, height } = pose;
  const cssWidth = `${width.toFixed(2)}px`;
  if (canvas.style.width !== cssWidth) canvas.style.width = cssWidth;
  plate.style.opacity = pose.opacity.toFixed(3);
  plate.style.transform = pose.scale === 1 ? "" : `scale(${pose.scale.toFixed(4)})`;
  plate.style.filter = pose.softness > 0.01 ? `blur(${pose.softness.toFixed(2)}px)` : "";
  // As the field collapses the text keeps a margin from the plate's end.
  plate.style.paddingLeft = hasText ? `${Math.max(0, 12 - width).toFixed(2)}px` : "";

  const w = Math.max(1, Math.round(width * dpr));
  const h = Math.round(height * dpr);
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
  }
  const g = canvas.getContext("2d");
  if (!g) return;
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.clearRect(0, 0, width, height);
  if (width < 1) return;
  const cx = width / 2;
  const cy = height / 2;
  const { base, core } = palette(pose);

  if (pose.glow > 0.01) {
    const reach = 34 + 20 * pose.glow;
    const strength = 0.3 * 0.55 * Math.min(1, pose.glow);
    const gradient = g.createRadialGradient(cx, cy, 0, cx, cy, reach);
    gradient.addColorStop(0, rgb(base, 0.5, core, strength));
    gradient.addColorStop(1, rgb(base, 0, base, 0));
    g.fillStyle = gradient;
    g.fillRect(0, 0, width, height);
  }

  const { columns, rows, dots } = scope;
  const pitch = scope.tuning.spacing;
  const hx = width / 2;
  const hy = height / 2;
  const radius = hy;
  const ox = (columns - 1) / 2;
  const oy = (rows - 1) / 2;
  const presence = 0.5 + 0.5 * Math.max(pose.live, pose.cool);
  const colors = Array.from({ length: CORE_STEPS + 1 }, (_, k) => rgb(base, k / CORE_STEPS, core));
  const unlit = 0.07;
  const full = 0.96;
  const curve = 0.9;

  // Halos first, then dots, so a halo never dims a neighbour's dot.
  for (let pass = 0; pass < 2; pass++) {
    for (let j = 0; j < rows; j++) {
      const y = (j - oy) * pitch;
      const row = Math.abs(y) / hy;
      const floorAlpha = unlit * (1 - 0.7 * smoothstep(0.35, 0.95, row)) * presence * pose.grid;
      // Unlit dots in a row share one colour and alpha: one path for all.
      const floor = pass === 1 ? new Path2D() : null;
      let floorDots = 0;
      for (let i = 0; i < columns; i++) {
        const x = (i - ox) * pitch;
        const mask = smoothstep(1.4, 4.6, -roundedBox(x, y, hx, hy, radius));
        if (mask <= 0) continue;
        const n = j * columns + i;
        const power = Math.max(dots[n * 2]!, 1e-5);
        const fresh = dots[n * 2 + 1]!;
        if (pass === 0) {
          if (power > 0.05) {
            const r = dotRadius(power, fresh, pitch) * 2;
            g.globalAlpha = 0.132 * power * power * mask;
            g.fillStyle = colors[coreIndex(power, fresh)]!;
            g.beginPath();
            g.arc(cx + x, cy + y, r, 0, Math.PI * 2);
            g.fill();
          }
          continue;
        }
        if (power < 0.01 && mask >= 0.999) {
          floor!.moveTo(cx + x + 0.25 * pitch, cy + y);
          floor!.arc(cx + x, cy + y, 0.25 * pitch, 0, Math.PI * 2);
          floorDots++;
          continue;
        }
        const alpha = floorAlpha + (full - floorAlpha) * Math.pow(power, curve * (1.4 + (1 - 1.4) * fresh));
        g.globalAlpha = Math.min(1, alpha * mask);
        g.fillStyle = colors[coreIndex(power, fresh)]!;
        g.beginPath();
        g.arc(cx + x, cy + y, dotRadius(power, fresh, pitch), 0, Math.PI * 2);
        g.fill();
      }
      if (floor && floorDots > 0) {
        g.globalAlpha = floorAlpha;
        g.fillStyle = colors[0]!;
        g.fill(floor);
      }
    }
  }
  g.globalAlpha = 1;
}

function dotRadius(power: number, fresh: number, pitch: number) {
  const shrink = 1 + (0.82 + 0.18 * fresh - 1) * smoothstep(0.03, 0.2, power);
  return (0.25 + 0.17 * Math.pow(power, 0.6)) * shrink * pitch;
}

function coreIndex(power: number, fresh: number) {
  const core = Math.min(Math.max(power * power * 1.1, 0), 0.9) * (0.3 + 0.7 * fresh);
  return Math.round(core * CORE_STEPS);
}

/**
 * The plate and its field. A new plate starts a recording from nothing:
 * the field grows from the middle as the trace unfolds. It ticks at 30 Hz
 * while the window is visible, and stops when unmounted.
 */
export function DotScope({ stage, loudness, reduced, children }: DotScopeProps) {
  const plate = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const scope = useRef<Scope | null>(null);
  scope.current ??= new Scope(tuningFor(window.devicePixelRatio || 1), reduced);
  const live = useRef({ stage, loudness, reduced, hasText: children != null });
  useLayoutEffect(() => {
    live.current = { stage, loudness, reduced, hasText: children != null };
  });
  const visible = useDocumentVisible();
  const tuning = scope.current.tuning;

  const frame = (dt: number) => {
    const s = scope.current;
    if (!s || !canvas.current || !plate.current) return;
    const { stage, loudness, reduced, hasText } = live.current;
    s.advance(dt, { stage, loudness: loudness(), reduceMotion: reduced });
    draw(canvas.current, plate.current, s, s.pose(), hasText);
  };

  useLayoutEffect(() => frame(0), []);

  useEffect(() => {
    if (!visible) return;
    let last = performance.now();
    return onTick(() => {
      const now = performance.now();
      frame((now - last) / 1000);
      last = now;
    });
  }, [visible]);

  return (
    <div
      ref={plate}
      className="flex shrink-0 items-center gap-2 overflow-hidden text-[12.5px] font-medium text-white"
      style={{
        height: tuning.height,
        borderRadius: tuning.height / 2,
        background: PLATE,
        boxShadow: `inset 0 0 0 1px ${RIM}, 0 3px 12px ${SHADOW}`,
        paddingRight: children != null ? 12 : 0,
        opacity: 0,
      }}
    >
      <canvas ref={canvas} aria-hidden="true" className="block shrink-0" style={{ height: tuning.height }} />
      {children}
    </div>
  );
}
