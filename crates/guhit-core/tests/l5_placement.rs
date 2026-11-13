//! Lane L5, catalog v2 and placement (DECISIONS D54): `Query::Catalog`,
//! `Query::PlacementOptions`, the object review items, built-in options and
//! the furnishing plan the copilot and agents share. The house here has five
//! rooms with doors and windows, so the rules meet real walls.

mod common;

use common::*;
use guhit_core::{placement, Document};
use guhit_model::*;
use serde_json::Value;

/// A 12 x 8 m house, five rooms: living (south west), bedroom (north west),
/// kitchen (south east), T&B and master bedroom (north east). Doors between
/// them and a front door; windows on the outside walls.
struct House {
    doc: Document,
    living: Id,
    bedroom: Id,
    kitchen: Id,
    bath: Id,
    master: Id,
}

fn wall_id(doc: &mut Document, a: Point, b: Point, t: f64) -> Id {
    let r = apply_checked(
        doc,
        Command::AddWall { start: a, end: b, thickness_mm: Some(t), height_mm: None, material_id: None, level_id: None, role: None },
    );
    r.diff.added.iter().find(|id| doc.project().elements.iter().any(|e| matches!(e, Element::Wall(w) if &w.id == *id))).expect("wall").clone()
}

fn put_opening(doc: &mut Document, wall: &str, ty: OpeningType, offset: f64, width: f64, sill: Option<f64>, height: Option<f64>) {
    apply_checked(
        doc,
        Command::AddOpening {
            wall_id: wall.into(),
            opening_type: ty,
            offset_mm: offset,
            width_mm: Some(width),
            height_mm: height,
            sill_mm: sill,
            style: None,
            flip_side: None,
            flip_hinge: None,
            type_id: None,
        },
    );
}

fn room_at(doc: &Document, x: f64, y: f64) -> Id {
    let state = doc.state();
    state
        .derived
        .rooms
        .iter()
        .find(|g| inside(p(x, y), &g.polygon))
        .unwrap_or_else(|| panic!("no room at {x}, {y}"))
        .room_id
        .clone()
}

fn set_usage(doc: &mut Document, id: &Id, name: &str, usage: RoomUsage) {
    let mut room = doc
        .project()
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Room(r) if &r.id == id => Some(r.clone()),
            _ => None,
        })
        .expect("room");
    room.name = name.into();
    room.usage = usage;
    room.auto_named = false;
    apply_checked(doc, Command::UpdateElement { element: Element::Room(room) });
}

