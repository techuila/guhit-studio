//! Agent swarms (DECISIONS D56, D57, docs/CONTRACT.md "Swarms"): one brief,
//! a lead splits it into parts, several agents draw the parts at once into
//! a draft, and a person accepts all of it, one part, or discards it.
//!
//! The draft (`draft.rs`) records each agent step and keeps a view, the
//! model with every step applied. Agents bound to a part write into it
//! through [`draft_edit`] and read it; nothing reaches the model before a
//! person accepts. While a draft is open the model is locked: every write
//! to it is refused `swarm_running` ([`check_main_write`], called from the
//! one commit path `AppService::commit_with` and the one undo path
//! `AppService::step_locked`; [`guard`] refuses the calls that replace the
//! open project).
//!
//! # Binding agents to parts
//! `swarm_start_mcp` issues a handle per part, `sw-<16 hex digits>-<key>`:
//! 64 random bits, never issued twice while the app runs. In
//! `AppService::agent_call` ([`on_call`]), an agent whose `agent` argument
//! or labelled URL equals a live handle is bound to that part and shown with
//! its name and color (not saved as a rename). The lead, the agent that
//! started the swarm, is bound to the part "lead"; a call of its connection
//! without a handle counts as the lead's. A handle of a swarm that ended is
//! refused `part_stopped`, so a late sub-agent never writes to the model.
//!
//! # Locks
//! Session, then draft, then the agents registry; never the other way. The
//! draft lock is a std mutex, never held across an await.
//!
//! # Owners
//! The contract step (C0) wrote the minimal draft below. L1 owns `draft.rs`
//! and the engine side, L2 the in-app orchestrator (`swarm_start` for the
//! app, pause, stop, cost), L3 the MCP tool. The stubs answer `not_built`
//! with their final argument shapes.

pub mod draft;
pub mod accept;
pub mod check;
pub mod cost;
pub mod lead;
pub mod orchestrate;
pub mod prompt;
pub mod worker;

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use guhit_core::{Document, Staging};
use guhit_model::*;
use serde_json::Value;

use crate::agents::{AgentCall, AgentHint, NearbyNote};
use crate::{arg, to_value, AppService, IpcResult};
use draft::{Draft, PartRec, LEAD};

/// Parts in one plan, the lead not counted.
pub const MAX_PARTS: usize = 6;
/// Longest part key.
pub const MAX_KEY_CHARS: usize = 24;
/// Steps one part may have in the draft.
pub const MAX_STEPS_PER_PART: usize = 400;
/// New elements one draft may hold.
pub const MAX_DRAFT_ELEMENTS: usize = 2000;
/// Feed lines kept, newest first.
pub const MAX_FEED: usize = 14;
/// The lead's part as people read it.
pub const LEAD_NAME: &str = "Lead";
/// Every handle starts with this.
pub const HANDLE_PREFIX: &str = "sw-";
/// Random bytes in a handle: 64 bits.
const TOKEN_BYTES: usize = 8;
/// Longest brief kept, in characters.
const MAX_BRIEF_CHARS: usize = 4000;
/// Longest part name kept.
const MAX_NAME_CHARS: usize = 40;
/// At most 8 `swarm` events a second: the time between two.
pub const EVENT_GAP: Duration = Duration::from_millis(125);

/// What a sub-agent is told, with its handle.
pub const HOW_TO: &str = "Give each sub-agent its part's handle. It passes agent: \"<handle>\" on every tool call (or connects to the part's url), draws only inside its part's levels and kinds, and checks its work with look and measure. Everything goes into a draft: nothing reaches the model until a person accepts it in the Guhit Studio window.";

// ===================================================================== state

/// The swarm of one `AppService`: at most one draft, for the open project.
#[derive(Default)]
pub struct Swarms {
    draft: Mutex<Option<Draft>>,
    /// Every handle issued since the app started: a new one never repeats
    /// one, and an old one is refused once its swarm ended.
    issued: Mutex<HashSet<String>>,
    /// The in-app swarm's workers, cost and gates (L2, `orchestrate`).
    pub(crate) runs: orchestrate::Runs,
    /// The `swarm` events' pace (`emit_swarm`).
    events: Arc<Mutex<Pace>>,
}

/// When the last `swarm` event went out, and the newest one waiting for its
/// turn.
#[derive(Default)]
struct Pace {
    last: Option<Instant>,
    waiting: Option<AppEvent>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Swarms {
    /// The draft, under its lock. Take it after the session lock, never
    /// before; never hold it across an await.
    pub(crate) fn lock(&self) -> MutexGuard<'_, Option<Draft>> {
        lock(&self.draft)
    }

    /// True while a draft is open: the model is locked.
    pub fn is_open(&self) -> bool {
        self.lock().is_some()
    }

    /// The part key an agent is bound to, while a draft is open.
    pub fn part_of(&self, agent: &str) -> Option<String> {
        self.lock().as_ref().and_then(|d| d.part_of(agent).map(str::to_string))
    }

    /// The draft view, for an agent bound to a part: what it reads in place
    /// of the model. None for every other caller.
    pub fn view_for(&self, agent: &str) -> Option<Arc<Document>> {
        self.lock().as_ref().filter(|d| d.part_of(agent).is_some()).map(|d| d.view.clone())
    }

