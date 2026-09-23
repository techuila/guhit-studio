//! Pipes (docs/CONTRACT.md, "Pipes"): fittings, penetrations, the take-off
//! and the pipe review items.
//!
//! Guhit coordinates pipes. It never sizes them, runs hydraulics or claims
//! code compliance (DECISIONS D19). Every finding is a suggestion; plumbing
//! plans are signed by a registered Master Plumber (RA 1378).
//!
//! Rules:
//! - Elbow: an interior point where a run turns by more than 1 degree, or the
//!   ends of two runs meeting at an angle.
//! - Tee: the end of one run touching another run's body (3D distance up to
//!   the larger radius) away from that run's ends. Three or more run ends at
//!   one point are a tee too: the two ends that line up best are the run,
//!   every other end is a branch off it. A branch landing where the other run
//!   bends is one tee, with no elbow there.
//! - Runs only join when they can carry the same flow: the same system, or
//!   drainage and vent. A cold water end resting on a hot water pipe is not a
//!   fitting, it is a clash.
//! - Penetrations: `slab`, `wall` and `roof`, one per crossing (see
//!   `penetrations`).
//!
//! Heights are handled as absolute elevations (level elevation plus z), so
//! runs on different levels meet and clash correctly. Everything reported
//! uses z above the floor of the element's own level. Output order is element
//! order, then along the run. No hash maps, no randomness.

use std::collections::BTreeMap;

use guhit_model::*;

use crate::geom::{bbox, dist, dot, perp_left, point_in_polygon, pt, signed_area, sub, unit};
use crate::issues::{issue, openings_of_face};
use crate::rooms::{face, FaceRef};
use crate::topo::{Analysis, WallTopo};
use crate::validate::wall_height;

/// A run that turns by more than this at a point has an elbow there.
const ELBOW_MIN_DEG: f64 = 1.0;
/// Drainage segments shorter than this are stubs and fittings, not runs, and
/// are not checked for fall.
const SLOPE_MIN_LENGTH_MM: f64 = 300.0;
/// Runs meet within their larger radius, and never closer than this.
const MIN_REACH_MM: f64 = 1.0;
/// Slab depth the 3D view draws (`src/viewer3d/scene/buildScene.ts`): the
/// plinth under the lowest level, a suspended slab under the others.
const SLAB_LOWEST_MM: f64 = 150.0;
const SLAB_UPPER_MM: f64 = 200.0;
/// A body closer than its radius by less than this only touches.
const TOUCH_MM: f64 = 1e-6;
/// Without a closed footprint the 3D view roofs the walls of the top level
/// when they span more than this both ways.
const FALLBACK_ROOF_SPAN_MM: f64 = 1000.0;
/// Pipes that are joined may overlap this far past their radii near the join.
const JOIN_SLACK_MM: f64 = 1.0;

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

