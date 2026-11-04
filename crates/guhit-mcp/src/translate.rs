//! MCP v2 editing tools: arguments to typed `Command`s (DECISIONS D3).
//!
//! Every editing tool, alone, in `batch` or as an entry of a `build` spec,
//! goes through `plan`: the arguments are parsed (unknown fields refused),
//! checked against the project as staged so far, and turned into commands.
//! Nothing here touches the document; `stage` runs the commands on one
//! staged copy and commits them as one undo step.
//!
//! The v1 editing tools still translate through the copilot
//! (`guhit_app::ai::tools::to_commands`), so they behave exactly as before.

use std::cell::OnceCell;

use guhit_app::ai::tools as copilot;
use guhit_model::*;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::fail::ToolFail;
use crate::geo::{self, pt};

/// The project as staged so far, with its derived data worked out once and
/// only when a tool asks for it.
pub(crate) struct View<'a> {
    pub project: &'a Project,
    derived: OnceCell<Derived>,
}

impl<'a> View<'a> {
    pub fn new(project: &'a Project) -> Self {
        Self { project, derived: OnceCell::new() }
    }

    pub fn with_derived(project: &'a Project, derived: Derived) -> Self {
        let cell = OnceCell::new();
        let _ = cell.set(derived);
        Self { project, derived: cell }
    }

    pub fn derived(&self) -> &Derived {
        self.derived.get_or_init(|| guhit_core::compute_derived(self.project))
    }
}

/// What a `ref` names once the step ran.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Primary {
    Kind(ElementKind),
    Level,
    None,
}

/// Commands that can only be worked out once the step's commands ran.
#[derive(Debug, Clone)]
pub(crate) enum Then {
    /// Name the room the walls closed around `point`.
    Room {
        level_id: Id,
        point: Point,
        name: Option<String>,
        usage: Option<RoomUsage>,
        floor_material_id: Option<Id>,
    },
    /// Give every element the commands added this material.
    Material { material_id: Id },
}

/// One step, translated.
#[derive(Debug)]
pub(crate) struct Plan {
    pub commands: Vec<Command>,
    pub then: Vec<Then>,
    pub primary: Primary,
    /// Figures the step worked out, for the result: a stair's riser height.
    pub info: Option<Value>,
}

impl Plan {
    fn of(commands: Vec<Command>, primary: Primary) -> Self {
        Self { commands, then: vec![], primary, info: None }
    }
}

/// v2 tools that edit the plan and can be steps of `batch`.
pub const EDIT_TOOLS: [&str; 17] = [
    "add_walls",
    "add_room",
    "add_opening",
    "add_stair",
    "add_column",
    "add_slab",
    "place_asset",
    "add_camera",
    "modify",
    "reshape",
    "delete",
    "levels",
    "set_project",
    "edit_pipe",
    "add_pipe",
    "link_devices",
    "set_review_mark",
];

/// True for a tool `plan` translates: a v2 editing tool or a v1 one.
pub(crate) fn is_edit(name: &str) -> bool {
    EDIT_TOOLS.contains(&name) || copilot::is_edit(name)
}

/// Translate one editing call against the staged view. `default_level` is
/// where new elements go when the call names no level.
pub(crate) fn plan(view: &View, tool: &str, args: &Value, default_level: &Id) -> Result<Plan, ToolFail> {
    let p = view.project;
    match tool {
        "add_walls" => add_walls(p, args, default_level),
        "add_room" => add_room(p, args, default_level),
        "add_opening" => add_opening(view, args, default_level),
        "add_stair" => add_stair(p, args, default_level),
        "add_column" => add_column(p, args, default_level),
        "add_slab" => add_slab(p, args, default_level),
        "place_asset" => place_asset(p, args, default_level),
        "add_camera" => add_camera(p, args),
        "modify" => modify(view, args),
        "reshape" => reshape(p, args),
        "delete" => {
            let i: IdsIn = parse(args)?;
            ids_exist(p, &i.ids)?;
            Ok(Plan::of(vec![Command::DeleteElements { ids: i.ids }], Primary::None))
        }
        "levels" => levels(p, args),
        "set_project" => set_project(p, args),
        "edit_pipe" => edit_pipe(p, args, default_level),
        "add_pipe" => add_pipe(p, args, default_level),
        "link_devices" => link_devices(p, args),
        "set_review_mark" => set_review_mark(args),
        // A command a tool built itself (import_underlay). Never a step a
        // caller can name: `is_edit` is false for it.
        // Commands a tool worked out itself, several in one step
        // (import_dxf). Never a step a caller can name.
        "__commands" => {
            let commands: Vec<Command> = parse(&args["commands"])?;
            Ok(Plan::of(commands, Primary::None))
        }
        "__command" => {
            let command: Command = parse(args)?;
            let primary = match &command {
                Command::AddElement { element } => Primary::Kind(element.kind()),
                _ => Primary::None,
            };
            Ok(Plan::of(vec![command], primary))
        }
        v1 if copilot::is_edit(v1) => {
            let commands = copilot::to_commands(p, v1, args, default_level)?;
            Ok(Plan::of(commands, v1_primary(v1)))
        }
        other => Err(ToolFail::new("unknown_tool", format!("`{other}` is not an editing tool"))),
    }
}

fn v1_primary(tool: &str) -> Primary {
    match tool {
        "add_wall" | "add_wall_chain" => Primary::Kind(ElementKind::Wall),
        "add_rect_room" => Primary::Kind(ElementKind::Room),
        "add_door" | "add_window" => Primary::Kind(ElementKind::Opening),
        "add_asset" => Primary::Kind(ElementKind::Asset),
        "add_level" => Primary::Level,
        _ => Primary::None,
    }
}

// ------------------------------------------------------------------ parsing

pub(crate) fn parse<T: DeserializeOwned>(args: &Value) -> Result<T, ToolFail> {
    serde_json::from_value(args.clone()).map_err(|e| ToolFail::args(e.to_string()))
}

/// A plan point: `{x, y}` or `[x, y]`, finite.
pub(crate) fn point(v: &Value, what: &str) -> Result<Point, ToolFail> {
    let (x, y) = match v {
        Value::Object(m) => {
            if let Some(extra) = m.keys().find(|k| !matches!(k.as_str(), "x" | "y")) {
                return Err(ToolFail::args(format!("{what} has an unknown field `{extra}`; a point is {{\"x\": mm, \"y\": mm}}")));
            }
            (m.get("x").and_then(Value::as_f64), m.get("y").and_then(Value::as_f64))
        }
        Value::Array(a) if a.len() == 2 => (a[0].as_f64(), a[1].as_f64()),
        _ => (None, None),
    };
    match (x, y) {
        (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Ok(pt(x, y)),
        _ => Err(ToolFail::args(format!("{what} must be a point {{\"x\": mm, \"y\": mm}} with finite numbers"))),
    }
}

fn points(list: &[Value], what: &str) -> Result<Vec<Point>, ToolFail> {
    list.iter().enumerate().map(|(i, v)| point(v, &format!("{what}[{i}]"))).collect()
}

/// A point with a height: `{x, y, z}`.
fn point3(v: &Value, what: &str) -> Result<Vec3, ToolFail> {
    let get = |k: &str| v.get(k).and_then(Value::as_f64).filter(|n| n.is_finite());
    match (get("x"), get("y"), get("z")) {
        (Some(x), Some(y), Some(z)) => Ok(Vec3 { x, y, z }),
        _ => Err(ToolFail::args(format!("{what} must be {{\"x\": mm, \"y\": mm, \"z\": mm}} with finite numbers"))),
    }
}

fn positive(v: f64, what: &str) -> Result<f64, ToolFail> {
    if v.is_finite() && v > 0.0 {
        Ok(v)
    } else {
        Err(ToolFail::args(format!("{what} must be a number greater than 0, got {v}")))
    }
}

fn positive_opt(v: Option<f64>, what: &str) -> Result<Option<f64>, ToolFail> {
    v.map(|v| positive(v, what)).transpose()
}

fn finite(v: f64, what: &str) -> Result<f64, ToolFail> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(ToolFail::args(format!("{what} must be a finite number")))
    }
}

fn find<'a>(project: &'a Project, id: &str) -> Result<&'a Element, ToolFail> {
    project
        .elements
        .iter()
        .find(|e| e.id() == id)
        .ok_or_else(|| ToolFail::new("not_found", format!("no element has id `{id}`")).with_ids(vec![id.to_string()]))
}

fn ids_exist(project: &Project, ids: &[Id]) -> Result<(), ToolFail> {
    if ids.is_empty() || ids.iter().any(|i| i.trim().is_empty()) {
        return Err(ToolFail::args("ids must hold at least one element id"));
    }
    for id in ids {
        find(project, id)?;
    }
    Ok(())
}

