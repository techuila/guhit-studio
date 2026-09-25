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
mod pipes;
mod query;
mod rooms;
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
    #[allow(dead_code)]
    origin: Origin,
    /// For an undo entry: the project before the command.
    /// For a redo entry: the project after it.
    project: Project,
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

const MAX_HISTORY: usize = 200;

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
        let mut next = self.project.clone();
        let outcome = exec::execute(&mut next, &command)?;
        let diff = diff_projects(&self.project, &next, &outcome);
        next.updated_at = defaults::now_rfc3339();
        let before = std::mem::replace(&mut self.project, next);
        self.derived = compute_derived(&self.project);
        self.revision += 1;
        self.undo.push(HistoryEntry {
            label: outcome.label,
            origin,
            project: before,
        });
        if self.undo.len() > MAX_HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
        Ok(ApplyResult {
            state: self.state(),
            diff,
        })
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
            },
            diff,
        })
    }

    pub fn undo(&mut self) -> Result<DocState, CoreError> {
        let entry = self.undo.pop().ok_or(CoreError::NothingToUndo)?;
        let current = std::mem::replace(&mut self.project, entry.project);
        self.redo.push(HistoryEntry {
            label: entry.label,
            origin: entry.origin,
            project: current,
        });
        self.derived = compute_derived(&self.project);
        self.revision += 1;
        Ok(self.state())
    }

    pub fn redo(&mut self) -> Result<DocState, CoreError> {
        let entry = self.redo.pop().ok_or(CoreError::NothingToRedo)?;
        let current = std::mem::replace(&mut self.project, entry.project);
        self.undo.push(HistoryEntry {
            label: entry.label,
            origin: entry.origin,
            project: current,
        });
        self.derived = compute_derived(&self.project);
        self.revision += 1;
        Ok(self.state())
    }

    /// Answer a question from model data. The result is JSON meant for the
    /// AI copilot and for display.
    pub fn query(&self, query: &Query) -> Result<serde_json::Value, CoreError> {
        query::run_query(&self.project, query)
    }
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
    diff
}
