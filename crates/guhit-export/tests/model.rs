//! Whole-model exports: IFC4 and 3D DXF.

mod common;

use std::collections::{HashMap, HashSet};

use common::*;
use guhit_export::dxf3d::{build_faces, faces_per_layer};
use guhit_export::ifc::{count_entities, dangling_refs, guid};
use guhit_export::{model_dxf3d, model_ifc, ExportError};
use guhit_model::*;

fn write_sample(name: &str, text: &str) -> std::path::PathBuf {
    let path = samples_dir().join(name);
    std::fs::write(&path, text).expect("write sample");
    path
}

// ------------------------------------------------------------------- IFC4

#[test]
fn ifc_has_a_valid_step_header_and_schema() {
    let s = fixture();
    let ifc = model_ifc(&s.project, &s.derived).unwrap();
    assert!(ifc.starts_with("ISO-10303-21;\n"));
    assert!(ifc.trim_end().ends_with("END-ISO-10303-21;"));
    assert!(ifc.contains("FILE_SCHEMA(('IFC4'));"));
    assert!(ifc.contains("FILE_DESCRIPTION("));
    assert!(ifc.contains("'Guhit Studio'"), "the writer names itself");
    assert!(ifc.contains(&format!("'{}.ifc'", s.project.name)));
    assert!(ifc.contains("\nDATA;\n") && ifc.contains("\nENDSEC;\n"));
    // No dashes other than a hyphen anywhere in the file.
    assert!(!ifc.contains('\u{2014}') && !ifc.contains('\u{2013}'));
}

#[test]
fn ifc_declares_millimeters_and_the_other_units() {
    let s = fixture();
    let ifc = model_ifc(&s.project, &s.derived).unwrap();
    assert!(ifc.contains("IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.)"));
    assert!(ifc.contains("IFCSIUNIT(*,.AREAUNIT.,$,.SQUARE_METRE.)"));
    assert!(ifc.contains("IFCSIUNIT(*,.VOLUMEUNIT.,$,.CUBIC_METRE.)"));
    assert!(ifc.contains(".PLANEANGLEUNIT.,'DEGREE'"));
    assert!(ifc.contains("IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-05,"));
}

#[test]
fn ifc_builds_the_whole_spatial_spine() {
    let s = fixture();
    let ifc = model_ifc(&s.project, &s.derived).unwrap();
    for kw in [
        "IFCPROJECT(",
        "IFCSITE(",
        "IFCBUILDING(",
        "IFCBUILDINGSTOREY(",
        "IFCRELAGGREGATES(",
        "IFCRELCONTAINEDINSPATIALSTRUCTURE(",
    ] {
        assert!(ifc.contains(kw), "missing {kw}");
    }
    let storeys = ifc.matches("IFCBUILDINGSTOREY(").count();
    assert_eq!(storeys, s.project.levels.len());
    assert!(ifc.contains(&format!("'{}'", s.project.levels[0].name)));
}

#[test]
fn ifc_counts_match_the_fixture() {
    let s = fixture();
    let ifc = model_ifc(&s.project, &s.derived).unwrap();
    let c = count_entities(&ifc);

    let walls = s
        .project
        .elements
        .iter()
        .filter(|e| matches!(e, Element::Wall(_)))
        .count();
    let doors = s
        .project
        .elements
        .iter()
        .filter(|e| matches!(e, Element::Opening(o) if o.opening_type == OpeningType::Door))
        .count();
    let windows = s
        .project
        .elements
        .iter()
        .filter(|e| matches!(e, Element::Opening(o) if o.opening_type == OpeningType::Window))
        .count();
    let rooms = s
        .project
        .elements
        .iter()
        .filter(|e| matches!(e, Element::Room(_)))
        .count();

    assert_eq!(c.walls, walls, "one IfcWall per wall");
    assert_eq!(c.doors, doors);
    assert_eq!(c.windows, windows);
    assert_eq!(c.openings, doors + windows, "one void per opening");
    assert_eq!(c.spaces, rooms);
    assert_eq!(c.doors as u32, s.derived.totals.door_count);
    assert_eq!(c.windows as u32, s.derived.totals.window_count);

    // One floor slab per footprint, plus two roof planes for the gable.
    assert_eq!(s.project.roof.kind, RoofKind::Gable);
    assert_eq!(c.roofs, 1, "one IfcRoof aggregating the planes");
    assert_eq!(c.slabs, s.derived.footprints.len() + 2);
    assert_eq!(ifc.matches("IFCRELVOIDSELEMENT(").count(), doors + windows);
    assert_eq!(ifc.matches("IFCRELFILLSELEMENT(").count(), doors + windows);
}

