//! The Wave 2 drafting tools of the core toolset (DECISIONS D53, D54): grid
//! lines, dimension strings and notes, roofs over an area with their posts,
//! and door and window types with marks.
//!
//! Each is an editing tool like the Wave 1 ones: translated here into typed
//! commands, staged and committed as one undo step by `stage`, so every
//! result lists the review items it added and resolved. `types` action
//! "list" is the one read: it answers on its own, never through `stage`.

use guhit_app::AppService;
use guhit_model::*;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::fail::ToolFail;
use crate::geo::{self, pt};
use crate::tools::{obj, scope_schema, Output, ToolDef, MM};
use crate::translate::{
    check_material, find, find_level, find_type, finite, kind_word, parse, pick_level, point, points, positive, positive_opt,
    Plan, Primary, Then, View,
};

// ------------------------------------------------------------- definitions

fn pt_schema() -> Value {
    json!({"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}}, "required": ["x", "y"]})
}

fn pts(desc: &str) -> Value {
    json!({"type": "array", "items": pt_schema(), "description": desc})
}

fn nums(desc: &str) -> Value {
    json!({"type": "array", "items": {"type": "number"}, "description": desc})
}

fn s(desc: &str) -> Value {
    json!({"type": "string", "description": desc})
}

fn n(desc: &str) -> Value {
    json!({"type": "number", "description": desc})
}

fn e(values: &[&str], desc: &str) -> Value {
    json!({"type": "string", "enum": values, "description": desc})
}

fn def(name: &'static str, description: String, schema: Value) -> ToolDef {
    ToolDef { name, description, schema, read_only: false, open_world: false }
}

fn with_scope(mut schema: Value) -> Value {
    schema["properties"]["scope"] = scope_schema();
    schema
}

pub fn definitions() -> Vec<ToolDef> {
    vec![
        def(
            "add_grid",
            format!("Structural grid lines, on every level, one undo step. A system: numbered lines 1, 2, 3 at the gaps x_spacings_mm apart going east and lettered A, B, C at y_spacings_mm going north, from origin, turned by rotation_deg; labels already taken continue. Or single lines {{label, start, end}}. Plans, elevations and sections draw them. {MM}"),
            obj(json!({
                "origin": pt_schema(),
                "rotation_deg": n(""),
                "x_spacings_mm": nums("Gaps between lines 1, 2, 3, west to east."),
                "y_spacings_mm": nums("Gaps between lines A, B, C, south to north."),
                "lines": {"type": "array", "items": {"type": "object", "properties": {"label": {"type": "string"}, "start": pt_schema(), "end": pt_schema()}, "required": ["label", "start", "end"]}},
            }), &[]),
        ),
        def(
            "annotate",
            format!("Dimensions and notes on a level, one undo step. kind string: a dimension string through points (2 or more, in order), offset_mm from them; or through the jambs of walls' openings (walls, face) or between grids, and those strings follow the model. auto: the engine's exterior strings (overall, walls and grids, openings) or rooms' inside strings, rewritten after every edit. note: text at a point. {MM}"),
            with_scope(obj(json!({
                "kind": e(&["string", "auto", "note"], ""),
                "points": pts(""),
                "walls": {"type": "array", "items": {"type": "string"}, "description": "Collinear walls."},
                "face": e(&["centerline", "exterior", "interior"], "Default exterior."),
                "grids": {"type": "array", "items": {"type": "string"}},
                "offset_mm": n("+ to the left going first to last. Default 800; on walls, on the side of face: outside the building for exterior, inside for interior."),
                "kinds": {"type": "array", "items": {"type": "string", "enum": ["exterior", "rooms"]}},
                "text": s(""),
                "at": pt_schema(),
                "size_mm": n("Default 250."),
                "rotation_deg": n(""),
                "level": s(""),
            }), &["kind"])),
        ),
        def(
            "add_roof",
            format!("A roof, one undo step: over the top floor (outline top_footprint, the house roof; it replaces that roof's shape), over a level's walls (footprint) or a drawn area (points: a carport, a lanai). shape flat, shed, gable or hip; pitched roofs need square corners, hip over 4, 6 or 8; gable_sides turn hip ends into gables on a rectangle. A shed rises along ridge_axis toward +x (east) or +y (north), its low eave on the west or south; flip true turns it to rise toward -x or -y, so a lanai north or east of the house slopes away from it. posts puts columns under open eaves. {MM}"),
            with_scope(obj(json!({
                "outline": e(&["top_footprint", "footprint", "polygon"], "Default polygon with points, else top_footprint."),
                "points": pts(""),
                "shape": e(&["flat", "shed", "gable", "hip"], "Default hip."),
                "pitch_deg": n("Default 25."),
                "ridge_axis": e(&["x", "y"], "Gable ridge, shed slope."),
                "flip": {"type": "boolean", "description": "Shed only: rise toward -x or -y instead. Default false."},
                "gable_sides": {"type": "array", "items": {"type": "integer"}, "description": "Side i runs point i to i + 1."},
                "overhang_mm": n("Default 600."),
                "thickness_mm": n("Default 150."),
                "eave_mm": n("Default the level height."),
                "fascia_mm": n("Default 200; 0 none."),
                "gutter_mm": n(""),
                "material_id": s(""),
                "level": s("footprint, polygon."),
                "posts": {"description": "true or {size_mm, max_spacing_mm}.", "anyOf": [{"type": "boolean"}, {"type": "object"}]},
            }), &[])),
        ),
        def(
            "types",
            format!("Door and window types with marks (D1, W1), as in Revit: a type writes its style, size and sill into every opening of it in the same step, so the schedule matches the plan. action list; set (type: mark or id to change, or none for a new one; mark, kind, style, width_mm, height_mm, sill_mm, material_id, description, remarks); delete (types); assign (every untyped opening gets a type, per level); apply (type to openings, or type null to untype them). add_opening and modify take type too. {MM}"),
            obj(json!({
                "action": e(&["list", "set", "delete", "assign", "apply"], ""),
                "type": {"description": "Mark or id.", "type": ["string", "null"]},
                "mark": s("Empty: the next free."),
                "kind": e(&["door", "window"], ""),
                "style": e(&["swing_single", "swing_double", "sliding", "fixed", "casement", "jalousie"], ""),
                "width_mm": n(""), "height_mm": n(""),
                "sill_mm": n("Windows."),
                "material_id": s(""),
                "description": s("Schedule text."),
                "remarks": s(""),
                "types": {"type": "array", "items": {"type": "string"}},
                "openings": {"type": "array", "items": {"type": "string"}},
                "level": s(""),
            }), &["action"]),
        ),
    ]
}

// ------------------------------------------------------------------- grids

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GridLineIn {
    label: String,
    start: Value,
    end: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddGridIn {
    origin: Option<Value>,
    rotation_deg: Option<f64>,
    x_spacings_mm: Option<Vec<f64>>,
    y_spacings_mm: Option<Vec<f64>>,
    lines: Option<Vec<GridLineIn>>,
}

fn spacings(v: Option<Vec<f64>>, what: &str) -> Result<Vec<f64>, ToolFail> {
    v.unwrap_or_default().into_iter().map(|g| positive(g, what)).collect()
}

pub(crate) fn add_grid(_p: &Project, args: &Value) -> Result<Plan, ToolFail> {
    let i: AddGridIn = parse(args)?;
    let xs = spacings(i.x_spacings_mm, "x_spacings_mm")?;
    let ys = spacings(i.y_spacings_mm, "y_spacings_mm")?;
    let lines = i.lines.unwrap_or_default();
    if xs.is_empty() && ys.is_empty() && lines.is_empty() {
        return Err(ToolFail::args("give x_spacings_mm and y_spacings_mm for a grid system, or lines {label, start, end}"));
    }
    let mut commands = vec![];
    if !xs.is_empty() || !ys.is_empty() {
        commands.push(Command::AddGridSystem {
            origin: match &i.origin {
                Some(v) => point(v, "origin")?,
                None => pt(0.0, 0.0),
            },
            rotation_deg: finite(i.rotation_deg.unwrap_or(0.0), "rotation_deg")?,
            x_spacings_mm: xs,
            y_spacings_mm: ys,
        });
    } else if i.origin.is_some() || i.rotation_deg.is_some() {
        return Err(ToolFail::args("origin and rotation_deg go with x_spacings_mm and y_spacings_mm"));
    }
    for (k, l) in lines.into_iter().enumerate() {
        let label = l.label.trim().to_string();
        if label.is_empty() {
            return Err(ToolFail::args(format!("lines[{k}].label is empty")));
        }
        commands.push(Command::AddElement {
            element: Element::Grid(GridLine {
                id: String::new(),
                label,
                start: point(&l.start, &format!("lines[{k}].start"))?,
                end: point(&l.end, &format!("lines[{k}].end"))?,
            }),
        });
    }
    Ok(Plan::of(commands, Primary::Kind(ElementKind::Grid)))
}

// --------------------------------------------------------------- annotate

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnnotateIn {
    kind: String,
    points: Option<Vec<Value>>,
    walls: Option<Vec<Id>>,
    face: Option<DimFace>,
    grids: Option<Vec<Id>>,
    offset_mm: Option<f64>,
    kinds: Option<Vec<AutoDimKind>>,
    text: Option<String>,
    at: Option<Value>,
    size_mm: Option<f64>,
    rotation_deg: Option<f64>,
    level: Option<String>,
}

/// A dimension line's offset when the call gives none: clear of a wall's
/// face and its door swings at 1:100.
const DIM_OFFSET_MM: f64 = 800.0;
/// A note's text height when the call gives none: 2.5 mm at 1:100.
const NOTE_SIZE_MM: f64 = 250.0;

pub(crate) fn annotate(view: &View, args: &Value, default_level: &Id) -> Result<Plan, ToolFail> {
    let p = view.project;
    let i: AnnotateIn = parse(args)?;
    let level_id = pick_level(p, i.level.as_deref(), default_level)?;
    let only = |allowed: &[&str]| -> Result<(), ToolFail> {
        let given = [
            ("points", i.points.is_some()),
            ("walls", i.walls.is_some()),
            ("face", i.face.is_some()),
            ("grids", i.grids.is_some()),
            ("offset_mm", i.offset_mm.is_some()),
            ("kinds", i.kinds.is_some()),
            ("text", i.text.is_some()),
            ("at", i.at.is_some()),
            ("size_mm", i.size_mm.is_some()),
            ("rotation_deg", i.rotation_deg.is_some()),
        ];
        match given.iter().find(|(name, on)| *on && !allowed.contains(name)) {
            Some((name, _)) => Err(ToolFail::args(format!("kind \"{}\" does not take {name}", i.kind))),
            None => Ok(()),
        }
    };
    match i.kind.as_str() {
        "note" => {
            only(&["text", "at", "size_mm", "rotation_deg"])?;
            let text = i.text.as_deref().map(str::trim).filter(|t| !t.is_empty()).ok_or_else(|| ToolFail::args("a note needs text"))?;
            let at = point(i.at.as_ref().ok_or_else(|| ToolFail::args("a note needs at {x, y}"))?, "at")?;
            let note = Annotation {
                id: String::new(),
                level_id,
                position: at,
                text: text.to_string(),
                size_mm: positive_opt(i.size_mm, "size_mm")?.unwrap_or(NOTE_SIZE_MM),
                rotation_deg: finite(i.rotation_deg.unwrap_or(0.0), "rotation_deg")?,
            };
            Ok(Plan::of(vec![Command::AddElement { element: Element::Annotation(note) }], Primary::Kind(ElementKind::Annotation)))
        }
        "auto" => {
            only(&["kinds"])?;
            let kinds = i.kinds.unwrap_or_else(|| vec![AutoDimKind::Exterior]);
            if kinds.is_empty() {
                return Err(ToolFail::args("kinds is empty; use exterior, rooms or both"));
            }
            let mut plan = Plan::of(vec![Command::AutoDimension { level_id, kinds }], Primary::Kind(ElementKind::Dimension));
            plan.info = Some(json!({"note": "Automatic dimensions follow every later edit in the same step. Editing one by hand makes it a plain dimension."}));
            Ok(plan)
        }
        "string" => {
            only(&["points", "walls", "face", "grids", "offset_mm", "text"])?;
            let mut offset = finite(i.offset_mm.unwrap_or(DIM_OFFSET_MM), "offset_mm")?;
            let text_override = i.text.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
            let (level_id, line, auto) = match (&i.points, &i.walls, &i.grids) {
                (Some(list), None, None) => {
                    if i.face.is_some() {
                        return Err(ToolFail::args("face goes with walls"));
                    }
                    (level_id, points(list, "points")?, None)
                }
                (None, Some(walls), None) => {
                    let (level, line) = wall_string(p, walls)?;
                    let face = i.face.unwrap_or(DimFace::Exterior);
                    // The engine measures the face on the line's side, so
                    // with no offset given the line goes on the side of the
                    // face asked for.
                    if i.offset_mm.is_none() && face != DimFace::Centerline {
                        if let Some(Element::Wall(w)) = walls.first().and_then(|id| p.elements.iter().find(|e| e.id() == id)) {
                            offset = side_offset(view.derived(), w, face == DimFace::Exterior);
                        }
                    }
                    (level, line, Some(DimRef::Openings { wall_ids: walls.clone(), face }))
                }
                (None, None, Some(grids)) => {
                    if i.face.is_some() {
                        return Err(ToolFail::args("face goes with walls"));
                    }
                    (level_id, grid_string(p, grids)?, Some(DimRef::Grids { grid_ids: grids.clone() }))
                }
                _ => return Err(ToolFail::args("a string measures points, walls or grids: give one of them")),
            };
            if line.len() < 2 {
                return Err(ToolFail::args("a dimension string needs at least 2 points"));
            }
            let (a, b) = (line[0], line[line.len() - 1]);
            let dim = Dimension {
                id: String::new(),
                level_id,
                a,
                b,
                offset_mm: offset,
                text_override,
                through: line[1..line.len() - 1].to_vec(),
                auto,
            };
            let parts: Vec<f64> = line.windows(2).map(|w| geo::r1(geo::dist(w[0], w[1]))).collect();
            let mut plan = Plan::of(vec![Command::AddElement { element: Element::Dimension(dim) }], Primary::Kind(ElementKind::Dimension));
            plan.info = Some(json!({"overall_mm": geo::r1(geo::dist(a, b)), "parts_mm": parts}));
            Ok(plan)
        }
        other => Err(ToolFail::args(format!("unknown kind `{other}`. Use string, auto or note"))),
    }
}

/// The default offset of a string along `wall`: outside the building for the
/// exterior face, inside for the interior one. Outside is where the level's
/// footprint is not, just past the wall's face on its left; a wall of no
/// closed building takes the left.
fn side_offset(derived: &Derived, wall: &Wall, exterior: bool) -> f64 {
    let Some(u) = dir(wall.start, wall.end) else { return DIM_OFFSET_MM };
    let reach = wall.thickness_mm / 2.0 + 50.0;
    let probe = pt((wall.start.x + wall.end.x) / 2.0 - u.y * reach, (wall.start.y + wall.end.y) / 2.0 + u.x * reach);
    let left_inside = derived
        .footprints
        .iter()
        .any(|f| f.level_id == wall.level_id && f.polygon.len() >= 3 && geo::contains(&f.polygon, probe));
    if left_inside == exterior {
        -DIM_OFFSET_MM
    } else {
        DIM_OFFSET_MM
    }
}

/// `overall_mm` and `parts_mm` of a dimension as stored: from `a` through
/// every `through` point to `b`.
pub(crate) fn report_parts(d: &Dimension, info: &mut Value) {
    let mut line = vec![d.a];
    line.extend(d.through.iter().copied());
    line.push(d.b);
    let parts: Vec<f64> = line.windows(2).map(|w| geo::r1(geo::dist(w[0], w[1]))).collect();
    info["overall_mm"] = json!(geo::r1(geo::dist(d.a, d.b)));
    info["parts_mm"] = json!(parts);
}

/// Unit vector from `a` to `b`, or None when they meet.
fn dir(a: Point, b: Point) -> Option<Point> {
    let d = geo::dist(a, b);
    (d > 1e-6).then(|| pt((b.x - a.x) / d, (b.y - a.y) / d))
}

/// The witness points of a string along collinear walls: their ends and
/// the jambs of every opening on them, in order along the first wall. The
/// engine rewrites them after each edit (`DimRef::Openings`).
fn wall_string(p: &Project, walls: &[Id]) -> Result<(Id, Vec<Point>), ToolFail> {
    if walls.is_empty() {
        return Err(ToolFail::args("walls is empty"));
    }
    let mut list: Vec<&Wall> = vec![];
    for id in walls {
        match find(p, id)? {
            Element::Wall(w) => list.push(w),
            other => return Err(ToolFail::args(format!("`{id}` is a {:?}, not a wall", other.kind()))),
        }
    }
    let first = list[0];
    let u = dir(first.start, first.end).ok_or_else(|| ToolFail::args("the first wall has no length"))?;
    let along = |q: Point| (q.x - first.start.x) * u.x + (q.y - first.start.y) * u.y;
    let off = |q: Point| (q.x - first.start.x) * -u.y + (q.y - first.start.y) * u.x;
    let mut ts: Vec<f64> = vec![];
    for w in &list {
        if w.level_id != first.level_id {
            return Err(ToolFail::args("the walls of one string are on one level"));
        }
        if off(w.start).abs() > 1.0 || off(w.end).abs() > 1.0 {
            return Err(ToolFail::args(format!("wall {} is not in line with wall {}: a string measures collinear walls", w.id, first.id)).with_ids(vec![w.id.clone(), first.id.clone()]));
        }
        ts.push(along(w.start));
        ts.push(along(w.end));
        for el in &p.elements {
            if let Element::Opening(o) = el {
                if o.wall_id == w.id {
                    if let Some(c) = geo::opening_center(w, o) {
                        ts.push(along(c) - o.width_mm / 2.0);
                        ts.push(along(c) + o.width_mm / 2.0);
                    }
                }
            }
        }
    }
    ts.sort_by(|a, b| a.total_cmp(b));
    ts.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    let line = ts.into_iter().map(|t| pt(first.start.x + u.x * t, first.start.y + u.y * t)).collect();
    Ok((first.level_id.clone(), line))
}

/// A string across parallel grid lines: from the middle of the first,
/// square across to each of the others.
fn grid_string(p: &Project, grids: &[Id]) -> Result<Vec<Point>, ToolFail> {
    let mut list: Vec<&GridLine> = vec![];
    for id in grids {
        match find(p, id)? {
            Element::Grid(g) => list.push(g),
            other => return Err(ToolFail::args(format!("`{id}` is a {:?}, not a grid line", other.kind()))),
        }
    }
    let [first, ..] = list.as_slice() else {
        return Err(ToolFail::args("grids needs at least 2 grid lines"));
    };
    if list.len() < 2 {
        return Err(ToolFail::args("grids needs at least 2 grid lines"));
    }
    let u = dir(first.start, first.end).ok_or_else(|| ToolFail::args("the first grid line has no length"))?;
    let normal = pt(-u.y, u.x);
    let mid = pt((first.start.x + first.end.x) / 2.0, (first.start.y + first.end.y) / 2.0);
    let mut out: Vec<(f64, Point)> = vec![];
    for g in &list {
        let v = dir(g.start, g.end).ok_or_else(|| ToolFail::args(format!("grid line {} has no length", g.label)))?;
        if (u.x * v.y - u.y * v.x).abs() > 0.01 {
            return Err(ToolFail::args(format!("grid lines {} and {} are not parallel", first.label, g.label)));
        }
        let t = (g.start.x - mid.x) * normal.x + (g.start.y - mid.y) * normal.y;
        out.push((t, pt(mid.x + normal.x * t, mid.y + normal.y * t)));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(out.into_iter().map(|(_, q)| q).collect())
}

// ------------------------------------------------------------------- roofs

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PostsIn {
    size_mm: Option<f64>,
    max_spacing_mm: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddRoofIn {
    outline: Option<String>,
    points: Option<Vec<Value>>,
    shape: Option<RoofShape>,
    pitch_deg: Option<f64>,
    ridge_axis: Option<Axis>,
    flip: Option<bool>,
    gable_sides: Option<Vec<u32>>,
    overhang_mm: Option<f64>,
    thickness_mm: Option<f64>,
    eave_mm: Option<f64>,
    fascia_mm: Option<f64>,
    gutter_mm: Option<f64>,
    material_id: Option<String>,
    level: Option<String>,
    posts: Option<Value>,
}

/// Pitch, overhang, thickness and fascia a new roof takes (PH defaults).
const ROOF_PITCH_DEG: f64 = 25.0;
const ROOF_OVERHANG_MM: f64 = 600.0;
const ROOF_THICKNESS_MM: f64 = 150.0;
const FASCIA_MM: f64 = 200.0;
const POST_MM: f64 = 200.0;
const POST_SPACING_MM: f64 = 3000.0;

pub(crate) fn add_roof(p: &Project, args: &Value, default_level: &Id) -> Result<Plan, ToolFail> {
    let i: AddRoofIn = parse(args)?;
    if i.flip == Some(true) && i.shape != Some(RoofShape::Shed) {
        return Err(ToolFail::args("flip turns a shed: give shape \"shed\" with it"));
    }
    let outline_word = i.outline.as_deref().map(str::trim).unwrap_or(if i.points.is_some() { "polygon" } else { "top_footprint" });
    let outline = match outline_word {
        "polygon" => {
            let list = i.points.as_ref().ok_or_else(|| ToolFail::args("outline polygon needs points"))?;
            let pts = points(list, "points")?;
            if pts.len() < 3 || geo::signed_area(&pts).abs() < 1.0e5 {
                return Err(ToolFail::args("a roof outline needs 3 or more points around an area"));
            }
            RoofOutline::Polygon { points: geo::ccw(pts) }
        }
        "footprint" | "top_footprint" if i.points.is_some() => {
            return Err(ToolFail::args(format!("outline {outline_word} follows the walls; points are for outline polygon")))
        }
        "footprint" => RoofOutline::Footprint,
        "top_footprint" => RoofOutline::TopFootprint,
        other => return Err(ToolFail::args(format!("unknown outline `{other}`. Use top_footprint, footprint or polygon"))),
    };
    let top = outline == RoofOutline::TopFootprint;
    if top && i.level.is_some() {
        return Err(ToolFail::args("the roof over the top floor belongs to no level: leave level out (it sits on the topmost level with a building)"));
    }
    if let Some(m) = &i.material_id {
        check_material(p, m)?;
    }
    let trim = |v: Option<f64>, default: Option<f64>, what: &str| -> Result<Option<Trim>, ToolFail> {
        match v.or(default) {
            None => Ok(None),
            Some(z) if z == 0.0 => Ok(None),
            Some(z) => Ok(Some(Trim { size_mm: positive(z, what)?, material_id: None })),
        }
    };
    let existing = if top { p.top_roof_area().cloned() } else { None };
    let roof = RoofArea {
        id: existing.as_ref().map(|r| r.id.clone()).unwrap_or_default(),
        level_id: if top { None } else { Some(pick_level(p, i.level.as_deref(), default_level)?) },
        outline,
        shape: i.shape.unwrap_or(RoofShape::Hip),
        pitch_deg: finite(i.pitch_deg.unwrap_or(ROOF_PITCH_DEG), "pitch_deg")?,
        ridge_axis: i.ridge_axis.unwrap_or(Axis::X),
        flip: i.flip.unwrap_or(false),
        gable_sides: i.gable_sides.unwrap_or_default(),
        overhang_mm: finite(i.overhang_mm.unwrap_or(ROOF_OVERHANG_MM), "overhang_mm")?,
        thickness_mm: positive(i.thickness_mm.unwrap_or(ROOF_THICKNESS_MM), "thickness_mm")?,
        eave_mm: i.eave_mm.map(|v| finite(v, "eave_mm")).transpose()?,
        material_id: i.material_id.or_else(|| Some(defaults::MAT_ROOF_DEFAULT.to_string())),
        fascia: trim(i.fascia_mm, Some(FASCIA_MM), "fascia_mm")?,
        gutter: trim(i.gutter_mm, None, "gutter_mm")?,
    };
    let posts = match &i.posts {
        None | Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(Value::Bool(true)) => Some((POST_MM, POST_SPACING_MM)),
        Some(v) => {
            let p: PostsIn = parse(v)?;
            Some((positive(p.size_mm.unwrap_or(POST_MM), "posts.size_mm")?, positive(p.max_spacing_mm.unwrap_or(POST_SPACING_MM), "posts.max_spacing_mm")?))
        }
    };
    let roof_id = existing.as_ref().map(|r| r.id.clone());
    let command = match existing {
        Some(_) => Command::UpdateElement { element: Element::Roof(roof) },
        None => Command::AddElement { element: Element::Roof(roof) },
    };
    let mut plan = Plan::of(vec![command], Primary::Kind(ElementKind::Roof));
    if let Some((size_mm, max_spacing_mm)) = posts {
        plan.then.push(Then::RoofPosts { roof_id, size_mm, max_spacing_mm });
    }
    plan.info = Some(json!({"note": "Planes, ridges, hips, eaves and areas are worked out by the engine: query what \"describe\" with the roof id, look the plan or the 3D view."}));
    Ok(plan)
}

// ------------------------------------------------------------------- types

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TypesIn {
    action: String,
    #[serde(rename = "type", default)]
    type_of: Option<Value>,
    mark: Option<String>,
    kind: Option<OpeningType>,
    style: Option<OpeningStyle>,
    width_mm: Option<f64>,
    height_mm: Option<f64>,
    sill_mm: Option<f64>,
    material_id: Option<String>,
    description: Option<String>,
    remarks: Option<String>,
    types: Option<Vec<String>>,
    openings: Option<Vec<Id>>,
    level: Option<String>,
}

/// The default style of a new type of `kind`.
fn default_style(kind: OpeningType) -> OpeningStyle {
    match kind {
        OpeningType::Door => OpeningStyle::SwingSingle,
        OpeningType::Window => OpeningStyle::Casement,
    }
}

pub(crate) fn types(p: &Project, args: &Value) -> Result<Plan, ToolFail> {
    let i: TypesIn = parse(args)?;
    let type_text = || -> Result<Option<String>, ToolFail> {
        match &i.type_of {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(t)) if t.trim().is_empty() => Ok(None),
            Some(Value::String(t)) => Ok(Some(t.trim().to_string())),
            Some(_) => Err(ToolFail::args("type is a type's mark or id")),
        }
    };
    match i.action.as_str() {
        "list" => Err(ToolFail::args("action \"list\" reads the types: call types on its own, not as a step of batch")),
        "set" => {
            if i.types.is_some() || i.openings.is_some() || i.level.is_some() {
                return Err(ToolFail::args("action \"set\" takes type, mark, kind, style, width_mm, height_mm, sill_mm, material_id, description and remarks"));
            }
            let mut spec = match type_text()? {
                Some(t) => find_type(p, &t)?.clone(),
                None => {
                    let kind = i.kind.ok_or_else(|| ToolFail::args("a new type needs kind (door or window), width_mm and height_mm"))?;
                    let (Some(w), Some(h)) = (i.width_mm, i.height_mm) else {
                        return Err(ToolFail::args("a new type needs width_mm and height_mm"));
                    };
                    OpeningSpec {
                        id: String::new(),
                        mark: String::new(),
                        opening_type: kind,
                        style: default_style(kind),
                        width_mm: positive(w, "width_mm")?,
                        height_mm: positive(h, "height_mm")?,
                        sill_mm: None,
                        material_id: None,
                        description: String::new(),
                        remarks: String::new(),
                    }
                }
            };
            if let Some(k) = i.kind {
                if k != spec.opening_type {
                    return Err(ToolFail::args(format!("type {} is a {} type; a type keeps its kind, so make a new one", spec.mark, kind_word(spec.opening_type))));
                }
            }
            if let Some(m) = i.mark {
                spec.mark = m.trim().to_string();
            }
            if let Some(s) = i.style {
                spec.style = s;
            }
            if let Some(w) = i.width_mm {
                spec.width_mm = positive(w, "width_mm")?;
            }
            if let Some(h) = i.height_mm {
                spec.height_mm = positive(h, "height_mm")?;
            }
            if let Some(s) = i.sill_mm {
                if spec.opening_type == OpeningType::Door {
                    return Err(ToolFail::args("a door type has no sill_mm"));
                }
                spec.sill_mm = Some(finite(s, "sill_mm")?);
            }
            if let Some(m) = i.material_id {
                check_material(p, &m)?;
                spec.material_id = Some(m);
            }
            if let Some(d) = i.description {
                spec.description = d.trim().to_string();
            }
            if let Some(r) = i.remarks {
                spec.remarks = r.trim().to_string();
            }
            let using = p.elements.iter().filter(|e| matches!(e, Element::Opening(o) if !spec.id.is_empty() && o.type_id.as_deref() == Some(spec.id.as_str()))).count();
            let mut plan = Plan::of(vec![Command::UpsertOpeningSpec { spec: Box::new(spec) }], Primary::None);
            plan.info = Some(json!({"openings_of_the_type": using, "note": "Every opening of the type took its style, size and sill in this step."}));
            Ok(plan)
        }
        "delete" => {
            let names = i.types.as_ref().filter(|t| !t.is_empty()).ok_or_else(|| ToolFail::args("action \"delete\" needs types: marks or ids"))?;
            let ids = names.iter().map(|t| find_type(p, t).map(|s| s.id.clone())).collect::<Result<Vec<_>, _>>()?;
            Ok(Plan::of(vec![Command::DeleteOpeningSpecs { ids }], Primary::None))
        }
        "assign" => {
            let level_id = match &i.level {
                Some(l) => Some(find_level(p, l)?.id.clone()),
                None => None,
            };
            Ok(Plan::of(vec![Command::AssignOpeningTypes { level_id }], Primary::None))
        }
        "apply" => {
            let openings = i.openings.as_ref().filter(|o| !o.is_empty()).ok_or_else(|| ToolFail::args("action \"apply\" needs openings: their ids"))?;
            let spec = match type_text()? {
                Some(t) => Some(find_type(p, &t)?.clone()),
                None if matches!(i.type_of, Some(Value::Null)) => None,
                None => return Err(ToolFail::args("action \"apply\" needs type: a mark or id, or null to make the openings untyped")),
            };
            let mut commands = vec![];
            for id in openings {
                let Element::Opening(o) = find(p, id)? else {
                    return Err(ToolFail::args(format!("`{id}` is not a door or window")).with_ids(vec![id.clone()]));
                };
                let mut o = o.clone();
                match &spec {
                    Some(spec) => {
                        if spec.opening_type != o.opening_type {
                            return Err(ToolFail::args(format!("type {} is a {} type; {id} is a {}", spec.mark, kind_word(spec.opening_type), kind_word(o.opening_type))).with_ids(vec![id.clone()]));
                        }
                        // As the engine writes it into every opening of the type.
                        o.type_id = Some(spec.id.clone());
                        o.style = spec.style;
                        o.width_mm = spec.width_mm;
                        o.height_mm = spec.height_mm;
                        if let Some(s) = spec.sill_mm {
                            o.sill_mm = s;
                        }
                    }
                    None => o.type_id = None,
                }
                commands.push(Command::UpdateElement { element: Element::Opening(o) });
            }
            Ok(Plan::of(commands, Primary::None))
        }
        other => Err(ToolFail::args(format!("unknown action `{other}`. Use list, set, delete, assign or apply"))),
    }
}

/// `types` action "list": every type with its mark, size and how many
/// openings it has on each level, and how many openings have no type.
pub(crate) async fn types_list(app: &AppService, ctx: &crate::agent::CallContext) -> Result<Output, ToolFail> {
    let r = crate::view::read(app, ctx).await;
    let doc = r.doc()?;
    let p = doc.project();
    let level_of = |o: &Opening| match p.elements.iter().find(|e| e.id() == &o.wall_id) {
        Some(Element::Wall(w)) => Some(w.level_id.clone()),
        _ => None,
    };
    let openings: Vec<&Opening> = p
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Opening(o) => Some(o),
            _ => None,
        })
        .collect();
    let rows: Vec<Value> = p
        .opening_types
        .iter()
        .map(|t| {
            let mine: Vec<&&Opening> = openings.iter().filter(|o| o.type_id.as_deref() == Some(t.id.as_str())).collect();
            let per_level: Vec<Value> = p
                .levels
                .iter()
                .filter_map(|l| {
                    let n = mine.iter().filter(|o| level_of(o).as_deref() == Some(l.id.as_str())).count();
                    (n > 0).then(|| json!({"level": l.name, "count": n}))
                })
                .collect();
            json!({
                "id": t.id, "mark": t.mark, "kind": t.opening_type, "style": t.style,
                "width_mm": t.width_mm, "height_mm": t.height_mm, "sill_mm": t.sill_mm,
                "material_id": t.material_id, "description": t.description, "remarks": t.remarks,
                "count": mine.len(), "per_level": per_level,
            })
        })
        .collect();
    let untyped: Vec<&Id> = openings.iter().filter(|o| o.type_id.is_none()).map(|o| &o.id).collect();
    Ok(Output::Json(json!({
        "revision": doc.revision(),
        "types": rows,
        "untyped_openings": untyped.len(),
        "untyped_ids": untyped.into_iter().take(50).collect::<Vec<_>>(),
        "note": "The door and window schedule counts by mark: query what \"schedules\". types action \"assign\" gives every untyped opening a type.",
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bungalow() -> Project {
        guhit_core::templates::sample_bungalow()
    }

    #[test]
    fn a_grid_system_and_single_lines() {
        let p = bungalow();
        let plan = add_grid(&p, &json!({"x_spacings_mm": [4000, 4000], "y_spacings_mm": [3000], "origin": {"x": 0, "y": 0}})).unwrap();
        assert!(matches!(&plan.commands[0], Command::AddGridSystem { x_spacings_mm, y_spacings_mm, .. } if x_spacings_mm.len() == 2 && y_spacings_mm.len() == 1));
        let plan = add_grid(&p, &json!({"lines": [{"label": "A", "start": [0, -1000], "end": [0, 7000]}]})).unwrap();
        assert!(matches!(&plan.commands[0], Command::AddElement { element: Element::Grid(g) } if g.label == "A"));
        assert_eq!(add_grid(&p, &json!({})).unwrap_err().code, "bad_args");
        assert_eq!(add_grid(&p, &json!({"x_spacings_mm": [0]})).unwrap_err().code, "bad_args");
    }

    #[test]
    fn a_string_along_walls_measures_their_ends_and_jambs() {
        let p = bungalow();
        let level = p.levels[0].id.clone();
        let south = "00000000-0000-4000-8000-000000000101".to_string();
        let view = View::new(&p);
        let plan = annotate(&view, &json!({"kind": "string", "walls": [south]}), &level).unwrap();
        let Command::AddElement { element: Element::Dimension(d) } = &plan.commands[0] else { panic!("{:?}", plan.commands) };
        assert!(matches!(&d.auto, Some(DimRef::Openings { face: DimFace::Exterior, .. })));
        assert_eq!(geo::dist(d.a, d.b), 8000.0);
        assert!(!d.through.is_empty(), "the jambs of the south wall's openings");
        let note = annotate(&view, &json!({"kind": "note", "text": "Finish grade +0.150", "at": [100, -900]}), &level).unwrap();
        assert!(matches!(&note.commands[0], Command::AddElement { element: Element::Annotation(a) } if a.size_mm == 250.0));
        let auto = annotate(&view, &json!({"kind": "auto"}), &level).unwrap();
        assert!(matches!(&auto.commands[0], Command::AutoDimension { kinds, .. } if kinds == &vec![AutoDimKind::Exterior]));
        assert_eq!(annotate(&view, &json!({"kind": "note", "text": "x", "at": [0, 0], "walls": []}), &level).unwrap_err().code, "bad_args");
        let pts = annotate(&view, &json!({"kind": "string", "points": [[0, -1000], [3000, -1000], [8000, -1000]]}), &level).unwrap();
        assert_eq!(pts.info.unwrap()["parts_mm"], json!([3000.0, 5000.0]));
    }

    #[test]
    fn a_roof_over_the_top_floor_replaces_its_shape_and_an_area_roof_is_new() {
        let p = bungalow();
        let level = p.levels[0].id.clone();
        let top = add_roof(&p, &json!({"shape": "hip", "pitch_deg": 30}), &level).unwrap();
        let Command::UpdateElement { element: Element::Roof(r) } = &top.commands[0] else { panic!() };
        assert_eq!((r.id.as_str(), r.shape, r.level_id.as_ref()), (p.top_roof_area().unwrap().id.as_str(), RoofShape::Hip, None));
        let carport = add_roof(&p, &json!({"points": [[8000, 0], [11000, 0], [11000, 6000], [8000, 6000]], "shape": "shed", "posts": true}), &level).unwrap();
        let Command::AddElement { element: Element::Roof(r) } = &carport.commands[0] else { panic!() };
        assert_eq!((r.shape, r.level_id.as_deref()), (RoofShape::Shed, Some(level.as_str())));
        assert!(matches!(&carport.then[0], Then::RoofPosts { roof_id: None, size_mm, .. } if *size_mm == 200.0));
        assert_eq!(add_roof(&p, &json!({"outline": "top_footprint", "level": "Ground Floor"}), &level).unwrap_err().code, "bad_args");
    }

    #[test]
    fn types_set_apply_and_refuse_a_door_sill() {
        let mut p = bungalow();
        let plan = types(&p, &json!({"action": "set", "kind": "door", "width_mm": 900, "height_mm": 2100, "description": "Panel door"})).unwrap();
        let Command::UpsertOpeningSpec { spec } = &plan.commands[0] else { panic!() };
        assert_eq!((spec.mark.as_str(), spec.style), ("", OpeningStyle::SwingSingle));
        assert_eq!(types(&p, &json!({"action": "set", "kind": "door", "width_mm": 900, "height_mm": 2100, "sill_mm": 0})).unwrap_err().code, "bad_args");
        p.opening_types.push(OpeningSpec { id: "t1".into(), mark: "D1".into(), ..(**spec).clone() });
        let door = p.elements.iter().find_map(|e| match e {
            Element::Opening(o) if o.opening_type == OpeningType::Door => Some(o.id.clone()),
            _ => None,
        });
        let plan = types(&p, &json!({"action": "apply", "type": "d1", "openings": [door.unwrap()]})).unwrap();
        assert!(matches!(&plan.commands[0], Command::UpdateElement { element: Element::Opening(o) } if o.type_id.as_deref() == Some("t1") && o.width_mm == 900.0));
        assert_eq!(types(&p, &json!({"action": "delete", "types": ["W9"]})).unwrap_err().code, "not_found");
        assert!(matches!(types(&p, &json!({"action": "assign"})).unwrap().commands[0], Command::AssignOpeningTypes { level_id: None }));
    }
}
