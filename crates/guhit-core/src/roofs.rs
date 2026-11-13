//! Roofs as elements (DECISIONS D54): the roof over the top floor, and
//! carport and lanai roofs. LANE L2 owns this module.
//!
//! `derive` works out every roof once (`Derived::roofs`): its planes, ridges,
//! hips, valleys, eaves and rakes, gable-end infill, fascia and gutter runs
//! and its areas. Pipes, the 3D view, exports, sections and review read it,
//! never their own roof math.
//!
//! - Hip roofs on square-cornered outlines (rectangles, L, T and U) come
//!   from the straight skeleton. With every side at one pitch and every
//!   corner square, the skeleton roof's height is the pitch times the
//!   distance to the outline measured square to it (the largest of the two
//!   axis distances in the outline's own frame): each plane rises from its
//!   eave and hips and valleys run at 45 degrees in plan. The wavefront at
//!   time t is the outline less every eave grown by t that way, a union of
//!   axis-aligned boxes, so it is found exactly on a grid of their edges.
//!   Between the times where it changes (half the gap between two parallel
//!   sides) each wavefront edge sweeps a trapezoid; a plane is the union of
//!   its trapezoids. Several events at one time, as equal arm widths on an
//!   L, T or U make, need no special case. Gable sides of a hip over a
//!   rectangle do not move.
//! - Gable and shed roofs on rectangles slope in the rectangle's own frame;
//!   `ridge_axis` x is the side nearest to east-west.
//! - A derived outline (`TopFootprint`, `Footprint`) the shape does not fit
//!   keeps the Wave 1 drawing: a gable or shed over the bounding box of the
//!   footprint, clipped to it, a hip drawn as a gable, and review suggests
//!   (`roof_outline`). Flat takes any outline.
//!
//! The eaves are the outline grown by the overhang. The underside at the
//! wall line is `eave_mm` (the level height when None) above the level
//! floor; the top is `thickness_mm / cos(pitch)` over the underside.

use guhit_model::*;
use serde_json::{json, Value};

use crate::error::CoreError;
use crate::exec::{outcome, Outcome};
use crate::geom::*;
use crate::ids::IdGen;
use crate::issues::issue;
use crate::topo::Analysis;
use crate::validate::{require_level, require_material};

/// Fascia a migrated roof gets: what Wave 1 drew in 3D.
pub const MIGRATED_FASCIA_MM: f64 = 200.0;
/// A corner within this of 90 or 270 degrees is square.
pub const SQUARE_EPS_DEG: f64 = 0.5;
/// Ranges a roof's numbers must fall in, as `SetRoof` always checked.
pub const PITCH_DEG: (f64, f64) = (0.0, 75.0);
pub const OVERHANG_MM: (f64, f64) = (0.0, 5000.0);
pub const THICKNESS_MM: (f64, f64) = (10.0, 1000.0);
/// A fascia board or gutter face, mm.
pub const TRIM_MM: (f64, f64) = (10.0, 1000.0);
/// Underside at the wall line above the level floor, mm.
pub const EAVE_MM: (f64, f64) = (500.0, 20_000.0);
/// Roof posts: size and the most they may stand apart, mm.
pub const POST_MM: (f64, f64) = (50.0, 1000.0);
pub const POST_SPACING_MM: (f64, f64) = (500.0, 20_000.0);
/// An open roof edge with no wall or post under it for longer than this is
/// flagged (`roof_unsupported`). A rule of thumb, not a structural check.
pub const UNSUPPORTED_SPAN_MM: f64 = 5000.0;
/// A post this close to a drawn roof's edge carries it.
const POST_REACH_MM: f64 = 400.0;
/// Without a closed footprint anywhere, the walls of the top level are
/// roofed over their bounding box when it spans more than this both ways.
const FALLBACK_SPAN_MM: f64 = 1000.0;

/// The roof over the top floor as one `Roof` value (`Project::top_roof`).
/// Every reader of the old project roof goes through this.
pub fn top_roof(project: &Project) -> Roof {
    project.top_roof()
}

/// The `TopFootprint` roof element `roof` describes, with `id`. None for
/// kind none. Fascia 200 mm, no gutter: what Wave 1 drew.
pub fn element_from_roof(id: Id, roof: &Roof) -> Option<RoofArea> {
    Some(RoofArea {
        id,
        level_id: None,
        outline: RoofOutline::TopFootprint,
        shape: RoofShape::of_kind(roof.kind)?,
        pitch_deg: roof.pitch_deg,
        ridge_axis: roof.ridge_axis,
        flip: false,
        gable_sides: vec![],
        overhang_mm: roof.overhang_mm,
        thickness_mm: roof.thickness_mm,
        eave_mm: None,
        material_id: roof.material_id.clone(),
        fascia: Some(Trim {
            size_mm: MIGRATED_FASCIA_MM,
            material_id: None,
        }),
        gutter: None,
    })
}

/// Schema 6 (plan 1.7): a project roof with a kind becomes one
/// `TopFootprint` roof element, unless the project has one already, and the
/// old roof's kind becomes none. Its other values stay, so `top_roof` of a
/// project whose roof is later removed still offers them. The id is seeded
/// from the project, so migrating the same file twice gives the same id.
/// Running it twice changes nothing.
pub fn migrate(project: &mut Project) {
    if project.roof.kind == RoofKind::None {
        return;
    }
    if project.top_roof_area().is_none() {
        let seed = Command::SetRoof { roof: project.roof.clone() };
        let id = IdGen::new(project, &seed).next_id();
        if let Some(area) = element_from_roof(id, &project.roof) {
            project.elements.push(Element::Roof(area));
        }
    }
    project.roof.kind = RoofKind::None;
}

/// `Command::SetRoof`: edit the roof over the top floor from a whole `Roof`
/// (P0-2). Kind none removes it; any other kind changes it, keeping fascia,
/// gutter, eave and gable sides, or adds it with a 200 mm fascia. The
/// legacy `Project::roof` keeps the values with kind none, so `top_roof`
/// still offers them once the roof is gone.
pub fn set_roof(project: &mut Project, ids: &mut IdGen, roof: &Roof) -> Result<Outcome, CoreError> {
    require_material(project, &roof.material_id)?;
    for (name, v, (lo, hi)) in [
        ("Roof pitch (degrees)", roof.pitch_deg, PITCH_DEG),
        ("Roof overhang (mm)", roof.overhang_mm, OVERHANG_MM),
        ("Roof thickness (mm)", roof.thickness_mm, THICKNESS_MM),
    ] {
        if !v.is_finite() || v < lo || v > hi {
            return Err(CoreError::invalid(
                "bad_roof",
                format!("{name} must be between {lo} and {hi}."),
            ));
        }
    }
    let at = project
        .elements
        .iter()
        .position(|e| matches!(e, Element::Roof(r) if r.outline == RoofOutline::TopFootprint));
    project.roof = Roof {
        kind: RoofKind::None,
        ..roof.clone()
    };
    let Some(shape) = RoofShape::of_kind(roof.kind) else {
        return Ok(match at {
            Some(i) => {
                project.elements.remove(i);
                outcome("Remove roof")
            }
            None => outcome("Edit roof"),
        });
    };
    match at {
        Some(i) => {
            if let Element::Roof(r) = &mut project.elements[i] {
                r.shape = shape;
                r.pitch_deg = roof.pitch_deg;
                r.overhang_mm = roof.overhang_mm;
                r.thickness_mm = roof.thickness_mm;
                r.ridge_axis = roof.ridge_axis;
                r.material_id = roof.material_id.clone();
                // Gable sides belong to a hip; another shape drops them.
                if shape != RoofShape::Hip {
                    r.gable_sides.clear();
                }
            }
            Ok(outcome("Edit roof"))
        }
        None => {
            if let Some(area) = element_from_roof(ids.next_id(), roof) {
                project.elements.push(Element::Roof(area));
            }
            Ok(outcome("Add roof"))
        }
    }
}

