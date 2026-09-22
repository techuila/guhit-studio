// Bridge, sample bungalow: every other tool, grips, slide, duplicate, rename,
// view navigation, keyboard handling and the failed command path.
import { at, clickAt, dragTo, expect, log, moveTo, ready, state, tool } from "./lib.mjs";

/** Fit, then one wheel notch out so there is room to work around the model. */
async function fitOut(page) {
  await page.click('[data-testid="fit"]');
  await page.waitForTimeout(120);
  await page.mouse.move(720, 450);
  await page.mouse.wheel(0, 300);
  await page.waitForTimeout(120);
}

const view = (page) => page.evaluate(() => ({ ...window.__planController.view }));

export default async (page, shot) => {
  await ready(page, "bridge");
  let s = await state(page);
  const base = s.counts;
  log("start", base);

  // ---- view navigation
  const v0 = await view(page);
  const c = await at(page, 4000, 3000);
  await page.mouse.move(c.x, c.y);
  await page.mouse.wheel(0, -300);
  await page.waitForTimeout(100);
  const v1 = await view(page);
  const c1 = await at(page, 4000, 3000);
  expect("wheel zooms in", v1.scale > v0.scale, { from: v0.scale, to: v1.scale });
  expect("zoom keeps the point under the cursor", Math.abs(c1.x - c.x) < 1 && Math.abs(c1.y - c.y) < 1, { c, c1 });
  // pinch = ctrl + wheel
  await page.keyboard.down("Control");
  await page.mouse.wheel(0, 40);
  await page.keyboard.up("Control");
  await page.waitForTimeout(100);
  const v2 = await view(page);
  expect("ctrl wheel (pinch) zooms out", v2.scale < v1.scale, { from: v1.scale, to: v2.scale });
  // middle drag pans
  await page.mouse.move(c.x, c.y);
  await page.mouse.down({ button: "middle" });
  await page.mouse.move(c.x + 120, c.y + 60, { steps: 4 });
  await page.mouse.up({ button: "middle" });
  const v3 = await view(page);
  expect("middle drag pans", Math.abs(v3.ox - v2.ox - 120) < 1 && Math.abs(v3.oy - v2.oy - 60) < 1, { dx: v3.ox - v2.ox, dy: v3.oy - v2.oy });
  // space drag pans
  await page.keyboard.down("Space");
  await page.mouse.down();
  await page.mouse.move(c.x + 20, c.y + 10, { steps: 4 });
  await page.mouse.up();
  await page.keyboard.up("Space");
  const v4 = await view(page);
  expect("space drag pans", Math.abs(v4.ox - v3.ox + 100) < 1, { dx: v4.ox - v3.ox });
  s = await state(page);
  expect("space drag did not select or edit", s.selection.length === 0 && s.revision === 0, { sel: s.selection.length, rev: s.revision });
  // two finger scroll: small non-notch deltas on both axes pan
  await page.evaluate(() => {
    const cv = document.querySelector('[data-testid="plan-canvas"] canvas');
    const e = new WheelEvent("wheel", { deltaX: 12, deltaY: 7, deltaMode: 0, bubbles: true, cancelable: true, clientX: 600, clientY: 400 });
    Object.defineProperty(e, "wheelDeltaY", { value: -21 });
    cv.dispatchEvent(e);
  });
  const v5 = await view(page);
  expect("trackpad scroll pans", Math.abs(v5.ox - v4.ox + 12) < 0.01 && Math.abs(v5.oy - v4.oy + 7) < 0.01 && v5.scale === v4.scale, { dx: v5.ox - v4.ox, dy: v5.oy - v4.oy });
  await page.click('[data-testid="fit"]');
  await page.waitForTimeout(150);

  // ---- rect room, typed 3000,2500 to the east of the house
  await fitOut(page);
  await tool(page, "rect_room");
  await clickAt(page, 8000, 0);
  await moveTo(page, 9500, 1200);
  await page.keyboard.type("3000,2500");
  await page.waitForTimeout(100);
  await shot("60-rect-typed");
  await page.keyboard.press("Enter");
  await page.waitForTimeout(400);
  s = await state(page);
  log("after rect", { counts: s.counts, rooms: s.rooms, toasts: s.toasts });
  expect("rect room added a third room", s.rooms.length === 3, s.rooms);
  const newRoom = s.rooms.find((r) => Math.abs(r.area_m2 - 2.85 * 2.35) < 0.01);
  expect("typed room is 3000 x 2500 on centerlines (net 6.6975)", !!newRoom, s.rooms);
  await fitOut(page);

  // ---- asset: sofa against the north wall of the living room, R rotates
  await page.selectOption('[data-testid="asset-key"]', "sofa-3");
  await moveTo(page, 2500, 5350);
  await shot("61-asset-face-snap");
  await clickAt(page, 2500, 5350);
  await page.waitForTimeout(300);
  s = await state(page);
  const sofa = await page.evaluate(() => window.__app.getState().doc.project.elements.find((e) => e.kind === "asset" && e.catalog_key === "sofa-3"));
  log("sofa", sofa && { pos: sofa.position, rot: sofa.rotation_deg });
  expect("sofa back sits on the inner face of the north wall (y 5475), rotation 0", !!sofa && Math.abs(sofa.position.y - (5925 - 450)) < 1 && sofa.rotation_deg === 0, sofa && sofa.position);
  await moveTo(page, 2500, 3000);
  await page.keyboard.press("r");
  await page.waitForTimeout(80);
  await shot("62-asset-rotated-ghost");
  const turns = await page.evaluate(() => window.__planController.turns);
  expect("R rotates the asset ghost", turns === 1, turns);

  // ---- column and stair
  await tool(page, "column");
  await clickAt(page, 9500, 5000);
  await tool(page, "stair");
  await clickAt(page, 12000, 3000);
  await page.waitForTimeout(300);
  s = await state(page);
  expect("column and stair placed", s.counts.column === base.column + 1 && s.counts.stair === base.stair + 1, s.counts);

  // ---- dimension between two wall endpoints, offset to the north
  await fitOut(page);
  await tool(page, "dimension");
  await clickAt(page, 30, 6040);
  await moveTo(page, 7960, 6030);
  await shot("63-dimension-second-point");
  await clickAt(page, 7960, 6030);
  await moveTo(page, 4000, 6850);
  await shot("64-dimension-offset");
  await clickAt(page, 4000, 6850);
  await page.waitForTimeout(300);
  const dim = await page.evaluate(() => window.__app.getState().doc.project.elements.filter((e) => e.kind === "dimension").pop());
  log("dimension", dim);
  expect("dimension snapped to both corners", !!dim && Math.abs(Math.hypot(dim.b.x - dim.a.x, dim.b.y - dim.a.y) - 8000) < 160, dim && { a: dim.a, b: dim.b });

  // ---- text
  await tool(page, "text");
  await clickAt(page, 8500, 5600);
  await page.keyboard.type("FLOOR PLAN");
  await shot("65-text-editing");
  await page.keyboard.press("Enter");
  await page.waitForTimeout(300);
  s = await state(page);
  expect("annotation added, text tool still active", s.counts.annotation === base.annotation + 1 && s.tool === "text", { n: s.counts.annotation, tool: s.tool });

  // ---- camera
  await tool(page, "camera");
  await clickAt(page, 1000, 1200);
  await moveTo(page, 3000, 3500);
  await shot("66-camera-target");
  await clickAt(page, 3000, 3500);
  await page.waitForTimeout(300);
  const cam = await page.evaluate(() => window.__app.getState().doc.project.elements.filter((e) => e.kind === "camera").pop());
  expect("camera added at eye height 1600", !!cam && cam.position.z === 1600 && cam.preset === "custom", cam);
  await page.keyboard.press("Escape");
  await fitOut(page);
  await shot("67-all-tools-result");

  // ---- opening slides along its wall
  const door = await page.evaluate(() => window.__app.getState().doc.project.elements.find((e) => e.kind === "opening" && e.opening_type === "door" && e.offset_mm === 1500));
  await dragTo(page, [1500, 0], [2600, 300], { beforeUp: () => shot("70-door-sliding") });
  const door2 = await page.evaluate((id) => window.__app.getState().doc.project.elements.find((e) => e.id === id), door.id);
  log("door offset", { before: door.offset_mm, after: door2.offset_mm });
  expect("door slid along the wall and stopped against the window (2750 - 450)", door2.offset_mm === 2300 && door2.wall_id === door.wall_id, door2.offset_mm);

  // ---- wall endpoint grip: pull the partition top end 1000 east
  await clickAt(page, 5000, 4500);
  await shot("71-partition-selected");
  await dragTo(page, [5000, 6000], [6000, 6020], { beforeUp: () => shot("72-endpoint-dragging") });
  s = await state(page);
  const part = s.walls.find((w) => w.start.x === 5000 && w.start.y === 0);
  log("partition", part);
  expect("endpoint grip moved the wall end to x 6000 on the north wall", !!part && part.end.x === 6000 && part.end.y === 6000, part);

  // ---- alt drag duplicates
  const bedsBefore = s.counts.asset;
  await page.keyboard.down("Alt");
  await dragTo(page, [6900, 4300], [6900, 1500], { beforeUp: () => shot("73-alt-duplicate") });
  await page.keyboard.up("Alt");
  s = await state(page);
  expect("alt drag duplicated the bed", s.counts.asset === bedsBefore + 1, s.counts.asset);

  // ---- asset rotate grip
  const bedId = s.selection[0];
  const grip = await page.evaluate(() => window.__planController.gripList().map((g) => ({ kind: g.kind, pos: g.pos })));
  log("grips of the copy", grip);
  if (grip[0]) {
    const pivot = await page.evaluate((id) => window.__app.getState().doc.project.elements.find((e) => e.id === id).position, bedId);
    await dragTo(page, [grip[0].pos.x, grip[0].pos.y], [pivot.x + 2000, pivot.y + 30], { beforeUp: () => shot("74-rotate-grip") });
    const rot = await page.evaluate((id) => window.__app.getState().doc.project.elements.find((e) => e.id === id).rotation_deg, bedId);
    expect("rotate grip turned the bed to 270", rot === 270, rot);
  }

  // ---- inline room rename by double click
  const lp = await page.evaluate(() => {
    const st = window.__app.getState();
    const r = st.doc.derived.rooms[0];
    return { id: r.room_id, p: r.label_point };
  });
  const sp = await at(page, lp.p.x, lp.p.y);
  await page.mouse.dblclick(sp.x, sp.y - 6);
  await page.waitForTimeout(150);
  await page.keyboard.press("Meta+a");
  await page.keyboard.type("Sala");
  await shot("75-room-rename");
  await page.keyboard.press("Enter");
  await page.waitForTimeout(300);
  const room = await page.evaluate((id) => window.__app.getState().doc.project.elements.find((e) => e.id === id), lp.id);
  expect("room renamed inline", room.name === "Sala" && room.auto_named === false, room.name);

  // ---- plain drag moves an asset with one command
  await page.keyboard.press("Escape");
  await clickAt(page, 9800, 6300);
  const sofa0 = await page.evaluate(() => window.__app.getState().doc.project.elements.find((e) => e.kind === "asset" && e.catalog_key === "sofa-3"));
  const revMove = (await state(page)).revision;
  await dragTo(page, [sofa0.position.x, sofa0.position.y], [sofa0.position.x + 1000, sofa0.position.y - 2000], { beforeUp: () => shot("77-asset-moving") });
  const sofa1 = await page.evaluate((id) => window.__app.getState().doc.project.elements.find((e) => e.id === id), sofa0.id);
  s = await state(page);
  log("sofa moved", { from: sofa0.position, to: sofa1.position, revs: s.revision - revMove });
  expect("drag moved the sofa by a snapped delta in one command", s.revision === revMove + 1 && Math.abs(sofa1.position.x - sofa0.position.x - 1000) <= 100 && Math.abs(sofa1.position.y - sofa0.position.y + 2000) <= 100, sofa1.position);

  // ---- dimension offset grip
  const dimEl = await page.evaluate(() => window.__app.getState().doc.project.elements.find((e) => e.kind === "dimension" && e.offset_mm === -900));
  await clickAt(page, 2000, -900);
  s = await state(page);
  expect("dimension selected by its line", s.selection[0] === dimEl.id, s.selection);
  await dragTo(page, [4000, -900], [4000, -1500], { beforeUp: () => shot("78-dimension-offset-grip") });
  const dimEl2 = await page.evaluate((id) => window.__app.getState().doc.project.elements.find((e) => e.id === id), dimEl.id);
  expect("offset grip moved the dimension line to -1500", dimEl2.offset_mm === -1500, dimEl2.offset_mm);

  // ---- annotation rename by double click
  const note = await page.evaluate(() => window.__app.getState().doc.project.elements.find((e) => e.kind === "annotation"));
  const np = await at(page, note.position.x + 300, note.position.y + 80);
  await page.mouse.dblclick(np.x, np.y);
  await page.waitForTimeout(150);
  await page.keyboard.press("Meta+a");
  await page.keyboard.type("GROUND FLOOR");
  await page.keyboard.press("Enter");
  await page.waitForTimeout(300);
  const note2 = await page.evaluate((id) => window.__app.getState().doc.project.elements.find((e) => e.id === id), note.id);
  expect("annotation text edited inline", note2.text === "GROUND FLOOR", note2.text);

  // ---- keyboard: backspace removes the last wall point, escape cancels, nothing is committed
  const rev = (await state(page)).revision;
  const wallsBefore = (await state(page)).counts.wall;
  await tool(page, "wall");
  await clickAt(page, 9000, 3500);
  await clickAt(page, 9000, 4500);
  await clickAt(page, 10500, 4500);
  let pts = await page.evaluate(() => window.__planController.op.points.length);
  await page.keyboard.press("Backspace");
  let pts2 = await page.evaluate(() => window.__planController.op.points.length);
  expect("Backspace removes the last point", pts === 3 && pts2 === 2, { pts, pts2 });
  await page.keyboard.type("12");
  await page.keyboard.press("Escape");
  let typedGone = await page.evaluate(() => window.__planController.op.kind === "wall" && window.__planController.op.typed === null);
  expect("first Escape clears the typed value only", typedGone, typedGone);
  await page.keyboard.press("Escape");
  s = await state(page);
  expect("second Escape cancels the chain, tool stays, nothing committed", s.op === "idle" && s.tool === "wall" && s.revision === rev, { op: s.op, tool: s.tool });
  // open chain finished with Enter = one command
  await clickAt(page, 9000, 3500);
  await clickAt(page, 9000, 4500);
  await clickAt(page, 10500, 4500);
  await page.keyboard.press("Enter");
  await page.waitForTimeout(400);
  s = await state(page);
  expect("Enter finishes an open chain as one command with 2 walls", s.revision === rev + 1 && s.counts.wall === wallsBefore + 2, { rev: s.revision - rev, walls: s.counts.wall });
  await page.click('[data-testid="undo"]');
  await page.waitForTimeout(300);
  s = await state(page);
  expect("one undo removes the whole chain", s.counts.wall === wallsBefore, s.counts.wall);

  // ---- failed command: the bridge rejects, the canvas must return to idle
  await page.route("**/ipc/doc_apply", (route) => route.fulfill({ status: 400, contentType: "application/json", body: JSON.stringify({ code: "invalid", message: "Simulated failure", element_ids: [] }) }));
  await clickAt(page, 9000, 3500);
  await clickAt(page, 9000, 4500);
  await page.keyboard.press("Enter");
  await page.waitForTimeout(400);
  s = await state(page);
  expect("after a failed command the op is idle and the error was toasted", s.op === "idle" && s.toasts.includes("Simulated failure"), { op: s.op, toasts: s.toasts });
  await page.keyboard.press("Escape");
  await tool(page, "select");
  await dragTo(page, [6900, 4300], [6000, 4300]);
  s = await state(page);
  expect("failed move also returns to idle", s.op === "idle", s.op);
  await shot("76-after-failed-command");
  await page.unroute("**/ipc/doc_apply");

  // ---- idle keys are not swallowed
  const leaked = await page.evaluate(async () => {
    let seen = 0;
    const h = () => seen++;
    window.addEventListener("keydown", h);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Delete", bubbles: true }));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "w", bubbles: true }));
    window.removeEventListener("keydown", h);
    return seen;
  });
  expect("idle keys reach other listeners", leaked === 2, leaked);
};
