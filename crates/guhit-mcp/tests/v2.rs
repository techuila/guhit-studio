//! MCP v2: the core toolset, `build`, refs, the structured errors, issues in
//! results, toolsets over HTTP and agents. The calls an MCP client makes,
//! against a real `AppService` on a throwaway data dir.

use guhit_app::agents::AgentHint;
use guhit_app::AppService;
use guhit_mcp::agent::CallContext;
use guhit_mcp::fail::ToolFail;
use guhit_mcp::tools::{self, Output, Picture};
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
        Ok(Output::Text(t)) => json!({ "text": t }),
        Err(f) => panic!("`{name}` failed: {}", f.display()),
    }
}

async fn rich(app: &AppService, name: &str, args: Value) -> (Value, Vec<Picture>) {
    match tools::call(app, name, args).await {
        Ok(Output::Rich { json, images }) => (json, images),
        Ok(_) => panic!("`{name}` returned no picture"),
        Err(f) => panic!("`{name}` failed: {}", f.display()),
    }
}

async fn fail(app: &AppService, name: &str, args: Value) -> ToolFail {
    match tools::call(app, name, args).await {
        Err(f) => f,
        Ok(_) => panic!("`{name}` was expected to fail"),
    }
}

async fn blank(app: &AppService) {
    ok(app, "project", json!({"action": "create", "name": "V2", "template": "blank"})).await;
}

fn project(app: &AppService) -> Project {
    let s = app.session.try_lock().expect("session free");
    s.doc.as_ref().unwrap().project().clone()
}

fn count(p: &Project, kind: ElementKind) -> usize {
    p.elements.iter().filter(|e| e.kind() == kind).count()
}

/// A two level house in one `build`: a double-height living room with a
/// stair under a void, a kitchen, a bedroom upstairs onto a balcony.
fn house_spec() -> Value {
    json!({
        "label": "Two storey house",
        "spec": {
            "levels": [{"ref": "upper", "name": "Upper floor", "height_mm": 3000}],
            "rooms": [
                {"ref": "living", "rect": {"x": 0, "y": 0, "width_mm": 6000, "depth_mm": 5000}, "name": "Living", "usage": "living", "height_mm": 6000},
                {"ref": "kitchen", "rect": {"x": 6000, "y": 0, "width_mm": 4000, "depth_mm": 5000}, "name": "Kitchen", "usage": "kitchen"},
                {"ref": "bed", "rect": {"x": 6000, "y": 0, "width_mm": 4000, "depth_mm": 5000}, "name": "Bedroom", "usage": "bedroom", "level": "$upper"},
                {"ref": "gallery", "polygon": [{"x": 0, "y": 0}, {"x": 6000, "y": 0}, {"x": 6000, "y": 5000}, {"x": 0, "y": 5000}], "name": "Gallery", "level": "$upper"}
            ],
            "columns": [{"ref": "col", "points": [{"x": -1000, "y": -1000}, {"x": 11000, "y": -1000}], "to_level": "$upper", "material_id": "mat-concrete-fairface"}],
            "slabs": [
                {"ref": "void", "slab_kind": "void", "level": "$upper", "rect": {"x": 300, "y": 300, "width_mm": 4500, "depth_mm": 4400}},
                {"ref": "balcony", "kind": "balcony", "level": "$upper", "rect": {"x": 6000, "y": -1500, "width_mm": 4000, "depth_mm": 1500}}
            ],
            "stairs": [{"ref": "stair", "start": {"x": 4200, "y": 500}, "direction_deg": 90, "to_level": "$upper"}],
            "openings": [
                {"ref": "front", "kind": "door", "at": {"x": 3000, "y": 0}, "swing_into": "$living"},
                {"ref": "inner", "kind": "door", "at": {"x": 6000, "y": 2500}, "swing_into": "$kitchen"},
                {"kind": "window", "at": {"x": 8000, "y": 0}, "style": "sliding"},
                {"ref": "to_balcony", "kind": "door", "at": {"x": 8000, "y": 0}, "level": "$upper", "style": "sliding", "width_mm": 1800}
            ],
            "assets": [{"ref": "bed_item", "catalog_key": "bed-double", "at": {"x": 8000, "y": 3000}, "level": "$upper"}],
            "cameras": [{"name": "Front", "position": {"x": 5000, "y": -8000, "z": 1600}, "target": {"x": 5000, "y": 2500, "z": 2500}}]
        }
    })
}

