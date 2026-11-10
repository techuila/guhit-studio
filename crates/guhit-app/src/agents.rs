//! Agents as participants (DECISIONS D42, D47, docs/CONTRACT.md "Agents"):
//! every MCP client working on the open project gets a name, a color, a
//! presence and a status line, its own history steps, and controls the
//! architect holds: pause, stop, a limit. Edits apply directly (D47), each
//! one undo step with the agent as author and its owner as `on_behalf_of`.
//! It all works with or without a live session; in one, agents on every
//! computer show to everyone (`live::wire`, the "agents" feature).
//!
//! # API for the MCP server
//!
//! Per tool call, the server:
//! 1. builds an [`AgentHint`] from the request: `connection` (L1: a stable
//!    key for the connection the call came on, the field is
//!    `AgentHint::connection`: the MCP session id the server issued, else
//!    the labelled URL, else the stdio instance), the `agent` tool argument,
//!    a labelled URL `/mcp/agent/<label>`, the stdio proxy's instance header
//!    `x-guhit-agent-instance` (and its `--agent <label>`), and `clientInfo`.
//!    Pause, stop, the limit and the rate limit belong to the connection:
//!    every handle (`agent`) on it shares them, so a new handle never gets
//!    out of them;
//! 2. calls [`AppService::agent_call`] with the tool's name and whether it
//!    edits. That resolves the agent (registering a new one), refuses a
//!    stopped agent, a paused agent's edits and a ninth agent, and records
//!    the call;
//! 3. edits through [`AppService::agent_edit`] (a staged batch, translated
//!    by a closure that may run twice) or [`AppService::agent_commit`] (one
//!    command), undoes with [`AppService::agent_undo`], chats with
//!    [`AppService::agent_chat`]. These check the agent's limit and the
//!    other agents' claims, commit as the agent and record where it worked;
//! 4. after a read, calls [`AppService::agent_done`] with what it looked at.
//!
//! Claims and status: [`AppService::agent_claim`], [`AppService::agent_release`],
//! [`AppService::agent_set_status`]. Everyone working now, with their claims:
//! [`AppService::agents_list`].
//!
//! Errors, each saying what to do: `agent_paused`, `agent_stopped`,
//! `too_many_agents`, `out_of_scope` (its limit), `claimed` (another agent's
//! claim), `rate_limited`, `other_author` (undoing someone else's step).
//!
//! Locks: the registry lock is a leaf. It is taken under the session lock,
//! never held across an await, and never held while a live session lock is
//! taken.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use guhit_core::Staging;
use guhit_model::*;
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::live::{self, LiveState};
use crate::{arg, files, snapshots, store, to_value, AppService, IpcResult, Session};

// ======================================================================= API

/// Who is calling, as the MCP server can tell from one request. Every field
/// is optional and as the client sent it; the registry cleans them. The
/// connection is `connection`, else `instance`, else `label`, else
/// `client_name`; the agent is the connection and its `agent` handle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentHint {
    /// A stable key for the connection the call came on, chosen by the
    /// server, never by the client's arguments: the MCP session id the
    /// server issued, the labelled URL, or the stdio instance. Pause, stop,
    /// the limit and the rate limit hold for the whole connection, every
    /// handle on it included.
    pub connection: Option<String>,
    /// The `agent` argument of the tool call ("ground-floor"): a handle
    /// inside one connection, the only way to tell subagents apart.
    pub agent: Option<String>,
    /// The label of a labelled URL (`/mcp/agent/<label>`), or the stdio
    /// proxy's `--agent <label>`.
    pub label: Option<String>,
    /// The stdio proxy's random id for its process (`x-guhit-agent-instance`).
    pub instance: Option<String>,
    /// `clientInfo` as the request carried it, or as the last client that
    /// named itself did.
    pub client_name: Option<String>,
    pub client_version: Option<String>,
}

/// What a tool call does to the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallKind {
    /// Reads the model, takes a picture or chats: allowed while paused.
    Read,
    /// Changes the model, undoes or redoes: refused while paused.
    Edit,
    /// Leaves the model as it is but spends money or runs long: an AI image,
    /// a traced render, a tour video. Refused while paused, like an edit, so
    /// a pause stops the spending too (GR1 #8).
    Work,
}

/// One tool call of one agent, from [`AppService::agent_call`]. It holds no
/// lock; pass it to the calls below.
#[derive(Debug, Clone)]
pub struct AgentCall {
    /// The agent's participant id: the author of its steps.
    pub id: Id,
    /// Its name as people see it: "Ground floor".
    pub name: String,
    pub kind: CallKind,
    /// The project open when the call started. An edit, undo or claim of
    /// the call is refused (`project_changed`) once another one is open.
    pub project_id: Option<Id>,
}

impl AppService {
    /// Start one tool call: resolve who calls (a new agent is registered,
    /// named and colored), refuse it when the agent is stopped, an edit when
    /// it is paused, and a new agent when 8 work on this computer already.
    /// The call is recorded: the agent is active, its status line follows
    /// `tool` ("add_door": "Placing doors") unless it set one.
    pub fn agent_call(&self, hint: &AgentHint, tool: &str, kind: CallKind) -> Result<AgentCall, IpcError> {
        let owner = self.agents.owner();
        let project_id = self.changes.borrow().project_id.clone();
        // A refused call is news too: a stopped agent that keeps trying stays
        // on the list, so the user can see it and resume it.
        let call = self.agents.call(hint, tool, kind, &owner, project_id);
        // A swarm part's handle binds the agent to that part (D56).
        let call = call.and_then(|mut call| crate::swarm::on_call(self, hint, &mut call).map(|()| call));
        self.agents.start_ticking();
        self.agents.publish();
        call
    }

    /// Translate, check and commit one editing call of an agent as one undo
    /// step. `build` translates the call on a project (with its derived data)
    /// into a staged batch, plus whatever the caller wants back (per step
    /// results). It runs once on a copy, outside the session lock; when the
    /// plan moved on before the commit, it runs again under the lock, so an
    /// agent's call never fails as `stale` because another agent edited
    /// meanwhile. On a live session guest the host commits it, and a `stale`
    /// from the host runs `build` again once.
    ///
    /// Refused, with nothing applied: a paused or stopped agent, an edit
    /// outside its limit (`out_of_scope`), one reaching another agent's
    /// claim (`claimed`), more than 5 commits a second (`rate_limited`).
    /// The first agent edit after 10 minutes without one saves a version
    /// first ("Before agents, 3:42 PM"). After the commit the agent's
    /// presence moves to what the edit touched.
    pub async fn agent_edit<T, E, F>(&self, call: &AgentCall, build: F) -> Result<(ApplyResult, T), E>
    where
        F: Fn(&Project, &Derived) -> Result<(Staging, T), E> + Send,
        T: Send,
        E: From<IpcError> + Send,
    {
        // An agent drawing a swarm part writes into the draft (D57).
        if self.swarm.part_of(&call.id).is_some() {
            return crate::swarm::draft_edit(self, call, build).await;
        }
        let (project, derived, revision, guest) = {
            let s = self.session.lock().await;
            let doc = s.open_doc()?;
            same_project(call, doc)?;
            (doc.project().clone(), doc.derived().clone(), doc.revision(), s.live_copy.is_some())
        };
        // Every project starts at revision 0, so the revision alone does not
        // tell two projects apart: the project id goes with it.
        let project_id = project.id.clone();
        let (staging, extra) = build(&project, &derived)?;
        drop((project, derived));
        if guest {
            return self.agent_edit_on_host(call, staging, extra, (project_id, revision), &build).await;
        }
        let mut s = self.session.lock().await;
        if s.live_copy.is_some() {
            return Err(live::live_lost().into());
        }
        let doc = s.open_doc()?;
        if doc.project().id != project_id {
            return Err(project_changed(doc).into());
        }
        let (staging, extra) = if doc.revision() == revision {
            (staging, extra)
        } else {
            build(doc.project(), doc.derived())?
        };
        let result = self.agent_commit_locked(&mut s, &call.id, &project_id, Edit::Staged(Box::new(staging)))?;
        drop(s);
        self.agents.publish();
        Ok((result, extra))
    }

    /// `agent_edit` for one command the caller built and checked itself.
    /// A `Command::Batch` is staged, so its later steps can build on the
    /// earlier ones as usual.
    pub async fn agent_commit(&self, call: &AgentCall, command: Command) -> Result<ApplyResult, IpcError> {
        let label = match &command {
            Command::Batch { label, .. } => label.clone(),
            _ => String::new(),
        };
        let commands = match &command {
            Command::Batch { commands, .. } => commands.clone(),
            other => vec![other.clone()],
        };
        let batch = matches!(command, Command::Batch { .. });
        // An agent drawing a swarm part writes into the draft (D57).
        if batch || self.swarm.part_of(&call.id).is_some() {
            let (result, ()) = self
                .agent_edit(call, |project: &Project, _: &Derived| {
                    Staging::rebuilt(project, &label, &commands).map(|st| (st, ())).map_err(IpcError::from)
                })
                .await?;
            return Ok(result);
        }
        let mut s = self.session.lock().await;
        let project_id = {
            let doc = s.open_doc()?;
            same_project(call, doc)?;
            doc.project().id.clone()
        };
        if s.live_copy.is_some() {
            // One command with absolute ids and mm: the host applies it to its
            // plan as it is, as it does a person's edit, so it is never stale.
            drop(s);
            self.agents.check_edit(&call.id, self.agents.now())?;
            let result = {
                let s = self.session.lock().await;
                let doc = s.open_doc()?;
                same_project(call, doc)?;
                self.agents.check_reach(&call.id, doc.project(), doc.derived(), doc.project(), &command)?;
                drop(s);
                live::guest_apply_as(self, command, Origin::Ai, None, Some(call.id.clone())).await?
            };
            self.agents.committed(&call.id, &result, self.agents.now());
            self.agents.publish();
            return Ok(result);
        }
        let result = self.agent_commit_locked(&mut s, &call.id, &project_id, Edit::One(Box::new(command)))?;
        drop(s);
        self.agents.publish();
        Ok(result)
    }

