//! `look` and `measure`: how an agent checks its work (docs/MCP.md "The
//! verify loop").
//!
//! The plan picture is drawn here from the engine's own plan primitives
//! (`guhit_export::plan::build_items`, the same lines a sheet prints), so it
//! needs no window, plus what an agent needs to read it: a grid with
//! coordinates, short element ids, a scale bar, and the mapping from pixels
//! to millimeters. 3D goes through the window (DECISIONS D31).
//!
//! `measure` reads geometry from the model, never from a picture (D8).

use std::sync::{Arc, OnceLock};

use guhit_app::AppService;
use guhit_export::plan::{self, Cat, Fill, HAlign, Item, Pen, PlanOptions, Prim};
use guhit_model::*;
use resvg::usvg;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::draft::element_level;
use crate::fail::ToolFail;
use crate::geo::{self, pt};
use crate::stage;
use crate::tools::{Output, Picture};
use crate::translate::{self, parse};

/// Long side of the plan picture, px, unless the call says otherwise.
const PLAN_PX: u32 = 1400;
/// Room around what is drawn, mm.
const MARGIN_MM: f64 = 1500.0;
/// Text height a plan aims at, px: a sheet's 2.5 mm dimension text.
const TEXT_PX: f64 = 13.0;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BoxIn {
    min: Value,
    max: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LookIn {
    mode: Option<String>,
    level: Option<String>,
    show: Option<Vec<String>>,
    bbox: Option<BoxIn>,
    overlay: Option<String>,
    width_px: Option<u32>,
    source: Option<String>,
    view: Option<String>,
    visibility: Option<Value>,
    mark: Option<String>,
    sheet: Option<String>,
    schedule: Option<ScheduleKind>,
    tour: Option<String>,
    at_s: Option<f64>,
    scale: Option<u32>,
}

const SHOW: [&str; 6] = ["dims", "ids", "grid", "scale_bar", "labels", "furniture"];

pub async fn look(app: &AppService, args: &Value, ctx: &crate::agent::CallContext) -> Result<Output, ToolFail> {
    let i: LookIn = parse(args)?;
    let mode = i.mode.as_deref().unwrap_or("plan");
    // Arguments of the Wave 2 modes, refused on the others.
    for (name, given, modes) in [
        ("mark", i.mark.is_some(), &["elevation", "section"][..]),
        ("sheet", i.sheet.is_some(), &["sheet"][..]),
        ("schedule", i.schedule.is_some(), &["schedule"][..]),
        ("tour", i.tour.is_some(), &["tour"][..]),
        ("at_s", i.at_s.is_some(), &["tour"][..]),
        ("scale", i.scale.is_some(), &["elevation", "section", "schedule"][..]),
    ] {
        if given && !modes.contains(&mode) {
            return Err(ToolFail::args(format!("{name} is for mode {}", modes.join(" or "))));
        }
    }
    if matches!(mode, "elevation" | "section" | "sheet" | "schedule" | "tour") {
        let plan_only = [("level", i.level.is_some()), ("show", i.show.is_some()), ("bbox", i.bbox.is_some()), ("overlay", i.overlay.is_some()), ("source", i.source.is_some()), ("view", i.view.is_some())];
        if let Some((name, _)) = plan_only.iter().find(|(_, given)| *given) {
            return Err(ToolFail::args(format!("{name} is not for mode {mode}")));
        }
        if i.visibility.is_some() && mode != "tour" {
            return Err(ToolFail::args("visibility is for mode 3d and tour"));
        }
    }
    let width = i.width_px.unwrap_or(PLAN_PX).clamp(400, crate::visuals::PICTURE_MAX_PX);
    match mode {
        "plan" => {}
        "elevation" | "section" => return Box::pin(mark_drawing(app, &i, mode, width)).await,
        "sheet" => return Box::pin(sheet_picture(app, &i, width)).await,
        "schedule" => return Box::pin(schedule_picture(app, &i, width)).await,
        "tour" => return Box::pin(tour_frame(app, &i)).await,
        "3d" => {
            if i.show.is_some() || i.bbox.is_some() || i.overlay.is_some() {
                return Err(ToolFail::args("show, bbox and overlay are for mode plan"));
            }
            let mut v1 = serde_json::Map::new();
            if let Some(v) = &i.view {
                v1.insert("view".into(), json!(v));
            }
            if let Some(v) = &i.visibility {
                v1.insert("visibility".into(), v.clone());
            }
            return crate::tools::dispatch(app, "capture_view", &Value::Object(v1), ctx).await;
        }
        other => return Err(ToolFail::args(format!("unknown mode `{other}`. Use plan, 3d, elevation, section, sheet, schedule or tour"))),
    }
    if i.visibility.is_some() {
        return Err(ToolFail::args("visibility is for mode 3d; the plan shows every visible layer"));
    }
    if let Some(bad) = i.show.iter().flatten().find(|s| !SHOW.contains(&s.as_str())) {
        return Err(ToolFail::args(format!("unknown show `{bad}`. Use {}", SHOW.join(", "))));
    }
    if i.source.as_deref() == Some("window") {
        if i.show.is_some() || i.overlay.is_some() {
            return Err(ToolFail::args("the window's own canvas cannot add show or overlay; leave source out for the engine drawing"));
        }
        let mut v1 = json!({});
        if let Some(l) = &i.level {
            v1["level"] = json!(l);
        }
        return crate::tools::dispatch(app, "get_plan_image", &v1, ctx).await;
    }
    let show: Vec<String> = i
        .show
        .clone()
        .unwrap_or_else(|| ["dims", "labels", "furniture", "scale_bar"].iter().map(|s| s.to_string()).collect());
    let has = |s: &str| show.iter().any(|x| x == s);

    let (project, derived, revision) = {
        let s = app.session.lock().await;
        let doc = s.doc.as_ref().ok_or_else(stage::no_document)?;
        (doc.project().clone(), doc.derived().clone(), doc.revision())
    };
    let level = match i.level.as_deref() {
        Some(l) => translate::find_level(&project, l)?.clone(),
        None => project.levels.first().cloned().ok_or_else(|| ToolFail::new("invalid", "the project has no level"))?,
    };
    let mut underlays = vec![];
    if i.overlay.as_deref() == Some("underlay") {
        for el in &project.elements {
            if let Element::Underlay(u) = el {
                if u.level_id == level.id {
                    let url: Value = app.handle("underlay_data", json!({"file_name": u.file_name})).await?;
                    underlays.push((u.clone(), url.as_str().unwrap_or_default().to_string()));
                }
            }
        }
    } else if let Some(o) = &i.overlay {
        return Err(ToolFail::args(format!("unknown overlay `{o}`. Use underlay")));
    }
    let bbox = match &i.bbox {
        Some(b) => {
            let (lo, hi) = (translate::point(&b.min, "bbox.min")?, translate::point(&b.max, "bbox.max")?);
            if hi.x - lo.x < 500.0 || hi.y - lo.y < 500.0 {
                return Err(ToolFail::args("bbox must be at least 500 x 500 mm, min south-west of max"));
            }
            Some((lo, hi))
        }
        None => None,
    };
    let drawing = draw_plan(&project, &derived, &level, &DrawOptions { show: &show, bbox, width_px: width, underlays: &underlays });
    let png = rasterize(&drawing.svg, drawing.width, drawing.height)?;
    let mut json = json!({
        "source": "engine",
        "revision": revision,
        "level": {"id": level.id, "name": level.name},
        "bbox_mm": {"min": drawing.min, "max": drawing.max},
        "px_per_mm": drawing.px_per_mm,
        "image_px": [drawing.width, drawing.height],
        "pixel_to_mm": "x = bbox_mm.min.x + px / px_per_mm, y = bbox_mm.max.y - py / px_per_mm (py from the top)",
        "show": show,
        "note": "Drawn by the engine from the model. Read numbers with query and measure, not off the picture.",
    });
    if has("ids") {
        json["ids"] = json!(drawing.short_ids);
    }
    if has("grid") {
        json["grid_mm"] = json!(drawing.grid_mm);
    }
    if drawing.empty {
        json["empty"] = json!(true);
        json["note"] = json!("Nothing is drawn on this level yet.");
    }
    let picture = crate::visuals::fit_picture(&png, "image/png")?;
    Ok(Output::Rich { json, images: vec![Picture { base64: picture.base64, mime: picture.mime }] })
}

// ------------------------------------------------------------------ drawing

pub(crate) struct DrawOptions<'a> {
    pub show: &'a [String],
    pub bbox: Option<(Point, Point)>,
    pub width_px: u32,
    pub underlays: &'a [(Underlay, String)],
}