#[tokio::test]
async fn a_two_level_house_with_a_stair_a_void_and_a_balcony_in_one_build() {
    let (app, _dir) = app();
    blank(&app).await;

    // A dry run commits nothing and says what would happen.
    let mut dry = house_spec();
    dry["dry_run"] = json!(true);
    let preview = ok(&app, "build", dry).await;
    assert_eq!(preview["dry_run"], true);
    assert_eq!(preview["revision"], 0);
    assert_eq!(count(&project(&app), ElementKind::Wall), 0, "a dry run commits nothing");

    let out = ok(&app, "build", house_spec()).await;
    assert_eq!(out["revision"], 1, "one undo step");
    assert_eq!(out["undo_label"], "MCP: Two storey house");
    for r in ["upper", "living", "kitchen", "bed", "gallery", "col", "void", "balcony", "stair", "front", "inner", "to_balcony", "bed_item"] {
        assert!(out["refs"][r].is_string(), "ref `{r}` has no id: {}", out["refs"]);
    }
    let p = project(&app);
    assert_eq!(p.levels.len(), 2);
    let upper = p.levels.iter().find(|l| l.name == "Upper floor").unwrap();
    assert_eq!(upper.elevation_mm, 3000.0);
    assert_eq!(count(&p, ElementKind::Stair), 1);
    assert_eq!(count(&p, ElementKind::Slab), 2);
    assert_eq!(count(&p, ElementKind::Column), 4, "two columns on each of two levels");
    assert_eq!(count(&p, ElementKind::Opening), 4);
    assert_eq!(count(&p, ElementKind::Camera), 1);
    // The double-height room's walls stand 6000 high.
    let living = out["refs"]["living"].as_str().unwrap();
    let room = p.elements.iter().find_map(|e| match e {
        Element::Room(r) if r.id == living => Some(r.clone()),
        _ => None,
    });
    assert_eq!(room.unwrap().usage, RoomUsage::Living);
    assert!(p.elements.iter().any(|e| matches!(e, Element::Wall(w) if w.height_mm == Some(6000.0))));
    // Slabs keep their kind; the balcony got its rail.
    let slab = |id: &str| p.elements.iter().find_map(|e| match e {
        Element::Slab(s) if s.id == id => Some(s.clone()),
        _ => None,
    });
    assert_eq!(slab(out["refs"]["void"].as_str().unwrap()).unwrap().kind, SlabKind::Void);
    let balcony = slab(out["refs"]["balcony"].as_str().unwrap()).unwrap();
    assert_eq!(balcony.kind, SlabKind::Balcony);
    assert_eq!(balcony.railing.unwrap().height_mm, 1000.0);
    // The doors swing into the rooms they were told to.
    let opening = |id: &str| p.elements.iter().find_map(|e| match e {
        Element::Opening(o) if o.id == id => Some(o.clone()),
        _ => None,
    });
    assert!(!opening(out["refs"]["front"].as_str().unwrap()).unwrap().flip_side, "the front door swings north, into the living room");
    assert!(opening(out["refs"]["inner"].as_str().unwrap()).unwrap().flip_side, "the inner door swings east, into the kitchen");
    assert_eq!(opening(out["refs"]["to_balcony"].as_str().unwrap()).unwrap().style, OpeningStyle::Sliding);
    // The stair's figures come back with the result.
    let stair = out["details"].as_array().unwrap().iter().find(|d| d["ref"] == "stair").unwrap();
    assert_eq!(stair["riser_mm"], 166.7);
    assert_eq!(stair["treads"], 17);
    // Per step lists, and the review items the build added.
    assert_eq!(out["steps"].as_array().unwrap().len(), 15, "one per spec entry");
    assert!(out["issues_added"].is_array());
    assert_eq!(out["base_revision"], 0);

    // One undo takes the whole house back.
    ok(&app, "history", json!({"action": "undo"})).await;
    assert_eq!(project(&app).elements.len(), 0);
    assert_eq!(project(&app).levels.len(), 1);
}

