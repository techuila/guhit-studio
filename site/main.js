/* GUHIT Studio site. No dependencies.
   One passive scroll listener, one rAF, all reads batched before all writes.
   Everything degrades: with JS off the acts render in their end state, because
   the `--p` progress variable falls back to 1 in the stylesheet. */

const root = document.documentElement;
const reduced = matchMedia("(prefers-reduced-motion: reduce)");
const narrow = matchMedia("(max-width: 860px)");
const fine = matchMedia("(hover: hover) and (pointer: fine)");
const clamp01 = (n) => (n < 0 ? 0 : n > 1 ? 1 : n);

/* ---------- 0. the load sequence, which is also the loader --------------- */
/* The class is already on <html> from the inline head script, so the first
   paint is navy. From there one clock runs the whole thing:
     0.0 - 1.4s   the drafting grid draws, every line in a random place at its
                  own moment, so the field fills everywhere at once
     1.4 - 1.6s   hold
     1.6 - 3.6s   the mark draws stroke by stroke, the teal arc last
     3.6 - 4.0s   hold on the finished G
     4.0 - 4.9s   the navy field collapses into the hero's own mark
     4.9 - 6.3s   the mark has landed: GUHIT is outlined letter by letter, each
                  fill trailing behind, and the rest of the hero rises in
   The collapse also waits for the page itself: fonts, the images above the
   fold, and the load event, each capped at 4s. If the page is slower than the
   animation the G holds, the arc breathes and a progress line fills; if the
   page is faster the sequence simply plays its own length.
   Any click, key or wheel jumps to the end state; the end state is the hero
   exactly as it renders without the intro. */

const introEl = document.getElementById("intro");
const EASE_OUT = "cubic-bezier(0.22, 1, 0.36, 1)";
const EASE_IN_OUT = "cubic-bezier(0.65, 0, 0.35, 1)";

const T = {
  gridEnd: 1400,   // every grid line has landed by here
  lineMin: 380,    // one grid line draws in lineMin to lineMax ms
  lineMax: 620,
  markAt: 1600,    // the first stroke of the mark
  markDur: 2000,   // through to the end of the teal arc, at 3600
  collapseAt: 4000,
  collapseDur: 900,
  writeStep: 150,  // must match .write tspan in styles.css
  writeDraw: 600,
  writeFill: 200,
  /* The cap is on the hold, not on the signals: it runs from the moment the
     animation is ready to collapse, so a slow page can hold the G but never
     for more than four seconds. A cap measured from the first frame instead
     would always expire before 5.1s and the hold could never be seen. */
  holdCap: 4000,
};
/* the mark's six strokes, offset from T.markAt, in document order:
   frame, wall, partition, bar, leaf, then the teal arc alone for 600ms */
const MARK = [
  { d: 0, t: 360 },
  { d: 360, t: 580 },
  { d: 940, t: 290 },
  { d: 1230, t: 95 },
  { d: 1325, t: 75 },
  { d: 1400, t: 600 },
];

/* ---- what "the page is ready" means, and how far along it is ---------- */
function pageReady(onProgress) {
  const jobs = [];
  const settle = (p) => Promise.resolve(p).then(() => {}, () => {});

  if (document.fonts) {
    /* one job per face, so the progress line has something to say while they
       arrive, then fonts.ready for anything else the stylesheet asked for */
    for (const face of ['600 1em "Archivo"', '400 1em "JetBrains Mono"']) {
      try {
        jobs.push(settle(document.fonts.load(face)));
      } catch {
        /* an engine that will not parse the shorthand: fonts.ready covers it */
      }
    }
    jobs.push(settle(document.fonts.ready));
  }

  /* only what the visitor is actually looking at can hold the loader */
  const vh = innerHeight;
  for (const img of document.images) {
    const r = img.getBoundingClientRect();
    if (r.top < vh && r.bottom > 0 && r.width > 0) {
      jobs.push(settle(img.decode ? img.decode() : Promise.resolve()));
    }
  }

  jobs.push(
    settle(
      document.readyState === "complete"
        ? Promise.resolve()
        : new Promise((res) => addEventListener("load", res, { once: true }))
    )
  );

  const total = jobs.length;
  let done = 0;
  onProgress(0);
  for (const j of jobs) {
    j.then(() => {
      done += 1;
      onProgress(done / total);
    });
  }
  return Promise.all(jobs).then(() => "ready");
}

/* the module is alive, so the head script's short rescue timer can stand down */
try { clearTimeout(window.__introGuard); } catch { /* never set */ }

if (introEl && root.classList.contains("intro-reduced")) {
  runReducedIntro();
} else if (introEl && root.classList.contains("intro-on")) {
  runIntro();
} else if (introEl) {
  /* skipped before the first paint: take the empty overlay out of the page */
  introEl.remove();
}

/* ---- reduced motion: the navy field, the static mark, the progress line,
        then a 300ms fade the moment the page is ready ------------------- */
function runReducedIntro() {
  const mono = document.querySelector(".hero__mark .monogram");
  const bar = document.getElementById("introBar");
  const status = document.getElementById("introStatus");
  let big = "";
  if (mono) {
    const r = mono.getBoundingClientRect();
    const s = (Math.min(innerWidth, innerHeight) * 0.4) / r.width;
    big = `translate(${(innerWidth / 2 - (r.left + r.width / 2)).toFixed(2)}px, ${(innerHeight / 2 - (r.top + r.height / 2)).toFixed(2)}px) scale(${s.toFixed(4)})`;
    mono.style.transform = big;
    introEl.style.setProperty("--markr", `${((r.width * s) / 2).toFixed(1)}px`);
  }
  introEl.classList.add("is-loading");
  if (status) status.textContent = "Loading";

  const end = () => {
    if (mono) mono.style.transform = "";
    if (status) status.textContent = "";
    root.classList.remove("intro-reduced");
    root.classList.add("intro-done");
    introEl.remove();
  };

  const fade = () => {
    if (!introEl.isConnected) return;
    const a = introEl.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 300, easing: "linear", fill: "both" });
    a.finished.then(end).catch(end);
  };
  Promise.race([
    pageReady((p) => {
      if (bar) bar.style.setProperty("--load", p.toFixed(3));
    }),
    new Promise((res) => setTimeout(res, T.holdCap)),
  ]).then(fade);
}

