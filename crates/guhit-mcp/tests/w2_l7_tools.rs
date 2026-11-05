//! Lane L7, MCP v2 for Wave 2 (DECISIONS D53, D54): the 26 core tools, the
//! documents and tours toolsets, the new look and query modes, and the rules
//! every tool keeps: review items in each edit result, a hint on each
//! failure, v1 names still answered.
//!
//! What another lane builds may still answer `not_built`: those calls are
//! checked to fail with that code and a hint, and to apply nothing, until
//! the lane lands; then they are checked to work.

use guhit_app::AppService;
use guhit_mcp::fail::ToolFail;
use guhit_mcp::tools::{self, Output};
use guhit_mcp::toolsets::{self, Toolsets};
use guhit_model::*;
use serde_json::{json, Value};

fn app() -> (AppService, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    (AppService::new_sandboxed(dir.path().to_path_buf()), dir)
}

async fn call(app: &AppService, name: &str, args: Value) -> Result<Value, ToolFail> {
    match tools::call(app, name, args).await? {
        Output::Json(v) => Ok(v),
        Output::Rich { json, images } => {
            let mut v = json;
            v["_images"] = json!(images.len());
            Ok(v)
        }
        Output::Text(t) => Ok(json!({ "text": t })),
    }
}

async fn ok(app: &AppService, name: &str, args: Value) -> Value {
    call(app, name, args).await.unwrap_or_else(|f| panic!("`{name}` failed: {}", f.display()))
}

async fn fail(app: &AppService, name: &str, args: Value) -> ToolFail {
    match call(app, name, args).await {
        Ok(v) => panic!("`{name}` should have failed, got {v}"),
        Err(f) => f,
    }
}

fn project(app: &AppService) -> Project {
    let s = app.session.try_lock().expect("session free");
    s.doc.as_ref().unwrap().project().clone()
}

fn revision(app: &AppService) -> u32 {
    let s = app.session.try_lock().expect("session free");
    s.doc.as_ref().unwrap().revision()
}

async fn bungalow(app: &AppService) {
    ok(app, "project", json!({"action": "create", "name": "Bahay", "template": "sample-bungalow"})).await;
}

/// An edit that may wait on another lane: it works, or it says `not_built`
/// with a hint and changes nothing.
async fn works_or_not_built(app: &AppService, name: &str, args: Value) -> Option<Value> {
    let before = revision(app);
    match call(app, name, args).await {
        Ok(v) => {
            assert!(v.get("issues_added").is_some() && v.get("issues_resolved").is_some(), "`{name}` result lists review items: {v}");
            Some(v)
        }
        Err(f) => {
            assert_eq!(f.code, "not_built", "`{name}`: {}", f.display());
            assert!(f.hint().is_some_and(|h| !h.is_empty()));
            assert_eq!(revision(app), before, "a refused edit changes nothing");
            None
        }
    }
}

#[test]
fn the_core_toolset_has_26_tools_and_two_new_toolsets() {
    let core = Toolsets::core().tools();
    assert_eq!(core.len(), 26);
    for t in ["add_grid", "annotate", "add_roof", "types"] {
        assert!(core.contains(&t), "{t} is a core tool");
    }
    assert_eq!(toolsets::tools_of("documents"), ["title_block", "sheets", "view_marks", "set_lot"]);
    assert_eq!(toolsets::tools_of("tours"), ["make_tour", "edit_tour", "export_tour"]);
    assert!(toolsets::tools_of("visuals").contains(&"set_look"));
    assert_eq!(Toolsets::all().tools().len(), 45);
    let defs = tools::definitions();
    let look = defs.iter().find(|d| d.name == "look").unwrap();
    assert_eq!(look.schema["properties"]["mode"]["enum"], json!(["plan", "3d", "elevation", "section", "sheet", "schedule", "tour"]));
    let stair = defs.iter().find(|d| d.name == "add_stair").unwrap();
    assert_eq!(stair.schema["properties"]["shape"]["enum"], json!(["straight", "l", "u"]));
    let query = defs.iter().find(|d| d.name == "query").unwrap();
    for w in ["schedules", "site", "catalog", "placements", "tour"] {
        assert!(query.schema["properties"]["what"]["enum"].as_array().unwrap().contains(&json!(w)), "query what {w}");
    }
}

