//! Swarms over MCP (DECISIONS D56, D57): a lead and three sub-agents draw a
//! two storey house into a draft through the real router on a random port,
//! as Claude Code (sub-agents on the lead's connection, told apart by their
//! handles) and Codex (a part's labelled URL) would. Main never moves until
//! a person accepts; each part then lands as one step by its own agent.

use std::collections::BTreeMap;
use std::time::Duration;

use guhit_app::AppService;
use guhit_model::*;
use serde_json::{json, Value};

fn app() -> (AppService, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    (AppService::new_sandboxed(dir.path().to_path_buf()), dir)
}

/// The real router on 127.0.0.1 and a random port.
async fn serve(app: AppService) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    app.mcp.set_port(port);
    tokio::spawn(async move { axum::serve(listener, guhit_mcp::router(app)).await });
    port
}

fn client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(120)).build().unwrap()
}

/// One MCP connection: an URL and the session the server issued it.
#[derive(Clone)]
struct Conn {
    url: String,
    session: String,
}

async fn post(url: &str, session: Option<&str>, body: Value) -> (Value, Option<String>) {
    let mut req = client()
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(body.to_string());
    if let Some(s) = session {
        req = req.header("mcp-session-id", s);
    }
    let res = req.send().await.expect("request");
    let issued = res.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()).map(str::to_string);
    let text = res.text().await.unwrap();
    (serde_json::from_str(&text).unwrap_or_else(|_| panic!("not JSON: {text}")), issued)
}

async fn connect(url: &str, client_name: &str) -> Conn {
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": client_name, "version": "1"}}});
    let (_, session) = post(url, None, init).await;
    Conn { url: url.to_string(), session: session.expect("a session id") }
}

impl Conn {
    /// A tool call; the whole result.
    async fn call(&self, name: &str, args: Value) -> Value {
        let body = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": name, "arguments": args}});
        let (v, _) = post(&self.url, Some(&self.session), body).await;
        v["result"].clone()
    }

    /// A call that must work: its structured content.
    async fn ok(&self, name: &str, args: Value) -> Value {
        let r = self.call(name, args.clone()).await;
        assert_eq!(r["isError"], false, "`{name}` {args} failed: {r}");
        r["structuredContent"].clone()
    }

    /// A call that must fail: its structured error.
    async fn fail(&self, name: &str, args: Value) -> Value {
        let r = self.call(name, args.clone()).await;
        assert_eq!(r["isError"], true, "`{name}` {args} was expected to fail: {r}");
        r["structuredContent"].clone()
    }

    async fn resource(&self, uri: &str) -> Value {
        let body = json!({"jsonrpc": "2.0", "id": 3, "method": "resources/read", "params": {"uri": uri}});
        let (v, _) = post(&self.url, Some(&self.session), body).await;
        serde_json::from_str(v["result"]["contents"][0]["text"].as_str().unwrap_or_else(|| panic!("no contents: {v}"))).unwrap()
    }
}

/// Main as it stands: the project and its undo history.
async fn main_model(app: &AppService) -> (Project, u32, usize) {
    let s = app.session.lock().await;
    let doc = s.doc.as_ref().unwrap();
    (doc.project().clone(), doc.revision(), doc.history().undo.len())
}

/// A sub-agent's call: its handle on every call, as the skill teaches.
fn with(handle: &str, mut args: Value) -> Value {
    args["agent"] = json!(handle);
    args
}

fn handle_of(started: &Value, key: &str) -> (String, String) {
    let h = started["handles"].as_array().unwrap().iter().find(|h| h["key"] == key).unwrap_or_else(|| panic!("no handle for {key}"));
    (h["handle"].as_str().unwrap().to_string(), h["url"].as_str().unwrap().to_string())
}