function runIntro() {
  const mono = document.querySelector(".hero__mark .monogram");
  const field = document.getElementById("introField");
  const grid = document.getElementById("introGrid");
  const word = document.querySelector(".wordmark");
  const bar = document.getElementById("introBar");
  const status = document.getElementById("introStatus");
  if (!mono || !field) {
    root.classList.remove("intro-on");
    introEl.remove();
    return;
  }

  let closed = false;
  const live = [];
  const timers = [];
  const at = (ms, fn) => timers.push(setTimeout(fn, Math.max(0, ms)));
  const t0 = performance.now();

  /* ---- 1. the grid, every line in a random place ------------------- */

  const P = 44;
  function buildGrid() {
    if (!grid) return;
    const vw = innerWidth, vh = innerHeight;
    grid.setAttribute("viewBox", `0 0 ${vw} ${vh}`);

    /* line the intro grid up with the hero's own 44px field */
    const hf = document.getElementById("heroField");
    const mod = (n, m) => ((n % m) + m) % m;
    let gx = 0, gy = 0;
    if (hf) {
      const r = hf.getBoundingClientRect();
      gx = mod(r.left, P);
      gy = mod(r.top, P);
    }

    const xs = [], ys = [];
    for (let x = gx; x <= vw; x += P) xs.push(x);
    for (let x = gx - P; x >= 0; x -= P) xs.push(x);
    for (let y = gy; y <= vh; y += P) ys.push(y);
    for (let y = gy - P; y >= 0; y -= P) ys.push(y);

    /* Ordering rule: none. The lines are shuffled, so each one starts in a
       random place, and many draw at once. The starts are spread evenly over
       the window, one per slot and jittered inside it, so the field fills at a
       steady pace with no bursts and no gaps, and every line has landed by
       T.gridEnd. Only the order and the pace of each line are random. */
    const order = xs.map((x) => ({ v: true, p: x })).concat(ys.map((y) => ({ v: false, p: y })));
    for (let i = order.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      const tmp = order[i];
      order[i] = order[j];
      order[j] = tmp;
    }

    const n = order.length;
    const slot = n > 0 ? (T.gridEnd - T.lineMax) / n : 0;
    const ns = "http://www.w3.org/2000/svg";
    const frag = document.createDocumentFragment();
    order.forEach((ln, k) => {
      const el = document.createElementNS(ns, "line");
      /* the path starts where the line should start growing from: the bottom
         for a vertical, the left for a horizontal. Walking the dash offset
         down to zero then grows the visible part from that end. */
      const len = ln.v ? vh : vw;
      if (ln.v) {
        el.setAttribute("x1", ln.p); el.setAttribute("y1", vh);
        el.setAttribute("x2", ln.p); el.setAttribute("y2", 0);
      } else {
        el.setAttribute("x1", 0); el.setAttribute("y1", ln.p);
        el.setAttribute("x2", vw); el.setAttribute("y2", ln.p);
      }
      el.setAttribute("stroke-dasharray", String(len));
      el.setAttribute("stroke-dashoffset", String(len));
      frag.appendChild(el);
      const delay = (k + Math.random()) * slot;
      const duration = T.lineMin + Math.random() * (T.lineMax - T.lineMin);
      live.push(
        el.animate(
          [{ strokeDashoffset: len }, { strokeDashoffset: 0 }],
          { duration, delay, easing: EASE_OUT, fill: "both" }
        )
      );
    });
    grid.appendChild(frag);
    return { lines: n, verticals: xs.length, horizontals: ys.length, slot };
  }
  const gridInfo = buildGrid();

  /* ---- 2. the mark, large and centred, drawn stroke by stroke -------- */

  /* the mark's own navy plate would punch a hole in the grid while it is the
     size of the screen, so it only fills once the collapse starts */
  const plate = mono.querySelector("rect");
  if (plate) plate.style.fill = "transparent";

  /* a transform, so the hero never reflows */
  const r0 = mono.getBoundingClientRect();
  const scale = (Math.min(innerWidth, innerHeight) * 0.4) / r0.width;
  const big = `translate(${(innerWidth / 2 - (r0.left + r0.width / 2)).toFixed(2)}px, ${(innerHeight / 2 - (r0.top + r0.height / 2)).toFixed(2)}px) scale(${scale.toFixed(4)})`;
  mono.style.transform = big;
  mono.classList.add("is-lit");
  introEl.style.setProperty("--markr", `${((r0.width * scale) / 2).toFixed(1)}px`);

  const strokes = Array.from(mono.querySelectorAll(".mg"));
  strokes.forEach((p, i) => {
    const step = MARK[i] || MARK[MARK.length - 1];
    const len = parseFloat(getComputedStyle(p).getPropertyValue("--len")) || 1000;
    live.push(
      p.animate(
        [{ strokeDashoffset: len }, { strokeDashoffset: 0 }],
        { duration: step.t, delay: T.markAt + step.d, easing: EASE_OUT, fill: "both" }
      )
    );
  });

  /* ---- 3. the gate: the animation's own clock, and the page's ------- */

  let armed = false;      // the animation has reached its collapse point
  let ready = false;      // the page has everything it needs
  function tryCollapse() {
    if (armed && ready) collapse();
  }
  at(T.collapseAt, () => {
    armed = true;
    if (!ready && !closed) {
      /* the page is the slow one: hold on the finished G and say so */
      introEl.classList.add("is-loading");
      mono.classList.add("is-waiting");
      if (status) status.textContent = "Loading";
      at(T.holdCap, () => {
        ready = true;
        tryCollapse();
      });
    }
    tryCollapse();
  });
  pageReady((p) => {
    if (bar) bar.style.setProperty("--load", p.toFixed(3));
  }).then(() => {
    ready = true;
    tryCollapse();
  });
  /* last resort: whatever happens, the hero is uncovered by here */
  at(T.collapseAt + T.holdCap + T.collapseDur + 500, endIntro);

  /* ---- 4. the collapse, and the wordmark written over it ------------ */

  function collapse() {
    if (closed) return;
    introEl.classList.remove("is-loading");
    mono.classList.remove("is-waiting");
    if (status) status.textContent = "";

    /* measure the real target now: the mark's own place in the hero */
    if (plate) plate.style.fill = "";
    mono.style.transform = "";
    const t = mono.getBoundingClientRect();
    const vw = innerWidth, vh = innerHeight;
    const opts = { duration: T.collapseDur, easing: EASE_IN_OUT, fill: "both" };

    live.push(
      field.animate(
        [
          { transform: "none" },
          {
            transform: `translate(${t.left.toFixed(2)}px, ${t.top.toFixed(2)}px) scale(${(t.width / vw).toFixed(5)}, ${(t.height / vh).toFixed(5)})`,
          },
        ],
        opts
      )
    );
    const flip = mono.animate([{ transform: big }, { transform: "none" }], opts);
    live.push(flip);
    introEl.classList.add("is-closing");

    flip.finished.then(landed).catch(() => {});
  }

  /* The mark is in its place. Only now is GUHIT written, so the word never
     shares the screen with the moving field, and the rest of the hero rises
     from this moment, wherever it turned out to be. */
  function landed() {
    if (closed) return;
    root.style.setProperty("--intro-o", `${Math.round(performance.now() - t0)}ms`);
    writeWordmark();
    endIntro();
  }

  function endIntro() {
    if (closed) return;
    closed = true;
    for (const id of timers) clearTimeout(id);
    timers.length = 0;
    for (const a of live) {
      try { a.cancel(); } catch { /* already gone */ }
    }
    live.length = 0;
    if (plate) plate.style.fill = "";
    mono.style.transform = "";
    mono.classList.remove("is-lit", "is-waiting");
    if (status) status.textContent = "";
    root.classList.remove("intro-on");
    root.classList.add("intro-done");
    introEl.remove();
  }

  /* skip: anything the visitor does jumps to the end state */
  function skip() {
    if (closed) return;
    root.style.setProperty("--intro-o", "0ms");
    endIntro();
    if (wordSvg) {
      wordSvg.remove();
      wordSvg = null;
    }
    root.classList.remove("intro-writing");
  }
  for (const ev of ["pointerdown", "keydown", "wheel", "touchstart"]) {
    addEventListener(ev, skip, { once: true, passive: true, capture: true });
  }

  /* ---- 5. the wordmark, drawn then filled, one letter behind the other -- */

  let wordSvg = null;
  /* rough outline length per letter, in ems of the cap height: enough that a
     letter finishes its stroke as its window closes */
  const LEN = { G: 4.6, U: 4.4, H: 5.4, I: 1.8, T: 3.4 };

  function writeWordmark() {
    if (!word) return;
    const cs = getComputedStyle(word);
    const size = parseFloat(cs.fontSize);
    if (!size) return;

    /* The baseline, in the wordmark's own box: an empty inline-block sits on
       it. By this point the fonts are loaded, because the collapse waited for
       them, so the metric is the final one. The probe goes before the word,
       never after it: at the largest size the word fills its line, and a probe
       after it wraps onto a second line, one line-height below the word. */
    const probe = document.createElement("span");
    probe.style.cssText = "display:inline-block;width:0;height:0;overflow:hidden";
    word.insertBefore(probe, word.firstChild);
    const box = word.getBoundingClientRect();
    const baseline = probe.getBoundingClientRect().bottom - box.top;
    probe.remove();

    /* Each letter's left edge in the real heading, so the outline lands on the
       real glyphs instead of re-flowing the word with SVG's own spacing. */
    const xs = [];
    const range = document.createRange();
    for (const node of word.childNodes) {
      if (node.nodeType !== Node.TEXT_NODE) continue;
      const s = node.data;
      for (let k = 0; k < s.length; k++) {
        if (/\s/.test(s[k])) continue;
        range.setStart(node, k);
        range.setEnd(node, k + 1);
        xs.push(range.getBoundingClientRect().left - box.left);
      }
    }

    const ns = "http://www.w3.org/2000/svg";
    const svg = document.createElementNS(ns, "svg");
    svg.setAttribute("class", "write");
    svg.setAttribute("aria-hidden", "true");
    svg.setAttribute("focusable", "false");
    /* No viewBox on purpose. The svg is sized 100% x 100% of the heading's own
       box, so with no viewBox one user unit is one CSS px of that box and
       nothing is ever scaled. A viewBox here would be built from the integer
       clientWidth / clientHeight and preserveAspectRatio would then rescale the
       whole outline by the rounding error, which is the ~1px drift this
       replaces. */

    const text = document.createElementNS(ns, "text");
    text.setAttribute("x", "0");
    text.setAttribute("y", baseline.toFixed(2));
    text.setAttribute("xml:space", "preserve");
    for (const prop of [
      "fontFamily", "fontSize", "fontWeight", "fontStyle", "fontStretch",
      "letterSpacing", "fontVariationSettings", "fontFeatureSettings",
      "fontKerning", "textRendering",
    ]) {
      if (cs[prop]) text.style[prop] = cs[prop];
    }

    const letters = (word.textContent || "").trim().toUpperCase();
    letters.split("").forEach((ch, i) => {
      const ts = document.createElementNS(ns, "tspan");
      ts.textContent = ch;
      if (xs.length === letters.length) ts.setAttribute("x", xs[i].toFixed(2));
      ts.style.setProperty("--i", String(i));
      ts.style.setProperty("--len", `${((LEN[ch] || 4.4) * size).toFixed(0)}`);
      text.appendChild(ts);
    });
    svg.appendChild(text);
    word.appendChild(svg);
    wordSvg = svg;
    root.classList.add("intro-writing");

    /* Not on the intro's timer list: the outline outlives the overlay by half a
       second, and the real heading must be shown again even so. */
    setTimeout(() => {
      if (!wordSvg) return;
      wordSvg.remove();
      wordSvg = null;
      root.classList.remove("intro-writing");
    }, (letters.length - 1) * T.writeStep + T.writeDraw + T.writeFill + 40);
  }
}

