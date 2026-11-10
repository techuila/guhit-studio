//! The in-app orchestrator (D56, plan.md section 2, amendments "Workers and
//! cost"): Guhit's own swarm on the person's Claude key.
//!
//! `swarm_start` checks the request, registers the lead as an agent and
//! opens a planning draft at once (the model is locked from then on), then
//! a task asks the lead for a plan (`lead`), puts the plan in the draft
//! with its setup levels as the lead's first step (`start_draft`, same
//! swarm id), and starts one worker per part (`worker`). Workers wait for
//! the milestones their part waits for (`Run::tick`, a `tokio::sync::watch`
//! the workers and the window's calls bump), draw into the draft as their
//! part's agent, and end with `finish_part`, a stop, or a failure. When
//! every part ended the draft is Ready for a person.
//!
//! # Money and the AI service
//! Every request of the lead and the workers goes through [`Run::send`]:
//! - nothing goes out while the swarm or the part is paused (by a person,
//!   or the Agents panel), so a paused swarm spends nothing;
//! - the cap: before each request, when what was spent, what requests in
//!   flight may cost and what this one may cost would pass the cap, every
//!   part pauses ("Raise cap and continue", `swarm_set_cap`, or Stop);
//! - the gate in `AiState`: at most `client::MAX_IN_FLIGHT` in flight;
//! - the first worker's request goes out alone, the others once it
//!   answered, so they read the shared prefix from the cache instead of
//!   each paying to write it;
//! - a 429 or an overload waits that part only, for the `retry-after` the
//!   API gave, and shows `SwarmStatus::ai_wait_until`;
//! - the spend is the API's `usage` at the price of the model that answered
//!   (`cost`), counted into `SwarmCost`, and each round is a line of
//!   `ai-log.jsonl`.
//!
//! # Locks
//! Session, then the draft (`app.swarm.lock()`), then a run's own state or
//! the agents registry, which are leaves. The draft lock is never held
//! across an await, nor while calling into the agents (`agent_call` takes
//! the draft lock itself).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use guhit_model::*;
use serde_json::{json, Value};
use tokio::sync::watch;

use super::cost::{self, Price};
use super::draft::{Draft, LEAD};
use super::{lead, prompt, start_draft, validate_plan, worker, Start, MAX_FEED};
use crate::agents::{AgentCall, AgentHint, CallKind, NearbyNote, MAX_AGENTS, MAX_NEARBY};
use crate::ai::client::{self, Metered, ModelClient, ModelRequest};
use crate::ai::log;
use crate::{arg, to_value, AppService, IpcResult};

/// The workers' model when the person chose none in the copilot settings:
/// the claude-api skill's default.
pub const DEFAULT_SWARM_MODEL: &str = "claude-opus-5-5";
/// How often a waiting worker looks again without a tick: catches what
/// the window's other calls change (accept, discard) and the Agents panel.
const POLL: Duration = Duration::from_millis(250);
/// Rate limits or overloads in a row before a part fails.
const MAX_WAITS: u32 = 8;
/// Waits without a `retry-after`: 2 s, doubling, at most 30 s.
const FIRST_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// Longest brief kept, in characters.
const MAX_BRIEF_CHARS: usize = 4000;
/// Tokens a request is assumed to write back, for the cap.
const OUTPUT_GUESS: u64 = 2000;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ===================================================================== state

/// The orchestrator's state of one `AppService` (`Swarms::runs`).
#[derive(Default)]
pub struct Runs {
    /// The in-app swarm started last.
    current: Mutex<Option<Arc<Run>>>,
    /// For MCP sub-agents' notes: per (swarm, agent), the newest step
    /// `seq` they heard of.
    told: Mutex<HashMap<(Id, Id), u32>>,
}

impl Runs {
    fn get(&self, swarm_id: &str) -> Option<Arc<Run>> {
        lock(&self.current).as_ref().filter(|r| r.swarm_id == swarm_id).cloned()
    }
}

/// One in-app swarm while it runs, shared by its lead and workers.
pub struct Run {
    pub swarm_id: Id,
    /// The model every request names.
    pub model: String,
    /// Its price, else the table's highest row (`cost::billed`): the cap
    /// holds for any model.
    price: Price,
    /// "Only the selection" (D30): every part is held to it.
    pub(crate) scope: Option<Vec<Id>>,
    /// The level on screen when the swarm started.
    pub(crate) active_level_id: Option<Id>,
    pub(crate) brief: String,
    log_dir: Option<PathBuf>,
    /// Bumped by every change a waiting worker may be waiting for: a
    /// milestone, a part ending, pause, resume, stop, the cap.
    tick: watch::Sender<u64>,
    st: Mutex<RunState>,
}