fn house() -> House {
    let mut doc = blank();
    let s = wall_id(&mut doc, p(0.0, 0.0), p(12000.0, 0.0), 150.0);
    let e = wall_id(&mut doc, p(12000.0, 0.0), p(12000.0, 8000.0), 150.0);
    let n = wall_id(&mut doc, p(12000.0, 8000.0), p(0.0, 8000.0), 150.0);
    let w = wall_id(&mut doc, p(0.0, 8000.0), p(0.0, 0.0), 150.0);
    let mid = wall_id(&mut doc, p(6000.0, 0.0), p(6000.0, 8000.0), 100.0);
    let west = wall_id(&mut doc, p(0.0, 5000.0), p(6000.0, 5000.0), 100.0);
    let east = wall_id(&mut doc, p(6000.0, 4000.0), p(12000.0, 4000.0), 100.0);
    let bath_wall = wall_id(&mut doc, p(8000.0, 4000.0), p(8000.0, 8000.0), 100.0);
    // Doors.
    put_opening(&mut doc, &s, OpeningType::Door, 1500.0, 900.0, None, None);
    put_opening(&mut doc, &west, OpeningType::Door, 5000.0, 800.0, None, None);
    put_opening(&mut doc, &mid, OpeningType::Door, 2500.0, 900.0, None, None);
    put_opening(&mut doc, &east, OpeningType::Door, 1000.0, 700.0, None, None);
    put_opening(&mut doc, &east, OpeningType::Door, 4800.0, 800.0, None, None);
    // Windows: living south and west, bedroom north, kitchen east over the
    // counter, T&B high, master north and east.
    put_opening(&mut doc, &s, OpeningType::Window, 4000.0, 1500.0, Some(900.0), Some(1200.0));
    put_opening(&mut doc, &w, OpeningType::Window, 5500.0, 1200.0, Some(900.0), Some(1200.0));
    put_opening(&mut doc, &n, OpeningType::Window, 9000.0, 1200.0, Some(900.0), Some(1200.0));
    put_opening(&mut doc, &e, OpeningType::Window, 2000.0, 1200.0, Some(1050.0), Some(1000.0));
    put_opening(&mut doc, &n, OpeningType::Window, 5000.0, 600.0, Some(1500.0), Some(500.0));
    put_opening(&mut doc, &n, OpeningType::Window, 2000.0, 1200.0, Some(900.0), Some(1200.0));
    put_opening(&mut doc, &e, OpeningType::Window, 6000.0, 1000.0, Some(900.0), Some(1200.0));
    let _ = bath_wall;
    let living = room_at(&doc, 3000.0, 2500.0);
    let bedroom = room_at(&doc, 3000.0, 6500.0);
    let kitchen = room_at(&doc, 9000.0, 2000.0);
    let bath = room_at(&doc, 7000.0, 6000.0);
    let master = room_at(&doc, 10000.0, 6000.0);
    set_usage(&mut doc, &living, "Living", RoomUsage::Living);
    set_usage(&mut doc, &bedroom, "Bedroom", RoomUsage::Bedroom);
    set_usage(&mut doc, &kitchen, "Kitchen", RoomUsage::Kitchen);
    set_usage(&mut doc, &bath, "T&B", RoomUsage::Bathroom);
    set_usage(&mut doc, &master, "Master bedroom", RoomUsage::MasterBedroom);
    House { doc, living, bedroom, kitchen, bath, master }
}

fn options(doc: &Document, room: &Id, key: &str) -> Value {
    doc.query(&Query::PlacementOptions { room_id: room.clone(), catalog_key: key.into() }).expect("placement options")
}

fn item(key: &str) -> CatalogItem {
    defaults::asset_catalog().into_iter().find(|c| c.key == key).expect("catalog item")
}

fn asset_at(key: &str, x: f64, y: f64, rot: f64) -> Asset {
    let it = item(key);
    Asset {
        id: String::new(),
        level_id: LEVEL.into(),
        catalog_key: it.key,
        name: it.name,
        category: it.category,
        position: p(x, y),
        rotation_deg: rot,
        width_mm: it.width_mm,
        depth_mm: it.depth_mm,
        height_mm: it.height_mm,
        elevation_mm: it.elevation_mm,
        light: it.light,
        links: vec![],
        circuit: String::new(),
        built_in: None,
    }
}

fn add_asset(doc: &mut Document, a: Asset) -> Id {
    apply_checked(doc, Command::AddElement { element: Element::Asset(a) }).diff.added[0].clone()
}

fn corners(c: Point, w: f64, d: f64, rot: f64) -> Vec<Point> {
    let (s, k) = rot.to_radians().sin_cos();
    [(-w / 2.0, -d / 2.0), (w / 2.0, -d / 2.0), (w / 2.0, d / 2.0), (-w / 2.0, d / 2.0)]
        .iter()
        .map(|(x, y)| p(c.x + x * k - y * s, c.y + x * s + y * k))
        .collect()
}

fn spots(v: &Value) -> Vec<(Point, f64)> {
    v["options"]
        .as_array()
        .expect("options")
        .iter()
        .map(|o| (p(o["position"]["x"].as_f64().unwrap(), o["position"]["y"].as_f64().unwrap()), o["rotation_deg"].as_f64().unwrap()))
        .collect()
}

const PLACEMENT_CODES: &[&str] = &["asset_in_wall", "assets_overlap", "door_swing_blocked", "asset_over_opening"];

fn placement_issues(doc: &Document) -> Vec<Issue> {
    doc.derived().issues.iter().filter(|i| PLACEMENT_CODES.contains(&i.code.as_str())).cloned().collect()
}

// ------------------------------------------------------------- catalog

