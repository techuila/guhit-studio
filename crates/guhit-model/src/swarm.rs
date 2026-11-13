//! Agent swarms (DECISIONS D56, D57). CONTRACT FILE - owned by the orchestrator.
//!
//! One brief, a lead agent splits it into parts, and several agents draw
//! the parts at once into a draft. The draft is a record of each agent step
//! plus a view the engine builds (main with the steps applied). Nothing
//! enters the model until a person accepts all of it or one part, or
//! discards it. While a draft is open the model is locked for everyone
//! else. docs/CONTRACT.md "Swarms" has the rules.
//!
//! Every type takes a `Swarm` prefix: `Draft` and `Estimate` already name
//! AI render types (`api.rs`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::api::{DocState, EditScope};
use crate::model::*;

/// The lead's split of a brief into parts (D56). Built by the in-app lead's
/// `plan_swarm` tool or by MCP `swarm` action `start`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SwarmPlan {
    /// One line: "Bungalow, 12 x 15 m lot".
    pub summary: String,
    /// What the lead adds before any part starts.
    #[serde(default)]
    pub setup: SwarmSetup,
    /// At most 6 parts, unique keys.
    pub parts: Vec<SwarmPartPlan>,
}

/// What the lead adds before any part starts, as its own draft steps. Every
/// part implicitly comes after Setup.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SwarmSetup {
    /// Levels to add, stacked on top of the highest level, in order.
    #[serde(default)]
    pub levels: Vec<SwarmLevel>,
}

/// A level to add. `height_mm` None is the project's default height.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SwarmLevel {
    pub name: String,
    #[serde(default)]
    pub height_mm: Option<f64>,
}

/// One part of the plan: who draws what, where, and after what.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SwarmPartPlan {
    /// Lowercase letters, digits and single hyphens, at most 24 characters
    /// ("shell", "upper-floor"). Never "lead". Part of the part's handle.
    pub key: String,
    /// As people read it: "Shell".
    pub name: String,
    /// One line: "Walls and columns".
    #[serde(default)]
    pub role: String,
    /// What the part's agent is told to do.
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub scope: SwarmScope,
    /// Milestones of other parts this part waits for before it starts.
    #[serde(default)]
    pub after: Vec<SwarmWait>,
    /// Milestones this part reports along the way ("walls"). "done" is
    /// implicit: it is reached when the part finishes.
    #[serde(default)]
    pub milestones: Vec<String>,
}

/// Where a part may draw, enforced by the engine like an agent's limit
/// (D30, D47). Empty lists limit nothing.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SwarmScope {
    /// Level ids. A plan may name levels by name (its own setup levels
    /// included): they are resolved to ids when the swarm starts, and
    /// `SwarmStatus::plan` holds the ids.
    #[serde(default)]
    pub levels: Vec<String>,
    #[serde(default)]
    pub kinds: Vec<ElementKind>,
    /// Always None in v1 (area scopes are cut, D57).
    #[serde(default)]
    pub area: Option<PlanRect>,
}

/// A box on the plan, mm, `min` the south-west corner.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanRect {
    pub min: Point,
    pub max: Point,
}

/// Wait until `part` reaches `milestone` ("done" when it finishes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SwarmWait {
    pub part: String,
    pub milestone: String,
}

/// Who runs the swarm: Guhit's own lead on the user's Claude key, or an MCP
/// client with its sub-agents (D56).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum SwarmSource {
    App,
    /// `client` as people read it: "Claude Code".
    Mcp { client: String },
}

/// The swarm as a whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SwarmState {
    /// The lead is splitting the brief.
    Planning,
    /// Parts are drawing.
    Running,
    /// A person paused every part: nothing runs and nothing is spent.
    Paused,
    /// Every part finished, or an MCP lead marked the draft ready.
    Ready,
    /// The app restarted with the draft open. Unused in v1 (drafts are not
    /// saved across a restart, D57).
    Interrupted,
    /// The swarm could not go on (a plan that failed twice, an error the
    /// message names). The draft so far can still be accepted or discarded.
    Failed,
}

