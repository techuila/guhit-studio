//! The MCP tool set.
//!
//! Every editing tool reuses the copilot's argument-to-`Command` translation
//! in `guhit_app::ai::tools`, so an MCP client and the in-app copilot behave
//! identically: same argument names, same validation, same engine errors. The
//! JSON schemas for those tools are taken from the copilot's own definitions
//! for the same reason; only the descriptions are rewritten, because an MCP
//! edit is applied at once while a copilot edit is staged for approval.
//!
//! Every editing tool commits exactly one `Command::Batch` with `Origin::Ai`
//! and an undo label prefixed "MCP: ", so one undo reverts one tool call.

use std::collections::BTreeMap;

use guhit_app::ai::tools as copilot;
use guhit_app::{store, AppService};
use guhit_model::*;
use serde_json::{json, Map, Value};

/// A tool call that did not work. The message goes back to the model as a
/// tool error, word for word, so it can correct itself and try again.
pub struct ToolFail(pub String);

impl From<copilot::ToolError> for ToolFail {
    fn from(e: copilot::ToolError) -> Self {
        Self(e.0)
    }
}

impl From<IpcError> for ToolFail {
    fn from(e: IpcError) -> Self {
        let ids = if e.element_ids.is_empty() {
            String::new()
        } else {
            format!(" (elements: {})", e.element_ids.join(", "))
        };
        Self(format!("{}: {}{}", e.code, strip_step_prefix(&e.message), ids))
    }
}

/// Drop the engine's own "Step 2 of 5: " prefix. This server always commits a
/// `Command::Batch`, even for one tool call, so that prefix counts internal
/// leaf commands the caller never wrote. `step_fail` puts the caller's own
/// step number back when the call really was a batch.
fn strip_step_prefix(message: &str) -> String {
    let Some(rest) = message.strip_prefix("Step ") else {
        return message.to_string();
    };
    let Some((head, tail)) = rest.split_once(": ") else {
        return message.to_string();
    };
    let counts: Vec<&str> = head.split(" of ").collect();
    let numeric = counts.len() == 2
        && counts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    if numeric {
        tail.to_string()
    } else {
        message.to_string()
    }
}

/// What a tool produced. Almost everything is JSON; `get_plan_image` hands
/// back a picture.
pub enum Output {
    Json(Value),
    Image { base64: String, mime: String },
}

fn ok(v: Value) -> Result<Output, ToolFail> {
    Ok(Output::Json(v))
}

// ---------------------------------------------------------------- definitions

pub struct ToolDef {
    pub name: &'static str,
    pub description: String,
    pub schema: Value,
    /// `readOnlyHint`. False means the tool writes to the open project.
    pub read_only: bool,
}

/// Editing tools whose arguments and translation come from the copilot.
/// Order is the order a client sees them in `tools/list`.
pub const EDIT_TOOLS: [&str; 17] = copilot::EDIT_TOOLS;

const MM: &str = "All lengths in the arguments and the result are MILLIMETERS.";

/// `export_plan` sheet names, as `SheetKind` spells them.
const SHEETS: [&str; 6] = ["plan", "lighting", "power", "plumbing", "plumbing_isometric", "aircon"];

fn sheet_kind(name: &str) -> Option<SheetKind> {
    Some(match name {
        "plan" => SheetKind::Plan,
        "lighting" => SheetKind::Lighting,
        "power" => SheetKind::Power,
        "plumbing" => SheetKind::Plumbing,
        "plumbing_isometric" => SheetKind::PlumbingIsometric,
        "aircon" => SheetKind::Aircon,
        _ => return None,
    })
}

