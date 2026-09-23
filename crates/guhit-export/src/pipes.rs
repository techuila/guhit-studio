//! Pipes in the exports: the styling every format shares, the plan view of a
//! pipe (runs and risers) and the 3D tube.
//!
//! Guhit coordinates pipes, it does not size them. Nothing here computes a
//! size, a flow or a required slope: a pipe is drawn as it was modelled.
//!
//! Plan conventions, the same on the sheet and in the 2D DXF:
//! - A segment that runs vertically (a riser or a drop) shows as a small
//!   circle at its plan position. Everything else is a line.
//! - Cold and hot water are solid, drainage is dashed, vent is dash-dot.
//! - On the sheet the line weight is the pipe size at the sheet scale, never
//!   thinner than `paper::STROKE_MIN`.

use guhit_model::{Element, Level, Pipe, PipeMaterial, PipeSystem, Project};

use crate::geom::*;

/// Every system in the order legends and layer tables list them.
pub const SYSTEMS: [PipeSystem; 4] = [
    PipeSystem::ColdWater,
    PipeSystem::HotWater,
    PipeSystem::Drainage,
    PipeSystem::Vent,
];

/// Back to front on the sheet: the wide drainage lines go first so the thin
/// supply lines stay readable on top of them.
pub const DRAW_ORDER: [PipeSystem; 4] = [
    PipeSystem::Drainage,
    PipeSystem::Vent,
    PipeSystem::ColdWater,
    PipeSystem::HotWater,
];

pub fn label(system: PipeSystem) -> &'static str {
    match system {
        PipeSystem::ColdWater => "Cold water",
        PipeSystem::HotWater => "Hot water",
        PipeSystem::Drainage => "Drainage",
        PipeSystem::Vent => "Vent",
    }
}

/// The token colors `--pipe-cold`, `--pipe-hot`, `--pipe-drain`, `--pipe-vent`
/// in `src/styles/tokens.css`.
pub fn color(system: PipeSystem) -> &'static str {
    match system {
        PipeSystem::ColdWater => "#2b7bd0",
        PipeSystem::HotWater => "#e0563a",
        PipeSystem::Drainage => "#9b6a35",
        PipeSystem::Vent => "#3a9a5c",
    }
}

/// Short name for SVG ids.
pub fn slug(system: PipeSystem) -> &'static str {
    match system {
        PipeSystem::ColdWater => "cold-water",
        PipeSystem::HotWater => "hot-water",
        PipeSystem::Drainage => "drainage",
        PipeSystem::Vent => "vent",
    }
}

/// DXF layer, AIA/NCS style.
pub fn dxf_layer(system: PipeSystem) -> &'static str {
    match system {
        PipeSystem::ColdWater => "P-DOMW-CPIP",
        PipeSystem::HotWater => "P-DOMW-HPIP",
        PipeSystem::Drainage => "P-SANR-PIPE",
        PipeSystem::Vent => "P-SANR-VENT",
    }
}

/// AutoCAD color index closest to the system color in hue and lightness.
/// R12 DXF has no true color.
pub fn dxf_color(system: PipeSystem) -> i64 {
    match system {
        PipeSystem::ColdWater => 150,
        PipeSystem::HotWater => 20,
        PipeSystem::Drainage => 33,
        PipeSystem::Vent => 103,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dash {
    Solid,
    Dashed,
    DashDot,
}

pub fn dash(system: PipeSystem) -> Dash {
    match system {
        PipeSystem::ColdWater | PipeSystem::HotWater => Dash::Solid,
        PipeSystem::Drainage => Dash::Dashed,
        PipeSystem::Vent => Dash::DashDot,
    }
}

/// Dash pattern in paper mm for a line `width` mm wide: dash, gap, and for
/// dash-dot a short dash and a second gap. Wider lines get longer dashes so
/// the pattern still reads. Empty for a solid line.
pub fn dash_pattern(d: Dash, width: f64) -> Vec<f64> {
    let k = (width / paper::STROKE_MIN).max(1.0).sqrt();
    match d {
        Dash::Solid => vec![],
        Dash::Dashed => vec![2.2 * k, 1.1 * k],
        Dash::DashDot => vec![3.0 * k, 0.9 * k, 0.4 * k, 0.9 * k],
    }
}

/// DXF linetype of a dash style: name, description and the R12 pattern at
/// 1:1 in paper mm (positive dash, negative gap, zero a dot). None for solid.
pub fn dxf_linetype(d: Dash) -> Option<(&'static str, &'static str, [f64; 4], usize)> {
    match d {
        Dash::Solid => None,
        Dash::Dashed => Some(("GUHIT_DASHED", "Dashed __ __ __", [2.2, -1.1, 0.0, 0.0], 2)),
        Dash::DashDot => Some((
            "GUHIT_DASHDOT",
            "Dash dot __ . __ .",
            [3.0, -0.9, 0.0, -0.9],
            4,
        )),
    }
}

pub fn material_label(material: PipeMaterial) -> &'static str {
    match material {
        PipeMaterial::Ppr => "PPR",
        PipeMaterial::Upvc => "uPVC",
        PipeMaterial::Gi => "GI",
        PipeMaterial::Pe => "PE",
        PipeMaterial::Copper => "Copper",
    }
}

/// IfcDistributionSystemEnum value.
pub fn ifc_system(system: PipeSystem) -> &'static str {
    match system {
        PipeSystem::ColdWater => ".DOMESTICCOLDWATER.",
        PipeSystem::HotWater => ".DOMESTICHOTWATER.",
        PipeSystem::Drainage => ".DRAINAGE.",
        PipeSystem::Vent => ".VENT.",
    }
}

