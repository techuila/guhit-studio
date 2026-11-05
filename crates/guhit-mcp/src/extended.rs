//! The opt-in toolsets (docs/MCP.md "Toolsets"): services, visuals, live,
//! review and interop. Their edits run through `stage` like every edit.

use std::io::Cursor;

use base64::Engine as _;
use guhit_app::AppService;
use guhit_model::*;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::agent::CallContext;
use crate::fail::ToolFail;
use crate::stage::{self, Options, StepIn};
use crate::tools::{obj, scope_schema, Output, ToolDef, MM};
use crate::translate::{self, parse};

/// Largest underlay image read.
const MAX_UNDERLAY_BYTES: usize = 25 * 1024 * 1024;

fn def(name: &'static str, description: String, schema: Value, read_only: bool, open_world: bool) -> ToolDef {
    ToolDef { name, description, schema, read_only, open_world }
}

fn edit_schema(properties: Value, required: &[&str]) -> Value {
    let mut s = obj(properties, required);
    s["properties"]["scope"] = scope_schema();
    s
}

pub fn definitions() -> Vec<ToolDef> {
    let p3 = json!({"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}, "z": {"type": "number"}}, "required": ["x", "y", "z"]});
    let session = crate::session::definitions();
    let visuals = crate::visuals::definitions();
    let take = |list: &[ToolDef], name: &str| {
        let d = list.iter().find(|d| d.name == name).unwrap_or_else(|| panic!("guhit-mcp: no definition for `{name}`"));
        ToolDef { name: d.name, description: d.description.clone(), schema: d.schema.clone(), read_only: d.read_only, open_world: d.open_world }
    };
    vec![
        // ------------------------------------------------------- services
        def(
            "edit_pipe",
            format!("Edit a pipe run in 3D, one undo step. op move_points (pipes [{{pipe_id, indices}}], dx_mm, dy_mm, dz_mm; empty indices move the whole run), segment_length (pipe_id, segment, length_mm, hold start|end), slope (pipe_id, from_segment, to_segment, slope_pct, hold), insert_point (pipe_id, segment, at {{x, y, z}}), remove_point (pipe_id, index). Segment n runs from point n to n + 1 of points_mm. Joined runs follow unless keep_joins is false. Geometry only: Guhit never sizes a pipe. {MM}"),
            edit_schema(json!({
                "op": {"type": "string", "enum": ["move_points", "segment_length", "slope", "insert_point", "remove_point"]},
                "pipe_id": {"type": "string"}, "pipes": {"type": "array", "items": {"type": "object"}},
                "dx_mm": {"type": "number"}, "dy_mm": {"type": "number"}, "dz_mm": {"type": "number"},
                "segment": {"type": "integer"}, "from_segment": {"type": "integer"}, "to_segment": {"type": "integer"},
                "length_mm": {"type": "number"}, "slope_pct": {"type": "number"}, "hold": {"type": "string", "enum": ["start", "end"]},
                "at": p3.clone(), "index": {"type": "integer"}, "keep_joins": {"type": "boolean"},
            }), &["op"]),
            false,
            false,
        ),
        def(
            "add_pipe",
            format!("Draw a service run for coordination, one undo step: system cold_water, hot_water, drainage, vent, storm, conduit, refrigerant or condensate, through points {{x, y, z}} (z above the level floor, negative below the slab). Material and size default per system. Guhit coordinates runs and never sizes them: plumbing is for a registered Master Plumber, electrical for a PEE, aircon for a PME. {MM}"),
            edit_schema(json!({
                "system": {"type": "string", "enum": ["cold_water", "hot_water", "drainage", "vent", "storm", "conduit", "refrigerant", "condensate"]},
                "points": {"type": "array", "items": p3}, "material": {"type": "string"}, "diameter_mm": {"type": "number"},
                "name": {"type": "string"}, "level": {"type": "string"},
            }), &["system", "points"]),
            false,
            false,
        ),
        def(
            "link_devices",
            format!("Link a switch to the lights it controls, or an outlet to the unit it feeds (action link, the default), or undo that (unlink), one undo step. Guhit draws links; it never checks circuits. {MM}"),
            edit_schema(json!({"from": {"type": "string"}, "to": {"type": "array", "items": {"type": "string"}}, "action": {"type": "string", "enum": ["link", "unlink"]}}), &["from", "to"]),
            false,
            false,
        ),
        // -------------------------------------------------------- visuals
        def(
            "render",
            format!("Path traced renders in the Guhit Studio window, saved to Visuals. action start: views (saved views by id or name, each from its own camera; none for the view on screen), quality quick (about a minute) or final, size hd|qhd|4k|square, wait_seconds; it answers with the images or a job_id. A window still rendering answers window_busy with what it is doing and when to try again. status: job_id and wait_seconds, or no job_id for what the window's render queue is doing. image: a saved visual by id, fitted to 1568 px. list: saved visuals, newest first. {MM}"),
            obj(json!({
                "action": {"type": "string", "enum": ["start", "status", "image", "list"]},
                "views": {"type": "array", "items": {"type": "string"}},
                "quality": {"type": "string", "enum": ["quick", "final"]},
                "size": {"type": "string", "enum": ["hd", "qhd", "4k", "square"]},
                "wait_seconds": {"type": "number"}, "job_id": {"type": "string"}, "id": {"type": "string"},
                "visibility": {"type": "object", "description": "For start: roof (default true) and cutaway at the active level (default false). Left out, the window draws the model as drawn.", "properties": {"roof": {"type": "boolean"}, "cutaway": {"type": "boolean"}}, "additionalProperties": false},
            }), &["action"]),
            false,
            false,
        ),
        take(&visuals, "visualize_render"),
        def(
            "set_look",
            format!("Make a saved visual the project look: AI renders follow its style (D53). id: from render action \"list\"; null clears it. It never changes the model. {MM}"),
            obj(json!({"id": {"type": ["string", "null"], "description": "A saved visual's id, or null."}}), &["id"]),
            false,
            false,
        ),
        // ----------------------------------------------------------- live
        take(&session, "get_session"),
        take(&session, "send_chat_message"),
        def(
            "set_status",
            format!("Your status line, by your cursor and in the Agents panel: \"Placing doors on the upper floor\". Empty text clears it. With other agents on the project, claim your level first: claim {{levels: [\"Upper floor\"]}} (or ids) keeps other agents out of it until 10 minutes after your last call, and their edits there are refused `claimed`; people are never blocked. release true gives it back. {MM}"),
            obj(json!({
                "text": {"type": "string", "description": "At most 80 characters."},
                "claim": {"type": "object", "description": "Levels (ids or names) and elements to keep other agents out of. Both empty releases.", "properties": {"levels": {"type": "array", "items": {"type": "string"}}, "ids": {"type": "array", "items": {"type": "string"}}}, "additionalProperties": false},
                "release": {"type": "boolean", "description": "Give back your claim."},
            }), &["text"]),
            false,
            true,
        ),
        // --------------------------------------------------------- review
        def(
            "set_review_mark",
            format!("Set a review item aside with a note, or reopen it, as one undo step. The target is one finding (issue_id from check), a whole check (code), or a check on one element (code and element_id). A set-aside item stays listed with status ignored and the note; it is never approved. Only when the user asks and gives the reason. {MM}"),
            edit_schema(json!({
                "action": {"type": "string", "enum": ["set_aside", "reopen"]},
                "issue_id": {"type": "string"}, "code": {"type": "string"}, "element_id": {"type": "string"},
                "note": {"type": "string", "description": "Why, in the user's words. Needed for set_aside."},
            }), &["action"]),
            false,
            false,
        ),
        // -------------------------------------------------------- interop
        def(
            "import_underlay",
            format!("Put the user's own plan image (PNG, JPEG, WebP) under a level to trace: path (absolute) or data (a data URL). Scale: mm_per_px, or scale {{a_px, b_px, distance_mm}} from two pixel points a known distance apart. position: plan point of the image's bottom-left corner (default 0, 0). Then draw over it in plan mm and compare with look overlay underlay. The image never changes the model by itself. {MM}"),
            edit_schema(json!({
                "path": {"type": "string"}, "data": {"type": "string"}, "file_name": {"type": "string"},
                "mm_per_px": {"type": "number"},
                "scale": {"type": "object", "properties": {"a_px": {"type": "object"}, "b_px": {"type": "object"}, "distance_mm": {"type": "number"}}},
                "position": {"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}}},
                "rotation_deg": {"type": "number"}, "opacity": {"type": "number"}, "level": {"type": "string"},
            }), &[]),
            false,
            false,
        ),
        def(
            "import_dxf",
            format!("Bring in a DXF (or DWG with the converter set up). action inspect: its layers, extent and the unit it declares, nothing changes. commit: as walls (parallel line pairs become walls, the rest linework) or as linework to trace, with mm_per_unit (ask the user when the file declares none), layers, offset {{x, y}} and level. path (absolute) or data with file_name. {MM}"),
            obj(json!({
                "action": {"type": "string", "enum": ["inspect", "commit"]},
                "path": {"type": "string"}, "data": {"type": "string"}, "file_name": {"type": "string"},
                "mode": {"type": "string", "enum": ["walls", "linework"]}, "mm_per_unit": {"type": "number"},
                "layers": {"type": "array", "items": {"type": "string"}},
                "offset": {"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}}},
                "level": {"type": "string"},
            }), &["action"]),
            false,
            false,
        ),
    ]
}

pub async fn call(app: &AppService, name: &str, args: &Value, ctx: &CallContext) -> Option<Result<Output, ToolFail>> {
    Some(match name {
        "edit_pipe" | "add_pipe" | "link_devices" => {
            let mut args = args.clone();
            let scope = match stage::take_scope(&mut args) {
                Ok(s) => s,
                Err(e) => return Some(Err(e)),
            };
            let label = format!("MCP: {}", name.replace('_', " "));
            stage::run(app, vec![StepIn::new(name, args)], scope.as_ref(), Options { label, dry_run: false, per_step: false }, ctx).await
        }
        "render" => render(app, args, ctx).await,
        "set_look" => crate::visuals::set_look(app, args).await,
        "export_tour" => crate::tours::export_tour(app, args).await,
        "set_status" => crate::agent::set_status(app, ctx, args).await,
        "import_underlay" => import_underlay(app, args, ctx).await,
        "import_dxf" => import_dxf(app, args, ctx).await,
        _ => return None,
    })
}

async fn render(app: &AppService, args: &Value, ctx: &CallContext) -> Result<Output, ToolFail> {
    let mut rest = args.as_object().cloned().unwrap_or_default();
    let action = rest.remove("action").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    // Status with no job: what the window's render queue is doing, whoever
    // started it (G4 problem 8).
    if action == "status" && rest.get("job_id").is_none_or(Value::is_null) {
        let activity = app.window_activity();
        return Ok(Output::Json(json!({
            "busy": activity.as_ref().is_some_and(|a| a["busy"] == true),
            "window": activity,
            "note": if activity.is_none() { "The window has not reported a render since it opened: nothing is rendering, or the window is closed." } else { "start waits for a render that is running; or pass job_id to wait for one of yours." },
        })));
    }
    let v1 = match action.as_str() {
        "start" => "render_view",
        "status" => "get_render_job",
        "image" => "get_render_image",
        "list" => "list_renders",
        other => return Err(ToolFail::args(format!("unknown action `{other}`. Use start, status, image or list"))),
    };
    crate::tools::dispatch(app, v1, &Value::Object(rest), ctx).await
}

// ----------------------------------------------------------------- interop

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PxIn {
    x: f64,
    y: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaleIn {
    a_px: PxIn,
    b_px: PxIn,
    distance_mm: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UnderlayIn {
    path: Option<String>,
    data: Option<String>,
    file_name: Option<String>,
    mm_per_px: Option<f64>,
    scale: Option<ScaleIn>,
    position: Option<Value>,
    rotation_deg: Option<f64>,
    opacity: Option<f64>,
    level: Option<String>,
}

/// Read a file the caller names. The dev bridge reads only inside its data
/// folder, as it writes only there (any local program can post to it).
async fn read_file(app: &AppService, path: &str, max: usize) -> Result<(String, Vec<u8>), ToolFail> {
    let target = std::path::PathBuf::from(path.trim());
    if !target.is_absolute() || target.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(ToolFail::args("path must be an absolute file path"));
    }
    if !app.reads_external_paths() {
        let data_dir = app.session.lock().await.data_dir.clone();
        if !target.starts_with(&data_dir) {
            return Err(ToolFail::new("forbidden", format!("this service only reads inside {}; pass data instead", data_dir.display())));
        }
    }
    let meta = std::fs::metadata(&target).map_err(|e| ToolFail::new("io", format!("cannot read {}: {e}", target.display())))?;
    if meta.len() as usize > max {
        return Err(ToolFail::args(format!("the file is larger than {} MB", max / (1024 * 1024))));
    }
    let bytes = std::fs::read(&target).map_err(|e| ToolFail::new("io", format!("cannot read {}: {e}", target.display())))?;
    let name = target.file_name().and_then(|n| n.to_str()).unwrap_or("image").to_string();
    Ok((name, bytes))
}

/// The bytes of a data URL or of plain base64.
fn decode_data(data: &str) -> Result<Vec<u8>, ToolFail> {
    let body = match data.split_once(";base64,") {
        Some((_, b)) => b,
        None => data,
    };
    base64::engine::general_purpose::STANDARD
        .decode(body.trim().as_bytes())
        .map_err(|e| ToolFail::args(format!("data is not base64: {e}")))
}

async fn import_underlay(app: &AppService, args: &Value, ctx: &CallContext) -> Result<Output, ToolFail> {
    let mut args = args.clone();
    let scope = stage::take_scope(&mut args)?;
    let i: UnderlayIn = parse(&args)?;
    let (name, bytes) = match (&i.path, &i.data) {
        (Some(p), None) => read_file(app, p, MAX_UNDERLAY_BYTES).await?,
        (None, Some(d)) => {
            let bytes = decode_data(d)?;
            if bytes.len() > MAX_UNDERLAY_BYTES {
                return Err(ToolFail::args("the image is larger than 25 MB"));
            }
            (i.file_name.clone().unwrap_or_else(|| "underlay.png".into()), bytes)
        }
        _ => return Err(ToolFail::args("give path or data")),
    };
    let format = image::guess_format(&bytes).map_err(|_| ToolFail::args("the file is not a PNG, JPEG or WebP image"))?;
    let mime = match format {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::WebP => "image/webp",
        _ => return Err(ToolFail::args("the file is not a PNG, JPEG or WebP image")),
    };
    let (w, h) = image::ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok())
        .ok_or_else(|| ToolFail::args("the image size cannot be read"))?;
    let mm_per_px = match (i.mm_per_px, &i.scale) {
        (Some(m), None) if m.is_finite() && m > 0.0 => m,
        (None, Some(s)) => {
            let px = ((s.b_px.x - s.a_px.x).powi(2) + (s.b_px.y - s.a_px.y).powi(2)).sqrt();
            if px < 1.0 || !s.distance_mm.is_finite() || s.distance_mm <= 0.0 {
                return Err(ToolFail::args("scale needs two different pixel points and a positive distance_mm"));
            }
            s.distance_mm / px
        }
        _ => return Err(ToolFail::args("give mm_per_px, or scale {a_px, b_px, distance_mm}: never guess the scale").with_hint("Ask the user for one known dimension on the drawing.")),
    };
    let position = match &i.position {
        Some(v) => translate::point(v, "position")?,
        None => Point { x: 0.0, y: 0.0 },
    };
    let level_id = {
        let s = app.session.lock().await;
        let doc = s.doc.as_ref().ok_or_else(stage::no_document)?;
        translate::pick_level(doc.project(), i.level.as_deref(), &stage::first_level(doc.project())?)?
    };
    let url = format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes));
    let stored: Value = app.handle("underlay_store", json!({"file_name": name, "data": url})).await?;
    let file_name = stored["file_name"].as_str().unwrap_or(&name).to_string();
    let command = Command::AddElement {
        element: Element::Underlay(Underlay {
            id: String::new(),
            level_id,
            file_name,
            position,
            width_px: w,
            height_px: h,
            mm_per_px,
            scale_confirmed: false,
            rotation_deg: i.rotation_deg.unwrap_or(0.0),
            opacity: i.opacity.unwrap_or(0.5).clamp(0.05, 1.0),
            locked: true,
        }),
    };
    let step = StepIn::new("__command", serde_json::to_value(&command).map_err(|e| ToolFail::new("io", e.to_string()))?);
    let mut out = stage::run(app, vec![step], scope.as_ref(), Options { label: "MCP: import underlay".into(), dry_run: false, per_step: false }, ctx).await?;
    if let Output::Json(v) = &mut out {
        v["underlay"] = json!({
            "width_mm": (w as f64 * mm_per_px).round(),
            "height_mm": (h as f64 * mm_per_px).round(),
            "mm_per_px": mm_per_px,
            "bottom_left": position,
            "note": "Plan point of pixel (px, py), y counted from the top: x = position.x + px * mm_per_px, y = position.y + (height_px - py) * mm_per_px. The user confirms the scale in the app.",
        });
    }
    Ok(out)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DxfIn {
    action: String,
    path: Option<String>,
    data: Option<String>,
    file_name: Option<String>,
    mode: Option<ImportMode>,
    mm_per_unit: Option<f64>,
    layers: Option<Vec<String>>,
    offset: Option<Value>,
    level: Option<String>,
}

async fn import_dxf(app: &AppService, args: &Value, ctx: &CallContext) -> Result<Output, ToolFail> {
    let i: DxfIn = parse(args)?;
    let input = match (&i.path, &i.data) {
        (Some(p), None) => json!({"path": p}),
        (None, Some(d)) => {
            let name = i.file_name.clone().ok_or_else(|| ToolFail::args("data needs file_name, for example plan.dxf"))?;
            let url = if d.starts_with("data:") { d.clone() } else { format!("data:application/octet-stream;base64,{d}") };
            json!({"file_name": name, "data": url})
        }
        _ => return Err(ToolFail::args("give path or data")),
    };
    match i.action.as_str() {
        "inspect" => Ok(Output::Json(app.handle("import_inspect", input).await?)),
        "commit" => {
            let mm_per_unit = i.mm_per_unit.filter(|m| m.is_finite() && *m > 0.0).ok_or_else(|| {
                ToolFail::args("commit needs mm_per_unit: 1 for a drawing in mm, 1000 in m. inspect suggests one; confirm it with the user")
            })?;
            let level_id = {
                let s = app.session.lock().await;
                let doc = s.doc.as_ref().ok_or_else(stage::no_document)?;
                translate::pick_level(doc.project(), i.level.as_deref(), &stage::first_level(doc.project())?)?
            };
            let offset = match &i.offset {
                Some(v) => translate::point(v, "offset")?,
                None => Point { x: 0.0, y: 0.0 },
            };
            let options = ImportOptions {
                mm_per_unit,
                layers: i.layers.unwrap_or_default(),
                mode: i.mode.unwrap_or(ImportMode::Walls),
                offset,
                level_id: Some(level_id),
            };
            // Worked out here and committed like every edit: as the agent,
            // inside its limit and claims, with the automatic version.
            let (name, bytes) = match (&i.path, &i.data) {
                (Some(p), None) => read_file(app, p, guhit_import::MAX_BYTES).await?,
                (None, Some(d)) => (i.file_name.clone().unwrap_or_default(), decode_data(d)?),
                _ => return Err(ToolFail::args("give path or data")),
            };
            if name.to_ascii_lowercase().ends_with(".dwg") {
                return Err(ToolFail::new("unavailable", "a DWG import goes through the app's converter, which this server cannot reach")
                    .with_hint("Save the drawing as DXF and import that, or ask the user to import the DWG in the app."));
            }
            let plan = guhit_import::plan(&name, &bytes, &options).map_err(|e| ToolFail::new("invalid", e.to_string()))?;
            if plan.is_empty() {
                return Err(ToolFail::new("invalid", format!("nothing in {name} could be imported"))
                    .with_hint("Try mode \"linework\", or check with action \"inspect\" that the layers you picked hold lines."));
            }
            let step = StepIn::new("__commands", json!({"commands": plan.commands}));
            let label = format!("MCP: import {name}");
            let mut out = stage::run(app, vec![step], None, Options { label, dry_run: false, per_step: false }, ctx).await?;
            if let Output::Json(v) = &mut out {
                v["walls_added"] = json!(plan.walls);
                v["linework_added"] = json!(plan.linework);
                v["skipped"] = json!(plan.skipped);
                v["note"] = json!("One undo step. Check the walls with look and check before building on them.");
            }
            Ok(out)
        }
        other => Err(ToolFail::args(format!("unknown action `{other}`. Use inspect or commit"))),
    }
}