/// The corners of a polygon that turn: repeated and straight-through points
/// dropped. Orientation is kept.
pub fn corners(points: &[Point]) -> Vec<Point> {
    let mut ps: Vec<Point> = vec![];
    for p in points {
        if ps.last().is_none_or(|q| dist(*q, *p) > 1e-6) {
            ps.push(*p);
        }
    }
    while ps.len() > 1 && dist(ps[0], ps[ps.len() - 1]) <= 1e-6 {
        ps.pop();
    }
    loop {
        let n = ps.len();
        if n < 3 {
            return ps;
        }
        let straight = (0..n).find(|&i| {
            let (a, b, c) = (ps[(i + n - 1) % n], ps[i], ps[(i + 1) % n]);
            match (unit(sub(b, a)), unit(sub(c, b))) {
                (Some(u), Some(v)) => cross(u, v).abs() < 1e-9 && dot(u, v) > 0.0,
                _ => true,
            }
        });
        match straight {
            Some(i) => {
                ps.remove(i);
            }
            None => return ps,
        }
    }
}

/// The corner that is not within `SQUARE_EPS_DEG` of 90 or 270 degrees, as
/// its turn in degrees, or None when every corner is square.
pub fn crooked_corner(corners: &[Point]) -> Option<f64> {
    let n = corners.len();
    let limit = SQUARE_EPS_DEG.to_radians().sin();
    (0..n).find_map(|i| {
        let (a, b, c) = (corners[(i + n - 1) % n], corners[i], corners[(i + 1) % n]);
        let (u, v) = (unit(sub(b, a))?, unit(sub(c, b))?);
        (dot(u, v).abs() > limit).then(|| 180.0 - dot(u, v).clamp(-1.0, 1.0).acos().to_degrees())
    })
}

/// Why a pitched `shape` cannot sit on a drawn outline with these corners,
/// naming what fits (P0-4). None when it can, and always for flat. Hip takes
/// rectangles, L, T and U outlines (4, 6 or 8 square corners); gable and
/// shed take rectangles.
pub fn outline_refusal(shape: RoofShape, corners: &[Point]) -> Option<String> {
    let n = corners.len();
    let what = match shape {
        RoofShape::Flat => return None,
        RoofShape::Hip => "A hip roof needs a rectangle, L, T or U outline with square corners",
        RoofShape::Gable => "A gable roof needs a rectangular outline with square corners",
        RoofShape::Shed => "A shed roof needs a rectangular outline with square corners",
    };
    let counts: &[usize] = if shape == RoofShape::Hip { &[4, 6, 8] } else { &[4] };
    if let Some(turn) = crooked_corner(corners) {
        return Some(format!(
            "{what}; this one has a corner of {turn:.1} degrees. Square the outline, or use a flat roof."
        ));
    }
    if !counts.contains(&n) {
        return Some(format!(
            "{what}; this one has {n} corners. Split it into roofs that fit, or use a flat roof."
        ));
    }
    None
}

/// The level a roof sits on: its own, or for the roof over the top floor
/// the topmost level with a building (the topmost with walls when no level
/// has a closed building yet).
pub fn roof_level<'a>(project: &'a Project, analysis: &Analysis, r: &RoofArea) -> Option<&'a Level> {
    match (&r.outline, &r.level_id) {
        (RoofOutline::TopFootprint, _) => {
            let built = |l: &&Level| analysis.level(&l.id).is_some_and(|t| !t.footprints.is_empty());
            let walled = |l: &&Level| {
                project
                    .elements
                    .iter()
                    .any(|e| matches!(e, Element::Wall(w) if w.level_id == l.id && w.role != WallRole::Site))
            };
            let top = |f: &dyn Fn(&&Level) -> bool| {
                project.levels.iter().filter(f).max_by(|a, b| a.elevation_mm.total_cmp(&b.elevation_mm))
            };
            top(&built).or_else(|| top(&walled))
        }
        (_, Some(id)) => project.levels.iter().find(|l| &l.id == id),
        _ => None,
    }
}

/// The outline a derived roof covers now: the outer wall faces of the
/// topmost level with a building (`TopFootprint`), or of the roof's level
/// (`Footprint`). One polygon per building.
pub fn derived_outlines<'a>(project: &Project, analysis: &'a Analysis, r: &RoofArea) -> Vec<&'a Vec<Point>> {
    let level = match (&r.outline, &r.level_id) {
        (RoofOutline::TopFootprint, _) => analysis
            .levels
            .iter()
            .filter(|l| !l.footprints.is_empty())
            .filter_map(|l| project.levels.iter().find(|x| x.id == l.level_id).map(|x| (l, x.elevation_mm)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(l, _)| l),
        (RoofOutline::Footprint, Some(id)) => analysis.levels.iter().find(|l| &l.level_id == id),
        _ => None,
    };
    level.map(|l| l.footprints.iter().map(|(p, _)| p).collect()).unwrap_or_default()
}

/// The outlines a roof stands on (its wall line, before the overhang):
/// the derived footprints, the drawn polygon, or with no closed building
/// anywhere the bounding box of the top level's walls (as Wave 1 did).
pub fn support_outlines(project: &Project, analysis: &Analysis, r: &RoofArea) -> Vec<Vec<Point>> {
    match &r.outline {
        RoofOutline::Polygon { points } => {
            let p = ensure_ccw(points);
            if p.len() >= 3 {
                vec![p]
            } else {
                vec![]
            }
        }
        RoofOutline::Footprint => derived_outlines(project, analysis, r).into_iter().map(|p| ensure_ccw(p)).collect(),
        RoofOutline::TopFootprint => {
            let found: Vec<Vec<Point>> = derived_outlines(project, analysis, r).into_iter().map(|p| ensure_ccw(p)).collect();
            if !found.is_empty() {
                return found;
            }
            let Some(level) = roof_level(project, analysis, r) else { return vec![] };
            let mut pts: Vec<Point> = vec![];
            for e in &project.elements {
                let Element::Wall(w) = e else { continue };
                if w.level_id != level.id || w.role == WallRole::Site {
                    continue;
                }
                match analysis.wall(&w.id) {
                    Some(t) if t.outline.len() >= 3 => pts.extend(t.outline.iter().copied()),
                    _ => pts.extend([w.start, w.end]),
                }
            }
            pts.retain(|p| is_finite(*p));
            if pts.is_empty() {
                return vec![];
            }
            let (lo, hi) = bbox(&pts);
            if hi.x - lo.x > FALLBACK_SPAN_MM && hi.y - lo.y > FALLBACK_SPAN_MM {
                vec![vec![pt(lo.x, lo.y), pt(hi.x, lo.y), pt(hi.x, hi.y), pt(lo.x, hi.y)]]
            } else {
                vec![]
            }
        }
    }
}

fn in_range(name: &str, v: f64, (lo, hi): (f64, f64), ids: &[Id]) -> Result<(), CoreError> {
    if v.is_finite() && v >= lo && v <= hi {
        Ok(())
    } else {
        Err(CoreError::invalid_for(
            "bad_roof",
            format!("{name} must be between {lo} and {hi}."),
            ids.to_vec(),
        ))
    }
}

