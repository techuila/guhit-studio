// Bridge, sample bungalow: the pipe tool draws all 8 service systems. One
// run per system through the canvas, then: colors and line patterns per
// system, falls only for drainage, storm and condensate, each run on its
// layer (PIPE_LAYER), and a locked electrical layer stopping conduit.
import { clickAt, expect, log, moveTo, ready, tool } from "./lib.mjs";

const SYSTEMS = ["cold_water", "hot_water", "drainage", "vent", "storm", "conduit", "refrigerant", "condensate"];
const FALLS = new Set(["drainage", "storm", "condensate"]);
const LAYER = { cold_water: "cold_water", hot_water: "hot_water", drainage: "drainage", vent: "vent", storm: "storm", conduit: "electrical", refrigerant: "aircon", condensate: "aircon" };

const revision = (page) => page.evaluate(() => window.__app.getState().doc.revision);
async function waitRevision(page, rev) {
  await page.waitForFunction((r) => window.__app.getState().doc.revision > r, rev, { timeout: 5000 }).catch(() => {});
  await page.waitForTimeout(200);
}

export default async (page, shot) => {
  await ready(page, "bridge");
  await tool(page, "pipe");
  for (let i = 0; i < SYSTEMS.length; i++) {
    const system = SYSTEMS[i];
    await page.selectOption('[data-testid="pipe-system"]', system);
    // The v3 bridge refuses sizes under 10 mm: draw the line set with the 12.7 gas line.
    if (system === "refrigerant") await page.evaluate(() => window.__app.getState().setTool("pipe", { pipeDiameterMm: 12.7 }));
    await page.waitForTimeout(80);
    const y = 5400 - i * 650;
    await clickAt(page, 600, y);
    await clickAt(page, 2600, y);
    await clickAt(page, 4300, y - 300);
    const rev = await revision(page);
    await page.keyboard.press("Enter");
    await waitRevision(page, rev);
  }
  const runs = await page.evaluate(() =>
    window.__app
      .getState()
      .doc.project.elements.filter((e) => e.kind === "pipe")
      .map((p) => ({ id: p.id, system: p.system, material: p.material, d: p.diameter_mm, z: p.points.map((v) => +v.z.toFixed(1)) })),
  );
  log("runs", runs);
  expect("one run per system", SYSTEMS.every((s) => runs.filter((r) => r.system === s).length === 1), runs.map((r) => r.system));
  for (const r of runs) {
    const falls = r.z[r.z.length - 1] < r.z[0];
    expect(`${r.system} ${FALLS.has(r.system) ? "falls" : "stays level"} as drawn`, falls === FALLS.has(r.system), r.z);
  }
  await page.mouse.move(1400, 880);
  await page.waitForTimeout(300);
  await shot("s01-eight-systems");

  // Each run sits on its layer: hiding a layer hides exactly its runs.
  for (const key of ["electrical", "aircon", "storm"]) {
    const rev = await revision(page);
    await page.evaluate((k) => window.__app.getState().dispatch({ type: "set_layer", layer: { key: k, visible: false, locked: false } }), key);
    await waitRevision(page, rev);
    const shown = await page.evaluate(() => window.__app.getState().doc.project.elements.filter((e) => e.kind === "pipe" && window.__planController.index.visibleIds.has(e.id)).map((e) => e.system));
    const expected = SYSTEMS.filter((s) => LAYER[s] !== key);
    expect(`hiding the ${key} layer hides its runs only`, JSON.stringify(shown.sort()) === JSON.stringify([...expected].sort()), shown);
    await page.evaluate((k) => window.__app.getState().dispatch({ type: "set_layer", layer: { key: k, visible: true, locked: false } }), key);
    await page.waitForTimeout(150);
  }

  // A locked electrical layer stops the conduit tool and says so.
  await page.evaluate(() => window.__app.getState().dispatch({ type: "set_layer", layer: { key: "electrical", visible: true, locked: true } }));
  await page.waitForTimeout(200);
  await page.selectOption('[data-testid="pipe-system"]', "conduit");
  await moveTo(page, 3000, 2000);
  const hint = await page.evaluate(() => window.__planController.getUi().hint);
  expect("the conduit tool names the locked electrical layer", /The Electrical layer is locked\. Unlock it to draw conduit runs/.test(hint ?? ""), hint);
  await clickAt(page, 3000, 2000);
  const op = await page.evaluate(() => window.__planController.op.kind);
  expect("and starts no run", op === "idle", op);
  await page.evaluate(() => window.__app.getState().dispatch({ type: "set_layer", layer: { key: "electrical", visible: true, locked: false } }));
};
