//! The permit set (DECISIONS D53, D54): sheets A0 to A3 with viewports at
//! stated scales, the PH title block (`titleblock`), schedules as tables
//! (`tables`), and the whole set in one PDF. LANE L4 owns this module,
//! `titleblock.rs` and `tables.rs`.
//!
//! The signatory prints in full on the PDF on disk; `SheetInputs::
//! mask_signatory` prints `Signatory::masked()` for agents' pictures.
//!
//! A sheet is the paper inside its border, the title block strip down the
//! right edge, and the drawing area left of it. Viewports live only in the
//! drawing area: a placed one is moved back inside it, a large one is
//! clipped to it, so nothing ever runs over the title block. Each viewport
//! has a title under it: a bubble with its number on the sheet over the
//! sheet number, the title, and the scale with a small scale bar (or "NOT
//! TO SCALE"); plans also get a north arrow.
//!
//! Scales: a viewport with a scale keeps it. Viewports without one share
//! the largest common scale at which every viewport of the sheet still
//! packs into the drawing area, left to right and top to bottom in order,
//! around the placed ones. A sheet whose one viewport is a services plan
//! (lighting, power, plumbing, aircon) or the plumbing isometric is the
//! service sheet itself, legend, counts and blank professional block
//! included, drawn left of the title block.
//!
//! What other lanes draw: elevations, sections, the roof, site and ceiling
//! plans come from `views::items` (lane L3). Schedules come from `Derived`
//! (lane L1).
//!
//! A printed sheet never carries error or instruction text: a viewport
//! with nothing to print is left out and `left_out` says why, for the
//! export's result; only the Sheets panel's preview (`sheet_preview_svg`)
//! shows it as a dashed frame with the reason. A long schedule continues on
//! more sheets ("A-7.1", recounted in "N OF M"), every floor plan of the set
//! prints at one scale, and a services sheet carries its trade's blank
//! signing block instead of the architect's (`titleblock`).

use std::collections::BTreeMap;

use guhit_model::{
    Derived, Element, Id, MarkKind, Orientation, PaperSize, PlanExportOptions, Project, ScheduleKind,
    Sheet, SheetKind, ViewRef, Viewport,
};

use crate::geom::*;
use crate::plan::{build_items, items_bounds, Cat, Item, PlanOptions};
use crate::sheet::{
    drawing_title, label_masks, num, paper_size_mm, scale_bar_segment_mm, write_item, Layout, Svg,
    COMMON_SCALES, FONT_FAMILY, INK, MUTED,
};
use crate::tables;
use crate::text::{clean, est_width, fit, xml_escape};
use crate::titleblock::{self, Logo, SheetInfo};
use crate::{ExportError, PlanOutput, SheetInputs};

// ------------------------------------------------------------- the page

/// Where things go on one sheet, paper mm, SVG y down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Page {
    pub width: f64,
    pub height: f64,
    pub margin: f64,
    /// Type and furniture size, larger on larger paper.
    pub k: f64,
    /// The title block strip: x, y, width, height.
    pub title_block: (f64, f64, f64, f64),
    /// Where viewports may go: x, y, width, height.
    pub area: (f64, f64, f64, f64),
    /// The Sheets panel's preview: a viewport with nothing to print shows
    /// a dashed frame that says why, and the vicinity frame says how to
    /// place its image. A printed sheet leaves both out.
    pub preview: bool,
}

impl Page {
    pub fn new(paper: PaperSize, orientation: Orientation) -> Page {
        let (width, height) = paper_size_mm(paper, orientation);
        let l = Layout::new(paper, orientation, false);
        let (m, k) = (l.margin, l.k);
        let sw = titleblock::strip_width(k);
        let pad = 5.0 * k;
        Page {
            width,
            height,
            margin: m,
            k,
            title_block: (width - m - sw, m, sw, height - 2.0 * m),
            area: (m + pad, m + pad, width - 2.0 * m - sw - 2.0 * pad, height - 2.0 * m - 2.0 * pad),
            preview: false,
        }
    }
}

/// A rectangle, x, y, width, height.
pub type Rect = (f64, f64, f64, f64);

fn overlaps(a: Rect, b: Rect) -> bool {
    a.0 < b.0 + b.2 - 1e-6 && b.0 < a.0 + a.2 - 1e-6 && a.1 < b.1 + b.3 - 1e-6 && b.1 < a.1 + a.3 - 1e-6
}

fn inside(a: Rect, area: Rect) -> bool {
    a.0 >= area.0 - 1e-6 && a.1 >= area.1 - 1e-6 && a.0 + a.2 <= area.0 + area.2 + 1e-6 && a.1 + a.3 <= area.1 + area.3 + 1e-6
}

/// Height of the title under a viewport.
fn title_band(k: f64) -> f64 {
    13.0 * k
}

/// Space between viewports.
fn gap(k: f64) -> f64 {
    9.0 * k
}

// --------------------------------------------------------- viewport body

/// What a viewport shows.
enum Body {
    /// Model mm items at 1:n, centered on `bounds`.
    Drawing { items: Vec<Item>, bounds: Bounds },
    /// A fragment drawn from the origin, in paper mm.
    Fixed { svg: String, w: f64, h: f64 },
    /// A view not built yet, or with nothing to draw: a dashed frame.
    Missing { reason: String, w: f64, h: f64 },
}

impl Body {
    /// Paper size at 1:n.
    fn size(&self, n: f64) -> (f64, f64) {
        match self {
            Body::Drawing { bounds, .. } => (bounds.width() / n, bounds.height() / n),
            Body::Fixed { w, h, .. } | Body::Missing { w, h, .. } => (*w, *h),
        }
    }
}

/// How a view relates to scale.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Scaling {
    /// Drawn at 1:n.
    Scaled,
    /// An image or diagram: "NOT TO SCALE".
    NotToScale,
    /// A table or notes: no scale line.
    None,
}

fn scaling(view: &ViewRef) -> Scaling {
    match view {
        ViewRef::VicinityMap { .. } | ViewRef::Perspective { .. } => Scaling::NotToScale,
        ViewRef::Schedule { .. } | ViewRef::Notes { .. } => Scaling::None,
        _ => Scaling::Scaled,
    }
}

/// Plans get a north arrow in their title. The site plan draws its own.
fn plan_like(view: &ViewRef) -> bool {
    matches!(view, ViewRef::Plan { .. } | ViewRef::RoofPlan)
}

fn level_name(p: &Project, id: &Id) -> String {
    p.levels.iter().find(|l| &l.id == id).map(|l| clean(&l.name)).unwrap_or_default()
}

fn mark<'a>(p: &'a Project, id: &Id) -> Option<&'a guhit_model::ViewMark> {
    p.elements.iter().find_map(|e| match e {
        Element::ViewMark(m) if &m.id == id => Some(m),
        _ => None,
    })
}

/// The title a view gets when the viewport has none ("NORTH ELEVATION").
pub fn view_title(p: &Project, view: &ViewRef) -> String {
    match view {
        ViewRef::Plan { level_id, sheet } => {
            let name = level_name(p, level_id);
            match sheet {
                SheetKind::Plan => drawing_title(&name),
                SheetKind::ReflectedCeiling => {
                    let base = drawing_title(&name);
                    let base = base.strip_suffix(" PLAN").unwrap_or(&base).to_string();
                    format!("{base} REFLECTED CEILING PLAN")
                }
                SheetKind::PlumbingIsometric => "PLUMBING ISOMETRIC DIAGRAMS".into(),
                kind => crate::service_sheet::sheet_title(*kind, &name),
            }
        }
        ViewRef::Elevation { mark_id } => {
            let label = mark(p, mark_id).map(|m| clean(&m.label).to_uppercase()).unwrap_or_default();
            if label.is_empty() {
                "ELEVATION".into()
            } else if label.ends_with("ELEVATION") {
                label
            } else {
                format!("{label} ELEVATION")
            }
        }
        ViewRef::Section { mark_id } => {
            let label = mark(p, mark_id).map(|m| clean(&m.label).to_uppercase()).unwrap_or_default();
            if label.is_empty() {
                "SECTION".into()
            } else if label.starts_with("SECTION") {
                label
            } else {
                format!("SECTION {label}-{label}")
            }
        }
        ViewRef::RoofPlan => "ROOF PLAN".into(),
        ViewRef::SitePlan => "SITE DEVELOPMENT PLAN".into(),
        ViewRef::VicinityMap { .. } => "VICINITY MAP".into(),
        ViewRef::Perspective { .. } => "PERSPECTIVE".into(),
        ViewRef::Schedule { schedule } => tables::schedule_title(*schedule).into(),
        ViewRef::Notes { .. } => "GENERAL NOTES".into(),
    }
}

/// The project's buildings, all levels, model mm: what a view not built yet
/// is sized from.
fn building_bounds(d: &Derived) -> Bounds {
    let mut b = Bounds::empty();
    for f in &d.footprints {
        for p in &f.polygon {
            b.add(V::from(*p));
        }
    }
    if b.is_empty() {
        b.add(v(0.0, 0.0));
        b.add(v(10000.0, 8000.0));
    }
    b
}

