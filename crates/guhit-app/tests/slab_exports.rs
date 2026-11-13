//! Slabs through the real engine and every export: a two storey bungalow
//! with a balcony, a void and a stair, built with `guhit_core::Document`
//! commands, then exported from `doc.derived()` (docs/CONTRACT.md "Slabs and
//! railings", DECISIONS D44 and D50).

use guhit_core::{templates, Document};
use guhit_model::*;

fn p(x: f64, y: f64) -> Point {
    Point { x, y }
}

fn slab(id: &str, level: &str, kind: SlabKind, pts: &[(f64, f64)], railing: Option<Railing>) -> Element {
    Element::Slab(Slab {
        id: id.into(),
        level_id: level.into(),
        kind,
        polygon: pts.iter().map(|q| p(q.0, q.1)).collect(),
        thickness_mm: if kind == SlabKind::Void { 0.0 } else { 150.0 },
        top_offset_mm: 0.0,
        material_id: None,
        railing,
    })
}

/// The sample bungalow, a second floor on the same 8 by 6 m box, a balcony
/// with a glass rail on the south front, a void with a bars rail over the
/// living room and an 11 riser stair with handrails up to it.
fn two_storey() -> (Document, Id, Id) {
    let mut doc = Document::new(templates::sample_bungalow());
    let ground = doc.project().levels[0].id.clone();
    doc.apply(
        Command::AddLevel {
            name: Some("Second Floor".into()),
            elevation_mm: Some(3000.0),
            height_mm: Some(3000.0),
        },
        Origin::User,
    )
    .expect("the second floor goes in");
    let second = doc
        .project()
        .levels
        .iter()
        .find(|l| l.id != ground)
        .expect("a second level")
        .id
        .clone();
    doc.apply(
        Command::AddWallChain {
            points: vec![p(0.0, 0.0), p(8000.0, 0.0), p(8000.0, 6000.0), p(0.0, 6000.0)],
            closed: true,
            thickness_mm: Some(150.0),
            level_id: Some(second.clone()),
            role: None,
        },
        Origin::User,
    )
    .expect("the second floor walls go in");
    let glass = Railing { height_mm: 1100.0, style: RailingStyle::Glass };
    let bars = Railing { height_mm: 1000.0, style: RailingStyle::Bars };
    for element in [
        slab(
            "slab-balcony",
            &second,
            SlabKind::Balcony,
            &[(1000.0, -1575.0), (4000.0, -1575.0), (4000.0, -75.0), (1000.0, -75.0)],
            Some(glass),
        ),
        slab(
            "slab-void",
            &second,
            SlabKind::Void,
            &[(500.0, 3500.0), (2500.0, 3500.0), (2500.0, 5500.0), (500.0, 5500.0)],
            Some(bars),
        ),
        Element::Stair(Stair {
            id: "stair-up".into(),
            level_id: ground.clone(),
            origin: p(3500.0, 500.0),
            rotation_deg: 0.0,
            width_mm: 1000.0,
            run_mm: 2750.0,
            riser_count: 11,
            railing: Some(Railing { height_mm: 900.0, style: RailingStyle::Bars }),
        }),
    ] {
        let what = format!("{:?}", element.kind());
        doc.apply(Command::AddElement { element }, Origin::User)
            .unwrap_or_else(|e| panic!("the {what} goes in: {e:?}"));
    }
    (doc, ground, second)
}

fn options(level: &str) -> PlanExportOptions {
    PlanExportOptions {
        level_id: Some(level.into()),
        paper: PaperSize::A3,
        orientation: Orientation::Landscape,
        scale_denominator: Some(100),
        show_dimensions: true,
        show_room_labels: true,
        show_assets: true,
        title_block: true,
        show_pipes: true,
        sheet: SheetKind::Plan,
        review_page: false,
        date: Some("2026-11-04".into()),
    }
}

