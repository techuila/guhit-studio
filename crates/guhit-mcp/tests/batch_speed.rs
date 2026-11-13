//! `batch` speed and equivalence.
//!
//! A batch used to re-preview every staged step for each new step, so its
//! time grew with the square of its length while it held the session lock.
//! Staging now applies each step once to one working copy. These tests hold
//! it to linear time and to the exact results of the old path: same ids,
//! same per-step lists, same final project, same refusal messages.

use std::time::Instant;

use guhit_app::ai::tools as copilot;
use guhit_app::AppService;
use guhit_core::Document;
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
        Ok(Output::Text(_)) => panic!("`{name}` returned text, expected JSON"),
        Err(ToolFail(m)) => panic!("`{name}` failed: {m}"),
    }
}

async fn fail(app: &AppService, name: &str, args: Value) -> String {
    match tools::call(app, name, args).await {
        Err(ToolFail(m)) => m,
        Ok(_) => panic!("`{name}` was expected to fail"),
    }
}

async fn open_project(app: &AppService) -> Project {
    ok(app, "create_project", json!({"name": "Speed", "template": "blank"})).await;
    let s = app.session.lock().await;
    s.doc.as_ref().unwrap().project().clone()
}

fn step(tool: &str, args: Value) -> Value {
    json!({"tool": tool, "args": args})
}

/// The walls and rooms of a test plan: a grid of rooms that share their
/// walls, and a row of boxes drawn wall by wall to the east.
fn shell_steps(cols: usize, rows: usize, boxes: usize) -> Vec<Value> {
    let mut steps = vec![];
    for r in 0..rows {
        for c in 0..cols {
            steps.push(step(
                "add_rect_room",
                json!({
                    "origin": {"x": c as f64 * 4000.0, "y": r as f64 * 3500.0},
                    "width_mm": 4000, "depth_mm": 3500,
                    "name": format!("Room {r}-{c}"),
                }),
            ));
        }
    }
    let x0 = cols as f64 * 4000.0 + 3000.0;
    for b in 0..boxes {
        let x = x0 + b as f64 * 3000.0;
        let corners = [(x, 0.0), (x + 2500.0, 0.0), (x + 2500.0, 2500.0), (x, 2500.0)];
        for i in 0..4 {
            let (a, z) = (corners[i], corners[(i + 1) % 4]);
            steps.push(step(
                "add_wall",
                json!({"start": {"x": a.0, "y": a.1}, "end": {"x": z.0, "y": z.1}}),
            ));
        }
    }
    steps
}

/// Apply steps one by one to a copy of the project.
fn applied(project: &Project, steps: &[Value]) -> Document {
    let mut doc = Document::new(project.clone());
    let level = project.levels[0].id.clone();
    for s in steps {
        let commands = copilot::to_commands(doc.project(), s["tool"].as_str().unwrap(), &s["args"], &level).unwrap();
        doc.apply(Command::Batch { label: "shell".into(), commands }, Origin::Ai).unwrap();
    }
    doc
}

/// Learn the wall ids the shell steps make (ids are a pure function of the
/// project and the command, so a copy gets the same ones), then hang
/// `openings` doors and windows on them and place `assets` objects in the
/// rooms.
fn fill_steps(project: &Project, shell: &[Value], openings: usize, assets: usize) -> Vec<Value> {
    let doc = applied(project, shell);
    let rooms: Vec<RoomGeometry> = doc.derived().rooms.clone();
    let mut steps = vec![];
    let mut used: Vec<String> = vec![];
    'outer: for pass in 0..4 {
        for g in &rooms {
            if steps.len() >= openings {
                break 'outer;
            }
            let Some(wall) = g.wall_ids.get(pass) else { continue };
            if used.contains(wall) {
                continue;
            }
            used.push(wall.clone());
            let tool = if pass % 2 == 0 { "add_door" } else { "add_window" };
            steps.push(step(tool, json!({"wall_id": wall, "position": "center"})));
        }
    }
    assert_eq!(steps.len(), openings, "the test plan has room for {openings} openings");
    let keys = ["bed-single", "sofa-2", "light-ceiling", "bed-double"];
    for i in 0..assets {
        let g = &rooms[i % rooms.len()];
        let (minx, maxx) = g.polygon.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.x), b.max(p.x)));
        let (miny, maxy) = g.polygon.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.y), b.max(p.y)));
        let round = (i / rooms.len()) as f64;
        let x = minx + (maxx - minx) * (0.3 + 0.2 * (round % 3.0));
        let y = miny + (maxy - miny) * (0.35 + 0.15 * (round % 2.0));
        steps.push(step(
            "add_asset",
            json!({"catalog_key": keys[i % keys.len()], "position": {"x": x, "y": y}}),
        ));
    }
    steps
}