    /// The swarm for the panel, or None.
    pub fn status(&self) -> Option<SwarmStatus> {
        self.lock().as_ref().map(Draft::status)
    }

    /// One handle per key, each new.
    pub(crate) fn issue(&self, keys: &[&str]) -> Vec<String> {
        let mut issued = lock(&self.issued);
        let rng = ring::rand::SystemRandom::new();
        keys.iter()
            .map(|key| loop {
                let mut bytes = [0u8; TOKEN_BYTES];
                ring::rand::SecureRandom::fill(&rng, &mut bytes).expect("the system random source works");
                let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                let handle = format!("{HANDLE_PREFIX}{token}-{key}");
                if issued.insert(handle.clone()) {
                    break handle;
                }
            })
            .collect()
    }

    fn was_issued(&self, handle: &str) -> bool {
        lock(&self.issued).contains(handle)
    }
}

/// A part's handle, for the agent that draws it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SwarmHandle {
    pub key: String,
    pub name: String,
    pub color: u8,
    /// "sw-1f0c9a4e7b2d3c58-shell": pass it as `agent` on every call.
    pub handle: String,
    /// The part's labelled MCP URL, for an agent in its own process.
    pub url: String,
}

/// What `swarm_start_mcp` answers.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SwarmStarted {
    pub swarm: SwarmStatus,
    /// One per plan part, in plan order. The lead needs none.
    pub handles: Vec<SwarmHandle>,
    pub how_to: String,
}

// ==================================================================== errors

/// A write to the model while a draft is open.
pub fn swarm_running() -> IpcError {
    IpcError::new("swarm_running", "A swarm draft is open: accept or discard it first.")
}

pub fn draft_open() -> IpcError {
    IpcError::new(
        "draft_open",
        "A swarm draft is open already. A person accepts or discards it in the Guhit Studio window before another swarm starts.",
    )
}

/// A swarm in a live session, or a swarm command on a live session guest.
pub fn live_session() -> IpcError {
    IpcError::new(
        "live_session",
        "Swarms do not run in a live session: the draft stays on one computer. Leave the session first.",
    )
}

pub fn part_stopped(name: &str) -> IpcError {
    IpcError::new(
        "part_stopped",
        format!("{name} was stopped in the swarm. Stop working and tell the user; its steps stay in the draft for them to accept or discard."),
    )
}

fn swarm_ended() -> IpcError {
    IpcError::new(
        "part_stopped",
        "The swarm this handle belongs to has ended: a person accepted or discarded its draft. Stop working and tell the user.",
    )
}

fn no_swarm() -> IpcError {
    IpcError::new("not_found", "No swarm draft is open.")
}

fn not_in_swarm() -> IpcError {
    IpcError::new("not_found", "This agent draws no part of the open swarm: pass the part's handle as agent, or use the part's url.")
}

// ================================================================ the lock

/// Commands that replace, close, rename or share the open project: refused
/// while a draft is open. Opening the project that is open already changes
/// nothing and passes; so does deleting or renaming another project. A
/// rename changes the model without a revision, which an accept would only
/// find out halfway.
const LOCKED_COMMANDS: &[&str] = &[
    "hub_open",
    "hub_create",
    "hub_close",
    "hub_delete",
    "hub_rename",
    "snapshot_restore",
    "bundle_open",
    "live_host",
    "live_join",
];

/// Called first in `AppService::handle`.
pub(crate) fn guard(app: &AppService, cmd: &str, args: &Value) -> Result<(), IpcError> {
    if !LOCKED_COMMANDS.contains(&cmd) {
        return Ok(());
    }
    let g = app.swarm.lock();
    let Some(d) = g.as_ref() else { return Ok(()) };
    let id = args.get("id").and_then(Value::as_str);
    match cmd {
        "hub_open" if id == Some(d.project_id.as_str()) => Ok(()),
        "hub_delete" | "hub_rename" if id != Some(d.project_id.as_str()) => Ok(()),
        _ => Err(swarm_running()),
    }
}

/// The main lock (D57): called by the one commit path and the one undo path
/// of the app service. An accept marks the draft `committing` under the
/// session lock before it commits, so its own commits pass.
pub(crate) fn check_main_write(app: &AppService) -> Result<(), IpcError> {
    // An accept commits its parts with the draft still open; it holds the
    // session lock throughout, so no other write reaches here meanwhile.
    if app.swarm.lock().as_ref().is_some_and(|d| !d.accepting()) {
        return Err(swarm_running());
    }
    Ok(())
}

// ================================================================ events