#[tokio::test]
async fn an_l_stair_and_an_area_roof_with_posts() {
    let (app, _dir) = app();
    bungalow(&app).await;
    ok(&app, "levels", json!({"action": "add", "name": "Second Floor"})).await;
    let stair = ok(&app, "add_stair", json!({"start": {"x": 6500, "y": 500}, "shape": "l", "turn": "left", "width_mm": 900})).await;
    assert_eq!(stair["details"][0]["shape"], "l");
    assert_eq!(stair["details"][0]["treads"], 16, "18 risers, two flights: n - 2 treads");
    let s = project(&app).elements.iter().find_map(|e| match e {
        Element::Stair(s) if s.shape == StairShape::L => Some(s.clone()),
        _ => None,
    });
    assert_eq!(s.unwrap().riser_count, 18);
    // A carport roof over drawn points, with posts under its open eaves.
    let carport = [[8000, 0], [11000, 0], [11000, 6000], [8000, 6000]];
    let roof = ok(&app, "add_roof", json!({"points": carport, "shape": "shed", "ridge_axis": "x", "pitch_deg": 5})).await;
    let roof_id = roof["id"].as_str().expect("the roof's id").to_string();
    assert!(project(&app).elements.iter().any(|e| matches!(e, Element::Roof(r) if r.id == roof_id && r.shape == RoofShape::Shed)));
    let posts = works_or_not_built(&app, "add_roof", json!({"points": [[-3000, 0], [0, 0], [0, 3000], [-3000, 3000]], "shape": "flat", "pitch_deg": 0, "posts": true})).await;
    if let Some(v) = posts {
        assert!(v["created"].as_array().unwrap().iter().any(|c| c["kind"] == "column"), "posts are columns: {v}");
    }
    // The house roof: changing it keeps its element.
    let before = project(&app).top_roof_area().unwrap().id.clone();
    ok(&app, "add_roof", json!({"shape": "hip", "pitch_deg": 25})).await;
    let after = project(&app);
    assert_eq!(after.top_roof_area().unwrap().id, before);
    assert_eq!(after.top_roof().kind, RoofKind::Hip);
    // A crooked drawn hip is refused with the engine's code and a hint.
    let f = fail(&app, "add_roof", json!({"points": [[0, 8000], [4000, 8000], [3000, 11000]], "shape": "hip"})).await;
    assert!(f.hint().is_some(), "{}", f.display());
}

#[tokio::test]
async fn grids_strings_notes_and_types() {
    let (app, _dir) = app();
    bungalow(&app).await;
    // Single grid lines work today; a grid system waits on L1 when it has not landed.
    let lines = ok(&app, "add_grid", json!({"lines": [{"label": "A", "start": [-1000, 0], "end": [9000, 0]}, {"label": "B", "start": [-1000, 6000], "end": [9000, 6000]}]})).await;
    assert_eq!(lines["created"].as_array().unwrap().len(), 2);
    works_or_not_built(&app, "add_grid", json!({"x_spacings_mm": [4000, 4000], "y_spacings_mm": [3000, 3000]})).await;
    let grids: Vec<Id> = project(&app).elements.iter().filter_map(|e| match e {
        Element::Grid(g) if g.label == "A" || g.label == "B" => Some(g.id.clone()),
        _ => None,
    }).collect();
    // A string between grids, a string through points, a note.
    let between = ok(&app, "annotate", json!({"kind": "string", "grids": grids, "offset_mm": 0})).await;
    assert_eq!(between["details"][0]["overall_mm"], 6000.0);
    let through = ok(&app, "annotate", json!({"kind": "string", "points": [[0, -1200], [5000, -1200], [8000, -1200]]})).await;
    assert_eq!(through["details"][0]["parts_mm"], json!([5000.0, 3000.0]));
    let d = project(&app).elements.iter().find_map(|e| match e {
        Element::Dimension(d) if d.through.len() == 1 => Some(d.clone()),
        _ => None,
    });
    assert_eq!(d.unwrap().through, vec![Point { x: 5000.0, y: -1200.0 }]);
    let note = ok(&app, "annotate", json!({"kind": "note", "text": "FINISH GRADE +0.150", "at": [0, -2000]})).await;
    assert_eq!(note["created"][0]["kind"], "annotation");
    works_or_not_built(&app, "annotate", json!({"kind": "auto", "kinds": ["exterior"]})).await;
    // Types: list reads; set waits on L1 when it has not landed.
    let list = ok(&app, "types", json!({"action": "list"})).await;
    assert!(list["untyped_openings"].as_u64().unwrap() > 0);
    if let Some(v) = works_or_not_built(&app, "types", json!({"action": "set", "kind": "door", "width_mm": 900, "height_mm": 2100, "description": "Flush door, painted"})).await {
        assert_eq!(v["types_added"].as_array().unwrap().len(), 1, "{v}");
        let list = ok(&app, "types", json!({"action": "list"})).await;
        assert_eq!(list["types"][0]["mark"], "D1");
    }
    assert_eq!(fail(&app, "types", json!({"action": "apply", "type": "Z9", "openings": ["x"]})).await.code, "not_found");
}

