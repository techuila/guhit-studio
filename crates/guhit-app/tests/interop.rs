//! End to end tests of the interchange commands against a temp data dir:
//! DXF import, reference models, the `.guhit` bundle and the DWG converter.

use std::io::Write;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use guhit_app::AppService;
use guhit_model::*;
use serde_json::{json, Value};

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{tag}-{}", defaults::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn call<T: serde::de::DeserializeOwned>(app: &AppService, cmd: &str, args: Value) -> T {
    let v = app
        .handle(cmd, args)
        .await
        .unwrap_or_else(|e| panic!("{cmd} failed: {e}"));
    serde_json::from_value(v).unwrap_or_else(|e| panic!("{cmd} returned an unexpected shape: {e}"))
}

async fn fail(app: &AppService, cmd: &str, args: Value) -> IpcError {
    match app.handle(cmd, args).await {
        Ok(v) => panic!("{cmd} should have failed, got {v}"),
        Err(e) => e,
    }
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", B64.encode(bytes))
}

// ---------------------------------------------------------------- fixtures

/// Two rooms drawn as double lines, 150 mm walls, in millimeters.
/// Centerlines: an 8000 x 5000 rectangle with a divider at x = 4000.
fn two_room_dxf() -> String {
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64| {
        let pts = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)];
        let mut s = String::from("  0\nLWPOLYLINE\n  8\nA-WALL\n 90\n4\n 70\n1\n");
        for (x, y) in pts {
            s.push_str(&format!(" 10\n{x}\n 20\n{y}\n"));
        }
        s
    };
    format!(
        "  0\nSECTION\n  2\nHEADER\n  9\n$ACADVER\n  1\nAC1009\n  9\n$INSUNITS\n 70\n4\n  0\nENDSEC\n\
           0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n 70\n2\n\
           0\nLAYER\n  2\n0\n 70\n0\n 62\n7\n  6\nCONTINUOUS\n\
           0\nLAYER\n  2\nA-WALL\n 70\n0\n 62\n7\n  6\nCONTINUOUS\n\
           0\nENDTAB\n  0\nENDSEC\n\
           0\nSECTION\n  2\nBLOCKS\n  0\nENDSEC\n\
           0\nSECTION\n  2\nENTITIES\n{}{}{}  0\nENDSEC\n  0\nEOF\n",
        rect(-75.0, -75.0, 8075.0, 5075.0),
        rect(75.0, 75.0, 3925.0, 4925.0),
        rect(4075.0, 75.0, 7925.0, 4925.0),
    )
}

fn import_options(mode: &str) -> Value {
    json!({
        "mm_per_unit": 1.0,
        "layers": [],
        "mode": mode,
        "offset": { "x": 0.0, "y": 0.0 },
        "level_id": null
    })
}

async fn blank_project(app: &AppService, name: &str) -> DocState {
    call(app, "hub_create", json!({ "name": name })).await
}

// ------------------------------------------------------------------ import