/// The plan: the ground floor first, the upper floor once the ground
/// floor's walls stand, and the furnishing once both floors are done.
fn start_args() -> Value {
    let all_ground = json!(["wall", "room", "opening", "column", "stair"]);
    json!({
        "action": "start",
        "brief": "A two storey house: living and kitchen below, two bedrooms above, furnished.",
        "summary": "Two storey house, 10 x 8 m",
        "setup": {"levels": [{"name": "Second Floor", "height_mm": 3000}]},
        "parts": [
            {"key": "ground", "name": "Ground floor", "role": "Walls, rooms, doors and windows below",
             "scope": {"levels": ["Ground Floor"], "kinds": all_ground}, "milestones": ["walls"],
             "instructions": "Living 6 x 8 m and kitchen 4 x 8 m, a front door and a window."},
            {"key": "upper", "name": "Upper floor", "role": "The second floor",
             "scope": {"levels": ["Second Floor"], "kinds": ["wall", "room", "opening", "slab"]},
             "after": [{"part": "ground", "milestone": "walls"}],
             "instructions": "Two bedrooms over the ground floor walls."},
            {"key": "furnish", "name": "Furnishing", "role": "Furniture on both floors",
             "scope": {"levels": [], "kinds": ["asset"]},
             "after": [{"part": "ground", "milestone": "done"}, {"part": "upper", "milestone": "done"}],
             "instructions": "A sofa in the living room, a bed in each bedroom."}
        ]
    })
}