/* ---------- 1. scroll-driven enter animations, with a fallback ---------- */

const hasSDA =
  typeof CSS !== "undefined" &&
  CSS.supports &&
  CSS.supports("animation-timeline", "view()");

if (!hasSDA) {
  root.classList.add("no-sda");
  if (!reduced.matches && "IntersectionObserver" in window) {
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) {
            e.target.classList.add("is-in");
            io.unobserve(e.target);
          }
        }
      },
      { rootMargin: "0px 0px -12% 0px", threshold: 0.1 }
    );
    document.querySelectorAll(".rise, .key").forEach((el) => io.observe(el));
  } else {
    document.querySelectorAll(".rise, .key").forEach((el) => el.classList.add("is-in"));
  }
}

/* ---------- 2. pinned act progress + hero parallax + crosshair ---------- */

const acts = Array.from(document.querySelectorAll(".act"));
const heroField = document.getElementById("heroField");
const hero = document.querySelector(".hero");
const crosshair = document.getElementById("crosshair");

let pinning = false;
let frame = 0;
let pointer = null;
let pointerDirty = false;
const boxes = new Map();
let heroH = 0;

function measure() {
  // Document-absolute tops. offsetTop is relative to the nearest positioned
  // ancestor, and <main> is positioned, so it would be short by the hero.
  const y = window.scrollY || window.pageYOffset || 0;
  heroH = hero ? hero.offsetHeight : 0;
  for (const act of acts) {
    const r = act.getBoundingClientRect();
    boxes.set(act, { top: Math.round(r.top + y), h: Math.round(r.height) });
  }
}

function write() {
  frame = 0;
  const y = window.scrollY || window.pageYOffset || 0;
  const vh = window.innerHeight;

  if (pinning) {
    for (const act of acts) {
      const b = boxes.get(act);
      if (!b) continue;
      const span = b.h - vh;
      const p = span > 0 ? clamp01((y - b.top) / span) : 1;
      act.style.setProperty("--p", p.toFixed(4));
    }
    if (heroField && y < heroH) {
      heroField.style.transform = `translate3d(0, ${(y * 0.16).toFixed(1)}px, 0)`;
    }
  }

  if (pointerDirty && pointer && crosshair) {
    pointerDirty = false;
    crosshair.style.transform = `translate3d(${pointer.x}px, ${pointer.y}px, 0)`;
  }
}

function schedule() {
  if (!frame) frame = requestAnimationFrame(write);
}

function setMode() {
  const on = !reduced.matches && !narrow.matches;
  if (on === pinning) return;
  pinning = on;
  if (!on) {
    for (const act of acts) act.style.removeProperty("--p");
    if (heroField) heroField.style.removeProperty("transform");
  } else {
    measure();
    schedule();
  }
}

setMode();
reduced.addEventListener("change", setMode);
narrow.addEventListener("change", setMode);

addEventListener("scroll", schedule, { passive: true });
addEventListener(
  "resize",
  () => {
    if (pinning) measure();
    schedule();
  },
  { passive: true }
);
addEventListener("load", () => {
  if (pinning) measure();
  schedule();
});

/* ---------- 3. crosshair, fine pointers only ---------------------------- */

if (crosshair && fine.matches && !reduced.matches) {
  addEventListener(
    "pointermove",
    (e) => {
      if (e.pointerType !== "mouse") return;
      pointer = { x: e.clientX, y: e.clientY };
      pointerDirty = true;
      crosshair.classList.add("on");
      schedule();
    },
    { passive: true }
  );
  addEventListener("pointerdown", () => crosshair.classList.add("hot"));
  addEventListener("pointerup", () => crosshair.classList.remove("hot"));
  addEventListener("pointerleave", () => crosshair.classList.remove("on"));
  document.addEventListener("pointerover", (e) => {
    const hot = e.target instanceof Element && e.target.closest("a, button, .chip, .key, .cell, .sun__rail");
    crosshair.classList.toggle("hot", Boolean(hot));
  });
}

/* ---------- 4. put the visitor's platform first ------------------------- */

const DL_BASE = "https://github.com/techuila/guhit-studio/releases/latest/download/";
const MAC_ARM = "Guhit-Studio-mac-apple-silicon.dmg";
const MAC_INTEL = "Guhit-Studio-mac-intel.dmg";

function detectOS() {
  const d = navigator.userAgentData;
  const p = (d && d.platform) || navigator.platform || "";
  const ua = navigator.userAgent || "";
  if (/win/i.test(p) || /Windows/i.test(ua)) return "win";
  if (/mac/i.test(p) || /Mac OS X/i.test(ua)) return "mac";
  return null;
}