#[tokio::test]
async fn inspect_then_commit_a_dxf_makes_walls_and_rooms() {
    let dir = TempDir::new("import");
    let app = AppService::new(dir.path().to_path_buf());
    blank_project(&app, "Reyes Residence").await;

    let dxf = two_room_dxf();
    let source = json!({ "file_name": "site.dxf", "data": data_url("application/dxf", dxf.as_bytes()) });

    let inspection: ImportInspection = call(&app, "import_inspect", source.clone()).await;
    assert_eq!(inspection.file_name, "site.dxf");
    assert_eq!(inspection.declared_unit.as_deref(), Some("mm"));
    assert_eq!(inspection.suggested_mm_per_unit, 1.0);
    let wall_layer = inspection.layers.iter().find(|l| l.name == "A-WALL").unwrap();
    assert_eq!(wall_layer.detected_walls, 7);
    assert_eq!(inspection.min, Point { x: -75.0, y: -75.0 });
    assert_eq!(inspection.max, Point { x: 8075.0, y: 5075.0 });

    let mut args = source.clone();
    args["options"] = import_options("walls");
    let result: ImportResult = call(&app, "import_commit", args).await;
    assert_eq!(result.walls_added, 7);
    assert_eq!(result.linework_added, 0);

    let walls = result
        .state
        .project
        .elements
        .iter()
        .filter(|e| matches!(e, Element::Wall(_)))
        .count();
    assert_eq!(walls, 7, "seven walls landed in the model");
    assert_eq!(result.state.derived.rooms.len(), 2, "the engine closed both rooms");
    let areas: Vec<f64> = result
        .state
        .derived
        .rooms
        .iter()
        .map(|r| (r.area_mm2 / 1e6 * 100.0).round() / 100.0)
        .collect();
    // Each room: 3850 x 4850 net = 18.67 m2.
    assert!(
        areas.iter().all(|a| (*a - 18.67).abs() < 0.05),
        "net areas come out right: {areas:?}"
    );
    assert_eq!(result.state.undo_label.as_deref(), Some("Import site.dxf"));

    // One undo takes the whole import back.
    let after: DocState = call(&app, "doc_undo", json!({})).await;
    assert!(after.project.elements.iter().all(|e| !matches!(e, Element::Wall(_))));
}

#[tokio::test]
async fn linework_mode_locks_everything_onto_the_underlays_layer() {
    let dir = TempDir::new("import-linework");
    let app = AppService::new(dir.path().to_path_buf());
    blank_project(&app, "Trace").await;

    let dxf = two_room_dxf();
    let mut args = json!({ "file_name": "site.dxf", "data": data_url("application/dxf", dxf.as_bytes()) });
    args["options"] = import_options("linework");
    let result: ImportResult = call(&app, "import_commit", args).await;
    assert_eq!(result.walls_added, 0);
    assert_eq!(result.linework_added, 1);
    let work: Vec<&Linework> = result
        .state
        .project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Linework(l) => Some(l),
            _ => None,
        })
        .collect();
    assert_eq!(work.len(), 1);
    assert_eq!(work[0].name, "site.dxf / A-WALL");
    assert!(work[0].locked);
    assert!(!work[0].level_id.is_empty(), "the app fills in the level");
}

#[tokio::test]
async fn a_file_that_is_not_a_drawing_is_refused() {
    let dir = TempDir::new("import-bad");
    let app = AppService::new(dir.path().to_path_buf());
    blank_project(&app, "Bad").await;

    let e = fail(
        &app,
        "import_inspect",
        json!({ "file_name": "notes.txt", "data": data_url("text/plain", b"hello") }),
    )
    .await;
    assert_eq!(e.code, "invalid");
    assert!(e.message.contains("supported file"), "{}", e.message);

    let e = fail(
        &app,
        "import_inspect",
        json!({ "file_name": "notes.dxf", "data": data_url("application/dxf", b"hello there, not a drawing at all") }),
    )
    .await;
    assert_eq!(e.code, "invalid");
    assert!(e.message.contains("not a DXF file"), "{}", e.message);
}

// --------------------------------------------------------- reference models

#[tokio::test]
async fn reference_models_are_stored_and_served_back() {
    let dir = TempDir::new("models");
    let app = AppService::new(dir.path().to_path_buf());
    blank_project(&app, "Site").await;

    let glb = b"glTF\x02\x00\x00\x00 pretend binary model";
    let stored: Value = call(
        &app,
        "model_store",
        json!({ "file_name": "massing.glb", "data": data_url("model/gltf-binary", glb) }),
    )
    .await;
    assert_eq!(stored["file_name"], "massing.glb");
    assert_eq!(stored["size"], glb.len());

    // Never overwrite: the same name comes back with a suffix.
    let again: Value = call(
        &app,
        "model_store",
        json!({ "file_name": "massing.glb", "data": data_url("model/gltf-binary", glb) }),
    )
    .await;
    assert_eq!(again["file_name"], "massing-2.glb");

    let url: String = call(&app, "model_data", json!({ "file_name": "massing.glb" })).await;
    assert!(url.starts_with("data:model/gltf-binary;base64,"), "{url}");
    let bytes = B64.decode(url.split_once(',').unwrap().1).unwrap();
    assert_eq!(bytes, glb);

    let e = fail(&app, "model_data", json!({ "file_name": "missing.obj" })).await;
    assert_eq!(e.code, "not_found");
}

