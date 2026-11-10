//! The minimal swarm draft (C0): binding, the draft, the main lock, accept
//! all and discard.

use super::*;
use crate::agents::CallKind;
use serde_json::json;

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
    app.handle("hub_create", json!({"name": "Swarm test", "template": "blank"})).await.unwrap();
    (app, dir)
}

async fn main_doc(app: &AppService) -> (Project, u32) {
    let s = app.session.lock().await;
    let doc = s.open_doc().unwrap();
    (doc.project().clone(), doc.revision())
}

fn part(key: &str, name: &str, kinds: Vec<ElementKind>) -> SwarmPartPlan {
    SwarmPartPlan {
        key: key.into(),
        name: name.into(),
        role: String::new(),
        instructions: String::new(),
        scope: SwarmScope { levels: vec![], kinds, area: None },
        after: vec![],
        milestones: vec![],
    }
}

/// Shell draws walls and columns, Rooms rooms, doors and windows after
/// Shell's walls, and Setup adds a second floor.
fn house_plan() -> SwarmPlan {
    let mut shell = part("shell", "Shell", vec![ElementKind::Wall, ElementKind::Column]);
    shell.milestones = vec!["walls".into()];
    let mut rooms = part("rooms", "Rooms", vec![ElementKind::Room, ElementKind::Opening]);
    rooms.after = vec![SwarmWait { part: "shell".into(), milestone: "walls".into() }];
    SwarmPlan {
        summary: "A small house".into(),
        setup: SwarmSetup { levels: vec![SwarmLevel { name: "Second Floor".into(), height_mm: None }] },
        parts: vec![shell, rooms],
    }
}

fn chain(points: &[(f64, f64)], closed: bool) -> Command {
    Command::AddWallChain {
        points: points.iter().map(|(x, y)| Point { x: *x, y: *y }).collect(),
        closed,
        thickness_mm: None,
        level_id: None,
        role: None,
    }
}

fn wall(a: (f64, f64), b: (f64, f64), level_id: Option<Id>) -> Command {
    Command::AddWall {
        start: Point { x: a.0, y: a.1 },
        end: Point { x: b.0, y: b.1 },
        thickness_mm: None,
        height_mm: None,
        material_id: None,
        level_id,
        role: None,
    }
}

fn opening(wall_id: &Id, kind: OpeningType) -> Command {
    Command::AddOpening {
        wall_id: wall_id.clone(),
        opening_type: kind,
        offset_mm: 2000.0,
        width_mm: None,
        height_mm: None,
        sill_mm: None,
        style: None,
        flip_side: None,
        flip_hinge: None,
        type_id: None,
    }
}

/// The walls among `ids` on `project`.
fn walls_of(project: &Project, ids: &[Id]) -> Vec<Id> {
    project
        .elements
        .iter()
        .filter(|e| matches!(e, Element::Wall(_)) && ids.contains(e.id()))
        .map(|e| e.id().clone())
        .collect()
}

struct Started {
    lead: AgentCall,
    started: SwarmStarted,
}

async fn start(app: &AppService, plan: SwarmPlan) -> Started {
    let lead = app.agent_call(&hint("s-lead", None), "swarm", CallKind::Edit).unwrap();
    let started = app.swarm_start_mcp(&lead, "A small house", plan).await.unwrap();
    Started { lead, started }
}

fn handle_of_part(st: &Started, key: &str) -> String {
    st.started.handles.iter().find(|h| h.key == key).unwrap().handle.clone()
}