    /// Undo (or with `redo` redo) the agent's own top step, in or out of a
    /// live session. Another agent's step is refused (`other_author`)
    /// always; a person's step only with `force`, which the MCP client may
    /// pass once the user confirmed (DECISIONS D29). A step that changed the
    /// signing architect is refused (`forbidden`) even with `force`.
    pub async fn agent_undo(&self, call: &AgentCall, redo: bool, force: bool) -> Result<DocState, IpcError> {
        // A swarm part's agent undoes its own draft steps, never the model.
        if self.swarm.part_of(&call.id).is_some() {
            return crate::swarm::draft_undo(self, call, redo, force).await;
        }
        self.agents.check_edit(&call.id, self.agents.now())?;
        let mut s = self.session.lock().await;
        same_project(call, s.open_doc()?)?;
        // Never across a change of the signing architect, `force` or not.
        crate::check_agent_steps(&s, &self.live, redo, 1)?;
        if let Some(copy) = s.live_copy.as_ref() {
            let top = if redo { copy.undo.redo_by.clone() } else { copy.undo.undo_by.clone() };
            let label = if redo { copy.undo.redo_label.clone() } else { copy.undo.undo_label.clone() };
            let revision = s.open_doc()?.revision();
            let host_knows_agents = live::host_knows_agents(self);
            let own = top.as_ref() == Some(&call.id)
                || (!host_knows_agents && top.is_some() && top == self.live.status().self_id);
            if !own {
                self.agents.refuse_step(&self.live, top.as_ref(), label.as_deref(), redo, force)?;
            }
            drop(s);
            let state = live::guest_step_as(self, redo, true, 1, Some(revision)).await?;
            self.agents.publish();
            return Ok(state);
        }
        let doc = s.open_doc()?;
        let list = doc.history();
        let step = if redo { list.redo.first() } else { list.undo.first() };
        let Some(step) = step else {
            let what = if redo { "redo" } else { "undo" };
            return Err(IpcError::new("nothing_to_undo", format!("There is nothing to {what}.")));
        };
        if step.author_id.as_ref() != Some(&call.id) {
            let label = step.label.clone();
            self.agents.refuse_step(&self.live, step.author_id.as_ref(), Some(&label), redo, force)?;
        }
        let state = self.step_locked(&mut s, redo, true, 1, None, Some(&call.id), None)?;
        drop(s);
        self.agents.publish();
        Ok(state)
    }

    /// Post a chat message as the agent: its name and color, marked as sent
    /// by AI. In a live session everyone sees it; without one it goes to the
    /// project's chat (`chat.jsonl`), which the Chat panel shows.
    pub async fn agent_chat(
        &self,
        call: &AgentCall,
        text: &str,
        at: Option<Point>,
        level_id: Option<Id>,
    ) -> Result<ChatMessage, IpcError> {
        let text = live::check_chat(text, at, level_id.as_deref())?;
        let author = self
            .agents
            .participant(&call.id, &self.agents.owner())
            .ok_or_else(|| unknown_agent(&call.id))?;
        live::agent_chat(self, author, &text, at, level_id).await
    }

    /// After a read: the agent's "looking" pointer moves to what it looked
    /// at, when `looked_at` names anything in the plan.
    pub async fn agent_done(&self, call: &AgentCall, looked_at: &[Id]) {
        if !looked_at.is_empty() {
            // A swarm part's agent looks at the draft.
            if let Some(view) = self.swarm.view_for(&call.id) {
                self.agents.place(&call.id, view.project(), looked_at);
            } else {
                let s = self.session.lock().await;
                if let Some(doc) = s.doc.as_ref() {
                    self.agents.place(&call.id, doc.project(), looked_at);
                }
            }
        }
        self.agents.publish();
    }

    /// Set the agent's status line ("Placing doors"), held for 60 s, or clear
    /// it with None so its tools' own lines show again.
    pub fn agent_set_status(&self, call: &AgentCall, text: Option<&str>) {
        self.agents.set_status(&call.id, text);
        self.agents.publish();
    }

    /// Claim levels (ids or names) and elements, so other agents keep out
    /// (DECISIONS D47). A lease: it ends 10 minutes after the agent's last
    /// call, or on release. Never blocks people. Refused (`claimed`) when
    /// another agent's claim meets it by reach (`scope::claims_overlap`: a
    /// level both claim, an element on the other's level, an element both
    /// reach); `not_found` for an id not in the plan. Both lists empty
    /// releases the claim.
    pub async fn agent_claim(&self, call: &AgentCall, level_ids: Vec<Id>, ids: Vec<Id>) -> Result<Option<AgentClaim>, IpcError> {
        let claim = {
            // Checked and set under the session lock, so two agents
            // claiming at once cannot both get the same level.
            let s = self.session.lock().await;
            let doc = s.open_doc()?;
            same_project(call, doc)?;
            // A swarm part's agent claims on the draft it draws in.
            let view = self.swarm.view_for(&call.id);
            let (project, derived) = match &view {
                Some(v) => (v.project(), v.derived()),
                None => (doc.project(), doc.derived()),
            };
            let (levels, ids) = claim_targets(project, &level_ids, &ids)?;
            self.agents.claim(&call.id, project, derived, levels, ids)?
        };
        self.agents.publish();
        Ok(claim)
    }

    /// Give up the agent's claim.
    pub fn agent_release(&self, call: &AgentCall) {
        self.agents.release(&call.id);
        self.agents.publish();
    }

    /// What the agent should hear with the result of this call (r5 3.4),
    /// so agents learn of each other without polling: whether it may edit
    /// (`activity`), chat that mentions it ("@Ground floor" or
    /// "@ground-floor") since its call before this one, at most
    /// `MAX_INBOX`, and other people's and agents' steps inside its claim
    /// since then, at most `MAX_NEARBY`, newest last. Works with or without
    /// a live session.
    pub async fn agent_notes(&self, call: &AgentCall) -> AgentNotes {
        let owner = self.agents.owner();
        let Some((since, handle, claim, told)) = self.agents.notes_window(&call.id) else {
            return AgentNotes::default();
        };
        let mut keys: Vec<String> = vec![];
        let activity = self.agents_list().into_iter().find(|a| a.participant.id == call.id).map(|a| a.activity);
        let mentioned = |text: &str| {
            let text = text.to_lowercase();
            let name = call.name.to_lowercase();
            [format!("@{name}"), format!("@{}", name.replace(' ', "-")), format!("@{handle}")]
                .iter()
                .any(|m| m.len() > 1 && text.contains(m.as_str()))
        };
        let mut inbox: Vec<InboxNote> = vec![];
        for m in self.chat_list().await.unwrap_or_default() {
            let key = format!("chat:{}", m.id);
            if m.author_id != call.id && at_or_after(&m.sent_at, &since) && mentioned(&m.text) && !told.contains(&key) {
                keys.push(key);
                inbox.push(InboxNote { from: m.author_name, text: m.text, sent_at: m.sent_at });
            }
        }
        inbox.drain(..inbox.len().saturating_sub(MAX_INBOX));
        let mut nearby: Vec<NearbyNote> = vec![];
        if self.swarm.part_of(&call.id).is_some() {
            // A swarm part hears of the other parts, not the model's history.
            nearby = crate::swarm::nearby(self, call);
        } else if let Some((levels, ids)) = claim {
            let s = self.session.lock().await;
            if let Some(doc) = s.doc.as_ref() {
                let history = self.live.history(doc, s.live_copy.as_ref());
                // Oldest first, so the newest come last.
                for step in history.undo.iter().rev() {
                    let touched: Vec<Id> = step.ids.iter().chain(&step.removed).cloned().collect();
                    let key = format!("step:{}|{}|{}", step.at, step.label, touched.join(","));
                    let old = !at_or_after(&step.at, &since) || told.contains(&key);
                    if step.author_id.as_ref() == Some(&call.id) || old {
                        continue;
                    }
                    let inside = guhit_core::scope::claims_overlap(doc.project(), doc.derived(), (&levels, &ids), (&[], &touched))
                        .is_some()
                        || touched.iter().any(|id| ids.contains(id));
                    if !inside {
                        continue;
                    }
                    // No author is this computer's own person.
                    let who = match (&step.author_name, &step.author_id) {
                        (Some(name), _) => name.clone(),
                        (None, None) => who_set(&owner),
                        (None, Some(_)) => "Someone".to_string(),
                    };
                    keys.push(key);
                    nearby.push(NearbyNote { who, what: step.label.clone(), at: step.at.clone(), ids: touched });
                }
            }
        }
        nearby.drain(..nearby.len().saturating_sub(MAX_NEARBY));
        self.agents.told(&call.id, keys);
        AgentNotes { activity, inbox, nearby }
    }

    /// Every agent working on the project now: this computer's, and in a
    /// live session everyone else's, newest call first.
    pub fn agents_list(&self) -> Vec<AgentSummary> {
        self.agents.all(&self.agents.owner())
    }
}

/// What an agent should hear with a result (`AppService::agent_notes`).
/// Empty lists mean nothing new.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct AgentNotes {
    /// Working, looking, idle, paused or stopped: whether it may edit. None
    /// for an agent this computer does not know.
    pub activity: Option<AgentActivity>,
    pub inbox: Vec<InboxNote>,
    pub nearby: Vec<NearbyNote>,
}

/// A chat message that mentions the agent.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct InboxNote {
    pub from: String,
    pub text: String,
    pub sent_at: String,
}

/// Someone else's step inside the agent's claim.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct NearbyNote {
    /// Who made it: a person's or another agent's name.
    pub who: String,
    /// Its undo label: "MCP: add walls".
    pub what: String,
    pub at: String,
    /// What it added, changed and removed.
    pub ids: Vec<Id>,
}

// ================================================================= the rules

/// Chat messages in one call's notes, the newest.
pub const MAX_INBOX: usize = 3;
/// Steps in one call's notes, the newest.
pub const MAX_NEARBY: usize = 5;
/// Notes an agent remembers it was told, so none comes twice.
const MAX_TOLD: usize = 64;