fn obj(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

/// Input schemas of the copilot tools, by name. The copilot is the one place
/// that defines what `add_door` takes, so this server does not restate it.
fn copilot_schemas() -> BTreeMap<String, Value> {
    copilot::definitions()
        .into_iter()
        .filter_map(|t| {
            let name = t.get("name")?.as_str()?.to_string();
            Some((name, t.get("input_schema")?.clone()))
        })
        .collect()
}

/// MCP wording for a reused copilot tool. Every entry says millimeters,
/// because a model reads one tool description, not the whole set.
fn reused_descriptions() -> BTreeMap<&'static str, String> {
    let d = |s: &str| format!("{s} {MM}");
    [
        // read
        ("get_project_summary", d("Totals of the open project: net floor area and gross area in square metres, total wall length, and counts of rooms, doors, windows and levels. Call this for any question about totals or counts instead of adding numbers up yourself.")),
        ("list_rooms", d("Every room in the open project with its id, name, usage, net floor area in square metres, perimeter and the ids of the walls that bound it. This is how you find a room the user names, and how you find the wall id to put a door or window on.")),
        ("describe_elements", d("Full stored data of specific elements by id, plus derived geometry such as wall length and wall joins. Call this before editing an element whose current dimensions or position you need.")),
        ("list_elements", d("Every element of one kind in the open project, with ids. Use it to find the walls, doors, windows, assets or pipes the user is talking about. Pipe points are x, y in plan and z above the level floor.")),
        ("find_rooms_without_exterior_window", d("Rooms with no window on a wall that faces the outside. Call this when the user asks about natural light, ventilation or dark rooms.")),
        ("list_review_items", d("Current design review items for the open project: narrow doors, rooms without windows, unclosed walls, pipes through columns or door openings, crossing runs, flat drains and condensate, penetrations and aircon core holes, lights without a switch, switches behind doors, aircon units without an outlet, line sets past the unit's limits and unit clearances. Each item has a status: open, or ignored with the designer's note (see set_review_mark); `resolved` lists set-aside findings that no longer appear. These are suggestions to check, never code compliance or permit findings. Items that point at one place carry a location_mm. Call this after a run of edits and tell the user what it says.")),
        ("get_pipe_takeoff", d("Run quantities of the open project from the engine, for every system: cold and hot water, drainage, vent, storm drains, electrical conduit, aircon line sets and condensate. Centerline length in metres per system, material and size, elbow, tee and sleeve counts, and every slab, wall and roof penetration with its position; an aircon run through a wall has its core hole size. Conduit needs no sleeves. Call this for any question about run lengths, fittings or sleeves instead of adding them up yourself. Guhit counts runs; it never sizes them: plumbing is for a registered Master Plumber, electrical for a Professional Electrical Engineer, aircon for a Professional Mechanical Engineer.")),
        ("get_schedule", d("Device and fixture counts of the open project from the engine, per level and room: lights, outlets, switches, the panelboard, detectors, aircon units, sanitary and utility fixtures. Each row has the catalog name and the row of the PH electrical inspection form it counts under; each level has totals by group and by form row. Call this for any question about how many lights, outlets, switches or fixtures there are instead of counting yourself. Circuits, loads and ratings are for the Professional Electrical Engineer.")),
        // edit
        ("add_wall", d("Add one straight wall between two points and commit it as one undo step. Omit thickness_mm to use the project default of 150 mm CHB. level (a level id or name) puts it on another level than the first.")),
        ("add_wall_chain", d("Add connected walls running through a list of points, as one undo step. closed joins the last point back to the first. Omit thickness_mm to use the project default of 150 mm. level (a level id or name) puts them on another level than the first.")),
        ("add_rect_room", d("Add a rectangular room: four walls plus a named room, as one undo step. origin is the SOUTH-WEST corner, width_mm runs east and depth_mm runs north, both measured on wall CENTERLINES. With the default 150 mm walls a 4000 x 3000 room has a net floor of 3850 x 2850. To share a wall with a room you already drew, put this origin exactly on that room's centerline. level (a level id or name) puts it on another level than the first, for example the \"Second Floor\" add_level made.")),
        ("add_door", d("Add a door on an existing wall, as one undo step. Get wall_id from list_rooms or list_elements. Defaults are 900 wide by 2100 high. A door between two rooms goes on the wall they share.")),
        ("add_window", d("Add a window on an existing wall, as one undo step. Get wall_id from list_rooms or list_elements. Defaults are 1200 wide by 1200 high with a 900 sill. Windows belong on exterior walls.")),
        ("resize_room", d("Move the wall or walls on one side of a room outward by delta_mm, as one undo step. A negative delta_mm moves them inward. Connected walls stretch to stay joined. Use this for \"make the bedroom 300 wider to the east\".")),
        ("set_wall_length", d("Set an exact centerline length for one wall, as one undo step. anchor says what stays put: \"start\" keeps the start point, \"end\" keeps the end point, \"center\" keeps the midpoint.")),
        ("move_elements", d("Move elements by a distance, as one undo step. dx_mm is east, dy_mm is north. With stretch_connected true, walls attached to a moved wall stay joined and stretch instead of breaking.")),
        ("rename_room", d("Give a room a new name, as one undo step. The name survives later wall edits.")),
        ("set_room_usage", d("Set the usage type of a room, as one undo step. Usage drives the review checks, so a bathroom is checked differently from a bedroom.")),
        ("set_opening_size", d("Change the size of one door or window, as one undo step. Give only the values that change.")),
        ("delete_elements", d("Delete elements, as one undo step. Deleting a wall also deletes the doors and windows hosted on it.")),
        ("set_material", d("Assign a material to elements, as one undo step. Walls, columns and openings take a surface material, rooms take a floor material. The material ids are in the resource guhit://docs/ph-defaults.")),
        ("set_roof", d("Change the roof of the open project, as one undo step. Give only the values that change. The Philippine default is a gable roof at 25 degrees with a 600 mm overhang in long-span pre-painted metal.")),
        ("add_asset", d("Place one item from the built-in library, as one undo step: furniture, fixtures, lights, outlets, switches, a panelboard, detectors or aircon units. position is the CENTER of its footprint and the item's local +y is its back (bed head, sofa back, water closet tank). Wall items (outlets, switches, wall lights, panelboards, split aircon indoor units) snap to the nearest wall face within 1000 mm of position, back to the wall; give a point just inside the room near that wall. Ceiling items hang from the ceiling. level (a level id or name) puts it on another level than the first. Linking a switch to its lights is done in the app with the link tool (L).")),
        ("add_level", d("Add a level (storey), as one undo step. By default it is named \"Level N\", its floor sits on top of the highest level (that level's elevation plus its floor-to-floor height) and it is 3000 mm floor to floor. Names are 1 to 60 characters, heights 2000 to 10000 mm, and no two levels share a floor elevation. The result lists the new level under levels_added. Draw on it by passing its name or id as level to add_wall, add_wall_chain, add_rect_room or add_asset, in the same batch or later.")),
        ("delete_level", d("Delete a level and everything on it, as one undo step: its walls with their doors and windows, rooms, columns, stairs, objects, notes, dimensions and pipes. Links from other objects to deleted ones are removed. The last level cannot be deleted. level is the level's id or name. Only do this when the user names the level to delete.")),
    ]
    .into_iter()
    .collect()
}

