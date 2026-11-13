//! Guhit Studio model engine.
//!
//! `Document` is the only way to change a project. The UI and the AI copilot
//! both go through `apply`, so every change is validated, undoable and logged.
//!
//! PUBLIC API IS CONTRACT: `guhit-app` depends on the signatures of
//! `Document`, `CoreError`, `compute_derived`, `migrate` and `templates`.
//! Internals are free to change.
//!
//! Modules:
//! - `geom`: segment and polygon math.
//! - `topo`: wall graph, faces, footprints, mitred wall outlines.
//! - `rooms`: seed based room reconciliation (DECISIONS D7).
//! - `dimensions`: dimensions follow the walls they were snapped to.
//! - `exec`: every `Command`.
//! - `validate`: rules and the post-command safety net.
//! - `issues`: design review suggestions and review marks.
//! - `pipes`: pipe and service run fittings, penetrations, take-off and
//!   their review items.
//! - `devices`: the object schedule and the device and aircon review items.
//! - `query`: read-only answers for the AI copilot.
//! - `scope`: the AI edit scope, whether a command stays inside the
//!   selection (DECISIONS D30).
//! - `ids`: deterministic ids, so `preview` equals `apply`.

use std::collections::BTreeMap;

use guhit_model::*;

mod derive;
mod devices;
mod dimensions;
mod error;
mod exec;
mod geom;
mod ids;
mod issues;
mod pipe_edit;
mod pipes;
mod query;
mod rooms;
pub mod scope;
pub mod templates;
mod topo;
mod validate;

pub use derive::compute_derived;
pub use error::CoreError;
pub use issues::{is_review_code, REVIEW_CODES};
pub use pipes::pipe_name;

/// The room each object stands in, by object id: the room of the closed wall
/// face that holds its position. Objects outside every room are left out.
/// For labels such as "Ceiling light in Bedroom".
pub fn asset_rooms(project: &Project) -> BTreeMap<Id, String> {
    let analysis = topo::analyze(project);
    let assigned = rooms::assign_by_seed(project, &analysis);
    let index = devices::RoomIndex::new(project, &analysis, &assigned);
    project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Asset(a) => index
                .at(&a.level_id, a.position)
                .map(|r| (a.id.clone(), r.name.clone())),
            _ => None,
        })
        .collect()
}

/// Bring a project from an older file up to `SCHEMA_VERSION`. Every layer the
/// project lacks is added after the ones it has, in `LayerKey` order, visible
/// and unlocked. Version 2 added the four pipe layers, version 3 the storm,
/// electrical and aircon layers; every new field of version 3 has a serde
/// default. Running it twice changes nothing. `Document::new` and
/// `Document::with_revision` call it, so every project that is opened goes
/// through it.
pub fn migrate(project: &mut Project) {
    for layer in defaults::default_layers() {
        if !project.layers.iter().any(|l| l.key == layer.key) {
            project.layers.push(layer);
        }
    }
    project.schema_version = SCHEMA_VERSION;
}

struct HistoryEntry {
    label: String,
    origin: Origin,
    /// Who made the step: a live session participant id (DECISIONS D29).
    /// None outside a live session.
    author: Option<Id>,
    /// When it was made, RFC 3339, as the caller of `apply_at` gave it.
    /// Empty from `apply` and `apply_as`.
    at: String,
    /// `Diff::summary` of the step.
    summary: String,
    /// What the step added, then what it changed, at most `MAX_STEP_IDS`.
    ids: Vec<Id>,
    /// What it removed, at most `MAX_STEP_IDS`.
    removed: Vec<Id>,
    /// For an undo entry: the project before the command.
    /// For a redo entry: the project after it.
    project: Project,
}

impl HistoryEntry {
    /// The step as `history_list` shows it. Names and colors are the app's
    /// to fill in: the engine only knows participant ids.
    fn step(&self) -> HistoryStep {
        HistoryStep {
            label: self.label.clone(),
            summary: self.summary.clone(),
            origin: self.origin,
            author_id: self.author.clone(),
            author_name: None,
            color: None,
            at: self.at.clone(),
            ids: self.ids.clone(),
            removed: self.removed.clone(),
        }
    }
}

