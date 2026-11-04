//! Turns the model into drawing primitives in model space (mm, y north).
//! The SVG sheet and the DXF writer both consume the same primitives, so a
//! door swing or a dimension is computed once.
//!
//! Annotation sizes (dimension text, ticks, room labels) are given on paper
//! in mm and multiplied by the scale denominator, so they print at the
//! intended size on a 1:N sheet and have a sensible height in a 1:1 DXF.
//!
//! Opening conventions used here (the model does not define them further):
//! - `flip_side == false`: the leaf swings to the left of the wall direction
//!   (start to end), that is toward the counter-clockwise normal.
//! - `flip_hinge == false`: the hinge is on the jamb nearer to the wall start.
//!
//! Slabs, railings and stairs follow docs/CONTRACT.md "Slabs and railings":
//! slab outlines with a light tint, voids crossed and marked OPEN TO BELOW,
//! railings drawn from `Derived::railings`, and a flight of n risers as n - 1
//! treads (DECISIONS D44), dashed with a DN arrow on the level it arrives at.

use guhit_model::{
    ColumnShape, Derived, DisplayUnit, Element, Id, LayerKey, Level, Opening, OpeningStyle,
    OpeningType, Project, RailingRun, RailingRunKind, RailingStyle, Slab, SlabKind, Stair, Wall,
};

use crate::geom::*;
use crate::text::{clean, est_width, format_length};
use crate::ExportError;

/// Drawing category. Maps to an SVG group and a DXF layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cat {
    /// Floor, balcony, deck and void outlines, the void cross and its label.
    Slab,
    Furn,
    /// Site walls: the lot fence and boundary walls (`WallRole::Site`).
    Site,
    /// A stair seen from the level it arrives at: its outline and treads,
    /// dashed. Shares the stairs layer in DXF.
    StrsBelow,
    Strs,
    /// Guard rails and handrails (`Derived::railings`).
    Rail,
    Wall,
    Cols,
    Door,
    Glaz,
    Dims,
    Text,
    Area,
}

impl Cat {
    /// Back to front draw order. Railings draw over both stair drawings.
    pub const ALL: [Cat; 13] = [
        Cat::Slab,
        Cat::Furn,
        Cat::Site,
        Cat::StrsBelow,
        Cat::Strs,
        Cat::Rail,
        Cat::Wall,
        Cat::Cols,
        Cat::Door,
        Cat::Glaz,
        Cat::Dims,
        Cat::Text,
        Cat::Area,
    ];

    pub fn dxf_layer(self) -> &'static str {
        match self {
            Cat::Wall => "A-WALL",
            Cat::Site => "L-SITE",
            Cat::Door => "A-DOOR",
            Cat::Glaz => "A-GLAZ",
            Cat::Cols => "A-COLS",
            Cat::Strs | Cat::StrsBelow => "A-FLOR-STRS",
            Cat::Slab => "A-FLOR-OTLN",
            Cat::Rail => "A-FLOR-HRAL",
            Cat::Furn => "A-FURN",
            Cat::Area => "A-AREA",
            Cat::Dims => "A-ANNO-DIMS",
            Cat::Text => "A-ANNO-TEXT",
        }
    }

    pub fn svg_id(self) -> &'static str {
        match self {
            Cat::Wall => "walls",
            Cat::Site => "site-walls",
            Cat::Door => "doors",
            Cat::Glaz => "windows",
            Cat::Cols => "columns",
            Cat::Strs => "stairs",
            Cat::StrsBelow => "stairs-below",
            Cat::Slab => "slabs",
            Cat::Rail => "railings",
            Cat::Furn => "assets",
            Cat::Area => "room-labels",
            Cat::Dims => "dimensions",
            Cat::Text => "annotations",
        }
    }
}