/// `AppService::emit` for `AppEvent::Swarm`: at most one every `EVENT_GAP`
/// (8 a second). An event within the gap of the last one waits for its
/// turn, a newer one taking its place, so the newest always goes out and
/// none goes out of order. Without a tokio runtime it goes out at once.
pub(crate) fn emit_swarm(app: &AppService, event: AppEvent) {
    let mut pace = lock(&app.swarm.events);
    if pace.waiting.is_some() {
        pace.waiting = Some(event);
        return;
    }
    let now = Instant::now();
    let due = pace.last.map(|t| t + EVENT_GAP).filter(|due| *due > now);
    let runtime = tokio::runtime::Handle::try_current();
    let (Some(due), Ok(runtime)) = (due, runtime) else {
        pace.last = Some(now);
        // Sent under the lock, so nothing overtakes it.
        let _ = app.events.send(event);
        return;
    };
    pace.waiting = Some(event);
    drop(pace);
    let (state, sender) = (app.swarm.events.clone(), app.events.clone());
    runtime.spawn(async move {
        tokio::time::sleep_until(due.into()).await;
        let mut pace = lock(&state);
        if let Some(event) = pace.waiting.take() {
            pace.last = Some(Instant::now());
            let _ = sender.send(event);
        }
    });
}

// ================================================================ binding

/// The hub arm in `AppService::agent_call`: bind an agent that calls with a
/// live handle to its part, show a bound agent as its part, refuse a
/// stopped part's agent and a handle of a swarm that ended.
pub(crate) fn on_call(app: &AppService, hint: &AgentHint, call: &mut AgentCall) -> Result<(), IpcError> {
    let named: Vec<String> = [hint.agent.as_deref(), hint.label.as_deref()]
        .into_iter()
        .flatten()
        .map(crate::agents::handle_of)
        .filter(|h| h.starts_with(HANDLE_PREFIX))
        .collect();
    let shown = {
        let mut g = app.swarm.lock();
        let ours = g.as_ref().is_some_and(|d| call.project_id.as_deref() == Some(d.project_id.as_str()));
        if !ours {
            drop(g);
            if named.iter().any(|h| app.swarm.was_issued(h)) {
                return Err(swarm_ended());
            }
            return Ok(());
        }
        let Some(d) = g.as_mut() else { return Ok(()) };
        let key = match d.bound.get(&call.id) {
            Some(key) => key.clone(),
            None => match d.handles.iter().find(|(h, _)| named.contains(h)) {
                Some((_, key)) => {
                    let key = key.clone();
                    d.bound.insert(call.id.clone(), key.clone());
                    key
                }
                None if named.iter().any(|h| app.swarm.was_issued(h)) => return Err(swarm_ended()),
                None => return Ok(()),
            },
        };
        let Some(rec) = d.part_mut(&key) else { return Ok(()) };
        if matches!(rec.part.state, SwarmPartState::Stopped | SwarmPartState::Accepted) {
            return Err(part_stopped(&rec.part.name));
        }
        rec.part.agent_id = Some(call.id.clone());
        (key != LEAD).then(|| (rec.part.name.clone(), rec.part.color))
    };
    if let Some((name, color)) = shown {
        app.agents.show_as(&call.id, &name, color);
        call.name = name;
    }
    Ok(())
}

// ================================================================ editing

/// The hub arm in `AppService::agent_edit` and `agent_commit`: an edit of an
/// agent bound to a part lands in the draft, never in the model. The same
/// two passes as `agent_edit`: `build` runs on the view outside the lock,
/// and again under it when the draft moved meanwhile. Checked: pause, stop
/// and the rate of the agent's connection, the person's limit and other
/// agents' claims (`check_reach`, which refuses what only people do), then
/// the part's state and scope. The step is recorded, the view rebuilt and
/// the `swarm` event published.
pub(crate) async fn draft_edit<T, E, F>(app: &AppService, call: &AgentCall, build: F) -> Result<(ApplyResult, T), E>
where
    F: Fn(&Project, &Derived) -> Result<(Staging, T), E> + Send,
    T: Send,
    E: From<IpcError> + Send,
{
    let (view, revision) = {
        let g = app.swarm.lock();
        let d = g.as_ref().ok_or_else(swarm_ended)?;
        if d.part_of(&call.id).is_none() {
            return Err(swarm_ended().into());
        }
        (d.view.clone(), d.draft_revision)
    };
    let (staging, extra) = build(view.project(), view.derived())?;
    drop(view);
    let now = app.agents.now();
    app.agents.check_edit(&call.id, now)?;
    let (result, extra, status) = commit_step(app, call, revision, staging, extra, &build)?;
    app.agents.committed(&call.id, &result, now);
    app.agents.publish();
    app.emit(AppEvent::Swarm { swarm: Some(status) });
    Ok((result, extra))
}

/// The second pass of `draft_edit`, under the draft lock.
fn commit_step<T, E, F>(
    app: &AppService,
    call: &AgentCall,
    revision: u32,
    staging: Staging,
    extra: T,
    build: &F,
) -> Result<(ApplyResult, T, SwarmStatus), E>
where
    F: Fn(&Project, &Derived) -> Result<(Staging, T), E>,
    E: From<IpcError>,
{
    let mut g = app.swarm.lock();
    let d = g.as_mut().ok_or_else(swarm_ended)?;
    let part = d.part_of(&call.id).map(str::to_string).ok_or_else(swarm_ended)?;
    if call.project_id.as_deref().is_some_and(|p| p != d.project_id) {
        return Err(IpcError::new("project_changed", "Another project was opened while this call ran, so nothing was applied.").into());
    }
    let (staging, extra) =
        if d.draft_revision == revision { (staging, extra) } else { build(d.view.project(), d.view.derived())? };
    let view = d.view.clone();
    app.agents.check_reach(&call.id, staging.base(), view.derived(), staging.project(), &staging.command())?;
    d.check_step(&part, view.derived(), &staging)?;
    // The agent's own status line goes on the step, for the reveal's cursor.
    d.note_status(&part, draft::agent_status(app, &call.id));
    let result = d.record(&part, &call.id, staging, defaults::now_rfc3339())?;
    Ok((result, extra, d.status()))
}

