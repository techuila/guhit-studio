#!/usr/bin/env node
// Asset pack pipeline: downloads CC0 sources (Poly Haven, ambientCG, Kenney
// Furniture Kit) into a raw cache (assets/raw/ by default, git-ignored),
// processes them into public/assets/pack/ (ships in the app), and writes
// manifest.json.
//
// Idempotent: every download is skipped when the destination file already
// exists. Re-run any time; delete the raw cache or public/assets/pack/ to
// force a clean rebuild.
//
// Usage: node scripts/assets-build.mjs [--raw <dir>] [--verify-only] [--check]
//          [--only hdri,mat-ground-grass,sofa-leather]
//   --raw <dir>  raw download cache (or env GUHIT_ASSETS_RAW), default assets/raw
//   --check      license and pack check only: no downloads, exits 1 on a problem
//   --only       rebuilds the named parts (hdri, material presets, model keys), the rest of the manifest kept

import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

/** The value after a `--flag`, or null. */
function argValue(flag) {
  const i = process.argv.indexOf(flag);
  return i >= 0 && i + 1 < process.argv.length ? process.argv[i + 1] : null;
}

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const RAW = path.resolve(argValue("--raw") ?? process.env.GUHIT_ASSETS_RAW ?? path.join(ROOT, "assets", "raw"));
const PACK = path.join(ROOT, "public", "assets", "pack");
const TMP = path.join(RAW, ".tmp");
const CATALOG_RS = path.join(ROOT, "crates", "guhit-model", "src", "catalog.rs");

const VERIFY_ONLY = process.argv.includes("--verify-only");
const CHECK_ONLY = process.argv.includes("--check");
/** `--only hdri,<preset>,<model key>`: rebuilds those parts only, the rest of the manifest kept. */
const ONLY = argValue("--only");

/** Whole pack budget: 40 MB, counted as 1024 x 1024 bytes. */
const PACK_LIMIT_BYTES = 40 * 1024 * 1024;
/** Hard ceiling for one catalog v2 model; most land near 400 KB. */
const MODEL_LIMIT_BYTES = 1.2 * 1024 * 1024;
/** The only license and source hosts the pack takes. */
const LICENSE = "CC0 1.0";
const SOURCE_HOSTS = ["polyhaven.com", "ambientcg.com", "kenney.nl"];

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
  // A moved file can come back as a 200 HTML page: never cache that as a zip.
  if (destPath.endsWith(".zip") && buf.toString("latin1", 0, 2) !== "PK") {
    throw new Error(`download is not a zip (got ${res.headers.get("content-type")}, ${buf.length} bytes): ${url}`);
  }
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
// CONTRACT DATA: mirrors crates/guhit-model/src/catalog.rs. Keep in sync by
// hand; the build does not parse Rust source (only `--check` reads the keys
// back from it). Sizes are millimeters. `uniform` scales by height alone and
// keeps the scan's own proportions.
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
  // Catalog v2 (Wave 2, lane L5): sizes close to each Poly Haven scan's own.
  { key: "sofa-leather", w: 1810, d: 820, h: 710 },
  { key: "sofa-wood", w: 2290, d: 970, h: 860 },
  { key: "lounge-chair", w: 1010, d: 1190, h: 1170 },
  { key: "armchair-classic", w: 850, d: 770, h: 1065 },
  { key: "ottoman", w: 885, d: 620, h: 625 },
  { key: "coffee-table-round", w: 1200, d: 1200, h: 490 },
  { key: "coffee-table-square", w: 1100, d: 1100, h: 370 },
  { key: "side-table", w: 550, d: 450, h: 550 },
  { key: "side-table-round", w: 385, d: 385, h: 760 },
  { key: "tea-table", w: 840, d: 840, h: 500 },
  { key: "sideboard", w: 2000, d: 500, h: 680 },
  { key: "ceiling-fan", w: 1460, d: 1460, h: 520 },
  { key: "plant-leafy", w: 700, d: 660, h: 840 },
  { key: "plant-succulent", w: 170, d: 185, h: 270 },
  { key: "vase-ceramic", w: 260, d: 260, h: 500 },
  { key: "wall-art", w: 600, d: 20, h: 465 },
  { key: "mirror-wall", w: 490, d: 26, h: 745 },
  { key: "shelf-cube", w: 1080, d: 370, h: 1555 },
  { key: "shelf-steel", w: 1100, d: 500, h: 2140 },
  { key: "basket-rattan", w: 375, d: 270, h: 220 },
  { key: "rocking-chair", w: 710, d: 830, h: 995 },
  { key: "dining-round-2", w: 800, d: 800, h: 745 },
  { key: "chair-dining", w: 435, d: 575, h: 975 },
  { key: "chair-wood-white", w: 430, d: 540, h: 955 },
  { key: "chair-monobloc", w: 640, d: 630, h: 880 },
  { key: "stool-wood", w: 480, d: 480, h: 750 },
  { key: "table-wood", w: 1200, d: 700, h: 760 },
  // The sets' footprint includes the chairs pulled up to the table.
  { key: "dining-set-4", w: 1200, d: 1600, h: 955 },
  { key: "dining-set-6", w: 1800, d: 1700, h: 955 },
  { key: "nightstand", w: 505, d: 510, h: 615 },
  { key: "dresser", w: 1200, d: 580, h: 1210 },
  { key: "cabinet-drawers", w: 1140, d: 490, h: 1880 },
  { key: "desk-metal", w: 1600, d: 760, h: 760 },
  { key: "range-electric", w: 550, d: 650, h: 860 },
  { key: "microwave", w: 500, d: 370, h: 300 },
  { key: "cabinet-white", w: 1195, d: 615, h: 1180 },
  { key: "outdoor-set", w: 780, d: 1830, h: 860 },
  { key: "bench-wood", w: 1165, d: 500, h: 890 },
  { key: "planter-box", w: 915, d: 415, h: 425 },
  { key: "car-covered", w: 1790, d: 4380, h: 1410 },
  { key: "gate-iron", w: 2950, d: 100, h: 2930 },
  { key: "lamp-post-classic", w: 505, d: 280, h: 2800, uniform: true },
  { key: "tree-flowering", w: 6800, d: 5300, h: 5500, uniform: true },
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

