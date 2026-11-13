//! The drift check (DECISIONS D53): does an AI image still show the model?
//!
//! The window draws the model's feature lines from the capture's camera (the
//! `Lines` map, black lines on white). Here the AI image's own edges are
//! found and every model edge looks for one nearby that runs the same way.
//! `missing` is the share of model edges with no such match. With the `Ids`
//! map each model edge also knows its element, so a window the image moved
//! shows up as a window whose edges all went missing, and the note names it.
//!
//! Everything runs on a small working copy (`WORK_PX` on the long side): the
//! check is about where walls, doors and windows are, not about texture.
//!
//! - Model edges: a working pixel is an edge when any line pixel falls in
//!   it. Its direction comes from the structure tensor of the line map, so a
//!   thin line gets the direction across it, as an edge in a photo does.
//!   Corners and crossings have no clear direction and match any edge.
//! - AI edges: grey, blurred, Sobel, thinned to the strongest pixel across
//!   the edge, above a threshold set by the image's own contrast.
//! - Matching: within `MATCH_PX` and `MATCH_BINS` of direction. The image is
//!   allowed a small shift (`SHIFT_PX`), the best one is used and the note
//!   says so: a slightly reframed picture is not a moved building.
//! - Furniture and pipes are left out of the score: restyling a sofa is what
//!   an AI render is for.

use std::collections::HashMap;

use image::imageops::{self, FilterType};
use image::{GrayImage, RgbImage, Rgba, RgbaImage};

use crate::renders::LegendEntry;

/// Long side of the working copy, pixels.
pub const WORK_PX: u32 = 384;
/// How far, in working pixels, an AI edge may be from a model edge and match.
pub const MATCH_PX: i32 = 2;
/// Directions are kept in this many bins over 180 degrees.
const BINS: u8 = 16;
/// Two directions agree within this many bins (about 34 degrees).
const MATCH_BINS: u8 = 3;
/// How far the whole image may be shifted to line up, working pixels.
pub const SHIFT_PX: i32 = 6;
/// Fewer scored model edge pixels than this and there is nothing to check.
pub const MIN_SCORED: usize = 60;
/// Below this structure tensor coherence an edge has no clear direction.
const MIN_COHERENCE: f32 = 0.35;
/// The weakest AI edge, in Sobel units of the blurred grey image.
const MIN_GRADIENT: f32 = 16.0;
/// No direction.
const ANY: u8 = u8::MAX;

/// The share of missing model edges above which an image is flagged.
pub const WARN_MISSING: f64 = 0.35;
/// An element with at least this share of its edges missing looks moved.
pub const ELEMENT_MOVED: f64 = 0.6;
/// An element needs this many edge pixels before it can be called moved.
pub const MIN_ELEMENT_PX: usize = 10;

/// How the check treats an element: its priority when an edge pixel touches
/// several (openings above walls), and whether its edges count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Weight {
    pub priority: u8,
    pub scored: bool,
}

/// One element's edges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementDrift {
    pub element_id: String,
    pub edges: usize,
    pub missing: usize,
}

impl ElementDrift {
    pub fn share(&self) -> f64 {
        if self.edges == 0 {
            0.0
        } else {
            self.missing as f64 / self.edges as f64
        }
    }

    /// Enough of its edges went missing to call it moved.
    pub fn moved(&self) -> bool {
        self.edges >= MIN_ELEMENT_PX && self.share() >= ELEMENT_MOVED
    }
}

/// What the check found.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// Share of scored model edges with no match, 0 to 1.
    pub missing: f64,
    /// Scored model edge pixels, working size.
    pub scored: usize,
    /// Per element, most missing first. Empty without an ids map.
    pub elements: Vec<ElementDrift>,
    /// The shift that lined the image up best, working pixels.
    pub shift: (i32, i32),
    /// Working size.
    pub size: (u32, u32),
    /// How far the image's shape is from the model view's, as a ratio (0 is
    /// the same shape).
    pub aspect_off: f64,
    /// PNG at working size: model edges with no match in red, the matched
    /// ones faint, on transparent.
    pub overlay: Vec<u8>,
}

impl Outcome {
    /// The shift as a share of the long side.
    pub fn shift_share(&self) -> f64 {
        let long = self.size.0.max(self.size.1).max(1) as f64;
        ((self.shift.0 * self.shift.0 + self.shift.1 * self.shift.1) as f64).sqrt() / long
    }
}

