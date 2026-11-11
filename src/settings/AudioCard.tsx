import { useEffect, useState } from "react";
import { BluetoothSlash, Microphone, SpeakerSimpleLow } from "@phosphor-icons/react";
import { api, type Microphones, type OtherAudio } from "../lib/api";
import type { Ctx } from "./App";
import { Card, CardHeader, Divider, Switch, Warning } from "./ui";

const OTHER_AUDIO: { value: OtherAudio; label: string; detail: string }[] = [
  { value: "nothing", label: "Do nothing", detail: "Other apps keep playing as they are." },
  { value: "lower", label: "Lower volume", detail: "Other apps play quieter while you speak, then go back to their volume." },
  { value: "mute", label: "Mute", detail: "Other apps go silent while you speak, then come back." },
  { value: "pause", label: "Pause media", detail: "Music and videos pause while you speak, then resume." },
];

const SELECT = "h-[28px] w-[190px] rounded-[6px] border border-card-border bg-fill px-2 text-[12.5px] outline-none";

function missingLine(missing: string, mics: Microphones) {
  if (mics.avoided && mics.using) return `${missing} isn't connected. Using ${mics.using} instead.`;
  return `${missing} isn't connected. Using the default microphone.`;
}

function micDetail(mics: Microphones | null, systemDefault: boolean) {
  if (!mics) return "Looking for microphones…";
  if (!mics.using) return "No microphone is connected.";
  if (mics.avoided) return `Using ${mics.using}. ${mics.avoided} is a Bluetooth headset, which would switch to call quality.`;
  return systemDefault ? `Follows Windows. Now using ${mics.using}.` : `Using ${mics.using}.`;
}

/** The Dictation page's Audio card: microphone and other apps' audio. */
export function AudioCard({ ctx }: { ctx: Ctx }) {
  const vd = ctx.state.settings.voiceDictation;
  const [mics, setMics] = useState<Microphones | null>(null);
  const [percent, setPercent] = useState(vd.lowerToPercent);
  const savedId = vd.inputDevice?.id ?? "";
  useEffect(() => setPercent(vd.lowerToPercent), [vd.lowerToPercent]);
  useEffect(() => {
    let live = true;
    const load = () =>
      api
        .getMicrophones(vd.inputDevice, vd.avoidBluetoothMic)
        .then((m) => live && setMics(m))
        .catch(console.error);
    load();
    // Plugging in a mic happens outside Settings.
    window.addEventListener("focus", load);
    return () => {
      live = false;
      window.removeEventListener("focus", load);
    };
  }, [savedId, vd.avoidBluetoothMic]);
  const mode = OTHER_AUDIO.find((o) => o.value === vd.otherAudio) ?? OTHER_AUDIO[0]!;
  const commitPercent = () => percent !== vd.lowerToPercent && ctx.saveVd({ lowerToPercent: percent });
  const pick = (id: string) => {
    if (!id) return ctx.saveVd({ inputDevice: null });
    const d = mics?.devices.find((m) => m.id === id);
    if (d) ctx.saveVd({ inputDevice: { id: d.id, name: d.name } });
  };
  return (
    <Card>
      <CardHeader
        icon={<Microphone size={19} />}
        title="Microphone"
        detail={micDetail(mics, !vd.inputDevice)}
        right={
          <select aria-label="Microphone" value={savedId} onChange={(e) => pick(e.target.value)} className={SELECT}>
            <option value="">System default</option>
            {mics?.devices.map((d) => (
              <option key={d.id} value={d.id}>
                {d.name}
              </option>
            ))}
            {vd.inputDevice && mics?.missing && <option value={vd.inputDevice.id}>{vd.inputDevice.name} (not connected)</option>}
          </select>
        }
      />
      {mics?.missing && <Warning>{missingLine(mics.missing, mics)}</Warning>}
      <Divider />
      <CardHeader
        icon={<BluetoothSlash size={19} />}
        title="Avoid Bluetooth headset mics"
        detail="Recording from a Bluetooth headset drops it to call quality, so everything you hear sounds worse. Quill uses another mic when there is one. A mic you pick above is always used."
        right={<Switch label="Avoid Bluetooth headset mics" checked={vd.avoidBluetoothMic} onChange={(v) => ctx.saveVd({ avoidBluetoothMic: v })} />}
      />
      <Divider />
      <CardHeader
        icon={<SpeakerSimpleLow size={19} />}
        title="Other audio while dictating"
        detail={mode.detail}
        right={
          <select
            aria-label="Other audio while dictating"
            value={vd.otherAudio}
            onChange={(e) => ctx.saveVd({ otherAudio: e.target.value as OtherAudio })}
            className={SELECT}
          >
            {OTHER_AUDIO.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        }
      />
      {vd.otherAudio === "lower" && (
        <div className="mt-3 ml-8 flex items-center gap-3">
          <span className="w-[190px] text-[12px] text-text-2">Lower to {percent}% of their volume</span>
          <input
            type="range"
            aria-label="Lower other apps to"
            min={5}
            max={80}
            step={5}
            value={percent}
            onChange={(e) => setPercent(Number(e.target.value))}
            onPointerUp={commitPercent}
            onKeyUp={commitPercent}
            onBlur={commitPercent}
            className="flex-1 accent-[var(--accent)]"
          />
        </div>
      )}
    </Card>
  );
}
