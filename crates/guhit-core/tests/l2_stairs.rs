//! Lane L2: `Derived::stairs` (DECISIONS D44, D54). Straight, L and U
//! flights, landings, nosings, the opening upstairs, a continuous walking
//! line, railings round flights and landings, and the stair review items
//! (headroom, short landings, the G4 landing walls).

mod common;

use common::*;
use guhit_core::Document;
use guhit_model::*;

const RISE: f64 = 3000.0;

fn bars() -> Option<Railing> {
    Some(Railing { height_mm: 900.0, style: RailingStyle::Bars })
}

fn add_level(doc: &mut Document, name: &str, elevation: f64) -> Id {
    apply_checked(
        doc,
        Command::AddLevel { name: Some(name.into()), elevation_mm: Some(elevation), height_mm: Some(3000.0) },
    );
    doc.project().levels.iter().find(|l| l.name == name).expect("level").id.clone()
}

/// A 10 by 8 m box on both floors, 150 mm walls.
fn two_storey() -> (Document, Id, Id) {
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 10000.0, 8000.0, Some("Living")));
    let up = add_level(&mut doc, "Second floor", RISE);
    apply_checked(
        &mut doc,
        Command::AddRectRoom {
            origin: p(0.0, 0.0),
            width_mm: 10000.0,
            depth_mm: 8000.0,
            name: Some("Hall".into()),
            thickness_mm: None,
            level_id: Some(up.clone()),
        },
    );
    (doc, LEVEL.to_string(), up)
}

#[allow(clippy::too_many_arguments)]
fn stair(level: &str, x: f64, y: f64, shape: StairShape, turn: Turn, risers: u32, run: f64, railing: Option<Railing>) -> Stair {
    Stair {
        id: String::new(),
        level_id: level.into(),
        origin: p(x, y),
        rotation_deg: 0.0,
        width_mm: 1000.0,
        run_mm: run,
        riser_count: risers,
        railing,
        shape,
        turn,
        first_flight_risers: None,
        landing_mm: None,
        well_mm: 0.0,
    }
}

fn put(doc: &mut Document, s: Stair) -> Id {
    apply_checked(doc, Command::AddElement { element: Element::Stair(s) }).diff.added[0].clone()
}

fn geometry<'a>(doc: &'a Document, id: &str) -> &'a StairGeometry {
    doc.derived().stairs.iter().find(|g| g.stair_id == id).expect("stair geometry")
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn issues_with(doc: &Document, code: &str) -> Vec<Issue> {
    doc.derived().issues.iter().filter(|i| i.code == code).cloned().collect()
}

/// Continuous from the floor to the floor above, never down, never more
/// than one riser at a time, and in plan never more than a landing apart.
fn assert_walk_line(g: &StairGeometry, rise: f64) {
    let w = &g.walk_line;
    assert!(w.len() >= 3, "{w:?}");
    assert!(near(w[0].z, 0.0), "starts on the floor");
    assert!(near(w[w.len() - 1].z, rise), "ends on the floor above: {}", w[w.len() - 1].z);
    for pair in w.windows(2) {
        let dz = pair[1].z - pair[0].z;
        assert!(dz >= -1e-9, "never down: {pair:?}");
        assert!(dz <= g.riser_mm + 1e-6, "no step over a riser: {dz} > {}", g.riser_mm);
        let d = (pair[1].x - pair[0].x).hypot(pair[1].y - pair[0].y);
        assert!(d > 1e-6 && d <= 2000.0, "continuous in plan: {d}");
    }
}

fn area(poly: &[Point]) -> f64 {
    common::area(poly).abs()
}

