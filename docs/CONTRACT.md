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
  flows from the first point to the last. Size range 6 to 300 mm (a 1/4 inch
  refrigerant liquid line is 6.35 mm).
- Systems: plumbing (`cold_water`, `hot_water`, `drainage`, `vent`), storm
  drainage (`storm`), electrical `conduit`, and aircon `refrigerant` (a line
  set, `diameter_mm` is the gas line) and `condensate`. Drainage, storm and
  condensate flow from the first point to the last (`PipeSystem::falls`).
- Layers (`PipeSystem::layer`, `PIPE_LAYER` in `src/contract/pipes.ts`): the
  four plumbing systems each have a layer of the same name; `storm` has
  `storm`; `conduit` is on `electrical`; `refrigerant` and `condensate` are on
  `aircon`. Electrical and lighting objects share the `electrical` layer, aircon
  units the `aircon` layer. Visible and locked work like every other layer.
  Schema 2 added the plumbing layers, schema 3 the others;
  `guhit_core::migrate` adds missing layers when a `Document` is created.
- Colors: tokens `--pipe-cold`, `--pipe-hot`, `--pipe-drain`, `--pipe-vent`,
  `--pipe-storm`, `--pipe-conduit`, `--pipe-refrigerant`, `--pipe-condensate`
  in `src/styles/tokens.css`. The 3D view, 2D plan, legends and exports use
  the same four colors.
- Tool defaults (`defaults::pipe_defaults`, mirrored by the frontend):

| System | Material | Size mm | Start height mm | Size menu |
|---|---|---|---|---|
| cold_water | ppr | 20 | 300 | PPR 20 25 32 40 50 63, GI 15 20 25 32 50, PE 20 25 32 |
| hot_water | ppr | 20 | 300 | PPR 20 25 32, copper 15 22 28 |
| drainage | upvc | 50 | -300 | uPVC 32 50 75 100 150 |
| vent | upvc | 50 | 300 | uPVC 32 50 75 100 |
| storm | upvc | 100 | -300 | uPVC 75 100 150 |
| conduit | pvc | 20 | 2800 | PVC 20 25 32 40 50, EMT 15 20 25, IMC 20 25, flexible 15 20 |
| refrigerant | copper | 9.52 | 2400 | copper 9.52 12.7 15.88 (gas line; the liquid line is 6.35) |
| condensate | pvc | 20 | 2300 | PVC 20 25 32 |

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

Service runs reuse every rule above. Differences by system:

- Conduit never needs a sleeve (it is cast into slabs and walls), so conduit
  crossings are left out of penetrations and `sleeve_count`.
- A refrigerant or condensate run crossing a wall is a core hole (65 mm, or
  90 mm for gas lines from 16 mm), sloped 5 to 7 mm down to the outside. The
  penetration summary names them so.
- The fall check covers every system that falls (`drain_slope_low` for
  drainage and storm, `condensate_slope_low` for condensate), with the drain
  default as the review default: no aircon manual gives a number.
- Joins: runs join within one system, drainage with vent, and nothing else.

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

## Devices, fixtures and links

Electrical, lighting, aircon and utility objects are `Asset`s (categories
`lighting`, `electrical`, `aircon`, `utility`); there is no separate device
element. What makes them devices lives in the catalog item:

- `CatalogItem::mount`: `floor`, `wall` (the back, +y, sits on a wall face),
  `ceiling` (hangs from the level height), `opening` (a window aircon). Placing
  a wall item snaps it to the nearest wall face; `elevation_mm` is the default
  underside height (outlets 243 so the center is about 300, switches 1143 so
  the center is 1200, split indoor units 2300). Ceiling items hang from the
  ceiling: the level height, or the underside of the next level's 200 mm
  slab (the slab the 3D view draws) when that is lower. Their underside is
  the ceiling minus their own height, except the pendant, which keeps its
  catalog underside (2000) and is only lowered when the ceiling would cut it.
- `CatalogItem::device`: the row of the PH electrical inspection form it
  counts under (lighting outlet, convenience receptacle, special purpose
  outlet, switch, panelboard, smoke detector, buzzer, push button) or an aircon
  role.
- `CatalogItem::light`: fixtures copy it into `Asset::light` when placed
  (lumens, kelvin, on). 900 lm is a 9 W LED bulb sold in PH.
- `CatalogItem::aircon`: line set sizes and manufacturer limits (minimum 3 m,
  maximum length and height difference, 3 m included in a standard install).