#[tokio::test]
async fn accept_all_reproduces_the_draft_ids_one_undo_step_per_part() {
    let (app, _dir) = app().await;
    let (before, revision) = main_doc(&app).await;
    let st = start(&app, house_plan()).await;
    assert_eq!(st.started.swarm.parts.len(), 3, "the lead and two parts");
    assert_eq!(st.started.swarm.parts[2].state, SwarmPartState::Waiting, "Rooms waits for Shell's walls");
    // Sub-agents of one connection, told apart by their handles.
    let shell = app.agent_call(&hint("s-lead", Some(&handle_of_part(&st, "shell"))), "add_walls", CallKind::Edit).unwrap();
    let rooms = app.agent_call(&hint("s-lead", Some(&handle_of_part(&st, "rooms"))), "add_opening", CallKind::Edit).unwrap();
    assert_eq!((shell.name.as_str(), rooms.name.as_str()), ("Shell", "Rooms"), "shown as their parts");
    assert_eq!(app.swarm_part_of(&shell.id).as_deref(), Some("shell"));
    assert_eq!(app.swarm_part_of(&st.lead.id).as_deref(), Some(LEAD));

    // Interleaved: Shell, Rooms on Shell's walls, Shell again, Rooms again.
    let first = app.agent_commit(&shell, chain(&[(0.0, 0.0), (6000.0, 0.0), (6000.0, 4000.0), (0.0, 4000.0)], true)).await.unwrap();
    let walls = walls_of(&first.state.project, &first.diff.added);
    assert_eq!(walls.len(), 4);
    app.agent_commit(&rooms, opening(&walls[0], OpeningType::Door)).await.unwrap();
    app
        .agent_edit(&shell, |p: &Project, _: &Derived| {
            Staging::rebuilt(p, "MCP: add walls", &[wall((6000.0, 0.0), (9000.0, 0.0), None)]).map(|s| (s, ())).map_err(IpcError::from)
        })
        .await
        .unwrap();
    app.agent_commit(&rooms, opening(&walls[2], OpeningType::Window)).await.unwrap();

    // Main never moved.
    let (during, at) = main_doc(&app).await;
    assert_eq!((during.elements.clone(), during.levels.clone(), at), (before.elements.clone(), before.levels.clone(), revision));

    let draft = app.swarm_draft(None).await.unwrap().unwrap();
    assert_eq!(draft.steps.len(), 5, "setup, then four agent steps");
    assert_eq!(draft.steps.iter().map(|s| s.part.as_str()).collect::<Vec<_>>(), [LEAD, "shell", "rooms", "shell", "rooms"]);
    let view = draft.state.project.clone();
    assert_eq!(view.levels.len(), before.levels.len() + 1, "setup added the second floor");
    let status = app.swarm_status().unwrap();
    assert_eq!(status.parts[2].state, SwarmPartState::Working, "a part that writes is working");
    let reviewed: BTreeMap<String, u32> = status.parts.iter().map(|p| (p.key.clone(), p.last_seq)).collect();

    // A part that wrote after the review is refused.
    let mut old = reviewed.clone();
    old.insert("rooms".into(), 2);
    let e = app.swarm_accept_all(&status.id, Some(&old)).await.unwrap_err();
    assert_eq!(e.code, "stale", "{e:?}");

    let done = app.swarm_accept_all(&status.id, Some(&reviewed)).await.unwrap();
    assert_eq!(done.applied.iter().map(|a| a.part.as_str()).collect::<Vec<_>>(), [LEAD, "shell", "rooms"], "in the order of first steps");
    let (after, now) = main_doc(&app).await;
    assert_eq!(after.elements, view.elements, "every id is the draft's");
    assert_eq!(after.levels, view.levels);
    assert_eq!(now, revision + 3, "one revision per part");
    assert!(app.swarm_status().is_none(), "the swarm ended");

    let history: HistoryList = serde_json::from_value(app.handle("history_list", json!({})).await.unwrap()).unwrap();
    let labels: Vec<&str> = history.undo.iter().take(3).map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["Swarm: Rooms", "Swarm: Shell", "Swarm: Lead"]);
    let authors: Vec<Option<&str>> = history.undo.iter().take(3).map(|s| s.author_id.as_deref()).collect();
    assert_eq!(authors, [Some(rooms.id.as_str()), Some(shell.id.as_str()), Some(st.lead.id.as_str())]);
    assert!(history.undo.iter().take(3).all(|s| s.origin == Origin::Ai));
    let versions: Vec<SnapshotMeta> = serde_json::from_value(app.handle("snapshot_list", json!({})).await.unwrap()).unwrap();
    assert!(versions.iter().any(|v| v.label.starts_with("Before swarm")), "a version first");

    // Three undos take the swarm back out.
    app.handle("doc_undo", json!({"steps": 3})).await.unwrap();
    assert_eq!(main_doc(&app).await.0.elements, before.elements);
}

