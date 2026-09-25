// Real app shell (the app root on the dev server, for example
// http://localhost:1842/): L with a switch selected starts the link tool on
// it, and clicking two lights links both, one undo step each.
import { clickAt, expect, log } from "./lib.mjs";
export default async (page, shot) => {
  await page.waitForFunction(() => !!window.__app, { timeout: 15000 });
  await page.evaluate(() => window.__app.getState().createProject(`Link check ${Date.now()}`, "sample-bungalow"));
  await page.waitForFunction(() => !!window.__app.getState().doc && !!window.__planController && window.__planController.width > 0, { timeout: 15000 });
  await page.evaluate(() => window.__app.getState().loadCatalog());
  await page.waitForTimeout(500);
  const lv = await page.evaluate(() => window.__app.getState().doc.project.levels[0].id);
  const mk = (key, cat, x, y, rot, w, d, h, e) => ({ kind: "asset", id: "", level_id: lv, catalog_key: key, name: key, category: cat, position: { x, y }, rotation_deg: rot, width_mm: w, depth_mm: d, height_mm: h, elevation_mm: e, light: null, links: [], circuit: "" });
  await page.evaluate(async (els) => { for (const element of els) await window.__app.getState().dispatch({ type: "add_element", element }); }, [mk("switch-2", "electrical", 2150, 95, 180, 70, 40, 115, 1143), mk("light-ceiling", "lighting", 2500, 3000, 0, 300, 300, 60, 2940), mk("light-downlight", "lighting", 1200, 4800, 0, 150, 150, 80, 2920)]);
  await page.waitForTimeout(400);
  const ids = await page.evaluate(() => window.__app.getState().doc.project.elements.filter((e) => e.kind === "asset" && e.category !== "furniture").map((e) => ({ id: e.id, key: e.catalog_key })));
  const sw = ids.find((x) => x.key === "switch-2").id;
  const anchor = (id) => page.evaluate((i) => { const el = window.__app.getState().doc.project.elements.find((e) => e.id === i); const p = window.__planController.anchorOf(el); return { x: p.x, y: p.y }; }, id);
  const a = await anchor(sw);
  await clickAt(page, a.x, a.y);
  await page.waitForTimeout(150);
  await page.keyboard.press("l");
  await page.waitForTimeout(200);
  const st = await page.evaluate(() => ({ tool: window.__app.getState().tool, source: window.__planController.linkSource() }));
  expect("in the app, L with a switch selected starts linking it", st.tool === "link" && st.source === sw, st);
  for (const x of ids.filter((i) => i.key.startsWith("light"))) {
    const p = await anchor(x.id);
    const rev = await page.evaluate(() => window.__app.getState().doc.revision);
    await clickAt(page, p.x, p.y);
    await page.waitForFunction((r) => window.__app.getState().doc.revision > r, rev, { timeout: 5000 }).catch(() => {});
  }
  const links = await page.evaluate((i) => window.__app.getState().doc.project.elements.find((e) => e.id === i).links.length, sw);
  expect("both lights linked from the app", links === 2, links);
  await page.mouse.move(700, 450);
  await page.waitForTimeout(400);
  await shot("shell-link-tool");
  log("undo label", await page.evaluate(() => window.__app.getState().doc.undo_label));
};
