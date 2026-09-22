// Bridge, sample bungalow: snapping glyphs and guides, Shift ortho, snap off,
// door flips (F, H), HiDPI backing store, idle Escape.
import { at, clickAt, expect, log, moveTo, ready, state, tool } from "./lib.mjs";

const snapInfo = (page) =>
  page.evaluate(() => {
    const r = window.__planController.snapResult;
    return r ? { type: r.type, point: r.point, guides: r.guides.map((g) => g.kind), locked: r.angleLocked } : null;
  });

export default async (page, shot) => {
  await ready(page, "bridge");
  await page.mouse.move(720, 450);
  await page.mouse.wheel(0, 300);
  await page.waitForTimeout(150);

  await tool(page, "wall");
  await moveTo(page, 40, 5960);
  let r = await snapInfo(page);
  expect("endpoint snap on a wall corner", r && r.type === "endpoint" && r.point.x === 0 && r.point.y === 6000, r);
  await shot("80-snap-endpoint");
  await moveTo(page, 8030, 3020);
  r = await snapInfo(page);
  expect("midpoint snap on the east wall", r && r.type === "midpoint" && r.point.y === 3000, r);
  await shot("81-snap-midpoint");
  await moveTo(page, 5020, 5990);
  r = await snapInfo(page);
  expect("intersection snap where the partition meets the north wall", r && (r.type === "intersection" || r.type === "endpoint") && r.point.x === 5000 && r.point.y === 6000, r);

  // Start a wall outside, draw east: alignment guide with the house corner x = 8000.
  await clickAt(page, 9500, 7000);
  await moveTo(page, 9530, 6020);
  r = await snapInfo(page);
  log("south from 9500,7000 near y 6000", r);
  expect("extension alignment with the north wall line, angle locked", r && r.locked && Math.abs(r.point.y - 6000) < 1 && r.point.x === 9500, r);
  await shot("82-snap-extension-guide");

  // Free angle, then Shift forces ortho.
  await moveTo(page, 11000, 6300);
  r = await snapInfo(page);
  expect("free direction is not angle locked", r && !r.locked, r);
  await page.keyboard.down("Shift");
  await page.waitForTimeout(80);
  r = await snapInfo(page);
  expect("Shift forces ortho without moving the mouse", r && r.locked && (r.point.y === 7000 || r.point.x === 9500), r);
  await shot("83-shift-ortho");
  await page.keyboard.up("Shift");

  // 45 degree lock
  await moveTo(page, 10520, 5990);
  r = await snapInfo(page);
  expect("45 degree lock", r && r.locked && Math.abs(Math.abs(r.point.x - 9500) - Math.abs(r.point.y - 7000)) < 1, r);
  await shot("84-angle-45");

  // Perpendicular onto the east wall from a point inside the bedroom.
  await page.keyboard.press("Escape");
  await clickAt(page, 6500, 2250);
  await moveTo(page, 7960, 2600);
  r = await snapInfo(page);
  log("toward the east wall", r);

  // Snap off: raw cursor.
  await page.keyboard.press("Escape");
  await page.click('[data-testid="snap"]');
  await moveTo(page, 40, 5960);
  r = await snapInfo(page);
  expect("snap off gives the raw cursor", r && r.type === "none", r);
  await page.click('[data-testid="snap"]');

  // Idle Escape is not swallowed and returns to select.
  const seen = await page.evaluate(() => {
    window.__esc = 0;
    window.addEventListener("keydown", (e) => e.key === "Escape" && window.__esc++);
    return true;
  });
  void seen;
  await page.keyboard.press("Escape");
  let s = await state(page);
  const esc = await page.evaluate(() => window.__esc);
  expect("idle Escape returns to select and still reaches other listeners", s.tool === "select" && esc === 1, { tool: s.tool, esc });
  // In an operation Escape is swallowed.
  await tool(page, "wall");
  await clickAt(page, 9500, 7000);
  await page.keyboard.press("Escape");
  const esc2 = await page.evaluate(() => window.__esc);
  expect("Escape during an operation is swallowed", esc2 === 1, esc2);

  // Door with F and H: goes in as one add_element with the flips.
  await tool(page, "door");
  await moveTo(page, 6500, 5900);
  const g0 = await page.evaluate(() => ({ side: window.__planController.openingGhost.opening.flip_side, hinge: window.__planController.openingGhost.opening.flip_hinge }));
  await page.keyboard.press("f");
  await page.keyboard.press("h");
  await page.waitForTimeout(80);
  const g1 = await page.evaluate(() => ({ side: window.__planController.openingGhost.opening.flip_side, hinge: window.__planController.openingGhost.opening.flip_hinge }));
  expect("F and H flip the ghost", g0.side !== g1.side && g0.hinge !== g1.hinge, { g0, g1 });
  await shot("85-door-flipped-ghost");
  const before = await state(page);
  const p = await at(page, 6500, 5900);
  await page.mouse.click(p.x, p.y);
  await page.waitForTimeout(400);
  s = await state(page);
  const added = s.openings.find((o) => !before.openings.some((b) => b.id === o.id));
  log("flipped door", added && { side: added.flip_side, hinge: added.flip_hinge, offset: added.offset_mm, toasts: s.toasts });
  expect("flipped door placed in one command", !!added && added.flip_side === g1.side && added.flip_hinge === g1.hinge && s.revision === before.revision + 1, added);
  await page.mouse.move(10, 300);
  await shot("86-door-flipped-placed");

  // HiDPI: the backing store follows the device pixel ratio.
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 2, mobile: false });
  await page.evaluate(() => window.dispatchEvent(new Event("resize")));
  await page.waitForTimeout(250);
  const px = await page.evaluate(() => {
    const cv = document.querySelector('[data-testid="plan-canvas"] canvas');
    return { dpr: window.devicePixelRatio, w: cv.width, cssW: cv.clientWidth, h: cv.height, cssH: cv.clientHeight };
  });
  expect("canvas backing store is 2x at dpr 2", px.dpr === 2 && px.w === px.cssW * 2 && px.h === px.cssH * 2, px);
  await shot("87-hidpi");
};
