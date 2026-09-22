//! Dimensions follow the geometry they measure.
//!
//! A drawing whose dimensions disagree with its geometry is worse than one
//! with no dimensions, so a linear dimension that was snapped to a wall moves
//! with that wall, inside the same command and the same undo step.
//!
//! Nothing new is stored in the model. Attachment is decided per command from
//! the state BEFORE it: an endpoint is attached when it lies within 1 mm of a
//! wall centerline endpoint (a joint) or of a wall outline corner, so
//! dimensions drawn to wall faces follow too. The endpoint then moves by the
//! displacement of that anchor. When two anchors at the same old point went to
//! different places the endpoint stays where it is: a guess would be worse
//! than leaving the drawing as the user left it.

use std::collections::BTreeMap;

use guhit_model::*;

use crate::geom::*;
use crate::topo::analyze;
use crate::validate::validate_element;

/// Displacements this close are the same displacement. Real disagreement is
/// millimeters apart; this only absorbs float dust from mitred corners.
const SAME_MOVE_EPS: f64 = 1e-6;

/// Where one wall joint or outline corner was, and where it went.
struct Anchor {
    level_id: Id,
    old: Point,
    new: Point,
}

impl Anchor {
    fn delta(&self) -> Point {
        sub(self.new, self.old)
    }
}

/// True for the commands whose job is to move walls that already exist.
/// A `Batch` is not listed: `run` handles every leaf on its own, so a
/// dimension follows through each step of a batch.
pub fn moves_walls(command: &Command) -> bool {
    matches!(
        command,
        Command::SetWallEndpoints { .. }
            | Command::SetWallLength { .. }
            | Command::MoveElements { .. }
            | Command::ResizeRoom { .. }
            | Command::RotateElements { .. }
    )
}

/// Move the endpoints of attached dimensions by the displacement of the wall
/// joint or outline corner they were snapped to. Returns how many dimensions
/// changed.
pub fn follow_walls(before: &Project, after: &mut Project) -> usize {
    if !before
        .elements
        .iter()
        .any(|e| matches!(e, Element::Dimension(_)))
    {
        return 0;
    }
    let anchors = moved_anchors(before, after);
    if anchors.is_empty() {
        return 0;
    }
    let old: BTreeMap<&Id, &Dimension> = before
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Dimension(d) => Some((&d.id, d)),
            _ => None,
        })
        .collect();

    let mut updates: Vec<(usize, Point, Point)> = vec![];
    for (i, el) in after.elements.iter().enumerate() {
        let Element::Dimension(d) = el else { continue };
        let Some(was) = old.get(&d.id) else { continue };
        // A dimension the command moved itself is never moved twice.
        if was.a != d.a || was.b != d.b {
            continue;
        }
        let a = displaced(&anchors, &d.level_id, d.a);
        let b = displaced(&anchors, &d.level_id, d.b);
        if a != d.a || b != d.b {
            updates.push((i, a, b));
        }
    }

    let mut changed = 0;
    for (i, a, b) in updates {
        let Element::Dimension(d) = &mut after.elements[i] else {
            continue;
        };
        let kept = (d.a, d.b);
        d.a = a;
        d.b = b;
        let moved = after.elements[i].clone();
        // A dimension never breaks the wall edit that dragged it. If following
        // would leave it invalid it stays where the user put it.
        if validate_element(after, &moved).is_err() {
            if let Element::Dimension(d) = &mut after.elements[i] {
                d.a = kept.0;
                d.b = kept.1;
            }
        } else {
            changed += 1;
        }
    }
    changed
}

/// The new position of `p`, or `p` itself when it is attached to nothing or
/// the answer is ambiguous.
fn displaced(anchors: &[Anchor], level_id: &Id, p: Point) -> Point {
    let mut found: Option<Point> = None;
    for a in anchors {
        if &a.level_id != level_id || dist(a.old, p) > JOIN_EPS {
            continue;
        }
        let d = a.delta();
        match found {
            // Two walls met here and went different ways. Leave it alone.
            Some(f) if dist(f, d) > SAME_MOVE_EPS => return p,
            Some(_) => {}
            None => found = Some(d),
        }
    }
    match found {
        Some(d) => add(p, d),
        None => p,
    }
}

/// Every wall joint and outline corner of `before`, paired with where it is in
/// `after`. Empty when no wall centerline moved. Anchors that did not move are
/// kept: they are what makes a broken joint ambiguous.
fn moved_anchors(before: &Project, after: &Project) -> Vec<Anchor> {
    let new: BTreeMap<&Id, &Wall> = after
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) => Some((&w.id, w)),
            _ => None,
        })
        .collect();
    let olds: Vec<&Wall> = before
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) => Some(w),
            _ => None,
        })
        .collect();

    let mut anchors: Vec<Anchor> = vec![];
    let mut any_move = false;
    for w in &olds {
        let Some(n) = new.get(&w.id) else { continue };
        if n.level_id != w.level_id {
            continue;
        }
        for (old, new) in [(w.start, n.start), (w.end, n.end)] {
            if dist(old, new) > SAME_MOVE_EPS {
                any_move = true;
            }
            anchors.push(Anchor {
                level_id: w.level_id.clone(),
                old,
                new,
            });
        }
    }
    if !any_move {
        return vec![];
    }

    // Outline corners, so a dimension drawn to a wall face follows as well.
    // A wall whose outline gained or lost a corner has no corner to corner
    // answer, so only its centerline joints speak for it.
    let (was, is) = (analyze(before), analyze(after));
    for w in &olds {
        let (Some(n), Some(ot), Some(nt)) = (new.get(&w.id), was.wall(&w.id), is.wall(&w.id))
        else {
            continue;
        };
        if n.level_id != w.level_id || ot.outline.len() != nt.outline.len() {
            continue;
        }
        for (old, new) in ot.outline.iter().zip(nt.outline.iter()) {
            anchors.push(Anchor {
                level_id: w.level_id.clone(),
                old: *old,
                new: *new,
            });
        }
    }
    anchors
}
