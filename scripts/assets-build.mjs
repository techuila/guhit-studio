#!/usr/bin/env node
// Asset pack pipeline: downloads CC0 sources (Poly Haven, ambientCG, Kenney
// Furniture Kit) into assets/raw/ (git-ignored), processes them into
// public/assets/pack/ (ships in the app), and writes manifest.json.
//
// Idempotent: every download is skipped when the destination file already
// exists. Re-run any time; delete assets/raw/ or public/assets/pack/ to force
// a clean rebuild.
//
// Usage: node scripts/assets-build.mjs [--verify-only]

import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const RAW = path.join(ROOT, "assets", "raw");
const PACK = path.join(ROOT, "public", "assets", "pack");
const TMP = path.join(RAW, ".tmp");

const VERIFY_ONLY = process.argv.includes("--verify-only");

// ---------------------------------------------------------------------------
// Small utilities
// ---------------------------------------------------------------------------

function log(msg) {
  process.stdout.write(msg + "\n");
}

function fmtBytes(n) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(2)} MB`;
}

function ensureDir(p) {
  fs.mkdirSync(p, { recursive: true });
}

/**
 * Resolve a package that is only a transitive dependency under pnpm's
 * virtual store (node_modules/.pnpm), not hoisted to top-level node_modules.
 * @gltf-transform/cli pulls in @gltf-transform/core, @gltf-transform/functions
 * and sharp; we reuse those instead of adding new direct dependencies.
 */
function resolvePnpm(pkg) {
  const store = path.join(ROOT, "node_modules", ".pnpm");
  const flat = pkg.replace("/", "+");
  const dirs = fs.readdirSync(store).filter((d) => d.startsWith(flat + "@"));
  if (!dirs.length) {
    throw new Error(`Cannot find "${pkg}" under node_modules/.pnpm. Run "pnpm install" first.`);
  }
  // Prefer the entry that isn't itself nested under another package's node_modules.
  return require_.resolve(pkg, { paths: [path.join(store, dirs[0], "node_modules")] });
}
import { createRequire } from "node:module";
const require_ = createRequire(import.meta.url);

async function download(url, destPath, label) {
  ensureDir(path.dirname(destPath));
  if (fs.existsSync(destPath)) {
    const st = fs.statSync(destPath);
    log(`  skip   ${label ?? path.basename(destPath)} (cached, ${fmtBytes(st.size)}) <- ${url}`);
    return destPath;
  }
  const res = await fetch(url, { redirect: "follow" });
  if (!res.ok) throw new Error(`download failed ${res.status} ${res.statusText}: ${url}`);
  const buf = Buffer.from(await res.arrayBuffer());
  fs.writeFileSync(destPath, buf);
  log(`  fetch  ${label ?? path.basename(destPath)} (${fmtBytes(buf.length)}) <- ${url}`);
  return destPath;
}

// ---------------------------------------------------------------------------
// Minimal pure-JS ZIP reader (no shelling out to `unzip`/`tar`, no new deps).
// Supports store (0) and deflate (8) compression, which covers both ambientCG
// and Kenney's zips. Not a general-purpose implementation (no ZIP64, no
// multi-disk archives) but sufficient for the small archives this pipeline
// downloads.
// ---------------------------------------------------------------------------

function readZipEntries(buf) {
  const EOCD_SIG = 0x06054b50;
  let eocdOffset = -1;
  const minPos = Math.max(0, buf.length - 65557);
  for (let i = buf.length - 22; i >= minPos; i--) {
    if (buf.readUInt32LE(i) === EOCD_SIG) {
      eocdOffset = i;
      break;
    }
  }
  if (eocdOffset < 0) throw new Error("not a valid zip file (EOCD not found)");
  const cdEntries = buf.readUInt16LE(eocdOffset + 10);
  const cdOffset = buf.readUInt32LE(eocdOffset + 16);

  const entries = new Map();
  let p = cdOffset;
  for (let i = 0; i < cdEntries; i++) {
    const sig = buf.readUInt32LE(p);
    if (sig !== 0x02014b50) throw new Error(`bad central directory entry at ${p}`);
    const method = buf.readUInt16LE(p + 10);
    const compSize = buf.readUInt32LE(p + 20);
    const uncompSize = buf.readUInt32LE(p + 24);
    const nameLen = buf.readUInt16LE(p + 28);
    const extraLen = buf.readUInt16LE(p + 30);
    const commentLen = buf.readUInt16LE(p + 32);
    const localHeaderOffset = buf.readUInt32LE(p + 42);
    const name = buf.toString("utf8", p + 46, p + 46 + nameLen);
    p += 46 + nameLen + extraLen + commentLen;

    if (!name.endsWith("/")) {
      // Read the local header to find the actual data offset (its
      // name/extra lengths can differ subtly from the central directory).
      const lp = localHeaderOffset;
      const lNameLen = buf.readUInt16LE(lp + 26);
      const lExtraLen = buf.readUInt16LE(lp + 28);
      const dataStart = lp + 30 + lNameLen + lExtraLen;
      const raw = buf.subarray(dataStart, dataStart + compSize);
      let data;
      if (method === 0) data = raw;
      else if (method === 8) data = zlib.inflateRawSync(raw);
      else throw new Error(`unsupported zip compression method ${method} for ${name}`);
      if (data.length !== uncompSize) {
        throw new Error(`size mismatch extracting ${name}: got ${data.length}, expected ${uncompSize}`);
      }
      entries.set(name, data);
    }
  }
  return entries;
}

function extractZip(zipPath, destDir, { only } = {}) {
  const buf = fs.readFileSync(zipPath);
  const entries = readZipEntries(buf);
  const written = [];
  for (const [name, data] of entries) {
    if (only && !only(name)) continue;
    const dest = path.join(destDir, name);
    ensureDir(path.dirname(dest));
    fs.writeFileSync(dest, data);
    written.push(dest);
  }
  return written;
}

// ---------------------------------------------------------------------------
// gltf-transform CLI + core/functions API (via the pnpm-nested resolver)
// ---------------------------------------------------------------------------

const GLTF_TRANSFORM_BIN = path.join(ROOT, "node_modules", ".bin", "gltf-transform");

function gltfTransform(args) {
  execFileSync(GLTF_TRANSFORM_BIN, args, { stdio: ["ignore", "pipe", "pipe"] });
}

let _core, _fns, _ext, _meshopt, _io;
async function gltfCore() {
  if (!_core) _core = await import(resolvePnpm("@gltf-transform/core"));
  return _core;
}
async function gltfFunctions() {
  if (!_fns) _fns = await import(resolvePnpm("@gltf-transform/functions"));
  return _fns;
}
async function gltfExtensions() {
  if (!_ext) _ext = await import(resolvePnpm("@gltf-transform/extensions"));
  return _ext;
}
async function meshopt() {
  if (!_meshopt) _meshopt = await import(resolvePnpm("meshoptimizer"));
  return _meshopt;
}

/** A NodeIO that can read/write files produced by `gltf-transform optimize`
 * (EXT_meshopt_compression), not just plain glTF. */
async function makeIO() {
  if (_io) return _io;
  const core = await gltfCore();
  const ext = await gltfExtensions();
  const mo = await meshopt();
  await mo.MeshoptDecoder.ready;
  await mo.MeshoptEncoder.ready;
  _io = new core.NodeIO()
    .registerExtensions(ext.ALL_EXTENSIONS)
    .registerDependencies({ "meshopt.decoder": mo.MeshoptDecoder, "meshopt.encoder": mo.MeshoptEncoder });
  return _io;
}

let _sharp;
async function sharp() {
  if (!_sharp) {
    const mod = await import(resolvePnpm("sharp"));
    _sharp = mod.default ?? mod;
  }
  return _sharp;
}

// ---------------------------------------------------------------------------
// CONTRACT DATA: mirrors crates/guhit-model/src/defaults.rs. Keep in sync by
// hand; this pipeline does not parse Rust source. Sizes are millimeters.
// ---------------------------------------------------------------------------

const CATALOG = [
  { key: "bed-single", w: 920, d: 1900, h: 500 },
  { key: "bed-double", w: 1370, d: 1900, h: 500 },
  { key: "bed-queen", w: 1520, d: 2030, h: 500 },
  { key: "wardrobe", w: 1200, d: 600, h: 2100 },
  { key: "sofa-3", w: 2100, d: 900, h: 800 },
  { key: "sofa-2", w: 1500, d: 900, h: 800 },
  { key: "armchair", w: 850, d: 850, h: 800 },
  { key: "coffee-table", w: 1100, d: 600, h: 420 },
  { key: "tv-console", w: 1600, d: 450, h: 500 },
  { key: "dining-4", w: 1200, d: 800, h: 750 },
  { key: "dining-6", w: 1800, d: 900, h: 750 },
  { key: "desk", w: 1200, d: 600, h: 750 },
  { key: "wc", w: 400, d: 700, h: 780 },
  { key: "lavatory", w: 500, d: 420, h: 200 },
  { key: "shower", w: 900, d: 900, h: 50 },
  { key: "bathtub", w: 1500, d: 750, h: 550 },
  { key: "kitchen-counter", w: 1800, d: 600, h: 900 },
  { key: "kitchen-sink", w: 1200, d: 600, h: 900 },
  { key: "range", w: 600, d: 600, h: 900 },
  { key: "refrigerator", w: 700, d: 700, h: 1750 },
  { key: "washing-machine", w: 600, d: 600, h: 850 },
  { key: "plant-pot", w: 500, d: 500, h: 1200 },
  { key: "tree", w: 3000, d: 3000, h: 5000 },
  { key: "car-sedan", w: 1800, d: 4500, h: 1450 },
];

// Kenney Furniture Kit source file per catalog key. `parts` composes several
// pieces into one GLB (dining sets: a table plus chairs around it).
const KENNEY_ZIP_URL =
  "https://kenney.nl/media/pages/assets/furniture-kit/440e0608a4-1677580847/kenney_furniture-kit.zip";
const KENNEY_MODEL_DIR = "Models/GLTF format";

const KENNEY_SINGLE = {
  "bed-single": "bedSingle.glb",
  "bed-double": "bedDouble.glb",
  "bed-queen": "bedDouble.glb", // scaled up to queen footprint
  wardrobe: "bookcaseClosedWide.glb",
  "sofa-3": "loungeSofaLong.glb",
  "sofa-2": "loungeSofa.glb",
  "tv-console": "cabinetTelevision.glb",
  desk: "desk.glb",
  wc: "toilet.glb",
  lavatory: "bathroomSink.glb",
  shower: "shower.glb",
  bathtub: "bathtub.glb",
  "kitchen-counter": "kitchenCabinet.glb",
  "kitchen-sink": "kitchenSink.glb",
  range: "kitchenStove.glb",
  refrigerator: "kitchenFridge.glb",
  "washing-machine": "washer.glb",
};

// Kenney palette recolor. The kit's "carpet" material (a saturated salmon,
// raw baseColorFactor ~[0.943, 0.367, 0.343], #f15e57 read as sRGB hex) is
// reused as-is for both the sofa upholstery (loungeSofa.glb,
// loungeSofaLong.glb) and the bed mattress/bedding top (bedSingle.glb,
// bedDouble.glb), which is what makes the pack's furniture look like the
// stock Kenney kit instead of a calm interior render. Every other Kenney
// material used by this catalog (wood, the metal greys, the near-white
// "carpetWhite"/"metalLight"/"_defaultMat" fixture and appliance tones, the
// pale glass) is already muted enough to keep unchanged.
//
// Matched by the original baseColorFactor value (not material name), scoped
// per catalog key since the same salmon serves two different real-world
// materials depending on which model it is on.
const KENNEY_SALMON = [0.9433962, 0.367175967, 0.3426486]; // "carpet" material, #f15e57 read as sRGB
const KENNEY_RECOLOR_TARGETS = {
  upholstery: [0x8f / 255, 0x8a / 255, 0x82 / 255], // #8f8a82 warm grey
  bedding: [0xee / 255, 0xea / 255, 0xe2 / 255], // #eeeae2 off-white
};
const KENNEY_RECOLOR_BY_KEY = {
  "sofa-2": "upholstery",
  "sofa-3": "upholstery",
  "bed-single": "bedding",
  "bed-double": "bedding",
  "bed-queen": "bedding",
};

function colorsClose(a, b, eps = 0.01) {
  return Math.abs(a[0] - b[0]) < eps && Math.abs(a[1] - b[1]) < eps && Math.abs(a[2] - b[2]) < eps;
}

/** Recolors the Kenney kit's saturated "carpet" material in place, for the
 * catalog keys that need it. No-op for every other key (including Poly Haven
 * hero models, which never pass through here) and for materials that do not
 * match the salmon value. */
function recolorKenneyMaterials(doc, catalogKey) {
  const targetName = KENNEY_RECOLOR_BY_KEY[catalogKey];
  if (!targetName) return;
  const target = KENNEY_RECOLOR_TARGETS[targetName];
  for (const mat of doc.getRoot().listMaterials()) {
    const bc = mat.getBaseColorFactor();
    if (colorsClose(bc, KENNEY_SALMON)) {
      mat.setBaseColorFactor([target[0], target[1], target[2], bc[3]]);
    }
  }
}

// Poly Haven hero swaps: photoscanned CC0 models used in place of the Kenney
// piece for a handful of catalog keys where a good modern-styled model
// exists. See ASSETS.md for why the others (sofa, bed, tree, car) were not
// swapped.
const POLYHAVEN_MODELS = {
  armchair: "modern_arm_chair_01",
  "coffee-table": "modern_coffee_table_01",
  "plant-pot": "potted_plant_01",
};

// Extra `gltf-transform optimize` flags per catalog key. potted_plant_01's
// foliage is dense (153k triangles as photoscanned) and lands at ~2 MB with
// only meshopt + 1024px textures, over the ~1.5 MB per-hero budget; a mild
// simplify pass brings it to ~1.4 MB while keeping the leaf silhouette.
const OPTIMIZE_OVERRIDES = {
  "plant-pot": ["--simplify-ratio", "0.25", "--simplify-error", "0.02"],
};

// Catalog keys with no CC0 source in any of the three approved libraries.
// The app's existing procedural fallback in src/viewer3d/scene/assets.ts
// covers these; this pack intentionally ships no model for them.
const NO_SOURCE = {
  tree: "Kenney Furniture Kit has no tree; Poly Haven's tree models (e.g. island_tree_01) carry a ~58 MB shared .bin far outside budget.",
  "car-sedan": "None of the three approved sources publish a CC0 vehicle model.",
};

// ambientCG materials. `size` is the physical tile size in millimeters the
// texture should be tiled at, chosen to match the UV-repeat convention
// already used by the built-in procedural patterns in
// src/viewer3d/scene/materials.ts (chb 800x400, concrete 1200x600, tile
// 600x600, wood_plank 1800x600, roof_sheet 250x1000, roof_tile 600x660) so a
// later agent can drop these in with the same `repeat = 1 / size` math.
const MATERIALS = [
  {
    preset: "mat-chb-painted",
    name: "CHB, plastered and painted",
    ambientId: "PaintedPlaster017",
    size_mm: [1000, 1000],
    note: "Seamless swatch; no fixed module (flat painted finish).",
  },
  {
    preset: "mat-chb-bare",
    name: "CHB, bare",
    ambientId: "Bricks032",
    size_mm: [800, 400],
    note: "Grey masonry block texture (ambientCG has no dedicated concrete-hollow-block asset; the only 'cinder block' tagged one, Bricks088, is fired red clay, wrong color for grey CHB). A single CHB block is 400x200mm (two courses per tile, matching the app's procedural chb pattern).",
  },
  {
    preset: "mat-concrete-fairface",
    name: "Concrete, fair-faced",
    ambientId: "Concrete030",
    size_mm: [1200, 600],
    note: "Formwork panel scale, matching the app's procedural concrete pattern.",
  },
  {
    preset: "mat-floor-concrete",
    name: "Polished concrete",
    ambientId: "Concrete048",
    size_mm: [1200, 600],
    note: "Same tile scale as fair-faced concrete for a consistent pattern key.",
  },
  {
    preset: "mat-tile-ceramic",
    name: "Ceramic tile 600 x 600",
    ambientId: "Tiles107",
    // Tiles107's raw 1024x1024 photo is the WHOLE tile grid (grout lines
    // included), not one tile. An earlier version of this pipeline cropped a
    // 40x40px grout-free patch from one cell and upscaled it to 1024, which
    // produced a blank white color map and a flat normal map (no grout, no
    // pattern) that src/viewer3d/scene/pack.ts's blank-map detector correctly
    // rejected. Fixed by using the full, uncropped image: a column/row
    // grout-line scan (see scripts/assets-build.mjs history) found 7 interior
    // grout lines each direction at a steady 128px pitch, i.e. an 8x8 grid of
    // tiles across the 1024px image. Physical size is therefore 8 tiles x
    // 600mm = 4800mm per side.
    size_mm: [4800, 4800],
  },
  {
    preset: "mat-tile-granite",
    name: "Granite tile",
    ambientId: "Granite001A",
    size_mm: [600, 600],
  },
  {
    preset: "mat-floor-laminate",
    name: "Wood laminate (wood plank floor)",
    ambientId: "WoodFloor051",
    size_mm: [1800, 600],
  },
  {
    preset: "mat-wood-cladding",
    name: "Wood cladding",
    ambientId: "Wood092",
    size_mm: [1800, 600],
  },
  {
    preset: "mat-roof-longspan",
    name: "Long-span pre-painted metal roof (corrugated)",
    ambientId: "CorrugatedSteel009",
    // The rib pattern repeats down the image (its 512px-tall axis, which
    // physical_size_mm[1] scales): a displacement-map peak scan found 30 ribs
    // at a steady ~17px pitch. The old [250, 1000] size put physical_size_mm[1]
    // at 1000mm, i.e. a rendered pitch of 1000/30 = 33mm, far tighter than a
    // long-span panel's ~250mm rib pitch. Rescaled so one rib is 250mm: 30 x
    // 250mm = 7500mm on that axis; the other axis (1024x512px image, 2:1)
    // scales with the image aspect: 7500 x 2 = 15000mm.
    size_mm: [15000, 7500],
  },
  {
    preset: "mat-roof-gi",
    name: "GI corrugated sheet",
    // Metal049A was a plain brushed-metal swatch with no rib pattern baked
    // into the photo (color-map stdev ~1.5-1.9, essentially flat), which
    // src/viewer3d/scene/pack.ts's blank-map detector correctly rejected.
    // Replaced with CorrugatedSteel005, a bare/galvanized grey set with
    // visible corrugation (ambientCG has no CorrugatedSteel007; 007A/B/C are
    // painted blue, not bare GI grey).
    ambientId: "CorrugatedSteel005",
    // A displacement-map peak scan found 10 ribs at a steady ~102px pitch
    // across the 1024px-wide image (the source is square, 1024x1024), close
    // to a real corrugated-GI sheet (~10-11 corrugations at 76mm pitch, the
    // standard GI rib spacing). 10 ribs x 76mm = 760mm; the other axis keeps
    // the image's 1:1 aspect, so also 760mm.
    size_mm: [760, 760],
  },
  {
    preset: "mat-roof-clay-tile",
    name: "Clay roof tile",
    ambientId: "RoofingTiles006",
    size_mm: [600, 660],
    note: "Flat modern clay tile look (RoofingTiles013A, the barrel/Spanish-profile option, is a near-black underexposed photo unsuitable as-is).",
  },
];

// Presets with pattern "none" in defaults.rs (flat MeshStandardMaterial
// color, no texture map) plus glass. Recorded in manifest.json for
// completeness but no download is made.
const FLAT_PRESETS = [
  { preset: "mat-paint-warm-white", name: "Paint, warm white", color: "#faf6ee" },
  { preset: "mat-paint-sage", name: "Paint, sage green", color: "#b7c4ae" },
  { preset: "mat-aluminum-frame", name: "Aluminum frame, powder coated", color: "#3b3f45" },
  { preset: "mat-steel", name: "Steel, painted", color: "#4a4f57" },
  {
    preset: "mat-wood-door",
    name: "Wood door, mahogany finish",
    color: "#6e4428",
    note: "Reuse mat-wood-cladding's texture set if a textured door is needed later.",
  },
  {
    preset: "mat-roof-concrete-deck",
    name: "Concrete roof deck",
    color: "#c4c2bc",
    note: "Reuse mat-floor-concrete / mat-concrete-fairface's texture set if needed.",
  },
  { preset: "mat-glass-clear", name: "Clear glass", color: "#bfe3ee", note: "Glass has no texture map: transparency + roughness only." },
];

// HDRI: one outdoor sunny sky. Poly Haven's /info endpoint does not expose a
// sun azimuth/altitude value, so sun direction is recorded as "unknown"
// (rule stated up front in the brief).
const HDRI = {
  id: "alps_field",
  name: "Alps Field",
  license: "CC0 1.0",
  sourceUrl: "https://polyhaven.com/a/alps_field",
  sun_direction: "unknown (Poly Haven's API does not publish sun azimuth/altitude; tags: sun, high-contrast, partly-cloudy)",
};

// ---------------------------------------------------------------------------
// Step: HDRI
// ---------------------------------------------------------------------------

async function buildHdri() {
  log("\n== HDRI ==");
  const filesRes = await fetch(`https://api.polyhaven.com/files/${HDRI.id}`);
  const files = await filesRes.json();
  const hdrUrl = files.hdri["1k"].hdr.url;
  const tonemappedUrl = files.tonemapped.url;

  const rawHdr = path.join(RAW, "hdri", `${HDRI.id}_1k.hdr`);
  await download(hdrUrl, rawHdr, "alps_field_1k.hdr");
  const rawTonemapped = path.join(RAW, "hdri", `${HDRI.id}_tonemapped.jpg`);
  await download(tonemappedUrl, rawTonemapped, "alps_field_tonemapped.jpg (source for the 512px preview)");

  const outDir = path.join(PACK, "hdri");
  ensureDir(outDir);
  const outHdr = path.join(outDir, "sky.hdr");
  fs.copyFileSync(rawHdr, outHdr);

  const outPreview = path.join(outDir, "sky-preview.jpg");
  const S = await sharp();
  await S(rawTonemapped).resize(512, 256, { fit: "fill" }).jpeg({ quality: 82 }).toFile(outPreview);

  const hdrBytes = fs.statSync(outHdr).size;
  const previewBytes = fs.statSync(outPreview).size;
  log(`  sky.hdr          ${fmtBytes(hdrBytes)}`);
  log(`  sky-preview.jpg  ${fmtBytes(previewBytes)} (512x256, downsized from the full-res tonemapped JPG with sharp)`);

  return {
    ...HDRI,
    file: "hdri/sky.hdr",
    preview_file: "hdri/sky-preview.jpg",
    resolution: "1k",
    source_download_url: hdrUrl,
    bytes: hdrBytes,
    preview_bytes: previewBytes,
  };
}

