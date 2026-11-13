//! The MCP tool set: the v2 tools (`draft`, `extended`) and the v1 names,
//! kept for one release as hidden aliases.
//!
//! Every edit, v1 or v2, runs through `stage`: translated, staged on one
//! working copy, checked against the scope (DECISIONS D30) and committed as
//! exactly one `Command::Batch` with `Origin::Ai` and an undo label prefixed
//! "MCP: ", so one undo reverts one tool call. The v1 editing tools
//! translate through the copilot (`guhit_app::ai::tools`), as before, so an
//! MCP client and the in-app copilot behave the same.
//!
//! Every editing tool takes an optional `scope`: the engine refuses a
//! command that reaches outside it. When the user switched on "Only the
//! selection" in the window, the window's selection is the scope of every
//! edit, whatever the call says. Every tool takes an optional `agent`: the
//! name the user sees for the caller (crate::agent).

use guhit_app::ai::tools as copilot;
use guhit_app::AppService;
use guhit_model::*;
use serde_json::{json, Map, Value};

use crate::agent::{self, CallContext};
use crate::session;
use crate::stage::{self, Options, StepIn};

pub use crate::fail::ToolFail;

/// A picture for the model: base64 bytes and their media type.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub base64: String,
    pub mime: String,
}

/// What a tool produced. Almost everything is JSON; the plan and render tools
/// hand back pictures with it.
pub enum Output {
    Json(Value),
    /// Plain text, such as a guide.
    Text(String),
    /// JSON plus pictures: a render's record and its preview, the plan.
    Rich { json: Value, images: Vec<Picture> },
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
    /// `openWorldHint`. True when the tool reaches beyond this computer: the
    /// image provider, or the other people in a live session.
    pub open_world: bool,
}

pub(crate) const MM: &str = "All lengths are MILLIMETERS.";

/// `export` sheet names, as `SheetKind` spells them.
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

pub(crate) fn obj(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

/// Every v2 tool, every toolset, in list order, each schema with the
/// optional `agent` argument.
pub fn definitions() -> Vec<ToolDef> {
    let mut out = crate::draft::definitions();
    for d in &mut out {
        d.schema["properties"]["agent"] = agent::argument_schema();
    }
    out
}

/// The `scope` argument every editing tool takes (DECISIONS D30).
pub(crate) fn scope_schema() -> Value {
    json!({
        "description": "\"selection\" (the user's, in the window) or element ids: an edit reaching outside is refused out_of_scope. With \"Only the selection\" on in the app, the selection binds every edit.",
        "anyOf": [
            {"type": "string", "enum": ["selection"]},
            {"type": "array", "items": {"type": "string"}, "minItems": 1},
        ],
    })
}

// ------------------------------------------------------------------- dispatch

fn args_object(args: &Value) -> Result<&Map<String, Value>, ToolFail> {
    args.as_object().ok_or_else(|| ToolFail::msg("invalid arguments: expected a JSON object"))
}

pub(crate) fn opt_str(args: &Value, name: &str) -> Result<Option<String>, ToolFail> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(ToolFail::msg(format!("invalid arguments: `{name}` must be a string"))),
    }
}

pub(crate) fn req_str(args: &Value, name: &str) -> Result<String, ToolFail> {
    opt_str(args, name)?
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ToolFail::msg(format!("invalid arguments: `{name}` is required")))
}

pub(crate) fn opt_bool(args: &Value, name: &str) -> Result<Option<bool>, ToolFail> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(ToolFail::msg(format!("invalid arguments: `{name}` must be true or false"))),
    }
}

pub(crate) fn opt_f64(args: &Value, name: &str) -> Result<Option<f64>, ToolFail> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|n| n.is_finite())
            .map(Some)
            .ok_or_else(|| ToolFail::msg(format!("invalid arguments: `{name}` must be a number"))),
    }
}

/// Run one tool for an in-process caller: every toolset on, nobody named.
pub async fn call(app: &AppService, name: &str, args: Value) -> Result<Output, ToolFail> {
    call_with(app, name, args, &CallContext::local()).await
}

