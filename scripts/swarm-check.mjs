// Swarm check (DECISIONS D56, D57): the swarm in the window, end to end.
//
// Stage A, the panel states: the dev page (src/swarm/dev) in light and dark,
// every panel state and the draft bar in each of its states.
//
// Stage B, a scripted MCP swarm: a dev bridge and Vite on lane L4's ports, a
// lead and 3 handles (Shell, Rooms, Furnish) build a small house into a draft
// over MCP while Playwright watches the window (the lead places the columns,
// so 4 cursors draw at once):
//   1. screenshots mid reveal with the cursors, the draft bar and the lock;
//   2. a frame probe: at most one pending requestAnimationFrame while
//      drawing; no frames at rest in 2D; in 3D the draft's own frames end
//      within 8 s (with the refine off), the resting view's refine then runs
//      once to its end, and the draft never starts it over;
//   3. the model stays as it was while the draft is open (the lock);
//   4. Accept one part (Shell) while it still draws, through its confirm,
//      then Accept all;
//   5. History lists one step per part, each with its part's agent as author.
//
//   cargo build -p guhit-devbridge
//   node scripts/swarm-check.mjs               # screenshots to ./swarm-check/
//
// Environment: CHROMIUM_PATH (a Chromium or headless shell), OUT (screenshot
// folder), KEEP=1 (leave the bridge's data folder), PANELS_ONLY=1 (stage A
// only, no bridge), BRIDGE_PORT (1449), UI_PORT (1469).

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";

const ROOT = resolve(import.meta.dirname, "..");
const OUT = resolve(process.env.OUT ?? join(ROOT, "swarm-check"));
const BRIDGE_BIN = join(ROOT, "target", "debug", process.platform === "win32" ? "guhit-devbridge.exe" : "guhit-devbridge");
const BRIDGE_PORT = Number(process.env.BRIDGE_PORT ?? 1449);
const UI_PORT = Number(process.env.UI_PORT ?? 1469);
const DATA = join(ROOT, ".devdata", "swarm-check");
const BRIDGE = `http://localhost:${BRIDGE_PORT}`;
const PANELS_ONLY = process.env.PANELS_ONLY === "1";

function findChromium() {
  if (process.env.CHROMIUM_PATH) return process.env.CHROMIUM_PATH;
  const cache = join(homedir(), "Library/Caches/ms-playwright");
  const dirs = existsSync(cache) ? readdirSync(cache).filter((d) => d.startsWith("chromium_headless_shell-")).sort() : [];
  for (const d of dirs.reverse()) {
    for (const sub of readdirSync(join(cache, d))) {
      for (const name of ["headless_shell", "chrome-headless-shell"]) {
        const bin = join(cache, d, sub, name);
        if (existsSync(bin)) return bin;
      }
    }
  }
  throw new Error("No cached Chromium found. Set CHROMIUM_PATH.");
}

// ------------------------------------------------------------------ helpers

const children = [];
function start(cmd, args, env = {}) {
  const child = spawn(cmd, args, { cwd: ROOT, env: { ...process.env, ...env }, stdio: ["ignore", "pipe", "pipe"] });
  child.stdout.on("data", () => {});
  child.stderr.on("data", (d) => {
    const line = String(d).trim();
    if (/error|panic/i.test(line)) console.log(`[${args[0] ?? cmd}] ${line.slice(0, 300)}`);
  });
  children.push(child);
  return child;
}

async function until(what, fn, ms = 20000) {
  const startAt = Date.now();
  for (;;) {
    try {
      const v = await fn();
      if (v) return v;
    } catch {
      // not yet
    }
    if (Date.now() - startAt > ms) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 200));
  }
}

const wait = (ms) => new Promise((r) => setTimeout(r, ms));

async function ipc(cmd, args = {}) {
  const res = await fetch(`${BRIDGE}/ipc/${cmd}`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(args) });
  const body = await res.json();
  if (!res.ok) throw Object.assign(new Error(`${cmd}: ${body.message}`), { body });
  return body;
}