/// The full tool list, in the order a client sees it.
pub fn definitions() -> Vec<ToolDef> {
    let schemas = copilot_schemas();
    let described = reused_descriptions();
    let mut out: Vec<ToolDef> = vec![];

    let reuse = |name: &'static str, read_only: bool, out: &mut Vec<ToolDef>| {
        // Both maps are built from constant tables, so a missing entry is a
        // programming error, not a runtime condition. The unit test below
        // catches it before it ships.
        let (Some(schema), Some(description)) = (schemas.get(name), described.get(name)) else {
            panic!("guhit-mcp: no copilot schema or description for tool `{name}`");
        };
        out.push(ToolDef {
            name,
            description: description.clone(),
            schema: schema.clone(),
            read_only,
        });
    };

    // ------------------------------------------------------------------ hub
    out.push(ToolDef {
        name: "list_projects",
        description: format!("List every Guhit Studio project stored on this machine, newest first: id, name, floor area in square metres, room count and dates. Use an id from here with open_project. {MM}"),
        schema: obj(json!({}), &[]),
        read_only: true,
    });
    out.push(ToolDef {
        name: "open_project",
        description: format!("Open a project by id and make it the document every other tool acts on. The desktop window switches to it. The project that was open is saved first. {MM}"),
        schema: obj(json!({"id": {"type": "string", "description": "Project id from list_projects."}}), &["id"]),
        read_only: false,
    });
    out.push(ToolDef {
        name: "create_project",
        description: format!("Create a project and open it. template \"blank\" starts with nothing drawn, which is what you want before drawing a house. template \"sample-bungalow\" starts from a small two-room house. template \"plumbing-demo\" starts from the bungalow with services: a T&B, columns, water, drainage and vent pipes, storm downspouts, lights, switches, outlets, a panelboard, a smoke detector and a split aircon with its line set and condensate drain, with review items to look at. {MM}"),
        schema: obj(
            json!({
                "name": {"type": "string", "description": "Project name shown in the hub."},
                "template": {"type": "string", "enum": ["blank", "sample-bungalow", "plumbing-demo"], "description": "Defaults to \"blank\"."},
            }),
            &["name"],
        ),
        read_only: false,
    });
    out.push(ToolDef {
        name: "close_project",
        description: format!("Close the open project and send the desktop window back to the project hub. Saves first. {MM}"),
        schema: obj(json!({}), &[]),
        read_only: false,
    });

    // ----------------------------------------------------------------- read
    for name in ["get_project_summary", "list_rooms", "list_elements", "describe_elements", "find_rooms_without_exterior_window", "list_review_items", "get_pipe_takeoff", "get_schedule"] {
        reuse(name, true, &mut out);
    }
    out.push(ToolDef {
        name: "get_plan_image",
        description: format!("The last plan thumbnail the desktop window saved for the open project, as a PNG. It is a picture of the plan as of the last time the window drew it, so it can be older than the current revision. When no window has drawn this project yet there is no thumbnail and this tool says so; use export_plan with format \"svg\" to get a drawing from the engine instead. {MM}"),
        schema: obj(json!({}), &[]),
        read_only: true,
    });
    out.push(ToolDef {
        name: "list_renders",
        description: format!("Saved 3D visuals of the open project, newest first, each tied to the model revision and camera it was captured from. {MM}"),
        schema: obj(json!({}), &[]),
        read_only: true,
    });

    // ----------------------------------------------------------------- edit
    for name in EDIT_TOOLS {
        reuse(name, false, &mut out);
    }
    out.push(ToolDef {
        name: "set_review_mark",
        description: format!("Set a review item aside with a note, or reopen it, as one undo step. The target is one finding (issue_id from list_review_items), a whole check (code, for example light_no_switch), or a check on one element (code plus element_id). A set-aside item stays in list_review_items with status \"ignored\" and the note; it is never approved, and a set-aside finding that stops appearing is listed as resolved. Only set an item aside when the user asks and gives the reason. {MM}"),
        schema: obj(
            json!({
                "action": {"type": "string", "enum": ["set_aside", "reopen"]},
                "issue_id": {"type": "string", "description": "One finding, by its id from list_review_items."},
                "code": {"type": "string", "description": "A whole check, by its code. With element_id, that check on one element."},
                "element_id": {"type": "string", "description": "With code: only the findings of that check that involve this element."},
                "note": {"type": "string", "description": "Why the item is set aside, in the user's words. Needed for set_aside."},
            }),
            &["action"],
        ),
        read_only: false,
    });
    out.push(ToolDef {
        name: "undo",
        description: format!("Undo the last change to the open project, whoever made it: this server, the desktop window or the in-app copilot. One call reverts one step. {MM}"),
        schema: obj(json!({}), &[]),
        read_only: false,
    });
    out.push(ToolDef {
        name: "redo",
        description: format!("Redo the change that was last undone in the open project. {MM}"),
        schema: obj(json!({}), &[]),
        read_only: false,
    });
    out.push(ToolDef {
        name: "save_version",
        description: format!("Save a named version of the open project that the user can restore later from the desktop app. This is a checkpoint, not an export, and it is not an undo step. {MM}"),
        schema: obj(json!({"label": {"type": "string", "description": "What this version is, for example \"Before moving the kitchen\"."}}), &["label"]),
        read_only: false,
    });
    out.push(ToolDef {
        name: "export_plan",
        description: format!("Draw the open project to a file in the app's exports folder and return the absolute path. format \"pdf\" and \"svg\" are sheets with a title block; \"dxf\" is CAD geometry. sheet picks the drawing: the architectural plan (the default), a lighting, power, plumbing or aircon layout, or the plumbing isometric diagrams; the signing engineer's fields stay blank. review_page adds a page of review items and their notes to a PDF. The sheet scale is picked to fit the paper unless you give scale_denominator. {MM}"),
        schema: obj(
            json!({
                "format": {"type": "string", "enum": ["pdf", "svg", "dxf"]},
                "paper": {"type": "string", "enum": ["a4", "a3", "a2", "a1"], "description": "Sheet size. Defaults to the project setting."},
                "orientation": {"type": "string", "enum": ["landscape", "portrait"], "description": "Defaults to landscape."},
                "scale_denominator": {"type": "integer", "description": "Drawing scale 1:N, for example 100. Omit to fit the sheet."},
                "show_dimensions": {"type": "boolean", "description": "Defaults to true."},
                "show_room_labels": {"type": "boolean", "description": "Defaults to true."},
                "show_assets": {"type": "boolean", "description": "Defaults to true."},
                "title_block": {"type": "boolean", "description": "Defaults to true."},
                "show_pipes": {"type": "boolean", "description": "Draw pipes on visible pipe layers, with a legend. Defaults to true."},
                "sheet": {"type": "string", "enum": SHEETS, "description": "Which drawing. Defaults to \"plan\". The service sheets draw their devices and runs with a legend and counts; \"power\" adds a schedule of loads with blank ratings for the Professional Electrical Engineer."},
                "review_page": {"type": "boolean", "description": "Add a page listing the review items, open and set aside, with their notes. PDF only. Defaults to false."},
            }),
            &["format"],
        ),
        read_only: false,
    });

    // ---------------------------------------------------------------- batch
    out.push(ToolDef {
        name: "batch",
        description: format!(
            "Apply several editing steps to the open project atomically, as ONE undo step: either every step lands or nothing does. This is the tool for \"draw the whole house\". Each step is {{\"tool\": <one of the editing tools>, \"args\": <that tool's arguments>}} and the steps run in order, each one seeing the result of the ones before it, so a later step can name an element an earlier step created. Ids are stable: the id an element gets does not change when more steps are appended, and the result lists what every step created under `steps`, so the normal way to build a house is one batch that lays out the rooms, then read the wall ids it returns and a second batch that hangs the doors and windows on them. If any step fails, NOTHING is applied and the error names the step that failed. {MM}"
        ),
        schema: obj(
            json!({
                "label": {"type": "string", "description": "Short undo label, for example \"3-bedroom bungalow\"."},
                "steps": {
                    "type": "array",
                    "description": "At least one step, applied in order.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": {"type": "string", "enum": EDIT_TOOLS},
                            "args": {"type": "object", "description": "Arguments for that tool, exactly as the tool takes them."},
                        },
                        "required": ["tool", "args"],
                        "additionalProperties": false,
                    },
                },
            }),
            &["steps"],
        ),
        read_only: false,
    });

    out
}

