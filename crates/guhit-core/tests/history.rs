//! Edit history (DECISIONS D38): what each step records, `Document::history`,
//! and taking back or bringing back several steps as one revision.

mod common;

use common::*;
use guhit_core::{templates, CoreError, Document, MAX_HISTORY, MAX_STEP_IDS};
use guhit_model::*;

const AT: &str = "2026-10-07T01:02:03Z";

fn same_state(doc: &Document) -> String {
    serde_json::to_string(&doc.state()).unwrap()
}

#[test]
fn a_step_records_its_time_summary_author_and_elements() {
    let mut doc = blank();
    let added = doc.apply_at(add_wall(p(0.0, 0.0), p(4000.0, 0.0)), Origin::Ai, Some("p1".into()), AT).unwrap();
    let wall = added.diff.added[0].clone();
    let h = doc.history();
    assert!(h.redo.is_empty());
    let step = &h.undo[0];
    assert_eq!(step.label, "Add wall");
    assert_eq!(step.summary, added.diff.summary);
    assert_eq!(step.origin, Origin::Ai);
    assert_eq!(step.author_id.as_deref(), Some("p1"));
    assert_eq!((step.author_name.clone(), step.color), (None, None), "names are the app's to fill in");
    assert_eq!(step.at, AT);
    let touched: Vec<Id> = added.diff.added.iter().chain(&added.diff.modified).cloned().collect();
    assert_eq!(step.ids, touched);
    assert!(step.removed.is_empty());

    // `apply` and `apply_as` record no time and no author.
    let deleted = doc.apply(Command::DeleteElements { ids: vec![wall.clone()] }, Origin::User).unwrap();
    let h = doc.history();
    assert_eq!(h.undo.len(), 2);
    assert_eq!(h.undo[0].label, "Delete", "newest first");
    assert_eq!((h.undo[0].at.as_str(), h.undo[0].author_id.clone()), ("", None));
    assert!(h.undo[0].removed.contains(&wall));
    assert_eq!(h.undo[0].summary, deleted.diff.summary);
    assert_eq!(h.undo[1].label, "Add wall");

    // An undone step keeps everything it recorded.
    doc.undo().unwrap();
    let h = doc.history();
    assert_eq!(h.undo.len(), 1);
    assert_eq!(h.redo.len(), 1);
    assert_eq!(h.redo[0].label, "Delete");
    assert!(h.redo[0].removed.contains(&wall));
    doc.redo().unwrap();
    assert_eq!(doc.history().undo[0].summary, deleted.diff.summary);
}

#[test]
fn undo_steps_takes_back_several_as_one_revision() {
    let mut doc = Document::new(templates::sample_bungalow());
    let original = doc.project().clone();
    let mut after = vec![];
    for i in 0..4 {
        let y = -2000.0 - 1000.0 * i as f64;
        let r = doc.apply_at(add_wall(p(0.0, y), p(3000.0, y)), Origin::User, None, AT).unwrap();
        after.push(r.state.project.clone());
    }
    assert_eq!(doc.revision(), 4);

    let s = doc.undo_steps(3).unwrap();
    assert_eq!(s.revision, 5, "one revision for three steps");
    assert_eq!(s.project, after[0], "exactly as after the first step");
    assert!(s.can_undo && s.can_redo);
    let h = doc.history();
    assert_eq!((h.undo.len(), h.redo.len()), (1, 3));
    // The redo stack is in order: the next redo is the second step.
    assert_eq!(doc.redo().unwrap().project, after[1]);
    assert_eq!(doc.revision(), 6);

    let s = doc.redo_steps(2).unwrap();
    assert_eq!(s.revision, 7);
    assert_eq!(s.project, after[3]);
    assert!(!s.can_redo);

    let s = doc.undo_steps(4).unwrap();
    assert_eq!(s.project, original, "all the way back, timestamps included");
    assert!(!s.can_undo);
    assert_eq!(s.redo_label.as_deref(), Some("Add wall"));
    let s = doc.redo_steps(4).unwrap();
    assert_eq!(s.project, after[3]);
    assert_eq!(s.revision, 9);
}