/// The rules of one roof element (validate hub, `Element::Roof`). C0 wrote
/// the contract rules: `level_id` is None exactly for `TopFootprint`; one
/// roof over the top floor per project; a drawn outline a pitched shape
/// cannot take is refused with `roof_outline` (derived outlines never are);
/// gable sides only on a hip over a rectangle (P1-9).
pub fn validate(project: &Project, r: &RoofArea) -> Result<(), CoreError> {
    let ids = [r.id.clone()];
    match (&r.outline, &r.level_id) {
        (RoofOutline::TopFootprint, Some(_)) => {
            return Err(CoreError::invalid_for(
                "roof_level",
                "The roof over the top floor belongs to no level: leave its level empty. It sits on the topmost level with a building.",
                ids.to_vec(),
            ))
        }
        (RoofOutline::TopFootprint, None) => {}
        (_, None) => {
            return Err(CoreError::invalid_for(
                "roof_level",
                "A roof over a footprint or a drawn area needs the level whose walls carry it.",
                ids.to_vec(),
            ))
        }
        (_, Some(level)) => require_level(project, level)?,
    }
    if r.outline == RoofOutline::TopFootprint {
        if let Some(other) = project.elements.iter().find(|e| {
            matches!(e, Element::Roof(o) if o.id != r.id && o.outline == RoofOutline::TopFootprint)
        }) {
            return Err(CoreError::invalid_for(
                "roof_exists",
                "The project already has a roof over the top floor. Edit that roof instead.",
                vec![other.id().clone()],
            ));
        }
    }
    require_material(project, &r.material_id)?;
    in_range("Roof pitch (degrees)", r.pitch_deg, PITCH_DEG, &ids)?;
    in_range("Roof overhang (mm)", r.overhang_mm, OVERHANG_MM, &ids)?;
    in_range("Roof thickness (mm)", r.thickness_mm, THICKNESS_MM, &ids)?;
    if let Some(e) = r.eave_mm {
        in_range("Eave height (mm)", e, EAVE_MM, &ids)?;
    }
    for (name, trim) in [("Fascia", &r.fascia), ("Gutter", &r.gutter)] {
        if let Some(t) = trim {
            in_range(&format!("{name} size (mm)"), t.size_mm, TRIM_MM, &ids)?;
            require_material(project, &t.material_id)?;
        }
    }
    let drawn = match &r.outline {
        RoofOutline::Polygon { points } => {
            if points.iter().any(|p| !is_finite(*p) || p.x.abs() > 1.0e7 || p.y.abs() > 1.0e7) {
                return Err(CoreError::invalid_for(
                    "roof_outline",
                    "A roof outline point is not a valid number.",
                    ids.to_vec(),
                ));
            }
            let c = corners(points);
            if c.len() < 3 || signed_area(&c).abs() < 1.0 {
                return Err(CoreError::invalid_for(
                    "roof_outline",
                    "A drawn roof needs an outline of at least three corners.",
                    ids.to_vec(),
                ));
            }
            if let Some(why) = outline_refusal(r.shape, &c) {
                return Err(CoreError::invalid_for("roof_outline", why, ids.to_vec()));
            }
            Some(c)
        }
        _ => None,
    };
    if !r.gable_sides.is_empty() {
        let hint = "Gable sides turn the ends of a hip roof over a rectangle into gables. Use a gable roof instead, or split the roof into rectangles.";
        if r.shape != RoofShape::Hip {
            return Err(CoreError::invalid_for("roof_gable_sides", format!("Only a hip roof has gable sides. {hint}"), ids.to_vec()));
        }
        let rectangle = match &drawn {
            Some(c) => c.len() == 4,
            None => {
                let analysis = crate::topo::analyze(project);
                let outlines = derived_outlines(project, &analysis, r);
                outlines.len() == 1 && {
                    let c = corners(outlines[0]);
                    c.len() == 4 && crooked_corner(&c).is_none()
                }
            }
        };
        if !rectangle {
            return Err(CoreError::invalid_for("roof_gable_sides", format!("This roof's outline is not a rectangle. {hint}"), ids.to_vec()));
        }
        let mut seen = vec![];
        for s in &r.gable_sides {
            if *s > 3 || seen.contains(s) {
                return Err(CoreError::invalid_for(
                    "roof_gable_sides",
                    "Gable sides are outline sides 0 to 3, each at most once.",
                    ids.to_vec(),
                ));
            }
            seen.push(*s);
        }
    }
    Ok(())
}

// ------------------------------------------------------------ polygons

/// Cleaned, counter-clockwise copy of a plan polygon (as the 3D view and
/// the exports read a footprint). Empty when it is not a polygon.
pub fn ensure_ccw(poly: &[Point]) -> Vec<Point> {
    let tol = 1e-4;
    let same = |a: Point, b: Point| (a.x - b.x).abs() < tol && (a.y - b.y).abs() < tol;
    let mut out: Vec<Point> = vec![];
    for p in poly {
        if !is_finite(*p) || out.last().is_some_and(|l| same(*l, *p)) {
            continue;
        }
        out.push(*p);
    }
    while out.len() > 1 && same(out[0], out[out.len() - 1]) {
        out.pop();
    }
    if out.len() < 3 {
        return vec![];
    }
    let area = signed_area(&out);
    if area.abs() < 1e-6 {
        return vec![];
    }
    if area < 0.0 {
        out.reverse();
    }
    out
}

/// Mitred outward offset of a counter-clockwise polygon, very sharp corners
/// limited (`offset_polygon` of the exports and the 3D view): the eave line
/// is the wall line grown by the overhang.
pub fn offset_polygon(poly_in: &[Point], d: f64) -> Vec<Point> {
    let poly = ensure_ccw(poly_in);
    if poly.len() < 3 || d.abs() < 1e-6 {
        return poly;
    }
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (prev, cur, next) = (poly[(i + n - 1) % n], poly[i], poly[(i + 1) % n]);
            let d1 = unit(sub(cur, prev)).unwrap_or(pt(0.0, 0.0));
            let d2 = unit(sub(next, cur)).unwrap_or(pt(0.0, 0.0));
            // The outward normal of a counter-clockwise edge is its right side.
            let (n1, n2) = (pt(d1.y, -d1.x), pt(d2.y, -d2.x));
            let bis = unit(add(n1, n2)).unwrap_or(n2);
            let cos = dot(bis, n1);
            add(cur, scale(bis, d / cos.max(0.35)))
        })
        .collect()
}

/// The part of `poly` where `dot(p, axis) - t` is at least 0 (`keep_greater`)
/// or at most 0 (Sutherland-Hodgman, as the exports clip).
fn clip_half_plane(poly: &[Point], axis: Point, t: f64, keep_greater: bool) -> Vec<Point> {
    let side = |p: Point| {
        let d = dot(p, axis) - t;
        if keep_greater {
            d
        } else {
            -d
        }
    };
    let mut out = Vec::with_capacity(poly.len() + 2);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let (da, db) = (side(a), side(b));
        if da >= 0.0 {
            out.push(a);
        }
        if (da > 0.0 && db < 0.0) || (da < 0.0 && db > 0.0) {
            out.push(lerp(a, b, da / (da - db)));
        }
    }
    out
}

/// Drops repeated points, a closing duplicate included.
fn clean(poly: &[Point]) -> Vec<Point> {
    let tol = 1e-4;
    let mut out: Vec<Point> = vec![];
    for p in poly {
        if out.last().is_some_and(|l| (l.x - p.x).abs() < tol && (l.y - p.y).abs() < tol) {
            continue;
        }
        out.push(*p);
    }
    while out.len() > 1 && dist(out[0], out[out.len() - 1]) < tol {
        out.pop();
    }
    out
}

/// Axis-aligned unions and loops on a grid: what the skeleton and the stair
/// openings are made of.
pub mod ortho {
    use guhit_model::Point;
    use std::collections::BTreeMap;

    /// Sorted, with values closer than 1e-7 merged.
    pub fn sorted_unique(mut v: Vec<f64>) -> Vec<f64> {
        v.retain(|x| x.is_finite());
        v.sort_by(|a, b| a.total_cmp(b));
        v.dedup_by(|a, b| (*a - *b).abs() < 1e-7);
        v
    }

    /// The counter-clockwise outlines of the grid cells (i, j) between
    /// `xs[i]..xs[i + 1]` and `ys[j]..ys[j + 1]` that are `inside`. Two
    /// pieces that touch at a corner stay two loops. Straight-through
    /// points are dropped.
    pub fn cell_loops(xs: &[f64], ys: &[f64], inside: &dyn Fn(usize, usize) -> bool) -> Vec<Vec<Point>> {
        if xs.len() < 2 || ys.len() < 2 {
            return vec![];
        }
        let (nx, ny) = (xs.len() - 1, ys.len() - 1);
        let cells: Vec<Vec<bool>> = (0..nx).map(|i| (0..ny).map(|j| inside(i, j)).collect()).collect();
        let on = |i: isize, j: isize| i >= 0 && j >= 0 && (i as usize) < nx && (j as usize) < ny && cells[i as usize][j as usize];
        // Directed boundary edges, inside on the left, by start vertex.
        let mut out_edges: BTreeMap<(usize, usize), Vec<(usize, usize)>> = BTreeMap::new();
        for i in 0..nx {
            for j in 0..ny {
                if !cells[i][j] {
                    continue;
                }
                let (ii, jj) = (i as isize, j as isize);
                let mut push = |a: (usize, usize), b: (usize, usize)| out_edges.entry(a).or_default().push(b);
                if !on(ii, jj - 1) {
                    push((i, j), (i + 1, j));
                }
                if !on(ii + 1, jj) {
                    push((i + 1, j), (i + 1, j + 1));
                }
                if !on(ii, jj + 1) {
                    push((i + 1, j + 1), (i, j + 1));
                }
                if !on(ii - 1, jj) {
                    push((i, j + 1), (i, j));
                }
            }
        }
        let mut loops = vec![];
        while let Some((&start, _)) = out_edges.iter().find(|(_, v)| !v.is_empty()) {
            let mut path = vec![start];
            let mut prev = start;
            let mut cur = out_edges.get_mut(&start).expect("an edge").remove(0);
            let mut guard = 0;
            while cur != start && guard < 1_000_000 {
                guard += 1;
                path.push(cur);
                let din = (cur.0 as isize - prev.0 as isize, cur.1 as isize - prev.1 as isize);
                let Some(list) = out_edges.get_mut(&cur) else { break };
                if list.is_empty() {
                    break;
                }
                // The leftmost turn keeps pieces that touch at a corner apart.
                let rank = |to: &(usize, usize)| {
                    let d = (to.0 as isize - cur.0 as isize, to.1 as isize - cur.1 as isize);
                    let c = din.0 * d.1 - din.1 * d.0;
                    let s = din.0 * d.0 + din.1 * d.1;
                    if c > 0 {
                        0
                    } else if s > 0 {
                        1
                    } else {
                        2
                    }
                };
                let k = (0..list.len()).min_by_key(|&k| rank(&list[k])).expect("an edge");
                let next = list.remove(k);
                prev = cur;
                cur = next;
            }
            let pts: Vec<Point> = path.iter().map(|&(i, j)| Point { x: xs[i], y: ys[j] }).collect();
            let simple = drop_straight(&pts);
            if simple.len() >= 3 {
                loops.push(simple);
            }
        }
        loops
    }

