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
        show_pipes: true,
        sheet: SheetKind::Plan,
        review_page: false,
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
            light: None,
            links: vec![],
            circuit: String::new(),
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
            light: None,
            links: vec![],
            circuit: String::new(),
        }),
    ];
    s.project.elements.append(&mut add);
    s
}

/// One pipe run: id, system, material, size mm, name, and the points x, y, z
/// in flow order.
pub type Run = (&'static str, PipeSystem, PipeMaterial, f64, &'static str, &'static [(f64, f64, f64)]);

/// The sixteen pipe runs of the plumbing concept, on the fixture's bungalow.
/// Supply rises from under the slab into the north wall chase, drainage
/// falls to the septic tank, one vent stack leaves through the roof.
pub const RUNS: [Run; 16] = [
    ("p-cw-main", PipeSystem::ColdWater, PipeMaterial::Pe, 25.0, "Service line",
     &[(600.0, -1800.0, 0.0), (600.0, -1800.0, -300.0), (600.0, 5850.0, -300.0), (2000.0, 5850.0, -300.0), (2000.0, 6000.0, -300.0), (2000.0, 6000.0, 300.0)]),
    ("p-cw-chase", PipeSystem::ColdWater, PipeMaterial::Ppr, 20.0, "Cold water chase",
     &[(2000.0, 6000.0, 300.0), (7450.0, 6000.0, 300.0)]),
    ("p-cw-sink", PipeSystem::ColdWater, PipeMaterial::Ppr, 20.0, "Sink supply",
     &[(2000.0, 6000.0, 300.0), (2000.0, 6000.0, 550.0), (2000.0, 5925.0, 550.0)]),
    ("p-cw-lav", PipeSystem::ColdWater, PipeMaterial::Ppr, 20.0, "Lavatory supply",
     &[(6300.0, 6000.0, 300.0), (6300.0, 6000.0, 550.0), (6300.0, 5925.0, 550.0)]),
    ("p-cw-wc", PipeSystem::ColdWater, PipeMaterial::Ppr, 20.0, "",
     &[(6780.0, 6000.0, 300.0), (6780.0, 5925.0, 300.0)]),
    ("p-cw-shower", PipeSystem::ColdWater, PipeMaterial::Ppr, 20.0, "Shower cold",
     &[(7450.0, 6000.0, 300.0), (7450.0, 6000.0, 1100.0), (7450.0, 5925.0, 1100.0)]),
    ("p-cw-heater", PipeSystem::ColdWater, PipeMaterial::Ppr, 20.0, "Heater feed",
     &[(6000.0, 6000.0, 300.0), (6000.0, 4200.0, 300.0), (6000.0, 4200.0, 1500.0), (7500.0, 4200.0, 1500.0), (7500.0, 4400.0, 1500.0), (7500.0, 4400.0, 1600.0)]),
    ("p-hw-shower", PipeSystem::HotWater, PipeMaterial::Ppr, 20.0, "Hot water to the shower",
     &[(7550.0, 4400.0, 2200.0), (7550.0, 4400.0, 2450.0), (7550.0, 6000.0, 2450.0), (7550.0, 6000.0, 1100.0), (7550.0, 5925.0, 1100.0)]),
    ("p-hw-lav", PipeSystem::HotWater, PipeMaterial::Ppr, 20.0, "Hot water to the lavatory",
     &[(7550.0, 5100.0, 2450.0), (6230.0, 5100.0, 2450.0), (6230.0, 6000.0, 2450.0), (6230.0, 6000.0, 550.0), (6230.0, 5925.0, 550.0)]),
    ("p-dr-main", PipeSystem::Drainage, PipeMaterial::Upvc, 100.0, "Building drain",
     &[(6300.0, 5650.0, -420.0), (6780.0, 5650.0, -430.0), (7500.0, 5650.0, -444.0), (7800.0, 5650.0, -450.0), (8600.0, 5650.0, -466.0)]),
    ("p-dr-wc", PipeSystem::Drainage, PipeMaterial::Upvc, 100.0, "Water closet drain",
     &[(6780.0, 5650.0, 20.0), (6780.0, 5650.0, -430.0)]),
    ("p-dr-lav", PipeSystem::Drainage, PipeMaterial::Upvc, 50.0, "Lavatory waste",
     &[(6300.0, 5760.0, 650.0), (6300.0, 5760.0, -410.0), (6300.0, 5650.0, -420.0)]),
    ("p-dr-shower", PipeSystem::Drainage, PipeMaterial::Upvc, 50.0, "Shower drain",
     &[(7500.0, 5500.0, 20.0), (7500.0, 5500.0, -420.0), (7500.0, 5650.0, -444.0)]),
    ("p-dr-sink", PipeSystem::Drainage, PipeMaterial::Upvc, 50.0, "Kitchen sink waste",
     &[(2000.0, 5700.0, 850.0), (2000.0, 5700.0, -300.0), (6300.0, 5700.0, -334.0), (6300.0, 5650.0, -420.0)]),
    ("p-dr-out", PipeSystem::Drainage, PipeMaterial::Upvc, 100.0, "Septic tank outlet",
     &[(10400.0, 4300.0, -520.0), (11600.0, 4300.0, -545.0)]),
    ("p-vt-stack", PipeSystem::Vent, PipeMaterial::Upvc, 50.0, "Vent stack",
     &[(7800.0, 5650.0, -450.0), (7800.0, 6000.0, -457.0), (7800.0, 6000.0, 3450.0)]),
];

pub fn pipe(level: &str, run: &Run) -> Pipe {
    Pipe {
        id: run.0.into(),
        level_id: level.into(),
        system: run.1,
        material: run.2,
        diameter_mm: run.3,
        points: run.5.iter().map(|p| Vec3 { x: p.0, y: p.1, z: p.2 }).collect(),
        name: run.4.into(),
    }
}

/// The fixture without any pipe, whatever the fixture file holds: the plan a
/// pipe export is compared against.
pub fn plain() -> DocState {
    let mut s = fixture();
    s.project.elements.retain(|e| !matches!(e, Element::Pipe(_)));
    s
}

/// The fixture with the four pipe layers (as a migrated project has them)
/// and the sixteen concept runs on its ground floor. The water closet supply
/// has no name, so it exports as "Cold water 20 mm".
pub fn plumbing() -> DocState {
    let mut s = plain();
    let lvl = level_id(&s);
    for key in [LayerKey::ColdWater, LayerKey::HotWater, LayerKey::Drainage, LayerKey::Vent] {
        if !s.project.layers.iter().any(|l| l.key == key) {
            s.project.layers.push(Layer { key, visible: true, locked: false });
        }
    }
    for run in &RUNS {
        s.project.elements.push(Element::Pipe(pipe(&lvl, run)));
    }
    s
}

pub fn set_layer(s: &mut DocState, key: LayerKey, visible: bool) {
    match s.project.layers.iter_mut().find(|l| l.key == key) {
        Some(l) => l.visible = visible,
        None => s.project.layers.push(Layer { key, visible, locked: false }),
    }
}

// ------------------------------------------------------------ service sheets

/// One placed object: id, catalog key, x, y, rotation, circuit tag and the
/// ids it links. Size, elevation, category and light come from the catalog.
pub type Obj = (&'static str, &'static str, f64, f64, f64, &'static str, &'static [&'static str]);

/// Wall faces of the bungalow (walls 150 mm, the middle wall 100 mm): the
/// living room runs x 75..4950, the bedroom x 5050..7925, both y 75..5925.
/// The T&B added by `services()` takes x 6050..7925, y 4250..5925.
/// Rotation puts the object's back (+y) on its wall: 0 north wall, 90 west,
/// 180 south, 270 east.
pub const OBJECTS: [Obj; 35] = [
    // Living and dining.
    ("lt-liv-1", "light-ceiling", 1500.0, 2600.0, 0.0, "L1", &[]),
    ("lt-liv-2", "light-ceiling", 3500.0, 2600.0, 0.0, "L1", &[]),
    ("lt-din", "light-pendant", 3300.0, 4300.0, 0.0, "L1", &[]),
    ("sw-liv", "switch-2", 2185.0, 95.0, 180.0, "L1", &["lt-liv-1", "lt-liv-2"]),
    ("sw-din", "switch-1", 4930.0, 4400.0, 270.0, "L1", &["lt-din"]),
    ("lt-porch", "light-outdoor", 800.0, -150.0, 0.0, "L1", &[]),
    ("sw-porch", "switch-1", 2400.0, 95.0, 180.0, "L1", &["lt-porch"]),
    ("lamp-liv", "light-floor-lamp", 400.0, 5500.0, 0.0, "", &[]),
    ("co-liv-1", "outlet-duplex", 95.0, 1800.0, 90.0, "C1", &[]),
    ("co-liv-2", "outlet-duplex", 1000.0, 5905.0, 0.0, "C1", &[]),
    ("co-kit", "outlet-counter", 1500.0, 5905.0, 0.0, "C1", &[]),
    ("spo-kit", "outlet-spo", 2800.0, 5905.0, 0.0, "C3", &[]),
    ("co-out", "outlet-outdoor", -105.0, 1000.0, 270.0, "C1", &[]),
    ("pb-main", "panelboard", 125.0, 700.0, 90.0, "", &[]),
    ("sd-liv", "smoke-detector", 2500.0, 1400.0, 0.0, "L1", &[]),
    ("bell", "doorbell-button", 2300.0, -90.0, 0.0, "", &[]),
    ("chime", "doorbell-chime", 4925.0, 1200.0, 270.0, "C1", &[]),
    ("ac-win", "aircon-window", 3900.0, 0.0, 180.0, "", &[]),
    // Bedroom: one light on two switches (S3), a wall light, a split unit.
    ("lt-bed", "light-ceiling", 6500.0, 2100.0, 0.0, "L2", &[]),
    ("sw-bed-door", "switch-1", 5070.0, 3635.0, 90.0, "L2", &["lt-bed"]),
    ("sw-bed-head", "switch-2", 7905.0, 2450.0, 270.0, "L2", &["lt-bed", "lt-bed-wall"]),
    ("lt-bed-wall", "light-wall", 7865.0, 1500.0, 270.0, "L2", &[]),
    ("co-bed-1", "outlet-duplex", 7905.0, 900.0, 270.0, "C2", &[]),
    ("co-bed-2", "outlet-duplex", 5070.0, 1500.0, 90.0, "C2", &[]),
    ("acu-bed", "aircon-indoor-1hp", 7810.0, 3100.0, 270.0, "", &[]),
    ("aco-bed", "outlet-aircon", 7905.0, 3700.0, 270.0, "AC1", &["acu-bed"]),
    ("cu-bed", "aircon-outdoor-1hp", 8330.0, 3100.0, 90.0, "", &[]),
    // T&B.
    ("dl-tb-1", "light-downlight", 6600.0, 5100.0, 0.0, "L2", &[]),
    ("dl-tb-2", "light-downlight", 7400.0, 4900.0, 0.0, "L2", &[]),
    ("sw-tb", "switch-1", 7250.0, 4130.0, 0.0, "L2", &["dl-tb-1", "dl-tb-2"]),
    // Plumbing fixtures and utilities at the ends of the concept runs.
    ("fx-sink", "kitchen-sink", 2000.0, 5625.0, 0.0, "", &[]),
    ("fx-lav", "lavatory", 6300.0, 5715.0, 0.0, "", &[]),
    ("fx-wc", "wc", 6780.0, 5575.0, 0.0, "", &[]),
    ("fx-shower", "shower", 7475.0, 5475.0, 0.0, "", &[]),
    ("fx-heater", "water-heater", 7525.0, 4300.0, 180.0, "", &[]),
];

/// Utilities placed outside the house.
pub const OUTSIDE: [Obj; 2] = [
    ("ut-meter", "water-meter", 600.0, -1800.0, 0.0, "", &[]),
    ("ut-septic", "septic-tank", 9500.0, 4950.0, 0.0, "", &[]),
];

/// Service runs: storm, conduit, line set and condensate.
pub const SERVICE_RUNS: [Run; 5] = [
    ("p-st-down", PipeSystem::Storm, PipeMaterial::Upvc, 100.0, "Downspout",
     &[(8150.0, -150.0, 2900.0), (8150.0, -150.0, -300.0), (9500.0, -150.0, -330.0)]),
    ("p-cd-light", PipeSystem::Conduit, PipeMaterial::Pvc, 20.0, "Lighting home run",
     &[(300.0, 1000.0, 1700.0), (300.0, 1000.0, 2800.0), (1500.0, 1000.0, 2800.0), (1500.0, 2600.0, 2800.0)]),
    ("p-cd-recept", PipeSystem::Conduit, PipeMaterial::Pvc, 20.0, "Receptacle home run",
     &[(300.0, 400.0, 1500.0), (300.0, 400.0, 300.0), (300.0, 1800.0, 300.0), (125.0, 1800.0, 300.0)]),
    ("p-ac-lineset", PipeSystem::Refrigerant, PipeMaterial::Copper, 9.52, "Bedroom line set",
     &[(7810.0, 2900.0, 2400.0), (8250.0, 2900.0, 2400.0), (8250.0, 2900.0, 400.0)]),
    ("p-ac-drain", PipeSystem::Condensate, PipeMaterial::Pvc, 20.0, "Bedroom condensate",
     &[(7810.0, 3300.0, 2300.0), (8150.0, 3300.0, 2290.0), (8150.0, 3300.0, 100.0)]),
];

pub fn object(level: &str, o: &Obj) -> Asset {
    let item = guhit_model::defaults::asset_catalog()
        .into_iter()
        .find(|c| c.key == o.1)
        .unwrap_or_else(|| panic!("catalog has no {}", o.1));
    Asset {
        id: o.0.into(),
        level_id: level.into(),
        catalog_key: o.1.into(),
        name: item.name.clone(),
        category: item.category,
        position: p(o.2, o.3),
        rotation_deg: o.4,
        width_mm: item.width_mm,
        depth_mm: item.depth_mm,
        height_mm: item.height_mm,
        elevation_mm: item.elevation_mm,
        light: item.light,
        links: o.6.iter().map(|s| s.to_string()).collect(),
        circuit: o.5.into(),
    }
}

fn room_geometry(id: &str, pts: &[(f64, f64)]) -> RoomGeometry {
    let polygon: Vec<Point> = pts.iter().map(|q| p(q.0, q.1)).collect();
    let mut area = 0.0;
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        area += a.0 * b.1 - b.0 * a.1;
    }
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |acc, q| (acc.0 + q.0, acc.1 + q.1));
    RoomGeometry {
        room_id: id.into(),
        polygon,
        centerline_polygon: vec![],
        area_mm2: area.abs() / 2.0,
        perimeter_mm: 0.0,
        label_point: p(sx / pts.len() as f64, sy / pts.len() as f64),
        wall_ids: vec![],
    }
}

pub const WALL_RIGHT: &str = "00000000-0000-4000-8000-000000000102";

/// The bungalow with a T&B, the sixteen plumbing runs, and a full set of
/// services: lights, switches (one light on two switches), outlets, a
/// panelboard, a detector, a doorbell, split and window aircon, storm,
/// conduit, a line set and condensate with their core holes. The schedule
/// is left empty, so the sheets count with the contract rule themselves.
pub fn services() -> DocState {
    let mut s = plumbing();
    let lvl = level_id(&s);
    s.project.name = "Santos Residence".into();
    // T&B walls and door, as in the plumbing demo.
    let t_wall = |id: &str, a: Point, b: Point| {
        Element::Wall(Wall { id: id.into(), level_id: lvl.clone(), start: a, end: b, thickness_mm: 100.0, height_mm: None, material_id: None })
    };
    s.project.elements.push(t_wall("w-tb-south", p(6000.0, 4200.0), p(8000.0, 4200.0)));
    s.project.elements.push(t_wall("w-tb-west", p(6000.0, 4200.0), p(6000.0, 6000.0)));
    s.project.elements.push(Element::Opening(Opening {
        id: "o-tb".into(),
        wall_id: "w-tb-south".into(),
        opening_type: OpeningType::Door,
        style: OpeningStyle::SwingSingle,
        offset_mm: 600.0,
        width_mm: 700.0,
        height_mm: 2100.0,
        sill_mm: 0.0,
        flip_side: true,
        flip_hinge: false,
        material_id: None,
    }));
    s.project.elements.push(Element::Room(Room {
        id: "r-tb".into(),
        level_id: lvl.clone(),
        name: "T&B".into(),
        usage: RoomUsage::Bathroom,
        seed: p(6900.0, 5000.0),
        floor_material_id: None,
        auto_named: false,
    }));
    let bedroom = s
        .project
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Room(r) if r.name == "Bedroom" => Some(r.id.clone()),
            _ => None,
        })
        .expect("bedroom");
    s.derived.rooms.retain(|g| g.room_id != bedroom);
    s.derived.rooms.push(room_geometry(
        &bedroom,
        &[(5050.0, 75.0), (7925.0, 75.0), (7925.0, 4150.0), (6050.0, 4150.0), (6050.0, 5925.0), (5050.0, 5925.0)],
    ));
    s.derived.rooms.push(room_geometry("r-tb", &[(6050.0, 4250.0), (7925.0, 4250.0), (7925.0, 5925.0), (6050.0, 5925.0)]));
    for key in [LayerKey::Storm, LayerKey::Electrical, LayerKey::Aircon] {
        if !s.project.layers.iter().any(|l| l.key == key) {
            s.project.layers.push(Layer { key, visible: true, locked: false });
        }
    }
    for o in OBJECTS.iter().chain(OUTSIDE.iter()) {
        s.project.elements.push(Element::Asset(object(&lvl, o)));
    }
    for run in &SERVICE_RUNS {
        s.project.elements.push(Element::Pipe(pipe(&lvl, run)));
    }
    // The line set and the condensate cross the east wall: core holes.
    for (pipe_id, y, z, d) in [("p-ac-lineset", 2900.0, 2400.0, 9.52), ("p-ac-drain", 3300.0, 2295.0, 20.0)] {
        s.derived.pipes.penetrations.push(PipePenetration {
            kind: PenetrationKind::Wall,
            pipe_id: pipe_id.into(),
            host_id: Some(WALL_RIGHT.into()),
            level_id: lvl.clone(),
            position: Vec3 { x: 8000.0, y, z },
            direction: Vec3 { x: 1.0, y: 0.0, z: 0.0 },
            diameter_mm: d,
        });
    }
    s
}

