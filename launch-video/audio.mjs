// Synthesizes the score and sound effects on the film's timeline: out/score.wav
// 120 BPM in A major, IV–V–iii–vi (Dmaj9 – E6/9 – C#m7 – F#m9), one chord per bar.
// Typing uses real keystroke recordings from sfx/keys (CC0). To use your own typing
// recording instead, save it as sfx/typing.wav; its keystrokes are detected and sliced.
import { createRequire } from "node:module";
import { writeFileSync, mkdirSync, readFileSync, readdirSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const require = createRequire(import.meta.url);
const TL = require("./timeline.js");
const C = TL.cues;
const dir = path.dirname(fileURLToPath(import.meta.url));

const SR = 48000;
const DUR = TL.duration;
const N = Math.ceil(SR * DUR);
const BEAT = TL.BEAT;
const BAR = BEAT * 4;
const rand = TL.rng(1234);
const noise = () => rand() * 2 - 1;
const mtof = (m) => 440 * Math.pow(2, (m - 69) / 12);

// Buses: music is filtered and sidechained; kick and sfx sit on top untouched.
const bus = () => ({ L: new Float32Array(N), R: new Float32Array(N) });
const MUS = bus(), KICK = bus(), SFX = bus(), VERB = bus(), DLY = bus();
function add(target, t0, len, fn, { gain = 1, pan = 0, verb = 0, delay = 0 } = {}) {
  const i0 = Math.max(0, Math.round(t0 * SR));
  const i1 = Math.min(N, Math.round((t0 + len) * SR));
  const gl = gain * Math.cos(((pan + 1) * Math.PI) / 4) * Math.SQRT2;
  const gr = gain * Math.sin(((pan + 1) * Math.PI) / 4) * Math.SQRT2;
  for (let i = i0; i < i1; i++) {
    const v = fn((i - i0) / SR, i / SR);
    target.L[i] += v * gl; target.R[i] += v * gr;
    if (verb) { VERB.L[i] += v * gl * verb; VERB.R[i] += v * gr * verb; }
    if (delay) { DLY.L[i] += v * gl * delay; DLY.R[i] += v * gr * delay; }
  }
}
const mus = (...a) => add(MUS, ...a);
const sfx = (...a) => add(SFX, ...a);

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
const saw = (p) => 2 * (p - Math.floor(p)) - 1;

// ---------- samples ----------
function readWav(file) {
  const b = readFileSync(file);
  let o = 12, fmt, data;
  while (o + 8 <= b.length) {
    const id = b.toString("ascii", o, o + 4), sz = b.readUInt32LE(o + 4);
    if (id === "fmt ") fmt = { ch: b.readUInt16LE(o + 10), sr: b.readUInt32LE(o + 12), bits: b.readUInt16LE(o + 22) };
    if (id === "data") data = b.subarray(o + 8, o + 8 + sz);
    o += 8 + sz + (sz & 1);
  }
  if (!fmt || !data || fmt.bits !== 16) throw new Error(`${file}: expected 16-bit PCM WAV`);
  const n = Math.floor(data.length / 2 / fmt.ch);
  const out = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    let s = 0;
    for (let c = 0; c < fmt.ch; c++) s += data.readInt16LE((i * fmt.ch + c) * 2) / 32768;
    out[i] = s / fmt.ch;
  }
  return { sr: fmt.sr, data: out };
}
function resample({ sr, data }) {
  if (sr === SR) return data;
  const n = Math.floor((data.length * SR) / sr), out = new Float32Array(n);
  for (let i = 0; i < n; i++) {
    const x = (i * sr) / SR, j = Math.floor(x), f = x - j;
    out[i] = (data[j] ?? 0) * (1 - f) + (data[j + 1] ?? 0) * f;
  }
  return out;
}
// Slice a typing recording into single keystrokes by onset detection.
function sliceTyping(sig) {
  const hop = 240, env = [];
  for (let i = 0; i + hop < sig.length; i += hop) {
    let e = 0;
    for (let j = 0; j < hop; j++) e = Math.max(e, Math.abs(sig[i + j]));
    env.push(e);
  }
  const peak = env.reduce((a, b) => Math.max(a, b), 0), hits = [];
  let last = -1e9;
  env.forEach((e, k) => {
    if (e > peak * 0.18 && e > (env[k - 1] ?? 0) * 1.8 && k * hop - last > SR * 0.07) {
      const s = Math.max(0, k * hop - 96);
      hits.push(sig.subarray(s, Math.min(sig.length, s + Math.round(SR * 0.14))));
      last = k * hop;
    }
  });
  return hits;
}
const keyDir = path.join(dir, "sfx", "keys");
const custom = path.join(dir, "sfx", "typing.wav");
const KEYS = existsSync(custom)
  ? sliceTyping(resample(readWav(custom)))
  : readdirSync(keyDir).filter((f) => f.endsWith(".wav")).sort().map((f) => resample(readWav(path.join(keyDir, f))));