#[tokio::test]
async fn model_store_refuses_anything_that_is_not_a_model() {
    let dir = TempDir::new("models-bad");
    let app = AppService::new(dir.path().to_path_buf());
    blank_project(&app, "Site").await;

    for name in ["installer.exe", "script.sh", "plan.dxf", "photo.png"] {
        let e = fail(
            &app,
            "model_store",
            json!({ "file_name": name, "data": data_url("application/octet-stream", b"MZ\x90\x00") }),
        )
        .await;
        assert_eq!(e.code, "invalid", "{name}");
        assert!(e.message.contains("glb, gltf, obj"), "{name}: {}", e.message);
    }
    // A path that tries to climb out is rewritten to a plain name first, and
    // then fails on the extension.
    let e = fail(
        &app,
        "model_store",
        json!({ "file_name": "../../evil.exe", "data": data_url("application/octet-stream", b"MZ") }),
    )
    .await;
    assert_eq!(e.code, "invalid");
    assert!(!dir.path().join("evil.exe").exists());
}

// ------------------------------------------------------------ .guhit bundle

async fn bundled_project(app: &AppService, name: &str) -> String {
    blank_project(app, name).await;
    let dxf = two_room_dxf();
    let mut args = json!({ "file_name": "site.dxf", "data": data_url("application/dxf", dxf.as_bytes()) });
    args["options"] = import_options("walls");
    let _: ImportResult = call(app, "import_commit", args).await;
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
    let state: DocState = call(app, "doc_state", json!({})).await;
    let id = state.project.id.clone();
    let _: Value = call(app, "hub_set_thumbnail", json!({ "id": id, "png": png })).await;
    let _: SnapshotMeta = call(app, "snapshot_create", json!({ "label": "before the client meeting" })).await;
    let _: Value = call(
        app,
        "model_store",
        json!({ "file_name": "tree.obj", "data": data_url("model/obj", b"v 0 0 0\nv 1 0 0\n") }),
    )
    .await;
    id
}