fn cross3(a: Vec3, b: Vec3) -> Vec3 {
    v3(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
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

fn unit3(a: Vec3) -> Option<Vec3> {
    let l = len3(a);
    if l.is_finite() && l > 1e-12 {
        Some(mul3(a, 1.0 / l))
    } else {
        None
    }
}

fn finite3(a: Vec3) -> bool {
    a.x.is_finite() && a.y.is_finite() && a.z.is_finite()
}

fn plan(a: Vec3) -> Point {
    pt(a.x, a.y)
}

/// Angle between two directions in degrees: 0 when they point the same way,
/// 180 when they are opposite.
fn angle_between(a: Vec3, b: Vec3) -> f64 {
    let c = len3(cross3(a, b));
    let d = dot3(a, b);
    if c == 0.0 && d == 0.0 {
        return 0.0;
    }
    c.atan2(d).to_degrees()
}

/// Angle between two lines in degrees, 0 to 90.
fn line_angle(a: Vec3, b: Vec3) -> f64 {
    let t = angle_between(a, b);
    t.min(180.0 - t)
}

fn round_to(v: f64, scale: f64) -> f64 {
    let r = (v * scale).round() / scale;
    if r == 0.0 {
        0.0
    } else {
        r
    }
}

/// Derived positions and angles are kept to a millionth, so float dust such
/// as 6499.999999999999 never reaches the UI or a fixture.
fn tidy(v: f64) -> f64 {
    round_to(v, 1.0e6)
}

fn tidy3(a: Vec3) -> Vec3 {
    v3(tidy(a.x), tidy(a.y), tidy(a.z))
}

fn tidy_dir(a: Vec3) -> Vec3 {
    v3(
        round_to(a.x, 1.0e9),
        round_to(a.y, 1.0e9),
        round_to(a.z, 1.0e9),
    )
}

/// Axis-aligned box, for cheap rejection before the exact tests.
#[derive(Clone, Copy)]
struct Aabb {
    lo: Vec3,
    hi: Vec3,
}

impl Aabb {
    fn of_points(points: &[Vec3], pad: f64) -> Self {
        let mut lo = v3(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut hi = v3(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in points {
            lo = v3(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
            hi = v3(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
        }
        Self {
            lo: v3(lo.x - pad, lo.y - pad, lo.z - pad),
            hi: v3(hi.x + pad, hi.y + pad, hi.z + pad),
        }
    }

    fn overlaps(&self, o: &Aabb) -> bool {
        self.lo.x <= o.hi.x
            && o.lo.x <= self.hi.x
            && self.lo.y <= o.hi.y
            && o.lo.y <= self.hi.y
            && self.lo.z <= o.hi.z
            && o.lo.z <= self.hi.z
    }
}

/// Closest points of segments p1-q1 and p2-q2, and their distance.
/// Real-Time Collision Detection (Ericson), 5.1.9.
fn closest_segments(p1: Vec3, q1: Vec3, p2: Vec3, q2: Vec3) -> (Vec3, Vec3, f64) {
    let d1 = sub3(q1, p1);
    let d2 = sub3(q2, p2);
    let r = sub3(p1, p2);
    let a = dot3(d1, d1);
    let e = dot3(d2, d2);
    let f = dot3(d2, r);
    let tiny = 1e-12;
    let (s, t) = if a <= tiny && e <= tiny {
        (0.0, 0.0)
    } else if a <= tiny {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = dot3(d1, r);
        if e <= tiny {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = dot3(d1, d2);
            let denom = a * e - b * b;
            let mut s = if denom > 1e-12 * a * e {
                ((b * f - c * e) / denom).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
            (s, t)
        }
    };
    let c1 = add3(p1, mul3(d1, s));
    let c2 = add3(p2, mul3(d2, t));
    (c1, c2, dist3(c1, c2))
}

/// Where along segment a-b a pipe body of radius `r` is inside a convex
/// solid, given the distance from a point to that solid: the parameter range
/// (0 at a, 1 at b), or None when the body only touches it or misses it.
/// The distance to a convex solid is convex along a line, so a golden
/// section search finds the closest approach and bisection the entry and exit.
fn body_hit(a: Vec3, b: Vec3, r: f64, distance: &dyn Fn(Vec3) -> f64) -> Option<(f64, f64)> {
    let f = |t: f64| distance(lerp3(a, b, t)) - r;
    const G: f64 = 0.381_966_011_250_105_2;
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    let mut x1 = lo + G * (hi - lo);
    let mut x2 = hi - G * (hi - lo);
    let mut f1 = f(x1);
    let mut f2 = f(x2);
    for _ in 0..90 {
        if f1 <= f2 {
            hi = x2;
            x2 = x1;
            f2 = f1;
            x1 = lo + G * (hi - lo);
            f1 = f(x1);
        } else {
            lo = x1;
            x1 = x2;
            f1 = f2;
            x2 = hi - G * (hi - lo);
            f2 = f(x2);
        }
    }
    let mut tm = 0.5 * (lo + hi);
    let mut fm = f(tm);
    for t in [0.0, 1.0] {
        let v = f(t);
        if v < fm {
            tm = t;
            fm = v;
        }
    }
    if fm >= -TOUCH_MM {
        return None;
    }
    let edge = |mut inside: f64, mut outside: f64| {
        for _ in 0..60 {
            let m = 0.5 * (inside + outside);
            if f(m) < 0.0 {
                inside = m;
            } else {
                outside = m;
            }
        }
        inside
    };
    let t_in = if f(0.0) < 0.0 { 0.0 } else { edge(tm, 0.0) };
    let t_out = if f(1.0) < 0.0 { 1.0 } else { edge(tm, 1.0) };
    Some((t_in, t_out))
}

// ------------------------------------------------------------ polygons

/// Cleaned, counter-clockwise copy of a plan polygon, the way the 3D view
/// reads a footprint (`ensureCCW` in `src/viewer3d/geom/polygon.ts`). Empty
/// when it is not a polygon.
fn ensure_ccw(poly: &[Point]) -> Vec<Point> {
    let tol = 1e-4;
    let same = |a: Point, b: Point| (a.x - b.x).abs() < tol && (a.y - b.y).abs() < tol;
    let mut out: Vec<Point> = vec![];
    for p in poly {
        if !(p.x.is_finite() && p.y.is_finite()) {
            continue;
        }
        if out.last().map(|l| same(*l, *p)).unwrap_or(false) {
            continue;
        }
        out.push(*p);
    }
    while out.len() > 1 && same(out[0], out[out.len() - 1]) {
        out.pop();
    }
    if out.len() < 3 {
        return vec![];
    }
    let area = signed_area(&out);
    if area.abs() < 1e-6 {
        return vec![];
    }
    if area < 0.0 {
        out.reverse();
    }
    out
}

/// Mitred outward offset, the 3D view's `offsetPolygon`: the roof outline is
/// the footprint grown by the overhang.
fn offset_polygon(poly_in: &[Point], d: f64) -> Vec<Point> {
    let poly = ensure_ccw(poly_in);
    if poly.len() < 3 || d.abs() < 1e-6 {
        return poly;
    }
    let normalize = |v: Point| {
        let l = (v.x * v.x + v.y * v.y).sqrt();
        if l < 1e-6 {
            pt(0.0, 0.0)
        } else {
            pt(v.x / l, v.y / l)
        }
    };
    let n = poly.len();
    (0..n)
        .map(|i| {
            let prev = poly[(i + n - 1) % n];
            let cur = poly[i];
            let next = poly[(i + 1) % n];
            let d1 = normalize(sub(cur, prev));
            let d2 = normalize(sub(next, cur));
            // The outward normal of a counter-clockwise edge is its right side.
            let n1 = pt(d1.y, -d1.x);
            let n2 = pt(d2.y, -d2.x);
            let bis = normalize(pt(n1.x + n2.x, n1.y + n2.y));
            let cos = bis.x * n1.x + bis.y * n1.y;
            let s = d / cos.max(0.35);
            pt(cur.x + bis.x * s, cur.y + bis.y * s)
        })
        .collect()
}

// ----------------------------------------------------------------- words

fn system_label(s: PipeSystem) -> &'static str {
    match s {
        PipeSystem::ColdWater => "Cold water",
        PipeSystem::HotWater => "Hot water",
        PipeSystem::Drainage => "Drainage",
        PipeSystem::Vent => "Vent",
    }
}

fn system_noun(s: PipeSystem) -> &'static str {
    match s {
        PipeSystem::ColdWater => "cold water",
        PipeSystem::HotWater => "hot water",
        PipeSystem::Drainage => "drainage",
        PipeSystem::Vent => "vent",
    }
}

/// "20" or "12.5": a size without a trailing ".0".
fn size(mm: f64) -> String {
    if (mm - mm.round()).abs() < 1e-9 {
        format!("{mm:.0}")
    } else {
        format!("{mm:.1}")
    }
}

/// "2.10 m": meters with two decimals.
fn meters(mm: f64) -> String {
    format!("{:.2} m", round_to(mm / 1000.0, 100.0))
}

/// "1.50 m above the floor" or "0.30 m below the floor".
fn height_phrase(z: f64) -> String {
    if z < -5.0 {
        format!("{} below the floor", meters(-z))
    } else {
        format!("{} above the floor", meters(z.max(0.0)))
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// How a pipe is called in review items: its name when it has one, otherwise
/// its system and size, for example "Cold water pipe 20 mm".
pub fn pipe_name(p: &Pipe) -> String {
    let name = p.name.trim();
    if name.is_empty() {
        format!("{} pipe {} mm", system_label(p.system), size(p.diameter_mm))
    } else {
        name.to_string()
    }
}

/// `pipe_name` for the start of a sentence.
fn name_start(p: &Pipe) -> String {
    capitalize(&pipe_name(p))
}

/// `pipe_name` inside a sentence.
fn name_mid(p: &Pipe) -> String {
    if p.name.trim().is_empty() {
        format!(
            "the {} pipe {} mm",
            system_noun(p.system),
            size(p.diameter_mm)
        )
    } else {
        p.name.trim().to_string()
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

// ------------------------------------------------------------------ runs

/// Floor elevation and floor-to-floor height of a level. An element whose
/// level is gone is read as standing on project zero.
fn level_of(project: &Project, id: &str) -> (f64, f64) {
    project
        .levels
        .iter()
        .find(|l| l.id == id)
        .map(|l| (l.elevation_mm, l.height_mm))
        .filter(|(e, h)| e.is_finite() && h.is_finite())
        .unwrap_or((0.0, defaults::DEFAULT_LEVEL_HEIGHT_MM))
}

/// One pipe, ready for the checks.
struct Run<'a> {
    pipe: &'a Pipe,
    /// Floor elevation of the pipe's level.
    floor: f64,
    radius: f64,
    /// Centerline with absolute heights, repeated points dropped.
    pts: Vec<Vec3>,
    /// Length along the run at each point.
    along: Vec<f64>,
}

struct Closest {
    dist: f64,
    point: Vec3,
    /// Direction of the segment the point is on.
    dir: Vec3,
    along: f64,
}

impl<'a> Run<'a> {
    /// None for a pipe the checks cannot use (bad numbers, fewer than two
    /// distinct points). Validation keeps those out of a project; this keeps
    /// an old or hand-edited file from breaking the derived data.
    fn new(project: &Project, pipe: &'a Pipe) -> Option<Self> {
        if !(pipe.diameter_mm.is_finite() && pipe.diameter_mm > 0.0) {
            return None;
        }
        let (floor, _) = level_of(project, &pipe.level_id);
        let mut pts: Vec<Vec3> = vec![];
        for p in &pipe.points {
            if !finite3(*p) {
                return None;
            }
            let q = v3(p.x, p.y, p.z + floor);
            if pts.last().map(|l| dist3(*l, q) < 1e-6).unwrap_or(false) {
                continue;
            }
            pts.push(q);
        }
        if pts.len() < 2 {
            return None;
        }
        let mut along = vec![0.0];
        for w in pts.windows(2) {
            along.push(along[along.len() - 1] + dist3(w[0], w[1]));
        }
        Some(Self {
            pipe,
            floor,
            radius: pipe.diameter_mm / 2.0,
            pts,
            along,
        })
    }

    fn length(&self) -> f64 {
        self.along[self.along.len() - 1]
    }

    fn first(&self) -> Vec3 {
        self.pts[0]
    }

    fn last(&self) -> Vec3 {
        self.pts[self.pts.len() - 1]
    }

    fn end(&self, last: bool) -> Vec3 {
        if last {
            self.last()
        } else {
            self.first()
        }
    }

    /// Direction from an end into the run.
    fn away(&self, last: bool) -> Vec3 {
        let n = self.pts.len();
        if last {
            sub3(self.pts[n - 2], self.pts[n - 1])
        } else {
            sub3(self.pts[1], self.pts[0])
        }
    }

    /// Absolute height to height above this run's floor.
    fn rel(&self, p: Vec3) -> Vec3 {
        v3(p.x, p.y, p.z - self.floor)
    }

    fn segments(&self) -> impl Iterator<Item = (usize, Vec3, Vec3)> + '_ {
        self.pts
            .windows(2)
            .enumerate()
            .map(|(i, w)| (i, w[0], w[1]))
    }

    /// The point of the centerline nearest to `p`. On a tie the earlier
    /// segment wins.
    fn closest(&self, p: Vec3) -> Closest {
        let mut best = Closest {
            dist: f64::INFINITY,
            point: self.pts[0],
            dir: self.away(false),
            along: 0.0,
        };
        for (i, a, b) in self.segments() {
            let d = sub3(b, a);
            let l2 = dot3(d, d);
            let t = if l2 > 0.0 {
                (dot3(sub3(p, a), d) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let q = lerp3(a, b, t);
            let dd = dist3(p, q);
            if dd < best.dist {
                best = Closest {
                    dist: dd,
                    point: q,
                    dir: d,
                    along: self.along[i] + t * l2.sqrt(),
                };
            }
        }
        best
    }

    fn fitting(
        &self,
        kind: FittingKind,
        branch: Option<&Id>,
        at: Vec3,
        diameter: f64,
        angle: f64,
    ) -> PipeFitting {
        PipeFitting {
            kind,
            pipe_id: self.pipe.id.clone(),
            branch_pipe_id: branch.cloned(),
            level_id: self.pipe.level_id.clone(),
            position: tidy3(self.rel(at)),
            diameter_mm: diameter,
            angle_deg: tidy(angle),
        }
    }
}

/// Runs that can be connected with a fitting: the same system, or drainage
/// and its vent.
fn can_join(a: PipeSystem, b: PipeSystem) -> bool {
    a == b
        || matches!(
            (a, b),
            (PipeSystem::Drainage, PipeSystem::Vent) | (PipeSystem::Vent, PipeSystem::Drainage)
        )
}

/// How close an end must be to join another run.
fn reach(a: &Run, b: &Run) -> f64 {
    a.radius.max(b.radius).max(MIN_REACH_MM)
}

// -------------------------------------------------------------- fittings

struct Found {
    /// The run the fitting sits on (its `pipe_id`) and where along it.
    run: usize,
    along: f64,
    branch: Option<usize>,
    fitting: PipeFitting,
}

/// A place where two runs are connected, lower run index first.
type Join = (usize, usize, Vec3);

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

fn fittings(runs: &[Run]) -> (Vec<Found>, Vec<Join>) {
    let mut found: Vec<Found> = vec![];
    let mut joins: Vec<Join> = vec![];
    let join =
        |joins: &mut Vec<Join>, a: usize, b: usize, p: Vec3| joins.push((a.min(b), a.max(b), p));

    // Elbows: a turn inside one run.
    for (ri, run) in runs.iter().enumerate() {
        for i in 1..run.pts.len() - 1 {
            let turn = angle_between(
                sub3(run.pts[i], run.pts[i - 1]),
                sub3(run.pts[i + 1], run.pts[i]),
            );
            if turn > ELBOW_MIN_DEG {
                found.push(Found {
                    run: ri,
                    along: run.along[i],
                    branch: None,
                    fitting: run.fitting(
                        FittingKind::Elbow,
                        None,
                        run.pts[i],
                        run.pipe.diameter_mm,
                        turn,
                    ),
                });
            }
        }
    }

    // Tees: an end resting on another run's body, away from that run's ends.
    let mut free: Vec<(usize, bool)> = vec![];
    for (ri, run) in runs.iter().enumerate() {
        for last in [false, true] {
            let p = run.end(last);
            let mut best: Option<(usize, Closest)> = None;
            for (oi, other) in runs.iter().enumerate() {
                if oi == ri || !can_join(run.pipe.system, other.pipe.system) {
                    continue;
                }
                let r = reach(run, other);
                // At one of its ends this is two ends meeting, handled below.
                if dist3(p, other.first()) <= r || dist3(p, other.last()) <= r {
                    continue;
                }
                let c = other.closest(p);
                if c.dist <= r && best.as_ref().map(|(_, b)| c.dist < b.dist).unwrap_or(true) {
                    best = Some((oi, c));
                }
            }
            match best {
                Some((oi, c)) => {
                    let main = &runs[oi];
                    found.push(Found {
                        run: oi,
                        along: c.along,
                        branch: Some(ri),
                        fitting: main.fitting(
                            FittingKind::Tee,
                            Some(&run.pipe.id),
                            c.point,
                            main.pipe.diameter_mm.max(run.pipe.diameter_mm),
                            line_angle(run.away(last), c.dir),
                        ),
                    });
                    join(&mut joins, ri, oi, c.point);
                    join(&mut joins, ri, oi, p);
                }
                None => free.push((ri, last)),
            }
        }
    }

    // Ends meeting ends. Ends of joinable runs within reach form one group.
    let n = free.len();
    let mut parent: Vec<usize> = (0..n).collect();
    for i in 0..n {
        for j in (i + 1)..n {
            let ((ra, la), (rb, lb)) = (free[i], free[j]);
            if ra == rb || !can_join(runs[ra].pipe.system, runs[rb].pipe.system) {
                continue;
            }
            if dist3(runs[ra].end(la), runs[rb].end(lb)) <= reach(&runs[ra], &runs[rb]) {
                let (x, y) = (find_root(&mut parent, i), find_root(&mut parent, j));
                if x != y {
                    parent[x.max(y)] = x.min(y);
                }
            }
        }
    }
    let mut groups: Vec<Vec<(usize, bool)>> = vec![];
    let mut slot: BTreeMap<usize, usize> = BTreeMap::new();
    for (i, end) in free.iter().enumerate() {
        let root = find_root(&mut parent, i);
        match slot.get(&root) {
            Some(k) => groups[*k].push(*end),
            None => {
                slot.insert(root, groups.len());
                groups.push(vec![*end]);
            }
        }
    }
    for ends in groups.iter().filter(|g| g.len() >= 2) {
        for a in 0..ends.len() {
            for b in (a + 1)..ends.len() {
                let ((ra, la), (rb, lb)) = (ends[a], ends[b]);
                if ra != rb {
                    join(&mut joins, ra, rb, runs[ra].end(la));
                    join(&mut joins, ra, rb, runs[rb].end(lb));
                }
            }
        }
        if ends.len() == 2 {
            // Two ends: straight is a plain coupling, anything else an elbow.
            let ((ra, la), (rb, lb)) = (ends[0], ends[1]);
            let turn = 180.0 - angle_between(runs[ra].away(la), runs[rb].away(lb));
            if turn > ELBOW_MIN_DEG {
                // The elbow sits on the run that arrives here, else the first.
                let (r, l) = if lb && !la { (rb, lb) } else { (ra, la) };
                let run = &runs[r];
                found.push(Found {
                    run: r,
                    along: if l { run.length() } else { 0.0 },
                    branch: None,
                    fitting: run.fitting(
                        FittingKind::Elbow,
                        None,
                        run.end(l),
                        runs[ra].pipe.diameter_mm.max(runs[rb].pipe.diameter_mm),
                        turn,
                    ),
                });
            }
            continue;
        }
        // Three or more ends: the pair of two runs that lines up best is the
        // run, every other end is a branch off it.
        let dir = |k: usize| unit3(runs[ends[k].0].away(ends[k].1)).unwrap_or(v3(0.0, 0.0, 0.0));
        let mut pair: Option<(usize, usize)> = None;
        let mut straightest = f64::INFINITY;
        for a in 0..ends.len() {
            for b in (a + 1)..ends.len() {
                if ends[a].0 == ends[b].0 {
                    continue;
                }
                let d = dot3(dir(a), dir(b));
                if d < straightest - 1e-12 {
                    straightest = d;
                    pair = Some((a, b));
                }
            }
        }
        // Linked ends always belong to two runs, so a pair exists.
        let Some((pa, pb)) = pair else { continue };
        let rank = |k: usize| {
            let (r, l) = ends[k];
            (runs[r].pipe.diameter_mm, l)
        };
        let (da, db) = (rank(pa), rank(pb));
        let (main_k, other_k) = if db.0 > da.0 || (db.0 == da.0 && db.1 && !da.1) {
            (pb, pa)
        } else {
            (pa, pb)
        };
        let (rm, lm) = ends[main_k];
        for (k, &(rk, lk)) in ends.iter().enumerate() {
            if k == pa || k == pb {
                continue;
            }
            // The other end of a loop that closes here branches off the
            // other run of the pair, never off itself.
            let (ro, lo) = if rk == rm { ends[other_k] } else { (rm, lm) };
            let on = &runs[ro];
            found.push(Found {
                run: ro,
                along: if lo { on.length() } else { 0.0 },
                branch: Some(rk),
                fitting: on.fitting(
                    FittingKind::Tee,
                    Some(&runs[rk].pipe.id),
                    on.end(lo),
                    on.pipe.diameter_mm.max(runs[rk].pipe.diameter_mm),
                    line_angle(runs[rk].away(lk), on.away(lo)),
                ),
            });
        }
    }

    // A branch landing where a run bends is one tee fitting, not an elbow
    // and a tee: the run passes through two of the tee's ports.
    let landed: Vec<(usize, f64, f64)> = found
        .iter()
        .filter(|f| f.fitting.kind == FittingKind::Tee)
        .filter_map(|f| f.branch.map(|b| (f.run, f.along, reach(&runs[f.run], &runs[b]))))
        .collect();
    found.retain(|f| {
        f.fitting.kind != FittingKind::Elbow
            || !landed
                .iter()
                .any(|&(r, along, near)| r == f.run && (f.along - along).abs() <= near)
    });

    found.sort_by(|a, b| {
        a.run
            .cmp(&b.run)
            .then(a.along.total_cmp(&b.along))
            .then((a.fitting.kind == FittingKind::Tee).cmp(&(b.fitting.kind == FittingKind::Tee)))
            .then(a.branch.cmp(&b.branch))
    });
    (found, joins)
}

// ------------------------------------------------------------- the house

/// A wall as the pipe checks see it.
struct WallBody<'a> {
    wall: &'a Wall,
    /// Plan outline with joins resolved.
    outline: &'a [Point],
    start: Point,
    dir: Point,
    normal: Point,
    half: f64,
    /// Absolute floor and top of the wall.
    floor: f64,
    top: f64,
    openings: Vec<&'a Opening>,
}

fn wall_bodies<'a>(
    project: &'a Project,
    walls: &BTreeMap<&str, &'a WallTopo>,
) -> Vec<WallBody<'a>> {
    let mut out = vec![];
    for el in &project.elements {
        let Element::Wall(w) = el else { continue };
        let Some(&topo) = walls.get(w.id.as_str()) else {
            continue;
        };
        let Some(dir) = unit(sub(w.end, w.start)) else {
            continue;
        };
        if topo.outline.len() < 3 || !w.thickness_mm.is_finite() {
            continue;
        }
        let (floor, _) = level_of(project, &w.level_id);
        let openings = project
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Opening(o) if o.wall_id == w.id => Some(o),
                _ => None,
            })
            .collect();
        out.push(WallBody {
            wall: w,
            outline: &topo.outline,
            start: w.start,
            dir,
            normal: perp_left(dir),
            half: w.thickness_mm / 2.0,
            floor,
            top: floor + wall_height(project, w),
            openings,
        });
    }
    out
}

/// A door or window opening: the hole through its wall.
struct Hole<'a> {
    opening: &'a Opening,
    start: Point,
    dir: Point,
    normal: Point,
    half: f64,
    /// Along the wall from its start.
    u0: f64,
    u1: f64,
    /// Absolute sill and head.
    z0: f64,
    z1: f64,
    bounds: Aabb,
}

impl Hole<'_> {
    fn distance(&self, p: Vec3) -> f64 {
        let q = sub(plan(p), self.start);
        let u = dot(q, self.dir);
        let v = dot(q, self.normal);
        let eu = (self.u0 - u).max(u - self.u1).max(0.0);
        let ev = (v.abs() - self.half).max(0.0);
        let ez = (self.z0 - p.z).max(p.z - self.z1).max(0.0);
        (eu * eu + ev * ev + ez * ez).sqrt()
    }
}

fn holes<'a>(project: &'a Project, bodies: &[WallBody<'a>]) -> Vec<Hole<'a>> {
    let mut out = vec![];
    for el in &project.elements {
        let Element::Opening(o) = el else { continue };
        let Some(w) = bodies.iter().find(|b| b.wall.id == o.wall_id) else {
            continue;
        };
        let (u0, u1) = (
            o.offset_mm - o.width_mm / 2.0,
            o.offset_mm + o.width_mm / 2.0,
        );
        let (z0, z1) = (w.floor + o.sill_mm, w.floor + o.sill_mm + o.height_mm);
        if ![u0, u1, z0, z1].iter().all(|v| v.is_finite()) {
            continue;
        }
        let corner = |u: f64, v: f64| {
            let p = pt(
                w.start.x + w.dir.x * u + w.normal.x * v,
                w.start.y + w.dir.y * u + w.normal.y * v,
            );
            v3(p.x, p.y, z0)
        };
        let mut pts = vec![
            corner(u0, -w.half),
            corner(u0, w.half),
            corner(u1, -w.half),
            corner(u1, w.half),
        ];
        pts.push(v3(pts[0].x, pts[0].y, z1));
        out.push(Hole {
            opening: o,
            start: w.start,
            dir: w.dir,
            normal: w.normal,
            half: w.half,
            u0,
            u1,
            z0,
            z1,
            bounds: Aabb::of_points(&pts, 0.0),
        });
    }
    out
}

/// A column as a solid from its floor to the level height.
struct Solid<'a> {
    column: &'a Column,
    cos: f64,
    sin: f64,
    z0: f64,
    z1: f64,
    bounds: Aabb,
}

impl Solid<'_> {
    fn distance(&self, p: Vec3) -> f64 {
        let c = self.column;
        let (dx, dy) = (p.x - c.center.x, p.y - c.center.y);
        let ez = (self.z0 - p.z).max(p.z - self.z1).max(0.0);
        let e = match c.shape {
            ColumnShape::Round => ((dx * dx + dy * dy).sqrt() - c.width_mm / 2.0).max(0.0),
            ColumnShape::Rect => {
                // Into the column's own axes.
                let lx = dx * self.cos + dy * self.sin;
                let ly = -dx * self.sin + dy * self.cos;
                let ex = (lx.abs() - c.width_mm / 2.0).max(0.0);
                let ey = (ly.abs() - c.depth_mm / 2.0).max(0.0);
                (ex * ex + ey * ey).sqrt()
            }
        };
        (e * e + ez * ez).sqrt()
    }
}

fn solids(project: &Project) -> Vec<Solid<'_>> {
    let mut out = vec![];
    for el in &project.elements {
        let Element::Column(c) = el else { continue };
        if ![
            c.center.x,
            c.center.y,
            c.width_mm,
            c.depth_mm,
            c.rotation_deg,
        ]
        .iter()
        .all(|v| v.is_finite())
        {
            continue;
        }
        let (floor, height) = level_of(project, &c.level_id);
        let (sin, cos) = c.rotation_deg.to_radians().sin_cos();
        let reach = match c.shape {
            ColumnShape::Round => c.width_mm / 2.0,
            ColumnShape::Rect => (c.width_mm * c.width_mm + c.depth_mm * c.depth_mm).sqrt() / 2.0,
        };
        let bounds = Aabb::of_points(
            &[
                v3(c.center.x - reach, c.center.y - reach, floor),
                v3(c.center.x + reach, c.center.y + reach, floor + height),
            ],
            0.0,
        );
        out.push(Solid {
            column: c,
            cos,
            sin,
            z0: floor,
            z1: floor + height,
            bounds,
        });
    }
    out
}