const keyPeak = KEYS.map((k) => k.reduce((a, v) => Math.max(a, Math.abs(v)), 0));
console.log(`${KEYS.length} keystroke samples${existsSync(custom) ? " sliced from sfx/typing.wav" : " from sfx/keys"}`);
let keyIdx = 0;
function key(t, g = 1, heavy = false) {
  const i = heavy ? 0 : (keyIdx = (keyIdx + 1 + Math.floor(rand() * 3)) % KEYS.length);
  const s = KEYS[i];
  const rate = heavy ? 0.88 : 0.96 + rand() * 0.08;
  const norm = 0.5 / (keyPeak[i] || 1);
  sfx(t, s.length / SR / rate, (x) => {
    const p = x * SR * rate, j = Math.floor(p), f = p - j;
    return ((s[j] ?? 0) * (1 - f) + (s[j + 1] ?? 0) * f) * norm;
  }, { gain: g * (0.8 + rand() * 0.3), pan: rand() * 0.3 - 0.15, verb: 0.04 });
}

// ---------- drums ----------
function kick(t, g = 1, target = KICK) {
  add(target, t, 0.55, (x) => {
    const ph = 2 * Math.PI * (48 * x + (110 / 30) * (1 - Math.exp(-x * 30)));
    return Math.tanh(1.6 * Math.sin(ph) * Math.exp(-x * 6.5)) + 0.35 * Math.sin(2 * Math.PI * 175 * x) * Math.exp(-x * 40) + (x < 0.003 ? noise() * 0.6 * (1 - x / 0.003) : 0);
  }, { gain: 0.7 * g });
}
function clap(t, g = 1) {
  const f = svf(), f2 = svf();
  mus(t, 0.45, (x) => {
    const env = x < 0.024 ? Math.exp(-((x % 0.008) * 400)) : Math.exp(-(x - 0.024) * 14);
    const body = Math.sin(2 * Math.PI * 190 * x) * Math.exp(-x * 30) * 0.5;
    return f(noise(), 1500, 1.1).bp * env * 1.5 + f2(noise(), 5000, 0.7).hp * Math.exp(-x * 22) * 0.35 + body;
  }, { gain: 0.28 * g, verb: 0.35 });
}
function hat(t, g = 1, open = false) {
  const f = svf();
  mus(t, open ? 0.35 : 0.06, (x) => f(noise(), 9000, 0.7).hp * Math.exp(-x * (open ? 9 : 70)), { gain: 0.09 * g, pan: 0.25 });
}
function shaker(t, g = 1) {
  const f = svf();
  mus(t, 0.07, (x) => f(noise(), 6500, 1.2).bp * Math.sin(Math.PI * Math.min(1, x / 0.07)), { gain: 0.07 * g, pan: -0.3 });
}
function crash(t, g = 1) {
  const f = svf();
  mus(t, 2.4, (x) => f(noise(), 7000, 0.5).hp * Math.exp(-x * 1.8), { gain: 0.1 * g, verb: 0.4 });
}

