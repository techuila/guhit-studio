//! Canonical project model. CONTRACT FILE - owned by the orchestrator.
//!
//! Conventions (every crate and the frontend rely on these):
//! - All lengths are `f64` millimeters. Never meters, never pixels.
//! - Plan coordinates: +x is east, +y is north. The 2D canvas flips y for display.
//! - 3D world: plan (x, y) maps to three.js (x, -y on the z axis), height is three.js y.
//! - Angles are degrees, counter-clockwise positive in plan.
//! - Ids are UUID v4 strings, stable for the life of an element.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub type Id = String;

/// Version 2 added pipes and the four pipe layers. Version 3 added the
/// electrical, aircon and storm layers, the site, review marks, and light,
/// links and circuit tags on objects. Version 4 added the site layer and
/// `Wall::role`. Version 5 added slabs, the slabs layer and
/// `Stair::railing`. Version 6 (DECISIONS D53, D54) added roofs as
/// elements, grid lines, view marks, the lot, tours, door and window types,
/// the title block, sheets and the grids and roofs layers; the old project
/// roof becomes one `RoofArea` over the top floor. Older files load and get
/// the missing layers (`guhit_core::migrate`); every new field has a default.
pub const SCHEMA_VERSION: u32 = 6;

/// For `skip_serializing_if` on a defaulted 0.0.
fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// For `skip_serializing_if` on a defaulted false.
fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// World-space point in mm: x east, y north, z up.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Project {
    pub schema_version: u32,
    pub id: Id,
    pub name: String,
    /// RFC 3339 timestamps.
    pub created_at: String,
    pub updated_at: String,
    pub settings: ProjectSettings,
    pub levels: Vec<Level>,
    pub layers: Vec<Layer>,
    pub materials: Vec<Material>,
    pub elements: Vec<Element>,
    /// The single roof of files before schema 6. `guhit_core::migrate` turns
    /// it into a `TopFootprint` `Element::Roof` and leaves its kind none, so
    /// nothing reads it: the roof over the top floor is `top_roof`.
    pub roof: Roof,
    /// Review items the designer set aside, with a note. Open is the default
    /// and is not stored; resolved is derived (`Derived::review_resolved`).
    #[serde(default)]
    pub review: Vec<ReviewMark>,
    /// Door and window types with their marks (D1, W1), DECISIONS D53.
    #[serde(default)]
    pub opening_types: Vec<OpeningSpec>,
    /// The PH title block printed on every sheet. Boxed, like the large
    /// command payloads, so `Project` and `DocState` stay small on the stack
    /// (the JSON is the same).
    #[serde(default)]
    pub title_block: Box<TitleBlock>,
    /// Sheets in set order, and the revision log.
    #[serde(default)]
    pub sheet_set: SheetSet,
}

impl Project {
    /// The roof element over the top floor: the first `Element::Roof` whose
    /// outline is `TopFootprint`. There is at most one.
    pub fn top_roof_area(&self) -> Option<&RoofArea> {
        self.elements.iter().find_map(|e| match e {
            Element::Roof(r) if r.outline == RoofOutline::TopFootprint => Some(r),
            _ => None,
        })
    }

    /// The roof over the top floor as one `Roof` value, for every reader of
    /// the old project roof (the copilot, MCP v1 aliases, the inspector, the
    /// Wave 1 roof drawing). From the `TopFootprint` roof element when there
    /// is one; else the legacy `roof`, which is what an unmigrated project
    /// (a test that sets it by hand) has, and kind none on a migrated one.
    /// Never build an edit from `roof` directly: once migrated its kind is
    /// none, and sending it back would delete the roof (DECISIONS D54).
    pub fn top_roof(&self) -> Roof {
        match self.top_roof_area() {
            Some(area) => area.as_roof(),
            None => self.roof.clone(),
        }
    }

