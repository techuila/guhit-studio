//! The model as planar faces for elevations and sections, built from
//! `model3d::build_scene` with the roofs and stairs of `Derived` when they
//! are there. World mm: x east, y north, z up (level elevation included).
//!
//! Every face is a planar polygon wound counter-clockwise seen from outside,
//! so its Newell normal points out of its solid. A solid that is not closed
//! (a fascia board) is marked so: its faces hide what is
//! behind them from either side.
//!
//! Furniture is left out (DECISIONS D54, plan risk "HLR"). Plants and cars
//! are `Figure`s that elevations draw as outlines; site objects (a gate, a
//! lamp post) are boxes. Pipes are coordination, not architecture, and are
//! left out too.

use guhit_model::{
    AssetCategory, ColumnShape, Derived, Element, Id, LayerKey, OpeningStyle, OpeningType, Project,
    RoofKind, SlabKind,
};

use crate::geom::*;
use crate::model3d::{self, LevelScene, WallSolid};
use crate::plan::layer_visible;

/// A point or vector in 3D, mm.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct P3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub fn p3(x: f64, y: f64, z: f64) -> P3 {
    P3 { x, y, z }
}

/// A plan point at height `z`.
pub fn lift(p: V, z: f64) -> P3 {
    p3(p.x, p.y, z)
}

