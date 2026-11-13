//! Builds a DXF 2013 `Doc` from the plan: the same primitives the sheet
//! draws (`plan::build_items`), with what CAD users expect on top.
//!
//! - Every door and window is an INSERT of a block per type: the mark ("D1")
//!   for a typed opening, "DOOR-SWING-900" for an untyped one. The block is
//!   the plan's own drawing of the opening, cut out of a one-wall plan, so
//!   it never drifts from the sheet. A hinge or swing on the other side is
//!   the same block mirrored. A type in walls of two thicknesses gets a
//!   second block, "D1_WALL200". Each insert carries invisible MARK, WIDTH,
//!   HEIGHT and SILL attributes for data extraction.
//! - Every object is an INSERT of a block per catalog item and size, with
//!   an invisible NAME attribute. Its label stays loose on A-FURN.
//! - Dimensions are DIMENSION entities with a dimension style per scale
//!   (`GUHIT-100`) and the anonymous `*D` block AutoCAD draws them from. A
//!   dimension string is one dimension per segment on one line.
//! - Cut walls, columns and every other solid ink fill are solid HATCHes
//!   inside their outline.
//! - Layers carry the plan's pens as lineweights; an entity drawn with
//!   another pen carries its own.
//!
//! Anything the plan draws that this file does not recognise stays loose
//! geometry on its layer, so nothing drawn is ever lost.

use std::collections::{HashMap, HashSet};

use guhit_model::{
    Derived, DisplayUnit, Element, LayerKey, Level, Opening, OpeningStyle, OpeningType, Point,
    Project, Wall, WallRole,
};

use super::doc::*;
use crate::geom::*;
use crate::pipes::{self, Dash, PlanPipe};
use crate::plan::{self, paper, Cat, Fill, HAlign, Item, Pen, PlanOptions, Prim};
use crate::text::{clean, est_width, format_length};

/// The pen a layer draws with; its lineweight is the layer's.
pub(crate) fn layer_pen(cat: Cat) -> Pen {
    match cat {
        Cat::Wall | Cat::Site => Pen::Heavy,
        Cat::Door | Cat::Glaz | Cat::Cols => Pen::Medium,
        _ => Pen::Light,
    }
}

pub(crate) fn pen_lineweight(pen: Pen) -> i64 {
    lineweight_of(crate::sheet::pen_width(pen))
}

/// AutoCAD color index per layer. Dark, readable on white and on black.
pub(crate) fn layer_color(cat: Cat) -> i64 {
    match cat {
        Cat::Wall | Cat::Cols | Cat::Text | Cat::Symb => 7,
        Cat::Site | Cat::Slab | Cat::Furn | Cat::Grid => 8,
        Cat::Roof => 2,
        Cat::Door => 4,
        Cat::Glaz => 5,
        Cat::Strs | Cat::StrsBelow | Cat::Rail => 6,
        Cat::Area => 3,
        Cat::Dims => 1,
    }
}

/// One plan to add: a level of a project at 1:`n`.
pub(crate) struct PlanView<'a> {
    pub project: &'a Project,
    pub derived: &'a Derived,
    pub level: &'a Level,
    pub n: f64,
    pub show_dimensions: bool,
    pub show_room_labels: bool,
    pub show_assets: bool,
    pub show_pipes: bool,
}

impl PlanView<'_> {
    fn options(&self, dims: bool) -> PlanOptions {
        PlanOptions {
            scale: self.n,
            show_dimensions: dims,
            show_room_labels: self.show_room_labels,
            show_assets: self.show_assets,
            unicode: true,
            skip_devices: false,
        }
    }

    fn pipes(&self) -> Vec<PlanPipe> {
        if self.show_pipes {
            pipes::plan_pipes(self.project, self.level)
        } else {
            Vec::new()
        }
    }

    /// Model extent of everything the plan draws, dimensions included.
    pub fn extent(&self) -> Bounds {
        let items = plan::build_items(self.project, self.derived, self.level, &self.options(self.show_dimensions));
        let mut b = plan::items_bounds(&items);
        let p = self.pipes();
        if !p.is_empty() {
            let pb = pipes::bounds(&p, self.n);
            b.add(pb.min);
            b.add(pb.max);
        }
        b
    }
}

/// A point transform: scale by (sx, sy), turn by `rot` degrees, move to `at`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Xf {
    pub at: V,
    pub rot: f64,
    pub sx: f64,
    pub sy: f64,
}

impl Xf {
    pub fn p(&self, q: V) -> V {
        self.at + v(q.x * self.sx, q.y * self.sy).rotated(self.rot)
    }

    fn mirrored(&self) -> bool {
        self.sx * self.sy < 0.0
    }

    /// Where a direction at `a` degrees points after the transform.
    fn angle(&self, a: f64) -> f64 {
        let d = dir(a);
        v(d.x * self.sx, d.y * self.sy).rotated(self.rot).angle_deg()
    }
}

