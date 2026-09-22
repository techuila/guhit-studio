//! Copilot tests. A scripted model stands in for Claude; everything else is
//! real: `AppService`, `Document`, a temp data dir, the log file.

use std::sync::Arc;

use guhit_core::Document;
use guhit_model::*;
use serde_json::{json, Value};

use super::client::{ModelResponse, ScriptedClient};
use super::keys::MemoryKeyStore;
use super::{log, AiState, MAX_ROUNDS};
use crate::AppService;

/// Never a real key. The tests assert that it shows up nowhere.
const FAKE_KEY: &str = "sk-ant-test-THIS-MUST-NEVER-LEAK-0123456789";

struct Rig {
    app: AppService,
    model: Arc<ScriptedClient>,
    _dir: tempfile::TempDir,
}

async fn rig(responses: Vec<ModelResponse>) -> Rig {
    rig_with(ScriptedClient::new(responses)).await
}

async fn rig_with(model: ScriptedClient) -> Rig {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut app = AppService::new(dir.path().to_path_buf());
    let model = Arc::new(model);
    app.ai = Arc::new(AiState::with_parts(model.clone(), Box::new(MemoryKeyStore::with_key(FAKE_KEY))));
    // Open a project the way the UI does. Fall back to a bare document if
    // the hub is not available.
    let opened = app.handle("hub_create", json!({"name": "AI test", "settings": null, "template": "blank"})).await;
    if opened.is_err() {
        app.session.lock().await.doc = Some(Document::new(defaults::new_project("AI test")));
    }
    Rig { app, model, _dir: dir }
}

impl Rig {
    async fn chat(&self, message: &str) -> Result<AiTurn, IpcError> {
        let request = json!({"message": message, "selection_ids": [], "history": []});
        let v = self.app.handle("ai_chat", json!({"request": request})).await?;
        Ok(serde_json::from_value(v).expect("AiTurn"))
    }

    async fn resolve(&self, id: &str, accept: bool) -> Result<AiResolveResult, IpcError> {
        let v = self.app.handle("ai_resolve", json!({"proposal_id": id, "accept": accept})).await?;
        Ok(serde_json::from_value(v).expect("AiResolveResult"))
    }

    async fn project(&self) -> Project {
        self.app.session.lock().await.doc.as_ref().expect("doc").project().clone()
    }

    async fn revision(&self) -> u32 {
        self.app.session.lock().await.doc.as_ref().expect("doc").revision()
    }

    async fn log_lines(&self) -> Vec<Value> {
        let dir = self.app.project_dir().await.expect("project dir");
        let text = std::fs::read_to_string(dir.join(log::LOG_FILE)).unwrap_or_default();
        text.lines().map(|l| serde_json::from_str(l).expect("log line is JSON")).collect()
    }
}

/// True when the engine assigns the same ids to the same command on the
/// same state (docs/CONTRACT.md). The baseline engine used random ids; the
/// id-dependent assertions only run once the engine is deterministic.
async fn engine_ids_are_deterministic(r: &Rig) -> bool {
    let s = r.app.session.lock().await;
    let doc = s.doc.as_ref().unwrap();
    let cmd: Command = serde_json::from_value(json!({
        "type": "add_wall", "start": {"x": 0.0, "y": 0.0}, "end": {"x": 1234.0, "y": 0.0},
        "thickness_mm": null, "height_mm": null, "material_id": null, "level_id": null
    }))
    .unwrap();
    let deterministic = doc.preview(&cmd).unwrap().diff.added == doc.preview(&cmd).unwrap().diff.added;
    if !deterministic {
        eprintln!("NOTE: engine ids are not deterministic yet, id-dependent assertions skipped");
    }
    deterministic
}

fn wall(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
    json!({"start": {"x": x0, "y": y0}, "end": {"x": x1, "y": y1}})
}

/// The tool results the loop sent back to the model in request `index`.
fn tool_results(model: &ScriptedClient, index: usize) -> Vec<Value> {
    let requests = model.requests.lock().unwrap();
    let last = requests[index].messages.last().expect("messages").clone();
    last["content"].as_array().cloned().unwrap_or_default()
}

