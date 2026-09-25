// The review list: open items grouped by level and room, set-aside marks,
// resolved marks, and the triage keys. Pure data, no React.
//
// Review items are suggestions. An item can be set aside with a note, for one
// finding, a whole check, or a check on one object (DECISIONS D24); resolved
// is derived by the engine. Nothing here, or anywhere, reads "approved".
import type { DocState, Element, Issue, Point, ReviewMark, ReviewTarget, RoomGeometry, Severity } from "../contract/bindings";
import { levelOfElement } from "./levels";

type Doc = Pick<DocState, "project" | "derived">;

// ---------------------------------------------------------------- check names

/** What each check looks for, as a plural noun phrase: "Narrow doors". */
const CHECK_LABEL: Record<string, string> = {
  room_no_window: "Rooms without a window",
  room_no_door: "Rooms without a door",
  room_small: "Small rooms",
  door_narrow: "Narrow doors",
  opening_blocked: "Openings where a wall meets",
  opening_near_corner: "Openings near a corner",
  wall_end_gap_start: "Walls stopping short at the start",
  wall_end_gap_end: "Walls stopping short at the end",
  wall_dangling_start: "Wall starts joined to nothing",
  wall_dangling_end: "Wall ends joined to nothing",
  wall_overlap: "Overlapping walls",
  pipe_through_column: "Pipes through a column",
  pipe_across_opening: "Pipes across a door or window",
  pipes_cross: "Crossing pipes",
  drain_slope_low: "Drains that fall too little",
  pipe_penetrations: "Sleeves and flashings",
  light_no_switch: "Lights with no switch",
  switch_no_load: "Switches that control nothing",
  switch_behind_door: "Switches behind a door",
  aircon_no_outlet: "Aircon units with no outlet",
  lineset_long: "Line sets over the maximum length",
  lineset_rise: "Line sets over the maximum rise",
  lineset_short: "Line sets under 3 m",
  lineset_extra: "Line set length past the standard install",
  condensate_slope_low: "Condensate drains that fall too little",
  condensate_open_end: "Condensate drains ending away from a drain",
  indoor_unit_clearance: "Indoor unit clearances",
  outdoor_unit_clearance: "Outdoor unit clearances",
  outdoor_unit_unsupported: "Outdoor units without support",
  unit_near_tv: "Aircon units near a TV",
};

/** "Narrow doors", or the code in words for a check this build does not know. */
export function checkLabel(code: string): string {
  const known = CHECK_LABEL[code];
  if (known) return known;
  const words = code.replace(/_/g, " ").trim();
  return words ? words[0].toUpperCase() + words.slice(1) : "A check";
}

// ---------------------------------------------------------------- targets

export type AsideScope = "issue" | "check" | "element";

/** The mark target for setting this item aside: the item, its whole check, or its check on its first object. */
export function targetFor(scope: AsideScope, issue: Pick<Issue, "id" | "code" | "element_ids">): ReviewTarget | null {
  if (scope === "issue") return { kind: "issue", id: issue.id };
  if (scope === "check") return { kind: "check", code: issue.code };
  const element = issue.element_ids[0];
  return element ? { kind: "element", code: issue.code, element_id: element } : null;
}

export function targetKey(t: ReviewTarget): string {
  switch (t.kind) {
    case "issue":
      return `issue:${t.id}`;
    case "check":
      return `check:${t.code}`;
    case "element":
      return `element:${t.code}:${t.element_id}`;
  }
}

/** True when the mark covers this item. Display grouping only; the engine sets `Issue::status`. */
export function markCovers(t: ReviewTarget, issue: Pick<Issue, "id" | "code" | "element_ids">): boolean {
  switch (t.kind) {
    case "issue":
      return t.id === issue.id;
    case "check":
      return t.code === issue.code;
    case "element":
      return t.code === issue.code && issue.element_ids.includes(t.element_id);
  }
}

// ---------------------------------------------------------------- open items by level and room

const SEVERITY_RANK: Record<Severity, number> = { error: 0, warning: 1, info: 2 };

export interface RoomGroup {
  key: string;
  roomId: string | null;
  label: string;
  /** Warnings first, then the engine's order. */
  items: Issue[];
  worst: Severity;
}

