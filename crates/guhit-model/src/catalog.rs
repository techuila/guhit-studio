//! The object library: the catalog items the plan, the 3D view, placement
//! and the copilot offer. Data only: the furnishing lane fills it, and
//! `defaults::asset_catalog` re-exports it. CONTRACT FILE.
//!
//! Catalog items use stable slug keys, not UUIDs, so the frontend can key
//! symbols and pack models on them.
//!
//! Each `item(...)` stays on one line: `src/viewer3d/scene/assets.test.ts`
//! and `src/editor2d/symbols.test.ts` read keys, sizes, mounts and lights
//! from this file's text.
//!
//! Catalog v2 (Wave 2, lane L5): every furnishing item names the rooms it
//! belongs in, search tags and the free space it needs in front, for
//! `Query::PlacementOptions`, `Query::Catalog` and "Furnish rooms". `model`
//! names the pack model (`public/assets/pack/manifest.json`), keyed by the
//! catalog key; None draws the procedural form. Built-ins (`parametric`) and
//! the site kit are drawn in code at any size.

use crate::model::*;

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
        mount: Mount::Floor,
        device: None,
        light: None,
        aircon: None,
        rooms: vec![],
        tags: vec![],
        clearance_mm: 0.0,
        parametric: None,
        model: None,
    }
}

fn mounted(mut it: CatalogItem, mount: Mount, device: Option<DeviceKind>) -> CatalogItem {
    it.mount = mount;
    it.device = device;
    it
}

/// A fixture that gives light: lumens and color temperature, on by default.
fn lit(mut it: CatalogItem, lumens: f64, kelvin: f64) -> CatalogItem {
    it.light = Some(AssetLight { lumens, kelvin, on: true });
    it
}

#[allow(clippy::too_many_arguments)]
fn aircon(
    mut it: CatalogItem,
    role: AirconRole,
    hp: f64,
    liquid_mm: f64,
    gas_mm: f64,
    max_line_m: f64,
    max_rise_m: f64,
) -> CatalogItem {
    it.aircon = Some(AirconSpec {
        role,
        hp,
        liquid_mm,
        gas_mm,
        min_line_m: if role == AirconRole::Window { 0.0 } else { 3.0 },
        max_line_m,
        max_rise_m,
        included_line_m: if role == AirconRole::Window { 0.0 } else { 3.0 },
    });
    it
}

/// Chained setters for the catalog v2 fields, so each item stays one line.
trait Furnish {
    /// Rooms it belongs in.
    fn rooms(self, rooms: &[RoomUsage]) -> Self;
    /// Search words, comma separated.
    fn tags(self, tags: &str) -> Self;
    /// Free space in front (its -y side), mm.
    fn clear(self, mm: f64) -> Self;
    /// Shown by the pack model of the same key.
    fn pack(self) -> Self;
    /// A built-in sized in code.
    fn built(self, kind: BuiltInKind) -> Self;
    /// Where it mounts.
    fn mount(self, mount: Mount) -> Self;
}

impl Furnish for CatalogItem {
    fn rooms(mut self, rooms: &[RoomUsage]) -> Self {
        self.rooms = rooms.to_vec();
        self
    }
    fn tags(mut self, tags: &str) -> Self {
        self.tags = tags.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
        self
    }
    fn clear(mut self, mm: f64) -> Self {
        self.clearance_mm = mm;
        self
    }
    fn pack(mut self) -> Self {
        self.model = Some(self.key.clone());
        self
    }
    fn built(mut self, kind: BuiltInKind) -> Self {
        self.parametric = Some(kind);
        self
    }
    fn mount(mut self, mount: Mount) -> Self {
        self.mount = mount;
        self
    }
}

