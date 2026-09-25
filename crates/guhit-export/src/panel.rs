//! The panel beside or under the drawing on the service sheets: the legend,
//! count tables, the schedule of loads, notes and the blank block of the
//! signing professional. Laid out in paper mm; each block renders at a given
//! width and reports its height, so the sheet can stack blocks in columns.
//!
//! Wording rule: counts and blank columns only. Nothing here states a rating,
//! a size or a compliance result; the signing fields stay empty.

use guhit_model::PipeSystem;

use crate::geom::*;
use crate::pipes;
use crate::plan::{items_bounds, Cat, Item, Pen, Prim};
use crate::services::{self, Shape};
use crate::sheet::{dash_attr, legend_pattern, line_cap, num, write_item_in, Ink, INK, MUTED, PLAN_INK};
use crate::text::{est_width, fit, xml_escape};

/// Light rules between table rows and columns.
const RULE: &str = "#b9c2cd";

/// A legend sample, drawn in the left cell of a legend row.
pub(crate) enum Sample {
    /// A device symbol built with D in paper mm, drawn as placed with no
    /// rotation. `wall` is the local y of the wall face to draw, if any.
    Symbol { shape: Shape, wall: Option<f64> },
    /// A run in its system's color and line style.
    Run(PipeSystem),
    /// A run as the isometric diagrams draw it.
    IsoRun(PipeSystem),
    /// A riser or drop: a white circle in the system color.
    Riser(PipeSystem),
    /// A dashed arc: a switch and the lights it controls.
    Link,
    /// Core hole through a wall.
    CoreHole,
    /// Flow direction on a drain.
    Arrow,
    /// A text sample such as "20 PPR" or "CWR-1".
    Text { text: String, bold: bool },
    /// A crossing: the rear line is broken.
    Crossing,
}

pub(crate) struct LegendRow {
    pub sample: Sample,
    pub text: String,
    /// Quantity on this level, or empty.
    pub qty: String,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Align {
    Left,
    Right,
    Center,
}

pub(crate) struct Column {
    /// Heading; a newline splits it into two lines.
    pub head: String,
    pub align: Align,
    /// Takes the width that is left, and gives it up first.
    pub flex: bool,
    /// Width in mm at k = 1 for a column that stays empty (blank for hand
    /// entries). 0: from the content.
    pub blank_width: f64,
}

pub(crate) struct Table {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<String>>,
    pub total: Option<Vec<String>>,
    /// Lines printed under the table.
    pub notes: Vec<String>,
    /// Printed instead of the table when there are no rows.
    pub empty: String,
}

pub(crate) enum Content {
    Legend(Vec<LegendRow>),
    Table(Table),
    Notes(Vec<String>),
    /// Name, PRC number, signature and seal, all blank.
    Signatory,
}

pub(crate) struct Block {
    /// SVG group id.
    pub id: String,
    pub title: String,
    pub content: Content,
}

pub(crate) struct Rendered {
    pub svg: String,
    pub h: f64,
}

/// Words split into lines no wider than `width` at `size`.
pub(crate) fn wrap(text: &str, size: f64, width: f64) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let candidate = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if est_width(&candidate, size, false) <= width || cur.is_empty() {
            cur = candidate;
        } else {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

struct Out {
    s: String,
}

impl Out {
    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, x: f64, y: f64, size: f64, anchor: &str, bold: bool, color: &str, text: &str) {
        if text.is_empty() {
            return;
        }
        self.s.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"{anchor}\"{} fill=\"{color}\">{}</text>\n",
            num(x),
            num(y),
            num(size),
            if bold { " font-weight=\"bold\"" } else { "" },
            xml_escape(text)
        ));
    }

    fn line(&mut self, a: (f64, f64), b: (f64, f64), w: f64, color: &str) {
        self.s.push_str(&format!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{color}\" stroke-width=\"{}\"/>\n",
            num(a.0),
            num(a.1),
            num(b.0),
            num(b.1),
            num(w)
        ));
    }

    #[allow(clippy::too_many_arguments)]
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, sw: f64, stroke: &str, fill: &str) {
        self.s.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{}\"/>\n",
            num(x),
            num(y),
            num(w),
            num(h),
            num(sw)
        ));
    }
}