#[test]
fn a_straight_stair_has_n_minus_1_treads_and_a_walk_line() {
    let (mut doc, ground, up) = two_storey();
    let id = put(&mut doc, stair(&ground, 4000.0, 1000.0, StairShape::Straight, Turn::Left, 16, 3750.0, bars()));
    let g = geometry(&doc, &id);
    assert_eq!(g.arrives_at.as_deref(), Some(up.as_str()));
    assert_eq!(g.flights.len(), 1);
    assert!(g.landings.is_empty());
    assert_eq!(g.flights[0].nosings.len(), 15, "16 risers, 15 treads (D44)");
    assert!(near(g.tread_mm, 250.0));
    assert!(near(g.riser_mm, RISE / 16.0));
    // The first nosing is on the first riser, one riser up; the last one
    // tread short of the top, one riser under the floor above.
    let n = &g.flights[0].nosings;
    assert!(near(n[0].a.y, 1000.0) && near(n[0].z, RISE / 16.0));
    assert!(near(n[14].a.y, 1000.0 + 14.0 * 250.0) && near(n[14].z, RISE * 15.0 / 16.0));
    assert!(near(g.flights[0].up.y, 1.0));
    // The opening is the flight, as Wave 1 cut it.
    assert!(near(area(&g.opening), 1000.0 * 3750.0));
    assert_walk_line(g, RISE);
}

#[test]
fn an_l_stair_has_n_minus_2_treads_and_a_landing_at_its_risers() {
    for turn in [Turn::Left, Turn::Right] {
        let (mut doc, ground, up) = two_storey();
        // 17 risers: 9 then 8 (half rounded up), 15 treads of 250.
        let id = put(&mut doc, stair(&ground, 5000.0, 1000.0, StairShape::L, turn, 17, 3750.0, bars()));
        let g = geometry(&doc, &id).clone();
        assert_eq!(g.arrives_at.as_deref(), Some(up.as_str()));
        assert_eq!(g.flights.len(), 2);
        let treads: usize = g.flights.iter().map(|f| f.nosings.len()).sum();
        assert_eq!(treads, 15, "n - 2 treads");
        assert_eq!(g.flights[0].nosings.len(), 8, "9 risers in the first flight, its top one onto the landing");
        assert_eq!(g.flights[1].nosings.len(), 7);
        assert!(near(g.tread_mm, 250.0));
        let riser = RISE / 17.0;
        assert_eq!(g.landings.len(), 1);
        assert!(near(g.landings[0].z, 9.0 * riser), "the landing is the first flight's top");
        // The first flight runs 8 treads up +y; the landing is the width deep.
        let lp = &g.landings[0].polygon;
        let ys: Vec<f64> = lp.iter().map(|q| q.y).collect();
        assert!(near(ys.iter().cloned().fold(f64::INFINITY, f64::min), 1000.0 + 2000.0));
        assert!(near(ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max), 1000.0 + 3000.0));
        // The second flight turns: west on a left turn, east on a right turn.
        let up2 = g.flights[1].up;
        let east = if turn == Turn::Right { 1.0 } else { -1.0 };
        assert!(near(up2.x, east) && near(up2.y, 0.0), "{up2:?}");
        // Its first nosing is one riser over the landing.
        assert!(near(g.flights[1].nosings[0].z, 10.0 * riser));
        assert!(near(g.flights[1].nosings[0].a.x, 5000.0 + east * 500.0));
        // The opening covers both flights and the landing: an L.
        assert_eq!(g.opening.len(), 6, "{:?}", g.opening);
        assert!(near(area(&g.opening), 1000.0 * 3000.0 + 1000.0 * 1750.0));
        assert_walk_line(&g, RISE);
        // The walk line turns on the landing, at the landing's height.
        assert!(g.walk_line.iter().any(|v| near(v.z, 9.0 * riser) && near(v.x, 5000.0) && near(v.y, 3500.0)));
    }
}

