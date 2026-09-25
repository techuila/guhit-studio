//! The copilot's tool set. Deliberately small: a handful of robust
//! operations (create, move, resize, rename, query) instead of the whole
//! command surface.
//!
//! - Read tools run right away through `Document::query`.
//! - Edit tools never touch the document. Each call is parsed into a typed
//!   input (unknown fields rejected), checked, and translated into one
//!   `Command`. The caller stages it and validates the whole staged batch
//!   with `Document::preview`.

use guhit_core::Document;
use guhit_model::*;
use serde::Deserialize;
use serde_json::{json, Value};

/// A tool call that could not run. The message goes back to the model as an
/// error tool result, word for word, so it can correct itself.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolError(pub String);

impl ToolError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl From<CoreError> for ToolError {
    fn from(e: CoreError) -> Self {
        let ipc: IpcError = e.into();
        let ids = if ipc.element_ids.is_empty() {
            String::new()
        } else {
            format!(" (elements: {})", ipc.element_ids.join(", "))
        };
        Self(format!("{}: {}{}", ipc.code, ipc.message, ids))
    }
}

use guhit_core::CoreError;

pub const READ_TOOLS: [&str; 8] = [
    "get_project_summary",
    "list_rooms",
    "describe_elements",
    "list_elements",
    "find_rooms_without_exterior_window",
    "list_review_items",
    "get_pipe_takeoff",
    "get_schedule",
];

pub const EDIT_TOOLS: [&str; 17] = [
    "add_wall",
    "add_wall_chain",
    "add_rect_room",
    "add_door",
    "add_window",
    "resize_room",
    "set_wall_length",
    "move_elements",
    "rename_room",
    "set_room_usage",
    "set_opening_size",
    "delete_elements",
    "set_material",
    "set_roof",
    "add_asset",
    "add_level",
    "delete_level",
];

pub fn is_read(name: &str) -> bool {
    READ_TOOLS.contains(&name)
}

pub fn is_edit(name: &str) -> bool {
    EDIT_TOOLS.contains(&name)
}

// -------------------------------------------------------------------- schemas

fn point_schema(what: &str) -> Value {
    json!({
        "type": "object",
        "description": format!("{what}. Plan coordinates in millimeters, x grows east, y grows north."),
        "properties": {
            "x": {"type": "number", "description": "millimeters, east"},
            "y": {"type": "number", "description": "millimeters, north"},
        },
        "required": ["x", "y"],
        "additionalProperties": false,
    })
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str], strict: bool) -> Value {
    let mut t = json!({
        "name": name,
        "description": description,
        "input_schema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        },
    });
    if strict {
        // Strict tool use: the API guarantees inputs match the schema.
        t["strict"] = json!(true);
    }
    t
}

fn ids_schema(description: &str) -> Value {
    json!({"type": "array", "items": {"type": "string"}, "description": description})
}

const POSITION_DOC: &str = "\"center\" puts it at the middle of the wall. \"offset\" uses offset_mm.";
const OFFSET_DOC: &str =
    "Distance in millimeters from the wall start point to the CENTER of the opening, along the wall. Required when position is \"offset\".";
const FLIP_SIDE_DOC: &str =
    "Which side of the wall the leaf swings to. false (the default) is the left of the wall direction start -> end.";
const FLIP_HINGE_DOC: &str =
    "Which jamb carries the hinge. false (the default) is the jamb nearer the wall start.";
const LEVEL_DOC: &str =
    "The level to draw on: its id or its exact name, from get_project_summary. Omit it for the level the user is working on.";

fn level_schema() -> Value {
    json!({"type": "string", "description": LEVEL_DOC})
}

/// The library by category, "key (name)", for the `add_asset` description:
/// the model needs to know that `switch-2` is a two-gang switch.
fn catalog_listing() -> String {
    let catalog = defaults::asset_catalog();
    let mut groups: Vec<(AssetCategory, Vec<String>)> = vec![];
    for c in &catalog {
        let entry = format!("{} ({})", c.key, c.name);
        match groups.iter_mut().find(|(k, _)| *k == c.category) {
            Some((_, list)) => list.push(entry),
            None => groups.push((c.category, vec![entry])),
        }
    }
    groups
        .iter()
        .map(|(category, list)| {
            let name = serde_json::to_value(category)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
            format!("{name}: {}", list.join(", "))
        })
        .collect::<Vec<_>>()
        .join(". ")
}

