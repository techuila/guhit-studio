//! Several drawings side by side in one model space, at 1:1 in mm: every
//! level's plan in one file (`plans`), or the views of the sheet set
//! (`sheets`, plan 1.6), one row per printed sheet in set order and its
//! viewports left to right. Each drawing keeps its own scale for text and
//! dimension styles, and its title and scale sit under it.
//!
//! The sheet set comes from `sheetset::printed_sheets`, so the DXF holds
//! the sheets the PDF prints (a long schedule's continuation sheets
//! included), each view at the scale the sheet layout chose and the scale
//! line only on views drawn to scale. Each sheet's row has a border and a
//! title block with the project, the sheet and "N OF M"; the license
//! numbers print on the PDF set only.
//!
//! A level's architectural plan is drawn here with blocks, dimensions and
//! hatches (`Builder::add_plan`). Schedules are tables of lines and text
//! (`tables::items`). Every other view comes from `views::items` (lane L3).
//! A view with nothing to draw, or an image (a perspective), is left out
//! and named in the file's header comments; no error text goes into the
//! drawing.

use guhit_model::{Derived, Id, PlanExportOptions, Project, SheetKind, ViewRef};

use super::build::{Builder, PlanView};
use super::doc;
use crate::geom::*;
use crate::plan::{items_bounds, Cat, HAlign, Item, Prim};
use crate::sheetset::{self, PrintedScale, PrintedSheet};
use crate::text::clean;
use crate::{tables, ExportError, PlanOutput, SheetInputs};

/// Paper sizes of the titles under each drawing, mm.
const TITLE: f64 = 4.0;
const SCALE_NOTE: f64 = 2.5;
const HEADING: f64 = 6.0;
/// Paper space between drawings and between rows, mm.
const GAP: f64 = 30.0;
/// The title block of a sheet's row: its width and type sizes, paper mm.
const BLOCK_W: f64 = 110.0;
const LABEL: f64 = 1.6;
const VALUE: f64 = 2.5;
const NUMBER: f64 = 7.0;
/// The layer of sheet borders and title blocks.
const TTLB: &str = "A-ANNO-TTLB";

enum Content<'a> {
    Plan(PlanView<'a>),
    Items(Vec<Item>, Bounds),
}

struct Slot<'a> {
    title: String,
    n: f64,
    /// "SCALE 1:50", "NOT TO SCALE", or none for a table.
    scale_note: Option<String>,
    content: Content<'a>,
}

fn text(b: &mut Builder, at: V, height: f64, s: &str) {
    text_on(b, Cat::Text.dxf_layer(), at, height, s, false);
}

fn text_on(b: &mut Builder, layer: &str, at: V, height: f64, s: &str, bold: bool) {
    let s = clean(s);
    if s.is_empty() {
        return;
    }
    if layer == Cat::Text.dxf_layer() {
        b.ensure_cat(Cat::Text, None);
    }
    let prim = Prim::Text { pos: at, height, rot_deg: 0.0, align: HAlign::Start, text: s, bold };
    b.add_prim(&prim, layer, None, None);
}

/// Lay one row of drawings out with their tops at `top`, a heading above.
/// Returns the y of the row's bottom, titles included, and the x where
/// the next drawing would go.
fn row(b: &mut Builder, heading: Option<&str>, slots: Vec<Slot>, top: f64) -> (f64, f64) {
    let n_max = slots.iter().map(|s| s.n).fold(0.0, f64::max).max(1.0);
    if let Some(h) = heading {
        text(b, v(0.0, top + 2.0 * HEADING * n_max), HEADING * n_max, h);
    }
    let mut x = 0.0;
    let mut bottom = top;
    for slot in slots {
        let n = slot.n;
        let extent = match &slot.content {
            Content::Plan(view) => view.extent(),
            Content::Items(_, bounds) => *bounds,
        };
        let (w, h, min) = if extent.is_empty() {
            (100.0 * n, 60.0 * n, v(0.0, -60.0 * n))
        } else {
            (extent.width(), extent.height(), v(extent.min.x, extent.max.y - extent.height()))
        };
        let shift = v(x - min.x, top - h - min.y);
        match &slot.content {
            Content::Plan(view) => {
                b.add_plan(view, shift);
            }
            Content::Items(items, _) => {
                b.ensure_arch_layers(items, None);
                b.add_items(items, shift);
            }
        }
        let under = top - h - 2.0 * TITLE * n;
        text(b, v(x, under), TITLE * n, &slot.title);
        let mut low = under;
        if let Some(note) = &slot.scale_note {
            text(b, v(x, under - 1.8 * TITLE * n), SCALE_NOTE * n, note);
            low = under - 1.8 * TITLE * n - SCALE_NOTE * n;
        }
        bottom = bottom.min(low);
        x += w + GAP * n;
    }
    (bottom, x)
}

fn scale_of(project: &Project, wanted: Option<u32>) -> u32 {
    wanted
        .filter(|n| *n > 0)
        .or(Some(project.settings.scale_denominator).filter(|n| *n > 0))
        .unwrap_or(100)
}

