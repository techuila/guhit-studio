//! AI edit scope (DECISIONS D30): what a selection reaches, where new
//! elements may go, and which commands stay inside it.

mod common;

use common::*;
use guhit_core::{compute_derived, scope, templates, CoreError, Document};
use guhit_model::*;

// Fixed ids from `templates::sample_bungalow`: an 8 x 6 m house with the
// Living / Dining west of a partition at x = 5000 and the Bedroom east of it.
// The south and north walls run the whole length, so both rooms share them.
const GROUND: &str = "00000000-0000-4000-8000-0000000000a1";
const LIVING: &str = "00000000-0000-4000-8000-000000000301";
const BEDROOM: &str = "00000000-0000-4000-8000-000000000302";
const SOUTH: &str = "00000000-0000-4000-8000-000000000101";
const EAST: &str = "00000000-0000-4000-8000-000000000102";
const NORTH: &str = "00000000-0000-4000-8000-000000000103";
const WEST: &str = "00000000-0000-4000-8000-000000000104";
const PARTITION: &str = "00000000-0000-4000-8000-000000000105";
/// On the south wall, in the living room.
const FRONT_DOOR: &str = "00000000-0000-4000-8000-000000000201";
/// On the partition.
const BEDROOM_DOOR: &str = "00000000-0000-4000-8000-000000000202";
/// On the south wall, in the living room.
const LIVING_WINDOW: &str = "00000000-0000-4000-8000-000000000203";
/// On the east wall.
const BEDROOM_WINDOW: &str = "00000000-0000-4000-8000-000000000204";
/// On the north wall, in the living room.
const NORTH_WINDOW: &str = "00000000-0000-4000-8000-000000000205";
const BED: &str = "00000000-0000-4000-8000-000000000401";
/// From (0, 0) to (8000, 0), the whole south side.
const SOUTH_DIMENSION: &str = "00000000-0000-4000-8000-000000000501";
/// Outside the house.
const EXTERIOR_CAMERA: &str = "00000000-0000-4000-8000-000000000601";
const UPPER: &str = "upper";

const OUTSIDE: &str = "outside the selection this edit is limited to.";

fn object(id: &str, level: &str, key: &str, x: f64, y: f64) -> Element {
    let item = defaults::asset_catalog()
        .into_iter()
        .find(|c| c.key == key)
        .expect("catalog item");
    Element::Asset(Asset {
        id: id.into(),
        level_id: level.into(),
        catalog_key: item.key,
        name: item.name,
        category: item.category,
        position: p(x, y),
        rotation_deg: 0.0,
        width_mm: item.width_mm,
        depth_mm: item.depth_mm,
        height_mm: item.height_mm,
        elevation_mm: item.elevation_mm,
        light: item.light,
        links: vec![],
        circuit: String::new(),
    })
}

fn column(id: &str, x: f64, y: f64) -> Element {
    Element::Column(Column {
        id: id.into(),
        level_id: GROUND.into(),
        center: p(x, y),
        shape: ColumnShape::Rect,
        width_mm: 200.0,
        depth_mm: 200.0,
        rotation_deg: 0.0,
        material_id: None,
    })
}

fn stair(id: &str, x: f64, y: f64) -> Element {
    Element::Stair(Stair {
        id: id.into(),
        level_id: GROUND.into(),
        origin: p(x, y),
        rotation_deg: 0.0,
        width_mm: 900.0,
        run_mm: 2700.0,
        riser_count: 15,
    })
}

fn note(id: &str, x: f64, y: f64) -> Element {
    Element::Annotation(Annotation {
        id: id.into(),
        level_id: GROUND.into(),
        position: p(x, y),
        text: "Closet".into(),
        size_mm: 250.0,
        rotation_deg: 0.0,
    })
}

fn dim(id: &str, a: Point, b: Point) -> Element {
    Element::Dimension(Dimension {
        id: id.into(),
        level_id: GROUND.into(),
        a,
        b,
        offset_mm: 500.0,
        text_override: None,
    })
}

fn pipe(id: &str, points: &[(f64, f64)]) -> Element {
    Element::Pipe(Pipe {
        id: id.into(),
        level_id: GROUND.into(),
        system: PipeSystem::ColdWater,
        material: PipeMaterial::Ppr,
        diameter_mm: 20.0,
        points: points
            .iter()
            .map(|(x, y)| Vec3 { x: *x, y: *y, z: 300.0 })
            .collect(),
        name: String::new(),
    })
}

fn camera(id: &str, x: f64, y: f64, z: f64) -> Element {
    Element::Camera(Camera {
        id: id.into(),
        name: "Bedroom view".into(),
        preset: CameraPreset::RoomInterior,
        position: Vec3 { x, y, z },
        target: Vec3 { x: x + 1000.0, y, z: 1200.0 },
        fov_deg: 60.0,
        light: None,
    })
}

