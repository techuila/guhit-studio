// Things the shell can do, shared by the top bar, shortcuts and the palette.
import type { DisplayUnit, OpeningStyle, RoofKind, Vec3 } from "../contract/bindings";
import { ipc } from "../contract/ipc";
import { getActiveController } from "../editor2d/controller";
import { rectIsEmpty } from "../editor2d/geom";
import { boundsOfIds, buildIndex, layerOf, levelOf } from "../editor2d/model";
import { bus } from "../state/bus";
import { useApp, type Tool } from "../state/store";
import { openRenderCompare, openRenderStudio } from "../viewer3d/render/renderStore";
import { useViewer, type NavMode, type ShellMode } from "../viewer3d/viewerStore";
import type { IconName } from "../ui/icons";
import { ADD_LEVEL_ABOVE, addedLevel, deleteLevelCommand } from "./levels";
import { siteOf } from "./site";
import { useShell, type FlyoutKind } from "./shellStore";
import { formatClock, stepMinutes, stepPreset, sunPresets, toggleLamps, type SunPreset, type SunPresetId } from "./sun";
import { checkForUpdates } from "./UpdateNotice";

// ---------------------------------------------------------------- tools

export interface ToolDef {
  tool: Tool;
  label: string;
  /** Natural wording for the palette. */
  phrase: string;
  icon: IconName;
  key: string | null;
  keywords: string;
  /** Tools of one group sit together on the rail, with a rule between groups. */
  group: "pick" | "draw" | "place" | "services" | "annotate" | "view";
  flyout?: FlyoutKind;
}

export const TOOLS: ToolDef[] = [
  { tool: "select", label: "Select", phrase: "Select and move things", icon: "select", key: "V", group: "pick", keywords: "pointer arrow pick move" },
  { tool: "wall", label: "Wall", phrase: "Draw walls", icon: "wall", key: "W", group: "draw", keywords: "line partition chb", flyout: "wall" },
  { tool: "rect_room", label: "Room", phrase: "Draw a room rectangle", icon: "room", key: "R", group: "draw", keywords: "rectangle box space" },
  { tool: "door", label: "Door", phrase: "Place a door", icon: "door", key: "D", group: "place", keywords: "opening swing sliding", flyout: "door" },
  { tool: "window", label: "Window", phrase: "Place a window", icon: "window", key: "N", group: "place", keywords: "opening jalousie casement glass", flyout: "window" },
  { tool: "column", label: "Column", phrase: "Place a column", icon: "column", key: "C", group: "place", keywords: "post pillar structure" },
  { tool: "stair", label: "Stair", phrase: "Add a stair", icon: "stair", key: "S", group: "place", keywords: "steps flight" },
  { tool: "asset", label: "Objects", phrase: "Place furniture and fixtures", icon: "asset", key: "O", group: "place", keywords: "library furniture sofa bed toilet sink kitchen car plant", flyout: "asset" },
  {
    tool: "pipe",
    label: "Services",
    phrase: "Draw pipes, conduit and aircon lines",
    icon: "pipe",
    key: "P",
    group: "services",
    keywords: "pipe plumbing water supply cold hot drain drainage waste sewer vent storm electrical conduit emt imc aircon refrigerant line set condensate ppr upvc gi pe copper pvc",
    flyout: "pipe",
  },
  { tool: "link", label: "Link", phrase: "Link a switch to its lights", icon: "link", key: "L", group: "services", keywords: "connect switch light lights outlet spo aircon unit 3-way three way controls feeds" },
  { tool: "dimension", label: "Dimension", phrase: "Add a dimension", icon: "dimension", key: "M", group: "annotate", keywords: "measure length distance" },
  { tool: "text", label: "Text", phrase: "Add a text note", icon: "text", key: "T", group: "annotate", keywords: "label annotation note" },
  { tool: "camera", label: "Camera", phrase: "Place a camera", icon: "camera", key: "K", group: "annotate", keywords: "view perspective render shot" },
  { tool: "pan", label: "Pan", phrase: "Pan the view", icon: "pan", key: "H", group: "view", keywords: "hand move scroll navigate" },
];

