//! Geometry with answers worked out by hand.

mod common;

use common::*;
use guhit_core::{compute_derived, templates, Document};
use guhit_model::*;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn rectangle_4x3_with_150_walls_is_3850_by_2850_net() {
    let mut doc = blank();
    let r = apply_checked(
        &mut doc,
        rect_room(0.0, 0.0, 4000.0, 3000.0, Some("Bedroom")),
    );
    assert_eq!(walls(&r.state.project).len(), 4);
    let room = room_named(&r.state.project, "Bedroom");
    assert!(!room.auto_named);
    let g = room_geo(&r.state, &room.id);
    assert_eq!(
        g.polygon,
        vec![
            p(75.0, 75.0),
            p(3925.0, 75.0),
            p(3925.0, 2925.0),
            p(75.0, 2925.0)
        ]
    );
    assert_eq!(
        g.centerline_polygon,
        vec![
            p(0.0, 0.0),
            p(4000.0, 0.0),
            p(4000.0, 3000.0),
            p(0.0, 3000.0)
        ]
    );
    assert_eq!(g.area_mm2, 3850.0 * 2850.0);
    assert_eq!(g.perimeter_mm, 2.0 * (3850.0 + 2850.0));
    assert_eq!(g.label_point, p(2000.0, 1500.0));
    assert_eq!(g.wall_ids.len(), 4);
    assert!(close(r.state.derived.totals.floor_area_m2, 10.9725));
    assert!(close(r.state.derived.totals.gross_area_m2, 4.15 * 3.15));
    assert!(close(r.state.derived.totals.wall_length_m, 14.0));
    assert!(r.state.derived.walls.iter().all(|w| w.exterior));
    assert_eq!(r.diff.added.len(), 5);
}

#[test]
fn l_shaped_room() {
    let mut doc = blank();
    let r = apply_checked(
        &mut doc,
        Command::AddWallChain {
            points: vec![
                p(0.0, 0.0),
                p(6000.0, 0.0),
                p(6000.0, 3000.0),
                p(3000.0, 3000.0),
                p(3000.0, 6000.0),
                p(0.0, 6000.0),
            ],
            closed: true,
            thickness_mm: Some(200.0),
            level_id: None,
        },
    );
    assert_eq!(walls(&r.state.project).len(), 6);
    assert_eq!(rooms(&r.state.project).len(), 1);
    let room = rooms(&r.state.project)[0];
    assert_eq!(room.name, "Room 1");
    assert!(room.auto_named);
    let g = room_geo(&r.state, &room.id);
    // Two rectangles on the inner faces: 5800 x 2800 plus 2800 x 3000.
    assert!(close(g.area_mm2, 5800.0 * 2800.0 + 2800.0 * 3000.0));
    assert_eq!(g.polygon.len(), 6);
    assert!(
        g.polygon.contains(&p(2900.0, 2900.0)),
        "reflex corner sits on both inner faces"
    );
    assert!(inside(g.label_point, &g.polygon));
    // Footprint on the outer faces: 6200 x 3200 plus 3200 x 3000.
    assert!(close(
        r.state.derived.footprints[0].area_mm2,
        6200.0 * 3200.0 + 3200.0 * 3000.0
    ));
}

