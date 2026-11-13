//! Drawings computed on demand (DECISIONS D54): elevations and sections
//! with hidden lines removed, the roof plan, the site plan, the reflected
//! ceiling plan and the vicinity map frame. Never stored and never in
//! `Derived`. LANE L3 owns this folder.
//!
//! `items` is the L3 to L4 seam: one view's plan items in drawing mm,
//! framed on a sheet by L4 (`sheetset`) and written to DXF by L8. Plans are
//! model mm, y north; elevations and sections are x across the picture
//! plane and y the height above project zero.
//!
//! Hidden line results and meshes are cached by the project's content (a
//! hash of its JSON, which changes with every revision), so a sheet set
//! that draws the same elevation twice, or a views viewer that redraws at
//! another scale, computes it once. The cache only remembers results; the
//! functions stay pure.

pub mod annot;
pub mod hlr;
pub mod mesh;
pub mod planmarks;
pub mod rcp;
pub mod roofplan;
pub mod section;
pub mod siteplan;
pub mod vicinity;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

use base64_lite::encode as b64;
use guhit_model::{Derived, Element, Id, MarkKind, Project, SheetKind, ViewMark, ViewRef};

use crate::geom::*;
use crate::plan::{self, items_bounds, Cat, HAlign, Item, Pen, PlanOptions, Prim};
use crate::sheet::{self, num, FONT_FAMILY, INK};
use crate::text::{clean, xml_escape};
use crate::{ExportError, PlanOutput, SheetInputs};

/// One view as drawing primitives in drawing mm, with their extent.
#[derive(Debug, Clone)]
pub struct ViewItems {
    pub items: Vec<Item>,
    pub bounds: Bounds,
    /// The view's own title: "NORTH ELEVATION", "SECTION A-A", "ROOF PLAN".
    pub title: String,
    /// The 1:N the annotation sizes were made for.
    pub scale_denominator: u32,
    /// Images to place under the items (the vicinity map), by file name in
    /// `SheetInputs::images`, over the drawing rectangle `min`..`max`.
    pub images: Vec<ViewImage>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ViewImage {
    pub file_name: String,
    pub min: V,
    pub max: V,
}

/// The scale a view is drawn for: the one asked, else the project's, else
/// 1:100.
pub fn scale_of(p: &Project, scale: Option<u32>) -> u32 {
    scale.filter(|n| *n > 0).or(Some(p.settings.scale_denominator).filter(|n| *n > 0)).unwrap_or(100)
}

/// The items of one view at 1:`scale` (None: the project scale), hidden
/// lines removed.
///
/// Plans, elevations, sections, the roof, site and ceiling plans, the
/// vicinity frame and notes are drawings. A schedule is a table and a
/// perspective an image: they answer `Failed`, and the sheet lays them out
/// itself (`tables`, the render's file).
pub fn items(p: &Project, d: &Derived, view: &ViewRef, scale: Option<u32>) -> Result<ViewItems, ExportError> {
    let n = scale_of(p, scale) as f64;
    match view {
        ViewRef::Plan { level_id, sheet } => plan_view(p, d, level_id, *sheet, n),
        ViewRef::Elevation { mark_id } => mark_view(p, d, mark_id, MarkKind::Elevation, n),
        ViewRef::Section { mark_id } => mark_view(p, d, mark_id, MarkKind::Section, n),
        ViewRef::RoofPlan => roofplan::items(p, d, n),
        ViewRef::SitePlan => siteplan::items(p, d, n),
        ViewRef::VicinityMap { file_name, radius_m } => Ok(vicinity::items(p, file_name.as_deref(), *radius_m, n)),
        ViewRef::Notes { text } => notes(text, n),
        ViewRef::Schedule { .. } => Err(ExportError::Failed("a schedule is a table, not a drawing (guhit_export::tables)".into())),
        ViewRef::Perspective { .. } => Err(ExportError::Failed("a perspective is a model render, not a drawing".into())),
    }
}

/// The title a viewport shows when it has none of its own.
pub fn default_title(p: &Project, view: &ViewRef) -> String {
    match view {
        ViewRef::Plan { level_id, sheet } => {
            let name = p.levels.iter().find(|l| &l.id == level_id).map(|l| l.name.clone()).unwrap_or_default();
            match sheet {
                SheetKind::Plan => crate::sheet::drawing_title(&name),
                SheetKind::ReflectedCeiling => {
                    let n = clean(&name).to_uppercase();
                    if n.is_empty() { "REFLECTED CEILING PLAN".into() } else { format!("{n} REFLECTED CEILING PLAN") }
                }
                other => crate::service_sheet::sheet_title(*other, &name),
            }
        }
        ViewRef::Elevation { mark_id } | ViewRef::Section { mark_id } => {
            find_mark(p, mark_id).map(|m| section::title(m)).unwrap_or_else(|| "VIEW".into())
        }
        ViewRef::RoofPlan => "ROOF PLAN".into(),
        ViewRef::SitePlan => "SITE PLAN".into(),
        ViewRef::VicinityMap { .. } => "VICINITY MAP".into(),
        ViewRef::Perspective { .. } => "PERSPECTIVE".into(),
        ViewRef::Schedule { schedule } => match schedule {
            guhit_model::ScheduleKind::DoorsWindows => "DOOR AND WINDOW SCHEDULE".into(),
            guhit_model::ScheduleKind::Rooms => "ROOM SCHEDULE".into(),
            guhit_model::ScheduleKind::Finishes => "FINISH SCHEDULE".into(),
            guhit_model::ScheduleKind::Areas => "FLOOR AREAS".into(),
        },
        ViewRef::Notes { .. } => "NOTES".into(),
    }
}

fn find_mark<'a>(p: &'a Project, id: &str) -> Option<&'a ViewMark> {
    p.elements.iter().find_map(|e| match e {
        Element::ViewMark(m) if m.id == id => Some(m),
        _ => None,
    })
}