pub(crate) struct Drawing {
    pub svg: String,
    pub width: u32,
    pub height: u32,
    pub min: Point,
    pub max: Point,
    pub px_per_mm: f64,
    /// Short id on the picture to the full id.
    pub short_ids: std::collections::BTreeMap<String, String>,
    pub grid_mm: f64,
    pub empty: bool,
}

fn points_of(item: &Item) -> Vec<(f64, f64)> {
    match &item.prim {
        Prim::Line { a, b } => vec![(a.x, a.y), (b.x, b.y)],
        Prim::Poly { pts, .. } => pts.iter().map(|p| (p.x, p.y)).collect(),
        Prim::Arc { c, r, .. } | Prim::Circle { c, r, .. } => vec![(c.x - r, c.y - r), (c.x + r, c.y + r)],
        Prim::Text { pos, .. } => vec![(pos.x, pos.y)],
    }
}

fn esc(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Ink per drawing category: black for what is built, blue for openings,
/// grey for annotation.
fn ink(cat: Cat) -> &'static str {
    match cat {
        Cat::Door | Cat::Glaz => "#1d4ed8",
        Cat::Dims => "#475569",
        Cat::Furn => "#64748b",
        Cat::Site => "#6b7280",
        Cat::Roof => "#334155",
        Cat::Grid | Cat::Symb => "#475569",
        Cat::Text | Cat::Area => "#111827",
        _ => "#111111",
    }
}

fn pen_px(pen: Pen) -> f64 {
    // A sheet's pens (0.35 to 0.13 mm) at the text size this picture uses.
    let w = match pen {
        Pen::Heavy => 0.35,
        Pen::Medium => 0.25,
        Pen::Light => 0.18,
        Pen::Fine => 0.13,
    };
    (w * TEXT_PX / 2.5).max(0.6)
}

/// A short id that is unique among `taken`: the first 6 characters, longer
/// when two would clash.
fn short_id(id: &str, taken: &std::collections::BTreeMap<String, String>) -> String {
    let mut n = 6.min(id.len());
    loop {
        let s: String = id.chars().take(n).collect();
        match taken.get(&s) {
            Some(full) if full != id && n < id.len() => n += 2,
            _ => return s,
        }
    }
}

