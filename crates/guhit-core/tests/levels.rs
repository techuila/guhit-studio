//! Adding and deleting levels (docs/CONTRACT.md, "3D navigation and shell
//! view state", Levels): defaults, validation, deterministic ids, one undo
//! step, everything on a deleted level goes with it, links are cleaned up.

mod common;

use common::*;
use guhit_core::{templates, Document};
use guhit_model::*;

fn add_level(name: Option<&str>, elevation: Option<f64>, height: Option<f64>) -> Command {
    Command::AddLevel {
        name: name.map(str::to_string),
        elevation_mm: elevation,
        height_mm: height,
    }
}

fn level_named<'a>(project: &'a Project, name: &str) -> &'a Level {
    project
        .levels
        .iter()
        .find(|l| l.name == name)
        .unwrap_or_else(|| panic!("no level {name}"))
}

fn on(level: &str, e: Element) -> Element {
    let mut e = e;
    match &mut e {
        Element::Wall(w) => w.level_id = level.into(),
        Element::Room(r) => r.level_id = level.into(),
        Element::Asset(a) => a.level_id = level.into(),
        Element::Pipe(p) => p.level_id = level.into(),
        Element::Column(c) => c.level_id = level.into(),
        _ => {}
    }
    e
}

fn light(id: &str, links: &[&str]) -> Element {
    let item = defaults::asset_catalog()
        .into_iter()
        .find(|c| c.key == if links.is_empty() { "light-ceiling" } else { "switch-2" })
        .unwrap();
    Element::Asset(Asset {
        id: id.into(),
        level_id: LEVEL.into(),
        catalog_key: item.key,
        name: item.name,
        category: item.category,
        position: p(1000.0, 1000.0),
        rotation_deg: 0.0,
        width_mm: item.width_mm,
        depth_mm: item.depth_mm,
        height_mm: item.height_mm,
        elevation_mm: item.elevation_mm,
        light: item.light,
        links: links.iter().map(|s| s.to_string()).collect(),
        circuit: String::new(),
    })
}

#[test]
fn a_new_level_stacks_on_the_highest_one() {
    let mut doc = blank();
    let r = apply_checked(&mut doc, add_level(None, None, None));
    let levels = &r.state.project.levels;
    assert_eq!(levels.len(), 2);
    let second = &levels[1];
    assert_eq!(
        (second.name.as_str(), second.elevation_mm, second.height_mm),
        ("Level 2", 3000.0, 3000.0)
    );
    assert_eq!(second.id.len(), 36, "a UUID shaped id");
    assert_eq!(r.state.undo_label.as_deref(), Some("Add level"));
    assert_eq!(r.diff.summary, "Added Level 2, floor at 3000 mm, 3000 mm floor to floor");
    assert!(r.diff.added.is_empty(), "a level is not an element");

    // The id is the same for the same project and command.
    let again = Document::new(blank_project())
        .preview(&add_level(None, None, None))
        .unwrap();
    assert_eq!(again.state.project.levels[1].id, second.id);

    // One undo step each way.
    assert_eq!(doc.undo().unwrap().project.levels.len(), 1);
    assert_eq!(doc.redo().unwrap().project.levels[1], *second);

    // The next one goes on the highest level, by elevation, not by order.
    let r = apply_checked(&mut doc, add_level(Some("  Roof deck  "), None, Some(2400.0)));
    let deck = level_named(&r.state.project, "Roof deck");
    assert_eq!((deck.elevation_mm, deck.height_mm), (6000.0, 2400.0));
    // A basement goes below, and the list stays in elevation order.
    let r = apply_checked(&mut doc, add_level(Some("Basement"), Some(-2800.0), Some(2800.0)));
    let names: Vec<&str> = r.state.project.levels.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, vec!["Basement", "Ground Floor", "Level 2", "Roof deck"]);
    // The default name skips a name in use.
    let r = apply_checked(&mut doc, add_level(None, None, None));
    assert_eq!(r.state.project.levels.last().unwrap().name, "Level 5");
    assert_eq!(r.state.project.levels.last().unwrap().elevation_mm, 8400.0);
}

