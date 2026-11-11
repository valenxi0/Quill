// Dot scope's simulation, ported from Hex (https://github.com/anomalyco/hex,
// MIT, see NOTICE.md): src/rune_hud/dot_scope.rs, src/rune_hud/dot_scope/scope.rs
// and src/rune_hud/spring.rs. It steps the voice, two braided traces and the
// phosphor at a fixed 240 Hz and yields one (brightness, freshness) pair per
// dot. Only Hex's default processing look, Flow, is ported. Quill additions:
// the field collapses after done and failed, so the outcome text can take
// its place, and `cancelled` clears like done.

const TAU = Math.PI * 2;
const STEP = 1 / 240;
const HISTORY = 512;
const SAMPLES_PER_COLUMN = 3;
const SPEEDS = [0.8, 1.25, 1.85, 2.6];
const BASE = [0.55, 0.42, 0.3, 0.2];
const SPATIAL = [1.4, 2.4, 3.8, 5.6];
const START_WIDTH = 30;
const ENTER = 0.42;
const WIPE = 0.3;
const PHOSPHOR_HEAD = 0.15;
const BREATH_PERIOD = 5.5;
const HIDDEN_SCALE = 0.9;
const HIDDEN_SOFTNESS = 3;
const WORK_HELIX = 2.6;
const FLOW_SPACING = 0.7;
const CARRIER_REST = 0.14;
/** Quill: how long after done or failed the field starts collapsing, s. */
const COLLAPSE_AFTER = 0.36;

export type Stage = "recording" | "processing" | "done" | "failed";

export interface Tuning {
  columns: number;
  rows: number;
  /** Dot pitch, px. */
  spacing: number;
  /** Plate width while recording, px. */
  width: number;
  /** Plate width while transcribing and cleaning up, px. */
  processingWidth: number;
  height: number;
  braidSpeed: number;
  helixCycles: number;
  gain: number;
  /** The amplitude spring's frequency, Hz. */
  response: number;
  /** How long the level takes to fall, s. */
  release: number;
  /** How fast the voice's history travels out from the centre, px/s. */
  historySpeed: number;
  /** The phosphor tail, s. */
  phosphor: number;
  glow: number;
  /** One processing pass, s. */
  shimmerPass: number;
  shimmer: number;
  /** Enter, exit and morph duration, s. */
  transition: number;
  bounce: number;
  autoRelease: number;
  /** Input below this never lights the scope, dBFS. */
  noiseFloor: number;
}

/** Hex's defaults: 43 x 9 dots at 2.6 pt on a 112 x 26 pt plate. */
export const HEX_TUNING: Tuning = {
  columns: 43,
  rows: 9,
  spacing: 2.6,
  width: 112,
  processingWidth: 76,
  height: 26,
  braidSpeed: 1,
  helixCycles: 1.6,
  gain: 1,
  response: 6.5,
  release: 0.15,
  historySpeed: 150,
  phosphor: 0.6,
  glow: 1,
  shimmerPass: 0.55,
  shimmer: 1,
  transition: 0.3,
  bounce: 0.15,
  autoRelease: 3,
  noiseFloor: -60,
};

/**
 * Hex's tuning for a screen's pixel density. From 150% scaling up it is
 * Hex's own. Below that a 2.6 px pitch draws dots one or two pixels wide,
 * which read as specks, so the pitch stays at 4 device pixels with 7 rows
 * and the plate grows to fit them, keeping Hex's proportions. `width`
 * narrows the plate and drops columns, for a preview with less room.
 */
export function tuningFor(dpr: number, width?: number): Tuning {
  let tuning = HEX_TUNING;
  if (dpr < 1.5) {
    const spacing = 4 / Math.max(1, dpr);
    const rows = 7;
    const height = rows * spacing + 4;
    tuning = { ...HEX_TUNING, rows, spacing, height, width: (height * HEX_TUNING.width) / HEX_TUNING.height };
  }
  const plate = Math.min(width ?? tuning.width, tuning.width);
  return {
    ...tuning,
    width: plate,
    columns: 2 * Math.floor((plate / tuning.spacing - 1) / 2) + 1,
    processingWidth: (plate * HEX_TUNING.processingWidth) / HEX_TUNING.width,
  };
}