fn plan_view(p: &Project, d: &Derived, level_id: &Id, sheet: SheetKind, n: f64) -> Result<ViewItems, ExportError> {
    let level = plan::resolve_level(p, Some(level_id))?;
    if sheet == SheetKind::ReflectedCeiling {
        return rcp::items(p, d, level, n);
    }
    if sheet == SheetKind::PlumbingIsometric {
        return Err(ExportError::Failed("the plumbing isometric is a diagram sheet of its own, not a plan view".into()));
    }
    let services = sheet != SheetKind::Plan;
    let mut items = plan::build_items(
        p,
        d,
        level,
        &PlanOptions {
            scale: n,
            show_dimensions: !services,
            show_room_labels: true,
            show_assets: !services,
            unicode: true,
            skip_devices: services,
        },
    );
    if services {
        // The layout's objects as their symbols, over the architecture.
        for dev in crate::services::devices_on(p, level, sheet, n) {
            for m in &dev.placed.marks {
                items.push(Item { cat: Cat::Furn, pen: m.pen, prim: m.prim.clone() });
            }
            for l in &dev.placed.labels {
                items.push(Item { cat: Cat::Furn, pen: Pen::Light, prim: l.clone() });
            }
        }
        if matches!(sheet, SheetKind::Plumbing | SheetKind::Aircon) {
            for pipe in crate::pipes::plan_pipes(p, level) {
                let wanted = match sheet {
                    SheetKind::Plumbing => !matches!(pipe.system, guhit_model::PipeSystem::Refrigerant | guhit_model::PipeSystem::Condensate | guhit_model::PipeSystem::Conduit),
                    _ => matches!(pipe.system, guhit_model::PipeSystem::Refrigerant | guhit_model::PipeSystem::Condensate),
                };
                if !wanted {
                    continue;
                }
                for run in &pipe.runs {
                    if run.len() >= 2 {
                        items.push(Item { cat: Cat::Furn, pen: Pen::Medium, prim: Prim::Poly { pts: run.clone(), closed: false, fill: crate::plan::Fill::None } });
                    }
                }
            }
        }
    }
    if items.is_empty() {
        return Err(plan::empty_level(level));
    }
    let bounds = items_bounds(&items);
    Ok(ViewItems { items, bounds, title: default_title(p, &ViewRef::Plan { level_id: level_id.clone(), sheet }), scale_denominator: n as u32, images: Vec::new() })
}

fn mark_view(p: &Project, d: &Derived, mark_id: &Id, kind: MarkKind, n: f64) -> Result<ViewItems, ExportError> {
    let mark = find_mark(p, mark_id).ok_or_else(|| ExportError::Failed(format!("view mark {mark_id} does not exist")))?;
    if mark.mark_kind != kind {
        let what = match mark.mark_kind {
            MarkKind::Section => "a section",
            MarkKind::Elevation => "an elevation",
        };
        return Err(ExportError::Failed(format!("view mark \"{}\" is {what} mark", clean(&mark.label))));
    }
    let (mesh, h) = mark_hlr(p, d, mark)?;
    section::draw(p, d, mark, &mesh, &h, n)
}

