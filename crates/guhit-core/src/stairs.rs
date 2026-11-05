//! Stairs: straight, L and U (DECISIONS D44, D54). LANE L2 owns this module.
//!
//! `derive` works out every stair once (`Derived::stairs`): its flights,
//! nosings, landings, the hole it makes in the floor it arrives at and the
//! walking line. Plan, 3D, walk mode, railings, exports, tours and review
//! read that, never their own stair math.
//!
//! The frame: `origin` is the center of the first riser and the first
//! flight climbs along the rotated +y, `width_mm` wide across it. Each
//! flight's top riser lands on its landing or on the floor above, so n
//! risers make n - 1 treads straight and n - 2 for an L or U, each
//! `run_mm / treads` deep. An L turns 90 degrees on a landing as deep as
//! `landing_mm` (the width when None) along the first flight; the second
//! flight leaves the landing's side, its far side in line with the
//! landing's far edge. A U turns back on a landing that spans both flights
//! and the well between them; the second flight runs down beside the first,
//! `well_mm` clear of it.

use guhit_model::*;
use serde_json::{json, Value};

use crate::error::CoreError;
use crate::geom::*;
use crate::issues::issue;
use crate::slabs;
use crate::topo::Analysis;
use crate::validate::{finite, finite_point, in_range, require_level};

/// The fewest risers an L or U stair takes: two flights of at least two.
pub const MIN_TURNING_RISERS: u32 = 4;
/// Headroom over the walking line under this is flagged (a rule of thumb).
pub const HEADROOM_MM: f64 = 2000.0;
/// The walking line is checked for headroom at most this far apart.
const HEADROOM_STEP_MM: f64 = 100.0;
/// Under an upper level's floor and under a landing, as the 3D view draws
/// them.
const SLAB_UNDER_MM: f64 = 200.0;
/// The concrete under a flight, measured down from the nosing line.
const WAIST_MM: f64 = 200.0;

/// The rules of one stair (validate hub, `Element::Stair`).
pub fn validate(project: &Project, s: &Stair) -> Result<(), CoreError> {
    let ids = [s.id.clone()];
    require_level(project, &s.level_id)?;
    finite_point("Stair origin", s.origin, &ids)?;
    finite("Stair rotation", s.rotation_deg, &ids)?;
    in_range("stair_size", "Stair width", s.width_mm, 300.0, 10_000.0, "mm", &ids)?;
    in_range("stair_size", "Stair run", s.run_mm, 300.0, 30_000.0, "mm", &ids)?;
    if s.riser_count < 1 || s.riser_count > 100 {
        return Err(CoreError::invalid_for(
            "stair_size",
            "A stair needs between 1 and 100 risers.",
            ids.to_vec(),
        ));
    }
    if s.shape.is_straight() {
        return Ok(());
    }
    let n = s.riser_count;
    if n < MIN_TURNING_RISERS {
        return Err(CoreError::invalid_for(
            "stair_size",
            format!("An L or U stair needs at least {MIN_TURNING_RISERS} risers, two in each flight; this one has {n}."),
            ids.to_vec(),
        ));
    }
    if let Some(first) = s.first_flight_risers {
        if first < 2 || first > n - 2 {
            return Err(CoreError::invalid_for(
                "stair_size",
                format!("The first flight takes 2 to {} of the {n} risers, not {first}.", n - 2),
                ids.to_vec(),
            ));
        }
    }
    if let Some(l) = s.landing_mm {
        in_range("stair_size", "Landing depth", l, 300.0, 10_000.0, "mm", &ids)?;
    }
    in_range("stair_size", "Gap between flights", s.well_mm, 0.0, 10_000.0, "mm", &ids)
}

// ---------------------------------------------------------------- frames