/** Hex Lab's synthetic voice: phrases of syllables with breaths between, 0..1. */
export function demoLevel(time: number): number {
  const PHRASE = 2.4;
  const SPOKEN = 1.7;
  const phrase = Math.floor(time / PHRASE);
  const t = time - phrase * PHRASE;
  if (t > SPOKEN) return 0.04;
  const shape = Math.pow(Math.sin((Math.PI * t) / SPOKEN), 0.4);
  const rate = TAU * (4.3 + 0.6 * Math.sin(phrase * 1.7));
  const syllable = 0.5 + 0.5 * Math.sin(rate * t + phrase);
  const stress = 0.75 + 0.25 * Math.sin(TAU * 1.1 * t + 2 * phrase);
  return clamp(0.08 + 0.82 * shape * Math.pow(syllable, 1.6) * stress, 0, 1);
}

/** [`demoLevel`] as a laptop microphone hears it, dBFS. */
export function demoLoudness(time: number): number {
  return -58 + 28 * demoLevel(time);
}

function clamp(x: number, lo: number, hi: number) {
  return Math.min(hi, Math.max(lo, x));
}

export function smoothstep(a: number, b: number, x: number) {
  const t = clamp((x - a) / (b - a), 0, 1);
  return t * t * (3 - 2 * t);
}

function bump(d: number, w: number) {
  return Math.exp(-(d / w) * (d / w));
}

function hermite(start: number, speed: number, end: number, t: number) {
  t = clamp(t, 0, 1);
  const t2 = t * t;
  const t3 = t2 * t;
  return (2 * t3 - 3 * t2 + 1) * start + (t3 - 2 * t2 + t) * WIPE * speed + (3 * t2 - 2 * t3) * end;
}

function passPosition(p: number) {
  return 0.5 - 0.5 * Math.cos(Math.PI * clamp(p, 0, 1));
}

interface Spec {
  duration: number;
  bounce: number;
}

const REDUCED_FADE: Spec = { duration: 0.16, bounce: 0 };

/** A frame-rate independent damped spring, solved exactly. */
class Spring {
  v = 0;
  target: number;
  constructor(public x: number) {
    this.target = x;
  }

  jump(x: number) {
    this.x = x;
    this.v = 0;
    this.target = x;
  }

  stepTo(target: number, spec: Spec, dt: number) {
    this.target = target;
    const omega = TAU / Math.max(spec.duration, 0.01);
    const zeta = spec.bounce >= 0 ? Math.max(1 - spec.bounce, 0) : 1 / Math.max(1 + spec.bounce, 0.01);
    const x0 = this.x - target;
    const v0 = this.v;
    if (zeta < 1 - 1e-4) {
      const decay = zeta * omega;
      const omegaD = omega * Math.sqrt(1 - zeta * zeta);
      const e = Math.exp(-decay * dt);
      const sin = Math.sin(omegaD * dt);
      const cos = Math.cos(omegaD * dt);
      const sinOverD = sin / omegaD;
      this.x = target + e * (x0 * (cos + decay * sinOverD) + v0 * sinOverD);
      this.v = e * (-x0 * omega * omega * sinOverD + v0 * (cos - decay * sinOverD));
    } else if (zeta > 1 + 1e-4) {
      const root = Math.sqrt(zeta * zeta - 1);
      const slow = -omega / (zeta + root);
      const fast = -omega * (zeta + root);
      const denom = slow - fast;
      const se = Math.exp(slow * dt);
      const fe = Math.exp(fast * dt);
      this.x = target + ((-fast * se + slow * fe) / denom) * x0 + ((se - fe) / denom) * v0;
      this.v = ((slow * fast * (fe - se)) / denom) * x0 + ((slow * se - fast * fe) / denom) * v0;
    } else {
      const e = Math.exp(-omega * dt);
      const c = v0 + omega * x0;
      this.x = target + (x0 + c * dt) * e;
      this.v = (v0 - omega * c * dt) * e;
    }
  }
}

const AUTO_MIN_SPAN = 16;
const AUTO_RANGE = 24;
const AUTO_ROOM_MARGIN = 5;
const AUTO_ATTACK = 0.04;
const ROOM_FALL = 0.3;
const ROOM_RISE = 6;

