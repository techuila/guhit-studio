//! Slabs and railings (DECISIONS D50, docs/CONTRACT.md "Slabs and
//! railings"): outline rules, the floor a level stands on, what voids,
//! balconies and decks do to areas, every railing run, and the slab and
//! stair review items.
//!
//! Areas of overlaps are exact for any simple polygon: a region is a signed
//! sum of convex pieces (the fan triangles of each outline), and the
//! intersection of two regions is the pairwise convex clip of their pieces
//! with the product of their signs. Unions go by inclusion and exclusion.

use std::collections::BTreeMap;

use guhit_model::*;

use crate::error::CoreError;
use crate::geom::*;
use crate::issues::issue;
use crate::topo::{Analysis, Face};

/// Outlines under this area are a slip, not a slab (0.01 m2, as rooms).
pub const MIN_SLAB_AREA_MM2: f64 = 10_000.0;
/// Two slabs sharing less than this much area only touch (1 cm2).
const OVERLAP_MM2: f64 = 100.0;
/// A void may stick out of the floor by this much before it is refused.
const VOID_OUTSIDE_MM2: f64 = 10_000.0;
/// A wall runs along an edge within this many degrees of parallel.
const ALONG_DEG: f64 = 2.0;
/// ... and with its centerline this far past half its thickness from it.
const ALONG_SLACK_MM: f64 = 50.0;
/// Open stretches shorter than this are not railed.
const MIN_OPEN_MM: f64 = 100.0;
/// A balcony or deck this high above the lowest floor gets a railing note.
/// A rule of thumb for the designer, not a regulation.
pub const RAIL_DROP_MM: f64 = 750.0;
/// The level a stair arrives at starts within this of the stair's top, as
/// the 3D view finds it (`STAIR_TOP_MATCH_MM` in src/viewer3d/walk/stairs.ts).
pub const STAIR_TOP_MATCH_MM: f64 = 300.0;
/// The floor a stair lands on is checked this far past its top riser.
const LANDING_MM: f64 = 300.0;

// ------------------------------------------------------------------ regions

/// A plan region as a signed sum of convex counter-clockwise pieces.
#[derive(Debug, Clone, Default)]
pub struct Region(Vec<(f64, Vec<Point>)>);

impl Region {
    /// The region inside one simple outline, either orientation.
    pub fn of(poly: &[Point]) -> Region {
        let mut out = Region::default();
        out.add(poly, 1.0);
        out
    }

    /// Disjoint outlines: their regions add up.
    pub fn of_all<'a>(polys: impl IntoIterator<Item = &'a Vec<Point>>) -> Region {
        let mut out = Region::default();
        for p in polys {
            out.add(p, 1.0);
        }
        out
    }

    /// Add `sign` times the region inside `poly`. A clockwise outline is
    /// taken the right way round.
    fn add(&mut self, poly: &[Point], sign: f64) {
        if poly.len() < 3 {
            return;
        }
        let flip = if signed_area(poly) < 0.0 { -1.0 } else { 1.0 };
        for i in 1..poly.len() - 1 {
            let t = [poly[0], poly[i], poly[i + 1]];
            let a = signed_area(&t) * flip;
            if a.abs() < 1e-9 {
                continue;
            }
            let piece = if signed_area(&t) > 0.0 {
                t.to_vec()
            } else {
                vec![t[0], t[2], t[1]]
            };
            self.0.push((sign * a.signum(), piece));
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn area(&self) -> f64 {
        self.0.iter().map(|(s, p)| s * signed_area(p)).sum()
    }

    pub fn intersect(&self, other: &Region) -> Region {
        let mut out = Region::default();
        for (sa, pa) in &self.0 {
            let (alo, ahi) = bbox(pa);
            for (sb, pb) in &other.0 {
                let (blo, bhi) = bbox(pb);
                if alo.x >= bhi.x || blo.x >= ahi.x || alo.y >= bhi.y || blo.y >= ahi.y {
                    continue;
                }
                let c = clip_convex(pa, pb);
                if c.len() >= 3 && signed_area(&c) > 1e-9 {
                    out.0.push((sa * sb, c));
                }
            }
        }
        out
    }
}

/// Sutherland-Hodgman: `subject` clipped by the convex counter-clockwise
/// `clip`. Both convex, so the result is their exact intersection.
fn clip_convex(subject: &[Point], clip: &[Point]) -> Vec<Point> {
    let mut out: Vec<Point> = subject.to_vec();
    let n = clip.len();
    for i in 0..n {
        if out.is_empty() {
            break;
        }
        let (a, b) = (clip[i], clip[(i + 1) % n]);
        let side = |p: Point| cross(sub(b, a), sub(p, a));
        let input = std::mem::take(&mut out);
        let m = input.len();
        for j in 0..m {
            let (p, q) = (input[j], input[(j + 1) % m]);
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                out.push(lerp(p, q, t));
            }
        }
    }
    out
}

/// Area of the union of `regions`, each a union of disjoint outlines.
pub fn union_area(regions: &[&Region]) -> f64 {
    let regions: Vec<&Region> = regions.iter().copied().filter(|r| !r.is_empty()).collect();
    let n = regions.len();
    let mut total = 0.0;
    for mask in 1u32..(1 << n) {
        let mut acc: Option<Region> = None;
        for (i, r) in regions.iter().enumerate() {
            if mask & (1 << i) != 0 {
                acc = Some(match acc {
                    None => (*r).clone(),
                    Some(a) => a.intersect(r),
                });
            }
        }
        let sign = if mask.count_ones() % 2 == 1 { 1.0 } else { -1.0 };
        total += sign * acc.map(|r| r.area()).unwrap_or(0.0);
    }
    total
}

/// Area of `r` that lies inside the union of `others`.
pub fn covered_area(r: &Region, others: &[&Region]) -> f64 {
    let clipped: Vec<Region> = others.iter().map(|o| r.intersect(o)).collect();
    let refs: Vec<&Region> = clipped.iter().collect();
    union_area(&refs)
}

/// Area of the intersection of two outlines.
pub fn overlap_area(a: &[Point], b: &[Point]) -> f64 {
    Region::of(a).intersect(&Region::of(b)).area()
}

// ------------------------------------------------------------------ outlines

