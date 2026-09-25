//! Electrical and aircon services (docs/CONTRACT.md, "Pipes", "Devices,
//! fixtures and links", "Review marks"): the schedule, review marks, links,
//! device validation and layers, service runs, every device and aircon
//! review item, the services demo and migration from version 2.

mod common;

use common::*;
use guhit_core::{compute_derived, templates, Document, REVIEW_CODES};
use guhit_model::*;
use serde_json::{json, Value};

// ------------------------------------------------------------------ setup

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3 { x, y, z }
}

fn wall_el(id: &str, a: (f64, f64), b: (f64, f64)) -> Element {
    Element::Wall(Wall {
        id: id.into(),
        level_id: LEVEL.into(),
        start: p(a.0, a.1),
        end: p(b.0, b.1),
        thickness_mm: 150.0,
        height_mm: None,
        material_id: None,
    })
}

fn room_el(id: &str, name: &str, usage: RoomUsage, seed: (f64, f64)) -> Element {
    Element::Room(Room {
        id: id.into(),
        level_id: LEVEL.into(),
        name: name.into(),
        usage,
        seed: p(seed.0, seed.1),
        floor_material_id: None,
        auto_named: false,
    })
}

/// A closed 6.0 x 4.0 m bedroom of 150 mm walls on centerlines. Inner
/// faces at x = 75 and 5925, y = 75 and 3925; outer faces 75 mm outside.
fn shell() -> Vec<Element> {
    vec![
        wall_el("w-south", (0.0, 0.0), (6000.0, 0.0)),
        wall_el("w-east", (6000.0, 0.0), (6000.0, 4000.0)),
        wall_el("w-north", (6000.0, 4000.0), (0.0, 4000.0)),
        wall_el("w-west", (0.0, 4000.0), (0.0, 0.0)),
        room_el("room-1", "Bedroom", RoomUsage::Bedroom, (3000.0, 2000.0)),
    ]
}

/// A catalog object at its catalog size and height.
fn obj(id: &str, key: &str, x: f64, y: f64, rotation: f64, links: &[&str]) -> Element {
    let item = defaults::asset_catalog()
        .into_iter()
        .find(|c| c.key == key)
        .unwrap_or_else(|| panic!("no catalog item {key}"));
    Element::Asset(Asset {
        id: id.into(),
        level_id: LEVEL.into(),
        catalog_key: item.key,
        name: item.name,
        category: item.category,
        position: p(x, y),
        rotation_deg: rotation,
        width_mm: item.width_mm,
        depth_mm: item.depth_mm,
        height_mm: item.height_mm,
        elevation_mm: item.elevation_mm,
        light: item.light,
        links: links.iter().map(|s| s.to_string()).collect(),
        circuit: String::new(),
    })
}

fn raised(mut e: Element, elevation: f64) -> Element {
    if let Element::Asset(a) = &mut e {
        a.elevation_mm = elevation;
    }
    e
}

fn on_level(mut e: Element, level: &str) -> Element {
    match &mut e {
        Element::Asset(a) => a.level_id = level.into(),
        Element::Wall(w) => w.level_id = level.into(),
        Element::Pipe(p) => p.level_id = level.into(),
        _ => {}
    }
    e
}

fn pipe_el(id: &str, system: PipeSystem, d: f64, pts: &[(f64, f64, f64)]) -> Element {
    Element::Pipe(Pipe {
        id: id.into(),
        level_id: LEVEL.into(),
        system,
        material: defaults::pipe_defaults(system).0,
        diameter_mm: d,
        points: pts.iter().map(|(x, y, z)| v(*x, *y, *z)).collect(),
        name: String::new(),
    })
}

fn project_with(elements: Vec<Element>) -> Project {
    let mut project = blank_project();
    project.elements = elements;
    project
}

fn derive(elements: Vec<Element>) -> Derived {
    let d = compute_derived(&project_with(elements));
    assert_calm(&d);
    d
}

fn doc_with(elements: Vec<Element>) -> Document {
    Document::new(project_with(elements))
}

fn items<'a>(d: &'a Derived, code: &str) -> Vec<&'a Issue> {
    d.issues.iter().filter(|i| i.code == code).collect()
}

fn near(a: Vec3, b: Vec3) -> bool {
    (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6 && (a.z - b.z).abs() < 1e-6
}

/// Review wording stays a suggestion: nothing reads like a code check or an
/// approval, and no long dashes.
fn assert_calm(d: &Derived) {
    for i in &d.issues {
        let text = i.message.to_lowercase();
        for banned in [
            "code", "comply", "compliance", "permit", "violat", "illegal", "must", "required", "approved",
        ] {
            assert!(!text.contains(banned), "review wording sounds like compliance: {}", i.message);
        }
        assert!(!i.message.contains('\u{2014}') && !i.message.contains('\u{2013}'), "{}", i.message);
    }
}

fn object<'a>(project: &'a Project, id: &str) -> &'a Asset {
    project
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Asset(a) if a.id == id => Some(a),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no object {id}"))
}

fn add(e: Element) -> Command {
    Command::AddElement { element: e }
}

fn with(e: Element, f: impl FnOnce(&mut Asset)) -> Element {
    let mut e = e;
    if let Element::Asset(a) = &mut e {
        f(a);
    }
    e
}

fn set_layer(key: LayerKey, locked: bool) -> Command {
    Command::SetLayer {
        layer: Layer {
            key,
            visible: true,
            locked,
        },
    }
}

fn mark(target: ReviewTarget, note: Option<&str>) -> Command {
    Command::SetReviewMark {
        target,
        note: note.map(str::to_string),
    }
}

fn issue_target(id: &str) -> ReviewTarget {
    ReviewTarget::Issue { id: id.into() }
}

fn issue_of<'a>(state: &'a DocState, id: &str) -> &'a Issue {
    state
        .derived
        .issues
        .iter()
        .find(|i| i.id == id)
        .unwrap_or_else(|| panic!("no issue {id}"))
}

// --------------------------------------------------------------- schedule