/// The sample bungalow with a second level, and something of every kind in
/// the bedroom and outside it.
fn house() -> Project {
    let mut project = templates::sample_bungalow();
    project.levels.push(Level {
        id: UPPER.into(),
        name: "Second Floor".into(),
        elevation_mm: 3000.0,
        height_mm: 3000.0,
    });
    let mut switch = object("switch-bed", GROUND, "switch-1", 5100.0, 3700.0);
    if let Element::Asset(a) = &mut switch {
        a.links = vec!["lamp-living".into()];
    }
    project.elements.extend([
        column("col-in", 5500.0, 1000.0),
        column("col-out", 2000.0, 1000.0),
        stair("stair-in", 7400.0, 500.0),
        note("note-in", 6000.0, 5500.0),
        dim("dim-in", p(5000.0, 3000.0), p(8000.0, 3000.0)),
        dim("dim-across", p(2000.0, 3000.0), p(7000.0, 3000.0)),
        pipe("pipe-in", &[(6000.0, 1000.0), (7000.0, 1000.0), (7000.0, 2000.0)]),
        pipe("pipe-across", &[(4000.0, 2000.0), (6000.0, 2000.0)]),
        camera("cam-in", 6500.0, 3000.0, 1600.0),
        camera("cam-up", 6500.0, 3000.0, 4600.0),
        object("lamp-up", UPPER, "light-ceiling", 6500.0, 3000.0),
        object("lamp-living", GROUND, "light-ceiling", 2500.0, 3000.0),
        switch,
    ]);
    project
}

fn ids(list: &[&str]) -> Vec<Id> {
    list.iter().map(|s| s.to_string()).collect()
}

fn scoped(project: &Project, scope_ids: &[&str], command: &Command) -> Result<(), CoreError> {
    scope::check(project, &compute_derived(project), &ids(scope_ids), command)
}

fn allowed(project: &Project, scope_ids: &[&str], command: &Command) {
    if let Err(e) = scoped(project, scope_ids, command) {
        panic!("{command:?} should stay inside {scope_ids:?}: {e}");
    }
}

/// The message and element ids of an `out_of_scope` refusal.
fn refused(project: &Project, scope_ids: &[&str], command: &Command) -> (String, Vec<Id>) {
    match scoped(project, scope_ids, command) {
        Err(CoreError::Invalid {
            code,
            message,
            element_ids,
        }) if code == "out_of_scope" => (message, element_ids),
        other => panic!("{command:?} should be outside {scope_ids:?}, got {other:?}"),
    }
}

fn delete(id: &str) -> Command {
    Command::DeleteElements { ids: ids(&[id]) }
}

fn add(element: Element) -> Command {
    Command::AddElement { element }
}

fn lamp(level: &str, x: f64, y: f64) -> Command {
    add(object("", level, "light-ceiling", x, y))
}

fn opening_of(project: &Project, id: &str) -> Opening {
    openings(project)
        .into_iter()
        .find(|o| o.id == id)
        .expect("opening")
        .clone()
}

// ------------------------------------------------------------------ reach

#[test]
fn a_room_reaches_its_walls_their_doors_and_windows_and_what_stands_inside() {
    let project = house();
    for id in [
        BEDROOM,
        SOUTH,
        EAST,
        NORTH,
        PARTITION,
        BEDROOM_DOOR,
        BEDROOM_WINDOW,
        BED,
        "switch-bed",
        "col-in",
        "stair-in",
        "note-in",
        "dim-in",
        "pipe-in",
        "cam-in",
    ] {
        allowed(&project, &[BEDROOM], &delete(id));
    }
    for id in [
        LIVING,
        WEST,
        // On the living room's part of the walls the bedroom shares.
        FRONT_DOOR,
        LIVING_WINDOW,
        NORTH_WINDOW,
        "col-out",
        "lamp-living",
        // One end, or one point, outside the room.
        "dim-across",
        SOUTH_DIMENSION,
        "pipe-across",
        EXTERIOR_CAMERA,
        // Right above the bedroom, one level up.
        "cam-up",
        "lamp-up",
    ] {
        let (message, element_ids) = refused(&project, &[BEDROOM], &delete(id));
        assert!(message.ends_with(&format!("is {OUTSIDE}")), "{message}");
        assert_eq!(element_ids, ids(&[id]));
    }
    // Named in plain words, without ids.
    assert_eq!(
        refused(&project, &[BEDROOM], &delete(WEST)).0,
        format!("Wall 6000 mm is {OUTSIDE}")
    );
    assert_eq!(
        refused(&project, &[BEDROOM], &delete(FRONT_DOOR)).0,
        format!("Door 900 x 2100 is {OUTSIDE}")
    );
    assert_eq!(
        refused(&project, &[BEDROOM], &delete("lamp-living")).0,
        format!("Ceiling light is {OUTSIDE}")
    );
    assert_eq!(
        refused(&project, &[BEDROOM], &delete("pipe-across")).0,
        format!("Cold water pipe 20 mm is {OUTSIDE}")
    );

    // The living room gets its own doors and windows on the shared walls.
    for id in [LIVING, WEST, SOUTH, NORTH, PARTITION, FRONT_DOOR, LIVING_WINDOW, NORTH_WINDOW, BEDROOM_DOOR, "col-out", "lamp-living"] {
        allowed(&project, &[LIVING], &delete(id));
    }
    for id in [BEDROOM_WINDOW, EAST, BED, SOUTH_DIMENSION, "dim-across"] {
        refused(&project, &[LIVING], &delete(id));
    }
}