#[test]
fn the_catalog_has_about_fifty_new_items_each_with_rooms_tags_and_a_form() {
    let all = defaults::asset_catalog();
    let start = all.iter().position(|c| c.key == "sofa-leather").expect("catalog v2 starts");
    let v2 = &all[start..];
    assert!(v2.len() >= 45, "{} catalog v2 items", v2.len());
    let mut keys: Vec<&str> = all.iter().map(|c| c.key.as_str()).collect();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), all.len(), "keys are unique");
    for c in v2 {
        assert!(!c.tags.is_empty(), "{} has tags", c.key);
        assert!(c.model.is_some() || c.parametric.is_some() || matches!(c.key.as_str(), "gate-steel" | "hedge" | "lamp-post"), "{} has a model or is drawn in code", c.key);
        if c.model.is_some() {
            assert_eq!(c.model.as_deref(), Some(c.key.as_str()), "pack models are keyed by catalog key");
        }
    }
    let built: Vec<&str> = all.iter().filter(|c| c.parametric.is_some()).map(|c| c.key.as_str()).collect();
    assert_eq!(built, vec!["kitchen-run", "wardrobe-builtin", "vanity", "counter", "shelf-wall"]);
    assert!(all.iter().any(|c| c.category == AssetCategory::Site && c.key == "gate-steel"));
}

#[test]
fn the_catalog_query_searches_names_tags_and_room_usage() {
    let doc = blank();
    let q = |search: Option<&str>, usage: Option<RoomUsage>| doc.query(&Query::Catalog { search: search.map(str::to_string), usage }).expect("catalog");
    let keys = |v: &Value| v["items"].as_array().unwrap().iter().map(|i| i["key"].as_str().unwrap().to_string()).collect::<Vec<_>>();

    let all = q(None, None);
    assert_eq!(all["count"].as_u64().unwrap() as usize, defaults::asset_catalog().len());

    let sofas = keys(&q(Some("sofa"), None));
    assert!(sofas.contains(&"sofa-leather".to_string()) && sofas.contains(&"sofa-wood".to_string()), "{sofas:?}");
    assert!(!sofas.contains(&"wc".to_string()));

    // A tag only: rattan is in no name.
    assert_eq!(keys(&q(Some("rattan"), None)), vec!["basket-rattan"]);
    // Every word must match.
    assert_eq!(keys(&q(Some("sofa wood"), None)), vec!["sofa-wood"]);

    let bedroom = keys(&q(None, Some(RoomUsage::Bedroom)));
    for k in ["bed-double", "nightstand", "wardrobe-builtin", "ceiling-fan"] {
        assert!(bedroom.contains(&k.to_string()), "{k} in {bedroom:?}");
    }
    assert!(!bedroom.contains(&"wc".to_string()));
    // Items for any room come in only when a search narrows the list.
    assert!(!bedroom.contains(&"outlet-duplex".to_string()));
    assert!(keys(&q(Some("outlet"), Some(RoomUsage::Bedroom))).contains(&"outlet-duplex".to_string()));

    let run = q(Some("kitchen run"), None);
    let it = &run["items"][0];
    assert_eq!(it["parametric"], "kitchen_run");
    assert_eq!(it["built_in_defaults"]["sink_at_mm"], 700.0);
    assert_eq!(it["built_in_defaults"]["cooktop_at_mm"], 1800.0);
}

// ------------------------------------------------------------- spots

#[test]
fn spots_keep_the_back_to_a_wall_and_stay_inside_the_room() {
    let h = house();
    let v = options(&h.doc, &h.living, "sofa-leather");
    let list = spots(&v);
    assert!(!list.is_empty(), "{v}");
    let it = item("sofa-leather");
    for (c, rot) in &list {
        // Inside the living room's net floor: x 75..5950, y 75..4950.
        for q in corners(*c, it.width_mm, it.depth_mm, *rot) {
            assert!(q.x > 70.0 && q.x < 5955.0 && q.y > 70.0 && q.y < 4955.0, "corner {q:?} of a spot at {c:?}");
        }
        // The back (+y turned) points at the nearest wall: a step 30 mm behind
        // the back leaves the room or meets a wall face.
        let (s, k) = rot.to_radians().sin_cos();
        let back = p(-s, k);
        let behind = p(c.x + back.x * (it.depth_mm / 2.0 + 30.0), c.y + back.y * (it.depth_mm / 2.0 + 30.0));
        assert!(behind.x < 75.0 || behind.x > 5950.0 || behind.y < 75.0 || behind.y > 4950.0, "spot {c:?} {rot} has its back to the room");
    }
}

