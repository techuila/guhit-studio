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
    /// Pipe fittings, penetrations and quantities. Empty without pipes.
    #[serde(default)]
    pub pipes: PipeNetwork,
    /// Object counts per level and room, for fixture tables and device
    /// schedules. Only objects whose catalog item has a `device` kind or is a
    /// sanitary, lighting, electrical, aircon or utility item, plus the
    /// kitchen sink and the washing machine, which count as plumbing fixtures.
    #[serde(default)]
    pub schedule: Vec<ScheduleRow>,
    /// Review marks whose finding the checks no longer produce: resolved.
    #[serde(default)]
    pub review_resolved: Vec<ReviewMark>,
    /// Every guard rail and handrail to draw: the open edges of slabs, the
    /// open sides of stairs and the guard around a stair's opening on the
    /// level above (DECISIONS D50). The plan, the 3D view and the exports
    /// draw these and never work out open edges themselves.
    #[serde(default)]
    pub railings: Vec<RailingRun>,
    /// Every stair's flights, landings, nosings, floor opening and walk
    /// line. The plan, the 3D view, walk mode, railings, IFC, elevations,
    /// sections, review and tours use this, never their own stair math.
    #[serde(default)]
    pub stairs: Vec<StairGeometry>,
    /// Every roof's planes, lines, gable-end infill and eaves, in the order
    /// of the roof elements. Replaces the three Wave 1 copies of roof math
    /// (core pipes, export model3d, TS roofMesh).
    #[serde(default)]
    pub roofs: Vec<RoofGeometry>,
    /// The door and window schedule and each opening's mark (DECISIONS D53).
    #[serde(default)]
    pub opening_schedule: OpeningSchedule,
    /// The room and finish schedule, one row per room with a polygon.
    #[serde(default)]
    pub room_schedule: Vec<RoomRow>,
    /// Areas per level, exactly as `Totals` counts them. `Query::ProjectSummary`
    /// and `Query::LevelAreas` read these.
    #[serde(default)]
    pub level_areas: Vec<LevelAreas>,
    /// The lot's derived geometry. None without a lot.
    #[serde(default)]
    pub site: Option<Box<SiteGeometry>>,
}

// ----------------------------------------------------------------- stairs

