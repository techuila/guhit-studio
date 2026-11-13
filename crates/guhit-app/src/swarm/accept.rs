//! Accepting a swarm draft (DECISIONS D57): all of it, or one part with the
//! parts it depends on, each part one undo step; discarding one part with
//! the parts that depend on it; and an agent's undo of its own draft steps.
//!
//! # Locks and the two passes
//! Session, then draft, then the agents registry. An accept works its plan
//! out on copies, holding no lock (`draft::plan`), then takes the session
//! lock and the draft lock and checks nothing it planned on moved: the
//! model's revision, the accepted parts' steps, and the rest of the draft
//! (new steps of other parts may have come in; they are replayed on the
//! result), and that the model is the one it planned on, all of it. If
//! something moved it plans again. It then marks the draft
//! (`Draft::committing`, which lets its commits through the main lock) and
//! holds the accepted parts (`Draft::holding`: they may not draw), commits
//! each part under the session lock it still holds, and installs the rest
//! of the draft on the new model. The draft stays open throughout, so a
//! commit that fails loses nothing: what did not go in is installed again. Discarding a
//! part and an agent's undo take the session lock first too, so they never
//! run while an accept commits.
//!
//! Lane L1 owns this file.

use std::collections::{BTreeMap, BTreeSet};

use guhit_core::replay;
use guhit_model::*;

use super::draft::{self, names_list, Draft, Rebuilt, LEAD};
use super::part_stopped;
use crate::agents::AgentCall;
use crate::{snapshots, AppService};

/// Times an accept plans again when the draft or the model moved under it.
const ATTEMPTS: usize = 4;

fn no_swarm() -> IpcError {
    IpcError::new("not_found", "No swarm draft is open.")
}

fn ended() -> IpcError {
    IpcError::new(
        "part_stopped",
        "The swarm this agent drew for has ended: a person accepted or discarded its draft. Stop working and tell the user.",
    )
}

/// The open draft, if it is `swarm_id` on `project_id`.
fn target<'a>(g: &'a mut Option<Draft>, swarm_id: &str, project_id: &str) -> Result<&'a mut Draft, IpcError> {
    let d = g.as_mut().ok_or_else(no_swarm)?;
    if d.id != swarm_id {
        return Err(IpcError::new("not_found", format!("Swarm {swarm_id} is not the open draft.")));
    }
    if d.project_id != project_id {
        return Err(IpcError::new("project_changed", "The swarm's project is not the open one."));
    }
    Ok(d)
}

/// The parts an accept of `parts` (None: all) takes: each named part with
/// every part it depends on, accepted parts left out.
fn closure_of(d: &Draft, parts: Option<&[String]>) -> Result<BTreeSet<String>, IpcError> {
    let keys: Vec<String> = match parts {
        None => d.parts.iter().map(|p| p.part.key.clone()).collect(),
        Some(keys) => {
            if keys.is_empty() {
                return Err(IpcError::new("bad_args", "Name at least one part, or none for all of them."));
            }
            for k in keys {
                let p = d.part(k).ok_or_else(|| IpcError::new("not_found", format!("The swarm has no part {k}.")))?;
                if p.part.state == SwarmPartState::Accepted {
                    return Err(IpcError::new("invalid", format!("{} is in the model already.", p.part.name)));
                }
            }
            keys.to_vec()
        }
    };
    Ok(d.closure(&keys).into_iter().filter(|k| d.part(k).is_some_and(|p| p.part.state != SwarmPartState::Accepted)).collect())
}

/// Refused `stale` when a part of `closure` drew, undid or lost a step
/// after the `last_seq` the person reviewed (a part left out counts as 0).
fn check_pins(d: &Draft, closure: &BTreeSet<String>, reviewed: Option<&BTreeMap<String, u32>>) -> Result<(), IpcError> {
    let Some(reviewed) = reviewed else { return Ok(()) };
    for p in d.parts.iter().filter(|p| closure.contains(&p.part.key)) {
        if p.part.last_seq != reviewed.get(&p.part.key).copied().unwrap_or(0) {
            return Err(IpcError::new(
                "stale",
                format!("{} drew more after you reviewed the draft. Look at it again, then accept.", p.part.name),
            ));
        }
    }
    Ok(())
}