/// The outline as the engine keeps it: no point within 1 mm of the one
/// before (the closing duplicate included), counter-clockwise.
pub fn normalize(poly: &[Point]) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(poly.len());
    for p in poly {
        if out.last().map(|l| dist(*l, *p) < JOIN_EPS).unwrap_or(false) {
            continue;
        }
        out.push(*p);
    }
    while out.len() > 1 && dist(out[0], out[out.len() - 1]) < JOIN_EPS {
        out.pop();
    }
    if signed_area(&out) < 0.0 {
        out.reverse();
    }
    out
}

pub fn kind_name(kind: SlabKind) -> &'static str {
    match kind {
        SlabKind::Floor => "floor slab",
        SlabKind::Balcony => "balcony",
        SlabKind::Deck => "deck",
        SlabKind::Void => "void",
    }
}

fn level_name(project: &Project, level_id: &str) -> String {
    project
        .levels
        .iter()
        .find(|l| l.id == level_id)
        .map(|l| l.name.clone())
        .unwrap_or_else(|| "this level".to_string())
}

fn m2(mm2: f64) -> String {
    format!("{:.2} m2", mm2 / 1.0e6)
}

fn slabs_on<'a>(project: &'a Project, level_id: &'a str) -> impl Iterator<Item = &'a Slab> + 'a {
    project.elements.iter().filter_map(move |e| match e {
        Element::Slab(s) if s.level_id == level_id => Some(s),
        _ => None,
    })
}

/// The level directly below `level_id` by elevation.
pub fn level_below<'a>(project: &'a Project, level_id: &str) -> Option<&'a Level> {
    let this = project.levels.iter().find(|l| l.id == level_id)?;
    project
        .levels
        .iter()
        .filter(|l| l.id != this.id && l.elevation_mm < this.elevation_mm)
        .max_by(|a, b| a.elevation_mm.total_cmp(&b.elevation_mm))
}

fn lowest_elevation(project: &Project) -> f64 {
    project
        .levels
        .iter()
        .map(|l| l.elevation_mm)
        .fold(f64::INFINITY, f64::min)
}

fn footprint_region(analysis: &Analysis, level_id: &str) -> Region {
    analysis
        .level(level_id)
        .map(|l| Region::of_all(l.footprints.iter().map(|(p, _)| p)))
        .unwrap_or_default()
}

/// The clear floor of every closed face on the level, rooms or not: inside
/// the wall faces, less wall islands. Faces never overlap, so inside a
/// footprint what is not in them is wall.
fn faces_region(analysis: &Analysis, level_id: &str) -> Region {
    let mut out = Region::default();
    if let Some(l) = analysis.level(level_id) {
        for f in &l.faces {
            out.add(&f.net, 1.0);
            for h in &f.holes {
                out.add(h, -1.0);
            }
        }
    }
    out
}

