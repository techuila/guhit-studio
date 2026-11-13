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
- Stairs: a straight flight has `riser_count - 1` treads of `run_mm / (riser_count - 1)`, so treads times tread length is the run and the top riser lands on the floor above (DECISIONS D44). An L or U stair has `riser_count - 2` (D54, "Wave 2 contract"). The inspector, the IFC, the plan and the exports count this way. See "Slabs and railings". Annotations: `position` is the left end of the first baseline.
- The roof over the top floor is an element since schema 6: read it with `Project::top_roof()` / `topRoof(project)`, never `Project::roof` ("Wave 2 contract").
- An element's level is `Element::level_id()` / `elementLevel(el)`; cameras and tours are views (`is_view` / `isView`).
- Walls have a `role` (`WallRole`, schema 4): `building` (the default, and every wall of an older file; left out of the JSON) or `site`. See "Site walls".
- `Diff::also_moved`: walls a command reshaped that it did not name (joined walls that stretched or shifted to follow). Also in `modified`, named in `summary` ("; also moved: Wall 4150 mm, Wall 3000 mm"), left out of the JSON when empty. See "Moving walls".

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
  Schema 2 added the plumbing layers, schema 3 the others, schema 4 `site`;
  `guhit_core::migrate` adds missing layers when a `Document` is created.
- Colors: tokens `--pipe-cold`, `--pipe-hot`, `--pipe-drain`, `--pipe-vent`,
  `--pipe-storm`, `--pipe-conduit`, `--pipe-refrigerant`, `--pipe-condensate`
  in `src/styles/tokens.css`. The 3D view, 2D plan, legends and exports use
  the same four colors.
- Tool defaults (`defaults::pipe_defaults`, mirrored by the frontend). The
  start height of a new run on a real level is `defaults::pipe_start_height_mm`
  (TS `pipeStartHeightMm(system, level, levels)` in `src/contract/pipes.ts`):
  the table, except conduit, which starts 100 mm
  (`CONDUIT_ABOVE_CEILING_MM`) above the level's ceiling, that is its height
  or the underside of the next level's 200 mm slab when that is lower. So it
  sits inside the slab above, or above the ceiling on the top level
  (DECISIONS D40). The table's conduit value is that rule on a one storey
  level of the default 3000 mm.

| System | Material | Size mm | Start height mm | Size menu |
|---|---|---|---|---|
| cold_water | ppr | 20 | 300 | PPR 20 25 32 40 50 63, GI 15 20 25 32 50, PE 20 25 32 |
| hot_water | ppr | 20 | 300 | PPR 20 25 32, copper 15 22 28 |
| drainage | upvc | 50 | -300 | uPVC 32 50 75 100 150 |
| vent | upvc | 50 | 300 | uPVC 32 50 75 100 |
| storm | upvc | 100 | -300 | uPVC 75 100 150 |
| conduit | pvc | 20 | ceiling + 100 (3100) | PVC 20 25 32 40 50, EMT 15 20 25, IMC 20 25, flexible 15 20 |
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
  Snapping onto a fixture joins at its connection height
  (`fixtureConnectionZ`, DECISIONS D40), always with a vertical riser at the
  fixture, a drain included (never a slope to it); a run that starts at a
  fixture keeps the pen height and drops or climbs to it with a riser. With
  no height chosen the tool starts at `pipeStartHeightMm` for the active level.
- Editing on the plan (DECISIONS D39, `src/editor2d/pipeEdit.ts`): a pick is
  the whole run, a segment or a point (`usePipePick`). A point stands for its
  plan node: a plan move carries every point a riser joins, and so does a
  height edit of the point; a segment's height edit moves its own two points.
  Every edit sends one of the commands below with `keep_joins` true (Alt while
  dragging sends false); values out of the engine's range are sent so its
  refusal says why. The drag preview follows joins by the same rules as the
  engine (`followers`, a port of `rests` and `support`). The inspector's Start and End height of a run that
  falls is one `SetPipeSlope` over the whole run holding the other end; of
  any other run, a `MovePipePoints` of that end and the points level with it.

Pipe editing commands (DECISIONS D39, `crates/guhit-core/src/pipe_edit.rs`).
Each is one undo step and works inside a `Batch`. Points are numbered from 0
in `Pipe::points` order; segment n runs from point n to point n + 1. Lengths
are 3D. They set geometry only: no command sizes a pipe or judges a fall
(D19).

| Command | What changes | Label |
|---|---|---|
| `MovePipePoints { targets, delta, keep_joins }` | each target's points (empty `indices`: every point) move by `delta` (x, y, z) | "Move pipe point", "Move pipe points", "Move pipe" or "Move pipes" (whole runs) |
| `SetPipeSegmentLength { pipe_id, segment, length_mm, hold, keep_joins }` | `hold: start` keeps point n; point n + 1 and every point after it move along the segment by the change. `end` the other way round. The rest of the run keeps its shape and slopes | "Set pipe length" |
| `SetPipeSlope { pipe_id, from_segment, to_segment, slope_pct, hold, keep_joins }` | from the held end, each segment of the range falls `slope_pct` percent of its plan length (negative rises); a riser (the rule above) or a segment steeper than 100 percent (an offset, such as a drop into a trap) keeps its own rise. Points past the other end of the range move up or down by the same amount | "Set pipe slope" |
| `InsertPipePoint { pipe_id, segment, at }` | a new point `at` between points n and n + 1 | "Add bend" |
| `RemovePipePoint { pipe_id, index }` | the point goes; at least two stay | "Remove bend", or "Remove pipe end" for an end |

- `keep_joins`: run ends follow what they rest on, joined as `Derived::pipes`
  below joins them on the geometry before the edit (joinable systems only,
  within the larger radius and at least 1 mm, absolute heights). Each end has
  one support at most:
  - a tee branch (an end on another run's body, the nearest body, the earlier
    run on a tie) keeps its place along that segment and follows only the
    part of the move across it, height included, interpolated where the
    segment's two points move differently, and kept on the moved segment
    when it would fall off an end. Along is the plan direction, or height on
    a riser. So a longer main or a main slid along its own axis leaves its
    tees where they are, and a branch a quarter along a segment whose far end
    rose 400 mm rises 100 mm. A branch on a bend moves with the bend;
  - two run ends meeting (an elbow or coupling) follow each other;
  - where three or more ends meet, every end follows the end of the run the
    tee is on (the larger of the two that line up best, or the one that
    arrives), and that one follows nothing there.
  A branch never drags the run it rests on. The runs the command names move
  only as the command says, never as followers. What follows carries what
  rests on it in turn. Without `keep_joins` only the named runs change.
- Summaries: "Moved 2 pipe points 300 mm up, 1 joined run followed", "Set
  the length of segment 0 of Kitchen sink waste to 1500 mm", "Set 2 segments
  of Kitchen sink waste to fall 2 percent".
- Errors (`CoreError::Invalid`, naming the pipe): `unknown_pipe`,
  `not_a_pipe`, `no_pipe_points` (no targets), `duplicate_pipe` (a run listed
  twice), `bad_point_index`, `duplicate_point`, `bad_segment` (out of range,
  or `from_segment` after `to_segment`), `not_finite`, `no_move` (a zero
  `delta`), `pipe_segment_short` (under `MIN_PIPE_SEGMENT_MM`, 10 mm),
  `bad_slope` (beyond plus or minus 100 percent), `no_slope_segment` (every
  segment of the range is a riser), `bend_on_point` (`at` within 1 mm of
  point n or n + 1), `pipe_too_short` (removing from a two point run). Then
  the usual checks of every command: `pipe_zero_segment` when two points in a
  row end up under 1 mm apart, `layer_locked` when a run that would change,
  edited or following, is on a locked layer.

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
| `move_pipe_points`, `set_pipe_segment_length`, `set_pipe_slope`, `insert_pipe_point`, `remove_pipe_point` | every run the command names is in reach; joined runs that follow with `keep_joins` are a side effect, like walls stretching |
| `duplicate_elements` | every target is in reach, and every copy lands as `add_element` would; a door or window copied without its wall stays on that wall |
| `set_review_mark` | an element target in reach, or a finding whose elements are all in reach (a finding the checks no longer make is read from its id, `code:ids`); a whole check never |
| `set_roof`, `set_project_settings`, `update_level`, `add_level`, `delete_level`, `set_layer`, `upsert_material` | never: they change the whole project |
| `batch` | every command in it, each on the project as the steps before it leave it, with what they made in reach; a refusal starts "Step 2 of 3: " like the engine's batch errors |

