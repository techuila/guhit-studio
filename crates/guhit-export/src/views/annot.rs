//! Drawing furniture shared by the views and the plan: explicit dashes (so
//! SVG, PDF and DXF all show them without linetypes), grid bubbles, door
//! and window tags, level datums and the north arrow. Sizes are paper mm
//! times the scale denominator `n`.

use crate::geom::*;
use crate::plan::{line, Cat, Fill, HAlign, Item, Pen, Prim};
use crate::text::{clean, est_width};

/// Paper sizes, mm.
pub mod paper {
    /// Grid bubble radius and label height.
    pub const BUBBLE_R: f64 = 4.0;
    pub const BUBBLE_TEXT: f64 = 3.5;
    /// Door and window tags.
    pub const TAG_R: f64 = 2.6;
    pub const TAG_TEXT: f64 = 2.0;
    /// Level datum text and symbol.
    pub const DATUM_TEXT: f64 = 2.5;
    pub const DATUM_R: f64 = 1.4;
    /// North arrow size.
    pub const NORTH: f64 = 12.0;
    /// Center line (grids): long dash, gap, dot, gap.
    pub const CENTER: [f64; 4] = [8.0, 1.2, 1.2, 1.2];
    /// Hidden line.
    pub const HIDDEN: [f64; 2] = [2.2, 1.1];
    /// Property line: long, gap, short, gap, short, gap.
    pub const PHANTOM: [f64; 6] = [10.0, 1.2, 2.0, 1.2, 2.0, 1.2];
    /// Label text.
    pub const LABEL: f64 = 2.5;
    pub const SMALL: f64 = 2.0;
}

pub fn text(cat: Cat, pos: V, height: f64, rot_deg: f64, align: HAlign, s: &str, bold: bool) -> Item {
    Item {
        cat,
        pen: Pen::Light,
        prim: Prim::Text { pos, height, rot_deg, align, text: clean(s), bold },
    }
}

/// Text centered on `c` (its middle, not its baseline).
pub fn centered(cat: Cat, c: V, height: f64, rot_deg: f64, s: &str, bold: bool) -> Item {
    let up = dir(rot_deg + 90.0);
    text(cat, c - up * (height * 0.36), height, rot_deg, HAlign::Middle, s, bold)
}

pub fn poly(cat: Cat, pen: Pen, pts: Vec<V>, closed: bool, fill: Fill) -> Item {
    Item { cat, pen, prim: Prim::Poly { pts, closed, fill } }
}

pub fn circle(cat: Cat, pen: Pen, c: V, r: f64, fill: Fill) -> Item {
    Item { cat, pen, prim: Prim::Circle { c, r, fill } }
}

/// The segment a to b as dashes of `pattern` (model mm: on, off, on, off,
/// ...), starting at `phase` into the pattern. Returns the phase it ends
/// at, so a polyline keeps its rhythm round its corners.
pub fn dash_into(out: &mut Vec<Item>, cat: Cat, pen: Pen, a: V, b: V, pattern: &[f64], phase: f64) -> f64 {
    let len = (b - a).len();
    let period: f64 = pattern.iter().sum();
    let Some(u) = (b - a).unit() else { return phase };
    if pattern.is_empty() || period <= 1e-9 || !len.is_finite() {
        out.push(line(cat, pen, a, b));
        return phase;
    }
    let mut t = 0.0;
    let mut ph = phase % period;
    // Find where in the pattern we are.
    let mut k = 0;
    while ph >= pattern[k] {
        ph -= pattern[k];
        k = (k + 1) % pattern.len();
    }
    while t < len {
        let step = (pattern[k] - ph).min(len - t);
        if k % 2 == 0 && step > 1e-9 {
            out.push(line(cat, pen, a + u * t, a + u * (t + step)));
        }
        t += step;
        ph += step;
        if ph >= pattern[k] - 1e-12 {
            ph = 0.0;
            k = (k + 1) % pattern.len();
        }
    }
    let used: f64 = pattern[..k].iter().sum::<f64>() + ph;
    used % period
}

