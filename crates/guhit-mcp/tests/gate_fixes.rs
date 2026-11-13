//! Fixes from the Wave 1 gate review: sessions keep their clients, imports
//! commit as the agent, multi-step undo checks every step first, chat is
//! rate limited, and a failing step is named once.

use base64::Engine as _;
use guhit_app::agents::AgentHint;
use guhit_app::AppService;
use guhit_mcp::agent::CallContext;
use guhit_mcp::fail::ToolFail;
use guhit_mcp::tools::{self, Output};
use guhit_model::*;
use serde_json::{json, Value};

fn app() -> (AppService, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    (AppService::new_sandboxed(dir.path().to_path_buf()), dir)
}

fn agent(label: &str) -> CallContext {
    CallContext::agent(AgentHint {
        connection: Some(format!("label:{label}")),
        label: Some(label.to_string()),
        ..AgentHint::default()
    })
}

async fn as_agent(app: &AppService, ctx: &CallContext, name: &str, args: Value) -> Result<Value, ToolFail> {
    match tools::call_with(app, name, args, ctx).await? {
        Output::Json(v) | Output::Rich { json: v, .. } => Ok(v),
        Output::Text(t) => Ok(json!({ "text": t })),
    }
}

async fn blank(app: &AppService) {
    tools::call(app, "project", json!({"action": "create", "name": "Gate", "template": "blank"})).await.unwrap_or_else(|f| panic!("project: {}", f.display()));
}

fn revision(app: &AppService) -> u32 {
    app.session.try_lock().unwrap().doc.as_ref().unwrap().revision()
}

// --------------------------------------------------------- 1. sessions

async fn post(url: &str, body: Value, session: Option<&str>) -> (Value, Option<String>) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut req = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(body.to_string());
    if let Some(s) = session {
        req = req.header("mcp-session-id", s);
    }
    let res = req.send().await.expect("request");
    let issued = res.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()).map(str::to_string);
    (serde_json::from_str(&res.text().await.unwrap()).unwrap(), issued)
}

fn init(client: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": client, "version": "1"}}})
}

fn tool(name: &str, args: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": name, "arguments": args}})
}

async fn serve(app: AppService) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, guhit_mcp::router(app)).await });
    format!("http://127.0.0.1:{port}/mcp")
}

#[tokio::test]
async fn each_session_keeps_its_own_client_and_nobody_borrows_the_last_one() {
    let (app, _dir) = app();
    blank(&app).await;
    let url = serve(app.clone()).await;
    // Two unlabelled clients on one URL: each gets its own session.
    let (_, claude) = post(&url, init("claude-code"), None).await;
    let (_, codex) = post(&url, init("codex"), None).await;
    let (claude, codex) = (claude.expect("a session for claude-code"), codex.expect("a session for codex"));
    assert_ne!(claude, codex);
    // Claude Code's later call, without clientInfo, after Codex initialized.
    let (out, _) = post(&url, tool("query", json!({"what": "summary"})), Some(&claude)).await;
    assert_eq!(out["result"]["isError"], false);
    post(&url, tool("query", json!({"what": "summary"})), Some(&codex)).await;
    let clients: Vec<String> = app.agents_list().iter().filter_map(|a| a.participant.agent.as_ref().map(|i| i.client.clone())).collect();
    assert!(clients.contains(&"claude-code".to_string()) && clients.contains(&"codex".to_string()), "{clients:?}");
    // A call with no session and no label is never credited to codex.
    post(&url, tool("query", json!({"what": "summary"})), None).await;
    let unknown = app.agents_list().into_iter().filter(|a| a.participant.agent.as_ref().is_some_and(|i| i.client.is_empty())).count();
    assert_eq!(unknown, 1, "the anonymous call is its own unknown agent, not codex");

    // After a restart (another server, another registry) the session still names its client.
    let (other, _dir2) = self::app();
    blank(&other).await;
    let url2 = serve(other.clone()).await;
    post(&url2, tool("query", json!({"what": "summary"})), Some(&claude)).await;
    assert_eq!(other.agents_list()[0].participant.agent.as_ref().unwrap().client, "claude-code");
}

// ----------------------------------------------------------- 2. import

