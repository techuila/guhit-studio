//! The printed sheet: paper size, scale selection, layout, SVG output.
//! One SVG user unit is one millimeter on paper.

use guhit_model::{Derived, Orientation, PaperSize, PipeSystem, PlanExportOptions, Project};

use crate::geom::*;
use crate::pipes::{self, Dash, PlanPipe};
use crate::plan::{
    build_items, empty_level, items_bounds, resolve_level, Cat, Fill, HAlign, Item, Pen,
    PlanOptions, Prim,
};
use crate::text::{clean, est_width, fit, xml_escape};
use crate::ExportError;

/// Line and text color: deep blueprint navy.
pub const INK: &str = "#14283f";
/// Small field captions in the title block.
pub(crate) const MUTED: &str = "#56657a";
/// Font stack written into the SVG. The PDF path resolves it through fontdb.
pub const FONT_FAMILY: &str = "Helvetica, Arial, 'Liberation Sans', 'DejaVu Sans', sans-serif";

/// Common architectural scales, largest drawing first.
pub const COMMON_SCALES: [u32; 12] = [20, 25, 50, 75, 100, 125, 150, 200, 250, 300, 400, 500];

/// Paper size in mm as (width, height) for the given orientation.
pub fn paper_size_mm(paper: PaperSize, orientation: Orientation) -> (f64, f64) {
    let (short, long) = match paper {
        PaperSize::A4 => (210.0, 297.0),
        PaperSize::A3 => (297.0, 420.0),
        PaperSize::A2 => (420.0, 594.0),
        PaperSize::A1 => (594.0, 841.0),
    };
    match orientation {
        Orientation::Landscape => (long, short),
        Orientation::Portrait => (short, long),
    }
}

pub fn paper_name(paper: PaperSize) -> &'static str {
    match paper {
        PaperSize::A4 => "A4",
        PaperSize::A3 => "A3",
        PaperSize::A2 => "A2",
        PaperSize::A1 => "A1",
    }
}

/// Stroke width in paper mm.
pub(crate) fn pen_width(pen: Pen) -> f64 {
    match pen {
        Pen::Heavy => 0.35,
        Pen::Medium => 0.25,
        Pen::Light => 0.18,
        Pen::Fine => 0.13,
    }
}

/// Fixed sheet geometry for one paper size and orientation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub width: f64,
    pub height: f64,
    /// Distance from the paper edge to the border line.
    pub margin: f64,
    /// Size factor for sheet furniture, larger on larger paper.
    pub k: f64,
    /// Height of the title block strip, 0 when off.
    pub title_h: f64,
    /// Height of the band with drawing title, scale bar and north arrow.
    pub band_h: f64,
    /// Area the plan may use: x, y, width, height.
    pub plan: (f64, f64, f64, f64),
}

impl Layout {
    pub fn new(paper: PaperSize, orientation: Orientation, title_block: bool) -> Layout {
        let (width, height) = paper_size_mm(paper, orientation);
        let (margin, k) = match paper {
            PaperSize::A4 | PaperSize::A3 => (10.0, 1.0),
            PaperSize::A2 => (15.0, 1.3),
            PaperSize::A1 => (15.0, 1.6),
        };
        let title_h = if title_block { 24.0 * k } else { 0.0 };
        let band_h = 21.0 * k;
        let pad = 6.0 * k;
        let plan = (
            margin + pad,
            margin + pad,
            width - 2.0 * (margin + pad),
            height - 2.0 * margin - title_h - band_h - 2.0 * pad,
        );
        Layout {
            width,
            height,
            margin,
            k,
            title_h,
            band_h,
            plan,
        }
    }
}

/// Largest common scale (smallest N) at which a drawing of the given model
/// size fits the available paper area. `size_at` returns the model extent
/// (width, height) in mm when annotated for scale 1:N. Falls back to the
/// smallest common scale when nothing fits.
pub fn pick_scale(avail_w: f64, avail_h: f64, mut size_at: impl FnMut(u32) -> (f64, f64)) -> u32 {
    for n in COMMON_SCALES {
        let (w, h) = size_at(n);
        if w / n as f64 <= avail_w && h / n as f64 <= avail_h {
            return n;
        }
    }
    COMMON_SCALES[COMMON_SCALES.len() - 1]
}

/// Compact number for SVG attributes.
pub(crate) fn num(x: f64) -> String {
    let x = if x.is_finite() { x } else { 0.0 };
    let s = format!("{x:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.to_string()
    }
}

pub(crate) struct Svg {
    pub(crate) s: String,
}

