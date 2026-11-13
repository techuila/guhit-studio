//! Gate round 2: dry runs meet the registry's checks, claiming is in the
//! schema, agents read chat and their inbox, an anonymous call is nobody's,
//! and pictures say what they show and when the window is busy.

use std::future::Future;

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
    CallContext::agent(AgentHint { connection: Some(format!("label:{label}")), label: Some(label.to_string()), ..AgentHint::default() })
}

async fn call(app: &AppService, ctx: &CallContext, name: &str, args: Value) -> Result<Value, ToolFail> {
    match tools::call_with(app, name, args, ctx).await? {
        Output::Json(v) | Output::Rich { json: v, .. } => Ok(v),
        Output::Text(t) => Ok(json!({ "text": t })),
    }
}

async fn house(app: &AppService) {
    tools::call(app, "project", json!({"action": "create", "name": "Round 2", "template": "blank"})).await.unwrap_or_else(|f| panic!("{}", f.display()));
    tools::call(app, "levels", json!({"action": "add", "name": "Upper"})).await.unwrap_or_else(|f| panic!("{}", f.display()));
}

#[tokio::test]
async fn a_dry_run_is_refused_where_the_commit_would_be() {
    let (app, _dir) = app();
    house(&app).await;
    let (upper, ground) = (agent("upper"), agent("ground"));
    call(&app, &upper, "set_status", json!({"text": "Upper floor", "claim": {"levels": ["Upper"]}})).await.unwrap();
    let spec = json!({"dry_run": true, "spec": {"walls": [{"points": [[0, 0], [4000, 0]], "level": "Upper"}]}});
    let dry = call(&app, &ground, "build", spec.clone()).await.unwrap_err();
    assert_eq!(dry.code, "claimed", "{}", dry.display());
    assert!(dry.text().starts_with("dry run:"), "{}", dry.text());
    // On its own level the dry run passes, and a paused agent's does not.
    let own = json!({"dry_run": true, "spec": {"walls": [{"points": [[0, 0], [4000, 0]]}]}});
    assert_eq!(call(&app, &ground, "build", own.clone()).await.unwrap()["dry_run"], true);
    let id = app.agents_list().into_iter().find(|a| a.participant.name == "Ground").unwrap().participant.id;
    app.handle("agent_pause", json!({"id": id})).await.unwrap();
    assert_eq!(call(&app, &ground, "batch", json!({"dry_run": true, "steps": [{"tool": "add_walls", "args": {"points": [[0, 0], [1000, 0]]}}]})).await.unwrap_err().code, "agent_paused");
}

#[test]
fn claiming_is_in_the_schema_and_taught() {
    let defs = tools::definitions();
    let status = defs.iter().find(|d| d.name == "set_status").unwrap();
    assert!(status.schema["properties"]["claim"]["properties"]["levels"].is_object());
    assert_eq!(status.schema["properties"]["release"]["type"], "boolean");
    assert!(status.description.contains("claim your level first"));
    let guide = tools::definitions().iter().find(|d| d.name == "get_session").unwrap().description.clone();
    assert!(guide.contains("with or without a live session"));
}

#[tokio::test]
async fn agents_read_chat_without_a_session_and_get_mentions_in_their_inbox() {
    let (app, _dir) = app();
    house(&app).await;
    let (main, bedroom) = (agent("main floor"), agent("bedroom floor"));
    // Bedroom's first call starts its inbox.
    call(&app, &bedroom, "query", json!({"what": "summary"})).await.unwrap();
    call(&app, &main, "send_chat_message", json!({"text": "@bedroom-floor please cut the void over the living room"})).await.unwrap();
    call(&app, &main, "send_chat_message", json!({"text": "note to self"})).await.unwrap();
    let next = call(&app, &bedroom, "query", json!({"what": "levels"})).await.unwrap();
    let inbox = next["inbox"].as_array().expect("an inbox");
    assert_eq!(inbox.len(), 1, "only the message naming Bedroom floor: {inbox:?}");
    assert!(inbox[0]["text"].as_str().unwrap().contains("cut the void"));
    assert!(call(&app, &bedroom, "query", json!({"what": "levels"})).await.unwrap().get("inbox").is_none(), "read once");
    // get_session reads the project chat with no live session.
    let session = call(&app, &bedroom, "get_session", json!({})).await.unwrap();
    assert_eq!(session["mode"], "off");
    let chat = session["recent_chat"].as_array().unwrap();
    assert_eq!(chat.len(), 2);
    assert_eq!(chat[0]["to_you"], true);
    assert!(session["agents"].as_array().unwrap().iter().any(|a| a["you"] == true && a["activity"].is_string()));
}

// ------------------------------------------------------ anonymous calls