#[tokio::test]
async fn the_title_block_never_writes_the_signatory_and_reads_masked() {
    let (app, _dir) = app();
    bungalow(&app).await;
    let f = fail(&app, "title_block", json!({"action": "set", "prc_no": "0123456"})).await;
    assert_eq!(f.code, "forbidden");
    assert!(f.hint().unwrap().contains("title block form"));
    let set = ok(&app, "title_block", json!({"action": "set", "tct_no": "T-004-2026-12345", "lot_no": "7", "block_no": "3"})).await;
    assert_eq!(set["title_block_changed"], true);
    assert!(set.get("issues_added").is_some());
    // A person sets the signatory; an agent reads it masked.
    let signatory = Signatory { name: "Ana Reyes".into(), prc_no: "0123456".into(), tin: "123-456-789-000".into(), ..Signatory::default() };
    app.handle("doc_apply", json!({"command": Command::SetSignatory { signatory: Box::new(signatory) }})).await.unwrap();
    let got = ok(&app, "title_block", json!({"action": "get"})).await;
    assert_eq!(got["title_block"]["tct_no"], "T-004-2026-12345");
    let text = got.to_string();
    assert!(!text.contains("0123456") && !text.contains("123-456-789-000"), "masked: {text}");
    assert_eq!(got["signatory_set"], true);
    assert_eq!(project(&app).title_block.signatory.prc_no, "0123456", "the project keeps it in full");
}

#[tokio::test]
async fn marks_sheets_and_the_lot() {
    let (app, _dir) = app();
    bungalow(&app).await;
    let mark = ok(&app, "view_marks", json!({"action": "add", "kind": "elevation", "label": "South", "start": [-1000, -3000], "end": [9000, -3000], "flip": false})).await;
    assert_eq!(mark["created"][0]["kind"], "view_mark");
    let list = ok(&app, "view_marks", json!({"action": "list"})).await;
    assert_eq!(list["count"], 1);
    // The elevation's drawing: worked out by the engine (L3), or not built yet.
    match call(&app, "look", json!({"mode": "elevation", "mark": "south"})).await {
        Ok(v) => {
            assert_eq!(v["_images"], 1);
            assert_eq!(v["mark"]["label"], "South");
        }
        Err(f) => assert_eq!(f.code, "not_built", "{}", f.display()),
    }
    let missing = fail(&app, "look", json!({"mode": "section", "mark": "Z"})).await;
    assert_eq!(missing.code, "not_found");
    assert!(missing.hint().unwrap().contains("view_marks"));
    // Sheets: list reads; make waits on L1 when it has not landed.
    let none = ok(&app, "sheets", json!({"action": "list"})).await;
    assert_eq!(none["count"], 0);
    if works_or_not_built(&app, "sheets", json!({"action": "make", "paper": "a1"})).await.is_some() {
        let set = ok(&app, "sheets", json!({"action": "list"})).await;
        assert!(set["count"].as_u64().unwrap() >= 2, "{set}");
    }
    // The lot from a TCT description.
    let tct = "Beginning at a point marked \"1\" on plan, being N. 45 deg. 30' E., 1,234.56 m. from BLLM No. 1; \
        thence S. 89 deg. 30' E., 12.00 m. to point 2; thence S. 00 deg. 30' W., 15.00 m. to point 3; \
        thence N. 89 deg. 30' W., 12.00 m. to point 4; thence N. 00 deg. 30' E., 15.00 m. to the point of beginning.";
    let lot = works_or_not_built(&app, "set_lot", json!({"description": tct, "start": [-2000, 12000], "sides": [{"role": "rear", "setback_mm": 2000}, {"role": "side", "setback_mm": 1500}, {"role": "road", "setback_mm": 3000, "road": "Mabini St.", "road_width_mm": 8000}, {"role": "side", "setback_mm": 1500}]})).await;
    if let Some(v) = lot {
        assert_eq!(v["details"][0]["legs"].as_array().unwrap().len(), 4);
        let site = ok(&app, "query", json!({"what": "site"})).await;
        assert!(site.is_object());
    }
}

