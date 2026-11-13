//! Lane L5, catalog v2 and AI furnishing (DECISIONS D54): placement
//! options, the catalog search, built-ins and "Furnish rooms", one proposal
//! per room (D8). A scripted model stands in for Claude; everything else is
//! real.

use std::sync::Arc;

use guhit_app::ai::client::{ModelRequest, ModelResponse, ScriptedClient};
use guhit_app::ai::keys::MemoryKeyStore;
use guhit_app::ai::{furnish, AiState};
use guhit_app::AppService;
use guhit_model::*;
use serde_json::{json, Value};

/// Never a real key.
const FAKE_KEY: &str = "sk-ant-test-furnish-0123456789";

fn app() -> (AppService, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    (AppService::new_sandboxed(dir.path().to_path_buf()), dir)
}

/// A service whose copilot is the scripted model.
fn app_with(model: Arc<ScriptedClient>) -> (AppService, tempfile::TempDir) {
    let (mut app, dir) = app();
    app.ai = Arc::new(AiState::with_parts(model, Box::new(MemoryKeyStore::with_key(FAKE_KEY))));
    (app, dir)
}

async fn call(app: &AppService, cmd: &str, args: Value) -> Value {
    app.handle(cmd, args).await.unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

async fn fail(app: &AppService, cmd: &str, args: Value) -> IpcError {
    match app.handle(cmd, args).await {
        Ok(v) => panic!("{cmd} should have failed, got {v}"),
        Err(e) => e,
    }
}

async fn state(app: &AppService) -> DocState {
    serde_json::from_value(call(app, "doc_state", json!({})).await).expect("doc state")
}

async fn apply(app: &AppService, command: Command) -> ApplyResult {
    serde_json::from_value(call(app, "doc_apply", json!({ "command": command })).await).expect("apply result")
}

fn pt(x: f64, y: f64) -> Point {
    Point { x, y }
}

async fn wall(app: &AppService, a: Point, b: Point, t: f64) -> Id {
    let r = apply(app, Command::AddWall { start: a, end: b, thickness_mm: Some(t), height_mm: None, material_id: None, level_id: None, role: None }).await;
    let s = state(app).await;
    r.diff.added.iter().find(|id| s.project.elements.iter().any(|e| matches!(e, Element::Wall(w) if &w.id == *id))).expect("a wall").clone()
}

async fn opening(app: &AppService, wall: &str, ty: OpeningType, offset: f64, width: f64, sill: Option<f64>, height: Option<f64>) {
    apply(
        app,
        Command::AddOpening {
            wall_id: wall.into(),
            opening_type: ty,
            offset_mm: offset,
            width_mm: Some(width),
            height_mm: height,
            sill_mm: sill,
            style: None,
            flip_side: None,
            flip_hinge: None,
            type_id: None,
        },
    )
    .await;
}

fn inside(p: Point, poly: &[Point]) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
    }
    c
}

