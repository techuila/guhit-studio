//! The plumbing isometric sheet: a water diagram (cold and hot) and a
//! sanitary diagram (drainage and vent) from the pipe centerlines.
//!
//! Plan x runs at 30 degrees, plan y at 150 degrees, height straight up.
//! Every level goes in at world height (level elevation plus the point's z),
//! so risers through floors read as one stack; the diagrams always show the
//! whole building. Lengths along the three axes keep the sheet scale, but
//! the diagrams are marked "not to scale", the PH custom for plumbing
//! isometrics.
//!
//! Every run carries its size and material ("20 PPR"), run ends at a
//! fixture carry the fixture tag ("WC"), vertical segments from 500 mm carry
//! a riser tag ("CWR-1", "SS-1", "VS-1"), drains carry flow arrows, and where
//! two runs cross without joining, the rear one is broken. Labels are placed
//! greedily: the first spot that hits no line, arrow or other label.
//!
//! Guhit does not size pipes: sizes are as drawn. The Master Plumber block
//! stays blank.

use std::collections::HashMap;

use guhit_model::{Asset, Derived, Element, Level, Pipe, PipeSystem, PlanExportOptions, Project};

use crate::geom::*;
use crate::panel::{Block, Content, LegendRow, Sample};
use crate::pipes::{self, v3, Dash, V3};
use crate::plan::{items_bounds, Cat, Item, Pen, Prim};
use crate::service_sheet::place_panel;
use crate::services;
use crate::sheet::{
    dash_attr, furniture, line_cap, num, pick_scale, write_item_in, Frame, Layout, North, Svg,
    FONT_FAMILY, INK, MUTED, PLAN_INK,
};
use crate::text::{clean, est_width, fit, xml_escape};
use crate::ExportError;

const C30: f64 = 0.866_025_403_784_438_6;

/// Diagram coordinates of a world point, mm: x to the right, y up.
pub fn to_iso(p: V3) -> V {
    v((p.x - p.y) * C30, (p.x + p.y) * 0.5 + p.z)
}

/// Distance from the viewer, who looks down from the front: larger is
/// further back.
pub fn depth(p: V3) -> f64 {
    p.x + p.y - p.z
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Water,
    Sanitary,
}

impl Which {
    pub fn systems(self) -> [PipeSystem; 2] {
        match self {
            Which::Water => [PipeSystem::ColdWater, PipeSystem::HotWater],
            Which::Sanitary => [PipeSystem::Drainage, PipeSystem::Vent],
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Which::Water => "WATER DISTRIBUTION ISOMETRIC",
            Which::Sanitary => "SANITARY DRAINAGE AND VENT ISOMETRIC",
        }
    }

    fn empty(self) -> &'static str {
        match self {
            Which::Water => "No cold or hot water runs in the model.",
            Which::Sanitary => "No drainage or vent runs in the model.",
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Which::Water => "iso-water",
            Which::Sanitary => "iso-sanitary",
        }
    }
}

/// Line style on the diagrams: water and drainage solid, vent dashed.
pub fn iso_dash(system: PipeSystem) -> Dash {
    match system {
        PipeSystem::Vent => Dash::Dashed,
        _ => Dash::Solid,
    }
}

/// Line weight on paper by pipe size: 0.35 mm for 20 mm, heavier for
/// drains, at most 0.7 mm.
pub fn iso_width(d: f64) -> f64 {
    (0.35 + (d - 20.0).max(0.0) * 0.0035).min(0.7)
}

/// Riser tag prefix of a vertical segment.
pub fn riser_prefix(system: PipeSystem, d: f64) -> Option<&'static str> {
    match system {
        PipeSystem::ColdWater => Some("CWR"),
        PipeSystem::HotWater => Some("HWR"),
        PipeSystem::Drainage if d >= 100.0 => Some("SS"),
        PipeSystem::Drainage => Some("WS"),
        PipeSystem::Vent => Some("VS"),
        _ => None,
    }
}

pub fn riser_meaning(prefix: &str) -> &'static str {
    match prefix {
        "CWR" => "Cold water riser",
        "HWR" => "Hot water riser",
        "SS" => "Soil stack, 100 mm and up",
        "WS" => "Waste stack",
        _ => "Vent stack",
    }
}

/// Shortest vertical segment that gets a riser tag, mm.
pub const RISER_TAG_MIN: f64 = 500.0;

/// A run end this close to a fixture's footprint (plan mm) serves it.
const FIXTURE_REACH: f64 = 150.0;

/// One run of a diagram, at world height.
pub struct Run<'a> {
    pub pipe: &'a Pipe,
    pub level: &'a Level,
    pub pts: Vec<V3>,
}

/// The runs of one diagram on visible layers, every level, in element order.
pub fn runs(project: &Project, which: Which) -> Vec<Run<'_>> {
    project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Pipe(p) if which.systems().contains(&p.system) => Some(p),
            _ => None,
        })
        .filter(|p| crate::plan::layer_visible(project, p.system.layer()) && pipes::diameter(p).is_some())
        .filter_map(|p| {
            let level = project.levels.iter().find(|l| l.id == p.level_id)?;
            let z0 = if level.elevation_mm.is_finite() { level.elevation_mm } else { 0.0 };
            let pts: Vec<V3> = pipes::points(p).into_iter().map(|q| q + v3(0.0, 0.0, z0)).collect();
            (pts.len() >= 2).then_some(Run { pipe: p, level, pts })
        })
        .collect()
}

