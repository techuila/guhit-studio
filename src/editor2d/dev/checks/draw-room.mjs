// Draws a closed 4000 x 3000 room with typed lengths on a blank bridge project,
// places a door and a window, moves a wall, then click and marquee selects.
import { at, clickAt, dragTo, expect, log, moveTo, ready, state, tool } from "./lib.mjs";

export default async (page, shot) => {
  await ready(page, "bridge");
  await tool(page, "wall");
  await clickAt(page, 0, 0);
  await moveTo(page, 2300, 40);
  await shot("10-wall-readout");
  await page.keyboard.type("4000");
  await page.waitForTimeout(100);
  await shot("11-wall-typed-box");
  await page.keyboard.press("Enter");
  await moveTo(page, 4050, 1700);
  await page.keyboard.type("3000");
  await page.keyboard.press("Enter");
  await moveTo(page, 1500, 3040);
  await page.keyboard.type("4000<180");
  await page.waitForTimeout(100);
  await shot("12-wall-typed-angle");
  await page.keyboard.press("Enter");
  let s = await state(page);
  expect("no command sent while the chain is open", s.counts.wall === 0 && s.op === "wall", { walls: s.counts.wall, op: s.op });
  await moveTo(page, 30, 40);
  await shot("13-wall-closing");
  await clickAt(page, 0, 0);
  await page.waitForTimeout(400);
  s = await state(page);
  log("after chain", { walls: s.walls, rooms: s.rooms, undo: s.undoLabel, toasts: s.toasts });
  expect("closed chain made 4 walls", s.counts.wall === 4, s.counts);
  expect("room detected with net area 10.9725 m2", s.rooms.length === 1 && Math.abs(s.rooms[0].area_m2 - 10.9725) < 1e-3, s.rooms);
  expect("back to idle after commit", s.op === "idle", s.op);
  const revAfterChain = s.revision;

  await page.click('[data-testid="fit"]');
  await page.waitForTimeout(150);
  await shot("14-room-done");

  // Door on the south wall, off center.
  await tool(page, "door");
  await moveTo(page, 1210, 60);
  await shot("20-door-ghost");
  await moveTo(page, 2010, 50);
  await shot("21-door-ghost-centered");
  await clickAt(page, 1210, 60);
  await page.waitForTimeout(300);
  // Window on the east wall.
  await tool(page, "window");
  await moveTo(page, 3960, 1500);
  await shot("22-window-ghost");
  await clickAt(page, 3960, 1500);
  await page.waitForTimeout(300);
  s = await state(page);
  log("openings", s.openings.map((o) => ({ type: o.opening_type, style: o.style, offset: o.offset_mm, w: o.width_mm, side: o.flip_side, hinge: o.flip_hinge })));
  expect("one door and one window placed", s.counts.opening === 2, s.counts);
  expect("tool stays active after placing", s.tool === "window", s.tool);
  await page.keyboard.press("Escape");
  s = await state(page);
  expect("Escape returns to select", s.tool === "select", s.tool);
  await page.mouse.move(5, 300);
  await shot("23-openings-placed");

  // Select the north wall by click, then drag its midpoint grip north by 1000.
  await clickAt(page, 2600, 3000);
  s = await state(page);
  expect("click selects one wall", s.selection.length === 1, s.selection);
  await shot("30-wall-selected");
  const north = s.walls.find((w) => Math.abs(w.start.y - 3000) < 1 && Math.abs(w.end.y - 3000) < 1);
  await dragTo(page, [2000, 3000], [2000, 4000], { beforeUp: () => shot("31-wall-moving") });
  await page.waitForTimeout(300);
  s = await state(page);
  const moved = s.walls.find((w) => w.id === north.id);
  log("moved wall", moved);
  expect("wall moved to y = 4000", moved.start.y === 4000 && moved.end.y === 4000, moved);
  expect("neighbours stretched, room is 4000 x 4000 net 14.8225", s.rooms.length === 1 && Math.abs(s.rooms[0].area_m2 - 14.8225) < 1e-3, s.rooms);
  await page.click('[data-testid="fit"]');
  await page.waitForTimeout(150);
  await shot("32-wall-moved");

  // Shift click toggles.
  await clickAt(page, 0, 2000);
  await page.keyboard.down("Shift");
  await clickAt(page, 4000, 2600);
  await page.keyboard.up("Shift");
  s = await state(page);
  expect("shift click adds a second wall", s.selection.length === 2, s.selection.length);

  // Window marquee, left to right around everything.
  await dragTo(page, [-350, -350], [4350, 4350], { beforeUp: () => shot("40-marquee-window") });
  s = await state(page);
  log("window marquee selection", s.selection.length);
  expect("window marquee selects walls, openings and the room label", s.selection.length >= 7, s.selection.length);
  await shot("41-marquee-window-result");
  // Window marquee that only partly covers walls selects nothing of them.
  await dragTo(page, [1000, 1000], [3000, 4300]);
  s = await state(page);
  log("partial window marquee", s.selection.length);
  const partial = s.selection.length;
  // Crossing marquee, right to left over the same area.
  await dragTo(page, [3000, 4300], [1000, 1000], { beforeUp: () => shot("42-marquee-crossing") });
  s = await state(page);
  log("crossing marquee", s.selection.length);
  expect("crossing picks more than window over the same area", s.selection.length > partial, { partial, crossing: s.selection.length });

  // Hover feedback.
  await clickAt(page, 4330, 4330);
  await moveTo(page, 4000, 3300);
  s = await state(page);
  expect("hover sets hoverId", !!s.hoverId, s.hoverId);
  await shot("43-hover");

  // The whole chain was one undo step: revision moved by exactly 1 for it.
  log("revision after chain", revAfterChain);
  expect("chain was a single command (revision 1)", revAfterChain === 1, revAfterChain);
  expect("no error toasts", s.toasts.length === 0, s.toasts);
  void at;
};
