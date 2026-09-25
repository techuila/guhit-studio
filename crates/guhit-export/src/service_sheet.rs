//! The service sheets of one level: lighting, power, plumbing and aircon
//! layouts. Each keeps the plan sheet's frame (border, band, title block,
//! scale logic) and adds a panel with the legend, counts, notes and the
//! blank block of the professional who signs that trade:
//!
//! | Sheet | Draws | Panel |
//! |---|---|---|
//! | lighting | fixtures, switches, links | legend, counts per room, PEE |
//! | power | outlets, SPOs, panelboard, detectors, bells, conduit | legend, counts per room, schedule of loads, PEE |
//! | plumbing | water, drainage, vent, storm, fixtures | legend, fixture table, Master Plumber |
//! | aircon | units, line sets, condensate, core holes | legend, units per room, PME |
//!
//! The architecture is drawn light and thin under the service, furniture as
//! bare outlines. Sheets are coordination drafts: counts come from the
//! model, sizes are as drawn, rating columns and signing fields stay blank.

use guhit_model::{
    Asset, AssetCategory, Derived, DeviceKind, Element, LayerKey, Level, PenetrationKind,
    PipeSystem, PlanExportOptions, Project, ScheduleGroup, ScheduleRow, SheetKind,
};

use crate::geom::*;
use crate::panel::{self, Align, Block, Column, Content, LegendRow, Sample, Table};
use crate::pipes::{self, PlanPipe};
use crate::plan::{
    asset_items, build_items, empty_level, items_bounds, layer_visible, resolve_level, rotated_rect,
    Cat, Fill, Item, Pen, PlanOptions, Prim,
};
use crate::services::{self, Device, Sym};
use crate::sheet::{
    furniture, label_masks, num, pick_scale, write_item_in, write_pipes, Frame, Ink, Layout, North,
    Svg, FONT_FAMILY, INK,
};
use crate::text::{clean, est_width, xml_escape};
use crate::ExportError;

/// Architecture under a service: light blue grey lines, pale wall fill.
pub(crate) const ARCH_LINE: &str = "#98a3b1";
const ARCH_FILL: &str = "#e4e8ee";
const ARCH_TEXT: &str = "#6a7788";

fn arch_pen(p: Pen) -> f64 {
    match p {
        Pen::Heavy => 0.18,
        Pen::Medium => 0.15,
        Pen::Light => 0.13,
        Pen::Fine => 0.1,
    }
}

const ARCH_INK: Ink = Ink {
    fill: ARCH_FILL,
    text: ARCH_TEXT,
    pen: arch_pen,
};

/// Plumbing fixtures on the plumbing sheet: the subject, in ink.
const FIXTURE_INK: Ink = Ink {
    fill: INK,
    text: INK,
    pen: fixture_pen,
};

fn fixture_pen(p: Pen) -> f64 {
    match p {
        Pen::Heavy | Pen::Medium => 0.25,
        Pen::Light => 0.18,
        Pen::Fine => 0.13,
    }
}

/// Drawing title of a sheet: "GROUND FLOOR LIGHTING LAYOUT".
pub(crate) fn sheet_title(sheet: SheetKind, level_name: &str) -> String {
    let what = match sheet {
        SheetKind::Plan => return crate::sheet::drawing_title(level_name),
        SheetKind::Lighting => "LIGHTING LAYOUT",
        SheetKind::Power => "POWER LAYOUT",
        SheetKind::Plumbing => "PLUMBING LAYOUT",
        SheetKind::Aircon => "AIRCON LAYOUT",
        SheetKind::PlumbingIsometric => return "PLUMBING ISOMETRIC DIAGRAMS".into(),
    };
    let upper = clean(level_name).to_uppercase();
    let name = upper.strip_suffix("PLAN").unwrap_or(&upper).trim().to_string();
    if name.is_empty() {
        what.into()
    } else {
        format!("{name} {what}")
    }
}

/// The professional who signs a sheet's trade, as the signing block titles it.
pub(crate) fn signatory(sheet: SheetKind) -> &'static str {
    match sheet {
        SheetKind::Lighting | SheetKind::Power => "PROFESSIONAL ELECTRICAL ENGINEER",
        SheetKind::Aircon => "PROFESSIONAL MECHANICAL ENGINEER",
        _ => "MASTER PLUMBER",
    }
}

fn notes(sheet: SheetKind) -> Vec<String> {
    let lines: &[&str] = match sheet {
        SheetKind::Lighting => &[
            "Coordination draft from the Guhit Studio model, for the PEE.",
            "Dashed arcs join each switch to the lights it controls. S3: a light with two switches.",
            "Circuits, loads, wire and breaker sizes are for the PEE to design.",
        ],
        SheetKind::Power => &[
            "Coordination draft from the Guhit Studio model, for the PEE.",
            "Quantities are counted from the model. Conduit is drawn as modelled.",
            "Circuits, loads, wire and breaker sizes are for the PEE to design.",
        ],
        SheetKind::Aircon => &[
            "Coordination draft from the Guhit Studio model, for the PME.",
            "Core holes: 65 mm, or 90 mm for gas lines from 16 mm, sloped 5 to 7 mm down to the outside.",
            "Unit capacities, line sets and drains are for the PME to design.",
        ],
        _ => &[
            "Coordination draft from the Guhit Studio model, for the Master Plumber.",
            "Pipe sizes and materials as drawn in the model. Sizing and design are for the Master Plumber.",
            "Circles mark risers and drops.",
        ],
    };
    lines.iter().map(|s| s.to_string()).collect()
}

/// Split a heading into two lines at the space nearest its middle.
fn two_lines(s: &str) -> String {
    if s.len() <= 8 || !s.contains(' ') {
        return s.to_string();
    }
    let mid = s.len() / 2;
    let at = s
        .match_indices(' ')
        .map(|(i, _)| i)
        .min_by_key(|i| (*i as i64 - mid as i64).abs())
        .unwrap_or(0);
    format!("{}\n{}", &s[..at], &s[at + 1..])
}

// -------------------------------------------------------------------- panel

