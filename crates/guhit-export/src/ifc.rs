//! IFC4 export as a hand written STEP physical file (ISO-10303-21).
//!
//! No IFC library: the file is a flat list of numbered entities, so writing it
//! directly keeps the crate dependency free and the output reviewable.
//!
//! Layout of the file:
//! 1. units, geometric representation context and its Body / Axis / Annotation
//!    subcontexts,
//! 2. the spatial spine IfcProject, IfcSite, IfcBuilding, one
//!    IfcBuildingStorey per level, tied together with IfcRelAggregates,
//! 3. the elements of each storey, tied to it with
//!    IfcRelContainedInSpatialStructure,
//! 4. property sets and material associations.
//!
//! Geometry is written in storey local coordinates: plan x and y as modelled,
//! z measured from the level floor. The storey placement carries the level
//! elevation, so a viewer sees the levels stacked.
//!
//! IfcOwnerHistory is optional in IFC4 and is written as `$` throughout.
//!
//! Simplifications, on purpose:
//! - Walls are IfcWall with PredefinedType .SOLIDWALL., not
//!   IfcWallStandardCase. That subtype demands an IfcMaterialLayerSetUsage,
//!   and the model carries one material per wall, not a layer build-up.
//! - A gable roof is one IfcRoof aggregating two sloped IfcSlab planes. The
//!   triangular gable ends are not written as walls: the walls keep their own
//!   height and the roof planes cover them, exactly as the live 3D view does.
//! - Openings are boxes, so an arched or shaped head is squared off. The void
//!   is 10 mm deeper than the wall on each face so it cuts clean everywhere.
//! - A door or window is a single thin box, not a frame with leaves and
//!   glazing. It exists so the hole reads as an opening in a viewer.
//! - A stair is one IfcStairFlight box the full level height, with the riser
//!   and tread numbers as properties. Individual treads are not modelled.
//! - Dimension elements are skipped: IFC has no plain 2D dimension in this
//!   context and a wrong one is worse than none.
//! - Linework, ReferenceModel, Underlay and Camera elements are skipped. They
//!   are tracing and viewing aids, not building elements.

use std::collections::HashMap;
use std::fmt::Write as _;

use guhit_model::{
    ColumnShape, Derived, Opening, OpeningStyle, OpeningType, Project, RoofKind, RoomUsage,
};

use crate::geom::*;
use crate::model3d::*;
use crate::ExportError;

// ------------------------------------------------------------------ IFC GUID

const B64: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz_$";

/// The 22 character IFC base64 form of a 128 bit id. The first character
/// carries 2 bits, the remaining 21 carry 6 each.
pub fn encode_guid(bytes: [u8; 16]) -> String {
    let mut n = u128::from_be_bytes(bytes);
    let mut out = [b'0'; 22];
    for i in (0..22).rev() {
        out[i] = B64[(n % 64) as usize];
        n /= 64;
    }
    String::from_utf8(out.to_vec()).expect("base64 alphabet is ascii")
}

/// 16 bytes from a UUID string, or None when it is not a UUID.
fn uuid_bytes(id: &str) -> Option<[u8; 16]> {
    let hex: Vec<u8> = id.bytes().filter(|b| *b != b'-').collect();
    if hex.len() != 32 || id.bytes().filter(|b| *b == b'-').count() > 4 {
        return None;
    }
    let mut out = [0u8; 16];
    for i in 0..16 {
        let hi = (hex[i * 2] as char).to_digit(16)?;
        let lo = (hex[i * 2 + 1] as char).to_digit(16)?;
        out[i] = (hi * 16 + lo) as u8;
    }
    Some(out)
}

/// FNV-1a over 128 bits. Deterministic across runs and platforms, which is
/// what keeps re-exported ids stable.
fn hash_bytes(s: &str) -> [u8; 16] {
    let mut h: u128 = 0x6c62272e07bb014262b821756295c58d;
    const PRIME: u128 = 0x0000000001000000000000000000013b;
    for b in s.as_bytes() {
        h ^= *b as u128;
        h = h.wrapping_mul(PRIME);
    }
    h.to_be_bytes()
}

/// Stable IfcGloballyUniqueId for an element. UUID ids keep their own bytes,
/// so the same project always exports the same ids.
pub fn guid(id: &str) -> String {
    encode_guid(uuid_bytes(id).unwrap_or_else(|| hash_bytes(id)))
}

/// Stable id for an entity derived from an element, for example the opening
/// that a door fills or the property set of a wall.
pub fn guid_for(role: &str, id: &str) -> String {
    encode_guid(hash_bytes(&format!("{role}:{id}")))
}

// ------------------------------------------------------------- STEP plumbing

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Ref(usize);

impl std::fmt::Display for Ref {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// A STEP real always carries a decimal point.
fn r(x: f64) -> String {
    let x = if x.is_finite() { x } else { 0.0 };
    let mut s = format!("{x:.6}");
    while s.ends_with('0') {
        s.pop();
    }
    if s == "-0." {
        s = "0.".into();
    }
    s
}

/// STEP string literal. Single quotes double up, non ASCII goes into the
/// \X2\ ... \X0\ escape as UTF-16 code units.
fn s(text: &str) -> String {
    let mut out = String::from("'");
    let mut esc: Option<String> = None;
    for c in text.chars() {
        let plain = c.is_ascii() && !c.is_ascii_control();
        if plain {
            if let Some(e) = esc.take() {
                out.push_str(&format!("\\X2\\{e}\\X0\\"));
            }
            if c == '\'' {
                out.push_str("''");
            } else if c == '\\' {
                out.push_str("\\\\");
            } else {
                out.push(c);
            }
        } else {
            let mut buf = [0u16; 2];
            let units = c.encode_utf16(&mut buf);
            let mut hex = String::new();
            for u in units.iter() {
                let _ = write!(hex, "{u:04X}");
            }
            esc.get_or_insert_with(String::new).push_str(&hex);
        }
    }
    if let Some(e) = esc {
        out.push_str(&format!("\\X2\\{e}\\X0\\"));
    }
    out.push('\'');
    out
}

fn list(refs: &[Ref]) -> String {
    let parts: Vec<String> = refs.iter().map(|x| x.to_string()).collect();
    format!("({})", parts.join(","))
}

struct Step {
    body: String,
    next: usize,
    /// Cache for the many repeated points and directions.
    cache: HashMap<String, Ref>,
}

impl Step {
    fn new() -> Step {
        Step {
            body: String::new(),
            next: 0,
            cache: HashMap::new(),
        }
    }

