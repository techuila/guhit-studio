//! Lane L8, interop: DXF 2013 with blocks, dimensions and hatches, and IFC4
//! with types and quantities. Writes the files the ezdxf and IfcOpenShell
//! checks read to target/export-samples/l8/.

mod common;

use std::collections::HashSet;
use std::path::PathBuf;

use common::*;
use guhit_export::dxf as gdxf;
use guhit_model::*;

fn out_dir() -> PathBuf {
    let dir = samples_dir().join("l8");
    std::fs::create_dir_all(&dir).expect("create l8 samples dir");
    dir
}

fn demo() -> DocState {
    let path = workspace_root().join("fixtures/plumbing-demo.docstate.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("read the plumbing demo")).expect("parses")
}

fn p(x: f64, y: f64) -> Point {
    Point { x, y }
}

/// `rich` with door and window types, a room in Parañaque, a dimension
/// string, a grid and a site.
fn typed() -> DocState {
    let mut s = rich();
    s.project.opening_types = vec![
        OpeningSpec {
            id: "t-d1".into(),
            mark: "D1".into(),
            opening_type: OpeningType::Door,
            style: OpeningStyle::SwingSingle,
            width_mm: 800.0,
            height_mm: 2100.0,
            sill_mm: None,
            material_id: None,
            description: "Solid wood panel door, painted".into(),
            remarks: String::new(),
        },
        OpeningSpec {
            id: "t-w1".into(),
            mark: "W1".into(),
            opening_type: OpeningType::Window,
            style: OpeningStyle::Sliding,
            width_mm: 1500.0,
            height_mm: 1200.0,
            sill_mm: Some(900.0),
            material_id: None,
            description: "Aluminum sliding window, clear glass".into(),
            remarks: String::new(),
        },
    ];
    // D1: the bedroom door (swings right), the bath door (swings right,
    // hinged at the far jamb), both in 100 mm walls, and a rear door in a
    // 150 mm wall. W1: the sliding window.
    for e in &mut s.project.elements {
        match e {
            Element::Opening(o) if ["o-bed", "o-bath"].contains(&o.id.as_str()) => {
                o.width_mm = 800.0;
                o.type_id = Some("t-d1".into());
            }
            Element::Opening(o) if o.id == "o-win1" => o.type_id = Some("t-w1".into()),
            Element::Room(r) if r.id == "r-living" => r.name = "Sala, Parañaque".into(),
            _ => {}
        }
    }
    s.project.elements.push(Element::Opening(Opening {
        id: "o-rear".into(),
        wall_id: "w-rear".into(),
        opening_type: OpeningType::Door,
        style: OpeningStyle::SwingSingle,
        offset_mm: 1500.0,
        width_mm: 800.0,
        height_mm: 2100.0,
        sill_mm: 0.0,
        flip_side: false,
        flip_hinge: true,
        material_id: None,
        type_id: Some("t-d1".into()),
    }));
    let lvl = level_id(&s);
    s.project.elements.push(Element::Dimension(Dimension {
        id: "d-string".into(),
        level_id: lvl,
        a: p(0.0, 6000.0),
        b: p(8000.0, 6000.0),
        offset_mm: 1500.0,
        text_override: None,
        through: vec![p(2500.0, 6000.0), p(5000.0, 6000.0)],
        auto: None,
    }));
    for (id, label, a, b) in [
        ("g-1", "1", p(0.0, -1500.0), p(0.0, 7500.0)),
        ("g-2", "2", p(8000.0, -1500.0), p(8000.0, 7500.0)),
        ("g-a", "A", p(-1500.0, 0.0), p(9500.0, 0.0)),
        ("g-b", "B", p(-1500.0, 6000.0), p(9500.0, 6000.0)),
    ] {
        s.project.elements.push(Element::Grid(GridLine { id: id.into(), label: label.into(), start: a, end: b }));
    }
    s.project.settings.site = Some(Site {
        city: "custom".into(),
        latitude_deg: 14.4793,
        longitude_deg: 121.0198,
        utc_offset_min: 480,
    });
    s
}

// ------------------------------------------------------------- DXF reading

/// Every (group code, value) pair of a DXF file.
fn pairs(text: &str) -> Vec<(i32, String)> {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| (c[0].trim().parse::<i32>().expect("group code"), c[1].to_string()))
        .collect()
}

/// The entities of one section as (type, pairs).
fn section(text: &str, name: &str) -> Vec<(String, Vec<(i32, String)>)> {
    let all = pairs(text);
    let start = all
        .windows(2)
        .position(|w| w[0] == (0, "SECTION".into()) && w[1] == (2, name.into()))
        .unwrap_or_else(|| panic!("no {name} section"));
    let mut out: Vec<(String, Vec<(i32, String)>)> = Vec::new();
    for (c, v) in &all[start + 2..] {
        if *c == 0 && v == "ENDSEC" {
            break;
        }
        if *c == 0 {
            out.push((v.clone(), Vec::new()));
        } else if let Some(last) = out.last_mut() {
            last.1.push((*c, v.clone()));
        }
    }
    out
}

fn get(e: &[(i32, String)], code: i32) -> Option<&str> {
    e.iter().find(|(c, _)| *c == code).map(|(_, v)| v.as_str())
}

fn load(text: &str) -> dxf::Drawing {
    dxf::Drawing::load(&mut text.as_bytes()).expect("the dxf crate reads our DXF 2013")
}

/// What every DXF 2013 file must hold, whatever it draws: the version and
/// units, unique handles, owners that exist, layers, linetypes, styles and
/// blocks that every entity names.
fn check_dxf(text: &str) -> dxf::Drawing {
    assert!(text.contains("$ACADVER\n  1\nAC1027\n"), "AC1027 header");
    assert!(text.trim_end().ends_with("EOF"));
    let all = pairs(text);
    let mut handles: HashSet<String> = HashSet::new();
    let body = all.iter().position(|p| *p == (2, "CLASSES".into())).expect("CLASSES section");
    for (c, v) in &all[body..] {
        if *c == 5 || *c == 105 {
            assert!(handles.insert(v.clone()), "handle {v} written twice");
        }
    }
    let max = handles.iter().map(|h| u64::from_str_radix(h, 16).unwrap()).max().unwrap();
    let seed = all.windows(2).find(|w| w[0] == (9, "$HANDSEED".into())).map(|w| u64::from_str_radix(&w[1].1, 16).unwrap()).unwrap();
    assert!(seed > max, "$HANDSEED {seed:X} past every handle {max:X}");
    for (c, v) in &all {
        if [330, 340, 350].contains(c) && v != "0" {
            assert!(handles.contains(v), "code {c} points at {v}, which no object has");
        }
    }
    let d = load(text);
    assert_eq!(d.header.version, dxf::enums::AcadVersion::R2013);
    assert_eq!(d.header.default_drawing_units, dxf::enums::Units::Millimeters);
    let layers: HashSet<String> = d.layers().map(|l| l.name.clone()).collect();
    let blocks: HashSet<String> = d.blocks().map(|b| b.name.clone()).collect();
    for l in d.layers() {
        assert!(doc_lineweights().contains(&(l.line_weight.raw_value() as i64)) || l.line_weight.raw_value() < 0, "{} weight", l.name);
    }
    for e in d.entities() {
        assert!(layers.contains(&e.common.layer), "entity on {} with no layer", e.common.layer);
        if let dxf::entities::EntityType::Insert(i) = &e.specific {
            assert!(blocks.contains(&i.name), "INSERT of missing block {}", i.name);
        }
    }
    d
}