/// Messages API `tools` array. Order is fixed so the prompt prefix stays
/// byte-stable and cacheable.
pub fn definitions() -> Vec<Value> {
    let catalog_keys: Vec<String> = defaults::asset_catalog().into_iter().map(|c| c.key).collect();
    let add_asset_doc = format!(
        "Stage one item from the built-in library: furniture, fixtures, lights, outlets, switches, a panelboard, detectors or aircon units. position is the center of its footprint. Wall items (outlets, switches, wall lights, panelboards, split aircon indoor units) snap to the nearest wall face within 1000 mm of position, back to the wall and facing the side position is on; give a point just inside the room near that wall. Ceiling items hang from the ceiling. The library: {}.",
        catalog_listing()
    );
    vec![
        // ---- read tools
        tool(
            "get_project_summary",
            "Totals from the model: floor area, gross area, wall length, counts of rooms, doors and windows, levels and settings. Call this for any question about totals or counts.",
            json!({}), &[], false,
        ),
        tool(
            "list_rooms",
            "Every room with its id, name, usage, net area, perimeter and bounding wall ids. Call this for any question about rooms or room areas, and to find a room the user names.",
            json!({}), &[], false,
        ),
        tool(
            "describe_elements",
            "Full data of specific elements by id, including derived geometry such as wall length. Call this before editing an element whose dimensions you need.",
            json!({"ids": ids_schema("Element ids to describe.")}), &["ids"], false,
        ),
        tool(
            "list_elements",
            "All elements of one kind with their ids. Use it to find walls, openings, assets or pipes the user refers to.",
            json!({"kind": {"type": "string", "enum": ["wall", "opening", "room", "column", "stair", "asset", "annotation", "dimension", "camera", "underlay", "pipe"]}}),
            &["kind"], false,
        ),
        tool(
            "find_rooms_without_exterior_window",
            "Rooms that have no window on a wall facing the outside. Call this when the user asks about natural light or ventilation.",
            json!({}), &[], false,
        ),
        tool(
            "list_review_items",
            "Current design review items, each with its status: open, or ignored with the designer's note. They are suggestions to check, not compliance findings.",
            json!({}), &[], false,
        ),
        tool(
            "get_pipe_takeoff",
            "Run quantities from the model for every system (plumbing, storm drains, conduit, aircon line sets and condensate): centerline length per system, material and size, elbow, tee and sleeve counts, and every slab, wall and roof penetration, with aircon core holes. Call this for any question about pipes, runs, fittings or sleeves. It counts; it does not size anything.",
            json!({}), &[], false,
        ),
        tool(
            "get_schedule",
            "Device and fixture counts from the model, per level and room, in the rows of the PH electrical inspection form (lighting outlets, convenience receptacles, special purpose outlets, switches, panelboards, detectors), with totals per level. Call this for any question about how many lights, outlets, switches or fixtures there are. Circuits, loads and ratings are for the electrical engineer.",
            json!({}), &[], false,
        ),
        // ---- edit tools (staged, never applied directly)
        tool(
            "add_wall",
            "Stage one straight wall between two points.",
            json!({
                "start": point_schema("Wall centerline start"),
                "end": point_schema("Wall centerline end"),
                "thickness_mm": {"type": "number", "description": "Omit to use the project default."},
                "level": level_schema(),
            }),
            &["start", "end"], true,
        ),
        tool(
            "add_wall_chain",
            "Stage connected walls through a list of points. closed joins the last point back to the first.",
            json!({
                "points": {"type": "array", "items": point_schema("Corner point"), "description": "At least 2 points, in order."},
                "closed": {"type": "boolean"},
                "thickness_mm": {"type": "number", "description": "Omit to use the project default."},
                "level": level_schema(),
            }),
            &["points", "closed"], true,
        ),
        tool(
            "add_rect_room",
            "Stage a rectangular room: four walls and a named room. origin is the south-west corner. width_mm runs east, depth_mm runs north, both measured on wall centerlines.",
            json!({
                "origin": point_schema("South-west corner"),
                "width_mm": {"type": "number"},
                "depth_mm": {"type": "number"},
                "name": {"type": "string", "description": "Room name, for example \"Bedroom\"."},
                "level": level_schema(),
            }),
            &["origin", "width_mm", "depth_mm"], true,
        ),
        tool(
            "add_door",
            "Stage a door on a wall. Defaults are 900 wide by 2100 high when sizes are omitted.",
            json!({
                "wall_id": {"type": "string"},
                "position": {"type": "string", "enum": ["center", "offset"], "description": POSITION_DOC},
                "offset_mm": {"type": "number", "description": OFFSET_DOC},
                "width_mm": {"type": "number"},
                "height_mm": {"type": "number"},
                "flip_side": {"type": "boolean", "description": FLIP_SIDE_DOC},
                "flip_hinge": {"type": "boolean", "description": FLIP_HINGE_DOC},
            }),
            &["wall_id", "position"], true,
        ),
        tool(
            "add_window",
            "Stage a window on a wall. Defaults are 1200 wide by 1200 high with a 900 sill when sizes are omitted.",
            json!({
                "wall_id": {"type": "string"},
                "position": {"type": "string", "enum": ["center", "offset"], "description": POSITION_DOC},
                "offset_mm": {"type": "number", "description": OFFSET_DOC},
                "width_mm": {"type": "number"},
                "height_mm": {"type": "number"},
                "sill_mm": {"type": "number", "description": "Sill height above the floor."},
                "flip_side": {"type": "boolean", "description": FLIP_SIDE_DOC},
                "flip_hinge": {"type": "boolean", "description": FLIP_HINGE_DOC},
            }),
            &["wall_id", "position"], true,
        ),
        tool(
            "resize_room",
            "Stage moving the wall or walls on one side of a room outward by delta_mm. A negative delta_mm moves them inward. Connected walls stretch to follow. Use this for requests like \"make this room 300 mm wider to the east\".",
            json!({
                "room_id": {"type": "string"},
                "side": {"type": "string", "enum": ["north", "south", "east", "west"]},
                "delta_mm": {"type": "number"},
            }),
            &["room_id", "side", "delta_mm"], true,
        ),
        tool(
            "set_wall_length",
            "Stage an exact centerline length for a wall. anchor says which part stays fixed: \"start\" keeps the start point, \"end\" keeps the end point, \"center\" keeps the midpoint.",
            json!({
                "wall_id": {"type": "string"},
                "length_mm": {"type": "number"},
                "anchor": {"type": "string", "enum": ["start", "end", "center"]},
            }),
            &["wall_id", "length_mm", "anchor"], true,
        ),
        tool(
            "move_elements",
            "Stage moving elements by a distance. dx_mm is east, dy_mm is north. With stretch_connected, walls attached to a moved wall stay joined and stretch.",
            json!({
                "ids": ids_schema("Elements to move."),
                "dx_mm": {"type": "number"},
                "dy_mm": {"type": "number"},
                "stretch_connected": {"type": "boolean"},
            }),
            &["ids", "dx_mm", "dy_mm", "stretch_connected"], true,
        ),
        tool(
            "rename_room",
            "Stage a new name for a room.",
            json!({"room_id": {"type": "string"}, "name": {"type": "string"}}),
            &["room_id", "name"], true,
        ),
        tool(
            "set_room_usage",
            "Stage a usage type for a room.",
            json!({
                "room_id": {"type": "string"},
                "usage": {"type": "string", "enum": ["living", "dining", "kitchen", "bedroom", "master_bedroom", "bathroom", "powder_room", "laundry", "garage", "porch", "hallway", "storage", "office", "other"]},
            }),
            &["room_id", "usage"], true,
        ),
        tool(
            "set_opening_size",
            "Stage new sizes for a door or window. Give only the values that change.",
            json!({
                "opening_id": {"type": "string"},
                "width_mm": {"type": "number"},
                "height_mm": {"type": "number"},
                "sill_mm": {"type": "number"},
            }),
            &["opening_id"], true,
        ),
        tool(
            "delete_elements",
            "Stage deleting elements. Deleting a wall also deletes the doors and windows on it.",
            json!({"ids": ids_schema("Elements to delete.")}),
            &["ids"], true,
        ),
        tool(
            "set_material",
            "Stage a material for elements. Walls, columns and openings take a surface material, rooms take a floor material. Material ids are listed in the project context.",
            json!({"ids": ids_schema("Elements to change."), "material_id": {"type": "string"}}),
            &["ids", "material_id"], true,
        ),
        tool(
            "set_roof",
            "Stage roof changes. Give only the values that change.",
            json!({
                "kind": {"type": "string", "enum": ["none", "flat", "shed", "gable"]},
                "pitch_deg": {"type": "number"},
                "overhang_mm": {"type": "number"},
                "ridge_axis": {"type": "string", "enum": ["x", "y"], "description": "Ridge direction for gable, slope direction for shed. x is east-west."},
                "material_id": {"type": "string"},
            }),
            &[], true,
        ),
        tool(
            "add_asset",
            &add_asset_doc,
            json!({
                "catalog_key": {"type": "string", "enum": catalog_keys},
                "position": point_schema("Center of the item"),
                "rotation_deg": {"type": "number", "description": "Counter-clockwise, 0 when omitted."},
                "level": level_schema(),
            }),
            &["catalog_key", "position"], true,
        ),
        tool(
            "add_level",
            "Stage a new level (storey). By default it is named \"Level N\", its floor sits on top of the highest level (that level's elevation plus its floor-to-floor height) and it is 3000 mm floor to floor. Names are 1 to 60 characters; heights 2000 to 10000 mm; no two levels share a floor elevation. Draw on it by passing its name as level to add_wall, add_wall_chain, add_rect_room or add_asset.",
            json!({
                "name": {"type": "string", "description": "For example \"Second Floor\". Omit for \"Level N\"."},
                "elevation_mm": {"type": "number", "description": "Floor elevation above project zero. Omit to stack it on the highest level."},
                "height_mm": {"type": "number", "description": "Floor-to-floor height. Omit for 3000."},
            }),
            &[], true,
        ),
        tool(
            "delete_level",
            "Stage deleting a level and everything on it: walls with their doors and windows, rooms, columns, stairs, objects, notes, dimensions and pipes. The last level cannot be deleted. Only do this when the user asks for it by name.",
            json!({"level": {"type": "string", "description": "The level's id or exact name, from get_project_summary."}}),
            &["level"], true,
        ),
    ]
}

