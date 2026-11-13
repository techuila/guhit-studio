//! The Wave 2 contract at the app (DECISIONS D53, D54): the copilot's
//! set_roof on a migrated project (P0-2), only people set license numbers
//! (P1-4), and every new IPC command is routed (lanes answer `not_built`
//! until they build it). Each lane has its own `w2_l*.rs` file.

use guhit_app::agents::{AgentHint, CallKind};
use guhit_app::AppService;
use guhit_model::*;
use serde_json::{json, Value};

fn app() -> (AppService, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    (AppService::new_sandboxed(dir.path().to_path_buf()), dir)
}

async fn call(app: &AppService, cmd: &str, args: Value) -> Value {
    app.handle(cmd, args).await.unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

async fn bungalow(app: &AppService) -> DocState {
    serde_json::from_value(call(app, "hub_create", json!({ "name": "Bahay", "template": "sample-bungalow" })).await).unwrap()
}

#[tokio::test]
async fn the_copilot_set_roof_changes_the_pitch_and_keeps_the_roof() {
    let (app, _dir) = app();
    let state = bungalow(&app).await;
    assert_eq!(state.project.roof.kind, RoofKind::None, "the template comes migrated");
    assert_eq!(state.project.top_roof().kind, RoofKind::Gable);
    let level = state.project.levels[0].id.clone();
    // "pitch 25": the copilot's set_roof builds a whole roof from top_roof.
    let command = guhit_app::ai::tools::to_command(&state.project, "set_roof", &json!({ "pitch_deg": 25 }), &level).unwrap();
    let applied: ApplyResult =
        serde_json::from_value(call(&app, "doc_apply", json!({ "command": command, "origin": "ai" })).await).unwrap();
    let roof = applied.state.project.top_roof();
    assert_eq!((roof.kind, roof.pitch_deg), (RoofKind::Gable, 25.0));
    assert!(applied.state.project.top_roof_area().is_some(), "the roof element is still there");
}

#[tokio::test]
async fn only_people_set_license_numbers() {
    let (app, _dir) = app();
    bungalow(&app).await;
    let signatory = Signatory { name: "Arch. Ana Reyes".into(), prc_no: "0123456".into(), ..Signatory::default() };
    let command = Command::SetSignatory { signatory: Box::new(signatory.clone()) };
    // An agent is refused, also inside a batch.
    let hint = AgentHint { agent: Some("drafter".into()), client_name: Some("claude-code".into()), ..AgentHint::default() };
    let agent = app.agent_call(&hint, "set_signatory", CallKind::Edit).unwrap();
    let e = app.agent_commit(&agent, command.clone()).await.unwrap_err();
    assert_eq!(e.code, "forbidden");
    assert!(e.message.contains("Only people"), "{}", e.message);
    let batch = Command::Batch { label: "b".into(), commands: vec![command.clone()] };
    assert_eq!(app.agent_commit(&agent, batch).await.unwrap_err().code, "forbidden");
    assert_eq!(guhit_app::people_only(&command).unwrap_err().code, "forbidden");
    // A person sets it; the title block keeps it when edited.
    let applied: ApplyResult =
        serde_json::from_value(call(&app, "doc_apply", json!({ "command": command, "origin": "user" })).await).unwrap();
    assert_eq!(applied.state.project.title_block.signatory, signatory);
    let block = TitleBlock { lot_no: "7".into(), ..TitleBlock::default() };
    let applied: ApplyResult = serde_json::from_value(
        call(&app, "doc_apply", json!({ "command": Command::SetTitleBlock { title_block: Box::new(block) }, "origin": "user" })).await,
    )
    .unwrap();
    assert_eq!(applied.state.project.title_block.signatory, signatory);
    assert_eq!(applied.state.project.title_block.lot_no, "7");
}

#[tokio::test]
async fn every_new_ipc_command_is_routed() {
    let (app, _dir) = app();
    bungalow(&app).await;
    let level = {
        let s: DocState = serde_json::from_value(call(&app, "doc_state", json!({})).await).unwrap();
        s.project.levels[0].id.clone()
    };
    // Professional profile: empty until someone stores one.
    let profile: Signatory = serde_json::from_value(call(&app, "professional_get", json!({})).await).unwrap();
    assert!(profile.is_empty());
    // Every command reaches its lane: it answers, or refuses for its own reason, never as unknown.
    for (cmd, args) in [
        ("export_sheets", json!({ "format": "pdf", "sheet_ids": [] })),
        ("sheet_preview", json!({ "sheet_id": "s" })),
        ("view_svg", json!({ "view": { "type": "plan", "level_id": level, "sheet": "reflected_ceiling" } })),
        ("file_store", json!({ "file_name": "logo.png", "data": "data:image/png;base64," })),
        ("file_data", json!({ "file_name": "logo.png" })),
        ("professional_set", json!({ "signatory": Signatory::default() })),
        ("render_set_look", json!({ "id": null })),
        ("video_begin", json!({ "tour_id": "t", "revision": 0 })),
        ("video_append", json!({ "id": "v", "data": "" })),
        ("video_abort", json!({ "id": "v" })),
    ] {
        if let Err(e) = app.handle(cmd, args).await {
            assert_ne!(e.code, "unknown_command", "{cmd}: {e}");
        }
    }
}
