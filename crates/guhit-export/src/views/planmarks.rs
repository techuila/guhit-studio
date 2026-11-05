//! What the floor plan draws for the permit set (DECISIONS D53, D54): door
//! and window type tags, grid lines with their bubbles, dimension strings,
//! and section and elevation marks. `plan::build_items` calls these, so
//! every plan sheet, the plan views of the set and the plan DXF show them.

use std::collections::HashMap;

use guhit_model::{Derived, Element, LayerKey, Level, MarkKind, OpeningType, Project, Wall};

use crate::geom::*;
use crate::plan::{line, paper as plan_paper, Cat, Fill, HAlign, Item, Pen, Prim};
use crate::text::{clean, est_width, format_length};

use super::annot::{self, paper};

/// Door and window tags: the mark ("D1", "W2") in a circle for a door or a
/// hexagon for a window, just off the wall face; a door's on the side its
/// leaf does not swing to, a window's on the outside.
pub fn type_tags(p: &Project, d: &Derived, level: &Level, n: f64, out: &mut Vec<Item>) {
    if d.opening_schedule.marks.is_empty() || !crate::plan::layer_visible(p, LayerKey::Openings) {
        return;
    }
    let marks: HashMap<&str, &str> = d.opening_schedule.marks.iter().map(|m| (m.opening_id.as_str(), m.mark.as_str())).collect();
    let walls: HashMap<&str, &Wall> = p
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) if w.level_id == level.id => Some((w.id.as_str(), w)),
            _ => None,
        })
        .collect();
    let room_ids: Vec<&str> = p
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Room(r) if r.level_id == level.id => Some(r.id.as_str()),
            _ => None,
        })
        .collect();
    let rooms: Vec<Vec<V>> = d
        .rooms
        .iter()
        .filter(|g| room_ids.contains(&g.room_id.as_str()))
        .map(|g| g.polygon.iter().map(V::from).collect())
        .collect();
    let inside = |q: V| rooms.iter().any(|r| r.len() >= 3 && point_in_polygon(q, r));
    for e in &p.elements {
        let Element::Opening(o) = e else { continue };
        let Some(mark) = marks.get(o.id.as_str()).map(|m| m.trim()).filter(|m| !m.is_empty()) else { continue };
        let Some(w) = walls.get(o.wall_id.as_str()) else { continue };
        let (s, e2) = (V::from(w.start), V::from(w.end));
        let Some(u) = (e2 - s).unit() else { continue };
        let c = s + u * o.offset_mm;
        let left = u.left();
        let t = w.thickness_mm.abs() / 2.0;
        let r = paper::TAG_R * n;
        let gap = t + r + 1.2 * n;
        let side = match o.opening_type {
            // The leaf swings to the left unless flipped: the tag goes the
            // other way.
            OpeningType::Door => {
                if o.flip_side {
                    1.0
                } else {
                    -1.0
                }
            }
            OpeningType::Window => {
                let probe = 600.0 + t;
                match (inside(c + left * probe), inside(c - left * probe)) {
                    (true, false) => -1.0,
                    (false, true) => 1.0,
                    _ => -1.0,
                }
            }
        };
        let at = c + left * (gap * side);
        out.extend(annot::opening_tag(Cat::Symb, at, mark, o.opening_type == OpeningType::Window, n));
    }
}

/// Grid lines: a chain line from start to end with a bubble past each end.
pub fn grids(p: &Project, n: f64, out: &mut Vec<Item>) {
    if !crate::plan::layer_visible(p, LayerKey::Grids) {
        return;
    }
    let r = paper::BUBBLE_R * n;
    for e in &p.elements {
        let Element::Grid(g) = e else { continue };
        let (a, b) = (V::from(g.start), V::from(g.end));
        let Some(u) = (b - a).unit() else { continue };
        out.extend(annot::dashed(Cat::Grid, Pen::Fine, a, b, &annot::scaled(&paper::CENTER, n)));
        for (end, dirn) in [(a, -u), (b, u)] {
            out.extend(annot::bubble(Cat::Grid, end + dirn * r, &g.label, n));
        }
    }
}

