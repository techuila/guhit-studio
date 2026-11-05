//! Typed model commands. CONTRACT FILE - owned by the orchestrator.
//!
//! Every mutation of a project goes through exactly one `Command`. The UI and
//! the AI copilot use the same set. A command is deterministic, validated,
//! serializable and is one undo step (`Batch` groups several into one step).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum WallAnchor {
    /// Keep the start fixed, move the end.
    Start,
    /// Keep the end fixed, move the start.
    End,
    /// Keep the midpoint fixed.
    Center,
}

/// One end of a pipe segment or range, in point order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PipeEnd {
    Start,
    End,
}

/// Points of one pipe run that a `MovePipePoints` moves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PipePoints {
    pub pipe_id: Id,
    /// Point indices, each at most once. Empty: every point of the run.
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Command {
    /// Add one wall. None fields use project and level defaults.
    AddWall {
        start: Point,
        end: Point,
        thickness_mm: Option<f64>,
        /// None: the level height, or `SITE_WALL_HEIGHT_MM` for a site wall.
        height_mm: Option<f64>,
        material_id: Option<Id>,
        level_id: Option<Id>,
        /// None is a building wall. Left out of the JSON when None, so a
        /// building wall's command (and the ids it seeds) is what it was.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<WallRole>,
    },
    /// Add connected walls through `points`. `closed` joins last to first.
    /// Site walls get `SITE_WALL_HEIGHT_MM`; building walls the level height.
    AddWallChain {
        points: Vec<Point>,
        closed: bool,
        thickness_mm: Option<f64>,
        level_id: Option<Id>,
        /// None is a building wall. Left out of the JSON when None, so a
        /// building wall's command (and the ids it seeds) is what it was.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        role: Option<WallRole>,
    },
    /// Add four walls forming a rectangle. `origin` is the south-west corner.
    /// `width_mm` (x) and `depth_mm` (y) are wall CENTERLINE dimensions.
    AddRectRoom {
        origin: Point,
        width_mm: f64,
        depth_mm: f64,
        name: Option<String>,
        thickness_mm: Option<f64>,
        level_id: Option<Id>,
    },
    /// Move both endpoints. Walls that share a moved endpoint follow it.
    SetWallEndpoints { wall_id: Id, start: Point, end: Point },
    /// Set an exact centerline length. Connected walls follow the moved end.
    SetWallLength {
        wall_id: Id,
        length_mm: f64,
        anchor: WallAnchor,
    },
    /// Split a wall in two at `at_mm` from its start. Hosted openings stay put.
    SplitWall { wall_id: Id, at_mm: f64 },

    /// Host a door or window on a wall. None fields use type defaults
    /// (door 900 x 2100 sill 0, window 1200 x 1200 sill 900).
    AddOpening {
        wall_id: Id,
        opening_type: OpeningType,
        offset_mm: f64,
        width_mm: Option<f64>,
        height_mm: Option<f64>,
        sill_mm: Option<f64>,
        style: Option<OpeningStyle>,
        /// None means false. See `Opening::flip_side` for the convention.
        #[serde(default)]
        flip_side: Option<bool>,
        #[serde(default)]
        flip_hinge: Option<bool>,
        /// Its door or window type (DECISIONS D53). The type's style, size
        /// and sill win over the fields above. Left out of the JSON when
        /// None, so an untyped opening's command seeds the ids it did.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        type_id: Option<Id>,
    },

    /// Move the bounding wall(s) on one side of a room outward by `delta_mm`
    /// (negative moves inward). Connected walls stretch to follow.
    ResizeRoom {
        room_id: Id,
        side: Side,
        delta_mm: f64,
    },

    /// Insert any element. An empty `id` is assigned by the core.
    AddElement { element: Element },
    /// Replace the element that has the same id. Used by the inspector.
    UpdateElement { element: Element },
    DeleteElements { ids: Vec<Id> },
    /// Translate elements. With `stretch_connected`, walls attached to a moved
    /// wall keep their joint and stretch. Openings move with their host wall.
    MoveElements {
        ids: Vec<Id>,
        delta: Point,
        stretch_connected: bool,
    },
    RotateElements {
        ids: Vec<Id>,
        pivot: Point,
        angle_deg: f64,
    },
    /// Copy elements, offset by `delta`. New ids are assigned by the core.
    DuplicateElements { ids: Vec<Id>, delta: Point },

    /// Assign a material. Walls, columns, openings: surface. Rooms: floor.
    SetMaterial { ids: Vec<Id>, material_id: Id },
    /// Insert or replace a material by id. An empty id is assigned.
    UpsertMaterial { material: Material },
    /// Edit the roof over the top floor (`RoofOutline::TopFootprint`) from a
    /// whole `Roof`: kind none removes it, any other kind changes it, or
    /// adds it when there is none (fascia 200 mm, as Wave 1 drew it). What
    /// `Roof` cannot say (fascia, gutter, eave, gable sides) is kept. For the
    /// copilot's `set_roof`, MCP's `set_project` and v1 aliases; roof
    /// elements are edited with `UpdateElement`.
    SetRoof { roof: Roof },
    SetProjectSettings { settings: ProjectSettings },
    /// Replace the level that has the same id.
    UpdateLevel { level: Level },
    /// Add a level. None fields: name "Level N", elevation on top of the
    /// highest level (its elevation plus its height), height the default.
    /// The new id is deterministic, so preview equals apply.
    AddLevel {
        name: Option<String>,
        elevation_mm: Option<f64>,
        height_mm: Option<f64>,
    },
    /// Delete a level and every element on it, as one undo step. The last
    /// level cannot be deleted.
    DeleteLevel { level_id: Id },
    SetLayer { layer: Layer },
    /// Set a review item, a whole check, or a check on one element aside with
    /// a note (`Some`), or reopen it (`None`). Replaces an existing mark with
    /// the same target.
    SetReviewMark { target: ReviewTarget, note: Option<String> },

    /// Move pipe points in 3D by `delta` (plan x, y and height z, mm):
    /// one point, a segment (its two points), a whole run (empty
    /// `indices`) or several runs at once. With `keep_joins`, the ends of
    /// other runs of a joinable system that rest on what moved follow, as
    /// `Derived::pipes` joins them: an end meeting a moved end moves with it
    /// (where three or more ends meet, the others follow the main, never the
    /// other way round), and a tee branch keeps its place along the segment
    /// it rests on and follows only the part of the move across it, height
    /// included, interpolated where the segment's two points move
    /// differently and kept on the moved segment. A branch never drags the
    /// run it rests on, and the named runs move only as the command says.
    /// DECISIONS D39.
    MovePipePoints {
        targets: Vec<PipePoints>,
        delta: Vec3,
        keep_joins: bool,
    },
    /// Set the 3D length of segment `segment` (from point `segment` to
    /// `segment + 1`). `hold: Start` keeps its first point, and its last point
    /// and every point after it move along the segment; `End` the other way
    /// round. The rest of the run keeps its shape and slopes.
    SetPipeSegmentLength {
        pipe_id: Id,
        segment: u32,
        length_mm: f64,
        hold: PipeEnd,
        keep_joins: bool,
    },
    /// Make segments `from_segment` to `to_segment` (inclusive) fall by
    /// `slope_pct` percent of their plan length, from the first point to the
    /// last (negative rises). Risers, and segments steeper than 100 percent,
    /// keep their own drop. `hold` is the end
    /// that keeps its height: the held end of the range, and everything past
    /// the other end moves up or down with it. Geometry only: Guhit does not
    /// size or certify a fall (DECISIONS D19).
    SetPipeSlope {
        pipe_id: Id,
        from_segment: u32,
        to_segment: u32,
        slope_pct: f64,
        hold: PipeEnd,
        keep_joins: bool,
    },
    /// Add a bend: a new point at `at`, between the two points of segment
    /// `segment`.
    InsertPipePoint { pipe_id: Id, segment: u32, at: Vec3 },
    /// Remove point `index`. The run keeps at least two points; removing an
    /// end shortens the run.
    RemovePipePoint { pipe_id: Id, index: u32 },

    // ----------------------------------------------------- Wave 2 (D53, D54)
    // Large payloads are boxed so `Command` stays its Wave 1 size on the
    // stack; the JSON is the same.
    /// Add or replace a door or window type by id. An empty id and an empty
    /// mark are assigned (the next free D or W number). Every opening of the
    /// type takes its style, size and sill in the same step; one that no
    /// longer fits its wall refuses the command.
    UpsertOpeningSpec { spec: Box<OpeningSpec> },
    /// Delete door and window types. Their openings keep their size and
    /// become untyped.
    DeleteOpeningSpecs { ids: Vec<Id> },
    /// Give every untyped opening a type: one per distinct opening type,
    /// style, size and sill, reusing a type that matches. None: every level.
    AssignOpeningTypes { level_id: Option<Id> },
    /// Grid lines 1, 2, 3 along x (each `x_spacings_mm` apart, west to east)
    /// and A, B, C along y (south to north), from `origin`, turned by
    /// `rotation_deg`. Labels already taken continue from the next free one.
    AddGridSystem {
        origin: Point,
        rotation_deg: f64,
        x_spacings_mm: Vec<f64>,
        y_spacings_mm: Vec<f64>,
    },
    /// Add automatic dimensions to a level: `exterior` is the overall,
    /// walls (and grids) and openings strings on every side; `rooms` the
    /// inside strings of each room.
    AutoDimension { level_id: Id, kinds: Vec<AutoDimKind> },
    /// Columns under the open eaves of a roof (no wall below), at most
    /// `max_spacing_mm` apart.
    AddRoofPosts {
        roof_id: Id,
        size_mm: f64,
        max_spacing_mm: f64,
    },
    /// Set the lot from its title's technical description: legs from
    /// `start`, each a bearing ("N 45-30 E" or "N 45°30' E") and a distance.
    /// The summary states the misclosure. Replaces the project's lot.
    SetLotFromSurvey { start: Point, legs: Vec<SurveyLeg> },
    /// Set the title block. Its `signatory` is ignored and the stored one
    /// kept: only `SetSignatory` changes it.
    SetTitleBlock { title_block: Box<TitleBlock> },
    /// Set the signing architect (DECISIONS D53). Only people: the app
    /// refuses it from agents (MCP) and the copilot, with a hint.
    SetSignatory { signatory: Box<Signatory> },
    /// Add or replace a sheet by id, at `index` in the set (None: at the
    /// end, or where it was). An empty id, and empty viewport ids, are
    /// assigned. Sheet numbers are unique.
    UpsertSheet { sheet: Box<Sheet>, index: Option<u32> },
    DeleteSheets { ids: Vec<Id> },
    /// Make the permit set: any missing elevation and section marks (four
    /// facing true north, two through the stair and the longest room), then
    /// A-1 site and vicinity, A-2 on plans, elevations, sections, roof plan
    /// and reflected ceiling plans, the schedules, and the E, P and M sheets
    /// the project has, with their blank professional blocks (D19).
    /// `replace`: drop the sheets there are first; otherwise add only
    /// sheets the set lacks.
    MakeSheetSet { paper: PaperSize, replace: bool },
    /// Add or replace a revision by id (empty: assigned) and mark it on these
    /// sheets.
    UpsertRevision { revision: SheetRevision, sheet_ids: Vec<Id> },
    /// Delete a revision and take it off every sheet.
    DeleteRevision { id: Id },
    /// Make a tour: fly in, orbit, the front door, rooms in door order with
    /// holds, up the stairs, day to dusk.
    MakeTour { name: Option<String>, options: AutoTour },

    /// Apply all or nothing, as one undo step.
    Batch { label: String, commands: Vec<Command> },
}

