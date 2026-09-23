//! The MCP tool set against a real `AppService` on a throwaway data dir.
//! These are the same calls an MCP client makes, minus the JSON-RPC framing.

use guhit_app::AppService;
use guhit_mcp::tools::{self, Output, ToolFail};
use serde_json::{json, Value};

fn app() -> (AppService, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    (AppService::new_sandboxed(dir.path().to_path_buf()), dir)
}

async fn ok(app: &AppService, name: &str, args: Value) -> Value {
    match tools::call(app, name, args).await {
        Ok(Output::Json(v)) => v,
        Ok(Output::Image { .. }) => panic!("`{name}` returned an image, expected JSON"),
        Err(ToolFail(m)) => panic!("`{name}` failed: {m}"),
    }
}

async fn fail(app: &AppService, name: &str, args: Value) -> String {
    match tools::call(app, name, args).await {
        Err(ToolFail(m)) => m,
        Ok(_) => panic!("`{name}` was expected to fail"),
    }
}

async fn new_project(app: &AppService) {
    ok(app, "create_project", json!({"name": "Test", "template": "blank"})).await;
}

/// Ids of the walls bounding the only room, in creation order.
async fn room_walls(app: &AppService) -> (String, Vec<String>) {
    let rooms = ok(app, "list_rooms", json!({})).await;
    let room = &rooms["rooms"][0];
    let walls = room["bounding_walls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["id"].as_str().unwrap().to_string())
        .collect();
    (room["id"].as_str().unwrap().to_string(), walls)
}

#[tokio::test]
async fn a_four_by_three_room_has_the_exact_net_area() {
    let (app, _dir) = app();
    new_project(&app).await;
    let added = ok(
        &app,
        "add_rect_room",
        json!({"origin": {"x": 0, "y": 0}, "width_mm": 4000, "depth_mm": 3000, "name": "Sala"}),
    )
    .await;

    // Four walls and one room, in one undo step labelled for the user.
    assert_eq!(added["created"].as_array().unwrap().len(), 5);
    assert_eq!(added["undo_label"], "MCP: add rect room");
    assert_eq!(added["revision"], 1);
    // 3850 x 2850 mm on the inner faces of 150 mm walls.
    assert_eq!(added["totals"]["floor_area_m2"], 10.9725);

    let rooms = ok(&app, "list_rooms", json!({})).await;
    let room = &rooms["rooms"][0];
    assert_eq!(room["name"], "Sala");
    assert_eq!(room["clear_width_x_mm"], 3850.0);
    assert_eq!(room["clear_depth_y_mm"], 2850.0);
}

#[tokio::test]
async fn a_door_that_does_not_fit_returns_the_engine_message() {
    let (app, _dir) = app();
    new_project(&app).await;
    ok(
        &app,
        "add_rect_room",
        json!({"origin": {"x": 0, "y": 0}, "width_mm": 4000, "depth_mm": 3000, "name": "Sala"}),
    )
    .await;
    let (_, walls) = room_walls(&app).await;

    let good = ok(&app, "add_door", json!({"wall_id": walls[0], "position": "center"})).await;
    assert_eq!(good["created"][0]["label"], "Door 900 x 2100");
    assert_eq!(good["undo_label"], "MCP: add door");

    let message = fail(
        &app,
        "add_door",
        json!({"wall_id": walls[0], "position": "offset", "offset_mm": 9000}),
    )
    .await;
    assert!(message.starts_with("invalid: The door"), "unexpected message: {message}");
    assert!(message.contains("does not fit"), "unexpected message: {message}");
    // The engine counts leaf commands; the caller made one call.
    assert!(!message.contains("Step 1 of 1"), "internal step prefix leaked: {message}");

    // The failure changed nothing.
    let summary = ok(&app, "get_project_summary", json!({})).await;
    assert_eq!(summary["totals"]["doors"], 1);
}

#[tokio::test]
async fn a_batch_is_all_or_nothing_and_one_undo_step() {
    let (app, _dir) = app();
    new_project(&app).await;
    let house = ok(
        &app,
        "batch",
        json!({
            "label": "3-room house",
            "steps": [
                {"tool": "add_rect_room", "args": {"origin": {"x": 0, "y": 0}, "width_mm": 6000, "depth_mm": 4000, "name": "Sala"}},
                {"tool": "add_rect_room", "args": {"origin": {"x": 6000, "y": 0}, "width_mm": 3600, "depth_mm": 4000, "name": "Kuwarto 1"}},
                {"tool": "add_rect_room", "args": {"origin": {"x": 0, "y": 4000}, "width_mm": 3000, "depth_mm": 3000, "name": "Kusina"}}
            ]
        }),
    )
    .await;

    assert_eq!(house["revision"], 1, "a batch is one revision, so one undo step");
    assert_eq!(house["undo_label"], "MCP: 3-room house");
    assert_eq!(house["totals"]["room_count"], 3);
    // Every step reports what it created, so the next call can name those ids.
    let steps = house["steps"].as_array().expect("per-step ids");
    assert_eq!(steps.len(), 3);
    assert_eq!(steps[1]["step"], 2);
    assert!(!steps[1]["created"].as_array().unwrap().is_empty());

    // One undo takes the whole house away.
    let undone = ok(&app, "undo", json!({})).await;
    assert_eq!(undone["revision"], 2);
    assert_eq!(ok(&app, "get_project_summary", json!({})).await["totals"]["rooms"], 0);
    let redone = ok(&app, "redo", json!({})).await;
    assert_eq!(redone["revision"], 3);
    assert_eq!(ok(&app, "get_project_summary", json!({})).await["totals"]["rooms"], 3);
}

#[tokio::test]
async fn a_failing_step_applies_nothing_and_names_the_step() {
    let (app, _dir) = app();
    new_project(&app).await;
    let message = fail(
        &app,
        "batch",
        json!({
            "steps": [
                {"tool": "add_rect_room", "args": {"origin": {"x": 0, "y": 0}, "width_mm": 4000, "depth_mm": 3000, "name": "Sala"}},
                {"tool": "add_rect_room", "args": {"origin": {"x": 0, "y": 0}, "width_mm": -1, "depth_mm": 3000}}
            ]
        }),
    )
    .await;
    assert!(message.starts_with("step 2 of 2 (`add_rect_room`)"), "unexpected: {message}");
    assert!(message.contains("Nothing was applied"), "unexpected: {message}");
    assert_eq!(ok(&app, "get_project_summary", json!({})).await["totals"]["rooms"], 0);
}

#[tokio::test]
async fn edits_wake_a_watcher_and_move_the_revision() {
    let (app, _dir) = app();
    let mut watch = app.watch_changes();
    assert_eq!(watch.borrow_and_update().revision, 0);

    new_project(&app).await;
    assert!(watch.has_changed().unwrap(), "creating a project is a change");
    let opened = watch.borrow_and_update().clone();
    assert!(opened.project_id.is_some());

    ok(
        &app,
        "add_rect_room",
        json!({"origin": {"x": 0, "y": 0}, "width_mm": 4000, "depth_mm": 3000}),
    )
    .await;
    assert!(watch.has_changed().unwrap());
    assert_eq!(watch.borrow_and_update().revision, 1);

    let revision: Value = app.handle("doc_revision", json!({})).await.unwrap();
    assert_eq!(revision["revision"], 1);
    assert_eq!(revision["project_id"], opened.project_id.unwrap());

    ok(&app, "close_project", json!({})).await;
    assert!(watch.has_changed().unwrap(), "closing is a change too");
    assert_eq!(watch.borrow_and_update().project_id, None);
}

#[tokio::test]
async fn hub_tools_switch_the_open_document() {
    let (app, _dir) = app();
    ok(&app, "create_project", json!({"name": "First", "template": "blank"})).await;
    let second = ok(
        &app,
        "create_project",
        json!({"name": "Second", "template": "sample-bungalow"}),
    )
    .await;
    assert!(second["totals"]["room_count"].as_u64().unwrap() > 0);

    let list = ok(&app, "list_projects", json!({})).await;
    assert_eq!(list.as_array().unwrap().len(), 2);

    let first_id = list
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "First")
        .unwrap()["id"]
        .clone();
    let reopened = ok(&app, "open_project", json!({"id": first_id})).await;
    assert_eq!(reopened["name"], "First");
    assert_eq!(reopened["totals"]["room_count"], 0);
}