#[derive(Default)]
struct RunState {
    /// Dollars the requests in flight may cost.
    reserved: f64,
    /// The cap paused the swarm: raising it resumes.
    cap_paused: bool,
    /// Per part, until when the AI service asked it to wait, with the time
    /// as RFC 3339 for the panel.
    waits: HashMap<String, (tokio::time::Instant, String)>,
    first: First,
    /// Per part, the prompt and answer tokens of its last request: what the
    /// next one reads from the cache.
    last_prompt: HashMap<String, u64>,
}

/// The stagger: the first worker request goes out alone.
#[derive(Debug, Default, PartialEq)]
enum First {
    #[default]
    Free,
    Out(String),
    Answered,
}

/// Why a request was not sent, or no more are.
#[derive(Debug, Clone, PartialEq)]
pub enum Halt {
    /// The swarm or the part ended: a stop, a discard, an accept.
    Ended,
    /// The part (or the lead) cannot go on; the words say why.
    Failed(String),
}

enum Check {
    Go(f64),
    Wait(Duration),
    End(Halt),
}

/// A part that will draw no more.
pub(crate) fn ended(state: SwarmPartState) -> bool {
    matches!(
        state,
        SwarmPartState::Done | SwarmPartState::Stopped | SwarmPartState::Failed | SwarmPartState::Blocked | SwarmPartState::Accepted
    )
}

/// True when `w` is reached: the part reported the milestone, or finished
/// (a finished part passed all of its milestones).
pub(crate) fn reached(d: &Draft, w: &SwarmWait) -> bool {
    d.part(&w.part).is_some_and(|p| {
        p.part.milestones.contains(&w.milestone) || matches!(p.part.state, SwarmPartState::Done | SwarmPartState::Accepted)
    })
}

/// A feed line, newest first.
pub(crate) fn feed(d: &mut Draft, part: &str, text: String) {
    d.feed.insert(0, SwarmFeedItem { at: defaults::now_rfc3339(), part: part.to_string(), text });
    d.feed.truncate(MAX_FEED);
}

/// Change the draft of swarm `id` under its lock and publish the status.
/// None when that swarm's draft is gone.
pub(crate) fn update<R>(app: &AppService, id: &str, f: impl FnOnce(&mut Draft) -> R) -> Option<R> {
    let (out, status) = {
        let mut g = app.swarm.lock();
        let d = g.as_mut().filter(|d| d.id == id)?;
        let out = f(d);
        (out, d.status())
    };
    app.emit(AppEvent::Swarm { swarm: Some(status) });
    Some(out)
}

/// Every part that is drawing or waiting, paused.
fn pause_all(d: &mut Draft) {
    if matches!(d.state, SwarmState::Running | SwarmState::Planning) {
        d.state = SwarmState::Paused;
    }
    for rec in d.parts.iter_mut().skip(1) {
        if matches!(rec.part.state, SwarmPartState::Working | SwarmPartState::Waiting) {
            rec.part.state = SwarmPartState::Paused;
        }
    }
}

/// A paused part back to what it was: waiting while what it waits for is
/// not reached, else working.
fn unpause(d: &mut Draft, key: &str) {
    let waiting = d.part(key).is_some_and(|p| p.after.iter().any(|w| !reached(d, w)));
    if let Some(rec) = d.part_mut(key) {
        if rec.part.state == SwarmPartState::Paused {
            rec.part.state = if waiting { SwarmPartState::Waiting } else { SwarmPartState::Working };
        }
    }
}

fn resume_all(d: &mut Draft) {
    if d.state == SwarmState::Paused {
        // A swarm paused while its lead planned has no plan yet.
        d.state = if d.plan.parts.is_empty() && d.steps.is_empty() { SwarmState::Planning } else { SwarmState::Running };
    }
    let keys: Vec<String> = d.parts.iter().skip(1).map(|p| p.part.key.clone()).collect();
    for key in keys {
        unpause(d, &key);
    }
}

/// When every part ended, the draft is Ready for a person.
fn settle(d: &mut Draft) {
    let parts_ended = d.parts.iter().skip(1).all(|p| ended(p.part.state));
    if !parts_ended || d.parts.len() < 2 || !matches!(d.state, SwarmState::Running | SwarmState::Paused) {
        return;
    }
    d.state = SwarmState::Ready;
    if let Some(lead) = d.part_mut(LEAD) {
        if !ended(lead.part.state) {
            lead.part.state = SwarmPartState::Done;
            lead.part.status = Some("Every part has ended".into());
        }
    }
    feed(d, LEAD, "Every part has ended: the draft is ready for review".into());
}

