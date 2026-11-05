//! Slabs, railings and stairs (DECISIONS D44, D50; docs/CONTRACT.md "Slabs
//! and railings"): outline rules, overlaps, voids over floor, areas, the
//! railing runs in `Derived::railings`, the review items, D44 treads and
//! the schema 5 migration.

mod common;

use common::*;
use guhit_core::{migrate, templates, Document};
use guhit_model::*;

const M2: f64 = 1.0e6;

fn bars(h: f64) -> Option<Railing> {
    Some(Railing { height_mm: h, style: RailingStyle::Bars })
}

fn slab(level: &str, kind: SlabKind, pts: &[(f64, f64)], railing: Option<Railing>) -> Slab {
    Slab {
        id: String::new(),
        level_id: level.into(),
        kind,
        polygon: pts.iter().map(|q| p(q.0, q.1)).collect(),
        thickness_mm: 150.0,
        top_offset_mm: 0.0,
        material_id: None,
        railing,
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<(f64, f64)> {
    vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
}

fn add(s: Slab) -> Command {
    Command::AddElement { element: Element::Slab(s) }
}

/// Add a slab that must go in, and return its id.
fn put(doc: &mut Document, s: Slab) -> Id {
    let r = apply_checked(doc, add(s));
    assert_eq!(r.diff.added.len(), 1, "{:?}", r.diff);
    r.diff.added[0].clone()
}

fn slab_of<'a>(doc: &'a Document, id: &str) -> &'a Slab {
    doc.project()
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Slab(s) if s.id == id => Some(s),
            _ => None,
        })
        .expect("the slab")
}

fn level_id(doc: &Document, name: &str) -> Id {
    doc.project().levels.iter().find(|l| l.name == name).expect("level").id.clone()
}

fn add_level(doc: &mut Document, name: &str, elevation: f64) -> Id {
    apply_checked(
        doc,
        Command::AddLevel { name: Some(name.into()), elevation_mm: Some(elevation), height_mm: Some(3000.0) },
    );
    level_id(doc, name)
}

fn rect_on(level: &str, x: f64, y: f64, w: f64, d: f64, name: &str) -> Command {
    Command::AddRectRoom {
        origin: p(x, y),
        width_mm: w,
        depth_mm: d,
        name: Some(name.into()),
        thickness_mm: None,
        level_id: Some(level.into()),
    }
}

/// An 8 by 6 m box on the ground floor and the same box on the second
/// floor, 150 mm walls (net 7850 x 5850).
fn two_storey() -> (Document, Id, Id) {
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 8000.0, 6000.0, Some("Living")));
    let up = add_level(&mut doc, "Second floor", 3000.0);
    apply_checked(&mut doc, rect_on(&up, 0.0, 0.0, 8000.0, 6000.0, "Hall"));
    (doc, LEVEL.to_string(), up)
}

fn room_area(doc: &Document, name: &str) -> f64 {
    let id = room_named(doc.project(), name).id.clone();
    room_geo(&doc.state(), &id).area_mm2
}

fn issues_with(doc: &Document, code: &str) -> Vec<Issue> {
    doc.derived().issues.iter().filter(|i| i.code == code).cloned().collect()
}

fn runs_of<'a>(doc: &'a Document, host: &str) -> Vec<&'a RailingRun> {
    doc.derived().railings.iter().filter(|r| r.host_id == host).collect()
}

