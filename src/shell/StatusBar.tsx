import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { DocState, Element } from "../contract/bindings";
import { useApp, useVisibleDoc } from "../state/store";
import { cx } from "../ui/controls";
import { Icon, type IconName } from "../ui/icons";
import { formatArea, formatAreaMm2, formatLength, lengthToInput } from "../ui/units";
import { useViewer, type ShellMode } from "../viewer3d/viewerStore";
import { TOOLS, currentSunPresets, showProjectSection, type ToolDef } from "./actions";
import { pipeLength, runLabel, sizeLabel } from "./pipes";
import { useStairTool } from "../editor2d/stairTool";
import { rememberedStyle } from "./railFlyout";
import { useShell } from "./shellStore";
import { formatClock, presetAtTime } from "./sun";
import s from "./chrome.module.css";

const KIND_LABEL: Record<Element["kind"], [string, string]> = {
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
  pipe: ["pipe", "pipes"],
  slab: ["slab", "slabs"],
  roof: ["roof", "roofs"],
  grid: ["grid line", "grid lines"],
  view_mark: ["view mark", "view marks"],
  lot: ["lot", "lots"],
  tour: ["tour", "tours"],
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
    else if (e.kind === "pipe") parts.push(`${runLabel(e.system)}, ${sizeLabel(e.material, e.diameter_mm)}`);
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
  const pipes = picked.filter((e) => e.kind === "pipe");
  if (pipes.length > 0) {
    const total = pipes.reduce((sum, e) => sum + pipeLength(e.points), 0);
    parts.push(`${pipes.length > 1 ? "total length" : "length"} ${formatLength(total, unit)}`);
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

const SHELL_LABEL: Record<ShellMode, string> = { solid: "Solid", xray: "X-ray", hidden: "Building hidden" };

/**
 * Shows the 3D building shell mode while it is not solid, so X pressed in
 * the plan view has a visible answer. Click to cycle, like X.
 */
/**
 * A status chip that grows in from the right of its group and folds away,
 * by its grid column (docs/MOTION.md rule 5: never max-width or margin).
 */
function Fold({ shown, quick, children }: { shown: boolean; quick?: boolean; children: ReactNode }) {
  return (
    <span className={cx(s.fold, quick && s.foldQuick, shown && s.foldOn)} aria-hidden={!shown}>
      <span className={s.foldClip}>{children}</span>
    </span>
  );
}

function ShellStatus() {
  const shell = useViewer((st) => st.shell);
  const cycle = useViewer((st) => st.cycleShell);
  const shown = shell !== "solid";
  return (
    <Fold shown={shown}>
      <button
        type="button"
        className={cx(s.statusToggle, s.statusToggleOn, s.foldItem)}
        tabIndex={shown ? 0 : -1}
        data-tip="3D building: X switches solid, X-ray, hidden"
        data-tip-side="top"
        onClick={cycle}
      >
        <Icon name={shell === "hidden" ? "eyeOff" : "xray"} size={13} />
        {SHELL_LABEL[shell]}
      </button>
    </Fold>
  );
}

/**
 * "Only the selection" is on and something is selected (DECISIONS D30): AI
 * edits, the copilot's and MCP clients', may change only the selection. Shown
 * here too, because an MCP client works while the copilot dock is closed.
 * Click to lift the limit.
 */
function AiScopeStatus() {
  const count = useApp((st) => (st.aiScope ? st.selection.length : 0));
  const setAiScope = useApp((st) => st.setAiScope);
  const shown = count > 0;
  return (
    <Fold shown={shown}>
      <button
        type="button"
        className={cx(s.statusToggle, s.statusToggleOn, s.foldItem)}
        tabIndex={shown ? 0 : -1}
        data-tip="AI edits may change only the selection. Click to lift the limit"
        data-tip-side="top"
        onClick={() => setAiScope(false)}
      >
        <Icon name="lock" size={13} />
        AI: selection only
      </button>
    </Fold>
  );
}

/** How long the sun readout stays after the light changes. */
const SUN_STATUS_MS = 2400;

/**
 * The live sun time, for a moment after it changes (U, I, the presets,
 * Shift+N), so the keys answer even while only the plan is on screen.
 */
function SunStatus() {
  const light = useViewer((st) => st.light);
  const [shown, setShown] = useState(false);
  // Only a change shows it: not the first render, nor StrictMode's second run.
  const seen = useRef(`${light.minutes}/${light.lamps}`);
  useEffect(() => {
    const now = `${light.minutes}/${light.lamps}`;
    if (now === seen.current) return;
    seen.current = now;
    setShown(true);
    const t = window.setTimeout(() => setShown(false), SUN_STATUS_MS);
    return () => window.clearTimeout(t);
  }, [light.minutes, light.lamps]);
  const preset = presetAtTime(currentSunPresets(), light.minutes);
  const dark = light.minutes < 6 * 60 || light.minutes >= 18 * 60;
  return (
    <Fold shown={shown} quick>
      <span className={cx(s.statusToggle, s.statusToggleOn, s.sunStatus, s.foldItem)} aria-live="polite">
        <Icon name={dark ? "moon" : "sun"} size={13} />
        {preset ? `${preset.label}, ` : "Sun "}
        {formatClock(light.minutes)}
        {light.lamps === "on" ? ", lamps on" : ""}
      </span>
    </Fold>
  );
}

/**
 * The tool hint: the tool's remembered style as a button that opens its
 * options (railFlyout.ts), so thickness, type or style is never hidden.
 */
function ToolHint({ def }: { def: ToolDef }) {
  const options = useApp((st) => st.toolOptions);
  const settings = useApp((st) => st.doc?.project.settings);
  const catalog = useApp((st) => st.catalog);
  const shape = useStairTool((st) => st.shape);
  const turn = useStairTool((st) => st.turn);
  const style = def.flyout ? rememberedStyle(def.tool, options, settings, catalog, { shape, turn }) : null;
  if (!style || !def.flyout) return <span className={s.statusHint}>{def.label} tool. Esc returns to Select.</span>;
  const flyout = def.flyout;
  return (
    <span className={s.statusHint}>
      {def.label}:{" "}
      <button
        type="button"
        className={s.statusStyle}
        aria-haspopup="dialog"
        data-tip={`${def.label} options`}
        data-tip-side="top"
        onClick={() => useShell.getState().requestFlyout(flyout)}
      >
        {style}
        <Icon name="chevronRight" size={11} />
      </button>{" "}
      Esc returns to Select.
    </span>
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
          <span className={s.statusAi}>Copilot preview. Nothing changes until you apply it.</span>
        ) : summary ? (
          <span className={s.statusSelection}>{summary}</span>
        ) : toolDef && tool !== "select" ? (
          <ToolHint def={toolDef} />
        ) : null}
      </div>

      <div className={s.statusRight}>
        <AiScopeStatus />
        <SunStatus />
        <ShellStatus />
        {level ? (
          <button
            key={level.id}
            type="button"
            className={s.levelStatus}
            data-tip={levels.length > 1 ? `Working on ${level.name}. Click for the levels` : "Click to add a level"}
            data-tip-side="top"
            onClick={() => showProjectSection("level")}
          >
            <Icon name="level" size={12} />
            {level.name}
          </button>
        ) : null}
        <span data-tip="Drawing scale" data-tip-side="top">1:{settings.scale_denominator}</span>
        <span data-tip={`${doc.derived.totals.room_count} rooms, gross ${formatArea(doc.derived.totals.gross_area_m2)}`} data-tip-side="top-end">
          Floor area <b>{formatArea(doc.derived.totals.floor_area_m2)}</b>
        </span>
      </div>
    </footer>
  );
}