#[test]
fn spots_stay_clear_of_doorways_and_swings() {
    let h = house();
    // The front door: south wall, x 1050..1950. Its leaf swings into the living room.
    let it = item("sideboard");
    for (c, rot) in spots(&options(&h.doc, &h.living, "sideboard")) {
        let foot = corners(c, it.width_mm, it.depth_mm, rot);
        let (lo, hi) = (foot.iter().map(|q| q.x).fold(f64::MAX, f64::min), foot.iter().map(|q| q.x).fold(f64::MIN, f64::max));
        let ylo = foot.iter().map(|q| q.y).fold(f64::MAX, f64::min);
        let on_south = ylo < 800.0;
        assert!(!(on_south && lo < 1950.0 && hi > 1050.0), "a sideboard spot at {c:?} blocks the front door");
    }
}

#[test]
fn tall_objects_never_go_under_a_window_and_low_ones_may() {
    let h = house();
    // Bedroom north wall window: x 2400..3600 (offset 9000 from x 12000 going west), sill 900.
    let tall = item("wardrobe-builtin");
    let mut blocked = 0;
    for (c, rot) in spots(&options(&h.doc, &h.bedroom, "wardrobe-builtin")) {
        let foot = corners(c, tall.width_mm, tall.depth_mm, rot);
        let (lo, hi) = (foot.iter().map(|q| q.x).fold(f64::MAX, f64::min), foot.iter().map(|q| q.x).fold(f64::MIN, f64::max));
        let yhi = foot.iter().map(|q| q.y).fold(f64::MIN, f64::max);
        if yhi > 7700.0 && lo < 3600.0 && hi > 2400.0 {
            blocked += 1;
        }
    }
    assert_eq!(blocked, 0, "a wardrobe spot covers the bedroom window");
    // A bed is 500 mm high: under a 900 mm sill is allowed.
    let bed = item("bed-double");
    let under = spots(&options(&h.doc, &h.bedroom, "bed-double")).into_iter().any(|(c, rot)| {
        let foot = corners(c, bed.width_mm, bed.depth_mm, rot);
        let yhi = foot.iter().map(|q| q.y).fold(f64::MIN, f64::max);
        let (lo, hi) = (foot.iter().map(|q| q.x).fold(f64::MAX, f64::min), foot.iter().map(|q| q.x).fold(f64::MIN, f64::max));
        yhi > 7700.0 && lo < 3600.0 && hi > 2400.0
    });
    assert!(under, "a bed may stand under the window");
}

#[test]
fn spots_leave_every_objects_front_clearance_free() {
    let mut h = house();
    // A sofa against the living room's west wall, facing east: its 450 mm
    // front clearance runs x 905..1355.
    let sofa = add_asset(&mut h.doc, asset_at("sofa-leather", 75.0 + 10.0 + 410.0, 2500.0, 90.0));
    assert!(placement_issues(&h.doc).is_empty(), "{:?}", placement_issues(&h.doc));
    let it = item("armchair");
    for (c, rot) in spots(&options(&h.doc, &h.living, "armchair")) {
        let foot = corners(c, it.width_mm, it.depth_mm, rot);
        // Touching the edge of it is fine; standing in it is not.
        let front = [p(905.0, 1595.0), p(1355.0, 1595.0), p(1355.0, 3405.0), p(905.0, 3405.0)];
        let taken = guhit_core::slabs::overlap_area(&foot, &front);
        assert!(taken < 10_000.0, "an armchair spot at {c:?} takes {taken} mm2 of the front of sofa {sofa}");
    }
}