// Catalog v2 (Wave 2, lane L5): one Poly Haven scan per new key, picked for
// homes in the Philippines (a narra-like wooden sala sofa, the monobloc
// chair, a rattan basket, the ceiling fan). Same normalize as the heroes, but
// textures go out as WebP (EXT_texture_webp) to keep each file near 400 KB.
// Poly Haven has no beds, toilets, sinks, showers, fridges or washers, so
// those stay Kenney or procedural.
const POLYHAVEN_V2 = {
  "sofa-leather": "sofa_02",
  "sofa-wood": "chinese_sofa",
  "lounge-chair": "mid_century_lounge_chair",
  "armchair-classic": "ArmChair_01",
  ottoman: "Ottoman_01",
  "coffee-table-round": "coffee_table_round_01",
  "coffee-table-square": "modern_coffee_table_02",
  "side-table": "side_table_01",
  "side-table-round": "side_table_tall_01",
  "tea-table": "chinese_tea_table",
  sideboard: "modern_wooden_cabinet",
  "ceiling-fan": "ceiling_fan",
  "plant-leafy": "potted_plant_02",
  "plant-succulent": "potted_plant_04",
  "vase-ceramic": "ceramic_vase_01",
  "wall-art": "fancy_picture_frame_01",
  "mirror-wall": "ornate_mirror_01",
  "shelf-cube": "wooden_display_shelves_01",
  "shelf-steel": "steel_frame_shelves_01",
  "basket-rattan": "wicker_basket_02",
  "rocking-chair": "Rockingchair_01",
  "dining-round-2": "round_wooden_table_02",
  "chair-dining": "dining_chair_02",
  "chair-wood-white": "painted_wooden_chair_01",
  "chair-monobloc": "plastic_monobloc_chair_01",
  "stool-wood": "bar_chair_round_01",
  "table-wood": "wooden_table_02",
  nightstand: "painted_wooden_nightstand",
  dresser: "GothicCommode_01",
  "cabinet-drawers": "drawer_cabinet",
  "desk-metal": "metal_office_desk",
  "range-electric": "electric_stove",
  microwave: "vintage_microwave",
  "cabinet-white": "painted_wooden_cabinet",
  "outdoor-set": "outdoor_table_chair_set_01",
  "bench-wood": "painted_wooden_bench",
  "planter-box": "planter_box_01",
  "car-covered": "covered_car",
  "gate-iron": "large_iron_gate",
  "lamp-post-classic": "street_lamp_01",
  "tree-flowering": "jacaranda_tree",
};

// Catalog v2 dining sets, composed from Poly Haven parts like the Kenney
// dining-4/6: one table plus chairs on the two long sides, facing it. The
// chairs are nodes over one shared chair mesh, so the file carries it once.
const POLYHAVEN_SETS = {
  "dining-set-4": { table: "wooden_table_02", chair: "painted_wooden_chair_01", tableSize: [1200, 800, 750], chairsPerSide: 2 },
  "dining-set-6": { table: "wooden_table_02", chair: "painted_wooden_chair_01", tableSize: [1800, 900, 750], chairsPerSide: 3 },
};
/** painted_wooden_chair_01 at the chair-wood-white size, mm (w, d, h). */
const SET_CHAIR_SIZE = [430, 540, 955];

// Degrees about +Y baked into a v2 GLB so its back lands on -Z. Poly Haven
// models face Blender -Y, which is glTF +Z, so most need none; each one was
// checked (see ASSETS.md) and only these turn. wooden_display_shelves_01 is
// modeled with its open cube face on +X (its 1078 mm width runs along Z), so
// a quarter turn puts the width on X and the cubes on +Z.
const V2_TURNS = {
  "shelf-cube": -90,
};

// v2 keys whose scan is heavy in geometry: extra `gltf-transform optimize`
// flags on top of the v2 defaults (meshopt, WebP, 1024 px). potted_plant_02
// goes from 69k to 24k triangles (868 to 576 KB) with the leaf shapes kept;
// the gate and the sideboard lose about a fifth of theirs. bar_chair_round_01
// and modern_coffee_table_02 barely reduced even at error 0.01, so they ship
// as scanned. jacaranda_tree is 3.86M triangles (a 208 MB .bin): its leaf
// clusters survive a strong simplify, so the canopy still reads as a tree.
const V2_OPTIMIZE_OVERRIDES = {
  "plant-leafy": ["--simplify-ratio", "0.35", "--simplify-error", "0.01"],
  "gate-iron": ["--simplify-ratio", "0.5", "--simplify-error", "0.002"],
  sideboard: ["--simplify-ratio", "0.5", "--simplify-error", "0.003"],
  "tree-flowering": ["--simplify-ratio", "0.01", "--simplify-error", "0.05"],
};
/**
 * v2 keys at 512 px textures instead of 1024. Each 1024 px map costs about
 * 5.6 MB of GPU memory (RGBA plus mipmaps), and a furnished house with 26 v2
 * models went from 117 to 443 MB. Small pieces (largest catalog side under
 * 1 m), the wall items, the small decor, the chairs, the nightstand, the side
 * tables, the ottoman and the planter go to 512 px (1.4 MB a map); the large
 * pieces and the electric range keep 1024. The ceiling fan hangs at 2.5 m, so
 * its five maps are 512 px too; it keeps its specular map, since three's
 * GLTFLoader turns KHR_materials_specular into a MeshPhysicalMaterial
 * specularIntensityMap. The tree's nine maps are 512 px: it is seen from
 * metres away.
 */
