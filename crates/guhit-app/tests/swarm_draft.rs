//! The swarm draft's engine side (DECISIONS D57, lane L1): accepting one
//! part with what it depends on, pinned to what the person reviewed;
//! `cross_part`, `changed_on_main` and `lost_id`; the cascading discard; an
//! agent's undo of its own draft steps; the read gate; and accepts while
//! agents keep drawing.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use guhit_app::agents::{AgentCall, AgentHint, CallKind};
use guhit_app::swarm::check::WindowOutput;
use guhit_app::AppService;
use guhit_core::Staging;
use guhit_model::*;
use serde_json::json;

// ================================================================ helpers

fn hint(connection: &str, agent: Option<&str>) -> AgentHint {
    AgentHint {
        connection: Some(connection.to_string()),
        agent: agent.map(str::to_string),
        client_name: Some("claude-code".into()),
        client_version: Some("2.1".into()),
        ..AgentHint::default()
    }
}

async fn app() -> (AppService, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let app = AppService::new_sandboxed(dir.path().to_path_buf());
    app.handle("hub_create", json!({"name": "Swarm draft", "template": "blank"})).await.unwrap();
    (app, dir)
}

async fn main_doc(app: &AppService) -> (Project, u32) {
    let s = app.session.lock().await;
    let doc = s.doc.as_ref().unwrap();
    (doc.project().clone(), doc.revision())
}

fn pt(x: f64, y: f64) -> Point {
    Point { x, y }
}

fn seg(a: (f64, f64), b: (f64, f64), level_id: Option<&Id>) -> Command {
    Command::AddWall {
        start: pt(a.0, a.1),
        end: pt(b.0, b.1),
        thickness_mm: None,
        height_mm: None,
        material_id: None,
        level_id: level_id.cloned(),
        role: None,
    }
}

fn rect(x: f64, y: f64, w: f64, d: f64, level_id: Option<&Id>) -> Command {
    Command::AddWallChain {
        points: vec![pt(x, y), pt(x + w, y), pt(x + w, y + d), pt(x, y + d)],
        closed: true,
        thickness_mm: None,
        level_id: level_id.cloned(),
        role: None,
    }
}

fn door(wall_id: &Id, offset_mm: f64) -> Command {
    Command::AddOpening {
        wall_id: wall_id.clone(),
        opening_type: OpeningType::Door,
        offset_mm,
        width_mm: None,
        height_mm: None,
        sill_mm: None,
        style: None,
        flip_side: None,
        flip_hinge: None,
        type_id: None,
    }
}

fn part(key: &str, name: &str, levels: Vec<String>, kinds: Vec<ElementKind>) -> SwarmPartPlan {
    SwarmPartPlan {
        key: key.into(),
        name: name.into(),
        role: String::new(),
        instructions: String::new(),
        scope: SwarmScope { levels, kinds, area: None },
        after: vec![],
        milestones: vec![],
    }
}

/// Shell draws the ground floor's walls; Rooms its rooms and openings,
/// after Shell's walls; Upper everything on the Second Floor, which setup
/// adds.
fn house_plan(ground: &str) -> SwarmPlan {
    let mut shell = part("shell", "Shell", vec![ground.into()], vec![ElementKind::Wall, ElementKind::Column]);
    shell.milestones = vec!["walls".into()];
    let mut rooms = part("rooms", "Rooms", vec![ground.into()], vec![ElementKind::Room, ElementKind::Opening]);
    rooms.after = vec![SwarmWait { part: "shell".into(), milestone: "walls".into() }];
    let upper = part("upper", "Upper", vec!["Second Floor".into()], vec![]);
    SwarmPlan {
        summary: "A two storey house".into(),
        setup: SwarmSetup { levels: vec![SwarmLevel { name: "Second Floor".into(), height_mm: None }] },
        parts: vec![shell, rooms, upper],
    }
}

struct House {
    id: Id,
    lead: AgentCall,
    shell: AgentCall,
    rooms: AgentCall,
    upper: AgentCall,
    second: Id,
}

async fn start(app: &AppService) -> House {
    let ground = main_doc(app).await.0.levels[0].id.clone();
    let lead = app.agent_call(&hint("c-lead", None), "swarm", CallKind::Edit).unwrap();
    let started = app.swarm_start_mcp(&lead, "A house", house_plan(&ground)).await.unwrap();
    let handle = |key: &str| started.handles.iter().find(|h| h.key == key).unwrap().handle.clone();
    // Each part on its own connection, so each has its own rate.
    let shell = app.agent_call(&hint("c-shell", Some(&handle("shell"))), "build", CallKind::Edit).unwrap();
    let rooms = app.agent_call(&hint("c-rooms", Some(&handle("rooms"))), "build", CallKind::Edit).unwrap();
    let upper = app.agent_call(&hint("c-upper", Some(&handle("upper"))), "build", CallKind::Edit).unwrap();
    let view = app.swarm_draft(None).await.unwrap().unwrap().state.project;
    let second = view.levels.iter().find(|l| l.name == "Second Floor").unwrap().id.clone();
    House { id: started.swarm.id, lead, shell, rooms, upper, second }
}