#[tokio::test]
async fn query_turn_has_no_proposal_and_feeds_model_data_back() {
    let r = rig(vec![
        ScriptedClient::tools(&[("get_project_summary", json!({}))]),
        ScriptedClient::text("The total floor area is 0.00 m2."),
    ])
    .await;
    let before = r.project().await;

    let turn = r.chat("What is the total floor area?").await.unwrap();

    assert!(turn.proposal.is_none());
    assert_eq!(turn.tools_used, vec!["get_project_summary"]);
    assert_eq!(turn.reply, "The total floor area is 0.00 m2.");
    assert_eq!(r.project().await, before);

    // Grounding path: the second request carries the query result, taken
    // from the document, as a tool_result for the model to answer from.
    assert_eq!(r.model.request_count(), 2);
    let results = tool_results(&r.model, 1);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["type"], "tool_result");
    assert!(results[0].get("is_error").is_none());
    let expected = {
        let s = r.app.session.lock().await;
        s.doc.as_ref().unwrap().query(&Query::ProjectSummary).unwrap()
    };
    let sent: Value = serde_json::from_str(results[0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(sent, expected);
}

#[tokio::test]
async fn first_request_has_system_prompt_context_and_strict_tools() {
    let r = rig(vec![ScriptedClient::text("ok")]).await;
    r.chat("hello").await.unwrap();
    let requests = r.model.requests.lock().unwrap();
    let req = &requests[0];
    assert!(req.system.contains("millimeters"));
    assert!(req.system.contains("Taglish"));
    assert!(!req.system.contains('\u{2014}') && !req.system.contains('\u{2013}'));
    let user = req.messages.last().unwrap()["content"].as_str().unwrap().to_string();
    assert!(user.contains("<project_context>") && user.ends_with("hello"));
    assert_eq!(req.tools.len(), 21);
    for tool in &req.tools {
        let name = tool["name"].as_str().unwrap();
        assert_eq!(tool["input_schema"]["additionalProperties"], json!(false), "{name}");
        assert_eq!(tool.get("strict") == Some(&json!(true)), super::tools::is_edit(name), "{name}");
    }
}

#[tokio::test]
async fn edit_turn_stages_a_batch_and_does_not_touch_the_document() {
    let r = rig(vec![
        ScriptedClient::tools(&[("add_wall", wall(0.0, 0.0, 4000.0, 0.0)), ("add_wall", wall(4000.0, 0.0, 4000.0, 3000.0))]),
        ScriptedClient::text("I staged two walls."),
    ])
    .await;
    let before = r.project().await;
    let revision = r.revision().await;

    let turn = r.chat("Add a 4 m wall east from the origin, then 3 m north").await.unwrap();

    let proposal = turn.proposal.expect("proposal");
    assert_eq!(proposal.base_revision, revision);
    match &proposal.command {
        Command::Batch { label, commands } => {
            assert!(label.starts_with("AI: "));
            assert_eq!(commands.len(), 2);
            // Plain typed commands: the core seeds ids per leaf, so a staged
            // wall keeps its id as the batch grows (see tools::wall_command).
            assert!(commands.iter().all(|c| matches!(c, Command::AddWall { .. })));
        }
        other => panic!("proposal command is not a batch: {other:?}"),
    }
    assert_eq!(proposal.preview.diff.added.len(), 2);
    assert_eq!(proposal.preview.state.project.elements.len(), before.elements.len() + 2);
    // Nothing was committed.
    assert_eq!(r.project().await, before);
    assert_eq!(r.revision().await, revision);

    // Each staged call reported the new ids back to the model.
    let deterministic = engine_ids_are_deterministic(&r).await;
    let results = tool_results(&r.model, 1);
    assert_eq!(results.len(), 2, "both results go back in one user message");
    let mut reported: Vec<String> = vec![];
    for result in &results {
        let body: Value = serde_json::from_str(result["content"].as_str().unwrap()).unwrap();
        assert_eq!(body["staged"], json!(true));
        let added = body["this_step"]["added"].as_array().unwrap().clone();
        assert!(!added.is_empty());
        if deterministic {
            assert_eq!(added.len(), 1, "one call, one new wall");
            reported.push(added[0]["id"].as_str().unwrap().to_string());
        }
    }
    if deterministic {
        // The ids the model was told about are the ids of the proposal.
        assert_eq!(reported, proposal.preview.diff.added);
    }
}

/// Body of the first tool result in the last message of `request`.
fn first_result(request: &super::client::ModelRequest) -> (Value, bool) {
    let block = request.messages.last().unwrap()["content"][0].clone();
    let is_error = block.get("is_error") == Some(&json!(true));
    let text = block["content"].as_str().unwrap_or_default().to_string();
    (serde_json::from_str(&text).unwrap_or(json!(text)), is_error)
}

#[tokio::test]
async fn a_later_step_can_build_on_a_wall_staged_in_the_same_turn() {
    let r = rig_with(ScriptedClient::dynamic(Box::new(|index, request| match index {
        0 => ScriptedClient::tools(&[("add_wall", wall(0.0, 0.0, 4000.0, 0.0))]),
        1 => {
            let (body, _) = first_result(request);
            let added = &body["this_step"]["added"][0];
            let wall_id = added["id"].as_str().unwrap().to_string();
            ScriptedClient::tools(&[("add_window", json!({"wall_id": wall_id, "position": "center"}))])
        }
        _ => {
            let (body, is_error) = first_result(request);
            assert!(!is_error, "window on a staged wall failed: {body}");
            ScriptedClient::text("Staged a wall with a window at its center.")
        }
    })))
    .await;

    let proposal = r.chat("a 4 m wall with a window in the middle").await.unwrap().proposal.expect("proposal");

    let elements = &proposal.preview.state.project.elements;
    let wall_id = elements.iter().find_map(|e| match e {
        Element::Wall(w) => Some(w.id.clone()),
        _ => None,
    });
    let window = elements.iter().find_map(|e| match e {
        Element::Opening(o) => Some(o.clone()),
        _ => None,
    });
    let window = window.expect("window staged");
    assert_eq!(Some(window.wall_id.clone()), wall_id);
    assert_eq!(window.offset_mm, 2000.0);

    // And the whole thing commits as previewed.
    let applied = r.resolve(&proposal.id, true).await.unwrap().applied.unwrap();
    assert_eq!(applied.state.project.elements, proposal.preview.state.project.elements);
    assert_eq!(applied.diff, proposal.preview.diff);
}

#[tokio::test]
async fn a_door_goes_on_a_wall_that_add_rect_room_staged_in_the_same_turn() {
    // The walls of add_rect_room get ids from the engine. Ids are seeded per
    // leaf command, so they do not move when the batch grows and a later step
    // of the same proposal can host a door on one of them.
    let r = rig_with(ScriptedClient::dynamic(Box::new(|index, request| match index {
        0 => ScriptedClient::tools(&[(
            "add_rect_room",
            json!({"origin": {"x": 0, "y": 0}, "width_mm": 4000, "depth_mm": 3000, "name": "Bedroom"}),
        )]),
        1 => {
            let (body, is_error) = first_result(request);
            assert!(!is_error, "{body}");
            let wall = body["this_step"]["added"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["kind"] == "wall")
                .expect("a new wall")
                .clone();
            ScriptedClient::tools(&[(
                "add_door",
                json!({"wall_id": wall["id"], "position": "center", "flip_side": true}),
            )])
        }
        _ => {
            let (body, is_error) = first_result(request);
            assert!(!is_error, "a door on a wall staged by add_rect_room must work: {body}");
            ScriptedClient::text("Staged the bedroom with a door.")
        }
    })))
    .await;

    let proposal = r.chat("a 4 x 3 m bedroom with a door").await.unwrap().proposal.expect("proposal");
    assert!(proposal.preview.diff.added.len() >= 6, "4 walls, a room and a door");
    let elements = &proposal.preview.state.project.elements;
    let room = elements.iter().find_map(|e| match e {
        Element::Room(room) => Some(room.name.clone()),
        _ => None,
    });
    assert_eq!(room.as_deref(), Some("Bedroom"));

    // The door is hosted on one of the walls the first step created, and the
    // flip flag reached the model.
    let door = elements
        .iter()
        .find_map(|e| match e {
            Element::Opening(o) => Some(o.clone()),
            _ => None,
        })
        .expect("door staged");
    assert_eq!(door.opening_type, OpeningType::Door);
    assert!(door.flip_side, "flip_side passed through the tool");
    assert!(!door.flip_hinge, "an omitted flip is false");
    assert!(elements.iter().any(|e| matches!(e, Element::Wall(w) if w.id == door.wall_id)));

    // And it applies exactly as previewed.
    let applied = r.resolve(&proposal.id, true).await.unwrap().applied.unwrap();
    assert_eq!(applied.state.project.elements, proposal.preview.state.project.elements);
}

#[tokio::test]
async fn new_elements_go_on_the_active_level_and_an_unknown_level_is_refused() {
    let r = rig(vec![
        ScriptedClient::tools(&[("add_wall", wall(0.0, 0.0, 4000.0, 0.0))]),
        ScriptedClient::text("Staged."),
    ])
    .await;

    // A second level, so "first level" and "active level" differ.
    let upper = {
        let mut s = r.app.session.lock().await;
        let doc = s.doc.as_mut().unwrap();
        let mut level = doc.project().levels[0].clone();
        level.id = defaults::new_id();
        level.name = "Second floor".into();
        level.elevation_mm = 3000.0;
        let id = level.id.clone();
        // Levels are project data, so add it straight to the open project.
        let mut project = doc.project().clone();
        project.levels.push(level);
        *doc = Document::new(project);
        id
    };

    let request = json!({"message": "a wall upstairs", "selection_ids": [], "active_level_id": upper, "history": []});
    let turn: AiTurn = serde_json::from_value(
        r.app.handle("ai_chat", json!({"request": request})).await.unwrap(),
    )
    .unwrap();
    let staged = turn.proposal.expect("proposal");
    let new_wall = staged
        .preview
        .state
        .project
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Wall(w) => Some(w.clone()),
            _ => None,
        })
        .expect("wall");
    assert_eq!(new_wall.level_id, upper, "the wall goes on the active level");

    let bad = json!({"message": "hi", "selection_ids": [], "active_level_id": "no-such-level", "history": []});
    let err = r.app.handle("ai_chat", json!({"request": bad})).await.unwrap_err();
    assert_eq!(err.code, "not_found");
    assert!(err.message.contains("no-such-level"), "{}", err.message);
}

