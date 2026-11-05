//! Lane L8, interop (DECISIONS D54): DXF 2013 with real dimensions and
//! blocks, IFC with types and quantities. The lane grows this file.

use guhit_app::AppService;
use guhit_model::*;
use serde_json::{json, Value};

fn app() -> (AppService, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    (AppService::new_sandboxed(dir.path().to_path_buf()), dir)
}

#[allow(dead_code)]
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

#[tokio::test]
async fn the_migrated_roof_still_exports_to_ifc() {
    let (app, _dir) = app();
    let state = bungalow(&app).await;
    let ifc = guhit_export::model_ifc(&state.project, &state.derived).unwrap();
    assert_eq!(ifc.matches("IFCROOF(").count(), 1);
    assert!(ifc.contains(".GABLE_ROOF."));
}

fn plan_options() -> PlanExportOptions {
    PlanExportOptions {
        level_id: None,
        paper: PaperSize::A3,
        orientation: Orientation::Landscape,
        scale_denominator: None,
        show_dimensions: true,
        show_room_labels: true,
        show_assets: true,
        title_block: false,
        show_pipes: true,
        sheet: SheetKind::Plan,
        review_page: false,
        date: None,
    }
}

/// A wall as (start, end, thickness), ends in a stable order.
fn walls_of(state: &DocState) -> Vec<(Point, Point, f64)> {
    let mut out: Vec<(Point, Point, f64)> = state
        .project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Wall(w) => {
                let (a, b) = if (w.start.x, w.start.y) <= (w.end.x, w.end.y) { (w.start, w.end) } else { (w.end, w.start) };
                Some((a, b, w.thickness_mm))
            }
            _ => None,
        })
        .collect();
    out.sort_by(|a, b| (a.0.x, a.0.y, a.1.x, a.1.y).partial_cmp(&(b.0.x, b.0.y, b.1.x, b.1.y)).unwrap());
    out
}

/// Where the files of the live engine's projects go for the ezdxf and
/// IfcOpenShell checks.
fn samples() -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/export-samples/l8");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn our_own_import_reads_the_dxf_2013_back_to_the_same_walls() {
    for template in ["sample-bungalow", "plumbing-demo"] {
        round_trip(template).await;
    }
}