/// The inputs: the line map, the ids map with its legend, the AI image.
pub struct Check<'a> {
    pub lines_png: &'a [u8],
    pub ids: Option<(&'a [u8], &'a [LegendEntry])>,
    pub ai: &'a [u8],
}

fn decode(bytes: &[u8], what: &str) -> Result<image::DynamicImage, String> {
    image::load_from_memory(bytes).map_err(|e| format!("the {what} does not decode: {e}"))
}

/// A float image, row major.
struct Grid {
    w: usize,
    h: usize,
    v: Vec<f32>,
}

impl Grid {
    fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            v: vec![0.0; w * h],
        }
    }

    fn at(&self, x: isize, y: isize) -> f32 {
        let x = x.clamp(0, self.w as isize - 1) as usize;
        let y = y.clamp(0, self.h as isize - 1) as usize;
        self.v[y * self.w + x]
    }

    /// Separable 1 4 6 4 1 blur.
    fn blur(&self) -> Self {
        const K: [f32; 5] = [1.0 / 16.0, 4.0 / 16.0, 6.0 / 16.0, 4.0 / 16.0, 1.0 / 16.0];
        let mut tmp = Grid::new(self.w, self.h);
        for y in 0..self.h {
            for x in 0..self.w {
                tmp.v[y * self.w + x] = (0..5)
                    .map(|i| K[i] * self.at(x as isize + i as isize - 2, y as isize))
                    .sum();
            }
        }
        let mut out = Grid::new(self.w, self.h);
        for y in 0..self.h {
            for x in 0..self.w {
                out.v[y * self.w + x] = (0..5)
                    .map(|i| K[i] * tmp.at(x as isize, y as isize + i as isize - 2))
                    .sum();
            }
        }
        out
    }

    /// Sobel x and y.
    fn sobel(&self) -> (Grid, Grid) {
        let mut gx = Grid::new(self.w, self.h);
        let mut gy = Grid::new(self.w, self.h);
        for y in 0..self.h as isize {
            for x in 0..self.w as isize {
                let p = |dx: isize, dy: isize| self.at(x + dx, y + dy);
                let i = y as usize * self.w + x as usize;
                gx.v[i] =
                    (p(1, -1) + 2.0 * p(1, 0) + p(1, 1)) - (p(-1, -1) + 2.0 * p(-1, 0) + p(-1, 1));
                gy.v[i] =
                    (p(-1, 1) + 2.0 * p(0, 1) + p(1, 1)) - (p(-1, -1) + 2.0 * p(0, -1) + p(1, -1));
            }
        }
        (gx, gy)
    }
}

/// Direction bin of a gradient angle, over 180 degrees.
fn bin_of(angle: f32) -> u8 {
    let a = angle.rem_euclid(std::f32::consts::PI);
    ((a / std::f32::consts::PI * BINS as f32).floor() as u8).min(BINS - 1)
}

fn bins_agree(a: u8, b: u8) -> bool {
    if a == ANY || b == ANY {
        return true;
    }
    let d = a.abs_diff(b);
    d.min(BINS - d) <= MATCH_BINS
}

/// The working size for a map of `w` x `h`.
fn working_size(w: u32, h: u32) -> (usize, usize) {
    let long = w.max(h).max(1);
    if long <= WORK_PX {
        return (w.max(1) as usize, h.max(1) as usize);
    }
    let s = WORK_PX as f64 / long as f64;
    (
        ((w as f64 * s).round() as usize).max(1),
        ((h as f64 * s).round() as usize).max(1),
    )
}

/// Source pixel range of working pixel `i` of `n`, over `src` pixels.
fn span(i: usize, n: usize, src: usize) -> (usize, usize) {
    let a = i * src / n;
    let b = ((i + 1) * src / n).max(a + 1).min(src);
    (a, b)
}

/// One model edge pixel.
struct ModelPx {
    x: usize,
    y: usize,
    bin: u8,
    /// Index into the element list, or None.
    element: Option<usize>,
}

/// Model edges at working size, with their direction.
fn model_edges(lines: &GrayImage, ww: usize, wh: usize) -> (Vec<(usize, usize)>, Grid) {
    let (w0, h0) = (lines.width() as usize, lines.height() as usize);
    let mut darkness = Grid::new(ww, wh);
    let mut edges = vec![];
    for y in 0..wh {
        let (y0, y1) = span(y, wh, h0);
        for x in 0..ww {
            let (x0, x1) = span(x, ww, w0);
            let mut dark = 0usize;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    if lines.get_pixel(sx as u32, sy as u32).0[0] < 128 {
                        dark += 1;
                    }
                }
            }
            darkness.v[y * ww + x] = 255.0 * dark as f32 / ((y1 - y0) * (x1 - x0)) as f32;
            if dark > 0 {
                edges.push((x, y));
            }
        }
    }
    (edges, darkness)
}