/// One flight in its own frame: `origin` is the center of its first riser,
/// `up` points up the flight, `right` across it to the right facing up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlightFrame {
    pub origin: Point,
    pub up: Point,
    pub right: Point,
    pub width: f64,
    /// Its treads' going, first riser to top riser.
    pub run: f64,
    pub treads: u32,
    /// Risers below its first riser: its first nosing is `below + 1` risers up.
    pub below: u32,
}

impl FlightFrame {
    /// A plan point `x` across (to the right) and `y` up the flight.
    pub fn at(&self, x: f64, y: f64) -> Point {
        add(self.origin, add(scale(self.right, x), scale(self.up, y)))
    }

    /// (across, up) of a plan point.
    pub fn local(&self, p: Point) -> (f64, f64) {
        let d = sub(p, self.origin);
        (dot(d, self.right), dot(d, self.up))
    }

    /// First riser left, first riser right, top right, top left (facing
    /// up): counter-clockwise.
    pub fn outline(&self) -> Vec<Point> {
        let hw = self.width / 2.0;
        vec![self.at(-hw, 0.0), self.at(hw, 0.0), self.at(hw, self.run), self.at(-hw, self.run)]
    }

    /// Height of the nosing line `y` up the flight, `riser` per riser: one
    /// riser over the floor it starts from at the first nosing, the floor
    /// or landing it arrives at by the top riser. Clamped to the flight.
    pub fn nosing_z(&self, riser: f64, y: f64) -> f64 {
        let going = if self.treads > 0 { self.run / self.treads as f64 } else { self.run.max(1.0) };
        riser * (self.below as f64 + 1.0 + y.clamp(0.0, self.run) / going)
    }
}

/// The frames of a derived stair's flights, read back from their outlines
/// (first riser left, first riser right, top right, top left) and nosings.
pub fn frames(g: &StairGeometry) -> Vec<FlightFrame> {
    g.flights
        .iter()
        .filter(|f| f.outline.len() == 4)
        .map(|f| {
            let (b0, b1, t0) = (f.outline[0], f.outline[1], f.outline[3]);
            let below = f
                .nosings
                .first()
                .map(|n| ((n.z / g.riser_mm.max(1e-9)).round() as u32).saturating_sub(1))
                .unwrap_or(0);
            FlightFrame {
                origin: lerp(b0, b1, 0.5),
                up: f.up,
                right: unit(sub(b1, b0)).unwrap_or(pt(1.0, 0.0)),
                width: dist(b0, b1),
                run: dot(sub(t0, b0), f.up),
                treads: f.nosings.len() as u32,
                below,
            }
        })
        .collect()
}

/// The end of a stair where people step off: the middle of the last
/// flight's top riser and the direction up it.
pub fn head(g: &StairGeometry) -> Option<(Point, Point)> {
    let f = frames(g).pop()?;
    Some((f.at(0.0, f.run), f.up))
}

/// The foot of a stair: the middle of the first riser and the direction up.
pub fn foot(g: &StairGeometry) -> Option<(Point, Point)> {
    let f = *frames(g).first()?;
    Some((f.origin, f.up))
}

/// The arrival edge of a stair (the top riser of its last flight), from its
/// left end to its right end facing up.
pub fn arrival_edge(g: &StairGeometry) -> Option<(Point, Point)> {
    let f = frames(g).pop()?;
    let hw = f.width / 2.0;
    Some((f.at(-hw, f.run), f.at(hw, f.run)))
}

/// Every plan piece a stair stands on: its flights, landings and (U) the
/// well between the flights, each a convex counter-clockwise polygon.
pub fn pieces(g: &StairGeometry) -> Vec<Vec<Point>> {
    let mut out: Vec<Vec<Point>> = g.flights.iter().map(|f| f.outline.clone()).collect();
    out.extend(g.landings.iter().map(|l| l.polygon.clone()));
    out
}

/// True when plan point `p` is on the stair: inside a flight or a landing.
pub fn covers(g: &StairGeometry, p: Point) -> bool {
    pieces(g).iter().any(|poly| point_in_polygon(p, poly))
}

