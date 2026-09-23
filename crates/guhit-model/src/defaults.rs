//! Built-in data: PH-first defaults, local material presets, object library,
//! render styles. CONTRACT FILE - owned by the orchestrator.
//!
//! Built-in materials and catalog items use stable slug ids, not UUIDs, so
//! the frontend can key symbols and swatches on them.

use crate::api::RenderStyle;
use crate::model::*;

pub fn new_id() -> Id {
    uuid::Uuid::new_v4().to_string()
}

/// Current UTC time as RFC 3339, second precision.
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since epoch (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m,
        d,
        rem / 3_600,
        (rem % 3_600) / 60,
        rem % 60
    )
}

pub const DEFAULT_LEVEL_HEIGHT_MM: f64 = 3000.0;
/// 150 mm: 6 inch CHB, the common PH exterior wall.
pub const DEFAULT_WALL_THICKNESS_MM: f64 = 150.0;

pub const DOOR_DEFAULT: (f64, f64, f64) = (900.0, 2100.0, 0.0);
pub const WINDOW_DEFAULT: (f64, f64, f64) = (1200.0, 1200.0, 900.0);

pub const MAT_WALL_DEFAULT: &str = "mat-chb-painted";
pub const MAT_FLOOR_DEFAULT: &str = "mat-tile-ceramic";
pub const MAT_ROOF_DEFAULT: &str = "mat-roof-longspan";

pub fn default_settings() -> ProjectSettings {
    ProjectSettings {
        display_unit: DisplayUnit::Mm,
        scale_denominator: 100,
        north_angle_deg: 0.0,
        paper: PaperSize::A3,
        default_wall_thickness_mm: DEFAULT_WALL_THICKNESS_MM,
        grid_mm: 100.0,
        client_name: String::new(),
        location: String::new(),
        designer: String::new(),
    }
}

pub fn default_layers() -> Vec<Layer> {
    use LayerKey::*;
    [
        Walls,
        Openings,
        Rooms,
        Columns,
        Stairs,
        Assets,
        Annotations,
        Dimensions,
        Underlays,
        ColdWater,
        HotWater,
        Drainage,
        Vent,
    ]
    .into_iter()
    .map(|key| Layer {
        key,
        visible: true,
        locked: false,
    })
    .collect()
}

/// Tool defaults per pipe system: material, nominal size in mm, and the
/// height above the floor a new run starts at. Suggestions for drawing, not
/// sizing. The frontend mirrors this table (docs/CONTRACT.md).
pub fn pipe_defaults(system: PipeSystem) -> (PipeMaterial, f64, f64) {
    match system {
        PipeSystem::ColdWater => (PipeMaterial::Ppr, 20.0, 300.0),
        PipeSystem::HotWater => (PipeMaterial::Ppr, 20.0, 300.0),
        PipeSystem::Drainage => (PipeMaterial::Upvc, 50.0, -300.0),
        PipeSystem::Vent => (PipeMaterial::Upvc, 50.0, 300.0),
    }
}

/// Default minimum fall of horizontal drainage, in percent: 2 percent, or
/// 1 percent from 100 mm up. A review default, not a code statement.
pub fn drain_min_slope_pct(diameter_mm: f64) -> f64 {
    if diameter_mm >= 100.0 {
        1.0
    } else {
        2.0
    }
}

pub fn default_roof() -> Roof {
    Roof {
        kind: RoofKind::None,
        pitch_deg: 20.0,
        overhang_mm: 600.0,
        thickness_mm: 150.0,
        ridge_axis: Axis::X,
        material_id: Some(MAT_ROOF_DEFAULT.to_string()),
    }
}

fn mat(
    id: &str,
    name: &str,
    category: MaterialCategory,
    color: &str,
    roughness: f64,
    metalness: f64,
    opacity: f64,
    pattern: MaterialPattern,
) -> Material {
    Material {
        id: id.to_string(),
        name: name.to_string(),
        category,
        color: color.to_string(),
        roughness,
        metalness,
        opacity,
        pattern,
        builtin: true,
    }
}