pub(crate) fn check_material(project: &Project, id: &str) -> Result<(), ToolFail> {
    if project.materials.iter().any(|m| m.id == id) {
        return Ok(());
    }
    let near = nearest_names(id, project.materials.iter().map(|m| m.id.as_str()), 3);
    Err(ToolFail::new("unknown_material", format!("no material has id `{id}`. Did you mean: {}?", near.join(", "))))
}

/// A level by id or name (case and spaces ignored), or `default`.
pub(crate) fn pick_level(project: &Project, level: Option<&str>, default: &Id) -> Result<Id, ToolFail> {
    let Some(wanted) = level.map(str::trim).filter(|l| !l.is_empty()) else {
        return Ok(default.clone());
    };
    find_level(project, wanted).map(|l| l.id.clone())
}

pub(crate) fn find_level<'a>(project: &'a Project, wanted: &str) -> Result<&'a Level, ToolFail> {
    let wanted = wanted.trim();
    if let Some(l) = project.levels.iter().find(|l| l.id == wanted) {
        return Ok(l);
    }
    let named: Vec<&Level> = project.levels.iter().filter(|l| l.name.trim().eq_ignore_ascii_case(wanted)).collect();
    match named.as_slice() {
        [one] => Ok(one),
        [] => {
            let known: Vec<String> = project.levels.iter().map(|l| format!("\"{}\" ({})", l.name, l.id)).collect();
            Err(ToolFail::new("unknown_level", format!("no level is called `{wanted}`. Levels: {}", known.join(", "))))
        }
        _ => Err(ToolFail::args(format!("more than one level is called `{wanted}`; give its id"))),
    }
}

/// Up to `n` names closest to `wanted`: substring matches first.
pub(crate) fn nearest_names<'a>(wanted: &str, names: impl Iterator<Item = &'a str>, n: usize) -> Vec<String> {
    let w = wanted.to_ascii_lowercase();
    let mut scored: Vec<(usize, &str)> = names
        .map(|name| {
            let l = name.to_ascii_lowercase();
            let d = crate::toolsets::distance(&w, &l);
            let score = if l.contains(&w) || w.contains(&l) { d.min(1) } else { d };
            (score, name)
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(b.1)));
    scored.into_iter().take(n).map(|(_, s)| s.to_string()).collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdsIn {
    ids: Vec<Id>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(deny_unknown_fields)]
struct RailingIn {
    height_mm: Option<f64>,
    style: Option<RailingStyle>,
}

/// `true`, `false` or `{height_mm, style}`. `default_height` fills a gap.
fn railing(v: Option<&Value>, default_height: f64, default: Option<Railing>) -> Result<Option<Railing>, ToolFail> {
    match v {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(false)) => Ok(None),
        Some(Value::Bool(true)) => Ok(Some(Railing { height_mm: default_height, style: RailingStyle::Bars })),
        Some(other) => {
            let r: RailingIn = parse(other)?;
            Ok(Some(Railing {
                height_mm: positive_opt(r.height_mm, "railing.height_mm")?.unwrap_or(default_height),
                style: r.style.unwrap_or(RailingStyle::Bars),
            }))
        }
    }
}

// -------------------------------------------------------------------- walls

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddWallsIn {
    points: Option<Vec<Value>>,
    start: Option<Value>,
    end: Option<Value>,
    closed: Option<bool>,
    thickness_mm: Option<f64>,
    height_mm: Option<f64>,
    level: Option<String>,
    role: Option<WallRole>,
    material_id: Option<String>,
}

fn add_walls(p: &Project, args: &Value, default_level: &Id) -> Result<Plan, ToolFail> {
    let i: AddWallsIn = parse(args)?;
    let mut pts = match (&i.points, &i.start, &i.end) {
        (Some(list), None, None) => points(list, "points")?,
        (None, Some(a), Some(b)) => vec![point(a, "start")?, point(b, "end")?],
        _ => return Err(ToolFail::args("give points (two or more), or start and end")),
    };
    let closed = i.closed.unwrap_or(false);
    if pts.len() < 2 || (closed && pts.len() < 3) {
        return Err(ToolFail::args("points needs at least 2 points, 3 when closed"));
    }
    if closed {
        pts.push(pts[0]);
    }
    let level_id = pick_level(p, i.level.as_deref(), default_level)?;
    if let Some(m) = &i.material_id {
        check_material(p, m)?;
    }
    let thickness_mm = positive_opt(i.thickness_mm, "thickness_mm")?;
    let height_mm = positive_opt(i.height_mm, "height_mm")?;
    let role = i.role.filter(|r| *r == WallRole::Site);
    let commands = pts
        .windows(2)
        .map(|s| Command::AddWall {
            start: s[0],
            end: s[1],
            thickness_mm,
            height_mm,
            material_id: i.material_id.clone(),
            level_id: Some(level_id.clone()),
            role,
        })
        .collect();
    Ok(Plan::of(commands, Primary::Kind(ElementKind::Wall)))
}

/// The parts of segment a-b that no building wall on the level already
/// covers, so a room drawn against a wall shares it instead of doubling it.
fn uncovered(project: &Project, level_id: &str, a: Point, b: Point) -> Vec<(Point, Point)> {
    let len = geo::dist(a, b);
    if len < 1.0 {
        return vec![];
    }
    let mut covered: Vec<(f64, f64)> = vec![];
    for el in &project.elements {
        let Element::Wall(w) = el else { continue };
        if w.level_id != level_id || w.is_site() {
            continue;
        }
        let (Some((t0, o0)), Some((t1, o1))) = (geo::project(a, b, w.start), geo::project(a, b, w.end)) else {
            continue;
        };
        if o0.abs() > 1.0 || o1.abs() > 1.0 {
            continue;
        }
        let (lo, hi) = (t0.min(t1).max(0.0), t0.max(t1).min(len));
        if hi > lo {
            covered.push((lo, hi));
        }
    }
    covered.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut out = vec![];
    let mut at = 0.0;
    let point_at = |t: f64| pt(a.x + (b.x - a.x) * t / len, a.y + (b.y - a.y) * t / len);
    for (lo, hi) in covered {
        if lo > at + 1.0 {
            out.push((point_at(at), point_at(lo)));
        }
        at = at.max(hi);
    }
    if len > at + 1.0 {
        out.push((point_at(at), b));
    }
    // Slivers shorter than a wall can be would be refused; a 1 mm gap joins.
    out.into_iter().filter(|(p, q)| geo::dist(*p, *q) >= 50.0).collect()
}

