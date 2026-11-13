//! Lane L9, tours (DECISIONS D54): the automatic tour (`MakeTour`), its
//! frames (`Query::TourFrames`) and `tour_through_wall`.

mod common;

use common::*;
use guhit_core::{templates, Document};
use guhit_model::*;
use serde_json::{json, Value};

const EYE: f64 = 1600.0;

fn auto(exterior: bool, dusk: bool) -> AutoTour {
    AutoTour { exterior, day_to_dusk: dusk, pace_mps: 1.0, levels: vec![] }
}

fn make(doc: &mut Document, options: AutoTour) -> Tour {
    apply_checked(doc, Command::MakeTour { name: None, options });
    doc.project()
        .elements
        .iter()
        .rev()
        .find_map(|e| match e {
            Element::Tour(t) => Some(t.clone()),
            _ => None,
        })
        .expect("a tour")
}

fn frames(doc: &Document, tour: &Tour, fps: Option<u32>) -> Value {
    doc.query(&Query::TourFrames { tour_id: tour.id.clone(), fps }).expect("frames")
}

fn v(f: &Value, key: &str) -> Vec3 {
    serde_json::from_value(f[key].clone()).expect("a point")
}

fn through_wall(doc: &Document) -> Vec<&Issue> {
    doc.derived().issues.iter().filter(|i| i.code == "tour_through_wall").collect()
}

fn data(name: &str) -> Project {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name);
    serde_json::from_str(&std::fs::read_to_string(path).expect("test data exists")).expect("a project file reads")
}

/// The bungalow with a second floor over it, reached by a straight stair in
/// the living room: a hall and a bedroom upstairs, with a door between.
fn two_storey() -> Project {
    let mut p = templates::sample_bungalow();
    let ground = p.levels[0].id.clone();
    let mut add = |v: Value| p.elements.push(serde_json::from_value(v).expect("an element"));
    let up = "lvl-up";
    for (i, (a, b)) in [((0.0, 0.0), (8000.0, 0.0)), ((8000.0, 0.0), (8000.0, 6000.0)), ((8000.0, 6000.0), (0.0, 6000.0)), ((0.0, 6000.0), (0.0, 0.0)), ((5000.0, 0.0), (5000.0, 6000.0))]
        .iter()
        .enumerate()
    {
        add(json!({ "kind": "wall", "id": format!("up-wall-{i}"), "level_id": up, "start": { "x": a.0, "y": a.1 }, "end": { "x": b.0, "y": b.1 }, "thickness_mm": 150.0, "height_mm": null, "material_id": null }));
    }
    add(json!({ "kind": "opening", "id": "up-door", "wall_id": "up-wall-4", "opening_type": "door", "style": "swing_single", "offset_mm": 3000.0, "width_mm": 900.0, "height_mm": 2100.0, "sill_mm": 0.0, "flip_side": false, "flip_hinge": false, "material_id": null }));
    add(json!({ "kind": "room", "id": "up-hall", "level_id": up, "name": "Hall", "usage": "hallway", "seed": { "x": 2500.0, "y": 3000.0 }, "floor_material_id": null, "auto_named": false }));
    add(json!({ "kind": "room", "id": "up-bed", "level_id": up, "name": "Bedroom 2", "usage": "bedroom", "seed": { "x": 6500.0, "y": 3000.0 }, "floor_material_id": null, "auto_named": false }));
    add(json!({ "kind": "stair", "id": "stair-1", "level_id": ground, "origin": { "x": 4000.0, "y": 1500.0 }, "rotation_deg": 0.0, "width_mm": 1000.0, "run_mm": 3000.0, "riser_count": 17, "railing": null }));
    p.levels.push(Level { id: up.into(), name: "Second Floor".into(), elevation_mm: 3000.0, height_mm: 3000.0 });
    p
}

fn room_polygon<'a>(doc: &'a Document, id: &str) -> &'a [Point] {
    &doc.derived().rooms.iter().find(|r| r.room_id == id).expect("room geometry").polygon
}

