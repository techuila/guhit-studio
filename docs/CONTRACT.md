# Contract

What every part of the app agrees on. Types live in `crates/guhit-model`
(Rust) and are generated into `src/contract/bindings` (TypeScript) by
`pnpm gen:types`. This file covers what the types cannot say.

## Conventions

- Lengths: f64 millimeters, always. Display unit is a view concern.
- Plan: +x east, +y north. Canvas flips y. 3D: plan (x, y) -> three.js (x, height, -y).
- Angles: degrees, counter-clockwise positive.
- Ids: UUID v4 strings. Built-in materials and catalog items use stable slugs (`mat-...`, `bed-double`).
- New element ids must be deterministic for a given (project state, command), so `preview` and `apply` return identical results.
- Images cross IPC as PNG data URLs.
- Openings: `flip_side` false = leaf swings to the left of the wall direction start -> end (counter-clockwise normal). `flip_hinge` false = hinge on the jamb nearer the wall start.
- Assets: local +y is the back of the object (bed head, sofa back, WC tank). 2D symbols and 3D forms must agree.
- Walls join when endpoints are within 1 mm. New ids are seeded per leaf command, so an id does not change when a batch grows.
- `Derived::footprints` can hold several buildings per level, largest first.
- Dimension endpoints within 1 mm of a wall joint or outline corner follow it when a command moves the wall (DECISIONS D12).
- Stairs: going depth = `run_mm / riser_count`. Annotations: `position` is the left end of the first baseline.

## Pipes

Plumbing is a coordination layer: Guhit places pipes, shows them in 2D and
3D, flags conflicts and counts quantities. It never sizes pipes or claims code
compliance; plumbing plans are signed by a registered Master Plumber (RA 1378).

- `Element::Pipe`: a run of straight segments through `points`. `x`, `y` are
  plan mm, `z` is the centerline height above the level floor (negative below
  the slab). At least two points, no two in a row closer than 1 mm. Drainage
  flows from the first point to the last. Size range 10 to 300 mm.
- Layers: each `PipeSystem` has its own `LayerKey` with the same snake_case
  name (`cold_water`, `hot_water`, `drainage`, `vent`). Visible and locked
  work like every other layer. Schema version 2 added them;
  `guhit_core::migrate` adds missing layers to older projects when a
  `Document` is created.
- Colors: tokens `--pipe-cold`, `--pipe-hot`, `--pipe-drain`, `--pipe-vent`
  in `src/styles/tokens.css`. The 3D view, 2D plan, legends and exports use
  the same four colors.
- Tool defaults (`defaults::pipe_defaults`, mirrored by the frontend):

| System | Material | Size mm | Start height mm | Size menu |
|---|---|---|---|---|
| cold_water | ppr | 20 | 300 | PPR 20 25 32 40 50 63, GI 15 20 25 32 50, PE 20 25 32 |
| hot_water | ppr | 20 | 300 | PPR 20 25 32, copper 15 22 28 |
| drainage | upvc | 50 | -300 | uPVC 32 50 75 100 150 |
| vent | upvc | 50 | 300 | uPVC 32 50 75 100 |

- Drainage fall default: `defaults::drain_min_slope_pct`, 2 percent, 1 percent
  from 100 mm up. The pipe tool lets new drainage fall by it as it is drawn.
- Riser: a segment whose plan length is under 1 mm, or at most 50 mm while it
  rises at least ten times its plan length. Plans, sheets and DXF draw a riser
  as a circle, never as a line.
- Frontend mirror: `src/contract/pipes.ts` holds the defaults, size menus,
  labels, colors and the fall rule. Every frontend module imports them from
  there, never keeps its own copy.
- Pipe tool (2D): clicks place points at the current height. PageUp and
  PageDown change it by 100 mm (Shift: 10 mm), typing `h1500` then Enter sets
  it. A height change adds a riser at the last point and writes
  `toolOptions.pipeElevationMm`. Drainage keeps falling at the default from
  the new height. Snapping onto another run of a joinable system goes level
  (or falls) to the join, then rises or drops to its height; a drain that
  would have to climb slopes up instead and gets a `drain_slope_low` item.

