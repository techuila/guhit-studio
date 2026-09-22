// Helpers shared by the scripted UI checks (node scripts/ui-check.mjs <url> <out.png> <steps.mjs>).

export async function ready(page, source) {
  await page.waitForFunction(
    (src) => {
      const el = document.querySelector('[data-testid="status"]');
      return !!el && el.textContent.includes(src) && !!window.__planController && !!window.__app.getState().doc;
    },
    source,
    { timeout: 15000 },
  );
  await page.waitForTimeout(150);
}

/** Model mm -> page pixel. */
export async function at(page, x, y) {
  return page.evaluate(
    ([mx, my]) => {
      const c = window.__planController;
      const r = document.querySelector('[data-testid="plan-canvas"] canvas').getBoundingClientRect();
      return { x: r.left + c.view.ox + mx * c.view.scale, y: r.top + c.view.oy - my * c.view.scale };
    },
    [x, y],
  );
}

export async function moveTo(page, x, y, steps = 6) {
  const p = await at(page, x, y);
  await page.mouse.move(p.x, p.y, { steps });
  await page.waitForTimeout(40);
}

export async function clickAt(page, x, y, opts = {}) {
  await moveTo(page, x, y);
  const p = await at(page, x, y);
  await page.mouse.click(p.x, p.y, opts);
  await page.waitForTimeout(120);
}

export async function dragTo(page, from, to, opts = {}) {
  await moveTo(page, from[0], from[1]);
  await page.mouse.down();
  const mid = [(from[0] + to[0]) / 2, (from[1] + to[1]) / 2];
  await moveTo(page, mid[0], mid[1], 5);
  await moveTo(page, to[0], to[1], 5);
  if (opts.beforeUp) await opts.beforeUp();
  await page.mouse.up();
  await page.waitForTimeout(250);
}

export async function tool(page, name) {
  await page.click(`[data-tool="${name}"]`);
  await page.waitForTimeout(60);
}

export async function state(page) {
  return page.evaluate(() => {
    const s = window.__app.getState();
    const els = s.doc.project.elements;
    const count = (k) => els.filter((e) => e.kind === k).length;
    return {
      revision: s.doc.revision,
      walls: els.filter((e) => e.kind === "wall").map((w) => ({ id: w.id, start: w.start, end: w.end })),
      openings: els.filter((e) => e.kind === "opening"),
      rooms: s.doc.derived.rooms.map((r) => ({ id: r.room_id, area_m2: +(r.area_mm2 / 1e6).toFixed(4) })),
      counts: { wall: count("wall"), opening: count("opening"), room: count("room"), asset: count("asset"), dimension: count("dimension"), annotation: count("annotation"), camera: count("camera"), column: count("column"), stair: count("stair") },
      selection: s.selection,
      hoverId: s.hoverId,
      tool: s.tool,
      canUndo: s.doc.can_undo,
      undoLabel: s.doc.undo_label,
      toasts: s.toasts.map((t) => t.message),
      op: window.__planController.op.kind,
    };
  });
}

export function log(label, value) {
  console.log(`CHECK ${label}: ${typeof value === "string" ? value : JSON.stringify(value)}`);
}

export function expect(label, cond, detail) {
  console.log(`${cond ? "PASS" : "FAIL"} ${label}${detail !== undefined ? " -> " + JSON.stringify(detail) : ""}`);
}
