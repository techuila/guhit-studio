//! Lane L3, views (DECISIONS D54): elevations and sections with hidden
//! lines removed, the roof, site and reflected ceiling plans, the vicinity
//! frame, through `view_svg` and with the engine's own `Derived`.
//!
//! `write_fresh_states` writes the sample bungalow, the plumbing demo and
//! the Wave 1 Lovell house, each with a freshly derived `Derived`, to
//! `$GUHIT_L3_STATES` when it is set; the export crate's L3 tests draw
//! their samples and time their views from those.

use guhit_app::AppService;
use guhit_core::Document;
use guhit_export::plan::Cat;
use guhit_export::views;
use guhit_model::*;
use serde_json::{json, Value};

fn app() -> (AppService, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    (AppService::new_sandboxed(dir.path().to_path_buf()), dir)
}

async fn call(app: &AppService, cmd: &str, args: Value) -> Value {
    app.handle(cmd, args).await.unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

#[allow(dead_code)]
async fn fail(app: &AppService, cmd: &str, args: Value) -> IpcError {
    match app.handle(cmd, args).await {
        Ok(v) => panic!("{cmd} should have failed, got {v}"),
        Err(e) => e,
    }
}

async fn bungalow(app: &AppService) -> DocState {
    serde_json::from_value(call(app, "hub_create", json!({ "name": "Bahay", "template": "sample-bungalow" })).await).unwrap()
}

fn data(name: &str) -> Project {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../guhit-core/tests/data").join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Bounds of a level's footprints.
fn footprint_box(d: &Derived) -> (f64, f64, f64, f64) {
    let mut b = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for f in &d.footprints {
        for q in &f.polygon {
            b = (b.0.min(q.x), b.1.min(q.y), b.2.max(q.x), b.3.max(q.y));
        }
    }
    b
}

/// A south elevation mark (looking north) 3 m in front of the buildings.
fn south_mark(p: &Project, d: &Derived) -> ViewMark {
    let b = footprint_box(d);
    ViewMark {
        id: "mark-south".into(),
        level_id: p.levels[0].id.clone(),
        mark_kind: MarkKind::Elevation,
        label: "South".into(),
        start: Point { x: b.0 - 3000.0, y: b.1 - 3000.0 },
        end: Point { x: b.2 + 3000.0, y: b.1 - 3000.0 },
        flip: false,
        depth_mm: None,
    }
}

#[tokio::test]
async fn views_are_wired_through_view_svg() {
    let (app, _dir) = app();
    let state = bungalow(&app).await;
    let level = state.project.levels[0].id.clone();
    for view in [
        ViewRef::Plan { level_id: level, sheet: SheetKind::ReflectedCeiling },
        ViewRef::RoofPlan,
        ViewRef::SitePlan,
        ViewRef::VicinityMap { file_name: None, radius_m: 500.0 },
    ] {
        let out = call(&app, "view_svg", json!({ "view": view, "scale": 100 })).await;
        let svg = out["svg"].as_str().unwrap();
        assert!(svg.starts_with("<?xml") && svg.len() > 500, "{view:?}");
        assert_eq!(out["scale_denominator"], 100);
    }
    // An elevation of a mark the project does not have is refused.
    let e = fail(&app, "view_svg", json!({ "view": ViewRef::Elevation { mark_id: "nope".into() } })).await;
    assert_ne!(e.code, "not_built");
}

/// Stability: a migrated schema 5 project's elevation spans exactly its
/// plan footprint, its walls from the floor to the wall top, and the roof
/// from `Derived::roofs` sits over them.
#[test]
fn a_migrated_schema_5_elevation_matches_its_footprint() {
    for name in ["schema5-sample-bungalow.project.json", "schema5-plumbing-demo.project.json"] {
        let doc = Document::new(data(name));
        let (p, d) = (doc.project(), doc.derived());
        assert_eq!(p.schema_version, SCHEMA_VERSION, "{name} migrated");
        assert!(!d.roofs.is_empty(), "{name} has its roof as an element");
        let mut p2 = p.clone();
        p2.elements.push(Element::ViewMark(south_mark(p, d)));
        let v = views::items(&p2, d, &ViewRef::Elevation { mark_id: "mark-south".into() }, Some(100)).unwrap();
        let mut walls = guhit_export::geom::Bounds::empty();
        // Walls and the columns at their corners.
        for it in v.items.iter().filter(|i| matches!(i.cat, Cat::Wall | Cat::Cols)) {
            if let guhit_export::plan::Prim::Line { a, b } = &it.prim {
                walls.add(*a);
                walls.add(*b);
            }
        }
        let fb = footprint_box(d);
        // Looking north, x runs east from the mark's start.
        let x0 = fb.0 - (fb.0 - 3000.0);
        let x1 = fb.2 - (fb.0 - 3000.0);
        // The plumbing demo's corner columns stand 25 mm proud of the walls.
        let ok = |got: f64, want: f64, out: f64| (got - want) * out > -1.0 && (got - want) * out < 30.0;
        assert!(ok(walls.min.x, x0, -1.0) && ok(walls.max.x, x1, 1.0), "{name}: walls {:?} vs footprint {x0}..{x1}", walls);
        let level = &p.levels[0];
        assert!((walls.min.y - level.elevation_mm).abs() < 1.0, "{name}: walls start at the floor");
        let top_roof = v
            .items
            .iter()
            .filter(|i| i.cat == Cat::Roof)
            .filter_map(|i| match &i.prim {
                guhit_export::plan::Prim::Line { a, b } => Some(a.y.max(b.y)),
                _ => None,
            })
            .fold(f64::NEG_INFINITY, f64::max);
        let derived_top = d.roofs.iter().flat_map(|g| g.planes.iter().flatten()).map(|q| q.z + level.elevation_mm).fold(f64::NEG_INFINITY, f64::max);
        assert!((top_roof - derived_top).abs() < 1.0, "{name}: roof top {top_roof} vs derived {derived_top}");
    }
}

/// Fresh states for the export crate's samples and timing.
#[test]
fn write_fresh_states() {
    let Ok(dir) = std::env::var("GUHIT_L3_STATES") else { return };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let fixture = |name: &str| -> Project {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(name);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        serde_json::from_value(v["project"].clone()).unwrap()
    };
    for (name, project) in [
        ("bungalow", fixture("sample-bungalow.docstate.json")),
        ("plumbing", fixture("plumbing-demo.docstate.json")),
        ("lovell", data("wave1-g5-lovell.project.json")),
    ] {
        let doc = Document::new(project);
        let out = json!({ "project": doc.project(), "derived": doc.derived() });
        std::fs::write(dir.join(format!("{name}.json")), serde_json::to_string(&out).unwrap()).unwrap();
    }
}
