// Real app shell (http://localhost:1552/): creates a fresh blank project
// through the real dev bridge, draws a closed room with no door (which
// produces the "room_no_door" review issue), then clicks that issue in the
// Inspector's Review section and checks it selects the room and focuses the
// plan canvas on it.
import { expect, log } from "./lib.mjs";

async function ready(page) {
  await page.waitForFunction(() => !!window.__app, { timeout: 15000 });
}

async function readyEditor(page) {
  await page.waitForFunction(
    () => !!window.__app && !!window.__app.getState().doc && !!window.__planController && window.__planController.width > 0,
    { timeout: 15000 },
  );
  await page.waitForTimeout(150);
}

export default async (page, shot) => {
  await page.goto("http://localhost:1552/", { waitUntil: "networkidle" });
  await ready(page);

  // Create a fresh blank project (through the real bridge) and wait for the editor.
  const projectName = `Review check ${Date.now()}`;
  await page.evaluate((name) => window.__app.getState().createProject(name, "blank"), projectName);
  await readyEditor(page);

  // A closed room with no door: engine auto-creates the Room element and
  // flags "room_no_door" since nothing on its boundary is a door.
  const roomId = await page.evaluate(async () => {
    const app = window.__app.getState();
    const level = app.doc.project.levels[0].id;
    const result = await app.dispatch({
      type: "add_rect_room",
      origin: { x: 0, y: 0 },
      width_mm: 4000,
      depth_mm: 3000,
      name: "No Door Room",
      thickness_mm: null,
      level_id: level,
    });
    const room = window.__app.getState().doc.project.elements.find((e) => e.kind === "room");
    return { added: result?.added ?? [], roomId: room?.id ?? null };
  });
  log("add_rect_room result", roomId);

  const issue = await page.evaluate(() => {
    const doc = window.__app.getState().doc;
    return doc.derived.issues.find((i) => i.code === "room_no_door") ?? null;
  });
  log("room_no_door issue", issue);
  expect("room_no_door issue exists after drawing a doorless room", !!issue, issue);
  if (!issue) return;

  await shot("00-review-before-click");

  const before = await page.evaluate(() => ({
    selection: window.__app.getState().selection,
    view: { ...window.__planController.view },
  }));

  // Click the review item in the Inspector by its message text.
  const button = page.locator("button", { hasText: issue.message });
  await button.first().scrollIntoViewIfNeeded();
  await button.first().click();
  await page.waitForTimeout(250);

  await shot("01-review-after-click");

  const after = await page.evaluate(() => ({
    selection: window.__app.getState().selection,
    view: { ...window.__planController.view },
  }));

  log("selection before/after", { before: before.selection, after: after.selection });
  log("view before/after", { before: before.view, after: after.view });

  const selectsIssueIds = issue.element_ids.length > 0 && issue.element_ids.every((id) => after.selection.includes(id)) && after.selection.length === issue.element_ids.length;
  expect("clicking the review item selects its element_ids", selectsIssueIds, after.selection);

  const viewChanged = before.view.ox !== after.view.ox || before.view.oy !== after.view.oy || before.view.scale !== after.view.scale;
  expect("clicking the review item focuses (moves) the plan view", viewChanged, after.view);
};
