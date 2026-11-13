//! Lane L1: grids, automatic dimensions, the lot from its technical
//! description, and the permit set (DECISIONS D53, D54).

mod common;

use common::*;
use guhit_core::{site, templates, Document};
use guhit_model::*;

fn bungalow() -> Document {
    Document::new(templates::sample_bungalow())
}

fn grids(doc: &Document) -> Vec<&GridLine> {
    doc.project()
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Grid(g) => Some(g),
            _ => None,
        })
        .collect()
}

fn dist2(a: Point, b: Point) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

// ------------------------------------------------------------------ grids

#[test]
fn a_grid_system_runs_1_2_3_along_x_and_a_b_c_along_y() {
    let mut doc = blank();
    let r = apply_checked(
        &mut doc,
        Command::AddGridSystem { origin: p(0.0, 0.0), rotation_deg: 0.0, x_spacings_mm: vec![4000.0, 3500.0], y_spacings_mm: vec![3000.0] },
    );
    assert_eq!(r.diff.summary, "Added 5 grid lines (1 to 3, A to B)");
    let g = grids(&doc);
    let labels: Vec<&str> = g.iter().map(|g| g.label.as_str()).collect();
    assert_eq!(labels, vec!["1", "2", "3", "A", "B"]);
    // 1, 2, 3 stand at x 0, 4000, 7500 and reach 3600 past A and B, so
    // their bubbles clear the exterior dimensions.
    assert_eq!((g[1].start, g[1].end), (p(4000.0, -3600.0), p(4000.0, 6600.0)));
    assert_eq!((g[2].start.x, g[4].start, g[4].end), (7500.0, p(-3600.0, 3000.0), p(11100.0, 3000.0)));

    // A second system continues the numbers and the letters.
    apply_checked(
        &mut doc,
        Command::AddGridSystem { origin: p(0.0, 10000.0), rotation_deg: 90.0, x_spacings_mm: vec![5000.0], y_spacings_mm: vec![2000.0] },
    );
    let labels: Vec<String> = grids(&doc).iter().map(|g| g.label.clone()).collect();
    assert_eq!(&labels[5..], &["4", "5", "C", "D"]);
    // Turned a quarter: numbered lines now run along x.
    let four = grids(&doc)[5].clone();
    assert!(near(four.start.y, 10000.0, 1e-6) && near(four.end.y, 10000.0, 1e-6), "{four:?}");

    assert_eq!(
        code(&apply_rejected(&mut doc, Command::AddGridSystem { origin: p(0.0, 0.0), rotation_deg: 0.0, x_spacings_mm: vec![50.0], y_spacings_mm: vec![] })),
        "bad_grid"
    );
    assert_eq!(
        code(&apply_rejected(&mut doc, Command::AddGridSystem { origin: p(0.0, 0.0), rotation_deg: 0.0, x_spacings_mm: vec![], y_spacings_mm: vec![] })),
        "bad_grid"
    );
}

#[test]
fn grid_letters_skip_i_and_o() {
    let letters: Vec<String> = (0..26).map(guhit_core::grids::grid_letter).collect();
    assert!(!letters.contains(&"I".to_string()) && !letters.contains(&"O".to_string()));
    assert_eq!((letters[7].as_str(), letters[8].as_str(), letters[23].as_str(), letters[24].as_str()), ("H", "J", "Z", "AA"));
}

// --------------------------------------------------- automatic dimensions

fn auto_dims(doc: &Document) -> Vec<Dimension> {
    doc.project()
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Dimension(d) if d.auto.is_some() => Some(d.clone()),
            _ => None,
        })
        .collect()
}

fn exterior(doc: &Document, side: Side, tier: DimTier) -> Option<Dimension> {
    auto_dims(doc).into_iter().find(|d| d.auto == Some(DimRef::Exterior { side, tier }))
}