/// Direction bins of the model edges from the structure tensor of the
/// blurred darkness, so a thin line gets the direction across it.
fn model_bins(darkness: &Grid, edges: &[(usize, usize)]) -> Vec<u8> {
    let (gx, gy) = darkness.blur().sobel();
    edges
        .iter()
        .map(|&(x, y)| {
            let (mut jxx, mut jxy, mut jyy) = (0.0f32, 0.0f32, 0.0f32);
            for dy in -2..=2isize {
                for dx in -2..=2isize {
                    let (a, b) = (
                        gx.at(x as isize + dx, y as isize + dy),
                        gy.at(x as isize + dx, y as isize + dy),
                    );
                    jxx += a * a;
                    jxy += a * b;
                    jyy += b * b;
                }
            }
            let trace = jxx + jyy;
            if trace <= f32::EPSILON {
                return ANY;
            }
            let coherence = ((jxx - jyy).powi(2) + 4.0 * jxy * jxy).sqrt() / trace;
            if coherence < MIN_COHERENCE {
                return ANY;
            }
            bin_of(0.5 * (2.0 * jxy).atan2(jxx - jyy))
        })
        .collect()
}

/// The AI image's edges at working size: a direction bin per pixel, or None.
/// Gradients are taken per color channel and the strongest one wins, so a
/// white wall against a pale blue sky still has its edge: the two can have
/// the same brightness and differ only in color.
fn ai_edges(ai: &RgbImage, ww: usize, wh: usize) -> Vec<Option<u8>> {
    let small = imageops::resize(ai, ww as u32, wh as u32, FilterType::Triangle);
    let channel = |c: usize| {
        let mut g = Grid::new(ww, wh);
        for (i, p) in small.pixels().enumerate() {
            g.v[i] = p.0[c] as f32;
        }
        g.blur().sobel()
    };
    let channels = [channel(0), channel(1), channel(2)];
    let mut gx = Grid::new(ww, wh);
    let mut gy = Grid::new(ww, wh);
    let mut mag = vec![0.0f32; ww * wh];
    for (i, best) in mag.iter_mut().enumerate() {
        for (cx, cy) in &channels {
            let m = (cx.v[i] * cx.v[i] + cy.v[i] * cy.v[i]).sqrt();
            if m > *best {
                *best = m;
                gx.v[i] = cx.v[i];
                gy.v[i] = cy.v[i];
            }
        }
    }
    // The threshold follows the image's own contrast, so a soft overcast
    // render and a hard noon one are judged alike.
    let mut sorted = mag.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p98 = sorted.get(sorted.len() * 98 / 100).copied().unwrap_or(0.0);
    let threshold = MIN_GRADIENT.max(0.12 * p98);
    let m = |x: isize, y: isize| -> f32 {
        if x < 0 || y < 0 || x >= ww as isize || y >= wh as isize {
            0.0
        } else {
            mag[y as usize * ww + x as usize]
        }
    };
    let mut out = vec![None; ww * wh];
    for y in 0..wh {
        for x in 0..ww {
            let i = y * ww + x;
            if mag[i] < threshold {
                continue;
            }
            let angle = gy.v[i].atan2(gx.v[i]);
            // The strongest pixel across the edge: its two neighbors along
            // the gradient are no stronger.
            let (dx, dy) = {
                let a = angle.rem_euclid(std::f32::consts::PI);
                let eighth = std::f32::consts::PI / 8.0;
                if !(eighth..7.0 * eighth).contains(&a) {
                    (1, 0)
                } else if a < 3.0 * eighth {
                    (1, 1)
                } else if a < 5.0 * eighth {
                    (0, 1)
                } else {
                    (-1, 1)
                }
            };
            let (xi, yi) = (x as isize, y as isize);
            if mag[i] >= m(xi + dx, yi + dy) && mag[i] >= m(xi - dx, yi - dy) {
                out[i] = Some(bin_of(angle));
            }
        }
    }
    out
}

/// Legend colors to legend indices; near colors match too.
struct Colors {
    exact: HashMap<[u8; 3], usize>,
    all: Vec<[u8; 3]>,
}

