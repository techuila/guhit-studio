/* GUHIT Studio site. No dependencies.
   One passive scroll listener, one rAF, all reads batched before all writes.
   Everything degrades: with JS off the acts render in their end state, because
   the `--p` progress variable falls back to 1 in the stylesheet. */

const root = document.documentElement;
const reduced = matchMedia("(prefers-reduced-motion: reduce)");
const narrow = matchMedia("(max-width: 860px)");
const fine = matchMedia("(hover: hover) and (pointer: fine)");
const clamp01 = (n) => (n < 0 ? 0 : n > 1 ? 1 : n);

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
