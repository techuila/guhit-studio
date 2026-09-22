//! Wall recognition and the command plan.
//!
//! A CAD floor plan draws walls as two parallel lines. The recognizer pairs
//! them up: two segments on the same layer that are parallel, between 50 and
//! 600 mm apart and overlap over at least 300 mm become one wall centerline,
//! trimmed to the overlap, with the measured thickness rounded to 5 mm.
//!
//! Recognition alone leaves corners open: each wall is trimmed to where its
//! own two faces overlap, so at a corner two walls stop half a thickness
//! short of each other. Three healing passes close that, because the engine
//! only joins walls whose endpoints are within 1 mm:
//! 1. snap endpoints within 20 mm onto one point,
//! 2. extend two walls that meet at an angle to their intersection,
//! 3. drop a still-loose endpoint onto the body of a wall that passes near it.
//!
//! Limits, all of them deliberate:
//! - Only straight walls. Curved walls stay linework.
//! - The nearest parallel line shadows the ones behind it over the same run,
//!   so a line never pairs with two partners covering the same stretch.
//! - A wall thicker than 600 mm or thinner than 50 mm is not recognized.
//! - Layers are handled one at a time, so a plan whose two wall faces sit on
//!   different layers needs both layers picked, and even then the pairing is
//!   per layer. Import it as linework instead.

use std::collections::{BTreeMap, BTreeSet};

use guhit_model::*;

use crate::read::Raw;
use crate::ImportPlan;

pub const MIN_THICKNESS_MM: f64 = 50.0;
pub const MAX_THICKNESS_MM: f64 = 600.0;
pub const MIN_OVERLAP_MM: f64 = 300.0;
pub const SNAP_MM: f64 = 20.0;
/// The engine refuses a wall shorter than this.
pub const MIN_WALL_MM: f64 = 50.0;
/// Thickness is reported to the nearest 5 mm.
const THICKNESS_STEP_MM: f64 = 5.0;
/// Two segments count as parallel below about 2 degrees.
const PARALLEL_SIN: f64 = 0.035;
/// Two walls count as meeting at an angle above about 11 degrees.
const CORNER_SIN: f64 = 0.2;
const MIN_SEG_MM: f64 = 1.0;
/// Angle bucket for the pairing search, in radians (2 degrees).
const BIN_RAD: f64 = 0.0349;
/// Coordinates are compared at 0.01 mm when building the wall graph.
const NODE_STEP: f64 = 0.01;

// ------------------------------------------------------------------ vectors

fn sub(a: Point, b: Point) -> Point {
    Point {
        x: a.x - b.x,
        y: a.y - b.y,
    }
}

fn add(a: Point, b: Point) -> Point {
    Point {
        x: a.x + b.x,
        y: a.y + b.y,
    }
}

fn mul(a: Point, k: f64) -> Point {
    Point { x: a.x * k, y: a.y * k }
}

fn dot(a: Point, b: Point) -> f64 {
    a.x * b.x + a.y * b.y
}

fn cross(a: Point, b: Point) -> f64 {
    a.x * b.y - a.y * b.x
}

fn len(a: Point) -> f64 {
    (a.x * a.x + a.y * a.y).sqrt()
}

fn dist(a: Point, b: Point) -> f64 {
    len(sub(a, b))
}

// ----------------------------------------------------------------- segments

/// One straight piece of an imported polyline, already in project mm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seg {
    pub a: Point,
    pub b: Point,
    /// Index into `Raw::paths`.
    pub path: usize,
    /// Index of this piece inside that path.
    pub idx: usize,
    /// Unit direction, canonical: its angle is in [0, pi).
    dir: Point,
    /// Signed distance of the line from the origin along the canonical normal.
    off: f64,
    len: f64,
}