impl Svg {
    pub(crate) fn line(&mut self, a: (f64, f64), b: (f64, f64), w: f64) {
        self.s.push_str(&format!(
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{INK}\" stroke-width=\"{}\"/>\n",
            num(a.0),
            num(a.1),
            num(b.0),
            num(b.1),
            num(w)
        ));
    }

    pub(crate) fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, stroke_w: f64, fill: &str) {
        self.s.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{fill}\" stroke=\"{INK}\" stroke-width=\"{}\"/>\n",
            num(x),
            num(y),
            num(w),
            num(h),
            num(stroke_w)
        ));
    }

    pub(crate) fn polygon(&mut self, pts: &[(f64, f64)], fill: &str, stroke_w: f64) {
        let p: Vec<String> = pts.iter().map(|p| format!("{},{}", num(p.0), num(p.1))).collect();
        self.s.push_str(&format!(
            "<polygon points=\"{}\" fill=\"{fill}\" stroke=\"{INK}\" stroke-width=\"{}\" stroke-linejoin=\"round\"/>\n",
            p.join(" "),
            num(stroke_w)
        ));
    }

    /// Upright text. `anchor` is start, middle or end.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn text(&mut self, x: f64, y: f64, size: f64, anchor: &str, bold: bool, color: &str, text: &str) {
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
}

/// Render the sheet. Returns the SVG and the scale used.
pub fn render(
    project: &Project,
    derived: &Derived,
    opts: &PlanExportOptions,
) -> Result<(String, u32), ExportError> {
    let level = resolve_level(project, opts.level_id.as_ref())?;
    let layout = Layout::new(opts.paper, opts.orientation, opts.title_block);
    let build = |n: u32| {
        build_items(
            project,
            derived,
            level,
            &PlanOptions {
                scale: n as f64,
                show_dimensions: opts.show_dimensions,
                show_room_labels: opts.show_room_labels,
                show_assets: opts.show_assets,
                unicode: true,
                skip_devices: false,
            },
        )
    };
    let pipes = if opts.show_pipes {
        pipes::plan_pipes(project, level)
    } else {
        Vec::new()
    };
    // What the drawing covers at 1:n: the plan and the pipes around it.
    let extent = |items: &[Item], n: u32| {
        let mut b = items_bounds(items);
        if !pipes.is_empty() {
            let pb = pipes::bounds(&pipes, n as f64);
            b.add(pb.min);
            b.add(pb.max);
        }
        b
    };

    // Fails early with Empty when there is nothing to draw.
    if build(100).is_empty() && pipes.is_empty() {
        return Err(empty_level(level));
    }

    let (px, py, pw, ph) = layout.plan;
    let scale = match opts.scale_denominator {
        Some(n) if n > 0 => n,
        _ => pick_scale(pw, ph, |n| {
            let b = extent(&build(n), n);
            (b.width(), b.height())
        }),
    };
    let items = build(scale);
    let bounds = extent(&items, scale);
    let n = scale as f64;
    let center = bounds.center();
    let (ox, oy) = (px + pw / 2.0, py + ph / 2.0);
    let to_paper = |p: V| -> (f64, f64) { (ox + (p.x - center.x) / n, oy - (p.y - center.y) / n) };

    let (w, h) = (layout.width, layout.height);
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
        xml_escape(&format!("{} - {} - 1:{}", clean(&project.name), clean(&level.name), scale))
    ));
    svg.s.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>\n",
        num(w),
        num(h)
    ));

    // The plan is clipped to the inside of the border above the furniture,
    // so an oversized drawing at a forced scale never runs over the title block.
    let m = layout.margin;
    let clip_h = h - 2.0 * m - layout.title_h - layout.band_h;
    svg.s.push_str(&format!(
        "<defs><clipPath id=\"plan-clip\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath></defs>\n",
        num(m),
        num(m),
        num(w - 2.0 * m),
        num(clip_h)
    ));
    svg.s.push_str(&format!(
        "<g id=\"plan\" clip-path=\"url(#plan-clip)\" stroke-linecap=\"butt\" stroke-linejoin=\"miter\" fill=\"none\" stroke=\"{INK}\">\n"
    ));
    for cat in Cat::ALL {
        let group: Vec<&Item> = items.iter().filter(|i| i.cat == cat).collect();
        if cat == Cat::Area && !pipes.is_empty() {
            // Pipes draw over the whole plan and over the room label masks,
            // so a run is never cut by a label. The names go on top and
            // still read. Area is the last category, so this always runs.
            if !group.is_empty() {
                svg.s.push_str("<g id=\"room-label-masks\">\n");
                label_masks(&mut svg.s, &group, n, &to_paper);
                svg.s.push_str("</g>\n");
            }
            write_pipes(&mut svg.s, &pipes, n, &to_paper);
            if !group.is_empty() {
                svg.s.push_str(&format!("<g id=\"{}\">\n", cat.svg_id()));
                for item in group {
                    write_item(&mut svg.s, item, n, &to_paper);
                }
                svg.s.push_str("</g>\n");
            }
            continue;
        }
        if group.is_empty() {
            continue;
        }
        svg.s.push_str(&format!("<g id=\"{}\">\n", cat.svg_id()));
        if cat == Cat::Area {
            label_masks(&mut svg.s, &group, n, &to_paper);
        }
        for item in group {
            write_item(&mut svg.s, item, n, &to_paper);
        }
        svg.s.push_str("</g>\n");
    }
    svg.s.push_str("</g>\n");

    let legend: Vec<(PipeSystem, f64)> = pipes::systems_present(&pipes)
        .into_iter()
        .map(|system| {
            let widest = pipes
                .iter()
                .filter(|p| p.system == system)
                .map(|p| pipes::stroke_width(p.diameter_mm, n))
                .fold(0.0, f64::max);
            (system, widest.clamp(pipes::paper::STROKE_MIN, LEGEND_MAX_STROKE))
        })
        .collect();
    let frame = Frame::plan(&clean(&level.name), scale, &legend);
    furniture(&mut svg, project, opts, &layout, &frame);

    svg.s.push_str("</svg>\n");
    Ok((svg.s, scale))
}

