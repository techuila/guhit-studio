//! Wave 2 gate fixes in the engine (fix round 1, lane F1): each test is a
//! gate finding that failed before its fix.

mod common;

use common::*;
use guhit_core::{site, templates, Document, Staging};
use guhit_model::*;

fn leg(bearing: &str, m: f64) -> SurveyLeg {
    SurveyLeg { bearing: bearing.into(), distance_mm: m * 1000.0 }
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

// ------------------------------------------------- GR1 #1: one-letter bearings

#[test]
fn a_bearing_with_one_letter_or_none_is_refused_with_a_hint() {
    for bad in ["N", "N 45", "45 E", "E", "S 12°05'", "45"] {
        let e = site::parse_bearing(bad).expect_err(bad);
        assert!(!e.is_empty(), "{bad}");
    }
    assert!(site::parse_bearing("N 45").unwrap_err().contains("no E or W at the end"));
    assert!(site::parse_bearing("45 E").unwrap_err().contains("no N or S at the start"));

    // Through the command, as MCP `set_lot` sends it: a refusal, never a crash.
    for bad in ["N", "N 45", "45 E", "E"] {
        let mut doc = blank();
        let legs = vec![leg(bad, 12.0), leg("N 0 deg E", 15.0), leg("S 90 deg W", 12.0), leg("S 0 deg W", 15.0)];
        let e = apply_rejected(&mut doc, Command::SetLotFromSurvey { start: p(0.0, 0.0), legs });
        assert_eq!(code(&e), "bad_bearing", "{bad}");
        let msg = e.to_string();
        assert!(msg.contains("Leg 1") && msg.contains("N 45 deg 30' E"), "{bad}: {msg}");
    }
}

// ------------------------------- GR1 #2: automatic dimensions after a retype

fn bungalow() -> Document {
    Document::new(templates::sample_bungalow())
}

fn opening_by_id<'a>(doc: &'a Document, suffix: &str) -> &'a Opening {
    doc.project()
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Opening(o) if o.id.ends_with(suffix) => Some(o),
            _ => None,
        })
        .expect("opening")
}

/// The points of a string, a to b.
fn string_points(d: &Dimension) -> Vec<Point> {
    std::iter::once(d.a).chain(d.through.iter().copied()).chain(std::iter::once(d.b)).collect()
}

#[test]
fn retyping_the_front_door_rewrites_its_automatic_dimensions_in_the_same_step() {
    let mut doc = bungalow();
    apply_checked(&mut doc, Command::AssignOpeningTypes { level_id: None });
    let level = doc.project().levels[0].id.clone();
    apply_checked(&mut doc, Command::AutoDimension { level_id: level, kinds: vec![AutoDimKind::Exterior] });
    let d2 = doc.project().opening_types.iter().find(|t| t.mark == "D2").unwrap().id.clone();
    // The inspector's type picker sends only the type.
    let door = opening_by_id(&doc, "201").clone();
    assert_eq!(door.width_mm, 900.0);
    apply_checked(&mut doc, Command::UpdateElement { element: Element::Opening(Opening { type_id: Some(d2), ..door.clone() }) });
    assert_eq!(opening_by_id(&doc, "201").width_mm, 800.0, "D2's width");

    // Nothing is left for the next command to rewrite.
    let mut fresh = doc.project().clone();
    assert_eq!(guhit_core::dims_auto::rewrite(&mut fresh), 0, "stale automatic dimensions after the retype");

    // The south openings string carries the 800 door.
    let south = doc
        .project()
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Dimension(d) if d.auto == Some(DimRef::Exterior { side: Side::South, tier: DimTier::Openings }) => Some(d.clone()),
            _ => None,
        })
        .unwrap();
    let pts = string_points(&south);
    let parts: Vec<f64> = pts.windows(2).map(|w| (w[1].x - w[0].x).abs().round()).collect();
    assert!(parts.contains(&800.0) && !parts.contains(&900.0), "{parts:?}");
}

// ------------------ GR2 note: what an automatic string will store, beforehand

#[test]
fn measure_now_gives_what_an_automatic_string_will_store() {
    let mut doc = bungalow();
    // The south wall, 0 to 8000 on its centerline, 150 thick, measured on
    // its outer face as MCP `annotate` asks.
    let dim = Dimension {
        id: String::new(),
        level_id: doc.project().levels[0].id.clone(),
        a: p(0.0, 0.0),
        b: p(8000.0, 0.0),
        offset_mm: -1200.0,
        text_override: None,
        through: vec![],
        auto: Some(DimRef::Openings { wall_ids: vec!["00000000-0000-4000-8000-000000000101".into()], face: DimFace::Exterior }),
    };
    let m = guhit_core::dims_auto::measure_now(doc.project(), &dim).expect("measured");
    let r = apply_checked(&mut doc, Command::AddElement { element: Element::Dimension(dim.clone()) });
    let stored = dimension(doc.project(), &r.diff.added[0]);
    assert_eq!((stored.a, stored.b, &stored.through), (m.a, m.b, &m.through));
    // On the face, 75 mm past each end of the centerline.
    assert_eq!((m.b.x - m.a.x).abs(), 8150.0);
    assert!(guhit_core::dims_auto::measure_now(doc.project(), &Dimension { auto: None, ..dim }).is_none());
}