async fn post(url: &str, body: Value) -> Value {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let res = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(body.to_string())
        .send()
        .await
        .unwrap();
    serde_json::from_str(&res.text().await.unwrap()).unwrap()
}

#[tokio::test]
async fn a_plain_call_after_a_labelled_client_is_not_that_client() {
    let (app, _dir) = app();
    house(&app).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn({
        let app = app.clone();
        async move { axum::serve(listener, guhit_mcp::router(app)).await }
    });
    let base = format!("http://127.0.0.1:{port}/mcp");
    post(&format!("{base}/agent/upper"), json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "codex", "version": "1"}}})).await;
    post(&format!("{base}/agent/upper"), json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "add_walls", "arguments": {"points": [[0, 0], [3000, 0]]}}})).await;
    // A plain call, no initialize, no session, no label.
    let out = post(&base, json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "add_walls", "arguments": {"points": [[0, 2000], [3000, 2000]]}}})).await;
    assert_eq!(out["result"]["isError"], false, "{out}");
    let history: HistoryList = serde_json::from_value(app.handle("history_list", json!({})).await.unwrap()).unwrap();
    let names: Vec<Option<String>> = history.undo.iter().take(2).map(|s| s.author_name.clone()).collect();
    assert_eq!(names[1].as_deref(), Some("Upper"));
    assert_ne!(names[0].as_deref(), Some("Upper"), "the plain call is not Upper's");
    assert!(app.agents_list().iter().all(|a| a.participant.agent.as_ref().map(|i| i.client.as_str()) != Some("codex") || a.participant.name == "Upper"));
}

// ------------------------------------------------- pictures and renders

fn fake_window<F, Fut>(app: &AppService, answer: F) -> tokio::task::JoinHandle<()>
where
    F: Fn(WindowRequest) -> Fut + Send + 'static,
    Fut: Future<Output = Result<WindowReply, IpcError>> + Send,
{
    let mut events = app.events();
    let app = app.clone();
    tokio::spawn(async move {
        while let Ok(event) = events.recv().await {
            let AppEvent::WindowRequest { request } = event else { continue };
            let (reply, error) = match answer(request.clone()).await {
                Ok(r) => (json!(r), Value::Null),
                Err(e) => (Value::Null, json!(e)),
            };
            let _ = app.handle("window_reply", json!({"id": request.id, "reply": reply, "error": error})).await;
        }
    })
}

#[tokio::test]
async fn pictures_carry_their_visibility_and_a_busy_window_says_when_to_retry() {
    let (app, _dir) = app();
    house(&app).await;
    let f = tools::call(&app, "look", json!({"mode": "plan", "visibility": {"roof": true}})).await.err().unwrap();
    assert_eq!(f.code, "bad_args");
    let f = tools::call(&app, "look", json!({"mode": "3d", "visibility": {"walls": false}})).await.err().unwrap();
    assert_eq!(f.code, "bad_args");
    // The window gets what the call asked for, None when it named nothing.
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let window = fake_window(&app, {
        let seen = seen.clone();
        move |request| {
            let seen = seen.clone();
            async move {
                match request.task {
                    WindowTask::Render { visibility, .. } => {
                        seen.lock().unwrap().push(visibility);
                        Err(IpcError::new("invalid", "A render is already running in Guhit Studio. Wait for it or stop it, then try again."))
                    }
                    WindowTask::CaptureView { visibility, .. } => {
                        seen.lock().unwrap().push(visibility);
                        Ok(WindowReply { render_ids: vec![], image: None, note: "Captured.".into() })
                    }
                    _ => Err(IpcError::new("invalid", "not this")),
                }
            }
        }
    });
    let shot = |args: Value| {
        let app = app.clone();
        async move {
            match tools::call(&app, "look", args).await {
                Ok(Output::Json(v) | Output::Rich { json: v, .. }) => v,
                Ok(Output::Text(_)) => panic!("text"),
                Err(f) => panic!("{}", f.display()),
            }
        }
    };
    let roof_off = shot(json!({"mode": "3d", "visibility": {"roof": false}})).await;
    assert_eq!(roof_off["visibility"], "roof off, no cutaway");
    let plain = shot(json!({"mode": "3d"})).await;
    assert!(plain["visibility"].as_str().unwrap().contains("default"));
    // A busy window: window_busy, with when to try again.
    let busy = tools::call(&app, "render", json!({"action": "start", "wait_seconds": 5, "visibility": {"cutaway": true}})).await.err().unwrap();
    assert_eq!(busy.code, "window_busy", "{}", busy.display());
    assert!(busy.message.contains("Try again in about"), "{}", busy.message);
    assert!(busy.hint().is_some());
    window.abort();
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen, vec![
        Some(ViewVisibility { roof: false, cutaway: false }),
        None,
        Some(ViewVisibility { roof: true, cutaway: true }),
    ]);
}