#[test]
fn a_u_stair_turns_back_beside_itself_over_a_landing_that_spans_the_well() {
    let (mut doc, ground, _) = two_storey();
    let mut s = stair(&ground, 3000.0, 1000.0, StairShape::U, Turn::Right, 18, 4000.0, bars());
    s.well_mm = 200.0;
    s.first_flight_risers = Some(9);
    s.landing_mm = Some(1200.0);
    let id = put(&mut doc, s);
    let g = geometry(&doc, &id).clone();
    let riser = RISE / 18.0;
    assert!(near(g.tread_mm, 250.0), "16 treads of 250");
    assert_eq!(g.flights[0].nosings.len(), 8);
    assert_eq!(g.flights[1].nosings.len(), 8);
    assert!(near(g.flights[1].up.y, -1.0), "the second flight runs back down the plan");
    // The second flight sits a width and the well over to the right.
    assert!(near(g.flights[1].nosings[0].a.x.min(g.flights[1].nosings[0].b.x), 3000.0 + 700.0));
    assert!(near(g.flights[1].nosings[0].z, 10.0 * riser));
    let l = &g.landings[0];
    assert!(near(l.z, 9.0 * riser));
    let xs: Vec<f64> = l.polygon.iter().map(|q| q.x).collect();
    let ys: Vec<f64> = l.polygon.iter().map(|q| q.y).collect();
    assert!(near(xs.iter().cloned().fold(f64::INFINITY, f64::min), 2500.0));
    assert!(near(xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max), 4700.0));
    assert!(near(ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - ys.iter().cloned().fold(f64::INFINITY, f64::min), 1200.0));
    // The opening: both flights, the well between them and the landing.
    assert_eq!(g.opening.len(), 4);
    assert!(near(area(&g.opening), 2200.0 * (2000.0 + 1200.0)));
    assert_walk_line(&g, RISE);
    assert!(issues_with(&doc, "stair_landing_short").is_empty());
}

#[test]
fn turning_stairs_rail_their_flights_and_landings() {
    let (mut doc, ground, up) = two_storey();
    let id = put(&mut doc, stair(&ground, 5000.0, 1000.0, StairShape::L, Turn::Left, 17, 3750.0, bars()));
    let g = geometry(&doc, &id).clone();
    let sides: Vec<RailingRun> = doc
        .derived()
        .railings
        .iter()
        .filter(|r| r.host_id == id && r.kind == RailingRunKind::StairSide)
        .cloned()
        .collect();
    assert_eq!(sides.len(), 4, "outer and inner, for each flight");
    let riser = g.riser_mm;
    for run in &sides {
        // The foot is on the nosing line: never more than a flight's rise
        // above the floor, flat along the landing.
        for v in &run.points {
            assert!(v.z >= riser - 1e-6 && v.z <= RISE + 1e-6, "{v:?}");
        }
    }
    // The outer rail of the first flight runs on round the landing, flat at
    // its height.
    let outer = sides.iter().find(|r| r.points.len() == 3).expect("the outer rail round the landing");
    assert!(near(outer.points[1].z, g.landings[0].z) && near(outer.points[2].z, g.landings[0].z));
    // The guard upstairs goes round the opening, open at the arrival end.
    let well: Vec<&RailingRun> = doc
        .derived()
        .railings
        .iter()
        .filter(|r| r.host_id == id && r.kind == RailingRunKind::StairWell)
        .collect();
    assert_eq!(well.len(), 1);
    assert_eq!(well[0].level_id, up);
    let (l, r) = guhit_core::stairs::arrival_edge(&g).unwrap();
    let pts = &well[0].points;
    let ends = [p(pts[0].x, pts[0].y), p(pts[pts.len() - 1].x, pts[pts.len() - 1].y)];
    let at = |a: Point, b: Point| (a.x - b.x).hypot(a.y - b.y) < 1e-6;
    assert!((at(ends[0], l) && at(ends[1], r)) || (at(ends[0], r) && at(ends[1], l)), "{ends:?} vs {l:?} {r:?}");
}

#[test]
fn stacked_stairs_flag_low_headroom() {
    let (mut doc, ground, up) = two_storey();
    put(&mut doc, stair(&ground, 3000.0, 1000.0, StairShape::Straight, Turn::Left, 16, 3750.0, None));
    assert!(issues_with(&doc, "stair_headroom_low").is_empty(), "a plain stair has its opening");
    // A stair on the floor above, starting over the middle of the first.
    let top = add_level(&mut doc, "Roof deck", 2.0 * RISE);
    let _ = top;
    put(&mut doc, stair(&up, 3000.0, 2500.0, StairShape::Straight, Turn::Left, 16, 3750.0, None));
    let found = issues_with(&doc, "stair_headroom_low");
    assert!(!found.is_empty(), "{:?}", doc.derived().issues);
    assert!(found[0].message.contains("another stair"), "{}", found[0].message);
}

#[test]
fn a_shallow_landing_gets_a_review_item() {
    let (mut doc, ground, _) = two_storey();
    let mut s = stair(&ground, 5000.0, 1000.0, StairShape::L, Turn::Left, 17, 3750.0, None);
    s.landing_mm = Some(800.0);
    let id = put(&mut doc, s);
    let found = issues_with(&doc, "stair_landing_short");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].element_ids, vec![id]);
    assert!(found[0].message.contains("0.80 m"), "{}", found[0].message);
}

