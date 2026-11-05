//! Wave 2 gate fixes, MCP side: an agent never undoes a change of the
//! signing architect, a paused agent cannot spend, stairs and room areas
//! are the engine's, `annotate` reports what it stored, sheds flip, the
//! vicinity map can be placed, and tour frames are not kept.

use std::future::Future;

use guhit_app::agents::AgentHint;
use guhit_app::AppService;
use guhit_mcp::agent::CallContext;
use guhit_mcp::fail::ToolFail;
use guhit_mcp::tools::{self, Output};
use guhit_model::*;
use serde_json::{json, Value};

const PIXEL: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

fn app() -> (AppService, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    (AppService::new_sandboxed(dir.path().to_path_buf()), dir)
}

fn agent(label: &str) -> CallContext {
    CallContext::agent(AgentHint { connection: Some(format!("label:{label}")), label: Some(label.to_string()), ..AgentHint::default() })
}

async fn as_agent(app: &AppService, ctx: &CallContext, name: &str, args: Value) -> Result<Value, ToolFail> {
    match tools::call_with(app, name, args, ctx).await? {
        Output::Json(v) | Output::Rich { json: v, .. } => Ok(v),
        Output::Text(t) => Ok(json!({ "text": t })),
    }
}

async fn ok(app: &AppService, name: &str, args: Value) -> Value {
    match tools::call(app, name, args).await {
        Ok(Output::Json(v) | Output::Rich { json: v, .. }) => v,
        Ok(Output::Text(t)) => json!({ "text": t }),
        Err(f) => panic!("{name}: {}", f.display()),
    }
}

async fn fail(app: &AppService, name: &str, args: Value) -> ToolFail {
    match tools::call(app, name, args).await {
        Ok(_) => panic!("{name} should have failed"),
        Err(f) => f,
    }
}

async fn blank(app: &AppService) {
    ok(app, "project", json!({"action": "create", "name": "Fix 1", "template": "blank"})).await;
}

fn project(app: &AppService) -> Project {
    app.session.try_lock().unwrap().doc.as_ref().unwrap().project().clone()
}

fn derived(app: &AppService) -> Derived {
    app.session.try_lock().unwrap().doc.as_ref().unwrap().derived().clone()
}

fn revision(app: &AppService) -> u32 {
    app.session.try_lock().unwrap().doc.as_ref().unwrap().revision()
}

fn new_id(before: &Project, after: &Project, kind: ElementKind) -> Id {
    after
        .elements
        .iter()
        .find(|e| e.kind() == kind && !before.elements.iter().any(|b| b.id() == e.id()))
        .map(|e| e.id().clone())
        .unwrap_or_else(|| panic!("no new {kind:?}"))
}

// ------------------------------------------------------------- GR1 #7

#[tokio::test]
async fn an_agent_never_undoes_or_redoes_a_change_of_the_signing_architect() {
    let (app, _dir) = app();
    blank(&app).await;
    let signatory = Signatory { name: "Arch. Juan Dela Cruz".into(), prc_no: "0123456".into(), ..Signatory::default() };
    let set = Command::SetSignatory { signatory: Box::new(signatory.clone()) };
    app.handle("doc_apply", json!({"command": set})).await.unwrap();
    let a = agent("ground");
    as_agent(&app, &a, "add_walls", json!({"points": [[0, 0], [3000, 0]]})).await.unwrap();
    let before = revision(&app);

    // Several steps at once, and undo_to: refused whole, force or not.
    for args in [json!({"action": "undo", "steps": 2, "force": true}), json!({"action": "undo_to", "index": 1, "force": true})] {
        let f = as_agent(&app, &a, "history", args.clone()).await.unwrap_err();
        assert_eq!(f.code, "forbidden", "{args}: {}", f.display());
        assert!(f.message.contains("Ask the user"), "{}", f.message);
        assert!(f.hint().is_some());
        assert_eq!(revision(&app), before, "{args}: nothing was undone");
    }
    // Its own step goes; the one below it never does.
    as_agent(&app, &a, "history", json!({"action": "undo"})).await.unwrap();
    for (tool, args) in [("history", json!({"action": "undo", "force": true})), ("undo", json!({"force": true}))] {
        assert_eq!(as_agent(&app, &a, tool, args).await.unwrap_err().code, "forbidden", "{tool}");
    }
    assert_eq!(project(&app).title_block.signatory, signatory);

    // Undone by the person: never the agent's to bring back.
    app.handle("doc_undo", json!({})).await.unwrap();
    assert_eq!(project(&app).title_block.signatory, Signatory::default());
    for (tool, args) in [
        ("history", json!({"action": "redo", "force": true})),
        ("history", json!({"action": "redo", "steps": 2, "force": true})),
        ("redo", json!({"force": true})),
    ] {
        assert_eq!(as_agent(&app, &a, tool, args.clone()).await.unwrap_err().code, "forbidden", "{tool} {args}");
    }
    // An in-process MCP caller is held to it too.
    assert_eq!(fail(&app, "history", json!({"action": "redo", "force": true})).await.code, "forbidden");
    assert_eq!(project(&app).title_block.signatory, Signatory::default(), "nothing was redone");
}

