//! The roof plan: every roof seen from above with its ridges, hips and
//! valleys, eaves and rakes, a slope arrow with the pitch on each plane,
//! gutters along the eaves, the walls under the roof dashed, and the lower
//! floors' roof decks.
//!
//! Lines and planes come from `Derived::roofs` (lane L2). Until those land,
//! the Wave 1 planes of `model3d::Scene::roofs` are drawn, with their lines
//! found from shared plane edges.

use guhit_model::{Derived, Element, Project, RoofLineKind};

use crate::geom::*;
use crate::model3d;
use crate::plan::{line, Cat, Fill, HAlign, Item, Pen, Prim};
use crate::ExportError;

use super::annot::{self, paper};
use super::ViewItems;

/// One roof plane in plan with its top height function, for arrows.
struct Plane {
    outline: Vec<V>,
    /// Height of the top surface: z = a x + b y + c.
    abc: [f64; 3],
}

/// A roof line in plan.
struct RLine {
    kind: RoofLineKind,
    a: V,
    b: V,
}

/// Paper size of the pitch label, mm.
const PITCH_TEXT: f64 = 2.2;

pub fn items(p: &Project, d: &Derived, n: f64) -> Result<ViewItems, ExportError> {
    let mut planes: Vec<Plane> = Vec::new();
    let mut lines: Vec<RLine> = Vec::new();
    let mut gutters: Vec<(Vec<V>, f64)> = Vec::new();

    if !d.roofs.is_empty() {
        for g in &d.roofs {
            for pl in &g.planes {
                if pl.len() < 3 {
                    continue;
                }
                let pts: Vec<V> = pl.iter().map(|q| v(q.x, q.y)).collect();
                planes.push(Plane { outline: pts, abc: plane_fit(pl.iter().map(|q| (v(q.x, q.y), q.z)).collect()) });
            }
            for l in &g.lines {
                lines.push(RLine { kind: l.kind, a: v(l.a.x, l.a.y), b: v(l.b.x, l.b.y) });
            }
            let gutter = p.elements.iter().find_map(|e| match e {
                Element::Roof(r) if r.id == g.roof_id => r.gutter.as_ref().map(|t| t.size_mm),
                _ => None,
            });
            if let Some(size) = gutter.filter(|s| s.is_finite() && *s > 1.0) {
                for run in &g.eaves {
                    gutters.push((run.iter().map(|q| v(q.x, q.y)).collect(), size));
                }
            }
        }
    } else {
        let scene = model3d::build_scene(p, d);
        for solid in &scene.roofs {
            let first = planes.len();
            for pl in &solid.planes {
                let pts = ensure_ccw(&pl.plan);
                if pts.len() < 3 {
                    continue;
                }
                let samples: Vec<(V, f64)> = pts.iter().map(|q| (*q, pl.top(*q))).collect();
                planes.push(Plane { outline: pts, abc: plane_fit(samples) });
            }
            lines.extend(lines_from_planes(&planes[first..]));
        }
    }

    let mut items: Vec<Item> = Vec::new();
    // The walls under the roofs, dashed: the topmost building's footprint.
    let hidden = annot::scaled(&paper::HIDDEN, n);
    let outlines: Vec<Vec<V>> = planes.iter().map(|pl| pl.outline.clone()).collect();
    let mut levels: Vec<&guhit_model::Level> = p.levels.iter().collect();
    levels.sort_by(|a, b| a.elevation_mm.partial_cmp(&b.elevation_mm).unwrap_or(std::cmp::Ordering::Equal));
    let with_fp: Vec<&guhit_model::Level> = levels
        .iter()
        .copied()
        .filter(|l| d.footprints.iter().any(|f| f.level_id == l.id && f.polygon.len() >= 3))
        .collect();
    for (k, l) in with_fp.iter().enumerate() {
        let top = k + 1 == with_fp.len();
        for fp in d.footprints.iter().filter(|f| f.level_id == l.id && f.polygon.len() >= 3) {
            let poly: Vec<V> = fp.polygon.iter().map(V::from).collect();
            for i in 0..poly.len() {
                let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                // Under a roof: dashed. A lower floor's edge out in the
                // open is the edge of its roof deck: a light line.
                let mut pieces = vec![(a, b)];
                for o in &outlines {
                    pieces = pieces.into_iter().flat_map(|(p, q)| annot::outside_polygon(p, q, o)).collect();
                }
                if top || outlines.is_empty() {
                    items.extend(annot::dashed(Cat::Wall, Pen::Fine, a, b, &hidden));
                } else {
                    for (p, q) in pieces {
                        items.push(line(Cat::Slab, Pen::Light, p, q));
                    }
                }
            }
        }
    }

    // Plane outlines: eaves and rakes heavy-medium, lines inside medium.
    for l in &lines {
        let pen = match l.kind {
            RoofLineKind::Eave | RoofLineKind::Rake => Pen::Heavy,
            _ => Pen::Medium,
        };
        items.push(line(Cat::Roof, pen, l.a, l.b));
    }
    if lines.is_empty() {
        for o in &outlines {
            items.push(Item { cat: Cat::Roof, pen: Pen::Medium, prim: Prim::Poly { pts: o.clone(), closed: true, fill: Fill::None } });
        }
    }
    // Gutters: a second line outside the eave.
    for (run, size) in &gutters {
        let pts = offset_path(run, -size, false);
        let pts2 = offset_path(run, *size, false);
        // Outward is the side away from the roofs.
        let out = pick_outside(&pts, &pts2, &outlines);
        if out.len() >= 2 {
            items.push(Item { cat: Cat::Roof, pen: Pen::Fine, prim: Prim::Poly { pts: out, closed: false, fill: Fill::None } });
        }
    }

    // Slope arrows, downhill, with the pitch.
    for pl in &planes {
        let grad = v(pl.abc[0], pl.abc[1]);
        let c = centroid(&pl.outline);
        let at = if point_in_polygon(c, &pl.outline) { c } else { bounds_of(&pl.outline).center() };
        let th = PITCH_TEXT * n;
        if grad.len() < 1e-4 {
            items.push(annot::centered(Cat::Roof, at, th, 0.0, "FLAT", false));
            continue;
        }
        let down = (-grad).unit().unwrap_or(v(0.0, -1.0));
        let pitch = grad.len().atan().to_degrees();
        let b = bounds_of(&pl.outline);
        let len = (9.0 * n).min(b.width().min(b.height()).max(1.0) * 0.45);
        let (from, to) = (at - down * (len / 2.0), at + down * (len / 2.0));
        items.push(line(Cat::Roof, Pen::Light, from, to));
        let hl = (1.8 * n).min(len * 0.4);
        let side = down.left();
        items.push(Item {
            cat: Cat::Roof,
            pen: Pen::Light,
            prim: Prim::Poly { pts: vec![to, to - down * hl + side * (hl * 0.35), to - down * hl - side * (hl * 0.35)], closed: true, fill: Fill::Ink },
        });
        let rot = readable_deg(down.angle_deg());
        let up = dir(rot + 90.0);
        items.push(annot::text(Cat::Roof, at + up * (0.9 * n), th, rot, HAlign::Middle, &format!("{pitch:.0}° SLOPE"), false));
    }

    if items.is_empty() {
        return Err(ExportError::Empty("the project has no roof to draw".into()));
    }
    let bounds = crate::plan::items_bounds(&items);
    Ok(ViewItems { items, bounds, title: "ROOF PLAN".into(), scale_denominator: n as u32, images: Vec::new() })
}