/// An accept that would leave steps out, refused until the person says
/// "Accept the rest" (`accept_rest`).
fn steps_skipped(skipped: &[SwarmConflict], names: &BTreeMap<String, String>) -> IpcError {
    let first = &skipped[0];
    let who = names.get(&first.part).cloned().unwrap_or_else(|| first.part.clone());
    let n = skipped.len();
    let many = if n == 1 { "1 step".to_string() } else { format!("{n} steps") };
    IpcError {
        code: "steps_skipped".into(),
        message: format!(
            "{many} of this accept would stay out of the model, first {who}'s step {} ({}): {} Accept the rest (accept_rest), or cancel. Steps that stay out stay in the draft.",
            first.step, first.label, first.message
        ),
        element_ids: skipped.iter().flat_map(|c| c.element_ids.iter().cloned()).collect(),
    }
}

impl AppService {
    /// Accept all (`parts` None) or the parts named with every part they
    /// depend on (D57), refusing `steps_skipped` when a step of theirs
    /// would stay out: `swarm_accept_with` with `accept_rest` false.
    pub async fn swarm_accept(
        &self,
        swarm_id: &str,
        parts: Option<&[String]>,
        reviewed: Option<&BTreeMap<String, u32>>,
    ) -> Result<SwarmAcceptResult, IpcError> {
        self.swarm_accept_with(swarm_id, parts, reviewed, false).await
    }

    /// Accept all (`parts` None) or the parts named with every part they
    /// depend on (D57). Each part is one undo step, in dependency order
    /// (ties in the order of their first step), its agent the author,
    /// `Origin::Ai`, label "Swarm: <name>"; its ids are the draft's. A
    /// version "Before swarm, 3:42 PM" is saved before the first accept.
    /// `reviewed` pins it: refused `stale` when one of the parts drew,
    /// undid or lost a step since the person looked.
    ///
    /// Accept all ends the swarm, and is refused `stale` (nothing applied)
    /// when its result would not be the view. Accepting parts leaves the
    /// rest in the draft, replayed on the new model; a step that no longer
    /// applies drops out as a conflict. A step of the accepted parts that
    /// does not apply on the model is refused `steps_skipped` (nothing
    /// applied) unless `accept_rest`; then the rest goes in and those steps
    /// stay in the draft, listed as conflicts. A part is accepted only when
    /// every step of it went in. The swarm ends once nothing is left.
    ///
    /// The draft stays open until every commit is in, its parts held so
    /// none of them draws meanwhile; when a commit fails, what did not go
    /// in is installed again on the model as it is, so nothing is lost.
    pub async fn swarm_accept_with(
        &self,
        swarm_id: &str,
        parts: Option<&[String]>,
        reviewed: Option<&BTreeMap<String, u32>>,
        accept_rest: bool,
    ) -> Result<SwarmAcceptResult, IpcError> {
        let all = parts.is_none();
        for _ in 0..ATTEMPTS {
            // Pass 1: plan on copies, holding no lock.
            let (mut main, main_revision) = {
                let s = self.session.lock().await;
                let doc = s.open_doc()?;
                (doc.project().clone(), doc.revision())
            };
            let (snap, closure, view) = {
                let mut g = self.swarm.lock();
                let d = target(&mut g, swarm_id, &main.id)?;
                let closure = closure_of(d, parts)?;
                check_pins(d, &closure, reviewed)?;
                (d.snapshot(), closure, d.view.clone())
            };
            if snap.base_revision != main_revision {
                return Err(IpcError::new("stale", "The model moved under the draft, so nothing was accepted. Discard the draft."));
            }
            let plan = draft::plan(&snap, &main, &closure);
            if all {
                if let Some(c) = plan.skipped.first() {
                    return Err(IpcError::new(
                        "stale",
                        format!("Step {} ({}) no longer applies to the model: {} Nothing was accepted.", c.step, c.label, c.message),
                    ));
                }
                if plan.main.elements != view.project().elements || plan.main.levels != view.project().levels {
                    return Err(IpcError::new(
                        "stale",
                        "Replaying the draft on the model did not give the draft you see, so nothing was accepted. Discard the draft, or ask the agents to draw it again.",
                    ));
                }
            } else if !plan.skipped.is_empty() && !accept_rest {
                return Err(steps_skipped(&plan.skipped, &snap.names));
            }
            drop(view);

            // Pass 2: under the session lock, then the draft lock. The model
            // must be the one planned on, all of it (`updated_at` aside, as
            // `Document::apply_replayed` checks), not only its revision.
            let mut s = self.session.lock().await;
            let doc = s.open_doc()?;
            main.updated_at = doc.project().updated_at.clone();
            if doc.revision() != main_revision || *doc.project() != main {
                continue;
            }
            #[cfg(test)]
            let fail_commit: Option<usize>;
            {
                let mut g = self.swarm.lock();
                let d = target(&mut g, swarm_id, &main.id)?;
                check_pins(d, &closure, reviewed)?;
                let seqs: Vec<u32> = d.steps.iter().filter(|st| closure.contains(&st.part)).map(|st| st.seq).collect();
                let rest_then: Vec<u32> = snap.steps.iter().filter(|st| !closure.contains(&st.part)).map(|st| st.seq).collect();
                let rest_now: Vec<u32> =
                    d.steps.iter().filter(|st| !closure.contains(&st.part)).map(|st| st.seq).take(rest_then.len()).collect();
                if seqs != plan.seqs || rest_now != rest_then || closure_of(d, parts)? != closure {
                    continue;
                }
                // A version first, once; nothing is accepted when it cannot
                // be saved.
                if !plan.commits.is_empty() && !d.versioned {
                    let label = format!("Before swarm, {}", chrono::Local::now().format("%-I:%M %p"));
                    let dir = s.open_project_dir()?;
                    snapshots::create(&dir, doc.project(), doc.revision(), &label, true)?;
                    d.versioned = true;
                }
                // The parts stop drawing until their commits are in (every
                // part for Accept all), and the main lock lets those commits
                // through. The session lock keeps everyone else out.
                d.holding = if all { d.parts.iter().map(|p| p.part.key.clone()).collect() } else { closure.clone() };
                d.committing = true;
                #[cfg(test)]
                {
                    fail_commit = d.fail_commit.take();
                }
            }
            let mut applied = vec![];
            let mut done = 0;
            let mut failed = None;
            for c in plan.commits.iter() {
                #[cfg(test)]
                if fail_commit == Some(done) {
                    failed = Some(IpcError::new("invalid", "A test refused this commit."));
                    break;
                }
                let replayed = guhit_core::Replayed { base: c.base.clone(), project: c.project.clone(), label: format!("Swarm: {}", c.name) };
                let author = (!c.author.is_empty()).then(|| c.author.clone());
                match self.commit_with(&mut s, None, author, Some(main.id.as_str()), |doc, author, at| {
                    doc.apply_replayed(replayed, Origin::Ai, author, at)
                }) {
                    Ok(result) => {
                        done += 1;
                        applied.push(SwarmApplied { part: c.part.clone(), revision: result.state.revision, summary: result.diff.summary });
                    }
                    // Saving failed: the step is in, the next ones go on.
                    Err(e) if e.code == "io" => {
                        done += 1;
                        failed.get_or_insert(e);
                    }
                    Err(e) => {
                        failed.get_or_insert(e);
                        break;
                    }
                }
            }
            let state = s.state().ok();
            let (main_after, revision_after) = {
                let doc = s.open_doc()?;
                (doc.project().clone(), doc.revision())
            };
            let status = self.install(swarm_id, all, &plan, done, main_after, revision_after);
            drop(s);
            self.emit(AppEvent::Swarm { swarm: status });
            self.agents.publish();
            if let Some(e) = failed {
                return Err(e);
            }
            return Ok(SwarmAcceptResult { applied, skipped: plan.skipped, state, dependents: vec![], differs: vec![] });
        }
        Err(IpcError::new("stale", "The draft kept changing while it was accepted, so nothing was. Try again."))
    }