/// The 12 x 8 m five-room house of the core tests (crates/guhit-core/tests/l5_placement.rs).
async fn house(app: &AppService) -> Vec<(Id, String)> {
    call(app, "hub_create", json!({ "name": "Bahay", "template": "blank" })).await;
    let s = wall(app, pt(0.0, 0.0), pt(12000.0, 0.0), 150.0).await;
    let e = wall(app, pt(12000.0, 0.0), pt(12000.0, 8000.0), 150.0).await;
    let n = wall(app, pt(12000.0, 8000.0), pt(0.0, 8000.0), 150.0).await;
    let w = wall(app, pt(0.0, 8000.0), pt(0.0, 0.0), 150.0).await;
    let mid = wall(app, pt(6000.0, 0.0), pt(6000.0, 8000.0), 100.0).await;
    let west = wall(app, pt(0.0, 5000.0), pt(6000.0, 5000.0), 100.0).await;
    let east = wall(app, pt(6000.0, 4000.0), pt(12000.0, 4000.0), 100.0).await;
    wall(app, pt(8000.0, 4000.0), pt(8000.0, 8000.0), 100.0).await;
    opening(app, &s, OpeningType::Door, 1500.0, 900.0, None, None).await;
    opening(app, &west, OpeningType::Door, 5000.0, 800.0, None, None).await;
    opening(app, &mid, OpeningType::Door, 2500.0, 900.0, None, None).await;
    opening(app, &east, OpeningType::Door, 1000.0, 700.0, None, None).await;
    opening(app, &east, OpeningType::Door, 4800.0, 800.0, None, None).await;
    opening(app, &s, OpeningType::Window, 4000.0, 1500.0, Some(900.0), Some(1200.0)).await;
    opening(app, &w, OpeningType::Window, 5500.0, 1200.0, Some(900.0), Some(1200.0)).await;
    opening(app, &n, OpeningType::Window, 9000.0, 1200.0, Some(900.0), Some(1200.0)).await;
    opening(app, &e, OpeningType::Window, 2000.0, 1200.0, Some(1050.0), Some(1000.0)).await;
    opening(app, &n, OpeningType::Window, 5000.0, 600.0, Some(1500.0), Some(500.0)).await;
    opening(app, &n, OpeningType::Window, 2000.0, 1200.0, Some(900.0), Some(1200.0)).await;
    opening(app, &e, OpeningType::Window, 6000.0, 1000.0, Some(900.0), Some(1200.0)).await;
    let mut rooms = vec![];
    for (x, y, name, usage) in [
        (3000.0, 2500.0, "Living", RoomUsage::Living),
        (3000.0, 6500.0, "Bedroom", RoomUsage::Bedroom),
        (9000.0, 2000.0, "Kitchen", RoomUsage::Kitchen),
        (7000.0, 6000.0, "T&B", RoomUsage::Bathroom),
        (10000.0, 6000.0, "Master bedroom", RoomUsage::MasterBedroom),
    ] {
        let st = state(app).await;
        let id = st.derived.rooms.iter().find(|g| inside(pt(x, y), &g.polygon)).expect("room").room_id.clone();
        let mut room = st
            .project
            .elements
            .iter()
            .find_map(|e| match e {
                Element::Room(r) if r.id == id => Some(r.clone()),
                _ => None,
            })
            .expect("room element");
        room.name = name.into();
        room.usage = usage;
        room.auto_named = false;
        apply(app, Command::UpdateElement { element: Element::Room(room) }).await;
        rooms.push((id, name.to_string()));
    }
    rooms
}

fn request(room_ids: Vec<Id>, style: &str) -> AiRequest {
    AiRequest {
        message: "Furnish the house".into(),
        selection_ids: vec![],
        active_level_id: None,
        history: vec![],
        scope: None,
        task: Some(AiTask::Furnish { room_ids, style: style.into() }),
    }
}

async fn furnish_turn(app: &AppService, req: &AiRequest) -> Result<AiTurn, IpcError> {
    app.handle("ai_chat", json!({ "request": req })).await.map(|v| serde_json::from_value(v).expect("AiTurn"))
}

const PLACEMENT_CODES: &[&str] = &["asset_in_wall", "assets_overlap", "door_swing_blocked", "asset_over_opening"];

fn placement_issues(state: &DocState) -> Vec<Issue> {
    state.derived.issues.iter().filter(|i| PLACEMENT_CODES.contains(&i.code.as_str())).cloned().collect()
}

/// The model's answer, read off the prompt it gets: living and bedroom
/// picked, kitchen with one key it may not use there, T&B and master left
/// to the engine.
fn scripted_picks(request: &ModelRequest) -> ModelResponse {
    let text = request.messages.last().and_then(|m| m["content"].as_str()).unwrap_or("").to_string();
    let id = |name: &str| -> String {
        let quoted = format!("\"{name}\"");
        text.lines()
            .find(|l| l.contains(&quoted))
            .and_then(|l| l.split("room_id ").nth(1))
            .and_then(|r| r.split_whitespace().next())
            .unwrap_or("")
            .to_string()
    };
    ScriptedClient::tools(&[(
        furnish::TOOL,
        json!({ "rooms": [
            { "room_id": id("Living"), "keys": ["sofa-wood", "tea-table", "sideboard", "rocking-chair", "plant-leafy", "ceiling-fan", "wall-art"] },
            { "room_id": id("Bedroom"), "keys": ["bed-double", "nightstand", "wardrobe-builtin", "basket-rattan", "ceiling-fan"] },
            { "room_id": id("Kitchen"), "keys": ["kitchen-run", "refrigerator", "bed-double", "dining-round-2"] },
        ]}),
    )])
}

