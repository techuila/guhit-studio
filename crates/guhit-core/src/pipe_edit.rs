//! Pipe editing commands (DECISIONS D39): `MovePipePoints`,
//! `SetPipeSegmentLength`, `SetPipeSlope`, `InsertPipePoint` and
//! `RemovePipePoint`. Joined runs follow a move when `keep_joins` is set.
//!
//! Following uses the joins of `Derived::pipes` (crates/guhit-core/src/
//! pipes.rs), on the geometry before the edit, with its tolerances (the
//! larger radius, at least 1 mm, in 3D with absolute heights): each run end
//! has one support at most (`rests`). An end joined to another run's point
//! moves as that point does. A tee branch keeps its place along the segment
//! it rests on and follows only the part of the move across it, height
//! included, interpolated where the segment's two points move differently,
//! and kept on the moved segment. A branch never drags the run it rests on,
//! and at three or more ends the main never follows the others. What
//! follows can carry others in turn. The runs the command names move only as
//! it says.
//!
//! Geometry only. Guhit never sizes a pipe or certifies a fall (DECISIONS
//! D19); the messages here say what changes, never whether it is right.

use guhit_model::*;

use crate::error::CoreError;
use crate::exec::{count_noun, Outcome};
use crate::pipes::{can_join, level_of, name_start, round_to, MIN_REACH_MM};
use crate::validate::PIPE_POINT_EPS_MM;

/// The shortest segment `SetPipeSegmentLength` sets. Shorter is a slip.
pub const MIN_PIPE_SEGMENT_MM: f64 = 10.0;
/// `SetPipeSlope` takes a fall from -100 to 100 percent: 45 degrees either
/// way. A segment steeper than this is a riser, not a slope, and keeps its
/// own rise.
pub const MAX_PIPE_SLOPE_PCT: f64 = 100.0;
/// Riser rule (docs/CONTRACT.md, "Pipes"): under 1 mm apart in plan, or at
/// most this while rising ten times as much.
const RISER_MAX_PLAN_MM: f64 = 50.0;
/// Followers settle in a few rounds; a loop of joins stops here.
const MAX_FOLLOW_ROUNDS: usize = 64;
/// A move shorter than this is no move.
const NO_MOVE_MM: f64 = 1e-9;

/// Applies one of the pipe editing commands. `exec::leaf` sends only those here.
pub(crate) fn apply(project: &mut Project, command: &Command) -> Result<Outcome, CoreError> {
    match command {
        Command::MovePipePoints {
            targets,
            delta,
            keep_joins,
        } => move_points(project, targets, *delta, *keep_joins),
        Command::SetPipeSegmentLength {
            pipe_id,
            segment,
            length_mm,
            hold,
            keep_joins,
        } => set_length(project, pipe_id, *segment, *length_mm, *hold, *keep_joins),
        Command::SetPipeSlope {
            pipe_id,
            from_segment,
            to_segment,
            slope_pct,
            hold,
            keep_joins,
        } => set_slope(
            project,
            pipe_id,
            *from_segment,
            *to_segment,
            *slope_pct,
            *hold,
            *keep_joins,
        ),
        Command::InsertPipePoint {
            pipe_id,
            segment,
            at,
        } => insert_point(project, pipe_id, *segment, *at),
        Command::RemovePipePoint { pipe_id, index } => remove_point(project, pipe_id, *index),
        _ => unreachable!("exec::leaf sends only the pipe editing commands here"),
    }
}

// --------------------------------------------------------------- 3D math

fn v3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3 { x, y, z }
}

fn add3(a: Vec3, b: Vec3) -> Vec3 {
    v3(a.x + b.x, a.y + b.y, a.z + b.z)
}

fn sub3(a: Vec3, b: Vec3) -> Vec3 {
    v3(a.x - b.x, a.y - b.y, a.z - b.z)
}

fn mul3(a: Vec3, k: f64) -> Vec3 {
    v3(a.x * k, a.y * k, a.z * k)
}

fn dot3(a: Vec3, b: Vec3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn len3(a: Vec3) -> f64 {
    dot3(a, a).sqrt()
}

fn dist3(a: Vec3, b: Vec3) -> f64 {
    len3(sub3(a, b))
}

fn lerp3(a: Vec3, b: Vec3, t: f64) -> Vec3 {
    add3(a, mul3(sub3(b, a), t))
}

fn finite3(a: Vec3) -> bool {
    a.x.is_finite() && a.y.is_finite() && a.z.is_finite()
}

fn plan_len(a: Vec3, b: Vec3) -> f64 {
    ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt()
}

/// A vertical segment, by the riser rule of the plan and the sheets.
fn is_vertical(a: Vec3, b: Vec3) -> bool {
    let h = plan_len(a, b);
    h < PIPE_POINT_EPS_MM || (h <= RISER_MAX_PLAN_MM && (b.z - a.z).abs() >= 10.0 * h)
}

/// A segment `SetPipeSlope` leaves at its own rise: vertical, or steeper
/// than `MAX_PIPE_SLOPE_PCT` (an offset such as the drop into a trap).
fn is_riser(a: Vec3, b: Vec3) -> bool {
    is_vertical(a, b) || (b.z - a.z).abs() > MAX_PIPE_SLOPE_PCT / 100.0 * plan_len(a, b)
}

/// Positions are kept to a millionth of a mm, as the derived pipe data is,
/// so float dust never reaches the model. A value too large to round stays
/// as it is, so validation calls it out of range, not invalid.
fn tidy(v: f64) -> f64 {
    let r = round_to(v, 1.0e6);
    if r.is_finite() {
        r
    } else {
        v
    }
}

fn tidy3(a: Vec3) -> Vec3 {
    v3(tidy(a.x), tidy(a.y), tidy(a.z))
}

/// "12.5" or "300": mm with at most one decimal.
fn mm(v: f64) -> String {
    let s = format!("{:.1}", round_to(v, 10.0));
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// "300 mm up", "500 mm east and 200 mm down".
fn delta_words(d: Vec3) -> String {
    let mut parts = vec![];
    for (v, pos, neg) in [
        (d.x, "east", "west"),
        (d.y, "north", "south"),
        (d.z, "up", "down"),
    ] {
        if v.abs() >= 0.05 {
            parts.push(format!(
                "{} mm {}",
                mm(v.abs()),
                if v > 0.0 { pos } else { neg }
            ));
        }
    }
    match parts.as_slice() {
        [] => "less than 0.1 mm".to_string(),
        [one] => one.clone(),
        [head @ .., last] => format!("{} and {last}", head.join(", ")),
    }
}

// --------------------------------------------------------------- lookups

/// The pipe `pipe_id`, or `unknown_pipe` / `not_a_pipe`.
fn find_pipe<'a>(project: &'a Project, pipe_id: &str) -> Result<&'a Pipe, CoreError> {
    match project.elements.iter().find(|e| e.id() == pipe_id) {
        Some(Element::Pipe(p)) => Ok(p),
        Some(_) => Err(CoreError::invalid_for(
            "not_a_pipe",
            format!("Element {pipe_id} is not a pipe."),
            vec![pipe_id.to_string()],
        )),
        None => Err(CoreError::invalid_for(
            "unknown_pipe",
            format!("There is no pipe with id {pipe_id}."),
            vec![pipe_id.to_string()],
        )),
    }
}

/// Segment `segment` of `pipe`, from point `segment` to `segment + 1`.
fn segment_index(pipe: &Pipe, segment: u32) -> Result<usize, CoreError> {
    let s = segment as usize;
    let count = pipe.points.len().saturating_sub(1);
    if s >= count {
        return Err(CoreError::invalid_for(
            "bad_segment",
            format!(
                "{} has no segment {segment}. Its segments are numbered 0 to {}.",
                name_start(pipe),
                count.saturating_sub(1)
            ),
            vec![pipe.id.clone()],
        ));
    }
    Ok(s)
}

/// Point `index` of `pipe`.
fn point_index(pipe: &Pipe, index: u32) -> Result<usize, CoreError> {
    let i = index as usize;
    if i >= pipe.points.len() {
        return Err(CoreError::invalid_for(
            "bad_point_index",
            format!(
                "{} has no point {index}. Its points are numbered 0 to {}.",
                name_start(pipe),
                pipe.points.len().saturating_sub(1)
            ),
            vec![pipe.id.clone()],
        ));
    }
    Ok(i)
}

fn not_finite(what: &str, ids: Vec<Id>) -> CoreError {
    CoreError::invalid_for("not_finite", format!("{what} is not a valid number."), ids)
}

// ----------------------------------------------------------------- moves

/// One run as the join rules see it, before the edit.
struct Run {
    system: PipeSystem,
    radius: f64,
    /// Centerline with absolute heights (level elevation plus z).
    pts: Vec<Vec3>,
}

impl Run {
    fn last(&self) -> usize {
        self.pts.len() - 1
    }

    /// Point index of an end: 0, or the last point.
    fn end_index(&self, last: bool) -> usize {
        if last {
            self.last()
        } else {
            0
        }
    }