/// The offset path whose middle is outside every roof outline.
fn pick_outside(a: &[V], b: &[V], outlines: &[Vec<V>]) -> Vec<V> {
    let mid = |p: &[V]| if p.len() >= 2 { (p[p.len() / 2 - usize::from(p.len() % 2 == 0)] + p[p.len() / 2]) * 0.5 } else { v(0.0, 0.0) };
    let inside = |q: V| outlines.iter().any(|o| point_in_polygon(q, o));
    if inside(mid(a)) {
        b.to_vec()
    } else {
        a.to_vec()
    }
}

/// Least squares plane z = a x + b y + c through the samples (exact for a
/// planar polygon).
fn plane_fit(s: Vec<(V, f64)>) -> [f64; 3] {
    if s.len() < 3 {
        return [0.0, 0.0, s.first().map(|q| q.1).unwrap_or(0.0)];
    }
    let m = s.len() as f64;
    let (cx, cy, cz) = s.iter().fold((0.0, 0.0, 0.0), |acc, (p, z)| (acc.0 + p.x / m, acc.1 + p.y / m, acc.2 + z / m));
    let (mut sxx, mut sxy, mut syy, mut sxz, mut syz) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (p, z) in &s {
        let (x, y, zz) = (p.x - cx, p.y - cy, z - cz);
        sxx += x * x;
        sxy += x * y;
        syy += y * y;
        sxz += x * zz;
        syz += y * zz;
    }
    let det = sxx * syy - sxy * sxy;
    if det.abs() < 1e-9 {
        return [0.0, 0.0, cz];
    }
    let a = (sxz * syy - syz * sxy) / det;
    let b = (syz * sxx - sxz * sxy) / det;
    [a, b, cz - a * cx - b * cy]
}