#[test]
fn two_rooms_sharing_a_partition() {
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 4000.0, 3000.0, Some("A")));
    // The second rectangle reuses the shared wall instead of doubling it.
    let r = apply_checked(&mut doc, rect_room(4000.0, 0.0, 3000.0, 3000.0, Some("B")));
    assert_eq!(walls(&r.state.project).len(), 7);
    assert_eq!(rooms(&r.state.project).len(), 2);
    let a = room_geo(&r.state, &room_named(&r.state.project, "A").id);
    let b = room_geo(&r.state, &room_named(&r.state.project, "B").id);
    assert!(close(a.area_mm2, 3850.0 * 2850.0));
    assert!(close(b.area_mm2, 2850.0 * 2850.0));
    let shared: Vec<&Id> = a
        .wall_ids
        .iter()
        .filter(|id| b.wall_ids.contains(id))
        .collect();
    assert_eq!(shared.len(), 1);
    let partition = r
        .state
        .derived
        .walls
        .iter()
        .find(|w| &w.wall_id == shared[0])
        .unwrap();
    assert!(!partition.exterior);
    assert_eq!(
        r.state.derived.walls.iter().filter(|w| w.exterior).count(),
        6
    );
    assert_eq!(r.state.derived.footprints.len(), 1);
    assert!(close(
        r.state.derived.footprints[0].area_mm2,
        7150.0 * 3150.0
    ));
    assert!(r
        .state
        .derived
        .issues
        .iter()
        .all(|i| i.code != "wall_overlap"));
}

#[test]
fn t_junction_partition_butts_into_the_through_walls() {
    let state = Document::new(templates::sample_bungalow()).state();
    assert_invariants(&state);
    let living = room_geo(&state, "00000000-0000-4000-8000-000000000301");
    let bedroom = room_geo(&state, "00000000-0000-4000-8000-000000000302");
    assert_eq!(bbox(&living.polygon), (75.0, 75.0, 4950.0, 5925.0));
    assert_eq!(bbox(&bedroom.polygon), (5050.0, 75.0, 7925.0, 5925.0));
    let partition = state
        .derived
        .walls
        .iter()
        .find(|w| w.wall_id.ends_with("105"))
        .unwrap();
    assert_eq!(
        partition.outline,
        vec![
            p(5050.0, 75.0),
            p(5050.0, 5925.0),
            p(4950.0, 5925.0),
            p(4950.0, 75.0)
        ],
        "the partition stops at the inner faces of the walls it meets"
    );
    assert_eq!(
        partition.joined_at_start,
        vec!["00000000-0000-4000-8000-000000000101".to_string()]
    );
    assert_eq!(
        partition.joined_at_end,
        vec!["00000000-0000-4000-8000-000000000103".to_string()]
    );
    let south = state
        .derived
        .walls
        .iter()
        .find(|w| w.wall_id.ends_with("101"))
        .unwrap();
    assert_eq!(
        south.outline,
        vec![
            p(-75.0, -75.0),
            p(8075.0, -75.0),
            p(7925.0, 75.0),
            p(75.0, 75.0)
        ],
        "L corners mitre"
    );
}

#[test]
fn resize_room_east_by_300_grows_width_by_exactly_300() {
    let mut doc = Document::new(templates::sample_bungalow());
    let living_id = "00000000-0000-4000-8000-000000000301";
    let bedroom_id = "00000000-0000-4000-8000-000000000302";
    let r = apply_checked(
        &mut doc,
        Command::ResizeRoom {
            room_id: living_id.into(),
            side: Side::East,
            delta_mm: 300.0,
        },
    );
    let living = room_geo(&r.state, living_id);
    let bedroom = room_geo(&r.state, bedroom_id);
    assert_eq!(bbox(&living.polygon), (75.0, 75.0, 5250.0, 5925.0));
    assert_eq!(living.area_mm2, 5175.0 * 5850.0);
    assert_eq!(bbox(&bedroom.polygon), (5350.0, 75.0, 7925.0, 5925.0));
    assert_eq!(bedroom.area_mm2, 2575.0 * 5850.0);
    assert_eq!(rooms(&r.state.project).len(), 2);
    assert_eq!(room_named(&r.state.project, "Bedroom").id, bedroom_id);
    // Only the partition moved. Its door rode along.
    assert_eq!(
        r.diff.modified,
        vec!["00000000-0000-4000-8000-000000000105".to_string()]
    );
    let partition = wall(&r.state.project, "00000000-0000-4000-8000-000000000105");
    assert_eq!(
        (partition.start, partition.end),
        (p(5300.0, 0.0), p(5300.0, 6000.0))
    );
    // The outer shell did not change.
    assert_eq!(r.state.derived.footprints[0].area_mm2, 8150.0 * 6150.0);
}