    /// The door or window type with this id.
    pub fn opening_type(&self, id: &str) -> Option<&OpeningSpec> {
        self.opening_types.iter().find(|t| t.id == id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DisplayUnit {
    Mm,
    M,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PaperSize {
    A4,
    A3,
    A2,
    A1,
    A0,
}

/// Sheet orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Orientation {
    Landscape,
    Portrait,
}

/// The plan sheets a PH permit and hand-off set asks for. Every sheet keeps
/// the title block; the signing professional's fields stay blank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SheetKind {
    /// The architectural plan. Pipes follow `show_pipes`.
    #[default]
    Plan,
    /// Lighting layout: fixtures, switches and their links, a legend and counts.
    Lighting,
    /// Power layout: outlets, special purpose outlets, panelboard, conduit,
    /// a legend, counts and a schedule of loads with blank ratings for the PEE.
    Power,
    /// Plumbing layout: water, drainage, vent and storm, legend, fixture table.
    Plumbing,
    /// Water and sanitary isometric diagrams, not to scale, with a legend.
    PlumbingIsometric,
    /// Aircon layout: units, line sets, condensate, core holes, a legend.
    Aircon,
    /// The reflected ceiling plan: ceilings seen from below, their heights
    /// and the ceiling fixtures.
    ReflectedCeiling,
}

/// Plan side of a room, a level's buildings or a dimension, by the outward
/// normal.
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
#[ts(export)]
pub struct ProjectSettings {
    /// Display only. Stored values are always mm.
    pub display_unit: DisplayUnit,
    /// Drawing scale 1:N, for example 100.
    pub scale_denominator: u32,
    /// Rotation of true north from +y, degrees counter-clockwise.
    pub north_angle_deg: f64,
    pub paper: PaperSize,
    pub default_wall_thickness_mm: f64,
    pub grid_mm: f64,
    pub client_name: String,
    pub location: String,
    pub designer: String,
    /// Where the house stands, for the sun. None means Manila.
    #[serde(default)]
    pub site: Option<Site>,
}

/// Geographic site of the project, used only for the sun position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Site {
    /// City preset key ("manila", "cebu", ...) or "custom". Display only.
    pub city: String,
    /// Degrees, north positive.
    pub latitude_deg: f64,
    /// Degrees, east positive.
    pub longitude_deg: f64,
    /// Minutes east of UTC. Philippine time is 480, with no daylight saving.
    pub utc_offset_min: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Level {
    pub id: Id,
    pub name: String,
    pub elevation_mm: f64,
    /// Floor-to-floor height. Walls without their own height use this.
    pub height_mm: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LayerKey {
    Walls,
    Openings,
    Rooms,
    Columns,
    Stairs,
    Assets,
    Annotations,
    Dimensions,
    Underlays,
    /// Plumbing layers, one per plumbing `PipeSystem`.
    ColdWater,
    HotWater,
    Drainage,
    Vent,
    /// Storm drainage runs.
    Storm,
    /// Electrical and lighting objects, and conduit runs.
    Electrical,
    /// Aircon units, refrigerant line sets and condensate runs.
    Aircon,
    /// Site walls: the lot fence and boundary walls (`WallRole::Site`).
    Site,
    /// Floors, balconies, decks and voids drawn as `Slab`s.
    Slabs,
    /// Grid lines (`GridLine`).
    Grids,
    /// Roofs (`RoofArea`).
    Roofs,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Layer {
    pub key: LayerKey,
    pub visible: bool,
    pub locked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MaterialCategory {
    Wall,
    Floor,
    Roof,
    Glass,
    Wood,
    Metal,
    Generic,
}

/// Procedural surface pattern hint for the 3D view and hatches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MaterialPattern {
    None,
    /// Concrete hollow block.
    Chb,
    Concrete,
    Tile,
    WoodPlank,
    RoofSheet,
    RoofTile,
    Glass,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Material {
    pub id: Id,
    pub name: String,
    pub category: MaterialCategory,
    /// "#rrggbb"
    pub color: String,
    pub roughness: f64,
    pub metalness: f64,
    pub opacity: f64,
    pub pattern: MaterialPattern,
    pub builtin: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RoofKind {
    None,
    Flat,
    Shed,
    Gable,
    /// A hip roof (`RoofShape::Hip`). Readers that only draw the Wave 1
    /// shapes draw it as a gable until `Derived::roofs` gives its planes.
    Hip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Axis {
    X,
    Y,
}

/// One roof preset over the building footprint (`Derived::footprints`): the
/// project roof of files before schema 6, and the value `SetRoof` and
/// `Project::top_roof` carry for the roof over the top floor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Roof {
    pub kind: RoofKind,
    pub pitch_deg: f64,
    pub overhang_mm: f64,
    pub thickness_mm: f64,
    /// Ridge direction for gable, slope direction for shed.
    pub ridge_axis: Axis,
    pub material_id: Option<Id>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum Element {
    Wall(Wall),
    Opening(Opening),
    Room(Room),
    Column(Column),
    Stair(Stair),
    Asset(Asset),
    Annotation(Annotation),
    Dimension(Dimension),
    Camera(Camera),
    Underlay(Underlay),
    Linework(Linework),
    ReferenceModel(ReferenceModel),
    Pipe(Pipe),
    /// A floor, balcony, deck or void on a level (DECISIONS D50).
    Slab(Slab),
    /// A roof over one area: the house, a lanai, a carport (DECISIONS D54).
    Roof(RoofArea),
    /// A grid line, a datum on every level.
    Grid(GridLine),
    /// Where a section cuts or an elevation looks from.
    ViewMark(ViewMark),
    /// The lot from its title. One per project.
    Lot(Lot),
    /// A video tour. A view like `Camera`, on no level.
    Tour(Tour),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ElementKind {
    Wall,
    Opening,
    Room,
    Column,
    Stair,
    Asset,
    Annotation,
    Dimension,
    Camera,
    Underlay,
    Linework,
    ReferenceModel,
    Pipe,
    Slab,
    Roof,
    Grid,
    ViewMark,
    Lot,
    Tour,
}

impl Element {
    pub fn id(&self) -> &Id {
        match self {
            Element::Wall(e) => &e.id,
            Element::Opening(e) => &e.id,
            Element::Room(e) => &e.id,
            Element::Column(e) => &e.id,
            Element::Stair(e) => &e.id,
            Element::Asset(e) => &e.id,
            Element::Annotation(e) => &e.id,
            Element::Dimension(e) => &e.id,
            Element::Camera(e) => &e.id,
            Element::Underlay(e) => &e.id,
            Element::Linework(e) => &e.id,
            Element::ReferenceModel(e) => &e.id,
            Element::Pipe(e) => &e.id,
            Element::Slab(e) => &e.id,
            Element::Roof(e) => &e.id,
            Element::Grid(e) => &e.id,
            Element::ViewMark(e) => &e.id,
            Element::Lot(e) => &e.id,
            Element::Tour(e) => &e.id,
        }
    }

    pub fn id_mut(&mut self) -> &mut Id {
        match self {
            Element::Wall(e) => &mut e.id,
            Element::Opening(e) => &mut e.id,
            Element::Room(e) => &mut e.id,
            Element::Column(e) => &mut e.id,
            Element::Stair(e) => &mut e.id,
            Element::Asset(e) => &mut e.id,
            Element::Annotation(e) => &mut e.id,
            Element::Dimension(e) => &mut e.id,
            Element::Camera(e) => &mut e.id,
            Element::Underlay(e) => &mut e.id,
            Element::Linework(e) => &mut e.id,
            Element::ReferenceModel(e) => &mut e.id,
            Element::Pipe(e) => &mut e.id,
            Element::Slab(e) => &mut e.id,
            Element::Roof(e) => &mut e.id,
            Element::Grid(e) => &mut e.id,
            Element::ViewMark(e) => &mut e.id,
            Element::Lot(e) => &mut e.id,
            Element::Tour(e) => &mut e.id,
        }
    }

    pub fn kind(&self) -> ElementKind {
        match self {
            Element::Wall(_) => ElementKind::Wall,
            Element::Opening(_) => ElementKind::Opening,
            Element::Room(_) => ElementKind::Room,
            Element::Column(_) => ElementKind::Column,
            Element::Stair(_) => ElementKind::Stair,
            Element::Asset(_) => ElementKind::Asset,
            Element::Annotation(_) => ElementKind::Annotation,
            Element::Dimension(_) => ElementKind::Dimension,
            Element::Camera(_) => ElementKind::Camera,
            Element::Underlay(_) => ElementKind::Underlay,
            Element::Linework(_) => ElementKind::Linework,
            Element::ReferenceModel(_) => ElementKind::ReferenceModel,
            Element::Pipe(_) => ElementKind::Pipe,
            Element::Slab(_) => ElementKind::Slab,
            Element::Roof(_) => ElementKind::Roof,
            Element::Grid(_) => ElementKind::Grid,
            Element::ViewMark(_) => ElementKind::ViewMark,
            Element::Lot(_) => ElementKind::Lot,
            Element::Tour(_) => ElementKind::Tour,
        }
    }

    /// The level an element stands on: the one helper every caller uses
    /// (DeleteLevel, per-level queries, layers, the AI scope and agent
    /// limits, history, presence). None for elements on no level of their
    /// own: openings (they are on their wall), cameras and tours (views),
    /// grid lines and the lot (they hold on every level), and the roof over
    /// the top floor (`RoofOutline::TopFootprint`). Deleting a level never
    /// deletes an element this returns None for, except openings, which go
    /// with their wall.
    pub fn level_id(&self) -> Option<&Id> {
        match self {
            Element::Wall(e) => Some(&e.level_id),
            Element::Room(e) => Some(&e.level_id),
            Element::Column(e) => Some(&e.level_id),
            Element::Stair(e) => Some(&e.level_id),
            Element::Asset(e) => Some(&e.level_id),
            Element::Annotation(e) => Some(&e.level_id),
            Element::Dimension(e) => Some(&e.level_id),
            Element::Underlay(e) => Some(&e.level_id),
            Element::Linework(e) => Some(&e.level_id),
            Element::ReferenceModel(e) => Some(&e.level_id),
            Element::Pipe(e) => Some(&e.level_id),
            Element::Slab(e) => Some(&e.level_id),
            Element::ViewMark(e) => Some(&e.level_id),
            Element::Roof(e) => e.level_id.as_ref(),
            Element::Opening(_)
            | Element::Camera(_)
            | Element::Tour(_)
            | Element::Grid(_)
            | Element::Lot(_) => None,
        }
    }

    /// Cameras and tours: views of the model, not parts of it. Every camera
    /// special case (scope, agent limits, work areas, copies) goes through
    /// this so tours act the same: always allowed, on no level, no layer.
    pub fn is_view(&self) -> bool {
        matches!(self, Element::Camera(_) | Element::Tour(_))
    }
}

/// What a wall is for.
///
/// Building walls make the rooms, the building outline and the floor area.
/// Site walls stand on the lot: a perimeter fence (the CHB fence round a PH
/// lot) or a boundary wall. They never close a room, never make a building
/// wall interior and never count in the floor area or the footprint. They
/// are on the `Site` layer, 1800 mm high when drawn without a height.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum WallRole {
    #[default]
    Building,
    Site,
}

impl WallRole {
    pub fn is_building(&self) -> bool {
        *self == WallRole::Building
    }
}

/// A straight wall defined by its centerline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Wall {
    pub id: Id,
    pub level_id: Id,
    pub start: Point,
    pub end: Point,
    pub thickness_mm: f64,
    /// None means use the level height.
    pub height_mm: Option<f64>,
    pub material_id: Option<Id>,
    /// Building (the default, and every wall of an older file) or site.
    /// Left out of the JSON for a building wall, so those read as before.
    #[serde(default, skip_serializing_if = "WallRole::is_building")]
    pub role: WallRole,
}

impl Wall {
    /// A lot fence or boundary wall.
    pub fn is_site(&self) -> bool {
        self.role == WallRole::Site
    }

    /// The layer the wall is drawn on: `Walls`, or `Site` for a site wall.
    pub fn layer(&self) -> LayerKey {
        match self.role {
            WallRole::Building => LayerKey::Walls,
            WallRole::Site => LayerKey::Site,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum OpeningType {
    Door,
    Window,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum OpeningStyle {
    SwingSingle,
    SwingDouble,
    Sliding,
    Fixed,
    Casement,
    Jalousie,
}

/// A door or window hosted on a wall. It has no level of its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Opening {
    pub id: Id,
    pub wall_id: Id,
    pub opening_type: OpeningType,
    pub style: OpeningStyle,
    /// Distance from the host wall start to the opening center, along the centerline.
    pub offset_mm: f64,
    pub width_mm: f64,
    pub height_mm: f64,
    /// Height of the sill above the level floor. 0 for doors.
    pub sill_mm: f64,
    /// Which side of the wall the leaf swings to. false: the left of the wall
    /// direction start -> end (its counter-clockwise normal). true: the right.
    pub flip_side: bool,
    /// Which jamb carries the hinge. false: the jamb nearer the wall start.
    /// true: the jamb nearer the wall end.
    pub flip_hinge: bool,
    pub material_id: Option<Id>,
    /// Its door or window type (`Project::opening_types`), DECISIONS D53.
    /// None: untyped, no mark. A typed opening's style, size and sill are
    /// its type's. Editing one of them moves it to the type that matches,
    /// or to a new type with the next free mark, in the same step. Left out
    /// of the JSON when None, so older commands seed the same ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_id: Option<Id>,
}

/// A named door or window type (DECISIONS D53), as Revit keeps them. The
/// engine writes its style, size and sill into every opening of the type in
/// the same step, so the schedule always matches the plan; schedules count
/// by mark.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OpeningSpec {
    pub id: Id,
    /// "D1", "W3". Unique in the project. Empty gets the next free mark.
    pub mark: String,
    pub opening_type: OpeningType,
    pub style: OpeningStyle,
    pub width_mm: f64,
    pub height_mm: f64,
    /// None: each opening of the type keeps its own sill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sill_mm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_id: Option<Id>,
    /// Schedule text: "Solid wood panel door, painted".
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub remarks: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RoomUsage {
    Living,
    Dining,
    Kitchen,
    Bedroom,
    MasterBedroom,
    Bathroom,
    PowderRoom,
    Laundry,
    Garage,
    Porch,
    Hallway,
    Storage,
    Office,
    Other,
}

/// A named space. Its polygon is derived: it is the closed wall face that
/// contains `seed`. See `Derived::rooms`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Room {
    pub id: Id,
    pub level_id: Id,
    pub name: String,
    pub usage: RoomUsage,
    pub seed: Point,
    pub floor_material_id: Option<Id>,
    /// True while the name was assigned by auto-detection and never edited.
    pub auto_named: bool,
    /// Wall, ceiling and base finishes for the finish schedule. Left out of
    /// the JSON when empty.
    #[serde(default, skip_serializing_if = "RoomFinishes::is_empty")]
    pub finishes: RoomFinishes,
    /// Ceiling height above the floor, mm. None: the level's ceiling (its
    /// height, or the underside of the slab above when that is lower).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceiling_height_mm: Option<f64>,
}

/// A room's finishes, for the room and finish schedules.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RoomFinishes {
    #[serde(default)]
    pub wall_material_id: Option<Id>,
    #[serde(default)]
    pub ceiling_material_id: Option<Id>,
    /// Skirting or base.
    #[serde(default)]
    pub base_material_id: Option<Id>,
    #[serde(default)]
    pub remarks: String,
}

impl RoomFinishes {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ColumnShape {
    Rect,
    Round,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Column {
    pub id: Id,
    pub level_id: Id,
    pub center: Point,
    pub shape: ColumnShape,
    /// Diameter when round.
    pub width_mm: f64,
    pub depth_mm: f64,
    pub rotation_deg: f64,
    pub material_id: Option<Id>,
}

/// A stair: straight, or L or U with a landing. `origin` is the center of
/// the first riser, the first flight runs along the rotated +y direction;
/// total rise is the level height. Each flight's top riser lands on its
/// landing or on the floor above (DECISIONS D44, extended by D54), so a
/// stair of n risers has n minus its flight count treads: n - 1 for a
/// straight stair, n - 2 for an L or U. Treads are `run_mm / treads` deep;
/// `run_mm` is the going of every tread, landings left out. An L or U needs
/// at least 4 risers. Every drawing, walk mode and export reads the stair
/// from `Derived::stairs`, never its own math.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Stair {
    pub id: Id,
    pub level_id: Id,
    pub origin: Point,
    pub rotation_deg: f64,
    pub width_mm: f64,
    pub run_mm: f64,
    pub riser_count: u32,
    /// Handrails along the open sides. None: no railing (files from before
    /// railings).
    #[serde(default)]
    pub railing: Option<Railing>,
    /// Straight (the default), L or U. The new stair fields are left out of
    /// the JSON at their defaults, so older commands seed the same ids.
    #[serde(default, skip_serializing_if = "StairShape::is_straight")]
    pub shape: StairShape,
    /// Which way an L or U turns going up.
    #[serde(default, skip_serializing_if = "Turn::is_left")]
    pub turn: Turn,
    /// Risers in the first flight of an L or U, 2 to n - 2. None: half,
    /// rounded up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_flight_risers: Option<u32>,
    /// Landing depth along the travel, mm. None: the stair width.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landing_mm: Option<f64>,
    /// U only: the clear gap between the two flights, mm.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub well_mm: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum StairShape {
    #[default]
    Straight,
    /// Two flights at right angles with a landing between.
    L,
    /// Two flights side by side, running opposite ways, with a landing.
    U,
}

impl StairShape {
    pub fn is_straight(&self) -> bool {
        *self == StairShape::Straight
    }

    /// Flights of this shape: 1 straight, 2 for an L or U.
    pub fn flights(&self) -> u32 {
        match self {
            StairShape::Straight => 1,
            StairShape::L | StairShape::U => 2,
        }
    }
}

/// Which way a stair turns going up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Turn {
    #[default]
    Left,
    Right,
}

impl Turn {
    pub fn is_left(&self) -> bool {
        *self == Turn::Left
    }
}

/// A guard rail or handrail.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Railing {
    /// Top of the rail above the walking surface, mm. 900 to 1200 is usual.
    pub height_mm: f64,
    pub style: RailingStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RailingStyle {
    /// Posts and a top rail with vertical bars.
    Bars,
    /// A glass panel with a top rail.
    Glass,
    /// A low solid parapet.
    Solid,
}

/// A slab on a level (DECISIONS D50). A `Floor` adds floor where the level's
/// own floor does not reach (an extension, a mezzanine). A `Balcony` or
/// `Deck` is an open floor outside the walls, with an optional rail along its
/// open edges. A `Void` cuts the level's floor away inside its outline, so the
/// space below is double height; its rail guards the edge. Rooms on the level
/// lose the area a void covers; balconies and decks are not floor area.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Slab {
    pub id: Id,
    pub level_id: Id,
    /// `slab_kind` in JSON and TypeScript: `Element` is tagged by `kind`, so
    /// the slab's own kind needs another key or the two collide.
    #[serde(rename = "slab_kind")]
    pub kind: SlabKind,
    /// Outline in plan mm, counter-clockwise, at least three points, not
    /// self-intersecting.
    pub polygon: Vec<Point>,
    /// Ignored for a void.
    pub thickness_mm: f64,
    /// Top of the slab above the level's floor, mm. 0 is flush; a sunken
    /// deck is negative.
    #[serde(default)]
    pub top_offset_mm: f64,
    #[serde(default)]
    pub material_id: Option<Id>,
    #[serde(default)]
    pub railing: Option<Railing>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SlabKind {
    Floor,
    Balcony,
    Deck,
    Void,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AssetCategory {
    Furniture,
    Sanitary,
    Kitchen,
    Appliance,
    Plant,
    Vehicle,
    /// Light fixtures. They give light in the 3D view (`Asset::light`).
    Lighting,
    /// Outlets, switches, panelboards, detectors, bells.
    Electrical,
    /// Aircon indoor, outdoor and window units.
    Aircon,
    /// Meters, tanks, septic tanks, LPG cylinders.
    Utility,
    /// Site objects: a gate, a lamp post, a planter.
    Site,
}

/// A placed library object. `catalog_key` refers to `CatalogItem::key`.
/// Local axes before rotation: width along x, depth along y, and +y is the
/// BACK of the object (bed head, sofa back, WC tank, counter splashback).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Asset {
    pub id: Id,
    pub level_id: Id,
    pub catalog_key: String,
    pub name: String,
    pub category: AssetCategory,
    /// Center of the footprint.
    pub position: Point,
    pub rotation_deg: f64,
    pub width_mm: f64,
    pub depth_mm: f64,
    pub height_mm: f64,
    /// Height of the underside above the level floor.
    pub elevation_mm: f64,
    /// Light the object gives, for fixtures. None: it gives no light.
    #[serde(default)]
    pub light: Option<AssetLight>,
    /// Objects this one controls or feeds: the lights of a switch, the unit
    /// an aircon outlet powers. Ids of other assets. Guhit draws the link;
    /// it never checks circuits.
    #[serde(default)]
    pub links: Vec<Id>,
    /// Free circuit tag, for example "L1" or "C3". Display and schedules only.
    #[serde(default)]
    pub circuit: String,
    /// A built-in sized in code (`CatalogItem::parametric`): its options.
    /// None for every other object. Left out of the JSON when None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub built_in: Option<Box<BuiltInParams>>,
}

/// Options of a parametric built-in: a kitchen run, wardrobe, vanity,
/// counter or shelf, sized by the object's own width, depth and height.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BuiltInParams {
    /// Wall cabinets above as well.
    pub upper: bool,
    /// Sink center from the left end, mm. None: no sink.
    pub sink_at_mm: Option<f64>,
    /// Cooktop center from the left end, mm. None: no cooktop.
    pub cooktop_at_mm: Option<f64>,
    /// Door count across the front.
    pub doors: u32,
}

/// The built-ins sized in code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum BuiltInKind {
    KitchenRun,
    Wardrobe,
    Vanity,
    Counter,
    Shelf,
}

/// Light from a fixture.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AssetLight {
    /// Luminous flux. A 9 W LED bulb sold in PH is about 900 lm.
    pub lumens: f64,
    /// Color temperature: 2700 to 3000 warm white, 4000 neutral, 6500 daylight.
    pub kelvin: f64,
    /// Switched on in the model. Night views light fixtures that are on.
    pub on: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Annotation {
    pub id: Id,
    pub level_id: Id,
    /// Left end of the first line's baseline. Further lines stack below.
    pub position: Point,
    pub text: String,
    /// Text height in model mm at the project scale.
    pub size_mm: f64,
    pub rotation_deg: f64,
}

/// Linear dimension between two plan points, or a dimension string through
/// witness points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Dimension {
    pub id: Id,
    pub level_id: Id,
    pub a: Point,
    pub b: Point,
    /// Signed distance of the dimension line from the a-b line.
    pub offset_mm: f64,
    pub text_override: Option<String>,
    /// Witness points between a and b, in order along a to b: a dimension
    /// string reads a, through..., b. Empty for a plain dimension. Left out
    /// of the JSON when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub through: Vec<Point>,
    /// What an automatic dimension measures. The engine rewrites a, b and
    /// `through` from it after every command, in the same step (D12 as
    /// amended by D54: it stores what it measures, not where). Wall
    /// following (D12) skips it; editing a, b or `through` by hand clears
    /// it; references to deleted walls or grids are dropped, and one left
    /// with nothing to measure is deleted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto: Option<DimRef>,
}

/// What an automatic dimension measures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum DimRef {
    /// The jambs of every opening on these collinear walls, and the walls'
    /// ends.
    Openings { wall_ids: Vec<Id>, face: DimFace },
    /// One side of the buildings on the dimension's own level.
    Exterior { side: Side, tier: DimTier },
    /// Between these grid lines.
    Grids { grid_ids: Vec<Id> },
}

/// Which face of the walls a dimension measures to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DimFace {
    Centerline,
    Exterior,
    Interior,
}

/// The tiers of an exterior dimension, outermost first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DimTier {
    /// End to end.
    Overall,
    /// Wall to wall, and the grids when there are any.
    Walls,
    /// Every opening's jambs.
    Openings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CameraPreset {
    EyeLevel,
    ExteriorCorner,
    Top,
    Axonometric,
    RoomInterior,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Camera {
    pub id: Id,
    pub name: String,
    pub preset: CameraPreset,
    pub position: Vec3,
    pub target: Vec3,
    pub fov_deg: f64,
    /// The light this view was saved with: sun time, sky, exposure, lamps.
    /// None: the view uses whatever light is live.
    #[serde(default)]
    pub light: Option<ViewLight>,
}

/// Sun, sky and exposure saved with a view (SketchUp saves shadow time per
/// scene; Enscape only the sun angle). Local time at `ProjectSettings::site`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ViewLight {
    /// 1 to 12.
    pub month: u8,
    /// 1 to 31.
    pub day: u8,
    /// Minutes after local midnight, 0 to 1439.
    pub minutes: u16,
    pub sky: SkyKind,
    /// Locked exposure in EV relative to the auto value. None: auto.
    pub exposure_ev: Option<f64>,
    /// Whether the fixtures are lit.
    #[serde(default)]
    pub lamps: LampMode,
}

/// Whether a view's light fixtures are lit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LampMode {
    /// Lit from dusk to dawn, off by day.
    #[default]
    Auto,
    /// Lit whatever the time of day.
    On,
    /// Off whatever the time of day.
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SkyKind {
    /// Physical sky that follows the sun.
    Clear,
    /// The same sky, overcast: soft light, faint shadows.
    Cloudy,
    /// The photographed HDRI sky of the asset pack; the sun follows the time.
    Photo,
}

/// Imported raster plan used as a tracing reference. The image file lives in
/// the project folder; the backend serves its bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Underlay {
    pub id: Id,
    pub level_id: Id,
    pub file_name: String,
    /// Plan position of the image's bottom-left corner.
    pub position: Point,
    pub width_px: u32,
    pub height_px: u32,
    /// Scale. Must be confirmed by the user, never assumed.
    pub mm_per_px: f64,
    pub scale_confirmed: bool,
    pub rotation_deg: f64,
    pub opacity: f64,
    pub locked: bool,
}

/// Imported vector linework (from DXF or DWG), kept as a tracing reference on
/// the Underlays layer. Not editable except move, rotate, delete. Coordinates
/// are already in project mm after the user confirmed the unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Linework {
    pub id: Id,
    pub level_id: Id,
    /// Source file name and layer, for display: "site.dxf / A-WALL".
    pub name: String,
    /// Polylines in plan mm. Closed ones repeat their first point at the end.
    pub polylines: Vec<Vec<Point>>,
    /// "#rrggbb"
    pub color: String,
    pub locked: bool,
}

