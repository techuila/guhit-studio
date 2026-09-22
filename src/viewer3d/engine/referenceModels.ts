// Imported 3D models (Element::ReferenceModel) shown next to the live model
// as context: a site survey, a neighbour, a SketchUp massing. Loading is
// async and keyed by file name so several elements sharing one file load it
// once; a missing or oversized file falls back to a labelled wireframe box
// and never throws. Not part of `buildScene` (that function is pure, no I/O):
// this store owns its own little group, added straight to the engine's
// scene, so it is never touched by the cutaway clip (which only walks
// `built.root`).

import * as THREE from "three";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { OBJLoader } from "three/examples/jsm/loaders/OBJLoader.js";
import { planRotationToWorld, planToWorld } from "../geom/coords";
import type { MaterialLibrary } from "../scene/materials";

/** Above this many triangles a model stays a placeholder and a toast warns instead. */
export const REFERENCE_TRIANGLE_CAP = 2_000_000;
const PLACEHOLDER_M = 1;

export interface ReferenceModelPlacement {
  id: string;
  fileName: string;
  position: { x: number; y: number };
  /** Absolute height above the project zero (level elevation + the element's own). */
  elevationMm: number;
  rotationDeg: number;
  scaleToMm: number;
}

type FileStatus =
  | { kind: "loading" }
  | { kind: "ready"; object: THREE.Object3D; triangles: number }
  | { kind: "missing" }
  | { kind: "oversized"; triangles: number };

interface Entry {
  mount: THREE.Group;
  fileName: string;
  content: THREE.Object3D;
  status: FileStatus["kind"];
}

export interface ReferenceModelCallbacks {
  /** Fetches the stored file as a data URL. Rejects when it cannot be read. */
  fetchModel: (fileName: string) => Promise<string>;
  /** The placeholder just swapped for the loaded geometry: play the entrance motion. */
  onSwap: (elementId: string, mount: THREE.Group) => void;
  invalidate: () => void;
  /** Routed to the app toast (a large model was kept as a placeholder). */
  warn: (message: string) => void;
}

function countTriangles(obj: THREE.Object3D): number {
  let n = 0;
  obj.traverse((o) => {
    const mesh = o as THREE.Mesh;
    if (!mesh.isMesh) return;
    const idx = mesh.geometry.getIndex();
    const count = idx ? idx.count : (mesh.geometry.getAttribute("position")?.count ?? 0);
    n += count / 3;
  });
  return Math.round(n);
}

/** OBJ carries no material info without a companion MTL: flatten its default material to neutral grey. */
function neutralizeIfBare(obj: THREE.Object3D, lib: MaterialLibrary): void {
  let grey: THREE.Material | null = null;
  obj.traverse((o) => {
    const mesh = o as THREE.Mesh;
    if (!mesh.isMesh || Array.isArray(mesh.material)) return;
    const mat = mesh.material as THREE.Material & { map?: THREE.Texture | null };
    const bare = mat.type === "MeshPhongMaterial" && !mat.map;
    if (!bare) return;
    grey ??= lib.plain("#9a9a9a", 0.85);
    mesh.material = grey;
    mesh.castShadow = true;
    mesh.receiveShadow = true;
  });
}

function labelSprite(text: string): THREE.Sprite | null {
  if (typeof document === "undefined") return null;
  const canvas = document.createElement("canvas");
  canvas.width = 320;
  canvas.height = 56;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.fillStyle = "rgba(24,24,22,0.8)";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.fillStyle = "#f4f2ec";
  ctx.font = "28px sans-serif";
  ctx.textBaseline = "middle";
  const label = text.length > 34 ? `${text.slice(0, 31)}...` : text;
  ctx.fillText(label, 12, canvas.height / 2);
  const tex = new THREE.CanvasTexture(canvas);
  tex.colorSpace = THREE.SRGBColorSpace;
  const sprite = new THREE.Sprite(new THREE.SpriteMaterial({ map: tex, depthTest: false, transparent: true }));
  sprite.scale.set(1.3, 0.23, 1);
  sprite.renderOrder = 5;
  return sprite;
}

