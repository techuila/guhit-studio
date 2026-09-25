//! Service sheets: lighting, power, plumbing, plumbing isometric, aircon,
//! and the review page. Test data: the bungalow with a T&B, the sixteen
//! plumbing runs and a full set of services (`common::services`).

mod common;

use common::*;
use guhit_export::{plan_dxf, plan_pdf, plan_svg};
use guhit_model::*;
use svg2pdf::usvg;

fn parse_svg(svg: &str) -> usvg::Tree {
    let mut db = usvg::fontdb::Database::new();
    db.load_system_fonts();
    let options = usvg::Options {
        fontdb: std::sync::Arc::new(db),
        ..usvg::Options::default()
    };
    usvg::Tree::from_str(svg, &options).expect("usvg parses the sheet")
}

fn write_png(svg: &str, path: &std::path::Path, px_per_mm: f32, region: Option<(f32, f32, f32, f32)>) {
    let tree = parse_svg(svg);
    let scale = px_per_mm * 25.4 / 96.0;
    let (x, y, w, h) = region.unwrap_or((0.0, 0.0, tree.size().width() * 25.4 / 96.0, tree.size().height() * 25.4 / 96.0));
    let mut pixmap = resvg::tiny_skia::Pixmap::new((w * px_per_mm).ceil() as u32, (h * px_per_mm).ceil() as u32).expect("pixmap");
    pixmap.fill(resvg::tiny_skia::Color::WHITE);
    let transform = resvg::tiny_skia::Transform::from_scale(scale, scale).post_translate(-x * px_per_mm, -y * px_per_mm);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.save_png(path).expect("write png");
}

fn sheet_opts(sheet: SheetKind, paper: PaperSize, orientation: Orientation) -> PlanExportOptions {
    let mut o = options(paper, orientation);
    o.sheet = sheet;
    o
}

const SHEETS: [(SheetKind, &str); 4] = [
    (SheetKind::Lighting, "lighting"),
    (SheetKind::Power, "power"),
    (SheetKind::Plumbing, "plumbing"),
    (SheetKind::Aircon, "aircon"),
];

/// Files to look at: target/export-samples/sheet-*.
#[test]
fn write_sheet_samples() {
    let dir = samples_dir();
    let s = services();
    for (sheet, name) in SHEETS {
        for (paper, orientation, tag) in [
            (PaperSize::A3, Orientation::Landscape, "a3"),
            (PaperSize::A4, Orientation::Portrait, "a4-portrait"),
        ] {
            let opts = sheet_opts(sheet, paper, orientation);
            let svg = plan_svg(&s.project, &s.derived, &opts).unwrap();
            let file = format!("sheet-{name}-{tag}");
            std::fs::write(dir.join(format!("{file}.svg")), &svg.data).unwrap();
            write_png(&svg.data, &dir.join(format!("{file}.png")), 6.0, None);
            let pdf = plan_pdf(&s.project, &s.derived, &opts).unwrap();
            std::fs::write(dir.join(format!("{file}.pdf")), &pdf.data).unwrap();
            let dxf = plan_dxf(&s.project, &s.derived, &opts).unwrap();
            std::fs::write(dir.join(format!("{file}.dxf")), &dxf.data).unwrap();
            if tag == "a3" {
                write_png(&svg.data, &dir.join(format!("{file}-detail.png")), 14.0, Some((25.0, 25.0, 170.0, 125.0)));
                write_png(&svg.data, &dir.join(format!("{file}-east.png")), 16.0, Some((175.0, 90.0, 80.0, 60.0)));
                write_png(&svg.data, &dir.join(format!("{file}-west.png")), 20.0, Some((55.0, 140.0, 70.0, 45.0)));
                write_png(&svg.data, &dir.join(format!("{file}-legend.png")), 12.0, Some((290.0, 10.0, 120.0, 60.0)));
            }
        }
    }
}

/// Files to look at: target/export-samples/sheet-isometric-*.
#[test]
fn write_isometric_samples() {
    let dir = samples_dir();
    for (state, name) in [(services(), "isometric"), (services_two_levels(), "isometric-two-levels")] {
        for (paper, orientation, tag) in [
            (PaperSize::A3, Orientation::Landscape, "a3"),
            (PaperSize::A4, Orientation::Portrait, "a4-portrait"),
            (PaperSize::A4, Orientation::Landscape, "a4"),
            (PaperSize::A1, Orientation::Landscape, "a1"),
        ] {
            let opts = sheet_opts(SheetKind::PlumbingIsometric, paper, orientation);
            let svg = plan_svg(&state.project, &state.derived, &opts).unwrap();
            let file = format!("sheet-{name}-{tag}");
            std::fs::write(dir.join(format!("{file}.svg")), &svg.data).unwrap();
            write_png(&svg.data, &dir.join(format!("{file}.png")), 6.0, None);
            let pdf = plan_pdf(&state.project, &state.derived, &opts).unwrap();
            std::fs::write(dir.join(format!("{file}.pdf")), &pdf.data).unwrap();
            let dxf = plan_dxf(&state.project, &state.derived, &opts).unwrap();
            std::fs::write(dir.join(format!("{file}.dxf")), &dxf.data).unwrap();
            if tag == "a4" {
                write_png(&svg.data, &dir.join(format!("{file}-zoom.png")), 16.0, Some((10.0, 10.0, 180.0, 130.0)));
            }
            if tag == "a3" {
                write_png(&svg.data, &dir.join(format!("{file}-water.png")), 14.0, Some((25.0, 45.0, 120.0, 150.0)));
                write_png(&svg.data, &dir.join(format!("{file}-sanitary.png")), 14.0, Some((160.0, 65.0, 140.0, 110.0)));
                write_png(&svg.data, &dir.join(format!("{file}-band.png")), 10.0, Some((280.0, 180.0, 140.0, 117.0)));
            }
        }
    }
}

