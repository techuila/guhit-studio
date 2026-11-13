//! Slabs, railings and stairs in the exports (docs/CONTRACT.md "Slabs and
//! railings", DECISIONS D44 and D50): the plan SVG and PDF, the 2D DXF, IFC
//! and the 3D DXF.

mod common;

use std::collections::{HashMap, HashSet};

use common::*;
use guhit_export::dxf3d::{build_faces, faces_per_layer};
use guhit_export::geom::{point_in_polygon, v, V};
use guhit_export::ifc::{count_entities, dangling_refs, write_with_counts};
use guhit_export::plan::{build_items, Cat, Fill, Item, PlanOptions, Prim};
use guhit_export::{model3d, model_dxf3d, model_ifc, plan_dxf, plan_pdf, plan_svg};
use guhit_model::*;
use svg2pdf::usvg;

fn second() -> PlanExportOptions {
    let mut opts = options(PaperSize::A3, Orientation::Landscape);
    opts.level_id = Some(SECOND.into());
    opts
}

fn level<'a>(s: &'a DocState, id: &str) -> &'a Level {
    s.project.levels.iter().find(|l| l.id == id).unwrap()
}

fn items(s: &DocState, level_id: &str) -> Vec<Item> {
    build_items(
        &s.project,
        &s.derived,
        level(s, level_id),
        &PlanOptions {
            scale: 100.0,
            show_dimensions: true,
            show_room_labels: true,
            show_assets: true,
            unicode: true,
            skip_devices: false,
        },
    )
}

fn of(items: &[Item], cat: Cat) -> Vec<&Item> {
    items.iter().filter(|i| i.cat == cat).collect()
}

fn lines(items: &[&Item]) -> Vec<(V, V)> {
    items
        .iter()
        .filter_map(|i| match i.prim {
            Prim::Line { a, b } => Some((a, b)),
            _ => None,
        })
        .collect()
}

/// The SVG group with this id, up to its closing tag.
fn group<'a>(svg: &'a str, id: &str) -> &'a str {
    let open = format!("<g id=\"{id}\">");
    let start = svg.find(&open).unwrap_or_else(|| panic!("no group {id}"));
    let end = svg[start..].find("</g>").unwrap() + start;
    &svg[start..end]
}

fn on_segment(p: V, a: V, b: V) -> bool {
    let d = b - a;
    let cross = (p.x - a.x) * d.y - (p.y - a.y) * d.x;
    cross.abs() / d.len() < 1e-6 && (p - a).dot(d) >= -1e-6 && (p - b).dot(a - b) >= -1e-6
}

fn parse_svg(svg: &str) -> usvg::Tree {
    let mut db = usvg::fontdb::Database::new();
    db.load_system_fonts();
    let options = usvg::Options {
        fontdb: std::sync::Arc::new(db),
        ..usvg::Options::default()
    };
    usvg::Tree::from_str(svg, &options).expect("usvg parses the sheet")
}

// ------------------------------------------------------------------ plan SVG

#[test]
fn svg_draws_slabs_and_railings_in_their_groups() {
    let mut s = slabs();
    // Plans draw stairs from `Derived::stairs` (D54).
    s.derived.stairs = guhit_core::compute_derived(&s.project).stairs;
    let svg = plan_svg(&s.project, &s.derived, &second()).unwrap().data;
    let slabs = group(&svg, "slabs");
    // Floor slab 5 percent, balcony 10 percent, voids untinted.
    assert_eq!(slabs.matches("fill-opacity=\"0.05\"").count(), 1, "{slabs}");
    assert_eq!(slabs.matches("fill-opacity=\"0.1\"").count(), 1);
    assert_eq!(slabs.matches("OPEN TO BELOW").count(), 2, "one label per void");
    let rails = group(&svg, "railings");
    // Balcony glass and void bars two lines each, the parapet one band, and
    // the stair well guard two lines.
    assert_eq!(rails.matches("<polyline").count() + rails.matches("<polygon").count(), 7, "{rails}");
    assert_eq!(rails.matches("fill-opacity=\"0.22\"").count(), 1, "the parapet band");
    // Groups draw back to front: slabs first, railings after the stairs.
    let at = |id: &str| svg.find(&format!("<g id=\"{id}\">")).unwrap();
    assert!(at("slabs") < at("walls"));
    assert!(at("stairs-below") < at("railings") && at("stairs") < at("railings"));
    parse_svg(&svg);
    // The PDF of the same sheet builds.
    let pdf = plan_pdf(&s.project, &s.derived, &second()).unwrap().data;
    assert!(pdf.starts_with(b"%PDF"));
}