fn two_room_dxf() -> String {
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64| {
        let mut s = String::from("  0\nLWPOLYLINE\n  8\nA-WALL\n 90\n4\n 70\n1\n");
        for (x, y) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
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

#[tokio::test]
async fn an_import_commits_as_the_agent_and_a_paused_agent_cannot_import() {
    let (app, _dir) = app();
    blank(&app).await;
    let ctx = agent("tracer");
    let data = format!("data:application/dxf;base64,{}", base64::engine::general_purpose::STANDARD.encode(two_room_dxf()));
    let args = json!({"action": "commit", "file_name": "site.dxf", "data": data, "mm_per_unit": 1});
    let out = as_agent(&app, &ctx, "import_dxf", args.clone()).await.expect("import");
    assert_eq!(out["walls_added"], 7);
    assert_eq!(out["undo_label"], "MCP: import site.dxf");
    let history: HistoryList = serde_json::from_value(app.handle("history_list", json!({})).await.unwrap()).unwrap();
    let me = app.agents_list().into_iter().find(|a| a.participant.name == "Tracer").unwrap();
    assert_eq!(history.undo[0].author_id.as_deref(), Some(me.participant.id.as_str()), "the step is the agent's, not the person's");
    app.handle("agent_pause", json!({"id": me.participant.id})).await.unwrap();
    let paused = as_agent(&app, &ctx, "import_dxf", args).await.unwrap_err();
    assert_eq!(paused.code, "agent_paused");
    // A DWG is the app's converter's: said plainly.
    app.handle("agent_resume", json!({"id": me.participant.id})).await.unwrap();
    let dwg = as_agent(&app, &ctx, "import_dxf", json!({"action": "commit", "file_name": "a.dwg", "data": data, "mm_per_unit": 1})).await.unwrap_err();
    assert_eq!(dwg.code, "unavailable");
}

// ---------------------------------------------------------- 3. history

#[tokio::test]
async fn a_multi_step_undo_checks_every_step_first_and_is_one_revision() {
    let (app, _dir) = app();
    blank(&app).await;
    let (a, b) = (agent("ground"), agent("upper"));
    let wall = |y: i32| json!({"points": [[0, y], [3000, y]]});
    as_agent(&app, &a, "add_walls", wall(0)).await.unwrap();
    as_agent(&app, &b, "add_walls", wall(2000)).await.unwrap();
    as_agent(&app, &a, "add_walls", wall(4000)).await.unwrap();
    let before = revision(&app);
    // Three steps back reach the other agent's step: nothing is undone.
    let refused = as_agent(&app, &a, "history", json!({"action": "undo", "steps": 3})).await.unwrap_err();
    assert_eq!(refused.code, "other_author");
    assert!(refused.message.contains("step 1"), "{}", refused.message);
    assert_eq!(revision(&app), before, "nothing was undone");
    let refused = as_agent(&app, &a, "history", json!({"action": "undo_to", "index": 1})).await.unwrap_err();
    assert_eq!(refused.code, "other_author");
    assert_eq!(revision(&app), before);
    // Its own two steps on top go back as one revision.
    as_agent(&app, &a, "add_walls", wall(6000)).await.unwrap();
    let r = revision(&app);
    let out = as_agent(&app, &a, "history", json!({"action": "undo", "steps": 2})).await.unwrap();
    assert_eq!(out["revision"], r + 1, "one revision for both steps");
    let walls = app.session.try_lock().unwrap().doc.as_ref().unwrap().project().elements.len();
    assert_eq!(walls, 2, "the two lower walls stay");
}

// ------------------------------------------------------ 4, 5 and chat

#[tokio::test]
async fn chat_is_described_as_the_agent_and_rate_limited() {
    let defs = tools::definitions();
    let chat = defs.iter().find(|d| d.name == "send_chat_message").unwrap();
    assert!(!chat.description.contains("Needs a live session"));
    assert!(!chat.description.contains("as the user of this computer"));
    let (app, _dir) = app();
    blank(&app).await;
    let ctx = agent("talker");
    for n in 0..6 {
        as_agent(&app, &ctx, "send_chat_message", json!({"text": format!("note {n}")})).await.expect("within the rate");
    }
    let limited = as_agent(&app, &ctx, "send_chat_message", json!({"text": "one too many"})).await.unwrap_err();
    assert_eq!(limited.code, "rate_limited");
}

#[tokio::test]
async fn a_failing_step_is_named_once() {
    let (app, _dir) = app();
    blank(&app).await;
    let f = match tools::call(&app, "batch", json!({"steps": [
        {"tool": "add_walls", "args": {"points": [[0, 0], [3000, 0]]}},
        {"tool": "add_opening", "args": {"kind": "door", "wall_id": "$nope"}}
    ]}))
    .await
    {
        Err(f) => f,
        Ok(_) => panic!("the batch should fail"),
    };
    assert_eq!(f.code, "unknown_ref");
    assert_eq!(f.step, Some(2));
    let text = f.text();
    assert!(text.starts_with("step 2 of 2 (`add_opening`): unknown_ref:"), "{text}");
    assert_eq!(text.matches("step 2").count(), 1, "{text}");
}
