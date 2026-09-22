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
     0.0 - 2.4s   the drafting grid draws, one line at a time, centre outward
     2.4 - 2.7s   hold
     2.7 - 4.7s   the mark draws stroke by stroke, the teal arc last
     4.7 - 5.1s   hold on the finished G
     5.1 - 6.0s   the navy field collapses into the hero's own mark
     5.1 - 6.5s   GUHIT is outlined letter by letter, each fill trailing behind
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
  gridEnd: 2400,   // the last grid line lands here
  lineDur: 500,    // how long one grid line takes to draw
  markAt: 2700,    // the first stroke of the mark
  markDur: 2000,   // through to the end of the teal arc, at 4700
  collapseAt: 5100,
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

function introSeen() {
  try {
    sessionStorage.setItem("guhit:intro", "done");
  } catch {
    /* private mode, or storage blocked: the intro simply plays again */
  }
}

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
    introSeen();
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

  /* ---- 1. the grid, one line at a time, centre outward --------------- */

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

    /* Ordering rule: every line is ranked by its distance from the centre of
       the viewport, nearest first, so the centre square closes before anything
       reaches an edge. The two ranked lists are then interleaved in proportion
       to their length, which reads as vertical, horizontal, vertical... and
       makes both axes reach their edges at the same moment. */
    const cx = vw / 2, cy = vh / 2;
    const vsort = xs.map((x) => ({ v: true, p: x })).sort((a, b) => Math.abs(a.p - cx) - Math.abs(b.p - cx) || a.p - b.p);
    const hsort = ys.map((y) => ({ v: false, p: y })).sort((a, b) => Math.abs(a.p - cy) - Math.abs(b.p - cy) || b.p - a.p);

    const order = [];
    let i = 0, j = 0;
    while (i < vsort.length || j < hsort.length) {
      const fv = i / (vsort.length || 1);
      const fh = j / (hsort.length || 1);
      if (j >= hsort.length || (i < vsort.length && fv <= fh)) order.push(vsort[i++]);
      else order.push(hsort[j++]);
    }

    const n = order.length;
    const stagger = n > 1 ? (T.gridEnd - T.lineDur) / (n - 1) : 0;
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
      live.push(
        el.animate(
          [{ strokeDashoffset: len }, { strokeDashoffset: 0 }],
          { duration: T.lineDur, delay: k * stagger, easing: EASE_OUT, fill: "both" }
        )
      );
    });
    grid.appendChild(frag);
    return { lines: n, verticals: vsort.length, horizontals: hsort.length, stagger };
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
    /* the rest of the hero rises from here, wherever "here" turned out to be */
    root.style.setProperty("--intro-o", `${Math.round(performance.now() - t0)}ms`);
    writeWordmark();

    flip.finished.then(endIntro).catch(() => {});
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
    introSeen();
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
       them, so the metric is the final one. */
    const probe = document.createElement("span");
    probe.style.cssText = "display:inline-block;width:0;height:0;overflow:hidden";
    word.appendChild(probe);
    const baseline = probe.getBoundingClientRect().bottom - word.getBoundingClientRect().top;
    probe.remove();

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
    const hot = e.target instanceof Element && e.target.closest("a, button, .fmt, .key, .cell");
    crosshair.classList.toggle("hot", Boolean(hot));
  });
}

/* ---------- 4. put the visitor's platform first ------------------------- */

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

/* The list endpoint answers 200 with [] before the first release, so a repo with
   no release yet does not log a 404 in anyone's console. */
fetch("https://api.github.com/repos/techuila/guhit-studio/releases?per_page=5", {
  headers: { Accept: "application/vnd.github+json" },
})
  .then((r) => (r.ok ? r.json() : null))
  .then((list) => {
    const d = Array.isArray(list) ? list.find((r) => r && !r.draft && r.tag_name) : null;
    if (!d) return;
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
  })
  .catch(() => {
    /* offline, rate limited, or no release yet: the page keeps its own words */
  });