/// Room labels mask what is under them (furniture outlines, door arcs) so the
/// name and area always read cleanly: one white box per room.
pub(crate) fn label_masks(s: &mut String, group: &[&Item], n: f64, to_paper: &dyn Fn(V) -> (f64, f64)) {
    let pad = 0.6 * n;
    let mut boxes: Vec<Bounds> = Vec::new();
    for item in group {
        let mut b = items_bounds(std::slice::from_ref(*item));
        if b.is_empty() {
            continue;
        }
        b.min = b.min - v(pad, pad);
        b.max = b.max + v(pad, pad);
        // Name and area of one room merge into a single mask.
        while let Some(i) = boxes.iter().position(|o| o.intersects(&b)) {
            let o = boxes.swap_remove(i);
            b.add(o.min);
            b.add(o.max);
        }
        boxes.push(b);
    }
    for b in boxes {
        let (x0, y0) = to_paper(v(b.min.x, b.max.y));
        let (x1, y1) = to_paper(v(b.max.x, b.min.y));
        s.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#ffffff\" stroke=\"none\"/>\n",
            num(x0),
            num(y0),
            num(x1 - x0),
            num(y1 - y0)
        ));
    }
}

/// Widest line a legend sample gets, paper mm.
pub(crate) const LEGEND_MAX_STROKE: f64 = 0.7;

pub(crate) fn dash_attr(pattern: &[f64]) -> String {
    if pattern.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = pattern.iter().map(|x| num(*x)).collect();
    format!(" stroke-dasharray=\"{}\"", parts.join(" "))
}

/// The pattern of a legend sample `length` mm long, stretched or shrunk a
/// little so the sample starts and ends on a full dash. A cut dash at the
/// end would read as a dot and make a dashed sample look dash-dot.
pub(crate) fn legend_pattern(d: Dash, width: f64, length: f64) -> Vec<f64> {
    let p = pipes::dash_pattern(d, width);
    if p.is_empty() {
        return p;
    }
    let period: f64 = p.iter().sum();
    let periods = ((length - p[0]) / period).round().max(1.0);
    let k = length / (periods * period + p[0]);
    p.iter().map(|x| x * k).collect()
}

pub(crate) fn line_cap(d: Dash) -> &'static str {
    match d {
        Dash::Solid => "round",
        Dash::Dashed | Dash::DashDot => "butt",
    }
}