#[test]
fn several_steps_are_all_or_nothing() {
    let mut doc = blank();
    assert!(matches!(doc.undo_steps(2), Err(CoreError::NothingToUndo)));
    assert!(matches!(doc.redo_steps(1), Err(CoreError::NothingToRedo)));
    for i in 0..3 {
        let y = 1000.0 * i as f64;
        doc.apply(add_wall(p(0.0, y), p(3000.0, y)), Origin::User).unwrap();
    }
    let before = same_state(&doc);
    let err = doc.undo_steps(0).unwrap_err();
    assert_eq!(code(&err), "bad_steps");
    let err = doc.undo_steps(4).unwrap_err();
    assert_eq!(code(&err), "too_many_steps");
    assert_eq!(err.to_string(), "Only 3 steps can be undone, not 4.");
    assert_eq!(same_state(&doc), before, "a refused undo changes nothing, not even the revision");

    doc.undo_steps(2).unwrap();
    let before = same_state(&doc);
    let err = doc.redo_steps(3).unwrap_err();
    assert_eq!(err.to_string(), "Only 2 steps can be redone, not 3.");
    assert_eq!(code(&doc.redo_steps(0).unwrap_err()), "bad_steps");
    assert_eq!(same_state(&doc), before);
    doc.undo().unwrap();
    assert_eq!(doc.undo_steps(2).unwrap_err().to_string(), "nothing to undo");
}

#[test]
fn a_step_keeps_at_most_max_step_ids() {
    let mut doc = blank();
    let n = MAX_STEP_IDS + 5;
    let commands = (0..n)
        .map(|i| {
            let y = 1000.0 * i as f64;
            add_wall(p(0.0, y), p(2000.0, y))
        })
        .collect();
    let added = doc.apply(Command::Batch { label: "Many walls".into(), commands }, Origin::User).unwrap();
    assert_eq!(added.diff.added.len(), n);
    let step = &doc.history().undo[0];
    assert_eq!(step.ids.len(), MAX_STEP_IDS);
    assert_eq!(step.ids[..], added.diff.added[..MAX_STEP_IDS]);

    doc.apply(Command::DeleteElements { ids: added.diff.added.clone() }, Origin::User).unwrap();
    let step = &doc.history().undo[0];
    assert_eq!(step.removed.len(), MAX_STEP_IDS);
    assert!(step.ids.is_empty());
}

#[test]
fn history_keeps_the_newest_max_history_steps() {
    let mut doc = blank();
    for i in 0..(MAX_HISTORY + 3) {
        let y = 1000.0 * i as f64;
        doc.apply_at(add_wall(p(0.0, y), p(2000.0, y)), Origin::User, None, &format!("step {i}")).unwrap();
    }
    let h = doc.history();
    assert_eq!(h.undo.len(), MAX_HISTORY);
    assert_eq!(h.undo[0].at, format!("step {}", MAX_HISTORY + 2));
    assert_eq!(h.undo[MAX_HISTORY - 1].at, "step 3", "the oldest three went");
    let s = doc.undo_steps(MAX_HISTORY).unwrap();
    assert!(!s.can_undo);
    assert_eq!(doc.history().redo.len(), MAX_HISTORY);
}

#[test]
fn step_projects_walk_the_states_an_undo_or_redo_passes_through() {
    let mut doc = blank();
    let start = doc.project().clone();
    doc.apply(add_wall(p(0.0, 0.0), p(4000.0, 0.0)), Origin::User).unwrap();
    let one = doc.project().clone();
    doc.apply(add_wall(p(0.0, 3000.0), p(4000.0, 3000.0)), Origin::User).unwrap();
    let two = doc.project().clone();

    // Undo: one step back, then two; fewer when fewer are kept.
    let back: Vec<&Project> = doc.step_projects(false, 5).collect();
    assert_eq!(back, vec![&one, &start]);
    assert_eq!(doc.step_projects(false, 1).count(), 1);
    assert_eq!(doc.step_projects(true, 3).count(), 0);

    // Each is what an undo or redo leaves.
    doc.undo_steps(2).unwrap();
    assert_eq!(doc.project(), &start);
    let forward: Vec<&Project> = doc.step_projects(true, 2).collect();
    assert_eq!(forward, vec![&one, &two]);
    doc.redo().unwrap();
    assert_eq!(doc.project(), &one);
    assert_eq!(doc.step_projects(true, 1).next(), Some(&two));
    assert_eq!(doc.step_projects(false, 1).next(), Some(&start));
}
