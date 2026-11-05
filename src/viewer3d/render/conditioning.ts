// Conditioning maps for AI renders (lane L6, DECISIONS D53): the line
// drawing, depth, normals, element ids and a clay view of a saved model
// view, drawn from its own camera so the AI keeps the model's geometry and
// the drift check has the model's lines to compare with. Saved with the
// record (`render_ai_maps_save`, renders/<id>.<map>.png).
//
// The maps come from the document, built again in a renderer of their own
// with the same builders as the live view (scene/buildScene.ts), like a
// render job: the live view is never touched and no frame is ever
// requested, so the one pending requestAnimationFrame stays the engine's
// (AGENTS.md). Each pass renders once, synchronously; the passes are spread
// over a few tasks so the window keeps answering input.
//
// - Ids: every element a flat color (`idColor`), black sky. The legend maps
//   colors to element ids and says how many pixels each covers, which is how
//   Rust knows what is in view and names what an AI image moved.
// - Normals and depth: view space, read back as floats.
// - Lines: black on white where the element changes, the surface turns
//   more than `CREASE_DEG`, or the depth jumps: the model's feature lines.
// - Depth: inverse depth, near is light. Normals: x red, y green, z blue.
// - Clay: white, evenly lit.
//
// Answers the `capture_maps` window task (shell/windowTasks.ts) and runs in
// the background after every capture and render (`queueMapsFor`).

import * as THREE from "three";
import type { Camera, DocState, IpcError, MapKind, RenderInfo, RenderRecord, ViewVisibility, WindowReply } from "../../contract/bindings";
import { ipc } from "../../contract/ipc";
import { useApp } from "../../state/store";
import { cameraToPose } from "../geom/coords";
import { buildScene } from "../scene/buildScene";
import { MaterialLibrary } from "../scene/materials";
import { aiRender, type LegendEntry, type MapsMeta } from "./aiRender";
import { MODEL_VISIBILITY } from "./liveView";

/** Long side of the maps, pixels: what the image model reads, and plenty for the drift check. */
export const MAPS_MAX_PX = 1024;
/** A surface that turns more than this between two pixels draws a line. */
export const CREASE_DEG = 30;
/** Depth that jumps by more than this share between two pixels draws a line. */
export const DEPTH_JUMP = 0.06;
/** Sky in the normal map: facing the camera, as normal maps draw it. */
const SKY_NORMAL: [number, number, number] = [128, 128, 255];
/** Clay background: light grey, so white walls still read against it. */
const CLAY_SKY = 0xf2f1ee;

// ------------------------------------------------------------------- pure

/** Index codes: 15 bits, scrambled by an odd multiplier (and back by its inverse) so neighboring elements get colors far apart and the map reads to a person too. */
const ID_BITS = 32768;
const SCRAMBLE = 7919;
const UNSCRAMBLE = 20495;

/** The ids map color of element index `i` (1 to 32767): 5 bits a channel, centered in its band, so a color survives small rounding. Index 0 is the sky, black. */
export function idColor(i: number): [number, number, number] {
  if (i <= 0) return [0, 0, 0];
  const v = (i * SCRAMBLE) % ID_BITS;
  return [((v >> 10) & 31) * 8 + 4, ((v >> 5) & 31) * 8 + 4, (v & 31) * 8 + 4];
}

/** The index of an ids map color, 0 for black. */
export function idIndexOf(r: number, g: number, b: number): number {
  if (r < 4 && g < 4 && b < 4) return 0;
  const v = ((r >> 3) << 10) | ((g >> 3) << 5) | (b >> 3);
  return (v * UNSCRAMBLE) % ID_BITS;
}

export function hexOf([r, g, b]: [number, number, number]): string {
  return `#${[r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("")}`;
}

/** The maps' size for an image of `w` x `h`: the same shape, at most `max` on the long side. */
export function mapsSize(w: number, h: number, max = MAPS_MAX_PX): { width: number; height: number } {
  const long = Math.max(1, w, h);
  const s = Math.min(1, max / long);
  return { width: Math.max(1, Math.round(w * s)), height: Math.max(1, Math.round(h * s)) };
}

