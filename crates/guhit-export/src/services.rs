//! Electrical, lighting and aircon objects on the service sheets: which
//! catalog items are devices, the plan symbols of the contract table
//! (docs/CONTRACT.md, "Devices, fixtures and links"), switch links, the room
//! an object stands in and the counts the sheets print.
//!
//! Symbols are built in the object's local frame in model mm: origin at the
//! footprint center, +x along the width, +y toward the back, which is the wall
//! face for wall objects. Their size comes from D, the symbol size (3 mm on
//! paper, so 300 mm at 1:100), never from the object, except where the table
//! says the symbol is the object's own rectangle (tube lights, aircon units).
//! Labels ("S3", "SPO", "WP") stay upright whatever the object's rotation.
//!
//! Guhit draws and counts devices. It never checks circuits, loads or
//! ratings: that is the work of the Professional Electrical Engineer.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

use guhit_model::{
    defaults, Asset, AssetCategory, CatalogItem, Derived, DeviceKind, Element, Id, LayerKey,
    Level, Project, Room, ScheduleGroup, ScheduleRow, SheetKind,
};

use crate::geom::*;
use crate::plan::{items_bounds, Cat, Fill, HAlign, Item, Pen, Prim};
use crate::text::clean;

/// Symbol size D on paper, mm. 3 mm is 300 mm at 1:100, the size the
/// contract table gives.
pub const SYMBOL_PAPER_MM: f64 = 3.0;

/// D in model mm at 1:n.
pub fn symbol_size(n: f64) -> f64 {
    SYMBOL_PAPER_MM * n
}

/// The built-in catalog, loaded once.
pub fn catalog() -> &'static [CatalogItem] {
    static CATALOG: OnceLock<Vec<CatalogItem>> = OnceLock::new();
    CATALOG.get_or_init(defaults::asset_catalog)
}

pub fn catalog_item(key: &str) -> Option<&'static CatalogItem> {
    catalog().iter().find(|c| c.key == key)
}

/// Display name of a catalog key: the catalog name, else the key itself.
pub fn catalog_name(key: &str) -> String {
    catalog_item(key)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| key.to_string())
}

/// The plan symbols of the contract table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Sym {
    CeilingLight,
    Pendant,
    Downlight,
    Tube,
    WallLight,
    OutdoorLight,
    Lamp,
    /// One to three gangs.
    Switch(u8),
    Outlet,
    OutdoorOutlet,
    Spo,
    AirconOutlet,
    Panelboard,
    SmokeDetector,
    PushButton,
    Chime,
    AcIndoor,
    AcOutdoor,
    AcWindow,
}

/// The symbol an object draws with, from its catalog key. Lighting objects
/// with an unknown key draw as a ceiling light; anything else unknown has no
/// symbol and draws as a plain outline.
pub fn symbol_of(a: &Asset) -> Option<Sym> {
    let key = a.catalog_key.as_str();
    let sym = match key {
        "light-ceiling" => Sym::CeilingLight,
        "light-pendant" => Sym::Pendant,
        "light-downlight" => Sym::Downlight,
        "light-tube" => Sym::Tube,
        "light-wall" => Sym::WallLight,
        "light-outdoor" => Sym::OutdoorLight,
        "light-floor-lamp" | "light-table-lamp" => Sym::Lamp,
        "outlet-duplex" | "outlet-counter" => Sym::Outlet,
        "outlet-outdoor" => Sym::OutdoorOutlet,
        "outlet-spo" => Sym::Spo,
        "outlet-aircon" => Sym::AirconOutlet,
        "switch-1" => Sym::Switch(1),
        "switch-2" => Sym::Switch(2),
        "switch-3" => Sym::Switch(3),
        "panelboard" => Sym::Panelboard,
        "smoke-detector" => Sym::SmokeDetector,
        "doorbell-button" => Sym::PushButton,
        "doorbell-chime" => Sym::Chime,
        "aircon-window" => Sym::AcWindow,
        k if k.starts_with("aircon-indoor") => Sym::AcIndoor,
        k if k.starts_with("aircon-outdoor") => Sym::AcOutdoor,
        _ if a.category == AssetCategory::Lighting => Sym::CeilingLight,
        _ => return None,
    };
    Some(sym)
}

impl Sym {
    /// The sheet the symbol belongs to.
    pub fn sheet(self) -> SheetKind {
        match self {
            Sym::CeilingLight
            | Sym::Pendant
            | Sym::Downlight
            | Sym::Tube
            | Sym::WallLight
            | Sym::OutdoorLight
            | Sym::Lamp
            | Sym::Switch(_) => SheetKind::Lighting,
            Sym::Outlet
            | Sym::OutdoorOutlet
            | Sym::Spo
            | Sym::AirconOutlet
            | Sym::Panelboard
            | Sym::SmokeDetector
            | Sym::PushButton
            | Sym::Chime => SheetKind::Power,
            Sym::AcIndoor | Sym::AcOutdoor | Sym::AcWindow => SheetKind::Aircon,
        }
    }

    pub fn is_light(self) -> bool {
        matches!(
            self,
            Sym::CeilingLight
                | Sym::Pendant
                | Sym::Downlight
                | Sym::Tube
                | Sym::WallLight
                | Sym::OutdoorLight
                | Sym::Lamp
        )
    }

    /// The project layer the object is on: electrical and lighting objects
    /// share `electrical`, aircon units `aircon`.
    pub fn layer_key(self) -> LayerKey {
        match self.sheet() {
            SheetKind::Aircon => LayerKey::Aircon,
            _ => LayerKey::Electrical,
        }
    }

