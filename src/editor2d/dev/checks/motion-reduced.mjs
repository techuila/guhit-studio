// Same canvas with prefers-reduced-motion: reduce. Every interaction must
// jump straight to its end state, the AI preview must not breathe, and the
// render loop must never keep a frame scheduled.
import { at, clickAt, expect, log, moveTo, ready, tool } from "./lib.mjs";

const anim = (page) =>
  page.evaluate(() => {
    const c = window.__planController;
    return { count: c.animCount(), idle: c.animIdle(), frames: c.frames, keys: c.animSnapshot() };
  });

export default async (page, shot) => {
  // The media query is read when src/ui/motion.ts loads, so emulate then reload.
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.reload({ waitUntil: "networkidle" });
  await ready(page, "bridge");

  const tokens = await page.evaluate(() => {
    const cs = getComputedStyle(document.documentElement);
    return ["--dur-press", "--dur-hover", "--dur-base", "--dur-panel", "--dur-scene"].map((k) => cs.getPropertyValue(k).trim());
  });
  log("duration tokens under reduced motion", tokens);
  expect("the CSS duration tokens are effectively zero", tokens.every((t) => t === "0.01ms"), tokens);

  // ---------------------------------------------------------------- hover and select
  await tool(page, "select");
  const p = await at(page, 4000, 0);
  await page.mouse.move(p.x - 200, p.y - 200);
  await page.waitForTimeout(120);
  await page.mouse.move(p.x, p.y, { steps: 2 });
  const hovered = await page.evaluate(() => window.__app.getState().hoverId);
  let a = await anim(page);
  log("hover", { hovered, count: a.count, value: a.keys[`hov:${hovered}`] });
  expect("hover is already at its end state, nothing is animating", a.keys[`hov:${hovered}`] === 1 && a.count === 0, {
    value: a.keys[`hov:${hovered}`],
    count: a.count,
  });

  await page.mouse.click(p.x, p.y);
  await page.waitForTimeout(80);
  const sel = await page.evaluate(() => window.__app.getState().selection[0]);
  a = await anim(page);
  log("select", { sel, count: a.count, outline: a.keys[`sel:${sel}`], grips: [a.keys["grip:0"], a.keys["grip:1"], a.keys["grip:2"]] });
  expect("the selection outline and every grip are fully drawn at once", a.keys[`sel:${sel}`] === 1 && a.keys["grip:0"] === 1 && a.keys["grip:2"] === 1, a.keys);
  expect("no stagger is left running", a.count === 0 && a.idle, { count: a.count, idle: a.idle });
  await shot("r01-select-no-motion");

  // ---------------------------------------------------------------- snapping
  await tool(page, "wall");
  await moveTo(page, 40, 5960);
  a = await anim(page);
  expect("the snap glyph is at full size immediately, no pop", a.keys["snap.pop"] === 1 && a.keys["snap.a"] === 1 && a.count === 0, {
    pop: a.keys["snap.pop"],
    alpha: a.keys["snap.a"],
    count: a.count,
  });
  await page.keyboard.press("Escape");

  // ---------------------------------------------------------------- tool ghost and flip
  await tool(page, "door");
  await moveTo(page, 4000, 0);
  a = await anim(page);
  expect("the tool ghost is fully drawn as soon as it has a target", a.keys["ghost.o"] === 1 && a.count === 0, { ghost: a.keys["ghost.o"], count: a.count });
  const swingBefore = a.keys["ghost.swing"];
  await page.keyboard.press("f");
  await page.waitForTimeout(40);
  a = await anim(page);
  expect("F flips the swing instantly, with no sweep", a.keys["ghost.swing"] === -swingBefore && a.count === 0, { before: swingBefore, now: a.keys["ghost.swing"] });
  await shot("r02-door-ghost-flipped");

  // ---------------------------------------------------------------- place and undo
  const before = await page.evaluate(() => window.__app.getState().doc.project.elements.length);
  await clickAt(page, 4000, 0);
  await page.waitForFunction((n) => window.__app.getState().doc.project.elements.length !== n, before, { timeout: 5000 });
  await page.waitForTimeout(80);
  a = await anim(page);
  const settling = Object.keys(a.keys).filter((k) => k.startsWith("add:") || k.startsWith("fl:") || k.startsWith("rm:"));
  log("after placing", { settling, count: a.count });
  expect("a placed element is simply there: no settle is left running", a.count === 0 && a.idle, { count: a.count, idle: a.idle, settling });
  await shot("r03-placed-no-settle");

  await tool(page, "select");
  await page.click('[data-testid="undo"]');
  await page.waitForFunction((n) => window.__app.getState().doc.project.elements.length === n, before, { timeout: 5000 });
  await page.waitForTimeout(120);
  a = await anim(page);
  expect("undo removes the element with no fade out", a.count === 0 && a.idle, { count: a.count, idle: a.idle });

  // ---------------------------------------------------------------- view
  await page.mouse.move(400, 300);
  await page.mouse.wheel(0, -600);
  await page.waitForTimeout(80);
  await page.evaluate(() => window.__bus.emit("zoom_to_fit"));
  const v = await page.evaluate(() => ({
    view: { ...window.__planController.view },
    drawn: { ...window.__planController.drawView() },
    count: window.__planController.animCount(),
  }));
  log("zoom to fit", v);
  expect("zoom to fit lands immediately, nothing eases", v.drawn.scale === v.view.scale && v.drawn.ox === v.view.ox && v.count === 0, v);
  await shot("r04-fit-instant");

  // ---------------------------------------------------------------- AI preview
  await page.evaluate(() => {
    const app = window.__app;
    const next = structuredClone(app.getState().doc);
    const w = next.project.elements.find((e) => e.kind === "wall");
    app.getState().setPreview({ state: next, diff: { added: [], modified: [w.id], removed: [], summary: "reduced motion check" } });
  });
  await page.waitForTimeout(150);
  const breath = await page.evaluate(async () => {
    const c = window.__planController;
    const f1 = c.frames;
    await new Promise((r) => setTimeout(r, 500));
    return { f1, f2: c.frames, idle: c.animIdle() };
  });
  log("preview over 500ms", breath);
  expect("the AI preview does not breathe: it is a steady tint and the loop stops", breath.f2 === breath.f1 && breath.idle, breath);
  await shot("r05-preview-steady");
  await page.evaluate(() => window.__app.getState().setPreview(null));

  // ---------------------------------------------------------------- nothing spins
  await page.waitForTimeout(150);
  const idleCheck = await page.evaluate(async () => {
    const c = window.__planController;
    const f1 = c.frames;
    await new Promise((r) => setTimeout(r, 400));
    return { f1, f2: c.frames, raf: c.animIdle() };
  });
  log("idle over 400ms", idleCheck);
  expect("no requestAnimationFrame is left scheduled", idleCheck.f2 === idleCheck.f1 && idleCheck.raf, idleCheck);
};