#[test]
fn a_wall_standing_inside_a_room_is_in_its_reach() {
    let mut project = house();
    let wall = |id: &str, a: Point, b: Point| {
        Element::Wall(Wall {
            id: id.into(),
            level_id: GROUND.into(),
            start: a,
            end: b,
            thickness_mm: 100.0,
            height_mm: None,
            material_id: None,
        })
    };
    project.elements.extend([
        wall("island", p(5600.0, 2000.0), p(7400.0, 2000.0)),
        wall("yard", p(9000.0, 1000.0), p(9000.0, 3000.0)),
    ]);
    allowed(&project, &[BEDROOM], &delete("island"));
    refused(&project, &[BEDROOM], &delete("yard"));
}

#[test]
fn a_wall_reaches_its_doors_and_windows_and_anything_else_only_itself() {
    let project = house();
    for id in [SOUTH, FRONT_DOOR, LIVING_WINDOW] {
        allowed(&project, &[SOUTH], &delete(id));
    }
    for id in [EAST, BEDROOM_DOOR, BEDROOM, LIVING, BED] {
        refused(&project, &[SOUTH], &delete(id));
    }

    allowed(&project, &[BED], &delete(BED));
    for id in [BEDROOM, "col-in", EAST] {
        refused(&project, &[BED], &delete(id));
    }
    allowed(&project, &[BEDROOM_DOOR], &delete(BEDROOM_DOOR));
    refused(&project, &[BEDROOM_DOOR], &delete(PARTITION));
}

// ------------------------------------------------------------------- area

#[test]
fn new_walls_must_land_in_the_area_within_the_tolerance() {
    let project = house();
    // From the partition's centerline to the east wall's: on the boundary.
    allowed(&project, &[BEDROOM], &add_wall(p(5000.0, 2000.0), p(8000.0, 2000.0)));
    // A little past both centerlines is inside the 50 mm tolerance.
    allowed(&project, &[BEDROOM], &add_wall(p(4960.0, 2500.0), p(8040.0, 2500.0)));

    let (message, element_ids) = refused(&project, &[BEDROOM], &add_wall(p(9000.0, 1000.0), p(9000.0, 4000.0)));
    assert_eq!(message, format!("The new wall would be {OUTSIDE}"));
    assert!(element_ids.is_empty());
    // 1 m out past the east wall, into the living room, and just past the
    // tolerance.
    refused(&project, &[BEDROOM], &add_wall(p(6000.0, 3000.0), p(9000.0, 3000.0)));
    refused(&project, &[BEDROOM], &add_wall(p(6000.0, 3000.0), p(4000.0, 3000.0)));
    refused(&project, &[BEDROOM], &add_wall(p(4900.0, 2500.0), p(8000.0, 2500.0)));

    // Every corner and side of a chain or a rectangular room.
    let chain = |points: &[Point], closed: bool| Command::AddWallChain {
        points: points.to_vec(),
        closed,
        thickness_mm: None,
        level_id: None,
    };
    let inside = [p(5500.0, 500.0), p(7500.0, 500.0), p(7500.0, 1500.0)];
    allowed(&project, &[BEDROOM], &chain(&inside, true));
    let (message, _) = refused(
        &project,
        &[BEDROOM],
        &chain(&[p(5500.0, 500.0), p(7500.0, 500.0), p(4000.0, 1500.0)], false),
    );
    assert_eq!(message, format!("The new walls would be {OUTSIDE}"));
    allowed(&project, &[BEDROOM], &rect_room(5500.0, 3500.0, 2000.0, 2000.0, Some("Closet")));
    let (message, _) = refused(&project, &[BEDROOM], &rect_room(4000.0, 3500.0, 2000.0, 2000.0, None));
    assert_eq!(message, format!("The new room would be {OUTSIDE}"));

    // Any other selected element gives its bounding box grown by 500 mm, and
    // a segment between two such boxes must not cross the plan between them.
    allowed(&project, &[WEST, EAST], &add_wall(p(-400.0, 1000.0), p(400.0, 1000.0)));
    refused(&project, &[WEST, EAST], &add_wall(p(-600.0, 1000.0), p(400.0, 1000.0)));
    refused(&project, &[WEST, EAST], &add_wall(p(200.0, 3000.0), p(7800.0, 3000.0)));
    refused(&project, &[WEST, EAST], &add(pipe("", &[(200.0, 3000.0), (7800.0, 3000.0)])));
}

