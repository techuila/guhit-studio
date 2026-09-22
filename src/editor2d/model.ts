// Read-only helpers over the document mirror: layer and level filtering,
// element shapes and key points. Pure, tested in model.test.ts.

import type {
  DocState,
  Element,
  LayerKey,
  Opening,
  Project,
  RoomGeometry,
  Wall,
  WallGeometry,
} from "../contract/bindings";
import type { P, Rect } from "./geom";
import {
  add,
  dirDeg,
  emptyRect,
  expandRect,
  left,
  lerp,
  mul,
  orientedRect,
  rotate,
  sub,
  thickSegment,
  unit,
  dist,
} from "./geom";

export type WallEl = Extract<Element, { kind: "wall" }>;
export type OpeningEl = Extract<Element, { kind: "opening" }>;

export function layerOf(el: Element): LayerKey {
  switch (el.kind) {
    case "wall":
      return "walls";
    case "opening":
      return "openings";
    case "room":
      return "rooms";
    case "column":
      return "columns";
    case "stair":
      return "stairs";
    case "asset":
      return "assets";
    case "annotation":
      return "annotations";
    case "dimension":
      return "dimensions";
    case "camera":
      return "annotations";
    case "underlay":
      return "underlays";
    case "linework":
      return "underlays";
    case "reference_model":
      return "underlays";
  }
}

export interface DocIndex {
  doc: DocState;
  levelId: string | null;
  byId: Map<string, Element>;
  wallGeo: Map<string, WallGeometry>;
  roomGeo: Map<string, RoomGeometry>;
  /** Elements on the active level whose layer is visible, in draw order. */
  visible: Element[];
  visibleIds: Set<string>;
  lockedLayers: Set<LayerKey>;
}

export function levelOf(el: Element, byId: Map<string, Element>): string | null {
  if (el.kind === "opening") {
    const host = byId.get(el.wall_id);
    return host && host.kind === "wall" ? host.level_id : null;
  }
  if (el.kind === "camera") return null;
  return el.level_id;
}

export function buildIndex(doc: DocState, activeLevelId: string | null): DocIndex {
  const project: Project = doc.project;
  const byId = new Map<string, Element>();
  for (const e of project.elements) byId.set(e.id, e);
  const wallGeo = new Map<string, WallGeometry>();
  for (const g of doc.derived.walls) wallGeo.set(g.wall_id, g);
  const roomGeo = new Map<string, RoomGeometry>();
  for (const g of doc.derived.rooms) roomGeo.set(g.room_id, g);
  const hidden = new Set<LayerKey>();
  const lockedLayers = new Set<LayerKey>();
  for (const l of project.layers) {
    if (!l.visible) hidden.add(l.key);
    if (l.locked) lockedLayers.add(l.key);
  }
  const levelId = activeLevelId ?? project.levels[0]?.id ?? null;
  const visible: Element[] = [];
  for (const e of project.elements) {
    if (hidden.has(layerOf(e))) continue;
    const lv = levelOf(e, byId);
    if (lv !== null && levelId !== null && lv !== levelId) continue;
    if (e.kind === "opening") {
      const host = byId.get(e.wall_id);
      if (!host || hidden.has("walls")) continue;
    }
    visible.push(e);
  }
  return {
    doc,
    levelId,
    byId,
    wallGeo,
    roomGeo,
    visible,
    visibleIds: new Set(visible.map((e) => e.id)),
    lockedLayers,
  };
}

export function isLocked(el: Element, index: DocIndex): boolean {
  if (index.lockedLayers.has(layerOf(el))) return true;
  if (el.kind === "underlay" && el.locked) return true;
  if (el.kind === "linework" && el.locked) return true;
  if (el.kind === "reference_model" && el.locked) return true;
  return false;
}

export function wallOutline(wall: Wall, index: DocIndex | null): P[] {
  const g = index?.wallGeo.get(wall.id);
  if (g && g.outline.length >= 3) return g.outline;
  return thickSegment(wall.start, wall.end, wall.thickness_mm);
}

