//! Lane L1: door and window types (DECISIONS D53) and the schedules
//! (`Derived::opening_schedule`, `room_schedule`, `level_areas`).

mod common;

use common::*;
use guhit_core::{templates, Document};
use guhit_model::*;

fn bungalow() -> Document {
    Document::new(templates::sample_bungalow())
}

fn opening_of<'a>(doc: &'a Document, id: &str) -> &'a Opening {
    doc.project()
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Opening(o) if o.id == id => Some(o),
            _ => None,
        })
        .expect("opening")
}

fn mark_of(doc: &Document, opening_id: &str) -> String {
    doc.derived()
        .opening_schedule
        .marks
        .iter()
        .find(|m| m.opening_id == opening_id)
        .map(|m| m.mark.clone())
        .unwrap_or_default()
}

fn type_by_mark<'a>(doc: &'a Document, mark: &str) -> &'a OpeningSpec {
    doc.project().opening_types.iter().find(|t| t.mark == mark).expect("type")
}

fn ob(n: u32) -> String {
    format!("00000000-0000-4000-8000-00000000020{n}")
}

fn typed_bungalow() -> Document {
    let mut doc = bungalow();
    apply_checked(&mut doc, Command::AssignOpeningTypes { level_id: None });
    doc
}

#[test]
fn assign_types_on_the_sample_bungalow_gives_the_expected_marks() {
    let mut doc = bungalow();
    let r = apply_checked(&mut doc, Command::AssignOpeningTypes { level_id: None });
    assert_eq!(r.diff.summary, "Typed 5 openings: new types D1, D2, W1, W2, W3");
    // D1: the 900 front door. D2: the 800 bedroom door. W1: the 1500
    // sliding window. W2 and W3: the 1200 casement and jalousie windows.
    let expected = [(1, "D1"), (2, "D2"), (3, "W1"), (4, "W2"), (5, "W3")];
    for (n, mark) in expected {
        assert_eq!(mark_of(&doc, &ob(n)), mark, "opening {n}");
    }
    let d1 = type_by_mark(&doc, "D1");
    assert_eq!((d1.opening_type, d1.style, d1.width_mm, d1.height_mm, d1.sill_mm), (OpeningType::Door, OpeningStyle::SwingSingle, 900.0, 2100.0, None));
    assert_eq!(d1.description, "Single swing door");
    let w1 = type_by_mark(&doc, "W1");
    assert_eq!((w1.style, w1.width_mm, w1.sill_mm), (OpeningStyle::Sliding, 1500.0, Some(900.0)));

    // The schedule: one row per mark, doors first, each counted once.
    let rows = &doc.derived().opening_schedule.rows;
    assert_eq!(rows.iter().map(|r| r.mark.as_str()).collect::<Vec<_>>(), vec!["D1", "D2", "W1", "W2", "W3"]);
    assert!(rows.iter().all(|r| r.total == 1 && r.per_level.len() == 1 && r.per_level[0].count == 1));

    // Again: nothing left to type.
    assert_eq!(code(&apply_rejected(&mut doc, Command::AssignOpeningTypes { level_id: None })), "nothing_to_type");
}

#[test]
fn a_type_change_rewrites_all_its_openings_in_one_step_and_one_undo_restores_them() {
    let mut doc = bungalow();
    // Both 1200 x 1200 windows as one type.
    let jal = opening_of(&doc, &ob(5)).clone();
    apply_checked(&mut doc, Command::UpdateElement { element: Element::Opening(Opening { style: OpeningStyle::Casement, ..jal }) });
    apply_checked(&mut doc, Command::AssignOpeningTypes { level_id: None });
    assert_eq!(mark_of(&doc, &ob(4)), "W2");
    assert_eq!(mark_of(&doc, &ob(5)), "W2");
    let before = doc.project().clone();

    let mut w2 = type_by_mark(&doc, "W2").clone();
    w2.width_mm = 1000.0;
    w2.height_mm = 1500.0;
    w2.sill_mm = Some(600.0);
    w2.style = OpeningStyle::Jalousie;
    let r = apply_checked(&mut doc, Command::UpsertOpeningSpec { spec: Box::new(w2) });
    assert_eq!(r.diff.summary.split(';').next().unwrap(), "Changed type W2: 2 windows follow");
    for n in [4, 5] {
        let o = opening_of(&doc, &ob(n));
        assert_eq!((o.style, o.width_mm, o.height_mm, o.sill_mm), (OpeningStyle::Jalousie, 1000.0, 1500.0, 600.0), "opening {n}");
    }
    let row = doc.derived().opening_schedule.rows.iter().find(|r| r.mark == "W2").unwrap();
    assert_eq!((row.width_mm, row.total), (1000.0, 2));

    doc.undo().unwrap();
    assert_eq!(doc.project(), &before, "one undo restores the type and both windows");
}