#[tokio::test]
async fn queries_for_schedules_site_placements_and_the_catalog() {
    let (app, _dir) = app();
    bungalow(&app).await;
    let s = ok(&app, "query", json!({"what": "schedules"})).await;
    for k in ["openings", "rooms", "levels"] {
        assert!(s.get(k).is_some(), "{k}: {s}");
    }
    let site = ok(&app, "query", json!({"what": "site"})).await;
    assert!(site.get("note").is_some() || site.is_null());
    let placements = ok(&app, "query", json!({"what": "placements", "room": "Bedroom", "catalog_key": "bed-double"})).await;
    assert!(placements.get("options").is_some(), "{placements}");
    assert_eq!(fail(&app, "query", json!({"what": "placements", "room": "Nowhere", "catalog_key": "bed-double"})).await.code, "not_found");
    let catalog = ok(&app, "query", json!({"what": "catalog", "search": "bed"})).await;
    assert!(catalog["count"].as_u64().unwrap() > 0);
    assert_eq!(fail(&app, "query", json!({"what": "summary", "tour": "x"})).await.code, "bad_args");
    // A schedule as data, drawn when the sheet layout is built.
    let sched = ok(&app, "look", json!({"mode": "schedule", "schedule": "doors_windows"})).await;
    assert!(sched.get("rows").is_some());
}

#[tokio::test]
async fn a_tour_is_made_edited_and_exported_through_the_window() {
    let (app, _dir) = app();
    bungalow(&app).await;
    let Some(made) = works_or_not_built(&app, "make_tour", json!({"name": "Walkthrough", "pace_mps": 0.8})).await else { return };
    let tour_id = made["id"].as_str().expect("the tour's id").to_string();
    let frames = ok(&app, "query", json!({"what": "tour", "tour": "walkthrough", "page_size": 2})).await;
    assert!(frames["count"].as_u64().unwrap() > 10, "{frames}");
    assert_eq!(frames["items"].as_array().unwrap().len(), 2);
    let edited = ok(&app, "edit_tour", json!({"tour": tour_id, "fps": 24, "shots": [{"shot": 1, "seconds": 4}]})).await;
    assert_eq!(edited["changed"][0]["kind"], "tour");
    // Without a window the export says so, with a hint.
    let f = fail(&app, "export_tour", json!({"tour": "Walkthrough"})).await;
    assert_eq!(f.code, "no_window");
    assert!(f.hint().is_some());
    // With one: a job, its id, then the video record.
    let mut events = app.events();
    let window = {
        let app = app.clone();
        tokio::spawn(async move {
            while let Ok(event) = events.recv().await {
                let AppEvent::WindowRequest { request } = event else { continue };
                let WindowTask::ExportTour { size, .. } = request.task else { continue };
                assert_eq!(size, RenderSize::Hd);
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                let reply = json!({"render_ids": [], "image": null, "note": "Exported 24 s at 1920 x 1080, H.264."});
                app.handle("window_reply", json!({"id": request.id, "reply": reply})).await.unwrap();
            }
        })
    };
    let started = ok(&app, "export_tour", json!({"tour": "Walkthrough", "wait_seconds": 1})).await;
    assert_eq!(started["status"], "running");
    let job = started["job_id"].as_str().unwrap().to_string();
    let done = ok(&app, "export_tour", json!({"job_id": job, "wait_seconds": 10})).await;
    assert_eq!(done["status"], "done");
    assert!(done["note"].as_str().unwrap().contains("H.264"));
    window.abort();
}