// ------------------------------------------------------------------- dispatch

fn args_object(args: &Value) -> Result<&Map<String, Value>, ToolFail> {
    args.as_object()
        .ok_or_else(|| ToolFail("invalid arguments: expected a JSON object".into()))
}

fn opt_str(args: &Value, name: &str) -> Result<Option<String>, ToolFail> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(ToolFail(format!("invalid arguments: `{name}` must be a string"))),
    }
}

fn req_str(args: &Value, name: &str) -> Result<String, ToolFail> {
    opt_str(args, name)?
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ToolFail(format!("invalid arguments: `{name}` is required")))
}

fn opt_bool(args: &Value, name: &str) -> Result<Option<bool>, ToolFail> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(ToolFail(format!("invalid arguments: `{name}` must be true or false"))),
    }
}

/// Run one tool. `args` is whatever the client sent; every tool validates it.
pub async fn call(app: &AppService, name: &str, args: Value) -> Result<Output, ToolFail> {
    args_object(&args)?;
    match name {
        // ------------------------------------------------------------- hub
        "list_projects" => ok(app.handle("hub_list", json!({})).await?),
        "open_project" => {
            let id = req_str(&args, "id")?;
            let state: Value = app.handle("hub_open", json!({ "id": id })).await?;
            ok(opened(&state))
        }
        "create_project" => {
            let name = req_str(&args, "name")?;
            let template = opt_str(&args, "template")?.unwrap_or_else(|| "blank".into());
            let state: Value = app
                .handle("hub_create", json!({ "name": name, "template": template }))
                .await?;
            ok(opened(&state))
        }
        "close_project" => {
            app.handle("hub_close", json!({})).await?;
            ok(json!({"ok": true, "open": false, "note": "the desktop window is back on the project hub"}))
        }

        // ------------------------------------------------------------ read
        n if copilot::is_read(n) => {
            let query = copilot::to_query(n, &args)?;
            ok(app.handle("doc_query", json!({ "query": query })).await?)
        }
        "list_renders" => ok(app.handle("render_list", json!({})).await?),
        "get_plan_image" => {
            let dir = app
                .project_dir()
                .await
                .ok_or_else(|| ToolFail("no_document: no project is open".into()))?;
            match store::thumbnail_data_url(&dir) {
                Some(url) => {
                    let base64 = url.rsplit_once("base64,").map(|(_, b)| b.to_string()).ok_or_else(|| {
                        ToolFail("io: the stored thumbnail is not a PNG data URL".into())
                    })?;
                    Ok(Output::Image { base64, mime: "image/png".into() })
                }
                None => Err(ToolFail(
                    "not_found: this project has no saved plan thumbnail yet. A thumbnail is written by the desktop window when it draws the plan, so a project created through this server has none until the window opens it. Use export_plan with format \"svg\" to get a drawing from the engine instead."
                        .into(),
                )),
            }
        }

        // ------------------------------------------------------------ edit
        n if copilot::is_edit(n) => {
            let label = format!("MCP: {}", n.replace('_', " "));
            edit(app, &label, &[(n.to_string(), args)]).await
        }
        "batch" => {
            let steps = args
                .get("steps")
                .and_then(Value::as_array)
                .ok_or_else(|| ToolFail("invalid arguments: `steps` must be an array".into()))?;
            if steps.is_empty() {
                return Err(ToolFail("invalid arguments: `steps` is empty, nothing would change".into()));
            }
            let mut parsed: Vec<(String, Value)> = vec![];
            for (i, step) in steps.iter().enumerate() {
                let tool = step
                    .get("tool")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ToolFail(format!("invalid arguments: steps[{i}] has no `tool`")))?;
                if !copilot::is_edit(tool) {
                    return Err(ToolFail(format!(
                        "invalid arguments: steps[{i}].tool is `{tool}`, which is not an editing tool. Editing tools: {}",
                        EDIT_TOOLS.join(", ")
                    )));
                }
                let step_args = step.get("args").cloned().unwrap_or_else(|| json!({}));
                args_object(&step_args).map_err(|_| {
                    ToolFail(format!("invalid arguments: steps[{i}].args must be a JSON object"))
                })?;
                parsed.push((tool.to_string(), step_args));
            }
            let label = match opt_str(&args, "label")? {
                Some(l) if !l.trim().is_empty() => format!("MCP: {}", l.trim()),
                _ => format!("MCP: batch of {} steps", parsed.len()),
            };
            edit(app, &label, &parsed).await
        }

        "set_review_mark" => set_review_mark(app, &args).await,

        // --------------------------------------------------- history, files
        "undo" => {
            let state: Value = app.handle("doc_undo", json!({})).await?;
            ok(history(&state, "undone"))
        }
        "redo" => {
            let state: Value = app.handle("doc_redo", json!({})).await?;
            ok(history(&state, "redone"))
        }
        "save_version" => {
            let label = req_str(&args, "label")?;
            ok(app.handle("snapshot_create", json!({ "label": label })).await?)
        }
        "export_plan" => {
            let format = req_str(&args, "format")?;
            let settings_paper = {
                let s = app.session.lock().await;
                s.doc
                    .as_ref()
                    .map(|d| d.project().settings.paper)
                    .ok_or_else(|| IpcError::new("no_document", "no project is open"))?
            };
            let paper = match opt_str(&args, "paper")?.as_deref() {
                None => settings_paper,
                Some("a4") => PaperSize::A4,
                Some("a3") => PaperSize::A3,
                Some("a2") => PaperSize::A2,
                Some("a1") => PaperSize::A1,
                Some(other) => {
                    return Err(ToolFail(format!(
                        "invalid arguments: unknown paper `{other}`. Use a4, a3, a2 or a1"
                    )))
                }
            };
            let orientation = match opt_str(&args, "orientation")?.as_deref() {
                None | Some("landscape") => Orientation::Landscape,
                Some("portrait") => Orientation::Portrait,
                Some(other) => {
                    return Err(ToolFail(format!(
                        "invalid arguments: unknown orientation `{other}`. Use landscape or portrait"
                    )))
                }
            };
            let sheet = match opt_str(&args, "sheet")?.as_deref() {
                None => SheetKind::Plan,
                Some(name) => sheet_kind(name).ok_or_else(|| {
                    ToolFail(format!(
                        "invalid arguments: unknown sheet `{name}`. Use {}",
                        SHEETS.join(", ")
                    ))
                })?,
            };
            let review_page = opt_bool(&args, "review_page")?.unwrap_or(false);
            if review_page && format != "pdf" {
                return Err(ToolFail(
                    "invalid arguments: review_page adds a page to a PDF. Use format \"pdf\", or leave review_page out"
                        .into(),
                ));
            }
            let scale_denominator = match args.get("scale_denominator") {
                None | Some(Value::Null) => None,
                Some(v) => Some(v.as_u64().filter(|n| *n > 0 && *n <= u32::MAX as u64).ok_or_else(|| {
                    ToolFail("invalid arguments: scale_denominator must be a positive whole number".into())
                })? as u32),
            };
            let options = PlanExportOptions {
                level_id: None,
                paper,
                orientation,
                scale_denominator,
                show_dimensions: opt_bool(&args, "show_dimensions")?.unwrap_or(true),
                show_room_labels: opt_bool(&args, "show_room_labels")?.unwrap_or(true),
                show_assets: opt_bool(&args, "show_assets")?.unwrap_or(true),
                title_block: opt_bool(&args, "title_block")?.unwrap_or(true),
                show_pipes: opt_bool(&args, "show_pipes")?.unwrap_or(true),
                sheet,
                review_page,
            };
            ok(app
                .handle(
                    "export_plan",
                    json!({ "format": format, "options": options, "path": Value::Null }),
                )
                .await?)
        }

        other => Err(ToolFail(format!("unknown tool `{other}`"))),
    }
}

