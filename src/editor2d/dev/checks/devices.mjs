// Bridge, sample bungalow: devices and service runs through the canvas.
// A switch placed by the latch-side guide of the front door, two ceiling
// lights linked to it with the link tool, a second switch at the bedroom door
// linked to one of them (a 3-way), a split aircon indoor unit on a wall with
// its outlet, a refrigerant line set and a condensate run, and a crowded
// drainage run whose tags must not overlap each other or dimension text.
import { at, clickAt, expect, log, moveTo, ready, tool } from "./lib.mjs";

const doc = (page) => page.evaluate(() => window.__app.getState().doc);
const revision = (page) => page.evaluate(() => window.__app.getState().doc.revision);

async function waitRevision(page, rev) {
  await page.waitForFunction((r) => window.__app.getState().doc.revision > r, rev, { timeout: 5000 }).catch(() => {});
  await page.waitForTimeout(300);
}

const assets = (page, key) =>
  page.evaluate(
    (k) =>
      window.__app
        .getState()
        .doc.project.elements.filter((e) => e.kind === "asset" && (!k || e.catalog_key === k))
        .map((a) => ({ id: a.id, key: a.catalog_key, x: +a.position.x.toFixed(1), y: +a.position.y.toFixed(1), rot: +a.rotation_deg.toFixed(1), elev: +a.elevation_mm.toFixed(1), links: a.links })),
    key,
  );

const ghost = (page) =>
  page.evaluate(() => {
    const c = window.__planController;
    const g = c.placementGhost;
    if (!g) return null;
    const e = g.element;
    return {
      x: +e.position.x.toFixed(1),
      y: +e.position.y.toFixed(1),
      rot: +e.rotation_deg.toFixed(1),
      elev: e.elevation_mm,
      mount: g.mount ? { kind: g.mount.kind, valid: g.mount.valid, reason: g.mount.reason, height: g.mount.heightLabel, guide: g.mount.guide ? { x: +g.mount.guide.point.x.toFixed(1), y: +g.mount.guide.point.y.toFixed(1), snapped: g.mount.guide.snapped } : null } : null,
      hint: c.getUi().hint,
      snap: c.snapResult ? { type: c.snapResult.type, label: c.snapResult.label ?? null } : null,
    };
  });

/** The plan point where an object's symbol takes links (and a click). */
const anchor = (page, id) =>
  page.evaluate((i) => {
    const c = window.__planController;
    const el = window.__app.getState().doc.project.elements.find((e) => e.id === i);
    const p = c.anchorOf(el);
    return { x: p.x, y: p.y };
  }, id);

async function clickDevice(page, id) {
  const p = await anchor(page, id);
  await clickAt(page, p.x, p.y);
}

async function pickAsset(page, key) {
  await tool(page, "asset");
  await page.selectOption('[data-testid="asset-key"]', key);
  await page.waitForTimeout(120);
}

async function place(page, x, y) {
  const rev = await revision(page);
  await clickAt(page, x, y);
  await waitRevision(page, rev);
}

async function press(page, key, times = 1) {
  for (let i = 0; i < times; i++) {
    await page.keyboard.press(key);
    await page.waitForTimeout(25);
  }
  await page.waitForTimeout(60);
}

const view = (page, rect) =>
  page.evaluate((r) => {
    const c = window.__planController;
    c.focusElements([], false);
    const w = c.width;
    const h = c.height;
    const scale = Math.min((w - 120) / (r.maxX - r.minX), (h - 120) / (r.maxY - r.minY));
    c.view = { scale, ox: w / 2 - ((r.minX + r.maxX) / 2) * scale, oy: h / 2 + ((r.minY + r.maxY) / 2) * scale };
    c.autoFit = false;
    c.invalidate();
  }, rect);