#[test]
fn ifc_counts_match_the_rich_project() {
    let s = rich();
    let ifc = model_ifc(&s.project, &s.derived).unwrap();
    let c = count_entities(&ifc);
    assert_eq!(c.walls, 7);
    assert_eq!(c.doors, 5);
    assert_eq!(c.windows, 4);
    assert_eq!(c.openings, 9);
    assert_eq!(c.spaces, 3);
    assert_eq!(c.columns, 2);
    assert_eq!(c.stairs, 1);
    assert_eq!(c.furnishings, 2);
    assert_eq!(c.annotations, 1);
    assert!(ifc.contains("IFCSTAIRFLIGHT("));
    // Dimensions are skipped: `rich` has three of them and one annotation,
    // and only the annotation reaches the file.
    let dimensions = s
        .project
        .elements
        .iter()
        .filter(|e| matches!(e, Element::Dimension(_)))
        .count();
    assert_eq!(dimensions, 3);
    assert_eq!(c.annotations, 1);
}

#[test]
fn every_ifc_reference_resolves() {
    for s in [fixture(), rich()] {
        let ifc = model_ifc(&s.project, &s.derived).unwrap();
        let missing = dangling_refs(&ifc);
        assert!(missing.is_empty(), "dangling references: {missing:?}");

        // Entity numbers are contiguous from 1 and never repeat.
        let mut seen = HashSet::new();
        let mut expected = 0usize;
        for line in ifc.lines() {
            let Some(rest) = line.strip_prefix('#') else {
                continue;
            };
            let (num, _) = rest.split_once('=').expect("entity line");
            let n: usize = num.parse().expect("entity number");
            expected += 1;
            assert_eq!(n, expected, "entity numbers run in order");
            assert!(seen.insert(n), "entity #{n} written twice");
            assert!(line.ends_with(';'), "entity #{n} is not terminated");
        }
        assert!(expected > 100, "the file should be substantial");
    }
}

#[test]
fn ifc_guids_are_stable_across_re_exports() {
    let s = fixture();
    let a = model_ifc(&s.project, &s.derived).unwrap();
    let b = model_ifc(&s.project, &s.derived).unwrap();
    assert_eq!(a, b, "the same project exports byte identical IFC");

    let ids = |text: &str| -> Vec<String> {
        text.lines()
            .filter_map(|l| {
                let rest = l.split_once('=')?.1;
                let start = rest.find('(')? + 1;
                let body = &rest[start..];
                if !body.starts_with('\'') {
                    return None;
                }
                let end = body[1..].find('\'')? + 1;
                let g = &body[1..end];
                if g.len() == 22 {
                    Some(g.to_string())
                } else {
                    None
                }
            })
            .collect()
    };
    let first = ids(&a);
    assert!(first.len() > 20);
    assert_eq!(first, ids(&b));
    // Every GlobalId is unique.
    let set: HashSet<&String> = first.iter().collect();
    assert_eq!(set.len(), first.len(), "duplicate IfcGloballyUniqueId");

    // Moving a wall keeps every element id.
    let mut moved = s.clone();
    for e in moved.project.elements.iter_mut() {
        if let Element::Wall(w) = e {
            w.start.y -= 250.0;
            w.end.y -= 250.0;
            break;
        }
    }
    let c = model_ifc(&moved.project, &moved.derived).unwrap();
    assert_ne!(a, c, "the geometry did change");
    assert_eq!(first, ids(&c), "ids survive a geometry edit");

    // The wall id drives the wall GlobalId.
    let wall_id = s
        .project
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Wall(w) => Some(w.id.clone()),
            _ => None,
        })
        .unwrap();
    assert!(a.contains(&format!("IFCWALL('{}'", guid(&wall_id))));
}