`Asset::links` holds the ids of what a device controls or feeds: a switch lists
its lights; an aircon outlet lists its unit. Two switches linking one light make
it a 3-way (drawn "S3"). The link tool (`L`) writes links; deleting an asset
removes it from every `links` list in the same command. `Asset::circuit` is a
free tag ("L1"); Guhit never checks circuits, loads or ratings (the PEE's work).

Switch placement: 200 mm from the latch side of the nearest door, center
1200 mm (BP 344 IRR 2024). The placement tool offers it; nothing enforces it.

Plan symbols (the 2D editor in `src/editor2d/symbols.ts` and the sheets in
`guhit-export` draw the same shapes; D is the symbol size, about 300 mm at
1:100, never scaled with the object):

| Catalog key | Symbol |
|---|---|
| `light-ceiling`, `light-pendant` | circle D with an X; pendant adds a small "P" |
| `light-downlight` | circle 0.6 D with a dot at the center |
| `light-tube` | the fixture's own rectangle with a line along it |
| `light-wall`, `light-outdoor` | half circle on the wall face with a line; outdoor adds "WP" |
| `light-floor-lamp`, `light-table-lamp` | circle 0.6 D with an X, drawn thin (plug-in lamp) |
| `outlet-duplex`, `outlet-counter` | circle 0.5 D on the wall face with two short parallel lines through it |
| `outlet-outdoor` | the duplex outlet with "WP" |
| `outlet-spo`, `outlet-aircon` | the duplex outlet, half filled, with "SPO" or "ACO" |
| `switch-1`, `switch-2`, `switch-3` | "S" beside the wall face with 1 to 3 dots under it; a switch sharing a light with another switch reads "S3" |
| `panelboard` | rectangle on the wall face, half filled on a diagonal, "PB" |
| `smoke-detector` | circle 0.6 D with "SD" |
| `doorbell-button`, `doorbell-chime` | small circle with a dot, "PB"; square with "CH" |
| `aircon-indoor-*` | the unit's rectangle with an arrow away from the wall, "ACU" |
| `aircon-outdoor-*` | the unit's rectangle with a circle (fan), "CU" |
| `aircon-window` | the unit's rectangle across the wall, "AC" |
| links | dashed arc from a switch to each light it controls, bowing to one side |

`Derived::schedule` counts objects per level and room for every catalog item
that is a device or a sanitary, lighting, electrical, aircon or utility item.
The kitchen sink (`kitchen-sink`) and the washing machine (`washing-machine`)
count in the plumbing group too, as fixtures with a water supply and a drain.

Review items from devices and aircon (suggestions, same wording rules):

| Code | Severity | element_ids | When |
|---|---|---|---|
| `light_no_switch` | info | light | a ceiling or wall light that no switch links |
| `switch_no_load` | info | switch | a switch that links nothing |
| `switch_behind_door` | warning | switch, door | inside the swing of a door on its hinge side |
| `aircon_no_outlet` | warning | unit | no aircon or special purpose outlet links an indoor or window unit |
| `lineset_long` | warning | run, indoor unit | longer than the unit's maximum |
| `lineset_rise` | warning | run, indoor unit | height difference over the unit's maximum |
| `lineset_short` | info | run, indoor unit | shorter than 3 m |
| `lineset_extra` | info | run | meters beyond the 3 m a standard install includes |
| `condensate_slope_low` | warning | run | falls less than the default or runs uphill |
| `condensate_open_end` | info | run | ends away from a drain, a floor drain or the outside |
| `indoor_unit_clearance` | warning | unit | under 150 mm free above, 120 mm at a side, or underside below 2300 mm |
| `outdoor_unit_clearance` | warning | unit | something within 2000 mm in front, 300 behind, 300 or 600 at the sides |
| `outdoor_unit_unsupported` | warning | unit | raised above the floor with no wall, slab or bracket under it |
| `unit_near_tv` | info | unit, tv console | a TV within 1 m |

Engine readings of the table above:

- An outlet linked to an outdoor unit counts as feeding the indoor unit its
  line set reaches (`aircon_no_outlet`).
- Outdoor unit sides are seen facing its front: 300 mm left, 600 mm right.
- `condensate_open_end` accepts a run ending outside the building, or within
  300 mm of a floor drain or a drain pipe. A condensate drain touching a drain
  pipe is still a `pipes_cross` clash (joins stay within one system); end it
  within 300 mm instead.
- A condensate crossing within 300 mm of a line set crossing on the same wall
  shares its core hole; `sleeve_count` still counts both.
- Unnamed runs read "Conduit 20 mm", "Line set 9.52 mm".
- Error codes: `unknown_review_code`, `bad_review_target`, `no_review_mark`,
  `asset_light`, `link_to_itself`, `duplicate_link`, `bad_link`,
  `circuit_too_long`.

## Review marks

`Project::review` stores only set-aside findings, each with a note.
`Command::SetReviewMark { target, note }` sets one (`Some(note)`) or removes
it (`None`); targets are one finding (`Issue::id`), a whole check (`code`), or
a check on one element. Derive merges them: an issue whose target matches is
`status: ignored` with the mark's `note`; a mark for one finding that no
longer appears is listed in `Derived::review_resolved` (resolved). Nothing is
ever "approved". The review list groups by level, then room, with counts;
ignoring asks for a note.

## AI edit scope

DECISIONS D30. `EditScope { ids }` limits an AI edit to the selection. The
copilot receives it as `AiRequest::scope`; MCP edits take it from the call
(`scope`) or from the window's presence (`Presence::ai_scope`, which binds
every MCP edit while it is on).