A refusal is `CoreError::Invalid { code: "out_of_scope" }` naming the
element in plain words ("Wall 3000 mm is outside the selection this edit is
limited to.") with its id in `element_ids`. It reaches IPC as `IpcError {
code: "invalid" }` with the element ids, and the model as a tool error that
starts with `out_of_scope:`. Side effects are allowed: connected walls
stretching, joined pipe runs following (D39), dimensions following (D12),
links removed (D21), rooms appearing in closed faces (D7). A scope naming an element that is not in the plan, or
a command naming a target that is not, is refused with `not_found`. The area
follows the staged plan, so once a step deletes a selected element its area
is gone: a turn that replaces a selected element adds the new one first.

## Site walls

A lot fence or boundary wall is a wall with `role: "site"`
(`AddWall::role`, `AddWallChain::role`, both optional; None is a building
wall and keeps the command JSON, and so its ids, as before).

- Topology: site walls are left out of the building's wall graph. They never
  close a room (the yard is no room), never make a building wall interior,
  and never count in `floor_area_m2`, `gross_area_m2` or the footprints. They
  join each other for their mitred outlines and always read `exterior`. A room
  drawn along a fence gets its own wall on that side.
- Height: a site wall drawn without one is `SITE_WALL_HEIGHT_MM` (1800 mm).
- Layer: `LayerKey::Site` (label "Site"), after `aircon`, visible and locked
  like the others. `element_layer` and the frontend `layerOf` put site walls
  there; a locked site layer refuses new site walls only.
- Review: no `wall_dangling` for a site wall, and a near miss counts only
  between walls of one role.
- Plan exports: category `Cat::Site`, SVG group `site-walls`, DXF layer
  `L-SITE` (written only when there are site walls, so other plans give the
  same file). 3D DXF: faces on `L-SITE`. IFC: `IfcWall` with ObjectType
  `'BOUNDARYWALL'` and PredefinedType `.USERDEFINED.`, named "Site wall N",
  `IsExternal` true.
- 2D draws them at half opacity; the wall tool flyout has "Building wall"
  and "Fence or site wall" (`ToolOptions::wallRole`).
- MCP and copilot: `role` "building" or "site" on `add_wall` and
  `add_wall_chain`; queries list a wall's `role`.

## Moving walls

`move_elements` with `stretch_connected`, `resize_room` and `set_wall_length`
move walls rigidly, and joined walls follow:

- A wall joined at a moved wall's end, at an angle, slides its end to the
  joint's new place.
- A wall resting on a moved wall's body (a T) slides along its own line onto
  the moved wall, so it stays straight.
- A wall that continues the moved wall in a straight line stays put when a
  cross wall at the joint, by its end or its body, keeps the joint on its line
  (R2: a lanai wall below the south wall no longer turns diagonal when the
  partition above it moves).
- A follower that was square or parallel to the wall it follows (within 1
  degree) must keep its direction. When it would turn, the move is refused
  with `stretch_turns_wall`, naming the walls: "This move would turn Wall
  1800 mm out of line: its end at the joint would move 500 mm sideways.
  Select it too so it moves along, or move without stretching connected
  walls." Diagonal walls follow as before.
- `set_wall_endpoints` (dragging a wall end) is not rigid: its followers
  still follow the end wherever it goes.

Every result names the walls that followed: `Diff::also_moved` and the
summary sentence; MCP edit results and copilot steps add `also_moved` with
each wall's id, label and new ends; the window shows an "Also moved: ..."
toast.

## Slabs and railings

A `Slab` is a floor, balcony, deck or void on one level (DECISIONS D50),
added with `AddElement` and edited with `UpdateElement`, on the Slabs layer
(`LayerKey::Slabs`, label "Slabs", after `site`). Schema 5 added slabs, the
layer and `Stair::railing`; `migrate` gives older files the layer, and an
older stair has no railing.

- Outline: the engine stores it counter-clockwise (a clockwise outline is
  reversed), without points closer than 1 mm to the one before and without a
  closing duplicate. Fewer than three points, a self-intersecting outline or
  one under 0.01 m2 is refused with `slab_outline`.
- Kinds: `floor` adds floor where the level's own floor does not reach (an
  extension, a mezzanine). `balcony` and `deck` are open floors outside the
  walls. `void` cuts the floor away so the space below is double height.
- Height: a slab's top is the level elevation plus `top_offset_mm`, and it is
  `thickness_mm` deep below that. A void has no thickness: it cuts every
  floor of its level inside its outline, which is the level's own floor (one
  slab per footprint), its floor, balcony and deck slabs, and the roof deck of
  the level below where that deck is under it (D26).
- Overlaps are refused, never merged: on one level, floor, balcony and deck
  slabs may not overlap one another, and voids may not overlap voids
  (`slab_overlap`, naming both slabs and the shared area, `element_ids` the
  slab being added or changed first). Shared edges and corners are fine; a
  void over a floor, balcony or deck is what a void is for.
- A void must lie over floor: inside its level's footprints, its level's
  floor, balcony and deck slabs, and the footprints of the level below (whose
  roof deck is the floor there). Otherwise `void_outside_floor`, saying how
  much hangs outside. A void on the lowest level would open onto the ground
  and is refused (`void_no_level_below`). Checked when the void is added or
  changed. The plan's slab tool starts on `floor`.
- Every area the engine reports uses these numbers: `Derived`, the plan
  labels, and the `room_list`, `project_summary`, `describe` and
  `rooms_without_exterior_window` queries (a room's `net_area_m2` less the
  void, with `void_area_m2` and `floor_area_m2` when slabs change them; a
  level's `net_floor_area_m2` and `gross_footprint_area_m2` as the totals
  count them).
- Areas: `RoomGeometry::area_mm2` is the net room area less the part a void
  covers. `Totals::floor_area_m2` sums the rooms less the part a balcony or
  deck covers (an open lanai walled in on a deck is a room, not floor area).
  `Totals::gross_area_m2` sums the footprints, plus floor slabs where they
  reach past the footprints. Gross area counts the walls: a void, balcony
  or deck takes off only its clear part, inside the wall faces or outside
  the building (a loggia or lanai inside the walls is open floor, not gross
  area). A stair opening is not taken off: a stair counts on every floor it
  serves.
- `top_offset_mm` is from the level's floor and must stay under the level
  height either way (`slab_top_offset`); a void ignores it.

### Railings (`Derived::railings`)

The engine works out every rail; the plan, the 3D view and the exports draw
`Derived::railings` (`RailingRun`) and never find open edges themselves.

- Open edge: a stretch of an outline that no wall of the same level runs
  along. A wall runs along an edge when it is within 2 degrees of parallel
  and its centerline is within half its thickness plus 50 mm of the edge
  line; it closes the overlap of the two. Site walls count. Open stretches
  under 100 mm are dropped, and stretches that meet at a corner are one run.
  A run all the way round repeats its first point at the end.
- A floor, balcony or deck edge is also closed where another floor, balcony
  or deck of the level runs along it (within 50 mm): an L drawn as two
  rectangles has no rail across its walking surface.
- A void edge is also closed, for each stair arriving on the level: across
  the stair's arrival end (within 50 mm, where people step off), across its
  opening (a rail there would hang over the flight), and along the opening's
  rim where the opening lies outside the void (two openings side by side).
  Where the void holds the opening, its edge along the rim keeps its rail.
- The `slab_no_railing` review measures the same open stretches.
- `slab_edge`: a slab with a `railing` gets one run per open stretch, its
  foot at `top_offset_mm` (0 for a void).
- `stair_side`: a stair with a `railing` gets a run along each open side of
  its flight (the lines `width_mm / 2` either side of the center), its foot on
  the nosing line: one riser up at the first nosing (`level height /
  riser_count`), the level height at the top end.
- `stair_well`: the level a stair arrives at is the level above whose
  elevation is within 300 mm of the stair's top (its level's elevation plus
  height), the closest one, as the 3D view finds it. There the flight's
  rectangle is the opening; a stair with a `railing` gets a guard along the
  opening's two sides and its bottom end (over the first riser), open parts
  only, foot at 0. The end the stair arrives at stays open. Parts inside or
  within 50 mm of a void on that level go: the void's own rail guards there.

### Stairs on the plan (D44)

A straight flight of `n` risers has `n - 1` treads, each `run_mm / (n - 1)`
deep: tread lines at `k * run_mm / (n - 1)` for `k = 1 .. n - 2` inside the
outline, the last tread ending at the top of the flight where the top riser
lands on the floor above. The UP arrow runs from the first tread to the top.
On the level the stair arrives at, the flight's outline and tread lines are
dashed with a DN arrow from the top end down the flight, and the rooms there
do not paint over the opening. Railings draw over both.

### Plan and exports

- Plan: floor, balcony and deck outlines in a thin line with a light tint
  (balconies and decks a little stronger); a void as its outline with a cross
  (the two diagonals of a four-corner outline, otherwise the diagonals of its
  bounding box clipped to it) and "OPEN TO BELOW". A railing is two thin lines
  50 mm apart along the run (bars, glass), a solid parapet a 100 mm band.
- The level below as an underlay (UI only, never exported): its walls in a
  faint tone under everything on the level, on by default, toggled per
  project (`guhit.planUnderlay.<project id>` in local storage).
- SVG groups `slabs` and `railings`; DXF layers `A-FLOR-OTLN` (slab outlines
  and the void cross) and `A-FLOR-HRAL` (railings), a stair from below on
  `A-FLOR-STRS` dashed. IFC: `IfcSlab` `.FLOOR.` for a floor slab, balconies
  and decks `.USERDEFINED.` with ObjectType `'BALCONY'` or `'DECK'`, names
  numbered across the file ("Floor slab 2", "Balcony 1"). An IFC4 opening
  voids one element, so a void is one `IfcOpeningElement` per slab of its
  level it overlaps ("Void 1 in Floor slab 2", each with its own
  `IfcRelVoidsElement`); a void over nothing the file holds (the roof deck
  below, which the IFC does not write) is left out. Railings are `IfcRailing`
  (`.GUARDRAIL.` for slab edges and stair wells, `.HANDRAIL.` for stair
  sides). The 3D DXF puts slabs on `A-FLOR-SLAB` with voids cut out and
  railings on `A-FLOR-HRAL`. GLB, OBJ and DAE come from the 3D view, which
  draws the same slabs and runs.

### Review

| Code | Severity | On | When |
|---|---|---|---|
| `slab_no_railing` | warning | slab | a balcony or deck whose top is 750 mm or more above the lowest level's floor, or a void above the lowest level, has open edges and no railing |
| `stair_top_no_floor` | warning | stair | the floor 300 mm past the top of the flight is missing: no level at the stair's top, or that spot is outside the level's floor (footprints, floor, balcony and deck slabs, the roof deck of the stair's own level) or inside a void |
| `stair_landing_blocked` | warning | stair, wall | a wall stands in the landing past the top step on the level the stair arrives at (as deep as the stair is wide, 900 mm at the least: the dead-end landing), or within 900 mm in front of the first step; it says how far |
| `asset_over_opening` | warning | object, void or stair | an object standing on the floor (furniture, bathroom, kitchen, appliance, plant, vehicle) has its center over a void or over the opening of a stair arriving on its level |
| `door_swing_blocked` | warning | door, object | an object standing on the floor overlaps the quarter circle a swing door's leaf sweeps by more than 0.02 m2 |

Landing depths and clearances are rules of thumb for the designer, never a
code requirement (D24).

### Engine and queries

- `guhit_core::slabs` holds the rules: `check_slab` (run after every command
  for the slabs it added or changed, on the analysis after it), `normalize`,
  `railings`, `review`, `stair_top_level`, `stair_rect`, and exact region
  areas (`Region`, `union_area`, `covered_area`, `overlap_area`).
- `describe` gives a stair `tread_count` (`riser_count - 1`), `tread_mm`,
  `riser_mm`, `arrives_at` (the level name, or null) and `railing`; a slab
  `slab_kind`, `polygon`, `thickness_mm`, `top_offset_mm`, `area_m2`,
  `railing_mm` and `railing_style`.
- Undo labels and summaries name the kind: "Add balcony", "Add void",
  "2 balconies".


## Sheets

`PlanExportOptions::sheet` (default `plan`) picks the sheet: `lighting`,
`power` (with a schedule of loads whose rating columns stay blank for the
PEE), `plumbing` (with a fixture table), `plumbing_isometric` (water and
sanitary diagrams, not to scale, legend box, a blank Master Plumber block),
`aircon`. `review_page` adds a PDF page of review items and notes.
`PlanExportOptions::date` is the title block DATE ("YYYY-MM-DD"); `export_plan`
fills in today on the computer's own calendar (`guhit_app::local_date`) when
it is None, and the export crate, which has no clock, falls back to the date
of the project's last change. DXF writes
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
  pack HDRI, turned so its sun sits at the computed azimuth). `photo` is the
  default: a tropical midday sky, 4K behind the model and 2K for light, with
  its sun taken out (the directional light is the sun) and its sky light
  scaled to the clear sky's. The ground is a lawn texture that fades into the
  photo's own ground.
- Tone mapping: AgX, for the live view, captures and renders alike.
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
- Ambient occlusion (light/ao.ts): every live frame and capture, inside the
  frame's one render call, a half resolution pass over the opaque model dims
  indirect light in corners and under objects (and direct light a little).
  Off in the X-ray and hidden shells. Refine samples reuse the frame's.
- Glass: thin panes that let the view through and reflect by Fresnel in the
  live view; real transmissive glass in the path tracer.

## Render

`bus.emit("render", { views })` renders the current view, every saved view,
or chosen cameras with a path tracer (`three-gpu-pathtracer`, WebGL 2, lazy
loaded) in its own offscreen renderer, so the live view stays usable. Sizes: HD
1920 x 1080 (default), QHD 2560 x 1440, 4K 3840 x 2160, square 2048 x 2048.
Quality: quick or final (`TraceQuality`), set by time. The result is
denoised, saved as a `RenderRecord` (`render_capture`) with the camera and
light it used, the revision the render started from, and `RenderRecord::info`
(`RenderInfo`: kind, size, samples, seconds, quality, graphics adapter, and
`visibility`, the roof and cutaway it showed), and
offered to "Visualize with AI" (D17). A capture of the 3D view saves
`RenderInfo` too, kind `capture`, with its size and visibility.
`visibility` is left out of the JSON when unknown (older records, tour
videos, shadow studies); the `capture_maps` task draws a record's maps with
it, else with the model's defaults. The job never calls
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
  While not `orbit` the 3D view owns every key without MOD; Escape clears
  the selection, then two Escapes within 2 s return to orbit. A walk starts
  in the largest room of the level, on the spot with the longest open view,
  looking along it (`geom/walkStart.ts`).
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
  as a ramp and switches level at the landing; its first and last treads are
  open at the sides where a floor is beside them (`walk/stairs.ts`). Door leaves swing open as you
  approach and close behind you (view only). Mouse-only: drag to look, scroll
  or two-finger swipe to move.
- The global shortcut handler defers keys to the 3D view only while that view
  is on screen. Switching to plan only ends a walk (`nav` back to `orbit`).
- The shell sends `walk_to` or sets `nav` only once the 3D view is up; the
  signal is `useApp().captureView` being registered. The 3D view resets `nav`
  to `orbit` when it truly unmounts.

## Live sessions

DECISIONS D29 and D32. Types: `crates/guhit-model/src/live.rs`. Code:
`crates/guhit-app/src/live/`. The relay: `docs/RELAY.md`.

- One computer hosts the open project (`live_host`). Its `Document` is the
  only authority. A guest (`live_join`) keeps a read-only copy: the host's
  project, revision and undo labels, with `Derived` computed locally by the
  same engine. Every change a guest makes is a typed `Command` sent to the
  host, validated and applied there, and the new state goes to everyone.
- Transport: TLS 1.3 (rustls, ring), over TCP straight to the host or over
  a WebSocket pair through the relay (`live/relay.rs`, protocol in
  `docs/RELAY.md`). The TLS runs end to end either way, so the relay only
  forwards encrypted bytes. The host makes a self-signed certificate for
  each session. Frames are a 4-byte big-endian length and a UTF-8 JSON
  message; at most 64 KiB before the guest is authenticated, 48 MiB after.
  A message over 16 KiB is deflated when that makes it smaller, marked by
  the high bit of the length; the limits count the JSON, and inflating
  stops past them. The frame messages are internal to `guhit-app` (host
  and guest are the same code, `live/wire.rs`, `PROTOCOL_VERSION` 2; a
  hello with another version is refused; additions an older build can
  ignore, with serde defaults, keep the version): guest to host `hello`,
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
  the change, the host's change counter, which orders the frames, and its
  `history`, below) after every change of its document, whoever made it,
  and before the reply to the guest edit that made it. So a guest's `doc_apply`,
  `doc_undo` and `doc_redo` return its copy at the new revision or later
  (with the host's diff for `doc_apply`). Renaming the open project also
  notifies watchers, so guests get the new name.
- Underlays and reference models travel in pieces of 2 MiB, so the 50 MB
  model limit fits under the frame limit. The host stores them with the
  same name, type and size checks as the window's own uploads.
- Invite: `guhit-live:` then base64url (no padding) of
  `{"v":3,"secret":...,"pin":...,"sealed":...,"project":"Bungalow","relay":{"url":"wss://relay.example.com","room":...}}`.
  `secret` is 128 random bits. `pin` is the SHA-256 of the certificate
  (base64url). `relay` is there when the host uses one: its address and the
  session's room, 128 random bits. The key that proves the room is the
  host's (256 bits) goes only to the relay, never into the invite.
  `sealed` holds the host's direct addresses, so the invite text never
  shows them: base64url of a 12-byte random nonce, then the
  ChaCha20-Poly1305 ciphertext and tag of the JSON array of addresses
  (`["192.168.1.20:1460",...]`), keyed by HKDF-SHA256 over the 16 secret
  bytes (no salt, info `guhit-live invite addrs v3`), with the 32 pin bytes
  as associated data, so a seal moved to another invite does not open. A
  host that takes no direct connections leaves `sealed` out, which needs a
  relay. A seal that does not open makes the invite `bad_args` ("This is
  not a Guhit live session invite."); a plain `addrs` in version 3 is
  ignored. The guest's app opens the seal in memory to connect; no
  screen, notice or error shows a guest the host's address
  (`LiveStatus::addresses` is empty for a guest). The key comes from the
  invite, so the seal keeps addresses out of sight, not away from someone
  who decodes the invite on purpose; a direct connection's peer address is
  also visible to the operating system's network tools. A host with
  `live_direct: false` never gives guests its address at all. Versions 1
  (no relay) and 2 (plain `addrs`) are still read; above 3 is "from a
  newer version". A guest accepts only the pinned certificate (no CA, no
  host name check). It tries every address at once, 4 s each, and the
  relay after 600 ms (at once when every address has failed), and keeps
  the first connection whose TLS handshake passes the pin; the others are
  dropped.
  The host compares the secret in constant time, answers a wrong secret
  after a 1 s delay and keeps at most 8 connections waiting to
  authenticate, direct and relayed together.
- Listening: the desktop app on every IPv4 interface, port `live_port` in
  `settings.json` (default 1460, else the next free one of the nine after
  it, else one the system picks), only while a session runs. A `port`
  given to `live_host` must be free (`invalid` otherwise). The dev bridge
  on 127.0.0.1 only. `LiveStatus::addresses` lists every IPv4 address of
  the computer: the one the default route leaves from first (found without
  sending anything), then private networks, then 100.64.0.0/10 (Tailscale
  and other VPNs), then the rest, at most 7, and 127.0.0.1 last; loopback
  and link-local addresses are skipped. `live_direct: false` in
  `settings.json` switches listening off: no listener and no addresses, so
  guests come only through the relay.
- Relay (DECISIONS D32): `live_relay` in `settings.json` is the relay's
  base URL: `wss://`, or `ws://` for a loopback host; no user info, query
  or fragment; a path prefix is allowed. Without the key, the build's
  `GUHIT_RELAY_URL` (compile time) if it was set. An empty string means no
  relay. `live_host` fails with `invalid` for an address that does not pass,
  and for `live_direct: false` without a relay (no one could join). A host
  with a relay registers the session's room there and keeps it registered:
  when the connection drops it tries again after 1, 2, 4, 8, 16 and then
  every 30 s, with the same room and key, so the invite keeps working.
  `LiveStatus::relay` says where that stands: `off` (no relay, or not
  hosting), `connecting` until the first attempt ends, `ready`, or
  `unavailable` while it retries. `live_host` does not wait for it.
- Names: `profile_get`/`profile_set` (`settings.json` key `profile_name`,
  1 to 40 characters, control characters removed). Hosting and joining need
  one (`bad_args` otherwise). Colors: the host is 0, each guest gets the
  lowest color no one in the session has, the least used one once all
  eight are taken; at most 16 participants (`live_refused` beyond). The
  host's participant id stays the same for every session while the app
  runs, so its steps stay its own.
- Why a join failed goes into `LiveStatus::notice` too. `live_unreachable`
  says why nothing answered: whether the host was tried directly, and
  through the relay whether the host is not online (the session may have
  ended), did not answer, the relay is busy, or the relay could not be
  reached. Neither it nor `live_pin` names an address.
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