#[tokio::test]
async fn look_measure_and_check_verify_the_build() {
    let (app, _dir) = app();
    blank(&app).await;
    let out = ok(&app, "build", house_spec()).await;
    let refs = out["refs"].clone();

    // The plan of the upper floor, drawn by the engine with no window.
    let (plan, images) = rich(&app, "look", json!({"mode": "plan", "level": "Upper floor", "show": ["dims", "ids", "grid", "scale_bar", "labels"]})).await;
    assert_eq!(plan["source"], "engine");
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].mime, "image/png");
    let ids = plan["ids"].as_object().unwrap();
    assert!(ids.values().any(|v| v == &refs["bed"]), "the short ids map to the bedroom");
    assert!(plan["px_per_mm"].as_f64().unwrap() > 0.0);
    // 3D needs the window; section is not in this version.
    assert_eq!(fail(&app, "look", json!({"mode": "3d"})).await.code, "no_window");
    let section = fail(&app, "look", json!({"mode": "section"})).await;
    assert_eq!(section.code, "unavailable");
    assert!(section.hint().unwrap().contains("measure"));

    // Measure: the stair, the levels, a room, two parallel walls, a gap.
    let h = ok(&app, "measure", json!({"kind": "height", "ids": [refs["stair"], "Ground Floor"]})).await;
    assert_eq!(h["items"][0]["riser_mm"], 166.7);
    assert_eq!(h["items"][0]["tread_mm"], 250.0);
    assert_eq!(h["items"][1]["floor_to_floor_mm"], 3000.0);
    let area = ok(&app, "measure", json!({"kind": "area", "ids": [refs["kitchen"]]})).await;
    assert_eq!(area["items"][0]["net_m2"], 18.67, "3850 x 4850 mm");
    let d = ok(&app, "measure", json!({"kind": "distance", "from": {"x": 0, "y": 0}, "to": {"x": 3000, "y": 4000}})).await;
    assert_eq!(d["mm"], 5000.0);
    let p = project(&app);
    let wall_at_x = |x: f64| {
        p.elements
            .iter()
            .find_map(|e| match e {
                Element::Wall(w) if w.level_id == p.levels[0].id && w.start.x == x && w.end.x == x => Some(w.id.clone()),
                _ => None,
            })
            .unwrap()
    };
    let cw = ok(&app, "measure", json!({"kind": "clear_width", "ids": [wall_at_x(0.0), wall_at_x(6000.0)]})).await;
    assert_eq!(cw["centerline_mm"], 6000.0);
    assert_eq!(cw["clear_mm"], 5850.0);
    let gap = ok(&app, "measure", json!({"kind": "clearance", "ids": [refs["bed_item"], wall_at_x(10000.0)]})).await;
    assert!(gap["gap_mm"].as_f64().is_some());
    assert_eq!(fail(&app, "measure", json!({"kind": "volume"})).await.code, "bad_args");

    // check lists items; since the build's base it lists what the build added.
    let all = ok(&app, "check", json!({})).await;
    assert!(all["note"].as_str().unwrap().contains("never code compliance"));
    let since = ok(&app, "check", json!({"since_revision": 0})).await;
    assert_eq!(since["since"]["revision"], 0);
    assert_eq!(since["since"]["added"].as_array().unwrap().len(), all["count"].as_u64().unwrap() as usize);
    let doors = ok(&app, "check", json!({"codes": ["door_narrow"]})).await;
    assert!(doors["items"].as_array().unwrap().iter().all(|i| i["code"] == "door_narrow"));
}

