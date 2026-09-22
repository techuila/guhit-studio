//! Shared test input: the golden fixture and a richer project built on it.
#![allow(dead_code)]

use std::path::PathBuf;

use guhit_model::*;

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn samples_dir() -> PathBuf {
    let dir = workspace_root().join("target/export-samples");
    std::fs::create_dir_all(&dir).expect("create samples dir");
    dir
}

pub fn fixture() -> DocState {
    let path = workspace_root().join("fixtures/sample-bungalow.docstate.json");
    let json = std::fs::read_to_string(&path).expect("read fixture");
    serde_json::from_str(&json).expect("fixture parses as DocState")
}

pub fn options(paper: PaperSize, orientation: Orientation) -> PlanExportOptions {
    PlanExportOptions {
        level_id: None,
        paper,
        orientation,
        scale_denominator: None,
        show_dimensions: true,
        show_room_labels: true,
        show_assets: true,
        title_block: true,
    }
}

pub fn level_id(state: &DocState) -> Id {
    state.project.levels[0].id.clone()
}

pub fn swing_leaf_count(project: &Project) -> usize {
    project
        .elements
        .iter()
        .map(|e| match e {
            Element::Opening(o) if o.opening_type == OpeningType::Door => match o.style {
                OpeningStyle::SwingDouble => 2,
                OpeningStyle::Sliding | OpeningStyle::Fixed => 0,
                _ => 1,
            },
            _ => 0,
        })
        .sum()
}

fn p(x: f64, y: f64) -> Point {
    Point { x, y }
}