// ----------------------------------------------------------------- read tools

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoInput {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DescribeInput {
    ids: Vec<Id>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListInput {
    kind: ElementKind,
}

fn parse<T: serde::de::DeserializeOwned>(input: &Value) -> Result<T, ToolError> {
    serde_json::from_value(input.clone()).map_err(|e| ToolError::new(format!("invalid arguments: {e}")))
}

/// Translate a read tool call into a `Query`. Returns the validated args too,
/// for the log.
pub fn to_query(name: &str, input: &Value) -> Result<Query, ToolError> {
    Ok(match name {
        "get_project_summary" => {
            parse::<NoInput>(input)?;
            Query::ProjectSummary
        }
        "list_rooms" => {
            parse::<NoInput>(input)?;
            Query::RoomList
        }
        "describe_elements" => {
            let i: DescribeInput = parse(input)?;
            if i.ids.is_empty() {
                return Err(ToolError::new("invalid arguments: ids is empty"));
            }
            Query::Describe { ids: i.ids }
        }
        "list_elements" => Query::ListElements { kind: parse::<ListInput>(input)?.kind },
        "find_rooms_without_exterior_window" => {
            parse::<NoInput>(input)?;
            Query::RoomsWithoutExteriorWindow
        }
        "list_review_items" => {
            parse::<NoInput>(input)?;
            Query::Issues
        }
        "get_pipe_takeoff" => {
            parse::<NoInput>(input)?;
            Query::PipeTakeoff
        }
        "get_schedule" => {
            parse::<NoInput>(input)?;
            Query::Schedule
        }
        other => return Err(ToolError::new(format!("unknown tool `{other}`"))),
    })
}

/// Run a read tool against `doc`.
pub fn run_read(doc: &Document, name: &str, input: &Value) -> Result<Value, ToolError> {
    let query = to_query(name, input)?;
    Ok(doc.query(&query)?)
}

// ----------------------------------------------------------------- edit tools

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PointInput {
    x: f64,
    y: f64,
}

impl PointInput {
    fn point(&self, what: &str) -> Result<Point, ToolError> {
        finite(self.x, &format!("{what}.x"))?;
        finite(self.y, &format!("{what}.y"))?;
        Ok(Point { x: self.x, y: self.y })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddWallInput {
    start: PointInput,
    end: PointInput,
    thickness_mm: Option<f64>,
    level: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddWallChainInput {
    points: Vec<PointInput>,
    closed: bool,
    thickness_mm: Option<f64>,
    level: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddRectRoomInput {
    origin: PointInput,
    width_mm: f64,
    depth_mm: f64,
    name: Option<String>,
    level: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddLevelInput {
    name: Option<String>,
    elevation_mm: Option<f64>,
    height_mm: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteLevelInput {
    level: String,
}

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
enum PositionInput {
    Center,
    Offset,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddOpeningInput {
    wall_id: Id,
    position: PositionInput,
    offset_mm: Option<f64>,
    width_mm: Option<f64>,
    height_mm: Option<f64>,
    sill_mm: Option<f64>,
    flip_side: Option<bool>,
    flip_hinge: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResizeRoomInput {
    room_id: Id,
    side: Side,
    delta_mm: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetWallLengthInput {
    wall_id: Id,
    length_mm: f64,
    anchor: WallAnchor,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveInput {
    ids: Vec<Id>,
    dx_mm: f64,
    dy_mm: f64,
    stretch_connected: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameRoomInput {
    room_id: Id,
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoomUsageInput {
    room_id: Id,
    usage: RoomUsage,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpeningSizeInput {
    opening_id: Id,
    width_mm: Option<f64>,
    height_mm: Option<f64>,
    sill_mm: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteInput {
    ids: Vec<Id>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetMaterialInput {
    ids: Vec<Id>,
    material_id: Id,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetRoofInput {
    kind: Option<RoofKind>,
    pitch_deg: Option<f64>,
    overhang_mm: Option<f64>,
    ridge_axis: Option<Axis>,
    material_id: Option<Id>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddAssetInput {
    catalog_key: String,
    position: PointInput,
    rotation_deg: Option<f64>,
    level: Option<String>,
}

/// The level a `level` argument names, by id or exact name (spaces around
/// it ignored, then case ignored), or `default` when there is none.
fn pick_level(project: &Project, level: Option<&str>, default: &Id) -> Result<Id, ToolError> {
    let Some(wanted) = level.map(str::trim).filter(|l| !l.is_empty()) else {
        return Ok(default.clone());
    };
    if let Some(l) = project.levels.iter().find(|l| l.id == wanted) {
        return Ok(l.id.clone());
    }
    let named: Vec<&Level> = project
        .levels
        .iter()
        .filter(|l| l.name.trim().eq_ignore_ascii_case(wanted))
        .collect();
    match named.as_slice() {
        [one] => Ok(one.id.clone()),
        [] => {
            let known: Vec<String> = project
                .levels
                .iter()
                .map(|l| format!("{} ({})", l.name, l.id))
                .collect();
            Err(ToolError::new(format!(
                "not_found: no level is called `{wanted}`. Levels: {}",
                known.join(", ")
            )))
        }
        _ => Err(ToolError::new(format!(
            "invalid arguments: more than one level is called `{wanted}`; give its id"
        ))),
    }
}

fn finite(v: f64, what: &str) -> Result<f64, ToolError> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(ToolError::new(format!("invalid arguments: {what} must be a finite number")))
    }
}

fn positive(v: f64, what: &str) -> Result<f64, ToolError> {
    finite(v, what)?;
    if v > 0.0 {
        Ok(v)
    } else {
        Err(ToolError::new(format!("invalid arguments: {what} must be greater than 0, got {v}")))
    }
}

fn positive_opt(v: Option<f64>, what: &str) -> Result<Option<f64>, ToolError> {
    v.map(|v| positive(v, what)).transpose()
}

fn non_empty_ids(ids: &[Id]) -> Result<(), ToolError> {
    if ids.is_empty() || ids.iter().any(|id| id.trim().is_empty()) {
        return Err(ToolError::new("invalid arguments: ids must hold at least one non-empty id"));
    }
    Ok(())
}

fn find<'a>(project: &'a Project, id: &str) -> Result<&'a Element, ToolError> {
    project
        .elements
        .iter()
        .find(|e| e.id() == id)
        .ok_or_else(|| ToolError::new(format!("not_found: no element has id `{id}`")))
}

fn find_room(project: &Project, id: &str) -> Result<Room, ToolError> {
    match find(project, id)? {
        Element::Room(r) => Ok(r.clone()),
        other => Err(ToolError::new(format!(
            "invalid arguments: `{id}` is a {:?}, not a room",
            other.kind()
        ))),
    }
}

fn check_material(project: &Project, id: &str) -> Result<(), ToolError> {
    if project.materials.iter().any(|m| m.id == id) {
        return Ok(());
    }
    let known: Vec<&str> = project.materials.iter().map(|m| m.id.as_str()).collect();
    Err(ToolError::new(format!(
        "not_found: no material has id `{id}`. Known material ids: {}",
        known.join(", ")
    )))
}

fn opening_command(
    project: &Project,
    input: &Value,
    opening_type: OpeningType,
) -> Result<Command, ToolError> {
    let i: AddOpeningInput = parse(input)?;
    if opening_type == OpeningType::Door && i.sill_mm.is_some() {
        return Err(ToolError::new("invalid arguments: a door has no sill_mm"));
    }
    let wall = match find(project, &i.wall_id)? {
        Element::Wall(w) => w,
        other => {
            return Err(ToolError::new(format!(
                "invalid arguments: `{}` is a {:?}, not a wall",
                i.wall_id,
                other.kind()
            )))
        }
    };
    let offset_mm = match (i.position, i.offset_mm) {
        (PositionInput::Center, None) => {
            let (dx, dy) = (wall.end.x - wall.start.x, wall.end.y - wall.start.y);
            (dx * dx + dy * dy).sqrt() / 2.0
        }
        (PositionInput::Center, Some(_)) => {
            return Err(ToolError::new(
                "invalid arguments: give offset_mm only with position \"offset\"",
            ))
        }
        (PositionInput::Offset, Some(v)) => finite(v, "offset_mm")?,
        (PositionInput::Offset, None) => {
            return Err(ToolError::new(
                "invalid arguments: position \"offset\" needs offset_mm",
            ))
        }
    };
    Ok(Command::AddOpening {
        wall_id: i.wall_id,
        opening_type,
        offset_mm,
        width_mm: positive_opt(i.width_mm, "width_mm")?,
        height_mm: positive_opt(i.height_mm, "height_mm")?,
        sill_mm: i.sill_mm.map(|v| finite(v, "sill_mm")).transpose()?,
        style: None,
        flip_side: i.flip_side,
        flip_hinge: i.flip_hinge,
    })
}

/// One straight wall on `level_id`, with the engine's own defaults.
///
/// The core seeds new ids per leaf command, from the project as it is right
/// before that leaf runs, so the id of a wall staged in step 1 does not move
/// when step 2 joins the batch. A later step can host a window on it.
fn wall_command(start: Point, end: Point, thickness_mm: Option<f64>, level_id: &Id) -> Command {
    Command::AddWall {
        start,
        end,
        thickness_mm,
        height_mm: None,
        material_id: None,
        level_id: Some(level_id.clone()),
    }
}

/// A wall item snaps to a wall face at most this far from the given point.
const WALL_SNAP_MM: f64 = 1000.0;

/// Where a wall item goes: its back on the nearest wall face of `level_id`
/// that `at` projects onto, on the side of the wall `at` is on. Returns the
/// center and the rotation that turns the item's back (+y) to the wall, or
/// None when no wall face is within `WALL_SNAP_MM`.
pub fn snap_to_wall(project: &Project, level_id: &str, at: Point, depth_mm: f64) -> Option<(Point, f64)> {
    let mut best: Option<(f64, Point, f64)> = None;
    for el in &project.elements {
        let Element::Wall(w) = el else { continue };
        if w.level_id != level_id || !w.thickness_mm.is_finite() {
            continue;
        }
        let (dx, dy) = (w.end.x - w.start.x, w.end.y - w.start.y);
        let len = (dx * dx + dy * dy).sqrt();
        if !len.is_finite() || len < 1.0 {
            continue;
        }
        let dir = (dx / len, dy / len);
        let left = (-dir.1, dir.0);
        let rel = (at.x - w.start.x, at.y - w.start.y);
        let along = rel.0 * dir.0 + rel.1 * dir.1;
        if !(0.0..=len).contains(&along) {
            continue;
        }
        let off = rel.0 * left.0 + rel.1 * left.1;
        let side = if off >= 0.0 { 1.0 } else { -1.0 };
        let gap = (off.abs() - w.thickness_mm / 2.0).abs();
        if gap > WALL_SNAP_MM || best.as_ref().map(|b| gap >= b.0).unwrap_or(false) {
            continue;
        }
        let out = side * (w.thickness_mm / 2.0 + depth_mm / 2.0);
        let center = Point {
            x: w.start.x + dir.0 * along + left.0 * out,
            y: w.start.y + dir.1 * along + left.1 * out,
        };
        // The back (+y after rotation) points at the wall: (-sin r, cos r).
        let back = (-side * left.0, -side * left.1);
        let turn = (-back.0).atan2(back.1).to_degrees();
        let turn = ((turn * 1.0e6).round() / 1.0e6).rem_euclid(360.0);
        best = Some((gap, center, if turn == -0.0 { 0.0 } else { turn }));
    }
    best.map(|(_, center, turn)| (center, turn))
}

/// Slab under an upper level, as the 3D view draws it.
const UPPER_SLAB_MM: f64 = 200.0;

/// Ceiling height of a level above its floor: its height, or the underside of
/// the next level's slab when that is lower (docs/CONTRACT.md, "Devices,
/// fixtures and links"). Mirrors `ceilingHeightMm` in src/editor2d/mount.ts.
pub fn ceiling_height_mm(project: &Project, level_id: &str) -> f64 {
    let Some(level) = project.levels.iter().find(|l| l.id == level_id) else {
        return defaults::DEFAULT_LEVEL_HEIGHT_MM;
    };
    let mut ceiling = level.height_mm;
    for l in &project.levels {
        if l.elevation_mm > level.elevation_mm + 1.0 {
            ceiling = ceiling.min(l.elevation_mm - UPPER_SLAB_MM - level.elevation_mm);
        }
    }
    ceiling.max(0.0)
}

/// Underside of a ceiling item under a ceiling `ceiling_mm` above the floor:
/// flush with it, except an item the catalog hangs lower than flush (the
/// pendant), which keeps its catalog height unless the ceiling would cut it.
/// Mirrors `ceilingElevation` in src/editor2d/mount.ts.
pub fn ceiling_elevation(ceiling_mm: f64, item: &CatalogItem) -> f64 {
    let flush = ceiling_mm - item.height_mm;
    let catalog_flush = defaults::DEFAULT_LEVEL_HEIGHT_MM - item.height_mm;
    if item.elevation_mm < catalog_flush - 1.0 {
        item.elevation_mm.min(flush).max(0.0)
    } else {
        flush.max(0.0)
    }
}

/// Translate an edit tool call into one typed `Command`.
pub fn to_command(project: &Project, name: &str, input: &Value, level_id: &Id) -> Result<Command, ToolError> {
    let mut commands = to_commands(project, name, input, level_id)?;
    match commands.len() {
        1 => Ok(commands.remove(0)),
        n => Err(ToolError::new(format!("`{name}` expands to {n} commands"))),
    }
}

/// Translate an edit tool call into typed `Command`s (one, except for a wall
/// chain, which is one per wall). `project` is the staged view: the open
/// project with every already staged command applied, so a call can refer to
/// elements staged earlier in the same turn. `level_id` is the level the user
/// is working on; every new element goes there.
pub fn to_commands(
    project: &Project,
    name: &str,
    input: &Value,
    level_id: &Id,
) -> Result<Vec<Command>, ToolError> {
    if name == "add_wall_chain" {
        let i: AddWallChainInput = parse(input)?;
        if i.points.len() < 2 {
            return Err(ToolError::new("invalid arguments: points needs at least 2 points"));
        }
        if i.closed && i.points.len() < 3 {
            return Err(ToolError::new("invalid arguments: a closed chain needs at least 3 points"));
        }
        let thickness_mm = positive_opt(i.thickness_mm, "thickness_mm")?;
        let level_id = &pick_level(project, i.level.as_deref(), level_id)?;
        let mut points = i
            .points
            .iter()
            .enumerate()
            .map(|(n, p)| p.point(&format!("points[{n}]")))
            .collect::<Result<Vec<_>, _>>()?;
        if i.closed {
            points.push(points[0]);
        }
        return Ok(points
            .windows(2)
            .map(|seg| wall_command(seg[0], seg[1], thickness_mm, level_id))
            .collect());
    }
    single_command(project, name, input, level_id).map(|c| vec![c])
}

fn single_command(
    project: &Project,
    name: &str,
    input: &Value,
    level_id: &Id,
) -> Result<Command, ToolError> {
    Ok(match name {
        "add_wall" => {
            let i: AddWallInput = parse(input)?;
            wall_command(
                i.start.point("start")?,
                i.end.point("end")?,
                positive_opt(i.thickness_mm, "thickness_mm")?,
                &pick_level(project, i.level.as_deref(), level_id)?,
            )
        }
        "add_rect_room" => {
            let i: AddRectRoomInput = parse(input)?;
            Command::AddRectRoom {
                origin: i.origin.point("origin")?,
                width_mm: positive(i.width_mm, "width_mm")?,
                depth_mm: positive(i.depth_mm, "depth_mm")?,
                name: i.name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()),
                thickness_mm: None,
                level_id: Some(pick_level(project, i.level.as_deref(), level_id)?),
            }
        }
        "add_level" => {
            let i: AddLevelInput = parse(input)?;
            Command::AddLevel {
                name: i.name.map(|n| n.trim().to_string()),
                elevation_mm: i.elevation_mm.map(|v| finite(v, "elevation_mm")).transpose()?,
                height_mm: positive_opt(i.height_mm, "height_mm")?,
            }
        }
        "delete_level" => {
            let i: DeleteLevelInput = parse(input)?;
            if i.level.trim().is_empty() {
                return Err(ToolError::new("invalid arguments: level is empty"));
            }
            let fallback = String::new();
            Command::DeleteLevel {
                level_id: pick_level(project, Some(&i.level), &fallback)?,
            }
        }
        "add_door" => opening_command(project, input, OpeningType::Door)?,
        "add_window" => opening_command(project, input, OpeningType::Window)?,
        "resize_room" => {
            let i: ResizeRoomInput = parse(input)?;
            let delta_mm = finite(i.delta_mm, "delta_mm")?;
            if delta_mm == 0.0 {
                return Err(ToolError::new("invalid arguments: delta_mm is 0, nothing would change"));
            }
            find_room(project, &i.room_id)?;
            Command::ResizeRoom { room_id: i.room_id, side: i.side, delta_mm }
        }
        "set_wall_length" => {
            let i: SetWallLengthInput = parse(input)?;
            Command::SetWallLength {
                wall_id: i.wall_id,
                length_mm: positive(i.length_mm, "length_mm")?,
                anchor: i.anchor,
            }
        }
        "move_elements" => {
            let i: MoveInput = parse(input)?;
            non_empty_ids(&i.ids)?;
            let delta = Point { x: finite(i.dx_mm, "dx_mm")?, y: finite(i.dy_mm, "dy_mm")? };
            if delta.x == 0.0 && delta.y == 0.0 {
                return Err(ToolError::new("invalid arguments: dx_mm and dy_mm are both 0, nothing would move"));
            }
            Command::MoveElements { ids: i.ids, delta, stretch_connected: i.stretch_connected }
        }
        "rename_room" => {
            let i: RenameRoomInput = parse(input)?;
            let name = i.name.trim();
            if name.is_empty() {
                return Err(ToolError::new("invalid arguments: name is empty"));
            }
            let mut room = find_room(project, &i.room_id)?;
            room.name = name.to_string();
            room.auto_named = false;
            Command::UpdateElement { element: Element::Room(room) }
        }
        "set_room_usage" => {
            let i: RoomUsageInput = parse(input)?;
            let mut room = find_room(project, &i.room_id)?;
            room.usage = i.usage;
            Command::UpdateElement { element: Element::Room(room) }
        }
        "set_opening_size" => {
            let i: OpeningSizeInput = parse(input)?;
            let mut opening = match find(project, &i.opening_id)? {
                Element::Opening(o) => o.clone(),
                other => {
                    return Err(ToolError::new(format!(
                        "invalid arguments: `{}` is a {:?}, not a door or window",
                        i.opening_id,
                        other.kind()
                    )))
                }
            };
            if i.width_mm.is_none() && i.height_mm.is_none() && i.sill_mm.is_none() {
                return Err(ToolError::new("invalid arguments: give at least one of width_mm, height_mm, sill_mm"));
            }
            if let Some(v) = positive_opt(i.width_mm, "width_mm")? {
                opening.width_mm = v;
            }
            if let Some(v) = positive_opt(i.height_mm, "height_mm")? {
                opening.height_mm = v;
            }
            if let Some(v) = i.sill_mm {
                opening.sill_mm = finite(v, "sill_mm")?;
            }
            Command::UpdateElement { element: Element::Opening(opening) }
        }
        "delete_elements" => {
            let i: DeleteInput = parse(input)?;
            non_empty_ids(&i.ids)?;
            Command::DeleteElements { ids: i.ids }
        }
        "set_material" => {
            let i: SetMaterialInput = parse(input)?;
            non_empty_ids(&i.ids)?;
            check_material(project, &i.material_id)?;
            Command::SetMaterial { ids: i.ids, material_id: i.material_id }
        }
        "set_roof" => {
            let i: SetRoofInput = parse(input)?;
            let mut roof = project.roof.clone();
            if i.kind.is_none()
                && i.pitch_deg.is_none()
                && i.overhang_mm.is_none()
                && i.ridge_axis.is_none()
                && i.material_id.is_none()
            {
                return Err(ToolError::new("invalid arguments: give at least one roof value to change"));
            }
            if let Some(kind) = i.kind {
                roof.kind = kind;
            }
            if let Some(v) = i.pitch_deg {
                roof.pitch_deg = finite(v, "pitch_deg")?;
            }
            if let Some(v) = i.overhang_mm {
                roof.overhang_mm = finite(v, "overhang_mm")?;
            }
            if let Some(axis) = i.ridge_axis {
                roof.ridge_axis = axis;
            }
            if let Some(id) = i.material_id {
                check_material(project, &id)?;
                roof.material_id = Some(id);
            }
            Command::SetRoof { roof }
        }
        "add_asset" => {
            let i: AddAssetInput = parse(input)?;
            let level_id = &pick_level(project, i.level.as_deref(), level_id)?;
            let item = defaults::asset_catalog()
                .into_iter()
                .find(|c| c.key == i.catalog_key)
                .ok_or_else(|| {
                    let keys: Vec<String> = defaults::asset_catalog().into_iter().map(|c| c.key).collect();
                    ToolError::new(format!(
                        "not_found: no catalog item has key `{}`. Known keys: {}",
                        i.catalog_key,
                        keys.join(", ")
                    ))
                })?;
            let mut position = i.position.point("position")?;
            let mut rotation_deg = i.rotation_deg.map(|v| finite(v, "rotation_deg")).transpose()?.unwrap_or(0.0);
            let mut elevation_mm = item.elevation_mm;
            match item.mount {
                // Back on the nearest wall face, as the placement tool does.
                Mount::Wall => {
                    if let Some((at, turn)) = snap_to_wall(project, level_id, position, item.depth_mm) {
                        position = at;
                        rotation_deg = turn;
                    }
                }
                // The catalog drop from a 3000 mm ceiling, kept under this
                // level's ceiling: its height, or the underside of the next
                // level's 200 mm slab when that is lower (docs/CONTRACT.md).
                Mount::Ceiling => {
                    elevation_mm = ceiling_elevation(ceiling_height_mm(project, level_id), &item);
                }
                Mount::Floor | Mount::Opening => {}
            }
            Command::AddElement {
                element: Element::Asset(Asset {
                    // Empty id: the core assigns a deterministic one.
                    id: String::new(),
                    level_id: level_id.clone(),
                    catalog_key: item.key,
                    name: item.name,
                    category: item.category,
                    position,
                    rotation_deg,
                    width_mm: item.width_mm,
                    depth_mm: item.depth_mm,
                    height_mm: item.height_mm,
                    elevation_mm,
                    light: item.light,
                    links: vec![],
                    circuit: String::new(),
                }),
            }
        }
        other => return Err(ToolError::new(format!("unknown tool `{other}`"))),
    })
}

// ------------------------------------------------------------- staged diffing

/// Readable name for an element: "Wall 4.00 m", "Room Bedroom", "Door 900".
pub fn element_label(el: &Element) -> String {
    match el {
        Element::Wall(w) => {
            let (dx, dy) = (w.end.x - w.start.x, w.end.y - w.start.y);
            format!("Wall {:.0} mm", (dx * dx + dy * dy).sqrt())
        }
        Element::Opening(o) => match o.opening_type {
            OpeningType::Door => format!("Door {:.0} x {:.0}", o.width_mm, o.height_mm),
            OpeningType::Window => format!("Window {:.0} x {:.0}", o.width_mm, o.height_mm),
        },
        Element::Room(r) => format!("Room {}", r.name),
        Element::Column(_) => "Column".into(),
        Element::Stair(_) => "Stair".into(),
        Element::Asset(a) => a.name.clone(),
        Element::Annotation(a) => format!("Text \"{}\"", a.text),
        Element::Dimension(_) => "Dimension".into(),
        Element::Camera(c) => format!("Camera {}", c.name),
        Element::Underlay(u) => format!("Underlay {}", u.file_name),
        Element::Linework(l) => format!("Linework {}", l.name),
        Element::ReferenceModel(m) => format!("Reference model {}", m.name),
        // "Heater feed (cold water, 20 mm)", "Bedroom line set (aircon line
        // set, 9.52 mm)", or "Cold water pipe 20 mm".
        Element::Pipe(p) => {
            let system = match p.system {
                PipeSystem::ColdWater => "cold water",
                PipeSystem::HotWater => "hot water",
                PipeSystem::Drainage => "drainage",
                PipeSystem::Vent => "vent",
                PipeSystem::Storm => "storm drain",
                PipeSystem::Conduit => "electrical conduit",
                PipeSystem::Refrigerant => "aircon line set",
                PipeSystem::Condensate => "aircon condensate drain",
            };
            if p.name.trim().is_empty() {
                guhit_core::pipe_name(p)
            } else {
                let size = format!("{:.2}", p.diameter_mm);
                let size = size.trim_end_matches('0').trim_end_matches('.');
                format!("{} ({system}, {size} mm)", p.name.trim())
            }
        }
    }
}

/// `element_label`, with the room an object stands in: "Ceiling light in
/// Bedroom". `rooms` is `guhit_core::asset_rooms` of the project.
pub fn element_label_in(el: &Element, rooms: &std::collections::BTreeMap<Id, String>) -> String {
    let label = element_label(el);
    match el {
        Element::Asset(a) => match rooms.get(&a.id).map(|r| r.trim()).filter(|r| !r.is_empty()) {
            Some(room) => format!("{label} in {room}"),
            None => label,
        },
        _ => label,
    }
}

/// What one staged call changed: the difference between the staged project
/// before and after it. New ids are listed so the model can refer to an
/// element it just staged.
///
/// Every id listed here stays valid for the rest of the turn. The core seeds
/// new ids per leaf command from the project state right before that leaf, so
/// appending another step never moves an id an earlier step produced.
pub fn step_diff(before: &Project, after: &Project) -> Value {
    // Objects are named with their room, so five ceiling lights read apart.
    let rooms_after = guhit_core::asset_rooms(after);
    let rooms_before = guhit_core::asset_rooms(before);
    let brief = |el: &Element| json!({"id": el.id(), "kind": el.kind(), "label": element_label_in(el, &rooms_after)});
    let brief_before =
        |el: &Element| json!({"id": el.id(), "kind": el.kind(), "label": element_label_in(el, &rooms_before)});
    let mut added = vec![];
    let mut modified = vec![];
    for el in &after.elements {
        match before.elements.iter().find(|b| b.id() == el.id()) {
            None => added.push(brief(el)),
            Some(b) if b != el => modified.push(brief(el)),
            _ => {}
        }
    }
    let removed: Vec<Value> = before
        .elements
        .iter()
        .filter(|b| !after.elements.iter().any(|a| a.id() == b.id()))
        .map(brief_before)
        .collect();
    let level = |l: &Level| {
        json!({"id": l.id, "name": l.name, "elevation_mm": l.elevation_mm, "height_mm": l.height_mm})
    };
    let levels_added: Vec<Value> = after
        .levels
        .iter()
        .filter(|l| !before.levels.iter().any(|b| b.id == l.id))
        .map(level)
        .collect();
    let levels_removed: Vec<Value> = before
        .levels
        .iter()
        .filter(|l| !after.levels.iter().any(|a| a.id == l.id))
        .map(level)
        .collect();
    let mut out = json!({
        "added": added,
        "modified": modified,
        "removed": removed,
        "roof_changed": before.roof != after.roof,
    });
    if !levels_added.is_empty() {
        out["levels_added"] = json!(levels_added);
    }
    if !levels_removed.is_empty() {
        out["levels_removed"] = json!(levels_removed);
    }
    out
}