// ------------------------------------- GR1 #3: a clockwise title keeps its order

/// A 12 by 15 m lot turned 45°30', corner 1 the north corner, clockwise as
/// most titles run. This one closes.
fn clockwise_title() -> Vec<SurveyLeg> {
    vec![leg("S. 44 deg. 30' E.", 12.00), leg("S. 45 deg. 30' W.", 15.00), leg("N. 44 deg. 30' W.", 12.00), leg("N. 45 deg. 30' E.", 15.00)]
}

fn the_lot(doc: &Document) -> Lot {
    doc.project()
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Lot(l) => Some(l.clone()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_clockwise_title_keeps_its_corner_order_so_numbers_and_bearings_match() {
    let mut doc = blank();
    let r = apply_checked(&mut doc, Command::SetLotFromSurvey { start: p(0.0, 20000.0), legs: clockwise_title() });
    assert_eq!(r.diff.summary, "Set the lot from 4 legs: 180.00 m2, it closes exactly");
    let lot = the_lot(&doc);
    assert_eq!(lot.corners[0], p(0.0, 20000.0), "corner 1 is the start");
    assert!(area(&lot.corners) < 0.0, "kept clockwise, as the title runs");
    // Corner 2 is where the title's first leg ends: 12 m to the south-east.
    let az = (180.0f64 - 44.5).to_radians();
    let c2 = p(12000.0 * az.sin(), 20000.0 + 12000.0 * az.cos());
    assert!(near(lot.corners[1].x, c2.x, 0.01) && near(lot.corners[1].y, c2.y, 0.01), "{:?}", lot.corners[1]);
    // Side i is the title's line i to i + 1, bearing and distance.
    let site = doc.derived().site.clone().unwrap();
    let bearings: Vec<&str> = site.sides.iter().map(|s| s.bearing.as_str()).collect();
    assert_eq!(bearings, vec!["S 44°30' E", "S 45°30' W", "N 44°30' W", "N 45°30' E"]);
    let lengths: Vec<f64> = site.sides.iter().map(|s| s.length_mm).collect();
    assert_eq!(lengths, vec![12000.0, 15000.0, 12000.0, 15000.0]);
    assert!(near(site.area_m2, 180.0, 1e-6));
}

#[test]
fn setbacks_and_their_review_work_on_a_clockwise_lot() {
    let mut doc = blank();
    // An 8 x 6 m house (outer faces -75 to 8075, -75 to 6075) on a 12 x 15
    // m lot square to the plan, its title running clockwise from the
    // south-west corner: west, north, east, then the south side as line 4-1.
    apply_checked(&mut doc, rect_room(0.0, 0.0, 8000.0, 6000.0, Some("House")));
    let legs = vec![leg("N 0 deg E", 15.0), leg("N 90 deg E", 12.0), leg("S 0 deg W", 15.0), leg("S 90 deg W", 12.0)];
    apply_checked(&mut doc, Command::SetLotFromSurvey { start: p(-2000.0, -3000.0), legs });
    let mut lot = the_lot(&doc);
    let want = [p(-2000.0, -3000.0), p(-2000.0, 12000.0), p(10000.0, 12000.0), p(10000.0, -3000.0)];
    assert_eq!(lot.corners.len(), 4);
    for (c, w) in lot.corners.iter().zip(want) {
        assert!(near(c.x, w.x, 1e-6) && near(c.y, w.y, 1e-6), "{:?}", lot.corners);
    }
    let site = doc.derived().site.clone().unwrap();
    assert_eq!(site.sides[3].bearing, "S 90°00' W");
    assert_eq!(site.sides[3].clearance_mm, Some(2925.0), "the south side, 2925 from the house face");

    // A 3 m setback on line 4-1, the south side: the house is 75 mm past it.
    lot.sides[3] = LotSide { role: LotSideRole::Road, setback_mm: 3000.0, road: "Mabini Street".into(), road_width_mm: Some(8000.0) };
    apply_checked(&mut doc, Command::UpdateElement { element: Element::Lot(lot) });
    let site = doc.derived().site.clone().unwrap();
    let has = |q: Point| site.setback_outline.iter().any(|c| (c.x - q.x).abs() < 1e-6 && (c.y - q.y).abs() < 1e-6);
    assert_eq!(site.setback_outline.len(), 4);
    assert!(has(p(-2000.0, 0.0)) && has(p(10000.0, 0.0)) && has(p(10000.0, 12000.0)), "{:?}", site.setback_outline);
    let over: Vec<&Issue> = doc.derived().issues.iter().filter(|i| i.code == "building_over_setback").collect();
    assert_eq!(over.len(), 1);
    assert!(over[0].message.starts_with("The building reaches 75 mm past the 3.00 m setback drawn along lot side 4, along Mabini Street."), "{}", over[0].message);
}

// --------------------------------- GR1 #6: staged steps keep their sentences

fn misclosing_title() -> Vec<SurveyLeg> {
    let mut legs = clockwise_title();
    legs[3] = leg("N. 45 deg. 31' E.", 15.00);
    legs
}

#[test]
fn a_staged_step_keeps_its_own_sentence_in_the_diff_and_the_history() {
    let mut doc = blank();
    let lot = Command::SetLotFromSurvey { start: p(0.0, 20000.0), legs: misclosing_title() };
    let mut staging = Staging::new(doc.project(), "MCP: set_lot");
    staging.push(&[lot]).unwrap();
    let staged = staging.diff().summary;
    assert!(staged.contains("Set the lot from 4 legs: 180.00 m2, misclosure 4 mm (1:"), "{staged}");
    // The same words as the batch it stands for.
    let batch = doc.preview(&staging.command()).unwrap().diff.summary;
    assert_eq!(staged, batch);
    let r = doc.apply_staged(staging, Origin::Ai, Some("agent-1".into()), "").unwrap();
    assert_eq!(r.diff.summary, batch);
    assert_eq!(doc.history().undo[0].summary, batch);

    // Several pushes keep every step's sentence, in order.
    let mut doc = bungalow();
    let level = doc.project().levels[0].id.clone();
    let mut staging = Staging::new(doc.project(), "MCP: build");
    staging.push(&[Command::AssignOpeningTypes { level_id: None }]).unwrap();
    staging.push(&[Command::AutoDimension { level_id: level, kinds: vec![AutoDimKind::Exterior] }]).unwrap();
    let summary = staging.diff().summary;
    let typed = summary.find("Typed 5 openings").unwrap_or_else(|| panic!("{summary}"));
    let dims = summary.find("Added 9 automatic dimensions").unwrap_or_else(|| panic!("{summary}"));
    assert!(typed < dims, "{summary}");
    assert_eq!(doc.preview(&staging.command()).unwrap().diff.summary, summary);
    doc.apply_staged(staging, Origin::Ai, None, "").unwrap();
}

// ----------------------------- GR1 #12: net area and floor area, both named

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
            railing: if kind == SlabKind::Deck { None } else { Some(Railing { style: RailingStyle::Bars, height_mm: 1000.0 }) },
        }),
    }
}

