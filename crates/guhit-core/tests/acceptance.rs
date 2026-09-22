//! Orchestrator acceptance tests. Written independently of the engine's own
//! tests, from the contract and the research happy path only.

use guhit_core::Document;
use guhit_model::*;

fn p(x: f64, y: f64) -> Point {
    Point { x, y }
}

fn doc() -> Document {
    Document::new(defaults::new_project("Acceptance"))
}

fn rooms(d: &Document) -> Vec<(String, f64)> {
    let mut out: Vec<(String, f64)> = d
        .derived()
        .rooms
        .iter()
        .map(|g| {
            let name = d
                .project()
                .elements
                .iter()
                .find_map(|e| match e {
                    Element::Room(r) if r.id == g.room_id => Some(r.name.clone()),
                    _ => None,
                })
                .expect("room geometry without a room element");
            (name, g.area_mm2)
        })
        .collect();
    out.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    out
}

fn wall(start: Point, end: Point, t: f64) -> Command {
    Command::AddWall { start, end, thickness_mm: Some(t), height_mm: None, material_id: None, level_id: None }
}

fn rect(w: f64, d: f64, name: &str) -> Command {
    Command::AddRectRoom { origin: p(0.0, 0.0), width_mm: w, depth_mm: d, name: Some(name.into()), thickness_mm: Some(150.0), level_id: None }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0 // 1 mm2
}

#[test]
fn happy_path_house() {
    let mut d = doc();
    d.apply(rect(8000.0, 6000.0, "Sala"), Origin::User).unwrap();
    let r = rooms(&d);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].0, "Sala");
    assert!(close(r[0].1, 7850.0 * 5850.0), "net area {}", r[0].1);

    // Partition splits it in two. Net areas must add up exactly.
    d.apply(wall(p(5000.0, 0.0), p(5000.0, 6000.0), 100.0), Origin::User).unwrap();
    let r = rooms(&d);
    assert_eq!(r.len(), 2, "{r:?}");
    assert!(close(r[0].1, 2875.0 * 5850.0), "{r:?}");
    assert!(close(r[1].1, 4875.0 * 5850.0), "{r:?}");
    assert!(r.iter().any(|(n, _)| n == "Sala"), "the original name must survive the split: {r:?}");
    assert!(close(d.derived().totals.floor_area_m2 * 1e6, r[0].1 + r[1].1));
    assert!(close(d.derived().totals.gross_area_m2 * 1e6, 8150.0 * 6150.0));

    // Door on the partition, window on the east wall.
    let partition = d.project().elements.iter().find_map(|e| match e {
        Element::Wall(w) if w.thickness_mm == 100.0 => Some(w.id.clone()),
        _ => None,
    }).unwrap();
    d.apply(Command::AddOpening { wall_id: partition.clone(), opening_type: OpeningType::Door, offset_mm: 3000.0, width_mm: None, height_mm: None, sill_mm: None, style: None, flip_side: None, flip_hinge: None }, Origin::User).unwrap();
    assert_eq!(d.derived().totals.door_count, 1);

    // An opening that does not fit is refused and changes nothing.
    let before = d.project().clone();
    let err = d.apply(Command::AddOpening { wall_id: partition.clone(), opening_type: OpeningType::Door, offset_mm: 5900.0, width_mm: Some(900.0), height_mm: None, sill_mm: None, style: None, flip_side: None, flip_hinge: None }, Origin::User);
    assert!(err.is_err());
    assert_eq!(d.project(), &before);
    // Overlapping the first door is refused too.
    assert!(d.apply(Command::AddOpening { wall_id: partition, opening_type: OpeningType::Window, offset_mm: 3200.0, width_mm: Some(600.0), height_mm: None, sill_mm: None, style: None, flip_side: None, flip_hinge: None }, Origin::User).is_err());

    // Resize the small room east by 300: its width grows by exactly 300.
    let small = d.derived().rooms.iter().min_by(|a, b| a.area_mm2.partial_cmp(&b.area_mm2).unwrap()).unwrap().room_id.clone();
    let before = d.project().clone();
    let cmd = Command::ResizeRoom { room_id: small, side: Side::East, delta_mm: 300.0 };
    let preview = d.preview(&cmd).unwrap();
    let applied = d.apply(cmd, Origin::Ai).unwrap();
    assert_eq!(preview.state.project.elements, applied.state.project.elements, "preview must equal apply");
    assert_eq!(preview.state.derived, applied.state.derived);
    assert_eq!(preview.diff, applied.diff);
    let r = rooms(&d);
    assert!(close(r[0].1, 3175.0 * 5850.0), "{r:?}");
    assert!(close(r[1].1, 4875.0 * 5850.0), "the neighbour must not change: {r:?}");

    // One undo restores the exact prior project.
    d.undo().unwrap();
    assert_eq!(d.project().elements, before.elements);
    d.redo().unwrap();
    assert_eq!(d.project().elements, applied.state.project.elements);
}

