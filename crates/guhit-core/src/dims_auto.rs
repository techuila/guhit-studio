//! Dimension strings and automatic dimensions (D12 as amended by D54).
//! LANE L1 owns this module.
//!
//! An automatic dimension (`Dimension::auto`) stores what it measures, not
//! where: the engine rewrites its a, b and `through` after every command,
//! in the same step (P1-2). `exec::run` calls `after_command` after
//! `post_validate`, like `dimensions::follow_walls`, which skips automatic
//! dimensions.
//!
//! What each `DimRef` measures (a to b always runs so that a positive
//! `offset_mm` puts the line on the measured side):
//! - `Openings { wall_ids, face }`: a run of collinear walls, along the
//!   first wall's start to end. The ends of each wall, the jambs of every
//!   opening on them, and where other walls meet the run on the measured
//!   side, on the chosen face. Exterior and interior faces are the face on
//!   the side of the dimension line (the sign of `offset_mm`).
//! - `Exterior { side, tier }`: one side (plan north is +y) of the buildings
//!   on the dimension's level, along the outermost face line of that side.
//!   `Overall` end to end; `Walls` the outline corners, the centerlines of
//!   the walls that meet that side's outer walls, and the grid lines across
//!   it; `Openings` the jambs of the openings in that side's outer walls,
//!   with the corners of those walls.
//! - `Grids { grid_ids }`: the grid lines, across them, through the start
//!   of the first one.
//!
//! An automatic dimension that a command moved by hand (its a, b or
//! `through` changed to anything but what it measures) stops being
//! automatic and stays where it was put. One whose walls or grids are all
//! gone, or whose level has no building left, is deleted.

use std::collections::BTreeMap;

use guhit_model::*;

use crate::error::CoreError;
use crate::exec::{count_noun, outcome, Outcome};
use crate::geom::add as plus;
use crate::geom::*;
use crate::ids::IdGen;
use crate::topo::{analyze, Analysis};
use crate::validate::{ensure_unlocked, finite_point, require_level};

/// Points closer than this along a string are one point, mm.
const MERGE_MM: f64 = 1.0;
/// Within this a hand edit is no edit: it is what the string measures.
const SAME_MM: f64 = 0.5;
/// Exterior tiers' distance from the outermost face, mm: openings nearest.
pub const TIER_OFFSET_MM: [(DimTier, f64); 3] = [
    (DimTier::Openings, 800.0),
    (DimTier::Walls, 1600.0),
    (DimTier::Overall, 2400.0),
];
/// Inside strings' distance from the wall face into the room, mm.
pub const ROOM_OFFSET_MM: f64 = 600.0;

/// a, `through...`, b.
#[derive(Debug, Clone, PartialEq)]
pub struct Measured {
    pub a: Point,
    pub b: Point,
    pub through: Vec<Point>,
}

impl Measured {
    fn close_to(&self, d: &Dimension) -> bool {
        dist(self.a, d.a) <= SAME_MM
            && dist(self.b, d.b) <= SAME_MM
            && self.through.len() == d.through.len()
            && self
                .through
                .iter()
                .zip(&d.through)
                .all(|(p, q)| dist(*p, *q) <= SAME_MM)
    }
}

/// After every leaf command (exec hub). Returns how many automatic
/// dimensions changed or went.
pub fn after_command(before: &Project, project: &mut Project) -> usize {
    if !project
        .elements
        .iter()
        .any(|e| matches!(e, Element::Dimension(d) if d.auto.is_some()))
    {
        return 0;
    }
    prune(project) + settle(before, project)
}