// -------------------------------------------------------------------- rooms

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RectIn {
    x: f64,
    y: f64,
    width_mm: f64,
    depth_mm: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddRoomIn {
    rect: Option<RectIn>,
    origin: Option<Value>,
    width_mm: Option<f64>,
    depth_mm: Option<f64>,
    polygon: Option<Vec<Value>>,
    name: Option<String>,
    usage: Option<RoomUsage>,
    floor_material_id: Option<String>,
    level: Option<String>,
    walls: Option<bool>,
    thickness_mm: Option<f64>,
    height_mm: Option<f64>,
}

fn rect_corners(x: f64, y: f64, w: f64, d: f64) -> Result<Vec<Point>, ToolFail> {
    for (name, v) in [("x", x), ("y", y)] {
        finite(v, name)?;
    }
    for (name, v) in [("width_mm", w), ("depth_mm", d)] {
        if !v.is_finite() || v < 300.0 {
            return Err(ToolFail::args(format!("{name} must be at least 300 mm, got {v}")));
        }
    }
    Ok(vec![pt(x, y), pt(x + w, y), pt(x + w, y + d), pt(x, y + d)])
}

fn add_room(p: &Project, args: &Value, default_level: &Id) -> Result<Plan, ToolFail> {
    let i: AddRoomIn = parse(args)?;
    let outline = match (&i.rect, &i.origin, &i.polygon) {
        (Some(r), None, None) => rect_corners(r.x, r.y, r.width_mm, r.depth_mm)?,
        (None, Some(o), None) => {
            let o = point(o, "origin")?;
            let (Some(w), Some(d)) = (i.width_mm, i.depth_mm) else {
                return Err(ToolFail::args("origin needs width_mm and depth_mm"));
            };
            rect_corners(o.x, o.y, w, d)?
        }
        (None, None, Some(list)) => {
            let poly = points(list, "polygon")?;
            if poly.len() < 3 || geo::signed_area(&poly).abs() < 90_000.0 {
                return Err(ToolFail::args("polygon needs at least 3 points around an area of 0.09 m2 or more"));
            }
            poly
        }
        _ => return Err(ToolFail::args("give one of rect {x, y, width_mm, depth_mm}, origin with width_mm and depth_mm, or polygon")),
    };
    let level_id = pick_level(p, i.level.as_deref(), default_level)?;
    if let Some(m) = &i.floor_material_id {
        check_material(p, m)?;
    }
    let thickness_mm = positive_opt(i.thickness_mm, "thickness_mm")?;
    let height_mm = positive_opt(i.height_mm, "height_mm")?;
    let name = i.name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    let mut commands = vec![];
    if i.walls.unwrap_or(true) {
        let n = outline.len();
        for k in 0..n {
            for (a, b) in uncovered(p, &level_id, outline[k], outline[(k + 1) % n]) {
                commands.push(Command::AddWall {
                    start: a,
                    end: b,
                    thickness_mm,
                    height_mm,
                    material_id: None,
                    level_id: Some(level_id.clone()),
                    role: None,
                });
            }
        }
    }
    let point = geo::interior_point(&outline).ok_or_else(|| ToolFail::args("the room outline has no inside"))?;
    Ok(Plan {
        commands,
        then: vec![Then::Room { level_id, point, name, usage: i.usage, floor_material_id: i.floor_material_id }],
        primary: Primary::Kind(ElementKind::Room),
        info: None,
    })
}

/// The room on `level_id` whose outline holds `point`, smallest first.
pub(crate) fn room_at(project: &Project, derived: &Derived, level_id: &str, point: Point) -> Option<Room> {
    derived
        .rooms
        .iter()
        .filter(|g| geo::contains(&g.centerline_polygon, point) || geo::contains(&g.polygon, point))
        .filter_map(|g| {
            project.elements.iter().find_map(|e| match e {
                Element::Room(r) if r.id == g.room_id && r.level_id == level_id => Some((r.clone(), g.area_mm2)),
                _ => None,
            })
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(r, _)| r)
}

/// The commands of a `Then`, against the project the step's commands made,
/// and the id it is about.
pub(crate) fn then_commands(view: &View, then: &Then, added: &[Id]) -> Result<(Vec<Command>, Option<Id>), ToolFail> {
    match then {
        Then::Room { level_id, point, name, usage, floor_material_id } => {
            let Some(mut room) = room_at(view.project, view.derived(), level_id, *point) else {
                return Err(ToolFail::new(
                    "no_room_here",
                    format!(
                        "the walls do not close a room around ({:.0}, {:.0}) on this level, so there is no room to name",
                        point.x, point.y
                    ),
                ));
            };
            let id = room.id.clone();
            let before = room.clone();
            if let Some(n) = name {
                room.name = n.clone();
                room.auto_named = false;
            }
            if let Some(u) = usage {
                room.usage = *u;
            }
            if let Some(m) = floor_material_id {
                room.floor_material_id = Some(m.clone());
            }
            let commands = if room == before { vec![] } else { vec![Command::UpdateElement { element: Element::Room(room) }] };
            Ok((commands, Some(id)))
        }
        Then::Material { material_id } => {
            if added.is_empty() {
                return Ok((vec![], None));
            }
            Ok((vec![Command::SetMaterial { ids: added.to_vec(), material_id: material_id.clone() }], None))
        }
    }
}

// ----------------------------------------------------------------- openings

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddOpeningIn {
    kind: OpeningType,
    at: Option<Value>,
    wall_id: Option<Id>,
    offset_mm: Option<f64>,
    width_mm: Option<f64>,
    height_mm: Option<f64>,
    sill_mm: Option<f64>,
    style: Option<OpeningStyle>,
    swing_into: Option<Id>,
    flip_side: Option<bool>,
    flip_hinge: Option<bool>,
    material_id: Option<String>,
    level: Option<String>,
}

/// How far from a wall's centerline, beyond half its thickness, a point
/// still finds it.
const WALL_FIND_MM: f64 = 300.0;

/// The wall a point is on: the nearest wall of the level whose centerline
/// the point projects onto, within half its thickness plus 300 mm. Building
/// walls win over site walls. Returns the wall and the offset along it.
pub(crate) fn wall_at<'a>(project: &'a Project, level_id: &str, at: Point) -> Result<(&'a Wall, f64), ToolFail> {
    let mut best: Option<(bool, f64, &Wall, f64)> = None;
    let mut nearest: Option<(f64, &Wall)> = None;
    for el in &project.elements {
        let Element::Wall(w) = el else { continue };
        if w.level_id != level_id {
            continue;
        }
        let Some((along, off)) = geo::project(w.start, w.end, at) else { continue };
        let len = geo::dist(w.start, w.end);
        let d = geo::dist_point_segment(at, w.start, w.end);
        if nearest.is_none_or(|(n, _)| d < n) {
            nearest = Some((d, w));
        }
        if !(0.0..=len).contains(&along) || off.abs() > w.thickness_mm / 2.0 + WALL_FIND_MM {
            continue;
        }
        let key = (w.is_site(), off.abs());
        if best.is_none_or(|(s, o, _, _)| (key.0, key.1) < (s, o)) {
            best = Some((key.0, key.1, w, along));
        }
    }
    match (best, nearest) {
        (Some((_, _, w, along)), _) => Ok((w, along)),
        (None, Some((d, w))) => Err(ToolFail::new(
            "not_found",
            format!("no wall on this level runs through ({:.0}, {:.0}); the nearest is {} at {:.0} mm", at.x, at.y, w.id, d),
        )
        .with_ids(vec![w.id.clone()])
        .with_hint("Give a point on the wall's centerline (look shows them), or wall_id with offset_mm.")),
        (None, None) => Err(ToolFail::new("not_found", "this level has no walls to put an opening on")
            .with_hint("Draw walls first with add_walls or add_room.")),
    }
}

fn add_opening(view: &View, args: &Value, default_level: &Id) -> Result<Plan, ToolFail> {
    let p = view.project;
    let i: AddOpeningIn = parse(args)?;
    let (wall, offset_mm) = match (&i.at, &i.wall_id) {
        (Some(at), None) => {
            if i.offset_mm.is_some() {
                return Err(ToolFail::args("give at, or wall_id with offset_mm, not both"));
            }
            let level_id = pick_level(p, i.level.as_deref(), default_level)?;
            wall_at(p, &level_id, point(at, "at")?)?
        }
        (None, Some(id)) => {
            let wall = match find(p, id)? {
                Element::Wall(w) => w,
                other => return Err(ToolFail::new("not_a_wall", format!("`{id}` is a {:?}, not a wall", other.kind()))),
            };
            let len = geo::dist(wall.start, wall.end);
            (wall, i.offset_mm.map(|o| finite(o, "offset_mm")).transpose()?.unwrap_or(len / 2.0))
        }
        _ => return Err(ToolFail::args("give at {x, y} on a wall, or wall_id (and offset_mm from its start)")),
    };
    if i.kind == OpeningType::Door && i.sill_mm.is_some() {
        return Err(ToolFail::args("a door has no sill_mm"));
    }
    if let Some(m) = &i.material_id {
        check_material(p, m)?;
    }
    let mut flip_side = i.flip_side;
    if let Some(room_id) = &i.swing_into {
        if flip_side.is_some() {
            return Err(ToolFail::args("give swing_into or flip_side, not both"));
        }
        flip_side = Some(swing_side(view, wall, offset_mm, room_id)?);
    }
    let mut plan = Plan::of(
        vec![Command::AddOpening {
            wall_id: wall.id.clone(),
            opening_type: i.kind,
            offset_mm,
            width_mm: positive_opt(i.width_mm, "width_mm")?,
            height_mm: positive_opt(i.height_mm, "height_mm")?,
            sill_mm: i.sill_mm.map(|v| finite(v, "sill_mm")).transpose()?,
            style: i.style,
            flip_side,
            flip_hinge: i.flip_hinge,
        }],
        Primary::Kind(ElementKind::Opening),
    );
    if let Some(m) = i.material_id {
        plan.then.push(Then::Material { material_id: m });
    }
    plan.info = Some(json!({"wall_id": wall.id, "offset_mm": geo::r1(offset_mm)}));
    Ok(plan)
}

/// `flip_side` for a leaf that swings into `room_id`: false when the room is
/// on the left of the wall direction (start to end), true on the right.
fn swing_side(view: &View, wall: &Wall, offset_mm: f64, room_id: &str) -> Result<bool, ToolFail> {
    let derived = view.derived();
    let Some(g) = derived.rooms.iter().find(|g| g.room_id == room_id) else {
        find(view.project, room_id)?;
        return Err(ToolFail::new("not_found", format!("`{room_id}` is not a room with closed walls")));
    };
    let len = geo::dist(wall.start, wall.end);
    let (ux, uy) = ((wall.end.x - wall.start.x) / len, (wall.end.y - wall.start.y) / len);
    let c = pt(wall.start.x + ux * offset_mm, wall.start.y + uy * offset_mm);
    let step = wall.thickness_mm / 2.0 + 150.0;
    let left = pt(c.x - uy * step, c.y + ux * step);
    let right = pt(c.x + uy * step, c.y - ux * step);
    let inside = |q: Point| geo::contains(&g.polygon, q) || geo::contains(&g.centerline_polygon, q);
    if inside(left) {
        Ok(false)
    } else if inside(right) {
        Ok(true)
    } else {
        Err(ToolFail::new("bad_args", format!("room `{room_id}` is on neither side of wall {} at this opening", wall.id))
            .with_ids(vec![room_id.to_string(), wall.id.clone()]))
    }
}

// ------------------------------------------------------------------- stairs

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddStairIn {
    level: Option<String>,
    to_level: Option<String>,
    start: Value,
    direction_deg: Option<f64>,
    width_mm: Option<f64>,
    run_mm: Option<f64>,
    riser_count: Option<u32>,
    tread_mm: Option<f64>,
    shape: Option<String>,
    railing: Option<Value>,
}

/// Riser height a new stair aims at when the call gives no riser count.
const RISER_TARGET_MM: f64 = 175.0;
/// Tread depth a new stair gets when the call gives neither run nor tread.
const TREAD_DEFAULT_MM: f64 = 250.0;

fn add_stair(p: &Project, args: &Value, default_level: &Id) -> Result<Plan, ToolFail> {
    let i: AddStairIn = parse(args)?;
    match i.shape.as_deref().map(str::trim) {
        None | Some("straight") => {}
        Some(other) => {
            return Err(ToolFail::new(
                "unavailable",
                format!("stairs are straight flights in this version, not `{other}`"),
            )
            .with_hint("Build an L or U stair as two straight flights with a landing slab between them (add_slab slab_kind \"floor\")."))
        }
    }
    let level_id = pick_level(p, i.level.as_deref(), default_level)?;
    let level = p.levels.iter().find(|l| l.id == level_id).ok_or_else(|| ToolFail::new("unknown_level", "the level is gone"))?;
    let rise = level.height_mm;
    if let Some(to) = i.to_level.as_deref() {
        let upper = find_level(p, to)?;
        if (upper.elevation_mm - (level.elevation_mm + rise)).abs() > 1.0 {
            return Err(ToolFail::new(
                "bad_level",
                format!(
                    "a stair on {} rises its floor-to-floor height, {rise:.0} mm, to {:.0}; {} is at {:.0}",
                    level.name,
                    level.elevation_mm + rise,
                    upper.name,
                    upper.elevation_mm
                ),
            )
            .with_hint("Set the lower level's height_mm to the gap with levels action \"update\", or put the stair on the level just below to_level."));
        }
    }
    let riser_count = match i.riser_count {
        Some(n) => n,
        None => (rise / RISER_TARGET_MM).ceil().max(2.0) as u32,
    };
    if riser_count < 2 {
        return Err(ToolFail::new("stair_size", "a stair needs at least 2 risers"));
    }
    let treads = (riser_count - 1) as f64;
    let run_mm = match (i.run_mm, i.tread_mm) {
        (Some(_), Some(_)) => return Err(ToolFail::args("give run_mm or tread_mm, not both")),
        (Some(r), None) => positive(r, "run_mm")?,
        (None, Some(t)) => positive(t, "tread_mm")? * treads,
        (None, None) => TREAD_DEFAULT_MM * treads,
    };
    let direction = finite(i.direction_deg.unwrap_or(90.0), "direction_deg")?;
    let width_mm = positive_opt(i.width_mm, "width_mm")?.unwrap_or(1000.0);
    let stair = Stair {
        id: String::new(),
        level_id,
        origin: point(&i.start, "start")?,
        rotation_deg: (direction - 90.0).rem_euclid(360.0),
        width_mm,
        run_mm,
        riser_count,
        railing: railing(i.railing.as_ref(), 900.0, Some(Railing { height_mm: 900.0, style: RailingStyle::Bars }))?,
    };
    let top = geo::stair_outline(&stair);
    let mut plan = Plan::of(vec![Command::AddElement { element: Element::Stair(stair) }], Primary::Kind(ElementKind::Stair));
    plan.info = Some(json!({
        "rise_mm": geo::r1(rise),
        "riser_mm": geo::r1(rise / riser_count as f64),
        "tread_mm": geo::r1(run_mm / treads),
        "treads": riser_count - 1,
        "top_landing_edge": [top[3], top[2]],
    }));
    Ok(plan)
}

// ------------------------------------------------------------------ columns

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddColumnIn {
    at: Option<Value>,
    points: Option<Vec<Value>>,
    shape: Option<ColumnShape>,
    width_mm: Option<f64>,
    depth_mm: Option<f64>,
    rotation_deg: Option<f64>,
    level: Option<String>,
    to_level: Option<String>,
    material_id: Option<String>,
}

fn add_column(p: &Project, args: &Value, default_level: &Id) -> Result<Plan, ToolFail> {
    let i: AddColumnIn = parse(args)?;
    let at = match (&i.at, &i.points) {
        (Some(a), None) => vec![point(a, "at")?],
        (None, Some(list)) if !list.is_empty() => points(list, "points")?,
        _ => return Err(ToolFail::args("give at {x, y}, or points for several columns")),
    };
    let from = pick_level(p, i.level.as_deref(), default_level)?;
    let levels = match i.to_level.as_deref() {
        None => vec![from],
        Some(to) => {
            let to = find_level(p, to)?;
            let lo = p.levels.iter().find(|l| l.id == from).map(|l| l.elevation_mm).unwrap_or(0.0);
            if to.elevation_mm < lo {
                return Err(ToolFail::args("to_level is below level"));
            }
            let mut on: Vec<&Level> = p.levels.iter().filter(|l| l.elevation_mm >= lo - 1.0 && l.elevation_mm <= to.elevation_mm + 1.0).collect();
            on.sort_by(|a, b| a.elevation_mm.total_cmp(&b.elevation_mm));
            on.into_iter().map(|l| l.id.clone()).collect()
        }
    };
    if let Some(m) = &i.material_id {
        check_material(p, m)?;
    }
    let width = positive_opt(i.width_mm, "width_mm")?.unwrap_or(300.0);
    let depth = positive_opt(i.depth_mm, "depth_mm")?.unwrap_or(width);
    let rotation = finite(i.rotation_deg.unwrap_or(0.0), "rotation_deg")?;
    let mut commands = vec![];
    for level_id in &levels {
        for c in &at {
            commands.push(Command::AddElement {
                element: Element::Column(Column {
                    id: String::new(),
                    level_id: level_id.clone(),
                    center: *c,
                    shape: i.shape.unwrap_or(ColumnShape::Rect),
                    width_mm: width,
                    depth_mm: depth,
                    rotation_deg: rotation,
                    material_id: i.material_id.clone(),
                }),
            });
        }
    }
    Ok(Plan::of(commands, Primary::Kind(ElementKind::Column)))
}

// ------------------------------------------------------------------- slabs

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddSlabIn {
    level: Option<String>,
    /// `slab_kind`, as a slab spells it in JSON; `kind` is taken too.
    #[serde(alias = "kind")]
    slab_kind: Option<SlabKind>,
    polygon: Option<Vec<Value>>,
    rect: Option<RectIn>,
    thickness_mm: Option<f64>,
    top_offset_mm: Option<f64>,
    material_id: Option<String>,
    railing: Option<Value>,
}

fn add_slab(p: &Project, args: &Value, default_level: &Id) -> Result<Plan, ToolFail> {
    let i: AddSlabIn = parse(args)?;
    let outline = match (&i.polygon, &i.rect) {
        (Some(list), None) => points(list, "polygon")?,
        (None, Some(r)) => {
            for (name, v) in [("rect.width_mm", r.width_mm), ("rect.depth_mm", r.depth_mm)] {
                positive(v, name)?;
            }
            vec![pt(r.x, r.y), pt(r.x + r.width_mm, r.y), pt(r.x + r.width_mm, r.y + r.depth_mm), pt(r.x, r.y + r.depth_mm)]
        }
        _ => return Err(ToolFail::args("give polygon (3 or more points) or rect {x, y, width_mm, depth_mm}")),
    };
    if outline.len() < 3 || geo::signed_area(&outline).abs() < 1.0 {
        return Err(ToolFail::new("slab_outline", "a slab needs an outline of at least three points around an area"));
    }
    let kind = i.slab_kind.unwrap_or(SlabKind::Floor);
    let level_id = pick_level(p, i.level.as_deref(), default_level)?;
    if let Some(m) = &i.material_id {
        check_material(p, m)?;
    }
    let thickness = match kind {
        SlabKind::Void => 0.0,
        SlabKind::Floor => positive_opt(i.thickness_mm, "thickness_mm")?.unwrap_or(200.0),
        SlabKind::Balcony | SlabKind::Deck => positive_opt(i.thickness_mm, "thickness_mm")?.unwrap_or(150.0),
    };
    let default_rail = (kind == SlabKind::Balcony).then_some(Railing { height_mm: 1000.0, style: RailingStyle::Bars });
    let slab = Slab {
        id: String::new(),
        level_id,
        kind,
        polygon: geo::ccw(outline),
        thickness_mm: thickness,
        top_offset_mm: finite(i.top_offset_mm.unwrap_or(0.0), "top_offset_mm")?,
        material_id: i.material_id,
        railing: railing(i.railing.as_ref(), 1000.0, default_rail)?,
    };
    let area = geo::signed_area(&slab.polygon) / 1.0e6;
    let mut plan = Plan::of(vec![Command::AddElement { element: Element::Slab(slab) }], Primary::Kind(ElementKind::Slab));
    plan.info = Some(json!({"outline_area_m2": (area * 100.0).round() / 100.0}));
    Ok(plan)
}

// ------------------------------------------------------------------ objects

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlaceAssetIn {
    catalog_key: String,
    at: Value,
    rotation_deg: Option<f64>,
    level: Option<String>,
    snap: Option<bool>,
}

/// Catalog keys like `wanted`, by key and by name.
pub(crate) fn catalog_suggestions(wanted: &str, n: usize) -> Vec<String> {
    let catalog = defaults::asset_catalog();
    let w = wanted.to_ascii_lowercase().replace(['_', ' '], "-");
    let mut scored: Vec<(usize, String)> = catalog
        .iter()
        .map(|c| {
            let key = c.key.to_ascii_lowercase();
            let name = c.name.to_ascii_lowercase();
            let d = crate::toolsets::distance(&w, &key).min(crate::toolsets::distance(&w, &name.replace(' ', "-")));
            let hit = key.contains(&w) || name.contains(&wanted.to_ascii_lowercase()) || w.split('-').any(|p| p.len() > 2 && (key.contains(p) || name.contains(p)));
            (if hit { d.min(2) } else { d + 2 }, format!("{} ({})", c.key, c.name))
        })
        .collect();
    scored.sort();
    scored.into_iter().take(n).map(|(_, s)| s).collect()
}

fn place_asset(p: &Project, args: &Value, default_level: &Id) -> Result<Plan, ToolFail> {
    let i: PlaceAssetIn = parse(args)?;
    let key = i.catalog_key.trim();
    if !defaults::asset_catalog().iter().any(|c| c.key == key) {
        return Err(ToolFail::new(
            "unknown_catalog_key",
            format!("no catalog item has key `{key}`. Did you mean: {}?", catalog_suggestions(key, 5).join(", ")),
        ));
    }
    let at = point(&i.at, "at")?;
    let mut v1 = json!({"catalog_key": key, "position": {"x": at.x, "y": at.y}});
    if let Some(r) = i.rotation_deg {
        v1["rotation_deg"] = json!(finite(r, "rotation_deg")?);
    }
    if let Some(l) = &i.level {
        v1["level"] = json!(l);
    }
    let mut commands = copilot::to_commands(p, "add_asset", &v1, default_level)?;
    // snap false: exactly where and how the call says.
    if i.snap == Some(false) {
        for c in &mut commands {
            if let Command::AddElement { element: Element::Asset(a) } = c {
                a.position = at;
                a.rotation_deg = i.rotation_deg.unwrap_or(0.0);
            }
        }
    }
    Ok(Plan::of(commands, Primary::Kind(ElementKind::Asset)))
}

// ------------------------------------------------------------------ cameras

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CameraIn {
    name: String,
    position: Value,
    target: Value,
    fov_deg: Option<f64>,
    level: Option<String>,
}

/// A saved view. Heights are above the floor of `level` (the first level).
fn add_camera(p: &Project, args: &Value) -> Result<Plan, ToolFail> {
    let i: CameraIn = parse(args)?;
    let name = i.name.trim();
    if name.is_empty() {
        return Err(ToolFail::args("name is empty"));
    }
    let (pos, target) = (point3(&i.position, "position")?, point3(&i.target, "target")?);
    let fov = i.fov_deg.unwrap_or(60.0);
    if !(20.0..=100.0).contains(&fov) {
        return Err(ToolFail::args(format!("fov_deg must be 20 to 100, got {fov}")));
    }
    let d = ((target.x - pos.x).powi(2) + (target.y - pos.y).powi(2) + (target.z - pos.z).powi(2)).sqrt();
    if d < 100.0 {
        return Err(ToolFail::args("target must be at least 100 mm from position"));
    }
    let floor = match i.level.as_deref() {
        Some(l) => find_level(p, l)?.elevation_mm,
        None => p.levels.first().map(|l| l.elevation_mm).unwrap_or(0.0),
    };
    let lift = |v: Vec3| Vec3 { x: v.x, y: v.y, z: v.z + floor };
    Ok(Plan::of(
        vec![Command::AddElement {
            element: Element::Camera(Camera {
                id: String::new(),
                name: name.to_string(),
                preset: CameraPreset::Custom,
                position: lift(pos),
                target: lift(target),
                fov_deg: fov,
                light: None,
            }),
        }],
        Primary::Kind(ElementKind::Camera),
    ))
}

// ------------------------------------------------------------------- modify

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModifyIn {
    ids: Vec<Id>,
    #[serde(rename = "move")]
    by: Option<MoveIn>,
    stretch_connected: Option<bool>,
    rotate_deg: Option<f64>,
    pivot: Option<Value>,
    set: Option<serde_json::Map<String, Value>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveIn {
    dx_mm: Option<f64>,
    dy_mm: Option<f64>,
}

/// The `set` fields each kind takes.
fn settable(kind: ElementKind) -> &'static [&'static str] {
    match kind {
        ElementKind::Room => &["name", "usage", "floor_material_id", "material_id"],
        ElementKind::Opening => &["width_mm", "height_mm", "sill_mm", "style", "flip_side", "flip_hinge", "material_id", "offset_mm", "swing_into"],
        ElementKind::Wall => &["thickness_mm", "height_mm", "material_id", "role"],
        ElementKind::Column => &["width_mm", "depth_mm", "shape", "rotation_deg", "material_id"],
        ElementKind::Stair => &["width_mm", "run_mm", "riser_count", "railing", "direction_deg"],
        ElementKind::Slab => &["slab_kind", "kind", "thickness_mm", "top_offset_mm", "material_id", "railing"],
        ElementKind::Asset => &["name", "circuit", "elevation_mm", "rotation_deg"],
        ElementKind::Camera => &["name", "fov_deg"],
        ElementKind::Annotation => &["text"],
        ElementKind::Pipe => &["name", "diameter_mm", "material"],
        _ => &[],
    }
}

fn num(v: &Value, what: &str) -> Result<f64, ToolFail> {
    v.as_f64().filter(|n| n.is_finite()).ok_or_else(|| ToolFail::args(format!("set.{what} must be a number")))
}

fn text(v: &Value, what: &str) -> Result<String, ToolFail> {
    v.as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).ok_or_else(|| ToolFail::args(format!("set.{what} must be a non-empty string")))
}