#[test]
fn a_new_level_is_checked() {
    let mut doc = blank();
    let rejected = |doc: &mut Document, cmd: Command| code(&apply_rejected(doc, cmd));
    assert_eq!(rejected(&mut doc, add_level(Some("   "), None, None)), "bad_name");
    let long = "L".repeat(61);
    assert_eq!(rejected(&mut doc, add_level(Some(&long), None, None)), "name_too_long");
    apply_checked(&mut doc, add_level(Some(&"L".repeat(60)), None, None));
    doc.undo().unwrap();
    assert_eq!(rejected(&mut doc, add_level(None, Some(f64::NAN), None)), "bad_level");
    assert_eq!(rejected(&mut doc, add_level(None, Some(2.0e6), None)), "bad_level");
    assert_eq!(rejected(&mut doc, add_level(None, None, Some(1999.0))), "bad_level");
    assert_eq!(rejected(&mut doc, add_level(None, None, Some(10_001.0))), "bad_level");
    assert_eq!(rejected(&mut doc, add_level(None, None, Some(f64::INFINITY))), "not_finite");
    // Two floors at one elevation.
    let e = apply_rejected(&mut doc, add_level(Some("Mezzanine"), Some(0.4), None));
    assert_eq!(code(&e), "level_elevation_taken");
    assert!(e.to_string().starts_with("Ground Floor already has its floor at 0 mm."), "{e}");
    apply_checked(&mut doc, add_level(None, None, Some(2000.0)));
    apply_checked(&mut doc, add_level(Some("Top"), None, Some(10_000.0)));
}

#[test]
fn deleting_a_level_takes_everything_on_it_in_one_step() {
    let mut project = templates::sample_bungalow();
    let ground = project.levels[0].id.clone();
    project.elements.push(on(&ground, light("ground-light", &[])));
    let mut doc = Document::new(project);
    let r = apply_checked(&mut doc, add_level(Some("Second Floor"), None, None));
    let upper = level_named(&r.state.project, "Second Floor").id.clone();
    // Upstairs: a room with a door, a pipe, and a switch that links a light
    // downstairs.
    apply_checked(
        &mut doc,
        Command::AddRectRoom {
            origin: p(0.0, 0.0),
            width_mm: 4000.0,
            depth_mm: 3000.0,
            name: Some("Upper bedroom".into()),
            thickness_mm: None,
            level_id: Some(upper.clone()),
        },
    );
    let south = walls(doc.project())
        .into_iter()
        .find(|w| w.level_id == upper && w.start.y == 0.0 && w.end.y == 0.0)
        .unwrap()
        .id
        .clone();
    apply_checked(&mut doc, opening(&south, OpeningType::Door, 2000.0));
    let mut sw = on(&upper, light("upper-switch", &["ground-light"]));
    if let Element::Asset(a) = &mut sw {
        a.position = p(2150.0, 95.0);
    }
    apply_checked(&mut doc, Command::AddElement { element: sw });
    apply_checked(
        &mut doc,
        Command::AddElement {
            element: on(
                &upper,
                Element::Pipe(Pipe {
                    id: String::new(),
                    level_id: String::new(),
                    system: PipeSystem::ColdWater,
                    material: PipeMaterial::Ppr,
                    diameter_mm: 20.0,
                    points: vec![Vec3 { x: 500.0, y: 500.0, z: 300.0 }, Vec3 { x: 2500.0, y: 500.0, z: 300.0 }],
                    name: String::new(),
                }),
            ),
        },
    );
    // Downstairs, a switch linked to the upstairs switch: deleting the upper
    // level must clear that link.
    let mut down = on(&ground, light("down-switch", &["upper-switch"]));
    if let Element::Asset(a) = &mut down {
        a.position = p(2150.0, 95.0);
    }
    apply_checked(&mut doc, Command::AddElement { element: down });
    let before = doc.state();
    let upstairs: Vec<Id> = before
        .project
        .elements
        .iter()
        .filter(|e| match e {
            Element::Opening(o) => walls(&before.project).iter().any(|w| w.id == o.wall_id && w.level_id == upper),
            Element::Camera(_) => false,
            other => serde_json::to_value(other).unwrap()["level_id"] == serde_json::json!(upper),
        })
        .map(|e| e.id().clone())
        .collect();
    assert_eq!(upstairs.len(), 8, "4 walls, a room, a door, a switch and a pipe");

    let r = apply_checked(&mut doc, Command::DeleteLevel { level_id: upper.clone() });
    let s = &r.state;
    assert_eq!(s.undo_label.as_deref(), Some("Delete level Second Floor"));
    assert_eq!(s.project.levels.len(), 1);
    assert_eq!(s.project.levels[0].id, ground);
    let mut removed = r.diff.removed.clone();
    removed.sort();
    let mut expected = upstairs.clone();
    expected.sort();
    assert_eq!(removed, expected);
    // The downstairs switch lost its link to the upstairs one, and the
    // ground floor, the camera and the light are still there.
    let down = s
        .project
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Asset(a) if a.id == "down-switch" => Some(a),
            _ => None,
        })
        .unwrap();
    assert!(down.links.is_empty());
    assert_eq!(r.diff.modified, vec!["down-switch".to_string()]);
    assert_eq!(
        r.diff.summary,
        "Delete level Second Floor: changed 1 switch; removed 1 opening, 1 pipe, 1 room, 1 switch, 4 walls"
    );
    assert!(s.project.elements.iter().any(|e| e.kind() == ElementKind::Camera));
    assert_eq!(rooms(&s.project).len(), 2);
    assert!(s.derived.pipes.takeoff.is_empty());

    // One undo brings the level and everything on it back, exactly.
    let undone = doc.undo().unwrap();
    assert_eq!(undone.project, before.project);
    assert_eq!(undone.derived, before.derived);
}

