//! Lane L7, MCP and agents (DECISIONS D53, D54): the agent side of the
//! Wave 2 contract. The MCP tools themselves are tested in `guhit-mcp`. The
//! lane grows this file.

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
async fn a_roofs_only_agent_can_edit_the_roof_over_the_top_floor() {
    use guhit_app::agents::{AgentHint, CallKind};
    let (app, _dir) = app();
    let state = bungalow(&app).await;
    let hint = AgentHint { agent: Some("roofer".into()), client_name: Some("codex".into()), ..AgentHint::default() };
    let agent = app.agent_call(&hint, "set_project", CallKind::Edit).unwrap();
    let limit = AgentLimit { level_ids: vec![], ids: vec![], kinds: vec![ElementKind::Roof] };
    call(&app, "agent_update", json!({ "id": agent.id, "limit": limit })).await;
    let roof = Roof { pitch_deg: 30.0, ..state.project.top_roof() };
    let r = app.agent_commit(&agent, Command::SetRoof { roof }).await.unwrap();
    assert_eq!(r.state.project.top_roof().pitch_deg, 30.0);
    // A wall is outside its limit.
    let wall = Command::AddWall {
        start: Point { x: 0.0, y: -3000.0 },
        end: Point { x: 3000.0, y: -3000.0 },
        thickness_mm: None,
        height_mm: None,
        material_id: None,
        level_id: None,
        role: None,
    };
    assert_eq!(app.agent_commit(&agent, wall).await.unwrap_err().code, "out_of_scope");
}