fn enum_of<T: DeserializeOwned>(v: &Value, what: &str) -> Result<T, ToolFail> {
    serde_json::from_value(v.clone()).map_err(|e| ToolFail::args(format!("set.{what}: {e}")))
}

/// One element with the `set` fields that apply to its kind.
fn apply_set(view: &View, el: &Element, set: &serde_json::Map<String, Value>) -> Result<Element, ToolFail> {
    let p = view.project;
    let mut el = el.clone();
    let allowed = settable(el.kind());
    for (key, v) in set.iter().filter(|(k, _)| allowed.contains(&k.as_str())) {
        if key.ends_with("material_id") {
            check_material(p, &text(v, key)?)?;
        }
        match (&mut el, key.as_str()) {
            (Element::Room(r), "name") => {
                r.name = text(v, key)?;
                r.auto_named = false;
            }
            (Element::Room(r), "usage") => r.usage = enum_of(v, key)?,
            (Element::Room(r), "floor_material_id" | "material_id") => r.floor_material_id = Some(text(v, key)?),
            (Element::Opening(o), "width_mm") => o.width_mm = positive(num(v, key)?, "set.width_mm")?,
            (Element::Opening(o), "height_mm") => o.height_mm = positive(num(v, key)?, "set.height_mm")?,
            (Element::Opening(o), "sill_mm") => o.sill_mm = num(v, key)?,
            (Element::Opening(o), "offset_mm") => o.offset_mm = num(v, key)?,
            (Element::Opening(o), "style") => o.style = enum_of(v, key)?,
            (Element::Opening(o), "flip_side") => o.flip_side = enum_of(v, key)?,
            (Element::Opening(o), "flip_hinge") => o.flip_hinge = enum_of(v, key)?,
            (Element::Opening(o), "material_id") => o.material_id = Some(text(v, key)?),
            (Element::Opening(o), "swing_into") => {
                let wall = match find(p, &o.wall_id)? {
                    Element::Wall(w) => w.clone(),
                    _ => return Err(ToolFail::new("not_a_wall", "the opening's host is not a wall")),
                };
                o.flip_side = swing_side(view, &wall, o.offset_mm, &text(v, key)?)?;
            }
            (Element::Wall(w), "thickness_mm") => w.thickness_mm = positive(num(v, key)?, "set.thickness_mm")?,
            (Element::Wall(w), "height_mm") => {
                w.height_mm = if v.is_null() { None } else { Some(positive(num(v, key)?, "set.height_mm")?) }
            }
            (Element::Wall(w), "material_id") => w.material_id = Some(text(v, key)?),
            (Element::Wall(w), "role") => w.role = enum_of(v, key)?,
            (Element::Column(c), "width_mm") => c.width_mm = positive(num(v, key)?, "set.width_mm")?,
            (Element::Column(c), "depth_mm") => c.depth_mm = positive(num(v, key)?, "set.depth_mm")?,
            (Element::Column(c), "shape") => c.shape = enum_of(v, key)?,
            (Element::Column(c), "rotation_deg") => c.rotation_deg = num(v, key)?,
            (Element::Column(c), "material_id") => c.material_id = Some(text(v, key)?),
            (Element::Stair(s), "width_mm") => s.width_mm = positive(num(v, key)?, "set.width_mm")?,
            (Element::Stair(s), "run_mm") => s.run_mm = positive(num(v, key)?, "set.run_mm")?,
            (Element::Stair(s), "riser_count") => s.riser_count = enum_of(v, key)?,
            (Element::Stair(s), "railing") => s.railing = railing(Some(v), 900.0, s.railing)?,
            (Element::Stair(s), "direction_deg") => s.rotation_deg = (num(v, key)? - 90.0).rem_euclid(360.0),
            (Element::Slab(s), "slab_kind" | "kind") => s.kind = enum_of(v, key)?,
            (Element::Slab(s), "thickness_mm") => s.thickness_mm = positive(num(v, key)?, "set.thickness_mm")?,
            (Element::Slab(s), "top_offset_mm") => s.top_offset_mm = num(v, key)?,
            (Element::Slab(s), "material_id") => s.material_id = Some(text(v, key)?),
            (Element::Slab(s), "railing") => s.railing = railing(Some(v), 1000.0, s.railing)?,
            (Element::Asset(a), "name") => a.name = text(v, key)?,
            (Element::Asset(a), "circuit") => a.circuit = v.as_str().unwrap_or("").trim().to_string(),
            (Element::Asset(a), "elevation_mm") => a.elevation_mm = num(v, key)?,
            (Element::Asset(a), "rotation_deg") => a.rotation_deg = num(v, key)?,
            (Element::Camera(c), "name") => c.name = text(v, key)?,
            (Element::Camera(c), "fov_deg") => c.fov_deg = num(v, key)?,
            (Element::Annotation(a), "text") => a.text = text(v, key)?,
            (Element::Pipe(pp), "name") => pp.name = v.as_str().unwrap_or("").trim().to_string(),
            (Element::Pipe(pp), "diameter_mm") => pp.diameter_mm = positive(num(v, key)?, "set.diameter_mm")?,
            (Element::Pipe(pp), "material") => pp.material = enum_of(v, key)?,
            _ => {}
        }
    }
    Ok(el)
}