/// Model size, mm, of a view not built yet.
fn estimate(p: &Project, d: &Derived, view: &ViewRef) -> (f64, f64) {
    let b = building_bounds(d);
    let tall: f64 = p.levels.iter().map(|l| l.height_mm.max(0.0)).sum::<f64>() + 2500.0;
    match view {
        ViewRef::Elevation { mark_id } | ViewRef::Section { mark_id } => {
            let along = mark(p, mark_id)
                .map(|m| {
                    let dir = (V::from(m.end) - V::from(m.start)).unit().unwrap_or(v(1.0, 0.0));
                    if dir.x.abs() >= dir.y.abs() {
                        b.width()
                    } else {
                        b.height()
                    }
                })
                .unwrap_or(b.width().max(b.height()));
            (along + 3000.0, tall + 1500.0)
        }
        _ => (b.width() + 4000.0, b.height() + 4000.0),
    }
}

/// Image bytes as a data URL part: MIME and base64. None when it is not a
/// PNG or a JPEG.
fn image_data(bytes: &[u8]) -> Option<(&'static str, String)> {
    let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else {
        return None;
    };
    Some((mime, base64(bytes)))
}

/// Width over height of a PNG or JPEG, from its header.
pub fn image_aspect(bytes: &[u8]) -> Option<f64> {
    let (w, h) = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) && bytes.len() >= 24 {
        let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        (w as f64, h as f64)
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        // Walk the JPEG markers to the first start of frame.
        let mut i = 2;
        loop {
            if i + 9 >= bytes.len() || bytes[i] != 0xff {
                return None;
            }
            let marker = bytes[i + 1];
            let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
            let sof = (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc);
            if sof {
                let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]);
                let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]);
                break (w as f64, h as f64);
            }
            i += 2 + len;
        }
    } else {
        return None;
    };
    (w > 0.0 && h > 0.0).then_some(w / h)
}

/// Standard base64 with padding. The export crate has no base64 dependency
/// and needs only this.
pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// An image framed in a `w` by `h` box at the origin.
fn image_svg(id: &str, mime: &str, b64: &str, w: f64, h: f64) -> String {
    format!(
        "<image id=\"{id}\" x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"xMidYMid meet\" href=\"data:{mime};base64,{b64}\"/>\n",
        num(w),
        num(h)
    )
}

/// A north arrow of radius `r` centered at (cx, cy), pointing to plan north.
fn north_arrow(s: &mut String, p: &Project, cx: f64, cy: f64, r: f64) {
    let a = p.settings.north_angle_deg;
    let a = if a.is_finite() { a } else { 0.0 };
    let (sn, cs) = a.to_radians().sin_cos();
    let d = (-sn, -cs);
    let q = (-d.1, d.0);
    let at = |fd: f64, fp: f64| (cx + d.0 * fd * r + q.0 * fp * r, cy + d.1 * fd * r + q.1 * fp * r);
    let poly = |pts: &[(f64, f64)], fill: &str| {
        let pts: Vec<String> = pts.iter().map(|p| format!("{},{}", num(p.0), num(p.1))).collect();
        format!(
            "<polygon points=\"{}\" fill=\"{fill}\" stroke=\"{INK}\" stroke-width=\"0.15\" stroke-linejoin=\"round\"/>\n",
            pts.join(" ")
        )
    };
    s.push_str("<g class=\"north-arrow\">\n");
    s.push_str(&format!(
        "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"{INK}\" stroke-width=\"0.2\"/>\n",
        num(cx),
        num(cy),
        num(r)
    ));
    let (tip, notch) = (at(0.92, 0.0), at(-0.3, 0.0));
    s.push_str(&poly(&[tip, at(-0.68, 0.46), notch], "#ffffff"));
    s.push_str(&poly(&[tip, at(-0.68, -0.46), notch], INK));
    let nl = at(1.0 + 0.55, 0.0);
    s.push_str(&format!(
        "<text x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"middle\" font-weight=\"bold\" fill=\"{INK}\" stroke=\"none\">N</text>\n",
        num(nl.0),
        num(nl.1 + 0.35 * r * 0.55),
        num(r * 0.55)
    ));
    s.push_str("</g>\n");
}

/// Notes text wrapped into a column, numbered by paragraph.
fn notes_body(text: &str, k: f64) -> Body {
    let w = 110.0 * k;
    let size = 2.0 * k;
    let indent = 4.0 * k;
    let mut s = String::from("<g class=\"notes\">\n");
    let mut y = 0.0;
    let paras: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    for (i, para) in paras.iter().enumerate() {
        let lines = crate::panel::wrap(&clean(para), size, w - indent);
        for (j, l) in lines.iter().enumerate() {
            y += 3.0 * k;
            if j == 0 && paras.len() > 1 {
                s.push_str(&format!(
                    "<text x=\"0\" y=\"{}\" font-size=\"{}\" fill=\"{INK}\" stroke=\"none\">{}.</text>\n",
                    num(y),
                    num(size),
                    i + 1
                ));
            }
            let x = if paras.len() > 1 { indent } else { 0.0 };
            s.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{INK}\" stroke=\"none\">{}</text>\n",
                num(x),
                num(y),
                num(size),
                xml_escape(l)
            ));
        }
        y += 1.2 * k;
    }
    s.push_str("</g>\n");
    if paras.is_empty() {
        return Body::Missing { reason: "No notes written yet.".into(), w: 80.0 * k, h: 20.0 * k };
    }
    Body::Fixed { svg: s, w, h: y + 1.0 * k }
}

/// The vicinity map: the placed image with the site marked at its center
/// and a north arrow (DECISIONS D53). No map service.
fn vicinity_body(p: &Project, file: &Option<String>, radius_m: f64, i: &SheetInputs, k: f64, preview: bool) -> Body {
    let bytes = file.as_ref().and_then(|f| i.images.get(f));
    let Some((mime, b64)) = bytes.and_then(|b| image_data(b)) else {
        // No image yet: the empty frame with the site, the radius and the
        // north arrow, ready to fill by hand. How to place one is the
        // panel's business, never the printed sheet's.
        let (w, h) = (110.0 * k, 82.5 * k);
        let (cx, cy) = (w / 2.0, h / 2.0);
        let r = h * 0.42;
        let mut s = format!(
            "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{INK}\" stroke-width=\"0.35\"/>\n<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke=\"{INK}\" stroke-width=\"0.18\" stroke-dasharray=\"2.2 1.1\"/>\n<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{INK}\"/>\n",
            num(w),
            num(h),
            num(cx),
            num(cy),
            num(r),
            num(cx),
            num(cy),
            num(0.9 * k)
        );
        let radius = if radius_m.is_finite() && radius_m > 0.0 { radius_m } else { 500.0 };
        s.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"middle\" font-weight=\"bold\" fill=\"{INK}\" stroke=\"none\">SITE</text>\n<text x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"middle\" fill=\"{INK}\" stroke=\"none\">{} m RADIUS</text>\n",
            num(cx),
            num(cy - 2.4 * k),
            num(2.2 * k),
            num(cx),
            num(cy - r + 3.5 * k),
            num(1.8 * k),
            trim_m(radius * 1000.0)
        ));
        if preview {
            let note = if file.is_some() { "The vicinity map image is missing from the project files." } else { "Place a vicinity map image in the Sheets panel." };
            let (t, ts) = fit(note, 2.2 * k, 1.4 * k, w - 8.0 * k, false);
            s.push_str(&format!(
                "<text class=\"preview-note\" x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"middle\" fill=\"{MUTED}\" stroke=\"none\">{}</text>\n",
                num(cx),
                num(cy + 8.0 * k),
                num(ts),
                xml_escape(&t)
            ));
        }
        let nr = 4.0 * k;
        north_arrow(&mut s, p, w - nr - 3.0 * k, nr + 5.0 * k, nr);
        return Body::Fixed { svg: s, w, h };
    };
    let aspect = bytes.and_then(|b| image_aspect(b)).unwrap_or(4.0 / 3.0).clamp(0.6, 2.2);
    let w = 110.0 * k;
    let h = w / aspect;
    let mut s = image_svg("vicinity-image", mime, &b64, w, h);
    s.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{INK}\" stroke-width=\"0.35\"/>\n",
        num(w),
        num(h)
    ));
    let (cx, cy) = (w / 2.0, h / 2.0);
    let r = 2.6 * k;
    s.push_str(&format!(
        "<g id=\"vicinity-site\"><circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"#ffffff\" fill-opacity=\"0.6\" stroke=\"#c0392b\" stroke-width=\"0.5\"/><circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"#c0392b\"/>",
        num(cx),
        num(cy),
        num(r),
        num(cx),
        num(cy),
        num(0.7 * k)
    ));
    s.push_str(&format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#ffffff\" stroke=\"none\"/><text x=\"{}\" y=\"{}\" font-size=\"{}\" font-weight=\"bold\" fill=\"#c0392b\" stroke=\"none\">SITE</text></g>\n",
        num(cx + r + 0.8 * k),
        num(cy - 2.2 * k),
        num(est_width("SITE", 2.2 * k, true) + 1.2 * k),
        num(3.0 * k),
        num(cx + r + 1.4 * k),
        num(cy),
        num(2.2 * k)
    ));
    let nr = 4.0 * k;
    s.push_str(&format!(
        "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"#ffffff\" fill-opacity=\"0.85\" stroke=\"none\"/>\n",
        num(w - nr - 3.0 * k),
        num(nr + 4.0 * k),
        num(nr * 1.8)
    ));
    north_arrow(&mut s, p, w - nr - 3.0 * k, nr + 5.0 * k, nr);
    Body::Fixed { svg: s, w, h }
}