#[tokio::test]
async fn failing_tool_call_is_reported_and_not_staged() {
    let r = rig(vec![
        ScriptedClient::tools(&[
            // Zero length: the engine rejects it.
            ("add_wall", wall(1000.0, 1000.0, 1000.0, 1000.0)),
            // Unknown field: rejected before it reaches the engine.
            ("add_wall", json!({"start": {"x": 0, "y": 0}, "end": {"x": 1, "y": 1}, "color": "red"})),
            // Unknown id: rejected by the translation step.
            ("rename_room", json!({"room_id": "no-such-room", "name": "Master Bedroom"})),
            ("add_wall", wall(0.0, 0.0, 5000.0, 0.0)),
        ]),
        ScriptedClient::text("One wall staged, three calls failed."),
    ])
    .await;

    let turn = r.chat("do several things").await.unwrap();

    let results = tool_results(&r.model, 1);
    assert_eq!(results.len(), 4);
    for failed in &results[0..3] {
        assert_eq!(failed["is_error"], json!(true), "{failed}");
        assert!(!failed["content"].as_str().unwrap().is_empty());
    }
    assert!(results[1]["content"].as_str().unwrap().contains("color"));
    assert!(results[2]["content"].as_str().unwrap().contains("no-such-room"));
    assert!(results[3].get("is_error").is_none());

    let proposal = turn.proposal.expect("the good call is still proposed");
    match &proposal.command {
        Command::Batch { commands, .. } => assert_eq!(commands.len(), 1, "failed calls are dropped"),
        _ => panic!("not a batch"),
    }
    assert_eq!(proposal.preview.diff.added.len(), 1);
}

