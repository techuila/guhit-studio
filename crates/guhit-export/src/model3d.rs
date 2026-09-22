//! Shared 3D scene extraction for the whole-model exports (IFC4 and 3D DXF).
//!
//! Everything here works in model millimeters: plan +x east, +y north, z up.
//! Heights returned by this module are measured from the level floor, so a
//! consumer places them either in a storey-local frame (IFC) or by adding the
//! level elevation (DXF).
//!
//! The wall decomposition mirrors the live 3D viewer (`src/viewer3d/geom/
//! wallMesh.ts`): the mitred plan outline is cut into strips at every opening
//! edge and heights into intervals at every sill and head, and each
//! (strip, interval) cell is either solid or void. That gives real reveals on
//! all four sides of an opening without a CSG library.

use std::collections::HashMap;

use guhit_model::{
    Annotation, Asset, Axis, Column, Derived, Element, Level, Opening, Project, Room, RoofKind,
    Stair, Wall,
};

use crate::geom::*;

/// Slab thickness under a storey floor, mm.
pub const SLAB_THICKNESS_MM: f64 = 150.0;

/// Wall local frame: u runs along the centerline from `start`, v to its left.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub origin: V,
    pub dir: V,
    pub left: V,
    pub length: f64,
}

pub fn frame(start: V, end: V) -> Option<Frame> {
    let d = end - start;
    let length = d.len();
    if !length.is_finite() || length <= 1e-3 {
        return None;
    }
    let dir = d.unit()?;
    Some(Frame {
        origin: start,
        dir,
        left: dir.left(),
        length,
    })
}

impl Frame {
    pub fn to_local(&self, p: V) -> V {
        let r = p - self.origin;
        v(r.dot(self.dir), r.dot(self.left))
    }

    pub fn to_plan(&self, uv: V) -> V {
        self.origin + self.dir * uv.x + self.left * uv.y
    }
}

/// An opening clamped to the wall body and to the built height, in wall local
/// coordinates: `a`..`b` along u, `bottom`..`top` in height above the floor.
#[derive(Debug, Clone, Copy)]
pub struct Cut {
    pub a: f64,
    pub b: f64,
    pub bottom: f64,
    pub top: f64,
}

/// One wall ready to be turned into geometry.
pub struct WallSolid<'a> {
    pub wall: &'a Wall,
    pub frame: Frame,
    /// Mitred outline in wall local (u, v), counter-clockwise.
    pub outline: Vec<V>,
    /// Same outline in plan coordinates, counter-clockwise.
    pub plan_outline: Vec<V>,
    pub height: f64,
    pub openings: Vec<&'a Opening>,
    pub cuts: Vec<Cut>,
    pub exterior: bool,
}

/// Local outline, counter-clockwise. Falls back to a plain rectangle when the
/// derived outline is missing or degenerate.
fn local_outline(wall: &Wall, derived_outline: Option<&[V]>, f: &Frame) -> Vec<V> {
    if let Some(pts) = derived_outline {
        if pts.len() >= 3 {
            let local: Vec<V> = pts.iter().map(|p| f.to_local(*p)).collect();
            let ccw = ensure_ccw(&local);
            if ccw.len() >= 3 {
                return ccw;
            }
        }
    }
    let t = wall.thickness_mm.max(1.0) / 2.0;
    vec![
        v(0.0, -t),
        v(f.length, -t),
        v(f.length, t),
        v(0.0, t),
    ]
}

fn clamp_openings(openings: &[&Opening], u_min: f64, u_max: f64, height: f64) -> Vec<Cut> {
    let margin = 1.0;
    let mut out = Vec::new();
    for o in openings {
        if ![o.offset_mm, o.width_mm, o.height_mm, o.sill_mm]
            .iter()
            .all(|x| x.is_finite())
        {
            continue;
        }
        let a = (o.offset_mm - o.width_mm / 2.0).max(u_min + margin);
        let b = (o.offset_mm + o.width_mm / 2.0).min(u_max - margin);
        let bottom = o.sill_mm.clamp(0.0, height);
        let top = (o.sill_mm + o.height_mm).clamp(0.0, height);
        if b - a < 1.0 || top - bottom < 1.0 {
            continue;
        }
        out.push(Cut { a, b, bottom, top });
    }
    out
}

