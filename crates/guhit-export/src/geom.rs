//! Small 2D vector helpers. Model space: mm, +x east, +y north, degrees
//! counter-clockwise.

use guhit_model::Point;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct V {
    pub x: f64,
    pub y: f64,
}

pub fn v(x: f64, y: f64) -> V {
    V { x, y }
}

impl From<Point> for V {
    fn from(p: Point) -> V {
        V { x: p.x, y: p.y }
    }
}

impl From<&Point> for V {
    fn from(p: &Point) -> V {
        V { x: p.x, y: p.y }
    }
}

impl std::ops::Add for V {
    type Output = V;
    fn add(self, o: V) -> V {
        v(self.x + o.x, self.y + o.y)
    }
}

impl std::ops::Sub for V {
    type Output = V;
    fn sub(self, o: V) -> V {
        v(self.x - o.x, self.y - o.y)
    }
}

impl std::ops::Mul<f64> for V {
    type Output = V;
    fn mul(self, k: f64) -> V {
        v(self.x * k, self.y * k)
    }
}

impl std::ops::Neg for V {
    type Output = V;
    fn neg(self) -> V {
        v(-self.x, -self.y)
    }
}

impl V {
    pub fn len(self) -> f64 {
        self.x.hypot(self.y)
    }

    pub fn dot(self, o: V) -> f64 {
        self.x * o.x + self.y * o.y
    }

    /// Unit vector, or None for a zero length vector.
    pub fn unit(self) -> Option<V> {
        let l = self.len();
        if l < 1e-9 || !l.is_finite() {
            None
        } else {
            Some(v(self.x / l, self.y / l))
        }
    }

    /// Left hand normal (rotated 90 degrees counter-clockwise).
    pub fn left(self) -> V {
        v(-self.y, self.x)
    }

    /// Angle of the vector in degrees, counter-clockwise from +x.
    pub fn angle_deg(self) -> f64 {
        self.y.atan2(self.x).to_degrees()
    }

