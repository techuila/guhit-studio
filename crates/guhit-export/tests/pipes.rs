//! Pipes in every backend export: plan sheets (SVG, PDF), 2D DXF, 3D DXF and
//! IFC4. Test data: the sixteen concept runs on the sample bungalow
//! (`common::plumbing`).

mod common;

use std::collections::{HashMap, HashSet};

use common::*;
use guhit_export::dxf3d::{build_faces, faces_per_layer};
use guhit_export::ifc::{count_entities, dangling_refs, guid_for};
use guhit_export::pipes::{tube_quads, V3};
use guhit_export::sheet::Layout;
use guhit_export::{model_dxf3d, model_ifc, plan_dxf, plan_pdf, plan_svg, ExportError};
use guhit_model::*;
use svg2pdf::usvg;

/// Per system, counted by hand from `RUNS` and cross-checked with an
/// independent script: plan polylines, riser marks, straight segments, tube
/// faces (8 sides per segment, 3 quads per end cap, every bend mitred).
const EXPECT: [(PipeSystem, usize, usize, usize, usize); 4] = [
    (PipeSystem::ColdWater, 8, 7, 18, 186),
    (PipeSystem::HotWater, 4, 3, 8, 76),
    (PipeSystem::Drainage, 5, 4, 13, 140),
    (PipeSystem::Vent, 1, 1, 2, 22),
];

/// Token colors from src/styles/tokens.css, SVG group ids, DXF layers.
fn style(system: PipeSystem) -> (&'static str, &'static str, &'static str) {
    match system {
        PipeSystem::ColdWater => ("#2b7bd0", "pipes-cold-water", "P-DOMW-CPIP"),
        PipeSystem::HotWater => ("#e0563a", "pipes-hot-water", "P-DOMW-HPIP"),
        PipeSystem::Drainage => ("#9b6a35", "pipes-drainage", "P-SANR-PIPE"),
        PipeSystem::Vent => ("#3a9a5c", "pipes-vent", "P-SANR-VENT"),
        other => panic!("the plumbing fixture has no {other:?} runs"),
    }
}

const ALL_PAPERS: [(PaperSize, Orientation); 8] = [
    (PaperSize::A4, Orientation::Landscape),
    (PaperSize::A4, Orientation::Portrait),
    (PaperSize::A3, Orientation::Landscape),
    (PaperSize::A3, Orientation::Portrait),
    (PaperSize::A2, Orientation::Landscape),
    (PaperSize::A2, Orientation::Portrait),
    (PaperSize::A1, Orientation::Landscape),
    (PaperSize::A1, Orientation::Portrait),
];

fn parse_svg(svg: &str) -> usvg::Tree {
    let mut db = usvg::fontdb::Database::new();
    db.load_system_fonts();
    let options = usvg::Options {
        fontdb: std::sync::Arc::new(db),
        ..usvg::Options::default()
    };
    usvg::Tree::from_str(svg, &options).expect("usvg parses the sheet")
}

/// The markup of `<g id="...">` up to its first closing tag. The system
/// groups hold no nested groups.
fn group<'a>(svg: &'a str, id: &str) -> &'a str {
    let start = svg
        .find(&format!("<g id=\"{id}\""))
        .unwrap_or_else(|| panic!("group {id} missing"));
    let end = svg[start..].find("</g>").expect("group closes") + start;
    &svg[start..end]
}

fn stroke_widths(markup: &str) -> Vec<String> {
    markup
        .lines()
        .filter(|l| l.starts_with("<polyline"))
        .map(|l| {
            let at = l.find("stroke-width=\"").unwrap() + 14;
            l[at..at + l[at..].find('"').unwrap()].to_string()
        })
        .collect()
}

// ---------------------------------------------------------------- the sheet