/// What the exports draw from the model alone: slab outlines, the void
/// cross, the stair by D44 and from below, IfcSlab and the void's opening.
#[test]
fn slabs_and_the_stair_reach_the_plan_dxf_and_ifc() {
    let (doc, ground, second) = two_storey();
    let (project, derived) = (doc.project(), doc.derived());

    let svg = guhit_export::plan_svg(project, derived, &options(&second)).unwrap().data;
    assert!(svg.contains("<g id=\"slabs\">"));
    assert_eq!(svg.matches("OPEN TO BELOW").count(), 1);
    assert!(svg.contains("<g id=\"stairs-below\">") && svg.contains(">DN<"), "the stair from below");
    let ground_svg = guhit_export::plan_svg(project, derived, &options(&ground)).unwrap().data;
    assert!(ground_svg.contains(">UP<") && !ground_svg.contains("<g id=\"stairs-below\">"));

    let dxf = guhit_export::plan_dxf(project, derived, &options(&second)).unwrap().data;
    assert!(dxf.contains("\n  2\nA-FLOR-OTLN\n"), "the layer table lists A-FLOR-OTLN");
    assert!(dxf.contains("\n  6\nGUHIT_DASHED\n"), "the flight from below is dashed");
    // D44 on the ground floor: 11 risers, 9 lines between 10 treads, and
    // the arrow shaft.
    let ground_dxf = guhit_export::plan_dxf(project, derived, &options(&ground)).unwrap().data;
    let stair_lines = ground_dxf.matches("  0\nLINE\n  8\nA-FLOR-STRS\n").count();
    assert_eq!(stair_lines, 9 + 1);

    let ifc = guhit_export::model_ifc(project, derived).unwrap();
    assert!(guhit_export::ifc::dangling_refs(&ifc).is_empty());
    let balcony = ifc.lines().find(|l| l.contains("'BALCONY'")).expect("an IfcSlab for the balcony");
    assert!(balcony.contains("IFCSLAB(") && balcony.ends_with(",$,.USERDEFINED.);"), "{balcony}");
    assert_eq!(derived.footprints.iter().filter(|f| f.level_id == second).count(), 1, "the engine finds the second floor");
    let void = ifc.lines().find(|l| l.contains("'Void 1 in ")).expect("an opening for the void");
    assert!(void.contains("IFCOPENINGELEMENT("));
    assert!(ifc.contains("IFCRELVOIDSELEMENT("));
    let dxf3d = guhit_export::model_dxf3d(project, derived).unwrap();
    assert!(dxf3d.contains("A-FLOR-SLAB"));
}

/// The rails the engine derives (`Derived::railings`) in every export. Needs
/// the engine side of D50.
#[test]
fn railings_from_the_engine_reach_every_export() {
    let (doc, ground, second) = two_storey();
    let (project, derived) = (doc.project(), doc.derived());
    let kinds = |k: RailingRunKind| derived.railings.iter().filter(|r| r.kind == k).count();
    assert!(kinds(RailingRunKind::SlabEdge) >= 2, "balcony and void rails: {:?}", derived.railings);
    assert_eq!(kinds(RailingRunKind::StairSide), 2, "both sides of the flight are open");
    assert_eq!(kinds(RailingRunKind::StairWell), 1, "one guard round the well");
    assert!(derived.railings.iter().filter(|r| r.kind == RailingRunKind::StairSide).all(|r| r.level_id == ground));
    assert!(derived.railings.iter().filter(|r| r.kind == RailingRunKind::StairWell).all(|r| r.level_id == second));

    let svg = guhit_export::plan_svg(project, derived, &options(&second)).unwrap().data;
    assert!(svg.contains("<g id=\"railings\">"));
    let dxf = guhit_export::plan_dxf(project, derived, &options(&second)).unwrap().data;
    assert!(dxf.contains("\n  2\nA-FLOR-HRAL\n"));
    let ifc = guhit_export::model_ifc(project, derived).unwrap();
    assert!(guhit_export::ifc::dangling_refs(&ifc).is_empty());
    assert_eq!(guhit_export::ifc::count_entities(&ifc).railings, derived.railings.len());
    assert_eq!(ifc.matches(",$,.HANDRAIL.);").count(), 2);
    let dxf3d = guhit_export::model_dxf3d(project, derived).unwrap();
    assert!(dxf3d.contains("\n  8\nA-FLOR-HRAL\n"), "rail solids in 3D");
}