#[tokio::test]
async fn a_failing_build_entry_names_its_step_and_ref_and_applies_nothing() {
    let (app, _dir) = app();
    blank(&app).await;
    let f = fail(&app, "build", json!({"spec": {
        "rooms": [{"ref": "a", "rect": {"x": 0, "y": 0, "width_mm": 4000, "depth_mm": 3000}}],
        "openings": [{"ref": "far", "kind": "door", "at": {"x": 2000, "y": 1500}}]
    }})).await;
    assert_eq!(f.code, "not_found");
    assert_eq!(f.step, Some(2));
    assert_eq!(f.r#ref.as_deref(), Some("far"));
    assert!(f.text().starts_with("step 2 of 2 (`add_opening`, openings[0], ref `far`)"), "{}", f.text());
    let s = f.structured();
    for key in ["code", "message", "element_ids", "location_mm", "step", "ref", "hint"] {
        assert!(s.get(key).is_some(), "{key} missing from {s}");
    }
    assert!(s["hint"].as_str().unwrap().contains("wall_id"), "{s}");
    assert_eq!(count(&project(&app), ElementKind::Wall), 0, "nothing applied");

    let unknown = fail(&app, "build", json!({"spec": {"doors": []}})).await;
    assert!(unknown.message.contains("unknown list `doors`"));
    let bad_ref = fail(&app, "build", json!({"spec": {"openings": [{"kind": "door", "wall_id": "$nope"}]}})).await;
    assert_eq!(bad_ref.code, "unknown_ref");
}

#[tokio::test]
async fn batch_steps_name_earlier_steps_by_ref() {
    let (app, _dir) = app();
    blank(&app).await;
    let out = ok(&app, "batch", json!({"label": "Box", "steps": [
        {"tool": "add_walls", "ref": "box", "args": {"points": [[0, 0], [5000, 0], [5000, 4000], [0, 4000]], "closed": true}},
        {"tool": "add_opening", "args": {"kind": "window", "wall_id": "$box.2", "offset_mm": 2500}},
        {"tool": "add_room", "ref": "office", "args": {"rect": {"x": 0, "y": 0, "width_mm": 5000, "depth_mm": 4000}, "walls": false, "name": "Office", "usage": "office"}},
        {"tool": "modify", "args": {"ids": ["$office"], "set": {"name": "Study"}}},
        {"tool": "add_wall", "args": {"start": {"x": 0, "y": 6000}, "end": {"x": 3000, "y": 6000}}}
    ]})).await;
    assert_eq!(out["undo_label"], "MCP: Box");
    assert_eq!(out["steps"].as_array().unwrap().len(), 5);
    let p = project(&app);
    let window_wall = p.elements.iter().find_map(|e| match e {
        Element::Opening(o) => Some(o.wall_id.clone()),
        _ => None,
    });
    let third_wall = p.elements.iter().filter(|e| e.kind() == ElementKind::Wall).nth(2).map(|e| e.id().clone());
    assert_eq!(window_wall, third_wall, "$box.2 is the third wall of the chain");
    assert!(p.elements.iter().any(|e| matches!(e, Element::Room(r) if r.name == "Study" && r.usage == RoomUsage::Office)));

    let not_edit = fail(&app, "batch", json!({"steps": [{"tool": "list_rooms", "args": {}}]})).await;
    assert_eq!(not_edit.code, "bad_args");
    let mut dry = fail(&app, "batch", json!({"dry_run": true, "steps": [{"tool": "delete", "args": {"ids": ["nope"]}}]})).await;
    assert_eq!(dry.code, "not_found");
    dry = fail(&app, "batch", json!({"steps": [{"tool": "add_wals", "args": {}}]})).await;
    assert_eq!(dry.hint().unwrap(), "Did you mean `add_walls`?");
}

#[tokio::test]
async fn the_element_tools_and_their_refusals() {
    let (app, _dir) = app();
    blank(&app).await;
    // add_walls with a height; one point is refused.
    let walls = ok(&app, "add_walls", json!({"points": [{"x": 0, "y": 0}, {"x": 4000, "y": 0}], "height_mm": 4500})).await;
    assert_eq!(walls["created"].as_array().unwrap().len(), 1);
    assert!(walls["id"].is_string());
    assert_eq!(fail(&app, "add_walls", json!({"points": [{"x": 0, "y": 0}]})).await.code, "bad_args");

    // add_room: a rect shares the wall above; the result lists issues.
    let room = ok(&app, "add_room", json!({"rect": {"x": 0, "y": 0, "width_mm": 4000, "depth_mm": 3000}, "name": "Bedroom", "usage": "bedroom"})).await;
    assert_eq!(room["created"].as_array().unwrap().iter().filter(|c| c["kind"] == "wall").count(), 3, "the south wall is shared");
    let codes: Vec<&str> = room["issues_added"].as_array().unwrap().iter().filter_map(|i| i["code"].as_str()).collect();
    assert!(codes.contains(&"room_no_door"), "{codes:?}");
    let room_id = room["id"].as_str().unwrap().to_string();
    assert_eq!(fail(&app, "add_room", json!({"name": "Nowhere"})).await.code, "bad_args");

    // add_opening at a point: the door closes room_no_door.
    let door = ok(&app, "add_opening", json!({"kind": "door", "at": {"x": 2000, "y": 3000}, "style": "swing_single", "swing_into": room_id})).await;
    let resolved: Vec<&str> = door["issues_resolved"].as_array().unwrap().iter().filter_map(|i| i["code"].as_str()).collect();
    assert!(resolved.contains(&"room_no_door"), "{resolved:?}");
    assert_eq!(door["details"][0]["offset_mm"], 2000.0);
    let far = fail(&app, "add_opening", json!({"kind": "window", "at": {"x": 2000, "y": 1500}})).await;
    assert_eq!(far.code, "not_found");
    assert!(far.hint().unwrap().contains("wall_id"));

    // add_stair: a to_level that is not the one above is refused.
    ok(&app, "levels", json!({"action": "add", "name": "Mezzanine", "elevation_mm": 2400})).await;
    let stair = fail(&app, "add_stair", json!({"start": [500, 500], "to_level": "Mezzanine"})).await;
    assert_eq!(stair.code, "bad_level");
    assert!(stair.hint().unwrap().contains("levels"));
    let ok_stair = ok(&app, "add_stair", json!({"start": [500, 500], "riser_count": 16, "tread_mm": 260, "railing": {"height_mm": 950, "style": "glass"}})).await;
    assert_eq!(ok_stair["details"][0]["tread_mm"], 260.0);

    // add_column: through two levels; nothing to place is refused.
    let cols = ok(&app, "add_column", json!({"at": {"x": -500, "y": -500}, "to_level": "Mezzanine", "shape": "round"})).await;
    assert_eq!(cols["created"].as_array().unwrap().len(), 2);
    assert_eq!(fail(&app, "add_column", json!({"width_mm": 300})).await.code, "bad_args");

    // add_slab: a deck; two points are refused.
    let deck = ok(&app, "add_slab", json!({"slab_kind": "deck", "rect": {"x": 4000, "y": 0, "width_mm": 2000, "depth_mm": 3000}, "railing": true})).await;
    assert_eq!(deck["details"][0]["outline_area_m2"], 6.0);
    assert_eq!(fail(&app, "add_slab", json!({"polygon": [[0, 0], [1000, 0]]})).await.code, "slab_outline");

    // place_asset: an unknown key answers with the nearest ones.
    ok(&app, "place_asset", json!({"catalog_key": "bed-single", "at": {"x": 1500, "y": 1500}})).await;
    let key = fail(&app, "place_asset", json!({"catalog_key": "double bed", "at": [1000, 1000]})).await;
    assert_eq!(key.code, "unknown_catalog_key");
    assert!(key.message.contains("bed-double"), "{}", key.message);

    // modify: move, turn and set; a field the kind does not take is refused.
    let asset = project(&app).elements.iter().find(|e| e.kind() == ElementKind::Asset).unwrap().id().clone();
    let m = ok(&app, "modify", json!({"ids": [asset], "move": {"dx_mm": 200}, "rotate_deg": 90, "set": {"name": "Guest bed"}})).await;
    assert_eq!(m["changed"].as_array().unwrap().len(), 1);
    let bad = fail(&app, "modify", json!({"ids": [asset], "set": {"usage": "office"}})).await;
    assert!(bad.message.contains("asset takes name"), "{}", bad.message);

    // reshape: resize the room; an unknown op is refused.
    let r = ok(&app, "reshape", json!({"op": "resize_room", "room_id": room_id, "side": "east", "delta_mm": 300})).await;
    assert!(r["also_moved"].is_array() || !r["changed"].as_array().unwrap().is_empty());
    assert_eq!(fail(&app, "reshape", json!({"op": "stretch"})).await.code, "bad_args");
    let wall = walls["id"].as_str().unwrap();
    let split = ok(&app, "reshape", json!({"op": "split_wall", "wall_id": wall, "at": {"x": 1000, "y": 0}})).await;
    assert!(split["id"].is_string(), "the new wall's id");

    // delete.
    let gone = ok(&app, "delete", json!({"ids": [asset]})).await;
    assert_eq!(gone["deleted"].as_array().unwrap().len(), 1);
    assert_eq!(fail(&app, "delete", json!({"ids": ["nope"]})).await.code, "not_found");
}

#[tokio::test]
async fn levels_project_settings_history_and_export() {
    let (app, dir) = app();
    blank(&app).await;
    // The ground floor's height can change now; an overlap is warned about.
    ok(&app, "levels", json!({"action": "add", "name": "Upper"})).await;
    let up = ok(&app, "levels", json!({"action": "update", "level": "ground floor", "height_mm": 3600})).await;
    assert!(up["warnings"][0].as_str().unwrap().contains("past the floor of Upper"), "{up}");
    let list = ok(&app, "levels", json!({"action": "list"})).await;
    assert_eq!(list["levels"][0]["height_mm"], 3600.0);
    assert!(list["warnings"].is_array());
    ok(&app, "levels", json!({"action": "update", "level": "Upper", "elevation_mm": 3600})).await;
    assert!(ok(&app, "levels", json!({"action": "list"})).await.get("warnings").is_none());
    assert_eq!(fail(&app, "levels", json!({"action": "rename"})).await.code, "bad_args");

    // set_project: site, north, settings and roof in one step.
    let set = ok(&app, "set_project", json!({
        "site": {"city": "newport", "latitude_deg": 33.6, "longitude_deg": -117.9, "utc_offset_min": -480},
        "north_angle_deg": 12,
        "settings": {"client_name": "Lovell", "designer": "Schindler"},
        "roof": {"kind": "flat"}
    })).await;
    assert_eq!(set["undo_label"], "MCP: set project");
    let p = project(&app);
    assert_eq!(p.settings.client_name, "Lovell");
    assert_eq!(p.settings.site.as_ref().unwrap().latitude_deg, 33.6);
    assert_eq!(p.roof.kind, RoofKind::Flat);
    assert_eq!(fail(&app, "set_project", json!({})).await.code, "bad_args");

    // history: list, undo, redo, undo_to.
    ok(&app, "add_room", json!({"rect": {"x": 0, "y": 0, "width_mm": 3000, "depth_mm": 3000}})).await;
    let h = ok(&app, "history", json!({"action": "list"})).await;
    assert_eq!(h["undo"][0]["label"], "MCP: add room");
    assert_eq!(h["undo_count"], 5);
    ok(&app, "history", json!({"action": "undo"})).await;
    let redone = ok(&app, "history", json!({"action": "redo"})).await;
    assert_eq!(redone["what"], "redone");
    let back = ok(&app, "history", json!({"action": "undo_to", "index": 1})).await;
    assert_eq!(back["next_redo"], "MCP: set project");

    // export: every level as PDF, the model as IFC, the bundle; GLB is the window's.
    ok(&app, "history", json!({"action": "redo", "steps": 2})).await;
    let pdf = ok(&app, "export", json!({"format": "pdf", "level": "all"})).await;
    let files = pdf["files"].as_array().unwrap();
    assert_eq!(files.len(), 1, "one sheet per level with something on it");
    assert_eq!(pdf["skipped"][0]["level"], "Upper");
    for f in files {
        assert!(std::path::Path::new(f["path"].as_str().unwrap()).starts_with(dir.path()));
    }
    let ifc = ok(&app, "export", json!({"format": "ifc"})).await;
    assert!(ifc["files"][0]["path"].as_str().unwrap().ends_with(".ifc"));
    let bundle = ok(&app, "export", json!({"format": "guhit"})).await;
    assert!(bundle["files"][0]["path"].as_str().is_some());
    assert_eq!(fail(&app, "export", json!({"format": "glb"})).await.code, "unavailable");
}

#[tokio::test]
async fn query_reads_compact_rows_and_pages() {
    let (app, _dir) = app();
    // No project: a clear refusal with a hint.
    let none = fail(&app, "query", json!({"what": "summary"})).await;
    assert_eq!(none.code, "no_document");
    assert!(none.hint().unwrap().contains("project"));
    blank(&app).await;
    ok(&app, "build", house_spec()).await;

    let summary = ok(&app, "query", json!({"what": "summary"})).await;
    assert_eq!(summary["revision"], 1);
    let rooms = ok(&app, "query", json!({"what": "rooms", "level": "Upper floor"})).await;
    assert_eq!(rooms["count"], 2);
    let row = &rooms["items"][0];
    assert!(row.get("net_area_m2").is_some() && row.get("wall_ids").is_some());
    assert!(row.get("bounding_walls").is_none(), "compact rows leave the long lists out");

    let walls = ok(&app, "query", json!({"what": "elements", "kind": "wall", "level": "Ground Floor", "page_size": 3})).await;
    assert_eq!(walls["items"].as_array().unwrap().len(), 3);
    assert_eq!(walls["next_page"], 2);
    let doors = ok(&app, "query", json!({"what": "elements", "kind": "door"})).await;
    assert_eq!(doors["count"], 3);
    let fields = ok(&app, "query", json!({"what": "elements", "kind": "slab", "fields": ["slab_kind"]})).await;
    assert_eq!(fields["items"][0].as_object().unwrap().len(), 3, "id, kind and the field asked for");
    let id = doors["items"][0]["id"].clone();
    let full = ok(&app, "query", json!({"what": "describe", "ids": [id]})).await;
    assert!(full["items"][0].get("host_wall_length_mm").is_some());

    let catalog = ok(&app, "query", json!({"what": "catalog", "search": "bed"})).await;
    assert!(catalog["count"].as_u64().unwrap() >= 2);
    let none = ok(&app, "query", json!({"what": "catalog", "search": "fireplace"})).await;
    assert_eq!(none["count"], 0);
    assert!(none["did_you_mean"].is_array());
    let mats = ok(&app, "query", json!({"what": "materials"})).await;
    assert!(mats["materials"].as_array().unwrap().iter().any(|m| m["id"] == "mat-chb-painted"));
    assert_eq!(fail(&app, "query", json!({"what": "describe"})).await.code, "bad_args");
    assert_eq!(fail(&app, "query", json!({"what": "elements", "kind": "chimney"})).await.code, "bad_args");
    let sel = ok(&app, "query", json!({"what": "selection"})).await;
    assert_eq!(sel["count"], 0);
}

#[tokio::test]
async fn project_and_guides() {
    let (app, _dir) = app();
    let created = ok(&app, "project", json!({"action": "create", "name": "Casa"})).await;
    let id = created["project_id"].clone();
    let list = ok(&app, "project", json!({"action": "list"})).await;
    assert!(list.as_array().unwrap().iter().any(|p| p["id"] == id));
    ok(&app, "project", json!({"action": "save_version", "label": "Start"})).await;
    ok(&app, "project", json!({"action": "close"})).await;
    ok(&app, "project", json!({"action": "open", "id": id})).await;
    assert_eq!(fail(&app, "project", json!({"action": "open"})).await.code, "bad_args");

    let guide = ok(&app, "get_guide", json!({"topic": "workflows"})).await;
    assert!(guide["text"].as_str().unwrap().contains("The verify loop"));
    let catalog = ok(&app, "get_guide", json!({"topic": "catalog"})).await;
    assert!(catalog["text"].as_str().unwrap().contains("`bed-double`"));
    assert_eq!(fail(&app, "get_guide", json!({"topic": "everything"})).await.code, "bad_args");
}

#[tokio::test]
async fn v1_names_still_answer_and_unknown_names_get_the_nearest() {
    let (app, _dir) = app();
    ok(&app, "create_project", json!({"name": "Old", "template": "blank"})).await;
    let room = ok(&app, "add_rect_room", json!({"origin": {"x": 0, "y": 0}, "width_mm": 4000, "depth_mm": 3000, "name": "Sala"})).await;
    assert_eq!(room["undo_label"], "MCP: add rect room");
    assert!(room["issues_added"].is_array(), "v1 tools get the v2 result fields too");
    let rooms = ok(&app, "list_rooms", json!({})).await;
    assert_eq!(rooms["rooms"][0]["name"], "Sala");
    ok(&app, "undo", json!({})).await;

    let unknown = fail(&app, "add_stairs", json!({})).await;
    assert_eq!(unknown.code, "unknown_tool");
    assert!(unknown.message.contains("Did you mean `add_stair`?"), "{}", unknown.message);
    let listed: Vec<String> = guhit_mcp::tool_list_for(&guhit_mcp::toolsets::Toolsets::all()).iter().map(|t| t.name.to_string()).collect();
    for (old, new) in guhit_mcp::toolsets::ALIASES {
        if old != new {
            assert!(!listed.iter().any(|n| n == old), "`{old}` is listed");
        }
    }
}

#[tokio::test]
async fn services_tools_draw_and_link() {
    let (app, _dir) = app();
    ok(&app, "project", json!({"action": "create", "name": "Services", "template": "sample-bungalow"})).await;
    let pipe = ok(&app, "add_pipe", json!({"system": "cold_water", "points": [{"x": 500, "y": 500, "z": 300}, {"x": 2500, "y": 500, "z": 300}]})).await;
    let pipe_id = pipe["id"].as_str().unwrap().to_string();
    let moved = ok(&app, "edit_pipe", json!({"op": "move_points", "pipes": [{"pipe_id": pipe_id, "indices": []}], "dz_mm": -100})).await;
    assert_eq!(moved["changed"][0]["id"], json!(pipe_id));
    let bend = ok(&app, "edit_pipe", json!({"op": "insert_point", "pipe_id": pipe_id, "segment": 0, "at": {"x": 1500, "y": 500, "z": 200}})).await;
    assert_eq!(bend["changed"].as_array().unwrap().len(), 1);
    assert_eq!(fail(&app, "edit_pipe", json!({"op": "bend"})).await.code, "bad_args");
    let switch = ok(&app, "place_asset", json!({"catalog_key": "switch-1", "at": {"x": 300, "y": 1000}})).await;
    let light = ok(&app, "place_asset", json!({"catalog_key": "light-ceiling", "at": {"x": 1500, "y": 1500}})).await;
    let linked = ok(&app, "link_devices", json!({"from": switch["id"], "to": [light["id"]]})).await;
    assert_eq!(linked["changed"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn agents_are_named_and_paused_ones_cannot_edit() {
    let (app, _dir) = app();
    blank(&app).await;
    let hint = AgentHint { client_name: Some("claude-code".into()), client_version: Some("2.0".into()), ..AgentHint::default() };
    let ctx = CallContext::agent(hint);
    let as_agent = |name: &'static str, args: Value| {
        let (app, ctx) = (app.clone(), ctx.clone());
        async move { tools::call_with(&app, name, args, &ctx).await }
    };
    let Ok(Output::Json(room)) = as_agent("add_room", json!({"rect": {"x": 0, "y": 0, "width_mm": 4000, "depth_mm": 3000}, "agent": "kitchen"})).await else {
        panic!("the agent's edit failed");
    };
    assert_eq!(room["agent"], "Kitchen");
    let agents = app.agents_list();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].participant.name, "Kitchen");
    // The step is the agent's.
    let history: HistoryList = serde_json::from_value(app.handle("history_list", json!({})).await.unwrap()).unwrap();
    assert_eq!(history.undo[0].author_id.as_deref(), Some(agents[0].participant.id.as_str()));
    // A status line and a claim.
    let Ok(Output::Json(status)) = as_agent("set_status", json!({"text": "Laying out the kitchen", "agent": "kitchen", "claim": {"levels": ["Ground Floor"]}})).await else {
        panic!("set_status failed");
    };
    assert!(status["claim"].is_object(), "{status}");
    // Each call sets the status line, unless the agent wrote its own.
    let status_of = || app.agents_list()[0].presence.status.clone();
    as_agent("add_walls", json!({"points": [[0, 4000], [2000, 4000]], "agent": "kitchen"})).await.ok();
    assert_eq!(status_of().as_deref(), Some("Laying out the kitchen"), "its own line wins for a minute");
    let other = CallContext::agent(AgentHint { label: Some("roof".into()), ..AgentHint::default() });
    tools::call_with(&app, "check", json!({}), &other).await.ok();
    let roof = app.agents_list().into_iter().find(|a| a.participant.name == "Roof").unwrap();
    assert_eq!(roof.presence.status.as_deref(), Some("Checking the model"));
    tools::call_with(&app, "add_opening", json!({"kind": "window", "at": {"x": 2000, "y": 0}}), &other).await.ok();
    let roof = app.agents_list().into_iter().find(|a| a.participant.name == "Roof").unwrap();
    assert_eq!(roof.presence.status.as_deref(), Some("Placing windows"));
    // Paused: reads work, edits are refused.
    app.handle("agent_pause", json!({"id": agents[0].participant.id})).await.unwrap();
    assert!(as_agent("query", json!({"what": "summary", "agent": "kitchen"})).await.is_ok());
    let Err(paused) = as_agent("add_walls", json!({"points": [[0, 5000], [3000, 5000]], "agent": "kitchen"})).await else {
        panic!("a paused agent edited");
    };
    assert_eq!(paused.code, "agent_paused");
    assert!(paused.hint().unwrap().contains("Agents panel"));
    // Its own step it may undo once resumed.
    app.handle("agent_resume", json!({"id": agents[0].participant.id})).await.unwrap();
    assert!(as_agent("history", json!({"action": "undo", "agent": "kitchen"})).await.is_ok());
}

// --------------------------------------------------------------- over HTTP

async fn serve(app: AppService) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, guhit_mcp::router(app)).await });
    port
}