export default async (page, shot) => {
  await ready(page, "bridge");
  await page.waitForFunction(() => window.__app.getState().catalog.length > 20, null, { timeout: 8000 });
  const d0 = await doc(page);
  log("catalog has devices", (await page.evaluate(() => window.__app.getState().catalog.filter((c) => c.device).map((c) => c.key))).length);

  // ------------------------------------------------------------ 1. a switch by the latch-side guide
  // Front door: south wall, offset 1500, 900 wide, hinge at x 1050, latch at x 1950. Inside face y 75.
  await view(page, { minX: 600, minY: -700, maxX: 3600, maxY: 1600 });
  await pickAsset(page, "switch-1");
  await moveTo(page, 2600, 400);
  let g = await ghost(page);
  log("switch away from the guide", g);
  expect("a switch ghost mounts on the wall face at its catalog height", g?.mount?.kind === "wall" && g.mount.valid && Math.abs(g.y - 95) < 0.6 && g.elev === 1143, g);
  expect("the latch guide of the nearest door is offered on the room side, 200 from the latch jamb", g?.mount?.guide && g.mount.guide.x === 2150 && g.mount.guide.y === 75 && !g.mount.guide.snapped, g?.mount?.guide);
  expect("the mounting height shows near the cursor", g?.mount?.height === "Center +1200 mm", g?.mount?.height);
  await shot("d01-switch-guide-offered");
  await moveTo(page, 2190, 300);
  g = await ghost(page);
  log("switch near the guide", g);
  expect("near the guide the switch snaps to it", g?.mount?.guide?.snapped === true && g.x === 2150 && Math.abs(g.y - 95) < 0.6 && g.snap?.label === "Latch side", g);
  await page.waitForTimeout(250);
  await shot("d02-switch-at-latch-guide");
  await place(page, 2190, 300);
  let sw = await assets(page, "switch-1");
  expect("a click places the switch at the guide", sw.length === 1 && sw[0].x === 2150 && Math.abs(sw[0].y - 95) < 0.6 && sw[0].elev === 1143 && sw[0].rot === 180, sw);
  const switchA = sw[0].id;

  // ------------------------------------------------------------ 2. two ceiling lights, linked with the link tool
  await view(page, { minX: -200, minY: -400, maxX: 5200, maxY: 6300 });
  await pickAsset(page, "light-ceiling");
  await moveTo(page, 2540, 2990);
  g = await ghost(page);
  log("ceiling light at the room middle", g);
  expect("a ceiling light snaps to the middle of the room and hangs from the level height", g?.mount?.kind === "ceiling" && g.x === 2512.5 && g.y === 3000 && g.elev === 2940 && g.snap?.label === "Room center", g);
  await place(page, 2540, 2990);
  await place(page, 2500, 1300);
  const lights = await assets(page, "light-ceiling");
  expect("two ceiling lights placed", lights.length === 2, lights);

  await tool(page, "link");
  await page.waitForTimeout(100);
  let hint = await page.evaluate(() => window.__planController.getUi().hint);
  expect("the link tool asks for a switch first", /Click a switch/.test(hint ?? ""), hint);
  await clickDevice(page, switchA);
  const source = await page.evaluate(() => window.__planController.linkSource());
  expect("clicking the switch picks it", source === switchA, source);
  // Record the commands the link tool sends.
  await page.evaluate(() => {
    window.__sent = [];
    const st = window.__app.getState();
    const orig = st.dispatch;
    window.__app.setState({
      dispatch: async (cmd) => {
        window.__sent.push(cmd);
        return orig(cmd);
      },
    });
  });
  let rev = await revision(page);
  await moveTo(page, lights[0].x + 20, lights[0].y + 10);
  await page.waitForTimeout(200);
  const pills = await page.evaluate(() => window.__app.getState().hoverId);
  expect("hovering a light with the switch picked previews the link", pills === lights[0].id, pills);
  await shot("d03-link-hover-light");
  await clickAt(page, lights[0].x + 20, lights[0].y + 10);
  const grew = await page
    .waitForFunction(() => Object.keys(window.__planController.animSnapshot()).some((k) => k.startsWith("lnk:")), null, { timeout: 3000, polling: 10 })
    .then(() => true)
    .catch(() => false);
  expect("a new link draws itself in", grew, grew);
  await waitRevision(page, rev);
  rev = await revision(page);
  await clickAt(page, lights[1].x - 10, lights[1].y + 20);
  await waitRevision(page, rev);
  const sent = await page.evaluate(() => window.__sent.map((c) => ({ type: c.type, id: c.element?.id, links: c.element?.links })));
  log("commands sent", sent);
  expect(
    "each light clicked sends one UpdateElement on the switch's links",
    sent.length === 2 && sent.every((c) => c.type === "update_element" && c.id === switchA) && sent[1].links.length === 2,
    sent,
  );
  sw = await assets(page, "switch-1");
  expect("the switch links both lights", JSON.stringify([...sw[0].links].sort()) === JSON.stringify(lights.map((l) => l.id).sort()), sw[0].links);
  await moveTo(page, 3600, 4800);
  await page.waitForTimeout(350);
  const arcs = await page.evaluate(() => window.__planController.linkDrawList().map((l) => ({ key: l.key, strong: l.strong, handle: !!l.handle })));
  expect("both links draw as arcs with a flip handle", arcs.filter((a) => a.strong && a.handle).length === 2, arcs);
  await shot("d04-switch-linked-to-two-lights");

  // Flip one bow with its handle: view only, no command.
  const before = await page.evaluate(() => window.__planController.linkDrawList()[0]);
  rev = await revision(page);
  const hp = await at(page, before.handle.x, before.handle.y);
  await page.mouse.move(hp.x, hp.y, { steps: 4 });
  await page.waitForTimeout(150);
  const handleHover = await page.evaluate((k) => ({ key: window.__planController.hoverHandle, grow: window.__planController.animValue(`bowh:${k}`, 0), cursor: window.__planController.getUi().cursor }), before.key);
  expect("the flip handle grows under the pointer", handleHover.key === before.key && handleHover.grow > 0.5 && handleHover.cursor === "pointer", handleHover);
  await page.mouse.click(hp.x, hp.y);
  const swept = await page
    .waitForFunction((k) => { const v = window.__planController.animSnapshot()[`bow:${k}`]; return v !== undefined && Math.abs(v) < 0.99; }, before.key, { timeout: 2000, polling: 5 })
    .then(() => true)
    .catch(() => false);
  expect("a flip sweeps the curve through the straight line", swept, swept);
  await page.waitForTimeout(400);
  const after = await page.evaluate((k) => window.__planController.linkDrawList().find((l) => l.key === k), before.key);
  const rev2 = await revision(page);
  expect("the flip handle sends the bow to the other side, without a command", Math.sign(after.bow) === -Math.sign(before.bow) && rev2 === rev, { before: before.bow, after: after.bow });
  await page.keyboard.press("Escape");
  await page.waitForTimeout(100);
  const ended = await page.evaluate(() => ({ op: window.__planController.op.kind, sel: window.__app.getState().selection.length }));
  expect("Escape ends linking", ended.op === "idle" && ended.sel === 0, ended);

  // ------------------------------------------------------------ 3. a 3-way: a second switch at the bedroom door
  // Bedroom door: wall x 5000 (100 thick), offset 3000, 800 wide: latch at y 3400, guide y 3600 on both faces.
  await pickAsset(page, "switch-1");
  await moveTo(page, 4800, 3650);
  g = await ghost(page);
  expect("the second switch snaps by the bedroom door's latch jamb", g?.mount?.guide?.snapped && g.mount.guide.y === 3600 && g.mount.guide.x === 4950, g);
  await place(page, 4800, 3650);
  sw = await assets(page, "switch-1");
  const switchB = sw.find((s) => s.id !== switchA);
  expect("second switch placed", !!switchB, sw);
  await tool(page, "link");
  await clickDevice(page, switchB.id);
  rev = await revision(page);
  await clickAt(page, lights[0].x, lights[0].y);
  await waitRevision(page, rev);
  // The inspector's "link from here" only selects: the link tool follows the selection.
  await page.evaluate((id) => window.__app.getState().select([id]), switchA);
  await page.waitForTimeout(100);
  const followed = await page.evaluate(() => window.__planController.linkSource());
  expect("while the link tool is on, selecting a device picks it", followed === switchA, followed);
  const threeWay = await page.evaluate(() => [...window.__planController.devices().threeWay]);
  expect("two switches on one light are a 3-way pair", threeWay.length === 2 && threeWay.includes(switchB.id), threeWay);
  await page.keyboard.press("Escape");
  await page.evaluate((id) => window.__app.getState().select([id]), lights[0].id);
  await tool(page, "select");
  await moveTo(page, 3000, 5200);
  await page.waitForTimeout(400);
  const lightLinks = await page.evaluate(() => window.__planController.linkDrawList().filter((l) => l.strong).length);
  expect("a selected light shows both of its switches", lightLinks === 2, lightLinks);
  await view(page, { minX: 1500, minY: -200, maxX: 5300, maxY: 4000 });
  await page.waitForTimeout(300);
  await shot("d05-three-way-S3");

  // ------------------------------------------------------------ 4. a split aircon indoor unit on a wall, and its outlet
  // Bedroom north wall: inner face y 5925, outer face y 6075.
  await page.evaluate(() => window.__app.getState().select([]));
  await view(page, { minX: 5000, minY: 4400, maxX: 8600, maxY: 7600 });
  await pickAsset(page, "aircon-indoor-1hp");
  await moveTo(page, 6520, 5500);
  g = await ghost(page);
  log("indoor unit ghost", g);
  expect("a split indoor unit hangs with its back on the wall face at 2300", g?.mount?.kind === "wall" && g.mount.valid && g.x === 6500 && Math.abs(g.y - (5925 - 115)) < 0.6 && g.rot === 0 && g.elev === 2300, g);
  expect("its underside height shows near the cursor", g?.mount?.height === "Underside +2300 mm", g?.mount?.height);
  await page.waitForTimeout(200);
  await shot("d06-aircon-indoor-on-wall");
  await place(page, 6520, 5500);
  const unit = (await assets(page, "aircon-indoor-1hp"))[0];
  expect("the indoor unit is placed", !!unit && unit.elev === 2300, unit);
  await pickAsset(page, "outlet-aircon");
  await moveTo(page, 7250, 5700);
  g = await ghost(page);
  expect("the aircon outlet mounts on the same face at its height", g?.mount?.valid && Math.abs(g.y - 5905) < 0.6 && g.mount.height === "Center +2000 mm", g);
  await place(page, 7250, 5700);
  const outlet = (await assets(page, "outlet-aircon"))[0];
  await tool(page, "link");
  await clickDevice(page, outlet.id);
  rev = await revision(page);
  await clickDevice(page, unit.id);
  await waitRevision(page, rev);
  const fed = (await assets(page, "outlet-aircon"))[0];
  expect("the link tool links the aircon outlet to its unit", JSON.stringify(fed.links) === JSON.stringify([unit.id]), fed.links);
  await page.keyboard.press("Escape");
  // The outdoor unit sits outside, its back on the outer face (a floor object snaps to faces too).
  await pickAsset(page, "aircon-outdoor-1hp");
  await place(page, 7500, 6450);
  const cu = (await assets(page, "aircon-outdoor-1hp"))[0];
  log("outdoor unit", cu);

  // ------------------------------------------------------------ 5. a refrigerant line set and a condensate run
  const pipes = () =>
    page.evaluate(() =>
      window.__app
        .getState()
        .doc.project.elements.filter((e) => e.kind === "pipe")
        .map((p) => ({ id: p.id, system: p.system, d: p.diameter_mm, material: p.material, points: p.points.map((v) => [Math.round(v.x), Math.round(v.y), +v.z.toFixed(1)]) })),
    );
  await tool(page, "pipe");
  await page.selectOption('[data-testid="pipe-system"]', "refrigerant");
  // The 12.7 gas line of a 2 HP set: the v3 bridge still refuses sizes under 10 mm (the engine source allows 6).
  await page.evaluate(() => window.__app.getState().setTool("pipe", { pipeDiameterMm: 12.7 }));
  await page.waitForTimeout(100);
  await moveTo(page, unit.x + 30, 5925);
  const snapAtUnit = await page.evaluate(() => {
    const r = window.__planController.snapResult;
    return r ? { type: r.type, label: r.label ?? null, x: Math.round(r.point.x), y: Math.round(r.point.y) } : null;
  });
  expect("a line set starts at the indoor unit (its back on the wall)", snapAtUnit?.type === "fixture" && snapAtUnit.x === 6500 && snapAtUnit.y === 5925, snapAtUnit);
  await clickAt(page, unit.x + 30, 5925);
  await clickAt(page, 6500, 6800);
  await clickAt(page, cu.x, 6800);
  await press(page, "PageDown", 20);
  await moveTo(page, cu.x + 20, cu.y + 20);
  await page.waitForTimeout(150);
  await shot("d07-lineset-drawing");
  await clickAt(page, cu.x + 20, cu.y + 20);
  rev = await revision(page);
  await page.keyboard.press("Enter");
  await waitRevision(page, rev);
  let list = await pipes(page);
  const lineset = list.find((p) => p.system === "refrigerant");
  log("line set", lineset);
  expect("the line set is a copper refrigerant run from the unit to the outdoor unit", !!lineset && lineset.material === "copper" && lineset.d === 12.7 && lineset.points[0][2] === 2400 && lineset.points[lineset.points.length - 1][2] === 400, lineset);

  await page.selectOption('[data-testid="pipe-system"]', "condensate");
  await page.waitForTimeout(100);
  await clickAt(page, 6250, 5925);
  await clickAt(page, 6250, 7000);
  rev = await revision(page);
  await page.keyboard.press("Enter");
  await waitRevision(page, rev);
  list = await pipes(page);
  const cond = list.find((p) => p.system === "condensate");
  log("condensate", cond);
  expect("the condensate run falls at the drain default as it is drawn", !!cond && cond.points[0][2] === 2300 && cond.points[1][2] < 2300 && Math.abs((2300 - cond.points[1][2]) / (7000 - 5925) - 0.02) < 0.001, cond);
  await tool(page, "select");
  await page.evaluate((id) => window.__app.getState().select([id]), cond.id);
  await moveTo(page, 8400, 4600);
  await page.waitForTimeout(400);
  const condTags = await page.evaluate(() => window.__planController.tagLayout);
  expect("a selected condensate run shows its fall and heights", !!condTags && condTags.placed.some((t) => t.id.includes(":f:")) && condTags.placed.some((t) => t.id.includes(":h:")), condTags?.placed.map((t) => t.id));
  await shot("d08-lineset-and-condensate");
  await page.evaluate((id) => window.__app.getState().select([id]), lineset.id);
  await page.waitForTimeout(300);
  await shot("d09-lineset-selected");

  // ------------------------------------------------------------ 6. hidden and locked layers hold the devices
  const setLayer = (key, visible, locked) =>
    page.evaluate(([k, v, l]) => window.__app.getState().dispatch({ type: "set_layer", layer: { key: k, visible: v, locked: l } }), [key, visible, locked]);
  await setLayer("electrical", false, false);
  await page.waitForTimeout(300);
  let vis = await page.evaluate(([a, b]) => ({ sw: window.__planController.index.visibleIds.has(a), light: window.__planController.index.visibleIds.has(b) }), [switchA, lights[0].id]);
  expect("hiding the electrical layer hides switches and lights", !vis.sw && !vis.light, vis);
  await page.evaluate((id) => window.__app.getState().select([id]), switchA);
  await page.waitForTimeout(200);
  const hiddenLinks = await page.evaluate(() => window.__planController.linkDrawList().length);
  expect("links follow the electrical layer's visibility", hiddenLinks === 0, hiddenLinks);
  await setLayer("electrical", true, true);
  await page.evaluate(() => window.__app.getState().select([]));
  await view(page, { minX: -200, minY: -400, maxX: 5200, maxY: 6300 });
  await page.waitForTimeout(300);
  await tool(page, "link");
  await clickDevice(page, switchA);
  rev = await revision(page);
  await clickDevice(page, lights[0].id);
  await page.waitForTimeout(300);
  const lockedTry = await page.evaluate(() => ({ notice: window.__planController.linkNotice, rev: window.__app.getState().doc.revision }));
  expect("a locked electrical layer refuses a link change and says why", lockedTry.rev === rev && /locked/.test(lockedTry.notice ?? ""), lockedTry);
  await page.keyboard.press("Escape");
  await tool(page, "select");
  const pick = await anchor(page, switchA);
  await moveTo(page, pick.x, pick.y);
  const hoverLocked = await page.evaluate(() => window.__app.getState().hoverId);
  expect("devices on a locked layer are not picked", hoverLocked !== switchA, hoverLocked);
  await setLayer("electrical", true, false);
  await page.waitForTimeout(200);

  // ------------------------------------------------------------ 6b. a window aircon in the bedroom's east window
  // East wall x 8000, casement window offset 3000, 1200 wide: y 2400 to 3600, sill 900.
  await view(page, { minX: 5600, minY: 1400, maxX: 9400, maxY: 4600 });
  await pickAsset(page, "aircon-window");
  await moveTo(page, 7700, 3040);
  g = await ghost(page);
  log("window aircon ghost", g);
  expect("a window aircon sets into the window, across the wall, centered, back to the outside", g?.mount?.kind === "opening" && g.mount.valid && g.x === 8000 && g.y === 3000 && g.rot === 270 && g.snap?.label === "Centered", g);
  await page.waitForTimeout(200);
  await shot("d12-window-aircon");
  await moveTo(page, 6500, 2000);
  g = await ghost(page);
  expect("away from a window it cannot be placed, and says why", g?.mount?.valid === false && g.mount.reason === "Move onto a window", g?.mount);
  rev = await revision(page);
  const bumped = page
    .waitForFunction(() => window.__planController.animSnapshot()["place.no"] !== undefined, null, { timeout: 3000, polling: 5 })
    .then(() => true)
    .catch(() => false);
  await clickAt(page, 6500, 2000);
  const refused = { rev: await revision(page), bump: await bumped };
  expect("a click there places nothing and the reason bumps", refused.rev === rev && refused.bump, refused);
  await place(page, 7700, 3040);
  const wac = (await assets(page, "aircon-window"))[0];
  expect("the window aircon is placed at its catalog height inside the opening", !!wac && wac.x === 8000 && wac.y === 3000 && wac.elev === 1200, wac);

  // ------------------------------------------------------------ 6c. a device in an AI proposal draws in the preview color
  await tool(page, "select");
  const previewDiff = await page.evaluate(async (lv) => {
    const result = await window.__ipc.docPreview({
      type: "add_element",
      element: { kind: "asset", id: "", level_id: lv, catalog_key: "outlet-duplex", name: "Convenience outlet, duplex", category: "electrical", position: { x: 6200, y: 95 }, rotation_deg: 180, width_mm: 70, depth_mm: 40, height_mm: 115, elevation_mm: 243, light: null, links: [], circuit: "" },
    });
    window.__app.getState().setPreview(result);
    return result.diff;
  }, d0.project.levels[0].id);
  await view(page, { minX: 5200, minY: -500, maxX: 7300, maxY: 1300 });
  await page.waitForTimeout(500);
  expect("the proposed outlet is in the preview", previewDiff.added.length === 1, previewDiff);
  await shot("d13-ai-preview-outlet");
  await page.evaluate(() => window.__app.getState().setPreview(null));
  await page.waitForTimeout(200);
  // A proposal that links the bedroom door switch to the second light shows its links without a selection.
  await page.evaluate(() => window.__app.getState().select([]));
  const proposed = await page.evaluate(async ([sid, l0, l1]) => {
    const el = window.__app.getState().doc.project.elements.find((e) => e.id === sid);
    const before = window.__planController.linkDrawList().length;
    const result = await window.__ipc.docPreview({ type: "update_element", element: { ...el, links: [l0, l1] } });
    window.__app.getState().setPreview(result);
    await new Promise((r) => setTimeout(r, 250));
    const list = window.__planController.linkDrawList();
    window.__app.getState().setPreview(null);
    await new Promise((r) => setTimeout(r, 100));
    return { before, shown: list.length, handles: list.filter((l) => l.handle).length, after: window.__planController.linkDrawList().length };
  }, [switchB.id, lights[0].id, lights[1].id]);
  expect("links in an AI proposal show without a selection, with no flip handle, and go with it", proposed.before === 0 && proposed.shown === 2 && proposed.handles === 0 && proposed.after === 0, proposed);

  // ------------------------------------------------------------ 7. crowded tags keep clear of each other and of dimension text
  // A drainage run zigzagging over the front dimension's text (y -900, text at x 4000).
  const level = d0.project.levels[0].id;
  rev = await revision(page);
  await page.evaluate(
    (lv) =>
      window.__app.getState().dispatch({
        type: "add_element",
        element: {
          kind: "pipe", id: "", level_id: lv, system: "drainage", material: "upvc", diameter_mm: 50, name: "",
          points: [
            { x: 3000, y: -700, z: -300 }, { x: 3300, y: -1050, z: -309 }, { x: 3300, y: -1050, z: -500 },
            { x: 3600, y: -760, z: -510 }, { x: 3900, y: -1080, z: -505 }, { x: 3900, y: -1080, z: -700 },
            { x: 4200, y: -760, z: -712 }, { x: 4500, y: -1060, z: -730 }, { x: 4800, y: -780, z: -731 },
            { x: 4800, y: -780, z: -900 }, { x: 5100, y: -1080, z: -910 }, { x: 5400, y: -760, z: -925 },
            { x: 5700, y: -1060, z: -931 },
          ],
        },
      }),
    level,
  );
  await waitRevision(page, rev);
  const drain = (await pipes(page)).find((p) => p.system === "drainage");
  await view(page, { minX: 0, minY: -3400, maxX: 8500, maxY: 1600 });
  await page.waitForTimeout(100);
  const faded = page
    .waitForFunction(() => Object.keys(window.__planController.animSnapshot()).some((k) => k.startsWith("tagi:")), null, { timeout: 3000, polling: 5 })
    .then(() => true)
    .catch(() => false);
  await page.evaluate((id) => window.__app.getState().select([id]), drain.id);
  const fadedIn = await faded;
  expect("tags fade in when they first get a spot", fadedIn, fadedIn);
  await moveTo(page, 6800, 600);
  await page.waitForTimeout(400);
  const layout = await page.evaluate(() => window.__planController.tagLayout);
  const overlap = (a, b) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
  let tagHits = 0;
  let obstacleHits = 0;
  for (let i = 0; i < layout.placed.length; i++) {
    for (let j = i + 1; j < layout.placed.length; j++) if (overlap(layout.placed[i].box, layout.placed[j].box)) tagHits++;
    for (const o of layout.obstacles) if (overlap(layout.placed[i].box, o)) obstacleHits++;
  }
  log("crowded layout", { asked: layout.asked, placed: layout.placed.length, obstacles: layout.obstacles.length });
  expect("no two tags overlap", tagHits === 0, tagHits);
  expect("no tag overlaps dimension text, room labels or readouts", obstacleHits === 0, obstacleHits);
  expect("the least important tags are dropped when crowded", layout.placed.length < layout.asked, layout);
  expect("fall warnings are kept first", layout.placed.slice(0, 2).every((t) => t.id.includes(":f:")), layout.placed.map((t) => t.id));
  await shot("d10-crowded-tags");
  // The same run from further out: more crowding, still no overlap.
  await view(page, { minX: -3000, minY: -4000, maxX: 11000, maxY: 7000 });
  await moveTo(page, 9000, 6000);
  await page.waitForTimeout(400);
  const far = await page.evaluate(() => window.__planController.tagLayout);
  let farHits = 0;
  for (let i = 0; i < far.placed.length; i++) {
    for (let j = i + 1; j < far.placed.length; j++) if (overlap(far.placed[i].box, far.placed[j].box)) farHits++;
    for (const o of far.obstacles) if (overlap(far.placed[i].box, o)) farHits++;
  }
  log("far layout", { asked: far.asked, placed: far.placed.length });
  expect("zoomed out, tags still never overlap", farHits === 0, farHits);
  await shot("d11-crowded-tags-far");
};