fn second_floor(doc: &mut Document) -> Id {
    apply_checked(doc, Command::AddLevel { name: Some("Second floor".into()), elevation_mm: Some(3000.0), height_mm: Some(3000.0) });
    doc.project().levels.iter().find(|l| l.name == "Second floor").unwrap().id.clone()
}

fn r2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[test]
fn the_room_schedule_gives_net_area_and_floor_area_for_a_lanai_on_a_deck() {
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 8000.0, 6000.0, Some("Living")));
    apply_checked(&mut doc, rect_room(0.0, -3000.0, 4000.0, 3000.0, Some("Lanai")));
    let up = second_floor(&mut doc);
    apply_checked(
        &mut doc,
        Command::AddRectRoom { origin: p(0.0, 0.0), width_mm: 8000.0, depth_mm: 6000.0, name: Some("Hall".into()), thickness_mm: None, level_id: Some(up.clone()) },
    );
    apply_checked(&mut doc, slab(&up, SlabKind::Void, 1000.0, 1000.0, 3000.0, 4000.0));
    apply_checked(&mut doc, slab(LEVEL, SlabKind::Deck, 0.0, -3000.0, 4000.0, 0.0));
    let d = doc.derived();
    let row = |name: &str| d.room_schedule.iter().find(|r| r.name == name).unwrap().clone();

    // The walled lanai on the deck: a net area, and no floor area.
    let lanai = row("Lanai");
    assert_eq!(r2(lanai.net_area_m2), r2(3.85 * 2.85));
    assert_eq!(lanai.floor_area_m2, 0.0);
    // The void comes off both.
    let hall = row("Hall");
    assert_eq!(r2(hall.net_area_m2), r2(7.85 * 5.85 - 6.0));
    assert_eq!(hall.floor_area_m2, hall.net_area_m2);
    // Net equals the room geometry; floor area adds up to the totals.
    for r in &d.room_schedule {
        let g = d.rooms.iter().find(|g| g.room_id == r.room_id).unwrap();
        assert!(near(r.net_area_m2, g.area_mm2 / 1.0e6, 1e-6), "{}", r.name);
    }
    let floor: f64 = d.room_schedule.iter().map(|r| r.floor_area_m2).sum();
    assert_eq!(r2(floor), r2(d.totals.floor_area_m2));

    // The queries give both, and their totals add up to their rows.
    let list = doc.query(&Query::RoomList).unwrap();
    let rows = list["rooms"].as_array().unwrap();
    let lanai_q = rows.iter().find(|r| r["name"] == "Lanai").unwrap();
    assert_eq!((lanai_q["net_area_m2"].as_f64(), lanai_q["floor_area_m2"].as_f64()), (Some(r2(3.85 * 2.85)), Some(0.0)));
    // Rows are rounded to the hundredth, the totals are not.
    let sum = |key: &str| rows.iter().map(|r| r[key].as_f64().unwrap()).sum::<f64>();
    let tol = 0.005 * rows.len() as f64;
    assert!(near(sum("net_area_m2"), list["total_net_area_m2"].as_f64().unwrap(), tol), "{list}");
    assert!(near(sum("floor_area_m2"), list["total_floor_area_m2"].as_f64().unwrap(), tol), "{list}");
    assert_eq!(list["total_floor_area_m2"].as_f64().unwrap(), r2(d.totals.floor_area_m2));
    assert!(list["total_net_area_m2"].as_f64().unwrap() > list["total_floor_area_m2"].as_f64().unwrap() + 10.0, "the lanai counts in net only");
    let summary = doc.query(&Query::ProjectSummary).unwrap();
    let s = summary["rooms"].as_array().unwrap().iter().find(|r| r["name"] == "Lanai").unwrap().clone();
    assert_eq!((s["net_area_m2"].as_f64(), s["floor_area_m2"].as_f64()), (Some(r2(3.85 * 2.85)), Some(0.0)));
}