#[test]
fn sheet_draws_every_system_in_its_color_and_style() {
    let s = plumbing();
    let out = plan_svg(&s.project, &s.derived, &options(PaperSize::A3, Orientation::Landscape)).unwrap();
    let svg = &out.data;
    assert!(svg.contains("<g id=\"pipes\" fill=\"none\" stroke-linejoin=\"round\">"));
    for (system, runs, risers, _, _) in EXPECT {
        let (color, id, _) = style(system);
        let g = group(svg, id);
        assert!(g.contains(&format!("stroke=\"{color}\"")), "{id} color");
        assert_eq!(g.matches("<polyline").count(), runs, "{id} runs");
        assert_eq!(g.matches("<circle").count(), risers, "{id} risers");
        // Risers are white circles in the system color.
        assert_eq!(g.matches("fill=\"#ffffff\"").count(), risers);
        let dashes: Vec<usize> = g
            .lines()
            .filter(|l| l.starts_with("<polyline"))
            .map(|l| match l.find("stroke-dasharray=\"") {
                Some(at) => l[at + 18..].split('"').next().unwrap().split(' ').count(),
                None => 0,
            })
            .collect();
        let want = match system {
            PipeSystem::ColdWater | PipeSystem::HotWater => 0, // solid
            PipeSystem::Drainage => 2,                         // dashed
            PipeSystem::Vent => 4,                             // dash-dot
            other => panic!("the plumbing fixture has no {other:?} runs"),
        };
        assert!(dashes.iter().all(|n| *n == want), "{id}: {dashes:?}");
    }
    // Pipes sit over the whole plan and over the white masks behind room
    // names, so no label cuts a run; the names themselves stay on top.
    let at = |needle: &str| svg.find(needle).unwrap_or_else(|| panic!("{needle}"));
    assert!(at("<g id=\"windows\"") < at("<g id=\"pipes\""));
    assert!(at("<g id=\"dimensions\"") < at("<g id=\"pipes\""));
    assert!(at("<g id=\"room-label-masks\"") < at("<g id=\"pipes\""));
    assert!(at("<g id=\"pipes\"") < at("<g id=\"room-labels\""));
    assert!(!group(svg, "room-labels").contains("<rect"), "masks moved below the pipes");
    // Wide drainage first, thin supply on top.
    assert!(at("id=\"pipes-drainage\"") < at("id=\"pipes-cold-water\""));
    parse_svg(svg);
}

#[test]
fn line_weight_follows_pipe_size_and_sheet_scale() {
    let s = plumbing();
    let mut opts = options(PaperSize::A3, Orientation::Landscape);
    opts.scale_denominator = Some(50);
    let svg = plan_svg(&s.project, &s.derived, &opts).unwrap().data;
    // 100 mm at 1:50 is 2 mm on paper, 50 mm is 1 mm, wider pipes first.
    assert_eq!(stroke_widths(group(&svg, "pipes-drainage")), ["2", "2", "1", "1", "1"]);
    // 25 mm service line 0.5, 20 mm lines 0.4.
    let cold = stroke_widths(group(&svg, "pipes-cold-water"));
    assert_eq!(cold[0], "0.5");
    assert!(cold[1..].iter().all(|w| w == "0.4"), "{cold:?}");

    opts.scale_denominator = Some(100);
    let svg = plan_svg(&s.project, &s.derived, &opts).unwrap().data;
    assert_eq!(stroke_widths(group(&svg, "pipes-drainage")), ["1", "1", "0.5", "0.5", "0.5"]);
    // Readable minimum: 20 mm at 1:100 would be 0.2 mm.
    assert!(stroke_widths(group(&svg, "pipes-cold-water")).iter().all(|w| w == "0.35"));
    assert!(stroke_widths(group(&svg, "pipes-hot-water")).iter().all(|w| w == "0.35"));
}

#[test]
fn the_sheet_takes_in_pipes_outside_the_building() {
    // The service line starts at the meter 1.8 m in front of the house and
    // the outlet runs to x = 11.6 m, so both must be on the sheet.
    let s = plumbing();
    let base = plain();
    let opts = options(PaperSize::A3, Orientation::Landscape);
    let with = plan_svg(&s.project, &s.derived, &opts).unwrap();
    let without = plan_svg(&base.project, &base.derived, &opts).unwrap();
    assert!(with.scale_denominator >= without.scale_denominator);
    let tree = parse_svg(&with.data);
    let px = tree.size().width() as f64 / 420.0;
    let pipes = tree.node_by_id("pipes").expect("pipes group").abs_stroke_bounding_box();
    let layout = Layout::new(PaperSize::A3, Orientation::Landscape, true);
    let (x0, y0) = (pipes.left() as f64 / px, pipes.top() as f64 / px);
    let (x1, y1) = (pipes.right() as f64 / px, pipes.bottom() as f64 / px);
    let (px0, py0, pw, ph) = layout.plan;
    assert!(x0 >= px0 - 0.01 && x1 <= px0 + pw + 0.01, "{x0}..{x1}");
    assert!(y0 >= py0 - 0.01 && y1 <= py0 + ph + 0.01, "{y0}..{y1}");
}

