//! The swarm draft (DECISIONS D57): the recorded steps, which are the
//! truth, and the view, the model with every step applied in order, which
//! agents read and the window draws.
//!
//! # Pinned ids
//! Each step is recorded with `guhit_core::replay::record`: every id its
//! leaf commands made, by slot, and a hash of everything it named. Replaying
//! it anywhere (`replay::replay`) makes the same ids or refuses `lost_id`;
//! something it names that changed is `changed_on_main`.
//!
//! # Dependencies
//! A part depends on another when one of its steps names (reads) or changes
//! (writes) something the other part made or changed before, or when its
//! plan waits for it (`after`). A step that would make two parts depend on
//! each other is refused `cross_part` when it is written, so every part with
//! the parts it depends on can be accepted alone.
//!
//! # Accepting
//! `plan` works out an accept on copies, holding no lock: the parts in
//! dependency order (ties in the order of their first step), each one undo
//! step, the project after part k being every step of parts 0 to k replayed
//! in recorded order on the model with pinned ids. Accept all therefore ends
//! exactly on the view. A step of the accepted parts that does not apply on
//! the model alone (`AcceptPlan::skipped`) stays in the draft. What is left
//! of the draft is replayed on the new model in recorded order (`rebuild`);
//! a step that no longer applies drops out as a `SwarmConflict`, and so
//! does every later step that needs what it made.
//!
//! Lane L1 owns this file.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use guhit_core::replay::{self, Recording};
use guhit_core::{CoreError, Document, Staging};
use guhit_model::*;

use super::{part_stopped, MAX_DRAFT_ELEMENTS, MAX_FEED, MAX_STEPS_PER_PART};
use crate::AppService;

/// The lead's part key. Its steps are the Setup steps and every call of the
/// lead's connection that names no handle.
pub const LEAD: &str = "lead";

/// One agent tool call that landed in the draft.
#[derive(Debug, Clone)]
pub struct RecordedStep {
    pub seq: u32,
    pub part: String,
    pub agent_id: Id,
    /// The undo label as the engine gives it ("MCP: add walls").
    pub label: String,
    /// The agent's status line when the step landed.
    pub status: Option<String>,
    /// The leaf commands, in order.
    pub commands: Vec<Command>,
    /// Against the draft before the step.
    pub added: Vec<Id>,
    pub modified: Vec<Id>,
    pub removed: Vec<Id>,
    pub at: String,
    /// The ids it made, by slot, and the hash of what it named.
    pub rec: Recording,
    /// Ids it names that the draft had before it.
    pub reads: Vec<Id>,
    /// What it changed or removed that was there before: elements, levels
    /// and door and window types.
    pub writes: Vec<Id>,
}

impl RecordedStep {
    pub fn public(&self) -> SwarmStep {
        SwarmStep {
            seq: self.seq,
            part: self.part.clone(),
            agent_id: self.agent_id.clone(),
            label: self.label.clone(),
            status: self.status.clone(),
            added: self.added.clone(),
            modified: self.modified.clone(),
            removed: self.removed.clone(),
            at: self.at.clone(),
        }
    }

    /// Every id it made.
    pub fn made(&self) -> impl Iterator<Item = &Id> {
        self.rec.made().chain(&self.added)
    }

    /// Everything it named or changed that was there before it.
    pub fn touches(&self) -> impl Iterator<Item = &Id> {
        self.reads.iter().chain(&self.writes)
    }

    fn conflict(&self, e: &CoreError) -> SwarmConflict {
        let (message, element_ids) = match e {
            CoreError::Invalid { message, element_ids, .. } => (message.clone(), element_ids.clone()),
            CoreError::NotFound(id) => (format!("It needs an element the model does not have ({id})."), vec![id.clone()]),
            other => (other.to_string(), vec![]),
        };
        SwarmConflict {
            part: self.part.clone(),
            step: self.seq,
            label: self.label.clone(),
            code: e.code().to_string(),
            message,
            element_ids,
        }
    }
}

/// One part with what the panel does not show: its scope, with level ids,
/// and what it waits for.
#[derive(Debug, Clone)]
pub struct PartRec {
    pub part: SwarmPart,
    pub scope: SwarmScope,
    pub after: Vec<SwarmWait>,
}

