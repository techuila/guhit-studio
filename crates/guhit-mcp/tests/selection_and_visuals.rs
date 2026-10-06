//! The MCP tools that work with the window: its selection and the "Only the
//! selection" limit (DECISIONS D30), and renders, captures and the plan
//! picture that the window makes (DECISIONS D31). A stand-in answers window
//! requests the way the desktop window does.

use std::future::Future;
use std::io::Cursor;
use std::time::Duration;

use base64::Engine as _;
use guhit_app::AppService;
use guhit_mcp::tools::{self, Output, ToolFail};
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
        Err(ToolFail(m)) => panic!("`{name}` failed: {m}"),
    }
}

async fn rich(app: &AppService, name: &str, args: Value) -> (Value, Vec<tools::Picture>) {
    match tools::call(app, name, args).await {
        Ok(Output::Rich { json, images }) => (json, images),
        Ok(Output::Json(v)) => panic!("`{name}` returned no picture: {v}"),
        Err(ToolFail(m)) => panic!("`{name}` failed: {m}"),
    }
}

async fn fail(app: &AppService, name: &str, args: Value) -> String {
    match tools::call(app, name, args).await {
        Err(ToolFail(m)) => m,
        Ok(_) => panic!("`{name}` was expected to fail"),
    }
}

/// What the window would send after the user selected `ids`.
async fn select(app: &AppService, ids: &[&str], only_the_selection: bool) {
    let presence = json!({"cursor": null, "level_id": null, "selection": ids, "typing": null, "ai_scope": only_the_selection});
    app.handle("presence_set", json!({ "presence": presence })).await.unwrap();
}

/// Two rooms that share no wall: Sala at the origin, Bedroom 2 m to the east.
async fn two_rooms(app: &AppService) -> (String, String) {
    ok(app, "create_project", json!({"name": "Scope", "template": "blank"})).await;
    let sala = ok(app, "add_rect_room", json!({"origin": {"x": 0, "y": 0}, "width_mm": 4000, "depth_mm": 3000, "name": "Sala"})).await;
    let bed = ok(app, "add_rect_room", json!({"origin": {"x": 6000, "y": 0}, "width_mm": 3000, "depth_mm": 3000, "name": "Bedroom"})).await;
    let room_of = |v: &Value| {
        v["created"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["kind"] == "room")
            .and_then(|e| e["id"].as_str())
            .unwrap()
            .to_string()
    };
    (room_of(&sala), room_of(&bed))
}

/// The wall of the open project whose centerline runs along y = `y` from x0 to x1.
async fn wall_along(app: &AppService, y: f64, x0: f64, x1: f64) -> String {
    let walls = ok(app, "list_elements", json!({"kind": "wall"})).await;
    let list = walls["elements"].as_array().expect("a list of walls");
    list.iter()
        .find(|w| {
            let (s, e) = (&w["start"], &w["end"]);
            let xs = [s["x_mm"].as_f64().unwrap(), e["x_mm"].as_f64().unwrap()];
            s["y_mm"].as_f64() == Some(y)
                && e["y_mm"].as_f64() == Some(y)
                && xs.iter().cloned().fold(f64::INFINITY, f64::min) == x0
                && xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max) == x1
        })
        .and_then(|w| w["id"].as_str())
        .expect("the wall")
        .to_string()
}

// ------------------------------------------------------------ the selection

#[tokio::test]
async fn get_selection_reads_what_the_window_selected() {
    let (app, _dir) = app();
    let (sala, _) = two_rooms(&app).await;

    let none = ok(&app, "get_selection", json!({})).await;
    assert_eq!(none["count"], 0);
    assert!(none["note"].as_str().unwrap().contains("Nothing is selected"));

    select(&app, &[&sala, "gone-id"], false).await;
    let one = ok(&app, "get_selection", json!({})).await;
    assert_eq!(one["count"], 1, "ids no longer in the plan are left out");
    assert_eq!(one["selected"][0]["label"], "Room Sala");
    assert_eq!(one["limited_to_selection"], false);
    assert!(one["details"].is_array() || one["details"].is_object());

    select(&app, &[&sala], true).await;
    assert_eq!(ok(&app, "get_selection", json!({})).await["limited_to_selection"], true);
}