export interface OpeningFrame {
  /** Center of the opening on the wall centerline. */
  center: P;
  /** Unit vector along the wall, start to end. */
  dir: P;
  /** Left hand normal of `dir`. */
  normal: P;
  /** Jamb points on the centerline, start side and end side. */
  jambA: P;
  jambB: P;
  thickness: number;
  width: number;
}

export function openingFrame(o: Pick<Opening, "offset_mm" | "width_mm">, wall: Wall): OpeningFrame {
  const dir = unit(sub(wall.end, wall.start));
  const normal = left(dir);
  const center = add(wall.start, mul(dir, o.offset_mm));
  return {
    center,
    dir,
    normal,
    jambA: add(center, mul(dir, -o.width_mm / 2)),
    jambB: add(center, mul(dir, o.width_mm / 2)),
    thickness: wall.thickness_mm,
    width: o.width_mm,
  };
}

export function openingRect(f: OpeningFrame, extra = 0): P[] {
  const n = mul(f.normal, f.thickness / 2 + extra);
  return [sub(f.jambA, n), sub(f.jambB, n), add(f.jambB, n), add(f.jambA, n)];
}

/** Outline of a stair: origin is the center of the first riser, run along rotated +y. */
export function stairOutline(origin: P, rotationDeg: number, width: number, run: number): P[] {
  const hw = width / 2;
  return [
    { x: -hw, y: 0 },
    { x: hw, y: 0 },
    { x: hw, y: run },
    { x: -hw, y: run },
  ].map((p) => add(origin, rotate(p, rotationDeg)));
}

/** Rough text box used for hit testing and marquee. Centered on `center`. */
export function textBox(center: P, text: string, heightMm: number, rotationDeg = 0): P[] {
  const lines = text.split("\n");
  const longest = lines.reduce((m, l) => Math.max(m, l.length), 1);
  const w = Math.max(longest, 1) * heightMm * 0.6;
  const h = lines.length * heightMm * 1.3;
  return orientedRect(center, w, h, rotationDeg);
}

/**
 * Rough box of an annotation. Contract: `position` is the left end of the
 * first line's baseline, further lines stack below.
 */
export function annotationBox(position: P, text: string, heightMm: number, rotationDeg = 0): P[] {
  const lines = text.split("\n");
  const longest = lines.reduce((m, l) => Math.max(m, l.length), 1);
  const w = longest * heightMm * 0.58;
  const top = heightMm * 0.95;
  const bottom = -(heightMm * 0.3 + (lines.length - 1) * heightMm * ANNOTATION_LINE);
  return [
    { x: 0, y: bottom },
    { x: w, y: bottom },
    { x: w, y: top },
    { x: 0, y: top },
  ].map((p) => add(position, rotate(p, rotationDeg)));
}

/** Line pitch of multi line annotations, as a multiple of the text height. */
export const ANNOTATION_LINE = 1.3;

export interface DimensionGeometry {
  /** Ends of the dimension line. */
  p1: P;
  p2: P;
  dir: P;
  normal: P;
  length: number;
  mid: P;
}

export function dimensionGeometry(a: P, b: P, offset: number): DimensionGeometry {
  const dir = unit(sub(b, a));
  const normal = left(dir);
  const p1 = add(a, mul(normal, offset));
  const p2 = add(b, mul(normal, offset));
  return { p1, p2, dir, normal, length: dist(a, b), mid: lerp(p1, p2, 0.5) };
}

export interface ShapeOptions {
  /** Model height of room labels in mm (depends on zoom). */
  labelHeightMm: number;
}

export interface Shape {
  points: P[];
  closed: boolean;
}

/**
 * The plan shape used for marquee selection and bounds. Rooms use their label
 * box so a marquee across a room does not always pick the room.
 */