    /// The loop without points where it runs straight through.
    pub fn drop_straight(pts: &[Point]) -> Vec<Point> {
        let mut ps = pts.to_vec();
        loop {
            let n = ps.len();
            if n < 3 {
                return ps;
            }
            let straight = (0..n).find(|&i| {
                let (a, b, c) = (ps[(i + n - 1) % n], ps[i], ps[(i + 1) % n]);
                let (ux, uy, vx, vy) = (b.x - a.x, b.y - a.y, c.x - b.x, c.y - b.y);
                let (lu, lv) = (ux.hypot(uy), vx.hypot(vy));
                lu < 1e-9 || lv < 1e-9 || ((ux * vy - uy * vx).abs() < 1e-9 * lu * lv && ux * vx + uy * vy > 0.0)
            });
            match straight {
                Some(i) => {
                    ps.remove(i);
                }
                None => return ps,
            }
        }
    }

    /// The outlines of a union of axis-aligned rectangles (x0, x1, y0, y1),
    /// counter-clockwise.
    pub fn rect_union(rects: &[(f64, f64, f64, f64)]) -> Vec<Vec<Point>> {
        let xs = sorted_unique(rects.iter().flat_map(|r| [r.0, r.1]).collect());
        let ys = sorted_unique(rects.iter().flat_map(|r| [r.2, r.3]).collect());
        let inside = |i: usize, j: usize| {
            let (cx, cy) = ((xs[i] + xs[i + 1]) / 2.0, (ys[j] + ys[j + 1]) / 2.0);
            rects.iter().any(|r| cx > r.0 && cx < r.1 && cy > r.2 && cy < r.3)
        };
        cell_loops(&xs, &ys, &inside)
    }
}

// ------------------------------------------------------------- skeleton

/// One plane of a roof in the making: its plan polygon, the plan unit
/// vector up its slope (zero when flat) and the slope's tangent, and the
/// top height at `origin`.
#[derive(Debug, Clone)]
struct Face {
    poly: Vec<Point>,
    origin: Point,
    up: Point,
    tan: f64,
    z0: f64,
}

impl Face {
    fn top(&self, p: Point) -> f64 {
        self.z0 + dot(sub(p, self.origin), self.up) * self.tan
    }
}

/// The frame an orthogonal outline is square in: the rotation (radians) of
/// its side nearest to east-west.
fn frame_angle(corners: &[Point]) -> f64 {
    let Some(d) = corners.get(1).and_then(|b| unit(sub(*b, corners[0]))) else {
        return 0.0;
    };
    let quarter = std::f64::consts::FRAC_PI_2;
    let mut a = d.y.atan2(d.x) % quarter;
    if a > quarter / 2.0 {
        a -= quarter;
    } else if a <= -quarter / 2.0 {
        a += quarter;
    }
    if a.abs() < 1e-12 {
        0.0
    } else {
        a
    }
}

fn rot(p: Point, a: f64) -> Point {
    if a == 0.0 {
        return p;
    }
    let (s, c) = a.sin_cos();
    pt(p.x * c - p.y * s, p.x * s + p.y * c)
}

/// A side of an orthogonal outline in its frame: horizontal or vertical,
/// its inward normal, the coordinate of its line along that normal, the
/// span along it, and whether it slopes (gable sides stand still).
#[derive(Debug, Clone, Copy)]
struct Side {
    n: Point,
    c: f64,
    lo: f64,
    hi: f64,
    horizontal: bool,
    moves: bool,
}

impl Side {
    /// Distance from the side measured square to it (the larger of the two
    /// axis distances to the segment).
    fn square_dist(&self, p: Point) -> f64 {
        let (along, across) = if self.horizontal { (p.x, p.y) } else { (p.y, p.x) };
        let line = if self.horizontal { self.c * self.n.y } else { self.c * self.n.x };
        let d_across = (across - line).abs();
        let d_along = (self.lo - along).max(along - self.hi).max(0.0);
        d_across.max(d_along)
    }
}

/// The hip planes of an orthogonal outline (counter-clockwise, every corner
/// square, already in its own frame), every side sloping except where
/// `fixed` (gable ends). Each plane as (polygon, side index), heights left
/// to the caller: a point's height is the pitch times its distance from
/// that side's line.
fn hip_planes(q: &[Point], fixed: &[bool]) -> Vec<(Vec<Point>, usize)> {
    let n = q.len();
    let sides: Vec<Side> = (0..n)
        .map(|i| {
            let (a, b) = (q[i], q[(i + 1) % n]);
            let horizontal = (b.x - a.x).abs() >= (b.y - a.y).abs();
            let d = unit(sub(b, a)).unwrap_or(pt(1.0, 0.0));
            let nrm = if horizontal { pt(0.0, d.x.signum()) } else { pt(-d.y.signum(), 0.0) };
            let (lo, hi) = if horizontal { (a.x.min(b.x), a.x.max(b.x)) } else { (a.y.min(b.y), a.y.max(b.y)) };
            Side { n: nrm, c: dot(a, nrm), lo, hi, horizontal, moves: !fixed.get(i).copied().unwrap_or(false) }
        })
        .collect();
    // Every time the wavefront can change: half the gap between two
    // parallel sides (the whole gap to a side that stands still).
    let mut times = vec![0.0];
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (sides[i], sides[j]);
            if a.horizontal != b.horizontal {
                continue;
            }
            let (ca, cb) = (a.c * if a.horizontal { a.n.y } else { a.n.x }, b.c * if b.horizontal { b.n.y } else { b.n.x });
            let speed = a.moves as u8 as f64 + b.moves as u8 as f64;
            if speed > 0.0 && (ca - cb).abs() > 1e-9 {
                times.push((ca - cb).abs() / speed);
            }
        }
    }
    let times = ortho::sorted_unique(times);
    // Plane keys: sides on one line facing one way share a plane.
    let key: Vec<usize> = (0..n)
        .map(|i| {
            (0..n)
                .find(|&j| {
                    let (a, b) = (sides[i], sides[j]);
                    dist(a.n, b.n) < 1e-9 && (a.c - b.c).abs() < 1e-6 && a.moves == b.moves
                })
                .unwrap_or(i)
        })
        .collect();
    let moving: Vec<Side> = sides.iter().copied().filter(|s| s.moves).collect();
    let base_xs: Vec<f64> = q.iter().map(|p| p.x).collect();
    let base_ys: Vec<f64> = q.iter().map(|p| p.y).collect();
    let mut traps: Vec<(usize, Vec<Point>)> = vec![];
    for w in times.windows(2) {
        let (t0, t1) = (w[0], w[1]);
        if t1 - t0 < 1e-7 {
            continue;
        }
        let tm = (t0 + t1) / 2.0;
        let mut xs = base_xs.clone();
        let mut ys = base_ys.clone();
        for s in &moving {
            let line = if s.horizontal { s.c * s.n.y } else { s.c * s.n.x };
            let (along, across) = if s.horizontal { (&mut xs, &mut ys) } else { (&mut ys, &mut xs) };
            along.extend([s.lo - tm, s.hi + tm]);
            across.extend([line - tm, line + tm]);
        }
        let xs = ortho::sorted_unique(xs);
        let ys = ortho::sorted_unique(ys);
        let inside = |i: usize, j: usize| {
            let c = pt((xs[i] + xs[i + 1]) / 2.0, (ys[j] + ys[j + 1]) / 2.0);
            point_in_polygon(c, q) && moving.iter().all(|s| s.square_dist(c) > tm)
        };
        let loops = ortho::cell_loops(&xs, &ys, &inside);
        if loops.is_empty() {
            break;
        }
        for lp in loops {
            let m = lp.len();
            // Which side each wavefront edge came from.
            let owner: Vec<Option<usize>> = (0..m)
                .map(|k| {
                    let (a, b) = (lp[k], lp[(k + 1) % m]);
                    let d = unit(sub(b, a))?;
                    let nrm = perp_left(d);
                    (0..n).find(|&i| {
                        let s = sides[i];
                        dist(s.n, nrm) < 1e-6 && (dot(a, nrm) - (s.c + if s.moves { tm } else { 0.0 })).abs() < 1e-5
                    })
                })
                .collect();
            let speed = |k: usize| owner[k].map(|i| sides[i].moves as u8 as f64).unwrap_or(1.0);
            let normal = |k: usize| perp_left(unit(sub(lp[(k + 1) % m], lp[k])).unwrap_or(pt(0.0, 0.0)));
            let vel: Vec<Point> = (0..m)
                .map(|k| {
                    let before = (k + m - 1) % m;
                    add(scale(normal(before), speed(before)), scale(normal(k), speed(k)))
                })
                .collect();
            for k in 0..m {
                let Some(i) = owner[k] else { continue };
                if !sides[i].moves {
                    continue;
                }
                let (a, b) = (lp[k], lp[(k + 1) % m]);
                let (va, vb) = (vel[k], vel[(k + 1) % m]);
                let poly = clean(&[
                    sub(a, scale(va, tm - t0)),
                    sub(b, scale(vb, tm - t0)),
                    add(b, scale(vb, t1 - tm)),
                    add(a, scale(va, t1 - tm)),
                ]);
                if poly.len() >= 3 && signed_area(&poly) > 1e-9 {
                    traps.push((key[i], poly));
                }
            }
        }
    }
    let mut keys: Vec<usize> = traps.iter().map(|t| t.0).collect();
    keys.sort();
    keys.dedup();
    let mut out = vec![];
    for k in keys {
        let polys: Vec<&Vec<Point>> = traps.iter().filter(|t| t.0 == k).map(|t| &t.1).collect();
        for lp in union_loops(&polys) {
            out.push((lp, k));
        }
    }
    out
}

