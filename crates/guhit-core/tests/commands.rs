//! One or more tests per command. Every successful command goes through
//! `apply_checked` (preview equals apply, invariants hold) and every rejected
//! one through `apply_rejected` (document untouched).

mod common;

use common::*;
use guhit_core::{templates, CoreError, Document};
use guhit_model::*;

const W1: &str = "00000000-0000-4000-8000-000000000101";
const W2: &str = "00000000-0000-4000-8000-000000000102";
const W3: &str = "00000000-0000-4000-8000-000000000103";
const W4: &str = "00000000-0000-4000-8000-000000000104";
const W5: &str = "00000000-0000-4000-8000-000000000105";
const DOOR_FRONT: &str = "00000000-0000-4000-8000-000000000201";
const DOOR_BED: &str = "00000000-0000-4000-8000-000000000202";
const LIVING: &str = "00000000-0000-4000-8000-000000000301";
const BEDROOM: &str = "00000000-0000-4000-8000-000000000302";
const BED: &str = "00000000-0000-4000-8000-000000000401";
const DIM: &str = "00000000-0000-4000-8000-000000000501";
const CAMERA: &str = "00000000-0000-4000-8000-000000000601";

fn bungalow() -> Document {
    Document::new(templates::sample_bungalow())
}

fn layer(key: LayerKey, locked: bool) -> Command {
    Command::SetLayer {
        layer: Layer {
            key,
            visible: true,
            locked,
        },
    }
}

// ------------------------------------------------------------------ AddWall

#[test]
fn add_wall_uses_project_defaults() {
    let mut doc = blank();
    let r = apply_checked(&mut doc, add_wall(p(0.0, 0.0), p(4000.0, 0.0)));
    let w = walls(&r.state.project)[0];
    assert_eq!(w.thickness_mm, 150.0);
    assert_eq!(w.level_id, LEVEL);
    assert_eq!(w.height_mm, None);
    assert_eq!(w.material_id.as_deref(), Some(defaults::MAT_WALL_DEFAULT));
    assert_eq!(r.diff.added, vec![w.id.clone()]);
    assert_eq!(r.state.undo_label.as_deref(), Some("Add wall"));
    assert_eq!(r.diff.summary, "Add wall: added 1 wall");
}

#[test]
fn add_wall_validation() {
    let mut doc = blank();
    let e = apply_rejected(&mut doc, add_wall(p(0.0, 0.0), p(49.0, 0.0)));
    assert_eq!(code(&e), "wall_too_short");
    assert!(e.to_string().contains("50 mm"));
    let e = apply_rejected(&mut doc, add_wall(p(0.0, f64::NAN), p(4000.0, 0.0)));
    assert_eq!(code(&e), "not_finite");
    let e = apply_rejected(&mut doc, add_wall(p(0.0, 0.0), p(f64::INFINITY, 0.0)));
    assert_eq!(code(&e), "not_finite");
    let e = apply_rejected(&mut doc, add_wall(p(0.0, 0.0), p(5.0e7, 0.0)));
    assert_eq!(code(&e), "out_of_range");
    for t in [49.0, 1001.0] {
        let e = apply_rejected(
            &mut doc,
            Command::AddWall {
                start: p(0.0, 0.0),
                end: p(1000.0, 0.0),
                thickness_mm: Some(t),
                height_mm: None,
                material_id: None,
                level_id: None,
            },
        );
        assert_eq!(code(&e), "wall_thickness");
    }
    let e = apply_rejected(
        &mut doc,
        Command::AddWall {
            start: p(0.0, 0.0),
            end: p(1000.0, 0.0),
            thickness_mm: None,
            height_mm: None,
            material_id: Some("mat-nope".into()),
            level_id: None,
        },
    );
    assert_eq!(code(&e), "unknown_material");
    let e = apply_rejected(
        &mut doc,
        Command::AddWall {
            start: p(0.0, 0.0),
            end: p(1000.0, 0.0),
            thickness_mm: None,
            height_mm: None,
            material_id: None,
            level_id: Some("level-nope".into()),
        },
    );
    assert_eq!(code(&e), "unknown_level");
    assert_eq!(doc.revision(), 0);
    assert!(!doc.state().can_undo);
}

// ------------------------------------------------------------- AddWallChain

#[test]
fn add_wall_chain_open_and_closed() {
    let mut doc = blank();
    let r = apply_checked(
        &mut doc,
        Command::AddWallChain {
            points: vec![p(0.0, 0.0), p(3000.0, 0.0), p(3000.0, 3000.0)],
            closed: false,
            thickness_mm: Some(100.0),
            level_id: None,
        },
    );
    assert_eq!(walls(&r.state.project).len(), 2);
    assert!(rooms(&r.state.project).is_empty());
    let r = apply_checked(
        &mut doc,
        Command::AddWallChain {
            points: vec![p(3000.0, 3000.0), p(0.0, 3000.0), p(0.0, 0.0)],
            closed: false,
            thickness_mm: Some(100.0),
            level_id: None,
        },
    );
    assert_eq!(
        rooms(&r.state.project).len(),
        1,
        "closing the loop makes a room in the same step"
    );
    assert_eq!(rooms(&r.state.project)[0].name, "Room 1");
    assert_eq!(r.diff.added.len(), 3);
    doc.undo().unwrap();
    assert!(
        rooms(doc.project()).is_empty(),
        "one undo removes the walls and the room"
    );

    let e = apply_rejected(
        &mut doc,
        Command::AddWallChain {
            points: vec![p(0.0, 0.0)],
            closed: false,
            thickness_mm: None,
            level_id: None,
        },
    );
    assert_eq!(code(&e), "chain_too_short");
    let e = apply_rejected(
        &mut doc,
        Command::AddWallChain {
            points: vec![p(9000.0, 0.0), p(9900.0, 0.0), p(9910.0, 0.0)],
            closed: false,
            thickness_mm: None,
            level_id: None,
        },
    );
    assert_eq!(code(&e), "wall_too_short");
}

// -------------------------------------------------------------- AddRectRoom

#[test]
fn add_rect_room_names_the_room_and_validates() {
    let mut doc = blank();
    let r = apply_checked(
        &mut doc,
        rect_room(0.0, 0.0, 4000.0, 3000.0, Some("  Kitchen ")),
    );
    let room = rooms(&r.state.project)[0];
    assert_eq!(room.name, "Kitchen");
    assert!(!room.auto_named);
    let r = apply_checked(&mut doc, rect_room(10_000.0, 0.0, 3000.0, 3000.0, None));
    assert!(rooms(&r.state.project)
        .iter()
        .any(|x| x.name == "Room 1" && x.auto_named));

    let e = apply_rejected(&mut doc, rect_room(0.0, 0.0, 4000.0, 3000.0, None));
    assert_eq!(code(&e), "room_exists");
    let e = apply_rejected(&mut doc, rect_room(20_000.0, 0.0, 100.0, 3000.0, None));
    assert_eq!(code(&e), "room_too_small");
    let e = apply_rejected(&mut doc, rect_room(20_000.0, 0.0, f64::NAN, 3000.0, None));
    assert_eq!(code(&e), "not_finite");
}

// --------------------------------------------------------- SetWallEndpoints

#[test]
fn set_wall_endpoints_drags_joined_walls_and_keeps_openings_in_place() {
    let mut doc = bungalow();
    // Pull the east wall 1 m further east.
    let r = apply_checked(
        &mut doc,
        Command::SetWallEndpoints {
            wall_id: W2.into(),
            start: p(9000.0, 0.0),
            end: p(9000.0, 6000.0),
        },
    );
    assert_eq!(wall(&r.state.project, W1).end, p(9000.0, 0.0));
    assert_eq!(wall(&r.state.project, W3).start, p(9000.0, 6000.0));
    assert_eq!(
        r.diff.summary,
        "Moved 1 wall, stretched 2 walls, updated 1 dimension"
    );
    // The south dimension was snapped to both ends of the south wall, so it
    // still measures it.
    assert_eq!(dimension(&r.state.project, DIM).b, p(9000.0, 0.0));
    // The north wall starts at the moved corner. Its window stays at x = 2500.
    let win = openings(&r.state.project)
        .into_iter()
        .find(|o| o.id.ends_with("205"))
        .unwrap();
    assert_eq!(win.offset_mm, 6500.0);
    let bed = room_geo(&r.state, BEDROOM);
    assert_eq!(bbox(&bed.polygon), (5050.0, 75.0, 8925.0, 5925.0));

    let e = apply_rejected(
        &mut doc,
        Command::SetWallEndpoints {
            wall_id: "nope".into(),
            start: p(0.0, 0.0),
            end: p(1.0, 1.0),
        },
    );
    assert_eq!(code(&e), "unknown_wall");
    let e = apply_rejected(
        &mut doc,
        Command::SetWallEndpoints {
            wall_id: LIVING.into(),
            start: p(0.0, 0.0),
            end: p(1000.0, 1.0),
        },
    );
    assert_eq!(code(&e), "not_a_wall");
}