    /// Direction from an end into the run, of unit length (zero when the
    /// first segment has none), as `Derived::pipes` lines ends up.
    fn away(&self, last: bool) -> Vec3 {
        let n = self.pts.len();
        let d = if last {
            sub3(self.pts[n - 2], self.pts[n - 1])
        } else {
            sub3(self.pts[1], self.pts[0])
        };
        let l = len3(d);
        if l.is_finite() && l > 1e-12 {
            mul3(d, 1.0 / l)
        } else {
            v3(0.0, 0.0, 0.0)
        }
    }
}

/// What one end of a run rests on, as `Derived::pipes` joins it, before the
/// edit. Each end has one support at most.
#[derive(Clone, Copy)]
enum Rest {
    /// A point of another run: the other end of a two run joint (an elbow or
    /// coupling), the main at a joint of three or more ends, or the bend a
    /// tee lands on. The end moves as that point does.
    Point { run: usize, index: usize },
    /// The body of segment `seg` of another run: a tee branch. The end keeps
    /// its place along the segment and moves only across it.
    Body { run: usize, seg: usize },
}

fn find_root(parent: &mut [usize], i: usize) -> usize {
    let mut r = i;
    while parent[r] != r {
        r = parent[r];
    }
    let mut c = i;
    while parent[c] != r {
        let next = parent[c];
        parent[c] = r;
        c = next;
    }
    r
}

