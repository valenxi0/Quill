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

  const typed = typing(["Typing every word by hamd", { back: 2 }, "nd is slow."], 1.75, 19, 7);
  const search = typing(["voice dictation app for windows"], 4.6, 38, 11);

  // Spoken words in the demo, with when each starts (seconds).
  const speechStart = 13.45;
  const speech = [
    ["um", 0.0, "filler"],
    ["so", 0.28, "filler"],
    ["I", 0.62],
    ["think", 0.74],
    ["we", 0.98],
    ["should", 1.1],
    ["uh", 1.42, "filler"],
    ["ship", 1.78],
    ["the", 1.98],
    ["windows", 2.08, "", "Windows"],
    ["build", 2.42],
    ["on", 2.62],
    ["friday", 2.74, "", "Friday."],
  ].map(([w, at, kind = "", clean]) => ({ w, t: speechStart + at, kind, clean: clean ?? w }));

  const TL = {
    BPM,
    BEAT,
    duration: 31,
    fps: 60,
    typed,
    search,
    speech,
    scenes: {
      cold: [0, 1.5],
      typing: [1.5, 4.6],
      search: [4.5, 8.05],
      turn: [8.0, 9.1],
      reveal: [8.95, 12.75],
      demo: [12.4, 19.75],
      feat: [19.4, 25.75],
      end: [25.4, 31],
    },
    // Cue times shared with the score.
    cues: {
      // Cold open: 0–1.5 s replays the demo from coldFrom at coldRate, then rewinds.
      coldFrom: 16.95,
      coldRate: 0.8,
      searchIn: 4.5,
      searchEnter: 5.5,
      cardsIn: 5.55,
      clicks: [6.0],
      denied: [6.0, 6.25, 6.5, 6.75],
      verdict: 5.8,
      untilNow: 8.0,
      drop: 9.0,
      tileIn: 9.5,
      wordmark: 10.25,
      tagline: 11.0,
      toDemo: 12.5,
      recStart: 13.3,
      transcribing: 16.5,
      cleaning: 16.85,
      pasted: 17.35,
      sent: 18.0,
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