    fn add(&mut self, entity: &str) -> Ref {
        self.next += 1;
        let _ = writeln!(self.body, "#{}={};", self.next, entity);
        Ref(self.next)
    }

    /// Adds the entity once and reuses it afterwards. Only for value types
    /// with no identity of their own (points, directions, placements).
    fn shared(&mut self, entity: &str) -> Ref {
        if let Some(r) = self.cache.get(entity) {
            return *r;
        }
        let r = self.add(entity);
        self.cache.insert(entity.to_string(), r);
        r
    }

    fn point3(&mut self, x: f64, y: f64, z: f64) -> Ref {
        self.shared(&format!(
            "IFCCARTESIANPOINT(({},{},{}))",
            r(x),
            r(y),
            r(z)
        ))
    }

    fn point2(&mut self, x: f64, y: f64) -> Ref {
        self.shared(&format!("IFCCARTESIANPOINT(({},{}))", r(x), r(y)))
    }

    fn dir3(&mut self, x: f64, y: f64, z: f64) -> Ref {
        self.shared(&format!("IFCDIRECTION(({},{},{}))", r(x), r(y), r(z)))
    }

    /// Placement at `z` above the parent origin, axes unrotated.
    fn axis_at(&mut self, x: f64, y: f64, z: f64) -> Ref {
        let p = self.point3(x, y, z);
        self.shared(&format!("IFCAXIS2PLACEMENT3D({p},$,$)"))
    }

    fn axis_rotated(&mut self, loc: (f64, f64, f64), axis: (f64, f64, f64), refd: (f64, f64, f64)) -> Ref {
        let p = self.point3(loc.0, loc.1, loc.2);
        let a = self.dir3(axis.0, axis.1, axis.2);
        let d = self.dir3(refd.0, refd.1, refd.2);
        self.shared(&format!("IFCAXIS2PLACEMENT3D({p},{a},{d})"))
    }

    fn placement(&mut self, parent: Option<Ref>, rel: Ref) -> Ref {
        let p = parent.map(|x| x.to_string()).unwrap_or_else(|| "$".into());
        self.add(&format!("IFCLOCALPLACEMENT({p},{rel})"))
    }

    /// Closed 2D polyline profile from plan points. Winding is normalised so
    /// the outer curve runs counter-clockwise.
    fn profile(&mut self, poly: &[V]) -> Option<Ref> {
        let ccw = ensure_ccw(poly);
        if ccw.len() < 3 {
            return None;
        }
        let mut pts: Vec<Ref> = ccw.iter().map(|p| self.point2(p.x, p.y)).collect();
        pts.push(pts[0]);
        let line = self.add(&format!("IFCPOLYLINE({})", list(&pts)));
        Some(self.add(&format!("IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,{line})")))
    }

    fn extruded(&mut self, profile: Ref, position: Ref, depth: f64) -> Ref {
        self.extruded_dir(profile, position, (0.0, 0.0, 1.0), depth)
    }

    /// Extrusion along an arbitrary local direction. A roof plane uses it to
    /// sweep straight up instead of along its own normal, so the slab is
    /// bounded by two vertically offset planes, exactly like the 3D viewer.
    fn extruded_dir(
        &mut self,
        profile: Ref,
        position: Ref,
        d: (f64, f64, f64),
        depth: f64,
    ) -> Ref {
        let dir = self.dir3(d.0, d.1, d.2);
        self.add(&format!(
            "IFCEXTRUDEDAREASOLID({profile},{position},{dir},{})",
            r(depth.max(1.0))
        ))
    }

    fn shape(&mut self, ctx: Ref, ident: &str, ty: &str, items: &[Ref]) -> Ref {
        self.add(&format!(
            "IFCSHAPEREPRESENTATION({ctx},{},{},{})",
            s(ident),
            s(ty),
            list(items)
        ))
    }

    fn product_shape(&mut self, reps: &[Ref]) -> Ref {
        self.add(&format!("IFCPRODUCTDEFINITIONSHAPE($,$,{})", list(reps)))
    }
}

/// A property value written into an IfcPropertySingleValue.
enum Val {
    Bool(bool),
    Text(String),
    Area(f64),
    Length(f64),
    Count(i64),
}

impl Val {
    fn step(&self) -> String {
        match self {
            Val::Bool(b) => format!("IFCBOOLEAN(.{}.)", if *b { "T" } else { "F" }),
            Val::Text(t) => format!("IFCLABEL({})", s(t)),
            Val::Area(a) => format!("IFCAREAMEASURE({})", r(*a)),
            Val::Length(l) => format!("IFCLENGTHMEASURE({})", r(*l)),
            Val::Count(c) => format!("IFCCOUNTMEASURE({c})"),
        }
    }
}

// ----------------------------------------------------------------- labelling

fn usage_label(u: RoomUsage) -> &'static str {
    match u {
        RoomUsage::Living => "Living room",
        RoomUsage::Dining => "Dining room",
        RoomUsage::Kitchen => "Kitchen",
        RoomUsage::Bedroom => "Bedroom",
        RoomUsage::MasterBedroom => "Master bedroom",
        RoomUsage::Bathroom => "Bathroom",
        RoomUsage::PowderRoom => "Powder room",
        RoomUsage::Laundry => "Laundry",
        RoomUsage::Garage => "Garage",
        RoomUsage::Porch => "Porch",
        RoomUsage::Hallway => "Hallway",
        RoomUsage::Storage => "Storage",
        RoomUsage::Office => "Office",
        RoomUsage::Other => "Room",
    }
}

