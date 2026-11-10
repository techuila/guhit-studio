//! One in-app worker (D56, amendments P1-7): its own loop, its own system
//! prompt, its own conversation. `ai::chat` and its proposal path (D8) are
//! not used and not changed.
//!
//! A worker is a registered agent on its own connection,
//! `swarm:<id>:<key>`, calling with its part's handle, so it is bound to
//! the part (`swarm::on_call`) and shown with the part's name and color;
//! its cursor, status line, the Agents panel's pause and stop all work as
//! for any agent. It waits until what its part waits for is reached, then
//! runs at most `MAX_WORKER_ROUNDS` rounds. Its tools are the copilot's
//! read and edit tools plus `swarm_milestone`, `set_status` and
//! `finish_part`. Reads answer from the draft view; edits go through
//! `agent_edit`, which lands them in the draft as the part's agent
//! (`swarm::draft_edit`, with every check of a draft edit). Each round's
//! tool results end with a short note of what other parts did nearby.

use std::collections::HashSet;
use std::sync::Arc;

use guhit_core::Staging;
use guhit_model::*;
use serde_json::{json, Value};

use super::draft::{describe_scope, PartRec};
use super::orchestrate::{ended, hint, idle, notes_since, reached, update, Halt, Run};
use super::prompt::{self, OtherPart, WorkerBrief};
use crate::agents::{AgentCall, AgentHint, CallKind};
use crate::ai::client::{self, ModelRequest, ToolUse};
use crate::ai::tools::{self, ToolError};
use crate::AppService;

/// Model round trips a worker may make.
pub const MAX_WORKER_ROUNDS: usize = 24;
const WORKER_MAX_TOKENS: u32 = 8192;
/// Ids listed per step in a nearby note.
const NOTE_IDS: usize = 8;
/// A waiting part's poll, besides the ticks.
const WAIT_POLL: std::time::Duration = std::time::Duration::from_millis(250);

/// How a worker's work ended.
enum End {
    /// `finish_part`, or the model ended its turn: the part is Done.
    Finished,
    /// A person stopped it, or the swarm ended.
    Ended,
    Failed(String),
    /// What it waited for ended first (v1: a blocked part stops).
    Blocked(String),
    OutOfRounds,
}

/// What a tool call means for the loop.
enum Flow {
    Go,
    Finished,
    Stop,
}

/// An edit that failed: the tool's own words, or the app's refusal.
enum EditFail {
    Tool(ToolError),
    Ipc(IpcError),
}

impl From<IpcError> for EditFail {
    fn from(e: IpcError) -> Self {
        Self::Ipc(e)
    }
}

impl EditFail {
    fn text(&self) -> String {
        match self {
            Self::Tool(e) => e.0.clone(),
            Self::Ipc(e) => format!("{}: {}", e.code, e.message),
        }
    }
}

/// What a worker keeps between rounds.
struct Ctx {
    swarm_id: Id,
    hint: AgentHint,
    /// The level new elements go on when a call names none.
    level_id: Id,
    /// What this part added, which stays in reach of "Only the selection".
    made: Vec<Id>,
    /// The newest step `seq` the nearby notes covered.
    cursor: u32,
    /// Milestones and finishes of other parts it was told of.
    told: HashSet<(String, String)>,
}

/// The worker's task. It always leaves its part ended.
pub(crate) async fn run(app: AppService, run: Arc<Run>, key: String, handle: String) {
    let end = work(&app, &run, &key, &handle).await;
    let (state, why) = match &end {
        End::Finished => (None, None),
        End::Ended => (Some(SwarmPartState::Stopped), Some("Stopped".to_string())),
        End::Failed(why) => (Some(SwarmPartState::Failed), Some(why.clone())),
        End::Blocked(why) => (Some(SwarmPartState::Blocked), Some(why.clone())),
        End::OutOfRounds => (Some(SwarmPartState::Done), Some(format!("Stopped after {MAX_WORKER_ROUNDS} rounds"))),
    };
    if let Some(state) = state {
        update(&app, &run.swarm_id, |d| {
            let declared = d.plan.parts.iter().find(|p| p.key == key).map(|p| p.milestones.clone()).unwrap_or_default();
            let Some(rec) = d.part_mut(&key) else { return };
            if ended(rec.part.state) {
                return;
            }
            rec.part.state = state;
            rec.part.status = why.clone();
            if state == SwarmPartState::Done {
                for m in declared {
                    if !rec.part.milestones.contains(&m) {
                        rec.part.milestones.push(m);
                    }
                }
            }
            let text = match state {
                SwarmPartState::Done => format!("{}: {}", rec.part.name, why.clone().unwrap_or_default()),
                SwarmPartState::Stopped => format!("{} stopped", rec.part.name),
                _ => format!("{}: {}", rec.part.name, why.clone().unwrap_or_default()),
            };
            super::orchestrate::feed(d, &key, text);
        });
    }
    if let End::Failed(why) | End::Blocked(why) = &end {
        run.log(&app, "swarm_part_end", json!({"part": key, "why": why})).await;
    }
    run.part_ended(&app, &key);
}