#[tokio::test]
async fn placement_options_and_the_catalog_answer_over_ipc() {
    let (app, _dir) = app();
    let rooms = house(&app).await;
    let living = rooms[0].0.clone();
    let options = call(&app, "doc_query", json!({ "query": Query::PlacementOptions { room_id: living, catalog_key: "sofa-leather".into() } })).await;
    assert!(!options["options"].as_array().unwrap().is_empty(), "{options}");
    let catalog = call(&app, "doc_query", json!({ "query": Query::Catalog { search: Some("rattan".into()), usage: None } })).await;
    assert_eq!(catalog["items"][0]["key"], "basket-rattan", "{catalog}");
    // The catalog listing carries the new fields.
    let items: Vec<CatalogItem> = serde_json::from_value(call(&app, "catalog_assets", json!({})).await).unwrap();
    let run = items.iter().find(|c| c.key == "kitchen-run").unwrap();
    assert_eq!(run.parametric, Some(BuiltInKind::KitchenRun));
    assert!(items.iter().find(|c| c.key == "sofa-wood").unwrap().model.is_some());
}

#[tokio::test]
async fn furnishing_five_rooms_with_the_fake_provider_raises_no_placement_item() {
    let model = Arc::new(ScriptedClient::dynamic(Box::new(|_, req| scripted_picks(req))));
    let (app, _dir) = app_with(model.clone());
    let rooms = house(&app).await;
    let ids: Vec<Id> = rooms.iter().map(|(id, _)| id.clone()).collect();

    let turn = furnish_turn(&app, &request(ids.clone(), "tropical")).await.expect("furnish turn");
    assert_eq!(model.request_count(), 1, "one model call for all rooms");
    assert!(turn.proposal.is_none());
    assert_eq!(turn.proposals.len(), 5, "{}", turn.reply);
    assert_eq!(turn.tools_used, vec![furnish::TOOL.to_string()]);
    assert!(turn.reply.contains("not a check against any code"), "{}", turn.reply);
    // Each proposal is one room's batch of new objects, clean in its dry run.
    for p in &turn.proposals {
        let Command::Batch { label, commands } = &p.command else { panic!("a batch") };
        assert!(label.starts_with("AI: Furnish "), "{label}");
        assert!(commands.iter().all(|c| matches!(c, Command::AddElement { element: Element::Asset(_) })));
        assert!(p.preview.diff.removed.is_empty() && p.preview.diff.modified.is_empty());
        let bad: Vec<Issue> = placement_issues(&p.preview.state).into_iter().filter(|i| i.element_ids.iter().any(|id| p.preview.diff.added.contains(id))).collect();
        assert!(bad.is_empty(), "{label}: {bad:?}");
    }
    // The model's picks were used, and a key not for the kitchen was dropped.
    let keys_of = |p: &AiProposal| -> Vec<String> {
        let Command::Batch { commands, .. } = &p.command else { return vec![] };
        commands
            .iter()
            .filter_map(|c| match c {
                Command::AddElement { element: Element::Asset(a) } => Some(a.catalog_key.clone()),
                _ => None,
            })
            .collect()
    };
    assert!(keys_of(&turn.proposals[0]).contains(&"sofa-wood".to_string()));
    assert!(keys_of(&turn.proposals[2]).contains(&"kitchen-run".to_string()));
    assert!(!keys_of(&turn.proposals[2]).contains(&"bed-double".to_string()));
    // The engine furnished the rooms the model left out.
    assert!(keys_of(&turn.proposals[3]).contains(&"wc".to_string()));
    assert!(keys_of(&turn.proposals[4]).contains(&"bed-queen".to_string()));

    // Apply all five, each on its own (D8). The first is the pending one;
    // each other room is asked for again, alone, which makes no model call
    // and gives the same objects, then applied.
    let first = &turn.proposals[0];
    let done: AiResolveResult = serde_json::from_value(call(&app, "ai_resolve", json!({ "proposal_id": first.id, "accept": true })).await).unwrap();
    assert!(done.applied.is_some());
    for (i, original) in turn.proposals.iter().enumerate().skip(1) {
        let again = furnish_turn(&app, &request(vec![ids[i].clone()], "tropical")).await.expect("one room");
        assert_eq!(model.request_count(), 1, "no model call to apply a room");
        assert_eq!(again.proposals.len(), 1);
        assert_eq!(keys_of(&again.proposals[0]), keys_of(original), "room {i} is the same plan");
        assert_eq!(again.proposals[0].preview.diff.added, original.preview.diff.added, "with the same ids");
        let r: AiResolveResult = serde_json::from_value(call(&app, "ai_resolve", json!({ "proposal_id": again.proposals[0].id, "accept": true })).await).unwrap();
        assert!(r.applied.is_some());
    }
    let s = state(&app).await;
    let found = placement_issues(&s);
    assert!(found.is_empty(), "{found:#?}");
    let placed = s.project.elements.iter().filter(|e| matches!(e, Element::Asset(_))).count();
    let planned: usize = turn.proposals.iter().map(|p| p.preview.diff.added.len()).sum();
    assert_eq!(placed, planned);
    // Five undo steps, each by the AI.
    let history = call(&app, "history_list", json!({})).await;
    let labels: Vec<String> = history["undo"].as_array().unwrap().iter().take(5).filter_map(|h| h["label"].as_str().map(str::to_string)).collect();
    assert_eq!(labels.len(), 5, "{history}");
    assert!(labels.iter().all(|l| l.starts_with("AI: Furnish ")), "{labels:?}");
}