/// An open project with derived data and undo history.
///
/// Undo is snapshot based on purpose (DECISIONS D6): one undo restores the
/// exact pre-command state, with no inverse-command bugs. Projects are small.
pub struct Document {
    project: Project,
    derived: Derived,
    revision: u32,
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

/// Undo steps kept. The oldest goes when a step past this is made.
pub const MAX_HISTORY: usize = 200;
/// Ids a step keeps of what it added or changed, and of what it removed,
/// for the History panel to show (DECISIONS D38). A step that touched more
/// keeps the first ones.
pub const MAX_STEP_IDS: usize = 200;

impl Document {
    pub fn new(project: Project) -> Self {
        Self::with_revision(project, 0)
    }

    /// Same as `new`, but the revision counter starts at `revision`, so a
    /// restored snapshot never repeats a revision number. History is empty.
    /// An older project is migrated first (`migrate`).
    pub fn with_revision(project: Project, revision: u32) -> Self {
        let mut project = project;
        migrate(&mut project);
        let derived = compute_derived(&project);
        Self {
            project,
            derived,
            revision,
            undo: vec![],
            redo: vec![],
        }
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn derived(&self) -> &Derived {
        &self.derived
    }

    pub fn revision(&self) -> u32 {
        self.revision
    }

    pub fn state(&self) -> DocState {
        DocState {
            project: self.project.clone(),
            derived: self.derived.clone(),
            revision: self.revision,
            can_undo: !self.undo.is_empty(),
            can_redo: !self.redo.is_empty(),
            undo_label: self.undo.last().map(|h| h.label.clone()),
            redo_label: self.redo.last().map(|h| h.label.clone()),
            undo_by: self.undo.last().and_then(|h| h.author.clone()),
            redo_by: self.redo.last().and_then(|h| h.author.clone()),
        }
    }

    /// Rename the project. This is not an undo step: it does not touch the
    /// undo and redo stacks and does not bump the revision. Every stored
    /// history snapshot gets the new name too, so an undo after a rename
    /// does not bring the old name back.
    pub fn rename(&mut self, name: &str) -> Result<(), CoreError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(CoreError::invalid("bad_name", "A project needs a name."));
        }
        self.project.name = name.to_string();
        self.project.updated_at = defaults::now_rfc3339();
        for entry in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            entry.project.name = name.to_string();
        }
        Ok(())
    }

    /// Validate and commit a command as one undo step.
    /// On error the document is unchanged.
    pub fn apply(&mut self, command: Command, origin: Origin) -> Result<ApplyResult, CoreError> {
        self.apply_as(command, origin, None)
    }

    /// `apply`, recording who made the step: a live session participant id
    /// (DECISIONS D29). `DocState::undo_by` reports it while the step is on
    /// top of the undo stack, and `redo_by` once it is undone.
    pub fn apply_as(
        &mut self,
        command: Command,
        origin: Origin,
        author: Option<Id>,
    ) -> Result<ApplyResult, CoreError> {
        self.apply_at(command, origin, author, "")
    }

    /// `apply_as`, recording when the step was made as well: `at` is the
    /// caller's clock (RFC 3339), so the engine stays free of it. `history`
    /// reports it with the step (DECISIONS D38).
    pub fn apply_at(
        &mut self,
        command: Command,
        origin: Origin,
        author: Option<Id>,
        at: &str,
    ) -> Result<ApplyResult, CoreError> {
        let mut next = self.project.clone();
        let outcome = exec::execute(&mut next, &command)?;
        let diff = diff_projects(&self.project, &next, &outcome);
        Ok(self.commit_next(next, outcome.label, diff, origin, author, at))
    }

    /// Commit a staged batch as one undo step: the same result as `apply_at`
    /// of `staging.command()`. When the project is still the one staging
    /// started from, the staged result is taken as it is, without running
    /// the batch a second time (running a command is deterministic).
    /// Otherwise, say after a rename, the batch runs on the project as it is
    /// now, like `apply_at`.
    pub fn apply_staged(
        &mut self,
        staging: Staging,
        origin: Origin,
        author: Option<Id>,
        at: &str,
    ) -> Result<ApplyResult, CoreError> {
        if staging.commands.is_empty() || staging.base != self.project {
            return self.apply_at(staging.command(), origin, author, at);
        }
        let diff = staging.diff();
        let label = staging.batch_label();
        Ok(self.commit_next(staging.project, label, diff, origin, author, at))
    }

    /// Make `next` the project, as one undo step.
    fn commit_next(
        &mut self,
        mut next: Project,
        label: String,
        diff: Diff,
        origin: Origin,
        author: Option<Id>,
        at: &str,
    ) -> ApplyResult {
        next.updated_at = defaults::now_rfc3339();
        let before = std::mem::replace(&mut self.project, next);
        self.derived = compute_derived(&self.project);
        self.revision += 1;
        let ids = diff.added.iter().chain(&diff.modified).take(MAX_STEP_IDS).cloned().collect();
        self.undo.push(HistoryEntry {
            label,
            origin,
            author,
            at: at.to_string(),
            summary: diff.summary.clone(),
            ids,
            removed: diff.removed.iter().take(MAX_STEP_IDS).cloned().collect(),
            project: before,
        });
        if self.undo.len() > MAX_HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
        ApplyResult {
            state: self.state(),
            diff,
        }
    }

    /// The undo and redo stacks, newest first: `undo[0]` is what `undo`
    /// takes back, `redo[0]` what `redo` brings back. Authors are
    /// participant ids only; `author_name` and `color` are None.
    pub fn history(&self) -> HistoryList {
        HistoryList {
            undo: self.undo.iter().rev().map(HistoryEntry::step).collect(),
            redo: self.redo.iter().rev().map(HistoryEntry::step).collect(),
        }
    }

    /// Dry run. Returns exactly what `apply` would return for the same
    /// command on the same revision (same elements, ids, derived data and
    /// diff), and commits nothing.
    pub fn preview(&self, command: &Command) -> Result<ApplyResult, CoreError> {
        let mut next = self.project.clone();
        let outcome = exec::execute(&mut next, command)?;
        let diff = diff_projects(&self.project, &next, &outcome);
        let derived = compute_derived(&next);
        Ok(ApplyResult {
            state: DocState {
                project: next,
                derived,
                revision: self.revision + 1,
                can_undo: true,
                can_redo: false,
                undo_label: Some(outcome.label),
                redo_label: None,
                undo_by: None,
                redo_by: None,
            },
            diff,
        })
    }

    pub fn undo(&mut self) -> Result<DocState, CoreError> {
        self.undo_steps(1)
    }

    pub fn redo(&mut self) -> Result<DocState, CoreError> {
        self.redo_steps(1)
    }

    /// Take back the `n` newest steps at once: one revision, the project
    /// exactly as it was before the oldest of them, and the steps on the
    /// redo stack in order, so `redo_steps(n)` brings all of them back. All
    /// or nothing: with fewer than `n` steps, or `n` 0, nothing changes.
    pub fn undo_steps(&mut self, n: usize) -> Result<DocState, CoreError> {
        Self::check_steps(n, self.undo.len(), CoreError::NothingToUndo, "undone")?;
        Self::move_steps(&mut self.project, &mut self.undo, &mut self.redo, n);
        self.derived = compute_derived(&self.project);
        self.revision += 1;
        Ok(self.state())
    }

    /// Bring back the `n` newest undone steps at once, the mirror of
    /// `undo_steps`.
    pub fn redo_steps(&mut self, n: usize) -> Result<DocState, CoreError> {
        Self::check_steps(n, self.redo.len(), CoreError::NothingToRedo, "redone")?;
        Self::move_steps(&mut self.project, &mut self.redo, &mut self.undo, n);
        self.derived = compute_derived(&self.project);
        self.revision += 1;
        Ok(self.state())
    }

    fn check_steps(n: usize, available: usize, none: CoreError, done: &str) -> Result<(), CoreError> {
        if n == 0 {
            return Err(CoreError::invalid("bad_steps", "Undo or redo at least one step."));
        }
        if available == 0 {
            return Err(none);
        }
        if n > available {
            let steps = if available == 1 { "step" } else { "steps" };
            return Err(CoreError::invalid(
                "too_many_steps",
                format!("Only {available} {steps} can be {done}, not {n}."),
            ));
        }
        Ok(())
    }

    /// Move the top `n` entries of `from` onto `to`, one at a time, each
    /// swapping its snapshot with the project, as `n` single steps would.
    /// The caller checked there are `n`.
    fn move_steps(project: &mut Project, from: &mut Vec<HistoryEntry>, to: &mut Vec<HistoryEntry>, n: usize) {
        for _ in 0..n {
            let Some(mut entry) = from.pop() else { return };
            std::mem::swap(project, &mut entry.project);
            to.push(entry);
        }
    }

    /// Answer a question from model data. The result is JSON meant for the
    /// AI copilot and for display.
    pub fn query(&self, query: &Query) -> Result<serde_json::Value, CoreError> {
        query::run_query(&self.project, query)
    }
}