let rpcId = 1;
/** One MCP tool call; `agent` names the caller (a handle binds it to its part). */
async function tool(name, args = {}) {
  const res = await fetch(`${BRIDGE}/mcp`, {
    method: "POST",
    headers: { "content-type": "application/json", accept: "application/json, text/event-stream" },
    body: JSON.stringify({ jsonrpc: "2.0", id: rpcId++, method: "tools/call", params: { name, arguments: args } }),
  });
  const body = await res.json();
  if (body.error) throw new Error(`mcp ${name}: ${JSON.stringify(body.error)}`);
  const r = body.result;
  return { isError: !!r.isError, text: r.content?.find((c) => c.type === "text")?.text ?? "", json: r.structuredContent ?? null };
}

const results = [];
function check(name, ok, detail = "") {
  results.push({ name, ok: !!ok });
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail ? `: ${String(detail).slice(0, 700)}` : ""}`);
}

/** Counts requestAnimationFrame calls and callbacks (`window.__frames`). */
const FRAME_PROBE = () => {
  // Frames run, the most pending at once, the most pending at once per loop
  // (the file that asked: the plan's controller, the 3D engine), and who
  // asked since the last reset.
  const raf = window.requestAnimationFrame.bind(window);
  const caf = window.cancelAnimationFrame.bind(window);
  const pending = new Map();
  const probe = {
    asked: 0,
    ran: 0,
    maxPending: 0,
    maxPerLoop: {},
    callers: {},
    /** Frames run, by the loop that asked (a frame asked before the reset counts for its loop too). */
    ranBy: {},
    /** Who asked for the frames still pending at the reset. */
    carried: [],
    reset() {
      this.asked = 0;
      this.ran = 0;
      this.maxPending = pending.size;
      this.maxPerLoop = {};
      this.callers = {};
      this.ranBy = {};
      this.carried = [...pending.values()].map((p) => p.stack);
    },
    /** Frames asked for and not run yet. */
    pendingNow() {
      return pending.size;
    },
  };
  window.__frames = probe;
  window.requestAnimationFrame = (cb) => {
    probe.asked++;
    const frames = (new Error().stack ?? "").split("\n").slice(2).filter((l) => l.includes("/src/"));
    const stack = frames.slice(0, 3).map((l) => l.trim().replace(/\(?https?:\/\/[^/]+\/|\?t=\d+|\)$/g, "")).join(" < ");
    probe.callers[stack] = (probe.callers[stack] ?? 0) + 1;
    const loop = (frames[0] ?? "").match(/src\/[\w/.-]+\.tsx?/)?.[0] ?? "other";
    const id = raf((t) => {
      pending.delete(id);
      probe.ran++;
      probe.ranBy[loop] = (probe.ranBy[loop] ?? 0) + 1;
      cb(t);
    });
    pending.set(id, { loop, stack });
    probe.maxPending = Math.max(probe.maxPending, pending.size);
    const same = [...pending.values()].filter((p) => p.loop === loop).length;
    probe.maxPerLoop[loop] = Math.max(probe.maxPerLoop[loop] ?? 0, same);
    return id;
  };
  window.cancelAnimationFrame = (id) => {
    pending.delete(id);
    caf(id);
  };
};

async function framesOver(page, ms, callers = false) {
  await page.evaluate(() => window.__frames.reset());
  await wait(ms);
  return page.evaluate(
    (withCallers) => ({
      ran: window.__frames.ran,
      maxPending: window.__frames.maxPending,
      ...(withCallers ? { callers: window.__frames.callers, ranBy: window.__frames.ranBy, carried: window.__frames.carried } : {}),
    }),
    callers,
  );
}

/**
 * Frames over `ms` from the moment the 3D engine is idle, waited for up to
 * `idleMs`: no frame pending, its loop off, and no async light work that
 * ends in frames (an exposure reading on its way back from the GPU, a lamp
 * set compiling). Counting from an arbitrary moment catches that work
 * finishing, or a one-shot frame asked just before the window opened, which
 * is not a loop that fails to stop. `idle` false: it never went idle in time.
 */
async function framesAtRest(page, ms, idleMs = 5000) {
  const idle = await page.evaluate(
    (limit) =>
      new Promise((done) => {
        const t0 = performance.now();
        const poll = () => {
          const s = window.__viewer3d?.stats();
          const quiet = window.__frames.pendingNow() === 0 && !(s?.looping ?? false) && !(s?.light.metering ?? false) && !(s?.light.compiling ?? null);
          if (quiet || performance.now() - t0 > limit) {
            // Reset in the same task the quiet was seen: nothing slips in between.
            window.__frames.reset();
            done(quiet);
            return;
          }
          setTimeout(poll, 50);
        };
        poll();
      }),
    idleMs,
  );
  await wait(ms);
  const frames = await page.evaluate(() => ({ ran: window.__frames.ran, maxPending: window.__frames.maxPending, callers: window.__frames.callers, ranBy: window.__frames.ranBy, carried: window.__frames.carried }));
  return { idle, ...frames };
}

/** A screenshot of a page or a locator. One that cannot be taken in time is noted, and the run goes on. */
async function snap(target, name) {
  try {
    await target.screenshot({ path: join(OUT, name), timeout: 60000 });
  } catch (e) {
    console.log(`  (no screenshot ${name}: ${String(e?.message ?? e).split("\n")[0]})`);
  }
}

// ------------------------------------------------------------------ stage A

async function panels(browser) {
  for (const scheme of ["light", "dark"]) {
    const page = await browser.newPage({ viewport: { width: 1840, height: 1500 }, colorScheme: scheme });
    await page.goto(`http://localhost:${UI_PORT}/src/swarm/dev/index.html`, { waitUntil: "networkidle" });
    await page.waitForSelector('[data-state="running"]');
    await wait(400);
    await snap({ screenshot: (o) => page.screenshot({ ...o, fullPage: true }) }, `A-panels-${scheme}.png`);
    for (const name of ["idle", "running", "mcp", "ready"]) {
      await snap(page.locator(`[data-state="${name}"]`), `A-panel-${name}-${scheme}.png`);
    }
    for (const name of ["bar-drawing", "bar-ready", "bar-conflicts", "bar-accept-asks", "bar-discard-asks", "bar-accept-differs"]) {
      await snap(page.locator(`[data-state="${name}"]`), `A-${name}-${scheme}.png`);
    }
    const states = await page.locator("figure[data-state]").count();
    check(`the dev page shows every panel and bar state (${scheme})`, states >= 16, `${states} states`);
    await page.close();
  }
}