/// `set_review_mark`: one `Command::SetReviewMark`, committed as one undo
/// step like every edit here. The engine checks the target.
async fn set_review_mark(app: &AppService, args: &Value) -> Result<Output, ToolFail> {
    let action = req_str(args, "action")?;
    let issue_id = opt_str(args, "issue_id")?.filter(|s| !s.trim().is_empty());
    let code = opt_str(args, "code")?.filter(|s| !s.trim().is_empty());
    let element_id = opt_str(args, "element_id")?.filter(|s| !s.trim().is_empty());
    let note = opt_str(args, "note")?.map(|n| n.trim().to_string());
    let target = match (issue_id, code, element_id) {
        (Some(id), None, None) => ReviewTarget::Issue { id },
        (None, Some(code), None) => ReviewTarget::Check { code },
        (None, Some(code), Some(element_id)) => ReviewTarget::Element { code, element_id },
        (Some(_), _, _) => {
            return Err(ToolFail(
                "invalid arguments: give either issue_id, or code with an optional element_id, not both".into(),
            ))
        }
        (None, None, _) => {
            return Err(ToolFail(
                "invalid arguments: say what to set aside: issue_id, or code with an optional element_id".into(),
            ))
        }
    };
    let note = match action.as_str() {
        "set_aside" => match note {
            Some(n) if !n.is_empty() => Some(n),
            _ => {
                return Err(ToolFail(
                    "invalid arguments: set_aside needs a note saying why, in the user's words".into(),
                ))
            }
        },
        "reopen" => None,
        other => {
            return Err(ToolFail(format!(
                "invalid arguments: unknown action `{other}`. Use set_aside or reopen"
            )))
        }
    };
    let label = match note {
        Some(_) => "MCP: set a review item aside",
        None => "MCP: reopen a review item",
    };
    let command = Command::Batch {
        label: label.to_string(),
        commands: vec![Command::SetReviewMark {
            target: target.clone(),
            note,
        }],
    };
    let result = app.commit(command, Origin::Ai).await?;
    let matches = |i: &Issue| match &target {
        ReviewTarget::Issue { id } => i.id == id.trim(),
        ReviewTarget::Check { code } => i.code == code.trim(),
        ReviewTarget::Element { code, element_id } => {
            i.code == code.trim() && i.element_ids.iter().any(|e| e == element_id.trim())
        }
    };
    let items: Vec<Value> = result
        .state
        .derived
        .issues
        .iter()
        .filter(|i| matches(i))
        .map(|i| json!({"id": i.id, "code": i.code, "status": i.status, "note": i.note, "message": i.message}))
        .collect();
    ok(json!({
        "ok": true,
        "revision": result.state.revision,
        "undo_label": result.state.undo_label,
        "items": items,
        "resolved": result.state.derived.review_resolved,
        "note": "Set aside is not approval. The item stays listed with its note.",
    }))
}