/// Line weight class. Widths in paper mm live in the sheet module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pen {
    Heavy,
    Medium,
    Light,
    Fine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fill {
    None,
    /// Solid ink, used for wall poche, columns and arrow heads.
    Ink,
    /// A light wash of ink, its opacity in percent: slabs and parapets.
    Tint(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HAlign {
    Start,
    Middle,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    Line {
        a: V,
        b: V,
    },
    Poly {
        pts: Vec<V>,
        closed: bool,
        fill: Fill,
    },
    /// Counter-clockwise from `start_deg` to `end_deg`.
    Arc {
        c: V,
        r: f64,
        start_deg: f64,
        end_deg: f64,
    },
    Circle {
        c: V,
        r: f64,
        fill: Fill,
    },
    /// `pos` is the baseline anchor. `height` is the font size in model mm.
    Text {
        pos: V,
        height: f64,
        rot_deg: f64,
        align: HAlign,
        text: String,
        bold: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub cat: Cat,
    pub pen: Pen,
    pub prim: Prim,
}

/// Paper sizes of annotation parts, in mm on the printed sheet.
pub mod paper {
    pub const DIM_TEXT: f64 = 2.5;
    pub const DIM_TEXT_GAP: f64 = 0.9;
    pub const DIM_TICK: f64 = 2.4;
    pub const DIM_EXT_GAP: f64 = 1.5;
    pub const DIM_EXT_OVERSHOOT: f64 = 1.5;
    pub const DIM_LINE_OVERSHOOT: f64 = 1.2;
    pub const ROOM_NAME: f64 = 3.0;
    pub const ROOM_AREA: f64 = 2.3;
    pub const ROOM_MIN: f64 = 1.6;
    pub const ASSET_LABEL: f64 = 1.7;
    pub const STAIR_LABEL: f64 = 2.0;
    pub const STAIR_ARROW: f64 = 2.2;
    pub const VOID_LABEL: f64 = 2.0;
    /// Dash and gap of a hidden line (a stair from below), the pattern of
    /// the DXF `GUHIT_DASHED` linetype.
    pub const HIDDEN_DASH: [f64; 2] = [2.2, 1.1];
}

pub struct PlanOptions {
    /// Scale denominator N of 1:N. Sizes annotation parts.
    pub scale: f64,
    pub show_dimensions: bool,
    pub show_room_labels: bool,
    pub show_assets: bool,
    /// False for DXF: text like "m2" must stay plain ASCII.
    pub unicode: bool,
    /// Leave out objects that a service sheet draws as symbols (lights,
    /// switches, outlets, aircon units). The plan draws them as outlines.
    pub skip_devices: bool,
}

/// The level to export: the requested one, or the first level.
pub fn resolve_level<'a>(
    project: &'a Project,
    level_id: Option<&Id>,
) -> Result<&'a Level, ExportError> {
    match level_id {
        Some(id) => project
            .levels
            .iter()
            .find(|l| &l.id == id)
            .ok_or_else(|| ExportError::Failed(format!("level {id} does not exist"))),
        None => project
            .levels
            .first()
            .ok_or_else(|| ExportError::Empty("the project has no levels".into())),
    }
}

/// A layer missing from the project (an older file) counts as visible.
pub(crate) fn layer_visible(project: &Project, key: LayerKey) -> bool {
    project
        .layers
        .iter()
        .find(|l| l.key == key)
        .map(|l| l.visible)
        .unwrap_or(true)
}

/// The error for a level with nothing visible to draw.
pub fn empty_level(level: &Level) -> ExportError {
    ExportError::Empty(format!(
        "level \"{}\" has no visible elements to draw",
        clean(&level.name)
    ))
}

/// Build every primitive of one level. Returns `ExportError::Empty` when the
/// level has nothing visible to draw.
pub fn build_plan(
    project: &Project,
    derived: &Derived,
    level: &Level,
    opts: &PlanOptions,
) -> Result<Vec<Item>, ExportError> {
    let out = build_items(project, derived, level, opts);
    if out.is_empty() {
        return Err(empty_level(level));
    }
    Ok(out)
}

/// Same as `build_plan`, but an empty level gives an empty list. Pipes are
/// drawn from their own list (`crate::pipes`), so a level holding only pipes
/// is still a drawing.
pub fn build_items(
    project: &Project,
    derived: &Derived,
    level: &Level,
    opts: &PlanOptions,
) -> Vec<Item> {
    let n = if opts.scale.is_finite() && opts.scale > 0.0 {
        opts.scale
    } else {
        100.0
    };
    let mut out: Vec<Item> = Vec::new();
    let on = |key: LayerKey| layer_visible(project, key);

    let walls: Vec<&Wall> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) if w.level_id == level.id => Some(w),
            _ => None,
        })
        .collect();

    let openings_on = on(LayerKey::Openings);
    let openings_of = |wall: &Wall| -> Vec<&Opening> {
        if !openings_on {
            return vec![];
        }
        project
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Opening(o) if o.wall_id == wall.id => Some(o),
                _ => None,
            })
            .collect()
    };

    // Room labels are built first so asset labels can keep clear of them.
    // They are appended last and draw on top.
    let mut room_items: Vec<Item> = Vec::new();
    if opts.show_room_labels && on(LayerKey::Rooms) {
        for e in &project.elements {
            if let Element::Room(r) = e {
                if r.level_id != level.id {
                    continue;
                }
                let geo = derived.rooms.iter().find(|g| g.room_id == r.id);
                let (at, area, avail) = match geo {
                    Some(g) => {
                        let mut b = Bounds::empty();
                        for p in &g.polygon {
                            b.add(p.into());
                        }
                        let lp = V::from(g.label_point);
                        let at = if lp.is_finite() { lp } else { r.seed.into() };
                        (at, Some(g.area_mm2), b.width() * 0.9)
                    }
                    None => (V::from(r.seed), None, f64::INFINITY),
                };
                room_label_items(&r.name, at, area, avail, n, opts.unicode, &mut room_items);
            }
        }
    }

    let label_boxes: Vec<Bounds> = room_items
        .iter()
        .map(|i| {
            // A little clearance around each label.
            let mut b = items_bounds(std::slice::from_ref(i));
            let pad = 1.5 * n;
            b.min = b.min - v(pad, pad);
            b.max = b.max + v(pad, pad);
            b
        })
        .collect();

    // Assets first so walls and symbols draw over them. Each object follows
    // its own layer: lighting and electrical objects are on `electrical`,
    // aircon units on `aircon`, the rest on `assets`.
    if opts.show_assets {
        for e in &project.elements {
            if let Element::Asset(a) = e {
                if a.level_id != level.id || !on(crate::services::asset_layer(a)) {
                    continue;
                }
                if opts.skip_devices && crate::services::symbol_of(a).is_some() {
                    continue;
                }
                asset_items(a, n, &label_boxes, &mut out);
            }
        }
    }

    if on(LayerKey::Slabs) {
        for e in &project.elements {
            if let Element::Slab(s) = e {
                if s.level_id == level.id {
                    slab_items(s, n, &mut out);
                }
            }
        }
    }

    if on(LayerKey::Stairs) {
        for e in &project.elements {
            if let Element::Stair(s) = e {
                if s.level_id == level.id {
                    stair_items(s, n, &mut out);
                } else if stair_arrival(project, s).is_some_and(|l| l.id == level.id) {
                    stair_from_below_items(s, n, &mut out);
                }
            }
        }
    }

    // Slab rails follow the slabs layer, stair rails the stairs layer.
    for run in derived.railings.iter().filter(|r| r.level_id == level.id) {
        let key = match run.kind {
            RailingRunKind::SlabEdge => LayerKey::Slabs,
            RailingRunKind::StairSide | RailingRunKind::StairWell => LayerKey::Stairs,
        };
        if on(key) {
            railing_items(run, &mut out);
        }
    }

    // Building walls follow the walls layer, site walls (fences, boundary
    // walls) the site layer.
    let drawn: Vec<&Wall> = walls.iter().copied().filter(|w| on(w.layer())).collect();
    if !drawn.is_empty() {
        let walls = &drawn;
        let outlines: Vec<Vec<V>> = walls
            .iter()
            .map(|wall| {
                derived
                    .walls
                    .iter()
                    .find(|g| g.wall_id == wall.id)
                    .map(|g| g.outline.iter().map(V::from).collect::<Vec<V>>())
                    .filter(|o| o.len() >= 3 && o.iter().all(|p| p.is_finite()))
                    .unwrap_or_else(|| fallback_outline(wall))
            })
            .collect();
        for (wall, outline) in walls.iter().zip(&outlines) {
            wall_items(wall, outline, &openings_of(wall), &mut out);
        }
        // Corners close among walls of one kind: a fence never patches a
        // corner of the house.
        for site in [false, true] {
            let (ws, os): (Vec<&Wall>, Vec<Vec<V>>) = walls
                .iter()
                .zip(&outlines)
                .filter(|(w, _)| w.is_site() == site)
                .map(|(w, o)| (*w, o.clone()))
                .unzip();
            corner_patches(&ws, &os, wall_cat(site), &mut out);
        }
    }

    if on(LayerKey::Columns) {
        for e in &project.elements {
            if let Element::Column(c) = e {
                if c.level_id != level.id {
                    continue;
                }
                let center = V::from(c.center);
                match c.shape {
                    ColumnShape::Round => out.push(Item {
                        cat: Cat::Cols,
                        pen: Pen::Medium,
                        prim: Prim::Circle {
                            c: center,
                            r: c.width_mm.abs() / 2.0,
                            fill: Fill::Ink,
                        },
                    }),
                    ColumnShape::Rect => out.push(Item {
                        cat: Cat::Cols,
                        pen: Pen::Medium,
                        prim: Prim::Poly {
                            pts: rotated_rect(center, c.width_mm, c.depth_mm, c.rotation_deg),
                            closed: true,
                            fill: Fill::Ink,
                        },
                    }),
                }
            }
        }
    }

    if openings_on {
        for wall in &walls {
            for o in openings_of(wall) {
                opening_items(wall, o, &mut out);
            }
        }
    }

    if opts.show_dimensions && on(LayerKey::Dimensions) {
        let meters = project.settings.display_unit == DisplayUnit::M;
        for e in &project.elements {
            if let Element::Dimension(d) = e {
                if d.level_id == level.id {
                    dimension_items(
                        d.a.into(),
                        d.b.into(),
                        d.offset_mm,
                        d.text_override.as_deref(),
                        meters,
                        n,
                        &mut out,
                    );
                }
            }
        }
    }

    if on(LayerKey::Annotations) {
        for e in &project.elements {
            if let Element::Annotation(a) = e {
                if a.level_id != level.id {
                    continue;
                }
                let h = if a.size_mm.is_finite() && a.size_mm > 0.0 {
                    a.size_mm
                } else {
                    2.5 * n
                };
                let down = dir(a.rotation_deg - 90.0);
                for (i, line) in a.text.lines().enumerate() {
                    let t = clean(line);
                    if t.is_empty() {
                        continue;
                    }
                    out.push(Item {
                        cat: Cat::Text,
                        pen: Pen::Light,
                        prim: Prim::Text {
                            pos: V::from(a.position) + down * (i as f64 * h * 1.4),
                            height: h,
                            rot_deg: a.rotation_deg,
                            align: HAlign::Start,
                            text: t,
                            bold: false,
                        },
                    });
                }
            }
        }
    }

    out.append(&mut room_items);

    out.retain(item_is_finite);
    out
}

