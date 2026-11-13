//! Wave 2 fix round, drawings and files: the gate's findings on elevations,
//! sections, posts, schedules and the sheets DXF, each on a small project
//! built here and derived by the engine.

mod common;

use common::*;
use guhit_export::geom::{v, V};
use guhit_export::plan::{Cat, Fill, Item, Prim};
use guhit_export::views::{self, annot};
use guhit_model::*;

fn pt(x: f64, y: f64) -> Point {
    Point { x, y }
}

/// The fixture's shell (settings, layers, materials) with no elements and
/// no sheets. Only its project is read: the tests derive their own.
fn shell() -> Project {
    let path = workspace_root().join("fixtures/sample-bungalow.docstate.json");
    let state: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut p: Project = serde_json::from_value(state["project"].clone()).unwrap();
    p.elements.clear();
    p.opening_types.clear();
    p.sheet_set = SheetSet::default();
    p
}

fn wall(p: &mut Project, level: &str, id: &str, a: (f64, f64), b: (f64, f64)) {
    p.elements.push(Element::Wall(Wall {
        id: id.into(),
        level_id: level.into(),
        start: pt(a.0, a.1),
        end: pt(b.0, b.1),
        thickness_mm: 150.0,
        height_mm: None,
        material_id: None,
        role: WallRole::Building,
    }));
}

/// Four walls round x0..x1, y0..y1, counter-clockwise.
fn box_walls(p: &mut Project, level: &str, tag: &str, (x0, y0, x1, y1): (f64, f64, f64, f64)) {
    let c = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
    for i in 0..4 {
        wall(p, level, &format!("{tag}-{i}"), c[i], c[(i + 1) % 4]);
    }
}

fn roof(id: &str, level: Option<&str>, outline: RoofOutline, shape: RoofShape, pitch: f64, axis: Axis) -> Element {
    Element::Roof(RoofArea {
        id: id.into(),
        level_id: level.map(String::from),
        outline,
        shape,
        pitch_deg: pitch,
        ridge_axis: axis,
        flip: false,
        gable_sides: vec![],
        overhang_mm: 600.0,
        thickness_mm: 150.0,
        eave_mm: None,
        material_id: None,
        fascia: None,
        gutter: None,
    })
}

fn mark(id: &str, level: &str, kind: MarkKind, label: &str, a: (f64, f64), b: (f64, f64)) -> Element {
    Element::ViewMark(ViewMark { id: id.into(), level_id: level.into(), mark_kind: kind, label: label.into(), start: pt(a.0, a.1), end: pt(b.0, b.1), flip: false, depth_mm: None })
}

const GROUND: &str = "lvl-ground";
const SECOND: &str = "lvl-second";

/// Two storeys of 9 x 8 m with a hip roof over the top and an inner wall
/// at x 2500 on the ground floor, four elevations and two sections.
fn two_storey(shape: RoofShape) -> (Project, Derived) {
    let mut p = shell();
    p.levels = vec![
        Level { id: GROUND.into(), name: "Ground Floor".into(), elevation_mm: 0.0, height_mm: 3000.0 },
        Level { id: SECOND.into(), name: "Second Floor".into(), elevation_mm: 3000.0, height_mm: 3000.0 },
    ];
    box_walls(&mut p, GROUND, "g", (0.0, 0.0, 9000.0, 8000.0));
    box_walls(&mut p, SECOND, "s", (0.0, 0.0, 9000.0, 8000.0));
    wall(&mut p, GROUND, "g-in", (2500.0, 0.0), (2500.0, 8000.0));
    p.elements.push(roof("roof-top", None, RoofOutline::TopFootprint, shape, 25.0, Axis::Y));
    let marks = [
        ("north", MarkKind::Elevation, "North", (14000.0, 14000.0), (-5000.0, 14000.0)),
        ("south", MarkKind::Elevation, "South", (-5000.0, -6000.0), (14000.0, -6000.0)),
        ("east", MarkKind::Elevation, "East", (15000.0, -5000.0), (15000.0, 13000.0)),
        ("west", MarkKind::Elevation, "West", (-6000.0, 13000.0), (-6000.0, -5000.0)),
        // Near the west wall, looking west: only the low part of the roof.
        ("sec-a", MarkKind::Section, "A", (700.0, 10000.0), (700.0, -2000.0)),
        ("sec-b", MarkKind::Section, "B", (-2000.0, 4300.0), (11000.0, 4300.0)),
    ];
    for (id, kind, label, a, b) in marks {
        p.elements.push(mark(id, GROUND, kind, label, a, b));
    }
    let d = guhit_core::compute_derived(&p);
    (p, d)
}