#[test]
fn spots_are_never_in_a_stairwell_or_over_a_void_and_such_objects_are_flagged() {
    // G4 problem 2.
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 8000.0, 6000.0, Some("Living")));
    apply_checked(&mut doc, Command::AddLevel { name: Some("Second floor".into()), elevation_mm: Some(3000.0), height_mm: Some(3000.0) });
    let up = doc.project().levels.iter().find(|l| l.name == "Second floor").unwrap().id.clone();
    apply_checked(
        &mut doc,
        Command::AddRectRoom { origin: p(0.0, 0.0), width_mm: 8000.0, depth_mm: 6000.0, name: Some("Hall".into()), thickness_mm: None, level_id: Some(up.clone()) },
    );
    // A stair along the west wall, arriving upstairs; a void in the east half upstairs.
    apply_checked(
        &mut doc,
        Command::AddElement {
            element: Element::Stair(Stair {
                id: String::new(),
                level_id: LEVEL.into(),
                origin: p(700.0, 1500.0),
                rotation_deg: 0.0,
                width_mm: 1000.0,
                run_mm: 2750.0,
                riser_count: 11,
                railing: None,
                shape: StairShape::Straight,
                turn: Turn::Left,
                first_flight_risers: None,
                landing_mm: None,
                well_mm: 0.0,
            }),
        },
    );
    let void = Slab {
        id: String::new(),
        level_id: up.clone(),
        kind: SlabKind::Void,
        polygon: vec![p(5000.0, 1500.0), p(7925.0, 1500.0), p(7925.0, 4500.0), p(5000.0, 4500.0)],
        thickness_mm: 150.0,
        top_offset_mm: 0.0,
        material_id: None,
        railing: None,
    };
    apply_checked(&mut doc, Command::AddElement { element: Element::Slab(void) });
    let living = room_named(doc.project(), "Living").id.clone();
    let hall = room_named(doc.project(), "Hall").id.clone();
    let stair = [p(200.0, 1500.0), p(1200.0, 1500.0), p(1200.0, 4250.0), p(200.0, 4250.0)];
    let hole = [p(5000.0, 1500.0), p(7925.0, 1500.0), p(7925.0, 4500.0), p(5000.0, 4500.0)];
    let overlap = |foot: &[Point], poly: &[Point]| guhit_core::slabs::overlap_area(foot, poly);
    for (room, key, poly) in [(&living, "bed-double", &stair), (&hall, "sofa-leather", &stair), (&hall, "sofa-leather", &hole), (&hall, "dining-set-4", &hole)] {
        let it = item(key);
        let list = spots(&options(&doc, room, key));
        assert!(!list.is_empty(), "{key} has some spot");
        for (c, rot) in list {
            let foot = corners(c, it.width_mm, it.depth_mm, rot);
            assert!(overlap(&foot, poly) < 10_000.0, "{key} offered at {c:?} over the stair or void");
        }
    }
    // Placed there anyway, it is flagged: on the stair downstairs, partly
    // over the opening and the void upstairs.
    let on_stair = add_asset(&mut doc, asset_at("armchair", 700.0, 3000.0, 0.0));
    let mut up_sofa = asset_at("sofa-2", 4500.0, 3000.0, 0.0);
    up_sofa.level_id = up.clone();
    let partly = add_asset(&mut doc, up_sofa);
    let flagged: Vec<Id> = doc.derived().issues.iter().filter(|i| i.code == "asset_over_opening").flat_map(|i| i.element_ids.clone()).collect();
    assert!(flagged.contains(&on_stair), "on the stair: {flagged:?}");
    assert!(flagged.contains(&partly), "partly over the void: {flagged:?}");
}

#[test]
fn ceiling_objects_go_in_the_middle_and_wall_objects_on_a_wall() {
    let h = house();
    let fan = options(&h.doc, &h.living, "ceiling-fan");
    assert_eq!(fan["options"][0]["kind"], "ceiling", "{fan}");
    let art = options(&h.doc, &h.living, "wall-art");
    let list = art["options"].as_array().unwrap();
    assert!(!list.is_empty());
    assert!(list.iter().all(|o| o["kind"] == "wall"));
    // A dining set is offered in the middle of the room as well.
    let dining = options(&h.doc, &h.living, "dining-set-4");
    assert!(dining["options"].as_array().unwrap().iter().any(|o| o["kind"] == "center"), "{dining}");
}