    /// DXF layer, NCS style.
    pub fn dxf_layer(self) -> &'static str {
        if self.is_light() {
            LAYER_LITE_FIXT
        } else if self.sheet() == SheetKind::Aircon {
            LAYER_HVAC_EQPM
        } else {
            LAYER_POWR_DEVC
        }
    }

    /// The row of the PH electrical inspection form, or the aircon role.
    pub fn device_kind(self) -> DeviceKind {
        match self {
            s if s.is_light() => DeviceKind::LightingOutlet,
            Sym::Switch(_) => DeviceKind::Switch,
            Sym::Outlet | Sym::OutdoorOutlet => DeviceKind::ConvenienceReceptacle,
            Sym::Spo | Sym::AirconOutlet => DeviceKind::SpecialPurposeOutlet,
            Sym::Panelboard => DeviceKind::Panelboard,
            Sym::SmokeDetector => DeviceKind::SmokeDetector,
            Sym::PushButton => DeviceKind::PushButton,
            Sym::Chime => DeviceKind::Buzzer,
            Sym::AcIndoor => DeviceKind::AirconIndoor,
            Sym::AcOutdoor => DeviceKind::AirconOutdoor,
            _ => DeviceKind::AirconWindow,
        }
    }

    /// Mounted on a wall face (+y is the wall).
    pub fn on_wall(self) -> bool {
        matches!(
            self,
            Sym::WallLight
                | Sym::OutdoorLight
                | Sym::Switch(_)
                | Sym::Outlet
                | Sym::OutdoorOutlet
                | Sym::Spo
                | Sym::AirconOutlet
                | Sym::Panelboard
                | Sym::PushButton
                | Sym::Chime
                | Sym::AcIndoor
        )
    }

    /// Drawn from the object's own rectangle instead of D.
    pub fn own_size(self) -> bool {
        matches!(self, Sym::Tube | Sym::Panelboard | Sym::AcIndoor | Sym::AcOutdoor | Sym::AcWindow)
    }

    /// Legend wording.
    pub fn label(self, three_way: bool) -> String {
        let gang = |g: u8| match g {
            1 => "one gang",
            2 => "two gang",
            _ => "three gang",
        };
        match self {
            Sym::CeilingLight => "Ceiling light".into(),
            Sym::Pendant => "Pendant light".into(),
            Sym::Downlight => "Downlight".into(),
            Sym::Tube => "Tube light".into(),
            Sym::WallLight => "Wall light".into(),
            Sym::OutdoorLight => "Outdoor wall light, weatherproof".into(),
            Sym::Lamp => "Floor or table lamp, plug-in".into(),
            Sym::Switch(g) if three_way => format!("Three-way switch, {}", gang(g)),
            Sym::Switch(g) => format!("Switch, {}", gang(g)),
            Sym::Outlet => "Convenience outlet, duplex".into(),
            Sym::OutdoorOutlet => "Convenience outlet, weatherproof".into(),
            Sym::Spo => "Special purpose outlet".into(),
            Sym::AirconOutlet => "Aircon outlet, special purpose".into(),
            Sym::Panelboard => "Panelboard".into(),
            Sym::SmokeDetector => "Smoke detector".into(),
            Sym::PushButton => "Doorbell push button".into(),
            Sym::Chime => "Doorbell chime".into(),
            Sym::AcIndoor => "Split aircon, indoor unit".into(),
            Sym::AcOutdoor => "Split aircon, outdoor unit".into(),
            Sym::AcWindow => "Window aircon".into(),
        }
    }

    /// The object size a legend sample draws with, for symbols that use
    /// their own rectangle, as multiples of D.
    fn legend_size(self) -> (f64, f64) {
        match self {
            Sym::Tube => (2.2, 0.35),
            Sym::Panelboard => (1.2, 0.35),
            Sym::AcIndoor => (2.6, 0.8),
            Sym::AcOutdoor => (2.2, 0.9),
            Sym::AcWindow => (1.4, 1.1),
            _ => (0.3, 0.12),
        }
    }
}

pub const LAYER_LITE_FIXT: &str = "E-LITE-FIXT";
pub const LAYER_LITE_CIRC: &str = "E-LITE-CIRC";
pub const LAYER_POWR_DEVC: &str = "E-POWR-DEVC";
pub const LAYER_HVAC_EQPM: &str = "M-HVAC-EQPM";

/// The layer an object is on (docs/CONTRACT.md): lighting and electrical
/// objects `electrical`, aircon units `aircon`, everything else `assets`.
pub fn asset_layer(a: &Asset) -> LayerKey {
    if let Some(s) = symbol_of(a) {
        return s.layer_key();
    }
    match a.category {
        AssetCategory::Lighting | AssetCategory::Electrical => LayerKey::Electrical,
        AssetCategory::Aircon => LayerKey::Aircon,
        _ => LayerKey::Assets,
    }
}

/// The inspection form row of an object: from the catalog, or from its
/// symbol when the catalog does not know the key. Plug-in lamps are in the
/// catalog without a row, so they count as no device.
pub fn device_kind(a: &Asset) -> Option<DeviceKind> {
    match catalog_item(&a.catalog_key) {
        Some(c) => c.device,
        None => symbol_of(a).map(Sym::device_kind),
    }
}

/// What the DXF TYPE attribute says: the inspection form row, or the
/// catalog name for objects without one.
pub fn type_label(a: &Asset) -> String {
    match device_kind(a) {
        Some(k) => device_kind_label(k).to_string(),
        None => clean(&catalog_name(&a.catalog_key)).to_uppercase(),
    }
}

/// Upper case wording of a device kind, as the DXF TYPE attribute and the
/// count tables print it.
pub fn device_kind_label(k: DeviceKind) -> &'static str {
    match k {
        DeviceKind::LightingOutlet => "LIGHTING OUTLET",
        DeviceKind::ConvenienceReceptacle => "CONVENIENCE RECEPTACLE",
        DeviceKind::SpecialPurposeOutlet => "SPECIAL PURPOSE OUTLET",
        DeviceKind::Switch => "SWITCH",
        DeviceKind::Panelboard => "PANELBOARD",
        DeviceKind::SmokeDetector => "SMOKE DETECTOR",
        DeviceKind::Buzzer => "BUZZER",
        DeviceKind::PushButton => "PUSH BUTTON",
        DeviceKind::AirconIndoor => "AIRCON INDOOR UNIT",
        DeviceKind::AirconOutdoor => "AIRCON OUTDOOR UNIT",
        DeviceKind::AirconWindow => "WINDOW AIRCON",
    }
}

// ------------------------------------------------------------------ shapes

/// One stroke or fill of a symbol.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub prim: Prim,
    pub pen: Pen,
}

/// Where an upright label goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Anchor {
    /// Centered on this local point.
    At(V),
    /// Just right of the symbol on paper, centered on its height.
    Right,
    /// Beyond a local point in a local direction: the label's upright box
    /// starts `gap` past `from` and is centered on the line, whatever the
    /// object's rotation.
    Beyond { from: V, dir: V, gap: f64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub anchor: Anchor,
    pub text: String,
    /// Text height, model mm.
    pub size: f64,
    pub bold: bool,
    /// Dots in a row under the text (switch gangs). With dots the text sits
    /// above the anchor and the dots below it, so the pair is centered.
    pub dots: u8,
}

/// A symbol in the object's local frame.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shape {
    pub marks: Vec<Mark>,
    pub labels: Vec<Label>,
}

impl Shape {
    fn line(&mut self, a: V, b: V, pen: Pen) {
        self.marks.push(Mark { prim: Prim::Line { a, b }, pen });
    }

    fn circle(&mut self, c: V, r: f64, fill: Fill, pen: Pen) {
        self.marks.push(Mark { prim: Prim::Circle { c, r, fill }, pen });
    }