#[test]
fn wall_edit_that_pushes_an_opening_off_is_rejected_with_the_opening_id() {
    let mut doc = bungalow();
    // Wall 2 has a window centered at 3000. Shorten it to 2 m.
    let e = apply_rejected(
        &mut doc,
        Command::SetWallEndpoints {
            wall_id: W2.into(),
            start: p(8000.0, 0.0),
            end: p(8000.0, 2000.0),
        },
    );
    assert_eq!(code(&e), "opening_outside_wall");
    assert_eq!(error_ids(&e)[0], "00000000-0000-4000-8000-000000000204");
    let e = apply_rejected(
        &mut doc,
        Command::SetWallLength {
            wall_id: W5.into(),
            length_mm: 3000.0,
            anchor: WallAnchor::Start,
        },
    );
    assert_eq!(code(&e), "opening_outside_wall");
    assert_eq!(error_ids(&e)[0], DOOR_BED);
}

// ------------------------------------------------------------ SetWallLength

#[test]
fn set_wall_length_moves_the_cross_walls_and_keeps_the_room_square() {
    let mut doc = blank();
    apply_checked(
        &mut doc,
        rect_room(0.0, 0.0, 4000.0, 3000.0, Some("Room A")),
    );
    let south = wall_at(doc.project(), p(2000.0, 0.0)).id.clone();
    let r = apply_checked(
        &mut doc,
        Command::SetWallLength {
            wall_id: south.clone(),
            length_mm: 5000.0,
            anchor: WallAnchor::Start,
        },
    );
    let g = room_geo(&r.state, &room_named(&r.state.project, "Room A").id);
    assert_eq!(
        g.polygon,
        vec![
            p(75.0, 75.0),
            p(4925.0, 75.0),
            p(4925.0, 2925.0),
            p(75.0, 2925.0)
        ]
    );
    let east = wall_at(&r.state.project, p(5000.0, 1500.0));
    assert_eq!((east.start, east.end), (p(5000.0, 0.0), p(5000.0, 3000.0)));

    // Anchor end: the start moves west, the west wall goes with it.
    let r = apply_checked(
        &mut doc,
        Command::SetWallLength {
            wall_id: south.clone(),
            length_mm: 6000.0,
            anchor: WallAnchor::End,
        },
    );
    let w = wall(&r.state.project, &south);
    assert_eq!((w.start, w.end), (p(-1000.0, 0.0), p(5000.0, 0.0)));
    assert_eq!(
        bbox(&room_geo(&r.state, &rooms(&r.state.project)[0].id).polygon),
        (-925.0, 75.0, 4925.0, 2925.0)
    );

    // Anchor center.
    let r = apply_checked(
        &mut doc,
        Command::SetWallLength {
            wall_id: south.clone(),
            length_mm: 5000.0,
            anchor: WallAnchor::Center,
        },
    );
    let w = wall(&r.state.project, &south);
    assert_eq!((w.start, w.end), (p(-500.0, 0.0), p(4500.0, 0.0)));
    assert_eq!(rooms(&r.state.project).len(), 1);

    let e = apply_rejected(
        &mut doc,
        Command::SetWallLength {
            wall_id: south.clone(),
            length_mm: 10.0,
            anchor: WallAnchor::Start,
        },
    );
    assert_eq!(code(&e), "wall_too_short");
    let e = apply_rejected(
        &mut doc,
        Command::SetWallLength {
            wall_id: south,
            length_mm: f64::NAN,
            anchor: WallAnchor::Start,
        },
    );
    assert_eq!(code(&e), "not_finite");
}

// ---------------------------------------------------------------- SplitWall

#[test]
fn split_wall_rehosts_openings_with_corrected_offsets() {
    let mut doc = bungalow();
    // Wall 1: door at 1500 (w 900), window at 3500 (w 1500). Split at 2400.
    let r = apply_checked(
        &mut doc,
        Command::SplitWall {
            wall_id: W1.into(),
            at_mm: 2400.0,
        },
    );
    assert_eq!(r.diff.added.len(), 1);
    let new_id = r.diff.added[0].clone();
    let first = wall(&r.state.project, W1);
    let second = wall(&r.state.project, &new_id);
    assert_eq!((first.start, first.end), (p(0.0, 0.0), p(2400.0, 0.0)));
    assert_eq!((second.start, second.end), (p(2400.0, 0.0), p(8000.0, 0.0)));
    assert_eq!(second.thickness_mm, 150.0);
    let door = openings(&r.state.project)
        .into_iter()
        .find(|o| o.id == DOOR_FRONT)
        .unwrap();
    assert_eq!((door.wall_id.as_str(), door.offset_mm), (W1, 1500.0));
    let window = openings(&r.state.project)
        .into_iter()
        .find(|o| o.id.ends_with("203"))
        .unwrap();
    assert_eq!(
        (window.wall_id.clone(), window.offset_mm),
        (new_id.clone(), 1100.0)
    );
    // Rooms keep their names and areas.
    assert_eq!(room_geo(&r.state, LIVING).area_mm2, 4875.0 * 5850.0);
    assert_eq!(room_named(&r.state.project, "Living / Dining").id, LIVING);
    assert!(room_geo(&r.state, LIVING).wall_ids.contains(&new_id));

    let e = apply_rejected(
        &mut doc,
        Command::SplitWall {
            wall_id: W2.into(),
            at_mm: 3100.0,
        },
    );
    assert_eq!(code(&e), "split_hits_opening");
    assert_eq!(error_ids(&e)[0], "00000000-0000-4000-8000-000000000204");
    for at in [10.0, 5990.0, f64::NAN] {
        let e = apply_rejected(
            &mut doc,
            Command::SplitWall {
                wall_id: W4.into(),
                at_mm: at,
            },
        );
        assert_eq!(code(&e), "bad_split");
    }
}

// --------------------------------------------------------------- AddOpening

#[test]
fn add_opening_defaults_and_validation() {
    let mut doc = bungalow();
    let r = apply_checked(&mut doc, opening(W4, OpeningType::Door, 3000.0));
    let door = openings(&r.state.project)
        .into_iter()
        .find(|o| o.id == r.diff.added[0])
        .unwrap();
    assert_eq!(
        (door.width_mm, door.height_mm, door.sill_mm),
        (900.0, 2100.0, 0.0)
    );
    let r = apply_checked(&mut doc, opening(W4, OpeningType::Window, 1000.0));
    let win = openings(&r.state.project)
        .into_iter()
        .find(|o| o.id == r.diff.added[0])
        .unwrap();
    assert_eq!(
        (win.width_mm, win.height_mm, win.sill_mm),
        (1200.0, 1200.0, 900.0)
    );
    assert_eq!(r.state.derived.totals.door_count, 3);
    assert_eq!(r.state.derived.totals.window_count, 4);

    // 50 mm end clearance: a 900 door needs its center at 500 or more.
    let e = apply_rejected(&mut doc, opening(W3, OpeningType::Door, 499.0));
    assert_eq!(code(&e), "opening_outside_wall");
    assert!(e.to_string().contains("50 mm"));
    apply_checked(&mut doc, opening(W3, OpeningType::Door, 500.0));
    let e = apply_rejected(&mut doc, opening(W3, OpeningType::Door, 7600.0));
    assert_eq!(code(&e), "opening_outside_wall");

    // Overlap with the window at 3000 on wall 2.
    let e = apply_rejected(&mut doc, opening(W2, OpeningType::Door, 3900.0));
    assert_eq!(code(&e), "opening_overlap");
    assert!(error_ids(&e).contains(&"00000000-0000-4000-8000-000000000204".to_string()));

    let e = apply_rejected(
        &mut doc,
        Command::AddOpening {
            wall_id: W2.into(),
            opening_type: OpeningType::Window,
            offset_mm: 1000.0,
            width_mm: Some(800.0),
            height_mm: Some(2500.0),
            sill_mm: Some(900.0),
            style: None,
            flip_side: None,
            flip_hinge: None,
        },
    );
    assert_eq!(code(&e), "opening_too_tall");
    let e = apply_rejected(&mut doc, opening("nope", OpeningType::Door, 1000.0));
    assert_eq!(code(&e), "unknown_wall");
    let e = apply_rejected(&mut doc, opening(W2, OpeningType::Door, f64::NAN));
    assert_eq!(code(&e), "not_finite");
}

