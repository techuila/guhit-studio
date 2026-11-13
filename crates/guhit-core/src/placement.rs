//! Placing furniture and fixtures (lane L5, DECISIONS D54): free spots in a
//! room (`Query::PlacementOptions`), the catalog search (`Query::Catalog`),
//! the object review items (`asset_in_wall`, `assets_overlap`, and objects
//! partly over a void or on a stair), built-in options, and the furnishing
//! plan the copilot ("Furnish rooms", D8) and agents (D47) share.
//!
//! One rule set decides where an object may stand, for spots, furnishing
//! and review alike:
//! - inside the room's floor, clear of walls, voids, stair openings and
//!   stair flights (G4 problem 2: never offered, flagged when placed);
//! - clear of every doorway and the sweep of its leaves;
//! - under a window only when lower than its sill;
//! - clear of other objects at the same height;
//! - its own front clearance (`CatalogItem::clearance_mm`) free, and
//!   nobody else's taken.
//!
//! Everything is a suggestion, like every review item. Nothing here sizes
//! anything for a code.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use guhit_model::*;
use serde_json::{json, Value};

use crate::error::CoreError;
use crate::geom::*;
use crate::issues::issue;
use crate::slabs::{overlap_area, stair_rect, stair_top_level};
use crate::topo::{analyze, Analysis};

/// Spots are tried every this far along a wall.
const STEP_MM: f64 = 50.0;
/// A floor object stands this far off the wall face.
const WALL_GAP_MM: f64 = 10.0;
/// Kept clear in front of a doorway, on both sides of its wall.
const DOOR_PASS_MM: f64 = 900.0;
/// A doorway's clear zone runs this far past each jamb.
const DOOR_JAMB_MM: f64 = 50.0;
/// The band along a wall face where an object sits under a window.
const WINDOW_BAND_MM: f64 = 150.0;
/// Overlap smaller than this is a touch, not a clash (0.01 m2).
const TOUCH_MM2: f64 = 10_000.0;
/// What a wall may take of an object's footprint before it is "in" it.
const IN_WALL_MM2: f64 = 5_000.0;
/// A freestanding table keeps this much around it for walking.
const AROUND_MM: f64 = 450.0;
/// Front clearance reaches this high: a wall shelf above it does not block it.
const FRONT_TOP_MM: f64 = 1_200.0;
/// At most this many spots are listed for one object.
const MAX_OPTIONS: usize = 48;

/// Kitchen wall cabinets: from the backsplash top to this, this deep at the back.
pub const UPPER_Z0_MM: f64 = 1_450.0;
pub const UPPER_Z1_MM: f64 = 2_150.0;
pub const UPPER_DEPTH_MM: f64 = 350.0;
/// A vanity's mirror cabinet.
pub const MIRROR_Z0_MM: f64 = 1_150.0;
pub const MIRROR_Z1_MM: f64 = 1_850.0;
pub const MIRROR_DEPTH_MM: f64 = 140.0;

/// The catalog, built once.
fn catalog_items() -> &'static [CatalogItem] {
    static CATALOG: OnceLock<Vec<CatalogItem>> = OnceLock::new();
    CATALOG.get_or_init(defaults::asset_catalog)
}

fn catalog_item(key: &str) -> Option<&'static CatalogItem> {
    catalog_items().iter().find(|c| c.key == key)
}

// ------------------------------------------------------------- built-ins

fn round50(v: f64) -> f64 {
    (v / 50.0).round() * 50.0
}

fn clamp_to(v: f64, lo: f64, hi: f64) -> f64 {
    v.max(lo).min(hi.max(lo))
}

/// The options of a built-in that carries none: a kitchen run's sink 30
/// percent along from the left end (seen from the front) and its cooktop at
/// three quarters, at least 900 mm on; a vanity's basin in the middle under
/// a mirror cabinet; about one door per 450 mm (500 for a wardrobe). The 3D
/// view and the plan symbol use the same rules (`builtInDefaults` in
/// src/viewer3d/scene/builtins.ts).
pub fn built_in_defaults(kind: BuiltInKind, width_mm: f64) -> BuiltInParams {
    let w = if width_mm.is_finite() && width_mm > 0.0 { width_mm } else { 600.0 };
    let doors = |per: f64, min: u32| ((w / per).round() as u32).max(min);
    match kind {
        BuiltInKind::KitchenRun => {
            let sink = if w >= 900.0 { round50(clamp_to(w * 0.3, 450.0, w - 450.0)) } else { round50(w / 2.0) };
            let mut cooktop = None;
            if w >= 1800.0 {
                let at = round50(clamp_to((w * 0.75).max(sink + 900.0), 350.0, w - 350.0));
                if (at - sink).abs() >= 600.0 {
                    cooktop = Some(at);
                }
            }
            BuiltInParams { upper: true, sink_at_mm: Some(sink), cooktop_at_mm: cooktop, doors: doors(450.0, 1) }
        }
        BuiltInKind::Vanity => BuiltInParams { upper: true, sink_at_mm: Some(round50(w / 2.0)), cooktop_at_mm: None, doors: doors(450.0, 1) },
        BuiltInKind::Counter => BuiltInParams { upper: false, sink_at_mm: None, cooktop_at_mm: None, doors: doors(450.0, 1) },
        BuiltInKind::Wardrobe => BuiltInParams { upper: false, sink_at_mm: None, cooktop_at_mm: None, doors: doors(500.0, 2) },
        BuiltInKind::Shelf => BuiltInParams { upper: false, sink_at_mm: None, cooktop_at_mm: None, doors: 0 },
    }
}

/// A built-in's kind and the options it is drawn with: its own, or the
/// defaults for its width. None for an object that is not a built-in.
pub fn built_in_of(a: &Asset) -> Option<(BuiltInKind, BuiltInParams)> {
    let kind = catalog_item(&a.catalog_key)?.parametric?;
    Some((kind, a.built_in.as_deref().copied().unwrap_or_else(|| built_in_defaults(kind, a.width_mm))))
}

/// A built-in's options (validate hub, part of `Element::Asset`): only
/// built-ins take them, a sink and a cooktop sit on the run and at least
/// 600 mm apart, and the door count fits the width.
pub fn validate_built_in(_project: &Project, a: &Asset) -> Result<(), CoreError> {
    let Some(p) = a.built_in.as_deref() else { return Ok(()) };
    let ids = vec![a.id.clone()];
    let bad = |msg: String| Err(CoreError::invalid_for("built_in_options", msg, ids.clone()));
    let Some(kind) = catalog_item(&a.catalog_key).and_then(|c| c.parametric) else {
        return bad(format!("{} is not a built-in, so it takes no built-in options.", a.name));
    };
    let w = a.width_mm;
    if let Some(s) = p.sink_at_mm {
        if !matches!(kind, BuiltInKind::KitchenRun | BuiltInKind::Vanity | BuiltInKind::Counter) {
            return bad(format!("{} has no sink. Only kitchen runs, counters and vanities do.", a.name));
        }
        if !s.is_finite() || s < 150.0 || s > w - 150.0 {
            return bad(format!("The sink of {} goes 150 to {:.0} mm from its left end, not {s:.0}.", a.name, (w - 150.0).max(150.0)));
        }
    }
    if let Some(c) = p.cooktop_at_mm {
        if !matches!(kind, BuiltInKind::KitchenRun | BuiltInKind::Counter) {
            return bad(format!("{} has no cooktop. Only kitchen runs and counters do.", a.name));
        }
        if !c.is_finite() || c < 300.0 || c > w - 300.0 {
            return bad(format!("The cooktop of {} goes 300 to {:.0} mm from its left end, not {c:.0}.", a.name, (w - 300.0).max(300.0)));
        }
        if let Some(s) = p.sink_at_mm {
            if (s - c).abs() < 600.0 {
                return bad(format!("Keep the sink and the cooktop of {} at least 600 mm apart; they are {:.0} mm apart.", a.name, (s - c).abs()));
            }
        }
    }
    let most = ((w / 250.0).floor() as u32).max(1);
    if p.doors > most {
        return bad(format!("{} fits at most {most} doors across its {w:.0} mm, not {}.", a.name, p.doors));
    }
    Ok(())
}

// ------------------------------------------------------------- geometry

/// The corners of a `w` x `d` box centered on `c`, turned `rot` degrees,
/// counter-clockwise.
fn oriented_rect(c: Point, w: f64, d: f64, rot: f64) -> Vec<Point> {
    let (s, k) = rot.to_radians().sin_cos();
    let at = |x: f64, y: f64| pt(c.x + x * k - y * s, c.y + x * s + y * k);
    vec![at(-w / 2.0, -d / 2.0), at(w / 2.0, -d / 2.0), at(w / 2.0, d / 2.0), at(-w / 2.0, d / 2.0)]
}

/// Local +x (along the width) and local +y (toward the back) in plan.
fn frame(rot: f64) -> (Point, Point) {
    let (s, k) = rot.to_radians().sin_cos();
    (pt(k, s), pt(-s, k))
}

/// The rotation that puts an object's back (+y) against a wall whose room
/// side is `n`.
fn rotation_facing(n: Point) -> f64 {
    let deg = n.x.atan2(-n.y).to_degrees();
    let deg = (deg % 360.0 + 360.0) % 360.0;
    if (deg - 360.0).abs() < 1e-9 {
        0.0
    } else {
        (deg * 1e6).round() / 1e6
    }
}

fn area(poly: &[Point]) -> f64 {
    signed_area(poly).abs()
}