/// Drop references to walls and grids that are gone; delete an automatic
/// dimension left with nothing to measure.
fn prune(project: &mut Project) -> usize {
    let walls: Vec<Id> = project
        .elements
        .iter()
        .filter(|e| matches!(e, Element::Wall(_)))
        .map(|e| e.id().clone())
        .collect();
    let grids: Vec<Id> = project
        .elements
        .iter()
        .filter(|e| matches!(e, Element::Grid(_)))
        .map(|e| e.id().clone())
        .collect();
    let mut n = 0;
    let mut empty: Vec<Id> = vec![];
    for el in project.elements.iter_mut() {
        let Element::Dimension(d) = el else { continue };
        let Some(auto) = &mut d.auto else { continue };
        let (refs, known) = match auto {
            DimRef::Openings { wall_ids, .. } => (wall_ids, &walls),
            DimRef::Grids { grid_ids } => (grid_ids, &grids),
            DimRef::Exterior { .. } => continue,
        };
        let had = refs.len();
        refs.retain(|id| known.contains(id));
        if refs.len() != had {
            n += 1;
        }
        if refs.is_empty() {
            empty.push(d.id.clone());
        }
    }
    project.elements.retain(|e| !empty.contains(e.id()));
    n + empty.len()
}

/// Rewrite every automatic dimension, or let go of the ones moved by hand.
fn settle(before: &Project, project: &mut Project) -> usize {
    let old: BTreeMap<&Id, &Dimension> = before
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Dimension(d) => Some((&d.id, d)),
            _ => None,
        })
        .collect();
    let analysis = analyze(project);
    let mut plans: Vec<(usize, Option<Measured>, bool)> = vec![];
    for (i, el) in project.elements.iter().enumerate() {
        let Element::Dimension(d) = el else { continue };
        let Some(auto) = &d.auto else { continue };
        let m = measure(project, &analysis, d, auto);
        // Edited by hand: the command changed where it is, and not to what
        // it measures. A dimension moved with what it measures keeps on.
        let edited = old
            .get(&d.id)
            .map(|was| {
                was.auto.as_ref() == Some(auto)
                    && (was.a != d.a || was.b != d.b || was.through != d.through)
            })
            .unwrap_or(false)
            && !m.as_ref().map(|m| m.close_to(d)).unwrap_or(false);
        plans.push((i, m, edited));
    }
    let mut n = 0;
    let mut gone: Vec<Id> = vec![];
    for (i, m, edited) in plans {
        let Element::Dimension(d) = &mut project.elements[i] else {
            continue;
        };
        if edited {
            d.auto = None;
            n += 1;
            continue;
        }
        match m {
            Some(m) => {
                if m.a != d.a || m.b != d.b || m.through != d.through {
                    d.a = m.a;
                    d.b = m.b;
                    d.through = m.through;
                    n += 1;
                }
            }
            // Nothing left to measure: an exterior string with no building.
            None if matches!(d.auto, Some(DimRef::Exterior { .. })) => gone.push(d.id.clone()),
            None => {}
        }
    }
    if !gone.is_empty() {
        n += gone.len();
        project.elements.retain(|e| !gone.contains(e.id()));
    }
    n
}

/// Rewrite every automatic dimension's a, b and `through` from what it
/// measures. Returns how many changed.
pub fn rewrite(project: &mut Project) -> usize {
    let snapshot = project.clone();
    settle(&snapshot, project)
}

/// What `d` will store once added to `project`: for an automatic dimension
/// the a, b and `through` the engine rewrites it to, so a caller can report
/// the parts before the command runs. None for a plain dimension, or when
/// there is nothing to measure.
pub fn measure_now(project: &Project, d: &Dimension) -> Option<Measured> {
    let auto = d.auto.as_ref()?;
    measure(project, &analyze(project), d, auto)
}

/// What an automatic dimension measures, now. None when there is nothing
/// to measure.
pub fn measure(
    project: &Project,
    analysis: &Analysis,
    d: &Dimension,
    auto: &DimRef,
) -> Option<Measured> {
    match auto {
        DimRef::Openings { wall_ids, face } => {
            measure_run(project, analysis, &d.level_id, wall_ids, *face, d.offset_mm)
        }
        DimRef::Exterior { side, tier } => {
            measure_side(project, analysis, &d.level_id, *side, *tier)
        }
        DimRef::Grids { grid_ids } => measure_grids(project, grid_ids),
    }
}