/** Follows the speaker's recent peak and the room, so any microphone moves the scope alike. */
class AutoLevel {
  peak = -30;
  room = -55;

  step(loudness: number, h: number, release: number, noiseFloor: number) {
    const db = Math.max(loudness, -120);
    const follow = (value: number, target: number, time: number) => value + (target - value) * (1 - Math.exp(-h / Math.max(time, 1e-3)));
    this.room = follow(this.room, db, db < this.room ? ROOM_FALL : ROOM_RISE);
    const lowest = Math.max(this.room, noiseFloor) + AUTO_MIN_SPAN;
    this.peak = db > this.peak ? follow(this.peak, db, AUTO_ATTACK) : follow(this.peak, lowest, release);
    this.peak = Math.max(this.peak, lowest);
    const low = Math.max(Math.max(this.room, noiseFloor) + AUTO_ROOM_MARGIN, this.peak - AUTO_RANGE);
    return clamp((db - low) / Math.max(this.peak - low, 1), 0, 1);
  }
}

class Voice {
  level = 0;
  reservoir = 0;
  energy = 0;
  kick = 0;
  kickVelocity = 0;
  band = 0;

  step(raw: number, h: number) {
    const previous = this.level;
    this.level += (raw - this.level) * (1 - Math.exp(-h / (raw > this.level ? 0.035 : 0.095)));
    const rise = Math.max((this.level - previous) / h, 0);
    this.reservoir += (1.75 * this.level * this.level - this.reservoir / 1.8) * h;
    this.energy = 1 - Math.exp(-this.reservoir * 1.45);
    const w = TAU * 1.3;
    const z = 0.22;
    this.kickVelocity += (-w * w * this.kick - 2 * z * w * this.kickVelocity + 15 * rise + 2.2 * this.level) * h;
    this.kick += this.kickVelocity * h;
    this.band += (raw - this.band) * (1 - Math.exp(-h / (raw > this.band ? 0.025 : 0.14)));
  }
}

class Trace {
  envelope = 0;
  amplitude = 0;
  amplitudeVelocity = 0;
  glowFast = 0;
  glowSpring = 0;
  glowSpringVelocity = 0;
  glowSlow = 0;
  glow = 0;
  history = new Float32Array(HISTORY * 5);
  head = 0;

  constructor(public phases: number[]) {}

  tick(voice: Voice, live: number, speed: number, tuning: Tuning, h: number) {
    const follow = (value: number, target: number, rise: number, fall: number) =>
      value + (target - value) * (1 - Math.exp(-h / (target > value ? rise : fall)));
    this.envelope = follow(this.envelope, voice.level, 0.022, tuning.release);
    const w = TAU * tuning.response;
    this.amplitudeVelocity += (-w * w * (this.amplitude - this.envelope) - 2 * 0.52 * w * this.amplitudeVelocity) * h;
    this.amplitude += this.amplitudeVelocity * h;
    this.glowFast = follow(this.glowFast, voice.level, 0.045, 0.13);
    const gw = TAU * 3.6;
    this.glowSpringVelocity += (-gw * gw * (this.glowSpring - this.glowFast) - 2 * 0.5 * gw * this.glowSpringVelocity) * h;
    this.glowSpring += this.glowSpringVelocity * h;
    this.glowSlow = follow(this.glowSlow, voice.level, 0.15, 0.7);
    const swell = 0.8 * Math.max(this.glowSpring - 0.25, 0) + 0.04 * Math.max(voice.kick, -0.5);
    this.glow = Math.min(Math.max(swell, 0) + 0.42 * this.glowSlow + 0.2 * voice.energy, 1.15) * live;
    const go = 0.6 + 1.1 * Math.min(this.envelope * 1.6, 1);
    for (let c = 0; c < 4; c++) this.phases[c] = (this.phases[c]! + h * TAU * SPEEDS[c]! * go * speed) % TAU;
    const emphasis = 0.35 + 0.65 * Math.min(voice.band / (voice.band + 0.06), 2);
    const offset = this.head * 5;
    const amplitude = Math.max(this.amplitude, 0) * live;
    this.history[offset] = amplitude;
    for (let c = 0; c < 4; c++) this.history[offset + 1 + c] = amplitude * BASE[c]! * emphasis;
    this.head = (this.head + 1) % HISTORY;
  }