async fn work(app: &AppService, run: &Run, key: &str, handle: &str) -> End {
    let hint = hint(&run.swarm_id, key, Some(handle));
    // The first call binds the agent to the part.
    let call = match app.agent_call(&hint, "swarm", CallKind::Read) {
        Ok(c) => c,
        Err(e) => return End::Failed(e.message),
    };
    if let Some(end) = wait_for_turn(app, run, key).await {
        return end;
    }
    let Some((first, mut ctx)) = first_message(app, run, key, hint) else { return End::Ended };
    let client = match client::client_for(app, &first) {
        Ok(c) => c,
        Err(e) => return End::Failed(e.message),
    };
    let mut request = ModelRequest {
        model: run.model.clone(),
        system: prompt::WORKER_SYSTEM.to_string(),
        messages: vec![json!({"role": "user", "content": first})],
        tools: prompt::worker_tools(),
        max_tokens: WORKER_MAX_TOKENS,
    };
    for round in 1..=MAX_WORKER_ROUNDS {
        let answer = match run.send(app, key, Some(&call.id), client.as_ref(), &request, round).await {
            Ok(m) => m,
            Err(Halt::Ended) => return End::Ended,
            Err(Halt::Failed(why)) => return End::Failed(why),
        };
        let response = answer.response;
        // A stop ends the part after its current request; a pause holds it
        // here, before its tools run.
        if !hold_while_paused(app, run, key).await {
            return End::Ended;
        }
        if response.stop_reason == "refusal" {
            return End::Failed("The AI service declined this part.".into());
        }
        let calls = response.tool_uses();
        if calls.is_empty() {
            // It ended its turn without finish_part: the part is done.
            let text = response.text();
            let summary = text.lines().next().map(str::trim).filter(|l| !l.is_empty());
            return match finish(app, &ctx, summary) {
                Ok(()) => End::Finished,
                Err(_) => End::Ended,
            };
        }
        let mut results: Vec<Value> = vec![];
        let mut flow = Flow::Go;
        for c in &calls {
            let (content, is_error, this) = tool(app, run, &mut ctx, c).await;
            let mut block = json!({"type": "tool_result", "tool_use_id": c.id, "content": content});
            if is_error {
                block["is_error"] = json!(true);
            }
            results.push(block);
            match this {
                Flow::Go => {}
                Flow::Finished => flow = Flow::Finished,
                Flow::Stop if !matches!(flow, Flow::Finished) => flow = Flow::Stop,
                Flow::Stop => {}
            }
        }
        match flow {
            Flow::Finished => return End::Finished,
            Flow::Stop => return End::Ended,
            Flow::Go => {}
        }
        if response.stop_reason == "max_tokens" {
            results.push(json!({"type": "text", "text": "Your last answer was cut off at the token limit. Make smaller calls."}));
        }
        if let Some(note) = nearby_note(app, run, key, &mut ctx) {
            results.push(json!({"type": "text", "text": note}));
        }
        // The answer as it came (thinking and tool_use blocks unchanged),
        // then every result in one user message.
        request.messages.push(json!({"role": "assistant", "content": response.content}));
        request.messages.push(json!({"role": "user", "content": results}));
    }
    End::OutOfRounds
}

enum Turn {
    Go,
    Wait(String),
    Blocked(String),
}