#[test]
fn resize_room_west_stretches_the_corner_walls_and_keeps_openings_in_place() {
    let mut doc = Document::new(templates::sample_bungalow());
    let living_id = "00000000-0000-4000-8000-000000000301";
    let r = apply_checked(
        &mut doc,
        Command::ResizeRoom {
            room_id: living_id.into(),
            side: Side::West,
            delta_mm: 500.0,
        },
    );
    let living = room_geo(&r.state, living_id);
    assert_eq!(bbox(&living.polygon), (-425.0, 75.0, 4950.0, 5925.0));
    let south = wall(&r.state.project, "00000000-0000-4000-8000-000000000101");
    assert_eq!(south.start, p(-500.0, 0.0));
    // The front door was 1500 from the old start. It stays at x = 1500.
    let door = openings(&r.state.project)
        .into_iter()
        .find(|o| o.id.ends_with("201"))
        .unwrap();
    assert_eq!(door.offset_mm, 2000.0);
    assert_eq!(
        r.diff.summary,
        "Moved the west side of Living / Dining by 500 mm (1 wall), stretched 2 walls, updated 1 dimension"
    );
    // The south dimension was drawn on the south wall, which is now 8500 long.
    let dim = dimension(&r.state.project, "00000000-0000-4000-8000-000000000501");
    assert_eq!((dim.a, dim.b), (p(-500.0, 0.0), p(8000.0, 0.0)));
    assert_eq!(dimension_reads(&r.state.project, &dim.id), 8500.0);
    // North wall runs east to west, so its end moved and its window offset did not.
    let window = openings(&r.state.project)
        .into_iter()
        .find(|o| o.id.ends_with("205"))
        .unwrap();
    assert_eq!(window.offset_mm, 5500.0);
}

#[test]
fn triangular_room_matches_the_inradius_formula() {
    let mut doc = blank();
    let r = apply_checked(
        &mut doc,
        Command::AddWallChain {
            points: vec![p(0.0, 0.0), p(8000.0, 0.0), p(0.0, 6000.0)],
            closed: true,
            thickness_mm: Some(200.0),
            level_id: None,
        },
    );
    let g = &r.state.derived.rooms[0];
    // 6-8-10 triangle: area 24 m2, inradius 2000. Offsetting by 100 scales it by 1900 / 2000.
    assert!((g.area_mm2 - 24.0e6 * (0.95f64).powi(2)).abs() < 1.0);
    assert_eq!(g.polygon.len(), 3);
}

#[test]
fn room_with_an_island_subtracts_the_island() {
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 8000.0, 8000.0, Some("Hall")));
    let r = apply_checked(
        &mut doc,
        Command::AddRectRoom {
            origin: p(3000.0, 3000.0),
            width_mm: 2000.0,
            depth_mm: 2000.0,
            name: Some("Core".into()),
            thickness_mm: Some(100.0),
            level_id: None,
        },
    );
    let hall = room_geo(&r.state, &room_named(&r.state.project, "Hall").id);
    let core = room_geo(&r.state, &room_named(&r.state.project, "Core").id);
    assert!(close(core.area_mm2, 1900.0 * 1900.0));
    assert!(close(hall.area_mm2, 7850.0 * 7850.0 - 2100.0 * 2100.0));
    assert!(!inside(hall.label_point, &core.centerline_polygon));
    assert_eq!(r.state.derived.footprints.len(), 1);
}

#[test]
fn compute_derived_matches_the_document() {
    let project = templates::sample_bungalow();
    assert_eq!(
        compute_derived(&project),
        Document::new(project).state().derived
    );
}