/// Rules one slab must meet among the others, run after a command that adds
/// or changes it: a simple outline, no overlap with a slab of its group,
/// and a void over floor. `analysis` is of the project after the command.
pub fn check_slab(project: &Project, analysis: &Analysis, s: &Slab) -> Result<(), CoreError> {
    let noun = kind_name(s.kind);
    let ids = vec![s.id.clone()];
    if s.polygon.len() < 3 || !is_simple(&s.polygon) {
        return Err(CoreError::invalid_for(
            "slab_outline",
            format!("The outline of this {noun} crosses itself. Draw it so no two edges cross."),
            ids,
        ));
    }
    let area = signed_area(&s.polygon).abs();
    if area < MIN_SLAB_AREA_MM2 {
        return Err(CoreError::invalid_for(
            "slab_outline",
            format!("This {noun} is only {}. A slab needs at least 0.01 m2.", m2(area)),
            ids,
        ));
    }

    // Floors, balconies and decks are all floor: none may cover another.
    // Voids may not cover voids. A void over floor is what a void is for.
    let mine = Region::of(&s.polygon);
    let is_void = s.kind == SlabKind::Void;
    for o in slabs_on(project, &s.level_id) {
        if o.id == s.id || (o.kind == SlabKind::Void) != is_void {
            continue;
        }
        let shared = mine.intersect(&Region::of(&o.polygon)).area();
        if shared > OVERLAP_MM2 {
            let hint = if is_void {
                "Voids may not overlap: draw one void that covers both."
            } else {
                "Floors, balconies and decks on one level may not overlap: move one or redraw its outline."
            };
            return Err(CoreError::invalid_for(
                "slab_overlap",
                format!(
                    "This {noun} overlaps a {} on {} by {}. {hint}",
                    kind_name(o.kind),
                    level_name(project, &s.level_id),
                    m2(shared)
                ),
                vec![s.id.clone(), o.id.clone()],
            ));
        }
    }

    if is_void {
        // A void opens the floor onto the level below. On the lowest level
        // it would open onto the ground.
        let Some(below_level) = level_below(project, &s.level_id) else {
            return Err(CoreError::invalid_for(
                "void_no_level_below",
                format!(
                    "{} is the lowest level, so a void there would open onto the ground. A void goes on the level above a double-height space. For an open area here use a deck, or leave the floor out.",
                    level_name(project, &s.level_id)
                ),
                ids,
            ));
        };
        let own = footprint_region(analysis, &s.level_id);
        let below = footprint_region(analysis, &below_level.id);
        let slabs = Region::of_all(
            slabs_on(project, &s.level_id)
                .filter(|o| o.kind != SlabKind::Void)
                .map(|o| &o.polygon),
        );
        let outside = area - covered_area(&mine, &[&own, &below, &slabs]);
        if outside > VOID_OUTSIDE_MM2 {
            return Err(CoreError::invalid_for(
                "void_outside_floor",
                format!(
                    "This void hangs {} outside the floor of {}. A void cuts floor away, so it must lie over the level's floor, a floor, balcony or deck slab, or the roof of the level below.",
                    m2(outside),
                    level_name(project, &s.level_id)
                ),
                ids,
            ));
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ areas

/// What slabs do to the areas of one level. Built only for levels that have
/// slabs, so a project without them keeps its totals to the last digit.
pub struct LevelSlabs {
    voids: Region,
    /// Balconies and decks: open floor, not floor area.
    open: Region,
    floors: Region,
}

pub struct SlabAreas {
    levels: BTreeMap<Id, LevelSlabs>,
}

impl SlabAreas {
    pub fn new(project: &Project) -> SlabAreas {
        let mut levels: BTreeMap<Id, LevelSlabs> = BTreeMap::new();
        for e in &project.elements {
            let Element::Slab(s) = e else { continue };
            let l = levels.entry(s.level_id.clone()).or_insert_with(|| LevelSlabs {
                voids: Region::default(),
                open: Region::default(),
                floors: Region::default(),
            });
            let target = match s.kind {
                SlabKind::Void => &mut l.voids,
                SlabKind::Balcony | SlabKind::Deck => &mut l.open,
                SlabKind::Floor => &mut l.floors,
            };
            target.add(&s.polygon, 1.0);
        }
        SlabAreas { levels }
    }

    fn room_region(f: &Face) -> Region {
        let mut r = Region::of(&f.net);
        for h in &f.holes {
            r.add(h, -1.0);
        }
        r
    }

    /// The room's net area less the part a void on its level covers.
    pub fn room_area(&self, f: &Face) -> f64 {
        match self.levels.get(&f.level_id) {
            Some(l) if !l.voids.is_empty() => {
                let r = Self::room_region(f);
                (f.area_mm2 - covered_area(&r, &[&l.voids])).max(0.0)
            }
            _ => f.area_mm2,
        }
    }

    /// The part of the room that is floor area: less voids, balconies and
    /// decks.
    pub fn room_floor_area(&self, f: &Face) -> f64 {
        match self.levels.get(&f.level_id) {
            Some(l) if !(l.voids.is_empty() && l.open.is_empty()) => {
                let r = Self::room_region(f);
                (f.area_mm2 - covered_area(&r, &[&l.voids, &l.open])).max(0.0)
            }
            _ => f.area_mm2,
        }
    }

    /// How much of the room's net area the voids on `level_id` (a level
    /// above it) lie over, mm2.
    pub fn void_over(&self, f: &Face, level_id: &str) -> f64 {
        match self.levels.get(level_id) {
            Some(l) if !l.voids.is_empty() => covered_area(&Self::room_region(f), &[&l.voids]),
            _ => 0.0,
        }
    }

    /// Gross area of a level: its footprints and floor slabs, less voids,
    /// balconies and decks. None when the level has no slabs: then it is
    /// the footprints' sum, as it always was.
    ///
    /// Gross area counts the walls themselves (GA P0-1): a void or a
    /// balcony drawn to the wall centerlines takes off only its clear part,
    /// inside the walls or outside the building, never the wall strip under
    /// it.
    pub fn gross_area(&self, analysis: &Analysis, level_id: &str) -> Option<f64> {
        let l = self.levels.get(level_id)?;
        let fp = footprint_region(analysis, level_id);
        let all = union_area(&[&fp, &l.floors, &l.voids, &l.open]);
        let open = union_area(&[&l.voids, &l.open]);
        // Inside the building, what is not wall is the faces' clear floor.
        let in_building = covered_area(&fp, &[&l.voids, &l.open]);
        let clear = covered_area(&faces_region(analysis, level_id), &[&l.voids, &l.open]);
        let under_walls = (in_building - clear).max(0.0);
        Some((all - (open - under_walls)).max(0.0))
    }
}

// ------------------------------------------------------------------ railings

/// Parts (0..1 along a-b) that a wall of `walls` runs along. A wall's
/// outline reaches half its thickness past each end.
fn wall_cover(walls: &[&Wall], a: Point, b: Point) -> Vec<(f64, f64)> {
    walls
        .iter()
        .filter_map(|w| along_cover(w.start, w.end, w.thickness_mm / 2.0 + ALONG_SLACK_MM, w.thickness_mm / 2.0, a, b))
        .collect()
}

/// The part of a-b that segment c-d runs along: within 2 degrees of
/// parallel, both ends within `reach` of the line, `extend` past its ends.
fn along_cover(c: Point, d: Point, reach: f64, extend: f64, a: Point, b: Point) -> Option<(f64, f64)> {
    let len = dist(a, b);
    let u = unit(sub(b, a))?;
    let cd = unit(sub(d, c))?;
    if cross(u, cd).abs() > ALONG_DEG.to_radians().sin() {
        return None;
    }
    if cross(u, sub(c, a)).abs() > reach || cross(u, sub(d, a)).abs() > reach {
        return None;
    }
    let (tc, td) = (dot(sub(c, a), u), dot(sub(d, a), u));
    let (lo, hi) = ((tc.min(td) - extend).max(0.0), (tc.max(td) + extend).min(len));
    (hi > lo).then(|| (lo / len, hi / len))
}

/// The part of a-b inside the convex counter-clockwise `poly`
/// (Cyrus-Beck).
fn clip_segment(a: Point, b: Point, poly: &[Point]) -> Option<(f64, f64)> {
    let d = sub(b, a);
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    let n = poly.len();
    for i in 0..n {
        let (e0, e1) = (poly[i], poly[(i + 1) % n]);
        let edge = sub(e1, e0);
        // Inside is to the left of each edge: cross(edge, p - e0) >= 0.
        let num = cross(edge, sub(a, e0));
        let den = cross(edge, d);
        if den.abs() < 1e-12 {
            if num < 0.0 {
                return None;
            }
            continue;
        }
        let t = -num / den;
        if den > 0.0 {
            lo = lo.max(t);
        } else {
            hi = hi.min(t);
        }
        if lo >= hi {
            return None;
        }
    }
    Some((lo, hi))
}

/// The parts of a-b inside the simple `poly` or within `tol` of its
/// outline. Cut where a-b crosses an edge or passes a corner, each piece
/// judged by its middle.
fn inside_or_on(a: Point, b: Point, poly: &[Point], tol: f64) -> Vec<(f64, f64)> {
    let len = dist(a, b);
    let Some(u) = unit(sub(b, a)) else {
        return vec![];
    };
    let mut cuts = vec![0.0, 1.0];
    let n = poly.len();
    for i in 0..n {
        let (c, d) = (poly[i], poly[(i + 1) % n]);
        if let Some(x) = line_intersect(a, sub(b, a), c, sub(d, c)) {
            let t = dot(sub(x, a), u) / len;
            let (_, off) = project_on_segment(x, c, d);
            if (0.0..=1.0).contains(&t) && off < 1e-6 {
                cuts.push(t);
            }
        }
        let (along, off) = project_on_segment(c, a, b);
        if off <= tol + 1e-6 {
            cuts.push((along / len).clamp(0.0, 1.0));
        }
    }
    cuts.sort_by(|x, y| x.total_cmp(y));
    cuts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
    cuts.windows(2)
        .filter(|w| {
            let m = lerp(a, b, (w[0] + w[1]) / 2.0);
            point_in_polygon(m, poly) || dist_to_boundary(m, poly) <= tol
        })
        .map(|w| (w[0], w[1]))
        .collect()
}

/// What is left of 0..1 once `closed` is taken out, without stretches
/// under `MIN_OPEN_MM`.
fn open_from(mut closed: Vec<(f64, f64)>, len: f64) -> Vec<(f64, f64)> {
    closed.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut open = vec![];
    let mut at = 0.0;
    for (lo, hi) in closed {
        if lo > at {
            open.push((at, lo));
        }
        at = f64::max(at, hi);
    }
    if at < 1.0 {
        open.push((at, 1.0));
    }
    open.retain(|(lo, hi)| (hi - lo) * len >= MIN_OPEN_MM);
    open
}

/// The open stretches of a path as runs of plan points. `closed_parts`
/// gives the parts (0..1) of an edge that need no rail. `closed` paths wrap
/// round; stretches that meet at a corner are one run, and a run all the
/// way round repeats its first point.
fn open_runs(path: &[Point], closed: bool, closed_parts: &dyn Fn(Point, Point) -> Vec<(f64, f64)>) -> Vec<Vec<Point>> {
    let n = path.len();
    let edges = if closed { n } else { n.saturating_sub(1) };
    // (edge, t0, t1)
    let mut pieces: Vec<(usize, f64, f64)> = vec![];
    for i in 0..edges {
        let (a, b) = (path[i], path[(i + 1) % n]);
        if dist(a, b) < 1e-9 {
            continue;
        }
        for (t0, t1) in open_from(closed_parts(a, b), dist(a, b)) {
            pieces.push((i, t0, t1));
        }
    }
    let at = |i: usize, t: f64| lerp(path[i], path[(i + 1) % n], t);
    let joins = |prev: &(usize, f64, f64), next: &(usize, f64, f64)| {
        prev.2 >= 1.0 - 1e-9 && next.1 <= 1e-9 && (prev.0 + 1) % n == next.0
    };
    let mut runs: Vec<Vec<Point>> = vec![];
    for (k, p) in pieces.iter().enumerate() {
        if k == 0 || !joins(&pieces[k - 1], p) {
            runs.push(vec![at(p.0, p.1)]);
        }
        runs.last_mut().expect("a run").push(at(p.0, p.2));
    }
    if closed && pieces.len() > 1 && joins(&pieces[pieces.len() - 1], &pieces[0]) {
        if runs.len() > 1 {
            // The last run carries on into the first.
            let first = runs.remove(0);
            let last = runs.last_mut().expect("a run");
            last.extend(first.into_iter().skip(1));
        } else if let Some(run) = runs.first_mut() {
            // Open all the way round: end where it started.
            let start = run[0];
            if dist(*run.last().expect("points"), start) > 1e-6 {
                run.push(start);
            }
        }
    }
    runs
}

/// The flight's frame: (start of the run, direction up the flight, across
/// to the right), as the 3D view and the exports place a stair.
pub fn stair_frame(s: &Stair) -> (Point, Point, Point) {
    let a = s.rotation_deg.to_radians();
    (s.origin, pt(-a.sin(), a.cos()), pt(a.cos(), a.sin()))
}

/// The level a stair arrives at: above its own, starting within 300 mm of
/// the stair's top, the closest one.
pub fn stair_top_level<'a>(project: &'a Project, s: &Stair) -> Option<&'a Level> {
    let level = project.levels.iter().find(|l| l.id == s.level_id)?;
    let top = level.elevation_mm + level.height_mm;
    project
        .levels
        .iter()
        .filter(|l| l.id != level.id && l.elevation_mm > level.elevation_mm)
        .filter(|l| (l.elevation_mm - top).abs() <= STAIR_TOP_MATCH_MM)
        .min_by(|a, b| (a.elevation_mm - top).abs().total_cmp(&(b.elevation_mm - top).abs()))
}

/// The flight's rectangle: bottom left, bottom right, top right, top left.
pub fn stair_rect(s: &Stair) -> [Point; 4] {
    let (o, up, across) = stair_frame(s);
    let hw = s.width_mm / 2.0;
    let b0 = sub(o, scale(across, hw));
    let b1 = add(o, scale(across, hw));
    [b0, b1, add(b1, scale(up, s.run_mm)), add(b0, scale(up, s.run_mm))]
}

/// A rectangle in a flight's frame: `across` from its center line
/// (negative to the left), `up` from its first riser. Counter-clockwise.
fn flight_box(f: &crate::stairs::FlightFrame, across: (f64, f64), up: (f64, f64)) -> [Point; 4] {
    [f.at(across.0, up.0), f.at(across.1, up.0), f.at(across.1, up.1), f.at(across.0, up.1)]
}

/// The convex pieces of a stair's opening: its flights, its landings, and
/// (U) the well between the flights.
fn well_pieces(g: &StairGeometry) -> Vec<Vec<Point>> {
    let mut out = crate::stairs::pieces(g);
    let f = crate::stairs::frames(g);
    if let (Some(one), Some(two)) = (f.first(), f.get(1)) {
        if dot(one.up, two.up) < -0.5 {
            let (x2, _) = one.local(two.origin);
            let side = x2.signum();
            let hw = one.width / 2.0;
            let low = (one.run - two.run).max(0.0);
            if (x2 - 2.0 * side * hw).abs() > 1.0 {
                let well = flight_box(one, (side * hw, x2 - side * hw), (low, one.run));
                let mut w = well.to_vec();
                if signed_area(&w) < 0.0 {
                    w.reverse();
                }
                out.push(w);
            }
        }
    }
    out
}

fn centroid(poly: &[Point]) -> Point {
    let n = poly.len().max(1) as f64;
    let s = poly.iter().fold(pt(0.0, 0.0), |acc, p| add(acc, *p));
    scale(s, 1.0 / n)
}

/// A convex counter-clockwise polygon grown by `d` (mitred).
fn grow_convex(poly: &[Point], d: f64) -> Vec<Point> {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (prev, cur, next) = (poly[(i + n - 1) % n], poly[i], poly[(i + 1) % n]);
            let d1 = unit(sub(cur, prev)).unwrap_or(pt(0.0, 0.0));
            let d2 = unit(sub(next, cur)).unwrap_or(pt(0.0, 0.0));
            let (n1, n2) = (pt(d1.y, -d1.x), pt(d2.y, -d2.x));
            let bis = unit(add(n1, n2)).unwrap_or(n2);
            add(cur, scale(bis, d / dot(bis, n1).max(0.35)))
        })
        .collect()
}