/// Positions along `u` from `origin`, merged, into a string on the line
/// `origin + n * f` (`at` gives each one's own point).
fn string_of(mut pts: Vec<(f64, Point)>) -> Option<Measured> {
    pts.retain(|(t, p)| t.is_finite() && is_finite(*p));
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, Point)> = vec![];
    for q in pts {
        match out.last() {
            Some(last) if q.0 - last.0 < MERGE_MM => {}
            _ => out.push(q),
        }
    }
    if out.len() < 2 || out[out.len() - 1].0 - out[0].0 < MERGE_MM {
        return None;
    }
    let a = out[0].1;
    let b = out[out.len() - 1].1;
    let through = out[1..out.len() - 1].iter().map(|q| q.1).collect();
    Some(Measured { a, b, through })
}

fn walls_by_id(project: &Project) -> BTreeMap<&Id, &Wall> {
    project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) => Some((&w.id, w)),
            _ => None,
        })
        .collect()
}

/// A run of collinear walls (`DimRef::Openings`).
fn measure_run(
    project: &Project,
    analysis: &Analysis,
    level_id: &Id,
    wall_ids: &[Id],
    face: DimFace,
    offset: f64,
) -> Option<Measured> {
    let walls = walls_by_id(project);
    let mut run: Vec<&Wall> = wall_ids
        .iter()
        .filter_map(|id| walls.get(id).copied())
        .collect();
    let first = *run.first()?;
    let u = unit(sub(first.end, first.start))?;
    let n = perp_left(u);
    let origin = first.start;
    let side = if offset < 0.0 { -1.0 } else { 1.0 };
    let half = run.iter().map(|w| w.thickness_mm / 2.0).fold(0.0, f64::max);
    let f = match face {
        DimFace::Centerline => 0.0,
        DimFace::Exterior | DimFace::Interior => side * half,
    };
    let along = |p: Point| dot(sub(p, origin), u);
    let on_face = |t: f64| plus(plus(origin, scale(u, t)), scale(n, f));
    let mut pts: Vec<(f64, Point)> = vec![];

    // An inside string measures its room's side from corner to corner.
    // Where the room now runs on past an end of the run (the wall that
    // closed it moved along the line, GA P1-6), the run follows the walls in
    // line up to the first wall that closes the room again.
    let (open_joints, limits) = if face == DimFace::Interior {
        extend_room_side(project, level_id, &mut run, origin, u, scale(n, side))
    } else {
        (vec![], (f64::NEG_INFINITY, f64::INFINITY))
    };
    let run = run;
    let at_open_joint = |t: f64| open_joints.iter().any(|j| (t - j).abs() <= MERGE_MM);

    for w in &run {
        let wu = unit(sub(w.end, w.start)).unwrap_or(u);
        // The face ends from the mitred outline: its corners on that side.
        let face_ends: Vec<f64> = match (face, analysis.wall(&w.id)) {
            (DimFace::Centerline, _) | (_, None) => vec![along(w.start), along(w.end)],
            (_, Some(t)) => {
                let wn = perp_left(wu);
                let k = dot(wn, n);
                let ts: Vec<f64> = t
                    .outline
                    .iter()
                    .filter(|p| {
                        let dn = dot(sub(**p, w.start), wn) * k;
                        dn * side > 0.0 && (dn.abs() - w.thickness_mm / 2.0).abs() <= 1.0
                    })
                    .map(|p| along(*p))
                    .collect();
                if ts.len() >= 2 {
                    vec![
                        ts.iter().cloned().fold(f64::INFINITY, f64::min),
                        ts.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
                    ]
                } else {
                    vec![along(w.start), along(w.end)]
                }
            }
        };
        // A joint the run was grown through is no corner of the room.
        for t in face_ends.into_iter().filter(|t| !at_open_joint(*t)) {
            pts.push((t, on_face(t)));
        }
        // Jambs.
        for el in &project.elements {
            let Element::Opening(o) = el else { continue };
            if o.wall_id != w.id {
                continue;
            }
            for k in [-0.5, 0.5] {
                let p = plus(w.start, scale(wu, o.offset_mm + k * o.width_mm));
                let t = along(p);
                pts.push((t, on_face(t)));
            }
        }
    }

    // Walls that meet the run on the measured side (both their faces), or
    // anywhere for a centerline string (their centerline).
    for el in &project.elements {
        let Element::Wall(j) = el else { continue };
        if &j.level_id != level_id || !j.role.is_building() || run.iter().any(|w| w.id == j.id) {
            continue;
        }
        let Some(ju) = unit(sub(j.end, j.start)) else {
            continue;
        };
        if cross(ju, u).abs() < 0.0175 {
            continue;
        }
        for (end, other) in [(j.start, j.end), (j.end, j.start)] {
            let meets = run.iter().any(|w| {
                let (t, d) = project_on_segment(end, w.start, w.end);
                t > MERGE_MM
                    && t < dist(w.start, w.end) - MERGE_MM
                    && d <= w.thickness_mm / 2.0 + MERGE_MM
            });
            if !meets {
                continue;
            }
            let towards = dot(sub(other, end), n);
            match face {
                DimFace::Centerline => {
                    let t = along(end);
                    pts.push((t, on_face(t)));
                }
                _ if towards * side > 0.0 => {
                    let jn = perp_left(ju);
                    for k in [-0.5, 0.5] {
                        let face_pt = plus(end, scale(jn, k * j.thickness_mm));
                        if let Some(x) = line_intersect(face_pt, ju, on_face(0.0), u) {
                            let t = along(x);
                            pts.push((t, on_face(t)));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    // Nothing past the walls that close the room on a grown end.
    pts.retain(|(t, _)| *t < limits.1 && *t > limits.0);
    string_of(pts)
}

/// Grow an inside string's run past an end that nothing closes on the
/// room's side (`inward`), along the walls that carry its line on, to the
/// first wall that meets them on that side. Returns the joints the run grew
/// through and where the room is closed again on each grown end (the
/// closing wall's centerline, or infinity), as positions along `u` from
/// `origin`.
fn extend_room_side<'a>(
    project: &'a Project,
    level_id: &Id,
    run: &mut Vec<&'a Wall>,
    origin: Point,
    u: Point,
    inward: Point,
) -> (Vec<f64>, (f64, f64)) {
    let along = |p: Point| dot(sub(p, origin), u);
    let building: Vec<&Wall> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) if &w.level_id == level_id && w.role.is_building() => Some(w),
            _ => None,
        })
        .collect();
    let in_line = |w: &Wall| unit(sub(w.end, w.start)).map(|wu| cross(wu, u).abs() < 0.0175).unwrap_or(false);
    // A cross wall at `p` that reaches into the room closes it there.
    let closes_at = |p: Point| {
        building.iter().any(|j| {
            !in_line(j)
                && dist_point_segment(p, j.start, j.end) <= j.thickness_mm / 2.0 + JOIN_EPS
                && [j.start, j.end].iter().any(|e| dot(sub(*e, p), inward) > MERGE_MM)
        })
    };
    let mut open = vec![];
    let mut limits = (f64::NEG_INFINITY, f64::INFINITY);
    for dir in [-1.0, 1.0] {
        // At most every wall once.
        for _ in 0..building.len() {
            let Some(end) = run
                .iter()
                .flat_map(|w| [w.start, w.end])
                .max_by(|a, b| (along(*a) * dir).total_cmp(&(along(*b) * dir)))
            else {
                break;
            };
            if closes_at(end) {
                break;
            }
            let next = building.iter().copied().find(|c| {
                !run.iter().any(|w| w.id == c.id)
                    && in_line(c)
                    && [(c.start, c.end), (c.end, c.start)]
                        .iter()
                        .any(|(near, far)| dist(*near, end) <= JOIN_EPS && (along(*far) - along(end)) * dir > MERGE_MM)
            });
            let Some(c) = next else { break };
            open.push(along(end));
            run.push(c);
            // The nearest cross wall meeting it on the room's side.
            let len = dist(c.start, c.end);
            let closing = building
                .iter()
                .filter(|j| !in_line(j))
                .filter_map(|j| {
                    [(j.start, j.end), (j.end, j.start)].iter().find_map(|(e, other)| {
                        let (t, d) = project_on_segment(*e, c.start, c.end);
                        (t > MERGE_MM && t < len - MERGE_MM && d <= c.thickness_mm / 2.0 + MERGE_MM && dot(sub(*other, *e), inward) > 0.0)
                            .then(|| along(*e))
                    })
                })
                .min_by(|a, b| ((a - along(end)) * dir).total_cmp(&((b - along(end)) * dir)));
            if let Some(t) = closing {
                if dir < 0.0 {
                    limits.0 = t;
                } else {
                    limits.1 = t;
                }
                break;
            }
        }
    }
    (open, limits)
}

/// The building outlines on a level: its footprints, or the box round its
/// building walls when none closes.
fn outlines(project: &Project, analysis: &Analysis, level_id: &Id) -> Vec<Vec<Point>> {
    let fps: Vec<Vec<Point>> = analysis
        .level(level_id)
        .map(|l| {
            l.footprints
                .iter()
                .filter(|(p, _)| p.len() >= 3)
                .map(|(p, _)| p.clone())
                .collect()
        })
        .unwrap_or_default();
    if !fps.is_empty() {
        return fps;
    }
    let pts: Vec<Point> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) if &w.level_id == level_id && w.role.is_building() => Some(w),
            _ => None,
        })
        .flat_map(|w| {
            analysis
                .wall(&w.id)
                .map(|t| t.outline.clone())
                .unwrap_or_else(|| vec![w.start, w.end])
        })
        .collect();
    if pts.is_empty() {
        return vec![];
    }
    let (lo, hi) = bbox(&pts);
    if hi.x - lo.x < MERGE_MM || hi.y - lo.y < MERGE_MM {
        return vec![];
    }
    vec![vec![lo, pt(hi.x, lo.y), hi, pt(lo.x, hi.y)]]
}