/// True for a call that spends money or runs for long without changing the
/// model: an AI image, a traced render, a tour video. A paused agent may not
/// start one (GR1 #8).
pub fn spends(name: &str, args: &Value) -> bool {
    let action = args.get("action").and_then(Value::as_str);
    match name {
        "visualize_render" | "render_view" | "export_tour" => true,
        "render" => action == Some("start"),
        _ => false,
    }
}

/// What a call is to the agents registry: an edit, work that spends (both
/// refused while the agent is paused) or a read.
pub fn call_kind(name: &str, args: &Value) -> guhit_app::agents::CallKind {
    use guhit_app::agents::CallKind;
    if spends(name, args) {
        CallKind::Work
    } else if reads_only(name, args) {
        CallKind::Read
    } else {
        CallKind::Edit
    }
}

/// True for a call that leaves the model as it is: reads, pictures, renders,
/// exports, chat and status. The agents registry lets a paused agent make
/// these, except the ones that spend (`spends`), and refuses its edits.
pub fn reads_only(name: &str, args: &Value) -> bool {
    let action = args.get("action").and_then(Value::as_str);
    let dry_run = args.get("dry_run").and_then(Value::as_bool) == Some(true);
    match name {
        "get_guide" | "query" | "look" | "measure" | "check" | "get_session" | "export" | "render"
        | "visualize_render" | "send_chat_message" | "set_status" => true,
        "project" => matches!(action, Some("list" | "save_version")),
        "levels" | "history" => action == Some("list"),
        "import_dxf" => action == Some("inspect"),
        "build" | "batch" => dry_run,
        // Wave 2: reads of the editing tools, and work that writes files or
        // records but never the model.
        "types" | "view_marks" => action == Some("list"),
        "title_block" => action == Some("get"),
        "sheets" => matches!(action, Some("list" | "preview" | "export")),
        "export_tour" | "set_look" => true,
        "export_plan" | "save_version" | "capture_view" | "render_view" => true,
        n => definitions_read_only(n),
    }
}

fn definitions_read_only(name: &str) -> bool {
    matches!(
        name,
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
            | "get_selection"
            | "get_render_job"
            | "get_render_image"
    )
}

/// Run one tool. `args` is whatever the client sent; every tool validates it.
pub async fn call_with(app: &AppService, name: &str, mut args: Value, ctx: &CallContext) -> Result<Output, ToolFail> {
    args_object(&args)?;
    let mut ctx = ctx.clone();
    let named = agent::take_argument(&mut args);
    if let (Some(hint), Some(named)) = (ctx.hint.as_mut(), named) {
        hint.agent = Some(named);
    }
    agent::admit(app, &mut ctx, name, &args, call_kind(name, &args))?;
    let mut result = dispatch(app, name, &args, &ctx).await;
    agent::after(app, &ctx, &args, &result).await;
    // What the agent should hear, on every answer it gets: the registry's
    // notes (DECISIONS D47, research r5 3.4), each told once. `activity`
    // says whether it may edit; `inbox` is chat naming it, `nearby` other
    // people's and agents' steps inside its claim.
    if let Some(call) = &ctx.call {
        let notes = app.agent_notes(call).await;
        if let Ok(Output::Json(v) | Output::Rich { json: v, .. }) = &mut result {
            if v.is_object() {
                v["activity"] = json!(notes.activity);
                if !notes.inbox.is_empty() {
                    v["inbox"] = json!(notes.inbox);
                }
                if !notes.nearby.is_empty() {
                    v["nearby"] = json!(notes.nearby);
                }
            }
        }
    }
    result
}