pub fn dashed(cat: Cat, pen: Pen, a: V, b: V, pattern: &[f64]) -> Vec<Item> {
    let mut out = Vec::new();
    dash_into(&mut out, cat, pen, a, b, pattern, 0.0);
    out
}

pub fn dashed_path(cat: Cat, pen: Pen, pts: &[V], closed: bool, pattern: &[f64]) -> Vec<Item> {
    let mut out = Vec::new();
    let n = pts.len();
    if n < 2 {
        return out;
    }
    let segs = if closed { n } else { n - 1 };
    let mut ph = 0.0;
    for i in 0..segs {
        ph = dash_into(&mut out, cat, pen, pts[i], pts[(i + 1) % n], pattern, ph);
    }
    out
}

/// A pattern in paper mm scaled to model mm at 1:n.
pub fn scaled(pattern: &[f64], n: f64) -> Vec<f64> {
    pattern.iter().map(|x| x * n).collect()
}

/// A grid bubble: a circle with the label in it.
pub fn bubble(cat: Cat, c: V, label: &str, n: f64) -> Vec<Item> {
    let r = paper::BUBBLE_R * n;
    let mut th = paper::BUBBLE_TEXT * n;
    let w = est_width(label, th, true);
    if w > r * 1.6 {
        th *= r * 1.6 / w;
    }
    vec![circle(cat, Pen::Light, c, r, Fill::None), centered(cat, c, th, 0.0, label, true)]
}

/// A section bubble: the circle split across, the drawing's number on its
/// sheet over that sheet's number ("1" over "A-6"), as PH sets mark a cut,
/// so it never reads as a grid bubble.
pub fn split_bubble(cat: Cat, c: V, top: &str, bottom: &str, n: f64) -> Vec<Item> {
    let r = paper::BUBBLE_R * n;
    let fit = |t: &str, size: f64| {
        let w = est_width(t, size, true);
        if w > r * 1.5 { size * r * 1.5 / w } else { size }
    };
    let mut out = vec![circle(cat, Pen::Light, c, r, Fill::None), line(cat, Pen::Light, c - v(r, 0.0), c + v(r, 0.0))];
    if !top.is_empty() {
        out.push(centered(cat, c + v(0.0, r * 0.45), fit(top, paper::BUBBLE_TEXT * n * 0.7), 0.0, top, true));
    }
    if !bottom.is_empty() {
        out.push(centered(cat, c - v(0.0, r * 0.45), fit(bottom, paper::BUBBLE_TEXT * n * 0.55), 0.0, bottom, false));
    }
    out
}

/// A door tag (circle) or window tag (hexagon) with its mark.
pub fn opening_tag(cat: Cat, c: V, mark: &str, window: bool, n: f64) -> Vec<Item> {
    let r = paper::TAG_R * n;
    let mut th = paper::TAG_TEXT * n;
    let w = est_width(mark, th, false);
    let room = if window { r * 1.5 } else { r * 1.4 };
    if w > room {
        th *= room / w;
    }
    let mut out = Vec::new();
    if window {
        let pts: Vec<V> = (0..6).map(|i| c + dir(i as f64 * 60.0) * (r * 1.08)).collect();
        out.push(poly(cat, Pen::Light, pts, true, Fill::None));
    } else {
        out.push(circle(cat, Pen::Light, c, r, Fill::None));
    }
    out.push(centered(cat, c, th, 0.0, mark, false));
    out
}

/// A level datum on an elevation or section: the quartered circle a
/// little right of `x0`, the line through it, and the label on the line
/// past the symbol ("+3.000 SECOND FLOOR"), above it or, with `below`,
/// under it.
pub fn datum(cat: Cat, x0: f64, y: f64, label: &str, n: f64, below: bool) -> Vec<Item> {
    let r = paper::DATUM_R * n;
    let th = paper::DATUM_TEXT * n;
    let c = v(x0 + 3.0 * n, y);
    let tx = c.x + r + 1.5 * n;
    let x1 = tx + est_width(label, th, false) + 1.0 * n;
    let mut out = vec![line(cat, Pen::Fine, v(x0, y), v(x1, y))];
    out.push(circle(cat, Pen::Light, c, r, Fill::None));
    // Two opposite quarters filled.
    for start in [0.0_f64, 180.0] {
        let mut pts = vec![c];
        for k in 0..=6 {
            pts.push(c + dir(start + 15.0 * k as f64) * r);
        }
        out.push(poly(cat, Pen::Fine, pts, true, Fill::Ink));
    }
    out.push(line(cat, Pen::Fine, v(c.x, y - r), v(c.x, y + r)));
    let ty = if below { y - 0.9 * n - th * 0.75 } else { y + 0.8 * n };
    out.push(text(cat, v(tx, ty), th, 0.0, HAlign::Start, label, false));
    out
}