/// An imported 3D model (glTF, GLB or OBJ) shown in the 3D view as context:
/// site, a neighbour, furniture, a massing from SketchUp. The file lives in
/// the project folder under `models/`; the backend serves its bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReferenceModel {
    pub id: Id,
    pub level_id: Id,
    pub name: String,
    pub file_name: String,
    /// Plan position of the model origin.
    pub position: Point,
    pub rotation_deg: f64,
    /// Height of the model origin above the level floor.
    pub elevation_mm: f64,
    /// Multiply model units by this to get mm (a model in meters uses 1000).
    pub scale_to_mm: f64,
    pub locked: bool,
}

/// The building service a pipe belongs to. Each system has its own layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PipeSystem {
    ColdWater,
    HotWater,
    /// Sanitary drainage, waste and soil. Flows from the first point to the last.
    Drainage,
    Vent,
    /// Storm drainage: downspouts and yard drains. Flows first to last.
    Storm,
    /// Electrical conduit.
    Conduit,
    /// Aircon refrigerant line set (liquid and gas lines in one insulated
    /// bundle). `diameter_mm` is the gas line.
    Refrigerant,
    /// Aircon condensate drain. Flows first to last.
    Condensate,
}

impl PipeSystem {
    pub fn layer(self) -> LayerKey {
        match self {
            PipeSystem::ColdWater => LayerKey::ColdWater,
            PipeSystem::HotWater => LayerKey::HotWater,
            PipeSystem::Drainage => LayerKey::Drainage,
            PipeSystem::Vent => LayerKey::Vent,
            PipeSystem::Storm => LayerKey::Storm,
            PipeSystem::Conduit => LayerKey::Electrical,
            PipeSystem::Refrigerant | PipeSystem::Condensate => LayerKey::Aircon,
        }
    }

