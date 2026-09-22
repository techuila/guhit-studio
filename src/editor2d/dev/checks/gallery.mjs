// Fixture mode: injects one of every catalog symbol, stairs, columns, text,
// dimensions at angles, all opening styles and a camera, then screenshots.
// Also exercises the AI preview tint, layer visibility and capturePlan.
import { expect, log, ready } from "./lib.mjs";

const CATALOG = [
  ["bed-single", 920, 1900], ["bed-double", 1370, 1900], ["bed-queen", 1520, 2030], ["wardrobe", 1200, 600],
  ["sofa-3", 2100, 900], ["sofa-2", 1500, 900], ["armchair", 850, 850], ["coffee-table", 1100, 600],
  ["tv-console", 1600, 450], ["dining-4", 1200, 800], ["dining-6", 1800, 900], ["desk", 1200, 600],
  ["wc", 400, 700], ["lavatory", 500, 420], ["shower", 900, 900], ["bathtub", 1500, 750],
  ["kitchen-counter", 1800, 600], ["kitchen-sink", 1200, 600], ["range", 600, 600], ["refrigerator", 700, 700],
  ["washing-machine", 600, 600], ["plant-pot", 500, 500], ["tree", 3000, 3000], ["car-sedan", 1800, 4500],
  ["mystery-item", 1000, 700],
];