/// The mesh and hidden line result of a mark, from the cache when the
/// project has not changed.
pub fn mark_hlr(p: &Project, d: &Derived, mark: &ViewMark) -> Result<(Arc<mesh::Mesh>, Arc<hlr::Hlr>), ExportError> {
    let (frame, opts, _) = section::frame_of(mark)?;
    let fp = fingerprint(p);
    let mesh = cached_mesh(fp, || mesh::build(p, d));
    let mut hasher = DefaultHasher::new();
    fp.hash(&mut hasher);
    for x in [mark.start.x, mark.start.y, mark.end.x, mark.end.y, mark.depth_mm.unwrap_or(-1.0)] {
        x.to_bits().hash(&mut hasher);
    }
    mark.flip.hash(&mut hasher);
    let key = hasher.finish();
    let h = cached_hlr(key, || {
        let extra = section::figure_pieces(&mesh, &frame);
        hlr::hidden_lines(&mesh, &frame, &opts, &extra)
    });
    Ok((mesh, h))
}

fn notes(text: &str, n: f64) -> Result<ViewItems, ExportError> {
    let th = 2.5 * n;
    let mut items = Vec::new();
    for (i, l) in text.lines().enumerate() {
        let t = clean(l);
        if t.is_empty() {
            continue;
        }
        items.push(Item {
            cat: Cat::Text,
            pen: Pen::Light,
            prim: Prim::Text { pos: v(0.0, -(i as f64) * th * 1.5), height: th, rot_deg: 0.0, align: HAlign::Start, text: t, bold: false },
        });
    }
    if items.is_empty() {
        return Err(ExportError::Empty("the notes are empty".into()));
    }
    let bounds = items_bounds(&items);
    Ok(ViewItems { items, bounds, title: "NOTES".into(), scale_denominator: n as u32, images: Vec::new() })
}

// ---------------------------------------------------------------- cache

/// A hash of the project's JSON: equal for equal content.
pub fn fingerprint(p: &Project) -> u64 {
    let mut h = DefaultHasher::new();
    match serde_json::to_vec(p) {
        Ok(bytes) => bytes.hash(&mut h),
        Err(_) => p.id.hash(&mut h),
    }
    h.finish()
}

struct Lru<T> {
    cap: usize,
    list: Vec<(u64, Arc<T>)>,
}

impl<T> Lru<T> {
    fn get_or(&mut self, key: u64, make: impl FnOnce() -> T) -> Arc<T> {
        if let Some(i) = self.list.iter().position(|(k, _)| *k == key) {
            let hit = self.list.remove(i);
            let v = hit.1.clone();
            self.list.push(hit);
            return v;
        }
        let v = Arc::new(make());
        self.list.push((key, v.clone()));
        if self.list.len() > self.cap {
            self.list.remove(0);
        }
        v
    }
}

fn cached_mesh(key: u64, make: impl FnOnce() -> mesh::Mesh) -> Arc<mesh::Mesh> {
    static CACHE: OnceLock<Mutex<Lru<mesh::Mesh>>> = OnceLock::new();
    let m = CACHE.get_or_init(|| Mutex::new(Lru { cap: 4, list: Vec::new() }));
    // Computed outside the lock would duplicate work under contention;
    // views are drawn one at a time in practice, so hold it.
    let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or(key, make)
}

fn cached_hlr(key: u64, make: impl FnOnce() -> hlr::Hlr) -> Arc<hlr::Hlr> {
    static CACHE: OnceLock<Mutex<Lru<hlr::Hlr>>> = OnceLock::new();
    let m = CACHE.get_or_init(|| Mutex::new(Lru { cap: 32, list: Vec::new() }));
    let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or(key, make)
}

// ------------------------------------------------------------------ svg

/// Paper around a lone view, mm.
const MARGIN: f64 = 10.0;
const TITLE_BAND: f64 = 16.0;

/// One view alone as SVG, for `view_svg` and the views viewer: the
/// drawing at 1:`scale` (None: the largest common scale at which it fits
/// the project's paper), its title and scale under it.
pub fn view_svg(p: &Project, d: &Derived, view: &ViewRef, scale: Option<u32>, i: &SheetInputs) -> Result<PlanOutput<String>, ExportError> {
    let n = match scale.filter(|n| *n > 0) {
        Some(n) => n,
        None => {
            let layout = sheet::Layout::new(p.settings.paper, guhit_model::Orientation::Landscape, false);
            let (_, _, pw, ph) = layout.plan;
            let mut failed: Option<ExportError> = None;
            let n = sheet::pick_scale(pw, ph, |n| match items(p, d, view, Some(n)) {
                Ok(v) => (v.bounds.width(), v.bounds.height()),
                Err(e) => {
                    failed = Some(e);
                    (0.0, 0.0)
                }
            });
            if let Some(e) = failed {
                return Err(e);
            }
            n
        }
    };
    let v_items = items(p, d, view, Some(n))?;
    Ok(PlanOutput { data: svg_of(&v_items, i), scale_denominator: n })
}