struct Box2 {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

fn mm_box(r: usvg::Rect, px: f64) -> Box2 {
    Box2 {
        x0: r.left() as f64 / px,
        y0: r.top() as f64 / px,
        x1: r.right() as f64 / px,
        y1: r.bottom() as f64 / px,
    }
}

fn text_box(group: &usvg::Group, needle: &str, px: f64) -> Option<Box2> {
    for node in group.children() {
        match node {
            usvg::Node::Text(t) => {
                if t.chunks().iter().any(|c| c.text() == needle) {
                    return Some(mm_box(t.abs_bounding_box(), px));
                }
            }
            usvg::Node::Group(g) => {
                if let Some(b) = text_box(g, needle, px) {
                    return Some(b);
                }
            }
            _ => {}
        }
    }
    None
}

#[test]
fn legend_sits_in_the_band_clear_of_title_block_title_and_scale_bar() {
    let s = plumbing();
    for (paper, orientation) in ALL_PAPERS {
        for title_block in [true, false] {
            let mut opts = options(paper, orientation);
            opts.title_block = title_block;
            let svg = plan_svg(&s.project, &s.derived, &opts).unwrap().data;
            let tree = parse_svg(&svg);
            let layout = Layout::new(paper, orientation, title_block);
            let px = tree.size().width() as f64 / layout.width;
            let legend = mm_box(
                tree.node_by_id("pipe-legend").expect("legend").abs_stroke_bounding_box(),
                px,
            );
            let what = format!("{paper:?} {orientation:?} title block {title_block}");
            let block_top = layout.height - layout.margin - layout.title_h;
            let band_top = block_top - layout.band_h;
            assert!(legend.y0 > band_top, "{what}: legend top {} above the band {band_top}", legend.y0);
            assert!(legend.y1 < block_top, "{what}: legend bottom {} reaches the title block {block_top}", legend.y1);
            let bar = text_box(tree.root(), "GRAPHIC SCALE", px).expect("scale caption");
            assert!(legend.x1 < bar.x0, "{what}: legend runs into the scale bar");
            let title = text_box(tree.root(), "GROUND FLOOR PLAN", px).expect("drawing title");
            assert!(legend.x0 > title.x1, "{what}: legend runs into the drawing title");
            let legend_svg = group(&svg, "pipe-legend");
            for label in ["PIPES", "Cold water", "Hot water", "Drainage", "Vent"] {
                assert!(legend_svg.contains(&format!(">{label}</text>")), "{what}: {label}");
            }
            // Dashed samples end on a full dash, so dashed never reads as
            // dash-dot.
            let samples: Vec<&str> = legend_svg.lines().filter(|l| l.starts_with("<line")).collect();
            assert_eq!(samples.len(), 4, "{what}");
            let mut dashed = 0;
            for l in samples {
                let attr = |name: &str| -> Option<f64> {
                    let key = format!(" {name}=\"");
                    let at = l.find(&key)? + key.len();
                    l[at..at + l[at..].find('"')?].split(' ').next()?.parse().ok()
                };
                let length = attr("x2").unwrap() - attr("x1").unwrap();
                let Some(at) = l.find("stroke-dasharray=\"") else { continue };
                let pattern: Vec<f64> = l[at + 18..].split('"').next().unwrap().split(' ').map(|x| x.parse().unwrap()).collect();
                let periods = (length - pattern[0]) / pattern.iter().sum::<f64>();
                assert!(periods.round() >= 1.0 && (periods - periods.round()).abs() < 0.01, "{what}: {l}");
                dashed += 1;
            }
            assert_eq!(dashed, 2, "{what}: drainage and vent samples");
        }
    }
}

#[test]
fn legend_lists_only_the_systems_drawn() {
    let mut s = plumbing();
    s.project
        .elements
        .retain(|e| !matches!(e, Element::Pipe(p) if p.system == PipeSystem::HotWater));
    set_layer(&mut s, LayerKey::Vent, false);
    let svg = plan_svg(&s.project, &s.derived, &options(PaperSize::A3, Orientation::Landscape))
        .unwrap()
        .data;
    let legend = group(&svg, "pipe-legend");
    assert!(legend.contains(">Cold water</text>") && legend.contains(">Drainage</text>"));
    assert!(!svg.contains(">Hot water<") && !svg.contains(">Vent<"));
    assert!(!svg.contains("#e0563a") && !svg.contains("#3a9a5c"), "no hot water or vent color");
    assert!(!svg.contains("pipes-vent") && !svg.contains("pipes-hot-water"));
}

#[test]
fn without_pipes_the_sheet_is_the_plain_plan() {
    let base = plain();
    let s = plumbing();
    for (paper, orientation) in [
        (PaperSize::A3, Orientation::Landscape),
        (PaperSize::A4, Orientation::Portrait),
    ] {
        let opts = options(paper, orientation);
        let plain_svg = plan_svg(&base.project, &base.derived, &opts).unwrap().data;
        let plain_dxf = plan_dxf(&base.project, &base.derived, &opts).unwrap().data;
        // Switched off in the options.
        let mut off = opts.clone();
        off.show_pipes = false;
        assert_eq!(plan_svg(&s.project, &s.derived, &off).unwrap().data, plain_svg);
        assert_eq!(plan_dxf(&s.project, &s.derived, &off).unwrap().data, plain_dxf);
        // Every pipe layer hidden.
        let mut hidden = s.clone();
        for key in [LayerKey::ColdWater, LayerKey::HotWater, LayerKey::Drainage, LayerKey::Vent] {
            set_layer(&mut hidden, key, false);
        }
        assert_eq!(plan_svg(&hidden.project, &hidden.derived, &opts).unwrap().data, plain_svg);
        assert_eq!(plan_dxf(&hidden.project, &hidden.derived, &opts).unwrap().data, plain_dxf);
    }
}

fn second_level(s: &mut DocState) {
    s.project.levels.push(Level {
        id: "level-2".into(),
        name: "Second Floor".into(),
        elevation_mm: 3000.0,
        height_mm: 3000.0,
    });
    let mut stack = pipe("level-2", &RUNS[15]);
    stack.id = "p-vt-stack-2".into();
    stack.points = vec![
        Vec3 { x: 7800.0, y: 6000.0, z: 0.0 },
        Vec3 { x: 7800.0, y: 6000.0, z: 1200.0 },
        Vec3 { x: 7300.0, y: 6000.0, z: 1200.0 },
    ];
    s.project.elements.push(Element::Pipe(stack));
}

#[test]
fn each_sheet_draws_the_pipes_of_its_own_level() {
    let mut s = plumbing();
    second_level(&mut s);
    let opts = options(PaperSize::A3, Orientation::Landscape);
    let ground = plan_svg(&s.project, &s.derived, &opts).unwrap().data;
    assert_eq!(group(&ground, "pipes-vent").matches("<polyline").count(), 1);

    // The upper level holds only the stack, and that is still a drawing.
    let mut upper = opts.clone();
    upper.level_id = Some("level-2".into());
    let svg = plan_svg(&s.project, &s.derived, &upper).unwrap().data;
    assert_eq!(group(&svg, "pipes-vent").matches("<polyline").count(), 1);
    assert_eq!(group(&svg, "pipes-vent").matches("<circle").count(), 1);
    assert!(!svg.contains("pipes-cold-water"));
    assert!(plan_dxf(&s.project, &s.derived, &upper).unwrap().data.contains("P-SANR-VENT"));
    assert!(plan_pdf(&s.project, &s.derived, &upper).unwrap().data.starts_with(b"%PDF"));
    // Without pipes it is empty, as before.
    upper.show_pipes = false;
    assert!(matches!(plan_svg(&s.project, &s.derived, &upper), Err(ExportError::Empty(_))));
    assert!(matches!(plan_dxf(&s.project, &s.derived, &upper), Err(ExportError::Empty(_))));
}

#[test]
fn pdf_sheet_with_pipes_converts() {
    let s = plumbing();
    let opts = options(PaperSize::A3, Orientation::Landscape);
    let pdf = plan_pdf(&s.project, &s.derived, &opts).unwrap();
    let svg = plan_svg(&s.project, &s.derived, &opts).unwrap();
    assert!(pdf.data.starts_with(b"%PDF"));
    assert_eq!(pdf.scale_denominator, svg.scale_denominator);
}

// ------------------------------------------------------------------- 2D DXF

fn load_dxf(text: &str) -> dxf::Drawing {
    dxf::Drawing::load(&mut text.as_bytes()).expect("dxf crate parses the output")
}

fn count_by_layer(drawing: &dxf::Drawing) -> HashMap<(String, &'static str), usize> {
    use dxf::entities::EntityType as T;
    let mut m = HashMap::new();
    for e in drawing.entities() {
        let kind = match &e.specific {
            T::Line(_) => "LINE",
            T::Polyline(_) => "POLYLINE",
            T::Arc(_) => "ARC",
            T::Circle(_) => "CIRCLE",
            T::Text(_) => "TEXT",
            T::Face3D(_) => "3DFACE",
            other => panic!("unexpected entity type {other:?}"),
        };
        *m.entry((e.common.layer.clone(), kind)).or_insert(0) += 1;
    }
    m
}

#[test]
fn dxf_puts_each_system_on_its_own_layer_and_linetype() {
    use dxf::entities::EntityType as T;
    let s = plumbing();
    let opts = options(PaperSize::A3, Orientation::Landscape);
    let text = plan_dxf(&s.project, &s.derived, &opts).unwrap().data;
    assert!(text.is_ascii());
    let d = load_dxf(&text);

    let layers: HashMap<String, &dxf::tables::Layer> = d.layers().map(|l| (l.name.clone(), l)).collect();
    for (name, aci, linetype) in [
        ("P-DOMW-CPIP", 150u8, "CONTINUOUS"),
        ("P-DOMW-HPIP", 20, "CONTINUOUS"),
        ("P-SANR-PIPE", 33, "GUHIT_DASHED"),
        ("P-SANR-VENT", 103, "GUHIT_DASHDOT"),
    ] {
        let l = layers.get(name).unwrap_or_else(|| panic!("layer {name} missing"));
        assert_eq!(l.color.index(), Some(aci), "{name} color");
        assert_eq!(l.line_type_name, linetype, "{name} linetype");
    }
    // Patterns plot at the project scale, 1:100: 2.2 mm dash, 1.1 mm gap.
    let lts: HashMap<String, &dxf::tables::LineType> =
        d.line_types().map(|l| (l.name.clone(), l)).collect();
    let dashed = lts["GUHIT_DASHED"];
    assert_eq!(dashed.dash_dot_space_lengths, vec![220.0, -110.0]);
    assert!((dashed.total_pattern_length - 330.0).abs() < 1e-9);
    let dashdot = lts["GUHIT_DASHDOT"];
    assert_eq!(dashdot.dash_dot_space_lengths, vec![300.0, -90.0, 0.0, -90.0]);
    assert!((dashdot.total_pattern_length - 480.0).abs() < 1e-9);

    let counts = count_by_layer(&d);
    let get = |layer: &str, kind: &'static str| counts.get(&(layer.to_string(), kind)).copied().unwrap_or(0);
    for (system, runs, risers, _, _) in EXPECT {
        let (_, _, layer) = style(system);
        assert_eq!(get(layer, "POLYLINE"), runs, "{layer} polylines");
        assert_eq!(get(layer, "CIRCLE"), risers, "{layer} riser circles");
    }
    // The building itself is drawn exactly as without pipes.
    let base = plain();
    let plain = count_by_layer(&load_dxf(&plan_dxf(&base.project, &base.derived, &opts).unwrap().data));
    let arch: HashMap<_, _> = counts.iter().filter(|((l, _), _)| !l.starts_with("P-")).map(|(k, v)| (k.clone(), *v)).collect();
    assert_eq!(arch, plain);

    // The service line in plan, and risers sized to plot at 1:100.
    let service = d
        .entities()
        .find_map(|e| match &e.specific {
            T::Polyline(p) if e.common.layer == "P-DOMW-CPIP" && p.vertices().count() == 4 => {
                Some(p.vertices().map(|v| (v.location.x, v.location.y)).collect::<Vec<_>>())
            }
            _ => None,
        })
        .expect("service line polyline");
    assert_eq!(service, vec![(600.0, -1800.0), (600.0, 5850.0), (2000.0, 5850.0), (2000.0, 6000.0)]);
    for e in d.entities() {
        if let T::Circle(c) = &e.specific {
            if e.common.layer.starts_with("P-") {
                assert!((c.radius - 110.0).abs() < 1e-9, "riser radius {}", c.radius);
                assert_eq!(e.common.line_type_name, "CONTINUOUS", "risers stay continuous");
            }
        }
    }
    // Extents take in the meter riser and the septic outlet.
    assert!(d.header.minimum_drawing_extents.y <= -1800.0 - 110.0 + 1e-6);
    assert!(d.header.maximum_drawing_extents.x >= 11600.0 - 1e-6);
}

// ------------------------------------------------------------------- 3D DXF

#[test]
fn dxf3d_draws_pipes_as_closed_tubes_on_their_layers() {
    use dxf::entities::EntityType as T;
    let s = plumbing();
    let text = model_dxf3d(&s.project, &s.derived).unwrap();
    let d = load_dxf(&text);
    let counts = count_by_layer(&d);
    let get = |layer: &str, kind: &'static str| counts.get(&(layer.to_string(), kind)).copied().unwrap_or(0);
    for (system, runs, risers, _, faces) in EXPECT {
        let (_, _, layer) = style(system);
        assert_eq!(get(layer, "3DFACE"), faces, "{layer} tube faces");
        // The plan linework at z = 0 is there too.
        assert_eq!(get(layer, "POLYLINE"), runs, "{layer} flat runs");
        assert_eq!(get(layer, "CIRCLE"), risers, "{layer} flat risers");
    }
    // Builder and file agree, pipe layers included.
    let built: HashMap<&str, usize> = faces_per_layer(&build_faces(&s.project, &s.derived)).into_iter().collect();
    for (system, _, _, _, faces) in EXPECT {
        assert_eq!(built[style(system).2], faces);
    }
    // Tubes keep continuous edges on the dashed layers.
    for e in d.entities() {
        if matches!(e.specific, T::Face3D(_)) && (e.common.layer == "P-SANR-PIPE" || e.common.layer == "P-SANR-VENT") {
            assert_eq!(e.common.line_type_name, "CONTINUOUS");
        }
    }
    // The building faces are the same as without pipes.
    let base = plain();
    let plain = count_by_layer(&load_dxf(&model_dxf3d(&base.project, &base.derived).unwrap()));
    let arch: HashMap<_, _> = counts.iter().filter(|((l, _), _)| !l.starts_with("P-")).map(|(k, v)| (k.clone(), *v)).collect();
    assert_eq!(arch, plain);
}

fn dist_to_segment(p: V3, a: V3, b: V3) -> f64 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.dot(ab)).clamp(0.0, 1.0);
    (p - (a + ab * t)).len()
}

