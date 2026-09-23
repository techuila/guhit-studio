// 3D viewer performance probe. Drives the dev harness in a REAL headed
// Chromium (headless uses SwiftShader, which measures nothing useful), runs a
// scripted fast orbit drag and reports frame times, draw calls and triangles.
//
//   node scripts/perf-3d.mjs [out.json] [--url http://localhost:1682/...] [--keep]
//
// Needs the dev bridge and vite running:
//   ./target/debug/guhit-devbridge --port 1681 --data .devdata/perf3d
//   VITE_BRIDGE_URL=http://localhost:1681 pnpm vite --port 1682
//
// Scenes measured: the sample bungalow as it loads, the same model with 300
// generated furniture assets, and the plumbing demo (16 pipe runs) in solid
// and X-ray. For each: a 3 s orbit drag, then 2 s of no input (rendered
// frames there must be 0, the viewer is render on demand). Walk mode is
// measured on the bungalow and the plumbing demo: 3 s holding W while
// dragging to turn, then 2 s standing still (frames must be 0 there too).

import { chromium } from "playwright-core";
import { readdirSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join, dirname } from "node:path";

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};
const out = args.find((a) => !a.startsWith("--") && args[args.indexOf(a) - 1] !== "--url") ?? ".devdata/perf3d/perf.json";
const url = flag("url", "http://localhost:1682/src/viewer3d/dev/index.html");
const keep = args.includes("--keep");
const shotDir = flag("shots", null);

/** Headed Chromium, not the headless shell: we need the Metal GPU path. */
function findChromium() {
  if (process.env.CHROMIUM_PATH) return process.env.CHROMIUM_PATH;
  const cache = join(homedir(), "Library/Caches/ms-playwright");
  const dirs = existsSync(cache) ? readdirSync(cache).filter((d) => /^chromium-\d+$/.test(d)).sort() : [];
  for (const d of dirs.reverse()) {
    for (const sub of readdirSync(join(cache, d))) {
      for (const bin of [
        join(cache, d, sub, "Chromium.app/Contents/MacOS/Chromium"),
        join(cache, d, sub, "Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"),
        join(cache, d, sub, "chrome"),
      ]) {
        if (existsSync(bin)) return bin;
      }
    }
  }
  throw new Error("No cached headed Chromium found. Set CHROMIUM_PATH.");
}

const pause = (ms) => new Promise((r) => setTimeout(r, ms));

/** Wraps renderer.render so every real frame is timed from inside the page. */
const INSTRUMENT = () => {
  const e = window.__viewer3d;
  const r = e.renderer;
  if (!r.__perfPatched) {
    r.__perfPatched = true;
    const orig = r.render.bind(r);
    window.__perf = { starts: [], costs: [], calls: [], tris: [], on: false };
    r.render = (scene, camera) => {
      const p = window.__perf;
      if (!p.on) return orig(scene, camera);
      const t0 = performance.now();
      orig(scene, camera);
      const t1 = performance.now();
      p.starts.push(t0);
      p.costs.push(t1 - t0);
      p.calls.push(r.info.render.calls);
      p.tris.push(r.info.render.triangles);
    };
  }
  window.__perf.starts.length = 0;
  window.__perf.costs.length = 0;
};

const START = () => {
  const p = window.__perf;
  p.starts.length = 0;
  p.costs.length = 0;
  p.calls.length = 0;
  p.tris.length = 0;
  p.on = true;
};