    pub fn rotated(self, deg: f64) -> V {
        let (s, c) = deg.to_radians().sin_cos();
        v(self.x * c - self.y * s, self.x * s + self.y * c)
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

/// Unit vector at the given angle.
pub fn dir(deg: f64) -> V {
    let (s, c) = deg.to_radians().sin_cos();
    v(c, s)
}

/// Normalize an angle to [0, 360).
pub fn norm_deg(a: f64) -> f64 {
    let r = a % 360.0;
    if r < 0.0 {
        r + 360.0
    } else {
        r
    }
}

/// Angle for text running along `deg` so it never reads upside down.
/// Result is in (-90, 90].
pub fn readable_deg(deg: f64) -> f64 {
    let mut a = norm_deg(deg);
    if a > 180.0 {
        a -= 360.0;
    }
    if a > 90.0 + 1e-6 {
        a -= 180.0;
    } else if a <= -90.0 + 1e-6 {
        a += 180.0;
    }
    a
}

/// Axis aligned bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: V,
    pub max: V,
}

impl Bounds {
    pub fn empty() -> Bounds {
        Bounds {
            min: v(f64::INFINITY, f64::INFINITY),
            max: v(f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }

    pub fn is_empty(&self) -> bool {
        !(self.min.x <= self.max.x && self.min.y <= self.max.y)
    }

    pub fn add(&mut self, p: V) {
        if !p.is_finite() {
            return;
        }
        self.min.x = self.min.x.min(p.x);
        self.min.y = self.min.y.min(p.y);
        self.max.x = self.max.x.max(p.x);
        self.max.y = self.max.y.max(p.y);
    }

    pub fn width(&self) -> f64 {
        if self.is_empty() {
            0.0
        } else {
            self.max.x - self.min.x
        }
    }

    pub fn height(&self) -> f64 {
        if self.is_empty() {
            0.0
        } else {
            self.max.y - self.min.y
        }
    }

    pub fn intersects(&self, o: &Bounds) -> bool {
        !self.is_empty()
            && !o.is_empty()
            && self.min.x <= o.max.x
            && o.min.x <= self.max.x
            && self.min.y <= o.max.y
            && o.min.y <= self.max.y
    }

    pub fn center(&self) -> V {
        if self.is_empty() {
            v(0.0, 0.0)
        } else {
            v(
                (self.min.x + self.max.x) / 2.0,
                (self.min.y + self.max.y) / 2.0,
            )
        }
    }
}

/// Clip a polygon to the half plane `dot(p - origin, axis) >= t` when
/// `keep_greater`, else `<= t`. Sutherland-Hodgman, one plane.
pub fn clip_half_plane(poly: &[V], origin: V, axis: V, t: f64, keep_greater: bool) -> Vec<V> {
    let side = |p: V| {
        let d = (p - origin).dot(axis) - t;
        if keep_greater {
            d
        } else {
            -d
        }
    };
    let mut out = Vec::with_capacity(poly.len() + 2);
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        let da = side(a);
        let db = side(b);
        if da >= 0.0 {
            out.push(a);
        }
        if (da > 0.0 && db < 0.0) || (da < 0.0 && db > 0.0) {
            let k = da / (da - db);
            out.push(a + (b - a) * k);
        }
    }
    out
}

/// Absolute area of a polygon.
pub fn polygon_area(poly: &[V]) -> f64 {
    signed_area(poly).abs()
}

/// Signed area. Positive when the polygon runs counter-clockwise.
pub fn signed_area(poly: &[V]) -> f64 {
    let mut s = 0.0;
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        s += a.x * b.y - b.x * a.y;
    }
    s / 2.0
}

/// Drops non-finite and repeated points, including a closing point equal to
/// the first one.
pub fn clean_polygon(poly: &[V]) -> Vec<V> {
    const TOL: f64 = 1e-4;
    let mut out: Vec<V> = Vec::with_capacity(poly.len());
    for p in poly {
        if !p.is_finite() {
            continue;
        }
        if let Some(last) = out.last() {
            if (last.x - p.x).abs() < TOL && (last.y - p.y).abs() < TOL {
                continue;
            }
        }
        out.push(*p);
    }
    while out.len() > 1 {
        let a = out[0];
        let b = out[out.len() - 1];
        if (a.x - b.x).abs() < TOL && (a.y - b.y).abs() < TOL {
            out.pop();
        } else {
            break;
        }
    }
    out
}

/// Cleaned counter-clockwise copy. Empty when the input is not a polygon.
pub fn ensure_ccw(poly: &[V]) -> Vec<V> {
    let mut p = clean_polygon(poly);
    if p.len() < 3 {
        return Vec::new();
    }
    let a = signed_area(&p);
    if a.abs() < 1e-6 {
        return Vec::new();
    }
    if a < 0.0 {
        p.reverse();
    }
    p
}

/// Bounds of a point list.
pub fn bounds_of(points: &[V]) -> Bounds {
    let mut b = Bounds::empty();
    for p in points {
        b.add(*p);
    }
    b
}

/// Keeps the part of the polygon with `lo <= x <= hi`. Infinite bounds pass
/// that side through untouched.
pub fn clip_to_x_range(poly: &[V], lo: f64, hi: f64) -> Vec<V> {
    let mut p = poly.to_vec();
    if lo.is_finite() {
        p = clip_half_plane(&p, v(0.0, 0.0), v(1.0, 0.0), lo, true);
    }
    if hi.is_finite() {
        p = clip_half_plane(&p, v(0.0, 0.0), v(1.0, 0.0), hi, false);
    }
    clean_polygon(&p)
}

/// Mitred outward offset of a polygon. Good for building footprints, which are
/// mostly right angles. Very sharp corners are limited so a spike cannot run
/// away. A negative distance offsets inward.
pub fn offset_polygon(poly_in: &[V], dist: f64) -> Vec<V> {
    let poly = ensure_ccw(poly_in);
    if poly.len() < 3 || dist.abs() < 1e-6 {
        return poly;
    }
    let n = poly.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let prev = poly[(i + n - 1) % n];
        let cur = poly[i];
        let next = poly[(i + 1) % n];
        let d1 = (cur - prev).unit().unwrap_or(v(0.0, 0.0));
        let d2 = (next - cur).unit().unwrap_or(v(0.0, 0.0));
        // Outward normal of a counter-clockwise edge is its right side.
        let n1 = v(d1.y, -d1.x);
        let n2 = v(d2.y, -d2.x);
        let bis = (n1 + n2).unit().unwrap_or(n2);
        let cos = bis.dot(n1);
        let scale = dist / cos.max(0.35);
        out.push(cur + bis * scale);
    }
    out
}

/// A polyline moved `d` to its left (negative: to its right), mitred at the
/// bends. A closed path mitres at its first point too. Very sharp bends are
/// limited like `offset_polygon`.
pub fn offset_path(pts: &[V], d: f64, closed: bool) -> Vec<V> {
    let n = pts.len();
    let seg = |i: usize| (pts[(i + 1) % n] - pts[i]).unit();
    (0..n)
        .map(|i| {
            let into = if i > 0 {
                seg(i - 1)
            } else if closed {
                seg(n - 1)
            } else {
                None
            };
            let from = if i + 1 < n || closed { seg(i) } else { None };
            let normal = match (into, from) {
                (Some(a), Some(b)) => {
                    let bis = (a.left() + b.left()).unit().unwrap_or(b.left());
                    bis * (1.0 / bis.dot(b.left()).max(0.35))
                }
                (Some(a), None) => a.left(),
                (None, Some(b)) => b.left(),
                (None, None) => v(0.0, 0.0),
            };
            pts[i] + normal * d
        })
        .collect()
}