#[test]
fn editing_one_typed_opening_moves_it_to_a_new_mark_and_back() {
    let mut doc = typed_bungalow();
    let front = opening_of(&doc, &ob(1)).clone();
    let r = apply_checked(&mut doc, Command::UpdateElement { element: Element::Opening(Opening { width_mm: 1000.0, ..front.clone() }) });
    assert!(r.diff.summary.ends_with("; Door moved from D1 to new type D3"), "{}", r.diff.summary);
    assert_eq!(mark_of(&doc, &ob(1)), "D3");
    assert_eq!(opening_of(&doc, &ob(1)).width_mm, 1000.0, "the edit is kept, never refused");
    let d3 = type_by_mark(&doc, "D3");
    assert_eq!((d3.width_mm, d3.description.as_str()), (1000.0, "Single swing door"));
    // D1 still exists, now unused.
    let d1 = doc.derived().opening_schedule.rows.iter().find(|r| r.mark == "D1").unwrap();
    assert_eq!(d1.total, 0);

    // Back to 900: it matches D1 again.
    let wide = opening_of(&doc, &ob(1)).clone();
    let r = apply_checked(&mut doc, Command::UpdateElement { element: Element::Opening(Opening { width_mm: 900.0, ..wide }) });
    assert!(r.diff.summary.ends_with("; Door moved from D3 to D1"), "{}", r.diff.summary);
    assert_eq!(mark_of(&doc, &ob(1)), "D1");

    // A sill change on a door type (no sill of its own) keeps the mark;
    // on a window type with a sill it moves.
    let w1 = opening_of(&doc, &ob(3)).clone();
    let r = apply_checked(&mut doc, Command::UpdateElement { element: Element::Opening(Opening { sill_mm: 1000.0, ..w1 }) });
    assert!(r.diff.summary.ends_with("; Window moved from W1 to new type W4"), "{}", r.diff.summary);
}

#[test]
fn giving_an_opening_a_type_takes_the_types_size_unless_it_does_not_fit() {
    let mut doc = typed_bungalow();
    let d1 = type_by_mark(&doc, "D1").id.clone();
    // The bedroom door (800) becomes a D1: it widens to 900.
    let bed = opening_of(&doc, &ob(2)).clone();
    apply_checked(&mut doc, Command::UpdateElement { element: Element::Opening(Opening { type_id: Some(d1.clone()), ..bed }) });
    assert_eq!((opening_of(&doc, &ob(2)).width_mm, mark_of(&doc, &ob(2)).as_str()), (900.0, "D1"));

    // A new door of type D1 takes D1's size whatever the command says.
    let wall = "00000000-0000-4000-8000-000000000103".to_string();
    let r = apply_checked(
        &mut doc,
        Command::AddOpening {
            wall_id: wall.clone(),
            opening_type: OpeningType::Door,
            offset_mm: 2000.0,
            width_mm: Some(700.0),
            height_mm: Some(2000.0),
            sill_mm: None,
            style: Some(OpeningStyle::Sliding),
            flip_side: None,
            flip_hinge: None,
            type_id: Some(d1.clone()),
        },
    );
    let new_id = r.diff.added[0].clone();
    let o = opening_of(&doc, &new_id);
    assert_eq!((o.style, o.width_mm, o.height_mm), (OpeningStyle::SwingSingle, 900.0, 2100.0));

    // A window cannot use a door type.
    let w = opening_of(&doc, &ob(4)).clone();
    let e = apply_rejected(&mut doc, Command::UpdateElement { element: Element::Opening(Opening { type_id: Some(d1.clone()), ..w }) });
    assert_eq!(code(&e), "opening_type_mismatch");

    // A type whose width no longer fits beside the wall's end: the opening
    // keeps its size and gets a type that matches it.
    let big = OpeningSpec {
        id: String::new(),
        mark: String::new(),
        opening_type: OpeningType::Door,
        style: OpeningStyle::SwingDouble,
        width_mm: 4000.0,
        height_mm: 2100.0,
        sill_mm: None,
        material_id: None,
        description: String::new(),
        remarks: String::new(),
    };
    apply_checked(&mut doc, Command::UpsertOpeningSpec { spec: Box::new(big) });
    let wide = type_by_mark(&doc, "D3").id.clone();
    let front = opening_of(&doc, &ob(1)).clone();
    let r = apply_checked(&mut doc, Command::UpdateElement { element: Element::Opening(Opening { type_id: Some(wide), ..front }) });
    assert!(r.diff.summary.ends_with("; Door kept its size, which D3 does not fit; type D1"), "{}", r.diff.summary);
    assert_eq!(opening_of(&doc, &ob(1)).width_mm, 900.0);
}