/// A dimension string: a, through..., b along the a to b direction at
/// `offset`, one figure per segment, extension lines from each witness
/// point. A figure too long for its segment steps up a line.
pub fn dimension_string(a: V, through: &[V], b: V, offset: f64, meters: bool, n: f64, out: &mut Vec<Item>) {
    let Some(u) = (b - a).unit() else { return };
    let nrm = u.left();
    let offset = if offset.is_finite() { offset } else { 0.0 };
    let sgn = if offset >= 0.0 { 1.0 } else { -1.0 };
    let mut pts: Vec<V> = std::iter::once(a).chain(through.iter().copied()).chain(std::iter::once(b)).collect();
    pts.retain(|q| q.is_finite());
    // Along the line, in order.
    let mut ts: Vec<(f64, V)> = pts.iter().map(|q| ((*q - a).dot(u), *q)).collect();
    ts.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
    ts.dedup_by(|x, y| (x.0 - y.0).abs() < 0.5);
    if ts.len() < 2 {
        return;
    }
    let base = a + nrm * offset;
    let foot = |t: f64| base + u * t;
    let gap = plan_paper::DIM_EXT_GAP * n;
    let over = plan_paper::DIM_EXT_OVERSHOOT * n;
    for (t, q) in &ts {
        let f = foot(*t);
        // From near the witness point to past the dimension line.
        let along = (f - *q).dot(nrm) * sgn;
        let start = if along > gap { *q + nrm * (gap * sgn) } else { *q };
        out.push(line(Cat::Dims, Pen::Light, start, f + nrm * (over * sgn)));
    }
    let lo = plan_paper::DIM_LINE_OVERSHOOT * n;
    let (t0, t1) = (ts[0].0, ts[ts.len() - 1].0);
    out.push(line(Cat::Dims, Pen::Light, foot(t0) - u * lo, foot(t1) + u * lo));
    let tick = u.rotated(45.0) * (plan_paper::DIM_TICK * n / 2.0);
    for (t, _) in &ts {
        let f = foot(*t);
        out.push(line(Cat::Dims, Pen::Medium, f - tick, f + tick));
    }
    let rot = readable_deg(u.angle_deg());
    let up = dir(rot + 90.0);
    let th = plan_paper::DIM_TEXT * n;
    let mut raised = false;
    for k in 0..ts.len() - 1 {
        let (s0, s1) = (ts[k].0, ts[k + 1].0);
        let label = format_length(s1 - s0, meters);
        let fits = est_width(&label, th, false) <= (s1 - s0) * 0.95;
        let lift = if fits {
            raised = false;
            0.0
        } else {
            raised = !raised;
            if raised { th * 1.25 } else { 0.0 }
        };
        let mid = foot((s0 + s1) / 2.0);
        out.push(Item {
            cat: Cat::Dims,
            pen: Pen::Light,
            prim: Prim::Text { pos: mid + up * (plan_paper::DIM_TEXT_GAP * n + lift), height: th, rot_deg: rot, align: HAlign::Middle, text: label, bold: false },
        });
    }
}

