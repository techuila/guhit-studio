//! Room reconciliation (DECISIONS D7), review items and queries.

mod common;

use common::*;
use guhit_core::{templates, Document};
use guhit_model::*;
use serde_json::Value;

const W1: &str = "00000000-0000-4000-8000-000000000101";
const W2: &str = "00000000-0000-4000-8000-000000000102";
const W5: &str = "00000000-0000-4000-8000-000000000105";
const LIVING: &str = "00000000-0000-4000-8000-000000000301";
const BEDROOM: &str = "00000000-0000-4000-8000-000000000302";

fn bungalow() -> Document {
    Document::new(templates::sample_bungalow())
}

// -------------------------------------------------------------------- rooms

#[test]
fn a_new_partition_gives_the_new_face_a_room_and_keeps_the_old_name() {
    let mut doc = bungalow();
    let r = apply_checked(&mut doc, add_wall(p(0.0, 3500.0), p(5000.0, 3500.0)));
    assert_eq!(rooms(&r.state.project).len(), 3);
    // The living room seed is at y = 3000, so it keeps the southern part.
    let living = room_geo(&r.state, LIVING);
    assert_eq!(bbox(&living.polygon), (75.0, 75.0, 4950.0, 3425.0));
    let new_room = rooms(&r.state.project)
        .into_iter()
        .find(|x| x.auto_named)
        .unwrap();
    assert_eq!(new_room.name, "Room 1");
    assert_eq!(new_room.usage, RoomUsage::Other);
    assert!(r.diff.added.contains(&new_room.id));
    // One undo removes the wall and the auto room together.
    doc.undo().unwrap();
    assert_eq!(rooms(doc.project()).len(), 2);
    assert_eq!(walls(doc.project()).len(), 5);
}

#[test]
fn a_seed_left_outside_by_a_wall_move_follows_its_face() {
    let mut doc = bungalow();
    // The living seed is at x = 2500. Pull the partition to x = 2000: the
    // seed is now on the bedroom side, but both faces clearly persist.
    let r = apply_checked(
        &mut doc,
        Command::ResizeRoom {
            room_id: LIVING.into(),
            side: Side::East,
            delta_mm: -3000.0,
        },
    );
    let living = room_named(&r.state.project, "Living / Dining");
    let bedroom = room_named(&r.state.project, "Bedroom");
    assert_eq!((living.id.as_str(), bedroom.id.as_str()), (LIVING, BEDROOM));
    assert_eq!(
        bbox(&room_geo(&r.state, LIVING).polygon),
        (75.0, 75.0, 1950.0, 5925.0)
    );
    assert_eq!(
        bbox(&room_geo(&r.state, BEDROOM).polygon),
        (2050.0, 75.0, 7925.0, 5925.0)
    );
    assert_eq!(living.seed, room_geo(&r.state, LIVING).label_point);
    assert_eq!(
        bedroom.seed,
        p(6500.0, 3000.0),
        "a seed that is still fine does not move"
    );
}

#[test]
fn auto_names_are_unique_and_reuse_free_numbers() {
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 3000.0, 3000.0, None));
    apply_checked(&mut doc, rect_room(10_000.0, 0.0, 3000.0, 3000.0, None));
    apply_checked(
        &mut doc,
        rect_room(20_000.0, 0.0, 3000.0, 3000.0, Some("Room 3")),
    );
    let r = apply_checked(&mut doc, rect_room(30_000.0, 0.0, 3000.0, 3000.0, None));
    let mut names: Vec<String> = rooms(&r.state.project)
        .iter()
        .map(|x| x.name.clone())
        .collect();
    names.sort();
    assert_eq!(names, vec!["Room 1", "Room 2", "Room 3", "Room 4"]);
}