/// A model render (never an AI image, D54), framed.
fn perspective_body(render_id: &Id, i: &SheetInputs, k: f64) -> Body {
    let bytes = i.images.get(render_id);
    let Some((mime, b64)) = bytes.and_then(|b| image_data(b)) else {
        return Body::Missing { reason: "The perspective render is missing from Visuals.".into(), w: 150.0 * k, h: 95.0 * k };
    };
    let aspect = bytes.and_then(|b| image_aspect(b)).unwrap_or(16.0 / 9.0).clamp(0.5, 3.0);
    let (max_w, max_h) = (160.0 * k, 110.0 * k);
    let (w, h) = if max_w / aspect <= max_h { (max_w, max_w / aspect) } else { (max_h * aspect, max_h) };
    let mut s = image_svg("perspective-image", mime, &b64, w, h);
    s.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{INK}\" stroke-width=\"0.25\"/>\n",
        num(w),
        num(h)
    ));
    Body::Fixed { svg: s, w, h }
}

/// Body of one viewport at 1:n. `max_h` bounds tables; `part` is the rows
/// of a schedule this sheet prints.
#[allow(clippy::too_many_arguments)]
fn body(p: &Project, d: &Derived, view: &ViewRef, n: u32, i: &SheetInputs, k: f64, max_h: f64, part: Option<&SchedulePart>, preview: bool) -> Body {
    let nf = n as f64;
    match view {
        ViewRef::Plan { level_id, sheet } => {
            let Some(level) = p.levels.iter().find(|l| &l.id == level_id) else {
                return Body::Missing { reason: "This level was deleted.".into(), w: 80.0 * k, h: 40.0 * k };
            };
            match sheet {
                SheetKind::Plan => {
                    let items = build_items(
                        p,
                        d,
                        level,
                        &PlanOptions {
                            scale: nf,
                            show_dimensions: true,
                            show_room_labels: true,
                            show_assets: true,
                            unicode: true,
                            skip_devices: false,
                        },
                    );
                    let bounds = items_bounds(&items);
                    if bounds.is_empty() {
                        return Body::Missing {
                            reason: format!("Level \"{}\" has nothing to draw yet.", clean(&level.name)),
                            w: 80.0 * k,
                            h: 40.0 * k,
                        };
                    }
                    Body::Drawing { items, bounds }
                }
                _ => view_items(p, d, view, n, k),
            }
        }
        ViewRef::Elevation { .. } | ViewRef::Section { .. } | ViewRef::RoofPlan | ViewRef::SitePlan => {
            view_items(p, d, view, n, k)
        }
        ViewRef::VicinityMap { file_name, radius_m } => vicinity_body(p, file_name, *radius_m, i, k, preview),
        ViewRef::Perspective { render_id } => perspective_body(render_id, i, k),
        ViewRef::Schedule { schedule } => {
            let mut t = tables::schedule(p, d, *schedule);
            if let Some(part) = part {
                t = tables::part(&t, part.from, part.to, part.continued_on.as_deref());
            }
            let r = tables::render(&t, table_k(k), max_h);
            Body::Fixed { svg: r.svg, w: r.w, h: r.h }
        }
        ViewRef::Notes { text } => notes_body(text, k),
    }
}

/// Tables grow slower than the sheet furniture on large paper.
fn table_k(k: f64) -> f64 {
    1.0 + (k - 1.0) * 0.5
}

/// A view lane L3 draws (`views::items`), or a dashed frame of about its
/// size while it is not built.
fn view_items(p: &Project, d: &Derived, view: &ViewRef, n: u32, k: f64) -> Body {
    match crate::views::items(p, d, view, Some(n)) {
        Ok(v) if !v.items.is_empty() && !v.bounds.is_empty() => Body::Drawing { items: v.items, bounds: v.bounds },
        Ok(_) => Body::Missing { reason: "Nothing to draw in this view yet.".into(), w: 80.0 * k, h: 40.0 * k },
        Err(e) => {
            let (mw, mh) = estimate(p, d, view);
            let reason = match e {
                ExportError::NotBuilt(_) => "This drawing is not built yet.".to_string(),
                ExportError::Empty(why) | ExportError::Failed(why) => sentence(&why),
            };
            Body::Missing { reason, w: (mw / n as f64).max(60.0 * k), h: (mh / n as f64).max(30.0 * k) }
        }
    }
}

/// What an error says, without its "export failed" prefix.
fn reason_of(e: &ExportError) -> String {
    match e {
        ExportError::Empty(why) | ExportError::Failed(why) | ExportError::NotBuilt(why) => why.clone(),
    }
}

/// What every printed sheet of `ids` (empty: all) leaves out, each line
/// "A-6 SECTIONS: SECTION C-C: Nothing to draw in this view yet.", for an
/// export's result.
pub fn left_out(p: &Project, d: &Derived, ids: &[Id], i: &SheetInputs) -> Result<Vec<String>, ExportError> {
    Ok(printed_sheets(p, d, ids, i)?
        .into_iter()
        .flat_map(|s| {
            let head = format!("{} {}", clean(&s.number), clean(&s.title).to_uppercase());
            s.left_out.into_iter().map(move |w| format!("{head}: {w}"))
        })
        .collect())
}

/// A reason as a sentence: "Nothing to draw: the mark looks away."
fn sentence(why: &str) -> String {
    let t = clean(why);
    let mut cs = t.chars();
    let mut out: String = match cs.next() {
        Some(f) => f.to_uppercase().chain(cs).collect(),
        None => return "Nothing to draw in this view.".into(),
    };
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

// --------------------------------------------------------------- layout

/// One viewport laid out on its sheet.
struct Placed<'a> {
    vp: &'a Viewport,
    /// 1-based number on the sheet.
    number: usize,
    title: String,
    scaling: Scaling,
    /// The scale it is drawn at (Scaled only).
    scale: u32,
    body: Body,
    /// The drawing or table, paper mm, SVG y down.
    content: Rect,
    /// Content and title together.
    frame: Rect,
    /// Nothing to print (a printed sheet, not the preview): left out of the
    /// sheet, never a frame with a reason on it.
    out: bool,
}

/// What a sheet's layout says, for tests and the panel.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportBox {
    pub id: Id,
    /// Content and title, x, y, width, height, SVG y down.
    pub frame: Rect,
    pub content: Rect,
    /// None: not to scale.
    pub scale: Option<u32>,
}

/// Width a viewport title needs.
fn title_width(title: &str, k: f64) -> f64 {
    let bubble = 9.5 * k;
    bubble + 3.0 * k + est_width(title, 3.2 * k, true).min(150.0 * k) + 4.0 * k
}

/// Frame size around a content size: the title below, and at least the
/// title's width.
fn frame_size(content: (f64, f64), title: &str, k: f64) -> (f64, f64) {
    (content.0.max(title_width(title, k).min(120.0 * k)), content.1 + title_band(k))
}

/// Place frames of `sizes` in order, top left first, around `fixed`
/// frames, inside `area`. None when one does not fit.
fn pack(area: Rect, fixed: &[Rect], sizes: &[(f64, f64)], g: f64) -> Option<Vec<Rect>> {
    let mut placed: Vec<Rect> = fixed.to_vec();
    let mut out = Vec::new();
    for &(w, h) in sizes {
        let mut xs = vec![area.0];
        let mut ys = vec![area.1];
        for r in &placed {
            xs.push(r.0 + r.2 + g);
            ys.push(r.1 + r.3 + g);
        }
        let mut cands: Vec<(f64, f64)> = ys.iter().flat_map(|y| xs.iter().map(move |x| (*x, *y))).collect();
        cands.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)));
        let spot = cands.into_iter().find(|&(x, y)| {
            let r = (x, y, w, h);
            let grown = (x - g / 2.0, y - g / 2.0, w + g, h + g);
            inside(r, area) && !placed.iter().any(|o| overlaps(grown, (o.0 - g / 2.0, o.1 - g / 2.0, o.2 + g, o.3 + g)))
        })?;
        let r = (spot.0, spot.1, w, h);
        placed.push(r);
        out.push(r);
    }
    Some(out)
}

/// A placed viewport's frame from its center (paper mm from the bottom
/// left), moved back inside the area when it pokes out.
fn fixed_frame(page: &Page, center: guhit_model::Point, size: (f64, f64), content_h: f64) -> Rect {
    let (ax, ay, aw, ah) = page.area;
    let (w, h) = size;
    // The center is the content's; the title hangs below it.
    let cx = if center.x.is_finite() { center.x } else { ax + aw / 2.0 };
    let cy = if center.y.is_finite() { page.height - center.y } else { ay + ah / 2.0 };
    let x = (cx - w / 2.0).min(ax + aw - w).max(ax);
    let y = (cy - content_h / 2.0).min(ay + ah - h).max(ay);
    (x, y, w, h)
}

/// The title of a viewport, or of its view.
fn title_of(p: &Project, vp: &Viewport) -> String {
    vp.title.as_deref().map(clean).filter(|t| !t.is_empty()).map(|t| t.to_uppercase()).unwrap_or_else(|| view_title(p, &vp.view))
}