/// Blocks placed on the sheet, and the area left for the drawing.
pub(crate) struct PanelPlaced {
    pub svg: String,
    /// x, y, width, height left for the drawing.
    pub area: (f64, f64, f64, f64),
    /// Clip rectangle of the drawing: the inside of the border above the
    /// band, minus the panel.
    pub clip: (f64, f64, f64, f64),
}

/// Stack the blocks in columns on the right of a landscape sheet, or in
/// columns along the bottom of a portrait sheet. When they do not fit, the
/// panel is set in smaller type, down to 70 percent.
pub(crate) fn place_panel(blocks: &[Block], l: &Layout) -> PanelPlaced {
    let m = l.margin;
    let band_top = l.height - m - l.title_h - l.band_h;
    if blocks.is_empty() {
        return PanelPlaced {
            svg: String::new(),
            area: l.plan,
            clip: (m, m, l.width - 2.0 * m, band_top - m),
        };
    }
    // Landscape: one column in smaller type before a second column, which
    // takes width from the drawing. Portrait: the columns are fixed by the
    // paper width, only the type shrinks.
    let steps = [1.0, 0.9, 0.8, 0.7];
    let max_cols = if l.width >= l.height { 3 } else { 1 };
    for cols in 1..=max_cols {
        for f in steps {
            if let Some(p) = try_panel(blocks, l, l.k * f, cols, false) {
                return p;
            }
        }
    }
    try_panel(blocks, l, l.k * 0.7, max_cols, true).expect("a forced panel is always placed")
}

/// One attempt at the panel with type scale `k` and at most `cols` columns
/// on a landscape sheet. None when it does not fit and `force` is off.
fn try_panel(blocks: &[Block], l: &Layout, k: f64, cols_allowed: usize, force: bool) -> Option<PanelPlaced> {
    let (px, py, pw, ph) = l.plan;
    let m = l.margin;
    let gap = 5.0 * l.k;
    let band_top = l.height - m - l.title_h - l.band_h;
    let mut svg = String::from("<g id=\"panel\">\n");
    let place = |svg: &mut String, r: &panel::Rendered, x: f64, y: f64| {
        svg.push_str(&format!("<g transform=\"translate({} {})\">\n", num(x), num(y)));
        svg.push_str(&r.svg);
        svg.push_str("</g>\n");
    };
    if l.width >= l.height {
        let col_w = (0.25 * pw).clamp(62.0 * l.k, 100.0 * l.k) * (k / l.k).max(0.85);
        let max_cols = (((0.6 * pw + gap) / (col_w + gap)).floor() as usize).clamp(1, cols_allowed);
        let mut cols: Vec<Vec<panel::Rendered>> = vec![Vec::new()];
        let mut used = 0.0;
        for b in blocks {
            let r = panel::render(b, col_w, k, ph);
            let need = if used > 0.0 { used + gap + r.h } else { r.h };
            if need > ph && used > 0.0 && cols.len() < max_cols {
                cols.push(Vec::new());
                used = r.h;
            } else {
                used = need;
            }
            if used > ph + 1e-6 && !force {
                return None;
            }
            cols.last_mut().expect("a column").push(r);
        }
        let panel_w = cols.len() as f64 * col_w + (cols.len() - 1) as f64 * gap;
        let x0 = px + pw - panel_w;
        for (i, col) in cols.iter().enumerate() {
            let x = x0 + i as f64 * (col_w + gap);
            let mut y = py;
            for r in col {
                place(&mut svg, r, x, y);
                y += r.h + gap;
            }
        }
        svg.push_str("</g>\n");
        let area_w = pw - panel_w - gap;
        Some(PanelPlaced {
            svg,
            area: (px, py, area_w, ph),
            clip: (m, m, x0 - gap / 2.0 - m, band_top - m),
        })
    } else {
        let min_col = 80.0 * k;
        let ncols = (((pw + gap) / (min_col + gap)).floor() as usize).clamp(1, 3);
        let col_w = (pw - (ncols - 1) as f64 * gap) / ncols as f64;
        let max_h = 0.5 * ph;
        let mut heights = vec![0.0f64; ncols];
        let mut placed: Vec<(usize, f64, panel::Rendered)> = Vec::new();
        for b in blocks {
            let r = panel::render(b, col_w, k, max_h);
            let (c, _) = heights
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .expect("a column");
            let y = if heights[c] > 0.0 { heights[c] + gap } else { 0.0 };
            heights[c] = y + r.h;
            placed.push((c, y, r));
        }
        let panel_h = heights.iter().cloned().fold(0.0, f64::max);
        if panel_h > max_h + 1e-6 && !force {
            return None;
        }
        let y0 = py + ph - panel_h;
        for (c, y, r) in &placed {
            place(&mut svg, r, px + *c as f64 * (col_w + gap), y0 + y);
        }
        svg.push_str("</g>\n");
        Some(PanelPlaced {
            svg,
            area: (px, py, pw, ph - panel_h - gap),
            clip: (m, m, l.width - 2.0 * m, y0 - gap / 2.0 - m),
        })
    }
}

// ------------------------------------------------------------------ content

/// Everything a service sheet draws on the plan area at one scale.
struct Drawing<'a> {
    arch: Vec<Item>,
    /// Furniture and fixtures around the service.
    context: Vec<Item>,
    /// Items of `context` drawn in ink (plumbing fixtures).
    fixtures: Vec<Item>,
    runs: Vec<PlanPipe>,
    devices: Vec<Device<'a>>,
    links: Vec<Prim>,
    core_holes: Vec<CoreHole>,
    /// "CH 65" beside each core hole, placed clear of the symbols, with a
    /// leader when it had to move away from the hole.
    core_labels: Vec<(Prim, Option<(V, V)>)>,
    names: Vec<Item>,
}

/// A wall crossing of a line set or condensate run.
#[derive(Debug, Clone)]
pub(crate) struct CoreHole {
    pub at: V,
    /// 65 or 90 mm.
    pub size: u32,
    /// The run through it: refrigerant or condensate.
    pub system: PipeSystem,
}