/// The staging the batch tool did before: every step re-previewed the whole
/// staged batch from the open project. Kept here as the reference the new
/// path must match. Returns the final project and, per step, what it
/// created, changed and deleted.
fn old_path(project: &Project, label: &str, steps: &[Value]) -> Result<(Project, Vec<Value>), String> {
    let doc = Document::new(project.clone());
    let level = project.levels[0].id.clone();
    let mut commands: Vec<Command> = vec![];
    let mut staged: Option<ApplyResult> = None;
    let mut per_step = vec![];
    for (i, s) in steps.iter().enumerate() {
        let name = s["tool"].as_str().unwrap();
        let view = match &staged {
            Some(p) => &p.state.project,
            None => doc.project(),
        };
        let next = copilot::to_commands(view, name, &s["args"], &level).map_err(|e| format!("step {}: {}", i + 1, e.0))?;
        commands.extend(next);
        let preview = doc
            .preview(&Command::Batch { label: label.to_string(), commands: commands.clone() })
            .map_err(|e| {
                // How the batch tool worded an engine refusal: the code, the
                // engine's message without its own "Step k of n: " count, the
                // element ids, and the caller's step.
                let ipc: IpcError = e.into();
                let message = match ipc.message.split_once(": ") {
                    Some((head, tail)) if head.starts_with("Step ") && head.contains(" of ") => tail.to_string(),
                    _ => ipc.message.clone(),
                };
                format!(
                    "step {} of {} (`{name}`): {}: {message} (elements: {}) Nothing was applied.",
                    i + 1,
                    steps.len(),
                    ipc.code,
                    ipc.element_ids.join(", ")
                )
            })?;
        let diff = copilot::step_diff(view, &preview.state.project);
        per_step.push(json!({
            "step": i + 1,
            "tool": name,
            "created": diff["added"],
            "changed": diff["modified"],
            "deleted": diff["removed"],
        }));
        staged = Some(preview);
    }
    Ok((staged.unwrap().state.project, per_step))
}

/// A batch with `n` steps of every kind, for timing.
fn house(project: &Project, cols: usize, rows: usize, boxes: usize, openings: usize, assets: usize) -> Vec<Value> {
    let shell = shell_steps(cols, rows, boxes);
    let mut steps = shell.clone();
    steps.extend(fill_steps(project, &shell, openings, assets));
    steps
}

#[tokio::test]
async fn a_200_step_batch_is_fast_and_one_undo_step() {
    let (app, _dir) = app();
    let project = open_project(&app).await;
    // 24 rooms, 40 walls, 60 openings, 76 objects.
    let steps = house(&project, 6, 4, 10, 60, 76);
    assert_eq!(steps.len(), 200);

    let started = Instant::now();
    let out = ok(&app, "batch", json!({"steps": steps, "label": "Whole house"})).await;
    let elapsed = started.elapsed();
    eprintln!("200 step batch: {} ms", elapsed.as_millis());

    assert_eq!(out["undo_label"], "MCP: Whole house");
    assert_eq!(out["revision"], 1);
    assert_eq!(out["steps"].as_array().unwrap().len(), 200);
    {
        let s = app.session.lock().await;
        let doc = s.doc.as_ref().unwrap();
        let count = |k: ElementKind| doc.project().elements.iter().filter(|e| e.kind() == k).count();
        assert_eq!(count(ElementKind::Opening), 60);
        assert_eq!(count(ElementKind::Asset), 76);
        assert_eq!(count(ElementKind::Room), 34, "24 grid rooms and 10 boxes");
    }
    // One undo takes the whole batch back.
    ok(&app, "undo", json!({})).await;
    {
        let s = app.session.lock().await;
        assert!(s.doc.as_ref().unwrap().project().elements.is_empty());
    }
    assert!(
        elapsed.as_millis() < 2000,
        "a 200 step batch took {} ms; it must stay under 2 s even in debug",
        elapsed.as_millis()
    );
}