export const DOOR_STYLES: Array<{ value: OpeningStyle; label: string }> = [
  { value: "swing_single", label: "Single swing" },
  { value: "swing_double", label: "Double swing" },
  { value: "sliding", label: "Sliding" },
];

export const WINDOW_STYLES: Array<{ value: OpeningStyle; label: string }> = [
  { value: "sliding", label: "Sliding" },
  { value: "casement", label: "Casement" },
  { value: "jalousie", label: "Jalousie" },
  { value: "fixed", label: "Fixed glass" },
];

export const ROOF_KINDS: Array<{ value: RoofKind; label: string }> = [
  { value: "none", label: "None" },
  { value: "flat", label: "Flat" },
  { value: "shed", label: "Shed" },
  { value: "gable", label: "Gable" },
];

/** Activates a tool, keeping a sensible default option for openings. */
export function activateTool(tool: Tool) {
  const app = useApp.getState();
  if (tool === "door") {
    const current = app.toolOptions.openingStyle;
    const ok = DOOR_STYLES.some((d) => d.value === current);
    app.setTool("door", { openingStyle: ok ? current : "swing_single" });
  } else if (tool === "window") {
    const current = app.toolOptions.openingStyle;
    const ok = WINDOW_STYLES.some((d) => d.value === current);
    app.setTool("window", { openingStyle: ok ? current : "sliding" });
  } else {
    app.setTool(tool);
  }
}

/**
 * The O shortcut: the object tool needs a chosen catalog item before it can
 * place anything, so it opens the tool rail's object library flyout the
 * same way clicking the rail button does.
 */
export function openAssetTool() {
  const app = useApp.getState();
  if (app.toolOptions.assetKey) activateTool("asset");
  useShell.getState().requestFlyout("asset");
}

// ---------------------------------------------------------------- 3D navigation and building shell

/** Shows the 3D view: the plan-only view becomes split. */
function show3dView() {
  const app = useApp.getState();
  if (app.viewMode === "2d") app.setViewMode("split");
}

/** Resolves once the 3D view has mounted and registered itself (it loads on demand). */
async function waitFor3dView(timeoutMs: number): Promise<boolean> {
  const start = Date.now();
  while (!useApp.getState().captureView) {
    if (Date.now() - start > timeoutMs) return false;
    await new Promise((r) => window.setTimeout(r, 60));
  }
  return true;
}

/**
 * Shows the 3D view and resolves true once it is up. False, with a toast, when
 * it does not come up: walking without it would leave no one owning the keys.
 */
async function ensure3dView(): Promise<boolean> {
  show3dView();
  if (await waitFor3dView(8000)) return true;
  useApp.getState().toast("error", "The 3D view is not ready yet. Open it and try again.");
  return false;
}

/** Walk (eye height, walls block) or fly (free) through the building in the 3D view. */
export async function enterNav(nav: Exclude<NavMode, "orbit">) {
  if (await ensure3dView()) useViewer.getState().setNav(nav);
}

/** Opens the walk settings (eye height and speed), walking first if needed. */
export async function openWalkSettings() {
  if (useViewer.getState().nav === "orbit") await enterNav("walk");
  const viewer = useViewer.getState();
  if (viewer.nav !== "orbit") viewer.setWalkSettingsOpen(true);
}

export function setShellMode(shell: ShellMode) {
  useViewer.getState().setShell(shell);
}

/**
 * Enters walk mode near these elements, looking at `location` when given
 * (the `Issue::location` convention). Sent once the 3D view is up.
 */
export async function walkTo(ids: string[], location: Vec3 | null) {
  if (await ensure3dView()) bus.emit("walk_to", { ids, location });
}

/** The project inspector with its Plumbing section open and scrolled into view. */
export function showPipeTakeoff() {
  useApp.getState().select([]);
  useShell.getState().revealSection("plumbing");
}

/** The project inspector with one of its sections open and scrolled into view. */
export function showProjectSection(key: "schedules" | "site" | "review" | "level") {
  useApp.getState().select([]);
  useShell.getState().revealSection(key);
}