// ---------------------------------------------------------------------------
// Step: Materials (ambientCG)
// ---------------------------------------------------------------------------

const AMBIENT_MAPS = [
  { suffix: "Color", out: "color" },
  { suffix: "NormalGL", out: "normal" },
  { suffix: "Roughness", out: "roughness" },
];

async function buildMaterials() {
  log("\n== Materials (ambientCG, 1K JPG) ==");
  const S = await sharp();
  const result = {};

  for (const m of MATERIALS) {
    const zipUrl = `https://ambientcg.com/get?file=${m.ambientId}_1K-JPG.zip`;
    const rawZip = path.join(RAW, "materials", `${m.ambientId}_1K-JPG.zip`);
    await download(zipUrl, rawZip, `${m.ambientId}_1K-JPG.zip (${m.preset})`);

    const extractDir = path.join(RAW, "materials", m.ambientId);
    if (!fs.existsSync(extractDir)) {
      extractZip(rawZip, extractDir, {
        only: (name) => AMBIENT_MAPS.some((mm) => name.endsWith(`_${mm.suffix}.jpg`)),
      });
    }

    const outDir = path.join(PACK, "materials", m.preset);
    ensureDir(outDir);
    const maps = [];
    for (const mm of AMBIENT_MAPS) {
      const srcFile = path.join(extractDir, `${m.ambientId}_1K-JPG_${mm.suffix}.jpg`);
      if (!fs.existsSync(srcFile)) {
        log(`  warn   ${m.ambientId} has no ${mm.suffix} map, skipping`);
        continue;
      }
      const outFile = path.join(outDir, `${mm.out}.jpg`);
      if (!fs.existsSync(outFile)) {
        let pipeline = S(srcFile);
        if (m.crop) {
          // See the `crop` comment on this material's config entry.
          pipeline = pipeline.extract({ left: m.crop.x, top: m.crop.y, width: m.crop.w, height: m.crop.h });
        }
        // Already 1024x1024 from ambientCG (or cropped above); resize() is a
        // defensive no-op for the uncropped path, the real work there is the
        // quality-82 recompress. For a crop, this is the upscale to 1024.
        await pipeline.resize(1024, 1024, { fit: "fill" }).jpeg({ quality: 82 }).toFile(outFile);
      }
      maps.push(mm.out);
    }
    const bytes = maps.reduce((sum, name) => sum + fs.statSync(path.join(outDir, `${name}.jpg`)).size, 0);
    log(`  ${m.preset.padEnd(22)} <- ${m.ambientId}  ${maps.join("+")}  ${fmtBytes(bytes)}`);

    result[m.preset] = {
      id: m.preset,
      name: m.name,
      source_url: `https://ambientcg.com/a/${m.ambientId}`,
      source_asset_id: m.ambientId,
      license: "CC0 1.0",
      physical_size_mm: m.size_mm,
      maps: maps.map((name) => `materials/${m.preset}/${name}.jpg`),
      bytes,
      note: m.note,
    };
  }

  for (const f of FLAT_PRESETS) {
    result[f.preset] = {
      id: f.preset,
      name: f.name,
      source_url: null,
      license: null,
      physical_size_mm: null,
      maps: [],
      bytes: 0,
      note: `No texture: flat MeshStandardMaterial color ${f.color} (pattern "none" in crates/guhit-model/src/defaults.rs).${f.note ? " " + f.note : ""}`,
    };
  }

  return result;
}