`Derived::pipes` (`PipeNetwork`), recomputed after every change. Positions are
plan x, y and z above the floor of `level_id`:

- Runs join only when they can carry the same flow: the same system, or
  drainage and vent. A cold water end resting on a hot water pipe is a
  `pipes_cross` clash, never a fitting.
- Elbow: an interior point where the direction turns by more than 1 degree.
  Also where the ends of two runs meet at an angle.
- Tee: the end of one run touching another run (3D distance up to the larger
  radius) away from that run's ends. `pipe_id` is the run joined,
  `branch_pipe_id` the run that ends there. Three or more run ends at one
  point make tees too: the two ends that line up best are the run, every
  other end is a branch. A branch landing where the other run bends is one
  tee and no elbow.
- Penetration, one per crossing:
  - `slab`: a segment with one end at or above the floor (z >= 0) and the
    other below it, crossing z = 0 inside a footprint of its level.
  - `wall`: a segment that crosses a wall's thickness (enters one long face,
    leaves the other) between the floor and the wall top, both ends outside
    the wall, not inside one of its openings. It counts only where the pipe
    crosses the wall's centerline inside the wall's resolved outline, so a
    chase passing a T-junction does not cross the partition. Segments along a
    wall (a chase) and vertical segments do not count.
  - `roof`: a segment on the top level that crosses the roof underside, with
    the roof shape the 3D view draws (`src/viewer3d/geom/roofMesh.ts`).
- Take-off: one row per system, material and size, centerline length rounded
  to the millimeter. `sleeve_count` is the number of penetrations.

Review items from pipes. Every one sets `Issue::location` except the summary:

| Code | Severity | element_ids | When |
|---|---|---|---|
| `pipe_through_column` | warning | pipe, column | the pipe body enters a column, floor to level height |
| `pipe_across_opening` | warning | pipe, opening | the pipe body passes through a door or window opening |
| `pipes_cross` | warning | two pipes, id order | two runs' bodies overlap where they are not joined |
| `drain_slope_low` | warning | pipe | a drainage segment at least 300 mm long, flatter than 45 degrees, falls less than the default or runs uphill |
| `pipe_penetrations` | info | pipes with penetrations | summary: how many sleeves or flashings, by kind |

One item per pipe and target, located at the first hit. Messages are plain
suggestions ("Route it above the door head at 2.10 m or under the slab"),
never approvals.

`Query::PipeTakeoff` returns `{ rows: [{ system, material, diameter_mm,
length_m, run_count }], total_length_m, elbow_count, tee_count, sleeve_count,
penetrations: [{ kind, pipe_id, host_id, position_mm }], note }`.

Exports: `PlanExportOptions::show_pipes` draws pipes on visible pipe layers
with a legend (SVG, PDF) or on one DXF layer per system: `P-DOMW-CPIP` cold
water, `P-DOMW-HPIP` hot water, `P-SANR-PIPE` drainage, `P-SANR-VENT` vent. 3D
DXF draws them as tubes on the same layers. IFC4 writes `IfcPipeSegment`s
assigned to one `IfcDistributionSystem` per system.

## 3D navigation and shell view state

`src/viewer3d/viewerStore.ts`, view state only, never saved in the project:

- `nav`: `orbit`, `walk` (eye height 1600 mm on the active level; walls,
  columns and objects taller than 300 mm block; door openings let you
  through), `fly` (free, nothing blocks).
  While not `orbit` the 3D view owns every key without MOD; Escape returns
  to orbit.
- `shell`: `solid`, `xray` (the building is drawn faint so pipes read through
  it), `hidden` (only floors, pipes and ghosted outlines). Pipes stay solid.
- `bus.emit("walk_to", { ids, location })` enters walk mode near a finding.
- The global shortcut handler defers keys to the 3D view only while that view
  is on screen. Switching to plan only ends a walk (`nav` back to `orbit`).
- The shell sends `walk_to` or sets `nav` only once the 3D view is up; the
  signal is `useApp().captureView` being registered. The 3D view resets `nav`
  to `orbit` when it truly unmounts.

## Engine API (`guhit-core`)

