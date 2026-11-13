//! Lane L1, the permit engine (DECISIONS D53, D54): door and window types,
//! grids, dimension strings and automatic dimensions, schedules, the lot,
//! the sheet set and revisions, through the app's IPC.

use guhit_app::AppService;
use guhit_model::*;
use serde_json::{json, Value};

fn app() -> (AppService, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    (AppService::new_sandboxed(dir.path().to_path_buf()), dir)
}

async fn call(app: &AppService, cmd: &str, args: Value) -> Value {
    app.handle(cmd, args).await.unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

async fn fail(app: &AppService, cmd: &str, args: Value) -> IpcError {
    match app.handle(cmd, args).await {
        Ok(v) => panic!("{cmd} should have failed, got {v}"),
        Err(e) => e,
    }
}

async fn bungalow(app: &AppService) -> DocState {
    serde_json::from_value(call(app, "hub_create", json!({ "name": "Bahay", "template": "sample-bungalow" })).await).unwrap()
}

async fn apply(app: &AppService, command: Command) -> ApplyResult {
    serde_json::from_value(call(app, "doc_apply", json!({ "command": command })).await).unwrap()
}

#[tokio::test]
async fn the_permit_set_builds_through_ipc() {
    let (app, _dir) = app();
    let state = bungalow(&app).await;
    let level = state.project.levels[0].id.clone();

    let r = apply(&app, Command::AssignOpeningTypes { level_id: None }).await;
    assert_eq!(r.diff.summary, "Typed 5 openings: new types D1, D2, W1, W2, W3");
    let marks: Vec<String> = r.state.derived.opening_schedule.rows.iter().map(|r| r.mark.clone()).collect();
    assert_eq!(marks, vec!["D1", "D2", "W1", "W2", "W3"]);

    let r = apply(&app, Command::AutoDimension { level_id: level.clone(), kinds: vec![AutoDimKind::Exterior] }).await;
    assert_eq!(r.diff.summary, "Added 9 automatic dimensions");

    let legs = ["S. 44 deg. 30' E.", "S. 45 deg. 30' W.", "N. 44 deg. 30' W.", "N. 45 deg. 30' E."]
        .iter()
        .zip([12000.0, 15000.0, 12000.0, 15000.0])
        .map(|(b, d)| SurveyLeg { bearing: b.to_string(), distance_mm: d })
        .collect();
    let r = apply(&app, Command::SetLotFromSurvey { start: Point { x: -3000.0, y: 9000.0 }, legs }).await;
    assert!(r.diff.summary.starts_with("Set the lot from 4 legs: 180.00 m2, it closes exactly"), "{}", r.diff.summary);

    let r = apply(&app, Command::MakeSheetSet { paper: PaperSize::A1, replace: false }).await;
    assert_eq!(r.state.project.sheet_set.sheets.len(), 7);

    // The queries hand out what Derived holds.
    let q = call(&app, "doc_query", json!({ "query": Query::OpeningSchedule })).await;
    assert_eq!(q["rows"].as_array().unwrap().len(), 5);
    let q = call(&app, "doc_query", json!({ "query": Query::LevelAreas })).await;
    assert_eq!(q["levels"][0]["level_id"], json!(level));
    let q = call(&app, "doc_query", json!({ "query": Query::Site })).await;
    assert_eq!(q["site"]["area_m2"].as_f64().map(|a| (a * 100.0).round() / 100.0), Some(180.0));
    let q = call(&app, "doc_query", json!({ "query": Query::RoomSchedule })).await;
    assert_eq!(q["rooms"].as_array().unwrap().len(), 2);

    // A bad bearing comes back as a plain refusal with a hint.
    let legs = vec![
        SurveyLeg { bearing: "X 45 E".into(), distance_mm: 1000.0 },
        SurveyLeg { bearing: "N 45 E".into(), distance_mm: 1000.0 },
        SurveyLeg { bearing: "N 45 W".into(), distance_mm: 1000.0 },
    ];
    let e = fail(&app, "doc_apply", json!({ "command": Command::SetLotFromSurvey { start: Point { x: 0.0, y: 0.0 }, legs } })).await;
    assert_eq!(e.code, "invalid", "every refusal reads invalid at IPC; the message carries the hint");
    assert!(e.message.contains("Leg 1"), "{}", e.message);
}

#[tokio::test]
async fn sheets_and_revisions_work_through_ipc() {
    let (app, _dir) = app();
    bungalow(&app).await;
    let sheet = Sheet {
        id: String::new(),
        number: String::new(),
        title: "Ground floor plan".into(),
        paper: PaperSize::A1,
        orientation: Orientation::Landscape,
        viewports: vec![],
        revision_ids: vec![],
    };
    let r = apply(&app, Command::UpsertSheet { sheet: Box::new(sheet), index: None }).await;
    assert_eq!(r.state.project.sheet_set.sheets[0].number, "A-1");
    let id = r.state.project.sheet_set.sheets[0].id.clone();
    let rev = SheetRevision { id: String::new(), mark: "1".into(), date: "2026-11-04".into(), description: "For permit".into() };
    let r = apply(&app, Command::UpsertRevision { revision: rev, sheet_ids: vec![id] }).await;
    assert_eq!(r.state.project.sheet_set.sheets[0].revision_ids.len(), 1);
}
