//! The Wave 2 contract through MCP (DECISIONS D54, P0-2): `set_project`
//! builds its roof edit from `Project::top_roof`, so "pitch 25" on a
//! migrated project changes the pitch and never deletes the roof.

use guhit_app::AppService;
use guhit_mcp::tools::{self, Output};
use guhit_model::*;
use serde_json::{json, Value};

fn app() -> (AppService, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    (AppService::new_sandboxed(dir.path().to_path_buf()), dir)
}

async fn ok(app: &AppService, name: &str, args: Value) -> Value {
    match tools::call(app, name, args).await {
        Ok(Output::Json(v)) => v,
        Ok(Output::Rich { json, .. }) => json,
        Ok(Output::Text(t)) => json!({ "text": t }),
        Err(f) => panic!("`{name}` failed: {}", f.display()),
    }
}

fn project(app: &AppService) -> Project {
    let s = app.session.try_lock().expect("session free");
    s.doc.as_ref().unwrap().project().clone()
}

#[tokio::test]
async fn set_project_pitch_keeps_the_migrated_roof() {
    let (app, _dir) = app();
    ok(&app, "project", json!({"action": "create", "name": "Bahay", "template": "sample-bungalow"})).await;
    let before = project(&app);
    assert_eq!(before.roof.kind, RoofKind::None, "migrated");
    assert_eq!(before.top_roof().kind, RoofKind::Gable);
    ok(&app, "set_project", json!({"roof": {"pitch_deg": 25}})).await;
    let after = project(&app);
    let roof = after.top_roof();
    assert_eq!((roof.kind, roof.pitch_deg), (RoofKind::Gable, 25.0));
    assert_eq!(after.top_roof_area().map(|r| r.id.clone()), before.top_roof_area().map(|r| r.id.clone()));
    // Kind none still removes it.
    ok(&app, "set_project", json!({"roof": {"kind": "none"}})).await;
    assert!(project(&app).top_roof_area().is_none());
}
