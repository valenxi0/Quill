// Render the film: parallel Playwright workers call seek(t) for every frame,
// then ffmpeg encodes the frames and muxes the synthesized score.
// node render.mjs [--fps 60] [--from 0] [--to 36] [--workers 3] [--out quill-launch.mp4]
import { chromium } from "playwright";
import { mkdirSync, existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import path from "node:path";

const dir = path.dirname(fileURLToPath(import.meta.url));
const TL = createRequire(import.meta.url)("./timeline.js");
const arg = (k, d) => { const i = process.argv.indexOf("--" + k); return i > 0 ? process.argv[i + 1] : d; };
const fps = Number(arg("fps", TL.fps));
const from = Number(arg("from", 0));
const to = Number(arg("to", TL.duration));
const workers = Number(arg("workers", 3));
const outName = arg("out", "quill-launch.mp4");
const frames = path.join(dir, "out", `frames${fps}`);
mkdirSync(frames, { recursive: true });

const first = Math.round(from * fps), last = Math.round(to * fps);
const total = last - first;
const t0 = Date.now();
let done = 0;
async function worker(w) {
  const browser = await chromium.launch({ args: ["--allow-file-access-from-files", "--font-render-hinting=none"] });
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
  page.on("pageerror", (e) => { console.error("pageerror:", e.message); process.exit(1); });
  await page.goto("file://" + path.join(dir, "index.html"));
  await page.evaluate(() => window.ready);
  for (let f = first + w; f < last; f += workers) {
    await page.evaluate((t) => window.seek(t), f / fps);
    await page.screenshot({ path: path.join(frames, `f${String(f).padStart(5, "0")}.png`) });
    if (++done % 120 === 0) console.log(`${done}/${total} frames, ${((Date.now() - t0) / 1000).toFixed(0)}s`);
  }
  await browser.close();
}
await Promise.all([...Array(workers)].map((_, w) => worker(w)));
console.log(`frames done in ${((Date.now() - t0) / 1000).toFixed(0)}s`);

const score = path.join(dir, "out", "score.wav");
if (!existsSync(score)) spawnSync("node", [path.join(dir, "audio.mjs")], { stdio: "inherit" });
const out = path.join(dir, "out", outName);
const r = spawnSync("ffmpeg", [
  "-y", "-hide_banner", "-loglevel", "error",
  "-framerate", String(fps), "-start_number", String(first), "-i", path.join(frames, "f%05d.png"),
  "-ss", String(from), "-t", String(to - from), "-i", score,
  "-frames:v", String(total),
  "-c:v", "libx264", "-preset", "slow", "-crf", "16", "-profile:v", "high", "-pix_fmt", "yuv420p",
  "-c:a", "aac", "-b:a", "320k", "-ar", "48000",
  "-movflags", "+faststart", "-shortest", out,
], { stdio: "inherit" });
if (r.status) process.exit(r.status);
console.log("wrote", out);
