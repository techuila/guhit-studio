//! 3D DXF export: the whole model as 3DFACE entities, plus the 2D plan
//! linework of every level at z = 0 on the layers the 2D DXF uses. One file
//! gives an AutoCAD user both the drawing and the solid.
//!
//! R12 (AC1009) flavor like `dxf.rs`: 3DFACE exists there and it is the most
//! widely readable DXF. Model space, millimeters, 1:1, y is north, z is up.
//!
//! Layers: the nine 2D layers plus A-ROOF and A-FLOR-SLAB.
//!
//! Simplifications, the same ones the IFC export makes: a gable roof is two
//! sloped prisms and the triangular gable ends are not filled, so the walls
//! keep their own height and the roof planes cover them; openings are boxes;
//! a door or window is one thin panel; Linework, ReferenceModel, Underlay and
//! Camera elements have no solid. Stairs do get real treads here, one box per
//! riser, because 3DFACE geometry is cheap.

use guhit_model::{
    ColumnShape, Derived, Element, LayerKey, OpeningType, Project, RoofKind,
};

use crate::geom::*;
use crate::model3d::*;
use crate::plan::{self, Cat, Item, Prim};
use crate::ExportError;

/// A point in 3D model space, mm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct P3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub fn p3(x: f64, y: f64, z: f64) -> P3 {
    P3 { x, y, z }
}

fn lift(p: V, z: f64) -> P3 {
    p3(p.x, p.y, z)
}

/// The extra layers the 3D file adds on top of the 2D set.
pub const ROOF_LAYER: &str = "A-ROOF";
pub const SLAB_LAYER: &str = "A-FLOR-SLAB";

fn layer_color(layer: &str) -> i64 {
    match layer {
        "A-WALL" => 7,
        "A-DOOR" => 4,
        "A-GLAZ" => 5,
        "A-COLS" => 7,
        "A-FLOR-STRS" => 6,
        "A-FURN" => 8,
        "A-AREA" => 3,
        "A-ANNO-DIMS" => 1,
        "A-ANNO-TEXT" => 7,
        ROOF_LAYER => 2,
        SLAB_LAYER => 9,
        _ => 7,
    }
}

fn all_layers() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = crate::dxf::dxf_layer_order()
        .iter()
        .map(|c| c.dxf_layer())
        .collect();
    v.push(ROOF_LAYER);
    v.push(SLAB_LAYER);
    v
}

fn f(x: f64) -> String {
    let x = if x.is_finite() { x } else { 0.0 };
    let s = format!("{x:.4}");
    let s = s.trim_end_matches('0');
    let s = if s.ends_with('.') {
        format!("{s}0")
    } else {
        s.to_string()
    };
    if s == "-0.0" {
        "0.0".into()
    } else {
        s
    }
}

/// One triangular or quadrilateral face on a layer.
#[derive(Debug, Clone)]
pub struct Face {
    pub layer: &'static str,
    pub pts: [P3; 4],
}

/// Collects faces while the model is walked.
#[derive(Default)]
pub struct Faces {
    pub list: Vec<Face>,
}