/// The whole model, grouped by level and ready for either exporter.
pub struct Scene<'a> {
    pub levels: Vec<LevelScene<'a>>,
    /// Roof planes over the top level, empty for `RoofKind::None`.
    pub roof: Vec<RoofPlane>,
    pub roof_kind: RoofKind,
    /// Index of the level the roof sits on, if any.
    pub roof_level: Option<usize>,
}

pub struct LevelScene<'a> {
    pub level: &'a Level,
    pub walls: Vec<WallSolid<'a>>,
    pub rooms: Vec<RoomSolid<'a>>,
    /// One per building on this level, counter-clockwise plan polygons.
    pub footprints: Vec<Vec<V>>,
    pub columns: Vec<&'a Column>,
    pub stairs: Vec<&'a Stair>,
    pub assets: Vec<&'a Asset>,
    pub annotations: Vec<&'a Annotation>,
}

pub struct RoomSolid<'a> {
    pub room: &'a Room,
    pub polygon: Vec<V>,
    pub area_mm2: f64,
    pub wall_ids: Vec<String>,
}

/// One planar roof surface. `plan` is its footprint in plan coordinates,
/// already offset by the overhang and clipped to this plane's share.
pub struct RoofPlane {
    pub plan: Vec<V>,
    /// Plan point where the plane's local origin sits.
    pub origin: V,
    /// Underside height at `origin`, mm above the level floor.
    pub base_h: f64,
    /// Horizontal unit vector along the ridge (the plane's local x). Always
    /// `up_slope` turned 90 degrees clockwise, so (along, up_slope) is right
    /// handed and the plane normal points up.
    pub along: V,
    /// Plan unit vector pointing up the slope.
    pub up_slope: V,
    pub tan: f64,
    pub cos: f64,
    /// Thickness measured perpendicular to the plane, mm.
    pub thickness: f64,
}

impl RoofPlane {
    /// Underside height at a plan point, mm above the level floor.
    pub fn under(&self, p: V) -> f64 {
        self.base_h + (p - self.origin).dot(self.up_slope) * self.tan
    }

    /// Top surface height at a plan point, mm above the level floor.
    pub fn top(&self, p: V) -> f64 {
        self.under(p) + self.thickness / self.cos
    }

    /// Profile coordinates of a plan point in the plane's own 2D space.
    pub fn profile(&self, p: V) -> V {
        let r = p - self.origin;
        v(r.dot(self.along), r.dot(self.up_slope) / self.cos)
    }
}

fn pts(points: &[guhit_model::Point]) -> Vec<V> {
    points.iter().map(V::from).collect()
}

fn clamp_pitch(deg: f64) -> f64 {
    if !deg.is_finite() {
        0.0
    } else {
        deg.clamp(0.0, 60.0)
    }
}