impl Drawing<'_> {
    fn bounds(&self, n: f64) -> Bounds {
        let mut b = items_bounds(&self.arch);
        for it in [&self.context, &self.fixtures, &self.names] {
            let ib = items_bounds(it);
            b.add(ib.min);
            b.add(ib.max);
        }
        if !self.runs.is_empty() {
            let pb = pipes::bounds(&self.runs, n);
            b.add(pb.min);
            b.add(pb.max);
        }
        for d in &self.devices {
            let db = d.placed.bounds();
            b.add(db.min);
            b.add(db.max);
        }
        for h in &self.core_holes {
            let r = 0.35 * services::symbol_size(n);
            b.add(h.at - v(r, r));
            b.add(h.at + v(r, r));
        }
        let lb = items_bounds(
            &self
                .core_labels
                .iter()
                .map(|(p, _)| Item { cat: Cat::Furn, pen: Pen::Light, prim: p.clone() })
                .collect::<Vec<Item>>(),
        );
        b.add(lb.min);
        b.add(lb.max);
        b
    }

    fn is_empty(&self) -> bool {
        self.arch.is_empty() && self.runs.is_empty() && self.devices.is_empty() && self.context.is_empty()
    }
}

/// Wall crossings of the aircon runs on a level, from `Derived::pipes`.
pub(crate) fn core_holes(project: &Project, derived: &Derived, level: &Level) -> Vec<CoreHole> {
    derived
        .pipes
        .penetrations
        .iter()
        .filter(|p| p.kind == PenetrationKind::Wall && p.level_id == level.id)
        .filter_map(|p| {
            let pipe = project.elements.iter().find_map(|e| match e {
                Element::Pipe(q) if q.id == p.pipe_id => Some(q),
                _ => None,
            })?;
            if !matches!(pipe.system, PipeSystem::Refrigerant | PipeSystem::Condensate)
                || !layer_visible(project, pipe.system.layer())
            {
                return None;
            }
            let big = pipe.system == PipeSystem::Refrigerant && pipe.diameter_mm >= 16.0;
            let at = v(p.position.x, p.position.y);
            at.is_finite().then_some(CoreHole { at, size: if big { 90 } else { 65 }, system: pipe.system })
        })
        .collect()
}

/// Objects of a level drawn around the service: furniture and fixtures, not
/// symbols. Site objects (plants, vehicles) stay off every service sheet,
/// utilities off all but the sheet of their trade.
fn context_assets<'a>(project: &'a Project, level: &Level, opts: &PlanExportOptions) -> Vec<&'a Asset> {
    if !opts.show_assets {
        return Vec::new();
    }
    project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Asset(a) if a.level_id == level.id => Some(a),
            _ => None,
        })
        .filter(|a| services::symbol_of(a).is_none() && layer_visible(project, services::asset_layer(a)))
        .filter(|a| match a.category {
            AssetCategory::Plant | AssetCategory::Vehicle => false,
            AssetCategory::Utility => is_fixture(a, opts.sheet),
            _ => true,
        })
        .collect()
}

/// Objects drawn in ink with their names: plumbing fixtures and water
/// utilities on the plumbing sheet, the electric meter on the power sheet.
fn is_fixture(a: &Asset, sheet: SheetKind) -> bool {
    match sheet {
        SheetKind::Plumbing => match a.category {
            AssetCategory::Sanitary | AssetCategory::Kitchen | AssetCategory::Appliance => true,
            AssetCategory::Utility => !matches!(a.catalog_key.as_str(), "electric-meter" | "lpg-cylinder"),
            _ => false,
        },
        SheetKind::Power => a.catalog_key == "electric-meter",
        _ => false,
    }
}

fn room_names(project: &Project, derived: &Derived, level: &Level, n: f64, avoid: &[Bounds]) -> Vec<Item> {
    if !layer_visible(project, LayerKey::Rooms) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for e in &project.elements {
        let Element::Room(r) = e else { continue };
        if r.level_id != level.id {
            continue;
        }
        let name = clean(&r.name);
        if name.is_empty() {
            continue;
        }
        let geo = derived.rooms.iter().find(|g| g.room_id == r.id);
        let (at, room_box) = match geo {
            Some(g) => {
                let lp = V::from(g.label_point);
                let b = bounds_of(&g.polygon.iter().map(V::from).collect::<Vec<V>>());
                (if lp.is_finite() { lp } else { V::from(r.seed) }, Some(b))
            }
            None => (V::from(r.seed), None),
        };
        let mut size = 2.2 * n;
        if let Some(b) = room_box {
            let w = est_width(&name, size, true);
            if w > 0.9 * b.width() && w > 0.0 {
                size = (size * 0.9 * b.width() / w).max(1.5 * n);
            }
        }
        let text_at = |c: V| Item {
            cat: Cat::Area,
            pen: Pen::Light,
            prim: services::centered_text(c, &name, size, true),
        };
        // Move the name off the symbols: step down, then up, inside the room.
        let mut chosen = text_at(at);
        for step in [0.0, -1.5, 1.5, -3.0, 3.0, -4.5, 4.5, -6.0, 6.0] {
            let c = at + v(0.0, step * size);
            let cand = text_at(c);
            let cb = items_bounds(std::slice::from_ref(&cand));
            let inside = room_box.map(|b| cb.min.y >= b.min.y && cb.max.y <= b.max.y).unwrap_or(true);
            if inside && !avoid.iter().any(|a| a.intersects(&cb)) {
                chosen = cand;
                break;
            }
        }
        out.push(chosen);
    }
    out
}