/// Size as a short number: 20, 22.5.
pub fn size_label(mm: f64) -> String {
    let s = format!("{:.1}", mm);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The pipe's own name, or "Cold water 20 mm" when it has none.
pub fn display_name(pipe: &Pipe) -> String {
    let name = crate::text::clean(&pipe.name);
    if name.is_empty() {
        format!("{} {} mm", label(pipe.system), size_label(pipe.diameter_mm))
    } else {
        name
    }
}

// ------------------------------------------------------------------ 3D points

/// A point or vector in model mm: plan x and y, z up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub fn v3(x: f64, y: f64, z: f64) -> V3 {
    V3 { x, y, z }
}

impl std::ops::Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl std::ops::Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl std::ops::Mul<f64> for V3 {
    type Output = V3;
    fn mul(self, k: f64) -> V3 {
        v3(self.x * k, self.y * k, self.z * k)
    }
}

impl V3 {
    pub fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: V3) -> V3 {
        v3(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    pub fn len(self) -> f64 {
        self.dot(self).sqrt()
    }

    pub fn unit(self) -> Option<V3> {
        let l = self.len();
        if l < 1e-9 || !l.is_finite() {
            None
        } else {
            Some(self * (1.0 / l))
        }
    }

    pub fn plan(self) -> V {
        v(self.x, self.y)
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

/// A unit vector at right angles to the unit vector `d`. Horizontal for any
/// pipe that is not vertical, so tube sides and IFC profiles sit the same way
/// on every run.
pub fn perpendicular(d: V3) -> V3 {
    v3(0.0, 0.0, 1.0)
        .cross(d)
        .unit()
        .unwrap_or_else(|| v3(1.0, 0.0, 0.0))
}

/// The centerline with non-finite points and repeats dropped.
pub fn points(pipe: &Pipe) -> Vec<V3> {
    let mut out: Vec<V3> = Vec::with_capacity(pipe.points.len());
    for p in &pipe.points {
        let q = v3(p.x, p.y, p.z);
        if !q.is_finite() {
            continue;
        }
        if let Some(last) = out.last() {
            if (q - *last).len() < 0.01 {
                continue;
            }
        }
        out.push(q);
    }
    out
}

/// The size a pipe is drawn at, or None when it cannot be drawn.
pub fn diameter(pipe: &Pipe) -> Option<f64> {
    let d = pipe.diameter_mm;
    (d.is_finite() && d > 0.0).then_some(d)
}

/// True when a segment runs vertically: under 1 mm apart in plan, or at most
/// 50 mm apart and rising at least ten times that. The plan shows it as a
/// riser mark instead of a line.
pub fn is_vertical(a: V3, b: V3) -> bool {
    let h = (b.plan() - a.plan()).len();
    let rise = (b.z - a.z).abs();
    h < 1.0 || (h <= 50.0 && rise >= 10.0 * h)
}

// ------------------------------------------------------------------ plan view

/// Sizes of pipe marks on the printed sheet, in paper mm.
pub mod paper {
    /// Thinnest pipe line, so a 20 mm pipe still reads at 1:100.
    pub const STROKE_MIN: f64 = 0.35;
    /// Smallest riser circle radius.
    pub const RISER_MIN_R: f64 = 1.1;
    /// Outline of a riser circle.
    pub const RISER_STROKE: f64 = 0.3;
}

/// One pipe as the plan shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanPipe {
    pub id: String,
    pub system: PipeSystem,
    pub diameter_mm: f64,
    /// Horizontal and sloped stretches, as plan polylines.
    pub runs: Vec<Vec<V>>,
    /// Plan positions of vertical segments, one per position.
    pub risers: Vec<V>,
}

/// Plan view of one pipe, or None when nothing of it can be drawn.
pub fn plan_view(pipe: &Pipe) -> Option<PlanPipe> {
    let d = diameter(pipe)?;
    let pts = points(pipe);
    if pts.len() < 2 {
        return None;
    }
    let mut runs: Vec<Vec<V>> = Vec::new();
    let mut risers: Vec<V> = Vec::new();
    let mut cur: Vec<V> = Vec::new();
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if is_vertical(a, b) {
            if cur.len() >= 2 {
                runs.push(std::mem::take(&mut cur));
            }
            cur.clear();
            let c = (a.plan() + b.plan()) * 0.5;
            if !risers.iter().any(|r| (*r - c).len() < 1.0) {
                risers.push(c);
            }
        } else {
            if cur.is_empty() {
                cur.push(a.plan());
            }
            cur.push(b.plan());
        }
    }
    if cur.len() >= 2 {
        runs.push(cur);
    }
    if runs.is_empty() && risers.is_empty() {
        return None;
    }
    Some(PlanPipe {
        id: pipe.id.clone(),
        system: pipe.system,
        diameter_mm: d,
        runs,
        risers,
    })
}

/// The pipes of one level on visible pipe layers, in element order.
pub fn plan_pipes(project: &Project, level: &Level) -> Vec<PlanPipe> {
    project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Pipe(p)
                if p.level_id == level.id
                    && crate::plan::layer_visible(project, p.system.layer()) =>
            {
                plan_view(p)
            }
            _ => None,
        })
        .collect()
}