// ----------------------------------- GR1 #14: a tour shot with no camera keys

fn toured_bungalow() -> Project {
    let mut doc = bungalow();
    apply_checked(
        &mut doc,
        Command::MakeTour { name: None, options: AutoTour { exterior: true, day_to_dusk: true, pace_mps: 1.0, levels: vec![] } },
    );
    doc.project().clone()
}

fn tour_of(project: &Project) -> Tour {
    project
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Tour(t) => Some(t.clone()),
            _ => None,
        })
        .unwrap()
}

/// The project with the keys of shots `which` taken away, as a damaged or
/// hand-edited file holds them.
fn without_keys(mut project: Project, which: &[usize]) -> Project {
    for el in project.elements.iter_mut() {
        if let Element::Tour(t) = el {
            for i in which {
                t.shots[*i].keys.clear();
            }
        }
    }
    project
}

#[test]
fn a_tour_shot_with_no_keys_never_panics_derive_review_or_frames() {
    let project = toured_bungalow();
    let shots = tour_of(&project).shots.len();
    assert!(shots >= 3);
    // The first, a middle and the last shot.
    let broken = without_keys(project, &[0, 1, shots - 1]);
    let derived = guhit_core::compute_derived(&broken);
    let t = tour_of(&broken);
    let v = guhit_core::tours::frames(&broken, &derived, &t.id, Some(10)).unwrap();
    // The key-less shots play for no time.
    let listed = v["shots"].as_array().unwrap();
    assert_eq!(listed[0]["frame_count"], 0);
    assert_eq!(listed[shots - 1]["frame_count"], 0);
    let total: f64 = t.shots.iter().filter(|s| !s.keys.is_empty()).map(|s| s.seconds).sum();
    assert!(near(v["seconds"].as_f64().unwrap(), total, 1e-3), "{} {total}", v["seconds"]);
    assert!(v["frame_count"].as_u64().unwrap() > 0);

    // Every shot key-less: nothing to play, still no panic.
    let all: Vec<usize> = (0..shots).collect();
    let empty = without_keys(toured_bungalow(), &all);
    let derived = guhit_core::compute_derived(&empty);
    let v = guhit_core::tours::frames(&empty, &derived, &t.id, None).unwrap();
    assert_eq!(v["frame_count"], 0);
}

#[test]
fn opening_a_project_drops_broken_shots_and_elements_with_a_note() {
    let project = toured_bungalow();
    let t = tour_of(&project);
    let mut broken = without_keys(project.clone(), &[1]);
    // An opening whose wall is gone, and a room on a level that is gone.
    let door = opening_by_id(&Document::new(project.clone()), "201").clone();
    broken.elements.push(Element::Opening(Opening { id: "orphan-door".into(), wall_id: "no-such-wall".into(), type_id: None, ..door }));
    let mut lost_room = match broken.elements.iter().find(|e| matches!(e, Element::Room(_))) {
        Some(Element::Room(r)) => r.clone(),
        _ => unreachable!(),
    };
    lost_room.id = "lost-room".into();
    lost_room.level_id = "no-such-level".into();
    broken.elements.push(Element::Room(lost_room));

    let doc = Document::new(broken);
    let notes = doc.repairs().to_vec();
    assert_eq!(notes.len(), 3, "{notes:?}");
    assert!(notes.iter().any(|n| n.contains(&t.name) && n.contains("1 shot with no camera keys")), "{notes:?}");
    assert!(notes.iter().any(|n| n.contains("orphan-door")), "{notes:?}");
    assert!(notes.iter().any(|n| n.contains("lost-room")), "{notes:?}");
    let kept = tour_of(doc.project());
    assert_eq!(kept.shots.len(), t.shots.len() - 1);
    assert!(kept.shots.iter().all(|s| !s.keys.is_empty()));
    assert!(doc.project().elements.iter().all(|e| e.id() != "orphan-door" && e.id() != "lost-room"));
    // What is left is valid: a later edit of the tour goes through.
    let mut doc = doc;
    apply_checked(&mut doc, Command::UpdateElement { element: Element::Tour(Tour { name: "Walk".into(), ..kept }) });

    // A sound project opens with no note and unchanged.
    let doc = Document::new(project.clone());
    assert!(doc.repairs().is_empty());
    assert_eq!(doc.project().elements, project.elements);
}