/// The walls of a level.
pub(crate) fn walls_on<'a>(project: &'a Project, level_id: &str) -> Vec<&'a Wall> {
    walls_of_level(project, level_id)
}

/// Parts (0..1 along a-b) that a wall of `walls` runs along.
pub(crate) fn wall_cover_of(walls: &[&Wall], a: Point, b: Point) -> Vec<(f64, f64)> {
    wall_cover(walls, a, b)
}

/// The open stretches of a path (`open_runs`).
pub(crate) fn open_runs_of(path: &[Point], closed: bool, closed_parts: &dyn Fn(Point, Point) -> Vec<(f64, f64)>) -> Vec<Vec<Point>> {
    open_runs(path, closed, closed_parts)
}

/// The rails along the sides of a stair's flights and landings
/// (`stair_side`) before walls take their share, each with the flight
/// whose nosing line its foot follows. A rail runs up a flight and on along
/// the landing it arrives at, flat there; the next flight starts its own.
/// The inner sides of a U whose flights are under 50 mm apart get none (a
/// wall between them is usual).
fn side_paths(g: &StairGeometry) -> Vec<(Vec<Point>, crate::stairs::FlightFrame)> {
    let f = crate::stairs::frames(g);
    if f.len() == 1 {
        let hw = f[0].width / 2.0;
        return vec![
            (vec![f[0].at(-hw, 0.0), f[0].at(-hw, f[0].run)], f[0]),
            (vec![f[0].at(hw, 0.0), f[0].at(hw, f[0].run)], f[0]),
        ];
    }
    let (Some(one), Some(two), Some(landing)) = (f.first().copied(), f.get(1).copied(), g.landings.first()) else {
        return vec![];
    };
    let hw = one.width / 2.0;
    // Which way the second flight lies, in the first flight's frame; in the
    // second flight's own frame its outer side is at -side, its inner +side.
    let (x2, _) = one.local(two.origin);
    let side = if x2 >= 0.0 { 1.0 } else { -1.0 };
    let lp: Vec<(f64, f64)> = landing.polygon.iter().map(|p| one.local(*p)).collect();
    let ytop = lp.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    let (outer2, inner2) = (-side * hw, side * hw);
    let mut out = vec![];
    if dot(one.up, two.up) < -0.5 {
        // U: out round the landing, in along the well.
        let far = lp.iter().map(|p| p.0 * side).fold(f64::NEG_INFINITY, f64::max) * side;
        out.push((vec![one.at(-side * hw, 0.0), one.at(-side * hw, ytop), one.at(far, ytop), one.at(far, one.run)], one));
        out.push((vec![two.at(outer2, 0.0), two.at(outer2, two.run)], two));
        let inner = x2 - side * hw;
        if (inner - side * hw).abs() >= 50.0 {
            out.push((vec![one.at(side * hw, 0.0), one.at(side * hw, one.run), one.at(inner, one.run)], one));
            out.push((vec![two.at(inner2, 0.0), two.at(inner2, two.run)], two));
        }
    } else {
        // L: the outer side runs on round the landing; the inner side
        // stops where the second flight starts.
        out.push((vec![one.at(-side * hw, 0.0), one.at(-side * hw, ytop), one.at(side * hw, ytop)], one));
        out.push((vec![two.at(outer2, 0.0), two.at(outer2, two.run)], two));
        let near = two.at(inner2, 0.0);
        let (_, ylow) = one.local(near);
        if ylow > 1.0 {
            out.push((vec![one.at(side * hw, 0.0), one.at(side * hw, ylow)], one));
        }
        out.push((vec![near, two.at(inner2, two.run)], two));
    }
    out
}