// ------------------------------------------------------------- GR1 #8

#[tokio::test]
async fn a_paused_agent_cannot_spend_money_or_start_long_work() {
    let (app, _dir) = app();
    blank(&app).await;
    let a = agent("renderer");
    as_agent(&app, &a, "query", json!({"what": "summary"})).await.unwrap();
    let me = app.agents_list().into_iter().find(|s| s.participant.name == "renderer" || s.participant.name == "Renderer").expect("the agent");
    app.handle("agent_pause", json!({"id": me.participant.id})).await.unwrap();

    for (tool, args) in [
        ("visualize_render", json!({"source_id": "x"})),
        ("render", json!({"action": "start", "wait_seconds": 1})),
        ("render_view", json!({})),
        ("export_tour", json!({"tour": "Walkthrough"})),
    ] {
        let f = as_agent(&app, &a, tool, args).await.unwrap_err();
        assert_eq!(f.code, "agent_paused", "{tool}: {}", f.display());
    }
    // Reads still work.
    as_agent(&app, &a, "query", json!({"what": "summary"})).await.unwrap();
    as_agent(&app, &a, "render", json!({"action": "list"})).await.unwrap();
    as_agent(&app, &a, "look", json!({"mode": "plan"})).await.unwrap();
}

// -------------------------------------------------------- GR1 #10, #11

#[tokio::test]
async fn measure_reads_an_l_stair_as_the_engine_builds_it() {
    let (app, _dir) = app();
    blank(&app).await;
    ok(&app, "levels", json!({"action": "add", "name": "Upper"})).await;
    let before = project(&app);
    ok(&app, "add_stair", json!({"start": {"x": 0, "y": 0}, "riser_count": 18, "shape": "l", "turn": "right"})).await;
    let stair = new_id(&before, &project(&app), ElementKind::Stair);

    // 18 risers in two flights: 16 treads of 250, as the engine, plan and
    // IFC count them.
    let h = ok(&app, "measure", json!({"kind": "height", "ids": [stair]})).await;
    let item = &h["items"][0];
    assert_eq!(item["treads"], 16, "{item}");
    assert_eq!(item["tread_mm"], 250.0, "{item}");
    assert_eq!(item["flights"], json!([9, 9]), "{item}");
    assert_eq!(item["landings"], 1, "{item}");

    // A wall across the second flight, beside where a straight flight of
    // the same run would be: the clearance is 0, they overlap.
    let d = derived(&app);
    let g = d.stairs.iter().find(|g| g.stair_id == stair).unwrap();
    let last = g.flights.last().unwrap();
    let n = last.outline.len() as f64;
    let c = Point { x: last.outline.iter().map(|p| p.x).sum::<f64>() / n, y: last.outline.iter().map(|p| p.y).sum::<f64>() / n };
    let s = match project(&app).elements.iter().find(|e| e.id() == &stair) {
        Some(Element::Stair(s)) => s.clone(),
        _ => unreachable!(),
    };
    // The case tells the two apart: the old straight outline misses it.
    let (sin, cos) = s.rotation_deg.to_radians().sin_cos();
    let (rel_x, rel_y) = (c.x - s.origin.x, c.y - s.origin.y);
    let (across, along) = (rel_x * cos + rel_y * sin, -rel_x * sin + rel_y * cos);
    assert!(across.abs() > s.width_mm / 2.0 + 300.0 || !(0.0..=s.run_mm).contains(&along), "the straight outline would cover it");
    let before = project(&app);
    let wall = json!({"points": [[c.x - last.up.x * 200.0, c.y - last.up.y * 200.0], [c.x + last.up.x * 200.0, c.y + last.up.y * 200.0]], "thickness_mm": 150});
    ok(&app, "add_walls", wall).await;
    let wall = new_id(&before, &project(&app), ElementKind::Wall);
    let gap = ok(&app, "measure", json!({"kind": "clearance", "ids": [stair, wall]})).await;
    assert_eq!(gap["gap_mm"], 0.0, "{gap}");
    assert_eq!(gap["overlap"], true);

    // Its footprint is its flights and landing.
    let area = ok(&app, "measure", json!({"kind": "area", "ids": [stair]})).await;
    let pieces: f64 = guhit_core::stairs::pieces(g).iter().map(|p| {
        let k = p.len();
        (0..k).map(|i| p[i].x * p[(i + 1) % k].y - p[(i + 1) % k].x * p[i].y).sum::<f64>().abs() / 2.0
    }).sum();
    assert_eq!(area["items"][0]["footprint_m2"], ((pieces / 1.0e4).round() / 100.0), "{area}");
}