#[tokio::test]
async fn the_main_lock_refuses_the_window_the_copilot_and_other_agents() {
    let (app, _dir) = app().await;
    app.handle("doc_apply", json!({"command": wall((0.0, -5000.0), (4000.0, -5000.0), None)})).await.unwrap();
    let project_id = main_doc(&app).await.0.id;
    let st = start(&app, house_plan()).await;
    let (_, revision) = main_doc(&app).await;
    let running = |r: Result<Value, IpcError>| assert_eq!(r.unwrap_err().code, "swarm_running");

    // The window.
    running(app.handle("doc_apply", json!({"command": wall((0.0, 9000.0), (4000.0, 9000.0), None)})).await);
    running(app.handle("doc_undo", json!({})).await);
    running(app.handle("doc_redo", json!({})).await);
    running(app.handle("hub_close", json!({})).await);
    running(app.handle("hub_create", json!({"name": "Another"})).await);
    running(app.handle("hub_delete", json!({"id": project_id})).await);
    running(app.handle("live_host", json!({})).await);
    let versions: Vec<SnapshotMeta> = serde_json::from_value(app.handle("snapshot_list", json!({})).await.unwrap()).unwrap();
    if let Some(v) = versions.first() {
        running(app.handle("snapshot_restore", json!({"id": v.id})).await);
    }
    // Opening the open project changes nothing, so it passes.
    app.handle("hub_open", json!({"id": project_id})).await.unwrap();
    // The copilot commits through these.
    let e = app.commit_if_revision(wall((0.0, 9000.0), (4000.0, 9000.0), None), Origin::Ai, revision).await.unwrap_err();
    assert_eq!(e.code, "swarm_running");
    assert_eq!(e.message, "A swarm draft is open: accept or discard it first.");
    let base = main_doc(&app).await.0;
    let staged = Staging::rebuilt(&base, "AI", &[wall((0.0, 9000.0), (4000.0, 9000.0), None)]).unwrap();
    assert_eq!(app.commit_staged_if_revision(staged, Origin::Ai, revision).await.unwrap_err().code, "swarm_running");
    // An agent outside the swarm, on another connection.
    let other = app.agent_call(&hint("s-other", None), "add_walls", CallKind::Edit).unwrap();
    assert!(app.swarm_part_of(&other.id).is_none());
    assert_eq!(app.agent_commit(&other, wall((0.0, 9000.0), (4000.0, 9000.0), None)).await.unwrap_err().code, "swarm_running");
    assert_eq!(app.agent_undo(&other, false, true).await.unwrap_err().code, "swarm_running");
    assert_eq!(main_doc(&app).await.1, revision, "nothing reached the model");

    // Discard unlocks.
    let e = app.handle("swarm_discard", json!({"swarm_id": "someone-else"})).await.unwrap_err();
    assert_eq!(e.code, "not_found");
    app.handle("swarm_discard", json!({"swarm_id": st.started.swarm.id})).await.unwrap();
    app.handle("doc_apply", json!({"command": wall((0.0, 9000.0), (4000.0, 9000.0), None)})).await.unwrap();
    app.agent_commit(&other, wall((0.0, 12000.0), (4000.0, 12000.0), None)).await.unwrap();

    // So does an accept.
    let st = start(&app, house_plan()).await;
    running(app.handle("doc_undo", json!({})).await);
    app.handle("swarm_accept", json!({"swarm_id": st.started.swarm.id})).await.unwrap();
    app.handle("doc_undo", json!({})).await.unwrap();
}

#[tokio::test]
async fn a_bound_agent_edit_lands_in_the_draft_and_never_in_main() {
    let (app, _dir) = app().await;
    let mut events = app.events();
    let st = start(&app, house_plan()).await;
    let (before, revision) = main_doc(&app).await;
    let shell = app.agent_call(&hint("s-shell", Some(&handle_of_part(&st, "shell"))), "add_walls", CallKind::Edit).unwrap();
    let result = app.agent_commit(&shell, wall((0.0, 0.0), (5000.0, 0.0), None)).await.unwrap();
    let new_wall = result.diff.added[0].clone();
    assert!(result.state.project.elements.iter().any(|e| e.id() == &new_wall), "the answer is the draft view");
    let (after, at) = main_doc(&app).await;
    assert_eq!((after.elements, at), (before.elements, revision), "main untouched");
    assert!(app.swarm.view_for(&shell.id).is_some(), "the agent reads the draft");
    assert!(app.swarm.view_for("agent-nobody").is_none());

    let draft = app.swarm_draft(Some(1)).await.unwrap().unwrap();
    assert_eq!(draft.steps.len(), 1, "only the steps after since_step");
    let shell_ids = draft.parts.iter().find(|p| p.part == "shell").unwrap();
    assert_eq!(shell_ids.ids, vec![new_wall.clone()]);
    assert!(draft.removed.is_empty());
    let status = app.swarm_status().unwrap();
    assert_eq!((status.added, status.parts[1].steps, status.parts[1].agent_id.clone()), (1, 1, Some(shell.id.clone())));
    // The step was published, within the events' pace.
    let mut saw = false;
    let deadline = tokio::time::Instant::now() + EVENT_GAP * 3;
    while let Ok(Ok(event)) = tokio::time::timeout_at(deadline, events.recv()).await {
        if let AppEvent::Swarm { swarm: Some(s) } = event {
            saw |= s.draft_revision == status.draft_revision;
            if saw {
                break;
            }
        }
    }
    assert!(saw, "AppEvent::Swarm after the step");
    // Its presence moved to the draft wall.
    let agent = app.agents_list().into_iter().find(|a| a.participant.id == shell.id).unwrap();
    assert_eq!(agent.presence.selection, vec![new_wall]);
    assert_eq!(agent.participant.name, "Shell");
    assert_eq!(agent.participant.color, status.parts[1].color);
}