/// What `AutoDimension` adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AutoDimKind {
    /// Overall, walls and grids, and openings strings round the outside.
    Exterior,
    /// Inside strings of each room.
    Rooms,
}

/// One leg of a lot's technical description.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SurveyLeg {
    /// Quadrant bearing as titles write it: "N 45-30 E", "S 12°05' W".
    pub bearing: String,
    pub distance_mm: f64,
}

/// What `MakeTour` makes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AutoTour {
    /// Fly in and orbit the outside first.
    pub exterior: bool,
    /// Light from day to dusk over the tour.
    pub day_to_dusk: bool,
    /// Walking pace, meters per second.
    pub pace_mps: f64,
    /// Levels to visit, in order. Empty: every level with rooms.
    #[serde(default)]
    pub levels: Vec<Id>,
}

/// Who issued a command. Recorded in history and AI logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Origin {
    User,
    Ai,
}

/// Read-only questions answered from model data, never estimated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Query {
    /// Totals, levels, settings and element counts.
    ProjectSummary,
    /// Every room with name, usage, area, perimeter and bounding walls.
    RoomList,
    /// Rooms with no window on a wall that faces the outside.
    RoomsWithoutExteriorWindow,
    /// Full data of specific elements, plus derived geometry.
    Describe { ids: Vec<Id> },
    /// All elements of one kind.
    ListElements { kind: ElementKind },
    /// Current review items.
    Issues,
    /// Pipe quantities by system, material and size, fitting and sleeve
    /// counts, and every penetration. From `Derived::pipes`.
    PipeTakeoff,
    /// Device and fixture counts per level and room, grouped, with labels.
    /// From `Derived::schedule`.
    Schedule,
    /// The door and window schedule, by mark, with counts per level. From
    /// `Derived::opening_schedule`.
    OpeningSchedule,
    /// The room and finish schedule. From `Derived::room_schedule`.
    RoomSchedule,
    /// Net, gross, open and void areas per level, exactly as `Totals` counts
    /// them. From `Derived::level_areas`.
    LevelAreas,
    /// The lot: area, sides with bearings and distances, setbacks, coverage.
    /// From `Derived::site`.
    Site,
    /// Free spots for a catalog item in a room, against each wall, clear of
    /// doors, swings, windows, voids and stair openings.
    PlacementOptions { room_id: Id, catalog_key: String },
    /// The camera at every frame of a tour (centripetal Catmull-Rom by arc
    /// length, eased on arc length), so preview, export and checks agree.
    /// `fps` None: the tour's own.
    TourFrames { tour_id: Id, fps: Option<u32> },
    /// Catalog items matching a search and a room usage.
    Catalog {
        search: Option<String>,
        usage: Option<RoomUsage>,
    },
}