// ---------- tonal ----------
// FM electric piano: a decaying modulation index gives the tine its bark.
function ep(t, m, len, g = 0.1, pan = 0) {
  const f = mtof(m);
  mus(t, len + 1.2, (x) => {
    const idx = 1.8 * Math.exp(-x * 5) + 0.35;
    const env = Math.exp(-x * 0.9) * (x > len ? Math.exp(-(x - len) * 6) : 1) * Math.min(1, x / 0.003);
    const tine = Math.sin(2 * Math.PI * f * 7.1 * x) * Math.exp(-x * 25) * 0.15;
    return (Math.sin(2 * Math.PI * f * x + idx * Math.sin(2 * Math.PI * f * x)) + tine) * env;
  }, { gain: g, pan, verb: 0.3 });
}
function pad(t, notes, len, g = 0.05) {
  notes.forEach((m, i) => {
    const f = svf();
    const fr = [-0.005, 0.0, 0.006].map((d) => mtof(m) * (1 + d));
    const ph = fr.map(() => rand());
    mus(t, len + 0.8, (x) => {
      let v = 0;
      fr.forEach((q, j) => (v += saw(ph[j] + q * x)));
      const env = Math.min(1, x / 0.6) * (x > len ? Math.exp(-(x - len) * 5) : 1);
      return f(v / 3, 900 + 500 * Math.sin(x * 0.8), 0.6).lp * env;
    }, { gain: g, pan: (i / Math.max(1, notes.length - 1)) * 0.9 - 0.45, verb: 0.5 });
  });
}
function bass(t, m, len, g = 0.42, rel = 60) {
  const f = svf();
  const fq = mtof(m);
  mus(t, len + 5 / rel, (x) => {
    const v = Math.sin(2 * Math.PI * fq * x) + 0.45 * Math.sin(4 * Math.PI * fq * x) + 0.2 * Math.sin(6 * Math.PI * fq * x) + 0.12 * saw(fq * x);
    const env = Math.min(1, x / 0.006) * (x > len ? Math.exp(-(x - len) * rel) : 1) * (0.75 + 0.25 * Math.exp(-x * 6));
    return f(Math.tanh(v * 1.3), 900, 0.8).lp * env;
  }, { gain: g });
}
// Lead: two detuned saws plus an octave sine, lowpass envelope, gentle vibrato, dotted-8th delay.
function lead(t, m, len, g = 0.075) {
  const f = svf();
  const fq = mtof(m);
  let p1 = 0, p2 = 0;
  mus(t, len + 0.25, (x) => {
    const vib = 1 + 0.004 * Math.sin(2 * Math.PI * 5.5 * x) * Math.min(1, x / 0.3);
    p1 += (fq * vib * 1.004) / SR; p2 += (fq * vib * 0.996) / SR;
    const env = Math.min(1, x / 0.012) * (x > len ? Math.exp(-(x - len) * 14) : 0.85 + 0.15 * Math.exp(-x * 8));
    const cut = 900 + 3200 * Math.exp(-x * 5);
    return (f((saw(p1) + saw(p2)) * 0.5, cut, 0.9).lp + 0.35 * Math.sin(2 * Math.PI * fq * 2 * x)) * env;
  }, { gain: g, verb: 0.25, delay: 0.3 });
}
function pluck(t, m, g = 0.05, pan = 0) {
  const fq = mtof(m);
  mus(t, 0.45, (x) => Math.sin(2 * Math.PI * fq * x + 2.5 * Math.exp(-x * 18) * Math.sin(2 * Math.PI * fq * 2 * x)) * Math.exp(-x * 9), { gain: g, pan, verb: 0.2, delay: 0.15 });
}
function bell(t, m, g = 0.15, pan = 0, target = mus) {
  const f0 = mtof(m);
  target(t, 2.2, (x) => Math.min(1, x / 0.002) * (Math.sin(2 * Math.PI * f0 * x) * Math.exp(-x * 2.4) + 0.4 * Math.sin(2 * Math.PI * f0 * 2.76 * x) * Math.exp(-x * 5.5) + 0.15 * Math.sin(2 * Math.PI * f0 * 5.4 * x) * Math.exp(-x * 10)), { gain: g, pan, verb: 0.5 });
}