fn inside(pt: Point, poly: &[Point]) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > pt.y) != (b.y > pt.y) && pt.x < (b.x - a.x) * (pt.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
    }
    c
}

/// Rooms with at least one door on their walls.
fn rooms_with_doors(doc: &Document) -> Vec<(Id, Id)> {
    let p = doc.project();
    let door_walls: Vec<&Id> = p
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Opening(o) if o.opening_type == OpeningType::Door => Some(&o.wall_id),
            _ => None,
        })
        .collect();
    doc.derived()
        .rooms
        .iter()
        .filter(|r| r.wall_ids.iter().any(|w| door_walls.contains(&w)))
        .filter_map(|r| {
            p.elements.iter().find_map(|e| match e {
                Element::Room(room) if room.id == r.room_id => Some((room.id.clone(), room.level_id.clone())),
                _ => None,
            })
        })
        .collect()
}

/// Every room with a door has a frame standing in it at eye height.
fn assert_visits_rooms(doc: &Document, f: &Value) {
    let list = f["frames"].as_array().unwrap();
    for (room, level) in rooms_with_doors(doc) {
        let l = doc.project().levels.iter().find(|l| l.id == level).unwrap();
        let poly = room_polygon(doc, &room);
        let seen = list.iter().any(|x| {
            let p = v(x, "position");
            (p.z - (l.elevation_mm + EYE)).abs() < 1.0 && inside(Point { x: p.x, y: p.y }, poly)
        });
        assert!(seen, "the tour never stands in room {room}");
    }
}

/// No jump within a shot, nor across shots that join: each step at most
/// one frame's travel at the shot's top speed (the pace for walks).
fn assert_continuous(tour: &Tour, f: &Value, pace_mps: f64) {
    let fps = f["fps"].as_f64().unwrap();
    let list = f["frames"].as_array().unwrap();
    let speeds: Vec<f64> = tour
        .shots
        .iter()
        .map(|s| {
            if s.kind == ShotMove::Walk {
                return pace_mps * 1000.0;
            }
            let mut len = 0.0;
            let mut prev: Option<Vec3> = None;
            for x in list.iter().filter(|x| x["shot"].as_u64().unwrap() as usize == tour.shots.iter().position(|o| o.id == s.id).unwrap()) {
                let p = v(x, "position");
                if let Some(q) = prev {
                    len += ((p.x - q.x).powi(2) + (p.y - q.y).powi(2) + (p.z - q.z).powi(2)).sqrt();
                }
                prev = Some(p);
            }
            // The eased top speed: 1 / (1 - ramp) of the average.
            len / s.seconds / (1.0 - guhit_core::tours::EASE_RAMP)
        })
        .collect();
    let mut walked = 0;
    for w in list.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let sa = a["shot"].as_u64().unwrap() as usize;
        let sb = b["shot"].as_u64().unwrap() as usize;
        let (pa, pb) = (v(a, "position"), v(b, "position"));
        let step = ((pa.x - pb.x).powi(2) + (pa.y - pb.y).powi(2) + (pa.z - pb.z).powi(2)).sqrt();
        if sa != sb && tour.shots[sb].transition.kind != TransitionKind::Cut {
            continue;
        }
        if b.get("fx").is_some() {
            continue;
        }
        let top = speeds[sa].max(speeds[sb]);
        assert!(step <= top / fps * 1.02 + 1.0, "a jump of {step:.0} mm at {} s (shot {sb}, top speed {top:.0} mm/s)", b["t"]);
        if tour.shots[sb].kind == ShotMove::Walk {
            assert!(step <= pace_mps * 1000.0 / fps * 1.01 + 0.2, "a walk faster than the pace at {} s: {step:.1} mm", b["t"]);
            walked += 1;
        }
    }
    assert!(walked > 0, "the tour walks");
}