#[test]
fn an_unknown_room_or_key_is_refused_and_a_full_room_says_why() {
    let h = house();
    assert!(h.doc.query(&Query::PlacementOptions { room_id: "nope".into(), catalog_key: "bed-double".into() }).is_err());
    let e = h.doc.query(&Query::PlacementOptions { room_id: h.bath.clone(), catalog_key: "no-such-thing".into() }).unwrap_err();
    assert_eq!(code(&e), "unknown_catalog_key");
    // A 4.4 m car does not fit the 1.9 x 3.9 m T&B.
    let v = options(&h.doc, &h.bath, "car-covered");
    assert!(v["options"].as_array().unwrap().is_empty());
    assert!(v["reason"].as_str().unwrap().starts_with("No free spot for Car, under a cover in T&B"), "{v}");
}

// ------------------------------------------------------------- review

#[test]
fn objects_in_walls_and_on_top_of_each_other_are_flagged() {
    let mut h = house();
    // Straddling the living room's west wall.
    let in_wall = add_asset(&mut h.doc, asset_at("wardrobe", 0.0, 2500.0, 270.0));
    let a = add_asset(&mut h.doc, asset_at("sofa-3", 3000.0, 2500.0, 0.0));
    let b = add_asset(&mut h.doc, asset_at("sofa-2", 3300.0, 2600.0, 0.0));
    let issues = h.doc.derived().issues.clone();
    assert!(issues.iter().any(|i| i.code == "asset_in_wall" && i.element_ids[0] == in_wall), "{issues:?}");
    assert!(issues.iter().any(|i| i.code == "assets_overlap" && i.element_ids.contains(&a) && i.element_ids.contains(&b)));
    assert!(is_review_code_ok("asset_in_wall") && is_review_code_ok("assets_overlap"));
}

#[test]
fn a_tree_is_its_trunk_so_its_crown_may_hang_over_the_house() {
    let mut h = house();
    // A 3 m crown 1 m off the west wall reaches over the living room's sofa.
    add_asset(&mut h.doc, asset_at("sofa-3", 75.0 + 10.0 + 450.0, 2500.0, 90.0));
    let tree = add_asset(&mut h.doc, asset_at("tree", -1000.0, 2500.0, 0.0));
    let found = placement_issues(&h.doc);
    assert!(found.is_empty(), "{found:?}");
    // Its trunk in the wall is flagged.
    let mut moved = h.doc.project().elements.iter().find_map(|e| match e {
        Element::Asset(a) if a.id == tree => Some(a.clone()),
        _ => None,
    }).unwrap();
    moved.position = p(0.0, 2500.0);
    apply_checked(&mut h.doc, Command::UpdateElement { element: Element::Asset(moved) });
    assert!(placement_issues(&h.doc).iter().any(|i| i.code == "asset_in_wall" && i.element_ids[0] == tree));
}

fn is_review_code_ok(code: &str) -> bool {
    guhit_core::is_review_code(code)
}

#[test]
fn heights_keep_apart_what_stands_apart() {
    let mut h = house();
    // A sofa in front of an outlet, a microwave on a kitchen run, a ceiling
    // fan over a coffee table, art over a sofa: none of them clash.
    let mut sofa = asset_at("sofa-leather", 3000.0, 4950.0 - 10.0 - 410.0, 0.0);
    sofa.id = "sofa".into();
    add_asset(&mut h.doc, sofa);
    add_asset(&mut h.doc, asset_at("outlet-duplex", 3000.0, 4950.0 - 20.0, 0.0));
    add_asset(&mut h.doc, asset_at("wall-art", 3000.0, 4950.0 - 10.0, 0.0));
    add_asset(&mut h.doc, asset_at("coffee-table-round", 3000.0, 2700.0, 0.0));
    add_asset(&mut h.doc, asset_at("ceiling-fan", 3000.0, 2700.0, 0.0));
    let mut run = asset_at("kitchen-run", 9000.0, 75.0 + 10.0 + 300.0, 180.0);
    run.built_in = Some(Box::new(placement::built_in_defaults(BuiltInKind::KitchenRun, run.width_mm)));
    add_asset(&mut h.doc, run);
    add_asset(&mut h.doc, asset_at("microwave", 8200.0, 75.0 + 10.0 + 200.0, 180.0));
    let found = placement_issues(&h.doc);
    assert!(found.is_empty(), "{found:?}");
}