fn drawing<'a>(project: &'a Project, derived: &Derived, level: &Level, opts: &PlanExportOptions, n: f64) -> Drawing<'a> {
    let sheet = opts.sheet;
    let arch = build_items(
        project,
        derived,
        level,
        &PlanOptions {
            scale: n,
            show_dimensions: opts.show_dimensions,
            show_room_labels: false,
            show_assets: false,
            unicode: true,
            skip_devices: true,
        },
    );
    let mut context = Vec::new();
    let mut fixtures = Vec::new();
    for a in context_assets(project, level, opts) {
        if is_fixture(a, sheet) {
            asset_items(a, n, &[], &mut fixtures);
        } else if a.width_mm.abs() >= 1.0 && a.depth_mm.abs() >= 1.0 {
            context.push(Item {
                cat: Cat::Furn,
                pen: Pen::Fine,
                prim: Prim::Poly {
                    pts: rotated_rect(V::from(a.position), a.width_mm, a.depth_mm, a.rotation_deg),
                    closed: true,
                    fill: Fill::None,
                },
            });
        }
    }
    let runs: Vec<PlanPipe> = if opts.show_pipes {
        pipes::plan_pipes(project, level)
            .into_iter()
            .filter(|p| pipes::sheet_systems(sheet).contains(&p.system))
            .collect()
    } else {
        Vec::new()
    };
    let devices = services::devices_on(project, level, sheet, n);
    let links = if sheet == SheetKind::Lighting {
        services::link_arcs(project, &devices, n)
    } else {
        Vec::new()
    };
    let core_holes = if sheet == SheetKind::Aircon && opts.show_pipes {
        core_holes(project, derived, level)
    } else {
        Vec::new()
    };
    let mut avoid: Vec<Bounds> = devices.iter().map(|d| d.placed.bounds()).collect();
    let core_labels = core_hole_labels(&core_holes, &runs, &avoid, n);
    avoid.extend(core_labels.iter().map(|(p, _)| {
        items_bounds(std::slice::from_ref(&Item { cat: Cat::Furn, pen: Pen::Light, prim: p.clone() }))
    }));
    let names = if opts.show_room_labels {
        room_names(project, derived, level, n, &avoid)
    } else {
        Vec::new()
    };
    Drawing { arch, context, fixtures, runs, devices, links, core_holes, core_labels, names }
}

/// "CH 65" or "CH 90" beside each core hole: right, left, below or above,
/// the first spot clear of symbols, riser marks and the other labels; else
/// further out with a leader back to the hole.
fn core_hole_labels(holes: &[CoreHole], runs: &[PlanPipe], avoid: &[Bounds], n: f64) -> Vec<(Prim, Option<(V, V)>)> {
    let dd = services::symbol_size(n);
    let r = 0.35 * dd;
    let size = 0.45 * dd;
    let mut taken: Vec<Bounds> = avoid.to_vec();
    for p in runs {
        let rr = pipes::riser_radius(p.diameter_mm, n);
        for c in &p.risers {
            taken.push(Bounds { min: *c - v(rr, rr), max: *c + v(rr, rr) });
        }
    }
    for h in holes {
        taken.push(Bounds { min: h.at - v(r, r), max: h.at + v(r, r) });
    }
    let text_box = |prim: &Prim| items_bounds(std::slice::from_ref(&Item { cat: Cat::Furn, pen: Pen::Light, prim: prim.clone() }));
    let mut out = Vec::new();
    for h in holes {
        let text = format!("CH {}", h.size);
        let (hw, hh) = (est_width(&text, size, false) / 2.0, 0.55 * size);
        let beside = |d: V, dist: f64| h.at + d * (dist + hw * d.x.abs() + hh * d.y.abs());
        let mut near: Vec<(V, bool)> = [0.0, 180.0, 270.0, 90.0, 315.0, 225.0, 45.0, 135.0]
            .iter()
            .map(|deg| (beside(dir(*deg), r + 0.25 * dd), false))
            .collect();
        for k in [2.0, 3.0, 4.0] {
            for deg in [270.0, 90.0, 0.0, 180.0, 315.0, 225.0, 45.0, 135.0] {
                near.push((beside(dir(deg), k * dd), true));
            }
        }
        let mut best: Option<(usize, Prim, Bounds, bool)> = None;
        for (c, leader) in near {
            let prim = services::centered_text(c, &text, size, false);
            let b = text_box(&prim);
            let hits = taken.iter().filter(|t| t.intersects(&b)).count();
            if hits == 0 {
                best = Some((0, prim, b, leader));
                break;
            }
            if best.as_ref().map(|x| hits < x.0).unwrap_or(true) {
                best = Some((hits, prim, b, leader));
            }
        }
        if let Some((_, prim, b, leader)) = best {
            taken.push(b);
            let lead = leader.then(|| {
                let end = v(h.at.x.clamp(b.min.x, b.max.x), h.at.y.clamp(b.min.y, b.max.y));
                let d = (end - h.at).unit().unwrap_or(v(1.0, 0.0));
                (h.at + d * r, end)
            });
            out.push((prim, lead));
        }
    }
    out
}

// ------------------------------------------------------------------- panels

fn fmt_len(mm: f64) -> String {
    format!("{:.1} m", mm / 1000.0)
}

/// Legend rows of a sheet: symbols drawn on this level with their count,
/// the runs with their length, and the marks the sheet uses.
fn legend(project: &Project, derived: &Derived, level: &Level, opts: &PlanExportOptions) -> Vec<LegendRow> {
    let sheet = opts.sheet;
    let d = drawing(project, derived, level, opts, 100.0);
    let dd = services::SYMBOL_PAPER_MM;
    let mut rows = Vec::new();
    let mut syms: Vec<(Sym, bool)> = d.devices.iter().map(|x| (x.sym, x.three_way)).collect();
    syms.sort();
    syms.dedup();
    for (sym, three) in syms {
        let count = d.devices.iter().filter(|x| x.sym == sym && x.three_way == three).count();
        rows.push(LegendRow {
            sample: Sample::Symbol {
                shape: services::legend_shape(sym, dd, three),
                wall: services::legend_wall(sym, dd),
            },
            text: sym.label(three),
            qty: count.to_string(),
        });
    }
    if !d.links.is_empty() {
        rows.push(LegendRow {
            sample: Sample::Link,
            text: "Switch to the lights it controls".into(),
            qty: d.links.len().to_string(),
        });
    }
    // Runs: one row per system drawn, with its length on this level.
    for system in pipes::SYSTEMS {
        if !d.runs.iter().any(|p| p.system == system) {
            continue;
        }
        let length: f64 = project
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Pipe(p) if p.level_id == level.id && p.system == system => Some(pipes::run_length(p)),
                _ => None,
            })
            .sum();
        rows.push(LegendRow {
            sample: Sample::Run(system),
            text: pipes::label(system).to_string(),
            qty: fmt_len(length),
        });
    }
    let risers: usize = d.runs.iter().map(|p| p.risers.len()).sum();
    if risers > 0 {
        let first = d.runs.iter().find(|p| !p.risers.is_empty()).map(|p| p.system).unwrap_or(PipeSystem::ColdWater);
        rows.push(LegendRow {
            sample: Sample::Riser(first),
            text: "Riser or drop".into(),
            qty: risers.to_string(),
        });
    }
    if !d.core_holes.is_empty() {
        rows.push(LegendRow {
            sample: Sample::CoreHole,
            text: "Core hole through a wall".into(),
            qty: d.core_holes.len().to_string(),
        });
    }
    if rows.is_empty() {
        let what = match sheet {
            SheetKind::Lighting => "No lights or switches on this level.",
            SheetKind::Power => "No outlets, panels, detectors or conduit on this level.",
            SheetKind::Aircon => "No aircon units or lines on this level.",
            _ => "No plumbing runs on this level.",
        };
        rows.push(LegendRow {
            sample: Sample::Text { text: "-".into(), bold: false },
            text: what.into(),
            qty: String::new(),
        });
    }
    rows
}