/// A self-contained project with every element kind and symbol variant the exporter draws:
/// an angled wall, flipped, double and sliding doors, columns, a stair,
/// annotations, vertical and angled dimensions. Only the project shell
/// (settings, level, layers) comes from the fixture.
pub fn rich() -> DocState {
    let mut s = fixture();
    let lvl = level_id(&s);
    s.project.name = "Cruz Residence, Two Bedroom Bungalow".into();
    s.project.settings.designer = "Ar. Maria Santos".into();
    s.project.settings.north_angle_deg = 25.0;
    let wall = |id: &str, a: Point, b: Point, t: f64| {
        Element::Wall(Wall {
            id: id.into(),
            level_id: lvl.clone(),
            start: a,
            end: b,
            thickness_mm: t,
            height_mm: None,
            material_id: None,
        })
    };
    let opening = |id: &str,
                   wall_id: &str,
                   ty: OpeningType,
                   style: OpeningStyle,
                   offset: f64,
                   width: f64,
                   flip_side: bool,
                   flip_hinge: bool| {
        Element::Opening(Opening {
            id: id.into(),
            wall_id: wall_id.into(),
            opening_type: ty,
            style,
            offset_mm: offset,
            width_mm: width,
            height_mm: 2100.0,
            sill_mm: 0.0,
            flip_side,
            flip_hinge,
            material_id: None,
        })
    };
    // Own shell and rooms, so this project does not depend on fixture numbers.
    // Walls carry no derived geometry, which exercises the fallback outline.
    s.project.elements.clear();
    s.derived = Derived::default();
    let room = |id: &str, name: &str, usage: RoomUsage, seed: Point| {
        Element::Room(Room {
            id: id.into(),
            level_id: lvl.clone(),
            name: name.into(),
            usage,
            seed,
            floor_material_id: None,
            auto_named: false,
        })
    };
    let room_geo = |id: &str, x0: f64, y0: f64, x1: f64, y1: f64| RoomGeometry {
        room_id: id.into(),
        polygon: vec![p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)],
        centerline_polygon: vec![],
        area_mm2: (x1 - x0) * (y1 - y0),
        perimeter_mm: 2.0 * ((x1 - x0) + (y1 - y0)),
        label_point: p((x0 + x1) / 2.0, (y0 + y1) / 2.0),
        wall_ids: vec![],
    };
    s.derived.rooms = vec![
        room_geo("r-living", 75.0, 75.0, 4950.0, 5925.0),
        room_geo("r-bed", 5050.0, 1950.0, 7925.0, 5925.0),
        room_geo("r-bath", 5050.0, 75.0, 7925.0, 1850.0),
    ];
    let first_wall = "w-front".to_string();
    let top_wall = "w-rear".to_string();
    s.project.elements = vec![
        wall("w-front", p(0.0, 0.0), p(8000.0, 0.0), 150.0),
        wall("w-right", p(8000.0, 0.0), p(8000.0, 6000.0), 150.0),
        wall("w-rear", p(8000.0, 6000.0), p(0.0, 6000.0), 150.0),
        wall("w-left", p(0.0, 6000.0), p(0.0, 0.0), 150.0),
        wall("w-mid", p(5000.0, 0.0), p(5000.0, 6000.0), 100.0),
        opening("o-main", "w-front", OpeningType::Door, OpeningStyle::SwingSingle, 1500.0, 900.0, false, false),
        opening("o-bed", "w-mid", OpeningType::Door, OpeningStyle::SwingSingle, 3000.0, 800.0, true, false),
        opening("o-win1", "w-left", OpeningType::Window, OpeningStyle::Sliding, 3000.0, 1500.0, false, false),
        opening("o-win2", "w-right", OpeningType::Window, OpeningStyle::Casement, 4000.0, 1200.0, false, false),
        opening("o-win3", "w-right", OpeningType::Window, OpeningStyle::Jalousie, 1000.0, 600.0, false, false),
        room("r-living", "Living / Dining", RoomUsage::Living, p(2500.0, 3000.0)),
        room("r-bed", "Bedroom", RoomUsage::Bedroom, p(6500.0, 4000.0)),
        room("r-bath", "T&B", RoomUsage::Bathroom, p(6500.0, 1000.0)),
        Element::Asset(Asset {
            id: "as-bed".into(),
            level_id: lvl.clone(),
            catalog_key: "bed-double".into(),
            name: "Double bed".into(),
            category: AssetCategory::Furniture,
            position: p(7100.0, 4200.0),
            rotation_deg: 0.0,
            width_mm: 1370.0,
            depth_mm: 1900.0,
            height_mm: 500.0,
            elevation_mm: 0.0,
        }),
        Element::Dimension(Dimension {
            id: "d-front".into(),
            level_id: lvl.clone(),
            a: p(0.0, 0.0),
            b: p(8000.0, 0.0),
            offset_mm: -3200.0,
            text_override: None,
        }),
    ];

    let mut add = vec![
        // Bathroom wall inside the bedroom, door flipped both ways.
        wall("w-bath", p(5000.0, 1900.0), p(8000.0, 1900.0), 100.0),
        opening("o-bath", "w-bath", OpeningType::Door, OpeningStyle::SwingSingle, 800.0, 700.0, true, true),
        // Angled garden wall with a window.
        wall("w-angled", p(8000.0, 6000.0), p(10200.0, 8200.0), 150.0),
        opening("o-angled", "w-angled", OpeningType::Window, OpeningStyle::Fixed, 1500.0, 1200.0, false, false),
        // Double door on the rear wall, sliding door on the front wall.
        opening("o-double", &top_wall, OpeningType::Door, OpeningStyle::SwingDouble, 5500.0, 1600.0, true, false),
        opening("o-slide", &first_wall, OpeningType::Door, OpeningStyle::Sliding, 3600.0, 1800.0, false, false),
        Element::Column(Column {
            id: "c-rect".into(),
            level_id: lvl.clone(),
            center: p(200.0, -2200.0),
            shape: ColumnShape::Rect,
            width_mm: 300.0,
            depth_mm: 300.0,
            rotation_deg: 0.0,
            material_id: None,
        }),
        Element::Column(Column {
            id: "c-round".into(),
            level_id: lvl.clone(),
            center: p(3000.0, -2200.0),
            shape: ColumnShape::Round,
            width_mm: 300.0,
            depth_mm: 300.0,
            rotation_deg: 0.0,
            material_id: None,
        }),
        Element::Stair(Stair {
            id: "s-1".into(),
            level_id: lvl.clone(),
            origin: p(9200.0, 300.0),
            rotation_deg: 0.0,
            width_mm: 1000.0,
            run_mm: 2750.0,
            riser_count: 11,
        }),
        Element::Annotation(Annotation {
            id: "a-porch".into(),
            level_id: lvl.clone(),
            position: p(900.0, -1300.0),
            text: "PORCH\nopen to sky".into(),
            size_mm: 250.0,
            rotation_deg: 0.0,
        }),
        Element::Dimension(Dimension {
            id: "d-vert".into(),
            level_id: lvl.clone(),
            a: p(0.0, 0.0),
            b: p(0.0, 6000.0),
            offset_mm: 900.0,
            text_override: None,
        }),
        Element::Dimension(Dimension {
            id: "d-angled".into(),
            level_id: lvl.clone(),
            a: p(8000.0, 6000.0),
            b: p(10200.0, 8200.0),
            offset_mm: 700.0,
            text_override: None,
        }),
        Element::Asset(Asset {
            id: "as-small".into(),
            level_id: lvl.clone(),
            catalog_key: "side-table".into(),
            name: "Side table".into(),
            category: AssetCategory::Furniture,
            position: p(5500.0, 5500.0),
            rotation_deg: 30.0,
            width_mm: 450.0,
            depth_mm: 450.0,
            height_mm: 500.0,
            elevation_mm: 0.0,
        }),
    ];
    s.project.elements.append(&mut add);
    s
}