#[test]
fn the_schedule_counts_devices_per_level_and_room() {
    let mut els = shell();
    els.push(wall_el("w-mid", (4000.0, 0.0), (4000.0, 4000.0)));
    // Two rooms now: the first face gets "Living", the second "Bed".
    els.retain(|e| e.kind() != ElementKind::Room);
    els.push(room_el("r-living", "Living", RoomUsage::Living, (2000.0, 2000.0)));
    els.push(room_el("r-bed", "Bed", RoomUsage::Bedroom, (5000.0, 2000.0)));
    els.extend([
        obj("l1", "light-ceiling", 2000.0, 2000.0, 0.0, &[]),
        obj("l2", "light-ceiling", 1000.0, 1000.0, 0.0, &[]),
        obj("l3", "light-ceiling", 5000.0, 2000.0, 0.0, &[]),
        obj("out", "light-outdoor", 1000.0, -150.0, 0.0, &[]),
        obj("bed", "bed-double", 5000.0, 1500.0, 0.0, &[]),
        obj("wc", "wc", 5000.0, 3500.0, 0.0, &[]),
        obj("tank", "water-tank", 8000.0, 2000.0, 0.0, &[]),
        // PH plumbing fixture tables list the kitchen sink and the washer;
        // other kitchen and appliance items stay out.
        obj("sink", "kitchen-sink", 2000.0, 3625.0, 0.0, &[]),
        obj("washer", "washing-machine", 3000.0, 4380.0, 180.0, &[]),
        obj("range", "range", 1000.0, 3625.0, 0.0, &[]),
        obj("counter", "kitchen-counter", 3000.0, 3625.0, 0.0, &[]),
        obj("fridge", "refrigerator", 3600.0, 3000.0, 0.0, &[]),
    ]);
    // Objects that are not in the catalog count by their own category.
    els.push(with(obj("lamp", "light-floor-lamp", 3000.0, 3000.0, 0.0, &[]), |a| {
        a.catalog_key = "my-lamp".into();
    }));
    els.push(with(obj("chair", "armchair", 3000.0, 1000.0, 0.0, &[]), |a| {
        a.catalog_key = "my-chair".into();
    }));
    let mut project = project_with(els);
    project.levels.push(Level {
        id: "level-2".into(),
        name: "Second Floor".into(),
        elevation_mm: 3000.0,
        height_mm: 3000.0,
    });
    project
        .elements
        .push(on_level(obj("up", "outlet-duplex", 1000.0, 1000.0, 0.0, &[]), "level-2"));
    let d = compute_derived(&project);

    type Row<'a> = (&'a str, Option<&'a str>, ScheduleGroup, &'a str, Option<DeviceKind>, u32);
    let rows: Vec<Row> = d
        .schedule
        .iter()
        .map(|r| {
            (
                r.level_id.as_str(),
                r.room_id.as_deref(),
                r.group,
                r.catalog_key.as_str(),
                r.device,
                r.count,
            )
        })
        .collect();
    use DeviceKind as D;
    use ScheduleGroup as G;
    assert_eq!(
        rows,
        vec![
            (LEVEL, Some("r-living"), G::Plumbing, "kitchen-sink", None, 1),
            (LEVEL, Some("r-living"), G::Electrical, "light-ceiling", Some(D::LightingOutlet), 2),
            (LEVEL, Some("r-living"), G::Electrical, "my-lamp", None, 1),
            (LEVEL, Some("r-bed"), G::Plumbing, "wc", None, 1),
            (LEVEL, Some("r-bed"), G::Electrical, "light-ceiling", Some(D::LightingOutlet), 1),
            (LEVEL, None, G::Plumbing, "washing-machine", None, 1),
            (LEVEL, None, G::Electrical, "light-outdoor", Some(D::LightingOutlet), 1),
            (LEVEL, None, G::Utility, "water-tank", None, 1),
            ("level-2", None, G::Electrical, "outlet-duplex", Some(D::ConvenienceReceptacle), 1),
        ]
    );
    for left_out in ["range", "kitchen-counter", "refrigerator", "bed-double", "my-chair"] {
        assert!(d.schedule.iter().all(|r| r.catalog_key != left_out), "{left_out} is counted");
    }

    // The query adds names, PH form rows and totals per level.
    let q = Document::new(project).query(&Query::Schedule).unwrap();
    assert_eq!(q["rows"][0]["room"], "Living");
    assert_eq!(q["rows"][0]["level"], "Ground Floor");
    assert_eq!(q["rows"][0]["group"], "plumbing");
    assert_eq!(q["rows"][0]["item"], "Kitchen sink counter");
    assert_eq!(q["rows"][0]["form_row"], Value::Null);
    assert_eq!(q["rows"][1]["item"], "Ceiling light");
    assert_eq!(q["rows"][1]["form_row"], "Lighting outlets");
    assert_eq!(q["rows"][2]["form_row"], Value::Null);
    assert_eq!(q["rows"][5]["room"], "Outside");
    assert_eq!(q["rows"][5]["item"], "Washing machine");
    assert_eq!(q["levels"].as_array().unwrap().len(), 2);
    assert_eq!(q["levels"][0]["total"], 9);
    assert_eq!(
        q["levels"][0]["by_group"],
        json!([
            {"group": "plumbing", "count": 3},
            {"group": "electrical", "count": 5},
            {"group": "utility", "count": 1},
        ])
    );
    assert_eq!(
        q["levels"][0]["by_form_row"],
        json!([{"form_row": "Lighting outlets", "count": 4}])
    );
    assert_eq!(q["levels"][1]["level"], "Second Floor");
    assert_eq!(
        q["levels"][1]["by_form_row"],
        json!([{"form_row": "Convenience receptacles", "count": 1}])
    );
    assert!(q["note"].as_str().unwrap().contains("Professional Electrical Engineer"));
}

// ----------------------------------------------------------- review marks

#[test]
fn a_finding_a_check_or_a_check_on_one_element_can_be_set_aside() {
    let mut els = shell();
    els.push(obj("la", "light-ceiling", 1500.0, 2000.0, 0.0, &[]));
    els.push(obj("lb", "light-ceiling", 4500.0, 2000.0, 0.0, &[]));
    let mut doc = doc_with(els);
    let before = doc.state();
    let codes: Vec<&str> = before.derived.issues.iter().map(|i| i.code.as_str()).collect();
    assert_eq!(codes, vec!["room_no_window", "room_no_door", "light_no_switch", "light_no_switch"]);
    assert!(before.derived.issues.iter().all(|i| i.status == IssueStatus::Open && i.note.is_empty()));

    // One finding. The note is trimmed; preview equals apply.
    let r = apply_checked(
        &mut doc,
        mark(issue_target("light_no_switch:la"), Some("  The switch is in the hall  ")),
    );
    assert_eq!(r.state.undo_label.as_deref(), Some("Set a review item aside"));
    assert_eq!(r.state.project.review.len(), 1);
    let la = issue_of(&r.state, "light_no_switch:la");
    assert_eq!((la.status, la.note.as_str()), (IssueStatus::Ignored, "The switch is in the hall"));
    assert_eq!(issue_of(&r.state, "light_no_switch:lb").status, IssueStatus::Open);
    assert!(r.diff.added.is_empty() && r.diff.modified.is_empty() && r.diff.removed.is_empty());
    // One undo step.
    assert!(doc.undo().unwrap().project.review.is_empty());
    assert_eq!(doc.redo().unwrap().project.review.len(), 1);

    // A check on one element, and a whole check. The most specific mark wins.
    apply_checked(
        &mut doc,
        mark(
            ReviewTarget::Element {
                code: "light_no_switch".into(),
                element_id: "lb".into(),
            },
            Some("Later"),
        ),
    );
    apply_checked(
        &mut doc,
        mark(
            ReviewTarget::Check {
                code: " light_no_switch ".into(),
            },
            Some("Switched from the hall"),
        ),
    );
    let r = apply_checked(
        &mut doc,
        mark(
            ReviewTarget::Check {
                code: "room_no_door".into(),
            },
            Some("The door comes later"),
        ),
    );
    let s = &r.state;
    assert_eq!(issue_of(s, "light_no_switch:la").note, "The switch is in the hall");
    assert_eq!(issue_of(s, "light_no_switch:lb").note, "Later");
    assert_eq!(issue_of(s, "room_no_door:room-1").note, "The door comes later");
    assert_eq!(issue_of(s, "room_no_window:room-1").status, IssueStatus::Open);
    // The stored target is trimmed.
    assert!(s.project.review.iter().any(|m| m.target
        == ReviewTarget::Check {
            code: "light_no_switch".into()
        }));

    // A new note for the same target replaces the mark where it is.
    let r = apply_checked(&mut doc, mark(issue_target("light_no_switch:la"), Some("Edited")));
    assert_eq!(r.state.undo_label.as_deref(), Some("Edit a review note"));
    assert_eq!(r.state.project.review.len(), 4);
    assert_eq!(r.state.project.review[0].note, "Edited");

    // Reopening one finding falls back to the check mark.
    let r = apply_checked(&mut doc, mark(issue_target("light_no_switch:la"), None));
    assert_eq!(r.state.undo_label.as_deref(), Some("Reopen a review item"));
    assert_eq!(r.state.project.review.len(), 3);
    assert_eq!(issue_of(&r.state, "light_no_switch:la").note, "Switched from the hall");

    // The query shows status and note.
    let q = doc.query(&Query::Issues).unwrap();
    assert_eq!(q["count"], 4);
    assert_eq!(q["ignored_count"], 3);
    assert_eq!(q["open_count"], 1);
    let first = q["items"].as_array().unwrap().iter().find(|i| i["id"] == "light_no_switch:lb").unwrap();
    assert_eq!(first["status"], "ignored");
    assert_eq!(first["note"], "Later");
    assert_eq!(doc.query(&Query::ProjectSummary).unwrap()["open_review_item_count"], 1);

    // A set-aside finding that the checks no longer produce is resolved.
    apply_checked(&mut doc, mark(issue_target("light_no_switch:la"), Some("Hall switch")));
    let r = apply_checked(&mut doc, Command::DeleteElements { ids: vec!["la".into()] });
    assert_eq!(
        r.state.derived.review_resolved,
        vec![ReviewMark {
            target: issue_target("light_no_switch:la"),
            note: "Hall switch".into(),
        }]
    );
    let q = doc.query(&Query::Issues).unwrap();
    assert_eq!(q["resolved"][0]["note"], "Hall switch");
    assert_eq!(q["resolved"][0]["target"]["kind"], "issue");
    // Marks for a whole check are never "resolved".
    assert_eq!(q["resolved"].as_array().unwrap().len(), 1);
    assert!(doc.undo().unwrap().derived.review_resolved.is_empty());
}

