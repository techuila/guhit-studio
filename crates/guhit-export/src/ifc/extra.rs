//! The Wave 2 parts of the IFC: stairs and roofs from `Derived` (flights,
//! landing slabs, roof planes, gable walls), the grid, the site's latitude
//! and longitude, and sanitary terminals.

use guhit_model::{Asset, AssetCategory, GridLine, Stair, StairGeometry, StairShape, Vec3};

use super::qto::{m2, m3, perimeter, Q};
use super::*;
use crate::pipes::{v3, V3};

/// A landing slab's thickness below its top, mm.
pub(super) const LANDING_THICKNESS_MM: f64 = 150.0;

fn p3(p: &Vec3) -> V3 {
    v3(p.x, p.y, p.z)
}

/// The unit normal of a planar polygon (Newell's method), None when it has
/// no area.
fn normal(poly: &[V3]) -> Option<V3> {
    let mut n = v3(0.0, 0.0, 0.0);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        n = n + v3((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
    }
    n.unit()
}

/// Area of a planar 3D polygon, mm².
pub(super) fn area3(poly: &[Vec3]) -> f64 {
    let pts: Vec<V3> = poly.iter().map(p3).collect();
    let mut n = v3(0.0, 0.0, 0.0);
    for i in 0..pts.len() {
        n = n + pts[i].cross(pts[(i + 1) % pts.len()]);
    }
    n.len() / 2.0
}

/// A sloped or flat polygon (a roof plane's top) swept straight down by
/// `depth`: the profile lies in the polygon's own plane, x level along it.
/// None for a vertical or degenerate polygon.
pub(super) fn sloped_solid(step: &mut Step, poly: &[Vec3], depth: f64) -> Option<Ref> {
    let pts: Vec<V3> = poly.iter().map(p3).collect();
    let n = normal(&pts)?;
    let z = if n.z < 0.0 { n * -1.0 } else { n };
    if z.z < 1e-3 {
        return None;
    }
    let x = v3(0.0, 0.0, 1.0).cross(z).unit().unwrap_or(v3(1.0, 0.0, 0.0));
    let y = z.cross(x);
    let o = pts[0];
    let profile_pts: Vec<V> = pts.iter().map(|p| v((*p - o).dot(x), (*p - o).dot(y))).collect();
    let profile = step.profile(&profile_pts)?;
    let pos = step.axis_rotated((o.x, o.y, o.z), (z.x, z.y, z.z), (x.x, x.y, x.z));
    // Global -z in the plane's frame; x is level, so its z is 0.
    Some(step.extruded_dir(profile, pos, (0.0, -y.z, -z.z), depth))
}

/// A vertical polygon (a gable end) made a wall `thickness` thick, centered
/// on its host wall (`model3d::infill_host`) when the polygon stands along
/// it, else on its own plane. None when it is not vertical.
fn vertical_solid(step: &mut Step, poly: &[Vec3], thickness: f64, host: Option<(f64, V, V)>) -> Option<Ref> {
    let pts: Vec<V3> = poly.iter().map(p3).collect();
    let n = normal(&pts)?;
    if n.z.abs() > 0.05 {
        return None;
    }
    let z = v3(n.x, n.y, 0.0).unit()?;
    let x = v3(0.0, 0.0, 1.0).cross(z).unit()?;
    let y = z.cross(x);
    let o = pts[0];
    let profile_pts: Vec<V> = pts.iter().map(|p| v((*p - o).dot(x), (*p - o).dot(y))).collect();
    let profile = step.profile(&profile_pts)?;
    // The infill may lie on a wall face; the wall it continues is centered
    // on the wall's centerline.
    let shift = crate::model3d::infill_shift(v(o.x, o.y), v(z.x, z.y), host);
    let start = o + z * (shift - thickness / 2.0);
    let pos = step.axis_rotated((start.x, start.y, start.z), (z.x, z.y, z.z), (x.x, x.y, x.z));
    Some(step.extruded(profile, pos, thickness))
}

/// Degrees as an IfcCompoundPlaneAngleMeasure: degrees, minutes, seconds
/// and millionths of a second, all with the angle's sign.
pub(super) fn compound_angle(deg: f64) -> String {
    let sign: i64 = if deg < 0.0 { -1 } else { 1 };
    let micro = (deg.abs() * 3600.0 * 1.0e6).round() as i64;
    let d = micro / 3_600_000_000;
    let m = micro % 3_600_000_000 / 60_000_000;
    let sec = micro % 60_000_000 / 1_000_000;
    let us = micro % 1_000_000;
    format!("({},{},{},{})", sign * d, sign * m, sign * sec, sign * us)
}

/// The IfcSanitaryTerminalTypeEnum of a plumbing fixture, None for every
/// other object. By catalog key: the WC, lavatory, shower, bathtub and
/// sinks of the catalog, and the same words in keys added later.
pub(super) fn sanitary_kind(a: &Asset) -> Option<&'static str> {
    if !matches!(a.category, AssetCategory::Sanitary | AssetCategory::Kitchen | AssetCategory::Furniture) {
        return None;
    }
    let key = a.catalog_key.to_lowercase();
    let words: Vec<&str> = key.split(|c: char| !c.is_alphanumeric()).collect();
    let word = |w: &str| words.contains(&w);
    Some(if word("wc") || key.contains("toilet") || key.contains("water-closet") {
        ".TOILETPAN."
    } else if word("urinal") {
        ".URINAL."
    } else if word("bidet") {
        ".BIDET."
    } else if key.contains("lavatory") || key.contains("basin") {
        ".WASHHANDBASIN."
    } else if key.contains("bathtub") || word("bath") || word("tub") {
        ".BATH."
    } else if key.contains("shower") {
        ".SHOWER."
    } else if key.contains("sink") {
        ".SINK."
    } else {
        return None;
    })
}