// ------------------- GR3 P0-1: move, copy and rotate carry the witness points

/// Each part of a string as the plan prints it: the points projected onto
/// the a to b line, in order.
fn parts(d: &Dimension) -> Vec<f64> {
    let len = ((d.b.x - d.a.x).powi(2) + (d.b.y - d.a.y).powi(2)).sqrt();
    let u = p((d.b.x - d.a.x) / len, (d.b.y - d.a.y) / len);
    let along: Vec<f64> = string_points(d).iter().map(|q| (q.x - d.a.x) * u.x + (q.y - d.a.y) * u.y).collect();
    along.windows(2).map(|w| ((w[1] - w[0]) * 1000.0).round() / 1000.0).collect()
}

fn string_doc() -> Document {
    let mut doc = blank();
    let dim = Dimension {
        id: "ds".into(),
        level_id: LEVEL.into(),
        a: p(0.0, -2000.0),
        b: p(8000.0, -2000.0),
        offset_mm: 0.0,
        text_override: None,
        through: vec![p(3000.0, -2000.0), p(5000.0, -2000.0)],
        auto: None,
    };
    apply_checked(&mut doc, Command::AddElement { element: Element::Dimension(dim) });
    doc
}

#[test]
fn moving_copying_and_rotating_a_dimension_string_keeps_its_parts() {
    let mut doc = string_doc();
    assert_eq!(parts(dimension(doc.project(), "ds")), vec![3000.0, 2000.0, 3000.0]);

    apply_checked(&mut doc, Command::MoveElements { ids: vec!["ds".into()], delta: p(1000.0, -1000.0), stretch_connected: false });
    let d = dimension(doc.project(), "ds").clone();
    assert_eq!(d.through, vec![p(4000.0, -3000.0), p(6000.0, -3000.0)]);
    assert_eq!(parts(&d), vec![3000.0, 2000.0, 3000.0], "after a move");

    let r = apply_checked(&mut doc, Command::DuplicateElements { ids: vec!["ds".into()], delta: p(0.0, 2500.0) });
    let copy = dimension(doc.project(), &r.diff.added[0]).clone();
    assert_eq!(copy.through, vec![p(4000.0, -500.0), p(6000.0, -500.0)]);
    assert_eq!(parts(&copy), vec![3000.0, 2000.0, 3000.0], "the copy");

    apply_checked(&mut doc, Command::RotateElements { ids: vec!["ds".into()], pivot: p(0.0, 0.0), angle_deg: 90.0 });
    let d = dimension(doc.project(), "ds").clone();
    let round = |q: Point| p(q.x.round(), q.y.round());
    assert_eq!(d.through.iter().copied().map(round).collect::<Vec<_>>(), vec![p(3000.0, 4000.0), p(3000.0, 6000.0)]);
    assert_eq!(parts(&d), vec![3000.0, 2000.0, 3000.0], "after a rotate");
}

// ------------------------- GS P0: the ceiling of a room under a void above

#[test]
fn a_room_under_a_void_gets_the_ceiling_of_the_level_above() {
    let mut doc = blank();
    // Ground floor 0 to 3000: a 6 x 5 m living and a 3 x 5 m study beside it.
    apply_checked(&mut doc, rect_room(0.0, 0.0, 6000.0, 5000.0, Some("Living")));
    apply_checked(&mut doc, rect_room(6000.0, 0.0, 3000.0, 5000.0, Some("Study")));
    let up = second_floor(&mut doc);
    apply_checked(
        &mut doc,
        Command::AddRectRoom { origin: p(0.0, 0.0), width_mm: 9000.0, depth_mm: 5000.0, name: Some("Hall".into()), thickness_mm: None, level_id: Some(up.clone()) },
    );
    let ceiling = |doc: &Document, name: &str| doc.derived().room_schedule.iter().find(|r| r.name == name).unwrap().ceiling_height_mm;
    assert_eq!((ceiling(&doc, "Living"), ceiling(&doc, "Study"), ceiling(&doc, "Hall")), (2800.0, 2800.0, 3000.0));

    // A 5.0 x 3.5 m void over most of the living: it is double height, up
    // to the second floor's ceiling, 3000 + 3000 from its floor. The study
    // keeps its 2800 under the slab; the hall upstairs keeps its 3000.
    apply_checked(&mut doc, slab(&up, SlabKind::Void, 500.0, 750.0, 5500.0, 4250.0));
    assert_eq!((ceiling(&doc, "Living"), ceiling(&doc, "Study"), ceiling(&doc, "Hall")), (6000.0, 2800.0, 3000.0));

    // A third floor puts a slab over the void: under it, 200 mm down.
    apply_checked(&mut doc, Command::AddLevel { name: Some("Third floor".into()), elevation_mm: Some(6000.0), height_mm: Some(3000.0) });
    assert_eq!((ceiling(&doc, "Living"), ceiling(&doc, "Hall")), (5800.0, 2800.0));

    // A room's own ceiling still wins.
    let mut living = room_named(doc.project(), "Living").clone();
    living.ceiling_height_mm = Some(4500.0);
    apply_checked(&mut doc, Command::UpdateElement { element: Element::Room(living) });
    assert_eq!(ceiling(&doc, "Living"), 4500.0);
}

