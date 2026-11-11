import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { Waveform } from "@phosphor-icons/react";
import { api, type PillStyle } from "../lib/api";
import { DotScope } from "../overlay/dotScope/DotScope";
import { demoLoudness } from "../overlay/dotScope/scope";
import { METER_HISTORY, METERS, type MeterStyle } from "../overlay/meters";
import { useDocumentVisible } from "../overlay/ticker";
import type { Ctx } from "./App";
import { Card, CardHeader, cx } from "./ui";

const STYLES: { value: PillStyle; label: string }[] = [
  { value: "bars", label: "Bars" },
  { value: "waveform", label: "Waveform" },
  { value: "dots", label: "Dots" },
  { value: "glow", label: "Glow" },
];

/** A fake voice for the previews: syllable bursts with pauses between phrases. */
function fakeLevel(seconds: number): number {
  const phrase = Math.sin(seconds * 0.9) + Math.sin(seconds * 0.37 + 1) > -0.6 ? 1 : 0.05;
  const syllable = Math.max(0, Math.sin(seconds * 9.5) * 0.55 + Math.sin(seconds * 5.3 + 2) * 0.35 + 0.25);
  return Math.min(1, Math.max(0, phrase * syllable * 0.85));
}

/** Same cadence as the microphone readings while recording. */
const TICK_MS = 83;

function fakeHistory(seconds: number) {
  return Array.from({ length: METER_HISTORY }, (_, i) => fakeLevel(seconds - (METER_HISTORY - 1 - i) * (TICK_MS / 1000)));
}

/** Fake readings for the previews. They only tick while Settings is visible. */
function useFakeLevels(running: boolean) {
  const [levels, setLevels] = useState(() => fakeHistory(4));
  useEffect(() => {
    if (!running) return;
    const start = performance.now();
    const id = setInterval(() => {
      const seconds = 4 + (performance.now() - start) / 1000;
      setLevels((prev) => [...prev.slice(1), fakeLevel(seconds)]);
    }, TICK_MS);
    return () => clearInterval(id);
  }, [running]);
  return levels;
}

/** As wide as the other cards' pills, so all four fit the narrowest Settings window. */
const DOTS_PREVIEW_WIDTH = 88;

const pickerStart = performance.now();

/** Hex Lab's synthetic voice, for the Dots preview. */
function demoVoice() {
  return demoLoudness((performance.now() - pickerStart) / 1000);
}

function MiniPill({ children }: { children: React.ReactNode }) {
  return (
    <span className="flex h-[40px] items-center rounded-full px-3" style={{ background: "rgba(10, 10, 10, 0.92)", border: "1px solid #ff453a" }}>
      {children}
    </span>
  );
}

function Meter({ style, levels, reduced }: { style: MeterStyle; levels: number[]; reduced: boolean }) {
  const Draw = METERS[style];
  return <Draw levels={levels} reduced={reduced} />;
}

export function PillPicker({ ctx }: { ctx: Ctx }) {
  const value = ctx.state.settings.voiceDictation.pillStyle;
  const [reduced, setReduced] = useState(false);
  useEffect(() => {
    api.animationsEnabled().then((on) => setReduced(!on)).catch(() => {});
  }, []);
  const visible = useDocumentVisible();
  const levels = useFakeLevels(visible);
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);

  const pick = (next: PillStyle) => {
    if (next !== value) ctx.saveVd({ pillStyle: next });
  };
  const onKeyDown = (e: KeyboardEvent) => {
    const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    if (!step) return;
    e.preventDefault();
    const index = Math.max(0, STYLES.findIndex((s) => s.value === value));
    const next = (index + step + STYLES.length) % STYLES.length;
    pick(STYLES[next]!.value);
    buttons.current[next]?.focus();
  };

  return (
    <Card>
      <CardHeader icon={<Waveform size={19} />} title="Recording pill" detail="How the pill shows your voice while you record. Applies from the next recording." />
      <div role="radiogroup" aria-label="Recording pill" className="mt-3 ml-8 grid grid-cols-4 gap-2" onKeyDown={onKeyDown}>
        {STYLES.map((s, i) => {
          const selected = s.value === value;
          return (
            <button
              key={s.value}
              ref={(el) => {
                buttons.current[i] = el;
              }}
              role="radio"
              aria-checked={selected}
              tabIndex={selected ? 0 : -1}
              onClick={() => pick(s.value)}
              className={cx(
                "flex min-w-0 flex-col items-center gap-2 rounded-[8px] bg-fill px-1 pt-3 pb-2 transition-[box-shadow,background-color] duration-150 hover:bg-fill-strong",
                selected ? "shadow-[0_0_0_2px_var(--accent)]" : "shadow-[0_0_0_1px_var(--card-border)]",
              )}
            >
              <span aria-hidden="true" className="flex h-[40px] items-center">
                {s.value === "dots" ? (
                  <DotScope stage="recording" loudness={demoVoice} reduced={reduced} width={DOTS_PREVIEW_WIDTH} />
                ) : (
                  <MiniPill>
                    <Meter style={s.value} levels={levels} reduced={reduced} />
                  </MiniPill>
                )}
              </span>
              <span className={cx("text-[12.5px]", selected ? "font-semibold" : "text-text-2")}>{s.label}</span>
            </button>
          );
        })}
      </div>
    </Card>
  );
}