export interface LevelGroup {
  key: string;
  levelId: string | null;
  label: string;
  count: number;
  rooms: RoomGroup[];
}

export interface Place {
  levelId: string | null;
  roomId: string | null;
}

function pointInPolygon(p: Point, poly: readonly Point[]): boolean {
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const a = poly[i];
    const b = poly[j];
    if (a.y > p.y !== b.y > p.y && p.x < ((b.x - a.x) * (p.y - a.y)) / (b.y - a.y) + a.x) inside = !inside;
  }
  return inside;
}

/** Points on both sides of a wall at `at`, a little past its faces. */
function beside(start: Point, end: Point, at: Point, thicknessMm: number, first: "left" | "right"): Point[] {
  const len = Math.hypot(end.x - start.x, end.y - start.y) || 1;
  const nx = -(end.y - start.y) / len;
  const ny = (end.x - start.x) / len;
  const d = thicknessMm / 2 + 150;
  const left = { x: at.x + nx * d, y: at.y + ny * d };
  const right = { x: at.x - nx * d, y: at.y - ny * d };
  return first === "left" ? [left, right] : [right, left];
}

/** Where to look for the room of an item: its location, then points on its first element. */
function probePoints(issue: Issue, first: Element | undefined, byId: Map<string, Element>): Point[] {
  const out: Point[] = [];
  if (issue.location) out.push({ x: issue.location.x, y: issue.location.y });
  if (!first) return out;
  switch (first.kind) {
    case "asset":
      out.push(first.position);
      break;
    case "column":
      out.push(first.center);
      break;
    case "stair":
      out.push(first.origin);
      break;
    case "annotation":
      out.push(first.position);
      break;
    case "dimension":
      out.push({ x: (first.a.x + first.b.x) / 2, y: (first.a.y + first.b.y) / 2 });
      break;
    case "pipe": {
      const pts = first.points;
      if (pts.length > 0) {
        const mid = pts[Math.floor((pts.length - 1) / 2)];
        const next = pts[Math.min(pts.length - 1, Math.floor((pts.length - 1) / 2) + 1)];
        out.push({ x: (mid.x + next.x) / 2, y: (mid.y + next.y) / 2 });
      }
      break;
    }
    case "opening": {
      const host = byId.get(first.wall_id);
      if (host?.kind !== "wall") break;
      const len = Math.hypot(host.end.x - host.start.x, host.end.y - host.start.y) || 1;
      const t = first.offset_mm / len;
      const at = { x: host.start.x + (host.end.x - host.start.x) * t, y: host.start.y + (host.end.y - host.start.y) * t };
      // A door leaf swings into the room it serves (flip_side false: to the left).
      out.push(...beside(host.start, host.end, at, host.thickness_mm, first.flip_side ? "right" : "left"));
      break;
    }
    case "wall": {
      const { start, end } = first;
      const len = Math.hypot(end.x - start.x, end.y - start.y) || 1;
      const ux = (end.x - start.x) / len;
      const uy = (end.y - start.y) / len;
      const inward = Math.min(150, len / 2);
      // Wall end checks are about one end.
      const at = /_start$/.test(issue.code)
        ? { x: start.x + ux * inward, y: start.y + uy * inward }
        : /_end$/.test(issue.code)
          ? { x: end.x - ux * inward, y: end.y - uy * inward }
          : { x: (start.x + end.x) / 2, y: (start.y + end.y) / 2 };
      out.push(...beside(start, end, at, first.thickness_mm, "left"));
      break;
    }
    default:
      break;
  }
  return out;
}

interface RoomShape {
  id: string;
  levelId: string;
  net: Point[];
  centerline: Point[];
}

function roomShapes(doc: Doc): RoomShape[] {
  const geo = new Map<string, RoomGeometry>(doc.derived.rooms.map((r) => [r.room_id, r]));
  const out: RoomShape[] = [];
  for (const e of doc.project.elements) {
    if (e.kind !== "room") continue;
    const g = geo.get(e.id);
    if (g) out.push({ id: e.id, levelId: e.level_id, net: g.polygon, centerline: g.centerline_polygon });
  }
  return out;
}