pub(crate) fn draw_plan(project: &Project, derived: &Derived, level: &Level, o: &DrawOptions) -> Drawing {
    let has = |s: &str| o.show.iter().any(|x| x == s);
    // Lay out once to find the extent, then again at the scale that gives
    // readable text for that extent.
    let first = plan::build_items(project, derived, level, &PlanOptions {
        scale: 100.0,
        show_dimensions: has("dims"),
        show_room_labels: has("labels"),
        show_assets: has("furniture"),
        unicode: true,
        skip_devices: false,
    });
    let mut xs: Vec<f64> = vec![];
    let mut ys: Vec<f64> = vec![];
    for item in &first {
        for (x, y) in points_of(item) {
            if x.is_finite() && y.is_finite() {
                xs.push(x);
                ys.push(y);
            }
        }
    }
    for (u, _) in o.underlays {
        xs.extend([u.position.x, u.position.x + u.width_px as f64 * u.mm_per_px]);
        ys.extend([u.position.y, u.position.y + u.height_px as f64 * u.mm_per_px]);
    }
    let empty = xs.is_empty();
    let fold = |v: &[f64], f: fn(f64, f64) -> f64, start: f64| v.iter().cloned().fold(start, f);
    let (min, max) = match o.bbox {
        Some(b) => b,
        None if empty => (pt(-5000.0, -5000.0), pt(5000.0, 5000.0)),
        None => (
            pt(fold(&xs, f64::min, f64::INFINITY) - MARGIN_MM, fold(&ys, f64::min, f64::INFINITY) - MARGIN_MM),
            pt(fold(&xs, f64::max, f64::NEG_INFINITY) + MARGIN_MM, fold(&ys, f64::max, f64::NEG_INFINITY) + MARGIN_MM),
        ),
    };
    let (w_mm, h_mm) = ((max.x - min.x).max(500.0), (max.y - min.y).max(500.0));
    let px_per_mm = o.width_px as f64 / w_mm.max(h_mm);
    let (width, height) = ((w_mm * px_per_mm).round().max(1.0) as u32, (h_mm * px_per_mm).round().max(1.0) as u32);
    // 1:N such that a 2.5 mm sheet text is TEXT_PX tall here.
    let scale = (TEXT_PX / px_per_mm / 2.5).clamp(10.0, 2000.0);
    let items = plan::build_items(project, derived, level, &PlanOptions {
        scale,
        show_dimensions: has("dims"),
        show_room_labels: has("labels"),
        show_assets: has("furniture"),
        unicode: true,
        skip_devices: false,
    });
    let x = |v: f64| (v - min.x) * px_per_mm;
    let y = |v: f64| (max.y - v) * px_per_mm;
    let f = |v: f64| format!("{:.1}", v);

    let mut s = String::new();
    s.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\" font-family=\"Helvetica, Arial, sans-serif\">\n<rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>\n"
    ));
    // Underlays first, under everything. Rotated about their bottom-left.
    for (u, url) in o.underlays {
        let (uw, uh) = (u.width_px as f64 * u.mm_per_px * px_per_mm, u.height_px as f64 * u.mm_per_px * px_per_mm);
        let (bx, by) = (x(u.position.x), y(u.position.y));
        s.push_str(&format!(
            "<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" opacity=\"{}\" transform=\"rotate({} {} {})\" preserveAspectRatio=\"none\" xlink:href=\"{}\"/>\n",
            f(bx), f(by - uh), f(uw), f(uh), u.opacity.clamp(0.05, 1.0), f(-u.rotation_deg), f(bx), f(by), esc(url)
        ));
    }
    // The grid, with coordinates along the top and left edges.
    let span = w_mm.max(h_mm);
    let grid_mm = [500.0, 1000.0, 2000.0, 5000.0, 10_000.0, 20_000.0].into_iter().find(|g| span / g <= 24.0).unwrap_or(50_000.0);
    if has("grid") {
        s.push_str("<g stroke=\"#cbd5e1\" stroke-width=\"0.6\">\n");
        let mut gx = (min.x / grid_mm).ceil() * grid_mm;
        while gx <= max.x {
            s.push_str(&format!("<line x1=\"{0}\" y1=\"0\" x2=\"{0}\" y2=\"{height}\"/>\n", f(x(gx))));
            gx += grid_mm;
        }
        let mut gy = (min.y / grid_mm).ceil() * grid_mm;
        while gy <= max.y {
            s.push_str(&format!("<line x1=\"0\" y1=\"{0}\" x2=\"{width}\" y2=\"{0}\"/>\n", f(y(gy))));
            gy += grid_mm;
        }
        s.push_str("</g>\n<g fill=\"#64748b\" font-size=\"11\">\n");
        let mut gx = (min.x / grid_mm).ceil() * grid_mm;
        while gx <= max.x {
            s.push_str(&format!("<text x=\"{}\" y=\"12\">{:.0}</text>\n", f(x(gx) + 2.0), gx));
            gx += grid_mm;
        }
        let mut gy = (min.y / grid_mm).ceil() * grid_mm;
        while gy <= max.y {
            s.push_str(&format!("<text x=\"2\" y=\"{}\">{:.0}</text>\n", f(y(gy) - 2.0), gy));
            gy += grid_mm;
        }
        s.push_str("</g>\n");
    }
    // Pipes on this level, thin, by system.
    for el in &project.elements {
        if let Element::Pipe(p) = el {
            if p.level_id != level.id || p.points.len() < 2 {
                continue;
            }
            let color = match p.system {
                PipeSystem::ColdWater => "#0284c7",
                PipeSystem::HotWater => "#dc2626",
                PipeSystem::Drainage | PipeSystem::Storm => "#78350f",
                PipeSystem::Vent => "#16a34a",
                PipeSystem::Conduit => "#a16207",
                PipeSystem::Refrigerant | PipeSystem::Condensate => "#7c3aed",
            };
            let pts: Vec<String> = p.points.iter().map(|q| format!("{},{}", f(x(q.x)), f(y(q.y)))).collect();
            s.push_str(&format!("<polyline points=\"{}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"1.4\"/>\n", pts.join(" ")));
        }
    }
    // The plan, back to front.
    for cat in Cat::ALL {
        for item in items.iter().filter(|it| it.cat == cat) {
            let color = ink(item.cat);
            let sw = f(pen_px(item.pen));
            let dash = if matches!(item.cat, Cat::StrsBelow) { " stroke-dasharray=\"5 3\"" } else { "" };
            let fill = |fl: &Fill| match fl {
                Fill::None => "fill=\"none\"".to_string(),
                Fill::Ink => format!("fill=\"{color}\""),
                #[allow(unreachable_patterns)]
                other => {
                    let pct = match other {
                        Fill::Tint(p) => *p as f64,
                        _ => 15.0,
                    };
                    format!("fill=\"{color}\" fill-opacity=\"{:.2}\"", pct / 100.0)
                }
            };
            match &item.prim {
                Prim::Line { a, b } => s.push_str(&format!(
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{color}\" stroke-width=\"{sw}\"{dash}/>\n",
                    f(x(a.x)), f(y(a.y)), f(x(b.x)), f(y(b.y))
                )),
                Prim::Poly { pts, closed, fill: fl } => {
                    let p: Vec<String> = pts.iter().map(|q| format!("{},{}", f(x(q.x)), f(y(q.y)))).collect();
                    let tag = if *closed { "polygon" } else { "polyline" };
                    s.push_str(&format!("<{tag} points=\"{}\" {} stroke=\"{color}\" stroke-width=\"{sw}\"{dash}/>\n", p.join(" "), fill(fl)));
                }
                Prim::Arc { c, r, start_deg, end_deg } => {
                    let at = |deg: f64| (c.x + r * deg.to_radians().cos(), c.y + r * deg.to_radians().sin());
                    let (p0, p1) = (at(*start_deg), at(*end_deg));
                    let sweep = (end_deg - start_deg).rem_euclid(360.0);
                    let large = if sweep > 180.0 { 1 } else { 0 };
                    s.push_str(&format!(
                        "<path d=\"M{} {}A{} {} 0 {large} 0 {} {}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"{sw}\"{dash}/>\n",
                        f(x(p0.0)), f(y(p0.1)), f(r * px_per_mm), f(r * px_per_mm), f(x(p1.0)), f(y(p1.1))
                    ));
                }
                Prim::Circle { c, r, fill: fl } => s.push_str(&format!(
                    "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" {} stroke=\"{color}\" stroke-width=\"{sw}\"/>\n",
                    f(x(c.x)), f(y(c.y)), f(r * px_per_mm), fill(fl)
                )),
                Prim::Text { pos, height, rot_deg, align, text, bold } => {
                    let anchor = if matches!(align, HAlign::Middle) { "middle" } else { "start" };
                    s.push_str(&format!(
                        "<text transform=\"translate({} {}) rotate({})\" font-size=\"{}\" text-anchor=\"{anchor}\"{} fill=\"{color}\">{}</text>\n",
                        f(x(pos.x)), f(y(pos.y)), f(-rot_deg), f((height * px_per_mm).max(7.0)),
                        if *bold { " font-weight=\"bold\"" } else { "" },
                        esc(text)
                    ));
                }
            }
        }
    }
    // Wall lengths along each wall, for an agent checking sizes.
    if has("dims") {
        s.push_str("<g fill=\"#475569\" font-size=\"10\" text-anchor=\"middle\">\n");
        for el in &project.elements {
            let Element::Wall(w) = el else { continue };
            if w.level_id != level.id {
                continue;
            }
            let len = geo::dist(w.start, w.end);
            if len < 300.0 {
                continue;
            }
            let (ux, uy) = ((w.end.x - w.start.x) / len, (w.end.y - w.start.y) / len);
            let off = w.thickness_mm / 2.0 + 14.0 / px_per_mm;
            let at = pt((w.start.x + w.end.x) / 2.0 - uy * off, (w.start.y + w.end.y) / 2.0 + ux * off);
            // Upright on screen: plan angles turn the other way after the y flip.
            let mut angle = -uy.atan2(ux).to_degrees();
            if angle > 90.0 {
                angle -= 180.0;
            } else if angle < -90.0 {
                angle += 180.0;
            }
            s.push_str(&format!(
                "<text transform=\"translate({} {}) rotate({})\" stroke=\"#ffffff\" stroke-width=\"3\" paint-order=\"stroke\">{:.0}</text>\n",
                f(x(at.x)), f(y(at.y) + 3.0), f(angle), len
            ));
        }
        s.push_str("</g>\n");
    }
    // Short ids by each element, in red.
    let mut short_ids = std::collections::BTreeMap::new();
    if has("ids") {
        s.push_str("<g fill=\"#b91c1c\" font-size=\"10\" font-weight=\"bold\" text-anchor=\"middle\">\n");
        let rooms_at: std::collections::BTreeMap<&Id, &RoomGeometry> = derived.rooms.iter().map(|g| (&g.room_id, g)).collect();
        for el in &project.elements {
            if element_level(project, el) != Some(&level.id) {
                continue;
            }
            let at = match el {
                Element::Wall(w) => Some(pt((w.start.x + w.end.x) / 2.0, (w.start.y + w.end.y) / 2.0)),
                Element::Opening(op) => match project.elements.iter().find(|e| e.id() == &op.wall_id) {
                    Some(Element::Wall(w)) => geo::opening_center(w, op),
                    _ => None,
                },
                Element::Room(r) => rooms_at.get(&r.id).map(|g| g.label_point),
                Element::Column(c) => Some(c.center),
                Element::Stair(st) => Some(st.origin),
                Element::Slab(sl) => geo::interior_point(&sl.polygon),
                Element::Asset(a) => Some(a.position),
                Element::Pipe(p) => p.points.first().map(|q| pt(q.x, q.y)),
                _ => None,
            };
            let Some(at) = at else { continue };
            let short = short_id(el.id(), &short_ids);
            short_ids.insert(short.clone(), el.id().clone());
            let dy = if matches!(el, Element::Room(_)) { 22.0 } else { -4.0 };
            s.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" stroke=\"#ffffff\" stroke-width=\"3\" paint-order=\"stroke\">{}</text>\n",
                f(x(at.x)), f(y(at.y) + dy), esc(&short)
            ));
        }
        s.push_str("</g>\n");
    }
    if has("scale_bar") {
        let target = w_mm / 5.0;
        let bar = [500.0, 1000.0, 2000.0, 5000.0, 10_000.0, 20_000.0].into_iter().rev().find(|b| *b <= target).unwrap_or(500.0);
        let (x0, y0) = (16.0, height as f64 - 18.0);
        let len = bar * px_per_mm;
        s.push_str(&format!(
            "<g stroke=\"#111111\" stroke-width=\"2\"><line x1=\"{0}\" y1=\"{1}\" x2=\"{2}\" y2=\"{1}\"/><line x1=\"{0}\" y1=\"{3}\" x2=\"{0}\" y2=\"{4}\"/><line x1=\"{2}\" y1=\"{3}\" x2=\"{2}\" y2=\"{4}\"/></g>\n<text x=\"{5}\" y=\"{6}\" font-size=\"12\" fill=\"#111111\">{7} mm</text>\n",
            f(x0), f(y0), f(x0 + len), f(y0 - 5.0), f(y0 + 5.0), f(x0 + len + 6.0), f(y0 + 4.0), bar
        ));
    }
    s.push_str(&format!(
        "<text x=\"{}\" y=\"{}\" font-size=\"13\" text-anchor=\"end\" fill=\"#111111\" font-weight=\"bold\">{} (floor {:.0} mm)</text>\n",
        width.saturating_sub(10),
        height.saturating_sub(12),
        esc(&level.name),
        level.elevation_mm
    ));
    s.push_str("</svg>\n");
    Drawing { svg: s, width, height, min, max, px_per_mm: (px_per_mm * 1.0e6).round() / 1.0e6, short_ids, grid_mm, empty }
}