// ------------------------------------------------------------- GR1 #12

#[tokio::test]
async fn rooms_report_net_and_floor_area_and_the_totals_add_up() {
    let (app, _dir) = app();
    blank(&app).await;
    let before = project(&app);
    ok(&app, "add_room", json!({"rect": {"x": 0, "y": 0, "width_mm": 6000, "depth_mm": 5000}, "name": "Living"})).await;
    let living = new_id(&before, &project(&app), ElementKind::Room);
    let before = project(&app);
    ok(&app, "add_room", json!({"rect": {"x": 9000, "y": 0, "width_mm": 3000, "depth_mm": 3000}, "name": "Lanai"})).await;
    let lanai = new_id(&before, &project(&app), ElementKind::Room);
    // A walled lanai on a deck: net area, but no floor area (D50).
    ok(&app, "add_slab", json!({"slab_kind": "deck", "rect": {"x": 8500, "y": -500, "width_mm": 4000, "depth_mm": 4000}})).await;

    let q = ok(&app, "query", json!({"what": "rooms"})).await;
    let items = q["items"].as_array().unwrap();
    let row = |id: &str| items.iter().find(|r| r["id"] == id).unwrap().clone();
    let (l, d) = (row(&living), row(&lanai));
    assert!(d["net_area_m2"].as_f64().unwrap() > 7.0, "{d}");
    assert_eq!(d["floor_area_m2"], 0.0, "{d}");
    assert_eq!(l["net_area_m2"], l["floor_area_m2"], "{l}");
    let sum = |key: &str| (items.iter().map(|r| r[key].as_f64().unwrap()).sum::<f64>() * 100.0).round() / 100.0;
    assert_eq!(q["total_net_area_m2"], sum("net_area_m2"), "{q}");
    assert_eq!(q["total_floor_area_m2"], sum("floor_area_m2"), "{q}");
    let totals = (derived(&app).totals.floor_area_m2 * 100.0).round() / 100.0;
    assert_eq!(q["total_floor_area_m2"], totals, "the floor total is the Totals'");

    let m = ok(&app, "measure", json!({"kind": "area", "ids": [lanai]})).await;
    assert_eq!(m["items"][0]["net_m2"], d["net_area_m2"], "{m}");
    assert_eq!(m["items"][0]["floor_m2"], 0.0, "{m}");
}

// ------------------------------------------------------------ GR2 note

#[tokio::test]
async fn annotate_reports_the_string_it_stored() {
    let (app, _dir) = app();
    blank(&app).await;
    ok(&app, "add_room", json!({"rect": {"x": 0, "y": 0, "width_mm": 8000, "depth_mm": 5000}, "name": "House", "thickness_mm": 150})).await;
    let south = project(&app)
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Wall(w) if w.start.y.abs() < 1.0 && w.end.y.abs() < 1.0 => Some(w.id.clone()),
            _ => None,
        })
        .unwrap();
    let before = project(&app);
    let out = ok(&app, "annotate", json!({"kind": "string", "walls": [south], "face": "exterior"})).await;
    let id = new_id(&before, &project(&app), ElementKind::Dimension);
    let Some(Element::Dimension(d)) = project(&app).elements.iter().find(|e| e.id() == &id).cloned() else { unreachable!() };
    let mut line = vec![d.a];
    line.extend(d.through.iter().copied());
    line.push(d.b);
    let stored: Vec<f64> = line.windows(2).map(|w| ((w[1].x - w[0].x).hypot(w[1].y - w[0].y) * 10.0).round() / 10.0).collect();
    let overall = ((d.b.x - d.a.x).hypot(d.b.y - d.a.y) * 10.0).round() / 10.0;
    let details = &out["details"][0];
    assert_eq!(details["parts_mm"], json!(stored), "{out}");
    assert_eq!(details["overall_mm"], overall, "{out}");
    assert_eq!(overall, 8150.0, "measured on the outside face");
}