  past(delay: number, channel: number) {
    const d = Math.min(delay / STEP, HISTORY - 2);
    const i = Math.floor(d);
    const k = d - i;
    const a = (this.head + 2 * HISTORY - 1 - i) % HISTORY;
    const b = (a + HISTORY - 1) % HISTORY;
    return this.history[a * 5 + channel]! * (1 - k) + this.history[b * 5 + channel]! * k;
  }
}

type Envelope =
  | { kind: "enter"; reach: number; head: number; unfold: number }
  | { kind: "work"; offset: number; spacing: number; head: number; swell: number }
  | { kind: "clear"; front: number; head: number; offset: number; spacing: number; glow: number; swell: number };

function flowLight(x: number, offset: number, spacing: number) {
  const r = (x - offset + spacing / 2) % spacing;
  const d = (r < 0 ? r + spacing : r) - spacing / 2;
  return bump(d, 7);
}

/** (amplitude, brightness, swell) at `x`. */
function envelopeAt(e: Envelope, x: number): [number, number, number] {
  switch (e.kind) {
    case "enter": {
      const d = Math.abs(x);
      const drawn = e.unfold * (1 - smoothstep(Math.max(e.reach - 14, 0), Math.max(e.reach, 0.001), d));
      const amplitude = drawn * (1 - smoothstep(Math.max(e.reach - 22, 0), Math.max(e.reach - 3, 0.001), d));
      const beam = e.head * bump(d - e.reach, 6);
      const swell = 1.8 * e.unfold * (e.head / 2.8) * bump(d - Math.max(e.reach - 14, 0), 14);
      return [amplitude, amplitude + beam, swell];
    }
    case "work": {
      const light = flowLight(x, e.offset, e.spacing);
      return [1, 1 + e.head * light, e.swell * light];
    }
    case "clear": {
      const d = Math.abs(x);
      const keep = smoothstep(e.front - 12, e.front - 2, d);
      const light = flowLight(x, e.offset, e.spacing);
      return [keep, keep * (1 + e.glow * light) + e.head * bump(d - e.front, 6), keep * e.swell * light];
    }
  }
}

export interface Pose {
  width: number;
  height: number;
  opacity: number;
  scale: number;
  softness: number;
  cool: number;
  fail: number;
  glow: number;
  grid: number;
  live: number;
}

export interface Frame {
  stage: Stage;
  /** The microphone's loudness, dBFS. */
  loudness: number;
  reduceMotion: boolean;
}

export class Scope {
  readonly tuning: Tuning;
  stage: Stage = "recording";
  private elapsed = 0;
  private speedAtDone = 0;
  private flowOffset = 0;
  private flowTurn = 0;
  private auto = new AutoLevel();
  private voices = [new Voice(), new Voice()];
  private traces = [new Trace([0.1, 1.7, 3.3, 4.6]), new Trace([2.4, 0.6, 5.1, 1.9])];
  private live = 0;
  private carrier = 0;
  private time = 0;
  private steps = 0;
  private pending = 0;
  private reduceMotion: boolean;
  readonly columns: number;
  readonly rows: number;
  private headLight: Float32Array[];
  private tailLight: Float32Array[];
  /** Each dot's brightness and freshness, row by row, interleaved. */
  readonly dots: Float32Array;
  private ys: Float32Array[];
  private brightness: Float32Array[];
  private grid = 0;
  private width = new Spring(START_WIDTH);
  private opacity = new Spring(0);
  private scale = new Spring(HIDDEN_SCALE);
  private softness = new Spring(HIDDEN_SOFTNESS);
  private cool = new Spring(0);
  private fail = new Spring(0);

  /** Starts a recording from nothing; with reduced motion the plate is already open. */
  constructor(tuning: Tuning, reduceMotion: boolean) {
    this.tuning = tuning;
    this.reduceMotion = reduceMotion;
    this.columns = tuning.columns;
    this.rows = tuning.rows;
    const dots = this.columns * this.rows;
    const samples = (this.columns - 1) * SAMPLES_PER_COLUMN + 1;
    this.headLight = [new Float32Array(dots), new Float32Array(dots)];
    this.tailLight = [new Float32Array(dots), new Float32Array(dots)];
    this.dots = new Float32Array(dots * 2);
    this.ys = [new Float32Array(samples), new Float32Array(samples)];
    this.brightness = [new Float32Array(samples), new Float32Array(samples)];
    if (reduceMotion) {
      this.scale.jump(1);
      this.softness.jump(0);
      this.width.jump(tuning.width);
    }
  }