/** The level and room an item belongs to. A room among its elements wins; else the room around its location or first element. */
export function placeIssue(issue: Issue, doc: Doc, byId: Map<string, Element>, rooms: RoomShape[] = roomShapes(doc)): Place {
  const els = issue.element_ids.map((id) => byId.get(id)).filter((e): e is Element => e !== undefined);
  const room = els.find((e) => e.kind === "room");
  if (room && room.kind === "room") return { levelId: room.level_id, roomId: room.id };
  const first = els[0];
  const levelId = first ? levelOfElement(first, byId) : null;
  if (!levelId) return { levelId: null, roomId: null };
  const onLevel = rooms.filter((r) => r.levelId === levelId);
  const probes = probePoints(issue, first, byId);
  for (const key of ["net", "centerline"] as const) {
    for (const p of probes) {
      const hit = onLevel.find((r) => r[key].length >= 3 && pointInPolygon(p, r[key]));
      if (hit) return { levelId, roomId: hit.id };
    }
  }
  return { levelId, roomId: null };
}

export const OUTSIDE_ROOMS = "Outside the rooms";
export const WHOLE_PROJECT = "Whole project";

/**
 * Open items grouped by level (in the project's level order), then by room.
 * Rooms with a warning come before rooms with only notes; items inside a room
 * are warnings first. Both keep the engine's order otherwise.
 */
export function groupReview(doc: Doc, items: Issue[]): LevelGroup[] {
  const byId = new Map(doc.project.elements.map((e) => [e.id, e]));
  const rooms = roomShapes(doc);
  const order = new Map(items.map((issue, i) => [issue.id, i]));
  const levels = new Map<string, { levelId: string | null; rooms: Map<string, { roomId: string | null; items: Issue[] }> }>();
  for (const issue of items) {
    const place = placeIssue(issue, doc, byId, rooms);
    const lk = place.levelId ?? "";
    let level = levels.get(lk);
    if (!level) levels.set(lk, (level = { levelId: place.levelId, rooms: new Map() }));
    const rk = place.roomId ?? "";
    let room = level.rooms.get(rk);
    if (!room) level.rooms.set(rk, (room = { roomId: place.roomId, items: [] }));
    room.items.push(issue);
  }
  const levelRank = new Map(doc.project.levels.map((l, i) => [l.id, i]));
  const roomName = (id: string) => {
    const el = byId.get(id);
    return el?.kind === "room" && el.name.trim() ? el.name : "Room";
  };
  const bySeverity = (a: Issue, b: Issue) => SEVERITY_RANK[a.severity] - SEVERITY_RANK[b.severity] || (order.get(a.id) ?? 0) - (order.get(b.id) ?? 0);

  return [...levels.entries()]
    .sort(([, a], [, b]) => (a.levelId === null ? 1 : 0) - (b.levelId === null ? 1 : 0) || (levelRank.get(a.levelId ?? "") ?? 0) - (levelRank.get(b.levelId ?? "") ?? 0))
    .map(([lk, level]) => {
      const roomGroups: RoomGroup[] = [...level.rooms.entries()].map(([rk, r]) => {
        const sorted = [...r.items].sort(bySeverity);
        return {
          key: `${lk}/${rk}`,
          roomId: r.roomId,
          label: r.roomId ? roomName(r.roomId) : level.levelId ? OUTSIDE_ROOMS : WHOLE_PROJECT,
          items: sorted,
          worst: sorted[0].severity,
        };
      });
      roomGroups.sort((a, b) => SEVERITY_RANK[a.worst] - SEVERITY_RANK[b.worst] || (order.get(a.items[0].id) ?? 0) - (order.get(b.items[0].id) ?? 0));
      const levelName = level.levelId ? (doc.project.levels.find((l) => l.id === level.levelId)?.name ?? "Level") : WHOLE_PROJECT;
      return {
        key: lk || "project",
        levelId: level.levelId,
        label: levelName,
        count: roomGroups.reduce((n, g) => n + g.items.length, 0),
        rooms: roomGroups,
      };
    });
}

// ---------------------------------------------------------------- set aside and resolved

export interface AsideRow {
  key: string;
  mark: ReviewMark;
  /** The item, or the check and object the mark covers. */
  title: string;
  /** Items the mark sets aside right now. */
  issues: Issue[];
  /** Elements to show. */
  elementIds: string[];
}

