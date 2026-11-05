//! Lane L7: the Wave 2 arms of `guhit_core::scope` (D30 selections, D47
//! agent limits and claims, P1-3): door and window types touch every
//! opening of the type, grid lines pass level limits and kind limits
//! decide, the lot, sheets, revisions and the title block stay whole-project
//! changes, tours are views, and roofs, view marks and grids land in a
//! selection like any element. The roof over the top floor: agent_limits.rs.

use guhit_core::{compute_derived, scope, templates, CoreError};
use guhit_model::*;

const GROUND: &str = "00000000-0000-4000-8000-0000000000a1";
const BEDROOM: &str = "00000000-0000-4000-8000-000000000302";

fn p(x: f64, y: f64) -> Point {
    Point { x, y }
}

/// The bungalow with an empty second level, "upper", every opening typed D1
/// or W1.
fn house() -> Project {
    let mut project = templates::sample_bungalow();
    let mut upper = project.levels[0].clone();
    upper.id = "upper".into();
    upper.name = "Second Floor".into();
    upper.elevation_mm = 3000.0;
    project.levels.push(upper);
    for (id, mark, kind) in [("d1", "D1", OpeningType::Door), ("w1", "W1", OpeningType::Window)] {
        project.opening_types.push(OpeningSpec {
            id: id.into(),
            mark: mark.into(),
            opening_type: kind,
            style: OpeningStyle::SwingSingle,
            width_mm: 900.0,
            height_mm: 2100.0,
            sill_mm: None,
            material_id: None,
            description: String::new(),
            remarks: String::new(),
        });
    }
    for el in project.elements.iter_mut() {
        if let Element::Opening(o) = el {
            o.type_id = Some(if o.opening_type == OpeningType::Door { "d1".into() } else { "w1".into() });
        }
    }
    project
}

fn limit(levels: &[&str], kinds: &[ElementKind]) -> AgentLimit {
    AgentLimit { level_ids: levels.iter().map(|s| s.to_string()).collect(), ids: vec![], kinds: kinds.to_vec() }
}

fn message(e: CoreError) -> String {
    match e {
        CoreError::Invalid { code, message, .. } => {
            assert_eq!(code, "out_of_scope");
            message
        }
        other => panic!("expected out_of_scope, got {other:?}"),
    }
}

fn spec(project: &Project, id: &str) -> OpeningSpec {
    OpeningSpec { width_mm: 1000.0, ..project.opening_type(id).unwrap().clone() }
}

#[test]
fn a_type_change_touches_every_opening_of_the_type() {
    let project = house();
    let derived = compute_derived(&project);
    let check = |l: &AgentLimit, c: &Command| scope::check_limit(&project, &derived, &project, l, c);
    let d1 = Command::UpsertOpeningSpec { spec: Box::new(spec(&project, "d1")) };
    // An openings-only agent may define D1; a walls-only one may not.
    assert!(check(&limit(&[], &[ElementKind::Opening]), &d1).is_ok());
    assert!(message(check(&limit(&[], &[ElementKind::Wall]), &d1).unwrap_err()).contains("a door"));
    // Every D1 is on the ground floor: a ground-floor agent may, an upper one not.
    assert!(check(&limit(&[GROUND], &[]), &d1).is_ok());
    assert!(message(check(&limit(&["upper"], &[]), &d1).unwrap_err()).contains("is on Ground Floor"));
    // Deleting a type, and typing the untyped ones, touch the same openings.
    assert!(check(&limit(&["upper"], &[]), &Command::DeleteOpeningSpecs { ids: vec!["w1".into()] }).is_err());
    let mut untyped = project.clone();
    untyped.elements.iter_mut().for_each(|e| {
        if let Element::Opening(o) = e {
            o.type_id = None;
        }
    });
    let assign = Command::AssignOpeningTypes { level_id: Some(GROUND.into()) };
    let d = compute_derived(&untyped);
    assert!(scope::check_limit(&untyped, &d, &untyped, &limit(&[GROUND], &[ElementKind::Opening]), &assign).is_ok());
    assert!(scope::check_limit(&untyped, &d, &untyped, &limit(&["upper"], &[]), &assign).is_err());
    // A selection (D30) never reaches a type: it is project data.
    let e = scope::check(&project, &derived, &[BEDROOM.into()], &d1).unwrap_err();
    assert!(message(e).starts_with("Changing door and window types affects the whole project"));
}