/// The positions of a string along x (north and south) or y (east, west).
fn xs(d: &Dimension) -> Vec<f64> {
    let mut v: Vec<f64> = std::iter::once(d.a).chain(d.through.iter().copied()).chain(std::iter::once(d.b)).map(|q| q.x).collect();
    v.sort_by(f64::total_cmp);
    v
}

fn auto_bungalow() -> Document {
    let mut doc = bungalow();
    let level = doc.project().levels[0].id.clone();
    apply_checked(&mut doc, Command::AutoDimension { level_id: level, kinds: vec![AutoDimKind::Exterior] });
    doc
}

#[test]
fn auto_dimension_adds_the_exterior_tiers() {
    let mut doc = bungalow();
    let level = doc.project().levels[0].id.clone();
    let r = apply_checked(&mut doc, Command::AutoDimension { level_id: level.clone(), kinds: vec![AutoDimKind::Exterior] });
    // North: openings (the jalousie), walls (the partition at 5000), overall.
    // East: openings (the casement), overall. South: openings (door and
    // sliding window), walls, overall. West: overall.
    assert_eq!(r.diff.summary, "Added 9 automatic dimensions");
    let overall = exterior(&doc, Side::North, DimTier::Overall).unwrap();
    assert_eq!((overall.a, overall.b, overall.offset_mm), (p(-75.0, 6075.0), p(8075.0, 6075.0), 2400.0));
    assert!(overall.through.is_empty());
    // Wall centerlines, the line along the outer face.
    let walls = exterior(&doc, Side::North, DimTier::Walls).unwrap();
    assert_eq!(xs(&walls), vec![0.0, 5000.0, 8000.0]);
    assert!(std::iter::once(walls.a).chain(walls.through.iter().copied()).chain([walls.b]).all(|q| q.y == 6075.0));
    // The jalousie: 1200 wide, centered 5500 from (8000, 6000) going west.
    let openings = exterior(&doc, Side::North, DimTier::Openings).unwrap();
    assert_eq!(xs(&openings), vec![-75.0, 1900.0, 3100.0, 8075.0]);
    // South runs east to west, so a positive offset is outside.
    let south = exterior(&doc, Side::South, DimTier::Overall).unwrap();
    assert_eq!((south.a, south.b), (p(8075.0, -75.0), p(-75.0, -75.0)));
    assert!(exterior(&doc, Side::West, DimTier::Walls).is_none(), "nothing between the corners");

    // Again: they replace themselves.
    let r = apply_checked(&mut doc, Command::AutoDimension { level_id: level, kinds: vec![AutoDimKind::Exterior] });
    assert_eq!(r.diff.summary, "Added 9 automatic dimensions in place of 9");
    assert_eq!(auto_dims(&doc).len(), 9);
}