fn doc_lineweights() -> Vec<i64> {
    vec![0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211]
}

fn opts() -> PlanExportOptions {
    options(PaperSize::A3, Orientation::Landscape)
}

fn plan_of(s: &DocState, level: &Level) -> String {
    let o = PlanExportOptions { level_id: Some(level.id.clone()), ..opts() };
    gdxf::plan(&s.project, &s.derived, &o).expect("the plan exports").data
}

fn count(d: &dxf::Drawing, layer: &str, pick: impl Fn(&dxf::entities::EntityType) -> bool) -> usize {
    d.entities().filter(|e| e.common.layer == layer && pick(&e.specific)).count()
}

// -------------------------------------------------------------- DXF tests

#[test]
fn every_plan_is_dxf_2013_in_millimeters() {
    for (name, s) in [("bungalow", fixture()), ("plumbing-demo", demo()), ("rich", rich()), ("typed", typed()), ("pipes", plumbing()), ("slabs", slabs())] {
        for level in &s.project.levels {
            let o = PlanExportOptions { level_id: Some(level.id.clone()), ..opts() };
            let Ok(out) = gdxf::plan(&s.project, &s.derived, &o) else { continue };
            check_dxf(&out.data);
            let slug = level.name.to_lowercase().replace(' ', "-");
            std::fs::write(out_dir().join(format!("{name}-{slug}.dxf")), &out.data).unwrap();
        }
    }
}

#[test]
fn doors_and_windows_are_inserts_of_one_block_per_type() {
    let s = typed();
    let text = plan_of(&s, &s.project.levels[0]);
    let d = check_dxf(&text);
    use dxf::entities::EntityType as T;
    let inserts: Vec<(String, dxf::entities::Insert)> = d
        .entities()
        .filter_map(|e| match &e.specific {
            T::Insert(i) => Some((e.common.layer.clone(), i.clone())),
            _ => None,
        })
        .collect();
    let doors = s.project.elements.iter().filter(|e| matches!(e, Element::Opening(o) if o.opening_type == OpeningType::Door)).count();
    let windows = s.project.elements.iter().filter(|e| matches!(e, Element::Opening(o) if o.opening_type == OpeningType::Window)).count();
    assert_eq!(inserts.iter().filter(|(l, _)| l == "A-DOOR").count(), doors);
    assert_eq!(inserts.iter().filter(|(l, _)| l == "A-GLAZ").count(), windows);
    // Nothing of a door is left loose: arcs live in the blocks.
    assert_eq!(count(&d, "A-DOOR", |t| matches!(t, T::Arc(_))), 0);
    // D1 is one block, its doors placed mirrored where they hinge or swing
    // the other way.
    // The rear wall comes first in the file, so its door names the block;
    // the same type in the 100 mm walls is a second block.
    let rear: Vec<&dxf::entities::Insert> = inserts.iter().filter(|(_, i)| i.name == "D1").map(|(_, i)| i).collect();
    assert_eq!(rear.len(), 1);
    assert_eq!((rear[0].x_scale_factor, rear[0].y_scale_factor), (-1.0, 1.0), "hinged at the far jamb");
    assert!((rear[0].rotation - 180.0).abs() < 1e-9, "the rear wall runs west");
    let d1: Vec<&dxf::entities::Insert> = inserts.iter().filter(|(_, i)| i.name == "D1_WALL100").map(|(_, i)| i).collect();
    assert_eq!(d1.len(), 2, "the bedroom and bath doors share one block");
    let scales: HashSet<(i64, i64)> = d1.iter().map(|i| (i.x_scale_factor as i64, i.y_scale_factor as i64)).collect();
    assert_eq!(scales, HashSet::from([(1, -1), (-1, -1)]), "mirrored for the swing side and the hinge");
    let d1 = [rear, d1].concat();
    assert_eq!(inserts.iter().filter(|(_, i)| i.name == "W1").count(), 1);
    for i in &d1 {
        let marks: Vec<(String, String)> = i.attributes().map(|a| (a.attribute_tag.clone(), a.value.clone())).collect();
        assert_eq!(marks[0], ("MARK".to_string(), "D1".to_string()));
        assert!(i.attributes().all(|a| a.flags & 1 == 1), "invisible");
    }
    let block = d.blocks().find(|b| b.name == "D1").expect("block D1");
    assert_eq!(block.entities.iter().filter(|e| matches!(e.specific, T::Arc(_))).count(), 1);
    // Untyped openings are named by kind, style and width.
    assert!(d.blocks().any(|b| b.name.starts_with("DOOR-")), "{:?}", d.blocks().map(|b| b.name.clone()).collect::<Vec<_>>());
    // Objects are blocks per catalog item.
    let assets = s.project.elements.iter().filter(|e| matches!(e, Element::Asset(_))).count();
    assert_eq!(inserts.iter().filter(|(l, _)| l == "A-FURN").count(), assets);
    assert!(d.blocks().any(|b| b.name == "bed-double"));
}

#[test]
fn dimensions_are_dimension_entities_with_a_style() {
    let s = typed();
    let text = plan_of(&s, &s.project.levels[0]);
    let d = check_dxf(&text);
    use dxf::entities::EntityType as T;
    let dims: Vec<dxf::entities::RotatedDimension> = d
        .entities()
        .filter_map(|e| match &e.specific {
            T::RotatedDimension(r) => Some(r.clone()),
            _ => None,
        })
        .collect();
    let plain = s.project.elements.iter().filter(|e| matches!(e, Element::Dimension(d) if d.through.is_empty())).count();
    // The string a, 2500, 5000, b is three dimensions on one line.
    assert_eq!(dims.len(), plain + 3);
    let names: HashSet<String> = d.dim_styles().map(|s| s.name.clone()).collect();
    for dim in &dims {
        assert!(names.contains(&dim.dimension_base.dimension_style_name), "{}", dim.dimension_base.dimension_style_name);
        let b = d.blocks().find(|b| b.name == dim.dimension_base.block_name).expect("the dimension's block");
        assert!(b.name.starts_with("*D"));
        assert!(b.entities.iter().filter(|e| matches!(e.specific, T::Line(_))).count() >= 5);
        assert!(b.entities.iter().any(|e| matches!(e.specific, T::Text(_))));
    }
    let style = d.dim_styles().find(|s| s.name == "GUHIT-100").expect("a style for 1:100");
    assert!((style.dimensioning_scale_factor - 100.0).abs() < 1e-9);
    // The string's segments measure 2500, 2500 and 3000 on one line.
    let mut string: Vec<f64> = dims
        .iter()
        .filter(|r| (r.dimension_base.definition_point_1.y - 7500.0).abs() < 1e-6)
        .map(|r| r.dimension_base.actual_measurement)
        .collect();
    string.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(string, vec![2500.0, 2500.0, 3000.0]);
}