/// Starter object library. The frontend draws a 2D symbol and a 3D form per
/// `key` (the pack model when there is one), and falls back to a labelled
/// box for unknown keys.
pub fn asset_catalog() -> Vec<CatalogItem> {
    use AssetCategory as A;
    use BuiltInKind as B;
    use DeviceKind as D;
    use Mount as M;
    use RoomUsage as R;
    // Rooms people sleep, sit and work in, for plants and lamps.
    const LIVED: &[RoomUsage] = &[R::Living, R::Dining, R::Bedroom, R::MasterBedroom, R::Office, R::Hallway, R::Porch];
    const BED: &[RoomUsage] = &[R::Bedroom, R::MasterBedroom];
    const BATH: &[RoomUsage] = &[R::Bathroom, R::PowderRoom];
    vec![
        item("bed-single", "Single bed", A::Furniture, 920.0, 1900.0, 500.0, 0.0).rooms(&[R::Bedroom]).tags("bed, single, kids").clear(600.0).pack(),
        item("bed-double", "Double bed", A::Furniture, 1370.0, 1900.0, 500.0, 0.0).rooms(BED).tags("bed, double").clear(600.0).pack(),
        item("bed-queen", "Queen bed", A::Furniture, 1520.0, 2030.0, 500.0, 0.0).rooms(&[R::MasterBedroom, R::Bedroom]).tags("bed, queen, master").clear(600.0).pack(),
        item("wardrobe", "Wardrobe", A::Furniture, 1200.0, 600.0, 2100.0, 0.0).rooms(BED).tags("wardrobe, closet, cabinet, storage").clear(600.0).pack(),
        item("sofa-3", "Sofa, 3 seater", A::Furniture, 2100.0, 900.0, 800.0, 0.0).rooms(&[R::Living]).tags("sofa, couch, 3 seater, modern").clear(450.0).pack(),
        item("sofa-2", "Sofa, 2 seater", A::Furniture, 1500.0, 900.0, 800.0, 0.0).rooms(&[R::Living, R::Office]).tags("sofa, couch, 2 seater, loveseat, modern").clear(450.0).pack(),
        item("armchair", "Armchair", A::Furniture, 850.0, 850.0, 800.0, 0.0).rooms(&[R::Living, R::Bedroom, R::MasterBedroom, R::Office]).tags("chair, armchair, modern, office").clear(300.0).pack(),
        item("coffee-table", "Coffee table", A::Furniture, 1100.0, 600.0, 420.0, 0.0).rooms(&[R::Living]).tags("table, coffee table, modern").pack(),
        item("tv-console", "TV console", A::Furniture, 1600.0, 450.0, 500.0, 0.0).rooms(&[R::Living, R::Bedroom, R::MasterBedroom]).tags("tv, television, console, cabinet").clear(900.0).pack(),
        item("dining-4", "Dining table, 4 seats", A::Furniture, 1200.0, 800.0, 750.0, 0.0).rooms(&[R::Dining, R::Kitchen]).tags("dining, table, chairs, 4 seats").pack(),
        item("dining-6", "Dining table, 6 seats", A::Furniture, 1800.0, 900.0, 750.0, 0.0).rooms(&[R::Dining]).tags("dining, table, chairs, 6 seats").pack(),
        item("desk", "Work desk", A::Furniture, 1200.0, 600.0, 750.0, 0.0).rooms(&[R::Office, R::Bedroom, R::MasterBedroom]).tags("desk, table, work, study").clear(700.0).pack(),
        item("wc", "Water closet", A::Sanitary, 400.0, 700.0, 780.0, 0.0).rooms(BATH).tags("toilet, wc, water closet").clear(550.0).pack(),
        item("lavatory", "Lavatory", A::Sanitary, 500.0, 420.0, 200.0, 650.0).rooms(BATH).tags("sink, lavatory, basin, wash basin").clear(550.0).pack(),
        item("shower", "Shower area", A::Sanitary, 900.0, 900.0, 50.0, 0.0).rooms(&[R::Bathroom]).tags("shower, bath, enclosure").clear(550.0).pack(),
        item("bathtub", "Bathtub", A::Sanitary, 1500.0, 750.0, 550.0, 0.0).rooms(&[R::Bathroom]).tags("bathtub, tub, bath").clear(550.0).pack(),
        item("kitchen-counter", "Kitchen counter", A::Kitchen, 1800.0, 600.0, 900.0, 0.0).rooms(&[R::Kitchen]).tags("counter, cabinet, kitchen").clear(900.0).pack(),
        item("kitchen-sink", "Kitchen sink counter", A::Kitchen, 1200.0, 600.0, 900.0, 0.0).rooms(&[R::Kitchen]).tags("sink, counter, kitchen").clear(900.0).pack(),
        item("range", "Cooking range", A::Appliance, 600.0, 600.0, 900.0, 0.0).rooms(&[R::Kitchen]).tags("stove, range, cooker, oven, gas").clear(900.0).pack(),
        item("refrigerator", "Refrigerator", A::Appliance, 700.0, 700.0, 1750.0, 0.0).rooms(&[R::Kitchen, R::Dining]).tags("fridge, refrigerator, ref, appliance").clear(900.0).pack(),
        item("washing-machine", "Washing machine", A::Appliance, 600.0, 600.0, 850.0, 0.0).rooms(&[R::Laundry, R::Kitchen, R::Bathroom]).tags("washer, washing machine, laundry, appliance").clear(700.0).pack(),
        item("plant-pot", "Potted plant", A::Plant, 500.0, 500.0, 1200.0, 0.0).rooms(LIVED).tags("plant, potted plant, green, decor").pack(),
        item("tree", "Tree", A::Plant, 3000.0, 3000.0, 5000.0, 0.0).tags("tree, garden, outdoor, landscape"),
        item("car-sedan", "Car, sedan", A::Vehicle, 1800.0, 4500.0, 1450.0, 0.0).rooms(&[R::Garage]).tags("car, sedan, vehicle, carport").clear(600.0),
        // Plumbing and utility fixtures.
        item("floor-drain", "Floor drain", A::Sanitary, 150.0, 150.0, 20.0, 0.0).tags("drain, floor drain, plumbing"),
        mounted(item("water-heater", "Water heater, instant", A::Sanitary, 250.0, 100.0, 400.0, 1500.0), M::Wall, None).tags("heater, water heater, shower heater"),
        item("water-meter", "Water meter", A::Utility, 250.0, 150.0, 150.0, 0.0).tags("meter, water meter, utility"),
        item("water-tank", "Water tank, 1000 L", A::Utility, 1000.0, 1000.0, 1300.0, 0.0).tags("tank, water tank, storage, utility"),
        item("septic-tank", "Septic tank, two chambers", A::Utility, 1800.0, 1200.0, 1500.0, -1650.0).tags("septic, septic tank, sewage, utility"),
        item("lpg-cylinder", "LPG cylinder, 11 kg", A::Utility, 300.0, 300.0, 600.0, 0.0).tags("lpg, gas, cylinder, tank"),
        mounted(item("electric-meter", "Electric meter", A::Utility, 200.0, 150.0, 300.0, 1500.0), M::Wall, None).tags("meter, electric meter, kwh, utility"),
        // Light fixtures: 900 lm is a 9 W LED bulb sold in PH stores.
        lit(mounted(item("light-ceiling", "Ceiling light", A::Lighting, 300.0, 300.0, 60.0, 2940.0), M::Ceiling, Some(D::LightingOutlet)), 900.0, 3000.0),
        lit(mounted(item("light-downlight", "Downlight", A::Lighting, 150.0, 150.0, 80.0, 2920.0), M::Ceiling, Some(D::LightingOutlet)), 700.0, 3000.0),
        lit(mounted(item("light-tube", "Tube light, LED T8", A::Lighting, 1200.0, 100.0, 60.0, 2940.0), M::Ceiling, Some(D::LightingOutlet)), 1800.0, 6500.0),
        lit(mounted(item("light-pendant", "Pendant light", A::Lighting, 350.0, 350.0, 400.0, 2000.0), M::Ceiling, Some(D::LightingOutlet)), 900.0, 2700.0),
        lit(mounted(item("light-wall", "Wall light", A::Lighting, 200.0, 120.0, 250.0, 1900.0), M::Wall, Some(D::LightingOutlet)), 600.0, 3000.0),
        lit(mounted(item("light-outdoor", "Outdoor wall light", A::Lighting, 150.0, 150.0, 250.0, 2100.0), M::Wall, Some(D::LightingOutlet)), 700.0, 3000.0),
        lit(item("light-floor-lamp", "Floor lamp", A::Lighting, 400.0, 400.0, 1600.0, 0.0).rooms(LIVED).tags("lamp, floor lamp, light"), 800.0, 2700.0),
        lit(item("light-table-lamp", "Table lamp", A::Lighting, 300.0, 300.0, 500.0, 750.0).tags("lamp, table lamp, light, tabletop"), 400.0, 2700.0),
        // Electrical devices. Switches at 1200 mm to center, 200 mm from the
        // latch side of the door (BP 344 IRR, 2024). Outlets at 300 mm.
        mounted(item("outlet-duplex", "Convenience outlet, duplex", A::Electrical, 70.0, 40.0, 115.0, 243.0), M::Wall, Some(D::ConvenienceReceptacle)),
        mounted(item("outlet-counter", "Counter outlet", A::Electrical, 70.0, 40.0, 115.0, 1043.0), M::Wall, Some(D::ConvenienceReceptacle)),
        mounted(item("outlet-outdoor", "Outdoor outlet, weatherproof", A::Electrical, 90.0, 60.0, 130.0, 235.0), M::Wall, Some(D::ConvenienceReceptacle)),
        mounted(item("outlet-spo", "Special purpose outlet", A::Electrical, 70.0, 40.0, 115.0, 243.0), M::Wall, Some(D::SpecialPurposeOutlet)),
        mounted(item("outlet-aircon", "Aircon outlet", A::Electrical, 70.0, 40.0, 115.0, 1943.0), M::Wall, Some(D::SpecialPurposeOutlet)),
        mounted(item("switch-1", "Switch, one gang", A::Electrical, 70.0, 40.0, 115.0, 1143.0), M::Wall, Some(D::Switch)),
        mounted(item("switch-2", "Switch, two gang", A::Electrical, 70.0, 40.0, 115.0, 1143.0), M::Wall, Some(D::Switch)),
        mounted(item("switch-3", "Switch, three gang", A::Electrical, 70.0, 40.0, 115.0, 1143.0), M::Wall, Some(D::Switch)),
        mounted(item("panelboard", "Panelboard", A::Electrical, 350.0, 100.0, 450.0, 1500.0), M::Wall, Some(D::Panelboard)),
        mounted(item("smoke-detector", "Smoke detector", A::Electrical, 120.0, 120.0, 50.0, 2950.0), M::Ceiling, Some(D::SmokeDetector)),
        mounted(item("doorbell-button", "Doorbell push button", A::Electrical, 50.0, 30.0, 80.0, 1260.0), M::Wall, Some(D::PushButton)),
        mounted(item("doorbell-chime", "Doorbell chime", A::Electrical, 120.0, 50.0, 120.0, 1940.0), M::Wall, Some(D::Buzzer)),
        // Aircon, split type and window type. Sizes and limits from the
        // Koppel, Carrier and Daikin PH manuals (research, 2026-09-23).
        aircon(mounted(item("aircon-indoor-1hp", "Split aircon indoor unit, 1.0 to 1.5 HP", A::Aircon, 800.0, 230.0, 295.0, 2300.0), M::Wall, Some(D::AirconIndoor)), AirconRole::Indoor, 1.5, 6.35, 9.52, 25.0, 10.0),
        aircon(mounted(item("aircon-indoor-2hp", "Split aircon indoor unit, 2.0 HP", A::Aircon, 965.0, 240.0, 320.0, 2300.0), M::Wall, Some(D::AirconIndoor)), AirconRole::Indoor, 2.0, 6.35, 12.7, 30.0, 20.0),
        aircon(mounted(item("aircon-indoor-3hp", "Split aircon indoor unit, 2.5 to 3.0 HP", A::Aircon, 1140.0, 275.0, 370.0, 2300.0), M::Wall, Some(D::AirconIndoor)), AirconRole::Indoor, 3.0, 6.35, 12.7, 30.0, 20.0),
        aircon(mounted(item("aircon-outdoor-1hp", "Aircon outdoor unit, 1.0 to 1.5 HP", A::Aircon, 770.0, 305.0, 555.0, 0.0), M::Floor, Some(D::AirconOutdoor)), AirconRole::Outdoor, 1.5, 6.35, 9.52, 25.0, 10.0),
        aircon(mounted(item("aircon-outdoor-3hp", "Aircon outdoor unit, 2.0 to 3.0 HP", A::Aircon, 890.0, 342.0, 673.0, 0.0), M::Floor, Some(D::AirconOutdoor)), AirconRole::Outdoor, 3.0, 6.35, 12.7, 30.0, 20.0),
        aircon(mounted(item("aircon-window", "Window aircon, 0.75 to 1.5 HP", A::Aircon, 471.0, 482.0, 345.0, 1200.0), M::Opening, Some(D::AirconWindow)), AirconRole::Window, 1.0, 0.0, 0.0, 0.0, 0.0),
        // ---------------------------------------------------- catalog v2
        // Photoscanned CC0 models from Poly Haven (assets/ASSETS.md). Sizes
        // are close to each scan's own, so the pack does not stretch them.
        // Living.
        item("sofa-leather", "Sofa, leather", A::Furniture, 1810.0, 820.0, 710.0, 0.0).rooms(&[R::Living, R::Office]).tags("sofa, couch, leather, 3 seater, classic, chesterfield").clear(450.0).pack(),
        item("sofa-wood", "Sofa, wooden frame", A::Furniture, 2290.0, 970.0, 860.0, 0.0).rooms(&[R::Living, R::Porch]).tags("sofa, couch, wood, sala set, narra, tropical, classic").clear(450.0).pack(),
        item("lounge-chair", "Lounge chair", A::Furniture, 1010.0, 1190.0, 1170.0, 0.0).rooms(&[R::Living, R::Bedroom, R::MasterBedroom, R::Office]).tags("chair, lounge chair, armchair, leather, mid century, modern").clear(300.0).pack(),
        item("armchair-classic", "Armchair, upholstered", A::Furniture, 850.0, 770.0, 1065.0, 0.0).rooms(&[R::Living, R::Bedroom, R::MasterBedroom]).tags("chair, armchair, upholstered, classic").clear(300.0).pack(),
        item("ottoman", "Ottoman", A::Furniture, 885.0, 620.0, 625.0, 0.0).rooms(&[R::Living, R::Bedroom, R::MasterBedroom]).tags("ottoman, footstool, pouf, leather").pack(),
        item("coffee-table-round", "Coffee table, round", A::Furniture, 1200.0, 1200.0, 490.0, 0.0).rooms(&[R::Living]).tags("table, coffee table, round, modern").pack(),
        item("coffee-table-square", "Coffee table, square", A::Furniture, 1100.0, 1100.0, 370.0, 0.0).rooms(&[R::Living]).tags("table, coffee table, square, modern").pack(),
        item("side-table", "Side table", A::Furniture, 550.0, 450.0, 550.0, 0.0).rooms(&[R::Living, R::Bedroom, R::MasterBedroom]).tags("table, side table, end table, wood, modern").pack(),
        item("side-table-round", "Side table, tall round", A::Furniture, 385.0, 385.0, 760.0, 0.0).rooms(&[R::Living, R::Bedroom, R::MasterBedroom, R::Hallway]).tags("table, side table, round, plant stand, classic").pack(),
        item("tea-table", "Low table, wood", A::Furniture, 840.0, 840.0, 500.0, 0.0).rooms(&[R::Living, R::Porch]).tags("table, coffee table, low table, wood, tea, tropical").pack(),
        item("sideboard", "Sideboard, slatted", A::Furniture, 2000.0, 500.0, 680.0, 0.0).rooms(&[R::Living, R::Dining]).tags("sideboard, cabinet, console, tv console, credenza, wood, modern").clear(600.0).pack(),
        mounted(item("ceiling-fan", "Ceiling fan", A::Appliance, 1460.0, 1460.0, 520.0, 2480.0), M::Ceiling, None).rooms(&[R::Living, R::Dining, R::Bedroom, R::MasterBedroom, R::Office]).tags("fan, ceiling fan, electric fan, appliance").pack(),
        item("plant-leafy", "Potted plant, leafy", A::Plant, 700.0, 660.0, 840.0, 0.0).rooms(LIVED).tags("plant, potted plant, leafy, tropical, green, decor").pack(),
        item("plant-succulent", "Succulent, small pot", A::Plant, 170.0, 185.0, 270.0, 0.0).rooms(&[R::Living, R::Bedroom, R::MasterBedroom, R::Office, R::Kitchen, R::Bathroom]).tags("plant, succulent, small, tabletop, decor").pack(),
        item("vase-ceramic", "Floor vase, ceramic", A::Furniture, 260.0, 260.0, 500.0, 0.0).rooms(&[R::Living, R::Dining, R::Hallway]).tags("vase, ceramic, white, decor").pack(),
        item("wall-art", "Picture frame, painting", A::Furniture, 600.0, 20.0, 465.0, 1350.0).mount(M::Wall).rooms(&[R::Living, R::Dining, R::Bedroom, R::MasterBedroom, R::Hallway, R::Office]).tags("art, painting, picture, frame, decor, wall").pack(),
        item("mirror-wall", "Wall mirror", A::Furniture, 490.0, 26.0, 745.0, 1100.0).mount(M::Wall).rooms(&[R::Bathroom, R::PowderRoom, R::Bedroom, R::MasterBedroom, R::Hallway]).tags("mirror, wall mirror, decor").pack(),
        item("shelf-cube", "Shelves, cube", A::Furniture, 1080.0, 370.0, 1555.0, 0.0).rooms(&[R::Living, R::Office, R::Bedroom, R::MasterBedroom]).tags("shelf, bookshelf, bookcase, cube, storage, modern").clear(600.0).pack(),
        item("shelf-steel", "Shelves, steel frame", A::Furniture, 1100.0, 500.0, 2140.0, 0.0).rooms(&[R::Storage, R::Garage, R::Laundry, R::Kitchen]).tags("shelf, rack, storage, steel, industrial, pantry").clear(600.0).pack(),
        item("basket-rattan", "Basket, rattan", A::Furniture, 375.0, 270.0, 220.0, 0.0).rooms(&[R::Laundry, R::Bathroom, R::Bedroom, R::MasterBedroom]).tags("basket, rattan, wicker, laundry, tropical").pack(),
        item("rocking-chair", "Rocking chair", A::Furniture, 710.0, 830.0, 995.0, 0.0).rooms(&[R::Living, R::Porch, R::Bedroom, R::MasterBedroom]).tags("chair, rocking chair, wood, classic").clear(400.0).pack(),
        // Dining. The sets' depth includes the chairs pulled up to the table.
        item("dining-round-2", "Round table, 2 seats", A::Furniture, 800.0, 800.0, 745.0, 0.0).rooms(&[R::Dining, R::Kitchen, R::Porch]).tags("table, dining table, round, wood, small").pack(),
        item("chair-dining", "Dining chair, leather", A::Furniture, 435.0, 575.0, 975.0, 0.0).rooms(&[R::Dining, R::Kitchen, R::Office]).tags("chair, dining chair, leather").pack(),
        item("chair-wood-white", "Chair, white wood", A::Furniture, 430.0, 540.0, 955.0, 0.0).rooms(&[R::Dining, R::Kitchen, R::Bedroom, R::MasterBedroom, R::Porch]).tags("chair, dining chair, wood, white, classic").pack(),
        item("chair-monobloc", "Chair, monobloc", A::Furniture, 640.0, 630.0, 880.0, 0.0).rooms(&[R::Porch, R::Garage, R::Dining, R::Kitchen]).tags("chair, monobloc, plastic, white, outdoor").pack(),
        item("stool-wood", "Stool, wood", A::Furniture, 480.0, 480.0, 750.0, 0.0).rooms(&[R::Kitchen, R::Dining]).tags("stool, bar stool, counter stool, wood").pack(),
        item("table-wood", "Table, wood", A::Furniture, 1200.0, 700.0, 760.0, 0.0).rooms(&[R::Dining, R::Kitchen, R::Office]).tags("table, dining table, desk, wood, simple").pack(),
        item("dining-set-4", "Dining set, 4 seats", A::Furniture, 1200.0, 1600.0, 955.0, 0.0).rooms(&[R::Dining, R::Kitchen]).tags("dining, dining set, table, chairs, 4 seats, wood").pack(),
        item("dining-set-6", "Dining set, 6 seats", A::Furniture, 1800.0, 1700.0, 955.0, 0.0).rooms(&[R::Dining]).tags("dining, dining set, table, chairs, 6 seats, wood").pack(),
        // Bedroom.
        item("nightstand", "Nightstand", A::Furniture, 505.0, 510.0, 615.0, 0.0).rooms(BED).tags("nightstand, bedside table, side table, wood").pack(),
        item("dresser", "Chest of drawers", A::Furniture, 1200.0, 580.0, 1210.0, 0.0).rooms(BED).tags("dresser, chest of drawers, drawers, cabinet, wood, classic").clear(600.0).pack(),
        item("cabinet-drawers", "Cabinet, drawers and shelves", A::Furniture, 1140.0, 490.0, 1880.0, 0.0).rooms(&[R::Bedroom, R::MasterBedroom, R::Living, R::Office, R::Kitchen]).tags("cabinet, drawers, shelves, storage, wood, modern").clear(600.0).pack(),
        item("desk-metal", "Office desk, steel", A::Furniture, 1600.0, 760.0, 760.0, 0.0).rooms(&[R::Office, R::Bedroom, R::MasterBedroom]).tags("desk, office desk, table, steel, work, study, industrial").clear(700.0).pack(),
        // Kitchen.
        item("range-electric", "Electric range", A::Appliance, 550.0, 650.0, 860.0, 0.0).rooms(&[R::Kitchen]).tags("stove, range, electric, oven, cooker, appliance").clear(900.0).pack(),
        item("microwave", "Microwave oven", A::Appliance, 500.0, 370.0, 300.0, 900.0).rooms(&[R::Kitchen]).tags("microwave, oven, appliance, countertop").pack(),
        item("cabinet-white", "Cabinet, painted white", A::Furniture, 1195.0, 615.0, 1180.0, 0.0).rooms(&[R::Kitchen, R::Dining, R::Laundry]).tags("cabinet, cupboard, painted, white, rustic, storage").clear(600.0).pack(),
        // Porch, garage and garden.
        item("outdoor-set", "Bistro table and two chairs", A::Furniture, 780.0, 1830.0, 860.0, 0.0).rooms(&[R::Porch]).tags("outdoor, table, chairs, bistro, folding, garden").pack(),
        item("bench-wood", "Bench, wood", A::Furniture, 1165.0, 500.0, 890.0, 0.0).rooms(&[R::Porch, R::Hallway]).tags("bench, wood, seat, entry, outdoor").clear(450.0).pack(),
        item("planter-box", "Planter box, wood", A::Plant, 915.0, 415.0, 425.0, 0.0).rooms(&[R::Porch]).tags("planter, planter box, wood, garden, outdoor").pack(),
        item("car-covered", "Car, under a cover", A::Vehicle, 1790.0, 4380.0, 1410.0, 0.0).rooms(&[R::Garage]).tags("car, vehicle, covered, carport").clear(600.0).pack(),
        item("tree-flowering", "Tree, flowering", A::Plant, 6800.0, 5300.0, 5500.0, 0.0).tags("tree, flowering, jacaranda, garden, outdoor, landscape").pack(),
        item("gate-iron", "Gate, iron, double swing", A::Site, 2950.0, 100.0, 2930.0, 0.0).tags("gate, iron, steel, double, swing, fence, entrance").pack(),
        lit(item("lamp-post-classic", "Lamp post, lantern", A::Site, 505.0, 280.0, 2800.0, 0.0).tags("lamp post, light, lantern, garden, street").pack(), 900.0, 2700.0),
        // Built-ins, sized in code (`Asset::built_in`): any width, depth and
        // height, with a sink and cooktop where the options put them.
        item("kitchen-run", "Kitchen run, base and wall cabinets", A::Kitchen, 2400.0, 600.0, 900.0, 0.0).built(B::KitchenRun).rooms(&[R::Kitchen]).tags("kitchen, counter, cabinets, sink, cooktop, built-in, modular").clear(900.0),
        item("wardrobe-builtin", "Built-in wardrobe", A::Furniture, 1800.0, 600.0, 2400.0, 0.0).built(B::Wardrobe).rooms(BED).tags("wardrobe, closet, built-in, cabinet, storage").clear(600.0),
        item("vanity", "Vanity with basin", A::Sanitary, 900.0, 500.0, 850.0, 0.0).built(B::Vanity).rooms(BATH).tags("vanity, sink, basin, lavatory, cabinet, built-in, mirror").clear(550.0),
        item("counter", "Counter, base cabinets", A::Kitchen, 1500.0, 600.0, 900.0, 0.0).built(B::Counter).rooms(&[R::Kitchen, R::Laundry, R::Dining]).tags("counter, cabinet, bar, built-in, laundry").clear(900.0),
        item("shelf-wall", "Wall shelves", A::Furniture, 1200.0, 250.0, 900.0, 1200.0).built(B::Shelf).mount(M::Wall).rooms(&[R::Living, R::Bedroom, R::MasterBedroom, R::Office, R::Kitchen]).tags("shelf, wall shelf, floating shelves, built-in"),
        // The site kit, drawn in code at any size. Paving is a deck slab
        // (`SlabKind::Deck`) with a paving material, not an object.
        item("gate-steel", "Gate, steel bars, sliding", A::Site, 3000.0, 100.0, 1800.0, 0.0).tags("gate, steel, sliding, fence, driveway, entrance"),
        item("hedge", "Hedge", A::Plant, 2000.0, 600.0, 1200.0, 0.0).tags("hedge, shrub, garden, fence, green, landscape"),
        lit(item("lamp-post", "Lamp post, modern", A::Site, 250.0, 250.0, 2800.0, 0.0).tags("lamp post, light, garden, bollard, outdoor"), 900.0, 3000.0),
    ]
}
