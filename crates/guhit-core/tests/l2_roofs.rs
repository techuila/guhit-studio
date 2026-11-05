//! Lane L2: `Derived::roofs` (DECISIONS D54). Hip roofs from the straight
//! skeleton of square-cornered outlines (rectangles, L, T and U, equal arm
//! widths included), gables, sheds and flats, by formula; the invariant that
//! every top point stands the pitch times its distance from the eaves above
//! them; gable sides; derived outlines that do not fit; roof posts and the
//! roof review items.

mod common;

use common::*;
use guhit_core::Document;
use guhit_model::*;

const PITCH: f64 = 25.0;
const THICK: f64 = 150.0;
const BASE: f64 = 3000.0;

fn tan() -> f64 {
    PITCH.to_radians().tan()
}

fn vertical() -> f64 {
    THICK / PITCH.to_radians().cos()
}

fn roof(points: &[(f64, f64)], shape: RoofShape, overhang: f64) -> RoofArea {
    RoofArea {
        id: String::new(),
        level_id: Some(LEVEL.into()),
        outline: RoofOutline::Polygon { points: points.iter().map(|q| p(q.0, q.1)).collect() },
        shape,
        pitch_deg: PITCH,
        ridge_axis: Axis::X,
        flip: false,
        gable_sides: vec![],
        overhang_mm: overhang,
        thickness_mm: THICK,
        eave_mm: None,
        material_id: None,
        fascia: Some(Trim { size_mm: 200.0, material_id: None }),
        gutter: None,
    }
}

fn put(doc: &mut Document, r: RoofArea) -> Id {
    apply_checked(doc, Command::AddElement { element: Element::Roof(r) }).diff.added[0].clone()
}

fn geometry<'a>(doc: &'a Document, id: &str) -> &'a RoofGeometry {
    doc.derived().roofs.iter().find(|g| g.roof_id == id).expect("roof geometry")
}