/**
 * The link tool, starting from this device: it stays selected, so the tool
 * links what is clicked next to it (docs/CONTRACT.md, "Devices, fixtures and links").
 */
export function linkFrom(deviceId: string | null) {
  const app = useApp.getState();
  if (deviceId) app.select([deviceId]);
  activateTool("link");
}

// ---------------------------------------------------------------- levels

/** Stacks a new level on the highest one (AddLevel with nulls) and works on it. */
export async function addLevelAbove() {
  const app = useApp.getState();
  const before = app.doc?.project.levels ?? [];
  const result = await app.dispatch(ADD_LEVEL_ABOVE);
  if (!result) return;
  const added = addedLevel(before, result.state.project.levels);
  if (added) useApp.getState().setActiveLevel(added.id);
}

/** Deletes a level with everything on it, one undo step. The inspector confirms first. */
export async function deleteLevel(levelId: string) {
  await useApp.getState().dispatch(deleteLevelCommand(levelId));
}

/** The palette's "Delete this level": the Levels section asks, inline. */
export function confirmDeleteActiveLevel() {
  const { doc, activeLevelId } = useApp.getState();
  const id = activeLevelId ?? doc?.project.levels[0]?.id;
  if (!id) return;
  showProjectSection("level");
  useShell.getState().requestLevelDelete(id);
}

// ---------------------------------------------------------------- sun, light and render

/** The five sun presets for the project's site on the live light's date. */
export function currentSunPresets(): SunPreset[] {
  const site = siteOf(useApp.getState().doc?.project.settings);
  const { month, day } = useViewer.getState().light;
  return sunPresets(site, new Date().getFullYear(), month, day);
}

/** A preset sets the time and the lamps. The date stays. */
export function applySunPreset(id: SunPresetId) {
  const preset = currentSunPresets().find((p) => p.id === id);
  if (preset) useViewer.getState().setLight({ minutes: preset.minutes, lamps: preset.lamps });
}

/** Shift+I and Shift+U: the next or previous preset, wrapping around the day. */
export function stepSunPreset(dir: 1 | -1) {
  const preset = stepPreset(currentSunPresets(), useViewer.getState().light.minutes, dir);
  useViewer.getState().setLight({ minutes: preset.minutes, lamps: preset.lamps });
}

/** I and U: the sun 15 minutes later or earlier. A held key repeats, so it scrubs. */
export function stepSunTime(dir: 1 | -1) {
  const viewer = useViewer.getState();
  const minutes = stepMinutes(viewer.light.minutes, dir);
  if (minutes !== viewer.light.minutes) viewer.setLight({ minutes });
}

/** Shift+N: lamps on, or back to auto (on after sunset). */
export function toggleLampsNow() {
  const viewer = useViewer.getState();
  viewer.setLight({ lamps: toggleLamps(viewer.light.lamps) });
}

/** Renders with the path tracer once the 3D view is up; the results land in Visuals. */
export async function renderViews(views: "current" | "all") {
  if (await ensure3dView()) bus.emit("render", { views });
}

/** The shadow study takes its frames from the live 3D view. */
export async function openShadowStudy() {
  if (await ensure3dView()) bus.emit("shadow_study");
}

// ---------------------------------------------------------------- document actions

export function deleteSelection() {
  const { selection, dispatch } = useApp.getState();
  if (selection.length === 0) return;
  void dispatch({ type: "delete_elements", ids: selection });
}

export function escapeToSelect() {
  const app = useApp.getState();
  app.setTool("select");
  app.select([]);
}

/** Selects everything on the active level, skipping hidden or locked layers. */
export function selectAllOnLevel() {
  const { doc, activeLevelId, select } = useApp.getState();
  if (!doc) return;
  const byId = new Map(doc.project.elements.map((e) => [e.id, e]));
  const blocked = new Set(doc.project.layers.filter((l) => !l.visible || l.locked).map((l) => l.key));
  const ids = doc.project.elements
    .filter((e) => e.kind !== "camera" && e.kind !== "underlay")
    .filter((e) => levelOf(e, byId) === activeLevelId)
    .filter((e) => !blocked.has(layerOf(e)))
    .map((e) => e.id);
  select(ids);
}

