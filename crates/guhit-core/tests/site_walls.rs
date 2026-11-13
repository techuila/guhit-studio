//! Site walls (a lot fence, boundary walls) and moving walls with joined
//! walls on the far side of a joint. Both from the senior architect review
//! (R2 problems 2 and 6).

mod common;

use common::*;
use guhit_core::{templates, Document};
use guhit_model::*;

fn fence(points: &[Point]) -> Command {
    Command::AddWallChain {
        points: points.to_vec(),
        closed: true,
        thickness_mm: None,
        level_id: None,
        role: Some(WallRole::Site),
    }
}

/// The 12 x 15 m lot round the 8 x 6 m sample bungalow.
fn lot() -> [Point; 4] {
    [p(-2000.0, -4000.0), p(10_000.0, -4000.0), p(10_000.0, 11_000.0), p(-2000.0, 11_000.0)]
}

/// R2 problem 2: a closed fence round the lot made the yard a 92 m2 room,
/// every house wall interior, and the fence 3 m high in 3D.
#[test]
fn a_lot_fence_leaves_the_house_as_it_was() {
    let mut doc = Document::new(templates::sample_bungalow());
    let before = doc.state();
    let r = apply_checked(&mut doc, fence(&lot()));
    let after = &r.state;

    // The house: same rooms, same room polygons and areas, same totals.
    assert_eq!(rooms(&after.project).len(), rooms(&before.project).len(), "the yard is no room");
    assert_eq!(after.derived.rooms, before.derived.rooms);
    assert_eq!(after.derived.totals.floor_area_m2, before.derived.totals.floor_area_m2);
    assert_eq!(after.derived.totals.gross_area_m2, before.derived.totals.gross_area_m2);
    assert_eq!(after.derived.footprints, before.derived.footprints);
    // Every house wall reads exactly as before, exterior flags included.
    for g in &before.derived.walls {
        let now = after.derived.walls.iter().find(|w| w.wall_id == g.wall_id).unwrap();
        assert_eq!(now, g, "house wall {} changed", g.wall_id);
    }
    assert!(before.derived.walls.iter().any(|w| w.exterior));
    // No review item says a room lost its outside wall.
    let codes = |s: &DocState| {
        let mut c: Vec<String> = s.derived.issues.iter().map(|i| i.code.clone()).collect();
        c.sort();
        c
    };
    assert_eq!(codes(after), codes(&before), "the fence adds no review items");

    // The fence: four site walls, 1800 mm high, on the site layer, outside.
    let fence: Vec<&Wall> = walls(&after.project).into_iter().filter(|w| w.is_site()).collect();
    assert_eq!(fence.len(), 4);
    for w in &fence {
        assert_eq!(w.role, WallRole::Site);
        assert_eq!(w.height_mm, Some(defaults::SITE_WALL_HEIGHT_MM));
        assert_eq!(w.height_mm, Some(1800.0));
        assert_eq!(w.layer(), LayerKey::Site);
        let g = after.derived.walls.iter().find(|g| g.wall_id == w.id).unwrap();
        assert!(g.exterior);
        assert!(g.outline.len() >= 4, "the fence has mitred outlines");
        assert_eq!(g.joined_at_start.len(), 1, "fence corners join each other");
    }
    assert_eq!(r.diff.summary, "Add 4 site walls: added 4 walls");

    // One undo takes it back.
    let undone = doc.undo().unwrap();
    assert_eq!(undone.derived, before.derived);
}

#[test]
fn a_room_drawn_along_the_fence_still_closes() {
    let mut doc = blank();
    apply_checked(&mut doc, fence(&lot()));
    // A room whose south side lies on the fence line gets its own wall.
    let r = apply_checked(&mut doc, rect_room(0.0, -4000.0, 3000.0, 3000.0, Some("Shed")));
    let shed = room_named(&r.state.project, "Shed");
    let g = room_geo(&r.state, &shed.id);
    assert_eq!(g.area_mm2, 2850.0 * 2850.0);
    assert_eq!(rooms(&r.state.project).len(), 1);
}