/// Wait until every milestone the part waits for is reached. None to go
/// on; else how the part ended while it waited.
async fn wait_for_turn(app: &AppService, run: &Run, key: &str) -> Option<End> {
    let mut rx = run.subscribe();
    let mut shown: Option<String> = None;
    loop {
        rx.borrow_and_update();
        let turn = {
            let g = app.swarm.lock();
            let Some(d) = g.as_ref().filter(|d| d.id == run.swarm_id) else { return Some(End::Ended) };
            let Some(rec) = d.part(key) else { return Some(End::Ended) };
            if ended(rec.part.state) {
                return Some(End::Ended);
            }
            let open: Vec<&SwarmWait> = rec.after.iter().filter(|w| !reached(d, w)).collect();
            let name_of = |k: &str| d.part(k).map(|p| p.part.name.clone()).unwrap_or_else(|| k.to_string());
            match open.iter().find(|w| d.part(&w.part).is_some_and(|p| ended(p.part.state))) {
                Some(w) => Turn::Blocked(format!("{} ended before {}, which this part waited for", name_of(&w.part), w.milestone)),
                None if open.is_empty() => Turn::Go,
                None => Turn::Wait(
                    open.iter().map(|w| format!("{}: {}", name_of(&w.part), w.milestone)).collect::<Vec<_>>().join(", "),
                ),
            }
        };
        match turn {
            Turn::Go => {
                update(app, &run.swarm_id, |d| {
                    if let Some(rec) = d.part_mut(key) {
                        if rec.part.state == SwarmPartState::Waiting {
                            rec.part.state = SwarmPartState::Working;
                        }
                        rec.part.status = Some("Starting".into());
                    }
                });
                return None;
            }
            Turn::Blocked(why) => return Some(End::Blocked(why)),
            Turn::Wait(what) => {
                if shown.as_ref() != Some(&what) {
                    let line = format!("Waiting for {what}");
                    update(app, &run.swarm_id, |d| {
                        if let Some(rec) = d.part_mut(key) {
                            rec.part.status = Some(line);
                        }
                    });
                    shown = Some(what);
                }
                idle(&mut rx, WAIT_POLL).await;
            }
        }
    }
}

/// Hold while the part or the swarm is paused. False once the part or the
/// swarm ended.
async fn hold_while_paused(app: &AppService, run: &Run, key: &str) -> bool {
    let mut rx = run.subscribe();
    loop {
        rx.borrow_and_update();
        let paused = {
            let g = app.swarm.lock();
            let Some(d) = g.as_ref().filter(|d| d.id == run.swarm_id) else { return false };
            let Some(rec) = d.part(key) else { return false };
            if ended(rec.part.state) {
                return false;
            }
            rec.part.state == SwarmPartState::Paused || d.state == SwarmState::Paused
        };
        if !paused {
            return true;
        }
        idle(&mut rx, WAIT_POLL).await;
    }
}

/// The part's first message, from the draft as it is when the part starts,
/// and the worker's state. None when the swarm is gone.
fn first_message(app: &AppService, run: &Run, key: &str, hint: AgentHint) -> Option<(String, Ctx)> {
    let (view, rec, planned, summary, others, cursor, told) = {
        let g = app.swarm.lock();
        let d = g.as_ref().filter(|d| d.id == run.swarm_id)?;
        let rec: PartRec = d.part(key)?.clone();
        let planned = d.plan.parts.iter().find(|p| p.key == key).cloned()?;
        let project = d.view.project();
        let others: Vec<OtherPart> = d
            .parts
            .iter()
            .skip(1)
            .filter(|p| p.part.key != key)
            .map(|p| {
                let after: Vec<String> = p
                    .after
                    .iter()
                    .map(|w| {
                        let name = d.part(&w.part).map(|q| q.part.name.clone()).unwrap_or_else(|| w.part.clone());
                        if w.milestone == "done" { format!("{name} is done") } else { format!("{name} reaches {}", w.milestone) }
                    })
                    .collect();
                OtherPart {
                    name: p.part.name.clone(),
                    key: p.part.key.clone(),
                    role: p.part.role.clone(),
                    scope: describe_scope(project, &p.scope),
                    after: if after.is_empty() { String::new() } else { format!(" Starts after {}.", after.join(" and ")) },
                }
            })
            .collect();
        let mut told = HashSet::new();
        for p in d.parts.iter().skip(1) {
            for m in &p.part.milestones {
                told.insert((p.part.key.clone(), m.clone()));
            }
            if ended(p.part.state) {
                told.insert((p.part.key.clone(), "done".to_string()));
            }
        }
        (d.view.clone(), rec, planned, d.plan.summary.clone(), others, d.seq, told)
    };
    let project = view.project();
    let level_id = rec
        .scope
        .levels
        .first()
        .cloned()
        .or_else(|| run.active_level_id.clone().filter(|id| project.levels.iter().any(|l| &l.id == id)))
        .or_else(|| project.levels.first().map(|l| l.id.clone()))
        .unwrap_or_default();
    // The scope line of "Only the selection" comes with the context.
    let context = crate::ai::prompt::project_context(&view, &[], run.scope.as_deref());
    let scope = describe_scope(project, &rec.scope);
    let message = prompt::worker_message(&WorkerBrief {
        key,
        name: &rec.part.name,
        role: &rec.part.role,
        scope: &scope,
        milestones: &planned.milestones,
        instructions: &planned.instructions,
        brief: &run.brief,
        summary: &summary,
        others: &others,
        scope_line: None,
        context: &context,
    });
    let ctx = Ctx { swarm_id: run.swarm_id.clone(), hint, level_id, made: vec![], cursor, told };
    Some((message, ctx))
}

