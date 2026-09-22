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
| `hub_create` | `name`, `settings?`, `template?` | `DocState` | opens it. templates: `blank`, `sample-bungalow` |
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