// ------------------- GA P0-1: gross area keeps the walls under a void or balcony

#[test]
fn gross_area_keeps_the_walls_under_a_void_and_a_balcony() {
    // The GS house: 9 x 8 m on both floors (outer faces 74.5725 m2), a 5.0 x
    // 3.5 m void drawn to the wall centerlines in a corner of the second
    // floor, and a balcony drawn to the south wall's centerline.
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 9000.0, 8000.0, Some("Living")));
    let up = second_floor(&mut doc);
    apply_checked(
        &mut doc,
        Command::AddRectRoom { origin: p(0.0, 0.0), width_mm: 9000.0, depth_mm: 8000.0, name: Some("Hall".into()), thickness_mm: None, level_id: Some(up.clone()) },
    );
    apply_checked(&mut doc, slab(&up, SlabKind::Void, 0.0, 0.0, 5000.0, 3500.0));
    apply_checked(&mut doc, slab(&up, SlabKind::Balcony, 2000.0, -1500.0, 6000.0, 0.0));
    let d = doc.derived();
    let upper = d.level_areas.iter().find(|l| l.level_id == up).unwrap();
    // Only the clear opening of the void comes off: 4.925 x 3.425 m. The
    // wall strips under the void and the balcony stay floor area.
    let want = 9.15 * 8.15 - 4.925 * 3.425;
    assert!(near(upper.gross_m2, want, 1e-6), "{} want {want}", upper.gross_m2);
    let ground = d.level_areas.iter().find(|l| l.level_id == LEVEL).unwrap();
    assert!(near(d.totals.gross_area_m2, ground.gross_m2 + upper.gross_m2, 1e-6));
    // The query reads the same number.
    let sum = doc.query(&Query::ProjectSummary).unwrap();
    let level = sum["levels"].as_array().unwrap().iter().find(|l| l["id"] == up.as_str()).unwrap().clone();
    assert_eq!(level["gross_footprint_area_m2"].as_f64(), Some(r2(want)));
}

// ------------------- GA P1-7: making the set again keeps numbers and revisions

fn sheet_by_number<'a>(doc: &'a Document, number: &str) -> &'a Sheet {
    doc.project().sheet_set.sheets.iter().find(|s| s.number == number).unwrap_or_else(|| panic!("no sheet {number}"))
}

#[test]
fn section_marks_never_take_a_grid_lines_label() {
    // Grids A to C (and 1 to 3): the set's sections become D and E, so a
    // section mark on the plan is never read as a grid line.
    let mut doc = bungalow();
    apply_checked(
        &mut doc,
        Command::AddGridSystem { origin: p(-4000.0, -3000.0), rotation_deg: 0.0, x_spacings_mm: vec![4000.0, 4000.0], y_spacings_mm: vec![4000.0, 4000.0] },
    );
    let grids: Vec<String> = doc.project().elements.iter().filter_map(|e| match e { Element::Grid(g) => Some(g.label.clone()), _ => None }).collect();
    assert!(grids.iter().any(|l| l == "A") && grids.iter().any(|l| l == "C"), "{grids:?}");
    apply_checked(&mut doc, Command::MakeSheetSet { paper: PaperSize::A1, replace: false });
    let sections: Vec<String> = doc
        .project()
        .elements
        .iter()
        .filter_map(|e| match e { Element::ViewMark(m) if m.mark_kind == MarkKind::Section => Some(m.label.clone()), _ => None })
        .collect();
    assert_eq!(sections, vec!["D".to_string(), "E".to_string()]);
}

