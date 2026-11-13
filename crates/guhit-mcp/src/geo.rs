//! Plan geometry the MCP tools need on their own: finding the wall at a
//! point, the room around a point, distances between outlines. Millimeters,
//! +x east, +y north. The engine's own geometry stays in `guhit-core`.

use guhit_model::*;

pub fn pt(x: f64, y: f64) -> Point {
    Point { x, y }
}

pub fn dist(a: Point, b: Point) -> f64 {
    ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt()
}

/// Signed area, positive when counter-clockwise.
pub fn signed_area(poly: &[Point]) -> f64 {
    let n = poly.len();
    if n < 3 {
        return 0.0;
    }
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f64>()
        / 2.0
}

/// The polygon counter-clockwise.
pub fn ccw(mut poly: Vec<Point>) -> Vec<Point> {
    if signed_area(&poly) < 0.0 {
        poly.reverse();
    }
    poly
}

/// Even-odd ray test. Points on an edge may go either way.
pub fn contains(poly: &[Point], p: Point) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// A point inside a simple polygon: its centroid when that is inside, else a
/// point just inside the middle of its longest edge.
pub fn interior_point(poly: &[Point]) -> Option<Point> {
    let n = poly.len();
    if n < 3 {
        return None;
    }
    let a = signed_area(poly);
    if a.abs() < 1.0 {
        return None;
    }
    let (mut cx, mut cy) = (0.0, 0.0);
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        let cross = p.x * q.y - q.x * p.y;
        cx += (p.x + q.x) * cross;
        cy += (p.y + q.y) * cross;
    }
    let c = pt(cx / (6.0 * a), cy / (6.0 * a));
    if contains(poly, c) {
        return Some(c);
    }
    let poly = ccw(poly.to_vec());
    let (i, _) = (0..n)
        .map(|i| (i, dist(poly[i], poly[(i + 1) % n])))
        .max_by(|x, y| x.1.total_cmp(&y.1))?;
    let (p, q) = (poly[i], poly[(i + 1) % n]);
    let len = dist(p, q);
    // Counter-clockwise: the inside is on the left.
    for inset in [50.0, 200.0, 10.0] {
        let m = pt((p.x + q.x) / 2.0 - (q.y - p.y) / len * inset, (p.y + q.y) / 2.0 + (q.x - p.x) / len * inset);
        if contains(&poly, m) {
            return Some(m);
        }
    }
    None
}

/// Where `p` projects on segment a-b: the distance along it from `a` (not
/// clamped) and the perpendicular distance, positive on the left.
pub fn project(a: Point, b: Point, p: Point) -> Option<(f64, f64)> {
    let len = dist(a, b);
    if !len.is_finite() || len < 1e-6 {
        return None;
    }
    let (ux, uy) = ((b.x - a.x) / len, (b.y - a.y) / len);
    let (rx, ry) = (p.x - a.x, p.y - a.y);
    Some((rx * ux + ry * uy, -rx * uy + ry * ux))
}

pub fn dist_point_segment(p: Point, a: Point, b: Point) -> f64 {
    let len = dist(a, b);
    if len < 1e-9 {
        return dist(p, a);
    }
    let t = (((p.x - a.x) * (b.x - a.x) + (p.y - a.y) * (b.y - a.y)) / (len * len)).clamp(0.0, 1.0);
    dist(p, pt(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t))
}

fn segments_cross(a: Point, b: Point, c: Point, d: Point) -> bool {
    let o = |p: Point, q: Point, r: Point| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
    let (d1, d2, d3, d4) = (o(c, d, a), o(c, d, b), o(a, b, c), o(a, b, d));
    ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0))
}