fn fonts() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    })
    .clone()
}

/// SVG to PNG bytes.
pub(crate) fn rasterize(svg: &str, width: u32, height: u32) -> Result<Vec<u8>, ToolFail> {
    let opt = usvg::Options { fontdb: fonts(), font_family: "Arial".into(), ..usvg::Options::default() };
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| ToolFail::new("io", format!("the plan drawing could not be read: {e}")))?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or_else(|| ToolFail::new("io", "the plan picture is too large"))?;
    resvg::render(&tree, resvg::tiny_skia::Transform::identity(), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| ToolFail::new("io", format!("the plan picture could not be encoded: {e}")))
}

/// Any SVG drawing (an elevation, a sheet, a schedule) as a PNG whose long
/// side is `long_px`, on white. Returns the PNG and its size.
pub(crate) fn svg_png(svg: &str, long_px: u32) -> Result<(Vec<u8>, u32, u32), ToolFail> {
    let opt = usvg::Options { fontdb: fonts(), font_family: "Arial".into(), ..usvg::Options::default() };
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| ToolFail::new("io", format!("the drawing could not be read: {e}")))?;
    let size = tree.size();
    let k = long_px as f32 / size.width().max(size.height()).max(1.0);
    let (w, h) = ((size.width() * k).round().max(1.0) as u32, (size.height() * k).round().max(1.0) as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or_else(|| ToolFail::new("io", "the picture is too large"))?;
    pixmap.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(k, k), &mut pixmap.as_mut());
    let png = pixmap.encode_png().map_err(|e| ToolFail::new("io", format!("the picture could not be encoded: {e}")))?;
    Ok((png, w, h))
}

/// A drawing as the picture an agent gets, with what it shows.
fn drawing_output(svg: &str, width: u32, json: Value) -> Result<Output, ToolFail> {
    let (png, w, h) = svg_png(svg, width)?;
    let mut json = json;
    json["image_px"] = json!([w, h]);
    let picture = crate::visuals::fit_picture(&png, "image/png")?;
    Ok(Output::Rich { json, images: vec![Picture { base64: picture.base64, mime: picture.mime }] })
}