#[test]
fn grid_lines_pass_level_limits_and_kinds_decide() {
    let project = house();
    let derived = compute_derived(&project);
    let check = |l: &AgentLimit, c: &Command| scope::check_limit(&project, &derived, &project, l, c);
    let system = Command::AddGridSystem { origin: p(0.0, 0.0), rotation_deg: 0.0, x_spacings_mm: vec![4000.0], y_spacings_mm: vec![3000.0] };
    let line = Command::AddElement { element: Element::Grid(GridLine { id: String::new(), label: "A".into(), start: p(0.0, -1000.0), end: p(0.0, 7000.0) }) };
    for c in [&system, &line] {
        assert!(check(&limit(&["upper"], &[]), c).is_ok(), "{c:?}");
        assert!(check(&limit(&[], &[ElementKind::Grid]), c).is_ok());
        assert!(message(check(&limit(&[], &[ElementKind::Wall]), c).unwrap_err()).contains("a grid"));
    }
    // A grid is nobody's claim: it holds on every level.
    assert_eq!(scope::claim_reach(&project, &derived, &project, &["upper".into()], &[], &line), None);
    // A selection never reaches one.
    assert!(message(scope::check(&project, &derived, &[BEDROOM.into()], &line).unwrap_err()).starts_with("Adding a grid line"));
}

#[test]
fn the_lot_sheets_revisions_and_title_block_are_whole_project_changes() {
    let project = house();
    let derived = compute_derived(&project);
    let any = limit(&[], &[ElementKind::Lot, ElementKind::Wall, ElementKind::Roof]);
    let sheet = Sheet {
        id: String::new(),
        number: "A-1".into(),
        title: "Site".into(),
        paper: PaperSize::A1,
        orientation: Orientation::Landscape,
        viewports: vec![],
        revision_ids: vec![],
    };
    for (c, what) in [
        (Command::SetLotFromSurvey { start: p(0.0, 0.0), legs: vec![] }, "Setting the lot"),
        (Command::SetTitleBlock { title_block: Box::default() }, "Changing the title block"),
        (Command::MakeSheetSet { paper: PaperSize::A1, replace: false }, "Changing the sheets"),
        (Command::UpsertSheet { sheet: Box::new(sheet), index: None }, "Changing the sheets"),
        (Command::DeleteRevision { id: "r".into() }, "Changing the sheets"),
    ] {
        let m = message(scope::check_limit(&project, &derived, &project, &any, &c).unwrap_err());
        assert!(m.starts_with(&format!("{what} affects the whole project")), "{m}");
        assert!(scope::check(&project, &derived, &[BEDROOM.into()], &c).is_err());
        // No limit, no refusal.
        assert!(scope::check_limit(&project, &derived, &project, &limit(&[], &[]), &c).is_ok());
    }
}

#[test]
fn tours_are_views_and_marks_and_roofs_land_where_drawn() {
    let project = house();
    let derived = compute_derived(&project);
    let tour = Command::MakeTour { name: None, options: AutoTour { exterior: true, day_to_dusk: true, pace_mps: 0.8, levels: vec![] } };
    assert!(scope::check_limit(&project, &derived, &project, &limit(&["upper"], &[ElementKind::Wall]), &tour).is_ok());
    assert!(scope::check(&project, &derived, &[BEDROOM.into()], &tour).is_ok());
    // A section mark and a carport roof sit on a level: a level limit decides.
    let mark = Command::AddElement {
        element: Element::ViewMark(ViewMark {
            id: String::new(),
            level_id: GROUND.into(),
            mark_kind: MarkKind::Section,
            label: "A".into(),
            start: p(-1000.0, 3000.0),
            end: p(9000.0, 3000.0),
            flip: false,
            depth_mm: None,
        }),
    };
    let carport = Command::AddElement {
        element: Element::Roof(RoofArea {
            id: String::new(),
            level_id: Some(GROUND.into()),
            outline: RoofOutline::Polygon { points: vec![p(8000.0, 0.0), p(11_000.0, 0.0), p(11_000.0, 6000.0), p(8000.0, 6000.0)] },
            shape: RoofShape::Shed,
            pitch_deg: 5.0,
            ridge_axis: Axis::X,
            flip: false,
            gable_sides: vec![],
            overhang_mm: 300.0,
            thickness_mm: 150.0,
            eave_mm: None,
            material_id: None,
            fascia: None,
            gutter: None,
        }),
    };
    for c in [&mark, &carport] {
        assert!(scope::check_limit(&project, &derived, &project, &limit(&[GROUND], &[]), c).is_ok());
        assert!(message(scope::check_limit(&project, &derived, &project, &limit(&["upper"], &[]), c).unwrap_err()).contains("would be on Ground Floor"));
        // Outside the bedroom: refused under that selection.
        assert!(scope::check(&project, &derived, &[BEDROOM.into()], c).is_err());
    }
    // A claim on the ground floor reaches both.
    assert!(scope::claim_reach(&project, &derived, &project, &[GROUND.into()], &[], &carport).is_some());
}