impl Colors {
    fn new(legend: &[LegendEntry]) -> Self {
        let all: Vec<[u8; 3]> = legend
            .iter()
            .map(|e| e.rgb().unwrap_or([0, 0, 0]))
            .collect();
        let exact = all.iter().enumerate().map(|(i, c)| (*c, i)).collect();
        Self { exact, all }
    }

    fn find(&self, c: [u8; 3]) -> Option<usize> {
        if c == [0, 0, 0] {
            return None;
        }
        if let Some(i) = self.exact.get(&c) {
            return Some(*i);
        }
        self.all
            .iter()
            .position(|o| o.iter().zip(c.iter()).all(|(a, b)| a.abs_diff(*b) <= 6))
    }
}

/// Run the check. `weight` says how each element id counts; an id it does
/// not know counts with the lowest priority.
pub fn check(c: &Check, weight: &dyn Fn(&str) -> Option<Weight>) -> Result<Outcome, String> {
    let lines = decode(c.lines_png, "line map")?.to_luma8();
    let ai = decode(c.ai, "AI image")?;
    let (w0, h0) = lines.dimensions();
    let (ww, wh) = working_size(w0, h0);
    let aspect_off = ((ai.width() as f64 / ai.height().max(1) as f64).ln()
        - (w0 as f64 / h0.max(1) as f64).ln())
    .abs();

    let (edges, darkness) = model_edges(&lines, ww, wh);
    let bins = model_bins(&darkness, &edges);

    // Which element each model edge belongs to, from the ids map.
    let mut elements: Vec<ElementDrift> = vec![];
    let mut scored_flags: Vec<bool> = vec![];
    let mut attributions: Vec<Option<usize>> = vec![None; edges.len()];
    let mut scored_px = vec![true; edges.len()];
    if let Some((ids_png, legend)) = c.ids {
        let ids = decode(ids_png, "ids map")?.to_rgb8();
        let (iw, ih) = (ids.width() as usize, ids.height() as usize);
        let colors = Colors::new(legend);
        let weights: Vec<Weight> = legend
            .iter()
            .map(|e| {
                weight(&e.element_id).unwrap_or(Weight {
                    priority: 0,
                    scored: true,
                })
            })
            .collect();
        let mut slot: HashMap<usize, usize> = HashMap::new();
        for (n, &(x, y)) in edges.iter().enumerate() {
            // The block under the working pixel, one working pixel wider.
            let (x0, x1) = (
                span(x.saturating_sub(1), ww, iw).0,
                span((x + 1).min(ww - 1), ww, iw).1,
            );
            let (y0, y1) = (
                span(y.saturating_sub(1), wh, ih).0,
                span((y + 1).min(wh - 1), wh, ih).1,
            );
            let mut best: Option<usize> = None;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let p = ids.get_pixel(sx as u32, sy as u32).0;
                    if let Some(i) = colors.find(p) {
                        if best.is_none_or(|b| weights[i].priority > weights[b].priority) {
                            best = Some(i);
                        }
                    }
                }
            }
            if let Some(i) = best {
                scored_px[n] = weights[i].scored;
                let at = *slot.entry(i).or_insert_with(|| {
                    elements.push(ElementDrift {
                        element_id: legend[i].element_id.clone(),
                        edges: 0,
                        missing: 0,
                    });
                    scored_flags.push(weights[i].scored);
                    elements.len() - 1
                });
                attributions[n] = Some(at);
            }
        }
    }
    let model: Vec<ModelPx> = edges
        .iter()
        .zip(&bins)
        .zip(&attributions)
        .zip(&scored_px)
        .filter(|(_, scored)| **scored)
        .map(|(((&(x, y), &bin), &element), _)| ModelPx { x, y, bin, element })
        .collect();

    let ai_bins = ai_edges(&ai.to_rgb8(), ww, wh);
    let matched_at = |p: &ModelPx, sx: i32, sy: i32| -> bool {
        for dy in -MATCH_PX..=MATCH_PX {
            for dx in -MATCH_PX..=MATCH_PX {
                let (x, y) = (p.x as i32 + dx + sx, p.y as i32 + dy + sy);
                if x < 0 || y < 0 || x >= ww as i32 || y >= wh as i32 {
                    continue;
                }
                if let Some(b) = ai_bins[y as usize * ww + x as usize] {
                    if bins_agree(p.bin, b) {
                        return true;
                    }
                }
            }
        }
        false
    };
    let count = |sx: i32, sy: i32, step: usize| {
        model
            .iter()
            .step_by(step)
            .filter(|p| matched_at(p, sx, sy))
            .count()
    };

    // The shift that lines the image up best: coarse over every fourth
    // pixel, then fine around it. Ties keep the smaller shift.
    let mut shift = (0, 0);
    if !model.is_empty() {
        let mut best = (count(0, 0, 4), 0i32);
        for sy in (-SHIFT_PX..=SHIFT_PX).step_by(2) {
            for sx in (-SHIFT_PX..=SHIFT_PX).step_by(2) {
                let n = count(sx, sy, 4);
                let dist = sx * sx + sy * sy;
                if n > best.0 || (n == best.0 && dist < best.1) {
                    best = (n, dist);
                    shift = (sx, sy);
                }
            }
        }
        let coarse = shift;
        let mut best = (
            count(coarse.0, coarse.1, 1),
            coarse.0 * coarse.0 + coarse.1 * coarse.1,
        );
        for sy in coarse.1 - 1..=coarse.1 + 1 {
            for sx in coarse.0 - 1..=coarse.0 + 1 {
                if sx.abs() > SHIFT_PX || sy.abs() > SHIFT_PX {
                    continue;
                }
                let n = count(sx, sy, 1);
                let dist = sx * sx + sy * sy;
                if n > best.0 || (n == best.0 && dist < best.1) {
                    best = (n, dist);
                    shift = (sx, sy);
                }
            }
        }
    }

    let mut overlay = RgbaImage::new(ww as u32, wh as u32);
    let mut missing_px = 0usize;
    for p in &model {
        let ok = matched_at(p, shift.0, shift.1);
        if let Some(e) = p.element {
            elements[e].edges += 1;
            if !ok {
                elements[e].missing += 1;
            }
        }
        if ok {
            overlay.put_pixel(p.x as u32, p.y as u32, Rgba([20, 184, 166, 90]));
        } else {
            missing_px += 1;
            overlay.put_pixel(p.x as u32, p.y as u32, Rgba([239, 68, 68, 255]));
        }
    }
    let mut png = std::io::Cursor::new(Vec::new());
    overlay
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| format!("the drift overlay could not be written: {e}"))?;

    let scored = model.len();
    let mut kept: Vec<ElementDrift> = elements
        .into_iter()
        .zip(scored_flags)
        .filter(|(_, s)| *s)
        .map(|(e, _)| e)
        .collect();
    kept.sort_by(|a, b| {
        b.missing
            .cmp(&a.missing)
            .then_with(|| a.element_id.cmp(&b.element_id))
    });
    Ok(Outcome {
        missing: if scored == 0 {
            0.0
        } else {
            missing_px as f64 / scored as f64
        },
        scored,
        elements: kept,
        shift,
        size: (ww as u32, wh as u32),
        aspect_off,
        overlay: png.into_inner(),
    })
}