    /// Runs whose flow must fall: the drain fall check applies to them.
    pub fn falls(self) -> bool {
        matches!(self, PipeSystem::Drainage | PipeSystem::Storm | PipeSystem::Condensate)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PipeMaterial {
    /// Polypropylene random copolymer, the usual PH water line.
    Ppr,
    /// Unplasticized PVC, the usual PH drainage and vent pipe.
    Upvc,
    /// Galvanized iron.
    Gi,
    /// Polyethylene (HDPE), for service connections.
    Pe,
    Copper,
    /// PVC conduit, and PVC pipe for condensate.
    Pvc,
    /// Electrical metallic tubing.
    Emt,
    /// Intermediate metal conduit.
    Imc,
    /// Flexible conduit.
    Flexible,
}

/// A pipe run: straight segments through `points`, with a bend at every
/// interior point. Fittings, penetrations and quantities are derived
/// (`Derived::pipes`). Guhit places and coordinates pipes. It does not size
/// them: plumbing design is signed by a registered Master Plumber (RA 1378).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Pipe {
    pub id: Id,
    pub level_id: Id,
    pub system: PipeSystem,
    pub material: PipeMaterial,
    /// Nominal size in mm, for example PPR 20 or uPVC 100. The 3D view draws
    /// it as the outside diameter.
    pub diameter_mm: f64,
    /// Centerline points. x and y are plan mm. z is the height above the
    /// level floor, negative below the slab. At least two points; two points
    /// in a row are never equal. Drainage flows from the first to the last.
    pub points: Vec<Vec3>,
    /// Optional label, for example "Kitchen sink waste". May be empty.
    pub name: String,
}

/// An entry of the built-in object library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CatalogItem {
    pub key: String,
    pub name: String,
    pub category: AssetCategory,
    pub width_mm: f64,
    pub depth_mm: f64,
    pub height_mm: f64,
    pub elevation_mm: f64,
    /// Where the object mounts. Wall objects sit with their back (+y) on a
    /// wall face; ceiling objects hang from the level height.
    #[serde(default)]
    pub mount: Mount,
    /// The row it counts under in device schedules and the PH electrical
    /// inspection form. None for furniture and fixtures that are not devices.
    #[serde(default)]
    pub device: Option<DeviceKind>,
    /// Default light when placed. Only light fixtures have one.
    #[serde(default)]
    pub light: Option<AssetLight>,
    /// Aircon units: line set and the manufacturer limits the review uses.
    #[serde(default)]
    pub aircon: Option<AirconSpec>,
    /// Rooms it belongs in, for placement and furnishing. Empty: any room.
    /// The new catalog fields are left out of the JSON at their defaults.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rooms: Vec<RoomUsage>,
    /// Search words: "modern", "rattan", "2 seater".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Free space it needs in front (its -y side), mm, for placement.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub clearance_mm: f64,
    /// A built-in sized in code instead of a fixed model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parametric: Option<BuiltInKind>,
    /// The asset pack model it shows in 3D (`public/assets/pack/manifest.json`).
    /// None: the procedural form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Mount {
    #[default]
    Floor,
    Wall,
    Ceiling,
    /// Set into a wall opening, like a window aircon.
    Opening,
}