#[test]
fn cut_walls_are_solid_hatches() {
    let s = fixture();
    let text = plan_of(&s, &s.project.levels[0]);
    let hatches: Vec<(String, Vec<(i32, String)>)> = section(&text, "ENTITIES").into_iter().filter(|(t, _)| t == "HATCH").collect();
    let walls = s.derived.walls.len();
    let on_walls = hatches.iter().filter(|(_, e)| get(e, 8) == Some("A-WALL")).count();
    assert!(on_walls >= walls, "{on_walls} wall hatches for {walls} walls");
    for (_, e) in &hatches {
        assert_eq!(get(e, 2), Some("SOLID"));
        assert_eq!(get(e, 70), Some("1"));
    }
    let d = check_dxf(&text);
    let wall = d.layers().find(|l| l.name == "A-WALL").unwrap();
    assert_eq!(wall.line_weight.raw_value(), 35, "walls plot with the heavy pen");
    let glaz = d.layers().find(|l| l.name == "A-GLAZ").unwrap();
    assert_eq!(glaz.line_weight.raw_value(), 25);
}

#[test]
fn text_is_utf8() {
    let s = typed();
    let text = plan_of(&s, &s.project.levels[0]);
    assert!(text.contains("Sala, Parañaque"));
    assert!(text.contains(" m²"), "areas in square meters");
    assert!(!text.contains("\\U+"));
    let d = check_dxf(&text);
    assert!(d.entities().any(|e| matches!(&e.specific, dxf::entities::EntityType::Text(t) if t.value == "Sala, Parañaque")));
}

#[test]
fn every_level_side_by_side_and_the_sheet_set() {
    let mut s = typed();
    // A second level with the same walls, so the plans file has two.
    let upper = Level { id: "lvl-2".into(), name: "Second Floor".into(), elevation_mm: 3000.0, height_mm: 3000.0 };
    s.project.levels.push(upper.clone());
    let walls: Vec<Element> = s
        .project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) => Some(Element::Wall(Wall { id: format!("{}-2", w.id), level_id: upper.id.clone(), ..w.clone() })),
            Element::Dimension(d) if d.id == "d-front" => {
                Some(Element::Dimension(Dimension { id: "d-front-2".into(), level_id: upper.id.clone(), ..d.clone() }))
            }
            _ => None,
        })
        .collect();
    s.project.elements.extend(walls);
    let all = gdxf::plans(&s.project, &s.derived, &opts()).unwrap();
    let d = check_dxf(&all.data);
    assert!(d.entities().any(|e| matches!(&e.specific, dxf::entities::EntityType::Text(t) if t.value == "SECOND FLOOR PLAN")));
    std::fs::write(out_dir().join("typed-plans.dxf"), &all.data).unwrap();

    s.project.sheet_set.sheets = vec![Sheet {
        id: "sh-a2".into(),
        number: "A-2".into(),
        title: "Floor plans".into(),
        paper: PaperSize::A1,
        orientation: Orientation::Landscape,
        viewports: vec![
            Viewport {
                id: "vp-1".into(),
                view: ViewRef::Plan { level_id: s.project.levels[0].id.clone(), sheet: SheetKind::Plan },
                scale_denominator: Some(100),
                position: None,
                title: None,
            },
            Viewport {
                id: "vp-2".into(),
                view: ViewRef::Plan { level_id: "lvl-2".into(), sheet: SheetKind::Plan },
                scale_denominator: Some(50),
                position: None,
                title: Some("UPPER FLOOR".into()),
            },
            Viewport { id: "vp-3".into(), view: ViewRef::RoofPlan, scale_denominator: None, position: None, title: None },
        ],
        revision_ids: vec![],
    }];
    // An elevation and a section from lane L3's views, on a second sheet.
    let lvl = level_id(&s);
    for (id, kind, label, a, b) in [
        ("vm-south", MarkKind::Elevation, "South", p(-2000.0, -3000.0), p(11000.0, -3000.0)),
        ("vm-a", MarkKind::Section, "A", p(3000.0, -2000.0), p(3000.0, 8000.0)),
    ] {
        s.project.elements.push(Element::ViewMark(ViewMark {
            id: id.into(),
            level_id: lvl.clone(),
            mark_kind: kind,
            label: label.into(),
            start: a,
            end: b,
            flip: false,
            depth_mm: None,
        }));
    }
    s.project.sheet_set.sheets.push(Sheet {
        id: "sh-a3".into(),
        number: "A-3".into(),
        title: "Elevation and section".into(),
        paper: PaperSize::A1,
        orientation: Orientation::Landscape,
        viewports: vec![
            Viewport { id: "vp-4".into(), view: ViewRef::Elevation { mark_id: "vm-south".into() }, scale_denominator: Some(100), position: None, title: None },
            Viewport { id: "vp-5".into(), view: ViewRef::Section { mark_id: "vm-a".into() }, scale_denominator: Some(100), position: None, title: None },
        ],
        revision_ids: vec![],
    });
    let text = gdxf::sheets(&s.project, &s.derived, &[]).unwrap();
    let d = check_dxf(&text);
    let texts: Vec<String> = d
        .entities()
        .filter_map(|e| match &e.specific {
            dxf::entities::EntityType::Text(t) => Some(t.value.clone()),
            _ => None,
        })
        .collect();
    for want in ["A-2  FLOOR PLANS", "GROUND FLOOR PLAN", "UPPER FLOOR", "SCALE 1:50", "A-3  ELEVATION AND SECTION"] {
        assert!(texts.iter().any(|t| t == want), "{want} in {texts:?}");
    }
    // The project has no roof: the roof plan is left out, named in the
    // header, with no error text in the drawing (GR2 #7).
    assert!(!texts.iter().any(|t| t.contains("nothing to export") || t == "ROOF PLAN"), "{texts:?}");
    assert!(text.contains("Not drawn here: A-2 FLOOR PLANS: ROOF PLAN"), "the header names it");
    // Two scales, two dimension styles.
    let styles: HashSet<String> = d.dim_styles().map(|s| s.name.clone()).collect();
    assert!(styles.contains("GUHIT-100") && styles.contains("GUHIT-50"), "{styles:?}");
    std::fs::write(out_dir().join("typed-sheets.dxf"), &text).unwrap();
    assert!(matches!(gdxf::sheets(&s.project, &s.derived, &["nope".into()]), Err(guhit_export::ExportError::Failed(_))));
}