/// The roof over one building of the top level, as the 3D view draws it
/// (`roofProfile` and `buildRoofMesh` in `src/viewer3d/geom/roofMesh.ts`).
struct RoofShape {
    kind: RoofKind,
    /// Footprint grown by the overhang: where the roof exists.
    outline: Vec<Point>,
    /// Wall top of the top level, above its floor: the underside at the
    /// footprint edge.
    base: f64,
    /// Vertical thickness.
    vertical: f64,
    tan: f64,
    /// True when the height depends on x, false when on y.
    x_across: bool,
    lo: f64,
    mid: f64,
    half: f64,
}

impl RoofShape {
    fn across(&self, p: Point) -> f64 {
        if self.x_across {
            p.x
        } else {
            p.y
        }
    }

    /// Height of the underside above the top level floor.
    fn under(&self, p: Point) -> f64 {
        match self.kind {
            RoofKind::Shed => self.base + (self.across(p) - self.lo) * self.tan,
            RoofKind::Gable => {
                self.base + (self.half - (self.across(p) - self.mid).abs()) * self.tan
            }
            RoofKind::Flat | RoofKind::None => self.base,
        }
    }

    /// Where segment a-b (heights above the top level floor) goes from below
    /// the underside to at or above it, or back, inside the roof outline:
    /// the crossing parameter, and the point where the centerline meets the
    /// middle of the roof there.
    fn crossings(&self, a: Vec3, b: Vec3) -> Vec<(f64, Vec3)> {
        let mut cuts = vec![0.0];
        if self.kind == RoofKind::Gable {
            let (ca, cb) = (self.across(plan(a)), self.across(plan(b)));
            if (ca - self.mid) * (cb - self.mid) < 0.0 {
                cuts.push((self.mid - ca) / (cb - ca));
            }
        }
        cuts.push(1.0);
        let f = |t: f64| {
            let p = lerp3(a, b, t);
            p.z - self.under(plan(p))
        };
        let mut out = vec![];
        for w in cuts.windows(2) {
            let (t0, t1) = (w[0], w[1]);
            let (f0, f1) = (f(t0), f(t1));
            if (f0 < 0.0) == (f1 < 0.0) {
                continue;
            }
            // Linear between the cuts: the underside is one plane there.
            let at = |target: f64| t0 + (t1 - t0) * (target - f0) / (f1 - f0);
            let tu = at(0.0);
            if !point_in_polygon(plan(lerp3(a, b, tu)), &self.outline) {
                continue;
            }
            let tm = at(self.vertical / 2.0).clamp(0.0, 1.0);
            out.push((tu, lerp3(a, b, tm)));
        }
        out
    }
}