/// Electrical and aircon device kinds, named after the rows of the PH
/// electrical inspection form so counts can be read straight into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DeviceKind {
    LightingOutlet,
    ConvenienceReceptacle,
    SpecialPurposeOutlet,
    Switch,
    Panelboard,
    SmokeDetector,
    Buzzer,
    PushButton,
    AirconIndoor,
    AirconOutdoor,
    AirconWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AirconRole {
    Indoor,
    Outdoor,
    Window,
}

/// Line set sizes and installation limits of an aircon unit, from PH
/// manufacturer manuals (Koppel, Carrier, Daikin). Review defaults only: the
/// unit's own manual and the PME decide.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AirconSpec {
    pub role: AirconRole,
    /// Nominal capacity in horsepower, as sold in PH.
    pub hp: f64,
    /// Liquid and gas line outside diameters in mm. 0 for window units.
    pub liquid_mm: f64,
    pub gas_mm: f64,
    /// Line set length limits in m, and the largest height difference.
    pub min_line_m: f64,
    pub max_line_m: f64,
    pub max_rise_m: f64,
    /// Line set length included in a standard PH installation, in m.
    /// Installers usually bill each meter beyond it.
    pub included_line_m: f64,
}

// ----------------------------------------------------------------- roofs

/// A roof over one area: the house, a lanai, a carport (DECISIONS D54).
/// Planes, ridges, hips, valleys, gable-end infill, fascia and gutters are
/// derived (`Derived::roofs`); every view draws those, never its own roof
/// math. On the `Roofs` layer.
///
/// Pitched roofs on a drawn `Polygon` need square corners (each within 0.5
/// degrees of 90 or 270): hip on rectangles, L, T and U outlines, gable and
/// shed on rectangles. A command that sets a drawn outline a pitched shape
/// cannot take is refused with `roof_outline`, naming what fits. A derived
/// outline (`TopFootprint`, `Footprint`) follows the walls, so it is never
/// refused: one that does not fit keeps the Wave 1 bounding box clipping (a
/// hip falls back to a gable) and raises a `roof_outline` suggestion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RoofArea {
    pub id: Id,
    /// The level whose walls carry it. None exactly for a `TopFootprint`
    /// roof, which belongs to no level (deleting a level never deletes it);
    /// a `Footprint` or `Polygon` roof always has one.
    pub level_id: Option<Id>,
    pub outline: RoofOutline,
    pub shape: RoofShape,
    /// Degrees from horizontal. Ignored for flat.
    pub pitch_deg: f64,
    /// Gable ridge direction, shed slope direction.
    pub ridge_axis: Axis,
    /// A shed rises against `ridge_axis` (toward -x or -y) instead of with
    /// it, so a lanai on any side can slope away from the house.
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip: bool,
    /// Hip roofs on a rectangle only: outline sides (side i runs from point
    /// i to i + 1) that end in a gable instead of a hip. Refused on any
    /// other outline or shape.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gable_sides: Vec<u32>,
    /// How far the eaves reach past the outline, mm.
    pub overhang_mm: f64,
    pub thickness_mm: f64,
    /// Underside at the wall line, above the floor of the level that carries
    /// it, mm. None: that level's height.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eave_mm: Option<f64>,
    pub material_id: Option<Id>,
    /// Fascia board along the eaves. None: no fascia.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fascia: Option<Trim>,
    /// Gutter along the eaves. None: no gutter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gutter: Option<Trim>,
}