/// Lay out every viewport of a sheet, with the schedule rows `parts` gives
/// each (none: all of them). Also says whether everything fit; when it
/// does not, what is left over is placed in rows and clipped to the area.
fn layout<'a>(p: &Project, d: &Derived, sheet: &'a Sheet, page: &Page, i: &SheetInputs, parts: &[Option<SchedulePart>]) -> (Vec<Placed<'a>>, bool) {
    let part = |j: usize| parts.get(j).and_then(|x| x.as_ref());
    let k = page.k;
    let g = gap(k);
    let max_h = page.area.3 - title_band(k);
    let project_scale = Some(p.settings.scale_denominator).filter(|n| *n > 0).unwrap_or(100);
    // Bodies at a reference scale: the viewport's own, else the project's.
    let reference: Vec<(u32, Body)> = sheet
        .viewports
        .iter()
        .enumerate()
        .map(|(j, vp)| {
            let n = vp.scale_denominator.filter(|n| *n > 0).unwrap_or(project_scale);
            (n, body(p, d, &vp.view, n, i, k, max_h, part(j), page.preview))
        })
        .collect();
    let titles: Vec<String> = sheet.viewports.iter().map(|vp| title_of(p, vp)).collect();
    let auto_scale = |vp: &Viewport| vp.scale_denominator.filter(|n| *n > 0).is_none() && scaling(&vp.view) == Scaling::Scaled;
    let any_auto = sheet.viewports.iter().any(auto_scale);

    // Content size of viewport j at 1:n, from its reference body.
    let size_at = |j: usize, n: u32, bodies: &[(u32, Body)]| -> (f64, f64) {
        let (rn, b) = &bodies[j];
        match b {
            Body::Drawing { bounds, .. } => (bounds.width() / n as f64, bounds.height() / n as f64),
            Body::Missing { w, h, .. } if scaling(&sheet.viewports[j].view) == Scaling::Scaled => {
                (w * *rn as f64 / n as f64, h * *rn as f64 / n as f64)
            }
            other => other.size(*rn as f64),
        }
    };
    // A printed sheet leaves out what has nothing to print.
    let gone = |j: usize, bodies: &[(u32, Body)]| !page.preview && matches!(bodies[j].1, Body::Missing { .. });
    let try_layout = |n: u32, bodies: &[(u32, Body)]| -> Option<Vec<Rect>> {
        let mut fixed = Vec::new();
        let mut sizes = Vec::new();
        let mut order = Vec::new();
        let mut frames = vec![(0.0, 0.0, 0.0, 0.0); sheet.viewports.len()];
        for (j, vp) in sheet.viewports.iter().enumerate() {
            if gone(j, bodies) {
                continue;
            }
            let scale = if auto_scale(vp) { n } else { bodies[j].0 };
            let c = size_at(j, scale, bodies);
            let f = frame_size(c, &titles[j], k);
            match vp.position {
                Some(center) => {
                    let r = fixed_frame(page, center, f, c.1);
                    // A placed viewport without a scale still has to fit the
                    // area and stay clear of the other placed ones.
                    if auto_scale(vp) && (f.0 > page.area.2 + 1e-6 || f.1 > page.area.3 + 1e-6 || fixed.iter().any(|o| overlaps(r, *o))) {
                        return None;
                    }
                    frames[j] = r;
                    fixed.push(r);
                }
                None => {
                    sizes.push(f);
                    order.push(j);
                }
            }
        }
        let packed = pack(page.area, &fixed, &sizes, g)?;
        for (r, j) in packed.into_iter().zip(order) {
            frames[j] = r;
        }
        Some(frames)
    };

    // The largest common scale that fits, for viewports without one.
    let mut bodies = reference;
    let mut chosen = project_scale;
    let mut frames = None;
    if any_auto {
        let candidates: Vec<u32> = COMMON_SCALES.to_vec();
        let start = candidates.iter().position(|n| try_layout(*n, &bodies).is_some()).unwrap_or(candidates.len() - 1);
        for &n in &candidates[start..] {
            // Rebuild at n: annotation grows with the scale.
            let exact: Vec<(u32, Body)> = sheet
                .viewports
                .iter()
                .enumerate()
                .map(|(j, vp)| {
                    if auto_scale(vp) && bodies[j].0 != n {
                        (n, body(p, d, &vp.view, n, i, k, max_h, part(j), page.preview))
                    } else {
                        (bodies[j].0, std::mem::replace(&mut bodies[j].1, Body::Missing { reason: String::new(), w: 0.0, h: 0.0 }))
                    }
                })
                .collect();
            bodies = exact;
            chosen = n;
            if let Some(f) = try_layout(n, &bodies) {
                frames = Some(f);
                break;
            }
        }
    }
    let frames = frames.or_else(|| try_layout(chosen, &bodies));
    let fits = frames.is_some();
    let frames = frames.unwrap_or_else(|| {
        // Nothing fits even at the smallest scale: placed ones where they
        // were put, the rest in rows from the top left, inside the area as
        // far as they go; the area clip cuts the rest and keeps everything
        // off the title block.
        let (ax, ay, aw, ah) = page.area;
        let (mut x, mut y, mut row_h) = (ax, ay, 0.0_f64);
        sheet
            .viewports
            .iter()
            .enumerate()
            .map(|(j, vp)| {
                if gone(j, &bodies) {
                    return (0.0, 0.0, 0.0, 0.0);
                }
                let scale = if auto_scale(vp) { chosen } else { bodies[j].0 };
                let c = size_at(j, scale, &bodies);
                let f = frame_size(c, &titles[j], k);
                if let Some(center) = vp.position {
                    return fixed_frame(page, center, f, c.1);
                }
                if x > ax && x + f.0 > ax + aw {
                    x = ax;
                    y += row_h + g;
                    row_h = 0.0;
                }
                let r = (x, y.min(ay + ah - f.1).max(ay), f.0, f.1);
                x += f.0 + g;
                row_h = row_h.max(f.1);
                r
            })
            .collect()
    });

    // A lone unplaced viewport sits in the middle of the area.
    let mut frames = frames;
    let printed: Vec<usize> = (0..frames.len()).filter(|j| !gone(*j, &bodies)).collect();
    if sheet.viewports.iter().all(|vp| vp.position.is_none()) && !printed.is_empty() {
        let mut gb = Bounds::empty();
        for f in printed.iter().map(|j| frames[*j]) {
            gb.add(v(f.0, f.1));
            gb.add(v(f.0 + f.2, f.1 + f.3));
        }
        let (ax, ay, aw, ah) = page.area;
        let dx = ((aw - gb.width()) / 2.0).max(0.0) + ax - gb.min.x;
        let dy = ((ah - gb.height()) / 2.0).max(0.0) + ay - gb.min.y;
        for f in frames.iter_mut() {
            f.0 += dx;
            f.1 += dy;
        }
    }

    let outs: Vec<bool> = (0..sheet.viewports.len()).map(|j| gone(j, &bodies)).collect();
    let mut number = 0;
    let placed = sheet
        .viewports
        .iter()
        .zip(bodies)
        .zip(frames)
        .zip(titles)
        .enumerate()
        .map(|(j, (((vp, (n, body)), frame), title))| {
            let out = outs[j];
            if !out {
                number += 1;
            }
            let scale = if auto_scale(vp) { chosen } else { n };
            let (cw, ch) = match &body {
                Body::Drawing { bounds, .. } => (bounds.width() / scale as f64, bounds.height() / scale as f64),
                Body::Missing { w, h, .. } if scaling(&vp.view) == Scaling::Scaled => (w * n as f64 / scale as f64, h * n as f64 / scale as f64),
                other => other.size(scale as f64),
            };
            let content = (frame.0 + (frame.2 - cw) / 2.0, frame.1, cw, ch);
            Placed { vp, number, title, scaling: scaling(&vp.view), scale, body, content, frame, out }
        })
        .collect::<Vec<_>>();
    (placed, fits)
}

/// The part of `r` inside `area`.
fn clip_to(r: Rect, area: Rect) -> Rect {
    let (ax1, ay1) = (area.0 + area.2, area.1 + area.3);
    let x0 = r.0.clamp(area.0, ax1);
    let y0 = r.1.clamp(area.1, ay1);
    let x1 = (r.0 + r.2).clamp(area.0, ax1);
    let y1 = (r.1 + r.3).clamp(area.1, ay1);
    (x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}

/// The viewport boxes of a sheet as the Sheets panel lays it out (every
/// viewport, one with nothing to print as its dashed frame): each clipped
/// to the drawing area, as the sheet clips them. For tests and readers;
/// `printed_sheets` gives the printed set.
pub fn viewport_boxes(p: &Project, d: &Derived, sheet: &Sheet, i: &SheetInputs) -> (Page, Vec<ViewportBox>) {
    let page = Page { preview: true, ..Page::new(sheet.paper, sheet.orientation) };
    let boxes = layout(p, d, sheet, &page, i, &[])
        .0
        .into_iter()
        .map(|pl| ViewportBox {
            id: pl.vp.id.clone(),
            frame: clip_to(pl.frame, page.area),
            content: clip_to(pl.content, page.area),
            scale: (pl.scaling == Scaling::Scaled).then_some(pl.scale),
        })
        .collect();
    (page, boxes)
}

// ---------------------------------------------------------------- drawing

fn svg_head(w: f64, h: f64, title: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\" font-family=\"{FONT_FAMILY}\">\n<title>{}</title>\n<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>\n",
        num(w),
        num(h),
        num(w),
        num(h),
        xml_escape(title),
        num(w),
        num(h)
    )
}

