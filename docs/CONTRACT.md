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
- Stairs: a straight flight has `riser_count - 1` treads of `run_mm / (riser_count - 1)`, so treads times tread length is the run and the top riser lands on the floor above (DECISIONS D44). The inspector, the IFC, the plan and the exports count this way. See "Slabs and railings". Annotations: `position` is the left end of the first baseline.
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
  reach past the footprints, less what voids, balconies and decks cover
  inside them (a loggia or lanai inside the walls is open floor, not gross
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

**Tools.** The core toolset `draft` (22 tools) is always listed; the toolsets
`services`, `visuals`, `live`, `review` and `interop` (11 more) are listed
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
`unknown_catalog_key`, `no_room_here` or `unavailable`. `step` is 1-based in
a `batch` or `build`, absent for one call. The text content keeps the v1
wording ("step 5 of 6 (`add_door`): invalid: ... Nothing was applied.") and
adds "Hint: ...". An unknown tool names the nearest v2 tool.

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
| `doc_undo`, `doc_redo` | `force?`, `steps?`, `expected_revision?` | `DocState` | autosaves. `steps` (default 1) takes back or brings back that many as one revision; 0 is `bad_args`, more than there are `invalid`. In a live session someone else's step among them needs `force` (`other_author`). With `expected_revision`, a document that moved on from it fails with `stale` and nothing changes |
| `history_list` | | `HistoryList` | the undo and redo stacks with who made each step and when, newest first ("Edit history" under "Live sessions") |
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
| `mcp_status` | | `McpStatus` | the MCP server for the Connect agent dialog (DECISIONS D35) |
| `mcp_toolsets` | | `{toolsets, available}` | the extended toolsets every agent gets by default, and every toolset name ("MCP v2") |
| `mcp_set_toolsets` | `toolsets` | `{toolsets, available}` | sets them, stored in settings.json as `mcp_toolsets`; unknown names are dropped. An agent that names its own (`?toolsets=`, `--toolsets`) gets those instead |
| `mcp_set_enabled` | `enabled` | `McpStatus` | the "Allow agents" switch, stored in settings.json as `mcp_enabled` (default on). Takes effect at once: while off, every MCP request gets HTTP 503 and a JSON-RPC error (code -32000, `guhit_app::mcp::DISABLED_MESSAGE`), and the stdio entry passes that error on |
| `agents_list` | | `AgentSummary[]` | every agent working on the project now, this computer's and the session's, newest call first ("Agents") |
| `agent_update` | `id`, `name?`, `limit?` | `AgentSummary` | rename (kept in settings.json), or set the limit; `limit` null clears it, left out keeps it. Levels and elements must be in the open plan (`bad_args`) |
| `agent_pause` | `id` or null | `AgentSummary[]` | pause one agent, or with null every agent of this computer |
| `agent_stop` | `id` or null | `AgentSummary[]` | stop one or every agent; its claim ends |
| `agent_resume` | `id` or null | `AgentSummary[]` | end a pause or a stop |
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

Errors are always `IpcError { code, message, element_ids }`. Codes: `not_found`, `invalid`, `no_document`, `stale`, `io`, `ai_not_configured`, `ai_failed`, `unknown_command`, `bad_args`, `forbidden` (dev bridge, non-localhost origin), `other_author` (undo or redo of someone else's step without `force`), `not_live`, `host_only`, `live_refused` (wrong secret, session full, other version), `live_unreachable` (neither an address nor the relay reached the host; the message says why), `live_pin` (the host's certificate does not match the invite), `live_lost` (the connection to the host dropped), `no_window` (no window answered a window request), `unavailable` (needs the desktop app, such as the Claude Desktop extension), and for agents ("Agents") `agent_paused`, `agent_stopped`, `too_many_agents`, `claimed`, `rate_limited`, `out_of_scope` (outside an agent's limit), `not_yours` (another computer's agent), `project_changed` (another project was opened while the call ran). An AI edit outside its scope has no IPC code of its own: the model reads a tool error that starts with `out_of_scope:`, and a proposal that stops fitting its scope before it is applied is `invalid`.

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
