// Bridge, sample bungalow: the pipe tool end to end. Adds a kitchen sink, a WC
// and a lavatory, then draws through the canvas: a cold water run with a riser,
// a drainage run falling as it is drawn, tee snaps, a selected pipe with its
// point heights and a node drag, then hidden and locked pipe layers.
import { at, clickAt, dragTo, expect, log, moveTo, ready, tool } from "./lib.mjs";

const pipes = (page) =>
  page.evaluate(() =>
    window.__app
      .getState()
      .doc.project.elements.filter((e) => e.kind === "pipe")
      .map((p) => ({ id: p.id, system: p.system, d: p.diameter_mm, points: p.points.map((v) => [Math.round(v.x), Math.round(v.y), +v.z.toFixed(1)]) })),
  );

const op = (page) =>
  page.evaluate(() => {
    const c = window.__planController;
    const o = c.op;
    return {
      kind: o.kind,
      points: o.points ? o.points.map((v) => [Math.round(v.x), Math.round(v.y), +v.z.toFixed(1)]) : null,
      penZ: o.penZ ?? null,
      snap: c.snapResult ? { type: c.snapResult.type, label: c.snapResult.label ?? null, z: c.snapResult.z ?? null, x: Math.round(c.snapResult.point.x), y: Math.round(c.snapResult.point.y) } : null,
      heightEntry: c.heightEntry ? c.heightEntry.fields[0] : null,
      hint: c.getUi().hint,
    };
  });

const setOptions = (page, options) => page.evaluate((o) => window.__app.getState().setTool("pipe", o), options);

async function press(page, key, times = 1) {
  for (let i = 0; i < times; i++) {
    await page.keyboard.press(key);
    await page.waitForTimeout(30);
  }
  await page.waitForTimeout(60);
}

async function waitRevision(page, rev) {
  await page.waitForFunction((r) => window.__app.getState().doc.revision > r, rev, { timeout: 5000 }).catch(() => {});
  await page.waitForTimeout(250);
}

const revision = (page) => page.evaluate(() => window.__app.getState().doc.revision);