#[test]
fn opening_labels_say_door_or_window_whichever_command_made_it() {
    let mut doc = blank();
    apply_checked(&mut doc, add_wall(p(0.0, 0.0), p(8000.0, 0.0)));
    let wall_id = walls(doc.project())[0].id.clone();

    let r = apply_checked(&mut doc, opening(&wall_id, OpeningType::Door, 1000.0));
    assert_eq!(r.state.undo_label.as_deref(), Some("Add door"));
    let r = apply_checked(&mut doc, opening(&wall_id, OpeningType::Window, 6000.0));
    assert_eq!(r.state.undo_label.as_deref(), Some("Add window"));

    // The whole-element path, which the canvas uses for a flipped opening.
    let r = apply_checked(
        &mut doc,
        Command::AddElement {
            element: Element::Opening(Opening {
                id: String::new(),
                wall_id: wall_id.clone(),
                opening_type: OpeningType::Door,
                style: OpeningStyle::SwingSingle,
                offset_mm: 4000.0,
                width_mm: 900.0,
                height_mm: 2100.0,
                sill_mm: 0.0,
                flip_side: true,
                flip_hinge: false,
                material_id: None,
            }),
        },
    );
    assert_eq!(r.state.undo_label.as_deref(), Some("Add door"));
}

#[test]
fn add_opening_carries_the_flip_flags_and_none_means_false() {
    let mut doc = blank();
    apply_checked(&mut doc, add_wall(p(0.0, 0.0), p(8000.0, 0.0)));
    let wall_id = walls(doc.project())[0].id.clone();

    let r = apply_checked(&mut doc, opening(&wall_id, OpeningType::Door, 1000.0));
    let plain = openings(&r.state.project)
        .into_iter()
        .find(|o| o.id == r.diff.added[0])
        .unwrap()
        .clone();
    assert!(!plain.flip_side && !plain.flip_hinge, "None means false");

    let r = apply_checked(
        &mut doc,
        Command::AddOpening {
            wall_id: wall_id.clone(),
            opening_type: OpeningType::Door,
            offset_mm: 4000.0,
            width_mm: None,
            height_mm: None,
            sill_mm: None,
            style: None,
            flip_side: Some(true),
            flip_hinge: Some(true),
        },
    );
    let flipped = openings(&r.state.project)
        .into_iter()
        .find(|o| o.id == r.diff.added[0])
        .unwrap();
    assert!(flipped.flip_side && flipped.flip_hinge);
}

#[test]
fn an_overlap_names_the_opening_that_moved_first() {
    let mut doc = blank();
    apply_checked(&mut doc, add_wall(p(0.0, 0.0), p(8000.0, 0.0)));
    let wall_id = walls(doc.project())[0].id.clone();
    let r = apply_checked(&mut doc, opening(&wall_id, OpeningType::Door, 1000.0));
    let door_id = r.diff.added[0].clone();
    let r = apply_checked(&mut doc, opening(&wall_id, OpeningType::Window, 3000.0));
    let window_id = r.diff.added[0].clone();

    // Drag the door east onto the window. The door ends up first along the
    // wall, but it is the one that moved, so it must be named first.
    let mut door = openings(doc.project())
        .into_iter()
        .find(|o| o.id == door_id)
        .unwrap()
        .clone();
    door.offset_mm = 2500.0;
    let e = apply_rejected(
        &mut doc,
        Command::UpdateElement {
            element: Element::Opening(door),
        },
    );
    assert_eq!(code(&e), "opening_overlap");
    assert!(
        e.to_string().contains("The door would overlap a window"),
        "{e}"
    );
    assert_eq!(error_ids(&e), vec![door_id.clone(), window_id.clone()]);

    // The other way round: move the window onto the door.
    let mut window = openings(doc.project())
        .into_iter()
        .find(|o| o.id == window_id)
        .unwrap()
        .clone();
    window.offset_mm = 1600.0;
    let e = apply_rejected(
        &mut doc,
        Command::UpdateElement {
            element: Element::Opening(window),
        },
    );
    assert_eq!(code(&e), "opening_overlap");
    assert!(
        e.to_string().contains("The window would overlap a door"),
        "{e}"
    );
    assert_eq!(error_ids(&e), vec![window_id, door_id]);
}

// --------------------------------------------------------------- ResizeRoom

#[test]
fn resize_room_validation_and_shrink() {
    let mut doc = bungalow();
    let r = apply_checked(
        &mut doc,
        Command::ResizeRoom {
            room_id: BEDROOM.into(),
            side: Side::North,
            delta_mm: -1000.0,
        },
    );
    // The north wall is shared, so both rooms get shallower.
    assert_eq!(
        bbox(&room_geo(&r.state, BEDROOM).polygon),
        (5050.0, 75.0, 7925.0, 4925.0)
    );
    assert_eq!(
        bbox(&room_geo(&r.state, LIVING).polygon),
        (75.0, 75.0, 4950.0, 4925.0)
    );
    assert_eq!(
        wall(&r.state.project, W5).end,
        p(5000.0, 5000.0),
        "the partition shortens with the wall it meets"
    );

    let e = apply_rejected(
        &mut doc,
        Command::ResizeRoom {
            room_id: LIVING.into(),
            side: Side::East,
            delta_mm: -6000.0,
        },
    );
    assert_eq!(code(&e), "resize_breaks_room");
    let e = apply_rejected(
        &mut doc,
        Command::ResizeRoom {
            room_id: LIVING.into(),
            side: Side::East,
            delta_mm: 0.0,
        },
    );
    assert_eq!(code(&e), "bad_delta");
    let e = apply_rejected(
        &mut doc,
        Command::ResizeRoom {
            room_id: W1.into(),
            side: Side::East,
            delta_mm: 100.0,
        },
    );
    assert_eq!(code(&e), "not_a_room");
    let e = apply_rejected(
        &mut doc,
        Command::ResizeRoom {
            room_id: "nope".into(),
            side: Side::East,
            delta_mm: 100.0,
        },
    );
    assert!(matches!(e, CoreError::NotFound(_)));
}

#[test]
fn resize_keeps_a_straight_neighbor_wall_in_place_when_a_cross_wall_can_close_the_gap() {
    // Two rooms stacked north-south. Their east sides are two separate
    // walls in one line, and the wall between the rooms ends at that joint.
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 4000.0, 3000.0, Some("South")));
    apply_checked(
        &mut doc,
        rect_room(0.0, 3000.0, 4000.0, 3000.0, Some("North")),
    );
    assert_eq!(walls(doc.project()).len(), 7);
    let south_id = room_named(doc.project(), "South").id.clone();
    let north_id = room_named(doc.project(), "North").id.clone();

    let r = apply_checked(
        &mut doc,
        Command::ResizeRoom {
            room_id: south_id.clone(),
            side: Side::East,
            delta_mm: 500.0,
        },
    );
    assert_eq!(
        bbox(&room_geo(&r.state, &south_id).polygon),
        (75.0, 75.0, 4425.0, 2925.0)
    );
    assert_eq!(
        bbox(&room_geo(&r.state, &north_id).polygon),
        (75.0, 3075.0, 3925.0, 5925.0),
        "the north room keeps its shape"
    );
    let north_east = wall_at(&r.state.project, p(4000.0, 4500.0));
    assert_eq!(
        (north_east.start.x, north_east.end.x),
        (4000.0, 4000.0),
        "the neighbor wall did not go slanted"
    );

    // Shrinking works too: the moved wall lands on the wall between the rooms.
    let r = apply_checked(
        &mut doc,
        Command::ResizeRoom {
            room_id: south_id.clone(),
            side: Side::East,
            delta_mm: -1000.0,
        },
    );
    assert_eq!(
        bbox(&room_geo(&r.state, &south_id).polygon),
        (75.0, 75.0, 3425.0, 2925.0)
    );
    assert_eq!(
        bbox(&room_geo(&r.state, &north_id).polygon),
        (75.0, 3075.0, 3925.0, 5925.0)
    );
}