// ---------------------------------------------------------------------------
// Step: Kenney Furniture Kit
// ---------------------------------------------------------------------------

async function fetchKenney() {
  log("\n== Kenney Furniture Kit ==");
  const rawZip = path.join(RAW, "kenney", "kenney_furniture-kit.zip");
  await download(KENNEY_ZIP_URL, rawZip, "kenney_furniture-kit.zip");
  const extractDir = path.join(RAW, "kenney", "extracted");
  if (!fs.existsSync(path.join(extractDir, KENNEY_MODEL_DIR))) {
    extractZip(rawZip, extractDir, { only: (name) => name.startsWith(KENNEY_MODEL_DIR + "/") && name.endsWith(".glb") });
    log(`  extracted ${KENNEY_MODEL_DIR}/*.glb`);
  } else {
    log(`  skip   already extracted -> ${extractDir}`);
  }
  return path.join(extractDir, KENNEY_MODEL_DIR);
}

// ---------------------------------------------------------------------------
// Step: Poly Haven hero models
// ---------------------------------------------------------------------------

async function fetchPolyHavenModel(id) {
  log(`  -- ${id} --`);
  const filesRes = await fetch(`https://api.polyhaven.com/files/${id}`);
  const files = await filesRes.json();
  const g = files.gltf["1k"].gltf;
  const dir = path.join(RAW, "polyhaven-models", id);
  const gltfPath = path.join(dir, path.basename(g.url));
  await download(g.url, gltfPath, `${id}.gltf`);
  for (const [relPath, info] of Object.entries(g.include)) {
    await download(info.url, path.join(dir, relPath), `${id}/${relPath}`);
  }
  return gltfPath;
}