// -------------------------------------------------------------- geometry

/// The level whose floor is within 300 mm of the stair's top (DECISIONS
/// D50), the one the stair arrives at.
pub fn arrives_at<'a>(project: &'a Project, s: &Stair) -> Option<&'a Level> {
    slabs::stair_top_level(project, s)
}

/// Risers in the first flight of an L or U: as given, else half rounded up,
/// kept to 2 .. n - 2.
pub fn first_flight(s: &Stair) -> u32 {
    let n = s.riser_count;
    s.first_flight_risers.unwrap_or(n.div_ceil(2)).clamp(2, n.saturating_sub(2).max(2))
}

/// Treads of a stair: risers less one per flight, at least one.
pub fn tread_count(s: &Stair) -> u32 {
    s.riser_count.saturating_sub(flights_of(s)).max(1)
}

/// The flights a stair is drawn with: an L or U under 4 risers is drawn
/// straight (validate refuses one, but derive never fails).
fn flights_of(s: &Stair) -> u32 {
    if s.riser_count < MIN_TURNING_RISERS {
        1
    } else {
        s.shape.flights()
    }
}

/// An axis-aligned rectangle in the first flight's frame, counter-clockwise.
fn rect(f: &FlightFrame, x0: f64, x1: f64, y0: f64, y1: f64) -> Vec<Point> {
    let (lo, hi) = (x0.min(x1), x0.max(x1));
    let (b, t) = (y0.min(y1), y0.max(y1));
    vec![f.at(lo, b), f.at(hi, b), f.at(hi, t), f.at(lo, t)]
}

fn flight_of(frame: &FlightFrame, riser: f64) -> Flight {
    let going = if frame.treads > 0 { frame.run / frame.treads as f64 } else { frame.run };
    let hw = frame.width / 2.0;
    Flight {
        outline: frame.outline(),
        nosings: (0..frame.treads)
            .map(|k| {
                let y = going * k as f64;
                Nosing {
                    a: frame.at(-hw, y),
                    b: frame.at(hw, y),
                    z: riser * (frame.below + k + 1) as f64,
                }
            })
            .collect(),
        up: frame.up,
    }
}

fn v3(p: Point, z: f64) -> Vec3 {
    Vec3 { x: p.x, y: p.y, z }
}

/// Appends `p` unless it repeats the last point in plan and height.
fn push_walk(line: &mut Vec<Vec3>, p: Vec3) {
    if line
        .last()
        .is_some_and(|q| (q.x - p.x).hypot(q.y - p.y) < 1e-6 && (q.z - p.z).abs() < 1e-6)
    {
        return;
    }
    line.push(p);
}

/// Up a flight along its center: half a going before its first riser at the
/// floor or landing it starts from, every tread's middle at its top, and
/// (for the last flight) half a going past the top riser on the floor.
fn walk_flight(line: &mut Vec<Vec3>, f: &FlightFrame, riser: f64, start: bool, end: bool) {
    let going = if f.treads > 0 { f.run / f.treads as f64 } else { f.run };
    if start {
        push_walk(line, v3(f.at(0.0, -going / 2.0), riser * f.below as f64));
    }
    for k in 0..f.treads {
        push_walk(line, v3(f.at(0.0, going * (k as f64 + 0.5)), riser * (f.below + k + 1) as f64));
    }
    if end {
        push_walk(line, v3(f.at(0.0, f.run + going / 2.0), riser * (f.below + f.treads + 1) as f64));
    }
}