/// A primitive put through a transform. A mirrored arc runs the other way,
/// so its ends swap.
pub(crate) fn xf_prim(p: &Prim, x: &Xf) -> Prim {
    match p {
        Prim::Line { a, b } => Prim::Line { a: x.p(*a), b: x.p(*b) },
        Prim::Poly { pts, closed, fill } => Prim::Poly {
            pts: pts.iter().map(|q| x.p(*q)).collect(),
            closed: *closed,
            fill: *fill,
        },
        Prim::Arc { c, r, start_deg, end_deg } => {
            let (s, e) = (x.angle(*start_deg), x.angle(*end_deg));
            let (s, e) = if x.mirrored() { (e, s) } else { (s, e) };
            Prim::Arc {
                c: x.p(*c),
                r: *r,
                start_deg: norm_deg(s),
                end_deg: norm_deg(e),
            }
        }
        Prim::Circle { c, r, fill } => Prim::Circle { c: x.p(*c), r: *r, fill: *fill },
        Prim::Text { pos, height, rot_deg, align, text, bold } => Prim::Text {
            pos: x.p(*pos),
            height: *height,
            rot_deg: x.angle(*rot_deg),
            align: *align,
            text: text.clone(),
            bold: *bold,
        },
    }
}

const TOL: f64 = 1e-3;

fn near(a: V, b: V) -> bool {
    (a - b).len() < TOL
}

fn same_angle(a: f64, b: f64) -> bool {
    let d = norm_deg(a - b);
    !(1e-4..=360.0 - 1e-4).contains(&d)
}

/// Two primitives that draw the same thing, within 0.001 mm.
pub(crate) fn same_prim(a: &Prim, b: &Prim) -> bool {
    match (a, b) {
        (Prim::Line { a: a0, b: a1 }, Prim::Line { a: b0, b: b1 }) => {
            (near(*a0, *b0) && near(*a1, *b1)) || (near(*a0, *b1) && near(*a1, *b0))
        }
        (Prim::Poly { pts: p, closed: c, fill: f }, Prim::Poly { pts: q, closed: d, fill: g }) => {
            c == d && f == g && p.len() == q.len() && p.iter().zip(q).all(|(x, y)| near(*x, *y))
        }
        (
            Prim::Arc { c, r, start_deg, end_deg },
            Prim::Arc { c: c2, r: r2, start_deg: s2, end_deg: e2 },
        ) => near(*c, *c2) && (r - r2).abs() < TOL && same_angle(*start_deg, *s2) && same_angle(*end_deg, *e2),
        (Prim::Circle { c, r, fill }, Prim::Circle { c: c2, r: r2, fill: f2 }) => {
            near(*c, *c2) && (r - r2).abs() < TOL && fill == f2
        }
        _ => false,
    }
}

/// Block names: letters, digits, hyphens and underscores.
fn block_name(text: &str) -> String {
    let s: String = clean(text)
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let s = s.trim_matches('_').to_string();
    if s.is_empty() {
        "BLOCK".into()
    } else {
        s
    }
}

fn style_word(style: OpeningStyle) -> &'static str {
    match style {
        OpeningStyle::SwingSingle => "SWING",
        OpeningStyle::SwingDouble => "DOUBLE",
        OpeningStyle::Sliding => "SLIDING",
        OpeningStyle::Fixed => "FIXED",
        OpeningStyle::Casement => "CASEMENT",
        OpeningStyle::Jalousie => "JALOUSIE",
    }
}

fn mm_text(x: f64) -> String {
    format!("{}", x.round() as i64)
}

pub(crate) struct Builder {
    pub doc: Doc,
    /// 1:N the linetypes are sized for. Entities of a view at another scale
    /// carry a linetype scale.
    pub n: f64,
    /// Block identity to block name.
    blocks: HashMap<String, String>,
    /// Block names in use, upper case: names are not case sensitive.
    taken: HashSet<String>,
    /// The plan's drawing of each opening shape on a straight wall.
    canon: HashMap<String, Vec<Item>>,
    dims: usize,
}

impl Builder {
    pub fn new(n: f64) -> Builder {
        Builder {
            doc: Doc::new(),
            n: if n.is_finite() && n > 0.0 { n } else { 100.0 },
            blocks: HashMap::new(),
            taken: HashSet::new(),
            canon: HashMap::new(),
            dims: 0,
        }
    }

    pub fn finish(self) -> Doc {
        self.doc
    }

    // ------------------------------------------------------------- tables

    pub fn ensure_layer(&mut self, name: &str, color: i64, linetype: &str, lineweight: i64) {
        if !self.doc.layers.iter().any(|l| l.name == name) {
            self.doc.layers.push(Layer {
                name: name.to_string(),
                color,
                linetype: linetype.to_string(),
                lineweight,
            });
        }
    }

    /// The layer of a plan category.
    pub fn ensure_cat(&mut self, cat: Cat, color: Option<i64>) {
        let name = cat.dxf_layer();
        self.ensure_layer(name, color.unwrap_or(layer_color(cat)), "CONTINUOUS", pen_lineweight(layer_pen(cat)));
    }

    /// The layers of these plan items, in the order the plan export always
    /// wrote them.
    pub fn ensure_arch_layers(&mut self, items: &[Item], color: Option<i64>) {
        for cat in super::layer_table_cats(items) {
            self.ensure_cat(cat, color);
        }
    }

