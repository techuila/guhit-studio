//! Fixed prose the server hands to the model: the `instructions` string and
//! the two documentation resources. Kept in one place so the wording that
//! teaches a model how to draw here is reviewed as a whole.

/// Server instructions. Sent once at `initialize`, so every later tool call
/// already has the conventions in context.
pub const INSTRUCTIONS: &str = "\
Guhit Studio is a semantic 2D floor plan editor for Philippine residential work. \
These tools drive the open document in the running desktop app: every edit is \
validated by the same Rust engine the app's own tools use, is saved right away \
and shows up in the window within a second.

Units and coordinates:
- Every length in every argument and every result is MILLIMETERS. 4 m is 4000.
- The plan is +x east, +y north. Angles are degrees, counter-clockwise.
- Areas come back in square metres (m2) because that is how plans are read.

How to draw:
- Open or create a project first. `create_project` and `open_project` switch the \
  document; every other tool acts on whatever is open.
- Build rooms with `add_rect_room`, one call per room. Give the origin as the \
  south-west corner, `width_mm` running east and `depth_mm` running north, both \
  measured on wall CENTERLINES. So a room drawn 4000 x 3000 with 150 mm walls has \
  a net floor of 3850 x 2850.
- Make neighbouring rooms SHARE a wall: put the next room's origin exactly on the \
  previous room's centerline, do not leave a gap and do not overlap. Walls whose \
  endpoints land within 1 mm of each other join automatically.
- Put doors on the shared wall between two rooms, and windows on exterior walls. \
  Get the wall id from `list_rooms` (each room lists its bounding wall ids) or \
  `list_elements`, then call `add_door` or `add_window` with that id.
- For a whole house in one go use `batch`: every step lands as ONE undo step, all \
  or nothing. Ids created by an earlier step are stable, so a later step in the \
  same batch can host a door on a wall the batch just made.
- After a run of edits call `list_review_items` and report what it says.

Pipes:
- A project can hold cold water, hot water, drainage and vent pipes that the \
  user draws in the app. Read them with `list_elements` (kind \"pipe\") and answer \
  length, fitting and sleeve questions with `get_pipe_takeoff`. These tools do not \
  draw pipes, and Guhit never sizes them: plumbing plans are signed by a \
  registered Master Plumber.

Honesty rules:
- Review items are suggestions to check. Never present anything from this server \
  as permit approval, structural certification or code compliance. You are not \
  checking the National Building Code.
- Areas, lengths and counts come from the engine. Report the numbers the tools \
  return; never estimate them yourself.
- A failed edit returns the engine's exact validation message. Read it, fix the \
  arguments and try again rather than working around it.";

/// `guhit://docs/conventions`.
pub const CONVENTIONS: &str = "\
# Guhit Studio conventions

## Units
- All stored and exchanged lengths are f64 MILLIMETERS. There is no other unit
  in any tool argument or result. Areas are reported in square metres (m2).
- Angles are degrees, counter-clockwise positive.

## Coordinates
- Plan: +x is east, +y is north. The origin is arbitrary; a project usually
  starts near (0, 0).
- A wall is a centerline from `start` to `end` plus a thickness. `add_rect_room`
  takes centerline dimensions, so the net interior is the centerline size minus
  one wall thickness on each axis.
- 3D maps plan (x, y) to three.js (x, height, -y). The 2D canvas flips y for
  screen space. Neither affects the numbers these tools take or return.

## Joins
- Two walls join when their endpoints are within 1 mm. Joined corners are
  mitred and the room polygon follows the inner faces.
- New element ids are seeded per leaf command from the project state right
  before that command runs. An id handed back by step 1 of a batch does not
  move when step 2 is appended.

## Openings
- `offset_mm` is the distance from the wall START point to the CENTER of the
  opening, measured along the wall centerline.
- `flip_side` false (the default) means the leaf swings to the LEFT of the wall
  direction start -> end, which is the counter-clockwise normal.
- `flip_hinge` false (the default) puts the hinge on the jamb nearer the wall
  start.

## Assets
- An asset's local +y is its back: the head of a bed, the back of a sofa, the
  tank of a water closet. `rotation_deg` turns it counter-clockwise from there.
- `position` is the CENTER of the footprint.

## Rooms
- A room is a name plus a seed point. Its polygon is derived from the closed
  wall face containing that seed, so a room keeps its name through wall edits.
- A closed face with no room gets one automatically inside the same undo step.

## Levels
- New elements go on the first level of the project unless the project has
  only one, which is the normal case for a bungalow.

## Pipes
- A pipe is a run of straight segments through `points`. x and y are plan
  mm, z is the centerline height above the level floor, negative below the
  slab. Drainage flows from the first point to the last.
- Each system (cold_water, hot_water, drainage, vent) has its own layer.
- Fittings, sleeves and lengths are derived by the engine: read them with
  `get_pipe_takeoff`. Guhit coordinates pipes; it never sizes them.";

/// `guhit://docs/ph-defaults`.
pub const PH_DEFAULTS: &str = "\
# Philippine defaults

Every number here is millimeters.

## Construction
- Default wall thickness: 150 mm. That is a 6 inch CHB (concrete hollow block)
  wall, plastered and painted, the common Philippine exterior and party wall.
  Pass `thickness_mm` only when the user asks for something else.
- Default level height: 3000 mm floor to floor.

## Openings
- Door: 900 wide x 2100 high, sill 0. Bedroom and bathroom doors are often
  800 x 2100; a main entrance is often 1000 x 2100.
- Window: 1200 wide x 1200 high, sill 900 above the floor. Bathroom windows are
  often 600 x 600 with a 1500 sill.

## Materials
Built-in material ids, usable with `set_material` and `set_roof`:

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