/// G4 problem 4: a first riser 75 mm from a wall, and the top of a turned
/// stair boxed in by walls, are flagged.
#[test]
fn landing_walls_are_flagged_for_turning_stairs_too() {
    let (mut doc, ground, _) = two_storey();
    // The south wall's inner face is y = 75: a first riser at y = 150.
    let id = put(&mut doc, stair(&ground, 5000.0, 150.0, StairShape::Straight, Turn::Left, 16, 3750.0, None));
    let found = issues_with(&doc, "stair_landing_blocked");
    assert!(found.iter().any(|i| i.element_ids[0] == id && i.message.contains("first step")), "{found:?}");
    // An L whose second flight runs into the west wall upstairs.
    let mut doc2 = two_storey().0;
    let l = put(&mut doc2, stair(&ground, 2550.0, 2000.0, StairShape::L, Turn::Left, 17, 3750.0, None));
    let found = issues_with(&doc2, "stair_landing_blocked");
    assert!(found.iter().any(|i| i.element_ids[0] == l && i.message.contains("top")), "{found:?}");
}

#[test]
fn an_l_or_u_under_four_risers_is_refused_and_described_with_its_treads() {
    let (mut doc, ground, _) = two_storey();
    let e = apply_rejected(
        &mut doc,
        Command::AddElement { element: Element::Stair(stair(&ground, 5000.0, 1000.0, StairShape::U, Turn::Left, 3, 1000.0, None)) },
    );
    assert_eq!(code(&e), "stair_size");
    let s = stair(&ground, 5000.0, 1000.0, StairShape::L, Turn::Left, 17, 3750.0, None);
    let id = put(&mut doc, s.clone());
    let mut stored = s;
    stored.id = id;
    let v = guhit_core::stairs::describe(doc.project(), &stored);
    assert_eq!(v["tread_count"], 15, "{v}");
    assert_eq!(v["first_flight_risers"], 9);
    assert_eq!(v["landing_mm"], 1000.0);
    assert_eq!(v["tread_mm"], 250.0);
}

/// Writes the engine's geometry of an L and a U stair, placed in the walk
/// tests' two storey bungalow (`src/viewer3d/walk/testModel.ts`, 3000 mm
/// levels), to `src/viewer3d/walk/turningStairs.fixture.json`, so walk mode
/// is tested on what the engine makes. Run it after a stair geometry change:
/// `cargo test -p guhit-core --test l2_stairs -- --ignored write_walk_fixture`.
#[test]
#[ignore]
fn write_walk_fixture() {
    let (mut doc, ground, _) = two_storey();
    let mut l = stair(&ground, 1000.0, 1000.0, StairShape::L, Turn::Right, 16, 3640.0, None);
    l.width_mm = 900.0;
    let mut u = stair(&ground, 5800.0, 1000.0, StairShape::U, Turn::Right, 16, 3640.0, None);
    u.width_mm = 900.0;
    u.well_mm = 100.0;
    let mut out = serde_json::Map::new();
    for (name, s) in [("l", l), ("u", u)] {
        let id = put(&mut doc, s);
        let mut el = serde_json::to_value(doc.project().elements.iter().find(|e| e.id() == &id).unwrap()).unwrap();
        el["id"] = serde_json::json!(format!("turning-{name}"));
        el["level_id"] = serde_json::json!("00000000-0000-4000-8000-0000000000a1");
        let mut g = serde_json::to_value(geometry(&doc, &id)).unwrap();
        g["stair_id"] = serde_json::json!(format!("turning-{name}"));
        g["level_id"] = serde_json::json!("00000000-0000-4000-8000-0000000000a1");
        g["arrives_at"] = serde_json::json!("test-level-2");
        out.insert(name.into(), serde_json::json!({ "element": el, "geometry": g }));
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/viewer3d/walk/turningStairs.fixture.json");
    std::fs::write(path, serde_json::to_string_pretty(&serde_json::Value::Object(out)).unwrap() + "\n").unwrap();
}