fn z_of(pl: &Plane, q: V) -> f64 {
    pl.abc[0] * q.x + pl.abc[1] * q.y + pl.abc[2]
}

/// Ridges, hips and valleys where two planes share an edge; eaves (level)
/// and rakes (sloping) where a plane edge is its own.
fn lines_from_planes(planes: &[Plane]) -> Vec<RLine> {
    let mut out = Vec::new();
    for (i, pl) in planes.iter().enumerate() {
        let o = &pl.outline;
        for k in 0..o.len() {
            let (a, b) = (o[k], o[(k + 1) % o.len()]);
            let Some(u) = (b - a).unit() else { continue };
            let len = (b - a).len();
            // Pieces of this edge shared with another plane's edge.
            let mut shared: Vec<(f64, f64, usize)> = Vec::new();
            for (j, other) in planes.iter().enumerate() {
                if j == i {
                    continue;
                }
                let oo = &other.outline;
                for m in 0..oo.len() {
                    let (c, e) = (oo[m], oo[(m + 1) % oo.len()]);
                    let off_c = (c - a).x * u.y - (c - a).y * u.x;
                    let off_e = (e - a).x * u.y - (e - a).y * u.x;
                    if off_c.abs() > 1.0 || off_e.abs() > 1.0 {
                        continue;
                    }
                    let (t0, t1) = ((c - a).dot(u), (e - a).dot(u));
                    let (lo, hi) = (t0.min(t1).max(0.0), t0.max(t1).min(len));
                    if hi - lo > 1.0 {
                        shared.push((lo, hi, j));
                    }
                }
            }
            if shared.is_empty() {
                let level = (z_of(pl, a) - z_of(pl, b)).abs() < 1.0;
                out.push(RLine { kind: if level { RoofLineKind::Eave } else { RoofLineKind::Rake }, a, b });
                continue;
            }
            for (lo, hi, j) in shared {
                // Each shared edge once.
                if j < i {
                    continue;
                }
                let (p, q) = (a + u * lo, a + u * hi);
                let mid = (p + q) * 0.5;
                let inward = u.left();
                // Convex (both planes fall away from the edge): ridge or hip.
                let zi = z_of(pl, mid + inward * 50.0);
                let zj = z_of(&planes[j], mid - inward * 50.0);
                let ze = z_of(pl, mid);
                let convex = zi < ze + 1e-6 && zj < ze + 1e-6;
                let level = (z_of(pl, p) - z_of(pl, q)).abs() < 1.0;
                let kind = match (convex, level) {
                    (true, true) => RoofLineKind::Ridge,
                    (true, false) => RoofLineKind::Hip,
                    (false, _) => RoofLineKind::Valley,
                };
                out.push(RLine { kind, a: p, b: q });
            }
        }
    }
    out
}