/// A point that stands for an element, for a default pivot.
fn anchor(project: &Project, el: &Element) -> Option<Point> {
    Some(match el {
        Element::Wall(w) => pt((w.start.x + w.end.x) / 2.0, (w.start.y + w.end.y) / 2.0),
        Element::Column(c) => c.center,
        Element::Stair(s) => s.origin,
        Element::Asset(a) => a.position,
        Element::Room(r) => r.seed,
        Element::Annotation(a) => a.position,
        Element::Slab(s) => geo::interior_point(&s.polygon)?,
        Element::Opening(o) => match project.elements.iter().find(|e| e.id() == &o.wall_id) {
            Some(Element::Wall(w)) => geo::opening_center(w, o)?,
            _ => return None,
        },
        Element::Camera(c) => pt(c.position.x, c.position.y),
        Element::Pipe(pp) => pt(pp.points.first()?.x, pp.points.first()?.y),
        _ => return None,
    })
}

fn modify(view: &View, args: &Value) -> Result<Plan, ToolFail> {
    let p = view.project;
    let i: ModifyIn = parse(args)?;
    ids_exist(p, &i.ids)?;
    if i.by.is_none() && i.rotate_deg.is_none() && i.set.as_ref().is_none_or(|s| s.is_empty()) {
        return Err(ToolFail::args("say what to change: move {dx_mm, dy_mm}, rotate_deg or set {...}"));
    }
    let mut commands = vec![];
    if let Some(set) = &i.set {
        let kinds: Vec<ElementKind> = i.ids.iter().filter_map(|id| p.elements.iter().find(|e| e.id() == id)).map(|e| e.kind()).collect();
        for key in set.keys() {
            if !kinds.iter().any(|k| settable(*k).contains(&key.as_str())) {
                let allowed: Vec<String> = kinds
                    .iter()
                    .map(|k| format!("{} takes {}", serde_json::to_value(k).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default(), settable(*k).join(", ")))
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect();
                return Err(ToolFail::args(format!("set.{key} does not apply to these elements: {}", allowed.join("; "))));
            }
        }
        for id in &i.ids {
            let el = find(p, id)?;
            let changed = apply_set(view, el, set)?;
            if &changed != el {
                commands.push(Command::UpdateElement { element: changed });
            }
        }
    }
    if let Some(m) = &i.by {
        let delta = pt(finite(m.dx_mm.unwrap_or(0.0), "move.dx_mm")?, finite(m.dy_mm.unwrap_or(0.0), "move.dy_mm")?);
        if delta.x != 0.0 || delta.y != 0.0 {
            commands.push(Command::MoveElements { ids: i.ids.clone(), delta, stretch_connected: i.stretch_connected.unwrap_or(true) });
        }
    }
    if let Some(angle) = i.rotate_deg {
        let angle = finite(angle, "rotate_deg")?;
        let pivot = match &i.pivot {
            Some(v) => point(v, "pivot")?,
            None => {
                let pts: Vec<Point> = i.ids.iter().filter_map(|id| p.elements.iter().find(|e| e.id() == id)).filter_map(|e| anchor(p, e)).collect();
                if pts.is_empty() {
                    return Err(ToolFail::args("give pivot {x, y}: these elements have no point to turn about"));
                }
                let (lo, hi) = pts.iter().fold((pts[0], pts[0]), |(lo, hi), q| (pt(lo.x.min(q.x), lo.y.min(q.y)), pt(hi.x.max(q.x), hi.y.max(q.y))));
                pt((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0)
            }
        };
        if angle != 0.0 {
            commands.push(Command::RotateElements { ids: i.ids.clone(), pivot, angle_deg: angle });
        }
    }
    if commands.is_empty() {
        return Err(ToolFail::args("nothing would change: the values are what the elements already have"));
    }
    Ok(Plan::of(commands, Primary::None))
}

// ------------------------------------------------------------------ reshape

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReshapeIn {
    op: String,
    room_id: Option<Id>,
    side: Option<Side>,
    delta_mm: Option<f64>,
    wall_id: Option<Id>,
    length_mm: Option<f64>,
    anchor: Option<WallAnchor>,
    start: Option<Value>,
    end: Option<Value>,
    at_mm: Option<f64>,
    at: Option<Value>,
}

fn need<T: Clone>(v: &Option<T>, op: &str, name: &str) -> Result<T, ToolFail> {
    v.clone().ok_or_else(|| ToolFail::args(format!("op \"{op}\" needs {name}")))
}

fn reshape(p: &Project, args: &Value) -> Result<Plan, ToolFail> {
    let i: ReshapeIn = parse(args)?;
    let op = i.op.as_str();
    let wall = |id: &str| -> Result<Wall, ToolFail> {
        match find(p, id)? {
            Element::Wall(w) => Ok(w.clone()),
            other => Err(ToolFail::new("not_a_wall", format!("`{id}` is a {:?}, not a wall", other.kind()))),
        }
    };
    let (command, primary) = match op {
        "resize_room" => {
            let room_id = need(&i.room_id, op, "room_id")?;
            match find(p, &room_id)? {
                Element::Room(_) => {}
                other => return Err(ToolFail::args(format!("`{room_id}` is a {:?}, not a room", other.kind()))),
            }
            let delta_mm = finite(need(&i.delta_mm, op, "delta_mm")?, "delta_mm")?;
            if delta_mm == 0.0 {
                return Err(ToolFail::args("delta_mm is 0, nothing would change"));
            }
            (Command::ResizeRoom { room_id, side: need(&i.side, op, "side")?, delta_mm }, Primary::None)
        }
        "set_wall_length" => {
            let wall_id = need(&i.wall_id, op, "wall_id")?;
            wall(&wall_id)?;
            (
                Command::SetWallLength {
                    wall_id,
                    length_mm: positive(need(&i.length_mm, op, "length_mm")?, "length_mm")?,
                    anchor: i.anchor.unwrap_or(WallAnchor::Start),
                },
                Primary::None,
            )
        }
        "set_wall_endpoints" => {
            let wall_id = need(&i.wall_id, op, "wall_id")?;
            wall(&wall_id)?;
            (
                Command::SetWallEndpoints {
                    wall_id,
                    start: point(&need(&i.start, op, "start")?, "start")?,
                    end: point(&need(&i.end, op, "end")?, "end")?,
                },
                Primary::None,
            )
        }
        "split_wall" => {
            let wall_id = need(&i.wall_id, op, "wall_id")?;
            let w = wall(&wall_id)?;
            let at_mm = match (i.at_mm, &i.at) {
                (Some(a), None) => finite(a, "at_mm")?,
                (None, Some(v)) => {
                    let q = point(v, "at")?;
                    geo::project(w.start, w.end, q).map(|(along, _)| along).ok_or_else(|| ToolFail::args("the wall has no length"))?
                }
                _ => return Err(ToolFail::args("op \"split_wall\" needs at_mm (from the wall start) or at {x, y}")),
            };
            (Command::SplitWall { wall_id, at_mm }, Primary::Kind(ElementKind::Wall))
        }
        other => {
            return Err(ToolFail::args(format!(
                "unknown op `{other}`. Use resize_room, set_wall_length, set_wall_endpoints or split_wall"
            )))
        }
    };
    Ok(Plan::of(vec![command], primary))
}

// ------------------------------------------------------------------- levels

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LevelsIn {
    action: String,
    level: Option<String>,
    name: Option<String>,
    elevation_mm: Option<f64>,
    height_mm: Option<f64>,
}

fn levels(p: &Project, args: &Value) -> Result<Plan, ToolFail> {
    let i: LevelsIn = parse(args)?;
    match i.action.as_str() {
        "add" => {
            if i.level.is_some() {
                return Err(ToolFail::args("action \"add\" takes name, elevation_mm and height_mm, not level"));
            }
            Ok(Plan::of(
                vec![Command::AddLevel {
                    name: i.name.map(|n| n.trim().to_string()),
                    elevation_mm: i.elevation_mm.map(|v| finite(v, "elevation_mm")).transpose()?,
                    height_mm: positive_opt(i.height_mm, "height_mm")?,
                }],
                Primary::Level,
            ))
        }
        "update" => {
            let mut level = find_level(p, &need(&i.level, "update", "level")?)?.clone();
            if i.name.is_none() && i.elevation_mm.is_none() && i.height_mm.is_none() {
                return Err(ToolFail::args("action \"update\" needs name, elevation_mm or height_mm"));
            }
            if let Some(n) = i.name {
                level.name = n.trim().to_string();
            }
            if let Some(e) = i.elevation_mm {
                level.elevation_mm = finite(e, "elevation_mm")?;
            }
            if let Some(h) = i.height_mm {
                level.height_mm = positive(h, "height_mm")?;
            }
            Ok(Plan::of(vec![Command::UpdateLevel { level }], Primary::Level))
        }
        "delete" => {
            let level = find_level(p, &need(&i.level, "delete", "level")?)?;
            Ok(Plan::of(vec![Command::DeleteLevel { level_id: level.id.clone() }], Primary::None))
        }
        other => Err(ToolFail::args(format!("unknown action `{other}`. Use list, add, update or delete"))),
    }
}

/// Storeys whose floor-to-floor height reaches past the floor above.
pub(crate) fn level_overlaps(project: &Project) -> Vec<String> {
    let mut levels: Vec<&Level> = project.levels.iter().collect();
    levels.sort_by(|a, b| a.elevation_mm.total_cmp(&b.elevation_mm));
    levels
        .windows(2)
        .filter(|w| w[0].elevation_mm + w[0].height_mm > w[1].elevation_mm + 1.0)
        .map(|w| {
            format!(
                "{} runs from {:.0} to {:.0} mm, past the floor of {} at {:.0} mm. Set its height_mm to {:.0} with levels action \"update\" if the storeys should stack.",
                w[0].name,
                w[0].elevation_mm,
                w[0].elevation_mm + w[0].height_mm,
                w[1].name,
                w[1].elevation_mm,
                w[1].elevation_mm - w[0].elevation_mm
            )
        })
        .collect()
}

// -------------------------------------------------------------- set_project

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetProjectIn {
    roof: Option<RoofIn>,
    site: Option<Value>,
    north_angle_deg: Option<f64>,
    settings: Option<SettingsIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoofIn {
    kind: Option<RoofKind>,
    pitch_deg: Option<f64>,
    overhang_mm: Option<f64>,
    thickness_mm: Option<f64>,
    ridge_axis: Option<Axis>,
    material_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SiteIn {
    city: Option<String>,
    latitude_deg: f64,
    longitude_deg: f64,
    utc_offset_min: Option<i32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsIn {
    client_name: Option<String>,
    location: Option<String>,
    designer: Option<String>,
    default_wall_thickness_mm: Option<f64>,
    scale_denominator: Option<u32>,
    paper: Option<PaperSize>,
    grid_mm: Option<f64>,
    display_unit: Option<DisplayUnit>,
}

fn set_project(p: &Project, args: &Value) -> Result<Plan, ToolFail> {
    let i: SetProjectIn = parse(args)?;
    let mut commands = vec![];
    if let Some(r) = i.roof {
        let mut roof = p.roof.clone();
        if let Some(k) = r.kind {
            roof.kind = k;
        }
        if let Some(v) = r.pitch_deg {
            roof.pitch_deg = finite(v, "roof.pitch_deg")?;
        }
        if let Some(v) = r.overhang_mm {
            roof.overhang_mm = finite(v, "roof.overhang_mm")?;
        }
        if let Some(v) = r.thickness_mm {
            roof.thickness_mm = positive(v, "roof.thickness_mm")?;
        }
        if let Some(a) = r.ridge_axis {
            roof.ridge_axis = a;
        }
        if let Some(m) = r.material_id {
            check_material(p, &m)?;
            roof.material_id = Some(m);
        }
        if roof != p.roof {
            commands.push(Command::SetRoof { roof });
        }
    }
    let mut settings = p.settings.clone();
    if let Some(site) = &i.site {
        settings.site = if site.is_null() {
            None
        } else {
            let s: SiteIn = parse(site)?;
            if !(-90.0..=90.0).contains(&s.latitude_deg) || !(-180.0..=180.0).contains(&s.longitude_deg) {
                return Err(ToolFail::args("site latitude_deg is -90 to 90 and longitude_deg -180 to 180"));
            }
            Some(Site {
                city: s.city.unwrap_or_else(|| "custom".into()),
                latitude_deg: s.latitude_deg,
                longitude_deg: s.longitude_deg,
                utc_offset_min: s.utc_offset_min.unwrap_or(480),
            })
        };
    }
    if let Some(n) = i.north_angle_deg {
        settings.north_angle_deg = finite(n, "north_angle_deg")?;
    }
    if let Some(s) = i.settings {
        if let Some(v) = s.client_name {
            settings.client_name = v;
        }
        if let Some(v) = s.location {
            settings.location = v;
        }
        if let Some(v) = s.designer {
            settings.designer = v;
        }
        if let Some(v) = s.default_wall_thickness_mm {
            settings.default_wall_thickness_mm = positive(v, "settings.default_wall_thickness_mm")?;
        }
        if let Some(v) = s.scale_denominator {
            settings.scale_denominator = v;
        }
        if let Some(v) = s.paper {
            settings.paper = v;
        }
        if let Some(v) = s.grid_mm {
            settings.grid_mm = positive(v, "settings.grid_mm")?;
        }
        if let Some(v) = s.display_unit {
            settings.display_unit = v;
        }
    }
    if settings != p.settings {
        commands.push(Command::SetProjectSettings { settings });
    }
    if commands.is_empty() {
        return Err(ToolFail::args("give roof, site, north_angle_deg or settings with a value that changes"));
    }
    Ok(Plan::of(commands, Primary::None))
}

// ----------------------------------------------------------------- services

fn edit_pipe(p: &Project, args: &Value, default_level: &Id) -> Result<Plan, ToolFail> {
    let mut rest = args.as_object().cloned().ok_or_else(|| ToolFail::args("expected a JSON object"))?;
    let op = rest.remove("op").and_then(|v| v.as_str().map(str::to_string)).ok_or_else(|| ToolFail::args("op is required"))?;
    let v1 = match op.as_str() {
        "move_points" => "move_pipe_points",
        "segment_length" => "set_pipe_segment_length",
        "slope" => "set_pipe_slope",
        "insert_point" => "insert_pipe_point",
        "remove_point" => "remove_pipe_point",
        other => {
            return Err(ToolFail::args(format!(
                "unknown op `{other}`. Use move_points, segment_length, slope, insert_point or remove_point"
            )))
        }
    };
    // v1 made these required; joined runs follow unless the call says not.
    if matches!(op.as_str(), "move_points" | "segment_length" | "slope") {
        rest.entry("keep_joins").or_insert(json!(true));
    }
    if matches!(op.as_str(), "segment_length" | "slope") {
        rest.entry("hold").or_insert(json!("start"));
    }
    if op == "move_points" {
        for k in ["dx_mm", "dy_mm", "dz_mm"] {
            rest.entry(k).or_insert(json!(0));
        }
    }
    let commands = copilot::to_commands(p, v1, &Value::Object(rest), default_level)?;
    Ok(Plan::of(commands, Primary::None))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddPipeIn {
    system: PipeSystem,
    material: Option<PipeMaterial>,
    diameter_mm: Option<f64>,
    points: Vec<Value>,
    level: Option<String>,
    name: Option<String>,
}

fn add_pipe(p: &Project, args: &Value, default_level: &Id) -> Result<Plan, ToolFail> {
    let i: AddPipeIn = parse(args)?;
    if i.points.len() < 2 {
        return Err(ToolFail::args("points needs at least 2 points {x, y, z}"));
    }
    let pts = i.points.iter().enumerate().map(|(n, v)| point3(v, &format!("points[{n}]"))).collect::<Result<Vec<_>, _>>()?;
    let (material, diameter, _) = defaults::pipe_defaults(i.system);
    let pipe = Pipe {
        id: String::new(),
        level_id: pick_level(p, i.level.as_deref(), default_level)?,
        system: i.system,
        material: i.material.unwrap_or(material),
        diameter_mm: positive_opt(i.diameter_mm, "diameter_mm")?.unwrap_or(diameter),
        points: pts,
        name: i.name.unwrap_or_default().trim().to_string(),
    };
    Ok(Plan::of(vec![Command::AddElement { element: Element::Pipe(pipe) }], Primary::Kind(ElementKind::Pipe)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkIn {
    from: Id,
    to: Vec<Id>,
    action: Option<String>,
}

fn link_devices(p: &Project, args: &Value) -> Result<Plan, ToolFail> {
    let i: LinkIn = parse(args)?;
    let mut asset = match find(p, &i.from)? {
        Element::Asset(a) => a.clone(),
        other => return Err(ToolFail::args(format!("`{}` is a {:?}, not an object", i.from, other.kind()))),
    };
    ids_exist(p, &i.to)?;
    match i.action.as_deref().unwrap_or("link") {
        "link" => {
            for id in &i.to {
                if !asset.links.contains(id) {
                    asset.links.push(id.clone());
                }
            }
        }
        "unlink" => asset.links.retain(|l| !i.to.contains(l)),
        other => return Err(ToolFail::args(format!("unknown action `{other}`. Use link or unlink"))),
    }
    Ok(Plan::of(vec![Command::UpdateElement { element: Element::Asset(asset) }], Primary::None))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewMarkIn {
    action: String,
    issue_id: Option<String>,
    code: Option<String>,
    element_id: Option<Id>,
    note: Option<String>,
}

fn set_review_mark(args: &Value) -> Result<Plan, ToolFail> {
    let i: ReviewMarkIn = parse(args)?;
    let s = |v: Option<String>| v.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    let target = match (s(i.issue_id), s(i.code), s(i.element_id)) {
        (Some(id), None, None) => ReviewTarget::Issue { id },
        (None, Some(code), None) => ReviewTarget::Check { code },
        (None, Some(code), Some(element_id)) => ReviewTarget::Element { code, element_id },
        _ => return Err(ToolFail::args("give issue_id, or code with an optional element_id")),
    };
    let note = match i.action.as_str() {
        "set_aside" => Some(s(i.note).ok_or_else(|| ToolFail::args("set_aside needs a note saying why, in the user's words"))?),
        "reopen" => None,
        other => return Err(ToolFail::args(format!("unknown action `{other}`. Use set_aside or reopen"))),
    };
    Ok(Plan::of(vec![Command::SetReviewMark { target, note }], Primary::None))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_with_box() -> (Project, Id) {
        let mut p = defaults::new_project("t");
        let level = p.levels[0].id.clone();
        let mut doc = guhit_core::Document::new(p.clone());
        doc.apply(
            Command::AddRectRoom { origin: pt(0.0, 0.0), width_mm: 4000.0, depth_mm: 3000.0, name: Some("A".into()), thickness_mm: None, level_id: None },
            Origin::Ai,
        )
        .unwrap();
        p = doc.project().clone();
        (p, level)
    }

    #[test]
    fn a_room_against_a_wall_shares_it() {
        let (p, level) = project_with_box();
        // East neighbour, 3000 deep against a 3000 wall: that side is covered.
        let plan = add_room(&p, &json!({"rect": {"x": 4000, "y": 0, "width_mm": 3000, "depth_mm": 3000}}), &level).unwrap();
        assert_eq!(plan.commands.len(), 3);
        // Taller neighbour: only the part above the old wall is new.
        let plan = add_room(&p, &json!({"rect": {"x": 4000, "y": 0, "width_mm": 3000, "depth_mm": 5000}}), &level).unwrap();
        let west: Vec<&Command> = plan
            .commands
            .iter()
            .filter(|c| matches!(c, Command::AddWall { start, end, .. } if start.x == 4000.0 && end.x == 4000.0))
            .collect();
        assert_eq!(west.len(), 1);
        if let Command::AddWall { start, end, .. } = west[0] {
            assert_eq!((start.y.min(end.y), start.y.max(end.y)), (3000.0, 5000.0));
        }
    }

    #[test]
    fn a_point_finds_its_wall() {
        let (p, level) = project_with_box();
        let (w, along) = wall_at(&p, &level, pt(2000.0, 40.0)).unwrap();
        assert_eq!((w.start, w.end), (pt(0.0, 0.0), pt(4000.0, 0.0)));
        assert_eq!(along, 2000.0);
        let e = wall_at(&p, &level, pt(2000.0, 1500.0)).unwrap_err();
        assert_eq!(e.code, "not_found");
    }

    #[test]
    fn stairs_work_out_risers_and_treads() {
        let (p, level) = project_with_box();
        let plan = add_stair(&p, &json!({"start": {"x": 500, "y": 500}}), &level).unwrap();
        let info = plan.info.unwrap();
        // 3000 mm in 18 risers of 166.7, 17 treads of 250.
        assert_eq!(info["riser_mm"], 166.7);
        assert_eq!(info["treads"], 17);
        assert_eq!(info["tread_mm"], 250.0);
        let Command::AddElement { element: Element::Stair(s) } = &plan.commands[0] else { panic!() };
        assert_eq!(s.rotation_deg, 0.0, "north is the default direction");
        assert_eq!(s.railing.unwrap().height_mm, 900.0);
        let e = add_stair(&p, &json!({"start": [0, 0], "shape": "u"}), &level).unwrap_err();
        assert_eq!(e.code, "unavailable");
    }

    #[test]
    fn unknown_catalog_keys_get_suggestions() {
        let (p, level) = project_with_box();
        let e = place_asset(&p, &json!({"catalog_key": "bed_double", "at": [1000, 1000]}), &level).unwrap_err();
        assert_eq!(e.code, "unknown_catalog_key");
        assert!(e.message.contains("bed-double"), "{}", e.message);
    }

    #[test]
    fn modify_refuses_fields_that_do_not_apply() {
        let (p, _) = project_with_box();
        let wall = p.elements.iter().find(|e| e.kind() == ElementKind::Wall).unwrap().id().clone();
        let view = View::new(&p);
        let e = modify(&view, &json!({"ids": [wall], "set": {"usage": "kitchen"}})).unwrap_err();
        assert!(e.message.contains("wall takes thickness_mm"), "{}", e.message);
        let ok = modify(&view, &json!({"ids": [wall], "set": {"height_mm": 4500}})).unwrap();
        assert!(matches!(&ok.commands[0], Command::UpdateElement { element: Element::Wall(w) } if w.height_mm == Some(4500.0)));
    }
}