/// Files to look at: target/export-samples/services*.ifc and services-3d.dxf.
#[test]
fn write_services_model_samples() {
    let dir = samples_dir();
    let s = services();
    std::fs::write(dir.join("services.ifc"), guhit_export::model_ifc(&s.project, &s.derived).unwrap()).unwrap();
    std::fs::write(dir.join("services-3d.dxf"), guhit_export::model_dxf3d(&s.project, &s.derived).unwrap()).unwrap();
    let two = services_two_levels();
    std::fs::write(dir.join("services-two-levels.ifc"), guhit_export::model_ifc(&two.project, &two.derived).unwrap()).unwrap();
}

/// Files to look at: target/export-samples/review-*.
#[test]
fn write_review_samples() {
    let dir = samples_dir();
    let s = services_with_review();
    for (sheet, name) in [(SheetKind::Plan, "plan"), (SheetKind::Lighting, "lighting")] {
        let mut opts = sheet_opts(sheet, PaperSize::A3, Orientation::Landscape);
        opts.review_page = true;
        let pdf = plan_pdf(&s.project, &s.derived, &opts).unwrap();
        std::fs::write(dir.join(format!("review-{name}.pdf")), &pdf.data).unwrap();
    }
    let opts = sheet_opts(SheetKind::Plan, PaperSize::A3, Orientation::Landscape);
    for (i, page) in guhit_export::review::pages(&s.project, &s.derived, &opts).iter().enumerate() {
        std::fs::write(dir.join(format!("review-page-{}.svg", i + 1)), page).unwrap();
        write_png(page, &dir.join(format!("review-page-{}.png", i + 1)), 6.0, None);
    }
}

// ------------------------------------------------------------------ helpers

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

fn svg_of(s: &DocState, sheet: SheetKind, paper: PaperSize, orientation: Orientation) -> String {
    plan_svg(&s.project, &s.derived, &sheet_opts(sheet, paper, orientation)).unwrap().data
}

/// The markup of `<g id="..."` up to its matching `</g>`, nested groups
/// included.
fn group<'a>(svg: &'a str, id: &str) -> &'a str {
    let start = svg
        .find(&format!("<g id=\"{id}\""))
        .unwrap_or_else(|| panic!("group {id} missing"));
    let mut depth = 0usize;
    let mut i = start;
    while i < svg.len() {
        if svg[i..].starts_with("<g ") || svg[i..].starts_with("<g>") {
            depth += 1;
        } else if svg[i..].starts_with("</g>") {
            depth -= 1;
            if depth == 0 {
                return &svg[start..i + 4];
            }
        }
        i += svg[i..].chars().next().unwrap().len_utf8();
    }
    panic!("group {id} does not close");
}

fn has_group(svg: &str, id: &str) -> bool {
    svg.contains(&format!("<g id=\"{id}\""))
}

/// Text contents of a markup, in order.
fn texts(markup: &str) -> Vec<String> {
    markup
        .split("<text ")
        .skip(1)
        .filter_map(|t| {
            let body = &t[t.find('>')? + 1..];
            Some(body[..body.find("</text>")?].replace("&amp;", "&"))
        })
        .collect()
}

/// Legend rows as (description, qty), read from the legend group: each
/// description text is followed by its quantity text.
fn legend_rows(svg: &str) -> Vec<(String, String)> {
    let g = group(svg, "legend");
    let mut rows = Vec::new();
    let mut last: Option<String> = None;
    for line in g.lines().filter(|l| l.starts_with("<text ")) {
        let text = texts(line).pop().unwrap_or_default();
        if line.contains("text-anchor=\"end\"") {
            if let Some(d) = last.take() {
                rows.push((d, text));
            }
        } else if line.contains("text-anchor=\"start\"") && !line.contains("font-weight=\"bold\"") {
            last = Some(text);
        }
    }
    rows
}

fn qty(rows: &[(String, String)], what: &str) -> String {
    rows.iter()
        .find(|(d, _)| d == what)
        .unwrap_or_else(|| panic!("legend row {what} missing in {rows:?}"))
        .1
        .clone()
}