/// True when `p` is inside the polygon (even-odd). A point on an edge may
/// go either way.
pub fn point_in_polygon(p: V, poly: &[V]) -> bool {
    let mut inside = false;
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (b.x - a.x) * (p.y - a.y) / (b.y - a.y) {
            inside = !inside;
        }
    }
    inside
}

/// Area centroid of a polygon, or the mean of its points when it has no area.
pub fn centroid(poly: &[V]) -> V {
    let a = signed_area(poly);
    if poly.is_empty() {
        return v(0.0, 0.0);
    }
    if a.abs() < 1e-9 {
        let s = poly.iter().fold(v(0.0, 0.0), |s, p| s + *p);
        return s * (1.0 / poly.len() as f64);
    }
    let (mut cx, mut cy) = (0.0, 0.0);
    for i in 0..poly.len() {
        let p = poly[i];
        let q = poly[(i + 1) % poly.len()];
        let k = p.x * q.y - q.x * p.y;
        cx += (p.x + q.x) * k;
        cy += (p.y + q.y) * k;
    }
    v(cx / (6.0 * a), cy / (6.0 * a))
}

/// `subject` clipped to the inside of the convex counter-clockwise polygon
/// `clip`. Exact when the subject is convex too.
pub fn clip_convex(subject: &[V], clip: &[V]) -> Vec<V> {
    let mut out = subject.to_vec();
    for i in 0..clip.len() {
        if out.len() < 3 {
            return Vec::new();
        }
        let a = clip[i];
        let Some(u) = (clip[(i + 1) % clip.len()] - a).unit() else {
            continue;
        };
        out = clip_half_plane(&out, a, u.left(), 0.0, true);
    }
    out
}

/// The triangles of a simple polygon, each counter-clockwise.
pub fn triangles(poly: &[V]) -> Vec<Vec<V>> {
    let p = ensure_ccw(poly);
    triangulate(&p)
        .iter()
        .map(|t| vec![p[t[0]], p[t[1]], p[t[2]]])
        .collect()
}

/// Area two simple polygons share, any winding: every pair of their
/// triangles is intersected. Shared edges and corners give 0.
pub fn overlap_area(a: &[V], b: &[V]) -> f64 {
    if !bounds_of(a).intersects(&bounds_of(b)) {
        return 0.0;
    }
    let tb = triangles(b);
    triangles(a)
        .iter()
        .map(|x| tb.iter().map(|y| polygon_area(&clip_convex(x, y))).sum::<f64>())
        .sum()
}

/// A convex polygon less a convex counter-clockwise hole: the convex pieces
/// left, one outside each edge of the hole. A piece clear of the hole comes
/// back whole, one inside it not at all.
pub fn subtract_convex(piece: &[V], hole: &[V]) -> Vec<Vec<V>> {
    let mut rest = piece.to_vec();
    let mut out = Vec::new();
    for i in 0..hole.len() {
        if rest.len() < 3 {
            break;
        }
        let a = hole[i];
        let Some(u) = (hole[(i + 1) % hole.len()] - a).unit() else {
            continue;
        };
        let outside = clean_polygon(&clip_half_plane(&rest, a, u.left(), 0.0, false));
        if outside.len() >= 3 && polygon_area(&outside) > 1.0 {
            out.push(outside);
        }
        rest = clip_half_plane(&rest, a, u.left(), 0.0, true);
    }
    out
}

/// The parts of the segment a to b that lie inside the polygon, in order.
pub fn clip_segment(a: V, b: V, poly: &[V]) -> Vec<(V, V)> {
    let d = b - a;
    let cross = |p: V, q: V| p.x * q.y - p.y * q.x;
    let mut ts = vec![0.0, 1.0];
    for i in 0..poly.len() {
        let p = poly[i];
        let e = poly[(i + 1) % poly.len()] - p;
        let den = cross(d, e);
        if den.abs() < 1e-12 {
            continue;
        }
        let w = p - a;
        let t = cross(w, e) / den;
        let s = cross(w, d) / den;
        if t > 0.0 && t < 1.0 && (0.0..=1.0).contains(&s) {
            ts.push(t);
        }
    }
    ts.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<(V, V)> = Vec::new();
    for w in ts.windows(2) {
        if w[1] - w[0] < 1e-9 || !point_in_polygon(a + d * ((w[0] + w[1]) / 2.0), poly) {
            continue;
        }
        let (p, q) = (a + d * w[0], a + d * w[1]);
        match out.last_mut() {
            Some(last) if (last.1 - p).len() < 1e-6 => last.1 = q,
            _ => out.push((p, q)),
        }
    }
    out
}