fn make_seg(a: Point, b: Point, path: usize, idx: usize) -> Seg {
    let d = sub(b, a);
    let l = len(d);
    let mut dir = if l > 0.0 { mul(d, 1.0 / l) } else { Point { x: 1.0, y: 0.0 } };
    // Canonical direction: angle in [0, pi).
    if dir.y < 0.0 || (dir.y == 0.0 && dir.x < 0.0) {
        dir = mul(dir, -1.0);
    }
    let normal = Point { x: -dir.y, y: dir.x };
    Seg {
        a,
        b,
        path,
        idx,
        dir,
        off: dot(a, normal),
        len: l,
    }
}

/// Every straight piece on one layer, scaled to mm and offset.
pub fn segments_of_layer(raw: &Raw, layer: usize, mm_per_unit: f64, offset: Point) -> Vec<Seg> {
    let place = |p: &Point| Point {
        x: p.x * mm_per_unit + offset.x,
        y: p.y * mm_per_unit + offset.y,
    };
    let mut out = vec![];
    for (pi, path) in raw.paths.iter().enumerate() {
        if path.layer != layer {
            continue;
        }
        for (i, w) in path.pts.windows(2).enumerate() {
            let seg = make_seg(place(&w[0]), place(&w[1]), pi, i);
            if seg.len >= MIN_SEG_MM {
                out.push(seg);
            }
        }
    }
    out
}

// -------------------------------------------------------------- recognition

/// A recognized wall centerline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WallLine {
    pub a: Point,
    pub b: Point,
    pub thickness_mm: f64,
}

#[derive(Debug, Clone, Copy)]
struct Cand {
    i: usize,
    j: usize,
    /// Signed perpendicular distance from segment i to segment j.
    d: f64,
    thickness: f64,
    /// The overlapping stretch, already on the centerline.
    p0: Point,
    p1: Point,
}

fn normal_of(s: &Seg) -> Point {
    Point {
        x: -s.dir.y,
        y: s.dir.x,
    }
}

/// The wall these two segments would make, or None when they are not a pair.
fn candidate(i: usize, j: usize, si: &Seg, sj: &Seg) -> Option<Cand> {
    let ci = si.dir;
    let flip = dot(ci, sj.dir) < 0.0;
    let cj = if flip { mul(sj.dir, -1.0) } else { sj.dir };
    let off_j = if flip { -sj.off } else { sj.off };
    if cross(ci, cj).abs() > PARALLEL_SIN {
        return None;
    }
    let d = off_j - si.off;
    if d.abs() < MIN_THICKNESS_MM || d.abs() > MAX_THICKNESS_MM {
        return None;
    }
    let ni = normal_of(si);
    let span = |p: Point, q: Point| {
        let (u, v) = (dot(p, ci), dot(q, ci));
        if u <= v {
            (u, v)
        } else {
            (v, u)
        }
    };
    let (ilo, ihi) = span(si.a, si.b);
    let (jlo, jhi) = span(sj.a, sj.b);
    let lo = ilo.max(jlo);
    let hi = ihi.min(jhi);
    if hi - lo < MIN_OVERLAP_MM {
        return None;
    }
    let mid = si.off + d / 2.0;
    let base = mul(ni, mid);
    let thickness = ((d.abs() / THICKNESS_STEP_MM).round() * THICKNESS_STEP_MM)
        .clamp(MIN_THICKNESS_MM, MAX_THICKNESS_MM);
    Some(Cand {
        i,
        j,
        d,
        thickness,
        p0: add(base, mul(ci, lo)),
        p1: add(base, mul(ci, hi)),
    })
}