// ------------------------------------------- AddElement / Update / Delete

fn column(id: &str) -> Element {
    Element::Column(Column {
        id: id.into(),
        level_id: LEVEL.into(),
        center: p(1000.0, 1000.0),
        shape: ColumnShape::Rect,
        width_mm: 300.0,
        depth_mm: 300.0,
        rotation_deg: 0.0,
        material_id: None,
    })
}

#[test]
fn add_element_assigns_ids_and_validates() {
    let mut doc = blank();
    let r = apply_checked(
        &mut doc,
        Command::AddElement {
            element: column(""),
        },
    );
    assert_eq!(r.diff.added.len(), 1);
    assert_eq!(r.diff.added[0].len(), 36, "assigned ids are UUID shaped");
    let r = apply_checked(
        &mut doc,
        Command::AddElement {
            element: column("col-2"),
        },
    );
    assert_eq!(r.diff.added, vec!["col-2".to_string()]);
    let e = apply_rejected(
        &mut doc,
        Command::AddElement {
            element: column("col-2"),
        },
    );
    assert_eq!(code(&e), "duplicate_id");

    let mut bad = column("");
    if let Element::Column(c) = &mut bad {
        c.width_mm = 0.0;
    }
    assert_eq!(
        code(&apply_rejected(
            &mut doc,
            Command::AddElement { element: bad }
        )),
        "column_size"
    );
    let mut bad = column("");
    if let Element::Column(c) = &mut bad {
        c.level_id = "nope".into();
    }
    assert_eq!(
        code(&apply_rejected(
            &mut doc,
            Command::AddElement { element: bad }
        )),
        "unknown_level"
    );
    let mut bad = column("");
    if let Element::Column(c) = &mut bad {
        c.center = p(f64::NAN, 0.0);
    }
    assert_eq!(
        code(&apply_rejected(
            &mut doc,
            Command::AddElement { element: bad }
        )),
        "not_finite"
    );

    // A wall or opening added as an element gets the same rules as AddWall and AddOpening.
    let short = Element::Wall(Wall {
        id: String::new(),
        level_id: LEVEL.into(),
        start: p(0.0, 0.0),
        end: p(10.0, 0.0),
        thickness_mm: 150.0,
        height_mm: None,
        material_id: None,
    });
    assert_eq!(
        code(&apply_rejected(
            &mut doc,
            Command::AddElement { element: short }
        )),
        "wall_too_short"
    );
}

#[test]
fn add_room_element_needs_a_free_closed_face() {
    let mut doc = bungalow();
    let room = |seed: Point| {
        Element::Room(Room {
            id: String::new(),
            level_id: "00000000-0000-4000-8000-0000000000a1".into(),
            name: "Extra".into(),
            usage: RoomUsage::Storage,
            seed,
            floor_material_id: None,
            auto_named: false,
        })
    };
    let e = apply_rejected(
        &mut doc,
        Command::AddElement {
            element: room(p(20_000.0, 0.0)),
        },
    );
    assert_eq!(code(&e), "room_not_enclosed");
    let e = apply_rejected(
        &mut doc,
        Command::AddElement {
            element: room(p(1000.0, 1000.0)),
        },
    );
    assert_eq!(code(&e), "room_exists");
    assert_eq!(error_ids(&e), vec![LIVING.to_string()]);
}

#[test]
fn update_element_replaces_and_refuses_a_kind_change() {
    let mut doc = bungalow();
    let mut living = rooms(doc.project())
        .into_iter()
        .find(|r| r.id == LIVING)
        .unwrap()
        .clone();
    living.name = "Sala".into();
    living.usage = RoomUsage::Living;
    let r = apply_checked(
        &mut doc,
        Command::UpdateElement {
            element: Element::Room(living),
        },
    );
    assert_eq!(r.diff.modified, vec![LIVING.to_string()]);
    assert_eq!(room_named(&r.state.project, "Sala").id, LIVING);

    let mut door = openings(doc.project())
        .into_iter()
        .find(|o| o.id == DOOR_BED)
        .unwrap()
        .clone();
    door.width_mm = 5000.0;
    door.offset_mm = 3000.0;
    apply_checked(
        &mut doc,
        Command::UpdateElement {
            element: Element::Opening(door.clone()),
        },
    );
    door.width_mm = 5950.0;
    let e = apply_rejected(
        &mut doc,
        Command::UpdateElement {
            element: Element::Opening(door),
        },
    );
    assert_eq!(code(&e), "opening_outside_wall");

    let e = apply_rejected(
        &mut doc,
        Command::UpdateElement {
            element: column(W1),
        },
    );
    assert_eq!(code(&e), "kind_mismatch");
    let e = apply_rejected(
        &mut doc,
        Command::UpdateElement {
            element: column("nope"),
        },
    );
    assert!(matches!(e, CoreError::NotFound(_)));

    let mut w = wall(doc.project(), W2).clone();
    w.thickness_mm = 2000.0;
    let e = apply_rejected(
        &mut doc,
        Command::UpdateElement {
            element: Element::Wall(w),
        },
    );
    assert_eq!(code(&e), "wall_thickness");
}

#[test]
fn auto_named_room_stops_being_auto_when_renamed() {
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 4000.0, 3000.0, None));
    let mut room = rooms(doc.project())[0].clone();
    assert!(room.auto_named);
    room.name = "Study".into();
    let r = apply_checked(
        &mut doc,
        Command::UpdateElement {
            element: Element::Room(room),
        },
    );
    assert!(!rooms(&r.state.project)[0].auto_named);
}

#[test]
fn delete_elements_takes_hosted_openings_and_reconciles_rooms() {
    let mut doc = bungalow();
    // Deleting the partition merges the rooms. The older room survives.
    let r = apply_checked(
        &mut doc,
        Command::DeleteElements {
            ids: vec![W5.into()],
        },
    );
    assert!(r.diff.removed.contains(&W5.to_string()));
    assert!(
        r.diff.removed.contains(&DOOR_BED.to_string()),
        "the door goes with its wall"
    );
    assert!(
        r.diff.removed.contains(&BEDROOM.to_string()),
        "two seeds in one face: the newer room goes"
    );
    assert_eq!(rooms(&r.state.project).len(), 1);
    assert_eq!(rooms(&r.state.project)[0].id, LIVING);
    assert_eq!(room_geo(&r.state, LIVING).area_mm2, 7850.0 * 5850.0);

    // Deleting a room gives the face a fresh auto-named room.
    let r = apply_checked(
        &mut doc,
        Command::DeleteElements {
            ids: vec![LIVING.into()],
        },
    );
    assert_eq!(rooms(&r.state.project).len(), 1);
    assert!(rooms(&r.state.project)[0].auto_named);
    assert_eq!(rooms(&r.state.project)[0].name, "Room 1");

    // Opening the shell removes the room.
    let r = apply_checked(
        &mut doc,
        Command::DeleteElements {
            ids: vec![W2.into()],
        },
    );
    assert!(rooms(&r.state.project).is_empty());
    assert!(r.state.derived.footprints[0].polygon.is_empty());

    let e = apply_rejected(
        &mut doc,
        Command::DeleteElements {
            ids: vec!["nope".into()],
        },
    );
    assert!(matches!(e, CoreError::NotFound(_)));
    let e = apply_rejected(&mut doc, Command::DeleteElements { ids: vec![] });
    assert_eq!(code(&e), "empty_selection");
}

// ------------------------------------------------------------- MoveElements

