mod common;


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

/// R2: exported at 02:48 on 4 Nov in Manila the title block read 2026-11-03,
/// the UTC date of the last change. The caller's local date wins; the
/// project's own date is only the fallback.
#[test]
fn title_block_date_is_the_callers_local_date() {
    let mut s = fixture();
    s.project.updated_at = "2026-11-03T18:48:00Z".into();
    let mut opts = options(PaperSize::A3, Orientation::Landscape);
    let svg = plan_svg(&s.project, &s.derived, &opts).unwrap().data;
    assert!(svg.contains(">2026-11-03<"), "no local date given: the project's date");
    opts.date = Some("2026-11-04".into());
    let svg = plan_svg(&s.project, &s.derived, &opts).unwrap().data;
    assert!(svg.contains(">2026-11-04<"));
    assert!(!svg.contains(">2026-11-03<"));
    // Something that is not a date falls back rather than printing junk.
    opts.date = Some("soon".into());
    let svg = plan_svg(&s.project, &s.derived, &opts).unwrap().data;
    assert!(svg.contains(">2026-11-03<"));
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
    assert!(!dxf.contains('\u{2014}') && !dxf.contains('\u{2013}'));
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

