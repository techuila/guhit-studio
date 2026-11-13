//! Lane L7: one test for each agent problem the Wave 1 senior architect
//! review (G4) found that MCP answers for: 3 (undo across other agents'
//! steps asks first), 5 (an anonymous call never borrows a name), 6 (the
//! claim is in the `set_status` schema), 7 (`look` uses the saved camera),
//! 8 (the render queue says what it is doing). Problem 10 (idle labels, PH
//! titles) is in the window: `src/live/format.test.ts` and
//! `src/live/agentLabels.test.ts`.

use std::future::Future;
use std::sync::{Arc, Mutex};

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

async fn local(app: &AppService, name: &str, args: Value) -> Value {
    call(app, &CallContext::local(), name, args).await.unwrap_or_else(|f| panic!("`{name}`: {}", f.display()))
}

/// The history with each step's author, as the History panel reads it.
async fn history(app: &AppService) -> HistoryList {
    serde_json::from_value(app.handle("history_list", json!({})).await.unwrap()).unwrap()
}

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

/// G4 problem 3: taking back several steps names another agent's step among
/// them and takes back nothing; the user's answer (`force`) is needed for a
/// person's step, and another agent's is never an agent's to take.
#[tokio::test]
async fn g4_3_undo_across_another_agents_steps_asks_first() {
    let (app, _dir) = app();
    local(&app, "project", json!({"action": "create", "name": "G4 3", "template": "blank"})).await;
    let (ground, upper) = (agent("ground"), agent("upper"));
    call(&app, &ground, "add_walls", json!({"points": [[0, 0], [4000, 0]]})).await.unwrap();
    call(&app, &upper, "add_walls", json!({"points": [[0, 3000], [4000, 3000]]})).await.unwrap();
    call(&app, &ground, "add_walls", json!({"points": [[0, 6000], [4000, 6000]]})).await.unwrap();
    let before = history(&app).await.undo.len();
    let refused = call(&app, &ground, "history", json!({"action": "undo", "steps": 3})).await.unwrap_err();
    assert_eq!(refused.code, "other_author", "{}", refused.display());
    assert!(refused.message.contains("Upper"), "names whose step it is: {}", refused.message);
    assert!(refused.hint().unwrap().contains("Ask the user"));
    assert_eq!(history(&app).await.undo.len(), before, "nothing was taken back");
    // Its own step alone goes.
    call(&app, &ground, "history", json!({"action": "undo"})).await.unwrap();
    assert_eq!(history(&app).await.undo.len(), before - 1);
}

/// G4 problem 5: a call that names no client, session, label or instance is
/// nobody's: it never edits as the agent that called before it.
#[tokio::test]
async fn g4_5_an_anonymous_call_never_borrows_another_agents_name() {
    let (app, _dir) = app();
    local(&app, "project", json!({"action": "create", "name": "G4 5", "template": "blank"})).await;
    let codex = CallContext::agent(AgentHint {
        connection: Some("label:upper".into()),
        label: Some("upper".into()),
        client_name: Some("codex".into()),
        ..AgentHint::default()
    });
    call(&app, &codex, "add_walls", json!({"points": [[0, 0], [3000, 0]]})).await.unwrap();
    let anonymous = CallContext::agent(AgentHint::default());
    call(&app, &anonymous, "add_walls", json!({"points": [[0, 2000], [3000, 2000]]})).await.unwrap();
    let steps = history(&app).await.undo;
    assert_eq!(steps[1].author_name.as_deref(), Some("Upper"));
    assert_ne!(steps[0].author_name.as_deref(), Some("Upper"), "the anonymous step is not Upper's");
    assert_ne!(steps[0].author_id, steps[1].author_id);
}

/// G4 problem 6: a client that validates arguments against the schema can
/// send a claim.
#[test]
fn g4_6_the_claim_is_in_the_set_status_schema() {
    let defs = tools::definitions();
    let status = defs.iter().find(|d| d.name == "set_status").unwrap();
    let props = &status.schema["properties"];
    assert_eq!(props["claim"]["type"], "object");
    assert_eq!(props["claim"]["properties"]["levels"]["type"], "array");
    assert_eq!(props["claim"]["properties"]["ids"]["type"], "array");
    assert_eq!(props["release"]["type"], "boolean");
    assert_eq!(status.schema["additionalProperties"], false, "and nothing else");
}