/// A stair as every consumer draws and walks it. Plan x and y in mm; z is
/// height above the floor of `level_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StairGeometry {
    pub stair_id: Id,
    pub level_id: Id,
    /// The level whose floor is within 300 mm of the stair's top.
    pub arrives_at: Option<Id>,
    /// Going of one tread and height of one riser, mm.
    pub tread_mm: f64,
    pub riser_mm: f64,
    /// One for a straight stair, two for an L or U, in walking order.
    pub flights: Vec<Flight>,
    pub landings: Vec<Landing>,
    /// The hole it makes in the floor it arrives at, counter-clockwise.
    /// Empty when it arrives nowhere.
    pub opening: Vec<Point>,
    /// The walking line from the bottom to the top; z is the walking
    /// surface. Walk mode, tours and headroom follow it.
    pub walk_line: Vec<Vec3>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Flight {
    /// Plan outline, counter-clockwise.
    pub outline: Vec<Point>,
    /// The front edge of each tread, bottom first; z is the tread's top.
    pub nosings: Vec<Nosing>,
    /// Plan unit vector pointing up the flight.
    pub up: Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Nosing {
    pub a: Point,
    pub b: Point,
    pub z: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Landing {
    /// Plan outline, counter-clockwise.
    pub polygon: Vec<Point>,
    /// Its top.
    pub z: f64,
}

// ------------------------------------------------------------------ roofs

/// A roof as every consumer draws it. Plan x and y in mm; z is height above
/// the floor of `level_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RoofGeometry {
    pub roof_id: Id,
    /// The level whose walls carry it: its own, or for a `TopFootprint`
    /// roof the topmost level with a building.
    pub level_id: Id,
    /// The top surface of each plane, counter-clockwise from above. The
    /// underside is `vertical_mm` straight below.
    pub planes: Vec<Vec<Vec3>>,
    /// The roof's thickness measured vertically.
    pub vertical_mm: f64,
    /// Ridges, hips, valleys, eaves and rakes.
    pub lines: Vec<RoofLine>,
    /// The vertical gable-end walls between the wall tops and the roof
    /// underside that the 3D view and the sections fill, each a polygon.
    pub infill: Vec<Vec<Vec3>>,
    /// Fascia and gutter runs along the eaves, each an open polyline.
    pub eaves: Vec<Vec<Vec3>>,
    /// Plan area covered, eaves included, and the sloped surface area.
    pub plan_area_m2: f64,
    pub surface_area_m2: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RoofLine {
    pub kind: RoofLineKind,
    pub a: Vec3,
    pub b: Vec3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RoofLineKind {
    Ridge,
    Hip,
    Valley,
    Eave,
    /// The sloping edge of a gable end.
    Rake,
}

// -------------------------------------------------------------- schedules

/// The door and window schedule (DECISIONS D53).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OpeningSchedule {
    /// One row per mark, in mark order; then untyped openings grouped by
    /// opening type, style, size and sill, with an empty mark.
    pub rows: Vec<OpeningRow>,
    /// Every typed opening's mark, for the tags on plans and elevations.
    pub marks: Vec<OpeningMark>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OpeningRow {
    /// Empty for untyped openings.
    pub mark: String,
    pub type_id: Option<Id>,
    pub opening_type: OpeningType,
    pub style: OpeningStyle,
    pub width_mm: f64,
    pub height_mm: f64,
    /// None: the openings differ in sill.
    pub sill_mm: Option<f64>,
    pub material_id: Option<Id>,
    pub description: String,
    pub per_level: Vec<LevelCount>,
    pub total: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LevelCount {
    pub level_id: Id,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OpeningMark {
    pub opening_id: Id,
    pub mark: String,
}

/// One room of the room and finish schedules, with both of its areas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RoomRow {
    pub room_id: Id,
    pub level_id: Id,
    pub name: String,
    pub usage: RoomUsage,
    /// Inside the walls, less what a void on its level takes:
    /// `RoomGeometry::area_mm2`.
    pub net_area_m2: f64,
    /// What counts as floor area, exactly as `Totals::floor_area_m2` sums
    /// it: also less balconies and decks, so a walled lanai on a deck has
    /// none. The rows add up to `Totals::floor_area_m2`.
    pub floor_area_m2: f64,
    pub perimeter_m: f64,
    /// The room's own; under a void in the floor above, the ceiling of the
    /// level the void opens into; else its level's ceiling.
    pub ceiling_height_mm: f64,
    pub floor_material_id: Option<Id>,
    pub finishes: RoomFinishes,
}

/// A level's areas, as `Totals` counts them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LevelAreas {
    pub level_id: Id,
    /// Room floor area, less voids, balconies and decks.
    pub net_m2: f64,
    /// Footprints plus floor slabs past them, less voids, balconies and decks.
    pub gross_m2: f64,
    /// Balconies and decks: open floor, not floor area.
    pub open_m2: f64,
    pub void_m2: f64,
}

// ------------------------------------------------------------------- site

/// The lot, worked out from `Lot`. Facts only, never a code check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SiteGeometry {
    pub lot_id: Id,
    pub area_m2: f64,
    /// One per lot side, in order.
    pub sides: Vec<LotSideGeometry>,
    /// The lot shrunk by each side's setback. Empty when no side has one.
    pub setback_outline: Vec<Point>,
    /// Ground floor footprint over lot area, percent. A fact, never a check.
    pub coverage_pct: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LotSideGeometry {
    pub length_mm: f64,
    /// Quadrant bearing relative to true north, "N 45°30' E".
    pub bearing: String,
    /// The nearest building face, mm. None without a building.
    pub clearance_mm: Option<f64>,
}

/// One continuous rail: a straight or bent run along open edges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RailingRun {
    /// The slab or stair that carries the rail.
    pub host_id: Id,
    /// The level it stands on. A stair's well guard stands on the level the
    /// stair arrives at.
    pub level_id: Id,
    pub kind: RailingRunKind,
    /// The foot of the rail, two or more points in order: plan x and y, z the
    /// walking surface above the level's floor (a slab's `top_offset_mm`, a
    /// stair's nosing line). A run that goes all the way round repeats its
    /// first point at the end.
    pub points: Vec<Vec3>,
    /// Top of the rail above its foot.
    pub height_mm: f64,
    pub style: RailingStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RailingRunKind {
    /// Along an open edge of a floor, balcony, deck or void.
    SlabEdge,
    /// Along an open side of a stair flight, rising with it.
    StairSide,
    /// Round the opening a stair makes in the floor it arrives at.
    StairWell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ScheduleGroup {
    Plumbing,
    Electrical,
    Aircon,
    Utility,
}

/// How many of one catalog item stand in one room of one level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScheduleRow {
    pub level_id: Id,
    /// The room the objects stand in. None when outside every room.
    pub room_id: Option<Id>,
    pub group: ScheduleGroup,
    pub catalog_key: String,
    /// From the catalog item, when it is a device.
    pub device: Option<DeviceKind>,
    pub count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum IssueStatus {
    #[default]
    Open,
    /// Set aside by the designer with a note (`Project::review`).
    Ignored,
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
    /// Sum of net room areas, less what voids, balconies and decks cover
    /// (docs/CONTRACT.md, "Slabs and railings").
    pub floor_area_m2: f64,
    /// Sum of footprint areas, plus floor slabs where they reach past the
    /// footprints, less what voids, balconies and decks cover inside them.
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
    /// Where the finding is, when it has one point (the pipe checks set it).
    /// Plan x and y in mm, z above the floor of the first element's level.
    #[serde(default)]
    pub location: Option<Vec3>,
    /// Open, or ignored by a review mark. Never "approved".
    #[serde(default)]
    pub status: IssueStatus,
    /// The note of the review mark that ignores it. Empty when open.
    #[serde(default)]
    pub note: String,
}

// ------------------------------------------------------------------ pipes

/// Everything derived from the pipes of a project. Positions use the pipe
/// convention: plan x and y, z above the floor of `level_id`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PipeNetwork {
    pub fittings: Vec<PipeFitting>,
    pub penetrations: Vec<PipePenetration>,
    /// One row per system, material and size. Sorted by system, then material,
    /// then size.
    pub takeoff: Vec<PipeTakeoffRow>,
    pub total_length_m: f64,
    pub elbow_count: u32,
    pub tee_count: u32,
    /// Penetrations: each needs a sleeve (slab, wall) or flashing (roof).
    pub sleeve_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FittingKind {
    /// A change of direction inside one run.
    Elbow,
    /// The end of one run joining another run away from its ends.
    Tee,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PipeFitting {
    pub kind: FittingKind,
    /// The run the fitting sits on. For a tee, the run that is joined.
    pub pipe_id: Id,
    /// For a tee, the run whose end joins. None for an elbow.
    pub branch_pipe_id: Option<Id>,
    pub level_id: Id,
    pub position: Vec3,
    /// The larger diameter at the fitting.
    pub diameter_mm: f64,
    /// Elbow: the change of direction in degrees. Tee: the angle between the
    /// branch and the run.
    pub angle_deg: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PenetrationKind {
    /// Through the floor slab of the pipe's level, inside a footprint.
    Slab,
    /// Across the thickness of a wall, outside its openings.
    Wall,
    /// Through the roof, usually a vent stack.
    Roof,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PipePenetration {
    pub kind: PenetrationKind,
    pub pipe_id: Id,
    /// The wall crossed, for `Wall`. None otherwise.
    pub host_id: Option<Id>,
    pub level_id: Id,
    /// Where the centerline crosses the middle of the slab, wall or roof.
    pub position: Vec3,
    /// Unit direction of the pipe there, for drawing the sleeve.
    pub direction: Vec3,
    pub diameter_mm: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PipeTakeoffRow {
    pub system: PipeSystem,
    pub material: PipeMaterial,
    pub diameter_mm: f64,
    /// Centerline length, rounded to the millimeter.
    pub length_m: f64,
    pub run_count: u32,
}
