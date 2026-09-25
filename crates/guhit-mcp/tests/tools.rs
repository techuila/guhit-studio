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
        Ok(Output::Rich { json, .. }) => json,
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

/// Pages in a PDF: page objects, not the page tree.
fn pdf_pages(bytes: &[u8]) -> usize {
    let text = String::from_utf8_lossy(bytes);
    text.matches("/Type /Page").count() - text.matches("/Type /Pages").count()
}

#[tokio::test]
async fn export_plan_draws_every_sheet_and_a_review_page() {
    let (app, dir) = app();
    ok(&app, "create_project", json!({"name": "Services", "template": "plumbing-demo"})).await;
    let read = |result: &Value| {
        let path = std::path::PathBuf::from(result["path"].as_str().unwrap());
        assert!(path.starts_with(dir.path()), "export escaped the data dir: {}", path.display());
        std::fs::read(&path).unwrap()
    };

    // The plan is the default; every sheet draws something else.
    let plan = read(&ok(&app, "export_plan", json!({"format": "svg"})).await);
    let explicit = read(&ok(&app, "export_plan", json!({"format": "svg", "sheet": "plan"})).await);
    assert_eq!(plan, explicit, "sheet \"plan\" is the default");
    for sheet in ["lighting", "power", "plumbing", "plumbing_isometric", "aircon"] {
        for format in ["svg", "pdf", "dxf"] {
            let bytes = read(&ok(&app, "export_plan", json!({"format": format, "sheet": sheet})).await);
            assert!(!bytes.is_empty(), "{sheet} {format} is empty");
            if format == "svg" {
                assert_ne!(bytes, plan, "the {sheet} sheet is the plan");
            }
        }
    }

    // A review page is one more PDF page.
    let without = read(&ok(&app, "export_plan", json!({"format": "pdf", "sheet": "lighting"})).await);
    let with = read(&ok(&app, "export_plan", json!({"format": "pdf", "sheet": "lighting", "review_page": true})).await);
    assert!(with.starts_with(b"%PDF"));
    assert_eq!(pdf_pages(&without), 1);
    assert!(pdf_pages(&with) > pdf_pages(&without), "no review page was added");
    let off = read(&ok(&app, "export_plan", json!({"format": "pdf", "sheet": "lighting", "review_page": false})).await);
    assert_eq!(pdf_pages(&off), 1);

    // Checked before anything is drawn.
    let unknown = fail(&app, "export_plan", json!({"format": "pdf", "sheet": "elevation"})).await;
    assert!(
        unknown.contains("unknown sheet `elevation`") && unknown.contains("plumbing_isometric"),
        "unexpected: {unknown}"
    );
    let not_a_name = fail(&app, "export_plan", json!({"format": "pdf", "sheet": 3})).await;
    assert!(not_a_name.contains("`sheet` must be a string"), "unexpected: {not_a_name}");
    let not_a_bool = fail(&app, "export_plan", json!({"format": "pdf", "review_page": "yes"})).await;
    assert!(not_a_bool.contains("`review_page` must be true or false"), "unexpected: {not_a_bool}");
    for format in ["svg", "dxf"] {
        let message = fail(&app, "export_plan", json!({"format": format, "review_page": true})).await;
        assert!(message.contains("review_page adds a page to a PDF"), "unexpected: {message}");
    }
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
        Ok(Output::Rich { json, images }) => {
            assert_eq!(json["source"], "thumbnail", "no window is open, so the saved thumbnail");
            assert_eq!(images.len(), 1);
            assert_eq!(images[0].mime, "image/png");
            assert!(images[0].base64.starts_with("iVBORw0KGgo"));
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
    assert_eq!(takeoff["total_length_m"], 59.654);
    assert_eq!(takeoff["elbow_count"], 29);
    assert_eq!(takeoff["tee_count"], 9);
    assert_eq!(takeoff["sleeve_count"], 9);
    // Every system: six plumbing rows, storm, the line set, the condensate.
    assert_eq!(takeoff["rows"].as_array().unwrap().len(), 9);
    assert_eq!(takeoff["rows"][7]["system"], "refrigerant");
    assert!(fail(&app, "get_pipe_takeoff", json!({"size": 20})).await.contains("size"));

    let pipes = ok(&app, "list_elements", json!({"kind": "pipe"})).await;
    assert_eq!(pipes["count"], 20);
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
    assert_eq!(ok(&app, "get_pipe_takeoff", json!({})).await["total_length_m"], 59.654);
    ok(&app, "undo", json!({})).await;
}

#[tokio::test]
async fn the_schedule_of_the_services_demo() {
    let (app, _dir) = app();
    ok(&app, "create_project", json!({"name": "Services", "template": "plumbing-demo"})).await;
    let schedule = ok(&app, "get_schedule", json!({})).await;
    assert_eq!(schedule["levels"][0]["total"], 27);
    // The kitchen sink is a plumbing fixture, listed first in its room.
    assert_eq!(schedule["rows"][0]["room"], "Living / Kitchen");
    assert_eq!(schedule["rows"][0]["group"], "plumbing");
    assert_eq!(schedule["rows"][0]["item"], "Kitchen sink counter");
    assert_eq!(schedule["rows"][1]["item"], "Ceiling light");
    assert_eq!(schedule["rows"][1]["form_row"], "Lighting outlets");
    let washer = schedule["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["catalog_key"] == "washing-machine")
        .expect("the washer is counted");
    assert_eq!((washer["room"].as_str(), washer["group"].as_str()), (Some("Outside"), Some("plumbing")));
    assert!(fail(&app, "get_schedule", json!({"room": "x"})).await.contains("room"));

    // A wall device placed through MCP lands on the wall, named by its room.
    let placed = ok(
        &app,
        "add_asset",
        json!({"catalog_key": "switch-1", "position": {"x": 7050, "y": 3900}}),
    )
    .await;
    assert_eq!(placed["created"][0]["label"], "Switch, one gang in Bedroom");
    let id = placed["created"][0]["id"].as_str().unwrap().to_string();
    let described = ok(&app, "describe_elements", json!({"ids": [id]})).await;
    assert_eq!(described["elements"][0]["position"], json!({"x_mm": 7050.0, "y_mm": 4130.0}));
    assert_eq!(described["elements"][0]["device"], "switch");
}

#[tokio::test]
async fn a_review_item_is_set_aside_with_a_note_and_reopened() {
    let (app, _dir) = app();
    ok(&app, "create_project", json!({"name": "Services", "template": "plumbing-demo"})).await;
    let items = ok(&app, "list_review_items", json!({})).await;
    let light = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["code"] == "light_no_switch")
        .unwrap()
        .clone();
    assert_eq!(light["status"], "open");
    let id = light["id"].as_str().unwrap().to_string();

    // A note is needed, and the target must be one thing.
    assert!(fail(&app, "set_review_mark", json!({"action": "set_aside", "issue_id": id})).await.contains("note"));
    assert!(fail(&app, "set_review_mark", json!({"action": "set_aside", "note": "x"})).await.contains("issue_id"));
    assert!(fail(
        &app,
        "set_review_mark",
        json!({"action": "set_aside", "issue_id": id, "code": "light_no_switch", "note": "x"})
    )
    .await
    .contains("not both"));
    let unknown = fail(&app, "set_review_mark", json!({"action": "set_aside", "code": "made_up", "note": "x"})).await;
    assert!(unknown.contains("no review check"), "{unknown}");

    let set = ok(
        &app,
        "set_review_mark",
        json!({"action": "set_aside", "issue_id": id, "note": "  The T&B switch is outside the door  "}),
    )
    .await;
    assert_eq!(set["undo_label"], "MCP: set a review item aside");
    assert_eq!(set["items"][0]["status"], "ignored");
    assert_eq!(set["items"][0]["note"], "The T&B switch is outside the door");
    let items = ok(&app, "list_review_items", json!({})).await;
    assert_eq!(items["ignored_count"], 1);

    // A whole check, then reopen the finding: one undo step each.
    ok(&app, "set_review_mark", json!({"action": "set_aside", "code": "lineset_extra", "note": "Quoted"})).await;
    let reopened = ok(&app, "set_review_mark", json!({"action": "reopen", "issue_id": id})).await;
    assert_eq!(reopened["items"][0]["status"], "open");
    assert!(fail(&app, "set_review_mark", json!({"action": "reopen", "issue_id": id})).await.contains("nothing to reopen"));
    ok(&app, "undo", json!({})).await;
    let items = ok(&app, "list_review_items", json!({})).await;
    assert_eq!(items["ignored_count"], 2);
}

#[tokio::test]
async fn a_second_storey_is_one_batch_and_one_undo_step() {
    let (app, _dir) = app();
    ok(&app, "create_project", json!({"name": "Two storeys", "template": "blank"})).await;
    let result = ok(
        &app,
        "batch",
        json!({
            "label": "Two storeys",
            "steps": [
                {"tool": "add_rect_room", "args": {"origin": {"x": 0, "y": 0}, "width_mm": 4000, "depth_mm": 3000, "name": "Sala"}},
                {"tool": "add_level", "args": {"name": "Second Floor"}},
                {"tool": "add_rect_room", "args": {"origin": {"x": 0, "y": 0}, "width_mm": 4000, "depth_mm": 3000, "name": "Bedroom", "level": "Second Floor"}},
            ],
        }),
    )
    .await;
    let steps = result["steps"].as_array().unwrap();
    let level = &steps[1]["levels_added"][0];
    assert_eq!(level["name"], "Second Floor");
    assert_eq!(level["elevation_mm"], 3000.0);
    assert_eq!(result["levels_added"][0]["id"], level["id"]);
    let summary = ok(&app, "get_project_summary", json!({})).await;
    let levels = summary["levels"].as_array().unwrap();
    assert_eq!(levels.len(), 2);
    assert_eq!((levels[1]["name"].as_str(), levels[1]["room_count"].as_u64()), (Some("Second Floor"), Some(1)));
    assert_eq!(levels[0]["room_count"], 1);

    // Refusals come from the engine or the level lookup, word for word.
    let taken = fail(&app, "add_level", json!({"elevation_mm": 3000})).await;
    assert!(taken.contains("Second Floor already has its floor at 3000 mm"), "unexpected: {taken}");
    let low = fail(&app, "add_level", json!({"height_mm": 1500})).await;
    assert!(low.contains("2000") && low.contains("10000"), "unexpected: {low}");
    let unknown = fail(&app, "add_wall", json!({"start": {"x": 0, "y": 0}, "end": {"x": 1000, "y": 0}, "level": "Attic"})).await;
    assert!(unknown.contains("no level is called `Attic`"), "unexpected: {unknown}");

    // Deleting the upper level takes its room and walls, in one undo step.
    let deleted = ok(&app, "delete_level", json!({"level": "second floor"})).await;
    assert_eq!(deleted["undo_label"], "MCP: delete level");
    assert_eq!(deleted["levels_removed"][0]["name"], "Second Floor");
    assert_eq!(deleted["deleted"].as_array().unwrap().len(), 5, "4 walls and the room");
    let last = fail(&app, "delete_level", json!({"level": "Ground Floor"})).await;
    assert!(last.contains("last level"), "unexpected: {last}");
    ok(&app, "undo", json!({})).await;
    let summary = ok(&app, "get_project_summary", json!({})).await;
    assert_eq!(summary["levels"].as_array().unwrap().len(), 2);
}