#[test]
fn move_wall_with_stretch_keeps_joints_without_stretch_breaks_them() {
    let mut doc = bungalow();
    let r = apply_checked(
        &mut doc,
        Command::MoveElements {
            ids: vec![W2.into()],
            delta: p(1000.0, 0.0),
            stretch_connected: true,
        },
    );
    assert_eq!(wall(&r.state.project, W1).end, p(9000.0, 0.0));
    assert_eq!(wall(&r.state.project, W3).start, p(9000.0, 6000.0));
    assert_eq!(
        r.diff.summary,
        "Moved 1 wall, stretched 2 walls, updated 1 dimension"
    );
    assert_eq!(dimension(&r.state.project, DIM).b, p(9000.0, 0.0));
    assert_eq!(rooms(&r.state.project).len(), 2);
    assert_eq!(
        bbox(&room_geo(&r.state, BEDROOM).polygon),
        (5050.0, 75.0, 8925.0, 5925.0)
    );
    doc.undo().unwrap();

    let r = apply_checked(
        &mut doc,
        Command::MoveElements {
            ids: vec![W2.into()],
            delta: p(1000.0, 0.0),
            stretch_connected: false,
        },
    );
    assert_eq!(wall(&r.state.project, W1).end, p(8000.0, 0.0));
    assert_eq!(
        dimension(&r.state.project, DIM).b,
        p(8000.0, 0.0),
        "the joint broke: the south wall stayed, so the dimension stays with it"
    );
    assert_eq!(
        rooms(&r.state.project).len(),
        1,
        "the bedroom is open now, so its room is gone"
    );
    assert!(r.diff.removed.contains(&BEDROOM.to_string()));
}

#[test]
fn moving_a_partition_carries_its_t_joints_and_door() {
    let mut doc = bungalow();
    let r = apply_checked(
        &mut doc,
        Command::MoveElements {
            ids: vec![W5.into(), DOOR_BED.into()],
            delta: p(-500.0, 0.0),
            stretch_connected: true,
        },
    );
    assert_eq!(wall(&r.state.project, W5).start, p(4500.0, 0.0));
    let door = openings(&r.state.project)
        .into_iter()
        .find(|o| o.id == DOOR_BED)
        .unwrap();
    assert_eq!(door.offset_mm, 3000.0, "the door rides with its host");
    assert_eq!(room_geo(&r.state, LIVING).area_mm2, 4375.0 * 5850.0);
}

#[test]
fn moving_a_whole_room_keeps_its_name() {
    let mut doc = blank();
    apply_checked(
        &mut doc,
        rect_room(0.0, 0.0, 4000.0, 3000.0, Some("Office")),
    );
    let ids: Vec<Id> = walls(doc.project()).iter().map(|w| w.id.clone()).collect();
    let room_id = rooms(doc.project())[0].id.clone();
    let r = apply_checked(
        &mut doc,
        Command::MoveElements {
            ids,
            delta: p(20_000.0, 5000.0),
            stretch_connected: true,
        },
    );
    let room = rooms(&r.state.project)[0];
    assert_eq!(
        (room.id.as_str(), room.name.as_str()),
        (room_id.as_str(), "Office")
    );
    assert_eq!(
        room.seed,
        p(22_000.0, 6500.0),
        "the seed moved to the new label point"
    );
    assert_eq!(room_geo(&r.state, &room_id).area_mm2, 3850.0 * 2850.0);
}

#[test]
fn move_other_elements_and_slide_an_opening() {
    let mut doc = bungalow();
    let r = apply_checked(
        &mut doc,
        Command::MoveElements {
            ids: vec![BED.into(), DIM.into(), CAMERA.into(), DOOR_FRONT.into()],
            delta: p(200.0, -100.0),
            stretch_connected: false,
        },
    );
    let bed = r
        .state
        .project
        .elements
        .iter()
        .find(|e| e.id() == BED)
        .unwrap();
    assert!(matches!(bed, Element::Asset(a) if a.position == p(7100.0, 4700.0)));
    let dim = r
        .state
        .project
        .elements
        .iter()
        .find(|e| e.id() == DIM)
        .unwrap();
    assert!(
        matches!(dim, Element::Dimension(d) if d.a == p(200.0, -100.0) && d.b == p(8200.0, -100.0))
    );
    let cam = r
        .state
        .project
        .elements
        .iter()
        .find(|e| e.id() == CAMERA)
        .unwrap();
    assert!(matches!(cam, Element::Camera(c) if c.position.x == -4800.0 && c.position.z == 4500.0));
    let door = openings(&r.state.project)
        .into_iter()
        .find(|o| o.id == DOOR_FRONT)
        .unwrap();
    assert_eq!(
        door.offset_mm, 1700.0,
        "an opening slides along its wall by the part of the move along the wall"
    );

    // Sliding the door into the window is refused.
    let e = apply_rejected(
        &mut doc,
        Command::MoveElements {
            ids: vec![DOOR_FRONT.into()],
            delta: p(1000.0, 0.0),
            stretch_connected: false,
        },
    );
    assert_eq!(code(&e), "opening_overlap");
    let e = apply_rejected(
        &mut doc,
        Command::MoveElements {
            ids: vec![BED.into()],
            delta: p(f64::NAN, 0.0),
            stretch_connected: false,
        },
    );
    assert_eq!(code(&e), "not_finite");
}

// ----------------------------------------------------------- RotateElements

#[test]
fn rotate_a_room_keeps_area_name_and_openings() {
    let mut doc = bungalow();
    let ids: Vec<Id> = doc
        .project()
        .elements
        .iter()
        .map(|e| e.id().clone())
        .collect();
    let r = apply_checked(
        &mut doc,
        Command::RotateElements {
            ids,
            pivot: p(4000.0, 3000.0),
            angle_deg: 90.0,
        },
    );
    let living = room_geo(&r.state, LIVING);
    assert!((living.area_mm2 - 4875.0 * 5850.0).abs() < 1e-3);
    assert_eq!(rooms(&r.state.project).len(), 2);
    let w1 = wall(&r.state.project, W1);
    assert!((w1.start.x - 7000.0).abs() < 1e-6 && (w1.start.y + 1000.0).abs() < 1e-6);
    let bed = r
        .state
        .project
        .elements
        .iter()
        .find(|e| e.id() == BED)
        .unwrap();
    assert!(matches!(bed, Element::Asset(a) if a.rotation_deg == 90.0));
    assert_eq!(openings(&r.state.project).len(), 5);

    let e = apply_rejected(
        &mut doc,
        Command::RotateElements {
            ids: vec![DOOR_BED.into()],
            pivot: p(0.0, 0.0),
            angle_deg: 45.0,
        },
    );
    assert_eq!(code(&e), "nothing_to_rotate");
    let e = apply_rejected(
        &mut doc,
        Command::RotateElements {
            ids: vec![W1.into()],
            pivot: p(0.0, 0.0),
            angle_deg: f64::INFINITY,
        },
    );
    assert_eq!(code(&e), "not_finite");
}

// -------------------------------------------------------- DuplicateElements

#[test]
fn duplicate_walls_brings_openings_and_makes_new_rooms() {
    let mut doc = bungalow();
    let r = apply_checked(
        &mut doc,
        Command::DuplicateElements {
            ids: vec![
                W1.into(),
                W2.into(),
                W3.into(),
                W4.into(),
                W5.into(),
                BED.into(),
            ],
            delta: p(20_000.0, 0.0),
        },
    );
    assert_eq!(walls(&r.state.project).len(), 10);
    assert_eq!(openings(&r.state.project).len(), 10);
    assert_eq!(rooms(&r.state.project).len(), 4);
    assert_eq!(r.diff.added.len(), 5 + 5 + 1 + 2);
    assert!(r.diff.modified.is_empty() && r.diff.removed.is_empty());
    let copy = wall_at(&r.state.project, p(24_000.0, 0.0));
    assert_ne!(copy.id, W1);
    let hosted: Vec<f64> = openings(&r.state.project)
        .iter()
        .filter(|o| o.wall_id == copy.id)
        .map(|o| o.offset_mm)
        .collect();
    assert_eq!(hosted, vec![1500.0, 3500.0]);
    let mut names: Vec<&str> = rooms(&r.state.project)
        .iter()
        .map(|x| x.name.as_str())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec!["Bedroom", "Living / Dining", "Room 1", "Room 2"]
    );

    // Duplicating an opening alone slides the copy along the host.
    let r = apply_checked(
        &mut doc,
        Command::DuplicateElements {
            ids: vec![DOOR_FRONT.into()],
            delta: p(4500.0, 0.0),
        },
    );
    let copy = openings(&r.state.project)
        .into_iter()
        .find(|o| o.id == r.diff.added[0])
        .unwrap();
    assert_eq!((copy.wall_id.as_str(), copy.offset_mm), (W1, 6000.0));
    let e = apply_rejected(
        &mut doc,
        Command::DuplicateElements {
            ids: vec![DOOR_FRONT.into()],
            delta: p(100.0, 0.0),
        },
    );
    assert_eq!(code(&e), "opening_overlap");
}

