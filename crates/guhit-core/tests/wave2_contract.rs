//! The Wave 2 contract (DECISIONS D53, D54; plan 1.7 and the C0 amendments):
//! the schema 6 migration, the roof over the top floor, elements on no
//! level, ids, the lot, roofs, stairs, the title block, viewports and
//! automatic dimensions. Each lane's own behavior is tested in its lane.

mod common;

use common::*;
use guhit_core::{migrate, scope, CoreError, Document};
use guhit_model::*;

fn data(name: &str) -> Project {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name);
    serde_json::from_str(&std::fs::read_to_string(path).expect("test data exists")).expect("a schema 5 file reads")
}

fn roof_areas(project: &Project) -> Vec<&RoofArea> {
    project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Roof(r) => Some(r),
            _ => None,
        })
        .collect()
}

fn add(el: Element) -> Command {
    Command::AddElement { element: el }
}

/// A 6 x 4 m house on the first level with a gable roof, migrated.
fn house_with_roof() -> Document {
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 6000.0, 4000.0, Some("Living")));
    let roof = Roof { kind: RoofKind::Gable, pitch_deg: 30.0, ..doc.project().top_roof() };
    apply_checked(&mut doc, Command::SetRoof { roof });
    doc
}

fn grid(id: &str, label: &str) -> Element {
    Element::Grid(GridLine { id: id.into(), label: label.into(), start: p(0.0, -1000.0), end: p(0.0, 5000.0) })
}

fn lot(id: &str) -> Element {
    let side = LotSide { role: LotSideRole::Side, setback_mm: 2000.0, road: String::new(), road_width_mm: None };
    Element::Lot(Lot {
        id: id.into(),
        corners: vec![p(-3000.0, -3000.0), p(12000.0, -3000.0), p(12000.0, 12000.0), p(-3000.0, 12000.0)],
        sides: vec![side.clone(), side.clone(), side.clone(), side],
        grade_mm: 150.0,
    })
}