fn style_label(style: OpeningStyle) -> &'static str {
    match style {
        OpeningStyle::SwingSingle => "swing single",
        OpeningStyle::SwingDouble => "swing double",
        OpeningStyle::Sliding => "sliding",
        OpeningStyle::Fixed => "fixed",
        OpeningStyle::Casement => "casement",
        OpeningStyle::Jalousie => "jalousie",
    }
}

/// IfcDoorTypeOperationEnum from the project's style and flip flags.
/// The contract says `flip_side == false` swings to the left of the wall
/// direction start to end, and `flip_hinge == false` hinges on the jamb
/// nearer the wall start.
fn door_operation(o: &Opening) -> &'static str {
    match o.style {
        OpeningStyle::SwingDouble => ".DOUBLE_DOOR_SINGLE_SWING.",
        OpeningStyle::Sliding => {
            if o.flip_hinge {
                ".SLIDING_TO_RIGHT."
            } else {
                ".SLIDING_TO_LEFT."
            }
        }
        OpeningStyle::Fixed => ".NOTDEFINED.",
        _ => {
            if o.flip_side {
                ".SINGLE_SWING_RIGHT."
            } else {
                ".SINGLE_SWING_LEFT."
            }
        }
    }
}

fn swing_side(o: &Opening) -> &'static str {
    if o.flip_side {
        "right"
    } else {
        "left"
    }
}

fn hinge_side(o: &Opening) -> &'static str {
    if o.flip_hinge {
        "end"
    } else {
        "start"
    }
}

fn window_partitioning(style: OpeningStyle) -> &'static str {
    match style {
        OpeningStyle::Fixed => ".SINGLE_PANEL.",
        OpeningStyle::Sliding => ".DOUBLE_PANEL_HORIZONTAL.",
        OpeningStyle::Casement => ".SINGLE_PANEL.",
        OpeningStyle::Jalousie => ".USERDEFINED.",
        _ => ".NOTDEFINED.",
    }
}

/// Timestamp for FILE_NAME. `updated_at` is RFC 3339; anything else falls back
/// to a fixed value so the output stays reproducible.
fn header_time(project: &Project) -> String {
    let t = project.updated_at.trim();
    let ok = t.len() >= 19
        && t.is_char_boundary(19)
        && t.as_bytes()[4] == b'-'
        && t.as_bytes()[10] == b'T';
    if ok {
        t[..19].to_string()
    } else {
        "1970-01-01T00:00:00".to_string()
    }
}

// ------------------------------------------------------------------ the file

struct Build<'a> {
    step: Step,
    project: &'a Project,
    /// Elements grouped per storey for IfcRelContainedInSpatialStructure.
    contained: Vec<Vec<Ref>>,
    /// Material name to the elements that use it.
    materials: Vec<(String, Vec<Ref>)>,
    counts: Counts,
}

/// What the file ended up holding. Used by the tests.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Counts {
    pub walls: usize,
    pub openings: usize,
    pub doors: usize,
    pub windows: usize,
    pub spaces: usize,
    pub slabs: usize,
    pub roofs: usize,
    pub columns: usize,
    pub stairs: usize,
    pub furnishings: usize,
    pub annotations: usize,
}

impl<'a> Build<'a> {
    fn material_for(&mut self, id: Option<&String>, fallback: &str, element: Ref) {
        let name = id
            .and_then(|m| self.project.materials.iter().find(|x| &x.id == m))
            .map(|m| m.name.clone())
            .unwrap_or_else(|| fallback.to_string());
        match self.materials.iter_mut().find(|(n, _)| *n == name) {
            Some((_, list)) => list.push(element),
            None => self.materials.push((name, vec![element])),
        }
    }

    fn pset(&mut self, name: &str, owner_id: &str, props: &[(&str, Val)], elements: &[Ref]) {
        if props.is_empty() || elements.is_empty() {
            return;
        }
        let prop_refs: Vec<Ref> = props
            .iter()
            .map(|(n, v)| {
                self.step.add(&format!(
                    "IFCPROPERTYSINGLEVALUE({},$,{},$)",
                    s(n),
                    v.step()
                ))
            })
            .collect();
        let set = self.step.add(&format!(
            "IFCPROPERTYSET({},$,{},$,{})",
            s(&guid_for(name, owner_id)),
            s(name),
            list(&prop_refs)
        ));
        self.step.add(&format!(
            "IFCRELDEFINESBYPROPERTIES({},$,$,$,{},{set})",
            s(&guid_for(&format!("{name}-rel"), owner_id)),
            list(elements)
        ));
    }
}

/// Write the whole project as an IFC4 STEP file.
pub fn write(project: &Project, derived: &Derived) -> Result<String, ExportError> {
    Ok(write_with_counts(project, derived)?.0)
}

