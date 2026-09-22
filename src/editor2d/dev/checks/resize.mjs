// Real app shell (not the PlanCanvas-only dev harness): loads the fixture
// straight into the editor and exercises PlanCanvas resize when switching
// view modes, dragging the split divider, and collapsing the side dock.
//
// Two things are verified after every resize settles:
//   1. No crop: the model point that was at the center of the view before a
//      resize is still on screen after it (or, when the resize was too big
//      to keep it centered, the whole model still fits).
//   2. Fit quality: when the view is in auto fit, the model bounds fill a
//      sensible share of the pane - not left tiny in a corner. This is the
//      regression check for the bug where a burst of intermediate sizes
//      during the --dur-panel pane transition (2D -> Split, dock collapse,
//      a divider drag) left the plan fit to a small intermediate frame and
//      never refit as the pane kept growing. A view the user has zoomed
//      (auto fit off) must instead keep its exact scale across a resize.
//
// Runs the whole sequence at two viewport sizes: 1440x900 and 1100x700.
import { expect, log } from "./lib.mjs";

async function ready(page) {
  await page.waitForFunction(
    () => !!window.__app && !!window.__app.getState().doc && !!window.__planController && window.__planController.width > 0,
    { timeout: 15000 },
  );
  await page.waitForTimeout(150);
}

/** Canvas geometry: view, size, auto-fit state, and the model bounds of the open doc. */
async function geom(page) {
  return page.evaluate(() => {
    const c = window.__planController;
    return { view: { ...c.view }, width: c.width, height: c.height, autoFit: c.autoFit };
  });
}

function worldAtScreenCenter(g) {
  return {
    x: (g.width / 2 - g.view.ox) / g.view.scale,
    y: (g.view.oy - g.height / 2) / g.view.scale,
  };
}

function screenOf(g, world) {
  return { x: g.view.ox + world.x * g.view.scale, y: g.view.oy - world.y * g.view.scale };
}

async function modelBounds(page) {
  return page.evaluate(() => {
    const c = window.__planController;
    if (!c.index) return null;
    // Inline copy of model.ts:modelBounds, kept simple: walls + rooms only.
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    const expand = (p) => {
      minX = Math.min(minX, p.x);
      minY = Math.min(minY, p.y);
      maxX = Math.max(maxX, p.x);
      maxY = Math.max(maxY, p.y);
    };
    for (const w of c.index.doc.project.elements) {
      if (w.kind === "wall") {
        expand(w.start);
        expand(w.end);
      }
    }
    return { minX, minY, maxX, maxY };
  });
}

/** True when the model's walls all project inside the canvas viewport. */
function fitsOnScreen(g, bounds, marginPx = 4) {
  const corners = [
    { x: bounds.minX, y: bounds.minY },
    { x: bounds.minX, y: bounds.maxY },
    { x: bounds.maxX, y: bounds.minY },
    { x: bounds.maxX, y: bounds.maxY },
  ];
  return corners.every((c) => {
    const s = screenOf(g, c);
    return s.x >= -marginPx && s.x <= g.width + marginPx && s.y >= -marginPx && s.y <= g.height + marginPx;
  });
}

/** The larger of the model bounds' width/height fill of the pane, 0..1+. */
function fitFill(g, bounds) {
  const bw = bounds.maxX - bounds.minX;
  const bh = bounds.maxY - bounds.minY;
  const fillX = (bw * g.view.scale) / g.width;
  const fillY = (bh * g.view.scale) / g.height;
  return Math.max(fillX, fillY);
}

async function waitForSize(page, prevWidth, prevHeight) {
  await page.waitForFunction(
    ([pw, ph]) => {
      const c = window.__planController;
      return c && (c.width !== pw || c.height !== ph);
    },
    [prevWidth, prevHeight],
    { timeout: 4000 },
  ).catch(() => {}); // some transitions (dock collapse) may not change the plan's size at all
  await page.waitForTimeout(300); // let the flex-basis / grid CSS transition settle
}

/**
 * Waits past the resize-burst debounce (about one --dur-panel of silence)
 * and the settled refit's own ease, so `geom()` reflects the final, resting
 * view rather than a mid-transition frame.
 */
async function waitForSettle(page) {
  await page.waitForTimeout(700); // css transition + the --dur-panel debounce
  await page
    .waitForFunction(() => window.__planController && window.__planController.animIdle(), { timeout: 2000 })
    .catch(() => {});
  await page.waitForTimeout(50);
}

