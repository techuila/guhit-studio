//! The `swarm` core tool (DECISIONS D56, D57, docs/CONTRACT.md "Swarms",
//! docs/MCP.md "Swarms").
//!
//! An MCP lead splits a brief into parts and starts a swarm with them; it
//! hands each sub-agent its part's handle. The sub-agents draw into a draft
//! (every read and edit of a bound agent goes to it, `view.rs`), report
//! milestones, wait for the parts they come after, and finish. The lead
//! watches, finishes last to mark the draft ready, and tells the user. A
//! person accepts or discards in the window: `accept` is always refused.
//!
//! The app does the work (`AppService::swarm_start_mcp`, `swarm_status`,
//! `swarm_milestone`, `swarm_finish`, `swarm_discard`); binding a handle to
//! its part happens in `AppService::agent_call`.

use std::time::{Duration, Instant};

use guhit_app::agents::AgentCall;
use guhit_app::AppService;
use guhit_model::*;
use serde_json::{json, Value};

use crate::agent::CallContext;
use crate::fail::ToolFail;
use crate::tools::{obj, opt_bool, opt_f64, opt_str, req_str, Output, ToolDef, MM};

/// Longest `wait`, s: under the usual MCP client timeouts.
pub const MAX_WAIT_S: f64 = 60.0;
/// `wait` without `timeout_s`.
const DEFAULT_WAIT_S: f64 = 30.0;
/// The lead's own part key.
const LEAD: &str = "lead";

/// What the lead hands each sub-agent with its handle.
pub const HOW_TO: &str = "Give each sub-agent its part's handle and instructions. It passes agent: \"<handle>\" on every tool call (or connects to the part's url), calls swarm wait first when its part comes after another, draws only its part's levels and kinds, checks with look and measure, calls swarm milestone as it reaches each one, and ends with swarm finish. Wait for every part (swarm wait), then call swarm finish yourself and ask the user to review the draft and press Accept in Guhit Studio. Nothing reaches the model until a person accepts.";

/// The tool as `tools/list` shows it.
pub(crate) fn definition() -> ToolDef {
    ToolDef {
        name: "swarm",
        description: format!(
            "Several agents draw one building at once into a draft a person accepts in Guhit Studio. start: you lead; a handle per part. Each sub-agent passes agent: <handle> on every call, draws only its part's levels and kinds, reports milestone, then finish. wait: until a part reaches a milestone, at most 60 s. status: parts and counts. Your finish as lead marks the draft ready. discard needs force. Only a person accepts. {MM}"
        ),
        schema: obj(
            json!({
                "action": {"type": "string", "enum": ["start", "status", "milestone", "wait", "finish", "discard", "accept"]},
                "brief": {"type": "string"},
                "summary": {"type": "string"},
                "setup": {"type": "object", "description": "{levels: [{name, height_mm}]}: levels the lead adds first."},
                "parts": {"type": "array", "items": {"type": "object"}, "description": "1 to 6 of {key (lowercase, hyphens), name, role, instructions, scope: {levels (ids or names), kinds}, after: [{part, milestone}], milestones}. Empty lists mean all."},
                "name": {"type": "string", "description": "milestone: one your part declares."},
                "for": {"type": "object", "description": "wait: {part, milestone}; default what your part comes after."},
                "timeout_s": {"type": "number"},
                "force": {"type": "boolean"},
            }),
            &["action"],
        ),
        read_only: false,
        open_world: false,
    }
}

/// Run the tool.
pub(crate) async fn call(app: &AppService, args: &Value, ctx: &CallContext) -> Result<Output, ToolFail> {
    let action = args.get("action").and_then(Value::as_str).unwrap_or_default();
    let out = match action {
        "start" => start(app, args, ctx).await,
        "status" => status(app, ctx).await,
        "milestone" => milestone(app, args, ctx),
        "wait" => wait(app, args, ctx).await,
        "finish" => finish(app, args, ctx),
        "discard" => discard(app, args, ctx).await,
        "accept" => Err(ToolFail::new("people_only", "Only a person accepts a swarm draft, in the Guhit Studio window.")),
        other => Err(ToolFail::args(format!(
            "unknown action `{other}`. Use start, status, milestone, wait, finish or discard"
        ))),
    };
    // A live session refuses a swarm; its usual advice (ask, then `force`)
    // is for switching projects, not this.
    out.map_err(|f| {
        if f.code == "live_session" {
            f.with_hint("Swarms do not run in a live session: the draft stays on one computer. Tell the user; once they leave the session, start the swarm again.")
        } else {
            f
        }
    })
}