/// Idle after this long without a call.
pub const IDLE_AFTER: Duration = Duration::from_secs(60);
/// Off the list after this long without a call (unless paused or stopped),
/// remembered for history while the app runs.
pub const OFF_LIST_AFTER: Duration = Duration::from_secs(10 * 60);
/// A claim ends this long after the agent's last call.
pub const CLAIM_LEASE: Duration = Duration::from_secs(10 * 60);
/// A status line an agent set itself shows this long.
pub const STATUS_HOLD: Duration = Duration::from_secs(60);
/// The first agent edit after this long without one saves a version first.
pub const VERSION_AFTER_QUIET: Duration = Duration::from_secs(10 * 60);
/// Agents on one computer at once.
pub const MAX_AGENTS: usize = 8;
/// Commits per agent within one second.
pub const MAX_COMMITS_PER_SECOND: usize = 5;
/// Agents a guest may show to the session.
pub const MAX_REMOTE_AGENTS: usize = 8;
/// settings.json: names people gave agents, by what the agent was named for.
pub const NAMES_KEY: &str = "agent_names";
/// The live session feature that carries agents (`Hello.features`).
pub const FEATURE: &str = "agents";

/// An agent's work area is at least this big, so following it never zooms
/// in on one door.
const MIN_AREA_W_MM: f64 = 8000.0;
const MIN_AREA_H_MM: f64 = 6000.0;
/// Room around what it touched, in its work area.
const AREA_MARGIN_MM: f64 = 1500.0;
/// Its 3D pointer floats this high above the floor of its level.
const POINTER_HEIGHT_MM: f64 = 1200.0;
/// Most ids an agent's presence selection carries.
const MAX_FOCUS: usize = 200;
/// Most ids a claim holds.
const MAX_CLAIM_IDS: usize = 2000;
/// How often the registry looks for agents going idle and claims ending.
const TICK: Duration = Duration::from_secs(1);
const MAX_HINT_CHARS: usize = 64;

/// What a person set an agent to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Run {
    Active,
    Paused,
    Stopped,
}

/// The person agents on this computer work for, as of now.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Owner {
    /// Their live session participant id. None outside a session.
    pub(crate) id: Option<Id>,
    pub(crate) name: String,
}

impl Run {
    /// The stricter of two: stopped, then paused, then active.
    fn max(self, other: Run) -> Run {
        let rank = |r: Run| match r {
            Run::Active => 0,
            Run::Paused => 1,
            Run::Stopped => 2,
        };
        if rank(other) > rank(self) {
            other
        } else {
            self
        }
    }
}

/// One connection an agent calls on (`AgentHint::connection`). What a
/// person sets holds for every handle on it, the ones to come included:
/// pausing or limiting one subagent cannot be dodged by naming another.
struct Conn {
    run: Run,
    limit: Option<AgentLimit>,
    /// Commits of every handle on it within the last second.
    commits: VecDeque<Instant>,
    /// Client and label: a connection that takes another's place (a stdio
    /// proxy started again) starts the way the newest one with the same
    /// lineage is.
    lineage: String,
    opened: Instant,
}

struct Agent {
    id: Id,
    /// Its connection's key in `Registry::conns`.
    conn: String,
    /// What it was named for, as the rename key in settings.json:
    /// "agent:ground-floor", "label:upper", "client:codex".
    rename_key: String,
    name: String,
    client: String,
    client_version: String,
    color: u8,
    /// Its last call edited (Working) or read (Looking).
    edited_last: bool,
    /// From its last tool.
    tool_status: Option<String>,
    /// From `set_status`, with when.
    status: Option<(String, Instant)>,
    presence: Presence,
    /// Levels and elements; the lease runs from `last_call`.
    claim: Option<(Vec<Id>, Vec<Id>)>,
    steps: u32,
    /// Its latest call, refused ones included: a stopped agent that keeps
    /// trying stays on the list.
    last_call: Instant,
    last_call_at: String,
    /// Its latest call that went through, and the one before: the notes of a
    /// call (`agent_notes`) cover what happened since `notes_since`.
    last_ok_at: String,
    notes_since: String,
    /// Chat messages and steps its notes already told it of (by key), the
    /// newest `MAX_TOLD`: times have whole seconds, so the window alone
    /// would tell some twice.
    told: VecDeque<String>,
    /// The project open at its latest call. Once it is idle, an agent of
    /// another project leaves the list (`Registry::shows`).
    project: Option<Id>,
}

struct Registry {
    /// Every agent of this computer since the app started, oldest first.
    agents: Vec<Agent>,
    /// Every connection agents called on, by key.
    conns: std::collections::HashMap<String, Conn>,
    /// What a person set for every agent at once (`agent_pause` with no
    /// id): new connections start this way until everyone is resumed.
    global: Run,
    /// The latest edit by any agent and its project, for the automatic
    /// version.
    last_edit: Option<(Id, Instant)>,
    /// Agents of other computers in the live session: on the host by guest
    /// id, on a guest under "" (from the host).
    remote: BTreeMap<Id, Vec<AgentSummary>>,
    /// The list the window was sent last.
    published: Option<Vec<AgentSummary>>,
    /// The project agents' limits and claims were last fitted to.
    project: Option<Id>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            agents: vec![],
            conns: std::collections::HashMap::new(),
            global: Run::Active,
            last_edit: None,
            remote: BTreeMap::new(),
            published: None,
            project: None,
        }
    }
}

/// The agents registry of one `AppService`, shared by its clones.
pub struct AgentsState {
    reg: Mutex<Registry>,
    data_dir: PathBuf,
    /// Hashed into every agent id, so two computers in one session never
    /// give the same agent id.
    salt: String,
    events: broadcast::Sender<AppEvent>,
    live: Weak<LiveState>,
    /// Added to the clock, for tests that cannot wait 10 minutes.
    skew: Mutex<Duration>,
    ticking: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn unknown_agent(id: &str) -> IpcError {
    IpcError::new("not_found", format!("No agent {id} works on this computer."))
}

/// What an agent commits: a staged batch, or one command.
enum Edit {
    Staged(Box<Staging>),
    One(Box<Command>),
}

impl AgentsState {
    pub(crate) fn new(data_dir: PathBuf, events: broadcast::Sender<AppEvent>, live: &Arc<LiveState>) -> Self {
        Self {
            reg: Mutex::new(Registry::default()),
            data_dir,
            salt: defaults::new_id(),
            events,
            live: Arc::downgrade(live),
            skew: Mutex::new(Duration::ZERO),
            ticking: AtomicBool::new(false),
        }
    }

    /// The registry's clock.
    pub(crate) fn now(&self) -> Instant {
        Instant::now() + *lock(&self.skew)
    }

    /// Move the registry's clock on, as if `by` passed: agents go idle,
    /// claims end, the automatic version comes due. For tests.
    #[doc(hidden)]
    pub fn advance_clock(&self, by: Duration) {
        *lock(&self.skew) += by;
        self.publish();
    }

    /// UTC wall time on the registry's clock, `at` from now.
    fn utc_at(&self, at: Instant) -> String {
        let now = Instant::now();
        let skew = *lock(&self.skew);
        let ahead = at.saturating_duration_since(now);
        let behind = now.saturating_duration_since(at);
        let t = chrono::Utc::now() + chrono::Duration::from_std(skew + ahead).unwrap_or_default()
            - chrono::Duration::from_std(behind).unwrap_or_default();
        t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    }

    /// The person this computer's agents work for: the host or this guest
    /// in a live session, else the profile name.
    pub(crate) fn owner(&self) -> Owner {
        if let Some(live) = self.live.upgrade() {
            let status = live.status();
            if let Some(me) = status.self_id.as_ref().and_then(|id| status.participants.iter().find(|p| &p.id == id)) {
                return Owner { id: Some(me.id.clone()), name: me.name.clone() };
            }
        }
        let name = files::read_settings(&self.data_dir)
            .get("profile_name")
            .and_then(Value::as_str)
            .map(live::clean_name)
            .unwrap_or_default();
        Owner { id: None, name }
    }

    /// One look per `TICK` for agents going idle or off the list and claims
    /// ending, while the app runs. Started by the first agent.
    fn start_ticking(self: &Arc<Self>) {
        if self.ticking.swap(true, Ordering::SeqCst) {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            self.ticking.store(false, Ordering::SeqCst);
            return;
        };
        let weak = Arc::downgrade(self);
        runtime.spawn(async move {
            let mut tick = tokio::time::interval(TICK);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                let Some(state) = weak.upgrade() else { return };
                state.publish();
            }
        });
    }

    // ------------------------------------------------------------ identity

    fn call(
        &self,
        hint: &AgentHint,
        tool: &str,
        kind: CallKind,
        owner: &Owner,
        project_id: Option<Id>,
    ) -> Result<AgentCall, IpcError> {
        let who = Who::of(hint);
        let id = agent_id(&self.salt, &who.key);
        let now = self.now();
        let mut reg = lock(&self.reg);
        let at = self.utc_at(now);
        if !reg.conns.contains_key(&who.connection) {
            let (run, limit) = reg.inherited(&who.lineage);
            let conn = Conn { run, limit, commits: VecDeque::new(), lineage: who.lineage.clone(), opened: now };
            reg.conns.insert(who.connection.clone(), conn);
        }
        let run = reg.conns.get(&who.connection).map(|c| c.run).unwrap_or(Run::Active);
        // A stopped agent takes no place among the 8: it is refused anyway.
        let needs_place = run != Run::Stopped;
        let index = match reg.agents.iter().position(|a| a.id == id) {
            Some(i) => {
                let counted = reg.counted(now).filter(|a| a.id != id).count();
                if needs_place && !reg.counts(&reg.agents[i], now) && counted >= MAX_AGENTS {
                    return Err(too_many());
                }
                i
            }
            None => {
                if needs_place && reg.counted(now).count() >= MAX_AGENTS {
                    return Err(too_many());
                }
                let names = files::read_settings(&self.data_dir)
                    .get(NAMES_KEY)
                    .and_then(|v| v.get(&who.rename_key))
                    .and_then(Value::as_str)
                    .map(live::clean_name)
                    .filter(|n| !n.is_empty());
                let base = names.unwrap_or_else(|| who.name.clone());
                let name = reg.unique_name(&base, now);
                let color = reg.free_color(now);
                reg.agents.push(Agent {
                    id: id.clone(),
                    conn: who.connection.clone(),
                    rename_key: who.rename_key.clone(),
                    name,
                    client: who.client.clone(),
                    client_version: who.client_version.clone(),
                    color,
                    edited_last: false,
                    tool_status: None,
                    status: None,
                    presence: Presence::default(),
                    claim: None,
                    steps: 0,
                    last_call: now,
                    last_call_at: at.clone(),
                    last_ok_at: at.clone(),
                    notes_since: at.clone(),
                    told: VecDeque::new(),
                    project: project_id.clone(),
                });
                reg.agents.len() - 1
            }
        };
        let agent = &mut reg.agents[index];
        // Refused calls count as calls: the agent shows while it tries.
        agent.last_call = now;
        agent.last_call_at = at.clone();
        agent.project = project_id.clone();
        match (run, kind) {
            (Run::Stopped, _) => return Err(stopped(owner, &agent.name)),
            (Run::Paused, CallKind::Edit | CallKind::Work) => return Err(paused(owner, &agent.name)),
            _ => {}
        }
        if !who.client.is_empty() {
            agent.client = who.client;
            agent.client_version = who.client_version;
        }
        agent.edited_last = kind != CallKind::Read;
        agent.tool_status = tool_status(tool).map(str::to_string);
        agent.notes_since = std::mem::replace(&mut agent.last_ok_at, at);
        let call = AgentCall { id: agent.id.clone(), name: agent.name.clone(), kind, project_id };
        let participant = agent.participant(owner);
        drop(reg);
        self.remember(&participant, owner);
        Ok(call)
    }