fn polygon_roof(id: &str, shape: RoofShape, points: Vec<Point>) -> Element {
    Element::Roof(RoofArea {
        id: id.into(),
        level_id: Some(LEVEL.into()),
        outline: RoofOutline::Polygon { points },
        shape,
        pitch_deg: 25.0,
        ridge_axis: Axis::X,
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

fn rect(x: f64, y: f64, w: f64, d: f64) -> Vec<Point> {
    vec![p(x, y), p(x + w, y), p(x + w, y + d), p(x, y + d)]
}

/// An L: 8 x 6 m with a 4 x 3 m bite out of the north east.
fn l_shape() -> Vec<Point> {
    vec![p(0.0, 0.0), p(8000.0, 0.0), p(8000.0, 3000.0), p(4000.0, 3000.0), p(4000.0, 6000.0), p(0.0, 6000.0)]
}

// ------------------------------------------------------------- migration

#[test]
fn schema_5_files_migrate_to_one_roof_over_the_top_floor() {
    for name in ["schema5-sample-bungalow.project.json", "schema5-plumbing-demo.project.json", "wave1-g5-lovell.project.json"] {
        let old = data(name);
        assert_eq!(old.schema_version, 5, "{name}");
        assert_ne!(old.roof.kind, RoofKind::None, "{name} has a roof");
        let doc = Document::new(old.clone());
        let project = doc.project();
        assert_eq!(project.schema_version, SCHEMA_VERSION, "{name}");
        assert_eq!(project.roof.kind, RoofKind::None, "{name}: the legacy roof is retired");
        let areas = roof_areas(project);
        assert_eq!(areas.len(), 1, "{name}");
        let r = areas[0];
        assert_eq!(r.level_id, None, "{name}: the roof over the top floor has no level");
        assert_eq!(r.outline, RoofOutline::TopFootprint);
        assert_eq!(r.fascia, Some(Trim { size_mm: 200.0, material_id: None }));
        assert_eq!(r.gutter, None);
        // The old values, read back through `top_roof`.
        assert_eq!(project.top_roof(), old.roof, "{name}");
        assert!(project.layers.iter().any(|l| l.key == LayerKey::Grids));
        assert!(project.layers.iter().any(|l| l.key == LayerKey::Roofs));
        // The same file migrates to the same id, migrating twice changes
        // nothing, and save and reload gives the same project.
        assert_eq!(Document::new(old.clone()).project(), project, "{name}");
        let mut twice = project.clone();
        migrate(&mut twice);
        assert_eq!(&twice, project, "{name}");
        let json = serde_json::to_string(project).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, project, "{name}");
        assert_eq!(serde_json::to_string(&back).unwrap(), json, "{name}");
        // Derived data still works on the migrated project.
        assert!(!doc.derived().walls.is_empty(), "{name}");
    }
}

#[test]
fn a_project_without_a_roof_gets_no_roof_element() {
    let doc = blank();
    assert!(roof_areas(doc.project()).is_empty());
    assert_eq!(doc.project().top_roof().kind, RoofKind::None);
}

// --------------------------------------------------- SetRoof (P0-2)

#[test]
fn set_roof_edits_the_roof_over_the_top_floor_and_never_deletes_it_by_accident() {
    let mut doc = house_with_roof();
    let id = roof_areas(doc.project())[0].id.clone();
    // A partial edit built from `top_roof`, as the copilot's set_roof and
    // MCP's set_project do: pitch 25 changes the pitch and keeps the roof.
    let roof = Roof { pitch_deg: 25.0, ..doc.project().top_roof() };
    let r = apply_checked(&mut doc, Command::SetRoof { roof });
    let areas = roof_areas(&r.state.project);
    assert_eq!(areas.len(), 1);
    assert_eq!((areas[0].id.as_str(), areas[0].pitch_deg, areas[0].shape), (id.as_str(), 25.0, RoofShape::Gable));
    // What `Roof` cannot say is kept.
    assert_eq!(areas[0].fascia, Some(Trim { size_mm: 200.0, material_id: None }));
    // Kind none removes it; the values stay on offer.
    let none = Roof { kind: RoofKind::None, ..doc.project().top_roof() };
    let r = apply_checked(&mut doc, Command::SetRoof { roof: none });
    assert!(roof_areas(&r.state.project).is_empty());
    assert_eq!(r.state.project.top_roof().pitch_deg, 25.0);
    assert_eq!(r.diff.summary, "Remove roof: removed 1 roof");
    // And back, as a hip.
    let hip = Roof { kind: RoofKind::Hip, ..doc.project().top_roof() };
    let r = apply_checked(&mut doc, Command::SetRoof { roof: hip });
    assert_eq!(roof_areas(&r.state.project)[0].shape, RoofShape::Hip);
    assert_eq!(r.state.project.top_roof().kind, RoofKind::Hip);
}

#[test]
fn there_is_one_roof_over_the_top_floor() {
    let mut doc = house_with_roof();
    let mut second = match polygon_roof("", RoofShape::Flat, rect(0.0, 0.0, 3000.0, 3000.0)) {
        Element::Roof(r) => r,
        _ => unreachable!(),
    };
    second.outline = RoofOutline::TopFootprint;
    second.level_id = None;
    assert_eq!(code(&apply_rejected(&mut doc, add(Element::Roof(second.clone())))), "roof_exists");
    second.level_id = Some(LEVEL.into());
    assert_eq!(code(&apply_rejected(&mut doc, add(Element::Roof(second)))), "roof_level");
}

// ------------------------------------------- drawn roof outlines (P0-4, P1-9)

#[test]
fn pitched_roofs_on_drawn_outlines_need_square_corners() {
    let mut doc = blank();
    // A gable takes a rectangle, not an L; a hip takes both; flat takes any.
    apply_checked(&mut doc, add(polygon_roof("", RoofShape::Gable, rect(0.0, 0.0, 6000.0, 4000.0))));
    let e = apply_rejected(&mut doc, add(polygon_roof("", RoofShape::Gable, l_shape())));
    assert_eq!(code(&e), "roof_outline");
    assert!(e.to_string().contains("rectangular"), "{e}");
    apply_checked(&mut doc, add(polygon_roof("", RoofShape::Hip, l_shape())));
    let skew = vec![p(0.0, 0.0), p(6000.0, 0.0), p(6500.0, 4000.0), p(0.0, 4000.0)];
    let e = apply_rejected(&mut doc, add(polygon_roof("", RoofShape::Hip, skew.clone())));
    assert_eq!(code(&e), "roof_outline");
    assert!(e.to_string().contains("degrees"), "{e}");
    apply_checked(&mut doc, add(polygon_roof("", RoofShape::Flat, skew)));
}

#[test]
fn gable_sides_belong_to_hip_roofs_over_rectangles() {
    let mut doc = blank();
    let with_sides = |shape: RoofShape, points: Vec<Point>, sides: Vec<u32>| match polygon_roof("", shape, points) {
        Element::Roof(mut r) => {
            r.gable_sides = sides;
            Element::Roof(r)
        }
        _ => unreachable!(),
    };
    apply_checked(&mut doc, add(with_sides(RoofShape::Hip, rect(0.0, 0.0, 6000.0, 4000.0), vec![1, 3])));
    let e = apply_rejected(&mut doc, add(with_sides(RoofShape::Hip, l_shape(), vec![0])));
    assert_eq!(code(&e), "roof_gable_sides");
    assert!(e.to_string().contains("not a rectangle"), "{e}");
    let e = apply_rejected(&mut doc, add(with_sides(RoofShape::Gable, rect(0.0, 0.0, 6000.0, 4000.0), vec![0])));
    assert_eq!(code(&e), "roof_gable_sides");
    let e = apply_rejected(&mut doc, add(with_sides(RoofShape::Hip, rect(0.0, 0.0, 6000.0, 4000.0), vec![4])));
    assert_eq!(code(&e), "roof_gable_sides");
}

// ------------------------------------------ elements on no level (P0-3)

#[test]
fn deleting_a_level_never_deletes_what_is_on_no_level() {
    let mut doc = house_with_roof();
    apply_checked(&mut doc, Command::AddLevel { name: Some("Second floor".into()), elevation_mm: None, height_mm: None });
    let upper = doc.project().levels[1].id.clone();
    apply_checked(&mut doc, add(grid("", "A")));
    apply_checked(&mut doc, add(lot("")));
    let mut mark = ViewMark {
        id: String::new(),
        level_id: LEVEL.into(),
        mark_kind: MarkKind::Section,
        label: "A".into(),
        start: p(-1000.0, 2000.0),
        end: p(7000.0, 2000.0),
        flip: false,
        depth_mm: None,
    };
    apply_checked(&mut doc, add(Element::ViewMark(mark.clone())));
    mark.level_id = upper.clone();
    mark.label = "B".into();
    apply_checked(&mut doc, add(Element::ViewMark(mark)));
    apply_checked(&mut doc, add(match polygon_roof("", RoofShape::Shed, rect(6000.0, 0.0, 3000.0, 3000.0)) {
        Element::Roof(mut r) => {
            r.level_id = Some(upper.clone());
            Element::Roof(r)
        }
        _ => unreachable!(),
    }));
    let kinds = |p: &Project| {
        let mut k: Vec<ElementKind> = p.elements.iter().map(|e| e.kind()).collect();
        k.sort_by_key(|k| format!("{k:?}"));
        k.dedup();
        k
    };
    let r = apply_checked(&mut doc, Command::DeleteLevel { level_id: upper });
    let project = &r.state.project;
    // The roof over the top floor, the grid and the lot stay; the upper
    // level's view mark and shed roof go.
    assert_eq!(roof_areas(project).len(), 1);
    assert_eq!(roof_areas(project)[0].outline, RoofOutline::TopFootprint);
    assert!(kinds(project).contains(&ElementKind::Grid));
    assert!(kinds(project).contains(&ElementKind::Lot));
    let marks: Vec<&Element> = project.elements.iter().filter(|e| e.kind() == ElementKind::ViewMark).collect();
    assert_eq!(marks.len(), 1);
    for el in &project.elements {
        if let Element::Grid(_) | Element::Lot(_) = el {
            assert_eq!(el.level_id(), None);
        }
    }
}

// ---------------------------------------------------------- ids (P0-5)

#[test]
fn identical_upsert_sheet_calls_make_two_sheets() {
    let mut doc = blank();
    let sheet = Sheet {
        id: String::new(),
        number: String::new(),
        title: "Floor plan".into(),
        paper: PaperSize::A1,
        orientation: Orientation::Landscape,
        viewports: vec![Viewport {
            id: String::new(),
            view: ViewRef::Plan { level_id: LEVEL.into(), sheet: SheetKind::Plan },
            scale_denominator: Some(100),
            position: None,
            title: None,
        }],
        revision_ids: vec![],
    };
    let cmd = Command::UpsertSheet { sheet: Box::new(sheet), index: None };
    apply_checked(&mut doc, cmd.clone());
    let r = apply_checked(&mut doc, cmd);
    let sheets = &r.state.project.sheet_set.sheets;
    assert_eq!(sheets.len(), 2);
    assert_ne!(sheets[0].id, sheets[1].id);
    assert_ne!(sheets[0].viewports[0].id, sheets[1].viewports[0].id);
    assert_eq!((sheets[0].number.as_str(), sheets[1].number.as_str()), ("A-1", "A-2"));
}

// ---------------------------------------------------------- lot (P1-7)

#[test]
fn a_project_has_one_lot() {
    let mut doc = blank();
    let r = apply_checked(&mut doc, add(lot("")));
    let first = r.state.project.elements.iter().find(|e| e.kind() == ElementKind::Lot).unwrap().id().clone();
    let e = apply_rejected(&mut doc, add(lot("")));
    assert_eq!(code(&e), "lot_exists");
    assert_eq!(error_ids(&e), vec![first]);
}

// -------------------------------------------------------- stairs (P2)

#[test]
fn an_l_or_u_stair_needs_four_risers() {
    let mut doc = blank();
    let stair = |shape: StairShape, risers: u32, first: Option<u32>| {
        add(Element::Stair(Stair {
            id: String::new(),
            level_id: LEVEL.into(),
            origin: p(1000.0, 1000.0),
            rotation_deg: 0.0,
            width_mm: 900.0,
            run_mm: 2500.0,
            riser_count: risers,
            railing: None,
            shape,
            turn: Turn::Right,
            first_flight_risers: first,
            landing_mm: None,
            well_mm: 0.0,
        }))
    };
    assert_eq!(code(&apply_rejected(&mut doc, stair(StairShape::L, 3, None))), "stair_size");
    assert_eq!(code(&apply_rejected(&mut doc, stair(StairShape::U, 16, Some(15)))), "stair_size");
    apply_checked(&mut doc, stair(StairShape::L, 4, None));
    apply_checked(&mut doc, stair(StairShape::U, 16, Some(8)));
    apply_checked(&mut doc, stair(StairShape::Straight, 3, None));
}

#[test]
fn new_stair_fields_stay_out_of_older_commands_json() {
    // Older commands' JSON, and so the ids they seed, stay the same.
    let s = Stair {
        id: "s".into(),
        level_id: LEVEL.into(),
        origin: p(0.0, 0.0),
        rotation_deg: 0.0,
        width_mm: 900.0,
        run_mm: 2500.0,
        riser_count: 15,
        railing: None,
        shape: StairShape::Straight,
        turn: Turn::Left,
        first_flight_risers: None,
        landing_mm: None,
        well_mm: 0.0,
    };
    let json = serde_json::to_value(Element::Stair(s)).unwrap();
    for key in ["shape", "turn", "first_flight_risers", "landing_mm", "well_mm"] {
        assert!(json.get(key).is_none(), "{key}");
    }
    let json = serde_json::to_value(opening("w", OpeningType::Door, 500.0)).unwrap();
    assert!(json.get("type_id").is_none());
}

// ------------------------------------------- title block (P1-4, P1-5)

#[test]
fn set_title_block_never_touches_the_signatory() {
    let mut doc = blank();
    let signatory = Signatory {
        name: "Arch. Juan Dela Cruz".into(),
        prc_no: "0012345".into(),
        tin: "123-456-789-000".into(),
        ..Signatory::default()
    };
    apply_checked(&mut doc, Command::SetSignatory { signatory: Box::new(signatory.clone()) });
    let block = TitleBlock {
        project_title: "Proposed two storey residence".into(),
        lot_no: "12".into(),
        signatory: Signatory { prc_no: "9999999".into(), ..Signatory::default() },
        ..TitleBlock::default()
    };
    let r = apply_checked(&mut doc, Command::SetTitleBlock { title_block: Box::new(block) });
    let tb = &r.state.project.title_block;
    assert_eq!(tb.lot_no, "12");
    assert_eq!(tb.signatory, signatory);
    // Masked for prompts, logs, MCP and agents' pictures.
    let masked = tb.signatory.masked();
    assert_eq!(masked.name, "Arch. Juan Dela Cruz");
    assert_eq!(masked.prc_no, "*****45");
    assert_eq!(masked.tin, "***-***-***-*00");
    assert_eq!(masked.ptr_no, "");
    assert!(!serde_json::to_string(&tb.masked()).unwrap().contains("0012345"));
}

// -------------------------------------------------- viewports (P1-8)

#[test]
fn a_viewport_whose_level_or_mark_is_deleted_leaves_its_sheet() {
    let mut doc = blank();
    apply_checked(&mut doc, Command::AddLevel { name: Some("Second floor".into()), elevation_mm: None, height_mm: None });
    let upper = doc.project().levels[1].id.clone();
    let mark = Element::ViewMark(ViewMark {
        id: "mark-a".into(),
        level_id: LEVEL.into(),
        mark_kind: MarkKind::Section,
        label: "A".into(),
        start: p(0.0, 0.0),
        end: p(5000.0, 0.0),
        flip: false,
        depth_mm: None,
    });
    apply_checked(&mut doc, add(mark));
    let vp = |view: ViewRef| Viewport { id: String::new(), view, scale_denominator: None, position: None, title: None };
    let sheet = Sheet {
        id: "sheet-1".into(),
        number: "A-3".into(),
        title: "Plans and section".into(),
        paper: PaperSize::A0,
        orientation: Orientation::Landscape,
        viewports: vec![
            vp(ViewRef::Plan { level_id: LEVEL.into(), sheet: SheetKind::Plan }),
            vp(ViewRef::Plan { level_id: upper.clone(), sheet: SheetKind::ReflectedCeiling }),
            vp(ViewRef::Section { mark_id: "mark-a".into() }),
        ],
        revision_ids: vec![],
    };
    apply_checked(&mut doc, Command::UpsertSheet { sheet: Box::new(sheet), index: None });
    let r = apply_checked(&mut doc, Command::DeleteLevel { level_id: upper });
    assert_eq!(r.state.project.sheet_set.sheets[0].viewports.len(), 2);
    assert!(r.diff.summary.ends_with("; removed 1 viewport from sheet A-3"), "{}", r.diff.summary);
    let r = apply_checked(&mut doc, Command::DeleteElements { ids: vec!["mark-a".into()] });
    assert_eq!(r.state.project.sheet_set.sheets[0].viewports.len(), 1);
    assert!(r.diff.summary.contains("removed 1 viewport from sheet A-3"), "{}", r.diff.summary);
    // A viewport must name what is there.
    let bad = Sheet {
        id: String::new(),
        number: String::new(),
        title: "x".into(),
        paper: PaperSize::A3,
        orientation: Orientation::Landscape,
        viewports: vec![vp(ViewRef::Elevation { mark_id: "nothing".into() })],
        revision_ids: vec![],
    };
    assert_eq!(code(&apply_rejected(&mut doc, Command::UpsertSheet { sheet: Box::new(bad), index: None })), "bad_viewport");
}

// ------------------------------------------ automatic dimensions (P1-2)

#[test]
fn automatic_dimensions_drop_deleted_walls_and_stop_being_automatic_when_edited() {
    let mut doc = blank();
    apply_checked(&mut doc, add_wall(p(0.0, 0.0), p(5000.0, 0.0)));
    apply_checked(&mut doc, add_wall(p(5000.0, 0.0), p(9000.0, 0.0)));
    let ids: Vec<Id> = walls(doc.project()).iter().map(|w| w.id.clone()).collect();
    let dim = |id: &str, wall_ids: Vec<Id>| {
        add(Element::Dimension(Dimension {
            id: id.into(),
            level_id: LEVEL.into(),
            a: p(0.0, 0.0),
            b: p(9000.0, 0.0),
            offset_mm: -800.0,
            text_override: None,
            through: vec![p(5000.0, 0.0)],
            auto: Some(DimRef::Openings { wall_ids, face: DimFace::Centerline }),
        }))
    };
    apply_checked(&mut doc, dim("both", ids.clone()));
    apply_checked(&mut doc, dim("first", vec![ids[0].clone()]));
    assert_eq!(code(&apply_rejected(&mut doc, dim("bad", vec!["nothing".into()]))), "bad_dimension");
    // Deleting the first wall drops it from both; "first" measures nothing
    // and goes in the same step.
    let r = apply_checked(&mut doc, Command::DeleteElements { ids: vec![ids[0].clone()] });
    assert_eq!(dimensions(&r.state.project).len(), 1);
    assert_eq!(
        dimension(&r.state.project, "both").auto,
        Some(DimRef::Openings { wall_ids: vec![ids[1].clone()], face: DimFace::Centerline })
    );
    // Moving a wall keeps the dimension automatic: the rewrite follows it
    // (L1's dims_auto tests check where it lands).
    let r = apply_checked(
        &mut doc,
        Command::MoveElements { ids: vec![ids[1].clone()], delta: p(0.0, 500.0), stretch_connected: false },
    );
    assert!(dimension(&r.state.project, "both").auto.is_some());
    assert_eq!(dimension(&r.state.project, "both").b, p(9000.0, 500.0));
    // Editing it by hand clears `auto`.
    let mut edited = dimension(doc.project(), "both").clone();
    edited.b = p(9500.0, 0.0);
    let r = apply_checked(&mut doc, Command::UpdateElement { element: Element::Dimension(edited) });
    assert_eq!(dimension(&r.state.project, "both").auto, None);
}

// ------------------------------------------------- views and stubs

#[test]
fn tours_are_views_on_no_level() {
    let mut doc = blank();
    let tour = Tour {
        id: String::new(),
        name: "Walkthrough".into(),
        fps: 30,
        shots: vec![Shot {
            id: String::new(),
            name: "Front".into(),
            kind: ShotMove::Hold,
            keys: vec![ShotKey { at: 0.0, position: Vec3 { x: 0.0, y: -8000.0, z: 1600.0 }, target: Vec3::default(), fov_deg: 60.0 }],
            seconds: 4.0,
            ease: Ease::InOut,
            light: None,
            light_end: None,
            transition: Transition { kind: TransitionKind::Cut, seconds: 0.0 },
        }],
    };
    let r = apply_checked(&mut doc, add(Element::Tour(tour.clone())));
    let el = r.state.project.elements.iter().find(|e| e.kind() == ElementKind::Tour).unwrap().clone();
    assert!(el.is_view());
    assert_eq!(el.level_id(), None);
    let Element::Tour(saved) = &el else { unreachable!() };
    assert!(!saved.shots[0].id.is_empty(), "shots get ids");
    // Always allowed under a selection scope, like a camera.
    apply_checked(&mut doc, rect_room(0.0, 0.0, 4000.0, 3000.0, Some("Bedroom")));
    let room = room_named(doc.project(), "Bedroom").id.clone();
    let derived = doc.derived().clone();
    assert!(scope::check(doc.project(), &derived, &[room.clone()], &add(Element::Tour(tour))).is_ok());
    // A copy gets new shot ids.
    let r = apply_checked(&mut doc, Command::DuplicateElements { ids: vec![el.id().clone()], delta: p(0.0, 0.0) });
    let shots: Vec<&Id> = r
        .state
        .project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Tour(t) => Some(&t.shots[0].id),
            _ => None,
        })
        .collect();
    assert_eq!(shots.len(), 2);
    assert_ne!(shots[0], shots[1]);
}

#[test]
fn unbuilt_wave_2_commands_say_so() {
    // Every Wave 2 command is built now; the refusal still reaches IPC by code.
    let ipc: IpcError = CoreError::not_built("A permit tool").into();
    assert_eq!(ipc.code, "not_built");
}