#[tokio::test]
async fn a_scoped_edit_changes_only_the_selection() {
    let (app, _dir) = app();
    let (sala, bed) = two_rooms(&app).await;
    select(&app, &[&sala], false).await;

    // Inside: the room's own walls move, and the ones joined to them stretch.
    let wider = ok(&app, "resize_room", json!({"room_id": sala, "side": "east", "delta_mm": 300, "scope": "selection"})).await;
    assert_eq!(wider["scope"]["set_by"], "this call");
    assert!(wider["scope"]["ids"].as_array().unwrap().iter().any(|i| i == &json!(sala)));

    // Outside: another room, and a change to the whole project.
    let other = fail(&app, "rename_room", json!({"room_id": bed, "name": "Guest", "scope": "selection"})).await;
    assert!(other.starts_with("out_of_scope:"), "unexpected: {other}");
    assert!(other.contains("Sala"), "the message says what the scope holds: {other}");
    let roof = fail(&app, "set_roof", json!({"pitch_deg": 30, "scope": "selection"})).await;
    assert!(roof.starts_with("out_of_scope:"), "unexpected: {roof}");

    // Explicit ids work the same; without a scope nothing is limited.
    ok(&app, "rename_room", json!({"room_id": bed, "name": "Guest", "scope": [bed]})).await;
    ok(&app, "set_roof", json!({"pitch_deg": 30})).await;

    let empty = fail(&app, "rename_room", json!({"room_id": bed, "name": "X", "scope": []})).await;
    assert!(empty.contains("empty list"), "unexpected: {empty}");
    select(&app, &[], false).await;
    let nothing = fail(&app, "rename_room", json!({"room_id": bed, "name": "X", "scope": "selection"})).await;
    assert!(nothing.contains("nothing is selected"), "unexpected: {nothing}");
}

#[tokio::test]
async fn only_the_selection_binds_every_edit() {
    let (app, _dir) = app();
    let (sala, bed) = two_rooms(&app).await;
    select(&app, &[&sala], true).await;

    // The call asks for nothing, the user's switch still holds.
    let refused = fail(&app, "rename_room", json!({"room_id": bed, "name": "Guest"})).await;
    assert!(refused.starts_with("out_of_scope:"), "unexpected: {refused}");
    assert!(refused.contains("Only the selection"), "the message says who set the limit: {refused}");
    let renamed = ok(&app, "rename_room", json!({"room_id": sala, "name": "Living"})).await;
    assert_eq!(renamed["scope"]["set_by"], "the user (Only the selection)");
    let review = fail(&app, "set_review_mark", json!({"action": "set_aside", "code": "room_no_window", "note": "later"})).await;
    assert!(review.starts_with("out_of_scope:"), "a whole check is project wide: {review}");

    // Switched on with nothing selected: no limit.
    select(&app, &[], true).await;
    ok(&app, "rename_room", json!({"room_id": bed, "name": "Guest"})).await;
}

#[tokio::test]
async fn a_scoped_batch_can_build_on_what_it_creates() {
    let (app, _dir) = app();
    ok(&app, "create_project", json!({"name": "Grow", "template": "blank"})).await;
    ok(&app, "add_rect_room", json!({"origin": {"x": 0, "y": 0}, "width_mm": 4000, "depth_mm": 3000, "name": "Sala"})).await;
    let south = wall_along(&app, 0.0, 0.0, 4000.0).await;
    select(&app, &[&south], false).await;

    // A garden wall 400 mm in front of the selected wall is in its area.
    let garden = json!({"tool": "add_wall", "args": {"start": {"x": 500, "y": -400}, "end": {"x": 3500, "y": -400}}});
    let first = ok(&app, "batch", json!({"steps": [garden.clone()], "scope": "selection"})).await;
    let new_wall = first["created"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "wall")
        .and_then(|e| e["id"].as_str())
        .unwrap()
        .to_string();
    ok(&app, "undo", json!({})).await;

    // The same wall again gets the same id, and a later step may put a window
    // on it: what a step creates joins the scope.
    let both = ok(
        &app,
        "batch",
        json!({"steps": [garden, {"tool": "add_window", "args": {"wall_id": new_wall, "position": "center"}}], "scope": "selection"}),
    )
    .await;
    assert_eq!(both["steps"].as_array().unwrap().len(), 2);

    // Far away is outside the area, and names its step.
    let far = fail(
        &app,
        "batch",
        json!({"steps": [{"tool": "add_wall", "args": {"start": {"x": 0, "y": 9000}, "end": {"x": 3000, "y": 9000}}}], "scope": "selection"}),
    )
    .await;
    assert!(far.contains("out_of_scope"), "unexpected: {far}");

    let inner = fail(&app, "batch", json!({"steps": [{"tool": "add_wall", "args": {"start": {"x": 0, "y": 0}, "end": {"x": 1, "y": 0}, "scope": "selection"}}]})).await;
    assert!(inner.contains("Give scope once"), "unexpected: {inner}");
}

