//! Devices, fixtures and aircon (docs/CONTRACT.md, "Devices, fixtures and
//! links"): the object schedule and the device review items.
//!
//! What makes an object a device is its catalog item (`CatalogItem::device`,
//! `mount`, `aircon`). An object whose catalog key is unknown is not a
//! device. Guhit coordinates devices. It never checks circuits, loads or
//! ratings (the PEE's work under RA 7920) and never sizes aircon (the PME's).
//! Every finding is a suggestion.
//!
//! Clearances are the Koppel installation manual's figures (research,
//! 2026-09-23). "Left" and "right" are as seen facing the unit's front; the
//! front is local -y, the back (+y) is the wall side.
//!
//! Output order is fixed: the checks in the order of the contract table,
//! element order inside a check. No hash maps, no randomness.

use std::collections::{BTreeMap, BTreeSet};

use guhit_model::*;

use crate::geom::*;
use crate::issues::issue;
use crate::pipes::{capitalize, level_of, located, meters, name_start, opening_name, round_to, size};
use crate::rooms::FaceRef;
use crate::topo::Analysis;
use crate::validate::wall_height;

/// A line set serves the indoor unit one of its ends is this close to, in
/// plan. Its other end meets the outdoor unit the same way.
const LINESET_REACH_MM: f64 = 600.0;
/// A standard PH installation includes this much line set when the unit's
/// catalog item does not say.
const DEFAULT_INCLUDED_LINE_M: f64 = 3.0;
/// Split indoor unit: free space above and at each side, lowest underside.
const INDOOR_ABOVE_MM: f64 = 150.0;
const INDOOR_SIDE_MM: f64 = 120.0;
const INDOOR_UNDERSIDE_MM: f64 = 2300.0;
/// Outdoor unit: free space in front, behind, on the left and on the right.
const OUTDOOR_FRONT_MM: f64 = 2000.0;
const OUTDOOR_BACK_MM: f64 = 300.0;
const OUTDOOR_LEFT_MM: f64 = 300.0;
const OUTDOOR_RIGHT_MM: f64 = 600.0;
/// An outdoor unit higher than this above its floor is raised.
const RAISED_MM: f64 = 50.0;
/// A wall this close behind a raised unit can carry a bracket.
const BRACKET_REACH_MM: f64 = 600.0;
/// A slab, wall top or object this close under a raised unit carries it.
const SUPPORT_GAP_MM: f64 = 50.0;
/// The manual keeps an indoor unit this far from a TV, radio or computer.
const TV_DISTANCE_MM: f64 = 1000.0;
/// Clearance zones stop this short of their edges, so something that only
/// touches a zone's edge is not in it.
const EDGE_MM: f64 = 1.0;

// ---------------------------------------------------------------- catalog

/// The built-in object library, read once per derive.
pub(crate) struct Catalog(Vec<CatalogItem>);

impl Catalog {
    pub(crate) fn new() -> Self {
        Self(defaults::asset_catalog())
    }

    pub(crate) fn get(&self, key: &str) -> Option<&CatalogItem> {
        self.0.iter().find(|c| c.key == key)
    }
}

/// Kitchen and appliance items that PH plumbing fixture tables list: they
/// have a water supply and a drain. Every other kitchen or appliance item is
/// left out of the schedule.
pub(crate) const PLUMBING_FIXTURE_KEYS: [&str; 2] = ["kitchen-sink", "washing-machine"];

/// The schedule group of a catalog item, or None when the schedule leaves it
/// out: only devices, sanitary, lighting, electrical, aircon and utility
/// items, and the plumbing fixtures in `PLUMBING_FIXTURE_KEYS`, are counted.
pub(crate) fn schedule_group(
    key: &str,
    category: AssetCategory,
    device: Option<DeviceKind>,
) -> Option<ScheduleGroup> {
    if PLUMBING_FIXTURE_KEYS.contains(&key) {
        return Some(ScheduleGroup::Plumbing);
    }
    match category {
        AssetCategory::Sanitary => Some(ScheduleGroup::Plumbing),
        AssetCategory::Lighting | AssetCategory::Electrical => Some(ScheduleGroup::Electrical),
        AssetCategory::Aircon => Some(ScheduleGroup::Aircon),
        AssetCategory::Utility => Some(ScheduleGroup::Utility),
        _ => device.map(|d| match d {
            DeviceKind::AirconIndoor | DeviceKind::AirconOutdoor | DeviceKind::AirconWindow => {
                ScheduleGroup::Aircon
            }
            _ => ScheduleGroup::Electrical,
        }),
    }
}

// ------------------------------------------------------------------ rooms

/// Which room an object stands in: the room of the face that holds its
/// position (`Analysis::face_at`, the smallest face by centerlines).
pub(crate) struct RoomIndex<'a> {
    analysis: &'a Analysis,
    assigned: &'a BTreeMap<Id, FaceRef>,
    rooms: BTreeMap<FaceRef, &'a Room>,
    /// Element order of every room, for sorting.
    order: BTreeMap<&'a str, usize>,
}