fn counts_block(project: &Project, rows: &[ScheduleRow], level: &Level, kinds: &[DeviceKind], title: &str, empty: &str) -> Block {
    let rc = services::room_counts(project, rows, level, kinds);
    let mut columns = vec![Column { head: "ROOM".into(), align: Align::Left, flex: true, blank_width: 0.0 }];
    for kd in &rc.columns {
        columns.push(Column {
            head: two_lines(services::device_kind_short(*kd)),
            align: Align::Right,
            flex: false,
            blank_width: 0.0,
        });
    }
    let cell = |c: u32| if c == 0 { "-".to_string() } else { c.to_string() };
    let table_rows: Vec<Vec<String>> = rc
        .rows
        .iter()
        .map(|(name, counts)| std::iter::once(name.clone()).chain(counts.iter().map(|c| cell(*c))).collect())
        .collect();
    let total = (!rc.rows.is_empty())
        .then(|| std::iter::once("TOTAL".to_string()).chain(rc.totals.iter().map(|c| cell(*c))).collect());
    Block {
        id: "room-counts".into(),
        title: title.into(),
        content: Content::Table(Table {
            columns,
            rows: table_rows,
            total,
            notes: vec![],
            empty: empty.into(),
        }),
    }
}

fn loads_block(project: &Project) -> Block {
    let rows: Vec<Vec<String>> = services::load_rows(project)
        .into_iter()
        .map(|r| vec![r.circuit, r.description, r.count.to_string(), String::new(), String::new(), String::new()])
        .collect();
    let blank = |head: &str| Column { head: head.into(), align: Align::Center, flex: false, blank_width: 9.0 };
    Block {
        id: "schedule-of-loads".into(),
        title: "SCHEDULE OF LOADS".into(),
        content: Content::Table(Table {
            columns: vec![
                Column { head: "CKT".into(), align: Align::Left, flex: false, blank_width: 0.0 },
                Column { head: "DESCRIPTION".into(), align: Align::Left, flex: true, blank_width: 0.0 },
                Column { head: "QTY".into(), align: Align::Right, flex: false, blank_width: 0.0 },
                blank("RATING\n(VA)"),
                blank("WIRE"),
                blank("BREAKER\n(AT)"),
            ],
            rows,
            total: None,
            notes: vec![
                "Rating, wire and breaker columns are left blank for the PEE to fill in.".into(),
                "Circuit tags as entered in the model, all levels. Untagged objects are under -.".into(),
            ],
            empty: "No lighting outlets, receptacles or other loads in the model yet.".into(),
        }),
    }
}

/// Plumbing fixtures per level, from the schedule.
fn fixtures_block(project: &Project, rows: &[ScheduleRow]) -> Block {
    let plumbing: Vec<&ScheduleRow> = rows.iter().filter(|r| r.group == ScheduleGroup::Plumbing).collect();
    let catalog_order = |key: &str| {
        services::catalog()
            .iter()
            .position(|c| c.key == key)
            .unwrap_or(usize::MAX)
    };
    let mut keys: Vec<&str> = plumbing.iter().map(|r| r.catalog_key.as_str()).collect();
    keys.sort_by_key(|k| (catalog_order(k), k.to_string()));
    keys.dedup();
    let levels: Vec<&Level> = project.levels.iter().collect();
    let many = levels.len() > 1;
    let mut columns = vec![
        Column { head: "TAG".into(), align: Align::Center, flex: false, blank_width: 0.0 },
        Column { head: "FIXTURE".into(), align: Align::Left, flex: true, blank_width: 0.0 },
    ];
    for l in &levels {
        let name = clean(&l.name).to_uppercase();
        columns.push(Column {
            head: two_lines(if name.is_empty() { "LEVEL" } else { &name }),
            align: Align::Right,
            flex: false,
            blank_width: 0.0,
        });
    }
    if many {
        columns.push(Column { head: "TOTAL".into(), align: Align::Right, flex: false, blank_width: 0.0 });
    }
    let mut totals = vec![0u32; levels.len()];
    let table_rows: Vec<Vec<String>> = keys
        .iter()
        .map(|key| {
            let mut row = vec![
                services::fixture_tag(key).unwrap_or("-").to_string(),
                clean(&services::catalog_name(key)),
            ];
            let mut sum = 0;
            for (i, l) in levels.iter().enumerate() {
                let c: u32 = plumbing
                    .iter()
                    .filter(|r| r.level_id == l.id && r.catalog_key == *key)
                    .map(|r| r.count)
                    .sum();
                totals[i] += c;
                sum += c;
                row.push(if c == 0 { "-".into() } else { c.to_string() });
            }
            if many {
                row.push(sum.to_string());
            }
            row
        })
        .collect();
    let total = (!table_rows.is_empty()).then(|| {
        let mut t = vec![String::new(), "TOTAL".to_string()];
        t.extend(totals.iter().map(|c| c.to_string()));
        if many {
            t.push(totals.iter().sum::<u32>().to_string());
        }
        t
    });
    Block {
        id: "fixture-table".into(),
        title: "FIXTURES".into(),
        content: Content::Table(Table {
            columns,
            rows: table_rows,
            total,
            notes: vec!["Counted from the model, per level.".into()],
            empty: "No plumbing fixtures in the model yet.".into(),
        }),
    }
}