#[tokio::test]
async fn every_failure_has_a_hint_and_v1_names_still_answer() {
    let (app, _dir) = app();
    bungalow(&app).await;
    for (tool, args) in [
        ("add_grid", json!({})),
        ("annotate", json!({"kind": "note"})),
        ("add_roof", json!({"outline": "nope"})),
        ("types", json!({"action": "delete", "types": ["W99"]})),
        ("sheets", json!({"action": "preview", "sheet": "A-99"})),
        ("view_marks", json!({"action": "delete", "ids": ["nope"]})),
        ("set_lot", json!({"legs": [{"bearing": "N 1-0 E", "distance_mm": 1000}]})),
        ("edit_tour", json!({"tour": "nope"})),
        ("set_look", json!({})),
        ("look", json!({"mode": "tour"})),
        ("query", json!({"what": "tour"})),
        ("add_stair", json!({"start": [0, 0], "shape": "u", "riser_count": 3})),
    ] {
        let f = fail(&app, tool, args).await;
        let hint = f.hint().unwrap_or_default();
        assert!(!hint.is_empty(), "`{tool}` failed without a hint: {}", f.display());
        assert_eq!(f.structured()["hint"], json!(hint));
    }
    // v1 names answer as before.
    let rooms = ok(&app, "list_rooms", json!({})).await;
    assert!(rooms["rooms"].as_array().unwrap().len() >= 2);
    ok(&app, "add_rect_room", json!({"origin": {"x": 9000, "y": 0}, "width_mm": 3000, "depth_mm": 3000, "name": "Office"})).await;
    assert!(project(&app).elements.iter().any(|e| matches!(e, Element::Room(r) if r.name == "Office")));
}

/// L6's R2 (D53): an agent's AI image shows its price first, counts against
/// the agents' daily cap and says so when the cap is reached; never a real
/// provider (a fake answers).
#[tokio::test]
async fn an_agents_ai_image_shows_its_price_and_meets_the_daily_cap() {
    use guhit_app::agents::AgentHint;
    use guhit_app::render_ai::keys::MemoryKeyStore;
    use guhit_app::render_ai::provider::FakeProvider;
    use guhit_app::render_ai::RenderAiState;
    use guhit_mcp::agent::CallContext;
    use std::sync::Arc;

    let dir = tempfile::tempdir().unwrap();
    let mut app = AppService::new_sandboxed(dir.path().to_path_buf());
    let fake = Arc::new(FakeProvider::new());
    app.render_ai = Arc::new(RenderAiState::with_parts(fake.clone(), Box::new(MemoryKeyStore::with_key("AIzaTEST-L7-NEVER-A-REAL-KEY-0000000"))));
    bungalow(&app).await;
    // A model view to start from.
    let camera = Camera {
        id: String::new(),
        name: "Front".into(),
        preset: CameraPreset::Custom,
        position: Vec3 { x: 4000.0, y: -10000.0, z: 1600.0 },
        target: Vec3 { x: 4000.0, y: 0.0, z: 1600.0 },
        fov_deg: 45.0,
        light: None,
    };
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    let source: RenderRecord = serde_json::from_value(app.handle("render_capture", json!({"camera": camera, "png": png})).await.unwrap()).unwrap();
    app.handle("render_ai_usage_set", json!({"agent_daily_cap": 1})).await.unwrap();
    let agent = CallContext::agent(AgentHint { connection: Some("label:renders".into()), label: Some("renders".into()), ..AgentHint::default() });
    let args = json!({"source_render_id": source.id, "prompt": "Modern tropical, late afternoon", "quality": "draft", "use_maps": false});
    let out = match tools::call_with(&app, "visualize_render", args.clone(), &agent).await {
        Ok(Output::Rich { json, images }) => {
            assert_eq!(images.len(), 1);
            json
        }
        Ok(_) => panic!("a picture"),
        Err(f) => panic!("{}", f.display()),
    };
    assert!(out["price_usd"].as_str().is_some_and(|p| !p.is_empty()), "{out}");
    assert_eq!(out["agents_today"]["cap"], 1);
    assert!(out["label"].as_str().unwrap().starts_with("AI visualization"));
    assert_eq!(fake.calls(), 1);
    let refused = tools::call_with(&app, "visualize_render", args, &agent).await.err().expect("the cap of 1 is reached");
    assert_eq!(refused.code, "rate_limited", "{}", refused.display());
    assert_eq!(fake.calls(), 1, "nothing spent on the refused call");
}