#[test]
fn review_marks_name_a_known_check() {
    let mut doc = doc_with(shell());
    let rejected = |doc: &mut Document, cmd: Command| code(&apply_rejected(doc, cmd));
    assert_eq!(
        rejected(&mut doc, mark(ReviewTarget::Check { code: "no_such_check".into() }, Some("x"))),
        "unknown_review_code"
    );
    assert_eq!(rejected(&mut doc, mark(issue_target("  "), Some("x"))), "bad_review_target");
    assert_eq!(rejected(&mut doc, mark(issue_target("nonsense:room-1"), Some("x"))), "unknown_review_code");
    assert_eq!(
        rejected(
            &mut doc,
            mark(
                ReviewTarget::Element {
                    code: "room_small".into(),
                    element_id: " ".into(),
                },
                Some("x"),
            )
        ),
        "bad_review_target"
    );
    assert_eq!(
        rejected(&mut doc, mark(ReviewTarget::Check { code: "door_narrow".into() }, None)),
        "no_review_mark"
    );
    // Every check the engine runs can be set aside, once each.
    let mut sorted: Vec<&str> = REVIEW_CODES.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), REVIEW_CODES.len(), "a review code is listed twice");
    for code in REVIEW_CODES {
        apply_checked(&mut doc, mark(ReviewTarget::Check { code: code.to_string() }, Some("Not now")));
    }
    assert_eq!(doc.project().review.len(), REVIEW_CODES.len());
}

// ------------------------------------------------- links, light, circuits

#[test]
fn light_links_and_circuit_tags_are_checked() {
    let mut doc = doc_with({
        let mut els = shell();
        els.push(obj("la", "light-ceiling", 2000.0, 2000.0, 0.0, &[]));
        els
    });
    let light = |f: fn(&mut AssetLight)| {
        with(obj("", "light-ceiling", 1000.0, 1000.0, 0.0, &[]), move |a| {
            let mut l = a.light.unwrap();
            f(&mut l);
            a.light = Some(l);
        })
    };
    assert_eq!(code(&apply_rejected(&mut doc, add(light(|l| l.lumens = 25_000.0)))), "asset_light");
    assert_eq!(code(&apply_rejected(&mut doc, add(light(|l| l.lumens = -1.0)))), "asset_light");
    assert_eq!(code(&apply_rejected(&mut doc, add(light(|l| l.kelvin = 1000.0)))), "asset_light");
    assert_eq!(code(&apply_rejected(&mut doc, add(light(|l| l.kelvin = 12_000.0)))), "asset_light");
    assert_eq!(code(&apply_rejected(&mut doc, add(light(|l| l.lumens = f64::NAN)))), "not_finite");
    apply_checked(&mut doc, add(light(|l| l.lumens = 20_000.0)));

    let switch = |links: &[&str]| obj("sw", "switch-1", 2150.0, 95.0, 180.0, links);
    assert_eq!(code(&apply_rejected(&mut doc, add(switch(&["sw"])))), "link_to_itself");
    assert_eq!(code(&apply_rejected(&mut doc, add(switch(&["nope"])))), "bad_link");
    let e = apply_rejected(&mut doc, add(switch(&["w-south"])));
    assert_eq!((code(&e).as_str(), error_ids(&e)), ("bad_link", vec!["sw".to_string(), "w-south".to_string()]));
    assert_eq!(code(&apply_rejected(&mut doc, add(switch(&["la", "la"])))), "duplicate_link");
    let long = with(switch(&["la"]), |a| a.circuit = "L1234567890123456".into());
    assert_eq!(code(&apply_rejected(&mut doc, add(long))), "circuit_too_long");

    // A circuit tag is trimmed, and 16 characters fit.
    let r = apply_checked(&mut doc, add(with(switch(&["la"]), |a| a.circuit = "  L1  ".into())));
    assert_eq!(object(&r.state.project, "sw").circuit, "L1");
    assert_eq!(r.state.undo_label.as_deref(), Some("Add switch"));
    assert_eq!(r.diff.summary, "Add switch: added 1 switch");
    let mut sw = object(doc.project(), "sw").clone();
    sw.circuit = " L123456789012345 ".into();
    let r = apply_checked(&mut doc, Command::UpdateElement { element: Element::Asset(sw.clone()) });
    assert_eq!(object(&r.state.project, "sw").circuit, "L123456789012345");
    // Editing links goes through the same checks.
    sw.links = vec!["nope".into()];
    assert_eq!(
        code(&apply_rejected(&mut doc, Command::UpdateElement { element: Element::Asset(sw) })),
        "bad_link"
    );
}

#[test]
fn deleting_an_object_removes_it_from_every_link_in_one_step() {
    let mut els = shell();
    els.push(obj("la", "light-ceiling", 1500.0, 2000.0, 0.0, &[]));
    els.push(obj("lb", "light-ceiling", 4500.0, 2000.0, 0.0, &[]));
    els.push(obj("sw", "switch-2", 2150.0, 95.0, 180.0, &["la", "lb"]));
    let mut doc = doc_with(els);
    let r = apply_checked(&mut doc, Command::DeleteElements { ids: vec!["la".into()] });
    assert_eq!(object(&r.state.project, "sw").links, vec!["lb".to_string()]);
    assert_eq!(r.diff.removed, vec!["la".to_string()]);
    assert_eq!(r.diff.modified, vec!["sw".to_string()]);
    assert_eq!(r.diff.summary, "Delete: changed 1 switch; removed 1 light");
    let back = doc.undo().unwrap();
    assert_eq!(object(&back.project, "sw").links, vec!["la".to_string(), "lb".to_string()]);

    // An aircon outlet on a locked electrical layer still loses the link to
    // a unit deleted from the unlocked aircon layer: a link to nothing says
    // nothing. The outlet itself stays locked.
    let mut els = shell();
    els.push(obj("iu", "aircon-indoor-1hp", 5810.0, 2000.0, 270.0, &[]));
    els.push(obj("aco", "outlet-aircon", 5905.0, 2550.0, 270.0, &["iu"]));
    let mut doc = doc_with(els);
    apply_checked(&mut doc, set_layer(LayerKey::Electrical, true));
    let r = apply_checked(&mut doc, Command::DeleteElements { ids: vec!["iu".into()] });
    assert!(object(&r.state.project, "aco").links.is_empty());
    assert_eq!(r.diff.summary, "Delete: changed 1 outlet; removed 1 aircon unit");
    let e = apply_rejected(&mut doc, Command::DeleteElements { ids: vec!["aco".into()] });
    assert_eq!(code(&e), "layer_locked");
}

#[test]
fn duplicating_objects_remaps_the_links_inside_the_copy() {
    let mut els = shell();
    els.push(obj("la", "light-ceiling", 1500.0, 2000.0, 0.0, &[]));
    els.push(obj("lb", "light-ceiling", 4500.0, 2000.0, 0.0, &[]));
    els.push(obj("sw", "switch-2", 2150.0, 95.0, 180.0, &["la", "lb"]));
    let mut doc = doc_with(els);
    let r = apply_checked(
        &mut doc,
        Command::DuplicateElements {
            ids: vec!["sw".into(), "la".into()],
            delta: p(0.0, 500.0),
        },
    );
    assert_eq!(r.diff.added.len(), 2);
    let new_light = r
        .diff
        .added
        .iter()
        .find(|id| object(&r.state.project, id).catalog_key == "light-ceiling")
        .unwrap();
    let new_switch = r
        .diff
        .added
        .iter()
        .find(|id| object(&r.state.project, id).catalog_key == "switch-2")
        .unwrap();
    // Inside the copy: remapped. Outside it: kept.
    assert_eq!(object(&r.state.project, new_switch).links, vec![new_light.clone(), "lb".to_string()]);
    assert_eq!(object(&r.state.project, "sw").links, vec!["la".to_string(), "lb".to_string()]);

    // A switch copied alone keeps its lights: two switches on one light.
    let r = apply_checked(
        &mut doc,
        Command::DuplicateElements {
            ids: vec!["sw".into()],
            delta: p(500.0, 0.0),
        },
    );
    let copy = &r.diff.added[0];
    assert_eq!(object(&r.state.project, copy).links, vec!["la".to_string(), "lb".to_string()]);
    assert!(items(&r.state.derived, "light_no_switch").is_empty());
}

