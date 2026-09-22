mod common;

use std::collections::HashMap;

use common::*;
use guhit_export::{plan_dxf, plan_pdf, plan_svg, ExportError};
use guhit_model::*;
use svg2pdf::usvg;

const ALL_PAPERS: [(PaperSize, f64, f64); 4] = [
    (PaperSize::A4, 210.0, 297.0),
    (PaperSize::A3, 297.0, 420.0),
    (PaperSize::A2, 420.0, 594.0),
    (PaperSize::A1, 594.0, 841.0),
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

// ------------------------------------------------------------------------ SVG

#[test]
fn svg_has_true_paper_size_for_every_sheet() {
    let s = fixture();
    for (paper, short, long) in ALL_PAPERS {
        for orientation in [Orientation::Landscape, Orientation::Portrait] {
            let (w, h) = match orientation {
                Orientation::Landscape => (long, short),
                Orientation::Portrait => (short, long),
            };
            let out = plan_svg(&s.project, &s.derived, &options(paper, orientation)).unwrap();
            assert!(out.data.contains(&format!("width=\"{w}mm\"")), "{paper:?} {orientation:?}");
            assert!(out.data.contains(&format!("height=\"{h}mm\"")));
            assert!(out.data.contains(&format!("viewBox=\"0 0 {w} {h}\"")));
            // usvg reports CSS pixels at 96 per inch.
            let tree = parse_svg(&out.data);
            let px = 96.0 / 25.4;
            assert!((tree.size().width() as f64 - w * px).abs() < 0.01);
            assert!((tree.size().height() as f64 - h * px).abs() < 0.01);
        }
    }
}

#[test]
fn svg_contains_rooms_areas_and_title_block() {
    let s = fixture();
    let out = plan_svg(
        &s.project,
        &s.derived,
        &options(PaperSize::A3, Orientation::Landscape),
    )
    .unwrap();
    let svg = &out.data;
    for e in &s.project.elements {
        if let Element::Room(r) = e {
            assert!(svg.contains(&r.name), "room name {} missing", r.name);
            let geo = s.derived.rooms.iter().find(|g| g.room_id == r.id);
            if let Some(g) = geo {
                let area = format!("{:.2} m\u{00b2}", g.area_mm2 / 1.0e6);
                assert!(svg.contains(&area), "area {area} missing");
            }
        }
    }
    assert!(svg.contains(&s.project.name));
    assert!(svg.contains(&s.project.settings.client_name));
    assert!(svg.contains(&s.project.settings.location));
    assert!(svg.contains("Guhit Studio"));
    assert!(svg.contains(&format!("1:{}", out.scale_denominator)));
    assert!(svg.contains(">N<"), "north arrow label");
    assert!(svg.contains("GRAPHIC SCALE"));
    assert!(svg.contains("#14283f"));
    assert!(!svg.contains('\u{2014}') && !svg.contains('\u{2013}'), "no long dashes");
}

#[test]
fn svg_auto_scale_fits_and_is_a_common_scale() {
    let s = fixture();
    let a3 = plan_svg(&s.project, &s.derived, &options(PaperSize::A3, Orientation::Landscape)).unwrap();
    let a4 = plan_svg(&s.project, &s.derived, &options(PaperSize::A4, Orientation::Landscape)).unwrap();
    let a1 = plan_svg(&s.project, &s.derived, &options(PaperSize::A1, Orientation::Landscape)).unwrap();
    for out in [&a3, &a4, &a1] {
        assert!(guhit_export::COMMON_SCALES.contains(&out.scale_denominator));
    }
    // Smaller paper can never get a larger drawing.
    assert!(a4.scale_denominator >= a3.scale_denominator);
    assert!(a3.scale_denominator >= a1.scale_denominator);
}

#[test]
fn svg_forced_scale_is_used_even_when_it_overflows() {
    let s = fixture();
    let mut opts = options(PaperSize::A4, Orientation::Portrait);
    opts.scale_denominator = Some(20);
    let out = plan_svg(&s.project, &s.derived, &opts).unwrap();
    assert_eq!(out.scale_denominator, 20);
    assert!(out.data.contains("SCALE 1:20"));
    // The oversized plan is clipped at the border, the sheet is still valid.
    assert!(out.data.contains("clip-path=\"url(#plan-clip)\""));
    parse_svg(&out.data);
    // An odd scale is accepted as given.
    opts.scale_denominator = Some(333);
    assert_eq!(plan_svg(&s.project, &s.derived, &opts).unwrap().scale_denominator, 333);
}

#[test]
fn svg_options_switch_content_off() {
    let s = fixture();
    let mut opts = options(PaperSize::A3, Orientation::Landscape);
    opts.show_dimensions = false;
    opts.show_room_labels = false;
    opts.show_assets = false;
    opts.title_block = false;
    let svg = plan_svg(&s.project, &s.derived, &opts).unwrap().data;
    assert!(!svg.contains("id=\"dimensions\""));
    assert!(!svg.contains("id=\"room-labels\""));
    assert!(!svg.contains("id=\"assets\""));
    assert!(!svg.contains("DESIGNED BY"));
    assert!(svg.contains("id=\"walls\""));
}

#[test]
fn svg_respects_layers_and_level() {
    let mut s = fixture();
    for l in &mut s.project.layers {
        if l.key == LayerKey::Rooms || l.key == LayerKey::Openings {
            l.visible = false;
        }
    }
    let opts = options(PaperSize::A3, Orientation::Landscape);
    let svg = plan_svg(&s.project, &s.derived, &opts).unwrap().data;
    assert!(!svg.contains("id=\"room-labels\""));
    assert!(!svg.contains("id=\"doors\"") && !svg.contains("id=\"windows\""));

    // A second, empty level exports nothing.
    let mut s = fixture();
    s.project.levels.push(Level {
        id: "level-2".into(),
        name: "Second Floor".into(),
        elevation_mm: 3000.0,
        height_mm: 3000.0,
    });
    let mut opts = options(PaperSize::A3, Orientation::Landscape);
    opts.level_id = Some("level-2".into());
    assert!(matches!(
        plan_svg(&s.project, &s.derived, &opts),
        Err(ExportError::Empty(_))
    ));
    opts.level_id = Some("missing".into());
    assert!(matches!(
        plan_svg(&s.project, &s.derived, &opts),
        Err(ExportError::Failed(_))
    ));
}

#[test]
fn empty_project_is_reported_as_empty() {
    let mut s = fixture();
    s.project.elements.clear();
    s.derived = Derived::default();
    let opts = options(PaperSize::A3, Orientation::Landscape);
    assert!(matches!(plan_svg(&s.project, &s.derived, &opts), Err(ExportError::Empty(_))));
    assert!(matches!(plan_pdf(&s.project, &s.derived, &opts), Err(ExportError::Empty(_))));
    assert!(matches!(plan_dxf(&s.project, &s.derived, &opts), Err(ExportError::Empty(_))));
    // Cameras alone are not drawable either, and no levels is also empty.
    s.project.levels.clear();
    assert!(matches!(plan_svg(&s.project, &s.derived, &opts), Err(ExportError::Empty(_))));
}

#[test]
fn missing_derived_data_still_draws() {
    let s = fixture();
    let opts = options(PaperSize::A3, Orientation::Landscape);
    let out = plan_svg(&s.project, &Derived::default(), &opts).unwrap();
    // Walls fall back to plain rectangles, room names move to their seeds.
    assert!(out.data.contains("id=\"walls\""));
    for e in &s.project.elements {
        if let Element::Room(r) = e {
            assert!(out.data.contains(&r.name));
        }
    }
    parse_svg(&out.data);
    plan_dxf(&s.project, &Derived::default(), &opts).unwrap();
}

#[test]
fn display_unit_meters_changes_dimension_text() {
    let mut s = rich();
    let opts = options(PaperSize::A3, Orientation::Landscape);
    let mm = plan_svg(&s.project, &s.derived, &opts).unwrap().data;
    assert!(mm.contains(">8000<") && mm.contains(">6000<"));
    s.project.settings.display_unit = DisplayUnit::M;
    let m = plan_svg(&s.project, &s.derived, &opts).unwrap().data;
    assert!(m.contains(">8.00<") && m.contains(">6.00<"));
    assert!(!m.contains(">8000<"));
}

#[test]
fn long_dashes_never_reach_the_sheet() {
    let mut s = rich();
    s.project.name = "Casa \u{2014} Uno".into();
    for e in &mut s.project.elements {
        if let Element::Room(r) = e {
            r.name = format!("{} \u{2013} A", r.name);
        }
    }
    let opts = options(PaperSize::A3, Orientation::Landscape);
    let svg = plan_svg(&s.project, &s.derived, &opts).unwrap().data;
    assert!(!svg.contains('\u{2014}') && !svg.contains('\u{2013}'));
    assert!(svg.contains("Casa - Uno"));
    let dxf = plan_dxf(&s.project, &s.derived, &opts).unwrap().data;
    assert!(dxf.is_ascii());
}

// ------------------------------------------------------------------------ PDF

fn media_box(pdf: &[u8]) -> [f64; 4] {
    let key = b"/MediaBox";
    let at = pdf
        .windows(key.len())
        .position(|w| w == key)
        .expect("MediaBox present");
    let rest = &pdf[at + key.len()..];
    let open = rest.iter().position(|b| *b == b'[').unwrap();
    let close = rest.iter().position(|b| *b == b']').unwrap();
    let inner = std::str::from_utf8(&rest[open + 1..close]).unwrap();
    let nums: Vec<f64> = inner.split_whitespace().map(|n| n.parse().unwrap()).collect();
    [nums[0], nums[1], nums[2], nums[3]]
}

#[test]
fn pdf_media_box_matches_every_paper_size() {
    let s = fixture();
    for (paper, short, long) in ALL_PAPERS {
        for orientation in [Orientation::Landscape, Orientation::Portrait] {
            let (w, h) = match orientation {
                Orientation::Landscape => (long, short),
                Orientation::Portrait => (short, long),
            };
            let out = plan_pdf(&s.project, &s.derived, &options(paper, orientation)).unwrap();
            assert!(out.data.starts_with(b"%PDF"));
            let mb = media_box(&out.data);
            let pt = 72.0 / 25.4;
            assert_eq!((mb[0], mb[1]), (0.0, 0.0));
            assert!((mb[2] - w * pt).abs() < 0.02, "{paper:?} {orientation:?}: {mb:?}");
            assert!((mb[3] - h * pt).abs() < 0.02, "{paper:?} {orientation:?}: {mb:?}");
            assert_eq!(
                out.data.windows(10).filter(|w| w == b"/Type /Pag").count(),
                2,
                "one page tree and one page"
            );
        }
    }
}

#[test]
fn pdf_embeds_a_font_and_matches_svg_scale() {
    let s = fixture();
    let opts = options(PaperSize::A3, Orientation::Landscape);
    let pdf = plan_pdf(&s.project, &s.derived, &opts).unwrap();
    let svg = plan_svg(&s.project, &s.derived, &opts).unwrap();
    assert_eq!(pdf.scale_denominator, svg.scale_denominator);
    let has = |needle: &[u8]| pdf.data.windows(needle.len()).any(|w| w == needle);
    assert!(has(b"/FontFile") || has(b"/FontFile2") || has(b"/FontFile3"), "text is embedded as a font");
}

// ------------------------------------------------------------------------ DXF

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
            other => panic!("unexpected entity type {other:?}"),
        };
        *m.entry((e.common.layer.clone(), kind)).or_insert(0) += 1;
    }
    m
}