/// The plan export's scale: the options', else the project's, else 100.
pub(crate) fn plan_scale(project: &Project, opts: &PlanExportOptions) -> u32 {
    scale_of(project, opts.scale_denominator)
}

fn plan_view<'a>(p: &'a Project, d: &'a Derived, level: &'a guhit_model::Level, n: f64) -> PlanView<'a> {
    PlanView {
        project: p,
        derived: d,
        level,
        n,
        show_dimensions: true,
        show_room_labels: true,
        show_assets: true,
        show_pipes: true,
    }
}

/// Every level's plan side by side, left to right in level order, each
/// titled. Levels with nothing to draw are left out.
pub fn plans(project: &Project, derived: &Derived, opts: &PlanExportOptions) -> Result<PlanOutput<String>, ExportError> {
    let scale = plan_scale(project, opts);
    let n = scale as f64;
    let slots: Vec<Slot> = project
        .levels
        .iter()
        .map(|level| Slot {
            title: crate::sheet::drawing_title(&clean(&level.name)),
            n,
            scale_note: Some(format!("SCALE 1:{scale}")),
            content: Content::Plan(PlanView {
                show_dimensions: opts.show_dimensions,
                show_room_labels: opts.show_room_labels,
                show_assets: opts.show_assets,
                show_pipes: opts.show_pipes,
                ..plan_view(project, derived, level, n)
            }),
        })
        .filter(|s| match &s.content {
            Content::Plan(view) => !view.extent().is_empty(),
            _ => true,
        })
        .collect();
    if slots.is_empty() {
        return Err(ExportError::Empty("no level has anything to draw".into()));
    }
    let mut b = Builder::new(n);
    b.doc.comments = vec![
        "Guhit Studio plans of every level".into(),
        format!("DXF 2013. Units: millimeters, model space 1:1, y is north. Text sized for plotting at 1:{scale}"),
    ];
    row(&mut b, None, slots, 0.0);
    Ok(PlanOutput {
        data: doc::write(&b.finish()),
        scale_denominator: scale,
    })
}

/// The sheets `ids` (empty: every sheet, in set order; the default set
/// when the project has none) as printed, their views side by side in
/// model space.
pub fn sheets(p: &Project, d: &Derived, ids: &[Id]) -> Result<String, ExportError> {
    let printed = sheetset::printed_sheets(p, d, ids, &SheetInputs::default())?;
    if printed.iter().all(|s| s.viewports.is_empty()) {
        return Err(ExportError::Empty("the sheets have no views".into()));
    }
    let project_n = scale_of(p, None) as f64;
    let scaled = |s: &PrintedSheet| {
        s.viewports
            .iter()
            .filter_map(|vp| match vp.scale {
                PrintedScale::Scaled(n) => Some(n as f64),
                _ => None,
            })
            .collect::<Vec<f64>>()
    };
    let first = printed.iter().flat_map(scaled).next().unwrap_or(project_n);
    let mut b = Builder::new(first);
    let mut left_out: Vec<String> = Vec::new();
    let mut top = 0.0;
    for sheet in &printed {
        // Tables, notes and pictures size their type for the sheet's
        // largest scale, so they sit with its drawings.
        let n_sheet = scaled(sheet).into_iter().fold(0.0, f64::max);
        let n_sheet = if n_sheet > 0.0 { n_sheet } else { project_n };
        let name = format!("{} {}", clean(&sheet.number), clean(&sheet.title).to_uppercase());
        // What the printed sheet itself leaves out.
        left_out.extend(sheet.left_out.iter().map(|w| format!("{name}: {w}")));
        let mut slots: Vec<Slot> = Vec::new();
        for vp in &sheet.viewports {
            let (n, scale_note) = match vp.scale {
                PrintedScale::Scaled(n) => (n as f64, Some(format!("SCALE 1:{n}"))),
                PrintedScale::NotToScale => (n_sheet, Some("NOT TO SCALE".to_string())),
                PrintedScale::None => (n_sheet, None),
            };
            let content = match &vp.viewport.view {
                ViewRef::Plan { level_id, sheet: SheetKind::Plan } => match p.levels.iter().find(|l| &l.id == level_id) {
                    Some(level) => Content::Plan(plan_view(p, d, level, n)),
                    None => {
                        left_out.push(format!("{name}: {} (its level was deleted)", vp.title));
                        continue;
                    }
                },
                ViewRef::Schedule { schedule } => {
                    let mut t = tables::schedule(p, d, *schedule);
                    if let Some(pt) = &vp.part {
                        t = tables::part(&t, pt.from, pt.to, pt.continued_on.as_deref());
                    }
                    let items = tables::items(&t, 1.0, n);
                    let bounds = items_bounds(&items);
                    Content::Items(items, bounds)
                }
                ViewRef::Perspective { .. } => {
                    left_out.push(format!("{name}: {} (a model render, printed on the PDF set)", vp.title));
                    continue;
                }
                view => match crate::views::items(p, d, view, Some(n as u32)) {
                    Ok(vi) if !vi.items.is_empty() && !vi.bounds.is_empty() => Content::Items(vi.items, vi.bounds),
                    _ => {
                        left_out.push(format!("{name}: {} (nothing to draw)", vp.title));
                        continue;
                    }
                },
            };
            slots.push(Slot { title: vp.title.clone(), n, scale_note, content });
        }
        let n_row = slots.iter().map(|s| s.n).fold(n_sheet, f64::max);
        let heading = format!("{}  {}", clean(&sheet.number), clean(&sheet.title).to_uppercase());
        // A sheet whose views are all left out keeps its place and its
        // title block, so the set counts the same as the PDF.
        let (bottom, right) = if slots.is_empty() {
            text(&mut b, v(0.0, top + 2.0 * HEADING * n_row), HEADING * n_row, &heading);
            (top - 60.0 * n_row, 100.0 * n_row)
        } else {
            row(&mut b, Some(&heading), slots, top)
        };
        let frame_bottom = title_block(&mut b, p, sheet, right, top + 3.5 * HEADING * n_row, bottom - 10.0 * n_row, n_row);
        top = frame_bottom - (GAP + 3.5 * HEADING) * n_row;
    }
    b.doc.comments = vec![
        "Guhit Studio sheet set, every view in model space".into(),
        "DXF 2013. Units: millimeters, model space 1:1, y is north. Each view is titled with its scale".into(),
        "License numbers print on the PDF set only".into(),
    ];
    b.doc.comments.extend(left_out.into_iter().map(|w| format!("Not drawn here: {w}")));
    Ok(doc::write(&b.finish()))
}