#[test]
fn the_site_layer_locks_site_walls_only() {
    let mut doc = blank();
    let lock = |key: LayerKey, locked: bool| Command::SetLayer { layer: Layer { key, visible: true, locked } };
    apply_checked(&mut doc, lock(LayerKey::Site, true));
    let e = apply_rejected(&mut doc, fence(&lot()));
    assert_eq!(code(&e), "layer_locked");
    assert!(e.to_string().contains("Site layer"), "{e}");
    // A building wall is on the walls layer, which is open.
    apply_checked(&mut doc, add_wall(p(0.0, 0.0), p(4000.0, 0.0)));
    apply_checked(&mut doc, lock(LayerKey::Site, false));
    apply_checked(&mut doc, lock(LayerKey::Walls, true));
    apply_checked(&mut doc, fence(&lot()));
}

#[test]
fn a_wall_from_an_older_file_is_a_building_wall() {
    let json = r#"{"id":"w","level_id":"l","start":{"x":0,"y":0},"end":{"x":1,"y":0},"thickness_mm":150,"height_mm":null,"material_id":null}"#;
    let w: Wall = serde_json::from_str(json).unwrap();
    assert_eq!(w.role, WallRole::Building);
    // A building wall's command reads as it did, so its ids do not move.
    let cmd = add_wall(p(0.0, 0.0), p(1.0, 0.0));
    assert!(!serde_json::to_string(&cmd).unwrap().contains("role"));
}

// ---------------------------------------------------------- moving walls

const SOUTH: &str = "south";
const PART: &str = "partition";
const LANAI_E: &str = "lanai-east";
const COUNTER: &str = "counter";

/// R2's house: an 8 x 9 m shell with one south wall, a partition at x = 0
/// standing on it, a lanai to the south whose east wall continues the
/// partition's line below the south wall, and a counter wall resting on the
/// partition.
fn r2_house() -> Document {
    let mut project = blank_project();
    let wall = |id: &str, a: Point, b: Point, t: f64| {
        Element::Wall(Wall {
            id: id.into(),
            level_id: LEVEL.into(),
            start: a,
            end: b,
            thickness_mm: t,
            height_mm: None,
            material_id: None,
            role: WallRole::Building,
        })
    };
    project.elements = vec![
        wall(SOUTH, p(-4000.0, -4500.0), p(4000.0, -4500.0), 150.0),
        wall("east", p(4000.0, -4500.0), p(4000.0, 4500.0), 150.0),
        wall("north", p(4000.0, 4500.0), p(-4000.0, 4500.0), 150.0),
        wall("west", p(-4000.0, 4500.0), p(-4000.0, -4500.0), 150.0),
        wall("cross", p(-4000.0, 1000.0), p(4000.0, 1000.0), 100.0),
        wall(PART, p(0.0, -4500.0), p(0.0, 1000.0), 100.0),
        wall(COUNTER, p(-4000.0, -1500.0), p(0.0, -1500.0), 100.0),
        wall(LANAI_E, p(0.0, -6300.0), p(0.0, -4500.0), 150.0),
        wall("lanai-south", p(-4000.0, -6300.0), p(0.0, -6300.0), 150.0),
        wall("lanai-west", p(-4000.0, -4500.0), p(-4000.0, -6300.0), 150.0),
    ];
    Document::new(project)
}

fn move_partition(dx: f64, dy: f64) -> Command {
    Command::MoveElements {
        ids: vec![PART.into()],
        delta: p(dx, dy),
        stretch_connected: true,
    }
}