/// Candidate pairs, found through angle buckets so a large drawing does not
/// turn into an all-pairs comparison. Inside a bucket the segments are sorted
/// by their perpendicular offset, so the search only ever walks the window of
/// lines that could be a wall's other face.
fn candidates(segs: &[Seg]) -> Vec<Cand> {
    let bins = (std::f64::consts::PI / BIN_RAD).ceil() as i64;
    let mut buckets: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    for (k, s) in segs.iter().enumerate() {
        let ang = s.dir.y.atan2(s.dir.x).rem_euclid(std::f64::consts::PI);
        let bin = ((ang / BIN_RAD).floor() as i64).clamp(0, bins - 1);
        buckets.entry(bin).or_default().push(k);
    }
    let bin_list: Vec<i64> = buckets.keys().copied().collect();
    let mut out = vec![];
    let mut seen: BTreeSet<(usize, usize)> = BTreeSet::new();
    for bin in bin_list {
        for step in [0, 1] {
            let other_bin = (bin + step).rem_euclid(bins);
            if buckets.get(&other_bin).is_none() {
                continue;
            }
            // Only the wrap from the last bucket to the first puts nearly
            // opposite canonical directions together, and there the offset of
            // the second list reads with the sign flipped.
            let flip = step == 1 && bin == bins - 1 && other_bin == 0;
            let key_of = |k: usize| if flip { -segs[k].off } else { segs[k].off };
            let mut a: Vec<usize> = buckets[&bin].clone();
            let mut b: Vec<usize> = buckets[&other_bin].clone();
            a.sort_by(|x, y| segs[*x].off.total_cmp(&segs[*y].off));
            b.sort_by(|x, y| key_of(*x).total_cmp(&key_of(*y)));
            let mut start = 0usize;
            for &i in &a {
                let lo = segs[i].off - MAX_THICKNESS_MM;
                let hi = segs[i].off + MAX_THICKNESS_MM;
                while start < b.len() && key_of(b[start]) < lo {
                    start += 1;
                }
                for &j in &b[start..] {
                    if key_of(j) > hi {
                        break;
                    }
                    if i == j {
                        continue;
                    }
                    let key = (i.min(j), i.max(j));
                    if !seen.insert(key) {
                        continue;
                    }
                    if let Some(c) = candidate(key.0, key.1, &segs[key.0], &segs[key.1]) {
                        out.push(c);
                    }
                }
            }
        }
    }
    out.sort_by(|x, y| (x.i, x.j).cmp(&(y.i, y.j)));
    out
}

/// Does this candidate survive segment `k`'s own view? On each side the
/// nearest partner claims the stretch it covers; a partner further away that
/// covers the same stretch is shadowed by it and never becomes a wall.
fn survives(k: usize, c: &Cand, mine: &[usize], all: &[Cand], segs: &[Seg]) -> bool {
    let axis = segs[k].dir;
    let side = |cand: &Cand| if cand.i == k { cand.d.signum() } else { -cand.d.signum() };
    let range = |cand: &Cand| {
        let (u, v) = (dot(cand.p0, axis), dot(cand.p1, axis));
        if u <= v {
            (u, v)
        } else {
            (v, u)
        }
    };
    let my_side = side(c);
    let mut order: Vec<usize> = mine.to_vec();
    order.sort_by(|x, y| {
        all[*x]
            .d
            .abs()
            .total_cmp(&all[*y].d.abs())
            .then_with(|| (all[*x].i, all[*x].j).cmp(&(all[*y].i, all[*y].j)))
    });
    let mut taken: Vec<(f64, f64)> = vec![];
    for oi in order {
        let o = &all[oi];
        if side(o) != my_side {
            continue;
        }
        let (olo, ohi) = range(o);
        let shadowed = taken
            .iter()
            .any(|(tlo, thi)| ohi.min(*thi) - olo.max(*tlo) > MIN_OVERLAP_MM / 2.0);
        let is_c = o.i == c.i && o.j == c.j;
        if shadowed {
            if is_c {
                return false;
            }
            continue;
        }
        if is_c {
            return true;
        }
        taken.push((olo, ohi));
    }
    false
}