/** Width and height from a PNG data URL's header, without decoding the image. */
export function pngSizeOfDataUrl(url: string): { width: number; height: number } | null {
  const comma = url.indexOf(",");
  if (comma < 0 || !url.slice(0, comma).includes("image/png")) return null;
  try {
    const head = atob(url.slice(comma + 1, comma + 1 + 44));
    if (head.slice(12, 16) !== "IHDR") return null;
    const u32 = (at: number) =>
      ((head.charCodeAt(at) << 24) | (head.charCodeAt(at + 1) << 16) | (head.charCodeAt(at + 2) << 8) | head.charCodeAt(at + 3)) >>> 0;
    const width = u32(16);
    const height = u32(20);
    return width > 0 && height > 0 ? { width, height } : null;
  } catch {
    return null;
  }
}

/** A capture's render info: its size from the PNG header and the roof and cutaway it shows. Undefined when the size cannot be read, so the record claims no more than is known. */
export function captureInfo(png: string, visibility: ViewVisibility): RenderInfo | undefined {
  const size = pngSizeOfDataUrl(png);
  if (!size) return undefined;
  return { kind: "capture", width: size.width, height: size.height, samples: 0, seconds: 0, quality: null, gpu: "", visibility };
}

/** Pixels per element index in an RGBA ids buffer. */
export function countIds(ids: Uint8Array): Map<number, number> {
  const counts = new Map<number, number>();
  for (let i = 0; i < ids.length; i += 4) {
    const n = idIndexOf(ids[i], ids[i + 1], ids[i + 2]);
    if (n > 0) counts.set(n, (counts.get(n) ?? 0) + 1);
  }
  return counts;
}

/** The legend: every element in view with its color and pixel count. `elementOf[i]` is the element of index i ("" for none). */
export function legendFrom(counts: Map<number, number>, elementOf: readonly string[]): LegendEntry[] {
  return [...counts.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([i, pixels]) => ({ color: hexOf(idColor(i)), element_id: elementOf[i] ?? "", pixels }));
}

/**
 * The line drawing: 0 (black) on a line, 255 elsewhere. A line runs between
 * two neighboring pixels of different elements (or model and sky), of the
 * same element where the surface turns more than CREASE_DEG, or where the
 * depth jumps by more than DEPTH_JUMP. Both pixels are marked, so lines are
 * two pixels wide and read at the image model's input size.
 *
 * `nd`: four floats a pixel, view space normal xyz and depth in meters (0 is
 * sky); null draws element and sky edges only.
 */
export function linesFrom(ids: Uint8Array, nd: Float32Array | null, width: number, height: number): Uint8Array {
  const out = new Uint8Array(width * height).fill(255);
  const cosCrease = Math.cos((CREASE_DEG * Math.PI) / 180);
  const idAt = (p: number) => idIndexOf(ids[p * 4], ids[p * 4 + 1], ids[p * 4 + 2]);
  const differ = (p: number, q: number): boolean => {
    const a = idAt(p);
    const b = idAt(q);
    if (a !== b) return true;
    if (a === 0 || !nd) return false;
    const da = nd[p * 4 + 3];
    const db = nd[q * 4 + 3];
    if (da <= 0 || db <= 0) return false;
    if (Math.abs(da - db) / Math.min(da, db) > DEPTH_JUMP) return true;
    const ax = nd[p * 4], ay = nd[p * 4 + 1], az = nd[p * 4 + 2];
    const bx = nd[q * 4], by = nd[q * 4 + 1], bz = nd[q * 4 + 2];
    const la = Math.hypot(ax, ay, az);
    const lb = Math.hypot(bx, by, bz);
    if (la < 1e-6 || lb < 1e-6) return false;
    return (ax * bx + ay * by + az * bz) / (la * lb) < cosCrease;
  };
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const p = y * width + x;
      if (x + 1 < width && differ(p, p + 1)) {
        out[p] = 0;
        out[p + 1] = 0;
      }
      if (y + 1 < height && differ(p, p + width)) {
        out[p] = 0;
        out[p + width] = 0;
      }
    }
  }
  return out;
}