fn item_is_finite(item: &Item) -> bool {
    match &item.prim {
        Prim::Line { a, b } => a.is_finite() && b.is_finite(),
        Prim::Poly { pts, .. } => pts.len() >= 2 && pts.iter().all(|p| p.is_finite()),
        Prim::Arc {
            c,
            r,
            start_deg,
            end_deg,
        } => c.is_finite() && r.is_finite() && *r > 0.0 && start_deg.is_finite() && end_deg.is_finite(),
        Prim::Circle { c, r, .. } => c.is_finite() && r.is_finite() && *r > 0.0,
        Prim::Text {
            pos, height, text, ..
        } => pos.is_finite() && height.is_finite() && *height > 0.0 && !text.is_empty(),
    }
}

/// Rectangle centered on `c`, `w` along the rotated x axis, `d` along the
/// rotated y axis. Counter-clockwise.
pub fn rotated_rect(c: V, w: f64, d: f64, rot_deg: f64) -> Vec<V> {
    let ux = dir(rot_deg) * (w.abs() / 2.0);
    let uy = dir(rot_deg + 90.0) * (d.abs() / 2.0);
    vec![c - ux - uy, c + ux - uy, c + ux + uy, c - ux + uy]
}

// ---------------------------------------------------------------------- walls

/// Opening span along the wall axis, clamped to the wall.
fn opening_span(o: &Opening, len: f64) -> Option<(f64, f64)> {
    let half = o.width_mm.abs() / 2.0;
    let a = (o.offset_mm - half).max(0.0);
    let b = (o.offset_mm + half).min(len);
    if b - a > 1.0 {
        Some((a, b))
    } else {
        None
    }
}

