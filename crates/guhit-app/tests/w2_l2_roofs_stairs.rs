//! Lane L2, roofs and stairs (DECISIONS D54). The roof migration guard of
//! plan 1.7 lives here: once a project roof is migrated to a `TopFootprint`
//! element, its planes in the whole-model exports stay within 0.5 mm of
//! what Wave 1 drew, L-shaped gables included (P0-4). The oracle below is a
//! frozen copy of Wave 1's `model3d::roof_planes`, so the guard holds when
//! L2 replaces the roof math with `Derived::roofs`.

use guhit_core::{compute_derived, Document};
use guhit_export::geom::*;
use guhit_export::model3d::build_scene;
use guhit_model::*;

const TOLERANCE_MM: f64 = 0.5;

/// One Wave 1 roof plane: plan outline, origin, up slope, tan, cos, and
/// the thickness perpendicular to the plane.
struct OldPlane {
    plan: Vec<V>,
    origin: V,
    base: f64,
    up: V,
    tan: f64,
    cos: f64,
    thickness: f64,
}

impl OldPlane {
    fn under(&self, p: V) -> f64 {
        self.base + (p - self.origin).dot(self.up) * self.tan
    }
    fn top(&self, p: V) -> f64 {
        self.under(p) + self.thickness / self.cos
    }
}

/// Wave 1's roof planes over one footprint, frozen (model3d.rs at aa55335).
fn wave1_planes(roof: &Roof, footprint: &[V], base: f64) -> Vec<OldPlane> {
    let fp = ensure_ccw(footprint);
    let b = bounds_of(&fp);
    if roof.kind == RoofKind::None || fp.len() < 3 || b.is_empty() {
        return vec![];
    }
    let thickness = roof.thickness_mm.max(10.0);
    let outline = offset_polygon(&fp, roof.overhang_mm.max(0.0));
    if outline.len() < 3 {
        return vec![];
    }
    let plane = |plan: Vec<V>, origin: V, up: V, tan: f64, cos: f64| OldPlane { plan, origin, base, up, tan, cos, thickness };
    if roof.kind == RoofKind::Flat {
        return vec![plane(outline, b.min, v(0.0, 1.0), 0.0, 1.0)];
    }
    let pitch = if roof.pitch_deg.is_finite() { roof.pitch_deg.clamp(0.0, 60.0) } else { 0.0 }.to_radians();
    let (tan, cos) = (pitch.tan(), pitch.cos());
    let axis_x = roof.ridge_axis == Axis::X;
    if roof.kind == RoofKind::Shed {
        let up = if axis_x { v(1.0, 0.0) } else { v(0.0, 1.0) };
        return vec![plane(outline, b.min, up, tan, cos)];
    }
    let across = if axis_x { v(0.0, 1.0) } else { v(1.0, 0.0) };
    let (lo, hi) = if axis_x { (b.min.y, b.max.y) } else { (b.min.x, b.max.x) };
    let mid = (lo + hi) / 2.0;
    let low = clip_half_plane(&outline, v(0.0, 0.0), across, mid, false);
    let high = clip_half_plane(&outline, v(0.0, 0.0), across, mid, true);
    let mut out = vec![];
    for (plan, origin, up) in [(low, across * lo, across), (high, across * hi, -across)] {
        let plan = clean_polygon(&plan);
        if plan.len() >= 3 {
            out.push(plane(plan, origin, up, tan, cos));
        }
    }
    out
}

/// Wave 1's planes of an unmigrated project: the project roof over the
/// topmost level with a footprint.
fn old_planes(old: &Project) -> Vec<OldPlane> {
    let derived = compute_derived(old);
    let top = old
        .levels
        .iter()
        .filter(|l| derived.footprints.iter().any(|f| f.level_id == l.id && f.polygon.len() >= 3))
        .max_by(|a, b| a.elevation_mm.total_cmp(&b.elevation_mm));
    let Some(top) = top else { return vec![] };
    derived
        .footprints
        .iter()
        .filter(|f| f.level_id == top.id && f.polygon.len() >= 3)
        .flat_map(|f| wave1_planes(&old.roof, &f.polygon.iter().map(V::from).collect::<Vec<_>>(), top.height_mm))
        .collect()
}