/// Roof planes over one building footprint. `base` is the wall top height
/// above the level floor.
pub fn roof_planes(project: &Project, footprint: &[V], base: f64) -> Vec<RoofPlane> {
    let roof = &project.roof;
    let fp = ensure_ccw(footprint);
    let b = bounds_of(&fp);
    if roof.kind == RoofKind::None || fp.len() < 3 || b.is_empty() {
        return Vec::new();
    }
    let thickness = roof.thickness_mm.max(10.0);
    let overhang = roof.overhang_mm.max(0.0);
    let outline = offset_polygon(&fp, overhang);
    if outline.len() < 3 {
        return Vec::new();
    }

    // `along` is always `up_slope` turned 90 degrees clockwise.
    let plane = |plan: Vec<V>, origin: V, up_slope: V, tan: f64, cos: f64| RoofPlane {
        plan,
        origin,
        base_h: base,
        along: v(up_slope.y, -up_slope.x),
        up_slope,
        tan,
        cos,
        thickness,
    };

    if roof.kind == RoofKind::Flat {
        return vec![plane(outline, b.min, v(0.0, 1.0), 0.0, 1.0)];
    }

    let pitch = clamp_pitch(roof.pitch_deg).to_radians();
    let tan = pitch.tan();
    let cos = pitch.cos();
    let axis_x = roof.ridge_axis == Axis::X;

    if roof.kind == RoofKind::Shed {
        // ridge_axis is the slope direction for a shed roof: it rises along +axis.
        let up_slope = if axis_x { v(1.0, 0.0) } else { v(0.0, 1.0) };
        return vec![plane(outline, b.min, up_slope, tan, cos)];
    }

    // Gable: the ridge runs along ridge_axis, the slopes fall across it.
    let across = if axis_x { v(0.0, 1.0) } else { v(1.0, 0.0) };
    let (lo, hi) = if axis_x {
        (b.min.y, b.max.y)
    } else {
        (b.min.x, b.max.x)
    };
    let mid = (lo + hi) / 2.0;
    let low_side = clip_half_plane(&outline, v(0.0, 0.0), across, mid, false);
    let high_side = clip_half_plane(&outline, v(0.0, 0.0), across, mid, true);
    let mut out = Vec::new();
    for (plan, origin, up_slope) in [
        (low_side, across * lo, across),
        (high_side, across * hi, -across),
    ] {
        let plan = clean_polygon(&plan);
        if plan.len() < 3 {
            continue;
        }
        out.push(plane(plan, origin, up_slope, tan, cos));
    }
    out
}