/// Fixture tags of the project: "WC", or "WC-1", "WC-2" when there are
/// several, numbered in element order.
pub fn fixture_tags(project: &Project) -> Vec<(&Asset, String)> {
    let fixtures: Vec<(&Asset, &'static str)> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Asset(a) => services::fixture_tag(&a.catalog_key).map(|t| (a, t)),
            _ => None,
        })
        .collect();
    let mut total: HashMap<&str, usize> = HashMap::new();
    for (_, t) in &fixtures {
        *total.entry(t).or_default() += 1;
    }
    let mut seen: HashMap<&str, usize> = HashMap::new();
    fixtures
        .into_iter()
        .map(|(a, t)| {
            let i = seen.entry(t).or_default();
            *i += 1;
            let name = if total[t] > 1 { format!("{t}-{i}") } else { t.to_string() };
            (a, name)
        })
        .collect()
}

/// Plan distance from a point to an object's footprint.
fn footprint_distance(p: V, a: &Asset) -> f64 {
    let c = V::from(a.position);
    let u = dir(a.rotation_deg);
    let w = dir(a.rotation_deg + 90.0);
    let lx = (p - c).dot(u);
    let ly = (p - c).dot(w);
    let dx = (lx.abs() - a.width_mm.abs() / 2.0).max(0.0);
    let dy = (ly.abs() - a.depth_mm.abs() / 2.0).max(0.0);
    dx.hypot(dy)
}

fn dist_point_seg3(p: V3, a: V3, b: V3) -> f64 {
    let ab = b - a;
    let l2 = ab.dot(ab);
    let t = if l2 < 1e-12 { 0.0 } else { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) };
    (p - (a + ab * t)).len()
}

/// A drawn piece of a run: a polyline in diagram mm.
pub struct Stroke {
    pub system: PipeSystem,
    /// Paper mm.
    pub width: f64,
    pub pts: Vec<V>,
}

/// One diagram at 1:n, in diagram mm. Text heights are paper sizes times n.
pub struct Diagram {
    pub which: Which,
    pub strokes: Vec<Stroke>,
    /// Flow arrows, filled triangles.
    pub arrows: Vec<[V; 3]>,
    /// Upright labels and tags.
    pub texts: Vec<Prim>,
    /// Thin lines from a label placed away from its run back to the run.
    pub leaders: Vec<(V, V)>,
    pub bounds: Bounds,
    /// Riser tag prefixes used, in first-use order.
    pub riser_prefixes: Vec<&'static str>,
    /// Fixture tags used, with the fixture name.
    pub fixtures: Vec<(String, String)>,
    pub crossings: usize,
    pub systems: Vec<PipeSystem>,
}

impl Diagram {
    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty()
    }
}

struct Seg {
    run: usize,
    index: usize,
    a3: V3,
    b3: V3,
    a: V,
    b: V,
}

/// Where segment p0-p1 meets segment q0-q1 in the plane: the parameters on
/// both, or None when parallel or apart.
fn intersect(p0: V, p1: V, q0: V, q1: V) -> Option<(f64, f64)> {
    let r = p1 - p0;
    let s = q1 - q0;
    let den = r.x * s.y - r.y * s.x;
    if den.abs() < 1e-9 * (r.len() * s.len()).max(1e-9) {
        return None;
    }
    let qp = q0 - p0;
    let t = (qp.x * s.y - qp.y * s.x) / den;
    let u = (qp.x * r.y - qp.y * r.x) / den;
    let eps = 1e-9;
    ((-eps..=1.0 + eps).contains(&t) && (-eps..=1.0 + eps).contains(&u)).then_some((t, u))
}