#[test]
fn making_the_set_again_keeps_sheet_numbers_and_revisions_by_view() {
    let mut doc = bungalow();
    apply_checked(&mut doc, Command::MakeSheetSet { paper: PaperSize::A1, replace: false });
    let numbers = |doc: &Document| doc.project().sheet_set.sheets.iter().map(|s| (s.number.clone(), s.title.clone())).collect::<Vec<_>>();
    let first = numbers(&doc);
    assert_eq!(first.len(), 7);
    let (a2, a5) = (sheet_by_number(&doc, "A-2").id.clone(), sheet_by_number(&doc, "A-5").id.clone());
    let rev = SheetRevision { id: String::new(), mark: "2".into(), date: "2026-11-05".into(), description: "Moved the partition".into() };
    apply_checked(&mut doc, Command::UpsertRevision { revision: rev, sheet_ids: vec![a2.clone(), a5.clone()] });
    let rev_id = doc.project().sheet_set.revisions[0].id.clone();
    // A section C, put on the sections sheet by hand, as the reviewer did.
    let level = doc.project().levels[0].id.clone();
    let c = ViewMark { id: "mark-c".into(), level_id: level, mark_kind: MarkKind::Section, label: "C".into(), start: p(2000.0, -2000.0), end: p(2000.0, 8000.0), flip: false, depth_mm: None };
    apply_checked(&mut doc, Command::AddElement { element: Element::ViewMark(c) });
    let mut sections = sheet_by_number(&doc, "A-5").clone();
    sections.viewports.push(Viewport { id: String::new(), view: ViewRef::Section { mark_id: "mark-c".into() }, scale_denominator: None, position: None, title: None });
    apply_checked(&mut doc, Command::UpsertSheet { sheet: Box::new(sections), index: None });

    // Again on A3, replacing the set.
    apply_checked(&mut doc, Command::MakeSheetSet { paper: PaperSize::A3, replace: true });
    let now = numbers(&doc);
    // Every sheet that came back keeps its number; section C is new, after them.
    assert_eq!(&now[..7], &first[..]);
    assert_eq!(now[7], ("A-8".to_string(), "Sections C".to_string()));
    assert!(doc.project().sheet_set.sheets.iter().all(|s| s.paper == PaperSize::A3));
    // The revision stays, marked on the same sheets, which keep their ids.
    assert_eq!(doc.project().sheet_set.revisions.len(), 1);
    assert_eq!(sheet_by_number(&doc, "A-2").id, a2);
    assert_eq!(sheet_by_number(&doc, "A-2").revision_ids, vec![rev_id.clone()]);
    assert_eq!(sheet_by_number(&doc, "A-5").revision_ids, vec![rev_id]);
    assert!(sheet_by_number(&doc, "A-8").revision_ids.is_empty());
    assert!(sheet_by_number(&doc, "A-6").revision_ids.is_empty());
}

// --------------------------- GA P1-1: service sheets only where the services are

fn service_sheets(doc: &Document) -> Vec<(String, usize)> {
    doc.project()
        .sheet_set
        .sheets
        .iter()
        .filter(|s| !s.number.starts_with("A-"))
        .map(|s| (s.number.clone(), s.viewports.len()))
        .collect()
}

#[test]
fn service_sheets_come_only_with_their_services_and_levels() {
    // No services: no E, P or M sheet.
    let mut doc = bungalow();
    apply_checked(&mut doc, Command::MakeSheetSet { paper: PaperSize::A1, replace: false });
    assert!(service_sheets(&doc).is_empty());

    // The demo has all of them, one viewport per level.
    let mut doc = Document::new(templates::plumbing_demo());
    apply_checked(&mut doc, Command::MakeSheetSet { paper: PaperSize::A1, replace: false });
    let got: Vec<String> = service_sheets(&doc).into_iter().map(|s| s.0).collect();
    assert_eq!(got, vec!["E-1", "E-2", "P-1", "P-2", "M-1"]);

    // An upper floor with no services gets no service viewports.
    let mut doc = Document::new(templates::plumbing_demo());
    apply_checked(&mut doc, Command::AddLevel { name: Some("Second floor".into()), elevation_mm: Some(3000.0), height_mm: Some(3000.0) });
    let up = doc.project().levels.iter().find(|l| l.name == "Second floor").unwrap().id.clone();
    apply_checked(
        &mut doc,
        Command::AddRectRoom { origin: p(0.0, 0.0), width_mm: 4000.0, depth_mm: 3000.0, name: Some("Study".into()), thickness_mm: None, level_id: Some(up.clone()) },
    );
    apply_checked(&mut doc, Command::MakeSheetSet { paper: PaperSize::A1, replace: false });
    let services: Vec<&Sheet> = doc.project().sheet_set.sheets.iter().filter(|s| !s.number.starts_with("A-")).collect();
    assert_eq!(services.len(), 5);
    for s in services {
        assert!(
            s.viewports.iter().all(|v| !matches!(&v.view, ViewRef::Plan { level_id, .. } if *level_id == up)),
            "{} {} shows the second floor",
            s.number,
            s.title
        );
    }

    // Fixtures with no pipes: no plumbing sheets; the other trades stay.
    let mut doc = Document::new(templates::plumbing_demo());
    let pipes: Vec<Id> = doc.project().elements.iter().filter(|e| matches!(e, Element::Pipe(_))).map(|e| e.id().clone()).collect();
    assert!(!pipes.is_empty());
    apply_checked(&mut doc, Command::DeleteElements { ids: pipes });
    assert!(doc.derived().schedule.iter().any(|r| r.group == ScheduleGroup::Plumbing && r.count > 0), "the fixtures stay");
    apply_checked(&mut doc, Command::MakeSheetSet { paper: PaperSize::A1, replace: false });
    let got: Vec<String> = service_sheets(&doc).into_iter().map(|s| s.0).collect();
    assert!(got.iter().all(|n| !n.starts_with("P-")), "{got:?}");
    assert!(got.contains(&"E-1".to_string()) && got.contains(&"M-1".to_string()), "{got:?}");
}

