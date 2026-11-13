// The tool rail's flyout rule, and the remembered style the status bar shows.
//
// One rule for every tool with options: the first click activates the tool
// with its remembered style (D44: door and window tools keep the last style),
// a click on the active tool or on its corner mark opens its options. The
// status bar shows the remembered style as a button that opens them too, so
// the style is never hidden state.
import type { CatalogItem, ProjectSettings, StairShape, Turn, WallRole } from "../contract/bindings";
import { PIPE_DEFAULTS } from "../contract/pipes";
import type { Tool, ToolOptions } from "../state/store";
import { formatLength } from "../ui/units";
import { DOOR_STYLES, SLAB_KINDS, WINDOW_STYLES, type ToolDef } from "./actions";
import { runLabel, sizeLabel } from "./pipes";
import type { FlyoutKind } from "./shellStore";

/** Building walls make rooms; site walls are the lot fence and boundary
 * walls (`WallRole`): 1800 mm high, on the site layer, never a room. */
export const WALL_ROLES: Array<{ role: WallRole; label: string; hint: string }> = [
  { role: "building", label: "Building wall", hint: "rooms and area" },
  { role: "site", label: "Fence or site wall", hint: "1800 mm" },
];

/** Common Philippine wall builds. null keeps the project default. */
export const WALL_PRESETS: Array<{ mm: number | null; label: string; hint: string }> = [
  { mm: null, label: "Project default", hint: "" },
  { mm: 100, label: "100 mm", hint: "4 in CHB, interior" },
  { mm: 150, label: "150 mm", hint: "6 in CHB, exterior" },
  { mm: 200, label: "200 mm", hint: "8 in CHB or firewall" },
];

/** Flyouts that stay open after a pick, so two settings take one trip. */
export const STAYS_OPEN: ReadonlySet<FlyoutKind> = new Set<FlyoutKind>(["wall"]);

export interface RailState {
  /** The tool in use. */
  tool: Tool;
  /** The flyout open now, if any. */
  open: FlyoutKind | null;
  /** The object the asset tool places; it cannot activate without one. */
  assetKey: string | null;
}

/** What a click on a rail button does: activate the tool, and which flyout is open after. */
export interface RailClick {
  activate: boolean;
  flyout: FlyoutKind | null;
}

/**
 * The rail flyout rule (DECISIONS D44, orchestrator ruling R16). `corner` is
 * a click on the button's corner mark, which always reaches the options.
 */
export function railClick(def: Pick<ToolDef, "tool" | "flyout">, state: RailState, corner = false): RailClick {
  const { flyout } = def;
  if (!flyout) return { activate: true, flyout: null };
  const toggled = state.open === flyout ? null : flyout;
  // The object tool places nothing until an object is picked: the library opens.
  if (flyout === "asset" && !state.assetKey) return { activate: false, flyout: toggled };
  const active = state.tool === def.tool;
  if (active || corner) return { activate: !active, flyout: toggled };
  return { activate: true, flyout: null };
}

const lower = (s: string) => s.charAt(0).toLowerCase() + s.slice(1);

const STAIR_LABEL: Record<StairShape, string> = { straight: "Straight", l: "L", u: "U" };

/**
 * The remembered style of the tool in use, for the status bar: "150 mm,
 * building wall", "Single swing", "Balcony, railing on", "L, turns left",
 * "Cold water pipe, 20 mm". Null for a tool without options worth naming.
 */
export function rememberedStyle(
  tool: Tool,
  options: ToolOptions,
  settings: Pick<ProjectSettings, "default_wall_thickness_mm" | "display_unit"> | null | undefined,
  catalog: CatalogItem[] = [],
  stair: { shape: StairShape; turn: Turn } = { shape: "straight", turn: "left" },
): string | null {
  const unit = settings?.display_unit ?? "mm";
  if (tool === "wall") {
    const mm = options.wallThicknessMm ?? settings?.default_wall_thickness_mm ?? null;
    const role = WALL_ROLES.find((r) => r.role === options.wallRole) ?? WALL_ROLES[0];
    return [mm === null ? "project default" : formatLength(mm, unit), lower(role.label)].join(", ");
  }
  if (tool === "door" || tool === "window") {
    const styles = tool === "door" ? DOOR_STYLES : WINDOW_STYLES;
    return styles.find((s) => s.value === options.openingStyle)?.label ?? styles[0].label;
  }
  if (tool === "slab") {
    const kind = SLAB_KINDS.find((k) => k.value === options.slabKind) ?? SLAB_KINDS[0];
    return `${kind.label}, ${options.slabRailing ? "railing on" : "no railing"}`;
  }
  if (tool === "asset") {
    return catalog.find((c) => c.key === options.assetKey)?.name ?? null;
  }
  if (tool === "stair") {
    return stair.shape === "straight" ? STAIR_LABEL.straight : `${STAIR_LABEL[stair.shape]}, turns ${stair.turn}`;
  }
  if (tool === "pipe") {
    // Material and size follow the system until picked (docs/CONTRACT.md, "Pipes").
    const d = PIPE_DEFAULTS[options.pipeSystem];
    return `${runLabel(options.pipeSystem)}, ${sizeLabel(options.pipeMaterial ?? d.material, options.pipeDiameterMm ?? d.diameterMm)}`;
  }
  return null;
}
