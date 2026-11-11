// Pure helpers for the Sheets panel and the views viewer (lane L4): reading
// the viewport boxes the engine writes into a sheet SVG, the position math
// for a dragged viewport, the views a project has, and the set the panel
// makes when the engine's `MakeSheetSet` is not there yet. No DOM, no store.
import type { Command, DocState, Element, PaperSize, ScheduleKind, Sheet, SheetKind, ViewRef, Viewport } from "../../contract/bindings";

type Doc = Pick<DocState, "project">;
type Rect = { x: number; y: number; w: number; h: number };

/** One viewport as the sheet SVG lays it out, paper mm, y down. */
export interface ViewportBox extends Rect {
  id: string;
  /** Content and its title. */
  frame: Rect;
  /** 1:N, or null for a table or an image. */
  scale: number | null;
  /** True when the user placed it; false when laid out in order. */
  placed: boolean;
}

/** What a sheet SVG says about its layout. */
export interface SheetLayout {
  page: { w: number; h: number };
  /** Where viewports may go; the title block is to its right. */
  area: Rect;
  viewports: ViewportBox[];
}

function attrs(tag: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const m of tag.matchAll(/([a-zA-Z_:-]+)="([^"]*)"/g)) out[m[1]] = m[2];
  return out;
}

const unescape = (s: string) => s.replace(/&quot;/g, '"').replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
const num = (v: string | undefined) => (v === undefined || v === "" ? NaN : Number(v));

/** Reads the `drawing-area` and `viewport` groups out of a sheet SVG. */
export function parseSheetSvg(svg: string): SheetLayout | null {
  const areaTag = svg.match(/<g id="drawing-area"[^>]*>/)?.[0];
  if (!areaTag) return null;
  const a = attrs(areaTag);
  const [pw, ph] = (a["data-page"] ?? "").split(" ").map(Number);
  const area = { x: num(a["data-x"]), y: num(a["data-y"]), w: num(a["data-w"]), h: num(a["data-h"]) };
  if (![pw, ph, area.x, area.y, area.w, area.h].every(Number.isFinite)) return null;
  const viewports: ViewportBox[] = [];
  for (const m of svg.matchAll(/<g class="viewport"[^>]*>/g)) {
    const v = attrs(m[0]);
    const [fx, fy, fw, fh] = (v["data-frame"] ?? "").split(" ").map(Number);
    const box: ViewportBox = {
      id: unescape(v["data-id"] ?? ""),
      x: num(v["data-x"]),
      y: num(v["data-y"]),
      w: num(v["data-w"]),
      h: num(v["data-h"]),
      frame: { x: fx, y: fy, w: fw, h: fh },
      scale: v["data-scale"] ? Number(v["data-scale"]) : null,
      placed: v["data-placed"] === "true",
    };
    if ([box.x, box.y, box.w, box.h, fx, fy, fw, fh].every(Number.isFinite)) viewports.push(box);
  }
  return { page: { w: pw, h: ph }, area, viewports };
}

/**
 * The `Viewport.position` after dragging a box by (dx, dy) paper mm, SVG y
 * down: the content's center, in mm from the paper's bottom left.
 */
export function droppedPosition(box: Rect, dx: number, dy: number, pageH: number): { x: number; y: number } {
  const round = (v: number) => Math.round(v * 10) / 10;
  return { x: round(box.x + box.w / 2 + dx), y: round(pageH - (box.y + box.h / 2 + dy)) };
}

/** Where a row dragged by `dy` px from `from` lands, rows `rowH` apart. */
export function dropIndex(from: number, dy: number, rowH: number, count: number): number {
  if (count <= 0 || rowH <= 0) return 0;
  return Math.max(0, Math.min(count - 1, Math.round(from + dy / rowH)));
}

/** Rows between the drag source and its target shift by one row to make room. */
export function shiftFor(index: number, from: number, to: number): number {
  if (from < to && index > from && index <= to) return -1;
  if (from > to && index < from && index >= to) return 1;
  return 0;
}

// ------------------------------------------------------------------ views

const marks = (doc: Doc, kind: "elevation" | "section") =>
  doc.project.elements.filter((e): e is Extract<Element, { kind: "view_mark" }> => e.kind === "view_mark" && e.mark_kind === kind);