/// Wall centerlines for one set of segments, with the segments they consumed.
pub fn recognize_with_use(segs: &[Seg]) -> (Vec<WallLine>, BTreeSet<usize>) {
    let all = candidates(segs);
    let mut by_seg: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (k, c) in all.iter().enumerate() {
        by_seg.entry(c.i).or_default().push(k);
        by_seg.entry(c.j).or_default().push(k);
    }
    let empty: Vec<usize> = vec![];
    let mut walls = vec![];
    let mut used = BTreeSet::new();
    for c in &all {
        let mi = by_seg.get(&c.i).unwrap_or(&empty);
        let mj = by_seg.get(&c.j).unwrap_or(&empty);
        if !survives(c.i, c, mi, &all, segs) || !survives(c.j, c, mj, &all, segs) {
            continue;
        }
        if dist(c.p0, c.p1) < MIN_WALL_MM {
            continue;
        }
        walls.push(WallLine {
            a: c.p0,
            b: c.p1,
            thickness_mm: c.thickness,
        });
        used.insert(c.i);
        used.insert(c.j);
    }
    (walls, used)
}

pub fn recognize(segs: &[Seg]) -> Vec<WallLine> {
    recognize_with_use(segs).0
}

// ----------------------------------------------------------------- healing

fn quant(p: Point) -> (i64, i64) {
    (
        (p.x / NODE_STEP).round() as i64,
        (p.y / NODE_STEP).round() as i64,
    )
}

fn end_mut(w: &mut WallLine, e: usize) -> &mut Point {
    if e == 0 {
        &mut w.a
    } else {
        &mut w.b
    }
}

fn end_of(w: &WallLine, e: usize) -> Point {
    if e == 0 {
        w.a
    } else {
        w.b
    }
}

/// Move endpoints within `tol` of each other onto their common average.
fn snap_endpoints(walls: &mut [WallLine], tol: f64) {
    let ends: Vec<(usize, usize)> = (0..walls.len()).flat_map(|i| [(i, 0), (i, 1)]).collect();
    let mut clusters: Vec<(Point, Vec<(usize, usize)>)> = vec![];
    for (i, e) in ends {
        let p = end_of(&walls[i], e);
        match clusters.iter_mut().find(|(c, _)| dist(*c, p) <= tol) {
            Some((_, members)) => members.push((i, e)),
            None => clusters.push((p, vec![(i, e)])),
        }
    }
    for (_, members) in &clusters {
        if members.len() < 2 {
            continue;
        }
        let n = members.len() as f64;
        let mut sum = Point::default();
        for (i, e) in members {
            sum = add(sum, end_of(&walls[*i], *e));
        }
        let mid = mul(sum, 1.0 / n);
        for (i, e) in members {
            *end_mut(&mut walls[*i], *e) = mid;
        }
    }
}

fn line_intersection(a0: Point, a1: Point, b0: Point, b1: Point) -> Option<Point> {
    let da = sub(a1, a0);
    let db = sub(b1, b0);
    let den = cross(da, db);
    if den.abs() < 1e-9 {
        return None;
    }
    let t = cross(sub(b0, a0), db) / den;
    Some(add(a0, mul(da, t)))
}

/// Extend two walls that meet at an angle out to their intersection, so the
/// corner closes. Tolerance follows the walls: half of each thickness plus
/// the snap distance is exactly the gap trimming leaves at a square corner.
fn heal_corners(walls: &mut Vec<WallLine>) {
    for p in 0..walls.len() {
        for q in (p + 1)..walls.len() {
            let tol = (walls[p].thickness_mm + walls[q].thickness_mm) / 2.0 + SNAP_MM;
            let dp = sub(walls[p].b, walls[p].a);
            let dq = sub(walls[q].b, walls[q].a);
            let (lp, lq) = (len(dp), len(dq));
            if lp < 1e-9 || lq < 1e-9 {
                continue;
            }
            if (cross(mul(dp, 1.0 / lp), mul(dq, 1.0 / lq))).abs() < CORNER_SIN {
                continue;
            }
            for ep in 0..2 {
                for eq in 0..2 {
                    let (pp, pq) = (end_of(&walls[p], ep), end_of(&walls[q], eq));
                    let gap = dist(pp, pq);
                    if gap < 1e-9 || gap > tol {
                        continue;
                    }
                    let Some(x) = line_intersection(walls[p].a, walls[p].b, walls[q].a, walls[q].b) else {
                        continue;
                    };
                    if dist(x, pp) > tol || dist(x, pq) > tol {
                        continue;
                    }
                    *end_mut(&mut walls[p], ep) = x;
                    *end_mut(&mut walls[q], eq) = x;
                }
            }
        }
    }
}