/** Inverse depth, near light and far dark, the sky black. */
export function depthGray(nd: Float32Array, width: number, height: number): Uint8Array {
  const out = new Uint8Array(width * height);
  let near = Infinity;
  let far = 0;
  for (let p = 0; p < width * height; p++) {
    const d = nd[p * 4 + 3];
    if (d > 0) {
      near = Math.min(near, d);
      far = Math.max(far, d);
    }
  }
  if (!(far > 0)) return out;
  const inv = (d: number) => 1 / d;
  const span = Math.max(1e-9, inv(near) - inv(far));
  for (let p = 0; p < width * height; p++) {
    const d = nd[p * 4 + 3];
    // 24 to 255: the farthest surface stays apart from the sky.
    out[p] = d > 0 ? Math.round(24 + 231 * ((inv(d) - inv(far)) / span)) : 0;
  }
  return out;
}

/** View space normals as colors: x red, y green, z blue. */
export function normalRgba(nd: Float32Array, width: number, height: number): Uint8ClampedArray<ArrayBuffer> {
  const out = new Uint8ClampedArray(width * height * 4);
  for (let p = 0; p < width * height; p++) {
    const d = nd[p * 4 + 3];
    const x = nd[p * 4], y = nd[p * 4 + 1], z = nd[p * 4 + 2];
    const l = Math.hypot(x, y, z);
    const sky = d <= 0 || l < 1e-6;
    out[p * 4] = sky ? SKY_NORMAL[0] : Math.round((x / l) * 127.5 + 127.5);
    out[p * 4 + 1] = sky ? SKY_NORMAL[1] : Math.round((y / l) * 127.5 + 127.5);
    out[p * 4 + 2] = sky ? SKY_NORMAL[2] : Math.round((z / l) * 127.5 + 127.5);
    out[p * 4 + 3] = 255;
  }
  return out;
}

/** Rows bottom up (WebGL) to top down (images), any channels. */
export function flipRows<T extends Uint8Array | Uint8ClampedArray | Float32Array>(data: T, width: number, height: number, channels: number): T {
  const row = width * channels;
  const out = new (data.constructor as { new (n: number): T })(data.length);
  for (let y = 0; y < height; y++) out.set(data.subarray((height - 1 - y) * row, (height - y) * row), y * row);
  return out;
}

/** A one channel image as RGBA, for a PNG. */
export function grayRgba(gray: Uint8Array): Uint8ClampedArray<ArrayBuffer> {
  const out = new Uint8ClampedArray(gray.length * 4);
  for (let p = 0; p < gray.length; p++) {
    out[p * 4] = out[p * 4 + 1] = out[p * 4 + 2] = gray[p];
    out[p * 4 + 3] = 255;
  }
  return out;
}

// ------------------------------------------------------------------ WebGL

const VERT = /* glsl */ `
  varying vec3 vNormal;
  varying vec3 vView;
  varying vec2 vUv;
  void main() {
    vec4 local = vec4(position, 1.0);
    vec3 n = normal;
    #ifdef USE_INSTANCING
      local = instanceMatrix * local;
      n = mat3(instanceMatrix) * n;
    #endif
    vec4 mv = modelViewMatrix * local;
    vView = mv.xyz;
    vNormal = normalMatrix * n;
    #ifdef USE_CUTOUT
      vUv = uv;
    #endif
    gl_Position = projectionMatrix * mv;
  }
`;

const FRAG = /* glsl */ `
  varying vec3 vNormal;
  varying vec3 vView;
  varying vec2 vUv;
  uniform vec3 idColor;
  uniform sampler2D cutout;
  uniform float cutoutAt;
  uniform float cutoutChannel;
  void main() {
    #ifdef USE_CUTOUT
      vec4 t = texture2D(cutout, vUv);
      float a = cutoutChannel > 0.5 ? t.g : t.a;
      if (a < cutoutAt) discard;
    #endif
    #ifdef MODE_IDS
      gl_FragColor = vec4(idColor, 1.0);
    #else
      vec3 n = vNormal;
      // Meshes without normals: the face normal from screen derivatives.
      if (dot(n, n) < 1e-8) n = cross(dFdx(vView), dFdy(vView));
      n = normalize(n);
      if (!gl_FrontFacing) n = -n;
      gl_FragColor = vec4(n, -vView.z);
    #endif
  }
`;

type Mode = "ids" | "nd";

interface CutoutSource {
  texture: THREE.Texture;
  at: number;
  /** 1 reads green (an alpha map), 0 reads alpha (a color map). */
  channel: number;
}