fn xy(run: &RailingRun) -> Vec<(f64, f64)> {
    run.points.iter().map(|v| ((v.x * 10.0).round() / 10.0, (v.y * 10.0).round() / 10.0)).collect()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

// ------------------------------------------------------------------ areas

#[test]
fn a_void_takes_its_area_off_the_room_the_floor_area_and_the_gross() {
    let (mut doc, _, up) = two_storey();
    let net = 7850.0 * 5850.0;
    assert!(near(room_area(&doc, "Hall"), net));
    let floor_before = doc.derived().totals.floor_area_m2;
    let gross_before = doc.derived().totals.gross_area_m2;

    let id = put(&mut doc, slab(&up, SlabKind::Void, &rect(1000.0, 1000.0, 3000.0, 4000.0), bars(1000.0)));
    assert!(near(room_area(&doc, "Hall"), net - 6.0e6), "{}", room_area(&doc, "Hall"));
    assert!(near(room_area(&doc, "Living"), net), "the room below keeps its area");
    let t = &doc.derived().totals;
    assert!((t.floor_area_m2 - (floor_before - 6.0)).abs() < 1e-6, "{}", t.floor_area_m2);
    assert!((t.gross_area_m2 - (gross_before - 6.0)).abs() < 1e-6, "{}", t.gross_area_m2);

    // No wall runs along it, so the rail goes all the way round.
    let runs = runs_of(&doc, &id);
    assert_eq!(runs.len(), 1);
    let r = runs[0];
    assert_eq!(r.kind, RailingRunKind::SlabEdge);
    assert_eq!(r.level_id, up);
    assert_eq!((r.height_mm, r.style), (1000.0, RailingStyle::Bars));
    assert_eq!(xy(r), vec![(1000.0, 1000.0), (3000.0, 1000.0), (3000.0, 4000.0), (1000.0, 4000.0), (1000.0, 1000.0)]);
    assert!(r.points.iter().all(|v| v.z == 0.0));

    // Undo brings the area back.
    doc.undo().unwrap();
    assert!(near(room_area(&doc, "Hall"), net));
    assert!(doc.derived().railings.is_empty());
}

#[test]
fn a_balcony_is_not_floor_area_and_rails_its_open_edges_only() {
    let (mut doc, _, up) = two_storey();
    let floor_before = doc.derived().totals.floor_area_m2;
    let gross_before = doc.derived().totals.gross_area_m2;
    // Against the south wall's outer face (wall centerline y = 0).
    let mut b = slab(&up, SlabKind::Balcony, &rect(2000.0, -1575.0, 5000.0, -75.0), bars(1100.0));
    b.top_offset_mm = -50.0;
    let id = put(&mut doc, b);
    assert_eq!(doc.derived().totals.floor_area_m2, floor_before);
    assert!((doc.derived().totals.gross_area_m2 - gross_before).abs() < 1e-6);

    let runs = runs_of(&doc, &id);
    assert_eq!(runs.len(), 1, "one U round the three open sides");
    assert_eq!(xy(runs[0]), vec![(2000.0, -75.0), (2000.0, -1575.0), (5000.0, -1575.0), (5000.0, -75.0)]);
    assert!(runs[0].points.iter().all(|v| v.z == -50.0), "the foot is on the slab top");
    assert!(issues_with(&doc, "slab_no_railing").is_empty());
}

#[test]
fn a_lanai_walled_in_on_a_deck_is_a_room_but_not_floor_area() {
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 8000.0, 6000.0, Some("Living")));
    apply_checked(&mut doc, rect_room(0.0, -3000.0, 4000.0, 3000.0, Some("Lanai")));
    let lanai = room_area(&doc, "Lanai");
    let floor_before = doc.derived().totals.floor_area_m2;
    let gross_before = doc.derived().totals.gross_area_m2;
    put(&mut doc, slab(LEVEL, SlabKind::Deck, &rect(0.0, -3000.0, 4000.0, 0.0), None));
    assert!(near(room_area(&doc, "Lanai"), lanai), "still a room of its own size");
    let t = &doc.derived().totals;
    assert!((t.floor_area_m2 - (floor_before - lanai / M2)).abs() < 1e-6, "{} {}", t.floor_area_m2, floor_before);
    // The lanai's walls stay gross area; only its clear floor comes off (GA
    // P0-1: open slabs are clipped to the inside of the walls).
    assert!((t.gross_area_m2 - (gross_before - lanai / M2)).abs() < 1e-6, "{}", t.gross_area_m2);
    // At grade: no railing note.
    assert!(issues_with(&doc, "slab_no_railing").is_empty());
}

#[test]
fn a_floor_slab_adds_gross_area_past_the_walls() {
    let (mut doc, _, up) = two_storey();
    let gross_before = doc.derived().totals.gross_area_m2;
    // Half inside the second floor's footprint (outer face x = 8075), half out.
    put(&mut doc, slab(&up, SlabKind::Floor, &rect(7075.0, 1000.0, 9075.0, 3000.0), None));
    assert!((doc.derived().totals.gross_area_m2 - (gross_before + 2.0)).abs() < 1e-6);
}

// ------------------------------------------------------------------ rules