impl<'a> RoomIndex<'a> {
    pub(crate) fn new(
        project: &'a Project,
        analysis: &'a Analysis,
        assigned: &'a BTreeMap<Id, FaceRef>,
    ) -> Self {
        let mut rooms = BTreeMap::new();
        let mut order = BTreeMap::new();
        let all = project.elements.iter().filter_map(|e| match e {
            Element::Room(r) => Some(r),
            _ => None,
        });
        for (i, r) in all.enumerate() {
            order.insert(r.id.as_str(), i);
            if let Some(f) = assigned.get(&r.id) {
                rooms.insert(*f, r);
            }
        }
        Self {
            analysis,
            assigned,
            rooms,
            order,
        }
    }

    /// The room whose face holds `p` on `level_id`. None outside every room.
    pub(crate) fn at(&self, level_id: &str, p: Point) -> Option<&'a Room> {
        if !is_finite(p) {
            return None;
        }
        let li = self
            .analysis
            .levels
            .iter()
            .position(|l| l.level_id == level_id)?;
        let fi = self.analysis.face_at(level_id, p)?;
        self.rooms.get(&(li, fi)).copied()
    }

    fn order_of(&self, room_id: &str) -> usize {
        self.order.get(room_id).copied().unwrap_or(usize::MAX)
    }
}

// --------------------------------------------------------------- schedule

/// `Derived::schedule`: how many of each counted catalog item stand in each
/// room of each level (see `schedule_group` for what is counted). Sorted by
/// level (project order), room (element order, outside last), group, then
/// catalog key.
pub(crate) fn schedule(project: &Project, rooms: &RoomIndex, catalog: &Catalog) -> Vec<ScheduleRow> {
    let level_order = |id: &str| {
        project
            .levels
            .iter()
            .position(|l| l.id == id)
            .unwrap_or(usize::MAX)
    };
    type Key = (usize, Id, usize, Option<Id>, ScheduleGroup, String);
    let mut rows: BTreeMap<Key, ScheduleRow> = BTreeMap::new();
    for el in &project.elements {
        let Element::Asset(a) = el else { continue };
        let item = catalog.get(&a.catalog_key);
        let (category, device) = match item {
            Some(i) => (i.category, i.device),
            None => (a.category, None),
        };
        let Some(group) = schedule_group(&a.catalog_key, category, device) else {
            continue;
        };
        let room = rooms.at(&a.level_id, a.position);
        let room_id = room.map(|r| r.id.clone());
        let key: Key = (
            level_order(&a.level_id),
            a.level_id.clone(),
            room.map(|r| rooms.order_of(&r.id)).unwrap_or(usize::MAX),
            room_id.clone(),
            group,
            a.catalog_key.clone(),
        );
        rows.entry(key)
            .or_insert_with(|| ScheduleRow {
                level_id: a.level_id.clone(),
                room_id,
                group,
                catalog_key: a.catalog_key.clone(),
                device,
                count: 0,
            })
            .count += 1;
    }
    rows.into_values().collect()
}

// ---------------------------------------------------------------- objects

/// Short noun for a catalog item in review items.
fn noun_of(key: &str) -> Option<&'static str> {
    Some(match key {
        "light-ceiling" => "ceiling light",
        "light-downlight" => "downlight",
        "light-tube" => "tube light",
        "light-pendant" => "pendant light",
        "light-wall" => "wall light",
        "light-outdoor" => "outdoor light",
        "light-floor-lamp" => "floor lamp",
        "light-table-lamp" => "table lamp",
        "outlet-duplex" => "outlet",
        "outlet-counter" => "counter outlet",
        "outlet-outdoor" => "outdoor outlet",
        "outlet-spo" => "special purpose outlet",
        "outlet-aircon" => "aircon outlet",
        "switch-1" | "switch-2" | "switch-3" => "switch",
        "panelboard" => "panelboard",
        "smoke-detector" => "smoke detector",
        "doorbell-button" => "doorbell button",
        "doorbell-chime" => "doorbell chime",
        "tv-console" => "TV console",
        "aircon-window" => "window aircon",
        k if k.starts_with("aircon-indoor") => "aircon indoor unit",
        k if k.starts_with("aircon-outdoor") => "aircon outdoor unit",
        _ => return None,
    })
}

/// A catalog or object name as a noun inside a sentence: the part before
/// the first comma, first letter lowercased unless it starts an acronym.
/// "Water tank, 1000 L" is "water tank", "LPG cylinder, 11 kg" stays "LPG
/// cylinder".
fn lower_first(s: &str) -> String {
    let head = s.split(',').next().unwrap_or(s).trim();
    let mut chars = head.chars();
    match (chars.next(), chars.next()) {
        (Some(a), Some(b)) if a.is_uppercase() && b.is_uppercase() => head.to_string(),
        (Some(a), _) => a.to_lowercase().chain(head.chars().skip(1)).collect(),
        (None, _) => String::new(),
    }
}

/// "a, b and c".
fn join_and(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [head @ .., last] => format!("{} and {last}", head.join(", ")),
    }
}