// ------------------------------------------------------------- built-ins

#[test]
fn built_in_defaults_follow_one_rule_with_the_frontend() {
    use BuiltInKind as K;
    // The same table is in src/viewer3d/scene/builtins.test.ts.
    let cases = [
        (K::KitchenRun, 2400.0, Some(700.0), Some(1800.0), 5, true),
        (K::KitchenRun, 1800.0, Some(550.0), Some(1450.0), 4, true),
        (K::KitchenRun, 1500.0, Some(450.0), None, 3, true),
        (K::KitchenRun, 600.0, Some(300.0), None, 1, true),
        (K::KitchenRun, 3600.0, Some(1100.0), Some(2700.0), 8, true),
        (K::Vanity, 900.0, Some(450.0), None, 2, true),
        (K::Counter, 1500.0, None, None, 3, false),
        (K::Wardrobe, 1800.0, None, None, 4, false),
        (K::Wardrobe, 600.0, None, None, 2, false),
        (K::Shelf, 1200.0, None, None, 0, false),
    ];
    for (kind, w, sink, cook, doors, upper) in cases {
        let d = placement::built_in_defaults(kind, w);
        assert_eq!((d.sink_at_mm, d.cooktop_at_mm, d.doors, d.upper), (sink, cook, doors, upper), "{kind:?} {w}");
    }
}

#[test]
fn built_in_options_are_checked() {
    let mut h = house();
    let mut run = asset_at("kitchen-run", 9000.0, 385.0, 180.0);
    let mut bad = |p: BuiltInParams, a: &mut Asset| {
        a.built_in = Some(Box::new(p));
        code(&apply_rejected(&mut h.doc, Command::AddElement { element: Element::Asset(a.clone()) }))
    };
    let ok = BuiltInParams { upper: true, sink_at_mm: Some(700.0), cooktop_at_mm: Some(1800.0), doors: 5 };
    assert_eq!(bad(BuiltInParams { sink_at_mm: Some(50.0), ..ok }, &mut run), "built_in_options");
    assert_eq!(bad(BuiltInParams { cooktop_at_mm: Some(1000.0), ..ok }, &mut run), "built_in_options");
    assert_eq!(bad(BuiltInParams { doors: 40, ..ok }, &mut run), "built_in_options");
    let mut sofa = asset_at("sofa-3", 3000.0, 2500.0, 0.0);
    assert_eq!(bad(ok, &mut sofa), "built_in_options");
    let mut wardrobe = asset_at("wardrobe-builtin", 3000.0, 6000.0, 0.0);
    assert_eq!(bad(BuiltInParams { upper: false, sink_at_mm: Some(500.0), cooktop_at_mm: None, doors: 3 }, &mut wardrobe), "built_in_options");
    run.built_in = Some(Box::new(ok));
    apply_checked(&mut h.doc, Command::AddElement { element: Element::Asset(run) });
}

// ------------------------------------------------------------- furnishing

