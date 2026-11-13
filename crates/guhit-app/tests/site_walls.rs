//! A lot fence through the exports: its own layer in the plan and in DXF
//! (`L-SITE`), a boundary wall in IFC, and nothing changed for the house.

use guhit_core::{templates, Document};
use guhit_model::*;

fn fenced() -> (Document, Vec<Id>) {
    let mut doc = Document::new(templates::sample_bungalow());
    let r = doc
        .apply(
            Command::AddWallChain {
                points: vec![
                    Point { x: -2000.0, y: -4000.0 },
                    Point { x: 10_000.0, y: -4000.0 },
                    Point { x: 10_000.0, y: 11_000.0 },
                    Point { x: -2000.0, y: 11_000.0 },
                ],
                closed: true,
                thickness_mm: None,
                level_id: None,
                role: Some(WallRole::Site),
            },
            Origin::User,
        )
        .expect("the fence goes in");
    let ids = r.diff.added.clone();
    (doc, ids)
}

fn options() -> PlanExportOptions {
    PlanExportOptions {
        level_id: None,
        paper: PaperSize::A2,
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

#[test]
fn the_fence_is_on_l_site_in_dxf_and_the_house_is_unchanged() {
    let plain = Document::new(templates::sample_bungalow());
    let before = guhit_export::plan_dxf(plain.project(), plain.derived(), &options()).unwrap().data;
    assert!(!before.contains("L-SITE"), "a plan without a fence has no site layer");

    let (doc, _) = fenced();
    let dxf = guhit_export::plan_dxf(doc.project(), doc.derived(), &options()).unwrap().data;
    // The layer is declared, and the fence's outlines are drawn on it.
    assert!(dxf.contains("\n  2\nL-SITE\n"), "the layer table lists L-SITE");
    assert!(dxf.matches("\n  8\nL-SITE\n").count() >= 4, "four fence walls on L-SITE");
    // The house walls stay where they were.
    assert_eq!(
        before.matches("\n  8\nA-WALL\n").count(),
        dxf.matches("\n  8\nA-WALL\n").count(),
        "no house wall moved to or from A-WALL"
    );

    let svg = guhit_export::plan_svg(doc.project(), doc.derived(), &options()).unwrap().data;
    assert!(svg.contains("<g id=\"site-walls\">"));

    // Hiding the site layer hides the fence only.
    let mut hidden = doc.project().clone();
    for l in hidden.layers.iter_mut() {
        if l.key == LayerKey::Site {
            l.visible = false;
        }
    }
    let dxf = guhit_export::plan_dxf(&hidden, doc.derived(), &options()).unwrap().data;
    assert!(!dxf.contains("L-SITE"));
    assert_eq!(before.matches("\n  8\nA-WALL\n").count(), dxf.matches("\n  8\nA-WALL\n").count());
}

#[test]
fn the_fence_is_a_boundary_wall_in_ifc_and_on_l_site_in_3d_dxf() {
    let (doc, ids) = fenced();
    let ifc = guhit_export::model_ifc(doc.project(), doc.derived()).unwrap();
    let boundary: Vec<&str> = ifc.lines().filter(|l| l.contains("IFCWALL(") && l.contains("'BOUNDARYWALL'")).collect();
    assert_eq!(boundary.len(), 4, "four boundary walls");
    for l in &boundary {
        assert!(l.ends_with(",.USERDEFINED.);"), "{l}");
        assert!(l.contains("'Site wall "), "{l}");
    }
    // The house walls stay solid walls.
    let solid = ifc.lines().filter(|l| l.contains("IFCWALL(") && l.ends_with(",.SOLIDWALL.);")).count();
    let house = doc.project().elements.iter().filter(|e| matches!(e, Element::Wall(w) if !w.is_site())).count();
    assert_eq!(solid, house);
    assert_eq!(ids.len(), 4);

    let dxf3d = guhit_export::model_dxf3d(doc.project(), doc.derived()).unwrap();
    assert!(dxf3d.contains("\n  2\nL-SITE\n"));
    assert!(dxf3d.matches("\n  8\nL-SITE\n").count() > 0, "fence faces on L-SITE");
    let plain = Document::new(templates::sample_bungalow());
    let before = guhit_export::model_dxf3d(plain.project(), plain.derived()).unwrap();
    assert!(!before.contains("L-SITE"));
}