/// The migrated project's planes, as the exports draw them now.
fn assert_same_planes(name: &str, old: &Project) {
    let expected = old_planes(old);
    let doc = Document::new(old.clone());
    assert_eq!(doc.project().roof.kind, RoofKind::None, "{name}: migrated");
    let scene = build_scene(doc.project(), doc.derived());
    let planes: Vec<_> = scene.roofs.iter().flat_map(|r| r.planes.iter()).collect();
    assert_eq!(planes.len(), expected.len(), "{name}: plane count");
    for (k, (new, old)) in planes.iter().zip(&expected).enumerate() {
        assert_eq!(new.plan.len(), old.plan.len(), "{name}: plane {k} corners");
        for (a, b) in new.plan.iter().zip(&old.plan) {
            assert!((*a - *b).len() <= TOLERANCE_MM, "{name}: plane {k} corner {a:?} vs {b:?}");
            assert!((new.top(*a) - old.top(*b)).abs() <= TOLERANCE_MM, "{name}: plane {k} top at {a:?}");
            assert!((new.under(*a) - old.under(*b)).abs() <= TOLERANCE_MM, "{name}: plane {k} underside at {a:?}");
        }
    }
}

fn data(name: &str) -> Project {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../guhit-core/tests/data").join(name);
    serde_json::from_str(&std::fs::read_to_string(path).expect("test data")).expect("a schema 5 project")
}

/// An L-shaped house, 10 x 8 m with a 5 x 4 m bite out of the north east,
/// as a schema 5 project with the given roof.
fn l_house(roof: Roof) -> Project {
    let mut doc = Document::new({
        let mut p = defaults::new_project("L");
        p.id = "l-house".into();
        p
    });
    let points = [(0.0, 0.0), (10000.0, 0.0), (10000.0, 4000.0), (5000.0, 4000.0), (5000.0, 8000.0), (0.0, 8000.0)]
        .map(|(x, y)| Point { x, y })
        .to_vec();
    doc.apply(
        Command::AddWallChain { points, closed: true, thickness_mm: None, level_id: None, role: None },
        Origin::User,
    )
    .unwrap();
    let mut old = doc.project().clone();
    old.roof = roof;
    old.schema_version = 5;
    old
}

#[test]
fn migrated_roofs_keep_their_wave_1_planes() {
    for name in ["schema5-sample-bungalow.project.json", "schema5-plumbing-demo.project.json", "wave1-g5-lovell.project.json"] {
        assert_same_planes(name, &data(name));
    }
    let base = defaults::default_roof();
    for (kind, axis) in [
        (RoofKind::Gable, Axis::X),
        (RoofKind::Gable, Axis::Y),
        (RoofKind::Shed, Axis::X),
        (RoofKind::Flat, Axis::X),
    ] {
        let roof = Roof { kind, ridge_axis: axis, pitch_deg: 25.0, ..base.clone() };
        assert_same_planes(&format!("L-shaped {kind:?} {axis:?}"), &l_house(roof));
    }
}

#[test]
fn the_exports_draw_every_roof_from_derived() {
    let doc = Document::new(guhit_core::templates::sample_bungalow());
    assert_eq!(doc.derived().roofs.len(), 1);
    let scene = build_scene(doc.project(), doc.derived());
    assert_eq!(scene.roofs.len(), 1);
    let g = &doc.derived().roofs[0];
    assert_eq!(scene.roofs[0].roof_id, g.roof_id);
    assert_eq!(scene.roofs[0].planes.len(), g.planes.len());
    // Each plane's top is the derived top, its underside `vertical_mm` lower.
    for (plane, top) in scene.roofs[0].planes.iter().zip(&g.planes) {
        for v3 in top {
            let p = v(v3.x, v3.y);
            assert!((plane.top(p) - v3.z).abs() < 1e-6);
            assert!((plane.under(p) - (v3.z - g.vertical_mm)).abs() < 1e-6);
        }
    }
    assert_eq!(scene.roofs[0].infill, g.infill, "gable ends go to the exports too");
}

/// A lanai roof on a lower level and a hip over the top floor: one solid
/// each, on its own level.
#[test]
fn drawn_roofs_join_the_scene_on_their_level() {
    let mut doc = Document::new(guhit_core::templates::sample_bungalow());
    let level = doc.project().levels[0].id.clone();
    let lanai = RoofArea {
        id: String::new(),
        level_id: Some(level.clone()),
        outline: RoofOutline::Polygon {
            points: [(-4000.0, 0.0), (-500.0, 0.0), (-500.0, 3000.0), (-4000.0, 3000.0)].map(|(x, y)| Point { x, y }).to_vec(),
        },
        shape: RoofShape::Shed,
        pitch_deg: 10.0,
        ridge_axis: Axis::X,
        flip: false,
        gable_sides: vec![],
        overhang_mm: 300.0,
        thickness_mm: 100.0,
        eave_mm: Some(2700.0),
        material_id: None,
        fascia: None,
        gutter: Some(Trim { size_mm: 125.0, material_id: None }),
    };
    doc.apply(Command::AddElement { element: Element::Roof(lanai) }, Origin::User).unwrap();
    let scene = build_scene(doc.project(), doc.derived());
    assert_eq!(scene.roofs.len(), 2);
    let shed = scene.roofs.iter().find(|r| r.kind == RoofKind::Shed).expect("the lanai roof");
    assert_eq!(scene.levels[shed.level].level.id, level);
    assert_eq!(shed.planes.len(), 1);
}

