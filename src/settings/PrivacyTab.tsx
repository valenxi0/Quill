import { useState } from "react";
import { ChartBar, Cpu } from "@phosphor-icons/react";
import { api } from "../lib/api";
import type { Ctx } from "./App";
import { LicensesCard } from "./SetupTab";
import { Button, Card, CardHeader, Divider, Section, Sheet, Switch } from "./ui";

export function PrivacyTab({ ctx }: { ctx: Ctx }) {
  const vd = ctx.state.settings.voiceDictation;
  const [confirmReset, setConfirmReset] = useState(false);
  return (
    <>
      <Card>
        <CardHeader
          icon={<Cpu size={19} />}
          title="Everything runs on this PC"
          detail="Recognition and cleanup run locally. Quill keeps audio in memory and discards it once the transcript is ready. It sends no audio or text anywhere; the internet is only used to download models and check for updates."
        />
      </Card>
      <Section title="Stored on this PC">
        <Card>
          <CardHeader
            icon={<ChartBar size={19} />}
            title="Keep insights"
            detail="Counts only: words, seconds, and the app you dictated into. Insights never keep your words."
            right={<Switch label="Keep insights" checked={vd.insightsEnabled} onChange={(v) => ctx.saveVd({ insightsEnabled: v })} />}
          />
          <Divider />
          <div className="ml-8 flex items-center justify-between gap-3">
            <p className="text-[12px] text-text-2">Clears totals, streaks, and app counts. Your dictionary and settings stay.</p>
            <Button small kind="destructive" onClick={() => setConfirmReset(true)}>
              Reset insights
            </Button>
          </div>
        </Card>
        <p className="px-0.5 text-[12px] text-text-3">Transcript history and learned corrections are on the Your voice page.</p>
      </Section>
      <LicensesCard />
      {confirmReset && (
        <Sheet
          title="Reset insights?"
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
    </>
  );
}