const LAYERS: [&str; 9] = [
    "A-WALL",
    "A-DOOR",
    "A-GLAZ",
    "A-COLS",
    "A-FLOR-STRS",
    "A-FURN",
    "A-AREA",
    "A-ANNO-DIMS",
    "A-ANNO-TEXT",
];

#[test]
fn dxf_fixture_parses_with_layers_and_counts() {
    let s = fixture();
    let opts = options(PaperSize::A3, Orientation::Landscape);
    let out = plan_dxf(&s.project, &s.derived, &opts).unwrap();
    assert!(out.data.is_ascii());
    assert!(out.data.contains("AC1009"));
    assert!(out.data.trim_end().ends_with("EOF"));
    assert_eq!(out.scale_denominator, s.project.settings.scale_denominator);

    let drawing = load_dxf(&out.data);
    let names: Vec<String> = drawing.layers().map(|l| l.name.clone()).collect();
    for l in LAYERS {
        assert!(names.iter().any(|n| n == l), "layer {l} missing in {names:?}");
    }

    let counts = count_by_layer(&drawing);
    let get = |layer: &str, kind: &'static str| counts.get(&(layer.to_string(), kind)).copied().unwrap_or(0);

    // One ARC per swing leaf.
    assert_eq!(get("A-DOOR", "ARC"), swing_leaf_count(&s.project));
    assert!(swing_leaf_count(&s.project) >= 1);

    // Walls are closed polylines, split where openings cut them: a wall with
    // k separate openings inside its length gives k + 1 pieces.
    let walls = s.project.elements.iter().filter(|e| matches!(e, Element::Wall(_))).count();
    let openings = s.project.elements.iter().filter(|e| matches!(e, Element::Opening(_))).count();
    // Corners whose derived outlines are not mitred get one patch each, so
    // allow up to one extra polyline per wall end.
    let wall_polys = get("A-WALL", "POLYLINE");
    assert!(wall_polys >= walls + openings, "{wall_polys}");
    assert!(wall_polys <= walls + openings + walls * 2, "{wall_polys}");

    // Windows: two face lines and at least one glass line each.
    let windows = s
        .project
        .elements
        .iter()
        .filter(|e| matches!(e, Element::Opening(o) if o.opening_type == OpeningType::Window))
        .count();
    assert!(get("A-GLAZ", "LINE") >= windows * 3);

    // Room labels: name and area per room.
    let rooms = s.project.elements.iter().filter(|e| matches!(e, Element::Room(_))).count();
    assert_eq!(get("A-AREA", "TEXT"), rooms * 2);

    // A dimension explodes into 2 extension lines, 1 dim line, 2 ticks, 1 text.
    let dims = s.project.elements.iter().filter(|e| matches!(e, Element::Dimension(_))).count();
    assert_eq!(get("A-ANNO-DIMS", "LINE"), dims * 5);
    assert_eq!(get("A-ANNO-DIMS", "TEXT"), dims);

    let assets = s.project.elements.iter().filter(|e| matches!(e, Element::Asset(_))).count();
    assert_eq!(get("A-FURN", "POLYLINE"), assets);
}