    /// Names, colors and owners for the history (DECISIONS D38).
    fn remember(&self, participant: &Participant, owner: &Owner) {
        if let Some(live) = self.live.upgrade() {
            live.remember_agent(participant, None, &owner.name);
        }
    }

    pub(crate) fn participant(&self, id: &str, owner: &Owner) -> Option<Participant> {
        lock(&self.reg).agents.iter().find(|a| a.id == id).map(|a| a.participant(owner))
    }

    /// Show an agent as the swarm part it draws (DECISIONS D56): the part's
    /// name and color, on the list, its cursor and its history steps. Kept
    /// in memory only, never saved as a rename.
    pub(crate) fn show_as(&self, id: &str, name: &str, color: u8) {
        let owner = self.owner();
        let participant = {
            let mut reg = lock(&self.reg);
            let Some(agent) = reg.agents.iter_mut().find(|a| a.id == id) else { return };
            if agent.name == name && agent.color == color {
                return;
            }
            agent.name = name.to_string();
            agent.color = color;
            agent.participant(&owner)
        };
        self.remember(&participant, &owner);
    }

    // --------------------------------------------------------------- rules

    /// Refuse an edit of a paused or stopped agent, and a sixth commit
    /// within a second on its connection (every handle on it together).
    pub(crate) fn check_edit(&self, id: &str, now: Instant) -> Result<(), IpcError> {
        let owner = self.owner();
        let mut reg = lock(&self.reg);
        let Registry { agents, conns, .. } = &mut *reg;
        let agent = agents.iter().find(|a| a.id == id).ok_or_else(|| unknown_agent(id))?;
        let conn = conns.get_mut(&agent.conn).ok_or_else(|| unknown_agent(id))?;
        match conn.run {
            Run::Stopped => return Err(stopped(&owner, &agent.name)),
            Run::Paused => return Err(paused(&owner, &agent.name)),
            Run::Active => {}
        }
        while conn.commits.front().is_some_and(|t| now.saturating_duration_since(*t) >= Duration::from_secs(1)) {
            conn.commits.pop_front();
        }
        if conn.commits.len() >= MAX_COMMITS_PER_SECOND {
            return Err(IpcError::new(
                "rate_limited",
                format!(
                    "{} made {MAX_COMMITS_PER_SECOND} changes in the last second. Wait a moment, or put several changes in one batch.",
                    agent.name
                ),
            ));
        }
        Ok(())
    }

    /// Refuse an edit outside the agent's limit, or reaching another
    /// agent's claim. `base` is the plan the command runs on, `staged` the
    /// plan after it.
    pub(crate) fn check_reach(
        &self,
        id: &str,
        base: &Project,
        derived: &Derived,
        staged: &Project,
        command: &Command,
    ) -> Result<(), IpcError> {
        // Only people set license numbers (D53): refused for every agent,
        // whatever its limit. Every agent edit path checks its reach here.
        crate::people_only(command)?;
        let owner = self.owner();
        let now = self.now();
        let (limit, name, claims) = {
            let reg = lock(&self.reg);
            let agent = reg.agents.iter().find(|a| a.id == id);
            let limit = agent.and_then(|a| reg.conns.get(&a.conn)).and_then(|c| c.limit.clone());
            let name = agent.map(|a| a.name.clone()).unwrap_or_default();
            (limit, name, reg.claims_of_others(id, now, &self.utc_at(now)))
        };
        if let Some(limit) = &limit {
            guhit_core::scope::check_limit(base, derived, staged, limit, command).map_err(|e| {
                // The engine's own code: `out_of_scope`, as D30 refuses.
                let code = match &e {
                    guhit_core::CoreError::Invalid { code, .. } => Some(code.to_string()),
                    _ => None,
                };
                let mut e = IpcError::from(e);
                if let Some(code) = code {
                    e.code = code;
                }
                e.message = format!(
                    "{} {} limited {name} to {}. Change only those, or ask the user to widen the limit in the Agents panel.",
                    e.message,
                    who_set(&owner),
                    describe_limit(base, derived, limit)
                );
                e
            })?;
        }
        check_claims(base, derived, staged, &claims, command)
    }

    /// Who may not be reached: every other agent's live claim, with its
    /// name, client and when it ends.
    fn claims_check_remote(&self, author: &str, base: &Project, derived: &Derived, staged: &Project, command: &Command) -> Result<(), IpcError> {
        let now = self.now();
        let claims = lock(&self.reg).claims_of_others(author, now, &self.utc_at(now));
        check_claims(base, derived, staged, &claims, command)
    }

    /// An agent's commit went in.
    pub(crate) fn committed(&self, id: &str, result: &ApplyResult, now: Instant) {
        let mut reg = lock(&self.reg);
        if let Some(last) = reg.last_edit.as_mut() {
            last.1 = now;
        }
        let Registry { agents, conns, .. } = &mut *reg;
        if let Some(agent) = agents.iter_mut().find(|a| a.id == id) {
            agent.steps += 1;
            agent.edited_last = true;
            if let Some(conn) = conns.get_mut(&agent.conn) {
                conn.commits.push_back(now);
            }
        }
        drop(reg);
        let touched: Vec<Id> = result.diff.added.iter().chain(&result.diff.modified).cloned().collect();
        self.place(id, &result.state.project, &touched);
    }

    /// Move an agent's presence to `ids` on `project`: pointer at their
    /// middle on their level, selection them, 3D pointer above, work area
    /// around them. True when it moved.
    fn place(&self, id: &str, project: &Project, ids: &[Id]) -> bool {
        let Some(area) = guhit_core::scope::work_area(project, ids) else { return false };
        let elevation = project.levels.iter().find(|l| l.id == area.level_id).map(|l| l.elevation_mm).unwrap_or(0.0);
        let mut focus: Vec<Id> =
            ids.iter().filter(|i| project.elements.iter().any(|e| e.id() == *i)).cloned().collect();
        focus.dedup();
        focus.truncate(MAX_FOCUS);
        let presence = live::clean_presence(Presence {
            cursor: Some(area.center),
            level_id: Some(area.level_id.clone()),
            selection: focus,
            pointer_3d: Some(Vec3 { x: area.center.x, y: area.center.y, z: elevation + POINTER_HEIGHT_MM }),
            plan_view: Some(PlanView {
                center: area.center,
                width_mm: (area.max.x - area.min.x + 2.0 * AREA_MARGIN_MM).max(MIN_AREA_W_MM),
                height_mm: (area.max.y - area.min.y + 2.0 * AREA_MARGIN_MM).max(MIN_AREA_H_MM),
            }),
            ..Presence::default()
        });
        let mut reg = lock(&self.reg);
        let Some(agent) = reg.agents.iter_mut().find(|a| a.id == id) else { return false };
        let moved = agent.presence != presence;
        agent.presence = presence;
        moved
    }

    fn set_status(&self, id: &str, text: Option<&str>) {
        let now = self.now();
        let mut reg = lock(&self.reg);
        if let Some(agent) = reg.agents.iter_mut().find(|a| a.id == id) {
            agent.status = text
                .map(|t| t.chars().filter(|c| !c.is_control()).take(MAX_STATUS_CHARS).collect::<String>())
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .map(|t| (t, now));
        }
    }

    /// Set an agent's claim, unless it meets another agent's claim by reach
    /// on `project`: a level both claim, an element on a level the other
    /// claims, an element both reach.
    fn claim(
        &self,
        id: &str,
        project: &Project,
        derived: &Derived,
        level_ids: Vec<Id>,
        ids: Vec<Id>,
    ) -> Result<Option<AgentClaim>, IpcError> {
        if level_ids.is_empty() && ids.is_empty() {
            self.release(id);
            return Ok(None);
        }
        let now = self.now();
        let utc_now = self.utc_at(now);
        let mut reg = lock(&self.reg);
        for other in reg.claims_of_others(id, now, &utc_now) {
            let met = guhit_core::scope::claims_overlap(
                project,
                derived,
                (&level_ids, &ids),
                (&other.level_ids, &other.ids),
            );
            if let Some(what) = met {
                return Err(IpcError::new(
                    "claimed",
                    format!(
                        "{what} is claimed by {} until {}. Claim something else, or ask {} in the chat to release it.",
                        other.who,
                        local_time(&other.expires_at),
                        other.name
                    ),
                ));
            }
        }
        let agent = reg.agents.iter_mut().find(|a| a.id == id).ok_or_else(|| unknown_agent(id))?;
        agent.claim = Some((level_ids.clone(), ids.clone()));
        let expires_at = agent.lease_end_at();
        Ok(Some(AgentClaim { level_ids, ids, expires_at }))
    }

    fn release(&self, id: &str) {
        if let Some(agent) = lock(&self.reg).agents.iter_mut().find(|a| a.id == id) {
            agent.claim = None;
        }
    }