/// The hub arm in `AppService::agent_check` (an MCP dry run): what
/// `draft_edit` would check, for an agent bound to a part. `derived`
/// belongs to the project the staging started from.
pub(crate) fn check_draft_edit(app: &AppService, call: &AgentCall, derived: &Derived, staging: &Staging) -> Result<(), IpcError> {
    app.agents.check_edit(&call.id, app.agents.now())?;
    app.agents.check_reach(&call.id, staging.base(), derived, staging.project(), &staging.command())?;
    let g = app.swarm.lock();
    let d = g.as_ref().ok_or_else(swarm_ended)?;
    let part = d.part_of(&call.id).ok_or_else(swarm_ended)?;
    d.check_step(part, derived, staging)
}

/// The hub arm in `AppService::agent_undo` for an agent bound to a part:
/// its own newest draft step, never the model (amendments, P0-4). L1 and
/// L3 build it.
pub(crate) async fn draft_undo(app: &AppService, call: &AgentCall, redo: bool, force: bool) -> Result<DocState, IpcError> {
    accept::draft_undo(app, call, redo, force).await
}

/// The hub arm in `AppService::agent_notes` for an agent bound to a part:
/// other parts' steps on its levels and the milestones reached, in place
/// of the model's history. Empty until L2 and L3 fill it.
pub(crate) fn nearby(app: &AppService, call: &AgentCall) -> Vec<NearbyNote> {
    orchestrate::nearby(app, call)
}

// ================================================================ the plan

/// True for a part key: lowercase letters and digits in words joined by
/// single hyphens, at most `MAX_KEY_CHARS`.
fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_KEY_CHARS
        && key.split('-').all(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))
}

/// The level `wanted` names on `project`: its id, else its name.
fn find_level<'a>(project: &'a Project, wanted: &str) -> Option<&'a Level> {
    let wanted = wanted.trim();
    project
        .levels
        .iter()
        .find(|l| l.id == wanted)
        .or_else(|| project.levels.iter().find(|l| l.name.trim().eq_ignore_ascii_case(wanted)))
}