#[test]
fn dxf_geometry_is_millimeters_y_up_and_closed() {
    use dxf::entities::EntityType as T;
    let s = fixture();
    let opts = options(PaperSize::A3, Orientation::Landscape);
    let drawing = load_dxf(&plan_dxf(&s.project, &s.derived, &opts).unwrap().data);

    // Wall polylines cover the same extent as the derived outlines, in mm.
    let mut want = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for g in &s.derived.walls {
        for p in &g.outline {
            want = (want.0.min(p.x), want.1.min(p.y), want.2.max(p.x), want.3.max(p.y));
        }
    }
    let mut got = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for e in drawing.entities() {
        if let T::Polyline(pl) = &e.specific {
            if e.common.layer == "A-WALL" {
                assert!(pl.is_closed(), "wall polylines are closed");
                assert!(pl.vertices().count() >= 3);
                for v in pl.vertices() {
                    got = (
                        got.0.min(v.location.x),
                        got.1.min(v.location.y),
                        got.2.max(v.location.x),
                        got.3.max(v.location.y),
                    );
                }
            }
        }
    }
    assert!((got.0 - want.0).abs() < 0.01 && (got.1 - want.1).abs() < 0.01, "{got:?} vs {want:?}");
    assert!((got.2 - want.2).abs() < 0.01 && (got.3 - want.3).abs() < 0.01, "{got:?} vs {want:?}");
    assert!(got.2 - got.0 > 1000.0, "coordinates are millimeters, not meters");

    // Every door arc is a quarter circle whose radius is the leaf width, and
    // its center sits on a face of the host wall.
    for e in drawing.entities() {
        if let T::Arc(arc) = &e.specific {
            let sweep = (arc.end_angle - arc.start_angle).rem_euclid(360.0);
            assert!((sweep - 90.0).abs() < 1e-6, "sweep {sweep}");
            let ok = s.project.elements.iter().any(|el| match el {
                Element::Opening(o) => (o.width_mm - arc.radius).abs() < 1e-6 || (o.width_mm / 2.0 - arc.radius).abs() < 1e-6,
                _ => false,
            });
            assert!(ok, "arc radius {} matches no door", arc.radius);
        }
    }

    // Room label text sits at the derived label point (y up, not flipped).
    for g in &s.derived.rooms {
        let near = drawing.entities().any(|e| match &e.specific {
            T::Text(t) if e.common.layer == "A-AREA" => {
                (t.second_alignment_point.x - g.label_point.x).abs() < 1.0
                    && (t.second_alignment_point.y - g.label_point.y).abs() < 600.0
            }
            _ => false,
        });
        assert!(near, "label near {:?}", g.label_point);
    }
}