// ---------- sfx ----------
function blip(t, m, g = 0.08) {
  sfx(t, 0.25, (x) => (Math.sin(2 * Math.PI * mtof(m) * x) + 0.3 * Math.sin(4 * Math.PI * mtof(m) * x)) * Math.exp(-x * 16) * Math.min(1, x / 0.003), { gain: g, verb: 0.2 });
}
function denied(t) {
  [0, 0.085].forEach((dt, i) => {
    const f = svf(), fq = i ? 155 : 207;
    sfx(t + dt, 0.11, (x) => f(Math.sign(Math.sin(2 * Math.PI * fq * x)), 1300, 0.8).lp * Math.exp(-x * 18), { gain: 0.13 });
  });
}
function whoosh(t, len = 0.45, g = 1, up = true) {
  const f = svf();
  sfx(t, len, (x) => {
    const u = x / len, fc = up ? 300 * Math.pow(16, u) : 5000 / Math.pow(16, u);
    return f(noise(), fc, 1.3).bp * Math.sin(Math.PI * u) ** 2;
  }, { gain: 0.22 * g, verb: 0.3, pan: up ? -0.2 : 0.2 });
}
function riser(t, len, g = 1) {
  const f = svf();
  sfx(t, len, (x) => {
    const u = x / len;
    return (f(noise(), 250 * Math.pow(36, u), 2.2).bp + Math.sin(2 * Math.PI * (220 + 440 * u * u) * x) * 0.08) * u * u;
  }, { gain: 0.3 * g, verb: 0.4 });
}
function impact(t, g = 1) {
  sfx(t, 1.8, (x) => Math.sin(2 * Math.PI * (36 + 44 * Math.exp(-x * 9)) * x) * Math.exp(-x * 2.4), { gain: 0.7 * g });
  const f = svf();
  sfx(t, 1.2, (x) => f(noise(), 2200, 0.5).lp * Math.exp(-x * 5), { gain: 0.15 * g, verb: 0.8 });
}
function reverse(t, len, g = 1) {
  const f = svf();
  sfx(t, len, (x) => f(noise(), 5500, 0.6).hp * Math.pow(x / len, 3), { gain: 0.2 * g, verb: 0.3 });
}

// ---------- arrangement ----------
const CH = [
  { root: 38, notes: [50, 54, 57, 61, 64] }, // Dmaj9
  { root: 40, notes: [52, 56, 59, 61, 66] }, // E6/9
  { root: 37, notes: [49, 52, 56, 59, 64] }, // C#m7
  { root: 42, notes: [54, 57, 61, 64, 68] }, // F#m9
];
// Hook, one phrase per chord: [beat, midi, length in beats]
const HOOK = [
  [[0, 76, 0.5], [0.5, 78, 0.5], [1, 81, 1], [2, 78, 0.5], [2.5, 76, 0.5], [3, 73, 1]],
  [[0, 71, 0.5], [0.5, 73, 0.5], [1, 76, 1], [2, 80, 1], [3, 78, 0.5], [3.5, 76, 0.5]],
  [[0, 76, 0.5], [0.5, 80, 0.5], [1, 83, 1], [2, 81, 0.5], [2.5, 80, 0.5], [3, 76, 1]],
  [[0, 73, 0.5], [0.5, 76, 0.5], [1, 78, 1.5], [2.5, 76, 0.5], [3, 73, 0.5], [3.5, 71, 0.5]],
];
const DROP = C.drop, END = C.endCard;
const LISTEN = [C.recStart, C.transcribing]; // the music closes down while Quill listens
const listening = (t) => t >= LISTEN[0] && t < LISTEN[1];

const INTRO = TL.start; // the film starts with the typing

// Intro (from the film start to 8 s): electric piano under the typing, shaker and soft kick with the search.
for (let bar = 0; bar < 4; bar++) {
  const t0 = INTRO + bar * BAR;
  const c = CH[bar % 4];
  c.notes.slice(1).forEach((m, i) => ep(t0 + i * 0.03, m, BAR * 0.9, 0.055, i * 0.2 - 0.3));
  if (t0 >= 3.5) bass(t0, c.root, BAR * 0.95, 0.22);
  for (let s = 0; s < 8; s++) if (t0 + (s * BEAT) / 2 >= C.searchIn) shaker(t0 + (s * BEAT) / 2, s % 2 ? 0.6 : 1);
  if (t0 >= 5.5) for (let b = 0; b < 4; b++) kick(t0 + b * BEAT, b === 0 ? 0.5 : 0.35);
}
riser(7.0, 1.0);