const os = detectOS();
if (os === "win") {
  document.querySelectorAll(".cta").forEach((cta) => {
    const win = cta.querySelector('[data-os="win"]');
    if (win && win !== cta.firstElementChild) cta.prepend(win);
  });
}

/* Only Chromium tells a page the CPU. Safari and Firefox always say Intel,
   so the main button stays Apple silicon, the common case since late 2020,
   and the Intel build sits one link below it. */
function preferIntelMac() {
  document.querySelectorAll('.btn[data-dl="mac"]').forEach((a) => {
    a.dataset.file = MAC_INTEL;
    a.href = DL_BASE + MAC_INTEL;
  });
  document.querySelectorAll("[data-dl-alt]").forEach((alt) => {
    const a = alt.querySelector("a");
    if (!a) return;
    a.dataset.file = MAC_ARM;
    a.href = DL_BASE + MAC_ARM;
    a.textContent = "Get the Apple silicon build";
    alt.firstChild.textContent = "Mac with Apple silicon? ";
  });
}
if (os === "mac" && navigator.userAgentData && navigator.userAgentData.getHighEntropyValues) {
  navigator.userAgentData
    .getHighEntropyValues(["architecture"])
    .then((v) => {
      if (v && v.architecture === "x86") preferIntelMac();
    })
    .catch(() => {});
}

/* ---------- 5. copy the MCP command ------------------------------------- */

document.querySelectorAll("[data-copy]").forEach((btn) => {
  const label = btn.querySelector(".copy__label");
  const src = document.querySelector(btn.dataset.copy);
  if (!src || !label) return;
  btn.addEventListener("click", async () => {
    const text = src.textContent.trim();
    let ok = false;
    try {
      await navigator.clipboard.writeText(text);
      ok = true;
    } catch {
      const ta = document.createElement("textarea");
      ta.value = text;
      ta.setAttribute("readonly", "");
      ta.style.cssText = "position:fixed;opacity:0;pointer-events:none";
      document.body.appendChild(ta);
      ta.select();
      try {
        ok = document.execCommand("copy");
      } catch {
        ok = false;
      }
      ta.remove();
    }
    label.textContent = ok ? "Copied" : "Press Ctrl+C";
    btn.classList.toggle("done", ok);
    setTimeout(() => {
      label.textContent = "Copy";
      btn.classList.remove("done");
    }, 2200);
  });
});

/* ---------- 6. latest release, with a graceful fallback ----------------- */

const relTag = document.getElementById("relTag");
const relLine = document.getElementById("relLine");
let releaseMissing = false;

/* Releases published before the fixed download names existed only carry the
   versioned bundle names, so each fixed name has a pattern to fall back on. */
const FALLBACK = {
  [MAC_ARM]: /_aarch64\.dmg$/i,
  [MAC_INTEL]: /_x64\.dmg$/i,
  "Guhit-Studio-windows-setup.exe": /-setup\.exe$/i,
};

function applyAssets(release) {
  const assets = Array.isArray(release.assets) ? release.assets : [];
  document.querySelectorAll("a[data-file]").forEach((a) => {
    const want = a.dataset.file;
    let asset = assets.find((x) => x.name === want);
    if (!asset && FALLBACK[want]) {
      asset = assets.find((x) => FALLBACK[want].test(x.name));
      if (asset) a.href = asset.browser_download_url;
    }
    const meta = a.querySelector("[data-size]");
    if (meta) meta.textContent = asset && asset.size ? `${Math.max(1, Math.round(asset.size / 1e6))} MB` : "";
  });
}

/* The list endpoint answers 200 with [] before the first release, so a repo with
   no release yet does not log a 404 in anyone's console. */
fetch("https://api.github.com/repos/techuila/guhit-studio/releases?per_page=5", {
  headers: { Accept: "application/vnd.github+json" },
})
  .then((r) => (r.ok ? r.json() : null))
  .then((list) => {
    if (!Array.isArray(list)) return;
    const d = list.find((r) => r && !r.draft && !r.prerelease && r.tag_name);
    if (!d) {
      releaseMissing = true;
      if (relLine) relLine.textContent = "The first build is on its way.";
      return;
    }
    if (relTag) relTag.textContent = d.tag_name;
    if (relLine) {
      const when = d.published_at ? new Date(d.published_at) : null;
      relLine.textContent = when
        ? `Latest release ${d.tag_name}, ${when.toLocaleDateString("en-GB", {
            day: "numeric",
            month: "short",
            year: "numeric",
          })}.`
        : `Latest release ${d.tag_name}.`;
    }
    applyAssets(d);
  })
  .catch(() => {
    /* offline or rate limited: the fixed download links still work */
  });

/* ---------- 7. after a download starts: how to open it the first time ---- */

function helpFor(link) {
  const scope = link.closest(".hero, .get") || document;
  return scope.querySelector("[data-dl-help]");
}

function showHelp(panel, kind, waiting) {
  const title = panel.querySelector("[data-dl-title]");
  if (title) {
    title.textContent = waiting
      ? "The first build is still being made. Try again in a few minutes."
      : "Your download has started. Then:";
  }
  panel.querySelectorAll("[data-for]").forEach((el) => {
    el.hidden = waiting || el.dataset.for !== kind;
  });
  const opening = panel.hidden;
  panel.hidden = false;
  if (opening && !reduced.matches) {
    panel.animate(
      [
        { opacity: 0, transform: "translateY(10px) scale(0.98)" },
        { opacity: 1, transform: "none" },
      ],
      { duration: 240, easing: "cubic-bezier(0.22, 1, 0.36, 1)" },
    );
  }
  if (title) title.focus({ preventScroll: true });
}

function hideHelp(panel) {
  if (panel.hidden) return;
  if (reduced.matches) {
    panel.hidden = true;
    return;
  }
  const out = panel.animate(
    [
      { opacity: 1, transform: "none" },
      { opacity: 0, transform: "translateY(6px)" },
    ],
    { duration: 160, easing: "cubic-bezier(0.55, 0, 0.8, 0.3)" },
  );
  out.onfinish = () => {
    panel.hidden = true;
  };
}

document.querySelectorAll("a[data-file]").forEach((a) => {
  a.addEventListener("click", (e) => {
    const panel = helpFor(a);
    const kind = a.dataset.dl === "win" ? "win" : "mac";
    if (releaseMissing) {
      e.preventDefault();
      if (panel) showHelp(panel, kind, true);
      return;
    }
    if (panel) showHelp(panel, kind, false);
  });
});

document.querySelectorAll("[data-dl-close]").forEach((btn) => {
  btn.addEventListener("click", () => {
    const panel = btn.closest("[data-dl-help]");
    if (panel) hideHelp(panel);
  });
});

/* ---------- 8. the sun: drag it, step it, or press U and I --------------- */
/* Five frames of the live 3D view, one per sun preset of the app. Dragging
   scrubs between them; letting go settles on the nearest preset. Arrow keys
   and the app's own U and I step one preset. The first time the widget is
   well in view it plays once from morning to night, unless the visitor has
   already touched it or asked for reduced motion. */