/// Plain rectangle around the centerline, used when a wall has no derived
/// outline.
fn fallback_outline(wall: &Wall) -> Vec<V> {
    let s = V::from(wall.start);
    let e = V::from(wall.end);
    match (e - s).unit() {
        Some(u) => {
            let h = u.left() * (wall.thickness_mm.abs() / 2.0);
            vec![s - h, e - h, e + h, s + h]
        }
        None => vec![],
    }
}

/// Where two walls meet end to end at an angle and their outlines are not
/// mitred, the outside of the corner is left open. Fill it with the mitre
/// parallelogram so the corner reads as one solid mass. Does nothing when
/// the outlines already cover the corner.
/// The drawing category of a wall: `Wall`, or `Site` for a fence.
fn wall_cat(site: bool) -> Cat {
    if site {
        Cat::Site
    } else {
        Cat::Wall
    }
}

fn corner_patches(walls: &[&Wall], outlines: &[Vec<V>], cat: Cat, out: &mut Vec<Item>) {
    for i in 0..walls.len() {
        for j in (i + 1)..walls.len() {
            let ends_i = [(walls[i].start, walls[i].end), (walls[i].end, walls[i].start)];
            let ends_j = [(walls[j].start, walls[j].end), (walls[j].end, walls[j].start)];
            for (ji, far_i) in ends_i {
                for (jj, far_j) in ends_j {
                    let joint = V::from(ji);
                    if (joint - V::from(jj)).len() > 1.0 {
                        continue;
                    }
                    let (Some(u1), Some(u2)) =
                        ((V::from(far_i) - joint).unit(), (V::from(far_j) - joint).unit())
                    else {
                        continue;
                    };
                    let sin = (u1.x * u2.y - u1.y * u2.x).abs();
                    if sin < 0.2 {
                        continue;
                    }
                    let a = u1 * (walls[j].thickness_mm.abs() / (2.0 * sin));
                    let b = u2 * (walls[i].thickness_mm.abs() / (2.0 * sin));
                    // Just inside the outer corner of the mitre.
                    let probe = joint - (a + b) * 0.9;
                    if outlines.iter().any(|o| point_in_polygon(probe, o)) {
                        continue;
                    }
                    out.push(Item {
                        cat,
                        pen: Pen::Heavy,
                        prim: Prim::Poly {
                            pts: vec![joint - a - b, joint + a - b, joint + a + b, joint - a + b],
                            closed: true,
                            fill: Fill::Ink,
                        },
                    });
                }
            }
        }
    }
}

fn wall_items(wall: &Wall, outline: &[V], openings: &[&Opening], out: &mut Vec<Item>) {
    let s = V::from(wall.start);
    let e = V::from(wall.end);
    let Some(u) = (e - s).unit() else { return };
    let len = (e - s).len();
    if outline.len() < 3 {
        return;
    }

    let mut spans: Vec<(f64, f64)> = openings
        .iter()
        .filter_map(|o| opening_span(o, len))
        .collect();
    spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for sp in spans {
        match merged.last_mut() {
            Some(last) if sp.0 <= last.1 => last.1 = last.1.max(sp.1),
            _ => merged.push(sp),
        }
    }

    // Solid pieces are what is left of the outline between the openings.
    let mut pieces: Vec<Vec<V>> = Vec::new();
    let mut from: Option<f64> = None;
    for (a, b) in merged.iter().copied().chain(std::iter::once((f64::NAN, f64::NAN))) {
        let mut piece = outline.to_vec();
        if let Some(t0) = from {
            piece = clip_half_plane(&piece, s, u, t0, true);
        }
        if a.is_finite() {
            piece = clip_half_plane(&piece, s, u, a, false);
        }
        if piece.len() >= 3 && polygon_area(&piece) > 1.0 {
            pieces.push(piece);
        }
        from = Some(b);
    }

    for pts in pieces {
        out.push(Item {
            cat: wall_cat(wall.is_site()),
            pen: Pen::Heavy,
            prim: Prim::Poly {
                pts,
                closed: true,
                fill: Fill::Ink,
            },
        });
    }
}

// ------------------------------------------------------------------- openings