/// `services()` with a second level holding a T&B stack and a WC, and a
/// schedule written by hand (as the engine derives it).
pub fn services_two_levels() -> DocState {
    let mut s = services();
    s.project.levels.push(Level {
        id: "level-2".into(),
        name: "Second Floor".into(),
        elevation_mm: 3000.0,
        height_mm: 3000.0,
    });
    s.project.elements.push(Element::Asset(object("level-2", &("fx-wc-2", "wc", 6780.0, 5575.0, 0.0, "", &[]))));
    s.project.elements.push(Element::Asset(object("level-2", &("fx-lav-2", "lavatory", 6300.0, 5715.0, 0.0, "", &[]))));
    let mut stack = pipe("level-2", &RUNS[15]);
    stack.id = "p-vt-stack-2".into();
    stack.points = vec![
        Vec3 { x: 7800.0, y: 6000.0, z: 0.0 },
        Vec3 { x: 7800.0, y: 6000.0, z: 1200.0 },
        Vec3 { x: 7300.0, y: 6000.0, z: 1200.0 },
    ];
    s.project.elements.push(Element::Pipe(stack));
    s.project.elements.push(Element::Pipe(Pipe {
        id: "p-dr-wc-2".into(),
        level_id: "level-2".into(),
        system: PipeSystem::Drainage,
        material: PipeMaterial::Upvc,
        diameter_mm: 100.0,
        points: vec![
            Vec3 { x: 6780.0, y: 5650.0, z: 20.0 },
            Vec3 { x: 6780.0, y: 5650.0, z: -200.0 },
            Vec3 { x: 7700.0, y: 5650.0, z: -210.0 },
            Vec3 { x: 7700.0, y: 5650.0, z: -3450.0 },
        ],
        name: "Second floor WC drain".into(),
    }));
    s.project.elements.push(Element::Pipe(Pipe {
        id: "p-cw-2".into(),
        level_id: "level-2".into(),
        system: PipeSystem::ColdWater,
        material: PipeMaterial::Ppr,
        diameter_mm: 20.0,
        points: vec![
            Vec3 { x: 6500.0, y: 6000.0, z: -2700.0 },
            Vec3 { x: 6500.0, y: 6000.0, z: 300.0 },
            Vec3 { x: 6780.0, y: 6000.0, z: 300.0 },
            Vec3 { x: 6780.0, y: 5925.0, z: 300.0 },
        ],
        name: "Second floor WC supply".into(),
    }));
    s.derived.schedule = guhit_export::services::schedule_rows(&s.project, &s.derived);
    s
}