/// An object placed in the plan, with its catalog item and room.
struct Placed<'a> {
    asset: &'a Asset,
    item: Option<&'a CatalogItem>,
    room: Option<&'a Room>,
    /// Absolute elevation of its level's floor, and the level height.
    floor: f64,
    level_height: f64,
    /// Local axes in plan: +x along the width, +y toward the back.
    ax: Point,
    ay: Point,
}

impl<'a> Placed<'a> {
    fn new(
        project: &Project,
        asset: &'a Asset,
        item: Option<&'a CatalogItem>,
        room: Option<&'a Room>,
    ) -> Option<Self> {
        let a = asset;
        let numbers = [
            a.position.x,
            a.position.y,
            a.rotation_deg,
            a.width_mm,
            a.depth_mm,
            a.height_mm,
            a.elevation_mm,
        ];
        if !numbers.iter().all(|v| v.is_finite()) {
            return None;
        }
        let (floor, level_height) = level_of(project, &a.level_id);
        let (s, c) = a.rotation_deg.to_radians().sin_cos();
        Some(Self {
            asset,
            item,
            room,
            floor,
            level_height,
            ax: pt(c, s),
            ay: pt(-s, c),
        })
    }

    fn id(&self) -> &'a Id {
        &self.asset.id
    }

    fn device(&self) -> Option<DeviceKind> {
        self.item.and_then(|i| i.device)
    }

    fn mount(&self) -> Mount {
        self.item.map(|i| i.mount).unwrap_or_default()
    }

    fn spec(&self) -> Option<&'a AirconSpec> {
        self.item.and_then(|i| i.aircon.as_ref())
    }

    fn role(&self) -> Option<AirconRole> {
        self.spec().map(|s| s.role)
    }

    fn hw(&self) -> f64 {
        self.asset.width_mm / 2.0
    }

    fn hd(&self) -> f64 {
        self.asset.depth_mm / 2.0
    }

    /// Local to plan.
    fn world(&self, x: f64, y: f64) -> Point {
        add(self.asset.position, add(scale(self.ax, x), scale(self.ay, y)))
    }

    /// The local box x0..x1 by y0..y1 in plan, counter-clockwise.
    fn box_poly(&self, x0: f64, x1: f64, y0: f64, y1: f64) -> Vec<Point> {
        vec![
            self.world(x0, y0),
            self.world(x1, y0),
            self.world(x1, y1),
            self.world(x0, y1),
        ]
    }

    fn footprint(&self) -> Vec<Point> {
        self.box_poly(-self.hw(), self.hw(), -self.hd(), self.hd())
    }

    /// Absolute underside and top.
    fn bottom(&self) -> f64 {
        self.floor + self.asset.elevation_mm
    }

    fn top(&self) -> f64 {
        self.bottom() + self.asset.height_mm
    }

    /// Middle of the object, z above its own floor: `Issue::location`.
    fn center(&self) -> Vec3 {
        Vec3 {
            x: self.asset.position.x,
            y: self.asset.position.y,
            z: self.asset.elevation_mm + self.asset.height_mm / 2.0,
        }
    }

    /// The user's own name, when it is not the catalog name.
    fn own_name(&self) -> Option<&'a str> {
        let name = self.asset.name.trim();
        let catalog = self.item.map(|i| i.name.as_str());
        (!name.is_empty() && Some(name) != catalog).then_some(name)
    }

    fn noun(&self) -> String {
        if let Some(n) = noun_of(&self.asset.catalog_key) {
            return n.to_string();
        }
        if let Some(item) = self.item {
            return lower_first(&item.name);
        }
        match self.asset.name.trim() {
            "" => "object".to_string(),
            n => lower_first(n),
        }
    }

    fn room_name(&self) -> Option<&'a str> {
        self.room.map(|r| r.name.trim()).filter(|n| !n.is_empty())
    }

    /// "Bedroom ceiling light", "Outdoor light", or the user's own name, for
    /// the start of a sentence.
    fn start(&self) -> String {
        if let Some(n) = self.own_name() {
            return capitalize(n);
        }
        match self.room_name() {
            Some(room) => format!("{room} {}", self.noun()),
            None => capitalize(&self.noun()),
        }
    }

    /// "the Bedroom ceiling light", "the outdoor light", or the user's own
    /// name, inside a sentence.
    fn mid(&self) -> String {
        if let Some(n) = self.own_name() {
            return n.to_string();
        }
        match self.room_name() {
            Some(room) => format!("the {room} {}", self.noun()),
            None => format!("the {}", self.noun()),
        }
    }
}

// --------------------------------------------------------- plan geometry

fn polys_overlap(a: &[Point], b: &[Point]) -> bool {
    if a.len() < 3 || b.len() < 3 {
        return false;
    }
    for i in 0..a.len() {
        let (p, q) = (a[i], a[(i + 1) % a.len()]);
        for j in 0..b.len() {
            if segments_touch(p, q, b[j], b[(j + 1) % b.len()]) {
                return true;
            }
        }
    }
    point_in_polygon(a[0], b) || point_in_polygon(b[0], a)
}