/// How an element reads in the note, and whether a moved one alone is
/// reason to warn (doors, windows, stairs, columns).
pub struct Named {
    pub label: String,
    pub key: bool,
}

/// The report for the record: the share missing, the warning, and one
/// plain sentence that names what moved when it can.
pub fn report(o: &Outcome, name: &dyn Fn(&str) -> Option<Named>) -> guhit_model::DriftReport {
    if o.scored < MIN_SCORED {
        return guhit_model::DriftReport {
            missing: 0.0,
            warn: false,
            note: "Too little of the model is in view to check this image against it.".to_string(),
        };
    }
    let moved: Vec<(&ElementDrift, Option<Named>)> = o
        .elements
        .iter()
        .filter(|e| e.moved())
        .map(|e| (e, name(&e.element_id)))
        .collect();
    let key_moved = moved.iter().any(|(_, n)| n.as_ref().is_some_and(|n| n.key));
    let warn = o.missing >= WARN_MISSING || key_moved;
    let pct = (o.missing * 100.0).round() as i64;
    let mut note = if warn {
        format!("{pct}% of the model's edges have no match in this image, so it may not show the design.")
    } else {
        format!(
            "{}% of the model's edges line up with this image.",
            100 - pct
        )
    };
    let mut labels: Vec<String> = moved
        .iter()
        .filter_map(|(_, n)| n.as_ref().map(|n| n.label.clone()))
        .collect();
    labels.dedup();
    if !labels.is_empty() {
        let shown: Vec<String> = labels.iter().take(3).cloned().collect();
        let more = labels.len().saturating_sub(3);
        let list = if more > 0 {
            format!("{} and {more} more", shown.join(", "))
        } else {
            join_and(&shown)
        };
        note.push_str(&format!(" Looks moved or missing: {list}."));
    }
    if o.shift_share() >= 0.005 {
        note.push_str(&format!(
            " The image is framed about {:.0}% off the model view.",
            (o.shift_share() * 100.0).max(1.0)
        ));
    }
    if o.aspect_off > 0.05 {
        note.push_str(" Its shape differs from the model view, so the check stretched it to fit.");
    }
    if warn {
        note.push_str(" Compare it with the model view before you share it.");
    }
    guhit_model::DriftReport {
        missing: (o.missing * 1000.0).round() / 1000.0,
        warn,
        note,
    }
}