fn opening_items(wall: &Wall, o: &Opening, out: &mut Vec<Item>) {
    let s = V::from(wall.start);
    let e = V::from(wall.end);
    let Some(u) = (e - s).unit() else { return };
    let len = (e - s).len();
    let Some((t0, t1)) = opening_span(o, len) else {
        return;
    };
    let w = t1 - t0;
    let c = s + u * ((t0 + t1) / 2.0);
    let t = wall.thickness_mm.abs();
    let side = if o.flip_side { -1.0 } else { 1.0 };
    let nrm = u.left() * side;

    match o.opening_type {
        OpeningType::Window => {
            let j0 = c - u * (w / 2.0);
            let j1 = c + u * (w / 2.0);
            let half = u.left() * (t / 2.0);
            for face in [half, -half] {
                out.push(Item {
                    cat: Cat::Glaz,
                    pen: Pen::Medium,
                    prim: Prim::Line {
                        a: j0 + face,
                        b: j1 + face,
                    },
                });
            }
            if o.style == OpeningStyle::Sliding {
                // Two sashes that overlap at the middle.
                let g = u.left() * (t * 0.11);
                let ov = u * (w * 0.06);
                out.push(line(Cat::Glaz, Pen::Light, j0 + g, c + ov + g));
                out.push(line(Cat::Glaz, Pen::Light, c - ov - g, j1 - g));
            } else {
                out.push(line(Cat::Glaz, Pen::Light, j0, j1));
            }
        }
        OpeningType::Door => match o.style {
            OpeningStyle::SwingDouble => {
                door_leaf(c - u * (w / 2.0), u, nrm, w / 2.0, t, out);
                door_leaf(c + u * (w / 2.0), -u, nrm, w / 2.0, t, out);
            }
            OpeningStyle::Sliding => {
                let pt = (t * 0.3).min(45.0);
                let ov = w * 0.05;
                let panel = |a: f64, b: f64, n0: f64, n1: f64| Item {
                    cat: Cat::Door,
                    pen: Pen::Medium,
                    prim: Prim::Poly {
                        pts: vec![
                            c + u * a + nrm * n0,
                            c + u * b + nrm * n0,
                            c + u * b + nrm * n1,
                            c + u * a + nrm * n1,
                        ],
                        closed: true,
                        fill: Fill::None,
                    },
                };
                out.push(panel(-w / 2.0, ov, 0.0, pt));
                out.push(panel(-ov, w / 2.0, -pt, 0.0));
            }
            OpeningStyle::Fixed => {
                // Cased opening: thin lines on both faces.
                let j0 = c - u * (w / 2.0);
                let j1 = c + u * (w / 2.0);
                let half = u.left() * (t / 2.0);
                out.push(line(Cat::Door, Pen::Fine, j0 + half, j1 + half));
                out.push(line(Cat::Door, Pen::Fine, j0 - half, j1 - half));
            }
            _ => {
                let (hinge, along) = if o.flip_hinge {
                    (c + u * (w / 2.0), -u)
                } else {
                    (c - u * (w / 2.0), u)
                };
                door_leaf(hinge, along, nrm, w, t, out);
            }
        },
    }
}

fn line(cat: Cat, pen: Pen, a: V, b: V) -> Item {
    Item {
        cat,
        pen,
        prim: Prim::Line { a, b },
    }
}

/// One swing leaf, drawn open at 90 degrees, with its quarter arc.
/// `hinge` is the jamb point on the wall centerline, `along` points from the
/// hinge jamb to the other jamb, `nrm` is the side the leaf swings to.
fn door_leaf(hinge: V, along: V, nrm: V, w: f64, wall_t: f64, out: &mut Vec<Item>) {
    let hp = hinge + nrm * (wall_t / 2.0);
    let tip = hp + nrm * w;
    let lt = 40.0_f64.min(w * 0.08);
    out.push(Item {
        cat: Cat::Door,
        pen: Pen::Medium,
        prim: Prim::Poly {
            pts: vec![hp, tip, tip + along * lt, hp + along * lt],
            closed: true,
            fill: Fill::None,
        },
    });
    let a_closed = along.angle_deg();
    let a_open = nrm.angle_deg();
    let (start_deg, end_deg) = if (norm_deg(a_open - a_closed) - 90.0).abs() < 1.0 {
        (a_closed, a_open)
    } else {
        (a_open, a_closed)
    };
    out.push(Item {
        cat: Cat::Door,
        pen: Pen::Light,
        prim: Prim::Arc {
            c: hp,
            r: w,
            start_deg: norm_deg(start_deg),
            end_deg: norm_deg(end_deg),
        },
    });
}

// --------------------------------------------------------------------- stairs

/// A level counts as the top of a flight when its floor is this close to it.
pub const STAIR_TOP_MATCH_MM: f64 = 300.0;

/// The level a stair arrives at: of the levels above the stair's own, the
/// one whose elevation is closest to the stair's top (its level's elevation
/// plus height), within 300 mm. The 3D view finds it the same way
/// (`src/viewer3d/walk/stairs.ts`).
pub fn stair_arrival<'a>(project: &'a Project, s: &Stair) -> Option<&'a Level> {
    let own = project.levels.iter().find(|l| l.id == s.level_id)?;
    let top = own.elevation_mm + own.height_mm;
    let mut best: Option<(&Level, f64)> = None;
    for l in &project.levels {
        if l.id == own.id || l.elevation_mm <= own.elevation_mm {
            continue;
        }
        let d = (l.elevation_mm - top).abs();
        if d <= STAIR_TOP_MATCH_MM && best.is_none_or(|(_, b)| d < b) {
            best = Some((l, d));
        }
    }
    best.map(|(l, _)| l)
}

