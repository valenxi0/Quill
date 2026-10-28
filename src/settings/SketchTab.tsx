// Settings → Sketch: talk while marking up the screen, then paste the
// screenshots and the transcript together.
import { Command, Hand, PencilSimpleLine } from "@phosphor-icons/react";
import type { DrawKey, SketchSettings } from "../lib/api";
import type { Ctx } from "./App";
import { ShortcutButton } from "./SetupTab";
import { Card, CardHeader, Divider, Section, Segmented, Switch, Warning } from "./ui";

export function SketchTab({ ctx }: { ctx: Ctx }) {
  const sketch = ctx.state.settings.sketch;
  const { capturingSketchHotkey, captureError, sketchHotkeyProblem, sketchHotkeyDisplay } = ctx.state;
  const save = (patch: Partial<SketchSettings>) => ctx.save({ ...ctx.state.settings, sketch: { ...sketch, ...patch } });
  const drawKey = sketch.drawKey === "ctrl" ? "Ctrl" : sketch.drawKey === "alt" ? "Alt" : "Shift";
  return (
    <div className="flex flex-col gap-5">
      <Card>
        <CardHeader
          icon={<PencilSimpleLine size={19} />}
          title="Sketch"
          detail="Talk while you mark up the screen. When you finish, Quill pastes the screenshots, then what you said at each one, into the app you started in. Made for showing an AI agent what you mean."
          right={<Switch label="Sketch" checked={sketch.enabled} onChange={(v) => save({ enabled: v })} />}
        />
        {sketch.enabled && (
          <>
            <Divider />
            <CardHeader
              icon={<Command size={19} />}
              title="Shortcut"
              detail={
                capturingSketchHotkey
                  ? "Hold Ctrl, Alt, Shift or Win, then press another key. Esc cancels."
                  : "Press once to start a Sketch and again to finish. Esc twice cancels."
              }
              right={<ShortcutButton ctx={ctx} sketch />}
            />
            {capturingSketchHotkey && captureError && <Warning>{captureError}</Warning>}
            {sketchHotkeyProblem && <Warning>{sketchHotkeyProblem}</Warning>}
            <Divider />
            <CardHeader
              icon={<Hand size={19} />}
              title="Draw key"
              detail={`Hold ${drawKey} and move the pointer to draw. Clicks still reach your apps.`}
              right={
                <Segmented<DrawKey>
                  width={210}
                  value={sketch.drawKey}
                  onChange={(v) => save({ drawKey: v })}
                  options={[
                    { value: "shift", label: "Shift" },
                    { value: "ctrl", label: "Ctrl" },
                    { value: "alt", label: "Alt" },
                  ]}
                />
              }
            />
          </>
        )}
      </Card>

      {sketch.enabled && (
        <Section title="How it works">
          <Card>
            <ul className="flex list-disc flex-col gap-1.5 pl-4 text-[12.5px] leading-[1.5] text-text-2">
              <li>
                Press <b className="text-text">{sketchHotkeyDisplay}</b> in the app you want to paste into. A red glow
                marks the screen being recorded.
              </li>
              <li>
                Hold <b className="text-text">{drawKey}</b> and move to draw, or turn on Draw in the bar and drag. Ctrl+Z
                undoes the last mark.
              </li>
              <li>Click around and talk as you go. Quill keeps a screenshot for every group of marks, every click, and every screen that changes and settles.</li>
              <li>
                Press <b className="text-text">{sketchHotkeyDisplay}</b> again, or Finish. Quill pastes the screenshots, then a line for each with what you said.
                In a terminal, it pastes their paths instead, which Claude Code turns into images.
              </li>
              <li>Screenshots are saved in your Quill folder for a day, so you can open them again from the pasted text.</li>
            </ul>
          </Card>
          <p className="px-0.5 text-[11.5px] leading-[1.45] text-text-3">
            The idea and the way you draw come from Scrawl by Kit Langton (MIT).
          </p>
        </Section>
      )}
    </div>
  );
}
