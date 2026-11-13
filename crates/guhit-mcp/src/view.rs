//! The one gate every MCP read of the model goes through (DECISIONS D57,
//! docs/CONTRACT.md "Swarms").
//!
//! An agent bound to a swarm part reads the draft it draws in: the model
//! with every draft step applied (`AppService::swarm_view`). Everyone else,
//! in-process callers included, reads the open document. Nothing else in
//! this crate locks the session or asks the app for the document; a test
//! reads the sources to check.
//!
//! What the window draws (3D, renders, the window's plan, tour frames) and
//! what is written from the model (exports, versions) shows the model and
//! never the draft, so a bound agent is refused those by category
//! (`draft_view`, [`refuse_window_backed`]).

use std::path::PathBuf;
use std::sync::Arc;

use guhit_app::{AppService, Session};
use guhit_core::Document;
use guhit_model::*;
use serde_json::{json, Value};
use tokio::sync::MutexGuard;

use crate::agent::CallContext;
use crate::fail::ToolFail;
use crate::stage;

/// What one call reads: the open document, or the draft its agent draws in.
pub(crate) enum Read<'a> {
    /// The session, locked for as long as this lives.
    Main(MutexGuard<'a, Session>),
    /// The draft view: no lock held, the model with every step applied.
    Draft(Arc<Document>),
}

impl Read<'_> {
    /// The document, or None while the hub shows.
    pub fn get(&self) -> Option<&Document> {
        match self {
            Read::Main(s) => s.doc.as_ref(),
            Read::Draft(d) => Some(d),
        }
    }

    /// The document; `no_document` while the hub shows.
    pub fn doc(&self) -> Result<&Document, ToolFail> {
        self.get().ok_or_else(stage::no_document)
    }
}

/// The draft the call's agent draws in, when it is bound to a swarm part.
pub(crate) fn draft_of(app: &AppService, ctx: &CallContext) -> Option<Arc<Document>> {
    ctx.call.as_ref().and_then(|c| app.swarm_view(&c.id))
}

/// What the call reads. Takes the session lock only for the model; hold it
/// no longer than the read.
pub(crate) async fn read<'a>(app: &'a AppService, ctx: &CallContext) -> Read<'a> {
    match draft_of(app, ctx) {
        Some(view) => Read::Draft(view),
        None => Read::Main(app.session.lock().await),
    }
}

/// The project, its derived data and revision, copied out of what the call
/// reads.
pub(crate) async fn snapshot(app: &AppService, ctx: &CallContext) -> Result<(Project, Derived, u32), ToolFail> {
    let r = read(app, ctx).await;
    let doc = r.doc()?;
    Ok((doc.project().clone(), doc.derived().clone(), doc.revision()))
}

/// One engine query on what the call reads (the v1 read tools).
pub(crate) async fn query(app: &AppService, ctx: &CallContext, query: &Query) -> Result<Value, ToolFail> {
    let r = read(app, ctx).await;
    r.doc()?.query(query).map_err(ToolFail::from)
}

/// The app's data folder. Not the model: no gate needed, kept here so the
/// session is locked in one file only.
pub(crate) async fn data_dir(app: &AppService) -> PathBuf {
    app.session.lock().await.data_dir.clone()
}

/// A view (elevation, section, schedule) drawn as sheets draw it. The draft
/// is drawn here from the view; the model through the app, which caches it.
pub(crate) async fn view_svg(app: &AppService, ctx: &CallContext, view: &ViewRef, scale: Option<u32>) -> Result<SvgDrawing, ToolFail> {
    if let Some(draft) = draft_of(app, ctx) {
        let inputs = guhit_export::SheetInputs { date: None, images: Default::default(), mask_signatory: true };
        let out = guhit_app::documents::view_svg(draft.project(), draft.derived(), view, scale.filter(|n| *n > 0), &inputs)
            .map_err(crate::documents::export_fail)?;
        return Ok(SvgDrawing { svg: out.data, scale_denominator: Some(out.scale_denominator) });
    }
    let v = app.handle("view_svg", json!({"view": view, "scale": scale})).await?;
    serde_json::from_value(v).map_err(|e| ToolFail::new("io", e.to_string()))
}

/// The model's shared undo history. A bound agent never reads it: its
/// history is the draft's steps (`draft_steps`).
pub(crate) async fn main_history(app: &AppService) -> Result<HistoryList, ToolFail> {
    serde_json::from_value(app.handle("history_list", json!({})).await?).map_err(|e| ToolFail::new("io", e.to_string()))
}

/// The model's revision, for `history` undo_to.
pub(crate) async fn main_revision(app: &AppService) -> Result<u32, ToolFail> {
    Ok(app.handle("doc_revision", json!({})).await?["revision"].as_u64().unwrap_or(0) as u32)
}

