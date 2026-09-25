//! Command execution. `execute` mutates a working copy of the project; the
//! caller (`Document`) only commits the copy when it returns Ok, so every
//! command is atomic and a `Batch` is all or nothing.

use std::collections::{BTreeMap, BTreeSet};

use guhit_model::*;

use crate::dimensions;
use crate::error::CoreError;
use crate::geom::*;
use crate::ids::IdGen;
use crate::issues::clean_review_target;
use crate::rooms::{assign_by_seed, face, next_room_name, reconcile};
use crate::topo::analyze;
use crate::validate::*;

pub struct Outcome {
    /// Short undo label, for example "Add wall".
    pub label: String,
    /// Optional sentence that describes the change better than the counts.
    pub summary: Option<String>,
}

fn outcome(label: &str) -> Outcome {
    Outcome {
        label: label.to_string(),
        summary: None,
    }
}

/// Run `command` on `project`. On Err the project may be half changed and
/// must be thrown away.
pub fn execute(project: &mut Project, command: &Command) -> Result<Outcome, CoreError> {
    run(project, command)
}

fn run(project: &mut Project, command: &Command) -> Result<Outcome, CoreError> {
    if let Command::Batch { label, commands } = command {
        if commands.is_empty() {
            return Err(CoreError::invalid(
                "empty_batch",
                "The batch has no commands.",
            ));
        }
        for (i, c) in commands.iter().enumerate() {
            run(project, c).map_err(|e| match e {
                CoreError::Invalid {
                    code,
                    message,
                    element_ids,
                } => CoreError::Invalid {
                    code,
                    message: format!("Step {} of {}: {}", i + 1, commands.len(), message),
                    element_ids,
                },
                other => other,
            })?;
        }
        let label = if label.trim().is_empty() {
            "Batch"
        } else {
            label.trim()
        };
        return Ok(outcome(label));
    }

    // Ids are seeded per leaf command, from the project as it is right now
    // and this command alone, never from an enclosing batch. An element made
    // by step N of a batch keeps its id when step N + 1 is appended.
    let mut ids = IdGen::new(project, command);
    let ids = &mut ids;
    let before = project.clone();
    let mut out = leaf(project, command, ids)?;
    post_validate(&before, project)?;
    // Deleted objects leave every `links` list in this same command. This
    // runs after `post_validate` on purpose, like the dimensions below: a
    // link to an object that is gone says nothing, so a locked layer does
    // not keep it.
    if matches!(
        command,
        Command::DeleteElements { .. } | Command::DeleteLevel { .. }
    ) {
        prune_links(project);
    }
    // A dimension snapped to a wall follows it, in this same command and undo
    // step. This runs after `post_validate` on purpose: a dimension is derived
    // from the geometry it measures, so a locked Dimensions layer does not
    // stop it from staying true.
    if dimensions::moves_walls(command) {
        let followed = dimensions::follow_walls(&before, project);
        if followed > 0 {
            if let Some(s) = &mut out.summary {
                s.push_str(&format!(", updated {}", plural(followed, "dimension")));
            }
        }
    }
    let rec = reconcile(&before, project, ids);

    match command {
        Command::AddRectRoom {
            origin,
            width_mm,
            depth_mm,
            name: Some(name),
            level_id,
            ..
        } if !name.trim().is_empty() => {
            let level = resolve_level(project, level_id)?;
            let center = pt(origin.x + width_mm / 2.0, origin.y + depth_mm / 2.0);
            let target = rec
                .assigned
                .iter()
                .filter(|(_, f)| {
                    let fc = face(&rec.analysis, **f);
                    fc.level_id == level && fc.contains(center)
                })
                .min_by(|a, b| {
                    face(&rec.analysis, *a.1)
                        .centerline_area_mm2
                        .total_cmp(&face(&rec.analysis, *b.1).centerline_area_mm2)
                })
                .map(|(id, _)| id.clone());
            if let Some(id) = target {
                for el in project.elements.iter_mut() {
                    if let Element::Room(r) = el {
                        if r.id == id {
                            r.name = name.trim().to_string();
                            r.auto_named = false;
                        }
                    }
                }
            }
        }
        Command::ResizeRoom { room_id, .. } => {
            let lost: Vec<Id> = before
                .elements
                .iter()
                .filter(|e| matches!(e, Element::Room(_)))
                .map(|e| e.id().clone())
                .filter(|id| !project.elements.iter().any(|e| e.id() == id))
                .collect();
            if !lost.is_empty() {
                let mut element_ids = vec![room_id.clone()];
                element_ids.extend(lost.into_iter().filter(|id| id != room_id));
                return Err(CoreError::invalid_for(
                    "resize_breaks_room",
                    "This resize would collapse a room or merge it with its neighbor. Try a smaller distance.",
                    element_ids,
                ));
            }
        }
        _ => {}
    }
    Ok(out)
}

/// The level an element stands on. Openings (on their wall) and cameras
/// have none of their own.
fn element_level(el: &Element) -> Option<&Id> {
    match el {
        Element::Wall(e) => Some(&e.level_id),
        Element::Room(e) => Some(&e.level_id),
        Element::Column(e) => Some(&e.level_id),
        Element::Stair(e) => Some(&e.level_id),
        Element::Asset(e) => Some(&e.level_id),
        Element::Annotation(e) => Some(&e.level_id),
        Element::Dimension(e) => Some(&e.level_id),
        Element::Underlay(e) => Some(&e.level_id),
        Element::Linework(e) => Some(&e.level_id),
        Element::ReferenceModel(e) => Some(&e.level_id),
        Element::Pipe(e) => Some(&e.level_id),
        Element::Opening(_) | Element::Camera(_) => None,
    }
}

/// "Level 2", or the next free number.
fn next_level_name(project: &Project) -> String {
    let mut n = project.levels.len() + 1;
    loop {
        let name = format!("Level {n}");
        if !project.levels.iter().any(|l| l.name.trim() == name) {
            return name;
        }
        n += 1;
    }
}

/// Remove, from every object's links, the ids of objects that no longer
/// exist. Returns how many objects lost a link.
fn prune_links(project: &mut Project) -> usize {
    let objects: BTreeSet<Id> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Asset(a) => Some(a.id.clone()),
            _ => None,
        })
        .collect();
    let mut changed = 0;
    for el in project.elements.iter_mut() {
        if let Element::Asset(a) = el {
            let before = a.links.len();
            a.links.retain(|l| objects.contains(l));
            if a.links.len() != before {
                changed += 1;
            }
        }
    }
    changed
}

