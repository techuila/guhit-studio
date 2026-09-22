//! 2D segment and polygon math. All values are millimeters.
//!
//! Everything here is pure and deterministic. No hash maps, no randomness.

use guhit_model::Point;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Endpoints closer than this are the same joint.
pub const JOIN_EPS: f64 = 1.0;

/// A mitre longer than this many offsets is cut back to a bevel.
const MITRE_LIMIT: f64 = 4.0;

/// Below this sine two directions count as parallel (about 1 degree).
const PARALLEL_SIN: f64 = 0.0175;

pub fn pt(x: f64, y: f64) -> Point {
    Point { x, y }
}

pub fn add(a: Point, b: Point) -> Point {
    pt(a.x + b.x, a.y + b.y)
}

pub fn sub(a: Point, b: Point) -> Point {
    pt(a.x - b.x, a.y - b.y)
}

pub fn scale(a: Point, k: f64) -> Point {
    pt(a.x * k, a.y * k)
}

pub fn dot(a: Point, b: Point) -> f64 {
    a.x * b.x + a.y * b.y
}

pub fn cross(a: Point, b: Point) -> f64 {
    a.x * b.y - a.y * b.x
}

pub fn length(a: Point) -> f64 {
    (a.x * a.x + a.y * a.y).sqrt()
}

pub fn dist(a: Point, b: Point) -> f64 {
    length(sub(a, b))
}