impl Faces {
    fn quad(&mut self, layer: &'static str, a: P3, b: P3, c: P3, d: P3) {
        for p in [a, b, c, d] {
            if !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite()) {
                return;
            }
        }
        self.list.push(Face {
            layer,
            pts: [a, b, c, d],
        });
    }

    fn tri(&mut self, layer: &'static str, a: P3, b: P3, c: P3) {
        // R12 3DFACE always carries four corners; a triangle repeats the last.
        self.quad(layer, a, b, c, c);
    }

    /// Triangulated cap of a plan polygon at a height given per point.
    fn cap(&mut self, layer: &'static str, poly: &[V], height: &dyn Fn(V) -> f64, up: bool) {
        for t in triangulate(poly) {
            let (a, b, c) = (poly[t[0]], poly[t[1]], poly[t[2]]);
            let (pa, pb, pc) = (
                lift(a, height(a)),
                lift(b, height(b)),
                lift(c, height(c)),
            );
            if up {
                self.tri(layer, pa, pb, pc);
            } else {
                self.tri(layer, pc, pb, pa);
            }
        }
    }

    /// Closed prism between two height functions over a plan polygon.
    fn prism(
        &mut self,
        layer: &'static str,
        poly: &[V],
        bottom: &dyn Fn(V) -> f64,
        top: &dyn Fn(V) -> f64,
    ) {
        if poly.len() < 3 {
            return;
        }
        self.cap(layer, poly, top, true);
        self.cap(layer, poly, bottom, false);
        for i in 0..poly.len() {
            let a = poly[i];
            let b = poly[(i + 1) % poly.len()];
            self.quad(
                layer,
                lift(a, bottom(a)),
                lift(b, bottom(b)),
                lift(b, top(b)),
                lift(a, top(a)),
            );
        }
    }

    fn box_solid(&mut self, layer: &'static str, poly: &[V], z0: f64, z1: f64) {
        self.prism(layer, poly, &|_| z0, &|_| z1);
    }
}

// --------------------------------------------------------------------- walls

/// Wall solid with its openings cut out: strips and height intervals, the same
/// decomposition the live 3D viewer uses, so reveals exist on all four sides.
fn wall_faces(faces: &mut Faces, w: &WallSolid, elevation: f64) {
    let cells = wall_cells(w);
    let intervals = cells.heights.len().saturating_sub(1);
    let on_cut = |u: f64, cut: f64| cut.is_finite() && (u - cut).abs() < 0.01;
    let e = elevation;

    for si in 0..cells.strips.len() {
        let poly = &cells.strips[si];
        if poly.len() < 3 {
            continue;
        }
        let lo = cells.edges[si];
        let hi = cells.edges[si + 1];
        let tris = triangulate(poly);

        for k in 0..intervals {
            if !cells.solid[si][k] {
                continue;
            }
            let h0 = e + cells.heights[k];
            let h1 = e + cells.heights[k + 1];

            // Outer faces. Edges that sit on a strip cut are jambs instead.
            for i in 0..poly.len() {
                let a = poly[i];
                let b = poly[(i + 1) % poly.len()];
                if (on_cut(a.x, lo) && on_cut(b.x, lo)) || (on_cut(a.x, hi) && on_cut(b.x, hi)) {
                    continue;
                }
                if (b - a).len() < 1e-6 {
                    continue;
                }
                let pa = w.frame.to_plan(a);
                let pb = w.frame.to_plan(b);
                faces.quad(
                    "A-WALL",
                    lift(pa, h0),
                    lift(pb, h0),
                    lift(pb, h1),
                    lift(pa, h1),
                );
            }

            // Top cap: the wall top, or the sill under a window.
            if k + 1 == intervals || !cells.solid[si][k + 1] {
                for t in &tris {
                    faces.tri(
                        "A-WALL",
                        lift(w.frame.to_plan(poly[t[0]]), h1),
                        lift(w.frame.to_plan(poly[t[1]]), h1),
                        lift(w.frame.to_plan(poly[t[2]]), h1),
                    );
                }
            }
            // Bottom cap: the wall base, or the underside of a lintel.
            if k == 0 || !cells.solid[si][k - 1] {
                for t in &tris {
                    faces.tri(
                        "A-WALL",
                        lift(w.frame.to_plan(poly[t[2]]), h0),
                        lift(w.frame.to_plan(poly[t[1]]), h0),
                        lift(w.frame.to_plan(poly[t[0]]), h0),
                    );
                }
            }
        }
    }

    // Jambs: the reveal faces where one side of a cut is solid and the other void.
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
        if !v_min.is_finite() || !v_max.is_finite() || v_max - v_min <= 1e-6 {
            continue;
        }
        for k in 0..intervals {
            let left = cells.solid[si][k];
            let right = cells.solid[si + 1][k];
            if left == right {
                continue;
            }
            let a = w.frame.to_plan(v(cut, v_min));
            let b = w.frame.to_plan(v(cut, v_max));
            let h0 = e + cells.heights[k];
            let h1 = e + cells.heights[k + 1];
            faces.quad(
                "A-WALL",
                lift(a, h0),
                lift(b, h0),
                lift(b, h1),
                lift(a, h1),
            );
        }
    }
}

