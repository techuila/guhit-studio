// Readable names for elements and tools, and the starter suggestions.
import type { AiProposal, DocState, Element, SlabKind } from "../contract/bindings";
import { PIPE_GROUP, PIPE_SYSTEM_LABEL } from "../contract/pipes";

function mm(v: number): string {
  return `${Math.round(v).toLocaleString("en-US")} mm`;
}

const USAGE_LABEL: Record<string, string> = {
  master_bedroom: "master bedroom",
  powder_room: "powder room",
};

export function elementName(el: Element): string {
  switch (el.kind) {
    case "wall":
      return `Wall, ${mm(Math.hypot(el.end.x - el.start.x, el.end.y - el.start.y))}`;
    case "opening":
      return `${el.opening_type === "door" ? "Door" : "Window"}, ${Math.round(el.width_mm)} x ${Math.round(el.height_mm)}`;
    case "room":
      return el.name || `Room (${USAGE_LABEL[el.usage] ?? el.usage})`;
    case "column":
      return "Column";
    case "stair":
      return "Stair";
    case "asset":
      return el.name;
    case "annotation":
      return `Text "${el.text}"`;
    case "dimension":
      return "Dimension";
    case "camera":
      return `Camera ${el.name}`;
    case "underlay":
      return `Underlay ${el.file_name}`;
    case "linework":
      return el.name;
    case "reference_model":
      return el.name;
    case "pipe": {
      // "Cold water pipe, 20 mm", "Conduit, 20 mm", "Refrigerant line set, 9.52 mm".
      const run = PIPE_GROUP[el.system] === "plumbing" ? `${PIPE_SYSTEM_LABEL[el.system]} pipe` : PIPE_SYSTEM_LABEL[el.system];
      return el.name || `${run}, ${Math.round(el.diameter_mm * 100) / 100} mm`;
    }
    case "slab":
      return SLAB_NAME[el.slab_kind];
    case "roof":
      return el.outline.type === "top_footprint" ? "Roof" : `Roof, ${el.shape}`;
    case "grid":
      return `Grid line ${el.label}`;
    case "view_mark":
      return `${el.mark_kind === "section" ? "Section" : "Elevation"} mark ${el.label}`;
    case "lot":
      return "Lot";
    case "tour":
      return `Tour ${el.name}`;
  }
}

const SLAB_NAME: Record<SlabKind, string> = {
  floor: "Floor slab",
  balcony: "Balcony",
  deck: "Deck",
  void: "Floor void",
};

export type ChangeKind = "added" | "changed" | "removed";

export interface AffectedRow {
  id: string;
  change: ChangeKind;
  kind: Element["kind"];
  name: string;
}

/** Elements a proposal touches, with names. Removed ones are named from the current document. */
/** What the selection is, in a few words: "Bedroom", "Wall, 3,000 mm", "3 elements". */
export function selectionLabel(doc: DocState | null, selection: string[]): string {
  if (selection.length === 1) {
    const el = doc?.project.elements.find((e) => e.id === selection[0]);
    if (el) return elementName(el);
  }
  return selection.length === 1 ? "1 element" : `${selection.length} elements`;
}

export function affectedRows(proposal: AiProposal, doc: DocState | null): AffectedRow[] {
  const after = new Map(proposal.preview.state.project.elements.map((e) => [e.id, e]));
  const before = new Map((doc?.project.elements ?? []).map((e) => [e.id, e]));
  const rows: AffectedRow[] = [];
  const add = (ids: string[], change: ChangeKind, from: Map<string, Element>) => {
    for (const id of ids) {
      const el = from.get(id);
      rows.push({ id, change, kind: el?.kind ?? "wall", name: el ? elementName(el) : "Element" });
    }
  };
  add(proposal.preview.diff.added, "added", after);
  add(proposal.preview.diff.modified, "changed", after);
  add(proposal.preview.diff.removed, "removed", before);
  return rows;
}