/// Plan distance from `p` to a polygon, 0 inside it.
fn point_poly_distance(p: Point, poly: &[Point]) -> f64 {
    if point_in_polygon(p, poly) {
        return 0.0;
    }
    dist_to_boundary(p, poly)
}

/// Plan distance between two polygons, 0 when they overlap.
fn poly_distance(a: &[Point], b: &[Point]) -> f64 {
    if polys_overlap(a, b) {
        return 0.0;
    }
    let one_way = |x: &[Point], y: &[Point]| {
        x.iter()
            .map(|p| dist_to_boundary(*p, y))
            .fold(f64::INFINITY, f64::min)
    };
    one_way(a, b).min(one_way(b, a))
}

/// Something that takes up room around an aircon unit.
struct Obstacle<'a> {
    id: &'a Id,
    level_id: &'a str,
    /// "a wall", "a column", "the Bedroom wardrobe".
    name: String,
    poly: Vec<Point>,
    /// Absolute bottom and top.
    z0: f64,
    z1: f64,
    wall: bool,
}

/// Flush plates on a wall (outlets, switches, buttons) take up no room.
fn is_flush_plate(device: Option<DeviceKind>) -> bool {
    matches!(
        device,
        Some(
            DeviceKind::ConvenienceReceptacle
                | DeviceKind::SpecialPurposeOutlet
                | DeviceKind::Switch
                | DeviceKind::PushButton
                | DeviceKind::Buzzer
        )
    )
}

fn obstacles<'a>(project: &'a Project, analysis: &'a Analysis, placed: &[Placed<'a>]) -> Vec<Obstacle<'a>> {
    let mut out = vec![];
    for el in &project.elements {
        match el {
            Element::Wall(w) => {
                let Some(topo) = analysis.wall(&w.id) else {
                    continue;
                };
                if topo.outline.len() < 3 {
                    continue;
                }
                let (floor, _) = level_of(project, &w.level_id);
                out.push(Obstacle {
                    id: &w.id,
                    level_id: &w.level_id,
                    name: "a wall".to_string(),
                    poly: topo.outline.clone(),
                    z0: floor,
                    z1: floor + wall_height(project, w),
                    wall: true,
                });
            }
            Element::Column(c) => {
                let numbers = [c.center.x, c.center.y, c.width_mm, c.depth_mm, c.rotation_deg];
                if !numbers.iter().all(|v| v.is_finite()) {
                    continue;
                }
                let (floor, height) = level_of(project, &c.level_id);
                let poly = match c.shape {
                    ColumnShape::Rect => {
                        let (s, co) = c.rotation_deg.to_radians().sin_cos();
                        let (hx, hy) = (c.width_mm / 2.0, c.depth_mm / 2.0);
                        [(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)]
                            .iter()
                            .map(|(x, y)| pt(c.center.x + x * co - y * s, c.center.y + x * s + y * co))
                            .collect()
                    }
                    ColumnShape::Round => (0..16)
                        .map(|i| {
                            let t = i as f64 / 16.0 * std::f64::consts::TAU;
                            pt(
                                c.center.x + c.width_mm / 2.0 * t.cos(),
                                c.center.y + c.width_mm / 2.0 * t.sin(),
                            )
                        })
                        .collect(),
                };
                out.push(Obstacle {
                    id: &c.id,
                    level_id: &c.level_id,
                    name: "a column".to_string(),
                    poly,
                    z0: floor,
                    z1: floor + height,
                    wall: false,
                });
            }
            _ => {}
        }
    }
    for p in placed {
        if is_flush_plate(p.device()) {
            continue;
        }
        out.push(Obstacle {
            id: p.id(),
            level_id: &p.asset.level_id,
            name: p.mid(),
            poly: p.footprint(),
            z0: p.bottom(),
            z1: p.top(),
            wall: false,
        });
    }
    out
}

/// The obstacle nearest to `unit` among those inside the plan `zone` between
/// heights `z0` and `z1`, with its plan distance from the unit.
fn nearest_in_zone<'o, 'a>(
    obstacles: &'o [Obstacle<'a>],
    unit: &Placed,
    zone: &[Point],
    z0: f64,
    z1: f64,
    skip: &BTreeSet<&Id>,
) -> Option<(&'o Obstacle<'a>, f64)> {
    let footprint = unit.footprint();
    let mut best: Option<(&Obstacle, f64)> = None;
    for o in obstacles {
        if o.id == unit.id() || skip.contains(o.id) || o.level_id != unit.asset.level_id {
            continue;
        }
        let beside = o.z0 < z1 - EDGE_MM && o.z1 > z0 + EDGE_MM;
        if !(beside && polys_overlap(zone, &o.poly)) {
            continue;
        }
        let d = poly_distance(&footprint, &o.poly);
        if best.map(|(_, bd)| d < bd).unwrap_or(true) {
            best = Some((o, d));
        }
    }
    best
}

fn mm_text(v: f64) -> String {
    format!("{:.0}", round_to(v.max(0.0), 1.0))
}

