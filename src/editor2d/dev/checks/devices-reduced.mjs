// Same canvas with prefers-reduced-motion: reduce, for device links: a new
// link appears at once, a bow flip lands at once, and the loop goes idle.
import { clickAt, expect, ready, tool } from "./lib.mjs";
export default async (page) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.reload({ waitUntil: "networkidle" });
  await ready(page, "bridge");
  const lv = await page.evaluate(() => window.__app.getState().doc.project.levels[0].id);
  const mk = (key, cat, x, y, rot, w, d, h, e) => ({ kind: "asset", id: "", level_id: lv, catalog_key: key, name: key, category: cat, position: { x, y }, rotation_deg: rot, width_mm: w, depth_mm: d, height_mm: h, elevation_mm: e, light: null, links: [], circuit: "" });
  await page.evaluate(async (els) => { for (const element of els) await window.__app.getState().dispatch({ type: "add_element", element }); }, [mk("switch-1", "electrical", 2150, 95, 180, 70, 40, 115, 1143), mk("light-ceiling", "lighting", 2500, 3000, 0, 300, 300, 60, 2940)]);
  await page.waitForTimeout(300);
  const [sw, li] = await page.evaluate(() => { const els = window.__app.getState().doc.project.elements; return [els.find((e) => e.catalog_key === "switch-1").id, els.find((e) => e.catalog_key === "light-ceiling").id]; });
  const anchor = (id) => page.evaluate((i) => { const el = window.__app.getState().doc.project.elements.find((e) => e.id === i); const p = window.__planController.anchorOf(el); return { x: p.x, y: p.y }; }, id);
  await tool(page, "link");
  let p = await anchor(sw);
  await clickAt(page, p.x, p.y);
  const rev = await page.evaluate(() => window.__app.getState().doc.revision);
  p = await anchor(li);
  await clickAt(page, p.x, p.y);
  await page.waitForFunction((r) => window.__app.getState().doc.revision > r, rev, { timeout: 5000 });
  await page.waitForTimeout(100);
  let keys = await page.evaluate(() => Object.keys(window.__planController.animSnapshot()));
  expect("under reduced motion a new link has no draw-in track", !keys.some((k) => k.startsWith("lnk:")), keys);
  const l = await page.evaluate(() => window.__planController.linkDrawList()[0]);
  const hp = await page.evaluate(([x, y]) => { const c = window.__planController; const r = document.querySelector('[data-testid="plan-canvas"] canvas').getBoundingClientRect(); return { x: r.left + c.view.ox + x * c.view.scale, y: r.top + c.view.oy - y * c.view.scale }; }, [l.handle.x, l.handle.y]);
  await page.mouse.move(hp.x, hp.y);
  await page.mouse.click(hp.x, hp.y);
  await page.waitForTimeout(60);
  const after = await page.evaluate(() => window.__planController.linkDrawList()[0].bow);
  expect("a flip lands at once", after === -l.bow, { before: l.bow, after });
  await page.mouse.move(10, 400);
  await page.waitForTimeout(200);
  const idle = await page.evaluate(() => ({ idle: window.__planController.animIdle(), running: window.__planController.animCount() }));
  expect("and the loop goes idle", idle.idle && idle.running === 0, idle);
};