/// The opening of a stair from the far end of its arrival edge round to
/// the near end: the guard round the well, open where people step off.
fn well_path(g: &StairGeometry) -> Vec<Point> {
    let Some((l, r)) = crate::stairs::arrival_edge(g) else { return vec![] };
    let poly = &g.opening;
    let n = poly.len();
    let Some(i) = (0..n).find(|&i| {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        dist_point_segment(l, a, b) < 1.0 && dist_point_segment(r, a, b) < 1.0
    }) else {
        return vec![];
    };
    let a = poly[i];
    // Along the edge from a: the arrival edge's near end s, then its far end e.
    let (s, e) = if dist(a, l) <= dist(a, r) { (l, r) } else { (r, l) };
    let mut path = vec![e];
    for k in 1..=n {
        path.push(poly[(i + k) % n]);
    }
    path.push(s);
    path.dedup_by(|x, y| dist(*x, *y) < 1e-6);
    path
}

/// What closes an edge, level by level: walls, the edges of floor,
/// balcony and deck slabs, the openings of stairs that arrive there, and
/// voids (docs/CONTRACT.md, "Railings").
struct Closers<'a> {
    walls: BTreeMap<&'a str, Vec<&'a Wall>>,
    slabs: BTreeMap<&'a str, Vec<&'a Slab>>,
    /// Stairs by the level they arrive at, with their geometry.
    arriving: BTreeMap<String, Vec<StairGeometry>>,
}

