import { useMemo } from "react";
import type { DocState } from "../contract/bindings";
import { useApp, useVisibleDoc } from "../state/store";
import { cx } from "../ui/controls";
import { Icon, type IconName } from "../ui/icons";
import { formatArea, formatAreaMm2, formatLength, lengthToInput } from "../ui/units";
import { TOOLS } from "./actions";
import s from "./chrome.module.css";

const KIND_LABEL: Record<string, [string, string]> = {
  wall: ["wall", "walls"],
  opening: ["opening", "openings"],
  room: ["room", "rooms"],
  column: ["column", "columns"],
  stair: ["stair", "stairs"],
  asset: ["object", "objects"],
  annotation: ["text note", "text notes"],
  dimension: ["dimension", "dimensions"],
  camera: ["camera", "cameras"],
  underlay: ["underlay", "underlays"],
  linework: ["linework", "linework"],
  reference_model: ["reference model", "reference models"],
};

function selectionSummary(doc: DocState, selection: string[]): string {
  if (selection.length === 0) return "";
  const unit = doc.project.settings.display_unit;
  const picked = doc.project.elements.filter((e) => selection.includes(e.id));
  if (picked.length === 0) return "";
  const kinds = new Set(picked.map((e) => e.kind));
  const parts: string[] = [];

  if (picked.length === 1) {
    const e = picked[0];
    if (e.kind === "room") parts.push(e.name || "Room");
    else if (e.kind === "opening") parts.push(e.opening_type === "door" ? "Door" : "Window");
    else if (e.kind === "asset") parts.push(e.name);
    else parts.push(KIND_LABEL[e.kind][0].replace(/^./, (c) => c.toUpperCase()));
  } else if (kinds.size === 1) {
    parts.push(`${picked.length} ${KIND_LABEL[picked[0].kind][1]}`);
  } else {
    parts.push(`${picked.length} selected`);
  }

  const wallIds = picked.filter((e) => e.kind === "wall").map((e) => e.id);
  if (wallIds.length > 0) {
    const total = doc.derived.walls.filter((w) => wallIds.includes(w.wall_id)).reduce((sum, w) => sum + w.length_mm, 0);
    if (total > 0) parts.push(`${wallIds.length > 1 ? "total length" : "length"} ${formatLength(total, unit)}`);
  }
  const roomIds = picked.filter((e) => e.kind === "room").map((e) => e.id);
  if (roomIds.length > 0) {
    const rooms = doc.derived.rooms.filter((r) => roomIds.includes(r.room_id));
    const area = rooms.reduce((sum, r) => sum + r.area_mm2, 0);
    if (area > 0) parts.push(`${roomIds.length > 1 ? "total area" : "area"} ${formatAreaMm2(area)}`);
    if (rooms.length === 1) parts.push(`perimeter ${formatLength(rooms[0].perimeter_mm, unit)}`);
  }
  if (picked.length === 1 && picked[0].kind === "opening") {
    parts.push(`${formatLength(picked[0].width_mm, unit)} wide`);
  }
  return parts.join(", ");
}

function Toggle({ on, label, icon, tip, onClick }: { on: boolean; label: string; icon: IconName; tip: string; onClick: () => void }) {
  return (
    <button
      type="button"
      className={cx(s.statusToggle, on && s.statusToggleOn)}
      aria-pressed={on}
      data-tip={tip}
      data-tip-side="top"
      onClick={onClick}
    >
      <Icon name={icon} size={13} />
      {label}
    </button>
  );
}

export function StatusBar() {
  const doc = useVisibleDoc();
  const cursor = useApp((st) => st.cursor);
  const selection = useApp((st) => st.selection);
  const tool = useApp((st) => st.tool);
  const snap = useApp((st) => st.snapEnabled);
  const ortho = useApp((st) => st.orthoEnabled);
  const grid = useApp((st) => st.gridVisible);
  const toggle = useApp((st) => st.toggle);
  const activeLevelId = useApp((st) => st.activeLevelId);
  const preview = useApp((st) => st.preview !== null);

  const summary = useMemo(() => (doc ? selectionSummary(doc, selection) : ""), [doc, selection]);
  if (!doc) return null;

  const { settings, levels } = doc.project;
  const unit = settings.display_unit;
  const level = levels.find((l) => l.id === activeLevelId) ?? levels[0];
  const toolDef = TOOLS.find((t) => t.tool === tool);
  const fmt = (v: number) => (unit === "m" ? (v / 1000).toFixed(3) : lengthToInput(Math.round(v), unit));

  return (
    <footer className={s.statusbar}>
      <div className={s.coords} aria-label="Cursor position">
        <span>
          <i>X</i>
          {cursor ? fmt(cursor.x) : "-"}
        </span>
        <span>
          <i>Y</i>
          {cursor ? fmt(cursor.y) : "-"}
        </span>
        <em>{unit}</em>
      </div>
      <div className={s.statusGroup}>
        <Toggle on={snap} label="Snap" icon="snap" tip={snap ? "Snapping is on" : "Snapping is off"} onClick={() => toggle("snapEnabled")} />
        <Toggle on={ortho} label="Ortho" icon="ortho" tip={ortho ? "Lines lock to 90 degree steps" : "Lines are free"} onClick={() => toggle("orthoEnabled")} />
        <Toggle on={grid} label="Grid" icon="grid" tip={grid ? "Grid is shown" : "Grid is hidden"} onClick={() => toggle("gridVisible")} />
      </div>

      <div className={s.statusMain}>
        {preview ? (
          <span className={s.statusAi}>Copilot preview. Nothing changes until you accept it.</span>
        ) : summary ? (
          <span className={s.statusSelection}>{summary}</span>
        ) : toolDef && tool !== "select" ? (
          <span className={s.statusHint}>{toolDef.label} tool. Esc returns to Select.</span>
        ) : null}
      </div>

      <div className={s.statusRight}>
        {level ? <span data-tip="Active level" data-tip-side="top">{level.name}</span> : null}
        <span data-tip="Drawing scale" data-tip-side="top">1:{settings.scale_denominator}</span>
        <span data-tip={`${doc.derived.totals.room_count} rooms, gross ${formatArea(doc.derived.totals.gross_area_m2)}`} data-tip-side="top-end">
          Floor area <b>{formatArea(doc.derived.totals.floor_area_m2)}</b>
        </span>
      </div>
    </footer>
  );
}