#[tokio::test]
async fn a_door_in_a_model_wall_reports_the_wall_for_display() {
    let (app, _dir) = app().await;
    let made = app.handle("doc_apply", json!({"command": wall((0.0, 0.0), (6000.0, 0.0), None)})).await.unwrap();
    let wall_id: Id = made["diff"]["added"][0].as_str().unwrap().to_string();
    let st = start(&app, house_plan()).await;
    let rooms = app.agent_call(&hint("s-rooms", Some(&handle_of_part(&st, "rooms"))), "add_opening", CallKind::Edit).unwrap();
    let door = app.agent_commit(&rooms, opening(&wall_id, OpeningType::Door)).await.unwrap().diff.added[0].clone();
    let draft = app.swarm_draft(None).await.unwrap().unwrap();
    assert_eq!(draft.display_modified, vec![wall_id]);
    assert!(draft.parts.iter().any(|p| p.part == "rooms" && p.ids == vec![door.clone()]));
}

#[tokio::test]
async fn a_part_is_held_to_its_scope() {
    let (app, _dir) = app().await;
    let mut plan = house_plan();
    plan.parts.push(SwarmPartPlan {
        scope: SwarmScope { levels: vec!["Second Floor".into()], kinds: vec![ElementKind::Asset], area: None },
        ..part("upper", "Upper", vec![])
    });
    let st = start(&app, plan).await;
    let rooms = app.agent_call(&hint("s-rooms", Some(&handle_of_part(&st, "rooms"))), "add_walls", CallKind::Edit).unwrap();
    let e = app.agent_commit(&rooms, wall((0.0, 0.0), (5000.0, 0.0), None)).await.unwrap_err();
    assert_eq!(e.code, "out_of_scope", "{e:?}");
    assert!(e.message.contains("Rooms") && e.message.contains("room, opening"), "{}", e.message);

    // A level limit: the setup level resolved to its id.
    let status = app.swarm_status().unwrap();
    let upper_plan = status.plan.unwrap().parts.into_iter().find(|p| p.key == "upper").unwrap();
    let view = app.swarm_draft(None).await.unwrap().unwrap().state.project;
    let second = view.levels.iter().find(|l| l.name == "Second Floor").unwrap();
    assert_eq!(upper_plan.scope.levels, vec![second.id.clone()]);
    let shell = app.agent_call(&hint("s-shell", Some(&handle_of_part(&st, "shell"))), "add_walls", CallKind::Edit).unwrap();
    let e = app.agent_commit(&shell, wall((0.0, 0.0), (5000.0, 0.0), Some(second.id.clone()))).await;
    assert!(e.is_ok(), "Shell has every level: {e:?}");

    // Only people set the signatory, in a draft too.
    let e = app
        .agent_commit(&shell, Command::SetSignatory { signatory: Box::default() })
        .await
        .unwrap_err();
    assert_eq!(e.code, "forbidden");

    // A dry run says the same as the commit.
    let draft_view = app.swarm.view_for(&rooms.id).unwrap();
    let staged = Staging::rebuilt(draft_view.project(), "dry", &[wall((0.0, 0.0), (5000.0, 0.0), None)]).unwrap();
    assert_eq!(app.agent_check(&rooms, draft_view.derived(), &staged).unwrap_err().code, "out_of_scope");
    assert_eq!(app.swarm_status().unwrap().parts[2].steps, 0, "a refused step leaves no trace");
}