#[test]
fn duplicating_a_room_with_its_walls_carries_the_name() {
    let mut doc = blank();
    apply_checked(
        &mut doc,
        rect_room(0.0, 0.0, 4000.0, 3000.0, Some("Bedroom")),
    );
    let ids: Vec<Id> = doc
        .project()
        .elements
        .iter()
        .map(|e| e.id().clone())
        .collect();
    let r = apply_checked(
        &mut doc,
        Command::DuplicateElements {
            ids,
            delta: p(10_000.0, 0.0),
        },
    );
    let names: Vec<&str> = rooms(&r.state.project)
        .iter()
        .map(|x| x.name.as_str())
        .collect();
    assert_eq!(names, vec!["Bedroom", "Bedroom"]);
}

// ------------------------------------------------ materials, roof, settings

#[test]
fn set_material_goes_to_the_right_slot_per_kind() {
    let mut doc = bungalow();
    let r = apply_checked(
        &mut doc,
        Command::SetMaterial {
            ids: vec![W1.into(), LIVING.into(), DOOR_FRONT.into()],
            material_id: "mat-chb-bare".into(),
        },
    );
    assert_eq!(
        wall(&r.state.project, W1).material_id.as_deref(),
        Some("mat-chb-bare")
    );
    assert_eq!(
        room_named(&r.state.project, "Living / Dining")
            .floor_material_id
            .as_deref(),
        Some("mat-chb-bare")
    );
    let door = openings(&r.state.project)
        .into_iter()
        .find(|o| o.id == DOOR_FRONT)
        .unwrap();
    assert_eq!(door.material_id.as_deref(), Some("mat-chb-bare"));
    assert_eq!(r.diff.modified.len(), 3);

    let e = apply_rejected(
        &mut doc,
        Command::SetMaterial {
            ids: vec![W1.into()],
            material_id: "mat-nope".into(),
        },
    );
    assert_eq!(code(&e), "unknown_material");
    let e = apply_rejected(
        &mut doc,
        Command::SetMaterial {
            ids: vec![W1.into(), DIM.into()],
            material_id: "mat-chb-bare".into(),
        },
    );
    assert_eq!(code(&e), "no_material_slot");
    assert_eq!(error_ids(&e), vec![DIM.to_string()]);
}

fn material(id: &str, color: &str) -> Material {
    Material {
        id: id.into(),
        name: "Narra".into(),
        category: MaterialCategory::Wood,
        color: color.into(),
        roughness: 0.6,
        metalness: 0.0,
        opacity: 1.0,
        pattern: MaterialPattern::WoodPlank,
        builtin: true,
    }
}

#[test]
fn upsert_material_inserts_replaces_and_validates() {
    let mut doc = blank();
    let count = doc.project().materials.len();
    let r = apply_checked(
        &mut doc,
        Command::UpsertMaterial {
            material: material("", "#8a5a2b"),
        },
    );
    assert_eq!(r.state.project.materials.len(), count + 1);
    let added = r.state.project.materials.last().unwrap().clone();
    assert_eq!(added.id.len(), 36);
    assert!(!added.builtin, "a user material is never builtin");
    assert_eq!(r.state.undo_label.as_deref(), Some("Add material"));

    let mut edit = added.clone();
    edit.color = "#112233".into();
    let r = apply_checked(&mut doc, Command::UpsertMaterial { material: edit });
    assert_eq!(r.state.project.materials.len(), count + 1);
    assert_eq!(r.state.project.materials.last().unwrap().color, "#112233");
    doc.undo().unwrap();
    assert_eq!(doc.project().materials.last().unwrap().color, "#8a5a2b");

    for bad in ["red", "#12345", "#gggggg"] {
        let e = apply_rejected(
            &mut doc,
            Command::UpsertMaterial {
                material: material("", bad),
            },
        );
        assert_eq!(code(&e), "bad_color");
    }
    let mut m = material("", "#ffffff");
    m.opacity = 1.5;
    assert_eq!(
        code(&apply_rejected(
            &mut doc,
            Command::UpsertMaterial { material: m }
        )),
        "bad_material_value"
    );
    let mut m = material("", "#ffffff");
    m.name = "  ".into();
    assert_eq!(
        code(&apply_rejected(
            &mut doc,
            Command::UpsertMaterial { material: m }
        )),
        "bad_name"
    );
}

#[test]
fn set_roof_and_project_settings() {
    let mut doc = blank();
    let mut roof = doc.project().roof.clone();
    roof.kind = RoofKind::Gable;
    roof.pitch_deg = 30.0;
    let r = apply_checked(&mut doc, Command::SetRoof { roof: roof.clone() });
    assert_eq!(r.state.project.roof, roof);
    roof.pitch_deg = 89.0;
    assert_eq!(
        code(&apply_rejected(
            &mut doc,
            Command::SetRoof { roof: roof.clone() }
        )),
        "bad_roof"
    );
    roof.pitch_deg = 20.0;
    roof.material_id = Some("mat-nope".into());
    assert_eq!(
        code(&apply_rejected(&mut doc, Command::SetRoof { roof })),
        "unknown_material"
    );

    let mut settings = doc.project().settings.clone();
    settings.default_wall_thickness_mm = 100.0;
    settings.location = "Cebu".into();
    apply_checked(
        &mut doc,
        Command::SetProjectSettings {
            settings: settings.clone(),
        },
    );
    let r = apply_checked(&mut doc, add_wall(p(0.0, 0.0), p(1000.0, 0.0)));
    assert_eq!(walls(&r.state.project)[0].thickness_mm, 100.0);
    settings.default_wall_thickness_mm = 10.0;
    assert_eq!(
        code(&apply_rejected(
            &mut doc,
            Command::SetProjectSettings {
                settings: settings.clone()
            }
        )),
        "wall_thickness"
    );
    settings.default_wall_thickness_mm = 150.0;
    settings.grid_mm = 0.0;
    assert_eq!(
        code(&apply_rejected(
            &mut doc,
            Command::SetProjectSettings {
                settings: settings.clone()
            }
        )),
        "bad_settings"
    );
    settings.grid_mm = 100.0;
    settings.scale_denominator = 0;
    assert_eq!(
        code(&apply_rejected(
            &mut doc,
            Command::SetProjectSettings { settings }
        )),
        "bad_settings"
    );
}

#[test]
fn update_level_checks_openings_against_the_new_height() {
    let mut doc = bungalow();
    let mut level = doc.project().levels[0].clone();
    level.name = "Ground".into();
    level.height_mm = 2700.0;
    let r = apply_checked(
        &mut doc,
        Command::UpdateLevel {
            level: level.clone(),
        },
    );
    assert_eq!(r.state.project.levels[0].height_mm, 2700.0);

    level.height_mm = 2000.0;
    let e = apply_rejected(
        &mut doc,
        Command::UpdateLevel {
            level: level.clone(),
        },
    );
    assert_eq!(code(&e), "opening_too_tall");
    assert!(!error_ids(&e).is_empty());
    level.height_mm = 500.0;
    assert_eq!(
        code(&apply_rejected(
            &mut doc,
            Command::UpdateLevel {
                level: level.clone()
            }
        )),
        "bad_level"
    );
    level.height_mm = 3000.0;
    level.id = "nope".into();
    assert_eq!(
        code(&apply_rejected(
            &mut doc,
            Command::UpdateLevel {
                level: level.clone()
            }
        )),
        "unknown_level"
    );
    level.id = doc.project().levels[0].id.clone();
    level.name = String::new();
    assert_eq!(
        code(&apply_rejected(&mut doc, Command::UpdateLevel { level })),
        "bad_name"
    );
}

// ------------------------------------------------------------ locked layers