/// The border of a sheet's row, from x 0 (less a margin) to the title
/// block at `x`, and the title block: project, owner, location, lot, firm,
/// architect, the sheet's contents, scale, date, number and "N OF M", as
/// the PDF's strip has them. Returns the border's bottom.
fn title_block(b: &mut Builder, p: &Project, sheet: &PrintedSheet, x: f64, top: f64, bottom: f64, n: f64) -> f64 {
    b.ensure_layer(TTLB, 7, "CONTINUOUS", 35);
    let tb = &p.title_block;
    let lot = [("Lot", &tb.lot_no), ("Blk", &tb.block_no), ("TCT", &tb.tct_no)]
        .iter()
        .filter(|(_, v)| !v.trim().is_empty())
        .map(|(k, v)| format!("{k} {}", clean(v)))
        .collect::<Vec<_>>()
        .join(", ");
    let project = if tb.project_title.trim().is_empty() { &p.name } else { &tb.project_title };
    let fields: Vec<(&str, String)> = vec![
        ("PROJECT", clean(project)),
        ("OWNER", clean(&p.settings.client_name)),
        ("LOCATION", clean(&p.settings.location)),
        ("LOT, BLOCK AND TCT", lot),
        ("FIRM", clean(&tb.firm_name)),
        ("ARCHITECT", clean(&tb.signatory.name)),
        ("SHEET CONTENTS", clean(&sheet.title).to_uppercase()),
        ("SCALE", sheet.scale.clone()),
        ("DATE", sheet.date.clone()),
    ];
    let cell = 9.0 * n;
    let number_h = 16.0 * n;
    let need = cell * fields.len() as f64 + number_h + 6.0 * n;
    let bottom = bottom.min(top - need);
    let (x0, x1) = (x, x + BLOCK_W * n);
    let line = |b: &mut Builder, a: V, c: V| b.add_prim(&Prim::Line { a, b: c }, TTLB, None, None);
    // The border round the row and the block.
    let left = -GAP * n / 2.0;
    let corners = [v(left, bottom), v(x1, bottom), v(x1, top), v(left, top)];
    for i in 0..4 {
        line(b, corners[i], corners[(i + 1) % 4]);
    }
    line(b, v(x0, bottom), v(x0, top));
    let pad = 2.5 * n;
    let mut y = top;
    for (label, value) in &fields {
        text_on(b, TTLB, v(x0 + pad, y - 2.0 * LABEL * n), LABEL * n, label, false);
        text_on(b, TTLB, v(x0 + pad, y - cell + 2.2 * n), VALUE * n, value, false);
        y -= cell;
        line(b, v(x0, y), v(x1, y));
    }
    text_on(b, TTLB, v(x0 + pad, y - 2.0 * LABEL * n), LABEL * n, "SHEET NO.", false);
    text_on(b, TTLB, v(x0 + pad, y - number_h + 3.0 * n), NUMBER * n, &sheet.number, true);
    text_on(b, TTLB, v(x1 - 26.0 * n, y - number_h + 3.0 * n), VALUE * n, &format!("{} OF {}", sheet.index, sheet.total), true);
    text_on(b, TTLB, v(x0 + pad, bottom + 2.0 * n), LABEL * n, "Drawn in Guhit Studio", false);
    bottom
}
