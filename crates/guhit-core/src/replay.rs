//! Replaying recorded commands onto the model (DECISIONS D57).
//!
//! A swarm draft records each agent step's commands. Accepting it replays
//! them on the model and commits the outcome, one undo step per part, with
//! `Document::apply_replayed`.
//!
//! # Pinned ids
//! [`record`] runs a step and notes every id its leaf commands' generators
//! handed out, each with its slot ([`Slot`]): what a command makes from its
//! own arguments by position, a room `reconcile` makes for a new face by
//! its level and bounding walls, a type `opening_types::follow` makes by the
//! opening that moved. [`replay`] runs the step again, anywhere, and every
//! recorded slot gets its recorded id back. When the engine asks for a slot
//! that was not recorded, never reaches one that was, or the id ends up as
//! something else, the replay is refused `lost_id`: an id is never given to
//! another element. Before the step runs, every element (and level, type,
//! sheet) it names must be as it was when it was recorded, else
//! `changed_on_main`. So replaying a part with the parts it depends on, on
//! a model without the other parts, gives the elements the draft showed.
//!
//! Lane L1 owns this module.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

use guhit_model::*;

use crate::{diff_projects, exec, CoreError, Document, Staging};

// ================================================================ the tape

/// Where an id came from inside one leaf command.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    /// The n-th id the command made from its own arguments, from 0.
    Own(u32),
    /// A room `reconcile` made for a new face: its level and the sorted ids
    /// of the walls around it.
    Room { level_id: Id, walls: Vec<Id> },
    /// A door or window type `opening_types::follow` made for an opening
    /// that no longer matched its type.
    Type { opening_id: Id },
}

impl Slot {
    /// The slot of a room for the face bounded by `walls` on `level_id`.
    pub fn room(level_id: &str, walls: &[Id]) -> Self {
        let mut walls = walls.to_vec();
        walls.sort();
        walls.dedup();
        Slot::Room { level_id: level_id.to_string(), walls }
    }
}

/// One id a leaf command's generator handed out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub slot: Slot,
    /// 0, or how many ids of the same slot came before it in the leaf (two
    /// faces with the same walls).
    pub nth: u32,
    pub id: Id,
    /// What the id named once the step was done (`known_ids`), or "" when
    /// nothing kept it.
    pub kind: String,
}

/// The ids one leaf command handed out, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LeafPins {
    pub pins: Vec<Pin>,
}

/// What `replay` needs to make a step again with the same ids.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recording {
    /// Per leaf command, in the order they ran (batches flattened).
    pub leaves: Vec<LeafPins>,
    /// Every id the step's commands name that the project had before it,
    /// with a hash of what it named then.
    pub named: Vec<(Id, u64)>,
}

impl Recording {
    /// Every id the step's generators handed out.
    pub fn made(&self) -> impl Iterator<Item = &Id> {
        self.leaves.iter().flat_map(|l| l.pins.iter().map(|p| &p.id))
    }
}

struct Tape {
    /// The recording to force; None while recording.
    replay: Option<Vec<LeafPins>>,
    /// What each leaf asked for and got, in leaf order.
    got: Vec<LeafPins>,
}

thread_local! {
    static TAPE: RefCell<Option<Tape>> = const { RefCell::new(None) };
}

/// Called by `IdGen::new`: the new leaf's place on the tape, while one runs.
pub(crate) fn begin_leaf() -> Option<usize> {
    TAPE.with(|t| {
        t.borrow_mut().as_mut().map(|tape| {
            tape.got.push(LeafPins::default());
            tape.got.len() - 1
        })
    })
}

/// The occurrence of `slot` in the leaf so far, and the id recorded for it
/// when replaying.
pub(crate) fn pinned(leaf: usize, slot: &Slot) -> (u32, Option<Id>) {
    TAPE.with(|t| {
        let t = t.borrow();
        let Some(tape) = t.as_ref() else { return (0, None) };
        let nth = tape.got.get(leaf).map(|l| l.pins.iter().filter(|p| &p.slot == slot).count() as u32).unwrap_or(0);
        let id = tape
            .replay
            .as_ref()
            .and_then(|r| r.get(leaf))
            .and_then(|l| l.pins.iter().find(|p| &p.slot == slot && p.nth == nth))
            .map(|p| p.id.clone());
        (nth, id)
    })
}