#[test]
fn outlines_are_kept_counter_clockwise_without_repeats() {
    let (mut doc, _, up) = two_storey();
    // Clockwise, with a repeated point and a closing duplicate.
    let cw = [(1000.0, 1000.0), (1000.0, 3000.0), (1000.0, 3000.4), (3000.0, 3000.0), (3000.0, 1000.0), (1000.0, 1000.0)];
    let id = put(&mut doc, slab(&up, SlabKind::Void, &cw, None));
    let s = slab_of(&doc, &id);
    assert_eq!(s.polygon.len(), 4);
    let area: f64 = (0..4)
        .map(|i| {
            let (a, b) = (s.polygon[i], s.polygon[(i + 1) % 4]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f64>()
        / 2.0;
    assert!(area > 0.0, "counter-clockwise");
    let summary = doc.history().undo[0].label.clone();
    assert_eq!(summary, "Add void");
}

#[test]
fn a_crossed_or_tiny_outline_is_refused() {
    let (mut doc, _, up) = two_storey();
    let bow = [(1000.0, 1000.0), (3000.0, 3000.0), (3000.0, 1000.0), (1000.0, 3000.0)];
    let e = apply_rejected(&mut doc, add(slab(&up, SlabKind::Balcony, &bow, None)));
    assert_eq!(code(&e), "slab_outline");
    assert!(e.to_string().contains("crosses itself"), "{e}");
    let e = apply_rejected(&mut doc, add(slab(&up, SlabKind::Floor, &rect(0.0, 0.0, 50.0, 50.0), None)));
    assert_eq!(code(&e), "slab_outline");
    let e = apply_rejected(&mut doc, add(slab(&up, SlabKind::Floor, &[(0.0, 0.0), (1000.0, 0.0)], None)));
    assert_eq!(code(&e), "slab_outline");
}

#[test]
fn overlapping_slabs_of_one_group_are_refused_and_named() {
    let (mut doc, _, up) = two_storey();
    let a = put(&mut doc, slab(&up, SlabKind::Balcony, &rect(2000.0, -1575.0, 5000.0, -75.0), None));
    // Another balcony, half on the first.
    let e = apply_rejected(&mut doc, add(slab(&up, SlabKind::Balcony, &rect(4000.0, -1575.0, 7000.0, -75.0), None)));
    assert_eq!(code(&e), "slab_overlap");
    let ids = error_ids(&e);
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[1], a, "names the slab it overlaps");
    assert!(e.to_string().contains("1.50 m2"), "{e}");
    // A deck is floor too.
    let e = apply_rejected(&mut doc, add(slab(&up, SlabKind::Deck, &rect(4000.0, -1575.0, 7000.0, -75.0), None)));
    assert_eq!(code(&e), "slab_overlap");
    // Sharing an edge is fine.
    put(&mut doc, slab(&up, SlabKind::Deck, &rect(5000.0, -1575.0, 7000.0, -75.0), None));
    // A void over the balcony is what a void is for.
    let v = put(&mut doc, slab(&up, SlabKind::Void, &rect(2500.0, -1000.0, 3500.0, -500.0), None));
    // Voids may not overlap voids.
    let e = apply_rejected(&mut doc, add(slab(&up, SlabKind::Void, &rect(3000.0, -1000.0, 4500.0, -500.0), None)));
    assert_eq!(code(&e), "slab_overlap");
    assert_eq!(error_ids(&e)[1], v);
    // Moving one slab onto another is refused the same way.
    let e = apply_rejected(
        &mut doc,
        Command::MoveElements { ids: vec![a.clone()], delta: p(1500.0, 0.0), stretch_connected: false },
    );
    assert_eq!(code(&e), "slab_overlap");
    assert_eq!(error_ids(&e)[0], a);
    // The same slab on another level is no overlap.
    put(&mut doc, slab(LEVEL, SlabKind::Deck, &rect(4000.0, -1575.0, 7000.0, -75.0), None));
}

#[test]
fn a_void_must_lie_over_floor() {
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 8000.0, 6000.0, Some("Living")));
    let up = add_level(&mut doc, "Second floor", 3000.0);
    // No walls up there yet: the ground floor's roof deck is the floor, so a
    // void over the living room is fine.
    put(&mut doc, slab(&up, SlabKind::Void, &rect(500.0, 500.0, 3000.0, 3000.0), None));
    // Half of this one hangs past the house.
    let e = apply_rejected(&mut doc, add(slab(&up, SlabKind::Void, &rect(7000.0, 500.0, 10000.0, 2000.0), None)));
    assert_eq!(code(&e), "void_outside_floor");
    assert!(e.to_string().contains("2.89 m2"), "{e}");
    // Over a balcony out there it is fine.
    put(&mut doc, slab(&up, SlabKind::Balcony, &rect(8075.0, 500.0, 10000.0, 2000.0), None));
    put(&mut doc, slab(&up, SlabKind::Void, &rect(7000.0, 500.0, 10000.0, 2000.0), None));
    // Gate round 2: the lowest level has nothing below to open into, even
    // inside its own walls (a slab tool click on a room must not cut a hole
    // to the lawn).
    for poly in [rect(500.0, 500.0, 3000.0, 3000.0), rect(8500.0, 3000.0, 9500.0, 4000.0)] {
        let e = apply_rejected(&mut doc, add(slab(LEVEL, SlabKind::Void, &poly, None)));
        assert_eq!(code(&e), "void_no_level_below");
        assert!(e.to_string().contains("lowest level"), "{e}");
    }
    // Neither can a void become one there by a kind change.
    let deck = put(&mut doc, slab(LEVEL, SlabKind::Deck, &rect(500.0, 500.0, 3000.0, 3000.0), None));
    let mut s = slab_of(&doc, &deck).clone();
    s.kind = SlabKind::Void;
    assert_eq!(code(&apply_rejected(&mut doc, Command::UpdateElement { element: Element::Slab(s) })), "void_no_level_below");
}

#[test]
fn slab_values_are_validated() {
    let (mut doc, _, up) = two_storey();
    let mut thin = slab(&up, SlabKind::Balcony, &rect(2000.0, -1575.0, 5000.0, -75.0), None);
    thin.thickness_mm = 20.0;
    assert_eq!(code(&apply_rejected(&mut doc, add(thin))), "slab_thickness");
    let mut low = slab(&up, SlabKind::Balcony, &rect(2000.0, -1575.0, 5000.0, -75.0), bars(100.0));
    low.railing = bars(100.0);
    assert_eq!(code(&apply_rejected(&mut doc, add(low))), "railing_height");
    // A void's thickness is ignored.
    let mut void = slab(&up, SlabKind::Void, &rect(1000.0, 1000.0, 2000.0, 2000.0), None);
    void.thickness_mm = 0.0;
    put(&mut doc, void);
}

#[test]
fn a_locked_slabs_layer_refuses_new_slabs() {
    let (mut doc, _, up) = two_storey();
    apply_checked(&mut doc, Command::SetLayer { layer: Layer { key: LayerKey::Slabs, visible: true, locked: true } });
    let e = apply_rejected(&mut doc, add(slab(&up, SlabKind::Void, &rect(1000.0, 1000.0, 2000.0, 2000.0), None)));
    assert_eq!(code(&e), "layer_locked");
}

// ------------------------------------------------------------------ railings and review

#[test]
fn open_edges_without_a_railing_get_a_review_item() {
    let (mut doc, _, up) = two_storey();
    let balcony = put(&mut doc, slab(&up, SlabKind::Balcony, &rect(2000.0, -1575.0, 5000.0, -75.0), None));
    let void = put(&mut doc, slab(&up, SlabKind::Void, &rect(1000.0, 1000.0, 3000.0, 4000.0), None));
    let floor = put(&mut doc, slab(&up, SlabKind::Floor, &rect(8075.0, 1000.0, 9075.0, 3000.0), None));
    let found = issues_with(&doc, "slab_no_railing");
    let on: Vec<&Id> = found.iter().map(|i| &i.element_ids[0]).collect();
    assert_eq!(on, vec![&balcony, &void], "a floor slab gets no note");
    assert!(found.iter().all(|i| i.severity == Severity::Warning));
    assert!(found[0].message.contains("6.00 m of open edge"), "{}", found[0].message);
    assert!(found[0].message.contains("3.00 m drop"), "{}", found[0].message);
    assert!(found[1].message.starts_with("This void on Second floor"), "{}", found[1].message);
    assert!(doc.derived().railings.is_empty(), "no railing, no runs");
    let _ = floor;

    // With a railing the note goes and the runs appear.
    let mut b = slab_of(&doc, &balcony).clone();
    b.railing = Some(Railing { height_mm: 1000.0, style: RailingStyle::Glass });
    apply_checked(&mut doc, Command::UpdateElement { element: Element::Slab(b) });
    assert_eq!(issues_with(&doc, "slab_no_railing").len(), 1);
    assert_eq!(runs_of(&doc, &balcony)[0].style, RailingStyle::Glass);
}