function cutoutOf(m: THREE.Material): CutoutSource | null {
  const t = m as THREE.Material & { map?: THREE.Texture | null; alphaMap?: THREE.Texture | null };
  if (!(m.alphaTest > 0)) return null;
  if (t.alphaMap) return { texture: t.alphaMap, at: m.alphaTest, channel: 1 };
  if (t.map) return { texture: t.map, at: m.alphaTest, channel: 0 };
  return null;
}

/** Materials the passes swap in, made once per look and disposed with the job. */
class PassMaterials {
  private cache = new Map<string, THREE.Material>();

  get(mode: Mode | "clay", side: THREE.Side, cutout: CutoutSource | null, id: [number, number, number], glass = false): THREE.Material {
    const key = [mode, side, cutout?.texture.uuid ?? "", cutout?.at ?? 0, mode === "ids" ? id.join(",") : "", mode === "clay" && glass ? "glass" : ""].join("|");
    let m = this.cache.get(key);
    if (m) return m;
    if (mode === "clay") {
      // Glass is darker, so windows still read on a clay wall.
      const clay = new THREE.MeshStandardMaterial({ color: glass ? 0x6f767e : 0xd9d6cf, roughness: glass ? 0.3 : 0.92, metalness: 0, side });
      if (cutout) {
        clay.alphaTest = cutout.at;
        if (cutout.channel === 1) clay.alphaMap = cutout.texture;
        else {
          clay.map = cutout.texture;
          clay.color.set(0xffffff);
        }
      }
      m = clay;
    } else {
      const defines: Record<string, string> = {};
      if (mode === "ids") defines.MODE_IDS = "";
      if (cutout) defines.USE_CUTOUT = "";
      m = new THREE.ShaderMaterial({
        vertexShader: VERT,
        fragmentShader: FRAG,
        defines,
        side,
        uniforms: {
          idColor: { value: new THREE.Vector3(id[0] / 255, id[1] / 255, id[2] / 255) },
          cutout: { value: cutout?.texture ?? null },
          cutoutAt: { value: cutout?.at ?? 0 },
          cutoutChannel: { value: cutout?.channel ?? 0 },
        },
      });
    }
    this.cache.set(key, m);
    return m;
  }

  dispose(): void {
    for (const m of this.cache.values()) m.dispose();
    this.cache.clear();
  }
}

export interface MapsInput {
  doc: DocState;
  camera: Camera;
  /** Size of the image the maps go with; the maps keep its shape. */
  width: number;
  height: number;
  /** What the capture shows. Default: roof on, no cutaway. */
  visibility?: ViewVisibility | null;
  /** The level a cutaway cuts. Default: the one on screen. */
  activeLevelId?: string | null;
}

export interface MapsResult {
  width: number;
  height: number;
  /** PNG data URLs by kind. */
  maps: Array<{ kind: MapKind; png: string }>;
  meta: MapsMeta;
}

function encodePng(rgba: Uint8ClampedArray<ArrayBuffer>, width: number, height: number): string {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("No 2D canvas for the maps.");
  ctx.putImageData(new ImageData(rgba, width, height), 0, 0);
  return canvas.toDataURL("image/png");
}

/** Lets the window answer input between passes. */
const breathe = () => new Promise<void>((r) => setTimeout(r, 0));

interface Swap {
  mesh: THREE.Mesh;
  material: THREE.Material | THREE.Material[];
  index: number;
}