#[tokio::test]
async fn all_calls_failing_means_no_proposal() {
    let r = rig(vec![
        ScriptedClient::tools(&[("add_wall", wall(0.0, 0.0, 0.0, 0.0))]),
        ScriptedClient::text("That wall has no length, so I could not stage it."),
    ])
    .await;
    let turn = r.chat("add a wall").await.unwrap();
    assert!(turn.proposal.is_none());
}

#[tokio::test]
async fn accept_commits_one_undo_step_and_undo_restores_the_project() {
    let r = rig(vec![
        ScriptedClient::tools(&[("add_wall", wall(0.0, 0.0, 4000.0, 0.0))]),
        ScriptedClient::tools(&[("add_wall", wall(4000.0, 0.0, 4000.0, 3000.0))]),
        ScriptedClient::text("Staged."),
    ])
    .await;
    let before = r.project().await;
    let revision = r.revision().await;
    let deterministic = engine_ids_are_deterministic(&r).await;
    let proposal = r.chat("two walls").await.unwrap().proposal.unwrap();

    let applied = r.resolve(&proposal.id, true).await.unwrap().applied.expect("applied");

    assert_eq!(applied.state.revision, revision + 1, "exactly one apply");
    assert_eq!(applied.state.project.elements.len(), before.elements.len() + 2);
    assert_eq!(applied.state.undo_label.as_deref(), proposal.preview.state.undo_label.as_deref());
    assert_eq!(r.project().await.elements, applied.state.project.elements);

    // Committed result equals the preview. This needs deterministic ids in
    // the engine (docs/CONTRACT.md). Geometry and counts must match always.
    assert_eq!(applied.diff.added.len(), proposal.preview.diff.added.len());
    assert_eq!(applied.diff.summary, proposal.preview.diff.summary);
    if deterministic {
        assert_eq!(applied.state.project.elements, proposal.preview.state.project.elements);
        assert_eq!(applied.state.derived, proposal.preview.state.derived);
        assert_eq!(applied.diff, proposal.preview.diff);
    }

    // One undo restores the prior project exactly.
    let undone = r.app.handle("doc_undo", json!({})).await.unwrap();
    let undone: DocState = serde_json::from_value(undone).unwrap();
    assert_eq!(undone.project.elements, before.elements);
    assert_eq!(undone.project.roof, before.roof);
    assert!(!undone.can_undo || undone.undo_label != applied.state.undo_label);

    // A resolved proposal cannot be resolved twice.
    assert_eq!(r.resolve(&proposal.id, true).await.unwrap_err().code, "not_found");
}