export default async (page, shot) => {
  await ready(page, "fixture");
  await page.evaluate((catalog) => {
    const app = window.__app;
    const doc = structuredClone(app.getState().doc);
    const level = doc.project.levels[0].id;
    const els = [];
    let x = 0;
    let y = -4000;
    catalog.forEach(([key, w, d], i) => {
      if (i % 9 === 0 && i > 0) { x = 0; y -= 5200; }
      els.push({ kind: "asset", id: "sym-" + i, level_id: level, catalog_key: key, name: key === "mystery-item" ? "Mystery item" : key, category: "furniture", position: { x: x + w / 2, y }, rotation_deg: 0, width_mm: w, depth_mm: d, height_mm: 500, elevation_mm: 0 });
      els.push({ kind: "annotation", id: "lbl-" + i, level_id: level, position: { x, y: y - Math.max(d / 2, 600) - 600 }, text: key, size_mm: 180, rotation_deg: 0 });
      x += w + 900;
    });
    els.push({ kind: "stair", id: "st-1", level_id: level, origin: { x: 10500, y: 500 }, rotation_deg: 0, width_mm: 1000, run_mm: 4000, riser_count: 16 });
    els.push({ kind: "stair", id: "st-2", level_id: level, origin: { x: 12500, y: 3000 }, rotation_deg: -90, width_mm: 900, run_mm: 3000, riser_count: 12 });
    els.push({ kind: "column", id: "col-1", level_id: level, center: { x: 9500, y: 5500 }, shape: "rect", width_mm: 300, depth_mm: 400, rotation_deg: 0, material_id: null });
    els.push({ kind: "column", id: "col-2", level_id: level, center: { x: 9500, y: 4500 }, shape: "round", width_mm: 350, depth_mm: 350, rotation_deg: 0, material_id: null });
    els.push({ kind: "dimension", id: "dim-2", level_id: level, a: { x: 8000, y: 0 }, b: { x: 8000, y: 6000 }, offset_mm: -700, text_override: null });
    els.push({ kind: "dimension", id: "dim-3", level_id: level, a: { x: 10000, y: 6500 }, b: { x: 13000, y: 8000 }, offset_mm: 500, text_override: null });
    els.push({ kind: "annotation", id: "note-1", level_id: level, position: { x: 200, y: 7000 }, text: "GROUND FLOOR PLAN\nScale 1:100", size_mm: 300, rotation_deg: 0 });
    els.push({ kind: "camera", id: "cam-2", name: "Living view", preset: "custom", position: { x: 1000, y: 1000, z: 1600 }, target: { x: 3500, y: 4000, z: 1600 }, fov_deg: 60 });
    els.push({ kind: "underlay", id: "ul-1", level_id: level, file_name: "missing-plan.png", position: { x: -6000, y: 0 }, width_px: 400, height_px: 500, mm_per_px: 10, scale_confirmed: false, rotation_deg: 0, opacity: 0.5, locked: false });
    // every opening style on the long south wall of a new strip
    els.push({ kind: "wall", id: "w-strip", level_id: level, start: { x: 0, y: 9500 }, end: { x: 13000, y: 9500 }, thickness_mm: 150, height_mm: null, material_id: null });
    const styles = [["door", "swing_single", 900, false, false], ["door", "swing_single", 900, true, true], ["door", "swing_double", 1500, false, false], ["door", "sliding", 1800, false, false], ["window", "sliding", 1200, false, false], ["window", "fixed", 1200, false, false], ["window", "casement", 1200, false, false], ["window", "jalousie", 1200, false, false]];
    styles.forEach(([t, st, w, fs, fh], i) => {
      els.push({ kind: "opening", id: "op-" + i, wall_id: "w-strip", opening_type: t, style: st, offset_mm: 900 + i * 1600, width_mm: w, height_mm: 2100, sill_mm: 0, flip_side: fs, flip_hinge: fh, material_id: null });
    });
    doc.project.elements.push(...els);
    app.getState().setDoc(doc);
    window.__bus.emit("zoom_to_fit");
  }, CATALOG);
  await page.waitForTimeout(300);
  await shot("50-gallery-all");

  // Zoom on the symbols.
  await page.evaluate(() => window.__bus.emit("focus_elements", ["sym-0", "sym-8", "sym-17"]));
  await page.waitForTimeout(200);
  await shot("51-gallery-symbols-a");
  await page.evaluate(() => window.__bus.emit("focus_elements", ["sym-18", "sym-24", "sym-23"]));
  await page.waitForTimeout(200);
  await shot("52-gallery-symbols-b");
  await page.evaluate(() => window.__bus.emit("focus_elements", ["w-strip"]));
  await page.waitForTimeout(200);
  await shot("53-gallery-openings");
  await page.evaluate(() => window.__bus.emit("focus_elements", ["st-1", "st-2", "col-1", "dim-3"]));
  await page.waitForTimeout(200);
  await shot("54-gallery-stairs-dims");

  // AI preview: one added, one modified, one removed.
  await page.evaluate(() => {
    const app = window.__app;
    const real = app.getState().doc;
    const next = structuredClone(real);
    const bed = next.project.elements.find((e) => e.kind === "asset" && e.catalog_key === "bed-double" && !e.id.startsWith("sym"));
    bed.position = { x: 6900, y: 1500 };
    next.project.elements = next.project.elements.filter((e) => e.id !== "st-1");
    next.project.elements.push({ kind: "asset", id: "ai-sofa", level_id: next.project.levels[0].id, catalog_key: "sofa-3", name: "Sofa", category: "furniture", position: { x: 2500, y: 5400 }, rotation_deg: 0, width_mm: 2100, depth_mm: 900, height_mm: 800, elevation_mm: 0 });
    app.getState().setPreview({ state: next, diff: { added: ["ai-sofa"], modified: [bed.id], removed: ["st-1"], summary: "test" } });
    window.__bus.emit("zoom_to_fit");
    window.__bus.emit("focus_elements", [bed.id, "st-1", "ai-sofa"]);
  });
  await page.waitForTimeout(200);
  await shot("55-ai-preview");
  await page.evaluate(() => window.__app.getState().setPreview(null));

  // Layer visibility and lock.
  const r = await page.evaluate(async () => {
    const app = window.__app;
    const doc = structuredClone(app.getState().doc);
    doc.project.layers = doc.project.layers.map((l) => (l.key === "assets" ? { ...l, visible: false } : l.key === "walls" ? { ...l, locked: true } : l));
    app.getState().setDoc(doc);
    window.__bus.emit("zoom_to_fit");
    await new Promise((r) => setTimeout(r, 100));
    return true;
  });
  await page.waitForTimeout(200);
  await shot("56-layers-assets-hidden");
  // Click a wall on the locked layer: must not select.
  const p = await page.evaluate(() => {
    const c = window.__planController;
    const rect = document.querySelector('[data-testid="plan-canvas"] canvas').getBoundingClientRect();
    return { x: rect.left + c.view.ox + 6500 * c.view.scale, y: rect.top + c.view.oy - 0 * c.view.scale };
  });
  await page.mouse.click(p.x, p.y);
  await page.waitForTimeout(100);
  const sel = await page.evaluate(() => window.__app.getState().selection);
  expect("locked wall layer is not selectable", sel.length === 0, sel);
  void r;

  // capturePlan
  const cap = await page.evaluate(async () => {
    const fn = window.__app.getState().capturePlan;
    if (!fn) return null;
    const url = await fn();
    const img = new Image();
    img.src = url;
    await img.decode();
    // show it for the screenshot
    img.style.cssText = "position:fixed;inset:40px;max-width:calc(100% - 80px);max-height:calc(100% - 80px);border:2px solid #c0392b;z-index:99;background:#fff";
    document.body.appendChild(img);
    return { prefix: url.slice(0, 22), w: img.naturalWidth, h: img.naturalHeight, bytes: url.length };
  });
  log("capturePlan", cap);
  expect("capturePlan returns a PNG data URL", !!cap && cap.prefix === "data:image/png;base64," && cap.w > 1000, cap);
  await page.waitForTimeout(100);
  await shot("57-capture-plan");
};