const STOP = () => {
  const e = window.__viewer3d;
  const p = window.__perf;
  p.on = false;
  const starts = p.starts;
  const intervals = [];
  for (let i = 1; i < starts.length; i++) intervals.push(starts[i] - starts[i - 1]);
  const sorted = [...intervals].sort((a, b) => a - b);
  const pick = (q) => (sorted.length ? sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))] : 0);
  const mean = intervals.length ? intervals.reduce((a, b) => a + b, 0) / intervals.length : 0;
  const cost = p.costs.length ? p.costs.reduce((a, b) => a + b, 0) / p.costs.length : 0;
  const span = starts.length > 1 ? starts[starts.length - 1] - starts[0] : 0;
  return {
    frames: starts.length,
    spanMs: Number(span.toFixed(1)),
    fpsMean: mean > 0 ? Number((1000 / mean).toFixed(1)) : 0,
    frameMeanMs: Number(mean.toFixed(2)),
    frameP95Ms: Number(pick(0.95).toFixed(2)),
    frameWorstMs: Number((sorted[sorted.length - 1] ?? 0).toFixed(2)),
    fpsP95: pick(0.95) > 0 ? Number((1000 / pick(0.95)).toFixed(1)) : 0,
    renderCostMeanMs: Number(cost.toFixed(2)),
    // Worst case over the whole drag, not a single sample: frustum culling
    // makes the count swing with the camera angle.
    calls: p.calls.length ? Math.max(...p.calls) : e.renderer.info.render.calls,
    callsMean: p.calls.length ? Math.round(p.calls.reduce((a, b) => a + b, 0) / p.calls.length) : 0,
    triangles: p.tris.length ? Math.max(...p.tris) : e.renderer.info.render.triangles,
    pixelRatio: e.renderer.getPixelRatio(),
  };
};

/** Renders counted with no input at all. Must be 0: the viewer is on demand. */
const IDLE = () => {
  const p = window.__perf;
  p.on = true;
  p.starts.length = 0;
  p.costs.length = 0;
  return null;
};

async function orbit(page, box, seconds) {
  const cx = box.x + box.width * 0.5;
  const cy = box.y + box.height * 0.55;
  const step = async (i) => {
    const a = i * 0.28;
    await page.mouse.move(cx + Math.cos(a) * 170, cy + Math.sin(a) * 90);
  };
  await page.mouse.move(cx, cy);
  await page.mouse.down();
  // Get the drag going before sampling: the gap between the last idle frame
  // and the first drag frame is a CDP round trip, not a rendered frame.
  for (let i = 0; i < 4; i++) {
    await step(i);
    await pause(8);
  }
  await page.evaluate(START);
  const t0 = Date.now();
  let i = 4;
  // A fast circular drag: ~125 pointer moves a second, 40 px steps.
  while (Date.now() - t0 < seconds * 1000) {
    await step(i);
    i++;
    await pause(8);
  }
  await page.mouse.up();
  return i;
}

/**
 * Walk mode: enters walk from the store (the toolbar path), holds W while a
 * slow drag turns the view, then stands still. Frame times come from the same
 * render instrumentation as the orbit drag.
 */
async function measureWalk(page, label, seconds) {
  await settled(page);
  // The orbit pose is put back afterwards, so the scenes after a walk start
  // from the same camera as before.
  await page.evaluate(() => {
    window.__orbitPose = window.__viewer3d.currentCamera();
  });
  await page.evaluate(() => window.__viewer.getState().setNav("walk"));
  await settled(page);
  const box = await page.locator('[data-testid="viewer3d-canvas"]').boundingBox();
  await page.evaluate(INSTRUMENT);
  const cx = box.x + box.width * 0.5;
  const cy = box.y + box.height * 0.5;
  await page.mouse.move(cx, cy);
  await page.keyboard.down("KeyW");
  await pause(60);
  await page.evaluate(START);
  await page.mouse.down();
  const t0 = Date.now();
  let i = 0;
  // A slow turn while walking: the walker circles and slides along walls.
  while (Date.now() - t0 < seconds * 1000) {
    await page.mouse.move(cx + Math.sin(i * 0.05) * 120, cy + Math.sin(i * 0.03) * 20);
    i++;
    await pause(16);
  }
  await page.mouse.up();
  await page.keyboard.up("KeyW");
  const walk = await page.evaluate(STOP);
  // Let the walker stop and the interaction tail land, then stand still.
  await pause(900);
  await page.evaluate(IDLE);
  await pause(2000);
  const idle = await page.evaluate(() => {
    const p = window.__perf;
    p.on = false;
    return p.starts.length;
  });
  const walker = await page.evaluate(() => window.__viewer3d.stats().walker);
  await page.evaluate(() => window.__viewer.getState().setNav("orbit"));
  await settled(page);
  await page.evaluate(() => window.__viewer3d.flyToCamera(window.__orbitPose, 0));
  await settled(page);
  return { label, pointerMoves: i, ...walk, idleFrames: idle, walker };
}