// ------------------------------------------------------------------ cameras

#[tokio::test]
async fn a_camera_is_saved_above_its_level_floor() {
    let (app, _dir) = app();
    ok(&app, "create_project", json!({"name": "Views", "template": "blank"})).await;
    ok(&app, "add_level", json!({"name": "Second Floor"})).await;
    let added = ok(
        &app,
        "add_camera",
        json!({"name": "Upstairs hall", "position": {"x": 0, "y": 0, "z": 1600}, "target": {"x": 3000, "y": 0, "z": 1500}, "level": "second floor"}),
    )
    .await;
    assert_eq!(added["undo_label"], "MCP: add camera");
    let cameras = ok(&app, "list_elements", json!({"kind": "camera"})).await;
    let list = cameras["elements"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["name"], "Upstairs hall");
    assert_eq!(list[0]["position_mm"]["z"], 4600.0, "1600 above a floor at 3000");
    assert_eq!(list[0]["fov_deg"], 60.0);

    let wide = fail(&app, "add_camera", json!({"name": "x", "position": {"x": 0, "y": 0, "z": 1600}, "target": {"x": 1, "y": 0, "z": 1600}, "fov_deg": 170})).await;
    assert!(wide.contains("fov_deg"), "unexpected: {wide}");
    let close = fail(&app, "add_camera", json!({"name": "x", "position": {"x": 0, "y": 0, "z": 1600}, "target": {"x": 10, "y": 0, "z": 1600}})).await;
    assert!(close.contains("100 mm"), "unexpected: {close}");
}

// ---------------------------------------------------------- window requests

/// Smallest valid PNG.
const PNG_1PX: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

fn camera() -> Value {
    json!({"id": "", "name": "View", "preset": "custom", "position": {"x": 0, "y": -8000, "z": 1600}, "target": {"x": 0, "y": 0, "z": 1200}, "fov_deg": 60, "light": null})
}

/// Stands in for the desktop window: every window request is answered with
/// what `answer` returns. Subscribes before it returns, so a request made
/// right after reaches it.
fn fake_window<F, Fut>(app: &AppService, answer: F) -> tokio::task::JoinHandle<()>
where
    F: Fn(AppService, WindowRequest) -> Fut + Send + 'static,
    Fut: Future<Output = Result<WindowReply, IpcError>> + Send,
{
    let mut events = app.events();
    let app = app.clone();
    tokio::spawn(async move {
        while let Ok(event) = events.recv().await {
            let AppEvent::WindowRequest { request } = event else { continue };
            let (reply, error) = match answer(app.clone(), request.clone()).await {
                Ok(r) => (json!(r), Value::Null),
                Err(e) => (Value::Null, json!(e)),
            };
            let _ = app.handle("window_reply", json!({"id": request.id, "reply": reply, "error": error})).await;
        }
    })
}

/// What the window does for a render or capture: saves a record, answers
/// with its id and a preview.
async fn save_render(app: AppService, request: WindowRequest) -> Result<WindowReply, IpcError> {
    let record: RenderRecord =
        serde_json::from_value(app.handle("render_capture", json!({"camera": camera(), "png": PNG_1PX})).await?).unwrap();
    let note = match request.task {
        WindowTask::Render { .. } => "Saved to Visuals. View: path traced, 64 samples in 3 s.",
        _ => "Captured the 3D view and saved it to Visuals.",
    };
    Ok(WindowReply { render_ids: vec![record.id], image: Some(PNG_1PX.into()), note: note.into() })
}

#[tokio::test]
async fn render_tools_need_a_window() {
    let (app, _dir) = app();
    ok(&app, "create_project", json!({"name": "No window", "template": "blank"})).await;
    for tool in ["render_view", "capture_view"] {
        let message = fail(&app, tool, json!({})).await;
        assert!(message.starts_with("no_window"), "{tool}: {message}");
    }
}