/// A review mark target with its strings trimmed, without any other check:
/// how stored marks are compared.
fn trimmed_target(target: &ReviewTarget) -> ReviewTarget {
    match target {
        ReviewTarget::Issue { id } => ReviewTarget::Issue {
            id: id.trim().to_string(),
        },
        ReviewTarget::Check { code } => ReviewTarget::Check {
            code: code.trim().to_string(),
        },
        ReviewTarget::Element { code, element_id } => ReviewTarget::Element {
            code: code.trim().to_string(),
            element_id: element_id.trim().to_string(),
        },
    }
}

pub fn resolve_level(project: &Project, level_id: &Option<Id>) -> Result<Id, CoreError> {
    match level_id {
        Some(id) => {
            require_level(project, id)?;
            Ok(id.clone())
        }
        None => project
            .levels
            .first()
            .map(|l| l.id.clone())
            .ok_or_else(|| CoreError::invalid("unknown_level", "The project has no level.")),
    }
}

fn require_ids(project: &Project, ids: &[Id]) -> Result<Vec<Id>, CoreError> {
    if ids.is_empty() {
        return Err(CoreError::invalid(
            "empty_selection",
            "No elements were given.",
        ));
    }
    let mut out: Vec<Id> = vec![];
    for id in ids {
        if !project.elements.iter().any(|e| e.id() == id) {
            return Err(CoreError::NotFound(id.clone()));
        }
        if !out.contains(id) {
            out.push(id.clone());
        }
    }
    Ok(out)
}

fn finite_delta(name: &str, p: Point) -> Result<(), CoreError> {
    if is_finite(p) {
        Ok(())
    } else {
        Err(CoreError::invalid(
            "not_finite",
            format!("{name} is not a valid number."),
        ))
    }
}

fn plural(n: usize, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

fn new_wall(
    project: &Project,
    ids: &mut IdGen,
    level_id: &Id,
    start: Point,
    end: Point,
    thickness: Option<f64>,
) -> Wall {
    Wall {
        id: ids.next_id(),
        level_id: level_id.clone(),
        start,
        end,
        thickness_mm: thickness.unwrap_or(project.settings.default_wall_thickness_mm),
        height_mm: None,
        material_id: project
            .materials
            .iter()
            .find(|m| m.id == defaults::MAT_WALL_DEFAULT)
            .map(|m| m.id.clone()),
    }
}

/// True when an existing wall on the level already covers the whole segment.
fn covered_by_existing(project: &Project, level_id: &Id, a: Point, b: Point) -> bool {
    project.elements.iter().any(|e| match e {
        Element::Wall(w) if &w.level_id == level_id => {
            dist_point_segment(a, w.start, w.end) <= JOIN_EPS
                && dist_point_segment(b, w.start, w.end) <= JOIN_EPS
        }
        _ => false,
    })
}

// ------------------------------------------------------------ wall moving

struct WallMove {
    id: Id,
    start: Point,
    end: Point,
}

/// Set a wall's endpoints and keep its openings where they are in the plan:
/// each opening center is projected onto the new centerline.
fn set_wall_geometry(project: &mut Project, wall_id: &Id, start: Point, end: Point) {
    let Some((old_start, old_end)) = project.elements.iter().find_map(|e| match e {
        Element::Wall(w) if &w.id == wall_id => Some((w.start, w.end)),
        _ => None,
    }) else {
        return;
    };
    let old_dir = unit(sub(old_end, old_start));
    let new_dir = unit(sub(end, start));
    for el in project.elements.iter_mut() {
        match el {
            Element::Wall(w) if &w.id == wall_id => {
                w.start = start;
                w.end = end;
            }
            Element::Opening(o) if &o.wall_id == wall_id => {
                if let (Some(od), Some(nd)) = (old_dir, new_dir) {
                    let center = add(old_start, scale(od, o.offset_mm));
                    let offset = dot(sub(center, start), nd);
                    // Avoid float dust on plain translations.
                    o.offset_mm = if (offset - o.offset_mm).abs() < 1e-6 {
                        o.offset_mm
                    } else {
                        offset
                    };
                }
            }
            _ => {}
        }
    }
}

/// Move walls to new endpoints. With `stretch`, walls joined to a moved wall
/// follow so joints stay closed. `smart` is for rigid translations: a wall
/// that continues the moved wall in a straight line stays put when a cross
/// wall at the same joint can keep the plan closed. Returns the ids of the
/// walls that were stretched.
fn move_walls(project: &mut Project, moves: &[WallMove], stretch: bool, smart: bool) -> Vec<Id> {
    struct Moved {
        level: Id,
        old_s: Point,
        old_e: Point,
        new_s: Point,
        new_e: Point,
    }
    let moved_ids: BTreeSet<&Id> = moves.iter().map(|m| &m.id).collect();
    let mut moved: Vec<Moved> = vec![];
    for m in moves {
        if let Some(Element::Wall(w)) = project.elements.iter().find(|e| e.id() == &m.id) {
            moved.push(Moved {
                level: w.level_id.clone(),
                old_s: w.start,
                old_e: w.end,
                new_s: m.start,
                new_e: m.end,
            });
        }
    }
    let others: Vec<Wall> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) if !moved_ids.contains(&w.id) => Some(w.clone()),
            _ => None,
        })
        .collect();

    let mut follow: Vec<WallMove> = vec![];
    if stretch {
        for w in &others {
            let mut ends = [w.start, w.end];
            let mut touched = false;
            for (k, end) in ends.iter_mut().enumerate() {
                let p = *end;
                let far = if k == 0 { w.end } else { w.start };
                let mut target: Option<Point> = None;
                // Shared endpoint.
                for m in moved.iter().filter(|m| m.level == w.level_id) {
                    let (old_j, new_j) = if dist(p, m.old_s) <= JOIN_EPS {
                        (m.old_s, m.new_s)
                    } else if dist(p, m.old_e) <= JOIN_EPS {
                        (m.old_e, m.new_e)
                    } else {
                        continue;
                    };
                    if dist(old_j, new_j) < 1e-9 {
                        break;
                    }
                    if smart
                        && !joint_should_follow(
                            w, p, far, m.old_s, m.old_e, old_j, new_j, &others, &moved_ids,
                        )
                    {
                        break;
                    }
                    target = Some(new_j);
                    break;
                }
                // End resting on the body of a moved wall (T junction).
                if target.is_none()
                    && !moved
                        .iter()
                        .any(|m| dist(p, m.old_s) <= JOIN_EPS || dist(p, m.old_e) <= JOIN_EPS)
                {
                    for m in moved.iter().filter(|m| m.level == w.level_id) {
                        let len = dist(m.old_s, m.old_e);
                        let (along, d) = project_on_segment(p, m.old_s, m.old_e);
                        if d <= JOIN_EPS && along > 0.0 && along < len {
                            if dist_point_segment(p, m.new_s, m.new_e) > JOIN_EPS {
                                target = Some(lerp(m.new_s, m.new_e, along / len));
                            }
                            break;
                        }
                    }
                }
                if let Some(t) = target {
                    *end = t;
                    touched = true;
                }
            }
            if touched {
                follow.push(WallMove {
                    id: w.id.clone(),
                    start: ends[0],
                    end: ends[1],
                });
            }
        }
    }
    for m in moves {
        set_wall_geometry(project, &m.id, m.start, m.end);
    }
    for f in &follow {
        set_wall_geometry(project, &f.id, f.start, f.end);
    }
    follow.into_iter().map(|f| f.id).collect()
}