    /// After the commits of an accept, `done` of them in: the parts that are
    /// in the model are accepted, and the rest of the draft is installed on
    /// the new model. All in: the rest is the plan's, with steps of other
    /// parts that came in meanwhile replayed after it; Accept all ends the
    /// swarm. A commit failed: every step not in the model is replayed on
    /// the model as it is, and those parts draw on. A part is accepted only
    /// when every step of it is in the model; a part with nothing applied
    /// stays as it was. Returns the swarm, None once it ended. The draft may
    /// have been discarded meanwhile; then there is nothing to do.
    fn install(
        &self,
        swarm_id: &str,
        all: bool,
        plan: &draft::AcceptPlan,
        done: usize,
        main: Project,
        revision: u32,
    ) -> Option<SwarmStatus> {
        let mut g = self.swarm.lock();
        let d = g.as_mut().filter(|d| d.id == swarm_id)?;
        d.committing = false;
        let held = std::mem::take(&mut d.holding);
        let whole = done == plan.commits.len();
        if whole && all {
            *g = None;
            return None;
        }
        let went_in: BTreeSet<u32> = if whole {
            plan.kept.iter().copied().collect()
        } else {
            done.checked_sub(1).and_then(|k| plan.commits.get(k)).map(|c| c.kept.iter().copied().collect()).unwrap_or_default()
        };
        // Accepted first, so `recount` keeps the `last_seq` they had.
        let mut names = vec![];
        for key in held.iter().filter(|k| k.as_str() != LEAD) {
            let mine = plan.steps_of.get(key).map(Vec::as_slice).unwrap_or_default();
            if mine.is_empty() || !mine.iter().all(|s| went_in.contains(s)) {
                continue;
            }
            d.redo.remove(key);
            if let Some(p) = d.part_mut(key) {
                p.part.state = SwarmPartState::Accepted;
                names.push(p.part.name.clone());
            }
        }
        let rest = if whole {
            let known: BTreeSet<u32> = plan
                .seqs
                .iter()
                .chain(plan.rest.kept.iter().map(|s| &s.seq))
                .chain(plan.rest.dropped.iter().map(|c| &c.step))
                .copied()
                .collect();
            let newer: Vec<draft::RecordedStep> = d.steps.iter().filter(|s| !known.contains(&s.seq)).cloned().collect();
            let rest = Rebuilt {
                project: plan.rest.project.clone(),
                kept: plan.rest.kept.clone(),
                dropped: plan.rest.dropped.clone(),
                lost: plan.rest.lost.clone(),
            };
            d.add_conflicts(plan.skipped.iter().cloned());
            rest.then(newer)
        } else {
            draft::rebuild(&main, d.steps.iter().filter(|s| !went_in.contains(&s.seq)).cloned())
        };
        // A step that stayed out once and went in since is no conflict.
        d.conflicts.retain(|c| !went_in.contains(&c.step));
        d.install(main, revision, rest);
        if !names.is_empty() {
            d.say(LEAD, format!("A person accepted {}", names_list(&names)));
        }
        if d.finished() {
            *g = None;
            return None;
        }
        Some(d.status())
    }