/// The panel of a service sheet.
pub(crate) fn blocks(project: &Project, derived: &Derived, level: &Level, opts: &PlanExportOptions) -> Vec<Block> {
    let sheet = opts.sheet;
    let rows = services::schedule_rows(project, derived);
    let mut out = vec![Block {
        id: "legend".into(),
        title: "LEGEND".into(),
        content: Content::Legend(legend(project, derived, level, opts)),
    }];
    match sheet {
        SheetKind::Lighting => out.push(counts_block(
            project,
            &rows,
            level,
            &[DeviceKind::LightingOutlet, DeviceKind::Switch],
            "COUNTS PER ROOM",
            "No lighting outlets or switches on this level.",
        )),
        SheetKind::Power => {
            out.push(counts_block(
                project,
                &rows,
                level,
                &[
                    DeviceKind::ConvenienceReceptacle,
                    DeviceKind::SpecialPurposeOutlet,
                    DeviceKind::Panelboard,
                    DeviceKind::SmokeDetector,
                    DeviceKind::Buzzer,
                    DeviceKind::PushButton,
                ],
                "COUNTS PER ROOM",
                "No outlets, panels, detectors or bells on this level.",
            ));
            out.push(loads_block(project));
        }
        SheetKind::Aircon => out.push(counts_block(
            project,
            &rows,
            level,
            &[DeviceKind::AirconIndoor, DeviceKind::AirconOutdoor, DeviceKind::AirconWindow],
            "UNITS PER ROOM",
            "No aircon units on this level.",
        )),
        SheetKind::Plumbing => out.push(fixtures_block(project, &rows)),
        _ => {}
    }
    out.push(Block { id: "notes".into(), title: "NOTES".into(), content: Content::Notes(notes(sheet)) });
    out.push(Block {
        id: "signatory".into(),
        title: signatory(sheet).into(),
        content: Content::Signatory,
    });
    out
}

// ------------------------------------------------------------------- render

/// Render a lighting, power, plumbing or aircon sheet. Returns the SVG and
/// the scale used.
pub(crate) fn render(project: &Project, derived: &Derived, opts: &PlanExportOptions) -> Result<(String, u32), ExportError> {
    let level = resolve_level(project, opts.level_id.as_ref())?;
    let layout = Layout::new(opts.paper, opts.orientation, opts.title_block);
    if drawing(project, derived, level, opts, 100.0).is_empty() {
        return Err(empty_level(level));
    }
    let blocks = blocks(project, derived, level, opts);
    let panel = place_panel(&blocks, &layout);
    let (px, py, pw, ph) = panel.area;
    let scale = match opts.scale_denominator {
        Some(n) if n > 0 => n,
        _ => pick_scale(pw, ph, |n| {
            let b = drawing(project, derived, level, opts, n as f64).bounds(n as f64);
            (b.width(), b.height())
        }),
    };
    let n = scale as f64;
    let d = drawing(project, derived, level, opts, n);
    let bounds = d.bounds(n);
    let center = bounds.center();
    let (ox, oy) = (px + pw / 2.0, py + ph / 2.0);
    let to_paper = |p: V| -> (f64, f64) { (ox + (p.x - center.x) / n, oy - (p.y - center.y) / n) };

    let (w, h) = (layout.width, layout.height);
    let title = crate::service_sheet::sheet_title(opts.sheet, &level.name);
    let mut svg = Svg { s: String::new() };
    svg.s.push_str(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\" font-family=\"{FONT_FAMILY}\">\n",
        num(w),
        num(h),
        num(w),
        num(h)
    ));
    svg.s.push_str(&format!(
        "<title>{}</title>\n",
        xml_escape(&format!("{} - {} - 1:{}", clean(&project.name), crate::sheet::title_case(&title), scale))
    ));
    svg.s.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>\n",
        num(w),
        num(h)
    ));
    let (cx, cy, cw, ch) = panel.clip;
    svg.s.push_str(&format!(
        "<defs><clipPath id=\"plan-clip\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath></defs>\n",
        num(cx),
        num(cy),
        num(cw),
        num(ch)
    ));
    svg.s.push_str(&format!(
        "<g id=\"plan\" clip-path=\"url(#plan-clip)\" stroke-linecap=\"butt\" stroke-linejoin=\"miter\" fill=\"none\" stroke=\"{ARCH_LINE}\">\n"
    ));
    svg.s.push_str("<g id=\"architecture\">\n");
    for cat in Cat::ALL {
        for item in d.arch.iter().filter(|i| i.cat == cat) {
            write_item_in(&mut svg.s, item, n, &to_paper, &ARCH_INK);
        }
    }
    svg.s.push_str("</g>\n");
    if !d.context.is_empty() {
        svg.s.push_str("<g id=\"context\">\n");
        for item in &d.context {
            write_item_in(&mut svg.s, item, n, &to_paper, &ARCH_INK);
        }
        svg.s.push_str("</g>\n");
    }
    if !d.fixtures.is_empty() {
        svg.s.push_str(&format!("<g id=\"fixtures\" stroke=\"{INK}\">\n"));
        for item in &d.fixtures {
            write_item_in(&mut svg.s, item, n, &to_paper, &FIXTURE_INK);
        }
        svg.s.push_str("</g>\n");
    }
    if !d.names.is_empty() {
        let names: Vec<&Item> = d.names.iter().collect();
        svg.s.push_str("<g id=\"room-label-masks\">\n");
        label_masks(&mut svg.s, &names, n, &to_paper);
        svg.s.push_str("</g>\n");
    }
    if !d.runs.is_empty() {
        write_pipes(&mut svg.s, &d.runs, n, &to_paper);
    }
    if !d.core_holes.is_empty() {
        write_core_holes(&mut svg.s, &d.core_holes, &d.core_labels, n, &to_paper);
    }
    if !d.links.is_empty() {
        svg.s.push_str(&format!(
            "<g id=\"links\" stroke=\"{INK}\" stroke-dasharray=\"{}\">\n",
            panel::LINK_DASH
        ));
        for arc in &d.links {
            let item = Item { cat: Cat::Furn, pen: Pen::Light, prim: arc.clone() };
            write_item_in(&mut svg.s, &item, n, &to_paper, &panel::SYMBOL_INK);
        }
        svg.s.push_str("</g>\n");
    }
    if !d.devices.is_empty() {
        write_devices(&mut svg.s, &d.devices, n, &to_paper);
    }
    if !d.names.is_empty() {
        svg.s.push_str("<g id=\"room-names\">\n");
        for item in &d.names {
            write_item_in(&mut svg.s, item, n, &to_paper, &ARCH_INK);
        }
        svg.s.push_str("</g>\n");
    }
    svg.s.push_str("</g>\n");
    svg.s.push_str(&panel.svg);

    let frame = Frame {
        title,
        scale_text: format!("SCALE 1:{scale}"),
        bar: Some(scale),
        north: North::Plan,
        legend: &[],
        block_scale: format!("1:{scale}"),
    };
    furniture(&mut svg, project, opts, &layout, &frame);
    svg.s.push_str("</svg>\n");
    Ok((svg.s, scale))
}