#[tokio::test]
async fn a_lead_and_three_handles_draw_a_two_storey_house_into_a_draft() {
    let (app, _dir) = app();
    let port = serve(app.clone()).await;
    let base = format!("http://127.0.0.1:{port}/mcp");

    // The lead, as Claude Code: one connection for it and its sub-agents.
    let lead = connect(&base, "claude-code").await;
    lead.ok("project", json!({"action": "create", "name": "Swarm house", "template": "blank"})).await;
    let (before, revision, steps_before) = main_model(&app).await;

    let started = lead.ok("swarm", start_args()).await;
    assert_eq!(started["you"], "lead");
    assert_eq!(started["handles"].as_array().unwrap().len(), 3, "a handle per part: {started}");
    assert!(started["how_to"].as_str().unwrap().contains("agent: \"<handle>\""));
    let swarm_id = started["swarm_id"].as_str().unwrap().to_string();
    let (ground_h, _) = handle_of(&started, "ground");
    let (upper_h, upper_url) = handle_of(&started, "upper");
    let (furnish_h, _) = handle_of(&started, "furnish");
    assert!(upper_url.ends_with(&format!("/mcp/agent/{upper_h}")), "{upper_url}");

    // Codex style: the upper floor's agent on the part's labelled URL.
    let upper = connect(&upper_url, "codex").await;
    // Someone outside the swarm, on its own connection.
    let outsider = connect(&format!("{base}/agent/outsider"), "cursor").await;

    // The ground floor: walls and rooms, the milestone, then the openings.
    let ground = {
        let lead = lead.clone();
        let h = ground_h.clone();
        tokio::spawn(async move {
            let rooms = json!({"label": "Ground floor", "spec": {"rooms": [
                {"ref": "living", "rect": {"x": 0, "y": 0, "width_mm": 6000, "depth_mm": 8000}, "name": "Living", "usage": "living"},
                {"ref": "kitchen", "rect": {"x": 6000, "y": 0, "width_mm": 4000, "depth_mm": 8000}, "name": "Kitchen", "usage": "kitchen"}
            ]}});
            let built = lead.ok("build", with(&h, rooms)).await;
            assert_eq!(built["agent"], "Ground floor", "shown as its part: {built}");
            lead.ok("swarm", with(&h, json!({"action": "milestone", "name": "walls"}))).await;
            let openings = json!({"label": "Ground floor openings", "spec": {"openings": [
                {"kind": "door", "at": {"x": 3000, "y": 0}},
                {"kind": "window", "at": {"x": 8000, "y": 0}}
            ]}});
            lead.ok("build", with(&h, openings)).await;
            // Its own history is the draft's.
            let history = lead.ok("history", with(&h, json!({"action": "list"}))).await;
            assert_eq!(history["draft"], true);
            assert_eq!(history["undo"][0]["yours"], true, "{history}");
            lead.ok("swarm", with(&h, json!({"action": "finish", "summary": "Living, kitchen, door and window"}))).await
        })
    };

    // The upper floor waits for the ground floor's walls, reads them, and
    // draws only its own level.
    let upper_task = {
        let upper = upper.clone();
        tokio::spawn(async move {
            let waited = upper.ok("swarm", json!({"action": "wait", "timeout_s": 60})).await;
            assert_eq!(waited["reached"], true, "{waited}");
            let below = upper.ok("query", json!({"what": "elements", "kind": "wall", "level": "Ground Floor"})).await;
            assert!(below["count"].as_u64().unwrap() >= 6, "the ground floor's walls are readable: {below}");
            let refused = upper.fail("add_walls", json!({"points": [{"x": 0, "y": -3000}, {"x": 4000, "y": -3000}], "level": "Ground Floor"})).await;
            assert_eq!(refused["code"], "out_of_scope", "{refused}");
            let rooms = json!({"label": "Upper floor", "spec": {"rooms": [
                {"rect": {"x": 0, "y": 0, "width_mm": 5000, "depth_mm": 8000}, "name": "Bedroom 1", "usage": "bedroom", "level": "Second Floor"},
                {"rect": {"x": 5000, "y": 0, "width_mm": 5000, "depth_mm": 8000}, "name": "Bedroom 2", "usage": "bedroom", "level": "Second Floor"}
            ]}});
            let built = upper.ok("build", rooms).await;
            assert_eq!(built["agent"], "Upper floor");
            // A dry run stages on the draft: the wall it names exists only there.
            let window = json!({"kind": "window", "at": {"x": 2500, "y": 0}, "level": "Second Floor"});
            let dry = upper.ok("build", json!({"spec": {"openings": [window]}, "dry_run": true})).await;
            assert_eq!(dry["dry_run"], true, "{dry}");
            upper.ok("add_opening", json!({"kind": "window", "at": {"x": 2500, "y": 0}, "level": "Second Floor"})).await;
            // The draft, not the model, in pictures and resources.
            let plan = upper.ok("look", json!({"level": "Ground Floor"})).await;
            assert_eq!(plan["draft"], true);
            assert!(plan.get("empty").is_none(), "the ground floor shows in the draft: {plan}");
            let current = upper.resource("guhit://project/current").await;
            assert!(current["rooms"].as_array().unwrap().len() >= 4, "{current}");
            // What shows the model is refused.
            assert_eq!(upper.fail("export", json!({"format": "pdf"})).await["code"], "draft_view");
            upper.ok("swarm", json!({"action": "finish", "summary": "Two bedrooms"})).await
        })
    };

    // The furnishing waits for both floors.
    let furnish = {
        let lead = lead.clone();
        let h = furnish_h.clone();
        tokio::spawn(async move {
            let waited = lead.ok("swarm", with(&h, json!({"action": "wait", "timeout_s": 60}))).await;
            assert_eq!(waited["reached"], true, "{waited}");
            let rooms = lead.ok("query", with(&h, json!({"what": "rooms"}))).await;
            assert!(rooms["count"].as_u64().unwrap() >= 4, "both floors' rooms: {rooms}");
            let walls = lead.fail("add_walls", with(&h, json!({"points": [{"x": 0, "y": -3000}, {"x": 4000, "y": -3000}]}))).await;
            assert_eq!(walls["code"], "out_of_scope");
            let assets = json!({"label": "Furniture", "spec": {"assets": [
                {"catalog_key": "sofa-3", "at": {"x": 3000, "y": 4000}},
                {"catalog_key": "bed-double", "at": {"x": 2500, "y": 4000}, "level": "Second Floor"},
                {"catalog_key": "bed-double", "at": {"x": 7500, "y": 4000}, "level": "Second Floor"}
            ]}});
            lead.ok("build", with(&h, assets)).await;
            // Only the lead discards.
            assert_eq!(lead.fail("swarm", with(&h, json!({"action": "discard", "force": true}))).await["code"], "forbidden");
            lead.ok("swarm", with(&h, json!({"action": "finish"}))).await
        })
    };

    // Meanwhile the lead watches and is refused what is not its to do.
    let refused = lead.fail("swarm", json!({"action": "accept"})).await;
    assert_eq!(refused["code"], "people_only");
    assert!(refused["hint"].as_str().unwrap().contains("Accept in Guhit Studio"));
    assert_eq!(lead.fail("look", json!({"mode": "3d"})).await["code"], "draft_view");
    assert_eq!(lead.fail("project", json!({"action": "create", "name": "Another"})).await["code"], "swarm_running");
    assert_eq!(lead.fail("project", json!({"action": "close", "force": true})).await["code"], "swarm_running");
    assert_eq!(lead.fail("project", json!({"action": "save_version", "label": "Mid swarm"})).await["code"], "draft_view");
    assert_eq!(lead.fail("swarm", json!({"action": "discard"})).await["code"], "needs_force");
    let blocked = outsider.fail("add_walls", json!({"points": [{"x": 0, "y": -3000}, {"x": 4000, "y": -3000}]})).await;
    assert_eq!(blocked["code"], "swarm_running");
    assert!(!blocked["hint"].as_str().unwrap().contains("force"), "never the live session's advice: {blocked}");

    let mut tasks = vec![ground, upper_task, furnish];
    let mut all = json!({});
    for _ in 0..12 {
        all = lead.ok("swarm", json!({"action": "wait", "timeout_s": 10})).await;
        if all["reached"] == true {
            break;
        }
        // A sub-agent that failed shows why now, not after every wait.
        let mut i = 0;
        while i < tasks.len() {
            if tasks[i].is_finished() {
                tasks.remove(i).await.unwrap();
            } else {
                i += 1;
            }
        }
    }
    assert_eq!(all["reached"], true, "every part finished: {all}");
    for t in tasks {
        t.await.unwrap();
    }

    // Main never moved; someone outside the swarm still reads it.
    let (during, at, steps) = main_model(&app).await;
    assert_eq!((&during.elements, &during.levels, at, steps), (&before.elements, &before.levels, revision, steps_before), "main is unchanged");
    let outside = outsider.ok("query", json!({"what": "elements"})).await;
    assert_eq!(outside["count"], 0, "an agent outside the swarm reads the model: {outside}");
    assert_eq!(outsider.resource("guhit://project/current").await["rooms"].as_array().unwrap().len(), 0);

    // The lead checks the whole draft and marks it ready.
    let status = lead.ok("swarm", json!({"action": "status"})).await["swarm"].clone();
    assert_eq!(status["parts"].as_array().unwrap().len(), 4, "the lead and three parts");
    for row in status["parts"].as_array().unwrap().iter().skip(1) {
        assert_eq!(row["state"], "done", "{row}");
    }
    assert_eq!(status["draft"]["by_kind"]["asset"], 3, "{status}");
    assert!(status["draft"]["by_kind"]["opening"].as_u64().unwrap() >= 3);
    let draft_walls = lead.ok("query", json!({"what": "elements", "kind": "wall"})).await;
    assert!(draft_walls["count"].as_u64().unwrap() >= 12, "the lead reads the draft: {draft_walls}");
    let done = lead.ok("swarm", json!({"action": "finish", "summary": "House drawn"})).await;
    assert_eq!(done["swarm"]["state"], "ready");
    assert!(done["note"].as_str().unwrap().contains("press Accept"));

    // A person accepts in the window, part by part, through the app's IPC
    // (over MCP an accept stays people_only): each part one step by its
    // agent, the parts it builds on first, as the check foretold.
    assert_eq!(lead.fail("swarm", json!({"action": "accept", "parts": ["ground"]})).await["code"], "people_only");
    let swarm = app.swarm_status().unwrap();
    let agent_of = |key: &str| swarm.parts.iter().find(|p| p.key == key).and_then(|p| p.agent_id.clone());
    let name_of = |key: &str| swarm.parts.iter().find(|p| p.key == key).map(|p| p.name.clone()).unwrap();
    let by_id = |p: &Project| p.elements.iter().map(|e| (e.id().clone(), e.clone())).collect::<BTreeMap<Id, Element>>();
    let view = app.swarm_draft(None).await.unwrap().unwrap().state.project;
    let mut committed: Vec<String> = vec![];
    for key in ["ground", "upper", "furnish"] {
        let open = app.swarm_status().expect("the swarm stays open until its last part");
        let reviewed: BTreeMap<String, u32> = open.parts.iter().map(|p| (p.key.clone(), p.last_seq)).collect();
        let check: SwarmAcceptResult = serde_json::from_value(app.handle("swarm_check", json!({"parts": [key]})).await.unwrap()).unwrap();
        assert!(check.skipped.is_empty(), "{key}: {:?}", check.skipped);
        let args = json!({"swarm_id": swarm_id, "parts": [key], "reviewed": reviewed});
        let done: SwarmAcceptResult = serde_json::from_value(app.handle("swarm_accept", args).await.unwrap()).unwrap();
        let parts: Vec<String> = done.applied.iter().map(|a| a.part.clone()).collect();
        assert_eq!(parts, check.applied.iter().map(|a| a.part.clone()).collect::<Vec<_>>(), "{key}: the check foretold it");
        assert_eq!(parts.last().map(String::as_str), Some(key), "{key} commits after what it builds on: {parts:?}");
        if let Some(st) = app.swarm_status() {
            assert_eq!(st.parts.iter().find(|p| p.key == key).unwrap().state, SwarmPartState::Accepted, "{key}");
        }
        committed.extend(parts);
    }
    let at = |key: &str| committed.iter().position(|k| k == key).unwrap_or_else(|| panic!("{key} committed: {committed:?}"));
    let mut once = committed.clone();
    once.sort();
    assert_eq!(once, ["furnish", "ground", "lead", "upper"], "every part once");
    assert!(at("lead") < at("upper") && at("ground") < at("upper") && at("upper") < at("furnish"), "{committed:?}");
    assert!(app.swarm_status().is_none(), "the swarm ends with its last part");
    let (after, _, _) = main_model(&app).await;
    assert_eq!(by_id(&after), by_id(&view), "the model is the draft, ids and all");
    assert_eq!(after.levels.len(), 2);
    let history: HistoryList = serde_json::from_value(app.handle("history_list", json!({})).await.unwrap()).unwrap();
    let newest: Vec<&HistoryStep> = history.undo.iter().take(committed.len()).collect();
    let labels: Vec<String> = newest.iter().map(|s| s.label.clone()).collect();
    assert_eq!(labels, committed.iter().rev().map(|k| format!("Swarm: {}", name_of(k))).collect::<Vec<_>>());
    let authors: Vec<Option<Id>> = newest.iter().map(|s| s.author_id.clone()).collect();
    assert_eq!(authors, committed.iter().rev().map(|k| agent_of(k)).collect::<Vec<_>>());
    assert!(authors.iter().all(Option::is_some));
    assert!(newest.iter().all(|s| s.origin == Origin::Ai));

    // The swarm is over: a late sub-agent is told so, and nothing lands.
    let late = lead.fail("add_walls", with(&ground_h, json!({"points": [{"x": 0, "y": -3000}, {"x": 4000, "y": -3000}]}))).await;
    assert_eq!(late["code"], "part_stopped");
    assert_eq!(lead.ok("swarm", json!({"action": "status"})).await["swarm"], Value::Null);
}