// ---------------------------------------------------------------- the writer

struct Writer {
    s: String,
}

impl Writer {
    fn pair(&mut self, code: i32, value: &str) {
        self.s.push_str(&format!("{code:>3}\n{value}\n"));
    }

    fn num(&mut self, code: i32, value: f64) {
        self.pair(code, &f(value));
    }

    fn int(&mut self, code: i32, value: i64) {
        self.pair(code, &format!("{value:>6}"));
    }
}

/// Build every 3D face of the model. Exposed so the tests can check the
/// geometry without reparsing the file.
pub fn build_faces(project: &Project, derived: &Derived) -> Faces {
    let scene = build_scene(project, derived);
    let mut faces = Faces::default();

    for ls in &scene.levels {
        let e = ls.level.elevation_mm;

        for fp in &ls.footprints {
            faces.box_solid(SLAB_LAYER, fp, e - SLAB_THICKNESS_MM, e);
        }

        for w in &ls.walls {
            wall_faces(&mut faces, w, e);
            // Leaf or sash panel inside each opening, so the hole reads as a
            // door or a window and not as a gap.
            for o in &w.openings {
                let sill = o.sill_mm.max(0.0);
                let height = o.height_mm.max(1.0).min((w.height - sill).max(1.0));
                if height < 1.0 || o.width_mm < 1.0 {
                    continue;
                }
                let (layer, depth) = match o.opening_type {
                    OpeningType::Door => ("A-DOOR", 45.0),
                    OpeningType::Window => ("A-GLAZ", 25.0),
                };
                let panel = opening_rect(w, o, depth);
                faces.box_solid(layer, &panel, e + sill, e + sill + height);
            }
        }

        for c in &ls.columns {
            let poly = match c.shape {
                ColumnShape::Round => circle(V::from(c.center), c.width_mm.max(1.0) / 2.0, 24),
                ColumnShape::Rect => rect(
                    V::from(c.center),
                    c.width_mm.max(1.0),
                    c.depth_mm.max(1.0),
                    c.rotation_deg,
                ),
            };
            faces.box_solid("A-COLS", &poly, e, e + ls.level.height_mm.max(1.0));
        }

        for st in &ls.stairs {
            for t in stair_treads(st, ls.level.height_mm) {
                faces.box_solid("A-FLOR-STRS", &t.plan, e, e + t.top);
            }
        }

        for a in &ls.assets {
            let poly = rect(
                V::from(a.position),
                a.width_mm.max(1.0),
                a.depth_mm.max(1.0),
                a.rotation_deg,
            );
            faces.box_solid(
                "A-FURN",
                &poly,
                e + a.elevation_mm,
                e + a.elevation_mm + a.height_mm.max(1.0),
            );
        }
    }

    if let Some(top) = scene.roof_level {
        let e = scene.levels[top].level.elevation_mm;
        for plane in &scene.roof {
            let plan = plane.plan.clone();
            faces.prism(
                ROOF_LAYER,
                &plan,
                &|p| e + plane.under(p),
                &|p| e + plane.top(p),
            );
        }
    }

    faces
}