/// Group the whole project into levels with geometry ready to write.
pub fn build_scene<'a>(project: &'a Project, derived: &'a Derived) -> Scene<'a> {
    let outlines: HashMap<&str, Vec<V>> = derived
        .walls
        .iter()
        .map(|w| (w.wall_id.as_str(), pts(&w.outline)))
        .collect();
    let exterior: HashMap<&str, bool> = derived
        .walls
        .iter()
        .map(|w| (w.wall_id.as_str(), w.exterior))
        .collect();
    let room_geo: HashMap<&str, &guhit_model::RoomGeometry> = derived
        .rooms
        .iter()
        .map(|r| (r.room_id.as_str(), r))
        .collect();

    let mut by_wall: HashMap<&str, Vec<&Opening>> = HashMap::new();
    for e in &project.elements {
        if let Element::Opening(o) = e {
            by_wall.entry(o.wall_id.as_str()).or_default().push(o);
        }
    }
    for list in by_wall.values_mut() {
        list.sort_by(|a, b| {
            a.offset_mm
                .partial_cmp(&b.offset_mm)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
    }

    let mut levels: Vec<LevelScene> = project
        .levels
        .iter()
        .map(|level| LevelScene {
            level,
            walls: Vec::new(),
            rooms: Vec::new(),
            footprints: Vec::new(),
            columns: Vec::new(),
            stairs: Vec::new(),
            assets: Vec::new(),
            annotations: Vec::new(),
        })
        .collect();
    let index: HashMap<&str, usize> = project
        .levels
        .iter()
        .enumerate()
        .map(|(i, l)| (l.id.as_str(), i))
        .collect();
    // Elements on an unknown level fall back to the first one, so nothing is
    // silently dropped.
    let fallback = if levels.is_empty() { None } else { Some(0) };
    let slot = move |id: &str| index.get(id).copied().or(fallback);

    for fp in &derived.footprints {
        let poly = ensure_ccw(&pts(&fp.polygon));
        if poly.len() < 3 {
            continue;
        }
        if let Some(i) = slot(&fp.level_id) {
            levels[i].footprints.push(poly);
        }
    }

    for e in &project.elements {
        match e {
            Element::Wall(w) => {
                let Some(i) = slot(&w.level_id) else { continue };
                let height = w
                    .height_mm
                    .filter(|h| h.is_finite() && *h > 1.0)
                    .unwrap_or(levels[i].level.height_mm);
                if !height.is_finite() || height <= 1.0 {
                    continue;
                }
                let Some(f) = frame(V::from(w.start), V::from(w.end)) else {
                    continue;
                };
                let outline = local_outline(w, outlines.get(w.id.as_str()).map(|o| &o[..]), &f);
                let ob = bounds_of(&outline);
                if outline.len() < 3 || ob.is_empty() {
                    continue;
                }
                let openings: Vec<&Opening> =
                    by_wall.get(w.id.as_str()).cloned().unwrap_or_default();
                let cuts = clamp_openings(&openings, ob.min.x, ob.max.x, height);
                let plan_outline = outline.iter().map(|p| f.to_plan(*p)).collect();
                levels[i].walls.push(WallSolid {
                    wall: w,
                    frame: f,
                    outline,
                    plan_outline,
                    height,
                    openings,
                    cuts,
                    exterior: exterior.get(w.id.as_str()).copied().unwrap_or(false),
                });
            }
            Element::Room(r) => {
                let Some(i) = slot(&r.level_id) else { continue };
                let g = room_geo.get(r.id.as_str());
                let poly = ensure_ccw(&pts(g.map(|g| &g.polygon[..]).unwrap_or(&[])));
                if poly.len() < 3 {
                    continue;
                }
                levels[i].rooms.push(RoomSolid {
                    room: r,
                    area_mm2: g.map(|g| g.area_mm2).unwrap_or_else(|| polygon_area(&poly)),
                    wall_ids: g.map(|g| g.wall_ids.clone()).unwrap_or_default(),
                    polygon: poly,
                });
            }
            Element::Column(c) => {
                if let Some(i) = slot(&c.level_id) {
                    levels[i].columns.push(c);
                }
            }
            Element::Stair(s) => {
                if let Some(i) = slot(&s.level_id) {
                    levels[i].stairs.push(s);
                }
            }
            Element::Asset(a) => {
                if let Some(i) = slot(&a.level_id) {
                    levels[i].assets.push(a);
                }
            }
            Element::Annotation(a) => {
                if let Some(i) = slot(&a.level_id) {
                    levels[i].annotations.push(a);
                }
            }
            _ => {}
        }
    }

    // The roof sits on the topmost level that has a footprint.
    let roof_level = levels
        .iter()
        .enumerate()
        .filter(|(_, l)| !l.footprints.is_empty())
        .max_by(|a, b| {
            a.1.level
                .elevation_mm
                .partial_cmp(&b.1.level.elevation_mm)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i);
    let mut roof = Vec::new();
    if project.roof.kind != RoofKind::None {
        if let Some(i) = roof_level {
            let base = levels[i].level.height_mm;
            for fp in &levels[i].footprints {
                roof.extend(roof_planes(project, fp, base));
            }
        }
    }

    Scene {
        levels,
        roof_kind: project.roof.kind,
        roof_level: if roof.is_empty() { None } else { roof_level },
        roof,
    }
}

/// Wall body split into solid cells. `strips[s]` is the plan polygon of strip
/// `s` in wall local coordinates, `heights` are the cut heights, and
/// `solid[s][k]` says whether the cell between `heights[k]` and
/// `heights[k + 1]` is material.
pub struct WallCells {
    /// Strip boundaries along u, with the ends open.
    pub edges: Vec<f64>,
    pub strips: Vec<Vec<V>>,
    pub heights: Vec<f64>,
    pub solid: Vec<Vec<bool>>,
}

fn unique_sorted(mut values: Vec<f64>) -> Vec<f64> {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<f64> = Vec::with_capacity(values.len());
    for x in values {
        if out.last().map(|last| x - last > 0.01).unwrap_or(true) {
            out.push(x);
        }
    }
    out
}

pub fn wall_cells(w: &WallSolid) -> WallCells {
    let mut u_cuts = Vec::new();
    for c in &w.cuts {
        u_cuts.push(c.a);
        u_cuts.push(c.b);
    }
    let u_cuts = unique_sorted(u_cuts);
    let mut hs = vec![0.0, w.height];
    for c in &w.cuts {
        hs.push(c.bottom);
        hs.push(c.top);
    }
    let heights = unique_sorted(hs);

    let mut edges = vec![f64::NEG_INFINITY];
    edges.extend(u_cuts);
    edges.push(f64::INFINITY);

    let strip_count = edges.len() - 1;
    let interval_count = heights.len().saturating_sub(1);
    let mut strips = Vec::with_capacity(strip_count);
    let mut solid = Vec::with_capacity(strip_count);
    for s in 0..strip_count {
        let lo = edges[s];
        let hi = edges[s + 1];
        let mid = if lo.is_finite() && hi.is_finite() {
            (lo + hi) / 2.0
        } else if lo.is_finite() {
            lo + 1.0
        } else {
            hi - 1.0
        };
        let mut row = Vec::with_capacity(interval_count);
        for k in 0..interval_count {
            let hm = (heights[k] + heights[k + 1]) / 2.0;
            let is_void = w
                .cuts
                .iter()
                .any(|c| mid > c.a && mid < c.b && hm > c.bottom && hm < c.top);
            row.push(!is_void);
        }
        solid.push(row);
        strips.push(clip_to_x_range(&w.outline, lo, hi));
    }

    WallCells {
        edges,
        strips,
        heights,
        solid,
    }
}

/// Plan rectangle of an opening on its host wall: `width` along the wall,
/// `depth` across it, centered on the centerline at `offset_mm`.
pub fn opening_rect(w: &WallSolid, o: &Opening, depth: f64) -> Vec<V> {
    let half_w = o.width_mm.max(1.0) / 2.0;
    let half_d = depth.max(1.0) / 2.0;
    [
        v(o.offset_mm - half_w, -half_d),
        v(o.offset_mm + half_w, -half_d),
        v(o.offset_mm + half_w, half_d),
        v(o.offset_mm - half_w, half_d),
    ]
    .iter()
    .map(|p| w.frame.to_plan(*p))
    .collect()
}

/// A rectangle centered on `c`, `w` wide along the rotated x, `d` deep along
/// the rotated y. Counter-clockwise.
pub fn rect(c: V, w: f64, d: f64, rot_deg: f64) -> Vec<V> {
    let ex = dir(rot_deg) * (w / 2.0);
    let ey = dir(rot_deg + 90.0) * (d / 2.0);
    vec![c - ex - ey, c + ex - ey, c + ex + ey, c - ex + ey]
}

/// Circle approximation, counter-clockwise, used for round columns.
pub fn circle(c: V, r: f64, segments: usize) -> Vec<V> {
    (0..segments)
        .map(|i| {
            let a = (i as f64) * 360.0 / (segments as f64);
            c + dir(a) * r
        })
        .collect()
}

/// One tread of a straight flight: plan rectangle and its top height above the
/// level floor. The flight runs along the rotated +y from `origin`, which is
/// the center of the first riser.
pub struct Tread {
    pub plan: Vec<V>,
    pub top: f64,
}

pub fn stair_treads(s: &Stair, level_height: f64) -> Vec<Tread> {
    let n = s.riser_count.max(1) as usize;
    let going = s.run_mm.max(1.0) / (n as f64);
    let rise = level_height.max(1.0) / (n as f64);
    let fwd = dir(s.rotation_deg + 90.0);
    let origin = V::from(s.origin);
    (0..n)
        .map(|i| {
            let c = origin + fwd * (going * (i as f64) + going / 2.0);
            Tread {
                plan: rect(c, s.width_mm.max(1.0), going, s.rotation_deg),
                top: rise * ((i + 1) as f64),
            }
        })
        .collect()
}

/// Whole plan rectangle of a stair flight, for the single-box IFC form.
pub fn stair_box(s: &Stair) -> Vec<V> {
    let fwd = dir(s.rotation_deg + 90.0);
    let run = s.run_mm.max(1.0);
    let c = V::from(s.origin) + fwd * (run / 2.0);
    rect(c, s.width_mm.max(1.0), run, s.rotation_deg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wall_cells_split_around_one_opening() {
        let wall = Wall {
            id: "w".into(),
            level_id: "l".into(),
            start: guhit_model::Point { x: 0.0, y: 0.0 },
            end: guhit_model::Point { x: 4000.0, y: 0.0 },
            thickness_mm: 150.0,
            height_mm: None,
            material_id: None,
        };
        let f = frame(v(0.0, 0.0), v(4000.0, 0.0)).unwrap();
        let outline = local_outline(&wall, None, &f);
        let solid = WallSolid {
            wall: &wall,
            frame: f,
            plan_outline: outline.iter().map(|p| f.to_plan(*p)).collect(),
            outline,
            height: 3000.0,
            openings: vec![],
            cuts: vec![Cut {
                a: 1500.0,
                b: 2400.0,
                bottom: 900.0,
                top: 2100.0,
            }],
            exterior: true,
        };
        let cells = wall_cells(&solid);
        assert_eq!(cells.strips.len(), 3, "left, opening, right");
        assert_eq!(cells.heights, vec![0.0, 900.0, 2100.0, 3000.0]);
        assert!(cells.solid[0].iter().all(|s| *s), "left strip is solid");
        assert_eq!(cells.solid[1], vec![true, false, true], "sill, void, lintel");
        assert!(cells.solid[2].iter().all(|s| *s), "right strip is solid");
    }

    #[test]
    fn gable_planes_meet_at_the_ridge() {
        let mut project = guhit_model::defaults::new_project("Test");
        project.roof = guhit_model::Roof {
            kind: RoofKind::Gable,
            pitch_deg: 20.0,
            overhang_mm: 600.0,
            thickness_mm: 150.0,
            ridge_axis: Axis::X,
            material_id: None,
        };
        let fp = vec![
            v(-75.0, -75.0),
            v(8075.0, -75.0),
            v(8075.0, 6075.0),
            v(-75.0, 6075.0),
        ];
        let planes = roof_planes(&project, &fp, 3000.0);
        assert_eq!(planes.len(), 2);
        let ridge = v(4000.0, 3000.0);
        let tan = 20f64.to_radians().tan();
        let cos = 20f64.to_radians().cos();
        let expect_under = 3000.0 + 3075.0 * tan;
        for p in &planes {
            assert!((p.under(ridge) - expect_under).abs() < 1e-6, "{}", p.under(ridge));
            assert!((p.top(ridge) - (expect_under + 150.0 / cos)).abs() < 1e-6);
            // The frame stays right handed, so the plane normal points up.
            assert!((p.along.x * p.up_slope.y - p.along.y * p.up_slope.x - 1.0).abs() < 1e-9);
            // Profile coordinates are flat: the origin maps to (something, 0).
            assert!(p.profile(p.origin).y.abs() < 1e-9);
        }
        // The eave dips below the wall top by the overhang.
        let eave = v(4000.0, -675.0);
        assert!(planes[0].under(eave) < 3000.0);
    }

    #[test]
    fn treads_climb_to_the_level_height() {
        let s = Stair {
            id: "s".into(),
            level_id: "l".into(),
            origin: guhit_model::Point { x: 0.0, y: 0.0 },
            rotation_deg: 0.0,
            width_mm: 1000.0,
            run_mm: 2750.0,
            riser_count: 11,
        };
        let treads = stair_treads(&s, 3000.0);
        assert_eq!(treads.len(), 11);
        assert!((treads[10].top - 3000.0).abs() < 1e-6);
        assert!((treads[0].top - 3000.0 / 11.0).abs() < 1e-6);
    }
}