/// A part's `history` acts on its own draft steps only and never reaches
/// the model's undo (without the routing, several steps went to
/// `doc_undo`): undo takes back the part's newest draft step and nothing of
/// another part's, and a step another part built on is refused
/// `other_author`, `force` or not.
#[tokio::test]
async fn a_parts_history_takes_back_only_its_own_draft_steps() {
    let (app, _dir) = app();
    let port = serve(app.clone()).await;
    let base = format!("http://127.0.0.1:{port}/mcp");
    let lead = connect(&base, "claude-code").await;
    lead.ok("project", json!({"action": "create", "name": "History", "template": "blank"})).await;
    let wall = Command::AddWall {
        start: Point { x: 0.0, y: -5000.0 },
        end: Point { x: 4000.0, y: -5000.0 },
        thickness_mm: None,
        height_mm: None,
        material_id: None,
        level_id: None,
        role: None,
    };
    app.handle("doc_apply", json!({"command": wall})).await.unwrap();
    let (before, revision, steps) = main_model(&app).await;
    let parts = json!([
        {"key": "shell", "name": "Shell", "scope": {"kinds": ["wall"]}},
        {"key": "rooms", "name": "Rooms", "scope": {"kinds": ["opening"]}}
    ]);
    let started = lead.ok("swarm", json!({"action": "start", "brief": "Walls and a door", "parts": parts})).await;
    // Each part on its own labelled URL, so each has its own rate.
    let shell = connect(&handle_of(&started, "shell").1, "codex").await;
    let rooms = connect(&handle_of(&started, "rooms").1, "codex").await;
    let draft = || async { app.swarm_draft(None).await.unwrap().unwrap() };
    let model_unchanged = |what: &str| {
        let what = what.to_string();
        let app = app.clone();
        let before = before.clone();
        async move {
            let (now, at, n) = main_model(&app).await;
            assert_eq!((&now.elements, at, n), (&before.elements, revision, steps), "{what} moved the model");
        }
    };

    // Shell draws wall A, Rooms puts a door in it, Shell draws wall B.
    shell.ok("add_walls", json!({"points": [{"x": 0, "y": 0}, {"x": 4000, "y": 0}]})).await;
    rooms.ok("add_opening", json!({"kind": "door", "at": {"x": 2000, "y": 0}})).await;
    shell.ok("add_walls", json!({"points": [{"x": 0, "y": 3000}, {"x": 4000, "y": 3000}]})).await;
    let drawn = draft().await;
    assert_eq!(drawn.steps.iter().map(|s| s.part.as_str()).collect::<Vec<_>>(), ["shell", "rooms", "shell"]);
    let list = shell.ok("history", json!({"action": "list"})).await;
    assert_eq!(list["draft"], true);
    assert_eq!(list["undo_count"], 3, "the draft's steps, not the model's history: {list}");
    let yours: Vec<bool> = list["undo"].as_array().unwrap().iter().map(|r| r["yours"].as_bool().unwrap()).collect();
    assert_eq!(yours, [true, false, true], "newest first: {list}");

    // Undo takes back Shell's newest step, wall B, and nothing else.
    let undone = shell.ok("history", json!({"action": "undo"})).await;
    assert_eq!((undone["draft"].clone(), undone["steps"].clone()), (json!(true), json!(1)), "{undone}");
    let after = draft().await;
    assert_eq!(after.steps.iter().map(|s| (s.part.as_str(), s.seq)).collect::<Vec<_>>(), [("shell", drawn.steps[0].seq), ("rooms", drawn.steps[1].seq)]);
    let gone: Vec<&Id> = drawn.steps[2].added.iter().filter(|id| !after.state.project.elements.iter().any(|e| e.id() == *id)).collect();
    assert_eq!(gone.len(), drawn.steps[2].added.len(), "wall B left the draft");
    assert!(drawn.steps[1].added.iter().all(|id| after.state.project.elements.iter().any(|e| e.id() == id)), "Rooms' door stays");
    model_unchanged("undo").await;

    // Wall A carries Rooms' door: refused, force or not, and so is undoing
    // back over Rooms' step.
    for args in [json!({"action": "undo"}), json!({"action": "undo", "force": true}), json!({"action": "undo_to", "index": 1})] {
        let refused = shell.fail("history", args.clone()).await;
        assert_eq!(refused["code"], "other_author", "{args}: {refused}");
        assert_eq!(draft().await.steps.len(), 2, "{args} took nothing back");
        model_unchanged(&args.to_string()).await;
    }

    // Rooms takes back its own door; then wall A has nothing on it and goes.
    rooms.ok("history", json!({"action": "undo"})).await;
    shell.ok("history", json!({"action": "undo"})).await;
    assert!(draft().await.steps.is_empty(), "every step taken back by its own part");
    model_unchanged("the last undos").await;
    // The lead's own history is the draft's too; the person's step is safe.
    assert_eq!(lead.ok("history", json!({"action": "list"})).await["draft"], true);
}