#[tokio::test]
async fn a_queued_room_is_applied_after_another_when_it_still_fits() {
    let (app, _dir) = app_with(Arc::new(ScriptedClient::new(vec![ScriptedClient::text("No picks.")])));
    let rooms = house(&app).await;
    let ids: Vec<Id> = rooms.iter().map(|(id, _)| id.clone()).collect();
    let turn = furnish_turn(&app, &request(ids, "modern")).await.unwrap();
    assert_eq!(turn.proposals.len(), 5, "{}", turn.reply);
    // A model that picks nothing leaves every room to the engine.
    assert!(turn.tools_used.is_empty());
    let a = furnish::resolve(&app, &turn.proposals[1].id, true).await.expect("queued").expect("applied");
    assert!(a.applied.is_some());
    // The plan moved on; the kitchen's proposal still adds the same objects.
    let b = furnish::resolve(&app, &turn.proposals[2].id, true).await.expect("queued").expect("rebased");
    assert!(b.applied.is_some());
    // Rejected ones apply nothing, and a resolved one is gone from the queue.
    let c = furnish::resolve(&app, &turn.proposals[3].id, false).await.expect("queued").expect("rejected");
    assert!(c.applied.is_none());
    assert!(furnish::resolve(&app, &turn.proposals[3].id, true).await.is_none());
    // Through IPC too: ai_resolve asks the furnishing queue first.
    let d: AiResolveResult = serde_json::from_value(call(&app, "ai_resolve", json!({ "proposal_id": turn.proposals[4].id, "accept": true })).await).unwrap();
    assert!(d.applied.is_some());
    assert!(placement_issues(&state(&app).await).is_empty());
}