/// The pipe group: one sub-group per system in its color, wide systems
/// first, and inside each the wider pipes first. Risers are white circles
/// over the line they end, so the line reads as turning up or down there.
pub(crate) fn write_pipes(s: &mut String, list: &[PlanPipe], n: f64, to_paper: &dyn Fn(V) -> (f64, f64)) {
    s.push_str("<g id=\"pipes\" fill=\"none\" stroke-linejoin=\"round\">\n");
    for system in pipes::DRAW_ORDER {
        let mut group: Vec<&PlanPipe> = list.iter().filter(|p| p.system == system).collect();
        if group.is_empty() {
            continue;
        }
        group.sort_by(|a, b| {
            b.diameter_mm
                .partial_cmp(&a.diameter_mm)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let d = pipes::dash(system);
        s.push_str(&format!(
            "<g id=\"pipes-{}\" stroke=\"{}\" stroke-linecap=\"{}\">\n",
            pipes::slug(system),
            pipes::color(system),
            line_cap(d)
        ));
        for p in &group {
            let w = pipes::stroke_width(p.diameter_mm, n);
            let dashes = dash_attr(&pipes::dash_pattern(d, w));
            for run in &p.runs {
                let pts: Vec<String> = run
                    .iter()
                    .map(|q| {
                        let q = to_paper(*q);
                        format!("{},{}", num(q.0), num(q.1))
                    })
                    .collect();
                s.push_str(&format!(
                    "<polyline points=\"{}\" stroke-width=\"{}\"{dashes}/>\n",
                    pts.join(" "),
                    num(w)
                ));
            }
        }
        for p in &group {
            let w = pipes::stroke_width(p.diameter_mm, n);
            let r = pipes::riser_radius(p.diameter_mm, n) / n;
            let sw = (w * 0.5).clamp(pipes::paper::RISER_STROKE, 0.7);
            for c in &p.risers {
                let q = to_paper(*c);
                s.push_str(&format!(
                    "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"#ffffff\" stroke-width=\"{}\"/>\n",
                    num(q.0),
                    num(q.1),
                    num(r),
                    num(sw)
                ));
            }
        }
        s.push_str("</g>\n");
    }
    s.push_str("</g>\n");
}

/// Legend of the pipe systems on the sheet, right aligned at `right` in the
/// band between the drawing title and the scale bar, so it never reaches the
/// title block. Returns its left edge.
fn pipe_legend(
    svg: &mut Svg,
    entries: &[(PipeSystem, f64)],
    l: &Layout,
    right: f64,
    title_x: f64,
    band_mid: f64,
) -> f64 {
    let k = l.k;
    let size = 2.0 * k;
    let sample = 10.0 * k;
    let gap = 1.6 * k;
    let col_gap = 3.5 * k;
    let pitch = 3.6 * k;
    let count = entries.len();
    let label_w = |i: usize| est_width(pipes::label(entries[i].0), size, false);
    // Column-major grid: supply systems share a column, sanitary the next.
    let width = |rows: usize| -> f64 {
        let cols = count.div_ceil(rows);
        let mut w = 0.0;
        for c in 0..cols {
            let widest = (c * rows..((c + 1) * rows).min(count))
                .map(label_w)
                .fold(0.0, f64::max);
            w += sample + gap + widest;
        }
        w + col_gap * (cols.saturating_sub(1)) as f64
    };
    let mut rows = count.clamp(1, 2);
    // Keep room for the drawing title; one column when two would crowd it.
    // The band holds four rows at most.
    let tall = count.min(4);
    if count > 2 && right - width(rows) - title_x < 46.0 * k && width(tall) < width(rows) {
        rows = tall;
    }
    let total = width(rows);
    let left = right - total;

    svg.s.push_str("<g id=\"pipe-legend\">\n");
    let first = band_mid + 0.6 * k - (rows as f64 - 1.0) * pitch / 2.0;
    svg.text(left, first - 3.3 * k, size, "start", false, MUTED, "PIPES");
    let mut x = left;
    for c in 0..count.div_ceil(rows) {
        let mut widest: f64 = 0.0;
        for r in 0..rows {
            let i = c * rows + r;
            if i >= count {
                break;
            }
            let (system, w) = entries[i];
            let y = first + pitch * r as f64;
            let d = pipes::dash(system);
            svg.s.push_str(&format!(
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{}\" stroke-linecap=\"{}\"{}/>\n",
                num(x),
                num(y),
                num(x + sample),
                num(y),
                pipes::color(system),
                num(w),
                line_cap(d),
                dash_attr(&legend_pattern(d, w, sample))
            ));
            svg.text(
                x + sample + gap,
                y + 0.72 * k,
                size,
                "start",
                false,
                INK,
                pipes::label(system),
            );
            widest = widest.max(label_w(i));
        }
        x += sample + gap + widest + col_gap;
    }
    svg.s.push_str("</g>\n");
    left
}

/// How plan primitives are inked: fill and text color, and the stroke width
/// of each pen. Strokes take the color of their group.
pub(crate) struct Ink {
    pub fill: &'static str,
    pub text: &'static str,
    pub pen: fn(Pen) -> f64,
}

/// The plan sheet: navy ink, the usual pens.
pub(crate) const PLAN_INK: Ink = Ink {
    fill: INK,
    text: INK,
    pen: pen_width,
};

pub(crate) fn write_item(s: &mut String, item: &Item, n: f64, to_paper: &dyn Fn(V) -> (f64, f64)) {
    write_item_in(s, item, n, to_paper, &PLAN_INK);
}

pub(crate) fn write_item_in(
    s: &mut String,
    item: &Item,
    n: f64,
    to_paper: &dyn Fn(V) -> (f64, f64),
    ink: &Ink,
) {
    let sw = num((ink.pen)(item.pen));
    match &item.prim {
        Prim::Line { a, b } => {
            let (a, b) = (to_paper(*a), to_paper(*b));
            s.push_str(&format!(
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke-width=\"{sw}\"/>\n",
                num(a.0),
                num(a.1),
                num(b.0),
                num(b.1)
            ));
        }
        Prim::Poly { pts, closed, fill } => {
            let p: Vec<String> = pts
                .iter()
                .map(|p| {
                    let q = to_paper(*p);
                    format!("{},{}", num(q.0), num(q.1))
                })
                .collect();
            let tag = if *closed { "polygon" } else { "polyline" };
            let fill = match fill {
                Fill::Ink => format!(" fill=\"{}\"", ink.fill),
                Fill::None => String::new(),
            };
            s.push_str(&format!(
                "<{tag} points=\"{}\"{fill} stroke-width=\"{sw}\"/>\n",
                p.join(" ")
            ));
        }
        Prim::Arc {
            c,
            r,
            start_deg,
            end_deg,
        } => {
            let p0 = to_paper(*c + dir(*start_deg) * *r);
            let p1 = to_paper(*c + dir(*end_deg) * *r);
            let sweep = norm_deg(end_deg - start_deg);
            let large = if sweep > 180.0 { 1 } else { 0 };
            // Counter-clockwise in the model is counter-clockwise on paper
            // after the y flip, which is sweep-flag 0 in SVG.
            s.push_str(&format!(
                "<path d=\"M{} {}A{} {} 0 {large} 0 {} {}\" stroke-width=\"{sw}\"/>\n",
                num(p0.0),
                num(p0.1),
                num(r / n),
                num(r / n),
                num(p1.0),
                num(p1.1)
            ));
        }
        Prim::Circle { c, r, fill } => {
            let q = to_paper(*c);
            let fill = match fill {
                Fill::Ink => format!(" fill=\"{}\"", ink.fill),
                Fill::None => String::new(),
            };
            s.push_str(&format!(
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"{fill} stroke-width=\"{sw}\"/>\n",
                num(q.0),
                num(q.1),
                num(r / n)
            ));
        }
        Prim::Text {
            pos,
            height,
            rot_deg,
            align,
            text,
            bold,
        } => {
            let q = to_paper(*pos);
            let anchor = match align {
                HAlign::Start => "start",
                HAlign::Middle => "middle",
            };
            let transform = if rot_deg.abs() < 1e-6 {
                format!("translate({} {})", num(q.0), num(q.1))
            } else {
                format!("translate({} {}) rotate({})", num(q.0), num(q.1), num(-rot_deg))
            };
            s.push_str(&format!(
                "<text transform=\"{transform}\" font-size=\"{}\" text-anchor=\"{anchor}\"{} fill=\"{}\" stroke=\"none\">{}</text>\n",
                num(height / n),
                if *bold { " font-weight=\"bold\"" } else { "" },
                ink.text,
                xml_escape(text)
            ));
        }
    }
}

/// Drawing title under the plan, for example "GROUND FLOOR PLAN".
pub(crate) fn drawing_title(level_name: &str) -> String {
    let name = level_name.trim();
    if name.is_empty() {
        return "FLOOR PLAN".into();
    }
    let upper = name.to_uppercase();
    if upper.ends_with("PLAN") {
        upper
    } else {
        format!("{upper} PLAN")
    }
}

/// Date part of an RFC 3339 timestamp, or "-" when it does not look like one.
fn sheet_date(project: &Project) -> String {
    let t = project.updated_at.trim();
    let bytes = t.as_bytes();
    let ok = bytes.len() >= 10
        && bytes[..10]
            .iter()
            .enumerate()
            .all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() });
    if ok {
        t[..10].to_string()
    } else {
        "-".into()
    }
}