/// Height of the title strip of every block.
fn head_h(k: f64) -> f64 {
    5.6 * k
}

/// Render a block `w` mm wide, at most `max_h` mm tall (tables drop rows to
/// fit). Coordinates start at the block's top left corner.
pub(crate) fn render(b: &Block, w: f64, k: f64, max_h: f64) -> Rendered {
    let mut o = Out { s: String::new() };
    let pad = 1.8 * k;
    let body = match &b.content {
        Content::Legend(rows) => legend(&mut o, rows, w, k),
        Content::Table(t) => table(&mut o, t, w, k, max_h - head_h(k)),
        Content::Notes(lines) => notes(&mut o, lines, w, k),
        Content::Signatory => signatory(&mut o, w, k),
    };
    let h = head_h(k) + body;
    let mut s = format!("<g id=\"{}\">\n", b.id);
    // Frame first, so the content draws over its white fill.
    s.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\" stroke=\"{INK}\" stroke-width=\"0.25\"/>\n",
        num(w),
        num(h)
    ));
    let (title, size) = fit(&b.title, 2.2 * k, 1.6 * k, w - 2.0 * pad - 10.0 * k, true);
    s.push_str(&format!(
        "<text x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"start\" font-weight=\"bold\" fill=\"{INK}\">{}</text>\n",
        num(pad),
        num(4.0 * k),
        num(size),
        xml_escape(&title)
    ));
    if let Content::Legend(rows) = &b.content {
        if rows.iter().any(|r| !r.qty.is_empty()) {
            s.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"end\" fill=\"{MUTED}\">QTY</text>\n",
                num(w - pad),
                num(4.0 * k),
                num(1.35 * k)
            ));
        }
    }
    s.push_str(&format!(
        "<line x1=\"0\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{INK}\" stroke-width=\"0.18\"/>\n",
        num(head_h(k)),
        num(w),
        num(head_h(k))
    ));
    s.push_str(&o.s);
    s.push_str("</g>\n");
    Rendered { svg: s, h }
}

// -------------------------------------------------------------------- legend

fn legend(o: &mut Out, rows: &[LegendRow], w: f64, k: f64) -> f64 {
    let pad = 1.8 * k;
    let cell = 14.0 * k;
    let base_h = 4.6 * k;
    let size = 1.8 * k;
    let qty_w = rows
        .iter()
        .map(|r| est_width(&r.qty, size, false))
        .fold(0.0, f64::max);
    let mut y = head_h(k) + 0.8 * k;
    for r in rows {
        let (sample_svg, sample_h) = sample(&r.sample, pad + cell / 2.0, cell - 1.5 * k, k);
        let row_h = base_h.max(sample_h + 1.4 * k);
        let cy = y + row_h / 2.0;
        o.s.push_str(&format!("<g transform=\"translate(0 {})\">\n{sample_svg}</g>\n", num(cy)));
        let tx = pad + cell + 1.2 * k;
        let avail = w - tx - pad - if qty_w > 0.0 { qty_w + 2.0 * k } else { 0.0 };
        let (t, ts) = fit(&r.text, size, 1.4 * k, avail.max(5.0), false);
        o.text(tx, cy + 0.36 * ts, ts, "start", false, INK, &t);
        o.text(w - pad, cy + 0.36 * size, size, "end", false, INK, &r.qty);
        y += row_h;
    }
    if rows.is_empty() {
        y += base_h;
    }
    y + 0.8 * k - head_h(k)
}