impl Build<'_> {
    /// The flights and landing slabs of a stair from `Derived::stairs`, in
    /// storey coordinates, each with its quantities. Returns the parts the
    /// IfcStair aggregates.
    pub(super) fn derived_stair(&mut self, st: &Stair, geo: &StairGeometry, place_parent: Ref, body_ctx: Ref, names: &mut Names) -> Vec<Ref> {
        let mut parts = Vec::new();
        let tread = geo.tread_mm.max(1.0);
        let riser = geo.riser_mm.max(1.0);
        for (k, f) in geo.flights.iter().enumerate() {
            if f.nosings.is_empty() {
                continue;
            }
            let up = V::from(f.up).unit().unwrap_or(v(0.0, 1.0));
            let base = f.nosings[0].z - riser;
            let mut solids = Vec::new();
            let mut volume = 0.0;
            for n in &f.nosings {
                let (a, b) = (V::from(n.a), V::from(n.b));
                let rect = vec![a, b, b + up * tread, a + up * tread];
                let Some(profile) = self.step.profile(&rect) else { continue };
                let pos = self.step.axis_at(0.0, 0.0, base);
                let height = (n.z - base).max(1.0);
                solids.push(self.step.extruded(profile, pos, height));
                volume += m3(m2(polygon_area(&rect)), height);
            }
            if solids.is_empty() {
                continue;
            }
            let body = self.step.shape(body_ctx, "Body", "SweptSolid", &solids);
            let shape = self.step.product_shape(&[body]);
            let place = {
                let a = self.step.axis_at(0.0, 0.0, 0.0);
                self.step.placement(Some(place_parent), a)
            };
            let treads = f.nosings.len() as i64;
            // The first flight keeps the id a straight stair always had.
            let id_role = if k == 0 { "flight".to_string() } else { format!("flight-{k}") };
            let flight = self.step.add(&format!(
                "IFCSTAIRFLIGHT({},$,{},$,$,{place},{shape},$,{},{treads},{},{},.STRAIGHT.)",
                s(&guid_for(&id_role, &st.id)),
                s(&format!("Flight {}", k + 1)),
                treads + 1,
                r(riser),
                r(tread)
            ));
            self.counts.stair_flights += 1;
            self.qto(
                "Qto_StairFlightBaseQuantities",
                &format!("{}:{k}", st.id),
                &[
                    ("Length", Q::Length(tread * treads as f64)),
                    ("GrossVolume", Q::Volume(volume)),
                    ("NetVolume", Q::Volume(volume)),
                ],
                &[flight],
            );
            parts.push(flight);
        }
        for (k, l) in geo.landings.iter().enumerate() {
            let outline: Vec<V> = l.polygon.iter().map(V::from).collect();
            let thick = LANDING_THICKNESS_MM.min(l.z).max(1.0);
            let Some(profile) = self.step.profile(&outline) else { continue };
            let pos = self.step.axis_at(0.0, 0.0, l.z - thick);
            let solid = self.step.extruded(profile, pos, thick);
            let body = self.step.shape(body_ctx, "Body", "SweptSolid", &[solid]);
            let shape = self.step.product_shape(&[body]);
            let place = {
                let a = self.step.axis_at(0.0, 0.0, 0.0);
                self.step.placement(Some(place_parent), a)
            };
            let name = numbered(&mut names.landings, "Landing");
            let slab = self.step.add(&format!(
                "IFCSLAB({},$,{},$,$,{place},{shape},$,.LANDING.)",
                s(&guid_for(&format!("landing-{k}"), &st.id)),
                s(&name)
            ));
            self.counts.slabs += 1;
            self.material_named("Concrete slab", &[slab]);
            self.slab_qto(&format!("{}:landing-{k}", st.id), &outline, thick, 0.0, slab);
            parts.push(slab);
        }
        parts
    }

    /// One IfcSlab .ROOF. per plane of a `Derived::roofs` roof. Returns the
    /// slabs and the sloped area in m².
    pub(super) fn derived_roof_planes(
        &mut self,
        planes: &[Vec<Vec3>],
        vertical_mm: f64,
        guid_of: &dyn Fn(usize) -> String,
        material_id: Option<&String>,
        place_parent: Ref,
        body_ctx: Ref,
    ) -> (Vec<Ref>, f64) {
        let mut out = Vec::new();
        let mut surface = 0.0;
        for (n, plane) in planes.iter().enumerate() {
            let Some(solid) = sloped_solid(&mut self.step, plane, vertical_mm.max(1.0)) else { continue };
            let body = self.step.shape(body_ctx, "Body", "SweptSolid", &[solid]);
            let shape = self.step.product_shape(&[body]);
            let place = {
                let a = self.step.axis_at(0.0, 0.0, 0.0);
                self.step.placement(Some(place_parent), a)
            };
            let id = guid_of(n);
            let slab = self.step.add(&format!(
                "IFCSLAB({},$,{},$,$,{place},{shape},$,.ROOF.)",
                s(&id),
                s(&format!("Roof plane {}", n + 1))
            ));
            self.counts.slabs += 1;
            self.material_for(material_id, "Roofing", slab);
            let area = m2(area3(plane));
            surface += area;
            // Thickness across the plane: the vertical thickness times the
            // cosine of the pitch.
            let pts: Vec<V3> = plane.iter().map(p3).collect();
            let cos = normal(&pts).map(|n| n.z.abs()).unwrap_or(1.0);
            let plan: Vec<V> = plane.iter().map(|p| v(p.x, p.y)).collect();
            self.qto(
                "Qto_SlabBaseQuantities",
                &id,
                &[
                    ("Width", Q::Length(vertical_mm * cos)),
                    ("Perimeter", Q::Length(perimeter(&plan))),
                    ("GrossArea", Q::Area(area)),
                    ("NetArea", Q::Area(area)),
                    ("GrossVolume", Q::Volume(m3(m2(polygon_area(&plan)), vertical_mm))),
                ],
                &[slab],
            );
            out.push(slab);
        }
        (out, surface)
    }

    /// Gable ends as walls: each infill polygon of a roof made a wall as
    /// thick as the nearest wall under it (150 mm when none is near), an
    /// IfcWall .USERDEFINED. 'GABLEWALL' (IFC4 has no gable wall type), so
    /// the house walls stay the only .SOLIDWALL.s.
    pub(super) fn gable_walls(
        &mut self,
        infill: &[Vec<Vec3>],
        walls: &[WallSolid],
        owner: &str,
        place_parent: Ref,
        body_ctx: Ref,
        names: &mut Names,
    ) -> Vec<Ref> {
        let mut out = Vec::new();
        for (k, poly) in infill.iter().enumerate() {
            if poly.len() < 3 {
                continue;
            }
            // The nearest wall within a meter: its thickness and centerline.
            let host = crate::model3d::infill_host(poly, walls);
            let thickness = host.map(|h| h.0).unwrap_or(150.0).max(1.0);
            let Some(solid) = vertical_solid(&mut self.step, poly, thickness, host) else { continue };
            let body = self.step.shape(body_ctx, "Body", "SweptSolid", &[solid]);
            let shape = self.step.product_shape(&[body]);
            let place = {
                let a = self.step.axis_at(0.0, 0.0, 0.0);
                self.step.placement(Some(place_parent), a)
            };
            let name = numbered(&mut names.gables, "Gable wall");
            let id = guid_for(&format!("gable-{k}"), owner);
            let wall = self.step.add(&format!("IFCWALL({},$,{},$,'GABLEWALL',{place},{shape},$,.USERDEFINED.)", s(&id), s(&name)));
            self.counts.walls += 1;
            self.material_named("Wall", &[wall]);
            let area = m2(area3(poly));
            let length = {
                let pts: Vec<V> = poly.iter().map(|p| v(p.x, p.y)).collect();
                let b = bounds_of(&pts);
                b.width().hypot(b.height())
            };
            self.qto(
                "Qto_WallBaseQuantities",
                &id,
                &[
                    ("Length", Q::Length(length)),
                    ("Width", Q::Length(thickness)),
                    ("GrossSideArea", Q::Area(area)),
                    ("NetSideArea", Q::Area(area)),
                    ("GrossVolume", Q::Volume(m3(area, thickness))),
                    ("NetVolume", Q::Volume(m3(area, thickness))),
                ],
                &[wall],
            );
            out.push(wall);
        }
        out
    }

    /// The project's grid lines as one IfcGrid: lines along the first
    /// line's direction are the U axes, the rest the V axes. None without
    /// grid lines in two directions, which IFC needs.
    pub(super) fn grid(&mut self, lines: &[&GridLine], project_id: &str, place_parent: Ref, ctx: Ref) -> Option<Ref> {
        let first = lines.first()?;
        let d0 = (V::from(first.end) - V::from(first.start)).unit()?;
        let along = |g: &GridLine| {
            (V::from(g.end) - V::from(g.start))
                .unit()
                .map(|d| (d.x * d0.y - d.y * d0.x).abs() < 0.05)
                .unwrap_or(false)
        };
        let (u, w): (Vec<&GridLine>, Vec<&GridLine>) = lines.iter().copied().partition(|g| along(g));
        if u.is_empty() || w.is_empty() {
            return None;
        }
        let square = w.iter().all(|g| {
            (V::from(g.end) - V::from(g.start)).unit().map(|d| d.dot(d0).abs() < 0.05).unwrap_or(false)
        });
        let mut curves = Vec::new();
        let mut axis = |b: &mut Build, g: &GridLine| {
            let a = b.step.point2(g.start.x, g.start.y);
            let e = b.step.point2(g.end.x, g.end.y);
            let line = b.step.add(&format!("IFCPOLYLINE(({a},{e}))"));
            curves.push(line);
            b.step.add(&format!("IFCGRIDAXIS({},{line},.T.)", s(&crate::text::clean(&g.label))))
        };
        let u_axes: Vec<Ref> = u.iter().map(|g| axis(self, g)).collect();
        let v_axes: Vec<Ref> = w.iter().map(|g| axis(self, g)).collect();
        let set = self.step.add(&format!("IFCGEOMETRICCURVESET({})", list(&curves)));
        let rep = self.step.shape(ctx, "FootPrint", "GeometricCurveSet", &[set]);
        let shape = self.step.product_shape(&[rep]);
        let place = {
            let a = self.step.axis_at(0.0, 0.0, 0.0);
            self.step.placement(Some(place_parent), a)
        };
        let grid = self.step.add(&format!(
            "IFCGRID({},$,'Grid',$,$,{place},{shape},{},{},$,{})",
            s(&guid_for("grid", project_id)),
            list(&u_axes),
            list(&v_axes),
            if square { ".RECTANGULAR." } else { ".IRREGULAR." }
        ));
        self.counts.grids += 1;
        Some(grid)
    }
}

/// IfcStairTypeEnum of a stair's shape.
pub(super) fn stair_kind(shape: StairShape) -> &'static str {
    match shape {
        StairShape::Straight => ".STRAIGHT_RUN_STAIR.",
        StairShape::L => ".QUARTER_TURN_STAIR.",
        StairShape::U => ".HALF_TURN_STAIR.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latitude_and_longitude_as_compound_angles() {
        assert_eq!(compound_angle(14.5995), "(14,35,58,200000)");
        assert_eq!(compound_angle(120.9842), "(120,59,3,120000)");
        assert_eq!(compound_angle(-33.5), "(-33,-30,0,0)");
        assert_eq!(compound_angle(0.0), "(0,0,0,0)");
    }
}