fn foot_on_segment(p: Point, a: Point, b: Point) -> Option<(Point, f64)> {
    let d = sub(b, a);
    let l = len(d);
    if l < 1e-9 {
        return None;
    }
    let t = dot(sub(p, a), d) / (l * l);
    if !(0.0..=1.0).contains(&t) {
        return None;
    }
    let foot = add(a, mul(d, t));
    Some((foot, t * l))
}

/// A wall end that found no partner, but stops just short of another wall's
/// body, is dropped onto that body. The engine then splits the other wall
/// there and the loop closes.
fn heal_tees(walls: &mut Vec<WallLine>) {
    let mut counts: BTreeMap<(i64, i64), u32> = BTreeMap::new();
    for w in walls.iter() {
        *counts.entry(quant(w.a)).or_insert(0) += 1;
        *counts.entry(quant(w.b)).or_insert(0) += 1;
    }
    let mut moves: Vec<(usize, usize, Point)> = vec![];
    for i in 0..walls.len() {
        for e in 0..2 {
            let p = end_of(&walls[i], e);
            if counts.get(&quant(p)).copied().unwrap_or(0) > 1 {
                continue;
            }
            let mut best: Option<(f64, Point)> = None;
            for (j, w) in walls.iter().enumerate() {
                if i == j {
                    continue;
                }
                let tol = (walls[i].thickness_mm + w.thickness_mm) / 2.0 + SNAP_MM;
                let Some((foot, along)) = foot_on_segment(p, w.a, w.b) else {
                    continue;
                };
                let d = dist(p, foot);
                if d > tol || along < MIN_WALL_MM || len(sub(w.b, w.a)) - along < MIN_WALL_MM {
                    continue;
                }
                if best.map(|(bd, _)| d < bd).unwrap_or(true) {
                    best = Some((d, foot));
                }
            }
            if let Some((_, foot)) = best {
                moves.push((i, e, foot));
            }
        }
    }
    for (i, e, p) in moves {
        *end_mut(&mut walls[i], e) = p;
    }
}

/// Fold two collinear walls of the same thickness that meet at a node where
/// nothing else arrives. A junction node is left alone, because a wall that
/// ends inside another wall's body is a T the engine has to resolve.
fn merge_collinear(walls: &mut Vec<WallLine>) {
    loop {
        let mut counts: BTreeMap<(i64, i64), u32> = BTreeMap::new();
        for w in walls.iter() {
            *counts.entry(quant(w.a)).or_insert(0) += 1;
            *counts.entry(quant(w.b)).or_insert(0) += 1;
        }
        let mut merged = None;
        'outer: for i in 0..walls.len() {
            for j in (i + 1)..walls.len() {
                if (walls[i].thickness_mm - walls[j].thickness_mm).abs() > 1e-9 {
                    continue;
                }
                let di = sub(walls[i].b, walls[i].a);
                let dj = sub(walls[j].b, walls[j].a);
                let (li, lj) = (len(di), len(dj));
                if li < 1e-9 || lj < 1e-9 {
                    continue;
                }
                if cross(mul(di, 1.0 / li), mul(dj, 1.0 / lj)).abs() > PARALLEL_SIN {
                    continue;
                }
                for ei in 0..2 {
                    for ej in 0..2 {
                        let node = end_of(&walls[i], ei);
                        if quant(node) != quant(end_of(&walls[j], ej)) {
                            continue;
                        }
                        if counts.get(&quant(node)).copied().unwrap_or(0) != 2 {
                            continue;
                        }
                        let a = end_of(&walls[i], 1 - ei);
                        let b = end_of(&walls[j], 1 - ej);
                        merged = Some((i, j, a, b));
                        break 'outer;
                    }
                }
            }
        }
        let Some((i, j, a, b)) = merged else { return };
        walls[i] = WallLine {
            a,
            b,
            thickness_mm: walls[i].thickness_mm,
        };
        walls.remove(j);
    }
}

