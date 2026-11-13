//! MCP v2: the core toolset `draft` and the extended toolsets
//! (docs/MCP.md "The tools", docs/CONTRACT.md "MCP v2").
//!
//! Twenty-two core tools cover drafting and checking a house: open a
//! project, read it (`query`, `look`, `measure`, `check`), draw it (`build`,
//! `batch` and one tool per element kind), change it (`modify`, `reshape`,
//! `delete`, `levels`, `set_project`), take steps back (`history`) and hand it
//! over (`export`). Every edit runs through `stage`: validated by the
//! engine, one undo step, with the review items it added and resolved.

use guhit_app::AppService;
use guhit_model::*;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::agent::CallContext;
use crate::fail::ToolFail;
use crate::stage::{self, Options, StepIn};
use crate::tools::{obj, scope_schema, Output, ToolDef, MM};
use crate::translate::{self, parse};

// --------------------------------------------------------------- schemas

fn pt(desc: &str) -> Value {
    json!({
        "type": "object",
        "description": desc,
        "properties": {"x": {"type": "number"}, "y": {"type": "number"}},
        "required": ["x", "y"],
    })
}

fn pts(desc: &str) -> Value {
    json!({"type": "array", "items": pt("x east, y north, mm"), "description": desc})
}

fn level() -> Value {
    json!({"type": "string", "description": "Level id or name. Default: the first level."})
}

fn ids(desc: &str) -> Value {
    json!({"type": "array", "items": {"type": "string"}, "description": desc})
}

fn num(desc: &str) -> Value {
    json!({"type": "number", "description": desc})
}

fn string(desc: &str) -> Value {
    json!({"type": "string", "description": desc})
}

fn enumerated(values: &[&str], desc: &str) -> Value {
    json!({"type": "string", "enum": values, "description": desc})
}

fn rect() -> Value {
    json!({
        "type": "object",
        "description": "x, y: the south-west corner; width_mm runs east, depth_mm north. Wall centerlines.",
        "properties": {"x": {"type": "number"}, "y": {"type": "number"}, "width_mm": {"type": "number"}, "depth_mm": {"type": "number"}},
        "required": ["x", "y", "width_mm", "depth_mm"],
    })
}

fn railing() -> Value {
    json!({
        "description": "true (1000 mm bars), false, or {height_mm, style: bars|glass|solid}.",
        "anyOf": [
            {"type": "boolean"},
            {"type": "object", "properties": {"height_mm": {"type": "number"}, "style": {"type": "string", "enum": ["bars", "glass", "solid"]}}},
        ],
    })
}

const USAGES: [&str; 14] = [
    "living", "dining", "kitchen", "bedroom", "master_bedroom", "bathroom", "powder_room", "laundry", "garage", "porch", "hallway", "storage", "office", "other",
];
const STYLES: [&str; 6] = ["swing_single", "swing_double", "sliding", "fixed", "casement", "jalousie"];
const SHEETS: [&str; 6] = ["plan", "lighting", "power", "plumbing", "plumbing_isometric", "aircon"];

fn def(name: &'static str, description: String, schema: Value, read_only: bool) -> ToolDef {
    ToolDef { name, description, schema, read_only, open_world: false }
}

fn with_scope(mut schema: Value) -> Value {
    schema["properties"]["scope"] = scope_schema();
    schema
}

fn add_walls_props() -> Value {
    json!({
        "points": pts("Two or more corner points, in order."),
        "closed": {"type": "boolean", "description": "Join the last point back to the first."},
        "thickness_mm": num("Default: the project's, 150 mm CHB."),
        "height_mm": num("Default: the level height (site walls 1800). A tall wall for a double-height room: give it here."),
        "role": enumerated(&["building", "site"], "site: a lot fence or boundary wall, never part of a room or the floor area."),
        "material_id": string("Surface material id (query what \"materials\")."),
        "level": level(),
    })
}

fn add_room_props() -> Value {
    json!({
        "rect": rect(),
        "polygon": pts("Corner points of any outline, in order, on wall centerlines."),
        "name": string("For example \"Kitchen\"."),
        "usage": enumerated(&USAGES, "Drives the review checks."),
        "floor_material_id": string("Floor material id."),
        "walls": {"type": "boolean", "description": "Draw the walls (default true). Sides an existing wall already covers are shared, not doubled. false names the room existing walls enclose."},
        "thickness_mm": num("Wall thickness. Default: the project's."),
        "height_mm": num("Wall height. Default: the level height."),
        "level": level(),
    })
}

fn add_opening_props() -> Value {
    json!({
        "kind": enumerated(&["door", "window"], ""),
        "at": pt("A point on the wall: the engine finds the wall and the offset."),
        "wall_id": string("Or the wall, with offset_mm from its start to the opening's center (default: the middle)."),
        "offset_mm": num(""),
        "width_mm": num("Door 900, window 1200 by default."),
        "height_mm": num("Door 2100, window 1200 by default."),
        "sill_mm": num("Windows only, default 900."),
        "style": enumerated(&STYLES, ""),
        "swing_into": string("Room id the leaf swings into."),
        "flip_hinge": {"type": "boolean", "description": "Hinge on the jamb nearer the wall end."},
        "material_id": string(""),
        "level": level(),
    })
}

fn add_stair_props() -> Value {
    json!({
        "start": pt("Center of the first riser."),
        "direction_deg": num("Direction you walk up, degrees counter-clockwise from east. Default 90 (north)."),
        "width_mm": num("Default 1000."),
        "riser_count": {"type": "integer", "description": "Default: risers of about 175 mm up the level height."},
        "run_mm": num("Plan length of the flight. Or tread_mm."),
        "tread_mm": num("Default 250."),
        "level": level(),
        "to_level": string("The level it arrives at: must be the one the level's height reaches."),
        "shape": enumerated(&["straight"], "Straight flights only for now."),
        "railing": railing(),
    })
}

fn add_column_props() -> Value {
    json!({
        "at": pt("Center."),
        "points": pts("Centers of several columns."),
        "shape": enumerated(&["rect", "round"], ""),
        "width_mm": num("Default 300 (the diameter when round)."),
        "depth_mm": num("Default: width_mm."),
        "rotation_deg": num(""),
        "level": level(),
        "to_level": string("Also put it on every level up to this one (inclusive)."),
        "material_id": string("For example mat-concrete-fairface."),
    })
}

fn add_slab_props() -> Value {
    json!({
        "slab_kind": enumerated(&["floor", "balcony", "deck", "void"], "floor: floor where the level's own does not reach. balcony, deck: open floor outside the walls (not floor area). void: cuts the level's floor away so the space below is double height."),
        "polygon": pts("Outline, 3 or more points."),
        "rect": rect(),
        "thickness_mm": num("Default 200 (floor), 150 (balcony, deck). Not for a void."),
        "top_offset_mm": num("Top above the level floor, default 0."),
        "railing": railing(),
        "material_id": string(""),
        "level": level(),
    })
}