/// Model space DXF in mm holding 3DFACE solids and the 2D plan linework of
/// every level at z = 0.
pub fn write(project: &Project, derived: &Derived) -> Result<String, ExportError> {
    if project.levels.is_empty() {
        return Err(ExportError::Empty("the project has no levels".into()));
    }
    let faces = build_faces(project, derived);

    // 2D linework for every level, on the same layers the 2D DXF uses.
    let scale = if project.settings.scale_denominator > 0 {
        project.settings.scale_denominator
    } else {
        100
    };
    let mut items: Vec<Item> = Vec::new();
    for level in &project.levels {
        let built = plan::build_plan(
            project,
            derived,
            level,
            &plan::PlanOptions {
                scale: scale as f64,
                show_dimensions: project
                    .layers
                    .iter()
                    .any(|l| l.key == LayerKey::Dimensions && l.visible),
                show_room_labels: true,
                show_assets: true,
                unicode: false,
            },
        );
        if let Ok(mut b) = built {
            items.append(&mut b);
        }
    }
    if faces.list.is_empty() && items.is_empty() {
        return Err(ExportError::Empty("the model has nothing to export".into()));
    }

    // Bounds over everything, for the header extents.
    let mut min = p3(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut max = p3(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for face in &faces.list {
        for p in face.pts {
            min = p3(min.x.min(p.x), min.y.min(p.y), min.z.min(p.z));
            max = p3(max.x.max(p.x), max.y.max(p.y), max.z.max(p.z));
        }
    }
    let flat = plan::items_bounds(&items);
    if !flat.is_empty() {
        min = p3(min.x.min(flat.min.x), min.y.min(flat.min.y), min.z.min(0.0));
        max = p3(max.x.max(flat.max.x), max.y.max(flat.max.y), max.z.max(0.0));
    }
    if !min.x.is_finite() {
        min = p3(0.0, 0.0, 0.0);
        max = p3(0.0, 0.0, 0.0);
    }

    let mut w = Writer { s: String::new() };
    w.pair(999, "Guhit Studio 3D model export");
    w.pair(
        999,
        "Units: millimeters, model space 1:1, y is north, z is up. 3DFACE solids plus 2D plan linework at z = 0",
    );

    // HEADER
    w.pair(0, "SECTION");
    w.pair(2, "HEADER");
    w.pair(9, "$ACADVER");
    w.pair(1, "AC1009");
    w.pair(9, "$INSBASE");
    w.num(10, 0.0);
    w.num(20, 0.0);
    w.num(30, 0.0);
    w.pair(9, "$EXTMIN");
    w.num(10, min.x);
    w.num(20, min.y);
    w.num(30, min.z);
    w.pair(9, "$EXTMAX");
    w.num(10, max.x);
    w.num(20, max.y);
    w.num(30, max.z);
    w.pair(9, "$LIMMIN");
    w.num(10, min.x);
    w.num(20, min.y);
    w.pair(9, "$LIMMAX");
    w.num(10, max.x);
    w.num(20, max.y);
    w.pair(9, "$LUNITS");
    w.int(70, 2);
    w.pair(9, "$LUPREC");
    w.int(70, 2);
    // Millimeters. R12 readers that do not know $INSUNITS skip it.
    w.pair(9, "$INSUNITS");
    w.int(70, 4);
    w.pair(0, "ENDSEC");

    // TABLES
    w.pair(0, "SECTION");
    w.pair(2, "TABLES");
    w.pair(0, "TABLE");
    w.pair(2, "LTYPE");
    w.int(70, 1);
    w.pair(0, "LTYPE");
    w.pair(2, "CONTINUOUS");
    w.int(70, 0);
    w.pair(3, "Solid line");
    w.int(72, 65);
    w.int(73, 0);
    w.num(40, 0.0);
    w.pair(0, "ENDTAB");

    let layers = all_layers();
    w.pair(0, "TABLE");
    w.pair(2, "LAYER");
    w.int(70, layers.len() as i64 + 1);
    w.pair(0, "LAYER");
    w.pair(2, "0");
    w.int(70, 0);
    w.int(62, 7);
    w.pair(6, "CONTINUOUS");
    for layer in &layers {
        w.pair(0, "LAYER");
        w.pair(2, layer);
        w.int(70, 0);
        w.int(62, layer_color(layer));
        w.pair(6, "CONTINUOUS");
    }
    w.pair(0, "ENDTAB");

    w.pair(0, "TABLE");
    w.pair(2, "STYLE");
    w.int(70, 1);
    w.pair(0, "STYLE");
    w.pair(2, "STANDARD");
    w.int(70, 0);
    w.num(40, 0.0);
    w.num(41, 1.0);
    w.num(50, 0.0);
    w.int(71, 0);
    w.num(42, 2.5 * scale as f64);
    w.pair(3, "txt");
    w.pair(4, "");
    w.pair(0, "ENDTAB");
    w.pair(0, "ENDSEC");

    w.pair(0, "SECTION");
    w.pair(2, "BLOCKS");
    w.pair(0, "ENDSEC");

    // ENTITIES: solids first, grouped by layer, then the flat linework.
    w.pair(0, "SECTION");
    w.pair(2, "ENTITIES");
    for layer in &layers {
        for face in faces.list.iter().filter(|x| &x.layer == layer) {
            w.pair(0, "3DFACE");
            w.pair(8, face.layer);
            for (i, p) in face.pts.iter().enumerate() {
                w.num(10 + i as i32, p.x);
                w.num(20 + i as i32, p.y);
                w.num(30 + i as i32, p.z);
            }
        }
    }
    for cat in crate::dxf::dxf_layer_order() {
        for item in items.iter().filter(|i| i.cat == cat) {
            flat_entity(&mut w, item, cat);
        }
    }
    w.pair(0, "ENDSEC");
    w.pair(0, "EOF");
    Ok(w.s)
}

/// The 2D plan primitives at z = 0. Text is dropped for the solid layers that
/// have no 2D meaning; everything else matches the 2D DXF writer.
fn flat_entity(w: &mut Writer, item: &Item, cat: Cat) {
    let layer = cat.dxf_layer();
    match &item.prim {
        Prim::Line { a, b } => {
            w.pair(0, "LINE");
            w.pair(8, layer);
            w.num(10, a.x);
            w.num(20, a.y);
            w.num(30, 0.0);
            w.num(11, b.x);
            w.num(21, b.y);
            w.num(31, 0.0);
        }
        Prim::Poly { pts, closed, .. } => {
            w.pair(0, "POLYLINE");
            w.pair(8, layer);
            w.int(66, 1);
            w.num(10, 0.0);
            w.num(20, 0.0);
            w.num(30, 0.0);
            w.int(70, if *closed { 1 } else { 0 });
            for p in pts {
                w.pair(0, "VERTEX");
                w.pair(8, layer);
                w.num(10, p.x);
                w.num(20, p.y);
                w.num(30, 0.0);
            }
            w.pair(0, "SEQEND");
            w.pair(8, layer);
        }
        Prim::Arc {
            c,
            r,
            start_deg,
            end_deg,
        } => {
            w.pair(0, "ARC");
            w.pair(8, layer);
            w.num(10, c.x);
            w.num(20, c.y);
            w.num(30, 0.0);
            w.num(40, *r);
            w.num(50, norm_deg(*start_deg));
            w.num(51, norm_deg(*end_deg));
        }
        Prim::Circle { c, r, .. } => {
            w.pair(0, "CIRCLE");
            w.pair(8, layer);
            w.num(10, c.x);
            w.num(20, c.y);
            w.num(30, 0.0);
            w.num(40, *r);
        }
        Prim::Text {
            pos,
            height,
            rot_deg,
            text,
            ..
        } => {
            w.pair(0, "TEXT");
            w.pair(8, layer);
            w.num(10, pos.x);
            w.num(20, pos.y);
            w.num(30, 0.0);
            w.num(40, *height);
            w.pair(1, &crate::dxf::dxf_text(text));
            if rot_deg.abs() > 1e-9 {
                w.num(50, norm_deg(*rot_deg));
            }
            w.pair(7, "STANDARD");
        }
    }
}

/// How many faces each layer got. Used by the tests and by the report.
pub fn faces_per_layer(faces: &Faces) -> Vec<(&'static str, usize)> {
    all_layers()
        .into_iter()
        .map(|l| (l, faces.list.iter().filter(|f| f.layer == l).count()))
        .collect()
}

/// True when the project has a roof that should produce faces.
pub fn has_roof(project: &Project) -> bool {
    project.roof.kind != RoofKind::None
        && project
            .elements
            .iter()
            .any(|e| matches!(e, Element::Wall(_)))
}