/// Section and elevation marks drawn on their level.
pub fn view_marks(p: &Project, level: &Level, n: f64, out: &mut Vec<Item>) {
    if !crate::plan::layer_visible(p, LayerKey::Annotations) {
        return;
    }
    let r = paper::BUBBLE_R * n;
    // Grid bubbles, which a mark's bubble moves out along its line to clear.
    let mut taken: Vec<V> = Vec::new();
    if crate::plan::layer_visible(p, LayerKey::Grids) {
        for e in &p.elements {
            let Element::Grid(g) = e else { continue };
            let (a, b) = (V::from(g.start), V::from(g.end));
            if let Some(u) = (b - a).unit() {
                taken.extend([a - u * r, b + u * r]);
            }
        }
    }
    let clear = |c: V, away: V, taken: &[V]| -> V {
        let mut c = c;
        for _ in 0..40 {
            if !taken.iter().any(|t| (*t - c).len() < 2.0 * r + 0.5 * n) {
                break;
            }
            c = c + away * (r * 0.5);
        }
        c
    };
    for e in &p.elements {
        let Element::ViewMark(m) = e else { continue };
        if m.level_id != level.id {
            continue;
        }
        let (a, b) = (V::from(m.start), V::from(m.end));
        let Some(u) = (b - a).unit() else { continue };
        let look = if m.flip { -u.left() } else { u.left() };
        let full = clean(&m.label);
        // A bubble holds a short code: "A", "1", or the first letter of
        // "North" with the whole name written under the bubble.
        let short = full.chars().count() <= 3;
        let label: String = if short { full.clone() } else { full.chars().take(1).collect::<String>().to_uppercase() };
        let name_under = |out: &mut Vec<Item>, c: V| {
            if !short {
                let th = paper::SMALL * n;
                out.push(annot::centered(Cat::Symb, c - v(0.0, r + th * 1.1), th, 0.0, &full.to_uppercase(), false));
            }
        };
        match m.mark_kind {
            MarkKind::Section => {
                // The cut: a chain line, heavy at its ends, a bubble and an
                // arrow toward the look at each end.
                out.extend(annot::dashed(Cat::Symb, Pen::Fine, a, b, &annot::scaled(&paper::CENTER, n)));
                let stub = (6.0 * n).min((b - a).len() / 3.0);
                out.push(line(Cat::Symb, Pen::Heavy, a, a + u * stub));
                out.push(line(Cat::Symb, Pen::Heavy, b - u * stub, b));
                // The bubble gives where the section is drawn ("1" over
                // "A-6"), never a lone letter a grid could have; a section
                // on no sheet yet shows its label over an empty half.
                let (top, bottom) = section_ref(p, &m.id).unwrap_or_else(|| (label.clone(), String::new()));
                for (end, outward) in [(a, -u), (b, u)] {
                    let c = clear(end + outward * r, outward, &taken);
                    if (c - end).len() > r * 1.01 {
                        // Moved clear of a grid bubble: the line runs on to it.
                        out.push(line(Cat::Symb, Pen::Fine, end, c - outward * r));
                    }
                    out.extend(annot::split_bubble(Cat::Symb, c, &top, &bottom, n));
                    out.push(arrow(c, look, r));
                    taken.push(c);
                }
            }
            MarkKind::Elevation => {
                // A bubble on the viewer's side of the middle, its arrow
                // pointing at what it looks at, moved back from the line
                // until it clears the grid bubbles.
                let mid = (a + b) * 0.5;
                let c = clear(mid - look * (r * 1.2), -look, &taken);
                taken.push(c);
                out.extend(annot::bubble(Cat::Symb, c, &label, n));
                out.push(arrow(c, look, r));
                name_under(out, c);
            }
        }
    }
}

/// Where a section is drawn: its viewport's number on the sheet and the
/// sheet's number ("1", "A-6"), from the set, or the default set an export
/// prints when the project has none. None when no sheet shows it.
fn section_ref(p: &Project, mark_id: &str) -> Option<(String, String)> {
    let set = if p.sheet_set.sheets.is_empty() { crate::sheetset::default_set(p, p.settings.paper) } else { p.sheet_set.sheets.clone() };
    set.iter().find_map(|s| {
        let j = s.viewports.iter().position(|vp| matches!(&vp.view, guhit_model::ViewRef::Section { mark_id: m } if m == mark_id))?;
        Some(((j + 1).to_string(), clean(&s.number)))
    })
}