fn place_asset_props() -> Value {
    json!({
        "catalog_key": string("A key from query what \"catalog\" (search finds them), for example bed-double."),
        "at": pt("Center of the footprint."),
        "rotation_deg": num("Counter-clockwise. The item's back is its local +y."),
        "snap": {"type": "boolean", "description": "Wall items snap their back to the nearest wall within 1000 mm (default true)."},
        "level": level(),
    })
}

/// Every v2 tool, core first, then the extended toolsets.
pub fn definitions() -> Vec<ToolDef> {
    let mut out = vec![];
    out.push(def(
        "project",
        format!("Projects on this computer. action list: every project, newest first. open (id) or create (name, template blank, sample-bungalow or plumbing-demo): it becomes the document every tool acts on. close. save_version (label): a version the user can restore, not an undo step. In a live session this computer hosts, open, create and close end it for everyone: they refuse with live_session; pass force only after the user confirms. {MM}"),
        obj(json!({
            "action": enumerated(&["list", "open", "create", "close", "save_version"], ""),
            "id": string("Project id, for open."),
            "name": string("Project name, for create."),
            "template": enumerated(&["blank", "sample-bungalow", "plumbing-demo"], "For create. Default blank."),
            "label": string("For save_version."),
            "force": {"type": "boolean", "description": "Only after the user confirms."},
        }), &["action"]),
        false,
    ));
    out.push(def(
        "get_guide",
        format!("A guide as markdown. conventions: units, coordinates, joins, openings, levels, slabs. workflows: how to draft a house and the verify loop. ph_defaults: sizes and material ids. catalog: every library item key. Read conventions and workflows before drawing. {MM}"),
        obj(json!({"topic": enumerated(&["conventions", "workflows", "ph_defaults", "catalog"], "")}), &["topic"]),
        true,
    ));
    out.push(def(
        "query",
        format!("Read the model; numbers come from the engine, never estimate them. what: summary (totals), levels, rooms, elements (filter by kind, level, ids), describe (full data of ids), selection (what the user selected in the window), schedule (device counts), takeoff (pipe runs), catalog (library, with search), materials, history. Compact rows by default; format full or fields for more. Paged: page, page_size. {MM}"),
        obj(json!({
            "what": enumerated(&["summary", "levels", "rooms", "elements", "describe", "selection", "schedule", "takeoff", "catalog", "materials", "history"], ""),
            "kind": enumerated(&["wall", "door", "window", "opening", "room", "column", "stair", "slab", "asset", "camera", "pipe", "annotation", "dimension", "underlay"], "For elements."),
            "level": level(),
            "ids": ids("For describe, or to filter elements."),
            "fields": ids("Keep only these fields of each row."),
            "format": enumerated(&["compact", "full"], "Default compact."),
            "search": string("For catalog: words in the key, name or category."),
            "page": {"type": "integer", "description": "From 1."},
            "page_size": {"type": "integer", "description": "Default 50, at most 200."},
        }), &["what"]),
        true,
    ));
    out.push(def(
        "look",
        format!("A picture to check the model with. mode plan: the plan of a level drawn by the engine (no window needed), with show dims, ids, grid, scale_bar; bbox crops it; overlay underlay draws the traced image. The result maps pixels to mm. mode 3d: the 3D view in the Guhit Studio window, or a saved view (view); the capture is kept in Visuals. section and elevation are not in this version. {MM}"),
        obj(json!({
            "mode": enumerated(&["plan", "3d", "section", "elevation"], "Default plan."),
            "level": level(),
            "show": {"type": "array", "items": {"type": "string", "enum": ["dims", "ids", "grid", "scale_bar", "labels", "furniture"]}, "description": "Default: dims, labels, furniture, scale_bar."},
            "bbox": {"type": "object", "description": "{min: {x, y}, max: {x, y}} in mm.", "properties": {"min": pt(""), "max": pt("")}},
            "overlay": enumerated(&["underlay"], ""),
            "width_px": {"type": "integer", "description": "Long side, 400 to 1568. Default 1400."},
            "source": enumerated(&["engine", "window"], "Plan only. window: the app's own canvas, when it is open."),
            "view": string("3d: a saved view's id or name; the picture is from that view's camera."),
            "visibility": {"type": "object", "description": "3d: roof (default true) and cutaway at the active level (default false). Left out, the window draws the model as drawn.", "properties": {"roof": {"type": "boolean"}, "cutaway": {"type": "boolean"}}, "additionalProperties": false},
        }), &[]),
        true,
    ));
    out.push(def(
        "measure",
        format!("Measure from the model, never by eye. kind distance: between from and to (points or element ids); clear_width: face to face between two parallel walls (ids), or a room's clear width and depth, or a door's width; height: walls, levels (floor to floor and clear), openings, stairs (riser, tread); area: rooms (net), slabs, or a polygon; clearance: the gap between two elements' outlines. {MM}"),
        obj(json!({
            "kind": enumerated(&["distance", "clear_width", "height", "area", "clearance"], ""),
            "from": {"description": "A point {x, y} or an element id."},
            "to": {"description": "A point {x, y} or an element id."},
            "ids": ids("Elements, or level ids for height."),
            "polygon": pts("For area."),
        }), &["kind"]),
        true,
    ));
    out.push(def(
        "check",
        format!("The design review: suggestions to check, never permit approval or code compliance. Every item with its code, element_ids, location_mm and status (open, or ignored with the designer's note: never approved). since_revision lists what changed since then; ids, codes and level filter. Also rooms with no outside window and storeys that overlap. Call it before handing over. {MM}"),
        obj(json!({
            "since_revision": {"type": "integer", "description": "Compare with the items at this revision (a result's base_revision)."},
            "ids": ids("Only items about these elements."),
            "codes": ids("Only these checks, for example door_narrow."),
            "level": level(),
            "include_ignored": {"type": "boolean", "description": "Default true."},
        }), &[]),
        true,
    ));
    out.push(def(
        "build",
        format!("Draft a whole level or house in one call, as ONE undo step, all or nothing. spec holds lists: levels, walls, rooms, columns, slabs, stairs, openings, assets, cameras, applied in that order. Each entry takes the arguments of add_level-like levels (name, elevation_mm, height_mm), add_walls, add_room, add_column, add_slab, add_stair, add_opening, place_asset or a camera (name, position, target {{x, y, z}}), plus an optional ref; a later entry names it as \"$ref\". dry_run checks it without committing. The result maps every ref to its id and lists the review items it added. {MM}"),
        with_scope(obj(json!({
            "spec": {"type": "object", "properties": {
                "levels": {"type": "array", "items": {"type": "object"}},
                "walls": {"type": "array", "items": {"type": "object"}},
                "rooms": {"type": "array", "items": {"type": "object"}},
                "columns": {"type": "array", "items": {"type": "object"}},
                "slabs": {"type": "array", "items": {"type": "object"}},
                "stairs": {"type": "array", "items": {"type": "object"}},
                "openings": {"type": "array", "items": {"type": "object"}},
                "assets": {"type": "array", "items": {"type": "object"}},
                "cameras": {"type": "array", "items": {"type": "object"}},
            }},
            "label": string("Undo label, for example \"Ground floor\"."),
            "replace_level": string("Clear this level (its elements, not the level) first."),
            "dry_run": {"type": "boolean"},
        }), &["spec"])),
        false,
    ));
    out.push(def(
        "batch",
        format!("Several edits as ONE undo step, all or nothing, in order. Each step is {{tool, args, ref}} with tool one of the editing tools (add_walls, add_room, add_opening, add_stair, add_column, add_slab, place_asset, modify, reshape, delete, levels, set_project and the service tools). A later step names what an earlier one made as \"$ref\" (\"$ref.2\": its third element). dry_run checks without committing. A failure names its step and ref. {MM}"),
        with_scope(obj(json!({
            "steps": {"type": "array", "items": {"type": "object", "properties": {"tool": {"type": "string"}, "args": {"type": "object"}, "ref": {"type": "string"}}, "required": ["tool", "args"]}},
            "label": string("Undo label."),
            "dry_run": {"type": "boolean"},
        }), &["steps"])),
        false,
    ));
    out.push(def("add_walls", format!("Straight walls through points, one undo step. Ends within 1 mm join; T junctions work. height_mm makes a tall wall (a double-height room). role site: a lot fence. {MM}"), with_scope(obj(add_walls_props(), &["points"])), false));
    out.push(def("add_room", format!("A named room: rect (south-west corner, width_mm east, depth_mm north) or polygon, on wall centerlines. Draws the walls unless walls is false; a side an existing wall covers is shared. With 150 mm walls a 4000 x 3000 rect has a net floor of 3850 x 2850. {MM}"), with_scope(obj(add_room_props(), &[])), false));
    out.push(def("add_opening", format!("A door or window. at: a point on the wall and the engine finds the wall; or wall_id and offset_mm. style: swing_single, swing_double, sliding, fixed, casement, jalousie. swing_into: the room the leaf opens into. A door between rooms goes on their shared wall, windows on outside walls. {MM}"), with_scope(obj(add_opening_props(), &["kind"])), false));
    out.push(def("add_stair", format!("A straight stair from start, walking up direction_deg, rising the level's floor-to-floor height to the level above. Risers default to about 175 mm and treads to 250; the result gives the riser height and tread depth. A 900 mm handrail unless railing is false. Cut the floor above with add_slab slab_kind void. {MM}"), with_scope(obj(add_stair_props(), &["start"])), false));
    out.push(def("add_column", format!("Columns at a point or points: rect or round, 300 mm by default. to_level repeats them on every level up to it, for frames through several storeys. {MM}"), with_scope(obj(add_column_props(), &[])), false));
    out.push(def("add_slab", format!("A slab on a level: a floor, a balcony or deck (with a railing, balcony 1000 mm bars by default) or a void that makes the room below double height (stairwells too). {MM}"), with_scope(obj(add_slab_props(), &[])), false));
    out.push(def("place_asset", format!("One library item: furniture, fixtures, lights, outlets, switches, panelboard, detectors, aircon. An unknown key answers with the nearest keys. Wall items snap to the nearest wall face; ceiling items hang from the ceiling. {MM}"), with_scope(obj(place_asset_props(), &["catalog_key", "at"])), false));
    out.push(def(
        "modify",
        format!("Change elements, one undo step: move {{dx_mm, dy_mm}} (joined walls stretch unless stretch_connected is false), rotate_deg about pivot (default their center), set fields: rooms name, usage, floor_material_id; openings width_mm, height_mm, sill_mm, style, swing_into, offset_mm; walls thickness_mm, height_mm, material_id; columns, stairs, slabs, objects their sizes, railing, material_id. {MM}"),
        with_scope(obj(json!({
            "ids": ids("Elements to change."),
            "move": {"type": "object", "properties": {"dx_mm": {"type": "number"}, "dy_mm": {"type": "number"}}},
            "stretch_connected": {"type": "boolean"},
            "rotate_deg": num("Counter-clockwise."),
            "pivot": pt(""),
            "set": {"type": "object", "description": "Field values, applied to each element whose kind takes them."},
        }), &["ids"])),
        false,
    ));
    out.push(def(
        "reshape",
        format!("Reshape walls and rooms, one undo step. op resize_room: room_id, side north|south|east|west, delta_mm (outward; negative inward). set_wall_length: wall_id, length_mm, anchor start|end|center. set_wall_endpoints: wall_id, start, end. split_wall: wall_id, at_mm from its start or at {{x, y}}. Joined walls follow; the result lists them under also_moved. {MM}"),
        with_scope(obj(json!({
            "op": enumerated(&["resize_room", "set_wall_length", "set_wall_endpoints", "split_wall"], ""),
            "room_id": string(""), "side": enumerated(&["north", "south", "east", "west"], ""), "delta_mm": num(""),
            "wall_id": string(""), "length_mm": num(""), "anchor": enumerated(&["start", "end", "center"], ""),
            "start": pt(""), "end": pt(""), "at_mm": num(""), "at": pt(""),
        }), &["op"])),
        false,
    ));
    out.push(def("delete", format!("Delete elements, one undo step. A wall takes its doors and windows with it. {MM}"), with_scope(obj(json!({"ids": ids("")}), &["ids"])), false));
    out.push(def(
        "levels",
        format!("Storeys. action list; add (name, elevation_mm, default on top of the highest; height_mm floor to floor, default 3000); update (level, then name, elevation_mm or height_mm: the ground floor's height too); delete (level, with everything on it: only when the user names it). The result warns when one storey's height runs past the floor above. {MM}"),
        obj(json!({
            "action": enumerated(&["list", "add", "update", "delete"], ""),
            "level": string("Level id or name, for update and delete."),
            "name": string(""), "elevation_mm": num(""), "height_mm": num(""),
        }), &["action"]),
        false,
    ));
    out.push(def(
        "set_project",
        format!("Project-wide values, one undo step: roof {{kind none|flat|shed|gable, pitch_deg, overhang_mm, thickness_mm, ridge_axis x|y, material_id}}, site {{city, latitude_deg, longitude_deg, utc_offset_min}} for the sun, north_angle_deg, settings {{client_name, location, designer, default_wall_thickness_mm, scale_denominator, paper, grid_mm}}. {MM}"),
        obj(json!({
            "roof": {"type": "object"}, "site": {"type": "object"}, "north_angle_deg": num("True north from +y, counter-clockwise."), "settings": {"type": "object"},
        }), &[]),
        false,
    ));
    out.push(def(
        "history",
        format!("The shared undo history (who made each step, when). action list; undo or redo (steps, default 1); undo_to (index from list: that step and every later one). In a live session someone else's step refuses with other_author; pass force only after the user confirms. {MM}"),
        obj(json!({
            "action": enumerated(&["list", "undo", "redo", "undo_to"], ""),
            "steps": {"type": "integer"}, "index": {"type": "integer"}, "limit": {"type": "integer", "description": "For list, default 20."},
            "force": {"type": "boolean", "description": "Only after the user confirms."},
        }), &["action"]),
        false,
    ));
    out.push(def(
        "export",
        format!("Write the project to the app's exports folder and return the paths. pdf, svg, dxf: plan sheets of a level, or level \"all\" for one per level; sheet picks plan or a service layout; review_page adds the review items to a PDF. ifc and dxf3d: the whole model. guhit: the project bundle. glb is written by the app's Export menu. {MM}"),
        obj(json!({
            "format": enumerated(&["pdf", "svg", "dxf", "dxf3d", "ifc", "glb", "guhit"], ""),
            "level": string("Level id or name, or \"all\". Default: the first level."),
            "sheet": enumerated(&SHEETS, ""),
            "paper": enumerated(&["a4", "a3", "a2", "a1"], ""),
            "orientation": enumerated(&["landscape", "portrait"], ""),
            "scale_denominator": {"type": "integer"},
            "show_dimensions": {"type": "boolean"},
            "review_page": {"type": "boolean"},
        }), &["format"]),
        false,
    ));
    out.extend(crate::extended::definitions());
    out
}

