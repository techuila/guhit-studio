//! Data computed from the model by `guhit-core` after every change.
//! CONTRACT FILE - owned by the orchestrator. Never persisted, never edited.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::*;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Derived {
    pub walls: Vec<WallGeometry>,
    pub rooms: Vec<RoomGeometry>,
    /// Outer boundaries, used for roof and slab. One entry per closed building
    /// on a level, largest first. A level with none has one entry with an
    /// empty polygon. Consumers must handle several per level.
    pub footprints: Vec<Footprint>,
    pub totals: Totals,
    /// Design review items. Suggestions, never compliance statements.
    pub issues: Vec<Issue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WallGeometry {
    pub wall_id: Id,
    pub length_mm: f64,
    /// Plan outline with joins resolved (mitred corners), counter-clockwise.
    pub outline: Vec<Point>,
    /// True when one face of the wall is on the outside of the footprint.
    pub exterior: bool,
    /// Walls sharing the start and end joints.
    pub joined_at_start: Vec<Id>,
    pub joined_at_end: Vec<Id>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RoomGeometry {
    pub room_id: Id,
    /// Net interior polygon on the inner wall faces, counter-clockwise.
    pub polygon: Vec<Point>,
    /// Same face on wall centerlines.
    pub centerline_polygon: Vec<Point>,
    /// Net interior area.
    pub area_mm2: f64,
    pub perimeter_mm: f64,
    /// Good label position, guaranteed inside the polygon.
    pub label_point: Point,
    pub wall_ids: Vec<Id>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Footprint {
    pub level_id: Id,
    /// Outer wall faces, counter-clockwise. Empty when no closed building.
    pub polygon: Vec<Point>,
    pub area_mm2: f64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Totals {
    /// Sum of net room areas.
    pub floor_area_m2: f64,
    /// Sum of footprint areas.
    pub gross_area_m2: f64,
    pub wall_length_m: f64,
    pub room_count: u32,
    pub door_count: u32,
    pub window_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Issue {
    /// Stable for the same finding on the same elements.
    pub id: String,
    pub severity: Severity,
    /// Machine code, for example "room_no_window", "door_narrow".
    pub code: String,
    pub message: String,
    pub element_ids: Vec<Id>,
}