/// Joint rule for rigid translations. `w` has its end `p` on the joint
/// `old_j` of a moved wall (`m_s` to `m_e`); the joint moves to `new_j`.
#[allow(clippy::too_many_arguments)]
fn joint_should_follow(
    w: &Wall,
    p: Point,
    far: Point,
    m_s: Point,
    m_e: Point,
    old_j: Point,
    new_j: Point,
    others: &[Wall],
    moved_ids: &BTreeSet<&Id>,
) -> bool {
    let (Some(wd), Some(md)) = (unit(sub(p, far)), unit(sub(m_e, m_s))) else {
        return true;
    };
    let parallel = |a: Point, b: Point| cross(a, b).abs() < 0.0175;
    let unmoved: Vec<&Wall> = others
        .iter()
        .filter(|o| o.level_id == w.level_id && !moved_ids.contains(&o.id) && o.id != w.id)
        .collect();

    if parallel(wd, md) {
        // A straight continuation of the moved wall stays where it is when a
        // cross wall at the joint keeps the old joint on its line: that wall
        // either extends through the joint or stays with the joint on its body.
        return !unmoved.iter().any(|q| {
            let at_joint = dist(q.start, old_j) <= JOIN_EPS || dist(q.end, old_j) <= JOIN_EPS;
            let Some(qd) = unit(sub(q.end, q.start)) else {
                return false;
            };
            at_joint && !parallel(qd, md) && cross(qd, sub(new_j, q.start)).abs() <= JOIN_EPS
        });
    }

    // A cross wall. When the joint moves back along its body the wall would
    // get shorter. It only does that when nothing else is attached to the
    // part that would go away. Otherwise it stays, and the moved wall lands
    // on its body as a T junction.
    let len = dist(far, p);
    let (along, d) = project_on_segment(new_j, far, p);
    let shrinking = d <= JOIN_EPS && along < len;
    if !shrinking {
        return true;
    }
    !unmoved.iter().any(|o| {
        [o.start, o.end].iter().any(|e| {
            let (a, dd) = project_on_segment(*e, far, p);
            dd <= JOIN_EPS && a > along + JOIN_EPS
        })
    })
}

fn translate_element(el: &mut Element, d: Point) {
    match el {
        Element::Wall(_) | Element::Opening(_) => {}
        Element::Room(r) => r.seed = add(r.seed, d),
        Element::Column(c) => c.center = add(c.center, d),
        Element::Stair(s) => s.origin = add(s.origin, d),
        Element::Asset(a) => a.position = add(a.position, d),
        Element::Annotation(a) => a.position = add(a.position, d),
        Element::Dimension(dm) => {
            dm.a = add(dm.a, d);
            dm.b = add(dm.b, d);
        }
        Element::Camera(c) => {
            c.position.x += d.x;
            c.position.y += d.y;
            c.target.x += d.x;
            c.target.y += d.y;
        }
        Element::Underlay(u) => u.position = add(u.position, d),
        Element::Linework(l) => {
            for pl in &mut l.polylines {
                for p in pl.iter_mut() {
                    *p = add(*p, d);
                }
            }
        }
        Element::ReferenceModel(m) => m.position = add(m.position, d),
        Element::Pipe(p) => {
            for v in p.points.iter_mut() {
                v.x += d.x;
                v.y += d.y;
            }
        }
    }
}

fn norm_deg(a: f64) -> f64 {
    let r = a.rem_euclid(360.0);
    if r.is_finite() {
        r
    } else {
        a
    }
}

// ------------------------------------------------------------ leaf commands