/// For a side: the outward normal, and the direction a to b runs so that a
/// positive offset is outward.
fn side_axes(side: Side) -> (Point, Point) {
    let out = match side {
        Side::North => pt(0.0, 1.0),
        Side::South => pt(0.0, -1.0),
        Side::East => pt(1.0, 0.0),
        Side::West => pt(-1.0, 0.0),
    };
    // perp_left(dir) == out.
    (out, pt(out.y, -out.x))
}

/// One side of the buildings on a level (`DimRef::Exterior`).
fn measure_side(
    project: &Project,
    analysis: &Analysis,
    level_id: &Id,
    side: Side,
    tier: DimTier,
) -> Option<Measured> {
    let shapes = outlines(project, analysis, level_id);
    let corners: Vec<Point> = shapes.iter().flatten().copied().collect();
    if corners.is_empty() {
        return None;
    }
    let (out, dir) = side_axes(side);
    // The outermost face line of this side.
    let reach = corners
        .iter()
        .map(|p| dot(*p, out))
        .fold(f64::NEG_INFINITY, f64::max);
    let along = |p: Point| dot(p, dir);
    let on_ref = |t: f64| plus(scale(dir, t), scale(out, reach));
    let lo = corners
        .iter()
        .map(|p| along(*p))
        .fold(f64::INFINITY, f64::min);
    let hi = corners
        .iter()
        .map(|p| along(*p))
        .fold(f64::NEG_INFINITY, f64::max);
    let ends: Vec<(f64, Point)> = vec![(lo, on_ref(lo)), (hi, on_ref(hi))];
    if tier == DimTier::Overall {
        return string_of(ends);
    }
    let mut pts: Vec<(f64, Point)> = vec![];

    // The outline edges that face this side (outward normal within 1 degree).
    let mut facing: Vec<(Point, Point)> = vec![];
    for poly in &shapes {
        let ccw = signed_area(poly) > 0.0;
        for i in 0..poly.len() {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            let Some(e) = unit(sub(q, p)) else { continue };
            let normal = if ccw { pt(e.y, -e.x) } else { pt(-e.y, e.x) };
            if dot(normal, out) > 0.9998 {
                facing.push((p, q));
            }
        }
    }
    let walls: Vec<&Wall> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) if &w.level_id == level_id && w.role.is_building() => Some(w),
            _ => None,
        })
        .collect();
    // A wall whose outer face lies on a facing edge.
    let on_facing = |w: &Wall| -> Option<(Point, Point)> {
        let wu = unit(sub(w.end, w.start))?;
        if cross(wu, dir).abs() > 0.0175 {
            return None;
        }
        facing.iter().copied().find(|(p, q)| {
            let (_, dm) = project_on_segment(scale(plus(w.start, w.end), 0.5), *p, *q);
            let line_d = cross(dir, sub(w.start, *p)).abs();
            (line_d - w.thickness_mm / 2.0).abs() <= 2.0
                && dm <= w.thickness_mm / 2.0 + dist(w.start, w.end) / 2.0 + 2.0
        })
    };

    match tier {
        DimTier::Overall => {}
        DimTier::Walls => {
            // Wall centerlines: the walls that meet this side's outer walls
            // from inside, its corner walls among them.
            for w in &walls {
                let Some(wu) = unit(sub(w.end, w.start)) else {
                    continue;
                };
                if dot(wu, dir).abs() > 0.0175 {
                    continue;
                }
                for (end, other) in [(w.start, w.end), (w.end, w.start)] {
                    let inward = dot(sub(other, end), out) < 0.0;
                    let touches = walls.iter().any(|f| {
                        f.id != w.id
                            && on_facing(f).is_some()
                            && dist_point_segment(end, f.start, f.end)
                                <= f.thickness_mm / 2.0 + MERGE_MM
                    });
                    if inward && touches {
                        let face_y = facing
                            .iter()
                            .map(|(p, _)| dot(*p, out))
                            .filter(|y| (y - dot(end, out)).abs() <= w.thickness_mm.max(300.0))
                            .fold(dot(end, out), f64::max);
                        let t = along(end);
                        pts.push((t, plus(scale(dir, t), scale(out, face_y))));
                    }
                }
            }
            // Grid lines across this side.
            for el in &project.elements {
                let Element::Grid(g) = el else { continue };
                let Some(gu) = unit(sub(g.end, g.start)) else {
                    continue;
                };
                if dot(gu, dir).abs() > 0.0175 {
                    continue;
                }
                let t = along(g.start);
                pts.push((t, on_ref(t)));
            }
        }
        DimTier::Openings => {
            let mut any = false;
            pts.extend(ends);
            for (p, q) in &facing {
                pts.push((along(*p), *p));
                pts.push((along(*q), *q));
            }
            for w in &walls {
                let Some((p, _)) = on_facing(w) else { continue };
                let wu = unit(sub(w.end, w.start)).unwrap_or(dir);
                for el in &project.elements {
                    let Element::Opening(o) = el else { continue };
                    if o.wall_id != w.id {
                        continue;
                    }
                    for k in [-0.5, 0.5] {
                        let c = plus(w.start, scale(wu, o.offset_mm + k * o.width_mm));
                        let t = along(c);
                        pts.push((t, plus(scale(dir, t), scale(out, dot(p, out)))));
                        any = true;
                    }
                }
            }
            if !any {
                return None;
            }
        }
    }
    // The line runs along the outermost face, whatever its ends measure.
    let mut m = string_of(pts)?;
    m.a = on_ref(along(m.a));
    m.b = on_ref(along(m.b));
    Some(m)
}