#[test]
fn services_sheets_are_dxf_2013_too() {
    let s = services();
    for sheet in [SheetKind::Lighting, SheetKind::Power, SheetKind::Plumbing, SheetKind::Aircon, SheetKind::PlumbingIsometric] {
        let o = PlanExportOptions { sheet, ..opts() };
        let text = guhit_export::plan_dxf(&s.project, &s.derived, &o).unwrap().data;
        check_dxf(&text);
        std::fs::write(out_dir().join(format!("services-{sheet:?}.dxf").to_lowercase()), &text).unwrap();
    }
}

#[test]
fn the_same_project_writes_the_same_bytes() {
    let s = typed();
    assert_eq!(plan_of(&s, &s.project.levels[0]), plan_of(&s, &s.project.levels[0]));
}

// --------------------------------------- the plan DXF checks, on DXF 2013
//
// What export.rs, pipes.rs and slabs.rs checked of the R12 plan DXF (layers,
// counts, geometry, swing and hinge, corner patches, pipe layers and
// linetypes, slabs and the stair below), now with doors as blocks and
// dimensions as DIMENSION entities.

use dxf::entities::EntityType as T;

fn kind_of(t: &T) -> &'static str {
    match t {
        T::Line(_) => "LINE",
        T::LwPolyline(_) => "LWPOLYLINE",
        T::Arc(_) => "ARC",
        T::Circle(_) => "CIRCLE",
        T::Text(_) => "TEXT",
        T::Insert(_) => "INSERT",
        T::RotatedDimension(_) => "DIMENSION",
        other => panic!("unexpected entity type {other:?}"),
    }
}

fn by_layer(d: &dxf::Drawing) -> std::collections::HashMap<(String, &'static str), usize> {
    let mut m = std::collections::HashMap::new();
    for e in d.entities() {
        *m.entry((e.common.layer.clone(), kind_of(&e.specific))).or_insert(0) += 1;
    }
    m
}

fn plan2013(s: &DocState, o: &PlanExportOptions) -> dxf::Drawing {
    check_dxf(&gdxf::plan(&s.project, &s.derived, o).unwrap().data)
}

const ARCH: [&str; 9] = ["A-WALL", "A-DOOR", "A-GLAZ", "A-COLS", "A-FLOR-STRS", "A-FURN", "A-AREA", "A-ANNO-DIMS", "A-ANNO-TEXT"];

fn count_elements(p: &Project, f: impl Fn(&Element) -> bool) -> usize {
    p.elements.iter().filter(|e| f(e)).count()
}

/// The arcs a drawing shows: loose ones, and those of each inserted block.
fn shown_arcs(d: &dxf::Drawing) -> usize {
    let loose = d.entities().filter(|e| matches!(e.specific, T::Arc(_))).count();
    let inserted: usize = d
        .entities()
        .filter_map(|e| match &e.specific {
            T::Insert(i) => d.blocks().find(|b| b.name == i.name),
            _ => None,
        })
        .map(|b| b.entities.iter().filter(|e| matches!(e.specific, T::Arc(_))).count())
        .sum();
    loose + inserted
}

#[test]
fn fixture_plan_layers_and_counts() {
    let s = fixture();
    let out = gdxf::plan(&s.project, &s.derived, &opts()).unwrap();
    assert_eq!(out.scale_denominator, s.project.settings.scale_denominator);
    let d = check_dxf(&out.data);
    let names: Vec<String> = d.layers().map(|l| l.name.clone()).collect();
    for l in ARCH {
        assert!(names.iter().any(|n| n == l), "layer {l} missing in {names:?}");
    }
    let counts = by_layer(&d);
    let get = |layer: &str, kind: &'static str| counts.get(&(layer.to_string(), kind)).copied().unwrap_or(0);
    let p = &s.project;
    let doors = count_elements(p, |e| matches!(e, Element::Opening(o) if o.opening_type == OpeningType::Door));
    let windows = count_elements(p, |e| matches!(e, Element::Opening(o) if o.opening_type == OpeningType::Window));
    let walls = count_elements(p, |e| matches!(e, Element::Wall(_)));
    let rooms = count_elements(p, |e| matches!(e, Element::Room(_)));
    let dims = count_elements(p, |e| matches!(e, Element::Dimension(_)));
    let assets = count_elements(p, |e| matches!(e, Element::Asset(_)));
    assert_eq!(get("A-DOOR", "INSERT"), doors);
    assert_eq!(get("A-GLAZ", "INSERT"), windows);
    assert_eq!(shown_arcs(&d), swing_leaf_count(p), "one arc per swing leaf");
    // Walls split where openings cut them, plus up to one corner patch per
    // wall end.
    let wall_polys = get("A-WALL", "LWPOLYLINE");
    assert!(wall_polys >= walls + doors + windows && wall_polys <= walls + doors + windows + walls * 2, "{wall_polys}");
    assert_eq!(get("A-AREA", "TEXT"), rooms * 2, "name and area per room");
    assert_eq!(get("A-ANNO-DIMS", "DIMENSION"), dims);
    assert_eq!(get("A-ANNO-DIMS", "LINE"), 0, "no exploded dimensions");
    assert_eq!(get("A-FURN", "INSERT"), assets);
}

#[test]
fn plan_geometry_is_millimeters_y_up_and_closed() {
    let s = fixture();
    let d = plan2013(&s, &opts());
    let mut want = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for g in &s.derived.walls {
        for p in &g.outline {
            want = (want.0.min(p.x), want.1.min(p.y), want.2.max(p.x), want.3.max(p.y));
        }
    }
    let mut got = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for e in d.entities().filter(|e| e.common.layer == "A-WALL") {
        if let T::LwPolyline(pl) = &e.specific {
            assert!(pl.is_closed(), "wall outlines are closed");
            assert!(pl.vertices.len() >= 3);
            for v in &pl.vertices {
                got = (got.0.min(v.x), got.1.min(v.y), got.2.max(v.x), got.3.max(v.y));
            }
        }
    }
    assert!((got.0 - want.0).abs() < 0.01 && (got.1 - want.1).abs() < 0.01, "{got:?} vs {want:?}");
    assert!((got.2 - want.2).abs() < 0.01 && (got.3 - want.3).abs() < 0.01, "{got:?} vs {want:?}");
    // Door arcs in the blocks are quarter circles as wide as a leaf.
    for b in d.blocks() {
        for e in &b.entities {
            if let T::Arc(arc) = &e.specific {
                let sweep = (arc.end_angle - arc.start_angle).rem_euclid(360.0);
                assert!((sweep - 90.0).abs() < 1e-6, "sweep {sweep}");
                assert!(s.project.elements.iter().any(|el| matches!(el, Element::Opening(o)
                    if (o.width_mm - arc.radius).abs() < 1e-6 || (o.width_mm / 2.0 - arc.radius).abs() < 1e-6)));
            }
        }
    }
    for g in &s.derived.rooms {
        let near = d.entities().any(|e| match &e.specific {
            T::Text(t) if e.common.layer == "A-AREA" => {
                (t.second_alignment_point.x - g.label_point.x).abs() < 1.0 && (t.second_alignment_point.y - g.label_point.y).abs() < 600.0
            }
            _ => false,
        });
        assert!(near, "label near {:?}", g.label_point);
    }
}