#[test]
fn ifc_carries_the_contract_properties() {
    let s = rich();
    let ifc = model_ifc(&s.project, &s.derived).unwrap();
    assert!(ifc.contains("'Pset_WallCommon'"));
    assert!(ifc.contains("'IsExternal'"));
    assert!(ifc.contains("IFCPROPERTYSINGLEVALUE('LoadBearing',$,IFCBOOLEAN(.F.),$)"));
    assert!(ifc.contains("'Pset_SpaceCommon'"));
    assert!(ifc.contains("'GrossPlannedArea'"));
    assert!(ifc.contains("'NetPlannedArea'"));
    assert!(ifc.contains("'Pset_DoorCommon'"));
    assert!(ifc.contains("'Pset_WindowCommon'"));
    assert!(ifc.contains("IFCRELASSOCIATESMATERIAL("));
    assert!(ifc.contains("IFCMATERIAL("));

    // Swing side follows the contract: flip_side false swings left.
    assert!(ifc.contains("IFCPROPERTYSINGLEVALUE('SwingSide',$,IFCLABEL('left'),$)"));
    assert!(ifc.contains("IFCPROPERTYSINGLEVALUE('SwingSide',$,IFCLABEL('right'),$)"));
    assert!(ifc.contains(".SINGLE_SWING_LEFT."));
    assert!(ifc.contains(".SINGLE_SWING_RIGHT."));
    // Hinge side too: o-bath is flip_hinge true.
    assert!(ifc.contains("IFCPROPERTYSINGLEVALUE('HingeSide',$,IFCLABEL('end'),$)"));

    // Areas are square meters because AREAUNIT is SQUARE_METRE. The living
    // room polygon in `rich` is 4875 x 5850 mm.
    assert!(
        ifc.contains("IFCAREAMEASURE(28.51875)"),
        "the living room area should be 28.51875 m2"
    );
}

#[test]
fn ifc_bounds_spaces_to_the_walls_that_enclose_them() {
    let s = fixture();
    let ifc = model_ifc(&s.project, &s.derived).unwrap();
    let expected: usize = s.derived.rooms.iter().map(|r| r.wall_ids.len()).sum();
    assert!(expected > 0, "the fixture rooms know their walls");
    assert_eq!(ifc.matches("IFCRELSPACEBOUNDARY(").count(), expected);
    assert!(ifc.contains(",$,.PHYSICAL.,.INTERNAL.)"));
}

#[test]
fn ifc_roof_follows_the_project_setting() {
    let base = fixture();
    for (kind, expect_roof, expect_planes) in [
        (RoofKind::None, false, 0usize),
        (RoofKind::Flat, false, 1),
        (RoofKind::Shed, true, 1),
        (RoofKind::Gable, true, 2),
    ] {
        let mut s = base.clone();
        s.project.roof.kind = kind;
        let ifc = model_ifc(&s.project, &s.derived).unwrap();
        let c = count_entities(&ifc);
        assert_eq!(c.roofs, usize::from(expect_roof), "{kind:?} IfcRoof");
        assert_eq!(
            c.slabs,
            s.derived.footprints.len() + expect_planes,
            "{kind:?} slabs"
        );
        if expect_roof {
            let tag = if kind == RoofKind::Gable {
                ".GABLE_ROOF."
            } else {
                ".SHED_ROOF."
            };
            assert!(ifc.contains(tag));
        }
    }
}

#[test]
fn ifc_refuses_an_empty_model() {
    let mut s = fixture();
    s.project.elements.clear();
    s.derived = Derived::default();
    assert!(matches!(
        model_ifc(&s.project, &s.derived),
        Err(ExportError::Empty(_))
    ));
}

#[test]
fn ifc_samples_are_written() {
    let s = fixture();
    let ifc = model_ifc(&s.project, &s.derived).unwrap();
    let path = write_sample("bungalow.ifc", &ifc);
    assert!(std::fs::metadata(&path).unwrap().len() > 10_000);

    let r = rich();
    let rich_ifc = model_ifc(&r.project, &r.derived).unwrap();
    write_sample("rich.ifc", &rich_ifc);
}

// ----------------------------------------------------------------- 3D DXF

fn parse_dxf(text: &str) -> dxf::Drawing {
    dxf::Drawing::load(&mut text.as_bytes()).expect("the dxf crate parses our output")
}

fn face_points(e: &dxf::entities::Entity) -> Option<[(f64, f64, f64); 4]> {
    match &e.specific {
        dxf::entities::EntityType::Face3D(f) => Some([
            (f.first_corner.x, f.first_corner.y, f.first_corner.z),
            (f.second_corner.x, f.second_corner.y, f.second_corner.z),
            (f.third_corner.x, f.third_corner.y, f.third_corner.z),
            (f.fourth_corner.x, f.fourth_corner.y, f.fourth_corner.z),
        ]),
        _ => None,
    }
}