const V2_TEXTURE_SIZE = Object.fromEntries(
  [
    "armchair-classic", "lounge-chair", "rocking-chair", "chair-dining", "chair-wood-white", "chair-monobloc", "stool-wood",
    "ottoman", "side-table", "side-table-round", "tea-table", "dining-round-2", "nightstand",
    "plant-leafy", "plant-succulent", "vase-ceramic", "basket-rattan", "planter-box", "wall-art", "mirror-wall", "microwave",
    "ceiling-fan", "tree-flowering",
  ].map((key) => [key, 512])
);
/** Size limit other than MODEL_LIMIT_BYTES: the tree was given about 1.5 MB. */
const V2_SIZE_LIMIT = { "tree-flowering": 1.5 * 1024 * 1024 };
/**
 * BLEND materials made OPAQUE. The jacaranda leaves carry no alpha at all (JPG
 * maps, white vertex colors), so BLEND only turned off depth writes and left
 * 50k leaf triangles drawn in index order.
 */
const V2_OPAQUE = new Set(["tree-flowering"]);

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
  {
    preset: "mat-ground-grass",
    name: "Lawn (the 3D view's ground)",
    ambientId: "Grass004",
    size_mm: [1400, 1400],
    note: "Not a project material: the 3D view draws its ground with it (src/viewer3d/engine/environment.ts), sampled at two scales so the tile does not repeat visibly.",
  },
  {
    preset: "mat-paving-stone",
    name: "Paving stones (site paving)",
    ambientId: "PavingStones099",
    // Grey I-shaped interlocking concrete pavers. An autocorrelation of the
    // joint lines finds 11 pavers across (93 px pitch) and 14 rows down (73 px
    // pitch) in the square 1024 px photo. A 200 mm paver makes the tile 11 x
    // 200 = 2200 mm, and the rows then come out 157 mm wide, close to the
    // common 200 x 160 mm I-block (ambientCG's own metadata says about 2 m).
    size_mm: [2200, 2200],
    note: "For deck slabs used as site paving (SlabKind::Deck with this material): driveways, carports, garden walks. Grey I-shaped interlocking concrete pavers, 200 x 157 mm at this tile size.",
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

// HDRI: one outdoor sunny sky for a tropical Philippine site. Pretoria
// Gardens (Poly Haven, CC0): a high midday sun, scattered cumulus, broadleaf
// trees on a flat horizon and grass under them, so the photo's own ground
// carries on from the lawn the viewer draws. The sun direction is found from
// the pixels at load time (src/viewer3d/engine/environment.ts, `scanHdri`).
// It replaced Alps Field (an alpine meadow, 1K light, a 512 px background).
const HDRI = {
  id: "pretoria_gardens",
  name: "Pretoria Gardens",
  license: "CC0 1.0",
  sourceUrl: "https://polyhaven.com/a/pretoria_gardens",
  sun_direction: "found from the pixels at load time (scanHdri in src/viewer3d/engine/environment.ts); midday, high sun",
};
/** Light comes from the 2K HDR; what is seen behind the model is a 4K JPG of the same sky. */
const HDRI_LIGHT_RES = "2k";
const HDRI_BACKGROUND_PX = [4096, 2048];

// ---------------------------------------------------------------------------
// Step: HDRI
// ---------------------------------------------------------------------------

async function buildHdri() {
  log("\n== HDRI ==");
  const filesRes = await fetch(`https://api.polyhaven.com/files/${HDRI.id}`);
  const files = await filesRes.json();
  const hdrUrl = files.hdri[HDRI_LIGHT_RES].hdr.url;
  const tonemappedUrl = files.tonemapped.url;

  const rawHdr = path.join(RAW, "hdri", `${HDRI.id}_${HDRI_LIGHT_RES}.hdr`);
  await download(hdrUrl, rawHdr, `${HDRI.id}_${HDRI_LIGHT_RES}.hdr`);
  const rawTonemapped = path.join(RAW, "hdri", `${HDRI.id}_tonemapped.jpg`);
  await download(tonemappedUrl, rawTonemapped, `${HDRI.id}_tonemapped.jpg (source for the 4K background)`);

  const outDir = path.join(PACK, "hdri");
  ensureDir(outDir);
  const outHdr = path.join(outDir, "sky.hdr");
  fs.copyFileSync(rawHdr, outHdr);

  // The old 512 px preview is gone: the background is the 4K JPG now.
  fs.rmSync(path.join(outDir, "sky-preview.jpg"), { force: true });
  const outBackground = path.join(outDir, "sky.jpg");
  const S = await sharp();
  const [bw, bh] = HDRI_BACKGROUND_PX;
  // Poly Haven's tonemapped JPG is the full 20K original: no pixel limit on the decode.
  await S(rawTonemapped, { limitInputPixels: false }).resize(bw, bh, { fit: "fill" }).jpeg({ quality: 84, mozjpeg: true }).toFile(outBackground);

  const hdrBytes = fs.statSync(outHdr).size;
  const backgroundBytes = fs.statSync(outBackground).size;
  log(`  sky.hdr  ${fmtBytes(hdrBytes)} (${HDRI_LIGHT_RES})`);
  log(`  sky.jpg  ${fmtBytes(backgroundBytes)} (${bw}x${bh}, downsized from the full-res tonemapped JPG with sharp)`);

  return {
    ...HDRI,
    file: "hdri/sky.hdr",
    background_file: "hdri/sky.jpg",
    resolution: HDRI_LIGHT_RES,
    background_px: [bw, bh],
    source_download_url: hdrUrl,
    bytes: hdrBytes,
    background_bytes: backgroundBytes,
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

async function buildMaterials(only = null) {
  log("\n== Materials (ambientCG, 1K JPG) ==");
  const S = await sharp();
  const result = {};

  for (const m of MATERIALS.filter((x) => !only || only.includes(x.preset))) {
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

  for (const f of only ? [] : FLAT_PRESETS) {
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
  try {
    await download(KENNEY_ZIP_URL, rawZip, "kenney_furniture-kit.zip");
  } catch (e) {
    // kenney.nl now hands out the kit only through a signed link made for a
    // browser session (checked 2026-11-04); the old media URL redirects to
    // the asset page, so a clean cache needs the zip placed by hand.
    throw new Error(
      `${e.message}\nDownload the Furniture Kit (v1.0, CC0) in a browser from https://kenney.nl/assets/furniture-kit and put the zip at ${rawZip}, then re-run.`
    );
  }
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

/** Every Poly Haven id the catalog uses: the heroes, the v2 scans and the set parts. */
function polyHavenIds() {
  const sets = Object.values(POLYHAVEN_SETS).flatMap((s) => [s.table, s.chair]);
  return [...new Set([...Object.values(POLYHAVEN_MODELS), ...Object.values(POLYHAVEN_V2), ...sets])];
}

const _phPaths = new Map();
/** The local .gltf path for a Poly Haven id, fetched into the raw cache on first use. */
async function polyHavenModel(id) {
  if (!_phPaths.has(id)) _phPaths.set(id, await fetchPolyHavenModel(id));
  return _phPaths.get(id);
}

let _kenneyDir = null;
async function kenneyModels() {
  _kenneyDir ??= await fetchKenney();
  return _kenneyDir;
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
 * rotation is applied to those. See ASSETS.md for how that was verified.
 *
 * Catalog v2 options: `turn` (degrees about +Y) is applied first, so the
 * target width and depth are measured after the turn; `uniform` scales all
 * three axes by the height ratio, keeping the scan's proportions.
 */
async function wrapAndScale(doc, scene, targetW_mm, targetD_mm, targetH_mm, { turn = 0, uniform = false } = {}) {
  const fns = await gltfFunctions();
  if (turn) {
    const spin = doc.createNode(`turn-${turn}`);
    for (const child of scene.listChildren()) {
      scene.removeChild(child);
      spin.addChild(child);
    }
    const r = (turn * Math.PI) / 360;
    spin.setRotation([0, Math.sin(r), 0, Math.cos(r)]);
    scene.addChild(spin);
  }
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
  const k = targetH_mm / 1000 / curH;
  outer.setScale(uniform ? [k, k, k] : [targetW_mm / 1000 / curW, k, targetD_mm / 1000 / curD]);
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
  // Catalog v2 models and sets: WebP textures. The sets also keep `--join
  // false`, since joining would bake a copy of the shared chair mesh per seat.
  const v2 = POLYHAVEN_V2[catalogKey] || POLYHAVEN_SETS[catalogKey];
  const v2Flags = v2 ? ["--texture-compress", "webp", ...(POLYHAVEN_SETS[catalogKey] ? ["--join", "false"] : []), ...(V2_OPTIMIZE_OVERRIDES[catalogKey] ?? [])] : [];
  const textureSize = String(V2_TEXTURE_SIZE[catalogKey] ?? 1024);
  gltfTransform(["optimize", tmpGlbPath, outPath, "--compress", "meshopt", "--texture-size", textureSize, "--instance", "false", ...extra, ...v2Flags]);
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

/**
 * Drops an occlusion map that is a texture of its own: the viewer draws its
 * own ambient occlusion, so only one packed into the metallicRoughness map
 * (Poly Haven's ARM) is worth its GPU memory. No v2 source has a loose one
 * today (none sets occlusionTexture at all); this keeps it that way.
 */
function dropLooseOcclusion(doc) {
  for (const mat of doc.getRoot().listMaterials()) {
    const ao = mat.getOcclusionTexture();
    if (ao && ao !== mat.getMetallicRoughnessTexture()) mat.setOcclusionTexture(null);
  }
}

/** A catalog v2 key from one Poly Haven scan: turned, normalized, WebP textures. */
async function buildPolyHavenV2Model(catalogKey, target) {
  const id = POLYHAVEN_V2[catalogKey];
  const io = await makeIO();
  const doc = await io.read(await polyHavenModel(id));
  if (V2_OPAQUE.has(catalogKey)) {
    for (const mat of doc.getRoot().listMaterials()) if (mat.getAlphaMode() === "BLEND") mat.setAlphaMode("OPAQUE");
  }
  dropLooseOcclusion(doc);
  const scene = doc.getRoot().listScenes()[0];
  const turn = V2_TURNS[catalogKey] ?? 0;
  await wrapAndScale(doc, scene, target.w, target.d, target.h, { turn, uniform: !!target.uniform });
  ensureDir(TMP);
  const tmpPath = path.join(TMP, `${catalogKey}.raw.glb`);
  await io.write(tmpPath, doc);

  const outPath = path.join(PACK, "models", `${catalogKey}.glb`);
  await optimizeToPack(tmpPath, outPath, catalogKey);
  const stats = await finalStats(outPath);
  return {
    file: `models/${catalogKey}.glb`,
    source: "polyhaven",
    source_url: `https://polyhaven.com/a/${id}`,
    source_asset_id: id,
    license: LICENSE,
    back_axis: "-z",
    ...(turn ? { turn_deg: turn } : {}),
    ...(target.uniform ? { scale: "uniformly by height" } : {}),
    ...(V2_OPTIMIZE_OVERRIDES[catalogKey] ? { optimize: V2_OPTIMIZE_OVERRIDES[catalogKey].join(" ") } : {}),
    ...(V2_TEXTURE_SIZE[catalogKey] ? { texture_px: V2_TEXTURE_SIZE[catalogKey] } : {}),
    ...(V2_OPAQUE.has(catalogKey) ? { alpha: "BLEND leaves set to OPAQUE (they carry no alpha)" } : {}),
    ...stats,
  };
}

/** A copy of a node subtree. The copies reference the same meshes, so no geometry is duplicated. */
function cloneNodeTree(doc, node) {
  const copy = doc.createNode(node.getName()).setTranslation(node.getTranslation()).setRotation(node.getRotation()).setScale(node.getScale());
  if (node.getMesh()) copy.setMesh(node.getMesh());
  for (const child of node.listChildren()) copy.addChild(cloneNodeTree(doc, child));
  return copy;
}

/**
 * dining-set-4 / dining-set-6: a Poly Haven table plus chairs on its two long
 * sides, facing it. The chair backs sit on the set's outer edge, so the GLB
 * fills the catalog footprint (chairs included) and the seats go under the
 * table edge by whatever depth is left over.
 */
async function buildPolyHavenSet(catalogKey, target) {
  const set = POLYHAVEN_SETS[catalogKey];
  const io = await makeIO();
  ensureDir(TMP);
  const [tw, td, th] = set.tableSize;
  const [cw, cd, ch] = SET_CHAIR_SIZE;

  const tableDoc = await io.read(await polyHavenModel(set.table));
  dropLooseOcclusion(tableDoc);
  await wrapAndScale(tableDoc, tableDoc.getRoot().listScenes()[0], tw, td, th);
  const tablePath = path.join(TMP, `${catalogKey}.table.glb`);
  await io.write(tablePath, tableDoc);

  // One normalized chair; each seat is a placed copy of its node tree.
  const chairDoc = await io.read(await polyHavenModel(set.chair));
  dropLooseOcclusion(chairDoc);
  const scene = chairDoc.getRoot().listScenes()[0];
  await wrapAndScale(chairDoc, scene, cw, cd, ch, { turn: V2_TURNS["chair-wood-white"] ?? 0 });
  const chair = scene.listChildren()[0]; // the "normalized-outer" node
  scene.removeChild(chair);
  const z = target.d / 2 - cd / 2;
  let first = true;
  for (let i = 0; i < set.chairsPerSide; i++) {
    for (const side of [-1, 1]) {
      const x = -tw / 2 + (tw / set.chairsPerSide) * (i + 0.5);
      const placed = chairDoc.createNode(`chair-${side}-${i}`).addChild(first ? chair : cloneNodeTree(chairDoc, chair));
      first = false;
      placed.setTranslation([x / 1000, 0, (side * z) / 1000]);
      // A chair faces +Z; the ones on the +Z side turn round to face the table.
      if (side === 1) placed.setRotation([0, 1, 0, 0]);
      scene.addChild(placed);
    }
  }
  const chairsPath = path.join(TMP, `${catalogKey}.chairs.glb`);
  await io.write(chairsPath, chairDoc);

  const mergedPath = path.join(TMP, `${catalogKey}.merged.glb`);
  gltfTransform(["merge", tablePath, chairsPath, mergedPath, "--merge-scenes"]);
  const outPath = path.join(PACK, "models", `${catalogKey}.glb`);
  await optimizeToPack(mergedPath, outPath, catalogKey);
  const stats = await finalStats(outPath);
  const seats = set.chairsPerSide * 2;
  return {
    file: `models/${catalogKey}.glb`,
    source: "polyhaven",
    source_url: `https://polyhaven.com/a/${set.table}`,
    source_asset_id: `${set.table} + ${seats}x ${set.chair} (composed)`,
    license: LICENSE,
    back_axis: "-z",
    composed_from: [
      { id: set.table, source_url: `https://polyhaven.com/a/${set.table}`, size_mm: set.tableSize },
      { id: set.chair, source_url: `https://polyhaven.com/a/${set.chair}`, size_mm: SET_CHAIR_SIZE, count: seats },
    ],
    chair_tuck_mm: Math.round(cd - (target.d - td) / 2),
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

/** Builds every catalog model, or only the keys in `only`. Sources are fetched as they are needed. */
async function buildModels(only = null) {
  log("\n== Models ==");
  const result = {};
  for (const target of CATALOG.filter((c) => !only || only.includes(c.key))) {
    const key = target.key;
    if (key === "dining-4" || key === "dining-6") {
      const chairsPerSide = key === "dining-4" ? 2 : 3;
      result[key] = await buildDiningSet(key, await kenneyModels(), target, chairsPerSide);
    } else if (POLYHAVEN_SETS[key]) {
      result[key] = await buildPolyHavenSet(key, target);
    } else if (POLYHAVEN_MODELS[key]) {
      const id = POLYHAVEN_MODELS[key];
      result[key] = await buildPolyHavenHeroModel(key, await polyHavenModel(id), target);
    } else if (POLYHAVEN_V2[key]) {
      result[key] = await buildPolyHavenV2Model(key, target);
    } else if (KENNEY_SINGLE[key]) {
      result[key] = await buildSingleKenneyModel(key, await kenneyModels(), KENNEY_SINGLE[key], target);
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
  lines.push(
    "Rebuild: `node scripts/assets-build.mjs [--raw <dir>]` (idempotent, skips cached downloads in the raw cache: `assets/raw/` by default, or `--raw` / `GUHIT_ASSETS_RAW`). A clean cache downloads about 440 MB, 214 MB of it the jacaranda tree."
  );
  lines.push("");
  lines.push(
    "The Kenney Furniture Kit zip has to be placed by hand on a clean cache: kenney.nl now hands it out only through a signed link made for a browser session, and its old media URL redirects to the asset page. Download it (v1.0, CC0) from https://kenney.nl/assets/furniture-kit into `<raw>/kenney/kenney_furniture-kit.zip`."
  );
  lines.push("");
  lines.push(
    "Check: `node scripts/assets-build.mjs --check` (no downloads, under a second; a full build runs it too). It fails when a file under `public/assets/pack/` is not in `manifest.json` or a listed file is missing, when an entry that ships a file is not CC0 1.0 or its source is not on polyhaven.com, ambientcg.com or kenney.nl, when a model key is not a catalog key in `crates/guhit-model/src/catalog.rs`, or when the pack is over 40 MB."
  );
  lines.push("");
  const packBytes = listFiles(PACK).reduce((sum, f) => sum + fs.statSync(path.join(PACK, f)).size, 0);
  lines.push(`Pack size: ${packBytes} bytes (${fmtBytes(packBytes)}) of the 40 MB budget.`);
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
  lines.push(`| Background | \`${manifest.hdri.background_file}\` (${fmtBytes(manifest.hdri.background_bytes)}, ${manifest.hdri.background_px.join("x")} JPG) |`);
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
  const paving = manifest.materials["mat-paving-stone"];
  if (paving) {
    lines.push(`\`mat-paving-stone\`, "${paving.name}". ${paving.note}`);
    lines.push("");
  }
  lines.push("## Models");
  lines.push("");
  lines.push(
    "Every model's origin is the footprint center, underside at y=0, and its back sits at local -Z"
  );
  lines.push(
    "(`docs/CONTRACT.md`: local +y in plan is the back; `src/viewer3d/scene/assets.ts` maps that to"
  );
  lines.push("three.js -z). How that was checked:");
  lines.push("");
  lines.push(
    "- Wave 1 (Kenney kit and the three Poly Haven heroes): already facing that way, no rotation. Checked on Kenney's isometric preview renders for `bedDouble` and `loungeSofa` (headboard and backrest on -Z) and with a geometric heuristic across the rest: the centroid of each mesh's upper 40% of vertices, which sits over the tall or detailed side of an asymmetric object, falls on -Z."
  );
  lines.push(
    "- Catalog v2 (Poly Haven scans): the same centroid, plus the tallest point on each half of the depth, the surface area in the front and back 15% of the depth, and flat-shaded side, top, front and back renders of each built GLB, compared with the Poly Haven thumbnail where those were unsure (the covered car, the picture frame, the mirror, the drawer cabinet, the cube shelves). Seat backs, the bench back, the stove's knobs and oven door, the sideboard's slatted doors, the drawer fronts, the car's hood and the frame and mirror faces all sit on +Z, so none of those turn. The one exception is `shelf-cube`: `wooden_display_shelves_01` is modeled with its open cubes on +X (its 1078 mm width runs along Z), so the build turns it -90 degrees about +Y. Turns are baked into the GLB: every v2 entry has `back_axis` -z. The wall items (`wall-art`, `mirror-wall`) keep their flat back board on -Z, the wall side."
  );
  lines.push("");
  const isV2 = (key) => !!(POLYHAVEN_V2[key] || POLYHAVEN_SETS[key]);
  lines.push("### Wave 1 models");
  lines.push("");
  lines.push("| Catalog key | Source | Bbox (mm, w x d x h) | Triangles | Bytes |");
  lines.push("|---|---|---|---|---|");
  for (const [key, m] of Object.entries(manifest.models)) {
    if (isV2(key)) continue;
    if (m.source === "none") {
      lines.push(`| \`${key}\` | none (procedural fallback in the viewer) | - | - | - |`);
      continue;
    }
    lines.push(`| \`${key}\` | ${m.source === "kenney" ? "Kenney" : "Poly Haven"}: ${m.source_file ?? m.source_asset_id} | ${m.bbox_mm.width}x${m.bbox_mm.depth}x${m.bbox_mm.height} | ${m.triangles} | ${fmtBytes(m.bytes)} |`);
  }
  lines.push("");
  lines.push("### Catalog v2 models");
  lines.push("");
  lines.push(
    "Photoscanned CC0 models from Poly Haven, one per new catalog key, normalized to the catalog size and optimized with meshopt and WebP textures. Textures are 1024 px for the large pieces and 512 px where noted (the small pieces, the chairs, the wall items and the decor): in the GPU a 1024 px map takes about 5.6 MB (RGBA plus mipmaps), a 512 px one 1.4 MB. No v2 model keeps an occlusion map of its own; the viewer draws its own ambient occlusion. The sets count their table and one chair, since every seat shares the chair mesh."
  );
  lines.push("");
  lines.push("| Catalog key | Source | Bbox (mm, w x d x h) | Triangles | Bytes | Build notes |");
  lines.push("|---|---|---|---|---|---|");
  for (const [key, m] of Object.entries(manifest.models)) {
    if (!isV2(key) || !m.file) continue;
    const src = m.composed_from
      ? m.composed_from.map((p) => `[${p.id}](${p.source_url})${p.count ? ` x${p.count}` : ""}`).join(" + ")
      : `[${m.source_asset_id}](${m.source_url})`;
    const notes = [];
    if (m.turn_deg) notes.push(`turned ${m.turn_deg} deg`);
    if (m.scale) notes.push(`scaled ${m.scale}`);
    if (m.optimize) notes.push(`simplify \`${m.optimize}\``);
    if (m.texture_px) notes.push(`${m.texture_px} px textures`);
    if (m.alpha) notes.push(m.alpha);
    if (m.composed_from) notes.push(`table ${m.composed_from[0].size_mm.join("x")}, chairs ${m.composed_from[1].size_mm.join("x")}, seats ${m.chair_tuck_mm} mm under the table edge`);
    lines.push(`| \`${key}\` | ${src} | ${m.bbox_mm.width}x${m.bbox_mm.depth}x${m.bbox_mm.height} | ${m.triangles} | ${fmtBytes(m.bytes)} | ${notes.join("; ") || "-"} |`);
  }
  lines.push("");
  lines.push(
    "Stretched more than 15% away from the scan's own proportions to fill the catalog size: `sideboard` (2440 mm wide scan made 2000 mm at the same 680 mm height), `desk-metal` (2000 x 947 mm made 1600 x 760, height kept), `microwave` (the scan is 1074 mm wide; at 500 x 370 x 300 it is about 20% taller than its own proportions) and the tables of `dining-set-4` and `dining-set-6` (the 1134 x 706 mm scan made 1200 x 800 and 1800 x 900). `tree-flowering` is scaled uniformly instead: `jacaranda_tree` is 24 x 18 m in plan and 19.8 m tall, so at 5.5 m tall it is about 6.8 x 5.3 m, the catalog footprint."
  );
  lines.push("");
  lines.push("Not covered by any CC0 source (procedural fallback in `src/viewer3d/scene/assets.ts` stays in use):");
  lines.push("");
  for (const [key, reason] of Object.entries(NO_SOURCE)) {
    lines.push(`- \`${key}\`: ${reason}`);
  }
  lines.push("");
  lines.push(
    "Kenney stays for the beds, the bath fixtures, the kitchen appliances and the other Wave 1 keys (`sofa-2`, `sofa-3`, `wardrobe`, `tv-console`, `desk`, `dining-4`, `dining-6`). Poly Haven has no usable beds (only bare or antique frames: `GothicBed_01`, `old_bed_frame`, `vintage_day_bed`) and no toilets, sinks, showers, fridges or washers, so those stay Kenney or procedural. Its classic sofas come in as keys of their own (`sofa-leather` from `sofa_02`, `sofa-wood` from `chinese_sofa`) next to the plain modern Kenney ones."
  );
  lines.push("");
  lines.push(
    "The v2 keys were chosen for homes in the Philippines: a narra-like wooden sala sofa (`sofa-wood`), the white monobloc chair (`chair-monobloc`), a rattan basket (`basket-rattan`) and the ceiling fan (`ceiling-fan`), with a low wooden table, a rocking chair, a bench, a planter box, a covered car, an iron gate, a lantern lamp post and a flowering tree for the porch and the site."
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
  lines.push("`baseColorFactor` replaced, before the optimize pass. Poly Haven models are never recolored.");
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
  if (manifest.hdri.background_file) referenced.push(manifest.hdri.background_file);
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
    if (!target) {
      log(`  UNKNOWN  ${key}: no catalog entry in this script`);
      ok = false;
      continue;
    }
    const isDiningSet = key === "dining-4" || key === "dining-6";
    const wErr = Math.abs(m.bbox_mm.width - target.w) / target.w;
    const dErr = Math.abs(m.bbox_mm.depth - target.d) / target.d;
    const hErr = Math.abs(m.bbox_mm.height - target.h) / target.h;
    // dining-4/dining-6: the catalog depth is the table's own footprint;
    // the composed model's depth legitimately exceeds it once chairs are
    // pulled out around it, so only width (which is not affected by chair
    // placement) is checked against the catalog for those two keys. A
    // `uniform` model keeps its scan's proportions: only height is checked.
    // dining-set-4/6 are checked against their full footprint like any key.
    const bad = target.uniform ? hErr > 0.10001 : wErr > 0.10001 || (!isDiningSet && (dErr > 0.10001 || hErr > 0.10001));
    if (bad) {
      log(`  TOLERANCE  ${key}: bbox ${m.bbox_mm.width}x${m.bbox_mm.depth}x${m.bbox_mm.height} vs catalog ${target.w}x${target.d}x${target.h} (${(wErr * 100).toFixed(1)}%/${(dErr * 100).toFixed(1)}%/${(hErr * 100).toFixed(1)}%)`);
      ok = false;
    } else if (isDiningSet) {
      log(`  note   ${key}: depth ${m.bbox_mm.depth}mm includes chairs pulled out around the ${target.d}mm table (expected)`);
    }
    if (!(m.triangles > 0)) {
      log(`  EMPTY  ${key}: no triangles`);
      ok = false;
    }
    const limit = V2_SIZE_LIMIT[key] ?? MODEL_LIMIT_BYTES;
    if ((POLYHAVEN_V2[key] || POLYHAVEN_SETS[key]) && m.bytes > limit) {
      log(`  TOO BIG  ${key}: ${fmtBytes(m.bytes)} over the ${fmtBytes(limit)} limit for this v2 model`);
      ok = false;
    }
  }
  log(`  gltf-transform inspect + bbox tolerance (10%) + v2 size limit: ${ok ? "pass" : "FAIL"}`);

  return ok;
}

// ---------------------------------------------------------------------------
// License and pack check (`--check`): reads the manifest, the pack folder and
// catalog.rs only. No downloads, no gltf-transform, so it runs in a second.
// ---------------------------------------------------------------------------

/** Every file under `dir`, as paths relative to it with forward slashes. */
function listFiles(dir, base = dir) {
  const out = [];
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) out.push(...listFiles(p, base));
    else out.push(path.relative(base, p).split(path.sep).join("/"));
  }
  return out;
}

/** True when `url` is on one of the approved source hosts (or a subdomain of one). */
function approvedHost(url) {
  try {
    const host = new URL(url).hostname;
    return SOURCE_HOSTS.some((h) => host === h || host.endsWith("." + h));
  } catch {
    return false;
  }
}

function check(manifest) {
  log("\n== Check (license, files, catalog keys, size) ==");
  const problems = [];

  // 1. Files on disk vs files the manifest references.
  const entries = [];
  if (manifest.hdri) entries.push({ label: "hdri", files: [manifest.hdri.file, manifest.hdri.background_file].filter(Boolean), entry: manifest.hdri });
  for (const [id, m] of Object.entries(manifest.materials ?? {})) entries.push({ label: `material ${id}`, files: m.maps ?? [], entry: m });
  for (const [key, m] of Object.entries(manifest.models ?? {})) entries.push({ label: `model ${key}`, files: m.file ? [m.file] : [], entry: m });
  const referenced = new Set(["manifest.json"]);
  for (const e of entries) for (const f of e.files) referenced.add(f);
  const onDisk = listFiles(PACK);
  for (const f of onDisk) if (!referenced.has(f)) problems.push(`unreferenced file: public/assets/pack/${f} (not in manifest.json)`);
  const present = new Set(onDisk);
  for (const f of referenced) if (!present.has(f)) problems.push(`missing file: public/assets/pack/${f} (referenced by manifest.json)`);

  // 2. License and source host of every entry that ships a file.
  for (const e of entries) {
    if (e.files.length === 0) continue;
    if (e.entry.license !== LICENSE) problems.push(`${e.label}: license "${e.entry.license}", not ${LICENSE}`);
    const url = e.entry.source_url ?? e.entry.sourceUrl;
    if (!approvedHost(url)) problems.push(`${e.label}: source URL "${url}" is not on ${SOURCE_HOSTS.join(", ")}`);
    for (const part of e.entry.composed_from ?? []) {
      if (part.source_url && !approvedHost(part.source_url)) problems.push(`${e.label}: part source URL "${part.source_url}" is not on ${SOURCE_HOSTS.join(", ")}`);
    }
  }

  // 3. Model keys must be catalog keys (`item("key", ...` in catalog.rs).
  const rs = fs.readFileSync(CATALOG_RS, "utf8");
  const catalogKeys = new Set([...rs.matchAll(/\bitem\(\s*"([^"]+)"/g)].map((m) => m[1]));
  for (const key of Object.keys(manifest.models ?? {})) {
    if (!catalogKeys.has(key)) problems.push(`model ${key}: not a catalog key in crates/guhit-model/src/catalog.rs`);
  }
  // Catalog items that ask for a pack model (`.pack()`) but have none fall back
  // to the procedural form: worth a line, not a failure.
  const packed = [...rs.matchAll(/\bitem\(\s*"([^"]+)"[^\n]*\.pack\(\)/g)].map((m) => m[1]);
  const noModel = packed.filter((k) => !manifest.models?.[k]?.file);
  if (noModel.length) log(`  note   ${noModel.length} catalog key(s) with .pack() and no pack model (procedural form): ${noModel.join(", ")}`);

  // 4. Pack size.
  const total = onDisk.reduce((sum, f) => sum + fs.statSync(path.join(PACK, f)).size, 0);
  const mb = (n) => (n / (1024 * 1024)).toFixed(2);
  log(`  pack size: ${total} bytes = ${mb(total)} MB (limit ${PACK_LIMIT_BYTES} bytes = ${mb(PACK_LIMIT_BYTES)} MB; 1 MB = 1024 x 1024 bytes)`);
  if (total > PACK_LIMIT_BYTES) problems.push(`pack is ${total} bytes (${mb(total)} MB), over the ${mb(PACK_LIMIT_BYTES)} MB limit`);

  log(`  ${onDisk.length} files on disk, ${referenced.size} referenced, ${catalogKeys.size} catalog keys, ${Object.keys(manifest.models ?? {}).length} model entries`);
  for (const p of problems) log(`  PROBLEM  ${p}`);
  log(`  check: ${problems.length === 0 ? "pass" : `FAIL (${problems.length} problem${problems.length === 1 ? "" : "s"})`}`);
  return problems.length === 0;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  if (CHECK_ONLY) {
    const ok = check(JSON.parse(fs.readFileSync(path.join(PACK, "manifest.json"), "utf8")));
    log(ok ? "\nCheck passed." : "\nCheck FAILED, see above.");
    process.exit(ok ? 0 : 1);
  }
  ensureDir(RAW);
  ensureDir(PACK);
  ensureDir(TMP);
  log(`raw cache: ${RAW}`);

  let manifest;
  if (VERIFY_ONLY) {
    manifest = JSON.parse(fs.readFileSync(path.join(PACK, "manifest.json"), "utf8"));
  } else if (ONLY) {
    // `--only hdri,mat-ground-grass,sofa-leather`: the sky, the named material
    // presets and the named models, the rest of the manifest kept as it is.
    const parts = ONLY.split(",");
    const old = JSON.parse(fs.readFileSync(path.join(PACK, "manifest.json"), "utf8"));
    const hdri = parts.includes("hdri") ? await buildHdri() : old.hdri;
    const modelKeys = parts.filter((p) => CATALOG.some((c) => c.key === p));
    const presets = parts.filter((p) => p !== "hdri" && !modelKeys.includes(p));
    const materials = presets.length > 0 ? { ...old.materials, ...(await buildMaterials(presets)) } : old.materials;
    const models = modelKeys.length > 0 ? { ...old.models, ...(await buildModels(modelKeys)) } : old.models;
    manifest = writeManifest(hdri, materials, models);
    writeAssetsDoc(manifest);
    log(`\nWrote ${parts.join(", ")} into ${path.relative(ROOT, path.join(PACK, "manifest.json"))} and assets/ASSETS.md`);
  } else {
    const hdri = await buildHdri();
    const materials = await buildMaterials();
    log("\n== Sources ==");
    await kenneyModels();
    for (const id of polyHavenIds()) await polyHavenModel(id);
    const models = await buildModels();
    manifest = writeManifest(hdri, materials, models);
    writeAssetsDoc(manifest);
    log(`\nWrote ${path.relative(ROOT, path.join(PACK, "manifest.json"))} and assets/ASSETS.md`);
  }

  const verified = await verify(manifest);
  const checked = check(manifest);
  const ok = verified && checked;
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