/// The geometry of one stair (`Derived::stairs`), z above its level's floor.
/// None when its level is missing or its numbers are not usable.
pub fn geometry(project: &Project, s: &Stair) -> Option<StairGeometry> {
    let level = project.levels.iter().find(|l| l.id == s.level_id)?;
    let n = s.riser_count.clamp(1, 200);
    let (w, run) = (s.width_mm, s.run_mm);
    let rise = level.height_mm;
    if !(w.is_finite() && run.is_finite() && rise.is_finite() && w >= 1.0 && run >= 1.0 && rise > 0.0) {
        return None;
    }
    if !(is_finite(s.origin) && s.rotation_deg.is_finite()) {
        return None;
    }
    let riser = rise / n as f64;
    let treads = tread_count(s);
    let going = run / treads as f64;
    let a = s.rotation_deg.to_radians();
    let first = FlightFrame {
        origin: s.origin,
        up: pt(-a.sin(), a.cos()),
        right: pt(a.cos(), a.sin()),
        width: w,
        run,
        treads,
        below: 0,
    };
    let hw = w / 2.0;
    let mut frames = vec![];
    let mut landings = vec![];
    let mut opening: Vec<Point>;
    let mut walk = vec![];
    if flights_of(s) == 1 {
        frames.push(first);
        opening = first.outline();
        walk_flight(&mut walk, &first, riser, true, true);
    } else {
        let f1 = first_flight(s);
        let (t1, t2) = (f1 - 1, n - f1 - 1);
        let (run1, run2) = (going * t1 as f64, going * t2 as f64);
        let depth = s.landing_mm.filter(|l| l.is_finite() && *l > 0.0).unwrap_or(w);
        // +1: the second flight leaves to the right of the first.
        let side = if s.turn == Turn::Right { 1.0 } else { -1.0 };
        let one = FlightFrame { run: run1, treads: t1, ..first };
        let landing_z = riser * f1 as f64;
        let mid_landing = run1 + depth / 2.0;
        match s.shape {
            StairShape::U => {
                let well = s.well_mm.max(0.0);
                let x2 = side * (w + well);
                let two = FlightFrame {
                    origin: one.at(x2, run1),
                    up: scale(one.up, -1.0),
                    right: scale(one.right, -1.0),
                    width: w,
                    run: run2,
                    treads: t2,
                    below: f1,
                };
                let (lx0, lx1) = ((-hw).min(x2 - hw), hw.max(x2 + hw));
                landings.push(Landing { polygon: rect(&one, lx0, lx1, run1, run1 + depth), z: landing_z });
                // The hole: both flights, the landing, and the well between
                // the flights as far as both run.
                let low = (run1 - run2).max(0.0).min(run1);
                let well_rect = rect(&one, hw * side, x2 - hw * side, low, run1);
                let parts = [
                    rect(&one, -hw, hw, 0.0, run1),
                    rect(&one, x2 - hw, x2 + hw, run1 - run2, run1),
                    rect(&one, lx0, lx1, run1, run1 + depth),
                    well_rect,
                ];
                opening = union_in_frame(&one, &parts);
                walk_flight(&mut walk, &one, riser, true, false);
                let enter = (run1 + going / 2.0).min(mid_landing);
                push_walk(&mut walk, v3(one.at(0.0, enter), landing_z));
                push_walk(&mut walk, v3(one.at(0.0, mid_landing), landing_z));
                push_walk(&mut walk, v3(one.at(x2, mid_landing), landing_z));
                push_walk(&mut walk, v3(one.at(x2, enter), landing_z));
                walk_flight(&mut walk, &two, riser, false, true);
                frames.push(one);
                frames.push(two);
            }
            _ => {
                // L: the landing continues the first flight; the second
                // flight leaves its side, its far side in line with the
                // landing's far edge.
                let yc = run1 + depth - hw;
                let two = FlightFrame {
                    origin: one.at(side * hw, yc),
                    up: scale(one.right, side),
                    right: scale(one.up, -side),
                    width: w,
                    run: run2,
                    treads: t2,
                    below: f1,
                };
                landings.push(Landing { polygon: rect(&one, -hw, hw, run1, run1 + depth), z: landing_z });
                let parts = [
                    rect(&one, -hw, hw, 0.0, run1 + depth),
                    rect(&one, side * hw, side * (hw + run2), yc - hw, yc + hw),
                ];
                opening = union_in_frame(&one, &parts);
                walk_flight(&mut walk, &one, riser, true, false);
                let enter = (run1 + going / 2.0).min(yc.max(run1));
                push_walk(&mut walk, v3(one.at(0.0, enter), landing_z));
                push_walk(&mut walk, v3(one.at(0.0, yc.max(run1)), landing_z));
                let leave = (hw - going / 2.0).max(0.0);
                push_walk(&mut walk, v3(one.at(side * leave, yc.max(run1)), landing_z));
                walk_flight(&mut walk, &two, riser, false, true);
                frames.push(one);
                frames.push(two);
            }
        }
    }
    let arrives = arrives_at(project, s);
    if arrives.is_none() {
        opening = vec![];
    }
    Some(StairGeometry {
        stair_id: s.id.clone(),
        level_id: s.level_id.clone(),
        arrives_at: arrives.map(|l| l.id.clone()),
        tread_mm: going,
        riser_mm: riser,
        flights: frames.iter().map(|f| flight_of(f, riser)).collect(),
        landings,
        opening,
        walk_line: walk,
    })
}

