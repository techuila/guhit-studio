// Frame cost with a generated plan of 200 walls: the animated frame must stay
// well inside a 60 fps budget, and the loop must still stop when it is done.
import { expect, log, ready } from "./lib.mjs";

/** Mean and worst cost of one full draw, in ms. */
const measure = (page, runs) =>
  page.evaluate((n) => {
    const c = window.__planController;
    c.draw(); // warm up
    const t = [];
    for (let i = 0; i < n; i++) {
      const a = performance.now();
      c.draw();
      t.push(performance.now() - a);
    }
    t.sort((x, y) => x - y);
    return { mean: +(t.reduce((s, v) => s + v, 0) / t.length).toFixed(2), p50: +t[Math.floor(n / 2)].toFixed(2), worst: +t[n - 1].toFixed(2) };
  }, runs);

export default async (page, shot) => {
  await ready(page, "bridge");

  const counts = await page.evaluate(() => {
    const app = window.__app;
    const doc = structuredClone(app.getState().doc);
    const level = doc.project.levels[0].id;
    // A serpentine of 200 wall segments, 20 rows of 10.
    for (let i = 0; i < 200; i++) {
      const row = Math.floor(i / 10);
      const col = i % 10;
      const y = -4000 - row * 1400;
      const x = col * 2200;
      doc.project.elements.push({
        kind: "wall",
        id: `perf-w-${i}`,
        level_id: level,
        start: { x, y },
        end: { x: x + 2000, y: y + (col % 2 === 0 ? 900 : -900) },
        thickness_mm: 150,
        height_mm: null,
        material_id: null,
      });
    }
    app.getState().setDoc(doc);
    window.__bus.emit("zoom_to_fit");
    return { walls: doc.project.elements.filter((e) => e.kind === "wall").length, total: doc.project.elements.length };
  });
  log("generated plan", counts);
  expect("the generated plan has at least 200 walls", counts.walls >= 200, counts);
  await page.waitForTimeout(600);
  await shot("p01-200-walls");

  const idle = await measure(page, 40);
  log("draw cost, nothing animating (ms)", idle);

  // Now with the hover tint, a selection, the grips and the AI preview breath
  // all live at once: the heaviest frame this canvas draws.
  await page.evaluate(() => {
    const app = window.__app;
    const s = app.getState();
    const ids = s.doc.project.elements.filter((e) => e.kind === "wall").map((e) => e.id);
    s.select([ids[3]]);
    s.setHover(ids[7]);
    const next = structuredClone(s.doc);
    app.getState().setPreview({ state: next, diff: { added: [], modified: ids.slice(0, 40), removed: [], summary: "perf" } });
  });
  await page.waitForTimeout(200);
  const busy = await measure(page, 40);
  log("draw cost, hover + selection + 40 breathing preview elements (ms)", busy);
  await shot("p02-200-walls-animating");

  expect("an animated frame stays inside the 16.7 ms budget at 200 walls", busy.mean < 16.7, busy);
  expect("the worst measured frame is still under two frames", busy.worst < 33, busy);

  const frames = await page.evaluate(async () => {
    const c = window.__planController;
    const f1 = c.frames;
    await new Promise((r) => setTimeout(r, 600));
    return { drawn: c.frames - f1, idle: c.animIdle() };
  });
  log("frames drawn in 600ms while the preview breathes", frames);
  expect("the preview keeps the loop running", frames.drawn > 5, frames);

  await page.evaluate(() => {
    window.__app.getState().setPreview(null);
    window.__app.getState().select([]);
    window.__app.getState().setHover(null);
  });
  await page.waitForTimeout(300);
  const after = await page.evaluate(async () => {
    const c = window.__planController;
    const f1 = c.frames;
    await new Promise((r) => setTimeout(r, 500));
    return { drawn: c.frames - f1, raf: c.animIdle(), running: c.animCount() };
  });
  log("after clearing everything", after);
  expect("with 200 walls on screen the loop still stops completely", after.drawn === 0 && after.raf && after.running === 0, after);
};