#[test]
fn the_bungalow_tour_enters_by_the_front_door_and_visits_every_room() {
    let mut doc = Document::new(templates::sample_bungalow());
    let tour = make(&mut doc, auto(true, true));
    let kinds: Vec<ShotMove> = tour.shots.iter().map(|s| s.kind).collect();
    assert_eq!(&kinds[..3], &[ShotMove::FlyIn, ShotMove::Orbit, ShotMove::PushIn], "{kinds:?}");
    assert_eq!(tour.shots[0].transition.kind, TransitionKind::PlanLift);
    let dusk = tour.shots.last().unwrap();
    assert_eq!(dusk.name, "Dusk");
    assert!(dusk.light.unwrap().minutes < dusk.light_end.unwrap().minutes, "day to dusk");
    assert!(tour.shots.iter().all(|s| !s.id.is_empty()));

    let f = frames(&doc, &tour, None);
    assert_eq!(f["fps"], 30);
    let total: f64 = tour.shots.iter().map(|s| s.seconds).sum();
    assert_eq!(f["frame_count"].as_u64().unwrap(), (total * 30.0).round() as u64);
    // In through the front door: a frame at its middle, at eye height.
    let list = f["frames"].as_array().unwrap();
    let through = list.iter().any(|x| {
        let p = v(x, "position");
        (p.x - 1500.0).abs() < 60.0 && p.y.abs() < 60.0 && (p.z - EYE).abs() < 1.0
    });
    assert!(through, "the camera goes through the front door");
    assert_visits_rooms(&doc, &f);
    assert_continuous(&tour, &f, 1.0);
    assert!(through_wall(&doc).is_empty(), "{:?}", through_wall(&doc));
}

#[test]
fn a_two_storey_tour_climbs_the_stair() {
    let mut doc = Document::new(two_storey());
    let tour = make(&mut doc, auto(true, false));
    let up = tour.shots.iter().position(|s| s.name == "Up the stairs").expect("a climb");
    let f = frames(&doc, &tour, None);
    let list = f["frames"].as_array().unwrap();
    // On the flight the camera rises from the ground floor's eye height to
    // the second floor's, inside the stair's plan.
    let on_flight: Vec<Vec3> = list
        .iter()
        .filter(|x| x["shot"].as_u64().unwrap() as usize == up)
        .map(|x| v(x, "position"))
        .filter(|p| p.x > 3500.0 && p.x < 4500.0 && p.y > 1500.0 && p.y < 4500.0)
        .collect();
    assert!(on_flight.len() > 30, "frames on the flight: {}", on_flight.len());
    assert!(on_flight.windows(2).all(|w| w[1].z >= w[0].z - 0.5), "the climb never drops");
    assert!(on_flight.first().unwrap().z < EYE + 300.0 && on_flight.last().unwrap().z > 3000.0 + EYE - 300.0);
    assert_visits_rooms(&doc, &f);
    assert_continuous(&tour, &f, 1.0);
    assert!(through_wall(&doc).is_empty(), "{:?}", through_wall(&doc));
    // Upstairs after the climb: the hall, then through the door to the bedroom.
    let names: Vec<&str> = tour.shots[up..].iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"Hall") && names.contains(&"Bedroom 2"), "{names:?}");
}

#[test]
fn the_tour_of_the_house_with_services_and_of_the_lovell_house() {
    let mut doc = Document::new(templates::plumbing_demo());
    let tour = make(&mut doc, auto(true, true));
    let f = frames(&doc, &tour, None);
    assert_visits_rooms(&doc, &f);
    assert_continuous(&tour, &f, 1.0);
    assert!(through_wall(&doc).is_empty(), "{:?}", through_wall(&doc));

    // Four levels, three stairs, fourteen rooms: every room with a door, and
    // at least one climb.
    let mut doc = Document::new(data("wave1-g5-lovell.project.json"));
    let tour = make(&mut doc, auto(false, false));
    let f = frames(&doc, &tour, None);
    assert_visits_rooms(&doc, &f);
    assert!(tour.shots.iter().filter(|s| s.name == "Up the stairs").count() >= 2);
    assert_continuous(&tour, &f, 1.0);
    // The walks go round every wall. The one item left is the model's: a wall
    // of the main floor stands over the lower flight, under 2 m above its
    // treads, and the camera at eye height meets it (stair headroom).
    let items = through_wall(&doc);
    assert!(items.len() <= 1, "{items:?}");
    for i in items {
        assert!(i.message.contains("\"Up the stairs\""), "{}", i.message);
    }
}