/// A filled arrow head on a bubble of radius `r` at `c`, toward `look`.
fn arrow(c: V, look: V, r: f64) -> Item {
    let side = look.left();
    let tip = c + look * (r * 1.75);
    // Tangent points where the head meets the circle.
    let a = c + look * (r * 0.55) + side * (r * 0.84);
    let b = c + look * (r * 0.55) - side * (r * 0.84);
    Item { cat: Cat::Symb, pen: Pen::Light, prim: Prim::Poly { pts: vec![a, tip, b], closed: true, fill: Fill::Ink } }
}

/// G4 problem 11: a stair label ("UP", "DN") under a wall's poche cannot
/// be read. Move each one that overlaps a wall, along its own up direction
/// first, to the nearest spot clear of every wall.
pub fn clear_stair_labels(items: &mut [Item]) {
    let walls: Vec<Vec<V>> = items
        .iter()
        .filter(|i| matches!(i.cat, Cat::Wall | Cat::Site | Cat::Cols))
        .filter_map(|i| match &i.prim {
            Prim::Poly { pts, closed: true, fill: Fill::Ink | Fill::Tint(_) } if pts.len() >= 3 => Some(pts.clone()),
            _ => None,
        })
        .collect();
    if walls.is_empty() {
        return;
    }
    // The flights: a label is best inside its own stair.
    let flights: Vec<Vec<V>> = items
        .iter()
        .filter(|i| matches!(i.cat, Cat::Strs | Cat::StrsBelow))
        .filter_map(|i| match &i.prim {
            Prim::Poly { pts, closed: true, fill: Fill::None } if pts.len() >= 3 => Some(pts.clone()),
            _ => None,
        })
        .collect();
    let hits = |b: &Bounds| {
        let quad = [b.min, v(b.max.x, b.min.y), b.max, v(b.min.x, b.max.y)];
        walls.iter().any(|w| bounds_of(w).intersects(b) && overlap_area(&quad, w) > 1.0)
    };
    for it in items.iter_mut() {
        if !matches!(it.cat, Cat::Strs | Cat::StrsBelow) {
            continue;
        }
        let Prim::Text { pos, height, rot_deg, .. } = &it.prim else { continue };
        let (pos, h, rot) = (*pos, *height, *rot_deg);
        let b = crate::plan::items_bounds(std::slice::from_ref(it));
        if !hits(&b) {
            continue;
        }
        let up = dir(rot + 90.0);
        let along = dir(rot);
        let mut best: Option<V> = None;
        // First a spot inside a flight, then any spot clear of the walls.
        'search: for in_flight in [true, false] {
            for k in 1..=8 {
                let step = h * 0.6 * k as f64;
                for d in [up * step, -up * step, along * step, -along * step] {
                    let mut moved = b;
                    moved.min = moved.min + d;
                    moved.max = moved.max + d;
                    if hits(&moved) {
                        continue;
                    }
                    if in_flight && !flights.iter().any(|f| point_in_polygon(moved.center(), f)) {
                        continue;
                    }
                    best = Some(pos + d);
                    break 'search;
                }
            }
        }
        if let (Some(np), Prim::Text { pos, .. }) = (best, &mut it.prim) {
            *pos = np;
        }
    }
}

/// The fence pens (G4 problem 11): a site wall prints as a light outline
/// over a grey band, never as heavy as the house walls.
pub const SITE_WALL_TINT: u8 = 40;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dimension_string_has_a_figure_per_segment() {
        let mut out = Vec::new();
        dimension_string(v(0.0, 0.0), &[v(1000.0, 0.0), v(4000.0, 0.0)], v(5000.0, 0.0), 800.0, false, 100.0, &mut out);
        let texts: Vec<String> = out
            .iter()
            .filter_map(|i| match &i.prim {
                Prim::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["1000", "3000", "1000"]);
        // Four extension lines, the dimension line, four ticks.
        assert_eq!(out.iter().filter(|i| matches!(i.prim, Prim::Line { .. })).count(), 4 + 1 + 4);
    }
}