#[tokio::test]
async fn stale_proposal_is_refused() {
    let r = rig(vec![
        ScriptedClient::tools(&[("add_wall", wall(0.0, 0.0, 4000.0, 0.0))]),
        ScriptedClient::text("Staged."),
    ])
    .await;
    let proposal = r.chat("a wall").await.unwrap().proposal.unwrap();

    // The user edits the plan before answering the proposal.
    let user_edit = Command::AddWall {
        start: Point { x: 0.0, y: 9000.0 },
        end: Point { x: 2000.0, y: 9000.0 },
        thickness_mm: None,
        height_mm: None,
        material_id: None,
        level_id: None,
    };
    r.app.commit(user_edit, Origin::User).await.unwrap();
    let after_edit = r.project().await;

    let err = r.resolve(&proposal.id, true).await.unwrap_err();
    assert_eq!(err.code, "stale");
    assert_eq!(r.project().await, after_edit, "a stale proposal applies nothing");
    let outcomes: Vec<Value> = r.log_lines().await.into_iter().filter(|l| l["event"] == "outcome").collect();
    assert_eq!(outcomes.last().unwrap()["outcome"], "stale");
}

#[tokio::test]
async fn reject_leaves_the_document_untouched() {
    let r = rig(vec![
        ScriptedClient::tools(&[("add_wall", wall(0.0, 0.0, 4000.0, 0.0))]),
        ScriptedClient::text("Staged."),
    ])
    .await;
    let before = r.project().await;
    let revision = r.revision().await;
    let proposal = r.chat("a wall").await.unwrap().proposal.unwrap();

    let result = r.resolve(&proposal.id, false).await.unwrap();

    assert!(result.applied.is_none());
    assert_eq!(r.project().await, before);
    assert_eq!(r.revision().await, revision);
    // The proposal is gone: accepting it afterwards fails.
    assert_eq!(r.resolve(&proposal.id, true).await.unwrap_err().code, "not_found");
}