#[test]
fn automatic_dimensions_follow_a_wall_move_an_opening_move_and_a_deleted_wall() {
    let mut doc = auto_bungalow();
    let east = "00000000-0000-4000-8000-000000000102".to_string();
    // Move the east wall 1 m east, its neighbors stretching.
    apply_checked(&mut doc, Command::MoveElements { ids: vec![east.clone()], delta: p(1000.0, 0.0), stretch_connected: true });
    let overall = exterior(&doc, Side::North, DimTier::Overall).unwrap();
    assert_eq!((overall.a, overall.b), (p(-75.0, 6075.0), p(9075.0, 6075.0)));
    assert_eq!(xs(&exterior(&doc, Side::South, DimTier::Openings).unwrap()).last(), Some(&9075.0));

    // Move the jalousie 500 mm along its wall (toward the west).
    let jal = doc
        .project()
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Opening(o) if o.id.ends_with("205") => Some(o.clone()),
            _ => None,
        })
        .unwrap();
    let before = xs(&exterior(&doc, Side::North, DimTier::Openings).unwrap());
    apply_checked(&mut doc, Command::UpdateElement { element: Element::Opening(Opening { offset_mm: jal.offset_mm + 500.0, ..jal }) });
    let after = xs(&exterior(&doc, Side::North, DimTier::Openings).unwrap());
    assert_eq!(after[1], before[1] - 500.0, "{before:?} {after:?}");
    assert_eq!(after[2], before[2] - 500.0);

    // A string along the partition, then the partition goes.
    let level = doc.project().levels[0].id.clone();
    let partition = "00000000-0000-4000-8000-000000000105".to_string();
    let string = Element::Dimension(Dimension {
        id: "dim-partition".into(),
        level_id: level,
        a: p(0.0, 0.0),
        b: p(1.0, 0.0),
        offset_mm: 500.0,
        text_override: None,
        through: vec![],
        auto: Some(DimRef::Openings { wall_ids: vec![partition.clone()], face: DimFace::Interior }),
    });
    apply_checked(&mut doc, Command::AddElement { element: string });
    let d = auto_dims(&doc).into_iter().find(|d| d.id == "dim-partition").unwrap();
    // Up the partition: its interior face ends and the bedroom door's jambs.
    assert_eq!(d.a.y.min(d.b.y), 75.0);
    assert_eq!(d.through.iter().map(|q| q.y).collect::<Vec<_>>(), vec![2600.0, 3400.0]);
    assert_eq!(d.a.x, 4950.0, "the face left of the partition, where the line is");
    let walls_before = xs(&exterior(&doc, Side::North, DimTier::Walls).unwrap());
    assert!(walls_before.contains(&5000.0));
    apply_checked(&mut doc, Command::DeleteElements { ids: vec![partition] });
    assert!(auto_dims(&doc).iter().all(|d| d.id != "dim-partition"), "a string left with nothing to measure goes");
    // The walls tier no longer stops at the partition, and keeps the rest.
    assert!(exterior(&doc, Side::North, DimTier::Walls).map(|d| !xs(&d).contains(&5000.0)).unwrap_or(true));
    assert!(exterior(&doc, Side::North, DimTier::Overall).is_some());

    // Undo brings the partition and its string back where they were.
    doc.undo().unwrap();
    assert!(auto_dims(&doc).iter().any(|d| d.id == "dim-partition"));
}

#[test]
fn editing_an_automatic_dimension_by_hand_makes_it_plain() {
    let mut doc = auto_bungalow();
    let mut d = exterior(&doc, Side::West, DimTier::Overall).unwrap();
    d.a = p(d.a.x - 500.0, d.a.y);
    apply_checked(&mut doc, Command::UpdateElement { element: Element::Dimension(d.clone()) });
    let now = doc.project().elements.iter().find(|e| e.id() == &d.id).unwrap().clone();
    let Element::Dimension(now) = now else { panic!() };
    assert_eq!((now.auto, now.a), (None, d.a));

    // Changing only its offset keeps it automatic.
    let mut n = exterior(&doc, Side::North, DimTier::Overall).unwrap();
    n.offset_mm = 3000.0;
    apply_checked(&mut doc, Command::UpdateElement { element: Element::Dimension(n.clone()) });
    assert_eq!(exterior(&doc, Side::North, DimTier::Overall).unwrap().offset_mm, 3000.0);

    // Moving the whole house with its dimensions keeps them automatic.
    let all: Vec<Id> = doc.project().elements.iter().filter(|e| !e.is_view()).map(|e| e.id().clone()).collect();
    apply_checked(&mut doc, Command::MoveElements { ids: all, delta: p(1000.0, 0.0), stretch_connected: false });
    let moved = exterior(&doc, Side::North, DimTier::Overall).unwrap();
    assert_eq!((moved.a, moved.b), (p(925.0, 6075.0), p(9075.0, 6075.0)));
}

#[test]
fn a_dimension_string_follows_walls_at_every_point() {
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 6000.0, 4000.0, None));
    apply_checked(&mut doc, add_wall(p(3000.0, 0.0), p(3000.0, 4000.0)));
    let d = Dimension {
        id: "s".into(),
        level_id: LEVEL.into(),
        a: p(0.0, 0.0),
        b: p(6000.0, 0.0),
        offset_mm: -800.0,
        text_override: None,
        through: vec![p(3000.0, 0.0)],
        auto: None,
    };
    apply_checked(&mut doc, Command::AddElement { element: Element::Dimension(d) });
    let mid = wall_at(doc.project(), p(3000.0, 2000.0)).id.clone();
    apply_checked(&mut doc, Command::MoveElements { ids: vec![mid], delta: p(500.0, 0.0), stretch_connected: true });
    let s = dimension(doc.project(), "s");
    assert_eq!(s.through, vec![p(3500.0, 0.0)]);
}