#[test]
fn rooms_are_per_level() {
    let mut project = blank_project();
    project.levels.push(Level {
        id: "level-2".into(),
        name: "Second Floor".into(),
        elevation_mm: 3000.0,
        height_mm: 3000.0,
    });
    let mut doc = Document::new(project);
    apply_checked(&mut doc, rect_room(0.0, 0.0, 4000.0, 3000.0, Some("Down")));
    let r = apply_checked(
        &mut doc,
        Command::AddRectRoom {
            origin: p(0.0, 0.0),
            width_mm: 4000.0,
            depth_mm: 3000.0,
            name: Some("Up".into()),
            thickness_mm: None,
            level_id: Some("level-2".into()),
        },
    );
    assert_eq!(
        walls(&r.state.project).len(),
        8,
        "walls on another level are not shared"
    );
    assert_eq!(room_named(&r.state.project, "Up").level_id, "level-2");
    assert_eq!(r.state.derived.footprints.len(), 2);
    assert!((r.state.derived.totals.gross_area_m2 - 2.0 * 4.15 * 3.15).abs() < 1e-9);
}

// ------------------------------------------------------------------- issues

fn codes(state: &DocState) -> Vec<String> {
    state
        .derived
        .issues
        .iter()
        .map(|i| i.code.clone())
        .collect()
}

#[test]
fn sample_bungalow_has_no_review_items() {
    assert!(bungalow().state().derived.issues.is_empty());
}

#[test]
fn room_without_door_and_window_is_flagged_calmly() {
    let mut doc = blank();
    let r = apply_checked(&mut doc, rect_room(0.0, 0.0, 4000.0, 3000.0, None));
    // An unnamed room only gets the mild door note.
    assert_eq!(codes(&r.state), vec!["room_no_door"]);
    assert_eq!(r.state.derived.issues[0].severity, Severity::Info);

    let mut room = rooms(doc.project())[0].clone();
    room.name = "Bedroom".into();
    room.usage = RoomUsage::Bedroom;
    let r = apply_checked(
        &mut doc,
        Command::UpdateElement {
            element: Element::Room(room.clone()),
        },
    );
    let c = codes(&r.state);
    assert!(c.contains(&"room_no_window".to_string()) && c.contains(&"room_no_door".to_string()));
    let no_window = r
        .state
        .derived
        .issues
        .iter()
        .find(|i| i.code == "room_no_window")
        .unwrap();
    assert_eq!(
        no_window.id,
        format!("room_no_window:{}", room.id),
        "issue ids are stable"
    );
    assert_eq!(no_window.element_ids, vec![room.id.clone()]);
    assert!(no_window
        .message
        .starts_with("Bedroom has no window on an outside wall"));

    let south = wall_at(doc.project(), p(2000.0, 0.0)).id.clone();
    apply_checked(&mut doc, opening(&south, OpeningType::Window, 2000.0));
    let east = wall_at(doc.project(), p(4000.0, 1500.0)).id.clone();
    let r = apply_checked(&mut doc, opening(&east, OpeningType::Door, 1500.0));
    assert!(codes(&r.state).is_empty(), "{:?}", r.state.derived.issues);
}

#[test]
fn window_on_an_inside_wall_does_not_count() {
    let mut doc = bungalow();
    // Remove the bedroom's two outside windows, put one in the partition.
    apply_checked(
        &mut doc,
        Command::DeleteElements {
            ids: vec![
                "00000000-0000-4000-8000-000000000204".into(),
                "00000000-0000-4000-8000-000000000205".into(),
            ],
        },
    );
    let r = apply_checked(&mut doc, opening(W5, OpeningType::Window, 1200.0));
    let issue = r
        .state
        .derived
        .issues
        .iter()
        .find(|i| i.code == "room_no_window")
        .expect("bedroom flagged");
    assert_eq!(issue.element_ids, vec![BEDROOM.to_string()]);
    // The living room window sits on the living room part of wall 1, so only the bedroom is listed.
    let q = doc.query(&Query::RoomsWithoutExteriorWindow).unwrap();
    assert_eq!(q["count"], 1);
    assert_eq!(q["rooms"][0]["name"], "Bedroom");
    assert_eq!(q["rooms"][0]["can_get_a_window"], true);
}