/// "+3.000" from an elevation in mm.
pub fn level_text(mm: f64) -> String {
    let m = mm / 1000.0;
    let m = if m.abs() < 0.0005 { 0.0 } else { m };
    if m < 0.0 {
        format!("-{:.3}", -m)
    } else {
        format!("+{m:.3}")
    }
}

/// A north arrow at `c`, pointing to true north: `north_angle_deg` is true
/// north's rotation from +y, counter-clockwise (ProjectSettings).
pub fn north_arrow(cat: Cat, c: V, north_angle_deg: f64, n: f64) -> Vec<Item> {
    let size = paper::NORTH * n;
    let r = size / 2.0;
    let up = dir(90.0 + if north_angle_deg.is_finite() { north_angle_deg } else { 0.0 });
    let side = up.rotated(-90.0);
    let tip = c + up * (r * 0.95);
    let tail = c - up * (r * 0.8);
    let mut out = vec![circle(cat, Pen::Light, c, r, Fill::None)];
    // Half the arrow filled, half open, the usual way.
    out.push(poly(cat, Pen::Light, vec![tip, c - up * (r * 0.35) + side * (r * 0.42), tail], true, Fill::Ink));
    out.push(poly(cat, Pen::Light, vec![tip, tail, c - up * (r * 0.35) - side * (r * 0.42)], true, Fill::None));
    out.push(centered(cat, c + up * (r + 2.6 * n), 3.2 * n, 0.0, "N", true));
    out
}

/// A closed circle as a polygon, counter-clockwise.
pub fn circle_pts(c: V, r: f64, segments: usize) -> Vec<V> {
    (0..segments).map(|i| c + dir(360.0 * i as f64 / segments as f64) * r).collect()
}

/// The parts of the segment a to b outside the polygon.
pub fn outside_polygon(a: V, b: V, poly: &[V]) -> Vec<(V, V)> {
    let len = (b - a).len();
    if len < 1e-9 {
        return vec![];
    }
    let inside = clip_segment(a, b, poly);
    let u = (b - a) * (1.0 / len);
    let mut spans: Vec<(f64, f64)> = inside.iter().map(|(p, q)| ((*p - a).dot(u), (*q - a).dot(u))).collect();
    spans.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = Vec::new();
    let mut at = 0.0;
    for (s0, s1) in spans {
        if s0 - at > 0.5 {
            out.push((a + u * at, a + u * s0));
        }
        at = at.max(s1);
    }
    if len - at > 0.5 {
        out.push((a + u * at, b));
    }
    out
}

/// Parallel hatch lines across a polygon at `angle_deg`, `spacing` apart.
pub fn hatch(cat: Cat, pen: Pen, poly: &[V], angle_deg: f64, spacing: f64) -> Vec<Item> {
    let mut out = Vec::new();
    if poly.len() < 3 || !(spacing > 1.0) {
        return out;
    }
    let u = dir(angle_deg);
    let nrm = u.left();
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut alo, mut ahi) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in poly {
        lo = lo.min(p.dot(nrm));
        hi = hi.max(p.dot(nrm));
        alo = alo.min(p.dot(u));
        ahi = ahi.max(p.dot(u));
    }
    let mut k = (lo / spacing).ceil();
    let mut count = 0;
    while k * spacing < hi && count < 4000 {
        let off = k * spacing;
        let a = nrm * off + u * (alo - 10.0);
        let b = nrm * off + u * (ahi + 10.0);
        for (p, q) in clip_segment(a, b, poly) {
            out.push(line(cat, pen, p, q));
        }
        k += 1.0;
        count += 1;
    }
    out
}