  pose(): Pose {
    const collapsing = this.collapsing();
    return {
      width: collapsing ? Math.max(this.width.x, 0) : Math.max(this.width.x, this.tuning.height),
      height: this.tuning.height,
      opacity: clamp(this.opacity.x, 0, 1),
      scale: Math.max(this.scale.x, 0.01),
      softness: clamp(this.softness.x, 0, HIDDEN_SOFTNESS),
      cool: clamp(this.cool.x, 0, 1),
      fail: clamp(this.fail.x, 0, 1),
      glow: ((this.traces[0]!.glow + this.traces[1]!.glow) / 2) * this.tuning.glow,
      grid: this.grid,
      live: this.live,
    };
  }

  private collapsing() {
    return (this.stage === "done" || this.stage === "failed") && this.elapsed > COLLAPSE_AFTER;
  }

  /** Advances by `dt` seconds in fixed 240 Hz steps. */
  advance(dt: number, frame: Frame) {
    if (frame.stage !== this.stage) {
      if (frame.stage === "done") this.speedAtDone = 1.5 * this.pulseSpeed();
      if (frame.stage === "processing") this.flowOffset = -this.width.x / 2 - 4;
      this.stage = frame.stage;
      this.elapsed = 0;
    }
    this.reduceMotion = frame.reduceMotion;
    this.pending += clamp(dt, 0, 0.25);
    while (this.pending >= STEP) {
      this.pending -= STEP;
      this.step(frame);
    }
  }

  private step(frame: Frame) {
    const h = STEP;
    const tuning = this.tuning;
    const recording = this.stage === "recording";
    const level = recording ? clamp(this.auto.step(frame.loudness, h, tuning.autoRelease, tuning.noiseFloor) * tuning.gain, 0, 1) : 0;
    this.voices[0]!.step(level, h);
    this.voices[1]!.step(level * 0.85, h);
    this.live += ((recording ? 1 : 0) - this.live) * (1 - Math.exp(-h / 0.55));
    const speed = frame.reduceMotion ? 0 : tuning.braidSpeed;
    for (let k = 0; k < 2; k++) this.traces[k]!.tick(this.voices[k]!, this.live, speed, tuning, h);
    const energy = Math.max(this.voices[0]!.energy, this.voices[1]!.energy);
    const working = this.stage === "processing" || this.stage === "done";
    this.flowTurn += ((working ? 1 : 0) - this.flowTurn) * (1 - Math.exp(-h / 0.25));
    if (working) this.flowOffset += this.flowSpeed() * h;
    const rest = CARRIER_REST + 0.22 * energy;
    const turn = rest + (this.flowTurnHz() - rest) * this.flowTurn;
    this.carrier = (this.carrier + h * TAU * turn * speed) % TAU;

    const width = this.collapsing()
      ? 0
      : recording
        ? tuning.width
        : this.stage === "failed"
          ? this.width.target
          : tuning.processingWidth;
    const cool = working ? 1 : 0;
    const fail = this.stage === "failed" ? 1 : 0;
    const fade: Spec = { duration: tuning.transition, bounce: 0 };
    const bouncy: Spec = { duration: tuning.transition, bounce: tuning.bounce };
    if (frame.reduceMotion) {
      this.width.jump(width);
      this.scale.jump(1);
      this.softness.jump(0);
      this.opacity.stepTo(1, REDUCED_FADE, h);
      this.cool.stepTo(cool, REDUCED_FADE, h);
      this.fail.stepTo(fail, REDUCED_FADE, h);
    } else {
      this.width.stepTo(width, bouncy, h);
      this.opacity.stepTo(1, fade, h);
      this.scale.stepTo(1, bouncy, h);
      this.softness.stepTo(0, fade, h);
      this.cool.stepTo(cool, fade, h);
      this.fail.stepTo(fail, fade, h);
    }

    this.elapsed += h;
    this.time += h;
    this.steps += 1;
    if (this.steps % 2 === 0) this.beam(2 * h);
  }