#[test]
fn a_four_corner_void_is_crossed_corner_to_corner() {
    let s = slabs();
    let all = items(&s, SECOND);
    let d1 = (v(500.0, 3500.0), v(2500.0, 5500.0));
    let d2 = (v(2500.0, 3500.0), v(500.0, 5500.0));
    let marks = lines(&of(&all, Cat::Slab));
    let on = |d: (V, V)| -> Vec<(V, V)> {
        marks
            .iter()
            .copied()
            .filter(|(a, b)| on_segment(*a, d.0, d.1) && on_segment(*b, d.0, d.1))
            .collect()
    };
    for d in [d1, d2] {
        let parts = on(d);
        // The label in the middle splits each diagonal in two.
        assert_eq!(parts.len(), 2, "{parts:?}");
        let ends: Vec<V> = parts.iter().flat_map(|(a, b)| [*a, *b]).collect();
        assert!(ends.iter().any(|p| (*p - d.0).len() < 1e-6), "reaches one corner");
        assert!(ends.iter().any(|p| (*p - d.1).len() < 1e-6), "reaches the other");
        let length: f64 = parts.iter().map(|(a, b)| (*b - *a).len()).sum();
        assert!(length < (d.1 - d.0).len() - 100.0, "a gap for the label");
    }
}

#[test]
fn an_l_shaped_void_crosses_its_bounding_box_inside_the_outline() {
    let s = slabs();
    let all = items(&s, SECOND);
    let l = [
        v(5500.0, 1000.0),
        v(7500.0, 1000.0),
        v(7500.0, 2000.0),
        v(6500.0, 2000.0),
        v(6500.0, 3000.0),
        v(5500.0, 3000.0),
    ];
    let diagonals = [(v(5500.0, 1000.0), v(7500.0, 3000.0)), (v(5500.0, 3000.0), v(7500.0, 1000.0))];
    let marks: Vec<(V, V)> = lines(&of(&all, Cat::Slab))
        .into_iter()
        .filter(|(a, _)| a.x >= 5499.0)
        .collect();
    assert!(!marks.is_empty());
    for (a, b) in &marks {
        assert!(
            diagonals.iter().any(|d| on_segment(*a, d.0, d.1) && on_segment(*b, d.0, d.1)),
            "{a:?} {b:?} is on a bounding box diagonal"
        );
        let mid = (*a + *b) * 0.5;
        assert!(point_in_polygon(mid, &l), "{mid:?} stays inside the outline");
    }
    // The diagonal from (5500, 1000) runs inside as far as the inner corner.
    assert!(marks.iter().any(|(a, b)| (*a - v(5500.0, 1000.0)).len() < 1e-6 || (*b - v(5500.0, 1000.0)).len() < 1e-6));
    assert!(!marks.iter().any(|(a, b)| a.x > 6501.0 && a.y > 2001.0 || b.x > 6501.0 && b.y > 2001.0), "nothing in the notch");
}

