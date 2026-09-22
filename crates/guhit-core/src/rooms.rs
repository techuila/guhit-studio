//! Seed based rooms (DECISIONS D7).
//!
//! A `Room` element owns the closed wall face that contains its seed. After
//! every command `reconcile` runs inside the same undo step:
//! - a face with no room gets a new auto-named room,
//! - a room whose seed is in no face is removed,
//! - two rooms in one face: the older one stays,
//! - a room whose face clearly persists (same bounding walls) keeps its name
//!   even when a wall move left the seed outside; the seed moves to the new
//!   label point.

use std::collections::{BTreeMap, BTreeSet};

use guhit_model::*;

use crate::ids::IdGen;
use crate::topo::{analyze, Analysis, Face};

/// (level index, face index) into `Analysis::levels[..].faces`.
pub type FaceRef = (usize, usize);

/// Rooms matched to faces by seed alone, first room in element order wins.
/// This is what `compute_derived` uses, and what `reconcile` guarantees to
/// be complete and conflict free.
pub fn assign_by_seed(project: &Project, analysis: &Analysis) -> BTreeMap<Id, FaceRef> {
    let mut taken: BTreeSet<FaceRef> = BTreeSet::new();
    let mut out = BTreeMap::new();
    for el in &project.elements {
        if let Element::Room(r) = el {
            if let Some(face) = face_ref_at(analysis, &r.level_id, r.seed) {
                if taken.insert(face) {
                    out.insert(r.id.clone(), face);
                }
            }
        }
    }
    out
}

fn face_ref_at(analysis: &Analysis, level_id: &str, p: Point) -> Option<FaceRef> {
    if !(p.x.is_finite() && p.y.is_finite()) {
        return None;
    }
    let li = analysis
        .levels
        .iter()
        .position(|l| l.level_id == level_id)?;
    analysis.face_at(level_id, p).map(|fi| (li, fi))
}

pub fn face(analysis: &Analysis, r: FaceRef) -> &Face {
    &analysis.levels[r.0].faces[r.1]
}

fn wall_set(face: &Face) -> BTreeSet<Id> {
    face.wall_ids.iter().cloned().collect()
}

fn jaccard(a: &BTreeSet<Id>, b: &BTreeSet<Id>) -> f64 {
    let inter = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        inter / union
    }
}

/// Smallest "Room N" that no room uses yet.
pub fn next_room_name(project: &Project) -> String {
    let used: BTreeSet<&str> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Room(r) => Some(r.name.as_str()),
            _ => None,
        })
        .collect();
    let mut n = 1;
    loop {
        let name = format!("Room {n}");
        if !used.contains(name.as_str()) {
            return name;
        }
        n += 1;
    }
}

pub struct Reconciled {
    pub analysis: Analysis,
    pub assigned: BTreeMap<Id, FaceRef>,
}

/// Bring the room elements in line with the closed faces of `project`.
/// `before` is the project as it was before the command.
pub fn reconcile(before: &Project, project: &mut Project, ids: &mut IdGen) -> Reconciled {
    let old_analysis = analyze(before);
    let old_sets: BTreeMap<Id, BTreeSet<Id>> = assign_by_seed(before, &old_analysis)
        .into_iter()
        .map(|(id, f)| (id, wall_set(face(&old_analysis, f))))
        .collect();
    let analysis = analyze(project);

    let rooms: Vec<(Id, Id, Point)> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Room(r) => Some((r.id.clone(), r.level_id.clone(), r.seed)),
            _ => None,
        })
        .collect();

    let mut taken: BTreeSet<FaceRef> = BTreeSet::new();
    let mut assigned: BTreeMap<Id, FaceRef> = BTreeMap::new();

    // Pass A: the face with exactly the same bounding walls as before.
    for (id, level_id, seed) in &rooms {
        let Some(old) = old_sets.get(id) else {
            continue;
        };
        let Some(li) = analysis.levels.iter().position(|l| &l.level_id == level_id) else {
            continue;
        };
        let mut pick: Option<FaceRef> = None;
        for (fi, f) in analysis.levels[li].faces.iter().enumerate() {
            if taken.contains(&(li, fi)) || &wall_set(f) != old {
                continue;
            }
            if f.contains(*seed) {
                pick = Some((li, fi));
                break;
            }
            pick = pick.or(Some((li, fi)));
        }
        if let Some(p) = pick {
            taken.insert(p);
            assigned.insert(id.clone(), p);
        }
    }

    // Pass B: the face that contains the seed. Older rooms go first.
    for (id, level_id, seed) in &rooms {
        if assigned.contains_key(id) {
            continue;
        }
        if let Some(f) = face_ref_at(&analysis, level_id, *seed) {
            if taken.insert(f) {
                assigned.insert(id.clone(), f);
            }
        }
    }

    // Pass C: a free face that still has most of the old bounding walls.
    for (id, level_id, _) in &rooms {
        if assigned.contains_key(id) {
            continue;
        }
        let Some(old) = old_sets.get(id) else {
            continue;
        };
        let Some(li) = analysis.levels.iter().position(|l| &l.level_id == level_id) else {
            continue;
        };
        let mut best: Option<(FaceRef, f64)> = None;
        for (fi, f) in analysis.levels[li].faces.iter().enumerate() {
            if taken.contains(&(li, fi)) {
                continue;
            }
            let score = jaccard(old, &wall_set(f));
            if score >= 0.6 && best.map(|(_, s)| score > s).unwrap_or(true) {
                best = Some(((li, fi), score));
            }
        }
        if let Some((f, _)) = best {
            taken.insert(f);
            assigned.insert(id.clone(), f);
        }
    }

    // Drop rooms without a face, fix seeds that ended up in a wall.
    project.elements.retain(|e| match e {
        Element::Room(r) => assigned.contains_key(&r.id),
        _ => true,
    });
    for el in project.elements.iter_mut() {
        if let Element::Room(r) = el {
            let f = face(&analysis, assigned[&r.id]);
            let smallest_here = analysis
                .face_at(&r.level_id, r.seed)
                .map(|fi| (assigned[&r.id].0, fi))
                == Some(assigned[&r.id]);
            if !smallest_here || !f.contains_net(r.seed) {
                r.seed = f.label;
            }
        }
    }

    // New rooms for free faces.
    for (li, level) in analysis.levels.iter().enumerate() {
        for (fi, f) in level.faces.iter().enumerate() {
            if taken.contains(&(li, fi)) {
                continue;
            }
            let id = ids.next_id();
            let name = next_room_name(project);
            project.elements.push(Element::Room(Room {
                id: id.clone(),
                level_id: level.level_id.clone(),
                name,
                usage: RoomUsage::Other,
                seed: f.label,
                floor_material_id: project
                    .materials
                    .iter()
                    .find(|m| m.id == defaults::MAT_FLOOR_DEFAULT)
                    .map(|m| m.id.clone()),
                auto_named: true,
            }));
            taken.insert((li, fi));
            assigned.insert(id, (li, fi));
        }
    }

    Reconciled { analysis, assigned }
}