/// Note an id the leaf's generator handed out.
pub(crate) fn handed(leaf: usize, slot: Slot, nth: u32, id: &Id) {
    TAPE.with(|t| {
        if let Some(l) = t.borrow_mut().as_mut().and_then(|tape| tape.got.get_mut(leaf)) {
            l.pins.push(Pin { slot, nth, id: id.clone(), kind: String::new() });
        }
    });
}

/// Run `f` with a tape: recording, or forcing `replay`. Returns what every
/// leaf that ran got. The tape is gone afterwards, even on a panic.
fn with_tape<R>(replay: Option<Vec<LeafPins>>, f: impl FnOnce() -> R) -> (R, Vec<LeafPins>) {
    struct Restore(Option<Tape>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let prev = self.0.take();
            TAPE.with(|t| *t.borrow_mut() = prev);
        }
    }
    let prev = TAPE.with(|t| t.borrow_mut().replace(Tape { replay, got: vec![] }));
    let restore = Restore(prev);
    let out = f();
    let got = TAPE.with(|t| t.borrow_mut().as_mut().map(|tape| std::mem::take(&mut tape.got))).unwrap_or_default();
    drop(restore);
    (out, got)
}

// =========================================================== record, replay

/// Every id a project knows, with what it names: an element's kind by its
/// serde name ("wall"), then "level", "opening_type", "sheet", "viewport",
/// "revision", "shot" and "material".
pub fn known_ids(project: &Project) -> BTreeMap<&Id, String> {
    let mut out: BTreeMap<&Id, String> = BTreeMap::new();
    for el in &project.elements {
        out.insert(el.id(), kind_name(el.kind()));
        if let Element::Tour(t) = el {
            for s in &t.shots {
                out.insert(&s.id, "shot".into());
            }
        }
    }
    for l in &project.levels {
        out.insert(&l.id, "level".into());
    }
    for t in &project.opening_types {
        out.insert(&t.id, "opening_type".into());
    }
    for s in &project.sheet_set.sheets {
        out.insert(&s.id, "sheet".into());
        for v in &s.viewports {
            out.insert(&v.id, "viewport".into());
        }
    }
    for r in &project.sheet_set.revisions {
        out.insert(&r.id, "revision".into());
    }
    for m in &project.materials {
        out.insert(&m.id, "material".into());
    }
    out
}

