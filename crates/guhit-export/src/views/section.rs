//! Elevations and sections from a view mark (DECISIONS D54): the model's
//! feature edges with hidden lines removed, cut solids in poche, and on
//! every one the grids, level datums ("+3.000 SECOND FLOOR"), the finish
//! grade, the roof and the door and window marks.
//!
//! Drawing space: x is across the picture plane (to the viewer's right),
//! y is the height above project zero, both mm. A section is cropped to its
//! cut line; an elevation shows the whole model beyond its plane.
//!
//! Pens: cut outlines heavy over solid poche; the profile against open sky
//! heavy on an elevation and medium on a section; near edges medium (light
//! on a section), farther ones light, far ones fine; sash and panel lines,
//! entourage, grids and datums fine.

use std::sync::Arc;

use guhit_model::{Derived, Element, LayerKey, MarkKind, Project, ViewMark};

use crate::geom::*;
use crate::plan::{line, Cat, Fill, Item, Pen, Prim};
use crate::text::clean;
use crate::ExportError;

use super::annot::{self, paper};
use super::hlr::{self, EdgeKind, Hlr, Options, Piece, ViewFrame};
use super::mesh::{FigureKind, Mesh, Part};
use super::ViewItems;

/// Depth past the front-most edge that still counts as near, mm.
const NEAR_MM: f64 = 1500.0;
/// Depth past the front-most edge from which edges are far, mm.
const FAR_MM: f64 = 6000.0;
/// How far beside a line the profile test looks, mm.
const PROFILE_PROBE_MM: f64 = 20.0;

/// The frame and clipping of a mark.
pub fn frame_of(mark: &ViewMark) -> Result<(ViewFrame, Options, Option<(f64, f64)>), ExportError> {
    let (a, b) = (V::from(mark.start), V::from(mark.end));
    let Some(u) = (b - a).unit() else {
        return Err(ExportError::Failed(format!("view mark \"{}\" has no length", clean(&mark.label))));
    };
    let look = if mark.flip { -u.left() } else { u.left() };
    let frame = ViewFrame::horizontal(a, look);
    let far = mark.depth_mm.filter(|d| d.is_finite() && *d > 0.0);
    let opts = Options { near: Some(0.0), far, remove_hidden: true };
    // A section is cropped to its line.
    let crop = (mark.mark_kind == MarkKind::Section).then(|| {
        let s1 = (b - a).dot(v(frame.right.x, frame.right.y));
        (s1.min(0.0), s1.max(0.0))
    });
    Ok((frame, opts, crop))
}

/// The drawing title: "NORTH ELEVATION", "SECTION A-A".
pub fn title(mark: &ViewMark) -> String {
    let label = clean(&mark.label).to_uppercase();
    match mark.mark_kind {
        MarkKind::Elevation if label.is_empty() => "ELEVATION".into(),
        MarkKind::Elevation if label.ends_with("ELEVATION") => label,
        MarkKind::Elevation => format!("{label} ELEVATION"),
        MarkKind::Section if label.is_empty() => "SECTION".into(),
        MarkKind::Section if label.starts_with("SECTION") => label,
        MarkKind::Section => format!("SECTION {label}-{label}"),
    }
}