/// A batch built one step at a time on one working copy of the project, for
/// callers that check every step as it is added: MCP `batch` and the AI
/// copilot's turn.
///
/// Each step runs once, on the project as the steps before it left it, so
/// staging N steps costs N steps. Re-previewing the whole staged batch for
/// every new step cost N squared and held the session lock for seconds.
/// Derived data is worked out only when a caller asks for it.
///
/// The result is exactly what `Document::preview` of the whole batch gives
/// on the same project: commands run in the same order on the same state,
/// and new ids are seeded per leaf command from the project right before it,
/// never from the enclosing batch (see `ids`). A refusal reads the same too:
/// "Step 4 of 9: ...", counting every command staged so far.
pub struct Staging {
    label: String,
    base: Project,
    project: Project,
    commands: Vec<Command>,
    /// Walls the staged steps reshaped without naming them, in order.
    also_moved: Vec<Id>,
}

impl Staging {
    /// Start staging on a copy of `project`. `label` is the batch's undo
    /// label, as `Command::Batch` would carry it.
    pub fn new(project: &Project, label: &str) -> Self {
        Self {
            label: label.to_string(),
            base: project.clone(),
            project: project.clone(),
            commands: vec![],
            also_moved: vec![],
        }
    }

    /// Stage `commands` again on `project`, for when the project changed
    /// under a turn that has staged steps already. Fails as `push` would.
    pub fn rebuilt(project: &Project, label: &str, commands: &[Command]) -> Result<Self, CoreError> {
        let mut staging = Self::new(project, label);
        staging.push(commands)?;
        Ok(staging)
    }

