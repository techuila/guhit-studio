// Bridge, sample bungalow: every 2D canvas row of the docs/MOTION.md inventory.
// Motion cannot be seen in one still, so this check asserts the animation
// registry instead: an interaction starts a named animation, it finishes, and
// the render loop goes idle afterwards (render on demand, no rAF spinning).
// Mid-animation screenshots are saved next to the assertions for review.
import { at, clickAt, expect, log, moveTo, ready, state, tool } from "./lib.mjs";

const anim = (page) =>
  page.evaluate(() => {
    const c = window.__planController;
    return { count: c.animCount(), idle: c.animIdle(), frames: c.frames, keys: c.animSnapshot() };
  });

const keysWith = (a, prefix) => Object.keys(a.keys).filter((k) => k.startsWith(prefix));

/** Waits for any animation key with this prefix to appear. Commands are async. */
async function waitForKey(page, prefix, label) {
  const found = await page
    .waitForFunction((p) => Object.keys(window.__planController.animSnapshot()).some((k) => k.startsWith(p)), prefix, { timeout: 5000 })
    .then(() => true)
    .catch(() => false);
  const a = await anim(page);
  expect(label, found, { keys: keysWith(a, prefix) });
  return a;
}

/**
 * Waits for a key to be moving away from `1` (headless draws about 25 fps, so
 * a fixed sleep can land between two frames and read the start value).
 */
async function waitForBelow(page, key, label) {
  const ok = await page
    .waitForFunction((k) => {
      const v = window.__planController.animValue(k, 0);
      return v < 0.999;
    }, key, { timeout: 4000 })
    .then(() => true)
    .catch(() => false);
  expect(label, ok, { [key]: await page.evaluate((k) => window.__planController.animValue(k, null), key) });
}

/** Waits until nothing is animating and no frame is scheduled. */
async function settle(page, label) {
  await page.waitForFunction(() => window.__planController.animCount() === 0 && window.__planController.animIdle(), null, { timeout: 4000 }).catch(() => {});
  const a = await anim(page);
  expect(`${label}: render loop is idle afterwards`, a.count === 0 && a.idle, { count: a.count, idle: a.idle });
  return a;
}