/// One part as the panel shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SwarmPartState {
    /// Waiting for a milestone of another part (`SwarmPart::waiting_for`).
    Waiting,
    Working,
    /// Paused by a person, or the whole swarm is paused.
    Paused,
    /// What it waited for stopped or failed. In v1 a blocked part stops
    /// with a note in the feed.
    Blocked,
    /// Finished; its steps wait in the draft for a person.
    Done,
    /// A person stopped it. Its steps stay in the draft; every further call
    /// of its agent is refused `part_stopped`.
    Stopped,
    Failed,
    /// A person accepted it: its steps are in the model and out of the draft.
    Accepted,
}

/// One part as the panel shows it. The lead is a part too, key "lead",
/// first in `SwarmStatus::parts`: its steps are the Setup steps and any
/// call of the lead's connection without a handle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SwarmPart {
    pub key: String,
    pub name: String,
    pub role: String,
    /// A peer color (0 to `PEER_COLOR_COUNT` - 1), as agents have.
    pub color: u8,
    /// The agent drawing it, once one is bound (its first call with the
    /// part's handle). None until then.
    pub agent_id: Option<Id>,
    pub state: SwarmPartState,
    /// Its status line: what it is doing now.
    pub status: Option<String>,
    /// Steps it has in the draft.
    pub steps: u32,
    /// Milestones it reached, in order ("walls", then "done").
    #[serde(default)]
    pub milestones: Vec<String>,
    /// What it still waits for. Empty once it may start.
    #[serde(default)]
    pub waiting_for: Vec<SwarmWait>,
    /// Its steps that no longer apply to the model.
    #[serde(default)]
    pub conflicts: u32,
    /// The `seq` of its newest step, 0 before its first. Accept pins to it
    /// (`swarm_accept` `reviewed`).
    #[serde(default)]
    pub last_seq: u32,
}

/// One line of the swarm's feed, newest first in `SwarmStatus::feed`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SwarmFeedItem {
    /// RFC 3339 UTC.
    pub at: String,
    /// The part's key.
    pub part: String,
    /// "Shell: added 4 walls".
    pub text: String,
}

/// One agent tool call that landed in the draft, in arrival order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SwarmStep {
    /// 1 for the first step of the swarm, then counting up; never reused.
    pub seq: u32,
    /// The part's key.
    pub part: String,
    pub agent_id: Id,
    /// Its undo label as the engine gives it: "MCP: add walls".
    pub label: String,
    /// The part's status line when the step landed.
    pub status: Option<String>,
    /// What the step added, changed and removed, against the draft before it.
    pub added: Vec<Id>,
    pub modified: Vec<Id>,
    pub removed: Vec<Id>,
    /// RFC 3339 UTC.
    pub at: String,
}

/// A step that no longer applies to the model, with the engine's reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SwarmConflict {
    pub part: String,
    /// The step's `seq`.
    pub step: u32,
    pub label: String,
    /// The engine's code, or one of the swarm's own: `lost_id` (an id the
    /// draft made cannot be made again), `changed_on_main` (an element the
    /// step names changed in the model since), `cross_part` (it would make
    /// two parts depend on each other), `stale` (a part wrote after the
    /// person reviewed it).
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub element_ids: Vec<Id>,
}

/// Money: the estimate, what was spent and the cap, in US dollars; tokens
/// from the API's usage. Only an in-app swarm has one.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SwarmCost {
    /// The model id the workers use.
    pub model: String,
    /// The model as people read it: "Claude Opus 5.5"; the id itself for a
    /// model the price table does not know.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub model_label: Option<String>,
    /// Set when the price table does not know the model (or the model that
    /// answered): the estimate and the spend use the table's highest row,
    /// so the cap still holds. "Billed at the highest rate: price unknown".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub price_note: Option<String>,
    /// The estimate's low and high figures.
    pub low_usd: Option<f64>,
    pub high_usd: Option<f64>,
    pub spent_usd: f64,
    /// Default US$3 (`swarm_cap_usd` in settings.json).
    pub cap_usd: f64,
    #[ts(type = "number")]
    pub input_tokens: u64,
    #[ts(type = "number")]
    pub output_tokens: u64,
    #[ts(type = "number")]
    pub cache_read_tokens: u64,
    #[serde(default)]
    #[ts(type = "number")]
    pub cache_write_tokens: u64,
}