    /// The project with every staged step applied.
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// The project staging started from.
    pub fn base(&self) -> &Project {
        &self.base
    }

    /// Walls the staged steps reshaped without naming them, in the order
    /// they first moved (`Diff::also_moved`).
    pub fn also_moved(&self) -> &[Id] {
        &self.also_moved
    }

    /// Every command staged so far, in order.
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    pub fn into_commands(self) -> Vec<Command> {
        self.commands
    }

    /// The staged batch as one command, ready to commit.
    pub fn command(&self) -> Command {
        Command::Batch {
            label: self.label.clone(),
            commands: self.commands.clone(),
        }
    }

    /// Run `commands` after the steps already staged. All or nothing: on
    /// error the staging is exactly as it was. Returns the project as it was
    /// before these commands, for a per-step diff.
    pub fn push(&mut self, commands: &[Command]) -> Result<Project, CoreError> {
        if self.commands.is_empty() && commands.is_empty() {
            return Err(CoreError::invalid("empty_batch", "The batch has no commands."));
        }
        let before = self.project.clone();
        let total = self.commands.len() + commands.len();
        let mut also_moved = vec![];
        for (j, c) in commands.iter().enumerate() {
            let step = self.commands.len() + j + 1;
            // The same wording as the batch loop in `exec`.
            match exec::execute(&mut self.project, c) {
                Ok(done) => also_moved.extend(done.also_moved),
                Err(e) => {
                    self.project = before;
                    return Err(match e {
                        CoreError::Invalid {
                            code,
                            message,
                            element_ids,
                        } => CoreError::Invalid {
                            code,
                            message: format!("Step {step} of {total}: {message}"),
                            element_ids,
                        },
                        other => other,
                    });
                }
            }
        }
        self.commands.extend(commands.iter().cloned());
        for id in also_moved {
            if !self.also_moved.contains(&id) {
                self.also_moved.push(id);
            }
        }
        Ok(before)
    }