/// One agent step, with the registry's clock moved on so the rate limit
/// (5 a second) never trips.
async fn step(app: &AppService, call: &AgentCall, command: Command) -> Result<ApplyResult, IpcError> {
    app.agents.advance_clock(Duration::from_millis(300));
    app.agent_commit(call, command).await
}

fn added_walls(r: &ApplyResult) -> Vec<Id> {
    r.diff
        .added
        .iter()
        .filter(|id| r.state.project.elements.iter().any(|e| matches!(e, Element::Wall(w) if &w.id == *id)))
        .cloned()
        .collect()
}

fn reviewed(app: &AppService) -> BTreeMap<String, u32> {
    app.swarm_status().unwrap().parts.iter().map(|p| (p.key.clone(), p.last_seq)).collect()
}

fn keys(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn part_state(app: &AppService, key: &str) -> SwarmPart {
    app.swarm_status().unwrap().parts.into_iter().find(|p| p.key == key).unwrap()
}

async fn view(app: &AppService) -> Project {
    app.swarm_draft(None).await.unwrap().unwrap().state.project
}

fn by_id(p: &Project) -> BTreeMap<Id, Element> {
    p.elements.iter().map(|e| (e.id().clone(), e.clone())).collect()
}

async fn history(app: &AppService) -> HistoryList {
    serde_json::from_value(app.handle("history_list", json!({})).await.unwrap()).unwrap()
}

// ================================================================== tests

#[tokio::test]
async fn accepting_one_part_brings_in_what_it_builds_on_and_leaves_the_rest_drawing() {
    let (app, _dir) = app().await;
    let (before, revision) = main_doc(&app).await;
    let h = start(&app).await;
    let walls = added_walls(&step(&app, &h.shell, rect(0.0, 0.0, 8000.0, 6000.0, None)).await.unwrap());
    let upper = step(&app, &h.upper, rect(0.0, 0.0, 8000.0, 6000.0, Some(&h.second))).await.unwrap();
    let door = step(&app, &h.rooms, door(&walls[0], 2000.0)).await.unwrap().diff.added[0].clone();
    let shell_more = step(&app, &h.shell, seg((8000.0, 0.0), (12_000.0, 0.0), None)).await.unwrap().diff.added[0].clone();
    let drawn = view(&app).await;

    // The check says what an accept of Rooms takes, and what would take it.
    let check = app.swarm_check(Some(&keys(&["rooms"]))).await.unwrap();
    assert_eq!(check.applied.iter().map(|a| a.part.as_str()).collect::<Vec<_>>(), ["shell", "rooms"], "Shell first, it builds on nothing");
    assert!(check.skipped.is_empty() && check.state.is_none(), "{check:?}");
    assert!(check.differs.is_empty(), "nothing comes out differently: {:?}", check.differs);
    let check = app.swarm_check(Some(&keys(&["shell"]))).await.unwrap();
    assert_eq!(check.dependents, ["rooms"], "discarding Shell would take Rooms");
    assert_eq!(main_doc(&app).await.1, revision, "a check changes nothing");

    let done = app.swarm_accept(&h.id, Some(&keys(&["rooms"])), Some(&reviewed(&app))).await.unwrap();
    assert_eq!(done.applied.iter().map(|a| a.part.as_str()).collect::<Vec<_>>(), ["shell", "rooms"]);
    let (main, now) = main_doc(&app).await;
    assert_eq!(now, revision + 2, "one revision per part");
    let main_els = by_id(&main);
    let draft_els = by_id(&drawn);
    for id in walls.iter().chain([&door, &shell_more]) {
        assert_eq!(main_els.get(id), draft_els.get(id), "{id} is in the model with the draft's id and shape");
    }
    assert!(upper.diff.added.iter().all(|id| !main_els.contains_key(id)), "Upper stays in the draft");
    assert!(main.levels.len() == before.levels.len(), "setup is not pulled in: nothing of Shell or Rooms needs it");

    let list = history(&app).await;
    let top: Vec<(&str, Option<&str>)> = list.undo.iter().take(2).map(|s| (s.label.as_str(), s.author_id.as_deref())).collect();
    assert_eq!(top, [("Swarm: Rooms", Some(h.rooms.id.as_str())), ("Swarm: Shell", Some(h.shell.id.as_str()))]);
    assert!(list.undo.iter().take(2).all(|s| s.origin == Origin::Ai));

    // The rest of the draft is drawn on the new model and keeps going.
    let status = app.swarm_status().unwrap();
    assert_eq!(status.main_revision, now);
    assert_eq!((part_state(&app, "shell").state, part_state(&app, "rooms").state), (SwarmPartState::Accepted, SwarmPartState::Accepted));
    assert_eq!(part_state(&app, "shell").steps, 0);
    assert!(status.conflicts.is_empty(), "{:?}", status.conflicts);
    let rest = view(&app).await;
    assert_eq!(by_id(&rest), draft_els, "the view still shows every element as drawn");
    assert_eq!(status.added as usize, rest.elements.len() - main.elements.len());
    let e = step(&app, &h.shell, seg((0.0, 9000.0), (3000.0, 9000.0), None)).await.unwrap_err();
    assert_eq!(e.code, "part_stopped", "an accepted part draws no more");
    step(&app, &h.upper, seg((0.0, 9000.0), (3000.0, 9000.0), Some(&h.second))).await.unwrap();
    assert_eq!(app.handle("doc_undo", json!({})).await.unwrap_err().code, "swarm_running", "the model stays locked");

    // Upper names the setup level, so the lead's setup comes with it, and
    // the swarm ends once nothing is left.
    let check = app.swarm_check(Some(&keys(&["upper"]))).await.unwrap();
    assert_eq!(check.applied.iter().map(|a| a.part.as_str()).collect::<Vec<_>>(), ["lead", "upper"]);
    let done = app.swarm_accept(&h.id, Some(&keys(&["upper"])), Some(&reviewed(&app))).await.unwrap();
    assert_eq!(done.applied.len(), 2);
    assert!(app.swarm_status().is_none(), "nothing left: the swarm ended");
    let main = main_doc(&app).await.0;
    assert_eq!(main.levels.len(), before.levels.len() + 1);
    assert!(upper.diff.added.iter().all(|id| main.elements.iter().any(|e| e.id() == id)), "Upper's ids are the draft's");
    let versions: Vec<SnapshotMeta> = serde_json::from_value(app.handle("snapshot_list", json!({})).await.unwrap()).unwrap();
    assert_eq!(versions.iter().filter(|v| v.label.starts_with("Before swarm")).count(), 1, "one version, before the first accept");
}

#[tokio::test]
async fn an_accept_is_pinned_to_the_parts_it_takes() {
    let (app, _dir) = app().await;
    let h = start(&app).await;
    let walls = added_walls(&step(&app, &h.shell, rect(0.0, 0.0, 8000.0, 6000.0, None)).await.unwrap());
    step(&app, &h.rooms, door(&walls[0], 2000.0)).await.unwrap();
    let seen = reviewed(&app);
    // A part outside the accept draws on: the pin does not care.
    step(&app, &h.upper, rect(0.0, 0.0, 5000.0, 5000.0, Some(&h.second))).await.unwrap();
    // A part inside it draws on: refused, nothing applied.
    step(&app, &h.shell, seg((8000.0, 0.0), (11_000.0, 0.0), None)).await.unwrap();
    let revision = main_doc(&app).await.1;
    let e = app.swarm_accept(&h.id, Some(&keys(&["rooms"])), Some(&seen)).await.unwrap_err();
    assert_eq!(e.code, "stale", "{e:?}");
    assert!(e.message.contains("Shell"), "{}", e.message);
    assert_eq!(main_doc(&app).await.1, revision);
    // A part that took a step back is a change too.
    app.agent_undo(&h.shell, false, false).await.unwrap();
    let mut pins = reviewed(&app);
    pins.insert("shell".into(), seen["shell"] + 100);
    assert_eq!(app.swarm_accept(&h.id, Some(&keys(&["rooms"])), Some(&pins)).await.unwrap_err().code, "stale");
    let mut pins = reviewed(&app);
    pins.remove("upper");
    app.swarm_accept(&h.id, Some(&keys(&["rooms"])), Some(&pins)).await.unwrap();
    // Accept all is pinned to every part.
    let e = app.swarm_accept(&h.id, None, Some(&BTreeMap::new())).await.unwrap_err();
    assert_eq!(e.code, "stale", "Upper drew since: {e:?}");
}

#[tokio::test]
async fn a_step_that_would_tie_two_parts_together_is_refused_cross_part() {
    let (app, _dir) = app().await;
    let h = start(&app).await;
    let wall = step(&app, &h.shell, seg((0.0, 0.0), (6000.0, 0.0), None)).await.unwrap().diff.added[0].clone();
    step(&app, &h.rooms, door(&wall, 2000.0)).await.unwrap();
    // Deleting the wall would take Rooms' door: Shell would depend on Rooms,
    // which depends on Shell.
    let delete = Command::DeleteElements { ids: vec![wall.clone()] };
    let view = app.swarm_view(&h.shell.id).unwrap();
    let staged = Staging::rebuilt(view.project(), "dry", std::slice::from_ref(&delete)).unwrap();
    let e = app.agent_check(&h.shell, view.derived(), &staged).unwrap_err();
    assert_eq!(e.code, "cross_part", "the dry run says so: {e:?}");
    let e = step(&app, &h.shell, delete).await.unwrap_err();
    assert_eq!(e.code, "cross_part", "{e:?}");
    assert!(e.message.contains("Rooms") && e.message.contains("Shell"), "{}", e.message);
    assert_eq!(part_state(&app, "shell").steps, 1, "a refused step leaves no trace");
    // Shell may still change what only it drew and Rooms only looked at.
    step(&app, &h.shell, Command::SetWallEndpoints { wall_id: wall.clone(), start: pt(0.0, 0.0), end: pt(7000.0, 0.0) }).await.unwrap();
}

#[tokio::test]
async fn steps_on_what_an_accept_changed_drop_out_as_conflicts() {
    let (app, _dir) = app().await;
    let h = start(&app).await;
    let wall = step(&app, &h.shell, seg((0.0, 0.0), (6000.0, 0.0), None)).await.unwrap().diff.added[0].clone();
    let door_id = step(&app, &h.rooms, door(&wall, 2000.0)).await.unwrap().diff.added[0].clone();
    // Shell lengthens the wall: Rooms only looked at it, so no tie.
    step(&app, &h.shell, Command::SetWallEndpoints { wall_id: wall.clone(), start: pt(0.0, 0.0), end: pt(8000.0, 0.0) }).await.unwrap();
    step(&app, &h.rooms, Command::DeleteElements { ids: vec![door_id.clone()] }).await.unwrap();
    // Accept Shell alone: Rooms' door was drawn on the shorter wall.
    app.swarm_accept(&h.id, Some(&keys(&["shell"])), Some(&reviewed(&app))).await.unwrap();
    let status = app.swarm_status().unwrap();
    let codes: Vec<(&str, &str)> = status.conflicts.iter().map(|c| (c.part.as_str(), c.code.as_str())).collect();
    assert_eq!(codes, [("rooms", "changed_on_main"), ("rooms", "lost_id")], "{:?}", status.conflicts);
    assert_eq!(status.conflicts[0].element_ids, vec![wall.clone()]);
    assert_eq!(status.conflicts[1].element_ids, vec![door_id.clone()], "the later step needs the lost door");
    let rooms = part_state(&app, "rooms");
    assert_eq!((rooms.steps, rooms.conflicts, rooms.last_seq), (0, 2, 0));
    let main = main_doc(&app).await.0;
    assert!(main.elements.iter().all(|e| e.id() != &door_id), "never relabelled, never applied");
}

#[tokio::test]
async fn discarding_a_part_takes_what_builds_on_it_only_after_a_confirm() {
    let (app, _dir) = app().await;
    let (before, revision) = main_doc(&app).await;
    let h = start(&app).await;
    let wall = step(&app, &h.shell, seg((0.0, 0.0), (6000.0, 0.0), None)).await.unwrap().diff.added[0].clone();
    step(&app, &h.rooms, door(&wall, 2000.0)).await.unwrap();
    let upper = step(&app, &h.upper, rect(0.0, 0.0, 5000.0, 5000.0, Some(&h.second))).await.unwrap();

    let e = app.swarm_part_discard("shell", false).await.unwrap_err();
    assert_eq!(e.code, "invalid");
    assert!(e.message.contains("Rooms builds on Shell"), "{}", e.message);
    assert_eq!(part_state(&app, "shell").steps, 1, "nothing discarded");

    let status = app.swarm_part_discard("shell", true).await.unwrap().unwrap();
    let state = |k: &str| status.parts.iter().find(|p| p.key == k).unwrap().clone();
    assert_eq!((state("shell").state, state("rooms").state), (SwarmPartState::Stopped, SwarmPartState::Stopped));
    assert_eq!((state("shell").steps, state("rooms").steps, state("upper").steps), (0, 0, 1));
    let drawn = view(&app).await;
    assert!(drawn.elements.iter().all(|e| e.id() != &wall), "Shell's wall is gone");
    assert!(upper.diff.added.iter().all(|id| drawn.elements.iter().any(|e| e.id() == id)), "Upper keeps its ids");
    assert_eq!(step(&app, &h.rooms, seg((0.0, 0.0), (3000.0, 0.0), None)).await.unwrap_err().code, "part_stopped");

    // The lead's setup takes Upper, which drew on its level: then nothing
    // is left and the swarm ends, unlocking the model.
    assert!(app.swarm_part_discard("lead", false).await.unwrap_err().message.contains("Upper"));
    assert_eq!(app.swarm_part_discard("lead", true).await.unwrap(), None);
    assert!(app.swarm_status().is_none());
    assert_eq!(main_doc(&app).await, (before, revision), "the model never moved");
    app.handle("doc_apply", json!({"command": seg((0.0, 0.0), (5000.0, 0.0), None)})).await.unwrap();
}

#[tokio::test]
async fn an_agent_undoes_and_redoes_only_its_own_draft_steps() {
    let (app, _dir) = app().await;
    let revision = main_doc(&app).await.1;
    let h = start(&app).await;
    let a = step(&app, &h.shell, seg((0.0, 0.0), (6000.0, 0.0), None)).await.unwrap().diff.added[0].clone();
    step(&app, &h.rooms, door(&a, 2000.0)).await.unwrap();
    let b = step(&app, &h.shell, seg((0.0, 8000.0), (6000.0, 8000.0), None)).await.unwrap();
    let b_id = b.diff.added[0].clone();
    let b_el = b.state.project.elements.iter().find(|e| e.id() == &b_id).cloned();

    let state = app.agent_undo(&h.shell, false, false).await.unwrap();
    assert!(state.project.elements.iter().all(|e| e.id() != &b_id), "its newest step left the draft");
    assert_eq!((state.can_redo, state.redo_label.is_some()), (true, true));
    let state = app.agent_undo(&h.shell, true, false).await.unwrap();
    assert_eq!(state.project.elements.iter().find(|e| e.id() == &b_id).cloned(), b_el, "redo brings it back with its id");
    app.agent_undo(&h.shell, false, false).await.unwrap();
    // Its first wall carries Rooms' door: refused, with or without force.
    for force in [false, true] {
        let e = app.agent_undo(&h.shell, false, force).await.unwrap_err();
        assert_eq!(e.code, "other_author", "{e:?}");
        assert!(e.message.contains("Rooms"), "{}", e.message);
    }
    // Rooms takes its own door back; then Shell may take the wall.
    app.agent_undo(&h.rooms, false, false).await.unwrap();
    app.agent_undo(&h.shell, false, false).await.unwrap();
    assert_eq!(app.agent_undo(&h.shell, false, false).await.unwrap_err().code, "nothing_to_undo");
    // A new step clears the redo.
    step(&app, &h.shell, seg((0.0, 0.0), (3000.0, 0.0), None)).await.unwrap();
    assert_eq!(app.agent_undo(&h.shell, true, false).await.unwrap_err().code, "nothing_to_undo");
    assert_eq!(main_doc(&app).await.1, revision, "the model's history never moved");
    assert!(app.swarm_status().unwrap().feed.iter().any(|f| f.text.starts_with("Took back")));
}

#[tokio::test]
async fn a_step_carries_its_agents_status_line() {
    let (app, _dir) = app().await;
    let h = start(&app).await;
    app.agent_set_status(&h.shell, Some("Walling the powder room"));
    step(&app, &h.shell, seg((0.0, 0.0), (6000.0, 0.0), None)).await.unwrap();
    let draft = app.swarm_draft(None).await.unwrap().unwrap();
    let last = draft.steps.last().unwrap();
    assert_eq!(last.status.as_deref(), Some("Walling the powder room"));
    assert_eq!(part_state(&app, "shell").status.as_deref(), Some("Walling the powder room"));
}

#[tokio::test]
async fn undoing_an_accepted_part_is_a_normal_undo() {
    let (app, _dir) = app().await;
    let h = start(&app).await;
    let walls = added_walls(&step(&app, &h.shell, rect(0.0, 0.0, 8000.0, 6000.0, None)).await.unwrap());
    step(&app, &h.rooms, door(&walls[0], 2000.0)).await.unwrap();
    app.swarm_accept(&h.id, Some(&keys(&["shell"])), Some(&reviewed(&app))).await.unwrap();
    let after_shell = main_doc(&app).await.0;
    app.swarm_accept(&h.id, None, Some(&reviewed(&app))).await.unwrap();
    assert!(app.swarm_status().is_none());
    app.handle("doc_undo", json!({})).await.unwrap();
    assert_eq!(main_doc(&app).await.0.elements, after_shell.elements, "one undo takes back Rooms");
    app.handle("doc_redo", json!({})).await.unwrap();
}

#[tokio::test]
async fn the_read_gate_serves_the_draft_and_refuses_what_shows_the_model() {
    let (app, _dir) = app().await;
    let h = start(&app).await;
    let wall = step(&app, &h.shell, seg((0.0, 0.0), (6000.0, 0.0), None)).await.unwrap().diff.added[0].clone();
    let other = app.agent_call(&hint("c-other", None), "look", CallKind::Read).unwrap();
    {
        let v = app.read_view(Some(&h.rooms.id)).await;
        assert!(v.is_draft());
        assert!(v.open_doc().unwrap().project().elements.iter().any(|e| e.id() == &wall), "a part reads its siblings' work");
    }
    for who in [Some(other.id.as_str()), None] {
        let v = app.read_view(who).await;
        assert!(!v.is_draft());
        assert!(v.open_doc().unwrap().project().elements.iter().all(|e| e.id() != &wall), "everyone else reads the model");
    }
    for what in [
        WindowOutput::Render,
        WindowOutput::RenderAi,
        WindowOutput::TourExport,
        WindowOutput::Export,
        WindowOutput::SaveVersion,
        WindowOutput::WindowLook,
        WindowOutput::View3d,
    ] {
        assert_eq!(app.refuse_draft_view(Some(&h.shell.id), what).unwrap_err().code, "draft_view", "{what:?}");
        assert!(app.refuse_draft_view(Some(&other.id), what).is_ok(), "{what:?} works for an agent outside the swarm");
        assert!(app.refuse_draft_view(None, what).is_ok());
    }
    // What replaces or shares the model waits for the draft.
    for (cmd, args) in [
        ("bundle_open", json!({"path": "/nowhere.guhit"})),
        ("live_join", json!({"invite": "x"})),
        ("hub_close", json!({})),
    ] {
        assert_eq!(app.handle(cmd, args).await.unwrap_err().code, "swarm_running", "{cmd}");
    }
    let _ = h.lead;
}

/// Random drafts accepted part by part, in random order (each accept takes
/// what the part depends on): the model ends with the draft's ids and
/// shapes, rooms aside from their count-based names, or the accept reports
/// conflicts; `swarm_check` foretells every difference.
#[tokio::test]
async fn accepting_parts_in_any_order_gives_the_draft_or_reports_conflicts() {
    for seed in 0..12u64 {
        let mut rng = Rng(seed);
        let (app, _dir) = app().await;
        let h = start(&app).await;
        let mut shell_walls: Vec<Id> = vec![];
        let (mut sx, mut ux) = (0.0, 0.0);
        for _ in 0..(5 + rng.below(8)) {
            match rng.below(4) {
                0 => {
                    let r = step(&app, &h.shell, rect(sx, 0.0, 4000.0 + 1000.0 * rng.below(3) as f64, 4000.0, None)).await.unwrap();
                    shell_walls.extend(added_walls(&r));
                    sx += 10_000.0;
                }
                1 if !shell_walls.is_empty() => {
                    let w = shell_walls[rng.below(shell_walls.len())].clone();
                    // Doors that do not fit are refused by the engine.
                    let _ = step(&app, &h.rooms, door(&w, 1000.0 + 500.0 * rng.below(3) as f64)).await;
                }
                2 => {
                    step(&app, &h.upper, rect(ux, 0.0, 5000.0, 3000.0 + 1000.0 * rng.below(3) as f64, Some(&h.second))).await.unwrap();
                    ux += 10_000.0;
                }
                _ => {
                    step(&app, &h.shell, seg((sx, 20_000.0), (sx + 3000.0, 20_000.0), None)).await.unwrap();
                    sx += 10_000.0;
                }
            }
        }
        let drawn = by_id(&view(&app).await);
        let mut order = vec!["shell", "rooms", "upper"];
        for i in (1..order.len()).rev() {
            order.swap(i, rng.below(i + 1));
        }
        for key in order {
            let Some(status) = app.swarm_status() else { break };
            if status.parts.iter().any(|p| p.key == key && p.state == SwarmPartState::Accepted) {
                continue;
            }
            let check = app.swarm_check(Some(&keys(&[key]))).await.unwrap();
            let shown = by_id(&view(&app).await);
            let before = main_doc(&app).await.0;
            let done = app.swarm_accept(&h.id, Some(&keys(&[key])), Some(&reviewed(&app))).await.unwrap();
            assert_eq!(done.skipped, check.skipped, "seed {seed}: the check foretold what stays out");
            let main = main_doc(&app).await.0;
            let new: BTreeSet<&Id> = main.elements.iter().map(|e| e.id()).filter(|id| !before.elements.iter().any(|b| b.id() == *id)).collect();
            let mut differ: BTreeSet<Id> = BTreeSet::new();
            for el in &main.elements {
                let Some(d) = drawn.get(el.id()) else {
                    assert!(!new.contains(el.id()), "seed {seed}: {} is not a draft id", el.id());
                    continue;
                };
                assert_eq!(el.kind(), d.kind(), "seed {seed}: never relabelled");
                if el != d {
                    match (el, d) {
                        (Element::Room(a), Element::Room(b)) => assert_eq!((&a.seed, &a.level_id), (&b.seed, &b.level_id), "seed {seed}"),
                        _ => panic!("seed {seed}: {} differs from the draft", el.id()),
                    }
                }
                // Against the view the person looked at when they checked.
                if new.contains(el.id()) && shown.get(el.id()).is_some_and(|v| v != el) {
                    differ.insert(el.id().clone());
                }
            }
            let foretold: BTreeSet<Id> = check.differs.iter().flat_map(|c| c.element_ids.clone()).collect();
            assert_eq!(differ, foretold, "seed {seed}, {key}: the check foretold every difference");
        }
        if app.swarm_status().is_some() {
            app.swarm_accept(&h.id, None, Some(&reviewed(&app))).await.unwrap();
        }
        let main = by_id(&main_doc(&app).await.0);
        for (id, d) in &drawn {
            let m = main.get(id).unwrap_or_else(|| panic!("seed {seed}: {id} never reached the model"));
            assert_eq!(m.kind(), d.kind());
        }
    }
}

/// Four agents draw at once on a multi-thread runtime while a person checks
/// and accepts parts and an agent undoes: no deadlock (a timeout guards it),
/// and every accepted step is in the model with its draft id.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn four_agents_write_while_parts_are_accepted_without_deadlock() {
    let (app, _dir) = app().await;
    let ground = main_doc(&app).await.0.levels[0].id.clone();
    let lead = app.agent_call(&hint("w-lead", None), "swarm", CallKind::Edit).unwrap();
    let mut plan = SwarmPlan {
        summary: "Four floors".into(),
        setup: SwarmSetup {
            levels: ["L2", "L3", "L4"].iter().map(|n| SwarmLevel { name: n.to_string(), height_mm: None }).collect(),
        },
        parts: vec![],
    };
    for (i, level) in [ground.as_str(), "L2", "L3", "L4"].iter().enumerate() {
        plan.parts.push(part(&format!("floor-{i}"), &format!("Floor {i}"), vec![level.to_string()], vec![]));
    }
    let started = app.swarm_start_mcp(&lead, "Four floors", plan).await.unwrap();
    let swarm_id = started.swarm.id.clone();
    let levels: Vec<Id> = view(&app).await.levels.iter().map(|l| l.id.clone()).collect();
    let work = async {
        let mut writers = vec![];
        for (i, h) in started.handles.iter().enumerate() {
            let app = app.clone();
            let handle = h.handle.clone();
            let level = levels[i].clone();
            writers.push(tokio::spawn(async move {
                let call = app.agent_call(&hint(&format!("w-{i}"), Some(&handle)), "build", CallKind::Edit).unwrap();
                let mut made: Vec<Id> = vec![];
                for n in 0..12 {
                    let x = n as f64 * 7000.0;
                    loop {
                        match app.agent_commit(&call, rect(x, 0.0, 5000.0, 4000.0, Some(&level))).await {
                            Ok(r) => {
                                made.extend(r.diff.added);
                                break;
                            }
                            Err(e) if e.code == "rate_limited" => tokio::time::sleep(Duration::from_millis(60)).await,
                            // Accepted under it, or swarm over: this writer is done.
                            Err(e) if e.code == "part_stopped" => return (call, made, true),
                            Err(e) => panic!("writer {i}: {e:?}"),
                        }
                    }
                    if n == 5 {
                        // Take a step back and forth while the others draw.
                        let _ = app.agent_undo(&call, false, false).await;
                        let _ = app.agent_undo(&call, true, false).await;
                    }
                }
                (call, made, false)
            }));
        }
        // A person checks and accepts parts while they draw.
        for key in ["floor-1", "floor-3"] {
            tokio::time::sleep(Duration::from_millis(400)).await;
            app.swarm_check(None).await.unwrap();
            loop {
                let pins: BTreeMap<String, u32> = app.swarm_status().unwrap().parts.iter().map(|p| (p.key.clone(), p.last_seq)).collect();
                match app.swarm_accept(&swarm_id, Some(&[key.to_string()]), Some(&pins)).await {
                    Ok(_) => break,
                    // The part drew between the look and the accept.
                    Err(e) if e.code == "stale" => tokio::time::sleep(Duration::from_millis(20)).await,
                    Err(e) => panic!("accept {key}: {e:?}"),
                }
            }
        }
        let mut results = vec![];
        for w in writers {
            results.push(w.await.unwrap());
        }
        results
    };
    let results = tokio::time::timeout(Duration::from_secs(120), work).await.expect("no deadlock: the work finished");
    let status = app.swarm_status().unwrap();
    assert!(status.conflicts.is_empty(), "{:?}", status.conflicts);
    app.swarm_accept(&swarm_id, None, Some(&status.parts.iter().map(|p| (p.key.clone(), p.last_seq)).collect())).await.unwrap();
    let main: BTreeSet<Id> = main_doc(&app).await.0.elements.iter().map(|e| e.id().clone()).collect();
    for (call, made, stopped) in &results {
        let landed: Vec<&Id> = made.iter().filter(|id| main.contains(*id)).collect();
        // An undone and redone step keeps its ids; everything a writer was
        // told landed is in the model, by the draft's ids.
        assert_eq!(landed.len(), made.len(), "{}: stopped early {stopped}", call.name);
        assert!(!made.is_empty());
    }
}