fn overlaps(a: &[Point], b: &[Point]) -> f64 {
    let (alo, ahi) = bbox(a);
    let (blo, bhi) = bbox(b);
    if alo.x >= bhi.x || blo.x >= ahi.x || alo.y >= bhi.y || blo.y >= ahi.y {
        return 0.0;
    }
    overlap_area(a, b)
}

fn round_pt(p: Point) -> Point {
    pt((p.x * 10.0).round() / 10.0, (p.y * 10.0).round() / 10.0)
}

// ------------------------------------------------------------- the rules

/// A box an object fills: plan outline and height range above the floor.
#[derive(Debug, Clone)]
struct Solid {
    outline: Vec<Point>,
    z0: f64,
    z1: f64,
}

/// What the rules need of an object, placed or not.
#[derive(Debug, Clone)]
struct Spec {
    key: String,
    name: String,
    w: f64,
    d: f64,
    h: f64,
    elevation: f64,
    mount: Mount,
    clearance: f64,
    /// Outlets, switches and detectors take part in no overlap.
    device: bool,
    built: Option<(BuiltInKind, BuiltInParams)>,
}

impl Spec {
    fn of_item(it: &CatalogItem) -> Spec {
        Spec {
            key: it.key.clone(),
            name: it.name.clone(),
            w: it.width_mm,
            d: it.depth_mm,
            h: it.height_mm,
            elevation: it.elevation_mm,
            mount: it.mount,
            clearance: it.clearance_mm,
            device: it.device.is_some() || it.category == AssetCategory::Electrical,
            built: it.parametric.map(|k| (k, built_in_defaults(k, it.width_mm))),
        }
    }

    fn of_asset(a: &Asset) -> Spec {
        let item = catalog_item(&a.catalog_key);
        Spec {
            key: a.catalog_key.clone(),
            name: a.name.clone(),
            w: a.width_mm,
            d: a.depth_mm,
            h: a.height_mm,
            elevation: a.elevation_mm,
            mount: item.map(|i| i.mount).unwrap_or_default(),
            clearance: item.map(|i| i.clearance_mm).unwrap_or(0.0),
            device: item.map(|i| i.device.is_some()).unwrap_or(false) || a.category == AssetCategory::Electrical,
            built: built_in_of(a),
        }
    }

    fn with_width(&self, w: f64) -> Spec {
        let mut s = self.clone();
        s.w = w;
        if let Some((k, p)) = s.built {
            let mut fresh = built_in_defaults(k, w);
            fresh.upper = p.upper;
            s.built = Some((k, fresh));
        }
        s
    }

    /// Standing on the floor (not hung, not buried).
    fn on_floor(&self) -> bool {
        self.mount == Mount::Floor && self.elevation >= 0.0
    }

    fn solids(&self, c: Point, rot: f64) -> Vec<Solid> {
        // A tree is its trunk: the crown may hang over a roof, a fence or a
        // bench, and nothing stands in it.
        if self.key == "tree" || self.key.starts_with("tree-") {
            let t = (self.w.min(self.d) * 0.08).max(200.0);
            return vec![Solid { outline: oriented_rect(c, t, t, rot), z0: self.elevation, z1: self.elevation + self.h * 0.5 }];
        }
        let base = Solid { outline: oriented_rect(c, self.w, self.d, rot), z0: self.elevation, z1: self.elevation + self.h };
        let mut out = vec![base];
        // Wall cabinets and mirror cabinets are their own boxes, so a
        // microwave on the counter does not clash with them.
        if let Some((kind, p)) = self.built {
            let (_, back) = frame(rot);
            let band = |depth: f64, z0: f64, z1: f64| {
                let depth = depth.min(self.d);
                let at = add(c, scale(back, self.d / 2.0 - depth / 2.0));
                Solid { outline: oriented_rect(at, self.w, depth, rot), z0, z1 }
            };
            match kind {
                BuiltInKind::KitchenRun if p.upper => out.push(band(UPPER_DEPTH_MM, UPPER_Z0_MM, UPPER_Z1_MM)),
                BuiltInKind::Vanity if p.upper => out.push(band(MIRROR_DEPTH_MM, MIRROR_Z0_MM, MIRROR_Z1_MM)),
                _ => {}
            }
        }
        out
    }

    /// The free space it needs in front, on the floor.
    fn front(&self, c: Point, rot: f64) -> Option<Vec<Point>> {
        if self.clearance <= 0.0 || !self.on_floor() {
            return None;
        }
        let (_, back) = frame(rot);
        let at = sub(c, scale(back, self.d / 2.0 + self.clearance / 2.0));
        Some(oriented_rect(at, self.w, self.clearance, rot))
    }
}

/// An object standing on the level.
#[derive(Debug, Clone)]
struct Body {
    id: Id,
    name: String,
    spec: Spec,
    center: Point,
    rot: f64,
    solids: Vec<Solid>,
    front: Option<Vec<Point>>,
}

impl Body {
    fn new(id: Id, spec: Spec, center: Point, rot: f64) -> Body {
        Body {
            id,
            name: spec.name.clone(),
            solids: spec.solids(center, rot),
            front: spec.front(center, rot),
            spec,
            center,
            rot,
        }
    }

    fn of_asset(a: &Asset) -> Body {
        Body::new(a.id.clone(), Spec::of_asset(a), a.position, a.rotation_deg)
    }

    fn back_dir(&self) -> Point {
        frame(self.rot).1
    }

    fn along(&self) -> Point {
        frame(self.rot).0
    }

    /// The point on the wall face right behind it.
    fn wall_point(&self) -> Point {
        add(self.center, scale(self.back_dir(), self.spec.d / 2.0 + WALL_GAP_MM))
    }

    fn top(&self) -> f64 {
        self.solids.iter().map(|s| s.z1).fold(f64::MIN, f64::max)
    }
}

/// Floor nobody can stand on.
#[derive(Debug, Clone)]
struct Hole {
    id: Id,
    outline: Vec<Point>,
    /// "a void", "the opening of a stair", "a stair".
    what: &'static str,
}

/// A doorway's clear zone or a window's band, with its height range.
#[derive(Debug, Clone)]
struct Zone {
    outline: Vec<Point>,
    z0: f64,
    z1: f64,
}

/// Why a spot is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Why {
    Outside,
    Wall,
    Void,
    Stair,
    Door,
    Window,
    Object,
    Clearance,
}

impl Why {
    fn text(self) -> &'static str {
        match self {
            Why::Outside => "it does not fit inside the room",
            Why::Wall => "a wall is in the way",
            Why::Void => "a void is in the way",
            Why::Stair => "a stair or its opening is in the way",
            Why::Door => "doorways and door swings are in the way",
            Why::Window => "windows are lower than its top",
            Why::Object => "other objects are in the way",
            Why::Clearance => "the space in front of it, or of another object, is taken",
        }
    }
}

/// A room's floor, as the rules see it.
#[derive(Debug, Clone)]
struct RoomView {
    id: Id,
    name: String,
    usage: RoomUsage,
    level_id: Id,
    /// Counter-clockwise on the inner wall faces.
    net: Vec<Point>,
    /// Wall islands inside it.
    islands: Vec<Vec<Point>>,
    area_mm2: f64,
    label: Point,
}

fn room_view(project: &Project, analysis: &Analysis, room_id: &Id) -> Result<RoomView, CoreError> {
    let room = project
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Room(r) if &r.id == room_id => Some(r),
            _ => None,
        })
        .ok_or_else(|| CoreError::NotFound(room_id.clone()))?;
    let face = analysis
        .face_at(&room.level_id, room.seed)
        .and_then(|i| analysis.level(&room.level_id).map(|l| &l.faces[i]))
        .ok_or_else(|| {
            CoreError::invalid_for(
                "room_not_closed",
                format!("{} has no closed walls around it, so it has no floor to place things on.", room.name),
                vec![room.id.clone()],
            )
        })?;
    let mut net = face.net.clone();
    if signed_area(&net) < 0.0 {
        net.reverse();
    }
    Ok(RoomView {
        id: room.id.clone(),
        name: room.name.clone(),
        usage: room.usage,
        level_id: room.level_id.clone(),
        area_mm2: area(&net) - face.holes.iter().map(|h| area(h)).sum::<f64>(),
        islands: face.holes.clone(),
        label: face.label,
        net,
    })
}

/// Everything on one level the rules test against.
#[derive(Debug, Clone)]
struct Scene {
    walls: Vec<(Id, Vec<Point>, f64)>,
    /// Wall centerlines on the level, for naming the wall a spot is on.
    wall_lines: Vec<(Id, Point, Point, f64)>,
    holes: Vec<Hole>,
    doors: Vec<Zone>,
    windows: Vec<Zone>,
    bodies: Vec<Body>,
}