#[tokio::test]
async fn handles_are_unguessable_and_never_reused() {
    let (app, _dir) = app().await;
    let st = start(&app, house_plan()).await;
    let first: Vec<String> = st.started.handles.iter().map(|h| h.handle.clone()).collect();
    for h in &st.started.handles {
        let token = h.handle.strip_prefix(HANDLE_PREFIX).and_then(|t| t.strip_suffix(&format!("-{}", h.key))).unwrap();
        assert_eq!(token.len(), 16, "64 random bits: {}", h.handle);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert!(h.url.ends_with(&format!("/mcp/agent/{}", h.handle)), "{}", h.url);
    }
    assert_ne!(&first[0][3..19], &first[1][3..19], "each part its own token");
    // A made-up handle binds nothing.
    let guess = app.agent_call(&hint("s-guess", Some("sw-0000000000000000-shell")), "add_walls", CallKind::Edit).unwrap();
    assert!(app.swarm_part_of(&guess.id).is_none());
    app.swarm_discard(None).await.unwrap();

    // After the swarm: an old handle is refused, never sent to the model.
    let e = app.agent_call(&hint("s-late", Some(&first[0])), "add_walls", CallKind::Edit).unwrap_err();
    assert_eq!(e.code, "part_stopped");
    let st = start(&app, house_plan()).await;
    let second: Vec<String> = st.started.handles.iter().map(|h| h.handle.clone()).collect();
    assert!(second.iter().all(|h| !first.contains(h)), "never issued twice");
    let e = app.agent_call(&hint("s-late", Some(&first[1])), "add_walls", CallKind::Edit).unwrap_err();
    assert_eq!(e.code, "part_stopped", "an old handle is not the new swarm's");
    // The labelled URL binds as the argument does.
    let label = AgentHint { label: Some(second[0].clone()), ..hint("l-codex", None) };
    let codex = app.agent_call(&label, "add_walls", CallKind::Edit).unwrap();
    assert_eq!(app.swarm_part_of(&codex.id).as_deref(), Some("shell"));
}

#[tokio::test]
async fn a_second_swarm_and_bad_plans_are_refused() {
    let (app, _dir) = app().await;
    let lead = app.agent_call(&hint("s-lead", None), "swarm", CallKind::Edit).unwrap();
    let refused = |plan: SwarmPlan| {
        let app = app.clone();
        let lead = lead.clone();
        async move { app.swarm_start_mcp(&lead, "x", plan).await.unwrap_err() }
    };
    let mut overlap = house_plan();
    overlap.parts[1].scope.kinds.push(ElementKind::Wall);
    assert!(refused(overlap).await.message.contains("`shell` and `rooms`"));
    let mut bad_key = house_plan();
    bad_key.parts[0].key = "Shell Part".into();
    assert_eq!(refused(bad_key).await.code, "bad_args");
    let mut lead_key = house_plan();
    lead_key.parts[0].key = LEAD.into();
    assert_eq!(refused(lead_key).await.code, "bad_args");
    let mut circle = house_plan();
    circle.parts[1].milestones = vec!["doors".into()];
    circle.parts[0].after = vec![SwarmWait { part: "rooms".into(), milestone: "doors".into() }];
    assert_eq!(refused(circle).await.code, "bad_args");
    let mut unknown_level = house_plan();
    unknown_level.parts[0].scope.levels = vec!["Basement".into()];
    assert!(refused(unknown_level).await.message.contains("Basement"));
    let mut area = house_plan();
    area.parts[0].scope.area = Some(PlanRect { min: Point { x: 0.0, y: 0.0 }, max: Point { x: 1.0, y: 1.0 } });
    assert_eq!(refused(area).await.code, "bad_args");
    assert!(app.swarm_status().is_none(), "nothing started");

    app.swarm_start_mcp(&lead, "x", house_plan()).await.unwrap();
    assert_eq!(refused(house_plan()).await.code, "draft_open");
}