#[test]
fn locked_layers_refuse_edits_and_name_the_layer() {
    let mut doc = bungalow();
    apply_checked(&mut doc, layer(LayerKey::Walls, true));
    let cases = vec![
        add_wall(p(0.0, -3000.0), p(4000.0, -3000.0)),
        rect_room(20_000.0, 0.0, 3000.0, 3000.0, None),
        Command::DeleteElements {
            ids: vec![W5.into()],
        },
        Command::MoveElements {
            ids: vec![W2.into()],
            delta: p(100.0, 0.0),
            stretch_connected: true,
        },
        Command::ResizeRoom {
            room_id: LIVING.into(),
            side: Side::East,
            delta_mm: 300.0,
        },
        Command::SplitWall {
            wall_id: W4.into(),
            at_mm: 3000.0,
        },
        Command::SetMaterial {
            ids: vec![W1.into()],
            material_id: "mat-chb-bare".into(),
        },
        Command::DuplicateElements {
            ids: vec![W1.into()],
            delta: p(0.0, -5000.0),
        },
    ];
    for cmd in cases {
        let e = apply_rejected(&mut doc, cmd);
        assert_eq!(code(&e), "layer_locked");
        assert!(e.to_string().contains("Walls layer is locked"), "{e}");
    }
    // Other layers still work, and unlocking restores editing.
    apply_checked(&mut doc, opening(W4, OpeningType::Window, 3000.0));
    apply_checked(&mut doc, layer(LayerKey::Walls, false));
    apply_checked(&mut doc, add_wall(p(0.0, -3000.0), p(4000.0, -3000.0)));

    // A locked Openings layer stops a wall delete that would take a door with it.
    apply_checked(&mut doc, layer(LayerKey::Openings, true));
    let e = apply_rejected(
        &mut doc,
        Command::DeleteElements {
            ids: vec![W5.into()],
        },
    );
    assert!(e.to_string().contains("Openings layer is locked"));
    assert_eq!(error_ids(&e), vec![DOOR_BED.to_string()]);
    // Openings may still ride along when their wall moves.
    apply_checked(
        &mut doc,
        Command::ResizeRoom {
            room_id: LIVING.into(),
            side: Side::West,
            delta_mm: 300.0,
        },
    );
}

#[test]
fn locked_underlay_cannot_move_until_unlocked() {
    let mut doc = blank();
    let underlay = Underlay {
        id: "u1".into(),
        level_id: LEVEL.into(),
        file_name: "plan.png".into(),
        position: p(0.0, 0.0),
        width_px: 1000,
        height_px: 800,
        mm_per_px: 10.0,
        scale_confirmed: true,
        rotation_deg: 0.0,
        opacity: 0.5,
        locked: true,
    };
    apply_checked(
        &mut doc,
        Command::AddElement {
            element: Element::Underlay(underlay.clone()),
        },
    );
    let mv = Command::MoveElements {
        ids: vec!["u1".into()],
        delta: p(100.0, 0.0),
        stretch_connected: false,
    };
    assert_eq!(
        code(&apply_rejected(&mut doc, mv.clone())),
        "underlay_locked"
    );
    assert_eq!(
        code(&apply_rejected(
            &mut doc,
            Command::DeleteElements {
                ids: vec!["u1".into()]
            }
        )),
        "underlay_locked"
    );
    let mut unlocked = underlay;
    unlocked.locked = false;
    apply_checked(
        &mut doc,
        Command::UpdateElement {
            element: Element::Underlay(unlocked),
        },
    );
    apply_checked(&mut doc, mv);
}

// ------------------------------------------------ dimensions follow geometry

/// A dimension between two plan points, added to the bungalow.
fn add_dimension(a: Point, b: Point) -> Command {
    Command::AddElement {
        element: Element::Dimension(Dimension {
            id: String::new(),
            level_id: "00000000-0000-4000-8000-0000000000a1".into(),
            a,
            b,
            offset_mm: -900.0,
            text_override: None,
        }),
    }
}

#[test]
fn the_south_dimension_follows_a_resize_and_still_reads_true() {
    let mut doc = bungalow();
    assert_eq!(dimension_reads(doc.project(), DIM), 8000.0);
    let r = apply_checked(
        &mut doc,
        Command::ResizeRoom {
            room_id: BEDROOM.into(),
            side: Side::East,
            delta_mm: 300.0,
        },
    );
    let dim = dimension(&r.state.project, DIM);
    assert_eq!((dim.a, dim.b), (p(0.0, 0.0), p(8300.0, 0.0)));
    assert_eq!(dimension_reads(&r.state.project, DIM), 8300.0);
    let south = wall(&r.state.project, W1);
    assert_eq!(
        dimension_reads(&r.state.project, DIM),
        south.end.x - south.start.x,
        "the dimension measures the wall it was drawn on"
    );
    assert!(r.diff.modified.contains(&DIM.to_string()));
    assert!(
        r.diff.summary.ends_with(", updated 1 dimension"),
        "{}",
        r.diff.summary
    );

    // One undo brings the whole step back, dimension included.
    let s = doc.undo().unwrap();
    assert_eq!(
        (dimension(&s.project, DIM).a, dimension(&s.project, DIM).b),
        (p(0.0, 0.0), p(8000.0, 0.0))
    );
    assert_eq!(wall(&s.project, W1).end, p(8000.0, 0.0));
}

#[test]
fn an_interior_face_to_face_dimension_follows_the_wall_it_measures() {
    let mut doc = bungalow();
    // Inner face of the west wall to the west face of the partition.
    let r = apply_checked(&mut doc, add_dimension(p(75.0, 75.0), p(4950.0, 75.0)));
    let id = r.diff.added[0].clone();
    assert_eq!(dimension_reads(&r.state.project, &id), 4875.0);

    // Push the partition 1 m east.
    let r = apply_checked(
        &mut doc,
        Command::MoveElements {
            ids: vec![W5.into()],
            delta: p(1000.0, 0.0),
            stretch_connected: true,
        },
    );
    let dim = dimension(&r.state.project, &id);
    assert_eq!(dim.a, p(75.0, 75.0), "the west face did not move");
    assert!(
        (dim.b.x - 5950.0).abs() < 1e-6 && (dim.b.y - 75.0).abs() < 1e-6,
        "face dimension did not follow the partition: {:?}",
        dim.b
    );
    assert!((dimension_reads(&r.state.project, &id) - 5875.0).abs() < 1e-6);
    assert_eq!(
        bbox(&room_geo(&r.state, LIVING).polygon),
        (75.0, 75.0, 5950.0, 5925.0),
        "and it still matches the room it measures"
    );
    assert!(r.diff.modified.contains(&id));
}

#[test]
fn a_dimension_attached_to_nothing_stays_put() {
    let mut doc = bungalow();
    // 3 m south of the house, snapped to no wall at all.
    let free = apply_checked(&mut doc, add_dimension(p(0.0, -3000.0), p(8000.0, -3000.0)));
    let id = free.diff.added[0].clone();
    let r = apply_checked(
        &mut doc,
        Command::ResizeRoom {
            room_id: BEDROOM.into(),
            side: Side::East,
            delta_mm: 300.0,
        },
    );
    let dim = dimension(&r.state.project, &id);
    assert_eq!((dim.a, dim.b), (p(0.0, -3000.0), p(8000.0, -3000.0)));
    assert!(!r.diff.modified.contains(&id));
    // The snapped one did move, so this is not a case of nothing happening.
    assert!(r.diff.modified.contains(&DIM.to_string()));
    assert_eq!(r.diff.summary.matches("dimension").count(), 1);
}

#[test]
fn a_selected_dimension_moves_once_not_twice() {
    let mut doc = bungalow();
    let r = apply_checked(
        &mut doc,
        Command::MoveElements {
            ids: vec![W2.into(), DIM.into()],
            delta: p(1000.0, 0.0),
            stretch_connected: true,
        },
    );
    let dim = dimension(&r.state.project, DIM);
    assert_eq!(
        (dim.a, dim.b),
        (p(1000.0, 0.0), p(9000.0, 0.0)),
        "the selection moved it once; following must not move it again"
    );
    assert!(
        !r.diff.summary.contains("dimension"),
        "{}",
        r.diff.summary
    );
}