/** Duplicates the selection offset by one grid step. The copies become the new selection. */
export function duplicateSelection() {
  const { doc, selection, dispatch, select } = useApp.getState();
  if (!doc || selection.length === 0) return;
  const grid = doc.project.settings.grid_mm;
  void dispatch({ type: "duplicate_elements", ids: selection, delta: { x: grid, y: grid } }).then((result) => {
    if (result && result.diff.added.length > 0) select(result.diff.added);
  });
}

/** Rotates the selection 90 degrees counter-clockwise about its own center. Openings turn with their wall. */
export function rotateSelectionCCW() {
  const { doc, selection, activeLevelId, dispatch } = useApp.getState();
  if (!doc || selection.length === 0) return;
  const index = buildIndex(doc, activeLevelId);
  const bounds = boundsOfIds(index, selection);
  if (rectIsEmpty(bounds)) return;
  const pivot = { x: (bounds.minX + bounds.maxX) / 2, y: (bounds.minY + bounds.maxY) / 2 };
  void dispatch({ type: "rotate_elements", ids: selection, pivot, angle_deg: 90 });
}

/** Saves a quick, auto-labeled version. Used by the MOD+S shortcut. */
export async function quickSaveVersion() {
  const { doc, toast, reportError } = useApp.getState();
  if (!doc) return;
  try {
    const list = await ipc.snapshotList();
    const meta = await ipc.snapshotCreate(`Version ${list.length + 1}`);
    toast("success", `Saved version "${meta.label}"`);
  } catch (e) {
    reportError(e);
  }
}

/** Zooms to fit the selection, or the whole plan when nothing is selected. */
export function zoomToSelection() {
  const { selection } = useApp.getState();
  if (selection.length > 0) bus.emit("focus_elements", selection);
  else bus.emit("zoom_to_fit");
}

export function setRoofKind(kind: RoofKind) {
  const { doc, dispatch } = useApp.getState();
  if (!doc) return;
  void dispatch({ type: "set_roof", roof: { ...doc.project.roof, kind } });
}

export function setDisplayUnit(unit: DisplayUnit) {
  const { doc, dispatch } = useApp.getState();
  if (!doc) return;
  void dispatch({ type: "set_project_settings", settings: { ...doc.project.settings, display_unit: unit } });
}

// ---------------------------------------------------------------- thumbnails

function downscale(png: string, maxWidth: number): Promise<string> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => {
      const scale = Math.min(1, maxWidth / Math.max(1, img.naturalWidth));
      if (scale === 1) return resolve(png);
      const canvas = document.createElement("canvas");
      canvas.width = Math.round(img.naturalWidth * scale);
      canvas.height = Math.round(img.naturalHeight * scale);
      const ctx = canvas.getContext("2d");
      if (!ctx) return resolve(png);
      ctx.imageSmoothingQuality = "high";
      ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
      resolve(canvas.toDataURL("image/png"));
    };
    img.onerror = () => resolve(png);
    img.src = png;
  });
}

let lastThumb: { projectId: string; revision: number; at: number } | null = null;

/** Saves a hub thumbnail from the plan (or the 3D view when no plan is mounted). Quiet on failure. */
export async function saveThumbnail(opts: { minIntervalMs?: number } = {}): Promise<boolean> {
  const { doc, capturePlan, captureView } = useApp.getState();
  if (!doc) return false;
  const id = doc.project.id;
  if (lastThumb && lastThumb.projectId === id) {
    if (lastThumb.revision === doc.revision) return false;
    if (opts.minIntervalMs && Date.now() - lastThumb.at < opts.minIntervalMs) return false;
  }
  try {
    let png: string | null = null;
    if (capturePlan) png = await capturePlan();
    else if (captureView) png = (await captureView()).png;
    if (!png || !png.startsWith("data:image/")) return false;
    await ipc.hubSetThumbnail(id, await downscale(png, 640));
    lastThumb = { projectId: id, revision: doc.revision, at: Date.now() };
    return true;
  } catch (e) {
    console.warn("thumbnail not saved", e);
    return false;
  }
}