/**
 * The marks in `Project::review`, except those for findings that are gone
 * (`Derived::review_resolved`), with the items each covers.
 */
export function asideRows(doc: Doc, nameOf: (id: string) => string | null): AsideRow[] {
  const resolved = new Set(doc.derived.review_resolved.map((m) => targetKey(m.target)));
  const issues = doc.derived.issues;
  const out: AsideRow[] = [];
  for (const mark of doc.project.review) {
    const key = targetKey(mark.target);
    if (resolved.has(key)) continue;
    const t = mark.target;
    const covered = issues.filter((i) => markCovers(t, i));
    if (t.kind === "issue") {
      const issue = covered[0];
      out.push({ key, mark, title: issue?.message ?? resolvedTitle(t.id), issues: covered, elementIds: issue?.element_ids ?? [] });
    } else if (t.kind === "check") {
      out.push({ key, mark, title: `${checkLabel(t.code)}, everywhere`, issues: covered, elementIds: [...new Set(covered.flatMap((i) => i.element_ids))] });
    } else {
      const name = nameOf(t.element_id);
      out.push({ key, mark, title: `${checkLabel(t.code)}, on ${name ?? "an object that is gone"}`, issues: covered, elementIds: name ? [t.element_id] : [] });
    }
  }
  return out;
}

/** Messages of items seen this session, so a resolved mark can still say what it was. */
const seenMessages = new Map<string, string>();

export function rememberIssues(issues: Issue[]): void {
  for (const i of issues) seenMessages.set(i.id, i.message);
}

/** What a resolved finding was: its last message, else its check. */
export function resolvedTitle(issueId: string): string {
  const seen = seenMessages.get(issueId);
  if (seen) return seen;
  // Engine ids read "<code>:<element ids>".
  const code = issueId.includes(":") ? issueId.slice(0, issueId.indexOf(":")) : "";
  return code && CHECK_LABEL[code] ? `${CHECK_LABEL[code]}: an item the checks no longer find` : "An item the checks no longer find";
}

// ---------------------------------------------------------------- triage keys

export interface TriageRow {
  key: string;
  /** Open items can be set aside; set-aside rows can be reopened. */
  kind: "open" | "aside";
}

export type TriageAction =
  | { type: "move"; key: string }
  | { type: "set_aside"; key: string }
  | { type: "reopen"; key: string }
  | { type: "show"; key: string };

export interface KeyInfo {
  key: string;
  altKey?: boolean;
  ctrlKey?: boolean;
  metaKey?: boolean;
  shiftKey?: boolean;
}

/**
 * What a key does while the review list has focus: up and down move, S sets
 * the item aside, O reopens a set-aside row, Enter shows it. Null for any
 * other key, which then reaches the global shortcuts (MOD+S still saves).
 */
export function triageKey(e: KeyInfo, rows: TriageRow[], activeKey: string | null): TriageAction | null {
  if (rows.length === 0 || e.altKey || e.ctrlKey || e.metaKey) return null;
  const at = rows.findIndex((r) => r.key === activeKey);
  const active = at >= 0 ? rows[at] : null;
  switch (e.key) {
    case "ArrowDown":
      return { type: "move", key: rows[at < 0 ? 0 : Math.min(rows.length - 1, at + 1)].key };
    case "ArrowUp":
      return { type: "move", key: rows[at < 0 ? 0 : Math.max(0, at - 1)].key };
    case "Home":
      return { type: "move", key: rows[0].key };
    case "End":
      return { type: "move", key: rows[rows.length - 1].key };
    case "Enter":
      return active ? { type: "show", key: active.key } : null;
    case "s":
    case "S":
      if (e.shiftKey) return null;
      return active?.kind === "open" ? { type: "set_aside", key: active.key } : null;
    case "o":
    case "O":
      if (e.shiftKey) return null;
      return active?.kind === "aside" ? { type: "reopen", key: active.key } : null;
    default:
      return null;
  }
}

/** The row to land on when `key` leaves the list: the next one, else the one before. */
export function neighbourAfterRemoval(rows: TriageRow[], key: string): string | null {
  const at = rows.findIndex((r) => r.key === key);
  if (at < 0) return rows[0]?.key ?? null;
  return rows[at + 1]?.key ?? rows[at - 1]?.key ?? null;
}