#[tokio::test]
async fn the_ipc_commands_have_their_shapes_and_stubs_say_not_built() {
    let (app, _dir) = app().await;
    assert_eq!(app.handle("swarm_status", json!({})).await.unwrap(), Value::Null);
    assert_eq!(app.handle("swarm_draft", json!({})).await.unwrap(), Value::Null);
    let request = json!({"brief": "A bungalow"});
    // L2's commands are built: the estimate prices a request, and the
    // others answer for themselves, never `not_built`.
    let cost: SwarmCost = serde_json::from_value(app.handle("swarm_estimate", json!({"request": request})).await.unwrap()).unwrap();
    assert!(cost.low_usd.is_some() && cost.high_usd >= cost.low_usd, "{cost:?}");
    for (cmd, args) in [
        ("swarm_pause", json!({"part": null})),
        ("swarm_resume", json!({})),
        ("swarm_stop", json!({"part": "shell"})),
        ("swarm_set_cap", json!({"cap_usd": 5.0})),
    ] {
        if let Err(e) = app.handle(cmd, args).await {
            assert_ne!(e.code, "not_built", "{cmd}: {e:?}");
        }
    }
    // L1's commands: without a swarm there is nothing to act on.
    for (cmd, args) in [
        ("swarm_part_discard", json!({"part": "shell", "with_dependents": true})),
        ("swarm_check", json!({"parts": ["shell"]})),
        ("swarm_accept", json!({"swarm_id": "x", "parts": ["shell"]})),
    ] {
        assert_eq!(app.handle(cmd, args).await.unwrap_err().code, "not_found", "{cmd}");
    }
    assert_eq!(app.handle("swarm_part_discard", json!({})).await.unwrap_err().code, "bad_args");
    assert_eq!(app.handle("swarm_start", json!({})).await.unwrap_err().code, "bad_args");
    assert_eq!(app.handle("swarm_set_cap", json!({"cap_usd": "lots"})).await.unwrap_err().code, "bad_args");
    assert_eq!(app.handle("swarm_accept", json!({"swarm_id": "x"})).await.unwrap_err().code, "not_found");
    assert_eq!(app.handle("swarm_discard", json!({"swarm_id": "x"})).await.unwrap_err().code, "not_found");
    let st = start(&app, house_plan()).await;
    let status: SwarmStatus = serde_json::from_value(app.handle("swarm_status", json!({})).await.unwrap()).unwrap();
    assert_eq!(status.id, st.started.swarm.id);
    assert_eq!(status.source, SwarmSource::Mcp { client: "Claude Code".into() });
    let draft: SwarmDraftState = serde_json::from_value(app.handle("swarm_draft", json!({"since_step": 0})).await.unwrap()).unwrap();
    assert_eq!(draft.steps.len(), 1, "the setup step");
}

// ======================================================== gate fixes (FA)

/// A house drawn into the draft: setup's level, then Shell's walls and
/// Rooms' door on them. Accept all commits Lead, Shell, Rooms.
async fn drawn_house(app: &AppService) -> (Started, Vec<Id>) {
    let st = start(app, house_plan()).await;
    let shell = app.agent_call(&hint("s-shell", Some(&handle_of_part(&st, "shell"))), "add_walls", CallKind::Edit).unwrap();
    let rooms = app.agent_call(&hint("s-rooms", Some(&handle_of_part(&st, "rooms"))), "add_opening", CallKind::Edit).unwrap();
    let first = app.agent_commit(&shell, chain(&[(0.0, 0.0), (6000.0, 0.0), (6000.0, 4000.0), (0.0, 4000.0)], true)).await.unwrap();
    let walls = walls_of(&first.state.project, &first.diff.added);
    app.agent_commit(&rooms, opening(&walls[0], OpeningType::Door)).await.unwrap();
    (st, walls)
}

/// Gate P1-2: Accept all keeps the draft open until every commit is in. A
/// commit that fails (here a test refuses the second one) leaves what went
/// in in the model, and every other step in the draft, still acceptable.
#[tokio::test]
async fn a_failed_commit_keeps_the_draft_and_what_did_not_go_in() {
    let (app, _dir) = app().await;
    let (before, revision) = main_doc(&app).await;
    let (st, walls) = drawn_house(&app).await;
    let view = app.swarm_draft(None).await.unwrap().unwrap().state.project;
    app.swarm.lock().as_mut().unwrap().fail_commit = Some(1);
    let e = app.swarm_accept_all(&st.started.swarm.id, None).await.unwrap_err();
    assert_eq!(e.message, "A test refused this commit.");
    // Lead's setup level went in; Shell's and Rooms' steps did not, and stay.
    let (main, now) = main_doc(&app).await;
    assert_eq!((now, main.levels.len(), main.elements.len()), (revision + 1, before.levels.len() + 1, before.elements.len()));
    let status = app.swarm_status().expect("the draft is still open");
    let part = |key: &str| status.parts.iter().find(|p| p.key == key).unwrap().clone();
    assert_eq!((part("shell").state, part("shell").steps), (SwarmPartState::Working, 1));
    assert_eq!((part("rooms").state, part("rooms").steps), (SwarmPartState::Working, 1));
    assert_eq!(status.main_revision, now, "the draft sits on the model as it is");
    {
        let g = app.swarm.lock();
        let d = g.as_ref().unwrap();
        assert!(!d.committing && d.holding.is_empty(), "nothing held after the accept");
    }
    // The model is locked again, and the next Accept all brings the rest in.
    let wall_cmd = wall((0.0, 9000.0), (4000.0, 9000.0), None);
    assert_eq!(app.handle("doc_apply", json!({"command": wall_cmd})).await.unwrap_err().code, "swarm_running");
    let done = app.swarm_accept_all(&st.started.swarm.id, None).await.unwrap();
    assert_eq!(done.applied.iter().map(|a| a.part.as_str()).collect::<Vec<_>>(), ["shell", "rooms"]);
    let after = main_doc(&app).await.0;
    assert_eq!(after.elements, view.elements, "the whole draft, with its ids");
    assert!(walls.iter().all(|w| after.elements.iter().any(|e| e.id() == w)));
    assert!(app.swarm_status().is_none());
}