/// Model length of one scale bar segment: the largest round length whose four
/// segments fit into `max_paper` mm at 1:n.
pub fn scale_bar_segment_mm(n: u32, max_paper: f64) -> f64 {
    const NICE: [f64; 13] = [
        50.0, 100.0, 200.0, 250.0, 500.0, 1000.0, 2000.0, 2500.0, 5000.0, 10000.0, 20000.0,
        25000.0, 50000.0,
    ];
    let mut best = NICE[0];
    for l in NICE {
        if 4.0 * l / n as f64 <= max_paper {
            best = l;
        }
    }
    best
}

fn meters_label(mm: f64) -> String {
    let s = format!("{:.2}", mm / 1000.0);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() {
        "0".into()
    } else {
        s.to_string()
    }
}

/// Which way the north arrow in the band points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum North {
    /// Plan north: paper up turned by the project's north angle.
    Plan,
    /// A unit direction on paper (x right, y down), for a diagram.
    Paper(f64, f64),
}

/// What the band and the title block say about the drawing.
pub(crate) struct Frame<'a> {
    /// Drawing title in capitals, for example "GROUND FLOOR PLAN".
    pub title: String,
    /// The line under the title: "SCALE 1:100" or "NOT TO SCALE".
    pub scale_text: String,
    /// The scale the graphic scale bar shows. None draws no bar.
    pub bar: Option<u32>,
    pub north: North,
    /// Pipe legend in the band, right aligned against the scale bar.
    pub legend: &'a [(PipeSystem, f64)],
    /// The SCALE cell of the title block.
    pub block_scale: String,
}