#[test]
fn electrical_and_aircon_objects_follow_their_layer_lock() {
    let mut els = shell();
    els.push(obj("la", "light-ceiling", 1500.0, 2000.0, 0.0, &[]));
    els.push(obj("bed", "bed-double", 3000.0, 1100.0, 180.0, &[]));
    let mut doc = doc_with(els);
    apply_checked(&mut doc, set_layer(LayerKey::Electrical, true));
    let e = apply_rejected(&mut doc, add(obj("", "switch-1", 2150.0, 95.0, 180.0, &[])));
    assert_eq!(code(&e), "layer_locked");
    assert!(e.to_string().contains("Electrical"), "{e}");
    let e = apply_rejected(
        &mut doc,
        Command::MoveElements {
            ids: vec!["la".into()],
            delta: p(100.0, 0.0),
            stretch_connected: false,
        },
    );
    assert_eq!((code(&e).as_str(), error_ids(&e)), ("layer_locked", vec!["la".to_string()]));
    // Furniture stays on the objects layer.
    apply_checked(&mut doc, add(obj("", "bed-single", 1000.0, 3000.0, 0.0, &[])));
    let mut bed = object(doc.project(), "bed").clone();
    bed.category = AssetCategory::Lighting;
    let e = apply_rejected(&mut doc, Command::UpdateElement { element: Element::Asset(bed) });
    assert_eq!(code(&e), "layer_locked");

    apply_checked(&mut doc, set_layer(LayerKey::Aircon, true));
    let e = apply_rejected(&mut doc, add(obj("", "aircon-indoor-1hp", 5810.0, 2000.0, 270.0, &[])));
    assert!(e.to_string().contains("Aircon"), "{e}");
    apply_checked(&mut doc, set_layer(LayerKey::Electrical, false));
    apply_checked(&mut doc, add(obj("", "switch-1", 2150.0, 95.0, 180.0, &["la"])));
}

#[test]
fn describe_shows_device_data() {
    let mut els = shell();
    els.push(obj("la", "light-ceiling", 2000.0, 2000.0, 0.0, &[]));
    els.push(with(obj("sw", "switch-1", 2150.0, 95.0, 180.0, &["la"]), |a| a.circuit = "L1".into()));
    els.push(obj("iu", "aircon-indoor-2hp", 5810.0, 2000.0, 270.0, &[]));
    let doc = doc_with(els);
    let q = doc
        .query(&Query::Describe {
            ids: vec!["sw".into(), "la".into(), "iu".into()],
        })
        .unwrap();
    let sw = &q["elements"][0];
    assert_eq!(sw["device"], "switch");
    assert_eq!(sw["mount"], "wall");
    assert_eq!(sw["layer"], "electrical");
    assert_eq!(sw["elevation_mm"], 1143.0);
    assert_eq!(sw["links"], json!([{"id": "la", "name": "Ceiling light"}]));
    assert_eq!(sw["circuit"], "L1");
    assert_eq!(sw["in_room"]["name"], "Bedroom");
    let la = &q["elements"][1];
    assert_eq!(la["linked_by"], json!([{"id": "sw", "name": "Switch, one gang"}]));
    assert_eq!(la["light"], json!({"lumens": 900.0, "kelvin": 3000.0, "on": true}));
    assert_eq!(la["mount"], "ceiling");
    let iu = &q["elements"][2];
    assert_eq!(iu["layer"], "aircon");
    assert_eq!(iu["aircon"]["role"], "indoor");
    assert_eq!(iu["aircon"]["gas_line_mm"], 12.7);
    assert_eq!(iu["aircon"]["max_line_m"], 30.0);
}

// ----------------------------------------------------------- service runs

#[test]
fn conduit_needs_no_sleeves() {
    let mut els = shell();
    // Through the north wall above the ceiling line, and down through the slab.
    els.push(pipe_el("c1", PipeSystem::Conduit, 20.0, &[(3000.0, 2000.0, 2800.0), (3000.0, 5000.0, 2800.0)]));
    els.push(pipe_el("c2", PipeSystem::Conduit, 25.0, &[(1000.0, 1000.0, 300.0), (1000.0, 1000.0, -300.0)]));
    let d = derive(els.clone());
    assert!(d.pipes.penetrations.is_empty());
    assert_eq!(d.pipes.sleeve_count, 0);
    assert!(items(&d, "pipe_penetrations").is_empty());
    let conduit: Vec<(PipeMaterial, f64, u32)> = d
        .pipes
        .takeoff
        .iter()
        .filter(|r| r.system == PipeSystem::Conduit)
        .map(|r| (r.material, r.diameter_mm, r.run_count))
        .collect();
    assert_eq!(conduit, vec![(PipeMaterial::Pvc, 20.0, 1), (PipeMaterial::Pvc, 25.0, 1)]);
    // A water pipe on the same line does need one.
    els.push(pipe_el("cw", PipeSystem::ColdWater, 20.0, &[(2000.0, 2000.0, 2800.0), (2000.0, 5000.0, 2800.0)]));
    let d = derive(els);
    assert_eq!(d.pipes.sleeve_count, 1);
    assert_eq!(d.pipes.penetrations[0].pipe_id, "cw");
}

#[test]
fn aircon_runs_through_a_wall_are_core_holes() {
    let mut els = shell();
    // A 9.52 mm line set and its condensate drain through the east wall,
    // 100 mm apart: one 65 mm hole.
    els.push(pipe_el(
        "ls",
        PipeSystem::Refrigerant,
        9.52,
        &[(5880.0, 2000.0, 2400.0), (6300.0, 2000.0, 2393.0)],
    ));
    els.push(pipe_el(
        "cd",
        PipeSystem::Condensate,
        20.0,
        &[(5880.0, 2100.0, 2310.0), (6300.0, 2100.0, 2280.0)],
    ));
    let d = derive(els.clone());
    assert_eq!(d.pipes.sleeve_count, 2);
    assert!(d.pipes.penetrations.iter().all(|p| p.kind == PenetrationKind::Wall));
    let summary = items(&d, "pipe_penetrations");
    assert_eq!(
        summary[0].message,
        "1 aircon core hole goes through a wall: 65 mm, sloped 5 to 7 mm down to the outside. Drill it before the wall is finished."
    );
    assert_eq!(summary[0].element_ids, vec!["cd".to_string(), "ls".to_string()]);
    let q = Document::new(project_with(els.clone())).query(&Query::PipeTakeoff).unwrap();
    for pen in q["penetrations"].as_array().unwrap() {
        assert_eq!(pen["core_hole_mm"], 65.0);
    }

    // A gas line from 16 mm needs a 90 mm hole.
    els.push(pipe_el(
        "big",
        PipeSystem::Refrigerant,
        15.88,
        &[(100.0, 1000.0, 2400.0), (-300.0, 1000.0, 2395.0)],
    ));
    let d = derive(els.clone());
    assert_eq!(
        items(&d, "pipe_penetrations")[0].message,
        "2 aircon core holes go through walls: 65 and 90 mm, each sloped 5 to 7 mm down to the outside. Drill them before the walls are finished."
    );

    // Sleeves and core holes in one summary; a condensate drain on its own
    // has its own hole.
    els.push(pipe_el("cw", PipeSystem::ColdWater, 20.0, &[(3000.0, 3000.0, 300.0), (3000.0, 3000.0, -300.0)]));
    els.push(pipe_el(
        "lone",
        PipeSystem::Condensate,
        20.0,
        &[(3000.0, 100.0, 2300.0), (3000.0, -300.0, 2290.0)],
    ));
    let d = derive(els);
    assert_eq!(d.pipes.sleeve_count, 5);
    assert_eq!(
        items(&d, "pipe_penetrations")[0].message,
        "1 pipe penetration needs a sleeve: 1 through the floor slab. Set the sleeves before the pour or the blockwork. 3 aircon core holes go through walls: 65 and 90 mm, each sloped 5 to 7 mm down to the outside. Drill them before the walls are finished."
    );
}