    fn poly(&mut self, pts: Vec<V>, fill: Fill, pen: Pen) {
        self.marks.push(Mark { prim: Prim::Poly { pts, closed: true, fill }, pen });
    }

    fn label(&mut self, anchor: Anchor, text: &str, size: f64, bold: bool) {
        self.labels.push(Label { anchor, text: text.into(), size, bold, dots: 0 });
    }
}

fn rect_pts(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<V> {
    vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)]
}

/// Half disk on the side of the circle away from +y (the room side).
fn half_disk(c: V, r: f64) -> Vec<V> {
    (0..=12)
        .map(|i| c + dir(180.0 + 180.0 * i as f64 / 12.0) * r)
        .collect()
}

/// Text heights in D. The shapes follow the 2D editor's table exactly
/// (`src/editor2d/symbols.ts`); its tags are 0.24 D tall, which prints at
/// 0.72 mm, so the sheet lifts text to a height that still reads on paper:
/// tags 0.4 D (1.2 mm), the switch letter 0.5 D (1.5 mm), "SD" 0.34 D
/// (1 mm) inside its circle and "CH" 0.26 D inside its square.
const TAG_H: f64 = 0.4;
const SWITCH_H: f64 = 0.5;
const INSIDE_H: f64 = 0.34;
const CHIME_H: f64 = 0.26;
/// Clear gap between a symbol's edge and its tag, in D (the editor's 0.07).
const TAG_GAP: f64 = 0.07;
/// Half the symbol pen (0.2 mm on paper) in D. On screen a stroke is a
/// pixel; on paper it takes a third of the gap, so every gap is measured
/// from the inked edge.
const INK: f64 = 0.034;

/// The symbol of the contract table in local model mm, drawn as the 2D
/// editor draws it (`deviceSymbol` in `src/editor2d/symbols.ts`). `w` and
/// `d` are the object's width and depth, `dd` is D.
pub fn shape(sym: Sym, w: f64, d: f64, dd: f64, three_way: bool) -> Shape {
    let mut s = Shape::default();
    // Legend samples are in paper mm, so the floor is relative to D.
    let floor = 1e-3 * dd.abs().max(1e-6);
    let (w, d) = (w.abs().max(floor), d.abs().max(floor));
    let hd = d / 2.0;
    let small = w.min(d);
    let tag_h = TAG_H * dd;
    let gap = (TAG_GAP + INK) * dd;
    let own = |s: &mut Shape| s.poly(rect_pts(-w / 2.0, -hd, w / 2.0, hd), Fill::None, Pen::Light);
    let x_lines = |s: &mut Shape, r: f64, pen: Pen| {
        let a = r * std::f64::consts::FRAC_1_SQRT_2;
        s.line(v(-a, -a), v(a, a), pen);
        s.line(v(-a, a), v(a, -a), pen);
    };
    // The half disc of the editor's `halfDisc`: the room side of a circle,
    // closed by its diameter.
    let half = |s: &mut Shape, c: V, r: f64, fill: Fill| s.poly(half_disk(c, r), fill, Pen::Light);
    match sym {
        Sym::CeilingLight | Sym::Pendant => {
            let r = 0.5 * dd;
            s.circle(v(0.0, 0.0), r, Fill::None, Pen::Light);
            x_lines(&mut s, r, Pen::Light);
            if sym == Sym::Pendant {
                s.label(Anchor::At(v(0.58 * dd, -0.44 * dd)), "P", TAG_H * dd, false);
            }
        }
        Sym::Downlight => {
            s.circle(v(0.0, 0.0), 0.3 * dd, Fill::None, Pen::Light);
            s.circle(v(0.0, 0.0), 0.07 * dd, Fill::Ink, Pen::Light);
        }
        Sym::Tube => {
            own(&mut s);
            if w >= d {
                s.line(v(-w / 2.0, 0.0), v(w / 2.0, 0.0), Pen::Light);
            } else {
                s.line(v(0.0, -hd), v(0.0, hd), Pen::Light);
            }
        }
        Sym::WallLight | Sym::OutdoorLight => {
            // Half disc on the wall face, and a radius square to the wall.
            let r = 0.5 * dd;
            half(&mut s, v(0.0, hd), r, Fill::None);
            s.line(v(0.0, hd), v(0.0, hd - r), Pen::Light);
            if sym == Sym::OutdoorLight {
                s.label(room_side(v(0.0, hd - r), gap), "WP", tag_h, false);
            }
        }
        Sym::Lamp => {
            s.circle(v(0.0, 0.0), 0.3 * dd, Fill::None, Pen::Fine);
            x_lines(&mut s, 0.3 * dd, Pen::Fine);
        }
        Sym::Switch(g) => {
            // "S" and its dots are one upright label beside the wall face.
            let text = if three_way { "S3" } else { "S" };
            s.labels.push(Label {
                anchor: room_side(v(0.0, hd), (SWITCH_GAP + INK) * dd),
                text: text.into(),
                size: SWITCH_H * dd,
                bold: true,
                dots: g.clamp(1, 3),
            });
        }
        Sym::Outlet | Sym::OutdoorOutlet | Sym::Spo | Sym::AirconOutlet => {
            // A circle touching the wall face, two lines parallel to the wall
            // through it; special purpose outlets fill the room side half.
            let r = 0.25 * dd;
            let c = v(0.0, hd - r);
            s.circle(c, r, Fill::None, Pen::Light);
            if matches!(sym, Sym::Spo | Sym::AirconOutlet) {
                half(&mut s, c, r, Fill::Ink);
            }
            for side in [-1.0, 1.0] {
                let y = c.y + side * 0.3 * r;
                s.line(v(-1.36 * r, y), v(1.36 * r, y), Pen::Light);
            }
            let text = match sym {
                Sym::OutdoorOutlet => "WP",
                Sym::Spo => "SPO",
                Sym::AirconOutlet => "ACO",
                _ => "",
            };
            if !text.is_empty() {
                s.label(room_side(v(0.0, c.y - r), gap), text, tag_h, false);
            }
        }
        Sym::Panelboard => {
            // Its own rectangle, half filled on the diagonal.
            own(&mut s);
            s.poly(vec![v(-w / 2.0, -hd), v(w / 2.0, -hd), v(w / 2.0, hd)], Fill::Ink, Pen::Light);
            s.label(room_side(v(0.0, -hd), gap), "PB", tag_h, false);
        }
        Sym::SmokeDetector => {
            s.circle(v(0.0, 0.0), 0.3 * dd, Fill::None, Pen::Light);
            s.label(Anchor::At(v(0.0, 0.0)), "SD", INSIDE_H * dd, false);
        }
        Sym::PushButton => {
            let r = 0.15 * dd;
            let c = v(0.0, hd - r);
            s.circle(c, r, Fill::None, Pen::Light);
            s.circle(c, 0.05 * dd, Fill::Ink, Pen::Light);
            s.label(room_side(v(0.0, c.y - r), gap), "PB", tag_h, false);
        }
        Sym::Chime => {
            let q = 0.5 * dd;
            s.poly(rect_pts(-q / 2.0, hd - q, q / 2.0, hd), Fill::None, Pen::Light);
            s.label(Anchor::At(v(0.0, hd - q / 2.0)), "CH", CHIME_H * dd, false);
        }
        Sym::AcIndoor => {
            // Its rectangle, an open arrow 0.6 D from the front into the
            // room, "ACU" inside.
            own(&mut s);
            let tip = -hd - 0.6 * dd;
            s.line(v(0.0, -hd), v(0.0, tip), Pen::Light);
            s.line(v(0.0, tip), v(-0.12 * dd, tip + 0.16 * dd), Pen::Light);
            s.line(v(0.0, tip), v(0.12 * dd, tip + 0.16 * dd), Pen::Light);
            s.label(Anchor::At(v(0.0, 0.0)), "ACU", (TAG_H * dd).min(0.55 * d), false);
        }
        Sym::AcOutdoor => {
            own(&mut s);
            s.circle(v(-0.16 * w, 0.0), (0.42 * d).min(0.26 * w), Fill::None, Pen::Light);
            s.label(Anchor::At(v(0.3 * w, 0.0)), "CU", (TAG_H * dd).min(0.5 * d), false);
        }
        Sym::AcWindow => {
            // The wall covers the unit's middle: "AC" sits inside against
            // its room side edge.
            own(&mut s);
            s.label(
                Anchor::Beyond { from: v(0.0, -hd), dir: v(0.0, 1.0), gap: (0.04 + INK) * dd },
                "AC",
                (TAG_H * dd).min(0.45 * small),
                false,
            );
        }
    }
    s
}