/// Line weight on paper, mm: the pipe size at 1:n, never under the minimum.
pub fn stroke_width(diameter_mm: f64, n: f64) -> f64 {
    (diameter_mm / n).max(paper::STROKE_MIN)
}

/// Riser circle radius in model mm at 1:n. Always wider than the line it
/// ends, so the mark reads as a symbol.
pub fn riser_radius(diameter_mm: f64, n: f64) -> f64 {
    stroke_width(diameter_mm, n).max(paper::RISER_MIN_R) * n
}

/// Model extent of the pipe marks at 1:n.
pub fn bounds(pipes: &[PlanPipe], n: f64) -> Bounds {
    let mut b = Bounds::empty();
    for p in pipes {
        for run in &p.runs {
            for q in run {
                b.add(*q);
            }
        }
        let r = riser_radius(p.diameter_mm, n);
        for c in &p.risers {
            b.add(*c - v(r, r));
            b.add(*c + v(r, r));
        }
    }
    b
}

/// Systems that have something drawn, in legend order.
pub fn systems_present(pipes: &[PlanPipe]) -> Vec<PipeSystem> {
    SYSTEMS
        .iter()
        .copied()
        .filter(|s| pipes.iter().any(|p| p.system == *s))
        .collect()
}

// ---------------------------------------------------------------------- tubes

/// Sides of an exported tube.
pub const TUBE_SIDES: usize = 8;

/// Closed tube around a pipe centerline as quads (a triangle repeats its last
/// corner), in model mm with `z0` added to every height.
///
/// Two segments meet on their mitre plane when the bend is 120 degrees or
/// less and the mitre stays short against both segments, so a bend is one
/// continuous surface without a notch. Other joints are cut square and
/// capped. Every piece is closed at both ends.
pub fn tube_quads(pipe: &Pipe, z0: f64, sides: usize) -> Vec<[V3; 4]> {
    let Some(d) = diameter(pipe) else {
        return Vec::new();
    };
    let r = d / 2.0;
    let pts: Vec<V3> = points(pipe)
        .into_iter()
        .map(|p| p + v3(0.0, 0.0, z0))
        .collect();
    tube_from_points(&pts, r, sides)
}

