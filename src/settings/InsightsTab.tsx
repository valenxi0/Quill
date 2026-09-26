import { useState } from "react";
import { api, type Insights } from "../lib/api";
import { compact, duration, monthDay } from "../lib/format";
import type { Ctx } from "./App";
import { Button, Card, cx, Icon, Sheet, Switch } from "./ui";
import { useInsights } from "./VoiceTab";

export function InsightsTab({ ctx }: { ctx: Ctx }) {
  const insights = useInsights(ctx.state);
  const [confirmReset, setConfirmReset] = useState(false);
  if (!insights) return null;
  return (
    <div className="flex flex-col gap-[10px]">
      {insights.isEmpty ? <Empty hotkey={ctx.state.hotkeyDisplay} /> : <Filled insights={insights} />}
      <div className="mt-1 flex items-center gap-3 px-1">
        <p className="flex-1 text-[11.5px] text-text-3">
          Insights keep counts only: words, seconds, and the app you dictated into. Never your words. Stored on this PC.
        </p>
        <span className="text-[12px] text-text-2">Keep insights</span>
        <Switch
          label="Keep insights"
          checked={ctx.state.settings.voiceDictation.insightsEnabled}
          onChange={(v) => ctx.saveVd({ insightsEnabled: v })}
        />
        <Button small onClick={() => setConfirmReset(true)}>
          Reset…
        </Button>
      </div>
      {confirmReset && (
        <Sheet
          title="Reset voice insights?"
          width={380}
          onClose={() => setConfirmReset(false)}
          footer={
            <>
              <Button onClick={() => setConfirmReset(false)}>Cancel</Button>
              <Button
                kind="destructive"
                onClick={() => {
                  api.resetInsights();
                  setConfirmReset(false);
                }}
              >
                Reset
              </Button>
            </>
          }
        >
          <p className="text-[12.5px] text-text-2">Totals, streaks, and app counts are cleared. Your dictionary and settings stay.</p>
        </Sheet>
      )}
    </div>
  );
}

function Empty({ hotkey }: { hotkey: string }) {
  return (
    <Card className="flex flex-col items-center py-12 text-center">
      <div className="mb-3 text-accent">
        <Icon.Waveform size={36} />
      </div>
      <h3 className="text-[15px] font-semibold">Insights start with your first dictation</h3>
      <p className="mt-1 max-w-[360px] text-[12.5px] text-text-2">
        Hold {hotkey} and speak. Quill counts words and time, never the words themselves.
      </p>
    </Card>
  );
}