/// Levels sorted the way the 3D view sorts them: by elevation, stable.
fn levels_by_elevation(project: &Project) -> Vec<&Level> {
    let mut levels: Vec<&Level> = project.levels.iter().collect();
    levels.sort_by(|a, b| a.elevation_mm.total_cmp(&b.elevation_mm));
    levels
}

/// The roofs the 3D view draws: one per building on the top level, or one
/// over the walls of the top level when it has no closed footprint yet.
fn roof_shapes(
    project: &Project,
    analysis: &Analysis,
    walls: &BTreeMap<&str, &WallTopo>,
) -> Vec<RoofShape> {
    let roof = &project.roof;
    if roof.kind == RoofKind::None {
        return vec![];
    }
    let Some(top) = levels_by_elevation(project).last().copied() else {
        return vec![];
    };
    let mut footprints: Vec<Vec<Point>> = analysis
        .level(&top.id)
        .map(|l| {
            l.footprints
                .iter()
                .map(|(p, _)| ensure_ccw(p))
                .filter(|p| p.len() >= 3)
                .collect()
        })
        .unwrap_or_default();
    if footprints.is_empty() {
        let mut pts: Vec<Point> = vec![];
        for el in &project.elements {
            let Element::Wall(w) = el else { continue };
            if w.level_id != top.id {
                continue;
            }
            match walls.get(w.id.as_str()) {
                Some(t) if t.outline.len() >= 3 => pts.extend(t.outline.iter().copied()),
                _ => pts.extend([w.start, w.end]),
            }
        }
        let pts: Vec<Point> = pts
            .into_iter()
            .filter(|p| p.x.is_finite() && p.y.is_finite())
            .collect();
        if !pts.is_empty() {
            let (lo, hi) = bbox(&pts);
            if hi.x - lo.x > FALLBACK_ROOF_SPAN_MM && hi.y - lo.y > FALLBACK_ROOF_SPAN_MM {
                footprints.push(vec![
                    pt(lo.x, lo.y),
                    pt(hi.x, lo.y),
                    pt(hi.x, hi.y),
                    pt(lo.x, hi.y),
                ]);
            }
        }
    }
    let pitch = if roof.pitch_deg.is_finite() {
        roof.pitch_deg.clamp(0.0, 60.0)
    } else {
        0.0
    };
    let thickness = roof.thickness_mm.max(10.0);
    let overhang = roof.overhang_mm.max(0.0);
    let mut out = vec![];
    for fp in footprints {
        let (lo, hi) = bbox(&fp);
        let (tan, vertical) = if roof.kind == RoofKind::Flat {
            (0.0, thickness)
        } else {
            let r = pitch.to_radians();
            (r.tan(), thickness / r.cos())
        };
        // Gable: the ridge runs along ridge_axis and the height depends on the
        // other axis. Shed: ridge_axis is the slope direction.
        let x_across = match roof.kind {
            RoofKind::Gable => roof.ridge_axis == Axis::Y,
            RoofKind::Shed => roof.ridge_axis == Axis::X,
            _ => false,
        };
        let (a, b) = if x_across { (lo.x, hi.x) } else { (lo.y, hi.y) };
        let outline = offset_polygon(&fp, overhang);
        if outline.len() < 3 {
            continue;
        }
        out.push(RoofShape {
            kind: roof.kind,
            outline,
            base: top.height_mm,
            vertical,
            tan,
            x_across,
            lo: a,
            mid: (a + b) / 2.0,
            half: (b - a) / 2.0,
        });
    }
    out
}