// ========================================================= gate repros

fn count(p: &Project, kind: ElementKind) -> usize {
    p.elements.iter().filter(|e| e.kind() == kind).count()
}

/// Gate P1-2 (the reviewer's repro): 60 draft walls accepted while someone
/// keeps renaming the project. A rename changed the model without a
/// revision, the accept had taken the draft out before committing, and 6
/// of 6 accepts lost the whole draft. Now a rename of the open project is
/// refused while a draft is open, and nothing is lost.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn accept_all_racing_a_rename_never_loses_the_draft() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    for trial in 0..3 {
        let (app, _dir) = app().await;
        let main = main_doc(&app).await.0;
        let ground = main.levels[0].id.clone();
        let lead = app.agent_call(&hint("c-lead", None), "swarm", CallKind::Edit).unwrap();
        let plan = SwarmPlan { summary: "walls".into(), setup: SwarmSetup::default(), parts: vec![part("shell", "Shell", vec![ground], vec![ElementKind::Wall])] };
        let started = app.swarm_start_mcp(&lead, "Walls", plan).await.unwrap();
        let shell = app.agent_call(&hint("c-shell", Some(&started.handles[0].handle)), "build", CallKind::Edit).unwrap();
        for i in 0..60 {
            let x = i as f64 * 2500.0;
            step(&app, &shell, seg((x, 0.0), (x + 1500.0, 0.0), None)).await.unwrap();
        }
        let stop = Arc::new(AtomicBool::new(false));
        let renamer = {
            let (app, stop, id) = (app.clone(), stop.clone(), main.id.clone());
            tokio::spawn(async move {
                let (mut tried, mut refused) = (0u32, 0u32);
                while !stop.load(Ordering::SeqCst) {
                    tried += 1;
                    if let Err(e) = app.handle("hub_rename", json!({"id": id, "name": format!("Race {tried}")})).await {
                        refused += u32::from(e.code == "swarm_running");
                    }
                    tokio::task::yield_now().await;
                }
                (tried, refused)
            })
        };
        let result = app.swarm_accept(&started.swarm.id, None, None).await;
        stop.store(true, Ordering::SeqCst);
        let (tried, refused) = renamer.await.unwrap();
        let after = main_doc(&app).await.0;
        let open = app.swarm_status().is_some();
        let walls = count(&after, ElementKind::Wall);
        println!("trial {trial}: accept = {:?}, renames tried {tried}, refused {refused}, draft open = {open}, walls in model = {walls}", result.as_ref().map(|r| r.applied.len()).map_err(|e| e.code.clone()));
        assert!(result.is_ok(), "trial {trial}: {result:?}");
        assert_eq!((walls, open), (60, false), "trial {trial}: the whole draft is in the model");
    }
}