/// One swinging leaf's sweep, as `slabs::swing_areas` draws it.
fn swing_sectors(w: &Wall, o: &Opening) -> Vec<Vec<Point>> {
    let Some(u) = unit(sub(w.end, w.start)) else { return vec![] };
    let nrm = if o.flip_side { scale(perp_left(u), -1.0) } else { perp_left(u) };
    let c = add(w.start, scale(u, o.offset_mm));
    let half = o.width_mm / 2.0;
    let leaves: Vec<(Point, Point, f64)> = match o.style {
        OpeningStyle::SwingDouble => vec![(sub(c, scale(u, half)), u, half), (add(c, scale(u, half)), scale(u, -1.0), half)],
        OpeningStyle::SwingSingle | OpeningStyle::Casement => {
            if o.flip_hinge {
                vec![(add(c, scale(u, half)), scale(u, -1.0), o.width_mm)]
            } else {
                vec![(sub(c, scale(u, half)), u, o.width_mm)]
            }
        }
        _ => vec![],
    };
    leaves
        .into_iter()
        .map(|(hinge, along, r)| {
            let hp = add(hinge, scale(nrm, w.thickness_mm / 2.0));
            let mut sector = vec![hp];
            for k in 0..=8 {
                let t = (k as f64 / 8.0) * std::f64::consts::FRAC_PI_2;
                sector.push(add(hp, scale(add(scale(along, t.cos()), scale(nrm, t.sin())), r)));
            }
            sector
        })
        .collect()
}

/// A band along the opening on one face of its wall, `depth` into the room.
fn face_band(w: &Wall, o: &Opening, side: f64, depth: f64, extra: f64) -> Option<Vec<Point>> {
    let u = unit(sub(w.end, w.start))?;
    let n = scale(perp_left(u), side);
    let c = add(w.start, scale(u, o.offset_mm));
    let half = o.width_mm / 2.0 + extra;
    let face = w.thickness_mm / 2.0;
    let a = add(sub(c, scale(u, half)), scale(n, face));
    let b = add(add(c, scale(u, half)), scale(n, face));
    let poly = vec![a, b, add(b, scale(n, depth)), add(a, scale(n, depth))];
    Some(if signed_area(&poly) < 0.0 { poly.into_iter().rev().collect() } else { poly })
}

impl Scene {
    fn new(project: &Project, analysis: &Analysis, level_id: &str) -> Scene {
        let walls_here: Vec<&Wall> = project
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Wall(w) if w.level_id == level_id => Some(w),
                _ => None,
            })
            .collect();
        let level_h = project.levels.iter().find(|l| l.id == level_id).map(|l| l.height_mm).unwrap_or(defaults::DEFAULT_LEVEL_HEIGHT_MM);
        let walls = walls_here
            .iter()
            .filter_map(|w| analysis.wall(&w.id).map(|t| (w.id.clone(), t.outline.clone(), w.height_mm.unwrap_or(level_h))))
            .filter(|(_, o, _)| o.len() >= 3)
            .collect();
        let wall_lines = walls_here.iter().map(|w| (w.id.clone(), w.start, w.end, w.thickness_mm)).collect();

        let mut doors = vec![];
        let mut windows = vec![];
        for e in &project.elements {
            let Element::Opening(o) = e else { continue };
            let Some(w) = walls_here.iter().find(|w| w.id == o.wall_id) else { continue };
            let top = o.sill_mm + o.height_mm;
            if o.opening_type == OpeningType::Door {
                let swings = matches!(o.style, OpeningStyle::SwingSingle | OpeningStyle::SwingDouble | OpeningStyle::Casement);
                for side in [1.0, -1.0] {
                    let swing_side = if o.flip_side { -1.0 } else { 1.0 };
                    let depth = if swings && side == swing_side { DOOR_PASS_MM.max(o.width_mm) } else { DOOR_PASS_MM };
                    if let Some(band) = face_band(w, o, side, depth, DOOR_JAMB_MM) {
                        doors.push(Zone { outline: band, z0: 0.0, z1: top.max(2_000.0) });
                    }
                }
                for s in swing_sectors(w, o) {
                    doors.push(Zone { outline: s, z0: 0.0, z1: top.max(2_000.0) });
                }
            } else {
                for side in [1.0, -1.0] {
                    if let Some(band) = face_band(w, o, side, WINDOW_BAND_MM, 0.0) {
                        windows.push(Zone { outline: band, z0: o.sill_mm, z1: top });
                    }
                }
            }
        }

        let mut holes = vec![];
        for e in &project.elements {
            if let Element::Slab(s) = e {
                if s.kind == SlabKind::Void && s.level_id == level_id && s.polygon.len() >= 3 {
                    holes.push(Hole { id: s.id.clone(), outline: s.polygon.clone(), what: "a void" });
                }
            }
        }
        // Stairs: the engine's geometry when lane L2's stairs are built, the
        // straight flight's rectangle until then.
        let geometry = crate::stairs::derive(project, analysis);
        for e in &project.elements {
            let Element::Stair(s) = e else { continue };
            let top = stair_top_level(project, s).map(|l| l.id.clone());
            match geometry.iter().find(|g| g.stair_id == s.id) {
                Some(g) => {
                    if g.level_id == level_id {
                        for f in &g.flights {
                            holes.push(Hole { id: s.id.clone(), outline: f.outline.clone(), what: "a stair" });
                        }
                        for l in &g.landings {
                            holes.push(Hole { id: s.id.clone(), outline: l.polygon.clone(), what: "a stair" });
                        }
                    }
                    if g.arrives_at.as_deref() == Some(level_id) && g.opening.len() >= 3 {
                        holes.push(Hole { id: s.id.clone(), outline: g.opening.clone(), what: "the opening of a stair" });
                    }
                }
                None => {
                    if s.level_id == level_id {
                        holes.push(Hole { id: s.id.clone(), outline: stair_rect(s).to_vec(), what: "a stair" });
                    }
                    if top.as_deref() == Some(level_id) {
                        holes.push(Hole { id: s.id.clone(), outline: stair_rect(s).to_vec(), what: "the opening of a stair" });
                    }
                }
            }
        }

        let bodies = project
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Asset(a) if a.level_id == level_id => Some(Body::of_asset(a)),
                _ => None,
            })
            .collect();
        Scene { walls, wall_lines, holes, doors, windows, bodies }
    }

    /// The wall a room edge runs along: the nearest one parallel to it, at
    /// half its thickness.
    fn wall_of_edge(&self, a: Point, b: Point) -> Option<Id> {
        let mid = lerp(a, b, 0.5);
        let dir = unit(sub(b, a))?;
        self.wall_lines
            .iter()
            .filter(|(_, s, e, _)| unit(sub(*e, *s)).map(|d| cross(d, dir).abs() < 0.02).unwrap_or(false))
            .map(|(id, s, e, t)| (id, (dist_point_segment(mid, *s, *e) - t / 2.0).abs()))
            .filter(|(_, off)| *off < 30.0)
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .map(|(id, _)| id.clone())
    }

    /// Whether `spec` may stand at `c` turned `rot`. `skip` are objects the
    /// test leaves out (the object itself when it is re-checked), `with` an
    /// object whose front clearance it may use (a chair at its desk).
    fn check(&self, room: Option<&RoomView>, spec: &Spec, c: Point, rot: f64, skip: &[&Id], with: Option<&Id>) -> Result<(), Why> {
        let solids = spec.solids(c, rot);
        let foot = &solids[0].outline;
        let foot_area = area(foot).max(1.0);
        // Inside the room's floor.
        if let Some(r) = room {
            let inside = overlaps(foot, &r.net);
            let slack = (foot_area * 0.01).max(2_000.0);
            if inside < foot_area - slack {
                return Err(Why::Outside);
            }
            if r.islands.iter().any(|h| overlaps(foot, h) > TOUCH_MM2.min(foot_area * 0.05)) {
                return Err(Why::Wall);
            }
        }
        if spec.mount != Mount::Opening && self.walls.iter().any(|(_, o, _)| overlaps(foot, o) > IN_WALL_MM2.min(foot_area * 0.05)) {
            return Err(Why::Wall);
        }
        let z_overlap = |a: &Solid, z0: f64, z1: f64| a.z0 < z1 - 1.0 && z0 < a.z1 - 1.0;
        // Floor that is not there. A ceiling object only minds a void.
        for h in &self.holes {
            if spec.mount == Mount::Ceiling && h.what != "a void" {
                continue;
            }
            if spec.mount == Mount::Wall {
                continue;
            }
            if overlaps(foot, &h.outline) > TOUCH_MM2.min(foot_area * 0.05) {
                return Err(if h.what == "a void" { Why::Void } else { Why::Stair });
            }
        }
        // Doorways and the sweep of their leaves.
        if spec.mount != Mount::Ceiling {
            for d in &self.doors {
                if solids.iter().any(|s| z_overlap(s, d.z0, d.z1) && overlaps(&s.outline, &d.outline) > TOUCH_MM2.min(area(&s.outline) * 0.05)) {
                    return Err(Why::Door);
                }
            }
            // Windows, by sill height.
            for wz in &self.windows {
                if solids.iter().any(|s| z_overlap(s, wz.z0, wz.z1) && overlaps(&s.outline, &wz.outline) > 1_000.0) {
                    return Err(Why::Window);
                }
            }
        }
        // Other objects at the same height, and the space in front.
        let front = spec.front(c, rot);
        for b in &self.bodies {
            if skip.contains(&&b.id) || b.spec.device || spec.device {
                continue;
            }
            for s in &solids {
                for t in &b.solids {
                    if z_overlap(s, t.z0, t.z1) && overlaps(&s.outline, &t.outline) > TOUCH_MM2.min(area(&s.outline).min(area(&t.outline)) * 0.05) {
                        return Err(Why::Object);
                    }
                }
            }
            if spec.on_floor() && with != Some(&b.id) {
                if let Some(bf) = &b.front {
                    if solids.iter().any(|s| s.z0 < FRONT_TOP_MM && overlaps(&s.outline, bf) > TOUCH_MM2) {
                        return Err(Why::Clearance);
                    }
                }
            }
            if let Some(f) = &front {
                if b.solids.iter().any(|t| t.z0 < FRONT_TOP_MM && b.spec.on_floor() && overlaps(f, &t.outline) > TOUCH_MM2) {
                    return Err(Why::Clearance);
                }
            }
        }
        // Its own front: inside the room and over floor.
        if let Some(f) = &front {
            if let Some(r) = room {
                if overlaps(f, &r.net) < area(f) * 0.8 {
                    return Err(Why::Clearance);
                }
            }
            if self.holes.iter().any(|h| overlaps(f, &h.outline) > TOUCH_MM2) {
                return Err(Why::Clearance);
            }
        }
        Ok(())
    }
}