/// The outline of a union of rectangles that are axis-aligned in `f`'s
/// frame, counter-clockwise; the largest piece when they fall apart.
fn union_in_frame(f: &FlightFrame, parts: &[Vec<Point>]) -> Vec<Point> {
    let rects: Vec<(f64, f64, f64, f64)> = parts
        .iter()
        .filter(|p| p.len() == 4)
        .map(|p| {
            let l: Vec<(f64, f64)> = p.iter().map(|q| f.local(*q)).collect();
            let xs = l.iter().map(|q| q.0);
            let ys = l.iter().map(|q| q.1);
            (
                xs.clone().fold(f64::INFINITY, f64::min),
                xs.fold(f64::NEG_INFINITY, f64::max),
                ys.clone().fold(f64::INFINITY, f64::min),
                ys.fold(f64::NEG_INFINITY, f64::max),
            )
        })
        .filter(|r| r.1 - r.0 > 1e-6 && r.3 - r.2 > 1e-6)
        .collect();
    let loops = crate::roofs::ortho::rect_union(&rects);
    loops
        .into_iter()
        .max_by(|a, b| signed_area(a).total_cmp(&signed_area(b)))
        .map(|l| l.iter().map(|q| f.at(q.x, q.y)).collect())
        .unwrap_or_default()
}

/// `Derived::stairs`, one per stair whose level exists, in element order.
pub fn derive(project: &Project, _analysis: &Analysis) -> Vec<StairGeometry> {
    project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Stair(s) => geometry(project, s),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------- review

fn mm_as_m(v: f64) -> String {
    format!("{:.2} m", v / 1000.0)
}

/// What stands over a plan point: the undersides of the floors, stairs and
/// roofs above, mm above the project's zero.
struct Overhead<'a> {
    project: &'a Project,
    analysis: &'a Analysis,
    stairs: &'a [StairGeometry],
    roofs: Vec<RoofGeometry>,
}