/// The refusal of a call as the model reads it, and whether the part
/// should stop.
fn refused(e: &IpcError) -> (String, bool, Flow) {
    let flow = if matches!(e.code.as_str(), "part_stopped" | "agent_stopped") { Flow::Stop } else { Flow::Go };
    (format!("{}: {}", e.code, e.message), true, flow)
}

/// Run one tool call. The content, whether it is an error, and what it
/// means for the loop.
async fn tool(app: &AppService, run: &Run, ctx: &mut Ctx, c: &ToolUse) -> (String, bool, Flow) {
    let text = |v: &Value, field: &str| v.get(field).and_then(Value::as_str).unwrap_or_default().trim().to_string();
    match c.name.as_str() {
        prompt::MILESTONE_TOOL => {
            let call = match app.agent_call(&ctx.hint, &c.name, CallKind::Read) {
                Ok(call) => call,
                Err(e) => return refused(&e),
            };
            let name = text(&c.input, "name");
            match app.swarm_milestone(&call, &name) {
                Ok(_) => {
                    run.poke();
                    (json!({"reached": name}).to_string(), false, Flow::Go)
                }
                Err(e) => refused(&e),
            }
        }
        prompt::STATUS_TOOL => {
            let call = match app.agent_call(&ctx.hint, &c.name, CallKind::Read) {
                Ok(call) => call,
                Err(e) => return refused(&e),
            };
            let line: String = text(&c.input, "text").chars().filter(|c| !c.is_control()).take(80).collect();
            app.agent_set_status(&call, Some(&line));
            let part = app.swarm_part_of(&call.id);
            update(app, &run.swarm_id, |d| {
                if let Some(rec) = part.as_deref().and_then(|p| d.part_mut(p)) {
                    rec.part.status = (!line.is_empty()).then(|| line.clone());
                }
            });
            (json!({"status": line}).to_string(), false, Flow::Go)
        }
        prompt::FINISH_TOOL => {
            let summary = text(&c.input, "summary");
            match finish(app, ctx, (!summary.is_empty()).then_some(summary.as_str())) {
                Ok(()) => (json!({"finished": true}).to_string(), false, Flow::Finished),
                Err(e) => refused(&e),
            }
        }
        name if tools::is_read(name) => read(app, ctx, c).await,
        name if tools::is_edit(name) => edit(app, run, ctx, c).await,
        other => (format!("Unknown tool `{other}`."), true, Flow::Go),
    }
}

/// The part is done: `swarm_finish` as its agent, its declared milestones
/// all reached.
fn finish(app: &AppService, ctx: &Ctx, summary: Option<&str>) -> Result<(), IpcError> {
    let call = app.agent_call(&ctx.hint, prompt::FINISH_TOOL, CallKind::Read)?;
    app.swarm_finish(&call, summary)?;
    let Some(key) = app.swarm_part_of(&call.id) else { return Ok(()) };
    update(app, &ctx.swarm_id, |d| {
        let declared = d.plan.parts.iter().find(|p| p.key == key).map(|p| p.milestones.clone()).unwrap_or_default();
        if let Some(rec) = d.part_mut(&key) {
            for m in declared {
                if !rec.part.milestones.contains(&m) {
                    rec.part.milestones.push(m);
                }
            }
        }
    });
    Ok(())
}

/// A read tool on the draft view.
async fn read(app: &AppService, ctx: &Ctx, c: &ToolUse) -> (String, bool, Flow) {
    let call = match app.agent_call(&ctx.hint, &c.name, CallKind::Read) {
        Ok(call) => call,
        Err(e) => return refused(&e),
    };
    let Some(view) = app.swarm_view(&call.id) else {
        return ("The swarm's draft is gone: stop.".into(), true, Flow::Stop);
    };
    match tools::run_read(&view, &c.name, &c.input) {
        Ok(data) => {
            let ids: Vec<Id> = c
                .input
                .get("ids")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            app.agent_done(&call, &ids).await;
            let out = json!({"view": "the swarm's draft: the model with every part's work so far", "data": data});
            (out.to_string(), false, Flow::Go)
        }
        Err(e) => (e.0, true, Flow::Go),
    }
}

