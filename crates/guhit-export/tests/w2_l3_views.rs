//! Lane L3, views (DECISIONS D54): elevations and sections with hidden
//! lines removed, the roof, site and ceiling plans, the vicinity frame.
//!
//! Projects come with a freshly derived `Derived` from `$GUHIT_L3_STATES`
//! when it is set (written by guhit-app's `w2_l3_views::write_fresh_states`,
//! which has the engine), else from the golden fixtures.
//!
//! - `hidden_lines_match_a_z_buffer_oracle`: random box scenes and the sample
//!   bungalow, drawn into a 4096 px depth raster; every sampled point of a
//!   visible piece must be front-most and every point of a hidden one must
//!   not. Zero failures.
//! - `write_view_samples`: an SVG and a PNG of every view of the sample
//!   bungalow and the plumbing demo in `$GUHIT_L3_OUT` (default
//!   `target/export-samples/l3`).
//! - `four_elevations_and_two_sections_are_fast`: the Lovell house, in
//!   under 2 s in a release build (asserted only there).

mod common;

use std::path::PathBuf;

use common::*;
use guhit_export::geom::{v, Bounds, V};
use guhit_export::views::mesh::{p3, Mesh, Part, P3};
use guhit_export::views::{self, hlr};
use guhit_model::*;
use svg2pdf::usvg;

fn out_dir() -> PathBuf {
    let dir = std::env::var("GUHIT_L3_OUT").map(PathBuf::from).unwrap_or_else(|_| samples_dir().join("l3"));
    std::fs::create_dir_all(&dir).expect("out dir");
    dir
}

/// A project and its derived data: fresh from `$GUHIT_L3_STATES`, else
/// the fixture (whose `Derived` may predate the lanes' drops).
fn state(name: &str) -> Option<(Project, Derived)> {
    if let Ok(dir) = std::env::var("GUHIT_L3_STATES") {
        let path = PathBuf::from(dir).join(format!("{name}.json"));
        if let Ok(text) = std::fs::read_to_string(path) {
            let v: serde_json::Value = serde_json::from_str(&text).unwrap();
            return Some((serde_json::from_value(v["project"].clone()).unwrap(), serde_json::from_value(v["derived"].clone()).unwrap()));
        }
    }
    let file = match name {
        "bungalow" => "sample-bungalow",
        "plumbing" => "plumbing-demo",
        _ => return None,
    };
    let path = workspace_root().join(format!("fixtures/{file}.docstate.json"));
    let s: DocState = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    Some((s.project, s.derived))
}

fn pt(x: f64, y: f64) -> Point {
    Point { x, y }
}

fn footprint_box(d: &Derived) -> (f64, f64, f64, f64) {
    let mut b = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for f in &d.footprints {
        for q in &f.polygon {
            b = (b.0.min(q.x), b.1.min(q.y), b.2.max(q.x), b.3.max(q.y));
        }
    }
    b
}

/// Four elevations looking at the buildings and two sections through
/// their middle, added to the project.
fn add_marks(p: &mut Project, d: &Derived) -> Vec<(String, ViewRef)> {
    let b = footprint_box(d);
    let (m, e) = (3000.0, 5000.0);
    let (cx, cy) = ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
    let level = p.levels[0].id.clone();
    let marks = [
        ("north", MarkKind::Elevation, "North", pt(b.2 + m, b.3 + e), pt(b.0 - m, b.3 + e)),
        ("south", MarkKind::Elevation, "South", pt(b.0 - m, b.1 - e), pt(b.2 + m, b.1 - e)),
        ("east", MarkKind::Elevation, "East", pt(b.2 + e, b.1 - m), pt(b.2 + e, b.3 + m)),
        ("west", MarkKind::Elevation, "West", pt(b.0 - e, b.3 + m), pt(b.0 - e, b.1 - m)),
        ("section-a", MarkKind::Section, "A", pt(b.0 - m, cy + 300.0), pt(b.2 + m, cy + 300.0)),
        ("section-b", MarkKind::Section, "B", pt(cx - 200.0, b.1 - m), pt(cx - 200.0, b.3 + m)),
    ];
    let mut out = Vec::new();
    for (name, kind, label, start, end) in marks {
        let id = format!("mark-{name}");
        p.elements.push(Element::ViewMark(ViewMark { id: id.clone(), level_id: level.clone(), mark_kind: kind, label: label.into(), start, end, flip: false, depth_mm: None }));
        let view = match kind {
            MarkKind::Elevation => ViewRef::Elevation { mark_id: id },
            MarkKind::Section => ViewRef::Section { mark_id: id },
        };
        out.push((name.to_string(), view));
    }
    out
}