/// Shortest distance between two outlines; 0 when they touch or overlap.
pub fn polygon_gap(a: &[Point], b: &[Point]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return f64::NAN;
    }
    if a.iter().any(|p| contains(b, *p)) || b.iter().any(|p| contains(a, *p)) {
        return 0.0;
    }
    let edges = |poly: &[Point]| -> Vec<(Point, Point)> {
        if poly.len() == 1 {
            return vec![(poly[0], poly[0])];
        }
        (0..poly.len()).map(|i| (poly[i], poly[(i + 1) % poly.len()])).collect()
    };
    let (ea, eb) = (edges(a), edges(b));
    let mut best = f64::INFINITY;
    for (p, q) in &ea {
        for (r, s) in &eb {
            if segments_cross(*p, *q, *r, *s) {
                return 0.0;
            }
            best = best
                .min(dist_point_segment(*p, *r, *s))
                .min(dist_point_segment(*q, *r, *s))
                .min(dist_point_segment(*r, *p, *q))
                .min(dist_point_segment(*s, *p, *q));
        }
    }
    best
}

/// The corners of a w x d rectangle centered on `c`, turned `rot_deg`.
pub fn rect(c: Point, w: f64, d: f64, rot_deg: f64) -> Vec<Point> {
    let (s, co) = rot_deg.to_radians().sin_cos();
    [(-w / 2.0, -d / 2.0), (w / 2.0, -d / 2.0), (w / 2.0, d / 2.0), (-w / 2.0, d / 2.0)]
        .iter()
        .map(|(x, y)| pt(c.x + x * co - y * s, c.y + x * s + y * co))
        .collect()
}

/// The plan outline of a stair: its run and width from the first riser.
pub fn stair_outline(s: &Stair) -> Vec<Point> {
    let (sin, cos) = s.rotation_deg.to_radians().sin_cos();
    // The flight runs along the rotated +y.
    let up = (-sin, cos);
    let side = (cos, sin);
    let half = s.width_mm / 2.0;
    let o = s.origin;
    vec![
        pt(o.x - side.0 * half, o.y - side.1 * half),
        pt(o.x + side.0 * half, o.y + side.1 * half),
        pt(o.x + side.0 * half + up.0 * s.run_mm, o.y + side.1 * half + up.1 * s.run_mm),
        pt(o.x - side.0 * half + up.0 * s.run_mm, o.y - side.1 * half + up.1 * s.run_mm),
    ]
}

/// The center of an opening on its wall's centerline.
pub fn opening_center(wall: &Wall, o: &Opening) -> Option<Point> {
    let len = dist(wall.start, wall.end);
    if len < 1e-6 {
        return None;
    }
    let t = o.offset_mm / len;
    Some(pt(wall.start.x + (wall.end.x - wall.start.x) * t, wall.start.y + (wall.end.y - wall.start.y) * t))
}

/// Round to 0.1 mm, so results read cleanly.
pub fn r1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interior_points_land_inside() {
        let square = vec![pt(0.0, 0.0), pt(4000.0, 0.0), pt(4000.0, 3000.0), pt(0.0, 3000.0)];
        assert!(contains(&square, interior_point(&square).unwrap()));
        // An L whose centroid is outside it.
        let l = vec![pt(0.0, 0.0), pt(6000.0, 0.0), pt(6000.0, 1000.0), pt(1000.0, 1000.0), pt(1000.0, 6000.0), pt(0.0, 6000.0)];
        let p = interior_point(&l).unwrap();
        assert!(contains(&l, p), "{p:?}");
        assert!(signed_area(&ccw(l.iter().rev().cloned().collect())) > 0.0);
    }

    #[test]
    fn gaps_between_outlines() {
        let a = rect(pt(0.0, 0.0), 1000.0, 1000.0, 0.0);
        let b = rect(pt(2000.0, 0.0), 1000.0, 1000.0, 0.0);
        assert!((polygon_gap(&a, &b) - 1000.0).abs() < 1e-6);
        let c = rect(pt(500.0, 0.0), 1000.0, 1000.0, 45.0);
        assert_eq!(polygon_gap(&a, &c), 0.0);
        let (along, off) = project(pt(0.0, 0.0), pt(1000.0, 0.0), pt(250.0, 80.0)).unwrap();
        assert_eq!((along, off), (250.0, 80.0));
    }
}
