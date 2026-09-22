// State and pure helpers for the render studio (DECISIONS D17).
//
// The studio lives inside the Visuals dock panel and can also open full size
// as an overlay. Both read this store, and so do the two palette actions, so
// the shell never needs to know how the studio works.

import { create } from "zustand";
import type { RenderQuality, RenderRecord, RenderAiSettings } from "../../contract/bindings";

// ---------------------------------------------------------------- options

export interface BuildingType {
  value: string;
  label: string;
}

/** Appended to the prompt as "building type: ...". */
export const BUILDING_TYPES: BuildingType[] = [
  { value: "single-house", label: "Single house" },
  { value: "two-storey-house", label: "Two storey house" },
  { value: "townhouse", label: "Townhouse" },
  { value: "apartment", label: "Apartment" },
  { value: "cafe-shop", label: "Cafe or shop" },
  { value: "office", label: "Office" },
];

export const QUALITIES: Array<{ value: RenderQuality; label: string; note: string }> = [
  { value: "draft", label: "Draft", note: "Fast look test" },
  { value: "standard", label: "Standard", note: "Everyday client image" },
  { value: "high", label: "High", note: "Presentation quality" },
];

// ---------------------------------------------------------------- pure helpers

/** Keeps a divider position inside 0..100. */
export function clampPercent(n: number): number {
  if (!Number.isFinite(n)) return 50;
  return Math.min(100, Math.max(0, n));
}

/** Divider position from a pointer, 1:1 with the container. */
export function percentFromPointer(clientX: number, left: number, width: number): number {
  if (width <= 0) return 50;
  return clampPercent(((clientX - left) / width) * 100);
}

/**
 * Keyboard stepping for the compare divider: arrows move 2 percent, page keys
 * 10, Home and End snap. Returns null for keys the slider does not handle.
 */
export function percentFromKey(key: string, current: number): number | null {
  switch (key) {
    case "ArrowLeft":
    case "ArrowDown":
      return clampPercent(current - 2);
    case "ArrowRight":
    case "ArrowUp":
      return clampPercent(current + 2);
    case "PageDown":
      return clampPercent(current - 10);
    case "PageUp":
      return clampPercent(current + 10);
    case "Home":
      return 0;
    case "End":
      return 100;
    default:
      return null;
  }
}

/** A capture with the AI visualizations made from it. */
export interface RenderGroup {
  /** Null when the source capture was deleted but its visualization was kept. */
  source: RenderRecord | null;
  visualizations: RenderRecord[];
  /** Newest created_at in the group, used for ordering. */
  newest: string;
}

/**
 * Groups renders so a visualization always sits next to the capture it was
 * made from. Newest group first; inside a group the source comes first and its
 * visualizations follow, newest first.
 */
export function groupRenders(records: RenderRecord[]): RenderGroup[] {
  const byId = new Map<string, RenderRecord>();
  for (const r of records) byId.set(r.id, r);
  const groups = new Map<string, RenderGroup>();
  const order: string[] = [];

  const groupFor = (key: string, source: RenderRecord | null): RenderGroup => {
    let g = groups.get(key);
    if (!g) {
      g = { source, visualizations: [], newest: "" };
      groups.set(key, g);
      order.push(key);
    }
    if (source && !g.source) g.source = source;
    return g;
  };

  const sorted = [...records].sort((a, b) => b.created_at.localeCompare(a.created_at));
  for (const r of sorted) {
    const parentId = r.source === "ai_visualization" ? r.source_render_id : null;
    if (parentId && byId.has(parentId)) {
      const g = groupFor(parentId, byId.get(parentId) ?? null);
      g.visualizations.push(r);
      if (r.created_at > g.newest) g.newest = r.created_at;
    } else if (r.source === "ai_visualization") {
      const g = groupFor(`orphan:${r.id}`, null);
      g.visualizations.push(r);
      if (r.created_at > g.newest) g.newest = r.created_at;
    } else {
      const g = groupFor(r.id, r);
      if (r.created_at > g.newest) g.newest = r.created_at;
    }
  }

  return order
    .map((k) => groups.get(k) as RenderGroup)
    .sort((a, b) => b.newest.localeCompare(a.newest));
}

/** The gallery strip order: every group flattened, source first. */
export function flattenGroups(groups: RenderGroup[]): RenderRecord[] {
  const out: RenderRecord[] = [];
  for (const g of groups) {
    if (g.source) out.push(g.source);
    out.push(...g.visualizations);
  }
  return out;
}

/** The free text sent to the provider: building type first, then the extras. */
export function buildPrompt(buildingTypeValue: string, extras: string): string {
  const type = BUILDING_TYPES.find((b) => b.value === buildingTypeValue);
  const parts: string[] = [];
  if (type) parts.push(`building type: ${type.label.toLowerCase()}`);
  const trimmed = extras.trim();
  if (trimmed) parts.push(trimmed);
  return parts.join(". ");
}