function disposeObject(root: THREE.Object3D): void {
  root.traverse((o) => {
    const mesh = o as THREE.Mesh;
    if (mesh.isMesh) mesh.geometry?.dispose();
    const sprite = o as THREE.Sprite;
    if (sprite.isSprite) {
      sprite.material.map?.dispose();
      sprite.material.dispose();
    }
    const line = o as THREE.LineSegments;
    if (line.isLineSegments) {
      line.geometry?.dispose();
      (line.material as THREE.Material)?.dispose();
    }
  });
}

/** Node's `Buffer`, when running under vitest: no `@types/node` in this project, so it is read off `globalThis`. */
type NodeBufferCtor = {
  from(data: string, encoding: string): { byteOffset: number; byteLength: number; buffer: ArrayBuffer };
};
const nodeBuffer = (globalThis as unknown as { Buffer?: NodeBufferCtor }).Buffer;

function dataUrlToArrayBuffer(dataUrl: string): ArrayBuffer {
  const comma = dataUrl.indexOf(",");
  const header = dataUrl.slice(5, comma);
  const base64 = header.includes("base64");
  const payload = dataUrl.slice(comma + 1);
  if (nodeBuffer) {
    const buf = base64 ? nodeBuffer.from(payload, "base64") : nodeBuffer.from(decodeURIComponent(payload), "utf-8");
    return buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength);
  }
  const binary = base64 ? atob(payload) : decodeURIComponent(payload);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes.buffer;
}

function dataUrlToText(dataUrl: string): string {
  return new TextDecoder().decode(dataUrlToArrayBuffer(dataUrl));
}

export class ReferenceModelStore {
  readonly root = new THREE.Group();
  private entries = new Map<string, Entry>();
  private files = new Map<string, FileStatus>();
  private gltf = new GLTFLoader();
  private obj = new OBJLoader();

  constructor(
    private lib: MaterialLibrary,
    private cb: ReferenceModelCallbacks,
  ) {
    this.root.name = "reference-models";
  }

  /** Call on every doc change with the placements currently visible (layer + level filtering already applied). */
  sync(items: ReferenceModelPlacement[], layerVisible: boolean, layerLocked: boolean): void {
    const seen = new Set(items.map((i) => i.id));
    for (const [id, entry] of this.entries) {
      if (seen.has(id)) continue;
      this.root.remove(entry.mount);
      disposeObject(entry.mount);
      this.entries.delete(id);
    }
    for (const item of items) {
      let entry = this.entries.get(item.id);
      if (!entry || entry.fileName !== item.fileName) {
        if (entry) {
          this.root.remove(entry.mount);
          disposeObject(entry.mount);
        }
        entry = this.createEntry(item);
        this.entries.set(item.id, entry);
        this.root.add(entry.mount);
      }
      this.place(entry, item);
      entry.mount.visible = layerVisible;
      entry.mount.traverse((o) => {
        o.userData.elementId = item.id;
        if (layerLocked) o.userData.locked = true;
        else delete o.userData.locked;
      });
      this.requestLoad(item.fileName);
    }
  }

  /** The mount for a selected/hovered element id, so the engine can highlight it like any other. */
  getMount(elementId: string): THREE.Group | null {
    return this.entries.get(elementId)?.mount ?? null;
  }

  /** Snapshot for export: the current mounts (placeholder or loaded), already positioned in scene space. */
  exportSnapshot(): Map<string, THREE.Object3D> {
    const out = new Map<string, THREE.Object3D>();
    for (const [id, entry] of this.entries) out.set(id, entry.mount);
    return out;
  }

  stats(): { files: number; entries: number; loaded: number } {
    let loaded = 0;
    for (const s of this.files.values()) if (s.kind === "ready") loaded++;
    return { files: this.files.size, entries: this.entries.size, loaded };
  }

  private place(entry: Entry, item: ReferenceModelPlacement): void {
    const [x, y, z] = planToWorld(item.position.x, item.position.y, item.elevationMm);
    entry.mount.position.set(x, y, z);
    entry.mount.rotation.y = planRotationToWorld(item.rotationDeg);
    const scale = Math.max(item.scaleToMm, 0.001) / 1000;
    entry.mount.scale.setScalar(scale);
  }