/// Compact answer after opening or creating a project. The full state is
/// large and the model does not need it; `list_rooms` and
/// `get_project_summary` give the details.
fn opened(state: &Value) -> Value {
    let project = &state["project"];
    json!({
        "ok": true,
        "project_id": project["id"],
        "name": project["name"],
        "revision": state["revision"],
        "levels": project["levels"].as_array().map(|l| l.len()).unwrap_or(0),
        "totals": state["derived"]["totals"],
        "note": "this project is now the document every other tool acts on",
    })
}

fn history(state: &Value, what: &str) -> Value {
    json!({
        "ok": true,
        "revision": state["revision"],
        "what": what,
        "can_undo": state["can_undo"],
        "can_redo": state["can_redo"],
        "next_undo": state["undo_label"],
        "next_redo": state["redo_label"],
    })
}

// ----------------------------------------------------------------- committing

/// Translate the steps, validate the whole thing with `Document::preview`,
/// then commit it as one `Command::Batch`.
///
/// Translation runs against the staged view: the open project with the earlier
/// steps of this call already applied, so step 2 can name an element step 1
/// made. The core seeds new ids per leaf command, so an id from step 1 does
/// not move when step 2 is appended.
///
/// The commit is guarded by the revision the translation was done on, so if
/// the desktop window or the copilot changes the plan in between, nothing is
/// applied and the caller is told to read the plan again.
async fn edit(app: &AppService, label: &str, steps: &[(String, Value)]) -> Result<Output, ToolFail> {
    let (before, commands, per_step, revision) = {
        let s = app.session.lock().await;
        let doc = s
            .doc
            .as_ref()
            .ok_or_else(|| IpcError::new("no_document", "no project is open. Call create_project or open_project first"))?;
        let level_id = first_level(doc.project())?;
        let before = doc.project().clone();

        let mut commands: Vec<Command> = vec![];
        let mut staged: Option<Project> = None;
        let mut per_step: Vec<Value> = vec![];
        for (i, (name, args)) in steps.iter().enumerate() {
            let view = staged.clone().unwrap_or_else(|| before.clone());
            let next = copilot::to_commands(&view, name, args, &level_id)
                .map_err(|e| step_fail(e.into(), i, name, steps.len()))?;
            commands.extend(next);
            // Validate everything staged so far. A failure here is the engine
            // refusing the whole batch, which is what a commit would do.
            let preview = doc
                .preview(&Command::Batch { label: label.to_string(), commands: commands.clone() })
                .map_err(|e| {
                    let ipc: IpcError = e.into();
                    step_fail(ipc.into(), i, name, steps.len())
                })?;
            let diff = copilot::step_diff(&view, &preview.state.project);
            if steps.len() > 1 {
                let mut one = json!({
                    "step": i + 1,
                    "tool": name,
                    "created": diff["added"],
                    "changed": diff["modified"],
                    "deleted": diff["removed"],
                });
                for key in ["levels_added", "levels_removed"] {
                    if let Some(v) = diff.get(key) {
                        one[key] = v.clone();
                    }
                }
                per_step.push(one);
            }
            staged = Some(preview.state.project);
        }
        (before, commands, per_step, doc.revision())
    };

    let result = app
        .commit_if_revision(Command::Batch { label: label.to_string(), commands }, Origin::Ai, revision)
        .await?;
    let mut out = edit_result(&before, &result);
    if !per_step.is_empty() {
        out["steps"] = Value::Array(per_step);
    }
    ok(out)
}

