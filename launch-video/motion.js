// Motion primitives. Every function is a pure function of time, so seek(t)
// can paint frame 812 without simulating frames 0 to 811.
(function () {
  const clamp = (x, a = 0, b = 1) => Math.min(b, Math.max(a, x));
  const lerp = (a, b, t) => a + (b - a) * t;
  const prog = (t, a, b) => (t <= a ? 0 : t >= b ? 1 : (t - a) / (b - a));

  function bezier(x1, y1, x2, y2) {
    const cx = 3 * x1, bx = 3 * (x2 - x1) - cx, ax = 1 - cx - bx;
    const cy = 3 * y1, by = 3 * (y2 - y1) - cy, ay = 1 - cy - by;
    const sx = (t) => ((ax * t + bx) * t + cx) * t;
    const sy = (t) => ((ay * t + by) * t + cy) * t;
    return (x) => {
      if (x <= 0) return 0;
      if (x >= 1) return 1;
      let lo = 0, hi = 1, t = x;
      for (let i = 0; i < 40; i++) {
        if (sx(t) < x) lo = t;
        else hi = t;
        t = (lo + hi) / 2;
      }
      return sy(t);
    };
  }

  // transitions.dev tokens: smooth ease-out for moves, ease-in-out for swaps.
  const easeOut = bezier(0.22, 1, 0.36, 1);
  const inOut = bezier(0.42, 0, 0.58, 1);
  const easeIn = bezier(0.55, 0, 0.8, 0.2);

  // Closed-form damped spring from 0 to 1, started at t = 0.
  function spring(t, { k = 170, c = 26, m = 1 } = {}) {
    if (t <= 0) return 0;
    const w0 = Math.sqrt(k / m);
    const z = c / (2 * Math.sqrt(k * m));
    if (z < 1) {
      const wd = w0 * Math.sqrt(1 - z * z);
      return 1 - Math.exp(-z * w0 * t) * (Math.cos(wd * t) + ((z * w0) / wd) * Math.sin(wd * t));
    }
    return 1 - Math.exp(-w0 * t) * (1 + w0 * t);
  }

  const S = {
    snappy: { k: 420, c: 32 }, // UI: pills, buttons, cursors
    ui: { k: 260, c: 25 },
    def: { k: 170, c: 26 }, // cards, containers, camera
    heavy: { k: 110, c: 22 }, // big type, logo lockups
    playful: { k: 260, c: 13 }, // visible overshoot
  };

  // A value that changes target several times: one spring per change, summed.
  function track(t, v0, changes) {
    let v = v0, prev = v0;
    for (const [at, to, cfg] of changes) {
      v += (to - prev) * spring(t - at, cfg || S.def);
      prev = to;
    }
    return v;
  }

  // Smooth value noise for organic wobble.
  function noise(x) {
    const i = Math.floor(x), f = x - i;
    const h = (n) => {
      const s = Math.sin(n * 127.1 + 311.7) * 43758.5453;
      return s - Math.floor(s);
    };
    const u = f * f * (3 - 2 * f);
    return lerp(h(i), h(i + 1), u);
  }

  window.M = { clamp, lerp, prog, bezier, easeOut, inOut, easeIn, spring, S, track, noise };
})();