#[tokio::test]
async fn a_render_comes_back_with_its_record_and_a_preview() {
    let (app, _dir) = app();
    ok(&app, "create_project", json!({"name": "Render", "template": "blank"})).await;
    let _window = fake_window(&app, save_render);

    let (json, images) = rich(&app, "render_view", json!({"quality": "final", "size": "4k"})).await;
    assert_eq!(json["status"], "done");
    assert_eq!(json["renders"].as_array().unwrap().len(), 1);
    assert!(json["renders"][0]["id"].is_string());
    assert!(json["note"].as_str().unwrap().contains("path traced"));
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].mime, "image/png");

    let (capture, _) = rich(&app, "capture_view", json!({})).await;
    assert_eq!(capture["status"], "done");
    let unknown = fail(&app, "capture_view", json!({"view": "Kitchen"})).await;
    assert!(unknown.contains("no saved view"), "unexpected: {unknown}");
}

#[tokio::test]
async fn a_long_render_hands_back_a_job_to_come_back_for() {
    let (app, _dir) = app();
    ok(&app, "create_project", json!({"name": "Slow", "template": "blank"})).await;
    let _window = fake_window(&app, |app, request| async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        save_render(app, request).await
    });
    let started = ok(&app, "render_view", json!({"wait_seconds": 1})).await;
    assert_eq!(started["status"], "running");
    let job = started["job_id"].as_str().unwrap().to_string();
    let (done, images) = rich(&app, "get_render_job", json!({"job_id": job, "wait_seconds": 10})).await;
    assert_eq!(done["status"], "done");
    assert_eq!(done["job_id"], json!(job));
    assert_eq!(images.len(), 1);
    let gone = fail(&app, "get_render_job", json!({"job_id": "nope"})).await;
    assert!(gone.starts_with("not_found"), "unexpected: {gone}");
}

#[tokio::test]
async fn the_window_says_why_it_cannot() {
    let (app, _dir) = app();
    ok(&app, "create_project", json!({"name": "Hub", "template": "blank"})).await;
    let _window = fake_window(&app, |_, _| async {
        Err(IpcError::new("no_document", "Guhit Studio shows the project list. Open the project first."))
    });
    let message = fail(&app, "render_view", json!({})).await;
    assert!(message.contains("project list"), "unexpected: {message}");
}

#[tokio::test]
async fn a_big_saved_render_is_fitted_for_the_model() {
    let (app, _dir) = app();
    ok(&app, "create_project", json!({"name": "Big", "template": "blank"})).await;
    let img = image::RgbImage::from_pixel(3000, 2000, image::Rgb([180, 200, 230]));
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img).write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    let png = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes.get_ref()));
    let record: RenderRecord =
        serde_json::from_value(app.handle("render_capture", json!({"camera": camera(), "png": png})).await.unwrap()).unwrap();

    let (json, images) = rich(&app, "get_render_image", json!({"id": record.id})).await;
    assert_eq!(json["record"]["id"], json!(record.id));
    assert_eq!(images[0].mime, "image/jpeg");
    let back = image::load_from_memory(&base64::engine::general_purpose::STANDARD.decode(&images[0].base64).unwrap()).unwrap();
    assert_eq!((back.width(), back.height()), (1568, 1045));
}

#[tokio::test]
async fn the_plan_picture_is_drawn_fresh_by_an_open_window() {
    let (app, _dir) = app();
    ok(&app, "create_project", json!({"name": "Plan", "template": "blank"})).await;
    ok(&app, "add_level", json!({"name": "Roof Deck"})).await;
    let _window = fake_window(&app, |_, request| async move {
        let WindowTask::CapturePlan { level_id } = request.task else {
            return Err(IpcError::new("invalid", "not a plan request"));
        };
        Ok(WindowReply { render_ids: vec![], image: Some(PNG_1PX.into()), note: format!("level {}", level_id.is_some()) })
    });
    let (json, images) = rich(&app, "get_plan_image", json!({"level": "roof deck"})).await;
    assert_eq!(json["source"], "window");
    assert_eq!(json["note"], "level true");
    assert_eq!(images.len(), 1);
    let unknown = fail(&app, "get_plan_image", json!({"level": "Basement"})).await;
    assert!(unknown.contains("no level is called"), "unexpected: {unknown}");
}

// ------------------------------------------------------------ live session