// --------------------------------------------------------------- GS P1

#[tokio::test]
async fn a_shed_roof_flips_through_mcp() {
    let (app, _dir) = app();
    blank(&app).await;
    let defs = tools::definitions();
    let add_roof = defs.iter().find(|d| d.name == "add_roof").unwrap();
    assert_eq!(add_roof.schema["properties"]["flip"]["type"], "boolean");
    assert!(add_roof.description.contains("flip"));
    let modify = defs.iter().find(|d| d.name == "modify").unwrap();
    assert!(modify.description.contains("flip"));

    // A lanai north of the house slopes away from it, toward the north.
    let before = project(&app);
    ok(&app, "add_roof", json!({"points": [[0, 6000], [4000, 6000], [4000, 9000], [0, 9000]], "shape": "shed", "ridge_axis": "y", "flip": true})).await;
    let roof = new_id(&before, &project(&app), ElementKind::Roof);
    let flip = |app: &AppService| match project(app).elements.iter().find(|e| e.id() == &roof) {
        Some(Element::Roof(r)) => r.flip,
        _ => unreachable!(),
    };
    assert!(flip(&app));
    ok(&app, "modify", json!({"ids": [roof], "set": {"flip": false}})).await;
    assert!(!flip(&app));
    ok(&app, "modify", json!({"ids": [roof], "set": {"flip": true}})).await;
    assert!(flip(&app));
    let f = fail(&app, "add_roof", json!({"points": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "shape": "hip", "flip": true})).await;
    assert_eq!(f.code, "bad_args", "{}", f.display());
}

// --------------------------------------------------------------- GS P2s

#[tokio::test]
async fn an_agent_places_the_vicinity_map() {
    let (app, dir) = app();
    blank(&app).await;
    ok(&app, "add_room", json!({"rect": {"x": 0, "y": 0, "width_mm": 6000, "depth_mm": 5000}, "name": "House"})).await;
    let f = fail(&app, "sheets", json!({"action": "vicinity", "image": PIXEL})).await;
    assert_eq!(f.code, "not_found", "no sheet frames one yet: {}", f.display());
    ok(&app, "sheets", json!({"action": "make", "paper": "a3"})).await;

    let out = ok(&app, "sheets", json!({"action": "vicinity", "image": PIXEL, "radius_m": 300})).await;
    let file = out["details"][0]["file_name"].as_str().expect("the stored name").to_string();
    let p = project(&app);
    let files = dir.path().join("projects").join(&p.id).join("files");
    assert!(files.join(&file).is_file(), "{file} in {}", files.display());
    let placed: Vec<&ViewRef> = p.sheet_set.sheets.iter().flat_map(|s| &s.viewports).map(|v| &v.view).filter(|v| matches!(v, ViewRef::VicinityMap { .. })).collect();
    assert!(!placed.is_empty());
    for v in placed {
        assert_eq!(v, &ViewRef::VicinityMap { file_name: Some(file.clone()), radius_m: 300.0 });
    }
    // A project file by name; a missing one is said so; a batch step takes
    // a name, not the image.
    ok(&app, "sheets", json!({"action": "vicinity", "file_name": file})).await;
    assert_eq!(fail(&app, "sheets", json!({"action": "vicinity", "file_name": "nope.png"})).await.code, "not_found");
    let f = fail(&app, "batch", json!({"steps": [{"tool": "sheets", "args": {"action": "vicinity", "image": PIXEL}}]})).await;
    assert_eq!(f.code, "bad_args", "{}", f.display());
}

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