export default async (page, shot) => {
  await ready(page, "bridge");
  const level = await page.evaluate(() => window.__app.getState().doc.project.levels[0].id);
  const unit = await page.evaluate(() => window.__app.getState().doc.project.settings.display_unit);
  log("display unit", unit);

  // Fixtures: local +y is the back, so a back against a wall faces that wall.
  const fixture = (key, name, x, y, rot, w, d, h, elev) => ({
    kind: "asset", id: "", level_id: level, catalog_key: key, name, category: key === "kitchen-sink" ? "kitchen" : "sanitary",
    position: { x, y }, rotation_deg: rot, width_mm: w, depth_mm: d, height_mm: h, elevation_mm: elev,
  });
  await page.evaluate(async (els) => {
    for (const element of els) await window.__app.getState().dispatch({ type: "add_element", element });
  }, [
    fixture("kitchen-sink", "Kitchen sink", 3500, 375, 180, 1200, 600, 900, 0),
    fixture("wc", "Water closet", 7575, 1500, 270, 400, 700, 780, 0),
    fixture("lavatory", "Lavatory", 7715, 2600, 270, 500, 420, 200, 650),
  ]);
  await page.waitForTimeout(400);

  // ------------------------------------------------------------ cold water with a riser
  await tool(page, "pipe");
  await moveTo(page, 3480, 110);
  let o = await op(page);
  log("idle over the sink back", o);
  expect("the sink's back edge middle is a snap target", o.snap?.type === "fixture" && o.snap.x === 3500 && o.snap.y === 75, o.snap);
  expect("idle hint names the system", /cold water run/.test(o.hint ?? ""), o.hint);
  await shot("p01-cold-idle-fixture-snap");
  await clickAt(page, 3480, 110);
  const pulse = await page.evaluate(() => window.__planController.animSnapshot()["pipe.pulse"]);
  expect("placing a point starts its settle ring", pulse !== undefined, pulse);
  await moveTo(page, 4800, 90);
  await clickAt(page, 4800, 90);
  o = await op(page);
  expect("two points at the start height 300", JSON.stringify(o.points) === JSON.stringify([[3500, 75, 300], [4800, 75, 300]]), o.points);
  await press(page, "PageUp", 9);
  o = await op(page);
  expect("PageUp x9 raises the pen to 1200", o.penZ === 1200, o.penZ);
  await moveTo(page, 4790, 3000);
  await shot("p02-cold-riser-pending");
  await clickAt(page, 4790, 3000);
  o = await op(page);
  expect(
    "the riser went in at the last point, then the run continued at 1200",
    o.points.length === 4 && JSON.stringify(o.points.slice(0, 3)) === JSON.stringify([[3500, 75, 300], [4800, 75, 300], [4800, 75, 1200]]) && o.points[3][0] === 4800 && o.points[3][2] === 1200,
    o.points,
  );
  let rev = await revision(page);
  await page.keyboard.press("Enter");
  // The finished run settles in (add:<id>, 180 ms): look for it while it runs.
  const settled = await page
    .waitForFunction(() => Object.keys(window.__planController.animSnapshot()).some((k) => k.startsWith("add:")), null, { timeout: 3000, polling: 10 })
    .then(() => true)
    .catch(() => false);
  await waitRevision(page, rev);
  let list = await pipes(page);
  const cold = list.find((p) => p.system === "cold_water");
  expect("finishing a run settles the new pipe in", settled, settled);
  log("cold run", cold);
  expect("Enter commits one cold water pipe", list.length === 1 && !!cold && cold.points.length === 4, list);
  const undoLabel = await page.evaluate(() => window.__app.getState().doc.undo_label);
  log("undo label", undoLabel);
  await page.mouse.move(40, 400);
  await page.waitForTimeout(300);
  await shot("p03-cold-committed");

  // ------------------------------------------------------------ typed height, Escape
  await moveTo(page, 2500, 1500);
  await page.keyboard.type("h1500");
  o = await op(page);
  expect("h1500 opens the height box without switching the view", o.heightEntry === "1500", o);
  await page.waitForTimeout(300);
  const box = await page.evaluate(() => document.querySelector('[data-testid="typed-box"]')?.textContent ?? null);
  expect("the height box shows the typed value", !!box && box.includes("1500") && box.includes("Enter sets the height"), box);
  await shot("p04-height-typed");
  await press(page, "Enter");
  const opts = await page.evaluate(() => window.__app.getState().toolOptions.pipeElevationMm);
  const view = await page.evaluate(() => window.__app.getState().viewMode);
  expect("Enter set the start height to 1500", opts === 1500, opts);
  expect("the 1 and 5 keys did not change the view mode", view === "2d", view);
  await clickAt(page, 2500, 1500);
  await clickAt(page, 2500, 2500);
  await press(page, "PageDown", 2);
  o = await op(page);
  expect("a PageDown pending riser", o.penZ === 1300 && o.points.length === 2, o);
  await press(page, "Escape");
  o = await op(page);
  expect("Escape drops the pending riser first", o.kind === "pipe" && o.penZ === 1500 && o.points.length === 2, o);
  await press(page, "Escape");
  o = await op(page);
  expect("then the last point", o.kind === "pipe" && o.points.length === 1, o);
  await press(page, "Escape");
  o = await op(page);
  expect("then the run", o.kind === "idle", o.kind);
  await setOptions(page, { pipeElevationMm: null });

  // ------------------------------------------------------------ drainage falling as drawn
  // One zoom step out, so the run out of the house stays in view.
  await moveTo(page, 4000, 2000);
  await page.evaluate(() => window.__planController.zoomStep(-1));
  await page.waitForTimeout(300);
  await page.selectOption('[data-testid="pipe-system"]', "drainage");
  await setOptions(page, { pipeDiameterMm: 100 });
  await moveTo(page, 7560, 1510);
  await page.keyboard.type("h0");
  await press(page, "Enter");
  o = await op(page);
  log("drain idle over the WC center", o);
  expect("the WC center is a snap target", o.snap?.type === "fixture" && o.snap.x === 7575 && o.snap.y === 1500, o.snap);
  await clickAt(page, 7560, 1510);
  await press(page, "PageDown", 3);
  await moveTo(page, 5600, 1520);
  await clickAt(page, 5600, 1520);
  await moveTo(page, 5610, -1000);
  await shot("p05-drain-rubber-band-fall");
  await clickAt(page, 5610, -1000);
  o = await op(page);
  log("drain run", o.points);
  const fall = (a, b) => ((a[2] - b[2]) / Math.hypot(b[0] - a[0], b[1] - a[1])) * 100;
  expect(
    "a drop at the WC, then 1% fall per segment for 100 mm",
    o.points.length === 4 && JSON.stringify(o.points.slice(0, 2)) === JSON.stringify([[7575, 1500, 0], [7575, 1500, -300]]) && Math.abs(fall(o.points[1], o.points[2]) - 1) < 0.01 && Math.abs(fall(o.points[2], o.points[3]) - 1) < 0.01,
    o.points,
  );
  await moveTo(page, 3000, -990);
  await shot("p06-drain-fall-labels");
  rev = await revision(page);
  await press(page, "Enter");
  await waitRevision(page, rev);

  // Lavatory waste, 50 mm at 2 percent, teed into the WC drain.
  await setOptions(page, { pipeDiameterMm: 50, pipeElevationMm: -300 });
  await moveTo(page, 7920, 2610);
  o = await op(page);
  expect("the lavatory back is a snap target", o.snap?.type === "fixture" && /Lavatory, back/.test(o.snap.label ?? ""), o.snap);
  await clickAt(page, 7920, 2610);
  await moveTo(page, 6500, 2600);
  await clickAt(page, 6500, 2600);
  await moveTo(page, 6490, 1520);
  await shot("p07b-drain-branch-arrives-low");
  // Started at -300 the branch arrives below the main: its last segment would
  // rise. Step back out of the run and start it higher.
  await press(page, "Escape", 3);
  o = await op(page);
  expect("Escape steps all the way out of the run", o.kind === "idle", o.kind);
  await setOptions(page, { pipeElevationMm: -150 });
  await clickAt(page, 7920, 2610);
  await moveTo(page, 6500, 2600);
  await clickAt(page, 6500, 2600);
  await moveTo(page, 6490, 1520);
  o = await op(page);
  log("drain tee snap", o.snap);
  const drainRun = (await pipes(page)).find((p) => p.system === "drainage");
  const [q1, q2] = [drainRun.points[1], drainRun.points[2]];
  const zAt = q1[2] + ((q2[2] - q1[2]) * (q1[0] - (o.snap?.x ?? 0))) / (q1[0] - q2[0]);
  expect("tee snap on the WC drain inherits its height there", o.snap?.type === "tee" && o.snap.y === 1500 && Math.abs(o.snap.z - zAt) < 0.2, { snap: o.snap, zAt });
  await shot("p07-drain-tee-snap");
  await clickAt(page, 6490, 1520);
  rev = await revision(page);
  await press(page, "Enter");
  await waitRevision(page, rev);
  list = await pipes(page);
  const lav = list.find((p) => p.system === "drainage" && p.d === 50);
  log("lavatory waste", lav);
  expect("the branch ends on the WC drain", !!lav && lav.points[lav.points.length - 1][1] === 1500, lav);
  const lastTwo = lav.points.slice(-2);
  expect("started higher, the branch falls to the tee and drops into the main there", lastTwo[0][0] === lastTwo[1][0] && lastTwo[0][1] === lastTwo[1][1] && lastTwo[0][2] > lastTwo[1][2], lav.points);

  // ------------------------------------------------------------ cold water tee
  await page.selectOption('[data-testid="pipe-system"]', "cold_water");
  await setOptions(page, { pipeElevationMm: 500 });
  await moveTo(page, 7920, 2400);
  await clickAt(page, 7920, 2400);
  await moveTo(page, 4830, 2410);
  o = await op(page);
  log("cold tee snap", o.snap);
  expect("tee snap on the cold main at its height 1200, on the straight line", o.snap?.type === "tee" && o.snap.x === 4800 && o.snap.y === 2400 && o.snap.z === 1200, o.snap);
  await shot("p08-cold-tee-snap");
  await clickAt(page, 4830, 2410);
  o = await op(page);
  expect("the branch climbs to the main with a riser at the tee", JSON.stringify(o.points.slice(-2)) === JSON.stringify([[4800, 2400, 500], [4800, 2400, 1200]]), o.points);
  rev = await revision(page);
  await press(page, "Enter");
  await waitRevision(page, rev);
  // ------------------------------------------------------------ vent: typed length, double click
  await page.selectOption('[data-testid="pipe-system"]', "vent");
  await moveTo(page, 7915, 2610);
  o = await op(page);
  expect("a vent snaps to the end of the lavatory waste (drainage and vent join)", o.snap?.type === "pipe_end" && o.snap.z === -150, o.snap);
  await clickAt(page, 7915, 2610);
  await page.keyboard.type("h2700");
  await press(page, "Enter");
  await moveTo(page, 7925, 3300);
  await page.keyboard.type("800");
  await press(page, "Enter");
  o = await op(page);
  expect("h2700 then a typed 800 adds a riser and a point 800 mm north", JSON.stringify(o.points) === JSON.stringify([[7925, 2600, -150], [7925, 2600, 2700], [7925, 3400, 2700]]), o.points);
  rev = await revision(page);
  const vEnd = await at(page, 7925, 3400);
  await page.mouse.dblclick(vEnd.x, vEnd.y);
  await waitRevision(page, rev);
  list = await pipes(page);
  const vent = list.find((p) => p.system === "vent");
  expect("a double click finishes the run", !!vent && vent.points.length === 3, vent ?? list);

  const fittings = await page.evaluate(() => window.__app.getState().doc.derived.pipes?.fittings ?? []);
  log("derived fittings", fittings.map((f) => ({ kind: f.kind, x: Math.round(f.position.x), y: Math.round(f.position.y), z: Math.round(f.position.z) })));
  await tool(page, "select");
  await page.mouse.move(40, 400);
  await page.waitForTimeout(350);
  await shot("p09-network");

  // ------------------------------------------------------------ select, heights, drag a node
  await clickAt(page, 4805, 1500);
  let sel = await page.evaluate(() => window.__app.getState().selection);
  expect("a click near the centerline selects the cold run", sel.length === 1 && sel[0] === cold.id, sel);
  await page.mouse.move(40, 400);
  await page.waitForTimeout(400);
  await shot("p10-selected-heights");
  await moveTo(page, 3500, 75, 3);
  await page.waitForTimeout(200);
  const hov = await page.evaluate(() => ({ grip: window.__planController.hoverGrip, cursor: window.__planController.getUi().cursor }));
  expect("hovering a node handle grows it and shows a grab cursor", hov.grip === 0 && hov.cursor === "grab", hov);
  await dragTo(page, [3500, 75], [3500, 600], { beforeUp: () => shot("p11-node-drag") });
  await page.waitForTimeout(300);
  list = await pipes(page);
  const moved = list.find((p) => p.id === cold.id);
  expect(
    "dragging the end node moved only that point, in plan, height kept",
    JSON.stringify(moved.points[0]) === JSON.stringify([3500, 600, 300]) && JSON.stringify(moved.points.slice(1)) === JSON.stringify(cold.points.slice(1)),
    moved.points,
  );
  await dragTo(page, [4800, 75], [4600, 400]);
  await page.waitForTimeout(300);
  list = await pipes(page);
  const moved2 = list.find((p) => p.id === cold.id);
  log("after the riser drag", moved2.points);
  expect(
    "dragging the riser node moves both its points together: it stays vertical, the rest stays",
    moved2.points.length === 4 &&
      moved2.points[1][0] === moved2.points[2][0] && moved2.points[1][1] === moved2.points[2][1] && moved2.points[1][0] !== 4800 &&
      moved2.points[1][2] === 300 && moved2.points[2][2] === 1200 &&
      JSON.stringify(moved2.points[0]) === JSON.stringify(moved.points[0]) && JSON.stringify(moved2.points[3]) === JSON.stringify(moved.points[3]),
    moved2.points,
  );

  // Marquee window around the lavatory waste only.
  await page.mouse.move(40, 400);
  const a = await at(page, 6350, 2750);
  const b = await at(page, 8050, 1400);
  await page.mouse.move(a.x, a.y);
  await page.mouse.down();
  await page.mouse.move(b.x, b.y, { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(200);
  sel = await page.evaluate(() => window.__app.getState().selection);
  const selKinds = await page.evaluate((ids) => ids.map((id) => window.__app.getState().doc.project.elements.find((e) => e.id === id)?.kind), sel);
  log("window marquee selection", selKinds);
  expect("a window marquee selects the lavatory waste", sel.includes(lav.id), sel);

  // ------------------------------------------------------------ hidden and locked layers
  await page.evaluate(() => window.__app.getState().select([]));
  const setLayer = (key, patch) =>
    page.evaluate(
      ([k, p]) => {
        const l = window.__app.getState().doc.project.layers.find((x) => x.key === k);
        return window.__app.getState().dispatch({ type: "set_layer", layer: { ...l, ...p } });
      },
      [key, patch],
    );
  await setLayer("cold_water", { visible: false });
  await setLayer("drainage", { locked: true });
  await page.waitForTimeout(400);
  // The drain outside the house, where nothing else is under it.
  await moveTo(page, 5580, -600);
  await page.waitForTimeout(250);
  const hover = await page.evaluate(() => window.__app.getState().hoverId);
  expect("a locked drain is not hovered", hover === null, hover);
  await clickAt(page, 5580, -600);
  sel = await page.evaluate(() => window.__app.getState().selection);
  expect("a locked drain is not selected by a click", sel.length === 0, sel);
  await clickAt(page, 4805, 1500);
  sel = await page.evaluate(() => window.__app.getState().selection);
  expect("a hidden cold run cannot be picked", !sel.includes(cold.id), sel);
  const idx = await page.evaluate(() => {
    const c = window.__planController;
    return { visible: c.index.visible.filter((e) => e.kind === "pipe").map((e) => e.system) };
  });
  expect("hidden cold water is not in the drawn set, locked drainage is", !idx.visible.includes("cold_water") && idx.visible.includes("drainage"), idx);
  await page.selectOption('[data-testid="pipe-system"]', "drainage");
  await moveTo(page, 3000, 4000);
  o = await op(page);
  expect("the pipe tool says the drainage layer is locked", /Drainage layer is locked/.test(o.hint ?? ""), o.hint);
  await clickAt(page, 3000, 4000);
  o = await op(page);
  expect("and does not start a run there", o.kind === "idle", o.kind);
  await shot("p12-hidden-cold-locked-drain");
  await tool(page, "select");
  await setLayer("cold_water", { visible: true });
  await setLayer("drainage", { locked: false });
  await page.mouse.move(40, 400);
  await page.waitForTimeout(400);
  await shot("p13-layers-back");

  // ------------------------------------------------------------ generic edits on a pipe
  const byId = async (id) => (await pipes(page)).find((p) => p.id === id);
  const ventBefore = await byId(vent.id);
  await dragTo(page, [7925, 3000], [7425, 3000]);
  await page.waitForTimeout(300);
  const ventMoved = await byId(vent.id);
  expect(
    "dragging a pipe body moves the whole run in plan, heights kept",
    ventMoved.points.every((q, i) => q[0] === ventBefore.points[i][0] - 500 && q[1] === ventBefore.points[i][1] && q[2] === ventBefore.points[i][2]),
    ventMoved.points,
  );
  const count0 = (await pipes(page)).length;
  const a0 = await at(page, 7425, 3000);
  const a1 = await at(page, 7425, 3800);
  await page.mouse.move(a0.x, a0.y, { steps: 3 });
  await page.keyboard.down("Alt");
  await page.mouse.down();
  await page.mouse.move(a1.x, a1.y, { steps: 6 });
  await page.mouse.up();
  await page.keyboard.up("Alt");
  await page.waitForTimeout(400);
  const afterDup = await pipes(page);
  expect("Alt drag duplicates a pipe", afterDup.length === count0 + 1, afterDup.length);
  const copy = afterDup.find((p) => !list.some((q) => q.id === p.id) && p.id !== vent.id && p.system === "vent");
  const rotRev = await revision(page);
  await page.evaluate((id) => void window.__app.getState().dispatch({ type: "rotate_elements", ids: [id], pivot: { x: 7425, y: 3000 }, angle_deg: 90 }), vent.id);
  const flash = await page
    .waitForFunction((id) => window.__planController.animSnapshot()[`fl:${id}`] !== undefined, vent.id, { timeout: 3000, polling: 10 })
    .then(() => true)
    .catch(() => false);
  await waitRevision(page, rotRev);
  const ventTurned = await byId(vent.id);
  log("vent after a 90 degree turn about (7425, 3000)", ventTurned.points);
  expect(
    "a pipe turns about the pivot in plan, heights kept",
    JSON.stringify(ventTurned.points) === JSON.stringify([[7825, 3000, -150], [7825, 3000, 2700], [7025, 3000, 2700]]),
    ventTurned.points,
  );
  expect("a turned pipe flashes once as it changes", flash, flash);
  if (copy) {
    await page.evaluate((id) => window.__app.getState().dispatch({ type: "delete_elements", ids: [id] }), copy.id);
    await page.waitForFunction((id) => window.__planController.animSnapshot()[`rm:${id}`] !== undefined, copy.id, { timeout: 3000 }).catch(() => {});
    const fade = await page.evaluate((id) => window.__planController.animSnapshot()[`rm:${id}`], copy.id);
    expect("a deleted pipe fades out", fade !== undefined, fade);
  }
  await page.waitForTimeout(400);
  await shot("p14-moved-turned");

  // An AI style preview adding a pipe draws it in the preview color.
  const preview = await page.evaluate(async (lv) => {
    const res = await fetch("http://localhost:1741/ipc/doc_preview", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ command: { type: "add_element", element: { kind: "pipe", id: "", level_id: lv, system: "hot_water", material: "ppr", diameter_mm: 20, points: [{ x: 1000, y: 5000, z: 300 }, { x: 4000, y: 5000, z: 300 }, { x: 4000, y: 5000, z: 1200 }], name: "" } } }),
    });
    const result = await res.json();
    window.__app.getState().setPreview(result);
    return result.diff;
  }, level);
  await page.waitForTimeout(500);
  log("preview diff", preview);
  expect("the previewed pipe is in the preview diff", preview.added.length === 1, preview);
  await shot("p15-ai-preview-ghost");
  await page.evaluate(() => window.__app.getState().setPreview(null));
};