/// The one open draft of an `AppService` (`super::Swarms`).
pub struct Draft {
    pub id: Id,
    pub project_id: Id,
    pub source: SwarmSource,
    pub state: SwarmState,
    pub brief: String,
    /// Scope levels hold ids.
    pub plan: SwarmPlan,
    /// The lead first, then the plan's parts.
    pub parts: Vec<PartRec>,
    /// Handle to part key. A handle binds the agent that first calls with it.
    pub handles: Vec<(String, String)>,
    /// Agent id to part key.
    pub bound: BTreeMap<Id, String>,
    /// The model the steps apply to. The model is locked while the draft is
    /// open and moves only with an accept, which moves this with it.
    pub base: Project,
    pub base_revision: u32,
    pub steps: Vec<RecordedStep>,
    /// The base with every step applied, in order. Its history is empty.
    pub view: Arc<Document>,
    pub draft_revision: u32,
    /// The last `seq` given out.
    pub seq: u32,
    /// Newest first.
    pub feed: Vec<SwarmFeedItem>,
    /// Elements the view adds, changes and removes against the base.
    pub counts: (u32, u32, u32),
    /// Money for an in-app swarm, kept by the orchestrator (L2). None for
    /// an MCP swarm.
    pub cost: Option<SwarmCost>,
    /// RFC 3339 UTC until which the AI service asked the workers to wait
    /// (a rate limit), kept by the orchestrator (L2).
    pub ai_wait_until: Option<String>,
    /// Steps that no longer apply, oldest first.
    pub conflicts: Vec<SwarmConflict>,
    /// Set while an accept commits its parts with the draft open (`accepting`).
    pub committing: bool,
    /// The parts an accept is committing right now: none of them may draw
    /// until it is done (`check_step` refuses, workers wait).
    pub holding: BTreeSet<String>,
    /// Tests only: the commit (0 first) at which an accept fails as if the
    /// engine refused it, to show nothing is lost.
    #[cfg(test)]
    pub fail_commit: Option<usize>,
    /// Per part, the steps its agent undid, newest last, for its redo. A new
    /// step of the part clears them.
    pub redo: BTreeMap<String, Vec<RecordedStep>>,
    /// True once the version "Before swarm, 3:42 PM" is saved: before the
    /// first accept only.
    pub versioned: bool,
}

impl Default for Draft {
    /// An empty draft on an empty project; `start_draft` fills it in.
    fn default() -> Self {
        let base = defaults::new_project("");
        Self {
            id: String::new(),
            project_id: String::new(),
            source: SwarmSource::App,
            state: SwarmState::Running,
            brief: String::new(),
            plan: SwarmPlan::default(),
            parts: vec![],
            handles: vec![],
            bound: BTreeMap::new(),
            view: Arc::new(Document::new(base.clone())),
            base,
            base_revision: 0,
            steps: vec![],
            draft_revision: 0,
            seq: 0,
            feed: vec![],
            counts: (0, 0, 0),
            cost: None,
            ai_wait_until: None,
            conflicts: vec![],
            committing: false,
            holding: BTreeSet::new(),
            #[cfg(test)]
            fail_commit: None,
            redo: BTreeMap::new(),
            versioned: false,
        }
    }
}

/// The status line `agent` shows now: its own (held 60 s after it set it),
/// else its tool's. For the steps it records.
pub fn agent_status(app: &AppService, agent: &str) -> Option<String> {
    app.agents_list().into_iter().find(|a| a.participant.id == agent).and_then(|a| a.presence.status)
}

impl Draft {
    /// True while an accept commits parts to the model with the draft still
    /// open: the main lock lets those commits through. The accepter holds
    /// the session lock throughout, so nobody else's write can pass.
    pub fn accepting(&self) -> bool {
        self.committing
    }

    /// The part an agent is bound to.
    pub fn part_of(&self, agent: &str) -> Option<&str> {
        self.bound.get(agent).map(String::as_str)
    }

    pub fn part(&self, key: &str) -> Option<&PartRec> {
        self.parts.iter().find(|p| p.part.key == key)
    }

    pub fn part_mut(&mut self, key: &str) -> Option<&mut PartRec> {
        self.parts.iter_mut().find(|p| p.part.key == key)
    }

    fn name_of(&self, key: &str) -> String {
        self.part(key).map(|p| p.part.name.clone()).unwrap_or_else(|| key.to_string())
    }