/// A part's history never reaches the model's undo, whatever it asks.
#[tokio::test]
async fn a_parts_history_never_undoes_the_model() {
    let (app, _dir) = app();
    let port = serve(app.clone()).await;
    let lead = connect(&format!("http://127.0.0.1:{port}/mcp"), "claude-code").await;
    lead.ok("project", json!({"action": "create", "name": "History", "template": "blank"})).await;
    // A person's step on the model before the swarm.
    let wall = Command::AddWall {
        start: Point { x: 0.0, y: -5000.0 },
        end: Point { x: 4000.0, y: -5000.0 },
        thickness_mm: None,
        height_mm: None,
        material_id: None,
        level_id: None,
        role: None,
    };
    app.handle("doc_apply", json!({"command": wall})).await.unwrap();
    let (before, revision, steps) = main_model(&app).await;
    let started = lead
        .ok("swarm", json!({"action": "start", "brief": "Walls", "parts": [{"key": "shell", "name": "Shell", "scope": {"kinds": ["wall"]}}]}))
        .await;
    let (h, _) = handle_of(&started, "shell");
    for y in [0, 2000] {
        lead.ok("add_walls", with(&h, json!({"points": [{"x": 0, "y": y}, {"x": 4000, "y": y}]}))).await;
    }
    let list = lead.ok("history", with(&h, json!({"action": "list"}))).await;
    assert_eq!(list["draft"], true);
    assert_eq!(list["undo_count"], 2, "the part's two draft steps, not the model's history: {list}");
    for args in [json!({"action": "undo", "steps": 2, "force": true}), json!({"action": "undo_to", "index": 1}), json!({"action": "undo"})] {
        let r = lead.call("history", with(&h, args.clone())).await;
        if r["isError"] == true {
            assert_ne!(r["structuredContent"]["code"], "swarm_running", "{args} reached the model's undo: {r}");
        } else {
            assert_eq!(r["structuredContent"]["draft"], true, "{r}");
        }
        let (now, at, n) = main_model(&app).await;
        assert_eq!((&now.elements, at, n), (&before.elements, revision, steps), "{args} moved the model");
    }
    // The lead's own history is the draft's too; the person's step is safe.
    assert_eq!(lead.ok("history", json!({"action": "list"})).await["draft"], true);
}