impl Run {
    fn st(&self) -> MutexGuard<'_, RunState> {
        lock(&self.st)
    }

    /// Wake every waiting worker.
    pub(crate) fn poke(&self) {
        self.tick.send_modify(|n| *n = n.wrapping_add(1));
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<u64> {
        self.tick.subscribe()
    }

    /// One line of `ai-log.jsonl`, with the swarm's id.
    pub(crate) async fn log(&self, _app: &AppService, event: &str, mut fields: Value) {
        if let Some(map) = fields.as_object_mut() {
            map.insert("swarm_id".into(), json!(self.swarm_id));
        }
        log::append(self.log_dir.as_deref(), event, fields).await;
    }

    /// The latest RFC 3339 time any part waits until, or None.
    fn wait_until_text(st: &RunState) -> Option<String> {
        let now = tokio::time::Instant::now();
        st.waits.values().filter(|(until, _)| *until > now).max_by_key(|(until, _)| *until).map(|(_, text)| text.clone())
    }

    /// What the next request of `part` may cost.
    fn next_usd(&self, st: &RunState, part: &str, prompt_tokens: u64, max_tokens: u32) -> f64 {
        let prev = st.last_prompt.get(part).copied().unwrap_or(0);
        cost::next_request_usd(&self.price, prev, prompt_tokens, OUTPUT_GUESS.min(max_tokens as u64))
    }

    /// May `part` send now? Checked under the draft lock: the swarm and the
    /// part, a wait the API asked for, the stagger, then the cap, which
    /// pauses every part when passed. `Go` holds what it may cost.
    fn check_send(&self, app: &AppService, part: &str, is_worker: bool, prompt_tokens: u64, max_tokens: u32) -> Check {
        let mut capped = None;
        let check = {
            let mut g = app.swarm.lock();
            let Some(d) = g.as_mut().filter(|d| d.id == self.swarm_id) else { return Check::End(Halt::Ended) };
            if d.state == SwarmState::Failed {
                return Check::End(Halt::Ended);
            }
            let Some(state) = d.part(part).map(|p| p.part.state) else { return Check::End(Halt::Ended) };
            if ended(state) {
                return Check::End(Halt::Ended);
            }
            // Paused, or a person is accepting the part right now.
            if state == SwarmPartState::Paused || d.state == SwarmState::Paused || d.holding.contains(part) {
                return Check::Wait(POLL);
            }
            let mut st = self.st();
            let now = tokio::time::Instant::now();
            if let Some((until, _)) = st.waits.get(part) {
                if *until > now {
                    return Check::Wait(*until - now);
                }
            }
            if is_worker && matches!(&st.first, First::Out(p) if p != part) {
                return Check::Wait(POLL);
            }
            let next = self.next_usd(&st, part, prompt_tokens, max_tokens);
            let over = d.cost.as_ref().is_some_and(|c| c.spent_usd + st.reserved + next > c.cap_usd);
            if over {
                st.cap_paused = true;
                drop(st);
                pause_all(d);
                let cap = d.cost.as_ref().map(|c| c.cap_usd).unwrap_or_default();
                feed(d, LEAD, format!("Paused: the next requests could pass the US${cap:.2} cap. Raise the cap to go on, or stop."));
                capped = Some(d.status());
                Check::Wait(POLL)
            } else {
                st.reserved += next;
                if is_worker && st.first == First::Free {
                    st.first = First::Out(part.to_string());
                }
                Check::Go(next)
            }
        };
        if let Some(status) = capped {
            app.emit(AppEvent::Swarm { swarm: Some(status) });
            self.poke();
        }
        check
    }

    /// Still free to send, after waiting for the gate: nobody paused or
    /// stopped meanwhile.
    fn still_free(&self, app: &AppService, part: &str) -> bool {
        let g = app.swarm.lock();
        g.as_ref().filter(|d| d.id == self.swarm_id).is_some_and(|d| {
            d.state != SwarmState::Paused
                && d.part(part).is_some_and(|p| !ended(p.part.state) && p.part.state != SwarmPartState::Paused)
        })
    }

    /// A request is back (`answered`), or never went: free what it held.
    /// The first request's answer lets the other workers go; one that never
    /// went leaves the turn to whoever asks next.
    fn release(&self, reserved: f64, part: &str, answered: bool) {
        let mut st = self.st();
        st.reserved = (st.reserved - reserved).max(0.0);
        if matches!(&st.first, First::Out(p) if p == part) {
            st.first = if answered { First::Answered } else { First::Free };
        }
    }

    /// Send one request for `part` (`agent` its worker, None for the lead):
    /// held while paused or waiting, refused once the part or the swarm
    /// ended, sent through the gate, its usage counted. A rate limit or an
    /// overload waits this part only and tries again.
    pub(crate) async fn send(
        &self,
        app: &AppService,
        part: &str,
        agent: Option<&str>,
        client: &dyn ModelClient,
        request: &ModelRequest,
        round: usize,
    ) -> Result<Metered, Halt> {
        let mut rx = self.subscribe();
        let prompt_tokens = cost::tokens_of(
            request.system.len()
                + serde_json::to_string(&request.tools).map(|s| s.len()).unwrap_or(0)
                + serde_json::to_string(&request.messages).map(|s| s.len()).unwrap_or(0),
        );
        let mut waits = 0u32;
        loop {
            rx.borrow_and_update();
            if let Some(agent) = agent {
                // The Agents panel's pause and stop hold for workers too.
                match agent_activity(app, agent) {
                    Some(AgentActivity::Paused) => {
                        idle(&mut rx, POLL).await;
                        continue;
                    }
                    Some(AgentActivity::Stopped) => return Err(Halt::Ended),
                    _ => {}
                }
            }
            let reserved = match self.check_send(app, part, agent.is_some(), prompt_tokens, request.max_tokens) {
                Check::Go(r) => r,
                Check::Wait(d) => {
                    idle(&mut rx, d).await;
                    continue;
                }
                Check::End(h) => return Err(h),
            };
            let permit = app.ai.gate.enter().await;
            if !self.still_free(app, part) {
                drop(permit);
                self.release(reserved, part, false);
                continue;
            }
            let started = std::time::Instant::now();
            let result = client.send_once(request).await;
            drop(permit);
            self.release(reserved, part, true);
            self.poke();
            match result {
                Ok(m) => {
                    self.count(app, part, round, &m, started.elapsed()).await;
                    return Ok(m);
                }
                Err(e) if e.is_wait() => {
                    waits += 1;
                    let backoff = (FIRST_BACKOFF * 2u32.saturating_pow(waits - 1)).min(MAX_BACKOFF);
                    let wait = e.retry_after().unwrap_or(backoff);
                    self.log(
                        app,
                        "swarm_wait",
                        json!({"part": part, "round": round, "code": e.code(), "wait_ms": wait.as_millis() as u64}),
                    )
                    .await;
                    if waits > MAX_WAITS {
                        return Err(Halt::Failed(format!("The AI service kept asking to wait: {}", e.message())));
                    }
                    self.wait_part(app, part, wait);
                }
                Err(e) => {
                    self.log(app, "swarm_error", json!({"part": part, "round": round, "code": e.code(), "message": e.message()}))
                        .await;
                    return Err(Halt::Failed(e.message().to_string()));
                }
            }
        }
    }

    /// The API asked `part` to wait: only it waits (amendments P1-6).
    fn wait_part(&self, app: &AppService, part: &str, wait: Duration) {
        let until = tokio::time::Instant::now() + wait;
        let text = (chrono::Utc::now() + chrono::Duration::from_std(wait).unwrap_or_default())
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        let shown = {
            let mut st = self.st();
            st.waits.insert(part.to_string(), (until, text));
            Self::wait_until_text(&st)
        };
        let secs = wait.as_secs_f64().ceil() as u64;
        update(app, &self.swarm_id, |d| {
            d.ai_wait_until = shown;
            if let Some(rec) = d.part_mut(part) {
                rec.part.status = Some(format!("Waiting for the AI service, {secs} s"));
            }
        });
    }

    /// Count a response: its usage into the cost, its wait cleared, a line
    /// in the log.
    async fn count(&self, app: &AppService, part: &str, round: usize, m: &Metered, took: Duration) {
        let (shown, cleared) = {
            let mut st = self.st();
            st.last_prompt.insert(part.to_string(), m.usage.prompt_tokens() + m.usage.output_tokens);
            let cleared = st.waits.remove(part).is_some();
            (Self::wait_until_text(&st), cleared)
        };
        let spent = update(app, &self.swarm_id, |d| {
            if let Some(c) = d.cost.as_mut() {
                cost::add(c, &m.usage, m.model.as_deref());
            }
            d.ai_wait_until = shown;
            if cleared {
                if let Some(rec) = d.part_mut(part) {
                    rec.part.status = None;
                }
            }
            d.cost.as_ref().map(|c| c.spent_usd)
        })
        .flatten();
        let u = &m.usage;
        self.log(
            app,
            "swarm_round",
            json!({
                "part": part,
                "round": round,
                "model": m.model.clone().unwrap_or_else(|| self.model.clone()),
                "stop_reason": m.response.stop_reason,
                "tools": m.response.tool_uses().iter().map(|t| t.name.clone()).collect::<Vec<_>>(),
                "usage": {
                    "input_tokens": u.input_tokens,
                    "output_tokens": u.output_tokens,
                    "cache_read_tokens": u.cache_read_tokens,
                    "cache_write_tokens": u.cache_write_tokens,
                },
                "spent_usd": spent,
                "model_ms": took.as_millis() as u64,
            }),
        )
        .await;
    }

    /// A part ended (any way): its wait goes, and the draft is Ready once
    /// every part ended.
    pub(crate) fn part_ended(&self, app: &AppService, part: &str) {
        let shown = {
            let mut st = self.st();
            st.waits.remove(part);
            Self::wait_until_text(&st)
        };
        update(app, &self.swarm_id, |d| {
            d.ai_wait_until = shown;
            settle(d);
        });
        self.poke();
    }
}

