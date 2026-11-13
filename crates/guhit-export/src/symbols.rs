//! Plan symbols of the catalog's furniture, fixtures, plants, cars and
//! built-ins, as the 2D editor draws them (`src/editor2d/symbols.ts`,
//! `drawAssetSymbol`), shape for shape, so a sheet and the DXF print what
//! the screen shows instead of a labelled box (GA P1-5).
//!
//! Every symbol is in local mm: origin at the footprint center, +x along
//! the width, +y toward the back of the object (bed head, sofa back, WC
//! tank), as `Asset::rotation_deg` turns it into the plan. Lines only: a
//! sheet prints no white fills. Dashes (the wall cabinets over a kitchen
//! run, a wall shelf, a gate's swing) are drawn as dash pieces sized for
//! plotting at 1:n. Devices (lights, outlets) keep their box on the
//! architectural plan; the services sheets draw their symbols.

use guhit_model::{BuiltInKind, BuiltInParams};

use crate::geom::*;
use crate::plan::{Fill, Prim};

use std::f64::consts::PI;

/// The symbol of catalog item `key` at `w` by `d` mm with its built-in
/// options, and the label it carries ("REF"); None for a key that has no
/// symbol (it prints as a labelled box). Plotting at 1:`n` sizes dashes.
pub fn furniture(key: &str, w: f64, d: f64, built_in: Option<&BuiltInParams>, n: f64) -> Option<(Vec<Prim>, &'static str)> {
    if !(w >= 1.0 && d >= 1.0) {
        return None;
    }
    let mut s = Sketch { out: Vec::new(), dash: 2.2 * n.max(1.0) };
    let (x, y) = (-w / 2.0, -d / 2.0);
    if let Some(p) = built_in_params(key, w, built_in) {
        s.built_in(key, w, d, &p);
        return Some((s.out, ""));
    }
    match key {
        "bed-single" => s.bed(w, d, 1),
        "bed-double" | "bed-queen" => s.bed(w, d, 2),
        "wardrobe" => s.wardrobe(w, d),
        "sofa-3" | "sofa-leather" | "sofa-wood" => s.sofa(w, d, 3),
        "sofa-2" => s.sofa(w, d, 2),
        "armchair" | "lounge-chair" | "armchair-classic" => s.sofa(w, d, 1),
        "rocking-chair" => {
            // The chair between two bowed rockers that run the full depth.
            s.sofa(w - 150.0, d - 40.0, 1);
            for side in [-1.0, 1.0] {
                let rx = side * (w / 2.0 - 35.0);
                let pts = quad(v(rx, y + 20.0), v(rx + side * 50.0, 0.0), v(rx, d / 2.0 - 20.0));
                s.path(pts, false);
            }
        }
        "coffee-table" | "coffee-table-square" => s.table_top(w, d, 50.0),
        "tea-table" => s.table_top(w, d, 40.0),
        "side-table" => s.table_top(w, d, 30.0),
        "table-wood" => s.table_top(w, d, 20.0),
        "ottoman" => s.table_top(w, d, w.min(d) * 0.3),
        "coffee-table-round" => s.disc(w, d, 0.82),
        "side-table-round" => s.disc(w, d, 0.78),
        "stool-wood" => s.disc(w, d, 0.66),
        "vase-ceramic" => s.disc(w, d, 0.6),
        "dining-round-2" => s.bistro(d, w.min(d) * 0.36, w.min(d) * 0.35),
        "sideboard" => s.cabinet(w, d, ((w / 500.0).round() as usize).max(2), false),
        "cabinet-white" => s.cabinet(w, d, ((w / 600.0).round() as usize).max(2), false),
        "dresser" => s.cabinet(w, d, ((w / 400.0).round() as usize).max(2), true),
        "cabinet-drawers" => s.cabinet(w, d, ((w / 570.0).round() as usize).max(2), true),
        "nightstand" => s.cabinet(w, d, 1, true),
        "shelf-cube" => {
            s.rr(x, y, w, d, 20.0);
            s.rr(x + 40.0, y + 40.0, w - 80.0, d - 80.0, 0.0);
            s.line(x + 40.0, y + 40.0, x + w - 40.0, y + d - 40.0);
            s.line(x + 40.0, y + d - 40.0, x + w - 40.0, y + 40.0);
        }
        "shelf-steel" => {
            s.rr(x, y, w, d, 0.0);
            for f in [1.0 / 3.0, 2.0 / 3.0] {
                s.line(x, y + d * f, x + w, y + d * f);
            }
            let k = 60.0_f64.min(w.min(d) * 0.1);
            for sx in [-1.0, 1.0] {
                for sy in [-1.0, 1.0] {
                    s.circle(sx * (w / 2.0 - k), sy * (d / 2.0 - k), k * 0.6);
                }
            }
        }
        "basket-rattan" => {
            s.rr(x, y, w, d, w.min(d) * 0.3);
            s.rr(x + 35.0, y + 35.0, w - 70.0, d - 70.0, w.min(d) * 0.2);
            s.weave(x + 60.0, y + 60.0, x + w - 60.0, y + d - 60.0, 55.0);
        }
        "ceiling-fan" => s.fan(w, d, 5),
        "wall-art" | "mirror-wall" => {
            // Thin, on the wall face (its back at y = d/2).
            let t = d.max(20.0);
            s.rr(x, d / 2.0 - t, w, t, 0.0);
        }
        "chair-dining" | "chair-wood-white" | "chair-monobloc" => s.chair(0.0, 0.0, 0.0, w, d),
        "tv-console" => {
            s.rr(x, y, w, d, 0.0);
            // The screen, against the back.
            s.rr(-w * 0.36, d / 2.0 - 140.0, w * 0.72, 60.0, 0.0);
            s.line(-w * 0.12, d / 2.0 - 140.0, -w * 0.12, d / 2.0 - 250.0);
            s.line(w * 0.12, d / 2.0 - 140.0, w * 0.12, d / 2.0 - 250.0);
            s.line(-w * 0.12, d / 2.0 - 250.0, w * 0.12, d / 2.0 - 250.0);
        }
        "dining-4" => s.dining(w, d, 4),
        "dining-6" => s.dining(w, d, 6),
        "dining-set-4" => s.dining_set(w, d, 4),
        "dining-set-6" => s.dining_set(w, d, 6),
        "desk" | "desk-metal" => {
            s.chair(0.0, -d / 2.0 - 120.0, PI, 440.0, 440.0);
            s.rr(x, y, w, d, 0.0);
        }
        "wc" => {
            let tank = d * 0.27;
            s.rr(x, d / 2.0 - tank, w, tank, 30.0);
            let bowl = d - tank - 10.0;
            s.ellipse(0.0, d / 2.0 - tank - bowl / 2.0, w * 0.46, bowl / 2.0, 0.0);
            s.ellipse(0.0, d / 2.0 - tank - bowl * 0.52, w * 0.27, bowl * 0.3, 0.0);
        }
        "lavatory" => {
            s.rr(x, y, w, d, 60.0);
            s.ellipse(0.0, -d * 0.06, w * 0.36, d * 0.3, 0.0);
            s.circle(0.0, d * 0.36, 18.0);
        }
        "shower" => {
            s.rr(x, y, w, d, 0.0);
            s.line(x, y, x + w, y + d);
            s.line(x, y + d, x + w, y);
            s.circle(0.0, 0.0, 55.0);
            s.circle(x + w - 130.0, y + d - 130.0, 70.0);
        }
        "bathtub" => {
            s.rr(x, y, w, d, 40.0);
            s.rr(x + 70.0, y + 70.0, w - 140.0, d - 140.0, (d - 140.0) / 2.0);
            s.circle(x + w - 230.0, 0.0, 28.0);
        }
        "kitchen-counter" => {
            s.rr(x, y, w, d, 0.0);
            s.line(x, y + 40.0, x + w, y + 40.0);
        }
        "kitchen-sink" => {
            s.rr(x, y, w, d, 0.0);
            s.line(x, y + 40.0, x + w, y + 40.0);
            let bw = (w * 0.34).min(430.0);
            s.rr(-bw - 25.0, -d * 0.24, bw, d * 0.56, 50.0);
            s.rr(25.0, -d * 0.24, bw, d * 0.56, 50.0);
            s.circle(0.0, d * 0.4, 22.0);
            s.line(0.0, d * 0.4, 0.0, d * 0.16);
        }
        "range" | "range-electric" => {
            s.rr(x, y, w, d, 0.0);
            let r = w.min(d) * 0.15;
            for sx in [-1.0, 1.0] {
                for sy in [-1.0, 1.0] {
                    s.circle(sx * w * 0.23, sy * d * 0.2 + d * 0.05, r);
                    s.circle(sx * w * 0.23, sy * d * 0.2 + d * 0.05, r * 0.35);
                }
            }
            s.line(x, y + d * 0.1, x + w, y + d * 0.1);
        }
        "microwave" => {
            s.rr(x, y, w, d, 20.0);
            // The door with its window, and a control strip of three buttons.
            let strip = 110.0_f64.min(w * 0.22);
            s.rr(x + 30.0, y + 30.0, w - strip - 60.0, d - 60.0, 15.0);
            s.rr(x + w - strip - 5.0, y + 30.0, strip - 25.0, d - 60.0, 10.0);
            for f in [0.25, 0.5, 0.75] {
                s.line(x + w - strip + 10.0, y + d * f, x + w - 35.0, y + d * f);
            }
        }
        "refrigerator" => {
            s.rr(x, y, w, d, 20.0);
            s.line(x, y + 60.0, x + w, y + 60.0);
            s.line(x + w - 90.0, y + 60.0, x + w - 90.0, y + 10.0);
            return Some((s.out, "REF"));
        }
        "washing-machine" => {
            s.rr(x, y, w, d, 30.0);
            s.line(x, d / 2.0 - d * 0.18, x + w, d / 2.0 - d * 0.18);
            let r = w.min(d * 0.8) * 0.34;
            s.circle(0.0, -d * 0.08, r);
            s.circle(0.0, -d * 0.08, r * 0.7);
        }
        "plant-pot" => s.plant(w.min(d) / 2.0, 8, 0.45, 0.42, 0.16),
        "plant-leafy" => s.plant(w.min(d) / 2.0, 9, 0.24, 0.5, 0.2),
        "plant-succulent" => s.plant(w.min(d) / 2.0, 12, 0.2, 0.55, 0.13),
        "tree" | "tree-flowering" => s.tree(w.min(d) / 2.0),
        "car-sedan" => s.car(w, d),
        // The mirrors stand 90 mm out of the body: the body is narrower.
        "car-covered" => s.car(w - 180.0, d),
        "outdoor-set" => s.bistro(d, 440.0_f64.min(w * 0.56), w * 0.38),
        "bench-wood" => {
            s.rr(x, y, w, d, 20.0);
            // The back against +y, seat slats in front of it.
            s.rr(x, d / 2.0 - 90.0, w, 90.0, 20.0);
            for f in [1.0 / 3.0, 2.0 / 3.0] {
                s.line(x + 40.0, y + (d - 90.0) * f, x + w - 40.0, y + (d - 90.0) * f);
            }
        }
        "planter-box" => {
            s.rr(x, y, w, d, 20.0);
            s.rr(x + 50.0, y + 50.0, w - 100.0, d - 100.0, 10.0);
            let count = ((w / 220.0).round() as usize).max(2);
            let r = ((d - 100.0) * 0.3).min(70.0);
            for i in 0..count {
                let px = x + 50.0 + ((w - 100.0) / count as f64) * (i as f64 + 0.5);
                s.circle(px, 0.0, r);
                s.circle(px, 0.0, r * 0.35);
            }
        }
        "gate-iron" => {
            // Two leaves hinged on the posts, swinging into the lot (-y).
            let post = d.min(w / 10.0);
            s.rr(x, y, post, d, 0.0);
            s.rr(x + w - post, y, post, d, 0.0);
            let hx = w / 2.0 - post / 2.0;
            let len = hx - 10.0;
            s.line(-hx, 0.0, -10.0, 0.0);
            s.line(10.0, 0.0, hx, 0.0);
            s.dashed(arc(v(-hx, 0.0), len, -PI / 2.0, 0.0));
            s.dashed(arc(v(hx, 0.0), len, PI, PI * 1.5));
        }
        "gate-steel" => {
            // A sliding gate: the frame, a tick per bar, the slide arrow.
            s.rr(x, y, w, d, 0.0);
            let count = ((w / 150.0).round() as usize).max(2);
            for i in 1..count {
                let bx = x + (w / count as f64) * i as f64;
                s.line(bx, y, bx, -d * 0.18);
                s.line(bx, d * 0.18, bx, d / 2.0);
            }
            let head = (d * 0.8).min(80.0);
            s.line(-w * 0.3, 0.0, w * 0.3, 0.0);
            s.line(w * 0.3, 0.0, w * 0.3 - head, head * 0.45);
            s.line(w * 0.3, 0.0, w * 0.3 - head, -head * 0.45);
        }
        "hedge" => s.hedge(w, d),
        "lamp-post" | "lamp-post-classic" => {
            let r = w.min(d) / 2.0;
            let k = r * std::f64::consts::FRAC_1_SQRT_2;
            s.circle(0.0, 0.0, r);
            s.line(-k, -k, k, k);
            s.line(-k, k, k, -k);
            s.out.push(Prim::Circle { c: v(0.0, 0.0), r: (r * 0.12).max(10.0), fill: Fill::Ink });
        }
        _ => return None,
    }
    Some((s.out, ""))
}