  private workPeriod() {
    return 2 * Math.max(this.tuning.shimmerPass, 0.05);
  }

  private flowSpacing() {
    return FLOW_SPACING * this.tuning.processingWidth;
  }

  private flowSpeed() {
    return this.flowSpacing() / this.workPeriod();
  }

  private flowTurnHz() {
    return (this.flowSpeed() * this.tuning.helixCycles) / Math.max(this.width.x, 1);
  }

  private pulseSpeed() {
    return (this.tuning.processingWidth / 2 + 12) / (1.4 * this.workPeriod());
  }

  private scan(width: number): { helix: number; decay: number; tail: number; grid: number; envelope: Envelope } {
    const e = this.elapsed;
    const still = { helix: 1, decay: 1, tail: 1, grid: 1, envelope: { kind: "enter", reach: 1e4, head: 0, unfold: 1 } as Envelope };
    if (this.stage === "failed" || this.reduceMotion) return still;
    if (this.stage === "recording") {
      return {
        helix: 1,
        decay: 1,
        tail: 1,
        grid: Math.min(e / 0.3, 1),
        envelope: {
          kind: "enter",
          reach: (width / 2 + 12) * passPosition(e / ENTER),
          head: 2.8 * (1 - smoothstep(0.08, ENTER, e)),
          unfold: smoothstep(0, 0.1, e),
        },
      };
    }
    if (this.stage === "processing") {
      const enter = smoothstep(0, 0.35, e);
      return {
        helix: 1 + (WORK_HELIX - 1) * enter,
        decay: 1,
        tail: 1,
        grid: 1,
        envelope: { kind: "work", offset: this.flowOffset, spacing: this.flowSpacing(), head: 2.5 * smoothstep(0.05, 0.3, e) * this.tuning.shimmer, swell: 0.9 * enter },
      };
    }
    return {
      helix: WORK_HELIX,
      decay: 1 - 0.6 * smoothstep(0, 0.12, e),
      tail: 1 - smoothstep(0, 0.15, e),
      grid: 1 - smoothstep(0.2, 0.4, e),
      envelope: {
        kind: "clear",
        front: hermite(0, this.speedAtDone, width / 2 + 8, e / WIPE),
        head: 2.8 * smoothstep(0, 0.04, e) * (1 - smoothstep(0.22, 0.32, e)),
        offset: this.flowOffset,
        spacing: this.flowSpacing(),
        glow: 2.5 * this.tuning.shimmer * (1 - smoothstep(0, 0.18, e)),
        swell: 0.9,
      },
    };
  }

