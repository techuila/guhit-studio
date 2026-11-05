//! The site plan: the lot from its title with each side's bearing and
//! distance, the setback lines, the building footprint hatched, the roof
//! outline dashed, roads with their names and widths, site walls, the lot
//! area and the footprint's share of it (facts, never a code check), and
//! the north arrow.
//!
//! Bearings, distances, setbacks and coverage come from `Derived::site`
//! (lane L1); until it lands they are worked out here the same way.

use guhit_model::{Derived, Element, LayerKey, Lot, LotSideRole, Project};

use crate::geom::*;
use crate::plan::{line, Cat, Fill, HAlign, Item, Pen, Prim};
use crate::text::clean;
use crate::ExportError;

use super::annot::{self, paper};
use super::ViewItems;

pub fn items(p: &Project, d: &Derived, n: f64) -> Result<ViewItems, ExportError> {
    let lot = p.elements.iter().find_map(|e| match e {
        Element::Lot(l) => Some(l),
        _ => None,
    });
    let mut items: Vec<Item> = Vec::new();
    let north = p.settings.north_angle_deg;

    // The ground floor: the lowest level with a building.
    let mut levels: Vec<&guhit_model::Level> = p.levels.iter().collect();
    levels.sort_by(|a, b| a.elevation_mm.partial_cmp(&b.elevation_mm).unwrap_or(std::cmp::Ordering::Equal));
    let ground = levels.iter().find(|l| d.footprints.iter().any(|f| f.level_id == l.id && f.polygon.len() >= 3));
    let footprints: Vec<Vec<V>> = ground
        .map(|g| {
            d.footprints
                .iter()
                .filter(|f| f.level_id == g.id && f.polygon.len() >= 3)
                .map(|f| ensure_ccw(&f.polygon.iter().map(V::from).collect::<Vec<V>>()))
                .collect()
        })
        .unwrap_or_default();
    for fp in &footprints {
        items.push(Item { cat: Cat::Wall, pen: Pen::Heavy, prim: Prim::Poly { pts: fp.clone(), closed: true, fill: Fill::None } });
        items.extend(annot::hatch(Cat::Wall, Pen::Fine, fp, 45.0, 1.5 * n));
    }
    // Upper floors past the ground floor, and the roof's eaves, dashed.
    let hidden = annot::scaled(&paper::HIDDEN, n);
    for l in levels.iter().skip_while(|l| Some(**l) != ground.copied()).skip(1) {
        for f in d.footprints.iter().filter(|f| f.level_id == l.id && f.polygon.len() >= 3) {
            let poly: Vec<V> = f.polygon.iter().map(V::from).collect();
            for i in 0..poly.len() {
                let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                let mut pieces = vec![(a, b)];
                for fp in &footprints {
                    pieces = pieces.into_iter().flat_map(|(p, q)| annot::outside_polygon(p, q, fp)).collect();
                }
                for (a, b) in pieces {
                    items.extend(annot::dashed(Cat::Wall, Pen::Light, a, b, &hidden));
                }
            }
        }
    }
    // The roofs' outer edges only: eaves and rakes.
    for g in &d.roofs {
        for l in g.lines.iter().filter(|l| matches!(l.kind, guhit_model::RoofLineKind::Eave | guhit_model::RoofLineKind::Rake)) {
            items.extend(annot::dashed(Cat::Roof, Pen::Fine, v(l.a.x, l.a.y), v(l.b.x, l.b.y), &hidden));
        }
    }
    if d.roofs.is_empty() {
        let scene = crate::model3d::build_scene(p, d);
        for solid in &scene.roofs {
            for pl in &solid.planes {
                items.extend(annot::dashed_path(Cat::Roof, Pen::Fine, &pl.plan, true, &hidden));
            }
        }
    }
    // Site walls: fences and boundary walls, as the plan draws them.
    if crate::plan::layer_visible(p, LayerKey::Site) {
        for e in &p.elements {
            if let Element::Wall(w) = e {
                if w.is_site() {
                    if let Some(g) = d.walls.iter().find(|g| g.wall_id == w.id) {
                        let o: Vec<V> = g.outline.iter().map(V::from).collect();
                        if o.len() >= 3 {
                            items.push(Item { cat: Cat::Site, pen: Pen::Light, prim: Prim::Poly { pts: o, closed: true, fill: Fill::Tint(35) } });
                        }
                    }
                }
            }
        }
    }

    if let Some(lot) = lot.filter(|l| l.corners.len() >= 3) {
        lot_items(p, d, lot, &footprints, n, &mut items);
    }

    let b = crate::plan::items_bounds(&items);
    if b.is_empty() {
        return Err(ExportError::Empty("the site plan has nothing to draw: no lot and no building".into()));
    }
    // The north arrow, top right of the drawing.
    let r = paper::NORTH * n / 2.0;
    items.extend(annot::north_arrow(Cat::Symb, v(b.max.x + r + 4.0 * n, b.max.y - r), north, n));
    let bounds = crate::plan::items_bounds(&items);
    Ok(ViewItems { items, bounds, title: "SITE PLAN".into(), scale_denominator: n as u32, images: Vec::new() })
}