/// Local material presets common in Philippine residential work.
pub fn default_materials() -> Vec<Material> {
    use MaterialCategory as C;
    use MaterialPattern as P;
    vec![
        mat(MAT_WALL_DEFAULT, "CHB, plastered and painted", C::Wall, "#f2efe8", 0.9, 0.0, 1.0, P::None),
        mat("mat-chb-bare", "CHB, bare", C::Wall, "#a9a9a4", 0.95, 0.0, 1.0, P::Chb),
        mat("mat-concrete-fairface", "Concrete, fair-faced", C::Wall, "#bdbdb8", 0.85, 0.0, 1.0, P::Concrete),
        mat("mat-paint-warm-white", "Paint, warm white", C::Wall, "#faf6ee", 0.9, 0.0, 1.0, P::None),
        mat("mat-paint-sage", "Paint, sage green", C::Wall, "#b7c4ae", 0.9, 0.0, 1.0, P::None),
        mat("mat-wood-cladding", "Wood cladding", C::Wood, "#9a6b43", 0.7, 0.0, 1.0, P::WoodPlank),
        mat(MAT_FLOOR_DEFAULT, "Ceramic tile 600 x 600", C::Floor, "#e4e0d8", 0.35, 0.0, 1.0, P::Tile),
        mat("mat-tile-granite", "Granite tile", C::Floor, "#6f6f73", 0.25, 0.05, 1.0, P::Tile),
        mat("mat-floor-laminate", "Wood laminate", C::Floor, "#b98d5f", 0.55, 0.0, 1.0, P::WoodPlank),
        mat("mat-floor-concrete", "Polished concrete", C::Floor, "#b3b1ab", 0.4, 0.0, 1.0, P::Concrete),
        mat(MAT_ROOF_DEFAULT, "Long-span pre-painted metal roof", C::Roof, "#7a3b33", 0.45, 0.6, 1.0, P::RoofSheet),
        mat("mat-roof-gi", "GI corrugated sheet", C::Roof, "#aab0b5", 0.4, 0.8, 1.0, P::RoofSheet),
        mat("mat-roof-clay-tile", "Clay roof tile", C::Roof, "#b0583a", 0.8, 0.0, 1.0, P::RoofTile),
        mat("mat-roof-concrete-deck", "Concrete roof deck", C::Roof, "#c4c2bc", 0.85, 0.0, 1.0, P::Concrete),
        mat("mat-glass-clear", "Clear glass", C::Glass, "#bfe3ee", 0.05, 0.0, 0.35, P::Glass),
        mat("mat-wood-door", "Wood door, mahogany finish", C::Wood, "#6e4428", 0.6, 0.0, 1.0, P::WoodPlank),
        mat("mat-aluminum-frame", "Aluminum frame, powder coated", C::Metal, "#3b3f45", 0.4, 0.8, 1.0, P::None),
        mat("mat-steel", "Steel, painted", C::Metal, "#4a4f57", 0.5, 0.7, 1.0, P::None),
    ]
}

fn item(
    key: &str,
    name: &str,
    category: AssetCategory,
    w: f64,
    d: f64,
    h: f64,
    elevation: f64,
) -> CatalogItem {
    CatalogItem {
        key: key.to_string(),
        name: name.to_string(),
        category,
        width_mm: w,
        depth_mm: d,
        height_mm: h,
        elevation_mm: elevation,
    }
}

