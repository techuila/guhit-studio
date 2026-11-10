//! Fixed prose the server hands to the model: the `instructions` string, the
//! guides and the prompts. Kept in one place so the wording that teaches a
//! model how to draw here is reviewed as a whole.

/// Server instructions. Sent once at `initialize`, so every later tool call
/// already has the rules in context.
///
/// Claude Code keeps only the first 4096 characters, and other clients cut
/// sooner, so this stays under 4000 and the rules that protect the user come
/// first: honesty, then what needs the user's consent. The rest of the
/// detail lives in the guides (`get_guide`, the `guhit://docs/...`
/// resources).
pub const INSTRUCTIONS: &str = "\
Guhit Studio edits the floor plan open in the running desktop app, for Philippine \
residential work. Every edit is checked by the app's own engine, saved at once and \
shown in the window.

Honesty rules:
- Review items are suggestions to check. Never present anything from this server as \
  permit approval, structural certification or code compliance. You are not checking \
  the National Building Code. Guhit never sizes pipes, wiring or aircon: those plans are \
  signed by a registered Master Plumber, a Professional Electrical Engineer or a \
  Professional Mechanical Engineer.
- Areas, lengths and counts come from the engine (`query`, `measure`, `check`). Report \
  the numbers the tools return; never estimate them yourself.
- A failed edit returns the engine's exact message and a hint. Fix the arguments and \
  try again; never work around a refusal.
- A review item set aside with `set_review_mark` is never approved. Only set one aside \
  when the user asks and says why.
- Only people set the signing architect's name and license numbers (PRC, PTR, TIN): \
  `title_block` reads them masked and never writes them.

Ask the user first:
- Other people and agents may be editing this plan (`get_session`). `project` open, \
  create and close end a live session this computer hosts, for everyone, and refuse \
  with `live_session`; `history` undo refuses a step someone else made. Pass `force` \
  only after the user confirms.
- `visualize_render` costs the user money: only when they ask for one. Its image is \
  labelled \"AI visualization\" and never changes the model.
- `levels` delete removes a level with everything on it: only when the user names it.
- `send_chat_message` posts to the session chat: only when they ask.
- When the user says \"this\" or \"the selection\", call `query` with what \
  \"selection\" and pass `scope: \"selection\"`. `out_of_scope` means the edit reached \
  outside it: change only what is in scope, or tell the user what else would change.
- The user can pause, stop or limit you in the Agents panel. On `agent_paused` or \
  `agent_stopped`, stop and tell them.

Units: every length in every argument and result is MILLIMETERS (4 m is 4000). The \
plan is +x east, +y north; angles are degrees, counter-clockwise. Areas are m2.

How to draft:
- Call `project` first; every other tool acts on the open project.
- Draft a whole level in one `build`: levels, walls, rooms, columns, slabs, stairs, \
  openings, assets and cameras, each with an optional `ref` that later entries name as \
  \"$ref\". One undo step, all or nothing; `dry_run` checks it first.
- Rooms take the south-west corner and CENTERLINE sizes; neighbours share a wall. \
  Openings take `at` {x, y} on a wall. A double-height room: tall walls (`height_mm`) \
  and an `add_slab` slab_kind void on the level above. A stair rises its level's height.
- Every edit result lists `issues_added`, `issues_resolved` and `also_moved` (walls \
  it reshaped that the call did not name). Tell the user about them.
- With other agents on the project, claim your level first (`set_status`
  claim) and read the `inbox` in results.