const MONEY = "[$₱€£]\\s?\\d+(?:\\.\\d+)?";

/**
 * Plain cost wording per quality level, taken from the provider's own hint.
 * A hint that names the levels ("$0.05 per draft, $0.10 per standard, ...")
 * gives each level its own amount; a hint with a range ("$0.045 to $0.151 per
 * image") gives the low end to Draft, the range to Standard and the high end
 * to High. Anything else is passed through as it came: we do not invent prices.
 */
export function costForQuality(settings: RenderAiSettings | null, quality: RenderQuality): string {
  const hint = settings?.cost_hint?.trim();
  if (!hint) return "";

  const named = new RegExp(`(${MONEY})[^$₱€£]{0,24}?\\b(draft|standard|high)\\b|\\b(draft|standard|high)\\b[^$₱€£]{0,24}?(${MONEY})`, "gi");
  for (const m of hint.matchAll(named)) {
    const level = (m[2] ?? m[3] ?? "").toLowerCase();
    const amount = (m[1] ?? m[4] ?? "").replace(/\s/g, "");
    if (level === quality && amount) return `about ${amount} per image`;
  }

  const amounts = hint.match(new RegExp(MONEY, "g")) ?? [];
  const first = amounts[0];
  const last = amounts[amounts.length - 1];
  if (amounts.length < 2 || !first || !last) return hint;
  const low = first.replace(/\s/g, "");
  const high = last.replace(/\s/g, "");
  if (quality === "draft") return `about ${low} per image`;
  if (quality === "high") return `about ${high} per image`;
  return `${low} to ${high} per image`;
}

/** "1301 x 800 px", the wording the badge uses. Empty until the image loads. */
export function formatResolution(width: number, height: number): string {
  if (!width || !height) return "";
  return `${Math.round(width)} x ${Math.round(height)} px`;
}

/** "gemini/gemini-3.1-flash-image" reads as "Gemini 3.1 flash image" in the badge. */
export function providerLabel(provider: string | null): string {
  if (!provider) return "";
  const model = provider.includes("/") ? provider.slice(provider.indexOf("/") + 1) : provider;
  const words = model.replace(/[-_]/g, " ").trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** Elapsed seconds, shown while a generation runs. */
export function elapsedLabel(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${s}s`;
}

// ---------------------------------------------------------------- store

export interface RenderStudioConfig {
  styleKey: string | null;
  buildingType: string;
  extras: string;
  quality: RenderQuality;
  keepGeometry: boolean;
}

interface RenderUiState {
  /** The studio opened full size over the app. */
  fullOpen: boolean;
  /** The capture the studio conditions on. */
  sourceId: string | null;
  /** The record shown in the compare view, an AI visualization or a capture. */
  compareId: string | null;
  /** Compare, or the AI image on its own. */
  compareMode: "compare" | "result";
  /** Divider position in percent, remembered across views. */
  divider: number;
  /** Record opened in the full screen lightbox. */
  lightboxId: string | null;
  config: RenderStudioConfig;
  /** Token the Settings dialog watches so it can reveal the AI rendering section. */
  settingsFocus: number;

  setFullOpen: (open: boolean) => void;
  setSourceId: (id: string | null) => void;
  setCompareId: (id: string | null) => void;
  setCompareMode: (mode: "compare" | "result") => void;
  setDivider: (pct: number) => void;
  setLightboxId: (id: string | null) => void;
  patchConfig: (patch: Partial<RenderStudioConfig>) => void;
  focusSettings: () => void;
}

export const useRenderUi = create<RenderUiState>((set) => ({
  fullOpen: false,
  sourceId: null,
  compareId: null,
  compareMode: "compare",
  divider: 50,
  lightboxId: null,
  config: {
    styleKey: null,
    buildingType: BUILDING_TYPES[0].value,
    extras: "",
    quality: "standard",
    keepGeometry: true,
  },
  settingsFocus: 0,

  setFullOpen: (fullOpen) => set({ fullOpen }),
  setSourceId: (sourceId) => set({ sourceId }),
  setCompareId: (compareId) => set({ compareId }),
  setCompareMode: (compareMode) => set({ compareMode }),
  setDivider: (pct) => set({ divider: clampPercent(pct) }),
  setLightboxId: (lightboxId) => set({ lightboxId }),
  patchConfig: (patch) => set((s) => ({ config: { ...s.config, ...patch } })),
  focusSettings: () => set((s) => ({ settingsFocus: s.settingsFocus + 1 })),
}));

/** Palette action: open the studio full size, with the dock tab on Visuals. */
export function openRenderStudio() {
  useRenderUi.getState().setFullOpen(true);
}

/** Palette action: open the studio full size on the compare view. */
export function openRenderCompare() {
  const st = useRenderUi.getState();
  st.setCompareMode("compare");
  st.setFullOpen(true);
}