/// Run a tool for a call that was admitted already: v2 tools use it for the
/// v1 code they share, with the same caller.
pub(crate) async fn dispatch(app: &AppService, name: &str, args: &Value, ctx: &CallContext) -> Result<Output, ToolFail> {
    // Boxed: v2 tools run v1 code through `dispatch`.
    if let Some(done) = Box::pin(crate::draft::call(app, name, args, ctx)).await {
        return done;
    }
    if let Some(done) = Box::pin(crate::extended::call(app, name, args, ctx)).await {
        return done;
    }
    if let Some(done) = session::call(app, name, args, ctx).await {
        return done;
    }
    if let Some(done) = crate::visuals::call(app, name, args, ctx).await {
        return done;
    }
    let args = args.clone();
    match name {
        // ---------------------------------------------------------- v1 hub
        "list_projects" => ok(app.handle("hub_list", json!({})).await?),
        "open_project" => {
            let id = req_str(&args, "id")?;
            session::guard_switch(app, &args, session::Switch::Open(&id)).await?;
            let state: Value = app.handle("hub_open", json!({ "id": id })).await?;
            let mut out = opened(&state);
            // What opening it mended (GR1 #14): the agent tells the user.
            let repairs = app.open_repairs().await;
            if !repairs.is_empty() {
                out["repairs"] = json!(repairs);
                out["repairs_note"] = json!("Opening this project mended what the app could not use, as listed; the file keeps it until the next change is saved. Tell the user.");
            }
            ok(out)
        }
        "create_project" => {
            let name = req_str(&args, "name")?;
            let template = opt_str(&args, "template")?.unwrap_or_else(|| "blank".into());
            session::guard_switch(app, &args, session::Switch::Create).await?;
            let state: Value = app.handle("hub_create", json!({ "name": name, "template": template })).await?;
            ok(opened(&state))
        }
        "close_project" => {
            session::guard_switch(app, &args, session::Switch::Close).await?;
            app.handle("hub_close", json!({})).await?;
            ok(json!({"ok": true, "open": false, "note": "the desktop window is back on the project hub"}))
        }

        // --------------------------------------------------------- v1 read
        n if copilot::is_read(n) => {
            let query = copilot::to_query(n, &args)?;
            ok(app.handle("doc_query", json!({ "query": query })).await?)
        }
        "list_renders" => ok(app.handle("render_list", json!({})).await?),
        "get_plan_image" => crate::visuals::plan_image(app, &args).await,

        // --------------------------------------------------------- v1 edit
        n if copilot::is_edit(n) => {
            let mut args = args;
            let scope = stage::take_scope(&mut args)?;
            let label = format!("MCP: {}", n.replace('_', " "));
            stage::run(app, vec![StepIn::new(n, args)], scope.as_ref(), Options { label, dry_run: false, per_step: false }, ctx).await
        }
        "set_review_mark" => set_review_mark(app, &args, ctx).await,

        // ------------------------------------------------ v1 history, files
        "undo" | "redo" => {
            let force = opt_bool(&args, "force")?.unwrap_or(false);
            let cmd = if name == "undo" { "doc_undo" } else { "doc_redo" };
            let state: Value = match &ctx.call {
                Some(call) => serde_json::to_value(app.agent_undo(call, name == "redo", force).await?)
                    .map_err(|e| ToolFail::new("io", e.to_string()))?,
                None => {
                    app.agent_steps_allowed(name == "redo", 1).await?;
                    app.handle(cmd, json!({ "force": force })).await?
                }
            };
            ok(history(&state, if name == "undo" { "undone" } else { "redone" }))
        }
        "save_version" => {
            let label = req_str(&args, "label")?;
            ok(app.handle("snapshot_create", json!({ "label": label })).await?)
        }
        "export_plan" => ok(export_plan(app, &args, None).await?),

        other => {
            let mut f = ToolFail::new("unknown_tool", format!("unknown tool `{other}`"));
            if let Some(n) = crate::toolsets::nearest(other) {
                f = f.with_hint(format!("Did you mean `{n}`? tools/list shows every tool."));
                f.message = format!("unknown tool `{other}`. Did you mean `{n}`?");
            }
            Err(f)
        }
    }
}