#[test]
fn bars_and_glass_rails_are_two_lines_50_apart() {
    let s = slabs();
    let all = items(&s, SECOND);
    let rails: Vec<&Item> = of(&all, Cat::Rail);
    let polys: Vec<(&Vec<V>, bool, Fill)> = rails
        .iter()
        .filter_map(|i| match &i.prim {
            Prim::Poly { pts, closed, fill } => Some((pts, *closed, *fill)),
            _ => None,
        })
        .collect();
    // The balcony rail starts at (1000, -75) running south: its two lines
    // start 25 mm either side.
    let starts: Vec<V> = polys.iter().map(|p| p.0[0]).collect();
    assert!(starts.iter().any(|p| (*p - v(1025.0, -75.0)).len() < 1e-6), "{starts:?}");
    assert!(starts.iter().any(|p| (*p - v(975.0, -75.0)).len() < 1e-6));
    // The void rail goes all the way round: two closed rings.
    let rings: Vec<&(&Vec<V>, bool, Fill)> = polys.iter().filter(|p| p.1 && p.2 == Fill::None).collect();
    assert_eq!(rings.len(), 2);
    for r in rings {
        assert_eq!(r.0.len(), 4);
    }
    // The parapet is one tinted band 100 mm wide.
    let band = polys.iter().find(|p| matches!(p.2, Fill::Tint(_))).expect("a parapet band");
    assert!(band.1);
    assert!(band.0.iter().any(|p| (*p - v(-50.0, 6075.0)).len() < 1e-6), "{:?}", band.0);
    assert!(band.0.iter().any(|p| (*p - v(50.0, 6075.0)).len() < 1e-6));
}

#[test]
fn hidden_layers_hide_slabs_and_their_rails() {
    let mut s = slabs();
    set_layer(&mut s, LayerKey::Slabs, false);
    let all = items(&s, SECOND);
    assert!(of(&all, Cat::Slab).is_empty());
    // Only the stair well guard is left: two lines.
    assert_eq!(of(&all, Cat::Rail).len(), 2);
    let svg = plan_svg(&s.project, &s.derived, &second()).unwrap().data;
    assert!(!svg.contains("<g id=\"slabs\">"));

    let mut s = slabs();
    set_layer(&mut s, LayerKey::Stairs, false);
    let all = items(&s, SECOND);
    assert!(of(&all, Cat::StrsBelow).is_empty() && of(&all, Cat::Strs).is_empty());
    assert_eq!(of(&all, Cat::Rail).len(), 5, "slab rails stay");
    let ground = items(&s, &level_id(&s));
    assert!(of(&ground, Cat::Rail).is_empty(), "stair side rails go with the stair");
}

// ------------------------------------------------------------- stairs (D44)

// ----------------------------------------------------------------------- DXF

fn load_dxf(text: &str) -> dxf::Drawing {
    dxf::Drawing::load(&mut text.as_bytes()).expect("dxf crate parses the output")
}

#[test]
fn dxf_without_slabs_has_none_of_the_new_layers() {
    let s = fixture();
    let text = plan_dxf(&s.project, &s.derived, &options(PaperSize::A3, Orientation::Landscape)).unwrap().data;
    for l in ["A-FLOR-OTLN", "A-FLOR-HRAL", "GUHIT_DASHED"] {
        assert!(!text.contains(l), "{l} in a plan without slabs");
    }
    let text = model_dxf3d(&s.project, &s.derived).unwrap();
    for l in ["A-FLOR-OTLN", "A-FLOR-HRAL", "GUHIT_DASHED"] {
        assert!(!text.contains(l), "{l} in a model without slabs");
    }
}

// ----------------------------------------------------------------------- IFC

/// The entity lines of a file by number.
fn entities(ifc: &str) -> HashMap<usize, String> {
    ifc.lines()
        .filter_map(|l| {
            let (n, rest) = l.strip_prefix('#')?.split_once('=')?;
            Some((n.parse().ok()?, rest.trim_end_matches(';').to_string()))
        })
        .collect()
}

/// The quoted Name (third attribute) of an entity line.
fn name_of(line: &str) -> String {
    let parts: Vec<&str> = line.split(',').collect();
    parts[2].trim_matches('\'').to_string()
}