#[test]
fn small_room_narrow_door_and_corner_notes() {
    let mut doc = blank();
    apply_checked(
        &mut doc,
        rect_room(0.0, 0.0, 2000.0, 2000.0, Some("Bedroom")),
    );
    let mut room = rooms(doc.project())[0].clone();
    room.usage = RoomUsage::Bedroom;
    apply_checked(
        &mut doc,
        Command::UpdateElement {
            element: Element::Room(room),
        },
    );
    let south = wall_at(doc.project(), p(1000.0, 0.0)).id.clone();
    let r = apply_checked(
        &mut doc,
        Command::AddOpening {
            wall_id: south,
            opening_type: OpeningType::Door,
            offset_mm: 360.0,
            width_mm: Some(600.0),
            height_mm: None,
            sill_mm: None,
            style: None,
            flip_side: None,
            flip_hinge: None,
        },
    );
    let c = codes(&r.state);
    assert!(c.contains(&"room_small".to_string()));
    assert!(c.contains(&"door_narrow".to_string()));
    assert!(c.contains(&"opening_near_corner".to_string()));
    let small = r
        .state
        .derived
        .issues
        .iter()
        .find(|i| i.code == "room_small")
        .unwrap();
    assert!(small.message.contains("3.42 m2"), "{}", small.message);
    for i in &r.state.derived.issues {
        let text = i.message.to_lowercase();
        for banned in [
            "code",
            "comply",
            "compliance",
            "permit",
            "violat",
            "illegal",
            "must",
            "required",
        ] {
            assert!(
                !text.contains(banned),
                "review wording sounds like compliance: {}",
                i.message
            );
        }
    }
}

#[test]
fn wall_problems_gap_dangling_overlap_and_blocked_opening() {
    let mut doc = bungalow();
    // Stops 40 mm short of the east wall.
    let r = apply_checked(&mut doc, add_wall(p(6000.0, 3000.0), p(7960.0, 3000.0)));
    let gap = r
        .state
        .derived
        .issues
        .iter()
        .find(|i| i.code == "wall_end_gap_end")
        .expect("gap flagged");
    assert_eq!(gap.severity, Severity::Warning);
    assert!(gap.element_ids.contains(&W2.to_string()));
    assert!(gap.message.contains("40 mm"));
    assert!(codes(&r.state).contains(&"wall_dangling_start".to_string()));
    doc.undo().unwrap();

    let r = apply_checked(&mut doc, add_wall(p(1000.0, 0.0), p(3000.0, 0.0)));
    let overlap = r
        .state
        .derived
        .issues
        .iter()
        .find(|i| i.code == "wall_overlap")
        .expect("overlap flagged");
    assert!(overlap.element_ids.contains(&W1.to_string()));
    doc.undo().unwrap();

    // A partition that lands in the middle of the front door.
    let r = apply_checked(&mut doc, add_wall(p(1500.0, 0.0), p(1500.0, 6000.0)));
    assert!(codes(&r.state).contains(&"opening_blocked".to_string()));
}

// ------------------------------------------------------------------ queries

#[test]
fn project_summary_query() {
    let doc = bungalow();
    let q = doc.query(&Query::ProjectSummary).unwrap();
    assert_eq!(q["project_name"], "Sample Bungalow");
    assert_eq!(q["location"], "Quezon City");
    assert_eq!(q["totals"]["net_floor_area_m2"], 45.34);
    assert_eq!(q["totals"]["gross_footprint_area_m2"], 50.12);
    assert_eq!(q["totals"]["rooms"], 2);
    assert_eq!(q["totals"]["doors"], 2);
    assert_eq!(q["totals"]["windows"], 3);
    assert_eq!(q["totals"]["total_wall_length_m"], 34.0);
    assert_eq!(q["element_counts"]["wall"], 5);
    assert_eq!(q["levels"][0]["name"], "Ground Floor");
    assert_eq!(q["levels"][0]["room_count"], 2);
    assert_eq!(q["roof"]["kind"], "gable");
}