/// The parts of the segment a to b outside the box, in order.
pub fn outside_bounds(a: V, b: V, bx: &Bounds) -> Vec<(V, V)> {
    if bx.is_empty() {
        return vec![(a, b)];
    }
    // Liang-Barsky: the stretch t0..t1 of the segment inside the box.
    let d = b - a;
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, q) in [
        (-d.x, a.x - bx.min.x),
        (d.x, bx.max.x - a.x),
        (-d.y, a.y - bx.min.y),
        (d.y, bx.max.y - a.y),
    ] {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return vec![(a, b)];
            }
            continue;
        }
        let r = q / p;
        if p < 0.0 {
            t0 = t0.max(r);
        } else {
            t1 = t1.min(r);
        }
    }
    if t0 >= t1 {
        return vec![(a, b)];
    }
    let mut out = Vec::new();
    if t0 > 1e-9 {
        out.push((a, a + d * t0));
    }
    if t1 < 1.0 - 1e-9 {
        out.push((a + d * t1, b));
    }
    out
}

/// Triangle index triples for a simple polygon of any winding. Ear clipping,
/// O(n^2), which is plenty for footprints and wall sections.
pub fn triangulate(poly: &[V]) -> Vec<[usize; 3]> {
    if poly.len() < 3 {
        return Vec::new();
    }
    if poly.len() == 3 {
        return vec![[0, 1, 2]];
    }
    let ccw = signed_area(poly) > 0.0;
    // Work on indices so the result refers to the caller's point list.
    let mut idx: Vec<usize> = (0..poly.len()).collect();
    if !ccw {
        idx.reverse();
    }
    let cross = |a: V, b: V, c: V| (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    let inside = |a: V, b: V, c: V, p: V| {
        // Strictly inside or on an edge of the triangle a, b, c (which is CCW).
        cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
    };

    let mut out = Vec::with_capacity(poly.len().saturating_sub(2));
    let mut guard = 0;
    while idx.len() > 3 {
        guard += 1;
        if guard > poly.len() * poly.len() + 16 {
            // Degenerate or self-intersecting: fan the rest so nothing is lost.
            break;
        }
        let n = idx.len();
        let mut clipped = false;
        for i in 0..n {
            let ia = idx[(i + n - 1) % n];
            let ib = idx[i];
            let ic = idx[(i + 1) % n];
            let (a, b, c) = (poly[ia], poly[ib], poly[ic]);
            if cross(a, b, c) <= 1e-9 {
                continue; // reflex or straight
            }
            let mut ear = true;
            for &j in &idx {
                if j == ia || j == ib || j == ic {
                    continue;
                }
                if inside(a, b, c, poly[j]) {
                    ear = false;
                    break;
                }
            }
            if ear {
                out.push([ia, ib, ic]);
                idx.remove(i);
                clipped = true;
                break;
            }
        }
        if !clipped {
            break;
        }
    }
    for i in 1..idx.len().saturating_sub(1) {
        out.push([idx[0], idx[i], idx[i + 1]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_angles() {
        assert!((readable_deg(0.0) - 0.0).abs() < 1e-9);
        assert!((readable_deg(90.0) - 90.0).abs() < 1e-9);
        assert!((readable_deg(180.0) - 0.0).abs() < 1e-9);
        assert!((readable_deg(270.0) - 90.0).abs() < 1e-9);
        assert!((readable_deg(135.0) + 45.0).abs() < 1e-9);
        assert!((readable_deg(-135.0) - 45.0).abs() < 1e-9);
    }

    #[test]
    fn triangulation_covers_the_polygon() {
        let l = [
            v(0.0, 0.0),
            v(60.0, 0.0),
            v(60.0, 20.0),
            v(20.0, 20.0),
            v(20.0, 50.0),
            v(0.0, 50.0),
        ];
        let tris = triangulate(&l);
        assert_eq!(tris.len(), 4, "n - 2 triangles");
        let total: f64 = tris
            .iter()
            .map(|t| polygon_area(&[l[t[0]], l[t[1]], l[t[2]]]))
            .sum();
        assert!((total - polygon_area(&l)).abs() < 1e-6);
        // Clockwise input gives the same cover.
        let mut cw = l.to_vec();
        cw.reverse();
        let total_cw: f64 = triangulate(&cw)
            .iter()
            .map(|t| polygon_area(&[cw[t[0]], cw[t[1]], cw[t[2]]]))
            .sum();
        assert!((total_cw - polygon_area(&l)).abs() < 1e-6);
        assert!(triangulate(&[v(0.0, 0.0), v(1.0, 1.0)]).is_empty());
    }

    #[test]
    fn offset_grows_a_rectangle_by_the_distance() {
        let sq = [v(0.0, 0.0), v(100.0, 0.0), v(100.0, 60.0), v(0.0, 60.0)];
        let out = offset_polygon(&sq, 10.0);
        let b = bounds_of(&out);
        assert!((b.min.x + 10.0).abs() < 1e-6);
        assert!((b.max.x - 110.0).abs() < 1e-6);
        assert!((b.min.y + 10.0).abs() < 1e-6);
        assert!((b.max.y - 70.0).abs() < 1e-6);
        // A clockwise input is normalised first, so it grows too.
        let mut cw = sq.to_vec();
        cw.reverse();
        assert!(polygon_area(&offset_polygon(&cw, 10.0)) > polygon_area(&sq));
    }

    #[test]
    fn clean_and_ccw_normalise_a_polygon() {
        let closed = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 5.0), v(0.0, 0.0)];
        assert_eq!(clean_polygon(&closed).len(), 3);
        let cw = [v(0.0, 0.0), v(0.0, 5.0), v(10.0, 5.0), v(10.0, 0.0)];
        assert!(signed_area(&ensure_ccw(&cw)) > 0.0);
        assert!(ensure_ccw(&[v(0.0, 0.0), v(1.0, 0.0), v(2.0, 0.0)]).is_empty());
    }

    #[test]
    fn overlaps_holes_and_clipped_segments() {
        let sq = [v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0), v(0.0, 100.0)];
        let hole = [v(25.0, 25.0), v(75.0, 25.0), v(75.0, 75.0), v(25.0, 75.0)];
        assert!((overlap_area(&sq, &hole) - 2500.0).abs() < 1e-6);
        // Sharing an edge is no overlap.
        let next = [v(100.0, 0.0), v(200.0, 0.0), v(200.0, 100.0), v(100.0, 100.0)];
        assert!(overlap_area(&sq, &next).abs() < 1e-6);
        // The pieces left round a hole add up to the rest of the square.
        let mut pieces = triangles(&sq);
        for t in triangles(&hole) {
            pieces = pieces.iter().flat_map(|p| subtract_convex(p, &t)).collect();
        }
        let left: f64 = pieces.iter().map(|p| polygon_area(p)).sum();
        assert!((left - 7500.0).abs() < 1e-6, "{left}");
        assert!(pieces.iter().all(|p| !point_in_polygon(centroid(p), &hole)));
        // A diagonal of an L stops at its inner corner.
        let l = [v(0.0, 0.0), v(20.0, 0.0), v(20.0, 10.0), v(10.0, 10.0), v(10.0, 20.0), v(0.0, 20.0)];
        let parts = clip_segment(v(0.0, 0.0), v(20.0, 20.0), &l);
        assert_eq!(parts.len(), 1);
        assert!((parts[0].1 - v(10.0, 10.0)).len() < 1e-9);
        // A box in the middle of a segment cuts it in two.
        let mut b = Bounds::empty();
        b.add(v(4.0, -1.0));
        b.add(v(6.0, 1.0));
        let out = outside_bounds(v(0.0, 0.0), v(10.0, 0.0), &b);
        assert_eq!(out.len(), 2);
        assert!((out[0].1.x - 4.0).abs() < 1e-9 && (out[1].0.x - 6.0).abs() < 1e-9);
        // Offsets of an open path are mitred at the bend.
        let path = offset_path(&[v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0)], 1.0, false);
        assert!((path[0] - v(0.0, 1.0)).len() < 1e-9);
        assert!((path[1] - v(9.0, 1.0)).len() < 1e-9);
        assert!((path[2] - v(9.0, 10.0)).len() < 1e-9);
    }

    #[test]
    fn clip_keeps_the_right_half() {
        let sq = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 2.0), v(0.0, 2.0)];
        let left = clip_half_plane(&sq, v(0.0, 0.0), v(1.0, 0.0), 4.0, false);
        assert!((polygon_area(&left) - 8.0).abs() < 1e-9);
        let right = clip_half_plane(&sq, v(0.0, 0.0), v(1.0, 0.0), 4.0, true);
        assert!((polygon_area(&right) - 12.0).abs() < 1e-9);
    }
}