/** Draws the maps of a view of `doc`. Throws when WebGL 2 is not there. */
export async function renderMaps(input: MapsInput): Promise<MapsResult> {
  const { width, height } = mapsSize(input.width, input.height);
  const show = input.visibility ?? MODEL_VISIBILITY;
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: false, alpha: false, preserveDrawingBuffer: true });
  const lib = new MaterialLibrary();
  const passes = new PassMaterials();
  const targets: THREE.WebGLRenderTarget[] = [];
  let built: ReturnType<typeof buildScene> | null = null;
  try {
    if (!renderer.capabilities.isWebGL2) throw new Error("The maps need WebGL 2.");
    renderer.setPixelRatio(1);
    renderer.setSize(width, height, false);
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    renderer.toneMapping = THREE.NoToneMapping;

    const scene = new THREE.Scene();
    built = buildScene(input.doc, lib, {
      cutaway: show.cutaway,
      activeLevelId: show.cutaway ? (input.activeLevelId ?? useApp.getState().activeLevelId) : null,
      packModels: true,
    });
    built.roofGroup.visible = show.roof && !show.cutaway;
    scene.add(built.root);
    scene.updateMatrixWorld(true);

    const pose = cameraToPose(input.camera);
    const camera = new THREE.PerspectiveCamera(Math.min(Math.max(pose.fovDeg || 45, 5), 110), width / height, 0.05, 3000);
    camera.position.set(...pose.position);
    camera.lookAt(new THREE.Vector3(...pose.target));
    camera.updateMatrixWorld();
    camera.updateProjectionMatrix();

    // Every visible mesh with its element; lines, points and sprites hide.
    const elementOf: string[] = ["", ""];
    const indexOf = new Map<string, number>([["", 1]]);
    const swaps: Swap[] = [];
    const hidden: THREE.Object3D[] = [];
    built.root.traverseVisible((o) => {
      const mesh = o as THREE.Mesh;
      if (!mesh.isMesh) {
        if ((o as THREE.Line).isLine || (o as THREE.Points).isPoints || (o as THREE.Sprite).isSprite) hidden.push(o);
        return;
      }
      let element = "";
      for (let at: THREE.Object3D | null = o; at; at = at.parent) {
        const id = at.userData?.elementId as string | undefined;
        if (id) {
          element = id;
          break;
        }
      }
      let index = indexOf.get(element);
      if (index === undefined) {
        index = elementOf.length;
        elementOf.push(element);
        indexOf.set(element, index);
      }
      swaps.push({ mesh, material: mesh.material, index });
    });
    for (const o of hidden) o.visible = false;

    const apply = (mode: Mode | "clay") => {
      for (const s of swaps) {
        const one = (m: THREE.Material) => passes.get(mode, m.side, cutoutOf(m), idColor(s.index), m.transparent && m.opacity < 0.999);
        s.mesh.material = Array.isArray(s.material) ? s.material.map(one) : one(s.material);
      }
    };
    const restore = () => {
      for (const s of swaps) s.mesh.material = s.material;
      for (const o of hidden) o.visible = true;
    };

    try {
      // Ids, 8 bits, exact colors.
      const idsTarget = new THREE.WebGLRenderTarget(width, height, { type: THREE.UnsignedByteType, depthBuffer: true });
      targets.push(idsTarget);
      apply("ids");
      renderer.setRenderTarget(idsTarget);
      renderer.setClearColor(0x000000, 1);
      renderer.clear();
      renderer.render(scene, camera);
      const idsRaw = new Uint8Array(width * height * 4);
      renderer.readRenderTargetPixels(idsTarget, 0, 0, width, height, idsRaw);
      await breathe();

      // Normals and depth, floats when the GPU can draw them.
      let nd: Float32Array | null = null;
      if (renderer.extensions.has("EXT_color_buffer_float")) {
        const ndTarget = new THREE.WebGLRenderTarget(width, height, { type: THREE.FloatType, depthBuffer: true });
        targets.push(ndTarget);
        apply("nd");
        renderer.setRenderTarget(ndTarget);
        renderer.setClearColor(0x000000, 0);
        renderer.clear();
        renderer.render(scene, camera);
        const raw = new Float32Array(width * height * 4);
        renderer.readRenderTargetPixels(ndTarget, 0, 0, width, height, raw);
        nd = flipRows(raw, width, height, 4);
      }
      await breathe();

      // Clay, evenly lit, on the canvas itself.
      apply("clay");
      const hemi = new THREE.HemisphereLight(0xffffff, 0x6b6760, 0.9);
      const key = new THREE.DirectionalLight(0xffffff, 1.9);
      key.position.copy(camera.position).add(new THREE.Vector3(-30, 50, 20));
      key.target.position.set(...pose.target);
      scene.add(hemi, key, key.target);
      renderer.setRenderTarget(null);
      renderer.setClearColor(CLAY_SKY, 1);
      renderer.clear();
      renderer.render(scene, camera);
      const clay = canvas.toDataURL("image/png");
      scene.remove(hemi, key, key.target);
      hemi.dispose();
      key.dispose();
      restore();
      await breathe();

      const ids = flipRows(idsRaw, width, height, 4);
      const lines = linesFrom(ids, nd, width, height);
      const maps: Array<{ kind: MapKind; png: string }> = [
        { kind: "lines", png: encodePng(grayRgba(lines), width, height) },
        { kind: "ids", png: encodePng(Uint8ClampedArray.from(ids), width, height) },
        { kind: "clay", png: clay },
      ];
      if (nd) {
        maps.push({ kind: "depth", png: encodePng(grayRgba(depthGray(nd, width, height)), width, height) });
        maps.push({ kind: "normal", png: encodePng(normalRgba(nd, width, height), width, height) });
      }
      const legend = legendFrom(countIds(ids), elementOf);
      return { width, height, maps, meta: { width, height, visibility: show, legend } };
    } finally {
      restore();
    }
  } finally {
    for (const t of targets) t.dispose();
    passes.dispose();
    built?.kit.dispose();
    lib.dispose();
    renderer.setRenderTarget(null);
    renderer.dispose();
    renderer.forceContextLoss();
  }
}