/// Gate P1-2: an accept checks the whole model it planned on, not only its
/// revision, before it commits. A change without a revision (a rename made
/// in the engine, past the `hub_rename` guard) makes it plan again; it never
/// reaches `apply_replayed`'s refusal and never loses the draft.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_accept_checks_the_whole_model_it_planned_on() {
    use std::sync::atomic::{AtomicBool, Ordering};
    for trial in 0..3 {
        let (app, _dir) = app().await;
        let st = start(&app, house_plan()).await;
        let shell = app.agent_call(&hint("s-shell", Some(&handle_of_part(&st, "shell"))), "add_walls", CallKind::Edit).unwrap();
        for i in 0..60 {
            let x = i as f64 * 2500.0;
            app.agents.advance_clock(std::time::Duration::from_millis(300));
            app.agent_commit(&shell, wall((x, 0.0), (x + 1500.0, 0.0), None)).await.unwrap();
        }
        let stop = Arc::new(AtomicBool::new(false));
        let renamer = {
            let (app, stop) = (app.clone(), stop.clone());
            tokio::spawn(async move {
                let mut n = 0u32;
                while !stop.load(Ordering::SeqCst) {
                    n += 1;
                    if let Some(doc) = app.session.lock().await.doc.as_mut() {
                        doc.rename(&format!("Race {n}")).unwrap();
                    }
                    tokio::task::yield_now().await;
                }
            })
        };
        let result = app.swarm_accept_all(&st.started.swarm.id, None).await;
        stop.store(true, Ordering::SeqCst);
        renamer.await.unwrap();
        match result {
            Ok(done) => assert_eq!(done.applied.len(), 2, "trial {trial}"),
            Err(e) => {
                assert!(!e.message.contains("after the replay was worked out"), "trial {trial}: {e:?}");
                assert!(app.swarm_status().is_some(), "trial {trial}: the draft is kept");
                app.swarm_accept_all(&st.started.swarm.id, None).await.unwrap();
            }
        }
        let main = main_doc(&app).await.0;
        assert_eq!(main.elements.iter().filter(|e| matches!(e, Element::Wall(_))).count(), 60, "trial {trial}");
    }
}

/// Gate P1-2: renaming the open project is refused while a draft is open;
/// it renames again once the draft is gone.
#[tokio::test]
async fn renaming_the_open_project_waits_for_the_draft() {
    let (app, _dir) = app().await;
    let (main, _) = main_doc(&app).await;
    let st = start(&app, house_plan()).await;
    let e = app.handle("hub_rename", json!({"id": main.id, "name": "Renamed"})).await.unwrap_err();
    assert_eq!(e.code, "swarm_running");
    assert_eq!(main_doc(&app).await.0.name, main.name);
    app.swarm_discard(Some(&st.started.swarm.id)).await.unwrap();
    app.handle("hub_rename", json!({"id": main.id, "name": "Renamed"})).await.unwrap();
    assert_eq!(main_doc(&app).await.0.name, "Renamed");
}

/// Gate P2: at most 8 `swarm` events a second, in order, the newest always
/// last, the end of the swarm too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn swarm_events_go_out_at_most_eight_a_second_and_the_last_one_always() {
    use std::time::{Duration, Instant};
    let (app, _dir) = app().await;
    let st = start(&app, house_plan()).await;
    tokio::time::sleep(EVENT_GAP * 2).await;
    let mut events = app.events();
    let status = |n: u32| {
        let mut s = st.started.swarm.clone();
        s.draft_revision = 1000 + n;
        AppEvent::Swarm { swarm: Some(s) }
    };
    // A burst of 50, then one every 5 ms for 400 ms.
    let started = Instant::now();
    for n in 0..50 {
        app.emit(status(n));
    }
    let mut n = 50;
    while started.elapsed() < Duration::from_millis(400) {
        app.emit(status(n));
        n += 1;
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let last = 1000 + n - 1;
    let sent_for = started.elapsed();
    tokio::time::sleep(EVENT_GAP * 2).await;
    let mut got = vec![];
    while let Ok(event) = events.try_recv() {
        if let AppEvent::Swarm { swarm: Some(s) } = event {
            got.push(s.draft_revision);
        }
    }
    let most = (sent_for.as_millis() / EVENT_GAP.as_millis()) as usize + 2;
    assert!(got.len() <= most, "{} events for {n} changes in {sent_for:?}, at most {most}", got.len());
    assert_eq!(got.last(), Some(&last), "the newest always goes out");
    assert!(got.windows(2).all(|w| w[0] < w[1]), "in order: {got:?}");
    app.swarm_discard(Some(&st.started.swarm.id)).await.unwrap();
    let ended = tokio::time::timeout(EVENT_GAP * 3, async {
        loop {
            if let Ok(AppEvent::Swarm { swarm: None }) = events.recv().await {
                return;
            }
        }
    })
    .await;
    assert!(ended.is_ok(), "the end of the swarm went out");
}