// ---------------------------------------------------------------- dispatch

/// Run a v2 tool, or None when `name` is not one this module answers.
pub async fn call(app: &AppService, name: &str, args: &Value, ctx: &CallContext) -> Option<Result<Output, ToolFail>> {
    Some(match name {
        "project" => project(app, args, ctx).await,
        "get_guide" => guide(args),
        "query" => query(app, args, ctx).await,
        "look" => crate::look::look(app, args, ctx).await,
        "measure" => crate::look::measure(app, args).await,
        "check" => check(app, args).await,
        "build" => build(app, args, ctx).await,
        "batch" => batch(app, args, ctx).await,
        "levels" if args.get("action").and_then(Value::as_str) == Some("list") => levels_list(app).await,
        "history" => history(app, args, ctx).await,
        "export" => export(app, args).await,
        n if translate::EDIT_TOOLS.contains(&n) && !matches!(n, "set_review_mark" | "add_camera") => single(app, n, args, ctx).await,
        _ => return None,
    })
}

/// One editing tool, as one undo step.
async fn single(app: &AppService, tool: &str, args: &Value, ctx: &CallContext) -> Result<Output, ToolFail> {
    let mut args = args.clone();
    let scope = stage::take_scope(&mut args)?;
    let label = format!("MCP: {}", tool.replace('_', " "));
    stage::run(app, vec![StepIn::new(tool, args)], scope.as_ref(), Options { label, dry_run: false, per_step: false }, ctx).await
}

