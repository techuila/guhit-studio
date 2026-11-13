//! Built-in data: PH-first defaults, local material presets, render styles,
//! and the object library (`catalog.rs`). CONTRACT FILE - owned by the
//! orchestrator.
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
/// Height of a site wall (a lot fence or boundary wall) drawn without one:
/// the usual PH perimeter CHB fence.
pub const SITE_WALL_HEIGHT_MM: f64 = 1800.0;

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
        site: None,
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
        Storm,
        Electrical,
        Aircon,
        Site,
        Slabs,
        Grids,
        Roofs,
    ]
    .into_iter()
    .map(|key| Layer {
        key,
        visible: true,
        locked: false,
    })
    .collect()
}

/// A new conduit run starts this far above its level's ceiling: inside the
/// slab of the level above, or above the ceiling on the top level
/// (DECISIONS D40).
pub const CONDUIT_ABOVE_CEILING_MM: f64 = 100.0;

/// Tool defaults per pipe system: material, nominal size in mm, and the
/// height above the floor a new run starts at. Suggestions for drawing, not
/// sizing. The frontend mirrors this table (docs/CONTRACT.md). The conduit
/// height here is for a one storey level of the default height; use
/// `pipe_start_height_mm` for a real level.
pub fn pipe_defaults(system: PipeSystem) -> (PipeMaterial, f64, f64) {
    match system {
        PipeSystem::ColdWater => (PipeMaterial::Ppr, 20.0, 300.0),
        PipeSystem::HotWater => (PipeMaterial::Ppr, 20.0, 300.0),
        PipeSystem::Drainage => (PipeMaterial::Upvc, 50.0, -300.0),
        PipeSystem::Vent => (PipeMaterial::Upvc, 50.0, 300.0),
        PipeSystem::Storm => (PipeMaterial::Upvc, 100.0, -300.0),
        // Conduit usually runs above the ceiling and drops in the wall.
        PipeSystem::Conduit => (
            PipeMaterial::Pvc,
            20.0,
            DEFAULT_LEVEL_HEIGHT_MM + CONDUIT_ABOVE_CEILING_MM,
        ),
        // A line set leaves the indoor unit near its top.
        PipeSystem::Refrigerant => (PipeMaterial::Copper, 9.52, 2400.0),
        PipeSystem::Condensate => (PipeMaterial::Pvc, 20.0, 2300.0),
    }
}

/// Height above the level floor a new run of `system` starts at, on a level
/// whose ceiling is `ceiling_mm` above its floor (its height, or the
/// underside of the slab above when that is lower). Conduit starts
/// `CONDUIT_ABOVE_CEILING_MM` above the ceiling; every other system at its
/// `pipe_defaults` height. Mirrored by `pipeStartHeightMm` in
/// src/contract/pipes.ts.
pub fn pipe_start_height_mm(system: PipeSystem, ceiling_mm: f64) -> f64 {
    match system {
        PipeSystem::Conduit if ceiling_mm.is_finite() => ceiling_mm + CONDUIT_ABOVE_CEILING_MM,
        other => pipe_defaults(other).2,
    }
}

/// Manila, the default site when a project has none.
pub fn default_site() -> Site {
    Site {
        city: "manila".into(),
        latitude_deg: 14.5995,
        longitude_deg: 120.9842,
        utc_offset_min: 480,
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

// One positional row per material keeps the preset table readable.
#[allow(clippy::too_many_arguments)]
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
        // Outdoor decks on the ground: a site kit paving (ambientCG texture in the pack).
        mat("mat-paving-stone", "Paving stones", C::Floor, "#a9a49a", 0.85, 0.0, 1.0, P::Tile),
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

/// Starter object library, kept in `catalog.rs`.
pub use crate::catalog::asset_catalog;

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
        review: vec![],
        opening_types: vec![],
        title_block: Box::default(),
        sheet_set: SheetSet::default(),
    }
}