/// A lot round the buildings, its road side south.
fn add_lot(p: &mut Project, d: &Derived) {
    let b = footprint_box(d);
    let corners = vec![pt(b.0 - 3000.0, b.1 - 4500.0), pt(b.2 + 2000.0, b.1 - 4500.0), pt(b.2 + 2400.0, b.3 + 2000.0), pt(b.0 - 3000.0, b.3 + 2000.0)];
    let side = |role, setback: f64, road: &str, width: Option<f64>| LotSide { role, setback_mm: setback, road: road.into(), road_width_mm: width };
    p.elements.push(Element::Lot(Lot {
        id: "lot-1".into(),
        corners,
        sides: vec![
            side(LotSideRole::Road, 3000.0, "Mabini Street", Some(8000.0)),
            side(LotSideRole::Side, 1500.0, "", None),
            side(LotSideRole::Rear, 1500.0, "", None),
            side(LotSideRole::Side, 1500.0, "", None),
        ],
        grade_mm: 150.0,
    }));
}

/// Grid lines 1, 2, 3 across and A, B along the buildings, a dimension
/// string along the south face, and marks for the openings that have none
/// yet (the engine's schedule gives them once types are assigned).
fn add_permit_marks(p: &mut Project, d: &mut Derived) {
    let b = footprint_box(d);
    let mut grid = |id: &str, label: &str, a: Point, z: Point| p.elements.push(Element::Grid(GridLine { id: id.into(), label: label.into(), start: a, end: z }));
    let xs = [b.0, (b.0 + b.2) / 2.0, b.2];
    for (i, x) in xs.iter().enumerate() {
        grid(&format!("grid-{}", i + 1), &(i + 1).to_string(), pt(*x, b.1 - 2500.0), pt(*x, b.3 + 2500.0));
    }
    for (i, y) in [b.1, b.3].iter().enumerate() {
        grid(&format!("grid-{}", ["a", "b"][i]), ["A", "B"][i], pt(b.0 - 2500.0, *y), pt(b.2 + 2500.0, *y));
    }
    let level = p.levels[0].id.clone();
    p.elements.push(Element::Dimension(Dimension {
        id: "dim-string".into(),
        level_id: level,
        a: pt(b.0, b.1),
        b: pt(b.2, b.1),
        offset_mm: -1800.0,
        text_override: None,
        through: vec![pt(xs[1], b.1)],
        auto: None,
    }));
    if d.opening_schedule.marks.is_empty() {
        let (mut doors, mut windows) = (0, 0);
        for e in &p.elements {
            if let Element::Opening(o) = e {
                let mark = match o.opening_type {
                    OpeningType::Door => {
                        doors += 1;
                        format!("D{doors}")
                    }
                    OpeningType::Window => {
                        windows += 1;
                        format!("W{windows}")
                    }
                };
                d.opening_schedule.marks.push(OpeningMark { opening_id: o.id.clone(), mark });
            }
        }
    }
}

