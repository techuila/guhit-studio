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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Command {
    /// Add one wall. None fields use project and level defaults.
    AddWall {
        start: Point,
        end: Point,
        thickness_mm: Option<f64>,
        height_mm: Option<f64>,
        material_id: Option<Id>,
        level_id: Option<Id>,
    },
    /// Add connected walls through `points`. `closed` joins last to first.
    AddWallChain {
        points: Vec<Point>,
        closed: bool,
        thickness_mm: Option<f64>,
        level_id: Option<Id>,
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
    SetLayer { layer: Layer },

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
}