/// A plan sheet: `export_plan` in v1, `export` pdf, svg and dxf in v2, of
/// `level_id` (None: the first level).
pub(crate) async fn export_plan(app: &AppService, args: &Value, level_id: Option<Id>) -> Result<Value, ToolFail> {
    let format = req_str(args, "format")?;
    let settings_paper = {
        let s = app.session.lock().await;
        s.doc
            .as_ref()
            .map(|d| d.project().settings.paper)
            .ok_or_else(|| IpcError::new("no_document", "no project is open"))?
    };
    let paper = match opt_str(args, "paper")?.as_deref() {
        None => settings_paper,
        Some("a4") => PaperSize::A4,
        Some("a3") => PaperSize::A3,
        Some("a2") => PaperSize::A2,
        Some("a1") => PaperSize::A1,
        Some(other) => return Err(ToolFail::msg(format!("invalid arguments: unknown paper `{other}`. Use a4, a3, a2 or a1"))),
    };
    let orientation = match opt_str(args, "orientation")?.as_deref() {
        None | Some("landscape") => Orientation::Landscape,
        Some("portrait") => Orientation::Portrait,
        Some(other) => {
            return Err(ToolFail::msg(format!("invalid arguments: unknown orientation `{other}`. Use landscape or portrait")))
        }
    };
    let sheet = match opt_str(args, "sheet")?.as_deref() {
        None => SheetKind::Plan,
        Some(name) => sheet_kind(name)
            .ok_or_else(|| ToolFail::msg(format!("invalid arguments: unknown sheet `{name}`. Use {}", SHEETS.join(", "))))?,
    };
    let review_page = opt_bool(args, "review_page")?.unwrap_or(false);
    if review_page && format != "pdf" {
        return Err(ToolFail::msg(
            "invalid arguments: review_page adds a page to a PDF. Use format \"pdf\", or leave review_page out",
        ));
    }
    let scale_denominator = match args.get("scale_denominator") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .filter(|n| *n > 0 && *n <= u32::MAX as u64)
                .ok_or_else(|| ToolFail::msg("invalid arguments: scale_denominator must be a positive whole number"))?
                as u32,
        ),
    };
    let options = PlanExportOptions {
        level_id,
        paper,
        orientation,
        scale_denominator,
        show_dimensions: opt_bool(args, "show_dimensions")?.unwrap_or(true),
        show_room_labels: opt_bool(args, "show_room_labels")?.unwrap_or(true),
        show_assets: opt_bool(args, "show_assets")?.unwrap_or(true),
        title_block: opt_bool(args, "title_block")?.unwrap_or(true),
        show_pipes: opt_bool(args, "show_pipes")?.unwrap_or(true),
        sheet,
        review_page,
        date: None,
    };
    Ok(app
        .handle("export_plan", json!({ "format": format, "options": options, "path": Value::Null }))
        .await?)
}