// Drop and groove
for (let bar = 0; DROP + bar * BAR < END; bar++) {
  const t0 = DROP + bar * BAR;
  const c = CH[bar % 4];
  pad(t0, c.notes, BAR, 0.035);
  c.notes.slice(1).forEach((m, i) => {
    ep(t0 + i * 0.02, m, BEAT * 1.4, 0.05, i * 0.2 - 0.3);
    ep(t0 + BEAT * 2.5 + i * 0.02, m, BEAT * 1.2, 0.035, i * 0.2 - 0.3);
  });
  bass(t0, c.root, BEAT * 1.4);
  bass(t0 + BEAT * 1.5, c.root + 12, BEAT * 0.4, 0.3);
  bass(t0 + BEAT * 2, c.root, BEAT * 1.4);
  bass(t0 + BEAT * 3.5, c.root + 7, BEAT * 0.45, 0.32);
  if (bar % 4 === 0) crash(t0, bar === 0 ? 1.3 : 0.8);
  for (let b = 0; b < 4; b++) {
    const t = t0 + b * BEAT;
    if (t >= END) break;
    const q = listening(t);
    kick(t, q ? 0.6 : 1);
    if (b % 2 === 1 && !q) clap(t);
    hat(t + BEAT / 2, q ? 0.4 : 1, b === 3 && !q);
    for (let s = 0; s < 4; s++) shaker(t + (s * BEAT) / 4, (q ? 0.5 : 1) * (s % 2 ? 0.55 : 0.9));
  }
  const hookOn = t0 < 12.5 || (t0 >= 19 && t0 < END);
  if (hookOn) HOOK[bar % 4].forEach(([b, m, d]) => lead(t0 + b * BEAT, m, d * BEAT * 0.92));
  if (t0 >= 19) {
    const tones = c.notes.slice(1).map((m) => m + 12);
    for (let s = 0; s < 16; s++) pluck(t0 + (s * BEAT) / 4, tones[[0, 2, 1, 3, 2, 1, 3, 2][s % 8] % tones.length], 0.035, s % 2 ? 0.4 : -0.4);
  }
  // demo bars: a quiet counter-line so the dictation has room
  if (!hookOn && !listening(t0 + 0.1)) [0, 1.5, 3].forEach((b, i) => bell(t0 + b * BEAT, c.notes[4] + 12 - i * 3, 0.05, i % 2 ? 0.3 : -0.3));
}

// "Until now.": the music stops dead for a bar, then drops on the downbeat.
impact(C.untilNow, 0.7);
reverse(8.4, 0.6, 1.2);
impact(DROP, 0.9);
bell(C.tileIn, 81, 0.12, 0, sfx); whoosh(C.tileIn - 0.3, 0.35, 0.6);
whoosh(C.wordmark - 0.1, 0.5, 0.6, false);

// Ending: one held chord with the hook's opening phrase, ringing out.
crash(END, 1.2); kick(END, 1);
pad(END, [50, 57, 61, 64, 69], DUR - END - 1.0, 0.045);
bass(END, 38, 2.0, 0.4, 1.6);
[54, 57, 61, 64].forEach((m, i) => ep(END + i * 0.04, m, 3.5, 0.06, i * 0.2 - 0.3));
[[0, 76, 0.5], [0.5, 78, 0.5], [1, 81, 3]].forEach(([b, m, d]) => lead(END + 0.02 + b * BEAT, m, d * BEAT));
bell(END + 1.5, 88, 0.08); bell(END + 2.5, 85, 0.07);

// ---------- sound effects ----------
TL.typed.keys.forEach((k) => key(k, 1));
TL.search.keys.forEach((k) => key(k, 0.75));
key(C.searchEnter, 1.3, true);
C.denied.forEach((t) => denied(t));
C.clicks.forEach((t) => key(t, 0.8));
whoosh(C.toDemo - 0.2, 0.45, 0.7);
blip(C.recStart, 76); blip(C.recStart + 0.08, 83);
blip(C.transcribing, 72, 0.06);
whoosh(C.cleaning - 0.05, 0.45, 0.4);
bell(C.pasted + 0.05, 85, 0.13, 0, sfx); bell(C.pasted + 0.13, 92, 0.1, 0, sfx);
whoosh(C.sent - 0.05, 0.25, 0.5);
blip(C.sent + 0.05, 81, 0.06);
C.features.forEach((t, i) => whoosh(t - 0.18, 0.4, i ? 0.55 : 0.75, i % 2 === 0));
C.styleTabs.slice(1).forEach((t) => key(t, 0.9));
key(C.learnClick, 1);
bell(C.learnClick + 0.15, 86, 0.1, 0, sfx); bell(C.learnClick + 0.24, 93, 0.08, 0, sfx);
riser(C.features[2] + 0.15, 0.7, 0.4);
bell(C.features[2] + 0.9, 88, 0.1, 0, sfx);