    /// Discard one part (D57), and with `with_dependents` every part that
    /// depends on it: their steps leave the draft and the rest is replayed.
    /// A part others depend on is refused (`invalid`, naming them) without
    /// `with_dependents`; the window asks first. Discarded parts stop. The
    /// swarm ends (None) once nothing is left to accept or draw.
    pub async fn swarm_part_discard(&self, part: &str, with_dependents: bool) -> Result<Option<SwarmStatus>, IpcError> {
        let _s = self.session.lock().await;
        let status = {
            let mut g = self.swarm.lock();
            let d = g.as_mut().ok_or_else(no_swarm)?;
            let p = d.part(part).ok_or_else(|| IpcError::new("not_found", format!("The swarm has no part {part}.")))?;
            if p.part.state == SwarmPartState::Accepted {
                return Err(IpcError::new("invalid", format!("{} is in the model already: undo it there.", p.part.name)));
            }
            let name = p.part.name.clone();
            let dependents = d.dependents(&[part.to_string()]);
            if !dependents.is_empty() && !with_dependents {
                let names: Vec<String> = dependents.iter().filter_map(|k| d.part(k).map(|p| p.part.name.clone())).collect();
                let verb = if names.len() == 1 { "builds" } else { "build" };
                return Err(IpcError::new(
                    "invalid",
                    format!("{} {verb} on {name}. Discard them too (with_dependents), or keep {name}.", names_list(&names)),
                ));
            }
            let mut targets = vec![part.to_string()];
            targets.extend(dependents);
            let rebuilt = draft::rebuild(&d.base, d.steps.iter().filter(|s| !targets.contains(&s.part)).cloned());
            let base = d.base.clone();
            d.install(base, d.base_revision, rebuilt);
            let mut names = vec![];
            for key in &targets {
                d.redo.remove(key);
                if let Some(p) = d.part_mut(key) {
                    names.push(p.part.name.clone());
                    if p.part.key != LEAD {
                        p.part.state = SwarmPartState::Stopped;
                    }
                }
            }
            d.recount();
            d.say(part, format!("A person discarded {}", names_list(&names)));
            if d.finished() {
                *g = None;
                None
            } else {
                Some(d.status())
            }
        };
        self.emit(AppEvent::Swarm { swarm: status.clone() });
        self.agents.publish();
        Ok(status)
    }
}