/// The world arc of a door: the block's arc put through its insert.
fn world_arc(d: &dxf::Drawing) -> (f64, f64, f64, f64, f64) {
    let ins = d
        .entities()
        .find_map(|e| match &e.specific {
            T::Insert(i) if e.common.layer == "A-DOOR" => Some(i.clone()),
            _ => None,
        })
        .expect("the door");
    let arc = d
        .blocks()
        .find(|b| b.name == ins.name)
        .unwrap()
        .entities
        .iter()
        .find_map(|e| match &e.specific {
            T::Arc(a) => Some(a.clone()),
            _ => None,
        })
        .unwrap();
    let (sx, sy, rot) = (ins.x_scale_factor, ins.y_scale_factor, ins.rotation.to_radians());
    let put = |x: f64, y: f64| {
        let (x, y) = (x * sx, y * sy);
        (ins.location.x + x * rot.cos() - y * rot.sin(), ins.location.y + x * rot.sin() + y * rot.cos())
    };
    let c = put(arc.center.x, arc.center.y);
    let angle = |deg: f64| {
        let r = deg.to_radians();
        let p = put(arc.center.x + r.cos(), arc.center.y + r.sin());
        (p.1 - c.1).atan2(p.0 - c.0).to_degrees().rem_euclid(360.0)
    };
    let (a0, a1) = (angle(arc.start_angle), angle(arc.end_angle));
    let (a0, a1) = if sx * sy < 0.0 { (a1, a0) } else { (a0, a1) };
    (c.0, c.1, arc.radius, a0, a1)
}

#[test]
fn swing_side_and_hinge_follow_the_flags() {
    // One wall along +x, one door: hinge at the start side jamb, leaf on the
    // left (+y). Flipped: hinge at the far jamb, leaf on -y.
    let mut s = rich();
    let lvl = level_id(&s);
    s.derived = Derived::default();
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
                start: p(0.0, 0.0),
                end: p(4000.0, 0.0),
                thickness_mm: 200.0,
                height_mm: None,
                material_id: None,
                role: WallRole::Building,
            }),
            Element::Opening(Opening {
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
                type_id: None,
            }),
        ];
        let d = plan2013(&s, &opts());
        assert_eq!(shown_arcs(&d), 1);
        let (x, y, r, s0, s1) = world_arc(&d);
        assert!((x - cx).abs() < 1e-6 && (y - cy).abs() < 1e-6, "{flip_side} {flip_hinge}: {x} {y}");
        assert!((r - 900.0).abs() < 1e-6);
        let same = |a: f64, b: f64| (a - b).rem_euclid(360.0) < 1e-6 || (a - b).rem_euclid(360.0) > 360.0 - 1e-6;
        assert!(same(s0, a0) && same(s1, a1), "{flip_side} {flip_hinge}: {s0} {s1}");
    }
}

#[test]
fn rich_plan_covers_every_layer() {
    let mut s = rich();
    // Plans draw stairs from `Derived::stairs` (D54): derive the stair the
    // project added, keeping the rest of its hand-built derived data.
    s.derived.stairs = guhit_core::compute_derived(&s.project).stairs;
    let d = plan2013(&s, &opts());
    let counts = by_layer(&d);
    let get = |layer: &str, kind: &'static str| counts.get(&(layer.to_string(), kind)).copied().unwrap_or(0);
    assert_eq!(shown_arcs(&d), 5, "three single leaves and one double");
    assert_eq!(get("A-COLS", "LWPOLYLINE"), 1);
    assert_eq!(get("A-COLS", "CIRCLE"), 1);
    assert_eq!(get("A-FLOR-STRS", "TEXT"), 1);
    // D44: 11 risers are 10 treads, 9 lines between them; D54: the flight
    // up to the 1200 mm cut is one closed outline (a polyline), and above
    // the cut its two sides and its top are dashed lines. The arrow and the
    // break line are polylines.
    assert_eq!(get("A-FLOR-STRS", "LINE"), 9 + 3);
    assert_eq!(get("A-ANNO-TEXT", "TEXT"), 2);
    assert_eq!(get("A-ANNO-DIMS", "DIMENSION"), 3);
    assert_eq!(get("A-AREA", "TEXT"), 6);
    for l in ARCH {
        assert!(counts.keys().any(|(layer, _)| layer == l), "no entities on {l}");
    }
}