// The spoken demo line (AI voice, Kokoro af_heart/af_nicole blend, pauses tightened),
// placed at the first word; timeline.js word times were measured from this file.
const voiceFile = path.join(dir, "sfx", "voice.wav");
if (existsSync(voiceFile)) {
  const v = resample(readWav(voiceFile));
  const pk = v.reduce((a, x) => Math.max(a, Math.abs(x)), 0) || 1;
  sfx(TL.speechStart, v.length / SR, (x) => v[Math.floor(x * SR)] * (0.7 / pk), { verb: 0.05 });
  console.log("mixed sfx/voice.wav");
}

// ---------- mix ----------
const kicks = [];
for (let t = 5.5; t < 8; t += BEAT) kicks.push(t);
for (let t = DROP; t <= END + 1e-6; t += BEAT) kicks.push(t);
function reverb(inp, seed) {
  const out = new Float32Array(N);
  const combs = [1557, 1617, 1491, 1422, 1277, 1356].map((d) => ({ d: d + seed, buf: new Float32Array(d + seed), i: 0, lp: 0 }));
  const aps = [556, 441, 341].map((d) => ({ d: d + seed, buf: new Float32Array(d + seed), i: 0 }));
  for (let n = 0; n < N; n++) {
    let s = 0;
    for (const c of combs) {
      const y = c.buf[c.i];
      c.lp = y * 0.55 + c.lp * 0.45;
      c.buf[c.i] = inp[n] + c.lp * 0.86;
      c.i = (c.i + 1) % c.d;
      s += y;
    }
    for (const a of aps) {
      const b = a.buf[a.i];
      a.buf[a.i] = s + b * 0.5;
      a.i = (a.i + 1) % a.d;
      s = b - s;
    }
    out[n] = s * 0.18;
  }
  return out;
}
// Ping-pong delay at a dotted eighth, fed into the reverb.
{
  const d = Math.round(SR * BEAT * 0.75);
  const tl = new Float32Array(N), tr = new Float32Array(N);
  for (let n = d; n < N; n++) {
    tl[n] = (DLY.L[n - d] + DLY.R[n - d]) * 0.5 + tr[n - d] * 0.45;
    tr[n] = tl[n - d] * 0.8;
  }
  for (let n = 0; n < N; n++) { MUS.L[n] += tl[n] * 0.35; MUS.R[n] += tr[n] * 0.35; VERB.L[n] += tl[n] * 0.15; VERB.R[n] += tr[n] * 0.15; }
}
const rl = reverb(VERB.L, 0), rr = reverb(VERB.R, 23);

// Music filter automation: warm in the intro, open at the drop, closed while
// Quill listens, sweeping back open from release to paste.
function cutoff(t) {
  if (t < C.searchIn) return 1600;
  if (t < 8) return 1600 + (t - C.searchIn) * 1300;
  if (t < LISTEN[0] - 0.3) return 18000;
  if (t < LISTEN[1]) return 18000 - 17200 * Math.min(1, (t - (LISTEN[0] - 0.3)) / 0.4);
  if (t < C.pasted + 0.3) return 800 * Math.pow(18000 / 800, (t - LISTEN[1]) / (C.pasted + 0.3 - LISTEN[1]));
  return 18000;
}
const fL = svf(), fR = svf();
const outL = new Float32Array(N), outR = new Float32Array(N);
let ki = 0, peak = 0;
for (let n = 0; n < N; n++) {
  const t = n / SR;
  while (ki + 1 < kicks.length && kicks[ki + 1] <= t) ki++;
  const since = kicks[ki] <= t ? t - kicks[ki] : 9;
  const duck = 1 - 0.45 * (1 - Math.exp(-since / 0.004)) * Math.exp(-since / 0.12);
  const open = t >= C.untilNow && t < DROP ? 0 : 1; // the dead stop before the drop
  const fc = cutoff(t);
  const mL = fL(MUS.L[n], fc, 0.8).lp * duck * open;
  const mR = fR(MUS.R[n], fc, 0.8).lp * duck * open;
  const fade = (t > DUR - 1.4 ? Math.max(0, (DUR - t) / 1.4) : 1) * Math.min(1, Math.max(0, t - TL.start) / 0.03);
  outL[n] = Math.tanh((mL + KICK.L[n] + SFX.L[n] + rl[n] * (open || 0.5)) * 1.1 * fade);
  outR[n] = Math.tanh((mR + KICK.R[n] + SFX.R[n] + rr[n] * (open || 0.5)) * 1.1 * fade);
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
