//! DXF export, as DXF 2013 (AC1027, `doc` and `build`): UTF-8 text
//! ("Parañaque", "m²"), `$INSUNITS` 4 (millimeters), layers with the plan's
//! pens as lineweights, a block per door and window type and per catalog
//! object placed as INSERTs, real DIMENSION entities with a dimension style
//! and their anonymous blocks, and solid HATCH for cut walls and columns.
//! `plan` writes one level, `plans` every level side by side, `sheets` the
//! views of the sheet set side by side (plan 1.6), `write_parts` the
//! services and isometric sheets and `write` drawings that are only items.
//!
//! The 3D DXF (`dxf3d`) stays R12 and borrows the pipe, linetype and text
//! helpers here.
//!
//! Model space, millimeters, 1:1, y north (DXF is y-up like the model).
//!
//! Pipes go on one layer per system (`pipes::dxf_layer`). Drainage and vent
//! layers use the dashed and dash-dot linetypes this file defines, with the
//! pattern sized to plot at 1:N. Pipe layers and linetypes are only written
//! when the drawing has pipes.
//!
//! Slabs go on `A-FLOR-OTLN` and railings on `A-FLOR-HRAL`, layers written
//! only when something is on them, like `L-SITE`. A stair seen from the level
//! it arrives at stays on `A-FLOR-STRS` with the `GUHIT_DASHED` linetype.

mod build;
mod doc;
mod sheets;

pub use sheets::{plans, sheets};

use guhit_model::{Derived, PipeSystem, PlanExportOptions, Project};

use crate::{ExportError, PlanOutput};

use crate::geom::*;
use crate::pipes::{self, Dash, PlanPipe};
use crate::plan::{items_bounds, Cat, Item, Prim};

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

/// Text for the R12 3D DXF, plain ASCII. Other characters use the \U+XXXX
/// escape that current CAD programs understand.
pub fn dxf_text(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '\u{00b2}' => out.push('2'),
            '\n' | '\r' | '\t' => out.push(' '),
            c if c.is_ascii() && !c.is_ascii_control() => out.push(c),
            c if (c as u32) <= 0xffff => out.push_str(&format!("\\U+{:04X}", c as u32)),
            _ => out.push('?'),
        }
    }
    out
}

/// One group code and value, in the layout both writers use.
fn put(s: &mut String, code: i32, value: &str) {
    s.push_str(&format!("{code:>3}\n{value}\n"));
}

fn put_int(s: &mut String, code: i32, value: i64) {
    put(s, code, &format!("{value:>6}"));
}

fn put_num(s: &mut String, code: i32, value: f64) {
    put(s, code, &f(value));
}

// ---------------------------------------------------------------------- pipes

/// The dash styles, beyond CONTINUOUS, that layers of these systems use.
pub(crate) fn pipe_dashes(systems: &[PipeSystem]) -> Vec<Dash> {
    [Dash::Dashed, Dash::DashDot]
        .into_iter()
        .filter(|d| systems.iter().any(|s| pipes::dash(*s) == *d))
        .collect()
}

/// LTYPE entries for the pipe dash styles, sized to plot at 1:`scale`.
pub(crate) fn write_pipe_linetypes(s: &mut String, dashes: &[Dash], scale: f64) {
    for d in dashes {
        let Some((name, description, pattern, count)) = pipes::dxf_linetype(*d) else {
            continue;
        };
        let pattern = &pattern[..count];
        put(s, 0, "LTYPE");
        put(s, 2, name);
        put_int(s, 70, 0);
        put(s, 3, description);
        put_int(s, 72, 65);
        put_int(s, 73, count as i64);
        put_num(s, 40, pattern.iter().map(|x| x.abs()).sum::<f64>() * scale);
        for x in pattern {
            put_num(s, 49, x * scale);
        }
    }
}

/// Linetype name of a pipe layer.
pub(crate) fn pipe_linetype(system: PipeSystem) -> &'static str {
    pipes::dxf_linetype(pipes::dash(system))
        .map(|t| t.0)
        .unwrap_or("CONTINUOUS")
}