    /// The part's status line becomes the agent's, when it shows one. The
    /// step recorded next carries it.
    pub fn note_status(&mut self, part: &str, status: Option<String>) {
        if let (Some(status), Some(rec)) = (status, self.part_mut(part)) {
            rec.part.status = Some(status);
        }
    }

    /// The swarm for the panel and the `swarm` event.
    pub fn status(&self) -> SwarmStatus {
        let parts = self
            .parts
            .iter()
            .map(|p| {
                let mut part = p.part.clone();
                part.waiting_for = p.after.iter().filter(|w| !self.reached(w)).cloned().collect();
                part
            })
            .collect();
        SwarmStatus {
            id: self.id.clone(),
            project_id: self.project_id.clone(),
            source: self.source.clone(),
            state: self.state,
            brief: self.brief.clone(),
            // None while the lead plans (CONTRACT "Swarms").
            plan: (self.state != SwarmState::Planning).then(|| self.plan.clone()),
            parts,
            feed: self.feed.clone(),
            draft_revision: self.draft_revision,
            main_revision: self.base_revision,
            added: self.counts.0,
            modified: self.counts.1,
            removed: self.counts.2,
            conflicts: self.conflicts.clone(),
            cost: self.cost.clone(),
            ai_wait_until: self.ai_wait_until.clone(),
        }
    }

    /// True when a part reached what `w` waits for.
    fn reached(&self, w: &SwarmWait) -> bool {
        self.part(&w.part).is_some_and(|p| {
            p.part.milestones.contains(&w.milestone)
                || (w.milestone == "done" && matches!(p.part.state, SwarmPartState::Done | SwarmPartState::Accepted))
        })
    }

    /// Refuse a step the part may not make: a stopped or accepted part, a
    /// paused one, one outside the part's scope, or one that would make two
    /// parts depend on each other (`cross_part`). `derived` belongs to the
    /// project the staging started from.
    pub fn check_step(&self, part: &str, derived: &Derived, staging: &Staging) -> Result<(), IpcError> {
        let rec = self.part(part).ok_or_else(|| IpcError::new("not_found", format!("The swarm has no part {part}.")))?;
        match rec.part.state {
            SwarmPartState::Stopped | SwarmPartState::Accepted | SwarmPartState::Failed => {
                return Err(part_stopped(&rec.part.name))
            }
            SwarmPartState::Paused => {
                return Err(IpcError::new(
                    "agent_paused",
                    format!("A person paused {} in the swarm. Stop and wait; reading the draft still works.", rec.part.name),
                ))
            }
            _ => {}
        }
        if self.holding.contains(part) {
            return Err(IpcError::new(
                "agent_paused",
                format!("A person is accepting {} into the model right now. Wait a moment, then check the swarm's status.", rec.part.name),
            ));
        }
        if self.state == SwarmState::Paused {
            return Err(IpcError::new(
                "agent_paused",
                "A person paused the swarm. Stop and wait; reading the draft still works.",
            ));
        }
        if rec.part.steps as usize >= MAX_STEPS_PER_PART {
            return Err(IpcError::new(
                "invalid",
                format!("{} has {MAX_STEPS_PER_PART} steps in the draft, the most a part may have. Finish the part.", rec.part.name),
            ));
        }
        if !(rec.scope.levels.is_empty() && rec.scope.kinds.is_empty()) {
            let limit = AgentLimit { level_ids: rec.scope.levels.clone(), ids: vec![], kinds: rec.scope.kinds.clone() };
            guhit_core::scope::check_limit(staging.base(), derived, staging.project(), &limit, &staging.command()).map_err(|e| {
                let mut e = IpcError::from(e);
                e.code = "out_of_scope".into();
                e.message = format!(
                    "{} The swarm's plan gives {} {}. Draw only that; another part draws the rest.",
                    e.message,
                    rec.part.name,
                    describe_scope(staging.base(), &rec.scope)
                );
                e
            })?;
        }
        let (reads, writes) = effects(staging.base(), staging.project(), staging.commands());
        self.check_cycle(part, reads.iter().chain(&writes))
    }