#[test]
fn a_tour_through_a_wall_is_flagged_and_making_one_is_one_undo_step() {
    let mut doc = Document::new(templates::sample_bungalow());
    let before = doc.project().elements.len();
    let tour = make(&mut doc, auto(false, false));
    assert_eq!(doc.project().elements.len(), before + 1);
    assert!(through_wall(&doc).is_empty());
    // A shot straight through the wall between the rooms, away from its door.
    let mut t = tour.clone();
    let k = |x: f64| ShotKey { at: if x < 5000.0 { 0.0 } else { 1.0 }, position: Vec3 { x, y: 1000.0, z: EYE }, target: Vec3 { x: x + 3000.0, y: 1000.0, z: EYE }, fov_deg: 60.0 };
    t.shots.push(Shot {
        id: String::new(),
        name: "Through".into(),
        kind: ShotMove::Path,
        keys: vec![k(3000.0), k(7000.0)],
        seconds: 4.0,
        ease: Ease::Linear,
        light: None,
        light_end: None,
        transition: Transition { kind: TransitionKind::Dissolve, seconds: 0.5 },
    });
    apply_checked(&mut doc, Command::UpdateElement { element: Element::Tour(t) });
    let items = through_wall(&doc);
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].severity, Severity::Warning);
    assert!(items[0].element_ids.contains(&tour.id));
    assert!(items[0].message.contains("Through"), "{}", items[0].message);
    let at = items[0].location.unwrap();
    assert!((at.x - 5000.0).abs() < 60.0, "{at:?}");
    doc.undo().unwrap();
    doc.undo().unwrap();
    assert_eq!(doc.project().elements.len(), before);
}

#[test]
fn make_tour_refuses_a_bad_pace_and_an_empty_plan() {
    let mut doc = Document::new(templates::sample_bungalow());
    let e = apply_rejected(&mut doc, Command::MakeTour { name: None, options: AutoTour { pace_mps: 9.0, ..auto(true, true) } });
    assert_eq!(code(&e), "tour_pace");
    let mut doc = blank();
    let e = apply_rejected(&mut doc, Command::MakeTour { name: None, options: auto(true, true) });
    assert_eq!(code(&e), "tour_empty");
}

#[test]
fn frames_follow_the_frame_rate_asked_for() {
    let mut doc = Document::new(templates::sample_bungalow());
    let tour = make(&mut doc, auto(false, false));
    let total: f64 = tour.shots.iter().map(|s| s.seconds).sum();
    for fps in [24, 60] {
        let f = frames(&doc, &tour, Some(fps));
        assert_eq!(f["frame_count"].as_u64().unwrap(), (total * fps as f64).round() as u64);
        let list = f["frames"].as_array().unwrap();
        assert!((list[1]["t"].as_f64().unwrap() - 1.0 / fps as f64).abs() < 1e-3);
    }
    // A second tour gets the next name.
    let again = make(&mut doc, auto(false, false));
    assert_eq!(again.name, "Tour 2");
}

/// What a tour adds to every staged step: `tour_through_wall` samples it.
/// Run: cargo test --release -p guhit-core --test l9_tours review_cost -- --ignored --nocapture
#[test]
#[ignore]
fn review_cost() {
    let mut doc = Document::new(data("wave1-g5-lovell.project.json"));
    let plain = doc.project().clone();
    let tour = make(&mut doc, auto(true, true));
    let with = doc.project().clone();
    let time = |p: &Project| {
        let t0 = std::time::Instant::now();
        for _ in 0..20 {
            std::hint::black_box(guhit_core::compute_derived(p));
        }
        t0.elapsed().as_secs_f64() * 1000.0 / 20.0
    };
    let seconds: f64 = tour.shots.iter().map(|s| s.seconds).sum();
    eprintln!("Lovell, {} shots, {seconds:.0} s: derive {:.2} ms without the tour, {:.2} ms with it", tour.shots.len(), time(&plain), time(&with));
}