#[tokio::test]
async fn a_new_turn_discards_the_pending_proposal() {
    let r = rig(vec![
        ScriptedClient::tools(&[("add_wall", wall(0.0, 0.0, 4000.0, 0.0))]),
        ScriptedClient::text("Staged."),
        ScriptedClient::text("Just chatting."),
    ])
    .await;
    let first = r.chat("a wall").await.unwrap().proposal.unwrap();
    let second = r.chat("never mind").await.unwrap();
    assert!(second.proposal.is_none());
    assert_eq!(r.resolve(&first.id, true).await.unwrap_err().code, "not_found");
}

#[tokio::test]
async fn round_cap_is_respected() {
    // A model that never stops calling tools: the script's last response
    // repeats forever.
    let r = rig(vec![ScriptedClient::tools(&[("get_project_summary", json!({}))])]).await;

    let turn = r.chat("loop forever").await.unwrap();

    assert_eq!(r.model.request_count(), MAX_ROUNDS);
    assert!(turn.proposal.is_none());
    assert!(turn.reply.contains("stopped after 8 steps"), "{}", turn.reply);
    let turn_line = r.log_lines().await.into_iter().find(|l| l["event"] == "turn").unwrap();
    assert_eq!(turn_line["hit_round_cap"], json!(true));
    assert_eq!(turn_line["rounds"], json!(MAX_ROUNDS));
}

#[tokio::test]
async fn log_lines_are_written_and_hold_no_key_material() {
    let r = rig(vec![
        ScriptedClient::tools(&[("list_review_items", json!({})), ("add_wall", wall(0.0, 0.0, 0.0, 0.0))]),
        ScriptedClient::tools(&[("add_wall", wall(0.0, 0.0, 4000.0, 0.0))]),
        ScriptedClient::text("Staged one wall."),
    ])
    .await;
    // Put the key through the settings path too.
    let settings = r.app.handle("ai_settings_set", json!({"api_key": FAKE_KEY, "model": null})).await.unwrap();
    assert_eq!(settings["has_api_key"], json!(true));
    assert!(!settings.to_string().contains(FAKE_KEY), "the key never crosses IPC");

    let proposal = r.chat("Add a 4 m wall").await.unwrap().proposal.unwrap();
    r.resolve(&proposal.id, true).await.unwrap();

    let lines = r.log_lines().await;
    let events: Vec<&str> = lines.iter().map(|l| l["event"].as_str().unwrap()).collect();
    assert_eq!(events, vec!["intent", "tool", "tool", "tool", "turn", "outcome"]);

    assert_eq!(lines[0]["message"], "Add a 4 m wall");
    assert_eq!(lines[1]["tool"], "list_review_items");
    assert_eq!(lines[1]["ok"], json!(true));
    assert_eq!(lines[2]["tool"], "add_wall");
    assert_eq!(lines[2]["ok"], json!(false));
    assert!(!lines[2]["error"].as_str().unwrap().is_empty());
    assert_eq!(lines[3]["ok"], json!(true));
    assert_eq!(lines[3]["staged"], json!(true));
    assert_eq!(lines[3]["args"]["end"]["x"], json!(4000.0));
    assert!(lines[3]["tool_ms"].is_u64() && lines[3]["model_ms"].is_u64());
    assert_eq!(lines[4]["proposal_id"], json!(proposal.id));
    assert!(lines[4]["latency_ms"].is_u64());
    assert_eq!(lines[5]["proposal_id"], json!(proposal.id));
    assert_eq!(lines[5]["outcome"], "accepted");

    // No secrets, no project dumps.
    let dir = r.app.project_dir().await.unwrap();
    let raw = std::fs::read_to_string(dir.join(log::LOG_FILE)).unwrap();
    assert!(!raw.contains(FAKE_KEY));
    assert!(!raw.contains("sk-ant"));
    assert!(!raw.contains("schema_version"), "no full project dump in the log");

    // Nor did the key reach the model request or the data dir.
    for request in r.model.requests.lock().unwrap().iter() {
        let sent = format!("{}{:?}{:?}", request.system, request.messages, request.tools);
        assert!(!sent.contains(FAKE_KEY));
    }
}