/// `view_svg` of a view, as the app draws it for sheets.
async fn view_drawing(app: &AppService, view: &ViewRef, scale: Option<u32>) -> Result<SvgDrawing, ToolFail> {
    let v = app.handle("view_svg", json!({"view": view, "scale": scale})).await.map_err(|e| {
        let not_built = e.code == "not_built";
        let f = ToolFail::from(e);
        if not_built {
            f.with_hint("That drawing is not built in this version yet. look mode plan per level, mode 3d from a saved view, and measure height show the same model.")
        } else {
            f
        }
    })?;
    serde_json::from_value(v).map_err(|e| ToolFail::new("io", e.to_string()))
}

/// look mode elevation or section: the drawing of a mark.
async fn mark_drawing(app: &AppService, i: &LookIn, mode: &str, width: u32) -> Result<Output, ToolFail> {
    let kind = if mode == "section" { MarkKind::Section } else { MarkKind::Elevation };
    let (mark, revision) = {
        let s = app.session.lock().await;
        let doc = s.doc.as_ref().ok_or_else(stage::no_document)?;
        let marks: Vec<&ViewMark> = doc
            .project()
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::ViewMark(m) if m.mark_kind == kind => Some(m),
                _ => None,
            })
            .collect();
        let known = || marks.iter().map(|m| format!("\"{}\" ({})", m.label, m.id)).collect::<Vec<_>>().join(", ");
        let hint = "view_marks action \"add\" makes one; sheets action \"make\" adds four elevations and two sections.";
        let Some(wanted) = i.mark.as_deref().map(str::trim).filter(|m| !m.is_empty()) else {
            return Err(if marks.is_empty() {
                ToolFail::new("not_found", format!("the project has no {mode} marks yet")).with_hint(hint)
            } else {
                ToolFail::args(format!("give mark: one of {}", known()))
            });
        };
        let found = marks
            .iter()
            .find(|m| m.id == wanted)
            .or_else(|| marks.iter().find(|m| m.label.trim().eq_ignore_ascii_case(wanted)))
            .ok_or_else(|| {
                ToolFail::new("not_found", if marks.is_empty() { format!("the project has no {mode} marks yet") } else { format!("no {mode} mark is `{wanted}`. Marks: {}", known()) }).with_hint(hint)
            })?;
        ((*found).clone(), doc.revision())
    };
    let view = if kind == MarkKind::Section { ViewRef::Section { mark_id: mark.id.clone() } } else { ViewRef::Elevation { mark_id: mark.id.clone() } };
    let drawing = view_drawing(app, &view, i.scale).await.map_err(|f| {
        if f.message.contains("nothing") {
            f.with_hint("A mark looks to the left going start to end: set flip with modify (set {flip: true}), or move it so the building is in front of it.")
        } else {
            f
        }
    })?;
    drawing_output(
        &drawing.svg,
        width,
        json!({
            "mode": mode, "revision": revision,
            "mark": {"id": mark.id, "label": mark.label, "start": mark.start, "end": mark.end, "flip": mark.flip},
            "scale": drawing.scale_denominator.map(|n| format!("1:{n}")),
            "note": "Drawn by the engine from the model, hidden lines removed. Read heights with measure, not off the picture.",
        }),
    )
}

/// look mode sheet: a permit sheet as an agent may see it (D53: license
/// numbers masked).
async fn sheet_picture(app: &AppService, i: &LookIn, width: u32) -> Result<Output, ToolFail> {
    let wanted = i.sheet.as_deref().ok_or_else(|| ToolFail::args("mode sheet needs sheet: its number (A-1) or id; sheets action \"list\" shows them"))?;
    let (svg, meta) = crate::documents::sheet_svg_for_agent(app, wanted).await?;
    drawing_output(
        &svg,
        width,
        json!({
            "mode": "sheet", "sheet": meta,
            "note": "The signing architect's license numbers are masked on this picture; the PDF on disk prints them. Nothing on a sheet is permit approval.",
        }),
    )
}

/// look mode schedule: the schedule as drawn on a sheet, and its rows as
/// data. The data comes back even when the drawing cannot be made.
async fn schedule_picture(app: &AppService, i: &LookIn, width: u32) -> Result<Output, ToolFail> {
    let schedule = i.schedule.ok_or_else(|| ToolFail::args("mode schedule needs schedule: doors_windows, rooms, finishes or areas"))?;
    let (data, revision) = {
        let s = app.session.lock().await;
        let doc = s.doc.as_ref().ok_or_else(stage::no_document)?;
        let q = match schedule {
            ScheduleKind::DoorsWindows => Query::OpeningSchedule,
            ScheduleKind::Rooms | ScheduleKind::Finishes => Query::RoomSchedule,
            ScheduleKind::Areas => Query::LevelAreas,
        };
        (doc.query(&q).map_err(ToolFail::from)?, doc.revision())
    };
    let json = json!({"mode": "schedule", "schedule": schedule, "revision": revision, "rows": data, "note": "The numbers are the engine's; schedules are data, never a compliance check."});
    match view_drawing(app, &ViewRef::Schedule { schedule }, i.scale).await {
        Ok(drawing) => drawing_output(&drawing.svg, width, json),
        Err(f) if f.code == "not_built" => {
            let mut json = json;
            json["drawing"] = json!("not built in this version yet: the rows above are the schedule");
            Ok(Output::Json(json))
        }
        Err(f) => Err(f),
    }
}

/// A frame's camera from `Query::TourFrames`: `{position, target, fov_deg}`,
/// at the top or under `camera`.
fn frame_camera(frame: &Value, name: &str) -> Option<Camera> {
    let c = if frame.get("position").is_some() { frame } else { frame.get("camera")? };
    let v = |k: &str| serde_json::from_value::<Vec3>(c.get(k)?.clone()).ok();
    Some(Camera {
        id: String::new(),
        name: name.to_string(),
        preset: CameraPreset::Custom,
        position: v("position")?,
        target: v("target")?,
        fov_deg: c.get("fov_deg").and_then(Value::as_f64).unwrap_or(60.0),
        light: c.get("light").and_then(|l| serde_json::from_value(l.clone()).ok()),
    })
}