/// True when segment a-b touches the box.
fn seg_hits_box(a: V, b: V, bx: &Bounds) -> bool {
    // Liang-Barsky clip of the segment against the box.
    let d = b - a;
    let mut t0 = 0.0f64;
    let mut t1 = 1.0f64;
    for (p, q) in [
        (-d.x, a.x - bx.min.x),
        (d.x, bx.max.x - a.x),
        (-d.y, a.y - bx.min.y),
        (d.y, bx.max.y - a.y),
    ] {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return false;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

fn text_box(p: &Prim) -> Bounds {
    items_bounds(std::slice::from_ref(&Item { cat: Cat::Text, pen: Pen::Light, prim: p.clone() }))
}

/// Greedy label placement against lines, arrows and placed labels.
struct Placer {
    lines: Vec<(V, V)>,
    boxes: Vec<Bounds>,
    /// Clearance from lines, diagram mm.
    clear: f64,
    /// Clearance between labels, diagram mm.
    spacing: f64,
}

fn grow(b: &Bounds, d: f64) -> Bounds {
    Bounds { min: b.min - v(d, d), max: b.max + v(d, d) }
}

/// The point of a box nearest to `p`.
fn nearest_on_box(b: &Bounds, p: V) -> V {
    v(p.x.clamp(b.min.x, b.max.x), p.y.clamp(b.min.y, b.max.y))
}

impl Placer {
    fn score(&self, b: &Bounds, leader: Option<(V, V)>) -> usize {
        let near = grow(b, self.clear);
        let lines = self.lines.iter().filter(|(a, c)| seg_hits_box(*a, *c, &near)).count();
        let apart = grow(b, self.spacing);
        let boxes = self.boxes.iter().filter(|o| o.intersects(&apart)).count();
        // A leader must not run through another label.
        let crossed = leader
            .map(|(a, c)| self.boxes.iter().filter(|o| seg_hits_box(a, c, o)).count())
            .unwrap_or(0);
        // Text over text is the worst outcome; a line through a label is
        // better than that.
        lines + 20 * boxes + 20 * crossed
    }

    /// Put `text` at the first free spot: next to its run (`near` centers),
    /// else further out with a leader back to one of `anchors`, else at the
    /// spot with the fewest hits (next to the run on a tie). Returns the
    /// label and its leader, if any.
    fn place(&mut self, text: &str, size: f64, bold: bool, near: &[V], anchors: &[V]) -> Option<(Prim, Option<(V, V)>)> {
        type Cand = (usize, Prim, Bounds, Option<(V, V)>);
        let eval = |center: V, anchor: Option<V>| -> Cand {
            let prim = services::centered_text(center, text, size, bold);
            let b = text_box(&prim);
            let leader = anchor.map(|a| (a, nearest_on_box(&b, a)));
            (self.score(&b, leader), prim, b, leader)
        };
        let mut best: Option<Cand> = None;
        let mut chosen: Option<Cand> = None;
        for c in near {
            let cand = eval(*c, None);
            if cand.0 == 0 {
                chosen = Some(cand);
                break;
            }
            if best.as_ref().map(|x| cand.0 < x.0).unwrap_or(true) {
                best = Some(cand);
            }
        }
        if chosen.is_none() {
            let w = est_width(text, size, bold);
            'far: for dist in [2.2, 3.4, 4.6, 6.0, 8.0, 10.0] {
                for deg in [90.0, 270.0, 30.0, 150.0, 210.0, 330.0, 0.0, 180.0, 60.0, 120.0, 240.0, 300.0] {
                    for anchor in anchors {
                        let center = beside(*anchor, dir(deg), dist * size, w / 2.0, 0.55 * size);
                        let cand = eval(center, Some(*anchor));
                        if cand.0 == 0 {
                            chosen = Some(cand);
                            break 'far;
                        }
                        if best.as_ref().map(|x| cand.0 < x.0).unwrap_or(true) {
                            best = Some(cand);
                        }
                    }
                }
            }
        }
        let best = chosen.or(best);
        let (_, prim, b, leader) = best?;
        self.boxes.push(b);
        if let Some(l) = leader {
            self.lines.push(l);
        }
        Some((prim, leader))
    }
}

/// Centers for a box of half size (hw, hh) beside a point along a unit
/// direction, `gap` clear of it.
fn beside(p: V, d: V, gap: f64, hw: f64, hh: f64) -> V {
    p + d * (gap + hw * d.x.abs() + hh * d.y.abs())
}

/// Build one diagram at 1:n.
pub fn build(project: &Project, which: Which, n: f64) -> Diagram {
    let runs = runs(project, which);
    let mut systems: Vec<PipeSystem> = which
        .systems()
        .into_iter()
        .filter(|s| runs.iter().any(|r| r.pipe.system == *s))
        .collect();
    systems.dedup();
    let mut out = Diagram {
        which,
        strokes: Vec::new(),
        arrows: Vec::new(),
        texts: Vec::new(),
        leaders: Vec::new(),
        bounds: Bounds::empty(),
        riser_prefixes: Vec::new(),
        fixtures: Vec::new(),
        crossings: 0,
        systems,
    };
    if runs.is_empty() {
        return out;
    }
    let segs: Vec<Seg> = runs
        .iter()
        .enumerate()
        .flat_map(|(ri, r)| {
            r.pts.windows(2).enumerate().map(move |(i, w)| Seg {
                run: ri,
                index: i,
                a3: w[0],
                b3: w[1],
                a: to_iso(w[0]),
                b: to_iso(w[1]),
            })
        })
        .filter(|s| (s.b - s.a).len() > 1e-6)
        .collect();

    // Crossings: the rear run gets a gap around the crossing.
    let gap = 1.1 * n;
    let mut gaps: Vec<Vec<(f64, f64)>> = vec![Vec::new(); segs.len()];
    for i in 0..segs.len() {
        for j in (i + 1)..segs.len() {
            let (s1, s2) = (&segs[i], &segs[j]);
            if s1.run == s2.run && s1.index.abs_diff(s2.index) <= 1 {
                continue;
            }
            let Some((t, u)) = intersect(s1.a, s1.b, s2.a, s2.b) else { continue };
            let p1 = s1.a3 + (s1.b3 - s1.a3) * t;
            let p2 = s2.a3 + (s2.b3 - s2.a3) * u;
            let r1 = runs[s1.run].pipe.diameter_mm / 2.0;
            let r2 = runs[s2.run].pipe.diameter_mm / 2.0;
            if (p1 - p2).len() <= r1 + r2 + 10.0 {
                continue; // they meet: a joint, not a crossing
            }
            out.crossings += 1;
            let (k, at) = if depth(p1) > depth(p2) { (i, t) } else { (j, u) };
            let len = (segs[k].b - segs[k].a).len();
            let h = gap / len;
            gaps[k].push(((at - h).max(0.0), (at + h).min(1.0)));
        }
    }

    // Strokes: each run as polylines, split at its gaps.
    for (ri, r) in runs.iter().enumerate() {
        let width = iso_width(r.pipe.diameter_mm);
        let mut cur: Vec<V> = Vec::new();
        let flush = |cur: &mut Vec<V>, out: &mut Vec<Stroke>| {
            if cur.len() >= 2 {
                out.push(Stroke { system: r.pipe.system, width, pts: std::mem::take(cur) });
            }
            cur.clear();
        };
        for (si, s) in segs.iter().enumerate().filter(|(_, s)| s.run == ri) {
            let mut g = gaps[si].clone();
            g.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let mut pieces: Vec<(f64, f64)> = Vec::new();
            let mut from = 0.0;
            for (g0, g1) in g {
                if g0 > from {
                    pieces.push((from, g0));
                }
                from = from.max(g1);
            }
            if from < 1.0 {
                pieces.push((from, 1.0));
            }
            let at = |t: f64| s.a + (s.b - s.a) * t;
            for (t0, t1) in pieces {
                if t0 > 1e-9 {
                    flush(&mut cur, &mut out.strokes);
                }
                if cur.is_empty() {
                    cur.push(at(t0));
                }
                cur.push(at(t1));
                if t1 < 1.0 - 1e-9 {
                    flush(&mut cur, &mut out.strokes);
                }
            }
            if gaps[si].iter().any(|g| g.1 >= 1.0 - 1e-9) {
                flush(&mut cur, &mut out.strokes);
            }
        }
        flush(&mut cur, &mut out.strokes);
    }

    // Flow arrows on drains, first point to last.
    let arrow_len = 1.6 * n;
    for s in &segs {
        let r = &runs[s.run];
        if r.pipe.system != PipeSystem::Drainage {
            continue;
        }
        let d = s.b - s.a;
        if d.len() / n < 4.0 {
            continue;
        }
        let u = d.unit().expect("segment has a length");
        let mid = s.a + d * 0.5;
        let tip = mid + u * (arrow_len / 2.0);
        let base = mid - u * (arrow_len / 2.0);
        let side = u.left() * (0.55 * n);
        out.arrows.push([tip, base + side, base - side]);
    }

    // Labels.
    let mut placer = Placer {
        lines: segs.iter().map(|s| (s.a, s.b)).collect(),
        boxes: out
            .arrows
            .iter()
            .map(|t| {
                let mut b = Bounds::empty();
                t.iter().for_each(|p| b.add(*p));
                b
            })
            .collect(),
        clear: 0.45 * n,
        spacing: 0.9 * n,
    };
    let tag_size = 1.8 * n;
    let label_size = 1.6 * n;
    let gap_label = 0.6 * n;

    // Fixture tags at free run ends near a fixture of the same level.
    let tags = fixture_tags(project);
    let mut tagged: Vec<&str> = Vec::new();
    for (ri, r) in runs.iter().enumerate() {
        for end in [0, r.pts.len() - 1] {
            let p = r.pts[end];
            let joined = runs.iter().enumerate().any(|(oj, o)| {
                o.pts.windows(2).enumerate().any(|(k, w)| {
                    // The run's own segment at this end does not count.
                    let own = oj == ri && (k == end || k + 1 == end);
                    !own && dist_point_seg3(p, w[0], w[1]) <= (r.pipe.diameter_mm + o.pipe.diameter_mm) / 2.0 + 5.0
                })
            });
            if joined {
                continue;
            }
            let near = tags
                .iter()
                .filter(|(a, _)| a.level_id == r.level.id)
                .map(|(a, name)| (footprint_distance(v(p.x, p.y), a), a, name))
                .filter(|(d, _, _)| *d <= FIXTURE_REACH)
                .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let Some((_, a, name)) = near else { continue };
            if tagged.contains(&a.id.as_str()) {
                continue;
            }
            tagged.push(a.id.as_str());
            let at = to_iso(p);
            let w = est_width(name, tag_size, true);
            let (hw, hh) = (w / 2.0, 0.55 * tag_size);
            let candidates: Vec<V> = [0.0, 180.0, 90.0, 270.0, 45.0, 135.0, 315.0, 225.0]
                .iter()
                .map(|deg| beside(at, dir(*deg), gap_label, hw, hh))
                .collect();
            if let Some((prim, leader)) = placer.place(name, tag_size, true, &candidates, &[at]) {
                out.texts.push(prim);
                out.leaders.extend(leader);
                let base = name.split('-').next().unwrap_or(name).to_string();
                if !out.fixtures.iter().any(|(t, _)| *t == base) {
                    out.fixtures.push((base, clean(&services::catalog_name(&a.catalog_key))));
                }
            }
        }
    }

    // Riser tags on vertical segments from 500 mm.
    let mut counters: HashMap<&'static str, usize> = HashMap::new();
    for s in &segs {
        let r = &runs[s.run];
        if !pipes::is_vertical(s.a3, s.b3) || (s.b3.z - s.a3.z).abs() < RISER_TAG_MIN {
            continue;
        }
        let Some(prefix) = riser_prefix(r.pipe.system, r.pipe.diameter_mm) else { continue };
        let i = counters.entry(prefix).or_default();
        *i += 1;
        let text = format!("{prefix}-{i}");
        let w = est_width(&text, tag_size, true);
        let (hw, hh) = (w / 2.0, 0.55 * tag_size);
        let mut candidates = Vec::new();
        for t in [0.5, 0.3, 0.7, 0.15, 0.85] {
            let p = s.a + (s.b - s.a) * t;
            candidates.push(beside(p, v(1.0, 0.0), gap_label, hw, hh));
            candidates.push(beside(p, v(-1.0, 0.0), gap_label, hw, hh));
        }
        let anchors: Vec<V> = [0.5, 0.25, 0.75].iter().map(|t| s.a + (s.b - s.a) * *t).collect();
        if let Some((prim, leader)) = placer.place(&text, tag_size, true, &candidates, &anchors) {
            out.texts.push(prim);
            out.leaders.extend(leader);
            if !out.riser_prefixes.contains(&prefix) {
                out.riser_prefixes.push(prefix);
            }
        }
    }

    // Size and material on every run, on its longest free stretch.
    for (ri, r) in runs.iter().enumerate() {
        let text = format!(
            "{} {}",
            pipes::size_label(r.pipe.diameter_mm),
            pipes::material_label(r.pipe.material)
        );
        let w = est_width(&text, label_size, false);
        let (hw, hh) = (w / 2.0, 0.55 * label_size);
        let mut own: Vec<&Seg> = segs.iter().filter(|s| s.run == ri).collect();
        own.sort_by(|a, b| {
            (b.b - b.a)
                .len()
                .partial_cmp(&(a.b - a.a).len())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut candidates = Vec::new();
        for s in &own {
            let d = (s.b - s.a).unit().unwrap_or(v(1.0, 0.0));
            let nrm = d.left();
            for t in [0.5, 0.3, 0.7, 0.15, 0.85] {
                let p = s.a + (s.b - s.a) * t;
                candidates.push(beside(p, nrm, gap_label, hw, hh));
                candidates.push(beside(p, -nrm, gap_label, hw, hh));
            }
        }
        let anchors: Vec<V> = own
            .iter()
            .flat_map(|s| [0.5, 0.25, 0.75].map(|t| s.a + (s.b - s.a) * t))
            .collect();
        if let Some((prim, leader)) = placer.place(&text, label_size, false, &candidates, &anchors) {
            out.texts.push(prim);
            out.leaders.extend(leader);
        }
    }

    let mut b = Bounds::empty();
    for s in &out.strokes {
        s.pts.iter().for_each(|p| b.add(*p));
    }
    for t in &out.arrows {
        t.iter().for_each(|p| b.add(*p));
    }
    for (a, c) in &out.leaders {
        b.add(*a);
        b.add(*c);
    }
    let tb = items_bounds(
        &out.texts
            .iter()
            .map(|p| Item { cat: Cat::Text, pen: Pen::Light, prim: p.clone() })
            .collect::<Vec<Item>>(),
    );
    b.add(tb.min);
    b.add(tb.max);
    out.bounds = b;
    out
}

/// Paper direction of plan north on the diagrams.
pub fn north_on_paper(project: &Project) -> (f64, f64) {
    let a = project.settings.north_angle_deg;
    let a = if a.is_finite() { a } else { 0.0 };
    let (sn, cs) = a.to_radians().sin_cos();
    let d = to_iso(v3(-sn, cs, 0.0));
    let l = d.len().max(1e-9);
    (d.x / l, -d.y / l)
}

// ------------------------------------------------------------------- sheet

fn legend(project: &Project) -> Vec<LegendRow> {
    let water = build(project, Which::Water, 100.0);
    let sanitary = build(project, Which::Sanitary, 100.0);
    let mut rows = Vec::new();
    for d in [&water, &sanitary] {
        for s in &d.systems {
            let text = match s {
                PipeSystem::Drainage => "Drainage, waste and soil".to_string(),
                other => pipes::label(*other).to_string(),
            };
            rows.push(LegendRow { sample: Sample::IsoRun(*s), text, qty: String::new() });
        }
    }
    if !sanitary.arrows.is_empty() {
        rows.push(LegendRow { sample: Sample::Arrow, text: "Flow direction".into(), qty: String::new() });
    }
    rows.push(LegendRow {
        sample: Sample::Text { text: "20 PPR".into(), bold: false },
        text: "Size in mm and material, as drawn".into(),
        qty: String::new(),
    });
    for d in [&water, &sanitary] {
        for prefix in &d.riser_prefixes {
            rows.push(LegendRow {
                sample: Sample::Text { text: format!("{prefix}-1"), bold: true },
                text: riser_meaning(prefix).into(),
                qty: String::new(),
            });
        }
    }
    let mut fixtures: Vec<(String, String)> = Vec::new();
    for d in [&water, &sanitary] {
        for f in &d.fixtures {
            if !fixtures.iter().any(|x| x.0 == f.0) {
                fixtures.push(f.clone());
            }
        }
    }
    for (tag, name) in fixtures {
        rows.push(LegendRow { sample: Sample::Text { text: tag, bold: true }, text: name, qty: String::new() });
    }
    if water.crossings + sanitary.crossings > 0 {
        rows.push(LegendRow {
            sample: Sample::Crossing,
            text: "Runs cross: the rear one is broken".into(),
            qty: String::new(),
        });
    }
    rows
}

fn blocks(project: &Project) -> Vec<Block> {
    vec![
        Block { id: "legend".into(), title: "LEGEND".into(), content: Content::Legend(legend(project)) },
        Block {
            id: "notes".into(),
            title: "NOTES".into(),
            content: Content::Notes(vec![
                "Diagrams not to scale. Runs from the Guhit Studio model, all levels at their heights.".into(),
                "Sizes in mm and materials as drawn in the model. Sizing and design are for the Master Plumber.".into(),
                "Coordination draft for the Master Plumber.".into(),
            ]),
        },
        Block { id: "signatory".into(), title: "MASTER PLUMBER".into(), content: Content::Signatory },
    ]
}

/// Paper room kept around a diagram's lines for its labels when the scale
/// is picked, mm at k = 1.
const LABEL_ROOM: f64 = 12.0;

/// Bounds of a diagram's lines, without labels. Independent of the scale.
fn line_bounds(project: &Project, which: Which) -> Bounds {
    let mut b = Bounds::empty();
    for r in runs(project, which) {
        for p in &r.pts {
            b.add(to_iso(*p));
        }
    }
    b
}

/// The two diagram areas, side by side or stacked. Each is (x, y, width,
/// height) including its title strip.
fn regions(area: (f64, f64, f64, f64), k: f64, side_by_side: bool) -> [(f64, f64, f64, f64); 2] {
    let (x, y, w, h) = area;
    let gap = 8.0 * k;
    if side_by_side {
        let cw = (w - gap) / 2.0;
        [(x, y, cw, h), (x + cw + gap, y, cw, h)]
    } else {
        let ch = (h - gap) / 2.0;
        [(x, y, w, ch), (x, y + ch + gap, w, ch)]
    }
}

fn title_strip(k: f64) -> f64 {
    12.0 * k
}

/// Render the plumbing isometric sheet. Returns the SVG and the scale the
/// diagrams are drawn at along their axes.
pub(crate) fn render(project: &Project, _derived: &Derived, opts: &PlanExportOptions) -> Result<(String, u32), ExportError> {
    if project.levels.is_empty() {
        return Err(ExportError::Empty("the project has no levels".into()));
    }
    if runs(project, Which::Water).is_empty() && runs(project, Which::Sanitary).is_empty() {
        return Err(ExportError::Empty(
            "the model has no water, drainage or vent runs for the isometric diagrams".into(),
        ));
    }
    let layout = Layout::new(opts.paper, opts.orientation, opts.title_block);
    let k = layout.k;
    let blocks = blocks(project);
    let panel = place_panel(&blocks, &layout);
    let strip = title_strip(k);
    let which = [Which::Water, Which::Sanitary];
    let lines: Vec<Bounds> = which.iter().map(|w| line_bounds(project, *w)).collect();
    // The largest common scale at which both diagrams, with room for their
    // labels, fit their areas; side by side or stacked, whichever draws
    // larger.
    let pick = |regs: &[(f64, f64, f64, f64); 2]| -> u32 {
        (0..2)
            .map(|i| {
                let (_, _, rw, rh) = regs[i];
                pick_scale(rw, rh - strip, |n| {
                    let margin = 2.0 * LABEL_ROOM * k * n as f64;
                    (lines[i].width() + margin, lines[i].height() + margin)
                })
            })
            .max()
            .unwrap_or(100)
    };
    let (area_x, area_y, area_w, area_h) = panel.area;
    let side = regions((area_x, area_y, area_w, area_h), k, true);
    let stacked = regions((area_x, area_y, area_w, area_h), k, false);
    let (regs, first) = if pick(&stacked) < pick(&side) { (stacked, pick(&stacked)) } else { (side, pick(&side)) };
    let fits = |n: u32| -> bool {
        (0..2).all(|i| {
            let d = build(project, which[i], n as f64);
            let (_, _, rw, rh) = regs[i];
            d.is_empty() || (d.bounds.width() / n as f64 <= rw && d.bounds.height() / n as f64 <= rh - strip)
        })
    };
    let scale = match opts.scale_denominator {
        Some(n) if n > 0 => n,
        _ => crate::sheet::COMMON_SCALES
            .iter()
            .copied()
            .filter(|n| *n >= first)
            .find(|n| fits(*n))
            .unwrap_or(first),
    };
    let n = scale as f64;

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
        xml_escape(&format!("{} - Plumbing isometric diagrams - not to scale", clean(&project.name)))
    ));
    svg.s.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>\n",
        num(w),
        num(h)
    ));
    svg.s.push_str("<defs>");
    for (i, (x, y, rw, rh)) in regs.iter().enumerate() {
        svg.s.push_str(&format!(
            "<clipPath id=\"iso-clip-{i}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath>",
            num(*x),
            num(*y),
            num(*rw),
            num(rh - strip)
        ));
    }
    svg.s.push_str("</defs>\n");
    for (i, which) in [Which::Water, Which::Sanitary].into_iter().enumerate() {
        let d = build(project, which, n);
        diagram_svg(&mut svg, &d, regs[i], strip, n, k, i);
    }
    svg.s.push_str(&panel.svg);
    let (nx, ny) = north_on_paper(project);
    let frame = Frame {
        title: "PLUMBING ISOMETRIC DIAGRAMS".into(),
        scale_text: "NOT TO SCALE".into(),
        bar: None,
        north: North::Paper(nx, ny),
        legend: &[],
        block_scale: "NTS".into(),
    };
    furniture(&mut svg, project, opts, &layout, &frame);
    svg.s.push_str("</svg>\n");
    Ok((svg.s, scale))
}