#[test]
fn every_tube_is_watertight_and_hugs_its_centerline() {
    let s = plumbing();
    for e in &s.project.elements {
        let Element::Pipe(p) = e else { continue };
        let quads = tube_quads(p, 0.0, 8);
        assert!(!quads.is_empty(), "{}", p.id);
        // Each edge of a closed surface belongs to exactly two faces.
        type Key = (i64, i64, i64);
        let key = |v: V3| -> Key { ((v.x * 1000.0).round() as i64, (v.y * 1000.0).round() as i64, (v.z * 1000.0).round() as i64) };
        let mut edges: HashMap<(Key, Key), usize> = HashMap::new();
        for q in &quads {
            let mut ring: Vec<V3> = q.to_vec();
            if ring[2] == ring[3] {
                ring.pop();
            }
            for i in 0..ring.len() {
                let (a, b) = (key(ring[i]), key(ring[(i + 1) % ring.len()]));
                let k = if a < b { (a, b) } else { (b, a) };
                *edges.entry(k).or_default() += 1;
            }
        }
        // Cap fans share their inner diagonals, which also count twice.
        assert!(edges.values().all(|n| *n == 2), "{} is not closed: {:?}", p.id, edges.values().filter(|n| **n != 2).count());
        // Every corner lies on the pipe surface: at the radius from the
        // centerline, or up to the mitre reach at a bend.
        let r = p.diameter_mm / 2.0;
        let pts: Vec<V3> = p.points.iter().map(|q| V3 { x: q.x, y: q.y, z: q.z }).collect();
        for q in &quads {
            for v in q {
                let d = pts.windows(2).map(|w| dist_to_segment(*v, w[0], w[1])).fold(f64::INFINITY, f64::min);
                assert!(d > r - 1e-6 && d < 2.0 * r, "{}: corner {d} from the centerline, radius {r}", p.id);
            }
        }
    }
}

