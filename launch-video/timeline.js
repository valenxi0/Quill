// Shared timing for the picture (index.html) and the score (audio.mjs).
// Everything is in seconds; one beat is 0.5 s at 120 BPM.
(function (root) {
  const BPM = 120;
  const BEAT = 60 / BPM;

  function rng(seed) {
    return () => {
      seed = (seed + 0x6d2b79f5) | 0;
      let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }

  // A keystroke schedule: strings are typed, {back:n} deletes n characters.
  function typing(ops, t0, cps, seed) {
    const r = rng(seed);
    let t = t0;
    let text = "";
    const states = [{ t: t0, text: "" }];
    const keys = [];
    for (const op of ops) {
      if (typeof op === "string") {
        for (const ch of op) {
          t += (1 / cps) * (0.55 + r() * 0.9);
          text += ch;
          states.push({ t, text });
          keys.push(t);
        }
      } else if (op.back) {
        t += op.pause ?? 0.22;
        for (let i = 0; i < op.back; i++) {
          t += 0.075;
          text = text.slice(0, -1);
          states.push({ t, text });
          keys.push(t);
        }
        t += 0.06;
      }
    }
    return { states, keys, end: t };
  }

  const typed = typing(["Typing every word by hamd", { back: 2 }, "nd is slow."], 0.3, 19, 7);
  const search = typing(["voice dictation app for windows"], 3.25, 30, 11);

  // Spoken words in the demo: start times measured from sfx/voice.wav (Whisper word alignment
  // plus the loudness envelope for the "uh", which runs into "ship").
  const speechStart = 12.9;
  const speech = [
    ["um", 0.0, "filler"],
    ["I", 0.58],
    ["think", 0.7],
    ["we", 0.96],
    ["should", 1.16],
    ["uh", 1.55, "filler"],
    ["ship", 1.95],
    ["the", 2.24],
    ["windows", 2.38, "", "Windows"],
    ["build", 2.78],
    ["on", 3.04],
    ["friday", 3.24, "", "Friday."],
  ].map(([w, at, kind = "", clean]) => ({ w, t: speechStart + at, kind, clean: clean ?? w }));

  const TL = {
    BPM,
    BEAT,
    start: 0,
    duration: 31,
    fps: 60,
    typed,
    search,
    speech,
    speechStart,
    scenes: {
      typing: [0, 3.1],
      search: [3.0, 8.05],
      turn: [8.0, 9.1],
      reveal: [8.95, 12.75],
      demo: [12.4, 19.75],
      feat: [19.4, 25.75],
      end: [25.4, 31],
    },
    // Cue times shared with the score.
    cues: {
      searchIn: 3.0,
      searchEnter: 4.5,
      cardsIn: 4.6,
      clicks: [5.2, 5.85, 6.5],
      denied: [5.2, 5.85, 6.5, 6.85],
      verdict: 4.95,
      verdictFocus: 7.05,
      untilNow: 8.0,
      drop: 9.0,
      tileIn: 9.5,
      wordmark: 10.25,
      tagline: 11.0,
      toDemo: 12.5,
      recStart: 12.85,
      transcribing: 16.75,
      cleaning: 17.1,
      pasted: 17.6,
      sent: 18.25,
      features: [19.5, 22.0, 24.0],
      styleTabs: [19.8, 20.35, 20.9, 21.45],
      learnClick: 23.1,
      endCard: 25.5,
      fadeOut: 30.2,
    },
    rng,
  };

  if (typeof module !== "undefined") module.exports = TL;
  else root.TL = TL;
})(this);