/// Wait for a tick, at most `at_most` (and never longer than `POLL`).
pub(crate) async fn idle(rx: &mut watch::Receiver<u64>, at_most: Duration) {
    let _ = tokio::time::timeout(at_most.min(POLL), rx.changed()).await;
}

/// What a person set an agent to, from the Agents panel.
fn agent_activity(app: &AppService, agent: &str) -> Option<AgentActivity> {
    app.agents_list().into_iter().find(|a| a.participant.id == agent).map(|a| a.activity)
}

/// Places free among the `MAX_AGENTS` of this computer.
fn free_places(app: &AppService) -> usize {
    let taken = app.agents_list().iter().filter(|a| a.activity != AgentActivity::Stopped).count();
    MAX_AGENTS.saturating_sub(taken)
}

// ================================================================== start

fn clean(raw: &str, max: usize) -> String {
    raw.chars().filter(|c| !c.is_control() || *c == '\n').take(max).collect::<String>().trim().to_string()
}

/// A worker's or the lead's agent: its own connection `swarm:<id>:<key>`,
/// and for a part its handle, which binds it to the part (`on_call`).
pub(crate) fn hint(swarm_id: &str, key: &str, handle: Option<&str>) -> AgentHint {
    let short: String = swarm_id.chars().filter(|c| c.is_ascii_alphanumeric()).take(12).collect();
    AgentHint {
        connection: Some(format!("swarm:{short}:{key}")),
        agent: Some(handle.unwrap_or(key).to_string()),
        label: None,
        instance: None,
        client_name: Some("guhit-studio".into()),
        client_version: Some(env!("CARGO_PKG_VERSION").into()),
    }
}