pub fn tube_from_points(pts: &[V3], r: f64, sides: usize) -> Vec<[V3; 4]> {
    let sides = sides.max(3);
    if pts.len() < 2 || !r.is_finite() || r <= 0.0 {
        return Vec::new();
    }
    let segs: Vec<(V3, V3, V3, f64)> = pts
        .windows(2)
        .filter_map(|w| {
            let len = (w[1] - w[0]).len();
            (w[1] - w[0]).unit().map(|dir| (w[0], w[1], dir, len))
        })
        .collect();
    if segs.is_empty() {
        return Vec::new();
    }

    // Mitre normal of the joint after segment i, when that joint is mitred.
    let mitre: Vec<Option<V3>> = (0..segs.len())
        .map(|i| {
            let next = segs.get(i + 1)?;
            let (d1, d2) = (segs[i].2, next.2);
            let n = (d1 + d2).unit()?;
            let cos_half = d1.dot(n);
            // Bends past 120 degrees would give a long spike.
            if cos_half < 0.5 {
                return None;
            }
            let reach = r * (1.0 - cos_half * cos_half).max(0.0).sqrt() / cos_half;
            (reach < 0.45 * segs[i].3.min(next.3)).then_some(n)
        })
        .collect();

    // Frames carried along the run. Across a mitre the frame is reflected in
    // the mitre plane, which is the rotation-minimising frame, so the rings
    // of both segments land on the same points of the joint.
    let mut frames: Vec<(V3, V3)> = Vec::with_capacity(segs.len());
    let mut u = perpendicular(segs[0].2);
    for (i, seg) in segs.iter().enumerate() {
        if i > 0 {
            u = match mitre[i - 1] {
                Some(n) => u - n * (2.0 * u.dot(n)),
                None => {
                    let p = u - seg.2 * u.dot(seg.2);
                    p.unit().unwrap_or_else(|| perpendicular(seg.2))
                }
            };
        }
        frames.push((u, seg.2.cross(u)));
    }

    let ring = |center: V3, dir: V3, (u, w): (V3, V3), plane: Option<V3>| -> Vec<V3> {
        (0..sides)
            .map(|k| {
                let a = std::f64::consts::TAU * (k as f64 + 0.5) / sides as f64;
                let off = u * (r * a.cos()) + w * (r * a.sin());
                let q = center + off;
                match plane {
                    Some(n) => q - dir * (off.dot(n) / dir.dot(n)),
                    None => q,
                }
            })
            .collect()
    };

    let mut out: Vec<[V3; 4]> = Vec::new();
    for (i, &(a, b, dir, _)) in segs.iter().enumerate() {
        let start_plane = if i > 0 { mitre[i - 1] } else { None };
        let end_plane = mitre[i];
        let s = ring(a, dir, frames[i], start_plane);
        let e = ring(b, dir, frames[i], end_plane);
        for k in 0..sides {
            let k1 = (k + 1) % sides;
            out.push([s[k], s[k1], e[k1], e[k]]);
        }
        if start_plane.is_none() {
            cap(&s, false, &mut out);
        }
        if end_plane.is_none() {
            cap(&e, true, &mut out);
        }
    }
    out
}

/// Flat cap over a ring as quads fanned from the first corner. `forward`
/// faces along the ring's own winding normal.
fn cap(ring: &[V3], forward: bool, out: &mut Vec<[V3; 4]>) {
    let pts: Vec<V3> = if forward {
        ring.to_vec()
    } else {
        ring.iter().rev().copied().collect()
    };
    let mut i = 1;
    while i + 1 < pts.len() {
        let c = pts[i + 1];
        let dd = if i + 2 < pts.len() { pts[i + 2] } else { c };
        out.push([pts[0], pts[i], c, dd]);
        i += 2;
    }
}