impl std::ops::Add for P3 {
    type Output = P3;
    fn add(self, o: P3) -> P3 {
        p3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl std::ops::Sub for P3 {
    type Output = P3;
    fn sub(self, o: P3) -> P3 {
        p3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl std::ops::Mul<f64> for P3 {
    type Output = P3;
    fn mul(self, k: f64) -> P3 {
        p3(self.x * k, self.y * k, self.z * k)
    }
}

impl std::ops::Neg for P3 {
    type Output = P3;
    fn neg(self) -> P3 {
        p3(-self.x, -self.y, -self.z)
    }
}

impl P3 {
    pub fn dot(self, o: P3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: P3) -> P3 {
        p3(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    pub fn len(self) -> f64 {
        self.dot(self).sqrt()
    }

    pub fn unit(self) -> Option<P3> {
        let l = self.len();
        if l > 1e-12 && l.is_finite() {
            Some(self * (1.0 / l))
        } else {
            None
        }
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    pub fn plan(self) -> V {
        v(self.x, self.y)
    }
}

/// What a solid is, for its line weight, layer and whether it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    Wall,
    /// A lot fence or boundary wall.
    Site,
    Door,
    Window,
    /// Floor slabs, balconies, decks and roof decks.
    Slab,
    Column,
    Stair,
    Rail,
    Roof,
    /// A site object: a gate, a lamp post, a planter.
    Object,
}

#[derive(Debug, Clone)]
pub struct Solid {
    pub part: Part,
    pub element_id: Id,
    /// False for single faces (a fascia board): they hide from both
    /// sides and have no inside.
    pub closed: bool,
}

#[derive(Debug, Clone)]
pub struct Face {
    pub pts: Vec<P3>,
    pub solid: u32,
}

/// A line drawn on a surface: a window's sash and louvres, a door's panels.
#[derive(Debug, Clone, Copy)]
pub struct Decal {
    pub a: P3,
    pub b: P3,
    pub solid: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FigureKind {
    Tree,
    Pot,
    Car,
}

/// Entourage drawn as an outline: a plant or a car.
#[derive(Debug, Clone)]
pub struct Figure {
    pub kind: FigureKind,
    pub element_id: Id,
    /// Center of its footprint at its underside.
    pub base: P3,
    pub width: f64,
    pub depth: f64,
    pub height: f64,
    pub rotation_deg: f64,
}

/// A door or window, for its mark on elevations and sections.
#[derive(Debug, Clone)]
pub struct OpeningRef {
    pub opening_id: Id,
    pub kind: OpeningType,
    /// Center of the opening on the wall centerline, at mid height.
    pub center: P3,
    /// Plan unit vector to the left of the wall.
    pub normal: P3,
    /// Half the wall thickness.
    pub half_t: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub solids: Vec<Solid>,
    pub faces: Vec<Face>,
    pub decals: Vec<Decal>,
    pub figures: Vec<Figure>,
    pub openings: Vec<OpeningRef>,
    /// True when the model was too big and every solid became its box.
    pub boxed: bool,
}

/// Above this many triangles every solid becomes its bounding box (plan
/// risk "HLR": box forms over 200k triangles).
pub const BOX_FORM_TRIANGLES: usize = 200_000;

/// Roof deck thickness where a level is not covered by the one above
/// (DECISIONS D26, `src/viewer3d/scene/roofDeck.ts`).
pub const DECK_MM: f64 = 200.0;
/// Fascia board thickness, mm.
const FASCIA_T: f64 = 25.0;

impl Mesh {
    fn solid(&mut self, part: Part, element_id: &str, closed: bool) -> u32 {
        self.solids.push(Solid { part, element_id: element_id.to_string(), closed });
        (self.solids.len() - 1) as u32
    }

    fn face(&mut self, solid: u32, pts: Vec<P3>) {
        if pts.len() >= 3 && pts.iter().all(|p| p.is_finite()) {
            self.faces.push(Face { pts, solid });
        }
    }

    fn decal(&mut self, solid: u32, a: P3, b: P3) {
        if a.is_finite() && b.is_finite() && (b - a).len() > 0.5 {
            self.decals.push(Decal { a, b, solid });
        }
    }

    /// A closed prism over a plan polygon between two height functions.
    fn prism(&mut self, solid: u32, poly: &[V], bottom: &dyn Fn(V) -> f64, top: &dyn Fn(V) -> f64) {
        let poly = ensure_ccw(&clean_polygon(poly));
        if poly.len() < 3 {
            return;
        }
        self.face(solid, poly.iter().map(|p| lift(*p, top(*p))).collect());
        self.face(solid, poly.iter().rev().map(|p| lift(*p, bottom(*p))).collect());
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            self.face(solid, vec![lift(a, bottom(a)), lift(b, bottom(b)), lift(b, top(b)), lift(a, top(a))]);
        }
    }

    fn box_solid(&mut self, solid: u32, poly: &[V], z0: f64, z1: f64) {
        if z1 - z0 > 0.5 {
            self.prism(solid, poly, &|_| z0, &|_| z1);
        }
    }

    /// A flat slab with holes: caps from convex pieces of the outline less
    /// the holes, sides along the outline and along the hole edges inside it.
    fn holed_slab(&mut self, solid: u32, poly: &[V], holes: &[Vec<V>], z0: f64, z1: f64) {
        let poly = ensure_ccw(poly);
        let cut: Vec<&Vec<V>> = holes.iter().filter(|h| overlap_area(&poly, h) > 1.0).collect();
        if cut.is_empty() {
            self.box_solid(solid, &poly, z0, z1);
            return;
        }
        let mut pieces = triangles(&poly);
        for hole in &cut {
            for t in triangles(hole) {
                pieces = pieces.iter().flat_map(|p| subtract_convex(p, &t)).collect();
            }
        }
        for p in &pieces {
            let p = ensure_ccw(p);
            self.face(solid, p.iter().map(|q| lift(*q, z1)).collect());
            self.face(solid, p.iter().rev().map(|q| lift(*q, z0)).collect());
        }
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            self.face(solid, vec![lift(a, z0), lift(b, z0), lift(b, z1), lift(a, z1)]);
        }
        for hole in &cut {
            let hole = ensure_ccw(hole);
            for i in 0..hole.len() {
                let (a, b) = (hole[i], hole[(i + 1) % hole.len()]);
                // Only where slab is beside the edge: inside the outline and
                // in no other hole (a stair well under a void).
                let Some(u) = (b - a).unit() else { continue };
                let probe = (a + b) * 0.5 + v(u.y, -u.x);
                if point_in_polygon(probe, &poly) && !cut.iter().any(|h| point_in_polygon(probe, h)) {
                    // The hole's walls face into it.
                    self.face(solid, vec![lift(b, z0), lift(a, z0), lift(a, z1), lift(b, z1)]);
                }
            }
        }
    }

    /// Add a closed box solid: a prism over a plan polygon from `z0` to
    /// `z1`. For tests and synthetic scenes.
    pub fn push_box(&mut self, part: Part, id: &str, poly: &[V], z0: f64, z1: f64) {
        let s = self.solid(part, id, true);
        self.box_solid(s, poly, z0, z1);
    }

    pub fn triangle_count(&self) -> usize {
        self.faces.iter().map(|f| f.pts.len().saturating_sub(2)).sum()
    }
}

/// Build the mesh of the whole project.
pub fn build(project: &Project, derived: &Derived) -> Mesh {
    let mut m = build_full(project, derived);
    if m.triangle_count() > BOX_FORM_TRIANGLES {
        m = box_forms(&m);
    }
    m
}

fn build_full(project: &Project, derived: &Derived) -> Mesh {
    let scene = model3d::build_scene(project, derived);
    let on = |k: LayerKey| layer_visible(project, k);
    let mut m = Mesh::default();

    // The lowest floor stands on the finish grade: its slab runs down to
    // the grade line the elevations draw, so the house never floats.
    let lowest = project.levels.iter().map(|l| l.elevation_mm).fold(f64::INFINITY, f64::min);
    let plinth = grade_below(project).max(model3d::SLAB_THICKNESS_MM);
    for (li, ls) in scene.levels.iter().enumerate() {
        let e = ls.level.elevation_mm;
        let under = if e <= lowest + 1e-6 { plinth } else { model3d::SLAB_THICKNESS_MM };
        // Voids and the openings of the stairs that arrive here cut the
        // level's floors, as the 3D view cuts them.
        // A void keeps the slab under the walls (`model3d::void_clear`).
        let mut voids: Vec<Vec<V>> = ls.voids.iter().flat_map(|s| model3d::void_clear(&model3d::slab_outline(s), &ls.walls)).collect();
        voids.extend(stair_wells(derived, &ls.level.id));
        if on(LayerKey::Walls) {
            for (k, fp) in ls.footprints.iter().enumerate() {
                let id = format!("footprint:{}:{k}", ls.level.id);
                let s = m.solid(Part::Slab, &id, true);
                m.holed_slab(s, fp, &voids, e - under, e);
            }
        }
        if on(LayerKey::Slabs) {
            for sl in &ls.slabs {
                let (bottom, top) = model3d::slab_heights(sl);
                let s = m.solid(Part::Slab, &sl.id, true);
                m.holed_slab(s, &model3d::slab_outline(sl), &voids, e + bottom, e + top);
            }
        }
        for run in &ls.railings {
            let key = match run.kind {
                guhit_model::RailingRunKind::SlabEdge => LayerKey::Slabs,
                _ => LayerKey::Stairs,
            };
            if !on(key) {
                continue;
            }
            for piece in model3d::railing_pieces(run) {
                let s = m.solid(Part::Rail, &run.host_id, true);
                rail_piece(&mut m, s, &piece, e);
            }
        }
        for w in &ls.walls {
            if !on(w.wall.layer()) {
                continue;
            }
            let part = if w.wall.is_site() { Part::Site } else { Part::Wall };
            let s = m.solid(part, &w.wall.id, true);
            wall_faces(&mut m, s, w, e);
            if on(LayerKey::Openings) {
                opening_panels(&mut m, w, e);
            }
        }
        if on(LayerKey::Columns) {
            for c in &ls.columns {
                let poly = match c.shape {
                    ColumnShape::Round => model3d::circle(V::from(c.center), c.width_mm.max(1.0) / 2.0, 24),
                    ColumnShape::Rect => model3d::rect(V::from(c.center), c.width_mm.max(1.0), c.depth_mm.max(1.0), c.rotation_deg),
                };
                // Up to the roof over it, sloped with the roof.
                let s = m.solid(Part::Column, &c.id, true);
                let top = model3d::column_top(derived, ls.level, c);
                m.prism(s, &poly, &|_| e, &|q| e + top(q));
            }
        }
        if on(LayerKey::Stairs) {
            for st in &ls.stairs {
                stair_solids(&mut m, derived, st, e, ls.level.height_mm);
            }
        }
        for a in &ls.assets {
            let kind = match a.category {
                AssetCategory::Plant if a.catalog_key == "plant-pot" || a.width_mm.max(a.depth_mm) < 1200.0 => Some(FigureKind::Pot),
                AssetCategory::Plant => Some(FigureKind::Tree),
                AssetCategory::Vehicle => Some(FigureKind::Car),
                _ => None,
            };
            if !on(crate::services::asset_layer(a)) {
                continue;
            }
            if let Some(kind) = kind {
                m.figures.push(Figure {
                    kind,
                    element_id: a.id.clone(),
                    base: lift(V::from(a.position), e + a.elevation_mm),
                    width: a.width_mm.abs(),
                    depth: a.depth_mm.abs(),
                    height: a.height_mm.abs(),
                    rotation_deg: a.rotation_deg,
                });
            } else if a.category == AssetCategory::Site {
                let poly = model3d::rect(V::from(a.position), a.width_mm.max(1.0), a.depth_mm.max(1.0), a.rotation_deg);
                let s = m.solid(Part::Object, &a.id, true);
                m.box_solid(s, &poly, e + a.elevation_mm, e + a.elevation_mm + a.height_mm.max(1.0));
            }
        }
        if on(LayerKey::Roofs) || on(LayerKey::Slabs) {
            roof_decks(&mut m, project, derived, &scene.levels, li);
        }
    }

    if on(LayerKey::Roofs) {
        roofs(&mut m, project, derived, &scene);
    }
    m
}

/// How far the finish grade lies below the lowest floor, mm: the lot's
/// `grade_mm`, else 150.
pub fn grade_below(project: &Project) -> f64 {
    project
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Lot(l) => Some(l.grade_mm),
            _ => None,
        })
        .filter(|g| g.is_finite() && *g >= 0.0)
        .unwrap_or(150.0)
}

// ---------------------------------------------------------------- walls

/// The wall body with its openings cut out: strips along the wall at every
/// opening edge, intervals in height at every sill and head, as the live
/// view and the 3D DXF do. Each cell is solid or void; faces are only where
/// solid meets air, so the wall is one closed surface.
fn wall_faces(m: &mut Mesh, solid: u32, w: &WallSolid, e: f64) {
    let cells = model3d::wall_cells(w);
    let intervals = cells.heights.len().saturating_sub(1);
    let on_cut = |u: f64, cut: f64| cut.is_finite() && (u - cut).abs() < 0.01;
    let to3 = |p: V, h: f64| lift(w.frame.to_plan(p), e + h);

    for si in 0..cells.strips.len() {
        let poly = ensure_ccw(&cells.strips[si]);
        if poly.len() < 3 {
            continue;
        }
        let (lo, hi) = (cells.edges[si], cells.edges[si + 1]);
        for k in 0..intervals {
            if !cells.solid[si][k] {
                continue;
            }
            let (h0, h1) = (cells.heights[k], cells.heights[k + 1]);
            for i in 0..poly.len() {
                let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                // Edges on a strip cut are jambs or inside the wall.
                if (on_cut(a.x, lo) && on_cut(b.x, lo)) || (on_cut(a.x, hi) && on_cut(b.x, hi)) {
                    continue;
                }
                if (b - a).len() < 1e-6 {
                    continue;
                }
                m.face(solid, vec![to3(a, h0), to3(b, h0), to3(b, h1), to3(a, h1)]);
            }
            if k + 1 == intervals || !cells.solid[si][k + 1] {
                m.face(solid, poly.iter().map(|p| to3(*p, h1)).collect());
            }
            if k == 0 || !cells.solid[si][k - 1] {
                m.face(solid, poly.iter().rev().map(|p| to3(*p, h0)).collect());
            }
        }
    }

    // Jambs, heads and sills already have caps; the reveal faces at a strip
    // cut face the void side.
    for si in 0..cells.strips.len().saturating_sub(1) {
        let cut = cells.edges[si + 1];
        let mut v_min = f64::INFINITY;
        let mut v_max = f64::NEG_INFINITY;
        for p in cells.strips[si].iter().chain(cells.strips[si + 1].iter()) {
            if on_cut(p.x, cut) {
                v_min = v_min.min(p.y);
                v_max = v_max.max(p.y);
            }
        }
        if !(v_max - v_min > 1e-6) {
            continue;
        }
        let a = v(cut, v_min);
        let b = v(cut, v_max);
        for k in 0..intervals {
            let left = cells.solid[si][k];
            let right = cells.solid[si + 1][k];
            if left == right {
                continue;
            }
            let (h0, h1) = (cells.heights[k], cells.heights[k + 1]);
            if left {
                // Solid on the low side: the face looks along +u.
                m.face(solid, vec![to3(a, h0), to3(b, h0), to3(b, h1), to3(a, h1)]);
            } else {
                m.face(solid, vec![to3(b, h0), to3(a, h0), to3(a, h1), to3(b, h1)]);
            }
        }
    }
}

/// The leaf or sash inside each opening, and the lines drawn on it.
fn opening_panels(m: &mut Mesh, w: &WallSolid, e: f64) {
    let ob = bounds_of(&w.outline);
    for o in &w.openings {
        let sill = o.sill_mm.max(0.0);
        let height = o.height_mm.min(w.height - sill);
        if !(height >= 1.0 && o.width_mm >= 1.0) {
            continue;
        }
        let a = (o.offset_mm - o.width_mm / 2.0).max(ob.min.x);
        let b = (o.offset_mm + o.width_mm / 2.0).min(ob.max.x);
        if b - a < 1.0 {
            continue;
        }
        let half_t = w.wall.thickness_mm.abs() / 2.0;
        let mid = (a + b) / 2.0;
        m.openings.push(OpeningRef {
            opening_id: o.id.clone(),
            kind: o.opening_type,
            center: lift(w.frame.to_plan(v(mid, 0.0)), e + sill + height / 2.0),
            normal: p3(w.frame.left.x, w.frame.left.y, 0.0),
            half_t,
            width: b - a,
            height,
        });
        // A cased opening has no leaf.
        if o.opening_type == OpeningType::Door && o.style == OpeningStyle::Fixed {
            continue;
        }
        let (part, depth) = match o.opening_type {
            OpeningType::Door => (Part::Door, 45.0),
            OpeningType::Window => (Part::Window, 25.0),
        };
        let s = m.solid(part, &o.id, true);
        let hd = depth / 2.0;
        let rect = [v(a, -hd), v(b, -hd), v(b, hd), v(a, hd)].map(|p| w.frame.to_plan(p));
        let (z0, z1) = (e + sill, e + sill + height);
        m.box_solid(s, &rect, z0, z1);
        // Lines on both faces; the back ones hide behind the panel.
        for side in [-hd, hd] {
            let at = |u: f64, h: f64| lift(w.frame.to_plan(v(u, side)), h);
            for (p, q) in panel_lines(o.opening_type, o.style, o.flip_hinge, a, b, z0, z1) {
                m.decal(s, at(p.x, p.y), at(q.x, q.y));
            }
        }
    }
}

/// The lines an elevation draws on a door or window, in (u along the wall,
/// height) over the panel `a`..`b`, `z0`..`z1`.
fn panel_lines(kind: OpeningType, style: OpeningStyle, flip_hinge: bool, a: f64, b: f64, z0: f64, z1: f64) -> Vec<(V, V)> {
    let mut out = Vec::new();
    let rect = |out: &mut Vec<(V, V)>, x0: f64, y0: f64, x1: f64, y1: f64| {
        if x1 - x0 > 1.0 && y1 - y0 > 1.0 {
            out.push((v(x0, y0), v(x1, y0)));
            out.push((v(x1, y0), v(x1, y1)));
            out.push((v(x1, y1), v(x0, y1)));
            out.push((v(x0, y1), v(x0, y0)));
        }
    };
    let mid = (a + b) / 2.0;
    let zm = (z0 + z1) / 2.0;
    match kind {
        OpeningType::Window => {
            // The frame: 50 mm inside the opening.
            let f = 50.0_f64.min((b - a) * 0.2).min((z1 - z0) * 0.2);
            rect(&mut out, a + f, z0 + f, b - f, z1 - f);
            match style {
                OpeningStyle::Sliding => out.push((v(mid, z0 + f), v(mid, z1 - f))),
                OpeningStyle::Casement | OpeningStyle::SwingSingle | OpeningStyle::SwingDouble => {
                    // The swing: a V whose point is at the hinge.
                    let (hinge, free) = if flip_hinge { (b - f, a + f) } else { (a + f, b - f) };
                    out.push((v(free, z0 + f), v(hinge, zm)));
                    out.push((v(hinge, zm), v(free, z1 - f)));
                }
                OpeningStyle::Jalousie => {
                    let mut z = z0 + f + 100.0;
                    while z < z1 - f - 20.0 {
                        out.push((v(a + f, z), v(b - f, z)));
                        z += 100.0;
                    }
                }
                OpeningStyle::Fixed => {}
            }
        }
        OpeningType::Door => {
            let f = 100.0_f64.min((b - a) * 0.15);
            match style {
                OpeningStyle::SwingDouble => {
                    out.push((v(mid, z0), v(mid, z1)));
                    rect(&mut out, a + f, z0 + 150.0, mid - f, z1 - f);
                    rect(&mut out, mid + f, z0 + 150.0, b - f, z1 - f);
                }
                OpeningStyle::Sliding => {
                    out.push((v(mid, z0), v(mid, z1)));
                    rect(&mut out, a + f, z0 + f, mid - f, z1 - f);
                    rect(&mut out, mid + f, z0 + f, b - f, z1 - f);
                }
                _ => rect(&mut out, a + f, z0 + 150.0, b - f, z1 - f),
            }
        }
    }
    out
}

// ------------------------------------------------------------ railings

fn rail_piece(m: &mut Mesh, solid: u32, piece: &model3d::RailPiece, e: f64) {
    let prof = ensure_ccw(&piece.profile);
    if prof.len() < 3 {
        return;
    }
    let half = piece.thickness / 2.0;
    let at = |p: V, s: f64| {
        let (q, w) = piece.at(p, s);
        lift(q, e + w)
    };
    // The profile is counter-clockwise in (along, up), whose normal is
    // `across`: the +half cap faces out as is.
    m.face(solid, prof.iter().map(|p| at(*p, half)).collect());
    m.face(solid, prof.iter().rev().map(|p| at(*p, -half)).collect());
    for i in 0..prof.len() {
        let (a, b) = (prof[i], prof[(i + 1) % prof.len()]);
        m.face(solid, vec![at(a, -half), at(b, -half), at(b, half), at(a, half)]);
    }
}

// --------------------------------------------------------------- stairs

/// The floor openings of the stairs that arrive on a level
/// (`StairGeometry::opening`), counter-clockwise.
fn stair_wells(derived: &Derived, level_id: &str) -> Vec<Vec<V>> {
    derived
        .stairs
        .iter()
        .filter(|g| g.arrives_at.as_deref() == Some(level_id) && g.opening.len() >= 3)
        .map(|g| ensure_ccw(&g.opening.iter().map(V::from).collect::<Vec<V>>()))
        .collect()
}

/// Treads as boxes from the floor to each tread top, and landings, from
/// `Derived::stairs` when the engine has them, else the Wave 1 straight
/// flight.
fn stair_solids(m: &mut Mesh, derived: &Derived, st: &guhit_model::Stair, e: f64, level_height: f64) {
    let s = m.solid(Part::Stair, &st.id, true);
    if let Some(g) = derived.stairs.iter().find(|g| g.stair_id == st.id) {
        let tread = if g.tread_mm.is_finite() && g.tread_mm > 1.0 { g.tread_mm } else { 250.0 };
        for f in &g.flights {
            let up = V::from(f.up).unit().unwrap_or(v(0.0, 1.0));
            for n in &f.nosings {
                let (a, b) = (V::from(n.a), V::from(n.b));
                m.box_solid(s, &[a, b, b + up * tread, a + up * tread], e, e + n.z);
            }
        }
        for l in &g.landings {
            let poly: Vec<V> = l.polygon.iter().map(V::from).collect();
            m.box_solid(s, &poly, e, e + l.z);
        }
        return;
    }
    for t in model3d::stair_treads(st, level_height) {
        m.box_solid(s, &t.plan, e, e + t.top);
    }
}

// ---------------------------------------------------------------- roofs

/// The roofs: `Derived::roofs` (planes, gable infill, fascia along the
/// eaves) when the engine has them, else the Wave 1 planes of
/// `Scene::roofs`.
fn roofs(m: &mut Mesh, project: &Project, derived: &Derived, scene: &model3d::Scene) {
    let elevation = |level_id: &str| {
        project.levels.iter().find(|l| l.id == level_id).map(|l| l.elevation_mm).unwrap_or(0.0)
    };
    if !derived.roofs.is_empty() {
        for g in &derived.roofs {
            let e = elevation(&g.level_id);
            let s = m.solid(Part::Roof, &g.roof_id, true);
            let vert = if g.vertical_mm.is_finite() && g.vertical_mm > 1.0 { g.vertical_mm } else { 150.0 };
            for plane in &g.planes {
                let top: Vec<P3> = plane.iter().map(|p| p3(p.x, p.y, e + p.z)).collect();
                roof_plane(m, s, &top, vert);
            }
            // Gable ends: walls as thick as the wall under them.
            let walls = scene.levels.iter().find(|l| l.level.id == g.level_id).map(|l| &l.walls[..]).unwrap_or(&[]);
            for poly in &g.infill {
                let si = m.solid(Part::Wall, &g.roof_id, true);
                let pts: Vec<P3> = poly.iter().map(|p| p3(p.x, p.y, e + p.z)).collect();
                infill_prism(m, si, &pts, model3d::infill_host(poly, walls));
            }
            let fascia = project.elements.iter().find_map(|el| match el {
                Element::Roof(r) if r.id == g.roof_id => r.fascia.as_ref().map(|f| f.size_mm),
                _ => None,
            });
            if let Some(size) = fascia.filter(|s| s.is_finite() && *s > 1.0) {
                let sf = m.solid(Part::Roof, &g.roof_id, false);
                for run in &g.eaves {
                    for k in 0..run.len().saturating_sub(1) {
                        let (a, b) = (run[k], run[k + 1]);
                        let (a, b) = (p3(a.x, a.y, e + a.z), p3(b.x, b.y, e + b.z));
                        m.face(sf, vec![a - p3(0.0, 0.0, size), b - p3(0.0, 0.0, size), b, a]);
                    }
                }
            }
        }
        return;
    }
    for solid in &scene.roofs {
        if solid.kind == RoofKind::None {
            continue;
        }
        let e = scene.levels[solid.level].level.elevation_mm;
        let s = m.solid(Part::Roof, &solid.roof_id, true);
        for plane in &solid.planes {
            m.prism(s, &plane.plan, &|p| e + plane.under(p), &|p| e + plane.top(p));
        }
        if solid.kind == RoofKind::Gable {
            gable_infill(m, &solid.roof_id, &scene.levels[solid.level], &solid.planes, e);
        }
        // The Wave 1 roof had a fascia board along its eaves; draw it on
        // the low edges of each plane.
        let fascia = project
            .top_roof_area()
            .filter(|r| r.id == solid.roof_id)
            .and_then(|r| r.fascia.as_ref().map(|f| f.size_mm))
            .filter(|s| s.is_finite() && *s > 1.0);
        if let Some(size) = fascia {
            let sf = m.solid(Part::Roof, &solid.roof_id, true);
            for plane in &solid.planes {
                let poly = ensure_ccw(&plane.plan);
                let low = poly.iter().map(|p| plane.under(*p)).fold(f64::INFINITY, f64::min);
                for i in 0..poly.len() {
                    let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                    if (plane.under(a) - low).abs() > 1.0 || (plane.under(b) - low).abs() > 1.0 {
                        continue;
                    }
                    let Some(u) = (b - a).unit() else { continue };
                    // Outward of a counter-clockwise edge is to its right.
                    let out = v(u.y, -u.x);
                    let top = e + plane.top(a);
                    let board = [a, b, b + out * FASCIA_T, a + out * FASCIA_T];
                    m.box_solid(sf, &board, top - size.max(plane.thickness), top);
                }
            }
        }
    }
}

/// The gable ends of a Wave 1 gable roof over rectangular footprints: the
/// triangle between the end wall's top and the roof's underside, in the
/// plane of the wall's outer face, as the live view fills it. Other
/// footprints wait for `RoofGeometry::infill` (lane L2).
fn gable_infill(m: &mut Mesh, roof_id: &str, ls: &LevelScene, planes: &[crate::roofs::RoofPlane], e: f64) {
    for fp in &ls.footprints {
        let poly = ensure_ccw(fp);
        let b = bounds_of(&poly);
        let square = poly.len() == 4
            && poly.iter().all(|p| ((p.x - b.min.x).abs() < 1.0 || (p.x - b.max.x).abs() < 1.0) && ((p.y - b.min.y).abs() < 1.0 || (p.y - b.max.y).abs() < 1.0));
        if !square {
            continue;
        }
        let under = |p: V| {
            planes
                .iter()
                .filter(|pl| point_in_polygon(p, &pl.plan) || pl.plan.iter().any(|q| (*q - p).len() < 1.0))
                .map(|pl| pl.under(p))
                .fold(f64::NEG_INFINITY, f64::max)
        };
        // The ridge runs along the roof planes' `along`.
        let Some(pl) = planes.first() else { continue };
        let along_x = pl.along.x.abs() > pl.along.y.abs();
        let base = ls.level.height_mm;
        let ends: [(V, V); 2] = if along_x {
            [(v(b.min.x, b.max.y), v(b.min.x, b.min.y)), (v(b.max.x, b.min.y), v(b.max.x, b.max.y))]
        } else {
            [(v(b.min.x, b.min.y), v(b.max.x, b.min.y)), (v(b.max.x, b.max.y), v(b.min.x, b.max.y))]
        };
        for (p0, p1) in ends {
            let mid = (p0 + p1) * 0.5;
            let top = under(mid);
            if !(top > base + 1.0) {
                continue;
            }
            // Wound so it faces out of the building.
            let tri = [lift(p0, e + base), lift(p1, e + base), lift(mid, e + top)];
            let host = model3d::infill_host(&tri.map(|q| guhit_model::Vec3 { x: q.x, y: q.y, z: q.z }), &ls.walls);
            let s = m.solid(Part::Wall, roof_id, true);
            infill_prism(m, s, &tri, host);
        }
    }
}

/// A gable-end infill polygon (vertical, z above project zero) made a
/// closed wall as thick as its host wall and centered on it
/// (`model3d::infill_host`), as the IFC builds it, so sections cut it in
/// poche. A polygon that is not vertical stays a single face.
fn infill_prism(m: &mut Mesh, s: u32, poly: &[P3], host: Option<(f64, V, V)>) {
    let mut pts: Vec<P3> = Vec::with_capacity(poly.len());
    for q in poly {
        if pts.last().is_none_or(|l: &P3| (*l - *q).len() > 0.01) {
            pts.push(*q);
        }
    }
    while pts.len() > 1 && (pts[0] - pts[pts.len() - 1]).len() <= 0.01 {
        pts.pop();
    }
    if pts.len() < 3 {
        return;
    }
    let n = super::hlr::newell(&pts);
    let Some(nu) = p3(n.x, n.y, 0.0).unit().filter(|_| n.z.abs() <= 0.05 * n.len()) else {
        m.face(s, pts);
        return;
    };
    let t = host.map(|h| h.0).unwrap_or(150.0).max(1.0);
    let shift = model3d::infill_shift(pts[0].plan(), v(nu.x, nu.y), host);
    let front: Vec<P3> = pts.iter().map(|q| *q + nu * (shift + t / 2.0)).collect();
    let back: Vec<P3> = pts.iter().map(|q| *q + nu * (shift - t / 2.0)).collect();
    // The polygon's own winding faces +nu.
    m.face(s, front.clone());
    m.face(s, back.iter().rev().copied().collect());
    for i in 0..pts.len() {
        let j = (i + 1) % pts.len();
        m.face(s, vec![back[i], back[j], front[j], front[i]]);
    }
}

/// A roof plane from its top polygon: the underside `vert` below, sides
/// between them.
fn roof_plane(m: &mut Mesh, s: u32, top: &[P3], vert: f64) {
    if top.len() < 3 {
        return;
    }
    // Wind it counter-clockwise from above.
    let plan: Vec<V> = top.iter().map(|p| p.plan()).collect();
    let top: Vec<P3> = if signed_area(&plan) < 0.0 { top.iter().rev().copied().collect() } else { top.to_vec() };
    let down = p3(0.0, 0.0, vert);
    m.face(s, top.clone());
    m.face(s, top.iter().rev().map(|p| *p - down).collect());
    for i in 0..top.len() {
        let (a, b) = (top[i], top[(i + 1) % top.len()]);
        m.face(s, vec![a - down, b - down, b, a]);
    }
}

/// D26: the part of a level the next level up does not cover gets a flat
/// 200 mm deck at its ceiling. The topmost level with a building keeps its
/// roof; a level that carries its own roof element has none either.
fn roof_decks(m: &mut Mesh, project: &Project, derived: &Derived, levels: &[LevelScene], li: usize) {
    let ls = &levels[li];
    if ls.footprints.is_empty() {
        return;
    }
    let top_built = levels
        .iter()
        .enumerate()
        .filter(|(_, l)| !l.footprints.is_empty())
        .max_by(|a, b| a.1.level.elevation_mm.partial_cmp(&b.1.level.elevation_mm).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i);
    if top_built == Some(li) {
        return;
    }
    let own_roof = project.elements.iter().any(|e| {
        matches!(e, Element::Roof(r) if r.level_id.as_deref() == Some(ls.level.id.as_str())
            && r.outline == guhit_model::RoofOutline::Footprint)
    });
    if own_roof {
        return;
    }
    let ceiling = ls.level.elevation_mm + ls.level.height_mm;
    // The next level up: the lowest one above this.
    let upper = levels
        .iter()
        .filter(|l| l.level.elevation_mm > ls.level.elevation_mm + 1.0)
        .min_by(|a, b| a.level.elevation_mm.partial_cmp(&b.level.elevation_mm).unwrap_or(std::cmp::Ordering::Equal));
    let mut pieces: Vec<Vec<V>> = ls.footprints.iter().flat_map(|fp| triangles(fp)).collect();
    if let Some(up) = upper {
        let mut covers: Vec<Vec<V>> = up.footprints.clone();
        covers.extend(up.slabs.iter().filter(|s| s.kind == SlabKind::Floor).map(|s| model3d::slab_outline(s)));
        for fp in &covers {
            for t in triangles(fp) {
                pieces = pieces.iter().flat_map(|p| subtract_convex(p, &t)).collect();
            }
        }
    }
    let pieces: Vec<Vec<V>> = pieces.into_iter().filter(|p| polygon_area(p) > 2500.0).collect();
    if pieces.is_empty() {
        return;
    }
    // It is the floor of the level above: that level's stair wells cut it.
    let wells = upper.map(|up| stair_wells(derived, &up.level.id)).unwrap_or_default();
    let mut pieces = pieces;
    for w in &wells {
        for t in triangles(w) {
            pieces = pieces.iter().flat_map(|p| subtract_convex(p, &t)).collect();
        }
    }
    let pieces: Vec<Vec<V>> = pieces.into_iter().filter(|p| polygon_area(p) > 2500.0).collect();
    if pieces.is_empty() {
        return;
    }
    let s = m.solid(Part::Slab, &format!("deck:{}", ls.level.id), true);
    for p in pieces {
        m.box_solid(s, &p, ceiling - DECK_MM, ceiling);
    }
}

// ------------------------------------------------------------ box forms

/// Every solid as its bounding box, for models too big to draw in full.
fn box_forms(full: &Mesh) -> Mesh {
    let mut lo = vec![p3(f64::INFINITY, f64::INFINITY, f64::INFINITY); full.solids.len()];
    let mut hi = vec![p3(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY); full.solids.len()];
    for f in &full.faces {
        let i = f.solid as usize;
        for p in &f.pts {
            lo[i] = p3(lo[i].x.min(p.x), lo[i].y.min(p.y), lo[i].z.min(p.z));
            hi[i] = p3(hi[i].x.max(p.x), hi[i].y.max(p.y), hi[i].z.max(p.z));
        }
    }
    let mut m = Mesh { boxed: true, figures: full.figures.clone(), openings: full.openings.clone(), ..Mesh::default() };
    for (i, s) in full.solids.iter().enumerate() {
        if !lo[i].is_finite() || !hi[i].is_finite() {
            continue;
        }
        let id = m.solid(s.part, &s.element_id, true);
        let r = [v(lo[i].x, lo[i].y), v(hi[i].x, lo[i].y), v(hi[i].x, hi[i].y), v(lo[i].x, hi[i].y)];
        m.box_solid(id, &r, lo[i].z, hi[i].z.max(lo[i].z + 1.0));
    }
    m
}