#[test]
fn ifc_writes_slabs_with_their_kinds_and_unique_names() {
    let s = slabs();
    let (ifc, counts) = write_with_counts(&s.project, &s.derived).unwrap();
    assert_eq!(counts, count_entities(&ifc));
    assert!(dangling_refs(&ifc).is_empty(), "{:?}", dangling_refs(&ifc));
    let ents = entities(&ifc);
    let slab_lines: Vec<&String> = ents.values().filter(|l| l.starts_with("IFCSLAB(")).collect();
    // Two footprint slabs, the rear floor slab, the balcony, two roof planes.
    assert_eq!(slab_lines.len(), 6);
    let floors: Vec<&&String> = slab_lines.iter().filter(|l| l.ends_with(",$,.FLOOR.)")).collect();
    assert_eq!(floors.len(), 3);
    let balcony: Vec<&&String> = slab_lines.iter().filter(|l| l.contains("'BALCONY'")).collect();
    assert_eq!(balcony.len(), 1);
    assert!(balcony[0].ends_with(",$,.USERDEFINED.)") && balcony[0].contains("'Balcony 1'"));
    let names: Vec<String> = slab_lines.iter().map(|l| name_of(l)).collect();
    let unique: HashSet<&String> = names.iter().collect();
    assert_eq!(unique.len(), names.len(), "duplicate slab names: {names:?}");
    for n in ["Floor slab 1", "Floor slab 2", "Floor slab 3", "Balcony 1"] {
        assert!(names.iter().any(|x| x == n), "{n} in {names:?}");
    }
    assert!(ifc.contains("IFCPROPERTYSINGLEVALUE('IsExternal',$,IFCBOOLEAN(.T.),$)"));

    // A deck is USERDEFINED with ObjectType DECK.
    let mut d = slabs();
    for e in d.project.elements.iter_mut() {
        if let Element::Slab(sl) = e {
            if sl.id == "sl-balcony" {
                sl.kind = SlabKind::Deck;
                sl.top_offset_mm = -150.0;
                sl.thickness_mm = 100.0;
            }
        }
    }
    let ifc = model_ifc(&d.project, &d.derived).unwrap();
    let deck = ifc.lines().find(|l| l.contains("'DECK'")).expect("a deck");
    assert!(deck.contains("'Deck 1'") && deck.ends_with(",$,.USERDEFINED.);"), "{deck}");
    // Its solid hangs from 150 mm under the floor, 100 mm deep.
    assert!(ifc.contains("IFCCARTESIANPOINT((0.,0.,-250.))"));
}

#[test]
fn ifc_voids_open_the_floor_they_cross() {
    let s = slabs();
    let ifc = model_ifc(&s.project, &s.derived).unwrap();
    let ents = entities(&ifc);
    let openings: Vec<(usize, &String)> = ents
        .iter()
        .filter(|(_, l)| l.starts_with("IFCOPENINGELEMENT(") && l.contains("'Void "))
        .map(|(n, l)| (*n, l))
        .collect();
    assert_eq!(openings.len(), 2, "both voids lie over the second floor slab only");
    let floor2 = ents
        .iter()
        .find(|(_, l)| l.starts_with("IFCSLAB(") && l.contains("'Floor slab 2'"))
        .map(|(n, _)| *n)
        .unwrap();
    for (n, l) in &openings {
        assert!(l.contains("in Floor slab 2'"), "{l}");
        let rels: Vec<&String> = ents
            .values()
            .filter(|r| r.starts_with("IFCRELVOIDSELEMENT(") && r.ends_with(&format!(",#{n})")))
            .collect();
        assert_eq!(rels.len(), 1, "one IfcRelVoidsElement per opening");
        assert!(rels[0].contains(&format!(",#{floor2},")), "{} voids the second floor slab", rels[0]);
    }
    // A void across the floor slab and the footprint opens both.
    let mut wide = slabs();
    for e in wide.project.elements.iter_mut() {
        if let Element::Slab(sl) = e {
            if sl.id == "sl-void" {
                sl.polygon = [(500.0, 5500.0), (2500.0, 5500.0), (2500.0, 6500.0), (500.0, 6500.0)]
                    .iter()
                    .map(|p| Point { x: p.0, y: p.1 })
                    .collect();
            }
        }
    }
    let ifc = model_ifc(&wide.project, &wide.derived).unwrap();
    assert!(dangling_refs(&ifc).is_empty());
    assert!(ifc.contains("'Void 1 in Floor slab 2'") && ifc.contains("'Void 1 in Floor slab 3'"));
    assert_eq!(ifc.matches("IFCRELVOIDSELEMENT(").count(), ifc.matches("IFCOPENINGELEMENT(").count());
    assert_eq!(ifc.matches("IFCOPENINGELEMENT(").count() - count_entities(&model_ifc(&slabs().project, &slabs().derived).unwrap()).openings, 1);
}