/// The SVG of drawn view items, one user unit a paper mm.
pub fn svg_of(v_items: &ViewItems, i: &SheetInputs) -> String {
    let n = v_items.scale_denominator.max(1) as f64;
    let b = v_items.bounds;
    let (bw, bh) = (b.width() / n, b.height() / n);
    let w = bw + 2.0 * MARGIN;
    let h = bh + 2.0 * MARGIN + TITLE_BAND;
    let to_paper = |q: V| -> (f64, f64) { (MARGIN + (q.x - b.min.x) / n, MARGIN + (b.max.y - q.y) / n) };
    let mut s = String::new();
    s.push_str(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\" font-family=\"{FONT_FAMILY}\">\n",
        num(w),
        num(h),
        num(w),
        num(h)
    ));
    s.push_str(&format!("<title>{}</title>\n", xml_escape(&format!("{} - 1:{}", v_items.title, n as u32))));
    s.push_str(&format!("<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>\n", num(w), num(h)));
    for img in &v_items.images {
        if let Some(bytes) = i.images.get(&img.file_name) {
            let (x0, y0) = to_paper(v(img.min.x, img.max.y));
            let (x1, y1) = to_paper(v(img.max.x, img.min.y));
            s.push_str(&format!(
                "<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"xMidYMid slice\" xlink:href=\"data:{};base64,{}\"/>\n",
                num(x0),
                num(y0),
                num(x1 - x0),
                num(y1 - y0),
                mime_of(bytes),
                b64(bytes)
            ));
        }
    }
    write_items(&mut s, &v_items.items, n, &to_paper);
    // Title and scale under the drawing.
    let ty = MARGIN + bh + 9.0;
    s.push_str(&format!(
        "<text x=\"{}\" y=\"{}\" font-size=\"4.2\" font-weight=\"bold\" fill=\"{INK}\">{}</text>\n",
        num(MARGIN),
        num(ty),
        xml_escape(&v_items.title)
    ));
    s.push_str(&format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{INK}\" stroke-width=\"0.35\"/>\n",
        num(MARGIN),
        num(ty + 1.6),
        num(MARGIN + crate::text::est_width(&v_items.title, 4.2, true) + 2.0),
        num(ty + 1.6)
    ));
    s.push_str(&format!(
        "<text x=\"{}\" y=\"{}\" font-size=\"2.8\" fill=\"{INK}\">SCALE 1:{}</text>\n",
        num(MARGIN),
        num(ty + 5.6),
        n as u32
    ));
    s.push_str("</svg>\n");
    s
}

/// Items in draw order (`Cat::ALL`), room labels over white masks, inked
/// as the plan sheet inks them. For L4's sheets too.
pub fn write_items(s: &mut String, items: &[Item], n: f64, to_paper: &dyn Fn(V) -> (f64, f64)) {
    s.push_str(&format!("<g stroke-linecap=\"butt\" stroke-linejoin=\"miter\" fill=\"none\" stroke=\"{INK}\">\n"));
    for cat in Cat::ALL {
        let group: Vec<&Item> = items.iter().filter(|i| i.cat == cat).collect();
        if group.is_empty() {
            continue;
        }
        s.push_str(&format!("<g id=\"{}\">\n", cat.svg_id()));
        if cat == Cat::Area {
            sheet::label_masks(s, &group, n, to_paper);
        }
        for item in group {
            sheet::write_item(s, item, n, to_paper);
        }
        s.push_str("</g>\n");
    }
    s.push_str("</g>\n");
}

fn mime_of(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        "image/jpeg"
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

/// Base64 without a dependency.
mod base64_lite {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(data: &[u8]) -> String {
        let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
        for c in data.chunks(3) {
            let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
            let x = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
            out.push(T[(x >> 18) as usize & 63] as char);
            out.push(T[(x >> 12) as usize & 63] as char);
            out.push(if c.len() > 1 { T[(x >> 6) as usize & 63] as char } else { '=' });
            out.push(if c.len() > 2 { T[x as usize & 63] as char } else { '=' });
        }
        out
    }
}