/// Faces one tube piece of `segments` straight segments has, with both caps.
pub fn tube_face_count(segments: usize, sides: usize) -> usize {
    segments * sides + 2 * (sides - 2).div_ceil(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pipe(points: &[(f64, f64, f64)], system: PipeSystem, d: f64) -> Pipe {
        Pipe {
            id: "p".into(),
            level_id: "l".into(),
            system,
            material: PipeMaterial::Upvc,
            diameter_mm: d,
            points: points
                .iter()
                .map(|p| guhit_model::Vec3 {
                    x: p.0,
                    y: p.1,
                    z: p.2,
                })
                .collect(),
            name: String::new(),
        }
    }

    #[test]
    fn plan_view_splits_runs_at_risers() {
        // Up, across, across, up: two risers and one plan polyline.
        let p = pipe(
            &[
                (600.0, -1800.0, 0.0),
                (600.0, -1800.0, -300.0),
                (600.0, 5850.0, -300.0),
                (2000.0, 5850.0, -300.0),
                (2000.0, 6000.0, -300.0),
                (2000.0, 6000.0, 300.0),
            ],
            PipeSystem::ColdWater,
            25.0,
        );
        let pv = plan_view(&p).unwrap();
        assert_eq!(pv.risers, vec![v(600.0, -1800.0), v(2000.0, 6000.0)]);
        assert_eq!(pv.runs.len(), 1);
        assert_eq!(pv.runs[0].len(), 4);
        // A pipe that only drops is a riser mark alone.
        let drop = pipe(
            &[(6780.0, 5650.0, 20.0), (6780.0, 5650.0, -430.0)],
            PipeSystem::Drainage,
            100.0,
        );
        let pv = plan_view(&drop).unwrap();
        assert!(pv.runs.is_empty());
        assert_eq!(pv.risers.len(), 1);
    }

    #[test]
    fn a_sloped_drain_is_not_a_riser() {
        assert!(!is_vertical(v3(0.0, 0.0, -300.0), v3(4300.0, 0.0, -334.0)));
        assert!(is_vertical(v3(0.0, 0.0, 0.0), v3(0.0, 0.5, 900.0)));
        assert!(is_vertical(v3(0.0, 0.0, 0.0), v3(20.0, 0.0, 900.0)));
        assert!(!is_vertical(v3(0.0, 0.0, 0.0), v3(80.0, 0.0, 900.0)));
    }

    #[test]
    fn names_and_sizes() {
        let mut p = pipe(
            &[(0.0, 0.0, 0.0), (1000.0, 0.0, 0.0)],
            PipeSystem::ColdWater,
            20.0,
        );
        assert_eq!(display_name(&p), "Cold water 20 mm");
        p.diameter_mm = 22.5;
        assert_eq!(display_name(&p), "Cold water 22.5 mm");
        p.name = "  Kitchen sink supply ".into();
        assert_eq!(display_name(&p), "Kitchen sink supply");
    }

    #[test]
    fn line_weight_follows_the_scale_with_a_floor() {
        assert!((stroke_width(100.0, 50.0) - 2.0).abs() < 1e-9);
        assert_eq!(stroke_width(20.0, 100.0), paper::STROKE_MIN);
        // Risers are always wider than their line.
        assert!(riser_radius(100.0, 50.0) / 50.0 >= stroke_width(100.0, 50.0));
        assert!((riser_radius(20.0, 100.0) - paper::RISER_MIN_R * 100.0).abs() < 1e-9);
    }

    fn close(a: V3, b: V3) -> bool {
        (a - b).len() < 1e-6
    }

    #[test]
    fn straight_tube_is_closed_with_eight_sides() {
        let quads = tube_from_points(&[v3(0.0, 0.0, 0.0), v3(1000.0, 0.0, 0.0)], 50.0, 8);
        assert_eq!(quads.len(), tube_face_count(1, 8));
        assert_eq!(quads.len(), 14);
        // Every side vertex sits on the radius.
        for q in &quads[..8] {
            for p in q {
                assert!(((p.y * p.y + p.z * p.z).sqrt() - 50.0).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn a_right_angle_bend_is_one_piece_without_a_gap() {
        let pts = [
            v3(0.0, 0.0, 0.0),
            v3(1000.0, 0.0, 0.0),
            v3(1000.0, 1000.0, 0.0),
        ];
        let quads = tube_from_points(&pts, 50.0, 8);
        assert_eq!(quads.len(), tube_face_count(2, 8), "no caps at the mitre");
        // The end ring of the first segment is the start ring of the second.
        let end_of_first: Vec<V3> = quads[..8].iter().map(|q| q[3]).collect();
        let start_of_second: Vec<V3> = quads.iter().skip(8 + 3).take(8).map(|q| q[0]).collect();
        for (a, b) in end_of_first.iter().zip(&start_of_second) {
            assert!(close(*a, *b), "{a:?} vs {b:?}");
            // On the mitre plane through the joint, normal (1, 1, 0).
            assert!(
                ((a.x - 1000.0) + a.y).abs() < 1e-6,
                "{a:?} is off the mitre plane"
            );
        }
    }

    #[test]
    fn a_u_turn_is_cut_square_and_capped() {
        let pts = [v3(0.0, 0.0, 0.0), v3(1000.0, 0.0, 0.0), v3(0.0, 1.0, 0.0)];
        let quads = tube_from_points(&pts, 10.0, 8);
        assert_eq!(quads.len(), 2 * tube_face_count(1, 8));
    }

    #[test]
    fn a_short_stub_is_not_mitred() {
        // 75 mm stub after a riser on a 100 mm pipe: the mitre would reach
        // past half the stub, so the joint is cut square.
        let pts = [
            v3(0.0, 0.0, 0.0),
            v3(0.0, 0.0, 250.0),
            v3(0.0, -75.0, 250.0),
        ];
        let quads = tube_from_points(&pts, 50.0, 8);
        assert_eq!(quads.len(), 2 * tube_face_count(1, 8));
        // A 20 mm pipe on the same path is mitred.
        let quads = tube_from_points(&pts, 10.0, 8);
        assert_eq!(quads.len(), tube_face_count(2, 8));
    }
}
