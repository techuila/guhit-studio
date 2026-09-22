/* GUHIT Studio site. No dependencies.
   One passive scroll listener, one rAF, all reads batched before all writes.
   Everything degrades: with JS off the acts render in their end state, because
   the `--p` progress variable falls back to 1 in the stylesheet. */

const root = document.documentElement;
const reduced = matchMedia("(prefers-reduced-motion: reduce)");
const narrow = matchMedia("(max-width: 860px)");
const fine = matchMedia("(hover: hover) and (pointer: fine)");
const clamp01 = (n) => (n < 0 ? 0 : n > 1 ? 1 : n);

/* ---------- 0. the load sequence ---------------------------------------- */
/* The class is already on <html> from the inline head script, so the first
   paint is navy. Here: place the mark, collapse the field into the hero mark
   with one FLIP transform, write the wordmark, then get out of the way.
   Any click, key or wheel jumps to the end state; the end state is the hero
   exactly as it renders without the intro. */

const introEl = document.getElementById("intro");
const EASE_IN_OUT = "cubic-bezier(0.65, 0, 0.35, 1)";
const COLLAPSE = 700;

function introSeen() {
  try {
    sessionStorage.setItem("guhit:intro", "done");
  } catch {
    /* private mode, or storage blocked: the intro simply plays again */
  }
}

if (introEl && root.classList.contains("intro-reduced")) {
  introSeen();
  const fade = introEl.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 200, easing: "linear" });
  fade.finished.then(() => introEl.remove()).catch(() => introEl.remove());
} else if (introEl && root.classList.contains("intro-on")) {
  runIntro();
} else if (introEl) {
  /* skipped before the first paint: take the empty overlay out of the page */
  introEl.remove();
}

function runIntro() {
  const mono = document.querySelector(".hero__mark .monogram");
  const field = document.getElementById("introField");
  const h1 = document.querySelector(".hero__h1");
  const word = document.querySelector(".wordmark");
  const arc = document.querySelector(".monogram > .mg");
  if (!mono || !field) {
    root.classList.remove("intro-on");
    introEl.remove();
    return;
  }

  /* line the intro grid up with the hero's own 44px field */
  const hf = document.getElementById("heroField");
  if (hf) {
    const r = hf.getBoundingClientRect();
    const mod = (n, m) => ((n % m) + m) % m;
    introEl.style.setProperty("--gx", `${mod(r.left, 44).toFixed(2)}px`);
    introEl.style.setProperty("--gy", `${mod(r.top, 44).toFixed(2)}px`);
  }

  /* the mark's own navy field would punch a hole in the grid while it is the
     size of the screen, so it only fills once the collapse starts */
  const plate = mono.querySelector("rect");
  if (plate) plate.style.fill = "transparent";

  /* the mark, large and centred: a transform, so the hero never reflows */
  let big = "none";
  function placeMark() {
    mono.style.transform = "";
    const r = mono.getBoundingClientRect();
    const vw = innerWidth, vh = innerHeight;
    const s = (Math.min(vw, vh) * 0.4) / r.width;
    const dx = vw / 2 - (r.left + r.width / 2);
    const dy = vh / 2 - (r.top + r.height / 2);
    big = `translate(${dx.toFixed(2)}px, ${dy.toFixed(2)}px) scale(${s.toFixed(4)})`;
    mono.style.transform = big;
    return r;
  }
  placeMark();
  mono.classList.add("is-lit");

  let closed = false;
  const live = [];

  function collapse() {
    if (closed) return;
    /* measure the real target now: the mark's own place in the hero */
    if (plate) plate.style.fill = "";
    mono.style.transform = "";
    const t = mono.getBoundingClientRect();
    const vw = innerWidth, vh = innerHeight;
    const opts = { duration: COLLAPSE, easing: EASE_IN_OUT, fill: "both" };

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
    writeWordmark();

    flip.finished.then(endIntro).catch(() => {});
  }

  /* the arc is the last stroke of the mark: when it lands, the field goes */
  if (arc) arc.addEventListener("animationend", collapse, { once: true });
  const guard = setTimeout(collapse, 2600);

  function endIntro() {
    if (closed) return;
    closed = true;
    clearTimeout(guard);
    for (const a of live) {
      try { a.cancel(); } catch { /* already gone */ }
    }
    live.length = 0;
    if (plate) plate.style.fill = "";
    mono.style.transform = "";
    mono.classList.remove("is-lit");
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

  /* ---- the wordmark, drawn then filled, one letter behind the other ---- */

  let wordSvg = null;
  /* rough outline length per letter, in ems of the cap height: enough that a
     letter finishes its stroke as its window closes */
  const LEN = { G: 4.6, U: 4.4, H: 5.4, I: 1.8, T: 3.4 };

  function writeWordmark() {
    if (!word || !h1) return;
    const cs = getComputedStyle(word);
    const size = parseFloat(cs.fontSize);
    if (!size) return;

    /* the baseline: an empty inline-block sits on it */
    const probe = document.createElement("span");
    probe.style.cssText = "display:inline-block;width:0;height:0;overflow:hidden";
    word.appendChild(probe);
    const pr = probe.getBoundingClientRect();
    const wr = word.getBoundingClientRect();
    const hr = h1.getBoundingClientRect();
    probe.remove();
    const baseline = pr.bottom;

    const pad = 12;
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("class", "write");
    svg.setAttribute("aria-hidden", "true");
    svg.style.left = `${wr.left - hr.left - pad}px`;
    svg.style.top = `${wr.top - hr.top - pad}px`;
    svg.style.width = `${wr.width + pad * 2}px`;
    svg.style.height = `${wr.height + pad * 2}px`;

    const text = document.createElementNS("http://www.w3.org/2000/svg", "text");
    text.setAttribute("x", String(pad));
    text.setAttribute("y", String(baseline - wr.top + pad));
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
      const ts = document.createElementNS("http://www.w3.org/2000/svg", "tspan");
      ts.textContent = ch;
      ts.style.setProperty("--i", String(i));
      ts.style.setProperty("--len", `${((LEN[ch] || 4.4) * size).toFixed(0)}`);
      text.appendChild(ts);
    });
    svg.appendChild(text);
    h1.appendChild(svg);
    wordSvg = svg;
    root.classList.add("intro-writing");

    /* 5 letters x 140ms apart, 340ms of stroke, 250ms of fill after the last */
    setTimeout(() => {
      if (!wordSvg) return;
      wordSvg.remove();
      wordSvg = null;
      root.classList.remove("intro-writing");
    }, (letters.length - 1) * 140 + 340 + 250 + 40);
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