/// A flight's plan frame: first riser center, unit vector up the flight,
/// unit vector across it, half width and run. None for a degenerate flight.
fn stair_frame(s: &Stair) -> Option<(V, V, V, f64, f64)> {
    let hw = s.width_mm.abs() / 2.0;
    let run = s.run_mm.abs();
    if !(hw >= 1.0 && run >= 1.0) {
        return None;
    }
    Some((V::from(s.origin), dir(s.rotation_deg + 90.0), dir(s.rotation_deg), hw, run))
}

/// The flight outline and its tread lines (DECISIONS D44): n risers make
/// n - 1 treads of run / (n - 1), so the lines sit at k * run / (n - 1) for
/// k = 1 .. n - 2 and the last tread ends at the top of the flight.
fn flight_items(s: &Stair, cat: Cat, out: &mut Vec<Item>) {
    let Some((o, f, r, hw, run)) = stair_frame(s) else { return };
    out.push(Item {
        cat,
        pen: Pen::Medium,
        prim: Prim::Poly {
            pts: vec![o - r * hw, o + r * hw, o + r * hw + f * run, o - r * hw + f * run],
            closed: true,
            fill: Fill::None,
        },
    });
    let treads = crate::model3d::tread_count(s.riser_count);
    for k in 1..treads {
        let d = f * (run * k as f64 / treads as f64);
        out.push(line(cat, Pen::Light, o - r * hw + d, o + r * hw + d));
    }
}

/// Direction of travel from `from` to `to`: a dot where it starts, a shaft
/// and a head. Always solid, on the stairs layer.
fn travel_arrow(from: V, to: V, n: f64, out: &mut Vec<Item>) {
    let Some(u) = (to - from).unit() else { return };
    let across = u.left();
    out.push(line(Cat::Strs, Pen::Light, from, to));
    out.push(Item {
        cat: Cat::Strs,
        pen: Pen::Light,
        prim: Prim::Circle {
            c: from,
            r: 0.45 * n,
            fill: Fill::Ink,
        },
    });
    let hl = (paper::STAIR_ARROW * n).min((to - from).len() * 0.45);
    out.push(Item {
        cat: Cat::Strs,
        pen: Pen::Light,
        prim: Prim::Poly {
            pts: vec![to, to - u * hl + across * (hl * 0.32), to - u * hl - across * (hl * 0.32)],
            closed: true,
            fill: Fill::Ink,
        },
    });
}

/// "UP" or "DN" centered on `center`, readable along the flight.
fn stair_label(s: &Stair, center: V, th: f64, text: &str, out: &mut Vec<Item>) {
    let rot = readable_deg(s.rotation_deg);
    let up = dir(rot + 90.0);
    out.push(Item {
        cat: Cat::Strs,
        pen: Pen::Light,
        prim: Prim::Text {
            pos: center - up * (th * 0.36),
            height: th,
            rot_deg: rot,
            align: HAlign::Middle,
            text: text.into(),
            bold: false,
        },
    });
}

/// Where a travel arrow starts or ends inside the flight: half a tread in,
/// at most a fifth of the run.
fn arrow_inset(s: &Stair, run: f64) -> f64 {
    let going = run / crate::model3d::tread_count(s.riser_count) as f64;
    (going * 0.5).min(run * 0.2)
}

/// A stair on its own level: the flight, an arrow from the first tread to
/// the top, "UP" just before the first riser.
fn stair_items(s: &Stair, n: f64, out: &mut Vec<Item>) {
    let Some((o, f, _, _, run)) = stair_frame(s) else { return };
    flight_items(s, Cat::Strs, out);
    let inset = arrow_inset(s, run);
    travel_arrow(o + f * inset, o + f * (run - inset), n, out);
    let th = paper::STAIR_LABEL * n;
    stair_label(s, o - f * (th * 1.1), th, "UP", out);
}

/// A stair seen from the level it arrives at: the flight dashed, an arrow
/// from the top end down the flight, "DN" just past the top.
fn stair_from_below_items(s: &Stair, n: f64, out: &mut Vec<Item>) {
    let Some((o, f, _, _, run)) = stair_frame(s) else { return };
    flight_items(s, Cat::StrsBelow, out);
    let inset = arrow_inset(s, run);
    travel_arrow(o + f * (run - inset), o + f * inset, n, out);
    let th = paper::STAIR_LABEL * n;
    stair_label(s, o + f * (run + th * 1.1), th, "DN", out);
}

// ---------------------------------------------------------------------- slabs

/// Tints in percent of ink: a floor slab, a balcony or deck a little
/// stronger, a solid parapet stronger still.
const FLOOR_TINT: u8 = 5;
const OPEN_TINT: u8 = 10;
const PARAPET_TINT: u8 = 22;

/// A slab's outline, tinted by kind. A void is its outline, a cross and
/// "OPEN TO BELOW".
fn slab_items(s: &Slab, n: f64, out: &mut Vec<Item>) {
    let poly = crate::model3d::slab_outline(s);
    if poly.len() < 3 {
        return;
    }
    let fill = match s.kind {
        SlabKind::Floor => Fill::Tint(FLOOR_TINT),
        SlabKind::Balcony | SlabKind::Deck => Fill::Tint(OPEN_TINT),
        SlabKind::Void => Fill::None,
    };
    out.push(Item {
        cat: Cat::Slab,
        pen: Pen::Light,
        prim: Prim::Poly {
            pts: poly.clone(),
            closed: true,
            fill,
        },
    });
    if s.kind == SlabKind::Void {
        void_marks(&poly, n, out);
    }
}

