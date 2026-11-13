//! Stairs on plan sheets. LANE L2 owns this module.
//!
//! `stairs_items` draws every stair from `Derived::stairs` (DECISIONS D54),
//! straight, L and U alike: on its own level the flights and landings with
//! their tread lines up to the plan's cut, a break line there and what is
//! above it dashed, the travel arrow along the walking line round the turn,
//! and UP; on the level it arrives at the whole stair dashed with DN and the
//! arrow from the top down.

use guhit_model::{Derived, Level, Project, Stair, StairGeometry};

use crate::geom::*;
use crate::plan::{line, paper, Cat, Fill, HAlign, Item, Pen, Prim};

/// A level counts as the top of a flight when its floor is this close to it.
pub const STAIR_TOP_MATCH_MM: f64 = 300.0;
/// The plan cuts a level this high above its floor: treads above it are
/// drawn dashed past a break line (the 2D editor cuts at the same height).
pub const STAIR_CUT_MM: f64 = 1200.0;

/// One flight's frame from its derived outline (first riser left, first
/// riser right, top right, top left): origin at the middle of the first
/// riser, `up`, `across` to the right, width and run.
fn flight_frame(f: &guhit_model::Flight) -> Option<(V, V, V, f64, f64)> {
    if f.outline.len() != 4 {
        return None;
    }
    let (b0, b1, t0) = (V::from(f.outline[0]), V::from(f.outline[1]), V::from(f.outline[3]));
    let up = V::from(f.up);
    let across = (b1 - b0).unit()?;
    let width = (b1 - b0).len();
    let run = (t0 - b0).dot(up);
    (width >= 1.0 && run >= 1.0).then_some(((b0 + b1) * 0.5, up, across, width, run))
}

/// The two plan points where a flight's sides are `u` up from its first riser.
fn across_at(o: V, up: V, across: V, hw: f64, u: f64) -> (V, V) {
    (o - across * hw + up * u, o + across * hw + up * u)
}

/// Every stair of `derived` on `level`: going up from it, or arriving on it
/// from below. The plan's stairs pass calls this once per level.
pub fn stairs_items(derived: &Derived, level: &Level, n: f64, out: &mut Vec<Item>) {
    for g in &derived.stairs {
        if g.level_id == level.id {
            stair_up_items(g, n, out);
        } else if g.arrives_at.as_deref() == Some(level.id.as_str()) {
            stair_down_items(g, n, out);
        }
    }
}

/// A flight's outline from `u0` to `u1` up it: the sides, and the riser
/// lines at either end that are real ends (`bottom`, `top`).
/// A part from the first riser is one closed outline (its top edge at the
/// cut falls on a tread line), which keeps labels finding the flight.
fn flight_part(o: V, up: V, across: V, hw: f64, (u0, u1): (f64, f64), (bottom, top): (bool, bool), cat: Cat, pen: Pen, out: &mut Vec<Item>) {
    let (l0, r0) = across_at(o, up, across, hw, u0);
    let (l1, r1) = across_at(o, up, across, hw, u1);
    if bottom {
        out.push(Item { cat, pen, prim: Prim::Poly { pts: vec![l0, r0, r1, l1], closed: true, fill: Fill::None } });
        return;
    }
    out.push(line(cat, pen, l0, l1));
    out.push(line(cat, pen, r0, r1));
    if top {
        out.push(line(cat, pen, l1, r1));
    }
}

/// The break line across a flight at `u`: a slanted line with a small zig
/// in the middle, as plans mark where the cut passes.
fn break_line(o: V, up: V, across: V, hw: f64, u: f64, going: f64, out: &mut Vec<Item>) {
    let slant = going.min(300.0);
    let (a, b) = (o - across * hw + up * (u - slant / 2.0), o + across * hw + up * (u + slant / 2.0));
    let mid = (a + b) * 0.5;
    let z = (b - a).unit().unwrap_or(across);
    let k = going.min(150.0) * 0.5;
    let p1 = mid - z * k;
    let p2 = mid + z * k;
    let zig = z.left() * (k * 0.8);
    out.push(Item {
        cat: Cat::Strs,
        pen: Pen::Light,
        prim: Prim::Poly { pts: vec![a, p1, p1 + zig + z * (k * 0.5), p2 - zig - z * (k * 0.5), p2, b], closed: false, fill: Fill::None },
    });
}

/// The arrow along a polyline: a dot where it starts, the line, and a head
/// at its end. Always solid, on the stairs layer.
fn path_arrow(path: &[V], n: f64, out: &mut Vec<Item>) {
    if path.len() < 2 {
        return;
    }
    let (from, to, before) = (path[0], path[path.len() - 1], path[path.len() - 2]);
    let Some(u) = (to - before).unit() else { return };
    out.push(Item { cat: Cat::Strs, pen: Pen::Light, prim: Prim::Poly { pts: path.to_vec(), closed: false, fill: Fill::None } });
    out.push(Item { cat: Cat::Strs, pen: Pen::Light, prim: Prim::Circle { c: from, r: 0.45 * n, fill: Fill::Ink } });
    let hl = (paper::STAIR_ARROW * n).min((to - before).len() * 0.9);
    let side = u.left() * (hl * 0.32);
    out.push(Item {
        cat: Cat::Strs,
        pen: Pen::Light,
        prim: Prim::Poly { pts: vec![to, to - u * hl + side, to - u * hl - side], closed: true, fill: Fill::Ink },
    });
}