    /// The linetype of a dash style, sized to plot at 1:`self.n`.
    pub fn ensure_dash(&mut self, d: Dash) -> &'static str {
        let Some((name, description, pattern, count)) = pipes::dxf_linetype(d) else {
            return "CONTINUOUS";
        };
        if !self.doc.linetypes.iter().any(|l| l.name == name) {
            self.doc.linetypes.push(LineType {
                name: name.to_string(),
                description: description.to_string(),
                pattern: pattern[..count].iter().map(|x| x * self.n).collect(),
            });
            // Dashed before dash-dot, whatever order they come in.
            self.doc.linetypes.sort_by_key(|l| l.name != "GUHIT_DASHED");
        }
        name
    }

    pub fn ensure_pipe_layers(&mut self, systems: &[guhit_model::PipeSystem]) {
        for s in systems {
            let lt = self.ensure_dash(pipes::dash(*s));
            self.ensure_layer(pipes::dxf_layer(*s), pipes::dxf_color(*s), lt, lineweight_of(pipes::paper::STROKE_MIN));
        }
    }

    fn dimstyle(&mut self, n: f64, meters: bool) -> String {
        let name = format!("GUHIT-{}{}", n.round() as i64, if meters { "-M" } else { "" });
        if !self.doc.dimstyles.iter().any(|d| d.name == name) {
            self.doc.dimstyles.push(DimStyle { name: name.clone(), scale: n, meters });
        }
        name
    }

    fn take_name(&mut self, wanted: &[String]) -> String {
        for w in wanted {
            if !self.taken.contains(&w.to_uppercase()) {
                self.taken.insert(w.to_uppercase());
                return w.clone();
            }
        }
        let base = wanted.first().cloned().unwrap_or_else(|| "BLOCK".into());
        let mut k = 2;
        loop {
            let name = format!("{base}_{k}");
            if self.taken.insert(name.to_uppercase()) {
                return name;
            }
            k += 1;
        }
    }

    // ----------------------------------------------------------- entities

    fn grow(&mut self, b: Bounds) {
        if !b.is_empty() && b.min.is_finite() && b.max.is_finite() {
            self.doc.bounds.add(b.min);
            self.doc.bounds.add(b.max);
        }
    }

    /// One primitive as entities on `layer`. A solid ink fill is a HATCH
    /// inside its outline; a tint stays an outline.
    pub fn prim_ents(&self, prim: &Prim, layer: &str, lineweight: Option<i64>, linetype: Option<&str>) -> Vec<Ent> {
        let ent = |kind: Kind| Ent {
            layer: layer.to_string(),
            linetype: linetype.map(str::to_string),
            color: None,
            lineweight,
            kind,
        };
        match prim {
            Prim::Line { a, b } => vec![ent(Kind::Line { a: *a, b: *b })],
            Prim::Poly { pts, closed, fill } => {
                if *fill == Fill::Ink && pts.len() >= 3 {
                    vec![
                        Ent::on(layer, Kind::Hatch { paths: vec![pts.iter().map(|p| (*p, 0.0)).collect()] }),
                        ent(Kind::Poly { pts: pts.clone(), closed: true }),
                    ]
                } else {
                    vec![ent(Kind::Poly { pts: pts.clone(), closed: *closed })]
                }
            }
            Prim::Arc { c, r, start_deg, end_deg } => vec![ent(Kind::Arc {
                c: *c,
                r: *r,
                start: *start_deg,
                end: *end_deg,
            })],
            Prim::Circle { c, r, fill } => {
                let circle = ent(Kind::Circle { c: *c, r: *r });
                if *fill == Fill::Ink {
                    // Two half circles: a bulge of 1 is a semicircle.
                    let path = vec![(*c - v(*r, 0.0), 1.0), (*c + v(*r, 0.0), 1.0)];
                    vec![Ent::on(layer, Kind::Hatch { paths: vec![path] }), circle]
                } else {
                    vec![circle]
                }
            }
            Prim::Text { pos, height, rot_deg, align, text, bold } => vec![ent(Kind::Text {
                pos: *pos,
                height: *height,
                rot: *rot_deg,
                middle: *align == HAlign::Middle,
                text: text.clone(),
                width: est_width(text, *height, *bold),
            })],
        }
    }

    /// A loose primitive in model space.
    pub fn add_prim(&mut self, prim: &Prim, layer: &str, lineweight: Option<i64>, linetype: Option<&str>) {
        let item = Item { cat: Cat::Furn, pen: Pen::Light, prim: prim.clone() };
        self.grow(plan::items_bounds(std::slice::from_ref(&item)));
        let ents = self.prim_ents(prim, layer, lineweight, linetype);
        self.doc.ents.extend(ents);
    }

    /// Plan items moved by `shift`, in layer order, each on its category's
    /// layer with its own lineweight when its pen is not the layer's. A
    /// stair seen from above is dashed.
    pub fn add_items(&mut self, items: &[Item], shift: V) {
        for cat in super::dxf_layer_order() {
            for item in items.iter().filter(|i| i.cat == cat) {
                let lw = (item.pen != layer_pen(cat)).then(|| pen_lineweight(item.pen));
                let lt = (cat == Cat::StrsBelow).then(|| self.ensure_dash(super::HIDDEN));
                let prim = super::offset_prim(&item.prim, shift);
                self.add_prim(&prim, cat.dxf_layer(), lw, lt);
            }
        }
    }

    /// Pipe runs as polylines and risers as circles on their system layers,
    /// weighted by size at 1:`n`. Risers stay continuous on dashed layers.
    pub fn add_pipes(&mut self, list: &[PlanPipe], n: f64, shift: V) {
        self.ensure_pipe_layers(&pipes::systems_present(list));
        let pb = pipes::bounds(list, n);
        if !pb.is_empty() {
            self.grow(Bounds { min: pb.min + shift, max: pb.max + shift });
        }
        let layer_lw = lineweight_of(pipes::paper::STROKE_MIN);
        for system in pipes::DRAW_ORDER {
            let mut group: Vec<&PlanPipe> = list.iter().filter(|p| p.system == system).collect();
            group.sort_by(|a, b| b.diameter_mm.partial_cmp(&a.diameter_mm).unwrap_or(std::cmp::Ordering::Equal));
            let layer = pipes::dxf_layer(system);
            for p in &group {
                let lw = lineweight_of(pipes::stroke_width(p.diameter_mm, n));
                for run in &p.runs {
                    let mut e = Ent::on(layer, Kind::Poly { pts: run.iter().map(|q| *q + shift).collect(), closed: false });
                    e.lineweight = (lw != layer_lw).then_some(lw);
                    self.doc.ents.push(e);
                }
            }
            for p in &group {
                let r = pipes::riser_radius(p.diameter_mm, n);
                for c in &p.risers {
                    let mut e = Ent::on(layer, Kind::Circle { c: *c + shift, r });
                    e.linetype = Some("CONTINUOUS".into());
                    self.doc.ents.push(e);
                }
            }
        }
    }

    /// A block of plan items around the origin, on layer 0 so it takes the
    /// layer of its insert. Items drawn with another pen than the insert's
    /// layer carry their own lineweight.
    fn block_ents(&self, items: &[Item], layer_cat: Cat) -> Vec<Ent> {
        let mut out = Vec::new();
        for item in items {
            let lw = (item.pen != layer_pen(layer_cat)).then(|| pen_lineweight(item.pen));
            out.extend(self.prim_ents(&item.prim, "0", lw, None));
        }
        out
    }

    fn insert(&mut self, layer: &str, block: &str, x: Xf, attrs: Vec<(String, String, f64)>, block_items: &[Item]) {
        let moved: Vec<Item> = block_items
            .iter()
            .map(|i| Item { cat: i.cat, pen: i.pen, prim: xf_prim(&i.prim, &x) })
            .collect();
        self.grow(plan::items_bounds(&moved));
        self.doc.ents.push(Ent::on(
            layer,
            Kind::Insert { block: block.to_string(), at: x.at, sx: x.sx, sy: x.sy, rot: x.rot, attrs },
        ));
    }

    // -------------------------------------------------------------- plan

    /// Add one level's plan moved by `shift`. False when it draws nothing.
    pub fn add_plan(&mut self, view: &PlanView, shift: V) -> bool {
        let items = plan::build_items(view.project, view.derived, view.level, &view.options(false));
        let pipe_list = view.pipes();
        self.ensure_arch_layers(&items, None);
        let mut used = vec![false; items.len()];
        self.opening_inserts(view, &items, &mut used, shift);
        self.asset_inserts(view, &items, &mut used, shift);
        let rest: Vec<Item> = items.iter().zip(&used).filter(|(_, u)| !**u).map(|(i, _)| i.clone()).collect();
        self.add_items(&rest, shift);
        let dims = if view.show_dimensions && plan::layer_visible(view.project, LayerKey::Dimensions) {
            self.add_dimensions(view, shift)
        } else {
            0
        };
        if !pipe_list.is_empty() {
            self.add_pipes(&pipe_list, view.n, shift);
        }
        !items.is_empty() || !pipe_list.is_empty() || dims > 0
    }

    /// The plan's own drawing of an opening of this shape, centered on the
    /// origin of a wall along +x, hinge at the start, swinging to +y.
    fn canonical(&mut self, view: &PlanView, shell: &Project, o: &Opening, t: f64) -> Vec<Item> {
        let key = format!("{:?}|{:?}|{:.2}|{:.2}", o.opening_type, o.style, o.width_mm, t);
        if let Some(c) = self.canon.get(&key) {
            return c.clone();
        }
        let len = o.width_mm.abs() + 4000.0;
        let wall = Wall {
            id: "guhit-dxf-block-wall".into(),
            level_id: view.level.id.clone(),
            start: Point { x: -len / 2.0, y: 0.0 },
            end: Point { x: len / 2.0, y: 0.0 },
            thickness_mm: t,
            height_mm: None,
            material_id: None,
            role: WallRole::Building,
        };
        let opening = Opening {
            id: "guhit-dxf-block-opening".into(),
            wall_id: wall.id.clone(),
            offset_mm: len / 2.0,
            flip_side: false,
            flip_hinge: false,
            type_id: None,
            ..o.clone()
        };
        let mut one = shell.clone();
        one.elements = vec![Element::Wall(wall), Element::Opening(opening)];
        let items: Vec<Item> = plan::build_items(&one, &Derived::default(), view.level, &view.options(false))
            .into_iter()
            .filter(|i| matches!(i.cat, Cat::Door | Cat::Glaz))
            .collect();
        self.canon.insert(key, items.clone());
        items
    }

    fn opening_inserts(&mut self, view: &PlanView, items: &[Item], used: &mut [bool], shift: V) {
        if !plan::layer_visible(view.project, LayerKey::Openings) {
            return;
        }
        // The project without its elements: the one-wall plans start from it.
        let shell = Project { elements: Vec::new(), ..view.project.clone() };
        let marks: HashMap<&str, &str> = view
            .derived
            .opening_schedule
            .marks
            .iter()
            .map(|m| (m.opening_id.as_str(), m.mark.as_str()))
            .collect();
        for e in &view.project.elements {
            let Element::Wall(wall) = e else { continue };
            if wall.level_id != view.level.id {
                continue;
            }
            let (s, end) = (V::from(wall.start), V::from(wall.end));
            let Some(u) = (end - s).unit() else { continue };
            let len = (end - s).len();
            for e in &view.project.elements {
                let Element::Opening(o) = e else { continue };
                if o.wall_id != wall.id {
                    continue;
                }
                // The span the plan draws, clamped to the wall. An opening
                // past a wall end is drawn cut, so it stays loose.
                let half = o.width_mm.abs() / 2.0;
                let (a, b) = ((o.offset_mm - half).max(0.0), (o.offset_mm + half).min(len));
                if b - a <= 1.0 || (b - a - o.width_mm.abs()).abs() > 0.5 {
                    continue;
                }
                let t = wall.thickness_mm.abs();
                let canon = self.canonical(view, &shell, o, t);
                if canon.is_empty() {
                    continue;
                }
                let at = s + u * ((a + b) / 2.0);
                let h = if o.flip_hinge { -1.0 } else { 1.0 };
                let sd = if o.flip_side { -1.0 } else { 1.0 };
                let mut tried: Vec<(f64, f64)> = Vec::new();
                for (sx, sy) in [(h, sd), (1.0, sd), (h, 1.0), (1.0, 1.0)] {
                    if tried.contains(&(sx, sy)) {
                        continue;
                    }
                    tried.push((sx, sy));
                    let x = Xf { at, rot: u.angle_deg(), sx, sy };
                    let Some(picks) = match_items(&canon, &x, items, used) else { continue };
                    for i in &picks {
                        used[*i] = true;
                    }
                    let mark = marks
                        .get(o.id.as_str())
                        .map(|m| m.to_string())
                        .filter(|m| !m.is_empty())
                        .or_else(|| {
                            o.type_id
                                .as_ref()
                                .and_then(|id| view.project.opening_type(id))
                                .map(|t| t.mark.clone())
                                .filter(|m| !m.is_empty())
                        })
                        .unwrap_or_default();
                    let kind = match o.opening_type {
                        OpeningType::Door => "DOOR",
                        OpeningType::Window => "WINDOW",
                    };
                    let base = if mark.is_empty() {
                        format!("{kind}-{}-{}", style_word(o.style), mm_text(o.width_mm))
                    } else {
                        block_name(&mark)
                    };
                    let geom = format!("{:?}|{:?}|{:.2}|{:.2}", o.opening_type, o.style, o.width_mm, t);
                    let name = self.opening_block(&base, &geom, &canon, t, o.opening_type);
                    let (cat, layer) = match o.opening_type {
                        OpeningType::Door => (Cat::Door, "A-DOOR"),
                        OpeningType::Window => (Cat::Glaz, "A-GLAZ"),
                    };
                    self.ensure_cat(cat, None);
                    let attr_h = paper::DIM_TEXT * view.n;
                    let attrs = vec![
                        ("MARK".to_string(), mark, attr_h),
                        ("WIDTH".to_string(), mm_text(o.width_mm), attr_h),
                        ("HEIGHT".to_string(), mm_text(o.height_mm), attr_h),
                        ("SILL".to_string(), mm_text(o.sill_mm), attr_h),
                    ];
                    let shifted = Xf { at: at + shift, ..x };
                    self.insert(layer, &name, shifted, attrs, &canon);
                    break;
                }
            }
        }
    }

    fn opening_block(&mut self, base: &str, geom: &str, canon: &[Item], t: f64, kind: OpeningType) -> String {
        let key = format!("opening|{base}|{geom}");
        if let Some(n) = self.blocks.get(&key) {
            return n.clone();
        }
        let name = self.take_name(&[base.to_string(), format!("{base}_WALL{}", t.round() as i64)]);
        let cat = match kind {
            OpeningType::Door => Cat::Door,
            OpeningType::Window => Cat::Glaz,
        };
        let ents = self.block_ents(canon, cat);
        let h = paper::DIM_TEXT * self.n;
        self.doc.blocks.push(Block {
            name: name.clone(),
            anonymous: false,
            ents,
            attdefs: ["MARK", "WIDTH", "HEIGHT", "SILL"].iter().map(|t| AttDef { tag: t.to_string(), height: h }).collect(),
        });
        self.blocks.insert(key, name.clone());
        name
    }

    fn asset_inserts(&mut self, view: &PlanView, items: &[Item], used: &mut [bool], shift: V) {
        if !view.show_assets {
            return;
        }
        for e in &view.project.elements {
            let Element::Asset(a) = e else { continue };
            if a.level_id != view.level.id || !plan::layer_visible(view.project, crate::services::asset_layer(a)) {
                continue;
            }
            let (w, d) = (a.width_mm.abs(), a.depth_mm.abs());
            if w < 1.0 || d < 1.0 {
                continue;
            }
            let at = V::from(a.position);
            // The catalog symbol the sheet prints (`symbols`), else the
            // labelled box.
            let symbol = crate::symbols::furniture(&a.catalog_key, w, d, a.built_in.as_deref(), view.n);
            let local: Vec<Item> = match &symbol {
                Some((prims, _)) => prims.iter().map(|prim| Item { cat: Cat::Furn, pen: Pen::Light, prim: prim.clone() }).collect(),
                None => vec![Item {
                    cat: Cat::Furn,
                    pen: Pen::Light,
                    prim: Prim::Poly { pts: plan::rotated_rect(v(0.0, 0.0), w, d, 0.0), closed: true, fill: Fill::None },
                }],
            };
            let x = Xf { at, rot: a.rotation_deg, sx: 1.0, sy: 1.0 };
            let Some(picks) = match_items(&local, &x, items, used) else { continue };
            for i in &picks {
                used[*i] = true;
            }
            let key = format!("asset|{}|{:.1}|{:.1}|{:?}", a.catalog_key, w, d, a.built_in);
            let labelled = symbol.is_none();
            let name = match self.blocks.get(&key) {
                Some(n) => n.clone(),
                None => {
                    let base = block_name(&a.catalog_key);
                    let name = self.take_name(&[base.clone(), format!("{base}_{}x{}", mm_text(w), mm_text(d))]);
                    let ents = self.block_ents(&local, Cat::Furn);
                    let attdefs = if labelled { vec![AttDef { tag: "NAME".into(), height: paper::ASSET_LABEL * self.n }] } else { vec![] };
                    self.doc.blocks.push(Block { name: name.clone(), anonymous: false, ents, attdefs });
                    self.blocks.insert(key, name.clone());
                    name
                }
            };
            let attrs = if labelled { vec![("NAME".to_string(), clean(&a.name), paper::ASSET_LABEL * view.n)] } else { vec![] };
            self.insert("A-FURN", &name, Xf { at: at + shift, ..x }, attrs, &local);
        }
    }

    // --------------------------------------------------------- dimensions

    fn add_dimensions(&mut self, view: &PlanView, shift: V) -> usize {
        let meters = view.project.settings.display_unit == DisplayUnit::M;
        let mut count = 0;
        for e in &view.project.elements {
            let Element::Dimension(d) = e else { continue };
            if d.level_id != view.level.id {
                continue;
            }
            let mut pts: Vec<V> = vec![V::from(d.a)];
            pts.extend(d.through.iter().map(V::from));
            pts.push(V::from(d.b));
            let pts: Vec<V> = pts.into_iter().map(|p| p + shift).collect();
            count += self.add_dimension(&pts, d.offset_mm, d.text_override.as_deref(), view.n, meters);
        }
        count
    }

    /// A dimension through `pts` (two for a plain dimension, more for a
    /// string), its line `offset` mm to the left of the first to the last
    /// point. One DIMENSION per segment, each with its own `*D` block drawn
    /// exactly as the plan draws a dimension. Returns how many it wrote.
    pub fn add_dimension(&mut self, pts: &[V], offset: f64, text_override: Option<&str>, n: f64, meters: bool) -> usize {
        let (Some(first), Some(last)) = (pts.first().copied(), pts.last().copied()) else { return 0 };
        let Some(u) = (last - first).unit() else { return 0 };
        if pts.iter().any(|p| !p.is_finite()) {
            return 0;
        }
        let nrm = u.left();
        let offset = if offset.is_finite() { offset } else { 0.0 };
        let base = first + nrm * offset;
        let project = |p: V| p + nrm * (base - p).dot(nrm);
        let style = self.dimstyle(n, meters);
        let segments: Vec<(V, V)> = pts
            .windows(2)
            .map(|w| (w[0], w[1]))
            .filter(|(a, b)| (*b - *a).dot(u).abs() > 0.5)
            .collect();
        let single = segments.len() == 1;
        let mut written = 0;
        for (p1, p2) in segments {
            let (q1, q2) = (project(p1), project(p2));
            let measurement = (q2 - q1).dot(u).abs();
            let override_text = text_override.map(clean).filter(|t| single && !t.is_empty());
            let label = override_text.clone().unwrap_or_else(|| format_length(measurement, meters));
            let along = (q2 - q1).unit().unwrap_or(u);
            let mut picture: Vec<Item> = Vec::new();
            let gap0 = paper::DIM_EXT_GAP * n;
            let over = paper::DIM_EXT_OVERSHOOT * n;
            for (p, q) in [(p1, q1), (p2, q2)] {
                let dist = (q - p).dot(nrm);
                let sgn = if dist >= 0.0 { 1.0 } else { -1.0 };
                let gap = gap0.min(dist.abs() * 0.5);
                picture.push(plan::line(Cat::Dims, Pen::Light, p + nrm * (sgn * gap), q + nrm * (sgn * over)));
            }
            let lo = paper::DIM_LINE_OVERSHOOT * n;
            picture.push(plan::line(Cat::Dims, Pen::Light, q1 - along * lo, q2 + along * lo));
            let tick = along.rotated(45.0) * (paper::DIM_TICK * n / 2.0);
            for q in [q1, q2] {
                picture.push(plan::line(Cat::Dims, Pen::Medium, q - tick, q + tick));
            }
            let rot = readable_deg(along.angle_deg());
            let up = dir(rot + 90.0);
            let mid = (q1 + q2) * 0.5;
            let h = paper::DIM_TEXT * n;
            picture.push(Item {
                cat: Cat::Dims,
                pen: Pen::Light,
                prim: Prim::Text {
                    pos: mid + up * (paper::DIM_TEXT_GAP * n),
                    height: h,
                    rot_deg: rot,
                    align: HAlign::Middle,
                    text: label,
                    bold: false,
                },
            });
            self.grow(plan::items_bounds(&picture));
            // Picture entities follow the dimension: color and weight
            // ByBlock, the ticks one pen heavier, as on the sheet.
            let mut ents = Vec::new();
            for item in &picture {
                for mut e in self.prim_ents(&item.prim, "0", None, None) {
                    e.color = Some(0);
                    e.lineweight = Some(if item.pen == Pen::Light { -2 } else { pen_lineweight(item.pen) });
                    ents.push(e);
                }
            }
            for p in [q1, q2, p1, p2] {
                let mut e = Ent::on("0", Kind::Point { p });
                e.color = Some(0);
                ents.push(e);
            }
            self.dims += 1;
            let block = format!("*D{}", self.dims);
            self.taken.insert(block.to_uppercase());
            self.doc.blocks.push(Block { name: block.clone(), anonymous: true, ents, attdefs: vec![] });
            self.ensure_cat(Cat::Dims, None);
            self.doc.ents.push(Ent::on(
                Cat::Dims.dxf_layer(),
                Kind::Dim {
                    block,
                    style: style.clone(),
                    p1,
                    p2,
                    line_at: q2,
                    text_mid: mid + up * (paper::DIM_TEXT_GAP * n + h / 2.0),
                    angle: u.angle_deg(),
                    measurement,
                    text: override_text,
                },
            ));
            written += 1;
        }
        written
    }
}