#[test]
fn a_rotated_selection_moves_its_dimension_once() {
    let mut doc = bungalow();
    let ids: Vec<Id> = doc
        .project()
        .elements
        .iter()
        .map(|e| e.id().clone())
        .collect();
    let r = apply_checked(
        &mut doc,
        Command::RotateElements {
            ids,
            pivot: p(0.0, 0.0),
            angle_deg: 90.0,
        },
    );
    let dim = dimension(&r.state.project, DIM);
    assert_eq!(dim.a, p(0.0, 0.0));
    assert!(
        (dim.b.x).abs() < 1e-6 && (dim.b.y - 8000.0).abs() < 1e-6,
        "{:?}",
        dim.b
    );
    assert_eq!(dimension_reads(&r.state.project, DIM), 8000.0);
}

#[test]
fn a_dimension_follows_through_a_batch_and_under_a_locked_layer() {
    let mut doc = bungalow();
    apply_checked(&mut doc, layer(LayerKey::Dimensions, true));
    let r = apply_checked(
        &mut doc,
        Command::Batch {
            label: "Widen twice".into(),
            commands: vec![
                Command::ResizeRoom {
                    room_id: BEDROOM.into(),
                    side: Side::East,
                    delta_mm: 300.0,
                },
                Command::SetWallLength {
                    wall_id: W1.into(),
                    length_mm: 9000.0,
                    anchor: WallAnchor::Start,
                },
            ],
        },
    );
    assert_eq!(
        dimension_reads(&r.state.project, DIM),
        9000.0,
        "a locked Dimensions layer does not stop a dimension staying true"
    );
    assert_eq!(dimension(&r.state.project, DIM).b, p(9000.0, 0.0));

    // Editing a dimension by hand is still refused while the layer is locked.
    let mut edited = dimension(doc.project(), DIM).clone();
    edited.offset_mm = -1200.0;
    let e = apply_rejected(
        &mut doc,
        Command::UpdateElement {
            element: Element::Dimension(edited),
        },
    );
    assert_eq!(code(&e), "layer_locked");
}

#[test]
fn deleting_a_wall_keeps_its_dimension() {
    let mut doc = bungalow();
    let r = apply_checked(
        &mut doc,
        Command::DeleteElements {
            ids: vec![W1.into()],
        },
    );
    let dim = dimension(&r.state.project, DIM);
    assert_eq!((dim.a, dim.b), (p(0.0, 0.0), p(8000.0, 0.0)));
}

#[test]
fn splitting_a_wall_does_not_drag_its_dimension() {
    let mut doc = bungalow();
    let r = apply_checked(
        &mut doc,
        Command::SplitWall {
            wall_id: W1.into(),
            at_mm: 2500.0,
        },
    );
    let dim = dimension(&r.state.project, DIM);
    assert_eq!(
        (dim.a, dim.b),
        (p(0.0, 0.0), p(8000.0, 0.0)),
        "a split changes no geometry, so the dimension is untouched"
    );
}

// -------------------------------------------------------------------- Batch

#[test]
fn batch_is_all_or_nothing_and_one_undo_step() {
    let mut doc = blank();
    let good = Command::Batch {
        label: "Add a bedroom with a door".into(),
        commands: vec![
            rect_room(0.0, 0.0, 4000.0, 3000.0, Some("Bedroom")),
            add_wall(p(4000.0, 0.0), p(8000.0, 0.0)),
        ],
    };
    let r = apply_checked(&mut doc, good);
    assert_eq!(
        r.state.undo_label.as_deref(),
        Some("Add a bedroom with a door")
    );
    assert_eq!(walls(&r.state.project).len(), 5);
    assert_eq!(doc.revision(), 1);
    doc.undo().unwrap();
    assert!(
        doc.project().elements.is_empty(),
        "one undo reverts the whole batch"
    );
    doc.redo().unwrap();
    assert_eq!(walls(doc.project()).len(), 5);

    let bad = Command::Batch {
        label: "Broken".into(),
        commands: vec![
            add_wall(p(0.0, 5000.0), p(4000.0, 5000.0)),
            add_wall(p(0.0, 0.0), p(1.0, 0.0)),
        ],
    };
    let e = apply_rejected(&mut doc, bad);
    assert_eq!(code(&e), "wall_too_short");
    assert!(e.to_string().starts_with("Step 2 of 2:"), "{e}");
    assert_eq!(walls(doc.project()).len(), 5);

    let e = apply_rejected(
        &mut doc,
        Command::Batch {
            label: "Empty".into(),
            commands: vec![],
        },
    );
    assert_eq!(code(&e), "empty_batch");
}

#[test]
fn batch_ids_are_per_leaf_so_a_growing_batch_keeps_earlier_ids() {
    // The AI flow: stage a batch step by step, previewing after each step.
    // Ids made by step 1 must not change when step 2 is appended, so step 2
    // can point at them.
    let mut doc = blank();
    let a = rect_room(0.0, 0.0, 4000.0, 3000.0, Some("Bath"));
    let ids_of = |r: &ApplyResult| -> Vec<Id> {
        r.state
            .project
            .elements
            .iter()
            .map(|e| e.id().clone())
            .collect()
    };

    let alone = doc.preview(&a).unwrap();
    let batch_a = doc
        .preview(&Command::Batch {
            label: "Stage".into(),
            commands: vec![a.clone()],
        })
        .unwrap();
    assert_eq!(
        ids_of(&alone),
        ids_of(&batch_a),
        "a bare command and Batch[a] assign the same ids"
    );

    // Step 2 references a wall that step 1 creates.
    let south = wall_at(&batch_a.state.project, p(2000.0, 0.0)).id.clone();
    let b = opening(&south, OpeningType::Door, 2000.0);
    let batch_ab = Command::Batch {
        label: "Stage".into(),
        commands: vec![a.clone(), b],
    };
    let staged = doc.preview(&batch_ab).unwrap();
    assert_eq!(
        ids_of(&staged)[..5],
        ids_of(&batch_a)[..],
        "ids of step 1 survive appending step 2"
    );
    assert_eq!(staged.state.project.elements.len(), 6);

    let r = apply_checked(&mut doc, batch_ab);
    let door = openings(&r.state.project)[0];
    assert_eq!(door.wall_id, south);
    assert_eq!(wall_at(&r.state.project, p(2000.0, 0.0)).id, south);
    assert_eq!(ids_of(&r), ids_of(&staged));

    // A third step with a different label and more commands still keeps them.
    doc.undo().unwrap();
    let c = add_wall(p(0.0, -2000.0), p(4000.0, -2000.0));
    let three = doc
        .preview(&Command::Batch {
            label: "Other label".into(),
            commands: vec![a, opening(&south, OpeningType::Door, 2000.0), c],
        })
        .unwrap();
    assert_eq!(ids_of(&three)[..6], ids_of(&staged)[..]);
}

// ------------------------------------------------------ determinism and ids

#[test]
fn same_state_and_command_give_the_same_ids_in_two_documents() {
    let cmds = vec![
        rect_room(0.0, 0.0, 4000.0, 3000.0, Some("A")),
        rect_room(4000.0, 0.0, 3000.0, 3000.0, None),
        add_wall(p(0.0, -2000.0), p(4000.0, -2000.0)),
    ];
    let mut a = blank();
    let mut b = blank();
    for c in cmds {
        let ra = a.apply(c.clone(), Origin::User).unwrap();
        let rb = b.apply(c, Origin::Ai).unwrap();
        assert_eq!(ra.state.project.elements, rb.state.project.elements);
        assert_eq!(ra.diff, rb.diff);
    }
    let mut ids: Vec<&Id> = a.project().elements.iter().map(|e| e.id()).collect();
    let n = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(n, ids.len());
    assert!(ids
        .iter()
        .all(|id| id.len() == 36 && id.as_bytes()[14] == b'4'));
}

#[test]
fn preview_commits_nothing() {
    let doc = bungalow();
    let before = serde_json::to_string(&doc.state()).unwrap();
    let r = doc
        .preview(&Command::DeleteElements {
            ids: vec![W5.into()],
        })
        .unwrap();
    assert_eq!(r.state.revision, 1);
    assert!(r.state.can_undo);
    assert_eq!(before, serde_json::to_string(&doc.state()).unwrap());
}