// ------------------ GA P1-6: a partition moved off a four-way joint stays clean

/// An 8 x 5 m house cut in four by two straight partitions, each drawn in
/// two pieces that meet at (4000, 2000), as the GS house's Foyer corner is.
/// Returns the house, the lower vertical piece and the west piece of the
/// horizontal partition.
fn four_way_house() -> (Document, Id, Id) {
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 8000.0, 5000.0, Some("House")));
    let new_wall = |doc: &mut Document, a: Point, b: Point| -> Id {
        let r = apply_checked(doc, add_wall(a, b));
        r.diff.added.iter().find(|id| doc.project().elements.iter().any(|e| matches!(e, Element::Wall(w) if &w.id == *id))).unwrap().clone()
    };
    let west = new_wall(&mut doc, p(4000.0, 2000.0), p(0.0, 2000.0));
    new_wall(&mut doc, p(8000.0, 2000.0), p(4000.0, 2000.0));
    new_wall(&mut doc, p(4000.0, 2000.0), p(4000.0, 5000.0));
    let moving = new_wall(&mut doc, p(4000.0, 0.0), p(4000.0, 2000.0));
    (doc, moving, west)
}

/// The x of every point of a string, sorted.
fn string_xs(d: &Dimension) -> Vec<f64> {
    let mut v: Vec<f64> = string_points(d).iter().map(|q| (q.x * 10.0).round() / 10.0).collect();
    v.sort_by(f64::total_cmp);
    v
}

#[test]
fn moving_a_partition_off_a_four_way_joint_overlaps_nothing_and_strings_follow() {
    let (mut doc, moving, west) = four_way_house();
    // The south-west room's inside string along the partition's west piece,
    // as automatic room dimensions make it.
    let string = Dimension {
        id: "sw-north".into(),
        level_id: LEVEL.into(),
        a: p(0.0, 0.0),
        b: p(1.0, 0.0),
        // The west piece runs west, so a positive offset is its south side.
        offset_mm: 600.0,
        text_override: None,
        through: vec![],
        auto: Some(DimRef::Openings { wall_ids: vec![west.clone()], face: DimFace::Interior }),
    };
    apply_checked(&mut doc, Command::AddElement { element: Element::Dimension(string) });
    let half = wall(doc.project(), &moving).thickness_mm / 2.0;
    let wall_half = walls(doc.project()).iter().find(|w| w.start == p(0.0, 0.0) || w.end == p(0.0, 0.0)).unwrap().thickness_mm / 2.0;
    assert_eq!(string_xs(dimension(doc.project(), "sw-north")), vec![wall_half, 4000.0 - half]);
    let before: Vec<Wall> = walls(doc.project()).into_iter().filter(|w| w.id != moving).cloned().collect();

    apply_checked(&mut doc, Command::MoveElements { ids: vec![moving.clone()], delta: p(300.0, 0.0), stretch_connected: true });
    let moved = wall(doc.project(), &moving).clone();
    assert_eq!((moved.start, moved.end), (p(4300.0, 0.0), p(4300.0, 2000.0)));
    // The other walls stay: the moved wall now ends on the east piece's body.
    for w in &before {
        let now = wall(doc.project(), &w.id);
        assert_eq!((now.start, now.end), (w.start, w.end), "wall {} moved", w.id);
    }
    assert!(doc.derived().issues.iter().all(|i| i.code != "wall_overlap"), "{:?}", doc.derived().issues.iter().map(|i| &i.message).collect::<Vec<_>>());

    // The room's inside string now runs to the moved wall's face: one part,
    // never a point at the old joint or at the moved wall's centerline.
    assert_eq!(string_xs(dimension(doc.project(), "sw-north")), vec![wall_half, 4300.0 - half]);
    let mut fresh = doc.project().clone();
    assert_eq!(guhit_core::dims_auto::rewrite(&mut fresh), 0);

    // Back the other way: the room's side shrinks to the moved wall's face.
    apply_checked(&mut doc, Command::MoveElements { ids: vec![moving.clone()], delta: p(-300.0, 0.0), stretch_connected: true });
    assert_eq!(string_xs(dimension(doc.project(), "sw-north")), vec![wall_half, 4000.0 - half]);
}