/// The model the workers use: the person's copilot model, else the
/// default.
async fn swarm_model(app: &AppService) -> String {
    let dir = app.session.lock().await.data_dir.clone();
    client::model_setting(&dir).unwrap_or_else(|| DEFAULT_SWARM_MODEL.to_string())
}

/// What the estimate and the cap start from.
fn sizes(context_chars: usize) -> cost::Sizes {
    let json_len = |v: &Value| serde_json::to_string(v).map(|s| s.len()).unwrap_or(0);
    cost::Sizes {
        worker_prefix: cost::tokens_of(prompt::WORKER_SYSTEM.len() + json_len(&Value::Array(prompt::worker_tools()))),
        lead_prefix: cost::tokens_of(prompt::LEAD_SYSTEM.len() + json_len(&prompt::plan_tool())),
        context: cost::tokens_of(context_chars),
    }
}

/// `swarm_estimate`: a low and a high figure on the person's model and
/// cap. Spends nothing.
pub async fn estimate(app: &AppService, request: &SwarmRequest) -> Result<SwarmCost, IpcError> {
    let model = swarm_model(app).await;
    let (context, data_dir) = {
        let s = app.session.lock().await;
        let context = s.doc.as_ref().map(|doc| crate::ai::prompt::project_context(doc, &[], None).len()).unwrap_or(4000);
        (context, s.data_dir.clone())
    };
    let cap = match request.cap_usd {
        Some(c) => cost::check_cap(c)?,
        None => cost::cap_setting(&data_dir),
    };
    Ok(cost::new_cost(&model, cap, &sizes(context + request.brief.len())))
}