#[test]
fn the_area_is_per_level() {
    let project = house();
    allowed(&project, &[BEDROOM], &lamp(GROUND, 6500.0, 3000.0));
    let (message, _) = refused(&project, &[BEDROOM], &lamp(UPPER, 6500.0, 3000.0));
    assert_eq!(message, format!("The new light would be {OUTSIDE}"));
    // A wall without a level goes on the first level, as the engine puts it.
    allowed(&project, &[BEDROOM], &add_wall(p(5500.0, 2000.0), p(7500.0, 2000.0)));
    let upstairs = Command::AddWall {
        start: p(5500.0, 2000.0),
        end: p(7500.0, 2000.0),
        thickness_mm: None,
        height_mm: None,
        material_id: None,
        level_id: Some(UPPER.into()),
    };
    refused(&project, &[BEDROOM], &upstairs);
    // Selected on the upper level, the same spot is inside.
    allowed(&project, &["lamp-up"], &lamp(UPPER, 6700.0, 3000.0));
    refused(&project, &["lamp-up"], &lamp(GROUND, 6700.0, 3000.0));
}

#[test]
fn a_door_or_window_needs_its_wall_in_reach_and_the_selections_part_of_it() {
    let project = house();
    allowed(&project, &[BEDROOM], &opening(EAST, OpeningType::Window, 1500.0));
    let (message, element_ids) = refused(&project, &[BEDROOM], &opening(WEST, OpeningType::Door, 3000.0));
    assert_eq!(message, format!("The new door would be on Wall 6000 mm, which is {OUTSIDE}"));
    assert_eq!(element_ids, ids(&[WEST]));
    // The south wall bounds both rooms: only the bedroom's part is inside.
    allowed(&project, &[BEDROOM], &opening(SOUTH, OpeningType::Window, 6500.0));
    let (message, element_ids) = refused(&project, &[BEDROOM], &opening(SOUTH, OpeningType::Window, 2000.0));
    assert_eq!(
        message,
        format!(
            "The new window would be on a part of Wall 8000 mm that is {OUTSIDE} \
             The selection covers it from 4950 to 8000 mm from its start."
        )
    );
    assert_eq!(element_ids, ids(&[SOUTH]));
    // With the front door selected too, the wall is covered in two stretches.
    let (message, _) = refused(&project, &[BEDROOM, FRONT_DOOR], &opening(SOUTH, OpeningType::Window, 3500.0));
    assert!(
        message.ends_with("The selection covers it from 550 to 2450 and from 4950 to 8000 mm from its start."),
        "{message}"
    );
    allowed(&project, &[BEDROOM, FRONT_DOOR], &opening(SOUTH, OpeningType::Window, 2300.0));
    // A door or window reaches only itself, not its wall.
    refused(&project, &[BEDROOM_DOOR], &opening(PARTITION, OpeningType::Door, 1000.0));

    // The same through add_element.
    let mut door = opening_of(&project, BEDROOM_DOOR);
    door.id = String::new();
    door.wall_id = EAST.into();
    door.offset_mm = 1500.0;
    allowed(&project, &[BEDROOM], &add(Element::Opening(door.clone())));
    door.wall_id = WEST.into();
    refused(&project, &[BEDROOM], &add(Element::Opening(door)));

    assert!(matches!(
        scoped(&project, &[BEDROOM], &opening("no-such-wall", OpeningType::Door, 500.0)),
        Err(CoreError::NotFound(id)) if id == "no-such-wall"
    ));
}

#[test]
fn add_element_checks_where_each_kind_lands() {
    let project = house();
    let cases: Vec<(Element, Element, &str)> = vec![
        (column("", 6000.0, 2000.0), column("", 2000.0, 2000.0), "column"),
        (stair("", 6000.0, 1000.0), stair("", 2000.0, 1000.0), "stair"),
        (note("", 6000.0, 1000.0), note("", 2000.0, 1000.0), "note"),
        (
            dim("", p(5000.0, 1000.0), p(8000.0, 1000.0)),
            dim("", p(5000.0, 1000.0), p(9000.0, 1000.0)),
            "dimension",
        ),
        (
            pipe("", &[(5500.0, 500.0), (7500.0, 500.0), (7500.0, 5500.0)]),
            pipe("", &[(5500.0, 500.0), (7500.0, 500.0), (7500.0, 6600.0)]),
            "pipe",
        ),
        (
            object("", GROUND, "bed-double", 6500.0, 2000.0),
            object("", GROUND, "bed-double", 2500.0, 2000.0),
            "object",
        ),
        (
            Element::Wall(Wall {
                id: String::new(),
                level_id: GROUND.into(),
                start: p(5500.0, 3000.0),
                end: p(7500.0, 3000.0),
                thickness_mm: 100.0,
                height_mm: None,
                material_id: None,
            }),
            Element::Wall(Wall {
                id: String::new(),
                level_id: GROUND.into(),
                start: p(5500.0, 3000.0),
                end: p(2500.0, 3000.0),
                thickness_mm: 100.0,
                height_mm: None,
                material_id: None,
            }),
            "wall",
        ),
        (
            Element::Room(Room {
                id: String::new(),
                level_id: GROUND.into(),
                name: "Nook".into(),
                usage: RoomUsage::Other,
                seed: p(6000.0, 2000.0),
                floor_material_id: None,
                auto_named: false,
            }),
            Element::Room(Room {
                id: String::new(),
                level_id: GROUND.into(),
                name: "Nook".into(),
                usage: RoomUsage::Other,
                seed: p(2000.0, 2000.0),
                floor_material_id: None,
                auto_named: false,
            }),
            "room",
        ),
    ];
    for (inside, outside, noun) in cases {
        allowed(&project, &[BEDROOM], &add(inside));
        let (message, _) = refused(&project, &[BEDROOM], &add(outside));
        assert_eq!(message, format!("The new {noun} would be {OUTSIDE}"));
    }

    // A view changes no part of the plan: always allowed.
    allowed(&project, &[BEDROOM], &add(camera("", -5000.0, -7000.0, 4500.0)));
    // Tracing references are never added under a scope, even inside it.
    let underlay = Element::Underlay(Underlay {
        id: String::new(),
        level_id: GROUND.into(),
        file_name: "plan.png".into(),
        position: p(6000.0, 1000.0),
        width_px: 100,
        height_px: 100,
        mm_per_px: 10.0,
        scale_confirmed: true,
        rotation_deg: 0.0,
        opacity: 0.5,
        locked: false,
    });
    let linework = Element::Linework(Linework {
        id: String::new(),
        level_id: GROUND.into(),
        name: "site.dxf / A-WALL".into(),
        polylines: vec![vec![p(6000.0, 1000.0), p(7000.0, 1000.0)]],
        color: "#333333".into(),
        locked: false,
    });
    let model = Element::ReferenceModel(ReferenceModel {
        id: String::new(),
        level_id: GROUND.into(),
        name: "Wardrobe".into(),
        file_name: "wardrobe.glb".into(),
        position: p(6500.0, 3000.0),
        rotation_deg: 0.0,
        elevation_mm: 0.0,
        scale_to_mm: 1000.0,
        locked: false,
    });
    for (element, noun) in [(underlay, "underlay"), (linework, "linework"), (model, "reference model")] {
        let (message, _) = refused(&project, &[BEDROOM], &add(element));
        assert_eq!(message, format!("The new {noun} would be {OUTSIDE}"));
    }
}