impl Frame<'_> {
    /// The plan sheet's frame: title from the level, scale bar, plan north.
    pub(crate) fn plan<'a>(level_name: &str, scale: u32, legend: &'a [(PipeSystem, f64)]) -> Frame<'a> {
        Frame {
            title: drawing_title(level_name),
            scale_text: format!("SCALE 1:{scale}"),
            bar: Some(scale),
            north: North::Plan,
            legend,
            block_scale: format!("1:{scale}"),
        }
    }
}

pub(crate) fn furniture(svg: &mut Svg, project: &Project, opts: &PlanExportOptions, l: &Layout, f: &Frame) {
    let k = l.k;
    let m = l.margin;
    let (w, h) = (l.width, l.height);
    let inner_w = w - 2.0 * m;
    let legend = f.legend;
    svg.s.push_str("<g id=\"sheet\">\n");

    // Border.
    svg.rect(m, m, inner_w, h - 2.0 * m, 0.35, "none");

    // Band: drawing title left, scale bar and north arrow right.
    let band_top = h - m - l.title_h - l.band_h;
    let band_mid = band_top + l.band_h / 2.0;
    let pad = 6.0 * k;

    // North arrow.
    let r = 5.2 * k;
    let nc = (w - m - pad - r - 1.5 * k, band_mid);
    let d = match f.north {
        North::Plan => {
            let a = project.settings.north_angle_deg;
            let a = if a.is_finite() { a } else { 0.0 };
            // North on paper: +y (up) rotated counter-clockwise by the north angle.
            let (sn, cs) = a.to_radians().sin_cos();
            (-sn, -cs)
        }
        North::Paper(x, y) => (x, y),
    };
    let p = (-d.1, d.0);
    let at = |fd: f64, fp: f64| (nc.0 + d.0 * fd * r + p.0 * fp * r, nc.1 + d.1 * fd * r + p.1 * fp * r);
    svg.s.push_str(&format!(
        "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"{INK}\" stroke-width=\"0.25\"/>\n",
        num(nc.0),
        num(nc.1),
        num(r)
    ));
    let tip = at(0.92, 0.0);
    let notch = at(-0.3, 0.0);
    svg.polygon(&[tip, at(-0.68, 0.46), notch], "#ffffff", 0.18);
    svg.polygon(&[tip, at(-0.68, -0.46), notch], INK, 0.18);
    let nl = at(1.0 + 2.6 * k / r, 0.0);
    svg.text(nl.0, nl.1 + 1.05 * k, 3.0 * k, "middle", true, INK, "N");

    // Scale bar, right aligned next to the north arrow.
    let bar_right = nc.0 - r - 4.2 * k - pad;
    let bar_x = match f.bar {
        Some(scale) => scale_bar(svg, l, scale, bar_right, band_mid),
        None => bar_right,
    };

    // Pipe legend between the drawing title and the scale bar.
    let title_x = m + pad;
    let title_right = if legend.is_empty() {
        bar_x
    } else {
        pipe_legend(svg, legend, l, bar_x - pad, title_x, band_mid)
    };

    // Drawing title.
    let title_max = (title_right - pad - title_x).max(20.0);
    let (title, title_size) = fit(&f.title, 4.2 * k, 2.4 * k, title_max, true);
    let title_w = est_width(&title, title_size, true).min(title_max);
    let base = band_mid - 0.4 * k;
    svg.text(title_x, base, title_size, "start", true, INK, &title);
    svg.line((title_x, base + 1.8 * k), (title_x + title_w + 2.0 * k, base + 1.8 * k), 0.5);
    svg.text(title_x, base + 5.6 * k, 2.6 * k, "start", false, INK, &f.scale_text);

    if opts.title_block {
        title_block(svg, project, &title_case(&f.title), opts, l, &f.block_scale);
    }
    svg.s.push_str("</g>\n");
}