/// SVG of one legend sample centered at (cx, 0), at most `max_w` wide.
/// Returns the markup and the sample's height.
fn sample(s: &Sample, cx: f64, max_w: f64, k: f64) -> (String, f64) {
    let mut o = Out { s: String::new() };
    let len = (10.0 * k).min(max_w);
    let x0 = cx - len / 2.0;
    let x1 = cx + len / 2.0;
    match s {
        Sample::Symbol { shape, wall } => symbol_sample(shape, *wall, cx, max_w),
        Sample::Run(system) => {
            let d = pipes::dash(*system);
            let width = 0.5;
            o.s.push_str(&format!(
                "<line x1=\"{}\" y1=\"0\" x2=\"{}\" y2=\"0\" stroke=\"{}\" stroke-width=\"{}\" stroke-linecap=\"{}\"{}/>\n",
                num(x0),
                num(x1),
                pipes::color(*system),
                num(width),
                line_cap(d),
                dash_attr(&legend_pattern(d, width, len))
            ));
            (o.s, 1.0)
        }
        Sample::IsoRun(system) => {
            let d = crate::iso::iso_dash(*system);
            let width = crate::iso::iso_width(if *system == PipeSystem::Drainage { 100.0 } else { 20.0 });
            o.s.push_str(&format!(
                "<line x1=\"{}\" y1=\"0\" x2=\"{}\" y2=\"0\" stroke=\"{}\" stroke-width=\"{}\" stroke-linecap=\"{}\"{}/>\n",
                num(x0),
                num(x1),
                pipes::color(*system),
                num(width),
                line_cap(d),
                dash_attr(&legend_pattern(d, width, len))
            ));
            (o.s, 1.0)
        }
        Sample::Riser(system) => {
            o.line((x0, 0.0), (cx - pipes::paper::RISER_MIN_R, 0.0), 0.35, pipes::color(*system));
            o.s.push_str(&format!(
                "<circle cx=\"{}\" cy=\"0\" r=\"{}\" fill=\"#ffffff\" stroke=\"{}\" stroke-width=\"{}\"/>\n",
                num(cx),
                num(pipes::paper::RISER_MIN_R),
                pipes::color(*system),
                num(pipes::paper::RISER_STROKE)
            ));
            (o.s, 2.0 * pipes::paper::RISER_MIN_R + 0.3)
        }
        Sample::Link => {
            o.s.push_str(&format!(
                "<path d=\"M{} {}Q{} {} {} {}\" fill=\"none\" stroke=\"{INK}\" stroke-width=\"0.18\" stroke-dasharray=\"{}\"/>\n",
                num(x0),
                num(0.9 * k),
                num(cx),
                num(-1.6 * k),
                num(x1),
                num(0.9 * k),
                LINK_DASH
            ));
            (o.s, 2.0 * k)
        }
        Sample::CoreHole => {
            let r = 0.35 * services::SYMBOL_PAPER_MM;
            o.s.push_str(&format!(
                "<circle cx=\"{}\" cy=\"0\" r=\"{}\" fill=\"#ffffff\" stroke=\"{INK}\" stroke-width=\"0.18\"/>\n",
                num(cx),
                num(r)
            ));
            let d = r * std::f64::consts::FRAC_1_SQRT_2;
            o.line((cx - d, d), (cx + d, -d), 0.18, INK);
            (o.s, 2.0 * r)
        }
        Sample::Arrow => {
            let (a, b) = (cx + 1.2, cx - 1.2);
            o.line((x0, 0.0), (x1, 0.0), 0.35, pipes::color(PipeSystem::Drainage));
            o.s.push_str(&format!(
                "<polygon points=\"{},0 {},{} {},{}\" fill=\"{INK}\" stroke=\"none\"/>\n",
                num(a),
                num(b),
                num(-0.7),
                num(b),
                num(0.7)
            ));
            (o.s, 1.6)
        }
        Sample::Text { text, bold } => {
            let size = 1.7 * k;
            let (t, ts) = fit(text, size, 1.2 * k, max_w, *bold);
            o.text(cx, 0.36 * ts, ts, "middle", *bold, INK, &t);
            (o.s, ts)
        }
        Sample::Crossing => {
            let gap = 0.9;
            o.line((x0, 0.0), (x1, 0.0), 0.35, pipes::color(PipeSystem::ColdWater));
            let top = -2.6 * k;
            let bottom = 2.6 * k;
            o.line((cx, top), (cx, -gap), 0.35, pipes::color(PipeSystem::HotWater));
            o.line((cx, gap), (cx, bottom), 0.35, pipes::color(PipeSystem::HotWater));
            (o.s, 5.2 * k)
        }
    }
}