/// The cross of a void: the two diagonals of a four-corner outline,
/// otherwise the diagonals of its bounding box, clipped to the outline and
/// kept clear of the label in the middle.
fn void_marks(poly: &[V], n: f64, out: &mut Vec<Item>) {
    let b = bounds_of(poly);
    let diagonals = if poly.len() == 4 {
        [(poly[0], poly[2]), (poly[1], poly[3])]
    } else {
        [(b.min, b.max), (v(b.min.x, b.max.y), v(b.max.x, b.min.y))]
    };
    let c = centroid(poly);
    let at = if point_in_polygon(c, poly) { c } else { b.center() };
    let text = "OPEN TO BELOW";
    let mut th = paper::VOID_LABEL * n;
    let w = est_width(text, th, false);
    let avail = b.width() * 0.85;
    if w > avail {
        th *= (avail / w).max(paper::ROOM_MIN / paper::VOID_LABEL);
    }
    let label = Item {
        cat: Cat::Slab,
        pen: Pen::Light,
        prim: Prim::Text {
            pos: at - v(0.0, th * 0.36),
            height: th,
            rot_deg: 0.0,
            align: HAlign::Middle,
            text: text.into(),
            bold: false,
        },
    };
    let mut clear = items_bounds(std::slice::from_ref(&label));
    let pad = 0.8 * n;
    clear.min = clear.min - v(pad, pad);
    clear.max = clear.max + v(pad, pad);
    for (a, z) in diagonals {
        for (p, q) in clip_segment(a, z, poly) {
            for (p, q) in outside_bounds(p, q, &clear) {
                out.push(line(Cat::Slab, Pen::Fine, p, q));
            }
        }
    }
    out.push(label);
}

// ------------------------------------------------------------------ railings

/// Half the gap between the two lines of a bars or glass rail, and half the
/// band of a solid parapet, mm.
const RAIL_HALF_MM: f64 = 25.0;
const PARAPET_HALF_MM: f64 = 50.0;

/// One rail along its run: two thin lines 50 mm apart for bars and glass, a
/// 100 mm tinted band for a solid parapet. A run all the way round gives
/// closed rings; a parapet ring is one outline with the inner ring as a hole.
fn railing_items(run: &RailingRun, out: &mut Vec<Item>) {
    let (pts, _, closed) = crate::model3d::railing_path(run);
    if pts.len() < 2 {
        return;
    }
    let poly = |pts: Vec<V>, closed: bool, fill: Fill| Item {
        cat: Cat::Rail,
        pen: Pen::Light,
        prim: Prim::Poly { pts, closed, fill },
    };
    match run.style {
        RailingStyle::Bars | RailingStyle::Glass => {
            for d in [RAIL_HALF_MM, -RAIL_HALF_MM] {
                out.push(poly(offset_path(&pts, d, closed), closed, Fill::None));
            }
        }
        RailingStyle::Solid => {
            let left = offset_path(&pts, PARAPET_HALF_MM, closed);
            let right = offset_path(&pts, -PARAPET_HALF_MM, closed);
            let band: Vec<V> = if closed {
                let mut b = left.clone();
                b.push(left[0]);
                b.push(right[0]);
                b.extend(right.iter().skip(1).rev());
                b.push(right[0]);
                b
            } else {
                left.into_iter().chain(right.into_iter().rev()).collect()
            };
            out.push(poly(band, true, Fill::Tint(PARAPET_TINT)));
        }
    }
}

// --------------------------------------------------------------------- assets

pub(crate) fn asset_items(a: &guhit_model::Asset, n: f64, avoid: &[Bounds], out: &mut Vec<Item>) {
    let c = V::from(a.position);
    let (w, d) = (a.width_mm.abs(), a.depth_mm.abs());
    if w < 1.0 || d < 1.0 {
        return;
    }
    let pts = rotated_rect(c, w, d, a.rotation_deg);
    out.push(Item {
        cat: Cat::Furn,
        pen: Pen::Light,
        prim: Prim::Poly {
            pts: pts.clone(),
            closed: true,
            fill: Fill::None,
        },
    });
    // Label along the longer side when it fits, otherwise one diagonal.
    let name = clean(&a.name);
    let th = paper::ASSET_LABEL * n;
    let (long, along_deg) = if w >= d {
        (w, a.rotation_deg)
    } else {
        (d, a.rotation_deg + 90.0)
    };
    let short = w.min(d);
    let fits = !name.is_empty() && est_width(&name, th, false) <= long * 0.9 && th * 1.6 <= short;
    let rot = readable_deg(along_deg);
    let up = dir(rot + 90.0);
    let label = Item {
        cat: Cat::Furn,
        pen: Pen::Light,
        prim: Prim::Text {
            pos: c - up * (th * 0.36),
            height: th,
            rot_deg: rot,
            align: HAlign::Middle,
            text: name,
            bold: false,
        },
    };
    let label_box = items_bounds(std::slice::from_ref(&label));
    if fits && !avoid.iter().any(|b| b.intersects(&label_box)) {
        out.push(label);
    } else {
        out.push(line(Cat::Furn, Pen::Fine, pts[0], pts[2]));
    }
}

// ----------------------------------------------------------------- dimensions