// --------------------------------------------------------------- commands

#[test]
fn an_update_needs_the_element_and_any_new_wall_in_reach() {
    let project = house();
    let mut bed = match project.elements.iter().find(|e| e.id() == BED) {
        Some(Element::Asset(a)) => a.clone(),
        other => panic!("{other:?}"),
    };
    bed.rotation_deg = 90.0;
    allowed(&project, &[BEDROOM], &Command::UpdateElement { element: Element::Asset(bed.clone()) });
    refused(&project, &[LIVING], &Command::UpdateElement { element: Element::Asset(bed) });

    // The bedroom door moved onto another wall.
    let mut door = opening_of(&project, BEDROOM_DOOR);
    door.wall_id = EAST.into();
    door.offset_mm = 1500.0;
    allowed(&project, &[BEDROOM], &Command::UpdateElement { element: Element::Opening(door.clone()) });
    door.wall_id = WEST.into();
    let (message, element_ids) = refused(&project, &[BEDROOM], &Command::UpdateElement { element: Element::Opening(door.clone()) });
    assert_eq!(message, format!("Door 800 x 2100 would be on Wall 6000 mm, which is {OUTSIDE}"));
    assert_eq!(element_ids, ids(&[WEST]));
    door.wall_id = SOUTH.into();
    door.offset_mm = 2000.0;
    let (message, _) = refused(&project, &[BEDROOM], &Command::UpdateElement { element: Element::Opening(door) });
    assert_eq!(
        message,
        format!(
            "Door 800 x 2100 would be on a part of Wall 8000 mm that is {OUTSIDE} \
             The selection covers it from 4950 to 8000 mm from its start."
        )
    );

    let mut ghost = opening_of(&project, BEDROOM_DOOR);
    ghost.id = "no-such-door".into();
    assert!(matches!(
        scoped(&project, &[BEDROOM], &Command::UpdateElement { element: Element::Opening(ghost) }),
        Err(CoreError::NotFound(id)) if id == "no-such-door"
    ));
}

#[test]
fn every_target_must_be_in_reach() {
    let project = house();
    let commands = |wall: &str, room: &str, thing: &str| {
        vec![
            Command::SetWallEndpoints {
                wall_id: wall.into(),
                start: p(8000.0, 0.0),
                end: p(8000.0, 6500.0),
            },
            Command::SetWallLength {
                wall_id: wall.into(),
                length_mm: 6500.0,
                anchor: WallAnchor::Start,
            },
            Command::SplitWall {
                wall_id: wall.into(),
                at_mm: 2000.0,
            },
            Command::ResizeRoom {
                room_id: room.into(),
                side: Side::East,
                delta_mm: 500.0,
            },
            Command::DeleteElements { ids: ids(&[thing]) },
            Command::MoveElements {
                ids: ids(&[thing]),
                delta: p(0.0, -300.0),
                stretch_connected: false,
            },
            Command::RotateElements {
                ids: ids(&[thing]),
                pivot: p(6000.0, 3000.0),
                angle_deg: 15.0,
            },
            Command::DuplicateElements {
                ids: ids(&[thing]),
                delta: p(0.0, -300.0),
            },
            Command::SetMaterial {
                ids: ids(&[wall]),
                material_id: defaults::MAT_WALL_DEFAULT.into(),
            },
        ]
    };
    for command in commands(EAST, BEDROOM, "col-in") {
        allowed(&project, &[BEDROOM], &command);
    }
    for command in commands(WEST, LIVING, "col-out") {
        let (message, element_ids) = refused(&project, &[BEDROOM], &command);
        assert!(message.ends_with(&format!("is {OUTSIDE}")), "{command:?}: {message}");
        assert_eq!(element_ids.len(), 1);
    }

    // Every target outside is listed; the first is named.
    let (message, element_ids) = refused(
        &project,
        &[BEDROOM],
        &Command::DeleteElements {
            ids: ids(&[BED, WEST, "col-out", LIVING, "col-out"]),
        },
    );
    assert_eq!(message, format!("Wall 6000 mm and 2 other elements are {OUTSIDE}"));
    assert_eq!(element_ids, ids(&[WEST, "col-out", LIVING]));

    assert!(matches!(
        scoped(&project, &[BEDROOM], &delete("no-such-element")),
        Err(CoreError::NotFound(id)) if id == "no-such-element"
    ));
}