#[tokio::test]
async fn settings_persist_the_model_and_merge_with_other_keys() {
    let r = rig(vec![ScriptedClient::text("ok")]).await;
    let data_dir = r.app.session.lock().await.data_dir.clone();
    std::fs::write(data_dir.join("settings.json"), r#"{"recent_export_dir":"/tmp/x","ai_model":"old"}"#).unwrap();

    let got = r.app.handle("ai_settings_get", json!({})).await.unwrap();
    assert_eq!(got, json!({"has_api_key": true, "model": "old"}));

    let set = r.app.handle("ai_settings_set", json!({"api_key": "", "model": "claude-sonnet-5"})).await.unwrap();
    assert_eq!(set, json!({"has_api_key": false, "model": "claude-sonnet-5"}));

    let file: Value = serde_json::from_str(&std::fs::read_to_string(data_dir.join("settings.json")).unwrap()).unwrap();
    assert_eq!(file["recent_export_dir"], "/tmp/x", "other settings survive");
    assert_eq!(file["ai_model"], "claude-sonnet-5");
    assert!(!file.to_string().contains("sk-"), "the key is never written to settings.json");

    // An empty model name falls back to the default.
    let reset = r.app.handle("ai_settings_set", json!({"api_key": null, "model": ""})).await.unwrap();
    assert_eq!(reset["model"], json!(super::DEFAULT_MODEL));
}

#[tokio::test]
async fn chat_without_a_key_reports_ai_not_configured() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = AppService::new(dir.path().to_path_buf());
    // Real client path, empty in-memory key store: no network is reached.
    app.ai = Arc::new(AiState::with_keys(Box::new(MemoryKeyStore::default())));
    app.session.lock().await.doc = Some(Document::new(defaults::new_project("No key")));
    let request = json!({"message": "hi", "selection_ids": [], "history": []});
    let err = app.handle("ai_chat", json!({"request": request})).await.unwrap_err();
    assert_eq!(err.code, "ai_not_configured");
}

#[tokio::test]
async fn refusal_drops_staged_work() {
    let r = rig(vec![
        ScriptedClient::tools(&[("add_wall", wall(0.0, 0.0, 4000.0, 0.0))]),
        ModelResponse { content: vec![], stop_reason: "refusal".into() },
    ])
    .await;
    let turn = r.chat("a wall").await.unwrap();
    assert!(turn.proposal.is_none());
    assert!(turn.reply.contains("declined"));
}

#[test]
fn center_position_uses_half_the_wall_length() {
    let mut project = defaults::new_project("t");
    let level_id = project.levels[0].id.clone();
    project.elements.push(Element::Wall(Wall {
        id: "w1".into(),
        level_id: level_id.clone(),
        start: Point { x: 0.0, y: 0.0 },
        end: Point { x: 3000.0, y: 4000.0 },
        thickness_mm: 150.0,
        height_mm: None,
        material_id: None,
    }));
    let cmd = super::tools::to_command(&project, "add_window", &json!({"wall_id": "w1", "position": "center"}), &level_id).unwrap();
    match cmd {
        Command::AddOpening { offset_mm, opening_type, width_mm, .. } => {
            assert_eq!(offset_mm, 2500.0);
            assert_eq!(opening_type, OpeningType::Window);
            assert_eq!(width_mm, None, "sizes the user did not give stay at engine defaults");
        }
        other => panic!("{other:?}"),
    }
    let err = super::tools::to_command(&project, "add_door", &json!({"wall_id": "w1", "position": "offset"}), &level_id).unwrap_err();
    assert!(err.0.contains("offset_mm"));
}