// ----------------------------------------------------------- penetrations

struct Pen {
    along: f64,
    pen: PipePenetration,
}

/// Every penetration, one per crossing:
/// - `slab`: a segment with one end at or above the floor (z >= 0) and the
///   other below it, crossing z = 0 inside a footprint of its level. Placed at
///   the middle of the slab the 3D view draws.
/// - `wall`: a segment that enters one long face of a wall and leaves the
///   other, both ends outside the wall's thickness, crossing the centerline
///   inside the wall's outline, between its floor and its top, and not inside
///   one of its openings. Chases along a wall and vertical segments never do.
/// - `roof`: a segment of a pipe on the top level that crosses the underside
///   of the roof the 3D view draws, inside its outline. Placed at the middle
///   of the roof.
fn penetrations(
    project: &Project,
    analysis: &Analysis,
    runs: &[Run],
    bodies: &[WallBody],
    roofs: &[RoofShape],
) -> Vec<Pen> {
    let levels = levels_by_elevation(project);
    let lowest = levels.first().map(|l| l.id.as_str());
    let top = levels.last().map(|l| l.id.as_str());
    let mut out: Vec<Pen> = vec![];
    for run in runs {
        let pipe = run.pipe;
        let footprints: Vec<&Vec<Point>> = analysis
            .level(&pipe.level_id)
            .map(|l| l.footprints.iter().map(|(p, _)| p).collect())
            .unwrap_or_default();
        let depth = if lowest == Some(pipe.level_id.as_str()) {
            SLAB_LOWEST_MM
        } else {
            SLAB_UPPER_MM
        };
        let on_top = top == Some(pipe.level_id.as_str());
        let mut mine: Vec<Pen> = vec![];
        let push = |mine: &mut Vec<Pen>,
                    i: usize,
                    t: f64,
                    kind: PenetrationKind,
                    host: Option<&Id>,
                    at: Vec3,
                    a: Vec3,
                    b: Vec3| {
            mine.push(Pen {
                along: run.along[i] + t * dist3(a, b),
                pen: PipePenetration {
                    kind,
                    pipe_id: pipe.id.clone(),
                    host_id: host.cloned(),
                    level_id: pipe.level_id.clone(),
                    position: tidy3(at),
                    direction: tidy_dir(unit3(sub3(b, a)).unwrap_or(v3(0.0, 0.0, 1.0))),
                    diameter_mm: pipe.diameter_mm,
                },
            });
        };
        for (i, a, b) in run.segments() {
            let (ar, br) = (run.rel(a), run.rel(b));

            // Slab.
            if (ar.z >= 0.0) != (br.z >= 0.0) {
                let t0 = ar.z / (ar.z - br.z);
                if footprints
                    .iter()
                    .any(|fp| point_in_polygon(plan(lerp3(ar, br, t0)), fp))
                {
                    let tm = ((ar.z + depth / 2.0) / (ar.z - br.z)).clamp(0.0, 1.0);
                    push(
                        &mut mine,
                        i,
                        t0,
                        PenetrationKind::Slab,
                        None,
                        lerp3(ar, br, tm),
                        a,
                        b,
                    );
                }
            }

            // Walls.
            if dist(plan(a), plan(b)) >= 1.0 {
                for w in bodies {
                    let va = dot(sub(plan(a), w.start), w.normal);
                    let vb = dot(sub(plan(b), w.start), w.normal);
                    let h = w.half;
                    if !((va <= -h && vb >= h) || (va >= h && vb <= -h)) {
                        continue;
                    }
                    let s = va / (va - vb);
                    let p0 = lerp3(a, b, s);
                    if !point_in_polygon(plan(p0), w.outline) || p0.z < w.floor || p0.z > w.top {
                        continue;
                    }
                    let z0 = p0.z - w.floor;
                    let u0 = dot(sub(plan(p0), w.start), w.dir);
                    let in_opening = w.openings.iter().any(|o| {
                        (u0 - o.offset_mm).abs() <= o.width_mm / 2.0
                            && z0 >= o.sill_mm
                            && z0 <= o.sill_mm + o.height_mm
                    });
                    if in_opening {
                        continue;
                    }
                    push(
                        &mut mine,
                        i,
                        s,
                        PenetrationKind::Wall,
                        Some(&w.wall.id),
                        run.rel(p0),
                        a,
                        b,
                    );
                }
            }

            // Roof.
            if on_top {
                for roof in roofs {
                    for (t, at) in roof.crossings(ar, br) {
                        push(&mut mine, i, t, PenetrationKind::Roof, None, at, a, b);
                    }
                }
            }
        }
        mine.sort_by(|x, y| x.along.total_cmp(&y.along));
        out.extend(mine);
    }
    out
}