```rust
Document::new(project) -> Document
doc.state() -> DocState
doc.apply(command, origin) -> Result<ApplyResult, CoreError>   // one undo step, atomic
doc.preview(&command) -> Result<ApplyResult, CoreError>        // same result, commits nothing
doc.undo() / doc.redo() -> Result<DocState, CoreError>
doc.query(&query) -> Result<serde_json::Value, CoreError>
compute_derived(&project) -> Derived
templates::sample_bungalow() -> Project
Document::with_revision(project, revision) -> Document       // restore without reusing revisions
doc.rename(name) -> Result<(), CoreError>                   // not an undo step, survives undo
migrate(&mut project)                                      // fills layers missing in older files; Document::new calls it
templates::plumbing_demo() -> Project                      // the `plumbing-demo` template
pipe_name(&pipe) -> String                                 // "Kitchen sink waste" or "Cold water pipe 20 mm", as review items say it
```

App service helpers shared with the AI module: `AppService::commit(command, origin)` (apply + autosave), `AppService::commit_if_revision(command, origin, expected_revision)` (same, atomic, `stale` on mismatch) and `AppService::project_dir()`.

`AppService::new` allows caller-supplied export paths (desktop, from the native save dialog). `AppService::new_sandboxed` (dev bridge) rejects any path outside its data dir with `forbidden`. `snapshot_restore` continues the revision counter, revisions never repeat.

After every change the engine recomputes `Derived`: wall outlines with
resolved joins, room polygons and areas, footprints, totals, review issues.

## IPC commands

One entry point: `AppService::handle(cmd, args) -> Result<Value, IpcError>`.
Args are a JSON object with the names below. The typed client is `src/contract/ipc.ts`.

| Command | Args | Returns | Notes |
|---|---|---|---|
| `hub_list` | | `ProjectMeta[]` | newest first |
| `hub_create` | `name`, `settings?`, `template?` | `DocState` | opens it. templates: `blank`, `sample-bungalow`, `plumbing-demo` (the bungalow with a T&B, fixtures and 16 pipe runs) |
| `hub_open` | `id` | `DocState` | |
| `hub_rename` | `id`, `name` | `ProjectMeta` | |
| `hub_duplicate` | `id` | `ProjectMeta` | |
| `hub_delete` | `id` | `null` | moves the folder to `trash/`, never hard-deletes |
| `hub_set_thumbnail` | `id`, `png` | `null` | |
| `hub_close` | | `null` | flushes autosave |
| `doc_state` | | `DocState or null` | |
| `doc_apply` | `command` | `ApplyResult` | autosaves |
| `doc_preview` | `command` | `ApplyResult` | |
| `doc_undo`, `doc_redo` | | `DocState` | autosaves |
| `doc_query` | `query` | JSON | |
| `doc_revision` | | `{revision, project_id}` | cheap poll for external changes |
| `snapshot_create` | `label` | `SnapshotMeta` | named version |
| `snapshot_list` | | `SnapshotMeta[]` | |
| `snapshot_restore` | `id` | `DocState` | takes an auto snapshot first; is undoable history reset |
| `catalog_assets` | | `CatalogItem[]` | |
| `export_plan` | `format`, `options`, `path?` | `ExportResult` | null path -> `<data>/exports/` |
| `export_image` | `png`, `name`, `path?` | `ExportResult` | |
| `underlay_store` | `file_name`, `data` | `{file_name}` | copies into the project folder |
| `underlay_data` | `file_name` | data URL | |
| `import_inspect` | `path` or `file_name`+`data` | `ImportInspection` | DXF; DWG when the converter is configured |
| `import_commit` | source + `options` | `ImportResult` | one undo step "Import <file>" |
| `model_store` | source | `{file_name, size}` | glTF/GLB/OBJ into `models/` |
| `model_data` | `file_name` | data URL | |
| `export_model` | `format` (ifc, dxf3d, dwg), `path?` | `ExportResult` | |
| `export_bytes` | `name`, `data`, `path?` | `ExportResult` | GLB/OBJ/DAE made by the frontend |
| `bundle_save` | `path?` | `ExportResult` | `.guhit` zip of the project folder |
| `bundle_open` | source | `DocState` | copies into `projects/`, new id if it collides |
| `dwg_status`, `dwg_set_path` | , `path` | `DwgConverterStatus` | ODA File Converter, stored in settings.json |
| `render_styles` | | `RenderStyle[]` | |
| `render_list` | | `RenderRecord[]` | |
| `render_capture` | `camera`, `png` | `RenderRecord` | Tier 1 capture, tied to revision |
| `render_data` | `id` | data URL | |
| `render_delete` | `id` | `null` | |
| `render_ai_settings_get` | | `RenderAiSettings` | |
| `render_ai_settings_set` | `api_key?`, `model?` | `RenderAiSettings` | `""` removes the key |
| `render_ai_generate` | `request` | `RenderAiResult` | 10 to 60 s; writes a `RenderRecord` with source `ai_visualization` and `source_render_id` |
| `ai_settings_get` | | `AiSettings` | |
| `ai_settings_set` | `api_key?`, `model?` | `AiSettings` | `""` removes the key |
| `ai_chat` | `request` | `AiTurn` | may hold one pending proposal |
| `ai_resolve` | `proposal_id`, `accept` | `AiResolveResult` | rejects with `stale` if revision moved |