/// Entourage outlines in view space: a tree's canopy and trunk, a pot and
/// its foliage, a car's side or front.
pub fn figure_pieces(mesh: &Mesh, frame: &ViewFrame) -> Vec<Piece> {
    let mut out = Vec::new();
    for (fi, f) in mesh.figures.iter().enumerate() {
        let c = frame.to_view(f.base);
        let t = c.z;
        let mut segs: Vec<(V, V)> = Vec::new();
        // Its width as seen: the footprint turned into the view.
        let ax = dir(f.rotation_deg);
        let r2 = v(frame.right.x, frame.right.y);
        let cos = ax.dot(r2).abs();
        let sin = (1.0 - cos * cos).max(0.0).sqrt();
        let seen_w = f.width * cos + f.depth * sin;
        let (x, y0, h) = (c.x, c.y, f.height.max(100.0));
        let ring = |segs: &mut Vec<(V, V)>, pts: &[V]| {
            for i in 0..pts.len() {
                segs.push((pts[i], pts[(i + 1) % pts.len()]));
            }
        };
        match f.kind {
            FigureKind::Tree => {
                let r = (seen_w / 2.0).min(h * 0.4).max(200.0);
                let cy = y0 + h - r;
                let pts: Vec<V> = (0..28)
                    .map(|i| {
                        let a = 360.0 * i as f64 / 28.0;
                        // A slightly lumpy canopy reads as foliage.
                        let k = 1.0 + 0.05 * ((a * 5.0).to_radians().sin());
                        v(x, cy) + dir(a) * (r * k)
                    })
                    .collect();
                ring(&mut segs, &pts);
                let tw = (r * 0.08).max(60.0);
                let bottom = cy - r * 0.95;
                segs.push((v(x - tw, y0), v(x - tw * 0.7, bottom)));
                segs.push((v(x + tw, y0), v(x + tw * 0.7, bottom)));
            }
            FigureKind::Pot => {
                let w = seen_w.max(200.0);
                let ph = (h * 0.3).min(500.0);
                ring(&mut segs, &[v(x - w * 0.3, y0), v(x + w * 0.3, y0), v(x + w * 0.4, y0 + ph), v(x - w * 0.4, y0 + ph)]);
                let r = (w * 0.5).min((h - ph) * 0.5).max(100.0);
                let pts: Vec<V> = (0..20).map(|i| v(x, y0 + ph + r) + dir(270.0 + 360.0 * i as f64 / 20.0) * r).collect();
                for i in 0..pts.len() - 1 {
                    segs.push((pts[i], pts[i + 1]));
                }
            }
            FigureKind::Car => {
                let w = seen_w.max(500.0);
                let side = f.depth * sin >= f.width * cos;
                let (x0, x1) = (x - w / 2.0, x + w / 2.0);
                let hb = y0 + h * 0.55;
                let wheel = 320.0_f64.min(h * 0.25);
                if side {
                    // Body, cabin and two wheels.
                    ring(&mut segs, &[
                        v(x0, y0 + wheel), v(x0 + w * 0.02, hb), v(x0 + w * 0.25, hb + 40.0),
                        v(x0 + w * 0.36, y0 + h), v(x0 + w * 0.72, y0 + h), v(x0 + w * 0.84, hb),
                        v(x1, hb - 60.0), v(x1, y0 + wheel),
                    ]);
                    for cx in [x0 + w * 0.2, x0 + w * 0.8] {
                        let pts: Vec<V> = (0..16).map(|i| v(cx, y0 + wheel) + dir(22.5 * i as f64) * wheel).collect();
                        ring(&mut segs, &pts);
                    }
                } else {
                    ring(&mut segs, &[
                        v(x0, y0 + 150.0), v(x0, hb), v(x0 + w * 0.15, y0 + h), v(x1 - w * 0.15, y0 + h),
                        v(x1, hb), v(x1, y0 + 150.0),
                    ]);
                    segs.push((v(x0 + w * 0.05, y0), v(x0 + w * 0.05, y0 + 150.0)));
                    segs.push((v(x1 - w * 0.05, y0), v(x1 - w * 0.05, y0 + 150.0)));
                }
            }
        }
        for (a, b) in segs {
            out.push(Piece { a, b, ta: t, tb: t, solid: fi as u32, kind: EdgeKind::Figure });
        }
    }
    out
}

/// The drawing category of a part.
pub fn part_cat(part: Part) -> Cat {
    match part {
        Part::Wall => Cat::Wall,
        Part::Site => Cat::Site,
        Part::Door => Cat::Door,
        Part::Window => Cat::Glaz,
        Part::Slab => Cat::Slab,
        Part::Roof => Cat::Roof,
        Part::Column => Cat::Cols,
        Part::Stair => Cat::Strs,
        Part::Rail => Cat::Rail,
        Part::Object => Cat::Furn,
    }
}

fn pen_rank(p: Pen) -> u8 {
    match p {
        Pen::Heavy => 3,
        Pen::Medium => 2,
        Pen::Light => 1,
        Pen::Fine => 0,
    }
}

fn rank_pen(r: u8) -> Pen {
    match r {
        3 => Pen::Heavy,
        2 => Pen::Medium,
        1 => Pen::Light,
        _ => Pen::Fine,
    }
}

const CATS: [Cat; Cat::ALL.len()] = Cat::ALL;