/// Across grid lines (`DimRef::Grids`): through the first one's start,
/// square to it.
fn measure_grids(project: &Project, grid_ids: &[Id]) -> Option<Measured> {
    let grids: Vec<&GridLine> = grid_ids
        .iter()
        .filter_map(|id| {
            project.elements.iter().find_map(|e| match e {
                Element::Grid(g) if &g.id == id => Some(g),
                _ => None,
            })
        })
        .collect();
    let first = grids.first()?;
    let gu = unit(sub(first.end, first.start))?;
    let u = pt(gu.y, -gu.x);
    let origin = first.start;
    let pts = grids
        .iter()
        .filter_map(|g| {
            let x = line_intersect(g.start, sub(g.end, g.start), origin, u)?;
            Some((dot(sub(x, origin), u), x))
        })
        .collect();
    string_of(pts)
}

/// `Command::AutoDimension`: the standard automatic dimensions of a level,
/// replacing the automatic ones of the same kinds it has. Exterior: on each
/// side with a building face, the overall string, the walls and grids
/// string when it says more, and the openings string when the side has
/// openings. Rooms: inside strings along each room's longest wall and the
/// longest one square to it.
pub fn add(
    project: &mut Project,
    ids: &mut IdGen,
    level_id: &Id,
    kinds: &[AutoDimKind],
) -> Result<Outcome, CoreError> {
    require_level(project, level_id)?;
    ensure_unlocked(project, ElementKind::Dimension)?;
    if kinds.is_empty() {
        return Err(CoreError::invalid(
            "bad_dimension",
            "Say which automatic dimensions to add: exterior, rooms or both.",
        ));
    }
    let exterior = kinds.contains(&AutoDimKind::Exterior);
    let rooms = kinds.contains(&AutoDimKind::Rooms);
    let before = project.elements.len();
    project.elements.retain(|e| match e {
        Element::Dimension(d) if &d.level_id == level_id => match &d.auto {
            Some(DimRef::Exterior { .. }) => !exterior,
            Some(DimRef::Openings {
                face: DimFace::Interior,
                ..
            }) => !rooms,
            _ => true,
        },
        _ => true,
    });
    let replaced = before - project.elements.len();
    let analysis = analyze(project);
    let mut made: Vec<Dimension> = vec![];
    let blank = |auto: DimRef, offset: f64| Dimension {
        id: String::new(),
        level_id: level_id.clone(),
        a: pt(0.0, 0.0),
        b: pt(0.0, 0.0),
        offset_mm: offset,
        text_override: None,
        through: vec![],
        auto: Some(auto),
    };

    if exterior {
        for side in [Side::North, Side::East, Side::South, Side::West] {
            for (tier, offset) in TIER_OFFSET_MM {
                let mut d = blank(DimRef::Exterior { side, tier }, offset);
                let Some(m) = measure(project, &analysis, &d, d.auto.as_ref().unwrap()) else {
                    continue;
                };
                if tier == DimTier::Walls && m.through.is_empty() {
                    continue;
                }
                d.a = m.a;
                d.b = m.b;
                d.through = m.through;
                made.push(d);
            }
        }
    }

    if rooms {
        let mut seen: Vec<(Vec<Id>, bool)> = vec![];
        let walls = walls_by_id(project);
        if let Some(level) = analysis.level(level_id) {
            for el in &project.elements {
                let Element::Room(r) = el else { continue };
                if &r.level_id != level_id {
                    continue;
                }
                let Some(fi) = analysis.face_at(level_id, r.seed) else {
                    continue;
                };
                let f = &level.faces[fi];
                let edges: Vec<_> = f
                    .boundary
                    .iter()
                    .filter(|e| !e.wall_ids.is_empty() && dist(e.from, e.to) > 300.0)
                    .collect();
                let Some(long) = edges
                    .iter()
                    .max_by(|a, b| dist(a.from, a.to).total_cmp(&dist(b.from, b.to)))
                else {
                    continue;
                };
                let lu = unit(sub(long.to, long.from)).unwrap_or(pt(1.0, 0.0));
                let square = edges
                    .iter()
                    .filter(|e| {
                        unit(sub(e.to, e.from))
                            .map(|eu| dot(eu, lu).abs() < 0.0175)
                            .unwrap_or(false)
                    })
                    .max_by(|a, b| dist(a.from, a.to).total_cmp(&dist(b.from, b.to)));
                for e in std::iter::once(*long).chain(square.copied()) {
                    let Some(first) = walls.get(&e.wall_ids[0]) else {
                        continue;
                    };
                    let Some(u) = unit(sub(first.end, first.start)) else {
                        continue;
                    };
                    let inside = dot(sub(f.label, first.start), perp_left(u)) >= 0.0;
                    let mut key = e.wall_ids.clone();
                    key.sort();
                    if seen.contains(&(key.clone(), inside)) {
                        continue;
                    }
                    seen.push((key, inside));
                    let offset = if inside {
                        ROOM_OFFSET_MM
                    } else {
                        -ROOM_OFFSET_MM
                    };
                    let mut d = blank(
                        DimRef::Openings {
                            wall_ids: e.wall_ids.clone(),
                            face: DimFace::Interior,
                        },
                        offset,
                    );
                    let Some(m) = measure(project, &analysis, &d, d.auto.as_ref().unwrap()) else {
                        continue;
                    };
                    d.a = m.a;
                    d.b = m.b;
                    d.through = m.through;
                    made.push(d);
                }
            }
        }
    }

    if made.is_empty() {
        return Err(CoreError::invalid(
            "nothing_to_dimension",
            "This level has no building walls to dimension yet. Draw the walls first.",
        ));
    }
    let n = made.len();
    for mut d in made {
        d.id = ids.next_id();
        project.elements.push(Element::Dimension(d));
    }
    let mut summary = format!("Added {}", count_noun(n, "automatic dimension"));
    if replaced > 0 {
        summary.push_str(&format!(" in place of {replaced}"));
    }
    Ok(Outcome {
        summary: Some(summary),
        ..outcome("Automatic dimensions")
    })
}

