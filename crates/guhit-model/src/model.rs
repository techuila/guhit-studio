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

/// Version 2 added pipes and the four pipe layers. Version 1 files load and
/// get the missing layers (`guhit_core::migrate`).
pub const SCHEMA_VERSION: u32 = 2;

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
    pub roof: Roof,
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
    /// Pipe layers, one per `PipeSystem`.
    ColdWater,
    HotWater,
    Drainage,
    Vent,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Axis {
    X,
    Y,
}

/// One roof preset over the building footprint (`Derived::footprint`).
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
        }
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

/// Straight single-flight stair. `origin` is the center of the first riser,
/// the flight runs along the rotated +y direction. Going depth is
/// `run_mm / riser_count`; total rise is the level height.
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

/// Linear dimension between two plan points.
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
}

impl PipeSystem {
    pub fn layer(self) -> LayerKey {
        match self {
            PipeSystem::ColdWater => LayerKey::ColdWater,
            PipeSystem::HotWater => LayerKey::HotWater,
            PipeSystem::Drainage => LayerKey::Drainage,
            PipeSystem::Vent => LayerKey::Vent,
        }
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
}