fn cat_index(c: Cat) -> u8 {
    CATS.iter().position(|x| *x == c).unwrap_or(0) as u8
}

/// Build the items of an elevation or section from its hidden line result.
pub fn draw(p: &Project, d: &Derived, mark: &ViewMark, mesh: &Mesh, h: &Arc<Hlr>, n: f64) -> Result<ViewItems, ExportError> {
    let (frame, _opts, crop) = frame_of(mark)?;
    let section = mark.mark_kind == MarkKind::Section;
    let in_crop = |x: f64| crop.is_none_or(|(s0, s1)| x >= s0 - 1e-6 && x <= s1 + 1e-6);
    let mut items: Vec<Item> = Vec::new();

    // Poche, cropped.
    let mut extent = Bounds::empty();
    for c in &h.cuts {
        let part = mesh.solids.get(c.solid as usize).map(|s| s.part).unwrap_or(Part::Wall);
        let cat = part_cat(part);
        let mut pts = c.pts.clone();
        if let Some((s0, s1)) = crop {
            if c.closed {
                pts = clip_half_plane(&pts, v(0.0, 0.0), v(1.0, 0.0), s0, true);
                pts = clip_half_plane(&pts, v(0.0, 0.0), v(1.0, 0.0), s1, false);
            }
        }
        if c.closed && pts.len() >= 3 && polygon_area(&pts) > 1.0 {
            pts.iter().for_each(|q| extent.add(*q));
            items.push(Item { cat, pen: Pen::Heavy, prim: Prim::Poly { pts, closed: true, fill: Fill::Ink } });
        } else if !c.closed {
            for k in 0..pts.len().saturating_sub(1) {
                for (a, b) in crop_seg(pts[k], pts[k + 1], crop) {
                    extent.add(a);
                    extent.add(b);
                    items.push(line(cat, Pen::Heavy, a, b));
                }
            }
        }
    }

    // Visible edges with their pens.
    let front = h
        .visible
        .iter()
        .filter(|e| e.kind != EdgeKind::Figure)
        .map(|e| e.ta.min(e.tb))
        .fold(f64::INFINITY, f64::min);
    let mut weighted: Vec<(V, V, (u8, u8))> = Vec::new();
    for e in &h.visible {
        for (a, b) in crop_seg(e.a, e.b, crop) {
            let (pen, cat) = match e.kind {
                EdgeKind::Figure => (Pen::Fine, Cat::Furn),
                EdgeKind::Decal => {
                    let part = mesh.solids.get(e.solid as usize).map(|s| s.part).unwrap_or(Part::Window);
                    (Pen::Fine, part_cat(part))
                }
                _ => {
                    let part = mesh.solids.get(e.solid as usize).map(|s| s.part).unwrap_or(Part::Wall);
                    let mid = (a + b) * 0.5;
                    let t = (e.ta + e.tb) / 2.0;
                    let pen = if is_profile(&h.grid, a, b, mid) {
                        if section { Pen::Medium } else { Pen::Heavy }
                    } else if t - front < NEAR_MM && !section {
                        Pen::Medium
                    } else if t - front < FAR_MM {
                        Pen::Light
                    } else {
                        Pen::Fine
                    };
                    (pen, part_cat(part))
                }
            };
            if e.kind != EdgeKind::Figure {
                extent.add(a);
                extent.add(b);
            }
            weighted.push((a, b, (pen_rank(pen), cat_index(cat))));
        }
    }
    for (a, b, (pr, ci)) in hlr::merge_collinear(&weighted) {
        items.push(line(CATS[ci as usize], rank_pen(pr), a, b));
    }

    if extent.is_empty() {
        return Err(ExportError::Empty(format!("{} shows nothing: the mark looks away from the model", title(mark).to_lowercase())));
    }

    // The finish grade, past the drawing on both sides.
    let lowest = p.levels.iter().map(|l| l.elevation_mm).fold(f64::INFINITY, f64::min);
    let lowest = if lowest.is_finite() { lowest } else { 0.0 };
    let grade = lowest - super::mesh::grade_below(p);
    let (gx0, gx1) = match crop {
        Some((s0, s1)) => (s0, s1),
        None => (extent.min.x - 1500.0, extent.max.x + 1500.0),
    };
    items.push(line(Cat::Site, Pen::Heavy, v(gx0, grade), v(gx1, grade)));

    // Grids that cross the picture plane, with bubbles on top.
    let top = extent.max.y;
    if crate::plan::layer_visible(p, LayerKey::Grids) {
        let look = v(frame.dir.x, frame.dir.y);
        let (o, right) = (v(frame.origin.x, frame.origin.y), v(frame.right.x, frame.right.y));
        for e in &p.elements {
            let Element::Grid(g) = e else { continue };
            let (ga, gb) = (V::from(g.start), V::from(g.end));
            let Some(gu) = (gb - ga).unit() else { continue };
            if gu.dot(look).abs() < 0.7 {
                continue;
            }
            // Where the grid's line meets the picture plane.
            let denom = gu.dot(look);
            let t = (o - ga).dot(look) / denom;
            let x = (ga + gu * t - o).dot(right);
            if !in_crop(x) || x < extent.min.x - 3000.0 || x > extent.max.x + 3000.0 {
                continue;
            }
            let y0 = grade - 2.0 * n;
            let y1 = top + 8.0 * n;
            items.extend(annot::dashed(Cat::Grid, Pen::Fine, v(x, y0), v(x, y1), &annot::scaled(&paper::CENTER, n)));
            items.extend(annot::bubble(Cat::Grid, v(x, y1 + paper::BUBBLE_R * n), &g.label, n));
        }
    }

    // Level datums at the right, then the grade's.
    let x0 = match crop {
        Some((_, s1)) => s1 + 3.0 * n,
        None => extent.max.x + 3.0 * n,
    };
    let mut levels: Vec<&guhit_model::Level> = p.levels.iter().collect();
    levels.sort_by(|a, b| a.elevation_mm.partial_cmp(&b.elevation_mm).unwrap_or(std::cmp::Ordering::Equal));
    let mut last_y = f64::NEG_INFINITY;
    for l in &levels {
        let y = l.elevation_mm;
        if y - last_y < 3.5 * n {
            continue;
        }
        last_y = y;
        let label = format!("{} {}", annot::level_text(y), clean(&l.name).to_uppercase());
        items.extend(annot::datum(Cat::Grid, x0, y, &label, n, false));
    }
    let first = levels.first().map(|l| l.elevation_mm).unwrap_or(0.0);
    if (grade - first).abs() >= 1.0 {
        // Close under the first floor, its label goes under the line.
        let below = first - grade < 3.5 * n;
        items.extend(annot::datum(Cat::Grid, x0, grade, &format!("{} FINISH GRADE", annot::level_text(grade)), n, below));
    }
    // The top of the roof: the building's, the same on every view, never
    // the highest roof edge this view happens to show.
    if let Some(top) = roof_top(p, d, mesh).filter(|t| t - last_y >= 3.5 * n) {
        items.extend(annot::datum(Cat::Grid, x0, top, &format!("{} TOP OF ROOF", annot::level_text(top)), n, false));
    }

    if !section {
        finish_notes(p, mesh, h, &extent, n, &mut items);
    }

    // Door and window marks where hidden line removal leaves the opening
    // in view: an edge of its leaf or sash is visible (a door straight
    // behind the front door has none), or for a cased opening without a
    // leaf, a point of its face.
    let marks: std::collections::HashMap<&str, &str> = d
        .opening_schedule
        .marks
        .iter()
        .map(|m| (m.opening_id.as_str(), m.mark.as_str()))
        .collect();
    if !marks.is_empty() && crate::plan::layer_visible(p, LayerKey::Openings) {
        let leaves: std::collections::HashMap<&str, u32> = mesh
            .solids
            .iter()
            .enumerate()
            .filter(|(_, s)| matches!(s.part, Part::Door | Part::Window))
            .map(|(i, s)| (s.element_id.as_str(), i as u32))
            .collect();
        let shown: std::collections::HashSet<u32> = h
            .visible
            .iter()
            .filter(|e| e.kind != EdgeKind::Figure && !crop_seg(e.a, e.b, crop).is_empty())
            .map(|e| e.solid)
            .collect();
        let look = super::mesh::p3(frame.dir.x, frame.dir.y, 0.0);
        for o in &mesh.openings {
            let Some(mark_text) = marks.get(o.opening_id.as_str()) else { continue };
            if mark_text.trim().is_empty() {
                continue;
            }
            let facing = o.normal.dot(look);
            if facing.abs() < 0.3 {
                continue;
            }
            let side = if facing < 0.0 { 1.0 } else { -1.0 };
            let at = frame.to_view(o.center + o.normal * (o.half_t * side));
            if at.z < 0.0 || !in_crop(at.x) || mark.depth_mm.is_some_and(|f| f > 0.0 && at.z > f) {
                continue;
            }
            let q = v(at.x, at.y);
            let seen = match leaves.get(o.opening_id.as_str()) {
                Some(solid) => shown.contains(solid),
                None => {
                    // Off the middle too: a point on a face's diagonal is in
                    // neither of its triangles.
                    let along = super::mesh::p3(o.normal.y, -o.normal.x, 0.0);
                    let face = o.center + o.normal * (o.half_t * side);
                    [(0.0, 0.0), (-0.3, -0.3), (0.3, -0.3), (0.3, 0.3), (-0.3, 0.3)].iter().any(|(fx, fy)| {
                        let pt = frame.to_view(face + along * (o.width * fx) + super::mesh::p3(0.0, 0.0, o.height * fy));
                        h.grid.depth_at(v(pt.x, pt.y)).is_none_or(|front| front >= pt.z - 5.0)
                    })
                }
            };
            if !seen {
                continue;
            }
            let window = o.kind == guhit_model::OpeningType::Window;
            items.extend(annot::opening_tag(Cat::Symb, q, mark_text, window, n));
        }
    }

    // Room names last, clear of the door, window and stair lines and tags.
    if section {
        room_labels(p, d, mark, &frame, crop, n, &mut items);
    }

    items.retain(|i| match &i.prim {
        Prim::Line { a, b } => a.is_finite() && b.is_finite(),
        _ => true,
    });
    let bounds = crate::plan::items_bounds(&items);
    Ok(ViewItems { items, bounds, title: title(mark), scale_denominator: n as u32, images: Vec::new() })
}