/** Waits until nothing is animating and the frame loop has stopped. */
async function settled(page) {
  for (let i = 0; i < 100; i++) {
    const s = await page.evaluate(() => {
      const st = window.__viewer3d.stats();
      return { animations: st.animations, looping: st.looping };
    });
    if (s.animations === 0 && !s.looping) return;
    await pause(100);
  }
  console.log("warning: never settled");
}

async function measureScene(page, label) {
  await settled(page);
  const box = await page.locator('[data-testid="viewer3d-canvas"]').boundingBox();
  await page.evaluate(INSTRUMENT);
  await pause(400);

  const moves = await orbit(page, box, 3);
  const drag = await page.evaluate(STOP);

  // Let damping and any restore frame land, then watch for 2 s of silence.
  await pause(900);
  await page.evaluate(IDLE);
  await pause(2000);
  const idle = await page.evaluate(() => {
    const p = window.__perf;
    p.on = false;
    return p.starts.length;
  });

  const stats = await page.evaluate(() => {
    const e = window.__viewer3d;
    const s = e.stats();
    let meshes = 0;
    e.scene.traverse((o) => {
      if (o.isMesh || o.isInstancedMesh) meshes++;
    });
    return { meshes, geometries: s.geometries, textures: s.textures, materials: s.library.materials, programs: s.programs, looping: s.looping };
  });

  // Cost of one document change that touches a single wall.
  const rebuildMs = await page.evaluate(() => {
    const app = window.__app.getState();
    const e = window.__viewer3d;
    const times = [];
    for (let i = 0; i < 5; i++) {
      const d = structuredClone(app.doc);
      const wall = d.project.elements.find((x) => x.kind === "wall");
      wall.end.x += i % 2 ? 40 : -40;
      d.revision += 1;
      const t0 = performance.now();
      e.setDoc(d, { cutaway: false, roofVisible: true, activeLevelId: null, ghostsRemoved: null });
      times.push(performance.now() - t0);
    }
    e.anim.finishAll();
    times.sort((a, b) => a - b);
    return Number(times[Math.floor(times.length / 2)].toFixed(1));
  });

  return { label, pointerMoves: moves, ...drag, idleFrames: idle, rebuildMs, ...stats };
}

/** Grows the current document to `total` furniture assets, laid out on a grid. */
const MAKE_ASSETS = (total) => {
  const app = window.__app.getState();
  const d = structuredClone(app.doc);
  const levelId = d.project.levels[0].id;
  const keys = window.__assetKeys;
  d.project.elements = d.project.elements.filter((e) => e.kind !== "asset");
  for (let i = 0; i < total; i++) {
    const key = keys[i % keys.length];
    const col = i % 20;
    const row = Math.floor(i / 20);
    d.project.elements.push({
      kind: "asset",
      id: `perf-${i}`,
      level_id: levelId,
      catalog_key: key,
      name: key,
      category: "furniture",
      position: { x: -22000 + col * 2400, y: -14000 + row * 2400 },
      rotation_deg: (i * 37) % 360,
      width_mm: 1000,
      depth_mm: 800,
      height_mm: 900,
      elevation_mm: 0,
    });
  }
  d.revision += 1;
  app.setDoc(d);
  return d.project.elements.filter((e) => e.kind === "asset").length;
};

const browser = await chromium.launch({
  executablePath: findChromium(),
  headless: false,
  args: ["--use-gl=angle", "--use-angle=metal", "--hide-scrollbars"],
});
const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });
const errors = [];
page.on("console", (m) => {
  if (m.type() === "error") errors.push(m.text());
});
page.on("pageerror", (e) => errors.push(e.message));