export async function leaveEditor() {
  await saveThumbnail();
  useShell.getState().close();
  await useApp.getState().closeProject();
}

// ---------------------------------------------------------------- palette

export interface PaletteAction {
  id: string;
  title: string;
  group: "Draw" | "View" | "Sun and render" | "Edit" | "Project" | "Roof" | "Panels";
  icon: IconName;
  keywords?: string;
  shortcut?: string;
  disabled?: boolean;
  run: () => void;
}

export const MOD = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform) ? "⌘" : "Ctrl+";
export const SHIFT = MOD === "⌘" ? "⇧" : "Shift+";
export const ALT = MOD === "⌘" ? "⌥" : "Alt+";

/** The step `useViewer.cycleShell` (the X key) takes from each shell mode. */
const SHELL_NEXT: Record<ShellMode, ShellMode> = { solid: "xray", xray: "hidden", hidden: "solid" };

export function paletteActions(): PaletteAction[] {
  const app = useApp.getState();
  const shell = useShell.getState();
  const doc = app.doc;
  const out: PaletteAction[] = [];

  for (const t of TOOLS) {
    out.push({
      id: `tool-${t.tool}`,
      title: t.phrase,
      group: "Draw",
      icon: t.icon,
      keywords: `${t.label} tool ${t.keywords}`,
      shortcut: t.key ?? undefined,
      run: () => activateTool(t.tool),
    });
  }

  out.push(
    { id: "view-2d", title: "Show the plan only", group: "View", icon: "view2d", keywords: "2d view mode drawing", shortcut: "1", run: () => app.setViewMode("2d") },
    { id: "view-split", title: "Show plan and 3D side by side", group: "View", icon: "split", keywords: "split view mode both", shortcut: "2", run: () => app.setViewMode("split") },
    { id: "view-3d", title: "Show the 3D model only", group: "View", icon: "view3d", keywords: "3d view mode perspective", shortcut: "3", run: () => app.setViewMode("3d") },
    { id: "zoom-fit", title: "Zoom to fit the whole plan", group: "View", icon: "fit", keywords: "zoom extents all center", shortcut: "F", run: () => bus.emit("zoom_to_fit") },
    { id: "zoom-selection", title: app.selection.length > 0 ? "Zoom to the selection" : "Zoom to fit the whole plan", group: "View", icon: "fit", keywords: "zoom focus selected", shortcut: "Z", run: zoomToSelection },
    { id: "zoom-in", title: "Zoom in", group: "View", icon: "plus", keywords: "zoom magnify", shortcut: `${MOD}=`, run: () => getActiveController()?.zoomStep(1) },
    { id: "zoom-out", title: "Zoom out", group: "View", icon: "plus", keywords: "zoom shrink", shortcut: `${MOD}-`, run: () => getActiveController()?.zoomStep(-1) },
    { id: "toggle-snap", title: app.snapEnabled ? "Turn snapping off" : "Turn snapping on", group: "View", icon: "snap", keywords: "snap toggle endpoints", shortcut: `${SHIFT}S`, run: () => app.toggle("snapEnabled") },
    { id: "toggle-ortho", title: app.orthoEnabled ? "Turn ortho off" : "Turn ortho on", group: "View", icon: "ortho", keywords: "ortho toggle straight 90 degrees angle lock", shortcut: `${SHIFT}O`, run: () => app.toggle("orthoEnabled") },
    { id: "toggle-grid", title: app.gridVisible ? "Hide the grid" : "Show the grid", group: "View", icon: "grid", keywords: "grid toggle", shortcut: "G", run: () => app.toggle("gridVisible") },
  );

  const viewer = useViewer.getState();
  // X steps solid, X-ray, hidden: the action it would pick next shows the key.
  const nextShell = SHELL_NEXT[viewer.shell];
  out.push(
    viewer.nav === "orbit"
      ? { id: "nav-walk", title: "Walk through the building", group: "View", icon: "walk", keywords: "walk mode first person eye level inside tour walkthrough 3d", shortcut: `${SHIFT}W`, run: () => void enterNav("walk") }
      : { id: "nav-orbit", title: "Stop walking and orbit again", group: "View", icon: "view3d", keywords: "orbit walk fly exit leave stop 3d", shortcut: "Esc", run: () => viewer.setNav("orbit") },
    { id: "nav-fly", title: "Fly through the building", group: "View", icon: "fly", keywords: "fly mode free camera tour 3d", disabled: viewer.nav === "fly", run: () => void enterNav("fly") },
    { id: "walk-settings", title: "Walk settings: eye height and speed", group: "View", icon: "walk", keywords: "walk settings eye height speed slow fast person child wheelchair 3d", run: () => void openWalkSettings() },
    { id: "shell-xray", title: "X-ray the building", group: "View", icon: "xray", keywords: "xray see through transparent ghost walls pipes plumbing 3d", shortcut: nextShell === "xray" ? "X" : undefined, disabled: viewer.shell === "xray", run: () => setShellMode("xray") },
    { id: "shell-hidden", title: "Hide the building", group: "View", icon: "eyeOff", keywords: "hide shell walls roof pipes only plumbing 3d", shortcut: nextShell === "hidden" ? "X" : undefined, disabled: viewer.shell === "hidden", run: () => setShellMode("hidden") },
  );
  if (viewer.shell !== "solid") {
    out.push({ id: "shell-solid", title: "Show the building solid", group: "View", icon: "view3d", keywords: "solid shell walls normal xray 3d", shortcut: nextShell === "solid" ? "X" : undefined, run: () => setShellMode("solid") });
  }

  // Sun, lamps and the render. Presets follow the site and the live date.
  const hasViews = doc?.project.elements.some((e) => e.kind === "camera") ?? false;
  out.push(
    { id: "render-current", title: "Render this view", group: "Sun and render", icon: "render", keywords: "render path tracer photo realistic image picture quick final hd 4k", shortcut: `${MOD}${ALT}R`, run: () => void renderViews("current") },
    {
      id: "render-all",
      title: "Render all saved views",
      group: "Sun and render",
      icon: "render",
      keywords: "render batch every saved view camera cameras all",
      disabled: !hasViews,
      run: () => void renderViews("all"),
    },
    { id: "shadow-study", title: "Shadow study", group: "Sun and render", icon: "clock", keywords: "sun shadow study frames hours dates contact sheet solar", run: () => void openShadowStudy() },
  );
  const light = viewer.light;
  for (const p of currentSunPresets()) {
    out.push({
      id: `sun-${p.id}`,
      title: `Sun: ${p.label}, ${formatClock(p.minutes)}${p.lamps === "on" ? ", lamps on" : ""}`,
      group: "Sun and render",
      icon: p.id === "dusk" || p.id === "night" ? "moon" : "sun",
      keywords: `sun time of day preset light ${p.id} ${p.id === "dusk" ? "sunset evening" : ""}`,
      disabled: Math.abs(light.minutes - p.minutes) < 0.5 && light.lamps === p.lamps,
      run: () => applySunPreset(p.id),
    });
  }
  out.push(
    { id: "sun-later", title: "Move the sun 15 minutes later", group: "Sun and render", icon: "sun", keywords: "sun time later forward scrub", shortcut: "I", disabled: light.minutes >= 1439, run: () => stepSunTime(1) },
    { id: "sun-earlier", title: "Move the sun 15 minutes earlier", group: "Sun and render", icon: "sun", keywords: "sun time earlier back scrub", shortcut: "U", disabled: light.minutes <= 0, run: () => stepSunTime(-1) },
    {
      id: "lamps",
      title: light.lamps === "on" ? "Lamps back to auto, on after sunset" : "Turn the lamps on",
      group: "Sun and render",
      icon: "bulb",
      keywords: "lamps lights fixtures night on auto",
      shortcut: `${SHIFT}N`,
      run: toggleLampsNow,
    },
    {
      id: "sun-path",
      title: viewer.sunPath ? "Hide the sun path" : "Show the sun path",
      group: "Sun and render",
      icon: "sunPath",
      keywords: "sun path arc solstice june december compass overlay",
      run: () => useViewer.getState().toggleSunPath(),
    },
    {
      id: "refine",
      title: viewer.refine ? "Stop refining the 3D view when it rests" : "Refine the 3D view when it rests",
      group: "Sun and render",
      icon: "refine",
      keywords: "refine quality soft shadows clean edges anti alias still",
      run: () => useViewer.getState().setRefine(!useViewer.getState().refine),
    },
    { id: "site", title: "Set the site for the sun", group: "Sun and render", icon: "pin", keywords: "site city location latitude longitude manila cebu davao baguio north utc", run: () => showProjectSection("site") },
  );

  if (doc) {
    const unit = doc.project.settings.display_unit;
    out.push({
      id: "unit",
      title: unit === "mm" ? "Show lengths in meters" : "Show lengths in millimeters",
      group: "View",
      icon: "dimension",
      keywords: "units mm m display",
      run: () => setDisplayUnit(unit === "mm" ? "m" : "mm"),
    });
  }

  out.push(
    { id: "undo", title: doc?.undo_label ? `Undo ${doc.undo_label}` : "Undo", group: "Edit", icon: "undo", shortcut: `${MOD}Z`, disabled: !doc?.can_undo, run: () => void app.undo() },
    { id: "redo", title: doc?.redo_label ? `Redo ${doc.redo_label}` : "Redo", group: "Edit", icon: "redo", shortcut: `${SHIFT}${MOD}Z`, disabled: !doc?.can_redo, run: () => void app.redo() },
    { id: "delete", title: "Delete the selection", group: "Edit", icon: "trash", keywords: "remove erase", shortcut: "Del", disabled: app.selection.length === 0, run: deleteSelection },
    { id: "select-all", title: "Select everything on this level", group: "Edit", icon: "select", keywords: "all", shortcut: `${MOD}A`, run: selectAllOnLevel },
    { id: "duplicate", title: "Duplicate the selection", group: "Edit", icon: "copy", keywords: "copy repeat", shortcut: `${MOD}D`, disabled: app.selection.length === 0, run: duplicateSelection },
    { id: "rotate-ccw", title: "Rotate the selection 90 degrees", group: "Edit", icon: "rotate", keywords: "turn spin counter clockwise", shortcut: `${SHIFT}R`, disabled: app.selection.length === 0, run: rotateSelectionCCW },
    { id: "deselect", title: "Clear the selection", group: "Edit", icon: "close", keywords: "deselect none", shortcut: "Esc", disabled: app.selection.length === 0, run: () => app.select([]) },
  );

  for (const r of ROOF_KINDS) {
    out.push({
      id: `roof-${r.value}`,
      title: r.value === "none" ? "Remove the roof" : `Make the roof ${r.label.toLowerCase()}`,
      group: "Roof",
      icon: "roof",
      keywords: `roof preset ${r.label}`,
      disabled: doc?.project.roof.kind === r.value,
      run: () => setRoofKind(r.value),
    });
  }

  out.push(
    { id: "export", title: "Export a drawing or image", group: "Project", icon: "export", keywords: "pdf svg dxf png print save file", shortcut: `${MOD}E`, run: () => shell.open("export") },
    { id: "export-pdf", title: "Export to PDF", group: "Project", icon: "export", keywords: "pdf print save quick", shortcut: `${SHIFT}${MOD}S`, run: () => shell.open("export") },
    { id: "import-cad", title: "Import DXF or DWG", group: "Project", icon: "import", keywords: "cad autocad drawing recognize walls linework", run: () => shell.requestImport("cad") },
    { id: "import-model", title: "Import 3D model", group: "Project", icon: "import", keywords: "gltf glb obj sketchup reference site massing", run: () => shell.requestImport("model") },
    { id: "import-bundle", title: "Open .guhit bundle", group: "Project", icon: "import", keywords: "guhit project open file", run: () => shell.requestImport("bundle") },
    { id: "version-new", title: "Save a new version", group: "Project", icon: "versions", keywords: "snapshot checkpoint history quick", shortcut: `${MOD}S`, run: () => void quickSaveVersion() },
    { id: "versions", title: "Browse and restore versions", group: "Project", icon: "versions", keywords: "snapshot history restore", run: () => shell.open("versions") },
    {
      id: "schedules",
      title: "Show the device schedules",
      group: "Project",
      icon: "schedule",
      keywords: "schedule schedules count outlets receptacles switches lights fixtures lumens aircon hp csv inspection form electrical",
      disabled: !doc?.project.elements.some((e) => e.kind === "asset" && (e.light !== null || e.links.length > 0 || e.category === "electrical" || e.category === "aircon")),
      run: () => showProjectSection("schedules"),
    },
    { id: "review", title: "Show the review list", group: "Project", icon: "check", keywords: "review items checks suggestions set aside resolved triage", run: () => showProjectSection("review") },
    { id: "level-add", title: "Add level above", group: "Project", icon: "level", keywords: "level storey floor second upper add new stack", run: () => void addLevelAbove() },
    {
      id: "level-delete",
      title: "Delete this level",
      group: "Project",
      icon: "trash",
      keywords: "level storey floor remove delete",
      disabled: (doc?.project.levels.length ?? 0) <= 1,
      run: confirmDeleteActiveLevel,
    },
    {
      id: "pipe-takeoff",
      title: "Show the pipe take-off",
      group: "Project",
      icon: "takeoff",
      keywords: "pipe takeoff take off plumbing quantities length elbows tees sleeves csv bill of materials",
      disabled: !doc?.project.elements.some((e) => e.kind === "pipe"),
      run: showPipeTakeoff,
    },
    { id: "hub", title: "Back to all projects", group: "Project", icon: "home", keywords: "hub close home", run: () => void leaveEditor() },
    { id: "shortcuts", title: "Show keyboard shortcuts", group: "Project", icon: "keyboard", keywords: "keys help", shortcut: "?", run: () => shell.open("shortcuts") },
    { id: "settings", title: "Settings", group: "Project", icon: "settings", keywords: "preferences interchange dwg converter oda", run: () => shell.open("settings") },
    { id: "check-updates", title: "Check for updates", group: "Project", icon: "import", keywords: "update upgrade version new release download", run: () => void checkForUpdates(true) },
    { id: "dock-copilot", title: "Ask the copilot", group: "Panels", icon: "copilot", keywords: "ai chat assistant settings", shortcut: `${MOD},`, run: () => shell.setDockTab("copilot") },
    { id: "dock-visuals", title: "Open visuals", group: "Panels", icon: "visuals", keywords: "render capture gallery image", run: () => shell.setDockTab("visuals") },
    {
      id: "render-ai",
      title: "Render with AI",
      group: "Panels",
      icon: "visuals",
      keywords: "ai render visualization gemini realistic image studio",
      run: () => {
        shell.setDockTab("visuals");
        openRenderStudio();
      },
    },
    {
      id: "render-compare",
      title: "Compare renders",
      group: "Panels",
      icon: "split",
      keywords: "compare before after slider ai visualization model view",
      run: () => {
        shell.setDockTab("visuals");
        openRenderCompare();
      },
    },
    { id: "dock-toggle", title: shell.dockCollapsed ? "Expand the side dock" : "Collapse the side dock", group: "Panels", icon: "panelRight", keywords: "dock hide show panel", run: () => shell.setDockCollapsed(!shell.dockCollapsed) },
  );

  return out;
}

/** Subsequence fuzzy score. Higher is better, null means no match. */
export function fuzzyScore(query: string, text: string): number | null {
  const q = query.toLowerCase().trim();
  if (q === "") return 0;
  const t = text.toLowerCase();
  const direct = t.indexOf(q);
  if (direct >= 0) return 1000 - direct - (direct === 0 || t[direct - 1] === " " ? 0 : 40);
  let score = 0;
  let ti = 0;
  let streak = 0;
  for (const ch of q) {
    if (ch === " ") continue;
    const found = t.indexOf(ch, ti);
    if (found < 0) return null;
    const wordStart = found === 0 || t[found - 1] === " ";
    streak = found === ti ? streak + 1 : 0;
    score += (wordStart ? 12 : 2) + streak * 4 - Math.min(8, found - ti);
    ti = found + 1;
  }
  return score;
}