    /// Refuse a step of `part` touching `touched` when it would make a part
    /// it does not depend on yet depend on it, the other way round.
    pub(crate) fn check_cycle<'a>(&self, part: &str, touched: impl Iterator<Item = &'a Id>) -> Result<(), IpcError> {
        let graph = self.depends();
        let by = touched_by(&self.steps);
        let mut new: BTreeMap<&str, Vec<Id>> = BTreeMap::new();
        for id in touched {
            for q in by.get(id).into_iter().flatten() {
                if *q != part && !graph.get(part).is_some_and(|d| d.contains(*q)) {
                    new.entry(q).or_default().push(id.clone());
                }
            }
        }
        for (q, ids) in new {
            if reaches(&graph, q, part) {
                let (me, other) = (self.name_of(part), self.name_of(q));
                return Err(IpcError {
                    code: "cross_part".into(),
                    message: format!(
                        "This step works on what {other} drew or changed, but {other} already builds on {me}, so the two parts would depend on each other and neither could be accepted alone. Leave {other}'s elements to {other}, or tell the lead."
                    ),
                    element_ids: ids,
                });
            }
        }
        Ok(())
    }

    /// Part to the parts it depends on: its plan's `after`, and every part
    /// that made or changed something one of its steps names or changes.
    pub fn depends(&self) -> BTreeMap<String, BTreeSet<String>> {
        depends_of(&self.parts, &self.steps)
    }

    /// `keys` with every part they depend on, directly or not.
    pub fn closure(&self, keys: &[String]) -> BTreeSet<String> {
        walk(&self.depends(), keys)
    }

    /// The parts that depend on `keys`, directly or not, `keys` left out.
    pub fn dependents(&self, keys: &[String]) -> Vec<String> {
        let graph = self.depends();
        let mut reverse: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (p, deps) in &graph {
            for q in deps {
                reverse.entry(q.clone()).or_default().insert(p.clone());
            }
        }
        let found = walk(&reverse, keys);
        self.parts
            .iter()
            .map(|p| p.part.key.clone())
            .filter(|k| found.contains(k) && !keys.contains(k) && self.part(k).is_some_and(|p| p.part.state != SwarmPartState::Accepted))
            .collect()
    }

    /// Put a step in: it becomes the newest step and the view moves on.
    /// `staging` started on the view as it is. The step is run once more to
    /// record its ids (`replay::record`); a step that would make two parts
    /// depend on each other is refused `cross_part`.
    pub fn record(&mut self, part: &str, agent: &Id, staging: Staging, at: String) -> Result<ApplyResult, IpcError> {
        if staging.commands().is_empty() {
            return Err(IpcError::new("invalid", "The call changed nothing: it has no commands."));
        }
        let label = match staging.command() {
            Command::Batch { label, .. } if !label.trim().is_empty() => label.trim().to_string(),
            _ => "Batch".to_string(),
        };
        let commands = staging.commands().to_vec();
        let (staging, rec) = replay::record(staging.base(), &label, &commands).map_err(IpcError::from)?;
        let (reads, writes) = effects(staging.base(), staging.project(), &commands);
        self.check_cycle(part, reads.iter().chain(&writes))?;
        let diff = staging.diff();
        let view = Document::with_revision(staging.project().clone(), self.base_revision + self.draft_revision + 1);
        let counts = counts_against(&self.base, view.project());
        if counts.0 as usize > MAX_DRAFT_ELEMENTS {
            return Err(IpcError::new(
                "invalid",
                format!("The draft would hold more than {MAX_DRAFT_ELEMENTS} new elements, the most a swarm draft may. Ask a person to accept it first."),
            ));
        }
        let pr = self.part_mut(part).ok_or_else(|| IpcError::new("not_found", format!("The swarm has no part {part}.")))?;
        if pr.part.state == SwarmPartState::Waiting {
            pr.part.state = SwarmPartState::Working;
        }
        let status = pr.part.status.clone();
        let name = pr.part.name.clone();
        self.seq += 1;
        self.draft_revision += 1;
        self.redo.remove(part);
        self.steps.push(RecordedStep {
            seq: self.seq,
            part: part.to_string(),
            agent_id: agent.clone(),
            label: label.clone(),
            status,
            commands,
            added: diff.added.clone(),
            modified: diff.modified.clone(),
            removed: diff.removed.clone(),
            at: at.clone(),
            rec,
            reads,
            writes,
        });
        self.recount();
        self.feed.insert(0, SwarmFeedItem { at, part: part.to_string(), text: format!("{name}: {}", diff.summary) });
        self.feed.truncate(MAX_FEED);
        self.counts = counts;
        self.view = Arc::new(view);
        let mut state = self.view.state();
        state.undo_label = Some(label);
        state.undo_by = Some(agent.clone());
        Ok(ApplyResult { state, diff })
    }

    /// Each part's step count, newest `seq` and conflicts, from the steps.
    /// A part without steps keeps its `last_seq` once accepted (what the
    /// person accepted), else it is 0.
    pub fn recount(&mut self) {
        for p in &mut self.parts {
            let key = p.part.key.as_str();
            let mine = self.steps.iter().filter(|s| s.part == key);
            p.part.steps = mine.clone().count() as u32;
            match mine.map(|s| s.seq).max() {
                Some(seq) => p.part.last_seq = seq,
                None if p.part.state == SwarmPartState::Accepted => {}
                None => p.part.last_seq = 0,
            }
            p.part.conflicts = self.conflicts.iter().filter(|c| c.part == key).count() as u32;
        }
    }

    /// Make `rebuilt` the draft: its base, steps and view, one draft
    /// revision on. Conflicts it found are added to the draft's.
    pub fn install(&mut self, base: Project, base_revision: u32, rebuilt: Rebuilt) {
        self.base = base;
        self.base_revision = base_revision;
        self.steps = rebuilt.kept;
        self.add_conflicts(rebuilt.dropped);
        self.draft_revision += 1;
        let view = Document::with_revision(rebuilt.project, self.base_revision + self.draft_revision);
        self.counts = counts_against(&self.base, view.project());
        self.view = Arc::new(view);
        self.recount();
    }

    /// Add conflicts, one per step: a step already listed keeps its entry.
    pub fn add_conflicts(&mut self, conflicts: impl IntoIterator<Item = SwarmConflict>) {
        for c in conflicts {
            if !self.conflicts.iter().any(|x| x.step == c.step) {
                self.conflicts.push(c);
            }
        }
    }

    /// A line for the feed, newest first.
    pub fn say(&mut self, part: &str, text: String) {
        self.feed.insert(0, SwarmFeedItem { at: defaults::now_rfc3339(), part: part.to_string(), text });
        self.feed.truncate(MAX_FEED);
    }

    /// True once nothing is left to accept or draw: no steps, and every plan
    /// part accepted, stopped, failed or done. The lead does not count.
    pub fn finished(&self) -> bool {
        self.steps.is_empty()
            && self.parts.iter().filter(|p| p.part.key != LEAD).all(|p| {
                matches!(
                    p.part.state,
                    SwarmPartState::Accepted | SwarmPartState::Stopped | SwarmPartState::Failed | SwarmPartState::Done
                )
            })
    }

    /// What `plan` needs, copied out of the lock.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            draft_revision: self.draft_revision,
            base_revision: self.base_revision,
            steps: self.steps.clone(),
            names: self.parts.iter().map(|p| (p.part.key.clone(), p.part.name.clone())).collect(),
            depends: self.depends(),
        }
    }

    /// What the window draws: the view, and its diff against `main`.
    pub fn draft_state(&self, main: &Project, since_step: u32) -> SwarmDraftState {
        // Each element under the part whose step touched it last.
        let mut owner: BTreeMap<&Id, &str> = BTreeMap::new();
        for step in &self.steps {
            for id in step.added.iter().chain(&step.modified) {
                owner.insert(id, step.part.as_str());
            }
        }
        let main_els: BTreeMap<&Id, &Element> = main.elements.iter().map(|e| (e.id(), e)).collect();
        let view = self.view.project();
        let view_ids: BTreeSet<&Id> = view.elements.iter().map(|e| e.id()).collect();
        let mut per_part: BTreeMap<&str, Vec<Id>> = BTreeMap::new();
        let mut changed: BTreeSet<&Id> = BTreeSet::new();
        for el in &view.elements {
            if main_els.get(el.id()).is_some_and(|m| *m == el) {
                continue;
            }
            changed.insert(el.id());
            let part = owner.get(el.id()).copied().unwrap_or(LEAD);
            per_part.entry(part).or_default().push(el.id().clone());
        }
        let parts = self
            .parts
            .iter()
            .filter_map(|p| {
                per_part.remove(p.part.key.as_str()).map(|ids| SwarmPartIds { part: p.part.key.clone(), color: p.part.color, ids })
            })
            .collect();
        let removed = main.elements.iter().filter(|e| !view_ids.contains(e.id())).map(|e| e.id().clone()).collect();
        // A draft door in a model wall changes how that wall looks.
        let mut display_modified: Vec<Id> = vec![];
        for el in &view.elements {
            if let Element::Opening(o) = el {
                let host_in_main = main_els.contains_key(&o.wall_id);
                if changed.contains(&o.id) && host_in_main && !changed.contains(&o.wall_id) && !display_modified.contains(&o.wall_id) {
                    display_modified.push(o.wall_id.clone());
                }
            }
        }
        SwarmDraftState {
            swarm_id: self.id.clone(),
            draft_revision: self.draft_revision,
            state: self.view.state(),
            parts,
            removed,
            steps: self.steps.iter().filter(|s| s.seq > since_step).map(RecordedStep::public).collect(),
            display_modified,
        }
    }
}