/// Dash of the link arcs on paper, mm.
pub(crate) const LINK_DASH: &str = "1.2 0.8";

/// A device symbol as a legend sample: placed with no rotation (the wall
/// face at the top), scaled down only if wider than the cell.
fn symbol_sample(shape: &Shape, wall: Option<f64>, cx: f64, max_w: f64) -> (String, f64) {
    let placed = services::place(shape, v(0.0, 0.0), 0.0);
    let dd = services::SYMBOL_PAPER_MM;
    let mut items: Vec<Item> = placed
        .marks
        .iter()
        .map(|m| Item { cat: Cat::Furn, pen: m.pen, prim: m.prim.clone() })
        .collect();
    items.extend(placed.labels.iter().map(|l| Item { cat: Cat::Furn, pen: Pen::Light, prim: l.clone() }));
    let wall_line = wall.map(|y| Item {
        cat: Cat::Furn,
        pen: Pen::Fine,
        prim: Prim::Line { a: v(-0.9 * dd, y), b: v(0.9 * dd, y) },
    });
    let mut all = items.clone();
    all.extend(wall_line.clone());
    let b = items_bounds(&all);
    if b.is_empty() {
        return (String::new(), 0.0);
    }
    let f = (max_w / b.width().max(1e-6)).min(1.0);
    let c = b.center();
    let to_paper = move |p: V| -> (f64, f64) { (cx + (p.x - c.x) * f, -(p.y - c.y) * f) };
    let mut s = String::new();
    s.push_str(&format!("<g fill=\"none\" stroke=\"{INK}\">\n"));
    if let Some(wl) = &wall_line {
        s.push_str("<g stroke=\"#8d99a8\">\n");
        write_item_in(&mut s, wl, 1.0 / f, &to_paper, &PLAN_INK);
        s.push_str("</g>\n");
    }
    for it in &items {
        write_item_in(&mut s, it, 1.0 / f, &to_paper, &SYMBOL_INK);
    }
    s.push_str("</g>\n");
    (s, b.height() * f)
}

/// Device symbols: navy, thin pens.
pub(crate) const SYMBOL_INK: Ink = Ink {
    fill: INK,
    text: INK,
    pen: symbol_pen,
};

pub(crate) fn symbol_pen(p: Pen) -> f64 {
    match p {
        Pen::Heavy => 0.3,
        Pen::Medium => 0.25,
        Pen::Light => 0.2,
        Pen::Fine => 0.13,
    }
}

// --------------------------------------------------------------------- table