/// Write one viewport's content and title.
fn write_viewport(s: &mut String, p: &Project, pl: &Placed, sheet_number: &str, k: f64) {
    let (x, y, w, h) = pl.content;
    let f = pl.frame;
    s.push_str(&format!(
        "<g class=\"viewport\" id=\"vp-{}\" data-id=\"{}\" data-x=\"{}\" data-y=\"{}\" data-w=\"{}\" data-h=\"{}\" data-frame=\"{} {} {} {}\" data-scale=\"{}\" data-placed=\"{}\">\n",
        xml_escape(&pl.vp.id),
        xml_escape(&pl.vp.id),
        num(x),
        num(y),
        num(w),
        num(h),
        num(f.0),
        num(f.1),
        num(f.2),
        num(f.3),
        if pl.scaling == Scaling::Scaled { pl.scale.to_string() } else { String::new() },
        pl.vp.position.is_some()
    ));
    let clip_id = format!("vpc-{}", pl.number);
    s.push_str(&format!(
        "<clipPath id=\"{clip_id}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath>\n",
        num(f.0 - 1.0 * k),
        num(f.1 - 1.0 * k),
        num(f.2 + 2.0 * k),
        num(h + 1.5 * k)
    ));
    s.push_str(&format!("<g clip-path=\"url(#{clip_id})\">\n"));
    match &pl.body {
        Body::Drawing { items, bounds } => {
            let n = pl.scale as f64;
            let c = bounds.center();
            let (ox, oy) = (x + w / 2.0, y + h / 2.0);
            let to_paper = |q: V| -> (f64, f64) { (ox + (q.x - c.x) / n, oy - (q.y - c.y) / n) };
            s.push_str(&format!(
                "<g class=\"drawing\" stroke-linecap=\"butt\" stroke-linejoin=\"miter\" fill=\"none\" stroke=\"{INK}\">\n"
            ));
            for cat in Cat::ALL {
                let group: Vec<&Item> = items.iter().filter(|i| i.cat == cat).collect();
                if cat == Cat::Area && !group.is_empty() {
                    label_masks(s, &group, n, &to_paper);
                }
                for item in group {
                    write_item(s, item, n, &to_paper);
                }
            }
            // Categories a lane adds later still draw, last.
            for item in items.iter().filter(|i| !Cat::ALL.contains(&i.cat)) {
                write_item(s, item, n, &to_paper);
            }
            s.push_str("</g>\n");
        }
        Body::Fixed { svg, .. } => {
            s.push_str(&format!("<g transform=\"translate({} {})\">\n", num(x), num(y)));
            s.push_str(svg);
            s.push_str("</g>\n");
        }
        Body::Missing { reason, .. } => {
            s.push_str(&format!(
                "<rect class=\"missing\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#f4f6f9\" stroke=\"{MUTED}\" stroke-width=\"0.25\" stroke-dasharray=\"2 1.2\"/>\n",
                num(x),
                num(y),
                num(w),
                num(h)
            ));
            let (t, ts) = fit(reason, 2.4 * k, 1.4 * k, w - 4.0 * k, false);
            s.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"middle\" fill=\"{MUTED}\" stroke=\"none\">{}</text>\n",
                num(x + w / 2.0),
                num(y + h / 2.0 + 0.36 * ts),
                num(ts),
                xml_escape(&t)
            ));
        }
    }
    s.push_str("</g>\n");

    // The title under it.
    let ty = y + h + 2.0 * k;
    let r = 4.4 * k;
    let bx = f.0 + r;
    let by = ty + r + 0.6 * k;
    s.push_str("<g class=\"viewport-title\">\n");
    s.push_str(&format!(
        "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"#ffffff\" stroke=\"{INK}\" stroke-width=\"0.3\"/>\n<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{INK}\" stroke-width=\"0.2\"/>\n",
        num(bx),
        num(by),
        num(r),
        num(bx - r),
        num(by),
        num(bx + r),
        num(by)
    ));
    s.push_str(&format!(
        "<text class=\"vp-number\" x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"middle\" font-weight=\"bold\" fill=\"{INK}\" stroke=\"none\">{}</text>\n",
        num(bx),
        num(by - 1.0 * k),
        num(2.6 * k),
        pl.number
    ));
    let (sn, sns) = fit(&clean(sheet_number), 1.8 * k, 1.0 * k, 1.7 * r, false);
    s.push_str(&format!(
        "<text x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"middle\" fill=\"{INK}\" stroke=\"none\">{}</text>\n",
        num(bx),
        num(by + 2.6 * k),
        num(sns),
        xml_escape(&sn)
    ));
    let tx = bx + r + 2.5 * k;
    let north_room = if plan_like(&pl.vp.view) { 10.0 * k } else { 0.0 };
    let max_w = (f.0 + f.2 - tx - north_room).max(20.0 * k);
    let (title, size) = fit(&pl.title, 3.2 * k, 2.0 * k, max_w, true);
    let base = ty + 4.4 * k;
    s.push_str(&format!(
        "<text class=\"vp-title\" x=\"{}\" y=\"{}\" font-size=\"{}\" font-weight=\"bold\" fill=\"{INK}\" stroke=\"none\">{}</text>\n",
        num(tx),
        num(base),
        num(size),
        xml_escape(&title)
    ));
    let tw = est_width(&title, size, true).min(max_w);
    s.push_str(&format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{INK}\" stroke-width=\"0.4\"/>\n",
        num(tx),
        num(base + 1.3 * k),
        num(tx + tw),
        num(base + 1.3 * k)
    ));
    let line2 = base + 5.0 * k;
    match pl.scaling {
        Scaling::Scaled => {
            let label = format!("SCALE 1:{}", pl.scale);
            s.push_str(&format!(
                "<text class=\"vp-scale\" x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{INK}\" stroke=\"none\">{}</text>\n",
                num(tx),
                num(line2),
                num(2.0 * k),
                label
            ));
            // A four segment scale bar after it, where there is room.
            let bar_x = tx + est_width(&label, 2.0 * k, false) + 3.0 * k;
            let room = f.0 + f.2 - north_room - bar_x;
            let seg_mm = scale_bar_segment_mm(pl.scale, room.min(40.0 * k));
            let seg = seg_mm / pl.scale as f64;
            if room >= 4.0 * seg && seg > 0.0 {
                let bh = 1.1 * k;
                let by = line2 - bh - 0.2 * k;
                s.push_str(&format!(
                    "<g class=\"vp-scale-bar\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#ffffff\" stroke=\"{INK}\" stroke-width=\"0.15\"/>",
                    num(bar_x),
                    num(by),
                    num(4.0 * seg),
                    num(bh)
                ));
                for q in [0.0, 2.0] {
                    s.push_str(&format!(
                        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{INK}\" stroke=\"none\"/>",
                        num(bar_x + q * seg),
                        num(by),
                        num(seg),
                        num(bh)
                    ));
                }
                let end = format!("{} m", trim_m(seg_mm * 4.0));
                s.push_str(&format!(
                    "<text x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{MUTED}\" stroke=\"none\">0</text><text x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"end\" fill=\"{MUTED}\" stroke=\"none\">{}</text></g>\n",
                    num(bar_x),
                    num(by - 0.6 * k),
                    num(1.4 * k),
                    num(bar_x + 4.0 * seg),
                    num(by - 0.6 * k),
                    num(1.4 * k),
                    end
                ));
            }
        }
        Scaling::NotToScale => {
            let extra = match &pl.vp.view {
                ViewRef::VicinityMap { radius_m, .. } => format!(" - {} M RADIUS FROM THE SITE", trim_m(*radius_m * 1000.0)),
                ViewRef::Perspective { .. } => " - MODEL RENDER".into(),
                _ => String::new(),
            };
            s.push_str(&format!(
                "<text class=\"vp-scale\" x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{INK}\" stroke=\"none\">NOT TO SCALE{}</text>\n",
                num(tx),
                num(line2),
                num(2.0 * k),
                extra
            ));
        }
        Scaling::None => {}
    }
    if plan_like(&pl.vp.view) {
        north_arrow(s, p, f.0 + f.2 - 4.0 * k, ty + 6.0 * k, 3.4 * k);
    }
    s.push_str("</g>\n</g>\n");
}

/// "2.5" for 2500 mm.
fn trim_m(mm: f64) -> String {
    let t = format!("{:.2}", mm / 1000.0);
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The sheet's SCALE cell: one scale, "AS SHOWN", "NTS" or blank.
fn block_scale(placed: &[Placed]) -> String {
    let mut scales: Vec<u32> = placed.iter().filter(|p| !p.out && p.scaling == Scaling::Scaled).map(|p| p.scale).collect();
    scales.sort_unstable();
    scales.dedup();
    let nts = placed.iter().any(|p| !p.out && p.scaling == Scaling::NotToScale);
    match (scales.len(), nts) {
        (0, false) => String::new(),
        (0, true) => "NTS".into(),
        (1, false) => format!("1:{}", scales[0]),
        _ => "AS SHOWN".into(),
    }
}

/// The title block date: the caller's, else the day the project last
/// changed.
fn sheet_date(p: &Project, i: &SheetInputs) -> String {
    let ymd = |t: &str| -> Option<String> {
        let b = t.trim().as_bytes();
        let ok = b.len() >= 10
            && b[..10].iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() });
        ok.then(|| t.trim()[..10].to_string())
    };
    i.date.as_deref().and_then(ymd).or_else(|| ymd(&p.updated_at)).unwrap_or_default()
}

