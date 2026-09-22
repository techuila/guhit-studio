// Real app shell (http://localhost:1552/): creates a fresh blank project,
// draws a room, then previews a resize_room command (the same mechanism the
// AI copilot uses: ipc.docPreview + store.setPreview) and checks the
// inspector switches to the visible (previewed) document, shows the
// "Previewing AI proposal" marker, and makes its fields read-only.
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

  const projectName = `Preview check ${Date.now()}`;
  await page.evaluate((name) => window.__app.getState().createProject(name, "blank"), projectName);
  await readyEditor(page);

  const committedArea = await page.evaluate(async () => {
    const app = window.__app.getState();
    const level = app.doc.project.levels[0].id;
    await app.dispatch({
      type: "add_rect_room",
      origin: { x: 0, y: 0 },
      width_mm: 4000,
      depth_mm: 3000,
      name: "Living",
      thickness_mm: null,
      level_id: level,
    });
    return window.__app.getState().doc.derived.totals.floor_area_m2;
  });
  log("committed floor area (m2)", committedArea);

  await shot("00-before-preview");

  const before = await page.evaluate(() => {
    const body = document.querySelector('[class*="inspectorBody"]');
    return {
      hasBanner: !!document.querySelector('[class*="previewBanner"]'),
      inert: body ? body.inert : null,
      floorAreaText: document.querySelector('[class*="totals"] dd')?.textContent ?? null,
    };
  });
  log("inspector before preview", before);
  expect("no preview banner before a preview", !before.hasBanner);
  expect("inspector not inert before a preview", before.inert === false);

  // Preview widening the room (like an AI proposal ghost): committed doc is
  // untouched, only `preview` is set.
  const previewInfo = await page.evaluate(async () => {
    const app = window.__app.getState();
    const room = app.doc.project.elements.find((e) => e.kind === "room");
    const result = await window.__ipc.docPreview({ type: "resize_room", room_id: room.id, side: "east", delta_mm: 2000 });
    window.__app.getState().setPreview(result);
    return { previewArea: result.state.derived.totals.floor_area_m2, committedAreaStillSame: window.__app.getState().doc.derived.totals.floor_area_m2 };
  });
  log("preview vs committed after setPreview", previewInfo);
  expect("preview area differs from committed area", previewInfo.previewArea !== committedArea, previewInfo);
  expect("committed doc unchanged by the preview", previewInfo.committedAreaStillSame === committedArea, previewInfo);

  await page.waitForTimeout(150);
  await shot("01-during-preview");

  const during = await page.evaluate(() => {
    const body = document.querySelector('[class*="inspectorBody"]');
    return {
      hasBanner: !!document.querySelector('[class*="previewBanner"]'),
      inert: body ? body.inert : null,
      floorAreaText: document.querySelector('[class*="totals"] dd')?.textContent ?? null,
    };
  });
  log("inspector during preview", during);
  expect("preview banner shown during a preview", during.hasBanner);
  expect("inspector body is inert (read-only) during a preview", during.inert === true);
  expect("inspector shows the PREVIEWED area, not the committed one", during.floorAreaText !== before.floorAreaText, during);

  // A field click during the preview must not focus/edit anything.
  const clickResult = await page.evaluate(() => {
    const input = document.querySelector('[class*="inspectorBody"] input');
    if (!input) return { hadInput: false };
    input.click();
    return { hadInput: true, focused: document.activeElement === input };
  });
  log("field click during preview", clickResult);
  expect("clicking a field during the preview does not focus it", !clickResult.focused, clickResult);

  // Discard the preview: the inspector goes back to normal.
  await page.evaluate(() => window.__app.getState().setPreview(null));
  await page.waitForTimeout(150);
  await shot("02-after-discard");
  const after = await page.evaluate(() => ({
    hasBanner: !!document.querySelector('[class*="previewBanner"]'),
    inert: document.querySelector('[class*="inspectorBody"]')?.inert ?? null,
    floorAreaText: document.querySelector('[class*="totals"] dd')?.textContent ?? null,
  }));
  log("inspector after discarding the preview", after);
  expect("no preview banner after discarding", !after.hasBanner);
  expect("inspector editable again after discarding", after.inert === false);
  expect("floor area back to the committed value after discarding", after.floorAreaText === before.floorAreaText, after);
};