#[test]
fn ifc_writes_one_railing_per_run_in_its_storey() {
    let s = slabs();
    let ifc = model_ifc(&s.project, &s.derived).unwrap();
    assert!(dangling_refs(&ifc).is_empty());
    assert_eq!(count_entities(&ifc).railings, 6);
    let ents = entities(&ifc);
    let rails: Vec<(usize, &String)> = ents
        .iter()
        .filter(|(_, l)| l.starts_with("IFCRAILING("))
        .map(|(n, l)| (*n, l))
        .collect();
    let guards: Vec<_> = rails.iter().filter(|(_, l)| l.ends_with(",$,.GUARDRAIL.)")).collect();
    let hands: Vec<_> = rails.iter().filter(|(_, l)| l.ends_with(",$,.HANDRAIL.)")).collect();
    assert_eq!((guards.len(), hands.len()), (4, 2));
    // Contained in the storey of the level each run stands on.
    let storey = |id: &str| {
        let name = &level(&s, id).name;
        ents.iter()
            .find(|(_, l)| l.starts_with("IFCBUILDINGSTOREY(") && l.contains(&format!("'{name}'")))
            .map(|(n, _)| *n)
            .unwrap()
    };
    for (n, _) in &hands {
        assert_eq!(container_of(&ents, *n), storey(&level_id(&s)), "a handrail stands on the ground floor");
    }
    for (n, _) in &guards {
        assert_eq!(container_of(&ents, *n), storey(SECOND), "guards stand on the second floor");
    }
    // Every GlobalId stays unique with several rails on one stair.
    let ids: Vec<String> = ents
        .values()
        .filter_map(|l| {
            let body = &l[l.find('(')? + 1..];
            let g = body.strip_prefix('\'')?.split('\'').next()?;
            (g.len() == 22).then(|| g.to_string())
        })
        .collect();
    let set: HashSet<&String> = ids.iter().collect();
    assert_eq!(set.len(), ids.len(), "duplicate GlobalId");
    assert!(ifc.contains("IFCPROPERTYSINGLEVALUE('Height',$,IFCPOSITIVELENGTHMEASURE(1100.),$)"));
}

/// The storey an element is contained in.
fn container_of(ents: &HashMap<usize, String>, n: usize) -> usize {
    for l in ents.values().filter(|l| l.starts_with("IFCRELCONTAINEDINSPATIALSTRUCTURE(")) {
        let open = l.find("$,$,$,(").unwrap() + 7;
        let close = l[open..].find(')').unwrap() + open;
        let members: Vec<usize> = l[open..close]
            .split(',')
            .map(|x| x.trim_start_matches('#').parse().unwrap())
            .collect();
        if members.contains(&n) {
            return l[close + 2..].trim_end_matches(')').trim_start_matches('#').parse().unwrap();
        }
    }
    panic!("#{n} is in no storey");
}

// -------------------------------------------------------------------- 3D DXF