/// Same as `write`, and also reports what went in.
pub fn write_with_counts(
    project: &Project,
    derived: &Derived,
) -> Result<(String, Counts), ExportError> {
    if project.levels.is_empty() {
        return Err(ExportError::Empty("the project has no levels".into()));
    }
    let scene = build_scene(project, derived);
    let has_geometry = scene.levels.iter().any(|l| {
        !l.walls.is_empty()
            || !l.rooms.is_empty()
            || !l.footprints.is_empty()
            || !l.columns.is_empty()
            || !l.stairs.is_empty()
            || !l.assets.is_empty()
    });
    if !has_geometry {
        return Err(ExportError::Empty("the model has nothing to export".into()));
    }

    let mut step = Step::new();

    // ------------------------------------------------------------ units
    let len_unit = step.add("IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.)");
    let area_unit = step.add("IFCSIUNIT(*,.AREAUNIT.,$,.SQUARE_METRE.)");
    let vol_unit = step.add("IFCSIUNIT(*,.VOLUMEUNIT.,$,.CUBIC_METRE.)");
    let rad_unit = step.add("IFCSIUNIT(*,.PLANEANGLEUNIT.,$,.RADIAN.)");
    // Written out in full: `r` rounds to 1e-6, which is plenty for a length in
    // millimeters but too coarse for a unit conversion factor.
    let degree_factor = step.add(&format!(
        "IFCMEASUREWITHUNIT(IFCPLANEANGLEMEASURE({:.16}),{rad_unit})",
        std::f64::consts::PI / 180.0
    ));
    let exponents = step.add("IFCDIMENSIONALEXPONENTS(0,0,0,0,0,0,0)");
    let deg_unit = step.add(&format!(
        "IFCCONVERSIONBASEDUNIT({exponents},.PLANEANGLEUNIT.,'DEGREE',{degree_factor})"
    ));
    let units = step.add(&format!(
        "IFCUNITASSIGNMENT(({len_unit},{area_unit},{vol_unit},{deg_unit}))"
    ));

    // -------------------------------------------- representation contexts
    let wcs = step.axis_at(0.0, 0.0, 0.0);
    let ctx = step.add(&format!(
        "IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,{wcs},$)"
    ));
    let body_ctx = step.add(&format!(
        "IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Body','Model',*,*,*,*,{ctx},$,.MODEL_VIEW.,$)"
    ));
    let axis_ctx = step.add(&format!(
        "IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Axis','Model',*,*,*,*,{ctx},$,.GRAPH_VIEW.,$)"
    ));
    let anno_ctx = step.add(&format!(
        "IFCGEOMETRICREPRESENTATIONSUBCONTEXT('Annotation','Model',*,*,*,*,{ctx},$,.PLAN_VIEW.,$)"
    ));

    // ------------------------------------------------------------- spine
    let ifc_project = step.add(&format!(
        "IFCPROJECT({},$,{},$,$,$,$,({ctx}),{units})",
        s(&guid(&project.id)),
        s(&project.name)
    ));
    let site_place = {
        let a = step.axis_at(0.0, 0.0, 0.0);
        step.placement(None, a)
    };
    let site_name = if project.settings.location.trim().is_empty() {
        "Site".to_string()
    } else {
        project.settings.location.trim().to_string()
    };
    let site = step.add(&format!(
        "IFCSITE({},$,{},$,$,{site_place},$,$,.ELEMENT.,$,$,$,$,$)",
        s(&guid_for("site", &project.id)),
        s(&site_name)
    ));
    let building_place = {
        let a = step.axis_at(0.0, 0.0, 0.0);
        step.placement(Some(site_place), a)
    };
    let building = step.add(&format!(
        "IFCBUILDING({},$,{},$,$,{building_place},$,$,.ELEMENT.,$,$,$)",
        s(&guid_for("building", &project.id)),
        s(&project.name)
    ));

    let mut build = Build {
        step,
        project,
        contained: vec![Vec::new(); scene.levels.len()],
        materials: Vec::new(),
        counts: Counts::default(),
    };

    // ---------------------------------------------------------- storeys
    let mut storeys: Vec<Ref> = Vec::new();
    let mut storey_places: Vec<Ref> = Vec::new();
    for ls in &scene.levels {
        let a = build.step.axis_at(0.0, 0.0, ls.level.elevation_mm);
        let place = build.step.placement(Some(building_place), a);
        let storey = build.step.add(&format!(
            "IFCBUILDINGSTOREY({},$,{},$,$,{place},$,$,.ELEMENT.,{})",
            s(&guid(&ls.level.id)),
            s(&ls.level.name),
            r(ls.level.elevation_mm)
        ));
        storeys.push(storey);
        storey_places.push(place);
    }

    // ---------------------------------------------------------- elements
    let mut spaces_per_storey: Vec<Vec<Ref>> = vec![Vec::new(); scene.levels.len()];
    let mut wall_refs: HashMap<&str, Ref> = HashMap::new();

    for (i, ls) in scene.levels.iter().enumerate() {
        let place_parent = storey_places[i];

        // ------------------------------------------------------- walls
        for (n, w) in ls.walls.iter().enumerate() {
            let a = build.step.axis_at(0.0, 0.0, 0.0);
            let place = build.step.placement(Some(place_parent), a);
            let Some(profile) = build.step.profile(&w.plan_outline) else {
                continue;
            };
            let pos = build.step.axis_at(0.0, 0.0, 0.0);
            let solid = build.step.extruded(profile, pos, w.height);
            let body = build.step.shape(body_ctx, "Body", "SweptSolid", &[solid]);
            let a0 = build.step.point2(w.wall.start.x, w.wall.start.y);
            let a1 = build.step.point2(w.wall.end.x, w.wall.end.y);
            let axis_line = build.step.add(&format!("IFCPOLYLINE({})", list(&[a0, a1])));
            let axis = build
                .step
                .shape(axis_ctx, "Axis", "Curve2D", &[axis_line]);
            let shape = build.step.product_shape(&[axis, body]);
            let wall = build.step.add(&format!(
                "IFCWALL({},$,{},$,$,{place},{shape},$,.SOLIDWALL.)",
                s(&guid(&w.wall.id)),
                s(&format!("Wall {}", n + 1))
            ));
            build.contained[i].push(wall);
            wall_refs.insert(w.wall.id.as_str(), wall);
            build.counts.walls += 1;
            build.material_for(w.wall.material_id.as_ref(), "Wall", wall);
            build.pset(
                "Pset_WallCommon",
                &w.wall.id,
                &[
                    ("Reference", Val::Text(format!("Wall {}", n + 1))),
                    ("IsExternal", Val::Bool(w.exterior)),
                    ("LoadBearing", Val::Bool(false)),
                ],
                &[wall],
            );

            // -------------------------------------------- openings
            for o in &w.openings {
                let sill = o.sill_mm.max(0.0);
                let height = o.height_mm.max(1.0).min((w.height - sill).max(1.0));
                if height < 1.0 || o.width_mm < 1.0 {
                    continue;
                }
                // The void is a little deeper than the wall so it cuts clean
                // through both faces in every viewer.
                let void_rect = opening_rect(w, o, w.wall.thickness_mm.max(1.0) + 20.0);
                let Some(void_profile) = build.step.profile(&void_rect) else {
                    continue;
                };
                let void_pos = build.step.axis_at(0.0, 0.0, sill);
                let void_solid = build.step.extruded(void_profile, void_pos, height);
                let void_body = build
                    .step
                    .shape(body_ctx, "Body", "SweptSolid", &[void_solid]);
                let void_shape = build.step.product_shape(&[void_body]);
                let o_place = {
                    let a = build.step.axis_at(0.0, 0.0, 0.0);
                    build.step.placement(Some(place_parent), a)
                };
                let opening = build.step.add(&format!(
                    "IFCOPENINGELEMENT({},$,{},$,$,{o_place},{void_shape},$,.OPENING.)",
                    s(&guid_for("void", &o.id)),
                    s(&format!(
                        "{} opening",
                        match o.opening_type {
                            OpeningType::Door => "Door",
                            OpeningType::Window => "Window",
                        }
                    ))
                ));
                build.step.add(&format!(
                    "IFCRELVOIDSELEMENT({},$,$,$,{wall},{opening})",
                    s(&guid_for("voids", &o.id))
                ));
                build.counts.openings += 1;

                // The leaf or sash: a thin box inside the opening, so a
                // viewer shows something where the hole is.
                let panel_depth = match o.opening_type {
                    OpeningType::Door => 45.0,
                    OpeningType::Window => 25.0,
                };
                let panel_rect = opening_rect(w, o, panel_depth);
                let Some(panel_profile) = build.step.profile(&panel_rect) else {
                    continue;
                };
                let panel_pos = build.step.axis_at(0.0, 0.0, sill);
                let panel_solid = build.step.extruded(panel_profile, panel_pos, height);
                let panel_body =
                    build
                        .step
                        .shape(body_ctx, "Body", "SweptSolid", &[panel_solid]);
                let panel_shape = build.step.product_shape(&[panel_body]);
                let f_place = {
                    let a = build.step.axis_at(0.0, 0.0, 0.0);
                    build.step.placement(Some(place_parent), a)
                };
                let filler = match o.opening_type {
                    OpeningType::Door => build.step.add(&format!(
                        "IFCDOOR({},$,{},$,$,{f_place},{panel_shape},$,{},{},.DOOR.,{},$)",
                        s(&guid(&o.id)),
                        s(&format!("Door {}", style_label(o.style))),
                        r(height),
                        r(o.width_mm),
                        door_operation(o)
                    )),
                    OpeningType::Window => build.step.add(&format!(
                        "IFCWINDOW({},$,{},$,$,{f_place},{panel_shape},$,{},{},.WINDOW.,{},$)",
                        s(&guid(&o.id)),
                        s(&format!("Window {}", style_label(o.style))),
                        r(height),
                        r(o.width_mm),
                        window_partitioning(o.style)
                    )),
                };
                build.step.add(&format!(
                    "IFCRELFILLSELEMENT({},$,$,$,{opening},{filler})",
                    s(&guid_for("fills", &o.id))
                ));
                build.contained[i].push(filler);
                match o.opening_type {
                    OpeningType::Door => build.counts.doors += 1,
                    OpeningType::Window => build.counts.windows += 1,
                }
                let default_material = match o.opening_type {
                    OpeningType::Door => "Door leaf",
                    OpeningType::Window => "Glazing",
                };
                build.material_for(o.material_id.as_ref(), default_material, filler);

                let common = if o.opening_type == OpeningType::Door {
                    "Pset_DoorCommon"
                } else {
                    "Pset_WindowCommon"
                };
                build.pset(
                    common,
                    &o.id,
                    &[
                        ("IsExternal", Val::Bool(w.exterior)),
                        ("Reference", Val::Text(style_label(o.style).to_string())),
                    ],
                    &[filler],
                );
                // Pset_DoorCommon has no swing side, so the contract's flip
                // convention goes into our own property set.
                let mut guhit: Vec<(&str, Val)> = vec![
                    ("Style", Val::Text(style_label(o.style).to_string())),
                    ("SillHeight", Val::Length(sill)),
                    ("HostWallOffset", Val::Length(o.offset_mm)),
                ];
                if o.opening_type == OpeningType::Door {
                    guhit.push(("SwingSide", Val::Text(swing_side(o).to_string())));
                    guhit.push(("HingeSide", Val::Text(hinge_side(o).to_string())));
                }
                build.pset("Guhit_Pset_Opening", &o.id, &guhit, &[filler]);
            }
        }

        // ------------------------------------------------------- slabs
        for (n, fp) in ls.footprints.iter().enumerate() {
            let Some(profile) = build.step.profile(fp) else {
                continue;
            };
            let pos = build.step.axis_at(0.0, 0.0, -SLAB_THICKNESS_MM);
            let solid = build.step.extruded(profile, pos, SLAB_THICKNESS_MM);
            let body = build.step.shape(body_ctx, "Body", "SweptSolid", &[solid]);
            let shape = build.step.product_shape(&[body]);
            let place = {
                let a = build.step.axis_at(0.0, 0.0, 0.0);
                build.step.placement(Some(place_parent), a)
            };
            let slab = build.step.add(&format!(
                "IFCSLAB({},$,{},$,$,{place},{shape},$,.FLOOR.)",
                s(&guid_for(&format!("slab-{n}"), &ls.level.id)),
                s(&format!("Floor slab {}", n + 1))
            ));
            build.contained[i].push(slab);
            build.counts.slabs += 1;
            build.material_for(None, "Concrete slab", slab);
        }

        // ------------------------------------------------------ spaces
        for rm in &ls.rooms {
            let Some(profile) = build.step.profile(&rm.polygon) else {
                continue;
            };
            let pos = build.step.axis_at(0.0, 0.0, 0.0);
            let solid = build
                .step
                .extruded(profile, pos, ls.level.height_mm.max(1.0));
            let body = build.step.shape(body_ctx, "Body", "SweptSolid", &[solid]);
            let shape = build.step.product_shape(&[body]);
            let place = {
                let a = build.step.axis_at(0.0, 0.0, 0.0);
                build.step.placement(Some(place_parent), a)
            };
            let space = build.step.add(&format!(
                "IFCSPACE({},$,{},$,$,{place},{shape},{},.ELEMENT.,.SPACE.,{})",
                s(&guid(&rm.room.id)),
                s(&rm.room.name),
                s(usage_label(rm.room.usage)),
                r(0.0)
            ));
            spaces_per_storey[i].push(space);
            build.counts.spaces += 1;
            let area_m2 = rm.area_mm2 / 1_000_000.0;
            build.pset(
                "Pset_SpaceCommon",
                &rm.room.id,
                &[
                    ("Reference", Val::Text(usage_label(rm.room.usage).to_string())),
                    ("IsExternal", Val::Bool(false)),
                    ("GrossPlannedArea", Val::Area(area_m2)),
                    ("NetPlannedArea", Val::Area(area_m2)),
                ],
                &[space],
            );
            // Space boundaries to the walls that enclose the room.
            for wid in &rm.wall_ids {
                let Some(wall) = wall_refs.get(wid.as_str()) else {
                    continue;
                };
                build.step.add(&format!(
                    "IFCRELSPACEBOUNDARY({},$,$,$,{space},{wall},$,.PHYSICAL.,.INTERNAL.)",
                    s(&guid_for(&format!("bound-{wid}"), &rm.room.id))
                ));
            }
        }

        // ----------------------------------------------------- columns
        for c in &ls.columns {
            let place = {
                let a = build.step.axis_at(0.0, 0.0, 0.0);
                build.step.placement(Some(place_parent), a)
            };
            let profile = match c.shape {
                ColumnShape::Round => {
                    let p = build.step.point2(c.center.x, c.center.y);
                    let a2 = build
                        .step
                        .shared(&format!("IFCAXIS2PLACEMENT2D({p},$)"));
                    Some(build.step.add(&format!(
                        "IFCCIRCLEPROFILEDEF(.AREA.,$,{a2},{})",
                        r(c.width_mm.max(1.0) / 2.0)
                    )))
                }
                ColumnShape::Rect => {
                    let poly = rect(
                        V::from(c.center),
                        c.width_mm.max(1.0),
                        c.depth_mm.max(1.0),
                        c.rotation_deg,
                    );
                    build.step.profile(&poly)
                }
            };
            let Some(profile) = profile else { continue };
            let pos = build.step.axis_at(0.0, 0.0, 0.0);
            let solid = build
                .step
                .extruded(profile, pos, ls.level.height_mm.max(1.0));
            let body = build.step.shape(body_ctx, "Body", "SweptSolid", &[solid]);
            let shape = build.step.product_shape(&[body]);
            let column = build.step.add(&format!(
                "IFCCOLUMN({},$,'Column',$,$,{place},{shape},$,.COLUMN.)",
                s(&guid(&c.id))
            ));
            build.contained[i].push(column);
            build.counts.columns += 1;
            build.material_for(c.material_id.as_ref(), "Concrete column", column);
        }

        // ------------------------------------------------------ stairs
        for st in &ls.stairs {
            let Some(profile) = build.step.profile(&stair_box(st)) else {
                continue;
            };
            let pos = build.step.axis_at(0.0, 0.0, 0.0);
            let rise = ls.level.height_mm.max(1.0);
            let solid = build.step.extruded(profile, pos, rise);
            let body = build.step.shape(body_ctx, "Body", "SweptSolid", &[solid]);
            let shape = build.step.product_shape(&[body]);
            let flight_place = {
                let a = build.step.axis_at(0.0, 0.0, 0.0);
                build.step.placement(Some(place_parent), a)
            };
            let risers = st.riser_count.max(1) as i64;
            let flight = build.step.add(&format!(
                "IFCSTAIRFLIGHT({},$,'Flight',$,$,{flight_place},{shape},$,{risers},{},{},{},.STRAIGHT.)",
                s(&guid_for("flight", &st.id)),
                risers - 1,
                r(rise / risers as f64),
                r(st.run_mm.max(1.0) / risers as f64)
            ));
            let stair_place = {
                let a = build.step.axis_at(0.0, 0.0, 0.0);
                build.step.placement(Some(place_parent), a)
            };
            let stair = build.step.add(&format!(
                "IFCSTAIR({},$,'Stair',$,$,{stair_place},$,$,.STRAIGHT_RUN_STAIR.)",
                s(&guid(&st.id))
            ));
            build.step.add(&format!(
                "IFCRELAGGREGATES({},$,$,$,{stair},({flight}))",
                s(&guid_for("stair-parts", &st.id))
            ));
            build.contained[i].push(stair);
            build.counts.stairs += 1;
            build.pset(
                "Guhit_Pset_Stair",
                &st.id,
                &[
                    ("NumberOfRiser", Val::Count(risers)),
                    ("NumberOfTreads", Val::Count(risers - 1)),
                    ("RiserHeight", Val::Length(rise / risers as f64)),
                    ("TreadLength", Val::Length(st.run_mm.max(1.0) / risers as f64)),
                ],
                &[flight],
            );
        }

        // ------------------------------------------------------ assets
        for a in &ls.assets {
            let poly = rect(
                V::from(a.position),
                a.width_mm.max(1.0),
                a.depth_mm.max(1.0),
                a.rotation_deg,
            );
            let Some(profile) = build.step.profile(&poly) else {
                continue;
            };
            let pos = build.step.axis_at(0.0, 0.0, a.elevation_mm);
            let solid = build.step.extruded(profile, pos, a.height_mm.max(1.0));
            let body = build.step.shape(body_ctx, "Body", "SweptSolid", &[solid]);
            let shape = build.step.product_shape(&[body]);
            let place = {
                let ax = build.step.axis_at(0.0, 0.0, 0.0);
                build.step.placement(Some(place_parent), ax)
            };
            let furn = build.step.add(&format!(
                "IFCFURNISHINGELEMENT({},$,{},$,$,{place},{shape},{})",
                s(&guid(&a.id)),
                s(&a.name),
                s(&a.catalog_key)
            ));
            build.contained[i].push(furn);
            build.counts.furnishings += 1;
        }

        // ------------------------------------------------- annotations
        for an in &ls.annotations {
            let loc = build.step.axis_at(an.position.x, an.position.y, 0.0);
            let text = an.text.replace(['\n', '\r'], " ");
            let literal = build
                .step
                .add(&format!("IFCTEXTLITERAL({},{loc},.LEFT.)", s(&text)));
            let rep = build
                .step
                .shape(anno_ctx, "Annotation", "Annotation2D", &[literal]);
            let shape = build.step.product_shape(&[rep]);
            let place = {
                let ax = build.step.axis_at(0.0, 0.0, 0.0);
                build.step.placement(Some(place_parent), ax)
            };
            let anno = build.step.add(&format!(
                "IFCANNOTATION({},$,'Note',$,'Text',{place},{shape})",
                s(&guid(&an.id))
            ));
            build.contained[i].push(anno);
            build.counts.annotations += 1;
            build.pset(
                "Guhit_Pset_Annotation",
                &an.id,
                &[
                    ("Text", Val::Text(text)),
                    ("TextHeight", Val::Length(an.size_mm)),
                ],
                &[anno],
            );
        }
    }

    // --------------------------------------------------------------- roof
    if let Some(top) = scene.roof_level {
        let place_parent = storey_places[top];
        let mut planes: Vec<Ref> = Vec::new();
        for (n, plane) in scene.roof.iter().enumerate() {
            let profile_poly: Vec<V> = plane.plan.iter().map(|p| plane.profile(*p)).collect();
            let Some(profile) = build.step.profile(&profile_poly) else {
                continue;
            };
            // Local frame of the plane: x along the ridge, y up the slope,
            // z the upward normal. `along` is `up_slope` turned clockwise, so
            // the frame is right handed and z points up.
            let x3 = (plane.along.x, plane.along.y, 0.0);
            let sin = (1.0 - plane.cos * plane.cos).max(0.0).sqrt();
            let z3 = (plane.along.y * sin, -plane.along.x * sin, plane.cos);
            let pos = build
                .step
                .axis_rotated((plane.origin.x, plane.origin.y, plane.base_h), z3, x3);
            // Sweep straight up: global +z is (0, sin, cos) in this frame, and
            // a vertical thickness of t / cos gives a perpendicular t.
            let solid = build.step.extruded_dir(
                profile,
                pos,
                (0.0, sin, plane.cos),
                plane.thickness / plane.cos.max(1e-6),
            );
            let body = build.step.shape(body_ctx, "Body", "SweptSolid", &[solid]);
            let shape = build.step.product_shape(&[body]);
            let place = {
                let a = build.step.axis_at(0.0, 0.0, 0.0);
                build.step.placement(Some(place_parent), a)
            };
            let slab = build.step.add(&format!(
                "IFCSLAB({},$,{},$,$,{place},{shape},$,.ROOF.)",
                s(&guid_for(&format!("roof-plane-{n}"), &project.id)),
                s(&format!("Roof plane {}", n + 1))
            ));
            planes.push(slab);
            build.counts.slabs += 1;
            build.material_for(project.roof.material_id.as_ref(), "Roofing", slab);
        }
        if !planes.is_empty() {
            match project.roof.kind {
                RoofKind::Flat | RoofKind::None => {
                    // A flat roof is a single slab, contained directly.
                    for p in &planes {
                        build.contained[top].push(*p);
                    }
                }
                RoofKind::Shed | RoofKind::Gable => {
                    let place = {
                        let a = build.step.axis_at(0.0, 0.0, 0.0);
                        build.step.placement(Some(place_parent), a)
                    };
                    let kind = if project.roof.kind == RoofKind::Gable {
                        ".GABLE_ROOF."
                    } else {
                        ".SHED_ROOF."
                    };
                    let roof = build.step.add(&format!(
                        "IFCROOF({},$,'Roof',$,$,{place},$,$,{kind})",
                        s(&guid_for("roof", &project.id))
                    ));
                    build.step.add(&format!(
                        "IFCRELAGGREGATES({},$,$,$,{roof},{})",
                        s(&guid_for("roof-parts", &project.id)),
                        list(&planes)
                    ));
                    build.contained[top].push(roof);
                    build.counts.roofs += 1;
                }
            }
        }
    }

    // ------------------------------------------------------ relationships
    build.step.add(&format!(
        "IFCRELAGGREGATES({},$,$,$,{ifc_project},({site}))",
        s(&guid_for("agg-project", &project.id))
    ));
    build.step.add(&format!(
        "IFCRELAGGREGATES({},$,$,$,{site},({building}))",
        s(&guid_for("agg-site", &project.id))
    ));
    build.step.add(&format!(
        "IFCRELAGGREGATES({},$,$,$,{building},{})",
        s(&guid_for("agg-building", &project.id)),
        list(&storeys)
    ));
    for (i, ls) in scene.levels.iter().enumerate() {
        if !build.contained[i].is_empty() {
            let elements = list(&build.contained[i]);
            let storey = storeys[i];
            build.step.add(&format!(
                "IFCRELCONTAINEDINSPATIALSTRUCTURE({},$,$,$,{elements},{storey})",
                s(&guid_for("contains", &ls.level.id))
            ));
        }
        if !spaces_per_storey[i].is_empty() {
            let spaces = list(&spaces_per_storey[i]);
            let storey = storeys[i];
            build.step.add(&format!(
                "IFCRELAGGREGATES({},$,$,$,{storey},{spaces})",
                s(&guid_for("agg-spaces", &ls.level.id))
            ));
        }
    }

    // ---------------------------------------------------------- materials
    let materials = std::mem::take(&mut build.materials);
    for (name, elements) in materials {
        if elements.is_empty() {
            continue;
        }
        let m = build.step.add(&format!("IFCMATERIAL({},$,$)", s(&name)));
        build.step.add(&format!(
            "IFCRELASSOCIATESMATERIAL({},$,$,$,{},{m})",
            s(&guid_for("material", &format!("{}:{name}", project.id))),
            list(&elements)
        ));
    }

    // -------------------------------------------------------------- file
    let mut out = String::with_capacity(build.step.body.len() + 1024);
    out.push_str("ISO-10303-21;\n");
    out.push_str("HEADER;\n");
    out.push_str(
        "FILE_DESCRIPTION(('ViewDefinition [ReferenceView_V1.2]'),'2;1');\n",
    );
    let _ = writeln!(
        out,
        "FILE_NAME({},{},({}),({}),{},{},'');",
        s(&format!("{}.ifc", project.name)),
        s(&header_time(project)),
        s(&author_name(project)),
        s("Guhit Studio"),
        s("Guhit Studio"),
        s("Guhit Studio")
    );
    out.push_str("FILE_SCHEMA(('IFC4'));\n");
    out.push_str("ENDSEC;\n");
    out.push_str("DATA;\n");
    out.push_str(&build.step.body);
    out.push_str("ENDSEC;\n");
    out.push_str("END-ISO-10303-21;\n");
    Ok((out, build.counts))
}

