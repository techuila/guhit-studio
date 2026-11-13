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

/// Plan side of a room, by the outward normal of its bounding wall(s).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Side {
    North,
    South,
    East,
    West,
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

    /// Apply all or nothing, as one undo step.
    Batch { label: String, commands: Vec<Command> },
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
}
