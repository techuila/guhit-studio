//! The reflected ceiling plan of one level: walls and columns cut as on
//! the plan, door heads across their openings, each room's ceiling with
//! its height (and its finish when one is set), and the ceiling fixtures
//! from the services layer: lights, smoke detectors and fans. Drawn the
//! plan's way round (a reflected plan is not mirrored).

use guhit_model::{Derived, Element, Level, LayerKey, OpeningType, Project, SheetKind};

use crate::geom::*;
use crate::plan::{self, line, Cat, Fill, Item, Pen, PlanOptions, Prim};
use crate::services::{self, Sym};
use crate::text::clean;
use crate::ExportError;

use super::annot::{self, paper};
use super::ViewItems;

/// A room's ceiling height: its own, the engine's schedule, or the level's
/// ceiling (its height, or under the 200 mm slab of the next level up when
/// that is lower, DECISIONS D26).
pub fn ceiling_height(p: &Project, d: &Derived, level: &Level, room: &guhit_model::Room) -> f64 {
    if let Some(h) = room.ceiling_height_mm.filter(|h| h.is_finite() && *h > 0.0) {
        return h;
    }
    if let Some(r) = d.room_schedule.iter().find(|r| r.room_id == room.id) {
        if r.ceiling_height_mm.is_finite() && r.ceiling_height_mm > 0.0 {
            return r.ceiling_height_mm;
        }
    }
    let under_next = p
        .levels
        .iter()
        .filter(|l| l.elevation_mm > level.elevation_mm + 1.0)
        .map(|l| l.elevation_mm - level.elevation_mm - 200.0)
        .fold(f64::INFINITY, f64::min);
    level.height_mm.min(under_next)
}

pub fn items(p: &Project, d: &Derived, level: &Level, n: f64) -> Result<ViewItems, ExportError> {
    let base = plan::build_items(
        p,
        d,
        level,
        &PlanOptions { scale: n, show_dimensions: false, show_room_labels: false, show_assets: false, unicode: true, skip_devices: true },
    );
    // Walls, columns and glazing lines; no swings, stairs or furniture.
    let mut items: Vec<Item> = base.into_iter().filter(|i| matches!(i.cat, Cat::Wall | Cat::Site | Cat::Cols | Cat::Glaz)).collect();

    // Door heads: a line on each face across the opening.
    if plan::layer_visible(p, LayerKey::Openings) {
        for e in &p.elements {
            let Element::Opening(o) = e else { continue };
            if o.opening_type != OpeningType::Door {
                continue;
            }
            let Some(w) = p.elements.iter().find_map(|x| match x {
                Element::Wall(w) if w.id == o.wall_id && w.level_id == level.id => Some(w),
                _ => None,
            }) else {
                continue;
            };
            let (s, e2) = (V::from(w.start), V::from(w.end));
            let Some(u) = (e2 - s).unit() else { continue };
            let len = (e2 - s).len();
            let a = (o.offset_mm - o.width_mm / 2.0).max(0.0);
            let b = (o.offset_mm + o.width_mm / 2.0).min(len);
            if b - a < 1.0 {
                continue;
            }
            let half = u.left() * (w.thickness_mm.abs() / 2.0);
            for face in [half, -half] {
                items.push(line(Cat::Door, Pen::Fine, s + u * a + face, s + u * b + face));
            }
        }
    }

    // Ceilings by room.
    let materials = |id: &Option<String>| -> Option<String> {
        let id = id.as_ref()?;
        p.materials.iter().find(|m| &m.id == id).map(|m| clean(&m.name))
    };
    if plan::layer_visible(p, LayerKey::Rooms) {
        for e in &p.elements {
            let Element::Room(r) = e else { continue };
            if r.level_id != level.id {
                continue;
            }
            let Some(g) = d.rooms.iter().find(|g| g.room_id == r.id) else { continue };
            let poly: Vec<V> = g.polygon.iter().map(V::from).collect();
            if poly.len() < 3 {
                continue;
            }
            let at = V::from(g.label_point);
            let at = if at.is_finite() { at } else { centroid(&poly) };
            let h = ceiling_height(p, d, level, r);
            let th = paper::LABEL * n;
            let mut lines = vec![(clean(&r.name).to_uppercase(), true), (format!("CH {:.3}", h / 1000.0), false)];
            if let Some(m) = materials(&r.finishes.ceiling_material_id) {
                lines.push((m, false));
            }
            let total = lines.len() as f64;
            for (k, (t, bold)) in lines.iter().enumerate() {
                if t.is_empty() {
                    continue;
                }
                let y = at.y + (total / 2.0 - k as f64 - 0.5) * th * 1.45;
                items.push(annot::centered(Cat::Area, v(at.x, y), th * if *bold { 1.0 } else { 0.85 }, 0.0, t, *bold));
            }
        }
    }

    // Ceiling fixtures.
    let dd = services::symbol_size(n);
    for dev in services::devices_on(p, level, SheetKind::Lighting, n)
        .into_iter()
        .chain(services::devices_on(p, level, SheetKind::Power, n))
    {
        if !matches!(dev.sym, Sym::CeilingLight | Sym::Pendant | Sym::Downlight | Sym::Tube | Sym::SmokeDetector) {
            continue;
        }
        for m in &dev.placed.marks {
            items.push(Item { cat: Cat::Furn, pen: m.pen, prim: m.prim.clone() });
        }
        for l in &dev.placed.labels {
            items.push(Item { cat: Cat::Furn, pen: Pen::Light, prim: l.clone() });
        }
    }
    // Ceiling fans: a circle with four blades.
    for e in &p.elements {
        let Element::Asset(a) = e else { continue };
        if a.level_id != level.id || !a.catalog_key.contains("fan") || !plan::layer_visible(p, services::asset_layer(a)) {
            continue;
        }
        let c = V::from(a.position);
        let r = (a.width_mm.abs().max(a.depth_mm.abs()) / 2.0).max(dd * 0.8);
        items.push(annot::circle(Cat::Furn, Pen::Light, c, r * 0.18, Fill::None));
        for k in 0..4 {
            let ang = a.rotation_deg + 45.0 + 90.0 * k as f64;
            let u = dir(ang);
            let s = u.left() * (r * 0.12);
            let (p0, p1) = (c + u * (r * 0.18), c + u * r);
            items.push(Item { cat: Cat::Furn, pen: Pen::Light, prim: Prim::Poly { pts: vec![p0 - s, p1 - s * 0.6, p1 + s * 0.6, p0 + s], closed: true, fill: Fill::None } });
        }
        items.push(annot::centered(Cat::Furn, c + v(0.0, r + 1.6 * n), 1.6 * n, 0.0, "CF", false));
    }

    if items.is_empty() {
        return Err(plan::empty_level(level));
    }
    let bounds = plan::items_bounds(&items);
    let name = clean(&level.name).to_uppercase();
    let title = if name.is_empty() { "REFLECTED CEILING PLAN".into() } else { format!("{name} REFLECTED CEILING PLAN") };
    Ok(ViewItems { items, bounds, title, scale_denominator: n as u32, images: Vec::new() })
}