/// The outlines of a union of counter-clockwise polygons that meet edge to
/// edge (corners of one may sit on the edges of another): shared edges
/// cancel, the rest chain into loops.
fn union_loops(polys: &[&Vec<Point>]) -> Vec<Vec<Point>> {
    const TOL: f64 = 1e-5;
    let mut verts: Vec<Point> = vec![];
    let id = |p: Point, verts: &mut Vec<Point>| -> usize {
        match verts.iter().position(|v| (v.x - p.x).abs() < TOL && (v.y - p.y).abs() < TOL) {
            Some(i) => i,
            None => {
                verts.push(p);
                verts.len() - 1
            }
        }
    };
    let rings: Vec<Vec<usize>> = polys.iter().map(|p| p.iter().map(|q| id(*q, &mut verts)).collect()).collect();
    // Directed edges, split wherever another vertex sits on them.
    let mut edges: Vec<(usize, usize)> = vec![];
    for ring in &rings {
        let m = ring.len();
        for k in 0..m {
            let (a, b) = (ring[k], ring[(k + 1) % m]);
            if a == b {
                continue;
            }
            let (pa, pb) = (verts[a], verts[b]);
            let len = dist(pa, pb);
            let mut on: Vec<(f64, usize)> = (0..verts.len())
                .filter(|&v| v != a && v != b)
                .filter_map(|v| {
                    let (t, off) = project_on_segment(verts[v], pa, pb);
                    (off < TOL && t > TOL && t < len - TOL).then_some((t, v))
                })
                .collect();
            on.sort_by(|x, y| x.0.total_cmp(&y.0));
            let mut chain = vec![a];
            chain.extend(on.into_iter().map(|(_, v)| v));
            chain.push(b);
            for w in chain.windows(2) {
                edges.push((w[0], w[1]));
            }
        }
    }
    // Cancel pairs running opposite ways.
    let mut keep = vec![true; edges.len()];
    for i in 0..edges.len() {
        if !keep[i] {
            continue;
        }
        if let Some(j) = (0..edges.len()).find(|&j| keep[j] && j != i && edges[j] == (edges[i].1, edges[i].0)) {
            keep[i] = false;
            keep[j] = false;
        }
    }
    let mut left: Vec<(usize, usize)> = edges.into_iter().zip(keep).filter(|(_, k)| *k).map(|(e, _)| e).collect();
    let mut loops = vec![];
    while !left.is_empty() {
        let (start, mut cur) = left.remove(0);
        let mut ring = vec![start];
        let mut prev = start;
        let mut guard = 0;
        while cur != start && guard < 100_000 {
            guard += 1;
            ring.push(cur);
            let din = sub(verts[cur], verts[prev]);
            let options: Vec<usize> = (0..left.len()).filter(|&k| left[k].0 == cur).collect();
            let Some(&k) = options.iter().max_by(|&&x, &&y| {
                let turn = |k: usize| {
                    let d = sub(verts[left[k].1], verts[cur]);
                    cross(din, d).atan2(dot(din, d))
                };
                turn(x).total_cmp(&turn(y))
            }) else {
                break;
            };
            let (_, next) = left.remove(k);
            prev = cur;
            cur = next;
        }
        let pts = ortho::drop_straight(&ring.iter().map(|&v| verts[v]).collect::<Vec<_>>());
        if pts.len() >= 3 && signed_area(&pts) > 1e-6 {
            loops.push(pts);
        }
    }
    loops
}

// ---------------------------------------------------------------- shapes

/// What one outline of a roof makes: planes, and whether the shape fits the
/// outline (false: drawn with the Wave 1 clipping).
struct Over {
    faces: Vec<Face>,
    fits: bool,
}

/// The pitch a roof slopes at, kept to the range a roof takes.
fn pitch_of(r: &RoofArea) -> f64 {
    if r.pitch_deg.is_finite() {
        r.pitch_deg.clamp(PITCH_DEG.0, PITCH_DEG.1)
    } else {
        0.0
    }
}

/// Planes of a gable or shed over a rectangle, or (for an outline that does
/// not fit) of the Wave 1 drawing over its bounding box, in a frame turned
/// by `angle` about the origin. `base` is the underside at the wall line,
/// `vertical` the roof's vertical thickness.
fn gable_or_shed(r: &RoofArea, fp: &[Point], angle: f64, base: f64, vertical: f64, shape: RoofShape) -> Vec<Face> {
    let local: Vec<Point> = fp.iter().map(|p| rot(*p, -angle)).collect();
    let fp = ensure_ccw(&local);
    if fp.len() < 3 {
        return vec![];
    }
    let (lo_pt, hi_pt) = bbox(&fp);
    let outline = offset_polygon(&fp, r.overhang_mm.max(0.0));
    if outline.len() < 3 {
        return vec![];
    }
    let tan = pitch_of(r).to_radians().tan();
    let axis_x = r.ridge_axis == Axis::X;
    let back = |poly: &[Point]| poly.iter().map(|p| rot(*p, angle)).collect::<Vec<_>>();
    let face = |poly: Vec<Point>, origin: Point, up: Point| Face {
        poly: back(&poly),
        origin: rot(origin, angle),
        up: rot(up, angle),
        tan,
        z0: base + vertical,
    };
    if shape == RoofShape::Shed {
        // ridge_axis is the slope direction of a shed: it rises along +axis,
        // or against it with `flip`, from the wall line on its low side.
        let up = if axis_x { pt(1.0, 0.0) } else { pt(0.0, 1.0) };
        return vec![if r.flip { face(outline, hi_pt, scale(up, -1.0)) } else { face(outline, lo_pt, up) }];
    }
    let across = if axis_x { pt(0.0, 1.0) } else { pt(1.0, 0.0) };
    let (lo, hi) = if axis_x { (lo_pt.y, hi_pt.y) } else { (lo_pt.x, hi_pt.x) };
    let mid = (lo + hi) / 2.0;
    let mut out = vec![];
    for (poly, origin, up) in [
        (clip_half_plane(&outline, across, mid, false), scale(across, lo), across),
        (clip_half_plane(&outline, across, mid, true), scale(across, hi), scale(across, -1.0)),
    ] {
        let poly = clean(&poly);
        if poly.len() >= 3 {
            out.push(face(poly, origin, up));
        }
    }
    out
}