`guhit_core::scope`:
- `validate(project, scope_ids)`: at least one id, and every id in the plan
  (`not_found` otherwise). Run it once, before the turn.
- `check(project, derived, scope_ids, command)`: validates, then answers for
  one command.
- `check_staged(project, derived, scope_ids, made_ids, command)`: for a
  command staged after others in the same turn. `project` and `derived` are
  the plan as staged so far; `made_ids` are the elements the earlier steps
  created (the staged preview's `diff.added`). Those are in reach, so a later
  step can build on them (a door on a wall the turn added), but they never
  widen the area, so what one step makes cannot carry the turn outside the
  selection. Selected ids that an earlier step removed are skipped.
- `describe(project, derived, scope_ids)`: one sentence for the model, for
  example "Room Bedroom with its 4 walls, 1 door, 1 window and 1 object, on
  Ground Floor".

Reach, derived on the project being checked:

| Selected | Reaches |
|---|---|
| room | itself; its bounding walls (`RoomGeometry::wall_ids`) and every wall standing inside it; the doors and windows on its part of those walls (center within 50 mm of its centerline polygon: a long wall can bound the next room too); and on its level every column, stair, object, text, dimension (both ends), pipe (every point and the segments between) and camera (on the storey that holds its height) inside its centerline polygon, with 50 mm of tolerance |
| wall | itself and its doors and windows |
| anything else | itself |
| made by an earlier step (`made_ids`) | itself; a wall also its doors and windows |

Area, for new elements: each selected room's centerline polygon, with 50 mm
of tolerance so a wall ending on a bounding wall's centerline is inside; the
bounding box of every other selected element grown by 500 mm. Per level: a
new wall or room without a level goes on the first level, as the engine puts
it. Cameras give no area. Walls and pipe runs are checked along every
segment (points at most 25 mm apart), so a wall from one selected room to
another cannot cross the room between them.

| Command | Allowed when |
|---|---|
| `add_wall`, `add_wall_chain`, `add_rect_room` | every point or corner, and every side between them, is in the area on its level |
| `add_opening` | the host wall is in reach and the opening's center is in the area |
| `add_element` | its anchor (wall centerline, asset position, column center, stair origin, text position, dimension ends, pipe points and segments, room seed) is in the area on its level; an opening as `add_opening`; a camera always (a view changes no part of the plan); an underlay, linework or reference model never |
| `update_element` | the element is in reach; an opening moved to another wall as `add_opening` on that wall |
| `set_wall_endpoints`, `set_wall_length`, `split_wall`, `resize_room`, `delete_elements`, `move_elements`, `rotate_elements`, `set_material` | every target is in reach |
| `duplicate_elements` | every target is in reach, and every copy lands as `add_element` would; a door or window copied without its wall stays on that wall |
| `set_review_mark` | an element target in reach, or a finding whose elements are all in reach (a finding the checks no longer make is read from its id, `code:ids`); a whole check never |
| `set_roof`, `set_project_settings`, `update_level`, `add_level`, `delete_level`, `set_layer`, `upsert_material` | never: they change the whole project |
| `batch` | every command in it, each on the project as the steps before it leave it, with what they made in reach; a refusal starts "Step 2 of 3: " like the engine's batch errors |

A refusal is `CoreError::Invalid { code: "out_of_scope" }` naming the
element in plain words ("Wall 3000 mm is outside the selection this edit is
limited to.") with its id in `element_ids`. It reaches IPC as `IpcError {
code: "invalid" }` with the element ids, and the model as a tool error that
starts with `out_of_scope:`. Side effects are allowed: connected walls
stretching, dimensions following (D12), links removed (D21), rooms appearing
in closed faces (D7). A scope naming an element that is not in the plan, or
a command naming a target that is not, is refused with `not_found`. The area
follows the staged plan, so once a step deletes a selected element its area
is gone: a turn that replaces a selected element adds the new one first.

## Sheets

`PlanExportOptions::sheet` (default `plan`) picks the sheet: `lighting`,
`power` (with a schedule of loads whose rating columns stay blank for the
PEE), `plumbing` (with a fixture table), `plumbing_isometric` (water and
sanitary diagrams, not to scale, legend box, a blank Master Plumber block),
`aircon`. `review_page` adds a PDF page of review items and notes. DXF writes
devices as blocks with attributes (type, tag, height, room) on NCS style
layers (`E-LITE-FIXT`, `E-POWR-DEVC`, `M-HVAC-EQPM` and the service run
layers).

## Sun and light

- Site: `ProjectSettings::site` (city preset, latitude, longitude, UTC offset
  in minutes). None means Manila (`defaults::default_site`). The frontend
  bundles PH city presets; there is no geocoding service.
- Live light: `useViewer().light` (month, day, local minutes, sky, exposure,
  lamps). Not saved in the project, not an undo step. `Camera::light`
  (`ViewLight`) saves it with a view, lamps included (`LampMode`: `auto`,
  `on` or `off`); applying the view restores it.
- Keys (3D view or app, not while typing or walking): `U` and `I` move the sun
  15 minutes back and forward (hold to scrub); `Shift+U` and `Shift+I` step
  through the presets (Morning 8:00, Noon, Afternoon 3 PM, Dusk, Night 8 PM);
  `Shift+N` switches lamps between auto and on.
- Sun position from SunCalc's formulas (no dependency), north from
  `ProjectSettings::north_angle_deg`.
- Sky: `clear` (a physical sky that follows the sun), `cloudy`, `photo` (the
  pack HDRI, turned so its sun sits at the computed azimuth).
- Exposure is automatic in the live view and locked into a saved view,
  a render or a study.
- Lamps: a fixture gives light when `Asset::light.on` and the lamps are on,
  or on auto from dusk to dawn. Off keeps every fixture dark. Rooms with no fixture get a soft ghost light at
  night so interiors are never pitch black; it is view only, never written to
  the model.
- Refine: when the camera rests, the view blends jittered frames (clean edges,
  soft sun shadows) and then stops. It follows the frame loop rule: every
  refine frame is scheduled through `ViewerEngine.schedule()`, and a still,
  refined view draws nothing.

## Render

`bus.emit("render", { views })` renders the current view, every saved view,
or chosen cameras with a path tracer (`three-gpu-pathtracer`, WebGL 2, lazy
loaded) in its own offscreen renderer, so the live view stays usable. Sizes: HD
1920 x 1080 (default), QHD 2560 x 1440, 4K 3840 x 2160, square 2048 x 2048.
Quality: quick or final (`TraceQuality`), set by time. The result is
denoised, saved as a `RenderRecord` (`render_capture`) with the camera and
light it used, the revision the render started from, and `RenderRecord::info`
(`RenderInfo`: kind, size, samples, seconds, quality, graphics adapter), and
offered to "Visualize with AI" (D17). The job never calls
requestAnimationFrame: it paces itself on GPU fences, so the live view keeps
its one pending frame. Esc cancels; stopping early saves what
is there. If the path tracer cannot start, the render falls back to a refined
raster capture labelled "Enhanced capture".

`bus.emit("shadow_study")` opens the shadow study: frames from the live view
every 30 or 60 minutes over one or more dates, exported as a contact sheet
with time, date, place and a north arrow on each frame.

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
- Wheel while walking: it changes speed while a move key is held or with Alt,
  and otherwise moves you forward and back. At the top of a stair you switch to
  the level whose floor elevation is within 300 mm of the top.
- Levels: `AddLevel` stacks a new level on the highest one by default;
  `DeleteLevel` removes a level with everything on it (never the last level).
- Walk settings (`useViewer().walk`, remembered per computer): eye height
  800 to 2500 mm (default 1600), speed (default 1.4 m/s); the wheel changes
  speed while walking. Clicking the minimap moves you there; double-clicking
  the floor glides there at eye height. Walking onto a stair's run climbs it
  as a ramp and switches level at the landing. Door leaves swing open as you
  approach and close behind you (view only). Mouse-only: drag to look, scroll
  or two-finger swipe to move.
- The global shortcut handler defers keys to the 3D view only while that view
  is on screen. Switching to plan only ends a walk (`nav` back to `orbit`).
- The shell sends `walk_to` or sets `nav` only once the 3D view is up; the
  signal is `useApp().captureView` being registered. The 3D view resets `nav`
  to `orbit` when it truly unmounts.

## Live sessions

DECISIONS D29. Types: `crates/guhit-model/src/live.rs`. Code:
`crates/guhit-app/src/live/`.

- One computer hosts the open project (`live_host`). Its `Document` is the
  only authority. A guest (`live_join`) keeps a read-only copy: the host's
  project, revision and undo labels, with `Derived` computed locally by the
  same engine. Every change a guest makes is a typed `Command` sent to the
  host, validated and applied there, and the new state goes to everyone.
- Transport: TCP with TLS 1.3 (rustls, ring). The host makes a self-signed
  certificate for each session. Frames are a 4-byte big-endian length and a
  UTF-8 JSON message; at most 64 KiB before the guest is authenticated,
  48 MiB after. The frame messages are internal to `guhit-app` (host and
  guest are the same code, `live/wire.rs`): guest to host `hello`,
  `request` (apply, undo, redo, chat, file get, file put; the reply carries
  its id), `presence`, `ping`, `bye`; host to guest `welcome`, `refused`,
  `reply`, `doc`, `presence`, `participants`, `chat`, `ping`, `end`. A
  frame that is too big or not a message closes that connection only.
- A side with nothing to send pings after 5 s; 20 s without a frame means
  the connection is lost. Each connection has one writer with a queue of
  64 frames: presence for a slow guest waits for the next batch, but a
  guest whose queue is full for a document, chat, participants or reply
  frame is disconnected (it reconnects and gets the current plan).
- The host sends a document frame (project, revision, undo meta, who made
  the change, and the host's change counter, which orders the frames)
  after every change of its document, whoever made it, and before the
  reply to the guest edit that made it. So a guest's `doc_apply`,
  `doc_undo` and `doc_redo` return its copy at the new revision or later
  (with the host's diff for `doc_apply`). Renaming the open project also
  notifies watchers, so guests get the new name.
- Underlays and reference models travel in pieces of 2 MiB, so the 50 MB
  model limit fits under the frame limit. The host stores them with the
  same name, type and size checks as the window's own uploads.
- Invite: `guhit-live:` then base64url (no padding) of
  `{"v":1,"secret":...,"pin":...,"addrs":["192.168.1.20:1460",...],"project":"Bungalow"}`.
  `secret` is 128 random bits. `pin` is the SHA-256 of the certificate
  (base64url). A guest accepts only the pinned certificate (no CA, no host
  name check) and tries the addresses in order, 4 s each; the host compares
  the secret in constant time, answers a wrong secret after a 1 s delay and
  keeps at most 8 connections waiting to authenticate.
- Listening: the desktop app on every IPv4 interface, port `live_port` in
  `settings.json` (default 1460, else the next free one of the nine after
  it, else one the system picks), only while a session runs. A `port`
  given to `live_host` must be free (`invalid` otherwise). The dev bridge
  on 127.0.0.1 only. `LiveStatus::addresses` lists the computer's LAN
  address first (found without sending anything) and 127.0.0.1 last.
- Names: `profile_get`/`profile_set` (`settings.json` key `profile_name`,
  1 to 40 characters, control characters removed). Hosting and joining need
  one (`bad_args` otherwise). Colors: the host is 0, each guest gets the
  lowest color no one in the session has, the least used one once all
  eight are taken; at most 16 participants (`live_refused` beyond). The
  host's participant id stays the same for every session while the app
  runs, so its steps stay its own.
- Why a join failed goes into `LiveStatus::notice` too.
- Ends: the host's `live_leave`, or the host closing or switching the
  project (`hub_close`, `hub_open` of another, `hub_create`, `bundle_open`,
  `hub_delete` of it, from the window or an MCP client) ends the session
  for everyone; guests get `LiveStatus::notice` ("Ana ended the live
  session.", or "Ana closed the project."), their document closes and
  their window goes to the hub. The host's own notice after a close: "The
  live session ended because the project closed." A guest's `live_leave`
  or `hub_close` leaves. `live_remove` (host only) removes a guest with a
  notice ("Ana removed you from the live session.").
- A guest that loses the connection is `reconnecting`: its window keeps the
  last plan, edits fail with `live_lost`, and it retries after 1, 2, 4 and
  8 s with the same invite, asking for its old participant id and color
  back (the host keeps them 60 s). The welcome gives each guest a token
  for this, so no one else can take its place. Back in: `joined` and the
  host's current state. Otherwise: `off`, notice "Lost the connection to
  the host.", the document closes. A guest the host removed is not let
  back in as the same participant.
- `live_save_copy` (guest, also after the session ended until another
  project opens) saves the last copy as a new local project with a new id,
  named "<name> (copy)". While the session runs, the underlay images and
  reference models the plan uses come along.

On a guest, these go to the host: `doc_apply`, `doc_undo`, `doc_redo`,
`AppService::commit` and `commit_if_revision` (copilot Apply, MCP edits,
`import_commit`), `underlay_store`, `underlay_data`, `model_store`,
`model_data`, `chat_send`. These use the local copy: `doc_state`,
`doc_revision`, `doc_preview`, `doc_query`, exports, the copilot's reading
and staging, MCP reads. Renders, exports and the AI log go to
`<data>/live/<project-id>/`, the guest's folder for the shared project
(exports without a path to its `exports/`). The copy is never saved to
`projects/`.
Refused on a guest with `host_only`: `snapshot_create`, `snapshot_list`,
`snapshot_restore`, `hub_rename` of the shared project, `bundle_save`.
Refused while joined, with `host_only` and "Leave the live session first":
`hub_open`, `hub_create`, `bundle_open`. `hub_set_thumbnail` of the shared
project does nothing on a guest.

History in a live session: every commit records its author
(`Document::apply_as`), `DocState::undo_by` and `redo_by` say whose step is
on top. `doc_undo` and `doc_redo` take back or bring back the step on top
of the one shared history. When that step is someone else's (another
participant's, or on a guest a step made before the session) and `force`
is not true, they fail with `other_author` and a sentence naming the
person and the step ("Ana made the last change: Move wall. Undo it
anyway?"); the window asks and calls again with `force`. On the host, steps
with no author (made before the session) are its own.

Presence: `presence_set` stores this window's `Presence`. MCP
`get_selection` reads it. In a session it goes to the others at most 20
times a second (the host sends batches every 50 ms, latest wins); `typing`
is cut to 160 characters and `selection` to 2000 ids, and ids longer than
64 characters are dropped. Everyone else's
arrives as `AppEvent::Presence`; `presence_list` returns the latest of
each, for a window that loads mid-session. A participant who leaves gets
`presence: None`.

Chat: `chat_send` (live only, `not_live` otherwise). Text is trimmed, with
control characters other than line breaks and tabs removed, 1 to 2000
characters, or 160 with `at` (cursor chat); `bad_args` otherwise, and for
an `at` that is not a number or a `level_id` that is not an id. The host
stamps the id,
author, color and time, appends the message to `<project>/chat.jsonl` on the
host (one JSON object per line, best effort) and sends `AppEvent::Chat` to
every window, the sender's included. `chat_list` returns the last 500 of
the open project's chat, oldest first: from `chat.jsonl` on the host or
with no session, from what the host sent on join plus what came since on a
guest.

## App events and window requests

`AppService::events()` is a broadcast of `AppEvent`. The desktop shell
emits each one to the window as the Tauri event `app_event`; the dev bridge
streams them as server-sent events on `GET /events` (one JSON `AppEvent`
per `data:` line). The frontend subscribes with `onAppEvent`
(`src/contract/ipc.ts`). Document changes keep `doc_changed`.

Window requests (DECISIONS D31): `AppService::window_request(task,
timeout)` emits `AppEvent::WindowRequest` and waits for the window's
`window_reply { id, reply, error }`. `window_start(task)` and
`window_wait(id, timeout)` split it for long renders (a job id the MCP
client can come back with; results are kept 10 minutes). No reply in time,
or no window listening: `no_window`. A window answers only while it shows a
project (`no_document` otherwise); the first reply wins.

| `WindowTask` | The window | `WindowReply` |
|---|---|---|
| `render { views, quality, size }` | renders like the Render button (`views` empty: the current 3D view, opening the 3D view first when needed) and saves each image to Visuals | `render_ids` in order, `image` a preview of the first at most 1568 px, `note` |
| `capture_view { camera_id }` | saves a capture of the live 3D view, from the saved view when given | `render_ids` with the record, `image` its preview |
| `capture_plan { level_id }` | draws the plan of that level (or the one on screen) on white, like the hub thumbnail but full size | `image`, a PNG data URL; nothing saved |

## Engine API (`guhit-core`)

```rust
Document::new(project) -> Document
doc.state() -> DocState
doc.apply(command, origin) -> Result<ApplyResult, CoreError>   // one undo step, atomic
doc.apply_as(command, origin, author) -> Result<ApplyResult, CoreError> // same, recording a live session participant (DocState::undo_by)
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
scope::validate(&project, &scope_ids) -> Result<(), CoreError>            // AI edit scope ("AI edit scope" above): ids present, not empty
scope::check(&project, &derived, &scope_ids, &command) -> Result<(), CoreError> // validate, then one command
scope::check_staged(&project, &derived, &scope_ids, &made_ids, &command)       // a command after others in the same turn
scope::describe(&project, &derived, &scope_ids) -> String                    // one sentence for the model
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
| `hub_create` | `name`, `settings?`, `template?` | `DocState` | opens it. templates: `blank`, `sample-bungalow`, `plumbing-demo` ("Bungalow with services": T&B, 16 plumbing runs, 2 downspouts, a line set and condensate drain, and 32 objects including lights, switches, outlets, a panelboard and a split aircon) |
| `hub_open` | `id` | `DocState` | |
| `hub_rename` | `id`, `name` | `ProjectMeta` | |
| `hub_duplicate` | `id` | `ProjectMeta` | |
| `hub_delete` | `id` | `null` | moves the folder to `trash/`, never hard-deletes |
| `hub_set_thumbnail` | `id`, `png` | `null` | |
| `hub_close` | | `null` | flushes autosave |
| `doc_state` | | `DocState or null` | |
| `doc_apply` | `command` | `ApplyResult` | autosaves |
| `doc_preview` | `command` | `ApplyResult` | |
| `doc_undo`, `doc_redo` | `force?` | `DocState` | autosaves. In a live session someone else's step needs `force` (`other_author`) |
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
| `render_capture` | `camera`, `png`, `revision?`, `info?` | `RenderRecord` | Tier 1 capture or render. Tied to `revision` (the one a render started from, never newer than the document) or the current one. `info` says how it was made; width and height at least 1, seconds 0 or more |
| `render_data` | `id` | data URL | |
| `render_delete` | `id` | `null` | |
| `render_ai_settings_get` | | `RenderAiSettings` | |
| `render_ai_settings_set` | `api_key?`, `model?` | `RenderAiSettings` | `""` removes the key |
| `render_ai_generate` | `request` | `RenderAiResult` | 10 to 60 s; writes a `RenderRecord` with source `ai_visualization` and `source_render_id` |
| `ai_settings_get` | | `AiSettings` | |
| `ai_settings_set` | `api_key?`, `model?` | `AiSettings` | `""` removes the key |
| `ai_chat` | `request` | `AiTurn` | may hold one pending proposal |
| `ai_resolve` | `proposal_id`, `accept` | `AiResolveResult` | rejects with `stale` if revision moved |
| `presence_set` | `presence` | `null` | this window's pointer, selection, level, cursor chat and AI scope |
| `presence_list` | | `PresenceEntry[]` | everyone else's latest presence in the live session |
| `profile_get` | | `Profile` | |
| `profile_set` | `name` | `Profile` | 1 to 40 characters |
| `live_status` | | `LiveStatus` | |
| `live_host` | `port?` | `LiveStatus` | shares the open project; needs a profile name |
| `live_join` | `invite` | `DocState` | opens the shared project, closing the open one first |
| `live_leave` | | `LiveStatus` | host: ends the session for everyone; guest: leaves, the document closes |
| `live_remove` | `participant_id` | `LiveStatus` | host only |
| `live_save_copy` | | `ProjectMeta` | guest: the shared project as a new local project |
| `chat_send` | `text`, `at?`, `level_id?` | `ChatMessage` | live session only |
| `chat_list` | | `ChatMessage[]` | the open project's chat, oldest first, last 500 |
| `window_reply` | `id`, `reply?`, `error?` | `null` | the window's answer to a `WindowRequest` |

External changes: after every commit, undo, redo, open, create, close, delete and restore, a rename of the open project, and every document a live session guest receives, `AppService::watch_changes()` fires `{revision, project_id, seq}`. The desktop shell forwards it as the Tauri event `doc_changed {revision}`; the dev bridge serves `/mcp` on its port and the UI polls `doc_revision`. The frontend subscribes with `onDocChanged` (`src/contract/ipc.ts`): `App.tsx` switches hub to editor, `EditorShell` refetches state. Full MCP tool list: `docs/MCP.md`.

Errors are always `IpcError { code, message, element_ids }`. Codes: `not_found`, `invalid`, `no_document`, `stale`, `io`, `ai_not_configured`, `ai_failed`, `unknown_command`, `bad_args`, `forbidden` (dev bridge, non-localhost origin), `other_author` (undo or redo of someone else's step without `force`), `not_live`, `host_only`, `live_refused` (wrong secret, session full, other version), `live_unreachable` (no address answered), `live_pin` (the host's certificate does not match the invite), `live_lost` (the connection to the host dropped), `no_window` (no window answered a window request). An AI edit outside its scope has no IPC code of its own: the model reads a tool error that starts with `out_of_scope:`, and a proposal that stops fitting its scope before it is applied is `invalid`.

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
    chat.jsonl            # live session chat, one ChatMessage per line (host)
  live/<project-id>/      # a guest's folder for a shared project: renders, exports, AI log
  exports/
  trash/
  settings.json           # also profile_name, live_port
```

`data_dir` is the OS app data dir in the desktop app and `.devdata/` for the bridge.

## Dev bridge

`cargo run -p guhit-devbridge -- [--port 1430] [--data .devdata]`

- `POST /ipc/<cmd>` with the args object as JSON body. 200 + result JSON, or 400 + `IpcError`.
- `GET /health` -> `{"ok":true}`.
- `GET /events` -> server-sent events, one JSON `AppEvent` per message.
- CORS: allow any `http://localhost:*` origin. Binds 127.0.0.1 only.
- The UI picks the bridge URL from `VITE_BRIDGE_URL` (default `http://localhost:1430`). In development, `?bridge=http://localhost:<port>` points one tab at another bridge: two bridges with their own `--data` and two tabs make a live session on one computer.

## Frontend join points

| File | Export | Owner |
|---|---|---|
| `src/shell/EditorShell.tsx` | `EditorShell()` | shell. Mounts everything below. |
| `src/hub/ProjectHub.tsx` | `ProjectHub()` | shell |
| `src/editor2d/PlanCanvas.tsx` | `PlanCanvas()` fills its parent | 2D |
| `src/viewer3d/Viewer3D.tsx` | `Viewer3D()` fills its parent | 3D |
| `src/viewer3d/RenderPanel.tsx` | `RenderPanel()` fills its parent. Visuals gallery. | 3D |
| `src/ai/AiDock.tsx` | `AiDock()` fills its parent | AI |
| `src/live/` | live session state (`useLive`), presence sync, remote cursors, cursor chat, the Chat panel, avatars, share and join dialogs | live |
| `src/shell/windowTasks.ts` | answers `AppEvent::WindowRequest` (render, capture view, capture plan) | shell |

Rules:
- Read state with `useApp` selectors. Draw `useVisibleDoc()` so AI previews show as ghosts; elements in `preview.diff` are tinted with `--draw-preview`.
- Mutate only via `useApp.getState().dispatch(command)`.
- `Viewer3D` registers `captureView` and `exportScene`, `PlanCanvas` registers `capturePlan`.
- One-shot view requests go over `src/state/bus.ts`.
- Respect `project.layers` (visible, locked) and `activeLevelId`.
- Backend pushes (live session, presence, chat, window requests) arrive through `onAppEvent`; one connection serves every subscriber.
- `useApp().aiScope` is the "Only the selection" switch (D30): the copilot sends it as `AiRequest::scope`, and it rides this window's presence so MCP clients are held to it too.
- `useApp().undoConfirm` holds an undo or redo the engine refused as someone else's (`other_author`); the shell asks and calls `resolveUndoConfirm`.