fn lot_items(p: &Project, d: &Derived, lot: &Lot, footprints: &[Vec<V>], n: f64, items: &mut Vec<Item>) {
    let corners: Vec<V> = lot.corners.iter().map(V::from).collect();
    let ccw = signed_area(&corners) >= 0.0;
    let k = corners.len();
    let site = d.site.as_deref().filter(|s| s.lot_id == lot.id && s.sides.len() == k);

    // The lot line: a phantom line, heavy.
    items.extend(annot::dashed_path(Cat::Site, Pen::Heavy, &corners, true, &annot::scaled(&paper::PHANTOM, n)));
    // Corner numbers, as the technical description counts them.
    let c = centroid(&corners);
    for (i, q) in corners.iter().enumerate() {
        items.push(annot::circle(Cat::Site, Pen::Light, *q, 0.6 * n, Fill::Ink));
        let out = (*q - c).unit().unwrap_or(v(1.0, 0.0));
        items.push(annot::centered(Cat::Text, *q + out * (2.6 * n), paper::SMALL * n, 0.0, &(i + 1).to_string(), true));
    }

    // Each side's bearing above the line and its distance below, outside.
    for i in 0..k {
        let (a, b) = (corners[i], corners[(i + 1) % k]);
        let Some(u) = (b - a).unit() else { continue };
        let len = (b - a).len();
        let (bearing, dist) = match site {
            Some(s) => (s.sides[i].bearing.clone(), s.sides[i].length_mm),
            None => (bearing(b - a, p.settings.north_angle_deg), len),
        };
        // Outside the lot is to the right of a counter-clockwise side.
        let out = if ccw { v(u.y, -u.x) } else { v(-u.y, u.x) };
        let rot = readable_deg(u.angle_deg());
        let up = dir(rot + 90.0);
        let mid = (a + b) * 0.5;
        let th = paper::SMALL * n;
        // Text "above" the line is on the side `up` points to.
        let above_out = up.dot(out) > 0.0;
        let (p_out, p_in) = (mid + out * (1.0 * n), mid - out * (1.0 * n));
        let (p_up, p_down) = if above_out { (p_out, p_in) } else { (p_in, p_out) };
        items.push(annot::text(Cat::Text, p_up + up * (0.2 * n), th, rot, HAlign::Middle, &clean(&bearing), false));
        items.push(annot::text(Cat::Text, p_down - up * (th + 0.2 * n), th, rot, HAlign::Middle, &format!("{:.2} m", dist / 1000.0), false));
    }

    // Setback lines, dashed, with their distance.
    let setback = match site {
        Some(s) if s.setback_outline.len() >= 3 => s.setback_outline.iter().map(V::from).collect(),
        _ => setback_outline(lot),
    };
    if setback.len() >= 3 {
        // A chain line, so it never reads as the dashed roof edge.
        items.extend(annot::dashed_path(Cat::Site, Pen::Light, &setback, true, &annot::scaled(&paper::CENTER, n)));
        for (i, side) in lot.sides.iter().enumerate().take(k) {
            if !(side.setback_mm > 0.0) {
                continue;
            }
            let (a, b) = (corners[i], corners[(i + 1) % k]);
            let Some(u) = (b - a).unit() else { continue };
            let inn = if ccw { u.left() } else { -u.left() };
            let at = (a + b) * 0.5 + inn * (side.setback_mm / 2.0);
            let rot = readable_deg(u.angle_deg());
            items.push(annot::centered(Cat::Text, at + u * ((b - a).len() * 0.25), paper::SMALL * n * 0.85, rot, &format!("{:.2} m SETBACK", side.setback_mm / 1000.0), false));
        }
    }

    // Each side's nearest building face (GA P1-10): the clearance the engine
    // measured (`Derived::site`), dimensioned to the lot line, with the
    // setback that side needs under it.
    if let Some(s) = site {
        let buildings: Vec<Vec<V>> = d.footprints.iter().filter(|f| f.polygon.len() >= 3).map(|f| f.polygon.iter().map(V::from).collect()).collect();
        let th = paper::SMALL * n;
        let tick_half = crate::plan::paper::DIM_TICK * n / 2.0;
        for i in 0..k {
            let Some(c) = s.sides[i].clearance_mm.filter(|c| c.is_finite() && *c > 1.0) else { continue };
            let (a, b) = (corners[i], corners[(i + 1) % k]);
            let Some((on_lot, on_face)) = nearest_pair(a, b, &buildings) else { continue };
            let Some(u) = (on_face - on_lot).unit() else { continue };
            items.push(line(Cat::Dims, Pen::Light, on_lot, on_face));
            let tick = u.rotated(45.0) * tick_half;
            for q in [on_lot, on_face] {
                items.push(line(Cat::Dims, Pen::Medium, q - tick, q + tick));
            }
            let rot = readable_deg(u.angle_deg());
            let up = dir(rot + 90.0);
            let mid = (on_lot + on_face) * 0.5;
            items.push(annot::text(Cat::Dims, mid + up * (0.6 * n), th, rot, HAlign::Middle, &format!("{:.2} m", c / 1000.0), false));
            let setback = lot.sides.get(i).map(|x| x.setback_mm).unwrap_or(0.0);
            if setback > 0.0 {
                items.push(annot::text(Cat::Dims, mid - up * (0.6 * n + th * 0.85), th * 0.85, rot, HAlign::Middle, &format!("SETBACK {:.2} m", setback / 1000.0), false));
            }
        }
    }

    // Roads along road sides: the far edge, the center line and the name.
    for (i, side) in lot.sides.iter().enumerate().take(k) {
        if side.role != LotSideRole::Road {
            continue;
        }
        let (a, b) = (corners[i], corners[(i + 1) % k]);
        let Some(u) = (b - a).unit() else { continue };
        let out = if ccw { v(u.y, -u.x) } else { v(-u.y, u.x) };
        let w = side.road_width_mm.filter(|w| w.is_finite() && *w > 0.0);
        let width = w.unwrap_or(6000.0);
        let ext = 3000.0_f64.min((b - a).len() * 0.5);
        let (a0, b0) = (a - u * ext, b + u * ext);
        items.push(line(Cat::Site, Pen::Light, a0 + out * width, b0 + out * width));
        items.extend(annot::dashed(Cat::Site, Pen::Fine, a0 + out * (width / 2.0), b0 + out * (width / 2.0), &annot::scaled(&paper::CENTER, n)));
        let rot = readable_deg(u.angle_deg());
        let mid = (a + b) * 0.5 + out * (width / 2.0);
        let name = if side.road.trim().is_empty() { "ROAD".to_string() } else { clean(&side.road).to_uppercase() };
        let th = paper::LABEL * n;
        let up = dir(rot + 90.0);
        items.push(annot::centered(Cat::Text, mid + up * (th * 0.9), th, rot, &name, true));
        if let Some(w) = w {
            items.push(annot::centered(Cat::Text, mid - up * (th * 0.9), paper::SMALL * n, rot, &format!("{:.2} m ROAD WIDTH", w / 1000.0), false));
        }
    }

    // The lot's facts, inside it near the bottom.
    let area = site.map(|s| s.area_m2).unwrap_or_else(|| polygon_area(&corners) / 1e6);
    let fp_area: f64 = footprints.iter().map(|f| polygon_area(f)).sum::<f64>() / 1e6;
    let coverage = site.map(|s| s.coverage_pct).unwrap_or(if area > 0.0 { fp_area / area * 100.0 } else { 0.0 });
    let tb = &p.title_block;
    let mut lines_txt = Vec::new();
    let lb = [tb.lot_no.trim(), tb.block_no.trim()];
    if !lb[0].is_empty() || !lb[1].is_empty() {
        let mut s = String::new();
        if !lb[0].is_empty() {
            s.push_str(&format!("LOT {}", clean(lb[0])));
        }
        if !lb[1].is_empty() {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str(&format!("BLK {}", clean(lb[1])));
        }
        lines_txt.push(s);
    }
    lines_txt.push(format!("LOT AREA {area:.2} m\u{00b2}"));
    if fp_area > 0.0 {
        lines_txt.push(format!("BUILDING FOOTPRINT {fp_area:.2} m\u{00b2} ({coverage:.1}%)"));
    }
    // Inside the lot, clear of the buildings and of the side figures: the
    // middle of the open strip between the lowest building and the lot's
    // bottom, else the lot's middle.
    let bb = bounds_of(&corners);
    let fb = footprints.iter().fold(Bounds::empty(), |mut b, f| {
        f.iter().for_each(|q| b.add(*q));
        b
    });
    let low = if fb.is_empty() { bb.center().y } else { (fb.min.y + bb.min.y) / 2.0 };
    let mut at = v(bb.center().x, low);
    if !point_in_polygon(at, &corners) || footprints.iter().any(|f| point_in_polygon(at, f)) {
        at = centroid(&corners);
    }
    let th = paper::SMALL * n;
    for (i, t) in lines_txt.iter().rev().enumerate() {
        items.push(annot::text(Cat::Text, at + v(0.0, i as f64 * th * 1.5), th, 0.0, HAlign::Middle, t, i + 1 == lines_txt.len()));
    }
}