/// look mode tour: the frame at `at_s`, captured in the window from the
/// camera the engine works out for it (the same an export uses).
async fn tour_frame(app: &AppService, i: &LookIn) -> Result<Output, ToolFail> {
    let wanted = i.tour.as_deref().ok_or_else(|| ToolFail::args("mode tour needs tour: its id or name"))?;
    let at = i.at_s.unwrap_or(0.0);
    if !(at.is_finite() && at >= 0.0) {
        return Err(ToolFail::args("at_s is 0 or more"));
    }
    let (tour, frames) = {
        let s = app.session.lock().await;
        let doc = s.doc.as_ref().ok_or_else(stage::no_document)?;
        let tour = crate::tours::find_tour(doc.project(), wanted)?.clone();
        let v = doc.query(&Query::TourFrames { tour_id: tour.id.clone(), fps: None }).map_err(ToolFail::from)?;
        (tour, v["frames"].as_array().cloned().unwrap_or_default())
    };
    if frames.is_empty() {
        return Err(ToolFail::new("not_built", format!("the engine has no frames for tour \"{}\" in this version yet", tour.name))
            .with_hint("look mode 3d from a saved view shows the model meanwhile."));
    }
    let fps = tour.fps.max(1) as f64;
    let index = ((at * fps).round() as usize).min(frames.len() - 1);
    let camera = frame_camera(&frames[index], &format!("{} at {:.1} s", tour.name, index as f64 / fps))
        .ok_or_else(|| ToolFail::new("io", "a tour frame has no camera"))?;
    let visibility = crate::visuals::visibility_of(&json!({"visibility": i.visibility}))?;
    let mut out = crate::visuals::capture_camera(app, camera, visibility, std::time::Duration::from_secs(45)).await?;
    if let Output::Json(v) | Output::Rich { json: v, .. } = &mut out {
        v["tour"] = json!({"id": tour.id, "name": tour.name, "frame": index, "frames": frames.len(), "at_s": (index as f64 / fps * 100.0).round() / 100.0, "seconds": (frames.len() as f64 / fps * 100.0).round() / 100.0});
        // A tour frame is a look, not a keeper: the window's capture leaves
        // Visuals once the picture is in hand, so frames never pile up in
        // the project and its bundle at about 2.4 MB each (GS F11).
        if v["status"] == "done" {
            let ids: Vec<String> = v["renders"].as_array().into_iter().flatten().filter_map(|r| r["id"].as_str().map(str::to_string)).collect();
            for id in &ids {
                if let Err(e) = app.handle("render_delete", json!({"id": id})).await {
                    eprintln!("guhit-mcp: a tour frame's capture {id} stayed in Visuals: {}", e.message);
                }
            }
            v["renders"] = json!([]);
            v["kept"] = json!(false);
            let kept = "A tour frame is not kept in Visuals. To keep one, look mode 3d or render action \"start\" from a saved view.";
            v["note"] = match v["note"].as_str().filter(|n| !n.trim().is_empty()) {
                Some(n) => json!(format!("{n} {kept}")),
                None => json!(kept),
            };
        }
    }
    Ok(out)
}

/// The engine-drawn plan of a level as SVG, for the resource
/// `guhit://project/levels/{level}/plan.svg`.
pub(crate) async fn plan_svg(app: &AppService, level: &str) -> Result<String, ToolFail> {
    let (project, derived) = {
        let s = app.session.lock().await;
        let doc = s.doc.as_ref().ok_or_else(stage::no_document)?;
        (doc.project().clone(), doc.derived().clone())
    };
    let level = translate::find_level(&project, level)?.clone();
    let show: Vec<String> = ["dims", "ids", "grid", "scale_bar", "labels", "furniture"].iter().map(|s| s.to_string()).collect();
    Ok(draw_plan(&project, &derived, &level, &DrawOptions { show: &show, bbox: None, width_px: PLAN_PX, underlays: &[] }).svg)
}

// ------------------------------------------------------------------ measure

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MeasureIn {
    kind: String,
    from: Option<Value>,
    to: Option<Value>,
    ids: Option<Vec<Id>>,
    polygon: Option<Vec<Value>>,
}

/// What a `from` or `to` names: a point, or an element.
enum Target<'a> {
    Point(Point),
    Element(&'a Element),
}

fn target<'a>(project: &'a Project, v: &Value, what: &str) -> Result<Target<'a>, ToolFail> {
    if let Some(id) = v.as_str() {
        return project
            .elements
            .iter()
            .find(|e| e.id() == id)
            .map(Target::Element)
            .ok_or_else(|| ToolFail::new("not_found", format!("no element has id `{id}`")));
    }
    translate::point(v, what).map(Target::Point)
}

/// An element's plan outlines, for distances, clearances and footprints:
/// one polygon, or for a stair every flight and landing it stands on, from
/// the engine's own stair geometry (`Derived::stairs`), so an L or U is
/// measured as it is built (GR1 #11).
fn outlines(project: &Project, derived: &Derived, el: &Element) -> Vec<Vec<Point>> {
    if let Element::Stair(s) = el {
        if let Some(g) = derived.stairs.iter().find(|g| g.stair_id == s.id) {
            let pieces = guhit_core::stairs::pieces(g);
            if !pieces.is_empty() {
                return pieces;
            }
        }
    }
    vec![outline(project, derived, el)]
}

/// The smallest gap between two sets of outlines; NaN when either is empty.
fn gap_between(a: &[Vec<Point>], b: &[Vec<Point>]) -> f64 {
    a.iter()
        .flat_map(|p| b.iter().map(move |q| geo::polygon_gap(p, q)))
        .filter(|g| !g.is_nan())
        .fold(f64::NAN, f64::min)
}

/// The middle of a set of outlines: weighted by area, or the mean of the
/// points when they have none.
fn middle(shapes: &[Vec<Point>]) -> Point {
    let (mut ax, mut ay, mut total) = (0.0, 0.0, 0.0);
    for s in shapes.iter().filter(|s| s.len() >= 3) {
        let a = geo::signed_area(s).abs();
        let n = s.len() as f64;
        ax += a * s.iter().map(|p| p.x).sum::<f64>() / n;
        ay += a * s.iter().map(|p| p.y).sum::<f64>() / n;
        total += a;
    }
    if total > 1e-6 {
        return pt(ax / total, ay / total);
    }
    let all: Vec<&Point> = shapes.iter().flatten().collect();
    let n = all.len().max(1) as f64;
    pt(all.iter().map(|p| p.x).sum::<f64>() / n, all.iter().map(|p| p.y).sum::<f64>() / n)
}