pub fn lerp(a: Point, b: Point, t: f64) -> Point {
    pt(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

pub fn is_finite(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// Unit vector, or None for a zero or non-finite vector.
pub fn unit(a: Point) -> Option<Point> {
    let l = length(a);
    if l.is_finite() && l > 1e-12 {
        Some(pt(a.x / l, a.y / l))
    } else {
        None
    }
}

/// Left normal of a direction (rotate 90 degrees counter-clockwise).
pub fn perp_left(d: Point) -> Point {
    pt(-d.y, d.x)
}

pub fn rotate_about(p: Point, pivot: Point, angle_deg: f64) -> Point {
    let (s, c) = angle_deg.to_radians().sin_cos();
    let v = sub(p, pivot);
    pt(pivot.x + v.x * c - v.y * s, pivot.y + v.x * s + v.y * c)
}

/// Distance along a-b (mm from a, not clamped) and the distance from `p` to
/// the closest point of the segment.
pub fn project_on_segment(p: Point, a: Point, b: Point) -> (f64, f64) {
    let ab = sub(b, a);
    let l = length(ab);
    if l < 1e-12 {
        return (0.0, dist(p, a));
    }
    let along = dot(sub(p, a), ab) / l;
    let clamped = along.clamp(0.0, l);
    let closest = lerp(a, b, clamped / l);
    (along, dist(p, closest))
}

pub fn dist_point_segment(p: Point, a: Point, b: Point) -> f64 {
    project_on_segment(p, a, b).1
}

/// Intersection of two infinite lines given as point + direction.
pub fn line_intersect(p1: Point, d1: Point, p2: Point, d2: Point) -> Option<Point> {
    let den = cross(d1, d2);
    let scale_ref = length(d1) * length(d2);
    if scale_ref < 1e-18 || den.abs() < 1e-12 * scale_ref {
        return None;
    }
    let t = cross(sub(p2, p1), d2) / den;
    let x = add(p1, scale(d1, t));
    if is_finite(x) {
        Some(x)
    } else {
        None
    }
}

pub fn signed_area(poly: &[Point]) -> f64 {
    let n = poly.len();
    if n < 3 {
        return 0.0;
    }
    let mut s = 0.0;
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        s += cross(a, b);
    }
    s / 2.0
}

pub fn perimeter(poly: &[Point]) -> f64 {
    let n = poly.len();
    if n < 2 {
        return 0.0;
    }
    (0..n).map(|i| dist(poly[i], poly[(i + 1) % n])).sum()
}

pub fn bbox(poly: &[Point]) -> (Point, Point) {
    let mut lo = pt(f64::INFINITY, f64::INFINITY);
    let mut hi = pt(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in poly {
        lo.x = lo.x.min(p.x);
        lo.y = lo.y.min(p.y);
        hi.x = hi.x.max(p.x);
        hi.y = hi.y.max(p.y);
    }
    (lo, hi)
}

/// Even-odd ray cast. Points exactly on the boundary are unspecified.
pub fn point_in_polygon(p: Point, poly: &[Point]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

pub fn dist_to_boundary(p: Point, poly: &[Point]) -> f64 {
    let n = poly.len();
    let mut best = f64::INFINITY;
    for i in 0..n {
        best = best.min(dist_point_segment(p, poly[i], poly[(i + 1) % n]));
    }
    best
}

/// Inside the polygon and at least `margin` away from its boundary.
pub fn strictly_inside(p: Point, poly: &[Point], margin: f64) -> bool {
    point_in_polygon(p, poly) && dist_to_boundary(p, poly) > margin
}

fn orient(a: Point, b: Point, c: Point) -> f64 {
    cross(sub(b, a), sub(c, a))
}

fn on_segment(a: Point, b: Point, p: Point) -> bool {
    p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x) && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
}

/// True when segments a-b and c-d touch or cross anywhere.
pub fn segments_touch(a: Point, b: Point, c: Point, d: Point) -> bool {
    let o1 = orient(a, b, c);
    let o2 = orient(a, b, d);
    let o3 = orient(c, d, a);
    let o4 = orient(c, d, b);
    if ((o1 > 0.0 && o2 < 0.0) || (o1 < 0.0 && o2 > 0.0))
        && ((o3 > 0.0 && o4 < 0.0) || (o3 < 0.0 && o4 > 0.0))
    {
        return true;
    }
    (o1 == 0.0 && on_segment(a, b, c))
        || (o2 == 0.0 && on_segment(a, b, d))
        || (o3 == 0.0 && on_segment(c, d, a))
        || (o4 == 0.0 && on_segment(c, d, b))
}

/// A simple polygon has no repeated vertex and no two edges that touch,
/// except neighbors at their shared vertex.
pub fn is_simple(poly: &[Point]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    for i in 0..n {
        if !is_finite(poly[i]) {
            return false;
        }
        let a = poly[i];
        let b = poly[(i + 1) % n];
        if dist(a, b) < 1e-9 {
            return false;
        }
        // The neighbor edge must not fold back onto this one.
        let c = poly[(i + 2) % n];
        if orient(a, b, c) == 0.0 && dot(sub(b, a), sub(c, b)) < 0.0 {
            return false;
        }
    }
    for i in 0..n {
        for j in (i + 1)..n {
            let adjacent = j == i + 1 || (i == 0 && j == n - 1);
            if adjacent {
                continue;
            }
            if segments_touch(poly[i], poly[(i + 1) % n], poly[j], poly[(j + 1) % n]) {
                return false;
            }
        }
    }
    true
}

/// Drop vertices that lie on the straight line between their neighbors,
/// and vertices that repeat the previous one.
pub fn remove_collinear(poly: &[Point], tol: f64) -> Vec<Point> {
    let mut pts: Vec<Point> = Vec::with_capacity(poly.len());
    for p in poly {
        if pts.last().map(|l| dist(*l, *p) < 1e-9).unwrap_or(false) {
            continue;
        }
        pts.push(*p);
    }
    while pts.len() > 1 && dist(pts[0], pts[pts.len() - 1]) < 1e-9 {
        pts.pop();
    }
    let mut changed = true;
    while changed && pts.len() > 3 {
        changed = false;
        let n = pts.len();
        for i in 0..n {
            let a = pts[(i + n - 1) % n];
            let b = pts[i];
            let c = pts[(i + 1) % n];
            let ac = sub(c, a);
            let l = length(ac);
            let off = if l < 1e-9 {
                dist(a, b)
            } else {
                (cross(ac, sub(b, a)) / l).abs()
            };
            let between = dot(sub(b, a), sub(c, b)) > 0.0;
            if off < tol && between {
                pts.remove(i);
                changed = true;
                break;
            }
        }
    }
    pts
}

/// Corner between two consecutive offset edges that meet at `v`.
/// `a` is the incoming unit direction offset left by `da`, `b` the outgoing
/// unit direction offset left by `db`. Returns one point for a clean mitre,
/// or two points for a step or a bevel.
pub fn offset_corner(v: Point, a: Point, da: f64, b: Point, db: f64) -> (Point, Option<Point>) {
    let pa = add(v, scale(perp_left(a), da));
    let pb = add(v, scale(perp_left(b), db));
    let two = |pa: Point, pb: Point| {
        if dist(pa, pb) < 1e-6 {
            (pa, None)
        } else {
            (pa, Some(pb))
        }
    };
    let c = cross(a, b);
    if c.abs() < PARALLEL_SIN {
        return two(pa, pb);
    }
    match line_intersect(pa, a, pb, b) {
        None => two(pa, pb),
        Some(x) => {
            // A left turn puts the offset corner inside the turn, which is
            // the true meeting point of the two faces. A right turn puts it
            // outside, where sharp angles shoot far away, so limit it there.
            let limit = MITRE_LIMIT * da.abs().max(db.abs()) + 1.0;
            if c < 0.0 && dist(x, v) > limit {
                two(pa, pb)
            } else {
                (x, None)
            }
        }
    }
}

/// Offset a closed walk to the left of its travel direction. `dists[i]`
/// belongs to the edge from `pts[i]` to `pts[i + 1]`. For a counter-clockwise
/// polygon this shrinks it, for a clockwise one it grows it.
pub fn offset_walk_left(pts: &[Point], dists: &[f64]) -> Vec<Point> {
    let n = pts.len();
    let mut out = Vec::with_capacity(n + 4);
    if n < 3 || dists.len() != n {
        return out;
    }
    for i in 0..n {
        let prev = pts[(i + n - 1) % n];
        let v = pts[i];
        let next = pts[(i + 1) % n];
        let (Some(a), Some(b)) = (unit(sub(v, prev)), unit(sub(next, v))) else {
            return vec![];
        };
        let (p, q) = offset_corner(v, a, dists[(i + n - 1) % n], b, dists[i]);
        out.push(p);
        if let Some(q) = q {
            out.push(q);
        }
    }
    out
}

// ------------------------------------------------------------- label point

fn signed_dist_to_rings(p: Point, rings: &[&[Point]]) -> f64 {
    let mut inside = false;
    let mut best = f64::INFINITY;
    for ring in rings {
        if point_in_polygon(p, ring) {
            inside = !inside;
        }
        best = best.min(dist_to_boundary(p, ring));
    }
    if inside {
        best
    } else {
        -best
    }
}

#[derive(Clone, Copy)]
struct Cell {
    c: Point,
    h: f64,
    d: f64,
    max: f64,
}

impl PartialEq for Cell {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Cell {}
impl PartialOrd for Cell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Cell {
    fn cmp(&self, other: &Self) -> Ordering {
        self.max
            .total_cmp(&other.max)
            .then(other.c.x.total_cmp(&self.c.x))
            .then(other.c.y.total_cmp(&self.c.y))
    }
}

fn cell(c: Point, h: f64, rings: &[&[Point]]) -> Cell {
    let d = signed_dist_to_rings(c, rings);
    Cell {
        c,
        h,
        d,
        max: d + h * std::f64::consts::SQRT_2,
    }
}

fn centroid(poly: &[Point]) -> Option<Point> {
    let a = signed_area(poly);
    if a.abs() < 1e-9 {
        return None;
    }
    let n = poly.len();
    let (mut cx, mut cy) = (0.0, 0.0);
    for i in 0..n {
        let p = poly[i];
        let q = poly[(i + 1) % n];
        let k = cross(p, q);
        cx += (p.x + q.x) * k;
        cy += (p.y + q.y) * k;
    }
    Some(pt(cx / (6.0 * a), cy / (6.0 * a)))
}

/// Fallback interior point: middle of the widest inside span of a few
/// horizontal scan lines.
fn scan_interior_point(rings: &[&[Point]]) -> Option<Point> {
    let (lo, hi) = bbox(rings[0]);
    let mut best: Option<(f64, Point)> = None;
    for k in 1..16 {
        let y = lo.y + (hi.y - lo.y) * (k as f64) / 16.0 + 1e-7;
        let mut xs: Vec<f64> = vec![];
        for ring in rings {
            let n = ring.len();
            for i in 0..n {
                let (a, b) = (ring[i], ring[(i + 1) % n]);
                if (a.y > y) != (b.y > y) {
                    xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
                }
            }
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        for pair in xs.chunks(2) {
            if pair.len() == 2 {
                let w = pair[1] - pair[0];
                if best.map(|(bw, _)| w > bw).unwrap_or(true) {
                    best = Some((w, pt((pair[0] + pair[1]) / 2.0, y)));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

/// Pole of inaccessibility: the interior point farthest from the boundary,
/// within `precision`. `holes` are excluded areas. Returns None when no
/// interior point can be found.
pub fn label_point(outer: &[Point], holes: &[Vec<Point>], precision: f64) -> Option<Point> {
    if outer.len() < 3 {
        return None;
    }
    let mut rings: Vec<&[Point]> = vec![outer];
    for h in holes {
        if h.len() >= 3 {
            rings.push(h.as_slice());
        }
    }
    let (lo, hi) = bbox(outer);
    let (w, h) = (hi.x - lo.x, hi.y - lo.y);
    let size = w.min(h);
    if !(size.is_finite() && size > 0.0) {
        return None;
    }
    let mut half = size / 2.0;
    let mut heap = BinaryHeap::new();
    let mut x = lo.x;
    while x < hi.x {
        let mut y = lo.y;
        while y < hi.y {
            heap.push(cell(pt(x + half, y + half), half, &rings));
            y += size;
        }
        x += size;
    }
    let mut best = cell(pt(lo.x + w / 2.0, lo.y + h / 2.0), 0.0, &rings);
    if let Some(c) = centroid(outer) {
        let cc = cell(c, 0.0, &rings);
        if cc.d > best.d {
            best = cc;
        }
    }
    let mut pops = 0;
    while let Some(c) = heap.pop() {
        pops += 1;
        if c.d > best.d {
            best = c;
        }
        if c.max - best.d <= precision || pops > 20_000 {
            continue;
        }
        half = c.h / 2.0;
        for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            heap.push(cell(pt(c.c.x + sx * half, c.c.y + sy * half), half, &rings));
        }
    }
    if best.d > 0.0 {
        return Some(best.c);
    }
    scan_interior_point(&rings).filter(|p| signed_dist_to_rings(*p, &rings) > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
        vec![pt(x0, y0), pt(x1, y0), pt(x1, y1), pt(x0, y1)]
    }

    #[test]
    fn area_and_orientation() {
        let r = rect(0.0, 0.0, 4000.0, 3000.0);
        assert_eq!(signed_area(&r), 12_000_000.0);
        let mut cw = r.clone();
        cw.reverse();
        assert_eq!(signed_area(&cw), -12_000_000.0);
        assert_eq!(perimeter(&r), 14_000.0);
    }

    #[test]
    fn point_in_polygon_basics() {
        let r = rect(0.0, 0.0, 10.0, 10.0);
        assert!(point_in_polygon(pt(5.0, 5.0), &r));
        assert!(!point_in_polygon(pt(15.0, 5.0), &r));
        assert!(!point_in_polygon(pt(5.0, -1.0), &r));
        assert!(strictly_inside(pt(5.0, 5.0), &r, 4.0));
        assert!(!strictly_inside(pt(1.0, 5.0), &r, 4.0));
    }

    #[test]
    fn line_intersection() {
        let x = line_intersect(pt(0.0, 0.0), pt(1.0, 0.0), pt(5.0, -5.0), pt(0.0, 1.0)).unwrap();
        assert!(dist(x, pt(5.0, 0.0)) < 1e-9);
        assert!(line_intersect(pt(0.0, 0.0), pt(1.0, 0.0), pt(0.0, 1.0), pt(2.0, 0.0)).is_none());
    }

    #[test]
    fn projection() {
        let (along, d) = project_on_segment(pt(3.0, 4.0), pt(0.0, 0.0), pt(10.0, 0.0));
        assert_eq!((along, d), (3.0, 4.0));
        let (along, d) = project_on_segment(pt(-3.0, 4.0), pt(0.0, 0.0), pt(10.0, 0.0));
        assert_eq!((along, d), (-3.0, 5.0));
    }

    #[test]
    fn simple_polygon_checks() {
        assert!(is_simple(&rect(0.0, 0.0, 1.0, 1.0)));
        let bow = vec![pt(0.0, 0.0), pt(10.0, 10.0), pt(10.0, 0.0), pt(0.0, 10.0)];
        assert!(!is_simple(&bow));
        let spike = vec![pt(0.0, 0.0), pt(10.0, 0.0), pt(5.0, 0.0), pt(5.0, 5.0)];
        assert!(!is_simple(&spike));
    }

    #[test]
    fn offset_shrinks_ccw_rectangle_per_edge() {
        let r = rect(0.0, 0.0, 5000.0, 6000.0);
        // south 75, east 50, north 75, west 75
        let net = offset_walk_left(&r, &[75.0, 50.0, 75.0, 75.0]);
        assert_eq!(
            net,
            vec![
                pt(75.0, 75.0),
                pt(4950.0, 75.0),
                pt(4950.0, 5925.0),
                pt(75.0, 5925.0)
            ]
        );
    }

    #[test]
    fn offset_grows_cw_rectangle() {
        let mut r = rect(0.0, 0.0, 8000.0, 6000.0);
        r.reverse();
        let out = offset_walk_left(&r, &[75.0; 4]);
        assert!((signed_area(&out).abs() - 8150.0 * 6150.0).abs() < 1e-3);
    }

    #[test]
    fn offset_handles_collinear_edges_with_a_step() {
        // South side is two collinear edges with different offsets.
        let p = vec![
            pt(0.0, 0.0),
            pt(2000.0, 0.0),
            pt(4000.0, 0.0),
            pt(4000.0, 3000.0),
            pt(0.0, 3000.0),
        ];
        let net = offset_walk_left(&p, &[75.0, 50.0, 75.0, 75.0, 75.0]);
        assert!(net.contains(&pt(2000.0, 75.0)));
        assert!(net.contains(&pt(2000.0, 50.0)));
        assert!(is_simple(&net));
        // Same offsets collapse to one point that remove_collinear drops.
        let net = offset_walk_left(&p, &[75.0; 5]);
        assert_eq!(remove_collinear(&net, 1e-6).len(), 4);
    }

    #[test]
    fn offset_l_shape_reflex_corner() {
        let l = vec![
            pt(0.0, 0.0),
            pt(6000.0, 0.0),
            pt(6000.0, 3000.0),
            pt(3000.0, 3000.0),
            pt(3000.0, 6000.0),
            pt(0.0, 6000.0),
        ];
        let net = offset_walk_left(&l, &[75.0; 6]);
        assert_eq!(net[3], pt(2925.0, 2925.0));
        let expect = 27_000_000.0 - 75.0 * perimeter(&l) + 4.0 * 75.0 * 75.0;
        // Each convex corner adds d^2 back, the reflex corner takes d^2 away: 5 - 1 = 4.
        assert!((signed_area(&net) - expect).abs() < 1e-6);
    }

    #[test]
    fn label_point_is_inside() {
        let r = rect(75.0, 75.0, 4950.0, 5925.0);
        let p = label_point(&r, &[], 10.0).unwrap();
        assert!(dist(p, pt(2512.5, 3000.0)) < 1e-6);
        // U shape: the centroid is outside, the label must not be.
        let u = vec![
            pt(0.0, 0.0),
            pt(9000.0, 0.0),
            pt(9000.0, 6000.0),
            pt(8000.0, 6000.0),
            pt(8000.0, 1000.0),
            pt(1000.0, 1000.0),
            pt(1000.0, 6000.0),
            pt(0.0, 6000.0),
        ];
        let p = label_point(&u, &[], 10.0).unwrap();
        assert!(strictly_inside(p, &u, 100.0));
    }

    #[test]
    fn label_point_avoids_holes() {
        let outer = rect(0.0, 0.0, 6000.0, 6000.0);
        let mut hole = rect(2000.0, 2000.0, 4000.0, 4000.0);
        hole.reverse();
        let p = label_point(&outer, &[hole.clone()], 10.0).unwrap();
        assert!(point_in_polygon(p, &outer));
        assert!(!point_in_polygon(p, &hole));
    }

    #[test]
    fn rotation() {
        let p = rotate_about(pt(10.0, 0.0), pt(0.0, 0.0), 90.0);
        assert!(dist(p, pt(0.0, 10.0)) < 1e-9);
    }
}