export function elementShape(el: Element, index: DocIndex, opt: ShapeOptions): Shape | null {
  switch (el.kind) {
    case "wall":
      return { points: wallOutline(el, index), closed: true };
    case "opening": {
      const host = index.byId.get(el.wall_id);
      if (!host || host.kind !== "wall") return null;
      return { points: openingRect(openingFrame(el, host)), closed: true };
    }
    case "room": {
      const g = index.roomGeo.get(el.id);
      const c = g ? g.label_point : el.seed;
      return { points: roomLabelBox(c, el.name, opt.labelHeightMm), closed: true };
    }
    case "column":
      return { points: orientedRect(el.center, el.width_mm, el.shape === "round" ? el.width_mm : el.depth_mm, el.rotation_deg), closed: true };
    case "stair":
      return { points: stairOutline(el.origin, el.rotation_deg, el.width_mm, el.run_mm), closed: true };
    case "asset":
      return { points: orientedRect(el.position, el.width_mm, el.depth_mm, el.rotation_deg), closed: true };
    case "annotation":
      return { points: annotationBox(el.position, el.text, el.size_mm, el.rotation_deg), closed: true };
    case "dimension": {
      const g = dimensionGeometry(el.a, el.b, el.offset_mm);
      return { points: [g.p1, g.p2], closed: false };
    }
    case "camera": {
      const c = { x: el.position.x, y: el.position.y };
      const r = opt.labelHeightMm * 1.2;
      return { points: orientedRect(c, r * 2, r * 2, 0), closed: true };
    }
    case "underlay": {
      const w = el.width_px * el.mm_per_px;
      const h = el.height_px * el.mm_per_px;
      const pts = [
        { x: 0, y: 0 },
        { x: w, y: 0 },
        { x: w, y: h },
        { x: 0, y: h },
      ].map((p) => add(el.position, rotate(p, el.rotation_deg)));
      return { points: pts, closed: true };
    }
    case "linework":
      return { points: el.polylines.flat(), closed: false };
    case "reference_model": {
      const r = opt.labelHeightMm * 1.2;
      return { points: orientedRect(el.position, r * 2, r * 2, 0), closed: true };
    }
  }
}

/** Two line label: name plus area. */
export function roomLabelBox(center: P, name: string, labelHeightMm: number): P[] {
  const w = Math.max(name.length, 8) * labelHeightMm * 0.62;
  const h = labelHeightMm * 2.8;
  return orientedRect(center, w, h, 0);
}

/** Points worth snapping a moved element by (nearest one to the grab point is used). */
export function keyPoints(el: Element, index: DocIndex): P[] {
  switch (el.kind) {
    case "wall":
      return [el.start, el.end, lerp(el.start, el.end, 0.5)];
    case "column":
      return [el.center];
    case "asset":
      return [el.position, ...orientedRect(el.position, el.width_mm, el.depth_mm, el.rotation_deg)];
    case "stair":
      return stairOutline(el.origin, el.rotation_deg, el.width_mm, el.run_mm);
    case "annotation":
      return [el.position];
    case "dimension":
      return [el.a, el.b];
    case "camera":
      return [{ x: el.position.x, y: el.position.y }];
    case "underlay":
      return [el.position];
    case "room": {
      const g = index.roomGeo.get(el.id);
      return g ? g.centerline_polygon : [el.seed];
    }
    case "linework":
      return el.polylines.flat();
    case "reference_model":
      return [el.position];
    case "opening":
      return [];
  }
}

export function modelBounds(index: DocIndex, includeCameras = false): Rect {
  const r = emptyRect();
  for (const el of index.visible) {
    if (el.kind === "camera" && !includeCameras) continue;
    if (el.kind === "room") {
      const g = index.roomGeo.get(el.id);
      if (g) for (const p of g.polygon) expandRect(r, p);
      continue;
    }
    if (el.kind === "dimension") {
      expandRect(r, el.a);
      expandRect(r, el.b);
    }
    const s = elementShape(el, index, { labelHeightMm: 200 });
    if (s) for (const p of s.points) expandRect(r, p);
  }
  return r;
}

export function boundsOfIds(index: DocIndex, ids: readonly string[]): Rect {
  const r = emptyRect();
  for (const id of ids) {
    const el = index.byId.get(id);
    if (!el) continue;
    if (el.kind === "room") {
      const g = index.roomGeo.get(el.id);
      if (g) for (const p of g.polygon) expandRect(r, p);
      continue;
    }
    const s = elementShape(el, index, { labelHeightMm: 200 });
    if (s) for (const p of s.points) expandRect(r, p);
  }
  return r;
}

/** Unit vector for an element rotation, pointing to its local +y ("back"). */
export function backDir(rotationDeg: number): P {
  return dirDeg(rotationDeg + 90);
}