#[test]
fn copies_must_land_in_the_area() {
    let project = house();
    let copy = |id: &str, x: f64, y: f64| Command::DuplicateElements {
        ids: ids(&[id]),
        delta: p(x, y),
    };
    allowed(&project, &[BEDROOM], &copy(BED, -1000.0, -2000.0));
    let (message, element_ids) = refused(&project, &[BEDROOM], &copy(BED, -4000.0, 0.0));
    assert_eq!(message, format!("The copy of Double bed would be {OUTSIDE}"));
    assert_eq!(element_ids, ids(&[BED]));

    // A window copied without its wall stays on that wall.
    allowed(&project, &[BEDROOM], &copy(BEDROOM_WINDOW, 0.0, 1500.0));
    let (message, _) = refused(&project, &[BEDROOM_WINDOW], &copy(BEDROOM_WINDOW, 0.0, 1500.0));
    assert_eq!(
        message,
        format!("The copy of Window 1200 x 1200 would be on Wall 6000 mm, which is {OUTSIDE}")
    );
    // A wall copied with its windows: only the wall has to land.
    allowed(&project, &[BEDROOM], &copy(PARTITION, 1500.0, 0.0));
    refused(&project, &[BEDROOM], &copy(PARTITION, -1500.0, 0.0));
    // A view may be copied anywhere.
    allowed(&project, &[BEDROOM], &copy("cam-in", -20_000.0, 0.0));
}

#[test]
fn a_review_mark_needs_what_it_names_in_reach() {
    let mut project = house();
    for el in project.elements.iter_mut() {
        if let Element::Opening(o) = el {
            if o.id == BEDROOM_DOOR || o.id == FRONT_DOOR {
                o.width_mm = 600.0;
            }
        }
    }
    let derived = compute_derived(&project);
    let finding = |door: &str| {
        derived
            .issues
            .iter()
            .find(|i| i.code == "door_narrow" && i.element_ids == ids(&[door]))
            .expect("a narrow door finding")
            .id
            .clone()
    };
    let mark = |target: ReviewTarget, note: Option<&str>| Command::SetReviewMark {
        target,
        note: note.map(str::to_string),
    };

    allowed(&project, &[BEDROOM], &mark(ReviewTarget::Issue { id: finding(BEDROOM_DOOR) }, Some("Closet door")));
    let (message, element_ids) = refused(&project, &[BEDROOM], &mark(ReviewTarget::Issue { id: finding(FRONT_DOOR) }, Some("ok")));
    assert_eq!(message, format!("That review item involves Door 600 x 2100, which is {OUTSIDE}"));
    assert_eq!(element_ids, ids(&[FRONT_DOOR]));

    let on = |element_id: &str| ReviewTarget::Element {
        code: "door_narrow".into(),
        element_id: element_id.into(),
    };
    allowed(&project, &[BEDROOM], &mark(on(BEDROOM_DOOR), Some("Closet door")));
    let (message, _) = refused(&project, &[BEDROOM], &mark(on(FRONT_DOOR), None));
    assert_eq!(message, format!("Door 600 x 2100 is {OUTSIDE}"));

    // A finding the checks no longer make still names its elements.
    let resolved = ReviewTarget::Issue {
        id: format!("door_narrow:{BEDROOM_WINDOW}"),
    };
    allowed(&project, &[BEDROOM], &mark(resolved.clone(), None));
    refused(&project, &[LIVING], &mark(resolved, None));

    // A whole check is never inside a selection.
    let whole = ReviewTarget::Check {
        code: "door_narrow".into(),
    };
    let (message, _) = refused(&project, &[BEDROOM], &mark(whole.clone(), Some("ok")));
    assert_eq!(
        message,
        format!("Setting a whole review check aside affects the whole project, which is {OUTSIDE}")
    );
    let (message, _) = refused(&project, &[BEDROOM], &mark(whole, None));
    assert!(message.starts_with("Reopening a whole review check"), "{message}");
}