fn all_views(p: &mut Project, d: &mut Derived) -> Vec<(String, ViewRef)> {
    add_permit_marks(p, d);
    let mut views = add_marks(p, d);
    add_lot(p, d);
    let level = p.levels[0].id.clone();
    views.push(("plan".into(), ViewRef::Plan { level_id: level.clone(), sheet: SheetKind::Plan }));
    views.push(("rcp".into(), ViewRef::Plan { level_id: level, sheet: SheetKind::ReflectedCeiling }));
    views.push(("roof-plan".into(), ViewRef::RoofPlan));
    views.push(("site-plan".into(), ViewRef::SitePlan));
    views.push(("vicinity".into(), ViewRef::VicinityMap { file_name: None, radius_m: 500.0 }));
    views
}

fn parse_svg(svg: &str) -> usvg::Tree {
    let mut db = usvg::fontdb::Database::new();
    db.load_system_fonts();
    let options = usvg::Options { fontdb: std::sync::Arc::new(db), ..usvg::Options::default() };
    usvg::Tree::from_str(svg, &options).expect("usvg parses the view")
}

fn write_png(svg: &str, path: &std::path::Path, px_per_mm: f32) {
    let tree = parse_svg(svg);
    let size = tree.size();
    let scale = px_per_mm * 25.4 / 96.0;
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1)).expect("pixmap");
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.save_png(path).expect("write png");
}

#[test]
fn write_view_samples() {
    let dir = out_dir();
    for name in ["bungalow", "plumbing", "lovell"] {
        let Some((mut p, mut d)) = state(name) else { continue };
        for (vname, view) in all_views(&mut p, &mut d) {
            let out = guhit_export::view_svg(&p, &d, &view, None, &guhit_export::SheetInputs::default()).unwrap_or_else(|e| panic!("{name} {vname}: {e}"));
            std::fs::write(dir.join(format!("{name}-{vname}.svg")), &out.data).unwrap();
            write_png(&out.data, &dir.join(format!("{name}-{vname}.png")), 8.0);
        }
    }
}

#[test]
fn every_drawn_view_has_items_and_bounds() {
    let (mut p, mut d) = state("bungalow").unwrap();
    for (name, view) in all_views(&mut p, &mut d) {
        let v = views::items(&p, &d, &view, Some(100)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!v.items.is_empty(), "{name} has items");
        assert!(!v.bounds.is_empty() && v.bounds.width() > 100.0, "{name} has bounds");
        assert!(!v.title.is_empty());
    }
    // Tables and renders are laid out by the sheet.
    assert!(views::items(&p, &d, &ViewRef::Schedule { schedule: ScheduleKind::Rooms }, None).is_err());
    assert!(views::items(&p, &d, &ViewRef::Perspective { render_id: "r".into() }, None).is_err());
    // An elevation from a section mark is refused, and a missing mark too.
    assert!(views::items(&p, &d, &ViewRef::Elevation { mark_id: "mark-section-a".into() }, None).is_err());
    assert!(views::items(&p, &d, &ViewRef::Section { mark_id: "nope".into() }, None).is_err());
}

// ------------------------------------------------------------- the oracle

/// A depth raster: per pixel the nearest depth at its center and the depth
/// plane of the triangle that gave it (t = a s + b y + c), so a point can be
/// checked at its exact place, not at the pixel center.
struct ZBuf {
    w: usize,
    h: usize,
    origin: V,
    px: f64,
    depth: Vec<f64>,
    plane: Vec<[f64; 3]>,
}

impl ZBuf {
    fn new(b: Bounds, size: usize) -> ZBuf {
        let span = b.width().max(b.height()).max(1.0);
        let px = span / size as f64;
        let w = (b.width() / px).ceil() as usize + 3;
        let h = (b.height() / px).ceil() as usize + 3;
        ZBuf { w, h, origin: b.min - v(px, px), px, depth: vec![f64::INFINITY; w * h], plane: vec![[0.0, 0.0, f64::INFINITY]; w * h] }
    }