async function fetchAllPolyHavenModels() {
  log("\n== Poly Haven hero models ==");
  const paths = {};
  for (const id of new Set(Object.values(POLYHAVEN_MODELS))) {
    paths[id] = await fetchPolyHavenModel(id);
  }
  return paths;
}

// ---------------------------------------------------------------------------
// Step: normalize + build each catalog model
// ---------------------------------------------------------------------------

/**
 * Wrap a scene's existing children in two new nodes so the model ends up
 * with: origin at the footprint center, underside at y=0, footprint scaled
 * to (targetW, targetD) mm and height scaled to targetH mm.
 *
 * Orientation: every source checked (Kenney kit + the three Poly Haven hero
 * models) already places its "back" at local -Z, matching this app's
 * convention (docs/CONTRACT.md: "Assets: local +y is the back of the
 * object", which src/viewer3d/scene/assets.ts maps to three.js -z). No
 * rotation is applied. See ASSETS.md for how that was verified.
 */
async function wrapAndScale(doc, scene, targetW_mm, targetD_mm, targetH_mm) {
  const fns = await gltfFunctions();
  const bbox = fns.getBounds(scene);
  const [minX, minY, minZ] = bbox.min;
  const [maxX, maxY, maxZ] = bbox.max;
  const curW = maxX - minX || 1;
  const curH = maxY - minY || 1;
  const curD = maxZ - minZ || 1;

  const children = scene.listChildren();
  const inner = doc.createNode("normalized-inner");
  for (const child of children) {
    scene.removeChild(child);
    inner.addChild(child);
  }
  inner.setTranslation([-(minX + maxX) / 2, -minY, -(minZ + maxZ) / 2]);

  const outer = doc.createNode("normalized-outer").addChild(inner);
  outer.setScale([targetW_mm / 1000 / curW, targetH_mm / 1000 / curH, targetD_mm / 1000 / curD]);
  scene.addChild(outer);
  return doc;
}