/// The shape a legend sample draws: the same symbol on a short wall above
/// it, with a fixed size for the symbols that use their own rectangle.
pub fn legend_shape(sym: Sym, dd: f64, three_way: bool) -> Shape {
    let (lw, ld) = sym.legend_size();
    shape(sym, lw * dd, ld * dd, dd, three_way)
}

/// Local y of the wall face a legend sample of a wall symbol shows.
pub fn legend_wall(sym: Sym, dd: f64) -> Option<f64> {
    sym.on_wall().then(|| sym.legend_size().1 * dd / 2.0)
}

/// Short tag of a plumbing fixture or utility, as the isometric diagrams
/// and the fixture table print it.
pub fn fixture_tag(key: &str) -> Option<&'static str> {
    Some(match key {
        "wc" => "WC",
        "lavatory" => "LAV",
        "shower" => "SH",
        "bathtub" => "BT",
        "floor-drain" => "FD",
        "water-heater" => "WH",
        "kitchen-sink" => "KS",
        "washing-machine" => "WSH",
        "water-meter" => "WM",
        "water-tank" => "WT",
        "septic-tank" => "ST",
        _ => return None,
    })
}

/// Where a link arc meets a symbol: a local point and the distance the arc
/// keeps from it.
pub fn link_point(sym: Sym, w: f64, d: f64, dd: f64) -> (V, f64) {
    let hd = d.abs() / 2.0;
    match sym {
        // The editor's anchors: where link arcs attach and the radius they
        // keep clear.
        Sym::CeilingLight | Sym::Pendant => (v(0.0, 0.0), 0.5 * dd),
        Sym::Downlight | Sym::Lamp | Sym::SmokeDetector => (v(0.0, 0.0), 0.3 * dd),
        Sym::Tube => (v(0.0, 0.0), w.abs().min(d.abs()) / 2.0),
        Sym::WallLight | Sym::OutdoorLight => (v(0.0, hd - 0.25 * dd), 0.3 * dd),
        Sym::Switch(_) => (v(0.0, hd - 0.65 * dd), 0.55 * dd),
        _ => (v(0.0, 0.0), 0.3 * dd),
    }
}

/// Clear gap between a wall face and the switch label, in D (the editor's).
const SWITCH_GAP: f64 = 0.08;

/// A label on the room side of a wall object: beyond `from`, away from the
/// wall (local -y), `gap` clear.
fn room_side(from: V, gap: f64) -> Anchor {
    Anchor::Beyond { from, dir: v(0.0, -1.0), gap }
}

/// Local to world: rotate by `rot_deg`, then move to `at`.
pub fn to_world(p: V, at: V, rot_deg: f64) -> V {
    at + dir(rot_deg) * p.x + dir(rot_deg + 90.0) * p.y
}

fn place_prim(p: &Prim, at: V, rot: f64) -> Prim {
    let t = |q: V| to_world(q, at, rot);
    match p {
        Prim::Line { a, b } => Prim::Line { a: t(*a), b: t(*b) },
        Prim::Poly { pts, closed, fill } => Prim::Poly {
            pts: pts.iter().map(|q| t(*q)).collect(),
            closed: *closed,
            fill: *fill,
        },
        Prim::Arc { c, r, start_deg, end_deg } => Prim::Arc {
            c: t(*c),
            r: *r,
            start_deg: norm_deg(start_deg + rot),
            end_deg: norm_deg(end_deg + rot),
        },
        Prim::Circle { c, r, fill } => Prim::Circle { c: t(*c), r: *r, fill: *fill },
        Prim::Text { pos, height, rot_deg, align, text, bold } => Prim::Text {
            pos: t(*pos),
            height: *height,
            rot_deg: rot_deg + rot,
            align: *align,
            text: text.clone(),
            bold: *bold,
        },
    }
}

fn item(prim: Prim, pen: Pen) -> Item {
    Item { cat: Cat::Furn, pen, prim }
}

/// Upright text centered on `c`.
pub fn centered_text(c: V, text: &str, size: f64, bold: bool) -> Prim {
    Prim::Text {
        pos: c - v(0.0, 0.36 * size),
        height: size,
        rot_deg: 0.0,
        align: HAlign::Middle,
        text: text.to_string(),
        bold,
    }
}

/// A symbol placed on the plan.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub marks: Vec<Mark>,
    /// Upright label texts, world coordinates.
    pub labels: Vec<Prim>,
}

impl Placed {
    pub fn bounds(&self) -> Bounds {
        let mut items: Vec<Item> = self.marks.iter().map(|m| item(m.prim.clone(), m.pen)).collect();
        items.extend(self.labels.iter().map(|l| item(l.clone(), Pen::Light)));
        items_bounds(&items)
    }
}