#[tokio::test]
async fn a_tour_frame_is_not_kept_in_visuals() {
    let (app, _dir) = app();
    ok(&app, "project", json!({"action": "create", "name": "Bahay", "template": "sample-bungalow"})).await;
    let made = match tools::call(&app, "make_tour", json!({"name": "Walkthrough"})).await {
        Ok(Output::Json(v) | Output::Rich { json: v, .. }) => v,
        Ok(_) => panic!("make_tour"),
        Err(f) if f.code == "not_built" => return,
        Err(f) => panic!("make_tour: {}", f.display()),
    };
    assert!(made.get("id").is_some(), "{made}");
    // The window saves its capture to Visuals, as the real one does.
    let window = fake_window(&app, |app, request| async move {
        let WindowTask::CaptureView { camera: Some(camera), .. } = request.task else {
            return Err(IpcError::new("invalid", "not this"));
        };
        let record: RenderRecord = serde_json::from_value(app.handle("render_capture", json!({"camera": camera, "png": PIXEL})).await?).unwrap();
        Ok(WindowReply { render_ids: vec![record.id], image: Some(PIXEL.into()), note: "Captured.".into() })
    });
    let out = ok(&app, "look", json!({"mode": "tour", "tour": "Walkthrough", "at_s": 1.0})).await;
    window.abort();
    assert_eq!(out["status"], "done", "{out}");
    assert_eq!(out["kept"], false, "{out}");
    let list = ok(&app, "render", json!({"action": "list"})).await;
    let records = list.as_array().cloned().or_else(|| list["renders"].as_array().cloned()).unwrap_or_default();
    assert!(records.is_empty(), "no frame stays in Visuals: {list}");
}

#[tokio::test]
async fn a_running_render_points_at_the_v2_tool() {
    let (app, _dir) = app();
    blank(&app).await;
    // A window that never answers: the render is still running.
    let window = fake_window(&app, |_, _| async {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        Err(IpcError::new("invalid", "late"))
    });
    let out = ok(&app, "render", json!({"action": "start", "wait_seconds": 1})).await;
    window.abort();
    assert_eq!(out["status"], "running", "{out}");
    let note = out["note"].as_str().unwrap();
    assert!(note.contains("render with action \"status\""), "{note}");
    assert!(!note.contains("get_render_job"), "{note}");
}

// ------------------------------------------------------------- GR1 #14

#[tokio::test]
async fn opening_a_project_with_a_keyless_shot_reports_the_repair() {
    let (app, dir) = app();
    ok(&app, "project", json!({"action": "create", "name": "Bahay", "template": "sample-bungalow"})).await;
    let made = match tools::call(&app, "make_tour", json!({"name": "Walkthrough"})).await {
        Ok(Output::Json(v) | Output::Rich { json: v, .. }) => v,
        Ok(_) => panic!("make_tour"),
        Err(f) => panic!("make_tour: {}", f.display()),
    };
    assert!(made.get("id").is_some(), "{made}");
    let id = project(&app).id;
    app.handle("hub_close", json!({})).await.unwrap();
    let clean: Value = app.handle("hub_open", json!({"id": id})).await.unwrap();
    assert!(clean.get("repairs").is_none(), "nothing to mend yet");

    // A damaged or shared file: the first shot has lost its camera keys.
    app.handle("hub_close", json!({})).await.unwrap();
    let file = dir.path().join("projects").join(&id).join("project.json");
    let mut raw: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    let tour = raw["elements"].as_array_mut().unwrap().iter_mut().find(|e| e["shots"].is_array()).expect("the tour");
    let shots = tour["shots"].as_array().unwrap().len();
    tour["shots"][0]["keys"] = json!([]);
    std::fs::write(&file, serde_json::to_vec(&raw).unwrap()).unwrap();

    // The window's open answers what was mended.
    let opened: Value = app.handle("hub_open", json!({"id": id})).await.unwrap();
    let repairs = opened["repairs"].as_array().cloned().unwrap_or_else(|| panic!("repairs: {}", opened["repairs"]));
    assert_eq!(repairs.len(), 1, "{repairs:?}");
    let note = repairs[0].as_str().unwrap().to_string();
    assert!(note.contains("Walkthrough") && note.contains("1 shot") && note.contains("no camera keys"), "{note}");
    let state: DocState = serde_json::from_value(opened).expect("still a DocState");
    let kept = state.project.elements.iter().find_map(|e| match e {
        Element::Tour(t) => Some(t.shots.len()),
        _ => None,
    });
    assert_eq!(kept, Some(shots - 1));

    // So does MCP project open, for the agent to tell the user.
    app.handle("hub_close", json!({})).await.unwrap();
    let out = ok(&app, "project", json!({"action": "open", "id": id})).await;
    assert_eq!(out["repairs"], json!([note]), "{out}");
    assert!(out["repairs_note"].as_str().unwrap().contains("Tell the user"));
}