/// The nearest points of the lot side a to b and the buildings' outlines:
/// (on the side, on a building face). Of equally near pairs (a face
/// parallel to the side), the one nearest the side's middle.
fn nearest_pair(a: V, b: V, buildings: &[Vec<V>]) -> Option<(V, V)> {
    let on = |p: V, s0: V, s1: V| {
        let d = s1 - s0;
        let t = ((p - s0).dot(d) / d.dot(d).max(1e-9)).clamp(0.0, 1.0);
        s0 + d * t
    };
    let mid = (a + b) * 0.5;
    let mut best: Option<(f64, f64, V, V)> = None;
    for poly in buildings {
        for k in 0..poly.len() {
            let (c, e) = (poly[k], poly[(k + 1) % poly.len()]);
            for (lot, face) in [(on(c, a, b), c), (on(e, a, b), e), (a, on(a, c, e)), (b, on(b, c, e))] {
                let gap = (face - lot).len();
                let off = (lot - mid).len();
                let better = match best {
                    None => true,
                    Some((g, o, ..)) => gap < g - 1.0 || (gap <= g + 1.0 && off < o),
                };
                if better {
                    best = Some((gap, off, lot, face));
                }
            }
        }
    }
    best.filter(|b| b.0 > 1.0).map(|(_, _, lot, face)| (lot, face))
}

