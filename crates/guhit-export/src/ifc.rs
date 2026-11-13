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
//! - A site wall (a lot fence or boundary wall, `WallRole::Site`) is an
//!   IfcWall with PredefinedType .USERDEFINED. and ObjectType 'BOUNDARYWALL',
//!   named "Site wall N", external and not load bearing. IFC4 has no
//!   boundary wall type of its own.
//! - A pitched roof is one IfcRoof aggregating one sloped IfcSlab per plane.
//!   Its gable ends are walls when `Derived::roofs` gives them (below),
//!   IfcWall .USERDEFINED. with ObjectType 'GABLEWALL', named "Gable wall
//!   N"; the Wave 1 planes have none, and the walls keep their own height.
//! - Openings are boxes, so an arched or shaped head is squared off. The void
//!   is 10 mm deeper than the wall on each face so it cuts clean everywhere.
//! - A door or window is a single thin box, not a frame with leaves and
//!   glazing. It exists so the hole reads as an opening in a viewer.
//! - Without `Derived::stairs`, a stair is one IfcStairFlight box the full
//!   level height, with the riser and tread numbers as properties.
//!   Individual treads are not modelled. A flight of n risers has n - 1
//!   treads of `run_mm / (n - 1)`, as the inspector shows them.
//! - Dimension elements are skipped: IFC has no plain 2D dimension in this
//!   context and a wrong one is worse than none.
//! - Linework, ReferenceModel, Underlay and Camera elements are skipped. They
//!   are tracing and viewing aids, not building elements.
//! - A pipe run is one IfcPipeSegment (.RIGIDSEGMENT.) per straight segment:
//!   a circle of the pipe size swept along the segment, contained in the
//!   storey of its level and assigned to one IfcDistributionSystem per
//!   system, which serves the building. Bends are where two segments meet;
//!   elbows and tees are not written as IfcPipeFitting. Sizes are as drawn:
//!   Guhit coordinates pipes, it does not size them.
//! - Floor, balcony and deck slabs (`Slab`) are IfcSlab, `.FLOOR.` or
//!   `.USERDEFINED.` with ObjectType 'BALCONY' or 'DECK', extruded from the
//!   outline down from their top. Every floor slab in the file, the one per
//!   footprint included, is named "Floor slab N" with N counted over the
//!   whole file, so no two share a name.
//! - A void is an IfcOpeningElement through each floor of its level it
//!   overlaps, one per floor because an opening voids exactly one element in
//!   IFC4. A void over no floor the file holds (over the roof deck of the
//!   level below, which is not written as a slab) is left out.
//! - Each `Derived::railings` run is one IfcRailing, `.GUARDRAIL.` for slab
//!   edges and stair wells, `.HANDRAIL.` for stair sides, its solids from
//!   `model3d::railing_pieces`.
//!
//! What a firm's Revit or ArchiCAD expects on top (Wave 2, lane L8):
//! - Base quantities (`qto`): Qto_WallBaseQuantities, Qto_SlabBaseQuantities
//!   (floors, balconies, decks, landings, roof planes),
//!   Qto_SpaceBaseQuantities, Qto_DoorBaseQuantities,
//!   Qto_WindowBaseQuantities, Qto_RoofBaseQuantities and
//!   Qto_StairFlightBaseQuantities.
//! - Types (`types`): an IfcDoorType or IfcWindowType per door and window
//!   type with its mark as name and tag, untyped openings grouped by kind,
//!   style and size, and an IfcWallType per thickness and material. A typed
//!   opening carries its mark as Tag and as Pset_*Common Reference.
//! - Stairs from `Derived::stairs` when the engine has them: one
//!   IfcStairFlight per flight, its treads as steps, and IfcSlab .LANDING.
//!   landings, aggregated by an IfcStair of the stair's shape. Without it,
//!   the one-box flight above.
//! - Roofs from `Derived::roofs` when the engine has them: an IfcRoof over
//!   one sloped IfcSlab per plane (the top surface swept straight down by
//!   the roof's vertical thickness), and the gable-end infill as walls.
//! - One IfcGrid from the grid lines, the IfcSite's latitude and longitude
//!   from the project's site and its land title number from the title
//!   block's TCT, and an IfcSanitaryTerminal per WC, lavatory, shower,
//!   bathtub and sink.

use std::collections::HashMap;
use std::fmt::Write as _;

use guhit_model::{
    ColumnShape, Derived, Element, GridLine, Opening, OpeningStyle, OpeningType, Project,
    RailingRunKind, RailingStyle, RoofKind, RoomUsage, SlabKind,
};

use crate::geom::*;
use crate::model3d::*;
use crate::pipes;
use crate::ExportError;

mod extra;
mod qto;
mod types;

use qto::{m2, m3, perimeter, Q};

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
    PositiveLength(f64),
    Count(i64),
    Real(f64),
}