// ------------------------------------------------------------- spots

/// A place an object may stand.
#[derive(Debug, Clone)]
struct Spot {
    kind: &'static str,
    /// "middle", "start" or "end" of a free stretch of wall.
    fit: &'static str,
    edge: Option<usize>,
    wall_id: Option<Id>,
    center: Point,
    rot: f64,
    /// The free stretch along the wall face, from the edge start, mm.
    free: Option<(f64, f64)>,
}

impl Spot {
    fn json(&self) -> Value {
        let mut v = json!({
            "kind": self.kind,
            "position": round_pt(self.center),
            "rotation_deg": self.rot,
        });
        if let Some(e) = self.edge {
            v["edge"] = json!(e);
            v["fit"] = json!(self.fit);
        }
        if let Some(w) = &self.wall_id {
            v["wall_id"] = json!(w);
        }
        if let Some((a, b)) = self.free {
            v["free_from_mm"] = json!((a * 10.0).round() / 10.0);
            v["free_to_mm"] = json!((b * 10.0).round() / 10.0);
        }
        v
    }
}

/// Spots with the object's back against each wall of the room, one per free
/// stretch and fit. Refusals are counted into `why`.
fn wall_spots(scene: &Scene, room: &RoomView, spec: &Spec, why: &mut BTreeMap<Why, usize>) -> Vec<Spot> {
    let mut out = vec![];
    let n = room.net.len();
    let gap = if spec.mount == Mount::Wall { 0.0 } else { WALL_GAP_MM };
    for i in 0..n {
        let (a, b) = (room.net[i], room.net[(i + 1) % n]);
        let len = dist(a, b);
        if len < spec.w + 1.0 {
            continue;
        }
        let Some(dir) = unit(sub(b, a)) else { continue };
        let inward = perp_left(dir);
        let rot = rotation_facing(inward);
        let center_at = |t: f64| add(add(a, scale(dir, t)), scale(inward, spec.d / 2.0 + gap));
        let (lo, hi) = (spec.w / 2.0, len - spec.w / 2.0);
        let mut ts = vec![];
        let mut t = lo;
        while t < hi {
            ts.push(t);
            t += STEP_MM;
        }
        ts.push(hi);
        let wall_id = scene.wall_of_edge(a, b);
        let mut run: Option<(f64, f64)> = None;
        let flush = |run: &mut Option<(f64, f64)>, out: &mut Vec<Spot>| {
            if let Some((t0, t1)) = run.take() {
                let free = Some((t0 - spec.w / 2.0, t1 + spec.w / 2.0));
                let mut push = |fit: &'static str, t: f64| {
                    out.push(Spot { kind: "wall", fit, edge: Some(i), wall_id: wall_id.clone(), center: center_at(t), rot, free });
                };
                push("middle", (t0 + t1) / 2.0);
                if t1 - t0 >= 2.0 * STEP_MM {
                    push("start", t0);
                    push("end", t1);
                }
            }
        };
        for &t in &ts {
            match scene.check(Some(room), spec, center_at(t), rot, &[], None) {
                Ok(()) => {
                    run = Some(match run {
                        Some((t0, _)) => (t0, t),
                        None => (t, t),
                    })
                }
                Err(w) => {
                    *why.entry(w).or_default() += 1;
                    flush(&mut run, &mut out);
                }
            }
        }
        flush(&mut run, &mut out);
    }
    out
}

/// The longest edge of the room and its direction.
fn longest_edge(room: &RoomView) -> Point {
    let n = room.net.len();
    (0..n)
        .map(|i| (room.net[i], room.net[(i + 1) % n]))
        .max_by(|x, y| dist(x.0, x.1).total_cmp(&dist(y.0, y.1)))
        .and_then(|(a, b)| unit(sub(b, a)))
        .unwrap_or(pt(1.0, 0.0))
}

/// Freestanding spots near the middle of the room, long side along its
/// longest wall or across it, with room to walk around.
fn center_spots(scene: &Scene, room: &RoomView, spec: &Spec, why: &mut BTreeMap<Why, usize>) -> Vec<Spot> {
    let axis = longest_edge(room);
    let base = rotation_facing(perp_left(axis));
    // Long side along the longest wall first.
    let rots = if spec.d > spec.w { [base + 90.0, base] } else { [base, base + 90.0] };
    let mut out = vec![];
    let ring = Spec { w: spec.w + 2.0 * AROUND_MM, d: spec.d + 2.0 * AROUND_MM, clearance: 0.0, built: None, h: spec.h.max(1.0), ..spec.clone() };
    let across = perp_left(axis);
    let mut offsets = vec![(0.0, 0.0)];
    for s in [300.0, 600.0, 900.0, 1_200.0] {
        for (u, v) in [(s, 0.0), (-s, 0.0), (0.0, s), (0.0, -s)] {
            offsets.push((u, v));
        }
    }
    for rot in rots {
        let rot = (rot % 360.0 + 360.0) % 360.0;
        for (u, v) in &offsets {
            let c = add(room.label, add(scale(axis, *u), scale(across, *v)));
            let ok = scene.check(Some(room), spec, c, rot, &[], None).and_then(|_| {
                // Room to walk around: the ring stays in the room and clear of objects.
                let ring_foot = oriented_rect(c, ring.w, ring.d, rot);
                if overlaps(&ring_foot, &room.net) < area(&ring_foot) * 0.85 {
                    return Err(Why::Outside);
                }
                for b in &scene.bodies {
                    if !b.spec.device && b.solids.iter().any(|s| s.z0 < FRONT_TOP_MM && overlaps(&ring_foot, &s.outline) > TOUCH_MM2) {
                        return Err(Why::Object);
                    }
                }
                Ok(())
            });
            match ok {
                Ok(()) => {
                    out.push(Spot { kind: "center", fit: "middle", edge: None, wall_id: None, center: c, rot, free: None });
                    break;
                }
                Err(w) => *why.entry(w).or_default() += 1,
            }
        }
    }
    out
}

/// A ceiling spot in the middle of the room.
fn ceiling_spots(scene: &Scene, room: &RoomView, spec: &Spec, why: &mut BTreeMap<Why, usize>) -> Vec<Spot> {
    let rot = rotation_facing(perp_left(longest_edge(room)));
    match scene.check(Some(room), spec, room.label, rot, &[], None) {
        Ok(()) => vec![Spot { kind: "ceiling", fit: "middle", edge: None, wall_id: None, center: room.label, rot, free: None }],
        Err(w) => {
            *why.entry(w).or_default() += 1;
            vec![]
        }
    }
}

/// Tables, sets and cars stand free in the room; the rest go against a wall.
fn freestanding(spec: &Spec) -> bool {
    let k = spec.key.as_str();
    k.starts_with("dining") || k.starts_with("coffee-table") || k == "tea-table" || k == "table-wood" || k == "ottoman" || k == "outdoor-set" || k.starts_with("car-")
}

fn spots_for(scene: &Scene, room: &RoomView, spec: &Spec, why: &mut BTreeMap<Why, usize>) -> Vec<Spot> {
    match spec.mount {
        Mount::Ceiling => ceiling_spots(scene, room, spec, why),
        Mount::Opening => vec![],
        _ => {
            let mut out = vec![];
            if freestanding(spec) {
                out.extend(center_spots(scene, room, spec, why));
            }
            out.extend(wall_spots(scene, room, spec, why));
            out
        }
    }
}

/// `Query::PlacementOptions`: free spots for `catalog_key` in a room,
/// against each wall (and in the middle for tables), clear of doors,
/// swings, windows (by sill height), voids, stair openings and every
/// object's front clearance.
pub fn options(project: &Project, analysis: &Analysis, room_id: &Id, catalog_key: &str) -> Result<Value, CoreError> {
    let room = room_view(project, analysis, room_id)?;
    let item = catalog_item(catalog_key).ok_or_else(|| {
        CoreError::invalid("unknown_catalog_key", format!("No catalog item has the key `{catalog_key}`. Query the catalog for the keys."))
    })?;
    let scene = Scene::new(project, analysis, &room.level_id);
    let spec = Spec::of_item(item);
    let mut why = BTreeMap::new();
    let spots = spots_for(&scene, &room, &spec, &mut why);
    let listed: Vec<Value> = spots.iter().take(MAX_OPTIONS).map(Spot::json).collect();
    let mut out = json!({
        "room_id": room.id,
        "room": room.name,
        "level_id": room.level_id,
        "catalog_key": catalog_key,
        "size": {"width_mm": spec.w, "depth_mm": spec.d, "height_mm": spec.h, "elevation_mm": spec.elevation},
        "mount": spec.mount,
        "clearance_mm": spec.clearance,
        "options": listed,
        "note": "Spots keep the object's back to the wall (rotation_deg turns local +y, its back, toward the wall), clear of doorways and door swings, windows lower than the object, walls, voids, stair openings, other objects and the space each object needs in front. free_from_mm and free_to_mm give the free stretch of that wall face from the edge start, so the object can slide within it.",
    });
    if spots.is_empty() {
        let main = why.iter().max_by_key(|(_, n)| **n).map(|(w, _)| w.text()).unwrap_or("there is no floor for it");
        out["reason"] = json!(format!("No free spot for {} in {}: {main}.", spec.name, room.name));
    }
    if spots.len() > MAX_OPTIONS {
        out["more"] = json!(spots.len() - MAX_OPTIONS);
    }
    Ok(out)
}