/// A service sheet (lighting, power, plumbing, aircon) or the plumbing
/// isometric, when it is the sheet's one viewport.
fn service_kind(sheet: &Sheet) -> Option<(&Viewport, Id, SheetKind)> {
    let isometric = |vp: &Viewport| matches!(vp.view, ViewRef::Plan { sheet: SheetKind::PlumbingIsometric, .. });
    match sheet.viewports.as_slice() {
        [vp] => match &vp.view {
            ViewRef::Plan { level_id, sheet: kind }
                if matches!(
                    kind,
                    SheetKind::Lighting | SheetKind::Power | SheetKind::Plumbing | SheetKind::Aircon | SheetKind::PlumbingIsometric
                ) =>
            {
                Some((vp, level_id.clone(), *kind))
            }
            _ => None,
        },
        // The isometric diagrams show the whole building at once: a sheet
        // of them, one viewport per level, is the one isometric sheet.
        [vp, ..] if sheet.viewports.iter().all(isometric) => match &vp.view {
            ViewRef::Plan { level_id, .. } => Some((vp, level_id.clone(), SheetKind::PlumbingIsometric)),
            _ => None,
        },
        _ => None,
    }
}

/// The trade of a sheet whose every viewport is a services plan or the
/// plumbing isometric of one trade: its signing professional (DECISIONS
/// D19), whose blank block the title block carries in place of the
/// architect's. None for an architectural sheet.
fn discipline(sheet: &Sheet) -> Option<&'static str> {
    let mut out: Option<&'static str> = None;
    for vp in &sheet.viewports {
        let ViewRef::Plan { sheet: kind, .. } = &vp.view else { return None };
        if !matches!(kind, SheetKind::Lighting | SheetKind::Power | SheetKind::Plumbing | SheetKind::Aircon | SheetKind::PlumbingIsometric) {
            return None;
        }
        let who = crate::service_sheet::signatory(*kind);
        if out.is_some_and(|o| o != who) {
            return None;
        }
        out = Some(who);
    }
    out
}

/// The service sheet drawn left of the title block. Returns its SVG content
/// (a nested `<svg>`) and the SCALE cell.
#[allow(clippy::too_many_arguments)]
fn service_sheet_svg(
    p: &Project,
    d: &Derived,
    sheet: &Sheet,
    page: &Page,
    vp: &Viewport,
    level_id: Id,
    kind: SheetKind,
    date: &str,
) -> Result<(String, String), ExportError> {
    let w = page.title_block.0 + page.margin;
    let layout = Layout::sized(sheet.paper, w, page.height, false);
    let opts = PlanExportOptions {
        level_id: Some(level_id),
        paper: sheet.paper,
        orientation: sheet.orientation,
        scale_denominator: vp.scale_denominator.filter(|n| *n > 0),
        show_dimensions: true,
        show_room_labels: true,
        show_assets: true,
        title_block: false,
        show_pipes: true,
        sheet: kind,
        review_page: false,
        date: (!date.is_empty()).then(|| date.to_string()),
    };
    let (inner, scale) = if kind == SheetKind::PlumbingIsometric {
        crate::iso::render_in(p, d, &opts, layout)?
    } else {
        crate::service_sheet::render_in(p, d, &opts, layout)?
    };
    // Nest it: drop the XML declaration and size it in user units.
    let body = inner.split_once("?>\n").map(|(_, rest)| rest).unwrap_or(&inner);
    let body = body.replacen(
        &format!("width=\"{}mm\" height=\"{}mm\"", num(w), num(page.height)),
        &format!("x=\"0\" y=\"0\" width=\"{}\" height=\"{}\"", num(w), num(page.height)),
        1,
    );
    let cell = if kind == SheetKind::PlumbingIsometric { "NTS".to_string() } else { format!("1:{scale}") };
    Ok((format!("<g id=\"service-sheet\">\n{body}</g>\n"), cell))
}

/// Draw one printed sheet. `index` is its 1-based place among `total`;
/// `preview` is the Sheets panel's picture (`Page::preview`).
fn draw_sheet(p: &Project, d: &Derived, printed: &Printed, index: usize, total: usize, i: &SheetInputs, preview: bool) -> Result<String, ExportError> {
    let sheet = &printed.sheet;
    let page = Page { preview, ..Page::new(sheet.paper, sheet.orientation) };
    let k = page.k;
    let date = sheet_date(p, i);
    let mut svg = Svg {
        s: svg_head(page.width, page.height, &format!("{} - {} {}", clean(&p.name), clean(&sheet.number), clean(&sheet.title))),
    };
    let (ax, ay, aw, ah) = page.area;
    svg.s.push_str(&format!(
        "<g id=\"drawing-area\" data-x=\"{}\" data-y=\"{}\" data-w=\"{}\" data-h=\"{}\" data-page=\"{} {}\"/>\n",
        num(ax),
        num(ay),
        num(aw),
        num(ah),
        num(page.width),
        num(page.height)
    ));
    let mut scale_cell = String::new();
    let mut drew_service = false;
    if let Some((vp, level_id, kind)) = service_kind(sheet) {
        if let Ok((content, cell)) = service_sheet_svg(p, d, sheet, &page, vp, level_id, kind, &date) {
            svg.s.push_str(&content);
            scale_cell = cell;
            drew_service = true;
        }
    }
    if !drew_service {
        let (placed, _) = layout(p, d, sheet, &page, i, &printed.parts);
        scale_cell = block_scale(&placed);
        // Everything in the drawing area only: never over the title block.
        svg.s.push_str(&format!(
            "<defs><clipPath id=\"area-clip\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath></defs>\n<g id=\"viewports\" clip-path=\"url(#area-clip)\">\n",
            num(ax),
            num(ay),
            num(aw),
            num(ah)
        ));
        for pl in placed.iter().filter(|pl| !pl.out) {
            write_viewport(&mut svg.s, p, pl, &sheet.number, k);
        }
        svg.s.push_str("</g>\n");
    }
    // The border.
    svg.rect(page.margin, page.margin, page.width - 2.0 * page.margin, page.height - 2.0 * page.margin, 0.5, "none");

    let tb = if i.mask_signatory { p.title_block.masked() } else { (*p.title_block).clone() };
    let revisions: Vec<_> = p.sheet_set.revisions.iter().filter(|r| sheet.revision_ids.contains(&r.id)).collect();
    let info = SheetInfo {
        number: &sheet.number,
        title: &sheet.title,
        index,
        total,
        scale: scale_cell,
        date,
        paper: sheet.paper,
        orientation: sheet.orientation,
        revisions,
        discipline: discipline(sheet),
    };
    let logo_data = tb.firm_logo.as_ref().and_then(|f| i.images.get(f)).and_then(|b| image_data(b));
    let logo = logo_data.as_ref().map(|(mime, b64)| Logo { mime, base64: b64 });
    titleblock::draw(&mut svg, p, &tb, &info, page.title_block, k, logo);
    svg.s.push_str("</svg>\n");
    Ok(svg.s)
}

// ------------------------------------------------------- continuations

/// The rows `from..to` of a schedule a printed sheet shows, and the sheet
/// number the rest continues on (None for the last part).
#[derive(Debug, Clone, PartialEq)]
pub struct SchedulePart {
    pub from: usize,
    pub to: usize,
    pub continued_on: Option<String>,
}

/// A sheet as printed: a sheet of the set, or a continuation of it with
/// what did not fit. `parts` gives each viewport's schedule rows.
#[derive(Debug, Clone)]
struct Printed {
    /// The set's sheet it comes from.
    source: Id,
    sheet: Sheet,
    parts: Vec<Option<SchedulePart>>,
}

/// The number of a sheet's `k`th printed page: "A-7", then "A-7.1".
fn page_number(number: &str, k: usize) -> String {
    if k == 0 {
        number.to_string()
    } else {
        format!("{}.{k}", clean(number))
    }
}

