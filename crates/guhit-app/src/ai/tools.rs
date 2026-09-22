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

pub const READ_TOOLS: [&str; 6] = [
    "get_project_summary",
    "list_rooms",
    "describe_elements",
    "list_elements",
    "find_rooms_without_exterior_window",
    "list_review_items",
];

pub const EDIT_TOOLS: [&str; 15] = [
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

/// Messages API `tools` array. Order is fixed so the prompt prefix stays
/// byte-stable and cacheable.
pub fn definitions() -> Vec<Value> {
    let catalog_keys: Vec<String> = defaults::asset_catalog().into_iter().map(|c| c.key).collect();
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
            "All elements of one kind with their ids. Use it to find walls, openings or assets the user refers to.",
            json!({"kind": {"type": "string", "enum": ["wall", "opening", "room", "column", "stair", "asset", "annotation", "dimension", "camera", "underlay"]}}),
            &["kind"], false,
        ),
        tool(
            "find_rooms_without_exterior_window",
            "Rooms that have no window on a wall facing the outside. Call this when the user asks about natural light or ventilation.",
            json!({}), &[], false,
        ),
        tool(
            "list_review_items",
            "Current design review items. They are suggestions to check, not compliance findings.",
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
            "Stage a furniture or fixture item from the built-in library. position is the center of its footprint.",
            json!({
                "catalog_key": {"type": "string", "enum": catalog_keys},
                "position": point_schema("Center of the item"),
                "rotation_deg": {"type": "number", "description": "Counter-clockwise, 0 when omitted."},
            }),
            &["catalog_key", "position"], true,
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
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddWallChainInput {
    points: Vec<PointInput>,
    closed: bool,
    thickness_mm: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddRectRoomInput {
    origin: PointInput,
    width_mm: f64,
    depth_mm: f64,
    name: Option<String>,
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
                level_id,
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
                level_id: Some(level_id.clone()),
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
            Command::AddElement {
                element: Element::Asset(Asset {
                    // Empty id: the core assigns a deterministic one.
                    id: String::new(),
                    level_id: level_id.clone(),
                    catalog_key: item.key,
                    name: item.name,
                    category: item.category,
                    position: i.position.point("position")?,
                    rotation_deg: i.rotation_deg.map(|v| finite(v, "rotation_deg")).transpose()?.unwrap_or(0.0),
                    width_mm: item.width_mm,
                    depth_mm: item.depth_mm,
                    height_mm: item.height_mm,
                    elevation_mm: item.elevation_mm,
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
    let brief = |el: &Element| json!({"id": el.id(), "kind": el.kind(), "label": element_label(el)});
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
        .map(brief)
        .collect();
    json!({
        "added": added,
        "modified": modified,
        "removed": removed,
        "roof_changed": before.roof != after.roof,
    })
}