fn table(o: &mut Out, t: &Table, w: f64, k: f64, max_h: f64) -> f64 {
    let pad = 1.8 * k;
    let top = head_h(k);
    if t.rows.is_empty() {
        let size = 1.6 * k;
        let lines = wrap(&t.empty, size, w - 2.0 * pad);
        let mut y = top + 1.4 * k;
        for l in &lines {
            y += 2.2 * k;
            o.text(pad, y, size, "start", false, MUTED, l);
        }
        return y + 1.4 * k - top;
    }
    let avail = w - 2.0 * pad;
    let mut scale = 1.0_f64;
    let mut widths;
    loop {
        let font = 1.7 * k * scale;
        let head_font = 1.3 * k * scale;
        let cpad = 1.0 * k * scale;
        widths = t
            .columns
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let head = c
                    .head
                    .lines()
                    .map(|l| est_width(l, head_font, true))
                    .fold(0.0, f64::max);
                let cells = t
                    .rows
                    .iter()
                    .map(|r| r.get(i).map(|x| est_width(x, font, false)).unwrap_or(0.0))
                    .chain(
                        t.total
                            .iter()
                            .map(|r| r.get(i).map(|x| est_width(x, font, true)).unwrap_or(0.0)),
                    )
                    .fold(0.0, f64::max);
                head.max(cells).max(c.blank_width * k * scale) + 2.0 * cpad
            })
            .collect::<Vec<f64>>();
        let sum: f64 = widths.iter().sum();
        let flex = t.columns.iter().position(|c| c.flex);
        if sum <= avail {
            // Give what is left to the flexible column.
            let i = flex.unwrap_or(0);
            widths[i] += avail - sum;
            break;
        }
        if let Some(i) = flex {
            let others = sum - widths[i];
            let min_flex = 14.0 * k * scale;
            if others + min_flex <= avail {
                widths[i] = avail - others;
                break;
            }
        }
        if scale <= 0.72 {
            // Squeeze every column in proportion; cells are cut to fit.
            let f = avail / sum;
            for x in widths.iter_mut() {
                *x *= f;
            }
            break;
        }
        scale -= 0.07;
    }
    let font = 1.7 * k * scale;
    let head_font = 1.3 * k * scale;
    let cpad = 1.0 * k * scale;
    let head_lines = t.columns.iter().map(|c| c.head.lines().count()).max().unwrap_or(1).max(1);
    let line_h = head_font * 1.25;
    let header_h = head_lines as f64 * line_h + 1.6 * k;
    let row_h = 3.9 * k * scale;
    let note_size = 1.45 * k;
    let note_lines: Vec<String> = t
        .notes
        .iter()
        .flat_map(|n| wrap(n, note_size, avail))
        .collect();
    let notes_h = if note_lines.is_empty() {
        0.0
    } else {
        note_lines.len() as f64 * 2.1 * k + 1.2 * k
    };
    let total_h = if t.total.is_some() { row_h } else { 0.0 };
    // Rows that fit under the height limit; the rest are summed up in one line.
    let fixed = 1.2 * k + header_h + total_h + notes_h + 1.4 * k;
    let fit_rows = (((max_h - fixed) / row_h).floor().max(1.0)) as usize;
    let mut rows: Vec<Vec<String>> = t.rows.clone();
    if rows.len() > fit_rows {
        let hidden = rows.len() - fit_rows + 1;
        rows.truncate(fit_rows - 1);
        let mut more = vec![String::new(); t.columns.len()];
        more[0] = format!("+ {hidden} more");
        rows.push(more);
    }

    let x_of = |i: usize| pad + widths[..i].iter().sum::<f64>();
    let mut y = top + 1.2 * k;
    let table_top = y;
    // Header.
    for (i, c) in t.columns.iter().enumerate() {
        let lines: Vec<&str> = c.head.lines().collect();
        let x = x_of(i);
        for (j, l) in lines.iter().enumerate() {
            let ly = y + 0.9 * k + (j + 1) as f64 * line_h - 0.25 * head_font
                + (head_lines - lines.len()) as f64 * line_h;
            let (tx, anchor) = match c.align {
                Align::Left => (x + cpad, "start"),
                Align::Right => (x + widths[i] - cpad, "end"),
                Align::Center => (x + widths[i] / 2.0, "middle"),
            };
            let (text, ts) = fit(l, head_font, head_font * 0.8, widths[i] - 2.0 * cpad, true);
            o.text(tx, ly, ts, anchor, true, INK, &text);
        }
    }
    y += header_h;
    o.line((pad, y), (w - pad, y), 0.25, INK);
    let write_row = |o: &mut Out, y: f64, cells: &[String], bold: bool| {
        for (i, c) in t.columns.iter().enumerate() {
            let cell = cells.get(i).map(|s| s.as_str()).unwrap_or("");
            let x = x_of(i);
            let (text, ts) = fit(cell, font, font * 0.8, widths[i] - 2.0 * cpad, bold);
            let (tx, anchor) = match c.align {
                Align::Left => (x + cpad, "start"),
                Align::Right => (x + widths[i] - cpad, "end"),
                Align::Center => (x + widths[i] / 2.0, "middle"),
            };
            o.text(tx, y + row_h / 2.0 + 0.36 * ts, ts, anchor, bold, INK, &text);
        }
    };
    for (ri, r) in rows.iter().enumerate() {
        write_row(o, y, r, false);
        y += row_h;
        if ri + 1 < rows.len() {
            o.line((pad, y), (w - pad, y), 0.1, RULE);
        }
    }
    if let Some(total) = &t.total {
        o.line((pad, y), (w - pad, y), 0.25, INK);
        write_row(o, y, total, true);
        y += row_h;
    }
    let table_bottom = y;
    // Column rules and the outline.
    for i in 1..t.columns.len() {
        let x = x_of(i);
        o.line((x, table_top), (x, table_bottom), 0.1, RULE);
    }
    o.rect(pad, table_top, avail, table_bottom - table_top, 0.18, INK, "none");
    if !note_lines.is_empty() {
        y += 1.2 * k;
        for l in &note_lines {
            y += 2.1 * k;
            o.text(pad, y - 0.5 * k, note_size, "start", false, MUTED, l);
        }
    }
    y + 1.4 * k - top
}