export const SCHEDULES: Array<{ kind: ScheduleKind; label: string }> = [
  { kind: "doors_windows", label: "Doors and windows" },
  { kind: "rooms", label: "Rooms" },
  { kind: "finishes", label: "Finishes" },
  { kind: "areas", label: "Floor areas" },
];

function planTitle(name: string): string {
  const up = name.trim().toUpperCase();
  if (!up) return "FLOOR PLAN";
  return up.endsWith("PLAN") ? up : `${up} PLAN`;
}

const SERVICE: Partial<Record<SheetKind, string>> = {
  lighting: "LIGHTING LAYOUT",
  power: "POWER LAYOUT",
  plumbing: "PLUMBING LAYOUT",
  aircon: "AIRCON LAYOUT",
};

/** The title the engine gives a view ("NORTH ELEVATION"), for lists. */
export function viewTitle(doc: Doc, view: ViewRef): string {
  switch (view.type) {
    case "plan": {
      const name = doc.project.levels.find((l) => l.id === view.level_id)?.name ?? "";
      if (view.sheet === "plan") return planTitle(name);
      if (view.sheet === "reflected_ceiling") return `${planTitle(name).replace(/ PLAN$/, "")} REFLECTED CEILING PLAN`;
      if (view.sheet === "plumbing_isometric") return "PLUMBING ISOMETRIC DIAGRAMS";
      return `${name.trim().toUpperCase()} ${SERVICE[view.sheet] ?? "LAYOUT"}`.trim();
    }
    case "elevation":
    case "section": {
      const m = doc.project.elements.find((e) => e.kind === "view_mark" && e.id === view.mark_id);
      const label = m && m.kind === "view_mark" ? m.label.trim().toUpperCase() : "";
      if (view.type === "elevation") return label ? (label.endsWith("ELEVATION") ? label : `${label} ELEVATION`) : "ELEVATION";
      return label ? (label.startsWith("SECTION") ? label : `SECTION ${label}-${label}`) : "SECTION";
    }
    case "roof_plan":
      return "ROOF PLAN";
    case "site_plan":
      return "SITE DEVELOPMENT PLAN";
    case "vicinity_map":
      return "VICINITY MAP";
    case "perspective":
      return "PERSPECTIVE";
    case "schedule":
      return (
        {
          doors_windows: "SCHEDULE OF DOORS AND WINDOWS",
          rooms: "ROOM SCHEDULE",
          finishes: "SCHEDULE OF FINISHES",
          areas: "FLOOR AREAS",
        } as const
      )[view.schedule];
    case "notes":
      return "GENERAL NOTES";
  }
}

/** A stable key for a view, for lists and selection. */
export function viewKey(view: ViewRef): string {
  switch (view.type) {
    case "plan":
      return `plan:${view.level_id}:${view.sheet}`;
    case "elevation":
    case "section":
      return `${view.type}:${view.mark_id}`;
    case "schedule":
      return `schedule:${view.schedule}`;
    case "perspective":
      return `perspective:${view.render_id}`;
    case "vicinity_map":
      return `vicinity:${view.file_name ?? ""}`;
    default:
      return view.type;
  }
}

export interface ViewChoice {
  group: "Plans" | "Ceiling plans" | "Elevations" | "Sections" | "Roof and site" | "Schedules";
  label: string;
  view: ViewRef;
}

/** Every drawing the project has, in the order a set lists them. */
export function projectViews(doc: Doc): ViewChoice[] {
  const out: ViewChoice[] = [];
  for (const l of doc.project.levels) out.push({ group: "Plans", label: l.name || "Level", view: { type: "plan", level_id: l.id, sheet: "plan" } });
  for (const l of doc.project.levels) out.push({ group: "Ceiling plans", label: l.name || "Level", view: { type: "plan", level_id: l.id, sheet: "reflected_ceiling" } });
  for (const m of marks(doc, "elevation")) out.push({ group: "Elevations", label: viewLabel(doc, { type: "elevation", mark_id: m.id }), view: { type: "elevation", mark_id: m.id } });
  for (const m of marks(doc, "section")) out.push({ group: "Sections", label: viewLabel(doc, { type: "section", mark_id: m.id }), view: { type: "section", mark_id: m.id } });
  out.push({ group: "Roof and site", label: "Roof plan", view: { type: "roof_plan" } });
  out.push({ group: "Roof and site", label: "Site development plan", view: { type: "site_plan" } });
  for (const s of SCHEDULES) out.push({ group: "Schedules", label: s.label, view: { type: "schedule", schedule: s.kind } });
  return out;
}