#[test]
fn dxf3d_treads_follow_d44_and_rails_go_on_their_layer() {
    let s = slabs();
    let faces = build_faces(&s.project, &s.derived);
    let per_layer: HashMap<&str, usize> = faces_per_layer(&faces).into_iter().collect();
    // Ten tread boxes of eight faces each.
    assert_eq!(per_layer["A-FLOR-STRS"], 10 * 8);
    assert!(per_layer["A-FLOR-HRAL"] > 0);
    let stair = s
        .project
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Stair(st) => Some(st),
            _ => None,
        })
        .unwrap();
    let treads = model3d::stair_treads(stair, 3000.0);
    assert_eq!(treads.len(), 10);
    assert!((treads[9].top - 3000.0 * 10.0 / 11.0).abs() < 1e-6, "the top riser lands on the floor above");

    // A stair side rail climbs: its faces span from one riser up to the
    // level height plus the rail. The west side of the flight, where the
    // well guard upstairs stands too.
    let zs: Vec<f64> = faces
        .list
        .iter()
        .filter(|f| {
            f.layer == "A-FLOR-HRAL"
                && f.pts.iter().all(|p| p.x < 3100.0 && p.x > 2900.0 && p.y > 300.0 && p.y < 3400.0)
        })
        .flat_map(|f| f.pts.iter().map(|p| p.z).collect::<Vec<_>>())
        .collect();
    let lo = zs.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = zs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    assert!((200.0..300.0).contains(&lo), "{lo}");
    assert!((3899.0..4000.0).contains(&hi), "{hi}");

    let text = model_dxf3d(&s.project, &s.derived).unwrap();
    let drawing = load_dxf(&text);
    assert!(drawing.layers().any(|l| l.name == "A-FLOR-HRAL"));
    assert!(drawing.layers().any(|l| l.name == "A-FLOR-OTLN"), "the 2D slab linework");
    let faces_on = drawing
        .entities()
        .filter(|e| e.common.layer == "A-FLOR-HRAL" && matches!(e.specific, dxf::entities::EntityType::Face3D(_)))
        .count();
    assert_eq!(faces_on, per_layer["A-FLOR-HRAL"]);
}

#[test]
fn dxf3d_slabs_leave_the_voids_open() {
    let s = slabs();
    let faces = build_faces(&s.project, &s.derived);
    // Top caps of the second floor slab, at its floor height.
    let caps: Vec<[V; 3]> = faces
        .list
        .iter()
        .filter(|f| f.layer == "A-FLOR-SLAB" && f.pts.iter().all(|p| (p.z - 3000.0).abs() < 1e-6))
        .map(|f| [v(f.pts[0].x, f.pts[0].y), v(f.pts[1].x, f.pts[1].y), v(f.pts[2].x, f.pts[2].y)])
        .collect();
    assert!(!caps.is_empty());
    let covered = |p: V| caps.iter().any(|t| point_in_polygon(p, t));
    assert!(!covered(v(1500.0, 4500.0)), "the void over the living room is open");
    assert!(!covered(v(6000.0, 2500.0)), "the L-shaped void is open");
    assert!(covered(v(7000.0, 2500.0)), "the notch of the L is floor");
    assert!(covered(v(4500.0, 4500.0)), "the rest is floor");
    assert!(covered(v(2000.0, 7000.0)), "the rear floor slab");
    // The balcony's top is level with the floor too, outside the walls.
    assert!(covered(v(2000.0, -1000.0)), "the balcony");
}

// ----------------------------------------------------------------- the sheet

fn write_png(svg: &str, path: &std::path::Path, px_per_mm: f32) {
    let tree = parse_svg(svg);
    let size = tree.size();
    let scale = px_per_mm * 25.4 / 96.0;
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).expect("pixmap");
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.save_png(path).expect("write png");
}

/// Writes the slab sheets a person should look at: target/export-samples/.
#[test]
fn slab_samples_are_written() {
    let s = slabs();
    let dir = samples_dir();
    for (name, lvl) in [("slabs-ground", level_id(&s)), ("slabs-second", SECOND.to_string())] {
        let mut opts = options(PaperSize::A3, Orientation::Landscape);
        opts.level_id = Some(lvl);
        let svg = plan_svg(&s.project, &s.derived, &opts).unwrap().data;
        std::fs::write(dir.join(format!("{name}.svg")), &svg).unwrap();
        write_png(&svg, &dir.join(format!("{name}.png")), 6.0);
        let dxf = plan_dxf(&s.project, &s.derived, &opts).unwrap().data;
        std::fs::write(dir.join(format!("{name}.dxf")), dxf).unwrap();
    }
    std::fs::write(dir.join("slabs.ifc"), model_ifc(&s.project, &s.derived).unwrap()).unwrap();
    std::fs::write(dir.join("slabs-3d.dxf"), model_dxf3d(&s.project, &s.derived).unwrap()).unwrap();
}