// -------------------------------------------------------------------- lot

fn leg(bearing: &str, m: f64) -> SurveyLeg {
    SurveyLeg { bearing: bearing.into(), distance_mm: m * 1000.0 }
}

#[test]
fn bearings_read_in_the_usual_ph_forms() {
    let az = |s: &str| site::parse_bearing(s).unwrap_or_else(|e| panic!("{s}: {e}"));
    assert!(near(az("N 45 deg 30' E"), 45.5, 1e-9));
    assert!(near(az("N. 45 deg. 30' E."), 45.5, 1e-9));
    assert!(near(az("N 45°30' E"), 45.5, 1e-9));
    assert!(near(az("N45°30'E"), 45.5, 1e-9));
    assert!(near(az("N 45-30 E"), 45.5, 1e-9));
    assert!(near(az("n 45d30' e"), 45.5, 1e-9));
    assert!(near(az("S 44 deg. 30' E."), 135.5, 1e-9));
    assert!(near(az("S 12°05'36\" W"), 192.0 + 5.0 / 60.0 + 36.0 / 3600.0, 1e-9));
    assert!(near(az("N 44 DEG 30 MIN W"), 315.5, 1e-9));
    assert!(near(az("North 10 degrees West"), 350.0, 1e-9));
    assert!(near(az("Due East"), 90.0, 1e-9));
    assert!(near(az("N 0 deg 00' E"), 0.0, 1e-9));
    for bad in ["N 95 deg E", "E 45 deg N", "N 45 deg 75' E", "45 deg 30'", "", "N 45 X 30 E"] {
        assert!(site::parse_bearing(bad).is_err(), "{bad} should not read");
    }
    assert_eq!(site::format_bearing(45.5), "N 45°30' E");
    assert_eq!(site::format_bearing(135.5), "S 44°30' E");
    assert_eq!(site::format_bearing(192.0 + 5.0 / 60.0 + 36.0 / 3600.0), "S 12°05'36\" W");
    assert_eq!(site::format_bearing(315.5), "N 44°30' W");
}

/// A 12 by 15 m lot turned 45°30' (a Quezon City subdivision), its title
/// bearings to the minute and distances to the centimeter, as TCTs write
/// them. Corner 1 is the north corner; it runs clockwise, as most do.
fn tct_legs() -> Vec<SurveyLeg> {
    vec![leg("S. 44 deg. 30' E.", 12.00), leg("S. 45 deg. 30' W.", 15.00), leg("N. 44 deg. 30' W.", 12.00), leg("N. 45 deg. 31' E.", 15.00)]
}