    /// Refuse undoing or redoing someone else's step: another agent's
    /// always, a person's unless `force`.
    fn refuse_step(
        &self,
        live: &LiveState,
        author: Option<&Id>,
        label: Option<&str>,
        redo: bool,
        force: bool,
    ) -> Result<(), IpcError> {
        let label = label.filter(|l| !l.is_empty()).unwrap_or("a change");
        let name = author.and_then(|a| live.name_of(a)).unwrap_or_else(|| "the user".to_string());
        if author.is_some_and(|a| live.is_agent(a)) {
            let message = if redo {
                format!("The step to redo is {name}'s, another agent's: {label}. Agents never undo or redo each other's steps; ask {name} in the chat, or ask the user to use the History panel.")
            } else {
                format!("The last change is {name}'s, another agent's: {label}. Agents never undo each other's steps; ask {name} in the chat, or ask the user to use the History panel.")
            };
            return Err(IpcError::new("other_author", message));
        }
        if force {
            return Ok(());
        }
        let message = if redo {
            format!("The step to redo is {name}'s: {label}. You undo and redo only your own steps. Ask the user; pass force true only after they confirm.")
        } else {
            format!("The last change is {name}'s: {label}. You undo only your own steps. Ask the user; pass force true only after they confirm.")
        };
        Err(IpcError::new("other_author", message))
    }

    // ---------------------------------------------------- the window's calls

    fn update(&self, id: &str, name: Option<String>, limit: Option<Option<AgentLimit>>) -> Result<(), IpcError> {
        let mut reg = lock(&self.reg);
        let agent = match reg.agents.iter_mut().find(|a| a.id == id) {
            Some(agent) => agent,
            None => return Err(not_here(&reg, id)),
        };
        if let Some(name) = name {
            let name = live::clean_name(&name);
            if name.is_empty() {
                return Err(IpcError::new("bad_args", "An agent's name needs at least one letter or number."));
            }
            agent.name = name.clone();
            let key = agent.rename_key.clone();
            drop(reg);
            let mut names = files::read_settings(&self.data_dir)
                .get(NAMES_KEY)
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            names.insert(key, json!(name));
            files::put_setting(&self.data_dir, NAMES_KEY, Some(Value::Object(names)))?;
            reg = lock(&self.reg);
        }
        // A limit holds for the agent's whole connection: another handle on
        // it is held to it too.
        if let Some(limit) = limit {
            let key = reg.agents.iter().find(|a| a.id == id).map(|a| a.conn.clone()).ok_or_else(|| unknown_agent(id))?;
            if let Some(conn) = reg.conns.get_mut(&key) {
                conn.limit = limit.filter(|l| !(l.level_ids.is_empty() && l.ids.is_empty() && l.kinds.is_empty()));
            }
        }
        let owner_name = self.owner_name_cached();
        let participant = reg.agents.iter().find(|a| a.id == id).map(|a| a.participant(&Owner { id: None, name: owner_name.clone() }));
        drop(reg);
        if let (Some(p), Some(live)) = (participant, self.live.upgrade()) {
            live.remember_agent(&p, None, &owner_name);
        }
        Ok(())
    }

    fn owner_name_cached(&self) -> String {
        files::read_settings(&self.data_dir)
            .get("profile_name")
            .and_then(Value::as_str)
            .map(live::clean_name)
            .unwrap_or_default()
    }

    /// Pause, stop or resume one agent with its whole connection (every
    /// handle on it, the ones to come included), or with None every agent of
    /// this computer, and every agent that connects until all are resumed.
    /// Stopping ends the claims it covers.
    /// A pause never loosens a stop; resuming one agent needs a free place
    /// among the 8 (resuming everyone brings everyone back).
    fn set_run(&self, id: Option<&str>, run: Run) -> Result<(), IpcError> {
        let now = self.now();
        let mut reg = lock(&self.reg);
        let set = |current: Run| if run == Run::Active { Run::Active } else { current.max(run) };
        let key = match id {
            Some(id) => match reg.agents.iter().find(|a| a.id == id) {
                Some(agent) => {
                    let freed = reg.run_of(agent) == Run::Stopped && run != Run::Stopped;
                    let siblings = reg.agents.iter().filter(|a| a.conn == agent.conn && reg.shows(a, now)).count();
                    if freed && reg.counted(now).count() + siblings > MAX_AGENTS {
                        return Err(IpcError::new(
                            "too_many_agents",
                            format!("{MAX_AGENTS} agents are working already. Stop another one before you resume {}.", agent.name),
                        ));
                    }
                    Some(agent.conn.clone())
                }
                None => return Err(not_here(&reg, id)),
            },
            None => {
                reg.global = set(reg.global);
                None
            }
        };
        for (_, conn) in reg.conns.iter_mut().filter(|(k, _)| key.as_ref().is_none_or(|key| key == *k)) {
            conn.run = set(conn.run);
        }
        if run == Run::Stopped {
            for agent in reg.agents.iter_mut().filter(|a| key.as_ref().is_none_or(|key| *key == a.conn)) {
                agent.claim = None;
            }
        }
        Ok(())
    }

    // ----------------------------------------------------------- the lists

    /// This computer's agents on the list, newest call first.
    fn local(&self, owner: &Owner) -> Vec<AgentSummary> {
        let now = self.now();
        let reg = lock(&self.reg);
        let mut list: Vec<(Instant, AgentSummary)> =
            reg.listed(now).map(|a| (a.last_call, self.summary(&reg, a, now, owner))).collect();
        list.sort_by(|a, b| b.0.cmp(&a.0));
        list.into_iter().map(|(_, s)| s).collect()
    }

    /// Everyone's agents, newest call first.
    pub(crate) fn all(&self, owner: &Owner) -> Vec<AgentSummary> {
        let mut all = self.local(owner);
        let remote: Vec<AgentSummary> = lock(&self.reg).remote.values().flatten().cloned().collect();
        all.extend(remote);
        all.sort_by(|a, b| b.last_call_at.cmp(&a.last_call_at));
        all
    }

    fn summary(&self, reg: &Registry, a: &Agent, now: Instant, owner: &Owner) -> AgentSummary {
        let idle = now.saturating_duration_since(a.last_call) >= IDLE_AFTER;
        let conn = reg.conns.get(&a.conn);
        let run = reg.run_of(a);
        let activity = match run {
            Run::Stopped => AgentActivity::Stopped,
            Run::Paused => AgentActivity::Paused,
            Run::Active if idle => AgentActivity::Idle,
            Run::Active if a.edited_last => AgentActivity::Working,
            Run::Active => AgentActivity::Looking,
        };
        let held = a.status.as_ref().filter(|(_, at)| now.saturating_duration_since(*at) < STATUS_HOLD);
        let mut presence = a.presence.clone();
        presence.status = match held {
            Some((text, _)) => Some(text.clone()),
            None if !idle && run == Run::Active => a.tool_status.clone(),
            None => None,
        };
        let lease_ends = a.last_call + CLAIM_LEASE;
        let claim = a.claim.as_ref().filter(|_| now < lease_ends).map(|(levels, ids)| AgentClaim {
            level_ids: levels.clone(),
            ids: ids.clone(),
            expires_at: a.lease_end_at(),
        });
        AgentSummary {
            participant: a.participant(owner),
            presence,
            activity,
            limit: conn.and_then(|c| c.limit.clone()),
            claim,
            steps: a.steps,
            last_call_at: a.last_call_at.clone(),
        }
    }

    /// Send the list to the window when it changed, and to the live session.
    pub(crate) fn publish(&self) {
        self.publish_with(false);
    }

    /// `publish` even when nothing changed: after joining or rejoining, so
    /// the host has this computer's agents.
    pub(crate) fn publish_with(&self, force: bool) {
        let owner = self.owner();
        let local = self.local(&owner);
        let (all, remote) = {
            let mut reg = lock(&self.reg);
            let mut all = local.clone();
            all.extend(reg.remote.values().flatten().cloned());
            all.sort_by(|a, b| b.last_call_at.cmp(&a.last_call_at));
            if !force && reg.published.as_ref() == Some(&all) {
                return;
            }
            reg.published = Some(all.clone());
            (all, reg.remote.clone())
        };
        let _ = self.events.send(AppEvent::Agents { agents: all });
        if let Some(live) = self.live.upgrade() {
            live::send_agents(&live, &local, &remote);
        }
    }

    /// The agents another computer in the live session shows: on the host a
    /// guest's (`source` its participant id), on a guest everyone else's
    /// (`source` ""). Empty drops them.
    pub(crate) fn set_remote(&self, source: &str, agents: Vec<AgentSummary>) {
        {
            let mut reg = lock(&self.reg);
            if agents.is_empty() {
                reg.remote.remove(source);
            } else {
                reg.remote.insert(source.to_string(), agents);
            }
        }
        self.publish();
    }

    /// Every other computer's agents dropped: the session ended.
    pub(crate) fn clear_remote(&self) {
        let had = !std::mem::take(&mut lock(&self.reg).remote).is_empty();
        if had {
            self.publish();
        }
    }

    /// The agent `agent` of the guest `guest`, when that guest shows it.
    pub(crate) fn remote_agent(&self, guest: &str, agent: &str) -> Option<Participant> {
        lock(&self.reg)
            .remote
            .get(guest)
            .and_then(|list| list.iter().find(|s| s.participant.id == agent))
            .map(|s| s.participant.clone())
    }

    /// For an agent's notes: since when, the handle it is mentioned by, and
    /// its live claim.
    #[allow(clippy::type_complexity)]
    fn notes_window(&self, id: &str) -> Option<(String, String, Option<(Vec<Id>, Vec<Id>)>, Vec<String>)> {
        let now = self.now();
        let reg = lock(&self.reg);
        let agent = reg.agents.iter().find(|a| a.id == id)?;
        let handle = agent.rename_key.split_once(':').map(|(_, h)| h.to_string()).unwrap_or_default();
        let claim = agent.claim.clone().filter(|_| now < agent.last_call + CLAIM_LEASE);
        Some((agent.notes_since.clone(), handle, claim, agent.told.iter().cloned().collect()))
    }

    /// Its notes told it of `keys`: the newest `MAX_TOLD` are kept.
    fn told(&self, id: &str, keys: Vec<String>) {
        if let Some(agent) = lock(&self.reg).agents.iter_mut().find(|a| a.id == id) {
            agent.told.extend(keys);
            while agent.told.len() > MAX_TOLD {
                agent.told.pop_front();
            }
        }
    }

    // --------------------------------------------------- another project