/// The highest point of every roof, mm above project zero: from
/// `Derived::roofs`, else from the roof faces of the mesh (a Wave 1 roof the
/// engine has not drawn). None without a roof or with the Roofs layer off.
pub fn roof_top(p: &Project, d: &Derived, mesh: &Mesh) -> Option<f64> {
    if !crate::plan::layer_visible(p, LayerKey::Roofs) {
        return None;
    }
    let top = if d.roofs.is_empty() {
        mesh.faces
            .iter()
            .filter(|f| mesh.solids.get(f.solid as usize).is_some_and(|s| s.part == Part::Roof))
            .flat_map(|f| f.pts.iter().map(|q| q.z))
            .fold(f64::NEG_INFINITY, f64::max)
    } else {
        d.roofs
            .iter()
            .flat_map(|g| {
                let e = p.levels.iter().find(|l| l.id == g.level_id).map(|l| l.elevation_mm).unwrap_or(0.0);
                g.planes.iter().flatten().map(move |q| e + q.z)
            })
            .fold(f64::NEG_INFINITY, f64::max)
    };
    top.is_finite().then_some(top)
}

/// The rooms a section cuts (GA P1-3): each one's name over the stretch of
/// the cut where it has floor (never over its void, which belongs to the
/// double height below), and its ceiling as a light line at the height the
/// room schedule gives with one "CH 2.800" note inside that stretch, as the
/// reflected ceiling plan prints it. Names and notes keep clear of each
/// other and of what is drawn in the room.
fn room_labels(p: &Project, d: &Derived, mark: &ViewMark, frame: &ViewFrame, crop: Option<(f64, f64)>, n: f64, items: &mut Vec<Item>) {
    if !crate::plan::layer_visible(p, LayerKey::Rooms) {
        return;
    }
    let (a, b) = (V::from(mark.start), V::from(mark.end));
    let s_of = |q: V| frame.to_view(super::mesh::lift(q, 0.0)).x;
    let (th, small) = (paper::LABEL * n, paper::SMALL * n);
    let mut taken: Vec<Bounds> = items
        .iter()
        .filter(|i| matches!(i.cat, Cat::Door | Cat::Glaz | Cat::Symb | Cat::Strs | Cat::Rail | Cat::Furn | Cat::Text))
        .map(|i| crate::plan::items_bounds(std::slice::from_ref(i)))
        .filter(|b| !b.is_empty())
        .collect();
    let free = |taken: &[Bounds], bx: &Bounds| !taken.iter().any(|o| o.intersects(bx));
    for row in &d.room_schedule {
        let Some(g) = d.rooms.iter().find(|g| g.room_id == row.room_id) else { continue };
        let Some(level) = p.levels.iter().find(|l| l.id == row.level_id) else { continue };
        let poly: Vec<V> = g.polygon.iter().map(V::from).collect();
        if poly.len() < 3 {
            continue;
        }
        let voids: Vec<Vec<V>> = p
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Slab(s) if s.kind == guhit_model::SlabKind::Void && s.level_id == row.level_id && s.polygon.len() >= 3 => Some(s.polygon.iter().map(V::from).collect()),
                _ => None,
            })
            .collect();
        // The stretches of the cut inside the room with floor under them,
        // as s ranges; the longest one carries the room.
        let mut pieces: Vec<(f64, f64)> = Vec::new();
        for (q0, q1) in clip_segment(a, b, &poly) {
            let (x0, x1) = (s_of(q0), s_of(q1));
            let mut keep = vec![(x0.min(x1), x0.max(x1))];
            for vd in &voids {
                for (v0, v1) in clip_segment(q0, q1, vd) {
                    let (c0, c1) = (s_of(v0).min(s_of(v1)), s_of(v0).max(s_of(v1)));
                    keep = keep.into_iter().flat_map(|(k0, k1)| [(k0, k1.min(c0)), (k0.max(c1), k1)]).filter(|(k0, k1)| k1 - k0 > 1.0).collect();
                }
            }
            pieces.extend(keep);
        }
        let Some((s0, s1)) = pieces
            .into_iter()
            .map(|(s0, s1)| match crop {
                Some((c0, c1)) => (s0.max(c0), s1.min(c1)),
                None => (s0, s1),
            })
            .max_by(|x, y| (x.1 - x.0).partial_cmp(&(y.1 - y.0)).unwrap_or(std::cmp::Ordering::Equal))
        else {
            continue;
        };
        if s1 - s0 < 300.0 {
            continue;
        }
        let e = level.elevation_mm;
        let ch = Some(row.ceiling_height_mm).filter(|c| c.is_finite() && *c > 100.0);
        if let Some(ch) = ch {
            items.push(line(Cat::Slab, Pen::Light, v(s0, e + ch), v(s1, e + ch)));
            // One note, under the line inside the room's stretch: at its
            // start, its end or its middle, the first clear of other notes.
            let label = format!("CH {:.3}", ch / 1000.0);
            let w = crate::text::est_width(&label, small, false);
            if w < (s1 - s0) * 0.9 {
                let y = e + ch - small * 1.4;
                let spots = [s0 + 1.0 * n, s1 - w - 1.0 * n, (s0 + s1 - w) / 2.0];
                let boxed = |x: f64, y: f64| Bounds { min: v(x - 0.3 * n, y - 0.4 * small), max: v(x + w + 0.3 * n, y + 1.1 * small) };
                let spot = spots.iter().flat_map(|x| [(*x, y), (*x, y - small * 1.6)]).find(|(x, y)| free(&taken, &boxed(*x, *y)));
                if let Some((x, y)) = spot {
                    taken.push(boxed(x, y));
                    items.push(annot::text(Cat::Text, v(x, y), small, 0.0, crate::plan::HAlign::Start, &label, false));
                }
            }
        }
        let name = clean(&row.name).to_uppercase();
        if name.is_empty() {
            continue;
        }
        let w = crate::text::est_width(&name, th, true);
        let size = if w > (s1 - s0) * 0.9 { th * (s1 - s0) * 0.9 / w } else { th };
        if size < th * 0.6 {
            continue;
        }
        // The first height, from about mid height, where the name sits
        // clear of what is drawn in the room.
        let h_room = ch.unwrap_or(level.height_mm);
        let x = (s0 + s1) / 2.0;
        let hw = crate::text::est_width(&name, size, true) / 2.0 + 0.5 * n;
        let boxed = |y: f64| Bounds { min: v(x - hw, y - size * 0.5), max: v(x + hw, y + size * 1.2) };
        let y = [0.45, 0.3, 0.6, 0.18, 0.75].iter().map(|f| e + h_room * f).find(|y| free(&taken, &boxed(*y))).unwrap_or(e + h_room * 0.45);
        taken.push(boxed(y));
        items.push(annot::centered(Cat::Text, v(x, y), size, 0.0, &name, true));
    }
}