export default async (page, shot) => {
  await ready(page, "bridge");
  await settle(page, "start");

  // ---------------------------------------------------------------- hover
  await tool(page, "select");
  const wall = await page.evaluate(() => window.__app.getState().doc.project.elements.find((e) => e.kind === "wall").id);
  const p = await at(page, 4000, 0);
  await page.mouse.move(p.x - 200, p.y - 200);
  await page.waitForTimeout(200);
  await page.mouse.move(p.x, p.y, { steps: 2 });
  const hovered = await page.evaluate(() => window.__app.getState().hoverId);
  let a = await anim(page);
  log("hover keys", { hovered, keys: keysWith(a, "hov:") });
  expect("hover starts a tint fade", !!hovered && keysWith(a, "hov:").includes(`hov:${hovered}`) && a.count > 0, { count: a.count, keys: keysWith(a, "hov:") });
  await page.waitForTimeout(55);
  await shot("m01-hover-mid");
  a = await settle(page, "hover");
  expect("hover tint lands at full strength", Math.abs(a.keys[`hov:${hovered}`] - 1) < 1e-6, a.keys[`hov:${hovered}`]);

  // ---------------------------------------------------------------- select + grips
  await page.mouse.click(p.x, p.y);
  await page.waitForTimeout(20);
  const sel = await page.evaluate(() => window.__app.getState().selection[0]);
  a = await anim(page);
  log("select keys", { id: sel, sel: keysWith(a, "sel:"), grip: keysWith(a, "grip:"), count: a.count });
  expect("selecting fades the outline in and scales the grips in", a.keys[`sel:${sel}`] < 1 && a.keys["grip:0"] < 1 && a.count > 0, {
    sel: a.keys[`sel:${sel}`],
    grip0: a.keys["grip:0"],
  });
  expect("grips are staggered: the second starts after the first", a.keys["grip:1"] < a.keys["grip:0"], { g0: a.keys["grip:0"], g1: a.keys["grip:1"] });
  await page.waitForTimeout(55);
  await shot("m02-select-grips-mid");
  a = await settle(page, "select");
  expect("selection outline and grips land at 1", a.keys[`sel:${sel}`] === 1 && a.keys["grip:0"] === 1, { sel: a.keys[`sel:${sel}`], grip0: a.keys["grip:0"] });

  // deselect fades out
  await clickAt(page, -1500, 3000);
  await page.waitForTimeout(20);
  a = await anim(page);
  expect("deselect fades the outline out", a.keys[`sel:${sel}`] === undefined || a.keys[`sel:${sel}`] < 1, a.keys[`sel:${sel}`]);
  expect("deselecting scales the grips back out", a.keys["grip:0"] === undefined || a.keys["grip:0"] < 1, a.keys["grip:0"]);
  await settle(page, "deselect");

  // ---------------------------------------------------------------- snapping
  await tool(page, "wall");
  await moveTo(page, 40, 5960); // wall corner: endpoint snap
  a = await anim(page);
  log("snap engage", { pop: a.keys["snap.pop"], alpha: a.keys["snap.a"] });
  expect("a new snap pops the glyph from 0.6 and fades it in", a.keys["snap.pop"] !== undefined && a.keys["snap.a"] !== undefined, a.keys);
  await shot("m03-snap-pop-mid");
  await settle(page, "snap");
  a = await anim(page);
  expect("the snap glyph settles at full size", Math.abs(a.keys["snap.pop"] - 1) < 1e-6 && Math.abs(a.keys["snap.a"] - 1) < 1e-6, {
    pop: a.keys["snap.pop"],
    alpha: a.keys["snap.a"],
  });
  // Switching to another snap target must retarget, not pop again.
  await moveTo(page, 8030, 3020); // midpoint of the east wall
  a = await anim(page);
  expect("switching snap targets retargets instead of restarting the pop", a.keys["snap.pop"] === 1 && a.keys["snap.a"] === 1, {
    pop: a.keys["snap.pop"],
    alpha: a.keys["snap.a"],
  });
  // Leaving the canvas releases the snap for good (inside it, the grid still snaps).
  await page.mouse.move(700, 8);
  await waitForBelow(page, "snap.a", "releasing a snap fades the glyph out");
  await settle(page, "snap release");
  a = await anim(page);
  expect("the faded out snap track is forgotten, nothing is left running", a.keys["snap.a"] === undefined, a.keys["snap.a"]);
  await page.keyboard.press("Escape");
  await tool(page, "select");

  // ---------------------------------------------------------------- tool ghost fade + flip
  await tool(page, "door");
  await moveTo(page, 4000, 0);
  a = await anim(page);
  expect("a tool ghost fades in when it finds a valid target", a.keys["ghost.o"] !== undefined, a.keys["ghost.o"]);
  await shot("m04-door-ghost-mid");
  await settle(page, "door ghost");
  const swingBefore = (await anim(page)).keys["ghost.swing"];
  await page.keyboard.press("f");
  const swept = await page
    .waitForFunction(() => Math.abs(window.__planController.animValue("ghost.swing", 1)) < 0.999, null, { timeout: 2000 })
    .then(() => true)
    .catch(() => false);
  a = await anim(page);
  log("swing after F", { before: swingBefore, now: a.keys["ghost.swing"] });
  expect("F sweeps the swing through the wall plane instead of jumping", swept, { before: swingBefore, now: a.keys["ghost.swing"] });
  await shot("m05-door-flip-mid");
  await settle(page, "door flip");
  a = await anim(page);
  expect("the swing lands on the other side", a.keys["ghost.swing"] === -swingBefore, a.keys["ghost.swing"]);
  await moveTo(page, -4000, 4000); // no wall under the cursor any more
  await page.waitForTimeout(25);
  a = await anim(page);
  log("ghost after losing the target", { ghost: a.keys["ghost.o"] });
  expect("losing the target fades the ghost out", a.keys["ghost.o"] === undefined || a.keys["ghost.o"] < 1, a.keys["ghost.o"]);
  await settle(page, "ghost out");

  // ---------------------------------------------------------------- place, delete, undo, redo
  const before = await page.evaluate(() => window.__app.getState().doc.project.elements.map((e) => e.id));
  await clickAt(page, 4000, 0);
  await page.waitForFunction((ids) => window.__app.getState().doc.project.elements.length !== ids.length, before, { timeout: 5000 });
  const placed = await page.evaluate((ids) => window.__app.getState().doc.project.elements.map((e) => e.id).find((id) => !ids.includes(id)), before);
  a = await anim(page);
  log("placed", { id: placed, add: a.keys[`add:${placed}`], count: a.count });
  expect("a placed element settles with an add animation", a.keys[`add:${placed}`] !== undefined, a.keys[`add:${placed}`]);
  await shot("m06-place-settle-mid");
  await settle(page, "place");

  await tool(page, "select");
  await page.click('[data-testid="undo"]');
  await waitForKey(page, "rm:", "undo fades the removed element out from its last geometry");
  await shot("m07-undo-remove-mid");
  await settle(page, "undo");

  await page.click('[data-testid="redo"]');
  await waitForKey(page, "add:", "redo settles what came back");
  await shot("m08-redo-flash-mid");
  await settle(page, "redo");

  // ---------------------------------------------------------------- drag lift and drop
  await tool(page, "select");
  await moveTo(page, 5000, 4500);
  const gp = await at(page, 5000, 4500);
  await page.mouse.click(gp.x, gp.y); // the interior partition, no openings on it
  await page.waitForTimeout(70);
  await shot("m08b-wall-grips-stagger-mid"); // the three grips scaling in, staggered
  await settle(page, "select the partition");
  let s = await state(page);
  expect("partition selected for the drag", s.selection.length === 1, s.selection);
  const from = await at(page, 5000, 4500);
  const to = await at(page, 5600, 4500);
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(from.x + 10, from.y, { steps: 2 });
  await page.waitForTimeout(25);
  a = await anim(page);
  const opKind = await page.evaluate(() => window.__planController.op.kind);
  log("pick up", { op: opKind, lift: a.keys["drag.lift"] });
  expect("picking a drag up lifts it (shadow and tint fade in)", opKind === "move" && a.keys["drag.lift"] > 0, { op: opKind, lift: a.keys["drag.lift"] });
  await shot("m09-drag-lift-mid");
  await page.mouse.move(to.x, to.y, { steps: 6 });
  // 1:1 tracking: the ghost delta follows the pointer with no easing or lag.
  const tracking = await page.evaluate(() => {
    const op = window.__planController.op;
    return op.kind === "move" ? op.delta : null;
  });
  log("drag delta while dragging", tracking);
  expect("the drag itself tracks 1:1 with no smoothing", !!tracking && Math.abs(tracking.x - 600) < 120 && Math.abs(tracking.y) < 120, tracking);
  await page.mouse.up();
  await waitForBelow(page, "drag.lift", "dropping puts the element back down");
  // The drop settles through the same document diff: changed walls flash once
  // and the rooms whose area moved cross fade their readout.
  a = await waitForKey(page, "fl:", "the dropped and stretched walls flash once");
  await waitForKey(page, "area:", "a room whose area changed cross fades its readout");
  await shot("m10-drop-settle-mid");
  await settle(page, "drop");

  // ---------------------------------------------------------------- rejected drop eases back
  await page.route("**/ipc/doc_apply", (route) => route.fulfill({ status: 400, contentType: "application/json", body: JSON.stringify({ code: "invalid", message: "Simulated failure", element_ids: [] }) }));
  const f2 = await at(page, 5600, 4500);
  const t2 = await at(page, 6400, 4500);
  await page.mouse.move(f2.x, f2.y);
  await page.mouse.down();
  await page.mouse.move(t2.x, t2.y, { steps: 6 });
  await page.mouse.up();
  const rejected = await page
    .waitForFunction(() => window.__planController.returningOp() !== null, null, { timeout: 5000 })
    .then(() => true)
    .catch(() => false);
  a = await anim(page);
  const returning = await page.evaluate(() => {
    const op = window.__planController.returningOp();
    return op ? { kind: op.kind, delta: op.delta } : null;
  });
  log("rejected drop", { back: a.keys["drag.back"], returning });
  expect("a rejected drop eases its ghost back to the origin", rejected && a.keys["drag.back"] !== undefined, { back: a.keys["drag.back"], returning });
  await shot("m11-rejected-ease-back-mid");
  await settle(page, "rejected drop");
  s = await state(page);
  expect("the canvas is idle after the rejection", s.op === "idle", s.op);
  await page.unroute("**/ipc/doc_apply");

  // ---------------------------------------------------------------- marquee fade out
  const ma = await at(page, -2000, 1000);
  const mb = await at(page, -800, 3000);
  await page.mouse.move(ma.x, ma.y);
  await page.mouse.down();
  await page.mouse.move(mb.x, mb.y, { steps: 6 });
  await page.waitForTimeout(20);
  await page.mouse.up();
  await waitForBelow(page, "marquee", "the marquee fades out on release");
  await shot("m11b-marquee-fade-mid");
  await settle(page, "marquee");

  // ---------------------------------------------------------------- view ease
  // Move the view away first, so the fit has somewhere to travel.
  await page.mouse.move(400, 300);
  await page.mouse.wheel(0, -600);
  await page.waitForTimeout(80);
  await page.evaluate(() => {
    window.__planController.frames = 0;
    window.__bus.emit("zoom_to_fit");
  });
  await page.waitForTimeout(20);
  const v = await page.evaluate(() => ({
    target: { ...window.__planController.view },
    drawn: { ...window.__planController.drawView() },
    running: window.__planController.animCount(),
  }));
  log("view ease", v);
  expect("zoom to fit eases what is drawn while the target view is already final", v.running > 0 && Math.abs(v.drawn.scale - v.target.scale) > 1e-9, v);
  await shot("m12-view-ease-mid");
  await settle(page, "zoom to fit");
  const v2 = await page.evaluate(() => ({ target: { ...window.__planController.view }, drawn: { ...window.__planController.drawView() } }));
  expect("the view ease lands exactly on the target", v2.drawn.scale === v2.target.scale && v2.drawn.ox === v2.target.ox, v2);

  // focus_elements eases the same way
  await page.evaluate((id) => window.__bus.emit("focus_elements", [id]), wall);
  await page.waitForTimeout(20);
  a = await anim(page);
  expect("focus_elements eases the view too", a.keys["view"] !== undefined && a.count > 0, { view: a.keys["view"], count: a.count });
  await settle(page, "focus elements");

  // a wheel always wins: it is immediate and 1:1
  await page.evaluate(() => window.__bus.emit("zoom_to_fit"));
  await page.waitForTimeout(20);
  const beforeWheel = await page.evaluate(() => ({ ...window.__planController.view }));
  await page.mouse.move(720, 450);
  await page.mouse.wheel(0, -300);
  await page.waitForTimeout(30);
  const afterWheel = await page.evaluate(() => ({
    view: { ...window.__planController.view },
    drawn: { ...window.__planController.drawView() },
    hasView: window.__planController.animValue("view", -1),
  }));
  log("wheel during a view ease", { beforeWheel, afterWheel });
  expect(
    "a wheel interrupts the ease and stays 1:1 (drawn equals the target)",
    afterWheel.drawn.scale === afterWheel.view.scale && afterWheel.view.scale > beforeWheel.scale,
    afterWheel,
  );
  await settle(page, "wheel");

  // ---------------------------------------------------------------- keyboard zoom
  const kz = await page.evaluate(() => ({ ...window.__planController.view }));
  await moveTo(page, 4000, 3000);
  await page.keyboard.press("=");
  await page.waitForTimeout(15);
  const kzMid = await page.evaluate(() => ({
    view: { ...window.__planController.view },
    drawn: { ...window.__planController.drawView() },
    count: window.__planController.animCount(),
  }));
  log("keyboard zoom", { before: kz.scale, target: kzMid.view.scale, drawn: kzMid.drawn.scale });
  expect("a keyboard zoom step eases, and lands on its target at once", kzMid.view.scale > kz.scale * 1.2 && kzMid.count > 0, kzMid);
  await settle(page, "keyboard zoom");
  await page.keyboard.press("-");
  await settle(page, "keyboard zoom out");

  // ---------------------------------------------------------------- DOM overlays
  await tool(page, "wall");
  await clickAt(page, 2000, 2000);
  await moveTo(page, 3500, 2000);
  await page.keyboard.type("4000");
  // It mounts at stage "enter" (scaled down and transparent) and switches to
  // "idle" two frames later, which is what makes the CSS transition run.
  const entering = await page
    .waitForFunction(
      () => {
        const el = document.querySelector('[data-testid="typed-box"]');
        return !!el && el.dataset.stage === "enter" && getComputedStyle(el).opacity === "0";
      },
      null,
      { timeout: 2000 },
    )
    .then(() => true)
    .catch(() => false);
  expect("the type-to-precise box mounts scaled down and transparent", entering, entering);
  const settled = await page
    .waitForFunction(
      () => {
        const el = document.querySelector('[data-testid="typed-box"]');
        return !!el && el.dataset.stage === "idle" && parseFloat(getComputedStyle(el).opacity) > 0.99;
      },
      null,
      { timeout: 2000 },
    )
    .then(() => true)
    .catch(() => false);
  const dom = await page.evaluate(() => {
    const el = document.querySelector('[data-testid="typed-box"]');
    const hint = document.querySelector('[class*="hint"]');
    return el ? { stage: el.dataset.stage, opacity: getComputedStyle(el).opacity, hint: hint ? hint.dataset.stage : null } : null;
  });
  log("typed box once it has scaled in", dom);
  expect("the type-to-precise box scales and fades in to its resting state", settled, dom);
  expect("the hint pill carries a motion stage too", !!dom && dom.hint === "idle", dom);
  await shot("m14-typed-box");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  // It must still be in the DOM, at stage "exit", before it is unmounted.
  const exiting = await page
    .waitForFunction(
      () => {
        const el = document.querySelector('[data-testid="typed-box"]');
        return !!el && el.dataset.stage === "exit";
      },
      null,
      { timeout: 1000 },
    )
    .then(() => true)
    .catch(() => false);
  log("typed box exit stage seen", exiting);
  expect("it stays mounted to play its exit instead of popping out", exiting, exiting);
  await page.waitForTimeout(300);
  const unmounted = await page.evaluate(() => !!document.querySelector('[data-testid="typed-box"]'));
  expect("and it is gone once the exit has played", unmounted === false, unmounted);
  await tool(page, "select");

  // ---------------------------------------------------------------- AI preview breathes, and only it loops
  await page.evaluate(() => {
    const app = window.__app;
    const next = structuredClone(app.getState().doc);
    const w = next.project.elements.find((e) => e.kind === "wall");
    app.getState().setPreview({ state: next, diff: { added: [], modified: [w.id], removed: [], summary: "motion check" } });
  });
  await page.waitForTimeout(120);
  const breath = await page.evaluate(async () => {
    const c = window.__planController;
    const read = () => ({ frames: c.frames, idle: c.animIdle() });
    const a1 = read();
    await new Promise((r) => setTimeout(r, 400));
    const a2 = read();
    return { a1, a2 };
  });
  log("preview breath", breath);
  expect("the AI preview keeps the loop running so it can breathe", breath.a2.frames > breath.a1.frames + 5, breath);
  await shot("m13-preview-breathing");
  await page.evaluate(() => window.__app.getState().setPreview(null));
  await page.waitForTimeout(150);
  const stopped = await page.evaluate(async () => {
    const c = window.__planController;
    const f1 = c.frames;
    await new Promise((r) => setTimeout(r, 400));
    return { f1, f2: c.frames, idle: c.animIdle() };
  });
  log("after the preview clears", stopped);
  expect("clearing the preview stops the loop dead", stopped.f2 === stopped.f1 && stopped.idle, stopped);

  // ---------------------------------------------------------------- render on demand
  await page.mouse.move(5, 5);
  await settle(page, "end");
  const idleCheck = await page.evaluate(async () => {
    const c = window.__planController;
    const f1 = c.frames;
    await new Promise((r) => setTimeout(r, 500));
    return { f1, f2: c.frames, raf: c.animIdle() };
  });
  log("idle over 500ms", idleCheck);
  expect("no requestAnimationFrame spinning: no frames drawn while nothing happens", idleCheck.f2 === idleCheck.f1 && idleCheck.raf, idleCheck);

  s = await state(page);
  log("final state", { op: s.op, toasts: s.toasts });
};