    fn tri(&mut self, a: P3, b: P3, c: P3) {
        let (q0, q1, q2) = (v(a.x, a.y), v(b.x, b.y), v(c.x, c.y));
        let area = (q1 - q0).x * (q2 - q0).y - (q1 - q0).y * (q2 - q0).x;
        if area.abs() < 1e-9 {
            return;
        }
        // Depth plane t = ga s + gb y + gc.
        let (d1, d2) = (q1 - q0, q2 - q0);
        let (z1, z2) = (b.z - a.z, c.z - a.z);
        let ga = (z1 * d2.y - z2 * d1.y) / area;
        let gb = (d1.x * z2 - d2.x * z1) / area;
        let gc = a.z - ga * q0.x - gb * q0.y;
        let lo = v(q0.x.min(q1.x).min(q2.x), q0.y.min(q1.y).min(q2.y));
        let hi = v(q0.x.max(q1.x).max(q2.x), q0.y.max(q1.y).max(q2.y));
        let i0 = (((lo.x - self.origin.x) / self.px).floor().max(0.0)) as usize;
        let i1 = (((hi.x - self.origin.x) / self.px).ceil() as usize).min(self.w - 1);
        let j0 = (((lo.y - self.origin.y) / self.px).floor().max(0.0)) as usize;
        let j1 = (((hi.y - self.origin.y) / self.px).ceil() as usize).min(self.h - 1);
        let edge = |p: V, q: V, r: V| (q - p).x * (r - p).y - (q - p).y * (r - p).x;
        for j in j0..=j1 {
            for i in i0..=i1 {
                let c = self.origin + v((i as f64 + 0.5) * self.px, (j as f64 + 0.5) * self.px);
                let (w0, w1, w2) = (edge(q1, q2, c), edge(q2, q0, c), edge(q0, q1, c));
                let inside = if area > 0.0 { w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 } else { w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0 };
                if !inside {
                    continue;
                }
                let t = a.z + ga * (c.x - q0.x) + gb * (c.y - q0.y);
                let k = j * self.w + i;
                if t < self.depth[k] {
                    self.depth[k] = t;
                    self.plane[k] = [ga, gb, gc];
                }
            }
        }
    }

    /// The front surfaces of the pixel under a point and its eight
    /// neighbours, each evaluated at the point: the nearest and the
    /// farthest. Open sky counts as infinitely far.
    fn around(&self, p: V) -> (f64, f64) {
        let i = ((p.x - self.origin.x) / self.px).floor() as i64;
        let j = ((p.y - self.origin.y) / self.px).floor() as i64;
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for dj in -1..=1 {
            for di in -1..=1 {
                let (x, y) = (i + di, j + dj);
                if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
                    hi = f64::INFINITY;
                    continue;
                }
                let k = y as usize * self.w + x as usize;
                let [a, b, c] = self.plane[k];
                let t = if c.is_finite() { a * p.x + b * p.y + c } else { f64::INFINITY };
                lo = lo.min(t);
                hi = hi.max(t);
            }
        }
        (lo, hi)
    }
}

/// Rasterize every face of the mesh in the view (no clipping: the oracle
/// views look at the scene from outside).
fn raster(mesh: &Mesh, frame: &hlr::ViewFrame, size: usize) -> ZBuf {
    let faces: Vec<Vec<P3>> = mesh.faces.iter().map(|f| f.pts.iter().map(|q| frame.to_view(*q)).collect()).collect();
    let mut b = Bounds::empty();
    for f in &faces {
        for q in f {
            b.add(v(q.x, q.y));
        }
    }
    let mut z = ZBuf::new(b, size);
    for f in &faces {
        let flat: Vec<V> = f.iter().map(|q| v(q.x, q.y)).collect();
        for t in guhit_export::geom::triangulate(&flat) {
            z.tri(f[t[0]], f[t[1]], f[t[2]]);
        }
    }
    z
}