/// A built-in's options: its own, or the defaults for its width, rule for
/// rule as `guhit_core::placement::built_in_defaults` and the 3D view's
/// `builtInDefaults` give them. None for a key that is not a built-in.
pub fn built_in_params(key: &str, w: f64, given: Option<&BuiltInParams>) -> Option<BuiltInParams> {
    let kind = match key {
        "kitchen-run" => BuiltInKind::KitchenRun,
        "wardrobe-builtin" => BuiltInKind::Wardrobe,
        "vanity" => BuiltInKind::Vanity,
        "counter" => BuiltInKind::Counter,
        "shelf-wall" => BuiltInKind::Shelf,
        _ => return None,
    };
    if let Some(p) = given {
        return Some(p.clone());
    }
    let w = if w.is_finite() && w > 0.0 { w } else { 600.0 };
    let round50 = |x: f64| (x / 50.0).round() * 50.0;
    let clamp = |x: f64, lo: f64, hi: f64| x.max(lo).min(hi.max(lo));
    let doors = |per: f64, min: u32| ((w / per).round() as u32).max(min);
    Some(match kind {
        BuiltInKind::KitchenRun => {
            let sink = if w >= 900.0 { round50(clamp(w * 0.3, 450.0, w - 450.0)) } else { round50(w / 2.0) };
            let mut cooktop = None;
            if w >= 1800.0 {
                let at = round50(clamp((w * 0.75).max(sink + 900.0), 350.0, w - 350.0));
                if (at - sink).abs() >= 600.0 {
                    cooktop = Some(at);
                }
            }
            BuiltInParams { upper: true, sink_at_mm: Some(sink), cooktop_at_mm: cooktop, doors: doors(450.0, 1) }
        }
        BuiltInKind::Vanity => BuiltInParams { upper: true, sink_at_mm: Some(round50(w / 2.0)), cooktop_at_mm: None, doors: doors(450.0, 1) },
        BuiltInKind::Counter => BuiltInParams { upper: false, sink_at_mm: None, cooktop_at_mm: None, doors: doors(450.0, 1) },
        BuiltInKind::Wardrobe => BuiltInParams { upper: false, sink_at_mm: None, cooktop_at_mm: None, doors: doors(500.0, 2) },
        BuiltInKind::Shelf => BuiltInParams { upper: false, sink_at_mm: None, cooktop_at_mm: None, doors: 0 },
    })
}