// --------------------------------------------------------------- the set

const vp = (view: ViewRef): Viewport => ({ id: "", view, scale_denominator: null, position: null, title: null });

function sheetOf(title: string, paper: PaperSize, views: ViewRef[]): Sheet {
  return { id: "", number: "", title, paper, orientation: "landscape", viewports: views.map(vp), revision_ids: [] };
}

/**
 * The permit set as one batch of `UpsertSheet`, the same set the export
 * prints for a project with no sheets (`sheetset::default_set`): a site
 * sheet when there is a lot, a plan per level with walls, elevations and
 * sections two to a sheet, the roof plan, and the schedules.
 */
export function defaultSetCommand(doc: Doc, paper: PaperSize): Command {
  const els = doc.project.elements;
  const sheets: Sheet[] = [];
  if (els.some((e) => e.kind === "lot")) {
    sheets.push(sheetOf("Site development plan and vicinity map", paper, [{ type: "site_plan" }, { type: "vicinity_map", file_name: null, radius_m: 500 }]));
  }
  for (const l of doc.project.levels) {
    if (els.some((e) => e.kind === "wall" && e.level_id === l.id)) {
      sheets.push(sheetOf(sentenceCase(planTitle(l.name)), paper, [{ type: "plan", level_id: l.id, sheet: "plan" }]));
    }
  }
  const pairs = <T,>(xs: T[]) => xs.reduce<T[][]>((acc, x, i) => (i % 2 === 0 ? [...acc, [x]] : [...acc.slice(0, -1), [...acc[acc.length - 1], x]]), []);
  for (const pair of pairs(marks(doc, "elevation"))) sheets.push(sheetOf("Elevations", paper, pair.map((m) => ({ type: "elevation", mark_id: m.id }))));
  for (const pair of pairs(marks(doc, "section"))) sheets.push(sheetOf("Sections", paper, pair.map((m) => ({ type: "section", mark_id: m.id }))));
  if (els.some((e) => e.kind === "roof")) sheets.push(sheetOf("Roof plan", paper, [{ type: "roof_plan" }]));
  if (doc.project.levels.length > 0) {
    sheets.push(sheetOf("Schedules", paper, SCHEDULES.map((s) => ({ type: "schedule", schedule: s.kind }) as ViewRef)));
  }
  return {
    type: "batch",
    label: "Make permit set",
    commands: sheets.map((sheet) => ({ type: "upsert_sheet", sheet, index: null })),
  };
}

/** "GROUND FLOOR PLAN" as "Ground floor plan". */
export function sentenceCase(t: string): string {
  const lower = t.toLowerCase();
  return lower.charAt(0).toUpperCase() + lower.slice(1);
}

/**
 * A view's name in a list beside level names: sentence case, with a mark
 * kept as drawn ("Section A-A", "North elevation"). On paper the title stays
 * uppercase (`viewTitle`), as drawings are titled.
 */
export function viewLabel(doc: Doc, view: ViewRef): string {
  const words = viewTitle(doc, view).split(" ");
  return words
    .map((w, i) => {
      // A section mark (A-A, 1-1, B2-B2) or a word with a digit stays as it is.
      if (/^[A-Z0-9]+(-[A-Z0-9]+)+$/.test(w) || /\d/.test(w)) return w;
      const lower = w.toLowerCase();
      return i === 0 ? lower.charAt(0).toUpperCase() + lower.slice(1) : lower;
    })
    .join(" ");
}

/** The next free revision mark: "1", "2", ... */
export function nextRevisionMark(marks: string[]): string {
  let n = 1;
  while (marks.includes(String(n))) n++;
  return String(n);
}

/** Today on this computer, "YYYY-MM-DD". */
export function today(d = new Date()): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export const SCALES = [20, 25, 50, 75, 100, 125, 150, 200, 250, 300, 400, 500];
export const PAPERS: Array<{ value: PaperSize; label: string }> = [
  { value: "a0", label: "A0" },
  { value: "a1", label: "A1" },
  { value: "a2", label: "A2" },
  { value: "a3", label: "A3" },
];
