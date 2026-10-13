// Render key frames for the critique loop: node stills.mjs 1.2 5.6 ... (or no args for the default set)
import { chromium } from "playwright";
import { mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const dir = path.dirname(fileURLToPath(import.meta.url));
const out = path.join(dir, "out", process.env.STILLS ?? "stills");
mkdirSync(out, { recursive: true });
const times = process.argv.slice(2).map(Number);
const list = times.length ? times : [0.6, 1.8, 2.6, 3.8, 4.9, 5.7, 6.6, 7.4, 8.4, 9.3, 9.8, 10.6, 11.6, 12.8, 13.6, 14.8, 16.1, 16.7, 17.1, 17.5, 18.3, 19.0, 20.2, 21.6, 23.0, 24.6, 25.4, 26.3, 26.9, 28.3, 29.6, 31.2, 32.5, 34.0];
const browser = await chromium.launch({ args: ["--allow-file-access-from-files", "--font-render-hinting=none"] });
const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
page.on("pageerror", (e) => console.error("pageerror:", e.message));
page.on("console", (m) => m.type() === "error" && console.error("console:", m.text()));
await page.goto("file://" + path.join(dir, "index.html"));
await page.evaluate(() => window.ready);
for (const t of list) {
  await page.evaluate((t) => window.seek(t), t);
  await page.screenshot({ path: path.join(out, `t${t.toFixed(2).padStart(5, "0")}.png`) });
}
await browser.close();
console.log(`wrote ${list.length} stills to ${out}`);