#[test]
fn a_void_edge_along_a_wall_needs_no_rail() {
    let (mut doc, _, up) = two_storey();
    // Against the west wall's inner face (centerline x = 0, face x = 75).
    let id = put(&mut doc, slab(&up, SlabKind::Void, &rect(75.0, 1000.0, 3000.0, 4000.0), bars(1000.0)));
    let runs = runs_of(&doc, &id);
    assert_eq!(runs.len(), 1);
    assert_eq!(xy(runs[0]), vec![(75.0, 1000.0), (3000.0, 1000.0), (3000.0, 4000.0), (75.0, 4000.0)]);
}

fn stair(level: &str, x: f64, y: f64, railing: Option<Railing>) -> Element {
    Element::Stair(Stair {
        id: String::new(),
        level_id: level.into(),
        origin: p(x, y),
        rotation_deg: 0.0,
        width_mm: 1000.0,
        run_mm: 2750.0,
        riser_count: 11,
        railing,
        shape: StairShape::Straight,
        turn: Turn::Left,
        first_flight_risers: None,
        landing_mm: None,
        well_mm: 0.0,
    })
}

fn put_stair(doc: &mut Document, e: Element) -> Id {
    apply_checked(doc, Command::AddElement { element: e }).diff.added[0].clone()
}

#[test]
fn a_stair_rails_its_open_sides_and_its_opening_upstairs() {
    let (mut doc, ground, up) = two_storey();
    let id = put_stair(&mut doc, stair(&ground, 4000.0, 1000.0, bars(900.0)));
    let runs = runs_of(&doc, &id);
    let sides: Vec<&&RailingRun> = runs.iter().filter(|r| r.kind == RailingRunKind::StairSide).collect();
    assert_eq!(sides.len(), 2);
    for s in &sides {
        assert_eq!(s.level_id, ground);
        assert_eq!(s.points.len(), 2);
        assert!(near(s.points[0].z, 3000.0 / 11.0), "one riser up at the first nosing");
        assert!(near(s.points[1].z, 3000.0), "the floor above at the top");
        assert!(near(s.points[0].y, 1000.0) && near(s.points[1].y, 3750.0));
    }
    assert_eq!(xy(sides[0]), vec![(3500.0, 1000.0), (3500.0, 3750.0)]);
    assert_eq!(xy(sides[1]), vec![(4500.0, 1000.0), (4500.0, 3750.0)]);
    let wells: Vec<&&RailingRun> = runs.iter().filter(|r| r.kind == RailingRunKind::StairWell).collect();
    assert_eq!(wells.len(), 1);
    assert_eq!(wells[0].level_id, up);
    assert_eq!(xy(wells[0]), vec![(3500.0, 3750.0), (3500.0, 1000.0), (4500.0, 1000.0), (4500.0, 3750.0)]);
    assert!(wells[0].points.iter().all(|v| v.z == 0.0));
    assert!(issues_with(&doc, "stair_top_no_floor").is_empty(), "it lands on the second floor");

    // Against the east wall (inner face x = 7925): one open side.
    let against = put_stair(&mut doc, stair(&ground, 7425.0, 1000.0, bars(900.0)));
    let sides = runs_of(&doc, &against).into_iter().filter(|r| r.kind == RailingRunKind::StairSide).count();
    assert_eq!(sides, 1);
    // Without a railing, nothing.
    let bare = put_stair(&mut doc, stair(&ground, 1500.0, 1000.0, None));
    assert!(runs_of(&doc, &bare).is_empty());
}

#[test]
fn a_stair_that_lands_on_no_floor_gets_a_review_item() {
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 8000.0, 6000.0, Some("Living")));
    let id = put_stair(&mut doc, stair(LEVEL, 4000.0, 1000.0, None));
    let found = issues_with(&doc, "stair_top_no_floor");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].element_ids, vec![id.clone()]);
    assert!(found[0].message.contains("the top level"), "{}", found[0].message);

    // A level that starts too high.
    let high = add_level(&mut doc, "Mezzanine", 3400.0);
    let found = issues_with(&doc, "stair_top_no_floor");
    assert!(found[0].message.contains("Mezzanine starts 3.40 m up"), "{}", found[0].message);
    apply_checked(&mut doc, Command::DeleteLevel { level_id: high });

    // A level at the top of the stair: it lands on the roof deck of the
    // ground floor, which is floor.
    let up = add_level(&mut doc, "Second floor", 3000.0);
    assert!(issues_with(&doc, "stair_top_no_floor").is_empty());
    // Unless a void is there.
    put(&mut doc, slab(&up, SlabKind::Void, &rect(3000.0, 3500.0, 5000.0, 4500.0), None));
    let found = issues_with(&doc, "stair_top_no_floor");
    assert!(found[0].message.contains("lands in a void"), "{}", found[0].message);

    // A stair that runs out past the house lands on nothing.
    let out = put_stair(&mut doc, stair(LEVEL, 6500.0, 4500.0, None));
    let found = issues_with(&doc, "stair_top_no_floor");
    assert!(found.iter().any(|i| i.element_ids == vec![out.clone()] && i.message.contains("outside the floor")));
}