/// An edit tool: translated on the draft view and drawn into the draft as
/// the part's agent (`agent_edit`, which routes a bound agent to
/// `swarm::draft_edit`). With "Only the selection" it must stay inside the
/// selection and what this part made, as the copilot's turns do (D30).
async fn edit(app: &AppService, run: &Run, ctx: &mut Ctx, c: &ToolUse) -> (String, bool, Flow) {
    let call: AgentCall = match app.agent_call(&ctx.hint, &c.name, CallKind::Edit) {
        Ok(call) => call,
        Err(e) => return refused(&e),
    };
    // The feed puts the part's name in front: "Shell: add wall chain: added 4 walls".
    let label = c.name.replace('_', " ");
    let (name, input, level, made, scope) = (c.name.clone(), c.input.clone(), ctx.level_id.clone(), ctx.made.clone(), run.scope.clone());
    let build = move |project: &Project, derived: &Derived| -> Result<(Staging, Value), EditFail> {
        let commands = tools::to_commands(project, &name, &input, &level).map_err(EditFail::Tool)?;
        if let Some(selection) = &scope {
            let one = match commands.as_slice() {
                [one] => one.clone(),
                many => Command::Batch { label: label.clone(), commands: many.to_vec() },
            };
            guhit_core::scope::check_staged(project, derived, selection, &made, &one).map_err(|e| {
                let ToolError(text) = e.into();
                EditFail::Tool(ToolError(format!("{text} The swarm is limited to the user's selection: draw only there.")))
            })?;
        }
        let staging = Staging::rebuilt(project, &label, &commands).map_err(|e| EditFail::Tool(e.into()))?;
        let step = tools::step_diff(project, staging.project());
        Ok((staging, step))
    };
    let mut tried = false;
    loop {
        match app.agent_edit(&call, &build).await {
            Ok((result, step)) => {
                ctx.made.extend(result.diff.added.iter().cloned());
                let out = json!({"drawn": true, "this_step": step, "draft_step": result.diff.summary});
                return (out.to_string(), false, Flow::Go);
            }
            // Five steps a second per agent: wait and try once more.
            Err(EditFail::Ipc(e)) if e.code == "rate_limited" && !tried => {
                tried = true;
                tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
            }
            Err(EditFail::Ipc(e)) => return refused(&e),
            Err(e) => return (e.text(), true, Flow::Go),
        }
    }
}

/// What other parts did since the last note: their steps on this part's
/// levels and the milestones they reached. None with nothing new.
fn nearby_note(app: &AppService, run: &Run, key: &str, ctx: &mut Ctx) -> Option<String> {
    let mut lines: Vec<String> = vec![];
    {
        let g = app.swarm.lock();
        let d = g.as_ref().filter(|d| d.id == run.swarm_id)?;
        let (notes, newest) = notes_since(d, key, ctx.cursor);
        ctx.cursor = newest;
        for n in notes {
            let mut ids: Vec<&str> = n.ids.iter().map(String::as_str).take(NOTE_IDS).collect();
            if n.ids.len() > NOTE_IDS {
                ids.push("...");
            }
            lines.push(format!("{} drew \"{}\" ({}).", n.who, n.what, ids.join(", ")));
        }
        for p in d.parts.iter().skip(1).filter(|p| p.part.key != key) {
            for m in &p.part.milestones {
                if ctx.told.insert((p.part.key.clone(), m.clone())) {
                    lines.push(format!("{} reached {m}.", p.part.name));
                }
            }
            if ended(p.part.state) && ctx.told.insert((p.part.key.clone(), "done".into())) {
                let how = match p.part.state {
                    SwarmPartState::Done => "finished",
                    SwarmPartState::Stopped => "was stopped",
                    SwarmPartState::Failed => "failed",
                    SwarmPartState::Blocked => "is blocked",
                    _ => "was accepted",
                };
                lines.push(format!("{} {how}.", p.part.name));
            }
        }
    }
    prompt::nearby_text(&lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_worker_draws_at_most_24_rounds() {
        assert_eq!(MAX_WORKER_ROUNDS, 24);
    }

    #[test]
    fn edit_failures_read_as_the_tool_or_the_app_said() {
        assert_eq!(EditFail::Tool(ToolError("invalid arguments: x".into())).text(), "invalid arguments: x");
        assert_eq!(EditFail::Ipc(IpcError::new("out_of_scope", "No.")).text(), "out_of_scope: No.");
        let (_, is_error, flow) = refused(&IpcError::new("part_stopped", "Stop."));
        assert!(is_error && matches!(flow, Flow::Stop));
    }
}