fn agent_call(ctx: &CallContext) -> Result<&AgentCall, ToolFail> {
    ctx.call
        .as_ref()
        .ok_or_else(|| ToolFail::args("A swarm needs an agent connection: call it over MCP, not in process."))
}

fn no_swarm() -> ToolFail {
    ToolFail::new("not_found", "No swarm draft is open.").with_hint("swarm action \"start\" starts one.")
}

// ------------------------------------------------------------------ start

async fn start(app: &AppService, args: &Value, ctx: &CallContext) -> Result<Output, ToolFail> {
    let call = agent_call(ctx)?;
    let brief = opt_str(args, "brief")?.unwrap_or_default();
    let plan: SwarmPlan = serde_json::from_value(json!({
        "summary": args.get("summary").cloned().unwrap_or(json!("")),
        "setup": args.get("setup").cloned().unwrap_or(json!({})),
        "parts": args.get("parts").cloned().unwrap_or(json!([])),
    }))
    .map_err(|e| ToolFail::args(format!("swarm start: {e}")))?;
    let started = app.swarm_start_mcp(call, &brief, plan).await?;
    Ok(Output::Json(json!({
        "ok": true,
        "swarm_id": started.swarm.id,
        "you": LEAD,
        "swarm": brief_status(&started.swarm, Some(LEAD)),
        "handles": started.handles,
        "how_to": HOW_TO,
    })))
}

// ----------------------------------------------------------------- status

/// The swarm as an agent reads it: parts, their milestones and what they
/// wait for, and the draft's counts. `me` is the caller's part.
fn brief_status(s: &SwarmStatus, me: Option<&str>) -> Value {
    let parts: Vec<Value> = s
        .parts
        .iter()
        .map(|p| {
            let mut v = json!({
                "key": p.key, "name": p.name, "state": p.state, "steps": p.steps, "milestones": p.milestones,
            });
            if let Some(line) = &p.status {
                v["status"] = json!(line);
            }
            if !p.waiting_for.is_empty() {
                v["waiting_for"] = json!(p.waiting_for.iter().map(|w| format!("{}:{}", w.part, w.milestone)).collect::<Vec<_>>());
            }
            if p.conflicts > 0 {
                v["conflicts"] = json!(p.conflicts);
            }
            if me == Some(p.key.as_str()) {
                v["yours"] = json!(true);
            }
            v
        })
        .collect();
    let mut out = json!({
        "id": s.id,
        "state": s.state,
        "parts": parts,
        "draft": {"revision": s.draft_revision, "added": s.added, "modified": s.modified, "removed": s.removed},
        "feed": s.feed.iter().take(5).map(|f| f.text.clone()).collect::<Vec<_>>(),
    });
    if !s.conflicts.is_empty() {
        out["conflicts"] = json!(s.conflicts);
    }
    out
}