/// The graphic scale bar, right aligned at `right`. Returns its left edge.
fn scale_bar(svg: &mut Svg, l: &Layout, scale: u32, right: f64, band_mid: f64) -> f64 {
    let k = l.k;
    let inner_w = l.width - 2.0 * l.margin;
    let max_bar = (inner_w * 0.3).min(70.0 * k);
    let seg_mm = scale_bar_segment_mm(scale, max_bar);
    let seg = seg_mm / scale as f64;
    let bar_w = seg * 4.0;
    let bar_x = right - bar_w;
    let bar_h = 1.5 * k;
    let bar_y = band_mid - bar_h / 2.0 + 0.6 * k;
    svg.rect(bar_x, bar_y, bar_w, bar_h, 0.18, "#ffffff");
    // First segment split in two, then alternating solid segments.
    svg.rect(bar_x, bar_y, seg / 2.0, bar_h, 0.18, INK);
    svg.rect(bar_x + seg, bar_y, seg, bar_h, 0.18, INK);
    svg.rect(bar_x + 3.0 * seg, bar_y, seg, bar_h, 0.18, INK);
    let label_size = 2.0 * k;
    for i in 0..=4 {
        let x = bar_x + seg * i as f64;
        svg.line((x, bar_y - 0.9 * k), (x, bar_y), 0.18);
        let label = meters_label(seg_mm * i as f64);
        let label = if i == 4 { format!("{label} m") } else { label };
        // Keep the last label centered on its number, not on the unit.
        let shift = if i == 4 { est_width(" m", label_size, false) / 2.0 } else { 0.0 };
        svg.text(x + shift, bar_y - 1.7 * k, label_size, "middle", false, INK, &label);
    }
    svg.text(
        bar_x,
        bar_y + bar_h + 3.0 * k,
        label_size,
        "start",
        false,
        MUTED,
        "GRAPHIC SCALE",
    );
    bar_x
}

/// Title case of a capitals title, for the DRAWING cell.
pub(crate) fn title_case(t: &str) -> String {
    let mut out = String::new();
    for (i, word) in t.split_whitespace().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let mut cs = word.chars();
        if let Some(f) = cs.next() {
            out.extend(f.to_uppercase());
            out.push_str(&cs.as_str().to_lowercase());
        }
    }
    out
}