fn leaf(project: &mut Project, command: &Command, ids: &mut IdGen) -> Result<Outcome, CoreError> {
    match command {
        Command::Batch { .. } => unreachable!("batches are handled by run"),

        Command::AddWall {
            start,
            end,
            thickness_mm,
            height_mm,
            material_id,
            level_id,
        } => {
            ensure_unlocked(project, ElementKind::Wall)?;
            let level = resolve_level(project, level_id)?;
            let mut wall = new_wall(project, ids, &level, *start, *end, *thickness_mm);
            wall.height_mm = *height_mm;
            if material_id.is_some() {
                wall.material_id = material_id.clone();
            }
            validate_wall(project, &wall)?;
            project.elements.push(Element::Wall(wall));
            Ok(outcome("Add wall"))
        }

        Command::AddWallChain {
            points,
            closed,
            thickness_mm,
            level_id,
        } => {
            ensure_unlocked(project, ElementKind::Wall)?;
            let level = resolve_level(project, level_id)?;
            if points.len() < 2 {
                return Err(CoreError::invalid(
                    "chain_too_short",
                    "A wall chain needs at least 2 points.",
                ));
            }
            if *closed && points.len() < 3 {
                return Err(CoreError::invalid(
                    "chain_too_short",
                    "A closed wall chain needs at least 3 points.",
                ));
            }
            let mut segs: Vec<(Point, Point)> = points.windows(2).map(|p| (p[0], p[1])).collect();
            if *closed {
                segs.push((points[points.len() - 1], points[0]));
            }
            let count = segs.len();
            for (a, b) in segs {
                let wall = new_wall(project, ids, &level, a, b, *thickness_mm);
                validate_wall(project, &wall)?;
                project.elements.push(Element::Wall(wall));
            }
            Ok(outcome(&format!("Add {}", plural(count, "wall"))))
        }

        Command::AddRectRoom {
            origin,
            width_mm,
            depth_mm,
            thickness_mm,
            level_id,
            ..
        } => {
            ensure_unlocked(project, ElementKind::Wall)?;
            let level = resolve_level(project, level_id)?;
            finite_delta("Room origin", *origin)?;
            for (name, v) in [("Room width", *width_mm), ("Room depth", *depth_mm)] {
                if !v.is_finite() {
                    return Err(CoreError::invalid(
                        "not_finite",
                        format!("{name} is not a valid number."),
                    ));
                }
                if v < 300.0 {
                    return Err(CoreError::invalid(
                        "room_too_small",
                        format!("{name} must be at least 300 mm, got {v:.0}."),
                    ));
                }
            }
            let (x0, y0, x1, y1) = (origin.x, origin.y, origin.x + width_mm, origin.y + depth_mm);
            let corners = [pt(x0, y0), pt(x1, y0), pt(x1, y1), pt(x0, y1)];
            let mut added = 0;
            for i in 0..4 {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                // A side that an existing wall already covers is shared, not doubled.
                if covered_by_existing(project, &level, a, b) {
                    continue;
                }
                let wall = new_wall(project, ids, &level, a, b, *thickness_mm);
                validate_wall(project, &wall)?;
                project.elements.push(Element::Wall(wall));
                added += 1;
            }
            if added == 0 {
                return Err(CoreError::invalid(
                    "room_exists",
                    "Walls already exist on all four sides of this rectangle.",
                ));
            }
            Ok(outcome("Add room"))
        }

        Command::SetWallEndpoints {
            wall_id,
            start,
            end,
        } => {
            find_wall(project, wall_id)?;
            finite_delta("Wall start", *start)?;
            finite_delta("Wall end", *end)?;
            let stretched = move_walls(
                project,
                &[WallMove {
                    id: wall_id.clone(),
                    start: *start,
                    end: *end,
                }],
                true,
                false,
            );
            Ok(Outcome {
                label: "Move wall ends".into(),
                summary: Some(move_summary(1, 0, stretched.len())),
            })
        }

        Command::SetWallLength {
            wall_id,
            length_mm,
            anchor,
        } => {
            let wall = find_wall(project, wall_id)?.clone();
            if !length_mm.is_finite() {
                return Err(CoreError::invalid_for(
                    "not_finite",
                    "Wall length is not a valid number.",
                    vec![wall_id.clone()],
                ));
            }
            if *length_mm < MIN_WALL_LENGTH_MM {
                return Err(CoreError::invalid_for(
                    "wall_too_short",
                    format!("A wall must be at least {MIN_WALL_LENGTH_MM:.0} mm long. This one would be {length_mm:.0} mm."),
                    vec![wall_id.clone()],
                ));
            }
            let Some(d) = unit(sub(wall.end, wall.start)) else {
                return Err(CoreError::invalid_for(
                    "wall_too_short",
                    "This wall has no direction, so its length cannot be set.",
                    vec![wall_id.clone()],
                ));
            };
            let old_len = dist(wall.start, wall.end);
            let grow = length_mm - old_len;
            let (new_start, new_end) = match anchor {
                WallAnchor::Start => (wall.start, add(wall.start, scale(d, *length_mm))),
                WallAnchor::End => (sub(wall.end, scale(d, *length_mm)), wall.end),
                WallAnchor::Center => (
                    sub(wall.start, scale(d, grow / 2.0)),
                    add(wall.end, scale(d, grow / 2.0)),
                ),
            };
            // Cross walls at a moved end shift with it and stay straight;
            // whatever is joined to them stretches.
            let mut moves = vec![WallMove {
                id: wall_id.clone(),
                start: new_start,
                end: new_end,
            }];
            for (old_j, new_j) in [(wall.start, new_start), (wall.end, new_end)] {
                let shift = sub(new_j, old_j);
                if length(shift) < 1e-9 {
                    continue;
                }
                for el in &project.elements {
                    let Element::Wall(o) = el else { continue };
                    if o.id == wall.id
                        || o.level_id != wall.level_id
                        || moves.iter().any(|m| m.id == o.id)
                    {
                        continue;
                    }
                    let joined = dist(o.start, old_j) <= JOIN_EPS || dist(o.end, old_j) <= JOIN_EPS;
                    let crosses = unit(sub(o.end, o.start))
                        .map(|od| cross(od, d).abs() >= 0.0175)
                        .unwrap_or(false);
                    if joined && crosses {
                        moves.push(WallMove {
                            id: o.id.clone(),
                            start: add(o.start, shift),
                            end: add(o.end, shift),
                        });
                    }
                }
            }
            let shifted = moves.len() - 1;
            let stretched = move_walls(project, &moves, true, true);
            Ok(Outcome {
                label: "Set wall length".into(),
                summary: Some(format!(
                    "Set wall length to {:.0} mm{}",
                    length_mm,
                    match (shifted, stretched.len()) {
                        (0, 0) => String::new(),
                        (s, 0) => format!(", moved {}", plural(s, "joined wall")),
                        (0, t) => format!(", stretched {}", plural(t, "wall")),
                        (s, t) => format!(
                            ", moved {} and stretched {}",
                            plural(s, "joined wall"),
                            plural(t, "wall")
                        ),
                    }
                )),
            })
        }

        Command::SplitWall { wall_id, at_mm } => {
            let wall = find_wall(project, wall_id)?.clone();
            let len = dist(wall.start, wall.end);
            if !at_mm.is_finite()
                || *at_mm < MIN_WALL_LENGTH_MM
                || *at_mm > len - MIN_WALL_LENGTH_MM
            {
                return Err(CoreError::invalid_for(
                    "bad_split",
                    format!(
                        "The split point must be between {:.0} and {:.0} mm from the wall start.",
                        MIN_WALL_LENGTH_MM,
                        (len - MIN_WALL_LENGTH_MM).max(MIN_WALL_LENGTH_MM)
                    ),
                    vec![wall_id.clone()],
                ));
            }
            for el in &project.elements {
                if let Element::Opening(o) = el {
                    if &o.wall_id == wall_id {
                        let (lo, hi) = (
                            o.offset_mm - o.width_mm / 2.0,
                            o.offset_mm + o.width_mm / 2.0,
                        );
                        if *at_mm > lo - OPENING_END_CLEARANCE_MM
                            && *at_mm < hi + OPENING_END_CLEARANCE_MM
                        {
                            return Err(CoreError::invalid_for(
                                "split_hits_opening",
                                "The split point is inside or too close to an opening. Move the split or the opening.",
                                vec![o.id.clone(), wall_id.clone()],
                            ));
                        }
                    }
                }
            }
            let mid = lerp(wall.start, wall.end, at_mm / len);
            let second = Wall {
                id: ids.next_id(),
                start: mid,
                ..wall.clone()
            };
            let second_id = second.id.clone();
            let mut pos = 0;
            for (i, el) in project.elements.iter_mut().enumerate() {
                match el {
                    Element::Wall(w) if &w.id == wall_id => {
                        w.end = mid;
                        pos = i + 1;
                    }
                    Element::Opening(o) if &o.wall_id == wall_id && o.offset_mm > *at_mm => {
                        o.wall_id = second_id.clone();
                        o.offset_mm -= at_mm;
                    }
                    _ => {}
                }
            }
            project.elements.insert(pos, Element::Wall(second));
            Ok(outcome("Split wall"))
        }

        Command::AddOpening {
            wall_id,
            opening_type,
            offset_mm,
            width_mm,
            height_mm,
            sill_mm,
            style,
            flip_side,
            flip_hinge,
        } => {
            ensure_unlocked(project, ElementKind::Opening)?;
            find_wall(project, wall_id)?;
            let (dw, dh, ds) = match opening_type {
                OpeningType::Door => defaults::DOOR_DEFAULT,
                OpeningType::Window => defaults::WINDOW_DEFAULT,
            };
            let opening = Opening {
                id: ids.next_id(),
                wall_id: wall_id.clone(),
                opening_type: *opening_type,
                style: style.unwrap_or(match opening_type {
                    OpeningType::Door => OpeningStyle::SwingSingle,
                    OpeningType::Window => OpeningStyle::Sliding,
                }),
                offset_mm: *offset_mm,
                width_mm: width_mm.unwrap_or(dw),
                height_mm: height_mm.unwrap_or(dh),
                sill_mm: sill_mm.unwrap_or(ds),
                // None means false. See `Opening::flip_side` for the convention.
                flip_side: flip_side.unwrap_or(false),
                flip_hinge: flip_hinge.unwrap_or(false),
                material_id: None,
            };
            project.elements.push(Element::Opening(opening));
            Ok(outcome(match opening_type {
                OpeningType::Door => "Add door",
                OpeningType::Window => "Add window",
            }))
        }

        Command::ResizeRoom {
            room_id,
            side,
            delta_mm,
        } => {
            let room = match project.elements.iter().find(|e| e.id() == room_id) {
                Some(Element::Room(r)) => r.clone(),
                Some(_) => {
                    return Err(CoreError::invalid_for(
                        "not_a_room",
                        format!("Element {room_id} is not a room."),
                        vec![room_id.clone()],
                    ))
                }
                None => return Err(CoreError::NotFound(room_id.clone())),
            };
            if !delta_mm.is_finite() || delta_mm.abs() < 1e-9 {
                return Err(CoreError::invalid_for(
                    "bad_delta",
                    "The resize distance must be a number other than zero.",
                    vec![room_id.clone()],
                ));
            }
            let analysis = analyze(project);
            let assigned = assign_by_seed(project, &analysis);
            let Some(fref) = assigned.get(room_id) else {
                return Err(CoreError::invalid_for(
                    "room_not_enclosed",
                    format!(
                        "{} is not enclosed by walls, so it cannot be resized.",
                        room.name
                    ),
                    vec![room_id.clone()],
                ));
            };
            let (side_vec, side_name) = match side {
                Side::North => (pt(0.0, 1.0), "north"),
                Side::South => (pt(0.0, -1.0), "south"),
                Side::East => (pt(1.0, 0.0), "east"),
                Side::West => (pt(-1.0, 0.0), "west"),
            };
            let mut wall_ids: Vec<Id> = vec![];
            for edge in &face(&analysis, *fref).boundary {
                let Some(d) = unit(sub(edge.to, edge.from)) else {
                    continue;
                };
                // The face is counter-clockwise, so outward is to the right.
                let outward = pt(d.y, -d.x);
                if dot(outward, side_vec) > 0.7072 {
                    for id in &edge.wall_ids {
                        if !wall_ids.contains(id) {
                            wall_ids.push(id.clone());
                        }
                    }
                }
            }
            if wall_ids.is_empty() {
                return Err(CoreError::invalid_for(
                    "no_wall_on_side",
                    format!("{} has no wall facing {side_name}.", room.name),
                    vec![room_id.clone()],
                ));
            }
            let shift = scale(side_vec, *delta_mm);
            let moves: Vec<WallMove> = project
                .elements
                .iter()
                .filter_map(|e| match e {
                    Element::Wall(w) if wall_ids.contains(&w.id) => Some(WallMove {
                        id: w.id.clone(),
                        start: add(w.start, shift),
                        end: add(w.end, shift),
                    }),
                    _ => None,
                })
                .collect();
            let stretched = move_walls(project, &moves, true, true);
            Ok(Outcome {
                label: format!("Resize {}", room.name),
                summary: Some(format!(
                    "Moved the {side_name} side of {} by {:.0} mm ({}), stretched {}",
                    room.name,
                    delta_mm,
                    plural(moves.len(), "wall"),
                    plural(stretched.len(), "wall")
                )),
            })
        }

        Command::AddElement { element } => {
            let mut element = element.clone();
            if let Element::Asset(a) = &mut element {
                a.circuit = a.circuit.trim().to_string();
            }
            ensure_element_unlocked(project, &element)?;
            if element.id().is_empty() {
                *element.id_mut() = ids.next_id();
            } else if project.elements.iter().any(|e| e.id() == element.id()) {
                return Err(CoreError::invalid_for(
                    "duplicate_id",
                    format!("An element with id {} already exists.", element.id()),
                    vec![element.id().clone()],
                ));
            } else {
                ids.reserve(element.id());
            }
            if let Element::Room(r) = &mut element {
                if r.name.trim().is_empty() {
                    r.name = next_room_name(project);
                    r.auto_named = true;
                }
                require_level(project, &r.level_id)?;
                finite_delta("Room seed", r.seed)?;
                let analysis = analyze(project);
                let Some(fi) = analysis.face_at(&r.level_id, r.seed) else {
                    return Err(CoreError::invalid(
                        "room_not_enclosed",
                        "That point is not inside a closed loop of walls, so there is no room there.",
                    ));
                };
                let li = analysis
                    .levels
                    .iter()
                    .position(|l| l.level_id == r.level_id)
                    .unwrap_or(0);
                if let Some((other, _)) = assign_by_seed(project, &analysis)
                    .into_iter()
                    .find(|(_, f)| *f == (li, fi))
                {
                    return Err(CoreError::invalid_for(
                        "room_exists",
                        "That space already has a room. Edit the existing room instead.",
                        vec![other],
                    ));
                }
            }
            validate_element(project, &element)?;
            let label = format!("Add {}", element_noun(&element));
            project.elements.push(element);
            Ok(outcome(&label))
        }

        Command::UpdateElement { element } => {
            let slot = project
                .elements
                .iter_mut()
                .find(|e| e.id() == element.id())
                .ok_or_else(|| CoreError::NotFound(element.id().clone()))?;
            if slot.kind() != element.kind() {
                return Err(CoreError::invalid_for(
                    "kind_mismatch",
                    format!(
                        "Element {} is a {} and cannot become a {}.",
                        element.id(),
                        kind_noun(slot.kind()),
                        kind_noun(element.kind())
                    ),
                    vec![element.id().clone()],
                ));
            }
            let mut element = element.clone();
            if let (Element::Room(new), Element::Room(old)) = (&mut element, &*slot) {
                new.name = new.name.trim().to_string();
                if new.name != old.name {
                    new.auto_named = false;
                }
            }
            if let Element::Asset(a) = &mut element {
                a.circuit = a.circuit.trim().to_string();
            }
            let label = format!("Edit {}", kind_noun(element.kind()));
            *slot = element;
            Ok(outcome(&label))
        }

        Command::DeleteElements { ids: del } => {
            let del = require_ids(project, del)?;
            project.elements.retain(|e| match e {
                // Openings go with their host wall.
                Element::Opening(o) => !del.contains(&o.id) && !del.contains(&o.wall_id),
                other => !del.contains(other.id()),
            });
            Ok(outcome("Delete"))
        }

        Command::MoveElements {
            ids: sel,
            delta,
            stretch_connected,
        } => {
            let sel = require_ids(project, sel)?;
            finite_delta("Move distance", *delta)?;
            let mut moves: Vec<WallMove> = vec![];
            let mut moved_other = 0;
            let snapshot = project.elements.clone();
            for el in &snapshot {
                if !sel.contains(el.id()) {
                    continue;
                }
                match el {
                    Element::Wall(w) => moves.push(WallMove {
                        id: w.id.clone(),
                        start: add(w.start, *delta),
                        end: add(w.end, *delta),
                    }),
                    Element::Opening(o) => {
                        // An opening slides along its host, unless the host moves too.
                        if sel.contains(&o.wall_id) {
                            continue;
                        }
                        let host = find_wall(project, &o.wall_id)?;
                        let along = unit(sub(host.end, host.start))
                            .map(|d| dot(*delta, d))
                            .unwrap_or(0.0);
                        for target in project.elements.iter_mut() {
                            if let Element::Opening(t) = target {
                                if t.id == o.id {
                                    t.offset_mm += along;
                                }
                            }
                        }
                        moved_other += 1;
                    }
                    _ => {
                        for target in project.elements.iter_mut() {
                            if target.id() == el.id() {
                                translate_element(target, *delta);
                            }
                        }
                        moved_other += 1;
                    }
                }
            }
            let wall_count = moves.len();
            let stretched = move_walls(project, &moves, *stretch_connected, true);
            Ok(Outcome {
                label: "Move".into(),
                summary: Some(move_summary(wall_count, moved_other, stretched.len())),
            })
        }

        Command::RotateElements {
            ids: sel,
            pivot,
            angle_deg,
        } => {
            let sel = require_ids(project, sel)?;
            finite_delta("Rotation pivot", *pivot)?;
            if !angle_deg.is_finite() {
                return Err(CoreError::invalid(
                    "not_finite",
                    "Rotation angle is not a valid number.",
                ));
            }
            let rot = |p: Point| rotate_about(p, *pivot, *angle_deg);
            let mut rotated = 0;
            for el in project.elements.iter_mut() {
                if !sel.contains(el.id()) {
                    continue;
                }
                rotated += 1;
                match el {
                    Element::Wall(w) => {
                        w.start = rot(w.start);
                        w.end = rot(w.end);
                    }
                    // Openings turn with their host wall.
                    Element::Opening(_) => rotated -= 1,
                    Element::Room(r) => r.seed = rot(r.seed),
                    Element::Column(c) => {
                        c.center = rot(c.center);
                        c.rotation_deg = norm_deg(c.rotation_deg + angle_deg);
                    }
                    Element::Stair(s) => {
                        s.origin = rot(s.origin);
                        s.rotation_deg = norm_deg(s.rotation_deg + angle_deg);
                    }
                    Element::Asset(a) => {
                        a.position = rot(a.position);
                        a.rotation_deg = norm_deg(a.rotation_deg + angle_deg);
                    }
                    Element::Annotation(a) => {
                        a.position = rot(a.position);
                        a.rotation_deg = norm_deg(a.rotation_deg + angle_deg);
                    }
                    Element::Dimension(d) => {
                        d.a = rot(d.a);
                        d.b = rot(d.b);
                    }
                    Element::Camera(c) => {
                        let p = rot(pt(c.position.x, c.position.y));
                        let t = rot(pt(c.target.x, c.target.y));
                        c.position.x = p.x;
                        c.position.y = p.y;
                        c.target.x = t.x;
                        c.target.y = t.y;
                    }
                    Element::Underlay(u) => {
                        u.position = rot(u.position);
                        u.rotation_deg = norm_deg(u.rotation_deg + angle_deg);
                    }
                    Element::Linework(l) => {
                        for pl in &mut l.polylines {
                            for p in pl.iter_mut() {
                                *p = rot(*p);
                            }
                        }
                    }
                    Element::ReferenceModel(m) => {
                        m.position = rot(m.position);
                        m.rotation_deg = norm_deg(m.rotation_deg + angle_deg);
                    }
                    Element::Pipe(p) => {
                        for v in p.points.iter_mut() {
                            let r = rot(pt(v.x, v.y));
                            v.x = r.x;
                            v.y = r.y;
                        }
                    }
                }
            }
            if rotated == 0 {
                return Err(CoreError::invalid_for(
                    "nothing_to_rotate",
                    "Doors and windows turn with their wall. Select the wall to rotate them.",
                    sel,
                ));
            }
            Ok(outcome("Rotate"))
        }

        Command::DuplicateElements { ids: sel, delta } => {
            let sel = require_ids(project, sel)?;
            finite_delta("Copy offset", *delta)?;
            let snapshot = project.elements.clone();
            let mut copies: Vec<Element> = vec![];
            // Original id to copy id, for the links between copied objects.
            let mut copied: BTreeMap<Id, Id> = BTreeMap::new();
            let mut count = 0;
            for el in &snapshot {
                if !sel.contains(el.id()) {
                    continue;
                }
                ensure_element_unlocked(project, el)?;
                match el {
                    Element::Wall(w) => {
                        let mut copy = w.clone();
                        copy.id = ids.next_id();
                        copy.start = add(w.start, *delta);
                        copy.end = add(w.end, *delta);
                        // A wall brings its doors and windows.
                        for hosted in &snapshot {
                            if let Element::Opening(o) = hosted {
                                if o.wall_id == w.id {
                                    let mut oc = o.clone();
                                    oc.id = ids.next_id();
                                    oc.wall_id = copy.id.clone();
                                    copied.insert(o.id.clone(), oc.id.clone());
                                    copies.push(Element::Opening(oc));
                                }
                            }
                        }
                        copied.insert(w.id.clone(), copy.id.clone());
                        copies.push(Element::Wall(copy));
                        count += 1;
                    }
                    Element::Opening(o) => {
                        if sel.contains(&o.wall_id) {
                            continue;
                        }
                        let host = find_wall(project, &o.wall_id)?;
                        let along = unit(sub(host.end, host.start))
                            .map(|d| dot(*delta, d))
                            .unwrap_or(0.0);
                        let mut oc = o.clone();
                        oc.id = ids.next_id();
                        oc.offset_mm += along;
                        copied.insert(o.id.clone(), oc.id.clone());
                        copies.push(Element::Opening(oc));
                        count += 1;
                    }
                    Element::Room(r) => {
                        // The copy only survives when its seed lands in a free closed face.
                        let mut rc = r.clone();
                        rc.id = ids.next_id();
                        rc.seed = add(r.seed, *delta);
                        if rc.auto_named {
                            // Named after the copies are in, see below.
                            rc.name = String::new();
                        }
                        copied.insert(r.id.clone(), rc.id.clone());
                        copies.push(Element::Room(rc));
                        count += 1;
                    }
                    other => {
                        let mut c = other.clone();
                        *c.id_mut() = ids.next_id();
                        translate_element(&mut c, *delta);
                        if let Element::Camera(cam) = &mut c {
                            cam.name = format!("{} copy", cam.name);
                        }
                        copied.insert(other.id().clone(), c.id().clone());
                        copies.push(c);
                        count += 1;
                    }
                }
            }
            // A copied switch controls the copy of a light copied with it,
            // and keeps its links to everything that was not copied.
            for c in copies.iter_mut() {
                if let Element::Asset(a) = c {
                    for link in a.links.iter_mut() {
                        if let Some(new) = copied.get(link) {
                            *link = new.clone();
                        }
                    }
                }
            }
            // Walls first, so hosted openings always find their host.
            copies.sort_by_key(|e| match e {
                Element::Wall(_) => 0,
                Element::Opening(_) => 1,
                _ => 2,
            });
            project.elements.extend(copies);
            while let Some(i) = project
                .elements
                .iter()
                .position(|e| matches!(e, Element::Room(r) if r.name.is_empty()))
            {
                let name = next_room_name(project);
                if let Element::Room(r) = &mut project.elements[i] {
                    r.name = name;
                }
            }
            Ok(outcome(&format!("Duplicate {}", plural(count, "element"))))
        }

        Command::SetMaterial {
            ids: sel,
            material_id,
        } => {
            let sel = require_ids(project, sel)?;
            require_material(project, &Some(material_id.clone()))?;
            let mut refused: Vec<Id> = vec![];
            for el in project.elements.iter_mut() {
                if !sel.contains(el.id()) {
                    continue;
                }
                match el {
                    Element::Wall(w) => w.material_id = Some(material_id.clone()),
                    Element::Column(c) => c.material_id = Some(material_id.clone()),
                    Element::Opening(o) => o.material_id = Some(material_id.clone()),
                    Element::Room(r) => r.floor_material_id = Some(material_id.clone()),
                    other => refused.push(other.id().clone()),
                }
            }
            if !refused.is_empty() {
                return Err(CoreError::invalid_for(
                    "no_material_slot",
                    "Only walls, columns, doors, windows and room floors take a material.",
                    refused,
                ));
            }
            Ok(outcome("Set material"))
        }

        Command::UpsertMaterial { material } => {
            let mut m = material.clone();
            m.name = m.name.trim().to_string();
            if m.name.is_empty() {
                return Err(CoreError::invalid("bad_name", "A material needs a name."));
            }
            let hex = m.color.strip_prefix('#').unwrap_or("");
            if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(CoreError::invalid(
                    "bad_color",
                    format!(
                        "Material color must look like #rrggbb, got \"{}\".",
                        m.color
                    ),
                ));
            }
            for (name, v) in [
                ("Roughness", m.roughness),
                ("Metalness", m.metalness),
                ("Opacity", m.opacity),
            ] {
                if !v.is_finite() || !(0.0..=1.0).contains(&v) {
                    return Err(CoreError::invalid(
                        "bad_material_value",
                        format!("{name} must be between 0 and 1."),
                    ));
                }
            }
            if m.id.is_empty() {
                m.id = ids.next_id();
            }
            match project.materials.iter_mut().find(|x| x.id == m.id) {
                Some(slot) => {
                    // The builtin flag belongs to the library, not to the caller.
                    m.builtin = slot.builtin;
                    *slot = m;
                    Ok(outcome("Edit material"))
                }
                None => {
                    m.builtin = false;
                    ids.reserve(&m.id);
                    project.materials.push(m);
                    Ok(outcome("Add material"))
                }
            }
        }

        Command::SetRoof { roof } => {
            require_material(project, &roof.material_id)?;
            for (name, v, lo, hi) in [
                ("Roof pitch (degrees)", roof.pitch_deg, 0.0, 75.0),
                ("Roof overhang (mm)", roof.overhang_mm, 0.0, 5000.0),
                ("Roof thickness (mm)", roof.thickness_mm, 10.0, 1000.0),
            ] {
                if !v.is_finite() || v < lo || v > hi {
                    return Err(CoreError::invalid(
                        "bad_roof",
                        format!("{name} must be between {lo} and {hi}."),
                    ));
                }
            }
            project.roof = roof.clone();
            Ok(outcome("Edit roof"))
        }

        Command::SetProjectSettings { settings } => {
            let s = settings;
            if s.scale_denominator == 0 {
                return Err(CoreError::invalid(
                    "bad_settings",
                    "The drawing scale must be 1:1 or smaller.",
                ));
            }
            if !s.north_angle_deg.is_finite() {
                return Err(CoreError::invalid(
                    "bad_settings",
                    "The north angle is not a valid number.",
                ));
            }
            if !s.grid_mm.is_finite() || s.grid_mm <= 0.0 {
                return Err(CoreError::invalid(
                    "bad_settings",
                    "The grid size must be more than 0 mm.",
                ));
            }
            if !s.default_wall_thickness_mm.is_finite()
                || s.default_wall_thickness_mm < MIN_WALL_THICKNESS_MM
                || s.default_wall_thickness_mm > MAX_WALL_THICKNESS_MM
            {
                return Err(CoreError::invalid(
                    "wall_thickness",
                    format!(
                        "The default wall thickness must be between {MIN_WALL_THICKNESS_MM} and {MAX_WALL_THICKNESS_MM} mm."
                    ),
                ));
            }
            project.settings = settings.clone();
            Ok(outcome("Edit project settings"))
        }

        Command::UpdateLevel { level } => {
            if level.name.trim().is_empty() {
                return Err(CoreError::invalid("bad_name", "A level needs a name."));
            }
            if !level.elevation_mm.is_finite() || level.elevation_mm.abs() > 1.0e6 {
                return Err(CoreError::invalid(
                    "bad_level",
                    "The level elevation is not a valid number.",
                ));
            }
            if !level.height_mm.is_finite()
                || level.height_mm < 1000.0
                || level.height_mm > 20_000.0
            {
                return Err(CoreError::invalid(
                    "bad_level",
                    "The level height must be between 1000 and 20000 mm.",
                ));
            }
            let Some(slot) = project.levels.iter_mut().find(|l| l.id == level.id) else {
                return Err(CoreError::invalid(
                    "unknown_level",
                    format!("There is no level with id {}.", level.id),
                ));
            };
            *slot = level.clone();
            // Doors and windows must still fit under walls that use the level height.
            let walls: Vec<Id> = project
                .elements
                .iter()
                .filter_map(|e| match e {
                    Element::Wall(w) if w.level_id == level.id => Some(w.id.clone()),
                    _ => None,
                })
                .collect();
            // Nothing is moving: the level height changed, not an opening.
            let moving = BTreeSet::new();
            for id in walls {
                check_hosted_openings(project, &id, &moving)?;
            }
            Ok(outcome("Edit level"))
        }

        Command::AddLevel {
            name,
            elevation_mm,
            height_mm,
        } => {
            // Default: on top of the highest level, "Level N", default height.
            let highest = project
                .levels
                .iter()
                .filter(|l| l.elevation_mm.is_finite() && l.height_mm.is_finite())
                .fold(None::<&Level>, |best, l| match best {
                    Some(b) if b.elevation_mm >= l.elevation_mm => Some(b),
                    _ => Some(l),
                });
            let elevation = elevation_mm
                .unwrap_or_else(|| highest.map(|l| l.elevation_mm + l.height_mm).unwrap_or(0.0));
            let height = height_mm.unwrap_or(defaults::DEFAULT_LEVEL_HEIGHT_MM);
            let name = match name {
                Some(n) => n.clone(),
                None => next_level_name(project),
            };
            let name = validate_new_level(project, &name, elevation, height)?;
            let level = Level {
                id: ids.next_id(),
                name: name.clone(),
                elevation_mm: elevation,
                height_mm: height,
            };
            // Levels stay in elevation order when they are in order already.
            let at = project
                .levels
                .iter()
                .position(|l| l.elevation_mm > elevation)
                .unwrap_or(project.levels.len());
            project.levels.insert(at, level);
            Ok(Outcome {
                label: "Add level".into(),
                summary: Some(format!(
                    "Added {name}, floor at {elevation:.0} mm, {height:.0} mm floor to floor"
                )),
            })
        }

        Command::DeleteLevel { level_id } => {
            require_level(project, level_id)?;
            if project.levels.len() <= 1 {
                return Err(CoreError::invalid(
                    "last_level",
                    "A project needs at least one level, so its last level cannot be deleted.",
                ));
            }
            let name = project
                .levels
                .iter()
                .find(|l| &l.id == level_id)
                .map(|l| l.name.clone())
                .unwrap_or_default();
            let walls: BTreeSet<Id> = project
                .elements
                .iter()
                .filter_map(|e| match e {
                    Element::Wall(w) if &w.level_id == level_id => Some(w.id.clone()),
                    _ => None,
                })
                .collect();
            // Everything on the level, and the doors and windows of its walls.
            project.elements.retain(|e| match e {
                Element::Opening(o) => !walls.contains(&o.wall_id),
                other => element_level(other) != Some(level_id),
            });
            project.levels.retain(|l| &l.id != level_id);
            Ok(outcome(&format!("Delete level {}", name.trim())))
        }

        Command::SetLayer { layer } => {
            match project.layers.iter_mut().find(|l| l.key == layer.key) {
                Some(slot) => *slot = layer.clone(),
                None => project.layers.push(layer.clone()),
            }
            Ok(outcome("Edit layer"))
        }

        Command::SetReviewMark { target, note } => {
            // A mark names a known check; one per target, replaced in place.
            let target = clean_review_target(target)?;
            let at = project
                .review
                .iter()
                .position(|m| trimmed_target(&m.target) == target);
            match (note, at) {
                (Some(note), Some(i)) => {
                    project.review[i] = ReviewMark {
                        target,
                        note: note.trim().to_string(),
                    };
                    Ok(outcome("Edit a review note"))
                }
                (Some(note), None) => {
                    project.review.push(ReviewMark {
                        target,
                        note: note.trim().to_string(),
                    });
                    Ok(outcome("Set a review item aside"))
                }
                (None, Some(i)) => {
                    project.review.remove(i);
                    Ok(outcome("Reopen a review item"))
                }
                (None, None) => Err(CoreError::invalid(
                    "no_review_mark",
                    "That review item is not set aside, so there is nothing to reopen.",
                )),
            }
        }
    }
}