/// LAYER entries for the pipe systems.
pub(crate) fn write_pipe_layers(s: &mut String, systems: &[PipeSystem]) {
    for system in systems {
        put(s, 0, "LAYER");
        put(s, 2, pipes::dxf_layer(*system));
        put_int(s, 70, 0);
        put_int(s, 62, pipes::dxf_color(*system));
        put(s, 6, pipe_linetype(*system));
    }
}

/// Pipe runs as open polylines and risers as circles, at z = 0, each on its
/// system layer. Riser circles are sized for plotting at 1:`scale` and stay
/// continuous on the dashed layers.
pub(crate) fn write_pipe_entities(s: &mut String, list: &[PlanPipe], scale: f64) {
    for system in pipes::DRAW_ORDER {
        let mut group: Vec<&PlanPipe> = list.iter().filter(|p| p.system == system).collect();
        group.sort_by(|a, b| {
            b.diameter_mm
                .partial_cmp(&a.diameter_mm)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let layer = pipes::dxf_layer(system);
        for p in &group {
            for run in &p.runs {
                put(s, 0, "POLYLINE");
                put(s, 8, layer);
                put_int(s, 66, 1);
                put_num(s, 10, 0.0);
                put_num(s, 20, 0.0);
                put_num(s, 30, 0.0);
                put_int(s, 70, 0);
                for q in run {
                    put(s, 0, "VERTEX");
                    put(s, 8, layer);
                    put_num(s, 10, q.x);
                    put_num(s, 20, q.y);
                    put_num(s, 30, 0.0);
                }
                put(s, 0, "SEQEND");
                put(s, 8, layer);
            }
        }
        for p in &group {
            let r = pipes::riser_radius(p.diameter_mm, scale);
            for c in &p.risers {
                put(s, 0, "CIRCLE");
                put(s, 8, layer);
                put(s, 6, "CONTINUOUS");
                put_num(s, 10, c.x);
                put_num(s, 20, c.y);
                put_num(s, 30, 0.0);
                put_num(s, 40, r);
            }
        }
    }
}

/// One level's plan as DXF 2013: the level `opts.level_id` (None: the
/// first) at `opts.scale_denominator`, else the project scale, else 1:100,
/// which sizes text, ticks, dimension styles and dashes. Paper and title
/// block options do not apply to model space. `opts.sheet` is not read:
/// the services sheets go through their own writers.
pub fn plan(project: &Project, derived: &Derived, opts: &PlanExportOptions) -> Result<PlanOutput<String>, ExportError> {
    let level = crate::plan::resolve_level(project, opts.level_id.as_ref())?;
    let scale = sheets::plan_scale(project, opts);
    let mut b = build::Builder::new(scale as f64);
    b.doc.comments = vec![
        "Guhit Studio plan export".into(),
        format!("DXF 2013. Units: millimeters, model space 1:1, y is north. Text sized for plotting at 1:{scale}"),
    ];
    let view = build::PlanView {
        project,
        derived,
        level,
        n: scale as f64,
        show_dimensions: opts.show_dimensions,
        show_room_labels: opts.show_room_labels,
        show_assets: opts.show_assets,
        show_pipes: opts.show_pipes,
    };
    if !b.add_plan(&view, v(0.0, 0.0)) {
        return Err(crate::plan::empty_level(level));
    }
    Ok(PlanOutput {
        data: doc::write(&b.finish()),
        scale_denominator: scale,
    })
}

/// Plan items and pipes as a DXF 2013 document, for drawings that are only
/// items (the reflected ceiling plan, views drawn elsewhere): layers,
/// lineweights, hatches and UTF-8 as `plan`, without the blocks and
/// dimensions only the model gives. `scale` sizes the dashes and riser
/// marks and goes into the header comment; geometry is 1:1.
pub fn write(items: &[Item], pipe_list: &[PlanPipe], scale: u32) -> String {
    write_parts(
        &Parts {
            items,
            pipes: pipe_list,
            ..Parts::new(format!(
                "Units: millimeters, model space 1:1, y is north. Text sized for plotting at 1:{scale}"
            ))
        },
        scale,
    )
}

/// A layer a sheet adds after the architecture and pipe layers.
pub(crate) struct ExtraLayer {
    pub name: String,
    pub color: i64,
    pub linetype: &'static str,
}

/// A block: geometry around its insertion point on layer 0 (so it takes the
/// layer of the insert), and the tags of its attributes.
pub(crate) struct BlockDef {
    pub name: String,
    pub prims: Vec<Prim>,
    pub attrs: Vec<&'static str>,
    pub attr_height: f64,
}

/// A block reference with its attribute values, all invisible.
pub(crate) struct Insert {
    pub layer: &'static str,
    pub block: String,
    pub at: V,
    pub attrs: Vec<(&'static str, String)>,
    pub attr_height: f64,
}

/// A loose entity on a named layer. Solid fills become hatches inside their
/// outline.
pub(crate) struct Ent {
    pub layer: String,
    pub prim: Prim,
    /// Overrides the layer's linetype.
    pub linetype: Option<&'static str>,
}

/// Everything one DXF file holds.
pub(crate) struct Parts<'a> {
    /// Second header comment.
    pub comment: String,
    pub items: &'a [Item],
    /// ACI color of the architecture layers; None keeps the plan colors.
    pub arch_color: Option<i64>,
    /// Write the nine architecture layers into the layer table. A diagram
    /// without architecture leaves them out.
    pub arch_layers: bool,
    pub pipes: &'a [PlanPipe],
    pub layers: Vec<ExtraLayer>,
    /// Dash styles the extra layers or entities use.
    pub dashes: Vec<Dash>,
    pub blocks: Vec<BlockDef>,
    pub inserts: Vec<Insert>,
    pub ents: Vec<Ent>,
}

impl<'a> Parts<'a> {
    pub(crate) fn new(comment: String) -> Parts<'a> {
        Parts {
            comment,
            items: &[],
            arch_color: None,
            arch_layers: true,
            pipes: &[],
            layers: Vec::new(),
            dashes: Vec::new(),
            blocks: Vec::new(),
            inserts: Vec::new(),
            ents: Vec::new(),
        }
    }
}

fn prim_item(p: &Prim) -> Item {
    Item {
        cat: Cat::Furn,
        pen: crate::plan::Pen::Light,
        prim: p.clone(),
    }
}

/// A services or isometric sheet as DXF 2013 from its parts: the plan
/// linework, pipes, the sheet's own layers, blocks with their inserts and
/// loose entities. The layer table holds exactly the layers the parts name.
pub(crate) fn write_parts(p: &Parts, scale: u32) -> String {
    let n = scale as f64;
    let mut b = build::Builder::new(n);
    b.doc.comments = vec!["Guhit Studio plan export".into(), p.comment.clone()];
    let systems = pipes::systems_present(p.pipes);
    let hidden = p.items.iter().any(|i| i.cat == Cat::StrsBelow);
    for d in [Dash::Dashed, Dash::DashDot] {
        let wanted = systems.iter().any(|s| pipes::dash(*s) == d) || p.dashes.contains(&d) || (hidden && d == HIDDEN);
        if wanted {
            b.ensure_dash(d);
        }
    }
    if p.arch_layers {
        b.ensure_arch_layers(p.items, p.arch_color);
    }
    b.ensure_pipe_layers(&systems);
    for l in &p.layers {
        b.ensure_layer(&l.name, l.color, l.linetype, doc::lineweight_of(0.25));
    }
    for bd in &p.blocks {
        let mut ents = Vec::new();
        for prim in &bd.prims {
            ents.extend(b.prim_ents(prim, "0", None, None));
        }
        b.doc.blocks.push(doc::Block {
            name: bd.name.clone(),
            anonymous: false,
            ents,
            attdefs: bd.attrs.iter().map(|t| doc::AttDef { tag: t.to_string(), height: bd.attr_height }).collect(),
        });
    }
    b.add_items(p.items, v(0.0, 0.0));
    b.add_pipes(p.pipes, n, v(0.0, 0.0));
    for ins in &p.inserts {
        if let Some(bd) = p.blocks.iter().find(|bd| bd.name == ins.block) {
            let moved: Vec<Item> = bd.prims.iter().map(|q| prim_item(&offset_prim(q, ins.at))).collect();
            let bb = items_bounds(&moved);
            if !bb.is_empty() {
                b.doc.bounds.add(bb.min);
                b.doc.bounds.add(bb.max);
            }
        }
        b.doc.bounds.add(ins.at);
        b.doc.ents.push(doc::Ent::on(
            ins.layer,
            doc::Kind::Insert {
                block: ins.block.clone(),
                at: ins.at,
                sx: 1.0,
                sy: 1.0,
                rot: 0.0,
                attrs: ins.attrs.iter().map(|(t, value)| (t.to_string(), value.clone(), ins.attr_height)).collect(),
            },
        ));
    }
    for e in &p.ents {
        b.add_prim(&e.prim, &e.layer, None, e.linetype);
    }
    doc::write(&b.finish())
}

/// A primitive moved by `at`.
pub(crate) fn offset_prim(p: &Prim, at: V) -> Prim {
    let t = |q: V| q + at;
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
            start_deg: *start_deg,
            end_deg: *end_deg,
        },
        Prim::Circle { c, r, fill } => Prim::Circle { c: t(*c), r: *r, fill: *fill },
        Prim::Text { pos, height, rot_deg, align, text, bold } => Prim::Text {
            pos: t(*pos),
            height: *height,
            rot_deg: *rot_deg,
            align: *align,
            text: text.clone(),
            bold: *bold,
        },
    }
}