const TOOL_LABEL: Record<string, string> = {
  get_project_summary: "Read project totals",
  list_rooms: "Read rooms",
  describe_elements: "Read element data",
  list_elements: "Listed elements",
  find_rooms_without_exterior_window: "Checked exterior windows",
  list_review_items: "Read review items",
  get_pipe_takeoff: "Read pipe take-off",
  get_schedule: "Read schedule",
  add_wall: "Add wall",
  add_wall_chain: "Add walls",
  add_rect_room: "Add room",
  add_door: "Add door",
  add_window: "Add window",
  resize_room: "Resize room",
  set_wall_length: "Set wall length",
  move_elements: "Move",
  move_pipe_points: "Move pipe",
  set_pipe_segment_length: "Set pipe length",
  set_pipe_slope: "Set pipe slope",
  insert_pipe_point: "Add bend",
  remove_pipe_point: "Remove bend",
  rename_room: "Rename room",
  set_room_usage: "Set room usage",
  set_opening_size: "Resize opening",
  delete_elements: "Delete",
  set_material: "Set material",
  set_roof: "Change roof",
  add_asset: "Place item",
  add_level: "Add level",
  delete_level: "Delete level",
};

/** A tool's readable name. One without a label reads as words: "get_plan" is "Get plan". */
export function toolLabel(name: string): string {
  return TOOL_LABEL[name] ?? name.replace(/_/g, " ").replace(/^./, (c) => c.toUpperCase());
}

/**
 * A model id as people read it: "claude-opus-5-5" is "Claude Opus 5.5", and a
 * dated id drops its date. An id that is not dash-separated words and
 * numbers comes back as it is. Null when there is no id.
 */
export function modelLabel(id: string | null | undefined): string | null {
  const raw = id?.trim() ?? "";
  if (!raw) return null;
  if (!/^[a-z0-9]+(-[a-z0-9]+)*$/i.test(raw)) return raw;
  const parts = raw.split("-").filter((p, i, all) => !(i === all.length - 1 && i > 0 && /^\d{8}$/.test(p)));
  const words: string[] = [];
  for (const p of parts) {
    const prev = words[words.length - 1];
    if (/^\d+$/.test(p) && prev !== undefined && /^\d+(\.\d+)*$/.test(prev)) words[words.length - 1] = `${prev}.${p}`;
    else words.push(/^[a-z]/.test(p) ? p[0].toUpperCase() + p.slice(1) : p);
  }
  return words.join(" ");
}

export interface Suggestion {
  text: string;
  /** Short language tag shown on the chip, for the Taglish example. */
  tag?: string;
}

/** Starter prompts. They follow the selection so "this room" means something. */
export function suggestionsFor(doc: DocState | null, selection: string[]): Suggestion[] {
  const selected = selection
    .map((id) => doc?.project.elements.find((e) => e.id === id))
    .filter((e): e is Element => !!e);
  const only = selected.length === 1 ? selected[0] : null;

  if (only?.kind === "room") {
    return [
      { text: "Make this room 300 mm wider to the east" },
      { text: "Rename to Master Bedroom" },
      { text: "What is the area of this room?" },
      { text: "Palakihin ang kwartong ito, 500 mm sa north side", tag: "Taglish" },
    ];
  }
  if (only?.kind === "wall") {
    return [
      { text: "Add a window at the center of this wall" },
      { text: "Make this wall exactly 3 m long" },
      { text: "Lagyan ng pinto ang wall na ito, 800 mm mula sa start", tag: "Taglish" },
    ];
  }
  if (only?.kind === "opening") {
    return [{ text: "Make this opening 1500 mm wide" }, { text: "Move this 300 mm to the right" }];
  }
  if (selected.length > 1) {
    return [{ text: "Move these 500 mm north" }, { text: "What did I select?" }];
  }
  return [
    { text: "What is the total floor area?" },
    { text: "Which rooms have no exterior window?" },
    { text: "Add a 4 x 3 m bedroom" },
    { text: "Ilan ang kwarto at ano ang total floor area?", tag: "Taglish" },
  ];
}