/// An element's plan outline, for distances and clearances. A stair's is
/// its flights and landings: use `outlines`.
fn outline(project: &Project, derived: &Derived, el: &Element) -> Vec<Point> {
    match el {
        Element::Wall(w) => derived
            .walls
            .iter()
            .find(|g| g.wall_id == w.id)
            .map(|g| g.outline.clone())
            .filter(|o| o.len() >= 3)
            .unwrap_or_else(|| vec![w.start, w.end]),
        Element::Column(c) => geo::rect(c.center, c.width_mm, c.depth_mm, c.rotation_deg),
        Element::Asset(a) => geo::rect(a.position, a.width_mm, a.depth_mm, a.rotation_deg),
        Element::Stair(s) => geo::stair_outline(s),
        Element::Slab(s) => s.polygon.clone(),
        Element::Room(r) => derived.rooms.iter().find(|g| g.room_id == r.id).map(|g| g.polygon.clone()).unwrap_or_else(|| vec![r.seed]),
        Element::Opening(o) => match project.elements.iter().find(|e| e.id() == &o.wall_id) {
            Some(Element::Wall(w)) => match geo::opening_center(w, o) {
                Some(c) => {
                    let angle = (w.end.y - w.start.y).atan2(w.end.x - w.start.x).to_degrees();
                    geo::rect(c, o.width_mm, w.thickness_mm, angle)
                }
                None => vec![],
            },
            _ => vec![],
        },
        Element::Pipe(p) => p.points.iter().map(|q| pt(q.x, q.y)).collect(),
        Element::Annotation(a) => vec![a.position],
        Element::Camera(c) => vec![pt(c.position.x, c.position.y)],
        _ => vec![],
    }
}

fn r(v: f64) -> f64 {
    geo::r1(v)
}

/// Two walls run side by side: their direction differs by less than half a
/// degree.
fn parallel(a: &Wall, b: &Wall) -> bool {
    let da = (a.end.y - a.start.y).atan2(a.end.x - a.start.x);
    let db = (b.end.y - b.start.y).atan2(b.end.x - b.start.x);
    let d = (da - db).rem_euclid(std::f64::consts::PI);
    d.min(std::f64::consts::PI - d) < 0.5_f64.to_radians()
}

pub async fn measure(app: &AppService, args: &Value) -> Result<Output, ToolFail> {
    let i: MeasureIn = parse(args)?;
    let s = app.session.lock().await;
    let doc = s.doc.as_ref().ok_or_else(stage::no_document)?;
    let project = doc.project();
    let derived = doc.derived();
    let ids = i.ids.clone().unwrap_or_default();
    let els = || -> Result<Vec<&Element>, ToolFail> {
        ids.iter()
            .map(|id| project.elements.iter().find(|e| e.id() == id).ok_or_else(|| ToolFail::new("not_found", format!("no element has id `{id}`"))))
            .collect()
    };
    let out = match i.kind.as_str() {
        "distance" => {
            let (a, b) = match (&i.from, &i.to, ids.as_slice()) {
                (Some(f), Some(t), []) => (target(project, f, "from")?, target(project, t, "to")?),
                (None, None, [x, y]) => (target(project, &json!(x), "ids[0]")?, target(project, &json!(y), "ids[1]")?),
                _ => return Err(ToolFail::args("give from and to (points or ids), or two ids")),
            };
            distance(project, derived, &a, &b)
        }
        "clear_width" => {
            let list = els()?;
            match list.as_slice() {
                [Element::Wall(a), Element::Wall(b)] => {
                    let Some((_, off)) = geo::project(a.start, a.end, pt((b.start.x + b.end.x) / 2.0, (b.start.y + b.end.y) / 2.0)) else {
                        return Err(ToolFail::args("the first wall has no length"));
                    };
                    if !parallel(a, b) {
                        return Err(ToolFail::args("the two walls are not parallel; use kind distance or clearance"));
                    }
                    json!({
                        "kind": "clear_width",
                        "centerline_mm": r(off.abs()),
                        "clear_mm": r(off.abs() - a.thickness_mm / 2.0 - b.thickness_mm / 2.0),
                        "note": "Face to face between the two walls, from their centerlines and thicknesses.",
                    })
                }
                [Element::Room(room)] => {
                    let v = doc.query(&Query::Describe { ids: vec![room.id.clone()] }).map_err(ToolFail::from)?;
                    let e = &v["elements"][0];
                    json!({"kind": "clear_width", "room": room.name, "clear_width_x_mm": e["clear_width_x_mm"], "clear_depth_y_mm": e["clear_depth_y_mm"], "is_rectangular": e["is_rectangular"], "note": "The room's net extent east-west (x) and north-south (y), on the inner wall faces."})
                }
                [Element::Opening(o)] => json!({"kind": "clear_width", "opening": o.id, "width_mm": r(o.width_mm), "height_mm": r(o.height_mm), "style": o.style, "note": "The opening's nominal size; the frame and the leaf take some of it."}),
                _ => return Err(ToolFail::args("clear_width takes two parallel walls, one room or one opening as ids")),
            }
        }
        "height" => {
            let rows: Vec<Value> = if ids.is_empty() {
                project.levels.iter().map(|l| level_height(project, l)).collect()
            } else {
                ids.iter()
                    .map(|id| {
                        if let Some(l) = project.levels.iter().find(|l| &l.id == id || l.name.eq_ignore_ascii_case(id)) {
                            return Ok(level_height(project, l));
                        }
                        let el = project.elements.iter().find(|e| e.id() == id).ok_or_else(|| ToolFail::new("not_found", format!("no element or level is `{id}`")))?;
                        Ok(element_height(project, derived, el))
                    })
                    .collect::<Result<_, ToolFail>>()?
            };
            json!({"kind": "height", "items": rows})
        }
        "area" => {
            if let Some(poly) = &i.polygon {
                let pts = poly.iter().enumerate().map(|(n, v)| translate::point(v, &format!("polygon[{n}]"))).collect::<Result<Vec<_>, _>>()?;
                json!({"kind": "area", "polygon_m2": (geo::signed_area(&pts).abs() / 1.0e4).round() / 100.0})
            } else {
                let rows: Vec<Value> = els()?
                    .into_iter()
                    .map(|el| match el {
                        Element::Room(room) => {
                            // Both areas as the room schedule gives them (GR1 #12).
                            let g = derived.rooms.iter().find(|g| g.room_id == room.id);
                            let row = derived.room_schedule.iter().find(|row| row.room_id == room.id);
                            json!({
                                "id": room.id, "room": room.name,
                                "net_m2": row.map(|r| crate::draft::m2_2(r.net_area_m2)).or(g.map(|g| crate::draft::m2_2(g.area_mm2 / 1.0e6))),
                                "floor_m2": row.map(|r| crate::draft::m2_2(r.floor_area_m2)),
                                "enclosed": g.is_some(),
                            })
                        }
                        Element::Slab(sl) => json!({"id": sl.id, "slab_kind": sl.kind, "outline_m2": (geo::signed_area(&sl.polygon).abs() / 1.0e4).round() / 100.0}),
                        other => {
                            let area: f64 = outlines(project, derived, other).iter().map(|o| geo::signed_area(o).abs()).sum();
                            json!({"id": other.id(), "kind": other.kind(), "footprint_m2": (area / 1.0e4).round() / 100.0})
                        }
                    })
                    .collect();
                if rows.is_empty() {
                    return Err(ToolFail::args("area takes ids (rooms, slabs) or a polygon"));
                }
                json!({"kind": "area", "items": rows})
            }
        }
        "clearance" => {
            let list = els()?;
            let [a, b] = list.as_slice() else {
                return Err(ToolFail::args("clearance takes two ids"));
            };
            let gap = gap_between(&outlines(project, derived, a), &outlines(project, derived, b));
            let same_level = element_level(project, a) == element_level(project, b);
            json!({
                "kind": "clearance",
                "gap_mm": if gap.is_finite() { json!(r(gap)) } else { Value::Null },
                "overlap": gap == 0.0,
                "same_level": same_level,
                "note": "The shortest plan distance between the two outlines; 0 when they touch or overlap.",
            })
        }
        other => return Err(ToolFail::args(format!("unknown kind `{other}`. Use distance, clear_width, height, area or clearance"))),
    };
    Ok(Output::Json(out))
}