fn one(points: &[(f64, f64)], shape: RoofShape, overhang: f64) -> RoofGeometry {
    let mut doc = blank();
    let id = put(&mut doc, roof(points, shape, overhang));
    geometry(&doc, &id).clone()
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<(f64, f64)> {
    vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn lines(g: &RoofGeometry, kind: RoofLineKind) -> Vec<&RoofLine> {
    g.lines.iter().filter(|l| l.kind == kind).collect()
}

fn len3(l: &RoofLine) -> f64 {
    let (dx, dy, dz) = (l.b.x - l.a.x, l.b.y - l.a.y, l.b.z - l.a.z);
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn plan_len(l: &RoofLine) -> f64 {
    (l.b.x - l.a.x).hypot(l.b.y - l.a.y)
}

fn plan_area(plane: &[Vec3]) -> f64 {
    let mut s = 0.0;
    for i in 0..plane.len() {
        let (a, b) = (plane[i], plane[(i + 1) % plane.len()]);
        s += a.x * b.y - b.x * a.y;
    }
    s / 2.0
}

/// Distance from `q` to an axis-aligned outline, measured square to it: the
/// smaller over its sides of the larger axis distance to the side.
fn square_dist(q: (f64, f64), outline: &[(f64, f64)]) -> f64 {
    let n = outline.len();
    (0..n)
        .map(|i| {
            let (a, b) = (outline[i], outline[(i + 1) % n]);
            let dx = (a.0.min(b.0) - q.0).max(q.0 - a.0.max(b.0)).max(0.0);
            let dy = (a.1.min(b.1) - q.1).max(q.1 - a.1.max(b.1)).max(0.0);
            dx.max(dy)
        })
        .fold(f64::INFINITY, f64::min)
}

/// The skeleton invariant: every top point of every plane stands the pitch
/// times its distance from the eaves above the eave height; the planes are
/// planar, face up, and tile the eave outline exactly.
fn assert_skeleton(g: &RoofGeometry, eaves: &[(f64, f64)], overhang: f64) {
    let z0 = BASE - overhang * tan() + vertical();
    let mut total = 0.0;
    for plane in &g.planes {
        let a = plan_area(plane);
        assert!(a > 0.0, "counter-clockwise from above: {plane:?}");
        total += a;
        for v in plane {
            let h = square_dist((v.x, v.y), eaves);
            assert!(near(v.z, z0 + tan() * h, 1e-6), "{v:?}: expected {} ({h} from the eaves)", z0 + tan() * h);
        }
    }
    let n = eaves.len();
    let outline: f64 = (0..n).map(|i| eaves[i].0 * eaves[(i + 1) % n].1 - eaves[(i + 1) % n].0 * eaves[i].1).sum::<f64>() / 2.0;
    assert!(near(total, outline, 1.0), "planes tile the outline: {total} vs {outline}");
    assert!(near(g.plan_area_m2, outline / 1.0e6, 1e-5));
    assert!(near(g.surface_area_m2, outline / 1.0e6 / PITCH.to_radians().cos(), 1e-5));
}

fn grow(points: &[(f64, f64)], d: f64) -> Vec<(f64, f64)> {
    let pts: Vec<Point> = points.iter().map(|q| p(q.0, q.1)).collect();
    guhit_core::roofs::offset_polygon(&pts, d).iter().map(|q| (q.x, q.y)).collect()
}

#[test]
fn a_rectangle_hip_has_a_ridge_of_l_minus_w() {
    let outline = rect(0.0, 0.0, 12000.0, 8000.0);
    let g = one(&outline, RoofShape::Hip, 600.0);
    assert_eq!(g.planes.len(), 4);
    let ridges = lines(&g, RoofLineKind::Ridge);
    assert_eq!(ridges.len(), 1, "{:?}", g.lines);
    assert!(near(plan_len(ridges[0]), 12000.0 - 8000.0, 1e-6), "L - W");
    // At the middle, W / 2 (eaves included) from the eaves.
    let z0 = BASE - 600.0 * tan() + vertical();
    assert!(near(ridges[0].a.z, z0 + tan() * 9200.0 / 2.0, 1e-6));
    let hips = lines(&g, RoofLineKind::Hip);
    assert_eq!(hips.len(), 4);
    for h in &hips {
        assert!(near(plan_len(h), 4600.0 * 2f64.sqrt(), 1e-6), "45 degree hips");
    }
    assert!(lines(&g, RoofLineKind::Valley).is_empty());
    assert_eq!(lines(&g, RoofLineKind::Eave).len(), 4);
    // Areas by formula: plan (L + 2o)(W + 2o), surface that over cos.
    assert!(near(g.plan_area_m2, 13.2 * 9.2, 1e-6));
    assert!(near(g.surface_area_m2, 13.2 * 9.2 / PITCH.to_radians().cos(), 1e-6));
    assert_skeleton(&g, &grow(&outline, 600.0), 600.0);
    // One eave run all the way round, closed.
    assert_eq!(g.eaves.len(), 1);
    let run = &g.eaves[0];
    assert_eq!(run.len(), 5);
    assert!(near(run[0].x, run[4].x, 1e-9) && near(run[0].y, run[4].y, 1e-9));
    assert!(run.iter().all(|v| near(v.z, z0, 1e-6)));
    assert!(g.infill.is_empty(), "a hip has no gable ends");
    assert!(near(g.vertical_mm, vertical(), 1e-9));
}

#[test]
fn a_square_hip_is_a_pyramid() {
    let g = one(&rect(0.0, 0.0, 6000.0, 6000.0), RoofShape::Hip, 0.0);
    assert_eq!(g.planes.len(), 4);
    assert!(lines(&g, RoofLineKind::Ridge).is_empty());
    assert_eq!(lines(&g, RoofLineKind::Hip).len(), 4);
    assert_skeleton(&g, &rect(0.0, 0.0, 6000.0, 6000.0), 0.0);
}

#[test]
fn l_t_and_u_hips_keep_the_invariant_with_equal_and_unequal_arms() {
    // (name, outline, inner corners, planes): one plane per side, except
    // sides in line facing the same way whose planes join.
    let shapes: Vec<(&str, Vec<(f64, f64)>, usize, usize)> = vec![
        // Unequal arms: 4 m and 5 m.
        ("L", vec![(0.0, 0.0), (10000.0, 0.0), (10000.0, 4000.0), (5000.0, 4000.0), (5000.0, 8000.0), (0.0, 8000.0)], 1, 6),
        // Equal arms, 4 m each: the two hips and the valley meet at once.
        ("L equal", vec![(0.0, 0.0), (10000.0, 0.0), (10000.0, 4000.0), (4000.0, 4000.0), (4000.0, 10000.0), (0.0, 10000.0)], 1, 6),
        // A T: a 12 x 4 m bar and a 4 m wide stem, all one width: the two
        // sides under the bar meet only at a point.
        ("T equal", vec![(0.0, 6000.0), (4000.0, 6000.0), (4000.0, 0.0), (8000.0, 0.0), (8000.0, 6000.0), (12000.0, 6000.0), (12000.0, 10000.0), (0.0, 10000.0)], 2, 8),
        // A narrower stem: the two sides under the bar are one plane, joined
        // over the end of the stem.
        ("T", vec![(0.0, 6000.0), (4500.0, 6000.0), (4500.0, 0.0), (7500.0, 0.0), (7500.0, 6000.0), (12000.0, 6000.0), (12000.0, 10000.0), (0.0, 10000.0)], 2, 7),
        // A U: arms and base all 4 m wide.
        ("U equal", vec![(0.0, 0.0), (12000.0, 0.0), (12000.0, 10000.0), (8000.0, 10000.0), (8000.0, 4000.0), (4000.0, 4000.0), (4000.0, 10000.0), (0.0, 10000.0)], 2, 8),
        ("U", vec![(0.0, 0.0), (13000.0, 0.0), (13000.0, 9000.0), (9000.0, 9000.0), (9000.0, 5000.0), (3500.0, 5000.0), (3500.0, 9000.0), (0.0, 9000.0)], 2, 8),
    ];
    for (name, outline, valleys, planes) in shapes {
        for overhang in [0.0, 600.0] {
            let g = one(&outline, RoofShape::Hip, overhang);
            let eaves = grow(&outline, overhang);
            assert_eq!(g.planes.len(), planes, "{name} {overhang}: planes");
            assert_skeleton(&g, &eaves, overhang);
            let v = lines(&g, RoofLineKind::Valley);
            // A valley from each inner corner (it may come in pieces).
            let from_corner = |c: (f64, f64)| {
                v.iter().any(|l| {
                    (near(l.a.x, c.0, 1e-3) && near(l.a.y, c.1, 1e-3)) || (near(l.b.x, c.0, 1e-3) && near(l.b.y, c.1, 1e-3))
                })
            };
            let reflex: Vec<(f64, f64)> = (0..eaves.len())
                .filter(|&i| {
                    let n = eaves.len();
                    let (a, b, c) = (eaves[(i + n - 1) % n], eaves[i], eaves[(i + 1) % n]);
                    (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0) < 0.0
                })
                .map(|i| eaves[i])
                .collect();
            assert_eq!(reflex.len(), valleys, "{name}");
            for c in reflex {
                assert!(from_corner(c), "{name} {overhang}: a valley from {c:?}: {:?}", g.lines);
            }
            assert!(!lines(&g, RoofLineKind::Hip).is_empty(), "{name}");
            assert!(lines(&g, RoofLineKind::Rake).is_empty(), "{name}: no gables");
        }
    }
}

#[test]
fn a_turned_rectangle_hips_in_its_own_frame() {
    let (s, c) = 30f64.to_radians().sin_cos();
    let turn = |x: f64, y: f64| (1000.0 + x * c - y * s, 2000.0 + x * s + y * c);
    let outline = vec![turn(0.0, 0.0), turn(10000.0, 0.0), turn(10000.0, 6000.0), turn(0.0, 6000.0)];
    let g = one(&outline, RoofShape::Hip, 0.0);
    assert_eq!(g.planes.len(), 4);
    let ridge = lines(&g, RoofLineKind::Ridge);
    assert_eq!(ridge.len(), 1);
    assert!(near(plan_len(ridge[0]), 4000.0, 1e-6));
    assert!(near(ridge[0].a.z, BASE + vertical() + tan() * 3000.0, 1e-6));
}

#[test]
fn a_gable_and_a_shed_on_a_rectangle_and_flat_on_anything() {
    let outline = rect(0.0, 0.0, 10000.0, 6000.0);
    let g = one(&outline, RoofShape::Gable, 500.0);
    assert_eq!(g.planes.len(), 2);
    let ridge = lines(&g, RoofLineKind::Ridge);
    assert_eq!(ridge.len(), 1);
    assert!(near(plan_len(ridge[0]), 11000.0, 1e-6), "the full length, eaves included");
    assert!(near(ridge[0].a.y, 3000.0, 1e-6), "ridge along x through the middle");
    assert_eq!(lines(&g, RoofLineKind::Rake).len(), 4);
    assert_eq!(g.eaves.len(), 2, "eaves on the two long sides");
    assert!(g.infill.is_empty(), "a drawn roof stands on posts: no gable walls");
    assert!(near(g.surface_area_m2, 11.0 * 7.0 / PITCH.to_radians().cos(), 1e-6));
    let shed = one(&outline, RoofShape::Shed, 0.0);
    assert_eq!(shed.planes.len(), 1);
    assert_eq!(lines(&shed, RoofLineKind::Eave).len(), 1);
    assert_eq!(lines(&shed, RoofLineKind::Rake).len(), 2);
    let high = lines(&shed, RoofLineKind::Ridge);
    assert!(near(high[0].a.z, BASE + vertical() + tan() * 10000.0, 1e-6), "rising along +x");
    let flat = one(&[(0.0, 0.0), (5000.0, 0.0), (6000.0, 3000.0), (0.0, 4000.0)], RoofShape::Flat, 300.0);
    assert_eq!(flat.planes.len(), 1);
    assert!(flat.planes[0].iter().all(|v| near(v.z, BASE + THICK, 1e-9)));
    assert_eq!(lines(&flat, RoofLineKind::Eave).len(), 4);
}

#[test]
fn gable_sides_turn_hip_ends_into_gables() {
    let outline = rect(0.0, 0.0, 12000.0, 8000.0);
    let mut doc = blank();
    let mut r = roof(&outline, RoofShape::Hip, 0.0);
    // Side 1 runs from (12000, 0) to (12000, 8000): the east end.
    r.gable_sides = vec![1];
    let id = put(&mut doc, r);
    let g = geometry(&doc, &id).clone();
    assert_eq!(g.planes.len(), 3, "south, north and the west hip");
    let ridge = lines(&g, RoofLineKind::Ridge);
    assert!(near(plan_len(ridge[0]), 12000.0 - 4000.0, 1e-6), "the ridge runs out to the gable");
    assert_eq!(lines(&g, RoofLineKind::Rake).len(), 2);
    let mut both = roof(&outline, RoofShape::Hip, 0.0);
    both.gable_sides = vec![1, 3];
    let id = put(&mut doc, both);
    let g = geometry(&doc, &id);
    assert_eq!(g.planes.len(), 2, "a gable");
    assert!(near(plan_len(lines(g, RoofLineKind::Ridge)[0]), 12000.0, 1e-6));
}

#[test]
fn a_top_floor_roof_that_does_not_fit_keeps_wave_1_and_says_so() {
    let mut doc = blank();
    let points = [(0.0, 0.0), (10000.0, 0.0), (10000.0, 4000.0), (5000.0, 4000.0), (5000.0, 8000.0), (0.0, 8000.0)]
        .map(|(x, y)| p(x, y))
        .to_vec();
    apply_checked(&mut doc, Command::AddWallChain { points, closed: true, thickness_mm: None, level_id: None, role: None });
    // A gable over an L: Wave 1's bounding box clipping, two planes.
    apply_checked(&mut doc, Command::SetRoof { roof: Roof { kind: RoofKind::Gable, ..defaults::default_roof() } });
    let g = doc.derived().roofs[0].clone();
    assert_eq!(g.planes.len(), 2);
    assert!(!g.infill.is_empty(), "gable ends over the walls");
    let found: Vec<&Issue> = doc.derived().issues.iter().filter(|i| i.code == "roof_outline").collect();
    assert_eq!(found.len(), 1);
    assert!(found[0].message.contains("bounding box"), "{}", found[0].message);
    // A hip fits an L: the skeleton, no suggestion.
    apply_checked(&mut doc, Command::SetRoof { roof: Roof { kind: RoofKind::Hip, ..defaults::default_roof() } });
    assert_eq!(doc.derived().roofs[0].planes.len(), 6);
    assert!(doc.derived().issues.iter().all(|i| i.code != "roof_outline"));
}

#[test]
fn the_sample_bungalow_roof_is_derived() {
    let doc = Document::new(guhit_core::templates::sample_bungalow());
    let g = &doc.derived().roofs;
    assert_eq!(g.len(), 1);
    assert!(!g[0].planes.is_empty());
    assert!(g[0].plan_area_m2 > 50.0);
}

/// A lanai shed on the north side of a house slopes down away from it: it
/// rises against its axis (`flip`), its eave on the outer, north edge.
#[test]
fn a_north_lanai_shed_slopes_down_away_from_the_house() {
    let mut doc = blank();
    apply_checked(&mut doc, rect_room(0.0, 0.0, 8000.0, 6000.0, Some("House")));
    let mut r = roof(&rect(500.0, 6075.0, 5500.0, 9075.0), RoofShape::Shed, 0.0);
    r.ridge_axis = Axis::Y;
    r.flip = true;
    r.eave_mm = Some(2400.0);
    let id = put(&mut doc, r);
    let g = geometry(&doc, &id);
    let z_at = |y: f64| g.planes[0].iter().filter(|v| near(v.y, y, 1e-6)).map(|v| v.z).fold(f64::NAN, f64::max);
    let (house, outer) = (z_at(6075.0), z_at(9075.0));
    assert!(near(outer, 2400.0 + vertical(), 1e-6), "the low eave on the outer edge: {outer}");
    assert!(near(house - outer, 3000.0 * tan(), 1e-6), "rising toward the house: {house}");
    let eave = lines(g, RoofLineKind::Eave);
    assert_eq!(eave.len(), 1);
    assert!(near(eave[0].a.y, 9075.0, 1e-6) && near(eave[0].b.y, 9075.0, 1e-6), "the gutter side is away from the house");
    // Without flip it rises the other way.
    let mut plain = roof(&rect(500.0, 6075.0, 5500.0, 9075.0), RoofShape::Shed, 0.0);
    plain.ridge_axis = Axis::Y;
    let id = put(&mut doc, plain);
    let g = geometry(&doc, &id);
    assert!(g.planes[0].iter().any(|v| near(v.y, 6075.0, 1e-6) && near(v.z, BASE + vertical(), 1e-6)));
}

#[test]
fn carport_posts_hold_up_its_open_eaves() {
    let mut doc = blank();
    // A house wall along the carport's north side; the rest is open.
    apply_checked(&mut doc, add_wall(p(0.0, 6000.0), p(3500.0, 6000.0)));
    let mut r = roof(&rect(0.0, 0.0, 3500.0, 6000.0), RoofShape::Shed, 300.0);
    r.ridge_axis = Axis::Y;
    let id = put(&mut doc, r);
    let found: Vec<&Issue> = doc.derived().issues.iter().filter(|i| i.code == "roof_unsupported").collect();
    assert_eq!(found.len(), 1, "{:?}", doc.derived().issues);
    let done = apply_checked(&mut doc, Command::AddRoofPosts { roof_id: id.clone(), size_mm: 150.0, max_spacing_mm: 3000.0 });
    let posts: Vec<&Column> = doc
        .project()
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Column(c) => Some(c),
            _ => None,
        })
        .collect();
    // The south corners, the middle of the south side (3.5 m) and of each
    // 6 m side; the north ends sit on the wall.
    assert_eq!(posts.len(), 5, "{posts:?}");
    assert_eq!(done.diff.added.len(), 5);
    for c in &posts {
        assert!(c.center.x > 0.0 && c.center.x < 3500.0 && c.center.y > 0.0 && c.center.y < 6000.0, "inside the outline");
    }
    assert!(doc.derived().issues.iter().all(|i| i.code != "roof_unsupported"), "{:?}", doc.derived().issues);
    // A second call has nothing left to do.
    let e = apply_rejected(&mut doc, Command::AddRoofPosts { roof_id: id, size_mm: 150.0, max_spacing_mm: 3000.0 });
    assert_eq!(code(&e), "no_open_eaves");
}

#[test]
fn ortho_unions_trace_their_outline() {
    let loops = guhit_core::roofs::ortho::rect_union(&[(0.0, 2.0, 0.0, 1.0), (1.0, 3.0, 0.0, 2.0)]);
    assert_eq!(loops.len(), 1);
    assert_eq!(loops[0].len(), 6);
    // Two boxes touching at a corner stay two.
    assert_eq!(guhit_core::roofs::ortho::rect_union(&[(0.0, 1.0, 0.0, 1.0), (1.0, 2.0, 1.0, 2.0)]).len(), 2);
}

#[test]
fn len3_is_used() {
    // Keep the helper honest: a hip's true length is its plan length over
    // the cosine of its own slope.
    let g = one(&rect(0.0, 0.0, 8000.0, 6000.0), RoofShape::Hip, 0.0);
    let h = lines(&g, RoofLineKind::Hip)[0];
    let rise = (h.b.z - h.a.z).abs();
    assert!(near(len3(h), plan_len(h).hypot(rise), 1e-9));
}