const sun = document.getElementById("sun");
const sunRange = document.getElementById("sunRange");
const sunNow = document.getElementById("sunNow");
if (sun && sunRange) {
  const PRESETS = [
    { at: "8:00 AM", name: "morning" },
    { at: "12:00 PM", name: "noon" },
    { at: "3:00 PM", name: "afternoon" },
    { at: "5:59 PM", name: "dusk, lamps on" },
    { at: "8:00 PM", name: "night" },
  ];
  const last = PRESETS.length - 1;
  const ticks = Array.from(sun.querySelectorAll(".sun__ticks li"));
  const timers = [];
  let touched = false;
  let inView = false;

  const show = (v, following) => {
    const i = Math.max(0, Math.min(last, Math.round(v)));
    sun.classList.toggle("is-dragging", following);
    sun.style.setProperty("--sun", v.toFixed(3));
    if (sunNow) sunNow.textContent = PRESETS[i].at;
    sunRange.setAttribute("aria-valuetext", `${PRESETS[i].at}, ${PRESETS[i].name}`);
    ticks.forEach((li, k) => li.classList.toggle("is-on", k === i));
  };
  const settle = (v) => {
    const c = Math.max(0, Math.min(last, Math.round(v)));
    sunRange.value = String(c);
    show(c, false);
  };
  const takeOver = () => {
    touched = true;
    for (const id of timers) clearTimeout(id);
    timers.length = 0;
  };

  sunRange.addEventListener("pointerdown", takeOver);
  sunRange.addEventListener("input", () => {
    takeOver();
    show(+sunRange.value, true);
  });
  sunRange.addEventListener("change", () => settle(+sunRange.value));
  sunRange.addEventListener("keydown", (e) => {
    const dir = { ArrowRight: 1, ArrowUp: 1, PageUp: 1, ArrowLeft: -1, ArrowDown: -1, PageDown: -1 }[e.key];
    if (dir) {
      e.preventDefault();
      takeOver();
      settle(Math.round(+sunRange.value) + dir);
    } else if (e.key === "Home" || e.key === "End") {
      e.preventDefault();
      takeOver();
      settle(e.key === "Home" ? 0 : last);
    }
  });
  addEventListener("keydown", (e) => {
    if (!inView || e.metaKey || e.ctrlKey || e.altKey) return;
    const k = e.key.toLowerCase();
    if (k !== "u" && k !== "i") return;
    const t = e.target;
    if (t instanceof HTMLElement && t !== sunRange && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName))) return;
    takeOver();
    settle(Math.round(+sunRange.value) + (k === "i" ? 1 : -1));
  });

  if ("IntersectionObserver" in window) {
    let played = false;
    new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          inView = e.isIntersecting;
          if (!e.isIntersecting || played || touched || reduced.matches) continue;
          if (e.intersectionRatio < 0.55) continue;
          played = true;
          for (let k = 1; k <= last; k++) {
            timers.push(setTimeout(() => settle(k), 500 + (k - 1) * 1150));
          }
        }
      },
      { threshold: [0, 0.55] }
    ).observe(sun);
  }
  settle(0);
}

/* ---------- 9. together: a live session to watch, and to join ----------- */
/* Ana, Ben and Mika work on one plan the way the app shows a live session
   (DECISIONS D29): each pointer in its own color with a name, cursor chat
   that grows out of the name, every message landing in the Chat panel, a
   wall dragged live with its dimension and the room's area following, and an
   undo that asks before taking back someone else's step. The visitor can
   join: their pointer gets a name, "/" opens cursor chat at it, and someone
   answers. One script on a loop clock that runs only while the window is on
   screen. Under reduced motion one still frame of it stands in, and joining
   still works. */