fn saved_view() -> Camera {
    Camera {
        id: String::new(),
        name: "Double height living".into(),
        preset: CameraPreset::Custom,
        position: Vec3 { x: 3200.0, y: 1200.0, z: 1500.0 },
        target: Vec3 { x: 3200.0, y: 6000.0, z: 2400.0 },
        fov_deg: 60.0,
        light: None,
    }
}

/// G4 problem 7: `look` mode 3d of a saved view hands the window that
/// view's camera as the engine holds it, and says whether the picture came
/// from it.
#[tokio::test]
async fn g4_7_look_uses_the_saved_camera_and_checks_it() {
    let (app, _dir) = app();
    local(&app, "project", json!({"action": "create", "name": "G4 7", "template": "sample-bungalow"})).await;
    app.handle("doc_apply", json!({"command": Command::AddElement { element: Element::Camera(saved_view()) }})).await.unwrap();
    let seen: Arc<Mutex<Vec<WindowTask>>> = Arc::default();
    // The window answers with a record whose camera is the one it was given,
    // or one that drifted (the Wave 1 bug: 3402, 1320, 2674).
    let drift = Arc::new(Mutex::new(false));
    let window = fake_window(&app, {
        let (seen, drift, app) = (seen.clone(), drift.clone(), app.clone());
        move |request| {
            let (seen, drift, app) = (seen.clone(), drift.clone(), app.clone());
            async move {
                seen.lock().unwrap().push(request.task.clone());
                let WindowTask::CaptureView { camera: Some(mut camera), .. } = request.task else {
                    return Err(IpcError::new("invalid", "expected a camera"));
                };
                if *drift.lock().unwrap() {
                    camera.position = Vec3 { x: 3402.0, y: 1320.0, z: 2674.0 };
                }
                let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
                let record: RenderRecord = serde_json::from_value(app.handle("render_capture", json!({"camera": camera, "png": png})).await?).unwrap();
                Ok(WindowReply { render_ids: vec![record.id], image: None, note: "Captured.".into() })
            }
        }
    });
    let shot = local(&app, "look", json!({"mode": "3d", "view": "double height LIVING"})).await;
    let task = seen.lock().unwrap()[0].clone();
    let WindowTask::CaptureView { camera_id, camera: Some(camera), .. } = task else { panic!("{task:?}") };
    assert_eq!(camera_id, None, "the camera itself, not an id the window looks up");
    assert_eq!((camera.position, camera.target, camera.fov_deg), (saved_view().position, saved_view().target, 60.0));
    assert_eq!(shot["camera"]["matches_view"], true, "{shot}");
    assert!(shot.get("warning").is_none());
    *drift.lock().unwrap() = true;
    let off = local(&app, "look", json!({"mode": "3d", "view": "Double height living"})).await;
    assert_eq!(off["camera"]["matches_view"], false);
    assert!(off["camera"]["off_by_mm"].as_f64().unwrap() > 1000.0);
    assert!(off["warning"].as_str().unwrap().contains("saved view"));
    window.abort();
}

/// G4 problem 8: the window reports its render queue; `render` status says
/// what it is doing, and a busy window's refusal says it too.
#[tokio::test]
async fn g4_8_the_render_queue_reports_what_it_is_doing() {
    let (app, _dir) = app();
    local(&app, "project", json!({"action": "create", "name": "G4 8", "template": "sample-bungalow"})).await;
    let idle = local(&app, "render", json!({"action": "status"})).await;
    assert_eq!(idle["busy"], false);
    // The user started a render in the window; it reports as it goes.
    app.handle("window_status", json!({"busy": true, "what": "Rendering", "view": "Front", "index": 1, "total": 2, "progress_pct": 75.0, "eta_s": 30.0})).await.unwrap();
    let busy = local(&app, "render", json!({"action": "status"})).await;
    assert_eq!(busy["busy"], true);
    assert_eq!(busy["window"]["status"], "Rendering Front, 1 of 2, 75%, about 30 s left.");
    let window = fake_window(&app, |_| async {
        Err(IpcError::new("invalid", "A render is already running in Guhit Studio. Wait for it or stop it, then try again."))
    });
    let refused = call(&app, &CallContext::local(), "render", json!({"action": "start", "wait_seconds": 5})).await.unwrap_err();
    assert_eq!(refused.code, "window_busy");
    assert!(refused.message.contains("Rendering Front, 1 of 2, 75%"), "{}", refused.message);
    assert!(refused.message.contains("Try again in about 35 s"), "{}", refused.message);
    window.abort();
    app.handle("window_status", json!({"busy": false})).await.unwrap();
    assert_eq!(local(&app, "render", json!({"action": "status"})).await["busy"], false);
}