impl Val {
    fn step(&self) -> String {
        match self {
            Val::Bool(b) => format!("IFCBOOLEAN(.{}.)", if *b { "T" } else { "F" }),
            Val::Text(t) => format!("IFCLABEL({})", s(t)),
            Val::Area(a) => format!("IFCAREAMEASURE({})", r(*a)),
            Val::Length(l) => format!("IFCLENGTHMEASURE({})", r(*l)),
            Val::PositiveLength(l) => format!("IFCPOSITIVELENGTHMEASURE({})", r(*l)),
            Val::Count(c) => format!("IFCCOUNTMEASURE({c})"),
            Val::Real(x) => format!("IFCREAL({})", r(*x)),
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
    pub pipe_segments: usize,
    pub pipe_systems: usize,
    pub outlets: usize,
    pub switches: usize,
    pub light_fixtures: usize,
    pub boards: usize,
    pub sensors: usize,
    pub alarms: usize,
    pub unitary: usize,
    pub cable_segments: usize,
    pub railings: usize,
    pub sanitary: usize,
    pub grids: usize,
    pub door_types: usize,
    pub window_types: usize,
    pub wall_types: usize,
    /// IfcElementQuantity sets.
    pub quantity_sets: usize,
    pub stair_flights: usize,
}

impl<'a> Build<'a> {
    /// The name of a material id, or `fallback`.
    fn material_name(&self, id: Option<&String>, fallback: &str) -> String {
        id.and_then(|m| self.project.materials.iter().find(|x| &x.id == m))
            .map(|m| m.name.clone())
            .unwrap_or_else(|| fallback.to_string())
    }

    fn material_named(&mut self, name: &str, elements: &[Ref]) {
        match self.materials.iter_mut().find(|(n, _)| n == name) {
            Some((_, list)) => list.extend_from_slice(elements),
            None => self.materials.push((name.to_string(), elements.to_vec())),
        }
    }

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
            || !l.pipes.is_empty()
            || !l.slabs.is_empty()
            || !l.railings.is_empty()
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
    // Latitude and longitude when the project says where it stands; the
    // land title (TCT) number when the title block has one.
    let (lat, lon) = match &project.settings.site {
        Some(g) if g.latitude_deg.is_finite() && g.longitude_deg.is_finite() => {
            (extra::compound_angle(g.latitude_deg), extra::compound_angle(g.longitude_deg))
        }
        _ => ("$".to_string(), "$".to_string()),
    };
    let tct = crate::text::clean(&project.title_block.tct_no);
    let tct = if tct.is_empty() { "$".to_string() } else { s(&tct) };
    let site = step.add(&format!(
        "IFCSITE({},$,{},$,$,{site_place},$,$,.ELEMENT.,{lat},{lon},$,{tct},$)",
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
    // Pipe segments per system, for the distribution systems.
    let mut system_members: Vec<(guhit_model::PipeSystem, Vec<Ref>)> = Vec::new();
    // Names are numbered over the whole file, never per level.
    let mut names = Names::default();
    // Rails of each host so far: a stair has up to two sides and a well.
    let mut rails_of: HashMap<&str, usize> = HashMap::new();
    let mut types = types::Types::default();
    // Each opening's mark from the schedule (DECISIONS D53).
    let marks: HashMap<&str, &str> = derived
        .opening_schedule
        .marks
        .iter()
        .map(|m| (m.opening_id.as_str(), m.mark.as_str()))
        .collect();

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
            let site = w.wall.is_site();
            let name = if site { format!("Site wall {}", n + 1) } else { format!("Wall {}", n + 1) };
            let (object_type, predefined) = if site {
                (s("BOUNDARYWALL"), ".USERDEFINED.")
            } else {
                ("$".to_string(), ".SOLIDWALL.")
            };
            let wall = build.step.add(&format!(
                "IFCWALL({},$,{},$,{object_type},{place},{shape},$,{predefined})",
                s(&guid(&w.wall.id)),
                s(&name)
            ));
            build.contained[i].push(wall);
            wall_refs.insert(w.wall.id.as_str(), wall);
            build.counts.walls += 1;
            build.material_for(w.wall.material_id.as_ref(), "Wall", wall);
            let material = build.material_name(w.wall.material_id.as_ref(), "Wall");
            types.add_wall(w.wall, &material, wall);
            {
                // Openings come off the sides and the volume, at the size
                // their voids cut.
                let t = w.wall.thickness_mm.abs();
                let length = (V::from(w.wall.end) - V::from(w.wall.start)).len();
                let cut: f64 = w
                    .openings
                    .iter()
                    .map(|o| {
                        let sill = o.sill_mm.max(0.0);
                        let h = o.height_mm.max(1.0).min((w.height - sill).max(1.0));
                        m2(o.width_mm.max(0.0) * h)
                    })
                    .sum();
                let foot = m2(polygon_area(&w.plan_outline));
                let side = m2(length * w.height);
                build.qto(
                    "Qto_WallBaseQuantities",
                    &w.wall.id,
                    &[
                        ("Length", Q::Length(length)),
                        ("Width", Q::Length(t)),
                        ("Height", Q::Length(w.height)),
                        ("GrossFootprintArea", Q::Area(foot)),
                        ("NetFootprintArea", Q::Area(foot)),
                        ("GrossSideArea", Q::Area(side)),
                        ("NetSideArea", Q::Area(side - cut)),
                        ("GrossVolume", Q::Volume(m3(foot, w.height))),
                        ("NetVolume", Q::Volume(m3(foot, w.height) - m3(cut, t))),
                    ],
                    &[wall],
                );
            }
            build.pset(
                "Pset_WallCommon",
                &w.wall.id,
                &[
                    ("Reference", Val::Text(name.clone())),
                    // A fence stands outside the building whatever the
                    // topology says.
                    ("IsExternal", Val::Bool(w.exterior || site)),
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
                // A typed opening is named and tagged with its mark.
                let mark: Option<String> = marks
                    .get(o.id.as_str())
                    .map(|m| m.to_string())
                    .or_else(|| o.type_id.as_ref().and_then(|t| project.opening_type(t)).map(|t| t.mark.clone()))
                    .map(|m| crate::text::clean(&m))
                    .filter(|m| !m.is_empty());
                let tag = mark.as_deref().map(s).unwrap_or_else(|| "$".into());
                let label = |kind: &str| match &mark {
                    Some(m) => format!("{kind} {m}"),
                    None => format!("{kind} {}", style_label(o.style)),
                };
                let filler = match o.opening_type {
                    OpeningType::Door => build.step.add(&format!(
                        "IFCDOOR({},$,{},$,$,{f_place},{panel_shape},{tag},{},{},.DOOR.,{},$)",
                        s(&guid(&o.id)),
                        s(&label("Door")),
                        r(height),
                        r(o.width_mm),
                        door_operation(o)
                    )),
                    OpeningType::Window => build.step.add(&format!(
                        "IFCWINDOW({},$,{},$,$,{f_place},{panel_shape},{tag},{},{},.WINDOW.,{},$)",
                        s(&guid(&o.id)),
                        s(&label("Window")),
                        r(height),
                        r(o.width_mm),
                        window_partitioning(o.style)
                    )),
                };
                types.add_opening(project, o, mark.as_deref(), filler);
                build.opening_qto(&o.id, o.opening_type == OpeningType::Door, o.width_mm, height, filler);
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
                        ("Reference", Val::Text(mark.clone().unwrap_or_else(|| style_label(o.style).to_string()))),
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
        // The floors of this level a void may cut.
        let mut floors: Vec<Floor> = Vec::new();
        // Each void's clear part: it never takes the slab under the walls
        // (GA P0-1), so the slab's net area is the schedule's gross area.
        let clear: Vec<Vec<Vec<V>>> = ls.voids.iter().map(|vd| crate::model3d::void_clear(&slab_outline(vd), &ls.walls)).collect();
        // The area of this level's voids over an outline, m².
        let void_cut = |outline: &[V]| -> f64 { clear.iter().flatten().map(|piece| m2(overlap_area(piece, outline))).sum() };
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
            let name = numbered(&mut names.floors, "Floor slab");
            let slab = build.step.add(&format!(
                "IFCSLAB({},$,{},$,$,{place},{shape},$,.FLOOR.)",
                s(&guid_for(&format!("slab-{n}"), &ls.level.id)),
                s(&name)
            ));
            build.contained[i].push(slab);
            build.counts.slabs += 1;
            build.material_for(None, "Concrete slab", slab);
            build.slab_qto(&format!("{}:slab-{n}", ls.level.id), fp, SLAB_THICKNESS_MM, void_cut(fp), slab);
            floors.push(Floor {
                key: format!("slab-{n}"),
                slab,
                bottom: -SLAB_THICKNESS_MM,
                top: 0.0,
                outline: fp.clone(),
                name,
            });
        }

        // Floor, balcony and deck slabs drawn as elements.
        for sl in &ls.slabs {
            let outline = slab_outline(sl);
            let (bottom, top) = slab_heights(sl);
            let Some(profile) = build.step.profile(&outline) else {
                continue;
            };
            let pos = build.step.axis_at(0.0, 0.0, bottom);
            let solid = build.step.extruded(profile, pos, top - bottom);
            let body = build.step.shape(body_ctx, "Body", "SweptSolid", &[solid]);
            let shape = build.step.product_shape(&[body]);
            let place = {
                let a = build.step.axis_at(0.0, 0.0, 0.0);
                build.step.placement(Some(place_parent), a)
            };
            let (name, object_type, predefined, external) = match sl.kind {
                SlabKind::Balcony => (
                    numbered(&mut names.balconies, "Balcony"),
                    s("BALCONY"),
                    ".USERDEFINED.",
                    true,
                ),
                SlabKind::Deck => (numbered(&mut names.decks, "Deck"), s("DECK"), ".USERDEFINED.", true),
                SlabKind::Floor | SlabKind::Void => {
                    (numbered(&mut names.floors, "Floor slab"), "$".to_string(), ".FLOOR.", false)
                }
            };
            let slab = build.step.add(&format!(
                "IFCSLAB({},$,{},$,{object_type},{place},{shape},$,{predefined})",
                s(&guid(&sl.id)),
                s(&name)
            ));
            build.contained[i].push(slab);
            build.counts.slabs += 1;
            build.material_for(sl.material_id.as_ref(), "Concrete slab", slab);
            build.pset(
                "Pset_SlabCommon",
                &sl.id,
                &[
                    ("Reference", Val::Text(name.clone())),
                    ("IsExternal", Val::Bool(external)),
                ],
                &[slab],
            );
            let cut = if sl.kind == SlabKind::Floor { void_cut(&outline) } else { 0.0 };
            build.slab_qto(&sl.id, &outline, top - bottom, cut, slab);
            floors.push(Floor {
                key: sl.id.clone(),
                slab,
                bottom,
                top,
                outline,
                name,
            });
        }

        // Voids: one opening through each floor they overlap, 10 mm past
        // both faces so it cuts clean.
        for (vd, pieces) in ls.voids.iter().zip(&clear) {
            let cut: Vec<&Floor> = floors
                .iter()
                .filter(|f| pieces.iter().any(|piece| overlap_area(piece, &f.outline) > 1.0))
                .collect();
            if cut.is_empty() {
                continue;
            }
            let void_name = numbered(&mut names.voids, "Void");
            for f in cut {
                // One extrusion per clear piece: the walls keep their slab.
                let pos = build.step.axis_at(0.0, 0.0, f.bottom - 10.0);
                let mut solids: Vec<Ref> = Vec::new();
                for piece in pieces {
                    if let Some(profile) = build.step.profile(piece) {
                        solids.push(build.step.extruded(profile, pos, f.top - f.bottom + 20.0));
                    }
                }
                if solids.is_empty() {
                    break;
                }
                let body = build.step.shape(body_ctx, "Body", "SweptSolid", &solids);
                let shape = build.step.product_shape(&[body]);
                let place = {
                    let a = build.step.axis_at(0.0, 0.0, 0.0);
                    build.step.placement(Some(place_parent), a)
                };
                let opening = build.step.add(&format!(
                    "IFCOPENINGELEMENT({},$,{},$,$,{place},{shape},$,.OPENING.)",
                    s(&guid_for(&format!("void-of-{}", f.key), &vd.id)),
                    s(&format!("{void_name} in {}", f.name))
                ));
                build.step.add(&format!(
                    "IFCRELVOIDSELEMENT({},$,$,$,{},{opening})",
                    s(&guid_for(&format!("voids-{}", f.key), &vd.id)),
                    f.slab
                ));
                build.counts.openings += 1;
            }
        }

        // ---------------------------------------------------- railings
        for run in &ls.railings {
            let mut solids: Vec<Ref> = Vec::new();
            for p in railing_pieces(run) {
                let Some(profile) = build.step.profile(&p.profile) else {
                    continue;
                };
                // x along the piece, z across it to the right, so y is up
                // and the profile (u, w) stands upright.
                let (x, z) = (p.along, p.across());
                let o = p.origin - z * (p.thickness / 2.0);
                let pos = build
                    .step
                    .axis_rotated((o.x, o.y, 0.0), (z.x, z.y, 0.0), (x.x, x.y, 0.0));
                solids.push(build.step.extruded(profile, pos, p.thickness));
            }
            if solids.is_empty() {
                continue;
            }
            let body = build.step.shape(body_ctx, "Body", "SweptSolid", &solids);
            let shape = build.step.product_shape(&[body]);
            let place = {
                let a = build.step.axis_at(0.0, 0.0, 0.0);
                build.step.placement(Some(place_parent), a)
            };
            let (name, predefined) = match run.kind {
                RailingRunKind::StairSide => (numbered(&mut names.handrails, "Handrail"), ".HANDRAIL."),
                RailingRunKind::SlabEdge | RailingRunKind::StairWell => {
                    (numbered(&mut names.guards, "Guard rail"), ".GUARDRAIL.")
                }
            };
            let k = rails_of.entry(run.host_id.as_str()).or_insert(0);
            let owner = format!("{}:{k}", run.host_id);
            *k += 1;
            let rail = build.step.add(&format!(
                "IFCRAILING({},$,{},$,$,{place},{shape},$,{predefined})",
                s(&guid_for("railing", &owner)),
                s(&name)
            ));
            build.contained[i].push(rail);
            build.counts.railings += 1;
            let (style, material) = match run.style {
                RailingStyle::Bars => ("bars", "Steel railing"),
                RailingStyle::Glass => ("glass", "Glass railing"),
                RailingStyle::Solid => ("solid", "Concrete parapet"),
            };
            build.material_named(material, &[rail]);
            build.pset(
                "Pset_RailingCommon",
                &owner,
                &[
                    ("Reference", Val::Text(name.clone())),
                    ("Height", Val::PositiveLength(run.height_mm.max(1.0))),
                ],
                &[rail],
            );
            build.pset(
                "Guhit_Pset_Railing",
                &owner,
                &[
                    ("Style", Val::Text(style.to_string())),
                    ("HostId", Val::Text(run.host_id.clone())),
                ],
                &[rail],
            );
        }

        // ------------------------------------------------------ spaces
        for rm in &ls.rooms {
            let Some(profile) = build.step.profile(&rm.polygon) else {
                continue;
            };
            // The schedule's row: its floor area (as Totals count it) and
            // its ceiling.
            let row = derived.room_schedule.iter().find(|r| r.room_id == rm.room.id);
            // The ceiling: the room's own, else the schedule's, else the
            // level height. A room under a void reaches the ceiling over it,
            // so the space is as tall as its ceiling when that is higher.
            let ceiling = rm
                .room
                .ceiling_height_mm
                .or_else(|| row.map(|r| r.ceiling_height_mm))
                .filter(|h| h.is_finite() && *h > 0.0)
                .unwrap_or(ls.level.height_mm.max(1.0));
            let height = ls.level.height_mm.max(1.0).max(ceiling);
            let pos = build.step.axis_at(0.0, 0.0, 0.0);
            let solid = build.step.extruded(profile, pos, height);
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
            // Gross: inside the walls less voids; net: the floor area the
            // schedule and Totals count, also less balconies and decks.
            let area_m2 = rm.area_mm2 / 1_000_000.0;
            let floor_m2 = row.map(|r| r.floor_area_m2).filter(|a| a.is_finite()).unwrap_or(area_m2);
            build.pset(
                "Pset_SpaceCommon",
                &rm.room.id,
                &[
                    ("Reference", Val::Text(usage_label(rm.room.usage).to_string())),
                    ("IsExternal", Val::Bool(false)),
                    ("GrossPlannedArea", Val::Area(area_m2)),
                    ("NetPlannedArea", Val::Area(floor_m2)),
                ],
                &[space],
            );
            build.qto(
                "Qto_SpaceBaseQuantities",
                &rm.room.id,
                &[
                    ("Height", Q::Length(height)),
                    ("FinishCeilingHeight", Q::Length(ceiling)),
                    ("GrossFloorArea", Q::Area(area_m2)),
                    ("NetFloorArea", Q::Area(floor_m2)),
                    ("GrossPerimeter", Q::Length(perimeter(&rm.polygon))),
                    ("GrossVolume", Q::Volume(m3(area_m2, ceiling))),
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
            // Up to the roof over its center (a carport or lanai post).
            let solid = build
                .step
                .extruded(profile, pos, crate::model3d::column_height(derived, ls.level, c));
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
            // The engine's flights and landings when it has them.
            if let Some(geo) = derived.stairs.iter().find(|g| g.stair_id == st.id && !g.flights.is_empty()) {
                let parts = build.derived_stair(st, geo, place_parent, body_ctx, &mut names);
                if parts.is_empty() {
                    continue;
                }
                let stair_place = {
                    let a = build.step.axis_at(0.0, 0.0, 0.0);
                    build.step.placement(Some(place_parent), a)
                };
                let stair = build.step.add(&format!(
                    "IFCSTAIR({},$,'Stair',$,$,{stair_place},$,$,{})",
                    s(&guid(&st.id)),
                    extra::stair_kind(st.shape)
                ));
                build.step.add(&format!(
                    "IFCRELAGGREGATES({},$,$,$,{stair},{})",
                    s(&guid_for("stair-parts", &st.id)),
                    list(&parts)
                ));
                build.contained[i].push(stair);
                build.counts.stairs += 1;
                let risers = st.riser_count.max(1) as i64;
                let treads: i64 = geo.flights.iter().map(|f| f.nosings.len() as i64).sum();
                build.pset(
                    "Guhit_Pset_Stair",
                    &st.id,
                    &[
                        ("NumberOfRiser", Val::Count(risers)),
                        ("NumberOfTreads", Val::Count(treads)),
                        ("RiserHeight", Val::Length(geo.riser_mm)),
                        ("TreadLength", Val::Length(geo.tread_mm)),
                    ],
                    &[stair],
                );
                continue;
            }
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
            // A straight flight of n risers has n - 1 treads between them,
            // and the run is their total going: the tread the inspector
            // shows, so treads times tread length is the run.
            let treads = (risers - 1).max(1);
            let tread = st.run_mm.max(1.0) / treads as f64;
            let flight = build.step.add(&format!(
                "IFCSTAIRFLIGHT({},$,'Flight',$,$,{flight_place},{shape},$,{risers},{},{},{},.STRAIGHT.)",
                s(&guid_for("flight", &st.id)),
                risers - 1,
                r(rise / risers as f64),
                r(tread)
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
                    ("TreadLength", Val::Length(tread)),
                ],
                &[flight],
            );
            build.counts.stair_flights += 1;
            let volume = m3(m2(polygon_area(&stair_box(st))), rise);
            build.qto(
                "Qto_StairFlightBaseQuantities",
                &st.id,
                &[
                    ("Length", Q::Length(tread * treads as f64)),
                    ("GrossVolume", Q::Volume(volume)),
                    ("NetVolume", Q::Volume(volume)),
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
            // Plumbing fixtures are sanitary terminals.
            if let Some(kind) = extra::sanitary_kind(a) {
                let fixture = build.step.add(&format!(
                    "IFCSANITARYTERMINAL({},$,{},$,{},{place},{shape},$,{kind})",
                    s(&guid(&a.id)),
                    s(&a.name),
                    s(&a.catalog_key)
                ));
                build.contained[i].push(fixture);
                build.counts.sanitary += 1;
                build.pset(
                    "Guhit_Pset_Object",
                    &a.id,
                    &[("CatalogKey", Val::Text(a.catalog_key.clone()))],
                    &[fixture],
                );
                continue;
            }
            let Some((entity, kind)) = device_entity(a) else {
                let furn = build.step.add(&format!(
                    "IFCFURNISHINGELEMENT({},$,{},$,$,{place},{shape},{})",
                    s(&guid(&a.id)),
                    s(&a.name),
                    s(&a.catalog_key)
                ));
                build.contained[i].push(furn);
                build.counts.furnishings += 1;
                continue;
            };
            // Devices: outlets, switches, fixtures, boards, detectors, bells
            // and aircon units, each a box of its size at its height.
            let device = build.step.add(&format!(
                "{entity}({},$,{},$,$,{place},{shape},{},{kind})",
                s(&guid(&a.id)),
                s(&a.name),
                s(&a.catalog_key)
            ));
            build.contained[i].push(device);
            match entity {
                "IFCOUTLET" => build.counts.outlets += 1,
                "IFCSWITCHINGDEVICE" => build.counts.switches += 1,
                "IFCLIGHTFIXTURE" => build.counts.light_fixtures += 1,
                "IFCELECTRICDISTRIBUTIONBOARD" => build.counts.boards += 1,
                "IFCSENSOR" => build.counts.sensors += 1,
                "IFCALARM" => build.counts.alarms += 1,
                _ => build.counts.unitary += 1,
            }
            let mut props: Vec<(&str, Val)> = vec![
                ("CatalogKey", Val::Text(a.catalog_key.clone())),
                ("Device", Val::Text(crate::services::type_label(a))),
                ("MountingHeight", Val::Length(a.elevation_mm + a.height_mm / 2.0)),
            ];
            let circuit = crate::text::clean(&a.circuit);
            if !circuit.is_empty() {
                props.push(("Circuit", Val::Text(circuit)));
            }
            if let Some(light) = a.light {
                props.push(("Lumens", Val::Real(light.lumens)));
                props.push(("Kelvin", Val::Real(light.kelvin)));
            }
            build.pset("Guhit_Pset_Device", &a.id, &props, &[device]);
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

        // ------------------------------------------------------- pipes
        for p in &ls.pipes {
            let conduit = p.system == guhit_model::PipeSystem::Conduit;
            let segments = if conduit {
                run_segments(&mut build, p, body_ctx, place_parent, "IFCCABLECARRIERSEGMENT", ".CONDUITSEGMENT.", "conduit-segment")
            } else {
                run_segments(&mut build, p, body_ctx, place_parent, "IFCPIPESEGMENT", ".RIGIDSEGMENT.", "pipe-segment")
            };
            if segments.is_empty() {
                continue;
            }
            build.contained[i].extend_from_slice(&segments);
            if conduit {
                build.counts.cable_segments += segments.len();
                build.material_named(pipes::material_label(p.material), &segments);
                match system_members.iter_mut().find(|(s, _)| *s == p.system) {
                    Some((_, list)) => list.extend_from_slice(&segments),
                    None => system_members.push((p.system, segments.clone())),
                }
                build.pset(
                    "Guhit_Pset_Conduit",
                    &p.id,
                    &[
                        ("System", Val::Text(pipes::label(p.system).to_string())),
                        ("Material", Val::Text(pipes::material_label(p.material).to_string())),
                        ("NominalDiameter", Val::PositiveLength(p.diameter_mm)),
                        ("RunId", Val::Text(p.id.clone())),
                        ("SegmentCount", Val::Count(segments.len() as i64)),
                    ],
                    &segments,
                );
                continue;
            }
            build.counts.pipe_segments += segments.len();
            build.material_named(pipes::material_label(p.material), &segments);
            match system_members.iter_mut().find(|(s, _)| *s == p.system) {
                Some((_, list)) => list.extend_from_slice(&segments),
                None => system_members.push((p.system, segments.clone())),
            }
            build.pset(
                "Pset_PipeSegmentTypeCommon",
                &p.id,
                &[("NominalDiameter", Val::PositiveLength(p.diameter_mm))],
                &segments,
            );
            build.pset(
                "Guhit_Pset_Pipe",
                &p.id,
                &[
                    ("System", Val::Text(pipes::label(p.system).to_string())),
                    (
                        "Material",
                        Val::Text(pipes::material_label(p.material).to_string()),
                    ),
                    ("PipeId", Val::Text(p.id.clone())),
                    ("SegmentCount", Val::Count(segments.len() as i64)),
                ],
                &segments,
            );
        }
    }

    // -------------------------------------------------------------- roofs
    // From `Derived::roofs` when the engine has them, else the Wave 1 planes
    // of the scene. The first roof keeps the Wave 1 GUIDs, so a re-export of
    // the same house matches.
    let derived_roofs = derived.roofs.iter().any(|g| !g.planes.is_empty());
    for (ri, g) in derived.roofs.iter().enumerate().filter(|_| derived_roofs) {
        let Some(top) = scene.levels.iter().position(|l| l.level.id == g.level_id) else {
            continue;
        };
        let area = project.elements.iter().find_map(|e| match e {
            Element::Roof(r) if r.id == g.roof_id => Some(r),
            _ => None,
        });
        // A roof element that is gone draws nothing, even when `Derived`
        // still has its planes; an empty id is the legacy project roof.
        if area.is_none() && !g.roof_id.is_empty() {
            continue;
        }
        let kind = area.map(|r| r.shape.kind()).unwrap_or_else(|| project.top_roof().kind);
        let material = area.and_then(|r| r.material_id.clone());
        let tag = |what: &str| if ri == 0 { what.to_string() } else { format!("{what}-{ri}") };
        let owner = if ri == 0 { project.id.clone() } else { g.roof_id.clone() };
        let place_parent = storey_places[top];
        let guid_of = |n: usize| guid_for(&format!("{}-{n}", tag("roof-plane")), &owner);
        let (planes, surface) =
            build.derived_roof_planes(&g.planes, g.vertical_mm, &guid_of, material.as_ref(), place_parent, body_ctx);
        let gables = build.gable_walls(&g.infill, &scene.levels[top].walls, &g.roof_id, place_parent, body_ctx, &mut names);
        build.contained[top].extend(gables);
        let surface = if g.surface_area_m2 > 0.0 { g.surface_area_m2 } else { surface };
        finish_roof(&mut build, kind, &planes, top, place_parent, &tag, &owner, surface, g.plan_area_m2);
    }
    for (ri, roof_solid) in scene.roofs.iter().enumerate().filter(|_| !derived_roofs) {
        let top = roof_solid.level;
        let tag = |what: &str| if ri == 0 { what.to_string() } else { format!("{what}-{ri}") };
        let place_parent = storey_places[top];
        let mut planes: Vec<Ref> = Vec::new();
        let (mut surface, mut projected) = (0.0, 0.0);
        for (n, plane) in roof_solid.planes.iter().enumerate() {
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
            let id = guid_for(&format!("{}-{n}", tag("roof-plane")), &project.id);
            let slab = build.step.add(&format!(
                "IFCSLAB({},$,{},$,$,{place},{shape},$,.ROOF.)",
                s(&id),
                s(&format!("Roof plane {}", n + 1))
            ));
            planes.push(slab);
            build.counts.slabs += 1;
            build.material_for(roof_solid.material_id.as_ref(), "Roofing", slab);
            let plan = m2(polygon_area(&plane.plan));
            let sloped = plan / plane.cos.max(1e-6);
            surface += sloped;
            projected += plan;
            build.qto(
                "Qto_SlabBaseQuantities",
                &id,
                &[
                    ("Width", Q::Length(plane.thickness)),
                    ("Perimeter", Q::Length(perimeter(&profile_poly))),
                    ("GrossArea", Q::Area(sloped)),
                    ("NetArea", Q::Area(sloped)),
                    ("GrossVolume", Q::Volume(m3(sloped, plane.thickness))),
                ],
                &[slab],
            );
        }
        let gables = build.gable_walls(
            &roof_solid.infill,
            &scene.levels[top].walls,
            &format!("{}:{ri}", project.id),
            place_parent,
            body_ctx,
            &mut names,
        );
        build.contained[top].extend(gables);
        finish_roof(&mut build, roof_solid.kind, &planes, top, place_parent, &tag, &project.id, surface, projected);
    }

    // --------------------------------------------------------------- grid
    // One grid for the whole building, on the lowest storey.
    let lines: Vec<&GridLine> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Grid(g) => Some(g),
            _ => None,
        })
        .collect();
    let lowest = (0..scene.levels.len()).min_by(|a, b| {
        scene.levels[*a]
            .level
            .elevation_mm
            .partial_cmp(&scene.levels[*b].level.elevation_mm)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if let Some(low) = lowest {
        if let Some(grid) = build.grid(&lines, &project.id, storey_places[low], anno_ctx) {
            build.contained[low].push(grid);
        }
    }

    // -------------------------------------------------------------- types
    types.write(&mut build, ifc_project, &project.id);

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

    // ------------------------------------------------------- pipe systems
    // One IfcDistributionSystem per system present, in the contract order,
    // holding its segments and serving the building.
    for system in pipes::SYSTEMS {
        let Some((_, members)) = system_members.iter().find(|(s, _)| *s == system) else {
            continue;
        };
        let key = pipes::slug(system);
        let sys = build.step.add(&format!(
            "IFCDISTRIBUTIONSYSTEM({},$,{},{},$,$,{})",
            s(&guid_for(&format!("pipe-system-{key}"), &project.id)),
            s(pipes::label(system)),
            s(system_note(system)),
            pipes::ifc_system(system)
        ));
        build.step.add(&format!(
            "IFCRELASSIGNSTOGROUP({},$,$,$,{},$,{sys})",
            s(&guid_for(
                &format!("pipe-system-members-{key}"),
                &project.id
            )),
            list(members)
        ));
        build.step.add(&format!(
            "IFCRELSERVICESBUILDINGS({},$,$,$,{sys},({building}))",
            s(&guid_for(&format!("pipe-system-serves-{key}"), &project.id))
        ));
        build.counts.pipe_systems += 1;
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

/// Running numbers for element names, over the whole file.
#[derive(Default)]
struct Names {
    floors: usize,
    balconies: usize,
    decks: usize,
    voids: usize,
    guards: usize,
    handrails: usize,
    landings: usize,
    gables: usize,
}

/// The roof over its planes: an IfcRoof of its kind aggregating them, with
/// Qto_RoofBaseQuantities. A flat roof is its one slab, contained directly.
#[allow(clippy::too_many_arguments)]
fn finish_roof(
    build: &mut Build,
    kind: RoofKind,
    planes: &[Ref],
    top: usize,
    place_parent: Ref,
    tag: &dyn Fn(&str) -> String,
    owner: &str,
    surface_m2: f64,
    projected_m2: f64,
) {
    if planes.is_empty() {
        return;
    }
    let predefined = match kind {
        RoofKind::Flat | RoofKind::None => {
            build.contained[top].extend_from_slice(planes);
            return;
        }
        RoofKind::Gable => ".GABLE_ROOF.",
        RoofKind::Hip => ".HIP_ROOF.",
        RoofKind::Shed => ".SHED_ROOF.",
    };
    let place = {
        let a = build.step.axis_at(0.0, 0.0, 0.0);
        build.step.placement(Some(place_parent), a)
    };
    let id = guid_for(&tag("roof"), owner);
    let roof = build.step.add(&format!("IFCROOF({},$,'Roof',$,$,{place},$,$,{predefined})", s(&id)));
    build.step.add(&format!(
        "IFCRELAGGREGATES({},$,$,$,{roof},{})",
        s(&guid_for(&tag("roof-parts"), owner)),
        list(planes)
    ));
    build.contained[top].push(roof);
    build.counts.roofs += 1;
    build.qto(
        "Qto_RoofBaseQuantities",
        &id,
        &[
            ("GrossArea", Q::Area(surface_m2)),
            ("NetArea", Q::Area(surface_m2)),
            ("ProjectedArea", Q::Area(projected_m2)),
        ],
        &[roof],
    );
}

/// The next name of a numbered kind: "Balcony 2".
fn numbered(counter: &mut usize, label: &str) -> String {
    *counter += 1;
    format!("{label} {counter}")
}

/// A floor of one level that a void may cut: the slab entity, its bottom
/// and top above the level floor, its outline and name. `key` makes the ids
/// of the openings through it.
struct Floor {
    key: String,
    slab: Ref,
    bottom: f64,
    top: f64,
    outline: Vec<V>,
    name: String,
}

/// What an IfcDistributionSystem says about itself: sizes as drawn, not a
/// design, for the trade of its system.
fn system_note(system: guhit_model::PipeSystem) -> &'static str {
    use guhit_model::PipeSystem as P;
    match system {
        P::Conduit => "Coordination model from Guhit Studio. Conduit sizes as drawn, not an electrical design.",
        P::Refrigerant | P::Condensate => {
            "Coordination model from Guhit Studio. Line set and drain sizes as drawn, not a mechanical design."
        }
        _ => "Coordination model from Guhit Studio. Pipe sizes as drawn, not a plumbing design.",
    }
}

/// The IFC entity and predefined type of an object that is a device, None
/// for furniture and fixtures (they stay IfcFurnishingElement).
fn device_entity(a: &guhit_model::Asset) -> Option<(&'static str, &'static str)> {
    use crate::services::Sym;
    let sym = crate::services::symbol_of(a)?;
    Some(match sym {
        s if s.is_light() => ("IFCLIGHTFIXTURE", ".POINTSOURCE."),
        Sym::Switch(_) => ("IFCSWITCHINGDEVICE", ".TOGGLESWITCH."),
        Sym::PushButton => ("IFCSWITCHINGDEVICE", ".MOMENTARYSWITCH."),
        Sym::Outlet | Sym::OutdoorOutlet | Sym::Spo | Sym::AirconOutlet => ("IFCOUTLET", ".POWEROUTLET."),
        Sym::Panelboard => ("IFCELECTRICDISTRIBUTIONBOARD", ".DISTRIBUTIONBOARD."),
        Sym::SmokeDetector => ("IFCSENSOR", ".SMOKESENSOR."),
        Sym::Chime => ("IFCALARM", ".BELL."),
        Sym::AcWindow => ("IFCUNITARYEQUIPMENT", ".AIRCONDITIONINGUNIT."),
        _ => ("IFCUNITARYEQUIPMENT", ".SPLITSYSTEM."),
    })
}

/// One segment entity per straight segment of a run: a circle of the run
/// size swept along the segment, in storey local coordinates. Named after
/// the run; the GlobalId comes from the run id, `role` and the index.
fn run_segments(
    build: &mut Build,
    p: &guhit_model::Pipe,
    body_ctx: Ref,
    place_parent: Ref,
    entity: &str,
    predefined: &str,
    role: &str,
) -> Vec<Ref> {
    let Some(d) = pipes::diameter(p) else {
        return Vec::new();
    };
    let pts = pipes::points(p);
    if pts.len() < 2 {
        return Vec::new();
    }
    let name = pipes::display_name(p);
    let total = pts.len() - 1;
    let mut out = Vec::with_capacity(total);
    for (k, w) in pts.windows(2).enumerate() {
        let (a, b) = (w[0], w[1]);
        let Some(dir) = (b - a).unit() else { continue };
        let length = (b - a).len();
        let x = pipes::perpendicular(dir);
        let pos = build
            .step
            .axis_rotated((a.x, a.y, a.z), (dir.x, dir.y, dir.z), (x.x, x.y, x.z));
        let center = build.step.point2(0.0, 0.0);
        let at = build
            .step
            .shared(&format!("IFCAXIS2PLACEMENT2D({center},$)"));
        let profile = build.step.shared(&format!(
            "IFCCIRCLEPROFILEDEF(.AREA.,$,{at},{})",
            r(d / 2.0)
        ));
        let solid = build.step.extruded(profile, pos, length);
        let body = build.step.shape(body_ctx, "Body", "SweptSolid", &[solid]);
        let shape = build.step.product_shape(&[body]);
        let place = {
            let ax = build.step.axis_at(0.0, 0.0, 0.0);
            build.step.placement(Some(place_parent), ax)
        };
        out.push(build.step.add(&format!(
            "{entity}({},$,{},{},$,{place},{shape},$,{predefined})",
            s(&guid_for(&format!("{role}-{k}"), &p.id)),
            s(&name),
            s(&format!("Segment {} of {total}", k + 1))
        )));
    }
    out
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
        pipe_segments: n("IFCPIPESEGMENT"),
        pipe_systems: n("IFCDISTRIBUTIONSYSTEM"),
        outlets: n("IFCOUTLET"),
        switches: n("IFCSWITCHINGDEVICE"),
        light_fixtures: n("IFCLIGHTFIXTURE"),
        boards: n("IFCELECTRICDISTRIBUTIONBOARD"),
        sensors: n("IFCSENSOR"),
        alarms: n("IFCALARM"),
        unitary: n("IFCUNITARYEQUIPMENT"),
        cable_segments: n("IFCCABLECARRIERSEGMENT"),
        railings: n("IFCRAILING"),
        sanitary: n("IFCSANITARYTERMINAL"),
        grids: n("IFCGRID"),
        door_types: n("IFCDOORTYPE"),
        window_types: n("IFCWINDOWTYPE"),
        wall_types: n("IFCWALLTYPE"),
        quantity_sets: n("IFCELEMENTQUANTITY"),
        stair_flights: n("IFCSTAIRFLIGHT"),
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