const live = document.getElementById("live");
const liveCanvas = document.getElementById("liveCanvas");
const livePlan = document.getElementById("livePlan");
const liveLayer = document.getElementById("liveLayer");
if (live && liveCanvas && livePlan && liveLayer) {
  const LOOP = 26000;
  const PER_CHAR = 90; // ms a typed character takes
  const SENT_MS = 4000; // the app keeps your own sent message in the bubble this long
  const REPLY_MS = 3200;
  // The plan's viewBox, in mm, and a tighter one on phones so it reads bigger
  // there. The canvas's aspect-ratio in styles.css follows each.
  const WIDE = { x: -1500, y: -1300, w: 11800, h: 9000 };
  const SMALL = { x: -500, y: -1500, w: 9600, h: 8800 };
  const small = matchMedia("(max-width: 560px)");
  let VB = small.matches ? SMALL : WIDE;
  const applyViewBox = () => {
    VB = small.matches ? SMALL : WIDE;
    livePlan.setAttribute("viewBox", `${VB.x} ${VB.y} ${VB.w} ${VB.h}`);
  };
  const motion = () => !reduced.matches;

  const CAST = {
    ana: { name: "Ana", peer: 0 },
    ben: { name: "Ben", peer: 1 },
    mika: { name: "Mika", peer: 2 },
  };
  const YOU = { name: "You", peer: 3 };

  /* Where each pointer is at moments of the loop, in plan mm as drawn (y
     down). "btn" is the Undo anyway button of the question Ben answers. */
  const PATHS = {
    ana: [[0, 2100, 3900], [2000, 2600, 3300], [4600, 3000, 2500], [7400, 8000, 2300], [7800, 8000, 2300],
      [9800, 8600, 2300], [10400, 8540, 2420], [13000, 8380, 2760], [16500, 7600, 1800], [19000, 7400, 1700],
      [23000, 4300, 2900], [26000, 2100, 3900]],
    ben: [[0, 6300, 1300], [2000, 6420, 1560], [3300, 6500, 3000], [6000, 6540, 3040], [9000, 6300, 3400],
      [14000, 5900, 3800], [17000, 6100, 3600], [17800, 6100, 3600], [18500, "btn"], [19400, "btn"],
      [23000, 6600, 1600], [26000, 6300, 1300]],
    mika: [[0, 1200, 5200], [3000, 1700, 4600], [8000, 2400, 4200], [12000, 3600, 4600], [13600, 6900, 4300],
      [15800, 6920, 4320], [17600, 5300, 4500], [19200, 4300, 5000], [23000, 3000, 5000], [26000, 1200, 5200]],
  };
  const PHASE = { ana: 0.4, ben: 2.1, mika: 4.3 };
  // Each line has a second wording for the next loop, so the Chat panel does
  // not just repeat itself.
  const SAYS = [
    { who: "ben", at: 3600, texts: ["Can this room be wider?", "Lakihan pa natin?"], hold: 3000 },
    { who: "ana", at: 10400, texts: ["Ayan, 600 mm wider.", "Done, plus 600."], hold: 2900 },
    { who: "mika", at: 13800, texts: ["Mas maganda dati.", "Hmm, too big now."], hold: 2200 },
    { who: "ana", at: 20000, texts: ["Sige, balik.", "Okay, back to 8000."], hold: 2800 },
  ];
  const lineOf = (s, k) => {
    const text = s.texts[k % s.texts.length];
    const sent = s.at + text.length * PER_CHAR + 250;
    return { text, sent, until: sent + s.hold };
  };
  const PRESSES = [{ who: "ben", at: 3300 }, { who: "ana", at: 7800 }, { who: "ben", at: 19300 }];
  const SELECT = [3300, 11000]; // Ben's selection of the bedroom
  const HOLD = [7800, 9800]; // Ana drags the east wall
  const ASK = [17800, 19400]; // Ben's undo would take back Ana's step: his app asks him
  const ASK_AT = [6100, 3600]; // where Ben waits for it
  const UNDO_AT = 19400;
  const MIKA_JOINS = 900; // once, in the first loop: her avatar and her pointer arrive
  const REPLIES = [
    { who: "ana", text: "Hi! Welcome to the session." },
    { who: "ben", text: "Uy, may bagong kasama!" },
    { who: "mika", text: "Kita ko na ang cursor mo." },
  ];

  /* ---- plan mm to canvas px and back (the plan is drawn with "meet") ---- */
  let box = { w: 0, h: 0, s: 1, ox: 0, oy: 0 };
  const layout = () => {
    const r = liveCanvas.getBoundingClientRect();
    const s = Math.min(r.width / VB.w, r.height / VB.h) || 1;
    box = { w: r.width, h: r.height, s, ox: (r.width - VB.w * s) / 2, oy: (r.height - VB.h * s) / 2 };
  };
  const toPx = (x, y) => [box.ox + (x - VB.x) * box.s, box.oy + (y - VB.y) * box.s];
  const toMm = (px, py) => [(px - box.ox) / box.s + VB.x, (py - box.oy) / box.s + VB.y];

  /* ---- the plan's moving parts ---- */
  const part = (k) => livePlan.querySelector(`[data-w="${k}"]`);
  const north = part("north"), south = part("south"), east = part("east"), bedFill = part("bedFill");
  const sel = part("sel"), dimLine = part("dimLine"), dimTicks = part("dimTicks"), dimText = part("dimText");
  const bedName = part("bedName"), bedArea = part("bedArea");
  let shownOff = -1;
  const paintWall = (off) => {
    off = Math.round(off * 10) / 10;
    if (off === shownOff) return;
    shownOff = off;
    const snap = Math.round(off / 10) * 10; // the numbers move in 10 mm steps, as the app's do
    north.setAttribute("width", 8150 + off);
    south.setAttribute("width", 8150 + off);
    east.setAttribute("x", 7925 + off);
    bedFill.setAttribute("width", 2875 + off);
    sel.setAttribute("width", 2875 + off);
    dimLine.setAttribute("d", `M-75 6150 V7000 M${8075 + off} 6150 V7000 M-75 6850 H${8075 + off}`);
    dimTicks.setAttribute("d", `M-215 7000 L65 6700 M${7935 + off} 7000 L${8215 + off} 6700`);
    dimText.setAttribute("x", 4000 + off / 2);
    dimText.textContent = String(8000 + snap);
    bedName.setAttribute("x", 6487 + off / 2);
    bedArea.setAttribute("x", 6487 + off / 2);
    bedArea.textContent = `${(((2875 + snap) * 5850) / 1e6).toFixed(2)} m²`;
  };

  /* ---- cursors, built like the app's: an arrow and a name ---- */
  const ARROW =
    '<svg class="lc__arrow" width="18" height="20" viewBox="0 0 18 20" aria-hidden="true">' +
    '<path d="M2 1.8v14.1l3.9-3.6 2.6 5.8 2.7-1.2-2.6-5.7 5.3-.3z"/></svg>';
  const makeCursor = (who, arrow) => {
    const el = document.createElement("div");
    el.className = "lc";
    el.style.setProperty("--peer", `var(--peer-${who.peer})`);
    el.innerHTML = `<div class="lc__body">${arrow ? ARROW : ""}<div class="lc__label">` +
      '<span class="lc__name"></span><span class="lc__text"></span></div><i class="lc__ring"></i></div>';
    el.querySelector(".lc__name").textContent = who.name;
    liveLayer.append(el);
    return { el, label: el.querySelector(".lc__label"), text: el.querySelector(".lc__text"),
      ring: el.querySelector(".lc__ring"), key: "", x: NaN, y: NaN, flip: null, lw: 0 };
  };
  const cursors = { ana: makeCursor(CAST.ana, true), ben: makeCursor(CAST.ben, true), mika: makeCursor(CAST.mika, true) };
  const you = makeCursor(YOU, false);
  you.el.classList.add("lc--you", "is-gone");

  // The label sits right of the tip, or left of it when only that side has room.
  const sideFor = (c) => {
    const w = c.lw || 60;
    const flip = c.x + 14 + w > box.w - 4 && c.x - 10 - w > 4;
    if (flip !== c.flip) {
      c.flip = flip;
      c.el.dataset.flip = String(flip);
    }
  };
  const placePx = (c, px, py) => {
    if (Math.abs(px - c.x) < 0.05 && Math.abs(py - c.y) < 0.05) return;
    c.x = px;
    c.y = py;
    c.el.style.transform = `translate3d(${px.toFixed(1)}px, ${py.toFixed(1)}px, 0)`;
    sideFor(c);
  };
  const paintLabel = (c, state) => {
    const key = state ? `${state.mode}|${state.text}` : "";
    if (key === c.key) return;
    const wasBubble = c.key !== "";
    c.key = key;
    c.el.classList.toggle("is-bubble", Boolean(state));
    c.el.classList.toggle("is-typing", Boolean(state) && state.mode === "typing");
    c.text.textContent = state ? state.text : "";
    c.lw = c.label.offsetWidth; // measured only when the words change
    if (!Number.isNaN(c.x)) sideFor(c);
    // the name grows into a bubble, as in the app
    if (state && !wasBubble && motion()) {
      c.label.animate([{ transform: "scale(0.9)", opacity: 0.6 }, { transform: "none", opacity: 1 }],
        { duration: 180, easing: EASE_OUT });
    }
  };
  const ring = (c) => {
    if (!motion()) return;
    c.ring.animate([{ opacity: 0.9, transform: "scale(0.3)" }, { opacity: 0, transform: "scale(1.4)" }],
      { duration: 520, easing: EASE_OUT });
  };

  /* ---- the undo question at Ben's pointer ---- */
  const ask = document.createElement("div");
  ask.className = "live__ask";
  ask.innerHTML = '<p class="live__ask-who num">On Ben\'s screen</p><p class="live__ask-title">Undo someone else\'s change?</p>' +
    '<p class="live__ask-text">Ana made the last change: Move. Undo it anyway?</p>' +
    '<div class="live__ask-row"><span>Cancel</span><span class="is-primary">Undo anyway</span></div>';
  liveLayer.append(ask);
  const askBtn = ask.querySelector(".is-primary");
  let btnPx = [0, 0];
  let askShown = false;
  const placeAsk = () => {
    const [ax, ay] = toPx(ASK_AT[0], ASK_AT[1]);
    const w = ask.offsetWidth || 250;
    const h = ask.offsetHeight || 110;
    let x = ax + 18;
    let y = ay + 24;
    if (x + w > box.w - 8) x = ax - w - 14;
    if (y + h > box.h - 8) y = ay - h - 10;
    ask.style.left = `${x.toFixed(1)}px`;
    ask.style.top = `${y.toFixed(1)}px`;
    ask.style.transformOrigin = `${x < ax ? "top right" : "top left"}`;
    // Ben's pointer lands on the button's middle, wherever the layout put it.
    // Measured once here, not every frame.
    btnPx = [x + askBtn.offsetLeft + askBtn.offsetWidth / 2, y + askBtn.offsetTop + askBtn.offsetHeight / 2];
  };
  const btnMm = () => toMm(btnPx[0], btnPx[1]);

  /* ---- where everyone is, and what their name says ---- */
  const easeInOut = (p) => (p < 0.5 ? 4 * p * p * p : 1 - Math.pow(-2 * p + 2, 3) / 2);
  const easeOut = (p) => 1 - Math.pow(1 - p, 3);
  const keyPoint = (k) => (k[1] === "btn" ? btnMm() : [k[1], k[2]]);
  const raw = (who, t) => {
    const path = PATHS[who];
    let i = 0;
    while (i < path.length - 2 && t >= path[i + 1][0]) i++;
    const a = path[i];
    const b = path[i + 1];
    const e = easeInOut(clamp01((t - a[0]) / (b[0] - a[0])));
    const A = keyPoint(a);
    const B = keyPoint(b);
    return { x: A[0] + (B[0] - A[0]) * e, y: A[1] + (B[1] - A[1]) * e, A, B, e, i };
  };
  const dragging = (who, t) => who === "ana" && t >= HOLD[0] && t < HOLD[1];
  const where = (who, t, abs) => {
    const r = raw(who, t);
    if (dragging(who, t)) return [r.x, r.y]; // the wall follows the pointer exactly
    let { x, y } = r;
    const dx = r.B[0] - r.A[0];
    const dy = r.B[1] - r.A[1];
    const d = Math.hypot(dx, dy);
    if (d > 1) {
      // a hand moves in an arc, not along a ruler
      const bow = d * 0.09 * Math.sin(Math.PI * r.e) * (r.i % 2 ? 1 : -1);
      x += (-dy / d) * bow;
      y += (dx / d) * bow;
    }
    // and never holds perfectly still
    const amp = d > 1 ? 10 : 30;
    x += amp * Math.sin(abs / 820 + PHASE[who]);
    y += amp * 0.8 * Math.sin(abs / 1070 + PHASE[who] * 1.7);
    return [x, y];
  };
  const wallAt = (t) => {
    if (t < HOLD[0]) return 0;
    if (t < HOLD[1]) return Math.max(0, Math.min(600, raw("ana", t).x - 8000));
    if (t < UNDO_AT) return 600;
    if (t < UNDO_AT + 360) return 600 * (1 - easeOut((t - UNDO_AT) / 360));
    return 0;
  };
  const replies = [];
  const sayState = (who, t, abs) => {
    for (const o of replies) {
      if (o.who !== who || abs < o.start || abs >= o.until) continue;
      if (abs < o.sent) {
        const n = Math.min(o.text.length, Math.floor((abs - o.start) / PER_CHAR) + 1);
        return { mode: "typing", text: o.text.slice(0, n) };
      }
      return { mode: "said", text: o.text };
    }
    const k = Math.floor(abs / LOOP);
    for (const s of SAYS) {
      const line = lineOf(s, k);
      if (s.who !== who || t < s.at || t >= line.until) continue;
      if (t < line.sent) {
        const n = Math.min(line.text.length, Math.floor((t - s.at) / PER_CHAR) + 1);
        return { mode: "typing", text: line.text.slice(0, n) };
      }
      return { mode: "said", text: line.text };
    }
    return null;
  };

  /* ---- the Chat panel, the avatars and the people count ---- */
  const log = document.getElementById("liveLog");
  const count = document.getElementById("liveCount");
  const peopleCount = document.getElementById("livePeopleCount");
  const avMika = live.querySelector('.live__av[data-who="mika"]');
  const avYou = live.querySelector('.live__av[data-who="you"]');
  let messages = log ? log.children.length : 0;
  let people = 3;
  const clockLabel = (d) => d.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
  if (log) {
    for (const el of log.querySelectorAll("[data-ago]")) {
      el.textContent = clockLabel(new Date(Date.now() - Number(el.dataset.ago) * 60000));
    }
  }
  const AT_ICON =
    '<svg class="msg__at" width="9" height="10" viewBox="0 0 18 20" aria-hidden="true"><title>Sent at a spot on the plan</title>' +
    '<path d="M2 1.8v14.1l3.9-3.6 2.6 5.8 2.7-1.2-2.6-5.7 5.3-.3z" fill="currentColor"/></svg>';
  const addMessage = (who, text) => {
    if (!log) return;
    const li = document.createElement("li");
    li.className = motion() ? "msg is-new" : "msg";
    li.style.setProperty("--peer", `var(--peer-${who.peer})`);
    li.innerHTML = '<i class="msg__av"></i><div><p class="msg__meta"><b></b> <span class="num"></span> ' + AT_ICON +
      '</p><p class="msg__text"></p></div>';
    li.querySelector(".msg__av").textContent = who.name[0];
    li.querySelector("b").textContent = who.name;
    li.querySelector(".num").textContent = clockLabel(new Date());
    li.querySelector(".msg__text").textContent = text;
    log.append(li);
    while (log.children.length > 12) log.firstElementChild.remove();
    messages += 1;
    if (count) count.textContent = String(messages);
  };
  const setPeople = (n) => {
    people = n;
    if (peopleCount) peopleCount.textContent = String(n);
  };
  const popIn = (av) => {
    if (!av) return;
    av.hidden = false;
    if (motion()) av.classList.add("is-in");
  };

  /* ---- one frame of the loop ---- */
  let mikaIn = true;
  let prevAbs = -1;
  // did a moment of the loop happen in (from, to] of the absolute clock
  const crossed = (at, from, to) => {
    const k = Math.floor((to - at) / LOOP);
    return k >= 0 && k * LOOP + at > from;
  };
  const render = (abs) => {
    if (!box.w) layout();
    const t = abs % LOOP;
    const arrived = abs >= MIKA_JOINS;
    if (arrived !== mikaIn) {
      mikaIn = arrived;
      cursors.mika.el.hidden = !arrived;
      if (arrived) {
        if (motion()) cursors.mika.el.classList.add("is-in");
        popIn(avMika);
        setPeople(people + 1);
      }
    }
    paintWall(wallAt(t));
    sel.classList.toggle("is-on", t >= SELECT[0] && t < SELECT[1]);
    east.classList.toggle("is-held", t >= HOLD[0] && t < HOLD[1]);
    const askOn = t >= ASK[0] && t < ASK[1];
    if (askOn !== askShown) {
      askShown = askOn;
      if (askOn) placeAsk();
      ask.classList.toggle("is-on", askOn);
    }
    askBtn.classList.toggle("is-pressed", t >= 19300 && t < 19460);
    for (const who of ["ana", "ben", "mika"]) {
      if (who === "mika" && !mikaIn) continue;
      const c = cursors[who];
      paintLabel(c, sayState(who, t, abs));
      const [x, y] = where(who, t, abs);
      const [px, py] = toPx(x, y);
      placePx(c, px, py);
    }
    if (prevAbs >= 0) {
      for (const p of PRESSES) if (crossed(p.at, prevAbs, abs)) ring(cursors[p.who]);
      const kNow = Math.floor(abs / LOOP);
      for (const s of SAYS) {
        for (const k of [kNow - 1, kNow]) {
          if (k < 0) continue;
          const line = lineOf(s, k);
          const when = k * LOOP + line.sent;
          if (when > prevAbs && when <= abs) addMessage(CAST[s.who], line.text);
        }
      }
      if (crossed(UNDO_AT, prevAbs, abs) && motion()) {
        // what an undo changes flashes once, as in the app
        east.animate([{ fill: "#0e8a8f" }, { fill: "#1f2d44" }], { duration: 700, easing: EASE_OUT });
      }
    }
    for (const o of replies) {
      if (!o.logged && abs >= o.sent) {
        o.logged = true;
        addMessage(CAST[o.who], o.text);
      }
    }
    while (replies.length && replies[0].until < abs) replies.shift();
    prevAbs = abs;
  };

  /* ---- the clock: runs only while the window is on screen ---- */
  let clock = 0;
  let last = 0;
  let raf = 0;
  let running = false;
  let onScreen = false;
  const frame = (now) => {
    raf = 0;
    if (!running) return;
    clock += last ? Math.min(64, now - last) : 16;
    last = now;
    render(clock);
    raf = requestAnimationFrame(frame);
  };
  const play = () => {
    if (running || !motion() || !onScreen || document.hidden) return;
    running = true;
    last = 0;
    raf = requestAnimationFrame(frame);
  };
  const pause = () => {
    running = false;
    if (raf) cancelAnimationFrame(raf);
    raf = 0;
  };
  // under reduced motion: Ben has just asked, everyone is in, nothing moves
  let stillShown = false;
  const still = () => {
    clock = LOOP + 6400;
    prevAbs = -1;
    render(clock);
    if (!stillShown) {
      stillShown = true;
      addMessage(CAST.ben, SAYS[0].texts[0]);
    }
  };

  /* ---- the visitor joins in ---- */
  const pos = document.getElementById("livePos");
  const fmt = (n) => (Math.round(n / 10) * 10).toLocaleString("en-US");
  let hovering = false;
  let pointerPx = null;
  let joined = false;
  const showPos = (p) => {
    if (!pos) return;
    if (!p) {
      pos.textContent = "Ground floor · 1:100";
      return;
    }
    const [x, y] = toMm(p[0], p[1]);
    pos.textContent = `X ${fmt(x)}  Y ${fmt(6000 - y)} mm`;
  };
  const join = () => {
    if (joined) return;
    joined = true;
    popIn(avYou);
    setPeople(people + 1);
  };

  const chat = document.createElement("form");
  chat.className = "you-chat";
  chat.hidden = true;
  chat.style.setProperty("--peer", "var(--peer-3)");
  chat.innerHTML = '<div class="you-chat__bubble"><span class="you-chat__name">You</span>' +
    '<input class="you-chat__input" maxlength="160" autocomplete="off" spellcheck="false" enterkeyhint="send"' +
    ' placeholder="Say something" aria-label="Your message, shown at your pointer">' +
    '<span class="you-chat__text" hidden></span></div>';
  liveCanvas.append(chat);
  const chatInput = chat.querySelector(".you-chat__input");
  const chatText = chat.querySelector(".you-chat__text");
  let chatMode = "closed"; // closed, typing, sent
  let chatTimer = 0;
  let chatPx = [0, 0];
  const chatBubble = chat.querySelector(".you-chat__bubble");
  // Like a cursor's label: right of the point, or left of it when only that
  // side has room; above it near the bottom edge.
  const placeChat = (p) => {
    chatPx = p;
    chat.style.transform = `translate3d(${p[0].toFixed(1)}px, ${p[1].toFixed(1)}px, 0)`;
    const w = chatBubble.offsetWidth || 200;
    chat.dataset.flip = String(p[0] + 14 + w > box.w - 4 && p[0] - 6 - w > 4);
    chat.dataset.flipY = String(p[1] > box.h - 90);
  };
  // where the bubble opens without a pointer: open floor in the living room,
  // far enough left for the bubble to fit on a phone
  const restPx = () => (small.matches ? toPx(500, 1300) : toPx(2300, 1500));
  const closeChat = () => {
    if (chatMode === "closed") return;
    clearTimeout(chatTimer);
    chatMode = "closed";
    const hide = () => {
      if (chatMode !== "closed") return;
      chat.hidden = true;
      if (hovering) you.el.classList.remove("is-gone");
    };
    if (motion()) {
      chat.dataset.stage = "exit";
      chatTimer = setTimeout(hide, 130);
    } else {
      hide();
    }
    if (chat.contains(document.activeElement)) chatInput.blur();
  };
  const openChat = () => {
    if (!box.w) layout();
    clearTimeout(chatTimer);
    join();
    chatMode = "typing";
    chatText.hidden = true;
    chatText.textContent = "";
    chatInput.hidden = false;
    chatInput.value = "";
    chat.hidden = false;
    placeChat(hovering && pointerPx ? pointerPx : restPx());
    you.el.classList.add("is-gone");
    if (motion()) {
      chat.dataset.stage = "enter";
      requestAnimationFrame(() => requestAnimationFrame(() => {
        if (chatMode === "typing") chat.dataset.stage = "idle";
      }));
    } else {
      chat.dataset.stage = "idle";
    }
    chatInput.focus({ preventScroll: true });
  };
  let replyCount = 0;
  const reply = () => {
    const r = REPLIES[replyCount++ % REPLIES.length];
    if (running) {
      const start = clock + 900;
      const sent = start + r.text.length * PER_CHAR + 250;
      replies.push({ who: r.who, text: r.text, start, sent, until: sent + REPLY_MS, logged: false });
      return;
    }
    // no clock to ride on: answer at once, and let it go after a moment
    setTimeout(() => {
      paintLabel(cursors[r.who], { mode: "said", text: r.text });
      addMessage(CAST[r.who], r.text);
      setTimeout(() => paintLabel(cursors[r.who], null), REPLY_MS);
    }, 900);
  };
  chat.addEventListener("submit", (e) => {
    e.preventDefault();
    if (chatMode !== "typing") return;
    const text = chatInput.value.replace(/\s+/g, " ").trim().slice(0, 160);
    if (!text) {
      closeChat();
      return;
    }
    // the bubble keeps what you sent a moment, then fades, as in the app
    chatMode = "sent";
    chatText.textContent = text;
    chatText.hidden = false;
    chatInput.hidden = true;
    placeChat(chatPx);
    chatInput.blur();
    addMessage(YOU, text);
    reply();
    clearTimeout(chatTimer);
    chatTimer = setTimeout(closeChat, SENT_MS);
  });
  chatInput.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      closeChat();
    }
  });
  chatInput.addEventListener("input", () => placeChat(chatPx)); // the bubble grows with the words
  chatInput.addEventListener("blur", () => {
    if (chatMode === "typing" && chatInput.value.trim() === "") closeChat();
  });

  liveCanvas.addEventListener("pointermove", (e) => {
    if (e.pointerType === "touch") return;
    if (!box.w) layout();
    const r = liveCanvas.getBoundingClientRect();
    pointerPx = [e.clientX - r.left, e.clientY - r.top];
    if (!hovering) {
      hovering = true;
      if (crosshair) crosshair.classList.add("away");
    }
    join();
    if (chatMode === "closed") you.el.classList.remove("is-gone");
    placePx(you, pointerPx[0], pointerPx[1]);
    if (chatMode !== "closed") placeChat(pointerPx);
    showPos(pointerPx);
  });
  liveCanvas.addEventListener("pointerleave", () => {
    hovering = false;
    you.el.classList.add("is-gone");
    if (crosshair) crosshair.classList.remove("away");
    showPos(null);
  });
  const tryBtn = document.getElementById("liveTry");
  if (tryBtn) {
    tryBtn.addEventListener("pointerdown", (e) => e.preventDefault()); // keep the focus for the field
    tryBtn.addEventListener("click", openChat);
  }
  addEventListener("keydown", (e) => {
    if (e.key !== "/" || e.metaKey || e.ctrlKey || e.altKey) return;
    const target = e.target;
    if (target instanceof HTMLElement && (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName))) return;
    if (!hovering && !live.contains(document.activeElement)) return;
    e.preventDefault();
    openChat();
  });

  /* ---- start ---- */
  applyViewBox();
  layout();
  small.addEventListener("change", () => {
    applyViewBox();
    layout();
    for (const c of Object.values(cursors)) c.x = NaN;
    if (askShown) placeAsk();
    if (!running) render(clock);
  });
  if ("ResizeObserver" in window) {
    new ResizeObserver(() => {
      layout();
      you.x = NaN;
      for (const c of Object.values(cursors)) c.x = NaN;
      if (askShown) placeAsk();
      if (!running) render(clock);
    }).observe(liveCanvas);
  }
  if (motion()) {
    // the first loop opens with two people in; Mika arrives a moment later
    mikaIn = false;
    cursors.mika.el.hidden = true;
    if (avMika) avMika.hidden = true;
    setPeople(2);
    render(0);
  } else {
    still();
  }
  if ("IntersectionObserver" in window) {
    new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          onScreen = e.isIntersecting;
          if (onScreen) play();
          else pause();
        }
      },
      { threshold: 0.12 }
    ).observe(live);
  } else {
    onScreen = true;
    play();
  }
  document.addEventListener("visibilitychange", () => (document.hidden ? pause() : play()));
  reduced.addEventListener("change", () => {
    if (reduced.matches) {
      pause();
      still();
    } else {
      play();
    }
  });
}