/// The planes over one outline (the wall line, counter-clockwise).
fn over_outline(r: &RoofArea, fp: &[Point], base: f64) -> Over {
    let pitch = pitch_of(r);
    let thickness = r.thickness_mm.max(THICKNESS_MM.0);
    let overhang = r.overhang_mm.max(0.0);
    if r.shape == RoofShape::Flat {
        let outline = offset_polygon(fp, overhang);
        let faces = if outline.len() >= 3 {
            vec![Face { poly: outline, origin: fp[0], up: pt(0.0, 0.0), tan: 0.0, z0: base + thickness }]
        } else {
            vec![]
        };
        return Over { faces, fits: true };
    }
    let (tan, cos) = (pitch.to_radians().tan(), pitch.to_radians().cos());
    let vertical = thickness / cos;
    let c = corners(fp);
    let fits = outline_refusal(r.shape, &c).is_none();
    if !fits {
        // The Wave 1 drawing: a gable (hip too) or shed over the bounding
        // box, in plan axes.
        let shape = if r.shape == RoofShape::Shed { RoofShape::Shed } else { RoofShape::Gable };
        return Over { faces: gable_or_shed(r, fp, 0.0, base, vertical, shape), fits };
    }
    let angle = frame_angle(&c);
    if r.shape != RoofShape::Hip {
        return Over { faces: gable_or_shed(r, fp, angle, base, vertical, r.shape), fits };
    }
    // Hip: the skeleton of the eave line in its own frame, its sides put
    // square (each line through the middle of its side, corners where the
    // lines meet).
    let eaves = offset_polygon(&c, overhang);
    let local: Vec<Point> = eaves.iter().map(|p| rot(*p, -angle)).collect();
    let m = local.len();
    let horizontal: Vec<bool> = (0..m)
        .map(|i| {
            let (a, b) = (local[i], local[(i + 1) % m]);
            (b.x - a.x).abs() >= (b.y - a.y).abs()
        })
        .collect();
    let line: Vec<f64> = (0..m)
        .map(|i| {
            let (a, b) = (local[i], local[(i + 1) % m]);
            if horizontal[i] {
                (a.y + b.y) / 2.0
            } else {
                (a.x + b.x) / 2.0
            }
        })
        .collect();
    let q: Vec<Point> = (0..m)
        .map(|i| {
            let before = (i + m - 1) % m;
            if horizontal[i] {
                pt(line[before], line[i])
            } else {
                pt(line[i], line[before])
            }
        })
        .collect();
    let fixed: Vec<bool> = (0..m).map(|i| m == 4 && r.gable_sides.contains(&(i as u32))).collect();
    let z0 = base - overhang * tan + vertical;
    let faces = hip_planes(&q, &fixed)
        .into_iter()
        .map(|(poly, side)| {
            let (a, b) = (q[side], q[(side + 1) % m]);
            let up = perp_left(unit(sub(b, a)).unwrap_or(pt(1.0, 0.0)));
            Face {
                poly: poly.iter().map(|p| rot(*p, angle)).collect(),
                origin: rot(a, angle),
                up: rot(up, angle),
                tan,
                z0,
            }
        })
        .collect();
    Over { faces, fits }
}

// ------------------------------------------------------------------ derive

fn v3(p: Point, z: f64) -> Vec3 {
    Vec3 { x: p.x, y: p.y, z }
}

/// The face whose polygon holds `p` (on its edge counts), best first.
fn face_at(faces: &[Face], p: Point) -> Option<&Face> {
    faces
        .iter()
        .find(|f| point_in_polygon(p, &f.poly))
        .or_else(|| faces.iter().min_by(|a, b| dist_to_boundary(p, &a.poly).total_cmp(&dist_to_boundary(p, &b.poly))))
}

/// True when segment a-b runs along an edge of `outline`.
fn on_outline(a: Point, b: Point, outline: &[Point]) -> bool {
    let n = outline.len();
    (0..n).any(|i| {
        let (c, d) = (outline[i], outline[(i + 1) % n]);
        dist_point_segment(a, c, d) < 1e-3 && dist_point_segment(b, c, d) < 1e-3
    })
}

/// Ridges, hips, valleys, eaves and rakes of a set of planes over one eave
/// outline.
fn lines_of(faces: &[Face], outline: &[Point], eave_z: f64) -> Vec<RoofLine> {
    let mut segs: Vec<(Point, Point, usize)> = vec![];
    for (fi, f) in faces.iter().enumerate() {
        let m = f.poly.len();
        for k in 0..m {
            segs.push((f.poly[k], f.poly[(k + 1) % m], fi));
        }
    }
    // Split every edge where a corner of another plane sits on it, so the
    // two sides of a skeleton edge match piece by piece.
    let corners_all: Vec<Point> = faces.iter().flat_map(|f| f.poly.iter().copied()).collect();
    let mut pieces: Vec<(Point, Point, usize)> = vec![];
    for (a, b, fi) in segs {
        let len = dist(a, b);
        if len < 1e-6 {
            continue;
        }
        let mut ts: Vec<f64> = corners_all
            .iter()
            .filter_map(|c| {
                let (t, off) = project_on_segment(*c, a, b);
                (off < 1e-4 && t > 1e-4 && t < len - 1e-4).then_some(t / len)
            })
            .collect();
        ts.push(0.0);
        ts.push(1.0);
        ts.sort_by(|x, y| x.total_cmp(y));
        ts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
        for w in ts.windows(2) {
            pieces.push((lerp(a, b, w[0]), lerp(a, b, w[1]), fi));
        }
    }
    let mut out = vec![];
    let same = |p: Point, q: Point| dist(p, q) < 1e-3;
    for (k, &(a, b, fi)) in pieces.iter().enumerate() {
        let f = &faces[fi];
        let (za, zb) = (f.top(a), f.top(b));
        if on_outline(a, b, outline) {
            let kind = if (za - zb).abs() > 1e-3 {
                RoofLineKind::Rake
            } else if (za - eave_z).abs() < 1e-3 {
                RoofLineKind::Eave
            } else {
                RoofLineKind::Ridge
            };
            out.push(RoofLine { kind, a: v3(a, za), b: v3(b, zb) });
            continue;
        }
        // An inner edge: once, from the plane listed first.
        let Some(&(_, _, gi)) = pieces.iter().enumerate().find(|(j, p)| *j != k && same(p.0, b) && same(p.1, a)).map(|(_, p)| p) else {
            continue;
        };
        if gi < fi {
            continue;
        }
        let g = &faces[gi];
        let mid = lerp(a, b, 0.5);
        let probe = add(mid, scale(perp_left(unit(sub(a, b)).unwrap_or(pt(0.0, 0.0))), 10.0));
        let convex = f.top(probe) >= g.top(probe) - 1e-6;
        let kind = if (za - zb).abs() <= 1e-3 {
            if convex {
                RoofLineKind::Ridge
            } else {
                RoofLineKind::Valley
            }
        } else if convex {
            RoofLineKind::Hip
        } else {
            RoofLineKind::Valley
        };
        out.push(RoofLine { kind, a: v3(a, za), b: v3(b, zb) });
    }
    out
}