/// Export a template's plan as DXF 2013, import it into a blank project,
/// and compare the walls. Writes the plan of every level, all levels side
/// by side and the IFC on the way.
async fn round_trip(template: &str) {
    let (app, _dir) = app();
    let original: DocState =
        serde_json::from_value(call(&app, "hub_create", json!({ "name": "Bahay", "template": template })).await).unwrap();
    for level in &original.project.levels {
        let opts = PlanExportOptions { level_id: Some(level.id.clone()), ..plan_options() };
        let out = guhit_export::dxf::plan(&original.project, &original.derived, &opts).unwrap();
        std::fs::write(samples().join(format!("app-{template}-{}.dxf", level.name.to_lowercase().replace(' ', "-"))), out.data).unwrap();
    }
    let all = guhit_export::dxf::plans(&original.project, &original.derived, &plan_options()).unwrap();
    std::fs::write(samples().join(format!("app-{template}-plans.dxf")), all.data).unwrap();
    let ifc = guhit_export::model_ifc(&original.project, &original.derived).unwrap();
    std::fs::write(samples().join(format!("app-{template}.ifc")), ifc).unwrap();

    let dxf = guhit_export::dxf::plan(&original.project, &original.derived, &plan_options()).unwrap().data;
    assert!(dxf.contains("AC1027"));
    let data = format!("data:application/dxf;base64,{}", base64_encode(dxf.as_bytes()));

    let fresh: DocState = serde_json::from_value(call(&app, "hub_create", json!({ "name": "Imported" })).await).unwrap();
    let inspection: ImportInspection =
        serde_json::from_value(call(&app, "import_inspect", json!({ "file_name": "bahay.dxf", "data": data })).await).unwrap();
    assert_eq!(inspection.declared_unit.as_deref(), Some("mm"));
    assert_eq!(inspection.suggested_mm_per_unit, 1.0);
    let result: ImportResult = serde_json::from_value(
        call(
            &app,
            "import_commit",
            json!({
                "file_name": "bahay.dxf",
                "data": data,
                "options": {
                    "mm_per_unit": 1.0,
                    "layers": ["A-WALL"],
                    "mode": "walls",
                    "offset": { "x": 0.0, "y": 0.0 },
                    "level_id": fresh.project.levels[0].id,
                }
            }),
        )
        .await,
    )
    .unwrap();
    // A DXF plan draws walls broken at doors and windows, as CAD plans do,
    // and the import reads each solid stretch as a wall. So every wall
    // comes back as its stretches between openings: on its centerline, at
    // its thickness, ending at the corners and at the jambs, within 1 mm.
    let got = walls_of(&result.state);
    let mut matched = 0;
    for e in &original.project.elements {
        let Element::Wall(w) = e else { continue };
        let (sx, sy) = (w.start.x, w.start.y);
        let len = (w.end.x - sx).hypot(w.end.y - sy);
        let (ux, uy) = ((w.end.x - sx) / len, (w.end.y - sy) / len);
        let along = |p: &Point| (p.x - sx) * ux + (p.y - sy) * uy;
        let off = |p: &Point| ((p.x - sx) * uy - (p.y - sy) * ux).abs();
        // The solid stretches: the wall less its openings.
        let mut cuts: Vec<(f64, f64)> = original
            .project
            .elements
            .iter()
            .filter_map(|e| match e {
                Element::Opening(o) if o.wall_id == w.id => Some((o.offset_mm - o.width_mm / 2.0, o.offset_mm + o.width_mm / 2.0)),
                _ => None,
            })
            .collect();
        cuts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let mut want: Vec<(f64, f64)> = Vec::new();
        let mut at = 0.0;
        for (a, b) in cuts {
            want.push((at, a));
            at = b;
        }
        want.push((at, len));
        let mut have: Vec<(f64, f64)> = got
            .iter()
            .filter(|(a, b, t)| off(a) < 1.0 && off(b) < 1.0 && (t - w.thickness_mm).abs() < 0.5)
            .map(|(a, b, _)| {
                let (p, q) = (along(a), along(b));
                (p.min(q), p.max(q))
            })
            .filter(|(p, q)| *q > -1.0 && *p < len + 1.0)
            .collect();
        have.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        // Jambs match within 1 mm. A wall end that butts into another wall
        // (a T) is drawn to that wall's face, so it may come back up to
        // half that wall's thickness short.
        let end_ok = |want: f64, got: f64, inward: f64| (want - got).abs() < 1.0 || (0.0..=100.0).contains(&(inward * (got - want)));
        let same = |x: &(f64, f64), y: &(f64, f64)| {
            let start = if x.0.abs() < 1e-6 { end_ok(x.0, y.0, 1.0) } else { (x.0 - y.0).abs() < 1.0 };
            let end = if (x.1 - len).abs() < 1e-6 { end_ok(x.1, y.1, -1.0) } else { (x.1 - y.1).abs() < 1.0 };
            start && end
        };
        // Every stretch that came back is one that was drawn, and every
        // stretch longer than twice the wall is thick came back: the import
        // reads a stub beside a jamb as a column, not a wall.
        for h in &have {
            assert!(want.iter().any(|x| same(x, h)), "wall {}: {have:?} for {want:?}", w.id);
        }
        for x in want.iter().filter(|(p, q)| q - p > 2.0 * w.thickness_mm) {
            assert!(have.iter().any(|h| same(x, h)), "wall {}: {have:?} for {want:?}", w.id);
        }
        matched += have.len();
    }
    assert_eq!(matched, got.len(), "no wall came back that was not drawn: {got:?}");
    // Blocks, dimensions and labels are left as the import always leaves
    // them, and said so.
    if original.project.elements.iter().any(|e| matches!(e, Element::Dimension(_))) {
        assert!(result.skipped.iter().any(|s| s.contains("DIMENSION")), "{:?}", result.skipped);
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}