fn diagram_svg(svg: &mut Svg, d: &Diagram, reg: (f64, f64, f64, f64), strip: f64, n: f64, k: f64, i: usize) {
    let (x, y, w, h) = reg;
    let dh = h - strip;
    svg.s.push_str(&format!("<g id=\"{}\">\n", d.which.slug()));
    if d.is_empty() {
        svg.text(x + w / 2.0, y + dh / 2.0, 2.2 * k, "middle", false, MUTED, d.which.empty());
    } else {
        let c = d.bounds.center();
        let (ox, oy) = (x + w / 2.0, y + dh / 2.0);
        let to_paper = move |p: V| -> (f64, f64) { (ox + (p.x - c.x) / n, oy - (p.y - c.y) / n) };
        svg.s.push_str(&format!(
            "<g clip-path=\"url(#iso-clip-{i})\" fill=\"none\" stroke-linejoin=\"round\">\n"
        ));
        for system in d.which.systems() {
            let strokes: Vec<&Stroke> = d.strokes.iter().filter(|s| s.system == system).collect();
            if strokes.is_empty() {
                continue;
            }
            let dash = iso_dash(system);
            svg.s.push_str(&format!(
                "<g class=\"iso-run\" data-system=\"{}\" stroke=\"{}\" stroke-linecap=\"{}\">\n",
                pipes::slug(system),
                pipes::color(system),
                line_cap(dash)
            ));
            for s in strokes {
                let pts: Vec<String> = s
                    .pts
                    .iter()
                    .map(|p| {
                        let q = to_paper(*p);
                        format!("{},{}", num(q.0), num(q.1))
                    })
                    .collect();
                svg.s.push_str(&format!(
                    "<polyline points=\"{}\" stroke-width=\"{}\"{}/>\n",
                    pts.join(" "),
                    num(s.width),
                    dash_attr(&pipes::dash_pattern(dash, s.width))
                ));
            }
            svg.s.push_str("</g>\n");
        }
        if !d.arrows.is_empty() {
            svg.s.push_str(&format!("<g class=\"flow-arrows\" fill=\"{INK}\" stroke=\"none\">\n"));
            for t in &d.arrows {
                let pts: Vec<String> = t
                    .iter()
                    .map(|p| {
                        let q = to_paper(*p);
                        format!("{},{}", num(q.0), num(q.1))
                    })
                    .collect();
                svg.s.push_str(&format!("<polygon points=\"{}\"/>\n", pts.join(" ")));
            }
            svg.s.push_str("</g>\n");
        }
        if !d.leaders.is_empty() {
            svg.s.push_str(&format!("<g class=\"leaders\" stroke=\"{INK}\" stroke-width=\"0.13\">\n"));
            for (a, c) in &d.leaders {
                let (p, q) = (to_paper(*a), to_paper(*c));
                svg.s.push_str(&format!(
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"/>\n",
                    num(p.0),
                    num(p.1),
                    num(q.0),
                    num(q.1)
                ));
            }
            svg.s.push_str("</g>\n");
        }
        svg.s.push_str("<g class=\"iso-labels\">\n");
        for t in &d.texts {
            let item = Item { cat: Cat::Text, pen: Pen::Light, prim: t.clone() };
            write_item_in(&mut svg.s, &item, n, &to_paper, &PLAN_INK);
        }
        svg.s.push_str("</g>\n");
        svg.s.push_str("</g>\n");
    }
    // Diagram title and "NOT TO SCALE" under it.
    let base = y + dh + 6.0 * k;
    let (title, size) = fit(d.which.title(), 3.0 * k, 2.0 * k, w - 4.0 * k, true);
    let tw = est_width(&title, size, true);
    svg.text(x + w / 2.0, base, size, "middle", true, INK, &title);
    svg.line((x + w / 2.0 - tw / 2.0 - 1.0 * k, base + 1.4 * k), (x + w / 2.0 + tw / 2.0 + 1.0 * k, base + 1.4 * k), 0.35);
    svg.text(x + w / 2.0, base + 4.6 * k, 2.2 * k, "middle", false, INK, "NOT TO SCALE");
    svg.s.push_str("</g>\n");
}