/// Check every piece against the raster. Returns (samples, failures).
fn check(h: &hlr::Hlr, z: &ZBuf, label: &str) -> (usize, usize) {
    let (mut n, mut bad) = (0, 0);
    for (pieces, visible) in [(&h.visible, true), (&h.hidden, false)] {
        for p in pieces.iter() {
            if p.kind == hlr::EdgeKind::Figure {
                continue;
            }
            let len = (p.b - p.a).len();
            // Away from the ends, where visibility changes.
            let skip = 3.0 * z.px / len.max(1e-9);
            if skip >= 0.5 {
                continue;
            }
            let count = ((len / z.px) as usize).clamp(1, 64);
            for k in 0..=count {
                let u = skip + (1.0 - 2.0 * skip) * k as f64 / count as f64;
                let (q, d) = (p.point(u), p.depth_at(u));
                let (lo, hi) = z.around(q);
                // Hidden means a face more than 0.5 mm nearer
                // (`hlr::DEPTH_EPS`); the checks sit a quarter mm either side.
                let ok = if visible { d <= hi + hlr::DEPTH_EPS + 0.25 } else { d > lo + hlr::DEPTH_EPS - 0.25 };
                n += 1;
                if !ok {
                    bad += 1;
                    if bad <= 5 {
                        eprintln!("{label}: {} piece {:?} at {q:?} depth {d} vs raster {lo}..{hi}", if visible { "visible" } else { "hidden" }, p.kind);
                    }
                }
            }
        }
    }
    (n, bad)
}

/// A frame looking along azimuth `az` (degrees from +x) and down `el`
/// degrees.
fn frame_at(target: P3, az: f64, el: f64, dist: f64) -> hlr::ViewFrame {
    let (a, e) = (az.to_radians(), el.to_radians());
    let dir = p3(a.cos() * e.cos(), a.sin() * e.cos(), -e.sin());
    let right = dir.cross(p3(0.0, 0.0, 1.0)).unit().unwrap();
    let up = right.cross(dir);
    hlr::ViewFrame { origin: target - dir * dist, right, up, dir }
}

/// A small deterministic random number source.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }

    /// A multiple of `step` in lo..hi.
    fn grid(&mut self, lo: f64, hi: f64, step: f64) -> f64 {
        lo + ((self.next() * (hi - lo) / step).floor()) * step
    }
}

fn box_scene(seed: u64, count: usize) -> Mesh {
    let mut r = Lcg(seed);
    let mut m = Mesh::default();
    for i in 0..count {
        let (cx, cy) = (r.grid(0.0, 20000.0, 50.0), r.grid(0.0, 20000.0, 50.0));
        let (w, d) = (r.grid(300.0, 4000.0, 50.0), r.grid(300.0, 4000.0, 50.0));
        let z0 = if r.next() < 0.6 { 0.0 } else { r.grid(0.0, 3000.0, 50.0) };
        let h = r.grid(300.0, 4000.0, 50.0);
        let rot = [0.0, 0.0, 0.0, 30.0, 45.0][(r.next() * 5.0) as usize % 5];
        let poly = guhit_export::model3d::rect(v(cx, cy), w, d, rot);
        m.push_box(Part::Wall, &format!("box{i}"), &poly, z0, z0 + h);
    }
    m
}