// --------------------------------------------------------------------- notes

fn notes(o: &mut Out, lines: &[String], w: f64, k: f64) -> f64 {
    let pad = 1.8 * k;
    let size = 1.55 * k;
    let indent = 3.0 * k;
    let mut y = head_h(k) + 0.9 * k;
    for (i, n) in lines.iter().enumerate() {
        let wrapped = wrap(n, size, w - 2.0 * pad - indent);
        for (j, l) in wrapped.iter().enumerate() {
            y += 2.25 * k;
            if j == 0 {
                o.text(pad, y, size, "start", false, INK, &format!("{}.", i + 1));
            }
            o.text(pad + indent, y, size, "start", false, INK, l);
        }
    }
    y + 1.6 * k - head_h(k)
}

// ----------------------------------------------------------------- signatory

fn signatory(o: &mut Out, w: f64, k: f64) -> f64 {
    let pad = 1.8 * k;
    let caption = 1.3 * k;
    let mut y = head_h(k) + 1.2 * k;
    for (label, h) in [("NAME", 7.0 * k), ("PRC NO.", 7.0 * k), ("SIGNATURE AND SEAL", 22.0 * k)] {
        o.rect(pad, y, w - 2.0 * pad, h, 0.18, INK, "none");
        o.text(pad + 1.0 * k, y + 2.1 * k, caption, "start", false, MUTED, label);
        y += h;
    }
    y + 1.6 * k - head_h(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_keeps_lines_inside_the_width() {
        let text = "Rating, wire and breaker columns are left blank for the PEE to fill in.";
        let lines = wrap(text, 1.5, 40.0);
        assert!(lines.len() >= 2);
        for l in &lines {
            assert!(est_width(l, 1.5, false) <= 40.0, "{l}");
        }
        assert_eq!(lines.join(" "), text);
    }

    #[test]
    fn tables_drop_rows_that_do_not_fit() {
        let t = Table {
            columns: vec![
                Column { head: "ROOM".into(), align: Align::Left, flex: true, blank_width: 0.0 },
                Column { head: "QTY".into(), align: Align::Right, flex: false, blank_width: 0.0 },
            ],
            rows: (0..40).map(|i| vec![format!("Room {i}"), "1".into()]).collect(),
            total: None,
            notes: vec![],
            empty: String::new(),
        };
        let b = Block { id: "t".into(), title: "T".into(), content: Content::Table(t) };
        let r = render(&b, 60.0, 1.0, 60.0);
        assert!(r.h <= 60.0 + 1e-6, "{}", r.h);
        assert!(r.svg.contains("more</text>"));
    }
}