async function countTriangles(doc) {
  let tris = 0;
  for (const mesh of doc.getRoot().listMeshes()) {
    for (const prim of mesh.listPrimitives()) {
      const indices = prim.getIndices();
      const pos = prim.getAttribute("POSITION");
      tris += Math.round((indices ? indices.getCount() : pos.getCount()) / 3);
    }
  }
  return tris;
}

async function optimizeToPack(tmpGlbPath, outPath, catalogKey) {
  ensureDir(path.dirname(outPath));
  // --instance false: optimize's default GPU-instancing pass (>=5 identical
  // mesh references) corrupts bounds reported for composed models like
  // dining-6 (6 chairs) without meaningfully shrinking these already-tiny
  // files, so it stays off.
  const extra = OPTIMIZE_OVERRIDES[catalogKey] ?? [];
  gltfTransform(["optimize", tmpGlbPath, outPath, "--compress", "meshopt", "--texture-size", "1024", "--instance", "false", ...extra]);
}

async function finalStats(glbPath) {
  const fns = await gltfFunctions();
  const io = await makeIO();
  const doc = await io.read(glbPath);
  const scene = doc.getRoot().listScenes()[0];
  const bbox = fns.getBounds(scene);
  const bbox_mm = {
    width: Math.round((bbox.max[0] - bbox.min[0]) * 1000),
    depth: Math.round((bbox.max[2] - bbox.min[2]) * 1000),
    height: Math.round((bbox.max[1] - bbox.min[1]) * 1000),
  };
  const triangles = await countTriangles(doc);
  const bytes = fs.statSync(glbPath).size;
  return { bbox_mm, triangles, bytes };
}