// ------------------------------------------------------------------ doors

/// One swinging leaf of a door: its hinge on the wall centerline, the
/// direction along the wall away from the opening, the side it swings to,
/// its width and the door head.
struct Leaf<'a> {
    opening: &'a Opening,
    level_id: &'a str,
    hinge: Point,
    away: Point,
    swing: Point,
    width: f64,
    head: f64,
}

fn door_leaves(project: &Project) -> Vec<Leaf<'_>> {
    let mut out = vec![];
    for el in &project.elements {
        let Element::Opening(o) = el else { continue };
        if o.opening_type != OpeningType::Door
            || !matches!(o.style, OpeningStyle::SwingSingle | OpeningStyle::SwingDouble)
        {
            continue;
        }
        let Some(Element::Wall(w)) = project.elements.iter().find(|e| e.id() == &o.wall_id) else {
            continue;
        };
        let Some(d) = unit(sub(w.end, w.start)) else {
            continue;
        };
        if ![o.offset_mm, o.width_mm, o.height_mm, o.sill_mm]
            .iter()
            .all(|v| v.is_finite())
        {
            continue;
        }
        let n = perp_left(d);
        let swing = if o.flip_side { scale(n, -1.0) } else { n };
        let near = add(w.start, scale(d, o.offset_mm - o.width_mm / 2.0));
        let far = add(w.start, scale(d, o.offset_mm + o.width_mm / 2.0));
        let back = scale(d, -1.0);
        let head = o.sill_mm + o.height_mm;
        let leaf = |hinge: Point, away: Point, width: f64| Leaf {
            opening: o,
            level_id: &w.level_id,
            hinge,
            away,
            swing,
            width,
            head,
        };
        match o.style {
            OpeningStyle::SwingDouble => {
                out.push(leaf(near, back, o.width_mm / 2.0));
                out.push(leaf(far, d, o.width_mm / 2.0));
            }
            _ => out.push(if o.flip_hinge {
                leaf(far, d, o.width_mm)
            } else {
                leaf(near, back, o.width_mm)
            }),
        }
    }
    out
}

/// True when `p` is behind the open leaf: on its swing side, beyond the
/// hinge along the wall, and within the leaf's reach of the hinge.
fn behind_leaf(leaf: &Leaf, p: Point) -> bool {
    let q = sub(p, leaf.hinge);
    let (u, v) = (dot(q, leaf.away), dot(q, leaf.swing));
    u >= 0.0 && v > 0.0 && (u * u + v * v).sqrt() <= leaf.width + 1e-6
}

// --------------------------------------------------------------- line sets

/// A refrigerant run and the units at its ends.
struct LineSet<'a> {
    pipe: &'a Pipe,
    /// Centerline length and the height difference of its ends, mm.
    length: f64,
    rise: f64,
    /// Index into `placed` of the indoor unit it serves, and that end,
    /// z above the pipe's floor.
    indoor: Option<(usize, Vec3)>,
    /// Index into `placed` of the outdoor unit at its other end.
    outdoor: Option<usize>,
}

fn line_sets<'a>(project: &'a Project, placed: &[Placed<'a>]) -> Vec<LineSet<'a>> {
    let mut out = vec![];
    for el in &project.elements {
        let Element::Pipe(p) = el else { continue };
        if p.system != PipeSystem::Refrigerant || p.points.len() < 2 {
            continue;
        }
        if !p
            .points
            .iter()
            .all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite())
        {
            continue;
        }
        let (floor, _) = level_of(project, &p.level_id);
        let length: f64 = p
            .points
            .windows(2)
            .map(|w| {
                ((w[1].x - w[0].x).powi(2) + (w[1].y - w[0].y).powi(2) + (w[1].z - w[0].z).powi(2))
                    .sqrt()
            })
            .sum();
        let (first, last) = (p.points[0], p.points[p.points.len() - 1]);
        let rise = (last.z - first.z).abs();
        // The unit nearest to an end in plan, then in height, then first in
        // element order.
        let nearest = |end: Vec3, role: AirconRole| -> Option<(usize, f64, f64)> {
            let mut best: Option<(usize, f64, f64)> = None;
            for (i, u) in placed.iter().enumerate() {
                if u.role() != Some(role) {
                    continue;
                }
                let d = point_poly_distance(pt(end.x, end.y), &u.footprint());
                if d > LINESET_REACH_MM {
                    continue;
                }
                let dz = (floor + end.z - (u.bottom() + u.top()) / 2.0).abs();
                if best.map(|(_, bd, bz)| d < bd - 1e-9 || (d <= bd + 1e-9 && dz < bz - 1e-9)).unwrap_or(true) {
                    best = Some((i, d, dz));
                }
            }
            best
        };
        let at_first = nearest(first, AirconRole::Indoor);
        let at_last = nearest(last, AirconRole::Indoor);
        let (indoor, other) = match (at_first, at_last) {
            (Some(a), Some(b)) if b.1 < a.1 - 1e-9 || (b.1 <= a.1 + 1e-9 && b.2 < a.2 - 1e-9) => {
                (Some((b.0, last)), Some(first))
            }
            (Some(a), _) => (Some((a.0, first)), Some(last)),
            (None, Some(b)) => (Some((b.0, last)), Some(first)),
            (None, None) => (None, None),
        };
        let outdoor = other
            .and_then(|end| nearest(end, AirconRole::Outdoor))
            .map(|(i, _, _)| i);
        out.push(LineSet {
            pipe: p,
            length,
            rise,
            indoor,
            outdoor,
        });
    }
    out
}