#[tokio::test]
async fn tools_say_so_when_no_project_is_open() {
    let (app, _dir) = app();
    let message = fail(
        &app,
        "add_rect_room",
        json!({"origin": {"x": 0, "y": 0}, "width_mm": 4000, "depth_mm": 3000}),
    )
    .await;
    assert!(message.starts_with("no_document"), "unexpected: {message}");
    assert!(message.contains("create_project"), "the message should say what to do: {message}");

    let message = fail(&app, "get_plan_image", json!({})).await;
    assert!(message.starts_with("no_document"), "unexpected: {message}");
}

#[tokio::test]
async fn export_plan_writes_into_the_exports_folder() {
    let (app, dir) = app();
    new_project(&app).await;
    ok(
        &app,
        "add_rect_room",
        json!({"origin": {"x": 0, "y": 0}, "width_mm": 4000, "depth_mm": 3000, "name": "Sala"}),
    )
    .await;
    let result = ok(&app, "export_plan", json!({"format": "svg", "paper": "a3"})).await;
    let path = std::path::PathBuf::from(result["path"].as_str().unwrap());
    assert!(path.starts_with(dir.path()), "export escaped the data dir: {}", path.display());
    assert!(path.is_file());
    assert_eq!(path.extension().unwrap(), "svg");

    let message = fail(&app, "export_plan", json!({"format": "svg", "paper": "a9"})).await;
    assert!(message.contains("unknown paper"), "unexpected: {message}");
}