/// Check a plan against the open project before anything starts. Refused
/// (`bad_args`, saying what to change): no parts or more than 6; a key that
/// is not lowercase words joined by hyphens, "lead", or used twice; a level
/// that is neither in the project nor added by setup; an area scope (cut in
/// v1); waiting for a part or milestone the plan does not declare; parts
/// that wait for each other; no part that can start at once; two parts that
/// could draw the same kind on the same level.
pub fn validate_plan(project: &Project, plan: &SwarmPlan) -> Result<(), IpcError> {
    let bad = |m: String| Err(IpcError::new("bad_args", m));
    let n = plan.parts.len();
    if n == 0 || n > MAX_PARTS {
        return bad(format!("A swarm has 1 to {MAX_PARTS} parts besides the lead; this plan has {n}."));
    }
    let mut setup: Vec<String> = vec![];
    for level in &plan.setup.levels {
        let name = level.name.trim();
        if name.is_empty() {
            return bad("Every setup level needs a name.".into());
        }
        if level.height_mm.is_some_and(|h| !(h.is_finite() && h > 0.0)) {
            return bad(format!("Setup level {name}: height_mm must be more than 0."));
        }
        if find_level(project, name).is_some() || setup.iter().any(|s| s.eq_ignore_ascii_case(name)) {
            return bad(format!("There is a level named {name} already. Give the setup level another name, or use that one."));
        }
        setup.push(name.to_string());
    }
    // A level as one key, the project's id or the setup name, to compare.
    let level_key = |wanted: &str| -> Option<String> {
        match find_level(project, wanted) {
            Some(l) => Some(l.id.clone()),
            None => setup.iter().find(|s| s.eq_ignore_ascii_case(wanted.trim())).map(|s| format!("setup:{}", s.to_lowercase())),
        }
    };
    let mut levels: Vec<Vec<String>> = vec![];
    for (i, p) in plan.parts.iter().enumerate() {
        if !valid_key(&p.key) {
            return bad(format!(
                "Part key `{}`: use lowercase letters, digits and single hyphens, at most {MAX_KEY_CHARS} characters (\"shell\", \"upper-floor\").",
                p.key
            ));
        }
        if p.key == LEAD {
            return bad("`lead` is the lead's own part. Give this part another key.".into());
        }
        if plan.parts[..i].iter().any(|q| q.key == p.key) {
            return bad(format!("Two parts have the key `{}`. Give each part its own.", p.key));
        }
        if p.name.trim().is_empty() {
            return bad(format!("Part `{}` needs a name.", p.key));
        }
        if p.scope.area.is_some() {
            return bad(format!("Part `{}`: area scopes are not in this version. Limit parts by levels and kinds.", p.key));
        }
        let mut keys = vec![];
        for wanted in &p.scope.levels {
            match level_key(wanted) {
                Some(k) => keys.push(k),
                None => {
                    return bad(format!(
                        "Part `{}` names the level {wanted}, which is neither in the project nor added by setup.",
                        p.key
                    ))
                }
            }
        }
        levels.push(keys);
        for m in &p.milestones {
            if m.trim().is_empty() || m == "done" {
                return bad(format!("Part `{}`: name each milestone; \"done\" comes by itself when a part finishes.", p.key));
            }
        }
        for w in &p.after {
            if w.part == p.key {
                return bad(format!("Part `{}` waits for itself.", p.key));
            }
            let Some(q) = plan.parts.iter().find(|q| q.key == w.part) else {
                return bad(format!("Part `{}` waits for `{}`, which is not a part of the plan.", p.key, w.part));
            };
            if w.milestone != "done" && !q.milestones.contains(&w.milestone) {
                return bad(format!(
                    "Part `{}` waits for `{}` to reach {}, which `{}` does not declare. Use one of its milestones, or \"done\".",
                    p.key, w.part, w.milestone, w.part
                ));
            }
        }
    }
    if plan.parts.iter().all(|p| !p.after.is_empty()) {
        return bad("Every part waits for another, so none can start. Let at least one part start at once.".into());
    }
    // Waiting in a circle: a depth-first walk over `after`.
    fn cycles(plan: &SwarmPlan, at: usize, path: &mut Vec<usize>, done: &mut Vec<bool>) -> Option<String> {
        if path.contains(&at) {
            return Some(plan.parts[at].key.clone());
        }
        if done[at] {
            return None;
        }
        path.push(at);
        for w in &plan.parts[at].after {
            if let Some(next) = plan.parts.iter().position(|q| q.key == w.part) {
                if let Some(found) = cycles(plan, next, path, done) {
                    return Some(found);
                }
            }
        }
        path.pop();
        done[at] = true;
        None
    }
    let mut done = vec![false; n];
    for i in 0..n {
        if let Some(key) = cycles(plan, i, &mut vec![], &mut done) {
            return bad(format!("Parts wait for each other in a circle, through `{key}`. Make the order go one way."));
        }
    }
    for i in 0..n {
        for j in i + 1..n {
            let (p, q) = (&plan.parts[i], &plan.parts[j]);
            let share_level = levels[i].is_empty() || levels[j].is_empty() || levels[i].iter().any(|l| levels[j].contains(l));
            let share_kind = p.scope.kinds.is_empty() || q.scope.kinds.is_empty() || p.scope.kinds.iter().any(|k| q.scope.kinds.contains(k));
            if share_level && share_kind {
                return bad(format!(
                    "Parts `{}` and `{}` could both draw the same kind of element on the same level. Give each part its own levels or its own kinds (an empty list means all of them).",
                    p.key, q.key
                ));
            }
        }
    }
    Ok(())
}

/// The plan's scope levels as ids on `project` (the view after setup).
fn resolve_levels(project: &Project, plan: &SwarmPlan) -> Result<SwarmPlan, IpcError> {
    let mut plan = plan.clone();
    for part in &mut plan.parts {
        let mut ids = vec![];
        for wanted in &part.scope.levels {
            let level = find_level(project, wanted)
                .ok_or_else(|| IpcError::new("bad_args", format!("Part `{}`: there is no level {wanted}.", part.key)))?;
            if !ids.contains(&level.id) {
                ids.push(level.id.clone());
            }
        }
        part.scope.levels = ids;
    }
    Ok(plan)
}

fn clean_text(raw: &str, max: usize) -> String {
    raw.chars().filter(|c| !c.is_control() || *c == '\n').take(max).collect::<String>().trim().to_string()
}

// ======================================================= the service's API