// ---------------------------------------------------------------- take-off

fn takeoff(runs: &[Run]) -> (Vec<PipeTakeoffRow>, f64) {
    let mut groups: Vec<(PipeSystem, PipeMaterial, f64, f64, u32)> = vec![];
    for run in runs {
        let p = run.pipe;
        match groups.iter_mut().find(|g| {
            g.0 == p.system && g.1 == p.material && g.2.to_bits() == p.diameter_mm.to_bits()
        }) {
            Some(g) => {
                g.3 += run.length();
                g.4 += 1;
            }
            None => groups.push((p.system, p.material, p.diameter_mm, run.length(), 1)),
        }
    }
    groups.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.total_cmp(&b.2)));
    let rows = groups
        .iter()
        .map(|g| PipeTakeoffRow {
            system: g.0,
            material: g.1,
            diameter_mm: g.2,
            length_m: g.3.round() / 1000.0,
            run_count: g.4,
        })
        .collect();
    let total_mm: f64 = groups.iter().map(|g| g.3.round()).sum();
    (rows, total_mm / 1000.0)
}

// ------------------------------------------------------------ review items

fn located(mut i: Issue, at: Vec3) -> Issue {
    i.location = Some(tidy3(at));
    i
}

/// "the T&B door": an opening named after the smallest room it opens into.
fn opening_name(
    project: &Project,
    analysis: &Analysis,
    assigned: &BTreeMap<Id, FaceRef>,
    o: &Opening,
) -> String {
    let noun = match o.opening_type {
        OpeningType::Door => "door",
        OpeningType::Window => "window",
    };
    let mut best: Option<(f64, &str)> = None;
    for el in &project.elements {
        let Element::Room(r) = el else { continue };
        let Some(f) = assigned.get(&r.id) else {
            continue;
        };
        let f = face(analysis, *f);
        if !openings_of_face(project, f)
            .iter()
            .any(|(x, _)| x.id == o.id)
        {
            continue;
        }
        if best.map(|(area, _)| f.area_mm2 < area).unwrap_or(true) {
            best = Some((f.area_mm2, r.name.trim()));
        }
    }
    match best {
        Some((_, room)) if !room.is_empty() => format!("the {room} {noun}"),
        _ => format!("a {noun}"),
    }
}