  private createEntry(item: ReferenceModelPlacement): Entry {
    const mount = new THREE.Group();
    mount.name = `reference_model-${item.id}`;
    const placeholder = this.buildPlaceholder(item.fileName, false);
    mount.add(placeholder);
    return { mount, fileName: item.fileName, content: placeholder, status: "loading" };
  }

  private buildPlaceholder(label: string, missing: boolean): THREE.Object3D {
    const g = new THREE.Group();
    g.name = "placeholder";
    const box = new THREE.LineSegments(
      new THREE.EdgesGeometry(new THREE.BoxGeometry(PLACEHOLDER_M, PLACEHOLDER_M, PLACEHOLDER_M)),
      new THREE.LineBasicMaterial({ color: missing ? 0xc23b3b : 0x8a8f98 }),
    );
    box.position.y = PLACEHOLDER_M / 2;
    g.add(box);
    const fill = new THREE.Mesh(
      new THREE.BoxGeometry(PLACEHOLDER_M, PLACEHOLDER_M, PLACEHOLDER_M),
      this.lib.plain(missing ? "#c23b3b" : "#8a8f98", 0.9),
    );
    fill.position.y = PLACEHOLDER_M / 2;
    fill.material.transparent = true;
    fill.material.opacity = 0.12;
    fill.userData.soft = true;
    g.add(fill);
    const sprite = labelSprite(missing ? `missing: ${label}` : label);
    if (sprite) {
      sprite.position.y = PLACEHOLDER_M + 0.16;
      g.add(sprite);
    }
    return g;
  }

  private requestLoad(fileName: string): void {
    const existing = this.files.get(fileName);
    if (existing) {
      if (existing.kind !== "loading") this.applyToAll(fileName, existing);
      return;
    }
    this.files.set(fileName, { kind: "loading" });
    this.cb
      .fetchModel(fileName)
      .then((dataUrl) => this.parse(fileName, dataUrl))
      .then((object) => {
        const triangles = countTriangles(object);
        neutralizeIfBare(object, this.lib);
        object.traverse((o) => {
          o.castShadow = true;
          o.receiveShadow = true;
        });
        const status: FileStatus =
          triangles > REFERENCE_TRIANGLE_CAP ? { kind: "oversized", triangles } : { kind: "ready", object, triangles };
        this.files.set(fileName, status);
        if (status.kind === "oversized") {
          this.cb.warn(`${fileName}: ${triangles.toLocaleString()} triangles, over the 2,000,000 limit. Kept as a placeholder.`);
        }
        this.applyToAll(fileName, status);
      })
      .catch(() => {
        const status: FileStatus = { kind: "missing" };
        this.files.set(fileName, status);
        this.applyToAll(fileName, status);
      });
  }

  private applyToAll(fileName: string, status: FileStatus): void {
    for (const [id, entry] of this.entries) if (entry.fileName === fileName) this.applyFileStatus(id, status);
  }

  private applyFileStatus(elementId: string, status: FileStatus): void {
    const entry = this.entries.get(elementId);
    if (!entry || status.kind === "loading") return;
    if (status.kind === "ready" && entry.status === "ready") return; // already swapped
    if (status.kind === "oversized") {
      entry.status = "oversized";
      return; // the placeholder stays
    }
    entry.mount.remove(entry.content);
    disposeObject(entry.content);
    const next = status.kind === "ready" ? status.object.clone(true) : this.buildPlaceholder(entry.fileName, true);
    next.traverse((o) => {
      o.userData.elementId = elementId;
    });
    entry.mount.add(next);
    entry.content = next;
    entry.status = status.kind;
    if (status.kind === "ready") this.cb.onSwap(elementId, entry.mount);
    this.cb.invalidate();
  }

  private async parse(fileName: string, dataUrl: string): Promise<THREE.Object3D> {
    if (/\.obj$/i.test(fileName)) return this.obj.parse(dataUrlToText(dataUrl));
    const buf = dataUrlToArrayBuffer(dataUrl);
    return new Promise((resolve, reject) => {
      this.gltf.parse(buf, "", (gltf) => resolve(gltf.scene), (err) => reject(err));
    });
  }

  dispose(): void {
    for (const entry of this.entries.values()) disposeObject(entry.mount);
    this.entries.clear();
    for (const status of this.files.values()) if (status.kind === "ready") disposeObject(status.object);
    this.files.clear();
  }
}