fn texts(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .filter_map(|i| match &i.prim {
            Prim::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn view_of(id: &str, kind: MarkKind) -> ViewRef {
    match kind {
        MarkKind::Elevation => ViewRef::Elevation { mark_id: id.into() },
        MarkKind::Section => ViewRef::Section { mark_id: id.into() },
    }
}

/// GR2 #1: every elevation and section prints the top of the roof from
/// `Derived::roofs`, never the highest roof edge that view shows.
#[test]
fn every_elevation_and_section_prints_the_same_top_of_roof() {
    let (p, d) = two_storey(RoofShape::Hip);
    let top = d.roofs.iter().flat_map(|g| g.planes.iter().flatten()).map(|q| q.z).fold(f64::NEG_INFINITY, f64::max) + 3000.0;
    assert!(top > 6000.0, "a roof over the second floor: {top}");
    let want = format!("{} TOP OF ROOF", annot::level_text(top));
    for (id, kind) in [("north", MarkKind::Elevation), ("south", MarkKind::Elevation), ("east", MarkKind::Elevation), ("west", MarkKind::Elevation), ("sec-a", MarkKind::Section), ("sec-b", MarkKind::Section)] {
        let v = views::items(&p, &d, &view_of(id, kind), Some(100)).unwrap_or_else(|e| panic!("{id}: {e}"));
        let t = texts(&v.items);
        let roofs: Vec<&String> = t.iter().filter(|x| x.ends_with("TOP OF ROOF")).collect();
        assert_eq!(roofs, vec![&want], "{id}");
    }
}

/// The poche polygons of a view.
fn poche(items: &[Item]) -> Vec<(Cat, Vec<V>)> {
    items
        .iter()
        .filter_map(|i| match &i.prim {
            Prim::Poly { pts, closed: true, fill: Fill::Ink } => Some((i.cat, pts.clone())),
            _ => None,
        })
        .collect()
}

/// GR2 #4: a gable end is a wall as thick as the one under it, so a
/// section through it cuts it in poche up to the roof, not only to the
/// plate.
#[test]
fn sections_cut_gable_ends_in_poche_up_to_the_roof() {
    let (p, d) = two_storey(RoofShape::Gable);
    assert!(d.roofs.iter().any(|g| !g.infill.is_empty()), "the gable roof has infill");
    let mesh = views::mesh::build(&p, &d);
    for s in mesh.solids.iter().filter(|s| s.element_id == "roof-top" && s.part == views::mesh::Part::Wall) {
        assert!(s.closed, "a gable end is a closed solid");
    }
    // Section A runs north to south at x 700 and cuts both gable ends.
    let v = views::items(&p, &d, &view_of("sec-a", MarkKind::Section), Some(100)).unwrap();
    let walls: Vec<Vec<V>> = poche(&v.items).into_iter().filter(|(c, _)| *c == Cat::Wall).map(|(_, p)| p).collect();
    let top = walls.iter().flatten().map(|q| q.y).fold(f64::NEG_INFINITY, f64::max);
    // The underside 775 mm in from the wall line at 25 degrees: 6361.
    assert!(top > 6300.0, "the gable end is cut up to the roof: wall poche reaches {top}");
    // Both ends, each about 150 thick.
    let ends = walls.iter().filter(|w| w.iter().any(|q| q.y > 6100.0)).count();
    assert!(ends >= 2, "{ends} gable ends cut");
    for w in walls.iter().filter(|w| w.iter().any(|q| q.y > 6100.0)) {
        let b = guhit_export::geom::bounds_of(w);
        assert!(b.width() > 100.0 && b.width() < 200.0, "a wall's thickness: {b:?}");
    }
}

/// One storey of 9 x 8 m with a 25 degree shed carport east of it on four
/// posts, rising away from the house.
fn carport() -> (Project, Derived) {
    let mut p = shell();
    p.levels = vec![Level { id: GROUND.into(), name: "Ground Floor".into(), elevation_mm: 0.0, height_mm: 3000.0 }];
    box_walls(&mut p, GROUND, "g", (0.0, 0.0, 9000.0, 8000.0));
    let outline = RoofOutline::Polygon { points: vec![pt(9075.0, 1000.0), pt(13000.0, 1000.0), pt(13000.0, 6000.0), pt(9075.0, 6000.0)] };
    let mut shed = roof("carport", Some(GROUND), outline, RoofShape::Shed, 25.0, Axis::X);
    if let Element::Roof(r) = &mut shed {
        r.overhang_mm = 300.0;
    }
    p.elements.push(shed);
    for (i, q) in POSTS.iter().enumerate() {
        p.elements.push(Element::Column(Column {
            id: format!("post-{i}"),
            level_id: GROUND.into(),
            center: pt(q.0, q.1),
            shape: ColumnShape::Rect,
            width_mm: 200.0,
            depth_mm: 200.0,
            rotation_deg: 0.0,
            material_id: None,
        }));
    }
    let d = guhit_core::compute_derived(&p);
    (p, d)
}

const POSTS: [(f64, f64); 4] = [(9400.0, 1300.0), (12700.0, 1300.0), (12700.0, 5700.0), (9400.0, 5700.0)];

/// The carport's underside at a plan point, from its derived planes
/// (z = a x + b y + c through three corners, less the vertical thickness).
fn carport_under(d: &Derived, at: (f64, f64)) -> f64 {
    let g = d.roofs.iter().find(|g| g.roof_id == "carport").expect("the carport roof is derived");
    let plane = g
        .planes
        .iter()
        .find(|pl| guhit_export::geom::point_in_polygon(v(at.0, at.1), &pl.iter().map(|q| v(q.x, q.y)).collect::<Vec<_>>()))
        .expect("a plane over the post");
    let (a, b, c) = (plane[0], plane[1], plane[2]);
    let (u, w) = ((b.x - a.x, b.y - a.y, b.z - a.z), (c.x - a.x, c.y - a.y, c.z - a.z));
    let n = (u.1 * w.2 - u.2 * w.1, u.2 * w.0 - u.0 * w.2, u.0 * w.1 - u.1 * w.0);
    a.z - (n.0 * (at.0 - a.x) + n.1 * (at.1 - a.y)) / n.2 - g.vertical_mm
}

/// GR2 #5: carport posts run to the underside of the roof over them in
/// the drawings' mesh, the 3D DXF and the IFC, not to the level height.
#[test]
fn posts_run_to_the_roof_over_them_in_every_export() {
    let (p, d) = carport();
    let want: Vec<f64> = POSTS.iter().map(|q| carport_under(&d, *q)).collect();
    assert!(want.iter().any(|w| (w - 3000.0).abs() > 300.0), "the shed rises well past the level height: {want:?}");

    // model3d, the shared rule.
    for (i, q) in POSTS.iter().enumerate() {
        let id = format!("post-{i}");
        let c = p.elements.iter().find_map(|e| match e {
            Element::Column(c) if c.id == id => Some(c),
            _ => None,
        });
        let h = guhit_export::model3d::column_height(&d, &p.levels[0], c.unwrap());
        assert!((h - want[i]).abs() < 0.5, "post {i} at {q:?}: {h} vs underside {}", want[i]);
    }

    // The drawings' mesh: the post's top follows the roof at every corner.
    let mesh = views::mesh::build(&p, &d);
    for i in 0..POSTS.len() {
        let id = format!("post-{i}");
        let s = mesh.solids.iter().position(|s| s.element_id == id).expect("the post's solid") as u32;
        for f in mesh.faces.iter().filter(|f| f.solid == s) {
            for c in f.pts.iter().filter(|c| c.z > 1.0) {
                let under = carport_under(&d, (c.x, c.y));
                assert!((c.z - under).abs() < 0.5, "post {i}: top {} at ({}, {}) vs underside {under}", c.z, c.x, c.y);
            }
        }
    }

    // The 3D DXF: the same faces.
    let faces = guhit_export::dxf3d::build_faces(&p, &d);
    let cols: Vec<_> = faces.list.iter().filter(|f| f.layer == "A-COLS").collect();
    assert!(!cols.is_empty());
    for f in cols {
        for c in f.pts.iter().filter(|c| c.z > 1.0) {
            let under = carport_under(&d, (c.x, c.y));
            assert!((c.z - under).abs() < 0.5, "3D DXF post corner at ({}, {}): {} vs underside {under}", c.x, c.y, c.z);
        }
    }

    // The IFC: each IfcColumn is extruded to the underside at its center.
    let ifc = guhit_export::model_ifc(&p, &d).unwrap();
    let mut depths = ifc_column_depths(&ifc);
    depths.sort_by(f64::total_cmp);
    let mut want = want;
    want.sort_by(f64::total_cmp);
    assert_eq!(depths.len(), 4, "{depths:?}");
    for (got, w) in depths.iter().zip(&want) {
        assert!((got - w).abs() < 0.5, "IFC column depth {got} vs underside {w}");
    }
}

/// Every `#n` reference in a STEP record.
fn step_refs(s: &str) -> Vec<String> {
    let b = s.as_bytes();
    let (mut out, mut i) = (vec![], 0);
    while i < b.len() {
        if b[i] == b'#' {
            let j = (i + 1..b.len()).find(|&j| !b[j].is_ascii_digit()).unwrap_or(b.len());
            out.push(s[i..j].to_string());
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// The extrusion depth of every IfcColumn's body.
fn ifc_column_depths(ifc: &str) -> Vec<f64> {
    let lines: std::collections::HashMap<&str, &str> = ifc.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim(), v.trim())).collect();
    let mut out = vec![];
    for rec in lines.values().filter(|v| v.starts_with("IFCCOLUMN(")) {
        // The placement first, then the product shape.
        let r = step_refs(rec);
        let Some(shape) = r.get(1).and_then(|k| lines.get(k.as_str())) else { continue };
        for rep in step_refs(shape) {
            let Some(body) = lines.get(rep.as_str()) else { continue };
            for solid in step_refs(body) {
                if let Some(x) = lines.get(solid.as_str()).filter(|x| x.starts_with("IFCEXTRUDEDAREASOLID(")) {
                    out.push(x.trim_end_matches(");").rsplit(',').next().unwrap().parse::<f64>().unwrap());
                }
            }
        }
    }
    out
}

fn spec(id: &str, mark: &str, kind: OpeningType, style: OpeningStyle, w: f64, h: f64, sill: f64) -> OpeningSpec {
    OpeningSpec { id: id.into(), mark: mark.into(), opening_type: kind, style, width_mm: w, height_mm: h, sill_mm: Some(sill), material_id: None, description: String::new(), remarks: String::new() }
}

#[allow(clippy::too_many_arguments)]
fn opening(p: &mut Project, id: &str, wall: &str, type_id: &str, kind: OpeningType, style: OpeningStyle, at: f64, w: f64, h: f64, sill: f64) {
    p.elements.push(Element::Opening(Opening {
        id: id.into(),
        wall_id: wall.into(),
        opening_type: kind,
        style,
        offset_mm: at,
        width_mm: w,
        height_mm: h,
        sill_mm: sill,
        flip_side: false,
        flip_hinge: false,
        material_id: None,
        type_id: Some(type_id.into()),
    }));
}

/// GS P1: an elevation tags only the doors and windows hidden line removal
/// leaves in view. A door straight behind the front door (its tag landed
/// on the front door's) or behind a solid wall gets none.
#[test]
fn elevations_tag_only_openings_left_in_view() {
    let mut p = shell();
    p.levels = vec![Level { id: GROUND.into(), name: "Ground Floor".into(), elevation_mm: 0.0, height_mm: 3000.0 }];
    box_walls(&mut p, GROUND, "g", (0.0, 0.0, 9000.0, 8000.0));
    wall(&mut p, GROUND, "inner", (0.0, 4000.0), (9000.0, 4000.0));
    let swing = OpeningStyle::SwingSingle;
    p.opening_types = vec![
        spec("t-d1", "D1", OpeningType::Door, swing, 900.0, 2100.0, 0.0),
        spec("t-d2", "D2", OpeningType::Door, swing, 800.0, 2100.0, 0.0),
        spec("t-d3", "D3", OpeningType::Door, OpeningStyle::Sliding, 1500.0, 2100.0, 0.0),
        spec("t-w1", "W1", OpeningType::Window, OpeningStyle::Casement, 1200.0, 1200.0, 900.0),
    ];
    // The front door, the door behind it, a door behind solid wall, a window.
    opening(&mut p, "d1", "g-0", "t-d1", OpeningType::Door, swing, 4000.0, 900.0, 2100.0, 0.0);
    opening(&mut p, "d2", "inner", "t-d2", OpeningType::Door, swing, 4000.0, 800.0, 2100.0, 0.0);
    opening(&mut p, "d3", "inner", "t-d3", OpeningType::Door, OpeningStyle::Sliding, 7000.0, 1500.0, 2100.0, 0.0);
    opening(&mut p, "w1", "g-0", "t-w1", OpeningType::Window, OpeningStyle::Casement, 2000.0, 1200.0, 1200.0, 900.0);
    p.elements.push(mark("south", GROUND, MarkKind::Elevation, "South", (-5000.0, -6000.0), (14000.0, -6000.0)));
    let d = guhit_core::compute_derived(&p);
    assert_eq!(d.opening_schedule.marks.len(), 4, "every opening has its mark");
    let v = views::items(&p, &d, &view_of("south", MarkKind::Elevation), Some(100)).unwrap();
    let t = texts(&v.items);
    let count = |m: &str| t.iter().filter(|x| *x == m).count();
    assert_eq!((count("D1"), count("W1")), (1, 1), "the front door and window: {t:?}");
    assert_eq!((count("D2"), count("D3")), (0, 0), "doors out of view: {t:?}");
}

/// GS P1: the floor a stair arrives at has its stair opening cut out, as
/// the 3D view cuts it, so a section through the landing shows no slab
/// over the stair.
#[test]
fn sections_cut_the_stair_opening_out_of_the_floor_above() {
    let (mut p, _) = two_storey(RoofShape::Hip);
    p.elements.push(Element::Stair(Stair {
        id: "stair".into(),
        level_id: GROUND.into(),
        origin: pt(5000.0, 1000.0),
        rotation_deg: 0.0,
        width_mm: 1000.0,
        run_mm: 2000.0,
        riser_count: 17,
        railing: None,
        shape: StairShape::L,
        turn: Turn::Left,
        first_flight_risers: Some(9),
        landing_mm: None,
        well_mm: 0.0,
    }));
    let d = guhit_core::compute_derived(&p);
    let g = d.stairs.iter().find(|g| g.stair_id == "stair").expect("the stair is derived");
    assert_eq!(g.arrives_at.as_deref(), Some(SECOND));
    let hole: Vec<V> = g.opening.iter().map(V::from).collect();
    assert!(hole.len() >= 3, "the stair opens the second floor");
    let landing: Vec<V> = g.landings[0].polygon.iter().map(V::from).collect();
    let probe = guhit_export::geom::centroid(&landing);

    // The second floor's slab has no top over the landing.
    let mesh = views::mesh::build(&p, &d);
    let floor = format!("footprint:{SECOND}:0");
    let s = mesh.solids.iter().position(|s| s.element_id == floor).expect("the second floor slab") as u32;
    for f in mesh.faces.iter().filter(|f| f.solid == s && f.pts.iter().all(|q| (q.z - 3000.0).abs() < 0.5)) {
        let plan: Vec<V> = f.pts.iter().map(|q| v(q.x, q.y)).collect();
        assert!(!guhit_export::geom::point_in_polygon(probe, &plan), "a slab top over the landing at {probe:?}");
    }

    // A section along x through the landing, looking north.
    p.elements.push(mark("sec-c", GROUND, MarkKind::Section, "C", (-2000.0, probe.y), (11000.0, probe.y)));
    let v_items = views::items(&p, &d, &view_of("sec-c", MarkKind::Section), Some(50)).unwrap();
    let hb = guhit_export::geom::bounds_of(&hole);
    let (s0, s1) = (hb.min.x + 2000.0 + 10.0, hb.max.x + 2000.0 - 10.0);
    let over = [v(s0, 2860.0), v(s1, 2860.0), v(s1, 2990.0), v(s0, 2990.0)];
    for (cat, poly) in poche(&v_items.items) {
        let a = guhit_export::geom::overlap_area(&ensure(&poly), &over);
        assert!(a < 1.0, "{cat:?} poche over the stair opening: {a} mm2");
    }
}

/// 96 rooms of 2.5 x 2.5 m in a 12 x 8 grid and 30 window types, one
/// window each: long schedules (the gate's `longsched.py`).
fn many_rooms() -> (Project, Derived) {
    let mut p = shell();
    p.levels = vec![Level { id: GROUND.into(), name: "Ground Floor".into(), elevation_mm: 0.0, height_mm: 3000.0 }];
    let c = 2500.0;
    for j in 0..=8 {
        for i in 0..12 {
            wall(&mut p, GROUND, &format!("h-{i}-{j}"), (i as f64 * c, j as f64 * c), ((i + 1) as f64 * c, j as f64 * c));
        }
    }
    for i in 0..=12 {
        for j in 0..8 {
            wall(&mut p, GROUND, &format!("v-{i}-{j}"), (i as f64 * c, j as f64 * c), (i as f64 * c, (j + 1) as f64 * c));
        }
    }
    for i in 0..12 {
        for j in 0..8 {
            p.elements.push(Element::Room(Room {
                id: format!("room-{i}-{j}"),
                level_id: GROUND.into(),
                name: format!("Room {i:02}-{j}"),
                usage: RoomUsage::Bedroom,
                seed: pt(i as f64 * c + c / 2.0, j as f64 * c + c / 2.0),
                floor_material_id: None,
                auto_named: false,
                finishes: RoomFinishes::default(),
                ceiling_height_mm: None,
            }));
        }
    }
    for k in 0..30 {
        let w = 600.0 + 20.0 * k as f64;
        let id = format!("t-w{}", k + 1);
        p.opening_types.push(spec(&id, &format!("W{}", k + 1), OpeningType::Window, OpeningStyle::Casement, w, 1000.0, 900.0));
        // South walls, north walls, then west walls.
        let host = if k < 12 { format!("h-{k}-0") } else if k < 24 { format!("h-{}-8", k - 12) } else { format!("v-0-{}", k - 24) };
        opening(&mut p, &format!("w{}", k + 1), &host, &id, OpeningType::Window, OpeningStyle::Casement, 1250.0, w, 1000.0, 900.0);
    }
    let d = guhit_core::compute_derived(&p);
    (p, d)
}

fn inside(a: (f64, f64, f64, f64), area: (f64, f64, f64, f64)) -> bool {
    a.0 >= area.0 - 1e-6 && a.1 >= area.1 - 1e-6 && a.0 + a.2 <= area.0 + area.2 + 1e-6 && a.1 + a.3 <= area.1 + area.3 + 1e-6
}

fn overlaps(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    a.0 < b.0 + b.2 - 1e-6 && b.0 < a.0 + a.2 - 1e-6 && a.1 < b.1 + b.3 - 1e-6 && b.1 < a.1 + a.3 - 1e-6
}

/// GR2 #3: on A4 and A3 the four schedules of 96 rooms and 30 window types
/// continue on more sheets, numbered on from their sheet and counted in
/// "N OF M", each row printed once, and nothing overlaps, runs under the
/// title block or is clipped.
#[test]
fn long_schedules_continue_on_more_sheets_and_never_overlap() {
    use guhit_export::sheetset::{self, PrintedScale};
    let (mut p, d) = many_rooms();
    assert_eq!(d.room_schedule.len(), 96, "96 rooms");
    assert_eq!(d.opening_schedule.rows.len(), 30, "30 window types");
    for paper in [PaperSize::A4, PaperSize::A3] {
        let views = [ScheduleKind::DoorsWindows, ScheduleKind::Areas, ScheduleKind::Rooms, ScheduleKind::Finishes];
        p.sheet_set.sheets = vec![Sheet {
            id: "sheet-schedules".into(),
            number: "A-7".into(),
            title: "Schedules".into(),
            paper,
            orientation: Orientation::Landscape,
            viewports: views
                .iter()
                .enumerate()
                .map(|(j, k)| Viewport { id: format!("vp-{j}"), view: ViewRef::Schedule { schedule: *k }, scale_denominator: None, position: None, title: None })
                .collect(),
            revision_ids: vec![],
        }];
        let inputs = guhit_export::SheetInputs::default();
        let printed = sheetset::printed_sheets(&p, &d, &[], &inputs).unwrap();
        assert!(printed.len() > 1, "{paper:?}: the schedules need more than one sheet");
        let mut rows: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for (k, s) in printed.iter().enumerate() {
            assert!(s.fits, "{paper:?} {}: everything fits", s.number);
            assert_eq!((s.index, s.total), (k + 1, printed.len()), "{paper:?}: N OF M recounted");
            assert_eq!(s.number, if k == 0 { "A-7".to_string() } else { format!("A-7.{k}") });
            for (a, vp) in s.viewports.iter().enumerate() {
                assert!(inside(vp.frame, s.area), "{paper:?} {}: {} inside the drawing area", s.number, vp.title);
                assert_eq!(vp.scale, PrintedScale::None, "a table has no scale");
                for other in s.viewports.iter().skip(a + 1) {
                    assert!(!overlaps(vp.frame, other.frame), "{paper:?} {}: {} over {}", s.number, vp.title, other.title);
                }
                let n = match &vp.part {
                    Some(pt) => pt.to - pt.from,
                    None => match &vp.viewport.view {
                        ViewRef::Schedule { schedule } => guhit_export::tables::schedule(&p, &d, *schedule).rows.len(),
                        _ => 0,
                    },
                };
                *rows.entry(vp.viewport.id.clone()).or_default() += n;
            }
        }
        assert_eq!(rows.get("vp-0"), Some(&30), "{paper:?}: every window type once");
        assert_eq!(rows.get("vp-2"), Some(&96), "{paper:?}: every room once in the room schedule");
        assert_eq!(rows.get("vp-3"), Some(&96), "{paper:?}: every room once in the finish schedule");
        // The pictures say the same: each room twice (rooms and finishes),
        // each mark once, "N OF M" on every sheet.
        let svgs = sheetset::sheets_svg(&p, &d, &[], &inputs).unwrap();
        assert_eq!(svgs.len(), printed.len());
        let all: String = svgs.iter().map(|(_, s)| s.as_str()).collect();
        for i in 0..12 {
            for j in 0..8 {
                assert_eq!(all.matches(&format!(">Room {i:02}-{j}<")).count(), 2, "{paper:?}: Room {i:02}-{j}");
            }
        }
        for k in 1..=30 {
            assert_eq!(all.matches(&format!(">W{k}<")).count(), 1, "{paper:?}: W{k}");
        }
        for (k, (number, svg)) in svgs.iter().enumerate() {
            assert!(svg.contains(&format!("{} OF {}", k + 1, svgs.len())), "{paper:?}: sheet {}", k + 1);
            dump(&format!("long-{paper:?}-{number}"), svg);
        }
        assert!(all.contains("(CONTINUED)") && all.contains("Continued on sheet A-7.1."), "{paper:?}: continuations say so");
    }
}

fn sheet(id: &str, number: &str, title: &str, paper: PaperSize, views: Vec<ViewRef>) -> Sheet {
    Sheet {
        id: id.into(),
        number: number.into(),
        title: title.into(),
        paper,
        orientation: Orientation::Landscape,
        viewports: views
            .into_iter()
            .enumerate()
            .map(|(j, view)| Viewport { id: format!("{id}-vp{j}"), view, scale_denominator: None, position: None, title: None })
            .collect(),
        revision_ids: vec![],
    }
}

/// GR2 #6, #7 and GS P2: the sheets DXF draws each view at the scale the
/// sheet layout chose, a scale line only on views drawn to scale,
/// schedules as tables and no error text; each sheet has a title block.
#[test]
fn the_sheets_dxf_uses_the_sheet_scales_and_draws_schedules() {
    use guhit_export::sheetset::{self, PrintedScale};
    let (mut p, _) = two_storey(RoofShape::Hip);
    p.title_block.signatory.name = "Juan Dela Cruz (sample)".into();
    p.title_block.signatory.prc_no = "0012345".into();
    p.sheet_set.sheets = vec![
        sheet("s1", "A-1", "Ground floor plan", PaperSize::A1, vec![ViewRef::Plan { level_id: GROUND.into(), sheet: SheetKind::Plan }, ViewRef::VicinityMap { file_name: None, radius_m: 500.0 }]),
        sheet("s2", "A-2", "Elevations", PaperSize::A1, vec![view_of("north", MarkKind::Elevation), view_of("sec-b", MarkKind::Section)]),
        sheet("s3", "A-3", "Schedules", PaperSize::A1, vec![ViewRef::Schedule { schedule: ScheduleKind::Areas }, ViewRef::Schedule { schedule: ScheduleKind::Rooms }]),
        sheet("s4", "A-4", "Perspective", PaperSize::A1, vec![ViewRef::Perspective { render_id: "render-1".into() }]),
    ];
    let d = guhit_core::compute_derived(&p);
    let printed = sheetset::printed_sheets(&p, &d, &[], &guhit_export::SheetInputs::default()).unwrap();
    let scaled: Vec<u32> = printed
        .iter()
        .flat_map(|s| s.viewports.iter())
        .filter_map(|vp| match vp.scale {
            PrintedScale::Scaled(n) => Some(n),
            _ => None,
        })
        .collect();
    assert!(scaled.iter().any(|n| *n != p.settings.scale_denominator), "the layout picks a scale of its own: {scaled:?}");
    let text = guhit_export::sheets_dxf(&p, &d, &[]).unwrap();
    if let Ok(dir) = std::env::var("GUHIT_FIX_OUT") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::Path::new(&dir).join("sheets.dxf"), &text).unwrap();
    }
    let drawing = dxf::Drawing::load(&mut text.as_bytes()).expect("the DXF reads");
    let texts: Vec<String> = drawing
        .entities()
        .filter_map(|e| match &e.specific {
            dxf::entities::EntityType::Text(t) => Some(t.value.clone()),
            _ => None,
        })
        .collect();
    let mut lines: Vec<String> = texts.iter().filter(|t| t.starts_with("SCALE 1:")).cloned().collect();
    lines.sort();
    let mut want: Vec<String> = scaled.iter().map(|n| format!("SCALE 1:{n}")).collect();
    want.sort();
    assert_eq!(lines, want, "one scale line per scaled view, at the sheet's scale");
    assert_eq!(texts.iter().filter(|t| *t == "NOT TO SCALE").count(), 1, "the vicinity map");
    for t in &texts {
        let low = t.to_lowercase();
        assert!(!low.contains("failed") && !low.contains("error") && !low.contains("not a drawing"), "error text in the DXF: {t}");
    }
    // The schedules as tables: headers, rows and totals.
    for want in ["FLOOR AREAS", "ROOM SCHEDULE", "LEVEL", "NET FLOOR AREA", "TOTAL"] {
        assert!(texts.iter().any(|t| t == want), "{want} in the DXF");
    }
    // A title block per sheet: number and N OF M, the name but no license number.
    for (k, s) in printed.iter().enumerate() {
        assert!(texts.iter().any(|t| t == &s.number), "{} in a title block", s.number);
        assert!(texts.iter().any(|t| *t == format!("{} OF {}", k + 1, printed.len())), "N OF M on {}", s.number);
    }
    assert!(texts.iter().any(|t| t == "Juan Dela Cruz (sample)"));
    assert!(!text.contains("0012345"), "license numbers print on the PDF only");
    assert!(drawing.layers().any(|l| l.name == "A-ANNO-TTLB"));
    // The perspective is an image: named in the header, not drawn.
    assert!(text.contains("Not drawn here: A-4 PERSPECTIVE"), "the header names what was left out");
}

/// GR1 #3 (F1 keeps a clockwise title's corner order): the site plan
/// numbers the corners and reads each side's bearing in the stored order,
/// and puts roads outside the lot and setback notes inside it, whichever
/// way the corners run.
#[test]
fn the_site_plan_follows_the_title_order_either_way_round() {
    for clockwise in [true, false] {
        let mut p = shell();
        p.levels = vec![Level { id: GROUND.into(), name: "Ground Floor".into(), elevation_mm: 0.0, height_mm: 3000.0 }];
        box_walls(&mut p, GROUND, "g", (3000.0, 4000.0, 9000.0, 11000.0));
        // Corner 1 at the south west, then up the west side (clockwise) or
        // along the south side (counter-clockwise).
        let mut corners = vec![pt(0.0, 0.0), pt(0.0, 15000.0), pt(12000.0, 15000.0), pt(12000.0, 0.0)];
        if !clockwise {
            corners = vec![pt(0.0, 0.0), pt(12000.0, 0.0), pt(12000.0, 15000.0), pt(0.0, 15000.0)];
        }
        // The road side runs along y 0 either way.
        let road = if clockwise { 3 } else { 0 };
        let sides = (0..4)
            .map(|i| LotSide {
                role: if i == road { LotSideRole::Road } else { LotSideRole::Side },
                setback_mm: if i == road { 3000.0 } else { 1500.0 },
                road: if i == road { "Mabini Street".into() } else { String::new() },
                road_width_mm: (i == road).then_some(8000.0),
            })
            .collect();
        p.elements.push(Element::Lot(Lot { id: "lot".into(), corners: corners.clone(), sides, grade_mm: 150.0 }));
        let d = guhit_core::compute_derived(&p);
        let site = d.site.as_deref().expect("the lot is derived");
        let v_items = views::items(&p, &d, &ViewRef::SitePlan, Some(200)).unwrap();
        let at = |s: &str| -> Vec<V> {
            v_items
                .items
                .iter()
                .filter_map(|i| match &i.prim {
                    Prim::Text { pos, text, .. } if text == s => Some(*pos),
                    _ => None,
                })
                .collect()
        };
        let lot: Vec<V> = corners.iter().map(V::from).collect();
        for (k, c) in lot.iter().enumerate() {
            let labels = at(&(k + 1).to_string());
            assert!(labels.iter().any(|q| (*q - *c).len() < 1500.0), "corner {} by {c:?}: {labels:?}", k + 1);
        }
        for (k, side) in site.sides.iter().enumerate() {
            let mid = (lot[k] + lot[(k + 1) % 4]) * 0.5;
            let found = at(&side.bearing);
            assert!(found.iter().any(|q| (*q - mid).len() < 2500.0), "side {} bearing {} by {mid:?}: {found:?}", k + 1, side.bearing);
        }
        // The first side reads as the title walks it.
        assert_eq!(site.sides[0].bearing, if clockwise { "N 0\u{b0}00' E" } else { "N 90\u{b0}00' E" });
        // The road beyond the south line, the setback note inside.
        let name = at("MABINI STREET");
        assert!(name.len() == 1 && name[0].y < 0.0, "the road outside the lot: {name:?}");
        let note = at("3.00 m SETBACK");
        assert!(note.len() == 1 && guhit_export::geom::point_in_polygon(note[0], &lot), "the setback note inside: {note:?}");

        // GA P1-10: each building face dimensioned to its lot line with the
        // engine's clearance, the required setback under it: 2.925 m to
        // the west and east lines (the faces are 75 out from the wall
        // centerlines), 3.925 m to the south and north ones.
        for side in &site.sides {
            let c = side.clearance_mm.expect("a building on the lot");
            assert!(!at(&format!("{:.2} m", c / 1000.0)).is_empty(), "clearance {c} dimensioned");
        }
        let dims: Vec<(V, V)> = v_items
            .items
            .iter()
            .filter(|i| i.cat == Cat::Dims)
            .filter_map(|i| match i.prim {
                Prim::Line { a, b } if (b - a).len() > 1000.0 => Some((a, b)),
                _ => None,
            })
            .collect();
        assert_eq!(dims.len(), 4, "four clearance dimensions: {dims:?}");
        let mut lens: Vec<f64> = dims.iter().map(|(a, b)| (*b - *a).len()).collect();
        lens.sort_by(f64::total_cmp);
        let mut want: Vec<f64> = site.sides.iter().filter_map(|s| s.clearance_mm).collect();
        want.sort_by(f64::total_cmp);
        for (l, w) in lens.iter().zip(&want) {
            assert!((l - w).abs() < 1.0, "a dimension {l} long reads {w}");
        }
        assert_eq!(at("SETBACK 3.00 m").len(), 1);
        assert_eq!(at("SETBACK 1.50 m").len(), 3);
    }
}

/// GS P2: a section or elevation mark's bubble moves out along its line
/// until it clears the grid bubbles, instead of printing over one.
#[test]
fn mark_bubbles_clear_grid_bubbles_on_plans() {
    let mut p = shell();
    p.levels = vec![Level { id: GROUND.into(), name: "Ground Floor".into(), elevation_mm: 0.0, height_mm: 3000.0 }];
    box_walls(&mut p, GROUND, "g", (0.0, 0.0, 9000.0, 8000.0));
    let grid = |id: &str, label: &str, a: (f64, f64), b: (f64, f64)| Element::Grid(GridLine { id: id.into(), label: label.into(), start: pt(a.0, a.1), end: pt(b.0, b.1) });
    p.elements.push(grid("g1", "1", (0.0, -2500.0), (0.0, 10500.0)));
    p.elements.push(grid("ga", "A", (-2500.0, 0.0), (11500.0, 0.0)));
    // A section 300 mm beside grid 1 and ending where it ends; an
    // elevation whose bubble lands on grid A's.
    p.elements.push(mark("sec-b", GROUND, MarkKind::Section, "B", (300.0, -2500.0), (300.0, 10500.0)));
    p.elements.push(mark("elev-w", GROUND, MarkKind::Elevation, "W", (-2120.0, 1500.0), (-2120.0, -1500.0)));
    let d = guhit_core::compute_derived(&p);
    let n = 100.0;
    let items = guhit_export::plan::build_items(
        &p,
        &d,
        &p.levels[0],
        &guhit_export::plan::PlanOptions { scale: n, show_dimensions: true, show_room_labels: true, show_assets: true, unicode: true, skip_devices: false },
    );
    let circles = |cat: Cat| -> Vec<(V, f64)> {
        items
            .iter()
            .filter(|i| i.cat == cat)
            .filter_map(|i| match &i.prim {
                Prim::Circle { c, r, .. } if *r > 300.0 => Some((*c, *r)),
                _ => None,
            })
            .collect()
    };
    let (grids, marks) = (circles(Cat::Grid), circles(Cat::Symb));
    assert_eq!((grids.len(), marks.len()), (4, 3), "grid and mark bubbles");
    for (c, r) in &marks {
        for (g, rg) in &grids {
            assert!((*c - *g).len() >= r + rg, "a mark bubble at {c:?} over a grid bubble at {g:?}");
        }
    }
}

/// GA P1-4: on A3 a large ground floor and a small upper floor print at
/// one scale, the one the larger needs, not 1:100 and 1:75.
#[test]
fn every_floor_plan_of_a_set_prints_at_one_scale() {
    use guhit_export::sheetset::{self, PrintedScale};
    let mut p = shell();
    p.levels = vec![
        Level { id: GROUND.into(), name: "Ground Floor".into(), elevation_mm: 0.0, height_mm: 3000.0 },
        Level { id: SECOND.into(), name: "Second Floor".into(), elevation_mm: 3000.0, height_mm: 3000.0 },
    ];
    box_walls(&mut p, GROUND, "g", (0.0, 0.0, 24000.0, 16000.0));
    box_walls(&mut p, SECOND, "s", (0.0, 0.0, 9000.0, 8000.0));
    p.sheet_set.sheets = vec![
        sheet("a2", "A-2", "Ground floor plan", PaperSize::A3, vec![ViewRef::Plan { level_id: GROUND.into(), sheet: SheetKind::Plan }]),
        sheet("a3", "A-3", "Second floor plan", PaperSize::A3, vec![ViewRef::Plan { level_id: SECOND.into(), sheet: SheetKind::Plan }]),
    ];
    let d = guhit_core::compute_derived(&p);
    // Alone, each sheet picks its own.
    let alone: Vec<Option<u32>> = p.sheet_set.sheets.iter().map(|s| sheetset::viewport_boxes(&p, &d, s, &Default::default()).1[0].scale).collect();
    assert_ne!(alone[0], alone[1], "the floors would print at {alone:?}");
    let printed = sheetset::printed_sheets(&p, &d, &[], &Default::default()).unwrap();
    let scales: Vec<PrintedScale> = printed.iter().map(|s| s.viewports[0].scale).collect();
    assert_eq!(scales[0], scales[1], "one plan scale for the set");
    assert_eq!(scales[0], PrintedScale::Scaled(alone.iter().flatten().copied().max().unwrap()));
    assert!(printed.iter().all(|s| s.fits));
}

fn room(id: &str, level: &str, name: &str, seed: (f64, f64)) -> Element {
    Element::Room(Room {
        id: id.into(),
        level_id: level.into(),
        name: name.into(),
        usage: RoomUsage::Living,
        seed: pt(seed.0, seed.1),
        floor_material_id: None,
        auto_named: false,
        finishes: RoomFinishes::default(),
        ceiling_height_mm: None,
    })
}

/// GA P1-3: sections name the rooms they cut and draw each ceiling at its
/// height; elevations note the wall and roof finishes from their
/// materials; and the house stands on the finish grade, `grade_mm` below
/// the lowest floor, instead of floating over it.
#[test]
fn sections_name_rooms_and_ceilings_elevations_note_finishes_on_grade() {
    let (mut p, _) = two_storey(RoofShape::Hip);
    for e in p.elements.iter_mut() {
        match e {
            Element::Wall(w) => w.material_id = Some("mat-chb-painted".into()),
            Element::Roof(r) => r.material_id = Some("mat-roof-longspan".into()),
            _ => {}
        }
    }
    p.elements.push(room("r-living", GROUND, "Living", (5500.0, 4000.0)));
    p.elements.push(room("r-bed", SECOND, "Bedroom", (4500.0, 4000.0)));
    p.elements.push(Element::Lot(Lot {
        id: "lot".into(),
        corners: vec![pt(-3000.0, -4000.0), pt(13000.0, -4000.0), pt(13000.0, 12000.0), pt(-3000.0, 12000.0)],
        sides: (0..4).map(|_| LotSide { role: LotSideRole::Side, setback_mm: 1500.0, road: String::new(), road_width_mm: None }).collect(),
        grade_mm: 300.0,
    }));
    let d = guhit_core::compute_derived(&p);
    let ch = |id: &str| d.room_schedule.iter().find(|r| r.room_id == id).map(|r| r.ceiling_height_mm).unwrap();

    // Section B runs west to east through both rooms.
    let sec = views::items(&p, &d, &view_of("sec-b", MarkKind::Section), Some(100)).unwrap();
    let t = texts(&sec.items);
    for want in ["LIVING", "BEDROOM", &format!("CH {:.3}", ch("r-living") / 1000.0), &format!("CH {:.3}", ch("r-bed") / 1000.0)] {
        assert!(t.iter().any(|x| x == want), "{want} in {t:?}");
    }
    for (id, e) in [("r-living", 0.0), ("r-bed", 3000.0)] {
        let y = e + ch(id);
        let ceiling = sec.items.iter().any(|i| i.cat == Cat::Slab && matches!(i.prim, Prim::Line { a, b } if (a.y - y).abs() < 0.5 && (b.y - y).abs() < 0.5 && (b.x - a.x).abs() > 1000.0));
        assert!(ceiling, "{id}: a ceiling line at {y}");
    }

    // The south elevation notes both finishes, each with a leader.
    let south = views::items(&p, &d, &view_of("south", MarkKind::Elevation), Some(100)).unwrap();
    let t = texts(&south.items);
    for want in ["CHB, PLASTERED AND PAINTED", "LONG-SPAN PRE-PAINTED METAL ROOF"] {
        assert_eq!(t.iter().filter(|x| *x == want).count(), 1, "{want} in {t:?}");
    }

    for (name, view) in [("south", view_of("south", MarkKind::Elevation)), ("sec-b", view_of("sec-b", MarkKind::Section))] {
        if let Ok(out) = guhit_export::view_svg(&p, &d, &view, Some(100), &Default::default()) {
            dump(&format!("p13-{name}"), &out.data);
        }
    }

    // On grade: the lowest line of the building is the grade line, -0.300.
    for view in [view_of("south", MarkKind::Elevation), view_of("sec-b", MarkKind::Section)] {
        let v_items = views::items(&p, &d, &view, Some(100)).unwrap();
        let grade = v_items
            .items
            .iter()
            .filter(|i| i.cat == Cat::Site)
            .filter_map(|i| match i.prim {
                Prim::Line { a, .. } => Some(a.y),
                _ => None,
            })
            .fold(f64::INFINITY, f64::min);
        assert!((grade + 300.0).abs() < 0.5, "the grade at -300: {grade}");
        let low = v_items
            .items
            .iter()
            .filter(|i| matches!(i.cat, Cat::Wall | Cat::Slab))
            .flat_map(|i| match &i.prim {
                Prim::Line { a, b } => vec![a.y, b.y],
                Prim::Poly { pts, .. } => pts.iter().map(|q| q.y).collect(),
                _ => vec![],
            })
            .fold(f64::INFINITY, f64::min);
        assert!((low - grade).abs() < 0.5, "{view:?}: the building reaches the grade: lowest {low}, grade {grade}");
    }
}

fn asset(id: &str, key: &str, name: &str, at: (f64, f64), w: f64, d: f64, rot: f64) -> Element {
    let mut v = serde_json::json!({
        "kind": "asset", "id": id, "level_id": GROUND, "catalog_key": key, "name": name, "category": "furniture",
        "position": {"x": at.0, "y": at.1}, "rotation_deg": rot, "width_mm": w, "depth_mm": d, "height_mm": 800.0,
        "elevation_mm": 0.0, "material_id": null
    });
    if key == "car-sedan" {
        v["category"] = "vehicle".into();
    }
    serde_json::from_value(v).expect("an asset")
}

/// GA P1-5: sheets and the plan DXF print each catalog item's own symbol,
/// the one the screen draws, not a labelled box; an item with no symbol
/// keeps its box and name. In the DXF the symbol is the item's block.
#[test]
fn sheets_and_the_dxf_print_furniture_symbols() {
    let mut p = shell();
    p.levels = vec![Level { id: GROUND.into(), name: "Ground Floor".into(), elevation_mm: 0.0, height_mm: 3000.0 }];
    box_walls(&mut p, GROUND, "g", (0.0, 0.0, 12000.0, 9000.0));
    let items = [
        ("bed", "bed-double", "Double bed", (2000.0, 2000.0), 1600.0, 2000.0, 0.0),
        ("sofa", "sofa-3", "Sofa, 3 seats", (6000.0, 2000.0), 2100.0, 900.0, 90.0),
        ("dining", "dining-set-6", "Dining set, 6 seats", (9500.0, 6500.0), 1800.0, 1600.0, 0.0),
        ("wc", "wc", "Water closet", (1000.0, 8000.0), 400.0, 700.0, 180.0),
        ("kitchen", "kitchen-run", "Kitchen run", (6000.0, 8500.0), 2400.0, 600.0, 180.0),
        ("car", "car-sedan", "Car, sedan", (9500.0, 2500.0), 1800.0, 4500.0, 0.0),
        ("odd", "mystery-box", "Mystery box", (4000.0, 5000.0), 2000.0, 1200.0, 0.0),
    ];
    for (id, key, name, at, w, d, rot) in items {
        p.elements.push(asset(id, key, name, at, w, d, rot));
    }
    let d = guhit_core::compute_derived(&p);
    let plan = guhit_export::plan::build_items(
        &p,
        &d,
        &p.levels[0],
        &guhit_export::plan::PlanOptions { scale: 50.0, show_dimensions: false, show_room_labels: true, show_assets: true, unicode: true, skip_devices: false },
    );
    if let Ok(out) = guhit_export::view_svg(&p, &d, &ViewRef::Plan { level_id: GROUND.into(), sheet: SheetKind::Plan }, Some(50), &Default::default()) {
        dump("symbols-plan", &out.data);
    }
    let t = texts(&plan);
    for (_, key, name, ..) in items {
        let labelled = t.iter().any(|x| x == name);
        assert_eq!(labelled, key == "mystery-box", "{key}: a symbol, not a labelled box ({t:?})");
    }
    // A bed is more than its outline: pillows, the blanket's edge.
    let furn = plan.iter().filter(|i| i.cat == Cat::Furn).count();
    assert!(furn > 7 * 4, "{furn} furniture items");

    let dxf_text = guhit_export::plan_dxf(&p, &d, &options(PaperSize::A3, Orientation::Landscape)).unwrap().data;
    let drawing = dxf::Drawing::load(&mut dxf_text.as_bytes()).unwrap();
    let block = |base: &str| drawing.blocks().find(|b| b.name.to_uppercase().starts_with(&base.to_uppercase())).map(|b| b.entities.len());
    assert!(block("BED-DOUBLE").is_some_and(|n| n > 5), "the bed's block is its symbol: {:?}", block("BED-DOUBLE"));
    assert!(block("DINING-SET-6").is_some_and(|n| n > 10));
    assert_eq!(block("MYSTERY-BOX"), Some(2), "the box and its NAME attribute");
    let inserts = drawing.entities().filter(|e| e.common.layer == "A-FURN" && matches!(e.specific, dxf::entities::EntityType::Insert(_))).count();
    let loose: Vec<String> = drawing
        .entities()
        .filter(|e| e.common.layer == "A-FURN" && !matches!(e.specific, dxf::entities::EntityType::Insert(_) | dxf::entities::EntityType::Text(_)))
        .map(|e| format!("{:?}", e.specific).chars().take(160).collect())
        .collect();
    assert_eq!(inserts, 7, "every object one INSERT of its block");
    assert!(loose.is_empty(), "loose furniture entities: {loose:?}");
}

/// GA P0-1: a void drawn to the wall centerlines takes off only its clear
/// part. The IFC floor slab's net area is the level's gross floor area as
/// the engine gives it, and the drawings keep the slab under the walls.
#[test]
fn the_ifc_slab_counts_the_walls_under_a_void_as_the_schedule_does() {
    let (mut p, _) = two_storey(RoofShape::Hip);
    let void = Slab {
        id: "void".into(),
        level_id: SECOND.into(),
        kind: SlabKind::Void,
        polygon: vec![pt(0.0, 0.0), pt(4000.0, 0.0), pt(4000.0, 3000.0), pt(0.0, 3000.0)],
        thickness_mm: 150.0,
        top_offset_mm: 0.0,
        material_id: None,
        railing: None,
    };
    p.elements.push(Element::Slab(void));
    let d = guhit_core::compute_derived(&p);
    let gross = d.level_areas.iter().find(|l| l.level_id == SECOND).map(|l| l.gross_m2).unwrap();
    // 9.15 x 8.15 less the clear void, 3.925 x 2.925.
    assert!((gross - (74.5725 - 3.925 * 2.925)).abs() < 0.01, "the engine's gross area: {gross}");
    let ifc = guhit_export::model_ifc(&p, &d).unwrap();
    let nets: Vec<f64> = ifc
        .lines()
        .filter_map(|l| l.split_once("IFCQUANTITYAREA('NetArea',$,$,"))
        .filter_map(|(_, rest)| rest.split(',').next()?.parse::<f64>().ok())
        .collect();
    assert!(nets.iter().any(|n| (n - gross).abs() < 0.01), "an IFC slab of net area {gross}: {nets:?}");
    // The section through the void keeps the slab under the west wall.
    p.elements.push(mark("sec-v", GROUND, MarkKind::Section, "V", (2000.0, 10000.0), (2000.0, -2000.0)));
    let v_items = views::items(&p, &d, &view_of("sec-v", MarkKind::Section), Some(50)).unwrap();
    // Looking west at x 2000 the cut runs north to south; the second floor
    // slab under the south wall (y -75..75) stays.
    let s_of = |y: f64| 10000.0 - y;
    let under_wall = [v(s_of(75.0) + 5.0, 2860.0), v(s_of(-75.0) - 5.0, 2860.0), v(s_of(-75.0) - 5.0, 2990.0), v(s_of(75.0) + 5.0, 2990.0)];
    let kept: f64 = poche(&v_items.items).iter().map(|(_, poly)| guhit_export::geom::overlap_area(&ensure(poly), &ensure(&under_wall))).sum();
    assert!(kept > 100.0 * 130.0 * 0.9, "the slab under the wall stays: {kept} mm2");
}

/// With `$GUHIT_FIX_STATE` (a saved doc state) and `$GUHIT_FIX_OUT` set:
/// that project's whole set as PNGs, its sheets DXF and what the set
/// leaves out, for a look. Does nothing otherwise.
#[test]
fn dump_a_saved_set() {
    let (Ok(path), Ok(_)) = (std::env::var("GUHIT_FIX_STATE"), std::env::var("GUHIT_FIX_OUT")) else { return };
    let state: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let p: Project = serde_json::from_value(state["project"].clone()).unwrap();
    let d = guhit_core::compute_derived(&p);
    let inputs = guhit_export::SheetInputs::default();
    for (number, svg) in guhit_export::sheetset::sheets_svg(&p, &d, &[], &inputs).unwrap() {
        dump(&format!("set-{number}"), &svg);
    }
    let dir = std::path::PathBuf::from(std::env::var("GUHIT_FIX_OUT").unwrap());
    std::fs::write(dir.join("set.dxf"), guhit_export::sheets_dxf(&p, &d, &[]).unwrap()).unwrap();
    std::fs::write(dir.join("left-out.txt"), guhit_export::sheetset::left_out(&p, &d, &[], &inputs).unwrap().join("\n")).unwrap();
    let ifc = guhit_export::model_ifc(&p, &d).unwrap();
    let gross: Vec<String> = d.level_areas.iter().map(|l| format!("{} gross {}", l.level_id, l.gross_m2)).collect();
    let nets = ifc_quantities(&ifc, "IFCQUANTITYAREA", "NetArea");
    std::fs::write(dir.join("areas.txt"), format!("{}\nIFC slab NetArea: {nets:?}\n", gross.join("\n"))).unwrap();
}

/// Every value of one IFC quantity, by name.
fn ifc_quantities(ifc: &str, kind: &str, name: &str) -> Vec<f64> {
    let key = format!("{kind}('{name}',$,$,");
    ifc.lines()
        .filter_map(|l| l.split_once(key.as_str()))
        .filter_map(|(_, rest)| rest.split(',').next()?.parse::<f64>().ok())
        .collect()
}

/// GR1 #12 and GS P0 (F1's notes): the room schedule prints the net and
/// the floor area, each column adding up to its total, the floor areas to
/// `Totals`; the IFC space's net area is the floor area, and a room under a
/// void is a space as tall as its double height ceiling.
#[test]
fn room_areas_add_up_and_a_double_height_space_is_that_tall() {
    let (mut p, _) = two_storey(RoofShape::Hip);
    wall(&mut p, GROUND, "g-mid", (2500.0, 4000.0), (9000.0, 4000.0));
    p.elements.push(room("r-living", GROUND, "Living", (5500.0, 2000.0)));
    p.elements.push(room("r-dining", GROUND, "Dining", (5500.0, 6000.0)));
    p.elements.push(room("r-gallery", SECOND, "Gallery", (5000.0, 6000.0)));
    p.elements.push(Element::Slab(Slab {
        id: "void".into(),
        level_id: SECOND.into(),
        kind: SlabKind::Void,
        polygon: vec![pt(2500.0, 0.0), pt(9000.0, 0.0), pt(9000.0, 4000.0), pt(2500.0, 4000.0)],
        thickness_mm: 150.0,
        top_offset_mm: 0.0,
        material_id: None,
        railing: None,
    }));
    let d = guhit_core::compute_derived(&p);
    let row = |id: &str| d.room_schedule.iter().find(|r| r.room_id == id).unwrap().clone();
    let living = row("r-living");
    assert!(living.ceiling_height_mm > 5000.0, "the living is double height: {}", living.ceiling_height_mm);

    let t = guhit_export::tables::schedule(&p, &d, ScheduleKind::Rooms);
    let col = |head: &str| t.columns.iter().position(|c| c.head.starts_with(head)).unwrap();
    let (net, floor) = (col("NET AREA"), col("FLOOR AREA"));
    let sum = |c: usize| t.rows.iter().map(|r| r[c].parse::<f64>().unwrap()).sum::<f64>();
    let total = t.total.as_ref().unwrap();
    for c in [net, floor] {
        assert!((sum(c) - total[c].parse::<f64>().unwrap()).abs() < 0.015, "{} adds up: {} vs {}", t.columns[c].head, sum(c), total[c]);
    }
    assert!((total[floor].parse::<f64>().unwrap() - d.totals.floor_area_m2).abs() < 0.01, "the floor areas total is Totals'");
    // The gallery has a void in it: its floor area is less than its net.
    let g = row("r-gallery");
    assert!(g.net_area_m2 > g.floor_area_m2 - 1e-9);

    let ifc = guhit_export::model_ifc(&p, &d).unwrap();
    let nets = ifc_quantities(&ifc, "IFCQUANTITYAREA", "NetFloorArea");
    for r in &d.room_schedule {
        assert!(nets.iter().any(|n| (n - r.floor_area_m2).abs() < 0.01), "{}: IFC net floor area {} in {nets:?}", r.name, r.floor_area_m2);
    }
    let heights = ifc_quantities(&ifc, "IFCQUANTITYLENGTH", "Height");
    assert!(heights.iter().any(|h| (h - living.ceiling_height_mm).abs() < 0.5), "the living space is {} tall: {heights:?}", living.ceiling_height_mm);
}

/// The orchestrator's look at the GS set: a plan note prints at plan text
/// size and clear of what it would cover; a section's plan bubble gives
/// where it is drawn ("1" over "A-6"), never a grid's letter; each room in
/// a section gets one ceiling note inside its own stretch, clear of the
/// others; and a room's name sits on its floor, never over its void.
#[test]
fn notes_section_bubbles_and_room_names_keep_clear() {
    let (mut p, _) = two_storey(RoofShape::Hip);
    wall(&mut p, GROUND, "g-mid", (2500.0, 5000.0), (9000.0, 5000.0));
    p.elements.push(room("r-living", GROUND, "Living", (5500.0, 2500.0)));
    p.elements.push(room("r-gallery", SECOND, "Gallery", (1200.0, 6000.0)));
    p.elements.push(Element::Slab(Slab {
        id: "void".into(),
        level_id: SECOND.into(),
        kind: SlabKind::Void,
        polygon: vec![pt(2500.0, 0.0), pt(9000.0, 0.0), pt(9000.0, 5000.0), pt(2500.0, 5000.0)],
        thickness_mm: 150.0,
        top_offset_mm: 0.0,
        material_id: None,
        railing: None,
    }));
    // A note put down over a car, at 250 model mm: 2.5 mm at the project's
    // 1:100.
    p.elements.push(asset("car", "car-sedan", "Car, sedan", (5500.0, 2500.0), 1800.0, 4500.0, 0.0));
    let note: Element = serde_json::from_value(serde_json::json!({
        "kind": "annotation", "id": "note", "level_id": GROUND, "position": {"x": 5000.0, "y": 2500.0},
        "text": "FINISH GRADE +0.000, FFL +0.300", "size_mm": 250.0, "rotation_deg": 0.0
    }))
    .unwrap();
    p.elements.push(note);
    p.sheet_set.sheets = vec![sheet("s6", "A-6", "Sections", PaperSize::A1, vec![view_of("sec-a", MarkKind::Section), view_of("sec-b", MarkKind::Section)])];
    let d = guhit_core::compute_derived(&p);
    let n = 50.0;
    let plan = |level: &str| {
        let l = p.levels.iter().find(|l| l.id == level).unwrap();
        guhit_export::plan::build_items(&p, &d, l, &guhit_export::plan::PlanOptions { scale: n, show_dimensions: true, show_room_labels: true, show_assets: true, unicode: true, skip_devices: false })
    };
    let ground = plan(GROUND);
    let text_at = |items: &[Item], s: &str| -> Vec<(V, f64)> {
        items
            .iter()
            .filter_map(|i| match &i.prim {
                Prim::Text { pos, text, height, .. } if text == s => Some((*pos, *height)),
                _ => None,
            })
            .collect()
    };
    // 1. The note: plan text size at 1:50, off the car.
    let found = text_at(&ground, "FINISH GRADE +0.000, FFL +0.300");
    assert_eq!(found.len(), 1);
    let (at, height) = found[0];
    assert!((height - 2.5 * n).abs() < 1e-6, "2.5 mm on paper at 1:50: {height}");
    let w = guhit_export::text::est_width("FINISH GRADE +0.000, FFL +0.300", height, false);
    let note_box = [v(at.x, at.y - 0.3 * height), v(at.x + w, at.y - 0.3 * height), v(at.x + w, at.y + 0.9 * height), v(at.x, at.y + 0.9 * height)];
    let car = guhit_export::geom::bounds_of(&[v(4600.0, 250.0), v(6400.0, 4750.0)]);
    assert!(!guhit_export::geom::bounds_of(&note_box).intersects(&car), "the note at {at:?} is off the car");

    // 2. The section's bubble on the plan: "1" over "A-6", no lone "A".
    let symb: Vec<String> = ground
        .iter()
        .filter(|i| i.cat == Cat::Symb)
        .filter_map(|i| match &i.prim {
            Prim::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(symb.iter().filter(|t| *t == "A-6").count(), 4, "both ends of both sections name their sheet: {symb:?}");
    assert!(!symb.iter().any(|t| t == "A" || t == "B"), "no section reads as a grid: {symb:?}");

    // 4. The gallery's name on its floor, never over the void.
    let upper = plan(SECOND);
    let void = [v(2500.0, 0.0), v(9000.0, 0.0), v(9000.0, 5000.0), v(2500.0, 5000.0)];
    for (at, _) in text_at(&upper, "Gallery") {
        assert!(!guhit_export::geom::point_in_polygon(at, &void), "the gallery's name at {at:?} is over its void");
    }

    // 3 and 4 in section B (west to east at y 4300, through the void):
    // one ceiling note per room, each inside its own stretch, none on
    // another; the gallery's name over its floor (x < 2500).
    let sec = views::items(&p, &d, &view_of("sec-b", MarkKind::Section), Some(50)).unwrap();
    let notes: Vec<(V, f64, String)> = sec
        .items
        .iter()
        .filter_map(|i| match &i.prim {
            Prim::Text { pos, height, text, .. } if text.starts_with("CH ") => Some((*pos, *height, text.clone())),
            _ => None,
        })
        .collect();
    assert!(notes.len() >= 2, "{notes:?}");
    let boxes: Vec<guhit_export::geom::Bounds> = notes
        .iter()
        .map(|(at, h, t)| guhit_export::geom::bounds_of(&[*at, *at + v(guhit_export::text::est_width(t, *h, false), *h)]))
        .collect();
    for i in 0..boxes.len() {
        for j in i + 1..boxes.len() {
            assert!(!boxes[i].intersects(&boxes[j]), "{} over {}", notes[i].2, notes[j].2);
        }
    }
    // The section's s runs from the mark's start at x -2000.
    let gallery = text_at(&sec.items, "GALLERY");
    assert_eq!(gallery.len(), 1);
    assert!(gallery[0].0.x + (-2000.0) < 2500.0, "the gallery's name over its floor: s {}", gallery[0].0.x);
}

/// The plumbing demo's project, derived here.
fn plumbing_demo() -> (Project, Derived) {
    let path = workspace_root().join("fixtures/plumbing-demo.docstate.json");
    let state: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let p: Project = serde_json::from_value(state["project"].clone()).unwrap();
    let d = guhit_core::compute_derived(&p);
    (p, d)
}

/// GA P0-2, P1-1 and P1-2: a sheet never prints error or instruction
/// text. Isometric viewports, one per level, are the one isometric sheet;
/// with no runs they are left out and the export says so. Plumbing sheets
/// carry a blank Master Plumber block, never the architect's numbers. The
/// vicinity frame without its image prints empty; only the panel preview
/// says how to place one.
#[test]
fn sheets_print_no_error_or_instruction_text() {
    use guhit_export::sheetset;
    let inputs = guhit_export::SheetInputs::default();
    let iso = |level: &str| ViewRef::Plan { level_id: level.into(), sheet: SheetKind::PlumbingIsometric };
    let plumbing = |level: &str| ViewRef::Plan { level_id: level.into(), sheet: SheetKind::Plumbing };
    for with_pipes in [false, true] {
        let (mut p, d0) = if with_pipes { plumbing_demo() } else { two_storey(RoofShape::Hip) };
        let levels: Vec<String> = p.levels.iter().map(|l| l.id.clone()).collect();
        p.title_block.signatory.name = "Juan Dela Cruz (sample)".into();
        p.title_block.signatory.prc_no = "0012345".into();
        p.sheet_set.sheets = vec![
            sheet("s1", "A-1", "Site and vicinity map", PaperSize::A1, vec![ViewRef::VicinityMap { file_name: None, radius_m: 500.0 }]),
            sheet("p1", "P-1", "Plumbing layout", PaperSize::A1, levels.iter().map(|l| plumbing(l)).collect()),
            sheet("p2", "P-2", "Plumbing isometric diagrams", PaperSize::A1, levels.iter().map(|l| iso(l)).collect()),
        ];
        let d = if with_pipes { d0 } else { guhit_core::compute_derived(&p) };
        let svgs = sheetset::sheets_svg(&p, &d, &[], &inputs).unwrap();
        for (number, svg) in &svgs {
            let low = svg.to_lowercase();
            for bad in ["export failed", "not a plan view", "nothing to export", "place a vicinity map image", "is missing from"] {
                assert!(!low.contains(bad), "{number} prints \"{bad}\" (pipes: {with_pipes})");
            }
            dump(&format!("ga-{with_pipes}-{number}"), svg);
        }
        let get = |n: &str| svgs.iter().find(|(x, _)| x == n).map(|(_, s)| s.as_str()).unwrap();
        // The architect signs A-1; the plumbing sheets get a blank Master
        // Plumber block and never the architect's name or numbers.
        assert!(get("A-1").contains(">ARCHITECT<") && get("A-1").contains("0012345"));
        for n in ["P-1", "P-2"] {
            let s = get(n);
            assert!(s.contains(">MASTER PLUMBER<"), "{n} has the Master Plumber's block");
            assert!(!s.contains(">ARCHITECT<") && !s.contains("0012345") && !s.contains("Juan Dela Cruz"), "{n} shows no architect");
            assert_eq!(s.matches(">MASTER PLUMBER<").count(), 1, "{n}: one signing block");
        }
        let left = sheetset::left_out(&p, &d, &[], &inputs).unwrap();
        if with_pipes {
            assert!(get("P-2").contains("iso-clip-0"), "the diagrams are drawn");
            assert!(left.iter().all(|l| !l.starts_with("P-2")), "{left:?}");
        } else {
            assert!(left.iter().any(|l| l.starts_with("P-2") && l.contains("no water, drainage or vent runs")), "the export says what it left out: {left:?}");
        }
        // The panel's preview keeps the hint.
        let preview = sheetset::sheet_preview_svg(&p, &d, "s1", &inputs).unwrap();
        assert!(preview.contains("Place a vicinity map image in the Sheets panel."));
    }
}

/// With `$GUHIT_FIX_OUT` set, an SVG and a PNG of a sheet or view there,
/// for a look.
fn dump(name: &str, svg: &str) {
    let Ok(dir) = std::env::var("GUHIT_FIX_OUT") else { return };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.svg")), svg).unwrap();
    let mut db = svg2pdf::usvg::fontdb::Database::new();
    db.load_system_fonts();
    let options = svg2pdf::usvg::Options { fontdb: std::sync::Arc::new(db), ..Default::default() };
    let tree = svg2pdf::usvg::Tree::from_str(svg, &options).unwrap();
    let scale = 6.0 * 25.4 / 96.0;
    let size = tree.size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32).unwrap();
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.save_png(dir.join(format!("{name}.png"))).unwrap();
}

fn ensure(poly: &[V]) -> Vec<V> {
    guhit_export::geom::ensure_ccw(poly)
}