#[test]
fn project_wide_commands_are_outside_every_scope() {
    let project = house();
    let commands = [
        (Command::SetRoof { roof: project.roof.clone() }, "Changing the roof"),
        (
            Command::SetProjectSettings {
                settings: project.settings.clone(),
            },
            "Changing the project settings",
        ),
        (
            Command::UpdateLevel {
                level: project.levels[0].clone(),
            },
            "Changing a level",
        ),
        (
            Command::AddLevel {
                name: None,
                elevation_mm: None,
                height_mm: None,
            },
            "Adding a level",
        ),
        (Command::DeleteLevel { level_id: UPPER.into() }, "Deleting a level"),
        (
            Command::SetLayer {
                layer: project.layers[0].clone(),
            },
            "Changing a layer",
        ),
        (
            Command::UpsertMaterial {
                material: project.materials[0].clone(),
            },
            "Adding or changing a material",
        ),
    ];
    for (command, what) in commands {
        for scope_ids in [&[BEDROOM][..], &[SOUTH, BED, EXTERIOR_CAMERA]] {
            let (message, element_ids) = refused(&project, scope_ids, &command);
            assert_eq!(message, format!("{what} affects the whole project, which is {OUTSIDE}"));
            assert!(element_ids.is_empty());
        }
    }
    assert_eq!(
        refused(&project, &[BEDROOM], &Command::SetRoof { roof: project.roof.clone() }).0,
        "Changing the roof affects the whole project, which is outside the selection this edit is limited to."
    );
}

// --------------------------------------------------------------- batches

#[test]
fn a_batch_is_checked_step_by_step() {
    let project = house();
    let doc = Document::new(project.clone());
    // Scoped to the east wall, a later step hosts a window on the wall an
    // earlier step made next to it. Ids are seeded per leaf command, so the
    // wall gets the id a preview of it alone gives.
    let wall = add_wall(p(7700.0, 1000.0), p(7700.0, 3000.0));
    let new_wall = doc.preview(&wall).unwrap().diff.added[0].clone();
    let window = opening(&new_wall, OpeningType::Window, 1000.0);
    let batch = |commands: Vec<Command>| Command::Batch {
        label: "Closet".into(),
        commands,
    };
    let good = batch(vec![wall.clone(), window.clone()]);
    allowed(&project, &[EAST], &good);
    doc.preview(&good).expect("and it runs");
    // Without the first step the new wall is not there to host it.
    assert!(matches!(
        scoped(&project, &[EAST], &window),
        Err(CoreError::NotFound(id)) if id == new_wall
    ));

    // One step outside refuses the whole batch and says which step.
    let (message, _) = refused(&project, &[EAST], &batch(vec![wall.clone(), lamp(GROUND, 6500.0, 3000.0)]));
    assert_eq!(message, format!("Step 2 of 2: The new light would be {OUTSIDE}"));

    // Batches inside batches, the same way.
    allowed(
        &project,
        &[EAST],
        &batch(vec![batch(vec![wall.clone()]), batch(vec![window.clone()])]),
    );
    let (message, _) = refused(
        &project,
        &[EAST],
        &batch(vec![batch(vec![wall.clone()]), batch(vec![lamp(GROUND, 6500.0, 3000.0)])]),
    );
    assert_eq!(message, format!("Step 2 of 2: Step 1 of 1: The new light would be {OUTSIDE}"));
    let (message, _) = refused(
        &project,
        &[EAST],
        &batch(vec![wall, batch(vec![window, Command::SetRoof { roof: project.roof.clone() }])]),
    );
    assert!(message.starts_with("Step 2 of 2: Step 2 of 2: Changing the roof"), "{message}");
}

#[test]
fn what_earlier_steps_made_is_in_reach_but_does_not_widen_the_area() {
    let mut doc = Document::new(house());
    let made = doc
        .apply(add_wall(p(7700.0, 1000.0), p(7700.0, 3000.0)), Origin::Ai)
        .unwrap()
        .diff
        .added;
    let (project, derived) = (doc.project(), doc.derived());
    let east = ids(&[EAST]);
    let window = opening(&made[0], OpeningType::Window, 1000.0);
    scope::check_staged(project, derived, &east, &made, &window).expect("a window on the made wall");
    assert!(scope::check(project, derived, &east, &window).is_err());
    // 700 mm west of the east wall is outside its area, though it is only
    // 400 mm from the made wall.
    let lamp = lamp(GROUND, 7300.0, 2000.0);
    let refusal = scope::check_staged(project, derived, &east, &made, &lamp).unwrap_err();
    assert_eq!(refusal.code(), "out_of_scope");

    // Selected ids an earlier step removed are skipped; the rest still holds.
    let with_gone = ids(&[EAST, "removed-earlier"]);
    scope::check_staged(project, derived, &with_gone, &made, &window).unwrap();
    assert!(matches!(
        scope::check(project, derived, &with_gone, &window),
        Err(CoreError::NotFound(id)) if id == "removed-earlier"
    ));
    // Nothing selected left and nothing made: nothing may change.
    let gone = ids(&["removed-earlier"]);
    assert_eq!(
        scope::check_staged(project, derived, &gone, &[], &lamp).unwrap_err().code(),
        "out_of_scope"
    );

    // The area follows the staged plan, so a selected element is replaced by
    // adding the new one first: once it is removed, its area is gone.
    let project = house();
    let queen = add(object("", GROUND, "bed-queen", 6900.0, 4800.0));
    let batch = |commands: Vec<Command>| Command::Batch {
        label: "Queen bed".into(),
        commands,
    };
    allowed(&project, &[BED], &batch(vec![queen.clone(), delete(BED)]));
    let (message, _) = refused(&project, &[BED], &batch(vec![delete(BED), queen]));
    assert!(
        message.starts_with("Step 2 of 2: Nothing of the selection")
            && message.ends_with("add the new one before removing the old."),
        "{message}"
    );
}