impl RoofArea {
    /// This roof as a `Roof` value (`Project::top_roof`). Fascia, gutter,
    /// eave and gable sides have no place in `Roof`; `SetRoof` keeps them.
    pub fn as_roof(&self) -> Roof {
        Roof {
            kind: self.shape.kind(),
            pitch_deg: self.pitch_deg,
            overhang_mm: self.overhang_mm,
            thickness_mm: self.thickness_mm,
            ridge_axis: self.ridge_axis,
            material_id: self.material_id.clone(),
        }
    }
}

/// The area a roof covers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum RoofOutline {
    /// The outer wall faces of the topmost level with a building, as the
    /// project roof always did. At most one roof per project has it.
    TopFootprint,
    /// The outer wall faces of the buildings on the roof's level.
    Footprint,
    /// A drawn area, plan mm, counter-clockwise.
    Polygon { points: Vec<Point> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RoofShape {
    Flat,
    Shed,
    Gable,
    Hip,
}

impl RoofShape {
    /// The `RoofKind` of this shape.
    pub fn kind(self) -> RoofKind {
        match self {
            RoofShape::Flat => RoofKind::Flat,
            RoofShape::Shed => RoofKind::Shed,
            RoofShape::Gable => RoofKind::Gable,
            RoofShape::Hip => RoofKind::Hip,
        }
    }

    /// The shape of a `RoofKind`. None for `RoofKind::None`.
    pub fn of_kind(kind: RoofKind) -> Option<RoofShape> {
        match kind {
            RoofKind::None => None,
            RoofKind::Flat => Some(RoofShape::Flat),
            RoofKind::Shed => Some(RoofShape::Shed),
            RoofKind::Gable => Some(RoofShape::Gable),
            RoofKind::Hip => Some(RoofShape::Hip),
        }
    }
}

/// A fascia board or a gutter: its face size and material.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Trim {
    pub size_mm: f64,
    #[serde(default)]
    pub material_id: Option<Id>,
}