/// Put a local shape at a world position and rotation. Labels stay upright.
pub fn place(shape: &Shape, at: V, rot_deg: f64) -> Placed {
    let rot = if rot_deg.is_finite() { rot_deg } else { 0.0 };
    let marks: Vec<Mark> = shape
        .marks
        .iter()
        .map(|m| Mark { prim: place_prim(&m.prim, at, rot), pen: m.pen })
        .collect();
    let mark_items: Vec<Item> = marks.iter().map(|m| item(m.prim.clone(), m.pen)).collect();
    let mb = items_bounds(&mark_items);
    let mut marks = marks;
    let mut labels = Vec::new();
    for l in &shape.labels {
        // The label around (0, 0): text, and the dots under it for switches.
        let mut text = centered_text(v(0.0, 0.0), &l.text, l.size, l.bold);
        let mut dots: Vec<Mark> = Vec::new();
        if l.dots > 0 {
            // The editor's layout: the text at the top of the label's box,
            // the dots under it (radius 0.12, pitch 0.42 and a 0.24 gap, in
            // text heights).
            let h = l.size;
            text = centered_text(v(0.0, 0.24 * h), &l.text, h, l.bold);
            let g = l.dots as f64;
            for i in 0..l.dots {
                let x = (i as f64 - (g - 1.0) / 2.0) * 0.42 * h;
                dots.push(Mark {
                    prim: Prim::Circle { c: v(x, -0.62 * h), r: 0.12 * h, fill: Fill::Ink },
                    pen: Pen::Fine,
                });
            }
        }
        let mut group: Vec<Item> = vec![item(text.clone(), Pen::Light)];
        group.extend(dots.iter().map(|m| item(m.prim.clone(), m.pen)));
        let gb = items_bounds(&group);
        let at_ = match &l.anchor {
            Anchor::At(p) => to_world(*p, at, rot),
            Anchor::Right => {
                let (x, y) = if mb.is_empty() { (at.x, at.y) } else { (mb.max.x, mb.center().y) };
                v(x + 0.3 * l.size - gb.min.x, y - gb.center().y)
            }
            Anchor::Beyond { from, dir: d, gap } => {
                let p0 = to_world(*from, at, rot);
                let wd = (dir(rot) * d.x + dir(rot + 90.0) * d.y).unit().unwrap_or(v(0.0, -1.0));
                // How far the box reaches back toward `from` from its anchor,
                // widened for round capitals wider than the width estimate.
                let pad = v(0.05 * l.size, 0.0);
                let (lo, hi) = (gb.min - pad, gb.max + pad);
                let back = [lo, v(hi.x, lo.y), hi, v(lo.x, hi.y)]
                    .iter()
                    .map(|c| -c.dot(wd))
                    .fold(f64::NEG_INFINITY, f64::max);
                let side = wd.left();
                p0 + wd * (gap + back) - side * gb.center().dot(side)
            }
        };
        labels.push(crate::dxf::offset_prim(&text, at_));
        marks.extend(dots.into_iter().map(|m| Mark { prim: crate::dxf::offset_prim(&m.prim, at_), pen: m.pen }));
    }
    Placed { marks, labels }
}

// ------------------------------------------------------------------ devices

/// One object drawn as a symbol on a service sheet.
#[derive(Debug, Clone)]
pub struct Device<'a> {
    pub asset: &'a Asset,
    pub sym: Sym,
    /// A switch that shares a light with another switch reads "S3".
    pub three_way: bool,
    pub shape: Shape,
    pub placed: Placed,
}

/// Switches that share a light with another switch: they read "S3".
pub fn three_way_switches(project: &Project) -> HashSet<Id> {
    let mut by_light: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in &project.elements {
        if let Element::Asset(a) = e {
            if matches!(symbol_of(a), Some(Sym::Switch(_))) {
                let mut seen = HashSet::new();
                for l in &a.links {
                    if seen.insert(l.as_str()) {
                        by_light.entry(l.as_str()).or_default().push(a.id.as_str());
                    }
                }
            }
        }
    }
    let mut out = HashSet::new();
    for switches in by_light.values() {
        if switches.len() >= 2 {
            out.extend(switches.iter().map(|s| s.to_string()));
        }
    }
    out
}

/// The objects of one level that a sheet draws as symbols, in element order,
/// on visible layers.
pub fn devices_on<'a>(project: &'a Project, level: &Level, sheet: SheetKind, n: f64) -> Vec<Device<'a>> {
    let dd = symbol_size(n);
    let three = three_way_switches(project);
    project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Asset(a) if a.level_id == level.id => Some(a),
            _ => None,
        })
        .filter_map(|a| {
            let sym = symbol_of(a)?;
            if sym.sheet() != sheet || !crate::plan::layer_visible(project, sym.layer_key()) {
                return None;
            }
            if !(a.position.x.is_finite() && a.position.y.is_finite()) {
                return None;
            }
            let three_way = matches!(sym, Sym::Switch(_)) && three.contains(&a.id);
            let shape = shape(sym, a.width_mm, a.depth_mm, dd, three_way);
            let placed = place(&shape, V::from(a.position), a.rotation_deg);
            Some(Device { asset: a, sym, three_way, shape, placed })
        })
        .collect()
}

/// A dashed link arc as the editor draws one (`linkArc` and `trimLink` in
/// `src/editor2d/links.ts`): both ends trimmed along the straight line to
/// the radius their symbols keep clear, then an arc through the trimmed ends
/// whose middle stands 0.18 of their distance off the line, left of the
/// switch-to-light direction for `bow` 1 and right of it for -1. None when
/// the symbols touch.
pub fn link_arc(from: V, from_trim: f64, to: V, to_trim: f64, bow: i8) -> Option<Prim> {
    let full = (to - from).len();
    if !full.is_finite() || full <= from_trim + to_trim + 1.0 {
        return None;
    }
    let u = (to - from).unit()?;
    let a = from + u * from_trim;
    let b = to - u * to_trim;
    let c = (b - a).len();
    let side = if bow < 0 { -1.0 } else { 1.0 };
    let sag = 0.18 * c;
    let r = (c * c / 4.0 + sag * sag) / (2.0 * sag);
    let center = (a + b) * 0.5 + u.left() * (side * (sag - r));
    let (at_switch, at_light) = ((a - center).angle_deg(), (b - center).angle_deg());
    // Arcs run counter-clockwise: from the light end to the switch end when
    // they bow left, the other way round when they bow right.
    let (start, end) = if side > 0.0 { (at_light, at_switch) } else { (at_switch, at_light) };
    Some(Prim::Arc {
        c: center,
        r,
        start_deg: norm_deg(start),
        end_deg: norm_deg(end),
    })
}