  /** Draws both traces into the dot field and decays the phosphor. */
  private beam(hb: number) {
    const { columns, rows } = this;
    const sub = SAMPLES_PER_COLUMN;
    const samples = (columns - 1) * sub + 1;
    const pitch = this.tuning.spacing;
    const width = Math.max(this.width.x, this.tuning.height);
    const scan = this.scan(width);
    this.grid = scan.grid;
    const mid = (rows - 1) / 2;
    const half = mid - 0.55;
    const breath = this.reduceMotion ? 0.5 : 0.5 + 0.5 * Math.sin((this.time * TAU) / BREATH_PERIOD);
    const rest = (0.55 + 0.35 * breath) * (0.4 + 0.6 * this.live) * scan.helix;
    const halfWidth = width / 2;
    for (let k = 0; k < 2; k++) {
      const trace = this.traces[k]!;
      const sign = k === 0 ? 1 : -1;
      const ys = this.ys[k]!;
      const brightness = this.brightness[k]!;
      for (let s = 0; s < samples; s++) {
        const x = (s / sub - (columns - 1) / 2) * pitch;
        if (Math.abs(x) > halfWidth) {
          ys[s] = mid;
          brightness[s] = 0;
          continue;
        }
        const [amplitude, bright, swell] = envelopeAt(scan.envelope, x);
        const own = Math.abs(x);
        const xn = (x + halfWidth) / width;
        const ends = Math.min(smoothstep(-halfWidth + 3, -halfWidth + 17, x), smoothstep(-halfWidth + 3, -halfWidth + 17, -x));
        const reach = 1 - 0.8 * smoothstep(14, halfWidth, own);
        const delay = own / this.tuning.historySpeed;
        const a = trace.past(delay, 0);
        let spread = 0;
        for (let c = 0; c < 4; c++) spread += trace.past(delay, 1 + c) * Math.sin(TAU * SPATIAL[c]! * xn - trace.phases[c]!);
        const helix = (rest + swell + half * 0.55 * a * reach) * Math.sin(TAU * this.tuning.helixCycles * xn - this.carrier);
        const y = half * Math.tanh((helix + half * 0.7 * spread * reach) / half) * ends * amplitude;
        ys[s] = mid - sign * y;
        const spatial = (0.3 + 0.7 * Math.min(a * 1.8, 1)) * (0.45 + 0.55 * ends);
        brightness[s] = spatial * amplitude + 0.36 * Math.max(bright - amplitude, 0);
      }
      for (let s = 0; s < samples; s++) {
        const slope = ((ys[Math.min(s + 1, samples - 1)]! - ys[Math.max(s - 1, 0)]!) * sub) / 2;
        brightness[s]! *= 1 - 0.55 + 0.55 / Math.sqrt(1 + 0.22 * slope * slope);
      }
    }

    const headKeep = Math.exp(-hb / (PHOSPHOR_HEAD * scan.decay));
    const tailKeep = Math.exp(-hb / (this.tuning.phosphor * scan.decay));
    const dx = 1 / sub;
    const lit = [0, 0];
    for (let i = 0; i < columns; i++) {
      const s0 = Math.max(i - 3, 0) * sub;
      const s1 = Math.min((i + 3) * sub, samples - 1);
      for (let j = 0; j < rows; j++) {
        const n = j * columns + i;
        let now = 0;
        let total = 0;
        for (let k = 0; k < 2; k++) {
          const ys = this.ys[k]!;
          let best = Number.MAX_VALUE;
          let nearest = i * sub;
          for (let s = s0; s < s1; s++) {
            const ay = ys[s]!;
            const dy = ys[s + 1]! - ay;
            const px = i - s * dx;
            const py = j - ay;
            const t = clamp((px * dx + py * dy) / (dx * dx + dy * dy), 0, 1);
            const ex = px - t * dx;
            const ey = py - t * dy;
            const d2 = ex * ex + ey * ey;
            if (d2 < best) {
              best = d2;
              nearest = s;
            }
          }
          const brightness = this.brightness[k]![nearest]!;
          const onBeam = Math.exp(-best / 0.25);
          const instant = Math.min(onBeam + 0.5 * Math.exp(-Math.sqrt(best) / 0.8), 1.2) * brightness;
          const core = onBeam * brightness;
          const head = Math.max(this.headLight[k]![n]! * headKeep, core);
          const tail = Math.max(this.tailLight[k]![n]! * tailKeep, core);
          this.headLight[k]![n] = head;
          this.tailLight[k]![n] = tail;
          const light = Math.max(instant, head, 0.55 * scan.tail * tail);
          lit[k] = light;
          now += instant;
          total += light;
        }
        const power = 1 - (1 - Math.min(lit[0]!, 1)) * (1 - Math.min(lit[1]!, 1));
        this.dots[n * 2] = clamp(power, 0, 1);
        this.dots[n * 2 + 1] = Math.min(now / Math.max(total, 1e-4), 1);
      }
    }
  }
}

export interface Palette {
  base: [number, number, number];
  core: [number, number, number];
}

type Rgb = [number, number, number];
const mix3 = (a: Rgb, b: Rgb, t: number): Rgb => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];

/** Hex's dark palette: warm pearl while recording, agent blue while working, coral on failure. */
export function palette(pose: Pose): Palette {
  const base = mix3(mix3([0.86, 0.83, 0.77], [0.7, 0.78, 0.9], pose.cool), [1, 0.38, 0.31], pose.fail);
  const core = mix3(mix3([0.98, 0.96, 0.92], [0.92, 0.95, 1], pose.cool), [1, 0.66, 0.6], pose.fail);
  return { base, core };
}

/** Hex's dark plate, rim and shadow. */
export const PLATE = "rgb(17 17 20 / 0.94)";
export const RIM = "rgb(255 255 255 / 0.11)";
export const SHADOW = "rgb(0 0 0 / 0.5)";