fn file_map(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|e| e.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(root, &p, out);
            } else if p.is_file() {
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                out.push((rel, std::fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = vec![];
    walk(dir, dir, &mut out);
    out
}

#[tokio::test]
async fn a_bundle_round_trips_into_a_fresh_service() {
    let source_dir = TempDir::new("bundle-src");
    let source = AppService::new(source_dir.path().to_path_buf());
    let id = bundled_project(&source, "Reyes Residence").await;

    let saved: ExportResult = call(&source, "bundle_save", json!({})).await;
    assert!(saved.path.ends_with(".guhit"), "{}", saved.path);
    let bytes = std::fs::read(&saved.path).unwrap();
    assert!(!bytes.is_empty());

    // A fresh service, a fresh data dir: the bundle is all it gets.
    let target_dir = TempDir::new("bundle-dst");
    let target = AppService::new(target_dir.path().to_path_buf());
    let opened: DocState = call(
        &target,
        "bundle_open",
        json!({ "file_name": "reyes.guhit", "data": data_url("application/zip", &bytes) }),
    )
    .await;
    assert_eq!(opened.project.id, id, "a free id is kept");
    assert_eq!(opened.project.name, "Reyes Residence");
    assert_eq!(opened.derived.rooms.len(), 2, "the imported plan came along");

    let from = file_map(&source_dir.path().join("projects").join(&id));
    let to = file_map(&target_dir.path().join("projects").join(&id));
    let names: Vec<&String> = to.iter().map(|(n, _)| n).collect();
    assert!(names.iter().any(|n| n.ends_with("project.json")));
    assert!(names.iter().any(|n| n.ends_with("thumbnail.png")));
    assert!(names.iter().any(|n| n.contains("snapshots")));
    assert!(names.iter().any(|n| n.contains("models")));
    assert_eq!(from, to, "every file came across byte for byte");

    // The project hub sees it, and re-opening keeps the same id.
    let list: Vec<ProjectMeta> = call(&target, "hub_list", json!({})).await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, id);
}

#[tokio::test]
async fn opening_a_bundle_over_a_different_project_makes_a_new_one() {
    let dir = TempDir::new("bundle-clash");
    let app = AppService::new(dir.path().to_path_buf());
    let id = bundled_project(&app, "Reyes Residence").await;
    let saved: ExportResult = call(&app, "bundle_save", json!({})).await;
    let bytes = std::fs::read(&saved.path).unwrap();
    let source = json!({ "file_name": "reyes.guhit", "data": data_url("application/zip", &bytes) });

    // Same bytes, same project: nothing new is made.
    let again: DocState = call(&app, "bundle_open", source.clone()).await;
    assert_eq!(again.project.id, id);
    assert_eq!(again.project.name, "Reyes Residence");

    // Now the project on disk moves on, so the bundle is a different plan
    // wearing the same id.
    let _: ProjectMeta = call(&app, "hub_rename", json!({ "id": id, "name": "Reyes Residence rev B" })).await;
    let opened: DocState = call(&app, "bundle_open", source).await;
    assert_ne!(opened.project.id, id, "a fresh id");
    assert_eq!(opened.project.name, "Reyes Residence (imported)");
    let list: Vec<ProjectMeta> = call(&app, "hub_list", json!({})).await;
    assert_eq!(list.len(), 2);
}

#[tokio::test]
async fn a_bundle_cannot_write_outside_the_project_folder() {
    let dir = TempDir::new("zip-slip");
    let app = AppService::new(dir.path().to_path_buf());
    blank_project(&app, "Victim").await;

    let project = json!({
        "schema_version": 1, "id": "11111111-1111-4111-8111-111111111111", "name": "Evil",
        "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z",
        "settings": serde_json::to_value(defaults::new_project("x").settings).unwrap(),
        "levels": serde_json::to_value(defaults::new_project("x").levels).unwrap(),
        "layers": serde_json::to_value(defaults::new_project("x").layers).unwrap(),
        "materials": serde_json::to_value(defaults::new_project("x").materials).unwrap(),
        "elements": [],
        "roof": serde_json::to_value(defaults::new_project("x").roof).unwrap(),
    });

    for evil in ["../../escaped.txt", "/tmp/escaped.txt", "a/../../escaped.txt"] {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("project.json", opts).unwrap();
        w.write_all(serde_json::to_string(&project).unwrap().as_bytes()).unwrap();
        w.start_file(evil.to_string(), opts).unwrap();
        w.write_all(b"owned").unwrap();
        let bytes = w.finish().unwrap().into_inner();

        let e = fail(
            &app,
            "bundle_open",
            json!({ "file_name": "evil.guhit", "data": data_url("application/zip", &bytes) }),
        )
        .await;
        assert_eq!(e.code, "invalid", "{evil}: {}", e.message);
        assert!(
            e.message.contains("outside") || e.message.contains("safe file name"),
            "{evil}: {}",
            e.message
        );
    }
    assert!(!dir.path().join("escaped.txt").exists());
    assert!(!dir.path().parent().unwrap().join("escaped.txt").exists());
}

#[tokio::test]
async fn a_bundle_that_is_not_a_bundle_is_refused() {
    let dir = TempDir::new("bundle-bad");
    let app = AppService::new(dir.path().to_path_buf());
    blank_project(&app, "Victim").await;

    let e = fail(
        &app,
        "bundle_open",
        json!({ "file_name": "notes.guhit", "data": data_url("application/zip", b"not a zip at all") }),
    )
    .await;
    assert_eq!(e.code, "invalid");

    // A real zip with no project.json in it.
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    w.start_file("readme.txt", zip::write::SimpleFileOptions::default()).unwrap();
    w.write_all(b"hello").unwrap();
    let bytes = w.finish().unwrap().into_inner();
    let e = fail(
        &app,
        "bundle_open",
        json!({ "file_name": "empty.guhit", "data": data_url("application/zip", &bytes) }),
    )
    .await;
    assert!(e.message.contains("project.json"), "{}", e.message);
}

// -------------------------------------------------------------------- DWG

#[tokio::test]
async fn dwg_reports_that_it_is_not_set_up_yet() {
    let dir = TempDir::new("dwg");
    let app = AppService::new(dir.path().to_path_buf());
    blank_project(&app, "Plan").await;

    let status: DwgConverterStatus = call(&app, "dwg_status", json!({})).await;
    assert!(!status.configured);
    assert!(!status.works);
    assert_eq!(status.path, None);
    assert!(status.message.contains("opendesign.com"), "{}", status.message);

    // A DWG cannot be imported until the converter is there.
    let e = fail(
        &app,
        "import_inspect",
        json!({ "file_name": "site.dwg", "data": data_url("application/octet-stream", b"AC1032 pretend dwg") }),
    )
    .await;
    assert_eq!(e.code, "invalid");
    assert!(e.message.contains("ODA File Converter"), "{}", e.message);

    // And neither can a DWG be written.
    let e = fail(&app, "export_model", json!({ "format": "dwg" })).await;
    assert_eq!(e.code, "invalid");
    assert!(e.message.contains("ODA File Converter"), "{}", e.message);

    // A path that is not a program is refused, and nothing is stored.
    let bogus = dir.path().join("not-a-converter");
    std::fs::write(&bogus, b"#!/bin/sh\n").unwrap();
    let e = fail(&app, "dwg_set_path", json!({ "path": "relative/path" })).await;
    assert_eq!(e.code, "invalid");
    let e = fail(
        &app,
        "dwg_set_path",
        json!({ "path": dir.path().join("nothing-here").to_string_lossy() }),
    )
    .await;
    assert_eq!(e.code, "not_found");
    let status: DwgConverterStatus = call(&app, "dwg_status", json!({})).await;
    assert!(!status.configured, "a failed attempt changes nothing");
}

#[tokio::test]
async fn setting_and_clearing_the_converter_path_works() {
    let dir = TempDir::new("dwg-set");
    let app = AppService::new(dir.path().to_path_buf());

    // Stand in for the converter: any program that starts and exits.
    let fake = dir.path().join("ODAFileConverter");
    std::fs::write(&fake, b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    if std::process::Command::new(&fake).spawn().is_err() {
        // Windows cannot run a shell script. The rest of the check is unix only.
        return;
    }

    let status: DwgConverterStatus = call(
        &app,
        "dwg_set_path",
        json!({ "path": fake.to_string_lossy() }),
    )
    .await;
    assert!(status.configured && status.works, "{}", status.message);
    assert_eq!(status.path.as_deref(), Some(fake.to_string_lossy().as_ref()));

    let settings: Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["dwg_converter_path"], fake.to_string_lossy().as_ref());

    let cleared: DwgConverterStatus = call(&app, "dwg_set_path", json!({ "path": "" })).await;
    assert!(!cleared.configured);
}

// -------------------------------------------------------------- model export

#[tokio::test]
async fn ifc_and_3d_dxf_are_written_to_the_exports_folder() {
    let dir = TempDir::new("model-export");
    let app = AppService::new(dir.path().to_path_buf());
    bundled_project(&app, "Reyes Residence").await;

    let ifc: ExportResult = call(&app, "export_model", json!({ "format": "ifc" })).await;
    assert!(ifc.path.ends_with(".ifc"), "{}", ifc.path);
    let text = std::fs::read_to_string(&ifc.path).unwrap();
    assert!(text.starts_with("ISO-10303-21;"), "an IFC STEP file");

    let dxf3d: ExportResult = call(&app, "export_model", json!({ "format": "dxf3d" })).await;
    assert!(dxf3d.path.ends_with(".dxf"), "{}", dxf3d.path);
    assert!(std::fs::read_to_string(&dxf3d.path).unwrap().contains("3DFACE"));

    let e = fail(&app, "export_model", json!({ "format": "collada" })).await;
    assert_eq!(e.code, "bad_args");
}