#[test]
fn furnishing_five_rooms_raises_no_placement_review_item() {
    let mut h = house();
    let rooms = [(&h.living, "Living"), (&h.bedroom, "Bedroom"), (&h.kitchen, "Kitchen"), (&h.bath, "T&B"), (&h.master, "Master bedroom")];
    let ids: Vec<Id> = rooms.iter().map(|(id, _)| (*id).clone()).collect();
    let mut placed: Vec<(String, Vec<String>)> = vec![];
    for id in &ids {
        let plan = placement::furnish_room(h.doc.project(), id, None, "modern").expect("plan");
        assert!(!plan.assets.is_empty(), "{} got nothing: {:?}", plan.room_name, plan.skipped);
        placed.push((plan.room_name.clone(), plan.assets.iter().map(|a| a.catalog_key.clone()).collect()));
        let label = format!("Furnish {}", plan.room_name);
        apply_checked(&mut h.doc, Command::Batch { label, commands: plan.commands() });
    }
    let found = placement_issues(&h.doc);
    assert!(found.is_empty(), "{found:#?}\n{placed:#?}");
    let has = |room: &str, key: &str| placed.iter().any(|(r, keys)| r == room && keys.iter().any(|k| k == key));
    assert!(has("Living", "sofa-leather"), "{placed:?}");
    assert!(has("Living", "coffee-table-round") || has("Living", "coffee-table"), "{placed:?}");
    assert!(has("Bedroom", "bed-double"), "{placed:?}");
    assert!(has("Master bedroom", "bed-queen"), "{placed:?}");
    assert!(placed.iter().find(|(r, _)| r == "Master bedroom").unwrap().1.iter().filter(|k| *k == "nightstand").count() == 2, "{placed:?}");
    assert!(has("Kitchen", "kitchen-run"), "{placed:?}");
    assert!(has("T&B", "wc"), "{placed:?}");
    assert!(has("T&B", "shower"), "{placed:?}");
    // Every object stands inside the room it was planned for.
    let state = h.doc.state();
    for (id, name) in rooms {
        let poly = &state.derived.rooms.iter().find(|g| &g.room_id == id).unwrap().polygon;
        let keys = &placed.iter().find(|(r, _)| r == name).unwrap().1;
        let inside_count = state
            .project
            .elements
            .iter()
            .filter(|e| matches!(e, Element::Asset(a) if inside(a.position, poly)))
            .count();
        assert_eq!(inside_count, keys.len(), "{name}: {keys:?}");
    }
}

#[test]
fn the_kitchen_run_is_sized_to_its_wall_with_the_sink_under_the_window() {
    let h = house();
    let plan = placement::furnish_room(h.doc.project(), &h.kitchen, Some(&["kitchen-run".to_string()]), "modern").unwrap();
    let run = plan.assets.iter().find(|a| a.catalog_key == "kitchen-run").expect("a run");
    assert!(run.width_mm >= 2400.0 && run.width_mm <= 3600.0 && run.width_mm % 50.0 == 0.0, "{}", run.width_mm);
    let p = run.built_in.as_deref().expect("options");
    // Sink and cooktop sit on the run, 900 mm or more apart.
    let sink = p.sink_at_mm.expect("sink");
    assert!(sink >= 450.0 && sink <= run.width_mm - 450.0);
    if let Some(c) = p.cooktop_at_mm {
        assert!((c - sink).abs() >= 900.0 - 1e-6);
    }
    // The east window (y 1400..2600) is over it when the run is on the east
    // wall, and then the sink is under it.
    if run.position.x > 11000.0 {
        let (s, k) = run.rotation_deg.to_radians().sin_cos();
        let left = p_left(run.position, run.width_mm, k, s);
        let sink_y = left.y + s * sink;
        assert!((1400.0..=2600.0).contains(&sink_y), "sink at y {sink_y}");
        assert!(!p.upper, "wall cabinets would cover the window");
    }
}

fn p_left(c: Point, w: f64, k: f64, s: f64) -> Point {
    p(c.x - k * w / 2.0, c.y - s * w / 2.0)
}

#[test]
fn a_plan_is_the_same_twice_and_picks_are_placed_in_order() {
    let h = house();
    let a = placement::furnish_room(h.doc.project(), &h.bedroom, None, "tropical").unwrap();
    let b = placement::furnish_room(h.doc.project(), &h.bedroom, None, "tropical").unwrap();
    assert_eq!(a, b);
    let picks: Vec<String> = ["nightstand", "bed-single", "no-such-key", "plant-leafy"].iter().map(|s| s.to_string()).collect();
    let plan = placement::furnish_room(h.doc.project(), &h.bedroom, Some(&picks), "modern").unwrap();
    let keys: Vec<&str> = plan.assets.iter().map(|a| a.catalog_key.as_str()).collect();
    // The bed comes first so the nightstands find it.
    assert_eq!(keys.first(), Some(&"bed-single"), "{keys:?}");
    assert_eq!(keys.iter().filter(|k| **k == "nightstand").count(), 2, "{keys:?}");
    assert!(plan.skipped.iter().any(|(k, why)| k == "no-such-key" && why == "not in the catalog"));
    assert!(plan.summary().starts_with("Bedroom: single bed"), "{}", plan.summary());
}