impl AppService {
    /// Start a swarm for an MCP lead (`swarm` action `start`, lane L3): the
    /// plan is checked (`validate_plan`), the draft created on the open
    /// project, setup levels added as the lead's first step, and a handle
    /// issued per part. The caller becomes the lead. Refused: `draft_open`
    /// when a draft is open, `live_session` in a live session.
    pub async fn swarm_start_mcp(&self, call: &AgentCall, brief: &str, plan: SwarmPlan) -> Result<SwarmStarted, IpcError> {
        if !matches!(self.live.role(), crate::live::Role::Off) {
            return Err(live_session());
        }
        let owner = self.agents.owner();
        let lead = self
            .agents
            .participant(&call.id, &owner)
            .ok_or_else(|| IpcError::new("not_found", format!("No agent {} works on this computer.", call.id)))?;
        let client = lead
            .agent
            .as_ref()
            .map(|a| crate::agents::readable_client(&a.client))
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| lead.name.clone());
        let port = self.mcp.status().port;
        let started = {
            let s = self.session.lock().await;
            if s.live_copy.is_some() {
                return Err(live_session());
            }
            let doc = s.open_doc()?;
            if call.project_id.as_deref().is_some_and(|p| p != doc.project().id) {
                return Err(IpcError::new("project_changed", "Another project was opened while this call ran, so no swarm started."));
            }
            let mut g = self.swarm.lock();
            if g.is_some() {
                return Err(draft_open());
            }
            validate_plan(doc.project(), &plan)?;
            let keys: Vec<&str> = plan.parts.iter().map(|p| p.key.as_str()).collect();
            let handles = self.swarm.issue(&keys);
            let start = Start {
                project: doc.project(),
                revision: doc.revision(),
                brief: clean_text(brief, MAX_BRIEF_CHARS),
                source: SwarmSource::Mcp { client },
                lead_id: call.id.clone(),
                lead_color: lead.color,
                plan: &plan,
                handles: &handles,
            };
            let draft = start_draft(start)?;
            let swarm = draft.status();
            let handles = draft
                .handles
                .iter()
                .filter_map(|(handle, key)| {
                    draft.part(key).map(|p| SwarmHandle {
                        key: key.clone(),
                        name: p.part.name.clone(),
                        color: p.part.color,
                        handle: handle.clone(),
                        url: format!("http://127.0.0.1:{port}/mcp/agent/{handle}"),
                    })
                })
                .collect();
            *g = Some(draft);
            SwarmStarted { swarm, handles, how_to: HOW_TO.to_string() }
        };
        self.emit(AppEvent::Swarm { swarm: Some(started.swarm.clone()) });
        self.agents.publish();
        Ok(started)
    }

    /// The swarm for the panel, or None.
    pub fn swarm_status(&self) -> Option<SwarmStatus> {
        self.swarm.status()
    }

    /// The part key `agent` draws, while a draft is open.
    pub fn swarm_part_of(&self, agent: &str) -> Option<String> {
        self.swarm.part_of(agent)
    }

    /// The draft view for an agent bound to a part (what `read_view` serves
    /// it), None for everyone else.
    pub fn swarm_view(&self, agent: &str) -> Option<Arc<Document>> {
        self.swarm.view_for(agent)
    }

    /// A part's agent reports a milestone its plan declares (MCP `swarm`
    /// action `milestone`, L3; an in-app worker's `swarm_milestone`, L2).
    /// It goes on the part's list once and in the feed; parts waiting for it
    /// see it in `waiting_for`. Refused `bad_args` for a milestone the plan
    /// does not declare for the part ("done" comes with `swarm_finish`),
    /// `part_stopped` for a stopped, failed or accepted part, `not_found`
    /// without a draft or for an agent outside the swarm.
    pub fn swarm_milestone(&self, call: &AgentCall, name: &str) -> Result<SwarmStatus, IpcError> {
        let name = clean_text(name, MAX_NAME_CHARS);
        let status = {
            let mut g = self.swarm.lock();
            let d = g.as_mut().ok_or_else(no_swarm)?;
            let key = d.part_of(&call.id).map(str::to_string).ok_or_else(not_in_swarm)?;
            let declared = d.plan.parts.iter().find(|p| p.key == key).map(|p| p.milestones.clone()).unwrap_or_default();
            if !declared.contains(&name) {
                let list = if declared.is_empty() { "none".to_string() } else { declared.join(", ") };
                return Err(IpcError::new(
                    "bad_args",
                    format!("The plan gives this part no milestone `{name}`. Its milestones: {list}. \"done\" comes when the part finishes."),
                ));
            }
            let rec = d.part_mut(&key).ok_or_else(not_in_swarm)?;
            if matches!(rec.part.state, SwarmPartState::Stopped | SwarmPartState::Failed | SwarmPartState::Accepted) {
                return Err(part_stopped(&rec.part.name));
            }
            if !rec.part.milestones.contains(&name) {
                rec.part.milestones.push(name.clone());
                let text = format!("{} reached {name}", rec.part.name);
                d.feed.insert(0, SwarmFeedItem { at: defaults::now_rfc3339(), part: key, text });
                d.feed.truncate(MAX_FEED);
            }
            d.status()
        };
        self.emit(AppEvent::Swarm { swarm: Some(status.clone()) });
        Ok(status)
    }

    /// A part's agent says its part is done (MCP `swarm` action `finish`,
    /// L3; an in-app worker's `finish_part`, L2): the part is Done, with
    /// `summary` as its status line, which reaches "done" for the parts
    /// waiting on it. From the lead it also marks the draft Ready for a
    /// person to review (a paused swarm stays paused); parts still drawing
    /// go on. Refused `part_stopped` for a stopped, failed or accepted part.
    pub fn swarm_finish(&self, call: &AgentCall, summary: Option<&str>) -> Result<SwarmStatus, IpcError> {
        let status = {
            let mut g = self.swarm.lock();
            let d = g.as_mut().ok_or_else(no_swarm)?;
            let key = d.part_of(&call.id).map(str::to_string).ok_or_else(not_in_swarm)?;
            let rec = d.part_mut(&key).ok_or_else(not_in_swarm)?;
            if matches!(rec.part.state, SwarmPartState::Stopped | SwarmPartState::Failed | SwarmPartState::Accepted) {
                return Err(part_stopped(&rec.part.name));
            }
            rec.part.state = SwarmPartState::Done;
            let summary = summary.map(|s| clean_text(s, 120)).filter(|s| !s.is_empty());
            if summary.is_some() {
                rec.part.status = summary.clone();
            }
            let text = match &summary {
                Some(s) => format!("{} finished: {s}", rec.part.name),
                None => format!("{} finished", rec.part.name),
            };
            d.feed.insert(0, SwarmFeedItem { at: defaults::now_rfc3339(), part: key.clone(), text });
            d.feed.truncate(MAX_FEED);
            if key == LEAD && d.state == SwarmState::Running {
                d.state = SwarmState::Ready;
            }
            d.status()
        };
        self.emit(AppEvent::Swarm { swarm: Some(status.clone()) });
        self.agents.publish();
        Ok(status)
    }

    /// What the window draws: the view and its diff against the model, with
    /// the steps after `since_step`. None without a draft.
    pub async fn swarm_draft(&self, since_step: Option<u32>) -> Result<Option<SwarmDraftState>, IpcError> {
        let s = self.session.lock().await;
        let g = self.swarm.lock();
        let Some(d) = g.as_ref() else { return Ok(None) };
        let main = s.open_doc()?.project();
        Ok(Some(d.draft_state(main, since_step.unwrap_or(0))))
    }

    /// `swarm_draft` for a window that has drawn `draft_revision` of swarm
    /// `swarm_id` (any swarm when None): only the status when the draft has
    /// not moved since, else the whole draft as `swarm_draft` gives it.
    pub async fn swarm_draft_since(
        &self,
        since_step: Option<u32>,
        draft_revision: u32,
        swarm_id: Option<&str>,
    ) -> Result<Option<SwarmDraftReply>, IpcError> {
        let s = self.session.lock().await;
        let g = self.swarm.lock();
        let Some(d) = g.as_ref() else { return Ok(None) };
        if d.draft_revision == draft_revision && swarm_id.is_none_or(|id| id == d.id) {
            return Ok(Some(SwarmDraftReply::Current(SwarmDraftCurrent {
                swarm_id: d.id.clone(),
                draft_revision,
                status: d.status(),
            })));
        }
        let main = s.open_doc()?.project();
        Ok(Some(SwarmDraftReply::Full(d.draft_state(main, since_step.unwrap_or(0)))))
    }

    /// Accept all (D57): every step replayed in order on the model, which
    /// equals the base because the model is locked, so the ids are the
    /// draft's; each part committed as one undo step, in the order of its
    /// first step, with its agent as author, `Origin::Ai` and the label
    /// "Swarm: <part>". A version "Before swarm, 3:42 PM" is saved first
    /// (nothing is accepted when it cannot be). The swarm then ends.
    /// `reviewed` pins it: refused `stale` when a part has a step after the
    /// `last_seq` the person reviewed (a part left out counts as 0).
    pub async fn swarm_accept_all(
        &self,
        swarm_id: &str,
        reviewed: Option<&BTreeMap<String, u32>>,
    ) -> Result<SwarmAcceptResult, IpcError> {
        // L1's pinned accept, with no parts: every part, in dependency order.
        self.swarm_accept(swarm_id, None, reviewed).await
    }

    /// Discard the draft and unlock the model. `swarm_id` None discards
    /// whatever draft is open (an MCP lead with `force`, lane L3). It takes
    /// the session lock first, so it waits for an accept that is committing
    /// and then answers from the draft as it is: `not_found` when that
    /// accept ended the swarm, never "discarded" for parts that went in.
    pub async fn swarm_discard(&self, swarm_id: Option<&str>) -> Result<(), IpcError> {
        let _s = self.session.lock().await;
        {
            let mut g = self.swarm.lock();
            let d = g.as_ref().ok_or_else(no_swarm)?;
            if swarm_id.is_some_and(|id| id != d.id) {
                return Err(IpcError::new("not_found", format!("Swarm {} is not the open draft.", swarm_id.unwrap_or_default())));
            }
            *g = None;
        }
        self.emit(AppEvent::Swarm { swarm: None });
        self.agents.publish();
        Ok(())
    }
}