#[test]
fn storm_and_condensate_runs_get_the_fall_check() {
    let d = derive(vec![
        // 10 mm over 2 m is 0.5 percent, under the 1 percent default for 100 mm.
        pipe_el("st", PipeSystem::Storm, 100.0, &[(7000.0, 1000.0, -300.0), (7000.0, 3000.0, -310.0)]),
        pipe_el("level", PipeSystem::Condensate, 20.0, &[(7000.0, 5000.0, 2300.0), (8000.0, 5000.0, 2300.0)]),
        pipe_el("up", PipeSystem::Condensate, 20.0, &[(7000.0, 6000.0, 2300.0), (8000.0, 6000.0, 2350.0)]),
        pipe_el("low", PipeSystem::Condensate, 20.0, &[(7000.0, 9000.0, 2300.0), (8000.0, 9000.0, 2290.0)]),
        // A drop and a short stub are not runs.
        pipe_el("drop", PipeSystem::Condensate, 20.0, &[(7000.0, 7000.0, 2300.0), (7000.0, 7000.0, 100.0)]),
        pipe_el("stub", PipeSystem::Condensate, 20.0, &[(7000.0, 8000.0, 2300.0), (7200.0, 8000.0, 2300.0)]),
        // Good fall.
        pipe_el("ok", PipeSystem::Storm, 100.0, &[(9000.0, 1000.0, -300.0), (9000.0, 3000.0, -330.0)]),
    ]);
    let storm = items(&d, "drain_slope_low");
    assert_eq!(storm.len(), 1);
    assert_eq!(storm[0].element_ids, vec!["st".to_string()]);
    assert_eq!(
        storm[0].message,
        "Storm drain pipe 100 mm falls 0.5 percent. The 2.00 m run drops 10 mm, under the 1 percent default for 100 mm storm drainage. Give it more fall: start it higher or connect it lower."
    );
    assert!(near(storm[0].location.unwrap(), v(7000.0, 2000.0, -305.0)));
    let cond = items(&d, "condensate_slope_low");
    assert_eq!(
        cond.iter().map(|i| i.element_ids[0].as_str()).collect::<Vec<_>>(),
        vec!["level", "up", "low"]
    );
    assert_eq!(cond[0].severity, Severity::Warning);
    assert_eq!(
        cond[0].message,
        "Condensate drain pipe 20 mm has no fall. The 1.00 m run is level. Guhit checks condensate against the 2 percent drain default, since aircon manuals give no number. Give it fall toward the outlet."
    );
    assert_eq!(
        cond[1].message,
        "Condensate drain pipe 20 mm runs uphill. The 1.00 m run rises 50 mm from its first point to its last, so water would run back toward the unit. Lower its far end, or reverse the run if it was drawn from the outlet."
    );
    assert_eq!(
        cond[2].message,
        "Condensate drain pipe 20 mm falls 1.0 percent. The 1.00 m run drops 10 mm, under the 2 percent drain default Guhit uses for condensate. Give it more fall toward the outlet, so water does not back up into the unit."
    );
}

#[test]
fn runs_join_only_within_one_system() {
    let d = derive(vec![
        pipe_el("c1", PipeSystem::Conduit, 20.0, &[(0.0, 0.0, 2800.0), (4000.0, 0.0, 2800.0)]),
        // A conduit ending on another conduit is a tee.
        pipe_el("c2", PipeSystem::Conduit, 20.0, &[(2000.0, 2000.0, 2800.0), (2000.0, 0.0, 2800.0)]),
        // A water pipe ending on the conduit is a clash, never a fitting.
        pipe_el("cw", PipeSystem::ColdWater, 20.0, &[(1000.0, 2000.0, 2800.0), (1000.0, 0.0, 2800.0)]),
        // Condensate ending on a line set: a clash too.
        pipe_el("k", PipeSystem::Condensate, 20.0, &[(0.0, 5000.0, 2300.0), (4000.0, 5000.0, 2300.0)]),
        pipe_el("r", PipeSystem::Refrigerant, 9.52, &[(2000.0, 7000.0, 2300.0), (2000.0, 5000.0, 2300.0)]),
    ]);
    let tees: Vec<(&str, Option<&str>)> = d
        .pipes
        .fittings
        .iter()
        .filter(|f| f.kind == FittingKind::Tee)
        .map(|f| (f.pipe_id.as_str(), f.branch_pipe_id.as_deref()))
        .collect();
    assert_eq!(tees, vec![("c1", Some("c2"))]);
    let clashes: Vec<Vec<Id>> = items(&d, "pipes_cross").iter().map(|i| i.element_ids.clone()).collect();
    assert_eq!(
        clashes,
        vec![vec!["c1".to_string(), "cw".to_string()], vec!["k".to_string(), "r".to_string()]]
    );
    assert!(
        items(&d, "pipes_cross")[1].message.ends_with("Move one of them so the lines clear each other."),
        "{}",
        items(&d, "pipes_cross")[1].message
    );
}

#[test]
fn a_condensate_drain_ends_at_a_drain_or_outside() {
    let inside = pipe_el(
        "cd",
        PipeSystem::Condensate,
        20.0,
        &[(3000.0, 3800.0, 2300.0), (3000.0, 2000.0, 2200.0)],
    );
    let mut els = shell();
    els.push(inside.clone());
    let d = derive(els.clone());
    let open = items(&d, "condensate_open_end");
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].severity, Severity::Info);
    assert_eq!(
        open[0].message,
        "Condensate drain pipe 20 mm ends inside the house, away from a drain. Lead it to a floor drain, a drain pipe or the outside, so the water has somewhere to go."
    );
    assert!(near(open[0].location.unwrap(), v(3000.0, 2000.0, 2200.0)));

    // A floor drain next to the end.
    let mut drained = els.clone();
    drained.push(obj("fd", "floor-drain", 3100.0, 2000.0, 0.0, &[]));
    assert!(items(&derive(drained), "condensate_open_end").is_empty());
    // A drain pipe next to the end.
    let mut piped = els.clone();
    piped.push(pipe_el("d", PipeSystem::Drainage, 50.0, &[(2000.0, 2100.0, 2100.0), (4000.0, 2100.0, 2080.0)]));
    assert!(items(&derive(piped), "condensate_open_end").is_empty());
    // Ending outside the house.
    let mut outside = shell();
    outside.push(pipe_el(
        "cd",
        PipeSystem::Condensate,
        20.0,
        &[(3000.0, 3800.0, 2300.0), (3000.0, 4500.0, 2200.0)],
    ));
    assert!(items(&derive(outside), "condensate_open_end").is_empty());
}

// ------------------------------------------------------ lights, switches

#[test]
fn a_light_needs_a_switch_and_a_switch_needs_a_load() {
    let mut els = shell();
    els.push(obj("la", "light-ceiling", 2000.0, 2000.0, 0.0, &[]));
    els.push(obj("wl", "light-wall", 135.0, 2000.0, 90.0, &[]));
    // A plug-in lamp has its own switch.
    els.push(obj("lamp", "light-floor-lamp", 1000.0, 1000.0, 0.0, &[]));
    els.push(obj("sw", "switch-1", 2150.0, 95.0, 180.0, &[]));
    let d = derive(els.clone());
    let lights = items(&d, "light_no_switch");
    assert_eq!(lights.len(), 2);
    assert_eq!(lights[0].severity, Severity::Info);
    assert_eq!(lights[0].message, "Bedroom ceiling light has no switch. Link a switch with the link tool (L).");
    assert_eq!(lights[1].message, "Bedroom wall light has no switch. Link a switch with the link tool (L).");
    assert!(near(lights[0].location.unwrap(), v(2000.0, 2000.0, 2970.0)));
    let loose = items(&d, "switch_no_load");
    assert_eq!(loose.len(), 1);
    assert_eq!(
        loose[0].message,
        "Bedroom switch controls nothing yet. Link it to its lights with the link tool (L), or remove it."
    );
    assert!(near(loose[0].location.unwrap(), v(2150.0, 95.0, 1200.5)));

    // Linked, and a second switch on the ceiling light (a 3-way).
    let mut linked = shell();
    linked.push(obj("la", "light-ceiling", 2000.0, 2000.0, 0.0, &[]));
    linked.push(obj("wl", "light-wall", 135.0, 2000.0, 90.0, &[]));
    linked.push(obj("sw", "switch-2", 2150.0, 95.0, 180.0, &["la", "wl"]));
    linked.push(obj("sw3", "switch-1", 5905.0, 2000.0, 270.0, &["la"]));
    let d = derive(linked);
    assert!(items(&d, "light_no_switch").is_empty());
    assert!(items(&d, "switch_no_load").is_empty());

    // A user's own name is used as written.
    let mut named = shell();
    named.push(with(obj("pd", "light-pendant", 3000.0, 2000.0, 0.0, &[]), |a| a.name = "Pendant over the table".into()));
    let d = derive(named);
    assert_eq!(
        items(&d, "light_no_switch")[0].message,
        "Pendant over the table has no switch. Link a switch with the link tool (L)."
    );
}