/// The string and automatic parts of one dimension (validate hub, part of
/// `Element::Dimension`): finite witness points, and an `auto` that names
/// walls or grids that exist.
pub fn validate(project: &Project, d: &Dimension) -> Result<(), CoreError> {
    let ids = [d.id.clone()];
    for p in &d.through {
        finite_point("Dimension witness point", *p, &ids)?;
    }
    let Some(auto) = &d.auto else { return Ok(()) };
    let (refs, what): (&[Id], &str) = match auto {
        DimRef::Openings { wall_ids, .. } => (wall_ids, "wall"),
        DimRef::Grids { grid_ids } => (grid_ids, "grid line"),
        DimRef::Exterior { .. } => return Ok(()),
    };
    if refs.is_empty() {
        return Err(CoreError::invalid_for(
            "bad_dimension",
            format!("An automatic dimension needs at least one {what} to measure."),
            ids.to_vec(),
        ));
    }
    for id in refs {
        let ok = match project.elements.iter().find(|e| e.id() == id) {
            Some(Element::Wall(_)) => what == "wall",
            Some(Element::Grid(_)) => what == "grid line",
            _ => false,
        };
        if !ok {
            return Err(CoreError::invalid_for(
                "bad_dimension",
                format!("An automatic dimension measures a {what} that does not exist ({id})."),
                ids.to_vec(),
            ));
        }
    }
    Ok(())
}