#[test]
fn corner_patches_only_where_outlines_leave_a_gap() {
    let mut s = rich();
    let lvl = level_id(&s);
    let wall = |id: &str, a: (f64, f64), b: (f64, f64)| {
        Element::Wall(Wall {
            id: id.into(),
            level_id: lvl.clone(),
            start: p(a.0, a.1),
            end: p(b.0, b.1),
            thickness_mm: 200.0,
            height_mm: None,
            material_id: None,
            role: WallRole::Building,
        })
    };
    s.project.elements = vec![wall("a", (0.0, 0.0), (4000.0, 0.0)), wall("b", (4000.0, 0.0), (4000.0, 3000.0))];
    let count = |derived: &Derived| {
        let d = check_dxf(&gdxf::plan(&s.project, derived, &opts()).unwrap().data);
        by_layer(&d).get(&("A-WALL".to_string(), "LWPOLYLINE")).copied().unwrap_or(0)
    };
    assert_eq!(count(&Derived::default()), 3, "butt ends leave the corner open: one patch");
    let geo = |id: &str, pts: &[(f64, f64)]| WallGeometry {
        wall_id: id.into(),
        length_mm: 0.0,
        outline: pts.iter().map(|q| p(q.0, q.1)).collect(),
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

#[test]
fn pipes_on_their_layers_and_linetypes() {
    let s = plumbing();
    let text = gdxf::plan(&s.project, &s.derived, &opts()).unwrap().data;
    let d = check_dxf(&text);
    let layers: std::collections::HashMap<String, &dxf::tables::Layer> = d.layers().map(|l| (l.name.clone(), l)).collect();
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
    // Patterns plot at 1:100: 2.2 mm dash, 1.1 mm gap.
    let lts: std::collections::HashMap<String, &dxf::tables::LineType> = d.line_types().map(|l| (l.name.clone(), l)).collect();
    assert_eq!(lts["GUHIT_DASHED"].dash_dot_space_lengths, vec![220.0, -110.0]);
    assert!((lts["GUHIT_DASHED"].total_pattern_length - 330.0).abs() < 1e-9);
    assert_eq!(lts["GUHIT_DASHDOT"].dash_dot_space_lengths, vec![300.0, -90.0, 0.0, -90.0]);
    // One polyline per run and one circle per riser, as the plan draws them.
    let counts = by_layer(&d);
    let level = &s.project.levels[0];
    for pp in guhit_export::pipes::plan_pipes(&s.project, level) {
        let layer = guhit_export::pipes::dxf_layer(pp.system);
        let runs: usize = guhit_export::pipes::plan_pipes(&s.project, level).iter().filter(|q| q.system == pp.system).map(|q| q.runs.len()).sum();
        let risers: usize = guhit_export::pipes::plan_pipes(&s.project, level).iter().filter(|q| q.system == pp.system).map(|q| q.risers.len()).sum();
        assert_eq!(counts.get(&(layer.to_string(), "LWPOLYLINE")).copied().unwrap_or(0), runs, "{layer}");
        assert_eq!(counts.get(&(layer.to_string(), "CIRCLE")).copied().unwrap_or(0), risers, "{layer}");
    }
    // The building is drawn exactly as without pipes.
    let base = plain();
    let plain_counts = by_layer(&plan2013(&base, &opts()));
    let arch: std::collections::HashMap<_, _> = counts.iter().filter(|((l, _), _)| !l.starts_with("P-")).map(|(k, v)| (k.clone(), *v)).collect();
    assert_eq!(arch, plain_counts);
    let service = d
        .entities()
        .find_map(|e| match &e.specific {
            T::LwPolyline(pl) if e.common.layer == "P-DOMW-CPIP" && pl.vertices.len() == 4 => {
                Some(pl.vertices.iter().map(|v| (v.x, v.y)).collect::<Vec<_>>())
            }
            _ => None,
        })
        .expect("service line");
    assert_eq!(service, vec![(600.0, -1800.0), (600.0, 5850.0), (2000.0, 5850.0), (2000.0, 6000.0)]);
    for e in d.entities() {
        if let T::Circle(c) = &e.specific {
            if e.common.layer.starts_with("P-") {
                assert!((c.radius - 110.0).abs() < 1e-9, "riser radius {}", c.radius);
                assert_eq!(e.common.line_type_name, "CONTINUOUS", "risers stay continuous");
            }
        }
    }
    assert!(d.header.minimum_drawing_extents.y <= -1800.0 - 110.0 + 1e-6);
    assert!(d.header.maximum_drawing_extents.x >= 11600.0 - 1e-6);
}

#[test]
fn slabs_rails_and_the_stair_below_on_their_layers() {
    let mut s = slabs();
    // Plans draw stairs from `Derived::stairs` (D54).
    s.derived.stairs = guhit_core::compute_derived(&s.project).stairs;
    let second = PlanExportOptions { level_id: Some(SECOND.into()), ..opts() };
    let text = gdxf::plan(&s.project, &s.derived, &second).unwrap().data;
    let d = check_dxf(&text);
    let names: Vec<String> = d.layers().map(|l| l.name.clone()).collect();
    for l in ["A-FLOR-OTLN", "A-FLOR-HRAL", "A-FLOR-STRS"] {
        assert_eq!(names.iter().filter(|n| *n == l).count(), 1, "{l} once in {names:?}");
    }
    let mut on: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for e in d.entities() {
        *on.entry(e.common.layer.clone()).or_insert(0) += 1;
    }
    assert!(on["A-FLOR-OTLN"] >= 4 + 4 + 2 + 2, "{on:?}");
    assert_eq!(on["A-FLOR-HRAL"], 7);
    let dashed = d
        .entities()
        .filter(|e| e.common.layer == "A-FLOR-STRS" && e.common.line_type_name == "GUHIT_DASHED")
        .filter(|e| matches!(e.specific, T::Line(_) | T::LwPolyline(_)))
        .count();
    assert_eq!(dashed, 10, "nine tread lines and the outline");
    assert!(text.contains("\n  1\nOPEN TO BELOW\n"));
    // A plan without slabs has none of their layers.
    let text = gdxf::plan(&fixture().project, &fixture().derived, &opts()).unwrap().data;
    for l in ["A-FLOR-OTLN", "A-FLOR-HRAL", "GUHIT_DASHED"] {
        assert!(!text.contains(l), "{l} in a plan without slabs");
    }
}

#[test]
fn long_dashes_never_reach_the_file() {
    let mut s = rich();
    s.project.name = "Casa \u{2014} Uno".into();
    for e in &mut s.project.elements {
        if let Element::Room(r) = e {
            r.name = format!("{} \u{2013} A", r.name);
        }
    }
    let text = gdxf::plan(&s.project, &s.derived, &opts()).unwrap().data;
    assert!(!text.contains('\u{2014}') && !text.contains('\u{2013}'));
    assert!(text.contains("Bedroom - A"));
}

// -------------------------------------------------------------------- IFC

fn v3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3 { x, y, z }
}

fn roof_area(id: &str, level: Option<Id>, outline: RoofOutline, shape: RoofShape) -> Element {
    Element::Roof(RoofArea {
        id: id.into(),
        level_id: level,
        outline,
        shape,
        pitch_deg: 25.0,
        ridge_axis: Axis::X,
        flip: false,
        gable_sides: vec![],
        overhang_mm: 600.0,
        thickness_mm: 150.0,
        eave_mm: None,
        material_id: None,
        fascia: None,
        gutter: None,
    })
}

/// `typed` with what the engine derives in Wave 2, written by hand: an L
/// stair with a landing, a hip roof over the house and a gable roof over
/// a lanai with its gable ends.
fn derived_wave2() -> DocState {
    let mut s = typed();
    let lvl = level_id(&s);
    let base = s.project.levels[0].height_mm;
    for e in &mut s.project.elements {
        if let Element::Stair(st) = e {
            st.shape = StairShape::L;
            st.turn = Turn::Right;
            st.riser_count = 16;
            st.first_flight_risers = Some(8);
            st.run_mm = 3500.0;
        }
    }
    let riser = base / 16.0;
    let flight1 = Flight {
        outline: vec![p(8700.0, 300.0), p(9700.0, 300.0), p(9700.0, 2050.0), p(8700.0, 2050.0)],
        nosings: (0..7)
            .map(|i| Nosing { a: p(8700.0, 300.0 + 250.0 * i as f64), b: p(9700.0, 300.0 + 250.0 * i as f64), z: riser * (i + 1) as f64 })
            .collect(),
        up: p(0.0, 1.0),
    };
    let flight2 = Flight {
        outline: vec![p(9700.0, 2050.0), p(11450.0, 2050.0), p(11450.0, 3050.0), p(9700.0, 3050.0)],
        nosings: (0..7)
            .map(|i| Nosing { a: p(9700.0 + 250.0 * i as f64, 3050.0), b: p(9700.0 + 250.0 * i as f64, 2050.0), z: riser * (i + 9) as f64 })
            .collect(),
        up: p(1.0, 0.0),
    };
    s.derived.stairs = vec![StairGeometry {
        stair_id: "s-1".into(),
        level_id: lvl.clone(),
        arrives_at: None,
        tread_mm: 250.0,
        riser_mm: riser,
        flights: vec![flight1, flight2],
        landings: vec![Landing { polygon: vec![p(8700.0, 2050.0), p(9700.0, 2050.0), p(9700.0, 3050.0), p(8700.0, 3050.0)], z: riser * 8.0 }],
        opening: vec![],
        walk_line: vec![],
    }];
    // A hip roof over the 8 x 6 m house, 600 mm eaves, 25 degrees.
    let ridge = base + 3600.0 * 25f64.to_radians().tan();
    s.project.elements.push(roof_area("r-top", None, RoofOutline::TopFootprint, RoofShape::Hip));
    let (w, e, so, n) = (-600.0, 8600.0, -600.0, 6600.0);
    let hip = vec![
        vec![v3(w, so, base), v3(e, so, base), v3(5000.0, 3000.0, ridge), v3(3000.0, 3000.0, ridge)],
        vec![v3(e, so, base), v3(e, n, base), v3(5000.0, 3000.0, ridge)],
        vec![v3(e, n, base), v3(w, n, base), v3(3000.0, 3000.0, ridge), v3(5000.0, 3000.0, ridge)],
        vec![v3(w, n, base), v3(w, so, base), v3(3000.0, 3000.0, ridge)],
    ];
    let geometry = |id: &str, planes: Vec<Vec<Vec3>>, infill: Vec<Vec<Vec3>>, plan: f64, surface: f64| RoofGeometry {
        roof_id: id.into(),
        level_id: lvl.clone(),
        planes,
        vertical_mm: 150.0,
        lines: vec![],
        infill,
        eaves: vec![],
        plan_area_m2: plan,
        surface_area_m2: surface,
    };
    let plan = 9.2 * 7.2;
    // A lanai gable east of the house: ridge along x at y = -2000.
    s.project.elements.push(roof_area(
        "r-lanai",
        Some(lvl.clone()),
        RoofOutline::Polygon { points: vec![p(0.0, -3000.0), p(4000.0, -3000.0), p(4000.0, -1000.0), p(0.0, -1000.0)] },
        RoofShape::Gable,
    ));
    let top = 2400.0 + 1000.0 * 25f64.to_radians().tan();
    let lanai = vec![
        vec![v3(0.0, -3000.0, 2400.0), v3(4000.0, -3000.0, 2400.0), v3(4000.0, -2000.0, top), v3(0.0, -2000.0, top)],
        vec![v3(4000.0, -1000.0, 2400.0), v3(0.0, -1000.0, 2400.0), v3(0.0, -2000.0, top), v3(4000.0, -2000.0, top)],
    ];
    let ends = vec![
        vec![v3(0.0, -3000.0, 2400.0), v3(0.0, -1000.0, 2400.0), v3(0.0, -2000.0, top)],
        vec![v3(4000.0, -1000.0, 2400.0), v3(4000.0, -3000.0, 2400.0), v3(4000.0, -2000.0, top)],
    ];
    s.derived.roofs = vec![
        geometry("r-top", hip, vec![], plan, plan / 25f64.to_radians().cos()),
        geometry("r-lanai", lanai, ends, 8.0, 8.0 / 25f64.to_radians().cos()),
    ];
    s
}

fn lines_of<'a>(ifc: &'a str, kw: &str) -> Vec<&'a str> {
    ifc.lines()
        .filter(|l| l.split_once('=').map(|(_, rest)| rest.starts_with(kw) && rest[kw.len()..].starts_with('(')).unwrap_or(false))
        .collect()
}

