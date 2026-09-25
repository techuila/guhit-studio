//! Starter projects. PUBLIC API IS CONTRACT: `guhit-app` calls these.

use guhit_model::*;

fn p(x: f64, y: f64) -> Point {
    Point { x, y }
}

/// The two-room sample house, same content as the golden fixture
/// `fixtures/sample-bungalow.docstate.json`. Ids and timestamps are fixed so
/// the fixture is stable.
pub fn sample_bungalow() -> Project {
    let mut project = defaults::new_project("Sample Bungalow");
    project.id = "00000000-0000-4000-8000-000000000001".into();
    project.levels[0].id = "00000000-0000-4000-8000-0000000000a1".into();
    project.created_at = "2026-09-22T00:00:00Z".into();
    project.updated_at = "2026-09-22T00:00:00Z".into();
    project.settings.client_name = "Sample Client".into();
    project.settings.location = "Quezon City".into();
    project.roof.kind = RoofKind::Gable;
    let level = project.levels[0].id.clone();

    let wall = |n: u32, a: Point, b: Point, t: f64| {
        Element::Wall(Wall {
            id: format!("00000000-0000-4000-8000-00000000010{n}"),
            level_id: level.clone(),
            start: a,
            end: b,
            thickness_mm: t,
            height_mm: None,
            material_id: Some(defaults::MAT_WALL_DEFAULT.into()),
        })
    };
    // 8.0 x 6.0 m shell on centerlines, one partition at x = 5000.
    let mut elements = vec![
        wall(1, p(0.0, 0.0), p(8000.0, 0.0), 150.0),
        wall(2, p(8000.0, 0.0), p(8000.0, 6000.0), 150.0),
        wall(3, p(8000.0, 6000.0), p(0.0, 6000.0), 150.0),
        wall(4, p(0.0, 6000.0), p(0.0, 0.0), 150.0),
        wall(5, p(5000.0, 0.0), p(5000.0, 6000.0), 100.0),
    ];
    #[allow(clippy::too_many_arguments)]
    let opening = |n: u32,
                   wall_n: u32,
                   ty: OpeningType,
                   style: OpeningStyle,
                   offset: f64,
                   w: f64,
                   h: f64,
                   sill: f64| {
        Element::Opening(Opening {
            id: format!("00000000-0000-4000-8000-00000000020{n}"),
            wall_id: format!("00000000-0000-4000-8000-00000000010{wall_n}"),
            opening_type: ty,
            style,
            offset_mm: offset,
            width_mm: w,
            height_mm: h,
            sill_mm: sill,
            flip_side: false,
            flip_hinge: false,
            material_id: None,
        })
    };
    elements.extend([
        opening(
            1,
            1,
            OpeningType::Door,
            OpeningStyle::SwingSingle,
            1500.0,
            900.0,
            2100.0,
            0.0,
        ),
        opening(
            2,
            5,
            OpeningType::Door,
            OpeningStyle::SwingSingle,
            3000.0,
            800.0,
            2100.0,
            0.0,
        ),
        opening(
            3,
            1,
            OpeningType::Window,
            OpeningStyle::Sliding,
            3500.0,
            1500.0,
            1200.0,
            900.0,
        ),
        opening(
            4,
            2,
            OpeningType::Window,
            OpeningStyle::Casement,
            3000.0,
            1200.0,
            1200.0,
            900.0,
        ),
        opening(
            5,
            3,
            OpeningType::Window,
            OpeningStyle::Jalousie,
            5500.0,
            1200.0,
            1200.0,
            900.0,
        ),
    ]);
    let room = |n: u32, name: &str, usage: RoomUsage, seed: Point| {
        Element::Room(Room {
            id: format!("00000000-0000-4000-8000-00000000030{n}"),
            level_id: level.clone(),
            name: name.into(),
            usage,
            seed,
            floor_material_id: Some(defaults::MAT_FLOOR_DEFAULT.into()),
            auto_named: false,
        })
    };
    elements.extend([
        room(1, "Living / Dining", RoomUsage::Living, p(2500.0, 3000.0)),
        room(2, "Bedroom", RoomUsage::Bedroom, p(6500.0, 3000.0)),
    ]);
    elements.push(Element::Asset(Asset {
        id: "00000000-0000-4000-8000-000000000401".into(),
        level_id: level.clone(),
        catalog_key: "bed-double".into(),
        name: "Double bed".into(),
        category: AssetCategory::Furniture,
        position: p(6900.0, 4800.0),
        rotation_deg: 0.0,
        width_mm: 1370.0,
        depth_mm: 1900.0,
        height_mm: 500.0,
        elevation_mm: 0.0,
        light: None,
        links: vec![],
        circuit: String::new(),
    }));
    elements.push(Element::Dimension(Dimension {
        id: "00000000-0000-4000-8000-000000000501".into(),
        level_id: level.clone(),
        a: p(0.0, 0.0),
        b: p(8000.0, 0.0),
        offset_mm: -900.0,
        text_override: None,
    }));
    elements.push(Element::Camera(Camera {
        id: "00000000-0000-4000-8000-000000000601".into(),
        name: "Exterior corner".into(),
        preset: CameraPreset::ExteriorCorner,
        position: Vec3 {
            x: -5000.0,
            y: -7000.0,
            z: 4500.0,
        },
        target: Vec3 {
            x: 4000.0,
            y: 3000.0,
            z: 1200.0,
        },
        fov_deg: 50.0,
        light: None,
    }));
    project.elements = elements;
    project
}

fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3 { x, y, z }
}

/// "Bungalow with services" (template id `plumbing-demo`): the house of the
/// approved plumbing walkthrough concept, with its electrical and aircon
/// services. An 8.0 x 6.0 m bungalow with a T&B, six 200 x 200 columns on the
/// grid, a gable roof, fixtures from the catalog, the 16 plumbing runs of the
/// concept, two storm downspouts, lights, switches, outlets, a panelboard, a
/// smoke detector and a split aircon for the bedroom with its line set and
/// condensate drain. Ids and timestamps are fixed so
/// `fixtures/plumbing-demo.docstate.json` is stable.
///
/// It is drawn to show the checks: the heater feed crosses the T&B door, the
/// cold water chase runs through a column, the kitchen sink waste falls too
/// little, seven pipes need a sleeve or flashing and the aircon lines one
/// core hole, the T&B light has no switch yet, and the line set runs past the
/// 3 m a standard installation includes. Nothing else is flagged.
pub fn plumbing_demo() -> Project {
    let mut project = defaults::new_project("Bungalow with services");
    project.id = "00000000-0000-4000-8000-000000000002".into();
    project.levels[0].id = "00000000-0000-4000-8000-0000000000a2".into();
    project.created_at = "2026-09-23T00:00:00Z".into();
    project.updated_at = "2026-09-23T00:00:00Z".into();
    project.settings.client_name = "Sample Client".into();
    project.settings.location = "Quezon City".into();
    project.roof = Roof {
        kind: RoofKind::Gable,
        pitch_deg: 20.0,
        overhang_mm: 600.0,
        thickness_mm: 100.0,
        ridge_axis: Axis::X,
        material_id: Some(defaults::MAT_ROOF_DEFAULT.into()),
    };
    let level = project.levels[0].id.clone();
    let id = |block: u32, n: u32| format!("00000000-0000-4000-8000-0000000{block:03}{n:02}");

    // Walls on centerlines: the 8.0 x 6.0 m shell, the bedroom partition at
    // x = 5000 and the two T&B walls.
    let walls = [
        (p(0.0, 0.0), p(8000.0, 0.0), 150.0),
        (p(8000.0, 0.0), p(8000.0, 6000.0), 150.0),
        (p(8000.0, 6000.0), p(0.0, 6000.0), 150.0),
        (p(0.0, 6000.0), p(0.0, 0.0), 150.0),
        (p(5000.0, 0.0), p(5000.0, 6000.0), 100.0),
        (p(6000.0, 4200.0), p(8000.0, 4200.0), 100.0),
        (p(6000.0, 4200.0), p(6000.0, 6000.0), 100.0),
    ];
    let mut elements: Vec<Element> = walls
        .iter()
        .enumerate()
        .map(|(i, (a, b, t))| {
            Element::Wall(Wall {
                id: id(110, i as u32 + 1),
                level_id: level.clone(),
                start: *a,
                end: *b,
                thickness_mm: *t,
                height_mm: None,
                material_id: Some(defaults::MAT_WALL_DEFAULT.into()),
            })
        })
        .collect();

    // (wall, type, style, offset, width, height, sill, flip_side)
    let openings = [
        (
            1,
            OpeningType::Door,
            OpeningStyle::SwingSingle,
            1500.0,
            900.0,
            2100.0,
            0.0,
            false,
        ),
        (
            1,
            OpeningType::Window,
            OpeningStyle::Sliding,
            3500.0,
            1500.0,
            1200.0,
            900.0,
            false,
        ),
        (
            2,
            OpeningType::Window,
            OpeningStyle::Casement,
            2100.0,
            1200.0,
            1200.0,
            900.0,
            false,
        ),
        (
            2,
            OpeningType::Window,
            OpeningStyle::Jalousie,
            4950.0,
            600.0,
            600.0,
            1500.0,
            false,
        ),
        (
            3,
            OpeningType::Window,
            OpeningStyle::Sliding,
            5800.0,
            1000.0,
            1000.0,
            1050.0,
            false,
        ),
        (
            4,
            OpeningType::Window,
            OpeningStyle::Sliding,
            3000.0,
            1500.0,
            1200.0,
            900.0,
            false,
        ),
        // The bedroom door swings into the bedroom, east of the partition.
        (
            5,
            OpeningType::Door,
            OpeningStyle::SwingSingle,
            2400.0,
            800.0,
            2100.0,
            0.0,
            true,
        ),
        // The T&B door swings into the T&B, north of its wall.
        (
            6,
            OpeningType::Door,
            OpeningStyle::SwingSingle,
            500.0,
            700.0,
            2100.0,
            0.0,
            false,
        ),
    ];
    for (i, (wall, ty, style, offset, w, h, sill, flip_side)) in openings.iter().enumerate() {
        elements.push(Element::Opening(Opening {
            id: id(120, i as u32 + 1),
            wall_id: id(110, *wall),
            opening_type: *ty,
            style: *style,
            offset_mm: *offset,
            width_mm: *w,
            height_mm: *h,
            sill_mm: *sill,
            flip_side: *flip_side,
            flip_hinge: false,
            material_id: None,
        }));
    }

    // One room per closed face (DECISIONS D7): the living area and kitchen
    // share one face, so they are one room.
    let rooms = [
        ("Living / Kitchen", RoomUsage::Living, p(2500.0, 2300.0)),
        ("Bedroom", RoomUsage::Bedroom, p(6500.0, 2700.0)),
        ("T&B", RoomUsage::Bathroom, p(6600.0, 4750.0)),
    ];
    for (i, (name, usage, seed)) in rooms.iter().enumerate() {
        elements.push(Element::Room(Room {
            id: id(130, i as u32 + 1),
            level_id: level.clone(),
            name: (*name).into(),
            usage: *usage,
            seed: *seed,
            floor_material_id: Some(defaults::MAT_FLOOR_DEFAULT.into()),
            auto_named: false,
        }));
    }

    // Columns at grid A to C, 1 to 2.
    let grid = [
        (0.0, 0.0),
        (5000.0, 0.0),
        (8000.0, 0.0),
        (0.0, 6000.0),
        (5000.0, 6000.0),
        (8000.0, 6000.0),
    ];
    for (i, (x, y)) in grid.iter().enumerate() {
        elements.push(Element::Column(Column {
            id: id(140, i as u32 + 1),
            level_id: level.clone(),
            center: p(*x, *y),
            shape: ColumnShape::Rect,
            width_mm: 200.0,
            depth_mm: 200.0,
            rotation_deg: 0.0,
            material_id: Some(defaults::MAT_WALL_DEFAULT.into()),
        }));
    }

    // Fixtures and furniture from the catalog, at their catalog sizes. Local
    // +y is the back, so the counters, range, lavatory, WC and shower face
    // south from the north wall, the sofa backs onto the west wall and the bed
    // head is on the south wall. The washer stands outside, against the north
    // wall. The water heater, the meter and the septic tank are left out; the
    // runs show where they go.
    let catalog = defaults::asset_catalog();
    let object = |block: u32, n: u32, key: &str, position: Point, rotation: f64, links: &[Id]| {
        let item = catalog
            .iter()
            .find(|c| c.key == key)
            .expect("the demo only uses catalog items");
        Element::Asset(Asset {
            id: id(block, n),
            level_id: level.clone(),
            catalog_key: item.key.clone(),
            name: item.name.clone(),
            category: item.category,
            position,
            rotation_deg: rotation,
            width_mm: item.width_mm,
            depth_mm: item.depth_mm,
            height_mm: item.height_mm,
            elevation_mm: item.elevation_mm,
            light: item.light,
            links: links.to_vec(),
            circuit: String::new(),
        })
    };
    let assets = [
        ("kitchen-sink", p(2000.0, 5625.0), 0.0),
        ("lavatory", p(6300.0, 5715.0), 0.0),
        ("wc", p(6780.0, 5575.0), 0.0),
        ("shower", p(7475.0, 5475.0), 0.0),
        ("sofa-3", p(550.0, 2600.0), 90.0),
        ("dining-4", p(3300.0, 3900.0), 0.0),
        ("bed-double", p(6695.0, 1035.0), 180.0),
        ("range", p(1000.0, 5625.0), 0.0),
        ("kitchen-counter", p(3500.0, 5625.0), 0.0),
        ("washing-machine", p(3500.0, 6380.0), 180.0),
    ];
    for (i, (key, position, rotation)) in assets.iter().enumerate() {
        elements.push(object(150, i as u32 + 1, key, *position, *rotation, &[]));
    }
    let range = id(150, 8);
    let washer = id(150, 10);

    // Lighting, power and aircon (docs/CONTRACT.md, "Devices, fixtures and
    // links"). Wall devices sit with their back on a wall face: the inner
    // faces are at x = 75, x = 4950 and 5050 (the partition), x = 7925,
    // y = 75 and y = 5925, the outer ones at y = -75 and y = 6075. Switches
    // are 200 mm from the latch side of their door. The bedroom light has two
    // switches, a 3-way: one by the door, one by the bed. The T&B light has
    // no switch yet.
    let dev = |n: u32| id(180, n);
    let devices: [(u32, &str, Point, f64, Vec<Id>); 22] = [
        // Lights: one per room, the dining pendant, the light by the front door.
        (1, "light-ceiling", p(2500.0, 1800.0), 0.0, vec![]),
        (2, "light-pendant", p(3300.0, 3900.0), 0.0, vec![]),
        (3, "light-ceiling", p(6500.0, 2400.0), 0.0, vec![]),
        (4, "light-ceiling", p(6800.0, 4700.0), 0.0, vec![]),
        (5, "light-outdoor", p(800.0, -150.0), 0.0, vec![]),
        // Switches. The front door latches at x = 1950, the bedroom door at
        // y = 2800.
        (6, "switch-2", p(2150.0, 95.0), 180.0, vec![dev(1), dev(5)]),
        (7, "switch-1", p(4930.0, 3000.0), 270.0, vec![dev(2)]),
        (8, "switch-1", p(5070.0, 3000.0), 90.0, vec![dev(3)]),
        (9, "switch-1", p(5850.0, 95.0), 180.0, vec![dev(3)]),
        // Outlets: two in the living area, one each side of the bed, two
        // over the kitchen counter, the range and washer outlets.
        (10, "outlet-duplex", p(95.0, 1300.0), 90.0, vec![]),
        (11, "outlet-duplex", p(4600.0, 95.0), 180.0, vec![]),
        (12, "outlet-duplex", p(5650.0, 95.0), 180.0, vec![]),
        (13, "outlet-duplex", p(7650.0, 95.0), 180.0, vec![]),
        (14, "outlet-counter", p(3000.0, 5905.0), 0.0, vec![]),
        (15, "outlet-counter", p(4100.0, 5905.0), 0.0, vec![]),
        (16, "outlet-spo", p(600.0, 5905.0), 0.0, vec![range.clone()]),
        (17, "outlet-spo", p(4000.0, 6095.0), 180.0, vec![washer.clone()]),
        (18, "panelboard", p(125.0, 700.0), 90.0, vec![]),
        (19, "smoke-detector", p(3700.0, 2200.0), 0.0, vec![]),
        // The bedroom split aircon: the indoor unit on the east wall, the
        // outdoor unit outside it, 320 mm off the wall, and its outlet.
        (20, "aircon-indoor-1hp", p(7810.0, 3400.0), 270.0, vec![]),
        (21, "aircon-outdoor-1hp", p(8550.0, 1800.0), 90.0, vec![]),
        (22, "outlet-aircon", p(7905.0, 3950.0), 270.0, vec![dev(20)]),
    ];
    for (n, key, position, rotation, links) in devices.iter() {
        elements.push(object(180, *n, key, *position, *rotation, links));
    }

    // Every run lists its points in flow order: supply from the source,
    // drainage toward the outlet. Heights are above the floor.
    use PipeMaterial::{Ppr, Upvc};
    use PipeSystem::{ColdWater, Drainage, HotWater, Vent};
    let runs: [(&str, PipeSystem, PipeMaterial, f64, Vec<Vec3>); 16] = [
        (
            "Service line",
            ColdWater,
            Ppr,
            25.0,
            vec![
                v(600.0, -1800.0, 0.0),
                v(600.0, -1800.0, -300.0),
                v(600.0, 5850.0, -300.0),
                v(2000.0, 5850.0, -300.0),
                v(2000.0, 6000.0, -300.0),
                v(2000.0, 6000.0, 300.0),
            ],
        ),
        (
            "Cold water chase",
            ColdWater,
            Ppr,
            20.0,
            vec![v(2000.0, 6000.0, 300.0), v(7450.0, 6000.0, 300.0)],
        ),
        (
            "Sink supply",
            ColdWater,
            Ppr,
            20.0,
            vec![
                v(2000.0, 6000.0, 300.0),
                v(2000.0, 6000.0, 550.0),
                v(2000.0, 5925.0, 550.0),
            ],
        ),
        (
            "Lavatory supply",
            ColdWater,
            Ppr,
            20.0,
            vec![
                v(6300.0, 6000.0, 300.0),
                v(6300.0, 6000.0, 550.0),
                v(6300.0, 5925.0, 550.0),
            ],
        ),
        (
            "Water closet supply",
            ColdWater,
            Ppr,
            20.0,
            vec![v(6780.0, 6000.0, 300.0), v(6780.0, 5925.0, 300.0)],
        ),
        (
            "Shower cold",
            ColdWater,
            Ppr,
            20.0,
            vec![
                v(7450.0, 6000.0, 300.0),
                v(7450.0, 6000.0, 1100.0),
                v(7450.0, 5925.0, 1100.0),
            ],
        ),
        (
            "Heater feed",
            ColdWater,
            Ppr,
            20.0,
            vec![
                v(6000.0, 6000.0, 300.0),
                v(6000.0, 4200.0, 300.0),
                v(6000.0, 4200.0, 1500.0),
                v(7500.0, 4200.0, 1500.0),
                v(7500.0, 4400.0, 1500.0),
                v(7500.0, 4400.0, 1600.0),
            ],
        ),
        (
            "Hot water to the shower",
            HotWater,
            Ppr,
            20.0,
            vec![
                v(7550.0, 4400.0, 2200.0),
                v(7550.0, 4400.0, 2450.0),
                v(7550.0, 6000.0, 2450.0),
                v(7550.0, 6000.0, 1100.0),
                v(7550.0, 5925.0, 1100.0),
            ],
        ),
        (
            "Hot water to the lavatory",
            HotWater,
            Ppr,
            20.0,
            vec![
                v(7550.0, 5100.0, 2450.0),
                v(6230.0, 5100.0, 2450.0),
                v(6230.0, 6000.0, 2450.0),
                v(6230.0, 6000.0, 550.0),
                v(6230.0, 5925.0, 550.0),
            ],
        ),
        (
            "Building drain",
            Drainage,
            Upvc,
            100.0,
            vec![
                v(6300.0, 5650.0, -420.0),
                v(6780.0, 5650.0, -430.0),
                v(7500.0, 5650.0, -444.0),
                v(7800.0, 5650.0, -450.0),
                v(8600.0, 5650.0, -466.0),
            ],
        ),
        (
            "Water closet drain",
            Drainage,
            Upvc,
            100.0,
            vec![v(6780.0, 5650.0, 20.0), v(6780.0, 5650.0, -430.0)],
        ),
        (
            "Lavatory waste",
            Drainage,
            Upvc,
            50.0,
            vec![
                v(6300.0, 5760.0, 650.0),
                v(6300.0, 5760.0, -410.0),
                v(6300.0, 5650.0, -420.0),
            ],
        ),
        (
            "Shower drain",
            Drainage,
            Upvc,
            50.0,
            vec![
                v(7500.0, 5500.0, 20.0),
                v(7500.0, 5500.0, -420.0),
                v(7500.0, 5650.0, -444.0),
            ],
        ),
        (
            "Kitchen sink waste",
            Drainage,
            Upvc,
            50.0,
            vec![
                v(2000.0, 5700.0, 850.0),
                v(2000.0, 5700.0, -300.0),
                v(6300.0, 5700.0, -334.0),
                v(6300.0, 5650.0, -420.0),
            ],
        ),
        (
            "Septic tank outlet",
            Drainage,
            Upvc,
            100.0,
            vec![v(10400.0, 4300.0, -520.0), v(11600.0, 4300.0, -545.0)],
        ),
        (
            "Vent stack",
            Vent,
            Upvc,
            50.0,
            vec![
                v(7800.0, 5650.0, -450.0),
                v(7800.0, 6000.0, -457.0),
                v(7800.0, 6000.0, 3450.0),
            ],
        ),
    ];
    for (i, (name, system, material, diameter, points)) in runs.into_iter().enumerate() {
        elements.push(Element::Pipe(Pipe {
            id: id(160, i as u32 + 1),
            level_id: level.clone(),
            system,
            material,
            diameter_mm: diameter,
            points,
            name: name.into(),
        }));
    }

    // Service runs. The downspouts come down the south and north faces from
    // the gutters under the eaves and fall 1.5 percent to the yard. The line
    // set leaves the back of the indoor unit, goes through the east wall 7 mm
    // down to the outside, drops and runs to the outdoor unit; the condensate
    // drain goes through the same core hole and falls to the ground outside.
    use PipeMaterial::{Copper, Pvc};
    use PipeSystem::{Condensate, Refrigerant, Storm};
    let services: [(&str, PipeSystem, PipeMaterial, f64, Vec<Vec3>); 4] = [
        (
            "Downspout, front",
            Storm,
            Upvc,
            100.0,
            vec![
                v(7700.0, -200.0, 2900.0),
                v(7700.0, -200.0, -300.0),
                v(7700.0, -1400.0, -318.0),
            ],
        ),
        (
            "Downspout, back",
            Storm,
            Upvc,
            100.0,
            vec![
                v(300.0, 6200.0, 2900.0),
                v(300.0, 6200.0, -300.0),
                v(300.0, 7400.0, -318.0),
            ],
        ),
        (
            "Bedroom line set",
            Refrigerant,
            Copper,
            9.52,
            vec![
                v(7880.0, 3100.0, 2400.0),
                v(8250.0, 3100.0, 2393.0),
                v(8250.0, 3100.0, 350.0),
                v(8250.0, 2300.0, 350.0),
                v(8450.0, 2300.0, 350.0),
            ],
        ),
        (
            "Bedroom aircon condensate",
            Condensate,
            Pvc,
            20.0,
            vec![
                v(7880.0, 3200.0, 2310.0),
                v(8250.0, 3200.0, 2280.0),
                v(8250.0, 3200.0, 150.0),
            ],
        ),
    ];
    for (i, (name, system, material, diameter, points)) in services.into_iter().enumerate() {
        elements.push(Element::Pipe(Pipe {
            id: id(190, i as u32 + 1),
            level_id: level.clone(),
            system,
            material,
            diameter_mm: diameter,
            points,
            name: name.into(),
        }));
    }

    elements.push(Element::Camera(Camera {
        id: id(170, 1),
        name: "Plumbing overview".into(),
        preset: CameraPreset::ExteriorCorner,
        position: v(12300.0, -6800.0, 7400.0),
        target: v(4600.0, 3300.0, 300.0),
        fov_deg: 42.0,
        light: None,
    }));
    project.elements = elements;
    project
}