// ---------------------------------------------- grids, view marks and site

/// A grid line: a datum on every level (no level of its own). Plans,
/// elevations and sections draw it. On the `Grids` layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GridLine {
    pub id: Id,
    /// "A", "1". Unique in the project.
    pub label: String,
    pub start: Point,
    pub end: Point,
}

/// Where a section cuts or an elevation looks from, drawn on its level's
/// plan. Its drawing is computed on demand, never stored. On the
/// `Annotations` layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ViewMark {
    pub id: Id,
    pub level_id: Id,
    pub mark_kind: MarkKind,
    /// "A", or "Front", "North".
    pub label: String,
    /// Section: the cut line. Elevation: the picture plane.
    pub start: Point,
    pub end: Point,
    /// false: it looks to the left of start to end. true: to the right.
    pub flip: bool,
    /// How far beyond the line is drawn, mm. None: everything.
    #[serde(default)]
    pub depth_mm: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MarkKind {
    Section,
    Elevation,
}

fn default_grade_mm() -> f64 {
    150.0
}

/// The lot from its title (TCT): one per project, on no level, on the
/// `Site` layer. Bearings, distances, area, setback outline and coverage are
/// derived (`Derived::site`). A second lot is refused, naming the first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Lot {
    pub id: Id,
    /// Plan mm, in the title's order from corner 1: clockwise (most
    /// titles) or counter-clockwise. Readers that need a winding check it.
    pub corners: Vec<Point>,
    /// One per corner: side i runs from corner i to i + 1 (the last back to
    /// the first).
    pub sides: Vec<LotSide>,
    /// Finish grade below the lowest floor, mm.
    #[serde(default = "default_grade_mm")]
    pub grade_mm: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LotSide {
    pub role: LotSideRole,
    /// The setback drawn along this side, mm; 0 is none. Drawn only, never
    /// a code check.
    pub setback_mm: f64,
    /// The road's name, on a road side.
    #[serde(default)]
    pub road: String,
    #[serde(default)]
    pub road_width_mm: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LotSideRole {
    Road,
    Side,
    Rear,
}

// ----------------------------------------------------------------- tours

/// A video tour: shots in order (DECISIONS D54). Frames are never stored;
/// an exported video is a Visuals record. A view like `Camera`: on no level,
/// no layer, always allowed in any edit scope or agent limit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Tour {
    pub id: Id,
    pub name: String,
    pub shots: Vec<Shot>,
    /// Frames per second: 24, 30 or 60.
    pub fps: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Shot {
    /// Unique in the project, like element ids.
    pub id: Id,
    pub name: String,
    /// The camera move.
    pub kind: ShotMove,
    /// Camera keys, `at` 0 to 1 through the shot, in order, at least one.
    /// Positions and targets follow `Camera`'s convention.
    pub keys: Vec<ShotKey>,
    pub seconds: f64,
    pub ease: Ease,
    /// Light at the start of the shot and at its end (day to dusk). None:
    /// the live light; `light_end` None: the same as `light`.
    #[serde(default)]
    pub light: Option<ViewLight>,
    #[serde(default)]
    pub light_end: Option<ViewLight>,
    /// How this shot begins, from the one before.
    pub transition: Transition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ShotMove {
    Hold,
    Orbit,
    PushIn,
    PullOut,
    Truck,
    Pedestal,
    FlyIn,
    /// Along the walk line, at eye height.
    Walk,
    /// Through the keys as drawn.
    Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ShotKey {
    /// 0 to 1 through the shot.
    pub at: f64,
    pub position: Vec3,
    pub target: Vec3,
    pub fov_deg: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Ease {
    Linear,
    InOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Transition {
    pub kind: TransitionKind,
    /// 0 for a cut.
    pub seconds: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TransitionKind {
    Cut,
    Dissolve,
    DipBlack,
    DipWhite,
    WhipPan,
    /// From the plan, lifting into 3D.
    PlanLift,
}

// --------------------------------------------------- title block, sheets

/// The PH title block (IRR of PD 1096, Figure III.2), printed on every
/// sheet. It prints what was typed and never says a set complies; the
/// signature and dry seal box stays blank. The owner and the location are
/// the project settings' `client_name` and `location`, never copied here.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TitleBlock {
    /// Empty: the project name.
    #[serde(default)]
    pub project_title: String,
    #[serde(default)]
    pub lot_no: String,
    #[serde(default)]
    pub block_no: String,
    #[serde(default)]
    pub tct_no: String,
    #[serde(default)]
    pub firm_name: String,
    #[serde(default)]
    pub firm_address: String,
    /// An image in the project's `files/` folder (`file_store`).
    #[serde(default)]
    pub firm_logo: Option<String>,
    /// The signing architect (DECISIONS D53). Only people change it, with
    /// `Command::SetSignatory`; `SetTitleBlock` keeps what is stored. What
    /// goes to an AI provider or an agent carries `Signatory::masked`.
    #[serde(default)]
    pub signatory: Signatory,
}

/// The signing architect's name and license numbers, as a PH permit sheet
/// prints them. Personal data under the Data Privacy Act (RA 10173): agents
/// and the copilot never write it (their `SetSignatory` is refused), and
/// prompts, AI logs, MCP reads and agents' pictures carry `masked()`. The
/// PDF on disk prints it in full.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Signatory {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub prc_no: String,
    #[serde(default)]
    pub prc_valid_until: String,
    #[serde(default)]
    pub ptr_no: String,
    #[serde(default)]
    pub ptr_date: String,
    #[serde(default)]
    pub ptr_place: String,
    #[serde(default)]
    pub iapoa_no: String,
    #[serde(default)]
    pub iapoa_or_no: String,
    #[serde(default)]
    pub iapoa_date: String,
    #[serde(default)]
    pub tin: String,
}

impl Signatory {
    /// Nothing typed in.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The same with every license and tax number hidden (PRC, PTR, IAPOA
    /// and its OR, TIN): each letter and digit but the last two becomes
    /// `*`, separators stay, and a number of four or fewer is hidden whole.
    /// An empty number stays empty, so a reader still sees what is unset.
    /// Name, dates and place are kept.
    pub fn masked(&self) -> Signatory {
        fn hide(v: &str) -> String {
            let total = v.chars().filter(|c| c.is_ascii_alphanumeric()).count();
            let keep = if total > 4 { 2 } else { 0 };
            let mut seen = 0;
            v.chars()
                .map(|c| {
                    if !c.is_ascii_alphanumeric() {
                        return c;
                    }
                    seen += 1;
                    if seen > total - keep {
                        c
                    } else {
                        '*'
                    }
                })
                .collect()
        }
        Signatory {
            prc_no: hide(&self.prc_no),
            ptr_no: hide(&self.ptr_no),
            iapoa_no: hide(&self.iapoa_no),
            iapoa_or_no: hide(&self.iapoa_or_no),
            tin: hide(&self.tin),
            ..self.clone()
        }
    }
}

impl TitleBlock {
    /// The same with the signatory masked (`Signatory::masked`).
    pub fn masked(&self) -> TitleBlock {
        TitleBlock {
            signatory: self.signatory.masked(),
            ..self.clone()
        }
    }
}

/// The sheets of the set, in order, and the revision log.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SheetSet {
    #[serde(default)]
    pub sheets: Vec<Sheet>,
    #[serde(default)]
    pub revisions: Vec<SheetRevision>,
}

/// One sheet: a paper size and its viewports, inside the title block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Sheet {
    pub id: Id,
    /// "A-1". Unique in the set.
    pub number: String,
    pub title: String,
    pub paper: PaperSize,
    pub orientation: Orientation,
    pub viewports: Vec<Viewport>,
    /// Revisions that touched this sheet, by `SheetRevision::id`.
    #[serde(default)]
    pub revision_ids: Vec<Id>,
}

/// A view placed on a sheet. A viewport whose level or view mark is deleted
/// leaves its sheet in the same step, and the step's summary says so.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Viewport {
    /// Unique in the project, like element ids.
    pub id: Id,
    pub view: ViewRef,
    /// 1:N. None: the largest common scale that fits.
    #[serde(default)]
    pub scale_denominator: Option<u32>,
    /// Center on the paper, mm from its bottom left. None: laid out in order.
    #[serde(default)]
    pub position: Option<Point>,
    /// None: from the view ("NORTH ELEVATION").
    #[serde(default)]
    pub title: Option<String>,
}

/// What a viewport shows. Elevations and sections are computed on demand
/// from their mark, never stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum ViewRef {
    /// A level's plan sheet: the plan, a services layout or the reflected
    /// ceiling plan.
    Plan { level_id: Id, sheet: SheetKind },
    Elevation { mark_id: Id },
    Section { mark_id: Id },
    RoofPlan,
    SitePlan,
    /// An image the user or an agent placed in `files/`, framed with a north
    /// arrow, the site marked and the radius note (DECISIONS D53). No map
    /// service.
    VicinityMap { file_name: Option<String>, radius_m: f64 },
    /// A model view render (`RenderRecord` with source `model_view`). Never
    /// an AI image.
    Perspective { render_id: Id },
    Schedule { schedule: ScheduleKind },
    Notes { text: String },
}

/// The tables a sheet can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ScheduleKind {
    /// The door and window schedule, by mark.
    DoorsWindows,
    Rooms,
    Finishes,
    /// Floor areas per level.
    Areas,
}

/// One revision of the set: its mark in the revision table of the sheets
/// it touched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SheetRevision {
    /// Unique in the project, like element ids.
    pub id: Id,
    /// "1", "A".
    pub mark: String,
    /// "YYYY-MM-DD".
    pub date: String,
    pub description: String,
}

// ---------------------------------------------------------------- review

/// A review item the designer set aside. Only "ignored" is stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReviewMark {
    pub target: ReviewTarget,
    /// Why it is set aside. The UI asks for one.
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum ReviewTarget {
    /// One finding, by `Issue::id`.
    Issue { id: String },
    /// Every finding of one check, by `Issue::code`.
    Check { code: String },
    /// Every finding of one check that involves one element.
    Element { code: String, element_id: Id },
}