async function buildSingleKenneyModel(catalogKey, kenneyDir, sourceFile, target) {
  const io = await makeIO();
  const doc = await io.read(path.join(kenneyDir, sourceFile));
  recolorKenneyMaterials(doc, catalogKey);
  const scene = doc.getRoot().listScenes()[0];
  await wrapAndScale(doc, scene, target.w, target.d, target.h);
  ensureDir(TMP);
  const tmpPath = path.join(TMP, `${catalogKey}.raw.glb`);
  await io.write(tmpPath, doc);

  const outPath = path.join(PACK, "models", `${catalogKey}.glb`);
  await optimizeToPack(tmpPath, outPath, catalogKey);
  const stats = await finalStats(outPath);
  return {
    file: `models/${catalogKey}.glb`,
    source: "kenney",
    source_url: "https://kenney.nl/assets/furniture-kit",
    source_file: sourceFile,
    license: "CC0 1.0",
    back_axis: "-z",
    ...stats,
  };
}

async function buildPolyHavenHeroModel(catalogKey, gltfPath, target) {
  const io = await makeIO();
  const doc = await io.read(gltfPath);
  const scene = doc.getRoot().listScenes()[0];
  await wrapAndScale(doc, scene, target.w, target.d, target.h);
  ensureDir(TMP);
  const tmpPath = path.join(TMP, `${catalogKey}.raw.glb`);
  await io.write(tmpPath, doc);

  const outPath = path.join(PACK, "models", `${catalogKey}.glb`);
  await optimizeToPack(tmpPath, outPath, catalogKey);
  const stats = await finalStats(outPath);
  const id = POLYHAVEN_MODELS[catalogKey];
  return {
    file: `models/${catalogKey}.glb`,
    source: "polyhaven",
    source_url: `https://polyhaven.com/a/${id}`,
    source_asset_id: id,
    license: "CC0 1.0",
    back_axis: "-z",
    ...stats,
  };
}

/** dining-4 / dining-6: one table + N chairs around it, merged into one GLB. */
async function buildDiningSet(catalogKey, kenneyDir, target, chairsPerSide) {
  const fns = await gltfFunctions();
  const io = await makeIO();

  ensureDir(TMP);
  const pieces = [];

  // Table: scaled to the full footprint.
  {
    const doc = await io.read(path.join(kenneyDir, "table.glb"));
    const scene = doc.getRoot().listScenes()[0];
    await wrapAndScale(doc, scene, target.w, target.d, target.h);
    const p = path.join(TMP, `${catalogKey}.table.glb`);
    await io.write(p, doc);
    pieces.push(p);
  }

  // Chairs: a fixed chair footprint (Kenney's own chair proportions scaled
  // to a plausible ~440x460mm seat), placed evenly along the two long
  // sides, facing the table.
  const chairW = 440;
  const chairD = 460;
  const chairH = 880;
  const inset = 60; // mm the chair overlaps under the table edge
  for (let i = 0; i < chairsPerSide; i++) {
    for (const side of [-1, 1]) {
      const doc = await io.read(path.join(kenneyDir, "chair.glb"));
      const scene = doc.getRoot().listScenes()[0];
      await wrapAndScale(doc, scene, chairW, chairD, chairH);
      // Chairs on the +z side (side=1) need to face -z (toward the table
      // center), which is a 180 degree turn from their default -z-back /
      // +z-front orientation.
      if (side === 1) {
        const root = doc.getRoot();
        const scene2 = root.listScenes()[0];
        const child = scene2.listChildren()[0]; // the "normalized-outer" node
        scene2.removeChild(child);
        const spin = doc.createNode("spin-180").addChild(child);
        spin.setRotation([0, 1, 0, 0]); // 180 deg about Y
        scene2.addChild(spin);
      }
      const x = -target.w / 2 + (target.w / chairsPerSide) * (i + 0.5);
      const z = side * (target.d / 2 - chairD / 2 + inset);
      const root = doc.getRoot();
      const scene2 = root.listScenes()[0];
      const top = scene2.listChildren()[0];
      scene2.removeChild(top);
      const placed = doc.createNode(`chair-${side}-${i}`).addChild(top);
      placed.setTranslation([x / 1000, 0, z / 1000]);
      scene2.addChild(placed);

      const p = path.join(TMP, `${catalogKey}.chair.${side}.${i}.glb`);
      await io.write(p, doc);
      pieces.push(p);
    }
  }

  const mergedPath = path.join(TMP, `${catalogKey}.merged.glb`);
  gltfTransform(["merge", ...pieces, mergedPath, "--merge-scenes"]);

  const outPath = path.join(PACK, "models", `${catalogKey}.glb`);
  await optimizeToPack(mergedPath, outPath, catalogKey);
  const stats = await finalStats(outPath);
  return {
    file: `models/${catalogKey}.glb`,
    source: "kenney",
    source_url: "https://kenney.nl/assets/furniture-kit",
    source_file: `table.glb + ${chairsPerSide * 2}x chair.glb (composed)`,
    license: "CC0 1.0",
    back_axis: "n/a (symmetric table and chairs, no single front/back)",
    composed_from: ["table.glb", `chair.glb x${chairsPerSide * 2}`],
    ...stats,
  };
}

async function buildModels(kenneyDir, polyhavenPaths) {
  log("\n== Models ==");
  const result = {};
  for (const target of CATALOG) {
    const key = target.key;
    if (key === "dining-4" || key === "dining-6") {
      const chairsPerSide = key === "dining-4" ? 2 : 3;
      result[key] = await buildDiningSet(key, kenneyDir, target, chairsPerSide);
    } else if (POLYHAVEN_MODELS[key]) {
      const id = POLYHAVEN_MODELS[key];
      result[key] = await buildPolyHavenHeroModel(key, polyhavenPaths[id], target);
    } else if (KENNEY_SINGLE[key]) {
      result[key] = await buildSingleKenneyModel(key, kenneyDir, KENNEY_SINGLE[key], target);
    } else if (NO_SOURCE[key]) {
      result[key] = { file: null, source: "none", note: NO_SOURCE[key] };
      log(`  --     ${key.padEnd(18)} no CC0 source (${NO_SOURCE[key]})`);
      continue;
    } else {
      throw new Error(`catalog key ${key} has no source mapping`);
    }
    const r = result[key];
    log(`  ${key.padEnd(18)} <- ${r.source_file ?? r.source_asset_id}  bbox ${r.bbox_mm.width}x${r.bbox_mm.depth}x${r.bbox_mm.height}mm  ${r.triangles} tri  ${fmtBytes(r.bytes)}`);
  }
  return result;
}