/// The support of every run end, `[first, last]` per run, with the rules and
/// tolerances of `fittings` in crates/guhit-core/src/pipes.rs:
/// - an end on another joinable run's body, away from its ends, is a tee
///   branch on the nearest such body (the earlier run on a tie);
/// - two free ends within reach are a joint, and each follows the other;
/// - at three or more free ends the two that line up best run through, and
///   every other end, the second of those two included, follows the end of
///   the run `Derived::pipes` puts the tee on (the larger, or the one that
///   arrives). That end follows nothing there: a branch never drags the main.
fn rests(runs: &[Option<Run>]) -> Vec<[Option<Rest>; 2]> {
    let mut out: Vec<[Option<Rest>; 2]> = vec![[None, None]; runs.len()];
    let reach = |a: &Run, b: &Run| a.radius.max(b.radius).max(MIN_REACH_MM);
    let mut free: Vec<(usize, bool)> = vec![];
    for (qi, q) in runs.iter().enumerate() {
        let Some(q) = q else { continue };
        for last in [false, true] {
            let end = q.pts[q.end_index(last)];
            let mut best: Option<(f64, usize, usize, f64)> = None;
            for (ri, r) in runs.iter().enumerate() {
                let Some(r) = r else { continue };
                if ri == qi || !can_join(q.system, r.system) {
                    continue;
                }
                let near = reach(q, r);
                if dist3(end, r.pts[0]) <= near || dist3(end, r.pts[r.last()]) <= near {
                    continue;
                }
                // The nearest point of the body; the earlier segment on a tie.
                let mut on: Option<(f64, usize, f64)> = None;
                for (seg, w) in r.pts.windows(2).enumerate() {
                    let d = sub3(w[1], w[0]);
                    let l2 = dot3(d, d);
                    let t = if l2 > 0.0 {
                        (dot3(sub3(end, w[0]), d) / l2).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let dd = dist3(end, lerp3(w[0], w[1], t));
                    if on.map(|o| dd < o.0).unwrap_or(true) {
                        on = Some((dd, seg, t));
                    }
                }
                if let Some((dd, seg, t)) = on {
                    if dd <= near && best.map(|b| dd < b.0).unwrap_or(true) {
                        best = Some((dd, ri, seg, t));
                    }
                }
            }
            let slot = last as usize;
            match best {
                Some((_, run, seg, t)) => {
                    let r = runs[run].as_ref().expect("a host run is valid");
                    let at = lerp3(r.pts[seg], r.pts[seg + 1], t);
                    // A branch landing on a bend moves with the bend.
                    out[qi][slot] = Some(if dist3(at, r.pts[seg]) < PIPE_POINT_EPS_MM {
                        Rest::Point { run, index: seg }
                    } else if dist3(at, r.pts[seg + 1]) < PIPE_POINT_EPS_MM {
                        Rest::Point {
                            run,
                            index: seg + 1,
                        }
                    } else {
                        Rest::Body { run, seg }
                    });
                }
                None => free.push((qi, last)),
            }
        }
    }

    // Free ends within reach of each other form one joint.
    let run = |k: usize| {
        runs[free[k].0]
            .as_ref()
            .expect("a free end is on a valid run")
    };
    let n = free.len();
    let mut parent: Vec<usize> = (0..n).collect();
    for i in 0..n {
        for j in (i + 1)..n {
            let ((ra, la), (rb, lb)) = (free[i], free[j]);
            let (a, b) = (run(i), run(j));
            if ra == rb || !can_join(a.system, b.system) {
                continue;
            }
            if dist3(a.pts[a.end_index(la)], b.pts[b.end_index(lb)]) <= reach(a, b) {
                let (x, y) = (find_root(&mut parent, i), find_root(&mut parent, j));
                if x != y {
                    parent[x.max(y)] = x.min(y);
                }
            }
        }
    }
    let mut groups: Vec<Vec<usize>> = vec![];
    let mut slot_of: Vec<Option<usize>> = vec![None; n];
    for i in 0..n {
        let root = find_root(&mut parent, i);
        match slot_of[root] {
            Some(g) => groups[g].push(i),
            None => {
                slot_of[root] = Some(groups.len());
                groups.push(vec![i]);
            }
        }
    }
    let point = |k: usize| {
        let (r, last) = free[k];
        Rest::Point {
            run: r,
            index: run(k).end_index(last),
        }
    };
    for g in groups.iter().filter(|g| g.len() >= 2) {
        if g.len() == 2 {
            let (a, b) = (g[0], g[1]);
            if free[a].0 != free[b].0 {
                out[free[a].0][free[a].1 as usize] = Some(point(b));
                out[free[b].0][free[b].1 as usize] = Some(point(a));
            }
            continue;
        }
        // The pair of two runs that lines up best, as `fittings` picks it.
        let dir = |k: usize| run(k).away(free[k].1);
        let mut pair: Option<(usize, usize)> = None;
        let mut straightest = f64::INFINITY;
        for x in 0..g.len() {
            for y in (x + 1)..g.len() {
                if free[g[x]].0 == free[g[y]].0 {
                    continue;
                }
                let d = dot3(dir(g[x]), dir(g[y]));
                if d < straightest - 1e-12 {
                    straightest = d;
                    pair = Some((g[x], g[y]));
                }
            }
        }
        let Some((pa, pb)) = pair else { continue };
        let rank = |k: usize| (run(k).radius, free[k].1);
        let (da, db) = (rank(pa), rank(pb));
        let (main, other) = if db.0 > da.0 || (db.0 == da.0 && db.1 && !da.1) {
            (pb, pa)
        } else {
            (pa, pb)
        };
        for &k in g {
            if k == main {
                continue;
            }
            // The other end of a loop that closes here follows the second
            // run of the pair, never its own run.
            let to = if free[k].0 == free[main].0 {
                other
            } else {
                main
            };
            out[free[k].0][free[k].1 as usize] = Some(point(to));
        }
    }
    out
}

/// The moves of one edit: for every pipe, in element order, the move of each
/// point (None: it stays). Moves are level relative, which is the same as
/// absolute since a move is a difference.
struct Moves {
    /// Element index of each pipe.
    at: Vec<usize>,
    deltas: Vec<Vec<Option<Vec3>>>,
    /// Runs the command names. Their points move only as the command says,
    /// never as followers.
    edited: Vec<bool>,
}

impl Moves {
    fn new(project: &Project) -> Self {
        let mut at = vec![];
        let mut deltas = vec![];
        for (i, el) in project.elements.iter().enumerate() {
            if let Element::Pipe(p) = el {
                at.push(i);
                deltas.push(vec![None; p.points.len()]);
            }
        }
        let edited = vec![false; at.len()];
        Self { at, deltas, edited }
    }

    /// The slot of pipe `pipe_id`, which the command edits.
    fn edit(&mut self, project: &Project, pipe_id: &str) -> usize {
        let slot = self
            .at
            .iter()
            .position(|&i| project.elements[i].id() == pipe_id)
            .expect("the pipe was found before it was moved");
        self.edited[slot] = true;
        slot
    }

    /// The command moves point `index` of pipe `slot` by `d`.
    fn set(&mut self, slot: usize, index: usize, d: Vec3) {
        if len3(d) > NO_MOVE_MM {
            self.deltas[slot][index] = Some(d);
        }
    }

    /// Joined runs follow what moved (see `rests` and the module comment).
    fn follow(&mut self, project: &Project) {
        let runs: Vec<Option<Run>> = self
            .at
            .iter()
            .map(|&i| match &project.elements[i] {
                Element::Pipe(p) => {
                    let (floor, _) = level_of(project, &p.level_id);
                    let finite = p.diameter_mm.is_finite() && p.points.iter().all(|v| finite3(*v));
                    (finite && p.points.len() >= 2).then(|| Run {
                        system: p.system,
                        radius: p.diameter_mm / 2.0,
                        pts: p.points.iter().map(|v| v3(v.x, v.y, v.z + floor)).collect(),
                    })
                }
                _ => None,
            })
            .collect();
        let rests = rests(&runs);
        for _ in 0..MAX_FOLLOW_ROUNDS {
            let mut changed = false;
            for (qi, q) in runs.iter().enumerate() {
                let Some(q) = q else { continue };
                if self.edited[qi] {
                    continue;
                }
                for last in [false, true] {
                    let index = q.end_index(last);
                    let Some(rest) = rests[qi][last as usize] else {
                        continue;
                    };
                    let Some(d) = self
                        .support(&runs, rest, q.pts[index])
                        .filter(|d| len3(*d) > NO_MOVE_MM)
                    else {
                        continue;
                    };
                    let same = self.deltas[qi][index]
                        .map(|old| dist3(old, d) <= NO_MOVE_MM)
                        .unwrap_or(false);
                    if !same {
                        self.deltas[qi][index] = Some(d);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// How an end at `p` moves with what it rests on, or None while that
    /// stays.
    fn support(&self, runs: &[Option<Run>], rest: Rest, p: Vec3) -> Option<Vec3> {
        match rest {
            Rest::Point { run, index } => self.deltas[run][index],
            Rest::Body { run, seg } => {
                let (da, db) = (self.deltas[run][seg], self.deltas[run][seg + 1]);
                if da.is_none() && db.is_none() {
                    return None;
                }
                let r = runs[run].as_ref()?;
                let zero = v3(0.0, 0.0, 0.0);
                let (a, b) = (r.pts[seg], r.pts[seg + 1]);
                let (a2, b2) = (add3(a, da.unwrap_or(zero)), add3(b, db.unwrap_or(zero)));
                let along = sub3(b, a);
                let l2 = dot3(along, along);
                if !(l2.is_finite() && l2 > 0.0) {
                    return None;
                }
                // Where the end sits along the segment, and how far off its
                // centerline: both are kept.
                let t = (dot3(sub3(p, a), along) / l2).clamp(0.0, 1.0);
                let on = lerp3(a, b, t);
                let off = sub3(p, on);
                // The point of the moved segment with no move along the old
                // segment, kept on the moved segment. Along is the plan
                // direction, so height is always across; along a riser it
                // is height, so plan is across.
                let u = if is_vertical(a, b) {
                    v3(0.0, 0.0, 1.0)
                } else {
                    let h = plan_len(a, b);
                    v3(along.x / h, along.y / h, 0.0)
                };
                let k = dot3(sub3(b2, a2), u);
                let s = if k.abs() > 1e-9 {
                    dot3(sub3(on, a2), u) / k
                } else {
                    t
                };
                let moved = add3(lerp3(a2, b2, s.clamp(0.0, 1.0)), off);
                Some(sub3(moved, p))
            }
        }
    }

    /// Write the moves into `project`. Returns how many pipes the command
    /// does not edit moved because they are joined.
    fn apply(&self, project: &mut Project) -> usize {
        let mut followed = 0;
        for (slot, &i) in self.at.iter().enumerate() {
            let Element::Pipe(p) = &mut project.elements[i] else {
                continue;
            };
            let mut moved = false;
            for (k, d) in self.deltas[slot].iter().enumerate() {
                if let Some(d) = d {
                    p.points[k] = tidy3(add3(p.points[k], *d));
                    moved = true;
                }
            }
            if moved && !self.edited[slot] {
                followed += 1;
            }
        }
        followed
    }
}

/// ", 2 joined runs followed", or nothing.
fn followed_words(n: usize) -> String {
    if n == 0 {
        String::new()
    } else {
        format!(", {} followed", count_noun(n, "joined run"))
    }
}

// --------------------------------------------------------------- commands

fn move_points(
    project: &mut Project,
    targets: &[PipePoints],
    delta: Vec3,
    keep_joins: bool,
) -> Result<Outcome, CoreError> {
    if targets.is_empty() {
        return Err(CoreError::invalid(
            "no_pipe_points",
            "No pipe points were given.",
        ));
    }
    let mut ids: Vec<Id> = vec![];
    for t in targets {
        if !ids.contains(&t.pipe_id) {
            ids.push(t.pipe_id.clone());
        }
    }
    if !finite3(delta) {
        return Err(not_finite("The move distance", ids));
    }
    if len3(delta) <= NO_MOVE_MM {
        return Err(CoreError::invalid_for(
            "no_move",
            "The move distance is zero, so nothing would move.",
            ids,
        ));
    }
    let mut moves = Moves::new(project);
    let mut points = 0;
    let mut whole = true;
    for (k, t) in targets.iter().enumerate() {
        let pipe = find_pipe(project, &t.pipe_id)?;
        if targets[..k].iter().any(|o| o.pipe_id == t.pipe_id) {
            return Err(CoreError::invalid_for(
                "duplicate_pipe",
                format!(
                    "{} is listed twice. List each pipe once, with all of its points.",
                    name_start(pipe)
                ),
                vec![t.pipe_id.clone()],
            ));
        }
        let indices: Vec<usize> = if t.indices.is_empty() {
            (0..pipe.points.len()).collect()
        } else {
            let mut seen: Vec<usize> = vec![];
            for &index in &t.indices {
                let i = point_index(pipe, index)?;
                if seen.contains(&i) {
                    return Err(CoreError::invalid_for(
                        "duplicate_point",
                        format!("Point {index} of {} is listed twice.", name_start(pipe)),
                        vec![t.pipe_id.clone()],
                    ));
                }
                seen.push(i);
            }
            seen
        };
        whole &= indices.len() == pipe.points.len();
        points += indices.len();
        let slot = moves.edit(project, &t.pipe_id);
        for i in indices {
            moves.set(slot, i, delta);
        }
    }
    if keep_joins {
        moves.follow(project);
    }
    let followed = moves.apply(project);
    let (label, what) = if whole {
        (
            if ids.len() == 1 {
                "Move pipe"
            } else {
                "Move pipes"
            },
            count_noun(ids.len(), "pipe"),
        )
    } else if points == 1 {
        ("Move pipe point", "1 pipe point".to_string())
    } else {
        ("Move pipe points", count_noun(points, "pipe point"))
    };
    Ok(Outcome {
        notes: vec![],
        label: label.into(),
        summary: Some(format!(
            "Moved {what} {}{}",
            delta_words(delta),
            followed_words(followed)
        )),
        also_moved: vec![],
    })
}

fn set_length(
    project: &mut Project,
    pipe_id: &Id,
    segment: u32,
    length_mm: f64,
    hold: PipeEnd,
    keep_joins: bool,
) -> Result<Outcome, CoreError> {
    let pipe = find_pipe(project, pipe_id)?.clone();
    let ids = vec![pipe_id.clone()];
    let s = segment_index(&pipe, segment)?;
    if !length_mm.is_finite() {
        return Err(not_finite("Pipe length", ids));
    }
    if length_mm < MIN_PIPE_SEGMENT_MM {
        return Err(CoreError::invalid_for(
            "pipe_segment_short",
            format!(
                "A pipe segment must be at least {MIN_PIPE_SEGMENT_MM:.0} mm long. This one would be {} mm.",
                mm(length_mm)
            ),
            ids,
        ));
    }
    let (a, b) = (pipe.points[s], pipe.points[s + 1]);
    let along = sub3(b, a);
    let old = len3(along);
    if !(old.is_finite() && old > 1e-9) {
        return Err(CoreError::invalid_for(
            "pipe_zero_segment",
            "This segment has no direction, so its length cannot be set.",
            ids,
        ));
    }
    let dir = mul3(along, 1.0 / old);
    let mut moves = Moves::new(project);
    let slot = moves.edit(project, pipe_id);
    let n = pipe.points.len();
    match hold {
        // The last point of the segment and everything after it slide along.
        PipeEnd::Start => {
            let shift = sub3(add3(a, mul3(dir, length_mm)), b);
            for j in (s + 1)..n {
                moves.set(slot, j, shift);
            }
        }
        PipeEnd::End => {
            let shift = sub3(sub3(b, mul3(dir, length_mm)), a);
            for j in 0..=s {
                moves.set(slot, j, shift);
            }
        }
    }
    if keep_joins {
        moves.follow(project);
    }
    let followed = moves.apply(project);
    Ok(Outcome {
        notes: vec![],
        label: "Set pipe length".into(),
        summary: Some(format!(
            "Set the length of segment {segment} of {} to {} mm{}",
            name_start(&pipe),
            mm(length_mm),
            followed_words(followed)
        )),
        also_moved: vec![],
    })
}

fn set_slope(
    project: &mut Project,
    pipe_id: &Id,
    from_segment: u32,
    to_segment: u32,
    slope_pct: f64,
    hold: PipeEnd,
    keep_joins: bool,
) -> Result<Outcome, CoreError> {
    let pipe = find_pipe(project, pipe_id)?.clone();
    let ids = vec![pipe_id.clone()];
    let from = segment_index(&pipe, from_segment)?;
    let to = segment_index(&pipe, to_segment)?;
    if from > to {
        return Err(CoreError::invalid_for(
            "bad_segment",
            format!(
                "The first segment ({from_segment}) comes after the last ({to_segment}). Give them in point order."
            ),
            ids,
        ));
    }
    if !slope_pct.is_finite() {
        return Err(not_finite("Pipe slope", ids));
    }
    if slope_pct.abs() > MAX_PIPE_SLOPE_PCT {
        return Err(CoreError::invalid_for(
            "bad_slope",
            format!(
                "A pipe slope must be between -{0:.0} and {0:.0} percent, got {1}.",
                MAX_PIPE_SLOPE_PCT,
                mm(slope_pct)
            ),
            ids,
        ));
    }
    let pts = &pipe.points;
    let sloped = (from..=to)
        .filter(|&k| !is_riser(pts[k], pts[k + 1]))
        .count();
    if sloped == 0 {
        let which = if from == to {
            format!(
                "Segment {from_segment} of {} is vertical or steeper than 45 degrees",
                name_start(&pipe)
            )
        } else {
            format!(
                "Segments {from_segment} to {to_segment} of {} are vertical or steeper than 45 degrees",
                name_start(&pipe)
            )
        };
        return Err(CoreError::invalid_for(
            "no_slope_segment",
            format!("{which}, so there is no slope to set."),
            ids,
        ));
    }
    // The rise of segment k from its first point to its last: a riser or a
    // steep offset keeps its own, every other segment falls by the slope of
    // its plan length.
    let rise = |k: usize| {
        if is_riser(pts[k], pts[k + 1]) {
            pts[k + 1].z - pts[k].z
        } else {
            -slope_pct / 100.0 * plan_len(pts[k], pts[k + 1])
        }
    };
    let mut z: Vec<f64> = pts.iter().map(|p| p.z).collect();
    match hold {
        PipeEnd::Start => {
            for k in from..=to {
                z[k + 1] = z[k] + rise(k);
            }
            let shift = z[to + 1] - pts[to + 1].z;
            for zj in z.iter_mut().skip(to + 2) {
                *zj += shift;
            }
        }
        PipeEnd::End => {
            for k in (from..=to).rev() {
                z[k] = z[k + 1] - rise(k);
            }
            let shift = z[from] - pts[from].z;
            for zj in z.iter_mut().take(from) {
                *zj += shift;
            }
        }
    }
    let mut moves = Moves::new(project);
    let slot = moves.edit(project, pipe_id);
    for (j, p) in pts.iter().enumerate() {
        moves.set(slot, j, v3(0.0, 0.0, z[j] - p.z));
    }
    if keep_joins {
        moves.follow(project);
    }
    let followed = moves.apply(project);
    let verb = if slope_pct < 0.0 { "rise" } else { "fall" };
    Ok(Outcome {
        notes: vec![],
        label: "Set pipe slope".into(),
        summary: Some(format!(
            "Set {} of {} to {verb} {} percent{}",
            count_noun(sloped, "segment"),
            name_start(&pipe),
            mm(slope_pct.abs()),
            followed_words(followed)
        )),
        also_moved: vec![],
    })
}

fn insert_point(
    project: &mut Project,
    pipe_id: &Id,
    segment: u32,
    at: Vec3,
) -> Result<Outcome, CoreError> {
    let pipe = find_pipe(project, pipe_id)?;
    let ids = vec![pipe_id.clone()];
    let s = segment_index(pipe, segment)?;
    if !finite3(at) {
        return Err(not_finite("The new bend", ids));
    }
    let (a, b) = (pipe.points[s], pipe.points[s + 1]);
    if dist3(at, a) < PIPE_POINT_EPS_MM || dist3(at, b) < PIPE_POINT_EPS_MM {
        return Err(CoreError::invalid_for(
            "bend_on_point",
            format!(
                "The new bend is on point {} or {} of {}. Put it between them.",
                s,
                s + 1,
                name_start(pipe)
            ),
            ids,
        ));
    }
    if let Some(Element::Pipe(p)) = project.elements.iter_mut().find(|e| e.id() == pipe_id) {
        p.points.insert(s + 1, at);
    }
    Ok(Outcome {
        notes: vec![],
        label: "Add bend".into(),
        summary: None,
        also_moved: vec![],
    })
}

fn remove_point(project: &mut Project, pipe_id: &Id, index: u32) -> Result<Outcome, CoreError> {
    let pipe = find_pipe(project, pipe_id)?;
    let i = point_index(pipe, index)?;
    let n = pipe.points.len();
    if n <= 2 {
        return Err(CoreError::invalid_for(
            "pipe_too_short",
            format!(
                "{} has only two points, and a pipe needs at least two. Delete the pipe instead.",
                name_start(pipe)
            ),
            vec![pipe_id.clone()],
        ));
    }
    let end = i == 0 || i == n - 1;
    if let Some(Element::Pipe(p)) = project.elements.iter_mut().find(|e| e.id() == pipe_id) {
        p.points.remove(i);
    }
    Ok(Outcome {
        notes: vec![],
        label: if end {
            "Remove pipe end"
        } else {
            "Remove bend"
        }
        .into(),
        summary: None,
        also_moved: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compute_derived, scope, CoreError, Document};

    /// A run: id, system, size and points.
    type Spec<'a> = (&'a str, PipeSystem, f64, &'a [(f64, f64, f64)]);

    fn project(pipes: &[Spec]) -> Project {
        let mut p = defaults::new_project("Pipe edit");
        let level = p.levels[0].id.clone();
        for (id, system, d, pts) in pipes {
            p.elements.push(Element::Pipe(Pipe {
                id: (*id).into(),
                level_id: level.clone(),
                system: *system,
                material: defaults::pipe_defaults(*system).0,
                diameter_mm: *d,
                points: pts.iter().map(|(x, y, z)| v3(*x, *y, *z)).collect(),
                name: String::new(),
            }));
        }
        p
    }

    fn doc(pipes: &[Spec]) -> Document {
        Document::new(project(pipes))
    }

    fn points(doc: &Document, id: &str) -> Vec<(f64, f64, f64)> {
        match doc.project().elements.iter().find(|e| e.id() == id) {
            Some(Element::Pipe(p)) => p.points.iter().map(|v| (v.x, v.y, v.z)).collect(),
            _ => panic!("no pipe {id}"),
        }
    }

    fn apply(doc: &mut Document, command: Command) -> ApplyResult {
        doc.apply(command, Origin::User)
            .unwrap_or_else(|e| panic!("{e:?}"))
    }

    fn refused(doc: &Document, command: Command) -> (String, String, Vec<Id>) {
        match doc.preview(&command) {
            Err(CoreError::Invalid {
                code,
                message,
                element_ids,
            }) => (code, message, element_ids),
            other => panic!(
                "expected a refusal, got {:?}",
                other.map(|r| r.diff.summary)
            ),
        }
    }

    fn mv(targets: &[(&str, &[u32])], d: (f64, f64, f64), keep_joins: bool) -> Command {
        Command::MovePipePoints {
            targets: targets
                .iter()
                .map(|(id, ix)| PipePoints {
                    pipe_id: (*id).into(),
                    indices: ix.to_vec(),
                })
                .collect(),
            delta: v3(d.0, d.1, d.2),
            keep_joins,
        }
    }

    fn length(id: &str, segment: u32, length_mm: f64, hold: PipeEnd, keep_joins: bool) -> Command {
        Command::SetPipeSegmentLength {
            pipe_id: id.into(),
            segment,
            length_mm,
            hold,
            keep_joins,
        }
    }

    fn slope(id: &str, from: u32, to: u32, pct: f64, hold: PipeEnd, keep_joins: bool) -> Command {
        Command::SetPipeSlope {
            pipe_id: id.into(),
            from_segment: from,
            to_segment: to,
            slope_pct: pct,
            hold,
            keep_joins,
        }
    }

    use PipeSystem::{ColdWater as Cold, Drainage, HotWater as Hot, Vent};

    // ------------------------------------------------------------ move

    #[test]
    fn a_point_moves_in_3d_and_only_that_point() {
        let mut d = doc(&[(
            "a",
            Cold,
            20.0,
            &[
                (0.0, 0.0, 300.0),
                (1000.0, 0.0, 300.0),
                (1000.0, 1000.0, 300.0),
            ],
        )]);
        let r = apply(&mut d, mv(&[("a", &[1])], (0.0, 0.0, 200.0), false));
        assert_eq!(
            points(&d, "a"),
            vec![
                (0.0, 0.0, 300.0),
                (1000.0, 0.0, 500.0),
                (1000.0, 1000.0, 300.0)
            ]
        );
        assert_eq!(r.diff.summary, "Moved 1 pipe point 200 mm up");
        assert_eq!(d.history().undo[0].label, "Move pipe point");
        // Several points, and the whole run.
        let r = apply(&mut d, mv(&[("a", &[0, 2])], (100.0, -50.0, 0.0), false));
        assert_eq!(
            r.diff.summary,
            "Moved 2 pipe points 100 mm east and 50 mm south"
        );
        assert_eq!(d.history().undo[0].label, "Move pipe points");
        let r = apply(&mut d, mv(&[("a", &[])], (0.0, 0.0, -150.0), false));
        assert_eq!(r.diff.summary, "Moved 1 pipe 150 mm down");
        assert_eq!(d.history().undo[0].label, "Move pipe");
        assert_eq!(
            points(&d, "a"),
            vec![
                (100.0, -50.0, 150.0),
                (1000.0, 0.0, 350.0),
                (1100.0, 950.0, 150.0)
            ]
        );
    }

    #[test]
    fn an_elbow_between_two_runs_holds_when_one_end_moves() {
        let runs: &[Spec] = &[
            ("a", Cold, 20.0, &[(0.0, 0.0, 300.0), (1000.0, 0.0, 300.0)]),
            (
                "b",
                Cold,
                20.0,
                &[(1000.0, 0.0, 300.0), (1000.0, 1000.0, 300.0)],
            ),
        ];
        let mut d = doc(runs);
        let r = apply(&mut d, mv(&[("a", &[1])], (0.0, 0.0, -100.0), true));
        assert_eq!(
            points(&d, "b"),
            vec![(1000.0, 0.0, 200.0), (1000.0, 1000.0, 300.0)]
        );
        assert_eq!(
            r.diff.summary,
            "Moved 1 pipe point 100 mm down, 1 joined run followed"
        );
        assert_eq!(
            d.derived().pipes.elbow_count,
            1,
            "the runs still meet at an elbow"
        );
        // Without keep_joins the other run stays and the joint opens.
        let mut d = doc(runs);
        let r = apply(&mut d, mv(&[("a", &[1])], (0.0, 0.0, -100.0), false));
        assert_eq!(
            points(&d, "b"),
            vec![(1000.0, 0.0, 300.0), (1000.0, 1000.0, 300.0)]
        );
        assert_eq!(r.diff.summary, "Moved 1 pipe point 100 mm down");
    }

    #[test]
    fn a_tee_branch_follows_the_segment_under_it_in_proportion() {
        let runs: &[Spec] = &[
            (
                "main",
                Cold,
                25.0,
                &[(0.0, 0.0, 300.0), (2000.0, 0.0, 300.0)],
            ),
            (
                "branch",
                Cold,
                20.0,
                &[(500.0, 0.0, 300.0), (500.0, 800.0, 300.0)],
            ),
        ];
        // Only the far end of the main rises: a quarter along, the branch
        // rises a quarter as much.
        let mut d = doc(runs);
        apply(&mut d, mv(&[("main", &[1])], (0.0, 0.0, 400.0), true));
        assert_eq!(
            points(&d, "branch"),
            vec![(500.0, 0.0, 400.0), (500.0, 800.0, 300.0)]
        );
        assert_eq!(
            d.derived().pipes.tee_count,
            1,
            "the branch still lands on the main"
        );
        // The whole main: the branch end moves with it.
        let mut d = doc(runs);
        apply(&mut d, mv(&[("main", &[])], (0.0, 300.0, 0.0), true));
        assert_eq!(
            points(&d, "branch"),
            vec![(500.0, 300.0, 300.0), (500.0, 800.0, 300.0)]
        );
        assert_eq!(d.derived().pipes.tee_count, 1);
        // Moving the branch never drags the main.
        let mut d = doc(runs);
        apply(&mut d, mv(&[("branch", &[0])], (100.0, 0.0, 0.0), true));
        assert_eq!(
            points(&d, "main"),
            vec![(0.0, 0.0, 300.0), (2000.0, 0.0, 300.0)]
        );
    }

    #[test]
    fn what_follows_carries_its_own_branches_and_other_systems_stay() {
        // b continues a at an elbow; c branches off b near that elbow; a hot
        // water end rests on b too but is not joined to it.
        let mut d = doc(&[
            ("a", Cold, 20.0, &[(0.0, 0.0, 300.0), (1000.0, 0.0, 300.0)]),
            (
                "b",
                Cold,
                20.0,
                &[(1000.0, 0.0, 300.0), (1000.0, 2000.0, 300.0)],
            ),
            (
                "c",
                Cold,
                20.0,
                &[(1000.0, 500.0, 300.0), (2000.0, 500.0, 300.0)],
            ),
            (
                "hot",
                Hot,
                20.0,
                &[(1000.0, 1000.0, 300.0), (2000.0, 1000.0, 300.0)],
            ),
        ]);
        let r = apply(&mut d, mv(&[("a", &[1])], (0.0, 0.0, 400.0), true));
        assert_eq!(points(&d, "b")[0], (1000.0, 0.0, 700.0));
        assert_eq!(
            points(&d, "c")[0],
            (1000.0, 500.0, 600.0),
            "a quarter along b"
        );
        assert_eq!(points(&d, "hot")[0], (1000.0, 1000.0, 300.0));
        assert_eq!(
            r.diff.summary,
            "Moved 1 pipe point 400 mm up, 2 joined runs followed"
        );
        // Drainage and vent join, so a vent on a drain follows it.
        let mut d = doc(&[
            (
                "drain",
                Drainage,
                100.0,
                &[(0.0, 0.0, -300.0), (2000.0, 0.0, -340.0)],
            ),
            (
                "vent",
                Vent,
                50.0,
                &[(1000.0, 0.0, -320.0), (1000.0, 0.0, 3000.0)],
            ),
        ]);
        apply(&mut d, mv(&[("drain", &[])], (0.0, 0.0, -100.0), true));
        assert_eq!(points(&d, "vent")[0], (1000.0, 0.0, -420.0));
    }

    #[test]
    fn a_tee_keeps_its_place_along_the_main_and_follows_only_across_it() {
        let runs: &[Spec] = &[
            (
                "main",
                Cold,
                25.0,
                &[(0.0, 0.0, 300.0), (2000.0, 0.0, 300.0)],
            ),
            (
                "branch",
                Cold,
                20.0,
                &[(500.0, 0.0, 300.0), (500.0, 800.0, 300.0)],
            ),
        ];
        let start = |d: &Document| points(d, "branch")[0];
        // A longer main, either end held: the branch stays where it is.
        for (hold, main) in [
            (
                PipeEnd::Start,
                vec![(0.0, 0.0, 300.0), (3000.0, 0.0, 300.0)],
            ),
            (
                PipeEnd::End,
                vec![(-1000.0, 0.0, 300.0), (2000.0, 0.0, 300.0)],
            ),
        ] {
            let mut d = doc(runs);
            apply(&mut d, length("main", 0, 3000.0, hold, true));
            assert_eq!(points(&d, "main"), main);
            assert_eq!(start(&d), (500.0, 0.0, 300.0));
            assert_eq!(d.derived().pipes.tee_count, 1);
        }
        // The whole main along its own axis: the tee stays put. Across and
        // up: the branch follows that part only.
        let mut d = doc(runs);
        apply(&mut d, mv(&[("main", &[])], (300.0, 0.0, 0.0), true));
        assert_eq!(start(&d), (500.0, 0.0, 300.0));
        let mut d = doc(runs);
        apply(&mut d, mv(&[("main", &[])], (300.0, 200.0, 50.0), true));
        assert_eq!(start(&d), (500.0, 200.0, 350.0));
        // One end moving along and up: the branch stays at x = 500, on the
        // moved main.
        let mut d = doc(runs);
        apply(&mut d, mv(&[("main", &[1])], (1000.0, 0.0, 400.0), true));
        let b = start(&d);
        assert_eq!((b.0, b.1), (500.0, 0.0));
        assert!(
            (b.2 - (300.0 + 400.0 * 500.0 / 3000.0)).abs() < 1e-6,
            "{b:?}"
        );
        assert_eq!(d.derived().pipes.tee_count, 1);
        // A shorter main keeps the branch on it.
        let mut d = doc(&[
            (
                "main",
                Cold,
                25.0,
                &[(0.0, 0.0, 300.0), (2000.0, 0.0, 300.0)],
            ),
            (
                "branch",
                Cold,
                20.0,
                &[(1900.0, 0.0, 300.0), (1900.0, 800.0, 300.0)],
            ),
        ]);
        apply(&mut d, length("main", 0, 1500.0, PipeEnd::Start, true));
        assert_eq!(start(&d), (1500.0, 0.0, 300.0));
        // Along a riser, height is along: a taller riser keeps the branch at
        // its height, a riser moved in plan carries it.
        let riser: &[Spec] = &[
            ("riser", Cold, 25.0, &[(0.0, 0.0, 0.0), (0.0, 0.0, 2000.0)]),
            (
                "branch",
                Cold,
                20.0,
                &[(0.0, 0.0, 1000.0), (800.0, 0.0, 1000.0)],
            ),
        ];
        let mut d = doc(riser);
        apply(&mut d, length("riser", 0, 3000.0, PipeEnd::Start, true));
        assert_eq!(start(&d), (0.0, 0.0, 1000.0));
        let mut d = doc(riser);
        apply(&mut d, mv(&[("riser", &[])], (200.0, 0.0, 500.0), true));
        assert_eq!(start(&d), (200.0, 0.0, 1000.0));
    }

    #[test]
    fn a_branch_on_a_main_never_lifts_another_branch_off_it() {
        let runs: &[Spec] = &[
            (
                "main",
                Cold,
                25.0,
                &[(0.0, 0.0, 300.0), (2000.0, 0.0, 300.0)],
            ),
            (
                "north",
                Cold,
                20.0,
                &[(1000.0, 0.0, 300.0), (1000.0, 800.0, 300.0)],
            ),
            (
                "south",
                Cold,
                20.0,
                &[(1000.0, 0.0, 300.0), (1000.0, -800.0, 300.0)],
            ),
        ];
        let mut d = doc(runs);
        apply(&mut d, mv(&[("north", &[0])], (0.0, 0.0, 50.0), true));
        assert_eq!(points(&d, "south")[0], (1000.0, 0.0, 300.0));
        assert_eq!(
            points(&d, "main"),
            vec![(0.0, 0.0, 300.0), (2000.0, 0.0, 300.0)]
        );
        // The main carries both.
        let mut d = doc(runs);
        apply(&mut d, mv(&[("main", &[])], (0.0, 0.0, 100.0), true));
        assert_eq!(points(&d, "north")[0], (1000.0, 0.0, 400.0));
        assert_eq!(points(&d, "south")[0], (1000.0, 0.0, 400.0));
    }

    #[test]
    fn at_three_ends_the_main_never_follows() {
        // The 100 mm drain and a 50 mm waste run through, a vent branches.
        let runs: &[Spec] = &[
            (
                "drain",
                Drainage,
                100.0,
                &[(0.0, 0.0, -300.0), (2000.0, 0.0, -340.0)],
            ),
            (
                "waste",
                Drainage,
                50.0,
                &[(-2000.0, 0.0, -260.0), (0.0, 0.0, -300.0)],
            ),
            (
                "vent",
                Vent,
                50.0,
                &[(0.0, 0.0, -300.0), (0.0, 0.0, 3000.0)],
            ),
        ];
        for moved in ["waste", "vent"] {
            let mut d = doc(runs);
            let r = apply(&mut d, mv(&[(moved, &[])], (0.0, 0.0, -100.0), true));
            assert_eq!(
                points(&d, "drain")[0],
                (0.0, 0.0, -300.0),
                "{moved} dragged the drain"
            );
            assert!(!r.diff.summary.contains("followed"), "{}", r.diff.summary);
        }
        let mut d = doc(runs);
        let r = apply(&mut d, mv(&[("drain", &[0])], (0.0, 0.0, -20.0), true));
        assert_eq!(points(&d, "waste")[1], (0.0, 0.0, -320.0));
        assert_eq!(points(&d, "vent")[0], (0.0, 0.0, -320.0));
        assert!(
            r.diff.summary.ends_with(", 2 joined runs followed"),
            "{}",
            r.diff.summary
        );
    }

    #[test]
    fn the_runs_an_edit_names_never_move_as_followers() {
        // a's start rests on b, and b's start rests on a: a loop of joins.
        let mut d = doc(&[
            (
                "a",
                Cold,
                20.0,
                &[(0.0, 0.0, 0.0), (1000.0, 0.0, 0.0), (1000.0, 1000.0, 0.0)],
            ),
            (
                "b",
                Cold,
                20.0,
                &[(1000.0, 500.0, 0.0), (-1000.0, -500.0, 0.0)],
            ),
        ]);
        apply(&mut d, mv(&[("a", &[2])], (0.0, 0.0, 400.0), true));
        assert_eq!(points(&d, "b")[0], (1000.0, 500.0, 200.0));
        assert_eq!(points(&d, "a")[0], (0.0, 0.0, 0.0));
    }

    #[test]
    fn whole_runs_are_named_only_when_every_target_is_whole() {
        let runs: &[Spec] = &[
            (
                "a",
                Cold,
                20.0,
                &[
                    (0.0, 0.0, 300.0),
                    (1000.0, 0.0, 300.0),
                    (1000.0, 1000.0, 300.0),
                ],
            ),
            (
                "b",
                Cold,
                20.0,
                &[(0.0, 5000.0, 300.0), (1000.0, 5000.0, 300.0)],
            ),
        ];
        for order in [
            [("a", &[1u32][..]), ("b", &[0, 1][..])],
            [("b", &[0, 1][..]), ("a", &[1][..])],
        ] {
            let mut d = doc(runs);
            let r = apply(&mut d, mv(&order, (0.0, 0.0, 100.0), false));
            assert_eq!(r.diff.summary, "Moved 3 pipe points 100 mm up");
            assert_eq!(d.history().undo[0].label, "Move pipe points");
        }
        let mut d = doc(runs);
        let r = apply(
            &mut d,
            mv(&[("a", &[]), ("b", &[1, 0])], (0.0, 0.0, 100.0), false),
        );
        assert_eq!(r.diff.summary, "Moved 2 pipes 100 mm up");
    }

    #[test]
    fn the_services_demo_keeps_its_review_items_when_wastes_move() {
        const DRAIN: &str = "00000000-0000-4000-8000-000000016010";
        const LAV: &str = "00000000-0000-4000-8000-000000016012";
        const SINK: &str = "00000000-0000-4000-8000-000000016014";
        const CHASE: &str = "00000000-0000-4000-8000-000000016002";
        let items = |d: &Document| -> Vec<(String, Vec<Id>)> {
            d.derived()
                .issues
                .iter()
                .map(|i| (i.code.clone(), i.element_ids.clone()))
                .collect()
        };
        let base = Document::new(crate::templates::plumbing_demo());
        let before = items(&base);
        // "Lower the sink waste 300 mm" and the lavatory waste 100 mm: the
        // building drain is the main where they meet, so it stays.
        for (id, dz) in [(SINK, -300.0), (LAV, -100.0)] {
            let mut d = Document::new(crate::templates::plumbing_demo());
            apply(&mut d, mv(&[(id, &[])], (0.0, 0.0, dz), true));
            assert_eq!(points(&d, DRAIN), points(&base, DRAIN), "{id}");
            assert_eq!(items(&d), before, "{id}");
        }
        // The whole sink waste at 2 percent: its 86 mm drop into the drain
        // is an offset and keeps its rise.
        let mut d = Document::new(crate::templates::plumbing_demo());
        apply(&mut d, slope(SINK, 0, 2, 2.0, PipeEnd::Start, true));
        let w = points(&d, SINK);
        assert!((w[2].2 - w[3].2 - 86.0).abs() < 1e-6, "{w:?}");
        assert!(
            (w[1].2 - w[2].2 - 86.0).abs() < 1e-6,
            "2 percent of 4300 mm"
        );
        assert_eq!(points(&d, DRAIN), points(&base, DRAIN));
        // The chase slid along its wall: the supplies teed on it stay put.
        let mut d = Document::new(crate::templates::plumbing_demo());
        apply(&mut d, mv(&[(CHASE, &[])], (100.0, 0.0, 0.0), true));
        for id in [
            "00000000-0000-4000-8000-000000016004",
            "00000000-0000-4000-8000-000000016005",
            "00000000-0000-4000-8000-000000016007",
        ] {
            assert_eq!(points(&d, id), points(&base, id), "{id}");
        }
    }

    #[test]
    fn a_steep_offset_keeps_its_drop_under_a_slope() {
        let mut d = doc(&[(
            "a",
            Drainage,
            50.0,
            &[
                (0.0, 0.0, 0.0),
                (2000.0, 0.0, 0.0),
                (2060.0, 0.0, -390.0),
                (3000.0, 0.0, -390.0),
            ],
        )]);
        let r = apply(&mut d, slope("a", 0, 2, 2.0, PipeEnd::Start, false));
        let z: Vec<f64> = points(&d, "a").iter().map(|p| p.2).collect();
        assert_eq!(z, vec![0.0, -40.0, -430.0, -448.8]);
        assert_eq!(
            r.diff.summary,
            "Set 2 segments of Drainage pipe 50 mm to fall 2 percent"
        );
    }

    // ---------------------------------------------------------- length

    #[test]
    fn a_segment_length_keeps_the_rest_of_the_run_as_it_was() {
        let run: &[(f64, f64, f64)] = &[
            (0.0, 0.0, 0.0),
            (1000.0, 0.0, 0.0),
            (1000.0, 1000.0, 0.0),
            (2000.0, 1000.0, -10.0),
        ];
        let mut d = doc(&[("a", Drainage, 50.0, run)]);
        let r = apply(&mut d, length("a", 0, 1500.0, PipeEnd::Start, false));
        assert_eq!(
            points(&d, "a"),
            vec![
                (0.0, 0.0, 0.0),
                (1500.0, 0.0, 0.0),
                (1500.0, 1000.0, 0.0),
                (2500.0, 1000.0, -10.0)
            ]
        );
        assert_eq!(
            r.diff.summary,
            "Set the length of segment 0 of Drainage pipe 50 mm to 1500 mm"
        );
        assert_eq!(d.history().undo[0].label, "Set pipe length");
        // Hold the end: the start side slides instead.
        let mut d = doc(&[("a", Drainage, 50.0, run)]);
        apply(&mut d, length("a", 1, 500.0, PipeEnd::End, false));
        assert_eq!(
            points(&d, "a"),
            vec![
                (0.0, 500.0, 0.0),
                (1000.0, 500.0, 0.0),
                (1000.0, 1000.0, 0.0),
                (2000.0, 1000.0, -10.0)
            ]
        );
        // The length is 3D: a sloped segment keeps its slope.
        let mut d = doc(&[(
            "s",
            Drainage,
            50.0,
            &[(0.0, 0.0, 0.0), (3000.0, 0.0, -60.0)],
        )]);
        apply(&mut d, length("s", 0, 1500.0, PipeEnd::Start, false));
        let p = points(&d, "s");
        let (dx, dz) = (p[1].0 - p[0].0, p[1].2 - p[0].2);
        assert!(((dx * dx + dz * dz).sqrt() - 1500.0).abs() < 1e-6);
        assert!((dz / dx + 0.02).abs() < 1e-9, "still 2 percent");
    }

    #[test]
    fn a_longer_segment_carries_what_is_joined_beyond_it() {
        let runs: &[Spec] = &[
            (
                "a",
                Cold,
                20.0,
                &[
                    (0.0, 0.0, 300.0),
                    (1000.0, 0.0, 300.0),
                    (1000.0, 1000.0, 300.0),
                ],
            ),
            (
                "b",
                Cold,
                20.0,
                &[(1000.0, 500.0, 300.0), (2000.0, 500.0, 300.0)],
            ),
            ("c", Cold, 20.0, &[(0.0, 0.0, 300.0), (0.0, -800.0, 300.0)]),
        ];
        let mut d = doc(runs);
        let r = apply(&mut d, length("a", 0, 1200.0, PipeEnd::Start, true));
        assert_eq!(points(&d, "b")[0], (1200.0, 500.0, 300.0));
        assert_eq!(
            points(&d, "c")[0],
            (0.0, 0.0, 300.0),
            "the held end stays, so does c"
        );
        assert_eq!(r.diff.summary, "Set the length of segment 0 of Cold water pipe 20 mm to 1200 mm, 1 joined run followed");
        let mut d = doc(runs);
        apply(&mut d, length("a", 0, 1200.0, PipeEnd::End, true));
        assert_eq!(points(&d, "c")[0], (-200.0, 0.0, 300.0));
        assert_eq!(points(&d, "b")[0], (1000.0, 500.0, 300.0));
    }

    // ----------------------------------------------------------- slope

    #[test]
    fn a_slope_falls_from_the_held_end_and_a_riser_keeps_its_drop() {
        let run: &[(f64, f64, f64)] = &[
            (0.0, 0.0, 0.0),
            (1000.0, 0.0, 0.0),
            (1000.0, 0.0, -500.0),
            (3000.0, 0.0, -500.0),
            (4000.0, 0.0, -500.0),
        ];
        let mut d = doc(&[("a", Drainage, 50.0, run)]);
        let r = apply(&mut d, slope("a", 0, 2, 2.0, PipeEnd::Start, false));
        let z: Vec<f64> = points(&d, "a").iter().map(|p| p.2).collect();
        assert_eq!(z, vec![0.0, -20.0, -520.0, -560.0, -560.0]);
        assert_eq!(
            r.diff.summary,
            "Set 2 segments of Drainage pipe 50 mm to fall 2 percent"
        );
        assert_eq!(d.history().undo[0].label, "Set pipe slope");
        // Hold the end: the last point of the range stays, the start rises.
        let mut d = doc(&[("a", Drainage, 50.0, run)]);
        apply(&mut d, slope("a", 0, 2, 2.0, PipeEnd::End, false));
        let z: Vec<f64> = points(&d, "a").iter().map(|p| p.2).collect();
        assert_eq!(z, vec![60.0, 40.0, -460.0, -500.0, -500.0]);
        // Points past the start move with it when the range starts later.
        let mut d = doc(&[("a", Drainage, 50.0, run)]);
        apply(&mut d, slope("a", 3, 3, 1.0, PipeEnd::End, false));
        let z: Vec<f64> = points(&d, "a").iter().map(|p| p.2).collect();
        assert_eq!(z, vec![10.0, 10.0, -490.0, -490.0, -500.0]);
        // A negative slope rises.
        let mut d = doc(&[("a", Drainage, 50.0, run)]);
        let r = apply(&mut d, slope("a", 3, 3, -1.5, PipeEnd::Start, false));
        assert_eq!(points(&d, "a")[4].2, -485.0);
        assert_eq!(
            r.diff.summary,
            "Set 1 segment of Drainage pipe 50 mm to rise 1.5 percent"
        );
    }

    #[test]
    fn a_new_slope_carries_the_runs_joined_downstream() {
        let mut d = doc(&[
            (
                "drain",
                Drainage,
                100.0,
                &[
                    (0.0, 0.0, -300.0),
                    (2000.0, 0.0, -300.0),
                    (3000.0, 0.0, -300.0),
                ],
            ),
            (
                "wc",
                Drainage,
                100.0,
                &[(3000.0, 1000.0, 0.0), (3000.0, 0.0, -300.0)],
            ),
        ]);
        let r = apply(&mut d, slope("drain", 0, 0, 1.0, PipeEnd::Start, true));
        assert_eq!(
            points(&d, "drain"),
            vec![
                (0.0, 0.0, -300.0),
                (2000.0, 0.0, -320.0),
                (3000.0, 0.0, -320.0)
            ]
        );
        assert_eq!(points(&d, "wc")[1], (3000.0, 0.0, -320.0));
        assert!(
            r.diff.summary.ends_with(", 1 joined run followed"),
            "{}",
            r.diff.summary
        );
    }

    // ------------------------------------------------------------ bends

    #[test]
    fn a_bend_is_added_and_removed() {
        let mut d = doc(&[("a", Cold, 20.0, &[(0.0, 0.0, 300.0), (2000.0, 0.0, 300.0)])]);
        let r = apply(
            &mut d,
            Command::InsertPipePoint {
                pipe_id: "a".into(),
                segment: 0,
                at: v3(1000.0, 500.0, 300.0),
            },
        );
        assert_eq!(
            points(&d, "a"),
            vec![
                (0.0, 0.0, 300.0),
                (1000.0, 500.0, 300.0),
                (2000.0, 0.0, 300.0)
            ]
        );
        assert_eq!(r.diff.summary, "Add bend: changed 1 pipe");
        let r = apply(
            &mut d,
            Command::RemovePipePoint {
                pipe_id: "a".into(),
                index: 1,
            },
        );
        assert_eq!(
            points(&d, "a"),
            vec![(0.0, 0.0, 300.0), (2000.0, 0.0, 300.0)]
        );
        assert_eq!(r.diff.summary, "Remove bend: changed 1 pipe");
        let (code, message, ids) = refused(
            &d,
            Command::RemovePipePoint {
                pipe_id: "a".into(),
                index: 0,
            },
        );
        assert_eq!(code, "pipe_too_short");
        assert_eq!(message, "Cold water pipe 20 mm has only two points, and a pipe needs at least two. Delete the pipe instead.");
        assert_eq!(ids, vec!["a".to_string()]);
        // Removing an end shortens the run.
        apply(
            &mut d,
            Command::InsertPipePoint {
                pipe_id: "a".into(),
                segment: 0,
                at: v3(1000.0, 0.0, 300.0),
            },
        );
        let r = apply(
            &mut d,
            Command::RemovePipePoint {
                pipe_id: "a".into(),
                index: 2,
            },
        );
        assert_eq!(
            points(&d, "a"),
            vec![(0.0, 0.0, 300.0), (1000.0, 0.0, 300.0)]
        );
        assert_eq!(r.diff.summary, "Remove pipe end: changed 1 pipe");
    }

    // ----------------------------------------------------------- errors

    #[test]
    fn bad_edits_are_refused_with_a_code_and_the_pipe() {
        let mut p = project(&[
            (
                "a",
                Cold,
                20.0,
                &[
                    (0.0, 0.0, 300.0),
                    (1000.0, 0.0, 300.0),
                    (1000.0, 0.0, 1300.0),
                ],
            ),
            (
                "b",
                Cold,
                20.0,
                &[(1000.0, 0.0, 1300.0), (2000.0, 0.0, 1300.0)],
            ),
        ]);
        let level = p.levels[0].id.clone();
        p.elements.push(Element::Wall(Wall {
            id: "w".into(),
            level_id: level,
            start: Point { x: 0.0, y: 2000.0 },
            end: Point {
                x: 3000.0,
                y: 2000.0,
            },
            thickness_mm: 150.0,
            height_mm: None,
            material_id: None,
            role: WallRole::Building,
        }));
        let d = Document::new(p);
        let code = |c: Command| refused(&d, c).0;
        let a = |id: &str| vec![id.to_string()];

        assert_eq!(code(mv(&[], (0.0, 0.0, 1.0), true)), "no_pipe_points");
        assert_eq!(
            refused(&d, mv(&[("zz", &[])], (0.0, 0.0, 1.0), true)).0,
            "unknown_pipe"
        );
        assert_eq!(
            refused(&d, mv(&[("w", &[])], (0.0, 0.0, 1.0), true)),
            (
                "not_a_pipe".to_string(),
                "Element w is not a pipe.".to_string(),
                a("w")
            )
        );
        assert_eq!(
            refused(&d, mv(&[("a", &[3])], (0.0, 0.0, 1.0), true)).1,
            "Cold water pipe 20 mm has no point 3. Its points are numbered 0 to 2."
        );
        assert_eq!(
            code(mv(&[("a", &[3])], (0.0, 0.0, 1.0), true)),
            "bad_point_index"
        );
        assert_eq!(
            code(mv(&[("a", &[1, 1])], (0.0, 0.0, 1.0), true)),
            "duplicate_point"
        );
        assert_eq!(
            code(mv(&[("a", &[0]), ("a", &[1])], (0.0, 0.0, 1.0), true)),
            "duplicate_pipe"
        );
        assert_eq!(code(mv(&[("a", &[0])], (0.0, 0.0, 0.0), true)), "no_move");
        assert_eq!(
            code(mv(&[("a", &[0])], (f64::NAN, 0.0, 0.0), true)),
            "not_finite"
        );
        assert_eq!(
            code(mv(&[("a", &[0])], (0.0, f64::INFINITY, 0.0), true)),
            "not_finite"
        );
        // Too far is out of range, not invalid.
        assert_eq!(
            code(mv(&[("a", &[0])], (1e305, 0.0, 0.0), false)),
            "out_of_range"
        );
        // Moving a point onto its neighbor leaves a zero segment.
        assert_eq!(
            code(mv(&[("a", &[0])], (1000.0, 0.0, 0.0), false)),
            "pipe_zero_segment"
        );

        assert_eq!(
            code(length("a", 2, 500.0, PipeEnd::Start, true)),
            "bad_segment"
        );
        assert_eq!(
            code(length("a", 0, 5.0, PipeEnd::Start, true)),
            "pipe_segment_short"
        );
        assert_eq!(
            refused(&d, length("a", 0, 5.0, PipeEnd::Start, true)).1,
            "A pipe segment must be at least 10 mm long. This one would be 5 mm."
        );
        assert_eq!(
            code(length("a", 0, f64::NAN, PipeEnd::Start, true)),
            "not_finite"
        );
        assert_eq!(
            code(length("w", 0, 500.0, PipeEnd::Start, true)),
            "not_a_pipe"
        );

        assert_eq!(
            code(slope("a", 0, 0, 150.0, PipeEnd::Start, true)),
            "bad_slope"
        );
        assert_eq!(
            refused(&d, slope("a", 0, 0, -101.0, PipeEnd::Start, true)).1,
            "A pipe slope must be between -100 and 100 percent, got -101."
        );
        assert_eq!(
            code(slope("a", 0, 0, f64::NAN, PipeEnd::Start, true)),
            "not_finite"
        );
        assert_eq!(
            code(slope("a", 1, 0, 2.0, PipeEnd::Start, true)),
            "bad_segment"
        );
        assert_eq!(
            code(slope("a", 0, 2, 2.0, PipeEnd::Start, true)),
            "bad_segment"
        );
        assert_eq!(
            refused(&d, slope("a", 1, 1, 2.0, PipeEnd::Start, true)),
            (
                "no_slope_segment".to_string(),
                "Segment 1 of Cold water pipe 20 mm is vertical or steeper than 45 degrees, so there is no slope to set."
                    .to_string(),
                a("a")
            )
        );

        let bend = |segment: u32, at: Vec3| Command::InsertPipePoint {
            pipe_id: "a".into(),
            segment,
            at,
        };
        assert_eq!(code(bend(0, v3(1000.0, 0.0, 300.4))), "bend_on_point");
        assert_eq!(code(bend(5, v3(500.0, 0.0, 300.0))), "bad_segment");
        assert_eq!(code(bend(0, v3(500.0, f64::NAN, 300.0))), "not_finite");
        assert_eq!(
            code(Command::RemovePipePoint {
                pipe_id: "a".into(),
                index: 9
            }),
            "bad_point_index"
        );
        assert_eq!(
            code(Command::RemovePipePoint {
                pipe_id: "w".into(),
                index: 0
            }),
            "not_a_pipe"
        );
    }

    #[test]
    fn a_locked_layer_stops_the_edit_and_the_runs_that_would_follow() {
        let mut p = project(&[
            (
                "cold",
                Cold,
                20.0,
                &[(0.0, 0.0, 300.0), (1000.0, 0.0, 300.0)],
            ),
            (
                "drain",
                Drainage,
                50.0,
                &[(0.0, 500.0, -300.0), (1000.0, 500.0, -320.0)],
            ),
            (
                "vent",
                Vent,
                50.0,
                &[(1000.0, 500.0, -320.0), (1000.0, 500.0, 3000.0)],
            ),
        ]);
        for l in p.layers.iter_mut() {
            if l.key == LayerKey::ColdWater || l.key == LayerKey::Vent {
                l.locked = true;
            }
        }
        let d = Document::new(p);
        let (code, message, ids) = refused(&d, mv(&[("cold", &[])], (0.0, 0.0, 100.0), true));
        assert_eq!(code, "layer_locked");
        assert!(message.contains("Cold water"), "{message}");
        assert_eq!(ids, vec!["cold".to_string()]);
        // The drain is free, but the vent on it would follow.
        let (code, _, ids) = refused(&d, mv(&[("drain", &[1])], (0.0, 0.0, -50.0), true));
        assert_eq!(
            (code.as_str(), ids),
            ("layer_locked", vec!["vent".to_string()])
        );
        // Breaking the join is allowed.
        assert!(d
            .preview(&mv(&[("drain", &[1])], (0.0, 0.0, -50.0), false))
            .is_ok());
    }

    // ------------------------------------------------------------ batch

    #[test]
    fn a_riser_is_one_batch_and_one_undo_step() {
        let mut d = doc(&[("a", Cold, 20.0, &[(0.0, 0.0, 300.0), (2000.0, 0.0, 300.0)])]);
        let before = d.project().clone();
        // Add a riser: two bends on one plan point, then lift the far side.
        let r = apply(
            &mut d,
            Command::Batch {
                label: "Add riser".into(),
                commands: vec![
                    Command::InsertPipePoint {
                        pipe_id: "a".into(),
                        segment: 0,
                        at: v3(1000.0, 0.0, 300.0),
                    },
                    Command::InsertPipePoint {
                        pipe_id: "a".into(),
                        segment: 1,
                        at: v3(1000.0, 0.0, 800.0),
                    },
                    mv(&[("a", &[3])], (0.0, 0.0, 500.0), true),
                ],
            },
        );
        assert_eq!(
            points(&d, "a"),
            vec![
                (0.0, 0.0, 300.0),
                (1000.0, 0.0, 300.0),
                (1000.0, 0.0, 800.0),
                (2000.0, 0.0, 800.0)
            ]
        );
        // The batch keeps the step's own sentence after the diff.
        assert_eq!(r.diff.summary, "Add riser: changed 1 pipe; Moved 1 pipe point 500 mm up");
        assert_eq!(d.history().undo.len(), 1);
        d.undo().unwrap();
        assert_eq!(d.project().elements, before.elements);
        // A failing step undoes the whole batch.
        let e = d
            .preview(&Command::Batch {
                label: "x".into(),
                commands: vec![
                    Command::InsertPipePoint {
                        pipe_id: "a".into(),
                        segment: 0,
                        at: v3(1000.0, 0.0, 300.0),
                    },
                    Command::RemovePipePoint {
                        pipe_id: "a".into(),
                        index: 7,
                    },
                ],
            })
            .unwrap_err();
        assert!(e.to_string().starts_with("Step 2 of 2: "), "{e}");
    }

    // ------------------------------------------------------------ scope

    #[test]
    fn the_selection_limits_the_runs_an_edit_names_not_the_ones_that_follow() {
        let p = project(&[
            ("a", Cold, 20.0, &[(0.0, 0.0, 300.0), (1000.0, 0.0, 300.0)]),
            (
                "b",
                Cold,
                20.0,
                &[(1000.0, 0.0, 300.0), (1000.0, 1000.0, 300.0)],
            ),
        ]);
        let derived = compute_derived(&p);
        let only_a = vec!["a".to_string()];
        let check = |c: &Command| scope::check(&p, &derived, &only_a, c);
        assert!(
            check(&mv(&[("a", &[1])], (0.0, 0.0, 100.0), true)).is_ok(),
            "b follows as a consequence"
        );
        assert!(check(&length("a", 0, 800.0, PipeEnd::Start, true)).is_ok());
        assert!(check(&slope("a", 0, 0, 1.0, PipeEnd::Start, true)).is_ok());
        assert!(check(&Command::InsertPipePoint {
            pipe_id: "a".into(),
            segment: 0,
            at: v3(500.0, 0.0, 600.0)
        })
        .is_ok());
        let outside = [
            mv(&[("a", &[1]), ("b", &[1])], (0.0, 0.0, 100.0), true),
            length("b", 0, 800.0, PipeEnd::Start, true),
            slope("b", 0, 0, 1.0, PipeEnd::Start, true),
            Command::InsertPipePoint {
                pipe_id: "b".into(),
                segment: 0,
                at: v3(1000.0, 500.0, 600.0),
            },
            Command::RemovePipePoint {
                pipe_id: "b".into(),
                index: 0,
            },
        ];
        for c in &outside {
            match check(c) {
                Err(CoreError::Invalid {
                    code,
                    element_ids,
                    message,
                }) => {
                    assert_eq!(code, "out_of_scope");
                    assert_eq!(element_ids, vec!["b".to_string()]);
                    assert!(
                        message.starts_with("Cold water pipe 20 mm is outside"),
                        "{message}"
                    );
                }
                other => panic!("{c:?}: {other:?}"),
            }
        }
    }
}