impl Overhead<'_> {
    fn elevation(&self, level_id: &str) -> Option<f64> {
        self.project.levels.iter().find(|l| l.id == level_id).map(|l| l.elevation_mm)
    }

    /// The lowest underside over `p` that is above `z`, with what it is.
    fn lowest_above(&self, p: Point, z: f64, own: &str) -> Option<(f64, &'static str)> {
        let mut best: Option<(f64, &'static str)> = None;
        let mut take = |under: f64, what: &'static str| {
            if under > z + 1.0 && best.is_none_or(|(b, _)| under < b) {
                best = Some((under, what));
            }
        };
        let mut levels: Vec<&Level> = self.project.levels.iter().collect();
        levels.sort_by(|a, b| a.elevation_mm.total_cmp(&b.elevation_mm));
        for (i, l) in levels.iter().enumerate() {
            if l.elevation_mm <= z {
                continue;
            }
            // Its floor: its buildings, the roof deck over the level below
            // (D26), and its floor, balcony and deck slabs; not its voids,
            // nor the openings of stairs that arrive on it.
            let on = |id: &str| {
                self.analysis
                    .level(id)
                    .is_some_and(|a| a.footprints.iter().any(|(poly, _)| point_in_polygon(p, poly)))
            };
            let below = i.checked_sub(1).map(|j| levels[j].id.as_str());
            let mut floor = on(&l.id) || below.is_some_and(on);
            let mut void = false;
            for e in &self.project.elements {
                if let Element::Slab(sl) = e {
                    if sl.level_id == l.id && point_in_polygon(p, &sl.polygon) {
                        if sl.kind == SlabKind::Void {
                            void = true;
                        } else {
                            floor = true;
                            take(l.elevation_mm + sl.top_offset_mm - sl.thickness_mm, "the floor above");
                        }
                    }
                }
            }
            let well = self
                .stairs
                .iter()
                .any(|g| g.arrives_at.as_deref() == Some(l.id.as_str()) && point_in_polygon(p, &g.opening));
            if floor && !void && !well {
                take(l.elevation_mm - SLAB_UNDER_MM, "the floor above");
            }
        }
        for g in self.stairs.iter().filter(|g| g.stair_id != own) {
            let Some(base) = self.elevation(&g.level_id) else { continue };
            for f in frames(g) {
                let (x, y) = f.local(p);
                if x.abs() <= f.width / 2.0 && (0.0..=f.run).contains(&y) {
                    take(base + f.nosing_z(g.riser_mm, y) - g.riser_mm - WAIST_MM, "another stair");
                }
            }
            for l in &g.landings {
                if point_in_polygon(p, &l.polygon) {
                    take(base + l.z - SLAB_UNDER_MM, "another stair's landing");
                }
            }
        }
        for r in &self.roofs {
            let Some(base) = self.elevation(&r.level_id) else { continue };
            for plane in &r.planes {
                let flat: Vec<Point> = plane.iter().map(|q| pt(q.x, q.y)).collect();
                if point_in_polygon(p, &flat) {
                    if let Some(top) = plane_height(plane, p) {
                        take(base + top - r.vertical_mm, "the roof");
                    }
                }
            }
        }
        best
    }
}

/// Height of a planar polygon over plan point `p`.
pub fn plane_height(plane: &[Vec3], p: Point) -> Option<f64> {
    let n = plane.len();
    // The normal from the polygon (Newell), robust to collinear corners.
    let (mut nx, mut ny, mut nz) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (a, b) = (plane[i], plane[(i + 1) % n]);
        nx += (a.y - b.y) * (a.z + b.z);
        ny += (a.z - b.z) * (a.x + b.x);
        nz += (a.x - b.x) * (a.y + b.y);
    }
    if nz.abs() < 1e-9 {
        return None;
    }
    let o = plane[0];
    Some(o.z - (nx * (p.x - o.x) + ny * (p.y - o.y)) / nz)
}