/// `set_review_mark`: one `Command::SetReviewMark`, committed as one undo
/// step like every edit here. The engine checks the target.
async fn set_review_mark(app: &AppService, args: &Value, ctx: &CallContext) -> Result<Output, ToolFail> {
    let mut args = args.clone();
    let asked = stage::take_scope(&mut args)?;
    let action = req_str(&args, "action")?;
    let issue_id = opt_str(&args, "issue_id")?.filter(|s| !s.trim().is_empty());
    let code = opt_str(&args, "code")?.filter(|s| !s.trim().is_empty());
    let element_id = opt_str(&args, "element_id")?.filter(|s| !s.trim().is_empty());
    let note = opt_str(&args, "note")?.map(|n| n.trim().to_string());
    let target = match (issue_id, code, element_id) {
        (Some(id), None, None) => ReviewTarget::Issue { id },
        (None, Some(code), None) => ReviewTarget::Check { code },
        (None, Some(code), Some(element_id)) => ReviewTarget::Element { code, element_id },
        (Some(_), _, _) => {
            return Err(ToolFail::msg(
                "invalid arguments: give either issue_id, or code with an optional element_id, not both",
            ))
        }
        (None, None, _) => {
            return Err(ToolFail::msg(
                "invalid arguments: say what to set aside: issue_id, or code with an optional element_id",
            ))
        }
    };
    let note = match action.as_str() {
        "set_aside" => match note {
            Some(n) if !n.is_empty() => Some(n),
            _ => return Err(ToolFail::msg("invalid arguments: set_aside needs a note saying why, in the user's words")),
        },
        "reopen" => None,
        other => return Err(ToolFail::msg(format!("invalid arguments: unknown action `{other}`. Use set_aside or reopen"))),
    };
    let label = match note {
        Some(_) => "MCP: set a review item aside",
        None => "MCP: reopen a review item",
    };
    let command = Command::Batch {
        label: label.to_string(),
        commands: vec![Command::SetReviewMark { target: target.clone(), note }],
    };
    let revision = {
        let s = app.session.lock().await;
        let doc = s.doc.as_ref().ok_or_else(stage::no_document)?;
        if let Some(scope) = stage::resolve_scope(app, doc.project(), asked.as_ref())? {
            guhit_core::scope::check(doc.project(), doc.derived(), &scope.ids, &command)
                .map_err(|e| stage::scope_fail(e, doc.project(), doc.derived(), &scope))?;
        }
        doc.revision()
    };
    let result = match &ctx.call {
        Some(call) => app.agent_commit(call, command).await?,
        None => app.commit_if_revision(command, Origin::Ai, revision).await?,
    };
    let matches = |i: &Issue| match &target {
        ReviewTarget::Issue { id } => i.id == id.trim(),
        ReviewTarget::Check { code } => i.code == code.trim(),
        ReviewTarget::Element { code, element_id } => i.code == code.trim() && i.element_ids.iter().any(|e| e == element_id.trim()),
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

/// Compact answer after opening or creating a project.
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

/// One command a tool built itself (not through a translation), checked
/// against the scope and committed as one undo step like every edit.
pub(crate) async fn commit_one(app: &AppService, label: &str, command: Command, args: &Value, ctx: &CallContext) -> Result<Output, ToolFail> {
    let mut scoped = args.clone();
    let asked = stage::take_scope(&mut scoped)?;
    let step = StepIn::new("__command", serde_json::to_value(&command).map_err(|e| ToolFail::new("io", e.to_string()))?);
    stage::run(app, vec![step], asked.as_ref(), Options { label: label.to_string(), dry_run: false, per_step: false }, ctx).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolsets::{self, Toolsets};

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
            assert!(d.schema["properties"].get("agent").is_some(), "{} takes no agent", d.name);
            assert!(d.description.contains("MILLIMETERS"), "{} does not say what unit it works in", d.name);
            assert!(d.description.len() <= 1024, "{} has a {} byte description; keep it under 1 KB", d.name, d.description.len());
        }
    }

    #[test]
    fn the_list_is_the_v2_toolsets() {
        let names: Vec<&str> = definitions().iter().map(|d| d.name).collect();
        assert_eq!(names, toolsets::all_v2(), "definitions and toolsets disagree");
        assert_eq!(Toolsets::core().tools().len(), 26);
        assert_eq!(names.len(), 45);
    }

    #[test]
    fn every_editing_tool_takes_a_scope() {
        for d in definitions() {
            let edits = crate::translate::EDIT_TOOLS.contains(&d.name)
                || matches!(d.name, "build" | "batch" | "import_underlay")
                || d.name == "set_review_mark";
            // Project-wide (D30): outside every scope, so they take none.
            let project_wide = matches!(
                d.name,
                "levels" | "set_project" | "add_grid" | "types" | "title_block" | "sheets" | "set_lot" | "make_tour" | "edit_tour"
            );
            assert_eq!(
                d.schema["properties"].get("scope").is_some(),
                edits && !project_wide && d.name != "add_camera",
                "scope on `{}` is wrong",
                d.name
            );
        }
    }

    #[test]
    fn only_the_provider_and_the_session_reach_outside() {
        for d in definitions() {
            assert_eq!(
                d.open_world,
                matches!(d.name, "visualize_render" | "send_chat_message" | "set_status"),
                "openWorldHint is wrong for {}",
                d.name
            );
        }
    }

    #[test]
    fn read_and_edit_tools_are_marked_correctly() {
        for d in definitions() {
            let expected = matches!(d.name, "get_guide" | "query" | "look" | "measure" | "check" | "get_session");
            assert_eq!(d.read_only, expected, "readOnlyHint is wrong for {}", d.name);
        }
    }

    #[test]
    fn every_sheet_name_is_a_sheet_kind_spelled_the_same_way() {
        for name in SHEETS {
            let kind = sheet_kind(name).unwrap_or_else(|| panic!("`{name}` is not a sheet"));
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(name));
        }
        assert_eq!(sheet_kind("elevation"), None);
    }
}