/// Quadrant bearing of a plan direction from true north: "N 45°30' E".
/// True north is `north_angle_deg` counter-clockwise from +y.
pub fn bearing(d: V, north_angle_deg: f64) -> String {
    let na = if north_angle_deg.is_finite() { north_angle_deg } else { 0.0 };
    let north = dir(90.0 + na);
    let east = dir(na);
    let (dn, de) = (d.dot(north), d.dot(east));
    let ang = de.abs().atan2(dn.abs()).to_degrees();
    let mut deg = ang.floor();
    let mut min = ((ang - deg) * 60.0).round();
    if min >= 60.0 {
        deg += 1.0;
        min = 0.0;
    }
    // Due east or west reads N 90 E or W, due north or south E.
    let ns = if dn >= -1e-9 { "N" } else { "S" };
    let ew = if de >= -1e-9 { "E" } else { "W" };
    format!("{ns} {deg:.0}\u{00b0}{min:02.0}' {ew}")
}

/// The lot shrunk by each side's setback: each side moved in, the corners
/// where neighbours meet. Empty when no side has a setback.
pub fn setback_outline(lot: &Lot) -> Vec<V> {
    let corners = ensure_ccw(&lot.corners.iter().map(V::from).collect::<Vec<V>>());
    let k = corners.len();
    if k < 3 || lot.sides.len() < k || !lot.sides.iter().any(|s| s.setback_mm > 0.0) {
        return Vec::new();
    }
    // ensure_ccw may have reversed the order; match sides to edges.
    let reversed = signed_area(&lot.corners.iter().map(V::from).collect::<Vec<V>>()) < 0.0;
    let setback_of = |i: usize| -> f64 {
        // Edge i of the CCW list runs corners[i] to corners[i+1].
        let j = if reversed { (2 * k - 2 - i) % k } else { i };
        lot.sides[j].setback_mm.max(0.0)
    };
    let lines: Vec<(V, V)> = (0..k)
        .map(|i| {
            let (a, b) = (corners[i], corners[(i + 1) % k]);
            let u = (b - a).unit().unwrap_or(v(1.0, 0.0));
            let off = u.left() * setback_of(i);
            (a + off, u)
        })
        .collect();
    let mut out = Vec::with_capacity(k);
    for i in 0..k {
        let (p0, u0) = lines[(i + k - 1) % k];
        let (p1, u1) = lines[i];
        let cross = u0.x * u1.y - u0.y * u1.x;
        if cross.abs() < 1e-9 {
            out.push(p1);
            continue;
        }
        let t = ((p1 - p0).x * u1.y - (p1 - p0).y * u1.x) / cross;
        out.push(p0 + u0 * t);
    }
    out
}