/// Everything after recognition: snap, close corners, resolve tees, merge.
pub fn heal(mut walls: Vec<WallLine>) -> Vec<WallLine> {
    snap_endpoints(&mut walls, SNAP_MM);
    heal_corners(&mut walls);
    snap_endpoints(&mut walls, SNAP_MM);
    heal_tees(&mut walls);
    snap_endpoints(&mut walls, SNAP_MM);
    merge_collinear(&mut walls);
    walls.retain(|w| dist(w.a, w.b) >= MIN_WALL_MM);
    walls
}

// ------------------------------------------------------------------- chains

/// Group walls into open chains so the plan uses `AddWallChain` where it can.
/// A chain only runs through nodes where exactly two walls of the same
/// thickness meet, so a junction always ends a chain.
fn chains(walls: &[WallLine]) -> Vec<Vec<usize>> {
    let mut at: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
    for (i, w) in walls.iter().enumerate() {
        at.entry(quant(w.a)).or_default().push(i);
        at.entry(quant(w.b)).or_default().push(i);
    }
    let step = |from: usize, node: Point, used: &BTreeSet<usize>| -> Option<usize> {
        let here = at.get(&quant(node))?;
        if here.len() != 2 {
            return None;
        }
        let next = *here.iter().find(|k| **k != from)?;
        if used.contains(&next) || (walls[next].thickness_mm - walls[from].thickness_mm).abs() > 1e-9 {
            return None;
        }
        Some(next)
    };
    let mut used: BTreeSet<usize> = BTreeSet::new();
    let mut out = vec![];
    for start in 0..walls.len() {
        if used.contains(&start) {
            continue;
        }
        used.insert(start);
        let mut chain = vec![start];
        // Walk forward from `b`, then backward from `a`.
        let mut cur = start;
        let mut node = walls[cur].b;
        while let Some(next) = step(cur, node, &used) {
            used.insert(next);
            chain.push(next);
            node = if quant(walls[next].a) == quant(node) {
                walls[next].b
            } else {
                walls[next].a
            };
            cur = next;
        }
        cur = start;
        node = walls[cur].a;
        while let Some(next) = step(cur, node, &used) {
            used.insert(next);
            chain.insert(0, next);
            node = if quant(walls[next].a) == quant(node) {
                walls[next].b
            } else {
                walls[next].a
            };
            cur = next;
        }
        out.push(chain);
    }
    out
}

/// Ordered points of a chain, or None when the walls do not line up.
fn chain_points(walls: &[WallLine], chain: &[usize]) -> Option<Vec<Point>> {
    if chain.len() < 2 {
        return None;
    }
    let first = walls[chain[0]];
    let second = walls[chain[1]];
    let shared = [second.a, second.b];
    let mut pts = if shared.iter().any(|p| quant(*p) == quant(first.b)) {
        vec![first.a, first.b]
    } else {
        vec![first.b, first.a]
    };
    for i in &chain[1..] {
        let w = walls[*i];
        let last = *pts.last()?;
        let next = if quant(w.a) == quant(last) {
            w.b
        } else if quant(w.b) == quant(last) {
            w.a
        } else {
            return None;
        };
        if quant(next) == quant(last) || pts.iter().any(|p| quant(*p) == quant(next)) {
            return None;
        }
        pts.push(next);
    }
    Some(pts)
}

// ---------------------------------------------------------------- the plan