/// `swarm_start`: open a planning draft and start the lead. Answers at once
/// with the planning status; the plan and the parts follow as `swarm`
/// events. Refused: `bad_args` (no brief, a bad cap), `live_session`,
/// `draft_open`, `no_document`, `ai_not_configured` without a key.
pub async fn start(app: &AppService, request: SwarmRequest) -> Result<SwarmStatus, IpcError> {
    let brief = clean(&request.brief, MAX_BRIEF_CHARS);
    if brief.is_empty() {
        return Err(IpcError::new("bad_args", "Write what the swarm should draw."));
    }
    if !matches!(app.live.role(), crate::live::Role::Off) {
        return Err(super::live_session());
    }
    if app.swarm.is_open() {
        return Err(super::draft_open());
    }
    let model = swarm_model(app).await;
    let log_dir = app.project_dir().await;
    let scope = request.scope.as_ref().map(|s| s.ids.clone());
    if scope.as_ref().is_some_and(|ids| ids.is_empty()) {
        return Err(IpcError::new("bad_args", "scope has no ids: limit the swarm to a selection, or send no scope"));
    }
    let (message, context_len, cap) = {
        let s = app.session.lock().await;
        if s.live_copy.is_some() {
            return Err(super::live_session());
        }
        let doc = s.open_doc()?;
        if let Some(ids) = &scope {
            guhit_core::scope::validate(doc.project(), ids)?;
        }
        let here = request
            .active_level_id
            .as_ref()
            .and_then(|id| doc.project().levels.iter().find(|l| &l.id == id))
            .map(|l| format!("{} (id {})", l.name, l.id));
        let context = crate::ai::prompt::project_context(doc, &[], scope.as_deref());
        let cap = match request.cap_usd {
            Some(c) => cost::check_cap(c)?,
            None => cost::cap_setting(&s.data_dir),
        };
        (prompt::lead_message(&brief, here.as_deref(), None, &context), context.len(), cap)
    };
    // The lead's client first: without a key nothing starts.
    let client = client::client_for(app, &message)?;
    let swarm_id = defaults::new_id();
    let lead_call = app.agent_call(&hint(&swarm_id, LEAD, None), "swarm", CallKind::Read)?;
    let owner = app.agents.owner();
    let lead_color = app.agents.participant(&lead_call.id, &owner).map(|p| p.color).unwrap_or(1);
    let new_cost = cost::new_cost(&model, cap, &sizes(context_len + brief.len()));
    let (low, high) = (new_cost.low_usd, new_cost.high_usd);
    let status = {
        let s = app.session.lock().await;
        let doc = s.open_doc()?;
        let mut g = app.swarm.lock();
        if g.is_some() {
            return Err(super::draft_open());
        }
        let empty = SwarmPlan::default();
        let mut d = start_draft(Start {
            project: doc.project(),
            revision: doc.revision(),
            brief: brief.clone(),
            source: SwarmSource::App,
            lead_id: lead_call.id.clone(),
            lead_color,
            plan: &empty,
            handles: &[],
        })?;
        d.id = swarm_id.clone();
        d.state = SwarmState::Planning;
        if let Some(lead) = d.part_mut(LEAD) {
            lead.part.status = Some("Reading the brief".into());
        }
        d.cost = Some(new_cost);
        let status = d.status();
        *g = Some(d);
        status
    };
    let (tick, _) = watch::channel(0u64);
    let run = Arc::new(Run {
        swarm_id: swarm_id.clone(),
        price: cost::billed(&model).0,
        model,
        scope,
        active_level_id: request.active_level_id.clone(),
        brief,
        log_dir,
        tick,
        st: Mutex::new(RunState::default()),
    });
    *lock(&app.swarm.runs.current) = Some(run.clone());
    run.log(app, "swarm_start", json!({"model": run.model, "cap_usd": cap, "low_usd": low, "high_usd": high})).await;
    app.emit(AppEvent::Swarm { swarm: Some(status.clone()) });
    app.agents.publish();
    tokio::spawn(drive(app.clone(), run, client, message, lead_call.id));
    Ok(status)
}

/// The lead's task: plan, put the plan in the draft, start the workers.
async fn drive(app: AppService, run: Arc<Run>, client: Arc<dyn ModelClient>, message: String, lead_id: Id) {
    let check = |plan: &SwarmPlan| -> Result<(), IpcError> {
        {
            let g = app.swarm.lock();
            let d = g.as_ref().filter(|d| d.id == run.swarm_id).ok_or_else(|| IpcError::new("not_found", "The swarm ended."))?;
            validate_plan(&d.base, plan)?;
        }
        let free = free_places(&app);
        if plan.parts.len() > free {
            return Err(IpcError::new(
                "too_many_agents",
                format!("Only {free} agents can work on this computer now. Make a plan of at most {free} parts."),
            ));
        }
        Ok(())
    };
    let planned = lead::plan(&app, &run, client.as_ref(), message, check).await;
    let plan = match planned {
        Ok(plan) => plan,
        Err(Halt::Ended) => return run.part_ended(&app, LEAD),
        Err(Halt::Failed(why)) => return fail_swarm(&app, &run, &why),
    };
    let handles = match install(&app, &run, &plan, &lead_id).await {
        Ok(h) => h,
        Err(e) if e.code == "ended" => return,
        Err(e) => return fail_swarm(&app, &run, &e.message),
    };
    for (handle, key) in handles {
        tokio::spawn(worker::run(app.clone(), run.clone(), key, handle));
    }
    run.poke();
}

/// The swarm cannot go on: Failed, with why in the feed. The draft so far
/// stays for a person to discard.
fn fail_swarm(app: &AppService, run: &Run, why: &str) {
    let why = clean(why, 300);
    update(app, &run.swarm_id, |d| {
        d.state = SwarmState::Failed;
        if let Some(lead) = d.part_mut(LEAD) {
            lead.part.state = SwarmPartState::Failed;
            lead.part.status = Some(why.clone());
        }
        feed(d, LEAD, format!("Lead: {why}"));
    });
    run.poke();
}