/// The side a link bows to, as the editor picks it until someone flips it
/// on screen (`defaultBow`; flips are never saved): away from the middle of
/// the switch's other loads, left when it has none or they sit on the line.
pub fn default_bow(from: V, to: V, others: &[V]) -> i8 {
    if others.is_empty() {
        return 1;
    }
    let sum = others.iter().fold(v(0.0, 0.0), |acc, p| acc + *p);
    let mid = sum * (1.0 / others.len() as f64);
    let d = to - from;
    let s = d.left().dot(mid - from);
    if s.abs() < 1e-6 * d.dot(d).max(1.0) {
        1
    } else if s > 0.0 {
        -1
    } else {
        1
    }
}

/// Where links attach to an object, as the editor's `assetSymbolAnchor`
/// places it; None for objects the link tool does not link.
fn link_anchor(a: &Asset, dd: f64) -> Option<V> {
    let sym = symbol_of(a);
    let links = match sym {
        Some(s) => s.is_light() || matches!(s, Sym::Switch(_) | Sym::Spo | Sym::AirconOutlet | Sym::AcIndoor | Sym::AcWindow),
        None => a.category == AssetCategory::Lighting,
    };
    if !(links && a.position.x.is_finite() && a.position.y.is_finite()) {
        return None;
    }
    let p = sym.map_or(v(0.0, 0.0), |s| link_point(s, a.width_mm, a.depth_mm, dd).0);
    Some(to_world(p, V::from(a.position), a.rotation_deg))
}

/// Link arcs from every switch drawn to every light drawn that it controls.
pub fn link_arcs(project: &Project, devices: &[Device], n: f64) -> Vec<Prim> {
    let dd = symbol_size(n);
    let by_id: HashMap<&str, &Device> = devices.iter().map(|d| (d.asset.id.as_str(), d)).collect();
    // Every object that links, on any level: the editor weighs all of a
    // switch's loads when it picks the side a link bows to.
    let anchors: HashMap<&str, V> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Asset(a) => link_anchor(a, dd).map(|p| (a.id.as_str(), p)),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    for s in devices {
        if !matches!(s.sym, Sym::Switch(_)) {
            continue;
        }
        // From the middle of the switch label, clear of its box.
        let sb = s.placed.bounds();
        let (from, strim) = if sb.is_empty() {
            let (sp, t) = link_point(s.sym, s.asset.width_mm, s.asset.depth_mm, dd);
            (to_world(sp, V::from(s.asset.position), s.asset.rotation_deg), t)
        } else {
            (sb.center(), (sb.max - sb.min).len() / 2.0 + 0.08 * dd)
        };
        let mut seen = HashSet::new();
        for id in &s.asset.links {
            if !seen.insert(id.as_str()) {
                continue;
            }
            let Some(light) = by_id.get(id.as_str()) else { continue };
            if !light.sym.is_light() {
                continue;
            }
            let (lp, ltrim) = link_point(light.sym, light.asset.width_mm, light.asset.depth_mm, dd);
            let to = to_world(lp, V::from(light.asset.position), light.asset.rotation_deg);
            let others: Vec<V> = s
                .asset
                .links
                .iter()
                .filter(|o| *o != id)
                .filter_map(|o| anchors.get(o.as_str()).copied())
                .collect();
            if let Some(arc) = link_arc(from, strim, to, ltrim, default_bow(from, to, &others)) {
                out.push(arc);
            }
        }
    }
    out
}

// -------------------------------------------------------------------- rooms

/// Room polygons of the project, for the room an object stands in.
pub struct Rooms<'a> {
    list: Vec<(&'a Room, Vec<V>)>,
}

impl<'a> Rooms<'a> {
    pub fn new(project: &'a Project, derived: &Derived) -> Rooms<'a> {
        let list = project
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Room(r) => Some(r),
                _ => None,
            })
            .filter_map(|r| {
                let g = derived.rooms.iter().find(|g| g.room_id == r.id)?;
                let poly: Vec<V> = g.polygon.iter().map(V::from).collect();
                (poly.len() >= 3).then_some((r, poly))
            })
            .collect();
        Rooms { list }
    }

    /// The room of `level_id` whose polygon holds `p`.
    pub fn room_at(&self, level_id: &str, p: V) -> Option<&'a Room> {
        self.list
            .iter()
            .find(|(r, poly)| r.level_id == level_id && inside(p, poly))
            .map(|(r, _)| *r)
    }
}

pub fn inside(p: V, poly: &[V]) -> bool {
    let mut inside = false;
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (b.x - a.x) * (p.y - a.y) / (b.y - a.y) {
            inside = !inside;
        }
    }
    inside
}

/// The room name for tables: the room's name, "Room" when it has none.
pub fn room_name(project: &Project, room_id: Option<&Id>) -> String {
    match room_id {
        None => "Outside rooms".into(),
        Some(id) => project
            .elements
            .iter()
            .find_map(|e| match e {
                Element::Room(r) if &r.id == id => Some(clean(&r.name)),
                _ => None,
            })
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Room".into()),
    }
}

// ------------------------------------------------------------------- counts

/// Kitchen and laundry objects that plumbing fixture tables list, as the
/// engine counts them.
pub const PLUMBING_FIXTURE_KEYS: [&str; 2] = ["kitchen-sink", "washing-machine"];