#[test]
fn marks_are_unique_and_an_empty_one_gets_the_next_free() {
    let mut doc = typed_bungalow();
    let spec = |mark: &str| OpeningSpec {
        id: String::new(),
        mark: mark.into(),
        opening_type: OpeningType::Window,
        style: OpeningStyle::Fixed,
        width_mm: 600.0,
        height_mm: 600.0,
        sill_mm: Some(1800.0),
        material_id: None,
        description: String::new(),
        remarks: String::new(),
    };
    let e = apply_rejected(&mut doc, Command::UpsertOpeningSpec { spec: Box::new(spec("w1")) });
    assert_eq!(code(&e), "mark_taken");
    assert!(e.to_string().contains("W4"), "{e}");
    let r = apply_checked(&mut doc, Command::UpsertOpeningSpec { spec: Box::new(spec("")) });
    assert_eq!(r.diff.summary, "Added window type W4");
    assert_eq!(type_by_mark(&doc, "W4").description, "Fixed window");
    // Two identical commands make two types (P0-5).
    apply_checked(&mut doc, Command::UpsertOpeningSpec { spec: Box::new(spec("")) });
    assert!(doc.project().opening_types.iter().any(|t| t.mark == "W5"));
    // Delete W2: its window is untyped and keeps its size.
    let w2 = type_by_mark(&doc, "W2").id.clone();
    let r = apply_checked(&mut doc, Command::DeleteOpeningSpecs { ids: vec![w2] });
    assert_eq!(r.diff.summary, "Deleted type W2; 1 opening is untyped now");
    assert_eq!(opening_of(&doc, &ob(4)).type_id, None);
    assert_eq!(opening_of(&doc, &ob(4)).width_mm, 1200.0);
    // The lowest free mark is reused.
    apply_checked(&mut doc, Command::UpsertOpeningSpec { spec: Box::new(spec("")) });
    assert!(doc.project().opening_types.iter().any(|t| t.mark == "W2" && t.style == OpeningStyle::Fixed));
    // The untyped window is listed with an empty mark.
    let rows = &doc.derived().opening_schedule.rows;
    assert!(rows.iter().any(|r| r.mark.is_empty() && r.total == 1 && r.style == OpeningStyle::Casement));
}

#[test]
fn a_door_type_cannot_become_a_window_type_while_doors_use_it() {
    let mut doc = typed_bungalow();
    let mut d1 = type_by_mark(&doc, "D1").clone();
    d1.opening_type = OpeningType::Window;
    assert_eq!(code(&apply_rejected(&mut doc, Command::UpsertOpeningSpec { spec: Box::new(d1) })), "opening_type_mismatch");
}

#[test]
fn a_type_that_pushes_an_opening_past_its_wall_is_refused() {
    let mut doc = typed_bungalow();
    let mut w1 = type_by_mark(&doc, "W1").clone();
    w1.height_mm = 2600.0;
    let e = apply_rejected(&mut doc, Command::UpsertOpeningSpec { spec: Box::new(w1) });
    assert_eq!(code(&e), "opening_too_tall");
}

// --------------------------------------------------------------- schedules

fn bars() -> Option<Railing> {
    Some(Railing { style: RailingStyle::Bars, height_mm: 1000.0 })
}

fn slab(level: &str, kind: SlabKind, x0: f64, y0: f64, x1: f64, y1: f64) -> Command {
    Command::AddElement {
        element: Element::Slab(Slab {
            id: String::new(),
            level_id: level.into(),
            kind,
            polygon: vec![p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)],
            thickness_mm: 150.0,
            top_offset_mm: 0.0,
            material_id: None,
            railing: if kind == SlabKind::Deck { None } else { bars() },
        }),
    }
}

/// Two storeys with a void, a balcony and a lanai deck.
fn house_with_slabs() -> (Document, Id) {
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 8000.0, 6000.0, Some("Living")));
    apply_checked(&mut doc, rect_room(0.0, -3000.0, 4000.0, 3000.0, Some("Lanai")));
    apply_checked(&mut doc, Command::AddLevel { name: Some("Second floor".into()), elevation_mm: Some(3000.0), height_mm: Some(3000.0) });
    let up = doc.project().levels.iter().find(|l| l.name == "Second floor").unwrap().id.clone();
    apply_checked(
        &mut doc,
        Command::AddRectRoom { origin: p(0.0, 0.0), width_mm: 8000.0, depth_mm: 6000.0, name: Some("Hall".into()), thickness_mm: None, level_id: Some(up.clone()) },
    );
    apply_checked(&mut doc, slab(&up, SlabKind::Void, 1000.0, 1000.0, 3000.0, 4000.0));
    apply_checked(&mut doc, slab(&up, SlabKind::Balcony, 5000.0, -1575.0, 7000.0, -75.0));
    apply_checked(&mut doc, slab(LEVEL, SlabKind::Deck, 0.0, -3000.0, 4000.0, 0.0));
    (doc, up)
}