#[test]
fn room_list_query() {
    let doc = bungalow();
    let q = doc.query(&Query::RoomList).unwrap();
    assert_eq!(q["room_count"], 2);
    let living = &q["rooms"][0];
    assert_eq!(living["id"], LIVING);
    assert_eq!(living["name"], "Living / Dining");
    assert_eq!(living["usage"], "living");
    assert_eq!(living["net_area_m2"], 28.52);
    assert_eq!(living["clear_width_x_mm"], 4875.0);
    assert_eq!(living["clear_depth_y_mm"], 5850.0);
    assert_eq!(living["perimeter_mm"], 21450.0);
    assert_eq!(living["is_rectangular"], true);
    assert_eq!(living["bounding_walls"].as_array().unwrap().len(), 4);
    assert_eq!(
        living["doors"].as_array().unwrap().len(),
        2,
        "front door and the shared bedroom door"
    );
    assert_eq!(
        living["windows"].as_array().unwrap().len(),
        2,
        "south window and the north window at x = 2500"
    );
    assert_eq!(living["has_window_on_outside_wall"], true);
    let bedroom = &q["rooms"][1];
    assert_eq!(bedroom["net_area_m2"], 16.82);
    assert_eq!(bedroom["windows"].as_array().unwrap().len(), 1);
    assert_eq!(bedroom["windows"][0]["on_outside_wall"], true);
}

#[test]
fn describe_list_and_issue_queries() {
    let mut doc = bungalow();
    let q = doc
        .query(&Query::Describe {
            ids: vec![
                W5.into(),
                "00000000-0000-4000-8000-000000000202".into(),
                BEDROOM.into(),
                "nope".into(),
            ],
        })
        .unwrap();
    assert_eq!(q["not_found"], serde_json::json!(["nope"]));
    let els = q["elements"].as_array().unwrap();
    assert_eq!(els.len(), 3);
    assert_eq!(els[0]["kind"], "wall");
    assert_eq!(els[0]["length_mm"], 6000.0);
    assert_eq!(els[0]["faces_outside"], false);
    assert_eq!(els[0]["rooms"].as_array().unwrap().len(), 2);
    assert_eq!(els[0]["openings"][0]["type"], "door");
    assert_eq!(els[1]["kind"], "opening");
    assert_eq!(els[1]["center"]["x_mm"], 5000.0);
    assert_eq!(els[1]["center"]["y_mm"], 3000.0);
    assert_eq!(els[1]["rooms"].as_array().unwrap().len(), 2);
    assert_eq!(els[2]["kind"], "room");
    assert_eq!(els[2]["net_area_m2"], 16.82);

    for (kind, count) in [
        (ElementKind::Wall, 5),
        (ElementKind::Opening, 5),
        (ElementKind::Room, 2),
        (ElementKind::Asset, 1),
        (ElementKind::Dimension, 1),
        (ElementKind::Camera, 1),
        (ElementKind::Column, 0),
    ] {
        let q = doc.query(&Query::ListElements { kind }).unwrap();
        assert_eq!(q["count"], count, "{kind:?}");
        assert_eq!(q["elements"].as_array().unwrap().len(), count);
    }
    let bed = doc
        .query(&Query::ListElements {
            kind: ElementKind::Asset,
        })
        .unwrap();
    assert_eq!(bed["elements"][0]["in_room"]["name"], "Bedroom");

    let q = doc.query(&Query::Issues).unwrap();
    assert_eq!(q["count"], 0);
    apply_checked(
        &mut doc,
        Command::DeleteElements {
            ids: vec!["00000000-0000-4000-8000-000000000202".into()],
        },
    );
    let q = doc.query(&Query::Issues).unwrap();
    assert_eq!(q["count"], 1);
    assert_eq!(q["items"][0]["code"], "room_no_door");
    assert_eq!(q["items"][0]["severity"], "warning");
    assert!(matches!(&q["items"][0]["message"], Value::String(s) if s.starts_with("Bedroom")));
    assert_eq!(
        doc.query(&Query::RoomsWithoutExteriorWindow).unwrap()["count"],
        0
    );
}