// ============================================================ dependencies

/// What a step reads (ids of `before` its commands name) and writes
/// (elements, levels and types of `before` that `after` changed or lacks).
pub fn effects(before: &Project, after: &Project, commands: &[Command]) -> (Vec<Id>, Vec<Id>) {
    let reads = replay::named_ids(before, commands);
    let mut writes = vec![];
    let now: BTreeMap<&Id, &Element> = after.elements.iter().map(|e| (e.id(), e)).collect();
    for el in &before.elements {
        if now.get(el.id()).is_none_or(|n| *n != el) {
            writes.push(el.id().clone());
        }
    }
    for l in &before.levels {
        if after.levels.iter().find(|x| x.id == l.id).is_none_or(|x| x != l) {
            writes.push(l.id.clone());
        }
    }
    for t in &before.opening_types {
        if after.opening_types.iter().find(|x| x.id == t.id).is_none_or(|x| x != t) {
            writes.push(t.id.clone());
        }
    }
    (reads, writes)
}

/// Id to the parts whose steps made or changed it.
fn touched_by(steps: &[RecordedStep]) -> BTreeMap<&Id, BTreeSet<&str>> {
    let mut by: BTreeMap<&Id, BTreeSet<&str>> = BTreeMap::new();
    for s in steps {
        for id in s.made().chain(&s.writes) {
            by.entry(id).or_default().insert(s.part.as_str());
        }
    }
    by
}