/// Prints batch times for the sizes the R3 friction log measured. Run with
/// `cargo test -p guhit-mcp --test batch_speed timings -- --ignored --nocapture`.
#[tokio::test]
#[ignore]
async fn timings() {
    for (cols, rows, boxes, openings, assets) in [(2, 2, 0, 4, 5), (3, 3, 1, 12, 20), (4, 3, 2, 16, 37), (6, 4, 10, 60, 76)] {
        let (app, _dir) = app();
        let project = open_project(&app).await;
        let steps = house(&project, cols, rows, boxes, openings, assets);
        let n = steps.len();
        let started = Instant::now();
        ok(&app, "batch", json!({"steps": steps})).await;
        eprintln!("{n} steps: {} ms", started.elapsed().as_millis());
    }
}

#[tokio::test]
async fn batch_results_match_the_old_staging_exactly() {
    let (app, _dir) = app();
    let project = open_project(&app).await;
    let shell = shell_steps(3, 2, 2);
    let mut steps = shell.clone();
    steps.extend(fill_steps(&project, &shell, 8, 8));
    // A move that stretches joined walls and a delete: steps that change and
    // remove what earlier steps made.
    let walls = applied(&project, &shell).derived().rooms[0].wall_ids.clone();
    steps.push(step("move_elements", json!({"ids": [walls[1]], "dx_mm": 200, "dy_mm": 0, "stretch_connected": true})));
    steps.push(step("delete_elements", json!({"ids": [walls[3]]})));

    let (old_project, old_steps) = old_path(&project, "MCP: Equal", &steps).expect("the old path stages it");
    let out = ok(&app, "batch", json!({"steps": steps, "label": "Equal"})).await;
    assert_eq!(out["steps"], Value::Array(old_steps), "per-step created, changed and deleted lists");

    let s = app.session.lock().await;
    let doc = s.doc.as_ref().unwrap();
    assert_eq!(doc.project().elements, old_project.elements, "same elements, same ids");
    assert_eq!(doc.project().levels, old_project.levels);
    assert_eq!(doc.derived(), &guhit_core::compute_derived(&old_project));
}

#[tokio::test]
async fn a_failing_step_applies_nothing_and_names_the_step() {
    let (app, _dir) = app();
    let project = open_project(&app).await;
    let shell = shell_steps(2, 1, 0);
    let mut steps = shell.clone();
    steps.extend(fill_steps(&project, &shell, 2, 0));
    // A door wider than its wall: the engine refuses it.
    let wall = steps[2]["args"]["wall_id"].clone();
    steps.push(step("add_door", json!({"wall_id": wall, "position": "center", "width_mm": 9000})));
    steps.push(step("add_asset", json!({"catalog_key": "sofa-2", "position": {"x": 1000, "y": 1000}})));

    let message = fail(&app, "batch", json!({"steps": steps})).await;
    // Word for word what the old staging said.
    let expected = old_path(&project, "MCP: batch of 6 steps", &steps).expect_err("the old path refused it too");
    assert_eq!(message, expected);
    assert!(message.starts_with(
        "step 5 of 6 (`add_door`): invalid: The door (9000 mm wide, centered 2000 mm from the wall start) does not fit on its wall, which is 4000 mm long."
    ));
    let s = app.session.lock().await;
    let doc = s.doc.as_ref().unwrap();
    assert_eq!(doc.revision(), 0, "nothing was committed");
    assert!(doc.project().elements.is_empty());
}