/// A sheet split into the sheets it prints on. Only a sheet with a
/// schedule continues: when everything does not fit, its viewports go in
/// order onto as few sheets as they fit, a long table split by rows with
/// its header on each part and its total on the last, so nothing overlaps
/// or is clipped. "N OF M" counts the continuations. A sheet that fits, or
/// whose drawing is too big even alone, prints as it is.
fn paginate(p: &Project, d: &Derived, sheet: &Sheet, i: &SheetInputs) -> Vec<Printed> {
    let whole = Printed { source: sheet.id.clone(), sheet: sheet.clone(), parts: vec![None; sheet.viewports.len()] };
    let schedules = sheet.viewports.iter().any(|vp| matches!(vp.view, ViewRef::Schedule { .. }));
    if !schedules || service_kind(sheet).is_some() {
        return vec![whole];
    }
    let page = Page::new(sheet.paper, sheet.orientation);
    let fits = |pr: &Printed| layout(p, d, &pr.sheet, &page, i, &pr.parts).1;
    if fits(&whole) {
        return vec![whole];
    }
    let blank = |k: usize| {
        let mut sh = sheet.clone();
        sh.viewports.clear();
        if k > 0 {
            sh.number = page_number(&sheet.number, k);
            let title = clean(&sheet.title);
            sh.title = if title.is_empty() { "Continued".into() } else { format!("{title} (continued)") };
        }
        Printed { source: sheet.id.clone(), sheet: sh, parts: vec![] }
    };
    let with = |pr: &Printed, vp: &Viewport, part: Option<SchedulePart>| {
        let mut next = pr.clone();
        let mut vp = vp.clone();
        // A placed viewport keeps its place only on the sheet it was placed on.
        if next.sheet.number != sheet.number {
            vp.position = None;
        }
        if part.as_ref().is_some_and(|pt| pt.from > 0) {
            vp.title = Some(format!("{} (continued)", title_of(p, &vp)));
        }
        next.sheet.viewports.push(vp);
        next.parts.push(part);
        next
    };
    let mut done: Vec<Printed> = Vec::new();
    let mut cur = blank(0);
    for vp in &sheet.viewports {
        let rows = match &vp.view {
            ViewRef::Schedule { schedule } => tables::schedule(p, d, *schedule).rows.len(),
            _ => 0,
        };
        if rows == 0 {
            let next = with(&cur, vp, None);
            if fits(&next) || cur.sheet.viewports.is_empty() {
                cur = next;
            } else {
                done.push(std::mem::replace(&mut cur, blank(done.len() + 1)));
                cur = with(&cur, vp, None);
            }
            continue;
        }
        let mut from = 0;
        while from < rows {
            let k = done.len();
            let part = |to: usize| SchedulePart { from, to, continued_on: (to < rows).then(|| page_number(&sheet.number, k + 1)) };
            // The most rows that still fit here: more rows only grow it.
            let (mut lo, mut hi) = (from, rows);
            while lo < hi {
                let mid = (lo + hi).div_ceil(2);
                if fits(&with(&cur, vp, Some(part(mid)))) {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
            if lo > from {
                cur = with(&cur, vp, Some(part(lo)));
                from = lo;
            } else if cur.sheet.viewports.is_empty() {
                // Not one row fits a sheet of its own (a table wider than
                // the paper): the rest prints here, clipped as before.
                cur = with(&cur, vp, Some(part(rows)));
                from = rows;
            }
            if from < rows {
                done.push(std::mem::replace(&mut cur, blank(k + 1)));
            }
        }
    }
    if !cur.sheet.viewports.is_empty() || done.is_empty() {
        done.push(cur);
    }
    done
}

/// The set as printed: the project's sheets, or the default set when it
/// has none, each with its continuations.
fn printed_set(p: &Project, d: &Derived, i: &SheetInputs) -> Vec<Printed> {
    let mut set: Vec<Sheet> = if p.sheet_set.sheets.is_empty() { default_set(p, p.settings.paper) } else { p.sheet_set.sheets.clone() };
    one_plan_scale(p, d, &mut set, i);
    set.iter().flat_map(|s| paginate(p, d, s, i)).collect()
}

/// Every floor plan of a set without a scale of its own prints at one
/// scale per paper size: the smallest that any of their sheets picks, so a
/// smaller upper floor is not drawn larger than the ground floor (GA P1-4).
fn one_plan_scale(p: &Project, d: &Derived, set: &mut [Sheet], i: &SheetInputs) {
    let floor_plan = |vp: &Viewport| vp.scale_denominator.filter(|n| *n > 0).is_none() && matches!(vp.view, ViewRef::Plan { sheet: SheetKind::Plan, .. });
    let mut scales: Vec<(PaperSize, u32)> = Vec::new();
    for sheet in set.iter().filter(|s| s.viewports.iter().any(floor_plan)) {
        let page = Page::new(sheet.paper, sheet.orientation);
        let (placed, _) = layout(p, d, sheet, &page, i, &[]);
        scales.extend(placed.iter().filter(|pl| !pl.out && floor_plan(pl.vp)).map(|pl| (sheet.paper, pl.scale)));
    }
    for sheet in set.iter_mut() {
        let of_paper = scales.iter().filter(|(paper, _)| *paper == sheet.paper).map(|(_, n)| *n);
        let (Some(n), Some(lo)) = (of_paper.clone().max(), of_paper.min()) else { continue };
        if n == lo {
            continue;
        }
        for vp in sheet.viewports.iter_mut().filter(|vp| floor_plan(vp)) {
            vp.scale_denominator = Some(n);
        }
    }
}

/// The sheets to draw: `ids` in set order (empty: all), each with its
/// continuations, its place in the printed set and the set's size. A
/// project with no sheets gets the default set (`default_set`), so an
/// export always has a set to print.
fn chosen(p: &Project, d: &Derived, ids: &[Id], i: &SheetInputs) -> Result<(Vec<(usize, Printed)>, usize), ExportError> {
    let set: Vec<Sheet> = if p.sheet_set.sheets.is_empty() { default_set(p, p.settings.paper) } else { p.sheet_set.sheets.clone() };
    for id in ids {
        if !set.iter().any(|s| &s.id == id) {
            return Err(ExportError::Failed(format!("sheet {id} does not exist")));
        }
    }
    let printed = printed_set(p, d, i);
    let total = printed.len();
    let list: Vec<(usize, Printed)> = printed
        .into_iter()
        .enumerate()
        .filter(|(_, s)| ids.is_empty() || ids.contains(&s.source))
        .map(|(j, s)| (j + 1, s))
        .collect();
    if list.is_empty() {
        return Err(ExportError::Empty("the project has no sheets and no levels to draw".into()));
    }
    Ok((list, total))
}

/// One sheet as SVG at true paper size, by `Sheet::id`: its first printed
/// sheet when a long schedule continues on more.
pub fn sheet_svg(p: &Project, d: &Derived, sheet_id: &str, i: &SheetInputs) -> Result<String, ExportError> {
    let (list, total) = chosen(p, d, &[sheet_id.to_string()], i)?;
    let (index, sheet) = &list[0];
    draw_sheet(p, d, sheet, *index, total, i, false)
}

/// The Sheets panel's picture of one sheet: as `sheet_svg`, and a viewport
/// with nothing to print shows a dashed frame that says why, the vicinity
/// frame how to place its image. Never for a file or an agent's picture.
pub fn sheet_preview_svg(p: &Project, d: &Derived, sheet_id: &str, i: &SheetInputs) -> Result<String, ExportError> {
    let (list, total) = chosen(p, d, &[sheet_id.to_string()], i)?;
    let (index, sheet) = &list[0];
    draw_sheet(p, d, sheet, *index, total, i, true)
}

/// The sheets `ids` (empty: all, in set order) as SVG, one per printed
/// sheet, continuations included, with each sheet's number.
pub fn sheets_svg(p: &Project, d: &Derived, ids: &[Id], i: &SheetInputs) -> Result<Vec<(String, String)>, ExportError> {
    let (list, total) = chosen(p, d, ids, i)?;
    list.iter()
        .map(|(index, sheet)| Ok((sheet.sheet.number.clone(), draw_sheet(p, d, sheet, *index, total, i, false)?)))
        .collect()
}

/// The sheets `ids` (empty: all, in set order) as one PDF.
pub fn sheets_pdf(p: &Project, d: &Derived, ids: &[Id], i: &SheetInputs) -> Result<Vec<u8>, ExportError> {
    let pages: Vec<String> = sheets_svg(p, d, ids, i)?.into_iter().map(|(_, s)| s).collect();
    crate::pdf::svgs_to_pdf(&pages)
}

/// How a printed viewport is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintedScale {
    /// At 1:n.
    Scaled(u32),
    /// An image or diagram: "NOT TO SCALE".
    NotToScale,
    /// A table or notes: no scale line.
    None,
}

/// One viewport of a printed sheet as the PDF draws it.
#[derive(Debug, Clone)]
pub struct PrintedViewport {
    pub viewport: Viewport,
    pub title: String,
    pub scale: PrintedScale,
    /// The schedule rows it prints; None: all of them.
    pub part: Option<SchedulePart>,
    /// Content and title, and the content alone, paper mm, SVG y down.
    pub frame: Rect,
    pub content: Rect,
}

/// One sheet of the printed set.
#[derive(Debug, Clone)]
pub struct PrintedSheet {
    /// The set's sheet it comes from.
    pub source: Id,
    pub number: String,
    pub title: String,
    pub paper: PaperSize,
    pub orientation: Orientation,
    /// 1-based place in the printed set, and the set's size.
    pub index: usize,
    pub total: usize,
    /// The title block's SCALE cell.
    pub scale: String,
    /// The title block DATE.
    pub date: String,
    /// The drawing area, paper mm, SVG y down.
    pub area: Rect,
    pub viewports: Vec<PrintedViewport>,
    /// The viewports a printed sheet leaves out, each with why: "SECTION
    /// C-C: nothing to draw: the mark looks away from the model." For the
    /// export's result, never printed on the sheet.
    pub left_out: Vec<String>,
    /// False when what it holds does not fit its drawing area.
    pub fits: bool,
}

/// The sheets `ids` (empty: all) as printed: continuations, numbers, "N OF
/// M", and each viewport's scale and place as the PDF lays it out. The
/// sheets DXF draws from this, so both say the same.
pub fn printed_sheets(p: &Project, d: &Derived, ids: &[Id], i: &SheetInputs) -> Result<Vec<PrintedSheet>, ExportError> {
    let (list, total) = chosen(p, d, ids, i)?;
    Ok(list
        .iter()
        .map(|(index, pr)| {
            let page = Page::new(pr.sheet.paper, pr.sheet.orientation);
            let (placed, fits) = layout(p, d, &pr.sheet, &page, i, &pr.parts);
            let service = service_kind(&pr.sheet).map(|(vp, level_id, kind)| service_sheet_svg(p, d, &pr.sheet, &page, vp, level_id, kind, &sheet_date(p, i)));
            let left_out: Vec<String> = match &service {
                Some(Ok(_)) => vec![],
                Some(Err(e)) => vec![format!("{}: {}", clean(&pr.sheet.title).to_uppercase(), sentence(&reason_of(e)))],
                None => placed
                    .iter()
                    .filter(|pl| pl.out)
                    .map(|pl| match &pl.body {
                        Body::Missing { reason, .. } => format!("{}: {}", pl.title, reason),
                        _ => pl.title.clone(),
                    })
                    .collect(),
            };
            let viewports = placed
                .iter()
                .enumerate()
                .filter(|(_, pl)| !pl.out)
                .map(|(j, pl)| PrintedViewport {
                    viewport: pl.vp.clone(),
                    title: pl.title.clone(),
                    scale: match pl.scaling {
                        Scaling::Scaled => PrintedScale::Scaled(pl.scale),
                        Scaling::NotToScale => PrintedScale::NotToScale,
                        Scaling::None => PrintedScale::None,
                    },
                    part: pr.parts.get(j).cloned().flatten(),
                    frame: pl.frame,
                    content: pl.content,
                })
                .collect();
            PrintedSheet {
                source: pr.source.clone(),
                number: pr.sheet.number.clone(),
                title: pr.sheet.title.clone(),
                paper: pr.sheet.paper,
                orientation: pr.sheet.orientation,
                index: *index,
                total,
                scale: block_scale(&placed),
                date: sheet_date(p, i),
                area: page.area,
                viewports,
                left_out,
                fits,
            }
        })
        .collect())
}

/// One view alone, framed with its title, at 1:`scale` (None: the largest
/// common scale at which it fits an A1 sheet's drawing area): the views
/// viewer, and every view lane L3 has not built a picture of its own for.
pub fn view_alone(p: &Project, d: &Derived, view: &ViewRef, scale: Option<u32>, i: &SheetInputs) -> Result<PlanOutput<String>, ExportError> {
    let sheet = Sheet {
        id: "view".into(),
        number: String::new(),
        title: String::new(),
        paper: PaperSize::A1,
        orientation: Orientation::Landscape,
        viewports: vec![Viewport { id: "view".into(), view: view.clone(), scale_denominator: scale, position: None, title: None }],
        revision_ids: vec![],
    };
    let page = Page::new(sheet.paper, sheet.orientation);
    let k = 1.0;
    let (mut placed, _) = layout(p, d, &sheet, &Page { k, ..page }, i, &[]);
    let mut pl = placed.remove(0);
    // Crop to the viewport with a margin.
    let pad = 8.0;
    let (fx, fy, fw, fh) = pl.frame;
    let dx = pad - fx;
    let dy = pad - fy;
    pl.frame = (pad, pad, fw, fh);
    pl.content = (pl.content.0 + dx, pl.content.1 + dy, pl.content.2, pl.content.3);
    let (w, h) = (fw + 2.0 * pad, fh + 2.0 * pad);
    let mut s = svg_head(w, h, &format!("{} - {}", clean(&p.name), pl.title));
    write_viewport(&mut s, p, &pl, "", k);
    s.push_str("</svg>\n");
    Ok(PlanOutput { data: s, scale_denominator: pl.scale })
}

// ---------------------------------------------------------- default set

/// The set an export prints when the project has no sheets yet: a site
/// sheet when there is a lot, one plan per level with a building,
/// elevations and sections two to a sheet, the roof plan, and the
/// schedules. Never stored; `MakeSheetSet` (lane L1) makes the real one.
pub fn default_set(p: &Project, paper: PaperSize) -> Vec<Sheet> {
    let mut sheets: Vec<Sheet> = Vec::new();
    let mut add = |title: String, views: Vec<ViewRef>| {
        let n = sheets.len() + 1;
        sheets.push(Sheet {
            id: format!("auto-sheet-{n}"),
            number: format!("A-{n}"),
            title,
            paper,
            orientation: Orientation::Landscape,
            viewports: views
                .into_iter()
                .enumerate()
                .map(|(j, view)| Viewport { id: format!("auto-vp-{n}-{}", j + 1), view, scale_denominator: None, position: None, title: None })
                .collect(),
            revision_ids: vec![],
        });
    };
    if p.elements.iter().any(|e| matches!(e, Element::Lot(_))) {
        add("Site development plan and vicinity map".into(), vec![ViewRef::SitePlan, ViewRef::VicinityMap { file_name: None, radius_m: 500.0 }]);
    }
    for l in &p.levels {
        let has = p.elements.iter().any(|e| e.level_id() == Some(&l.id) && matches!(e, Element::Wall(_)));
        if has {
            add(sentence_case(&drawing_title(&l.name)), vec![ViewRef::Plan { level_id: l.id.clone(), sheet: SheetKind::Plan }]);
        }
    }
    let marks = |kind: MarkKind| -> Vec<Id> {
        p.elements
            .iter()
            .filter_map(|e| match e {
                Element::ViewMark(m) if m.mark_kind == kind => Some(m.id.clone()),
                _ => None,
            })
            .collect()
    };
    for pair in marks(MarkKind::Elevation).chunks(2) {
        add("Elevations".into(), pair.iter().map(|id| ViewRef::Elevation { mark_id: id.clone() }).collect());
    }
    for pair in marks(MarkKind::Section).chunks(2) {
        add("Sections".into(), pair.iter().map(|id| ViewRef::Section { mark_id: id.clone() }).collect());
    }
    if p.elements.iter().any(|e| matches!(e, Element::Roof(_))) {
        add("Roof plan".into(), vec![ViewRef::RoofPlan]);
    }
    if !p.levels.is_empty() {
        add(
            "Schedules".into(),
            vec![
                ViewRef::Schedule { schedule: ScheduleKind::DoorsWindows },
                ViewRef::Schedule { schedule: ScheduleKind::Areas },
                ViewRef::Schedule { schedule: ScheduleKind::Rooms },
                ViewRef::Schedule { schedule: ScheduleKind::Finishes },
            ],
        );
    }
    sheets
}

/// "GROUND FLOOR PLAN" as "Ground floor plan".
fn sentence_case(t: &str) -> String {
    let lower = t.to_lowercase();
    let mut cs = lower.chars();
    match cs.next() {
        Some(f) => f.to_uppercase().chain(cs).collect(),
        None => String::new(),
    }
}

/// Every image a set needs, by the key `SheetInputs::images` uses: the firm
/// logo and vicinity maps by file name, perspectives by render id. The app
/// reads these and fills `images`.
pub fn image_keys(p: &Project) -> (Vec<String>, Vec<Id>) {
    let mut files: Vec<String> = Vec::new();
    let mut renders: Vec<Id> = Vec::new();
    if let Some(logo) = &p.title_block.firm_logo {
        files.push(logo.clone());
    }
    for vp in p.sheet_set.sheets.iter().flat_map(|s| s.viewports.iter()) {
        match &vp.view {
            ViewRef::VicinityMap { file_name: Some(f), .. } if !files.contains(f) => files.push(f.clone()),
            ViewRef::Perspective { render_id } if !renders.contains(render_id) => renders.push(render_id.clone()),
            _ => {}
        }
    }
    (files, renders)
}

/// Images keyed for `SheetInputs`.
pub type Images = BTreeMap<String, Vec<u8>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn png_and_jpeg_aspects() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R'];
        png.extend_from_slice(&1920u32.to_be_bytes());
        png.extend_from_slice(&1080u32.to_be_bytes());
        assert!((image_aspect(&png).unwrap() - 16.0 / 9.0).abs() < 1e-9);
        // SOI, an APP0 of 4 bytes, then SOF0 with 600 x 800.
        let jpeg = [0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0, 0, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x02, 0x58, 0x03, 0x20, 0x03];
        assert!((image_aspect(&jpeg).unwrap() - 800.0 / 600.0).abs() < 1e-9);
        assert_eq!(image_aspect(b"GIF89a"), None);
    }

    #[test]
    fn the_title_block_is_outside_the_drawing_area_on_every_paper() {
        for paper in [PaperSize::A0, PaperSize::A1, PaperSize::A2, PaperSize::A3] {
            for o in [Orientation::Landscape, Orientation::Portrait] {
                let page = Page::new(paper, o);
                assert!(!overlaps(page.area, page.title_block), "{paper:?} {o:?}");
                assert!(page.area.2 > 100.0 && page.area.3 > 100.0, "{paper:?} {o:?}: {:?}", page.area);
            }
        }
    }

    #[test]
    fn packing_keeps_order_and_never_overlaps() {
        let area = (0.0, 0.0, 330.0, 200.0);
        let frames = pack(area, &[], &[(100.0, 80.0), (100.0, 80.0), (100.0, 80.0), (120.0, 60.0)], 5.0).unwrap();
        for (a, r) in frames.iter().enumerate() {
            assert!(inside(*r, area));
            for b in frames.iter().skip(a + 1) {
                assert!(!overlaps(*r, *b));
            }
        }
        assert!(frames[0].0 < frames[1].0 && frames[1].0 < frames[2].0);
        assert!(pack(area, &[], &[(400.0, 10.0)], 5.0).is_none());
        // Around a placed frame.
        let fixed = [(0.0, 0.0, 150.0, 150.0)];
        let f = pack(area, &fixed, &[(100.0, 100.0)], 5.0).unwrap();
        assert!(!overlaps(f[0], fixed[0]));
    }
}