/// Points of a quadratic curve from a to c with control b.
fn quad(a: V, b: V, c: V) -> Vec<V> {
    (0..=8)
        .map(|i| {
            let t = i as f64 / 8.0;
            a * ((1.0 - t) * (1.0 - t)) + b * (2.0 * (1.0 - t) * t) + c * (t * t)
        })
        .collect()
}

/// Points of a circular arc from angle a0 to a1 (radians, counter-clockwise
/// when a1 > a0).
fn arc(c: V, r: f64, a0: f64, a1: f64) -> Vec<V> {
    let steps = (((a1 - a0).abs() / (PI / 16.0)).ceil() as usize).max(2);
    (0..=steps).map(|i| c + v((a0 + (a1 - a0) * i as f64 / steps as f64).cos(), (a0 + (a1 - a0) * i as f64 / steps as f64).sin()) * r).collect()
}

/// Points of an ellipse arc, turned by `rot`, from a0 to a1.
fn ellipse_arc(c: V, rx: f64, ry: f64, rot: f64, a0: f64, a1: f64, steps: usize) -> Vec<V> {
    let (cr, sr) = (rot.cos(), rot.sin());
    (0..=steps)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f64 / steps as f64;
            let (px, py) = (rx * a.cos(), ry * a.sin());
            c + v(px * cr - py * sr, px * sr + py * cr)
        })
        .collect()
}