fn move_summary(walls: usize, others: usize, stretched: usize) -> String {
    let mut moved = vec![];
    if walls > 0 {
        moved.push(plural(walls, "wall"));
    }
    if others > 0 {
        moved.push(plural(others, "other element"));
    }
    let mut s = format!(
        "Moved {}",
        if moved.is_empty() {
            "nothing".to_string()
        } else {
            moved.join(" and ")
        }
    );
    if stretched > 0 {
        s.push_str(&format!(", stretched {}", plural(stretched, "wall")));
    }
    s
}

/// Noun for one element. Openings say what they are, so the undo label of a
/// door reads "Add door" whichever command made it; devices say what they
/// are too ("Add switch").
fn element_noun(el: &Element) -> &'static str {
    match el {
        Element::Opening(o) => match o.opening_type {
            OpeningType::Door => "door",
            OpeningType::Window => "window",
        },
        other => diff_noun(other),
    }
}

/// Noun for one element in a diff summary: "light", "switch", "outlet" and
/// "aircon unit" for devices, the kind noun for everything else.
pub fn diff_noun(el: &Element) -> &'static str {
    match el {
        Element::Asset(a) => match a.catalog_key.as_str() {
            k if k.starts_with("light-") => "light",
            k if k.starts_with("switch-") => "switch",
            k if k.starts_with("outlet-") => "outlet",
            k if k.starts_with("aircon-") => "aircon unit",
            "panelboard" => "panelboard",
            "smoke-detector" => "smoke detector",
            "doorbell-button" => "doorbell button",
            "doorbell-chime" => "doorbell chime",
            _ => "object",
        },
        other => kind_noun(other.kind()),
    }
}

/// "1 switch", "2 switches", "3 lights".
pub fn count_noun(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else if ["ch", "sh", "s", "x"].iter().any(|end| noun.ends_with(end)) {
        format!("{n} {noun}es")
    } else {
        format!("{n} {noun}s")
    }
}

pub fn kind_noun(kind: ElementKind) -> &'static str {
    match kind {
        ElementKind::Wall => "wall",
        ElementKind::Opening => "opening",
        ElementKind::Room => "room",
        ElementKind::Column => "column",
        ElementKind::Stair => "stair",
        ElementKind::Asset => "object",
        ElementKind::Annotation => "note",
        ElementKind::Dimension => "dimension",
        ElementKind::Camera => "camera",
        ElementKind::Underlay => "underlay",
        ElementKind::Linework => "linework",
        ElementKind::ReferenceModel => "reference model",
        ElementKind::Pipe => "pipe",
    }
}