/// `agent_undo` for an agent bound to a part (amendments, P0-4): undo takes
/// its part's newest draft step out, redo puts back the one it took out
/// last (until the part draws again). Never the model's history. A step
/// another part built on is refused `other_author`, `force` or not.
pub(crate) async fn draft_undo(app: &AppService, call: &AgentCall, redo: bool, _force: bool) -> Result<DocState, IpcError> {
    app.agents.check_edit(&call.id, app.agents.now())?;
    let _s = app.session.lock().await;
    let (state, status) = {
        let mut g = app.swarm.lock();
        let d = g.as_mut().ok_or_else(ended)?;
        let part = d.part_of(&call.id).map(str::to_string).ok_or_else(ended)?;
        let rec = d.part(&part).ok_or_else(ended)?;
        let name = rec.part.name.clone();
        match rec.part.state {
            SwarmPartState::Stopped | SwarmPartState::Accepted | SwarmPartState::Failed => return Err(part_stopped(&name)),
            SwarmPartState::Paused => {
                return Err(IpcError::new("agent_paused", format!("A person paused {name} in the swarm. Stop and wait.")))
            }
            _ => {}
        }
        if d.state == SwarmState::Paused {
            return Err(IpcError::new("agent_paused", "A person paused the swarm. Stop and wait."));
        }
        if redo {
            redo_step(d, &part)?;
        } else {
            undo_step(d, &part)?;
        }
        let mut state = d.view.state();
        let newest = d.steps.iter().rev().find(|s| s.part == part);
        state.can_undo = newest.is_some();
        state.undo_label = newest.map(|s| s.label.clone());
        state.undo_by = newest.map(|s| s.agent_id.clone());
        let top = d.redo.get(&part).and_then(|r| r.last());
        state.can_redo = top.is_some();
        state.redo_label = top.map(|s| s.label.clone());
        state.redo_by = top.map(|s| s.agent_id.clone());
        (state, d.status())
    };
    app.emit(AppEvent::Swarm { swarm: Some(status) });
    app.agents.publish();
    Ok(state)
}

fn undo_step(d: &mut Draft, part: &str) -> Result<(), IpcError> {
    let Some(at) = d.steps.iter().rposition(|s| s.part == part) else {
        return Err(IpcError::new("nothing_to_undo", "There is nothing to undo: this part has no step in the draft."));
    };
    let step = &d.steps[at];
    let made: BTreeSet<&Id> = step.made().chain(&step.writes).collect();
    if let Some(later) = d.steps[at + 1..].iter().find(|t| t.part != part && t.touches().any(|id| made.contains(id))) {
        let other = d.part(&later.part).map(|p| p.part.name.clone()).unwrap_or_else(|| later.part.clone());
        return Err(IpcError::new(
            "other_author",
            format!("{other} built on this step ({}), so undoing it would take {other}'s work away too. Leave it, or tell the lead.", step.label),
        ));
    }
    let mut steps = d.steps.clone();
    let removed = steps.remove(at);
    let rebuilt = draft::rebuild(&d.base, steps);
    if let Some(c) = rebuilt.dropped.first() {
        let other = d.part(&c.part).map(|p| p.part.name.clone()).unwrap_or_else(|| c.part.clone());
        return Err(IpcError::new(
            "other_author",
            format!("{other}'s step {} needs this step ({}), so it stays. Leave it, or tell the lead.", c.label, removed.label),
        ));
    }
    let base = d.base.clone();
    d.install(base, d.base_revision, rebuilt);
    let text = format!("Took back {}", removed.label);
    d.redo.entry(part.to_string()).or_default().push(removed);
    d.say(part, text);
    Ok(())
}

fn redo_step(d: &mut Draft, part: &str) -> Result<(), IpcError> {
    let Some(mut step) = d.redo.get_mut(part).and_then(Vec::pop) else {
        return Err(IpcError::new("nothing_to_undo", "There is nothing to redo: this part took no step back since it last drew."));
    };
    d.check_cycle(part, step.touches())?;
    let staging = replay::replay(d.view.project(), &step.label, &step.commands, &step.rec).map_err(|e| {
        let mut e = IpcError::from(e);
        e.message = format!("{} The step cannot come back.", e.message);
        e
    })?;
    let diff = staging.diff();
    d.seq += 1;
    step.seq = d.seq;
    step.added = diff.added;
    step.modified = diff.modified;
    step.removed = diff.removed;
    let text = format!("Brought back {}", step.label);
    let mut steps = d.steps.clone();
    steps.push(step);
    let rebuilt = Rebuilt { project: staging.project().clone(), kept: steps, dropped: vec![], lost: BTreeSet::new() };
    let base = d.base.clone();
    d.install(base, d.base_revision, rebuilt);
    d.say(part, text);
    Ok(())
}
