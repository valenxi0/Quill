// Synthesizes the score and sound effects on the film's timeline: out/score.wav
// 120 BPM in F major. Every hit reads its time from timeline.js.
import { createRequire } from "node:module";
import { writeFileSync, mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const require = createRequire(import.meta.url);
const TL = require("./timeline.js");
const C = TL.cues;
const dir = path.dirname(fileURLToPath(import.meta.url));

const SR = 48000;
const DUR = TL.duration;
const N = Math.ceil(SR * DUR);
const L = new Float32Array(N), R = new Float32Array(N); // dry
const VL = new Float32Array(N), VR = new Float32Array(N); // reverb send
const rand = TL.rng(1234);
const noise = () => rand() * 2 - 1;
const mtof = (m) => 440 * Math.pow(2, (m - 69) / 12);
const BEAT = TL.BEAT;

function add(t0, len, fn, { gain = 1, pan = 0, send = 0 } = {}) {
  const i0 = Math.max(0, Math.round(t0 * SR));
  const i1 = Math.min(N, Math.round((t0 + len) * SR));
  const gl = gain * Math.cos(((pan + 1) * Math.PI) / 4) * Math.SQRT2;
  const gr = gain * Math.sin(((pan + 1) * Math.PI) / 4) * Math.SQRT2;
  for (let i = i0; i < i1; i++) {
    const v = fn((i - i0) / SR, i / SR);
    L[i] += v * gl; R[i] += v * gr;
    if (send) { VL[i] += v * gl * send; VR[i] += v * gr * send; }
  }
}

// TPT state-variable filter
function svf() {
  let ic1 = 0, ic2 = 0;
  return (x, fc, q = 0.7) => {
    const g = Math.tan((Math.PI * Math.min(fc, SR * 0.45)) / SR);
    const k = 1 / q;
    const a1 = 1 / (1 + g * (g + k)), a2 = g * a1, a3 = g * a2;
    const v3 = x - ic2, v1 = a1 * ic1 + a2 * v3, v2 = ic2 + a2 * ic1 + a3 * v3;
    ic1 = 2 * v1 - ic1; ic2 = 2 * v2 - ic2;
    return { lp: v2, bp: v1, hp: x - k * v1 - v2 };
  };
}

// ---------- instruments ----------
function kick(t, g = 1) {
  add(t, 0.5, (x) => {
    const f = 45 + 95 * Math.exp(-x * 28);
    const ph = 2 * Math.PI * (45 * x + (95 / 28) * (1 - Math.exp(-x * 28)));
    return Math.sin(ph) * Math.exp(-x * 7) + (x < 0.004 ? noise() * 0.4 * (1 - x / 0.004) : 0) + 0 * f;
  }, { gain: 0.8 * g });
}
function clap(t, g = 1) {
  const f = svf();
  add(t, 0.35, (x) => {
    const env = (x < 0.03 ? Math.exp(-((x % 0.01) * 300)) : Math.exp(-(x - 0.03) * 18));
    return f(noise(), 1400, 1.2).bp * env * 1.6;
  }, { gain: 0.32 * g, send: 0.5 });
}
function hat(t, g = 1, open = false) {
  const f = svf();
  add(t, open ? 0.3 : 0.08, (x) => f(noise(), 8000, 0.8).hp * Math.exp(-x * (open ? 12 : 60)), { gain: 0.11 * g, pan: 0.2 });
}
function tone(t, m, len, { gain = 0.2, harm = [1], decay = 4, attack = 0.005, pan = 0, send = 0.2, detune = 0 } = {}) {
  const f0 = mtof(m) * (1 + detune);
  add(t, len, (x) => {
    let v = 0;
    harm.forEach((h, i) => { v += (Math.sin(2 * Math.PI * f0 * h * x) * Math.exp(-x * decay * (1 + i * 0.8))) / (i + 1); });
    return v * Math.min(1, x / attack);
  }, { gain, pan, send });
}
function bell(t, m, g = 0.22, pan = 0) {
  const f0 = mtof(m);
  add(t, 2.5, (x) => {
    const a = Math.min(1, x / 0.002);
    return a * (Math.sin(2 * Math.PI * f0 * x) * Math.exp(-x * 2.2) + 0.45 * Math.sin(2 * Math.PI * f0 * 2.76 * x) * Math.exp(-x * 5) + 0.2 * Math.sin(2 * Math.PI * f0 * 5.4 * x) * Math.exp(-x * 9));
  }, { gain: g, pan, send: 0.55 });
}
function pluck(t, m, g = 0.1, pan = 0) {
  tone(t, m, 0.6, { gain: g, harm: [1, 2, 3, 4, 5, 6], decay: 7, pan, send: 0.3 });
}
// Supersaw pad with slow attack and a lowpass that follows `cut(t)`.
function pad(t, notes, len, { gain = 0.08, cut = () => 1600, attack = 0.4, release = 0.6, send = 0.5 } = {}) {
  notes.forEach((m, ni) => {
    const f = svf();
    const fr = [-0.006, 0, 0.007].map((d) => mtof(m) * (1 + d));
    const ph = fr.map(() => rand());
    add(t, len + release, (x, abs) => {
      let v = 0;
      fr.forEach((fq, j) => { const p = (ph[j] + fq * x) % 1; v += 2 * p - 1; });
      const env = Math.min(1, x / attack) * (x > len ? Math.exp(-(x - len) * (4 / release)) : 1);
      return f(v / 3, cut(abs), 0.6).lp * env;
    }, { gain, pan: (ni / (notes.length - 1 || 1)) * 0.8 - 0.4, send });
  });
}
function bass(t, m, len, g = 0.32) {
  const f = svf();
  add(t, len, (x) => {
    const p = (mtof(m) * x) % 1;
    const v = Math.sin(2 * Math.PI * mtof(m) * x) * 0.8 + (2 * p - 1) * 0.3;
    return f(v, 700, 0.7).lp * Math.min(1, x / 0.005) * Math.exp(-x * 2.5);
  }, { gain: g });
}
function click(t, g = 1) {
  const f = svf();
  const pitch = 1800 + rand() * 900;
  add(t, 0.03, (x) => (f(noise(), 3500, 0.9).bp * 1.4 + Math.sin(2 * Math.PI * pitch * x) * 0.3) * Math.exp(-x * 160), { gain: 0.5 * g * (0.75 + rand() * 0.4), pan: rand() * 0.4 - 0.2 });
}
function thock(t, g = 1) { // big keycap
  add(t, 0.09, (x) => (Math.sin(2 * Math.PI * (220 + 300 * Math.exp(-x * 60)) * x) * 0.8 + noise() * 0.25 * Math.exp(-x * 120)) * Math.exp(-x * 45), { gain: 0.45 * g });
}
function denied(t) {
  [0, 0.085].forEach((dt, i) => {
    const f = svf();
    const fq = i ? 155 : 207;
    add(t + dt, 0.11, (x) => { const sq = Math.sign(Math.sin(2 * Math.PI * fq * x)); return f(sq, 1400, 0.8).lp * Math.exp(-x * 18); }, { gain: 0.16 });
  });
}
function whoosh(t, len = 0.45, g = 1, up = true) {
  const f = svf();
  add(t, len, (x) => {
    const u = x / len;
    const fc = up ? 300 * Math.pow(18, u) : 5000 * Math.pow(1 / 18, u);
    return f(noise(), fc, 1.4).bp * Math.sin(Math.PI * u) ** 2;
  }, { gain: 0.3 * g, send: 0.3, pan: up ? -0.2 : 0.2 });
}
function riser(t, len, g = 1) {
  const f = svf();
  add(t, len, (x) => {
    const u = x / len;
    return (f(noise(), 200 * Math.pow(40, u), 2.5).bp * 0.9 + Math.sin(2 * Math.PI * (110 + 330 * u * u) * x) * 0.15) * u * u;
  }, { gain: 0.35 * g, send: 0.4 });
}
function impact(t, g = 1) {
  add(t, 1.8, (x) => Math.sin(2 * Math.PI * (38 + 40 * Math.exp(-x * 9)) * x) * Math.exp(-x * 2.6), { gain: 0.8 * g });
  const f = svf();
  add(t, 1.2, (x) => f(noise(), 2500, 0.5).lp * Math.exp(-x * 5), { gain: 0.18 * g, send: 0.8 });
}
function reverse(t, len, g = 1) { // reversed cymbal swell ending at t + len
  const f = svf();
  add(t, len, (x) => f(noise(), 6000, 0.6).hp * Math.pow(x / len, 3), { gain: 0.22 * g, send: 0.3 });
}

// ---------- arrangement ----------
// Typing and search: real keystroke times from timeline.js
TL.typed.keys.forEach((k) => click(k, 1));
TL.search.keys.forEach((k) => click(k, 0.8));
click(C.searchEnter, 1.4);
thock(C.searchEnter, 0.6);

// Intro bed: dark Dm(add9) pad, heartbeat kick from the search on
pad(0, [50, 57, 62, 64], 8.0, { gain: 0.05, cut: (t) => 500 + t * 120, attack: 1.5, release: 0.2 });
for (let t = 3.0; t < 8.0; t += 1.0) kick(t, 0.45);
for (let t = 4.5; t < 8.0; t += 0.25) hat(t, (t * 4) % 2 < 1 ? 0.9 : 0.5);
C.cardsIn && [0, 1, 2, 3].forEach((i) => pluck(C.cardsIn + i * 0.07, [69, 72, 76, 79][i], 0.07, -0.3 + i * 0.2));
C.denied.forEach((t) => denied(t));
C.clicks.forEach((t) => click(t, 1.2));
riser(7.0, 1.0, 1);
bell(C.macOnly, 50, 0.12);

// "Until now." — hard stop, low impact, reverse swell into the drop
impact(C.untilNow, 0.7);
reverse(8.35, 0.65, 1.2);

// Drop and groove (F major: Fmaj7 – Am7 – Dm9 – Bbmaj7/F)
const CHORDS = [[53, 57, 60, 64], [57, 60, 64, 67], [50, 57, 60, 64, 65], [53, 57, 58, 62]];
const ROOTS = [41, 45, 38, 46];
const END = C.endCard;
impact(C.drop, 1);
bell(C.drop, 77, 0.16, -0.3); bell(C.drop + 0.08, 81, 0.14, -0.1); bell(C.drop + 0.16, 84, 0.13, 0.1); bell(C.drop + 0.24, 88, 0.12, 0.3);
bell(C.tileIn, 72, 0.18); whoosh(C.tileIn - 0.3, 0.35, 0.6);
whoosh(C.wordmark - 0.1, 0.5, 0.7, false);

const quiet = (t) => t >= C.recStart && t < C.transcribing; // thin the groove while you speak
for (let bar = 0; C.drop + bar * 2 < END; bar++) {
  const t0 = C.drop + bar * 2;
  const ch = bar % 4;
  const cutoff = (t) => (quiet(t) ? 700 : t < 12.5 ? 1200 + (t - 9) * 300 : 2200);
  pad(t0, CHORDS[ch], 2, { gain: 0.055, cut: cutoff, attack: 0.08, release: 0.3 });
  for (let b = 0; b < 4; b++) {
    const t = t0 + b * BEAT;
    if (t >= END) break;
    kick(t, quiet(t) ? 0.55 : 1);
    if (b % 2 === 1 && !quiet(t) && t > 9.9) clap(t);
    for (let s = 0; s < 4; s++) {
      const th = t + (s * BEAT) / 4;
      if (t > 9.9) hat(th, (quiet(th) ? 0.35 : 1) * (s === 2 ? 1 : 0.45), s === 2 && b === 3);
    }
    // offbeat bass
    if (t > 9.9) bass(t + BEAT / 2, ROOTS[ch], BEAT * 0.45, quiet(t) ? 0.18 : 0.32);
  }
  // arp in the feature run
  if (t0 >= 19 && t0 < END) {
    const tones = CHORDS[ch].map((m) => m + 12);
    for (let s = 0; s < 16; s++) {
      const t = t0 + (s * BEAT) / 4;
      if (t >= END) break;
      const m = tones[[0, 1, 2, 3, 2, 1, 3, 2][s % 8] % tones.length];
      pluck(t, m + (s % 8 === 6 ? 12 : 0), 0.055, s % 2 ? 0.35 : -0.35);
    }
  }
}

// Demo sound design
whoosh(C.toDemo - 0.2, 0.45, 0.8);
tone(C.recStart, 76, 0.25, { gain: 0.1, harm: [1, 2], decay: 14 });
tone(C.recStart + 0.08, 83, 0.3, { gain: 0.1, harm: [1, 2], decay: 12 });
tone(C.transcribing, 72, 0.25, { gain: 0.08, harm: [1, 2], decay: 14 });
whoosh(C.cleaning - 0.05, 0.45, 0.45);
bell(C.pasted + 0.05, 84, 0.2); bell(C.pasted + 0.14, 91, 0.16);
whoosh(C.sent - 0.05, 0.25, 0.6);
tone(C.sent + 0.05, 79, 0.2, { gain: 0.1, harm: [1, 3], decay: 18 });

// Airplane-mode toggle and offline dictation in F1
click(C.features[0] + 0.4, 1.5); thock(C.features[0] + 0.4, 0.5);
tone(C.features[0] + 0.7, 76, 0.25, { gain: 0.08, harm: [1, 2], decay: 14 });
bell(C.features[0] + 1.55, 84, 0.14); bell(C.features[0] + 1.63, 91, 0.1);

// Feature transitions, tab switches, the Add click, installer
C.features.forEach((t, i) => i && whoosh(t - 0.18, 0.4, 0.7, i % 2 === 0));
whoosh(C.features[0] - 0.2, 0.45, 0.9);
C.styleTabs.slice(1).forEach((t) => click(t, 1.3));
click(C.learnClick, 1.4);
bell(C.learnClick + 0.15, 86, 0.16); bell(C.learnClick + 0.24, 93, 0.12);
tone(C.features[1] + 0.74, 88, 0.3, { gain: 0.12, harm: [1, 2], decay: 10 }); // F2 paste
riser(C.features[5] + 0.15, 0.7, 0.5);
bell(C.features[5] + 0.9, 89, 0.14);

// End card: last stab, then the pad rings out with a bell figure
impact(END, 0.9);
reverse(END - 0.5, 0.5, 1);
kick(END, 1);
pad(END, [53, 60, 64, 67, 69], DUR - END - 1.2, { gain: 0.07, cut: (t) => 2400 - (t - END) * 250, attack: 0.02, release: 1.6, send: 0.7 });
bass(END, 29, 3, 0.4);
[[END, 84], [END + 0.5, 81], [END + 1.0, 79], [END + 1.5, 77], [END + 2.5, 72]].forEach(([t, m], i) => bell(t, m, 0.15 - i * 0.015, i % 2 ? 0.25 : -0.25));

// ---------- sidechain, reverb, master ----------
const kicks = [];
for (let t = 3.0; t < 8.0; t += 1.0) kicks.push(t);
for (let t = C.drop; t < END; t += BEAT) kicks.push(t);
// Reverb: 4 combs + 2 allpasses per side (Schroeder)
function reverb(inp, seed) {
  const out = new Float32Array(N);
  const combs = [1557, 1617, 1491, 1422].map((d) => ({ d: d + seed, buf: new Float32Array(d + seed), i: 0, fb: 0.84, lp: 0 }));
  const aps = [556, 441].map((d) => ({ d: d + seed, buf: new Float32Array(d + seed), i: 0 }));
  for (let n = 0; n < N; n++) {
    let s = 0;
    for (const c of combs) {
      const y = c.buf[c.i];
      c.lp = y * 0.6 + c.lp * 0.4;
      c.buf[c.i] = inp[n] + c.lp * c.fb;
      c.i = (c.i + 1) % c.d;
      s += y;
    }
    for (const a of aps) {
      const b = a.buf[a.i];
      const y = -s + b;
      a.buf[a.i] = s + b * 0.5;
      a.i = (a.i + 1) % a.d;
      s = y;
    }
    out[n] = s * 0.25;
  }
  return out;
}
const rl = reverb(VL, 0), rr = reverb(VR, 23);
let peak = 0;
const outL = new Float32Array(N), outR = new Float32Array(N);
let ki = 0;
for (let n = 0; n < N; n++) {
  const t = n / SR;
  while (ki + 1 < kicks.length && kicks[ki + 1] <= t) ki++;
  const since = kicks[ki] <= t ? t - kicks[ki] : 9;
  const duck = 1 - 0.35 * Math.exp(-since / 0.09);
  const fadeIn = Math.min(1, t / 0.05);
  const fadeOut = t > DUR - 1.2 ? Math.max(0, (DUR - t) / 1.2) : 1;
  const l = (L[n] + rl[n] * duck) * fadeIn * fadeOut;
  const r = (R[n] + rr[n] * duck) * fadeIn * fadeOut;
  outL[n] = Math.tanh(l * 1.15);
  outR[n] = Math.tanh(r * 1.15);
  peak = Math.max(peak, Math.abs(outL[n]), Math.abs(outR[n]));
}
const norm = 0.89 / peak;
const buf = Buffer.alloc(44 + N * 4);
buf.write("RIFF", 0); buf.writeUInt32LE(36 + N * 4, 4); buf.write("WAVE", 8);
buf.write("fmt ", 12); buf.writeUInt32LE(16, 16); buf.writeUInt16LE(1, 20); buf.writeUInt16LE(2, 22);
buf.writeUInt32LE(SR, 24); buf.writeUInt32LE(SR * 4, 28); buf.writeUInt16LE(4, 32); buf.writeUInt16LE(16, 34);
buf.write("data", 36); buf.writeUInt32LE(N * 4, 40);
for (let n = 0; n < N; n++) {
  buf.writeInt16LE(Math.round(outL[n] * norm * 32767), 44 + n * 4);
  buf.writeInt16LE(Math.round(outR[n] * norm * 32767), 46 + n * 4);
}
mkdirSync(path.join(dir, "out"), { recursive: true });
writeFileSync(path.join(dir, "out", "score.wav"), buf);
console.log(`score.wav: ${DUR}s, peak ${peak.toFixed(2)} → normalized`);