// --------------------------------------------------------------- records

/** The image size of a record: from its render info, else its PNG header, else HD. */
async function sizeOf(record: RenderRecord): Promise<{ width: number; height: number }> {
  if (record.info?.width && record.info?.height) return { width: record.info.width, height: record.info.height };
  try {
    const size = pngSizeOfDataUrl(await ipc.renderData(record.id));
    if (size) return size;
  } catch {
    /* fall through */
  }
  return { width: 1920, height: 1080 };
}

/** Draws and saves the maps of a saved model view. Answers its record, maps attached. */
export async function saveMapsFor(
  record: RenderRecord,
  opts: { visibility?: ViewVisibility | null; activeLevelId?: string | null; doc?: DocState | null } = {},
): Promise<RenderRecord> {
  if (record.source !== "model_view" || record.video) {
    throw { code: "invalid", message: "Only a model view has conditioning maps.", element_ids: [] } satisfies IpcError;
  }
  const doc = opts.doc ?? useApp.getState().doc;
  if (!doc) throw { code: "no_document", message: "No project is open.", element_ids: [] } satisfies IpcError;
  const size = await sizeOf(record);
  const result = await renderMaps({
    doc,
    camera: record.camera,
    width: size.width,
    height: size.height,
    // What the image showed, as its record keeps it, unless asked otherwise.
    visibility: opts.visibility ?? record.info?.visibility ?? null,
    activeLevelId: opts.activeLevelId,
  });
  return aiRender.saveMaps(record.id, result.maps, result.meta);
}

let chain: Promise<unknown> = Promise.resolve();

/**
 * Draws a capture's maps in the background, right after it is saved, with
 * the roof and cutaway it was taken with. One at a time; a failure is only
 * logged: the capture is saved either way, and a render without maps still
 * works (without them, and without a drift check).
 */
export function queueMapsFor(record: RenderRecord, visibility: ViewVisibility | null, activeLevelId: string | null = null): Promise<RenderRecord | null> {
  const doc = useApp.getState().doc;
  const job = chain.then(async () => {
    try {
      return await saveMapsFor(record, { visibility, activeLevelId, doc });
    } catch (e) {
      console.warn("[guhit] conditioning maps not saved", record.id, e);
      return null;
    }
  });
  chain = job;
  return job;
}

/** Waits for the maps queued so far. */
export function mapsSettled(): Promise<unknown> {
  return chain;
}

/** The maps of render `renderId`, saved with it: the `capture_maps` window task. */
export async function captureMaps(renderId: string): Promise<WindowReply> {
  const records = await ipc.renderList();
  const record = records.find((r) => r.id === renderId);
  if (!record) throw { code: "not_found", message: `No saved view has id "${renderId}".`, element_ids: [] } satisfies IpcError;
  // The roof and cutaway the record says it showed. An older record without
  // them: an agent's capture shows the model's defaults unless it asked
  // otherwise (WindowTask::CaptureView::visibility), so the maps do too.
  const saved = await saveMapsFor(record, { visibility: record.info?.visibility ?? MODEL_VISIBILITY });
  const lines = saved.maps?.find((m) => m.kind === "lines");
  return {
    render_ids: [saved.id],
    image: null,
    note: lines
      ? `Saved ${saved.maps?.length ?? 0} conditioning maps with ${record.camera.name || "the view"}: lines, depth, normals, element ids and clay.`
      : "The maps were drawn without a line map.",
  };
}