/// Gate P1-3 (the reviewer's repro): Shell draws a box, Dims dimensions it.
/// Dims alone does not replay on the model (no walls there), so accepting
/// it would drop its step. The accept is refused `steps_skipped`; "Accept
/// the rest" keeps the step in the draft, as a conflict, and never marks
/// Dims accepted; Accept all then brings everything in.
#[tokio::test]
async fn accepting_a_part_never_drops_its_steps_without_a_trace_in_the_draft() {
    let (app, _dir) = app().await;
    let main = main_doc(&app).await.0;
    let ground = main.levels[0].id.clone();
    let lead = app.agent_call(&hint("c-lead", None), "swarm", CallKind::Edit).unwrap();
    let plan = SwarmPlan {
        summary: "box and dims".into(),
        setup: SwarmSetup::default(),
        parts: vec![part("shell", "Shell", vec![ground.clone()], vec![ElementKind::Wall]), part("dims", "Dims", vec![ground.clone()], vec![ElementKind::Dimension])],
    };
    let started = app.swarm_start_mcp(&lead, "Box", plan).await.unwrap();
    let h = |k: &str| started.handles.iter().find(|x| x.key == k).unwrap().handle.clone();
    let shell = app.agent_call(&hint("c-shell", Some(&h("shell"))), "build", CallKind::Edit).unwrap();
    let dims = app.agent_call(&hint("c-dims", Some(&h("dims"))), "build", CallKind::Edit).unwrap();
    step(&app, &shell, rect(0.0, 0.0, 9000.0, 6000.0, None)).await.unwrap();
    step(&app, &dims, Command::AutoDimension { level_id: ground.clone(), kinds: vec![AutoDimKind::Exterior] }).await.unwrap();
    app.swarm_finish(&dims, Some("dimensioned")).unwrap();
    let drawn = count(&view(&app).await, ElementKind::Dimension);
    assert!(drawn > 0);

    let check = app.swarm_check(Some(&keys(&["dims"]))).await.unwrap();
    assert!(check.applied.is_empty(), "nothing of Dims would go in: {:?}", check.applied);
    assert_eq!(check.skipped.iter().map(|c| c.code.as_str()).collect::<Vec<_>>(), ["nothing_to_dimension"]);

    // Asked first: refused, nothing changes.
    let before = app.swarm_status().unwrap();
    let e = app.swarm_accept(&started.swarm.id, Some(&keys(&["dims"])), None).await.unwrap_err();
    assert_eq!(e.code, "steps_skipped", "{e:?}");
    assert!(e.message.contains("Dims"), "{}", e.message);
    assert_eq!(app.swarm_status().unwrap().draft_revision, before.draft_revision, "nothing changed");

    // Accept the rest: there is none, and the step stays in the draft.
    let done = app.swarm_accept_with(&started.swarm.id, Some(&keys(&["dims"])), None, true).await.unwrap();
    assert!(done.applied.is_empty());
    assert_eq!(done.skipped.len(), 1);
    let model = main_doc(&app).await.0;
    let d = part_state(&app, "dims");
    println!("after: model dimensions {}, dims part state {:?}, dims steps in draft {}", count(&model, ElementKind::Dimension), d.state, d.steps);
    assert_eq!((d.state, d.steps, d.conflicts), (SwarmPartState::Done, 1, 1), "Dims as it was, its step kept and flagged");
    assert_eq!(count(&model, ElementKind::Dimension), 0);
    assert_eq!(count(&view(&app).await, ElementKind::Dimension), drawn, "the draft still shows the dimensions");
    let status = app.swarm_status().unwrap();
    assert_eq!(status.conflicts.iter().map(|c| (c.part.as_str(), c.code.as_str())).collect::<Vec<_>>(), [("dims", "nothing_to_dimension")]);

    // With Shell, Dims goes in, and the conflict with it.
    app.swarm_accept(&started.swarm.id, None, Some(&reviewed(&app))).await.unwrap();
    assert!(app.swarm_status().is_none());
    assert_eq!(count(&main_doc(&app).await.0, ElementKind::Dimension), drawn);
}

/// Gate P1-3: a part is accepted only when its steps went in. Upper has
/// drawn nothing, so accepting it applies nothing and it keeps working;
/// Shell's step goes in, so Shell is accepted.
#[tokio::test]
async fn a_part_is_accepted_only_when_its_steps_went_in() {
    let (app, _dir) = app().await;
    let h = start(&app).await;
    step(&app, &h.shell, rect(0.0, 0.0, 6000.0, 4000.0, None)).await.unwrap();
    let done = app.swarm_accept(&h.id, Some(&keys(&["upper"])), Some(&reviewed(&app))).await.unwrap();
    assert!(done.applied.is_empty(), "{:?}", done.applied);
    assert_eq!(part_state(&app, "upper").state, SwarmPartState::Working);
    let done = app.swarm_accept(&h.id, Some(&keys(&["shell"])), Some(&reviewed(&app))).await.unwrap();
    assert_eq!(done.applied.iter().map(|a| a.part.as_str()).collect::<Vec<_>>(), ["shell"]);
    assert_eq!(part_state(&app, "shell").state, SwarmPartState::Accepted);
}

/// splitmix64, so runs repeat.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}