#[test]
fn an_unknown_scope_id_is_not_found_and_an_empty_scope_is_refused() {
    let project = house();
    let derived = compute_derived(&project);
    assert!(matches!(
        scope::check(&project, &derived, &ids(&[BEDROOM, "nope"]), &delete(BED)),
        Err(CoreError::NotFound(id)) if id == "nope"
    ));
    assert!(matches!(
        scope::validate(&project, &ids(&["nope"])),
        Err(CoreError::NotFound(id)) if id == "nope"
    ));
    let empty = scope::check(&project, &derived, &[], &delete(BED)).unwrap_err();
    assert_eq!(empty.code(), "out_of_scope");
    assert_eq!(scope::validate(&project, &[]).unwrap_err().code(), "out_of_scope");
    scope::validate(&project, &ids(&[BEDROOM, BED])).unwrap();
}

#[test]
fn what_the_engine_changes_as_a_consequence_is_allowed() {
    let mut doc = Document::new(house());
    // Moving the east wall stretches the south and north walls, which it does
    // not reach, and the south dimension follows its corner (D12).
    let push = Command::MoveElements {
        ids: ids(&[EAST]),
        delta: p(1000.0, 0.0),
        stretch_connected: true,
    };
    allowed(doc.project(), &[EAST], &push);
    refused(doc.project(), &[EAST], &delete(SOUTH));
    let applied = doc.apply(push, Origin::Ai).unwrap();
    assert!(applied.diff.modified.contains(&SOUTH.to_string()));
    assert!(applied.diff.modified.contains(&NORTH.to_string()));
    assert_eq!(wall(&applied.state.project, SOUTH).end, p(9000.0, 0.0));
    assert_eq!(dimension(&applied.state.project, SOUTH_DIMENSION).b, p(9000.0, 0.0));

    // Deleting the living room lamp drops it from the bedroom switch's links.
    allowed(doc.project(), &[LIVING], &delete("lamp-living"));
    refused(doc.project(), &[LIVING], &delete("switch-bed"));
    let applied = doc.apply(delete("lamp-living"), Origin::Ai).unwrap();
    assert!(applied.diff.modified.contains(&"switch-bed".to_string()));

    // A resize moves walls the user never selected, which is what they asked.
    let resize = Command::ResizeRoom {
        room_id: BEDROOM.into(),
        side: Side::North,
        delta_mm: 500.0,
    };
    allowed(doc.project(), &[BEDROOM], &resize);
    doc.apply(resize, Origin::Ai).unwrap();
}

// -------------------------------------------------------------- describe

#[test]
fn describe_says_what_the_selection_holds() {
    let project = templates::sample_bungalow();
    let derived = compute_derived(&project);
    let describe = |list: &[&str]| scope::describe(&project, &derived, &ids(list));
    assert_eq!(
        describe(&[BEDROOM]),
        "Room Bedroom with its 4 walls, 1 door, 1 window and 1 object, on Ground Floor"
    );
    assert_eq!(
        describe(&[LIVING]),
        "Room Living / Dining with its 4 walls, 2 doors and 2 windows, on Ground Floor"
    );
    assert_eq!(
        describe(&[SOUTH]),
        "Wall 8000 mm with its 1 door and 1 window, on Ground Floor"
    );
    assert_eq!(
        describe(&[BED, BEDROOM_WINDOW, BED]),
        "Double bed; Window 1200 x 1200, on Ground Floor"
    );
    assert_eq!(describe(&[EXTERIOR_CAMERA]), "Camera Exterior corner");
    assert_eq!(describe(&["nope"]), "nothing");
    assert_eq!(
        describe(&[WEST, EAST, NORTH, SOUTH, PARTITION, BED, BEDROOM_DOOR]),
        "Wall 6000 mm; Wall 6000 mm with its 1 window; Wall 8000 mm with its 1 window; \
         Wall 8000 mm with its 1 door and 1 window; Wall 6000 mm with its 1 door; \
         2 more elements, on Ground Floor"
    );

    let project = house();
    let derived = compute_derived(&project);
    assert_eq!(
        scope::describe(&project, &derived, &ids(&[BEDROOM, "lamp-up"])),
        "Room Bedroom with its 4 walls, 1 door, 1 window, 2 objects, 1 column, 1 stair, \
         1 pipe, 1 note, 1 dimension and 1 camera; Ceiling light, on Ground Floor and Second Floor"
    );
}