/// Object counts per level and room: `Derived::schedule`, or, when the
/// derived data carries none, the same rows counted here with the contract
/// rule (catalog devices and sanitary, lighting, electrical, aircon and
/// utility objects).
pub fn schedule_rows(project: &Project, derived: &Derived) -> Vec<ScheduleRow> {
    if !derived.schedule.is_empty() {
        return derived.schedule.clone();
    }
    let rooms = Rooms::new(project, derived);
    // (level index, room index, catalog key) -> (group, device kind, count)
    type Key = (usize, Option<usize>, String);
    let mut counts: BTreeMap<Key, (ScheduleGroup, Option<DeviceKind>, u32)> = BTreeMap::new();
    let level_index: HashMap<&str, usize> = project
        .levels
        .iter()
        .enumerate()
        .map(|(i, l)| (l.id.as_str(), i))
        .collect();
    let room_index: HashMap<&str, usize> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Room(r) => Some(r.id.as_str()),
            _ => None,
        })
        .enumerate()
        .map(|(i, id)| (id, i))
        .collect();
    let mut room_ids: HashMap<usize, Id> = HashMap::new();
    for e in &project.elements {
        let Element::Asset(a) = e else { continue };
        let Some(&li) = level_index.get(a.level_id.as_str()) else { continue };
        let device = device_kind(a);
        let category = catalog_item(&a.catalog_key).map(|c| c.category).unwrap_or(a.category);
        // The engine's rule (guhit-core `schedule_group`): the kitchen and
        // laundry fixtures with water and a drain, then the category, then
        // the device kind.
        let group = if PLUMBING_FIXTURE_KEYS.contains(&a.catalog_key.as_str()) {
            ScheduleGroup::Plumbing
        } else {
            match (category, device) {
                (AssetCategory::Sanitary, _) => ScheduleGroup::Plumbing,
                (AssetCategory::Lighting | AssetCategory::Electrical, _) => ScheduleGroup::Electrical,
                (AssetCategory::Aircon, _) => ScheduleGroup::Aircon,
                (AssetCategory::Utility, _) => ScheduleGroup::Utility,
                (_, Some(DeviceKind::AirconIndoor | DeviceKind::AirconOutdoor | DeviceKind::AirconWindow)) => {
                    ScheduleGroup::Aircon
                }
                (_, Some(_)) => ScheduleGroup::Electrical,
                _ => continue,
            }
        };
        let room = rooms.room_at(&a.level_id, V::from(a.position)).or_else(|| {
            // Objects set into a wall (a window aircon) stand in the room
            // in front of them.
            let front = to_world(v(0.0, -a.depth_mm.abs() / 2.0 - 10.0), V::from(a.position), a.rotation_deg);
            rooms.room_at(&a.level_id, front)
        });
        let ri = room.and_then(|r| room_index.get(r.id.as_str()).copied());
        if let (Some(i), Some(r)) = (ri, room) {
            room_ids.insert(i, r.id.clone());
        }
        let entry = counts
            .entry((li, ri, a.catalog_key.clone()))
            .or_insert((group, device, 0));
        entry.2 += 1;
    }
    counts
        .into_iter()
        .map(|((li, ri, key), (group, device, count))| ScheduleRow {
            level_id: project.levels[li].id.clone(),
            room_id: ri.and_then(|i| room_ids.get(&i).cloned()),
            group,
            catalog_key: key,
            device,
            count,
        })
        .collect()
}

/// A table of counts: one row per room of a level (in element order, then
/// objects outside rooms), one column per device kind that has any.
#[derive(Debug, Clone, PartialEq)]
pub struct RoomCounts {
    pub columns: Vec<DeviceKind>,
    pub rows: Vec<(String, Vec<u32>)>,
    pub totals: Vec<u32>,
}

/// Counts per room of one level for the given device kinds.
pub fn room_counts(project: &Project, rows: &[ScheduleRow], level: &Level, kinds: &[DeviceKind]) -> RoomCounts {
    let of_level: Vec<&ScheduleRow> = rows
        .iter()
        .filter(|r| r.level_id == level.id)
        .filter(|r| r.device.is_some_and(|d| kinds.contains(&d)))
        .collect();
    let columns: Vec<DeviceKind> = kinds
        .iter()
        .copied()
        .filter(|k| of_level.iter().any(|r| r.device == Some(*k) && r.count > 0))
        .collect();
    // Rooms in element order, then the objects outside every room.
    let mut order: Vec<Option<Id>> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Room(r) if r.level_id == level.id => Some(Some(r.id.clone())),
            _ => None,
        })
        .collect();
    for r in &of_level {
        if !order.contains(&r.room_id) {
            order.push(r.room_id.clone());
        }
    }
    let mut out_rows = Vec::new();
    let mut totals = vec![0u32; columns.len()];
    for room in order {
        let counts: Vec<u32> = columns
            .iter()
            .map(|k| {
                of_level
                    .iter()
                    .filter(|r| r.room_id == room && r.device == Some(*k))
                    .map(|r| r.count)
                    .sum()
            })
            .collect();
        if counts.iter().all(|c| *c == 0) {
            continue;
        }
        for (t, c) in totals.iter_mut().zip(&counts) {
            *t += c;
        }
        out_rows.push((room_name(project, room.as_ref()), counts));
    }
    RoomCounts { columns, rows: out_rows, totals }
}

/// Short column heading of a device kind for the count tables.
pub fn device_kind_short(k: DeviceKind) -> &'static str {
    match k {
        DeviceKind::LightingOutlet => "LIGHTING OUTLETS",
        DeviceKind::ConvenienceReceptacle => "CONV. RECEPT.",
        DeviceKind::SpecialPurposeOutlet => "SPO",
        DeviceKind::Switch => "SWITCHES",
        DeviceKind::Panelboard => "PANEL",
        DeviceKind::SmokeDetector => "SMOKE DET.",
        DeviceKind::Buzzer => "BUZZER",
        DeviceKind::PushButton => "PUSH BUTTON",
        DeviceKind::AirconIndoor => "INDOOR",
        DeviceKind::AirconOutdoor => "OUTDOOR",
        DeviceKind::AirconWindow => "WINDOW",
    }
}

/// One line of the schedule of loads: a circuit tag and what is on it,
/// over every level. The rating, wire and breaker columns stay blank for the
/// PEE.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadRow {
    /// The free circuit tag, "-" for objects without one.
    pub circuit: String,
    pub description: String,
    pub count: u32,
}