fn r2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[test]
fn schedule_totals_equal_the_totals_with_a_void_a_balcony_and_a_deck() {
    let (doc, up) = house_with_slabs();
    let d = doc.derived();
    let rooms: f64 = d.room_schedule.iter().map(|r| r.floor_area_m2).sum();
    assert_eq!(r2(rooms), r2(d.totals.floor_area_m2));
    let net: f64 = d.level_areas.iter().map(|l| l.net_m2).sum();
    let gross: f64 = d.level_areas.iter().map(|l| l.gross_m2).sum();
    assert_eq!(r2(net), r2(d.totals.floor_area_m2));
    assert_eq!(r2(gross), r2(d.totals.gross_area_m2));

    // The void comes off the hall, the deck takes the lanai out of floor area.
    let hall = d.room_schedule.iter().find(|r| r.name == "Hall").unwrap();
    assert_eq!(r2(hall.floor_area_m2), r2(7.85 * 5.85 - 6.0));
    let lanai = d.room_schedule.iter().find(|r| r.name == "Lanai").unwrap();
    assert_eq!(lanai.floor_area_m2, 0.0);
    let upper = d.level_areas.iter().find(|l| l.level_id == up).unwrap();
    assert_eq!((r2(upper.void_m2), r2(upper.open_m2)), (6.0, 3.0));
    let ground = d.level_areas.iter().find(|l| l.level_id == LEVEL).unwrap();
    assert_eq!((r2(ground.void_m2), r2(ground.open_m2)), (0.0, 12.0));

    // Ceilings: the ground floor sits under the second floor's 200 mm slab.
    let living = d.room_schedule.iter().find(|r| r.name == "Living").unwrap();
    assert_eq!(living.ceiling_height_mm, 2800.0);
    assert_eq!(hall.ceiling_height_mm, 3000.0);
}

#[test]
fn query_areas_equal_derived_from_one_source() {
    let (doc, _) = house_with_slabs();
    let d = doc.derived();
    let levels = doc.query(&Query::LevelAreas).unwrap();
    assert_eq!(levels["levels"], serde_json::to_value(&d.level_areas).unwrap());
    let rooms = doc.query(&Query::RoomSchedule).unwrap();
    assert_eq!(rooms["rooms"], serde_json::to_value(&d.room_schedule).unwrap());
    // ProjectSummary's per level numbers match the schedule's to the
    // hundredth of a square meter.
    let summary = doc.query(&Query::ProjectSummary).unwrap();
    for l in summary["levels"].as_array().unwrap() {
        let id = l["id"].as_str().unwrap();
        let row = d.level_areas.iter().find(|x| x.level_id == id).unwrap();
        assert_eq!(l["net_floor_area_m2"].as_f64().unwrap(), r2(row.net_m2), "net of {id}");
        assert_eq!(l["gross_footprint_area_m2"].as_f64().unwrap(), r2(row.gross_m2), "gross of {id}");
    }
}

#[test]
fn room_finishes_and_ceiling_heights_reach_the_room_schedule() {
    let mut doc = bungalow();
    let mut room = room_named(doc.project(), "Bedroom").clone();
    room.finishes = RoomFinishes {
        wall_material_id: Some(defaults::MAT_WALL_DEFAULT.into()),
        ceiling_material_id: None,
        base_material_id: None,
        remarks: "Painted, semi gloss".into(),
    };
    room.ceiling_height_mm = Some(2700.0);
    apply_checked(&mut doc, Command::UpdateElement { element: Element::Room(room.clone()) });
    let row = doc.derived().room_schedule.iter().find(|r| r.room_id == room.id).unwrap();
    assert_eq!(row.ceiling_height_mm, 2700.0);
    assert_eq!(row.finishes, room.finishes);
    let living = doc.derived().room_schedule.iter().find(|r| r.name == "Living / Dining").unwrap();
    assert_eq!(living.ceiling_height_mm, 3000.0, "a single storey: the level height");

    room.ceiling_height_mm = Some(500.0);
    assert_eq!(code(&apply_rejected(&mut doc, Command::UpdateElement { element: Element::Room(room) })), "room_ceiling");
}

#[test]
fn a_room_with_no_door_gets_a_review_item() {
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 4000.0, 3000.0, Some("Store")));
    assert!(doc.derived().issues.iter().any(|i| i.code == "room_no_door"));
}