// ------------------------------------------------------------------ project

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectIn {
    action: String,
    id: Option<String>,
    name: Option<String>,
    template: Option<String>,
    label: Option<String>,
    force: Option<bool>,
}

async fn project(app: &AppService, args: &Value, ctx: &CallContext) -> Result<Output, ToolFail> {
    let i: ProjectIn = parse(args)?;
    let force = json!({"force": i.force.unwrap_or(false)});
    let v1 = |extra: Value| {
        let mut a = force.clone();
        for (k, v) in extra.as_object().cloned().unwrap_or_default() {
            a[k] = v;
        }
        a
    };
    match i.action.as_str() {
        "list" => crate::tools::dispatch(app, "list_projects", &json!({}), ctx).await,
        "open" => {
            let id = i.id.ok_or_else(|| ToolFail::args("action \"open\" needs id (project action \"list\" shows them)"))?;
            crate::tools::dispatch(app, "open_project", &v1(json!({"id": id})), ctx).await
        }
        "create" => {
            let name = i.name.unwrap_or_else(|| "Untitled project".into());
            let template = i.template.unwrap_or_else(|| "blank".into());
            crate::tools::dispatch(app, "create_project", &v1(json!({"name": name, "template": template})), ctx).await
        }
        "close" => crate::tools::dispatch(app, "close_project", &force, ctx).await,
        "save_version" => {
            let label = i.label.ok_or_else(|| ToolFail::args("action \"save_version\" needs label"))?;
            crate::tools::dispatch(app, "save_version", &json!({"label": label}), ctx).await
        }
        other => Err(ToolFail::args(format!("unknown action `{other}`. Use list, open, create, close or save_version"))),
    }
}

fn guide(args: &Value) -> Result<Output, ToolFail> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct GuideIn {
        topic: String,
    }
    let i: GuideIn = parse(args)?;
    Ok(Output::Text(match i.topic.as_str() {
        "conventions" => crate::text::CONVENTIONS.to_string(),
        "workflows" => crate::text::WORKFLOWS.to_string(),
        "ph_defaults" => crate::text::PH_DEFAULTS.to_string(),
        "catalog" => catalog_text(),
        other => {
            return Err(ToolFail::args(format!(
                "unknown topic `{other}`. Use \"conventions\", \"workflows\", \"ph_defaults\" or \"catalog\"."
            )))
        }
    }))
}

/// The library as markdown, by category.
pub(crate) fn catalog_text() -> String {
    let mut out = String::from("# Library\n\nKeys for place_asset. Sizes are width x depth x height in mm.\n");
    let mut last = None;
    for c in defaults::asset_catalog() {
        if last != Some(c.category) {
            let name = serde_json::to_value(c.category).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
            out.push_str(&format!("\n## {name}\n"));
            last = Some(c.category);
        }
        let mount = match c.mount {
            Mount::Wall => ", on a wall",
            Mount::Ceiling => ", from the ceiling",
            Mount::Opening => ", in a wall opening",
            Mount::Floor => "",
        };
        out.push_str(&format!("- `{}`: {}, {:.0} x {:.0} x {:.0}{mount}\n", c.key, c.name, c.width_mm, c.depth_mm, c.height_mm));
    }
    out
}

// ------------------------------------------------------------------- query

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryIn {
    what: String,
    kind: Option<String>,
    level: Option<String>,
    ids: Option<Vec<Id>>,
    fields: Option<Vec<String>>,
    format: Option<String>,
    search: Option<String>,
    page: Option<usize>,
    page_size: Option<usize>,
}

/// The level an element stands on: its own, or its host wall's.
pub(crate) fn element_level<'a>(project: &'a Project, el: &'a Element) -> Option<&'a Id> {
    Some(match el {
        Element::Wall(e) => &e.level_id,
        Element::Room(e) => &e.level_id,
        Element::Column(e) => &e.level_id,
        Element::Stair(e) => &e.level_id,
        Element::Asset(e) => &e.level_id,
        Element::Annotation(e) => &e.level_id,
        Element::Dimension(e) => &e.level_id,
        Element::Underlay(e) => &e.level_id,
        Element::Linework(e) => &e.level_id,
        Element::ReferenceModel(e) => &e.level_id,
        Element::Pipe(e) => &e.level_id,
        Element::Slab(e) => &e.level_id,
        Element::Opening(o) => match project.elements.iter().find(|w| w.id() == &o.wall_id) {
            Some(Element::Wall(w)) => &w.level_id,
            _ => return None,
        },
        Element::Camera(_) => return None,
    })
}