/// An elevation's finish notes (GA P1-3): each material of the walls and
/// roofs the view shows, once, at the left of the drawing with a leader to a
/// visible face of it. A wall or roof with no material set gets none.
fn finish_notes(p: &Project, mesh: &Mesh, h: &Hlr, extent: &Bounds, n: f64, items: &mut Vec<Item>) {
    let material_of = |solid: u32| -> Option<String> {
        let s = mesh.solids.get(solid as usize)?;
        let id = p.elements.iter().find_map(|e| match (e, s.part) {
            (Element::Wall(w), Part::Wall) if w.id == s.element_id => w.material_id.clone(),
            (Element::Roof(r), Part::Roof) if r.id == s.element_id => r.material_id.clone(),
            _ => None,
        })?;
        let name = clean(&p.materials.iter().find(|m| m.id == id)?.name).to_uppercase();
        (!name.is_empty()).then_some(name)
    };
    // Per material, a point on a visible face next to its longest visible
    // edge: just above or below it, where that solid is front-most.
    let mut notes: Vec<(String, V, f64)> = Vec::new();
    for e in h.visible.iter().filter(|e| matches!(e.kind, EdgeKind::Crease | EdgeKind::Silhouette | EdgeKind::Boundary)) {
        let Some(name) = material_of(e.solid) else { continue };
        let len = (e.b - e.a).len();
        if notes.iter().any(|x| x.0 == name && x.2 >= len) {
            continue;
        }
        let mid = (e.a + e.b) * 0.5;
        let Some(at) = [v(0.0, -300.0), v(0.0, 300.0), v(300.0, 0.0), v(-300.0, 0.0)]
            .into_iter()
            .map(|o| mid + o)
            .find(|q| h.grid.solid_at(*q).is_some_and(|s| material_of(s).as_deref() == Some(name.as_str())))
        else {
            continue;
        };
        notes.retain(|x| x.0 != name);
        notes.push((name, at, len));
    }
    if notes.is_empty() {
        return;
    }
    notes.sort_by(|a, b| b.1.y.partial_cmp(&a.1.y).unwrap_or(std::cmp::Ordering::Equal));
    let th = paper::SMALL * n;
    // Left of the grade line's end, stacked down from the highest.
    let x_text = extent.min.x - 1500.0 - 3.0 * n;
    let mut last = f64::INFINITY;
    for (name, at, _) in notes {
        let y = at.y.min(last - th * 2.2);
        last = y;
        let w = crate::text::est_width(&name, th, false);
        items.push(annot::text(Cat::Text, v(x_text - w - 1.0 * n, y - th * 0.35), th, 0.0, crate::plan::HAlign::Start, &name, false));
        items.push(line(Cat::Symb, Pen::Fine, v(x_text, y), at));
        items.push(annot::circle(Cat::Symb, Pen::Fine, at, 0.45 * n, Fill::Ink));
    }
}

/// A visible line is on the profile when open sky is right beside it.
fn is_profile(grid: &hlr::Grid, a: V, b: V, mid: V) -> bool {
    let Some(u) = (b - a).unit() else { return false };
    let side = u.left() * PROFILE_PROBE_MM;
    grid.depth_at(mid + side).is_none() || grid.depth_at(mid - side).is_none()
}

/// The segment cropped to the s range.
fn crop_seg(a: V, b: V, crop: Option<(f64, f64)>) -> Vec<(V, V)> {
    let Some((s0, s1)) = crop else { return vec![(a, b)] };
    let (mut u0, mut u1) = (0.0_f64, 1.0_f64);
    let dx = b.x - a.x;
    if dx.abs() < 1e-12 {
        if a.x < s0 - 1e-6 || a.x > s1 + 1e-6 {
            return vec![];
        }
    } else {
        let (t0, t1) = ((s0 - a.x) / dx, (s1 - a.x) / dx);
        u0 = u0.max(t0.min(t1));
        u1 = u1.min(t0.max(t1));
    }
    if u1 - u0 <= 1e-9 {
        return vec![];
    }
    vec![(a + (b - a) * u0, a + (b - a) * u1)]
}