#[test]
fn hidden_lines_match_a_z_buffer_oracle() {
    let opts = hlr::Options { near: None, far: None, remove_hidden: true };
    let (mut samples, mut failures) = (0, 0);
    // Random boxes: on a 50 mm grid, some turned, some lifted.
    for seed in 1..=6u64 {
        let mesh = box_scene(seed, 40);
        let target = p3(10000.0, 10000.0, 1500.0);
        for (az, el) in [(90.0, 0.0), (0.0, 0.0), (180.0, 0.0), (270.0, 0.0), (30.0, 0.0), (120.0, 25.0), (225.0, 40.0)] {
            let frame = frame_at(target, az, el, 40000.0);
            let h = hlr::hidden_lines(&mesh, &frame, &opts, &[]);
            let z = raster(&mesh, &frame, 4096);
            let (n, bad) = check(&h, &z, &format!("boxes {seed} at {az}/{el}"));
            assert!(!h.hidden.is_empty() && !h.visible.is_empty());
            samples += n;
            failures += bad;
        }
    }
    // The sample bungalow from the four sides and two angles.
    let (p, d) = state("bungalow").unwrap();
    let mesh = views::mesh::build(&p, &d);
    let b = footprint_box(&d);
    let target = p3((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0, 1500.0);
    for (az, el) in [(90.0, 0.0), (270.0, 0.0), (0.0, 0.0), (180.0, 0.0), (60.0, 0.0), (135.0, 30.0)] {
        let frame = frame_at(target, az, el, 30000.0);
        let h = hlr::hidden_lines(&mesh, &frame, &opts, &[]);
        let z = raster(&mesh, &frame, 4096);
        let (n, bad) = check(&h, &z, &format!("bungalow at {az}/{el}"));
        samples += n;
        failures += bad;
    }
    eprintln!("oracle: {samples} samples, {failures} failures");
    assert!(samples > 10000, "enough samples: {samples}");
    assert_eq!(failures, 0, "{failures} of {samples} samples disagree with the depth raster");
}

/// The poche area of a section through `walls` (plan pairs, 150 thick, the
/// level height) along x = `x`, looking +x, the plan turned by `rot`
/// degrees about the origin. Model mm2.
fn cut_area(walls: &[((f64, f64), (f64, f64))], x: f64, rot: f64) -> f64 {
    use guhit_export::plan::{Fill, Prim};
    // Only the fixture's project: the walls are derived here.
    let path = workspace_root().join("fixtures/sample-bungalow.docstate.json");
    let state: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut p: Project = serde_json::from_value(state["project"].clone()).unwrap();
    p.elements.clear();
    let level = p.levels[0].id.clone();
    let turn = |x: f64, y: f64| {
        let q = v(x, y).rotated(rot);
        pt(q.x, q.y)
    };
    for (i, (a, b)) in walls.iter().enumerate() {
        p.elements.push(Element::Wall(Wall {
            id: format!("w{i}"),
            level_id: level.clone(),
            start: turn(a.0, a.1),
            end: turn(b.0, b.1),
            thickness_mm: 150.0,
            height_mm: None,
            material_id: None,
            role: WallRole::Building,
        }));
    }
    p.elements.push(Element::ViewMark(ViewMark {
        id: "cut".into(),
        level_id: level,
        mark_kind: MarkKind::Section,
        label: "A".into(),
        start: turn(x, 7000.0),
        end: turn(x, -2000.0),
        flip: false,
        depth_mm: None,
    }));
    let d = guhit_core::compute_derived(&p);
    let v = views::items(&p, &d, &ViewRef::Section { mark_id: "cut".into() }, Some(50)).expect("the section draws");
    v.items
        .iter()
        .filter_map(|i| match &i.prim {
            Prim::Poly { pts, closed: true, fill: Fill::Ink } => Some(guhit_export::geom::polygon_area(pts)),
            _ => None,
        })
        .sum()
}

/// GR2 #2: a section exactly on a wall joint, along a wall's center line,
/// or along a T wall from its joint cuts the walls it would cut a hair off
/// the line: faces on the cut plane follow one tolerance rule, and the end
/// faces of two walls meeting on the plane are a cut, not a pair to drop.
#[test]
fn a_cut_on_a_joint_or_a_face_keeps_its_poche() {
    let one = [((0.0, 0.0), (5000.0, 0.0))];
    let joint = [((0.0, 0.0), (2500.0, 0.0)), ((2500.0, 0.0), (5000.0, 0.0))];
    let along = [((2500.0, 0.0), (2500.0, 5000.0))];
    let tee = [((0.0, 0.0), (2500.0, 0.0)), ((2500.0, 0.0), (5000.0, 0.0)), ((2500.0, 0.0), (2500.0, 5000.0))];
    // A room with an inner wall from the joint of the two south walls.
    let room = [
        ((0.0, 0.0), (2500.0, 0.0)),
        ((2500.0, 0.0), (5000.0, 0.0)),
        ((5000.0, 0.0), (5000.0, 4000.0)),
        ((5000.0, 4000.0), (0.0, 4000.0)),
        ((0.0, 4000.0), (0.0, 0.0)),
        ((2500.0, 0.0), (2500.0, 4000.0)),
    ];
    let cases: [(&str, &[((f64, f64), (f64, f64))]); 5] = [("one wall", &one), ("joint", &joint), ("along", &along), ("tee", &tee), ("room", &room)];
    for rot in [0.0, 30.0] {
        for (name, walls) in cases {
            let on = cut_area(walls, 2500.0, rot);
            for off in [-1.0, 1.0, 0.01] {
                let near = cut_area(walls, 2500.0 + off, rot);
                assert!(near > 1e5, "{name} at {rot} deg, {off} off: some poche");
                assert!((on - near).abs() <= near * 0.01 + 1e4, "{name} at {rot} deg: {on:.0} mm2 on the line, {near:.0} {off} mm off");
            }
        }
    }
    // A plane on a wall's outer face, the wall beyond: not cut, as a hair
    // in front of the face.
    let face = cut_area(&[((0.0, 0.0), (0.0, 5000.0)), ((0.0, 5000.0), (5000.0, 5000.0))], -75.0, 0.0);
    let before = cut_area(&[((0.0, 0.0), (0.0, 5000.0)), ((0.0, 5000.0), (5000.0, 5000.0))], -76.0, 0.0);
    assert!((face - before).abs() < 1e4, "{face} on the face, {before} before it");
}

#[test]
fn merge_collinear_joins_overlaps_and_keeps_the_heavier() {
    let pieces = vec![(v(0.0, 0.0), v(10.0, 0.0), 1u8), (v(10.005, 0.0), v(20.0, 0.0), 1), (v(5.0, 0.0), v(8.0, 0.0), 3), (v(0.0, 5.0), v(3.0, 5.0), 1)];
    let out = hlr::merge_collinear(&pieces);
    // 0..5 light, 5..8 heavy, 8..20 light, and the other line.
    assert_eq!(out.len(), 4, "{out:?}");
    assert!(out.iter().any(|(a, b, w)| *w == 3 && (a.x - 5.0).abs() < 1e-9 && (b.x - 8.0).abs() < 1e-9));
    assert!(out.iter().any(|(a, b, w)| *w == 1 && (a.x - 8.0).abs() < 1e-9 && (b.x - 20.0).abs() < 1e-9));
}

#[test]
fn four_elevations_and_two_sections_are_fast() {
    let Some((mut p, d)) = state("lovell").or_else(|| state("bungalow")) else { return };
    let marks = add_marks(&mut p, &d);
    let t = std::time::Instant::now();
    for (name, view) in &marks {
        let v = views::items(&p, &d, view, Some(100)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!v.items.is_empty());
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let mesh = views::mesh::build(&p, &d);
    eprintln!("six views of {} in {ms:.0} ms ({} triangles)", p.name, mesh.triangle_count());
    if !cfg!(debug_assertions) {
        assert!(ms < 2000.0, "{ms} ms");
    }
}


// ------------------------------------------------------------ plan marks

fn plan_items(p: &Project, d: &Derived) -> Vec<guhit_export::plan::Item> {
    guhit_export::plan::build_items(
        p,
        d,
        &p.levels[0],
        &guhit_export::plan::PlanOptions { scale: 100.0, show_dimensions: true, show_room_labels: true, show_assets: true, unicode: true, skip_devices: false },
    )
}

fn texts(items: &[guhit_export::plan::Item]) -> Vec<String> {
    items
        .iter()
        .filter_map(|i| match &i.prim {
            guhit_export::plan::Prim::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// G4 problem 11: a stair whose first riser stands at a wall prints "UP"
/// clear of the wall, inside its flight.
#[test]
fn up_moves_clear_of_the_wall_it_starts_at() {
    use guhit_export::plan::{Cat, Fill, Prim};
    let (mut p, d) = state("bungalow").unwrap();
    let level = p.levels[0].id.clone();
    // The north wall's inner face is at y 5925, solid at x 6500 (its window
    // is at x 2500); the flight runs south.
    p.elements.push(Element::Stair(Stair {
        id: "stair".into(),
        level_id: level,
        origin: pt(6500.0, 5925.0),
        rotation_deg: 180.0,
        width_mm: 1000.0,
        run_mm: 2500.0,
        riser_count: 11,
        railing: None,
        shape: StairShape::Straight,
        turn: Turn::Left,
        first_flight_risers: None,
        landing_mm: None,
        well_mm: 0.0,
    }));
    // Plans draw stairs from `Derived::stairs` (D54).
    let mut d = d;
    d.stairs = guhit_core::compute_derived(&p).stairs;
    let items = plan_items(&p, &d);
    let walls: Vec<Vec<V>> = items
        .iter()
        .filter(|i| i.cat == Cat::Wall)
        .filter_map(|i| match &i.prim {
            Prim::Poly { pts, fill: Fill::Ink, .. } => Some(pts.clone()),
            _ => None,
        })
        .collect();
    let up = items
        .iter()
        .find(|i| i.cat == Cat::Strs && matches!(&i.prim, Prim::Text { text, .. } if text == "UP"))
        .expect("an UP label");
    let b = guhit_export::plan::items_bounds(std::slice::from_ref(up));
    let quad = [b.min, v(b.max.x, b.min.y), b.max, v(b.min.x, b.max.y)];
    for w in &walls {
        assert!(guhit_export::geom::overlap_area(&quad, w) < 1.0, "UP at {b:?} overlaps a wall");
    }
    assert!(b.max.y < 5925.0 && b.min.y > 5925.0 - 2500.0, "UP inside the flight: {b:?}");
}

/// G4 problem 11: a fence prints light over a grey band, the house heavy.
#[test]
fn site_walls_print_lighter_than_house_walls() {
    use guhit_export::plan::{Cat, Fill, Pen, Prim};
    let (mut p, d) = state("bungalow").unwrap();
    let level = p.levels[0].id.clone();
    p.elements.push(Element::Wall(Wall {
        id: "fence".into(),
        level_id: level,
        start: pt(-3000.0, -3000.0),
        end: pt(11000.0, -3000.0),
        thickness_mm: 150.0,
        height_mm: Some(1800.0),
        material_id: None,
        role: WallRole::Site,
    }));
    let items = plan_items(&p, &d);
    let fence: Vec<_> = items.iter().filter(|i| i.cat == Cat::Site).collect();
    assert!(!fence.is_empty());
    for f in fence {
        assert_eq!(f.pen, Pen::Light);
        assert!(matches!(f.prim, Prim::Poly { fill: Fill::Tint(_), .. }));
    }
    assert!(items.iter().any(|i| i.cat == Cat::Wall && i.pen == Pen::Heavy && matches!(i.prim, Prim::Poly { fill: Fill::Ink, .. })));
}

#[test]
fn the_plan_draws_tags_grids_strings_and_marks() {
    let (mut p, mut d) = state("bungalow").unwrap();
    add_permit_marks(&mut p, &mut d);
    add_marks(&mut p, &d);
    let t = texts(&plan_items(&p, &d));
    for want in ["D1", "D2", "W1", "W2", "W3", "1", "2", "3", "A", "B", "N", "NORTH"] {
        assert!(t.iter().any(|x| x == want), "{want} in {t:?}");
    }
    // The string along the south face: 8150 split at grid 2.
    assert_eq!(t.iter().filter(|x| *x == "4075").count(), 2, "{t:?}");
}
