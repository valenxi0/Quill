import { type Insights } from "../lib/api";
import { compact, duration, monthDay } from "../lib/format";
import type { Ctx } from "./App";
import { Card, cx, Icon } from "./ui";
import { useInsights } from "./VoiceTab";
import { AppIcon } from "./AppScope";

export function InsightsTab({ ctx }: { ctx: Ctx }) {
  const insights = useInsights(ctx.state);
  if (!insights) return null;
  if (!ctx.state.settings.voiceDictation.insightsEnabled && insights.isEmpty) {
    return (
      <Card className="py-10 text-center">
        <h3 className="text-[15px] font-semibold">Insights are off</h3>
        <p className="mt-1 text-[12.5px] text-text-2">Turn them on under Privacy and data to count words and time saved.</p>
      </Card>
    );
  }
  return insights.isEmpty ? <Empty hotkey={ctx.state.hotkeyDisplay} /> : <Filled insights={insights} />;
}

function Empty({ hotkey }: { hotkey: string }) {
  return (
    <Card className="flex flex-col items-center py-12 text-center">
      <div className="mb-3 text-accent">
        <Icon.Waveform size={36} />
      </div>
      <h3 className="text-[15px] font-semibold">Your first dictation starts the count</h3>
      <p className="mt-1 max-w-[360px] text-[12.5px] text-text-2">
        Hold {hotkey}, speak, and release. Quill counts words and time saved, never the words themselves.
      </p>
    </Card>
  );
}

const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

function Filled({ insights: i }: { insights: Insights }) {
  const days = i.heatmap.filter((c) => !c.future).slice(-7);
  return (
    <>
      <div className="grid grid-cols-[1.35fr_1fr] gap-[10px]">
        <Card className="flex flex-col px-5 py-4">
          <p className="text-[12px] text-text-2">This week</p>
          <p className="mt-0.5 flex items-baseline gap-2">
            <span className="text-[34px] leading-none font-semibold tracking-[-0.02em] tabular-nums">{compact(i.weekWords)}</span>
            <span className="text-[14px] font-medium text-text-2">words, {duration(i.weekSavedSeconds)} of typing saved</span>
          </p>
          <WeekChart days={days} />
        </Card>
        <Card className="flex flex-col p-0">
          <div className="flex flex-1 flex-col justify-center px-[18px] py-3">
            <p className="text-[12px] text-text-2">Speaking speed</p>
            <p className="text-[22px] font-semibold tabular-nums">
              {Math.round(i.wpm)} <span className="text-[13px] font-medium text-text-2">wpm</span>
            </p>
          </div>
          <div className="flex flex-1 flex-col justify-center border-t border-hairline px-[18px] py-3">
            <p className="text-[12px] text-text-2">Saved this month</p>
            <p className="text-[22px] font-semibold tabular-nums">{duration(i.monthSavedSeconds)}</p>
            <p className="text-[11.5px] text-text-3 tabular-nums" title="Compared with typing at 40 wpm">
              {compact(i.totalWords)} words{i.since ? ` since ${monthDay(i.since)}` : ""}
            </p>
          </div>
        </Card>
      </div>
      <div className="grid grid-cols-2 gap-[10px]">
        <Card>
          <h2 className="mb-3 text-[13px] font-semibold">Where you dictate</h2>
          {i.apps.length === 0 ? (
            <p className="text-[12px] text-text-3">No dictations this month yet.</p>
          ) : (
            <div className="flex flex-col gap-2.5">
              {i.apps.slice(0, 6).map((a) => (
                <div key={a.exe} className="flex items-center gap-2.5">
                  <AppIcon app={a} />
                  <span className="min-w-0 flex-1 truncate text-[12.5px]">{a.name}</span>
                  <div className="h-[4px] w-[84px] shrink-0 overflow-hidden rounded-full bg-fill-strong">
                    <div className="h-full rounded-full bg-accent" style={{ width: `${Math.max(3, (a.words / i.apps[0]!.words) * 100)}%` }} />
                  </div>
                  <span className="w-11 text-right text-[12px] text-text-2 tabular-nums">{compact(a.words)}</span>
                </div>
              ))}
            </div>
          )}
        </Card>
        <Card>
          <div className="mb-3 flex items-baseline gap-2">
            <h2 className="flex-1 text-[13px] font-semibold tabular-nums">
              {i.currentStreak} day streak
            </h2>
            <span className="text-[12px] text-text-3 tabular-nums">Longest {i.longestStreak}</span>
          </div>
          <Heatmap cells={i.heatmap} />
        </Card>
      </div>
      <p className="px-0.5 text-[11.5px] text-text-3">Counts only, stored on this PC. Change this under Privacy and data.</p>
    </>
  );
}

/** Words per day for the last seven days, as a filled line. */
function WeekChart({ days }: { days: Insights["heatmap"] }) {
  if (days.length < 2) return <div className="mt-4 h-[74px]" />;
  const max = Math.max(1, ...days.map((d) => d.words));
  const x = (k: number) => (k / (days.length - 1)) * 300;
  const y = (w: number) => 52 - (w / max) * 46;
  const line = days.map((d, k) => `${k ? "L" : "M"}${x(k)} ${y(d.words)}`).join(" ");
  return (
    <div className="mt-auto pt-4">
      <svg viewBox="0 0 300 56" preserveAspectRatio="none" className="h-[56px] w-full" aria-hidden="true">
        <defs>
          <linearGradient id="week-fill" x1="0" x2="0" y1="0" y2="1">
            <stop offset="0" stopColor="var(--accent)" stopOpacity="0.28" />
            <stop offset="1" stopColor="var(--accent)" stopOpacity="0" />
          </linearGradient>
        </defs>
        <path d={`${line} L300 56 L0 56Z`} fill="url(#week-fill)" />
        <path d={line} fill="none" stroke="var(--accent)" strokeWidth={2} vectorEffect="non-scaling-stroke" strokeLinejoin="round" />
      </svg>
      <div className="mt-1 flex justify-between text-[11.5px] text-text-3">
        {days.map((d) => (
          <span key={d.date} title={`${monthDay(d.date)}: ${d.words} words`}>
            {WEEKDAYS[new Date(`${d.date}T12:00:00`).getDay()]}
          </span>
        ))}
      </div>
    </div>
  );
}

function Heatmap({ cells }: { cells: Insights["heatmap"] }) {
  const max = Math.max(1, ...cells.map((c) => c.words));
  const level = (w: number) => (w === 0 ? 0 : Math.min(4, Math.ceil((w / max) * 4)));
  const weeks = Array.from({ length: Math.ceil(cells.length / 7) }, (_, w) => cells.slice(w * 7, w * 7 + 7));
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
                className={cx("aspect-square rounded-[2px]", c.future && "opacity-0")}
                style={{ background: shades[level(c.words)] }}
              />
            ))}
          </div>
        ))}
      </div>
      <div className="mt-2 flex items-center justify-end gap-1 text-[11px] text-text-3">
        Less
        {shades.map((s) => (
          <span key={s} className="h-[9px] w-[9px] rounded-[2px]" style={{ background: s }} />
        ))}
        More
      </div>
    </div>
  );
}