#[test]
fn hostile_input_is_rejected_and_harmless() {
    let mut d = doc();
    d.apply(rect(4000.0, 3000.0, "A"), Origin::User).unwrap();
    let before = d.project().clone();
    let bad = [
        wall(p(f64::NAN, 0.0), p(1000.0, 0.0), 150.0),
        wall(p(0.0, 0.0), p(f64::INFINITY, 0.0), 150.0),
        wall(p(0.0, 0.0), p(0.0, 0.0), 150.0),
        wall(p(0.0, 0.0), p(10.0, 0.0), 150.0),
        wall(p(0.0, 0.0), p(1000.0, 0.0), 0.0),
        wall(p(0.0, 0.0), p(1000.0, 0.0), -150.0),
        wall(p(0.0, 0.0), p(1000.0, 0.0), 1e9),
        Command::AddRectRoom { origin: p(0.0, 0.0), width_mm: -4000.0, depth_mm: 3000.0, name: None, thickness_mm: None, level_id: None },
        Command::DeleteElements { ids: vec!["nope".into()] },
        Command::ResizeRoom { room_id: "nope".into(), side: Side::North, delta_mm: 100.0 },
        Command::SetMaterial { ids: vec![], material_id: "mat-does-not-exist".into() },
        Command::Batch { label: "x".into(), commands: vec![wall(p(0.0, 9000.0), p(3000.0, 9000.0), 150.0), wall(p(0.0, 0.0), p(0.0, 0.0), 150.0)] },
    ];
    for c in bad {
        let r = d.apply(c.clone(), Origin::User);
        assert!(r.is_err(), "should be rejected: {c:?}");
        assert_eq!(d.project(), &before, "a rejected command changed the project: {c:?}");
    }
    // Shrinking a room through itself must not produce garbage.
    let room = d.derived().rooms[0].room_id.clone();
    let r = d.apply(Command::ResizeRoom { room_id: room, side: Side::East, delta_mm: -5000.0 }, Origin::User);
    if r.is_ok() {
        for g in &d.derived().rooms {
            assert!(g.area_mm2.is_finite() && g.area_mm2 > 0.0);
        }
    } else {
        assert_eq!(d.project(), &before);
    }
}

#[test]
fn crossing_and_far_away_geometry() {
    let mut d = doc();
    d.apply(rect(6000.0, 6000.0, "Big"), Origin::User).unwrap();
    // A diagonal wall corner to corner makes two triangles.
    d.apply(wall(p(0.0, 0.0), p(6000.0, 6000.0), 100.0), Origin::User).unwrap();
    let r = rooms(&d);
    assert_eq!(r.len(), 2, "{r:?}");
    assert!((r[0].1 - r[1].1).abs() < 1000.0, "symmetric halves: {r:?}");
    assert!(r[0].1 + r[1].1 < 5850.0 * 5850.0);
    // An X crossing in a far-away building, at large coordinates.
    let o = 5.0e6; // inside the engine's 10 km coordinate limit
    d.apply(Command::AddRectRoom { origin: p(o, o), width_mm: 4000.0, depth_mm: 4000.0, name: None, thickness_mm: Some(150.0), level_id: None }, Origin::User).unwrap();
    d.apply(wall(p(o + 2000.0, o), p(o + 2000.0, o + 4000.0), 100.0), Origin::User).unwrap();
    d.apply(wall(p(o, o + 2000.0), p(o + 4000.0, o + 2000.0), 100.0), Origin::User).unwrap();
    let r = rooms(&d);
    assert_eq!(r.len(), 6, "{r:?}");
    let quads: Vec<_> = r.iter().filter(|(_, a)| close(*a, 1875.0 * 1875.0)).collect();
    assert_eq!(quads.len(), 4, "four equal quadrants at 5e6 mm from origin: {r:?}");
    assert_eq!(d.derived().footprints.iter().filter(|f| !f.polygon.is_empty()).count(), 2);
}

#[test]
fn state_roundtrips_through_json() {
    let mut d = doc();
    d.apply(rect(4000.0, 3000.0, "A"), Origin::User).unwrap();
    let state = d.state();
    let json = serde_json::to_string(&state).unwrap();
    let back: DocState = serde_json::from_str(&json).unwrap();
    assert_eq!(back, state);
    // Reopening a saved project gives the same derived data.
    let reopened = Document::new(back.project.clone());
    assert_eq!(reopened.derived(), &state.derived);
}

#[test]
fn dimensions_never_disagree_with_the_walls_they_measure() {
    let mut d = Document::new(guhit_core::templates::sample_bungalow());
    let dim = |d: &Document| {
        d.project().elements.iter().find_map(|e| match e {
            Element::Dimension(x) => Some(((x.b.x - x.a.x).powi(2) + (x.b.y - x.a.y).powi(2)).sqrt()),
            _ => None,
        }).unwrap()
    };
    assert!((dim(&d) - 8000.0).abs() < 1e-6);
    let bedroom = d.project().elements.iter().find_map(|e| match e {
        Element::Room(r) if r.name == "Bedroom" => Some(r.id.clone()),
        _ => None,
    }).unwrap();
    let before = d.project().clone();
    let cmd = Command::ResizeRoom { room_id: bedroom, side: Side::East, delta_mm: 300.0 };
    let preview = d.preview(&cmd).unwrap();
    let applied = d.apply(cmd, Origin::Ai).unwrap();
    assert_eq!(preview.state.project.elements, applied.state.project.elements);
    assert!((dim(&d) - 8300.0).abs() < 1e-6, "dimension reads {} but the house is 8300 wide", dim(&d));
    // Shrink it back past the original and check again.
    let bedroom = d.derived().rooms.iter().min_by(|a, b| a.area_mm2.partial_cmp(&b.area_mm2).unwrap()).unwrap().room_id.clone();
    d.apply(Command::ResizeRoom { room_id: bedroom, side: Side::East, delta_mm: -800.0 }, Origin::User).unwrap();
    assert!((dim(&d) - 7500.0).abs() < 1e-6, "{}", dim(&d));
    d.undo().unwrap();
    d.undo().unwrap();
    assert_eq!(d.project().elements, before.elements);
}