/// R2 problem 6: moving the partition 500 east turned the lanai's east wall
/// into an 1868 mm diagonal, and the result said only "changed 3 walls".
#[test]
fn moving_a_partition_keeps_the_lanai_wall_beyond_the_joint_straight() {
    let mut doc = r2_house();
    let r = apply_checked(&mut doc, move_partition(500.0, 0.0));
    let project = &r.state.project;

    let lanai = wall(project, LANAI_E);
    assert_eq!((lanai.start, lanai.end), (p(0.0, -6300.0), p(0.0, -4500.0)), "the lanai wall stays put");
    let part = wall(project, PART);
    assert_eq!((part.start, part.end), (p(500.0, -4500.0), p(500.0, 1000.0)));
    // The counter resting on the partition stretches along its own line.
    let counter = wall(project, COUNTER);
    assert_eq!((counter.start, counter.end), (p(-4000.0, -1500.0), p(500.0, -1500.0)));
    // Every wall is still square to the plan.
    for w in walls(project) {
        assert!(w.start.x == w.end.x || w.start.y == w.end.y, "wall {} turned", w.id);
    }
    // North, south-east, the two kitchen rooms either side of the counter,
    // and the lanai, which is still closed and as big as it was.
    assert_eq!(r.state.derived.rooms.len(), 5);
    let lanai_room = r
        .state
        .derived
        .rooms
        .iter()
        .find(|g| g.wall_ids.contains(&LANAI_E.to_string()))
        .expect("the lanai is still a room");
    assert!((lanai_room.area_mm2 - 3850.0 * 1650.0).abs() < 1e-3, "{}", lanai_room.area_mm2);

    // The result names the wall that moved without being named.
    assert_eq!(r.diff.also_moved, vec![COUNTER.to_string()]);
    assert!(r.diff.summary.ends_with("; also moved: Wall 4500 mm"), "{}", r.diff.summary);
}

/// When the joint would leave the line of the wall beyond it, keeping that
/// wall straight is impossible: the move is refused, naming it.
#[test]
fn a_move_that_would_turn_a_square_wall_is_refused_with_its_name() {
    let mut doc = r2_house();
    let e = apply_rejected(&mut doc, move_partition(500.0, 300.0));
    assert_eq!(code(&e), "stretch_turns_wall");
    assert_eq!(error_ids(&e), vec![LANAI_E.to_string()]);
    assert_eq!(
        e.to_string(),
        "This move would turn Wall 1800 mm out of line: its end at the joint would move 500 mm sideways. Select it too so it moves along, or move without stretching connected walls."
    );
    // Selecting it too moves both rigidly.
    let r = apply_checked(
        &mut doc,
        Command::MoveElements {
            ids: vec![PART.into(), LANAI_E.into()],
            delta: p(500.0, 0.0),
            stretch_connected: true,
        },
    );
    assert_eq!(wall(&r.state.project, LANAI_E).start, p(500.0, -6300.0));
    // Without stretching, nothing follows and nothing is refused.
    doc.undo().unwrap();
    apply_checked(
        &mut doc,
        Command::MoveElements {
            ids: vec![PART.into()],
            delta: p(500.0, 300.0),
            stretch_connected: false,
        },
    );
}

/// A diagonal wall was never square to anything, so it still follows the
/// joint as before; the result names it.
#[test]
fn a_diagonal_wall_still_follows_and_is_named() {
    let mut doc = blank();
    apply_checked(&mut doc, add_wall(p(0.0, 0.0), p(4000.0, 0.0)));
    apply_checked(&mut doc, add_wall(p(4000.0, 0.0), p(6000.0, 2000.0)));
    let ids: Vec<Id> = walls(doc.project()).iter().map(|w| w.id.clone()).collect();
    let r = apply_checked(
        &mut doc,
        Command::MoveElements {
            ids: vec![ids[0].clone()],
            delta: p(0.0, -500.0),
            stretch_connected: true,
        },
    );
    assert_eq!(wall(&r.state.project, &ids[1]).start, p(4000.0, -500.0));
    assert_eq!(r.diff.also_moved, vec![ids[1].clone()]);
}