const report = { url, when: new Date().toISOString(), scenes: [] };
try {
  await page.goto(url, { waitUntil: "networkidle" });
  await page.waitForFunction(() => window.__source && window.__source !== "loading", null, { timeout: 30000 });
  await page.waitForFunction(() => window.__viewer3d && !window.__viewer3d.isEmpty(), null, { timeout: 30000 });
  report.source = await page.evaluate(() => window.__source);
  report.dpr = await page.evaluate(() => window.devicePixelRatio);
  report.gpu = await page.evaluate(() => {
    const c = document.createElement("canvas").getContext("webgl2");
    const d = c && c.getExtension("WEBGL_debug_renderer_info");
    return d ? c.getParameter(d.UNMASKED_RENDERER_WEBGL) : "unknown";
  });
  await pause(2500);

  report.scenes.push(await measureScene(page, "bungalow"));
  if (shotDir) await page.screenshot({ path: join(shotDir, "bungalow.png") });
  report.scenes.push(await measureWalk(page, "bungalow-walk", 3));

  // Same model with 300 furniture assets.
  await page.evaluate(() => {
    window.__assetKeys = null;
  });
  await page.evaluate(async () => {
    const m = await import("/src/viewer3d/scene/assets.ts");
    window.__assetKeys = m.KNOWN_ASSET_KEYS;
  });
  const count = await page.evaluate(MAKE_ASSETS, 300);
  report.assetCount = count;
  await pause(3000);
  await page.evaluate(() => window.__viewer3d.anim.finishAll());
  await pause(800);
  report.scenes.push(await measureScene(page, "assets-300"));
  if (shotDir) await page.screenshot({ path: join(shotDir, "assets-300.png") });

  // The plumbing demo: pipe batches in solid and X-ray, and a walk.
  const plumbingUrl = `${url}${url.includes("?") ? "&" : "?"}template=plumbing-demo`;
  await page.goto(plumbingUrl, { waitUntil: "networkidle" });
  await page.waitForFunction(() => window.__source && window.__source !== "loading", null, { timeout: 30000 });
  await page.waitForFunction(() => window.__viewer3d && !window.__viewer3d.isEmpty(), null, { timeout: 30000 });
  await pause(2500);
  report.pipes = await page.evaluate(() => window.__viewer3d.stats().pipes);
  report.scenes.push(await measureScene(page, "plumbing"));
  await page.evaluate(() => window.__viewer.getState().setShell("xray"));
  await settled(page);
  report.scenes.push(await measureScene(page, "plumbing-xray"));
  if (shotDir) await page.screenshot({ path: join(shotDir, "plumbing-xray.png") });
  await page.evaluate(() => window.__viewer.getState().setShell("solid"));
  report.scenes.push(await measureWalk(page, "plumbing-walk", 3));
} finally {
  report.consoleErrors = errors;
  if (!keep) await browser.close();
}

mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, JSON.stringify(report, null, 2));

const cols = ["label", "frames", "fpsMean", "frameMeanMs", "frameP95Ms", "frameWorstMs", "calls", "callsMean", "triangles", "meshes", "idleFrames", "rebuildMs", "pixelRatio"];
const rows = report.scenes.map((s) => cols.map((c) => String(s[c] ?? "")));
const w = cols.map((c, i) => Math.max(c.length, ...rows.map((r) => r[i].length)));
const line = (cells) => cells.map((c, i) => c.padEnd(w[i])).join("  ");
console.log(`gpu: ${report.gpu}   dpr: ${report.dpr}   source: ${report.source}`);
console.log(line(cols));
console.log(line(w.map((n) => "-".repeat(n))));
for (const r of rows) console.log(line(r));
if (errors.length) console.log(`console errors: ${errors.length}\n  ${errors.slice(0, 5).join("\n  ")}`);
console.log(`saved ${out}`);