/// Put `plan` in the planning draft: a new draft from `start_draft` with
/// the same id, its setup levels as the lead's first step, a handle per
/// part. A pause made while planning holds. The parts' handles, in plan
/// order.
async fn install(app: &AppService, run: &Run, plan: &SwarmPlan, lead_id: &Id) -> Result<Vec<(String, String)>, IpcError> {
    let gone = || IpcError::new("ended", "The swarm ended while its lead planned.");
    let s = app.session.lock().await;
    let doc = s.open_doc()?;
    let (status, handles) = {
        let mut g = app.swarm.lock();
        let old = g.as_ref().filter(|d| d.id == run.swarm_id).ok_or_else(gone)?;
        let lead = old.part(LEAD).ok_or_else(gone)?;
        if ended(lead.part.state) || old.state == SwarmState::Failed {
            return Err(gone());
        }
        validate_plan(doc.project(), plan)?;
        let keys: Vec<&str> = plan.parts.iter().map(|p| p.key.as_str()).collect();
        let issued = app.swarm.issue(&keys);
        let mut new = start_draft(Start {
            project: doc.project(),
            revision: doc.revision(),
            brief: old.brief.clone(),
            source: SwarmSource::App,
            lead_id: lead_id.clone(),
            lead_color: lead.part.color,
            plan,
            handles: &issued,
        })?;
        new.id = old.id.clone();
        new.cost = old.cost.clone();
        new.ai_wait_until = old.ai_wait_until.clone();
        let paused = old.state == SwarmState::Paused;
        let mut items = new.feed.clone();
        items.extend(old.feed.iter().cloned());
        new.feed = items;
        let names: Vec<&str> = plan.parts.iter().map(|p| p.name.as_str()).collect();
        let n = names.len();
        feed(&mut new, LEAD, format!("Lead: split the brief into {n} part{}: {}", if n == 1 { "" } else { "s" }, names.join(", ")));
        if let Some(lead) = new.part_mut(LEAD) {
            lead.part.status = Some("Watching the parts".into());
        }
        if paused {
            pause_all(&mut new);
        }
        let handles = new.handles.clone();
        let status = new.status();
        *g = Some(new);
        (status, handles)
    };
    drop(s);
    app.emit(AppEvent::Swarm { swarm: Some(status) });
    app.agents.publish();
    Ok(handles)
}

// ============================================================ the window

fn no_swarm() -> IpcError {
    IpcError::new("not_found", "No swarm draft is open.")
}

/// `swarm_pause`, `swarm_resume`, `swarm_stop`, for every part (`part`
/// None) or one. Pause holds parts before their next request; stop ends
/// them after their current request, their steps staying in the draft.
/// Works for an MCP swarm too: its parts' agents are refused while paused
/// or stopped (`Draft::check_step`, `on_call`).
pub fn set_run(app: &AppService, cmd: &str, part: Option<&str>) -> Result<SwarmStatus, IpcError> {
    let status = {
        let mut g = app.swarm.lock();
        let d = g.as_mut().ok_or_else(no_swarm)?;
        if let Some(key) = part {
            if d.part(key).is_none() {
                return Err(IpcError::new("not_found", format!("The swarm has no part {key}.")));
            }
        }
        let run = app.swarm.runs.get(&d.id);
        match (cmd, part) {
            ("swarm_pause", None) => {
                pause_all(d);
                feed(d, LEAD, "Paused".into());
            }
            ("swarm_pause", Some(key)) => {
                if let Some(rec) = d.part_mut(key) {
                    if matches!(rec.part.state, SwarmPartState::Working | SwarmPartState::Waiting) {
                        rec.part.state = SwarmPartState::Paused;
                    }
                }
            }
            ("swarm_resume", None) => {
                if let Some(run) = &run {
                    run.st().cap_paused = false;
                }
                resume_all(d);
                feed(d, LEAD, "Resumed".into());
            }
            ("swarm_resume", Some(key)) => unpause(d, key),
            (_, None) => {
                let planning = d.parts.len() < 2;
                // An in-app lead only watches once it planned; an MCP lead draws.
                let lead_draws = planning || d.source != SwarmSource::App;
                for rec in d.parts.iter_mut() {
                    if !ended(rec.part.state) && (rec.part.key != LEAD || lead_draws) {
                        rec.part.state = SwarmPartState::Stopped;
                        rec.part.status = Some("Stopped".into());
                    }
                }
                if planning {
                    d.state = SwarmState::Failed;
                    feed(d, LEAD, "Stopped before the lead split the brief".into());
                } else {
                    if d.state == SwarmState::Paused {
                        d.state = SwarmState::Running;
                    }
                    feed(d, LEAD, "Stopped every part".into());
                    settle(d);
                }
            }
            (_, Some(key)) => {
                if let Some(rec) = d.part_mut(key) {
                    if !ended(rec.part.state) {
                        rec.part.state = SwarmPartState::Stopped;
                        rec.part.status = Some("Stopped".into());
                        let text = format!("{} stopped", rec.part.name);
                        feed(d, key, text);
                    }
                }
                settle(d);
            }
        }
        if let Some(run) = run {
            run.poke();
        }
        d.status()
    };
    app.emit(AppEvent::Swarm { swarm: Some(status.clone()) });
    app.agents.publish();
    Ok(status)
}