struct Box2 {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Box2 {
    fn overlaps(&self, o: &Box2, tol: f64) -> bool {
        self.x0 < o.x1 - tol && o.x0 < self.x1 - tol && self.y0 < o.y1 - tol && o.y0 < self.y1 - tol
    }
}

fn node_box(tree: &usvg::Tree, id: &str, px: f64) -> Option<Box2> {
    let r = tree.node_by_id(id)?.abs_stroke_bounding_box();
    Some(Box2 {
        x0: r.left() as f64 / px,
        y0: r.top() as f64 / px,
        x1: r.right() as f64 / px,
        y1: r.bottom() as f64 / px,
    })
}

fn text_boxes(group: &usvg::Group, px: f64, out: &mut Vec<(String, Box2)>) {
    for node in group.children() {
        match node {
            usvg::Node::Text(t) => {
                let r = t.abs_bounding_box();
                let text = t.chunks().iter().map(|c| c.text()).collect::<String>();
                out.push((text, Box2 {
                    x0: r.left() as f64 / px,
                    y0: r.top() as f64 / px,
                    x1: r.right() as f64 / px,
                    y1: r.bottom() as f64 / px,
                }));
            }
            usvg::Node::Group(g) => text_boxes(g, px, out),
            _ => {}
        }
    }
}

// ---------------------------------------------------------- service sheets

#[test]
fn every_service_sheet_keeps_the_frame_and_the_scale_logic() {
    let s = services();
    for (sheet, _) in SHEETS {
        for (paper, orientation) in ALL_PAPERS {
            let out = plan_svg(&s.project, &s.derived, &sheet_opts(sheet, paper, orientation)).unwrap();
            let svg = &out.data;
            let what = format!("{sheet:?} {paper:?} {orientation:?}");
            assert!(guhit_export::COMMON_SCALES.contains(&out.scale_denominator), "{what}");
            for needle in ["PROJECT", "DRAWING", "DESIGNED BY", "Guhit Studio", "GRAPHIC SCALE", ">N<"] {
                assert!(svg.contains(needle), "{what}: {needle}");
            }
            assert!(svg.contains(&format!("SCALE 1:{}", out.scale_denominator)), "{what}");
            let title = match sheet {
                SheetKind::Lighting => "GROUND FLOOR LIGHTING LAYOUT",
                SheetKind::Power => "GROUND FLOOR POWER LAYOUT",
                SheetKind::Plumbing => "GROUND FLOOR PLUMBING LAYOUT",
                _ => "GROUND FLOOR AIRCON LAYOUT",
            };
            assert!(svg.contains(&format!(">{title}<")), "{what}: title");
            parse_svg(svg);
        }
    }
    // A forced scale is used as given.
    let mut opts = sheet_opts(SheetKind::Power, PaperSize::A3, Orientation::Landscape);
    opts.scale_denominator = Some(75);
    let out = plan_svg(&s.project, &s.derived, &opts).unwrap();
    assert_eq!(out.scale_denominator, 75);
    assert!(out.data.contains("SCALE 1:75"));
}

#[test]
fn panel_drawing_band_and_title_block_never_overlap() {
    let s = services();
    let blocks = ["legend", "room-counts", "schedule-of-loads", "fixture-table", "notes", "signatory"];
    let drawn = ["architecture", "context", "fixtures", "pipes", "links", "devices", "core-holes", "room-names"];
    for (sheet, _) in SHEETS {
        for (paper, orientation) in ALL_PAPERS {
            for title_block in [true, false] {
                let mut opts = sheet_opts(sheet, paper, orientation);
                opts.title_block = title_block;
                let svg = plan_svg(&s.project, &s.derived, &opts).unwrap().data;
                let tree = parse_svg(&svg);
                let layout = guhit_export::sheet::Layout::new(paper, orientation, title_block);
                let px = tree.size().width() as f64 / layout.width;
                let what = format!("{sheet:?} {paper:?} {orientation:?} title block {title_block}");
                let band_top = layout.height - layout.margin - layout.title_h - layout.band_h;
                let panel: Vec<(&str, Box2)> = blocks.iter().filter_map(|id| node_box(&tree, id, px).map(|b| (*id, b))).collect();
                assert!(panel.len() >= 3, "{what}: {} blocks", panel.len());
                for (i, (a, ba)) in panel.iter().enumerate() {
                    assert!(ba.y1 < band_top, "{what}: {a} reaches the band ({} > {band_top})", ba.y1);
                    assert!(ba.x0 > layout.margin && ba.x1 < layout.width - layout.margin, "{what}: {a} leaves the border");
                    for (b, bb) in &panel[i + 1..] {
                        assert!(!ba.overlaps(bb, 0.01), "{what}: {a} overlaps {b}");
                    }
                }
                for id in drawn {
                    let Some(d) = node_box(&tree, id, px) else { continue };
                    assert!(d.y1 < band_top, "{what}: {id} reaches the band");
                    for (b, bb) in &panel {
                        assert!(!d.overlaps(bb, 0.01), "{what}: {id} runs into {b}");
                    }
                }
                // The band itself: title, scale bar and north arrow as on the plan.
                let title = node_box(&tree, "sheet", px).expect("frame");
                assert!(title.y1 <= layout.height - layout.margin + 0.5, "{what}");
            }
        }
    }
}

#[test]
fn lighting_sheet_draws_fixtures_switches_s3_and_links() {
    let s = services();
    let svg = svg_of(&s, SheetKind::Lighting, PaperSize::A3, Orientation::Landscape);
    let devices = group(&svg, "devices");
    let count = |key: &str| devices.matches(&format!("data-key=\"{key}\"")).count();
    assert_eq!(count("light-ceiling"), 3);
    assert_eq!(count("light-pendant"), 1);
    assert_eq!(count("light-downlight"), 2);
    assert_eq!(count("light-wall"), 1);
    assert_eq!(count("light-outdoor"), 1);
    assert_eq!(count("light-floor-lamp"), 1);
    assert_eq!(count("switch-1") + count("switch-2"), 6);
    // Two switches share the bedroom light: both read S3, the rest S.
    assert_eq!(devices.matches("data-label=\"S3\"").count(), 2);
    assert_eq!(devices.matches("data-label=\"S\"").count(), 4);
    // One dashed arc per switch and light it controls.
    let links = group(&svg, "links");
    assert!(links.contains("stroke-dasharray"));
    assert_eq!(links.matches("<path").count(), 9);
    // No outlets, runs or aircon on the lighting sheet.
    assert!(!devices.contains("outlet-") && !devices.contains("aircon-"));
    assert!(!has_group(&svg, "pipes"));
    // Legend counts the symbols drawn on this level.
    let rows = legend_rows(&svg);
    assert_eq!(qty(&rows, "Ceiling light"), "3");
    assert_eq!(qty(&rows, "Three-way switch, one gang"), "1");
    assert_eq!(qty(&rows, "Three-way switch, two gang"), "1");
    assert_eq!(qty(&rows, "Switch, one gang"), "3");
    assert_eq!(qty(&rows, "Switch to the lights it controls"), "9");
    // Counts per room, from the schedule rule: lighting outlets and switches.
    let counts = texts(group(&svg, "room-counts"));
    let at = |name: &str| counts.iter().position(|t| t == name).unwrap_or_else(|| panic!("{name} in {counts:?}"));
    assert_eq!(&counts[at("Living / Dining") + 1..at("Living / Dining") + 3], ["3", "3"]);
    assert_eq!(&counts[at("Bedroom") + 1..at("Bedroom") + 3], ["2", "3"]);
    assert_eq!(&counts[at("T&B") + 1..at("T&B") + 3], ["2", "-"]);
    assert_eq!(&counts[at("TOTAL") + 1..at("TOTAL") + 3], ["8", "6"]);
    assert!(svg.contains(">PROFESSIONAL ELECTRICAL ENGINEER<"));
}

#[test]
fn counts_per_room_come_from_the_schedule() {
    let mut s = services_two_levels();
    let svg = svg_of(&s, SheetKind::Lighting, PaperSize::A3, Orientation::Landscape);
    let counts = texts(group(&svg, "room-counts"));
    let i = counts.iter().position(|t| t == "Living / Dining").unwrap();
    assert_eq!(counts[i + 1], "3");
    // The table prints what the engine's schedule says, not its own count.
    for row in &mut s.derived.schedule {
        if row.catalog_key == "light-ceiling" && row.room_id.is_some() && row.count == 2 {
            row.count = 12;
        }
    }
    let svg = svg_of(&s, SheetKind::Lighting, PaperSize::A3, Orientation::Landscape);
    let counts = texts(group(&svg, "room-counts"));
    let i = counts.iter().position(|t| t == "Living / Dining").unwrap();
    assert_eq!(counts[i + 1], "13");
}

#[test]
fn power_sheet_has_outlets_conduit_and_a_blank_schedule_of_loads() {
    let s = services();
    let svg = svg_of(&s, SheetKind::Power, PaperSize::A3, Orientation::Landscape);
    let devices = group(&svg, "devices");
    for (label, n) in [("SPO", 1), ("ACO", 1), ("WP", 1), ("PB", 2), ("SD", 1), ("CH", 1)] {
        assert_eq!(devices.matches(&format!("data-label=\"{label}\"")).count(), n, "{label}");
    }
    assert!(has_group(&svg, "pipes-conduit"));
    assert!(!svg.contains("pipes-cold-water") && !svg.contains("pipes-refrigerant"));
    let rows = legend_rows(&svg);
    assert_eq!(qty(&rows, "Convenience outlet, duplex"), "5");
    assert_eq!(qty(&rows, "Special purpose outlet"), "1");
    assert!(qty(&rows, "Conduit").ends_with(" m"));
    let loads = group(&svg, "schedule-of-loads");
    let t = texts(loads);
    for head in ["CKT", "DESCRIPTION", "QTY", "RATING", "(VA)", "WIRE", "BREAKER", "(AT)"] {
        assert!(t.iter().any(|x| x == head), "{head} in {t:?}");
    }
    let circuits: Vec<&String> = t.iter().filter(|x| ["AC1", "C1", "C2", "C3", "L1", "L2"].contains(&x.as_str())).collect();
    assert_eq!(circuits, ["AC1", "C1", "C2", "C3", "L1", "L2"]);
    assert!(t.iter().any(|x| x == "4 lighting outlets, 1 smoke detector"));
    assert!(t.iter().any(|x| x.contains("left blank for the PEE")));
    // Rating, wire and breaker cells hold nothing: every row has exactly
    // three texts (circuit, description, quantity).
    let rows_text = t.iter().skip_while(|x| x.as_str() != "(AT)").skip(1).take_while(|x| !x.starts_with("Rating")).count();
    assert_eq!(rows_text, 6 * 3);
}

#[test]
fn plumbing_sheet_draws_water_drain_vent_storm_and_counts_fixtures_per_level() {
    let s = services_two_levels();
    let svg = svg_of(&s, SheetKind::Plumbing, PaperSize::A3, Orientation::Landscape);
    for id in ["pipes-cold-water", "pipes-hot-water", "pipes-drainage", "pipes-vent", "pipes-storm"] {
        assert!(has_group(&svg, id), "{id}");
    }
    for id in ["pipes-conduit", "pipes-refrigerant", "pipes-condensate", "devices", "links"] {
        assert!(!has_group(&svg, id), "{id}");
    }
    let t = texts(group(&svg, "fixture-table"));
    for head in ["TAG", "FIXTURE", "GROUND", "SECOND", "TOTAL"] {
        assert!(t.iter().any(|x| x == head), "{head} in {t:?}");
    }
    let wc = t.iter().position(|x| x == "Water closet").unwrap();
    assert_eq!(&t[wc - 1..wc + 4], ["WC", "Water closet", "1", "1", "2"]);
    // From the schedule: three WCs upstairs in the schedule reads 3.
    let mut s = s;
    for row in &mut s.derived.schedule {
        if row.catalog_key == "wc" && row.level_id == "level-2" {
            row.count = 3;
        }
    }
    let svg = svg_of(&s, SheetKind::Plumbing, PaperSize::A3, Orientation::Landscape);
    let t = texts(group(&svg, "fixture-table"));
    let wc = t.iter().position(|x| x == "Water closet").unwrap();
    assert_eq!(&t[wc + 1..wc + 4], ["1", "3", "4"]);
    assert!(svg.contains(">MASTER PLUMBER<"));
}

#[test]
fn aircon_sheet_draws_units_line_sets_condensate_and_core_holes() {
    let s = services();
    let svg = svg_of(&s, SheetKind::Aircon, PaperSize::A3, Orientation::Landscape);
    let devices = group(&svg, "devices");
    for (label, n) in [("ACU", 1), ("CU", 1), ("AC", 1)] {
        assert_eq!(devices.matches(&format!("data-label=\"{label}\"")).count(), n, "{label}");
    }
    assert!(has_group(&svg, "pipes-refrigerant") && has_group(&svg, "pipes-condensate"));
    assert!(!has_group(&svg, "pipes-conduit") && !has_group(&svg, "pipes-drainage"));
    let holes = group(&svg, "core-holes");
    assert_eq!(texts(holes), ["CH 65", "CH 65"]);
    let rows = legend_rows(&svg);
    assert_eq!(qty(&rows, "Core hole through a wall"), "2");
    assert_eq!(qty(&rows, "Split aircon, indoor unit"), "1");
    // The window unit stands in the wall; it counts in the room in front.
    let t = texts(group(&svg, "room-counts"));
    let living = t.iter().position(|x| x == "Living / Dining").unwrap();
    assert_eq!(t[living + 1..living + 4].iter().filter(|x| x.as_str() == "1").count(), 1);
    assert!(svg.contains(">PROFESSIONAL MECHANICAL ENGINEER<"));
}

#[test]
fn hidden_layers_and_empty_levels() {
    let mut s = services();
    set_layer(&mut s, LayerKey::Electrical, false);
    let svg = svg_of(&s, SheetKind::Lighting, PaperSize::A3, Orientation::Landscape);
    assert!(!has_group(&svg, "devices") && !has_group(&svg, "links"));
    assert!(svg.contains("No lights or switches on this level."));
    // The plan sheet follows the same layer: lighting and electrical outlines
    // go, furniture, fixtures and aircon units stay.
    let outlines = |s: &DocState| group(&svg_of(s, SheetKind::Plan, PaperSize::A3, Orientation::Landscape), "assets").matches("<polygon").count();
    let shown = services();
    assert_eq!(outlines(&shown) - outlines(&s), 15 + 12, "lighting and power objects");
    // A level with nothing on it is empty on every sheet.
    let mut s = services();
    s.project.levels.push(Level { id: "empty".into(), name: "Roof deck".into(), elevation_mm: 6000.0, height_mm: 3000.0 });
    for (sheet, _) in SHEETS {
        let mut opts = sheet_opts(sheet, PaperSize::A3, Orientation::Landscape);
        opts.level_id = Some("empty".into());
        assert!(matches!(plan_svg(&s.project, &s.derived, &opts), Err(guhit_export::ExportError::Empty(_))), "{sheet:?}");
        assert!(matches!(plan_dxf(&s.project, &s.derived, &opts), Err(guhit_export::ExportError::Empty(_))), "{sheet:?}");
    }
}

#[test]
fn sheets_carry_no_approval_wording() {
    let s = services_with_review();
    let mut all = String::new();
    for (sheet, _) in SHEETS {
        all.push_str(&svg_of(&s, sheet, PaperSize::A3, Orientation::Landscape));
    }
    all.push_str(&svg_of(&s, SheetKind::PlumbingIsometric, PaperSize::A3, Orientation::Landscape));
    let lower = all.to_lowercase();
    for word in ["approved", "compliant", "complies", "certified", "passed"] {
        assert!(!lower.contains(word), "{word}");
    }
    // The signing fields are blank: captions only.
    for sheet in [SheetKind::Lighting, SheetKind::Plumbing, SheetKind::Aircon] {
        let svg = svg_of(&s, sheet, PaperSize::A3, Orientation::Landscape);
        let t = texts(group(&svg, "signatory"));
        assert_eq!(&t[1..], ["NAME", "PRC NO.", "SIGNATURE AND SEAL"], "{sheet:?}");
    }
}

// --------------------------------------------------------------- isometric

#[test]
fn isometric_sheet_draws_both_diagrams_not_to_scale() {
    let s = services();
    let out = plan_svg(&s.project, &s.derived, &sheet_opts(SheetKind::PlumbingIsometric, PaperSize::A3, Orientation::Landscape)).unwrap();
    let svg = &out.data;
    assert!(guhit_export::COMMON_SCALES.contains(&out.scale_denominator));
    assert_eq!(svg.matches(">NOT TO SCALE<").count(), 3, "band and both diagrams");
    assert!(svg.contains(">NTS<") && !svg.contains("GRAPHIC SCALE"));
    let water = group(svg, "iso-water");
    let sanitary = group(svg, "iso-sanitary");
    assert!(water.contains("data-system=\"cold-water\"") && water.contains("data-system=\"hot-water\""));
    assert!(sanitary.contains("data-system=\"drainage\"") && sanitary.contains("data-system=\"vent\""));
    assert!(!water.contains("data-system=\"drainage\"") && !sanitary.contains("data-system=\"cold-water\""));
    // Size and material on every run.
    let wt = texts(water);
    let st = texts(sanitary);
    let sizes = |t: &[String], s: &str| t.iter().filter(|x| x.as_str() == s).count();
    assert_eq!(sizes(&wt, "20 PPR") + sizes(&wt, "25 PE"), 9, "{wt:?}");
    assert_eq!(sizes(&st, "50 uPVC") + sizes(&st, "100 uPVC"), 7, "{st:?}");
    // Riser tags from 500 mm, fixture tags at the fixtures.
    for tag in ["CWR-1", "CWR-2", "CWR-3", "HWR-1", "HWR-2", "KS", "LAV", "WC", "SH", "WH", "WM"] {
        assert!(wt.iter().any(|x| x == tag), "water {tag} in {wt:?}");
    }
    for tag in ["WS-1", "WS-2", "VS-1", "KS", "LAV", "WC", "SH", "ST"] {
        assert!(st.iter().any(|x| x == tag), "sanitary {tag} in {st:?}");
    }
    // Flow arrows on the drains only.
    assert!(sanitary.contains("class=\"flow-arrows\"") && !water.contains("class=\"flow-arrows\""));
    // Legend box and the blank Master Plumber block.
    assert!(texts(group(svg, "legend")).iter().any(|x| x == "Runs cross: the rear one is broken"));
    assert_eq!(&texts(group(svg, "signatory"))[..], ["MASTER PLUMBER", "NAME", "PRC NO.", "SIGNATURE AND SEAL"]);
}

#[test]
fn isometric_breaks_the_rear_run_at_crossings() {
    use guhit_export::iso::{build, Which};
    let s = services();
    let d = build(&s.project, Which::Water, 50.0);
    assert!(d.crossings > 0, "the hot water runs cross in this view");
    // A broken run draws as more polylines than it has runs.
    let runs = guhit_export::iso::runs(&s.project, Which::Water).len();
    assert!(d.strokes.len() > runs, "{} strokes for {runs} runs", d.strokes.len());
    // A run passing in front of another breaks the rear one; a run that
    // starts on another (a tee) breaks nothing.
    let mut p = fixture();
    p.project.elements.clear();
    let lvl = level_id(&p);
    let run = |id: &str, pts: &[(f64, f64, f64)]| {
        Element::Pipe(Pipe {
            id: id.into(),
            level_id: lvl.clone(),
            system: PipeSystem::ColdWater,
            material: PipeMaterial::Ppr,
            diameter_mm: 20.0,
            points: pts.iter().map(|q| Vec3 { x: q.0, y: q.1, z: q.2 }).collect(),
            name: id.into(),
        })
    };
    // A along x at the floor; B along y, 500 mm up: they cross in the view
    // at A (1500, 0, 0) and B (1000, -500, 500). A is further back.
    p.project.elements = vec![
        run("a", &[(0.0, 0.0, 0.0), (2000.0, 0.0, 0.0)]),
        run("b", &[(1000.0, -1000.0, 500.0), (1000.0, 1000.0, 500.0)]),
        run("c", &[(400.0, 0.0, 0.0), (400.0, 800.0, 0.0)]),
    ];
    let d = build(&p.project, Which::Water, 50.0);
    assert_eq!(d.crossings, 1);
    assert_eq!(d.strokes.len(), 4, "A in two pieces, B and C whole");
    // A's pieces lie on the line through the origin at 30 degrees.
    let on_a = |p: &guhit_export::geom::V| (p.x * 0.5 - p.y * 0.866_025_403_784_438_6).abs() < 1e-6;
    let a_pieces = d.strokes.iter().filter(|s| s.pts.iter().all(on_a)).count();
    assert_eq!(a_pieces, 2, "the rear run A is the broken one");
}

#[test]
fn isometric_labels_never_overlap() {
    for (state, name) in [(services(), "one level"), (services_two_levels(), "two levels")] {
        for (paper, orientation) in ALL_PAPERS {
            let svg = svg_of(&state, SheetKind::PlumbingIsometric, paper, orientation);
            let tree = parse_svg(&svg);
            let layout = guhit_export::sheet::Layout::new(paper, orientation, true);
            let px = tree.size().width() as f64 / layout.width;
            for id in ["iso-water", "iso-sanitary"] {
                let usvg::Node::Group(g) = tree.node_by_id(id).expect("diagram") else { panic!("{id}") };
                let mut boxes = Vec::new();
                text_boxes(g, px, &mut boxes);
                for i in 0..boxes.len() {
                    for j in i + 1..boxes.len() {
                        let (p, q) = (&boxes[i].1, &boxes[j].1);
                        assert!(
                            !p.overlaps(q, 0.05),
                            "{name} {paper:?} {orientation:?} {id}: {} ({:.2},{:.2})-({:.2},{:.2}) overlaps {} ({:.2},{:.2})-({:.2},{:.2})",
                            boxes[i].0, p.x0, p.y0, p.x1, p.y1,
                            boxes[j].0, q.x0, q.y0, q.x1, q.y1
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn isometric_needs_runs() {
    let s = fixture();
    let opts = sheet_opts(SheetKind::PlumbingIsometric, PaperSize::A3, Orientation::Landscape);
    assert!(matches!(plan_svg(&s.project, &s.derived, &opts), Err(guhit_export::ExportError::Empty(_))));
    assert!(matches!(plan_dxf(&s.project, &s.derived, &opts), Err(guhit_export::ExportError::Empty(_))));
}

// --------------------------------------------------------------------- DXF

fn load_dxf(text: &str) -> dxf::Drawing {
    dxf::Drawing::load(&mut text.as_bytes()).expect("dxf crate parses the output")
}

#[test]
fn sheet_dxf_writes_devices_as_blocks_with_attributes() {
    use dxf::entities::EntityType as T;
    let s = services();
    for (sheet, layers, inserts) in [
        (SheetKind::Lighting, &["E-LITE-FIXT", "E-POWR-DEVC"][..], 15),
        (SheetKind::Power, &["E-POWR-DEVC"][..], 12),
        (SheetKind::Aircon, &["M-HVAC-EQPM"][..], 3),
    ] {
        let text = plan_dxf(&s.project, &s.derived, &sheet_opts(sheet, PaperSize::A3, Orientation::Landscape)).unwrap().data;
        assert!(text.is_ascii());
        let d = load_dxf(&text);
        let mut n = 0;
        for e in d.entities() {
            if let T::Insert(ins) = &e.specific {
                n += 1;
                assert!(layers.contains(&e.common.layer.as_str()), "{sheet:?}: insert on {}", e.common.layer);
                let tags: Vec<&str> = ins.attributes().map(|a| a.attribute_tag.as_str()).collect();
                assert_eq!(tags, ["TYPE", "TAG", "HEIGHT", "ROOM"], "{sheet:?}");
                assert!(ins.attributes().all(|a| a.flags & 1 == 1), "attributes are invisible");
                let block = d.blocks().find(|b| b.name == ins.name).expect("block defined");
                assert!(block.entities.iter().any(|e| !matches!(e.specific, T::AttributeDefinition(_))), "block has geometry");
            }
        }
        assert_eq!(n, inserts, "{sheet:?}");
    }
    // The values read back.
    let text = plan_dxf(&s.project, &s.derived, &sheet_opts(SheetKind::Lighting, PaperSize::A3, Orientation::Landscape)).unwrap().data;
    let d = load_dxf(&text);
    let door_switch = d
        .entities()
        .find_map(|e| match &e.specific {
            T::Insert(i) if (i.location.x - 5070.0).abs() < 1e-6 && (i.location.y - 3635.0).abs() < 1e-6 => Some(i.clone()),
            _ => None,
        })
        .expect("bedroom door switch");
    let values: Vec<(String, String)> = door_switch.attributes().map(|a| (a.attribute_tag.clone(), a.value.clone())).collect();
    assert_eq!(
        values,
        [
            ("TYPE".to_string(), "SWITCH".to_string()),
            ("TAG".into(), "L2".into()),
            ("HEIGHT".into(), "1200".into()),
            ("ROOM".into(), "Bedroom".into()),
        ]
    );
    assert_eq!(door_switch.name, "SWITCH-1_R90");
    // Links are arcs on E-LITE-CIRC, dashed by the layer.
    let arcs = d.entities().filter(|e| e.common.layer == "E-LITE-CIRC" && matches!(e.specific, T::Arc(_))).count();
    assert_eq!(arcs, 9);
    let circ = d.layers().find(|l| l.name == "E-LITE-CIRC").expect("link layer");
    assert_eq!(circ.line_type_name, "GUHIT_DASHED");
}

#[test]
fn each_sheet_dxf_holds_only_its_own_layers() {
    let s = services();
    let arch = ["A-WALL", "A-DOOR", "A-GLAZ", "A-COLS", "A-FLOR-STRS", "A-FURN", "A-AREA", "A-ANNO-DIMS", "A-ANNO-TEXT"];
    for (sheet, own) in [
        (SheetKind::Lighting, &["E-LITE-FIXT", "E-POWR-DEVC", "E-LITE-CIRC"][..]),
        (SheetKind::Power, &["E-POWR-DEVC", "E-POWR-COND"][..]),
        (SheetKind::Plumbing, &["P-DOMW-CPIP", "P-DOMW-HPIP", "P-SANR-PIPE", "P-SANR-VENT", "P-STRM-PIPE"][..]),
        (SheetKind::Aircon, &["M-HVAC-EQPM", "M-REFR-PIPE", "M-COND-PIPE"][..]),
        (SheetKind::PlumbingIsometric, &["P-DOMW-CPIP", "P-DOMW-HPIP", "P-SANR-PIPE", "P-SANR-VENT", "P-ANNO-TEXT", "P-ANNO-SYMB"][..]),
    ] {
        let d = load_dxf(&plan_dxf(&s.project, &s.derived, &sheet_opts(sheet, PaperSize::A3, Orientation::Landscape)).unwrap().data);
        let mut table: Vec<String> = d.layers().map(|l| l.name.clone()).filter(|n| n != "0").collect();
        table.sort();
        let mut want: Vec<String> = own.iter().map(|s| s.to_string()).collect();
        if sheet != SheetKind::PlumbingIsometric {
            want.extend(arch.iter().map(|s| s.to_string()));
        }
        want.sort();
        assert_eq!(table, want, "{sheet:?}");
        for e in d.entities() {
            assert!(want.contains(&e.common.layer), "{sheet:?}: entity on {}", e.common.layer);
        }
    }
}

// --------------------------------------------------------------- IFC4, 3D

#[test]
fn ifc_writes_devices_conduit_and_the_new_systems() {
    use guhit_export::ifc::{count_entities, dangling_refs};
    let s = services();
    let ifc = guhit_export::model_ifc(&s.project, &s.derived).unwrap();
    assert!(dangling_refs(&ifc).is_empty());
    let c = count_entities(&ifc);
    assert_eq!(c.outlets, 8);
    assert_eq!(c.switches, 7, "six toggle switches and the doorbell button");
    assert_eq!(c.light_fixtures, 9);
    assert_eq!(c.boards, 1);
    assert_eq!(c.sensors, 1);
    assert_eq!(c.alarms, 1);
    assert_eq!(c.unitary, 3);
    assert_eq!(c.cable_segments, 6, "two conduit runs of three straight segments");
    assert_eq!(c.pipe_systems, 8);
    for (kind, enumv) in [
        ("IFCOUTLET(", ".POWEROUTLET."),
        ("IFCSWITCHINGDEVICE(", ".TOGGLESWITCH."),
        ("IFCLIGHTFIXTURE(", ".POINTSOURCE."),
        ("IFCELECTRICDISTRIBUTIONBOARD(", ".DISTRIBUTIONBOARD."),
        ("IFCSENSOR(", ".SMOKESENSOR."),
        ("IFCUNITARYEQUIPMENT(", ".SPLITSYSTEM."),
        ("IFCUNITARYEQUIPMENT(", ".AIRCONDITIONINGUNIT."),
        ("IFCCABLECARRIERSEGMENT(", ".CONDUITSEGMENT."),
    ] {
        assert!(ifc.lines().any(|l| l.contains(kind) && l.ends_with(&format!("{enumv});"))), "{kind} {enumv}");
    }
    for (name, enumv) in [("Conduit", ".ELECTRICAL."), ("Refrigerant line set", ".REFRIGERATION."), ("Condensate drain", ".DRAINAGE."), ("Storm drain", ".STORMWATER.")] {
        assert!(
            ifc.lines().any(|l| l.contains("IFCDISTRIBUTIONSYSTEM(") && l.contains(&format!(",'{name}',")) && l.ends_with(&format!("{enumv});"))),
            "{name}"
        );
    }
    // SPOs are power outlets with their own name.
    assert!(ifc.lines().any(|l| l.contains("IFCOUTLET(") && l.contains("'Special purpose outlet'")));
    // Ids stay the same on a re-export.
    assert_eq!(ifc, guhit_export::model_ifc(&s.project, &s.derived).unwrap());
}

#[test]
fn dxf3d_puts_the_new_runs_on_their_layers() {
    let s = services();
    let d = load_dxf(&guhit_export::model_dxf3d(&s.project, &s.derived).unwrap());
    for layer in ["P-STRM-PIPE", "E-POWR-COND", "M-REFR-PIPE", "M-COND-PIPE"] {
        assert!(d.layers().any(|l| l.name == layer), "{layer} in the table");
        let faces = d.entities().filter(|e| e.common.layer == layer && matches!(e.specific, dxf::entities::EntityType::Face3D(_))).count();
        assert!(faces > 0, "{layer} tubes");
    }
}

// ------------------------------------------------------------------ review

#[test]
fn review_page_lists_open_items_by_level_then_set_aside_notes() {
    let s = services_with_review();
    let opts = sheet_opts(SheetKind::Plan, PaperSize::A3, Orientation::Landscape);
    let pages = guhit_export::review::pages(&s.project, &s.derived, &opts);
    assert_eq!(pages.len(), 1);
    let t = texts(&pages[0]);
    let at = |needle: &str| t.iter().position(|x| x.contains(needle)).unwrap_or_else(|| panic!("{needle} in {t:?}"));
    assert_eq!(t[0], "Design review (suggestions)");
    assert!(at("OPEN (4)") < at("Ground Floor (3)"));
    assert!(at("Ground Floor (3)") < at("Second Floor (1)"));
    assert!(at("Second Floor (1)") < at("SET ASIDE (2)"));
    assert!(at("Kitchen sink waste passes") < at("Second Floor (1)"));
    assert!(at("Second floor WC drain falls") > at("Second Floor (1)"));
    // Items read as the engine's message, as in the app, not as check codes.
    assert!(!t.iter().any(|x| x.contains("Pipe across opening") || x.contains("Light no switch")), "{t:?}");
    assert!(at("Note: Checked with the installer") > at("SET ASIDE (2)"));
    assert!(at("NO LONGER FOUND (1)") > at("SET ASIDE (2)"));
    assert!(at("Note: Column moved") > at("NO LONGER FOUND (1)"));
    // A finding that is gone still says what it was.
    assert!(at("Pipes through a column, on Second floor WC drain") > at("NO LONGER FOUND (1)"));
    assert!(at("Pipes through a column, on Second floor WC drain") < at("Note: Column moved"));
    // Page 2 of the PDF; SVG and DXF ignore the option.
    let mut o = opts.clone();
    o.review_page = true;
    let pdf = plan_pdf(&s.project, &s.derived, &o).unwrap().data;
    assert_eq!(pdf.windows(10).filter(|w| w == b"/Type /Pag").count(), 3, "one page tree, two pages");
    assert_eq!(plan_svg(&s.project, &s.derived, &o).unwrap().data, plan_svg(&s.project, &s.derived, &opts).unwrap().data);
    // A long list continues on more pages.
    let mut many = s.clone();
    let first = many.derived.issues[0].clone();
    for i in 0..120 {
        let mut x = first.clone();
        x.id = format!("n{i}");
        many.derived.issues.push(x);
    }
    let pages = guhit_export::review::pages(&many.project, &many.derived, &opts);
    assert!(pages.len() >= 3, "{}", pages.len());
    assert!(pages[1].contains("Design review (suggestions), continued"));
    let mut o = opts.clone();
    o.review_page = true;
    let pdf = plan_pdf(&many.project, &many.derived, &o).unwrap().data;
    assert_eq!(pdf.windows(10).filter(|w| w == b"/Type /Pag").count(), pages.len() + 2);
}

// ------------------------------------------------------------- plan sheet

#[test]
fn the_plan_legend_lists_every_system_drawn() {
    let s = services();
    for (paper, orientation) in ALL_PAPERS {
        let svg = svg_of(&s, SheetKind::Plan, paper, orientation);
        let legend = group(&svg, "pipe-legend");
        for label in ["Cold water", "Hot water", "Drainage", "Vent", "Storm drain", "Conduit", "Refrigerant line set", "Condensate drain"] {
            assert!(legend.contains(&format!(">{label}</text>")), "{paper:?} {orientation:?}: {label}");
        }
        let tree = parse_svg(&svg);
        let layout = guhit_export::sheet::Layout::new(paper, orientation, true);
        let px = tree.size().width() as f64 / layout.width;
        let lb = node_box(&tree, "pipe-legend", px).unwrap();
        let band_top = layout.height - layout.margin - layout.title_h - layout.band_h;
        assert!(lb.y0 > band_top && lb.y1 < layout.height - layout.margin - layout.title_h, "{paper:?} {orientation:?}: legend leaves the band");
    }
}