#[test]
fn a_tct_description_of_a_12_by_15_lot_closes_within_10_mm() {
    let mut doc = blank();
    let r = apply_checked(&mut doc, Command::SetLotFromSurvey { start: p(0.0, 20000.0), legs: tct_legs() });
    // One leg is a minute off: 15 m x 0.00029 rad = 4.4 mm.
    let summary = r.diff.summary.clone();
    assert!(summary.starts_with("Set the lot from 4 legs: 180.00 m2, misclosure 4 mm (1:"), "{summary}");
    assert!(summary.ends_with("spread over the corners"), "{summary}");
    let lot = doc.project().elements.iter().find_map(|e| match e {
        Element::Lot(l) => Some(l.clone()),
        _ => None,
    });
    let lot = lot.unwrap();
    assert_eq!(lot.corners[0], p(0.0, 20000.0), "corner 1 stays where it was put");
    assert!(area(&lot.corners) < 0.0, "clockwise, in the title's order");
    let site = doc.derived().site.clone().unwrap();
    assert!(near(site.area_m2, 180.0, 0.01), "{}", site.area_m2);
    // Side 1 is the title's line 1-2, about S 44°30' E.
    assert_eq!(site.sides.len(), 4);
    assert!(site.sides.iter().all(|s| near(s.length_mm, 12000.0, 5.0) || near(s.length_mm, 15000.0, 5.0)));
    assert!(site.sides[0].bearing.starts_with("S 44°"), "{}", site.sides[0].bearing);
    assert_eq!(site.coverage_pct, 0.0);
    assert!(site.setback_outline.is_empty());

    // Setting it again edits the same lot.
    let id = lot.id.clone();
    let mut legs = tct_legs();
    legs[3] = leg("N 45-30 E", 15.0);
    let r = apply_checked(&mut doc, Command::SetLotFromSurvey { start: p(0.0, 20000.0), legs });
    assert!(r.diff.summary.contains("it closes exactly"), "{}", r.diff.summary);
    let lots: Vec<&Lot> = doc.project().elements.iter().filter_map(|e| match e {
        Element::Lot(l) => Some(l),
        _ => None,
    }).collect();
    assert_eq!((lots.len(), lots[0].id.clone()), (1, id));
}

#[test]
fn a_bad_bearing_or_an_open_survey_is_refused_with_a_hint() {
    let mut doc = blank();
    let mut legs = tct_legs();
    legs[1] = leg("S. 95 deg. 30' W.", 15.0);
    let e = apply_rejected(&mut doc, Command::SetLotFromSurvey { start: p(0.0, 0.0), legs });
    assert_eq!(code(&e), "bad_bearing");
    let msg = e.to_string();
    assert!(msg.contains("Leg 2") && msg.contains("0 to 90") && msg.contains("N 45 deg 30' E"), "{msg}");

    let mut legs = tct_legs();
    legs[2] = leg("S. 44 deg. 30' E.", 12.0);
    let e = apply_rejected(&mut doc, Command::SetLotFromSurvey { start: p(0.0, 0.0), legs });
    assert_eq!(code(&e), "lot_not_closed");
    assert!(e.to_string().contains("24.00 m from where they start"), "{e}");

    let e = apply_rejected(&mut doc, Command::SetLotFromSurvey { start: p(0.0, 0.0), legs: tct_legs()[..2].to_vec() });
    assert_eq!(code(&e), "lot_outline");
}

#[test]
fn the_site_states_bearings_clearances_coverage_and_setbacks_as_facts() {
    let mut doc = blank();
    // An 8 x 6 m house (outer faces -75 to 8075, -75 to 6075) on a 12 x 15 m
    // lot square to the plan, true north up.
    apply_checked(&mut doc, rect_room(0.0, 0.0, 8000.0, 6000.0, Some("House")));
    let legs = vec![leg("N 90 deg E", 12.0), leg("N 0 deg E", 15.0), leg("S 90 deg W", 12.0), leg("S 0 deg W", 15.0)];
    apply_checked(&mut doc, Command::SetLotFromSurvey { start: p(-2000.0, -3000.0), legs });
    let site = doc.derived().site.clone().unwrap();
    assert_eq!(site.area_m2, 180.0);
    let bearings: Vec<&str> = site.sides.iter().map(|s| s.bearing.as_str()).collect();
    assert_eq!(bearings, vec!["N 90°00' E", "N 0°00' E", "S 90°00' W", "S 0°00' W"]);
    // South side at y -3000: the house face at -75.
    assert_eq!(site.sides[0].clearance_mm, Some(2925.0));
    assert_eq!(site.sides[1].clearance_mm, Some(10000.0 - 8075.0));
    assert!(near(site.coverage_pct, 8.15 * 6.15 / 180.0 * 100.0, 0.01), "{}", site.coverage_pct);
    assert!(doc.derived().issues.iter().all(|i| i.code != "building_over_setback"));

    // A 3 m front setback on the south side: the house is 75 mm past it.
    let mut lot = doc.project().elements.iter().find_map(|e| match e {
        Element::Lot(l) => Some(l.clone()),
        _ => None,
    }).unwrap();
    lot.sides[0] = LotSide { role: LotSideRole::Road, setback_mm: 3000.0, road: "Mabini Street".into(), road_width_mm: Some(8000.0) };
    apply_checked(&mut doc, Command::UpdateElement { element: Element::Lot(lot) });
    let site = doc.derived().site.clone().unwrap();
    assert_eq!(site.setback_outline.len(), 4);
    let has = |q: Point| site.setback_outline.iter().any(|c| dist2(*c, q) < 1e-6);
    assert!(has(p(-2000.0, 0.0)) && has(p(10000.0, 0.0)) && has(p(10000.0, 12000.0)), "{:?}", site.setback_outline);
    let over: Vec<&Issue> = doc.derived().issues.iter().filter(|i| i.code == "building_over_setback").collect();
    assert_eq!(over.len(), 1);
    assert!(over[0].message.starts_with("The building reaches 75 mm past the 3.00 m setback drawn along lot side 1, along Mabini Street."), "{}", over[0].message);
    assert_eq!(over[0].severity, Severity::Warning);
}