fn distance(project: &Project, derived: &Derived, a: &Target, b: &Target) -> Value {
    match (a, b) {
        (Target::Point(p), Target::Point(q)) => json!({
            "kind": "distance", "mm": r(geo::dist(*p, *q)), "dx_mm": r(q.x - p.x), "dy_mm": r(q.y - p.y),
        }),
        (Target::Element(Element::Wall(w)), Target::Element(Element::Wall(v))) if parallel(w, v) => {
            let off = geo::project(w.start, w.end, pt((v.start.x + v.end.x) / 2.0, (v.start.y + v.end.y) / 2.0)).map(|x| x.1).unwrap_or(f64::NAN);
            json!({
                "kind": "distance",
                "centerline_mm": r(off.abs()),
                "clear_mm": r(off.abs() - w.thickness_mm / 2.0 - v.thickness_mm / 2.0),
                "note": "Parallel walls: centerline to centerline, and face to face.",
            })
        }
        (Target::Point(p), Target::Element(Element::Wall(w))) | (Target::Element(Element::Wall(w)), Target::Point(p)) => {
            let d = geo::dist_point_segment(*p, w.start, w.end);
            json!({"kind": "distance", "to_centerline_mm": r(d), "to_face_mm": r((d - w.thickness_mm / 2.0).max(0.0))})
        }
        _ => {
            let shape = |t: &Target| match t {
                Target::Point(p) => vec![vec![*p]],
                Target::Element(e) => outlines(project, derived, e),
            };
            let (sa, sb) = (shape(a), shape(b));
            json!({
                "kind": "distance",
                "gap_mm": r(gap_between(&sa, &sb)),
                "center_to_center_mm": r(geo::dist(middle(&sa), middle(&sb))),
                "note": "gap_mm is between the outlines; center_to_center_mm between their middles.",
            })
        }
    }
}

fn level_height(project: &Project, l: &Level) -> Value {
    json!({
        "level": l.name, "id": l.id,
        "floor_mm": r(l.elevation_mm),
        "floor_to_floor_mm": r(l.height_mm),
        "ceiling_mm": r(guhit_app::ai::tools::ceiling_height_mm(project, &l.id)),
        "top_mm": r(l.elevation_mm + l.height_mm),
    })
}

fn element_height(project: &Project, derived: &Derived, el: &Element) -> Value {
    let level = element_level(project, el).and_then(|id| project.levels.iter().find(|l| &l.id == id));
    let floor = level.map(|l| l.elevation_mm).unwrap_or(0.0);
    let lh = level.map(|l| l.height_mm).unwrap_or(defaults::DEFAULT_LEVEL_HEIGHT_MM);
    match el {
        Element::Wall(w) => {
            let h = w.height_mm.unwrap_or(lh);
            json!({"id": w.id, "kind": "wall", "height_mm": r(h), "own_height": w.height_mm.is_some(), "top_mm": r(floor + h)})
        }
        Element::Opening(o) => json!({"id": o.id, "kind": "opening", "sill_mm": r(o.sill_mm), "height_mm": r(o.height_mm), "head_mm": r(o.sill_mm + o.height_mm)}),
        Element::Stair(s) => {
            // The engine's stair, as plan, 3D, IFC and the walk line build it
            // (D54): n risers make n - 1 treads straight, n - 2 in an L or U.
            let g = derived.stairs.iter().find(|g| g.stair_id == s.id);
            let n = s.riser_count.max(1) as f64;
            json!({
                "id": s.id, "kind": "stair", "shape": s.shape, "rise_mm": r(lh), "riser_count": s.riser_count,
                "riser_mm": r(g.map(|g| g.riser_mm).unwrap_or(lh / n)),
                "treads": guhit_core::stairs::tread_count(s),
                "tread_mm": g.map(|g| r(g.tread_mm)),
                "flights": g.map(|g| g.flights.iter().map(|f| f.nosings.len() + 1).collect::<Vec<_>>()),
                "landings": g.map(|g| g.landings.len()),
                "run_mm": r(s.run_mm),
            })
        }
        Element::Column(c) => json!({"id": c.id, "kind": "column", "height_mm": r(lh), "note": "A column rises its level's floor-to-floor height."}),
        Element::Asset(a) => json!({"id": a.id, "kind": "object", "name": a.name, "elevation_mm": r(a.elevation_mm), "height_mm": r(a.height_mm), "top_mm": r(a.elevation_mm + a.height_mm)}),
        Element::Slab(s) => json!({"id": s.id, "kind": "slab", "slab_kind": s.kind, "top_mm": r(floor + s.top_offset_mm), "thickness_mm": r(s.thickness_mm), "railing_mm": s.railing.map(|x| r(x.height_mm))}),
        other => json!({"id": other.id(), "kind": other.kind(), "note": "no height to measure"}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_level_still_draws() {
        let project = defaults::new_project("t");
        let derived = guhit_core::compute_derived(&project);
        let show: Vec<String> = SHOW.iter().map(|s| s.to_string()).collect();
        let d = draw_plan(&project, &derived, &project.levels[0], &DrawOptions { show: &show, bbox: None, width_px: 600, underlays: &[] });
        assert!(d.empty);
        let png = rasterize(&d.svg, d.width, d.height).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
    }

    #[test]
    fn short_ids_grow_when_they_clash() {
        let mut taken = std::collections::BTreeMap::new();
        taken.insert("abcdef".to_string(), "abcdef12".to_string());
        assert_eq!(short_id("abcdef99", &taken), "abcdef99");
        assert_eq!(short_id("abcdef12", &taken), "abcdef");
    }
}