/// The IFC and the 3D DXF follow the roof over the top floor through
/// `SetRoof`, re-derived each time (moved from guhit-export's tests, which
/// cannot derive). The bungalow is one rectangle, so a hip is a real hip:
/// four planes and no gable ends; a shed fills its high side and its two
/// sloping ends, never the low side.
#[test]
fn ifc_and_dxf3d_roofs_follow_the_roof_kind() {
    use guhit_export::dxf3d::{build_faces, faces_per_layer};
    use guhit_export::ifc::count_entities;
    for (kind, roofs, planes, gables, tag) in [
        (RoofKind::None, 0usize, 0usize, 0usize, None),
        (RoofKind::Flat, 0, 1, 0, None),
        (RoofKind::Shed, 1, 1, 3, Some(".SHED_ROOF.")),
        (RoofKind::Gable, 1, 2, 2, Some(".GABLE_ROOF.")),
        (RoofKind::Hip, 1, 4, 0, Some(".HIP_ROOF.")),
    ] {
        let mut doc = Document::new(guhit_core::templates::sample_bungalow());
        let roof = Roof { kind, ..doc.project().top_roof() };
        doc.apply(Command::SetRoof { roof }, Origin::User).unwrap();
        let (p, d) = (doc.project(), doc.derived());
        let infill: usize = d.roofs.iter().map(|r| r.infill.len()).sum();
        assert_eq!(d.roofs.iter().map(|r| r.planes.len()).sum::<usize>(), planes, "{kind:?} planes");
        assert_eq!(infill, gables, "{kind:?} gable ends");
        let ifc = guhit_export::model_ifc(p, d).unwrap();
        let c = count_entities(&ifc);
        assert_eq!(c.roofs, roofs, "{kind:?} IfcRoof");
        assert_eq!(c.slabs, d.footprints.len() + planes, "{kind:?} slabs");
        let walls = p.elements.iter().filter(|e| matches!(e, Element::Wall(_))).count();
        assert_eq!(c.walls, walls + infill, "{kind:?} walls and gable ends");
        if let Some(tag) = tag {
            assert!(ifc.contains(tag), "{kind:?} {tag}");
        }
        let faces = build_faces(p, d);
        let per_layer: std::collections::HashMap<&str, usize> = faces_per_layer(&faces).into_iter().collect();
        let n = per_layer.get("A-ROOF").copied().unwrap_or(0);
        // A prism per plane (two caps and a side per corner) and the gable
        // ends.
        let least: usize = d.roofs.iter().flat_map(|r| r.planes.iter()).map(|pl| pl.len() + 2).sum::<usize>() + infill;
        assert!(n >= least, "{kind:?}: {n} roof faces, want {least}");
        if kind == RoofKind::None {
            assert_eq!(n, 0);
        }
    }
}

#[test]
fn stairs_come_apart_into_treads_and_landings() {
    let mut doc = Document::new(guhit_core::templates::sample_bungalow());
    let level = doc.project().levels[0].id.clone();
    let stair = Stair {
        id: String::new(),
        level_id: level,
        origin: Point { x: 2000.0, y: 1000.0 },
        rotation_deg: 0.0,
        width_mm: 1000.0,
        run_mm: 3750.0,
        riser_count: 17,
        railing: None,
        shape: StairShape::U,
        turn: Turn::Right,
        first_flight_risers: None,
        landing_mm: None,
        well_mm: 100.0,
    };
    let id = doc.apply(Command::AddElement { element: Element::Stair(stair) }, Origin::User).unwrap().diff.added[0].clone();
    let g = guhit_export::model3d::stair_geometry(doc.derived(), &id).expect("derived");
    let parts = guhit_export::model3d::stair_parts(g);
    assert_eq!(parts.iter().filter(|p| !p.landing).count(), 15, "n - 2 treads");
    assert_eq!(parts.iter().filter(|p| p.landing).count(), 1);
    for p in parts.iter().filter(|p| !p.landing) {
        assert!((polygon_area(&p.plan) - 1000.0 * 250.0).abs() < 1e-3, "every tread one going deep");
    }
}