// ------------------------------------------------------------ the checks

/// The device and aircon review items (docs/CONTRACT.md, "Devices, fixtures
/// and links"), in the order of the contract table.
pub(crate) fn device_issues(
    project: &Project,
    analysis: &Analysis,
    rooms: &RoomIndex,
    catalog: &Catalog,
) -> Vec<Issue> {
    let placed: Vec<Placed> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Asset(a) => Placed::new(
                project,
                a,
                catalog.get(&a.catalog_key),
                rooms.at(&a.level_id, a.position),
            ),
            _ => None,
        })
        .collect();
    let sets = line_sets(project, &placed);
    if placed.is_empty() && sets.is_empty() {
        return vec![];
    }
    let ids: BTreeSet<&Id> = project.elements.iter().map(|e| e.id()).collect();
    let mut out = vec![];

    // Lights and switches.
    let switches: Vec<&Placed> = placed
        .iter()
        .filter(|p| p.device() == Some(DeviceKind::Switch))
        .collect();
    let switched: BTreeSet<&Id> = switches.iter().flat_map(|s| s.asset.links.iter()).collect();
    for light in placed.iter().filter(|p| {
        p.device() == Some(DeviceKind::LightingOutlet) && matches!(p.mount(), Mount::Ceiling | Mount::Wall)
    }) {
        if switched.contains(light.id()) {
            continue;
        }
        out.push(located(
            issue(
                "light_no_switch",
                Severity::Info,
                format!(
                    "{} has no switch. Link a switch with the link tool (L).",
                    light.start()
                ),
                vec![light.id().clone()],
            ),
            light.center(),
        ));
    }
    for s in &switches {
        if s.asset.links.iter().any(|l| ids.contains(l)) {
            continue;
        }
        out.push(located(
            issue(
                "switch_no_load",
                Severity::Info,
                format!(
                    "{} controls nothing yet. Link it to its lights with the link tool (L), or remove it.",
                    s.start()
                ),
                vec![s.id().clone()],
            ),
            s.center(),
        ));
    }
    let leaves = door_leaves(project);
    if !leaves.is_empty() {
        for s in &switches {
            let mut doors: Vec<&Opening> = vec![];
            for leaf in &leaves {
                if leaf.level_id != s.asset.level_id
                    || s.asset.elevation_mm >= leaf.head
                    || doors.iter().any(|d| d.id == leaf.opening.id)
                {
                    continue;
                }
                if behind_leaf(leaf, s.asset.position) {
                    doors.push(leaf.opening);
                }
            }
            for door in doors {
                out.push(located(
                    issue(
                        "switch_behind_door",
                        Severity::Warning,
                        format!(
                            "{} is behind {} when the door is open. Move it to the latch side, about 200 mm from the frame.",
                            s.start(),
                            opening_name(project, analysis, rooms.assigned, door)
                        ),
                        vec![s.id().clone(), door.id.clone()],
                    ),
                    s.center(),
                ));
            }
        }
    }

    // Aircon outlets. A split unit counts as fed when an outlet links it or
    // the outdoor unit its line set reaches.
    let fed: BTreeSet<&Id> = placed
        .iter()
        .filter(|p| p.device() == Some(DeviceKind::SpecialPurposeOutlet))
        .flat_map(|p| p.asset.links.iter())
        .collect();
    for (i, u) in placed.iter().enumerate() {
        let role = u.role();
        if !matches!(role, Some(AirconRole::Indoor | AirconRole::Window)) {
            continue;
        }
        let through_outdoor = sets.iter().any(|ls| {
            ls.indoor.map(|(k, _)| k) == Some(i)
                && ls
                    .outdoor
                    .map(|o| fed.contains(placed[o].id()))
                    .unwrap_or(false)
        });
        if fed.contains(u.id()) || through_outdoor {
            continue;
        }
        out.push(located(
            issue(
                "aircon_no_outlet",
                Severity::Warning,
                format!(
                    "{} has no outlet linked to it. Add an aircon outlet near it and link them with the link tool (L).",
                    u.start()
                ),
                vec![u.id().clone()],
            ),
            u.center(),
        ));
    }

    // Line sets against the limits of the indoor unit they serve.
    for ls in &sets {
        let Some((k, end)) = ls.indoor else { continue };
        let u = &placed[k];
        let Some(spec) = u.spec() else { continue };
        let ids = vec![ls.pipe.id.clone(), u.id().clone()];
        if spec.max_line_m > 0.0 && ls.length > spec.max_line_m * 1000.0 + 0.5 {
            out.push(located(
                issue(
                    "lineset_long",
                    Severity::Warning,
                    format!(
                        "{} is {} long, over the {} m {} allows. Move the outdoor unit closer, or check the unit's own manual.",
                        name_start(ls.pipe),
                        meters(ls.length),
                        size(spec.max_line_m),
                        u.mid()
                    ),
                    ids.clone(),
                ),
                end,
            ));
        }
        if spec.max_rise_m > 0.0 && ls.rise > spec.max_rise_m * 1000.0 + 0.5 {
            out.push(located(
                issue(
                    "lineset_rise",
                    Severity::Warning,
                    format!(
                        "{} has its ends {} apart in height, over the {} m {} allows. Bring the outdoor unit nearer the indoor unit's level, or check the unit's own manual.",
                        name_start(ls.pipe),
                        meters(ls.rise),
                        size(spec.max_rise_m),
                        u.mid()
                    ),
                    ids.clone(),
                ),
                end,
            ));
        }
        if spec.min_line_m > 0.0 && ls.length < spec.min_line_m * 1000.0 - 0.5 {
            out.push(located(
                issue(
                    "lineset_short",
                    Severity::Info,
                    format!(
                        "{} is {} long, under the {} m minimum in the manual of {}, which keeps vibration and noise down. Move the outdoor unit a little farther, or add a loop to the line set.",
                        name_start(ls.pipe),
                        meters(ls.length),
                        size(spec.min_line_m),
                        u.mid()
                    ),
                    ids,
                ),
                end,
            ));
        }
    }
    for ls in &sets {
        let included = ls
            .indoor
            .and_then(|(k, _)| placed[k].spec())
            .map(|s| s.included_line_m)
            .filter(|v| *v > 0.0)
            .unwrap_or(DEFAULT_INCLUDED_LINE_M);
        let extra = ls.length - included * 1000.0;
        if extra <= 5.0 {
            continue;
        }
        out.push(issue(
            "lineset_extra",
            Severity::Info,
            format!(
                "{} is {} long, {} more than the {} m a standard installation includes. Installers usually charge for each extra meter.",
                name_start(ls.pipe),
                meters(ls.length),
                meters(extra),
                size(included)
            ),
            vec![ls.pipe.id.clone()],
        ));
    }

    // Unit clearances.
    let things = obstacles(project, analysis, &placed);
    for u in placed.iter().filter(|p| p.role() == Some(AirconRole::Indoor)) {
        let (hw, hd) = (u.hw(), u.hd());
        let (bottom, top) = (u.bottom(), u.top());
        // The wall the unit hangs on: it holds the point just behind its back.
        let behind = u.world(0.0, hd + EDGE_MM);
        let host: BTreeSet<&Id> = things
            .iter()
            .filter(|o| o.wall && o.level_id == u.asset.level_id && point_in_polygon(behind, &o.poly))
            .map(|o| o.id)
            .collect();
        let mut problems: Vec<String> = vec![];
        let mut above = u.floor + u.level_height - top;
        let inner = u.box_poly(-hw + EDGE_MM, hw - EDGE_MM, -hd + EDGE_MM, hd - EDGE_MM);
        for o in &things {
            if o.id == u.id() || host.contains(o.id) || o.level_id != u.asset.level_id {
                continue;
            }
            if o.z0 >= top - EDGE_MM && polys_overlap(&inner, &o.poly) {
                above = above.min(o.z0 - top);
            }
        }
        if above < INDOOR_ABOVE_MM - 0.5 {
            problems.push(format!("{} mm free above it", mm_text(above)));
        }
        for (side, x0, x1) in [
            ("left", -hw - INDOOR_SIDE_MM + EDGE_MM, -hw),
            ("right", hw, hw + INDOOR_SIDE_MM - EDGE_MM),
        ] {
            let zone = u.box_poly(x0, x1, -hd + EDGE_MM, hd - EDGE_MM);
            if let Some((_, d)) = nearest_in_zone(&things, u, &zone, bottom, top, &host) {
                problems.push(format!("{} mm free at its {side} side", mm_text(d)));
            }
        }
        if u.asset.elevation_mm < INDOOR_UNDERSIDE_MM - 0.5 {
            problems.push(format!(
                "its underside is {} above the floor",
                meters(u.asset.elevation_mm)
            ));
        }
        if problems.is_empty() {
            continue;
        }
        out.push(located(
            issue(
                "indoor_unit_clearance",
                Severity::Warning,
                format!(
                    "{} has too little clearance: {}. Its manual asks for 150 mm above, 120 mm at each side and the underside at 2.30 m or higher, for air flow and service.",
                    u.start(),
                    join_and(&problems)
                ),
                vec![u.id().clone()],
            ),
            u.center(),
        ));
    }
    let none: BTreeSet<&Id> = BTreeSet::new();
    for u in placed.iter().filter(|p| p.role() == Some(AirconRole::Outdoor)) {
        let (hw, hd) = (u.hw(), u.hd());
        let (bottom, top) = (u.bottom(), u.top());
        let zones = [
            (
                "in front of it",
                u.box_poly(-hw + EDGE_MM, hw - EDGE_MM, -hd - OUTDOOR_FRONT_MM + EDGE_MM, -hd),
            ),
            (
                "behind it",
                u.box_poly(-hw + EDGE_MM, hw - EDGE_MM, hd, hd + OUTDOOR_BACK_MM - EDGE_MM),
            ),
            (
                "on its left",
                u.box_poly(-hw - OUTDOOR_LEFT_MM + EDGE_MM, -hw, -hd + EDGE_MM, hd - EDGE_MM),
            ),
            (
                "on its right",
                u.box_poly(hw, hw + OUTDOOR_RIGHT_MM - EDGE_MM, -hd + EDGE_MM, hd - EDGE_MM),
            ),
        ];
        let mut problems: Vec<String> = vec![];
        for (place, zone) in &zones {
            if let Some((o, d)) = nearest_in_zone(&things, u, zone, bottom, top, &none) {
                problems.push(format!("{} {} mm {place}", o.name, mm_text(d)));
            }
        }
        if problems.is_empty() {
            continue;
        }
        out.push(located(
            issue(
                "outdoor_unit_clearance",
                Severity::Warning,
                format!(
                    "{} has too little free space around it: {}. Its manual asks for 2000 mm in front, 300 mm behind, 300 mm on the left and 600 mm on the right, for air flow and service.",
                    u.start(),
                    join_and(&problems)
                ),
                vec![u.id().clone()],
            ),
            u.center(),
        ));
    }
    for u in placed.iter().filter(|p| p.role() == Some(AirconRole::Outdoor)) {
        if u.asset.elevation_mm <= RAISED_MM || supported(project, analysis, &things, &placed, u) {
            continue;
        }
        out.push(located(
            issue(
                "outdoor_unit_unsupported",
                Severity::Warning,
                format!(
                    "{} stands {} above the floor with nothing under it. Set it on a slab or a ledge, or hang it on a wall bracket.",
                    u.start(),
                    meters(u.asset.elevation_mm)
                ),
                vec![u.id().clone()],
            ),
            u.center(),
        ));
    }

    // A TV near an indoor or window unit.
    for u in placed
        .iter()
        .filter(|p| matches!(p.role(), Some(AirconRole::Indoor | AirconRole::Window)))
    {
        let footprint = u.footprint();
        for tv in placed.iter().filter(|p| {
            p.asset.catalog_key == "tv-console" && p.asset.level_id == u.asset.level_id
        }) {
            let d = poly_distance(&footprint, &tv.footprint());
            if d >= TV_DISTANCE_MM - 0.5 {
                continue;
            }
            let where_ = if d < 0.5 {
                format!("is right above {}", tv.mid())
            } else {
                format!("is {} mm from {}", mm_text(d), tv.mid())
            };
            out.push(located(
                issue(
                    "unit_near_tv",
                    Severity::Info,
                    format!(
                        "{} {where_}. Its manual keeps the unit at least 1 m from a TV, radio or computer; move one of them.",
                        u.start()
                    ),
                    vec![u.id().clone(), tv.id().clone()],
                ),
                u.center(),
            ));
        }
    }
    out
}