fn door(id: &str, wall: &str, offset: f64, width: f64, style: OpeningStyle, flip_hinge: bool) -> Element {
    Element::Opening(Opening {
        id: id.into(),
        wall_id: wall.into(),
        opening_type: OpeningType::Door,
        style,
        offset_mm: offset,
        width_mm: width,
        height_mm: 2100.0,
        sill_mm: 0.0,
        flip_side: false,
        flip_hinge,
        material_id: None,
    })
}

#[test]
fn a_switch_behind_a_door_on_its_hinge_side() {
    // The door swings into the room from the south wall, hinged at x = 1050,
    // latching at x = 1950.
    let base = || {
        let mut els = shell();
        els.push(door("d1", "w-south", 1500.0, 900.0, OpeningStyle::SwingSingle, false));
        els.push(obj("la", "light-ceiling", 2000.0, 2000.0, 0.0, &[]));
        els
    };
    let with_switch = |x: f64, y: f64, rotation: f64| {
        let mut els = base();
        els.push(obj("s", "switch-1", x, y, rotation, &["la"]));
        derive(els)
    };
    let d = with_switch(800.0, 95.0, 180.0);
    let found = items(&d, "switch_behind_door");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Warning);
    assert_eq!(found[0].element_ids, vec!["s".to_string(), "d1".to_string()]);
    assert_eq!(
        found[0].message,
        "Bedroom switch is behind the Bedroom door when the door is open. Move it to the latch side, about 200 mm from the frame."
    );
    assert!(near(found[0].location.unwrap(), v(800.0, 95.0, 1200.5)));
    // The latch side, the other face of the wall, and past the leaf are clear.
    for (x, y, r) in [(2150.0, 95.0, 180.0), (800.0, -95.0, 0.0), (100.0, 95.0, 180.0)] {
        assert!(items(&with_switch(x, y, r), "switch_behind_door").is_empty(), "{x} {y}");
    }

    // Hinged on the other jamb, the same latch-side switch is behind it.
    let mut els = shell();
    els.push(door("d1", "w-south", 1500.0, 900.0, OpeningStyle::SwingSingle, true));
    els.push(obj("la", "light-ceiling", 2000.0, 2000.0, 0.0, &[]));
    els.push(obj("s", "switch-1", 2150.0, 95.0, 180.0, &["la"]));
    assert_eq!(items(&derive(els), "switch_behind_door").len(), 1);

    // A double door has a hinge at each jamb. On the north wall, running
    // west, it swings south into the room.
    let mut els = shell();
    els.push(door("d2", "w-north", 3000.0, 1200.0, OpeningStyle::SwingDouble, false));
    els.push(obj("la", "light-ceiling", 2000.0, 2000.0, 0.0, &[]));
    els.push(obj("se", "switch-1", 3800.0, 3905.0, 0.0, &["la"]));
    els.push(obj("sw", "switch-1", 2200.0, 3905.0, 0.0, &["la"]));
    els.push(obj("far", "switch-1", 4500.0, 3905.0, 0.0, &["la"]));
    let d = derive(els);
    let ids: Vec<&str> = items(&d, "switch_behind_door").iter().map(|i| i.element_ids[0].as_str()).collect();
    assert_eq!(ids, vec!["se", "sw"]);
}

// ------------------------------------------------------------------ aircon

/// The indoor unit on the east wall: 800 mm along y from 1600 to 2400,
/// back on the inner face, underside at 2300.
fn indoor() -> Element {
    obj("iu", "aircon-indoor-1hp", 5810.0, 2000.0, 270.0, &[])
}

#[test]
fn an_aircon_unit_needs_an_outlet() {
    let mut els = shell();
    els.push(indoor());
    let d = derive(els.clone());
    let found = items(&d, "aircon_no_outlet");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Warning);
    assert_eq!(
        found[0].message,
        "Bedroom aircon indoor unit has no outlet linked to it. Add an aircon outlet near it and link them with the link tool (L)."
    );
    assert!(near(found[0].location.unwrap(), v(5810.0, 2000.0, 2447.5)));
    let mut fed = els.clone();
    fed.push(obj("aco", "outlet-aircon", 5905.0, 2550.0, 270.0, &["iu"]));
    assert!(items(&derive(fed), "aircon_no_outlet").is_empty());
    // An ordinary outlet does not feed a unit.
    let mut duplex = els.clone();
    duplex.push(obj("o", "outlet-duplex", 5905.0, 2550.0, 270.0, &["iu"]));
    assert_eq!(items(&derive(duplex), "aircon_no_outlet").len(), 1);

    // A window unit, and one fed through a special purpose outlet.
    let mut window = shell();
    window.push(obj("wu", "aircon-window", 3000.0, 0.0, 0.0, &[]));
    assert_eq!(items(&derive(window.clone()), "aircon_no_outlet").len(), 1);
    window.push(obj("spo", "outlet-spo", 3500.0, 95.0, 180.0, &["wu"]));
    assert!(items(&derive(window), "aircon_no_outlet").is_empty());

    // An outlet at the outdoor unit feeds the pair its line set joins.
    let mut split = shell();
    split.push(indoor());
    split.push(obj("ou", "aircon-outdoor-1hp", 6547.5, 2000.0, 90.0, &[]));
    split.push(obj("ospo", "outlet-aircon", 6200.0, 3000.0, 0.0, &["ou"]));
    split.push(pipe_el(
        "ls",
        PipeSystem::Refrigerant,
        9.52,
        &[(5880.0, 2000.0, 2400.0), (6300.0, 2000.0, 2393.0), (6300.0, 2000.0, 300.0), (6380.0, 2000.0, 300.0)],
    ));
    assert!(items(&derive(split), "aircon_no_outlet").is_empty());
}

fn line_set(points: &[(f64, f64, f64)]) -> Derived {
    let mut els = shell();
    els.push(indoor());
    els.push(obj("aco", "outlet-aircon", 5905.0, 2550.0, 270.0, &["iu"]));
    els.push(pipe_el("ls", PipeSystem::Refrigerant, 9.52, points));
    derive(els)
}