    /// A project opened, or the open one changed: limits and claims keep
    /// only the levels it has, and when it is another project only its
    /// elements too (elements are looked up only then, so a commit stays
    /// cheap). Called after every change.
    ///
    /// A limit left naming no level and no element of the new project would
    /// limit the agent to nothing, and dropping it would let the agent work
    /// unlimited on a project nobody limited it on. So that agent's
    /// connection is paused instead, its limit cleared (kinds kept, they
    /// hold in any project): the panel shows it paused, and the user sets a
    /// new limit or resumes it. A claim keeps what the project has, or ends.
    pub(crate) fn project_opened(&self, project: &Project) {
        let changed = {
            let mut reg = lock(&self.reg);
            let switched = reg.project.as_deref() != Some(project.id.as_str());
            reg.project = Some(project.id.clone());
            let has_level = |id: &Id| project.levels.iter().any(|l| &l.id == id);
            let has_element = |id: &Id| !switched || project.elements.iter().any(|e| e.id() == id);
            let mut changed = false;
            for conn in reg.conns.values_mut() {
                let Some(limit) = conn.limit.as_mut() else { continue };
                let placed = !limit.level_ids.is_empty() || !limit.ids.is_empty();
                let before = (limit.level_ids.len(), limit.ids.len());
                limit.level_ids.retain(has_level);
                limit.ids.retain(has_element);
                if before == (limit.level_ids.len(), limit.ids.len()) {
                    continue;
                }
                changed = true;
                if placed && limit.level_ids.is_empty() && limit.ids.is_empty() {
                    conn.run = conn.run.max(Run::Paused);
                }
                if limit.level_ids.is_empty() && limit.ids.is_empty() && limit.kinds.is_empty() {
                    conn.limit = None;
                }
            }
            for agent in reg.agents.iter_mut() {
                let Some((levels, ids)) = agent.claim.as_mut() else { continue };
                let before = (levels.len(), ids.len());
                levels.retain(has_level);
                ids.retain(has_element);
                if before != (levels.len(), ids.len()) {
                    changed = true;
                    if levels.is_empty() && ids.is_empty() {
                        agent.claim = None;
                    }
                }
            }
            changed
        };
        if changed {
            self.publish();
        }
    }

    // ------------------------------------------------------ the auto version

    /// The first agent edit after `VERSION_AFTER_QUIET` without one saves a
    /// version of the plan as it is first: "Before agents, 3:42 PM". Best
    /// effort: a version that cannot be written never stops the edit.
    /// Every agent edit, this computer's or a guest's, counts as agent work
    /// from now, whether or not it goes in.
    pub(crate) fn version_before(&self, s: &Session) {
        let now = self.now();
        let Some(doc) = s.doc.as_ref() else { return };
        let project = doc.project().id.clone();
        let due = {
            let mut reg = lock(&self.reg);
            let due = reg.last_edit.as_ref().is_none_or(|(p, t)| {
                *p != project || now.saturating_duration_since(*t) >= VERSION_AFTER_QUIET
            });
            reg.last_edit = Some((project, now));
            due
        };
        if !due || s.live_copy.is_some() {
            return;
        }
        let Ok(dir) = store::project_dir(&s.data_dir, &doc.project().id) else { return };
        let skew = chrono::Duration::from_std(*lock(&self.skew)).unwrap_or_default();
        let label = format!("Before agents, {}", (chrono::Local::now() + skew).format("%-I:%M %p"));
        if let Err(e) = snapshots::create(&dir, doc.project(), doc.revision(), &label, true) {
            eprintln!("guhit-app: the version before agents could not be saved: {}", e.message);
        }
    }
}

const MAX_STATUS_CHARS: usize = 80;

impl Registry {
    fn listed(&self, now: Instant) -> impl Iterator<Item = &Agent> {
        self.agents.iter().filter(move |a| self.shows(a, now))
    }

    /// On the list: called within `OFF_LIST_AFTER`, and on the open project,
    /// or another one but not idle yet. An agent that worked on a project
    /// since closed or deleted leaves once it goes idle, so the panel shows
    /// who works on this project (GS F7); it comes back with its next call.
    fn shows(&self, a: &Agent, now: Instant) -> bool {
        a.on_list(now) && (a.project == self.project || now.saturating_duration_since(a.last_call) < IDLE_AFTER)
    }

    /// What a person set for the agent: its connection's.
    fn run_of(&self, a: &Agent) -> Run {
        self.conns.get(&a.conn).map(|c| c.run).unwrap_or(Run::Active)
    }

    /// An agent takes one of the `MAX_AGENTS` places while it is on the list
    /// and not stopped: stopping one frees its place, and it stays in the
    /// history.
    fn counts(&self, a: &Agent, now: Instant) -> bool {
        self.shows(a, now) && self.run_of(a) != Run::Stopped
    }

    fn counted(&self, now: Instant) -> impl Iterator<Item = &Agent> {
        self.agents.iter().filter(move |a| self.counts(a, now))
    }

    /// How a new connection with `lineage` starts: as everyone was set
    /// (`global`), and no looser than the newest connection of the same
    /// client and label, so starting a stdio proxy again keeps its pause,
    /// stop and limit.
    fn inherited(&self, lineage: &str) -> (Run, Option<AgentLimit>) {
        let last = self.conns.values().filter(|c| c.lineage == lineage).max_by_key(|c| c.opened);
        let run = last.map(|c| c.run).unwrap_or(Run::Active).max(self.global);
        (run, last.and_then(|c| c.limit.clone()))
    }

    /// `base`, or "base 2", "base 3" when an agent on the list has it.
    fn unique_name(&self, base: &str, now: Instant) -> String {
        let taken = |name: &str| self.listed(now).any(|a| a.name == name);
        if !taken(base) {
            return base.to_string();
        }
        (2..).map(|n| format!("{base} {n}")).find(|n| !taken(n)).unwrap_or_else(|| base.to_string())
    }

    /// The lowest color no agent on the list has, from 1 (0 is the host's).
    fn free_color(&self, now: Instant) -> u8 {
        let mut uses = [0usize; PEER_COLOR_COUNT as usize];
        uses[0] = usize::MAX / 2;
        for a in self.listed(now) {
            if let Some(n) = uses.get_mut(a.color as usize) {
                *n += 1;
            }
        }
        let fewest = uses.iter().copied().min().unwrap_or(0);
        uses.iter().position(|n| *n == fewest).unwrap_or(1) as u8
    }

    /// Every live claim of an agent other than `id`, this computer's and
    /// the session's.
    fn claims_of_others(&self, id: &str, now: Instant, utc_now: &str) -> Vec<Claim> {
        let mut out = vec![];
        for a in self.agents.iter().filter(|a| a.id != id) {
            if let Some((levels, ids)) = a.claim.as_ref().filter(|_| now < a.last_call + CLAIM_LEASE && self.run_of(a) != Run::Stopped) {
                out.push(Claim {
                    name: a.name.clone(),
                    who: with_client(&a.name, &a.client),
                    level_ids: levels.clone(),
                    ids: ids.clone(),
                    expires_at: a.lease_end_at(),
                });
            }
        }
        for s in self.remote.values().flatten().filter(|s| s.participant.id != id) {
            if let Some(c) = s.claim.as_ref().filter(|c| later(&c.expires_at, utc_now)) {
                let client = s.participant.agent.as_ref().map(|a| a.client.as_str()).unwrap_or("");
                out.push(Claim {
                    name: s.participant.name.clone(),
                    who: with_client(&s.participant.name, client),
                    level_ids: c.level_ids.clone(),
                    ids: c.ids.clone(),
                    expires_at: c.expires_at.clone(),
                });
            }
        }
        out
    }
}

impl Agent {
    /// When its claim ends, RFC 3339 UTC: `CLAIM_LEASE` after its last call.
    /// Worked out from the stored time, so it reads the same on every tick.
    fn lease_end_at(&self) -> String {
        add_secs(&self.last_call_at, CLAIM_LEASE.as_secs() as i64)
    }

    /// On the list while it called in the last `OFF_LIST_AFTER`, refused
    /// calls included. Its connection keeps a pause, stop or limit after.
    fn on_list(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.last_call) < OFF_LIST_AFTER
    }

    fn participant(&self, owner: &Owner) -> Participant {
        Participant {
            id: self.id.clone(),
            name: self.name.clone(),
            color: self.color,
            role: ParticipantRole::Guest,
            agent: Some(AgentInfo {
                owner_id: owner.id.clone(),
                owner_name: owner.name.clone(),
                client: self.client.clone(),
                client_version: self.client_version.clone(),
            }),
        }
    }
}

/// Another agent's claim, ready for a refusal.
struct Claim {
    name: String,
    /// "Upper floor (Cursor)".
    who: String,
    level_ids: Vec<Id>,
    ids: Vec<Id>,
    expires_at: String,
}

fn check_claims(base: &Project, derived: &Derived, staged: &Project, claims: &[Claim], command: &Command) -> Result<(), IpcError> {
    for claim in claims {
        if let Some((what, ids)) = guhit_core::scope::claim_reach(base, derived, staged, &claim.level_ids, &claim.ids, command) {
            let mut e = IpcError::new(
                "claimed",
                format!(
                    "{what} is claimed by {} until {}. Work somewhere else, or ask {} in the chat to release it.",
                    claim.who,
                    local_time(&claim.expires_at),
                    claim.name
                ),
            );
            e.element_ids = ids;
            return Err(e);
        }
    }
    Ok(())
}

/// "Axl", or "The user" before a name is chosen.
fn who_set(owner: &Owner) -> String {
    if owner.name.is_empty() {
        "The user".to_string()
    } else {
        owner.name.clone()
    }
}

fn paused(owner: &Owner, name: &str) -> IpcError {
    IpcError::new(
        "agent_paused",
        format!("{} paused {name}. Stop and tell the user; do not retry until they resume you. Reading the plan still works.", who_set(owner)),
    )
}

fn stopped(owner: &Owner, name: &str) -> IpcError {
    IpcError::new(
        "agent_stopped",
        format!(
            "{} stopped {name} in Guhit Studio. Stop working and tell the user; every call is refused until they resume you in the Agents panel.",
            who_set(owner)
        ),
    )
}

fn too_many() -> IpcError {
    IpcError::new(
        "too_many_agents",
        format!("{MAX_AGENTS} agents are working on this computer already. Ask the user to stop one in the Agents panel, which frees its place at once, or wait until one has been quiet for 10 minutes."),
    )
}

/// Refuse a call's edit once another project is open than the one the call
/// started on.
fn same_project(call: &AgentCall, doc: &guhit_core::Document) -> Result<(), IpcError> {
    match &call.project_id {
        Some(id) if *id != doc.project().id => Err(project_changed(doc)),
        _ => Ok(()),
    }
}