/** Checks that the model point centered before a resize is still visible after it, and the fit quality. */
async function checkNoCrop(page, label, shot, shotName, action) {
  const before = await geom(page);
  const centerWorld = worldAtScreenCenter(before);
  await action();
  await waitForSize(page, before.width, before.height);
  await waitForSettle(page);
  const after = await geom(page);
  await shot(shotName);

  const bounds = await modelBounds(page);
  const stillFits = bounds ? fitsOnScreen(after, bounds) : true;
  const fill = bounds ? fitFill(after, bounds) : null;
  const centerScreen = screenOf(after, centerWorld);
  const centerStillVisible = centerScreen.x >= 0 && centerScreen.x <= after.width && centerScreen.y >= 0 && centerScreen.y <= after.height;
  const resized = before.width !== after.width || before.height !== after.height;

  log(label, { before: { w: before.width, h: before.height }, after: { w: after.width, h: after.height }, resized, autoFit: after.autoFit, fill, stillFits });
  // The old center point should still be on screen (proves it was not left
  // anchored top-left and pushed off), and the whole model must still fit
  // (proves a resize too big to keep exactly centered falls back to a refit).
  expect(`${label}: old view center still on screen`, centerStillVisible, centerScreen);
  expect(`${label}: model still fits, no crop`, stillFits);
  if (after.autoFit && bounds && after.width > 300 && after.height > 300) {
    // The regression check: auto fit must not leave the plan tiny in a big
    // pane, nor overflow it. Skipped for panes narrow enough that fitRect's
    // fixed-pixel margin (70px each side) dominates the fill ratio on its
    // own (for example a split pane dragged down to a sliver) - that is an
    // honest small pane, not the bug (the bug left the plan tiny in a pane
    // that had grown back to a normal size).
    expect(`${label}: auto fit fills a sensible share of the pane (55%-95%)`, fill >= 0.55 && fill <= 0.95, { fill });
  }
  return after;
}

async function checkUserZoomKeepsScale(page, label, shot, shotName, action) {
  await page.evaluate(() => window.__planController.zoomStep(1));
  await page.waitForTimeout(100);
  const before = await geom(page);
  expect(`${label}: zoomStep turned auto fit off`, before.autoFit === false, { autoFit: before.autoFit });
  await action();
  await waitForSize(page, before.width, before.height).catch(() => {});
  await waitForSettle(page);
  const after = await geom(page);
  await shot(shotName);
  log(label, { before: before.view.scale, after: after.view.scale, autoFit: after.autoFit });
  expect(`${label}: auto fit stayed off`, after.autoFit === false, { autoFit: after.autoFit });
  expect(`${label}: user-zoomed scale is unchanged by the resize`, after.view.scale === before.view.scale, {
    before: before.view.scale,
    after: after.view.scale,
  });
}

async function runSequence(page, shot, prefix) {
  await page.goto("http://localhost:1552/?fixture=1", { waitUntil: "networkidle" });
  await ready(page);
  await shot(`${prefix}00-initial-2d`);

  // 1. 2D -> Split.
  await checkNoCrop(page, `${prefix} 2d -> split`, shot, `${prefix}01-after-split`, async () => {
    await page.keyboard.press("2");
  });

  // 2. Drag the split divider a long way, shrinking the plan pane a lot.
  await checkNoCrop(page, `${prefix} drag split divider`, shot, `${prefix}02-after-divider-drag`, async () => {
    const divider = await page.$('[aria-label="Resize plan and 3D views"]');
    const box = await divider.boundingBox();
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x - 400, box.y + box.height / 2, { steps: 10 });
    await page.mouse.up();
  });

  // Back to a clean split before the next scenario.
  await page.keyboard.press("2");
  await page.waitForTimeout(300);

  // 3. Collapse the side dock.
  await checkNoCrop(page, `${prefix} collapse dock`, shot, `${prefix}03-after-dock-collapse`, async () => {
    await page.click('[aria-label="Collapse the dock"]');
  });

  // 4. Split -> 2D only.
  await checkNoCrop(page, `${prefix} split -> 2d`, shot, `${prefix}04-back-to-2d`, async () => {
    await page.keyboard.press("1");
  });

  // 5. 2D -> 3D only (the plan pane disappears and comes back later; this
  // exercises the fitted/auto-fit reset path, not just a resize).
  await page.keyboard.press("3");
  await page.waitForTimeout(300);

  // 6. 3D -> Split (the plan pane reappears at a mid-size and must fit well).
  await checkNoCrop(page, `${prefix} 3d -> split`, shot, `${prefix}05-after-3d-to-split`, async () => {
    await page.keyboard.press("2");
  });

  // 7. Split -> 2D again.
  await checkNoCrop(page, `${prefix} split -> 2d again`, shot, `${prefix}06-back-to-2d-again`, async () => {
    await page.keyboard.press("1");
  });

  // 8. A user-zoomed view must keep its exact scale across a view mode
  // change and a dock collapse: auto fit must never come back on its own.
  await checkUserZoomKeepsScale(page, `${prefix} user zoom survives split`, shot, `${prefix}07-user-zoom-split`, async () => {
    await page.keyboard.press("2");
  });
  await checkUserZoomKeepsScale(page, `${prefix} user zoom survives dock collapse`, shot, `${prefix}08-user-zoom-dock`, async () => {
    await page.click('[aria-label="Expand the dock"]').catch(() => {});
  });

  // Pressing F (zoom to fit) turns auto fit back on.
  await page.keyboard.press("f");
  await waitForSettle(page);
  const refit = await geom(page);
  expect(`${prefix} F turns auto fit back on`, refit.autoFit === true, { autoFit: refit.autoFit });
}

export default async (page, shot) => {
  const viewports = [
    { width: 1440, height: 900, label: "1440x900" },
    { width: 1100, height: 700, label: "1100x700" },
  ];
  for (const vp of viewports) {
    await page.setViewportSize({ width: vp.width, height: vp.height });
    await runSequence(page, shot, `${vp.label}-`);
  }
};