/// The title block strip. `drawing` goes into the DRAWING cell as given.
pub(crate) fn title_block(
    svg: &mut Svg,
    project: &Project,
    drawing: &str,
    opts: &PlanExportOptions,
    l: &Layout,
    block_scale: &str,
) {
    let k = l.k;
    let m = l.margin;
    let x0 = m;
    let y0 = l.height - m - l.title_h;
    let inner_w = l.width - 2.0 * m;
    let row_h = l.title_h / 2.0;
    svg.line((x0, y0), (x0 + inner_w, y0), 0.35);

    let or_dash = |s: &str| {
        let c = clean(s);
        if c.is_empty() {
            "-".to_string()
        } else {
            c
        }
    };
    let drawing = clean(drawing);
    let paper = format!(
        "{} {}",
        paper_name(opts.paper),
        match opts.orientation {
            guhit_model::Orientation::Landscape => "landscape",
            guhit_model::Orientation::Portrait => "portrait",
        }
    );

    // Column widths as fractions of the strip. The last column is the brand.
    let fractions = [0.32, 0.23, 0.17, 0.14, 0.14];
    let cells: [[(&str, String, bool); 2]; 4] = [
        [
            ("PROJECT", or_dash(&project.name), true),
            ("DRAWING", drawing, false),
        ],
        [
            ("CLIENT", or_dash(&project.settings.client_name), false),
            ("LOCATION", or_dash(&project.settings.location), false),
        ],
        [
            ("DESIGNED BY", or_dash(&project.settings.designer), false),
            ("DATE", sheet_date(project), false),
        ],
        [
            ("SCALE", block_scale.to_string(), true),
            ("PAPER", paper, false),
        ],
    ];

    let inset = 2.2 * k;
    let mut x = x0;
    for (ci, col) in cells.iter().enumerate() {
        let cw = inner_w * fractions[ci];
        for (ri, (caption, value, bold)) in col.iter().enumerate() {
            let cy = y0 + row_h * ri as f64;
            svg.text(x + inset, cy + 3.4 * k, 1.7 * k, "start", false, MUTED, caption);
            let size = if *bold { 3.4 * k } else { 3.0 * k };
            let (t, s) = fit(value, size, 1.9 * k, cw - 2.0 * inset, *bold);
            svg.text(x + inset, cy + 8.9 * k, s, "start", *bold, INK, &t);
        }
        svg.line((x, y0 + row_h), (x + cw, y0 + row_h), 0.18);
        x += cw;
        svg.line((x, y0), (x, y0 + l.title_h), 0.18);
    }

    // Brand cell.
    let cw = x0 + inner_w - x;
    let cx = x + cw / 2.0;
    let (brand, bs) = fit("Guhit Studio", 3.8 * k, 2.0 * k, cw - 2.0 * inset, true);
    svg.text(cx, y0 + l.title_h / 2.0 + 0.2 * k, bs, "middle", true, INK, &brand);
    let (tag, ts) = fit("Draw. See. Build the idea.", 1.8 * k, 1.2 * k, cw - 2.0 * inset, false);
    svg.text(cx, y0 + l.title_h / 2.0 + 4.4 * k, ts, "middle", false, MUTED, &tag);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_sizes() {
        assert_eq!(paper_size_mm(PaperSize::A4, Orientation::Portrait), (210.0, 297.0));
        assert_eq!(paper_size_mm(PaperSize::A3, Orientation::Landscape), (420.0, 297.0));
        assert_eq!(paper_size_mm(PaperSize::A1, Orientation::Landscape), (841.0, 594.0));
    }

    #[test]
    fn pick_scale_takes_the_largest_that_fits() {
        // 8 m by 6 m building, 300 by 200 mm available.
        let n = pick_scale(300.0, 200.0, |_| (8000.0, 6000.0));
        assert_eq!(n, 50); // 8000/50 = 160 fits, 8000/25 = 320 does not
        let n = pick_scale(300.0, 200.0, |_| (30000.0, 12000.0));
        assert_eq!(n, 100);
        let n = pick_scale(150.0, 100.0, |_| (30000.0, 12000.0));
        assert_eq!(n, 200);
    }

    #[test]
    fn pick_scale_limits() {
        assert_eq!(pick_scale(300.0, 200.0, |_| (100.0, 100.0)), 20);
        // Nothing fits: smallest common scale.
        assert_eq!(pick_scale(300.0, 200.0, |_| (1.0e6, 1.0e6)), 500);
        // Height can be the limiting side.
        assert_eq!(pick_scale(300.0, 50.0, |_| (8000.0, 6000.0)), 125);
    }

    #[test]
    fn pick_scale_sees_scale_dependent_size() {
        // Annotations grow with N, so the extent is asked per candidate.
        // 50: (4900 + 100) / 50 = 100 fits exactly.
        assert_eq!(pick_scale(100.0, 100.0, |n| (4900.0 + 2.0 * n as f64, 1000.0)), 50);
        // A little more annotation and 1:50 no longer fits.
        assert_eq!(pick_scale(100.0, 100.0, |n| (4900.0 + 3.0 * n as f64, 1000.0)), 75);
    }

    #[test]
    fn scale_bar_lengths() {
        assert_eq!(scale_bar_segment_mm(100, 60.0), 1000.0);
        assert_eq!(scale_bar_segment_mm(50, 60.0), 500.0);
        assert_eq!(scale_bar_segment_mm(200, 60.0), 2500.0);
        assert_eq!(meters_label(2500.0), "2.5");
        assert_eq!(meters_label(0.0), "0");
    }

    #[test]
    fn titles() {
        assert_eq!(drawing_title("Ground Floor"), "GROUND FLOOR PLAN");
        assert_eq!(drawing_title("Roof plan"), "ROOF PLAN");
        assert_eq!(drawing_title(""), "FLOOR PLAN");
    }
}