// ------------------------------------------------------------------ stage B

/** A small house: 4 outer walls and one inner, two rooms, doors, windows, furniture. */
const W = 9000;
const H = 7000;
const SHELL = [
  { points: [{ x: 0, y: 0 }, { x: W, y: 0 }], status: "Laying out the outer walls" },
  { points: [{ x: W, y: 0 }, { x: W, y: H }], status: "Laying out the outer walls" },
  { points: [{ x: W, y: H }, { x: 0, y: H }], status: "Laying out the outer walls" },
  { points: [{ x: 0, y: H }, { x: 0, y: 0 }], status: "Laying out the outer walls" },
  { points: [{ x: 5000, y: 0 }, { x: 5000, y: H }], status: "Splitting living from bedroom" },
];

/**
 * One part's steps, a status line before each. One connection carries every
 * part here, and it may make 5 changes a second: a refusal `rate_limited`
 * waits a second and tries again, as its hint says.
 */
async function runPart(handle, steps, gapMs) {
  const out = [];
  for (const s of steps) {
    // The status line goes with the step (set_status is in an extended toolset: quiet if it is off).
    await tool("set_status", { agent: handle, text: s.status }).catch(() => null);
    let r = await tool(s.tool, { ...s.args, agent: handle });
    for (let i = 0; i < 3 && r.isError && r.text.includes("rate_limited"); i++) {
      await wait(1100);
      r = await tool(s.tool, { ...s.args, agent: handle });
    }
    out.push(r);
    if (r.isError) console.log(`  ${handle.split("-").pop()} ${s.tool}: ${r.text.slice(0, 200)}`);
    await wait(gapMs);
  }
  return out;
}