fn issue(id: &str, severity: Severity, code: &str, message: &str, ids: &[&str], status: IssueStatus, note: &str) -> Issue {
    Issue {
        id: id.into(),
        severity,
        code: code.into(),
        message: message.into(),
        element_ids: ids.iter().map(|s| s.to_string()).collect(),
        location: None,
        status,
        note: note.into(),
    }
}

/// `services()` with review items as the engine derives them: open items on
/// the ground floor, one on the second floor, two set aside with notes and
/// one set-aside finding that is no longer found.
pub fn services_with_review() -> DocState {
    let mut s = services_two_levels();
    use IssueStatus::{Ignored, Open};
    s.derived.issues = vec![
        issue("i1", Severity::Warning, "pipe_across_opening", "Kitchen sink waste passes through the opening of Door 1. Route it above the door head at 2.10 m or under the slab.", &["p-dr-sink"], Open, ""),
        issue("i2", Severity::Info, "light_no_switch", "No switch controls Floor lamp. Link a switch to it with the link tool if it is not a plug-in lamp.", &["lamp-liv"], Open, ""),
        issue("i3", Severity::Warning, "switch_behind_door", "Switch, one gang sits behind the swing of the bedroom door. Move it to the latch side.", &["sw-bed-door"], Open, ""),
        issue("i4", Severity::Warning, "lineset_long", "Bedroom line set is 4.9 m, inside the 25 m the indoor unit allows.", &["p-ac-lineset", "acu-bed"], Ignored, "Checked with the installer on site; the unit sits outside the bedroom window."),
        issue("i5", Severity::Info, "lineset_extra", "Bedroom line set runs 1.9 m beyond the 3 m a standard install includes.", &["p-ac-lineset"], Ignored, ""),
        issue("i6", Severity::Warning, "drain_slope_low", "Second floor WC drain falls 0.1 percent over 920 mm, less than the 1 percent default.", &["p-dr-wc-2"], Open, ""),
    ];
    s.derived.review_resolved = vec![ReviewMark {
        target: ReviewTarget::Issue { id: "pipe_through_column:p-dr-wc-2".into() },
        note: "Column moved to clear the drain.".into(),
    }];
    s
}