/// "a", "a and b", "a, b and c".
fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use image::{GrayImage, Luma, Rgb, RgbImage};

    /// A rectangle on the plan of a synthetic view, pixels at 1024 x 576.
    #[derive(Clone, Copy)]
    pub struct Rect {
        pub x0: u32,
        pub y0: u32,
        pub x1: u32,
        pub y1: u32,
    }

    pub const WALL: Rect = Rect {
        x0: 200,
        y0: 160,
        x1: 824,
        y1: 500,
    };
    pub const ROOF: Rect = Rect {
        x0: 170,
        y0: 90,
        x1: 854,
        y1: 160,
    };
    pub const WINDOW: Rect = Rect {
        x0: 290,
        y0: 250,
        x1: 410,
        y1: 350,
    };
    pub const DOOR: Rect = Rect {
        x0: 600,
        y0: 320,
        x1: 680,
        y1: 500,
    };
    pub const MOVED_WINDOW: Rect = Rect {
        x0: 440,
        y0: 230,
        x1: 560,
        y1: 330,
    };
    pub const SIZE: (u32, u32) = (1024, 576);

    pub const WALL_RGB: [u8; 3] = [12, 20, 28];
    pub const ROOF_RGB: [u8; 3] = [36, 20, 28];
    pub const WINDOW_RGB: [u8; 3] = [60, 44, 28];
    pub const DOOR_RGB: [u8; 3] = [84, 44, 52];

    fn png_of(img: impl Into<image::DynamicImage>) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        img.into()
            .write_to(&mut out, image::ImageFormat::Png)
            .expect("png");
        out.into_inner()
    }

    fn outline(img: &mut GrayImage, r: Rect) {
        for t in 0..2 {
            for x in r.x0..=r.x1 {
                img.put_pixel(x, r.y0 + t, Luma([0]));
                img.put_pixel(x, r.y1 - t, Luma([0]));
            }
            for y in r.y0..=r.y1 {
                img.put_pixel(r.x0 + t, y, Luma([0]));
                img.put_pixel(r.x1 - t, y, Luma([0]));
            }
        }
    }

    fn fill(img: &mut RgbImage, r: Rect, c: [u8; 3]) {
        for y in r.y0..=r.y1 {
            for x in r.x0..=r.x1 {
                img.put_pixel(x, y, Rgb(c));
            }
        }
    }

    /// The model's line map: the house face, its roof band, a window and a door.
    pub fn lines_map() -> Vec<u8> {
        let mut img = GrayImage::from_pixel(SIZE.0, SIZE.1, Luma([255]));
        for r in [WALL, ROOF, WINDOW, DOOR] {
            outline(&mut img, r);
        }
        png_of(img)
    }

    /// The ids map, with one color per element; black is the sky.
    pub fn ids_map() -> Vec<u8> {
        let mut img = RgbImage::from_pixel(SIZE.0, SIZE.1, Rgb([0, 0, 0]));
        fill(&mut img, WALL, WALL_RGB);
        fill(&mut img, ROOF, ROOF_RGB);
        fill(&mut img, WINDOW, WINDOW_RGB);
        fill(&mut img, DOOR, DOOR_RGB);
        png_of(img)
    }

    pub fn legend() -> Vec<LegendEntry> {
        let e = |c: [u8; 3], id: &str| LegendEntry {
            color: format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]),
            element_id: id.to_string(),
            pixels: 1,
        };
        vec![
            e(WALL_RGB, "wall-1"),
            e(ROOF_RGB, "roof-1"),
            e(WINDOW_RGB, "win-1"),
            e(DOOR_RGB, "door-1"),
        ]
    }

    /// A pretend AI photo of the house at another size: a sky gradient, a
    /// lawn with a busy texture, the face, the roof, the glass and the door,
    /// a soft shadow, all blurred a little the way a photo is. `window` is
    /// where the image puts the window.
    pub fn ai_photo(width: u32, height: u32, window: Rect) -> Vec<u8> {
        let (sx, sy) = (width as f32 / SIZE.0 as f32, height as f32 / SIZE.1 as f32);
        let scale = |r: Rect| Rect {
            x0: (r.x0 as f32 * sx) as u32,
            y0: (r.y0 as f32 * sy) as u32,
            x1: (r.x1 as f32 * sx) as u32,
            y1: (r.y1 as f32 * sy) as u32,
        };
        let mut img = RgbImage::new(width, height);
        let mut seed = 12345u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            ((seed >> 16) % 40) as i32 - 20
        };
        for y in 0..height {
            for x in 0..width {
                let t = y as f32 / height as f32;
                let c = if (y as f32) < 500.0 * sy {
                    [(150.0 + 60.0 * t) as u8, (190.0 + 40.0 * t) as u8, 235]
                } else {
                    let n = noise();
                    [
                        (70 + n).clamp(0, 255) as u8,
                        (120 + n).clamp(0, 255) as u8,
                        (60 + n / 2).clamp(0, 255) as u8,
                    ]
                };
                img.put_pixel(x, y, Rgb(c));
            }
        }
        fill(&mut img, scale(WALL), [222, 210, 188]);
        // A soft band of shade under the eaves, as a photo has.
        let shade = scale(Rect {
            x0: WALL.x0 + 3,
            y0: WALL.y0 + 3,
            x1: WALL.x1 - 3,
            y1: WALL.y0 + 30,
        });
        fill(&mut img, shade, [200, 190, 170]);
        fill(&mut img, scale(ROOF), [150, 72, 52]);
        fill(&mut img, scale(window), [58, 80, 104]);
        fill(&mut img, scale(DOOR), [104, 64, 40]);
        png_of(image::DynamicImage::ImageRgb8(img).blur(1.2 * sx))
    }

    pub fn weights(id: &str) -> Option<Weight> {
        Some(match id {
            "win-1" | "door-1" => Weight {
                priority: 6,
                scored: true,
            },
            "roof-1" => Weight {
                priority: 2,
                scored: true,
            },
            "wall-1" => Weight {
                priority: 1,
                scored: true,
            },
            _ => return None,
        })
    }

    pub fn names(id: &str) -> Option<Named> {
        Some(match id {
            "win-1" => Named {
                label: "window W1 on the Ground Floor".into(),
                key: true,
            },
            "door-1" => Named {
                label: "door D1 on the Ground Floor".into(),
                key: true,
            },
            "roof-1" => Named {
                label: "the roof".into(),
                key: false,
            },
            "wall-1" => Named {
                label: "a wall on the Ground Floor".into(),
                key: false,
            },
            _ => return None,
        })
    }

    fn run(ai: &[u8]) -> (Outcome, guhit_model::DriftReport) {
        let (lines, ids, legend) = (lines_map(), ids_map(), legend());
        let o = check(
            &Check {
                lines_png: &lines,
                ids: Some((&ids, &legend)),
                ai,
            },
            &weights,
        )
        .expect("checks");
        let r = report(&o, &names);
        (o, r)
    }

    #[test]
    fn the_same_house_is_not_flagged() {
        let (o, r) = run(&ai_photo(1536, 864, WINDOW));
        assert!(o.missing < 0.12, "missing {}", o.missing);
        assert!(!r.warn, "{}", r.note);
        assert!(o.elements.iter().all(|e| !e.moved()), "{:?}", o.elements);
        assert!(r.note.contains("line up"));
        assert!(o.shift_share() < 0.005, "lined up as drawn: {:?}", o.shift);
        assert!(!r.note.contains("framed"), "{}", r.note);
    }

    #[test]
    fn a_moved_window_is_flagged_and_named() {
        let (o, r) = run(&ai_photo(1536, 864, MOVED_WINDOW));
        assert!(r.warn, "{}", r.note);
        let window = o
            .elements
            .iter()
            .find(|e| e.element_id == "win-1")
            .expect("the window has edges");
        assert!(window.moved(), "{window:?}");
        assert!(r.note.contains("window W1"), "{}", r.note);
        assert!(!r.note.contains("door D1"), "the door stayed: {}", r.note);
        assert!(o.missing > 0.05);
        // The overlay is a PNG at working size with red where the window was.
        let overlay = image::load_from_memory(&o.overlay)
            .expect("overlay")
            .to_rgba8();
        assert_eq!(overlay.dimensions(), o.size);
        let (wx, wy) = (
            (WINDOW.x0 + WINDOW.x1) / 2 * o.size.0 / SIZE.0,
            WINDOW.y0 * o.size.1 / SIZE.1,
        );
        let red = (wy.saturating_sub(2)..wy + 3)
            .any(|y| overlay.get_pixel(wx, y).0 == [239, 68, 68, 255]);
        assert!(red, "the old window's top edge is red");
    }

    #[test]
    fn a_jpeg_answer_is_checked_too() {
        let png = ai_photo(1024, 576, WINDOW);
        let img = image::load_from_memory(&png).expect("png").to_rgb8();
        let mut jpeg = std::io::Cursor::new(Vec::new());
        img.write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .expect("jpeg");
        let (_, r) = run(&jpeg.into_inner());
        assert!(!r.warn, "{}", r.note);
    }

    #[test]
    fn a_slightly_reframed_image_lines_up_and_says_so() {
        // The same house, shifted 12 px right and 6 down at 1024 wide.
        let png = ai_photo(1024, 576, WINDOW);
        let src = image::load_from_memory(&png).expect("png").to_rgb8();
        let mut shifted = RgbImage::from_pixel(1024, 576, Rgb([170, 205, 235]));
        for y in 6..576 {
            for x in 12..1024 {
                shifted.put_pixel(x, y, *src.get_pixel(x - 12, y - 6));
            }
        }
        let (o, r) = run(&png_of(shifted));
        assert!(!r.warn, "{}", r.note);
        assert_ne!(o.shift, (0, 0));
        assert!(r.note.contains("framed"), "{}", r.note);
    }

    #[test]
    fn a_different_building_is_flagged() {
        // A flat grey picture with one unrelated box: almost nothing matches.
        let mut img = RgbImage::from_pixel(1024, 576, Rgb([128, 128, 128]));
        fill(
            &mut img,
            Rect {
                x0: 40,
                y0: 40,
                x1: 140,
                y1: 120,
            },
            [30, 30, 30],
        );
        let (o, r) = run(&png_of(img));
        assert!(o.missing > 0.8, "missing {}", o.missing);
        assert!(r.warn);
    }

    #[test]
    fn furniture_does_not_count() {
        // Mark the door as furniture: its edges leave the score.
        let (lines, ids, legend) = (lines_map(), ids_map(), legend());
        let ai = ai_photo(1024, 576, WINDOW);
        let w = |id: &str| {
            if id == "door-1" {
                Some(Weight {
                    priority: 5,
                    scored: false,
                })
            } else {
                weights(id)
            }
        };
        let o = check(
            &Check {
                lines_png: &lines,
                ids: Some((&ids, &legend)),
                ai: &ai,
            },
            &w,
        )
        .expect("checks");
        assert!(o.elements.iter().all(|e| e.element_id != "door-1"));
        let all = check(
            &Check {
                lines_png: &lines,
                ids: Some((&ids, &legend)),
                ai: &ai,
            },
            &weights,
        )
        .expect("checks");
        assert!(o.scored < all.scored);
    }

    #[test]
    fn without_an_ids_map_every_edge_counts() {
        let lines = lines_map();
        let ai = ai_photo(1024, 576, MOVED_WINDOW);
        let o = check(
            &Check {
                lines_png: &lines,
                ids: None,
                ai: &ai,
            },
            &weights,
        )
        .expect("checks");
        assert!(o.elements.is_empty());
        assert!(o.missing > 0.05);
    }

    #[test]
    fn too_little_in_view_is_said_plainly() {
        let blank = png_of(GrayImage::from_pixel(640, 360, Luma([255])));
        let ai = ai_photo(640, 360, WINDOW);
        let o = check(
            &Check {
                lines_png: &blank,
                ids: None,
                ai: &ai,
            },
            &weights,
        )
        .expect("checks");
        let r = report(&o, &names);
        assert!(!r.warn);
        assert!(r.note.contains("Too little"));
    }

    #[test]
    fn lists_read_naturally() {
        assert_eq!(join_and(&["a".into()]), "a");
        assert_eq!(join_and(&["a".into(), "b".into()]), "a and b");
        assert_eq!(
            join_and(&["a".into(), "b".into(), "c".into()]),
            "a, b and c"
        );
    }
}