#[tokio::test]
async fn a_plan_image_is_the_saved_thumbnail() {
    let (app, _dir) = app();
    let state: Value = app
        .handle("hub_create", json!({"name": "Thumb", "template": "blank"}))
        .await
        .unwrap();
    let id = state["project"]["id"].as_str().unwrap().to_string();

    let message = fail(&app, "get_plan_image", json!({})).await;
    assert!(message.starts_with("not_found"), "unexpected: {message}");
    assert!(message.contains("export_plan"), "the message should offer a way out: {message}");

    // Smallest valid PNG, as the desktop window would post it.
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    app.handle("hub_set_thumbnail", json!({"id": id, "png": png}))
        .await
        .unwrap();
    match tools::call(&app, "get_plan_image", json!({})).await {
        Ok(Output::Image { base64, mime }) => {
            assert_eq!(mime, "image/png");
            assert!(base64.starts_with("iVBORw0KGgo"));
        }
        other => panic!("expected an image, got {:?}", other.is_ok()),
    }
}

#[tokio::test]
async fn an_unknown_tool_is_a_tool_error_not_a_crash() {
    let (app, _dir) = app();
    let message = fail(&app, "demolish_everything", json!({})).await;
    assert!(message.contains("unknown tool"), "unexpected: {message}");
}

#[tokio::test]
async fn the_pipe_takeoff_of_the_plumbing_demo() {
    let (app, _dir) = app();
    ok(&app, "create_project", json!({"name": "Pipes", "template": "plumbing-demo"})).await;

    let takeoff = ok(&app, "get_pipe_takeoff", json!({})).await;
    assert_eq!(takeoff["total_length_m"], 44.94);
    assert_eq!(takeoff["elbow_count"], 23);
    assert_eq!(takeoff["tee_count"], 9);
    assert_eq!(takeoff["sleeve_count"], 7);
    assert_eq!(takeoff["rows"].as_array().unwrap().len(), 6);
    assert!(fail(&app, "get_pipe_takeoff", json!({"size": 20})).await.contains("size"));

    let pipes = ok(&app, "list_elements", json!({"kind": "pipe"})).await;
    assert_eq!(pipes["count"], 16);
    let items = ok(&app, "list_review_items", json!({})).await;
    let codes: Vec<&str> = items["items"].as_array().unwrap().iter().filter_map(|i| i["code"].as_str()).collect();
    assert!(codes.contains(&"pipe_across_opening") && codes.contains(&"pipe_penetrations"), "{codes:?}");

    // Moving a pipe is an ordinary edit, one undo step, and the take-off follows.
    let moved = ok(
        &app,
        "move_elements",
        json!({"ids": ["00000000-0000-4000-8000-000000016015"], "dx_mm": 500, "dy_mm": 0, "stretch_connected": false}),
    )
    .await;
    assert_eq!(moved["changed"][0]["kind"], "pipe");
    assert_eq!(ok(&app, "get_pipe_takeoff", json!({})).await["total_length_m"], 44.94);
    ok(&app, "undo", json!({})).await;
}
