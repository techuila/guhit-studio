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
    }));
    project.elements = elements;
    project
}