fn kind_name(kind: ElementKind) -> String {
    serde_json::to_value(kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

fn json<T: serde::Serialize>(x: &T) -> serde_json::Value {
    serde_json::to_value(x).unwrap_or_default()
}

/// What `id` names in `project`, as JSON, for the hash check.
fn named_value(project: &Project, id: &str) -> Option<serde_json::Value> {
    if let Some(el) = project.elements.iter().find(|e| e.id() == id) {
        return Some(json(el));
    }
    if let Some(l) = project.levels.iter().find(|l| l.id == id) {
        return Some(json(l));
    }
    if let Some(t) = project.opening_types.iter().find(|t| t.id == id) {
        return Some(json(t));
    }
    for s in &project.sheet_set.sheets {
        if s.id == id {
            return Some(json(s));
        }
        if let Some(vp) = s.viewports.iter().find(|vp| vp.id == id) {
            return Some(json(vp));
        }
    }
    if let Some(r) = project.sheet_set.revisions.iter().find(|r| r.id == id) {
        return Some(json(r));
    }
    for el in &project.elements {
        if let Element::Tour(t) = el {
            if let Some(s) = t.shots.iter().find(|s| s.id == id) {
                return Some(json(s));
            }
        }
    }
    project.materials.iter().find(|m| m.id == id).map(json)
}

fn hash_of(value: &serde_json::Value) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    value.to_string().hash(&mut h);
    h.finish()
}

/// Every string in the commands' arguments.
fn strings_in(commands: &[Command]) -> BTreeSet<String> {
    fn walk(v: &serde_json::Value, out: &mut BTreeSet<String>) {
        match v {
            serde_json::Value::String(s) => {
                out.insert(s.clone());
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            serde_json::Value::Object(o) => o.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut out = BTreeSet::new();
    for c in commands {
        walk(&json(c), &mut out);
    }
    out
}

/// The ids of `project` (everything `known_ids` lists) that `commands` name
/// anywhere in their arguments.
pub fn named_ids(project: &Project, commands: &[Command]) -> Vec<Id> {
    let strings = strings_in(commands);
    known_ids(project).into_keys().filter(|id| strings.contains(id.as_str())).cloned().collect()
}

/// Run `commands` on `project` as `Staging::rebuilt` does, and record the
/// ids every leaf handed out and a hash of everything they name.
pub fn record(project: &Project, label: &str, commands: &[Command]) -> Result<(Staging, Recording), CoreError> {
    let named = named_ids(project, commands)
        .into_iter()
        .filter_map(|id| named_value(project, &id).map(|v| (id, hash_of(&v))))
        .collect();
    let (staging, mut leaves) = with_tape(None, || Staging::rebuilt(project, label, commands));
    let staging = staging?;
    tag_kinds(&mut leaves, staging.project());
    Ok((staging, Recording { leaves, named }))
}

fn tag_kinds(leaves: &mut [LeafPins], project: &Project) {
    let known = known_ids(project);
    for pin in leaves.iter_mut().flat_map(|l| l.pins.iter_mut()) {
        pin.kind = known.get(&pin.id).cloned().unwrap_or_default();
    }
}

/// Run a recorded step again on `project` with its ids pinned. Refused,
/// with nothing to show for it: `lost_id` when something the step names is
/// missing, or the engine makes other ids than the recording (one more,
/// one fewer, or one that ends up as something else); `changed_on_main`
/// when something it names is not as it was when it was recorded; and the
/// engine's own refusals.
pub fn replay(project: &Project, label: &str, commands: &[Command], rec: &Recording) -> Result<Staging, CoreError> {
    let mut missing = vec![];
    let mut changed = vec![];
    for (id, hash) in &rec.named {
        match named_value(project, id) {
            None => missing.push(id.clone()),
            Some(v) if hash_of(&v) != *hash => changed.push(id.clone()),
            Some(_) => {}
        }
    }
    if !missing.is_empty() {
        let message = format!(
            "It works on {} the model does not have, made by a step that is not coming with it.",
            count(missing.len(), "element")
        );
        return Err(CoreError::invalid_for("lost_id", message, missing));
    }
    if !changed.is_empty() {
        let message = format!("{} it works on changed since it was drawn.", capital(&count(changed.len(), "element")));
        return Err(CoreError::invalid_for("changed_on_main", message, changed));
    }
    let (staging, mut got) = with_tape(Some(rec.leaves.clone()), || Staging::rebuilt(project, label, commands));
    let staging = staging?;
    tag_kinds(&mut got, staging.project());
    let mut lost: Vec<Id> = vec![];
    let mut extra = 0usize;
    if got.len() != rec.leaves.len() {
        lost.extend(rec.made().cloned());
    } else {
        for (want, have) in rec.leaves.iter().zip(&got) {
            for p in &want.pins {
                if !have.pins.iter().any(|q| q.slot == p.slot && q.nth == p.nth && q.id == p.id && q.kind == p.kind) {
                    lost.push(p.id.clone());
                }
            }
            extra += have.pins.iter().filter(|q| !want.pins.iter().any(|p| p.slot == q.slot && p.nth == q.nth)).count();
        }
    }
    if lost.is_empty() && extra == 0 {
        return Ok(staging);
    }
    let message = if lost.is_empty() {
        format!("On the model it would make {} the draft did not.", count(extra, "more element"))
    } else {
        format!("On the model it cannot make {} it made in the draft.", count(lost.len(), "element"))
    };
    Err(CoreError::invalid_for("lost_id", message, lost))
}

fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

// =================================================================== commit

/// A project worked out on `base` outside the document, from recorded
/// commands, to commit as one undo step.
#[derive(Debug, Clone)]
pub struct Replayed {
    /// The project it was worked out on. The document must still be it
    /// (`updated_at` aside) for the commit to go in.
    pub base: Project,
    /// The project after it.
    pub project: Project,
    /// The undo label: "Swarm: Shell".
    pub label: String,
}

impl Document {
    /// Commit a replayed project as one undo step, with its author and time
    /// as `apply_at` records them. Refused (`stale`, nothing changed) when
    /// the document is no longer the project the replay started on.
    pub fn apply_replayed(
        &mut self,
        replayed: Replayed,
        origin: Origin,
        author: Option<Id>,
        at: &str,
    ) -> Result<ApplyResult, CoreError> {
        let mut base = replayed.base;
        base.updated_at = self.project.updated_at.clone();
        if base != self.project {
            return Err(CoreError::invalid(
                "stale",
                "The model changed after the replay was worked out, so nothing was applied.",
            ));
        }
        let label = match replayed.label.trim() {
            "" => "Replay".to_string(),
            l => l.to_string(),
        };
        let outcome = exec::Outcome { label: label.clone(), summary: None, also_moved: vec![], notes: vec![] };
        let diff = diff_projects(&self.project, &replayed.project, &outcome);
        Ok(self.commit_next(replayed.project, label, diff, origin, author, at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(x: f64) -> Command {
        Command::AddWall {
            start: Point { x, y: 0.0 },
            end: Point { x, y: 3000.0 },
            thickness_mm: None,
            height_mm: None,
            material_id: None,
            level_id: None,
            role: None,
        }
    }

    #[test]
    fn a_replayed_project_commits_as_one_step_and_a_moved_document_refuses_it() {
        let mut doc = Document::new(crate::templates::sample_bungalow());
        let base = doc.project().clone();
        let staged = crate::Staging::rebuilt(&base, "Two walls", &[wall(20_000.0), wall(23_000.0)]).unwrap();
        let replayed = Replayed { base: base.clone(), project: staged.project().clone(), label: "Swarm: Shell".into() };
        let before = doc.revision();
        let result = doc.apply_replayed(replayed.clone(), Origin::Ai, Some("agent-1".into()), "2026-11-10T00:00:00Z").unwrap();
        assert_eq!(result.state.revision, before + 1);
        assert_eq!(result.diff.added.len(), 2);
        assert_eq!(result.state.undo_label.as_deref(), Some("Swarm: Shell"));
        assert_eq!(result.state.undo_by.as_deref(), Some("agent-1"));
        assert_eq!(doc.project().elements, staged.project().elements, "the ids are the replay's");
        // The document moved on: the same replay is refused, nothing changes.
        let e = doc.apply_replayed(replayed, Origin::Ai, None, "").unwrap_err();
        assert!(matches!(&e, CoreError::Invalid { code, .. } if *code == "stale"), "{e:?}");
        assert_eq!(doc.revision(), before + 1);
        doc.undo().unwrap();
        assert_eq!(doc.project().elements, base.elements, "one undo takes it all back");
    }

    // ------------------------------------------------------------ pinning

    fn pt(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    fn seg(a: (f64, f64), b: (f64, f64), level_id: Option<Id>) -> Command {
        Command::AddWall {
            start: pt(a.0, a.1),
            end: pt(b.0, b.1),
            thickness_mm: None,
            height_mm: None,
            material_id: None,
            level_id,
            role: None,
        }
    }

    /// A closed rectangle of walls: four walls and, by `reconcile`, a room.
    fn rect(x: f64, y: f64, w: f64, d: f64, level_id: Option<Id>) -> Command {
        Command::AddWallChain {
            points: vec![pt(x, y), pt(x + w, y), pt(x + w, y + d), pt(x, y + d)],
            closed: true,
            thickness_mm: None,
            level_id,
            role: None,
        }
    }

    fn door(wall_id: &Id, offset_mm: f64) -> Command {
        Command::AddOpening {
            wall_id: wall_id.clone(),
            opening_type: OpeningType::Door,
            offset_mm,
            width_mm: None,
            height_mm: None,
            sill_mm: None,
            style: None,
            flip_side: None,
            flip_hinge: None,
            type_id: None,
        }
    }

    fn code(e: &CoreError) -> &str {
        e.code()
    }

    fn refused<T>(r: Result<T, CoreError>) -> CoreError {
        match r {
            Ok(_) => panic!("expected a refusal"),
            Err(e) => e,
        }
    }

    fn ids_of(p: &Project) -> BTreeSet<Id> {
        p.elements.iter().map(|e| e.id().clone()).collect()
    }

    /// A draft of steps, each recorded on the project the ones before left.
    struct Draft {
        base: Project,
        view: Project,
        steps: Vec<(usize, Vec<Command>, Recording)>,
    }

    impl Draft {
        fn new(base: Project) -> Self {
            Draft { view: base.clone(), base, steps: vec![] }
        }

        fn step(&mut self, part: usize, commands: Vec<Command>) -> Staging {
            let (staging, rec) = record(&self.view, "step", &commands).unwrap();
            self.view = staging.project().clone();
            self.steps.push((part, commands, rec));
            staging
        }

        /// The steps of `parts`, in recorded order, replayed on the base.
        fn replay(&self, parts: &[usize]) -> Result<Project, CoreError> {
            let mut p = self.base.clone();
            for (part, commands, rec) in &self.steps {
                if parts.contains(part) {
                    p = replay(&p, "step", commands, rec)?.project().clone();
                }
            }
            Ok(p)
        }
    }

    #[test]
    fn recording_changes_nothing_and_replaying_on_the_same_model_gives_the_same_project() {
        let base = defaults::new_project("t");
        let plain = Staging::rebuilt(&base, "step", &[rect(0.0, 0.0, 6000.0, 4000.0, None)]).unwrap();
        let (recorded, rec) = record(&base, "step", &[rect(0.0, 0.0, 6000.0, 4000.0, None)]).unwrap();
        assert_eq!(plain.project(), recorded.project(), "the tape changes no id");
        assert_eq!(rec.leaves.len(), 1);
        let kinds: Vec<&str> = rec.leaves[0].pins.iter().map(|p| p.kind.as_str()).collect();
        assert_eq!(kinds, ["wall", "wall", "wall", "wall", "room"]);
        assert!(matches!(rec.leaves[0].pins[4].slot, Slot::Room { ref walls, .. } if walls.len() == 4));
        let again = replay(&base, "step", &[rect(0.0, 0.0, 6000.0, 4000.0, None)], &rec).unwrap();
        assert_eq!(again.project(), recorded.project());
        // No tape is left behind: ids outside a replay are as always.
        assert_eq!(Staging::rebuilt(&base, "step", &[rect(0.0, 0.0, 6000.0, 4000.0, None)]).unwrap().project(), plain.project());
    }

    #[test]
    fn a_part_replayed_without_the_others_keeps_the_ids_the_draft_gave_it() {
        let mut d = Draft::new(defaults::new_project("t"));
        d.step(0, vec![rect(0.0, 0.0, 6000.0, 4000.0, None)]);
        let b = d.step(1, vec![rect(20_000.0, 0.0, 5000.0, 5000.0, None)]);
        let b_ids = ids_of(b.project()).difference(&ids_of(b.base())).cloned().collect::<BTreeSet<_>>();
        assert_eq!(b_ids.len(), 5, "4 walls and a room");
        // Plain re-running gives other ids: new ids hang on every id in the model.
        let plain = Staging::rebuilt(&d.base, "step", &d.steps[1].1).unwrap();
        assert!(ids_of(plain.project()).is_disjoint(&b_ids), "without pinning the ids move");
        // Pinned, part 1 alone gets the draft's ids and elements.
        let alone = d.replay(&[1]).unwrap();
        assert_eq!(ids_of(&alone), b_ids);
        for el in &alone.elements {
            let in_view = d.view.elements.iter().find(|v| v.id() == el.id()).unwrap();
            match (el, in_view) {
                // Rooms are named by count: "Room 1" alone, "Room 2" in the draft.
                (Element::Room(a), Element::Room(v)) => assert_eq!((&a.seed, &a.level_id), (&v.seed, &v.level_id)),
                _ => assert_eq!(el, in_view),
            }
        }
        assert_eq!(d.replay(&[0, 1]).unwrap(), d.view, "every step in order is the view");
    }

    #[test]
    fn a_later_command_of_a_step_finds_the_id_an_earlier_one_made() {
        // Review P0-1: command 2 names what command 1 made, in one step.
        let mut d = Draft::new(defaults::new_project("t"));
        d.step(0, vec![seg((0.0, 10_000.0), (5000.0, 10_000.0), None)]);
        let made = Staging::rebuilt(&d.view, "probe", &[seg((0.0, 0.0), (6000.0, 0.0), None)]).unwrap();
        let wall_id = made.diff().added[0].clone();
        let step = vec![Command::Batch {
            label: "wall and door".into(),
            commands: vec![seg((0.0, 0.0), (6000.0, 0.0), None), door(&wall_id, 2000.0)],
        }];
        let staged = d.step(1, step.clone());
        assert!(staged.project().elements.iter().any(|e| e.id() == &wall_id));
        assert_eq!(d.steps[1].2.leaves.len(), 2, "a batch records leaf by leaf");
        // Without part 0 the plain run makes another wall id, so the door fails.
        let plain = refused(Staging::rebuilt(&d.base, "step", &step));
        assert_eq!(code(&plain), "unknown_wall", "{plain:?}");
        let alone = d.replay(&[1]).unwrap();
        let door_el = alone.elements.iter().find_map(|e| match e {
            Element::Opening(o) => Some(o),
            _ => None,
        });
        assert_eq!(door_el.map(|o| &o.wall_id), Some(&wall_id));
    }

    #[test]
    fn rooms_and_types_are_found_by_their_key_in_any_order() {
        let project = defaults::new_project("t");
        let cmd = Command::DeleteElements { ids: vec![] };
        let a = Slot::room("L1", &["w2".into(), "w1".into()]);
        let b = Slot::room("L1", &["w3".into(), "w4".into()]);
        let t = Slot::Type { opening_id: "o1".into() };
        let pin = |slot: &Slot, id: &str| Pin { slot: slot.clone(), nth: 0, id: id.into(), kind: String::new() };
        let tape = vec![LeafPins { pins: vec![pin(&a, "room-a"), pin(&b, "room-b"), pin(&t, "type-1")] }];
        let (got, leaves) = with_tape(Some(tape), || {
            let mut ids = crate::ids::IdGen::new(&project, &cmd);
            [ids.next_keyed(t.clone()), ids.next_keyed(b.clone()), ids.next_keyed(a.clone()), ids.next_keyed(Slot::room("L1", &["w9".into()]))]
        });
        assert_eq!(&got[..3], ["type-1", "room-b", "room-a"]);
        assert_eq!(got[3].len(), 36, "a slot that was not recorded gets a new id");
        assert_eq!(Slot::room("L1", &["w1".into(), "w2".into(), "w1".into()]), a, "walls sorted, once each");
        assert_eq!(leaves[0].pins.len(), 4);
    }

    #[test]
    fn a_step_whose_room_cannot_be_made_again_is_lost_id_and_nothing_is_relabelled() {
        // The draft closed a square: one room around four walls. On a model
        // where a wall already splits that square, the step makes two other
        // rooms: its room is lost, never given to one of them.
        let base = defaults::new_project("t");
        let square = vec![rect(0.0, 0.0, 6000.0, 4000.0, None)];
        let (staged, rec) = record(&base, "square", &square).unwrap();
        let room_id = staged.diff().added.into_iter().find(|id| {
            staged.project().elements.iter().any(|e| matches!(e, Element::Room(r) if &r.id == id))
        });
        let split = Staging::rebuilt(&base, "split", &[seg((3000.0, 0.0), (3000.0, 4000.0), None)]).unwrap();
        let e = refused(replay(split.project(), "square", &square, &rec));
        assert_eq!(code(&e), "lost_id", "{e:?}");
        let CoreError::Invalid { element_ids, .. } = &e else { panic!() };
        assert_eq!(element_ids.first(), room_id.as_ref(), "the room's id is named, not reused");
    }

    #[test]
    fn a_step_on_an_element_that_changed_is_changed_on_main() {
        let mut d = Draft::new(defaults::new_project("t"));
        let shell = d.step(0, vec![seg((0.0, 0.0), (6000.0, 0.0), None)]);
        let wall_id = shell.diff().added[0].clone();
        d.step(1, vec![door(&wall_id, 2000.0)]);
        // Someone lengthened the wall on the model since.
        let moved = Staging::rebuilt(
            &d.view,
            "move",
            &[Command::SetWallEndpoints { wall_id: wall_id.clone(), start: pt(0.0, 0.0), end: pt(7000.0, 0.0) }],
        )
        .unwrap();
        let mut on = d.base.clone();
        on = replay(&on, "step", &d.steps[0].1, &d.steps[0].2).unwrap().project().clone();
        let mut changed = on.clone();
        changed.elements = moved.project().elements.clone();
        let e = refused(replay(&changed, "step", &d.steps[1].1, &d.steps[1].2));
        assert_eq!(code(&e), "changed_on_main", "{e:?}");
        // The door's part alone, without the wall's: lost, not not_found.
        let e = d.replay(&[1]).unwrap_err();
        assert_eq!(code(&e), "lost_id", "{e:?}");
        assert!(replay(&on, "step", &d.steps[1].1, &d.steps[1].2).is_ok(), "with the wall as drawn it applies");
    }

    // ----------------------------------------------------- property test

    /// splitmix64: a small seeded generator, so runs repeat.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    /// Random drafts of three parts, each on its own level, plus a setup
    /// step that adds the levels: any set of parts replayed with setup, in
    /// recorded order with pinned ids, gives every element its draft id and
    /// kind; elements differ from the view only in a room's count-based
    /// name. All parts together give the view exactly.
    #[test]
    fn any_set_of_parts_replays_with_the_draft_ids_and_never_relabels() {
        for seed in 0..40u64 {
            let mut rng = Rng(seed);
            let mut d = Draft::new(defaults::new_project("t"));
            let setup = d.step(
                9,
                vec![
                    Command::AddLevel { name: Some("L2".into()), elevation_mm: None, height_mm: None },
                    Command::AddLevel { name: Some("L3".into()), elevation_mm: None, height_mm: None },
                ],
            );
            let levels: Vec<Id> = setup.project().levels.iter().map(|l| l.id.clone()).collect();
            let mut walls: Vec<Vec<Id>> = vec![vec![]; 3];
            let mut cells = [0usize; 3];
            for _ in 0..(6 + rng.below(10)) {
                let part = rng.below(3);
                let level = Some(levels[part].clone());
                let op = rng.below(4);
                let commands = match op {
                    0 | 1 => {
                        let c = cells[part] as f64;
                        cells[part] += 1;
                        vec![rect(c * 9000.0, 0.0, 4000.0 + rng.below(3) as f64 * 1000.0, 3000.0 + rng.below(3) as f64 * 1000.0, level)]
                    }
                    2 if !walls[part].is_empty() => {
                        let w = walls[part][rng.below(walls[part].len())].clone();
                        vec![door(&w, 1200.0)]
                    }
                    _ => {
                        let c = cells[part] as f64;
                        cells[part] += 1;
                        vec![seg((c * 9000.0, 20_000.0), (c * 9000.0 + 3000.0, 20_000.0), level)]
                    }
                };
                // A door that does not fit is refused by the engine: skip it.
                let Ok((staging, rec)) = record(&d.view, "step", &commands) else { continue };
                for id in &staging.diff().added {
                    if staging.project().elements.iter().any(|e| matches!(e, Element::Wall(w) if &w.id == id)) {
                        walls[part].push(id.clone());
                    }
                }
                d.view = staging.project().clone();
                d.steps.push((part, commands, rec));
            }
            assert_eq!(d.replay(&[9, 0, 1, 2]).unwrap(), d.view, "seed {seed}: all parts are the view");
            for set in [vec![9, 0], vec![9, 1], vec![9, 2], vec![9, 0, 2], vec![9, 1, 2]] {
                let got = d.replay(&set).unwrap_or_else(|e| panic!("seed {seed}, parts {set:?}: {e:?}"));
                let view: BTreeMap<&Id, &Element> = d.view.elements.iter().map(|e| (e.id(), e)).collect();
                for el in &got.elements {
                    let v = view.get(el.id()).unwrap_or_else(|| panic!("seed {seed}: {} is not a draft id", el.id()));
                    assert_eq!(el.kind(), v.kind(), "seed {seed}: an id names another kind");
                    match (el, v) {
                        (Element::Room(a), Element::Room(b)) => {
                            assert_eq!((&a.seed, &a.level_id), (&b.seed, &b.level_id), "seed {seed}")
                        }
                        _ => assert_eq!(el, *v, "seed {seed}, parts {set:?}"),
                    }
                }
                let want: BTreeSet<&Id> =
                    d.steps.iter().filter(|s| set.contains(&s.0)).flat_map(|s| s.2.made()).filter(|id| view.contains_key(id)).collect();
                let have: BTreeSet<&Id> = got.elements.iter().map(|e| e.id()).collect();
                assert!(want.is_subset(&have), "seed {seed}: every element the parts made is there");
            }
        }
    }
}