/// Layer and entity order, walls first. `StrsBelow` draws on the stairs
/// layer and has no table entry of its own.
pub fn dxf_layer_order() -> [Cat; 16] {
    [
        Cat::Wall,
        Cat::Site,
        Cat::Slab,
        Cat::Door,
        Cat::Glaz,
        Cat::Cols,
        Cat::Strs,
        Cat::StrsBelow,
        Cat::Rail,
        Cat::Roof,
        Cat::Furn,
        Cat::Area,
        Cat::Dims,
        Cat::Grid,
        Cat::Symb,
        Cat::Text,
    ]
}

/// Categories whose layer is written only when an item is on it.
pub(crate) fn optional_cat(cat: Cat) -> bool {
    matches!(cat, Cat::Site | Cat::Slab | Cat::Rail | Cat::Roof | Cat::Grid | Cat::Symb)
}

/// The categories with a layer table entry for these items, in order.
pub(crate) fn layer_table_cats(items: &[Item]) -> Vec<Cat> {
    dxf_layer_order()
        .into_iter()
        .filter(|c| *c != Cat::StrsBelow && (!optional_cat(*c) || items.iter().any(|i| i.cat == *c)))
        .collect()
}

/// The dash style of hidden lines: a stair seen from above its top.
pub(crate) const HIDDEN: Dash = Dash::Dashed;

/// Linetype name of hidden lines, `GUHIT_DASHED`.
pub(crate) fn hidden_linetype() -> &'static str {
    pipes::dxf_linetype(HIDDEN).map(|t| t.0).unwrap_or("CONTINUOUS")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_text() {
        assert_eq!(f(8000.0), "8000.0");
        assert_eq!(f(-75.5), "-75.5");
        assert_eq!(f(-0.00001), "0.0");
        assert_eq!(f(0.12345678), "0.1235");
        assert_eq!(dxf_text("28.52 m\u{00b2}"), "28.52 m2");
        assert_eq!(dxf_text("Ba\u{00f1}o"), "Ba\\U+00F1o");
    }
}