fn project_changed(doc: &guhit_core::Document) -> IpcError {
    IpcError::new(
        "project_changed",
        format!(
            "Another project, \"{}\", was opened while this call ran, so nothing was applied. Check which project is open (get_project_summary) before you edit again.",
            doc.project().name
        ),
    )
}

/// An id that is not this computer's agent: another computer's, or none.
fn not_here(reg: &Registry, id: &str) -> IpcError {
    match reg.remote.values().flatten().find(|s| s.participant.id == id) {
        Some(s) => {
            let owner = s.participant.agent.as_ref().map(|a| a.owner_name.clone()).unwrap_or_default();
            IpcError::new(
                "not_yours",
                format!("{} works on {owner}'s computer, so only {owner} can change, pause or stop it.", s.participant.name),
            )
        }
        None => unknown_agent(id),
    }
}

/// "Ground floor (Codex)", or the name alone when the client says nothing
/// more.
fn with_client(name: &str, client: &str) -> String {
    let client = readable_client(client);
    if client.is_empty() || client == name {
        name.to_string()
    } else {
        format!("{name} ({client})")
    }
}

/// "3:52 PM" on this computer's clock, from RFC 3339 UTC.
fn local_time(rfc3339: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .map(|t| t.with_timezone(&chrono::Local).format("%-I:%M %p").to_string())
        .unwrap_or_else(|_| "later".to_string())
}

/// True when the RFC 3339 time `t` is at or after `since`, counted in whole
/// seconds: chat and history times have no fractions, so a message in the
/// same second as the call before still counts. A time that does not parse
/// counts.
fn at_or_after(t: &str, since: &str) -> bool {
    match (chrono::DateTime::parse_from_rfc3339(t), chrono::DateTime::parse_from_rfc3339(since)) {
        (Ok(t), Ok(since)) => t.timestamp() >= since.timestamp(),
        _ => true,
    }
}

/// True when the RFC 3339 time `a` is after `b`. A time that does not
/// parse is never later: a claim with a broken time holds nothing.
fn later(a: &str, b: &str) -> bool {
    match (chrono::DateTime::parse_from_rfc3339(a), chrono::DateTime::parse_from_rfc3339(b)) {
        (Ok(a), Ok(b)) => a > b,
        _ => false,
    }
}

fn add_secs(rfc3339: &str, secs: i64) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .map(|t| (t.with_timezone(&chrono::Utc) + chrono::Duration::seconds(secs)).format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        .unwrap_or_else(|_| rfc3339.to_string())
}

/// "Ground Floor and walls, doors and windows": a limit in words.
fn describe_limit(project: &Project, derived: &Derived, limit: &AgentLimit) -> String {
    let mut parts: Vec<String> = vec![];
    let levels: Vec<&str> = project
        .levels
        .iter()
        .filter(|l| limit.level_ids.contains(&l.id))
        .map(|l| l.name.trim())
        .filter(|n| !n.is_empty())
        .collect();
    if !levels.is_empty() {
        parts.push(join_and(&levels.iter().map(|s| s.to_string()).collect::<Vec<_>>()));
    }
    if !limit.ids.is_empty() {
        parts.push(guhit_core::scope::describe(project, derived, &limit.ids));
    }
    if !limit.kinds.is_empty() {
        let kinds: Vec<String> = limit.kinds.iter().map(|k| kind_plural(*k).to_string()).collect();
        parts.push(join_and(&kinds));
    }
    if parts.is_empty() {
        "nothing".to_string()
    } else {
        parts.join("; ")
    }
}

fn kind_plural(kind: ElementKind) -> &'static str {
    match kind {
        ElementKind::Wall => "walls",
        ElementKind::Opening => "doors and windows",
        ElementKind::Room => "rooms",
        ElementKind::Column => "columns",
        ElementKind::Stair => "stairs",
        ElementKind::Asset => "objects",
        ElementKind::Annotation => "notes",
        ElementKind::Dimension => "dimensions",
        ElementKind::Camera => "cameras",
        ElementKind::Underlay => "underlays",
        ElementKind::Linework => "linework",
        ElementKind::ReferenceModel => "reference models",
        ElementKind::Pipe => "pipes",
        ElementKind::Slab => "slabs",
        ElementKind::Roof => "roofs",
        ElementKind::Grid => "grid lines",
        ElementKind::ViewMark => "section and elevation marks",
        ElementKind::Lot => "the lot",
        ElementKind::Tour => "tours",
    }
}

fn join_and(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The levels (by id or name) and elements a claim names, checked on the
/// plan. At most `MAX_CLAIM_IDS` elements.
fn claim_targets(project: &Project, levels: &[Id], ids: &[Id]) -> Result<(Vec<Id>, Vec<Id>), IpcError> {
    let mut level_ids = vec![];
    for wanted in levels {
        let wanted = wanted.trim();
        let level = project
            .levels
            .iter()
            .find(|l| l.id == wanted)
            .or_else(|| project.levels.iter().find(|l| l.name.trim().eq_ignore_ascii_case(wanted)))
            .ok_or_else(|| IpcError::new("not_found", format!("There is no level {wanted}. list_levels names them.")))?;
        if !level_ids.contains(&level.id) {
            level_ids.push(level.id.clone());
        }
    }
    let mut element_ids = vec![];
    for id in ids.iter().take(MAX_CLAIM_IDS) {
        let id = id.trim();
        if !project.elements.iter().any(|e| e.id() == id) {
            return Err(IpcError::new("not_found", format!("element not found: {id}")));
        }
        if !element_ids.iter().any(|e| e == id) {
            element_ids.push(id.to_string());
        }
    }
    Ok((level_ids, element_ids))
}

// ================================================================== identity

/// Who an agent is, from its hint.
struct Who {
    /// The connection key and the handle: hashed into the id.
    key: String,
    /// The connection key alone: what pause, stop and limits hang on.
    connection: String,
    /// Client and label, for a connection that takes another's place.
    lineage: String,
    rename_key: String,
    name: String,
    client: String,
    client_version: String,
}

/// Control characters out, spaces trimmed, at most `MAX_HINT_CHARS`.
fn clean_hint(raw: &Option<String>) -> Option<String> {
    raw.as_deref()
        .map(|s| s.chars().filter(|c| !c.is_control()).take(MAX_HINT_CHARS).collect::<String>())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// "Ground Floor", "ground_floor" and "ground-floor" are one handle.
pub(crate) fn handle_of(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// "ground-floor" as people read it: "Ground floor".
fn readable_handle(text: &str) -> String {
    let words: Vec<String> = text
        .split(|c: char| c == '-' || c == '_' || c.is_whitespace())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    let joined = words.join(" ");
    let mut chars = joined.chars();
    let name = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    };
    live::clean_name(&name)
}

/// A client's name as people know it: "claude-code" is "Claude Code".
pub fn readable_client(name: &str) -> String {
    let key = name.trim().to_ascii_lowercase();
    let known = match key.as_str() {
        "" => return String::new(),
        "claude-code" | "claude code" => "Claude Code",
        "claude-ai" | "claude-desktop" | "claude desktop" => "Claude Desktop",
        "codex" | "codex-mcp-client" | "codex-cli" => "Codex",
        "cursor" | "cursor-vscode" => "Cursor",
        "visual studio code" | "vscode" | "visual-studio-code" => "VS Code",
        "github-copilot" | "copilot" | "copilot-cli" => "Copilot",
        "gemini-cli" | "gemini-cli-mcp-client" => "Gemini CLI",
        "windsurf" | "windsurf-client" => "Windsurf",
        "cline" | "cline-mcp-client" => "Cline",
        "goose" => "Goose",
        "zed" => "Zed",
        "lmstudio" | "lm-studio" | "lm studio" => "LM Studio",
        "opencode" => "OpenCode",
        "continue" => "Continue",
        "roo-code" | "roo code" => "Roo Code",
        "amp" => "Amp",
        "guhit-stdio" => "Guhit Studio",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_string();
    }
    let words: Vec<String> = name
        .split(|c: char| c == '-' || c == '_' || c.is_whitespace())
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(first) => first.to_uppercase().chain(c).collect(),
                None => String::new(),
            }
        })
        .collect();
    live::clean_name(&words.join(" "))
}

impl Who {
    fn of(hint: &AgentHint) -> Self {
        let agent = clean_hint(&hint.agent).map(|a| handle_of(&a)).filter(|a| !a.is_empty());
        let label = clean_hint(&hint.label).map(|l| handle_of(&l)).filter(|l| !l.is_empty());
        let instance = clean_hint(&hint.instance);
        let given = clean_hint(&hint.connection);
        let client = clean_hint(&hint.client_name).unwrap_or_default();
        let client_version = clean_hint(&hint.client_version).unwrap_or_default();
        let lineage = format!("{}|{}", client.to_ascii_lowercase(), label.clone().unwrap_or_default());
        let connection = match (&given, &instance, &label) {
            (Some(k), _, _) => format!("k:{k}"),
            (None, Some(i), _) => format!("i:{i}"),
            (None, None, Some(l)) => format!("l:{l}"),
            (None, None, None) if !client.is_empty() => format!("c:{}", client.to_ascii_lowercase()),
            (None, None, None) => "c:unknown".to_string(),
        };
        let key = match &agent {
            Some(a) => format!("{connection}#{a}"),
            None => connection.clone(),
        };
        let (rename_key, name) = match (&agent, &label) {
            (Some(a), _) => (format!("agent:{a}"), readable_handle(a)),
            (None, Some(l)) => (format!("label:{l}"), readable_handle(l)),
            (None, None) if !client.is_empty() => {
                (format!("client:{}", client.to_ascii_lowercase()), readable_client(&client))
            }
            (None, None) => ("client:unknown".to_string(), "Unknown agent".to_string()),
        };
        let name = if name.is_empty() { "Unknown agent".to_string() } else { name };
        Self { key, connection, lineage, rename_key, name, client, client_version }
    }
}

/// "agent-" and 16 hex digits of a hash of the salt and the key: the same
/// agent keeps its id while the app runs.
fn agent_id(salt: &str, key: &str) -> Id {
    let digest = ring::digest::digest(&ring::digest::SHA256, format!("{salt}\n{key}").as_bytes());
    let hex: String = digest.as_ref()[..8].iter().map(|b| format!("{b:02x}")).collect();
    format!("agent-{hex}")
}