impl<'a> Closers<'a> {
    fn new(project: &'a Project) -> Closers<'a> {
        let mut c = Closers {
            walls: BTreeMap::new(),
            slabs: BTreeMap::new(),
            arriving: BTreeMap::new(),
        };
        for e in &project.elements {
            match e {
                Element::Wall(w) => c.walls.entry(w.level_id.as_str()).or_default().push(w),
                Element::Slab(s) => c.slabs.entry(s.level_id.as_str()).or_default().push(s),
                Element::Stair(s) => {
                    if let Some(g) = crate::stairs::geometry(project, s) {
                        if let Some(top) = g.arrives_at.clone() {
                            c.arriving.entry(top).or_default().push(g);
                        }
                    }
                }
                _ => {}
            }
        }
        c
    }

    fn walls(&self, level: &str) -> &[&'a Wall] {
        self.walls.get(level).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Parts of the edge a-b of slab `s` that need no rail: along a wall;
    /// for a floor, balcony or deck, along another one on the level (an L
    /// drawn as two rectangles); for a void, across the arrival end of a
    /// stair arriving on the level (where people step off), across its
    /// opening, and along the opening's rim where the opening is outside the
    /// void (two openings side by side need no rail between them).
    fn slab_edge(&self, s: &Slab, a: Point, b: Point) -> Vec<(f64, f64)> {
        let mut closed = wall_cover(self.walls(&s.level_id), a, b);
        if s.kind == SlabKind::Void {
            for g in self.arriving.get(&s.level_id).into_iter().flatten() {
                let tol = ALONG_SLACK_MM;
                if let Some(f) = crate::stairs::frames(g).pop() {
                    let hw = f.width / 2.0;
                    let arrival = flight_box(&f, (-hw - tol, hw + tol), (f.run - tol, f.run + tol));
                    closed.extend(clip_segment(a, b, &arrival));
                }
                // Each convex piece of the opening: the flights, the landing
                // and the well of a U.
                for well in well_pieces(g) {
                    let grown = grow_convex(&well, tol);
                    let Some((lo, hi)) = clip_segment(a, b, &grown) else {
                        continue;
                    };
                    let mid = lerp(a, b, (lo + hi) / 2.0);
                    if dist_to_boundary(mid, &well) > tol {
                        // Across the opening: the rail would hang over the flight.
                        closed.extend(clip_segment(a, b, &well));
                    } else if dist_to_boundary(mid, &g.opening) <= tol {
                        // Along the rim. The void's inside is left of a-b.
                        let center = centroid(&well);
                        if cross(sub(b, a), sub(center, a)) < 0.0 {
                            closed.push((lo, hi));
                        }
                    }
                }
            }
        } else {
            for o in self.slabs.get(s.level_id.as_str()).into_iter().flatten() {
                if o.id == s.id || o.kind == SlabKind::Void {
                    continue;
                }
                let n = o.polygon.len();
                for i in 0..n {
                    closed.extend(along_cover(o.polygon[i], o.polygon[(i + 1) % n], ALONG_SLACK_MM, 0.0, a, b));
                }
            }
        }
        closed
    }

    /// Parts of a stair's well guard on `level` that need none: along a
    /// wall, or inside or on a void there (the void's own rail guards it).
    fn well_guard(&self, level: &str, a: Point, b: Point) -> Vec<(f64, f64)> {
        let mut closed = wall_cover(self.walls(level), a, b);
        for v in self.slabs.get(level).into_iter().flatten() {
            if v.kind == SlabKind::Void {
                closed.extend(inside_or_on(a, b, &v.polygon, ALONG_SLACK_MM));
            }
        }
        closed
    }
}

/// Open stretches of a slab's outline, with no regard to its railing.
pub fn slab_open_runs(project: &Project, s: &Slab) -> Vec<Vec<Point>> {
    let closers = Closers::new(project);
    open_runs(&s.polygon, true, &|a, b| closers.slab_edge(s, a, b))
}

fn flat(points: &[Point], z: f64) -> Vec<Vec3> {
    points.iter().map(|p| Vec3 { x: p.x, y: p.y, z }).collect()
}

/// Every railing run of the project (`Derived::railings`).
pub fn railings(project: &Project) -> Vec<RailingRun> {
    let mut out = vec![];
    let closers = Closers::new(project);
    for e in &project.elements {
        match e {
            Element::Slab(s) => {
                let Some(r) = s.railing else { continue };
                let z = if s.kind == SlabKind::Void { 0.0 } else { s.top_offset_mm };
                for run in open_runs(&s.polygon, true, &|a, b| closers.slab_edge(s, a, b)) {
                    out.push(RailingRun {
                        host_id: s.id.clone(),
                        level_id: s.level_id.clone(),
                        kind: RailingRunKind::SlabEdge,
                        points: flat(&run, z),
                        height_mm: r.height_mm,
                        style: r.style,
                    });
                }
            }
            Element::Stair(s) => {
                let Some(r) = s.railing else { continue };
                let Some(g) = crate::stairs::geometry(project, s) else { continue };
                // The foot of a handrail is on the nosing line: one riser up
                // at a flight's first nosing, its landing or the floor above
                // at its top, flat along a landing.
                for (side, frame) in side_paths(&g) {
                    for run in open_runs(&side, false, &|a, b| wall_cover(closers.walls(&s.level_id), a, b)) {
                        out.push(RailingRun {
                            host_id: s.id.clone(),
                            level_id: s.level_id.clone(),
                            kind: RailingRunKind::StairSide,
                            points: run
                                .iter()
                                .map(|p| Vec3 { x: p.x, y: p.y, z: frame.nosing_z(g.riser_mm, frame.local(*p).1) })
                                .collect(),
                            height_mm: r.height_mm,
                            style: r.style,
                        });
                    }
                }
                // Round the opening on the floor it arrives at, all but the
                // end where people step off. Parts inside or on a void go:
                // the void's own rail guards there.
                if let Some(top) = stair_top_level(project, s) {
                    for run in open_runs(&well_path(&g), false, &|a, b| closers.well_guard(&top.id, a, b)) {
                        out.push(RailingRun {
                            host_id: s.id.clone(),
                            level_id: top.id.clone(),
                            kind: RailingRunKind::StairWell,
                            points: flat(&run, 0.0),
                            height_mm: r.height_mm,
                            style: r.style,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    out
}

// ------------------------------------------------------------------ review

fn run_length(run: &[Point]) -> f64 {
    run.windows(2).map(|w| dist(w[0], w[1])).sum()
}

fn mm_as_m(v: f64) -> String {
    format!("{:.2} m", v / 1000.0)
}

/// Review items for slabs and stairs (docs/CONTRACT.md, "Slabs and
/// railings"). Suggestions, never statements about codes.
pub fn review(project: &Project, analysis: &Analysis) -> Vec<Issue> {
    let mut issues = vec![];
    let lowest = lowest_elevation(project);
    for e in &project.elements {
        match e {
            Element::Slab(s) if s.railing.is_none() && s.kind != SlabKind::Floor => {
                let Some(level) = project.levels.iter().find(|l| l.id == s.level_id) else {
                    continue;
                };
                let drop = match s.kind {
                    SlabKind::Void => level_below(project, &s.level_id).map(|b| level.elevation_mm - b.elevation_mm),
                    _ => Some(level.elevation_mm + s.top_offset_mm - lowest),
                };
                let Some(drop) = drop.filter(|d| *d >= RAIL_DROP_MM) else {
                    continue;
                };
                let open: f64 = slab_open_runs(project, s).iter().map(|r| run_length(r)).sum();
                if open < MIN_OPEN_MM {
                    continue;
                }
                let (what, below) = match s.kind {
                    SlabKind::Void => ("This void", "to the floor below"),
                    SlabKind::Balcony => ("This balcony", "to the ground"),
                    _ => ("This deck", "to the ground"),
                };
                issues.push(issue(
                    "slab_no_railing",
                    Severity::Warning,
                    format!(
                        "{what} on {} has {} of open edge with a {} drop {below} and no railing. A guard rail along the open edge keeps people from falling.",
                        level.name,
                        mm_as_m(open),
                        mm_as_m(drop)
                    ),
                    vec![s.id.clone()],
                ));
            }
            Element::Stair(s) => {
                if let Some(message) = stair_top_problem(project, analysis, s) {
                    issues.push(issue("stair_top_no_floor", Severity::Warning, message, vec![s.id.clone()]));
                }
                for (ids, message) in stair_landings(project, analysis, s) {
                    issues.push(issue("stair_landing_blocked", Severity::Warning, message, ids));
                }
            }
            _ => {}
        }
    }
    issues.extend(objects_over_openings(project));
    issues.extend(door_swings(project));
    issues
}

/// Objects that stand on the floor, the ones a void, a stair opening or a
/// door leaf can catch: not lights, devices, units, meters or tanks, and
/// not a rug.
fn stands_on_floor(a: &Asset) -> bool {
    use AssetCategory::*;
    matches!(a.category, Furniture | Sanitary | Kitchen | Appliance | Plant | Vehicle)
        && a.elevation_mm < 300.0
        && a.height_mm >= 30.0
}

/// The plan rectangle an object covers.
fn footprint(a: &Asset) -> Vec<Point> {
    let (s, c) = a.rotation_deg.to_radians().sin_cos();
    let (hw, hd) = (a.width_mm / 2.0, a.depth_mm / 2.0);
    [(-hw, -hd), (hw, -hd), (hw, hd), (-hw, hd)]
        .iter()
        .map(|(x, y)| pt(a.position.x + x * c - y * s, a.position.y + x * s + y * c))
        .collect()
}

/// An object standing where its level has no floor: over a void, or over
/// the opening of a stair that arrives there.
fn objects_over_openings(project: &Project) -> Vec<Issue> {
    let mut out = vec![];
    let arriving: Vec<(StairGeometry, Id)> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Stair(s) => crate::stairs::geometry(project, s).and_then(|g| g.arrives_at.clone().map(|l| (g, l))),
            _ => None,
        })
        .collect();
    for e in &project.elements {
        let Element::Asset(a) = e else { continue };
        if !stands_on_floor(a) {
            continue;
        }
        let level = level_name(project, &a.level_id);
        let over_void = slabs_on(project, &a.level_id)
            .find(|v| v.kind == SlabKind::Void && point_in_polygon(a.position, &v.polygon));
        let over_well = arriving
            .iter()
            .find(|(g, top)| top == &a.level_id && point_in_polygon(a.position, &g.opening));
        let (what, other) = match (over_void, over_well) {
            (Some(v), _) => ("a void", v.id.clone()),
            (None, Some((g, _))) => ("the opening of a stair", g.stair_id.clone()),
            _ => continue,
        };
        out.push(issue(
            "asset_over_opening",
            Severity::Warning,
            format!(
                "{} on {level} stands over {what}, where there is no floor under it. Move it onto the floor.",
                a.name
            ),
            vec![a.id.clone(), other],
        ));
    }
    out
}

/// The area a swing door's leaves sweep: one quarter circle per leaf, from
/// the wall face on the side it swings to (as the plan draws it).
fn swing_areas(w: &Wall, o: &Opening) -> Vec<Vec<Point>> {
    let Some(u) = unit(sub(w.end, w.start)) else {
        return vec![];
    };
    let nrm = if o.flip_side { scale(perp_left(u), -1.0) } else { perp_left(u) };
    let c = add(w.start, scale(u, o.offset_mm));
    let half = o.width_mm / 2.0;
    let leaves: Vec<(Point, Point, f64)> = match o.style {
        OpeningStyle::SwingDouble => vec![
            (sub(c, scale(u, half)), u, half),
            (add(c, scale(u, half)), scale(u, -1.0), half),
        ],
        OpeningStyle::SwingSingle | OpeningStyle::Casement => {
            if o.flip_hinge {
                vec![(add(c, scale(u, half)), scale(u, -1.0), o.width_mm)]
            } else {
                vec![(sub(c, scale(u, half)), u, o.width_mm)]
            }
        }
        _ => vec![],
    };
    leaves
        .into_iter()
        .map(|(hinge, along, r)| {
            let hp = add(hinge, scale(nrm, w.thickness_mm / 2.0));
            let mut sector = vec![hp];
            for k in 0..=8 {
                let t = (k as f64 / 8.0) * std::f64::consts::FRAC_PI_2;
                sector.push(add(hp, scale(add(scale(along, t.cos()), scale(nrm, t.sin())), r)));
            }
            sector
        })
        .collect()
}

/// Objects inside the sweep of a door's leaf: the door would hit them.
fn door_swings(project: &Project) -> Vec<Issue> {
    /// Less than this much overlap is a touch, not a clash (0.02 m2).
    const CLASH_MM2: f64 = 20_000.0;
    let mut out = vec![];
    let assets: Vec<&Asset> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Asset(a) if stands_on_floor(a) => Some(a),
            _ => None,
        })
        .collect();
    for e in &project.elements {
        let Element::Opening(o) = e else { continue };
        if o.opening_type != OpeningType::Door {
            continue;
        }
        let Some(Element::Wall(w)) = project.elements.iter().find(|x| x.id() == &o.wall_id) else {
            continue;
        };
        let areas = swing_areas(w, o);
        if areas.is_empty() {
            continue;
        }
        let reach = o.width_mm + w.thickness_mm;
        let center = add(w.start, scale(unit(sub(w.end, w.start)).unwrap_or_default(), o.offset_mm));
        for a in assets.iter().filter(|a| a.level_id == w.level_id) {
            if dist(a.position, center) > reach + a.width_mm.max(a.depth_mm) {
                continue;
            }
            let fp = Region::of(&footprint(a));
            let hit: f64 = areas.iter().map(|s| Region::of(s).intersect(&fp).area()).sum();
            if hit > CLASH_MM2 {
                out.push(issue(
                    "door_swing_blocked",
                    Severity::Warning,
                    format!(
                        "{} stands where this door swings open, so the door would hit it. Move it, or turn the door to swing the other way.",
                        a.name
                    ),
                    vec![o.id.clone(), a.id.clone()],
                ));
            }
        }
    }
    out
}

/// A wall right at either end of a flight: at the top on the level it
/// arrives at (a dead-end landing), at the foot on its own level. A top
/// landing at least as deep as the stair is wide (900 mm at the least) and
/// 900 mm clear at the foot are rules of thumb here, not a regulation.
fn stair_landings(project: &Project, analysis: &Analysis, s: &Stair) -> Vec<(Vec<Id>, String)> {
    let Some(g) = crate::stairs::geometry(project, s) else { return vec![] };
    let frames = crate::stairs::frames(&g);
    let (Some(first), Some(last)) = (frames.first().copied(), frames.last().copied()) else {
        return vec![];
    };
    let hw = s.width_mm / 2.0;
    let top_depth = s.width_mm.max(900.0);
    let foot_depth = 900.0;
    // Kept 50 mm in from the sides, so a wall the flight runs along is no
    // landing wall.
    let inset = ALONG_SLACK_MM.min(hw / 2.0);
    let mut out = vec![];
    let ends = [
        (g.arrives_at.clone(), last, (last.run, last.run + top_depth), true),
        (Some(s.level_id.clone()), first, (-foot_depth, 0.0), false),
    ];
    for (level, frame, along, top) in ends {
        let (run, up, origin) = (frame.run, frame.up, frame.origin);
        let depth = along.1 - along.0;
        let Some(level) = level else { continue };
        let landing = Region::of(&flight_box(&frame, (inset - hw, hw - inset), along));
        // How far the nearest wall stands from the end of the flight.
        let mut nearest: Option<(f64, Id)> = None;
        for w in walls_of_level(project, &level) {
            let Some(t) = analysis.wall(&w.id) else { continue };
            if t.outline.len() < 3 {
                continue;
            }
            let hit = landing.intersect(&Region::of(&t.outline));
            if hit.area() <= 10_000.0 {
                continue;
            }
            // Wall outlines are convex, so the positive pieces are the overlap.
            for (_, piece) in hit.0.iter().filter(|(sign, _)| *sign > 0.0) {
                for p in piece {
                    let from_end = if top {
                        dot(sub(*p, origin), up) - run
                    } else {
                        -dot(sub(*p, origin), up)
                    };
                    if nearest.as_ref().map(|(d, _)| from_end < *d).unwrap_or(true) {
                        nearest = Some((from_end.max(0.0), w.id.clone()));
                    }
                }
            }
        }
        if let Some((d, wall)) = nearest {
            let message = if top {
                format!(
                    "The top of this stair meets a wall {} past its last step on {}. Leave a landing at least {} deep, as wide as the stair.",
                    mm_as_m(d),
                    level_name(project, &level),
                    mm_as_m(depth)
                )
            } else {
                format!(
                    "The first step of this stair is {} from a wall. Leave at least {} clear in front of it to step on.",
                    mm_as_m(d),
                    mm_as_m(depth)
                )
            };
            out.push((vec![s.id.clone(), wall], message));
        }
    }
    out
}

fn walls_of_level<'a>(project: &'a Project, level_id: &str) -> Vec<&'a Wall> {
    project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) if w.level_id == level_id => Some(w),
            _ => None,
        })
        .collect()
}