/// Every step of the open swarm draft, oldest first, when the call's agent
/// draws a part of it.
pub(crate) async fn draft_steps(app: &AppService, ctx: &CallContext) -> Result<Option<(SwarmStatus, Vec<SwarmStep>)>, ToolFail> {
    if draft_of(app, ctx).is_none() {
        return Ok(None);
    }
    let Some(status) = app.swarm_status() else { return Ok(None) };
    let draft = app.swarm_draft(Some(0)).await?;
    Ok(draft.map(|d| (status, d.steps)))
}

// ------------------------------------------------------ window and disk

/// What a call shows or writes from the model rather than the draft: the
/// window's 3D view and plan, renders, AI images, tour frames and videos,
/// exports and saved versions. None for everything else.
pub(crate) fn window_backed(tool: &str, args: &Value) -> Option<&'static str> {
    let action = args.get("action").and_then(Value::as_str);
    let mode = args.get("mode").and_then(Value::as_str);
    Some(match tool {
        "look" if mode == Some("3d") => "look mode 3d (the window's 3D view)",
        "look" if mode == Some("tour") => "look mode tour (a frame of the window's 3D view)",
        "look" if args.get("source").and_then(Value::as_str) == Some("window") => "look source window (the window's plan)",
        "get_plan_image" => "the window's plan picture",
        "capture_view" => "a 3D capture",
        "render" if action == Some("start") => "a render",
        "render_view" => "a render",
        "visualize_render" => "an AI visualization",
        "export_tour" => "a tour video",
        "export" | "export_plan" => "an export",
        "sheets" if action == Some("export") => "an export",
        "project" if action == Some("save_version") => "a saved version",
        "save_version" => "a saved version",
        _ => return None,
    })
}

/// Refuse a bound agent what the window draws or what is written from the
/// model (`draft_view`): it would show the model, not the draft the agent
/// draws in.
pub(crate) fn refuse_window_backed(app: &AppService, ctx: &CallContext, tool: &str, args: &Value) -> Result<(), ToolFail> {
    let Some(what) = window_backed(tool, args) else { return Ok(()) };
    if draft_of(app, ctx).is_none() {
        return Ok(());
    }
    Err(ToolFail::new(
        "draft_view",
        format!("{what} shows the model, not the swarm draft you draw in, so a swarm part cannot use it."),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every MCP read goes through this file (amendments, "Every read and
    /// write goes through one gate"): no other source file locks the
    /// session, takes the document out of it, or asks the app for the
    /// document, its history or its drawings. Test code is left out.
    #[test]
    fn no_tool_reads_the_document_around_the_gate() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let forbidden = [
            "session.lock(",
            "session.try_lock(",
            ".doc.as_ref(",
            "s.doc",
            "open_doc(",
            "\"doc_query\"",
            "\"doc_state\"",
            "\"doc_revision\"",
            "\"history_list\"",
            "\"view_svg\"",
            "\"sheet_preview\"",
        ];
        let mut found = vec![];
        let mut files = 0;
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") || path.file_name().and_then(|n| n.to_str()) == Some("view.rs") {
                continue;
            }
            files += 1;
            let text = std::fs::read_to_string(&path).unwrap();
            let code = text.split("#[cfg(test)]").next().unwrap_or("");
            for (n, line) in code.lines().enumerate() {
                let line = line.split("//").next().unwrap_or("");
                for f in forbidden {
                    if line.contains(f) {
                        found.push(format!("{}:{}: {}", path.file_name().unwrap().to_string_lossy(), n + 1, line.trim()));
                    }
                }
            }
        }
        assert!(files > 15, "the sources were not found in {}", dir.display());
        assert!(found.is_empty(), "these read the model around view.rs:\n{}", found.join("\n"));
    }

    #[test]
    fn window_and_disk_outputs_are_named_by_category() {
        for (tool, args) in [
            ("look", json!({"mode": "3d"})),
            ("look", json!({"mode": "tour", "tour": "t"})),
            ("look", json!({"source": "window"})),
            ("render", json!({"action": "start"})),
            ("visualize_render", json!({})),
            ("export_tour", json!({"tour": "t"})),
            ("export", json!({"format": "pdf"})),
            ("sheets", json!({"action": "export"})),
            ("project", json!({"action": "save_version", "label": "x"})),
            ("capture_view", json!({})),
            ("get_plan_image", json!({})),
        ] {
            assert!(window_backed(tool, &args).is_some(), "{tool} {args} is not refused");
        }
        for (tool, args) in [
            ("look", json!({})),
            ("look", json!({"mode": "elevation", "mark": "A"})),
            ("look", json!({"mode": "sheet", "sheet": "A-1"})),
            ("render", json!({"action": "list"})),
            ("sheets", json!({"action": "list"})),
            ("project", json!({"action": "list"})),
            ("query", json!({"what": "summary"})),
            ("measure", json!({"kind": "distance"})),
        ] {
            assert!(window_backed(tool, &args).is_none(), "{tool} {args} reads the draft, it is not refused");
        }
    }
}