// ------------------------------------------------------------- catalog

/// `Query::Catalog`: items whose key, name, category or tags hold every
/// word of `search`, for a room usage. A usage lists the items that name it;
/// items for any room are added only when a search narrows the list.
pub fn catalog(search: &Option<String>, usage: &Option<RoomUsage>) -> Value {
    let words: Vec<String> = search
        .as_deref()
        .unwrap_or("")
        .to_lowercase()
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    let category = |c: AssetCategory| serde_json::to_value(c).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    let items: Vec<Value> = catalog_items()
        .iter()
        .filter(|it| {
            let text = format!("{} {} {} {}", it.key, it.name, category(it.category), it.tags.join(" ")).to_lowercase();
            words.iter().all(|w| text.contains(w.as_str()))
        })
        .filter(|it| match usage {
            None => true,
            Some(u) => it.rooms.contains(u) || (it.rooms.is_empty() && !words.is_empty()),
        })
        .map(|it| {
            let mut v = serde_json::to_value(it).unwrap_or(Value::Null);
            if it.parametric.is_some() {
                v["built_in_defaults"] = json!(built_in_defaults(it.parametric.expect("checked"), it.width_mm));
            }
            v
        })
        .collect();
    json!({
        "count": items.len(),
        "search": search,
        "usage": usage,
        "items": items,
        "note": "Sizes in mm. clearance_mm is the free space an item needs in front (its -y side). rooms lists the room usages it belongs in; empty means any room. model names the photoscanned pack model the 3D view shows; built-ins (parametric) are drawn in code at any size.",
    })
}

// ------------------------------------------------------------- review

/// `asset_in_wall`, `assets_overlap`, and objects partly over a void or a
/// stair opening, or standing on a stair (G4 problem 2). An object whose
/// center is over a void or a stair opening is `slabs::review`'s
/// `asset_over_opening` already; this adds the ones only partly over.
pub fn review(project: &Project, analysis: &Analysis) -> Vec<Issue> {
    let mut out = vec![];
    for level in &project.levels {
        if !project.elements.iter().any(|e| matches!(e, Element::Asset(a) if a.level_id == level.id)) {
            continue;
        }
        let scene = Scene::new(project, analysis, &level.id);
        let bodies: Vec<&Body> = scene.bodies.iter().filter(|b| !b.spec.device && b.spec.mount != Mount::Opening).collect();
        for b in &bodies {
            let foot = &b.solids[0].outline;
            let foot_area = area(foot).max(1.0);
            // Buried objects (a septic tank) are under the floor, not in a wall.
            if b.solids[0].z1 > 0.0 {
                if let Some((wall_id, _, _)) = scene
                    .walls
                    .iter()
                    .find(|(_, o, h)| b.solids[0].z0 < *h && overlaps(foot, o) > IN_WALL_MM2.min(foot_area * 0.05).max(500.0))
                {
                    out.push(issue(
                        "asset_in_wall",
                        Severity::Warning,
                        format!("{} reaches into a wall. Move it into the room, clear of the wall.", b.name),
                        vec![b.id.clone(), wall_id.clone()],
                    ));
                }
            }
            if b.spec.on_floor() {
                for h in &scene.holes {
                    let covered = overlaps(foot, &h.outline);
                    if covered <= TOUCH_MM2.min(foot_area * 0.05) {
                        continue;
                    }
                    let centered = point_in_polygon(b.center, &h.outline);
                    // The center over a void or an arriving stair's opening
                    // is slabs.rs's finding.
                    if centered && h.what != "a stair" {
                        continue;
                    }
                    let message = if h.what == "a stair" {
                        format!("{} stands on a stair. Move it off the flight.", b.name)
                    } else {
                        format!("{} stands partly over {}, where there is no floor under it. Move it onto the floor.", b.name, h.what)
                    };
                    out.push(issue("asset_over_opening", Severity::Warning, message, vec![b.id.clone(), h.id.clone()]));
                    break;
                }
            }
        }
        for (i, a) in bodies.iter().enumerate() {
            for b in bodies.iter().skip(i + 1) {
                let hit = a.solids.iter().any(|s| {
                    b.solids.iter().any(|t| {
                        s.z0 < t.z1 - 1.0
                            && t.z0 < s.z1 - 1.0
                            && overlaps(&s.outline, &t.outline) > TOUCH_MM2.min(area(&s.outline).min(area(&t.outline)) * 0.05).max(500.0)
                    })
                });
                if hit {
                    out.push(issue(
                        "assets_overlap",
                        Severity::Warning,
                        format!("{} and {} stand in the same place. Move one of them.", a.name, b.name),
                        vec![a.id.clone(), b.id.clone()],
                    ));
                }
            }
        }
    }
    out
}

// ------------------------------------------------------------- furnishing

/// What "Furnish rooms" places in one room: new objects only, never an
/// edit to what is there.
#[derive(Debug, Clone, PartialEq)]
pub struct FurnishPlan {
    pub room_id: Id,
    pub room_name: String,
    pub usage: RoomUsage,
    pub assets: Vec<Asset>,
    /// Catalog keys that found no spot, with why.
    pub skipped: Vec<(String, String)>,
}

impl FurnishPlan {
    /// One `AddElement` per new object, in the order they were placed.
    pub fn commands(&self) -> Vec<Command> {
        self.assets.iter().map(|a| Command::AddElement { element: Element::Asset(a.clone()) }).collect()
    }