/// A raised outdoor unit is carried by a wall bracket (a wall right behind
/// it at its height), a slab (a level floor under it inside a footprint),
/// a wall top or an object top under it.
fn supported(
    project: &Project,
    analysis: &Analysis,
    things: &[Obstacle],
    placed: &[Placed],
    u: &Placed,
) -> bool {
    let bottom = u.bottom();
    let center = u.asset.position;
    let (back, reach) = (u.world(0.0, u.hd()), u.world(0.0, u.hd() + BRACKET_REACH_MM));
    let bracket = things.iter().any(|o| {
        o.wall
            && o.level_id == u.asset.level_id
            && o.z0 <= bottom + SUPPORT_GAP_MM
            && o.z1 >= bottom - SUPPORT_GAP_MM
            && (point_in_polygon(back, &o.poly)
                || (0..o.poly.len()).any(|i| {
                    segments_touch(back, reach, o.poly[i], o.poly[(i + 1) % o.poly.len()])
                }))
    });
    if bracket {
        return true;
    }
    let slab = project.levels.iter().any(|l| {
        (l.elevation_mm - bottom).abs() <= SUPPORT_GAP_MM
            && analysis
                .level(&l.id)
                .map(|t| t.footprints.iter().any(|(fp, _)| point_in_polygon(center, fp)))
                .unwrap_or(false)
    });
    if slab {
        return true;
    }
    let wall_top = things.iter().any(|o| {
        o.wall && (o.z1 - bottom).abs() <= SUPPORT_GAP_MM && point_in_polygon(center, &o.poly)
    });
    if wall_top {
        return true;
    }
    placed.iter().any(|p| {
        p.id() != u.id()
            && (p.top() - bottom).abs() <= SUPPORT_GAP_MM
            && point_in_polygon(center, &p.footprint())
    })
}