#[test]
fn a_described_stair_counts_treads_as_d44_does() {
    let (mut doc, ground, _) = two_storey();
    let id = put_stair(&mut doc, stair(&ground, 4000.0, 1000.0, bars(900.0)));
    let v = doc.query(&Query::Describe { ids: vec![id] }).unwrap();
    let s = &v["elements"][0];
    assert_eq!(s["riser_count"], 11);
    assert_eq!(s["tread_count"], 10, "{s}");
    assert_eq!(s["tread_mm"], 275.0);
    assert_eq!(s["riser_mm"], 272.7);
    assert_eq!(s["arrives_at"], "Second floor");
    assert_eq!(s["railing"]["style"], "bars");
}

// ------------------------------------------------------------------ migration

#[test]
fn a_version_4_project_gets_the_slabs_layer_and_its_stairs_no_railing() {
    let mut v4 = serde_json::to_value(templates::sample_bungalow()).unwrap();
    v4["schema_version"] = serde_json::json!(4);
    let layers = v4["layers"].as_array_mut().unwrap();
    layers.retain(|l| !matches!(l["key"].as_str(), Some("slabs" | "grids" | "roofs")));
    assert_eq!(layers.len(), 17);
    let level = v4["levels"][0]["id"].clone();
    v4["elements"].as_array_mut().unwrap().push(serde_json::json!({
        "kind": "stair", "id": "old-stair", "level_id": level,
        "origin": { "x": 1000.0, "y": 1000.0 }, "rotation_deg": 0.0,
        "width_mm": 900.0, "run_mm": 2700.0, "riser_count": 15
    }));
    let old: Project = serde_json::from_value(v4).expect("a version 4 file reads");
    let Some(Element::Stair(s)) = old.elements.iter().find(|e| e.id() == "old-stair") else {
        panic!("the stair");
    };
    assert_eq!(s.railing, None);

    let doc = Document::new(old.clone());
    assert_eq!(doc.project().schema_version, SCHEMA_VERSION);
    // Slabs, then the grids and roofs layers of schema 6.
    let added: Vec<(LayerKey, bool, bool)> =
        doc.project().layers[17..].iter().map(|l| (l.key, l.visible, l.locked)).collect();
    assert_eq!(
        added,
        vec![(LayerKey::Slabs, true, false), (LayerKey::Grids, true, false), (LayerKey::Roofs, true, false)]
    );
    assert_eq!(doc.project().layers.len(), 20);
    let mut twice = old;
    migrate(&mut twice);
    let once = twice.clone();
    migrate(&mut twice);
    assert_eq!(once, twice, "migrating twice changes nothing");

    // A slab round-trips with its kind under `slab_kind`.
    let el = Element::Slab(slab(LEVEL, SlabKind::Balcony, &rect(0.0, 0.0, 1000.0, 1000.0), bars(1000.0)));
    let json = serde_json::to_value(&el).unwrap();
    assert_eq!(json["kind"], "slab");
    assert_eq!(json["slab_kind"], "balcony");
}

// ------------------------------------------------------------------ gate round 2

/// Every area a query reports is the one the plan shows: less the void.
#[test]
fn queries_report_room_and_level_areas_less_the_void() {
    let (mut doc, _, up) = two_storey();
    put(&mut doc, slab(&up, SlabKind::Void, &rect(1000.0, 1000.0, 3000.0, 4000.0), bars(1000.0)));
    let hall = room_named(doc.project(), "Hall").id.clone();
    let shown = room_geo(&doc.state(), &hall).area_mm2 / M2;
    assert!((shown - 39.9225).abs() < 1e-6, "{shown}");

    let list = doc.query(&Query::RoomList).unwrap();
    let row = list["rooms"].as_array().unwrap().iter().find(|r| r["id"] == hall.as_str()).unwrap().clone();
    assert_eq!(row["net_area_m2"], 39.92, "{row}");
    assert_eq!(row["void_area_m2"], 6.0);
    assert_eq!(list["total_net_area_m2"], ((doc.derived().totals.floor_area_m2 * 100.0).round() / 100.0));

    let sum = doc.query(&Query::ProjectSummary).unwrap();
    let level = sum["levels"].as_array().unwrap().iter().find(|l| l["id"] == up.as_str()).unwrap().clone();
    assert_eq!(level["net_floor_area_m2"], 39.92, "{level}");
    assert_eq!(level["gross_footprint_area_m2"], 44.12, "{level}");
    let in_summary = sum["rooms"].as_array().unwrap().iter().find(|r| r["id"] == hall.as_str()).unwrap().clone();
    assert_eq!(in_summary["net_area_m2"], 39.92);
    let levels_net: f64 = sum["levels"].as_array().unwrap().iter().map(|l| l["net_floor_area_m2"].as_f64().unwrap()).sum();
    assert!((levels_net - doc.derived().totals.floor_area_m2).abs() < 0.02, "levels add up to the total");

    let described = doc.query(&Query::Describe { ids: vec![hall.clone()] }).unwrap();
    assert_eq!(described["elements"][0]["net_area_m2"], 39.92, "{described}");
    let no_window = doc.query(&Query::RoomsWithoutExteriorWindow).unwrap();
    let row = no_window["rooms"].as_array().unwrap().iter().find(|r| r["id"] == hall.as_str()).unwrap().clone();
    assert_eq!(row["net_area_m2"], 39.92);
}