/// The fields a compact row keeps, by the `kind` of the engine's JSON.
fn compact_keys(kind: &str) -> &'static [&'static str] {
    match kind {
        "wall" => &["id", "kind", "level", "start", "end", "length_mm", "thickness_mm", "height_mm", "role", "faces_outside"],
        "opening" => &["id", "kind", "type", "style", "wall_id", "offset_from_wall_start_mm", "width_mm", "height_mm", "sill_mm", "center"],
        "room" => &["id", "kind", "name", "usage", "level", "net_area_m2", "clear_width_x_mm", "clear_depth_y_mm", "enclosed", "label_point"],
        "column" => &["id", "kind", "level", "center", "shape", "width_mm", "depth_mm"],
        "stair" => &["id", "kind", "level", "origin", "rotation_deg", "width_mm", "run_mm", "riser_count"],
        "slab" => &["id", "kind", "slab_kind", "level", "polygon", "thickness_mm", "top_offset_mm", "railing_mm"],
        "object" => &["id", "kind", "name", "catalog_key", "level", "position", "rotation_deg", "in_room"],
        "camera" => &["id", "kind", "name", "position_mm", "target_mm", "fov_deg"],
        "pipe" => &["id", "kind", "label", "system", "level", "diameter_mm", "length_mm"],
        "note" => &["id", "kind", "level", "text", "position"],
        "dimension" => &["id", "kind", "level", "a", "b", "measured_mm"],
        _ => &["id", "kind", "level", "name"],
    }
}

fn pick(row: &Value, keys: &[&str]) -> Value {
    let mut out = Map::new();
    for k in keys {
        if let Some(v) = row.get(*k) {
            if !v.is_null() {
                out.insert((*k).to_string(), v.clone());
            }
        }
    }
    Value::Object(out)
}

fn shape_rows(rows: Vec<Value>, i: &QueryIn) -> Vec<Value> {
    rows.into_iter()
        .map(|row| match (&i.fields, i.format.as_deref()) {
            (Some(fields), _) => {
                let mut keys: Vec<&str> = vec!["id", "kind"];
                keys.extend(fields.iter().map(String::as_str));
                pick(&row, &keys)
            }
            (None, Some("full")) => row,
            _ => {
                let kind = row["kind"].as_str().unwrap_or("").to_string();
                let mut c = pick(&row, compact_keys(&kind));
                // A building wall says nothing about its role.
                if c.get("role").and_then(Value::as_str) == Some("building") {
                    c.as_object_mut().map(|m| m.remove("role"));
                }
                if kind == "object" {
                    if let Some(name) = row["in_room"]["name"].as_str() {
                        c["in_room"] = json!(name);
                    }
                }
                if kind == "room" {
                    c["wall_ids"] = json!(row["bounding_walls"].as_array().map(|w| w.iter().map(|x| x["id"].clone()).collect::<Vec<_>>()));
                    c["doors"] = json!(row["doors"].as_array().map(|d| d.len()).unwrap_or(0));
                    c["windows"] = json!(row["windows"].as_array().map(|d| d.len()).unwrap_or(0));
                }
                c
            }
        })
        .collect()
}

fn paged(what: &str, rows: Vec<Value>, i: &QueryIn, revision: u32) -> Value {
    let default = if i.format.as_deref() == Some("full") { 20 } else { 50 };
    let size = i.page_size.unwrap_or(default).clamp(1, 200);
    let total = rows.len();
    let pages = total.div_ceil(size).max(1);
    let page = i.page.unwrap_or(1).clamp(1, pages);
    let items: Vec<Value> = rows.into_iter().skip((page - 1) * size).take(size).collect();
    let mut out = json!({"what": what, "revision": revision, "count": total, "page": page, "pages": pages, "items": items});
    if page < pages {
        out["next_page"] = json!(page + 1);
    }
    out
}

/// The kinds a `kind` filter names, and the opening type it narrows to.
fn kind_filter(kind: &str) -> Result<(ElementKind, Option<OpeningType>), ToolFail> {
    Ok(match kind {
        "door" => (ElementKind::Opening, Some(OpeningType::Door)),
        "window" => (ElementKind::Opening, Some(OpeningType::Window)),
        "object" | "asset" => (ElementKind::Asset, None),
        "view" | "camera" => (ElementKind::Camera, None),
        "note" | "annotation" => (ElementKind::Annotation, None),
        other => (
            serde_json::from_value(json!(other)).map_err(|_| ToolFail::args(format!("unknown kind `{other}`")))?,
            None,
        ),
    })
}

async fn query(app: &AppService, args: &Value, ctx: &CallContext) -> Result<Output, ToolFail> {
    let i: QueryIn = parse(args)?;
    match i.what.as_str() {
        "selection" => return crate::tools::dispatch(app, "get_selection", &json!({}), ctx).await,
        "history" => return history(app, &json!({"action": "list"}), ctx).await,
        "catalog" => return Ok(Output::Json(catalog_query(&i))),
        "levels" => return levels_list(app).await,
        _ => {}
    }
    let s = app.session.lock().await;
    let doc = s.doc.as_ref().ok_or_else(stage::no_document)?;
    let project = doc.project();
    let revision = doc.revision();
    let level_id = match i.level.as_deref() {
        Some(l) => Some(translate::find_level(project, l)?.id.clone()),
        None => None,
    };
    let engine = |q: Query| doc.query(&q).map_err(ToolFail::from);
    let out = match i.what.as_str() {
        "summary" => {
            let mut v = engine(Query::ProjectSummary)?;
            v["revision"] = json!(revision);
            v
        }
        "rooms" => {
            let v = engine(Query::RoomList)?;
            let rows: Vec<Value> = v["rooms"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|r| {
                    level_id.as_ref().is_none_or(|l| {
                        project.elements.iter().any(|e| matches!(e, Element::Room(room) if room.id == r["id"] && &room.level_id == l))
                    })
                })
                .map(|mut r| {
                    r["kind"] = json!("room");
                    r
                })
                .collect();
            let mut out = paged("rooms", shape_rows(rows, &i), &i, revision);
            out["total_net_area_m2"] = v["total_net_area_m2"].clone();
            out
        }
        "elements" | "describe" => {
            let filter = i.kind.as_deref().map(kind_filter).transpose()?;
            if i.what == "describe" && i.ids.as_ref().is_none_or(|v| v.is_empty()) {
                return Err(ToolFail::args("what \"describe\" needs ids"));
            }
            let wanted: Vec<Id> = project
                .elements
                .iter()
                .filter(|e| i.ids.as_ref().is_none_or(|ids| ids.contains(e.id())))
                .filter(|e| match &filter {
                    None => true,
                    Some((k, t)) => {
                        e.kind() == *k
                            && t.is_none_or(|t| matches!(e, Element::Opening(o) if o.opening_type == t))
                    }
                })
                .filter(|e| level_id.as_ref().is_none_or(|l| element_level(project, e) == Some(l)))
                .map(|e| e.id().clone())
                .collect();
            let missing: Vec<&Id> = i.ids.iter().flatten().filter(|id| !project.elements.iter().any(|e| e.id() == *id)).collect();
            let rows = if wanted.is_empty() {
                vec![]
            } else {
                engine(Query::Describe { ids: wanted })?["elements"].as_array().cloned().unwrap_or_default()
            };
            let shaped = if i.what == "describe" && i.format.is_none() && i.fields.is_none() {
                rows
            } else {
                shape_rows(rows, &i)
            };
            let mut out = paged(&i.what, shaped, &i, revision);
            if !missing.is_empty() {
                out["not_found"] = json!(missing);
            }
            out
        }
        "schedule" => engine(Query::Schedule)?,
        "takeoff" => engine(Query::PipeTakeoff)?,
        "materials" => json!({
            "materials": project.materials.iter().map(|m| json!({"id": m.id, "name": m.name, "category": m.category})).collect::<Vec<_>>(),
        }),
        other => {
            return Err(ToolFail::args(format!(
                "unknown what `{other}`. Use summary, levels, rooms, elements, describe, selection, schedule, takeoff, catalog, materials or history"
            )))
        }
    };
    Ok(Output::Json(out))
}