fn kind_name(kind: ElementKind) -> String {
    serde_json::to_value(kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

async fn status(app: &AppService, ctx: &CallContext) -> Result<Output, ToolFail> {
    let me = ctx.call.as_ref().and_then(|c| app.swarm_part_of(&c.id));
    let Some(status) = app.swarm_status() else {
        return Ok(Output::Json(json!({"swarm": null, "note": "No swarm draft is open."})));
    };
    let mut out = brief_status(&status, me.as_deref());
    // What each part drew, by kind, worked out by the engine's diff of the
    // draft against the model.
    if let Some(draft) = app.swarm_draft(Some(u32::MAX)).await? {
        let elements = &draft.state.project.elements;
        let mut all: std::collections::BTreeMap<String, u32> = Default::default();
        for ids in &draft.parts {
            let mut drew: std::collections::BTreeMap<String, u32> = Default::default();
            for el in elements.iter().filter(|e| ids.ids.contains(e.id())) {
                *drew.entry(kind_name(el.kind())).or_default() += 1;
                *all.entry(kind_name(el.kind())).or_default() += 1;
            }
            if let Some(row) = out["parts"].as_array_mut().and_then(|rows| rows.iter_mut().find(|r| r["key"] == ids.part.as_str())) {
                row["drew"] = json!(drew);
            }
        }
        out["draft"]["by_kind"] = json!(all);
    }
    out["note"] = json!(match status.state {
        SwarmState::Ready => "The draft is ready: a person reviews it and accepts or discards it in Guhit Studio.",
        _ => "Nothing in the draft reaches the model until a person accepts it in Guhit Studio.",
    });
    Ok(Output::Json(json!({"swarm": out})))
}

// -------------------------------------------------------------- milestone

fn milestone(app: &AppService, args: &Value, ctx: &CallContext) -> Result<Output, ToolFail> {
    let call = agent_call(ctx)?;
    let name = req_str(args, "name")?;
    if name.trim() == "done" {
        return Err(ToolFail::args("\"done\" is reached by swarm action \"finish\"; milestone names one your part declares"));
    }
    let status = app.swarm_milestone(call, &name)?;
    let me = app.swarm_part_of(&call.id);
    // Parts that waited only for this, and can start now.
    let key = me.clone().unwrap_or_default();
    let free: Vec<&str> = status
        .parts
        .iter()
        .filter(|p| p.state == SwarmPartState::Waiting && p.waiting_for.is_empty())
        .filter(|p| status.plan.as_ref().is_some_and(|plan| plan.parts.iter().any(|q| q.key == p.key && q.after.iter().any(|w| w.part == key))))
        .map(|p| p.name.as_str())
        .collect();
    let mut out = json!({"ok": true, "part": me, "milestone": name.trim(), "swarm": brief_status(&status, me.as_deref())});
    if !free.is_empty() {
        out["can_start"] = json!(free);
    }
    Ok(Output::Json(out))
}

// ------------------------------------------------------------------- wait

/// What a `wait` waits for.
enum Until {
    /// One part reaching one milestone ("done" when it finishes).
    One(SwarmWait),
    /// Everything the caller's part comes after.
    Mine(String),
    /// Every part but the lead's finished, stopped or failed.
    AllParts,
}

fn finished(p: &SwarmPart) -> bool {
    matches!(p.state, SwarmPartState::Done | SwarmPartState::Accepted)
}

fn ended(p: &SwarmPart) -> bool {
    matches!(p.state, SwarmPartState::Stopped | SwarmPartState::Failed)
}

/// Whether `until` holds on `s`: Some(true) reached, Some(false) can never
/// be (a waited-for part stopped), None not yet.
fn reached(s: &SwarmStatus, until: &Until) -> Result<Option<bool>, ToolFail> {
    let part = |key: &str| {
        s.parts
            .iter()
            .find(|p| p.key == key)
            .ok_or_else(|| ToolFail::args(format!("the swarm has no part `{key}`. Parts: {}", s.parts.iter().map(|p| p.key.as_str()).collect::<Vec<_>>().join(", "))))
    };
    Ok(match until {
        Until::One(w) => {
            let p = part(&w.part)?;
            if p.milestones.contains(&w.milestone) || (w.milestone == "done" && finished(p)) {
                Some(true)
            } else if ended(p) {
                Some(false)
            } else {
                None
            }
        }
        Until::Mine(key) => {
            let me = part(key)?;
            if me.waiting_for.is_empty() {
                Some(true)
            } else if me.waiting_for.iter().any(|w| s.parts.iter().any(|p| p.key == w.part && ended(p))) {
                Some(false)
            } else {
                None
            }
        }
        Until::AllParts => s.parts.iter().filter(|p| p.key != LEAD).all(|p| finished(p) || ended(p)).then_some(true),
    })
}

async fn wait(app: &AppService, args: &Value, ctx: &CallContext) -> Result<Output, ToolFail> {
    let timeout = opt_f64(args, "timeout_s")?.unwrap_or(DEFAULT_WAIT_S);
    if timeout < 0.0 {
        return Err(ToolFail::args("timeout_s is 0 to 60"));
    }
    let timeout = Duration::from_secs_f64(timeout.min(MAX_WAIT_S));
    let me = ctx.call.as_ref().and_then(|c| app.swarm_part_of(&c.id));
    let until = match args.get("for") {
        None | Some(Value::Null) => match &me {
            Some(key) if key != LEAD => Until::Mine(key.clone()),
            _ => Until::AllParts,
        },
        Some(v) => {
            let part = v.get("part").and_then(Value::as_str).map(str::trim).filter(|p| !p.is_empty());
            let part = part.ok_or_else(|| ToolFail::args("for is {part, milestone}: the part's key, and one of its milestones or \"done\""))?;
            let milestone = v.get("milestone").and_then(Value::as_str).map(str::trim).filter(|m| !m.is_empty()).unwrap_or("done");
            Until::One(SwarmWait { part: part.to_string(), milestone: milestone.to_string() })
        }
    };
    let what = match &until {
        Until::One(w) => format!("{} to reach {}", w.part, w.milestone),
        Until::Mine(_) => "what your part comes after".to_string(),
        Until::AllParts => "every part to finish".to_string(),
    };
    // Listen first, then look, so nothing that lands in between is missed.
    let mut events = app.events();
    let started = Instant::now();
    loop {
        let Some(status) = app.swarm_status() else {
            return Ok(Output::Json(json!({
                "reached": false,
                "ended": true,
                "note": "The swarm has ended: a person accepted or discarded its draft. Stop working and tell the user.",
            })));
        };
        let done = reached(&status, &until)?;
        let waited = (started.elapsed().as_secs_f64() * 10.0).round() / 10.0;
        if let Some(ok) = done {
            let mut out = json!({"reached": ok, "for": what, "waited_s": waited, "swarm": brief_status(&status, me.as_deref())});
            if !ok {
                out["note"] = json!("A part this waits for stopped or failed, so it will not get there. Tell the user, or ask the lead what to do.");
            }
            return Ok(Output::Json(out));
        }
        let left = timeout.saturating_sub(started.elapsed());
        if left.is_zero() {
            return Ok(Output::Json(json!({
                "reached": false,
                "timed_out": true,
                "for": what,
                "waited_s": waited,
                "swarm": brief_status(&status, me.as_deref()),
                "note": "Not yet. Call wait again; status shows what the other parts are doing.",
            })));
        }
        // Any event, or a lagging receiver, is a reason to look again.
        if let Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) = tokio::time::timeout(left, events.recv()).await {
            tokio::time::sleep(left.min(Duration::from_millis(250))).await;
        }
    }
}

// ----------------------------------------------------------------- finish

fn finish(app: &AppService, args: &Value, ctx: &CallContext) -> Result<Output, ToolFail> {
    let call = agent_call(ctx)?;
    let summary = opt_str(args, "summary")?;
    let status = app.swarm_finish(call, summary.as_deref())?;
    let me = app.swarm_part_of(&call.id);
    let lead = me.as_deref() == Some(LEAD);
    let still: Vec<&str> =
        status.parts.iter().filter(|p| p.key != LEAD && !finished(p) && !ended(p)).map(|p| p.name.as_str()).collect();
    let mut out = json!({"ok": true, "part": me, "swarm": brief_status(&status, me.as_deref())});
    out["note"] = json!(if lead {
        "The draft is ready for review. Tell the user to look at it in Guhit Studio and press Accept, or Discard. Only a person accepts; you never do."
    } else {
        "Your part is done. Stop drawing and report to the lead."
    });
    if lead && !still.is_empty() {
        out["still_drawing"] = json!(still);
    }
    Ok(Output::Json(out))
}

// ---------------------------------------------------------------- discard

async fn discard(app: &AppService, args: &Value, ctx: &CallContext) -> Result<Output, ToolFail> {
    let call = agent_call(ctx)?;
    if app.swarm_status().is_none() {
        return Err(no_swarm());
    }
    if app.swarm_part_of(&call.id).as_deref() != Some(LEAD) {
        return Err(ToolFail::new("forbidden", "Only the lead, the agent that started the swarm, discards its draft.")
            .with_hint("Tell the lead, or ask the user to press Discard in Guhit Studio."));
    }
    if !opt_bool(args, "force")?.unwrap_or(false) {
        return Err(ToolFail::new(
            "needs_force",
            "Discarding throws away every part's work in the draft, for good.",
        ));
    }
    app.swarm_discard(None).await?;
    Ok(Output::Json(json!({
        "ok": true,
        "discarded": true,
        "note": "The draft is gone; the model is as it was before the swarm.",
    })))
}