/// The first place where a run's body is inside a solid: the middle of that
/// stretch, as an absolute point.
fn first_hit(run: &Run, bounds: &Aabb, distance: &dyn Fn(Vec3) -> f64) -> Option<Vec3> {
    for (_, a, b) in run.segments() {
        if !Aabb::of_points(&[a, b], run.radius).overlaps(bounds) {
            continue;
        }
        if let Some((t0, t1)) = body_hit(a, b, run.radius, distance) {
            return Some(lerp3(a, b, 0.5 * (t0 + t1)));
        }
    }
    None
}

fn across_openings(
    project: &Project,
    analysis: &Analysis,
    assigned: &BTreeMap<Id, FaceRef>,
    runs: &[Run],
    holes: &[Hole],
) -> Vec<Issue> {
    let mut out = vec![];
    for run in runs {
        let p = run.pipe;
        for hole in holes {
            let Some(at) = first_hit(run, &hole.bounds, &|q| hole.distance(q)) else {
                continue;
            };
            let at = run.rel(at);
            let o = hole.opening;
            let head = meters(o.sill_mm + o.height_mm);
            let way_out = match o.opening_type {
                OpeningType::Door => {
                    format!("Route it above the door head at {head} or under the slab.")
                }
                OpeningType::Window if o.sill_mm >= 100.0 => {
                    format!(
                        "Route it below the sill at {} or above the window head at {head}.",
                        meters(o.sill_mm)
                    )
                }
                OpeningType::Window => {
                    format!("Route it above the window head at {head} or under the slab.")
                }
            };
            let message = format!(
                "{} crosses {}. The {} mm {} line runs {} through the {} mm opening. {way_out}",
                name_start(p),
                opening_name(project, analysis, assigned, o),
                size(p.diameter_mm),
                system_noun(p.system),
                height_phrase(at.z),
                size(o.width_mm),
            );
            out.push(located(
                issue(
                    "pipe_across_opening",
                    Severity::Warning,
                    message,
                    vec![p.id.clone(), o.id.clone()],
                ),
                at,
            ));
        }
    }
    out
}

fn through_columns(runs: &[Run], solids: &[Solid]) -> Vec<Issue> {
    let mut out = vec![];
    for run in runs {
        let p = run.pipe;
        for solid in solids {
            let Some(at) = first_hit(run, &solid.bounds, &|q| solid.distance(q)) else {
                continue;
            };
            let at = run.rel(at);
            let c = solid.column;
            let column = match c.shape {
                ColumnShape::Rect => format!("{} x {} mm", size(c.width_mm), size(c.depth_mm)),
                ColumnShape::Round => format!("{} mm round", size(c.width_mm)),
            };
            let message = format!(
                "{} runs through a column. The {} mm {} line passes through the {column} column {}. Keep pipes out of columns: route it around the column or under the slab, or ask the structural engineer first.",
                name_start(p),
                size(p.diameter_mm),
                system_noun(p.system),
                height_phrase(at.z),
            );
            out.push(located(
                issue(
                    "pipe_through_column",
                    Severity::Warning,
                    message,
                    vec![p.id.clone(), c.id.clone()],
                ),
                at,
            ));
        }
    }
    out
}

fn pipe_crosses(runs: &[Run], joins: &[Join]) -> Vec<Issue> {
    let mut out = vec![];
    let boxes: Vec<Vec<Aabb>> = runs
        .iter()
        .map(|r| {
            r.segments()
                .map(|(_, a, b)| Aabb::of_points(&[a, b], r.radius))
                .collect()
        })
        .collect();
    // A box around each whole run rejects most pairs at once.
    let whole: Vec<Aabb> = runs
        .iter()
        .map(|r| Aabb::of_points(&r.pts, r.radius))
        .collect();
    let mut joined_at: BTreeMap<(usize, usize), Vec<Vec3>> = BTreeMap::new();
    for (a, b, p) in joins {
        joined_at.entry((*a, *b)).or_default().push(*p);
    }
    let none: Vec<Vec3> = vec![];
    for i in 0..runs.len() {
        for j in (i + 1)..runs.len() {
            if !whole[i].overlaps(&whole[j]) {
                continue;
            }
            // Reported and searched in id order, so the finding is the same
            // whichever run was drawn first.
            let (fi, si) = if runs[i].pipe.id <= runs[j].pipe.id {
                (i, j)
            } else {
                (j, i)
            };
            let (f, s) = (&runs[fi], &runs[si]);
            let limit = f.radius + s.radius - TOUCH_MM;
            let slack = f.radius + s.radius + JOIN_SLACK_MM;
            let joined = joined_at.get(&(i, j)).unwrap_or(&none);
            let mut hit: Option<Vec3> = None;
            'search: for (ka, a0, a1) in f.segments() {
                for (kb, b0, b1) in s.segments() {
                    if !boxes[fi][ka].overlaps(&boxes[si][kb]) {
                        continue;
                    }
                    let (ca, cb, d) = closest_segments(a0, a1, b0, b1);
                    if d >= limit {
                        continue;
                    }
                    // Where the runs are joined their bodies overlap by design.
                    if joined
                        .iter()
                        .any(|p| dist3(ca, *p) <= slack || dist3(cb, *p) <= slack)
                    {
                        continue;
                    }
                    hit = Some(lerp3(ca, cb, 0.5));
                    break 'search;
                }
            }
            let Some(at) = hit else { continue };
            let at = f.rel(at);
            let way_out = if can_join(f.pipe.system, s.pipe.system) {
                "Move one of them, or join them with a fitting if they connect."
            } else {
                "Move one of them so the lines clear each other."
            };
            let message = format!(
                "{} crosses {}. The lines touch {} where they are not joined. {way_out}",
                name_start(f.pipe),
                name_mid(s.pipe),
                height_phrase(at.z),
            );
            out.push(located(
                issue(
                    "pipes_cross",
                    Severity::Warning,
                    message,
                    vec![f.pipe.id.clone(), s.pipe.id.clone()],
                ),
                at,
            ));
        }
    }
    out
}

