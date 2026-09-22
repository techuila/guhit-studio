// Readable names for elements and tools, and the starter suggestions.
import type { AiProposal, DocState, Element } from "../contract/bindings";

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
  }
}

export type ChangeKind = "added" | "changed" | "removed";

export interface AffectedRow {
  id: string;
  change: ChangeKind;
  kind: Element["kind"];
  name: string;
}

/** Elements a proposal touches, with names. Removed ones are named from the current document. */
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
  add_wall: "Add wall",
  add_wall_chain: "Add walls",
  add_rect_room: "Add room",
  add_door: "Add door",
  add_window: "Add window",
  resize_room: "Resize room",
  set_wall_length: "Set wall length",
  move_elements: "Move",
  rename_room: "Rename room",
  set_room_usage: "Set room usage",
  set_opening_size: "Resize opening",
  delete_elements: "Delete",
  set_material: "Set material",
  set_roof: "Change roof",
  add_asset: "Place item",
};

export function toolLabel(name: string): string {
  return TOOL_LABEL[name] ?? name;
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