#[tokio::test]
async fn furnishing_needs_a_closed_room_and_a_key_and_keeps_to_the_selection() {
    let (app, _dir) = app();
    let rooms = house(&app).await;
    // No API key: the copilot says so, as a chat turn does.
    let e = fail(&app, "ai_chat", json!({ "request": request(vec![rooms[0].0.clone()], "modern") })).await;
    assert_eq!(e.code, "ai_not_configured");

    let (app, _dir) = app_with(Arc::new(ScriptedClient::new(vec![ScriptedClient::text("ok")])));
    let rooms = house(&app).await;
    let e = fail(&app, "ai_chat", json!({ "request": request(vec!["no-such-room".into()], "modern") })).await;
    assert_eq!(e.code, "not_found");
    // Limited to the living room (D30): the other rooms are left out.
    let mut req = request(rooms.iter().map(|(id, _)| id.clone()).collect(), "modern");
    req.scope = Some(EditScope { ids: vec![rooms[0].0.clone()] });
    let turn = furnish_turn(&app, &req).await.unwrap();
    assert_eq!(turn.proposals.len(), 1, "{}", turn.reply);
    assert!(turn.reply.contains("outside the selection"), "{}", turn.reply);
}

#[tokio::test]
async fn placement_catalog_and_furnishing_are_wired() {
    let (app, _dir) = app();
    let state: DocState = serde_json::from_value(call(&app, "hub_create", json!({ "name": "Bahay", "template": "sample-bungalow" })).await).unwrap();
    let room = state.derived.rooms[0].room_id.clone();
    let options = call(&app, "doc_query", json!({ "query": Query::PlacementOptions { room_id: room.clone(), catalog_key: "bed-double".into() } })).await;
    assert!(options["options"].is_array(), "{options}");
    let catalog = call(&app, "doc_query", json!({ "query": Query::Catalog { search: None, usage: None } })).await;
    assert!(catalog["items"].as_array().unwrap().len() >= 100, "{catalog}");
}

/// Gate P1-5: a room's proposal made before a swarm started is refused while
/// the draft is open and stays queued, so it applies once the draft is
/// discarded. Furnishing during the draft asks the model nothing.
#[tokio::test]
async fn a_furnishing_proposal_waits_out_a_swarm_draft() {
    let model = Arc::new(ScriptedClient::new(vec![ScriptedClient::text("No picks.")]));
    let (app, _dir) = app_with(model.clone());
    let rooms = house(&app).await;
    let ids: Vec<Id> = rooms.iter().map(|(id, _)| id.clone()).collect();
    let turn = furnish_turn(&app, &request(ids.clone(), "modern")).await.unwrap();
    assert!(turn.proposals.len() >= 2, "{}", turn.reply);
    let asked = model.requests.lock().unwrap().len();
    let revision = state(&app).await.revision;
    // An MCP swarm opens its draft.
    let hint = guhit_app::agents::AgentHint {
        connection: Some("c-lead".into()),
        client_name: Some("claude-code".into()),
        client_version: Some("2.1".into()),
        ..Default::default()
    };
    let lead = app.agent_call(&hint, "swarm", guhit_app::agents::CallKind::Edit).unwrap();
    let part = SwarmPartPlan {
        key: "shell".into(),
        name: "Shell".into(),
        role: String::new(),
        instructions: String::new(),
        scope: SwarmScope::default(),
        after: vec![],
        milestones: vec![],
    };
    let plan = SwarmPlan { summary: "Walls".into(), setup: SwarmSetup::default(), parts: vec![part] };
    let swarm_id = app.swarm_start_mcp(&lead, "Walls", plan).await.unwrap().swarm.id;
    for p in &turn.proposals[..2] {
        let e = fail(&app, "ai_resolve", json!({ "proposal_id": p.id, "accept": true })).await;
        assert_eq!(e.code, "swarm_running");
    }
    assert_eq!(fail(&app, "ai_chat", json!({ "request": request(ids, "modern") })).await.code, "swarm_running");
    assert_eq!(model.requests.lock().unwrap().len(), asked, "nothing asked during the draft");
    assert_eq!(state(&app).await.revision, revision);
    app.swarm_discard(Some(&swarm_id)).await.unwrap();
    for p in &turn.proposals[..2] {
        let r: AiResolveResult = serde_json::from_value(call(&app, "ai_resolve", json!({ "proposal_id": p.id, "accept": true })).await).unwrap();
        assert!(r.applied.is_some(), "applied after the discard");
    }
}