    /// "Bedroom 1: double bed, 2 nightstands, built-in wardrobe 1800 mm".
    pub fn summary(&self) -> String {
        let mut counts: Vec<(String, usize)> = vec![];
        for a in &self.assets {
            let label = match built_in_of(a) {
                Some(_) => format!("{} {:.0} mm", a.name.to_lowercase(), a.width_mm),
                None => a.name.to_lowercase(),
            };
            match counts.iter_mut().find(|(l, _)| *l == label) {
                Some((_, n)) => *n += 1,
                None => counts.push((label, 1)),
            }
        }
        let list: Vec<String> = counts.into_iter().map(|(l, n)| if n > 1 { format!("{n} x {l}") } else { l }).collect();
        if list.is_empty() {
            format!("{}: nothing fits", self.room_name)
        } else {
            format!("{}: {}", self.room_name, list.join(", "))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    Bed,
    Sofa,
    Run,
    Desk,
    Vanity,
    Wc,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Pref {
    Longest,
    AwayFromDoor,
    UnderWindow,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Place {
    Wall(Pref),
    /// In front of the anchor, past its front clearance, same way round.
    Front(Role),
    /// Beside the anchor along its wall: both sides, or the first that fits.
    Beside(Role, bool),
    /// Against the wall that faces the anchor.
    Facing(Role),
    /// On the wall above the anchor (wall objects).
    Above(Role),
    /// Freestanding in the middle of the room.
    Center,
    /// Snug in a free corner.
    Corner,
    Ceiling,
    /// A built-in along the longest free stretch of wall, sized to it.
    Run(f64, f64),
}

#[derive(Debug, Clone)]
struct Step {
    role: Role,
    keys: Vec<&'static str>,
    place: Place,
    /// A chair at its desk may use the desk's front clearance.
    companion: bool,
}

fn step(role: Role, keys: &[&'static str], place: Place) -> Step {
    Step { role, keys: keys.to_vec(), place, companion: false }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Style {
    Modern,
    Tropical,
    Classic,
    Industrial,
}

fn style_of(s: &str) -> Style {
    let s = s.to_lowercase();
    if ["tropical", "bahay", "kubo", "filipino", "resort", "rattan"].iter().any(|w| s.contains(w)) {
        Style::Tropical
    } else if ["classic", "traditional", "heritage", "colonial", "vintage"].iter().any(|w| s.contains(w)) {
        Style::Classic
    } else if s.contains("industrial") || s.contains("loft") {
        Style::Industrial
    } else {
        Style::Modern
    }
}

/// The default furnishing of a room by its use, style and size.
fn recipe(usage: RoomUsage, style: Style, area_m2: f64) -> Vec<Step> {
    use Place as P;
    use Role as R;
    use RoomUsage as U;
    let s = style;
    let pick = |modern: &[&'static str], tropical: &[&'static str], classic: &[&'static str], industrial: &[&'static str]| -> Vec<&'static str> {
        match s {
            Style::Modern => modern.to_vec(),
            Style::Tropical => tropical.to_vec(),
            Style::Classic => classic.to_vec(),
            Style::Industrial => industrial.to_vec(),
        }
    };
    let with = |role: R, keys: Vec<&'static str>, place: P| Step { role, keys, place, companion: false };
    let mut steps = vec![];
    match usage {
        U::Living => {
            steps.push(with(R::Sofa, pick(&["sofa-leather", "sofa-3", "sofa-2"], &["sofa-wood", "sofa-leather", "sofa-2"], &["sofa-wood", "sofa-leather", "sofa-2"], &["sofa-leather", "sofa-3", "sofa-2"]), P::Wall(Pref::AwayFromDoor)));
            steps.push(with(R::Other, pick(&["coffee-table-round", "coffee-table", "coffee-table-square"], &["tea-table", "coffee-table"], &["tea-table", "coffee-table-round"], &["coffee-table-square", "coffee-table"]), P::Front(R::Sofa)));
            steps.push(step(R::Other, &["sideboard", "tv-console"], P::Facing(R::Sofa)));
            steps.push(step(R::Other, &["side-table", "side-table-round"], P::Beside(R::Sofa, false)));
            if area_m2 >= 14.0 {
                steps.push(with(R::Other, pick(&["lounge-chair", "armchair"], &["rocking-chair", "armchair"], &["armchair-classic", "rocking-chair"], &["lounge-chair", "armchair"]), P::Wall(Pref::Any)));
            }
            steps.push(step(R::Other, &["wall-art"], P::Above(R::Sofa)));
            steps.push(step(R::Other, &["plant-leafy", "plant-pot"], P::Corner));
            if area_m2 >= 16.0 {
                steps.push(step(R::Other, &["shelf-cube"], P::Wall(Pref::Any)));
                steps.push(step(R::Other, &["plant-pot", "vase-ceramic"], P::Corner));
            }
            steps.push(step(R::Other, &["ceiling-fan"], P::Ceiling));
        }
        U::Dining => {
            let sets: &[&'static str] = if area_m2 >= 12.0 { &["dining-set-6", "dining-set-4", "dining-round-2"] } else { &["dining-set-4", "dining-round-2"] };
            steps.push(step(R::Other, sets, P::Center));
            steps.push(with(R::Other, pick(&["sideboard", "cabinet-white"], &["cabinet-white", "sideboard"], &["cabinet-white", "sideboard"], &["sideboard"]), P::Wall(Pref::Longest)));
            steps.push(step(R::Other, &["wall-art"], P::Wall(Pref::Any)));
            steps.push(step(R::Other, &["plant-leafy", "plant-pot"], P::Corner));
            steps.push(step(R::Other, &["ceiling-fan"], P::Ceiling));
        }
        U::Kitchen => {
            steps.push(step(R::Run, &["kitchen-run"], P::Run(1_200.0, 3_600.0)));
            steps.push(step(R::Other, &["refrigerator"], P::Beside(R::Run, false)));
            steps.push(step(R::Other, &["refrigerator"], P::Wall(Pref::Any)));
            if area_m2 >= 9.0 {
                steps.push(step(R::Other, &["dining-round-2", "table-wood"], P::Center));
            }
        }
        U::Bedroom | U::MasterBedroom => {
            let beds: &[&'static str] = if usage == U::MasterBedroom {
                &["bed-queen", "bed-double", "bed-single"]
            } else if area_m2 < 9.0 {
                &["bed-single"]
            } else {
                &["bed-double", "bed-single"]
            };
            steps.push(step(R::Bed, beds, P::Wall(Pref::AwayFromDoor)));
            steps.push(step(R::Other, &["nightstand"], P::Beside(R::Bed, true)));
            steps.push(step(R::Other, &["wardrobe-builtin"], P::Run(1_200.0, 2_400.0)));
            steps.push(step(R::Other, &["wardrobe"], P::Wall(Pref::Any)));
            if area_m2 >= 12.0 {
                steps.push(with(R::Other, pick(&["cabinet-drawers", "dresser"], &["dresser"], &["dresser"], &["cabinet-drawers"]), P::Wall(Pref::Any)));
            }
            if area_m2 >= 11.0 {
                steps.push(step(R::Desk, &["desk-metal", "desk"], P::Wall(Pref::UnderWindow)));
                steps.push(Step { role: R::Other, keys: vec!["chair-wood-white"], place: P::Front(R::Desk), companion: true });
            }
            if usage == U::MasterBedroom && area_m2 >= 16.0 {
                steps.push(with(R::Other, pick(&["lounge-chair"], &["rocking-chair"], &["armchair-classic"], &["lounge-chair"]), P::Wall(Pref::Any)));
            }
            steps.push(step(R::Other, &["wall-art"], P::Above(R::Bed)));
            steps.push(step(R::Other, &["plant-leafy"], P::Corner));
            steps.push(step(R::Other, &["ceiling-fan"], P::Ceiling));
        }
        U::Bathroom | U::PowderRoom => {
            // The shower takes its corner first: a water closet in the middle
            // of the back wall would leave none (the door holds the others).
            if usage == U::Bathroom {
                steps.push(step(R::Other, &["shower"], P::Corner));
            }
            steps.push(step(R::Wc, &["wc"], P::Wall(Pref::AwayFromDoor)));
            let (lo, hi) = if usage == U::PowderRoom { (600.0, 900.0) } else { (600.0, 1_000.0) };
            steps.push(step(R::Vanity, &["vanity"], P::Run(lo, hi)));
            steps.push(step(R::Vanity, &["lavatory"], P::Wall(Pref::Any)));
            if usage == U::Bathroom {
                steps.push(step(R::Other, &["basket-rattan"], P::Corner));
            }
        }
        U::Laundry => {
            steps.push(step(R::Other, &["washing-machine"], P::Wall(Pref::Any)));
            steps.push(step(R::Run, &["counter"], P::Run(900.0, 1_800.0)));
            steps.push(step(R::Other, &["shelf-steel"], P::Wall(Pref::Any)));
            steps.push(step(R::Other, &["basket-rattan"], P::Corner));
        }
        U::Office => {
            steps.push(step(R::Desk, &["desk-metal", "desk"], P::Wall(Pref::UnderWindow)));
            steps.push(Step { role: R::Other, keys: vec!["chair-dining"], place: P::Front(R::Desk), companion: true });
            steps.push(step(R::Other, &["shelf-cube"], P::Wall(Pref::Any)));
            if area_m2 >= 10.0 {
                steps.push(step(R::Other, &["lounge-chair", "armchair"], P::Wall(Pref::Any)));
            }
            steps.push(step(R::Other, &["plant-leafy", "plant-pot"], P::Corner));
            steps.push(step(R::Other, &["ceiling-fan"], P::Ceiling));
        }
        U::Porch => {
            steps.push(with(R::Sofa, pick(&["bench-wood"], &["sofa-wood", "bench-wood"], &["bench-wood"], &["bench-wood"]), P::Wall(Pref::Longest)));
            steps.push(with(R::Other, pick(&["outdoor-set"], &["tea-table"], &["outdoor-set"], &["outdoor-set"]), P::Center));
            steps.push(with(R::Other, pick(&["chair-monobloc"], &["rocking-chair", "chair-monobloc"], &["rocking-chair"], &["chair-monobloc"]), P::Wall(Pref::Any)));
            steps.push(step(R::Other, &["planter-box"], P::Wall(Pref::Any)));
            steps.push(step(R::Other, &["plant-leafy", "plant-pot"], P::Corner));
        }
        U::Garage => {
            steps.push(step(R::Other, &["car-covered", "car-sedan"], P::Center));
            steps.push(step(R::Other, &["shelf-steel"], P::Wall(Pref::Any)));
        }
        U::Hallway => {
            steps.push(step(R::Other, &["bench-wood"], P::Wall(Pref::Longest)));
            steps.push(step(R::Other, &["wall-art"], P::Wall(Pref::Any)));
            steps.push(step(R::Other, &["plant-pot", "vase-ceramic"], P::Corner));
        }
        U::Storage => {
            steps.push(step(R::Other, &["shelf-steel"], P::Wall(Pref::Longest)));
            steps.push(step(R::Other, &["shelf-steel"], P::Wall(Pref::Any)));
        }
        U::Other => {
            steps.push(step(R::Other, &["plant-pot"], P::Corner));
        }
    }
    steps
}

/// How a key the model picked is placed, from what it is.
fn step_for_key(key: &'static str) -> Option<Step> {
    let it = catalog_item(key)?;
    let has = |w: &str| it.tags.iter().any(|t| t == w);
    let k = it.key.as_str();
    let s = |role: Role, place: Place| Some(Step { role, keys: vec![key], place, companion: false });
    if it.mount == Mount::Ceiling {
        return s(Role::Other, Place::Ceiling);
    }
    match it.parametric {
        Some(BuiltInKind::KitchenRun) => return s(Role::Run, Place::Run(1_200.0, 3_600.0)),
        Some(BuiltInKind::Counter) => return s(Role::Run, Place::Run(900.0, 2_400.0)),
        Some(BuiltInKind::Vanity) => return s(Role::Vanity, Place::Run(600.0, 1_200.0)),
        Some(BuiltInKind::Wardrobe) => return s(Role::Other, Place::Run(1_200.0, 2_400.0)),
        Some(BuiltInKind::Shelf) => return s(Role::Other, Place::Wall(Pref::Any)),
        None => {}
    }
    if k.starts_with("bed-") {
        return s(Role::Bed, Place::Wall(Pref::AwayFromDoor));
    }
    if has("sofa") || k == "bench-wood" {
        return s(Role::Sofa, Place::Wall(Pref::AwayFromDoor));
    }
    if k == "wc" {
        return s(Role::Wc, Place::Wall(Pref::AwayFromDoor));
    }
    if k == "lavatory" {
        return s(Role::Vanity, Place::Wall(Pref::Any));
    }
    if has("desk") && !k.starts_with("table") {
        return s(Role::Desk, Place::Wall(Pref::UnderWindow));
    }
    if has("coffee table") {
        return s(Role::Other, Place::Front(Role::Sofa));
    }
    if has("nightstand") {
        return s(Role::Other, Place::Beside(Role::Bed, true));
    }
    if k == "sideboard" || k == "tv-console" {
        return s(Role::Other, Place::Facing(Role::Sofa));
    }
    if k == "side-table" || k == "side-table-round" {
        return s(Role::Other, Place::Beside(Role::Sofa, false));
    }
    if k == "wall-art" {
        return s(Role::Other, Place::Above(Role::Bed));
    }
    if k == "mirror-wall" {
        return s(Role::Other, Place::Above(Role::Vanity));
    }
    if k == "refrigerator" {
        return s(Role::Other, Place::Beside(Role::Run, false));
    }
    if freestanding(&Spec::of_item(it)) {
        return s(Role::Other, Place::Center);
    }
    if it.category == AssetCategory::Plant || k == "vase-ceramic" || k == "basket-rattan" || k == "shower" {
        return s(Role::Other, Place::Corner);
    }
    s(Role::Other, Place::Wall(Pref::Any))
}

/// What one placement makes: the object's spec (a built-in sized to its
/// spot) and where it stands.
struct Placed {
    spec: Spec,
    center: Point,
    rot: f64,
}

/// Distance from a point to the nearest doorway of the scene.
fn door_distance(scene: &Scene, p: Point) -> f64 {
    scene
        .doors
        .iter()
        .flat_map(|z| z.outline.iter())
        .map(|q| dist(*q, p))
        .fold(1.0e6, f64::min)
}

fn best_wall_spot(scene: &Scene, spots: &[Spot], pref: Pref, spec: &Spec) -> Option<Spot> {
    let middles: Vec<&Spot> = spots.iter().filter(|s| s.kind == "wall" && s.fit == "middle").collect();
    let free_len = |s: &Spot| s.free.map(|(a, b)| b - a).unwrap_or(0.0);
    let pick = match pref {
        Pref::Longest => middles.iter().max_by(|a, b| free_len(a).total_cmp(&free_len(b))).copied(),
        Pref::AwayFromDoor => middles
            .iter()
            .max_by(|a, b| (door_distance(scene, a.center) + free_len(a) * 0.25).total_cmp(&(door_distance(scene, b.center) + free_len(b) * 0.25)))
            .copied(),
        Pref::UnderWindow => {
            let under: Vec<&Spot> = middles
                .iter()
                .copied()
                .filter(|s| {
                    let foot = oriented_rect(s.center, spec.w, spec.d, s.rot);
                    scene.windows.iter().any(|w| overlaps(&foot, &w.outline) > 1_000.0)
                })
                .collect();
            under.into_iter().max_by(|a, b| free_len(a).total_cmp(&free_len(b))).or_else(|| middles.iter().max_by(|a, b| free_len(a).total_cmp(&free_len(b))).copied())
        }
        Pref::Any => middles.iter().max_by(|a, b| free_len(a).total_cmp(&free_len(b))).copied(),
    };
    pick.cloned()
}

/// A built-in sized to the longest free stretch of wall it fits, between
/// `lo` and `hi` wide, its sink under a window when the stretch has one.
fn place_run(scene: &Scene, room: &RoomView, base: &Spec, lo: f64, hi: f64) -> Option<Placed> {
    let mut widths = vec![];
    let mut w = hi;
    while w > lo {
        widths.push(w);
        w -= 300.0;
    }
    widths.push(lo);
    let uppers: &[bool] = match base.built {
        Some((BuiltInKind::KitchenRun | BuiltInKind::Vanity, _)) => &[true, false],
        _ => &[false],
    };
    for &upper in uppers {
        for &w in &widths {
            let mut spec = base.with_width(w);
            if let Some((_, p)) = spec.built.as_mut() {
                p.upper = upper;
            }
            let mut why = BTreeMap::new();
            let spots = wall_spots(scene, room, &spec, &mut why);
            let Some(best) = best_wall_spot(scene, &spots, Pref::Longest, &spec) else { continue };
            // Grow it to the free stretch, then center it there.
            let (f0, f1) = best.free.unwrap_or((0.0, w));
            let grown = ((f1 - f0).min(hi) / 50.0).floor() * 50.0;
            let edge = best.edge.unwrap_or(0);
            let n = room.net.len();
            let (a, b) = (room.net[edge], room.net[(edge + 1) % n]);
            let dir = unit(sub(b, a)).unwrap_or(pt(1.0, 0.0));
            let inward = perp_left(dir);
            for width in [grown, w] {
                if width < w - 1e-6 {
                    continue;
                }
                let mut sized = base.with_width(width);
                if let Some((_, p)) = sized.built.as_mut() {
                    p.upper = upper;
                }
                let t = (f0 + f1) / 2.0;
                let center = add(add(a, scale(dir, t)), scale(inward, sized.d / 2.0 + WALL_GAP_MM));
                if scene.check(Some(room), &sized, center, best.rot, &[], None).is_err() {
                    continue;
                }
                place_sink(scene, &mut sized, center, best.rot);
                return Some(Placed { spec: sized, center, rot: best.rot });
            }
        }
    }
    None
}

/// Puts a kitchen run's sink under a window on its wall, and its cooktop at
/// least 900 mm from it.
fn place_sink(scene: &Scene, spec: &mut Spec, center: Point, rot: f64) {
    let Some((kind, p)) = spec.built.as_mut() else { return };
    if *kind != BuiltInKind::KitchenRun {
        return;
    }
    let (along, back) = frame(rot);
    let left = sub(center, scale(along, spec.w / 2.0));
    let band = oriented_rect(add(center, scale(back, spec.d / 2.0)), spec.w, 2.0 * WINDOW_BAND_MM, rot);
    let window = scene.windows.iter().filter(|z| overlaps(&band, &z.outline) > 1_000.0).map(|z| {
        let n = z.outline.len() as f64;
        let c = z.outline.iter().fold(pt(0.0, 0.0), |acc, q| add(acc, *q));
        pt(c.x / n, c.y / n)
    }).next();
    let w = spec.w;
    if let Some(wc) = window {
        let s = round50(clamp_to(dot(sub(wc, left), along), 450.0, w - 450.0));
        p.sink_at_mm = Some(s);
        // The cooktop on the longer side, at least 900 mm on.
        let right = (s + 900.0).max(w * 0.75).min(w - 350.0);
        let left_at = (s - 900.0).min(w * 0.25).max(350.0);
        p.cooktop_at_mm = if right - s >= 900.0 - 1e-6 && right >= 350.0 {
            Some(round50(right))
        } else if s - left_at >= 900.0 - 1e-6 {
            Some(round50(left_at))
        } else {
            None
        };
    }
}

/// Where one step's object goes, given the anchors placed so far.
fn place_step(scene: &Scene, room: &RoomView, spec: &Spec, st: &Step, anchors: &BTreeMap<Role, Body>, why: &mut BTreeMap<Why, usize>) -> Vec<Placed> {
    let ok = |c: Point, rot: f64, with: Option<&Id>, why: &mut BTreeMap<Why, usize>| match scene.check(Some(room), spec, c, rot, &[], with) {
        Ok(()) => true,
        Err(w) => {
            *why.entry(w).or_default() += 1;
            false
        }
    };
    let one = |c: Point, rot: f64| vec![Placed { spec: spec.clone(), center: c, rot }];
    match st.place {
        Place::Wall(pref) => {
            let spots = wall_spots(scene, room, spec, why);
            best_wall_spot(scene, &spots, pref, spec).map(|s| one(s.center, s.rot)).unwrap_or_default()
        }
        Place::Corner => {
            let spots = wall_spots(scene, room, spec, why);
            let corner_d = |s: &Spot| room.net.iter().map(|q| dist(*q, s.center)).fold(f64::MAX, f64::min);
            spots
                .iter()
                .filter(|s| s.fit != "middle" || s.free.map(|(a, b)| b - a < spec.w + 2.0 * STEP_MM).unwrap_or(false))
                .filter(|s| corner_d(s) < spec.w.max(spec.d) * 0.75 + 150.0)
                .max_by(|a, b| door_distance(scene, a.center).total_cmp(&door_distance(scene, b.center)))
                .map(|s| one(s.center, s.rot))
                .unwrap_or_default()
        }
        Place::Center => center_spots(scene, room, spec, why).first().map(|s| one(s.center, s.rot)).unwrap_or_default(),
        Place::Ceiling => ceiling_spots(scene, room, spec, why).first().map(|s| one(s.center, s.rot)).unwrap_or_default(),
        Place::Run(lo, hi) => place_run(scene, room, spec, lo, hi).map(|p| vec![p]).unwrap_or_default(),
        Place::Front(role) => {
            let Some(a) = anchors.get(&role) else { return vec![] };
            let front = scale(a.back_dir(), -1.0);
            let with = if st.companion { Some(&a.id) } else { None };
            let reach = if st.companion { 0.0 } else { a.spec.clearance };
            // A chair faces its desk: turned round.
            let rot = if st.companion { (a.rot + 180.0) % 360.0 } else { a.rot };
            for extra in [0.0, 100.0, 200.0, 300.0] {
                let c = add(a.center, scale(front, a.spec.d / 2.0 + reach + extra + spec.d / 2.0));
                if ok(c, rot, with, why) {
                    return one(c, rot);
                }
            }
            vec![]
        }
        Place::Beside(role, both) => {
            let Some(a) = anchors.get(&role) else { return vec![] };
            let back_plane = add(a.center, scale(a.back_dir(), a.spec.d / 2.0));
            let mut out = vec![];
            for side in [-1.0, 1.0] {
                let c = add(sub(back_plane, scale(a.back_dir(), spec.d / 2.0)), scale(a.along(), side * (a.spec.w / 2.0 + spec.w / 2.0 + 30.0)));
                // Each placed one counts for the next.
                let mut s2 = scene.clone();
                for p in &out {
                    let p: &Placed = p;
                    s2.bodies.push(Body::new("pending".into(), p.spec.clone(), p.center, p.rot));
                }
                if s2.check(Some(room), spec, c, a.rot, &[], None).is_ok() {
                    out.push(Placed { spec: spec.clone(), center: c, rot: a.rot });
                    if !both {
                        break;
                    }
                } else {
                    *why.entry(Why::Object).or_default() += 1;
                }
            }
            out
        }
        Place::Facing(role) => {
            let Some(a) = anchors.get(&role) else {
                let spots = wall_spots(scene, room, spec, why);
                return best_wall_spot(scene, &spots, Pref::Any, spec).map(|s| one(s.center, s.rot)).unwrap_or_default();
            };
            let want = (a.rot + 180.0) % 360.0;
            let spots = wall_spots(scene, room, spec, why);
            let axis_off = |s: &Spot| dot(sub(s.center, a.center), a.along()).abs();
            let mut facing: Vec<&Spot> = spots
                .iter()
                .filter(|s| {
                    let d = ((s.rot - want) % 360.0 + 360.0) % 360.0;
                    d.min(360.0 - d) < 5.0
                })
                .collect();
            facing.sort_by(|x, y| axis_off(x).total_cmp(&axis_off(y)));
            // Slide the best stretch's spot onto the anchor's axis when it fits.
            if let Some(s) = facing.first() {
                let target = sub(s.center, scale(a.along(), dot(sub(s.center, a.center), a.along())));
                if ok(target, s.rot, None, why) {
                    return one(target, s.rot);
                }
                return one(s.center, s.rot);
            }
            vec![]
        }
        Place::Above(role) => {
            let Some(a) = anchors.get(&role) else {
                if spec.mount == Mount::Wall {
                    let spots = wall_spots(scene, room, spec, why);
                    return best_wall_spot(scene, &spots, Pref::Any, spec).map(|s| one(s.center, s.rot)).unwrap_or_default();
                }
                return vec![];
            };
            let mut s2 = spec.clone();
            s2.elevation = s2.elevation.max(a.top() + 250.0);
            let c = sub(a.wall_point(), scale(a.back_dir(), s2.d / 2.0));
            match scene.check(Some(room), &s2, c, a.rot, &[], None) {
                Ok(()) => vec![Placed { spec: s2, center: c, rot: a.rot }],
                Err(w) => {
                    *why.entry(w).or_default() += 1;
                    vec![]
                }
            }
        }
    }
}

/// A stable id for a furnishing object, so a proposal made twice on the same
/// plan makes the same ids, shaped like a UUID v4 string.
fn furnish_id(project: &Project, room_id: &str, key: &str, index: usize) -> Id {
    // Salted again on the rare id the project already has.
    for salt in 0u32.. {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for part in [project.id.as_str(), room_id, key, &index.to_string(), &salt.to_string()] {
            for b in part.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            h ^= 0xff;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        let mut g = h ^ 0x9e37_79b9_7f4a_7c15;
        g = (g ^ (g >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        g ^= g >> 27;
        let a = format!("{h:016x}{g:016x}");
        let id = format!("{}-{}-4{}-8{}-{}", &a[0..8], &a[8..12], &a[13..16], &a[17..20], &a[20..32]);
        if !project.elements.iter().any(|e| e.id() == &id) {
            return id;
        }
    }
    unreachable!("an unused id exists")
}

/// The furnishing of one room: what to place and where. `picks` are the
/// catalog keys to place in order (the copilot's or an agent's choice);
/// None or empty uses the default set for the room's use, `style` and size.
/// Every object passes the same rules as `Query::PlacementOptions`, so
/// nothing lands in a wall, a doorway, a door swing, a void, a stair or
/// another object's space. Objects that find no spot are listed in
/// `skipped`. The room is only read; apply `FurnishPlan::commands`.
pub fn furnish_room(project: &Project, room_id: &Id, picks: Option<&[String]>, style: &str) -> Result<FurnishPlan, CoreError> {
    let analysis = analyze(project);
    let room = room_view(project, &analysis, room_id)?;
    let mut scene = Scene::new(project, &analysis, &room.level_id);
    let steps: Vec<Step> = match picks.filter(|p| !p.is_empty()) {
        Some(keys) => {
            let mut steps: Vec<Step> = keys
                .iter()
                .filter_map(|k| catalog_items().iter().find(|c| &c.key == k))
                .filter_map(|it| step_for_key(it.key.as_str()))
                .collect();
            // Anchors first, so tables find their sofa and nightstands their bed.
            steps.sort_by_key(|s| s.role == Role::Other);
            steps
        }
        None => recipe(room.usage, style_of(style), room.area_mm2 / 1.0e6),
    };
    let mut plan = FurnishPlan { room_id: room.id.clone(), room_name: room.name.clone(), usage: room.usage, assets: vec![], skipped: vec![] };
    if let Some(keys) = picks {
        for k in keys {
            if catalog_item(k).is_none() {
                plan.skipped.push((k.clone(), "not in the catalog".into()));
            }
        }
    }
    let mut anchors: BTreeMap<Role, Body> = BTreeMap::new();
    let mut done_roles: Vec<Role> = vec![];
    for st in &steps {
        // A fallback step for a role already filled (the wardrobe after a
        // built-in one, the lavatory after a vanity) is left out.
        if st.role != Role::Other && done_roles.contains(&st.role) {
            continue;
        }
        if matches!(st.place, Place::Wall(_)) && st.keys.iter().any(|k| *k == "wardrobe" || *k == "refrigerator") {
            let dup = plan.assets.iter().any(|a| st.keys.contains(&a.catalog_key.as_str()) || (a.catalog_key == "wardrobe-builtin" && st.keys.contains(&"wardrobe")));
            if dup {
                continue;
            }
        }
        let mut why = BTreeMap::new();
        let mut placed_any = false;
        for key in &st.keys {
            let Some(item) = catalog_item(key) else { continue };
            let spec = Spec::of_item(item);
            let placed = place_step(&scene, &room, &spec, st, &anchors, &mut why);
            if placed.is_empty() {
                continue;
            }
            for p in placed {
                let index = plan.assets.len();
                let built_in = p.spec.built.map(|(_, params)| Box::new(params));
                let asset = Asset {
                    id: furnish_id(project, &room.id, key, index),
                    level_id: room.level_id.clone(),
                    catalog_key: item.key.clone(),
                    name: item.name.clone(),
                    category: item.category,
                    position: round_pt(p.center),
                    rotation_deg: p.rot,
                    width_mm: p.spec.w,
                    depth_mm: p.spec.d,
                    height_mm: p.spec.h,
                    elevation_mm: p.spec.elevation,
                    light: item.light,
                    links: vec![],
                    circuit: String::new(),
                    built_in,
                };
                let body = Body::of_asset(&asset);
                if st.role != Role::Other && !anchors.contains_key(&st.role) {
                    anchors.insert(st.role, body.clone());
                }
                scene.bodies.push(body);
                plan.assets.push(asset);
            }
            placed_any = true;
            break;
        }
        if placed_any {
            if st.role != Role::Other {
                done_roles.push(st.role);
            }
        } else if let Some(first) = st.keys.first() {
            let main = why.iter().max_by_key(|(_, n)| **n).map(|(w, _)| w.text()).unwrap_or("there is no free spot");
            if !plan.skipped.iter().any(|(k, _)| k == first) {
                plan.skipped.push((first.to_string(), main.to_string()));
            }
        }
    }
    // The fallback steps that never ran because the first choice fitted
    // are not "skipped".
    plan.skipped.retain(|(k, _)| !plan.assets.iter().any(|a| &a.catalog_key == k));
    if steps.iter().any(|s| s.keys.contains(&"lavatory")) && plan.assets.iter().any(|a| a.catalog_key == "vanity") {
        plan.skipped.retain(|(k, _)| k != "lavatory");
    }
    if plan.assets.iter().any(|a| a.catalog_key == "wardrobe-builtin") {
        plan.skipped.retain(|(k, _)| k != "wardrobe");
    }
    Ok(plan)
}

/// The rooms of a project with a closed floor, for "Furnish rooms": id,
/// name, usage, level and floor area.
pub fn furnishable_rooms(project: &Project) -> Vec<(Id, String, RoomUsage, Id, f64)> {
    let analysis = analyze(project);
    project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Room(r) => room_view(project, &analysis, &r.id).ok().map(|v| (v.id, v.name, v.usage, v.level_id, v.area_mm2 / 1.0e6)),
            _ => None,
        })
        .collect()
}

/// The catalog keys that suit a room usage, for the copilot's prompt.
pub fn keys_for_usage(usage: RoomUsage) -> Vec<&'static str> {
    catalog_items().iter().filter(|c| c.rooms.contains(&usage)).map(|c| c.key.as_str()).collect()
}