function Filled({ insights: i }: { insights: Insights }) {
  return (
    <>
      <p className="px-1 text-[14px]">
        This week you spoke <b className="text-accent">{compact(i.weekWords)} words</b> and saved{" "}
        <b className="text-accent">{duration(i.weekSavedSeconds)}</b> of typing.
      </p>
      <div className="grid grid-cols-3 gap-[10px]">
        <div
          className="rounded-[12px] px-4 py-3.5 text-white"
          style={{ background: "linear-gradient(135deg, #3d9bff 0%, #0a60e0 100%)" }}
        >
          <p className="text-[12px] opacity-85">Speaking speed</p>
          <p className="mt-1 text-[26px] font-semibold tabular-nums">
            {Math.round(i.wpm)} <span className="text-[13px] font-medium opacity-85">wpm</span>
          </p>
          <Sparkline values={i.sparkline} />
        </div>
        <Card>
          <p className="text-[12px] text-text-2">Time saved</p>
          <p className="mt-1 text-[26px] font-semibold tabular-nums">{duration(i.monthSavedSeconds)}</p>
          <p className="text-[11.5px] text-text-3">This month, against 40 wpm typing</p>
        </Card>
        <Card>
          <p className="text-[12px] text-text-2">Words dictated</p>
          <p className="mt-1 text-[26px] font-semibold tabular-nums">{compact(i.totalWords)}</p>
          <p className="text-[11.5px] text-text-3">{i.since ? `Since ${monthDay(i.since)}` : ""}</p>
        </Card>
      </div>
      <div className="grid grid-cols-[1fr_1.25fr] gap-[10px]">
        <Card>
          <p className="mb-2 text-[13px] font-semibold">Where you dictate</p>
          {i.apps.length === 0 ? (
            <p className="text-[12px] text-text-3">Nothing this month yet.</p>
          ) : (
            <div className="flex flex-col gap-2">
              {i.apps.slice(0, 6).map((a) => (
                <div key={a.exe} className="flex items-center gap-2">
                  <AppGlyph name={a.name} />
                  <div className="min-w-0 flex-1">
                    <div className="flex justify-between text-[12px]">
                      <span className="truncate">{a.name}</span>
                      <span className="text-text-3 tabular-nums">{compact(a.words)}</span>
                    </div>
                    <div className="mt-1 h-[4px] rounded-full bg-fill-strong">
                      <div className="h-full rounded-full bg-accent" style={{ width: `${(a.words / i.apps[0]!.words) * 100}%` }} />
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </Card>
        <Card>
          <div className="mb-2 flex items-baseline gap-2">
            <span className="text-alert">
              <Icon.Flame />
            </span>
            <span className="text-[20px] font-semibold tabular-nums">{i.currentStreak}</span>
            <span className="text-[12px] text-text-2">day streak</span>
            <span className="flex-1" />
            <span className="text-[12px] text-text-3">Longest {i.longestStreak}</span>
          </div>
          <Heatmap cells={i.heatmap} />
        </Card>
      </div>
    </>
  );
}

function AppGlyph({ name }: { name: string }) {
  const hue = [...name].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 7);
  return (
    <span
      className="flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded-[6px] text-[11px] font-semibold text-white"
      style={{ background: `hsl(${hue} 55% 50%)` }}
    >
      {name.slice(0, 1).toUpperCase()}
    </span>
  );
}

function Sparkline({ values }: { values: number[] }) {
  if (values.length < 2) return <div className="mt-2 h-[28px]" />;
  const max = Math.max(...values);
  const min = Math.min(...values);
  const span = max - min || 1;
  const pts = values.map((v, k) => `${(k / (values.length - 1)) * 100},${26 - ((v - min) / span) * 22}`).join(" ");
  return (
    <svg viewBox="0 0 100 28" preserveAspectRatio="none" className="mt-2 h-[28px] w-full">
      <polyline points={pts} fill="none" stroke="white" strokeOpacity={0.9} strokeWidth={1.6} vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

function Heatmap({ cells }: { cells: Insights["heatmap"] }) {
  const max = Math.max(1, ...cells.map((c) => c.words));
  const level = (w: number) => (w === 0 ? 0 : Math.min(4, Math.ceil((w / max) * 4)));
  const weeks = Array.from({ length: 15 }, (_, w) => cells.slice(w * 7, w * 7 + 7));
  const shades = ["var(--fill-strong)", "rgb(10 132 255 / 0.3)", "rgb(10 132 255 / 0.5)", "rgb(10 132 255 / 0.75)", "var(--accent)"];
  return (
    <div>
      <div className="flex gap-[3px]">
        {weeks.map((week, w) => (
          <div key={w} className="flex flex-1 flex-col gap-[3px]">
            {week.map((c) => (
              <div
                key={c.date}
                title={c.future ? undefined : `${monthDay(c.date)}: ${c.words} words`}
                className={cx("aspect-square rounded-[3px]", c.future && "opacity-0")}
                style={{ background: shades[level(c.words)] }}
              />
            ))}
          </div>
        ))}
      </div>
      <div className="mt-2 flex items-center justify-end gap-1 text-[11px] text-text-3">
        Less
        {shades.map((s) => (
          <span key={s} className="h-[10px] w-[10px] rounded-[2px]" style={{ background: s }} />
        ))}
        More
      </div>
    </div>
  );
}