/// The symbol being drawn, in local mm.
struct Sketch {
    out: Vec<Prim>,
    /// Dash length, model mm.
    dash: f64,
}

impl Sketch {
    fn line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) {
        self.out.push(Prim::Line { a: v(x1, y1), b: v(x2, y2) });
    }

    fn path(&mut self, pts: Vec<V>, closed: bool) {
        if pts.len() >= 2 {
            self.out.push(Prim::Poly { pts, closed, fill: Fill::None });
        }
    }

    /// A polyline as dashes.
    fn dashed(&mut self, pts: Vec<V>) {
        let (on, off) = (self.dash, self.dash * 0.5);
        let mut left = on;
        let mut drawing = true;
        for k in 0..pts.len().saturating_sub(1) {
            let (mut a, b) = (pts[k], pts[k + 1]);
            let Some(u) = (b - a).unit() else { continue };
            let mut rest = (b - a).len();
            while rest > 1e-9 {
                let step = left.min(rest);
                let next = a + u * step;
                if drawing {
                    self.out.push(Prim::Line { a, b: next });
                }
                a = next;
                rest -= step;
                left -= step;
                if left <= 1e-9 {
                    drawing = !drawing;
                    left = if drawing { on } else { off };
                }
            }
        }
    }

    /// The canvas's rounded rectangle: x, y its low corner, corners of
    /// radius r drawn as quadratic curves.
    fn rr(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        let k = r.min(w.abs() / 2.0).min(h.abs() / 2.0).max(0.0);
        if k < 1e-9 {
            self.path(vec![v(x, y), v(x + w, y), v(x + w, y + h), v(x, y + h)], true);
            return;
        }
        let mut pts = vec![v(x + k, y), v(x + w - k, y)];
        pts.extend(quad(v(x + w - k, y), v(x + w, y), v(x + w, y + k)).into_iter().skip(1));
        pts.push(v(x + w, y + h - k));
        pts.extend(quad(v(x + w, y + h - k), v(x + w, y + h), v(x + w - k, y + h)).into_iter().skip(1));
        pts.push(v(x + k, y + h));
        pts.extend(quad(v(x + k, y + h), v(x, y + h), v(x, y + h - k)).into_iter().skip(1));
        pts.push(v(x, y + k));
        pts.extend(quad(v(x, y + k), v(x, y), v(x + k, y)).into_iter().skip(1));
        pts.pop();
        self.path(pts, true);
    }

    fn circle(&mut self, cx: f64, cy: f64, r: f64) {
        if r > 1e-6 {
            self.out.push(Prim::Circle { c: v(cx, cy), r, fill: Fill::None });
        }
    }

    fn ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64, rot: f64) {
        let (rx, ry) = (rx.abs(), ry.abs());
        if (rx - ry).abs() < 1e-6 {
            self.circle(cx, cy, rx);
            return;
        }
        let mut pts = ellipse_arc(v(cx, cy), rx, ry, rot, 0.0, 2.0 * PI, 36);
        pts.pop();
        self.path(pts, true);
    }

    fn bed(&mut self, w: f64, d: f64, pillows: usize) {
        let (x, y) = (-w / 2.0, -d / 2.0);
        self.rr(x, y, w, d, 40.0);
        // The headboard strip at the back.
        self.line(x, d / 2.0 - 60.0, x + w, d / 2.0 - 60.0);
        let gap = 70.0;
        let pw = (w - gap * (pillows as f64 + 1.0)) / pillows as f64;
        let ph = 380.0_f64.min(d * 0.2);
        for i in 0..pillows {
            self.rr(x + gap + i as f64 * (pw + gap), d / 2.0 - 110.0 - ph, pw, ph, 70.0);
        }
        // The blanket's edge and its folded corner.
        let by = d / 2.0 - 110.0 - ph - 140.0;
        self.line(x, by, x + w, by);
        self.line(x + w, by - 380.0, x + w - 380.0, by);
    }

    fn sofa(&mut self, w: f64, d: f64, seats: usize) {
        let (x, y) = (-w / 2.0, -d / 2.0);
        let arm = 170.0_f64.min(w * 0.14);
        let back = 220.0_f64.min(d * 0.28);
        self.rr(x, y, w, d, 60.0);
        self.rr(x, d / 2.0 - back, w, back, 60.0);
        self.rr(x, y + 40.0, arm, d - back - 40.0, 50.0);
        self.rr(w / 2.0 - arm, y + 40.0, arm, d - back - 40.0, 50.0);
        let sw = (w - arm * 2.0) / seats as f64;
        for i in 1..seats {
            let sx = x + arm + sw * i as f64;
            self.line(sx, y + 30.0, sx, d / 2.0 - back);
        }
    }

    /// A chair turned by `angle` about (cx, cy), its back on its local +y.
    fn chair(&mut self, cx: f64, cy: f64, angle: f64, size: f64, depth: f64) {
        let mut c = Sketch { out: Vec::new(), dash: self.dash };
        c.rr(-size / 2.0, -depth / 2.0, size, depth, size.min(depth) * 0.18);
        c.line(-size / 2.0 + size * 0.1, depth / 2.0 - depth * 0.18, size / 2.0 - size * 0.1, depth / 2.0 - depth * 0.18);
        let (cos, sin) = (angle.cos(), angle.sin());
        let to = |q: V| v(cx + q.x * cos - q.y * sin, cy + q.x * sin + q.y * cos);
        for prim in c.out {
            self.out.push(map_prim(&prim, &to));
        }
    }

    fn dining(&mut self, w: f64, d: f64, seats: usize) {
        let (cs, out) = (420.0, 230.0);
        let step = w / 2.0;
        for i in 0..2 {
            let cx = -w / 2.0 + step * (i as f64 + 0.5);
            self.chair(cx, d / 2.0 + out - cs / 2.0, 0.0, cs, cs);
            self.chair(cx, -d / 2.0 - out + cs / 2.0, PI, cs, cs);
        }
        if seats == 6 {
            self.chair(w / 2.0 + out - cs / 2.0, 0.0, -PI / 2.0, cs, cs);
            self.chair(-w / 2.0 - out + cs / 2.0, 0.0, PI / 2.0, cs, cs);
        }
        self.rr(-w / 2.0, -d / 2.0, w, d, 30.0);
    }

    fn dining_set(&mut self, w: f64, d: f64, seats: usize) {
        let zone = 400.0_f64.min(d * 0.25);
        let td = d - zone * 2.0;
        let per = seats / 2;
        let step = w / per as f64;
        let cs = 420.0_f64.min(step * 0.85);
        for i in 0..per {
            let cx = -w / 2.0 + step * (i as f64 + 0.5);
            self.chair(cx, td / 2.0 + zone - cs / 2.0, 0.0, cs, cs);
            self.chair(cx, -td / 2.0 - zone + cs / 2.0, PI, cs, cs);
        }
        self.rr(-w / 2.0, -td / 2.0, w, td, 30.0);
    }

    fn bistro(&mut self, d: f64, cs: f64, tr: f64) {
        self.chair(0.0, d / 2.0 - cs / 2.0, 0.0, cs, cs);
        self.chair(0.0, -d / 2.0 + cs / 2.0, PI, cs, cs);
        self.circle(0.0, 0.0, tr);
        self.circle(0.0, 0.0, tr * 0.78);
    }

    fn tree(&mut self, r: f64) {
        let lobes = 11.0;
        let pts: Vec<V> = (0..88)
            .map(|i| {
                let a = i as f64 / 88.0 * 2.0 * PI;
                let k = r * (0.9 + 0.1 * (a * lobes).cos());
                v(a.cos() * k, a.sin() * k)
            })
            .collect();
        self.path(pts, true);
        for i in 0..7 {
            let a = i as f64 / 7.0 * 2.0 * PI + 0.3;
            self.line(a.cos() * r * 0.12, a.sin() * r * 0.12, a.cos() * r * 0.62, a.sin() * r * 0.62);
        }
        self.out.push(Prim::Circle { c: v(0.0, 0.0), r: (r * 0.07).max(10.0), fill: Fill::Ink });
    }

    fn car(&mut self, w: f64, d: f64) {
        let (x, y) = (-w / 2.0, -d / 2.0);
        self.rr(x, y, w, d, w * 0.32);
        // The front is local -y.
        let (hood, trunk, glass, inset) = (d * 0.24, d * 0.17, d * 0.12, w * 0.12);
        self.rr(x + inset, y + hood, w - inset * 2.0, d - hood - trunk, w * 0.12);
        self.path(vec![v(x + inset, y + hood), v(x + inset * 1.7, y + hood + glass), v(w / 2.0 - inset * 1.7, y + hood + glass), v(w / 2.0 - inset, y + hood)], false);
        self.path(
            vec![
                v(x + inset, d / 2.0 - trunk),
                v(x + inset * 1.7, d / 2.0 - trunk - glass * 0.8),
                v(w / 2.0 - inset * 1.7, d / 2.0 - trunk - glass * 0.8),
                v(w / 2.0 - inset, d / 2.0 - trunk),
            ],
            false,
        );
        // The mirrors.
        self.rr(x - 90.0, y + hood + 60.0, 90.0, 160.0, 30.0);
        self.rr(w / 2.0, y + hood + 60.0, 90.0, 160.0, 30.0);
    }

    fn table_top(&mut self, w: f64, d: f64, r: f64) {
        let k = 70.0_f64.min(w.min(d) * 0.15);
        self.rr(-w / 2.0, -d / 2.0, w, d, r);
        self.rr(-w / 2.0 + k, -d / 2.0 + k, w - k * 2.0, d - k * 2.0, r * 0.6);
    }

    fn disc(&mut self, w: f64, d: f64, inner: f64) {
        self.ellipse(0.0, 0.0, w / 2.0, d / 2.0, 0.0);
        self.ellipse(0.0, 0.0, w / 2.0 * inner, d / 2.0 * inner, 0.0);
    }

    fn plant(&mut self, r: f64, leaves: usize, core: f64, len: f64, wid: f64) {
        self.circle(0.0, 0.0, r * core);
        for i in 0..leaves {
            let a = i as f64 / leaves as f64 * 2.0 * PI;
            self.ellipse(a.cos() * r * (1.0 - len), a.sin() * r * (1.0 - len), r * len, r * wid, a);
        }
    }

    fn cabinet(&mut self, w: f64, d: f64, cells: usize, knobs: bool) {
        let (x, y) = (-w / 2.0, -d / 2.0);
        self.rr(x, y, w, d, 20.0);
        self.line(x, 0.0, x + w, 0.0);
        let cw = w / cells as f64;
        for i in 1..cells {
            self.line(x + cw * i as f64, y, x + cw * i as f64, 0.0);
        }
        if knobs {
            let r = 22.0_f64.min(d * 0.05);
            for i in 0..cells {
                self.circle(x + cw * (i as f64 + 0.5), y + d * 0.22, r);
            }
        }
    }

    fn weave(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, gap: f64) {
        let mut c = x0 + y0 + gap;
        while c < x1 + y1 {
            let a = x0.max(c - y1);
            let b = x1.min(c - y0);
            if b - a > 1.0 {
                self.line(a, c - a, b, c - b);
            }
            c += gap;
        }
    }

    fn wardrobe(&mut self, w: f64, d: f64) {
        let (x, y) = (-w / 2.0, -d / 2.0);
        self.rr(x, y, w, d, 0.0);
        self.line(x + 40.0, 0.0, x + w - 40.0, 0.0);
        let count = ((w / 170.0).round() as usize).max(3);
        for i in 1..count {
            let hx = x + w / count as f64 * i as f64;
            self.line(hx - 40.0, -d * 0.32, hx + 40.0, d * 0.32);
        }
    }

    fn hedge(&mut self, w: f64, d: f64) {
        let a = (d * 0.12).min(70.0);
        let yb = d / 2.0 - a;
        let cap = (d / 2.0).min(w * 0.2);
        let (x0, x1) = (-w / 2.0 + cap, w / 2.0 - cap);
        let count = (((x1 - x0) / (d * 0.34)).round() as usize).max(2);
        let p = (x1 - x0) / count as f64;
        let r = p * p / 4.0 / (2.0 * a) + a / 2.0;
        let beta = (r - a).atan2(p / 2.0);
        let mut pts = vec![v(x0, yb)];
        for i in 0..count {
            pts.extend(arc(v(x0 + p * (i as f64 + 0.5), yb + a - r), r, PI - beta, beta).into_iter().skip(1));
        }
        pts.extend(ellipse_arc(v(x1, 0.0), cap, yb, 0.0, PI / 2.0, -PI / 2.0, 12).into_iter().skip(1));
        for i in (0..count).rev() {
            pts.extend(arc(v(x0 + p * (i as f64 + 0.5), -(yb + a - r)), r, -beta, beta - PI).into_iter().skip(1));
        }
        pts.extend(ellipse_arc(v(x0, 0.0), cap, yb, 0.0, -PI / 2.0, -1.5 * PI, 12).into_iter().skip(1));
        pts.pop();
        self.path(pts, true);
    }

    fn fan(&mut self, w: f64, d: f64, blades: usize) {
        let ro = w.min(d) / 2.0;
        let rh = ro * 0.14;
        let mid = (rh + ro) / 2.0;
        self.circle(0.0, 0.0, rh);
        for i in 0..blades {
            let a = i as f64 / blades as f64 * 2.0 * PI + PI / 2.0;
            self.ellipse(a.cos() * mid, a.sin() * mid, (ro - rh) / 2.0, ro * 0.09, a);
        }
    }

    fn joints(&mut self, w: f64, d: f64, count: u32) {
        let len = 70.0_f64.min(d * 0.2);
        for i in 1..count {
            let jx = -w / 2.0 + w / count as f64 * i as f64;
            self.line(jx, -d / 2.0, jx, -d / 2.0 + len);
        }
    }

    fn sink_bowls(&mut self, cx: f64, d: f64, sw: f64) {
        let bowls = if sw >= 600.0 { 2 } else { 1 };
        let gap = 30.0;
        let bw = (sw - gap * (bowls as f64 - 1.0)) / bowls as f64;
        for i in 0..bowls {
            self.rr(cx - sw / 2.0 + i as f64 * (bw + gap), -d * 0.26, bw, d * 0.5, 50.0);
        }
        let r = 24.0_f64.min(d * 0.04);
        self.circle(cx, d * 0.36, r);
        self.line(cx, d * 0.36 - r, cx, d * 0.16);
    }

    fn cooktop(&mut self, cx: f64, d: f64, cw: f64) {
        let (ch, cy) = (d * 0.66, d * 0.04);
        self.rr(cx - cw / 2.0, cy - ch / 2.0, cw, ch, 30.0);
        let r = cw.min(ch) * 0.19;
        for sx in [-1.0, 1.0] {
            for sy in [-1.0, 1.0] {
                self.circle(cx + sx * cw * 0.24, cy + sy * ch * 0.23, r);
            }
        }
    }

    /// A built-in at its own size: a base run with doors across the front,
    /// the sink and cooktop where its options put them, a dashed line where
    /// the wall cabinets above stop; a wall shelf dashed.
    fn built_in(&mut self, key: &str, w: f64, d: f64, p: &BuiltInParams) {
        let (x, y) = (-w / 2.0, -d / 2.0);
        let doors = p.doors.max(1);
        let at = |mm: f64, half: f64| x + mm.max(half + 20.0).min(w - half - 20.0);
        match key {
            "kitchen-run" => {
                self.rr(x, y, w, d, 0.0);
                self.line(x, y + 40.0, x + w, y + 40.0);
                self.joints(w, d, doors);
                let sw = 800.0_f64.min(w - 80.0);
                if let Some(s) = p.sink_at_mm.filter(|_| sw >= 200.0) {
                    self.sink_bowls(at(s, sw / 2.0), d, sw);
                }
                let cw = 560.0_f64.min(w - 40.0);
                if let Some(c) = p.cooktop_at_mm.filter(|_| cw >= 200.0) {
                    self.cooktop(at(c, cw / 2.0), d, cw);
                }
                // The wall cabinets above reach 350 mm out from the wall.
                let uy = d / 2.0 - 350.0;
                if p.upper && uy > y + 1.0 {
                    self.dashed(vec![v(x, uy), v(x + w, uy)]);
                }
            }
            "counter" => {
                self.rr(x, y, w, d, 0.0);
                self.line(x, y + 40.0, x + w, y + 40.0);
                self.joints(w, d, doors);
            }
            "vanity" => {
                self.rr(x, y, w, d, 20.0);
                self.line(x, y + 40.0, x + w, y + 40.0);
                self.joints(w, d, doors);
                let rx = (w * 0.3).min(260.0);
                let bx = at(p.sink_at_mm.unwrap_or(w / 2.0), rx);
                self.ellipse(bx, -d * 0.04, rx, (d * 0.28).min(160.0), 0.0);
                self.circle(bx, d / 2.0 - 60.0_f64.min(d * 0.12), 18.0_f64.min(d * 0.04));
            }
            "wardrobe-builtin" => {
                self.wardrobe(w, d);
                self.joints(w, d, doors);
            }
            "shelf-wall" => {
                // Above the floor: dashed, the shelves parallel to the wall.
                self.dashed(vec![v(x, y), v(x + w, y), v(x + w, y + d), v(x, y + d), v(x, y)]);
                let count = if d >= 300.0 { 3 } else { 2 };
                for i in 1..=count {
                    let sy = y + d * i as f64 / (count as f64 + 1.0);
                    self.dashed(vec![v(x, sy), v(x + w, sy)]);
                }
            }
            _ => self.rr(x, y, w, d, 0.0),
        }
    }
}

/// A primitive moved through `to`.
pub fn map_prim(p: &Prim, to: &dyn Fn(V) -> V) -> Prim {
    match p {
        Prim::Line { a, b } => Prim::Line { a: to(*a), b: to(*b) },
        Prim::Poly { pts, closed, fill } => Prim::Poly { pts: pts.iter().map(|q| to(*q)).collect(), closed: *closed, fill: *fill },
        Prim::Circle { c, r, fill } => Prim::Circle { c: to(*c), r: *r, fill: *fill },
        other => other.clone(),
    }
}