// ---------------------------------------------------------------------------
// Manifest + docs
// ---------------------------------------------------------------------------

function writeManifest(hdri, materials, models) {
  const manifest = {
    version: 1,
    generated_at: new Date().toISOString(),
    hdri,
    materials,
    models,
  };
  fs.writeFileSync(path.join(PACK, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
  return manifest;
}

function writeAssetsDoc(manifest) {
  const lines = [];
  lines.push("# Asset Pack");
  lines.push("");
  lines.push("CC0 asset pack for the 3D viewer: HDRI sky, PBR materials, catalog models.");
  lines.push("Built by `scripts/assets-build.mjs` from three approved CC0 sources:");
  lines.push("[Poly Haven](https://polyhaven.com), [ambientCG](https://ambientcg.com), and the");
  lines.push("[Kenney Furniture Kit](https://kenney.nl/assets/furniture-kit).");
  lines.push("");
  lines.push("Rebuild: `node scripts/assets-build.mjs` (idempotent, skips cached downloads in `assets/raw/`).");
  lines.push("");
  lines.push("## License");
  lines.push("");
  lines.push(
    "Every asset in this pack is CC0 1.0 (public domain dedication): no attribution required, free for commercial use, modification and redistribution. Full text: https://creativecommons.org/publicdomain/zero/1.0/"
  );
  lines.push("");
  lines.push("## HDRI");
  lines.push("");
  lines.push("| Field | Value |");
  lines.push("|---|---|");
  lines.push(`| Asset | ${manifest.hdri.name} (\`${manifest.hdri.id}\`) |`);
  lines.push(`| Source | ${manifest.hdri.sourceUrl} |`);
  lines.push(`| License | ${manifest.hdri.license} |`);
  lines.push(`| File | \`${manifest.hdri.file}\` (${fmtBytes(manifest.hdri.bytes)}) |`);
  lines.push(`| Preview | \`${manifest.hdri.preview_file}\` (${fmtBytes(manifest.hdri.preview_bytes)}, 512x256 JPG) |`);
  lines.push(`| Sun direction | ${manifest.hdri.sun_direction} |`);
  lines.push("");
  lines.push("## Materials");
  lines.push("");
  lines.push("Physical tile sizes match the UV-repeat convention already used by the built-in");
  lines.push("procedural patterns in `src/viewer3d/scene/materials.ts` (`repeat = 1 / size`), so a");
  lines.push("later agent can swap in these textures without re-deriving tiling scale.");
  lines.push("");
  lines.push("| Preset | Source | Size (mm) | Maps | Bytes |");
  lines.push("|---|---|---|---|---|");
  for (const m of Object.values(manifest.materials)) {
    if (!m.source_url) {
      lines.push(`| \`${m.id}\` | none (flat color) | - | - | 0 |`);
      continue;
    }
    lines.push(
      `| \`${m.id}\` | [${m.source_asset_id}](${m.source_url}) | ${m.physical_size_mm[0]}x${m.physical_size_mm[1]} | ${m.maps.map((p) => p.split("/").pop().replace(".jpg", "")).join(", ")} | ${fmtBytes(m.bytes)} |`
    );
  }
  lines.push("");
  lines.push("Presets with no texture (flat `MeshStandardMaterial` color, matching `pattern: none`");
  lines.push("in `crates/guhit-model/src/defaults.rs`): " + Object.values(manifest.materials).filter((m) => !m.source_url).map((m) => `\`${m.id}\``).join(", ") + ".");
  lines.push("");
  lines.push("## Models");
  lines.push("");
  lines.push(
    "Orientation: every source checked (the full Kenney kit sample plus all three Poly Haven"
  );
  lines.push(
    "hero swaps) already places its \"back\" at local -Z, matching this app's convention"
  );
  lines.push(
    "(`docs/CONTRACT.md`: local +y in plan is the back; `src/viewer3d/scene/assets.ts` maps"
  );
  lines.push(
    "that to three.js -z). Verified by: (1) visual check of Kenney's isometric preview renders"
  );
  lines.push(
    "for `bedDouble` and `loungeSofa` (headboard / backrest sit on the -Z side in both), and"
  );
  lines.push(
    "(2) a geometric heuristic across the rest of the kit and the Poly Haven models (the"
  );
  lines.push(
    "centroid of each mesh's upper 40% of vertices, which tends to sit over the tall/detailed"
  );
  lines.push(
    "side of asymmetric objects, consistently falls on the -Z side). No rotation was applied to"
  );
  lines.push("any model as a result. Every model's origin is the footprint center, underside at y=0.");
  lines.push("");
  lines.push("| Catalog key | Source | Bbox (mm, w x d x h) | Triangles | Bytes |");
  lines.push("|---|---|---|---|---|");
  for (const [key, m] of Object.entries(manifest.models)) {
    if (m.source === "none") {
      lines.push(`| \`${key}\` | none (procedural fallback in the viewer) | - | - | - |`);
      continue;
    }
    lines.push(`| \`${key}\` | ${m.source === "kenney" ? "Kenney" : "Poly Haven"}: ${m.source_file ?? m.source_asset_id} | ${m.bbox_mm.width}x${m.bbox_mm.depth}x${m.bbox_mm.height} | ${m.triangles} | ${fmtBytes(m.bytes)} |`);
  }
  lines.push("");
  lines.push("Not covered by any CC0 source (procedural fallback in `src/viewer3d/scene/assets.ts` stays in use):");
  lines.push("");
  for (const [key, reason] of Object.entries(NO_SOURCE)) {
    lines.push(`- \`${key}\`: ${reason}`);
  }
  lines.push("");
  lines.push(
    "Kept as Kenney instead of a Poly Haven hero swap: `sofa-3`, `sofa-2`, `bed-single`, `bed-double`,"
  );
  lines.push(
    "`bed-queen`. Poly Haven's only sofa models (`sofa_02`, `sofa_03`, `Sofa_01`) and bed models"
  );
  lines.push(
    "(`GothicBed_01`, `old_bed_frame`, `vintage_day_bed`) are vintage/Victorian pieces or a bare frame"
  );
  lines.push(
    "with no mattress; none fit the app's tropical-modern / modern-minimal render styles"
  );
  lines.push(
    "(`crates/guhit-model/src/defaults.rs` `render_styles()`) better than Kenney's plain kit pieces."
  );
  lines.push("");
  lines.push("## Kenney palette recolor");
  lines.push("");
  lines.push(
    "The Kenney Furniture Kit's stock palette includes one saturated color, a salmon"
  );
  lines.push(
    "(`carpet` material, raw `baseColorFactor` ~`[0.943, 0.367, 0.343]`, `#f15e57` read as sRGB"
  );
  lines.push(
    "hex) reused as-is for both the sofa upholstery and the bed mattress/bedding top. Every other"
  );
  lines.push(
    "Kenney material this pack uses (wood, the metal greys, the near-white fixture/appliance"
  );
  lines.push(
    "tones, the pale glass) was already muted enough to keep unchanged. Applied in"
  );
  lines.push(
    "`scripts/assets-build.mjs` (`recolorKenneyMaterials`) via `@gltf-transform/core`'s `NodeIO`:"
  );
  lines.push(
    "each Kenney model's materials are read, matched against the salmon value, and their"
  );
  lines.push("`baseColorFactor` replaced, before the optimize pass. Poly Haven models are never touched.");
  lines.push("");
  lines.push("| Catalog keys | Original | New | Role |");
  lines.push("|---|---|---|---|");
  lines.push("| `sofa-2`, `sofa-3` | `#f15e57` salmon (`carpet`) | `#8f8a82` warm grey | upholstery |");
  lines.push("| `bed-single`, `bed-double`, `bed-queen` | `#f15e57` salmon (`carpet`) | `#eeeae2` off-white | mattress / bedding |");
  lines.push("");
  lines.push(
    "Unchanged (already muted, kept as-is): wood tones (`wood` `#e59964`, `woodDark` `#ad744c`),"
  );
  lines.push(
    "appliance/fixture whites and metal greys (`carpetWhite` `#f8ffff`, `metalLight` `#effaf4`,"
  );
  lines.push(
    "`metal` `#bdd2d6`, `metalMedium` `#5e7777`, `metalDark` `#4e6363`, `_defaultMat` `#ffffff`), and"
  );
  lines.push("the pale glass (`glass` `#b2d3c4`).");
  lines.push("");
  fs.writeFileSync(path.join(ROOT, "assets", "ASSETS.md"), lines.join("\n") + "\n");
}

// ---------------------------------------------------------------------------
// Verify
// ---------------------------------------------------------------------------

async function verify(manifest) {
  log("\n== Verify ==");
  let ok = true;

  // 1. total pack size
  const du = execFileSync("du", ["-sh", PACK]).toString().trim();
  log(`  du -sh public/assets/pack -> ${du}`);

  // 2. every referenced file exists
  const referenced = [];
  if (manifest.hdri.file) referenced.push(manifest.hdri.file);
  if (manifest.hdri.preview_file) referenced.push(manifest.hdri.preview_file);
  for (const m of Object.values(manifest.materials)) for (const f of m.maps) referenced.push(f);
  for (const m of Object.values(manifest.models)) if (m.file) referenced.push(m.file);
  for (const rel of referenced) {
    const p = path.join(PACK, rel);
    if (!fs.existsSync(p)) {
      log(`  MISSING  ${rel}`);
      ok = false;
    }
  }
  log(`  manifest file-existence check: ${referenced.length} files referenced, ${ok ? "all present" : "SOME MISSING"}`);

  // 3. every GLB parses with gltf-transform inspect + bbox tolerance
  for (const [key, m] of Object.entries(manifest.models)) {
    if (!m.file) continue;
    const p = path.join(PACK, m.file);
    try {
      execFileSync(GLTF_TRANSFORM_BIN, ["inspect", p], { stdio: ["ignore", "pipe", "pipe"] });
    } catch (e) {
      log(`  INSPECT FAILED  ${key}: ${e.message}`);
      ok = false;
      continue;
    }
    const target = CATALOG.find((c) => c.key === key);
    const isDiningSet = key === "dining-4" || key === "dining-6";
    const wErr = Math.abs(m.bbox_mm.width - target.w) / target.w;
    const dErr = Math.abs(m.bbox_mm.depth - target.d) / target.d;
    // dining-4/dining-6: the catalog depth is the table's own footprint;
    // the composed model's depth legitimately exceeds it once chairs are
    // pulled out around it, so only width (which is not affected by chair
    // placement) is checked against the catalog for those two keys.
    if (wErr > 0.10001 || (!isDiningSet && dErr > 0.10001)) {
      log(`  TOLERANCE  ${key}: bbox ${m.bbox_mm.width}x${m.bbox_mm.depth} vs catalog ${target.w}x${target.d} (${(wErr * 100).toFixed(1)}%/${(dErr * 100).toFixed(1)}%)`);
      ok = false;
    } else if (isDiningSet) {
      log(`  note   ${key}: depth ${m.bbox_mm.depth}mm includes chairs pulled out around the ${target.d}mm table (expected)`);
    }
  }
  log(`  gltf-transform inspect + bbox tolerance (10%): ${ok ? "pass" : "FAIL"}`);

  return ok;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  ensureDir(RAW);
  ensureDir(PACK);
  ensureDir(TMP);

  let manifest;
  if (VERIFY_ONLY) {
    manifest = JSON.parse(fs.readFileSync(path.join(PACK, "manifest.json"), "utf8"));
  } else {
    const hdri = await buildHdri();
    const materials = await buildMaterials();
    const kenneyDir = await fetchKenney();
    const polyhavenPaths = await fetchAllPolyHavenModels();
    const models = await buildModels(kenneyDir, polyhavenPaths);
    manifest = writeManifest(hdri, materials, models);
    writeAssetsDoc(manifest);
    log(`\nWrote ${path.relative(ROOT, path.join(PACK, "manifest.json"))} and assets/ASSETS.md`);
  }

  const ok = await verify(manifest);
  fs.rmSync(TMP, { recursive: true, force: true });
  if (!ok) {
    log("\nVerification FAILED, see above.");
    process.exit(1);
  }
  log("\nDone.");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