History in a live session: every commit records its author and the time
on the app's clock (`Document::apply_at`), `DocState::undo_by` and
`redo_by` say whose step is on top. `doc_undo` and `doc_redo` take back or
bring back the step on top of the one shared history, or with `steps` that
many as one revision and one document frame (a guest sends them to the
host as one request). When any of those steps is someone else's (another
participant's, or on a guest a step made before the session) and `force`
is not true, they fail with `other_author` and a sentence naming the
person and the step ("Ana made the last change: Move wall. Undo it
anyway?"), or for several steps everyone and how many are theirs ("Takes
back 4 steps, 2 by Ana. Undo anyway?", "Takes back 5 steps, 1 by Cy and 2
by Ben.", "Brings back 2 steps, all by Ben. Redo anyway?"); the window asks
and calls again with `force` and `expected_revision`, the revision the
question was asked at, so an edit made while it was open is never taken
back unasked: the answer then fails with `stale` ("the plan moved on from
revision 7 to 8, so nothing was undone") and the window says so. A guest
sends `expected_revision` to the host with the request (the wire field
has a serde default; an older host ignores it). More steps than there are
fail with `invalid` ("Only 3 steps can be undone, not 4.") without asking. On the
host, steps with no author (made before the session) are its own.

Edit history (DECISIONS D38): `history_list` returns `HistoryList`, the
undo and redo stacks newest first (`undo[0]` is what an undo takes back,
`redo[0]` what a redo brings back), each a `HistoryStep`: label, summary
(`Diff::summary`), origin (`ai` for the copilot and MCP clients), author,
time (RFC 3339, second precision; empty for a step an older host made),
`ids` (added, then changed) and `removed`, at most 200 each
(`guhit_core::MAX_STEP_IDS`; a step that touched more keeps the first). At
most 200 steps (`guhit_core::MAX_HISTORY`), in memory only: they go when
the project closes or a version is restored, and Versions stay the lasting
record. A step with no `author_id` is this computer's own, in a session
or not: the host's own steps have none, and on a guest neither do the
guest's. Other steps carry the participant id with `author_name` and
`color` as they were when the step was made, kept after the person leaves
and into the next session while the app runs (the host remembers
everyone who joined, as chat messages keep `author_name`; a guest that
never met someone keeps the name and color the host sent). Guests get the
host's history in every document frame and in the welcome
(`DocFrame::history`), with the host's steps from before the session
named as the host's; a guest's `history_list` answers from the last
frame. An older host's frames carry no history: a guest then lists only
the top undo and redo step from the undo meta, without time or elements,
and refuses `steps` over 1 with `invalid` instead of asking that host for
one step.

Presence: `presence_set` stores this window's `Presence`. MCP
`get_selection` reads it. In a session it goes to the others at most 20
times a second (the host sends batches every 50 ms, latest wins); `typing`
is cut to 160 characters and `selection` to 2000 ids, and ids longer than
64 characters are dropped. Everyone else's
arrives as `AppEvent::Presence`; `presence_list` returns the latest of
each, for a window that loads mid-session. A participant who leaves gets
`presence: None`.

Follow mode (DECISIONS D37). Presence fields, all `Option` with
`#[serde(default)]`, so a field an older version leaves out reads as null:
`following` (the participant this window follows), `view_mode` (`"2d"`,
`"3d"` or `"split"`, the store's `ViewMode`), `plan_view` (`PlanView`: the
center and size in mm of the plan area on screen, so a window of another
shape shows the same area), `camera` (`LiveCamera`: position and target in
mm with z up from project zero like `Camera`, `fov_deg`, and `nav` orbit,
walk or fly; walking, the target is 3 m ahead of the eyes) and
`pointer_3d` (the model point under the pointer in 3D, null off the
model). `clean_presence` drops an id that is not an id, a value that is
not finite and an area with no size, rounds to 1 mm and keeps `fov_deg` in
5 to 110. The window (`src/live/presenceSync.ts`) rounds the same way and
compares every field, so a camera-only change still goes out.
- While live, a window always sends `following`, `view_mode` and
  `pointer_3d`. It sends `plan_view` and `camera` only while some other
  participant's `following` is its id, and each only while that view is on
  screen (no `plan_view` in `"3d"`, no `camera` in `"2d"`); the camera from
  `ViewerEngine.liveCamera()` once, then `onCameraChange`.
- Clicking an avatar in the top bar follows that person (`src/live/follow.ts`);
  clicking it again, Stop or Esc stops. The follower takes their level and
  view mode, eases the plan (`PlanController.followView`) and the camera
  (`ViewerEngine.follow`) there over `--dur-scene`, then glides linearly over
  `--dur-press` x 0.75 per update; a resized plan fits the same area again.
  The follower stays in orbit (`endFollow` when it stops).
- Following stops on: Esc (before walk mode and the plan see it, unless a
  dialog or text field has the key), Stop, the avatar again, `user_view`
  from either view, `zoom_to_fit`, `focus_elements`, `focus_point`,
  `apply_camera` and `walk_to` asked for in this window, a level or view
  mode change that is not the followed person's, starting a walk or flight,
  leaving the editor, and with a toast when the person followed leaves or
  the session ends (reconnecting keeps it). Pressing either view is a
  `user_view`. Hover, pointer moves and a selection made off the canvas do
  not stop it.
- No loops: following someone whose chain of `following` comes back to you
  is refused with a toast ("Ana is following you already."). When two
  people click each other at once, the one with the lowest participant id
  in the loop stops, the same on every computer.
- A version before follow mode never sends `view_mode`, so a peer whose
  presence has none while live cannot be followed: the click is refused
  with a toast ("Ana's version of Guhit cannot be followed. Ask Ana to
  update."), and a follow started before their first presence arrived
  stops with the same line. Their avatar's tooltip says so too.

Chat: `chat_send` (live only, `not_live` otherwise). Text is trimmed, with
control characters other than line breaks and tabs removed, 1 to 2000
characters, or 160 with `at` (cursor chat); `bad_args` otherwise, and for
an `at` that is not a number or a `level_id` that is not an id. The host
stamps the id,
author, color and time, appends the message to `<project>/chat.jsonl` on the
host (one JSON object per line, best effort; once the file passes 2 MB it
is cut to its newest messages, at most 1000 in at most 1 MB) and sends `AppEvent::Chat` to
every window, the sender's included. `chat_list` returns the last 500 of
the open project's chat, oldest first: from `chat.jsonl` on the host or
with no session, from what the host sent on join plus what came since on a
guest.

## Agents

DECISIONS D42, D47, D50. Types: `crates/guhit-model/src/live.rs`
(`AgentInfo`, `AgentActivity`, `AgentLimit`, `AgentClaim`, `AgentSummary`).
Registry and rules: `crates/guhit-app/src/agents.rs`; limits, claims and
the work area: `guhit_core::scope` (`check_limit`, `claim_reach`,
`work_area`). Every MCP agent is a participant with a name, color,
presence and status line, with or without a live session. Its edits apply
directly, each one undo step with the agent as author.

Identity, per tool call, from the hint the MCP server passes
(`AgentHint`): the connection (`connection`, a key the server issues: its
MCP session id, else the labelled URL, else the stdio instance; without one
the stdio instance `x-guhit-agent-instance`, else the label of
`/mcp/agent/<label>` or `--agent <label>`, else `clientInfo`), and the
`agent` tool argument, a handle inside one connection (how subagents are
told apart).
- The id is `agent-` and 16 hex digits of a hash of this computer's salt
  (new each run), the connection key and the handle. It holds while the
  app runs; two computers never share one.
- Pause, stop, the limit and the rate limit belong to the connection, not
  the handle: every handle on it, the ones to come included, shares them,
  so a new `agent` handle never gets out of them. A new connection with
  the same client and label as an older one (a stdio proxy started again)
  starts the way the newest such connection is: paused, stopped or
  limited.
- The name is the handle or label as words ("ground-floor" is "Ground
  floor"), else the client's ("claude-code" is "Claude Code", "codex"
  "Codex"), else "Unknown agent". A name another agent on the list has
  gets " 2", " 3". A rename (`agent_update`) is kept in settings.json under
  `agent_names`, by what the agent was named for (`agent:<handle>`,
  `label:<label>`, `client:<name>`).
- Colors are the peer colors from 1 up, the lowest no agent on the list
  has. `role` is always `guest`, so code finding the host by role never
  finds an agent.
- Idle after 60 s without a call; off the list after 10 minutes without
  one (refused calls count, so a stopped agent that keeps trying stays
  shown); remembered for the history while the app runs, its connection's
  pause, stop and limit kept. At most 8 per computer take a place
  (`too_many_agents`): a stopped agent takes none, so stopping one frees a
  place at once, and resuming one needs a free place (resuming everyone
  brings everyone back).

Presence (`AgentSummary::presence`), set by the app, never sent by the
agent: after an edit, `cursor` at the middle of what it added and changed
on the level most of it stands on, `level_id` that level, `selection` those
elements (at most 200), `pointer_3d` 1200 mm above that level's floor,
`plan_view` the box around them plus 1500 mm, at least 8 x 6 m; after a
read, the same for what it looked at (`agent_done`). `status` is the line
it set (held 60 s), else its tool's ("Placing doors"), none once idle.
`activity`: `working` (its last call edited), `looking` (it read),
`idle`, `paused`, `stopped`.

Rules, enforced in the app and the engine, not in prompts:
- Pause (`agent_pause`): its edits, undo and redo fail `agent_paused`
  ("Axl paused Ground floor. Stop and tell the user; do not retry until
  they resume you."); reads and chat work. Stop (`agent_stop`): every call
  fails `agent_stopped`, and its claim ends. `agent_resume` ends either.
  Each takes `id` (that agent's whole connection), or null for every agent
  of this computer and every agent that connects until everyone is
  resumed. A pause never loosens a stop. A pause that lands while a call
  runs stops its commit. "Allow agents" (D35) stays the switch for all of
  MCP.
- One project: a call remembers the project open when it started
  (`AgentCall::project_id`), and its edit, undo or claim is refused
  (`project_changed`, nothing applied) once another project is open, even
  at the same revision; a translation that ran while the project changed
  is never committed onto the new one. When another project opens, limits
  and claims keep only its levels and elements (levels are also dropped
  when one is deleted). A limit left naming no level and no element of the
  open project would limit the agent to nothing, and dropping it would let
  the agent work unlimited on a project nobody limited it on, so its
  connection is paused instead and the level and element parts cleared
  (kinds kept, they hold in any project): the panel shows it paused until
  the user resumes it or sets a new limit. A claim left empty ends.
- Limit (`agent_update { limit }`): levels, elements and kinds, checked on
  every command of the edit by `scope::check_limit`, step by step through
  a batch. Levels: every element it targets, and every new element, on
  those levels. Elements: D30's reach and area. Kinds: every target and
  new element of those kinds (a room resize needs walls). Under a level or
  kind limit roof, levels, layers, settings and materials are refused;
  cameras never are. A refusal is `out_of_scope` naming the element: "The
  new wall would be on Second Floor, outside what this agent is limited
  to. Axl limited Ground floor to Ground Floor. Change only those, or ask
  the user to widen the limit in the Agents panel." "Only the selection"
  (D30) binds every agent on top of its limit.
- Claim (`agent_claim`): levels (ids or names) and elements, a lease that
  ends 10 minutes after the claimer's last call, or on release. Another
  agent's edit that reaches it (`scope::claim_reach`: anything on a
  claimed level, a new element landing on one, changing or deleting a
  claimed level, a claimed element with D30's reach) fails `claimed`:
  "Second Floor is claimed by Upper floor (Claude Code) until 3:52 PM. Work
  somewhere else, or ask Upper floor in the chat to release it." A claim
  that meets another agent's by reach (`scope::claims_overlap`: a level
  both claim, an element on a level the other claims, either way round, or
  an element both reach) fails the same way. Claims never block
  people. A guest's agents see the session's claims and the host checks
  them again.
- One lock: `agent_edit` translates the call once on a copy outside the
  session lock; when the plan moved on before the commit it translates
  again under the lock, so an agent never fails `stale` because someone
  else edited. On a guest the host commits it, and a `stale` from the host
  translates once more.
- Undo (`agent_undo`): an agent undoes or redoes only its own top step.
  Another agent's step is `other_author` always; a person's is
  `other_author` unless `force` (after the user confirms, D29). In a live
  session your own agent's steps never ask you; another person's agent's
  ask as D29.
- At most 5 commits a second per connection, every handle on it together (`rate_limited`).
- The first agent edit in a project after 10 minutes without one saves an
  automatic version first, "Before agents, 3:42 PM" (local time), so work
  past the 200-step history stays restorable. The host does it for guests'
  agents.
- History: `HistoryStep::author_id` is the agent; `on_behalf_of` and
  `on_behalf_name` the person it worked for, always set on an agent's step,
  with or without a session: this computer's own person is its host
  participant id (the same for every session while the app runs) and the
  profile name, or "this computer" before a name is picked. A step with
  `author_id` None is a person's.
- Chat (`agent_chat`): posted as the agent (its id, name, color,
  `via_ai`). Without a live session it goes to the project's `chat.jsonl`
  and `AppEvent::Chat`, so the Chat panel shows it.
- Notes (`agent_notes`, r5 3.4), for the MCP server to put in every result
  so agents learn of each other without polling: `activity` (whether it
  may edit: a paused agent reads that it is paused), `inbox` (chat that
  mentions it, "@Ground floor" or "@ground-floor", since its call before,
  at most 3) and `nearby` (other people's and agents' steps inside its
  claim since then, at most 5: who, the undo label, when, the ids), each
  told once. Works with or without a live session.

The window: `AppEvent::Agents { agents }` with the full list, newest call
first, on every change (each call, an agent going idle or off the list, a
claim ending; looked for once a second); `agents_list`; `McpStatus::agents`.
In a live session the list holds this computer's agents and everyone
else's; only the owner's computer can rename, limit, pause or stop an
agent (`not_yours` elsewhere).

Live sessions (`PROTOCOL_VERSION` stays 2): the hello and the welcome list
`features`; "agents" switches on `ToHost::Agents` (a guest's agents, sent
on every change once the welcome listed it), `ToGuest::Agents` (every
agent in the session but that guest's own, sent only to a guest whose
hello listed it), and `agent` on an apply or a chat. Agents are never in
`participants` and outside `MAX_PARTICIPANTS`. The host takes at most 8
agents per guest, forces their owner to that guest, keeps only `agent-`
ids no one else has, and records a guest's agent as the author of its
apply. An older guest is sent no agents, and its history still names agent
steps; an older host is sent none, and records a guest's agent's steps as
the guest's.

## MCP v2

The MCP server (`crates/guhit-mcp`, docs/MCP.md) is the main way agents
build the model (DECISIONS D42). It is a transport over `AppService`, like
the Tauri shell and the dev bridge, and adds no model rules of its own.

**Tools.** The core toolset `draft` (27 tools, Wave 2 added `add_grid`,
`annotate`, `add_roof` and `types`, swarms `swarm`) is always listed; the toolsets
`services`, `visuals`, `live`, `review`, `interop`, `documents` and `tours`
(19 more) are listed
when the request URL says `?toolsets=a,b` (or `all`), else when settings.json
`mcp_toolsets` names them. Listing only: every tool answers whether listed or
not. `guhit_mcp::toolsets` holds the lists; `guhit_mcp::tool_list_for` the
definitions. Every schema takes an optional `agent`; every editing tool takes
`scope` (AI edit scope above).

**Edits.** Every editing tool, `batch` step and `build` entry is translated by
`guhit_mcp::translate::plan` into typed `Command`s against the project as
staged so far (unknown fields refused), staged on one `guhit_core::Staging`
and committed as ONE `Command::Batch` with `Origin::Ai` and the label
`MCP: ...` (`guhit_mcp::stage::run`). An agent's call commits through
`AppService::agent_edit` (its author, limit and claims); an in-process call
through `commit_staged_if_revision`. `dry_run` stages and commits nothing. v1
editing tools translate through the copilot (`guhit_app::ai::tools`) as
before. A step whose result depends on the step before it (naming the room a
wall outline closed, giving new openings a material) runs a second phase on
the staged project; it is still the same undo step.

**Refs.** A `build` entry or `batch` step may carry `ref` ("kitchen"). A
later step's string `"$kitchen"` (or `{"$ref": "kitchen"}`) becomes the id of
the element of the step's kind it made (the room it named for `add_room`,
the level for `levels`); `"$kitchen.2"` the third such element. A `"$name"`
naming no earlier ref is refused (`unknown_ref`). `build` applies its lists
in the order levels, walls, rooms, columns, slabs, stairs, openings, assets,
cameras.

**Edit results.** `ok`, `revision`, `base_revision` (the revision the edit
started from), `summary`, `undo_label`, `created`, `changed`, `deleted` (id,
kind, label, at most 100 each, then `*_count`), `levels_added`,
`levels_removed`, `issues_added` (issues whose id the base revision lacked:
id, code, severity, message, element_ids, location_mm), `issues_resolved`
(the other way), `also_moved` (`Diff::also_moved`), `totals`; with
`batch`/`build`: `steps` (per step lists, `ref` and `id`), `refs`; one call:
`id`; `details` (stair riser and tread, opening wall and offset, slab area);
`warnings` (storeys whose height runs past the floor above); `scope`; `agent`
(its name).

**Errors.** `isError` with structured content `{code, message, element_ids,
location_mm, step, ref, hint}` (`guhit_mcp::fail::ToolFail`). `code` is the
engine's own code (`CoreError::code`, so `opening_overlap`, not `invalid`),
an `IpcError` code, `bad_args`, `unknown_tool`, `unknown_ref`,
`unknown_catalog_key`, `no_room_here`, `unavailable` or `needs_force` (the
swarm lead's `discard` without `force`). `step` is 1-based in
a `batch` or `build`, absent for one call. The text content keeps the v1
wording ("step 5 of 6 (`add_door`): invalid: ... Nothing was applied.") and
adds "Hint: ...". The swarm refusals (`swarm_running`, `draft_open`,
`part_stopped`, `draft_view`, `people_only`) have their own hints, never the
live session's advice to pass `force`; a swarm action refused
`live_session` says to leave the session. An unknown tool names the nearest
v2 tool.

**Check baselines.** The engine keeps no history of derived data, so
`check since_revision` compares with the review items this server noted at
the latest revision at or before it: it notes them before and after each of
its edits and at each `check` (64 revisions per project, in memory).

**Pictures.** `look` mode plan draws `guhit_export::plan::build_items` of a
level as SVG with agent overlays (wall lengths, short ids with a map to full
ids, a grid, a scale bar, underlays) and rasterizes it with resvg to PNG, at
most 1568 px. `px_per_mm` and `bbox_mm` map pixels to plan mm. The resource
template `guhit://project/levels/{level}/plan.svg` serves the same SVG. 3D
goes through the window (`WindowTask::CaptureView`).

**Agents** (the interface with the agents registry above). Per call the
server builds `guhit_app::agents::AgentHint` from the `agent` argument, the
labelled URL `/mcp/agent/<label>`, the stdio proxy's `x-guhit-agent-instance`
header and the connection's `clientInfo`, calls `AppService::agent_call`
(Read for reads, pictures, exports, chat, status and dry runs; Edit for the
rest), commits through `agent_edit` or `agent_commit`, undoes through
`agent_undo`, chats through `agent_chat`, and after a read calls
`agent_done` with the ids it named. `initialize` answers with an
`Mcp-Session-Id` (`guhit_mcp::new_session_id`: a random part and the client's
name and version in hex, so it names its client after an app restart too);
`AgentHint::connection` is `session:<id>`, else `label:<label>`, else
`instance:<id>`. A request without `clientInfo` takes the client its session
names, else the one its label, instance or TCP peer named
(`McpState::saw_client_at`), else none: never the last client seen. A
multi-step `history` undo checks every step first (the agent's own, or a
person's with `force`; another agent's never), then undoes them as one
revision with `expected_revision`. `import_dxf` commits through
`agent_edit` like every edit; a DWG answers `unavailable`. An agent posts at
most 6 chat messages a minute (`rate_limited`). Every result of an agent's call carries `activity` and, when they hold
anything, `inbox` and `nearby` from `AppService::agent_notes`. A `dry_run`
runs the registry's checks (`AppService::agent_check`). `look` mode 3d and
`render` pass `visibility` to `WindowTask` (None when the call names none). Each call sets the status line from
the tool (`guhit_mcp::agent::tool_line`) through `agent_set_status`, unless
the agent's own `set_status` is less than 60 s old.

**IPC.** `mcp_toolsets` → `{toolsets, available}`; `mcp_set_toolsets`
`{toolsets: [names]}` → the same, stored in settings.json as `mcp_toolsets`
(unknown names dropped, `"all"` is every toolset).

**Not in v2.** Overhead (garage) door styles, L and U stairs, section and
elevation pictures, and GLB export from the server: each answers
`unavailable` or is not offered, with a hint naming what to use instead.

## Swarms

DECISIONS D56, D57. Types: `crates/guhit-model/src/swarm.rs`. App side:
`crates/guhit-app/src/swarm/` (`mod.rs` the service API, the lock, binding
and IPC; `draft.rs` the draft). Engine: `guhit_core::replay`
(`Replayed`, `Document::apply_replayed`). MCP: the core tool `swarm`
(`crates/guhit-mcp/src/swarm.rs`).

One brief, a lead splits it into parts, several agents draw the parts at
once into a draft, and a person accepts all of it (or, once L1 lands, one
part) or discards it. Nothing enters the model before a person accepts, in
the window. No agent can accept (`people_only`).

**Types.**
- `SwarmPlan { summary, setup: SwarmSetup { levels: [SwarmLevel { name, height_mm? }] }, parts: [SwarmPartPlan] }`.
  `SwarmPartPlan { key, name, role, instructions, scope: SwarmScope, after: [SwarmWait { part, milestone }], milestones }`.
  `SwarmScope { levels, kinds, area }`: levels are ids once the swarm
  started (a plan may name levels by name, setup levels included); empty
  lists limit nothing; `area` is always None in v1. "done" is every part's
  implicit last milestone.
- `SwarmStatus` is the panel and the `swarm` event: `id`, `project_id`,
  `source` (`{type: "app"}` or `{type: "mcp", client}`), `state`
  (`planning`, `running`, `paused`, `ready`, `interrupted` (unused in v1),
  `failed`), `brief`, `plan` (scope levels as ids), `parts`, `feed` (newest
  first, at most 14), `draft_revision` (up by one with every draft change),
  `main_revision` (holds while the draft is open), `added`, `modified`,
  `removed` (the draft against the model), `conflicts`, `cost` (in-app
  only), `ai_wait_until`.
- `SwarmPart`: `key`, `name`, `role`, `color` (a peer color), `agent_id`
  (once bound), `state` (`waiting`, `working`, `paused`, `blocked`, `done`,
  `stopped`, `failed`, `accepted`), `status`, `steps`, `milestones`
  reached, `waiting_for`, `conflicts`, `last_seq` (its newest step's `seq`,
  0 before its first). The lead is the first part, key `lead`, name "Lead":
  its steps are the Setup steps and every call of the lead's connection
  that names no handle.
- `SwarmStep { seq, part, agent_id, label, status, added, modified, removed, at }`:
  one agent tool call in the draft, `seq` from 1, never reused.
- `SwarmDraftState { swarm_id, draft_revision, state, parts, removed, steps, display_modified }`:
  `state` is the draft view (the model with every step applied, derived by
  the engine, empty history); `parts` per part the elements it added or
  changed against the model (`SwarmPartIds { part, color, ids }`, each id
  under the part whose step touched it last); `removed` model elements the
  draft removes; `steps` those after `since_step`; `display_modified` model
  elements whose look changes without a step changing them (the host walls
  of draft doors and windows; 3D hides their model meshes). The diff is
  worked out in Rust, never in TypeScript.
- `SwarmConflict { part, step, label, code, message, element_ids }`: codes
  are the engine's, or `lost_id`, `changed_on_main`, `cross_part`, `stale`
  (L1).
- `SwarmCost { model, model_label?, price_note?, low_usd?, high_usd?, spent_usd, cap_usd, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens }` (L2).
  `model` is the id the workers use: the `ai_model` setting, else
  `claude-opus-5-5`; `model_label` how people read it ("Claude Opus 5.5",
  the id itself for a model the price table does not know). A model the
  table does not know is estimated and billed at the table's highest row
  (US$10 / US$50, reads 0.1x) and `price_note` says "Billed at the highest
  rate: price unknown", so the cap always holds. A response from a model
  without a row is billed at the requested model's price first.
- `SwarmRequest { brief, active_level_id?, scope?, review_plan, cap_usd? }`:
  `review_plan` is ignored in v1.
- `SwarmAcceptResult { applied: [SwarmApplied { part, revision, summary }], skipped, state?, dependents, differs }`:
  `applied` the parts that commit (a part that changes nothing in the
  model is not listed); `skipped` the steps that stay out; `state` is the
  model after an accept (None for a check); `dependents` (check only) the
  parts that depend on the parts checked.
- `SwarmDraftReply` is `SwarmDraftCurrent { swarm_id, draft_revision, status }`
  (the window is current: only the status) or `SwarmDraftState`.

**The main lock.** While a draft is open every write to the model is
refused `swarm_running`, message "A swarm draft is open: accept or discard
it first.": a person in the window (`doc_apply`, `doc_undo`, `doc_redo`,
imports), the copilot (`ai_resolve`), agents outside the swarm, and a live
host's commit. The check sits in the app service's one commit path
(`AppService::commit_with`, which `commit`, `commit_if_revision`,
`commit_staged_if_revision`, `commit_locked` and the agents' commits use)
and its one undo path (`AppService::step_locked`), as
`swarm::check_main_write`. `swarm::guard`, first in `AppService::handle`,
refuses what replaces, closes, renames or shares the open project:
`hub_open` of another project, `hub_create`, `hub_close`, `hub_delete` and
`hub_rename` of the open project (a rename changes the model without a
revision), `snapshot_restore`, `bundle_open`, `live_host`, `live_join`.
Reading, selecting, saving a version, renders and exports work. The
copilot asks the model nothing while a draft is open: `ai_chat` (furnishing
too) is refused `swarm_running` before any request, so nothing is spent;
`ai_resolve` of a proposal made before is refused `swarm_running` and the
proposal stays pending (a furnishing room stays queued), so it applies once
the draft is accepted or discarded. A person can still accept or discard
(and, with L2, pause, stop and change the speed).

**Binding agents.** `AppService::swarm_start_mcp(call, brief, plan)` (MCP
`swarm` action `start`) checks the plan, creates the draft on the open
project with the model as its base, adds the setup levels as the lead's
first step ("Swarm setup: add levels"), resolves scope levels to ids, and
answers `SwarmStarted { swarm, handles: [{key, name, color, handle, url}], how_to }`.
A handle is `sw-<16 hex digits>-<key>`: 64 random bits from the system
source, never issued twice while the app runs; `url` is
`http://127.0.0.1:<mcp port>/mcp/agent/<handle>`. The caller becomes the
lead. In `AppService::agent_call`, an agent whose `agent` argument or
labelled URL equals a live handle is bound to that part (by agent id, for
the rest of the swarm) and shown with the part's name and color on the
list, its cursor and its history steps (in memory, never saved as a
rename). A made-up handle binds nothing; a handle of a swarm that ended is
refused `part_stopped`, so a late sub-agent never writes to the model; so
is every call of a stopped or accepted part's agent. Refused at start:
`draft_open` (a draft is open), `live_session` (a live session; a guest is
refused every `swarm_*` call), `bad_args` for a bad plan: no parts or more
than 6, a key that is not lowercase words joined by single hyphens (at most
24 characters), `lead` or a repeat, a level neither in the project nor in
setup, an area, waiting for a part or milestone the plan does not declare,
a circle of waits, no part that can start at once, two parts that could
draw the same kind on the same level (an empty list is all levels or all
kinds).

**Draft edits.** For a bound agent, `agent_edit` and `agent_commit` go to
`swarm::draft_edit` and never touch the model or its history. Two passes as
in `agent_edit`: `build` runs on the view outside the draft lock, and again
under it when the draft moved meanwhile. Checked in order: the connection's
pause, stop and rate (`agent_paused`, `agent_stopped`, `rate_limited`), the
person's limit and other agents' claims with what only people may do
(`out_of_scope`, `claimed`, `forbidden` for `SetSignatory`), then the
part's state (`part_stopped`, `agent_paused`) and scope (`out_of_scope`,
naming the part and its levels and kinds), at most 400 steps a part and
2000 new elements a draft (`invalid`). The step is recorded (its leaf
commands, label, diff, the part's status line, time), the view rebuilt, the
agent's presence moved to what it drew, and `AppEvent::Swarm` published.
The answer is an `ApplyResult` on the view: `state.revision` is the base
revision plus `draft_revision`, `undo_label` the step's label.
`agent_check` (an MCP dry run) checks the same for a bound agent.
`agent_done` and `agent_claim` use the view; `agent_notes` leaves the
model's history out (`swarm::nearby`, L2 and L3). `agent_undo` for a bound
agent is `swarm::draft_undo` (`not_built` until L1 and L3; it never undoes
the model). `AppService::swarm_view(agent)` is the view a bound agent reads:
every MCP read of the model goes through one gate, `guhit-mcp/src/view.rs`
(`query`, `look` plan, elevation, section, sheet and schedule, `measure`,
`check`, the list reads, dry runs, the v1 read aliases and the resources),
and a test fails if any other source file locks the session or asks the app
for the document. A bound agent is refused `draft_view` for what the window
draws or what is written from the model: `look` mode 3d, mode tour and
source window, `get_plan_image`, `capture_view`, `render` start,
`visualize_render`, `export_tour`, `export`, `sheets` export and saved
versions. MCP `history` for a bound agent lists the draft's steps, and undo,
redo and undo_to go through `agent_undo` on its own draft steps only
(`other_author` for another part's).

**Accept all** (`swarm_accept` with `parts` null). Under the session lock:
pinned to `reviewed` (refused `stale` when a part has a step after the
`last_seq` the person reviewed; a part left out counts as 0); every step
replayed in recorded order on the model, which equals the base because the
model is locked, so every id is the draft's (refused `stale`, nothing
applied, if the result is not the view element for element); a version
"Before swarm, 3:42 PM" saved first (nothing is accepted when it cannot
be); then each part committed as one undo step, author its agent (that of
its newest step), `Origin::Ai`, label "Swarm: <part name>"
(`Document::apply_replayed`). Parts commit in dependency order, ties in the
order of their first step; part k's step is every step of parts 0 to k
replayed in recorded order with pinned ids, so the last one is the view
exactly. The draft stays open, every part held (`Draft::holding`: no
step lands, an agent is refused `agent_paused`, a worker waits) and
`committing` set, until every commit is in; before them the accept checks
under the session lock that the model is the one it planned on, all of it
(`updated_at` aside), and plans again if not. If a commit fails, the parts
whose steps went in are accepted and every other step is installed again on
the model as it is, so nothing is lost. The swarm then ends:
`AppEvent::Swarm { swarm: None }`. Undoing an accepted part later is a
normal undo.

**Pinned ids** (`guhit_core::replay`). Each step is recorded once more as it
lands (`replay::record`): every id its leaf commands made, by slot
(`Slot::Own(n)` in order, `Slot::Room` for a room `reconcile` made, keyed by
level and sorted bounding walls, `Slot::Type` for a type
`opening_types::follow` made, keyed by the opening), and a hash of
everything it names. `replay::replay` runs it anywhere with those ids: one
more, one fewer or one that ends up as something else is `lost_id`, never a
relabel; something it names that changed is `changed_on_main`.

**Dependencies.** A part depends on another when one of its steps names or
changes what the other made or changed before, or when its plan waits for
it. A step that would make two parts depend on each other is refused
`cross_part` when written (and by `agent_check`).

**Accept parts** (`swarm_accept` with `parts`): each part with every part it
depends on, each one undo step, pinned to `reviewed` for those parts only
(`stale` when one of them drew, undid or lost a step since). A step of
those parts that does not apply on the model (replaying every step of the
parts taken leaves it out: dependencies come from ids, so a spatial need
such as an auto dimension on another part's walls is missed) is refused
`steps_skipped`, nothing applied, unless the request says `accept_rest`
(the window asks first: "Accept the rest" or Cancel); then the rest goes in
and those steps stay in the draft, listed in `SwarmStatus::conflicts`
until they go in with what they need. The rest of the draft is replayed on
the new model in recorded order; a step that no longer applies drops out
into `SwarmStatus::conflicts`, and so does every later step needing what it
made (`lost_id`). A part is `accepted` only when every step of it went in
(its agent then gets `part_stopped`); a part with nothing applied stays as
it was. The lead keeps going. The swarm ends once no step is left and every
plan part is accepted, stopped, failed or done. Two passes: planned on
copies with no lock, then under the session lock and the draft lock it
checks the model (all of it) and the parts did not move (new steps of other
parts are replayed after), marks the draft (`Draft::accepting`, which the
main lock lets through), holds the parts it takes and commits.

**Check** (`swarm_check`, changes nothing): `applied` the parts an accept
would commit, in order (revision 0), `skipped` the steps that would stay
out (an accept is refused `steps_skipped` unless `accept_rest`), `dependents` the parts built on the ones named, `differs` the elements
the model would get differently from the view (code `differs`; an auto
room's count-based name, for one).

**Discard part** (`swarm_part_discard`): a part others depend on is refused
`invalid`, naming them, unless `with_dependents`; then all of them leave the
draft and stop, and the rest is replayed. `null` when nothing is left (the
swarm ended).

**An agent's undo** (`agent_undo` for a bound agent, MCP `history`): undo
takes its part's newest draft step out, redo puts back the last one it took
(until the part draws again); a step another part built on is
`other_author`, `force` or not. Never the model's history.

**The read gate.** `AppService::read_view(agent)` is the draft view for a
bound agent, the model for everyone else;
`AppService::refuse_draft_view(agent, WindowOutput)` refuses a bound agent
renders, AI renders, tour videos, exports, versions, window-drawn looks and
3D (`draft_view`). MCP's own gate is `crates/guhit-mcp/src/view.rs` (L3).

**Discard** (`swarm_discard`): the draft is dropped and the model unlocked;
`AppEvent::Swarm { swarm: None }`. `AppService::swarm_discard(None)` drops
whatever draft is open (an MCP lead with `force`, L3). It takes the session
lock first, so a discard that races an accept waits for its commits and
then answers from what is left: `not_found` when the accept ended the
swarm, and parts already in the model stay there.

**Milestones and finishing.** `AppService::swarm_milestone(call, name)` puts
a milestone the plan declares for the caller's part on its list (once) and
in the feed (`bad_args` for one it does not declare, `part_stopped` for a
stopped, failed or accepted part, `not_found` outside the swarm).
`AppService::swarm_finish(call, summary)` marks the part Done (its "done"),
with the summary as its status line; from the lead it marks the draft
Ready, unless paused. MCP `swarm` actions `milestone`, `finish`, `wait` (a
long poll on `AppEvent::Swarm`, at most 60 s: `for` {part, milestone}, else
what the caller's part comes after, else for the lead every part done),
`status` (parts, the draft's counts, what each part drew by kind) and
`discard` (the lead only, with `force`) use them; L2's workers use the same
two methods.

**Locks.** Session, then draft (a std mutex in `swarm::Swarms`, never held
across an await), then the agents registry. Never the other way.

**v1 leaves out** saving drafts across a restart (no `drafts/` folder,
`project.json` unchanged, `SCHEMA_VERSION` stays 6, `PROTOCOL_VERSION`
unchanged), Continue and `resume`, area scopes, plan review, per-part undo
(`swarm_part_undo`) and the paced reveal in 3D.

## App events and window requests

`AppService::events()` is a broadcast of `AppEvent`. The desktop shell
emits each one to the window as the Tauri event `app_event`; the dev bridge
streams them as server-sent events on `GET /events` (one JSON `AppEvent`
per `data:` line). The frontend subscribes with `onAppEvent`
(`src/contract/ipc.ts`). Document changes keep `doc_changed`. The swarm
sends `AppEvent::Swarm { swarm }` on every change of its draft, `None` when
it ends ("Swarms"), at most 8 a second: a change within 125 ms of the last
event waits, a newer one taking its place, so the newest always goes out
and none out of order (`swarm::emit_swarm`).

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
Staging::new(&project, label)                              // a batch built step by step on one working copy
staging.push(&commands) -> Result<Project, CoreError>       // run them after the staged steps; all or nothing; returns the project before them; errors read "Step k of n: " counting every staged command
staging.project() / .diff() / .derived() / .also_moved() / .command()  // staged view, preview diff, derived (computed on call), walls that followed, the Batch
doc.apply_staged(staging, origin, author, at) -> Result<ApplyResult, CoreError> // commit it as apply_at would, without running it again when the project is unchanged
doc.apply_at(command, origin, author, at) -> Result<ApplyResult, CoreError> // same, recording when (the caller's clock, RFC 3339)
doc.undo() / doc.redo() -> Result<DocState, CoreError>
doc.undo_steps(n) / doc.redo_steps(n) -> Result<DocState, CoreError> // n steps as one revision, all or nothing (bad_steps for 0, too_many_steps)
doc.history() -> HistoryList                               // newest first; author ids only, names are the app's to fill in
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

App service helpers shared with the AI module: `AppService::commit(command, origin)` (apply + autosave), `AppService::commit_if_revision(command, origin, expected_revision)` (same, atomic, `stale` on mismatch), `AppService::commit_staged_if_revision(staging, origin, expected_revision)` (the same for a `Staging`, holding the session lock only to swap the result in; a guest sends the batch to the host) and `AppService::project_dir()`.

Batches stage linearly: MCP `batch` and the copilot run each step once on one
working copy, so N steps cost N steps (a 200 step batch takes about 0.6 s in
a debug build; re-previewing the whole batch per step took 179 s). MCP stages
outside the session lock. The engine keeps a small process-wide cache of
level analyses and room label points keyed by the exact wall geometry, so
unchanged levels are not analyzed again; results are identical.

`AppService::new` allows caller-supplied export paths (desktop, from the native save dialog). `AppService::new_sandboxed` (dev bridge) rejects any path outside its data dir with `forbidden`. `snapshot_restore` continues the revision counter, revisions never repeat.

After every change the engine recomputes `Derived`: wall outlines with
resolved joins, room polygons and areas, footprints, totals, review issues.

## Wave 2 contract (DECISIONS D53, D54)

The types are in `guhit-model`; this section says what they cannot. Each
lane writes the detail of its own part below its heading here, and asks the
orchestrator for any change to the types, the hubs or another lane's part.

### Schema 6 and the migration

`SCHEMA_VERSION` is 6. `guhit_core::migrate` (run by `Document::new` and
`with_revision`, idempotent) adds the `grids` and `roofs` layers and, when
the project roof has a kind and there is no `TopFootprint` roof element,
adds one `RoofArea { level_id: None, outline: TopFootprint }` with the old
shape, pitch, axis, overhang, thickness and material, a 200 mm fascia (what
Wave 1 drew), no gutter, and an id seeded from the project
(`roofs::migrate`); then `Project::roof.kind` becomes `none` and keeps its
other values. The templates come out migrated. Every other new field takes
its serde default. The schema 5 copies of the two fixtures and the Wave 1
G5 Lovell project are kept in `crates/guhit-core/tests/data/` for the
migration tests (the G4 house was never saved as a project file).

New fields on existing types (`Opening::type_id`, `Room::finishes` and
`ceiling_height_mm`, the five `Stair` fields, `Dimension::through` and
`auto`, `Asset::built_in`, the `CatalogItem` fields, the `RenderRecord`
fields, `AddOpening::type_id`, `CaptureView::camera`) are left out of the
JSON at their defaults, as `Wall::role` is, so an older command's JSON and
the ids it seeds stay the same. Large payloads are boxed in Rust
(`Project::title_block`, the `SetTitleBlock`, `SetSignatory`, `UpsertSheet`
and `UpsertOpeningSpec` payloads, `Derived::site`, `Asset::built_in`) so
`Command`, `Project` and `DocState` stay small on the stack; the JSON and
the TypeScript are the same. Keep new large payloads boxed: the MCP v2 tests
run within about 300 KB of the 2 MiB test thread stack in debug builds.

### The roof over the top floor

Read it with `Project::top_roof()` (Rust; `guhit_core::roofs::top_roof`
too) or `topRoof(project)` (`src/contract/roof.ts`), never from
`Project::roof`: once migrated its kind is `none`, and an edit built from it
deletes the roof. `top_roof` is the `TopFootprint` element as a `Roof`, or
the legacy roof of an unmigrated project. `RoofKind` gained `hip`; Wave 1
roof drawing (core pipes, export model3d, TS roofMesh) draws a hip as a
gable until `Derived::roofs` lands.

`SetRoof { roof }`: kind none removes the `TopFootprint` roof (summary
"Remove roof"); any other kind changes it, keeping fascia, gutter, eave and
gable sides (a non-hip drops its gable sides), or adds it with a 200 mm
fascia. The copilot's `set_roof` and MCP's `set_project` build the whole
`Roof` from `top_roof`, so "pitch 25" changes only the pitch. Roof elements
are otherwise edited with `AddElement`, `UpdateElement`, `DeleteElements`
and `SetMaterial` (roofs take a material).

Roof rules (`roofs::validate`): `level_id` is None exactly for
`TopFootprint` (`roof_level`); one `TopFootprint` roof per project
(`roof_exists`); a drawn `Polygon` outline must suit a pitched shape:
every corner within 0.5 degrees of 90 or 270, gable and shed on 4 corners,
hip on 4, 6 or 8 (`roof_outline`, naming what fits; flat takes any). A
derived outline (`TopFootprint`, `Footprint`) is never refused: when its
shape does not fit, `Derived::roofs` keeps the Wave 1 bounding box
clipping (a hip falls back to a gable) and review raises a `roof_outline`
suggestion. `gable_sides` only on a hip over a rectangle, sides 0 to 3 each
once (`roof_gable_sides`). A drawn outline moves and turns with the roof; a
quarter turn swaps its ridge axis. `flip` (default false) makes a shed rise
against `ridge_axis`, toward -x or -y, so a lanai on any side slopes away
from the house.

### Elements on no level, and views

`Element::level_id()` (Rust) and `elementLevel(el)` (`src/contract/
elements.ts`) are the one answer: None for openings (they are on their
wall), cameras and tours (views), grid lines and the lot (every level) and
the `TopFootprint` roof. `DeleteLevel` never deletes them (openings go with
their wall). View marks and `Footprint` and `Polygon` roofs belong to their
level and go with it. `Element::is_view()` / `isView(el)` is true for
cameras and tours: every camera special case (scope, agent limits, work
areas, copies) goes through it. Layers: roofs `roofs`, grids `grids`, view
marks `annotations`, the lot `site`; views have no layer in Rust (a locked
layer never stops one; the TS plan index files them under annotations, as
it does cameras).

### Door and window types (D53)

`Project::opening_types` holds `OpeningSpec`s; an opening's `type_id`
names one of its own opening type (`unknown_opening_type`,
`opening_type_mismatch`). A typed opening's style, size and sill are its
type's (a type's `sill_mm` None leaves each opening its own). The rules the
lane builds (`opening_types`): `UpsertOpeningSpec` writes the type into
every opening of it in the same step, refused only by the existing fit
rules; editing one typed opening's style, size or sill moves it to the type
that matches, or a new type with the next free mark, in the same step, and
the summary says so ("Door moved from D1 to D4"); never refuse
(`opening_types::follow`, run after `post_validate`). `DeleteOpeningSpecs`
leaves the openings untyped at their size. Marks: "D1", "W1", unique, empty
gets the next free.

### Dimension strings and automatic dimensions (D12 amended by D54)

A dimension string is `a`, `through...`, `b` in order. An automatic
dimension (`auto: Some(DimRef)`) stores what it measures; `exec::run`
calls `dims_auto::after_command` after `post_validate`, like wall
following: editing a, b or `through` by hand clears `auto`; references to
deleted walls or grids are dropped, and a dimension left measuring nothing
is deleted in the same step; then `dims_auto::rewrite` (the lane's) sets
a, b and `through` from what it measures. Wall following (D12) skips
automatic dimensions. `DimRef::Exterior` measures the dimension's own
level (it has no `level_id` of its own).

### Stairs (D44 extended)

`Stair::shape` straight (default), `l` or `u`. Each flight's top riser
lands on its landing or the floor, so n risers make n minus the flight
count treads (n - 1 straight, n - 2 L or U), each `run_mm / treads`;
`run_mm` excludes landings. An L or U needs at least 4 risers;
`first_flight_risers` is 2 to n - 2 (None: half, rounded up);
`landing_mm` 300 to 10000 (None: the width); `well_mm` 0 to 10000 (U).
`Derived::stairs` is the one stair geometry every consumer reads, z above
the stair's level floor.

### Roofs and stairs, derived (lane L2)

**Stairs** (`guhit_core::stairs`). An L leaves its first flight onto a
landing `landing_mm` deep (the width when None) along the travel; the
second flight leaves the landing's side (`turn` right: to the right going
up), its far side in line with the landing's far edge. A U's landing spans
both flights and the well; the second flight runs back down beside the
first, `well_mm` clear. Each `Flight::outline` is exactly [first riser
left, first riser right, top right, top left] facing up (counter-clockwise);
its nosings are bottom first, the first on the first riser, `z` the tread's
top. `opening` is the union of the flights, the landing and a U's well
(empty when the stair arrives nowhere). `walk_line` starts half a going
before the first riser at z 0, passes the middle of every tread at its top,
runs flat across the landing round the turn, and ends half a going past the
top riser at the level height: no step between points over one riser.
Helpers: `geometry`, `frames` (each flight's frame), `head`, `foot`,
`arrival_edge`, `pieces`, `covers`, `tread_count`, `describe`.
Railings (`stair_side`) run up each flight's open sides on its nosing line
and on round a landing, flat there; the next flight starts its own run; the
inner sides of a U under 50 mm apart get none. The well guard upstairs goes
round the whole opening but the arrival edge. Review: `stair_headroom_low`
(under 2000 mm clear over the walking line: under the floors above outside
the stairs' openings and voids, other stairs' soffits and landings, roofs),
`stair_landing_short` (an L or U landing shallower than the stair is wide);
`stair_landing_blocked` and `stair_top_no_floor` read the first and last
flights.

**Roofs** (`guhit_core::roofs`). The eave line is the wall line grown by
`overhang_mm` (mitred); the underside at the wall line is `eave_mm`, or the
level height; the top is `thickness_mm / cos(pitch)` over the underside
(`vertical_mm`). A hip on a square-cornered outline is the straight skeleton
of its eave line: every top point stands tan(pitch) times its distance from
the eaves above the eave height, the distance measured square to the outline
(the larger of the two axis distances to the nearest eave, in the outline's
own frame), so hips and valleys run at 45 degrees and a rectangle's ridge is
L - W. Sides in line facing the same way are one plane where the wavefront
joins them. Gable sides stand still. Gable and shed slope in the rectangle's
own frame; `ridge_axis` x is its side nearest to east-west. Lines: `eave`
(low edges), `rake` (sloping outline edges), `ridge` (horizontal inner
edges, and a shed's high edge), `hip`, `valley`. `eaves` are the low edges
in outline order, z the top at the edge. `infill` only on derived outlines,
along the wall line where the underside is above the wall top (first edge on
the wall top, the outline's way round, so inward is its left).
`plan_area_m2` is the eave outline, `surface_area_m2` each plane over its
cosine. A never-migrated project's legacy roof derives with an empty
`roof_id`. The roof over the top floor without a closed building anywhere
covers the bounding box of the top level's walls when it spans over
1000 mm both ways (Wave 1). Review: `roof_outline` (info, a derived outline
the shape does not fit), `roof_unsupported` (a drawn roof with over 5 m of
edge with no wall along it and no column within 400 mm).
`AddRoofPosts { roof_id, size_mm 50..1000, max_spacing_mm 500..20000 }`:
square columns on the roof's level along every open stretch of its wall
line (no wall along it), at each corner and at most the spacing apart, outer
faces on the line; none where a wall carries the end, nor within a post of
an existing column; nothing to add is `no_open_eaves`. Pipes cross every
plane of a roof on their level (`roof` penetrations).

**Drawing.** Plans (`guhit_export::stairs::stairs_items`, the 2D canvas the
same): on its own level a stair is solid up to the cut 1200 mm above the
floor, with a break line there and the rest dashed (`StrsBelow`), the arrow
along the walking line round the turn and UP before the first riser; on the
level it arrives at, all dashed with DN. Roofs on the plan: the eaves dashed
on the level the roof sits on only. 3D: every derived roof on its level
(covering, soffit, fascia along eaves and rakes, a box gutter along eaves,
gable ends a wall thick, a ceiling at the wall tops under a pitched roof over
walls); roof decks (D26) drop under any roof on their level; a deck slab with
no material reads as paving stones within 300 mm of the ground of the lowest level
(a carport), timber otherwise. Walk mode climbs an L or U along its walking
line (`u` follows it round the turn), held within the flights and landing.

### Grids, view marks and the lot

Grid labels are unique; a grid line is at least 100 mm long. A section or
elevation mark is drawn on its level's plan; its drawing is computed on
demand from the mark (`guhit_export::views`), never stored. One lot per
project (`lot_exists`, naming the first): at least three corners, one side
per corner, setbacks 0 or more. Setbacks and coverage are drawn facts,
never code checks.

### Title block and the signatory (D53)

`TitleBlock` has no owner or location: sheets print
`ProjectSettings::client_name` and `location`. `SetTitleBlock` keeps the
stored signatory whatever it is given; only `SetSignatory` changes it, and
`guhit_app::people_only` refuses `SetSignatory` (also inside a batch) for
every agent path (`AgentsState::check_reach`, a guest's agents on the host)
and the copilot's tools, with code `forbidden`. `Signatory::masked()` hides
PRC, PTR, IAPOA, its OR and TIN (every letter and digit but the last two,
all of one of four or fewer; separators stay; empty stays empty) and keeps
the name, dates and place; `TitleBlock::masked()` masks its signatory. Use
it for prompts, AI logs, MCP reads and agents' pictures
(`SheetInputs::mask_signatory`); the PDF on disk prints in full.

### Sheets, viewports and revisions

`UpsertSheet` assigns an empty sheet id, empty viewport ids and an empty
number ("A-n", the first free); numbers are unique
(`sheet_number_taken`); a viewport's level or view mark must exist with the
right mark kind (`bad_viewport`). A viewport whose level or mark is deleted
leaves its sheet in that step, and the summary ends "; removed 1 viewport
from sheet A-3" (`sheets::prune_viewports`). `UpsertRevision` assigns an
empty id and marks the revision on `sheet_ids`; `DeleteRevision` takes it
off every sheet. A perspective viewport is a model render, never an AI
image. Ids of types, sheets, viewports, revisions and shots are hashed and
reserved by `IdGen` (P0-5): two identical `UpsertSheet` calls with empty
ids make two sheets.

### Tours

A tour is a view (no level, no layer, always allowed). Shots get ids like
elements: empty ones on add and update, all new on a copy. fps 24, 30 or
60; each shot at least one key, `at` 0 to 1, 0.1 to 600 s, a transition of
0 to 10 s.

Frames (`guhit_core::tours`, lane L9). Every reader takes the camera from
`Query::TourFrames { tour_id, fps }` (fps None: the tour's own), never its
own math: playback, the timeline, the 3D path, the video and the review.
The answer is `{ tour_id, name, fps, seconds, frame_count, shots: [{ id,
name, move, start, seconds, transition, first_frame, frame_count }],
frames: [{ t, shot, position, target, fov_deg, light?, fx? }], doors:
[{ id, from, to, open }] }`, one frame
every 1 / fps seconds from 0, `round(seconds * fps)` of them; positions to
0.1 mm.
- A tour lasts the sum of its shots. A shot's keys are taken in `at` order;
  position and target each run on a centripetal Catmull-Rom spline (alpha
  0.5, mirrored ends) through them. The shot's ease turns time into
  progress: `linear`, or `in_out`, which speeds up over the first fifth,
  runs evenly, and slows over the last fifth (top speed 1.25 times the
  average). Between two keys, progress moves along the position path by arc
  length; when the camera stands still, along the target path (a pan); when
  neither moves, evenly in time (a zoom). The lens runs linearly between
  keys. `MakeTour` writes `at` by arc length, so its shots move at an even
  pace, and sizes walks so their top speed is the pace.
- A transition happens over the first `transition.seconds` of its shot
  (`fx.mix` 0 to 1). Dissolve: the held frame `fx.from_frame` (the last of
  the shot before; black for a first shot) fades out over this one. Dip to
  black or white: the held frame goes to the color over the first half, the
  color clears over the second. Whip pan: the camera already swings out of
  the last shot's end and into this one (80 degrees), `fx.blur` the sideways
  smear. Plan lift: the camera already lifts from straight above the
  building, north up; the 2D plan lies over the picture and fades out over
  the first 60 percent. Playback (`tour/player.ts`) and the video
  (`render/tourJob.ts`) draw these layers the same way (`fxLayers`).
- `light` (a `ViewLight`) is the shot's light, the clock running from
  `light` to `light_end` (day to dusk); none: the live light.

`MakeTour { name, options: AutoTour }` adds one tour, one undo step, 30 fps,
named "Tour" (or "Tour 2" and on): with `exterior`, a fly-in (its first
transition a plan lift), an orbit over the roof and a push-in down to the
front door (the outside door into the living room or hall, else the
widest); then through the front door, every room with a door, nearest by
doors first, with a 3.5 s hold panning across each from its doorway; to
the stairs and up `Derived::stairs` walk lines at eye height (1600 mm over
the treads; straight up the flight's middle without one); the rooms upstairs
the same way; rooms only reached from outside after a dissolve to their own
door; with `day_to_dusk`, a pull-out outside from 16:50 to 18:00 on 15
April after a dissolve. Walks go round walls through the corners of the
room they are in. Walks look at their path 1.5 m ahead (1.8 m on a stair, tilted up 0.7
of its rise), coming in where a wall is in the way, so the camera turns
into a corner or up the next flight before it gets there; the turn out of
a hold goes the way the walk turns. Walks keep 600 mm from walls where the
room allows, pass a sliding door through the half its panel opens, and
step off a stair short of a wall ahead (800 mm). Refused: a pace outside 0.3 to 3 m/s (`tour_pace`), no
walls or no level with rooms (`tour_empty`), an unknown level.

`tour_through_wall` (a warning, `element_ids` the tour then the wall, id
`tour_through_wall:<tour>,<wall or level>`, `location` where, z above
project zero) is raised once per wall or floor the camera passes through,
sampled at 5 frames a second: the move between two samples meets a wall
outside a door's opening (a window is glass), or crosses an upper floor
over its buildings and floor slabs outside a stair opening or a void. A
move across shots counts only where they join, and never under a whip pan.

Videos (`guhit-app` `video.rs`): `video_begin` keeps an upload on disk
(`renders/<id>.part` and `.part.json`, so no state lives in memory and a
day old upload is dropped); `video_append` adds bytes in order;
`video_finish` checks the info (codec `avc`, `vp9` or `png`, 1 to 120 fps,
1 to 8192 px, the upload's tour), moves the bytes to `renders/<id>.mp4` (an
MP4 starts with an ftyp box) or splits them at each PNG's end into
`renders/<id>/frame-000001.png...`, saves the poster as `renders/<id>.png`
or `.jpg`, and adds a `RenderRecord` with `info.kind` `video`, `video` set
and the camera of the tour's first key. `video::is_video_file` names what
`.guhit` bundles leave out unless `videos` is asked.

The window encodes (`tour/export.ts`): WebCodecs H.264 High, Main, then
Baseline, then VP9, each at the lowest level the browser supports, first in
"quality" latency mode then "realtime", and each must turn three test
frames into three chunks within 4 s before it is used: Safari 27 says yes
to H.264 Main and High in "quality" mode and then never answers. No
encoder: PNG frames, and the result says so. Mediabunny (MPL-2.0, small,
writes MP4 in the browser; DECISIONS D54) muxes a fragmented MP4, so bytes
are written in order and stream to `video_append` in 2 MiB chunks, at most
four waiting. Frames render through `ViewerEngine.captureCanvas` with four
jittered samples (`refine.ts` `Accumulator`), paced on GPU fences, never
requestAnimationFrame. A hidden window is warned about: its timers slow to
once a second.

### Derived, queries and review

The derive order is fixed: walls, rooms, footprints and totals, then
stairs, railings, roofs, pipes, schedules (`opening_schedule`,
`room_schedule`, `level_areas`), site, then review (Wave 1 checks, slabs,
pipes, devices, then stairs, roofs, placement, site, tours), then review
marks. Each step may read the ones before it. New queries:
`OpeningSchedule`, `RoomSchedule`, `LevelAreas` and `Site` hand out the
derived data; `PlacementOptions`, `TourFrames` and `Catalog` are the lanes'.
`Query::Schedule` and `Derived::schedule` keep meaning device counts. New
review codes (suggestions): `stair_headroom_low`, `stair_landing_short`,
`roof_outline`, `roof_unsupported`, `asset_in_wall`, `assets_overlap`,
`tour_through_wall`, `building_over_setback` (`room_no_door` exists).

### Scope and agent limits

AI edit scope (D30): `AddRoofPosts` needs its roof in reach; `MakeTour` and
new tours and cameras are always allowed; a new grid, a new or changed
`TopFootprint` roof or lot, types, automatic dimensions, the title block,
the signatory, sheets and revisions are whole-project changes. Drawn roofs
and view marks reach themselves. Agent limits (P1-3, corrected by the
orchestrator): the `TopFootprint` roof is a roof on no level that reaches the
topmost level with a building (the highest level while none has one), and
so do posts under it: a level limit lets an agent edit it (`SetRoof`,
`UpdateElement`, `DeleteElements`, `AddRoofPosts`) only when it names that
level, and a kinds-only limit with roofs always does. It is still nobody's
claim. Grids pass level limits and a kind limit decides;
`UpsertOpeningSpec` and `DeleteOpeningSpecs` touch each opening of the
type, `AssignOpeningTypes` each untyped opening it would type, so an
openings-only agent can define D1; `AutoDimension` makes dimensions on its
level; the lot, sheets, the title block and revisions are whole-project
changes; tours are never refused.

### Stubs, hubs and `not_built`

Each lane's module starts as a stub with its final signature: commands
answer `not_built` (`CoreError::not_built`, IPC code `not_built`,
`ExportError::NotBuilt`), derive and queries answer empty data. The hubs
(`guhit-core` `exec`, `validate`, `derive`, `query`, `issues`; `guhit-app`
`lib.rs` routing and `ai/mod.rs`; `guhit-export` `lib.rs`; the frontend
hubs listed in "Frontend join points") have one-line arms into the lanes'
modules and are edited only by the orchestrator after C0.

### Live sessions

`PROTOCOL_VERSION` is 3: a version 2 guest is refused by a version 3 host,
and a version 2 host refuses a version 3 guest, both with "Update both to
the latest version." The `schema6` feature idea is dropped.

### MCP pictures of drawings

MCP `look` rasterizes elevations, sections, sheets and schedules in Rust
with the `resvg` already in `guhit-mcp` (`look::svg_png`), as it does the
plan, not through a window request. Elevations, sections and schedules come
from the `view_svg` IPC command; a sheet from `guhit_export::sheet_svg` with
`mask_signatory` set and its images, and every license number still printed
in full is replaced by its masked form before the picture is made (D53).
A tour frame is captured in the window from the camera `Query::TourFrames`
gives for it (`WindowTask::CaptureView` with `camera`). A saved view's `look`
hands the window that view's `Camera` as the engine holds it (`camera_id`
null), and the result compares the record's camera with it.

### The window's render queue (G4 problem 8)

| Command | Args | Returns |
|---|---|---|
| `window_status` | `WindowActivity { busy, what, view?, index?, total?, progress_pct?, eta_s?, job_id? }` | null |

The window sends it when a render starts, every 2 s while it runs, when it
moves to the next view and when it stops, whoever started it, and around a
tour export. `AppService::window_activity()` gives MCP the last report (not
believed after 30 s while busy) with the window requests not answered yet;
`render` action status and `window_busy` refusals quote it.

### Export seam (plan 1.6)

`guhit_export::{view_svg, sheet_svg, sheets_pdf, sheets_dxf}` and
`views::items(p, d, view, scale) -> ViewItems { items, bounds }` (L3 to
L4) with `SheetInputs { date, images, mask_signatory }`. `model3d::Scene`
holds `roofs: Vec<RoofSolid { roof_id, kind, material_id, level, planes,
infill }>`; the roof math moved to `guhit_export::roofs` and the stair
plan items to `guhit_export::stairs` (behavior unchanged). The planes of a
migrated roof stay within 0.5 mm of Wave 1's, L-shaped gables included
(`crates/guhit-app/tests/w2_l2_roofs_stairs.rs`, with a frozen copy of the
Wave 1 math).

### Permit engine (lane L1)

Door and window types (`guhit_core::opening_types`):
- `doors` are the doors the tour moves, in the view only (a tour never
  edits the model): door opening `id` goes from where it stood (as drawn,
  before its first move) to `open` between `from` and `to` seconds,
  smoothstep, then holds. `open` 0 is shut, 1 a swing leaf at 90 degrees,
  more turns it on toward the wall (up to 165), 1 for a sliding panel all
  the way over the fixed one. A door the camera goes through opens wide
  before the camera comes within its width plus 600 mm, as far as no wall
  or object stops it, and stays open; held short of 150 degrees it shuts
  once the camera is 700 mm past, unless the camera comes back through
  soon. A door the camera only passes is shut from the start (`from` and
  `to` -1), so it never moves on screen. Playback, scrubbing and the video
  apply them the same way (`tour/frames.ts` `doorsAt`, `DoorSwing.pose`).
- A type drives style, width and height always; the sill when the type has
  one (`sill_mm` Some); the material when it has one. `UpsertOpeningSpec`
  writes them into every opening of the type; the fit rules still refuse a
  type that pushes an opening past its wall. A type used by openings keeps
  its kind (`opening_type_mismatch`). An empty description becomes "Single
  swing door", "Sliding window" and so on.
- Marks: unique ignoring letter case (`mark_taken`, naming the next free);
  empty gets the lowest free "D<n>" or "W<n>". Schedules sort marks by
  prefix, then number (D2 before D10).
- `AssignOpeningTypes` groups untyped openings by kind, style, width,
  height, material and, for windows, sill (a new door type has no sill of
  its own); reuses a type that matches; new marks go doors first, widest
  first, then tallest, then style (double swing, single swing, sliding,
  casement, jalousie, fixed). Nothing to type is refused
  (`nothing_to_type`). The sample bungalow gives D1 (front, 900), D2
  (bedroom, 800), W1 (1500 sliding), W2 (1200 casement), W3 (1200
  jalousie).
- `follow`, after every leaf: an opening that just got its type (new, or a
  new `type_id`) takes the type's values when they fit its wall, else keeps
  its own and moves; an opening whose own driven values changed moves to the
  type that matches, or to a new type with the next free mark. The step's
  summary ends "; Door moved from D1 to new type D3" (or "to D1", or "Door
  kept its size, which D3 does not fit; type D1").

Schedules (`guhit_core::schedules`): `opening_schedule.rows` is one row
per type (doors, then windows, mark order; a type no opening uses has total
0; `per_level` lists every level, 0 included), then untyped groups with an
empty mark; a typed row's sill and material are the type's, else the
openings' common one. `room_schedule` rows give `net_area_m2` (less
voids, as `RoomGeometry`) and `floor_area_m2` (as `Totals` counts it, also
less balconies and decks), and the floor areas add up to
`Totals::floor_area_m2`; `level_areas` net and gross add up to `Totals`. A
room's ceiling height is its own; else, when a void in the floor of the
level right above lies over more than half of the room, that level's
ceiling measured from the room's floor (double height); else its level's
ceiling: the level height, or 200 mm under the next level up when that is
lower (D26; `schedules::level_ceiling_mm`).

Grids: `AddGridSystem` makes n + 1 lines from n spacings: numbers spaced
along x (lines run along y), letters spaced along y (no I or O; after Z,
AA). Numbers and letters continue after the project's highest. Lines reach
3600 mm past the outermost lines the other way, or past the buildings, so
their bubbles clear the automatic exterior dimensions.

Automatic dimensions (`guhit_core::dims_auto`). A to b always runs so that
a positive `offset_mm` puts the line on the measured side.
- `Openings { wall_ids, face }`: along the first wall's start to end; the
  ends of each wall's face, every opening's jambs, and other walls meeting
  the run between its ends (both their faces on the measured side; their
  centerline for a centerline string). The face measured is the one on the
  side of the line (the sign of the offset).
- `Exterior { side, tier }`: plan sides (north is +y). Overall: outermost
  face to outermost face. Walls: the centerlines of the walls that meet
  that side's outer walls from inside, corner walls included, and the grid
  lines across the side. Openings: that side's outer face corners and the
  jambs of the openings in its outer walls. a and b lie on the outermost
  face line; `through` points sit where they measure.
- `Grids { grid_ids }`: across the grid lines, through the first one's
  start.
- `AutoDimension` replaces the level's automatic strings of the kinds asked.
  Exterior: per side (north, east, south, west) openings 800 mm, walls 1600
  mm and overall 2400 mm off the outermost face; a walls string only when it
  has a point between its ends, an openings string only when the side has
  openings. Rooms: inside strings, 600 mm into the room, along each room's
  longest wall and the longest square to it, each wall and side once.
- After every command: references to deleted walls and grids drop, a string
  left with none (or an exterior string whose level has no building) goes,
  and every automatic string is rewritten. One the command moved to
  anything but what it measures was edited by hand: it becomes plain. One
  moved together with what it measures stays automatic. Changing only the
  offset keeps it automatic.
- Plain dimension strings follow walls at every witness point (D12).

The lot (`guhit_core::site`):
- `SetLotFromSurvey` reads quadrant bearings as PH titles write them ("N 45
  deg 30' E", "N. 45 deg. 30' E.", "N 45°30' E", "N 45-30 E", "S 12°05'30\"
  W", "N45d30'E", "Due North"), against true north (`north_angle_deg`). A
  bad one is refused (`bad_bearing`) naming the leg and the forms it reads.
  Legs that end farther than 1:100 of the perimeter from their start are
  refused (`lot_not_closed`); a smaller misclosure is spread by the compass
  rule and stated in the summary ("misclosure 4 mm (1:13,500), spread over
  the corners"). Corners are kept in the title's order from corner 1 (the
  start), clockwise or not, so side i is the title's line i to i + 1. An
  existing lot takes the new corners and keeps its id, grade and,
  when the count matches, its sides.
- `Derived::site`: area; per side its length, bearing ("N 45°30' E", to the
  second when it has seconds) and the nearest building footprint (any
  level); the setback outline (the lot offset inward by each side's
  setback); coverage: the lowest built level's footprints over the lot
  area, in percent, a fact.
- `building_over_setback` (warning): a footprint corner past a side's
  setback line, one item per side, id `building_over_setback:<lot>:<side>`,
  saying how far. Never a code check.

`MakeSheetSet`:
- First the marks it lacks, on the lowest built level: elevations "North",
  "East", "South", "West" (true north), each 5 m off that face, looking back
  at it (an existing elevation with that label, or looking the same way,
  counts); then sections up to two in all, "A", "B": along the first stair's
  run through its first flight, and along the longest room's longest side
  through its label (across it as well when there is no stair; ties go to
  the larger room). Each spans the buildings and 3500 mm past them.
- Then the sheets, landscape, scales left to fit: A-1 site plan and
  vicinity map (500 m); a plan per built level; the elevations two to a
  sheet (North and South, East and West); the sections; the roof plan with
  each level's reflected ceiling plan; the schedules (doors and windows,
  rooms, finishes, areas); then only for the services the project has (runs
  of that trade, or objects the schedule counts under it): E-1 lighting,
  E-2 power, P-1 plumbing, P-2 plumbing isometric, M-1 aircon, each with a
  viewport per built level.
- `replace` true keeps each sheet that comes back (by its views) with its
  number, id and revision marks; new sheets follow. Service sheets show only
  the levels with that trade; plumbing sheets need plumbing runs, the
  isometric water, drainage or vent runs. false adds only views on no sheet
  yet, numbered after the existing ones, and refuses
  when nothing is missing (`nothing_to_add`). A project with no building
  walls is refused (`nothing_to_draw`).

At IPC every refusal reads `invalid` (only `not_built` keeps its code);
the message carries the hint.

### Views (lane L3)

`guhit_export::views::items(p, d, view, scale)` draws every drawing
`ViewRef` as plan `Item`s in drawing mm: a plan, a services plan or the
reflected ceiling plan of a level, an elevation, a section, the roof plan,
the site plan, the vicinity frame and notes. A schedule (a table) and a
perspective (a render) answer `Failed`; the sheet lays those out.
`ViewItems { items, bounds, title, scale_denominator, images }`: `title` is
the view's own ("NORTH ELEVATION", "SECTION A-A", "GROUND FLOOR REFLECTED
CEILING PLAN"); `images` are pictures to place under the items over
`min..max` (the vicinity map), by file name in `SheetInputs::images`.
Plans are model mm, y north. Elevations and sections are x across the
picture plane to the viewer's right and y the height above project zero.

Elevations and sections come from `model3d::build_scene` with
`Derived::roofs` and `Derived::stairs` (`views::mesh`): walls with their
openings cut, door and window panels with their sash, louvre and panel
lines, slabs, roof decks (D26), columns, stairs, railings, roofs with
fascia and gable infill, site objects as boxes. Furniture is left out;
plants and cars are outlines (`section::figure_pieces`). Above 200k
triangles every solid becomes its box. `views::hlr` keeps feature edges
only (creases over 20 degrees, silhouettes, boundaries) and drops an edge
where a surface simply carries on into a coplanar face (two walls in line,
a wall over the slab edge under it); it hides with a uniform screen grid,
a face hiding what is more than 0.5 mm behind it, and joins collinear
pieces at 0.01 mm. A view mark looks to the left of start to end (`flip`:
the right), from its line: a section cuts there (poche, heavy) and is
cropped to its line; an elevation shows the whole model beyond its plane.
`depth_mm` clips beyond. Pens: cut heavy over solid poche; the profile
against open sky heavy (medium on a section); near edges medium, farther
light, beyond 6 m fine; sash lines, entourage, grids and datums fine. Every
elevation and section draws the grids that cross its plane with bubbles,
level datums ("+3.000 SECOND FLOOR"), the finish grade (the lot's
`grade_mm` under the lowest floor, else 150 mm), the top of the roof and
the marks of `Derived::opening_schedule` where the opening shows.

Roof plan: `Derived::roofs` planes and lines (eaves and rakes heavy,
ridges, hips and valleys medium), a downhill arrow with the pitch on each
plane, gutters, the walls under the roof dashed and lower floors' deck
edges. Site plan: the lot as a phantom line with numbered corners, each
side's bearing and distance (`Derived::site`, else worked out the same
way), setbacks as chain lines, the ground floor hatched, upper floors and
roof edges dashed, roads with name and width, site walls, lot area and
footprint share (facts, never a check) and the north arrow. Reflected
ceiling plan: walls cut, door heads, each room's ceiling height ("CH
2.700": the room's own, `room_schedule`, else the level's) and ceiling
finish, ceiling lights, smoke detectors and fans. Vicinity frame: the
image, its 500 m (`radius_m`) ring, the site at the center, north up.

Every plan (`plan::build_items`) also draws door and window tags from
`Derived::opening_schedule::marks`, grid lines with bubbles, dimension
strings (`Dimension::through`, a figure per segment) and the section and
elevation marks of its level. Site walls print as a light outline over a
grey band, and "UP" and "DN" move clear of the walls (G4 problem 11).

Results are cached inside guhit-export by a hash of the project JSON (it
changes with every revision): meshes and hidden line results, so drawing
the same view twice, or at another scale, computes it once.
`views::svg_of` and `views::write_items` ink items as the plan sheet does.
The sheets' DXF (`sheets_dxf`, lane L8's `dxf::sheets`) draws its views
from `views::items`. View linework is on the plan's categories, plus
`Cat::Roof` (`A-ROOF-OTLN`: roofs in elevations, sections and the roof
plan, slope arrows), `Cat::Grid` (`S-GRID`: grids, bubbles, level datums)
and `Cat::Symb` (`A-ANNO-SYMB`: door and window tags, view marks, north
arrows).

### Sheets and the PH title block (lane L4)

A sheet (`guhit_export::sheetset`) is its paper inside a border, the PH
title block as a strip down the right edge (`titleblock::strip_width`, 66
mm times the paper's type size: A3 1, A2 1.3, A1 1.6, A0 2), and the
drawing area left of it (`sheetset::Page`). Viewports live only in the
drawing area: a placed one (`position`, the content's center in paper mm
from the bottom left) is moved back inside it, and everything is clipped
to it, so nothing ever prints over the title block, at any paper size or
orientation. Unplaced viewports pack in order, left to right and top to
bottom, around the placed ones; viewports without a scale share the
largest common scale (`COMMON_SCALES`) at which the whole sheet still
packs; a stated scale is kept. Each viewport gets a title under it: a
bubble with its number on the sheet over the sheet number, the title (the
viewport's, else the view's: "NORTH ELEVATION", "SECTION A-A", "GROUND
FLOOR PLAN"), and "SCALE 1:100" with a scale bar, or "NOT TO SCALE" for
the vicinity map and perspectives; plans and the roof plan get a north
arrow. A sheet whose one viewport is a lighting, power, plumbing or aircon
plan, or the plumbing isometric, is that service sheet (legend, counts,
the trade's blank professional block) drawn left of the PH block.

Bodies: plans from `plan::build_items`; ceiling, services, elevation,
section, roof and site plans from `views::items` (L3), shown as a dashed
frame saying why while one cannot be drawn; schedules as tables
(`tables::schedule`, from `Derived::opening_schedule`, `room_schedule`,
`level_areas`, never counted again; a tall table continues in another
column with its header repeated, rows are never dropped); notes; the
vicinity map image (from `files/`) with the site marked and a north arrow,
the radius in its title; a perspective from Visuals. Images reach the
export through `SheetInputs::images`: the app (`documents::sheet_images`)
reads the firm logo and vicinity maps from the project's `files/` folder
and perspectives from Visuals, model views only; an AI image never goes on
a sheet.

The title block, top to bottom: firm (logo, name, address); project
(title, else the project name; owner and location from the project
settings; lot, block and TCT); revisions (the sheet's, in set order, at
least twelve ruled rows; then a blank remarks box on tall paper);
architect (name, PRC number and validity, PTR number with date and place,
IAPOA number with OR number and date, TIN); signature and dry seal, left
blank, with the printed name to sign over; sheet contents (title, SCALE
"1:100", "AS SHOWN" or "NTS", DATE, PAPER); sheet number with "3 OF 12"
(its place in the whole set). A field never typed prints blank. License
numbers print exactly as typed, smaller when long, never cut; the PDF on
disk prints them in full and `SheetInputs::mask_signatory` prints
`Signatory::masked()` (agents' pictures). The block never says a set
complies. Each field is a `<text class="tb-<field>">` (`tb-prc-no`,
`tb-tin`, `tb-sheet-number`, ...), so tests and readers find it.

The sheet SVG carries what the Sheets panel needs: `<g id="drawing-area"
data-x data-y data-w data-h data-page="W H">` and one `<g class="viewport"
data-id data-x data-y data-w data-h data-frame="x y w h" data-scale
data-placed>` per viewport (paper mm, y down; the content rect, and the
content with its title). A project with no sheets prints the default set
(`sheetset::default_set`: a site sheet when there is a lot, a plan per
level with walls, elevations and sections two to a sheet, the roof plan,
the schedules), never stored. `export_sheets` writes PDF as one file, SVG
as one file per sheet (`<name>-a-1.svg`, ...) and DXF through
`sheets_dxf`; file names carry the local time. `sheet_preview` and
`view_svg` answers are cached by project and revision in `documents.rs`.
Files: the project's own images in `projects/<id>/files/` (`file_store`
never overwrites, it picks a free name); the signing architect profile in
settings.json under `professional`, which fills the title block only when
a person asks (a `SetSignatory` from the window).

### AI render v2 (D53, lane L6)

Geometry stays the authority: an AI image is a record and a file, labelled
"AI visualization", and never writes back. The source of an AI image is
always a model view; the project look is a style reference only.

- **Conditioning maps.** The window draws a model view's maps from its own
  camera, in a renderer of its own (`src/viewer3d/render/conditioning.ts`,
  no requestAnimationFrame): `lines` (black on white where the element
  changes, the surface turns over 30 degrees or the depth jumps over 6%),
  `depth` (inverse, near light), `normal` (view space), `ids` (one color
  per element, 5 bits a channel, black sky) and `clay`, at most 1024 px on
  the long side. They are drawn in the background after every capture and
  render, with the roof and cutaway it used, and on the `capture_maps`
  window task. Saved as `renders/<id>.<map>.png`, listed in
  `RenderRecord::maps`, with `renders/<id>.maps.json` (`MapsMeta { width,
  height, visibility, legend: [{ color, element_id, pixels }] }`). Deleting
  the record trashes them with it.
- **The request.** Text first, then the capture, then the `lines` and
  `depth` maps, then the style reference (JPEG, at most 1536 px). The text
  is preamble, an image guide ("Image 2 is the model's line drawing ..."),
  model facts, style, user text, geometry line. Model facts come from Rust
  (`render_ai/facts.rs`): storeys, roofs, walls, the doors, windows, rooms
  (with finishes) and furniture in view from the ids legend, the view's
  time of day, and "Do not move, add or remove walls, doors or windows",
  with the counts in view. The title block never goes.
- **The project look.** `render_set_look` marks one still (usually an
  approved AI image) as `look`; later renders send it unless the request's
  `style_reference_id` names another, or is `""` or `"none"` for no
  reference. Deleting it clears the look.
- **Cost.** Auto mode uses Gemini Pro only for High or with a style
  reference; an explicit model setting wins. `render_ai_estimate` prices a
  request before anything is spent. Agents (`render_ai_generate` with
  `agent`, set by the MCP server) get `render_ai_agent_daily_cap` images
  per project per local day (settings.json, default 20, 0 to 1000, counted
  in `renders/ai-usage.json`); past it the call fails `rate_limited` with
  what the user can do. People are never capped.
- **Drift check** (`render_ai/drift.rs`). The AI image and the line map at
  384 px: model edges (direction from the structure tensor) look for an AI
  edge (strongest color channel, thinned) within 2 px and 34 degrees,
  after the best shift within 6 px. `DriftReport::missing` is the share
  left over, furniture and pipes not counted; `warn` above 0.35 or when a
  door, window, stair or column loses 60% of its edges, and the note names
  them. `renders/<id>.drift.png` keeps the unmatched edges. No line map:
  `drift` is None.
- **People decide before saving.** The window renders with
  `render_ai_draft` (not saved), keeps a clean image at once and shows a
  flagged one until the person keeps or discards it. Drafts last 30 minutes.

| Command (`render_ai::OWNS`) | Args | Returns |
|---|---|---|
| `render_ai_estimate` | `request`, `agent?` | `Estimate { model, model_label, image_size, price_usd, why, pro, style_reference_id, maps, maps_missing, prompt, agents }` |
| `render_ai_draft` | `request` | `Draft { pending_id, image, drift, overlay, estimate, seconds, provider }` |
| `render_ai_keep` | `pending_id` | `RenderRecord` |
| `render_ai_discard` | `pending_id` | `null` |
| `render_ai_generate` | `request`, `agent?` | `RenderAiResult`; with `agent`, capped, and the window draws missing maps first |
| `render_ai_maps_save` | `render_id`, `maps: [{kind, png}]`, `meta?: MapsMeta` | `RenderRecord` |
| `render_ai_map_data` | `id`, `kind` | data URL |
| `render_ai_drift_overlay` | `id` | data URL or null |
| `render_ai_usage` | | `AgentUsage { day, used, cap, default_cap }` |
| `render_ai_usage_set` | `agent_daily_cap` | `AgentUsage` |

`Estimate`, `Draft`, `AgentUsage`, `MapsMeta` and `LegendEntry` are contract
types in `guhit-model` (`api.rs`, generated bindings); `guhit_app::render_ai`
and `guhit_app::renders` re-export them. The typed calls are in
`src/contract/ipc.ts` (`renderAiEstimate`, `renderAiDraft`, `renderAiKeep`,
`renderAiDiscard`, `renderAiMapsSave`, `renderAiMapData`,
`renderAiDriftOverlay`, `renderAiUsage`, `renderAiUsageSet`), wrapped by
`aiRender` in `src/viewer3d/render/aiRender.ts`. A debug build with
`GUHIT_RENDER_AI_FAKE=faithful` or `moved` uses a local stand-in provider
that answers with the capture itself (graded, or with its right third slid
sideways), so the flow can be tried without a key or a provider call.

### Interop: DXF 2013 and IFC types and quantities (lane L8)

**DXF 2013 (AC1027)**, `guhit_export::dxf::{plan, plans, sheets}`, and the
services and isometric sheets (`plan_dxf` with those `sheet`s):

- UTF-8 text as typed ("Parañaque", "m²"), `$INSUNITS` 4 (mm), model
  space 1:1, y north, handles stable for the same project.
- Layers carry the plan's pens as lineweights: walls and site walls 0.35,
  doors, windows and columns 0.25, the rest 0.18; an entity drawn with
  another pen carries its own.
- Doors and windows are INSERTs of a block per type: the mark ("D1") for a
  typed opening, "DOOR-SWING-900" for an untyped one, "D1_WALL200" when a
  type sits in walls of another thickness. The block is the plan's own
  drawing of the opening; a flipped hinge or swing is the block mirrored.
  Each insert has invisible MARK, WIDTH, HEIGHT and SILL attributes.
  Objects are INSERTs of a block per catalog key and size with a NAME
  attribute; their labels stay loose.
- Dimensions are DIMENSION entities (rotated linear) with a style per scale
  (`GUHIT-100`, `GUHIT-100-M` for meters) and their `*D` blocks, drawn as
  the sheet draws them; a dimension string is one per segment on one line.
- Cut walls, columns and every solid ink fill are solid HATCHes in their
  outline.
- `plans` puts every level side by side; `sheets` lays each sheet's views
  in a row in set order, titled with their scale, plans drawn as above
  and other views from `views::items`. A view that cannot be drawn leaves
  its title and the reason.
- `plan_dxf` writes the plan through `dxf::plan` and `sheets_dxf` through
  `dxf::sheets`; drawings that are only items (the reflected ceiling plan)
  go through `dxf::write`, DXF 2013 without blocks or dimensions. Only the
  3D DXF stays R12.

**IFC4**, `guhit_export::model_ifc`:

- Qto sets on every wall, slab (floors, balconies, decks, landings, roof
  planes), space, door, window, roof and stair flight: lengths in mm,
  areas in m², volumes in m³, openings taken off walls and voids off floors.
- An IfcDoorType or IfcWindowType per type, named and tagged with its mark,
  untyped openings grouped by kind, style and size; an IfcWallType per
  thickness and material. Each defines its occurrences and is declared to
  the project. A typed door or window has its mark as Tag and as the
  Pset_*Common Reference.
- With `Derived::stairs`: an IfcStair of its shape (straight, quarter or
  half turn) aggregating one IfcStairFlight per flight, treads as steps,
  and IfcSlab .LANDING. landings 150 mm deep. With `Derived::roofs`: an
  IfcRoof over one sloped IfcSlab per plane and the gable ends as IfcWall
  .USERDEFINED. 'GABLEWALL' ("Gable wall N") centered on the wall below; a
  flat roof is its slab. The first roof keeps the Wave 1 GUIDs.
- One IfcGrid on the lowest storey (lines along the first one are U, the
  rest V; none when they all run one way), IfcSite latitude and longitude
  from `ProjectSettings::site` and LandTitleNumber from the title block
  TCT, an IfcSanitaryTerminal per WC, lavatory, shower, bathtub and sink.

### Catalog v2, placement and furnishing (lane L5)

The catalog (`crates/guhit-model/src/catalog.rs`, 108 items) gains 51:
43 photoscanned Poly Haven models (`sofa-leather` to `lamp-post-classic`),
five built-ins and three site kit objects drawn in code. Every furnishing
item names `rooms`, `tags` and `clearance_mm`; `model` is the pack model,
keyed by the catalog key in `public/assets/pack/manifest.json` (None: the
procedural form). The pack is 29.14 MB (40 MB cap, D51);
`node scripts/assets-build.mjs --check` fails on any unlisted pack file,
any source other than CC0 Poly Haven, ambientCG or Kenney, any model key
not in the catalog, or a pack over 40 MB.

Built-ins (`CatalogItem::parametric`, `Asset::built_in`): `kitchen-run`,
`wardrobe-builtin`, `vanity`, `counter`, `shelf-wall`, sized by the
object's own width, depth and height. Positions are from the left end
seen from the front (local -x). An asset without options draws with
`placement::built_in_defaults`, mirrored by `builtInDefaults` in
`src/viewer3d/scene/builtins.ts` (both tested against one table): a
kitchen run's sink at 30 percent rounded to 50 mm (450 mm from either
end), its cooktop at three quarters and at least 900 mm on (none under
1800 mm), wall cabinets from 1450 to 2150 mm, 350 deep; a vanity's basin
in the middle under a mirror cabinet (1150 to 1850 mm, 140 deep); about a
door per 450 mm (500 for a wardrobe). Validation (`built_in_options`):
only built-ins take options; a sink sits 150 mm or more from the ends,
only on a kitchen run, counter or vanity; a cooktop 300 mm or more from
the ends, only on a kitchen run or counter, and 600 mm or more from the
sink; at most one door per 250 mm.

Site kit: `gate-steel`, `hedge`, `lamp-post` (and the scanned `gate-iron`,
`lamp-post-classic`) are objects of `AssetCategory::Site` or `Plant` on the
`assets` layer. Paving is a deck slab (`SlabKind::Deck`) with the built-in
material `mat-paving-stone` (ambientCG grey pavers in the pack).

One rule set (`guhit_core::placement`) decides where an object may stand,
for `PlacementOptions`, review and furnishing alike: its footprint inside
the room's net floor (1 percent slack) and clear of wall islands and wall
outlines; clear of voids, stairs on their own level and stair openings on
the level they arrive at (the engine's stair geometry once L2 has it, the
straight flight's rectangle until then); clear of each doorway's zone
(900 mm deep on both faces, the leaf width on the swing side when wider,
50 mm past each jamb, plus the swing sectors) up to the door head; out of
a window's 150 mm band on either face when any part of it is higher than
the sill; clear of other objects whose height ranges meet (devices never
count); its front clearance free of objects standing below 1200 mm and
80 percent inside the room, and nobody else's front taken. Built-ins'
wall and mirror cabinets are their own boxes. Touches under 0.01 m2 (or
5 percent of the smaller footprint) do not count.

`Query::PlacementOptions { room_id, catalog_key }` answers
`{room_id, room, level_id, catalog_key, size, mount, clearance_mm,
options: [...], note, reason?, more?}`. Each option is
`{kind, position, rotation_deg, edge?, fit?, wall_id?, free_from_mm?,
free_to_mm?}`: `kind` "wall" (back against the wall of net edge `edge`,
10 mm off its face, `fit` the middle, start or end of a free stretch whose
extent along the edge is `free_from_mm` to `free_to_mm`), "center"
(tables, sets and cars, with 450 mm to walk round) or "ceiling". At most
48; `reason` says why when there is none. An unknown key is refused as
`unknown_catalog_key`, a room without closed walls as `room_not_closed`.

`Query::Catalog { search, usage }` answers `{count, search, usage, items,
note}`: items whose key, name, category or tags hold every word of
`search`; a usage lists the items that name it, and items for any room
only when a search narrows it. Built-ins carry `built_in_defaults`.

Review (suggestions): `asset_in_wall` (more than 0.005 m2 of the footprint
in a wall outline, under the wall's height; buried and through-window
objects excepted), `assets_overlap` (two objects whose boxes meet in plan
and height), and `asset_over_opening` for an object standing on a stair
flight or partly over a void or stair opening (the center over one stays
`slabs::review`'s finding).

Furnishing: `placement::furnish_room(project, room_id, picks, style)`
returns a `FurnishPlan { room_id, room_name, usage, assets, skipped }`
(new objects only, never an edit) and `commands()`, one `AddElement` each.
Without picks it uses the room's default set for its use, size and style
(modern, tropical, classic, industrial); with picks it places them in
order, anchors first (bed, sofa, run, desk, vanity, water closet), then
what goes with them (nightstands beside the bed, a coffee table in front
of the sofa, a console facing it, art above it, a chair at the desk). Ids
are stable for the same plan. Agents (D47) use it through L7's tools.
The copilot's `AiTask::Furnish { room_ids, style }` (empty: every closed
room on the active level) makes one model call with one tool,
`choose_furniture` (picks per room, from the keys for its use; unknown
keys dropped), then one proposal per room in `AiTurn::proposals`
(`proposal` is None), each a `Batch` labelled "AI: Furnish <room>" whose
dry run raises no `asset_in_wall`, `assets_overlap`, `door_swing_blocked`
or `asset_over_opening` on its objects. Every room's proposal waits in the
furnishing queue, which `ai_resolve` asks first (`ai::furnish::resolve`):
any room is applied or discarded on its own, in any order. One made
before another room went in is checked again on the plan as it is now and
applied only when it adds the same objects and raises none of those
items; otherwise it is `stale`. The first room is also the hub's pending
proposal. Asking again for one room makes no model call (the picks are
kept per room and style), gives the same objects with the same ids, and
replaces that room's proposal in the queue; a new chat turn clears the
queue. Built-in options are edited in the inspector
(`src/shell/fields/BuiltInFields.tsx`).

## IPC commands

One entry point: `AppService::handle(cmd, args) -> Result<Value, IpcError>`.
Args are a JSON object with the names below. The typed client is `src/contract/ipc.ts`.

| Command | Args | Returns | Notes |
|---|---|---|---|
| `hub_list` | | `ProjectMeta[]` | newest first |
| `hub_create` | `name`, `settings?`, `template?` | `DocState` | opens it. templates: `blank`, `sample-bungalow`, `plumbing-demo` ("Bungalow with services": T&B, 16 plumbing runs, 2 downspouts, a line set and condensate drain, and 32 objects including lights, switches, outlets, a panelboard and a split aircon) |
| `hub_open` | `id` | `DocState` | the answer may carry `repairs`: what opening mended (`Document::repairs`) |
| `hub_rename` | `id`, `name` | `ProjectMeta` | |
| `hub_duplicate` | `id` | `ProjectMeta` | |
| `hub_delete` | `id` | `null` | moves the folder to `trash/`, never hard-deletes |
| `hub_set_thumbnail` | `id`, `png` | `null` | |
| `hub_close` | | `null` | flushes autosave |
| `doc_state` | | `DocState or null` | |
| `doc_apply` | `command` | `ApplyResult` | autosaves |
| `doc_preview` | `command` | `ApplyResult` | |
| `doc_undo`, `doc_redo` | `force?`, `steps?`, `expected_revision?` | `DocState` | autosaves. `steps` (default 1) takes back or brings back that many as one revision; 0 is `bad_args`, more than there are `invalid`. In a live session someone else's step among them needs `force` (`other_author`). With `expected_revision`, a document that moved on from it fails with `stale` and nothing changes |
| `history_list` | | `HistoryList` | the undo and redo stacks with who made each step and when, newest first ("Edit history" under "Live sessions") |
| `doc_query` | `query` | JSON | |
| `doc_revision` | | `{revision, project_id}` | cheap poll for external changes |
| `snapshot_create` | `label` | `SnapshotMeta` | named version |
| `snapshot_list` | | `SnapshotMeta[]` | |
| `snapshot_restore` | `id` | `DocState` | takes an auto snapshot first; is undoable history reset; the answer may carry `repairs`: what opening mended (`Document::repairs`) |
| `catalog_assets` | | `CatalogItem[]` | |
| `export_plan` | `format`, `options`, `path?` | `ExportResult` | null path -> `<data>/exports/` |
| `export_image` | `png`, `name`, `path?` | `ExportResult` | |
| `underlay_store` | `file_name`, `data` | `{file_name}` | copies into the project folder |
| `underlay_data` | `file_name` | data URL | |
| `export_sheets` | the fields of `SheetExportOptions`: `format` (pdf, svg, dxf), `sheet_ids` (empty: all), `path?`, `date?` | `ExportResult` | one PDF for the whole set (L4, `documents.rs`) |
| `sheet_preview` | `sheet_id` | `SvgDrawing` | one sheet at paper size (L4) |
| `view_svg` | `view: ViewRef`, `scale?` | `SvgDrawing` | one view alone, `scale_denominator` used (L3 draws, L4 routes) |
| `file_store` | `file_name`, `data` (image data URL) | `{file_name}` | into the project's `files/` folder: firm logo, vicinity map (L4) |
| `file_data` | `file_name` | data URL | (L4) |
| `professional_get` | | `Signatory` | this computer's signing architect profile in settings.json; empty when unset (L4) |
| `professional_set` | `signatory` | `Signatory` | (L4) |
| `video_begin` | `tour_id`, `revision` | `{id}` | a tour video streamed from the window (L9, `video.rs`) |
| `video_append` | `id`, `data` (base64, at most 4 MiB) | `null` | (L9) |
| `video_finish` | `id`, `poster` (data URL), `info: VideoInfo` | `RenderRecord` | `renders/<id>.mp4`, kind `video` (L9) |
| `video_abort` | `id` | `null` | (L9) |
| `import_inspect` | `path` or `file_name`+`data` | `ImportInspection` | DXF; DWG when the converter is configured |
| `import_commit` | source + `options` | `ImportResult` | one undo step "Import <file>" |
| `model_store` | source | `{file_name, size}` | glTF/GLB/OBJ into `models/` |
| `model_data` | `file_name` | data URL | |
| `export_model` | `format` (ifc, dxf3d, dwg), `path?` | `ExportResult` | |
| `export_bytes` | `name`, `data`, `path?` | `ExportResult` | GLB/OBJ/DAE made by the frontend |
| `bundle_save` | `path?`, `videos?` | `ExportResult` | `.guhit` zip of the project folder; tour videos and their frames only with `videos: true` (D54) |
| `bundle_open` | source | `DocState` | copies into `projects/`, new id if it collides; the answer may carry `repairs`: what opening mended (`Document::repairs`) |
| `dwg_status`, `dwg_set_path` | , `path` | `DwgConverterStatus` | ODA File Converter, stored in settings.json |
| `render_styles` | | `RenderStyle[]` | |
| `render_list` | | `RenderRecord[]` | |
| `render_capture` | `camera`, `png`, `revision?`, `info?` | `RenderRecord` | Tier 1 capture or render. Tied to `revision` (the one a render started from, never newer than the document) or the current one. `info` says how it was made; width and height at least 1, seconds 0 or more |
| `render_data` | `id` | data URL | |
| `render_delete` | `id` | `null` | |
| `render_set_look` | `id` (null clears) | `RenderRecord[]` | the project look, AI renders' style reference (L6, `renders.rs`) |
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
| `mcp_status` | | `McpStatus` | the MCP server for the Connect agent dialog (DECISIONS D35) |
| `mcp_toolsets` | | `{toolsets, available}` | the extended toolsets every agent gets by default, and every toolset name ("MCP v2") |
| `mcp_set_toolsets` | `toolsets` | `{toolsets, available}` | sets them, stored in settings.json as `mcp_toolsets`; unknown names are dropped. An agent that names its own (`?toolsets=`, `--toolsets`) gets those instead |
| `mcp_set_enabled` | `enabled` | `McpStatus` | the "Allow agents" switch, stored in settings.json as `mcp_enabled` (default on). Takes effect at once: while off, every MCP request gets HTTP 503 and a JSON-RPC error (code -32000, `guhit_app::mcp::DISABLED_MESSAGE`), and the stdio entry passes that error on |
| `agents_list` | | `AgentSummary[]` | every agent working on the project now, this computer's and the session's, newest call first ("Agents") |
| `agent_update` | `id`, `name?`, `limit?` | `AgentSummary` | rename (kept in settings.json), or set the limit; `limit` null clears it, left out keeps it. Levels and elements must be in the open plan (`bad_args`) |
| `agent_pause` | `id` or null | `AgentSummary[]` | pause one agent, or with null every agent of this computer |
| `agent_stop` | `id` or null | `AgentSummary[]` | stop one or every agent; its claim ends |
| `agent_resume` | `id` or null | `AgentSummary[]` | end a pause or a stop |
| `swarm_estimate` | `request: SwarmRequest` | `SwarmCost` | spends nothing (L2; `not_built` until then) |
| `swarm_start` | `request: SwarmRequest` | `SwarmStatus` | an in-app swarm (L2; `not_built`). An MCP swarm starts with the `swarm` tool |
| `swarm_status` | | `SwarmStatus` or `null` | |
| `swarm_draft` | `since_step?`, `draft_revision?`, `swarm_id?` | `SwarmDraftState` or `null`; with `draft_revision`, `SwarmDraftReply` or `null` | the view and its diff against the model, the steps after `since_step` ("Swarms"). With `draft_revision` (the one the window drew, of `swarm_id` when given) and a draft that has not moved since: only `SwarmDraftCurrent { swarm_id, draft_revision, status }` |
| `swarm_pause`, `swarm_resume`, `swarm_stop` | `part?` (null: every part) | `SwarmStatus` | (L2; `not_built`) |
| `swarm_part_discard` | `part`, `with_dependents?` | `SwarmStatus` or `null` | (L1; `not_built`) |
| `swarm_check` | `parts?` (null: all) | `SwarmAcceptResult` | changes nothing (L1; `not_built`) |
| `swarm_accept` | `swarm_id`, `parts?` (null: all), `reviewed?` (part key to the `last_seq` reviewed), `accept_rest?` | `SwarmAcceptResult` | refused `steps_skipped` when a step of the parts would stay out, unless `accept_rest` ("Swarms") |
| `swarm_discard` | `swarm_id` | `null` | drops the draft, unlocks the model |
| `swarm_set_cap` | `cap_usd` | `SwarmStatus` | (L2; `not_built`) |
| `mcp_claude_desktop_bundle` | | `{path}` | writes the Claude Desktop extension `<data>/claude-desktop/guhit-studio.mcpb` (MCPB manifest 0.3, a binary server whose command is `stdio_command` with `--mcp-stdio`) and returns its absolute path. `unavailable` when there is no `stdio_command` (the dev bridge) |

`McpStatus { enabled, listening, port, url, stdio_command, last_client, claude_desktop_bundle }`:
`listening` is true while the MCP listener is bound, `port` is the port it serves on or tried
(the bridge's own port in the dev bridge), `url` is `http://127.0.0.1:{port}/mcp`, `stdio_command`
is the absolute path of the desktop executable (None in the dev bridge), and `claude_desktop_bundle`
is true when `stdio_command` is set. `last_client` is an `McpClientSeen { name, version, at }`: the
last `clientInfo` the server saw (from `initialize`, from `_meta` of a 2026-07-28 request, or from
the stdio entry, which forwards its client's), with `at` (RFC 3339 UTC) moved on every later request.
Requests over HTTP carry no session, so a request that does not name itself counts for the last
client that did. In memory only, since the app started. The transports report into
`AppService::mcp` (`set_port`, `set_listening`, `set_stdio_command`).

External changes: after every commit, undo, redo, open, create, close, delete and restore, a rename of the open project, and every document a live session guest receives, `AppService::watch_changes()` fires `{revision, project_id, seq}`. The desktop shell forwards it as the Tauri event `doc_changed {revision}`; the dev bridge serves `/mcp` on its port and the UI polls `doc_revision`. The frontend subscribes with `onDocChanged` (`src/contract/ipc.ts`): `App.tsx` switches hub to editor, `EditorShell` refetches state. Full MCP tool list: `docs/MCP.md`.

Errors are always `IpcError { code, message, element_ids }`. Codes: `not_found`, `invalid`, `no_document`, `stale`, `io`, `ai_not_configured`, `ai_failed`, `unknown_command`, `bad_args`, `forbidden` (dev bridge, non-localhost origin), `other_author` (undo or redo of someone else's step without `force`), `not_live`, `host_only`, `live_refused` (wrong secret, session full, other version), `live_unreachable` (neither an address nor the relay reached the host; the message says why), `live_pin` (the host's certificate does not match the invite), `live_lost` (the connection to the host dropped), `no_window` (no window answered a window request), `not_built` (a Wave 2 command, query or drawing whose lane has not built it yet), `unavailable` (needs the desktop app, such as the Claude Desktop extension), and for agents ("Agents") `agent_paused`, `agent_stopped`, `too_many_agents`, `claimed`, `rate_limited`, `out_of_scope` (outside an agent's limit), `not_yours` (another computer's agent), `project_changed` (another project was opened while the call ran), and for swarms ("Swarms") `swarm_running` (a write to the model while a draft is open), `draft_open` (a second swarm), `live_session` (a swarm in a live session, any `swarm_*` call on a guest), `part_stopped` (a stopped part's agent, or a handle of a swarm that ended), `draft_view` (a window-backed output for an agent drawing in a draft), `people_only` (an agent tried to accept), `cross_part` (a step that would make two parts depend on each other), `steps_skipped` (an accept that would leave steps of its parts out, without `accept_rest`). An AI edit outside its scope has no IPC code of its own: the model reads a tool error that starts with `out_of_scope:`, and a proposal that stops fitting its scope before it is applied is `invalid`.

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
  claude-desktop/guhit-studio.mcpb   # Claude Desktop extension, written by mcp_claude_desktop_bundle
  settings.json           # also profile_name, live_port, live_relay, live_direct, mcp_port, mcp_enabled
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
- `useApp().undoConfirm` holds an undo or redo the engine refused as someone else's (`other_author`); the shell asks and calls `resolveUndoConfirm`. `undoSteps(n, expectedRevision?)` and `redoSteps(n, expectedRevision?)` take several steps as one revision (History's "Undo to here", D38), asking through `undoConfirm` when the engine says `other_author`. `UndoConfirm::steps` says how many and `revision` the revision the question was asked at; "Undo anyway" sends it, and a `stale` answer turns into a toast.

### Wave 2 hooks (C0)

One-line hooks the hubs above call into each lane's module (lane in
brackets); the lanes own the modules, the orchestrator the hubs. File and
line detail is in the C0 handoff.

| Hub | Hook | Lane module |
|---|---|---|
| `src/contract/ipc.ts` | `exportSheets`, `sheetPreview`, `viewSvg`, `fileStore`, `fileData`, `professionalGet`, `professionalSet`, `renderSetLook`, `videoBegin`, `videoAppend`, `videoFinish`, `videoAbort` | Rust lanes above |
| `src/state/bus.ts` | `open_sheets`, `open_view` [L4], `play_tour` [L9] | panels open themselves |
| `src/state/bus.ts` | `swarm { part }`: show the AI dock's Swarm tab [swarm L4] | `src/swarm/` |
| `src/contract/ipc.ts` | `swarmEstimate` to `swarmSetCap`, the `swarm` app event [swarm C0] | `crates/guhit-app/src/swarm/` |
| `src/state/store.ts` | tools `grid`, `dimension_string`, `view_mark`, `lot` [L1], `roof` [L2] | `src/editor2d/laneTools.ts` registry |
| `src/editor2d/controller.ts` | tool dispatch | `editor2d/permit/{gridTool,dimStringTool,viewMarkTool,lotTool}.ts` [L1], `editor2d/{roofTool,stairTool}.ts` [L2] |
| `src/editor2d/render.ts` | draw passes | `editor2d/permit/draw.ts` [L1], `editor2d/drawRoofsStairs.ts` [L2] |
| `src/editor2d/hit.ts` | stair hits | `editor2d/hitRoofsStairs.ts` [L2] |
| `src/shell/ElementFields.tsx`, `Inspector.tsx` | element fields, project sections | `shell/fields/*.tsx` [L1, L2], `shell/TypesPanel.tsx`, `SchedulesSection.tsx` [L1], `shell/sheets/TitleBlockSection.tsx` [L4], `shell/tours/{ToursSection,TourFields}.tsx` [L9] |
| `src/shell/EditorShell.tsx`, `TopBar.tsx` | panels and buttons | `shell/sheets/*`, `shell/views/ViewsPanel.tsx` [L4], `shell/tours/*` [L9], `shell/TypesPanel.tsx` [L1] |
| `src/viewer3d/scene/buildScene.ts` | scene builders | `scene/{roofs,stairs}.ts` [L2], `scene/{builtins,siteKit}.ts` [L5], `tour/pathPreview.ts` [L9] |
| `src/viewer3d/engine/ViewerEngine.ts` [L6] | walk, assets and tour hooks in the one frame loop | `walk/entry.ts` [L2], `scene/pack.ts` [L5], `tour/player.ts` [L9] |
| `src/shell/windowTasks.ts` [L7] | `capture_maps`, `export_tour`, `capture_view.camera` | `render/conditioning.ts` [L6], `tour/export.ts` [L9] |