fn qto_count(ifc: &str, name: &str) -> usize {
    lines_of(ifc, "IFCELEMENTQUANTITY").iter().filter(|l| l.contains(&format!(",'{name}',"))).count()
}

fn check_ifc(name: &str, s: &DocState) -> String {
    use guhit_export::ifc::{count_entities, dangling_refs, write_with_counts};
    let (ifc, counts) = write_with_counts(&s.project, &s.derived).unwrap();
    assert!(dangling_refs(&ifc).is_empty(), "{name}: {:?}", dangling_refs(&ifc));
    assert_eq!(counts, count_entities(&ifc), "{name}: counts match the file");
    // Every wall, door, window, space and slab has its base quantities.
    assert_eq!(qto_count(&ifc, "Qto_WallBaseQuantities"), counts.walls, "{name} walls");
    assert_eq!(qto_count(&ifc, "Qto_DoorBaseQuantities"), counts.doors, "{name} doors");
    assert_eq!(qto_count(&ifc, "Qto_WindowBaseQuantities"), counts.windows, "{name} windows");
    assert_eq!(qto_count(&ifc, "Qto_SpaceBaseQuantities"), counts.spaces, "{name} spaces");
    assert_eq!(qto_count(&ifc, "Qto_SlabBaseQuantities"), counts.slabs, "{name} slabs");
    assert_eq!(qto_count(&ifc, "Qto_RoofBaseQuantities"), counts.roofs, "{name} roofs");
    assert_eq!(qto_count(&ifc, "Qto_StairFlightBaseQuantities"), counts.stair_flights, "{name} flights");
    // Every door, window and wall has a type, and every type is declared.
    let refs = |l: &str| l.split_once('=').map(|(_, rest)| rest.matches('#').count()).unwrap_or(0);
    let typed: usize = lines_of(&ifc, "IFCRELDEFINESBYTYPE").iter().map(|l| refs(l) - 1).sum();
    assert_eq!(typed, counts.doors + counts.windows + counts.walls - lines_of(&ifc, "IFCWALL").iter().filter(|l| l.contains("'Gable wall")).count());
    if counts.door_types + counts.window_types + counts.wall_types > 0 {
        assert_eq!(lines_of(&ifc, "IFCRELDECLARES").len(), 1);
    }
    assert_eq!(ifc, write_with_counts(&s.project, &s.derived).unwrap().0, "{name}: the same file twice");
    std::fs::write(out_dir().join(format!("{name}.ifc")), &ifc).unwrap();
    ifc
}