/// Runs of the eave outline at the eave height, in outline order: each an
/// open polyline; one all the way round repeats its first point.
fn eave_runs(faces: &[Face], outline: &[Point], eave_z: f64) -> Vec<Vec<Vec3>> {
    let n = outline.len();
    let height = |p: Point| face_at(faces, p).map(|f| f.top(p)).unwrap_or(eave_z);
    let is_eave: Vec<bool> = (0..n)
        .map(|i| {
            let (a, b) = (outline[i], outline[(i + 1) % n]);
            (height(a) - eave_z).abs() < 0.5 && (height(b) - eave_z).abs() < 0.5 && (height(lerp(a, b, 0.5)) - eave_z).abs() < 0.5
        })
        .collect();
    if is_eave.iter().all(|e| *e) {
        let mut run: Vec<Vec3> = outline.iter().map(|p| v3(*p, eave_z)).collect();
        run.push(v3(outline[0], eave_z));
        return vec![run];
    }
    // Start after a side that is not an eave, so no run is cut in two.
    let start = (0..n).find(|&i| !is_eave[i]).map(|i| (i + 1) % n).unwrap_or(0);
    let mut runs: Vec<Vec<Vec3>> = vec![];
    let mut cur: Vec<Vec3> = vec![];
    for k in 0..n {
        let i = (start + k) % n;
        if is_eave[i] {
            if cur.is_empty() {
                cur.push(v3(outline[i], eave_z));
            }
            cur.push(v3(outline[(i + 1) % n], eave_z));
        } else if !cur.is_empty() {
            runs.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        runs.push(cur);
    }
    runs
}

/// The vertical infill between the wall tops (`base`) and the roof
/// underside along each side of the wall line, where the underside is
/// higher: gable ends, the high side of a shed.
fn infill_of(faces: &[Face], wall_line: &[Point], base: f64, vertical: f64) -> Vec<Vec<Vec3>> {
    let n = wall_line.len();
    let mut out = vec![];
    for i in 0..n {
        let (a, b) = (wall_line[i], wall_line[(i + 1) % n]);
        let len = dist(a, b);
        if len < 1.0 {
            continue;
        }
        // Break where the side crosses a plane's edge.
        let mut ts = vec![0.0, 1.0];
        for f in faces {
            let m = f.poly.len();
            for k in 0..m {
                let (c, d) = (f.poly[k], f.poly[(k + 1) % m]);
                if let Some(x) = line_intersect(a, sub(b, a), c, sub(d, c)) {
                    let (t, off) = project_on_segment(x, a, b);
                    let (_, off2) = project_on_segment(x, c, d);
                    if off < 1e-6 && off2 < 1e-6 && t > 1e-6 && t < len - 1e-6 {
                        ts.push(t / len);
                    }
                }
                let (t, off) = project_on_segment(c, a, b);
                if off < 1e-4 && t > 1e-6 && t < len - 1e-6 {
                    ts.push(t / len);
                }
            }
        }
        ts.sort_by(|x, y| x.total_cmp(y));
        ts.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
        let under = |t: f64| {
            let p = lerp(a, b, t);
            // Judge the plane a little inside, off the side itself.
            let probe = add(p, scale(perp_left(unit(sub(b, a)).unwrap_or(pt(0.0, 0.0))), 1.0));
            let f = face_at(faces, probe).or_else(|| face_at(faces, p));
            f.map(|f| f.top(p) - vertical).unwrap_or(base).max(base)
        };
        let tops: Vec<(f64, f64)> = ts.iter().map(|t| (*t, under(*t))).collect();
        if tops.iter().all(|(_, u)| *u - base < 1.0) {
            continue;
        }
        let mut poly = vec![v3(a, base), v3(b, base)];
        for (t, u) in tops.iter().rev() {
            poly.push(v3(lerp(a, b, *t), *u));
        }
        out.push(poly);
    }
    out
}

/// The geometry of one roof (`Derived::roofs`), z above its level's floor.
/// None when it has no level or nothing to stand on.
pub fn geometry(project: &Project, analysis: &Analysis, r: &RoofArea) -> Option<(RoofGeometry, bool)> {
    let level = roof_level(project, analysis, r)?;
    let base = r.eave_mm.filter(|e| e.is_finite()).unwrap_or(level.height_mm);
    let pitch = if r.shape == RoofShape::Flat { 0.0 } else { pitch_of(r) };
    let thickness = r.thickness_mm.max(THICKNESS_MM.0);
    let vertical = thickness / pitch.to_radians().cos();
    let tan = pitch.to_radians().tan();
    let overhang = r.overhang_mm.max(0.0);
    let eave_z = base - overhang * tan + vertical;
    let mut g = RoofGeometry {
        roof_id: r.id.clone(),
        level_id: level.id.clone(),
        planes: vec![],
        vertical_mm: vertical,
        lines: vec![],
        infill: vec![],
        eaves: vec![],
        plan_area_m2: 0.0,
        surface_area_m2: 0.0,
    };
    let mut fits = true;
    let derived = !matches!(r.outline, RoofOutline::Polygon { .. });
    let outlines = support_outlines(project, analysis, r);
    if outlines.is_empty() {
        return None;
    }
    for fp in &outlines {
        let over = over_outline(r, fp, base);
        fits &= over.fits;
        if over.faces.is_empty() {
            continue;
        }
        let eaves = offset_polygon(fp, overhang);
        g.plan_area_m2 += signed_area(&eaves).abs() / 1.0e6;
        for f in &over.faces {
            let area = signed_area(&f.poly).abs();
            g.surface_area_m2 += area * (1.0 + f.tan * f.tan).sqrt() / 1.0e6;
            g.planes.push(f.poly.iter().map(|p| v3(*p, f.top(*p))).collect());
        }
        g.lines.extend(lines_of(&over.faces, &eaves, eave_z));
        g.eaves.extend(eave_runs(&over.faces, &eaves, eave_z));
        if derived {
            g.infill.extend(infill_of(&over.faces, fp, base, vertical));
        }
    }
    if g.planes.is_empty() {
        return None;
    }
    let tidy = |v: f64| (v * 1.0e6).round() / 1.0e6;
    g.plan_area_m2 = tidy(g.plan_area_m2);
    g.surface_area_m2 = tidy(g.surface_area_m2);
    Some((g, fits))
}

/// `Derived::roofs`, in the order of the roof elements: hip skeletons,
/// gables, sheds and flats with their lines, eaves, infill and areas. A
/// derived outline the shape does not fit keeps the Wave 1 clipping.
pub fn derive(project: &Project, analysis: &Analysis) -> Vec<RoofGeometry> {
    let mut out: Vec<RoofGeometry> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Roof(r) => geometry(project, analysis, r).map(|(g, _)| g),
            _ => None,
        })
        .collect();
    // A project that was never migrated (`compute_derived` on a raw file)
    // still has its legacy roof over the top floor, as `top_roof` reads it:
    // derived with an empty roof id.
    if project.top_roof_area().is_none() {
        if let Some(r) = element_from_roof(String::new(), &project.roof) {
            out.extend(geometry(project, analysis, &r).map(|(g, _)| g));
        }
    }
    out
}

/// The roof a plan point is under, on `level_id`: the top of its covering
/// at `p`, mm above that level's floor. The highest when roofs overlap.
pub fn top_at(roofs: &[RoofGeometry], level_id: &str, p: Point) -> Option<f64> {
    roofs
        .iter()
        .filter(|g| g.level_id == level_id)
        .flat_map(|g| g.planes.iter())
        .filter(|plane| point_in_polygon(p, &plane.iter().map(|q| pt(q.x, q.y)).collect::<Vec<_>>()))
        .filter_map(|plane| crate::stairs::plane_height(plane, p))
        .max_by(|a, b| a.total_cmp(b))
}

// ------------------------------------------------------------------ review

fn mm_as_m(v: f64) -> String {
    format!("{:.1} m", v / 1000.0)
}

fn shape_name(s: RoofShape) -> &'static str {
    match s {
        RoofShape::Flat => "flat",
        RoofShape::Shed => "shed",
        RoofShape::Gable => "gable",
        RoofShape::Hip => "hip",
    }
}

/// Roof review items: `roof_outline` (a derived outline the shape does not
/// fit, drawn with the Wave 1 clipping) and `roof_unsupported` (a drawn
/// roof with a long stretch of edge over no wall or post). Suggestions.
pub fn review(project: &Project, analysis: &Analysis, _roofs: &[RoofGeometry]) -> Vec<Issue> {
    let mut out = vec![];
    for e in &project.elements {
        let Element::Roof(r) = e else { continue };
        let derived = !matches!(r.outline, RoofOutline::Polygon { .. });
        if derived && r.shape != RoofShape::Flat {
            let why = support_outlines(project, analysis, r)
                .iter()
                .find_map(|fp| outline_refusal(r.shape, &corners(fp)));
            if let Some(why) = why {
                let drawn = if r.shape == RoofShape::Shed { "a shed" } else { "a gable" };
                out.push(issue(
                    "roof_outline",
                    Severity::Info,
                    format!(
                        "This {} roof follows walls it does not fit, so it is drawn as {drawn} over their bounding box. {why}",
                        shape_name(r.shape)
                    ),
                    vec![r.id.clone()],
                ));
            }
        }
        if let Some(span) = unsupported_span(project, r) {
            out.push(issue(
                "roof_unsupported",
                Severity::Warning,
                format!(
                    "This roof has {} of edge with no wall or post under it. Add posts under the open eaves, or a wall, about every 3 m.",
                    mm_as_m(span)
                ),
                vec![r.id.clone()],
            ));
        }
    }
    out
}

