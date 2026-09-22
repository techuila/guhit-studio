// Bridge, sample bungalow: door swing default near a wall centerline.
// Wall 101 (0,0)->(8000,0) is the SOUTH exterior wall; both rooms are north
// of it (label points at y=3000), so within half its 150mm thickness of the
// centerline a door must default to swinging north (into a room), even when
// the cursor sits a few mm on the outside (south) of the line - this is the
// exact "door swings out of the building" case the orchestrator hit.
import { at, expect, log, moveTo, ready, tool } from "./lib.mjs";

const ghost = (page) =>
  page.evaluate(() => {
    const g = window.__planController.openingGhost;
    return g ? { flip_side: g.opening.flip_side, offset: g.opening.offset_mm } : null;
  });

export default async (page, shot) => {
  await ready(page, "bridge");
  await tool(page, "door");

  // On the centerline exactly.
  await moveTo(page, 4000, 0);
  let g = await ghost(page);
  log("on the centerline", g);
  expect("on the centerline: swings into the room (flip_side false)", g && g.flip_side === false, g);
  await shot("90-door-on-centerline");

  // A few mm south of the centerline (outside the building), still inside
  // the 75mm half-thickness band: must still default into the room.
  await moveTo(page, 4000, -30);
  g = await ghost(page);
  log("30mm south of centerline (in band)", g);
  expect("inside the band, south side: still swings into the room", g && g.flip_side === false, g);
  await shot("91-door-in-band-outside-cursor-side");

  // Symmetric check north of the centerline, inside the band.
  await moveTo(page, 4000, 30);
  g = await ghost(page);
  log("30mm north of centerline (in band)", g);
  expect("inside the band, north side: still swings into the room", g && g.flip_side === false, g);

  // Well south of the wall, outside the band: the cursor side decides, and
  // now the door is on the outside face, so it flips to swing south (out).
  await moveTo(page, 4000, -200);
  g = await ghost(page);
  log("200mm south of centerline (outside band)", g);
  expect("outside the band, cursor south: follows the cursor (flip_side true)", g && g.flip_side === true, g);
  await shot("92-door-outside-band-cursor-south");

  // Well north of the wall, outside the band, on the room side: cursor
  // decides and agrees with the room-side default.
  await moveTo(page, 4000, 200);
  g = await ghost(page);
  log("200mm north of centerline (outside band)", g);
  expect("outside the band, cursor north: follows the cursor (flip_side false)", g && g.flip_side === false, g);

  // F still flips, even inside the band.
  await moveTo(page, 4000, 0);
  await page.keyboard.press("f");
  await page.waitForTimeout(60);
  g = await ghost(page);
  log("F pressed on the centerline", g);
  expect("F flips the default even on the centerline", g && g.flip_side === true, g);
  await shot("93-door-f-flip-on-centerline");

  // Place it and confirm the committed opening carries the same flip via add_opening.
  // (The fixture already has a front door, so diff against the openings that existed before this click.)
  const beforeIds = await page.evaluate(() => window.__app.getState().doc.project.elements.filter((e) => e.kind === "opening").map((e) => e.id));
  const p = await at(page, 4000, 0);
  await page.mouse.click(p.x, p.y);
  await page.waitForTimeout(300);
  const opening = await page.evaluate(
    (ids) => window.__app.getState().doc.project.elements.find((e) => e.kind === "opening" && !ids.includes(e.id)),
    beforeIds,
  );
  log("placed door", opening);
  expect("placed door keeps the flip (F was still down from the check above)", !!opening && opening.flip_side === true, opening);
};