#[test]
fn ifc_types_carry_the_marks() {
    let s = typed();
    let ifc = check_ifc("typed", &s);
    let c = guhit_export::ifc::count_entities(&ifc);
    // D1 and the untyped single, double and sliding doors.
    assert_eq!(c.door_types, 4);
    // W1 and the untyped casement, jalousie and fixed windows.
    assert_eq!(c.window_types, 4);
    // 150 and 100 mm walls.
    assert_eq!(c.wall_types, 2);
    let d1 = lines_of(&ifc, "IFCDOORTYPE").into_iter().find(|l| l.contains(",'D1',")).expect("type D1");
    assert!(d1.contains("'Solid wood panel door, painted'"), "{d1}");
    assert!(d1.contains(",'D1','swing single 800 x 2100',.DOOR."), "the mark is the tag: {d1}");
    // Its three doors are tagged D1 and point at it.
    let doors = lines_of(&ifc, "IFCDOOR");
    assert_eq!(doors.iter().filter(|l| l.contains("'Door D1'") && l.contains(",'D1',")).count(), 3);
    assert!(ifc.contains("IFCPROPERTYSINGLEVALUE('Reference',$,IFCLABEL('D1'),$)"));
    let w1 = lines_of(&ifc, "IFCWINDOWTYPE").into_iter().find(|l| l.contains(",'W1',")).expect("type W1");
    assert!(w1.contains(".DOUBLE_PANEL_HORIZONTAL."), "{w1}");
    assert!(lines_of(&ifc, "IFCWALLTYPE").iter().any(|l| l.contains("'Wall 150 mm'") && l.ends_with(",.SOLIDWALL.);")));
}

#[test]
fn ifc_quantities_add_up() {
    let s = typed();
    let ifc = check_ifc("typed", &s);
    // The 8000 mm front wall, 150 thick, holds the 900 main door, the 1800
    // sliding door, 2100 high each.
    let h = s.project.levels[0].height_mm;
    let side = 8.0 * h / 1000.0;
    let net = side - (0.9 + 1.8) * 2.1;
    assert!(ifc.contains(&format!("IFCQUANTITYAREA('GrossSideArea',$,$,{},$)", fmt(side))), "gross side {side}");
    assert!(ifc.contains(&format!("IFCQUANTITYAREA('NetSideArea',$,$,{},$)", fmt(net))), "net side {net}");
    // The living room, 4875 x 5850.
    assert!(ifc.contains("IFCQUANTITYAREA('NetFloorArea',$,$,28.51875,$)"));
    assert!(ifc.contains("IFCQUANTITYLENGTH('GrossPerimeter',$,$,21450.,$)"));
    // The 800 x 2100 doors.
    assert!(ifc.contains("IFCQUANTITYAREA('Area',$,$,1.68,$)"));
}

fn fmt(x: f64) -> String {
    let mut s = format!("{x:.6}");
    while s.ends_with('0') {
        s.pop();
    }
    s
}

#[test]
fn ifc_stairs_roofs_grid_site_and_fixtures() {
    let s = derived_wave2();
    let ifc = check_ifc("typed-wave2", &s);
    let c = guhit_export::ifc::count_entities(&ifc);
    // The L stair: two flights of seven treads and a landing slab.
    assert_eq!(c.stairs, 1);
    assert!(lines_of(&ifc, "IFCSTAIR")[0].ends_with(",.QUARTER_TURN_STAIR.);"));
    assert_eq!(c.stair_flights, 2);
    let riser = s.project.levels[0].height_mm / 16.0;
    for f in lines_of(&ifc, "IFCSTAIRFLIGHT") {
        assert!(f.ends_with(&format!(",8,7,{},250.,.STRAIGHT.);", fmt(riser))), "{f}");
    }
    assert_eq!(lines_of(&ifc, "IFCSLAB").iter().filter(|l| l.ends_with(",.LANDING.);")).count(), 1);
    // The stair aggregates both flights and the landing.
    let stair = lines_of(&ifc, "IFCSTAIR")[0].split_once('=').unwrap().0.to_string();
    let parts = lines_of(&ifc, "IFCRELAGGREGATES")
        .into_iter()
        .find(|l| l.contains(&format!(",$,$,$,{stair},(")))
        .expect("the stair's parts");
    assert_eq!(parts.split_once('=').unwrap().1.matches('#').count(), 1 + 3, "{parts}");
    // The hip over its four planes and the lanai gable over two, with the
    // gable ends as walls.
    assert_eq!(c.roofs, 2);
    assert!(lines_of(&ifc, "IFCROOF").iter().any(|l| l.ends_with(",.HIP_ROOF.);")));
    assert!(lines_of(&ifc, "IFCROOF").iter().any(|l| l.ends_with(",.GABLE_ROOF.);")));
    assert_eq!(lines_of(&ifc, "IFCSLAB").iter().filter(|l| l.ends_with(",.ROOF.);")).count(), 6);
    assert_eq!(lines_of(&ifc, "IFCWALL").iter().filter(|l| l.contains("'Gable wall")).count(), 2);
    assert!(ifc.contains("IFCQUANTITYAREA('ProjectedArea',$,$,66.24,$)"));
    // The grid: 1 and 2 one way, A and B the other.
    assert_eq!(c.grids, 1);
    assert_eq!(lines_of(&ifc, "IFCGRIDAXIS").len(), 4);
    assert!(lines_of(&ifc, "IFCGRID")[0].ends_with(",.RECTANGULAR.);"));
    // Parañaque, 14.4793 N, 121.0198 E.
    let site = lines_of(&ifc, "IFCSITE")[0];
    assert!(site.contains(",(14,28,45,480000),(121,1,11,280000),"), "{site}");
}

#[test]
fn ifc_of_the_plumbing_demo_has_its_fixtures() {
    let s = demo();
    let ifc = check_ifc("plumbing-demo", &s);
    let c = guhit_export::ifc::count_entities(&ifc);
    // The kitchen sink, the lavatory, the WC and the shower.
    assert_eq!(c.sanitary, 4);
    for kind in [".SINK.", ".WASHHANDBASIN.", ".TOILETPAN.", ".SHOWER."] {
        assert!(lines_of(&ifc, "IFCSANITARYTERMINAL").iter().any(|l| l.ends_with(&format!(",{kind});"))), "{kind}");
    }
    check_ifc("bungalow", &fixture());
    check_ifc("rich", &rich());
    check_ifc("services", &services());
    check_ifc("slabs", &slabs());
}

#[test]
fn the_site_tct_is_the_land_title_number() {
    let mut s = typed();
    s.project.title_block.tct_no = "T-123456".into();
    let ifc = guhit_export::model_ifc(&s.project, &s.derived).unwrap();
    assert!(lines_of(&ifc, "IFCSITE")[0].ends_with(",'T-123456',$);"));
    s.project.settings.site = None;
    let ifc = guhit_export::model_ifc(&s.project, &s.derived).unwrap();
    assert!(lines_of(&ifc, "IFCSITE")[0].contains(",.ELEMENT.,$,$,$,'T-123456',$);"));
}