fn columns_of<'a>(project: &'a Project, level_id: &str) -> Vec<&'a Column> {
    project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Column(c) if c.level_id == level_id => Some(c),
            _ => None,
        })
        .collect()
}

/// The longest stretch of a drawn roof's edge with no wall along it and no
/// post near it, when longer than `UNSUPPORTED_SPAN_MM`.
fn unsupported_span(project: &Project, r: &RoofArea) -> Option<f64> {
    let RoofOutline::Polygon { points } = &r.outline else { return None };
    let level = r.level_id.as_ref()?;
    let outline = ensure_ccw(points);
    let n = outline.len();
    if n < 3 {
        return None;
    }
    let walls = crate::slabs::walls_on(project, level);
    let posts: Vec<Point> = columns_of(project, level).iter().map(|c| c.center).collect();
    // Supports along the perimeter, as distances from its first corner.
    let mut at = 0.0;
    let mut covered: Vec<(f64, f64)> = vec![];
    let mut marks: Vec<f64> = vec![];
    for i in 0..n {
        let (a, b) = (outline[i], outline[(i + 1) % n]);
        let len = dist(a, b);
        for (t0, t1) in crate::slabs::wall_cover_of(&walls, a, b) {
            covered.push((at + t0 * len, at + t1 * len));
        }
        for p in &posts {
            let (t, off) = project_on_segment(*p, a, b);
            if off <= POST_REACH_MM {
                marks.push(at + t);
            }
        }
        at += len;
    }
    let total = at;
    if covered.is_empty() && marks.is_empty() {
        return (total > UNSUPPORTED_SPAN_MM).then_some(total);
    }
    // Gaps between supports round the loop.
    let mut points: Vec<(f64, f64)> = covered;
    points.extend(marks.iter().map(|m| (*m, *m)));
    points.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut longest: f64 = 0.0;
    let mut reach = points[0].1;
    for p in points.iter().skip(1) {
        longest = longest.max(p.0 - reach);
        reach = reach.max(p.1);
    }
    longest = longest.max(points[0].0 + total - reach);
    (longest > UNSUPPORTED_SPAN_MM).then_some(longest)
}

/// `Command::AddRoofPosts`: square columns under the open eaves of a roof
/// (stretches of its wall line with no wall along them), at every corner
/// of an open stretch and at most `max_spacing_mm` apart, their outer faces
/// on the line. Spots within a post of an existing column are skipped. One
/// undo step.
pub fn add_posts(
    project: &mut Project,
    ids: &mut IdGen,
    roof_id: &Id,
    size_mm: f64,
    max_spacing_mm: f64,
) -> Result<Outcome, CoreError> {
    let Some(Element::Roof(r)) = project.elements.iter().find(|e| e.id() == roof_id).cloned() else {
        return Err(CoreError::invalid_for("not_found", "There is no roof with that id.", vec![roof_id.clone()]));
    };
    let ids_of = [r.id.clone()];
    for (name, v, range) in [("Post size (mm)", size_mm, POST_MM), ("Post spacing (mm)", max_spacing_mm, POST_SPACING_MM)] {
        if !(v.is_finite() && v >= range.0 && v <= range.1) {
            return Err(CoreError::invalid_for(
                "bad_roof_posts",
                format!("{name} must be between {} and {}.", range.0, range.1),
                ids_of.to_vec(),
            ));
        }
    }
    let analysis = crate::topo::analyze(project);
    let Some(level) = roof_level(project, &analysis, &r).map(|l| l.id.clone()) else {
        return Err(CoreError::invalid_for("roof_level", "This roof stands on no level yet.", ids_of.to_vec()));
    };
    let walls = crate::slabs::walls_on(project, &level);
    let existing: Vec<Point> = columns_of(project, &level).iter().map(|c| c.center).collect();
    let mut spots: Vec<(Point, f64)> = vec![];
    for outline in support_outlines(project, &analysis, &r) {
        let n = outline.len();
        let corner_at = |p: Point| outline.iter().position(|c| dist(*c, p) < 1e-3);
        for run in crate::slabs::open_runs_of(&outline, true, &|a, b| crate::slabs::wall_cover_of(&walls, a, b)) {
            let last = run.len().saturating_sub(1);
            for (k, w) in run.windows(2).enumerate() {
                let (a, b) = (w[0], w[1]);
                let len = dist(a, b);
                let Some(d) = unit(sub(b, a)) else { continue };
                let inward = perp_left(d);
                let half = size_mm / 2.0;
                let angle = d.y.atan2(d.x).to_degrees();
                let gaps = (len / max_spacing_mm).ceil().max(1.0) as usize;
                for j in 0..=gaps {
                    let t = len * j as f64 / gaps as f64;
                    let p = add(a, scale(d, t));
                    let end = (j == 0 && k == 0) || (j == gaps && k + 1 == last);
                    // A run's end on a wall needs no post: the wall carries it.
                    let on_wall = walls.iter().any(|w| dist_point_segment(p, w.start, w.end) <= w.thickness_mm / 2.0 + 50.0);
                    if end && on_wall {
                        continue;
                    }
                    let spot = match corner_at(p) {
                        Some(ci) => {
                            let prev = outline[(ci + n - 1) % n];
                            let next = outline[(ci + 1) % n];
                            let (d1, d2) = (unit(sub(p, prev)).unwrap_or(d), unit(sub(next, p)).unwrap_or(d));
                            let bis = unit(add(perp_left(d1), perp_left(d2))).unwrap_or(inward);
                            let cos = dot(bis, perp_left(d1)).max(0.35);
                            add(p, scale(bis, half / cos))
                        }
                        None => add(p, scale(inward, half)),
                    };
                    if spots.iter().any(|(s, _)| dist(*s, spot) < size_mm) || existing.iter().any(|s| dist(*s, spot) < size_mm) {
                        continue;
                    }
                    spots.push((spot, angle));
                }
            }
        }
    }
    if spots.is_empty() {
        return Err(CoreError::invalid_for(
            "no_open_eaves",
            "Every edge of this roof already stands on a wall or a post, so it needs no posts.",
            ids_of.to_vec(),
        ));
    }
    let count = spots.len();
    for (center, angle) in spots {
        project.elements.push(Element::Column(Column {
            id: ids.next_id(),
            level_id: level.clone(),
            center,
            shape: ColumnShape::Rect,
            width_mm: size_mm,
            depth_mm: size_mm,
            rotation_deg: (angle * 100.0).round() / 100.0,
            material_id: None,
        }));
    }
    let mut o = outcome(if count == 1 { "Add roof post" } else { "Add roof posts" });
    o.summary = Some(format!(
        "Added {count} {} under the open eaves, at most {:.1} m apart.",
        if count == 1 { "post" } else { "posts" },
        max_spacing_mm / 1000.0
    ));
    Ok(o)
}

/// A roof for `Query::Describe` and `ListElements`, with its areas.
pub fn describe(project: &Project, r: &RoofArea) -> Value {
    let level = r
        .level_id
        .as_ref()
        .and_then(|id| project.levels.iter().find(|l| &l.id == id))
        .map(|l| l.name.clone());
    let analysis = crate::topo::analyze(project);
    let g = geometry(project, &analysis, r);
    json!({
        "id": r.id,
        "kind": "roof",
        "level": level,
        "outline": r.outline,
        "shape": r.shape,
        "pitch_deg": r.pitch_deg,
        "ridge_axis": r.ridge_axis,
        "overhang_mm": r.overhang_mm,
        "thickness_mm": r.thickness_mm,
        "eave_mm": r.eave_mm,
        "gable_sides": r.gable_sides,
        "flip": r.flip,
        "fascia": r.fascia,
        "gutter": r.gutter,
        "material_id": r.material_id,
        "planes": g.as_ref().map(|(g, _)| g.planes.len()),
        "plan_area_m2": g.as_ref().map(|(g, _)| (g.plan_area_m2 * 100.0).round() / 100.0),
        "surface_area_m2": g.as_ref().map(|(g, _)| (g.surface_area_m2 * 100.0).round() / 100.0),
        "fits_outline": g.as_ref().map(|(_, fits)| *fits),
    })
}