fn object(level: &str, key: &str, x: f64, y: f64) -> Element {
    let item = defaults::asset_catalog().into_iter().find(|c| c.key == key).expect("catalog item");
    Element::Asset(Asset {
        id: String::new(),
        level_id: level.into(),
        catalog_key: item.key,
        name: item.name,
        category: item.category,
        position: p(x, y),
        rotation_deg: 0.0,
        width_mm: item.width_mm,
        depth_mm: item.depth_mm,
        height_mm: item.height_mm,
        elevation_mm: item.elevation_mm,
        light: item.light,
        links: vec![],
        circuit: String::new(),
        built_in: None,
    })
}

fn put_el(doc: &mut Document, e: Element) -> Id {
    apply_checked(doc, Command::AddElement { element: e }).diff.added[0].clone()
}

#[test]
fn an_object_over_a_void_or_a_stair_opening_gets_a_review_item() {
    let (mut doc, ground, up) = two_storey();
    let st = put_stair(&mut doc, stair(&ground, 6000.0, 1000.0, None));
    let v = put(&mut doc, slab(&up, SlabKind::Void, &rect(1000.0, 1000.0, 3000.0, 4000.0), bars(1000.0)));
    let sofa = put_el(&mut doc, object(&up, "sofa-2", 2000.0, 2500.0));
    let chair = put_el(&mut doc, object(&up, "armchair", 6000.0, 2000.0));
    // On the floor, and a lamp hanging over the void: nothing to say.
    put_el(&mut doc, object(&up, "armchair", 4500.0, 4500.0));
    put_el(&mut doc, object(&up, "light-ceiling", 2000.0, 2500.0));
    let found = issues_with(&doc, "asset_over_opening");
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[0].element_ids, vec![sofa, v]);
    assert!(found[0].message.contains("stands over a void"), "{}", found[0].message);
    assert_eq!(found[1].element_ids, vec![chair, st]);
    assert!(found[1].message.contains("opening of a stair"), "{}", found[1].message);
}

#[test]
fn furniture_inside_a_door_swing_gets_a_review_item() {
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 8000.0, 6000.0, Some("Living")));
    let south = wall_at(doc.project(), p(4000.0, 0.0)).clone();
    let door = apply_checked(&mut doc, opening(&south.id, OpeningType::Door, 2000.0)).diff.added[0].clone();
    // Where the leaf sweeps: past the hinge jamb, on the side it swings to.
    let u = {
        let (dx, dy) = (south.end.x - south.start.x, south.end.y - south.start.y);
        let l = (dx * dx + dy * dy).sqrt();
        (dx / l, dy / l)
    };
    let nrm = (-u.1, u.0);
    let c = (south.start.x + u.0 * 2000.0, south.start.y + u.1 * 2000.0);
    let at = (c.0 + nrm.0 * 600.0, c.1 + nrm.1 * 600.0);
    let chair = put_el(&mut doc, object(LEVEL, "armchair", at.0, at.1));
    // Well away from the door.
    put_el(&mut doc, object(LEVEL, "sofa-2", 6000.0, 4500.0));
    let found = issues_with(&doc, "door_swing_blocked");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].element_ids, vec![door.clone(), chair.clone()]);
    assert!(found[0].message.contains("where this door swings open"), "{}", found[0].message);
    // Turned to swing the other way, the door clears it.
    let mut o = openings(doc.project()).into_iter().find(|o| o.id == door).unwrap().clone();
    o.flip_side = true;
    apply_checked(&mut doc, Command::UpdateElement { element: Element::Opening(o.clone()) });
    assert!(issues_with(&doc, "door_swing_blocked").is_empty());
    // A sliding door sweeps nothing.
    o.flip_side = false;
    o.style = OpeningStyle::Sliding;
    apply_checked(&mut doc, Command::UpdateElement { element: Element::Opening(o) });
    assert!(issues_with(&doc, "door_swing_blocked").is_empty());
}

/// The reviewer's dead-end landing: walls boxing in the top of the flight
/// on the floor it arrives at, and a first riser 75 mm from a wall.
#[test]
fn a_wall_at_either_end_of_a_stair_gets_a_review_item() {
    let (mut doc, ground, up) = two_storey();
    let st = put_stair(&mut doc, stair(&ground, 4000.0, 1000.0, None));
    assert!(issues_with(&doc, "stair_landing_blocked").is_empty(), "clear at both ends: {:?}", issues_with(&doc, "stair_landing_blocked"));
    // A wall 150 mm past the top step, upstairs.
    let r = apply_checked(
        &mut doc,
        Command::AddWall { start: p(3000.0, 3900.0), end: p(5000.0, 3900.0), thickness_mm: None, height_mm: None, material_id: None, level_id: Some(up.clone()), role: None },
    );
    let wall = r.diff.added[0].clone();
    let found = issues_with(&doc, "stair_landing_blocked");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].element_ids, vec![st.clone(), wall]);
    assert!(found[0].message.contains("meets a wall 0.07 m past its last step on Second floor"), "{}", found[0].message);
    // The stair still lands on floor: this is the check that covers it.
    assert!(issues_with(&doc, "stair_top_no_floor").is_empty());

    // A first riser 75 mm from the south wall's inner face.
    let foot = put_stair(&mut doc, stair(&ground, 6500.0, 150.0, None));
    let found = issues_with(&doc, "stair_landing_blocked");
    let at_foot = found.iter().find(|i| i.element_ids[0] == foot).expect("the foot is flagged");
    assert!(at_foot.message.contains("first step of this stair is 0.07 m from a wall"), "{}", at_foot.message);
}

