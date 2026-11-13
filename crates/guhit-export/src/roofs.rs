//! Roof solids for the whole-model exports. LANE L2 owns this module.
//!
//! Every roof comes from `Derived::roofs` (DECISIONS D54): the engine works
//! out the planes once, and this module only turns each plane into the
//! frame the IFC and 3D DXF writers extrude (`RoofPlane`). The planes of a
//! migrated roof stay within 0.5 mm of Wave 1's
//! (`crates/guhit-app/tests/w2_l2_roofs_stairs.rs`).

use guhit_model::{Axis, Derived, Element, Id, Project, Roof, RoofGeometry, RoofKind, Vec3};

use crate::geom::*;
use crate::model3d::LevelScene;

/// One roof of the scene, on one level.
pub struct RoofSolid {
    /// The roof element's id.
    pub roof_id: Id,
    pub kind: RoofKind,
    pub material_id: Option<Id>,
    /// Index into `Scene::levels` of the level that carries it.
    pub level: usize,
    pub planes: Vec<RoofPlane>,
    /// Gable-end infill polygons, plan x and y and z above the level floor,
    /// as `RoofGeometry::infill`.
    pub infill: Vec<Vec<Vec3>>,
}

/// The roofs of the scene, one per entry of `Derived::roofs`, each on the
/// level that carries it.
pub fn scene_roofs(project: &Project, derived: &Derived, levels: &[LevelScene]) -> Vec<RoofSolid> {
    let mut out = vec![];
    for g in &derived.roofs {
        let Some(level) = levels.iter().position(|l| l.level.id == g.level_id) else { continue };
        // An empty id is the legacy roof of a project that was never
        // migrated (`Project::top_roof`).
        let (kind, material_id) = match project.elements.iter().find(|e| e.id() == &g.roof_id) {
            Some(Element::Roof(r)) => (r.shape.kind(), r.material_id.clone()),
            _ if g.roof_id.is_empty() => {
                let r = project.top_roof();
                (r.kind, r.material_id)
            }
            _ => continue,
        };
        let planes: Vec<RoofPlane> = g.planes.iter().filter_map(|p| plane_of(p, g.vertical_mm)).collect();
        if planes.is_empty() {
            continue;
        }
        out.push(RoofSolid {
            roof_id: g.roof_id.clone(),
            kind,
            material_id,
            level,
            planes,
            infill: g.infill.clone(),
        });
    }
    out
}

/// The `RoofPlane` of one derived plane: its top surface as given, the
/// underside `vertical` below. None for fewer than three corners.
pub fn plane_of(top: &[Vec3], vertical: f64) -> Option<RoofPlane> {
    if top.len() < 3 {
        return None;
    }
    let plan: Vec<V> = top.iter().map(|p| v(p.x, p.y)).collect();
    // The plane z = gx * x + gy * y + c, from its Newell normal.
    let (mut nx, mut ny, mut nz) = (0.0, 0.0, 0.0);
    let n = top.len();
    for i in 0..n {
        let (a, b) = (top[i], top[(i + 1) % n]);
        nx += (a.y - b.y) * (a.z + b.z);
        ny += (a.z - b.z) * (a.x + b.x);
        nz += (a.x - b.x) * (a.y + b.y);
    }
    if nz.abs() < 1e-9 {
        return None;
    }
    let grad = v(-nx / nz, -ny / nz);
    let tan = grad.len();
    let up_slope = if tan > 1e-9 { grad * (1.0 / tan) } else { v(0.0, 1.0) };
    let tan = if tan > 1e-9 { tan } else { 0.0 };
    let cos = 1.0 / (1.0 + tan * tan).sqrt();
    let origin = plan[0];
    Some(RoofPlane {
        plan,
        origin,
        base_h: top[0].z - vertical,
        along: v(up_slope.y, -up_slope.x),
        up_slope,
        tan,
        cos,
        thickness: vertical * cos,
    })
}

/// The derived roof of an element, if it has one.
pub fn geometry_of<'a>(derived: &'a Derived, roof_id: &str) -> Option<&'a RoofGeometry> {
    derived.roofs.iter().find(|g| g.roof_id == roof_id)
}

/// One planar roof surface. `plan` is its footprint in plan coordinates,
/// eaves included.
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

fn clamp_pitch(deg: f64) -> f64 {
    if !deg.is_finite() {
        0.0
    } else {
        deg.clamp(0.0, 60.0)
    }
}

/// Wave 1's roof planes over one footprint, kept for code that draws a
/// `Roof` value with no element behind it. `base` is the wall top height
/// above the level floor. A gable or shed clips the footprint's bounding
/// box; a hip is drawn as a gable. Element roofs come from `Derived::roofs`.
pub fn roof_planes(roof: &Roof, footprint: &[V], base: f64) -> Vec<RoofPlane> {
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