/// Stair review items: `stair_headroom_low` (under 2000 mm clear over the
/// walking line) and `stair_landing_short` (an L or U landing shallower than
/// the stair is wide). Rules of thumb for the designer, never a code check.
/// `stair_top_no_floor` and `stair_landing_blocked` are in `slabs::review`.
pub fn review(project: &Project, analysis: &Analysis, stairs: &[StairGeometry]) -> Vec<Issue> {
    let mut out = vec![];
    let overhead = Overhead { project, analysis, stairs, roofs: crate::roofs::derive(project, analysis) };
    for g in stairs {
        let Some(Element::Stair(s)) = project.elements.iter().find(|e| e.id() == &g.stair_id) else {
            continue;
        };
        if !s.shape.is_straight() && flights_of(s) == 2 {
            let depth = s.landing_mm.unwrap_or(s.width_mm);
            if depth + 1.0 < s.width_mm {
                out.push(issue(
                    "stair_landing_short",
                    Severity::Warning,
                    format!(
                        "The landing of this stair is {} deep, less than the stair's {} width. A landing at least as deep as the stair is wide lets people turn with a load.",
                        mm_as_m(depth),
                        mm_as_m(s.width_mm)
                    ),
                    vec![s.id.clone()],
                ));
            }
        }
        if let Some((clear, what, step)) = headroom(&overhead, g) {
            if clear < HEADROOM_MM {
                out.push(issue(
                    "stair_headroom_low",
                    Severity::Warning,
                    format!(
                        "Over this stair, {what} is only {} above the treads at about step {step}. About {} clear lets people walk up without ducking.",
                        mm_as_m(clear),
                        mm_as_m(HEADROOM_MM)
                    ),
                    vec![s.id.clone()],
                ));
            }
        }
    }
    out
}

/// The least headroom along a stair's walking line: the clearance, what is
/// over it, and the step it is at (risers up, 1 based).
fn headroom(o: &Overhead, g: &StairGeometry) -> Option<(f64, &'static str, u32)> {
    let base = o.elevation(&g.level_id)?;
    let mut least: Option<(f64, &'static str, u32)> = None;
    for w in g.walk_line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = (b.x - a.x).hypot(b.y - a.y);
        let steps = ((len / HEADROOM_STEP_MM).ceil() as usize).max(1);
        for k in 0..=steps {
            let t = k as f64 / steps as f64;
            let p = pt(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
            let z = a.z + (b.z - a.z) * t;
            // Past the top riser the walker stands on the floor above.
            if !covers(g, p) {
                continue;
            }
            if let Some((under, what)) = o.lowest_above(p, base + z, &g.stair_id) {
                let clear = under - (base + z);
                if least.is_none_or(|(c, _, _)| clear < c) {
                    let step = ((z / g.riser_mm.max(1.0)).round() as u32).max(1);
                    least = Some((clear, what, step));
                }
            }
        }
    }
    least
}

/// A stair for `Query::Describe` and `ListElements`: tread count, tread and
/// riser size, where it arrives, its shape and landing (D44, D54).
pub fn describe(project: &Project, s: &Stair) -> Value {
    let g = geometry(project, s);
    let level = project.levels.iter().find(|l| l.id == s.level_id).map(|l| l.name.clone());
    let r1 = |v: f64| (v * 10.0).round() / 10.0;
    json!({
        "id": s.id,
        "kind": "stair",
        "level": level,
        "shape": s.shape,
        "turn": if s.shape.is_straight() { Value::Null } else { json!(s.turn) },
        "origin": { "x": r1(s.origin.x), "y": r1(s.origin.y) },
        "rotation_deg": (s.rotation_deg * 100.0).round() / 100.0,
        "width_mm": r1(s.width_mm),
        "run_mm": r1(s.run_mm),
        "riser_count": s.riser_count,
        "tread_count": tread_count(s),
        "tread_mm": g.as_ref().map(|g| r1(g.tread_mm)),
        "riser_mm": g.as_ref().map(|g| r1(g.riser_mm)),
        "first_flight_risers": if flights_of(s) == 2 { json!(first_flight(s)) } else { Value::Null },
        "landing_mm": if flights_of(s) == 2 { json!(r1(s.landing_mm.unwrap_or(s.width_mm))) } else { Value::Null },
        "well_mm": if s.shape == StairShape::U { json!(r1(s.well_mm)) } else { Value::Null },
        "arrives_at": arrives_at(project, s).map(|l| l.name.clone()),
        "railing": s.railing.map(|r| json!({ "height_mm": r1(r.height_mm), "style": r.style })),
    })
}