fn drain_slopes(runs: &[Run]) -> Vec<Issue> {
    let mut out = vec![];
    for run in runs
        .iter()
        .filter(|r| r.pipe.system == PipeSystem::Drainage)
    {
        let p = run.pipe;
        let min = defaults::drain_min_slope_pct(p.diameter_mm);
        let mut first: Option<(Vec3, Vec3)> = None;
        let mut others = 0;
        for (_, a, b) in run.segments() {
            let (a, b) = (run.rel(a), run.rel(b));
            let h = dist(plan(a), plan(b));
            let dz = b.z - a.z;
            // Short stubs and anything 45 degrees or steeper are not runs.
            if dist3(a, b) < SLOPE_MIN_LENGTH_MM || dz.abs() >= h {
                continue;
            }
            let pct = -dz / h * 100.0;
            if pct < min - 1e-9 {
                if first.is_none() {
                    first = Some((a, b));
                } else {
                    others += 1;
                }
            }
        }
        let Some((a, b)) = first else { continue };
        let length = meters(dist3(a, b));
        let dz = b.z - a.z;
        let pct = -dz / dist(plan(a), plan(b)) * 100.0;
        let mut message = if dz >= 0.5 {
            format!(
                "{} runs uphill. The {length} run rises {:.0} mm from its first point to its last, against the flow. Lower its far end, or reverse the run if it was drawn from the outlet.",
                name_start(p),
                dz,
            )
        } else if dz > -0.5 {
            format!(
                "{} has no fall. The {length} run is level, and the default for {} mm drainage is {} percent. Give it fall: start it higher or connect it lower.",
                name_start(p),
                size(p.diameter_mm),
                size(min),
            )
        } else {
            format!(
                "{} falls {:.1} percent. The {length} run drops {:.0} mm, under the {} percent default for {} mm drainage. Give it more fall: start it higher or connect it lower.",
                name_start(p),
                round_to(pct, 10.0),
                -dz,
                size(min),
                size(p.diameter_mm),
            )
        };
        if others > 0 {
            message.push_str(&format!(
                " {} of this run {} under the default too.",
                plural(others, "other segment", "other segments"),
                if others == 1 { "is" } else { "are" }
            ));
        }
        out.push(located(
            issue(
                "drain_slope_low",
                Severity::Warning,
                message,
                vec![p.id.clone()],
            ),
            lerp3(a, b, 0.5),
        ));
    }
    out
}

fn penetration_summary(pens: &[Pen]) -> Option<Issue> {
    if pens.is_empty() {
        return None;
    }
    let count = |k: PenetrationKind| pens.iter().filter(|p| p.pen.kind == k).count();
    let (slab, wall, roof) = (
        count(PenetrationKind::Slab),
        count(PenetrationKind::Wall),
        count(PenetrationKind::Roof),
    );
    let n = pens.len();
    let mut parts = vec![];
    if slab > 0 {
        parts.push(format!("{slab} through the floor slab"));
    }
    if wall > 0 {
        parts.push(if wall == 1 {
            "1 through a wall".to_string()
        } else {
            format!("{wall} through walls")
        });
    }
    if roof > 0 {
        parts.push(format!("{roof} through the roof"));
    }
    let sleeves = slab + wall;
    let need = match (sleeves > 0, roof > 0, n == 1) {
        (true, true, _) => "need sleeves or flashing",
        (true, false, true) => "needs a sleeve",
        (true, false, false) => "need sleeves",
        (false, _, true) => "needs flashing",
        (false, _, false) => "need flashing",
    };
    let advice = match (sleeves > 0, roof > 0) {
        (true, true) => "Set the sleeves before the pour or the blockwork, and detail the flashing before the roofing.",
        (true, false) => "Set the sleeves before the pour or the blockwork.",
        _ => "Detail the flashing before the roofing.",
    };
    let mut ids: Vec<Id> = vec![];
    for p in pens {
        if !ids.contains(&p.pen.pipe_id) {
            ids.push(p.pen.pipe_id.clone());
        }
    }
    ids.sort();
    Some(issue(
        "pipe_penetrations",
        Severity::Info,
        format!(
            "{} {need}: {}. {advice}",
            plural(n, "pipe penetration", "pipe penetrations"),
            parts.join(", ")
        ),
        ids,
    ))
}

// ------------------------------------------------------------------- entry

/// `Derived::pipes` and the pipe review items.
pub(crate) fn derive_pipes(
    project: &Project,
    analysis: &Analysis,
    assigned: &BTreeMap<Id, FaceRef>,
) -> (PipeNetwork, Vec<Issue>) {
    let runs: Vec<Run> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Pipe(p) => Run::new(project, p),
            _ => None,
        })
        .collect();
    if runs.is_empty() {
        return (PipeNetwork::default(), vec![]);
    }
    let walls: BTreeMap<&str, &WallTopo> = analysis
        .walls
        .iter()
        .map(|w| (w.wall_id.as_str(), w))
        .collect();
    let bodies = wall_bodies(project, &walls);
    let roofs = roof_shapes(project, analysis, &walls);

    let (found, joins) = fittings(&runs);
    let pens = penetrations(project, analysis, &runs, &bodies, &roofs);
    let (rows, total_length_m) = takeoff(&runs);

    let mut issues = across_openings(project, analysis, assigned, &runs, &holes(project, &bodies));
    issues.extend(through_columns(&runs, &solids(project)));
    issues.extend(pipe_crosses(&runs, &joins));
    issues.extend(drain_slopes(&runs));
    issues.extend(penetration_summary(&pens));

    let fittings: Vec<PipeFitting> = found.into_iter().map(|f| f.fitting).collect();
    let network = PipeNetwork {
        elbow_count: fittings
            .iter()
            .filter(|f| f.kind == FittingKind::Elbow)
            .count() as u32,
        tee_count: fittings
            .iter()
            .filter(|f| f.kind == FittingKind::Tee)
            .count() as u32,
        sleeve_count: pens.len() as u32,
        fittings,
        penetrations: pens.into_iter().map(|p| p.pen).collect(),
        takeoff: rows,
        total_length_m,
    };
    (network, issues)
}