/// The status line a tool gives: "add_door" is "Placing doors".
fn tool_status(tool: &str) -> Option<&'static str> {
    Some(match tool {
        "add_wall" | "add_wall_chain" | "set_wall_length" => "Drawing walls",
        "add_rect_room" | "resize_room" => "Laying out rooms",
        "add_door" => "Placing doors",
        "add_window" => "Placing windows",
        "set_opening_size" => "Sizing openings",
        "add_asset" => "Placing objects",
        "add_level" | "delete_level" => "Working on the levels",
        "add_slab" => "Laying slabs",
        "add_stair" => "Placing stairs",
        "batch" => "Building",
        "move_elements" => "Moving elements",
        "delete_elements" => "Removing elements",
        "set_material" => "Setting materials",
        "set_roof" => "Shaping the roof",
        "rename_room" | "set_room_usage" => "Naming rooms",
        "move_pipe_points" | "set_pipe_segment_length" | "set_pipe_slope" | "insert_pipe_point"
        | "remove_pipe_point" => "Routing pipes",
        "undo" => "Undoing a step",
        "redo" => "Redoing a step",
        "get_plan_image" | "render_view" | "capture_view" | "get_view" => "Looking at the plan",
        "describe_elements" | "list_rooms" | "list_elements" | "get_project_summary" | "list_levels" => {
            "Reading the plan"
        }
        "list_review_items" | "set_review_mark" => "Checking the review",
        "send_chat_message" => "Chatting",
        _ => return None,
    })
}

// ============================================================ commit paths

impl AppService {
    /// The commit of an agent's edit under the session lock: the rules, the
    /// automatic version, the commit with the agent as author.
    /// `project_id` is the project the edit was made for: the open one must
    /// still be it.
    fn agent_commit_locked(&self, s: &mut Session, agent: &Id, project_id: &Id, edit: Edit) -> Result<ApplyResult, IpcError> {
        let now = self.agents.now();
        self.agents.check_edit(agent, now)?;
        {
            let doc = s.open_doc()?;
            if doc.project().id != *project_id {
                return Err(project_changed(doc));
            }
            match &edit {
                Edit::Staged(st) => {
                    self.agents.check_reach(agent, st.base(), doc.derived(), st.project(), &st.command())?
                }
                Edit::One(c) => self.agents.check_reach(agent, doc.project(), doc.derived(), doc.project(), c)?,
            }
        }
        self.agents.version_before(s);
        let author = Some(agent.clone());
        let result = match edit {
            Edit::Staged(st) => self.commit_with(s, None, author, Some(project_id), |doc, author, at| {
                doc.apply_staged(*st, Origin::Ai, author, at)
            })?,
            Edit::One(c) => {
                self.commit_with(s, None, author, Some(project_id), |doc, author, at| {
                    doc.apply_at(*c, Origin::Ai, author, at)
                })?
            }
        };
        self.agents.committed(agent, &result, now);
        Ok(result)
    }

    /// An agent's edit on a live session guest: checked here, committed on
    /// the host with the agent as author. A `stale` from the host runs
    /// `build` once more on the copy as it is then.
    async fn agent_edit_on_host<T, E, F>(
        &self,
        call: &AgentCall,
        staging: Staging,
        extra: T,
        (project_id, revision): (Id, u32),
        build: &F,
    ) -> Result<(ApplyResult, T), E>
    where
        F: Fn(&Project, &Derived) -> Result<(Staging, T), E> + Send,
        T: Send,
        E: From<IpcError> + Send,
    {
        let mut attempt = Some((staging, extra, revision));
        let mut retried = false;
        loop {
            let (staging, extra, revision) = match attempt.take() {
                Some(a) => a,
                None => {
                    let s = self.session.lock().await;
                    let doc = s.open_doc()?;
                    if doc.project().id != project_id {
                        return Err(project_changed(doc).into());
                    }
                    let revision = doc.revision();
                    let (st, extra) = build(doc.project(), doc.derived())?;
                    (st, extra, revision)
                }
            };
            self.agents.check_edit(&call.id, self.agents.now())?;
            {
                let s = self.session.lock().await;
                let doc = s.open_doc()?;
                if doc.project().id != project_id {
                    return Err(project_changed(doc).into());
                }
                self.agents.check_reach(&call.id, staging.base(), doc.derived(), staging.project(), &staging.command())?;
            }
            let command = staging.command();
            match live::guest_apply_as(self, command, Origin::Ai, Some(revision), Some(call.id.clone())).await {
                Ok(result) => {
                    self.agents.committed(&call.id, &result, self.agents.now());
                    self.agents.publish();
                    return Ok((result, extra));
                }
                Err(e) if e.code == "stale" && !retried => retried = true,
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// On the host, before a guest's agent's edit: other agents' claims and
    /// the automatic version.
    pub(crate) fn agent_check_remote(&self, s: &Session, author: &str, command: &Command) -> Result<(), IpcError> {
        // Only people set license numbers (D53), a guest's agents included.
        crate::people_only(command)?;
        let doc = s.open_doc()?;
        self.agents.claims_check_remote(author, doc.project(), doc.derived(), doc.project(), command)?;
        self.agents.version_before(s);
        Ok(())
    }
}

// ======================================================================= IPC

/// Commands this module answers. `AppService::handle` routes on this list.
pub const OWNS: &[&str] = &["agents_list", "agent_update", "agent_pause", "agent_stop", "agent_resume"];

/// `agents_list`, `agent_update`, `agent_pause`, `agent_stop`, `agent_resume`.
pub async fn handle(app: &AppService, cmd: &str, args: Value) -> IpcResult {
    match cmd {
        "agents_list" => to_value(&app.agents_list()),
        "agent_update" => {
            let id: String = arg(&args, "id")?;
            let name: Option<String> = arg(&args, "name")?;
            // Left out keeps the limit; null clears it.
            let limit = match args.get("limit") {
                None => None,
                Some(Value::Null) => Some(None),
                Some(v) => Some(Some(
                    serde_json::from_value::<AgentLimit>(v.clone())
                        .map_err(|e| IpcError::new("bad_args", format!("argument `limit`: {e}")))?,
                )),
            };
            if let Some(Some(limit)) = &limit {
                let s = app.session.lock().await;
                if let Some(doc) = s.doc.as_ref() {
                    check_limit_ids(doc.project(), limit)?;
                }
            }
            app.agents.update(&id, name, limit)?;
            app.agents.publish();
            app.agents_list()
                .into_iter()
                .find(|a| a.participant.id == id)
                .map(|a| to_value(&a))
                .unwrap_or_else(|| Err(unknown_agent(&id)))
        }
        "agent_pause" | "agent_stop" | "agent_resume" => {
            let id: Option<String> = arg(&args, "id")?;
            let run = match cmd {
                "agent_pause" => Run::Paused,
                "agent_stop" => Run::Stopped,
                _ => Run::Active,
            };
            app.agents.set_run(id.as_deref(), run)?;
            app.agents.publish();
            to_value(&app.agents_list())
        }
        _ => Err(IpcError::new("unknown_command", format!("unknown command `{cmd}`"))),
    }
}

/// A limit names levels and elements of the open plan.
fn check_limit_ids(project: &Project, limit: &AgentLimit) -> Result<(), IpcError> {
    if let Some(id) = limit.level_ids.iter().find(|id| !project.levels.iter().any(|l| &l.id == *id)) {
        return Err(IpcError::new("bad_args", format!("argument `limit`: there is no level {id}")));
    }
    if let Some(id) = limit.ids.iter().find(|id| !project.elements.iter().any(|e| e.id() == id.as_str())) {
        return Err(IpcError::new("bad_args", format!("argument `limit`: element not found: {id}")));
    }
    Ok(())
}

/// `mcp_status` and `mcp_set_enabled` answers with the agents filled in.
pub(crate) fn with_agents(app: &AppService, cmd: &str, mut out: Value) -> Value {
    if matches!(cmd, "mcp_status" | "mcp_set_enabled") {
        if let Some(map) = out.as_object_mut() {
            map.insert("agents".into(), serde_json::to_value(app.agents_list()).unwrap_or(Value::Array(vec![])));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_comes_from_the_strongest_hint() {
        let hint = |agent: Option<&str>, label: Option<&str>, instance: Option<&str>, client: Option<&str>| AgentHint {
            connection: None,
            agent: agent.map(str::to_string),
            label: label.map(str::to_string),
            instance: instance.map(str::to_string),
            client_name: client.map(str::to_string),
            client_version: Some("1.0".into()),
        };
        let a = Who::of(&hint(Some("Ground Floor"), None, None, Some("claude-code")));
        assert_eq!((a.name.as_str(), a.key.as_str()), ("Ground floor", "c:claude-code#ground-floor"));
        let b = Who::of(&hint(Some("ground_floor"), None, None, Some("claude-code")));
        assert_eq!(a.key, b.key, "one handle however it is written");
        let c = Who::of(&hint(None, Some("upper"), None, Some("cursor")));
        assert_eq!((c.name.as_str(), c.key.as_str(), c.rename_key.as_str()), ("Upper", "l:upper", "label:upper"));
        let d = Who::of(&hint(None, None, Some("p123"), Some("claude-ai")));
        assert_eq!((d.name.as_str(), d.key.as_str()), ("Claude Desktop", "i:p123"));
        let e = Who::of(&hint(None, None, None, None));
        assert_eq!(e.name, "Unknown agent");
        // The server's connection key comes first; handles on it share it.
        let f = Who::of(&AgentHint { connection: Some("s-1".into()), ..hint(Some("kitchen"), None, Some("p9"), Some("codex")) });
        assert_eq!((f.key.as_str(), f.connection.as_str(), f.lineage.as_str()), ("k:s-1#kitchen", "k:s-1", "codex|"));
        assert_eq!(c.lineage, "cursor|upper");
        assert_eq!(readable_client("gemini-cli-mcp-client"), "Gemini CLI");
        assert_eq!(readable_client("my-agent_tool"), "My Agent Tool");
        let id = agent_id("salt", &a.key);
        assert!(id.starts_with("agent-") && id.len() == 22, "{id}");
        assert!(files::check_id(&id, "agent").is_ok());
        assert_ne!(agent_id("other", &a.key), id, "another computer, another id");
    }
}