// --------------------------------------------------------------------- DXF

/// Layers of the isometric DXF besides the run layers.
pub const LAYER_TEXT: &str = "P-ANNO-TEXT";
pub const LAYER_SYMB: &str = "P-ANNO-SYMB";

/// The isometric diagrams as DXF linework in diagram mm (plan x at 30
/// degrees, plan y at 150 degrees, height up), the water diagram at the
/// origin and the sanitary one to its right. Runs on their system layers,
/// labels, tags and leaders on `P-ANNO-TEXT`, flow arrows on `P-ANNO-SYMB`.
/// Text is sized for plotting at 1:n.
pub(crate) fn dxf(project: &Project, opts: &PlanExportOptions) -> Result<(String, u32), ExportError> {
    let scale = opts
        .scale_denominator
        .filter(|n| *n > 0)
        .or(Some(project.settings.scale_denominator).filter(|n| *n > 0))
        .unwrap_or(100);
    let n = scale as f64;
    let water = build(project, Which::Water, n);
    let sanitary = build(project, Which::Sanitary, n);
    if water.is_empty() && sanitary.is_empty() {
        return Err(ExportError::Empty(
            "the model has no water, drainage or vent runs for the isometric diagrams".into(),
        ));
    }
    let mut parts = crate::dxf::Parts::new(format!(
        "Plumbing isometric diagrams for {}, not to scale. Diagram mm: plan x at 30 degrees, plan y at 150 degrees, height up. Text sized for plotting at 1:{scale}",
        clean(&project.name)
    ));
    parts.arch_layers = false;
    let gap = 40.0 * n;
    let mut x = 0.0;
    let mut systems: Vec<PipeSystem> = Vec::new();
    for d in [&water, &sanitary] {
        if d.is_empty() {
            continue;
        }
        let off = v(x - d.bounds.min.x, -d.bounds.min.y);
        for st in &d.strokes {
            if !systems.contains(&st.system) {
                systems.push(st.system);
            }
            parts.ents.push(crate::dxf::Ent {
                layer: pipes::dxf_layer(st.system).to_string(),
                prim: Prim::Poly {
                    pts: st.pts.iter().map(|p| *p + off).collect(),
                    closed: false,
                    fill: crate::plan::Fill::None,
                },
                linetype: Some(if iso_dash(st.system) == Dash::Dashed { "GUHIT_DASHED" } else { "CONTINUOUS" }),
            });
        }
        for t in &d.arrows {
            parts.ents.push(crate::dxf::Ent {
                layer: LAYER_SYMB.into(),
                prim: Prim::Poly {
                    pts: t.iter().map(|p| *p + off).collect(),
                    closed: true,
                    fill: crate::plan::Fill::Ink,
                },
                linetype: None,
            });
        }
        for (a, c) in &d.leaders {
            parts.ents.push(crate::dxf::Ent {
                layer: LAYER_TEXT.into(),
                prim: Prim::Line { a: *a + off, b: *c + off },
                linetype: None,
            });
        }
        for t in &d.texts {
            parts.ents.push(crate::dxf::Ent {
                layer: LAYER_TEXT.into(),
                prim: crate::dxf::offset_prim(t, off),
                linetype: None,
            });
        }
        let cx = x + d.bounds.width() / 2.0;
        for (text, size, dy, bold) in [(d.which.title(), 3.0 * n, 8.0 * n, true), ("NOT TO SCALE", 2.2 * n, 12.5 * n, false)] {
            parts.ents.push(crate::dxf::Ent {
                layer: LAYER_TEXT.into(),
                prim: services::centered_text(v(cx, -dy), text, size, bold),
                linetype: None,
            });
        }
        x += d.bounds.width() + gap;
    }
    for s in pipes::SYSTEMS {
        if systems.contains(&s) {
            parts.layers.push(crate::dxf::ExtraLayer {
                name: pipes::dxf_layer(s).to_string(),
                color: pipes::dxf_color(s),
                linetype: "CONTINUOUS",
            });
        }
    }
    for name in [LAYER_TEXT, LAYER_SYMB] {
        parts.layers.push(crate::dxf::ExtraLayer { name: name.into(), color: 7, linetype: "CONTINUOUS" });
    }
    if systems.iter().any(|s| iso_dash(*s) == Dash::Dashed) {
        parts.dashes.push(Dash::Dashed);
    }
    Ok((crate::dxf::write_parts(&parts, scale), scale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axes_run_at_30_and_150_degrees_and_up() {
        let x = to_iso(v3(1000.0, 0.0, 0.0));
        let y = to_iso(v3(0.0, 1000.0, 0.0));
        let z = to_iso(v3(0.0, 0.0, 1000.0));
        assert!((x.angle_deg() - 30.0).abs() < 1e-9);
        assert!((y.angle_deg() - 150.0).abs() < 1e-9);
        assert!((z.angle_deg() - 90.0).abs() < 1e-9);
        // Lengths along the axes keep their size.
        assert!((x.len() - 1000.0).abs() < 1e-9 && (y.len() - 1000.0).abs() < 1e-9);
        // Nearer and higher is in front.
        assert!(depth(v3(0.0, 0.0, 100.0)) < depth(v3(0.0, 0.0, 0.0)));
        assert!(depth(v3(100.0, 100.0, 0.0)) > depth(v3(0.0, 0.0, 0.0)));
    }

    #[test]
    fn segments_meet_or_miss() {
        let hit = intersect(v(0.0, 0.0), v(10.0, 0.0), v(5.0, -5.0), v(5.0, 5.0)).unwrap();
        assert!((hit.0 - 0.5).abs() < 1e-9 && (hit.1 - 0.5).abs() < 1e-9);
        assert!(intersect(v(0.0, 0.0), v(10.0, 0.0), v(11.0, -5.0), v(11.0, 5.0)).is_none());
        assert!(intersect(v(0.0, 0.0), v(10.0, 0.0), v(0.0, 1.0), v(10.0, 1.0)).is_none());
        let b = Bounds { min: v(2.0, 2.0), max: v(4.0, 4.0) };
        assert!(seg_hits_box(v(0.0, 3.0), v(10.0, 3.0), &b));
        assert!(!seg_hits_box(v(0.0, 5.0), v(10.0, 5.0), &b));
        assert!(seg_hits_box(v(3.0, 3.0), v(3.5, 3.5), &b));
    }
}