/// The reflected ceiling plan as a whole sheet for `plan_svg` and
/// `plan_pdf` (`SheetKind::ReflectedCeiling`): the plan sheet's paper,
/// border, band and title block around the ceiling plan of
/// `opts.level_id`. Returns the SVG and the scale used.
pub fn sheet(p: &Project, d: &Derived, opts: &guhit_model::PlanExportOptions) -> Result<(String, u32), ExportError> {
    use crate::sheet::{self, num, Frame, Layout, North, Svg, FONT_FAMILY};
    use crate::text::xml_escape;
    let level = plan::resolve_level(p, opts.level_id.as_ref())?;
    let layout = Layout::new(opts.paper, opts.orientation, opts.title_block);
    let (px, py, pw, ph) = layout.plan;
    let scale = match opts.scale_denominator.filter(|n| *n > 0) {
        Some(n) => n,
        None => {
            let mut failed = None;
            let n = sheet::pick_scale(pw, ph, |n| match items(p, d, level, n as f64) {
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
    let v = items(p, d, level, scale as f64)?;
    let n = scale as f64;
    let center = v.bounds.center();
    let (ox, oy) = (px + pw / 2.0, py + ph / 2.0);
    let to_paper = |q: V| -> (f64, f64) { (ox + (q.x - center.x) / n, oy - (q.y - center.y) / n) };
    let (w, h) = (layout.width, layout.height);
    let mut svg = Svg { s: String::new() };
    svg.s.push_str(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {} {}\" font-family=\"{FONT_FAMILY}\">\n",
        num(w),
        num(h),
        num(w),
        num(h)
    ));
    svg.s.push_str(&format!("<title>{}</title>\n", xml_escape(&format!("{} - {} - 1:{scale}", clean(&p.name), v.title))));
    svg.s.push_str(&format!("<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>\n", num(w), num(h)));
    let m = layout.margin;
    let clip_h = h - 2.0 * m - layout.title_h - layout.band_h;
    svg.s.push_str(&format!(
        "<defs><clipPath id=\"plan-clip\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath></defs>\n<g id=\"plan\" clip-path=\"url(#plan-clip)\">\n",
        num(m),
        num(m),
        num(w - 2.0 * m),
        num(clip_h)
    ));
    super::write_items(&mut svg.s, &v.items, n, &to_paper);
    svg.s.push_str("</g>\n");
    let frame = Frame {
        title: v.title.clone(),
        scale_text: format!("SCALE 1:{scale}"),
        bar: Some(scale),
        north: North::Plan,
        legend: &[],
        block_scale: format!("1:{scale}"),
    };
    sheet::furniture(&mut svg, p, opts, &layout, &frame);
    svg.s.push_str("</svg>\n");
    Ok((svg.s, scale))
}

/// The reflected ceiling plan as model space DXF (`plan_dxf`), annotated
/// for 1:N (`opts.scale_denominator`, else the project's, else 100).
pub fn dxf(p: &Project, d: &Derived, opts: &guhit_model::PlanExportOptions) -> Result<(String, u32), ExportError> {
    let level = plan::resolve_level(p, opts.level_id.as_ref())?;
    let scale = super::scale_of(p, opts.scale_denominator);
    let v = items(p, d, level, scale as f64)?;
    Ok((crate::dxf::write(&v.items, &[], scale), scale))
}