fn chosen_layers(raw: &Raw, options: &ImportOptions) -> Vec<usize> {
    if options.layers.is_empty() {
        (0..raw.layers.len()).collect()
    } else {
        options
            .layers
            .iter()
            .filter_map(|name| raw.layer_index(name))
            .collect()
    }
}

/// Polylines for the pieces of `layer` that no wall used, in mm.
fn leftover_polylines(
    raw: &Raw,
    layer: usize,
    mm: f64,
    offset: Point,
    consumed: &BTreeSet<(usize, usize)>,
) -> Vec<Vec<Point>> {
    let place = |p: &Point| Point {
        x: p.x * mm + offset.x,
        y: p.y * mm + offset.y,
    };
    let mut out = vec![];
    for (pi, path) in raw.paths.iter().enumerate() {
        if path.layer != layer {
            continue;
        }
        let mut run: Vec<Point> = vec![];
        for i in 0..path.pts.len() - 1 {
            if consumed.contains(&(pi, i)) {
                if run.len() >= 2 {
                    out.push(std::mem::take(&mut run));
                } else {
                    run.clear();
                }
                continue;
            }
            if run.is_empty() {
                run.push(place(&path.pts[i]));
            }
            run.push(place(&path.pts[i + 1]));
        }
        if run.len() >= 2 {
            out.push(run);
        }
    }
    out
}

pub fn build_plan(raw: &Raw, file_name: &str, mm: f64, options: &ImportOptions) -> ImportPlan {
    let layers = chosen_layers(raw, options);
    let level_id = options.level_id.clone().unwrap_or_default();
    let offset = options.offset;
    let mut skipped = raw.skipped.clone();

    let mut wall_lines: Vec<WallLine> = vec![];
    let mut consumed: BTreeSet<(usize, usize)> = BTreeSet::new();
    if options.mode == ImportMode::Walls {
        for layer in &layers {
            let segs = segments_of_layer(raw, *layer, mm, offset);
            let (walls, used) = recognize_with_use(&segs);
            for k in used {
                consumed.insert((segs[k].path, segs[k].idx));
            }
            wall_lines.extend(walls);
        }
    }
    let before = wall_lines.len();
    let wall_lines = heal(wall_lines);
    if wall_lines.len() < before {
        skipped.push(format!(
            "{} recognized wall(s) were merged or dropped while closing corners",
            before - wall_lines.len()
        ));
    }

    let mut commands: Vec<Command> = vec![];
    let mut walls_added = 0u32;
    for chain in chains(&wall_lines) {
        match chain_points(&wall_lines, &chain) {
            Some(points) => {
                walls_added += chain.len() as u32;
                commands.push(Command::AddWallChain {
                    points,
                    closed: false,
                    thickness_mm: Some(wall_lines[chain[0]].thickness_mm),
                    level_id: options.level_id.clone(),
                });
            }
            None => {
                for i in chain {
                    let w = wall_lines[i];
                    walls_added += 1;
                    commands.push(Command::AddWall {
                        start: w.a,
                        end: w.b,
                        thickness_mm: Some(w.thickness_mm),
                        height_mm: None,
                        material_id: None,
                        level_id: options.level_id.clone(),
                    });
                }
            }
        }
    }

    let mut linework_added = 0u32;
    for layer in &layers {
        let polylines = if options.mode == ImportMode::Walls {
            leftover_polylines(raw, *layer, mm, offset, &consumed)
        } else {
            leftover_polylines(raw, *layer, mm, offset, &BTreeSet::new())
        };
        if polylines.is_empty() {
            continue;
        }
        let info = &raw.layers[*layer];
        linework_added += 1;
        commands.push(Command::AddElement {
            element: Element::Linework(Linework {
                id: String::new(),
                level_id: level_id.clone(),
                name: format!("{file_name} / {}", info.name),
                polylines,
                color: info.color.clone(),
                locked: true,
            }),
        });
    }

    ImportPlan {
        commands,
        walls: walls_added,
        linework: linework_added,
        skipped,
    }
}