#[test]
fn dxf_swing_side_and_hinge_follow_the_flags() {
    use dxf::entities::EntityType as T;
    // One wall along +x, one door. Default: hinge at the start side jamb,
    // leaf on the left (+y). Flipped: hinge at the far jamb, leaf on -y.
    let mut s = rich();
    let lvl = level_id(&s);
    s.derived = Derived::default();
    let door = |flip_side, flip_hinge| Opening {
        id: "o".into(),
        wall_id: "w".into(),
        opening_type: OpeningType::Door,
        style: OpeningStyle::SwingSingle,
        offset_mm: 2000.0,
        width_mm: 900.0,
        height_mm: 2100.0,
        sill_mm: 0.0,
        flip_side,
        flip_hinge,
        material_id: None,
    };
    for (flip_side, flip_hinge, cx, cy, a0, a1) in [
        (false, false, 1550.0, 100.0, 0.0, 90.0),
        (false, true, 2450.0, 100.0, 90.0, 180.0),
        (true, false, 1550.0, -100.0, 270.0, 0.0),
        (true, true, 2450.0, -100.0, 180.0, 270.0),
    ] {
        s.project.elements = vec![
            Element::Wall(Wall {
                id: "w".into(),
                level_id: lvl.clone(),
                start: Point { x: 0.0, y: 0.0 },
                end: Point { x: 4000.0, y: 0.0 },
                thickness_mm: 200.0,
                height_mm: None,
                material_id: None,
            }),
            Element::Opening(door(flip_side, flip_hinge)),
        ];
        let opts = options(PaperSize::A3, Orientation::Landscape);
        let drawing = load_dxf(&plan_dxf(&s.project, &s.derived, &opts).unwrap().data);
        let arcs: Vec<_> = drawing
            .entities()
            .filter_map(|e| match &e.specific {
                T::Arc(a) => Some(a.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(arcs.len(), 1);
        let a = &arcs[0];
        assert!((a.center.x - cx).abs() < 1e-6 && (a.center.y - cy).abs() < 1e-6, "{flip_side} {flip_hinge}: {:?}", a.center);
        assert!((a.radius - 900.0).abs() < 1e-6);
        assert!((a.start_angle - a0).abs() < 1e-6 && (a.end_angle - a1).abs() < 1e-6, "{flip_side} {flip_hinge}: {} {}", a.start_angle, a.end_angle);
    }
}

#[test]
fn dxf_rich_project_covers_every_layer() {
    let s = rich();
    let opts = options(PaperSize::A3, Orientation::Landscape);
    let out = plan_dxf(&s.project, &s.derived, &opts).unwrap();
    let drawing = load_dxf(&out.data);
    let counts = count_by_layer(&drawing);
    let get = |layer: &str, kind: &'static str| counts.get(&(layer.to_string(), kind)).copied().unwrap_or(0);
    assert_eq!(get("A-DOOR", "ARC"), swing_leaf_count(&s.project));
    assert_eq!(get("A-DOOR", "ARC"), 5); // three single leaves and one double
    assert_eq!(get("A-COLS", "POLYLINE"), 1);
    assert_eq!(get("A-COLS", "CIRCLE"), 1);
    assert_eq!(get("A-FLOR-STRS", "TEXT"), 1);
    assert_eq!(get("A-FLOR-STRS", "LINE"), 10 + 1); // inner risers and the arrow shaft
    assert_eq!(get("A-ANNO-TEXT", "TEXT"), 2);
    assert_eq!(get("A-ANNO-DIMS", "TEXT"), 3);
    assert_eq!(get("A-AREA", "TEXT"), 6);
    for l in LAYERS {
        assert!(counts.keys().any(|(layer, _)| layer == l), "no entities on {l}");
    }
}

// -------------------------------------------------------------------- samples

fn write_png(svg: &str, path: &std::path::Path, px_per_mm: f32) {
    let tree = parse_svg(svg);
    let size = tree.size();
    let scale = px_per_mm * 25.4 / 96.0;
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).expect("pixmap");
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.save_png(path).expect("write png");
}

/// Close-up of a sheet region given in paper mm (x, y, width, height).
fn write_png_region(svg: &str, path: &std::path::Path, px_per_mm: f32, region: (f32, f32, f32, f32)) {
    let tree = parse_svg(svg);
    let scale = px_per_mm * 25.4 / 96.0;
    let (w, h) = ((region.2 * px_per_mm).ceil() as u32, (region.3 * px_per_mm).ceil() as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).expect("pixmap");
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale)
        .post_translate(-region.0 * px_per_mm, -region.1 * px_per_mm);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.save_png(path).expect("write png");
}

/// Writes the files a person should look at: target/export-samples/.
#[test]
fn write_sample_outputs() {
    let dir = samples_dir();
    for (name, state, paper, orientation) in [
        ("bungalow-a3", fixture(), PaperSize::A3, Orientation::Landscape),
        ("bungalow-a4-portrait", fixture(), PaperSize::A4, Orientation::Portrait),
        ("rich-a3", rich(), PaperSize::A3, Orientation::Landscape),
        ("rich-a1", rich(), PaperSize::A1, Orientation::Landscape),
    ] {
        let opts = options(paper, orientation);
        let svg = plan_svg(&state.project, &state.derived, &opts).unwrap();
        std::fs::write(dir.join(format!("{name}.svg")), &svg.data).unwrap();
        write_png(&svg.data, &dir.join(format!("{name}.png")), 6.0);
        if name == "rich-a3" {
            write_png_region(&svg.data, &dir.join("rich-a3-detail.png"), 14.0, (125.0, 40.0, 140.0, 125.0));
        }
        let pdf = plan_pdf(&state.project, &state.derived, &opts).unwrap();
        std::fs::write(dir.join(format!("{name}.pdf")), &pdf.data).unwrap();
        let dxf = plan_dxf(&state.project, &state.derived, &opts).unwrap();
        std::fs::write(dir.join(format!("{name}.dxf")), &dxf.data).unwrap();
    }
    // Forced scale that overflows the sheet.
    let s = fixture();
    let mut opts = options(PaperSize::A4, Orientation::Landscape);
    opts.scale_denominator = Some(25);
    let svg = plan_svg(&s.project, &s.derived, &opts).unwrap();
    std::fs::write(dir.join("bungalow-a4-overflow.svg"), &svg.data).unwrap();
    write_png(&svg.data, &dir.join("bungalow-a4-overflow.png"), 6.0);
}

#[test]
fn corner_patches_only_where_outlines_leave_a_gap() {
    // Two walls meeting at a right angle, one door in the first.
    let mut s = rich();
    let lvl = level_id(&s);
    let wall = |id: &str, a: (f64, f64), b: (f64, f64)| {
        Element::Wall(Wall {
            id: id.into(),
            level_id: lvl.clone(),
            start: Point { x: a.0, y: a.1 },
            end: Point { x: b.0, y: b.1 },
            thickness_mm: 200.0,
            height_mm: None,
            material_id: None,
        })
    };
    s.project.elements = vec![wall("a", (0.0, 0.0), (4000.0, 0.0)), wall("b", (4000.0, 0.0), (4000.0, 3000.0))];
    let opts = options(PaperSize::A3, Orientation::Landscape);
    let count = |derived: &Derived| {
        let drawing = load_dxf(&plan_dxf(&s.project, derived, &opts).unwrap().data);
        count_by_layer(&drawing)
            .get(&("A-WALL".to_string(), "POLYLINE"))
            .copied()
            .unwrap_or(0)
    };
    // Butt ended rectangles leave the outer corner open: one patch.
    assert_eq!(count(&Derived::default()), 3);
    // Mitred outlines cover the corner: no patch.
    let geo = |id: &str, pts: &[(f64, f64)]| WallGeometry {
        wall_id: id.into(),
        length_mm: 0.0,
        outline: pts.iter().map(|p| Point { x: p.0, y: p.1 }).collect(),
        exterior: true,
        joined_at_start: vec![],
        joined_at_end: vec![],
    };
    let mitred = Derived {
        walls: vec![
            geo("a", &[(0.0, -100.0), (4100.0, -100.0), (3900.0, 100.0), (0.0, 100.0)]),
            geo("b", &[(4100.0, -100.0), (4100.0, 3000.0), (3900.0, 3000.0), (3900.0, 100.0)]),
        ],
        ..Derived::default()
    };
    assert_eq!(count(&mitred), 2);
}