#[test]
fn tubes_sit_at_world_height() {
    let mut s = plumbing();
    second_level(&mut s);
    let faces = build_faces(&s.project, &s.derived);
    let vent: Vec<f64> = faces
        .list
        .iter()
        .filter(|f| f.layer == "P-SANR-VENT")
        .flat_map(|f| f.pts.iter().map(|p| p.z))
        .collect();
    let (lo, hi) = vent.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), z| (a.min(*z), b.max(*z)));
    // An octagon with flat sides at top and bottom reaches r cos 22.5 degrees
    // above and below the axis. Ground floor stack: lowest at the foot of
    // the 50 mm pipe at -457. Upper stack: horizontal at 3000 + 1200.
    let reach = 25.0 * 22.5f64.to_radians().cos();
    assert!((lo - (-457.0 - reach)).abs() < 0.5, "{lo}");
    assert!((hi - (3000.0 + 1200.0 + reach)).abs() < 1e-6, "{hi}");
    let upper = tube_quads(
        match s.project.elements.iter().find(|e| e.id() == "p-vt-stack-2") {
            Some(Element::Pipe(p)) => p,
            _ => panic!("stack"),
        },
        3000.0,
        8,
    );
    assert!(upper.iter().flatten().all(|p| p.z >= 3000.0 - 25.0 - 1e-6));
}