#[test]
fn the_last_level_and_locked_layers_stay() {
    let mut doc = blank();
    let e = apply_rejected(&mut doc, Command::DeleteLevel { level_id: LEVEL.into() });
    assert_eq!(code(&e), "last_level");
    assert!(e.to_string().contains("last level"), "{e}");
    let e = apply_rejected(&mut doc, Command::DeleteLevel { level_id: "nowhere".into() });
    assert_eq!(code(&e), "unknown_level");

    // A level whose walls are on a locked layer is kept, like any locked wall.
    let r = apply_checked(&mut doc, add_level(Some("Upper"), None, None));
    let upper = level_named(&r.state.project, "Upper").id.clone();
    apply_checked(
        &mut doc,
        Command::AddWall {
            start: p(0.0, 0.0),
            end: p(3000.0, 0.0),
            thickness_mm: None,
            height_mm: None,
            material_id: None,
            level_id: Some(upper.clone()),
        },
    );
    apply_checked(
        &mut doc,
        Command::SetLayer {
            layer: Layer {
                key: LayerKey::Walls,
                visible: true,
                locked: true,
            },
        },
    );
    let e = apply_rejected(&mut doc, Command::DeleteLevel { level_id: upper.clone() });
    assert_eq!(code(&e), "layer_locked");
    // An empty level goes even with the layer locked.
    let r = apply_checked(&mut doc, add_level(Some("Empty"), None, None));
    let empty = level_named(&r.state.project, "Empty").id.clone();
    let r = apply_checked(&mut doc, Command::DeleteLevel { level_id: empty });
    assert_eq!(r.state.project.levels.len(), 2);
    assert_eq!(r.diff.summary, "Delete level Empty");
}

#[test]
fn a_two_storey_services_demo_keeps_its_ground_floor_when_the_upper_goes() {
    let mut doc = Document::new(templates::plumbing_demo());
    let before = doc.state();
    let r = apply_checked(&mut doc, add_level(None, None, None));
    let upper = r.state.project.levels[1].id.clone();
    // The roof sits on the top level, as in the 3D view: an empty upper
    // level has none, so the vent stack no longer goes through a roof.
    // Every other finding and the schedule of the ground floor stay.
    let summary = |d: &Derived| {
        d.issues
            .iter()
            .find(|i| i.code == "pipe_penetrations")
            .map(|i| i.message.clone())
            .unwrap()
    };
    assert!(summary(&r.state.derived).starts_with("6 pipe penetrations need sleeves: 6 through the floor slab."));
    let others = |d: &Derived| {
        d.issues
            .iter()
            .filter(|i| i.code != "pipe_penetrations")
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(others(&r.state.derived), others(&before.derived));
    assert_eq!(r.state.derived.schedule, before.derived.schedule);
    let r = apply_checked(&mut doc, Command::DeleteLevel { level_id: upper });
    assert_eq!(r.state.project.elements, before.project.elements);
    assert_eq!(r.state.derived, before.derived);
}