#[tokio::test]
async fn session_tools_without_a_session() {
    let (app, _dir) = app();
    ok(&app, "create_project", json!({"name": "Alone", "template": "blank"})).await;
    let session = ok(&app, "get_session", json!({})).await;
    assert_eq!(session["mode"], "off");
    let chat = fail(&app, "send_chat_message", json!({"text": "hello"})).await;
    assert!(chat.starts_with("not_live"), "unexpected: {chat}");
    // Alone, every step is yours: force changes nothing.
    ok(&app, "add_rect_room", json!({"origin": {"x": 0, "y": 0}, "width_mm": 3000, "depth_mm": 3000})).await;
    let undone = ok(&app, "undo", json!({"force": true})).await;
    assert_eq!(undone["what"], "undone");
}

/// A computer with its profile name set, as hosting and joining need. The
/// relay is off (`live_relay: ""`), so a build made with `GUHIT_RELAY_URL`
/// does not reach out to it.
async fn named(name: &str) -> (AppService, tempfile::TempDir) {
    let (app, dir) = app();
    std::fs::write(dir.path().join("settings.json"), json!({ "live_relay": "" }).to_string()).unwrap();
    app.handle("profile_set", json!({ "name": name })).await.unwrap();
    (app, dir)
}

async fn live(app: &AppService) -> LiveStatus {
    serde_json::from_value(app.handle("live_status", json!({})).await.unwrap()).unwrap()
}

#[tokio::test]
async fn a_hosted_session_ends_only_when_the_user_says_so() {
    let (ana, _dir) = named("Ana").await;
    let other = ok(&ana, "create_project", json!({"name": "Iba", "template": "blank"})).await;
    let shared = ok(&ana, "create_project", json!({"name": "Bahay", "template": "blank"})).await;
    ana.handle("live_host", json!({})).await.unwrap();

    let other_id = other["project_id"].as_str().unwrap();
    for (name, args) in [
        ("open_project", json!({"id": other_id})),
        ("create_project", json!({"name": "Bago"})),
        ("close_project", json!({})),
    ] {
        let refused = fail(&ana, name, args).await;
        assert!(refused.starts_with("live_session"), "{name}: {refused}");
        assert!(refused.contains("ends it for everyone") && refused.contains("force"), "{name}: {refused}");
    }
    let status = live(&ana).await;
    assert_eq!(status.mode, LiveMode::Hosting, "nothing ended");
    assert_eq!(status.project_id.as_deref(), shared["project_id"].as_str());

    // The shared project is already open: opening it changes nothing.
    ok(&ana, "open_project", json!({"id": shared["project_id"]})).await;
    assert_eq!(live(&ana).await.mode, LiveMode::Hosting);

    // The user agreed.
    ok(&ana, "open_project", json!({"id": other_id, "force": true})).await;
    assert_eq!(live(&ana).await.mode, LiveMode::Off);
    let open = ok(&ana, "get_project_summary", json!({})).await;
    assert_eq!(open["project_name"], "Iba", "unexpected: {open}");
}

#[tokio::test]
async fn a_guest_leaves_only_when_the_user_says_so() {
    let (ana, _a) = named("Ana").await;
    ok(&ana, "create_project", json!({"name": "Bahay", "template": "blank"})).await;
    let hosted: LiveStatus = serde_json::from_value(ana.handle("live_host", json!({})).await.unwrap()).unwrap();
    let (ben, _b) = named("Ben").await;
    ben.handle("live_join", json!({"invite": hosted.invite.unwrap()})).await.unwrap();
    assert_eq!(live(&ben).await.mode, LiveMode::Joined);

    // The host's refusal names who would be dropped.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while live(&ana).await.participants.len() < 2 {
        assert!(std::time::Instant::now() < deadline, "Ben never showed up at the host");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let host_refused = fail(&ana, "close_project", json!({})).await;
    assert!(host_refused.contains("hosting a live session with Ben"), "unexpected: {host_refused}");

    let refused = fail(&ben, "close_project", json!({})).await;
    assert!(refused.starts_with("live_session") && refused.contains("Ana hosts") && refused.contains("leaves it"), "unexpected: {refused}");
    assert_eq!(live(&ben).await.mode, LiveMode::Joined, "still in");
    // A guest cannot open another project at all; the app says why.
    let open = fail(&ben, "create_project", json!({"name": "Akin"})).await;
    assert!(open.starts_with("host_only"), "unexpected: {open}");

    ok(&ben, "close_project", json!({"force": true})).await;
    assert_eq!(live(&ben).await.mode, LiveMode::Off);
    assert_eq!(live(&ana).await.mode, LiveMode::Hosting, "the host goes on");
}