// ------------------------------------------------------------------ gate fixes

/// Gate P1: an L-shaped balcony drawn as two rectangles gets no rail along
/// the edge they share, and the review counts only the outer edges.
#[test]
fn an_l_balcony_of_two_slabs_has_no_rail_across_its_walking_surface() {
    let (mut doc, _, up) = two_storey();
    let a = put(&mut doc, slab(&up, SlabKind::Balcony, &rect(2000.0, -1575.0, 5000.0, -75.0), bars(1000.0)));
    let b = put(&mut doc, slab(&up, SlabKind::Balcony, &rect(5000.0, -3575.0, 6500.0, -75.0), bars(1000.0)));
    let on_shared = |r: &RailingRun| {
        r.points.windows(2).any(|w| {
            let mid = ((w[0].x + w[1].x) / 2.0, (w[0].y + w[1].y) / 2.0);
            (mid.0 - 5000.0).abs() < 1.0 && mid.1 > -1575.0 && mid.1 < -75.0
        })
    };
    for id in [&a, &b] {
        let runs = runs_of(&doc, id);
        assert!(!runs.is_empty());
        assert!(runs.iter().all(|r| !on_shared(r)), "a rail on the shared edge: {runs:?}");
    }
    // The first balcony keeps its two outer sides as one run: west, south.
    assert_eq!(xy(runs_of(&doc, &a)[0]), vec![(2000.0, -75.0), (2000.0, -1575.0), (5000.0, -1575.0)]);
    // The second rails its south and east sides and the stretch of its west
    // side below the first balcony.
    let mut b_runs: Vec<Vec<(f64, f64)>> = runs_of(&doc, &b).iter().map(|r| xy(r)).collect();
    b_runs.sort_by(|x, y| x[0].partial_cmp(&y[0]).unwrap());
    assert_eq!(b_runs, vec![vec![(5000.0, -1575.0), (5000.0, -3575.0), (6500.0, -3575.0), (6500.0, -75.0)]]);

    // Without railings, the review measures the same open edges.
    let mut doc2 = two_storey().0;
    let up2 = level_id(&doc2, "Second floor");
    put(&mut doc2, slab(&up2, SlabKind::Balcony, &rect(2000.0, -1575.0, 5000.0, -75.0), None));
    put(&mut doc2, slab(&up2, SlabKind::Balcony, &rect(5000.0, -3575.0, 6500.0, -75.0), None));
    let found = issues_with(&doc2, "slab_no_railing");
    assert_eq!(found.len(), 2);
    assert!(found[0].message.contains("4.50 m of open edge"), "{}", found[0].message);
    assert!(found[1].message.contains("7.00 m of open edge"), "{}", found[1].message);
}

/// Gate P1: a void round a stair (the double-height stair hall) puts no
/// rail across the stair's arrival end, and the stair's well guard does not
/// stand inside the void.
#[test]
fn a_stair_hall_void_leaves_the_arrival_open_and_drops_the_well_guard() {
    let (mut doc, ground, up) = two_storey();
    // Flight x 3500..4500, y 1000..3750, arriving on the second floor.
    let st = put_stair(&mut doc, stair(&ground, 4000.0, 1000.0, bars(900.0)));
    // The void covers the flight and more, and ends at its top.
    let v = put(&mut doc, slab(&up, SlabKind::Void, &rect(3000.0, 500.0, 5500.0, 3750.0), bars(1000.0)));
    let crosses_arrival = |r: &RailingRun| {
        r.points.windows(2).any(|w| {
            let mid = ((w[0].x + w[1].x) / 2.0, (w[0].y + w[1].y) / 2.0);
            (mid.1 - 3750.0).abs() < 1.0 && mid.0 > 3500.0 && mid.0 < 4500.0
        })
    };
    let void_runs = runs_of(&doc, &v);
    assert!(!void_runs.is_empty());
    assert!(void_runs.iter().all(|r| !crosses_arrival(r)), "a rail across the top of the stair: {void_runs:?}");
    // The void's rail stops either side of the arrival end.
    assert_eq!(void_runs.len(), 1);
    assert_eq!(
        xy(void_runs[0]),
        vec![(3450.0, 3750.0), (3000.0, 3750.0), (3000.0, 500.0), (5500.0, 500.0), (5500.0, 3750.0), (4550.0, 3750.0)],
        "{void_runs:?}"
    );
    let wells = runs_of(&doc, &st).into_iter().filter(|r| r.kind == RailingRunKind::StairWell).count();
    assert_eq!(wells, 0, "the well guard stood inside the void");
    // The stair still lands on floor.
    assert!(issues_with(&doc, "stair_top_no_floor").is_empty());

    // A void that stops halfway up the flight: its edge across the opening
    // goes, and the guard stays only where the opening is outside the void.
    doc.undo().unwrap();
    let v = put(&mut doc, slab(&up, SlabKind::Void, &rect(3000.0, 500.0, 5500.0, 2500.0), bars(1000.0)));
    let mut void_runs: Vec<Vec<(f64, f64)>> = runs_of(&doc, &v).iter().map(|r| xy(r)).collect();
    void_runs.sort_by(|x, y| x[0].partial_cmp(&y[0]).unwrap());
    assert_eq!(
        void_runs,
        vec![
            vec![(3500.0, 2500.0), (3000.0, 2500.0), (3000.0, 500.0), (5500.0, 500.0), (5500.0, 2500.0), (4500.0, 2500.0)],
        ],
        "the edge across the opening goes"
    );
    let mut wells: Vec<Vec<(f64, f64)>> =
        runs_of(&doc, &st).iter().filter(|r| r.kind == RailingRunKind::StairWell).map(|r| xy(r)).collect();
    wells.sort_by(|x, y| x[0].partial_cmp(&y[0]).unwrap());
    assert_eq!(wells, vec![vec![(3500.0, 3750.0), (3500.0, 2500.0)], vec![(4500.0, 2500.0), (4500.0, 3750.0)]]);

    // Along the opening's rim: a void that holds the opening keeps its rail
    // there; a void beside it, with the opening outside, needs none.
    doc.undo().unwrap();
    let holds = put(&mut doc, slab(&up, SlabKind::Void, &rect(3500.0, 500.0, 5500.0, 3750.0), bars(1000.0)));
    let west_rail = |doc: &Document, id: &str| {
        runs_of(doc, id).iter().any(|r| {
            r.points.windows(2).any(|w| (w[0].x - 3500.0).abs() < 1.0 && (w[1].x - 3500.0).abs() < 1.0 && (w[0].y - w[1].y).abs() > 1000.0)
        })
    };
    assert!(west_rail(&doc, &holds), "the rim of the opening inside the void is its edge");
    doc.undo().unwrap();
    let beside = put(&mut doc, slab(&up, SlabKind::Void, &rect(2500.0, 500.0, 3500.0, 3750.0), bars(1000.0)));
    assert!(!west_rail(&doc, &beside), "no rail between two openings");
    let well_west = runs_of(&doc, &st)
        .iter()
        .filter(|r| r.kind == RailingRunKind::StairWell)
        .any(|r| r.points.windows(2).any(|w| (w[0].x - 3500.0).abs() < 1.0 && (w[1].x - 3500.0).abs() < 1.0));
    assert!(!well_west, "nor a well guard there");
}