External changes: after every commit, undo, redo, open, create, close, delete and restore, `AppService::watch_changes()` fires `{revision, project_id, seq}`. The desktop shell forwards it as the Tauri event `doc_changed {revision}`; the dev bridge serves `/mcp` on its port and the UI polls `doc_revision`. The frontend subscribes with `onDocChanged` (`src/contract/ipc.ts`): `App.tsx` switches hub to editor, `EditorShell` refetches state. Full MCP tool list: `docs/MCP.md`.

Errors are always `IpcError { code, message, element_ids }`. Codes: `not_found`, `invalid`, `no_document`, `stale`, `io`, `ai_not_configured`, `ai_failed`, `unknown_command`, `bad_args`, `forbidden` (dev bridge, non-localhost origin).

`hub_open` on the project that is already open returns the current state with its undo history intact.

## Storage layout

```
<data_dir>/
  projects/<project-id>/
    project.json          # Project, written atomically (temp file + rename)
    thumbnail.png
    snapshots/<id>.json   # { meta: SnapshotMeta, project: Project }
    underlays/<file>
    renders/<id>.png + renders/index.json
    models/<file>       # reference models (glb, gltf, obj)
    ai-log.jsonl          # one line per AI tool call and outcome
  exports/
  trash/
  settings.json
```

`data_dir` is the OS app data dir in the desktop app and `.devdata/` for the bridge.

## Dev bridge

`cargo run -p guhit-devbridge -- [--port 1430] [--data .devdata]`

- `POST /ipc/<cmd>` with the args object as JSON body. 200 + result JSON, or 400 + `IpcError`.
- `GET /health` -> `{"ok":true}`.
- CORS: allow any `http://localhost:*` origin. Binds 127.0.0.1 only.
- The UI picks the bridge URL from `VITE_BRIDGE_URL` (default `http://localhost:1430`).

## Frontend join points

| File | Export | Owner |
|---|---|---|
| `src/shell/EditorShell.tsx` | `EditorShell()` | shell. Mounts everything below. |
| `src/hub/ProjectHub.tsx` | `ProjectHub()` | shell |
| `src/editor2d/PlanCanvas.tsx` | `PlanCanvas()` fills its parent | 2D |
| `src/viewer3d/Viewer3D.tsx` | `Viewer3D()` fills its parent | 3D |
| `src/viewer3d/RenderPanel.tsx` | `RenderPanel()` fills its parent. Visuals gallery. | 3D |
| `src/ai/AiDock.tsx` | `AiDock()` fills its parent | AI |

Rules:
- Read state with `useApp` selectors. Draw `useVisibleDoc()` so AI previews show as ghosts; elements in `preview.diff` are tinted with `--draw-preview`.
- Mutate only via `useApp.getState().dispatch(command)`.
- `Viewer3D` registers `captureView` and `exportScene`, `PlanCanvas` registers `capturePlan`.
- One-shot view requests go over `src/state/bus.ts`.
- Respect `project.layers` (visible, locked) and `activeLevelId`.