/// The walking line in plan from the middle of the first tread to the
/// middle of the last: what the arrow follows round the turn.
fn arrow_path(g: &StairGeometry) -> Vec<V> {
    let w = &g.walk_line;
    if w.len() < 3 {
        return w.iter().map(|p| v(p.x, p.y)).collect();
    }
    let mut out: Vec<V> = vec![];
    for p in &w[1..w.len() - 1] {
        let q = v(p.x, p.y);
        if out.last().is_none_or(|l| (*l - q).len() > 1e-6) {
            out.push(q);
        }
    }
    // Straight runs are one segment; keep only the bends.
    let mut bends: Vec<V> = vec![];
    for (i, p) in out.iter().enumerate() {
        if i == 0 || i + 1 == out.len() {
            bends.push(*p);
            continue;
        }
        let (a, b) = (out[i] - out[i - 1], out[i + 1] - out[i]);
        if a.x * b.y - a.y * b.x != 0.0 && (a.x * b.y - a.y * b.x).abs() > 1e-6 * a.len() * b.len() {
            bends.push(*p);
        }
    }
    bends
}

fn label(at: V, along: V, th: f64, text: &str, out: &mut Vec<Item>) {
    let rot = readable_deg(along.y.atan2(along.x).to_degrees() - 90.0);
    let upr = dir(rot + 90.0);
    out.push(Item {
        cat: Cat::Strs,
        pen: Pen::Light,
        prim: Prim::Text { pos: at - upr * (th * 0.36), height: th, rot_deg: rot, align: HAlign::Middle, text: text.into(), bold: false },
    });
}

/// A stair on its own level: below the cut, flights and landings solid with
/// their tread lines; a break line at the cut; above it dashed. The arrow
/// along the walking line from the first tread round the turn to the top,
/// "UP" before the first riser.
pub fn stair_up_items(g: &StairGeometry, n: f64, out: &mut Vec<Item>) {
    let cut = STAIR_CUT_MM;
    let mut cut_drawn = false;
    for f in &g.flights {
        let Some((o, up, across, w, run)) = flight_frame(f) else { continue };
        let hw = w / 2.0;
        let going = if f.nosings.is_empty() { run } else { run / f.nosings.len() as f64 };
        // The first tread whose top is over the cut, and where it starts.
        let above = f.nosings.iter().position(|ns| ns.z > cut);
        let split = match above {
            Some(0) => 0.0,
            Some(k) => going * k as f64,
            None => run,
        };
        if split > 0.0 {
            flight_part(o, up, across, hw, (0.0, split), (true, above.is_none()), Cat::Strs, Pen::Medium, out);
        }
        if split < run {
            flight_part(o, up, across, hw, (split, run), (split == 0.0, true), Cat::StrsBelow, Pen::Light, out);
        }
        for ns in f.nosings.iter().skip(1) {
            let cat = if ns.z > cut { Cat::StrsBelow } else { Cat::Strs };
            out.push(line(cat, Pen::Light, V::from(ns.a), V::from(ns.b)));
        }
        if !cut_drawn && above.is_some_and(|k| k > 0) {
            break_line(o, up, across, hw, split, going, out);
            cut_drawn = true;
        }
    }
    for l in &g.landings {
        let cat = if l.z > cut { Cat::StrsBelow } else { Cat::Strs };
        let pts: Vec<V> = l.polygon.iter().map(|p| V::from(*p)).collect();
        out.push(Item { cat, pen: if cat == Cat::Strs { Pen::Medium } else { Pen::Light }, prim: Prim::Poly { pts, closed: true, fill: Fill::None } });
    }
    path_arrow(&arrow_path(g), n, out);
    if let Some((o, up, ..)) = g.flights.first().and_then(flight_frame) {
        let th = paper::STAIR_LABEL * n;
        label(o - up * (th * 1.1), up, th, "UP", out);
    }
}

/// A stair seen from the level it arrives at: every flight and landing
/// dashed with its tread lines, the arrow from the top down the walking
/// line, "DN" just past the top.
pub fn stair_down_items(g: &StairGeometry, n: f64, out: &mut Vec<Item>) {
    for f in &g.flights {
        let Some((o, up, across, w, run)) = flight_frame(f) else { continue };
        flight_part(o, up, across, w / 2.0, (0.0, run), (true, true), Cat::StrsBelow, Pen::Light, out);
        for ns in f.nosings.iter().skip(1) {
            out.push(line(Cat::StrsBelow, Pen::Light, V::from(ns.a), V::from(ns.b)));
        }
    }
    for l in &g.landings {
        let pts: Vec<V> = l.polygon.iter().map(|p| V::from(*p)).collect();
        out.push(Item { cat: Cat::StrsBelow, pen: Pen::Light, prim: Prim::Poly { pts, closed: true, fill: Fill::None } });
    }
    let mut path = arrow_path(g);
    path.reverse();
    path_arrow(&path, n, out);
    if let Some((o, up, _, _, run)) = g.flights.last().and_then(flight_frame) {
        let th = paper::STAIR_LABEL * n;
        label(o + up * (run + th * 1.1), up, th, "DN", out);
    }
}

/// The level a stair arrives at: of the levels above the stair's own, the
/// one whose elevation is closest to the stair's top (its level's elevation
/// plus height), within 300 mm. The 3D view finds it the same way
/// (`src/viewer3d/walk/stairs.ts`).
pub fn stair_arrival<'a>(project: &'a Project, s: &Stair) -> Option<&'a Level> {
    let own = project.levels.iter().find(|l| l.id == s.level_id)?;
    let top = own.elevation_mm + own.height_mm;
    let mut best: Option<(&Level, f64)> = None;
    for l in &project.levels {
        if l.id == own.id || l.elevation_mm <= own.elevation_mm {
            continue;
        }
        let d = (l.elevation_mm - top).abs();
        if d <= STAIR_TOP_MATCH_MM && best.is_none_or(|(_, b)| d < b) {
            best = Some((l, d));
        }
    }
    best.map(|(l, _)| l)
}