/// What a new draft is made of. L2 starts an in-app swarm with it too
/// (`source` `App`, its workers bound through `Draft::bound`).
pub(crate) struct Start<'a> {
    pub project: &'a Project,
    pub revision: u32,
    pub brief: String,
    pub source: SwarmSource,
    pub lead_id: Id,
    pub lead_color: u8,
    pub plan: &'a SwarmPlan,
    /// One per plan part, in order (`Swarms::issue`).
    pub handles: &'a [String],
}

/// A new draft on `project`: the lead's part, then a part per plan part with
/// its own color, the setup levels as the lead's first step, and the scope
/// levels resolved to ids.
pub(crate) fn start_draft(st: Start) -> Result<Draft, IpcError> {
    let lead_color = st.lead_color;
    let mut colors = (1..PEER_COLOR_COUNT).filter(move |c| *c != lead_color).cycle();
    let part = |key: &str, name: &str, role: &str, color: u8, state: SwarmPartState| SwarmPart {
        key: key.to_string(),
        name: name.to_string(),
        role: role.to_string(),
        color,
        agent_id: None,
        state,
        status: None,
        steps: 0,
        milestones: vec![],
        waiting_for: vec![],
        conflicts: 0,
        last_seq: 0,
    };
    let mut lead = part(LEAD, LEAD_NAME, "Splits the brief and adds the levels", st.lead_color, SwarmPartState::Working);
    lead.agent_id = Some(st.lead_id.clone());
    let mut parts = vec![PartRec { part: lead, scope: SwarmScope::default(), after: vec![] }];
    for p in &st.plan.parts {
        let state = if p.after.is_empty() { SwarmPartState::Working } else { SwarmPartState::Waiting };
        let name = clean_text(&p.name, MAX_NAME_CHARS);
        let role = clean_text(&p.role, 120);
        parts.push(PartRec {
            part: part(&p.key, &name, &role, colors.next().unwrap_or(1), state),
            scope: p.scope.clone(),
            after: p.after.clone(),
        });
    }
    let mut bound = BTreeMap::new();
    bound.insert(st.lead_id.clone(), LEAD.to_string());
    let mut draft = Draft {
        id: defaults::new_id(),
        project_id: st.project.id.clone(),
        source: st.source,
        state: SwarmState::Running,
        brief: st.brief,
        plan: st.plan.clone(),
        parts,
        handles: st.handles.iter().cloned().zip(st.plan.parts.iter().map(|p| p.key.clone())).collect(),
        bound,
        base: st.project.clone(),
        base_revision: st.revision,
        steps: vec![],
        view: Arc::new(Document::with_revision(st.project.clone(), st.revision)),
        draft_revision: 0,
        seq: 0,
        feed: vec![],
        counts: (0, 0, 0),
        ..Default::default()
    };
    if !st.plan.setup.levels.is_empty() {
        let commands: Vec<Command> = st
            .plan
            .setup
            .levels
            .iter()
            .map(|l| Command::AddLevel { name: Some(l.name.trim().to_string()), elevation_mm: None, height_mm: l.height_mm })
            .collect();
        let staging = Staging::rebuilt(draft.view.project(), "Swarm setup: add levels", &commands).map_err(IpcError::from)?;
        draft.record(LEAD, &st.lead_id, staging, defaults::now_rfc3339())?;
    }
    let plan = resolve_levels(draft.view.project(), st.plan)?;
    for (rec, p) in draft.parts.iter_mut().skip(1).zip(&plan.parts) {
        rec.scope = p.scope.clone();
    }
    draft.plan = plan;
    Ok(draft)
}