/// Why the top of a stair meets no floor, or None when it lands on one.
fn stair_top_problem(project: &Project, analysis: &Analysis, s: &Stair) -> Option<String> {
    let level = project.levels.iter().find(|l| l.id == s.level_id)?;
    let top = level.elevation_mm + level.height_mm;
    let Some(arrive) = stair_top_level(project, s) else {
        let next = project
            .levels
            .iter()
            .filter(|l| l.id != level.id && l.elevation_mm > level.elevation_mm)
            .min_by(|a, b| a.elevation_mm.total_cmp(&b.elevation_mm));
        return Some(match next {
            Some(n) => format!(
                "This stair rises {}, but {} starts {} up. Make the level heights meet, or the stair will not reach the floor.",
                mm_as_m(level.height_mm),
                n.name,
                mm_as_m(n.elevation_mm - level.elevation_mm)
            ),
            None => format!(
                "This stair rises {} above {}, the top level, where there is no floor. Add a level above, or a slab where it lands.",
                mm_as_m(top - level.elevation_mm),
                level.name
            ),
        });
    };
    // Past the top riser of the last flight.
    let (head, up) = crate::stairs::geometry(project, s).as_ref().and_then(crate::stairs::head)?;
    let landing = add(head, scale(up, LANDING_MM));
    let on_slab = |void: bool| {
        slabs_on(project, &arrive.id)
            .filter(|o| (o.kind == SlabKind::Void) == void)
            .any(|o| point_in_polygon(landing, &o.polygon))
    };
    let on_footprint = |level_id: &str| {
        analysis
            .level(level_id)
            .map(|l| l.footprints.iter().any(|(p, _)| point_in_polygon(landing, p)))
            .unwrap_or(false)
    };
    let in_void = on_slab(true);
    // The level's own floor, its slabs, or the roof deck of the stair's
    // level under it (D26).
    let on_floor = on_footprint(&arrive.id) || on_footprint(&level.id) || on_slab(false);
    if in_void {
        Some(format!(
            "The top of this stair lands in a void on {}. Move the stair or the void so it lands on floor.",
            arrive.name
        ))
    } else if !on_floor {
        Some(format!(
            "The top of this stair lands outside the floor of {}. Extend the floor there with a slab, or turn the stair.",
            arrive.name
        ))
    } else {
        None
    }
}