fn dimension_items(
    a: V,
    b: V,
    offset: f64,
    text_override: Option<&str>,
    meters: bool,
    n: f64,
    out: &mut Vec<Item>,
) {
    let Some(u) = (b - a).unit() else { return };
    let length = (b - a).len();
    let nrm = u.left();
    let offset = if offset.is_finite() { offset } else { 0.0 };
    let a2 = a + nrm * offset;
    let b2 = b + nrm * offset;
    // Extension lines run from near the measured point to past the dim line.
    let sgn = if offset >= 0.0 { 1.0 } else { -1.0 };
    let gap = (paper::DIM_EXT_GAP * n).min(offset.abs() * 0.5);
    let over = paper::DIM_EXT_OVERSHOOT * n;
    for (p, p2) in [(a, a2), (b, b2)] {
        out.push(line(
            Cat::Dims,
            Pen::Light,
            p + nrm * (sgn * gap),
            p2 + nrm * (sgn * over),
        ));
    }
    let lo = paper::DIM_LINE_OVERSHOOT * n;
    out.push(line(Cat::Dims, Pen::Light, a2 - u * lo, b2 + u * lo));
    // 45 degree ticks, a little heavier than the lines.
    let tick = u.rotated(45.0) * (paper::DIM_TICK * n / 2.0);
    for p in [a2, b2] {
        out.push(line(Cat::Dims, Pen::Medium, p - tick, p + tick));
    }
    // Text centered above the line, never upside down.
    let label = match text_override.map(clean) {
        Some(t) if !t.is_empty() => t,
        _ => format_length(length, meters),
    };
    let rot = readable_deg(u.angle_deg());
    let up = dir(rot + 90.0);
    let mid = (a2 + b2) * 0.5;
    out.push(Item {
        cat: Cat::Dims,
        pen: Pen::Light,
        prim: Prim::Text {
            pos: mid + up * (paper::DIM_TEXT_GAP * n),
            height: paper::DIM_TEXT * n,
            rot_deg: rot,
            align: HAlign::Middle,
            text: label,
            bold: false,
        },
    });
}

// ---------------------------------------------------------------- room labels

fn room_label_items(
    name: &str,
    at: V,
    area_mm2: Option<f64>,
    avail_width: f64,
    n: f64,
    unicode: bool,
    out: &mut Vec<Item>,
) {
    let name = clean(name);
    let mut name_h = paper::ROOM_NAME * n;
    let mut area_h = paper::ROOM_AREA * n;
    if !name.is_empty() && avail_width.is_finite() && avail_width > 0.0 {
        let w = est_width(&name, name_h, true);
        if w > avail_width {
            let k = (avail_width / w).max(paper::ROOM_MIN / paper::ROOM_NAME);
            name_h *= k;
            area_h *= k;
        }
    }
    let area = area_mm2.filter(|a| a.is_finite() && *a > 0.0).map(|a| {
        let unit = if unicode { "m\u{00b2}" } else { "m2" };
        format!("{:.2} {unit}", a / 1.0e6)
    });
    // Two lines centered on the label point.
    let gap = name_h * 0.45;
    let (name_base, area_base) = match (&area, name.is_empty()) {
        (Some(_), false) => (at + v(0.0, gap / 2.0), at - v(0.0, gap / 2.0 + area_h * 0.74)),
        (None, _) => (at - v(0.0, name_h * 0.36), at),
        (Some(_), true) => (at, at - v(0.0, area_h * 0.36)),
    };
    if !name.is_empty() {
        out.push(Item {
            cat: Cat::Area,
            pen: Pen::Light,
            prim: Prim::Text {
                pos: name_base,
                height: name_h,
                rot_deg: 0.0,
                align: HAlign::Middle,
                text: name,
                bold: true,
            },
        });
    }
    if let Some(area) = area {
        out.push(Item {
            cat: Cat::Area,
            pen: Pen::Light,
            prim: Prim::Text {
                pos: area_base,
                height: area_h,
                rot_deg: 0.0,
                align: HAlign::Middle,
                text: area,
                bold: false,
            },
        });
    }
}

// --------------------------------------------------------------------- bounds

/// Number of straight segments used to bound an arc.
const ARC_STEPS: usize = 8;

pub fn items_bounds(items: &[Item]) -> Bounds {
    let mut b = Bounds::empty();
    for item in items {
        match &item.prim {
            Prim::Line { a, b: p } => {
                b.add(*a);
                b.add(*p);
            }
            Prim::Poly { pts, .. } => pts.iter().for_each(|p| b.add(*p)),
            Prim::Arc {
                c,
                r,
                start_deg,
                end_deg,
            } => {
                let sweep = norm_deg(end_deg - start_deg);
                for i in 0..=ARC_STEPS {
                    let a = start_deg + sweep * i as f64 / ARC_STEPS as f64;
                    b.add(*c + dir(a) * *r);
                }
            }
            Prim::Circle { c, r, .. } => {
                b.add(*c - v(*r, *r));
                b.add(*c + v(*r, *r));
            }
            Prim::Text {
                pos,
                height,
                rot_deg,
                align,
                text,
                bold,
            } => {
                let w = est_width(text, *height, *bold);
                let along = dir(*rot_deg);
                let up = dir(rot_deg + 90.0);
                let x0 = match align {
                    HAlign::Start => 0.0,
                    HAlign::Middle => -w / 2.0,
                };
                for (dx, dy) in [(x0, -0.25), (x0 + w, -0.25), (x0 + w, 0.85), (x0, 0.85)] {
                    b.add(*pos + along * dx + up * (dy * *height));
                }
            }
        }
    }
    b
}