fn catalog_query(i: &QueryIn) -> Value {
    let words: Vec<String> = i
        .search
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase()
        .split([' ', ',', '-', '_'])
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    let rows: Vec<Value> = defaults::asset_catalog()
        .into_iter()
        .filter(|c| {
            let hay = format!("{} {} {}", c.key, c.name, serde_json::to_value(c.category).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()).to_ascii_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .map(|c| json!({"key": c.key, "name": c.name, "category": c.category, "mount": c.mount, "width_mm": c.width_mm, "depth_mm": c.depth_mm, "height_mm": c.height_mm}))
        .collect();
    let mut out = json!({"what": "catalog", "count": rows.len(), "items": rows});
    if rows_empty(&out) {
        if let Some(s) = &i.search {
            out["did_you_mean"] = json!(translate::catalog_suggestions(s, 6));
        }
    }
    out
}

fn rows_empty(v: &Value) -> bool {
    v["count"] == 0
}

async fn levels_list(app: &AppService) -> Result<Output, ToolFail> {
    let s = app.session.lock().await;
    let doc = s.doc.as_ref().ok_or_else(stage::no_document)?;
    let project = doc.project();
    let mut levels: Vec<&Level> = project.levels.iter().collect();
    levels.sort_by(|a, b| a.elevation_mm.total_cmp(&b.elevation_mm));
    let rows: Vec<Value> = levels
        .iter()
        .map(|l| {
            let count = |k: ElementKind| {
                project.elements.iter().filter(|e| e.kind() == k && element_level(project, e) == Some(&l.id)).count()
            };
            json!({
                "id": l.id, "name": l.name,
                "elevation_mm": l.elevation_mm, "height_mm": l.height_mm, "top_mm": l.elevation_mm + l.height_mm,
                "walls": count(ElementKind::Wall), "rooms": count(ElementKind::Room), "stairs": count(ElementKind::Stair),
                "slabs": count(ElementKind::Slab), "columns": count(ElementKind::Column), "objects": count(ElementKind::Asset),
            })
        })
        .collect();
    let mut out = json!({"revision": doc.revision(), "levels": rows});
    let warnings = translate::level_overlaps(project);
    if !warnings.is_empty() {
        out["warnings"] = json!(warnings);
    }
    Ok(Output::Json(out))
}

// ------------------------------------------------------------------- check

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckIn {
    since_revision: Option<u32>,
    ids: Option<Vec<Id>>,
    codes: Option<Vec<String>>,
    level: Option<String>,
    include_ignored: Option<bool>,
}

async fn check(app: &AppService, args: &Value) -> Result<Output, ToolFail> {
    let i: CheckIn = parse(args)?;
    let s = app.session.lock().await;
    let doc = s.doc.as_ref().ok_or_else(stage::no_document)?;
    let project = doc.project();
    let derived = doc.derived();
    let revision = doc.revision();
    stage::note_issues(&project.id, revision, &derived.issues);
    let level_id = match i.level.as_deref() {
        Some(l) => Some(translate::find_level(project, l)?.id.clone()),
        None => None,
    };
    let keep = |issue: &Issue| {
        let by_code = i.codes.as_ref().is_none_or(|c| c.iter().any(|c| c == &issue.code));
        let by_ids = i.ids.as_ref().is_none_or(|ids| issue.element_ids.iter().any(|e| ids.contains(e)));
        let by_level = level_id.as_ref().is_none_or(|l| {
            issue.element_ids.iter().any(|id| project.elements.iter().any(|e| e.id() == id && element_level(project, e) == Some(l)))
        });
        let by_status = i.include_ignored.unwrap_or(true) || issue.status == IssueStatus::Open;
        by_code && by_ids && by_level && by_status
    };
    let items: Vec<&Issue> = derived.issues.iter().filter(|x| keep(x)).collect();
    let open = items.iter().filter(|x| x.status == IssueStatus::Open).count();
    let mut out = json!({
        "revision": revision,
        "count": items.len(),
        "open_count": open,
        "ignored_count": items.len() - open,
        "items": items.iter().take(100).map(|x| stage::issue_brief(x)).collect::<Vec<_>>(),
        "note": "Suggestions to check, never code compliance or permit approval. An ignored item was set aside by the designer with a note; it is never approved.",
    });
    if items.len() > 100 {
        out["truncated"] = json!(true);
    }
    if let Some(since) = i.since_revision {
        match stage::issues_at(&project.id, since) {
            Some((at, old)) => {
                let kept: Vec<Issue> = old.into_iter().filter(|x| keep(x)).collect();
                let now: Vec<Issue> = items.iter().map(|x| (*x).clone()).collect();
                let (added, resolved) = stage::issue_change(&kept, &now);
                out["since"] = json!({"revision": at, "asked": since, "added": added, "resolved": resolved});
            }
            None => {
                out["since"] = json!({
                    "asked": since,
                    "note": "This server has no record of the review items at that revision: it notes them at every revision it edits or checks. Every current item is listed.",
                });
            }
        }
    }
    let daylight = doc.query(&Query::RoomsWithoutExteriorWindow).map_err(ToolFail::from)?;
    let rooms: Vec<Value> = daylight["rooms"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|r| i.ids.as_ref().is_none_or(|ids| r["id"].as_str().is_some_and(|id| ids.iter().any(|x| x == id))))
        .map(|r| json!({"id": r["id"], "name": r["name"], "can_get_a_window": r["can_get_a_window"]}))
        .collect();
    out["rooms_without_outside_window"] = json!(rooms);
    let warnings = translate::level_overlaps(project);
    if !warnings.is_empty() {
        out["level_warnings"] = json!(warnings);
    }
    out["resolved_marks"] = json!(derived.review_resolved.len());
    Ok(Output::Json(out))
}

// ------------------------------------------------------------------- build

/// The spec's lists, in the order they are applied, and the tool each
/// entry takes the arguments of.
const BUILD_ORDER: [(&str, &str); 9] = [
    ("levels", "levels"),
    ("walls", "add_walls"),
    ("rooms", "add_room"),
    ("columns", "add_column"),
    ("slabs", "add_slab"),
    ("stairs", "add_stair"),
    ("openings", "add_opening"),
    ("assets", "place_asset"),
    ("cameras", "add_camera"),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildIn {
    spec: Map<String, Value>,
    label: Option<String>,
    replace_level: Option<String>,
    dry_run: Option<bool>,
}

async fn build(app: &AppService, args: &Value, ctx: &CallContext) -> Result<Output, ToolFail> {
    let mut args = args.clone();
    let scope = stage::take_scope(&mut args)?;
    let i: BuildIn = parse(&args)?;
    if let Some(extra) = i.spec.keys().find(|k| !BUILD_ORDER.iter().any(|(s, _)| s == k)) {
        return Err(ToolFail::args(format!(
            "spec has an unknown list `{extra}`. Lists: {}",
            BUILD_ORDER.iter().map(|(s, _)| *s).collect::<Vec<_>>().join(", ")
        )));
    }
    let mut steps = vec![];
    if let Some(level) = &i.replace_level {
        let s = app.session.lock().await;
        let doc = s.doc.as_ref().ok_or_else(stage::no_document)?;
        let project = doc.project();
        let id = translate::find_level(project, level)?.id.clone();
        let ids: Vec<Id> = project
            .elements
            .iter()
            .filter(|e| !matches!(e, Element::Opening(_)) && element_level(project, e) == Some(&id))
            .map(|e| e.id().clone())
            .collect();
        if !ids.is_empty() {
            let mut step = StepIn::new("delete", json!({"ids": ids}));
            step.origin = Some("replace_level".into());
            steps.push(step);
        }
    }
    for (list, tool) in BUILD_ORDER {
        let Some(entries) = i.spec.get(list) else { continue };
        let entries = entries.as_array().ok_or_else(|| ToolFail::args(format!("spec.{list} must be a list")))?;
        for (n, entry) in entries.iter().enumerate() {
            let mut entry = entry.as_object().cloned().ok_or_else(|| ToolFail::args(format!("spec.{list}[{n}] must be an object")))?;
            let r#ref = match entry.remove("ref") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().trim_start_matches('$').to_string()),
                Some(_) => return Err(ToolFail::args(format!("spec.{list}[{n}].ref must be a name"))),
            };
            entry.remove("agent");
            if list == "levels" {
                entry.entry("action").or_insert(json!("add"));
            }
            steps.push(StepIn { tool: tool.to_string(), args: Value::Object(entry), r#ref, origin: Some(format!("{list}[{n}]")) });
        }
    }
    if steps.is_empty() {
        return Err(ToolFail::args("spec is empty, nothing would change"));
    }
    let label = match i.label.as_deref().map(str::trim) {
        Some(l) if !l.is_empty() => format!("MCP: {l}"),
        _ => format!("MCP: build of {} elements", steps.len()),
    };
    stage::run(app, steps, scope.as_ref(), Options { label, dry_run: i.dry_run.unwrap_or(false), per_step: true }, ctx).await
}

async fn batch(app: &AppService, args: &Value, ctx: &CallContext) -> Result<Output, ToolFail> {
    let mut args = args.clone();
    let scope = stage::take_scope(&mut args)?;
    let steps = args
        .get("steps")
        .and_then(Value::as_array)
        .ok_or_else(|| ToolFail::msg("invalid arguments: `steps` must be an array"))?;
    if steps.is_empty() {
        return Err(ToolFail::msg("invalid arguments: `steps` is empty, nothing would change"));
    }
    if let Some(extra) = args.as_object().and_then(|m| m.keys().find(|k| !matches!(k.as_str(), "steps" | "label" | "dry_run")).cloned()) {
        return Err(ToolFail::args(format!("unknown field `{extra}`; batch takes steps, label, dry_run and scope")));
    }
    let mut parsed: Vec<StepIn> = vec![];
    for (i, step) in steps.iter().enumerate() {
        let tool = step
            .get("tool")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolFail::msg(format!("invalid arguments: steps[{i}] has no `tool`")))?;
        if !translate::is_edit(tool) || tool == "set_review_mark" {
            let mut f = ToolFail::msg(format!(
                "invalid arguments: steps[{i}].tool is `{tool}`, which is not an editing tool. Editing tools: {}",
                translate::EDIT_TOOLS.iter().filter(|t| **t != "set_review_mark").cloned().collect::<Vec<_>>().join(", ")
            ));
            if let Some(n) = crate::toolsets::nearest(tool).filter(|n| translate::is_edit(n)) {
                f = f.with_hint(format!("Did you mean `{n}`?"));
            }
            return Err(f);
        }
        let mut step_args = step.get("args").cloned().unwrap_or_else(|| json!({}));
        if !step_args.is_object() {
            return Err(ToolFail::msg(format!("invalid arguments: steps[{i}].args must be a JSON object")));
        }
        if step_args.get("scope").is_some() {
            return Err(ToolFail::msg(format!(
                "invalid arguments: steps[{i}].args has a `scope`. Give scope once, on the batch itself"
            )));
        }
        crate::agent::take_argument(&mut step_args);
        let r#ref = match step.get("ref") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().trim_start_matches('$').to_string()),
            Some(_) => return Err(ToolFail::args(format!("steps[{i}].ref must be a name"))),
        };
        parsed.push(StepIn { tool: tool.to_string(), args: step_args, r#ref, origin: None });
    }
    let label = match args.get("label").and_then(Value::as_str).map(str::trim) {
        Some(l) if !l.is_empty() => format!("MCP: {l}"),
        _ => format!("MCP: batch of {} steps", parsed.len()),
    };
    let dry_run = args.get("dry_run").and_then(Value::as_bool).unwrap_or(false);
    let per_step = parsed.len() > 1;
    stage::run(app, parsed, scope.as_ref(), Options { label, dry_run, per_step }, ctx).await
}

// ----------------------------------------------------------------- history

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryIn {
    action: String,
    steps: Option<u32>,
    index: Option<u32>,
    limit: Option<usize>,
    force: Option<bool>,
}

fn step_brief(index: usize, s: &HistoryStep) -> Value {
    let mut v = json!({"index": index, "label": s.label, "summary": s.summary, "origin": s.origin, "at": s.at, "elements": s.ids.len() + s.removed.len()});
    if let Some(n) = &s.author_name {
        v["by"] = json!(n);
    }
    if let Some(n) = &s.on_behalf_name {
        v["for"] = json!(n);
    }
    v
}

async fn history(app: &AppService, args: &Value, ctx: &CallContext) -> Result<Output, ToolFail> {
    let i: HistoryIn = parse(args)?;
    let force = i.force.unwrap_or(false);
    let step = |cmd: &str, steps: u32, expected: Option<u32>| {
        let mut a = json!({"force": force, "steps": steps});
        if let Some(r) = expected {
            a["expected_revision"] = json!(r);
        }
        (cmd.to_string(), a)
    };
    let (cmd, call_args) = match i.action.as_str() {
        "list" => {
            let list: HistoryList = serde_json::from_value(app.handle("history_list", json!({})).await?)
                .map_err(|e| ToolFail::new("io", e.to_string()))?;
            let limit = i.limit.unwrap_or(20).clamp(1, 200);
            return Ok(Output::Json(json!({
                "undo": list.undo.iter().take(limit).enumerate().map(|(n, s)| step_brief(n, s)).collect::<Vec<_>>(),
                "redo": list.redo.iter().take(limit).enumerate().map(|(n, s)| step_brief(n, s)).collect::<Vec<_>>(),
                "undo_count": list.undo.len(),
                "redo_count": list.redo.len(),
                "note": "undo[0] is what an undo takes back. undo_to with an index takes back that step and every later one.",
            })));
        }
        "undo" => step("doc_undo", i.steps.unwrap_or(1).max(1), None),
        "redo" => step("doc_redo", i.steps.unwrap_or(1).max(1), None),
        "undo_to" => {
            let index = i.index.ok_or_else(|| ToolFail::args("action \"undo_to\" needs index, from history action \"list\""))?;
            let revision: u32 = app.handle("doc_revision", json!({})).await?["revision"].as_u64().unwrap_or(0) as u32;
            step("doc_undo", index + 1, Some(revision))
        }
        other => return Err(ToolFail::args(format!("unknown action `{other}`. Use list, undo, redo or undo_to"))),
    };
    let redo = cmd == "doc_redo";
    let steps = call_args["steps"].as_u64().unwrap_or(1).max(1) as u32;
    let state: Value = match &ctx.call {
        // One step: the registry's own undo, which checks whose it is.
        Some(call) if steps == 1 && call_args.get("expected_revision").is_none() => {
            serde_json::to_value(app.agent_undo(call, redo, force).await?).map_err(|e| ToolFail::new("io", e.to_string()))?
        }
        // Several: every step is checked first, then they go back as one
        // revision, and only if the plan is still where it was checked.
        Some(call) => {
            let revision = match call_args["expected_revision"].as_u64() {
                Some(r) => r as u32,
                None => app.handle("doc_revision", json!({})).await?["revision"].as_u64().unwrap_or(0) as u32,
            };
            let list: HistoryList = serde_json::from_value(app.handle("history_list", json!({})).await?)
                .map_err(|e| ToolFail::new("io", e.to_string()))?;
            check_own_steps(if redo { &list.redo } else { &list.undo }, steps, &call.id, force, redo)?;
            app.handle(&cmd, json!({"steps": steps, "force": true, "expected_revision": revision})).await?
        }
        None => app.handle(&cmd, call_args).await?,
    };
    Ok(Output::Json(json!({
        "ok": true,
        "revision": state["revision"],
        "what": if cmd == "doc_undo" { "undone" } else { "redone" },
        "can_undo": state["can_undo"],
        "can_redo": state["can_redo"],
        "next_undo": state["undo_label"],
        "next_redo": state["redo_label"],
    })))
}

/// The first `n` steps of `list` are the agent's own, or a person's with
/// `force`. Another agent's step is never taken back (DECISIONS D47).
fn check_own_steps(list: &[HistoryStep], n: u32, agent: &str, force: bool, redo: bool) -> Result<(), ToolFail> {
    let (what, done) = if redo { ("redo", "redone") } else { ("undo", "undone") };
    if list.len() < n as usize {
        return Err(ToolFail::new("invalid", format!("there are only {} steps to {what}", list.len())));
    }
    for (i, step) in list.iter().take(n as usize).enumerate() {
        if step.author_id.as_deref() == Some(agent) {
            continue;
        }
        let who = step.author_name.clone().unwrap_or_else(|| "the user".to_string());
        let an_agent = step.on_behalf_of.is_some() || step.author_id.as_deref().is_some_and(|a| a.starts_with("agent-"));
        if an_agent || !force {
            let hint = if an_agent {
                "An agent takes back only its own steps. Ask the user, or take back fewer steps."
            } else {
                "Ask the user first, then call again with force true."
            };
            return Err(ToolFail::new(
                "other_author",
                format!("step {i} of the {what} list, \"{}\", is {who}'s, so nothing was {done}", step.label),
            )
            .with_hint(hint));
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ export

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportIn {
    format: String,
    level: Option<String>,
    sheet: Option<String>,
    paper: Option<String>,
    orientation: Option<String>,
    scale_denominator: Option<u32>,
    show_dimensions: Option<bool>,
    review_page: Option<bool>,
}

async fn export(app: &AppService, args: &Value) -> Result<Output, ToolFail> {
    let i: ExportIn = parse(args)?;
    let file = |v: Value, level: Option<&Level>| {
        let mut f = json!({"path": v["path"], "scale": v["scale_denominator"].as_u64().map(|n| format!("1:{n}"))});
        if let Some(l) = level {
            f["level"] = json!(l.name);
        }
        f
    };
    let mut skipped: Vec<Value> = vec![];
    let files: Vec<Value> = match i.format.as_str() {
        "pdf" | "svg" | "dxf" => {
            let all = i.level.as_deref().map(str::trim) == Some("all");
            let levels: Vec<Level> = {
                let s = app.session.lock().await;
                let doc = s.doc.as_ref().ok_or_else(stage::no_document)?;
                let p = doc.project();
                match i.level.as_deref().map(str::trim) {
                    Some("all") => p.levels.clone(),
                    Some(l) => vec![translate::find_level(p, l)?.clone()],
                    None => p.levels.first().cloned().into_iter().collect(),
                }
            };
            let mut out = vec![];
            for level in &levels {
                let mut v1 = json!({"format": i.format});
                for (k, v) in [
                    ("sheet", i.sheet.clone().map(Value::from)),
                    ("paper", i.paper.clone().map(Value::from)),
                    ("orientation", i.orientation.clone().map(Value::from)),
                    ("scale_denominator", i.scale_denominator.map(Value::from)),
                    ("show_dimensions", i.show_dimensions.map(Value::from)),
                    ("review_page", i.review_page.map(Value::from)),
                ] {
                    if let Some(v) = v {
                        v1[k] = v;
                    }
                }
                match crate::tools::export_plan(app, &v1, Some(level.id.clone())).await {
                    Ok(result) => out.push(file(result, Some(level))),
                    // Every level: an empty one is left out and named.
                    Err(e) if all && e.message.contains("nothing to export") => {
                        skipped.push(json!({"level": level.name, "why": e.message}))
                    }
                    Err(e) => return Err(e),
                }
            }
            if out.is_empty() {
                return Err(ToolFail::new("invalid", "nothing to export: no level has anything drawn on it yet"));
            }
            out
        }
        "ifc" | "dxf3d" => vec![file(app.handle("export_model", json!({"format": i.format, "path": Value::Null})).await?, None)],
        "guhit" => vec![file(app.handle("bundle_save", json!({"path": Value::Null})).await?, None)],
        "glb" => {
            return Err(ToolFail::new("unavailable", "GLB is written by the window's 3D exporter, which this server version cannot reach")
                .with_hint("Use export format \"ifc\" or \"dxf3d\" for the whole model, or ask the user to use Export > GLB in the app."))
        }
        other => return Err(ToolFail::args(format!("unknown format `{other}`. Use pdf, svg, dxf, dxf3d, ifc, glb or guhit"))),
    };
    let mut out = json!({"ok": true, "format": i.format, "files": files});
    if !skipped.is_empty() {
        out["skipped"] = json!(skipped);
    }
    Ok(Output::Json(out))
}