// --------------------------------------------------------------------- IFC4

fn entity_lines<'a>(ifc: &'a str, kw: &str) -> Vec<(usize, &'a str)> {
    ifc.lines()
        .filter_map(|l| {
            let (num, rest) = l.strip_prefix('#')?.split_once('=')?;
            (rest.starts_with(kw) && rest[kw.len()..].starts_with('(')).then(|| (num.parse().unwrap(), rest))
        })
        .collect()
}

fn refs(text: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'#' {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            out.push(text[i + 1..j].parse().unwrap());
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

#[test]
fn ifc_writes_pipe_segments_in_systems_with_materials() {
    let s = plumbing();
    let ifc = model_ifc(&s.project, &s.derived).unwrap();
    let c = count_entities(&ifc);
    let total: usize = EXPECT.iter().map(|e| e.3).sum();
    assert_eq!(total, 41);
    assert_eq!(c.pipe_segments, total, "one IfcPipeSegment per straight segment");
    assert_eq!(c.pipe_systems, 4);
    assert_eq!(ifc.matches("IFCRELASSIGNSTOGROUP(").count(), 4);
    assert_eq!(ifc.matches("IFCRELSERVICESBUILDINGS(").count(), 4);
    for kind in [".DOMESTICCOLDWATER.", ".DOMESTICHOTWATER.", ".DRAINAGE.", ".VENT."] {
        assert_eq!(entity_lines(&ifc, "IFCDISTRIBUTIONSYSTEM").iter().filter(|(_, l)| l.contains(kind)).count(), 1, "{kind}");
    }
    let segments = entity_lines(&ifc, "IFCPIPESEGMENT");
    assert!(segments.iter().all(|(_, l)| l.ends_with(",.RIGIDSEGMENT.);")));
    // Named after the pipe, or after its system and size when unnamed.
    assert_eq!(segments.iter().filter(|(_, l)| l.contains(",'Kitchen sink waste','Segment ")).count(), 3);
    assert_eq!(segments.iter().filter(|(_, l)| l.contains(",'Cold water 20 mm','Segment 1 of 1'")).count(), 1);
    // The body is a circle of the pipe size swept along the segment.
    assert!(ifc.contains("IFCCIRCLEPROFILEDEF(.AREA.,$,") && ifc.contains(",12.5);") && ifc.contains(",50.);"));
    assert!(ifc.contains("'NominalDiameter',$,IFCPOSITIVELENGTHMEASURE(100.)"));

    // Every segment belongs to its storey and to exactly one system.
    let seg_ids: HashSet<usize> = segments.iter().map(|(n, _)| *n).collect();
    let contained: HashSet<usize> = entity_lines(&ifc, "IFCRELCONTAINEDINSPATIALSTRUCTURE")
        .iter()
        .flat_map(|(_, l)| refs(l))
        .collect();
    assert!(seg_ids.is_subset(&contained), "segments outside the storey");
    let systems: HashMap<usize, &str> = entity_lines(&ifc, "IFCDISTRIBUTIONSYSTEM").into_iter().collect();
    let mut per_system: HashMap<&str, usize> = HashMap::new();
    let mut seen: HashSet<usize> = HashSet::new();
    for (_, l) in entity_lines(&ifc, "IFCRELASSIGNSTOGROUP") {
        let r = refs(l);
        let (group, members) = r.split_last().unwrap();
        let name = systems[group].split('\'').nth(3).unwrap();
        for m in members {
            assert!(seg_ids.contains(m) && seen.insert(*m), "#{m} assigned twice or not a segment");
        }
        per_system.insert(name, members.len());
    }
    assert_eq!(seen.len(), total);
    for (system, _, _, segs, _) in EXPECT {
        let label = match system {
            PipeSystem::ColdWater => "Cold water",
            PipeSystem::HotWater => "Hot water",
            PipeSystem::Drainage => "Drainage",
            PipeSystem::Vent => "Vent",
            other => panic!("the plumbing fixture has no {other:?} runs"),
        };
        assert_eq!(per_system[label], segs, "{label}");
    }
    // Each system serves the building.
    let building = entity_lines(&ifc, "IFCBUILDING")[0].0;
    for (_, l) in entity_lines(&ifc, "IFCRELSERVICESBUILDINGS") {
        assert_eq!(*refs(l).last().unwrap(), building);
    }

    // Materials by name: PE service line, PPR supply, uPVC sanitary.
    let materials: HashMap<usize, String> = entity_lines(&ifc, "IFCMATERIAL")
        .into_iter()
        .map(|(n, l)| (n, l.split('\'').nth(1).unwrap().to_string()))
        .collect();
    let mut material_of: HashMap<usize, String> = HashMap::new();
    for (_, l) in entity_lines(&ifc, "IFCRELASSOCIATESMATERIAL") {
        let r = refs(l);
        let (m, elements) = r.split_last().unwrap();
        for e in elements {
            material_of.insert(*e, materials[m].clone());
        }
    }
    let mut count: HashMap<&str, usize> = HashMap::new();
    for n in &seg_ids {
        *count.entry(material_of[n].as_str()).or_default() += 1;
    }
    assert_eq!(count.get("PE"), Some(&5));
    assert_eq!(count.get("PPR"), Some(&(18 - 5 + 8)));
    assert_eq!(count.get("uPVC"), Some(&(13 + 2)));
}

#[test]
fn ifc_pipe_file_is_consistent_and_ids_are_stable() {
    let s = plumbing();
    let a = model_ifc(&s.project, &s.derived).unwrap();
    assert!(dangling_refs(&a).is_empty(), "{:?}", dangling_refs(&a));
    let mut expected = 0;
    for line in a.lines() {
        if let Some(rest) = line.strip_prefix('#') {
            expected += 1;
            assert_eq!(rest.split_once('=').unwrap().0.parse::<usize>().unwrap(), expected);
        }
    }
    assert_eq!(a, model_ifc(&s.project, &s.derived).unwrap(), "deterministic");
    assert!(a.contains(&format!("IFCPIPESEGMENT('{}'", guid_for("pipe-segment-0", "p-cw-main"))));

    let ids = |text: &str| -> Vec<String> {
        text.lines()
            .filter_map(|l| {
                let rest = l.split_once('=')?.1;
                let body = &rest[rest.find('(')? + 1..];
                let g = body.strip_prefix('\'')?.split('\'').next()?;
                (g.len() == 22).then(|| g.to_string())
            })
            .collect()
    };
    let first = ids(&a);
    let unique: HashSet<&String> = first.iter().collect();
    assert_eq!(unique.len(), first.len(), "duplicate GlobalId");
    // Moving a pipe point keeps every id.
    let mut moved = s.clone();
    for e in &mut moved.project.elements {
        if let Element::Pipe(p) = e {
            p.points[0].z -= 50.0;
        }
    }
    let b = model_ifc(&moved.project, &moved.derived).unwrap();
    assert_ne!(a, b);
    assert_eq!(first, ids(&b));
}

#[test]
fn a_project_with_only_pipes_still_exports() {
    let mut s = plumbing();
    s.project.elements.retain(|e| matches!(e, Element::Pipe(_)));
    s.derived = Derived::default();
    let ifc = model_ifc(&s.project, &s.derived).unwrap();
    assert_eq!(count_entities(&ifc).pipe_segments, 41);
    assert_eq!(count_entities(&ifc).walls, 0);
    let d3 = model_dxf3d(&s.project, &s.derived).unwrap();
    assert!(d3.contains("P-SANR-PIPE"));
    let opts = options(PaperSize::A3, Orientation::Landscape);
    plan_svg(&s.project, &s.derived, &opts).unwrap();
    plan_dxf(&s.project, &s.derived, &opts).unwrap();
}

// ------------------------------------------------------------------ samples

fn write_png(svg: &str, path: &std::path::Path, px_per_mm: f32, region: Option<(f32, f32, f32, f32)>) {
    let tree = parse_svg(svg);
    let scale = px_per_mm * 25.4 / 96.0;
    let (x, y, w, h) = region.unwrap_or((0.0, 0.0, tree.size().width() * 25.4 / 96.0, tree.size().height() * 25.4 / 96.0));
    let mut pixmap = resvg::tiny_skia::Pixmap::new((w * px_per_mm).ceil() as u32, (h * px_per_mm).ceil() as u32).expect("pixmap");
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale).post_translate(-x * px_per_mm, -y * px_per_mm);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.save_png(path).expect("write png");
}

/// Files to look at: target/export-samples/plumbing-*.
#[test]
fn write_pipe_samples() {
    let dir = samples_dir();
    let s = plumbing();
    for (name, paper, orientation) in [
        ("plumbing-a3", PaperSize::A3, Orientation::Landscape),
        ("plumbing-a4-portrait", PaperSize::A4, Orientation::Portrait),
        ("plumbing-a1", PaperSize::A1, Orientation::Landscape),
    ] {
        let opts = options(paper, orientation);
        let svg = plan_svg(&s.project, &s.derived, &opts).unwrap();
        std::fs::write(dir.join(format!("{name}.svg")), &svg.data).unwrap();
        write_png(&svg.data, &dir.join(format!("{name}.png")), 6.0, None);
        let pdf = plan_pdf(&s.project, &s.derived, &opts).unwrap();
        std::fs::write(dir.join(format!("{name}.pdf")), &pdf.data).unwrap();
        let dxf = plan_dxf(&s.project, &s.derived, &opts).unwrap();
        std::fs::write(dir.join(format!("{name}.dxf")), &dxf.data).unwrap();
    }
    // Close-ups on A3 at 1:50: the bathroom and kitchen wall, and the band.
    let mut opts = options(PaperSize::A3, Orientation::Landscape);
    opts.scale_denominator = Some(50);
    let svg = plan_svg(&s.project, &s.derived, &opts).unwrap().data;
    std::fs::write(dir.join("plumbing-a3-50.svg"), &svg).unwrap();
    write_png(&svg, &dir.join("plumbing-a3-50.png"), 6.0, None);
    let layout = Layout::new(PaperSize::A3, Orientation::Landscape, true);
    let band = layout.height - layout.margin - layout.title_h - layout.band_h;
    write_png(&svg, &dir.join("plumbing-a3-band.png"), 12.0, Some((0.0, (band - 4.0) as f32, 420.0, (layout.band_h + layout.title_h + 12.0) as f32)));
    write_png(&svg, &dir.join("plumbing-a3-50-detail.png"), 14.0, Some((150.0, 20.0, 150.0, 120.0)));

    let ifc = model_ifc(&s.project, &s.derived).unwrap();
    std::fs::write(dir.join("plumbing.ifc"), &ifc).unwrap();
    let d3 = model_dxf3d(&s.project, &s.derived).unwrap();
    std::fs::write(dir.join("plumbing-3d.dxf"), &d3).unwrap();
    let mut two = plumbing();
    second_level(&mut two);
    std::fs::write(dir.join("plumbing-two-levels.ifc"), model_ifc(&two.project, &two.derived).unwrap()).unwrap();
    std::fs::write(dir.join("plumbing-two-levels-3d.dxf"), model_dxf3d(&two.project, &two.derived).unwrap()).unwrap();
}