fn step_fail(f: ToolFail, index: usize, tool: &str, total: usize) -> ToolFail {
    if total == 1 {
        f
    } else {
        ToolFail(format!(
            "step {} of {total} (`{tool}`): {} Nothing was applied.",
            index + 1,
            f.0
        ))
    }
}

/// New elements go on the first level unless a call names another with
/// `level`. A bungalow has exactly one.
fn first_level(project: &Project) -> Result<Id, IpcError> {
    project
        .levels
        .first()
        .map(|l| l.id.clone())
        .ok_or_else(|| IpcError::new("invalid", "the project has no level"))
}

/// What one editing call did: the engine's own diff summary, plus the ids and
/// readable labels of everything created, so the next call can refer to them.
fn edit_result(before: &Project, result: &ApplyResult) -> Value {
    let step = copilot::step_diff(before, &result.state.project);
    let mut out = json!({
        "ok": true,
        "revision": result.state.revision,
        "summary": result.diff.summary,
        "undo_label": result.state.undo_label,
        "created": step["added"],
        "changed": step["modified"],
        "deleted": step["removed"],
        "totals": result.state.derived.totals,
    });
    for key in ["levels_added", "levels_removed"] {
        if let Some(v) = step.get(key) {
            out[key] = v.clone();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_unique_name_and_an_object_schema() {
        let defs = definitions();
        let mut names: Vec<&str> = defs.iter().map(|d| d.name).collect();
        names.sort_unstable();
        let unique = names.len();
        names.dedup();
        assert_eq!(unique, names.len(), "duplicate tool name");
        for d in &defs {
            assert_eq!(d.schema["type"], "object", "{} has no object schema", d.name);
            assert!(d.schema.get("properties").is_some(), "{} has no properties", d.name);
            assert!(
                d.description.contains("MILLIMETERS"),
                "{} does not say what unit it works in",
                d.name
            );
        }
    }

    #[test]
    fn the_whole_advertised_set_is_present() {
        let defs = definitions();
        let names: Vec<&str> = defs.iter().map(|d| d.name).collect();
        for expected in [
            "list_projects", "open_project", "create_project", "close_project",
            "get_project_summary", "list_rooms", "list_elements", "describe_elements",
            "find_rooms_without_exterior_window", "list_review_items", "get_pipe_takeoff", "get_schedule",
            "get_plan_image", "list_renders",
            "add_wall", "add_wall_chain", "add_rect_room", "add_door", "add_window", "resize_room",
            "set_wall_length", "move_elements", "rename_room", "set_room_usage", "set_opening_size",
            "delete_elements", "set_material", "set_roof", "add_asset", "add_level", "delete_level",
            "set_review_mark", "undo", "redo", "save_version", "export_plan", "batch",
        ] {
            assert!(names.contains(&expected), "tool `{expected}` is missing");
        }
        assert_eq!(names.len(), 37, "the tool list changed: update docs/MCP.md");
    }

    #[test]
    fn the_engines_internal_step_prefix_is_removed() {
        assert_eq!(
            strip_step_prefix("Step 1 of 1: The door does not fit."),
            "The door does not fit."
        );
        assert_eq!(
            strip_step_prefix("Step 12 of 40: Walls overlap."),
            "Walls overlap."
        );
        // Not the engine's prefix: leave it alone.
        assert_eq!(strip_step_prefix("Stepping stone: no"), "Stepping stone: no");
        assert_eq!(strip_step_prefix("Step one of two: no"), "Step one of two: no");
        assert_eq!(strip_step_prefix("The door does not fit."), "The door does not fit.");
    }

    #[test]
    fn read_and_edit_tools_are_marked_correctly() {
        for d in definitions() {
            let expected = matches!(
                d.name,
                "list_projects"
                    | "get_project_summary"
                    | "list_rooms"
                    | "list_elements"
                    | "describe_elements"
                    | "find_rooms_without_exterior_window"
                    | "list_review_items"
                    | "get_pipe_takeoff"
                    | "get_schedule"
                    | "get_plan_image"
                    | "list_renders"
            );
            assert_eq!(d.read_only, expected, "readOnlyHint is wrong for {}", d.name);
        }
    }
}

#[cfg(test)]
mod sheet_tests {
    use super::*;

    #[test]
    fn every_sheet_name_is_a_sheet_kind_spelled_the_same_way() {
        for name in SHEETS {
            let kind = sheet_kind(name).unwrap_or_else(|| panic!("`{name}` is not a sheet"));
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(name));
        }
        assert_eq!(sheet_kind("elevation"), None);
        let export = definitions().into_iter().find(|d| d.name == "export_plan").unwrap();
        assert_eq!(export.schema["properties"]["sheet"]["enum"], json!(SHEETS));
        assert_eq!(export.schema["properties"]["review_page"]["type"], "boolean");
    }
}