async function swarmRun(browser) {
  const name = `Swarm check ${Date.now()}`;
  await ipc("hub_create", { name, settings: null, template: "blank" });
  await ipc("hub_close", {});

  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  await page.addInitScript(FRAME_PROBE);
  page.errors = [];
  page.on("pageerror", (e) => page.errors.push(e.message));
  page.on("console", (m) => {
    if (m.type() === "error" && !m.text().startsWith("Failed to load resource")) page.errors.push(m.text());
  });
  await page.goto(`http://localhost:${UI_PORT}/?bridge=${encodeURIComponent(BRIDGE)}`, { waitUntil: "networkidle" });
  await page.click(`[aria-label="Open ${name}"]`);
  await page.waitForSelector('nav[aria-label="Drawing tools"]');
  // The dock (lazy) listens for the swarm: wait for it, so the window watches the swarm start.
  await page.waitForSelector('[aria-label="Ask the copilot or run a swarm"]');
  const before = await ipc("doc_state");

  // ---- The lead starts the swarm with 3 parts.
  const started = await tool("swarm", {
    agent: "Claude Code",
    action: "start",
    brief: "A small house on a 12 x 15 m lot: living and a bedroom, a front door, windows, furniture.",
    summary: "Small house, 9 x 7 m",
    setup: { levels: [] },
    parts: [
      { key: "shell", name: "Shell", role: "Walls", instructions: "Draw the walls.", scope: { levels: [], kinds: ["wall"] }, after: [], milestones: ["walls"] },
      { key: "rooms", name: "Rooms", role: "Rooms, doors, windows", instructions: "Name the rooms, add doors and windows.", scope: { levels: [], kinds: ["room", "opening"] }, after: [], milestones: [] },
      { key: "furnish", name: "Furnish", role: "Furniture", instructions: "Furnish the rooms.", scope: { levels: [], kinds: ["asset"] }, after: [], milestones: [] },
    ],
  });
  const handles = Object.fromEntries((started.json?.handles ?? []).map((h) => [h.key, h.handle]));
  check("the lead starts a swarm of 3 handles over MCP", !started.isError && Object.keys(handles).length === 3, started.isError ? started.text : Object.keys(handles).join(", "));
  if (started.isError) return { page, ok: false };

  await page.waitForSelector('[data-testid="swarm-draft-bar"]', { timeout: 10000 });
  check("the window opens the dock on Swarm when the swarm starts", await page.locator('[data-testid="swarm-panel"]').isVisible());

  // ---- The parts draw at once. Rooms and Furnish start once Shell has walls.
  // The frame probe counts over the whole drawing, until the reveal has caught up.
  await page.evaluate(() => window.__frames.reset());
  const shell = runPart(handles.shell, SHELL.map((w) => ({ tool: "add_walls", args: { points: w.points }, status: w.status })), 500);
  await wait(1900);
  const rooms = runPart(
    handles.rooms,
    [
      { tool: "add_room", args: { rect: { x: 0, y: 0, width_mm: 5000, depth_mm: H }, name: "Living", usage: "living", walls: false }, status: "Naming the living room" },
      { tool: "add_opening", args: { kind: "door", at: { x: 2500, y: 0 }, width_mm: 900 }, status: "Front door, 900 mm" },
      { tool: "add_opening", args: { kind: "window", at: { x: 2500, y: H }, width_mm: 1800, style: "sliding" }, status: "Living window, 1800 mm" },
      { tool: "add_room", args: { rect: { x: 5000, y: 0, width_mm: W - 5000, depth_mm: H }, name: "Bedroom", usage: "bedroom", walls: false }, status: "Naming the bedroom" },
      { tool: "add_opening", args: { kind: "door", at: { x: 5000, y: 3500 }, width_mm: 800 }, status: "Bedroom door, 800 mm" },
      { tool: "add_opening", args: { kind: "window", at: { x: W, y: 3500 }, width_mm: 1200 }, status: "Bedroom window, 1200 mm" },
    ],
    700,
  );
  const lead = runPart(
    "Claude Code",
    [
      { x: 0, y: 0 },
      { x: W, y: 0 },
      { x: W, y: H },
      { x: 0, y: H },
    ].map((at) => ({ tool: "add_column", args: { at, width_mm: 300 }, status: "Setting the corner columns" })),
    750,
  );
  const furnish = runPart(
    handles.furnish,
    [
      { tool: "place_asset", args: { catalog_key: "sofa-3", at: { x: 2500, y: 3000 } }, status: "Sofa facing the window" },
      { tool: "place_asset", args: { catalog_key: "bed-double", at: { x: 7000, y: 4500 } }, status: "Bed against the wall" },
      { tool: "place_asset", args: { catalog_key: "dining-4", at: { x: 2500, y: 5500 } }, status: "Table for four" },
    ],
    900,
  );

  // Mid reveal: the cursors draw.
  await wait(1400);
  const cursors = await page.locator('[data-testid^="agent-cursor-"]').count();
  await snap(page, "B-01-mid-reveal.png");
  check("4 cursors draw on the plan at once", cursors >= 4, `${cursors} cursors`);
  await wait(800);
  await snap(page, "B-02-mid-reveal-later.png");

  const [shellOut, roomsOut, furnishOut, leadOut] = await Promise.all([shell, rooms, furnish, lead]);
  const refused = [...shellOut, ...roomsOut, ...furnishOut, ...leadOut].filter((r) => r.isError);
  check("every part's steps land in the draft", refused.length === 0, refused.map((r) => r.text.slice(0, 80)).join(" | "));
  const main = await ipc("doc_state");
  check("the model does not move while the swarm draws (D56)", main.revision === before.revision && main.project.elements.length === before.project.elements.length, `revision ${before.revision} -> ${main.revision}`);
  const locked = await ipc("doc_apply", { command: { type: "add_wall", start: { x: -3000, y: -3000 }, end: { x: -1000, y: -3000 }, thickness_mm: null, height_mm: null, material_id: null, level_id: null } }).then(
    () => null,
    (e) => e.body?.code,
  );
  check("a person's write is refused swarm_running while the draft is open (D57)", locked === "swarm_running", String(locked));

  // ---- The reveal catches up, then everything rests.
  await until("the reveal to finish", () => page.evaluate(() => !window.__swarm?.reveal.pending(performance.now())), 30000);
  const drawing = await page.evaluate(() => ({ ran: window.__frames.ran, perLoop: window.__frames.maxPerLoop }));
  // The rule is per frame loop: the plan's controller and the 3D engine. UI helpers (one rAF per component) are not loops.
  const planPending = drawing.perLoop["src/editor2d/controller.ts"] ?? 0;
  const enginePending = drawing.perLoop["src/viewer3d/engine/ViewerEngine.ts"] ?? 0;
  check("while drawing, the plan keeps at most one frame pending", drawing.ran > 0 && planPending === 1 && enginePending <= 1, JSON.stringify(drawing));
  await wait(600);
  await snap(page, "B-03-draft-done-2d.png");
  await snap(page.locator('[data-testid="swarm-draft-bar"]'), "B-04-draft-bar.png");
  await snap(page.locator('[data-testid="swarm-lock"]'), "B-05-lock-banner.png");
  await snap(page.locator('[data-testid="swarm-panel"]'), "B-06-panel.png");
  const rest2d = await framesOver(page, 1500);
  check("at rest the plan draws no frames", rest2d.ran === 0, JSON.stringify(rest2d));
  const toolsOff = await page.locator('nav[aria-label="Drawing tools"] button[aria-label="Wall"]').isDisabled();
  check("editing tools are off while the draft is open", toolsOff);

  // ---- 3D: the draft group fades in, then the view sleeps.
  await page.evaluate(() => window.__app.getState().setViewMode("3d"));
  await until("the 3D draft group", () => page.evaluate(() => (window.__viewer3d?.draftState().shown ?? 0) > 0), 15000);
  await wait(1200);
  const draft3d = await page.evaluate(() => window.__viewer3d.draftState());
  check("the 3D view shows the draft as its own group", draft3d.shown > 0, JSON.stringify(draft3d));
  await snap(page, "B-07-draft-3d.png");
  // From here the draft is still. Any redraw the draft code asks for would start the resting view's refine over.
  await page.evaluate(() => {
    const e = window.__viewer3d;
    const inv = e.invalidate;
    window.__draftRedraws = [];
    window.__redraws = {};
    e.invalidate = () => {
      const stack = (new Error().stack ?? "").split("\n").slice(2).filter((l) => l.includes("/src/")).slice(0, 4).map((l) => l.trim().replace(/\(?https?:\/\/[^/]+\/|\?t=\d+|\)$/g, "")).join(" < ");
      if (/Draft|\/swarm\//.test(stack)) window.__draftRedraws.push(stack);
      window.__redraws[stack] = (window.__redraws[stack] ?? 0) + 1;
      inv();
    };
  });
  // Each rest window opens once no frame is pending and the engine's loop is
  // off (bounded), so a one-shot frame asked just before is not counted; a
  // loop that does not stop never goes idle and fails.
  const restDetail = (r) => `${JSON.stringify({ idle: r.idle, ran: r.ran, maxPending: r.maxPending })}${r.ran || !r.idle ? `; frames ${JSON.stringify({ asked: r.callers, ranBy: r.ranBy, carried: r.carried })}` : ""}`;
  // 1. The refine off: what is left is the draft's own work, which ends within 8 s.
  await page.evaluate(() => window.__viewer.getState().setRefine(false));
  const stillAt = Date.now();
  await page.evaluate(() => (window.__redraws = {}));
  const rest3d = await framesAtRest(page, 1500, 8000);
  const why = await page.evaluate(() => window.__redraws);
  check(
    "at rest the 3D view draws no frames (still within 8 s, the refine off)",
    rest3d.idle && rest3d.ran === 0,
    `${restDetail(rest3d)}, still after ${Date.now() - stillAt - 1500} ms${rest3d.ran ? `; redraws ${JSON.stringify(why)}` : ""}`,
  );
  // 2. The refine on (the default): it takes its 64 samples once, then stops. A software GPU takes a while per sample.
  await page.evaluate(() => window.__viewer.getState().setRefine(true));
  const refineAt = Date.now();
  const refined = await until("the refine to finish", () => page.evaluate(() => { const l = window.__viewer3d.stats().light; return !l.refining && l.refineSamples > 0 ? l.refineSamples : null; }), 120000).catch(() => null);
  await page.evaluate(() => (window.__redraws = {}));
  const afterRefine = await framesAtRest(page, 1500, 5000);
  const redraws = await page.evaluate(() => window.__draftRedraws);
  const why2 = await page.evaluate(() => window.__redraws);
  check(
    "the refine runs to the end and the 3D view sleeps; the draft never starts it over",
    refined !== null && afterRefine.idle && afterRefine.ran === 0 && redraws.length === 0,
    `${refined ?? "no"} samples in ${Date.now() - refineAt - 1500} ms, then ${restDetail(afterRefine)}; draft redraws ${redraws.length}${redraws.length ? `: ${redraws[0]}` : ""}${afterRefine.ran ? `; redraws ${JSON.stringify(why2)}` : ""}`,
  );
  await page.evaluate(() => window.__app.getState().setViewMode("2d"));
  await wait(500);

  // ---- Accept one part while it is still drawing, then all. Each Accept that asks is confirmed.
  const swarmState = () => page.evaluate(() => window.__swarm.useSwarm.getState());
  const ids = await page.evaluate(() => Object.fromEntries((window.__swarm.useSwarm.getState().status?.parts ?? []).map((p) => [p.key, p.agent_id])));
  const shellBefore = await page.evaluate(() => window.__swarm.useSwarm.getState().status?.parts.find((p) => p.key === "shell")?.state ?? null);
  await page.click('[data-testid="swarm-accept-shell"]');
  const asked = await until("the confirm", () => page.evaluate(() => window.__swarm.useSwarm.getState().ask?.text ?? null), 5000).catch(() => null);
  await snap(page, "B-08-accept-one-asks.png");
  check("accepting a part still drawing asks first and says so", shellBefore !== "working" || /Shell is still drawing/.test(asked ?? ""), `${shellBefore}: ${asked}`);
  if (asked) await page.click('[data-testid="swarm-ask-confirm"]');
  const shellAfter = await until("Shell to be accepted", () => page.evaluate(() => {
    const st = window.__swarm.useSwarm.getState();
    const shell = st.status?.parts.find((p) => p.key === "shell");
    return shell?.state === "accepted" || (st.status && !shell) ? "accepted" : null;
  }), 10000).catch(() => null);
  const oneHistory = (await ipc("history_list")).undo.filter((s) => s.label.startsWith("Swarm: "));
  const shellStep = oneHistory.find((s) => s.label === "Swarm: Shell");
  const note = (await swarmState()).note?.text ?? null;
  check(
    "Accept one part (Shell) takes it into the model as one step by its agent",
    shellAfter === "accepted" && !!shellStep && shellStep.author_id === ids.shell,
    `shell ${shellAfter ?? "not accepted"}; history ${oneHistory.map((s) => s.label).join(", ") || "none"}${note ? `; note: ${note}` : ""}`,
  );
  await snap(page, "B-09-after-accept-one.png");
  if (await page.locator('[data-testid="swarm-accept-all"]').count()) {
    await page.click('[data-testid="swarm-accept-all"]');
    const askAll = await until("the confirm", () => page.evaluate(() => window.__swarm.useSwarm.getState().ask?.text ?? null), 3000).catch(() => null);
    if (askAll) await page.click('[data-testid="swarm-ask-confirm"]');
    await until("the draft to close", async () => (await page.locator('[data-testid="swarm-overlay"]').count()) === 0, 10000).catch(() => null);
  }
  const done = await ipc("doc_state");
  const closed = (await ipc("swarm_status")) === null;
  check("Accept all brings the draft into the model and closes the swarm", closed && done.project.elements.length > before.project.elements.length, `${done.project.elements.length} elements, swarm ${closed ? "closed" : "open"}`);
  await snap(page, "B-10-accepted.png");

  // ---- History: one step per part, with its agent as author.
  const history = await ipc("history_list");
  const steps = history.undo.filter((s) => s.label.startsWith("Swarm: "));
  const byPart = Object.fromEntries(steps.map((s) => [s.label.slice("Swarm: ".length).toLowerCase(), s]));
  const keys = ["lead", "shell", "rooms", "furnish"];
  const authors = keys.map((k) => `${k}=${byPart[k]?.author_id === ids[k] ? "ok" : (byPart[k]?.author_id ?? "missing")}`);
  check("History lists one step per part", keys.every((k) => byPart[k]) && steps.length === keys.length, steps.map((s) => s.label).join(", "));
  check("each part's step has its agent as author", keys.every((k) => byPart[k] && byPart[k].author_id === ids[k]), authors.join(" "));
  check("no errors in the window", page.errors.length === 0, page.errors.slice(0, 3).join(" | "));
  return { page, ok: true };
}

// -------------------------------------------------------------------- run

mkdirSync(OUT, { recursive: true });
if (!PANELS_ONLY && !existsSync(BRIDGE_BIN)) {
  console.error(`No ${BRIDGE_BIN}. Build it first: cargo build -p guhit-devbridge`);
  process.exit(2);
}
let browser;
try {
  start(process.execPath, [join(ROOT, "node_modules", "vite", "bin", "vite.js"), "--port", String(UI_PORT), "--strictPort"], { VITE_BRIDGE_URL: BRIDGE });
  if (!PANELS_ONLY) {
    rmSync(DATA, { recursive: true, force: true });
    start(BRIDGE_BIN, ["--port", String(BRIDGE_PORT), "--data", DATA]);
    await until("the bridge", async () => (await fetch(`${BRIDGE}/health`)).ok);
  }
  await until("the UI", async () => (await fetch(`http://localhost:${UI_PORT}/`)).ok, 60000);
  browser = await chromium.launch({
    executablePath: findChromium(),
    args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
  });
  await panels(browser);
  if (!PANELS_ONLY) await swarmRun(browser);
} catch (e) {
  check("the run finished", false, e?.stack ?? String(e));
} finally {
  await browser?.close().catch(() => {});
  for (const c of children) c.kill();
  if (!PANELS_ONLY && process.env.KEEP !== "1") rmSync(DATA, { recursive: true, force: true });
}
const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length} passed, ${failed.length} failed. Screenshots: ${OUT}`);
process.exit(failed.length > 0 ? 1 : 0);