#[test]
fn dxf3d_parses_and_declares_millimeters() {
    let s = fixture();
    let text = model_dxf3d(&s.project, &s.derived).unwrap();
    assert!(text.contains("AC1009"));
    assert!(text.contains("$INSUNITS"));
    let drawing = parse_dxf(&text);
    assert!(drawing.entities().count() > 50);
    let names: HashSet<String> = drawing.layers().map(|l| l.name.clone()).collect();
    for expected in [
        "A-WALL",
        "A-DOOR",
        "A-GLAZ",
        "A-COLS",
        "A-FLOR-STRS",
        "A-FURN",
        "A-AREA",
        "A-ANNO-DIMS",
        "A-ANNO-TEXT",
        "A-ROOF",
        "A-FLOR-SLAB",
    ] {
        assert!(names.contains(expected), "missing layer {expected}");
    }
}

fn faces_by_layer(drawing: &dxf::Drawing) -> HashMap<String, usize> {
    let mut per_layer: HashMap<String, usize> = HashMap::new();
    for e in drawing.entities() {
        if face_points(e).is_some() {
            *per_layer.entry(e.common.layer.clone()).or_default() += 1;
        }
    }
    per_layer
}

#[test]
fn dxf3d_has_faces_on_every_solid_layer() {
    // `rich` carries columns, a stair and assets but no derived footprint;
    // the fixture carries the footprint, so between them every layer is hit.
    for (s, layers) in [
        (
            rich(),
            vec!["A-WALL", "A-DOOR", "A-GLAZ", "A-COLS", "A-FLOR-STRS", "A-FURN"],
        ),
        (
            fixture(),
            vec!["A-WALL", "A-DOOR", "A-GLAZ", "A-FURN", "A-ROOF", "A-FLOR-SLAB"],
        ),
    ] {
        let text = model_dxf3d(&s.project, &s.derived).unwrap();
        let drawing = parse_dxf(&text);
        let per_layer = faces_by_layer(&drawing);
        for layer in layers {
            assert!(
                per_layer.get(layer).copied().unwrap_or(0) > 0,
                "no 3DFACE on {layer}, got {per_layer:?}"
            );
        }
        // The counts the builder produced agree with what the file holds.
        let built: HashMap<&str, usize> = faces_per_layer(&build_faces(&s.project, &s.derived))
            .into_iter()
            .collect();
        for (layer, n) in &built {
            assert_eq!(
                per_layer.get(*layer).copied().unwrap_or(0),
                *n,
                "face count for {layer}"
            );
        }
    }
}

#[test]
fn dxf3d_roof_faces_follow_the_roof_kind() {
    let base = fixture();
    for (kind, min_faces) in [
        (RoofKind::None, 0usize),
        (RoofKind::Flat, 8),
        (RoofKind::Shed, 8),
        (RoofKind::Gable, 16),
    ] {
        let mut s = base.clone();
        s.project.roof.kind = kind;
        let faces = build_faces(&s.project, &s.derived);
        let per_layer: HashMap<&str, usize> = faces_per_layer(&faces).into_iter().collect();
        let n = per_layer["A-ROOF"];
        assert!(n >= min_faces, "{kind:?} produced {n} roof faces");
        if kind == RoofKind::None {
            assert_eq!(n, 0);
        }
    }
}

#[test]
fn dxf3d_slab_layer_matches_the_footprints() {
    let s = fixture();
    let faces = build_faces(&s.project, &s.derived);
    let per_layer: HashMap<&str, usize> = faces_per_layer(&faces).into_iter().collect();
    assert!(!s.derived.footprints.is_empty());
    // A rectangular footprint prism: 2 caps of 2 triangles plus 4 sides.
    assert_eq!(per_layer["A-FLOR-SLAB"], 8 * s.derived.footprints.len());
}

#[test]
fn every_face_vertex_is_finite_and_inside_the_model_bounds() {
    for s in [fixture(), rich()] {
        let text = model_dxf3d(&s.project, &s.derived).unwrap();
        let drawing = parse_dxf(&text);
        // Generous bounds around the plan, from the walls plus the roof overhang.
        let mut min = (f64::INFINITY, f64::INFINITY);
        let mut max = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for e in &s.project.elements {
            if let Element::Wall(w) = e {
                for p in [w.start, w.end] {
                    min = (min.0.min(p.x), min.1.min(p.y));
                    max = (max.0.max(p.x), max.1.max(p.y));
                }
            }
        }
        let pad = 4000.0;
        let top = s
            .project
            .levels
            .iter()
            .map(|l| l.elevation_mm + l.height_mm)
            .fold(0.0f64, f64::max);
        let mut faces = 0;
        for e in drawing.entities() {
            let Some(pts) = face_points(e) else { continue };
            faces += 1;
            for (x, y, z) in pts {
                assert!(x.is_finite() && y.is_finite() && z.is_finite(), "{x} {y} {z}");
                assert!(x >= min.0 - pad && x <= max.0 + pad, "x {x} out of bounds");
                assert!(y >= min.1 - pad && y <= max.1 + pad, "y {y} out of bounds");
                assert!(z >= -1000.0 && z <= top + 6000.0, "z {z} out of bounds");
            }
        }
        assert!(faces > 40, "only {faces} faces");
    }
}