/// Gate P2: gross area takes off a balcony inside the walls (a loggia) as
/// docs/CONTRACT.md says, and floor area the part of the room it covers.
#[test]
fn a_loggia_inside_the_walls_comes_off_gross_and_floor_area() {
    let (mut doc, _, up) = two_storey();
    let (floor, gross) = (doc.derived().totals.floor_area_m2, doc.derived().totals.gross_area_m2);
    put(&mut doc, slab(&up, SlabKind::Balcony, &rect(1000.0, 1000.0, 3000.0, 2000.0), bars(1000.0)));
    let t = &doc.derived().totals;
    assert!((t.gross_area_m2 - (gross - 2.0)).abs() < 1e-6, "{}", t.gross_area_m2);
    assert!((t.floor_area_m2 - (floor - 2.0)).abs() < 1e-6, "{}", t.floor_area_m2);
}

/// Gate P2: `top_offset_mm` is from the level's floor; an elevation passed
/// by mistake is refused with a message that says so.
#[test]
fn a_top_offset_past_one_storey_is_refused() {
    let (mut doc, _, up) = two_storey();
    let mut b = slab(&up, SlabKind::Balcony, &rect(2000.0, -1575.0, 5000.0, -75.0), None);
    b.top_offset_mm = 3000.0;
    let e = apply_rejected(&mut doc, add(b.clone()));
    assert_eq!(code(&e), "slab_top_offset");
    assert!(e.to_string().contains("measured from the level's floor"), "{e}");
    b.top_offset_mm = -3000.0;
    assert_eq!(code(&apply_rejected(&mut doc, add(b.clone()))), "slab_top_offset");
    b.top_offset_mm = 2500.0;
    put(&mut doc, b);
    let mut d = slab(&up, SlabKind::Deck, &rect(-2075.0, 1000.0, -75.0, 3000.0), None);
    d.top_offset_mm = -450.0;
    put(&mut doc, d);
    // A void ignores it.
    let mut v = slab(&up, SlabKind::Void, &rect(1000.0, 1000.0, 2000.0, 2000.0), None);
    v.top_offset_mm = 9000.0;
    put(&mut doc, v);
}

#[test]
fn every_slab_kind_survives_preview_apply_undo_and_redo() {
    let (mut doc, _, up) = two_storey();
    for (kind, poly) in [
        (SlabKind::Floor, rect(8075.0, 1000.0, 9075.0, 3000.0)),
        (SlabKind::Balcony, rect(2000.0, -1575.0, 5000.0, -75.0)),
        (SlabKind::Deck, rect(-2075.0, 1000.0, -75.0, 3000.0)),
        (SlabKind::Void, rect(1000.0, 1000.0, 3000.0, 4000.0)),
    ] {
        put(&mut doc, slab(&up, kind, &poly, bars(1000.0)));
    }
    let with = doc.state();
    for _ in 0..4 {
        doc.undo().unwrap();
    }
    assert!(doc.project().elements.iter().all(|e| !matches!(e, Element::Slab(_))));
    for _ in 0..4 {
        doc.redo().unwrap();
    }
    assert_eq!(doc.state().derived, with.derived);
    assert_invariants(&doc.state());
    // A rotate keeps the outline counter-clockwise and the rules hold.
    let void = doc.project().elements.iter().find(|e| matches!(e, Element::Slab(s) if s.kind == SlabKind::Void)).unwrap().id().clone();
    apply_checked(&mut doc, Command::RotateElements { ids: vec![void.clone()], pivot: p(2000.0, 2500.0), angle_deg: 90.0 });
    let s = slab_of(&doc, &void);
    assert!(s.polygon.iter().all(|q| q.x > 400.0 && q.x < 3600.0));
}