// ===================================================================== IPC

/// Commands this module answers. `AppService::handle` routes on this list;
/// a live session guest is refused every one (`live_session`).
pub const OWNS: &[&str] = &[
    "swarm_estimate",
    "swarm_start",
    "swarm_status",
    "swarm_draft",
    "swarm_pause",
    "swarm_resume",
    "swarm_stop",
    "swarm_part_discard",
    "swarm_check",
    "swarm_accept",
    "swarm_discard",
    "swarm_set_cap",
];

pub async fn handle(app: &AppService, cmd: &str, args: Value) -> IpcResult {
    match cmd {
        "swarm_status" => to_value(&app.swarm_status()),
        "swarm_draft" => {
            let since_step: Option<u32> = arg(&args, "since_step")?;
            let draft_revision: Option<u32> = arg(&args, "draft_revision")?;
            let swarm_id: Option<Id> = arg(&args, "swarm_id")?;
            match draft_revision {
                Some(r) => to_value(&app.swarm_draft_since(since_step, r, swarm_id.as_deref()).await?),
                None => to_value(&app.swarm_draft(since_step).await?),
            }
        }
        "swarm_accept" => {
            let swarm_id: Id = arg(&args, "swarm_id")?;
            let parts: Option<Vec<String>> = arg(&args, "parts")?;
            let reviewed: Option<BTreeMap<String, u32>> = arg(&args, "reviewed")?;
            let accept_rest: Option<bool> = arg(&args, "accept_rest")?;
            to_value(&app.swarm_accept_with(&swarm_id, parts.as_deref(), reviewed.as_ref(), accept_rest.unwrap_or(false)).await?)
        }
        "swarm_discard" => {
            let swarm_id: Id = arg(&args, "swarm_id")?;
            app.swarm_discard(Some(&swarm_id)).await?;
            Ok(Value::Null)
        }
        // Lane L2, the in-app orchestrator.
        "swarm_estimate" | "swarm_start" | "swarm_pause" | "swarm_resume" | "swarm_stop" | "swarm_set_cap" => {
            orchestrate::handle(app, cmd, args).await
        }
        // Lane L1, the engine and draft.
        "swarm_part_discard" => {
            let part: String = arg(&args, "part")?;
            let with_dependents: Option<bool> = arg(&args, "with_dependents")?;
            to_value(&app.swarm_part_discard(&part, with_dependents.unwrap_or(false)).await?)
        }
        "swarm_check" => {
            let parts: Option<Vec<String>> = arg(&args, "parts")?;
            to_value(&app.swarm_check(parts.as_deref()).await?)
        }
        _ => Err(IpcError::new("unknown_command", format!("unknown command `{cmd}`"))),
    }
}

#[cfg(test)]
mod tests;