fn depends_of(parts: &[PartRec], steps: &[RecordedStep]) -> BTreeMap<String, BTreeSet<String>> {
    let mut graph: BTreeMap<String, BTreeSet<String>> =
        parts.iter().map(|p| (p.part.key.clone(), p.after.iter().map(|w| w.part.clone()).collect())).collect();
    let mut by: BTreeMap<&Id, BTreeSet<&str>> = BTreeMap::new();
    for s in steps {
        let deps = graph.entry(s.part.clone()).or_default();
        for id in s.touches() {
            for q in by.get(id).into_iter().flatten() {
                if *q != s.part {
                    deps.insert(q.to_string());
                }
            }
        }
        for id in s.made().chain(&s.writes) {
            by.entry(id).or_default().insert(s.part.as_str());
        }
    }
    graph
}

/// `from` and everything reachable from it.
fn walk(graph: &BTreeMap<String, BTreeSet<String>>, from: &[String]) -> BTreeSet<String> {
    let mut seen: BTreeSet<String> = from.iter().cloned().collect();
    let mut todo: Vec<String> = from.to_vec();
    while let Some(k) = todo.pop() {
        for q in graph.get(&k).into_iter().flatten() {
            if seen.insert(q.clone()) {
                todo.push(q.clone());
            }
        }
    }
    seen
}

fn reaches(graph: &BTreeMap<String, BTreeSet<String>>, from: &str, to: &str) -> bool {
    walk(graph, &[from.to_string()]).contains(to)
}

// ================================================================= rebuild

/// Steps replayed on a model.
pub struct Rebuilt {
    /// The model with every kept step applied.
    pub project: Project,
    /// The steps that applied, their diffs against the new model.
    pub kept: Vec<RecordedStep>,
    /// The steps that no longer apply.
    pub dropped: Vec<SwarmConflict>,
    /// What the dropped steps made: a later step that needs it drops too.
    pub lost: BTreeSet<Id>,
}

/// Replay `steps` in order on `base` with pinned ids. A step that no longer
/// applies drops out as a conflict, and so does every later step that names
/// or changes something it made (`lost_id`).
pub fn rebuild(base: &Project, steps: impl IntoIterator<Item = RecordedStep>) -> Rebuilt {
    Rebuilt { project: base.clone(), kept: vec![], dropped: vec![], lost: BTreeSet::new() }.then(steps)
}