/// Starter object library. The frontend draws a 2D symbol and a simple 3D
/// form per `key`, and falls back to a labelled box for unknown keys.
pub fn asset_catalog() -> Vec<CatalogItem> {
    use AssetCategory as A;
    vec![
        item("bed-single", "Single bed", A::Furniture, 920.0, 1900.0, 500.0, 0.0),
        item("bed-double", "Double bed", A::Furniture, 1370.0, 1900.0, 500.0, 0.0),
        item("bed-queen", "Queen bed", A::Furniture, 1520.0, 2030.0, 500.0, 0.0),
        item("wardrobe", "Wardrobe", A::Furniture, 1200.0, 600.0, 2100.0, 0.0),
        item("sofa-3", "Sofa, 3 seater", A::Furniture, 2100.0, 900.0, 800.0, 0.0),
        item("sofa-2", "Sofa, 2 seater", A::Furniture, 1500.0, 900.0, 800.0, 0.0),
        item("armchair", "Armchair", A::Furniture, 850.0, 850.0, 800.0, 0.0),
        item("coffee-table", "Coffee table", A::Furniture, 1100.0, 600.0, 420.0, 0.0),
        item("tv-console", "TV console", A::Furniture, 1600.0, 450.0, 500.0, 0.0),
        item("dining-4", "Dining table, 4 seats", A::Furniture, 1200.0, 800.0, 750.0, 0.0),
        item("dining-6", "Dining table, 6 seats", A::Furniture, 1800.0, 900.0, 750.0, 0.0),
        item("desk", "Work desk", A::Furniture, 1200.0, 600.0, 750.0, 0.0),
        item("wc", "Water closet", A::Sanitary, 400.0, 700.0, 780.0, 0.0),
        item("lavatory", "Lavatory", A::Sanitary, 500.0, 420.0, 200.0, 650.0),
        item("shower", "Shower area", A::Sanitary, 900.0, 900.0, 50.0, 0.0),
        item("bathtub", "Bathtub", A::Sanitary, 1500.0, 750.0, 550.0, 0.0),
        item("kitchen-counter", "Kitchen counter", A::Kitchen, 1800.0, 600.0, 900.0, 0.0),
        item("kitchen-sink", "Kitchen sink counter", A::Kitchen, 1200.0, 600.0, 900.0, 0.0),
        item("range", "Cooking range", A::Appliance, 600.0, 600.0, 900.0, 0.0),
        item("refrigerator", "Refrigerator", A::Appliance, 700.0, 700.0, 1750.0, 0.0),
        item("washing-machine", "Washing machine", A::Appliance, 600.0, 600.0, 850.0, 0.0),
        item("plant-pot", "Potted plant", A::Plant, 500.0, 500.0, 1200.0, 0.0),
        item("tree", "Tree", A::Plant, 3000.0, 3000.0, 5000.0, 0.0),
        item("car-sedan", "Car, sedan", A::Vehicle, 1800.0, 4500.0, 1450.0, 0.0),
    ]
}

pub fn render_styles() -> Vec<RenderStyle> {
    let s = |key: &str, name: &str, description: &str, prompt: &str| RenderStyle {
        key: key.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        prompt: prompt.to_string(),
    };
    vec![
        s(
            "tropical-modern",
            "Tropical modern",
            "Deep eaves, wood accents, lush planting, bright daylight",
            "tropical modern Philippine residence, deep roof overhangs, warm wood accents, white plastered walls, lush tropical landscaping, bright late-morning daylight, photorealistic architectural photography",
        ),
        s(
            "modern-minimal",
            "Modern minimal",
            "Clean white volumes, dark frames, restrained planting",
            "minimalist modern house, clean white volumes, dark aluminum window frames, restrained landscaping, soft overcast light, photorealistic architectural photography",
        ),
        s(
            "bahay-kubo-contemporary",
            "Contemporary bahay kubo",
            "Raised floor feel, bamboo and wood, wide openings",
            "contemporary bahay kubo inspired house, bamboo and hardwood details, wide ventilated openings, steep roof, garden setting, golden hour light, photorealistic",
        ),
        s(
            "dusk-exterior",
            "Dusk exterior",
            "Warm interior glow against a blue hour sky",
            "residential exterior at blue hour, warm interior lighting glowing through windows, wet pavement reflections, photorealistic architectural photography",
        ),
        s(
            "concept-sketch",
            "Concept sketch",
            "Loose marker and pencil presentation sketch",
            "architectural concept sketch, loose pencil linework with marker washes, white paper, presentation drawing",
        ),
        s(
            "interior-warm",
            "Warm interior",
            "Natural light, wood floors, soft furnishings",
            "warm residential interior, natural daylight through windows, wood flooring, soft neutral furnishings, indoor plants, photorealistic interior photography",
        ),
    ]
}

/// A blank metric project with one level.
pub fn new_project(name: &str) -> Project {
    let now = now_rfc3339();
    Project {
        schema_version: SCHEMA_VERSION,
        id: new_id(),
        name: name.to_string(),
        created_at: now.clone(),
        updated_at: now,
        settings: default_settings(),
        levels: vec![Level {
            id: new_id(),
            name: "Ground Floor".to_string(),
            elevation_mm: 0.0,
            height_mm: DEFAULT_LEVEL_HEIGHT_MM,
        }],
        layers: default_layers(),
        materials: default_materials(),
        elements: vec![],
        roof: default_roof(),
    }
}