// ------------------------------------------------------------- sheet set

fn sheets(doc: &Document) -> Vec<(String, String, usize)> {
    doc.project().sheet_set.sheets.iter().map(|s| (s.number.clone(), s.title.clone(), s.viewports.len())).collect()
}

fn marks(doc: &Document) -> Vec<ViewMark> {
    doc.project()
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::ViewMark(m) => Some(m.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn make_sheet_set_on_the_plain_bungalow_makes_no_services_sheets() {
    let mut doc = bungalow();
    let r = apply_checked(&mut doc, Command::MakeSheetSet { paper: PaperSize::A1, replace: false });
    assert_eq!(r.diff.summary, "Made 7 sheets: A-1, A-2, A-3, A-4, A-5, A-6, A-7, with 4 elevation marks and 2 section marks");
    let got = sheets(&doc);
    let want: Vec<(String, String, usize)> = [
        ("A-1", "Site Development Plan and Vicinity Map", 2),
        ("A-2", "Ground Floor Plan", 1),
        ("A-3", "North and South Elevations", 2),
        ("A-4", "East and West Elevations", 2),
        ("A-5", "Sections A and B", 2),
        ("A-6", "Roof Plan and Reflected Ceiling Plans", 2),
        ("A-7", "Schedules of Doors, Windows, Rooms, Finishes and Areas", 4),
    ]
    .iter()
    .map(|(n, t, v)| (n.to_string(), t.to_string(), *v))
    .collect();
    assert_eq!(got, want);
    assert!(doc.project().sheet_set.sheets.iter().all(|s| s.paper == PaperSize::A1));

    // The marks: each elevation stands off its face and looks back at it.
    let m = marks(&doc);
    let north = m.iter().find(|m| m.label == "North").unwrap();
    assert_eq!(north.mark_kind, MarkKind::Elevation);
    assert!(north.start.y > 6075.0 && north.end.y > 6075.0 && north.start.x > north.end.x, "{north:?}");
    let south = m.iter().find(|m| m.label == "South").unwrap();
    assert!(south.start.y < -75.0 && south.start.x < south.end.x, "{south:?}");
    let sections: Vec<&ViewMark> = m.iter().filter(|m| m.mark_kind == MarkKind::Section).collect();
    assert_eq!(sections.iter().map(|m| m.label.as_str()).collect::<Vec<_>>(), vec!["A", "B"]);
    // Through the longest room, along it and across it.
    let along = |m: &ViewMark| unit_x(m);
    assert!(along(sections[0]) != along(sections[1]));

    // Again without replace: nothing is missing.
    assert_eq!(code(&apply_rejected(&mut doc, Command::MakeSheetSet { paper: PaperSize::A1, replace: false })), "nothing_to_add");
    // With replace: the same set, no new marks.
    let r = apply_checked(&mut doc, Command::MakeSheetSet { paper: PaperSize::A3, replace: true });
    assert_eq!(r.diff.summary, "Made 7 sheets: A-1, A-2, A-3, A-4, A-5, A-6, A-7 in place of 7 sheets");
    assert_eq!(marks(&doc).len(), 6);
}

fn unit_x(m: &ViewMark) -> bool {
    (m.end.x - m.start.x).abs() > (m.end.y - m.start.y).abs()
}

#[test]
fn make_sheet_set_on_the_plumbing_demo_makes_the_e_and_p_sheets() {
    let mut doc = Document::new(templates::plumbing_demo());
    apply_checked(&mut doc, Command::MakeSheetSet { paper: PaperSize::A1, replace: false });
    let numbers: Vec<String> = sheets(&doc).into_iter().map(|s| s.0).collect();
    for n in ["E-1", "E-2", "P-1", "P-2"] {
        assert!(numbers.contains(&n.to_string()), "{n} in {numbers:?}");
    }
    let titles: Vec<String> = sheets(&doc).into_iter().map(|s| s.1).collect();
    assert!(titles.contains(&"Lighting Layout".to_string()) && titles.contains(&"Plumbing Isometric Diagrams".to_string()));
    let p1 = doc.project().sheet_set.sheets.iter().find(|s| s.number == "P-1").unwrap();
    assert!(p1.viewports.iter().all(|v| matches!(v.view, ViewRef::Plan { sheet: SheetKind::Plumbing, .. })));
}

#[test]
fn a_deleted_mark_leaves_its_sheet_in_the_same_step() {
    let mut doc = bungalow();
    apply_checked(&mut doc, Command::MakeSheetSet { paper: PaperSize::A1, replace: false });
    let a = marks(&doc).into_iter().find(|m| m.label == "A").unwrap();
    let r = apply_checked(&mut doc, Command::DeleteElements { ids: vec![a.id] });
    assert!(r.diff.summary.ends_with("; removed 1 viewport from sheet A-5"), "{}", r.diff.summary);
    // Making the set again adds the missing section and its sheet.
    let r = apply_checked(&mut doc, Command::MakeSheetSet { paper: PaperSize::A1, replace: false });
    assert_eq!(r.diff.summary, "Made sheet A-8, with 1 section mark");
}

#[test]
fn auto_dimension_rooms_adds_inside_strings_along_the_walls() {
    let mut doc = bungalow();
    let level = doc.project().levels[0].id.clone();
    let r = apply_checked(&mut doc, Command::AutoDimension { level_id: level, kinds: vec![AutoDimKind::Rooms] });
    // Living: along the west wall and along the north one (the longest wall
    // and the longest square to it). Bedroom: along its longest wall; its
    // north wall's inside string is the living room's, so it is not repeated.
    assert_eq!(r.diff.summary, "Added 3 automatic dimensions");
    let north = "00000000-0000-4000-8000-000000000103".to_string();
    let d = auto_dims(&doc)
        .into_iter()
        .find(|d| d.auto == Some(DimRef::Openings { wall_ids: vec![north.clone()], face: DimFace::Interior }))
        .unwrap_or_else(|| panic!("{:#?}", auto_dims(&doc)));
    // The north wall runs west, so the room (south of it) is on its left.
    assert_eq!(d.offset_mm, dims_room_offset());
    // The inside face from corner to corner, the jalousie's jambs, and both
    // faces of the partition.
    assert_eq!(xs(&d), vec![75.0, 1900.0, 3100.0, 4950.0, 5050.0, 7925.0]);
    assert!(std::iter::once(d.a).chain(d.through.iter().copied()).chain([d.b]).all(|q| q.y == 5925.0));
    assert_eq!((d.a.x, d.b.x), (7925.0, 75.0), "a to b along the wall");
}

fn dims_room_offset() -> f64 {
    guhit_core::dims_auto::ROOM_OFFSET_MM
}