/// `wait` is a long poll: it answers when the milestone lands, not at its
/// timeout, and says when it gave up.
#[tokio::test]
async fn wait_answers_when_the_milestone_lands() {
    let (app, _dir) = app();
    let port = serve(app.clone()).await;
    let lead = connect(&format!("http://127.0.0.1:{port}/mcp"), "claude-code").await;
    lead.ok("project", json!({"action": "create", "name": "Wait", "template": "blank"})).await;
    let started = lead
        .ok("swarm", json!({"action": "start", "brief": "Walls then rooms", "parts": [
            {"key": "shell", "name": "Shell", "scope": {"kinds": ["wall"]}, "milestones": ["walls"]},
            {"key": "rooms", "name": "Rooms", "scope": {"kinds": ["room"]}, "after": [{"part": "shell", "milestone": "walls"}]}
        ]}))
        .await;
    let (shell, _) = handle_of(&started, "shell");
    let (rooms, _) = handle_of(&started, "rooms");
    let early = lead.ok("swarm", with(&rooms, json!({"action": "wait", "timeout_s": 0.2}))).await;
    assert_eq!((early["reached"].clone(), early["timed_out"].clone()), (json!(false), json!(true)), "{early}");
    let unknown = lead.fail("swarm", with(&shell, json!({"action": "milestone", "name": "roof"}))).await;
    assert_eq!(unknown["code"], "bad_args");
    let waiter = {
        let lead = lead.clone();
        let rooms = rooms.clone();
        tokio::spawn(async move {
            let t = std::time::Instant::now();
            let v = lead.ok("swarm", with(&rooms, json!({"action": "wait", "timeout_s": 30}))).await;
            (v, t.elapsed())
        })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    lead.ok("swarm", with(&shell, json!({"action": "milestone", "name": "walls"}))).await;
    let (v, took) = waiter.await.unwrap();
    assert_eq!(v["reached"], true, "{v}");
    assert!(took < Duration::from_secs(10), "it answered when the milestone landed, after {took:?}");
    let named = lead.ok("swarm", json!({"action": "wait", "for": {"part": "shell", "milestone": "walls"}, "timeout_s": 1})).await;
    assert_eq!(named["reached"], true);
    assert_eq!(lead.fail("swarm", json!({"action": "wait", "for": {"part": "nope"}})).await["code"], "bad_args");
    // The lead discards with force, after the user agreed: the model is free.
    lead.ok("swarm", json!({"action": "discard", "force": true})).await;
    assert!(app.swarm_status().is_none());
    assert_eq!(lead.ok("swarm", json!({"action": "status"})).await["swarm"], Value::Null);
    // A handle of a swarm that ended binds nothing and writes nothing.
    assert_eq!(lead.fail("swarm", with(&rooms, json!({"action": "status"}))).await["code"], "part_stopped");
}