/// With every join resolved, the wall outlines tile the ring between the
/// footprint and the rooms exactly: no gaps and no double counting.
fn assert_walls_tile(state: &DocState) {
    let walls_area: f64 = state.derived.walls.iter().map(|w| area(&w.outline)).sum();
    let gross: f64 = state.derived.footprints.iter().map(|f| f.area_mm2).sum();
    let net: f64 = state.derived.rooms.iter().map(|r| r.area_mm2).sum();
    assert!(
        (walls_area - (gross - net)).abs() < 1e-3,
        "wall outlines cover {walls_area} mm2, footprint minus rooms is {} mm2",
        gross - net
    );
}

#[test]
fn wall_outlines_tile_the_plan_at_l_t_and_three_way_joints() {
    // L corners and T junctions.
    assert_walls_tile(&Document::new(templates::sample_bungalow()).state());

    // Three wall ends meeting in one point (two rooms drawn one after the other).
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 4000.0, 3000.0, Some("A")));
    let r = apply_checked(
        &mut doc,
        Command::AddRectRoom {
            origin: p(4000.0, 0.0),
            width_mm: 3000.0,
            depth_mm: 3000.0,
            name: Some("B".into()),
            thickness_mm: Some(100.0),
            level_id: None,
        },
    );
    assert_walls_tile(&r.state);

    // L-shaped room with a reflex corner.
    let mut doc = blank();
    let r = apply_checked(
        &mut doc,
        Command::AddWallChain {
            points: vec![
                p(0.0, 0.0),
                p(6000.0, 0.0),
                p(6000.0, 3000.0),
                p(3000.0, 3000.0),
                p(3000.0, 6000.0),
                p(0.0, 6000.0),
            ],
            closed: true,
            thickness_mm: Some(200.0),
            level_id: None,
        },
    );
    assert_walls_tile(&r.state);

    // Not axis aligned: a 6-8-10 triangle.
    let mut doc = blank();
    let r = apply_checked(
        &mut doc,
        Command::AddWallChain {
            points: vec![p(0.0, 0.0), p(8000.0, 0.0), p(0.0, 6000.0)],
            closed: true,
            thickness_mm: Some(200.0),
            level_id: None,
        },
    );
    assert_walls_tile(&r.state);
}

#[test]
fn rotated_rectangle_keeps_its_net_area() {
    let mut doc = blank();
    apply_checked(
        &mut doc,
        rect_room(0.0, 0.0, 4000.0, 3000.0, Some("Tilted")),
    );
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
            pivot: p(1234.0, -567.0),
            angle_deg: 33.0,
        },
    );
    let g = room_geo(&r.state, &room_named(&r.state.project, "Tilted").id);
    assert!((g.area_mm2 - 3850.0 * 2850.0).abs() < 1e-3);
    assert!((g.perimeter_mm - 2.0 * (3850.0 + 2850.0)).abs() < 1e-6);
    assert_eq!(g.polygon.len(), 4);
    assert!((r.state.derived.footprints[0].area_mm2 - 4150.0 * 3150.0).abs() < 1e-3);
    assert_walls_tile(&r.state);
}

#[test]
fn wall_end_just_short_of_a_wall_body_still_makes_a_t_junction() {
    let mut doc = Document::new(templates::sample_bungalow());
    // Ends 0.6 mm short of the south wall centerline and 0.8 mm short of the north one.
    let r = apply_checked(&mut doc, add_wall(p(2500.0, 0.6), p(2500.0, 5999.2)));
    assert_eq!(rooms(&r.state.project).len(), 3);
    let new_wall = r.state.derived.walls.last().unwrap();
    assert_eq!(
        new_wall.joined_at_start,
        vec!["00000000-0000-4000-8000-000000000101".to_string()]
    );
    assert_eq!(
        new_wall.joined_at_end,
        vec!["00000000-0000-4000-8000-000000000103".to_string()]
    );
    // 2 mm away is a gap, not a joint.
    doc.undo().unwrap();
    let r = apply_checked(&mut doc, add_wall(p(2500.0, 2.0), p(2500.0, 6000.0)));
    assert_eq!(rooms(&r.state.project).len(), 2);
    assert!(r
        .state
        .derived
        .issues
        .iter()
        .any(|i| i.code == "wall_end_gap_start"));
}