#[test]
fn a_line_set_goes_against_the_limits_of_its_indoor_unit() {
    // 0.42 m through the wall, 2.10 m down, then 24.48 m away: 27 m.
    let d = line_set(&[
        (5880.0, 2000.0, 2400.0),
        (6300.0, 2000.0, 2400.0),
        (6300.0, 2000.0, 300.0),
        (6300.0, 26_480.0, 300.0),
    ]);
    let long = items(&d, "lineset_long");
    assert_eq!(long.len(), 1);
    assert_eq!(long[0].severity, Severity::Warning);
    assert_eq!(long[0].element_ids, vec!["ls".to_string(), "iu".to_string()]);
    assert_eq!(
        long[0].message,
        "Line set 9.52 mm is 27.00 m long, over the 25 m the Bedroom aircon indoor unit allows. Move the outdoor unit closer, or check the unit's own manual."
    );
    assert!(near(long[0].location.unwrap(), v(5880.0, 2000.0, 2400.0)));
    assert!(items(&d, "lineset_rise").is_empty() && items(&d, "lineset_short").is_empty());
    assert_eq!(
        items(&d, "lineset_extra")[0].message,
        "Line set 9.52 mm is 27.00 m long, 24.00 m more than the 3 m a standard installation includes. Installers usually charge for each extra meter."
    );
    assert!(items(&d, "lineset_extra")[0].location.is_none());

    // 12.10 m up to a roof deck: over the 10 m height difference.
    let d = line_set(&[(5880.0, 2000.0, 2400.0), (6300.0, 2000.0, 2400.0), (6300.0, 2000.0, 14_500.0)]);
    let rise = items(&d, "lineset_rise");
    assert_eq!(
        rise[0].message,
        "Line set 9.52 mm has its ends 12.10 m apart in height, over the 10 m the Bedroom aircon indoor unit allows. Bring the outdoor unit nearer the indoor unit's level, or check the unit's own manual."
    );
    assert!(items(&d, "lineset_long").is_empty());

    // Straight through the wall: under the 3 m minimum.
    let d = line_set(&[(5880.0, 2000.0, 2400.0), (6300.0, 2000.0, 2400.0)]);
    let short = items(&d, "lineset_short");
    assert_eq!(short[0].severity, Severity::Info);
    assert_eq!(
        short[0].message,
        "Line set 9.52 mm is 0.42 m long, under the 3 m minimum in the manual of the Bedroom aircon indoor unit, which keeps vibration and noise down. Move the outdoor unit a little farther, or add a loop to the line set."
    );
    assert!(items(&d, "lineset_extra").is_empty());

    // 5 m: only the extra meters.
    let d = line_set(&[
        (5880.0, 2000.0, 2400.0),
        (6300.0, 2000.0, 2400.0),
        (6300.0, 2000.0, 300.0),
        (6300.0, 4480.0, 300.0),
    ]);
    let codes: Vec<&str> = d
        .issues
        .iter()
        .filter(|i| i.code.starts_with("lineset"))
        .map(|i| i.code.as_str())
        .collect();
    assert_eq!(codes, vec!["lineset_extra"]);
    assert_eq!(
        items(&d, "lineset_extra")[0].message,
        "Line set 9.52 mm is 5.00 m long, 2.00 m more than the 3 m a standard installation includes. Installers usually charge for each extra meter."
    );

    // Ends away from every indoor unit: no unit limits, the 3 m default.
    let d = derive(vec![pipe_el(
        "far",
        PipeSystem::Refrigerant,
        12.7,
        &[(1000.0, 1000.0, 2400.0), (1000.0, 5000.0, 2400.0)],
    )]);
    let codes: Vec<&str> = d.issues.iter().map(|i| i.code.as_str()).collect();
    assert_eq!(codes, vec!["lineset_extra"]);
    assert!(d.issues[0].message.starts_with("Line set 12.7 mm is 4.00 m long, 1.00 m more"));
}

#[test]
fn indoor_unit_clearances_follow_the_manual() {
    let base = || {
        let mut els = shell();
        els.push(obj("aco", "outlet-aircon", 5905.0, 2550.0, 270.0, &["iu"]));
        els
    };
    let clear = |mut els: Vec<Element>, unit: Element| {
        els.push(unit);
        derive(els)
    };
    // On its wall, 405 mm under the ceiling: nothing, even with a wardrobe
    // right beside it below its underside.
    let mut els = base();
    els.push(obj("wr", "wardrobe", 5625.0, 3000.0, 270.0, &[]));
    assert!(items(&clear(els, indoor()), "indoor_unit_clearance").is_empty());

    // A column 50 mm off its left side (north, facing the unit).
    let mut els = base();
    els.push(Element::Column(Column {
        id: "col".into(),
        level_id: LEVEL.into(),
        center: p(5800.0, 2550.0),
        shape: ColumnShape::Rect,
        width_mm: 200.0,
        depth_mm: 200.0,
        rotation_deg: 0.0,
        material_id: None,
    }));
    let d = clear(els.clone(), indoor());
    let found = items(&d, "indoor_unit_clearance");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Warning);
    assert_eq!(
        found[0].message,
        "Bedroom aircon indoor unit has too little clearance: 50 mm free at its left side. Its manual asks for 150 mm above, 120 mm at each side and the underside at 2.30 m or higher, for air flow and service."
    );
    // Low as well: both in one item.
    let d = clear(els, raised(indoor(), 2000.0));
    assert!(items(&d, "indoor_unit_clearance")[0]
        .message
        .contains("50 mm free at its left side and its underside is 2.00 m above the floor."));

    // A 2650 mm level leaves 55 mm above the unit.
    let mut project = project_with({
        let mut els = base();
        els.push(indoor());
        els
    });
    project.levels[0].height_mm = 2650.0;
    let d = compute_derived(&project);
    assert!(items(&d, "indoor_unit_clearance")[0]
        .message
        .contains("clearance: 55 mm free above it."));
}

#[test]
fn outdoor_unit_clearances_follow_the_manual() {
    // Outside the east wall, facing east. 150 mm off the wall: too close.
    let outdoor = |x: f64| obj("ou", "aircon-outdoor-1hp", x, 2000.0, 90.0, &[]);
    let mut els = shell();
    els.push(outdoor(6377.5));
    let d = derive(els);
    let found = items(&d, "outdoor_unit_clearance");
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].message,
        "Aircon outdoor unit has too little free space around it: a wall 150 mm behind it. Its manual asks for 2000 mm in front, 300 mm behind, 300 mm on the left and 600 mm on the right, for air flow and service."
    );
    assert!(near(found[0].location.unwrap(), v(6377.5, 2000.0, 277.5)));

    // 320 mm off the wall is fine.
    let mut els = shell();
    els.push(outdoor(6547.5));
    assert!(items(&derive(els.clone()), "outdoor_unit_clearance").is_empty());

    // A water tank 600 mm in front, a column 215 mm on its right (north).
    els.push(obj("tank", "water-tank", 7800.0, 2000.0, 0.0, &[]));
    els.push(Element::Column(Column {
        id: "col".into(),
        level_id: LEVEL.into(),
        center: p(6547.5, 2700.0),
        shape: ColumnShape::Rect,
        width_mm: 200.0,
        depth_mm: 200.0,
        rotation_deg: 0.0,
        material_id: None,
    }));
    let d = derive(els);
    assert_eq!(
        items(&d, "outdoor_unit_clearance")[0].message,
        "Aircon outdoor unit has too little free space around it: the water tank 600 mm in front of it and a column 215 mm on its right. Its manual asks for 2000 mm in front, 300 mm behind, 300 mm on the left and 600 mm on the right, for air flow and service."
    );
}

#[test]
fn a_raised_outdoor_unit_stands_on_something() {
    let unit = |x: f64, elevation: f64| raised(obj("ou", "aircon-outdoor-1hp", x, 2000.0, 90.0, &[]), elevation);
    // In the open, 1.2 m up.
    let mut els = shell();
    els.push(unit(9000.0, 1200.0));
    let d = derive(els);
    let found = items(&d, "outdoor_unit_unsupported");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Warning);
    assert_eq!(
        found[0].message,
        "Aircon outdoor unit stands 1.20 m above the floor with nothing under it. Set it on a slab or a ledge, or hang it on a wall bracket."
    );
    // On a wall bracket: the east wall is 200 mm behind it.
    let mut els = shell();
    els.push(unit(6427.5, 1200.0));
    assert!(items(&derive(els), "outdoor_unit_unsupported").is_empty());
    // On top of a water tank.
    let mut els = shell();
    els.push(obj("tank", "water-tank", 9000.0, 2000.0, 0.0, &[]));
    els.push(unit(9000.0, 1300.0));
    assert!(items(&derive(els), "outdoor_unit_unsupported").is_empty());
    // On the slab of the level above.
    let mut project = project_with(shell());
    project.levels.push(Level {
        id: "level-2".into(),
        name: "Roof deck".into(),
        elevation_mm: 3000.0,
        height_mm: 3000.0,
    });
    for w in shell().into_iter().filter(|e| e.kind() == ElementKind::Wall) {
        let mut upper = on_level(w, "level-2");
        *upper.id_mut() = format!("{}-2", upper.id());
        project.elements.push(upper);
    }
    project.elements.push(unit(3000.0, 3000.0));
    assert!(items(&compute_derived(&project), "outdoor_unit_unsupported").is_empty());
    // Standing on its own floor is never raised.
    let mut els = shell();
    els.push(unit(9000.0, 0.0));
    assert!(items(&derive(els), "outdoor_unit_unsupported").is_empty());
}