#[test]
fn wall_tops_sit_at_the_level_height() {
    let s = fixture();
    let level = &s.project.levels[0];
    let top = level.elevation_mm + level.height_mm;
    let text = model_dxf3d(&s.project, &s.derived).unwrap();
    let drawing = parse_dxf(&text);
    let mut highest: f64 = f64::NEG_INFINITY;
    let mut at_top = 0;
    for e in drawing.entities() {
        if e.common.layer != "A-WALL" {
            continue;
        }
        let Some(pts) = face_points(e) else { continue };
        for (_, _, z) in pts {
            highest = highest.max(z);
            if (z - top).abs() < 1e-6 {
                at_top += 1;
            }
        }
    }
    assert!((highest - top).abs() < 1e-6, "wall top is {highest}, want {top}");
    assert!(at_top > 8, "only {at_top} wall vertices reach the wall top");
}

#[test]
fn openings_cut_real_holes_in_the_wall_faces() {
    let s = fixture();
    // The head of the first window must show up as a wall vertex height.
    let win = s
        .project
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Opening(o) if o.opening_type == OpeningType::Window => Some(o.clone()),
            _ => None,
        })
        .unwrap();
    let head = win.sill_mm + win.height_mm;
    let text = model_dxf3d(&s.project, &s.derived).unwrap();
    let drawing = parse_dxf(&text);
    let mut heights: Vec<f64> = Vec::new();
    for e in drawing.entities() {
        if e.common.layer != "A-WALL" {
            continue;
        }
        let Some(pts) = face_points(e) else { continue };
        for (_, _, z) in pts {
            heights.push(z);
        }
    }
    assert!(
        heights.iter().any(|z| (z - win.sill_mm).abs() < 1e-6),
        "no wall face at the sill {}",
        win.sill_mm
    );
    assert!(
        heights.iter().any(|z| (z - head).abs() < 1e-6),
        "no wall face at the head {head}"
    );
}

#[test]
fn dxf3d_carries_the_2d_linework_at_z_zero() {
    let s = fixture();
    let text = model_dxf3d(&s.project, &s.derived).unwrap();
    let drawing = parse_dxf(&text);
    let mut flat = 0;
    let mut texts = 0;
    for e in drawing.entities() {
        match &e.specific {
            dxf::entities::EntityType::Line(l) => {
                assert_eq!(l.p1.z, 0.0);
                assert_eq!(l.p2.z, 0.0);
                flat += 1;
            }
            dxf::entities::EntityType::Polyline(_) => flat += 1,
            dxf::entities::EntityType::Arc(a) => {
                assert_eq!(a.center.z, 0.0);
                flat += 1;
            }
            dxf::entities::EntityType::Circle(c) => {
                assert_eq!(c.center.z, 0.0);
                flat += 1;
            }
            dxf::entities::EntityType::Text(t) => {
                assert_eq!(t.location.z, 0.0);
                texts += 1;
            }
            _ => {}
        }
    }
    assert!(flat > 5, "only {flat} flat entities");
    assert!(texts > 0, "room labels are missing");
}

#[test]
fn dxf3d_is_deterministic_and_sample_is_written() {
    let s = fixture();
    let a = model_dxf3d(&s.project, &s.derived).unwrap();
    let b = model_dxf3d(&s.project, &s.derived).unwrap();
    assert_eq!(a, b);
    let path = write_sample("bungalow-3d.dxf", &a);
    assert!(std::fs::metadata(&path).unwrap().len() > 10_000);
}

#[test]
fn dxf3d_header_reports_millimeters() {
    let s = fixture();
    let text = model_dxf3d(&s.project, &s.derived).unwrap();
    let drawing = parse_dxf(&text);
    assert_eq!(
        drawing.header.default_drawing_units,
        dxf::enums::Units::Millimeters,
        "$INSUNITS should read back as millimeters"
    );
}