fn natural_key(tag: &str) -> (String, u64, String) {
    let letters: String = tag.chars().take_while(|c| !c.is_ascii_digit()).collect();
    let rest = &tag[letters.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let number = digits.parse().unwrap_or(0);
    (letters.to_uppercase(), number, rest[digits.len()..].to_string())
}

/// Schedule of loads skeleton: one row per circuit tag, loads only (lights,
/// outlets, special purpose outlets, detectors and bells), untagged last.
pub fn load_rows(project: &Project) -> Vec<LoadRow> {
    let mut by_tag: BTreeMap<String, BTreeMap<DeviceKind, u32>> = BTreeMap::new();
    for e in &project.elements {
        let Element::Asset(a) = e else { continue };
        let Some(k) = device_kind(a) else { continue };
        if !matches!(
            k,
            DeviceKind::LightingOutlet
                | DeviceKind::ConvenienceReceptacle
                | DeviceKind::SpecialPurposeOutlet
                | DeviceKind::SmokeDetector
                | DeviceKind::Buzzer
        ) {
            continue;
        }
        let tag = clean(&a.circuit);
        *by_tag.entry(tag).or_default().entry(k).or_default() += 1;
    }
    let noun = |k: DeviceKind, n: u32| -> String {
        let (one, many) = match k {
            DeviceKind::LightingOutlet => ("lighting outlet", "lighting outlets"),
            DeviceKind::ConvenienceReceptacle => ("convenience outlet", "convenience outlets"),
            DeviceKind::SpecialPurposeOutlet => ("special purpose outlet", "special purpose outlets"),
            DeviceKind::SmokeDetector => ("smoke detector", "smoke detectors"),
            _ => ("bell", "bells"),
        };
        format!("{n} {}", if n == 1 { one } else { many })
    };
    let mut tags: Vec<String> = by_tag.keys().cloned().collect();
    tags.sort_by(|a, b| match (a.is_empty(), b.is_empty()) {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => natural_key(a).cmp(&natural_key(b)),
    });
    tags.into_iter()
        .map(|tag| {
            let kinds = &by_tag[&tag];
            let parts: Vec<String> = kinds.iter().map(|(k, n)| noun(*k, *n)).collect();
            let mut description = parts.join(", ");
            if let Some(first) = description.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            LoadRow {
                circuit: if tag.is_empty() { "-".into() } else { tag },
                description,
                count: kinds.values().sum(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_device_in_the_catalog_has_a_symbol_on_its_sheet() {
        for item in catalog() {
            let a = Asset {
                id: "a".into(),
                level_id: "l".into(),
                catalog_key: item.key.clone(),
                name: item.name.clone(),
                category: item.category,
                position: guhit_model::Point { x: 0.0, y: 0.0 },
                rotation_deg: 0.0,
                width_mm: item.width_mm,
                depth_mm: item.depth_mm,
                height_mm: item.height_mm,
                elevation_mm: item.elevation_mm,
                light: item.light,
                links: vec![],
                circuit: String::new(),
            };
            match item.device {
                Some(kind) => {
                    let sym = symbol_of(&a).unwrap_or_else(|| panic!("{} has no symbol", item.key));
                    assert_eq!(sym.device_kind(), kind, "{}", item.key);
                    assert_eq!(asset_layer(&a), sym.layer_key(), "{}", item.key);
                }
                None if item.category == AssetCategory::Lighting => {
                    assert_eq!(symbol_of(&a), Some(Sym::Lamp), "{}", item.key);
                }
                None => assert!(symbol_of(&a).is_none(), "{}", item.key),
            }
        }
    }

    #[test]
    fn link_arcs_bow_like_the_editor_and_stop_short_of_both_ends() {
        let from = v(0.0, 0.0);
        let to = v(3000.0, 0.0);
        let ends = |arc: &Prim| -> (V, V, V) {
            let Prim::Arc { c, r, start_deg, end_deg } = arc else { panic!("not an arc") };
            let mid = norm_deg(start_deg + norm_deg(end_deg - start_deg) / 2.0);
            (*c + dir(*start_deg) * *r, *c + dir(*end_deg) * *r, *c + dir(mid) * *r)
        };
        // Trimmed first along the line, then bowed by 0.18 of what is left:
        // 3000 - 150 - 170 = 2680 long, its middle 482.4 off the line.
        let left = link_arc(from, 150.0, to, 170.0, 1).expect("an arc");
        let (s, e, top) = ends(&left);
        assert!((s - v(2830.0, 0.0)).len() < 1e-6, "light end {s:?}");
        assert!((e - v(150.0, 0.0)).len() < 1e-6, "switch end {e:?}");
        assert!((top - v(1490.0, 0.18 * 2680.0)).len() < 1e-6, "{top:?}");
        let right = link_arc(from, 150.0, to, 170.0, -1).expect("an arc");
        let (s, e, top) = ends(&right);
        assert!((s - v(150.0, 0.0)).len() < 1e-6, "switch end {s:?}");
        assert!((e - v(2830.0, 0.0)).len() < 1e-6, "light end {e:?}");
        assert!((top - v(1490.0, -0.18 * 2680.0)).len() < 1e-6, "{top:?}");
        assert!(link_arc(from, 150.0, v(200.0, 0.0), 170.0, 1).is_none());
        // Left unless the switch's other loads are on the left.
        assert_eq!(default_bow(from, to, &[]), 1);
        assert_eq!(default_bow(from, to, &[v(1000.0, 800.0)]), -1);
        assert_eq!(default_bow(from, to, &[v(1000.0, -800.0)]), 1);
        assert_eq!(default_bow(from, to, &[v(1000.0, 800.0), v(1000.0, -800.0)]), 1);
    }

    #[test]
    fn two_switches_on_one_light_read_s3() {
        let mut p = defaults::new_project("t");
        let lvl = p.levels[0].id.clone();
        let mk = |id: &str, key: &str, links: &[&str]| {
            Element::Asset(Asset {
                id: id.into(),
                level_id: lvl.clone(),
                catalog_key: key.into(),
                name: key.into(),
                category: AssetCategory::Electrical,
                position: guhit_model::Point { x: 0.0, y: 0.0 },
                rotation_deg: 0.0,
                width_mm: 70.0,
                depth_mm: 40.0,
                height_mm: 115.0,
                elevation_mm: 1143.0,
                light: None,
                links: links.iter().map(|s| s.to_string()).collect(),
                circuit: String::new(),
            })
        };
        p.elements = vec![
            mk("s1", "switch-1", &["l1", "l2"]),
            mk("s2", "switch-2", &["l2"]),
            mk("s3", "switch-1", &["l3"]),
        ];
        let three = three_way_switches(&p);
        assert!(three.contains("s1") && three.contains("s2") && !three.contains("s3"));
    }

    #[test]
    fn load_rows_sort_tags_naturally_and_leave_untagged_last() {
        let mut p = defaults::new_project("t");
        let lvl = p.levels[0].id.clone();
        let mk = |key: &str, circuit: &str| {
            Element::Asset(Asset {
                id: format!("{key}-{circuit}"),
                level_id: lvl.clone(),
                catalog_key: key.into(),
                name: key.into(),
                category: AssetCategory::Electrical,
                position: guhit_model::Point { x: 0.0, y: 0.0 },
                rotation_deg: 0.0,
                width_mm: 70.0,
                depth_mm: 40.0,
                height_mm: 115.0,
                elevation_mm: 243.0,
                light: None,
                links: vec![],
                circuit: circuit.into(),
            })
        };
        p.elements = vec![
            mk("outlet-duplex", "C10"),
            mk("outlet-duplex", "C2"),
            mk("light-ceiling", "L1"),
            mk("switch-1", "L1"),
            mk("outlet-spo", ""),
        ];
        let rows = load_rows(&p);
        let tags: Vec<&str> = rows.iter().map(|r| r.circuit.as_str()).collect();
        assert_eq!(tags, ["C2", "C10", "L1", "-"]);
        assert_eq!(rows[2].description, "1 lighting outlet");
        assert_eq!(rows[2].count, 1, "switches are not loads");
    }
}