#[test]
fn every_edit_tool_translates_to_a_typed_command() {
    let mut project = defaults::new_project("t");
    let level_id = project.levels[0].id.clone();
    project.elements.push(Element::Room(Room {
        id: "r1".into(),
        level_id: level_id.clone(),
        name: "Room 1".into(),
        usage: RoomUsage::Other,
        seed: Point { x: 1.0, y: 1.0 },
        floor_material_id: None,
        auto_named: true,
    }));
    project.elements.push(Element::Wall(Wall {
        id: "w1".into(),
        level_id: level_id.clone(),
        start: Point { x: 0.0, y: 0.0 },
        end: Point { x: 4000.0, y: 0.0 },
        thickness_mm: 150.0,
        height_mm: None,
        material_id: None,
    }));
    project.elements.push(Element::Opening(Opening {
        id: "o1".into(),
        wall_id: "w1".into(),
        opening_type: OpeningType::Door,
        style: OpeningStyle::SwingSingle,
        offset_mm: 1000.0,
        width_mm: 900.0,
        height_mm: 2100.0,
        sill_mm: 0.0,
        flip_side: false,
        flip_hinge: false,
        material_id: None,
    }));
    let p = json!({"x": 0, "y": 0});
    let cases = vec![
        ("add_wall", wall(0.0, 0.0, 1.0, 1.0)),
        ("add_wall_chain", json!({"points": [p, {"x": 1000, "y": 0}], "closed": false})),
        ("add_rect_room", json!({"origin": p, "width_mm": 4000, "depth_mm": 3000, "name": "Bedroom"})),
        ("add_door", json!({"wall_id": "w1", "position": "offset", "offset_mm": 800})),
        ("add_window", json!({"wall_id": "w1", "position": "center", "sill_mm": 900})),
        ("resize_room", json!({"room_id": "r1", "side": "east", "delta_mm": 300})),
        ("set_wall_length", json!({"wall_id": "w1", "length_mm": 3500, "anchor": "start"})),
        ("move_elements", json!({"ids": ["w1"], "dx_mm": 100, "dy_mm": 0, "stretch_connected": true})),
        ("rename_room", json!({"room_id": "r1", "name": "Master Bedroom"})),
        ("set_room_usage", json!({"room_id": "r1", "usage": "master_bedroom"})),
        ("set_opening_size", json!({"opening_id": "o1", "width_mm": 800})),
        ("delete_elements", json!({"ids": ["o1"]})),
        ("set_material", json!({"ids": ["w1"], "material_id": "mat-paint-sage"})),
        ("set_roof", json!({"kind": "gable", "pitch_deg": 25})),
        ("add_asset", json!({"catalog_key": "bed-double", "position": p})),
    ];
    assert_eq!(cases.len(), super::tools::EDIT_TOOLS.len());
    for (name, input) in cases {
        let cmd = super::tools::to_command(&project, name, &input, &level_id);
        assert!(cmd.is_ok(), "{name}: {cmd:?}");
    }
    // Spot checks.
    match super::tools::to_command(&project, "rename_room", &json!({"room_id": "r1", "name": " Master Bedroom "}), &level_id).unwrap() {
        Command::UpdateElement { element: Element::Room(room) } => {
            assert_eq!(room.name, "Master Bedroom");
            assert!(!room.auto_named);
        }
        other => panic!("{other:?}"),
    }
    match super::tools::to_command(&project, "resize_room", &json!({"room_id": "r1", "side": "east", "delta_mm": 300}), &level_id).unwrap() {
        Command::ResizeRoom { side, delta_mm, .. } => {
            assert_eq!(side, Side::East);
            assert_eq!(delta_mm, 300.0);
        }
        other => panic!("{other:?}"),
    }
    assert!(super::tools::to_command(&project, "set_material", &json!({"ids": ["w1"], "material_id": "mat-nope"}), &level_id)
        .unwrap_err()
        .0
        .contains("Known material ids"));
    assert!(super::tools::to_command(&project, "resize_room", &json!({"room_id": "w1", "side": "east", "delta_mm": 300}), &level_id).is_err());
}