/// Device symbols in navy, one group per object with its catalog key and
/// printed label, for the tests and for anyone reading the SVG.
fn write_devices(s: &mut String, devices: &[Device], n: f64, to_paper: &dyn Fn(V) -> (f64, f64)) {
    s.push_str(&format!(
        "<g id=\"devices\" fill=\"none\" stroke=\"{INK}\" stroke-linecap=\"round\" stroke-linejoin=\"round\">\n"
    ));
    for d in devices {
        let label = match &d.shape.labels.first() {
            Some(l) => l.text.clone(),
            None => String::new(),
        };
        s.push_str(&format!(
            "<g class=\"device\" data-key=\"{}\" data-label=\"{}\">\n",
            xml_escape(&d.asset.catalog_key),
            xml_escape(&label)
        ));
        for m in &d.placed.marks {
            let item = Item { cat: Cat::Furn, pen: m.pen, prim: m.prim.clone() };
            write_item_in(s, &item, n, to_paper, &panel::SYMBOL_INK);
        }
        for l in &d.placed.labels {
            let item = Item { cat: Cat::Furn, pen: Pen::Light, prim: l.clone() };
            write_item_in(s, &item, n, to_paper, &panel::SYMBOL_INK);
        }
        s.push_str("</g>\n");
    }
    s.push_str("</g>\n");
}

/// Core hole marks: a circle with a slash, and the hole size beside it.
fn write_core_holes(
    s: &mut String,
    holes: &[CoreHole],
    labels: &[(Prim, Option<(V, V)>)],
    n: f64,
    to_paper: &dyn Fn(V) -> (f64, f64),
) {
    let dd = services::symbol_size(n);
    let r = 0.35 * dd;
    s.push_str(&format!("<g id=\"core-holes\" stroke=\"{INK}\">\n"));
    for (i, h) in holes.iter().enumerate() {
        let mut items = vec![
            Item { cat: Cat::Furn, pen: Pen::Light, prim: Prim::Circle { c: h.at, r, fill: Fill::None } },
            Item {
                cat: Cat::Furn,
                pen: Pen::Light,
                prim: Prim::Line { a: h.at + dir(225.0) * r, b: h.at + dir(45.0) * r },
            },
        ];
        if let Some((l, lead)) = labels.get(i) {
            items.push(Item { cat: Cat::Furn, pen: Pen::Light, prim: l.clone() });
            if let Some((a, b)) = lead {
                items.push(Item { cat: Cat::Furn, pen: Pen::Fine, prim: Prim::Line { a: *a, b: *b } });
            }
        }
        s.push_str(&format!(
            "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"#ffffff\" stroke=\"none\"/>\n",
            num(to_paper(h.at).0),
            num(to_paper(h.at).1),
            num(r / n)
        ));
        for it in &items {
            write_item_in(s, it, n, to_paper, &panel::SYMBOL_INK);
        }
    }
    s.push_str("</g>\n");
}

// ---------------------------------------------------------------------- DXF

/// Block name of a device: its catalog key in capitals, with the rotation
/// when it is not 0 ("OUTLET-DUPLEX_R90"). Symbols keep their labels upright,
/// so the rotation is part of the block's geometry.
fn block_base(a: &Asset) -> String {
    let key: String = a
        .catalog_key
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_uppercase() } else { '_' })
        .collect();
    let key = if key.is_empty() { "DEVICE".to_string() } else { key };
    let rot = norm_deg(if a.rotation_deg.is_finite() { a.rotation_deg } else { 0.0 }).round() as i64 % 360;
    if rot == 0 {
        key
    } else {
        format!("{key}_R{rot}")
    }
}

