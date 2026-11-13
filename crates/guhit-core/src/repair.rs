//! What opening a project mends (GR1 #14). A damaged, hand-edited or shared
//! file must never crash the engine on every open. `Document::with_revision`
//! runs `repair` after `migrate`, and `Document::repairs` keeps its notes for
//! the app to show.
//!
//! Only what the engine cannot use goes: a second element with an id already
//! taken, a tour shot with no camera key, and an element on a level, or in a
//! wall, that is gone, or with a number that is not a number. An element
//! that only breaks a rule added since the file was saved stays as it is: a
//! person's work is never dropped for a rule.

use std::collections::BTreeSet;

use guhit_model::*;

use crate::error::CoreError;
use crate::exec::{count_noun, kind_noun};
use crate::validate::validate_element;

/// Rule codes that mean the engine cannot use the element at all.
const BROKEN: [&str; 4] = ["not_finite", "unknown_level", "unknown_wall", "not_a_wall"];

/// Mend `project` in place. Returns one plain sentence per repair; empty
/// when the project is sound, which leaves it unchanged.
pub fn repair(project: &mut Project) -> Vec<String> {
    let mut notes = vec![];

    // Two elements with one id: the first keeps it.
    let mut seen: BTreeSet<Id> = BTreeSet::new();
    let mut doubled: Vec<String> = vec![];
    project.elements.retain(|e| {
        let first = seen.insert(e.id().clone());
        if !first {
            doubled.push(format!("Removed a second {} with the id {}.", kind_noun(e.kind()), e.id()));
        }
        first
    });
    notes.extend(doubled);

    // A shot with no camera key has no picture to show.
    for el in project.elements.iter_mut() {
        if let Element::Tour(t) = el {
            let had = t.shots.len();
            t.shots.retain(|s| !s.keys.is_empty());
            let gone = had - t.shots.len();
            if gone > 0 {
                notes.push(format!("Tour \"{}\": removed {} with no camera keys.", t.name, count_noun(gone, "shot")));
            }
        }
    }

    // One at a time, from the start again: a wall that goes takes its doors
    // and windows with it on the next pass.
    loop {
        let broken = project.elements.iter().enumerate().find_map(|(i, el)| match validate_element(project, el) {
            Err(CoreError::Invalid { code, message, .. }) if BROKEN.contains(&code.as_str()) => Some((i, message)),
            _ => None,
        });
        let Some((i, why)) = broken else { break };
        let el = project.elements.remove(i);
        notes.push(format!("Removed {} {}: {why}", kind_noun(el.kind()), el.id()));
    }
    notes
}