impl Rebuilt {
    /// Replay `steps` after the ones already in, as `rebuild` does.
    pub fn then(mut self, steps: impl IntoIterator<Item = RecordedStep>) -> Rebuilt {
        for mut step in steps {
            let needs: Vec<Id> =
                step.touches().filter(|id| self.lost.contains(*id)).cloned().collect::<BTreeSet<_>>().into_iter().collect();
            let outcome = if needs.is_empty() {
                replay::replay(&self.project, &step.label, &step.commands, &step.rec)
            } else {
                Err(CoreError::invalid_for("lost_id", "It works on what a step that no longer applies made.", needs))
            };
            match outcome {
                Ok(staging) => {
                    let diff = staging.diff();
                    step.added = diff.added;
                    step.modified = diff.modified;
                    step.removed = diff.removed;
                    self.project = staging.project().clone();
                    self.kept.push(step);
                }
                Err(e) => {
                    self.lost.extend(step.made().cloned());
                    self.dropped.push(step.conflict(&e));
                }
            }
        }
        self
    }
}

// ================================================================== accept

/// The draft as an accept needs it, copied out of its lock.
#[derive(Clone)]
pub struct Snapshot {
    pub draft_revision: u32,
    pub base_revision: u32,
    pub steps: Vec<RecordedStep>,
    /// Part key to name.
    pub names: BTreeMap<String, String>,
    pub depends: BTreeMap<String, BTreeSet<String>>,
}

/// One part as one undo step of an accept: the project before it (the
/// model at that point) and after it.
pub struct PartCommit {
    pub part: String,
    pub name: String,
    pub author: Id,
    pub base: Project,
    pub project: Project,
    /// The `seq` of every step in the model once this commit is in.
    pub kept: Vec<u32>,
}

/// An accept worked out on copies.
pub struct AcceptPlan {
    /// The parts accepted, in commit order, every one in the closure.
    pub parts: Vec<String>,
    /// One per part that changes the model, in order, each based on the one
    /// before (the first on the model).
    pub commits: Vec<PartCommit>,
    /// Steps of the accepted parts that do not apply on the model: the ones
    /// replaying every step of the closure leaves out. They stay in the
    /// draft.
    pub skipped: Vec<SwarmConflict>,
    /// The `seq` of every step of the accepted parts.
    pub seqs: Vec<u32>,
    /// The same, per part.
    pub steps_of: BTreeMap<String, Vec<u32>>,
    /// The `seq` of every step in the model after the accept.
    pub kept: Vec<u32>,
    /// What is left of the draft, replayed on the model after the accept:
    /// the other parts' steps and the skipped ones, in recorded order.
    pub rest: Rebuilt,
    /// The model after the accept.
    pub main: Project,
}

/// The parts of `closure` in commit order: each after the parts it depends
/// on, ties in the order of their first step (parts with no step last, in
/// key order).
pub fn commit_order(snap: &Snapshot, closure: &BTreeSet<String>) -> Vec<String> {
    let first = |k: &str| snap.steps.iter().find(|s| s.part == k).map(|s| s.seq).unwrap_or(u32::MAX);
    let mut left: Vec<String> = closure.iter().cloned().collect();
    left.sort_by_key(|k| (first(k), k.clone()));
    let mut order: Vec<String> = vec![];
    while !left.is_empty() {
        let ready = left
            .iter()
            .position(|k| snap.depends.get(k).into_iter().flatten().all(|q| !closure.contains(q) || order.contains(q)))
            // A circle (refused when written, so never): the first one goes.
            .unwrap_or(0);
        order.push(left.remove(ready));
    }
    order
}