fn author_name(project: &Project) -> String {
    let d = project.settings.designer.trim();
    if d.is_empty() {
        "Guhit Studio".to_string()
    } else {
        d.to_string()
    }
}

/// Counts the elements in a finished file, by entity keyword.
pub fn count_entities(ifc: &str) -> Counts {
    let n = |kw: &str| {
        ifc.lines()
            .filter(|l| {
                l.split_once('=')
                    .map(|(_, rest)| rest.starts_with(kw) && rest[kw.len()..].starts_with('('))
                    .unwrap_or(false)
            })
            .count()
    };
    Counts {
        walls: n("IFCWALL"),
        openings: n("IFCOPENINGELEMENT"),
        doors: n("IFCDOOR"),
        windows: n("IFCWINDOW"),
        spaces: n("IFCSPACE"),
        slabs: n("IFCSLAB"),
        roofs: n("IFCROOF"),
        columns: n("IFCCOLUMN"),
        stairs: n("IFCSTAIR"),
        furnishings: n("IFCFURNISHINGELEMENT"),
        annotations: n("IFCANNOTATION"),
    }
}

/// Every `#N` reference in the file that no entity defines. Empty means the
/// file is internally consistent.
pub fn dangling_refs(ifc: &str) -> Vec<usize> {
    let mut defined = std::collections::HashSet::new();
    for line in ifc.lines() {
        if let Some(rest) = line.strip_prefix('#') {
            if let Some((num, _)) = rest.split_once('=') {
                if let Ok(n) = num.trim().parse::<usize>() {
                    defined.insert(n);
                }
            }
        }
    }
    let mut missing = Vec::new();
    for line in ifc.lines() {
        let body = match line.split_once('=') {
            Some((_, rest)) => rest,
            None => continue,
        };
        let bytes = body.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'#' {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end].is_ascii_digit() {
                    end += 1;
                }
                if end > start {
                    let n: usize = body[start..end].parse().unwrap_or(0);
                    if !defined.contains(&n) {
                        missing.push(n);
                    }
                }
                i = end;
            } else {
                i += 1;
            }
        }
    }
    missing.sort_unstable();
    missing.dedup();
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guid_encoding_is_22_chars_and_stable() {
        let g = guid("00000000-0000-4000-8000-0000000000a1");
        assert_eq!(g.len(), 22);
        assert_eq!(g, guid("00000000-0000-4000-8000-0000000000a1"));
        assert!(g.chars().all(|c| B64.contains(&(c as u8))));
        // All zero bytes encode to 22 zeros.
        assert_eq!(encode_guid([0u8; 16]), "0".repeat(22));
        // The top two bits only, so the first character is 0 to 3.
        let mut top = [0u8; 16];
        top[0] = 0xff;
        assert_eq!(encode_guid(top).chars().next(), Some('3'));
    }

    #[test]
    fn non_uuid_ids_still_get_stable_guids() {
        let a = guid("w-front");
        let b = guid("w-front");
        let c = guid("w-rear");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 22);
    }

    #[test]
    fn step_strings_and_reals() {
        assert_eq!(r(8000.0), "8000.");
        assert_eq!(r(-75.5), "-75.5");
        assert_eq!(r(0.0), "0.");
        assert_eq!(s("Living"), "'Living'");
        assert_eq!(s("T&B's"), "'T&B''s'");
        assert_eq!(s("Ba\u{f1}o"), "'Ba\\X2\\00F1\\X0\\o'");
    }
}