/// `swarm_set_cap`: the running swarm's cap. When the cap paused the swarm
/// and the new one leaves room, the swarm goes on.
pub fn set_cap(app: &AppService, cap_usd: f64) -> Result<SwarmStatus, IpcError> {
    let cap = cost::check_cap(cap_usd)?;
    let status = {
        let mut g = app.swarm.lock();
        let d = g.as_mut().ok_or_else(no_swarm)?;
        let run = app.swarm.runs.get(&d.id);
        let Some(c) = d.cost.as_mut() else {
            return Err(IpcError::new("bad_args", "Only a swarm Guhit runs on your Claude key has a cap."));
        };
        c.cap_usd = cap;
        let spent = c.spent_usd;
        if let Some(run) = &run {
            let mut st = run.st();
            if st.cap_paused && spent + st.reserved < cap {
                st.cap_paused = false;
                drop(st);
                resume_all(d);
                feed(d, LEAD, format!("The cap is US${cap:.2}: going on"));
            }
            run.poke();
        }
        d.status()
    };
    app.emit(AppEvent::Swarm { swarm: Some(status.clone()) });
    Ok(status)
}

// ================================================================ nearby

/// The levels a part draws on, as ids; empty is every level.
fn levels_of(d: &Draft, part: &str) -> Vec<Id> {
    d.part(part).map(|p| p.scope.levels.clone()).unwrap_or_default()
}

/// Other parts' steps after `since` that touched `part`'s levels, newest
/// last, at most `MAX_NEARBY`; and the newest `seq` there is.
pub(crate) fn notes_since(d: &Draft, part: &str, since: u32) -> (Vec<NearbyNote>, u32) {
    let levels = levels_of(d, part);
    let view = d.view.project();
    let level_of = |id: &Id| {
        view.elements
            .iter()
            .chain(d.base.elements.iter())
            .find(|e| e.id() == id)
            .and_then(|e| e.level_id().cloned())
    };
    let mut notes: Vec<NearbyNote> = vec![];
    for step in d.steps.iter().filter(|s| s.seq > since && s.part != part) {
        let ids: Vec<Id> = step.added.iter().chain(&step.modified).chain(&step.removed).cloned().collect();
        let near = levels.is_empty() || ids.iter().any(|id| level_of(id).is_none_or(|l| levels.contains(&l)));
        if !near {
            continue;
        }
        let who = d.part(&step.part).map(|p| p.part.name.clone()).unwrap_or_else(|| step.part.clone());
        notes.push(NearbyNote { who, what: step.label.clone(), at: step.at.clone(), ids });
    }
    notes.drain(..notes.len().saturating_sub(MAX_NEARBY));
    (notes, d.seq)
}

/// `swarm::nearby`: what an agent bound to a part hears in its notes,
/// other parts' steps on its levels since its last call (MCP sub-agents'
/// `agent_notes`; in-app workers get theirs in each round).
pub(crate) fn nearby(app: &AppService, call: &AgentCall) -> Vec<NearbyNote> {
    let g = app.swarm.lock();
    let Some(d) = g.as_ref() else { return vec![] };
    let Some(part) = d.part_of(&call.id) else { return vec![] };
    let key = (d.id.clone(), call.id.clone());
    let mut told = lock(&app.swarm.runs.told);
    told.retain(|(swarm, _), _| *swarm == d.id);
    let since = told.get(&key).copied().unwrap_or(0);
    let (notes, newest) = notes_since(d, part, since);
    told.insert(key, newest);
    notes
}

// ===================================================================== IPC

/// The calls this module answers, routed from `swarm::handle`.
pub async fn handle(app: &AppService, cmd: &str, args: Value) -> IpcResult {
    match cmd {
        "swarm_estimate" => {
            let request: SwarmRequest = arg(&args, "request")?;
            to_value(&estimate(app, &request).await?)
        }
        "swarm_start" => {
            let request: SwarmRequest = arg(&args, "request")?;
            to_value(&start(app, request).await?)
        }
        "swarm_pause" | "swarm_resume" | "swarm_stop" => {
            let part: Option<String> = arg(&args, "part")?;
            to_value(&set_run(app, cmd, part.as_deref())?)
        }
        "swarm_set_cap" => {
            let cap_usd: f64 = arg(&args, "cap_usd")?;
            to_value(&set_cap(app, cap_usd)?)
        }
        _ => Err(IpcError::new("unknown_command", format!("unknown command `{cmd}`"))),
    }
}