/// For each item of `canon` put through `x`, a distinct unused plan item
/// that draws the same thing, or None when any is missing.
fn match_items(canon: &[Item], x: &Xf, items: &[Item], used: &[bool]) -> Option<Vec<usize>> {
    let mut picks: Vec<usize> = Vec::new();
    for c in canon {
        let want = xf_prim(&c.prim, x);
        let found = items.iter().enumerate().find(|(i, it)| {
            !used[*i] && !picks.contains(i) && it.cat == c.cat && it.pen == c.pen && same_prim(&it.prim, &want)
        })?;
        picks.push(found.0);
    }
    Some(picks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dxf::entities::EntityType as T;

    fn load(text: &str) -> dxf::Drawing {
        dxf::Drawing::load(&mut text.as_bytes()).expect("the dxf crate reads it")
    }

    fn dims(d: &dxf::Drawing) -> Vec<dxf::entities::RotatedDimension> {
        d.entities()
            .filter_map(|e| match &e.specific {
                T::RotatedDimension(r) => Some(r.clone()),
                _ => None,
            })
            .collect()
    }

    fn block_texts(d: &dxf::Drawing, name: &str) -> Vec<String> {
        d.blocks()
            .find(|b| b.name == name)
            .expect("the block")
            .entities
            .iter()
            .filter_map(|e| match &e.specific {
                T::Text(t) => Some(t.value.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_dimension_is_one_entity_drawn_by_its_own_block() {
        let mut b = Builder::new(100.0);
        assert_eq!(b.add_dimension(&[v(0.0, 0.0), v(4000.0, 0.0)], 1000.0, None, 100.0, false), 1);
        let doc = b.finish();
        assert_eq!(doc.dimstyles, vec![DimStyle { name: "GUHIT-100".into(), scale: 100.0, meters: false }]);
        let block = &doc.blocks[0];
        assert_eq!(block.name, "*D1");
        assert!(block.anonymous);
        let lines = block.ents.iter().filter(|e| matches!(e.kind, Kind::Line { .. })).count();
        assert_eq!(lines, 5, "two extension lines, the dimension line and two ticks");
        assert_eq!(block.ents.iter().filter(|e| matches!(e.kind, Kind::Point { .. })).count(), 4);
        // Extension lines start 1.5 paper mm off the object and run 1.5 mm
        // past the line; the line runs 1.2 mm past them, as on the sheet.
        assert!(block.ents.iter().any(|e| e.kind == Kind::Line { a: v(0.0, 150.0), b: v(0.0, 1150.0) }));
        assert!(block.ents.iter().any(|e| e.kind == Kind::Line { a: v(-120.0, 1000.0), b: v(4120.0, 1000.0) }));

        let text = super::super::doc::write(&doc);
        let d = load(&text);
        let found = dims(&d);
        assert_eq!(found.len(), 1);
        let r = &found[0];
        assert_eq!(r.dimension_base.block_name, "*D1");
        assert_eq!(r.dimension_base.dimension_style_name, "GUHIT-100");
        assert_eq!((r.dimension_base.definition_point_1.x, r.dimension_base.definition_point_1.y), (4000.0, 1000.0));
        assert_eq!((r.definition_point_2.x, r.definition_point_2.y), (0.0, 0.0));
        assert_eq!((r.definition_point_3.x, r.definition_point_3.y), (4000.0, 0.0));
        assert_eq!(r.dimension_base.actual_measurement, 4000.0);
        assert_eq!(r.rotation_angle, 0.0);
        assert_eq!(block_texts(&d, "*D1"), vec!["4000".to_string()]);
        let style = d.dim_styles().find(|s| s.name == "GUHIT-100").expect("the style");
        assert_eq!(style.dimensioning_scale_factor, 100.0);
        assert_eq!(style.dimensioning_text_height, 2.5);
        assert!(style.dimensioning_tick_size > 0.0, "ticks, not arrows");
        // The anonymous block is flagged so.
        assert!(text.contains("  2\n*D1\n 70\n1\n"));
    }

    #[test]
    fn strings_meters_overrides_and_angles() {
        let mut b = Builder::new(50.0);
        // A string through two witness points: three segments on one line.
        let pts = [v(0.0, 0.0), v(2500.0, 0.0), v(5000.0, 300.0), v(8000.0, 0.0)];
        assert_eq!(b.add_dimension(&pts, -800.0, Some("ignored on a string"), 50.0, true), 3);
        // A plain one with its own text.
        assert_eq!(b.add_dimension(&[v(0.0, 0.0), v(3000.0, 3000.0)], 500.0, Some("CLEAR"), 50.0, true), 1);
        // Nothing to measure.
        assert_eq!(b.add_dimension(&[v(10.0, 10.0), v(10.0, 10.0)], 500.0, None, 50.0, false), 0);
        let d = load(&super::super::doc::write(&b.finish()));
        let found = dims(&d);
        assert_eq!(found.len(), 4);
        // Every segment of the string sits on the line 800 mm below.
        for r in &found[..3] {
            assert!((r.dimension_base.definition_point_1.y + 800.0).abs() < 1e-9);
            assert_eq!(r.dimension_base.dimension_style_name, "GUHIT-50-M");
            assert_eq!(r.dimension_base.text, "<>");
        }
        let measured: Vec<f64> = found[..3].iter().map(|r| r.dimension_base.actual_measurement).collect();
        assert_eq!(measured, vec![2500.0, 2500.0, 3000.0]);
        assert_eq!(block_texts(&d, "*D2"), vec!["2.50".to_string()]);
        let angled = &found[3];
        assert_eq!(angled.dimension_base.text, "CLEAR");
        assert!((angled.rotation_angle - 45.0).abs() < 1e-9);
        assert!((angled.dimension_base.actual_measurement - 3000.0 * 2f64.sqrt()).abs() < 1e-3);
        assert_eq!(block_texts(&d, "*D4"), vec!["CLEAR".to_string()]);
        let style = d.dim_styles().find(|s| s.name == "GUHIT-50-M").expect("meters style");
        assert_eq!(style.dimensioning_scale_factor, 50.0);
        assert_eq!(style.dimension_linear_measurement_scale_factor, 0.001);
    }

    #[test]
    fn a_mirrored_arc_runs_the_other_way() {
        let arc = Prim::Arc { c: v(0.0, 0.0), r: 900.0, start_deg: 0.0, end_deg: 90.0 };
        let hinge = Xf { at: v(100.0, 0.0), rot: 0.0, sx: -1.0, sy: 1.0 };
        assert!(same_prim(&xf_prim(&arc, &hinge), &Prim::Arc { c: v(100.0, 0.0), r: 900.0, start_deg: 90.0, end_deg: 180.0 }));
        let both = Xf { at: v(0.0, 0.0), rot: 90.0, sx: -1.0, sy: -1.0 };
        assert!(same_prim(&xf_prim(&arc, &both), &Prim::Arc { c: v(0.0, 0.0), r: 900.0, start_deg: 270.0, end_deg: 0.0 }));
        let line = Prim::Line { a: v(0.0, 0.0), b: v(1.0, 0.0) };
        assert!(same_prim(&xf_prim(&line, &both), &Prim::Line { a: v(0.0, -1.0), b: v(0.0, 0.0) }));
    }
}