    /// What the staged batch adds, changes and removes against the project
    /// it started from: the `diff` of `Document::preview` of the batch.
    pub fn diff(&self) -> Diff {
        diff_projects(
            &self.base,
            &self.project,
            &exec::Outcome {
                label: self.batch_label(),
                summary: None,
                also_moved: self.also_moved.clone(),
            },
        )
    }

    /// The undo label, as the batch loop in `exec` trims it.
    fn batch_label(&self) -> String {
        let label = self.label.trim();
        if label.is_empty() { "Batch" } else { label }.to_string()
    }

    /// Derived data of the staged project. Worked out on every call.
    pub fn derived(&self) -> Derived {
        compute_derived(&self.project)
    }
}

/// "; also moved: Wall 4150 mm, Wall 3000 mm", naming each wall by its new
/// length, at most four of them.
fn also_moved_sentence(ids: &[Id], after: &BTreeMap<&Id, &Element>) -> Option<String> {
    if ids.is_empty() {
        return None;
    }
    let names: Vec<String> = ids
        .iter()
        .filter_map(|id| match after.get(id) {
            Some(Element::Wall(w)) => {
                let (dx, dy) = (w.end.x - w.start.x, w.end.y - w.start.y);
                Some(format!("Wall {:.0} mm", (dx * dx + dy * dy).sqrt()))
            }
            _ => None,
        })
        .collect();
    let shown = names.len().min(4);
    let mut list = names[..shown].join(", ");
    if names.len() > shown {
        list.push_str(&format!(" and {} more", names.len() - shown));
    }
    Some(format!("; also moved: {list}"))
}

fn diff_projects(before: &Project, after: &Project, outcome: &exec::Outcome) -> Diff {
    let mut diff = Diff::default();
    let mut counts: [BTreeMap<&'static str, usize>; 3] =
        [BTreeMap::new(), BTreeMap::new(), BTreeMap::new()];
    let old: BTreeMap<&Id, &Element> = before.elements.iter().map(|e| (e.id(), e)).collect();
    let new: BTreeMap<&Id, &Element> = after.elements.iter().map(|e| (e.id(), e)).collect();
    for el in &after.elements {
        match old.get(el.id()) {
            None => {
                diff.added.push(el.id().clone());
                *counts[0].entry(exec::diff_noun(el)).or_default() += 1;
            }
            Some(b) if *b != el => {
                diff.modified.push(el.id().clone());
                *counts[1].entry(exec::diff_noun(el)).or_default() += 1;
            }
            _ => {}
        }
    }
    for el in &before.elements {
        if !new.contains_key(el.id()) {
            diff.removed.push(el.id().clone());
            *counts[2].entry(exec::diff_noun(el)).or_default() += 1;
        }
    }
    let part = |verb: &str, c: &BTreeMap<&'static str, usize>| -> Option<String> {
        if c.is_empty() {
            return None;
        }
        let items: Vec<String> = c
            .iter()
            .map(|(noun, n)| exec::count_noun(*n, noun))
            .collect();
        Some(format!("{verb} {}", items.join(", ")))
    };
    // Walls reshaped without being named: only those that were there before
    // and did change. Named in the summary so nobody misses one.
    diff.also_moved = outcome
        .also_moved
        .iter()
        .filter(|id| diff.modified.contains(id))
        .cloned()
        .collect();
    let also = also_moved_sentence(&diff.also_moved, &new);
    diff.summary = match &outcome.summary {
        Some(s) => s.clone(),
        None => {
            let parts: Vec<String> = [
                part("added", &counts[0]),
                part("changed", &counts[1]),
                part("removed", &counts[2]),
            ]
            .into_iter()
            .flatten()
            .collect();
            if parts.is_empty() {
                outcome.label.clone()
            } else {
                format!("{}: {}", outcome.label, parts.join("; "))
            }
        }
    };
    if let Some(also) = also {
        diff.summary.push_str(&also);
    }
    diff
}