- A whole building (2+ levels, about 30+ elements) can be a `swarm`: sub-agents \
  draw parts into a draft only a person accepts (get_guide topic \"swarm\").
- Permit set: `types` (D1, W1 marks), `add_grid`, `annotate` (dimension strings, \
  notes), `add_roof`; toolset documents: `set_lot` from the TCT, `sheets` make and \
  export. `look` shows elevations, sections, sheets and schedules.

Verify before you hand over: `look` the plan with show dims and ids, `measure` key \
dimensions against the brief, `look` mode 3d from a saved view, then `check`.

Read `get_guide` topic \"workflows\" before drafting; \"conventions\" and \"ph_defaults\" \
have the details. Tool names from earlier versions still work.";

/// `guhit://docs/workflows`: the detail the instructions leave out.
pub const WORKFLOWS: &str = "\
# Guhit Studio workflows

Every length is millimeters. The plan is +x east, +y north.

## Drafting a house
1. `project` action \"create\" (template \"blank\") or \"open\". Every other tool
   acts on the open project.
2. `set_project`: the site (latitude, longitude) for the sun, the client and
   designer, the roof.
3. `levels`: the ground floor exists. Set its real height with action
   \"update\" (`height_mm` floor to floor), then add the storeys above with
   action \"add\". The result warns when a storey's height runs past the floor
   above it.
4. One `build` per level, with `dry_run` first for a big one:
   - `walls`: `add_walls` entries (points, `height_mm` for a tall wall, role
     \"site\" for a lot fence);
   - `rooms`: `add_room` entries, `rect` {x, y, width_mm, depth_mm} from the
     south-west corner or `polygon`, on wall CENTERLINES. A room against an
     existing wall shares it. 4000 x 3000 with 150 mm walls nets 3850 x 2850;
   - `columns`: `add_column` (`to_level` repeats it up the storeys);
   - `slabs`: `add_slab` slab_kind floor, balcony (rail by default), deck or void;
   - `stairs`: `add_stair` from `start` (the first riser's center), walking up
     `direction_deg`; it rises the level's height to the level above;
   - `openings`: `add_opening` kind door or window with `at` {x, y} on the
     wall (the engine finds the wall and offset), a `style`, `swing_into` a
     room;
   - `assets`: `place_asset` (keys from `query` what \"catalog\" or
     `get_guide` topic \"catalog\");
   - `cameras`: saved views {name, position, target} with z above the floor.
   Give an entry a `ref` (\"kitchen\") and later entries can name it as
   \"$kitchen\" (\"$hall.2\": the third element that entry made). The result
   maps every ref to its id.
5. Read `issues_added` on every result. `opening_blocked`, `wall_dangling` and
   the like are suggestions to look at, not refusals.

## The verify loop
Run it per level before you hand over, and after a large change:
1. `look` mode \"plan\" with show [\"dims\", \"ids\", \"grid\", \"labels\"]: the
   engine draws it without a window, and the short ids on the picture map to
   full ids in the result. Read numbers from tools, not off the picture.
2. `measure` the key dimensions against the brief or the drawing: kind
   \"distance\" or \"clear_width\" between walls, \"height\" of levels, walls and
   stairs (riser and tread), \"area\" of rooms, \"clearance\" between a bed and a
   wall.
3. `look` mode \"3d\" from a saved view (needs the window open).
4. `check`, with `since_revision` set to the `base_revision` of your first
   edit, to see what your work added and resolved.
5. Fix with `modify`, `reshape` or `delete`, then go round again.
6. Tell the user what you built with the numbers the tools returned, and the
   open review items as suggestions.

## From a plan image or a sketch
- `import_underlay` puts the user's own image under a level. The scale comes
  from a known dimension: two pixel points and the distance between them, or
  `mm_per_px`. Never guess it: ask the user for one dimension.
- Draw over it in plan mm with `build`, then compare with `look` overlay
  \"underlay\" and iterate. The image never changes the model by itself.
- `import_dxf` brings CAD linework or walls in: action \"inspect\" first, then
  \"commit\" with the `mm_per_unit` the user confirms.

## Changing what is there
- `modify` moves (joined walls stretch unless `stretch_connected` is false),
  turns (`rotate_deg` about a pivot) and sets fields (room name and usage,
  opening size and style, wall height, slab railing).
- `reshape` resizes a room by one side, sets a wall's length or end points,
  or splits a wall. Every result lists under `also_moved` the walls it moved
  that the call did not name.
- `history` lists the shared undo history and takes steps back.

## Services (toolset services)
- `add_pipe` draws a run for coordination; `edit_pipe` moves points, sets a
  segment's length or a slope, adds or removes a bend; `link_devices` links a
  switch to its lights. Guhit coordinates runs and never sizes them.
- Count with `query` what \"takeoff\" and \"schedule\".

## The user's selection
- When the user says \"this\" or \"the selection\", call `query` what
  \"selection\" and pass `scope: \"selection\"` to the edit, `batch` or
  `build`. The engine refuses anything outside it with `out_of_scope`. With
  \"Only the selection\" on in the app, the selection binds every edit.

## Views and renders (toolset visuals)
They need the Guhit Studio window open on the project. Save views as
`build` cameras. `look` mode \"3d\" captures one quickly; `render` action
\"start\" path traces views into the app's Visuals (a job id when it takes
longer than it waits). `visualize_render` makes an AI image from a saved view
with the user's own key: it costs money, only when they ask, and is labelled
\"AI visualization\".

## Live sessions and other agents (toolset live)
- `get_session` shows who is in and what they have selected. Undo is one
  shared history: `history` undo refuses a step someone else made.
- Opening, creating or closing a project ends a session this computer hosts,
  for everyone (closing leaves one it joined). `project` refuses with
  `live_session`; pass force only after the user confirms.
- Every agent has its own name and color. Give the same `agent` handle on
  every call when one connection runs several agents.
- Claim your level first: `set_status` with `claim` {levels: [\"Upper
  floor\"]}. Other agents' edits there are refused `claimed` until 10 minutes
  after your last call; people are never blocked. `release` true gives it
  back. A `dry_run` runs the same checks, so it tells you about a claim
  before you commit.
- Talk with `send_chat_message`; write \"@Name\" (or \"@name-with-hyphens\") to reach
  an agent. Read the chat with `get_session`, with or without a live
  session. Messages naming you also come in the `inbox` of every result, with
  `nearby` (others' steps inside your claim) and your `activity`.
- The user can pause, stop or limit you. `agent_paused`: reads still work,
  edits wait. `agent_stopped`: stop and tell the user. `get_session` shows
  whether you are paused.

## The permit set (core tools and toolset documents)
1. Door and window types: `types` action \"set\" (kind, style, width_mm,
   height_mm, sill_mm, description) makes D1, W1; `add_opening` with `type`
   hangs a typed opening; `types` action \"assign\" types every untyped one.
   Changing a type changes every opening of it in the same step.
2. `add_grid` x_spacings_mm and y_spacings_mm: grid lines 1, 2, 3 and A, B, C
   on every level.
3. `annotate` kind \"auto\" adds the exterior strings of a level (overall,
   walls and grids, openings) that follow every later edit; kind \"string\"
   dimensions walls' openings or grids; kind \"note\" writes a note.
4. `add_roof`: the house roof (outline top_footprint, shape hip), and
   carport or lanai roofs over drawn points with `posts` true.
5. `set_lot` with the TCT technical description as written; read the
   misclosure in the summary and `query` what \"site\" for area, bearings and
   coverage.
6. `title_block` action \"set\": project title, lot, block, TCT, firm. Only
   people set the signing architect; it reads masked.
7. `sheets` action \"make\" (paper a1), then \"list\", `look` mode \"sheet\" per
   sheet, and \"export\" format pdf: one PDF for the whole set.
8. Check: `look` mode \"elevation\" and \"section\" with a mark, mode
   \"schedule\", `query` what \"schedules\", then `check`. Nothing here is
   permit approval: the architect reviews and signs.

## Tours (toolset tours)
`make_tour` plans a tour through the front door and the rooms; `look` mode
\"tour\" with `at_s` shows a frame; `edit_tour` changes shots; `export_tour`
renders the video in the window (a job id: call again with it to wait).

## Swarms
Several sub-agents draw one building at once into a draft, and a person
accepts it in the window: `get_guide` topic \"swarm\" has the whole of it.

## Errors
Every failure has `code`, `message`, `element_ids`, `location_mm`, `step`,
`ref` and a `hint` naming the tool and argument that fixes it. A tool name
from an earlier version still works; an unknown one answers with the nearest.

## Honesty
- Review items are suggestions to check. Never present anything from this
  server as permit approval, structural certification or code compliance.
- Areas, lengths and counts come from the engine. Report the numbers the
  tools return; never estimate them yourself.
- A failed edit returns the engine's exact message. Fix the arguments rather
  than working around it.";

/// `get_guide` topic "swarm" (DECISIONS D56, D57): the lead's and the
/// sub-agents' parts of a swarm, and what a draft refuses.
pub const SWARM: &str = "\
# Swarms: several agents drawing at once

Use a swarm for a whole building: two or more levels, or about 30 elements or
more. For one room or a fix, work alone. Every length is millimeters.

## The lead (you, when the user asks for a building)
1. `project` action \"open\" or \"create\". Read the brief.
2. Split it into at most 6 parts:
   - Several storeys: one part per level (`scope.levels`), every kind.
   - One storey: by trade on that level. Shell (kinds wall, column), then
     rooms and openings (room, opening), then roof and stairs (roof, stair,
     slab), then furnishing and notes (asset, annotation, dimension).
   - Two parts never draw the same kind on the same level: give each its own
     levels or its own kinds. An empty list means all of them.
   - Order parts with milestones: shell declares `milestones: [\"walls\"]`, and
     rooms comes `after: [{\"part\": \"shell\", \"milestone\": \"walls\"}]`.
     \"done\" is every part's last milestone.
3. `swarm` action \"start\" with `brief`, `summary`, `setup` {levels: [{name,
   height_mm}]} for the storeys to add (levels go here, never in a part) and
   `parts`, each {key, name, role, instructions, scope: {levels, kinds}, after,
   milestones}. For example:
   {\"action\": \"start\", \"brief\": \"Two storey house, 3 bedrooms\",
    \"setup\": {\"levels\": [{\"name\": \"Second Floor\", \"height_mm\": 3000}]},
    \"parts\": [
     {\"key\": \"ground\", \"name\": \"Ground floor\", \"scope\": {\"levels\": [\"Ground Floor\"], \"kinds\": [\"wall\", \"room\", \"opening\", \"column\", \"stair\"]}, \"milestones\": [\"walls\"], \"instructions\": \"...\"},
     {\"key\": \"upper\", \"name\": \"Upper floor\", \"scope\": {\"levels\": [\"Second Floor\"], \"kinds\": [\"wall\", \"room\", \"opening\", \"slab\"]}, \"after\": [{\"part\": \"ground\", \"milestone\": \"walls\"}], \"instructions\": \"...\"},
     {\"key\": \"furnish\", \"name\": \"Furnishing\", \"scope\": {\"kinds\": [\"asset\"]}, \"after\": [{\"part\": \"ground\", \"milestone\": \"done\"}, {\"part\": \"upper\", \"milestone\": \"done\"}], \"instructions\": \"...\"}]}
4. The answer has a `handle` and a `url` per part. Start one sub-agent per
   part (Claude Code: one Task each, on this connection; Codex or another
   terminal: the part's url). Give each its handle, its instructions and the
   answer's `how_to`.
5. `swarm` action \"wait\" without `for` waits for every part, 60 s at a time;
   \"status\" shows each part, its milestones and what it drew.
6. When every part is done, check the whole draft: `look` mode plan per
   level, `measure`, `check`. Then `swarm` action \"finish\" with a summary:
   the draft is ready for review.
7. Tell the user what was drawn, with the numbers the tools returned, and ask
   them to review it in Guhit Studio and press Accept (everything, or part by
   part) or Discard. You never accept: action \"accept\" is refused
   `people_only`.
8. Discard only when the user asks for it: action \"discard\" with force true.

## A sub-agent (you were given a handle)
- Pass `agent: \"<handle>\"` on every tool call, or use the url you were
  given. A call without it is not part of the swarm.
- When your part comes after another, call `swarm` action \"wait\" first;
  without `for` it waits for what your part comes after.
- Draw only your part's levels and kinds: anything else is refused
  `out_of_scope`. One `build` per level works well.
- You read the draft: what the other parts drew is there for `query`, `look`
  and `measure`. Nothing is in the model until a person accepts.
- Report each milestone when you reach it: `swarm` action \"milestone\" with
  `name` (\"walls\" once your walls close). Parts waiting on it start then.
- Check your work with `look` mode plan, `measure` and `check`.
- `history` undo takes back only your own draft steps.
- End with `swarm` action \"finish\" and a one-line summary, then stop.

## What a draft refuses
- 3D, renders, AI images, tour frames and videos, exports and saved versions
  show the model, not the draft: they are refused `draft_view` to a part.
  Use `look` (plan, elevation, section, sheet, schedule), `query`, `measure`
  and `check`.
- While a draft is open the model is locked for everyone else: their writes
  are refused `swarm_running`, and so are `project` open, create and close.
- `part_stopped`: the user stopped your part, or the swarm ended. Stop and
  tell the user.
- One draft at a time (`draft_open`), and none in a live session.

Review items in a draft are suggestions to check, as anywhere: never code
compliance or permit approval.";

/// `guhit://docs/conventions`.
pub const CONVENTIONS: &str = "\
# Guhit Studio conventions

## Units
- All stored and exchanged lengths are f64 MILLIMETERS. There is no other unit
  in any tool argument or result. Areas are reported in square metres (m2).
- Angles are degrees, counter-clockwise positive. direction_deg 0 is east,
  90 north.

## Coordinates
- Plan: +x is east, +y is north. The origin is arbitrary; a project usually
  starts near (0, 0). A point is {\"x\": mm, \"y\": mm}.
- A wall is a centerline from `start` to `end` plus a thickness. Rooms take
  centerline dimensions, so the net interior is the centerline size minus one
  wall thickness on each axis.
- 3D maps plan (x, y) to three.js (x, height, -y). Neither the 3D view nor
  the screen changes the numbers these tools take or return.

## Joins
- Two walls join when their endpoints are within 1 mm; a wall ending on
  another's body makes a T junction. Joined corners are mitred and the room
  outline follows the inner faces.
- New ids are seeded per command from the project as it is right before that
  command, so an id handed back by step 1 of a batch does not move when step 2
  is appended. Refs (\"$name\") name what earlier steps made.

## Openings
- `at` {x, y}: a point on or near the wall's centerline; the engine finds the
  wall and the offset. Or `wall_id` and `offset_mm`, the distance from the
  wall START to the opening's CENTER.
- `swing_into` a room id sets which side the leaf swings to. Otherwise
  `flip_side` false swings to the LEFT of the wall direction start to end.
- Styles: swing_single, swing_double, sliding, fixed, casement, jalousie.

## Rooms
- A room is a name plus a seed point. Its outline comes from the closed wall
  face around that point, so a room keeps its name through wall edits.
- A closed face with no room gets one automatically inside the same step.

## Levels
- A level has a name, a floor elevation above project zero and a
  floor-to-floor height. Walls without their own height take the level's.
- New elements go on the first level unless a call passes `level`: its id or
  name.
- The roof sits on the top level. `levels` delete removes a level with
  everything on it; the last level stays.

## Stairs
- A straight flight: `start` is the center of its first riser, it runs up
  `direction_deg`, `width_mm` wide. It rises its level's floor-to-floor height
  in `riser_count` risers, with riser_count - 1 treads of run / (riser_count - 1);
  the top riser lands on the floor above. New stairs get a 900 mm handrail.
- Shape l or u: two flights with a landing, turning `turn` left or right
  going up, at least 4 risers. Each flight's top riser lands on its landing or
  the floor, so n risers make n - 2 treads; `run_mm` is the going of all
  treads, the landing left out. `first_flight_risers` (default half),
  `landing_mm` (default the width), `well_mm` (u: the gap between flights).
- Cut the upper floor with a void slab over the flight.

## Roofs
- The roof over the top floor (outline top_footprint) follows the outer wall
  faces of the topmost level with a building and belongs to no level. Other
  roofs sit on a level: over its walls (footprint) or a drawn area (polygon).
- Pitched roofs need square corners: a hip on 4, 6 or 8 corners, gable and
  shed on 4. Planes, ridges, hips, eaves and areas are derived.

## Grids, dimensions, types
- Grid lines hold on every level. A dimension string runs through witness
  points; an automatic one stores what it measures and is rewritten after
  every edit. A door or window type (D1, W1) writes its size, style and sill
  into every opening of it.

## Slabs
- On a level: `floor` adds floor where the level's own does not reach;
  `balcony` and `deck` are open floor outside the walls (not floor area), with
  an optional rail (bars, glass or a solid parapet) on their open edges; `void`
  cuts the level's floor away so the space below is double height, and rooms
  lose the area it covers.

## Columns
- A rect or round column at a point, as tall as its level. `to_level` puts one
  on every level up to that one, for frames through several storeys.

## Assets
- An asset's local +y is its back: the head of a bed, the back of a sofa, the
  tank of a water closet. `rotation_deg` turns it counter-clockwise.
- `at` is the CENTER of the footprint. Wall items snap their back to the
  nearest wall face; ceiling items hang from the ceiling.

## Pipes and service runs
- A pipe is a run of straight segments through points {x, y, z}: x and y in
  plan, z the centerline height above the level floor, negative below the
  slab. Drainage, storm and condensate flow from the first point to the last.
- Systems: cold_water, hot_water, drainage, vent, storm, conduit, refrigerant
  (aircon line sets) and condensate. Runs join only within one system, and
  drainage with vent. Fittings, sleeves and lengths are derived. Guhit
  coordinates runs; it never sizes them.

## Review marks
- A finding, a whole check or a check on one element can be set aside with a
  note. It stays listed as ignored, never approved.

## Edit scope
- An edit limited to a scope may change: a selected room, its bounding walls,
  their doors and windows and what stands inside it; a selected wall and its
  openings; any other selected element. New elements must land inside the
  selection's area on its level. Roof, levels and settings are outside every
  scope.";

/// `guhit://docs/ph-defaults`.
pub const PH_DEFAULTS: &str = "\
# Philippine defaults

Every number here is millimeters.

## Construction
- Default wall thickness: 150 mm. That is a 6 inch CHB (concrete hollow block)
  wall, plastered and painted, the common Philippine exterior and party wall.
  Pass `thickness_mm` only when the user asks for something else.
- Default level height: 3000 mm floor to floor.
- Stairs: risers of about 175 mm and treads of 250 mm by default, 1000 mm wide.

## Openings
- Door: 900 wide x 2100 high, sill 0. Bedroom and bathroom doors are often
  800 x 2100; a main entrance is often 1000 x 2100.
- Window: 1200 wide x 1200 high, sill 900 above the floor. Bathroom windows are
  often 600 x 600 with a 1500 sill.

## Materials
Built-in material ids:

Walls: mat-chb-painted (default), mat-chb-bare, mat-concrete-fairface,
mat-paint-warm-white, mat-paint-sage, mat-wood-cladding.
Floors: mat-tile-ceramic (default), mat-tile-granite, mat-floor-laminate,
mat-floor-concrete.
Roofs: mat-roof-longspan (default), mat-roof-gi, mat-roof-clay-tile,
mat-roof-concrete-deck.
Other: mat-glass-clear, mat-wood-door, mat-aluminum-frame, mat-steel.

## Roof
- Default: gable, 25 degrees pitch, 600 mm overhang, long-span pre-painted
  metal. Long-span metal roofing is the normal residential roof here.

## Sensible room sizes
- Bedroom 3000 x 3000 and up, master bedroom from 3600 x 3600.
- Toilet and bath 1500 x 2000 minimum, 1800 x 2400 comfortable.
- Kitchen 2400 x 3000, living and dining together 4000 x 6000 for a bungalow.
- Hallway 1000 wide, 1200 when it serves three or more rooms.

These are conventions and rules of thumb, not code requirements. Nothing here
is a compliance statement.";

/// One prompt: `{arg}` in the body is replaced by the argument.
pub struct PromptText {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// Name, description, required.
    pub args: &'static [(&'static str, &'static str, bool)],
    pub body: &'static str,
}

pub const PROMPTS: [PromptText; 5] = [
    PromptText {
        name: "draft_from_brief",
        title: "Draft a house from a brief",
        description: "Draft a house in Guhit Studio from a written brief, level by level, and check it.",
        args: &[
            ("brief", "What the client wants: rooms, storeys, style, lot size.", true),
            ("lot", "The lot, for example \"12 x 20 m, road on the south\".", false),
        ],
        body: "Draft this house in Guhit Studio.\n\nBrief: {brief}\nLot: {lot}\n\n\
Work like this:\n\
1. Read get_guide topic \"workflows\" and \"conventions\". Every length is millimeters, +x east, +y north.\n\
2. project action \"create\" (template blank). set_project with the site and the client.\n\
3. levels: set the ground floor's height, add the storeys above.\n\
4. For each level, one build call with walls, rooms, columns, slabs, stairs, openings and assets, refs for what later entries name. Run it with dry_run first and fix what it refuses.\n\
5. Verify each level: look mode plan with show dims and ids, measure the key dimensions against the brief, then check with since_revision set to your first base_revision.\n\
6. Fix with modify and reshape, and go round again.\n\
7. Tell the user what you built with the areas and dimensions the tools returned. Review items are suggestions to check, never code compliance or permit approval.",
    },
    PromptText {
        name: "model_from_plan_image",
        title: "Model a plan from an image",
        description: "Trace the user's own plan image or sketch into a Guhit Studio model, at a scale from a known dimension.",
        args: &[
            ("image_path", "Absolute path of the plan image (PNG, JPEG or WebP).", true),
            ("known_dimension", "One dimension on the drawing, for example \"the front wall is 10.2 m\".", true),
            ("level", "The level to model, default the first.", false),
        ],
        body: "Model the plan in {image_path} on level {level} in Guhit Studio.\n\nKnown dimension: {known_dimension}\n\n\
1. Look at the image and find the two ends of the known dimension in pixels.\n\
2. import_underlay with path, scale {a_px, b_px, distance_mm} and position {x: 0, y: 0}. Never guess the scale.\n\
3. The result gives the formula from pixels to plan mm. Write a build spec of walls, rooms and openings in plan mm and run it with dry_run, then for real.\n\
4. look mode plan with overlay \"underlay\" and show dims, compare the walls with the image, fix with modify and reshape, repeat until they match.\n\
5. measure the known dimension and two others, run check, and report what matched and what you could not read from the image.",
    },
    PromptText {
        name: "verify_model",
        title: "Verify the model",
        description: "Run the verify loop on the open project: plan pictures, measurements and the review, with numbers from the engine.",
        args: &[
            ("level", "A level to check, default every level.", false),
            ("brief", "What the model should match: dimensions, areas, rooms.", false),
        ],
        body: "Verify the Guhit Studio model, level: {level}.\n\nWhat it should match: {brief}\n\n\
1. query what \"levels\" and what \"summary\".\n\
2. Per level: look mode plan with show dims, ids, grid and labels; query what \"rooms\" with the level.\n\
3. measure what the brief names: distance and clear_width between walls, height of levels and stairs (riser, tread), area of rooms.\n\
4. look mode 3d from the saved views, when the window is open.\n\
5. check, and list every open item.\n\
6. Report each figure with the number the tool returned, what differs from the brief, and the review items as suggestions to check. Never call anything compliant or approved.",
    },
    PromptText {
        name: "client_render_pack",
        title: "Client render pack",
        description: "Save views, render them and export the sheets for a client presentation.",
        args: &[
            ("views", "The views the client should see, for example \"front, living room, kitchen\".", false),
        ],
        body: "Make a client pack of the open Guhit Studio project. Views: {views}\n\n\
1. Turn on the visuals toolset (MCP URL ?toolsets=visuals) if render is not in your tool list.\n\
2. Save each view as a build camera {name, position, target} with the eye at about 1600 mm above the floor.\n\
3. look mode 3d from each view to check it frames well, then render action \"start\" with those views (quality final when the user wants it).\n\
4. export format pdf with level \"all\", and ifc for the consultants.\n\
5. Only call visualize_render when the user asks: it costs them money, and its images are labelled AI visualization.\n\
6. List the files and saved visuals for the user.",
    },
    PromptText {
        name: "swarm_from_brief",
        title: "Draft a building with a swarm",
        description: "Split a brief into parts and draw them with several sub-agents at once, into a draft the user accepts in Guhit Studio.",
        args: &[
            ("brief", "What the client wants: rooms, storeys, style, lot size.", true),
            ("lot", "The lot, for example \"12 x 20 m, road on the south\".", false),
        ],
        body: "Draft this building in Guhit Studio with a swarm of sub-agents.\n\nBrief: {brief}\nLot: {lot}\n\n\
1. Read get_guide topic \"swarm\", then \"workflows\" and \"conventions\". Every length is millimeters, +x east, +y north.\n\
2. project action \"create\" (template blank), or open the user's project.\n\
3. Split the brief into at most 6 parts: one per level for several storeys, else by trade (shell, rooms and openings, roof and stairs, furnishing). No two parts draw the same kind on the same level. Order them with milestones.\n\
4. swarm action \"start\" with the brief, a summary, setup levels for the storeys to add, and the parts. You are the lead.\n\
5. Start one sub-agent per handle. Tell each: its handle, passed as agent on every call; its part's instructions; to wait first when its part comes after another; to draw only its levels and kinds, check with look and measure, report its milestones, and end with swarm action \"finish\".\n\
6. swarm action \"wait\" until every part is done, then look, measure and check the whole draft, and call swarm action \"finish\".\n\
7. Tell the user what was drawn, with the numbers the tools returned, and ask them to review the draft in Guhit Studio and press Accept or Discard. Never accept it yourself; discard only when the user asks, with force. Review items are suggestions to check, never code compliance or permit approval.",
    },
];