/// A two storey box with one stair of `shape` on the ground floor.
fn stair_house(shape: StairShape) -> (Document, Id, Id) {
    let mut doc = Document::new({
        let mut p = defaults::new_project("Stairs");
        p.id = "stair-house".into();
        p
    });
    let ground = doc.project().levels[0].id.clone();
    let points = [(0.0, 0.0), (10000.0, 0.0), (10000.0, 8000.0), (0.0, 8000.0)].map(|(x, y)| Point { x, y }).to_vec();
    doc.apply(Command::AddWallChain { points: points.clone(), closed: true, thickness_mm: None, level_id: None, role: None }, Origin::User).unwrap();
    doc.apply(Command::AddLevel { name: Some("Second floor".into()), elevation_mm: Some(3000.0), height_mm: Some(3000.0) }, Origin::User).unwrap();
    let up = doc.project().levels[1].id.clone();
    doc.apply(Command::AddWallChain { points, closed: true, thickness_mm: None, level_id: Some(up.clone()), role: None }, Origin::User).unwrap();
    let stair = Stair {
        id: String::new(),
        level_id: ground.clone(),
        origin: Point { x: 3000.0, y: 1000.0 },
        rotation_deg: 0.0,
        width_mm: 1000.0,
        run_mm: if shape.is_straight() { 2750.0 } else { 3750.0 },
        riser_count: if shape.is_straight() { 11 } else { 17 },
        railing: None,
        shape,
        turn: Turn::Right,
        first_flight_risers: None,
        landing_mm: None,
        well_mm: if shape == StairShape::U { 100.0 } else { 0.0 },
    };
    doc.apply(Command::AddElement { element: Element::Stair(stair) }, Origin::User).unwrap();
    (doc, ground, up)
}

fn plan_items(doc: &Document, level_id: &str) -> Vec<guhit_export::plan::Item> {
    let level = doc.project().levels.iter().find(|l| l.id == level_id).unwrap();
    let mut out = vec![];
    guhit_export::stairs::stairs_items(doc.derived(), level, 100.0, &mut out);
    out
}

fn texts(items: &[guhit_export::plan::Item]) -> Vec<String> {
    items
        .iter()
        .filter_map(|i| match &i.prim {
            guhit_export::plan::Prim::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// The plan of a stair from `Derived::stairs` (what `plan.rs` draws once it
/// calls `stairs::stairs_items`): treads solid to the 1200 mm cut, a break
/// line, dashed above, UP; dashed with DN upstairs; the arrow round the turn.
#[test]
fn plans_draw_stairs_from_derived_with_the_cut_up_and_dn() {
    use guhit_export::plan::{Cat, Prim};
    let (doc, ground, up) = stair_house(StairShape::Straight);
    let items = plan_items(&doc, &ground);
    assert_eq!(texts(&items), vec!["UP".to_string()]);
    // 11 risers of 272.7 mm: nine tread lines between the 10 treads, at
    // nosings 2 to 10; those over the 1200 mm cut (from the 5th) are dashed.
    let tread = |i: &&guhit_export::plan::Item| {
        matches!(i.prim, Prim::Line { a, b } if (a.y - b.y).abs() < 1e-6 && (a.x - b.x).abs() > 999.0 && a.y > 1000.5 && a.y < 3749.5)
    };
    let solid = items.iter().filter(|i| i.cat == Cat::Strs).filter(tread).count();
    let dashed = items.iter().filter(|i| i.cat == Cat::StrsBelow).filter(tread).count();
    assert_eq!(solid + dashed, 9);
    assert_eq!(solid, 3, "nosings at 545, 818 and 1091 mm are under the cut");
    assert!(
        items.iter().any(|i| i.cat == Cat::Strs && matches!(&i.prim, Prim::Poly { closed: false, pts, .. } if pts.len() == 6)),
        "a break line"
    );
    let upstairs = plan_items(&doc, &up);
    assert_eq!(texts(&upstairs), vec!["DN".to_string()]);
    assert!(upstairs.iter().filter(|i| matches!(i.prim, Prim::Line { .. })).all(|i| i.cat == Cat::StrsBelow), "dashed from above");
    for shape in [StairShape::L, StairShape::U] {
        let (doc, ground, _) = stair_house(shape);
        let items = plan_items(&doc, &ground);
        // The arrow follows the walking line round the landing: a polyline with bends.
        let arrow = items
            .iter()
            .find_map(|i| match &i.prim {
                Prim::Poly { closed: false, pts, .. } if pts.len() >= 3 && pts.len() != 6 => Some(pts.clone()),
                _ => None,
            })
            .expect("an arrow round the turn");
        let turns = arrow
            .windows(3)
            .filter(|w| ((w[1] - w[0]).x * (w[2] - w[1]).y - (w[1] - w[0]).y * (w[2] - w[1]).x).abs() > 1.0)
            .count();
        assert!(turns >= 1, "{shape:?}: {arrow:?}");
        assert_eq!(texts(&items), vec!["UP".to_string()]);
        assert!(
            items.iter().any(|i| matches!(&i.prim, Prim::Poly { closed: true, pts, .. } if pts.len() == 4)),
            "{shape:?}: the landing"
        );
    }
}