/// The swarm for the panel and the `swarm` app event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SwarmStatus {
    pub id: Id,
    pub project_id: Id,
    pub source: SwarmSource,
    pub state: SwarmState,
    pub brief: String,
    /// None while the lead plans. Scope levels are level ids.
    pub plan: Option<SwarmPlan>,
    /// The lead first, then the plan's parts in order.
    pub parts: Vec<SwarmPart>,
    /// Newest first, at most 14.
    pub feed: Vec<SwarmFeedItem>,
    /// Goes up by one with every change of the draft: a step in, a part
    /// accepted or discarded.
    pub draft_revision: u32,
    /// The model's revision. It holds while the draft is open (the model is
    /// locked), and moves only with an accept.
    #[serde(default)]
    pub main_revision: u32,
    /// Elements the draft adds, changes and removes against the model.
    pub added: u32,
    pub modified: u32,
    pub removed: u32,
    #[serde(default)]
    pub conflicts: Vec<SwarmConflict>,
    pub cost: Option<SwarmCost>,
    /// RFC 3339 UTC until which the AI service asked the workers to wait
    /// (a rate limit). None when nobody waits.
    #[serde(default)]
    pub ai_wait_until: Option<String>,
}

/// What the window draws: main plus the draft, derived by the engine. The
/// diff against the model is worked out in Rust, never in TypeScript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SwarmDraftState {
    pub swarm_id: Id,
    pub draft_revision: u32,
    /// The draft view: the model with every draft step applied, with its
    /// derived data. Its history is empty.
    pub state: DocState,
    /// Per part, the draft elements it added or changed against the model
    /// (each id under the part whose step touched it last).
    pub parts: Vec<SwarmPartIds>,
    /// Model elements the draft removes.
    pub removed: Vec<Id>,
    /// Steps after `since_step`, oldest first.
    pub steps: Vec<SwarmStep>,
    /// Model elements whose look changes with the draft although no step
    /// changed them: the host walls of draft doors and windows. 3D hides
    /// their model meshes while the draft shows.
    #[serde(default)]
    pub display_modified: Vec<Id>,
}

/// `swarm_draft` with `draft_revision` when the window is current: the
/// draft has not moved since that revision, so only the status comes back,
/// not the view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SwarmDraftCurrent {
    pub swarm_id: Id,
    pub draft_revision: u32,
    pub status: SwarmStatus,
}

/// What `swarm_draft` answers with `draft_revision`: the status alone when
/// the window is current (it has `status`), else the whole draft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export)]
pub enum SwarmDraftReply {
    Current(SwarmDraftCurrent),
    Full(SwarmDraftState),
}

/// The draft elements of one part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SwarmPartIds {
    pub part: String,
    pub color: u8,
    pub ids: Vec<Id>,
}

/// What starting an in-app swarm needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SwarmRequest {
    pub brief: String,
    /// The level on screen, which the lead takes as "here".
    #[serde(default)]
    pub active_level_id: Option<Id>,
    /// "Only the selection" (D30): every part is held to it too.
    #[serde(default)]
    pub scope: Option<EditScope>,
    /// Show the plan to the person before any part starts. Ignored in v1
    /// (plan review is cut, D57).
    #[serde(default)]
    pub review_plan: bool,
    /// The spending cap in US dollars. None: the `swarm_cap_usd` setting,
    /// default 3.
    #[serde(default)]
    pub cap_usd: Option<f64>,
}

/// What an accept did, or what a check says it would do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SwarmAcceptResult {
    /// Parts committed (or, for a check, that would be), in order, each as
    /// one undo step. A part that changes nothing in the model is not
    /// listed.
    pub applied: Vec<SwarmApplied>,
    /// Steps left out, with why. An accept with any is refused
    /// `steps_skipped` unless it says `accept_rest`; they stay in the draft.
    pub skipped: Vec<SwarmConflict>,
    /// The model after the accept. None for a check.
    pub state: Option<DocState>,
    /// A check only: the parts that depend on the parts checked, which
    /// discarding them would take too.
    #[serde(default)]
    pub dependents: Vec<String>,
    /// A check only: elements the accept would leave differently from the
    /// draft view, one entry each, code `differs` (an auto room named
    /// "Room 1" in the draft as "Room 3", a wall joined differently).
    #[serde(default)]
    pub differs: Vec<SwarmConflict>,
}

/// One part committed as one undo step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SwarmApplied {
    pub part: String,
    /// The model's revision after this part's step. 0 for a check.
    pub revision: u32,
    /// The step's summary: "Swarm: Shell: added 12 walls".
    pub summary: String,
}