/// Work out accepting `closure` (dependency closed) on `main`, holding no
/// lock: commits per part in `commit_order`, the project after part k being
/// the steps of parts 0 to k replayed in recorded order with pinned ids,
/// then the rest of the draft replayed on the result. A step the last
/// replay (every step of the closure) leaves out is `skipped` and goes to
/// the rest, so it stays in the draft.
pub fn plan(snap: &Snapshot, main: &Project, closure: &BTreeSet<String>) -> AcceptPlan {
    let order = commit_order(snap, closure);
    let mut commits = vec![];
    let mut prev = main.clone();
    let mut taken: BTreeSet<&str> = BTreeSet::new();
    let mut last: Option<(Vec<SwarmConflict>, Vec<u32>)> = None;
    for part in &order {
        taken.insert(part.as_str());
        if !snap.steps.iter().any(|s| &s.part == part) {
            continue;
        }
        let rebuilt = rebuild(main, snap.steps.iter().filter(|s| taken.contains(s.part.as_str())).cloned());
        let kept: Vec<u32> = rebuilt.kept.iter().map(|s| s.seq).collect();
        if rebuilt.project != prev {
            let author = snap.steps.iter().rev().find(|s| &s.part == part).map(|s| s.agent_id.clone()).unwrap_or_default();
            let name = snap.names.get(part).cloned().unwrap_or_else(|| part.clone());
            commits.push(PartCommit {
                part: part.clone(),
                name,
                author,
                base: prev.clone(),
                project: rebuilt.project.clone(),
                kept: kept.clone(),
            });
            prev = rebuilt.project;
        }
        last = Some((rebuilt.dropped, kept));
    }
    let (mut skipped, kept) = last.unwrap_or_default();
    skipped.sort_by_key(|c| c.step);
    let out: BTreeSet<u32> = skipped.iter().map(|c| c.step).collect();
    let mine = || snap.steps.iter().filter(|s| closure.contains(&s.part));
    let seqs = mine().map(|s| s.seq).collect();
    let mut steps_of: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for s in mine() {
        steps_of.entry(s.part.clone()).or_default().push(s.seq);
    }
    let rest = rebuild(&prev, snap.steps.iter().filter(|s| !closure.contains(&s.part) || out.contains(&s.seq)).cloned());
    AcceptPlan { parts: order, commits, skipped, seqs, steps_of, kept, rest, main: prev }
}

/// How Accept all commits the draft on `main`, without committing it: every
/// part in commit order, the last one ending on the view. Refused `stale`
/// when a step no longer applies or the result is not the view.
pub fn plan_accept(d: &Draft, main: &Project) -> Result<Vec<PartCommit>, IpcError> {
    let snap = d.snapshot();
    let all: BTreeSet<String> = d.parts.iter().map(|p| p.part.key.clone()).collect();
    let plan = plan(&snap, main, &all);
    let view = d.view.project();
    if let Some(c) = plan.skipped.first() {
        return Err(IpcError::new(
            "stale",
            format!("Step {} ({}) no longer applies to the model: {} Nothing was accepted.", c.step, c.label, c.message),
        ));
    }
    if plan.main.elements != view.elements || plan.main.levels != view.levels {
        return Err(IpcError::new(
            "stale",
            "Replaying the draft on the model did not give the draft you see, so nothing was accepted. Discard the draft, or ask the agents to draw it again.",
        ));
    }
    Ok(plan.commits)
}

// =================================================================== words

/// Elements `view` adds, changes and removes against `base`.
fn counts_against(base: &Project, view: &Project) -> (u32, u32, u32) {
    let before: BTreeMap<&Id, &Element> = base.elements.iter().map(|e| (e.id(), e)).collect();
    let after: BTreeSet<&Id> = view.elements.iter().map(|e| e.id()).collect();
    let (mut added, mut modified) = (0u32, 0u32);
    for el in &view.elements {
        match before.get(el.id()) {
            None => added += 1,
            Some(b) if *b != el => modified += 1,
            _ => {}
        }
    }
    let removed = base.elements.iter().filter(|e| !after.contains(e.id())).count() as u32;
    (added, modified, removed)
}

/// "Ground Floor; walls and columns": a scope in words.
pub fn describe_scope(project: &Project, scope: &SwarmScope) -> String {
    let mut parts: Vec<String> = vec![];
    let levels: Vec<String> = project
        .levels
        .iter()
        .filter(|l| scope.levels.contains(&l.id))
        .map(|l| l.name.trim().to_string())
        .filter(|n| !n.is_empty())
        .collect();
    if !levels.is_empty() {
        parts.push(levels.join(", "));
    }
    if !scope.kinds.is_empty() {
        let kinds: Vec<String> = scope
            .kinds
            .iter()
            .map(|k| serde_json::to_value(k).ok().and_then(|v| v.as_str().map(|s| s.replace('_', " "))).unwrap_or_default())
            .collect();
        parts.push(kinds.join(", "));
    }
    if parts.is_empty() {
        "the whole project".to_string()
    } else {
        parts.join("; ")
    }
}

/// "Shell", "Shell and Rooms", "Shell, Rooms and Roof".
pub fn names_list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}