#[test]
fn a_tv_within_a_meter_of_an_aircon_unit() {
    let with_unit = |x: f64| {
        let mut els = shell();
        els.push(obj("tv", "tv-console", 3000.0, 3600.0, 0.0, &[]));
        // On the north wall, facing south.
        els.push(obj("iu", "aircon-indoor-1hp", x, 3810.0, 0.0, &[]));
        derive(els)
    };
    let d = with_unit(3000.0);
    let found = items(&d, "unit_near_tv");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity, Severity::Info);
    assert_eq!(found[0].element_ids, vec!["iu".to_string(), "tv".to_string()]);
    assert_eq!(
        found[0].message,
        "Bedroom aircon indoor unit is right above the Bedroom TV console. Its manual keeps the unit at least 1 m from a TV, radio or computer; move one of them."
    );
    assert_eq!(
        items(&with_unit(5000.0), "unit_near_tv")[0].message,
        "Bedroom aircon indoor unit is 800 mm from the Bedroom TV console. Its manual keeps the unit at least 1 m from a TV, radio or computer; move one of them."
    );
    assert!(items(&with_unit(5200.0), "unit_near_tv").is_empty());
}

// --------------------------------------------------------------- the demo

#[test]
fn the_services_demo_counts_its_devices() {
    let doc = Document::new(templates::plumbing_demo());
    let state = doc.state();
    assert_invariants(&state);
    let d = &state.derived;
    let rooms: Vec<(Option<&str>, &str, u32)> = d
        .schedule
        .iter()
        .map(|r| (r.room_id.as_deref().map(|id| &id[id.len() - 2..]), r.catalog_key.as_str(), r.count))
        .collect();
    // Rooms 01 Living / Kitchen, 02 Bedroom, 03 T&B, then outside.
    assert_eq!(
        rooms,
        vec![
            (Some("01"), "kitchen-sink", 1),
            (Some("01"), "light-ceiling", 1),
            (Some("01"), "light-pendant", 1),
            (Some("01"), "outlet-counter", 2),
            (Some("01"), "outlet-duplex", 2),
            (Some("01"), "outlet-spo", 1),
            (Some("01"), "panelboard", 1),
            (Some("01"), "smoke-detector", 1),
            (Some("01"), "switch-1", 1),
            (Some("01"), "switch-2", 1),
            (Some("02"), "light-ceiling", 1),
            (Some("02"), "outlet-aircon", 1),
            (Some("02"), "outlet-duplex", 2),
            (Some("02"), "switch-1", 2),
            (Some("02"), "aircon-indoor-1hp", 1),
            (Some("03"), "lavatory", 1),
            (Some("03"), "shower", 1),
            (Some("03"), "wc", 1),
            (Some("03"), "light-ceiling", 1),
            (None, "washing-machine", 1),
            (None, "light-outdoor", 1),
            (None, "outlet-spo", 1),
            (None, "aircon-outdoor-1hp", 1),
        ]
    );
    let q = doc.query(&Query::Schedule).unwrap();
    let level = &q["levels"][0];
    assert_eq!(level["total"], 27);
    assert_eq!(
        level["by_group"],
        json!([
            {"group": "plumbing", "count": 5},
            {"group": "electrical", "count": 20},
            {"group": "aircon", "count": 2},
        ])
    );
    // The range and the kitchen counter are not plumbing fixtures.
    assert!(d.schedule.iter().all(|r| r.catalog_key != "range" && r.catalog_key != "kitchen-counter"));
    assert_eq!(
        level["by_form_row"],
        json!([
            {"form_row": "Lighting outlets", "count": 5},
            {"form_row": "Convenience receptacles", "count": 6},
            {"form_row": "Special purpose outlets, aircon", "count": 1},
            {"form_row": "Special purpose outlets", "count": 2},
            {"form_row": "Toggle switches", "count": 4},
            {"form_row": "Panelboards", "count": 1},
            {"form_row": "Fire alarm detectors", "count": 1},
        ])
    );

    // The bedroom light is a 3-way: two switches link it.
    let light = "00000000-0000-4000-8000-000000018003";
    let switches = state
        .project
        .elements
        .iter()
        .filter(|e| matches!(e, Element::Asset(a) if a.links.iter().any(|l| l == light)))
        .count();
    assert_eq!(switches, 2);
}

#[test]
fn a_switch_for_the_demo_t_and_b_light_is_one_checked_step() {
    let mut doc = Document::new(templates::plumbing_demo());
    let level = doc.project().levels[0].id.clone();
    // Outside the T&B door, 200 mm past its latch at x = 6850, on the
    // bedroom face of the T&B wall.
    let switch = on_level(
        obj("", "switch-1", 7050.0, 4130.0, 0.0, &["00000000-0000-4000-8000-000000018004"]),
        &level,
    );
    let r = apply_checked(&mut doc, add(switch));
    assert_eq!(r.diff.summary, "Add switch: added 1 switch");
    let codes: Vec<&str> = r.state.derived.issues.iter().map(|i| i.code.as_str()).collect();
    assert!(!codes.contains(&"light_no_switch"), "{codes:?}");
    assert!(!codes.contains(&"switch_behind_door"), "{codes:?}");
    // Setting the line set note aside is one step too.
    let extra = r
        .state
        .derived
        .issues
        .iter()
        .find(|i| i.code == "lineset_extra")
        .unwrap()
        .id
        .clone();
    let r = apply_checked(&mut doc, mark(issue_target(&extra), Some("Quoted by the installer")));
    assert_eq!(issue_of(&r.state, &extra).status, IssueStatus::Ignored);
}

// -------------------------------------------------------------- migration

#[test]
fn a_version_2_project_opens_with_the_new_layers() {
    // A version 2 file: no storm, electrical or aircon layer, no site, no
    // review marks, objects without light, links or circuit, cameras
    // without light.
    let mut value = serde_json::to_value(templates::sample_bungalow()).unwrap();
    value["schema_version"] = json!(2);
    value.as_object_mut().unwrap().remove("review");
    value["settings"].as_object_mut().unwrap().remove("site");
    let layers: Vec<Value> = value["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| !matches!(l["key"].as_str(), Some("storm" | "electrical" | "aircon")))
        .map(|l| {
            let mut l = l.clone();
            if l["key"] == "drainage" {
                l["locked"] = json!(true);
            }
            l
        })
        .collect();
    assert_eq!(layers.len(), 13);
    value["layers"] = json!(layers);
    for e in value["elements"].as_array_mut().unwrap() {
        let map = e.as_object_mut().unwrap();
        match map["kind"].as_str() {
            Some("asset") => {
                map.remove("light");
                map.remove("links");
                map.remove("circuit");
            }
            Some("camera") => {
                map.remove("light");
            }
            _ => {}
        }
    }
    let old: Project = serde_json::from_value(value).expect("a version 2 file still reads");
    assert_eq!(old.schema_version, 2);
    assert_eq!(old.layers.len(), 13);

    let doc = Document::new(old);
    let project = doc.project();
    assert_eq!(project.schema_version, SCHEMA_VERSION);
    assert_eq!(SCHEMA_VERSION, 3);
    let keys: Vec<LayerKey> = project.layers.iter().map(|l| l.key).collect();
    let expected: Vec<LayerKey> = defaults::default_layers().iter().map(|l| l.key).collect();
    assert_eq!(keys, expected, "all 16 layers, in LayerKey order");
    for l in &project.layers[13..] {
        assert!(l.visible && !l.locked, "{:?}", l.key);
    }
    let drainage = project.layers.iter().find(|l| l.key == LayerKey::Drainage).unwrap();
    assert!(drainage.locked, "an old layer keeps its lock");
    assert!(project.review.is_empty());
    assert_eq!(project.settings.site, None);
    let bed = object(project, "00000000-0000-4000-8000-000000000401");
    assert_eq!((bed.light, bed.links.len(), bed.circuit.as_str()), (None, 0, ""));
    assert_invariants(&doc.state());
}