/// Gate P2: a discard that races an accept waits for its commits (the
/// accept holds the session lock), then answers from what is left: it
/// never says "discarded" for parts that went into the model.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_discard_waits_for_an_accept_and_answers_from_what_is_left() {
    use std::time::Duration;
    let (one, _dir) = app().await;
    let (st, _walls) = drawn_house(&one).await;
    let id = st.started.swarm.id.clone();
    // An accept committing: the session lock is held, and it ends the swarm.
    let held = one.session.lock().await;
    let discard = {
        let (one, id) = (one.clone(), id.clone());
        tokio::spawn(async move { one.swarm_discard(Some(&id)).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!discard.is_finished(), "the discard waits for the accept");
    assert!(one.swarm_status().is_some());
    *one.swarm.lock() = None;
    drop(held);
    let e = discard.await.unwrap().unwrap_err();
    assert_eq!(e.code, "not_found", "the accept ended the swarm: {e:?}");

    // A real race: one of the two wins, and the other says so.
    for trial in 0..4u64 {
        let (app, _dir) = app().await;
        let (st, walls) = drawn_house(&app).await;
        let id = st.started.swarm.id.clone();
        let accept = {
            let (app, id) = (app.clone(), id.clone());
            tokio::spawn(async move { app.swarm_accept_all(&id, None).await })
        };
        tokio::time::sleep(Duration::from_millis(trial)).await;
        let discarded = app.swarm_discard(Some(&id)).await;
        let accepted = accept.await.unwrap();
        let model = main_doc(&app).await.0;
        let in_model = walls.iter().filter(|w| model.elements.iter().any(|e| e.id() == *w)).count();
        match (&accepted, &discarded) {
            (Ok(_), Err(e)) => assert_eq!((e.code.as_str(), in_model), ("not_found", walls.len()), "trial {trial}"),
            (Err(_), Ok(())) => assert_eq!(in_model, 0, "trial {trial}"),
            other => panic!("trial {trial}: accept and discard both answered {other:?}"),
        }
        assert!(app.swarm_status().is_none(), "trial {trial}");
    }
}

/// Gate P2: a window that has the draft's revision gets the status back,
/// not the whole view.
#[tokio::test]
async fn swarm_draft_answers_only_the_status_to_a_window_that_is_current() {
    let (app, _dir) = app().await;
    let (st, _walls) = drawn_house(&app).await;
    let status = app.swarm_status().unwrap();
    let args = json!({"since_step": 3, "draft_revision": status.draft_revision, "swarm_id": status.id});
    let current = app.handle("swarm_draft", args).await.unwrap();
    let full = app.handle("swarm_draft", json!({"since_step": 3})).await.unwrap();
    match serde_json::from_value::<SwarmDraftReply>(current.clone()).unwrap() {
        SwarmDraftReply::Current(c) => {
            assert_eq!((c.swarm_id, c.draft_revision, c.status), (st.started.swarm.id.clone(), status.draft_revision, status.clone()))
        }
        SwarmDraftReply::Full(_) => panic!("the window is current"),
    }
    let (small, big) = (current.to_string().len(), full.to_string().len());
    assert!(small * 2 < big, "{small} B against {big} B");
    // A moved draft, or another swarm's revision, gets the whole draft.
    let older = json!({"since_step": 0, "draft_revision": status.draft_revision - 1});
    let reply: SwarmDraftReply = serde_json::from_value(app.handle("swarm_draft", older).await.unwrap()).unwrap();
    assert!(matches!(reply, SwarmDraftReply::Full(ref d) if d.draft_revision == status.draft_revision && d.steps.len() == 3));
    let other = json!({"draft_revision": status.draft_revision, "swarm_id": "another-swarm"});
    let reply: SwarmDraftReply = serde_json::from_value(app.handle("swarm_draft", other).await.unwrap()).unwrap();
    assert!(matches!(reply, SwarmDraftReply::Full(_)));
}