/// A key for block geometry, so identical symbols share one block.
fn signature(prims: &[Prim]) -> String {
    let r = |x: f64| format!("{:.2}", x);
    prims
        .iter()
        .map(|p| match p {
            Prim::Line { a, b } => format!("L{},{},{},{}", r(a.x), r(a.y), r(b.x), r(b.y)),
            Prim::Poly { pts, closed, fill } => format!(
                "P{}{}{}",
                closed,
                *fill == Fill::Ink,
                pts.iter().map(|q| format!("{},{}", r(q.x), r(q.y))).collect::<Vec<_>>().join(";")
            ),
            Prim::Arc { c, r: rr, start_deg, end_deg } => format!("A{},{},{},{},{}", r(c.x), r(c.y), r(*rr), r(*start_deg), r(*end_deg)),
            Prim::Circle { c, r: rr, fill } => format!("C{},{},{},{}", r(c.x), r(c.y), r(*rr), *fill == Fill::Ink),
            Prim::Text { pos, height, text, .. } => format!("T{},{},{},{}", r(pos.x), r(pos.y), r(*height), text),
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// Layer colors of the service layers (AutoCAD color index).
fn service_layer_color(layer: &str) -> i64 {
    match layer {
        services::LAYER_LITE_FIXT | services::LAYER_LITE_CIRC => 2,
        services::LAYER_POWR_DEVC => 1,
        _ => 6,
    }
}

/// The DXF of a service sheet: the architecture on its usual layers in grey,
/// the sheet's runs on their system layers, devices as blocks with the
/// attributes TYPE, TAG (the circuit tag), HEIGHT (of the device center,
/// mm) and ROOM on `E-LITE-FIXT`, `E-POWR-DEVC` or `M-HVAC-EQPM`, switch
/// links as arcs on `E-LITE-CIRC`, core holes on the run layer. Returns the
/// text and the scale the symbols and text are sized for.
pub(crate) fn dxf(project: &Project, derived: &Derived, opts: &PlanExportOptions) -> Result<(String, u32), ExportError> {
    let level = resolve_level(project, opts.level_id.as_ref())?;
    let scale = opts
        .scale_denominator
        .filter(|n| *n > 0)
        .or(Some(project.settings.scale_denominator).filter(|n| *n > 0))
        .unwrap_or(100);
    let n = scale as f64;
    let items = build_items(
        project,
        derived,
        level,
        &PlanOptions {
            scale: n,
            show_dimensions: opts.show_dimensions,
            show_room_labels: opts.show_room_labels,
            show_assets: opts.show_assets,
            unicode: false,
            skip_devices: true,
        },
    );
    let d = drawing(project, derived, level, opts, n);
    if items.is_empty() && d.runs.is_empty() && d.devices.is_empty() {
        return Err(empty_level(level));
    }
    let rooms = services::Rooms::new(project, derived);
    let mut parts = crate::dxf::Parts::new(format!(
        "{} for {}. Units: millimeters, model space 1:1, y is north. Symbols and text sized for plotting at 1:{scale}",
        crate::sheet::title_case(&sheet_title(opts.sheet, &level.name)),
        crate::text::clean(&project.name)
    ));
    parts.items = &items;
    parts.arch_color = Some(8);
    parts.pipes = &d.runs;
    let attr_h = 0.4 * services::symbol_size(n);
    let mut layers_used: Vec<&'static str> = Vec::new();
    for dev in &d.devices {
        let a = dev.asset;
        let at = V::from(a.position);
        let mut prims: Vec<Prim> = dev.placed.marks.iter().map(|m| crate::dxf::offset_prim(&m.prim, -at)).collect();
        prims.extend(dev.placed.labels.iter().map(|l| crate::dxf::offset_prim(l, -at)));
        let sig = signature(&prims);
        let base = block_base(a);
        let mut name = base.clone();
        let mut k = 1;
        loop {
            match parts.blocks.iter().position(|b| b.name == name) {
                None => {
                    parts.blocks.push(crate::dxf::BlockDef {
                        name: name.clone(),
                        prims,
                        attrs: vec!["TYPE", "TAG", "HEIGHT", "ROOM"],
                        attr_height: attr_h,
                    });
                    break;
                }
                Some(i) if signature(&parts.blocks[i].prims) == sig => break,
                Some(_) => {
                    k += 1;
                    name = format!("{base}_{k}");
                }
            }
        }
        let room = rooms
            .room_at(&a.level_id, at)
            .or_else(|| {
                let front = services::to_world(v(0.0, -a.depth_mm.abs() / 2.0 - 10.0), at, a.rotation_deg);
                rooms.room_at(&a.level_id, front)
            })
            .map(|r| clean(&r.name))
            .unwrap_or_default();
        let layer = dev.sym.dxf_layer();
        if !layers_used.contains(&layer) {
            layers_used.push(layer);
        }
        parts.inserts.push(crate::dxf::Insert {
            layer,
            block: name,
            at,
            attrs: vec![
                ("TYPE", services::type_label(a)),
                ("TAG", clean(&a.circuit)),
                ("HEIGHT", format!("{:.0}", a.elevation_mm + a.height_mm / 2.0)),
                ("ROOM", room),
            ],
            attr_height: attr_h,
        });
    }
    if !d.links.is_empty() {
        layers_used.push(services::LAYER_LITE_CIRC);
        parts.dashes.push(pipes::Dash::Dashed);
        for arc in &d.links {
            parts.ents.push(crate::dxf::Ent {
                layer: services::LAYER_LITE_CIRC.to_string(),
                prim: arc.clone(),
                linetype: None,
            });
        }
    }
    let dd = services::symbol_size(n);
    let r = 0.35 * dd;
    for (i, h) in d.core_holes.iter().enumerate() {
        let layer = pipes::dxf_layer(h.system).to_string();
        let mut prims = vec![
            Prim::Circle { c: h.at, r, fill: Fill::None },
            Prim::Line { a: h.at + dir(225.0) * r, b: h.at + dir(45.0) * r },
        ];
        if let Some((l, lead)) = d.core_labels.get(i) {
            prims.push(l.clone());
            if let Some((a, b)) = lead {
                prims.push(Prim::Line { a: *a, b: *b });
            }
        }
        for prim in prims {
            parts.ents.push(crate::dxf::Ent { layer: layer.clone(), prim, linetype: Some("CONTINUOUS") });
        }
    }
    // Core holes sit on their run's layer, which exists when the run is drawn.
    let drawn = pipes::systems_present(&d.runs);
    let mut hole_systems: Vec<PipeSystem> = d.core_holes.iter().map(|h| h.system).collect();
    hole_systems.sort();
    hole_systems.dedup();
    for s in hole_systems {
        if !drawn.contains(&s) {
            parts.layers.push(crate::dxf::ExtraLayer {
                name: pipes::dxf_layer(s).to_string(),
                color: pipes::dxf_color(s),
                linetype: "CONTINUOUS",
            });
        }
    }
    for layer in layers_used {
        parts.layers.push(crate::dxf::ExtraLayer {
            name: layer.to_string(),
            color: service_layer_color(layer),
            linetype: if layer == services::LAYER_LITE_CIRC { "GUHIT_DASHED" } else { "CONTINUOUS" },
        });
    }
    Ok((crate::dxf::write_parts(&parts, scale), scale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_name_the_level_and_the_trade() {
        assert_eq!(sheet_title(SheetKind::Lighting, "Ground Floor"), "GROUND FLOOR LIGHTING LAYOUT");
        assert_eq!(sheet_title(SheetKind::Power, "Second floor plan"), "SECOND FLOOR POWER LAYOUT");
        assert_eq!(sheet_title(SheetKind::Aircon, ""), "AIRCON LAYOUT");
        assert_eq!(sheet_title(SheetKind::PlumbingIsometric, "Ground Floor"), "PLUMBING ISOMETRIC DIAGRAMS");
        assert_eq!(two_lines("LIGHTING OUTLETS"), "LIGHTING\nOUTLETS");
        assert_eq!(two_lines("SPO"), "SPO");
    }
}