async fn rpc(url: &str, body: Value) -> Value {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let res = client
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(body.to_string())
        .send()
        .await
        .expect("request");
    serde_json::from_str(&res.text().await.unwrap()).expect("a JSON answer")
}

#[tokio::test]
async fn toolsets_errors_and_agents_over_http() {
    let (app, _dir) = app();
    let port = serve(app.clone()).await;
    let base = format!("http://127.0.0.1:{port}/mcp");
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "codex", "version": "1"}}});
    let info = rpc(&base, init).await;
    assert!(info["result"]["capabilities"]["prompts"].is_object());
    let list = |url: String| async move { rpc(&url, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await["result"]["tools"].as_array().unwrap().len() };
    assert_eq!(list(base.clone()).await, 22);
    assert_eq!(list(format!("{base}?toolsets=visuals,live")).await, 27);
    assert_eq!(list(format!("{base}?toolsets=all")).await, 33);
    // The setting is the default when the URL names none.
    app.handle("mcp_set_toolsets", json!({"toolsets": ["services"]})).await.unwrap();
    assert_eq!(list(base.clone()).await, 25);

    // A labelled URL names the agent; a failure is isError with structure.
    let call = |url: String, name: &str, args: Value| {
        let body = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": name, "arguments": args}});
        async move { rpc(&url, body).await["result"].clone() }
    };
    call(format!("{base}/agent/upstairs"), "project", json!({"action": "create", "name": "Http"})).await;
    let made = call(format!("{base}/agent/upstairs"), "add_room", json!({"rect": {"x": 0, "y": 0, "width_mm": 3000, "depth_mm": 3000}})).await;
    assert_eq!(made["isError"], false, "{made}");
    assert_eq!(made["structuredContent"]["agent"], "Upstairs");
    let bad = call(base.clone(), "add_opening", json!({"kind": "door", "at": {"x": 1500, "y": 1500}})).await;
    assert_eq!(bad["isError"], true);
    assert_eq!(bad["structuredContent"]["code"], "not_found");
    assert!(bad["content"][0]["text"].as_str().unwrap().contains("Hint:"));
    let unknown = call(base.clone(), "draw_house", json!({})).await;
    assert_eq!(unknown["structuredContent"]["code"], "unknown_tool");

    // Two labelled connections keep their own clients, though neither
    // names it again after initialize.
    for (label, client) in [("ground", "claude-code"), ("upper", "codex")] {
        let init = json!({"jsonrpc": "2.0", "id": 9, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": client, "version": "1"}}});
        rpc(&format!("{base}/agent/{label}"), init).await;
    }
    for label in ["ground", "upper"] {
        call(format!("{base}/agent/{label}"), "query", json!({"what": "summary"})).await;
    }
    let client_of = |name: &str| {
        app.agents_list()
            .into_iter()
            .find(|a| a.participant.name == name)
            .and_then(|a| a.participant.agent)
            .map(|a| a.client)
            .unwrap_or_default()
    };
    assert_eq!(client_of("Ground"), "claude-code");
    assert_eq!(client_of("Upper"), "codex");
    assert_eq!(app.agents_list().iter().find(|a| a.participant.name == "Ground").unwrap().presence.status.as_deref(), Some("Reading the plan"));

    // Prompts, resources and the plan template.
    let prompts = rpc(&base, json!({"jsonrpc": "2.0", "id": 4, "method": "prompts/list"})).await;
    assert_eq!(prompts["result"]["prompts"].as_array().unwrap().len(), 4);
    let templates = rpc(&base, json!({"jsonrpc": "2.0", "id": 5, "method": "resources/templates/list"})).await;
    assert_eq!(templates["result"]["resourceTemplates"][0]["uriTemplate"], "guhit://project/levels/{level}/plan.svg");
    let svg = rpc(&base, json!({"jsonrpc": "2.0", "id": 6, "method": "resources/read", "params": {"uri": "guhit://project/levels/Ground%20Floor/plan.svg"}})).await;
    assert!(svg["result"]["contents"][0]["text"].as_str().unwrap().starts_with("<svg"), "{svg}");
    let review = rpc(&base, json!({"jsonrpc": "2.0", "id": 7, "method": "resources/read", "params": {"uri": "guhit://review"}})).await;
    assert!(review["result"]["contents"][0]["text"].as_str().unwrap().contains("never code compliance"));
}
