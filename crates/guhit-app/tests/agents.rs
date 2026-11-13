//! Agents as participants (DECISIONS D42, D47, docs/CONTRACT.md "Agents"):
//! several MCP agents on one computer, each with its own name, presence and
//! history steps; pause, stop and resume; level and kind limits; claims;
//! undo of their own steps; the automatic version; and agents in a live
//! session, with older peers that do not know them.
//!
//! The registry is driven the way the MCP server drives it: an `AgentHint`
//! per call, `agent_call`, then `agent_commit` or `agent_undo`.

use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use guhit_app::agents::{AgentCall, AgentHint, CallKind};
use guhit_app::live::invite::Invite;
use guhit_app::live::{tls, wire};
use guhit_app::AppService;
use guhit_model::*;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::broadcast;

const WAIT: Duration = Duration::from_secs(10);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("agents-{tag}-{}", defaults::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Peer {
    app: AppService,
    events: broadcast::Receiver<AppEvent>,
    dir: TempDir,
}

/// A computer named `name`, the relay off, with a project open.
async fn computer(name: &str) -> Peer {
    let peer = peer(name).await;
    let _: DocState = call(&peer.app, "hub_create", json!({ "name": "Bahay" })).await;
    peer
}

async fn peer(name: &str) -> Peer {
    let dir = TempDir::new(name);
    std::fs::write(dir.0.join("settings.json"), json!({ "live_relay": "" }).to_string()).unwrap();
    let app = AppService::new_sandboxed(dir.0.clone());
    let events = app.events();
    let _: Profile = call(&app, "profile_set", json!({ "name": name })).await;
    Peer { app, events, dir }
}

async fn call<T: DeserializeOwned>(app: &AppService, cmd: &str, args: Value) -> T {
    let v = app.handle(cmd, args).await.unwrap_or_else(|e| panic!("{cmd} failed: {e}"));
    serde_json::from_value(v).unwrap_or_else(|e| panic!("{cmd} returned an unexpected shape: {e}"))
}

async fn fail(app: &AppService, cmd: &str, args: Value) -> IpcError {
    match app.handle(cmd, args).await {
        Ok(v) => panic!("{cmd} should have failed, got {v}"),
        Err(e) => e,
    }
}

/// The agent `handle` of `client`, each handle in its own MCP session, as
/// separate agents are (the server's session id is the connection).
fn hint(agent: Option<&str>, client: &str) -> AgentHint {
    let session = agent.map(|h| format!("session-{}", h.to_lowercase().replace(['_', ' '], "-")));
    AgentHint {
        connection: session,
        agent: agent.map(str::to_string),
        client_name: Some(client.to_string()),
        client_version: Some("1.0.0".into()),
        ..AgentHint::default()
    }
}

/// The agent `handle` on the connection `connection`: subagents of one
/// session share it.
fn on(app: &AppService, connection: &str, handle: &str, kind: CallKind) -> Result<AgentCall, IpcError> {
    let hint = AgentHint { connection: Some(connection.into()), ..hint(Some(handle), "claude-code") };
    app.agent_call(&hint, "add_wall", kind)
}

/// An editing call of the agent `handle` of Claude Code.
fn agent(app: &AppService, handle: &str) -> AgentCall {
    app.agent_call(&hint(Some(handle), "claude-code"), "add_wall", CallKind::Edit)
        .unwrap_or_else(|e| panic!("{handle}: {e}"))
}

fn p(x: f64, y: f64) -> Point {
    Point { x, y }
}

fn wall_on(level: Option<&str>, x0: f64, y0: f64, x1: f64, y1: f64) -> Command {
    Command::AddWall {
        start: p(x0, y0),
        end: p(x1, y1),
        thickness_mm: None,
        height_mm: None,
        material_id: None,
        level_id: level.map(str::to_string),
        role: None,
    }
}

fn wall(x0: f64, y0: f64, x1: f64, y1: f64) -> Command {
    wall_on(None, x0, y0, x1, y1)
}

async fn doc(app: &AppService) -> DocState {
    let state: Option<DocState> = call(app, "doc_state", json!({})).await;
    state.expect("a project is open")
}

async fn history(app: &AppService) -> HistoryList {
    call(app, "history_list", json!({})).await
}

fn list(app: &AppService) -> Vec<AgentSummary> {
    app.agents_list()
}

fn find<'a>(list: &'a [AgentSummary], id: &str) -> &'a AgentSummary {
    list.iter().find(|a| a.participant.id == id).unwrap_or_else(|| panic!("agent {id} in {list:?}"))
}

/// The project with a second level, "Second Floor": its levels' ids.
async fn two_levels(app: &AppService) -> (Id, Id) {
    let add = Command::AddLevel { name: Some("Second Floor".into()), elevation_mm: None, height_mm: None };
    let _: ApplyResult = call(app, "doc_apply", json!({ "command": add })).await;
    let state = doc(app).await;
    (state.project.levels[0].id.clone(), state.project.levels[1].id.clone())
}

fn last_wall(state: &DocState) -> Id {
    state.project.elements.iter().rev().find(|e| matches!(e, Element::Wall(_))).map(|e| e.id().clone()).unwrap()
}

async fn until<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = Instant::now() + WAIT;
    while !check().await {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

// --------------------------------------------------------- one computer

#[tokio::test]
async fn two_agents_on_one_computer_are_two_participants() {
    let mut axl = computer("Axl").await;
    let ground = agent(&axl.app, "ground-floor");
    // The same handle, however it is written, is the same agent.
    assert_eq!(agent(&axl.app, "ground_floor").id, ground.id);
    let upper = agent(&axl.app, "Upper Floor");
    assert_ne!(ground.id, upper.id);
    assert_eq!((ground.name.as_str(), upper.name.as_str()), ("Ground floor", "Upper floor"));

    axl.app.agent_commit(&ground, wall(0.0, 0.0, 4000.0, 0.0)).await.unwrap();
    let applied = axl.app.agent_commit(&upper, wall(0.0, 3000.0, 4000.0, 3000.0)).await.unwrap();
    assert_eq!(applied.state.undo_by.as_deref(), Some(upper.id.as_str()));

    // History names each agent and the person it worked for.
    let steps = history(&axl.app).await.undo;
    assert_eq!(steps[0].author_id.as_deref(), Some(upper.id.as_str()));
    assert_eq!(steps[0].author_name.as_deref(), Some("Upper floor"));
    assert_eq!(steps[1].author_id.as_deref(), Some(ground.id.as_str()));
    assert_eq!(steps[0].on_behalf_name.as_deref(), Some("Axl"));
    assert!(steps[0].on_behalf_of.is_some(), "an agent step is always marked, session or not");
    assert_ne!(steps[0].color, steps[1].color);

    // Presence: the pointer on what it touched, its work area around it.
    let agents = list(&axl.app);
    assert_eq!(agents.len(), 2);
    assert_eq!(agents[0].participant.id, upper.id, "newest call first");
    let up = find(&agents, &upper.id);
    let made = last_wall(&applied.state);
    assert_eq!(up.presence.cursor, Some(p(2000.0, 3000.0)));
    assert_eq!(up.presence.level_id.as_deref(), Some(applied.state.project.levels[0].id.as_str()));
    assert_eq!(up.presence.selection, vec![made]);
    let area = up.presence.plan_view.unwrap();
    assert!(area.width_mm >= 8000.0 && area.height_mm >= 6000.0, "{area:?}");
    assert!(up.presence.pointer_3d.is_some());
    assert_eq!(up.presence.status.as_deref(), Some("Drawing walls"));
    assert_eq!((up.activity, up.steps), (AgentActivity::Working, 1));
    let info = up.participant.agent.as_ref().unwrap();
    assert_eq!((info.client.as_str(), info.owner_name.as_str(), info.owner_id.clone()), ("claude-code", "Axl", None));
    assert_eq!(up.participant.role, ParticipantRole::Guest, "an agent is never the host");

    // The window heard of it, and the MCP status lists them.
    let mut heard = None;
    while let Ok(e) = axl.events.try_recv() {
        if let AppEvent::Agents { agents } = e {
            heard = Some(agents);
        }
    }
    assert_eq!(heard.unwrap().len(), 2);
    let status: McpStatus = call(&axl.app, "mcp_status", json!({})).await;
    assert_eq!(status.agents.len(), 2);
    let listed: Vec<AgentSummary> = call(&axl.app, "agents_list", json!({})).await;
    assert_eq!(listed.len(), 2);

    // Two Codex windows that name no handle: their stdio instances tell
    // them apart, and the second is numbered.
    let codex = |instance: &str| AgentHint { instance: Some(instance.into()), ..hint(None, "codex") };
    let one = axl.app.agent_call(&codex("p1"), "list_rooms", CallKind::Read).unwrap();
    let two = axl.app.agent_call(&codex("p2"), "list_rooms", CallKind::Read).unwrap();
    assert_eq!((one.name.as_str(), two.name.as_str()), ("Codex", "Codex 2"));
    let looking = find(&list(&axl.app), &one.id).activity;
    assert_eq!(looking, AgentActivity::Looking);
}

#[tokio::test]
async fn agents_go_idle_then_off_the_list_and_stay_in_the_history() {
    let axl = computer("Axl").await;
    let ground = agent(&axl.app, "ground-floor");
    axl.app.agent_commit(&ground, wall(0.0, 0.0, 4000.0, 0.0)).await.unwrap();
    axl.app.agents.advance_clock(Duration::from_secs(61));
    let a = find(&list(&axl.app), &ground.id).clone();
    assert_eq!(a.activity, AgentActivity::Idle);
    assert_eq!(a.presence.status, None, "an idle agent's tool line goes");
    axl.app.agents.advance_clock(Duration::from_secs(10 * 60));
    assert!(list(&axl.app).is_empty());
    assert_eq!(history(&axl.app).await.undo[0].author_name.as_deref(), Some("Ground floor"));
}

#[tokio::test]
async fn an_idle_agent_of_another_project_leaves_the_list() {
    // GS F7: the panel listed "Probe", which had worked on a project since
    // deleted.
    let axl = computer("Axl").await;
    let a = doc(&axl.app).await.project.id;
    let probe = agent(&axl.app, "probe");
    axl.app.agent_commit(&probe, wall(0.0, 0.0, 4000.0, 0.0)).await.unwrap();
    let _: DocState = call(&axl.app, "hub_create", json!({ "name": "B" })).await;
    let _: Value = call(&axl.app, "hub_delete", json!({ "id": a })).await;
    let here = agent(&axl.app, "here");
    // Still working a moment ago: it shows, so the user sees who is busy.
    find(&list(&axl.app), &probe.id);
    axl.app.agents.advance_clock(Duration::from_secs(61));
    let agents = list(&axl.app);
    assert!(!agents.iter().any(|s| s.participant.id == probe.id), "{agents:?}");
    assert_eq!(find(&agents, &here.id).activity, AgentActivity::Idle, "this project's idle agent stays");
    // It comes back with its next call.
    let again = axl.app.agent_call(&hint(Some("probe"), "claude-code"), "list_rooms", CallKind::Read).unwrap();
    assert_eq!(again.id, probe.id);
    find(&list(&axl.app), &probe.id);
}

#[tokio::test]
async fn at_most_eight_agents_work_on_one_computer() {
    let axl = computer("Axl").await;
    for i in 0..8 {
        agent(&axl.app, &format!("helper-{i}"));
    }
    let e = axl.app.agent_call(&hint(Some("one-more"), "codex"), "add_wall", CallKind::Edit).unwrap_err();
    assert_eq!(e.code, "too_many_agents");
    assert!(e.message.contains("stop one"), "{}", e.message);
    // One that is already on the list still gets in.
    agent(&axl.app, "helper-3");
}

#[tokio::test]
async fn a_rename_is_kept_in_the_settings() {
    let axl = computer("Axl").await;
    let ground = agent(&axl.app, "ground-floor");
    let renamed: AgentSummary =
        call(&axl.app, "agent_update", json!({ "id": ground.id, "name": "  Ground team " })).await;
    assert_eq!(renamed.participant.name, "Ground team");
    let e = fail(&axl.app, "agent_update", json!({ "id": ground.id, "name": " " })).await;
    assert_eq!(e.code, "bad_args");
    let settings: Value =
        serde_json::from_str(&std::fs::read_to_string(axl.dir.0.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["agent_names"]["agent:ground-floor"], "Ground team");

    // The next run of the app finds the name again.
    let again = AppService::new_sandboxed(axl.dir.0.clone());
    let back = again.agent_call(&hint(Some("ground-floor"), "claude-code"), "list_rooms", CallKind::Read).unwrap();
    assert_eq!(back.name, "Ground team");
}

#[tokio::test]
async fn pause_stop_and_resume() {
    let axl = computer("Axl").await;
    let ground = agent(&axl.app, "ground-floor");
    let upper = agent(&axl.app, "upper-floor");

    let after: Vec<AgentSummary> = call(&axl.app, "agent_pause", json!({ "id": ground.id })).await;
    assert_eq!(find(&after, &ground.id).activity, AgentActivity::Paused);
    let e = axl.app.agent_call(&hint(Some("ground-floor"), "claude-code"), "add_wall", CallKind::Edit).unwrap_err();
    assert_eq!(e.code, "agent_paused");
    assert_eq!(
        e.message,
        "Axl paused Ground floor. Stop and tell the user; do not retry until they resume you. Reading the plan still works."
    );
    // Reads still work, and the other agent is not held up.
    axl.app.agent_call(&hint(Some("ground-floor"), "claude-code"), "list_rooms", CallKind::Read).unwrap();
    axl.app.agent_commit(&upper, wall(0.0, 0.0, 1000.0, 0.0)).await.unwrap();
    // A pause that lands while a call is under way stops its commit.
    let e = axl.app.agent_commit(&ground, wall(0.0, 1000.0, 1000.0, 1000.0)).await.unwrap_err();
    assert_eq!(e.code, "agent_paused");

    let after: Vec<AgentSummary> = call(&axl.app, "agent_stop", json!({ "id": null })).await;
    assert!(after.iter().all(|a| a.activity == AgentActivity::Stopped), "{after:?}");
    let e = axl.app.agent_call(&hint(Some("upper-floor"), "claude-code"), "list_rooms", CallKind::Read).unwrap_err();
    assert_eq!(e.code, "agent_stopped");
    assert!(e.message.starts_with("Axl stopped Upper floor"), "{}", e.message);

    let after: Vec<AgentSummary> = call(&axl.app, "agent_resume", json!({ "id": ground.id })).await;
    assert_eq!(find(&after, &ground.id).activity, AgentActivity::Looking);
    assert_eq!(find(&after, &upper.id).activity, AgentActivity::Stopped);
    agent(&axl.app, "ground-floor");
    let _: Vec<AgentSummary> = call(&axl.app, "agent_resume", json!({ "id": null })).await;
    axl.app.agent_commit(&agent(&axl.app, "upper-floor"), wall(0.0, 2000.0, 1000.0, 2000.0)).await.unwrap();

    let e = fail(&axl.app, "agent_pause", json!({ "id": "agent-nobody" })).await;
    assert_eq!(e.code, "not_found");
}

#[tokio::test]
async fn a_level_limit_and_a_kind_limit_refuse_with_clear_messages() {
    let axl = computer("Axl").await;
    let (ground_level, second) = two_levels(&axl.app).await;
    let ground = agent(&axl.app, "ground-floor");
    let services = agent(&axl.app, "services");
    let limit = json!({ "level_ids": [ground_level], "ids": [], "kinds": [] });
    let limited: AgentSummary = call(&axl.app, "agent_update", json!({ "id": ground.id, "limit": limit })).await;
    assert_eq!(limited.limit.as_ref().unwrap().level_ids, vec![ground_level.clone()]);

    let e = axl.app.agent_commit(&ground, wall_on(Some(&second), 0.0, 0.0, 3000.0, 0.0)).await.unwrap_err();
    assert_eq!(e.code, "out_of_scope", "{e:?}");
    assert!(
        e.message.starts_with("The new wall would be on Second Floor, outside what this agent is limited to."),
        "{}",
        e.message
    );
    assert!(e.message.contains("Axl limited Ground floor to Ground Floor"), "{}", e.message);
    let on_ground = axl.app.agent_commit(&ground, wall_on(Some(&ground_level), 0.0, 0.0, 3000.0, 0.0)).await.unwrap();
    let ground_wall = last_wall(&on_ground.state);
    // The roof over the top floor stands on the top floor, outside this
    // agent's level (no level has a building yet, so the highest level).
    let roof = Roof { kind: RoofKind::Hip, ..on_ground.state.project.top_roof() };
    let e = axl.app.agent_commit(&ground, Command::SetRoof { roof }).await.unwrap_err();
    assert_eq!(e.code, "out_of_scope", "{e:?}");
    assert!(
        e.message.starts_with("The new roof over the top floor would be on Second Floor, outside what this agent is limited to."),
        "{}",
        e.message
    );
    let settings = Command::SetProjectSettings { settings: on_ground.state.project.settings.clone() };
    let e = axl.app.agent_commit(&ground, settings).await.unwrap_err();
    assert!(e.message.starts_with("Changing the project settings affects the whole project"), "{}", e.message);
    let add = Command::AddLevel { name: None, elevation_mm: None, height_mm: None };
    assert_eq!(axl.app.agent_commit(&ground, add).await.unwrap_err().code, "out_of_scope");

    // A wall the person drew on the second floor is out of its reach too.
    let theirs: ApplyResult =
        call(&axl.app, "doc_apply", json!({ "command": wall_on(Some(&second), 0.0, 5000.0, 2000.0, 5000.0) })).await;
    let theirs = last_wall(&theirs.state);
    let away = Command::MoveElements { ids: vec![theirs.clone()], delta: p(100.0, 0.0), stretch_connected: false };
    let e = axl.app.agent_commit(&ground, away).await.unwrap_err();
    assert!(e.message.starts_with("Wall 2000 mm is on Second Floor"), "{}", e.message);
    assert_eq!(e.element_ids, vec![theirs.clone()]);

    // Services may only place doors and windows.
    let limit = json!({ "level_ids": [], "ids": [], "kinds": ["opening"] });
    let _: AgentSummary = call(&axl.app, "agent_update", json!({ "id": services.id, "limit": limit })).await;
    let e = axl.app.agent_commit(&services, wall(0.0, 8000.0, 2000.0, 8000.0)).await.unwrap_err();
    assert!(e.message.starts_with("The new wall is a wall, outside what this agent is limited to."), "{}", e.message);
    assert!(e.message.contains("Axl limited Services to doors and windows"), "{}", e.message);
    let shift = Command::MoveElements { ids: vec![ground_wall.clone()], delta: p(0.0, 100.0), stretch_connected: false };
    let e = axl.app.agent_commit(&services, shift).await.unwrap_err();
    assert!(e.message.starts_with("Wall 3000 mm is a wall,"), "{}", e.message);
    let door = Command::AddOpening {
        wall_id: ground_wall.clone(),
        opening_type: OpeningType::Door,
        offset_mm: 1500.0,
        width_mm: None,
        height_mm: None,
        sill_mm: None,
        style: None,
        flip_side: None,
        flip_hinge: None,
        type_id: None,
    };
    axl.app.agent_commit(&services, door).await.unwrap();

    // A batch is checked step by step: a wall it makes is still a wall.
    let batch = Command::Batch { label: "MCP: batch".into(), commands: vec![wall(0.0, 9000.0, 1000.0, 9000.0)] };
    assert_eq!(axl.app.agent_commit(&services, batch).await.unwrap_err().code, "out_of_scope");

    // Cleared with null; the person is never limited.
    let _: AgentSummary = call(&axl.app, "agent_update", json!({ "id": ground.id, "limit": null })).await;
    axl.app.agent_commit(&agent(&axl.app, "ground-floor"), wall_on(Some(&second), 0.0, 0.0, 1000.0, 0.0)).await.unwrap();
    let e = fail(&axl.app, "agent_update", json!({ "id": ground.id, "limit": { "level_ids": ["nope"], "ids": [], "kinds": [] } })).await;
    assert_eq!(e.code, "bad_args");
}

#[tokio::test]
async fn a_claim_keeps_other_agents_out_but_never_people() {
    let axl = computer("Axl").await;
    let (_, second) = two_levels(&axl.app).await;
    let ground = agent(&axl.app, "ground-floor");
    let upper = agent(&axl.app, "upper-floor");

    let claim = axl.app.agent_claim(&upper, vec!["second floor".into()], vec![]).await.unwrap().unwrap();
    assert_eq!(claim.level_ids, vec![second.clone()]);
    assert!(!claim.expires_at.is_empty());
    assert_eq!(find(&list(&axl.app), &upper.id).claim.as_ref().unwrap().level_ids, vec![second.clone()]);

    let e = axl.app.agent_commit(&ground, wall_on(Some(&second), 0.0, 0.0, 3000.0, 0.0)).await.unwrap_err();
    assert_eq!(e.code, "claimed");
    assert!(e.message.starts_with("Second Floor is claimed by Upper floor (Claude Code) until "), "{}", e.message);
    assert!(e.message.contains("ask Upper floor in the chat to release it"), "{}", e.message);
    let e = axl.app.agent_claim(&ground, vec![second.clone()], vec![]).await.unwrap_err();
    assert_eq!(e.code, "claimed");

    // People are never blocked; the claimer itself works on.
    let _: ApplyResult =
        call(&axl.app, "doc_apply", json!({ "command": wall_on(Some(&second), 0.0, 5000.0, 2000.0, 5000.0) })).await;
    axl.app.agent_commit(&upper, wall_on(Some(&second), 0.0, 1000.0, 3000.0, 1000.0)).await.unwrap();
    // The ground floor is free.
    axl.app.agent_commit(&ground, wall(0.0, 0.0, 3000.0, 0.0)).await.unwrap();

    // Released, the level is open again.
    axl.app.agent_release(&upper);
    axl.app.agent_commit(&ground, wall_on(Some(&second), 0.0, 2000.0, 3000.0, 2000.0)).await.unwrap();

    // A lease ends 10 minutes after the claimer's last call.
    axl.app.agent_claim(&upper, vec![second.clone()], vec![]).await.unwrap();
    assert_eq!(axl.app.agent_commit(&ground, wall_on(Some(&second), 0.0, 3000.0, 3000.0, 3000.0)).await.unwrap_err().code, "claimed");
    axl.app.agents.advance_clock(Duration::from_secs(10 * 60 + 1));
    axl.app.agent_commit(&agent(&axl.app, "ground-floor"), wall_on(Some(&second), 0.0, 3000.0, 3000.0, 3000.0)).await.unwrap();

    let e = axl.app.agent_claim(&ground, vec!["Attic".into()], vec![]).await.unwrap_err();
    assert_eq!(e.code, "not_found");
}

#[tokio::test]
async fn an_agent_undoes_only_its_own_step() {
    let axl = computer("Axl").await;
    let ground = agent(&axl.app, "ground-floor");
    let upper = agent(&axl.app, "upper-floor");
    axl.app.agent_commit(&ground, wall(0.0, 0.0, 4000.0, 0.0)).await.unwrap();
    axl.app.agent_commit(&upper, wall(0.0, 3000.0, 4000.0, 3000.0)).await.unwrap();

    let e = axl.app.agent_undo(&ground, false, false).await.unwrap_err();
    assert_eq!(e.code, "other_author");
    assert!(e.message.starts_with("The last change is Upper floor's, another agent's: Add wall."), "{}", e.message);
    let e = axl.app.agent_undo(&ground, false, true).await.unwrap_err();
    assert_eq!(e.code, "other_author", "force never takes another agent's step");

    let state = axl.app.agent_undo(&upper, false, false).await.unwrap();
    assert_eq!(state.project.elements.len(), 1);
    // Its undone step is its to redo, not the other agent's.
    assert_eq!(axl.app.agent_undo(&ground, true, false).await.unwrap_err().code, "other_author");
    axl.app.agent_undo(&upper, true, false).await.unwrap();
    axl.app.agent_undo(&upper, false, false).await.unwrap();
    axl.app.agent_undo(&ground, false, false).await.unwrap();
    assert!(doc(&axl.app).await.project.elements.is_empty());

    // A person's step is theirs: refused, unless the user confirmed.
    let _: ApplyResult = call(&axl.app, "doc_apply", json!({ "command": wall(0.0, 0.0, 1000.0, 0.0) })).await;
    let e = axl.app.agent_undo(&ground, false, false).await.unwrap_err();
    assert_eq!(e.code, "other_author");
    assert!(e.message.contains("pass force true only after they confirm"), "{}", e.message);
    axl.app.agent_undo(&ground, false, true).await.unwrap();

    // The window undoes an agent's step without asking: every agent here is
    // this computer's.
    axl.app.agent_commit(&ground, wall(0.0, 0.0, 1000.0, 0.0)).await.unwrap();
    let _: DocState = call(&axl.app, "doc_undo", json!({})).await;
}

#[tokio::test]
async fn an_agent_never_undoes_or_redoes_a_change_of_the_signing_architect() {
    // GR1 #7: a person sets the PRC number; an agent's forced undo emptied
    // it, and its forced redo wrote it back.
    let axl = computer("Axl").await;
    let ground = agent(&axl.app, "ground-floor");
    let signatory = Signatory { name: "Arch. Juan Dela Cruz".into(), prc_no: "0123456".into(), ..Signatory::default() };
    let set = Command::SetSignatory { signatory: Box::new(signatory.clone()) };
    let _: ApplyResult = call(&axl.app, "doc_apply", json!({ "command": set })).await;

    let e = axl.app.agent_undo(&ground, false, true).await.unwrap_err();
    assert_eq!(e.code, "forbidden", "{}", e.message);
    assert!(e.message.contains("Ask the user"), "a hint: {}", e.message);
    assert_eq!(doc(&axl.app).await.project.title_block.signatory, signatory, "nothing was undone");

    // The agent's own step on top: undone, but never the one below it.
    axl.app.agent_commit(&ground, wall(0.0, 0.0, 4000.0, 0.0)).await.unwrap();
    axl.app.agent_undo(&ground, false, false).await.unwrap();
    assert_eq!(axl.app.agent_undo(&ground, false, true).await.unwrap_err().code, "forbidden");

    // Undone by the person, the step is never the agent's to redo.
    let _: DocState = call(&axl.app, "doc_undo", json!({})).await;
    assert_eq!(doc(&axl.app).await.project.title_block.signatory, Signatory::default());
    let e = axl.app.agent_undo(&ground, true, true).await.unwrap_err();
    assert_eq!(e.code, "forbidden", "{}", e.message);
    assert_eq!(doc(&axl.app).await.project.title_block.signatory, Signatory::default(), "nothing was redone");
    // The person still can.
    let _: DocState = call(&axl.app, "doc_redo", json!({})).await;
    assert_eq!(doc(&axl.app).await.project.title_block.signatory, signatory);
}

#[tokio::test]
async fn the_first_agent_edit_after_a_quiet_spell_saves_a_version() {
    let axl = computer("Axl").await;
    let versions = |app: AppService| async move {
        let list: Vec<SnapshotMeta> = call(&app, "snapshot_list", json!({})).await;
        list.into_iter().filter(|s| s.label.starts_with("Before agents, ")).collect::<Vec<_>>()
    };
    let _: ApplyResult = call(&axl.app, "doc_apply", json!({ "command": wall(0.0, 0.0, 1000.0, 0.0) })).await;
    assert!(versions(axl.app.clone()).await.is_empty(), "people's edits save none");
    let ground = agent(&axl.app, "ground-floor");
    axl.app.agent_commit(&ground, wall(0.0, 1000.0, 1000.0, 1000.0)).await.unwrap();
    let saved = versions(axl.app.clone()).await;
    assert_eq!(saved.len(), 1);
    assert!(saved[0].label.ends_with(" AM") || saved[0].label.ends_with(" PM"), "{}", saved[0].label);
    assert!(saved[0].auto);
    assert_eq!(saved[0].revision, 1, "the plan before the agent's edit");

    // More edits by any agent while they keep working save no more.
    axl.app.agent_commit(&agent(&axl.app, "upper-floor"), wall(0.0, 2000.0, 1000.0, 2000.0)).await.unwrap();
    axl.app.agent_commit(&ground, wall(0.0, 3000.0, 1000.0, 3000.0)).await.unwrap();
    assert_eq!(versions(axl.app.clone()).await.len(), 1);

    axl.app.agents.advance_clock(Duration::from_secs(10 * 60 + 1));
    axl.app.agent_commit(&agent(&axl.app, "ground-floor"), wall(0.0, 4000.0, 1000.0, 4000.0)).await.unwrap();
    assert_eq!(versions(axl.app.clone()).await.len(), 2);
}

#[tokio::test]
async fn an_agent_commits_at_most_five_times_a_second() {
    let axl = computer("Axl").await;
    let ground = agent(&axl.app, "ground-floor");
    for i in 0..5 {
        axl.app.agent_commit(&ground, wall(0.0, i as f64 * 1000.0, 1000.0, i as f64 * 1000.0)).await.unwrap();
    }
    let e = axl.app.agent_commit(&ground, wall(0.0, 9000.0, 1000.0, 9000.0)).await.unwrap_err();
    assert_eq!(e.code, "rate_limited");
    assert!(e.message.contains("one batch"), "{}", e.message);
    axl.app.agents.advance_clock(Duration::from_millis(1100));
    axl.app.agent_commit(&ground, wall(0.0, 9000.0, 1000.0, 9000.0)).await.unwrap();
}

#[tokio::test]
async fn agents_chat_into_the_project_without_a_session() {
    let axl = computer("Axl").await;
    let ground = agent(&axl.app, "ground-floor");
    let message = axl.app.agent_chat(&ground, " Walls are up. ", None, None).await.unwrap();
    assert_eq!((message.author_id.as_str(), message.author_name.as_str()), (ground.id.as_str(), "Ground floor"));
    assert!(message.via_ai);
    assert_eq!(message.text, "Walls are up.");
    let chat: Vec<ChatMessage> = call(&axl.app, "chat_list", json!({})).await;
    assert_eq!(chat, vec![message]);
    // People still need a session to chat.
    assert_eq!(fail(&axl.app, "chat_send", json!({ "text": "hi" })).await.code, "not_live");
}

#[tokio::test]
async fn a_staged_edit_runs_again_when_the_plan_moved_on() {
    let axl = computer("Axl").await;
    let ground = agent(&axl.app, "ground-floor");
    let runs = Arc::new(Mutex::new(0));
    let counted = runs.clone();
    let app = axl.app.clone();
    let (result, ()) = axl
        .app
        .agent_edit(&ground, move |project: &Project, _: &Derived| -> Result<_, IpcError> {
            *counted.lock().unwrap() += 1;
            // Someone else edits while the first translation runs.
            if *counted.lock().unwrap() == 1 {
                let app = app.clone();
                std::thread::spawn(move || {
                    tokio::runtime::Runtime::new().unwrap().block_on(async {
                        app.commit(wall(0.0, 5000.0, 1000.0, 5000.0), Origin::User).await.unwrap();
                    })
                })
                .join()
                .unwrap();
            }
            let mut staging = guhit_core::Staging::new(project, "MCP: add wall");
            staging.push(&[wall(0.0, 0.0, 1000.0, 0.0)]).map_err(IpcError::from)?;
            Ok((staging, ()))
        })
        .await
        .unwrap();
    assert_eq!(*runs.lock().unwrap(), 2, "translated again under the lock");
    assert_eq!(result.state.project.elements.len(), 2, "both edits are in");
}

// ------------------------------------------------- gate review findings

/// Run `f` on its own runtime in another thread, as another client would.
fn meanwhile(f: impl std::future::Future<Output = ()> + Send + 'static) {
    std::thread::spawn(move || tokio::runtime::Runtime::new().unwrap().block_on(f)).join().unwrap();
}

#[tokio::test]
async fn an_edit_never_lands_on_another_project() {
    let axl = computer("Axl").await;
    let x = doc(&axl.app).await.project.id;
    let y: DocState = call(&axl.app, "hub_create", json!({ "name": "Other" })).await;
    let y = y.project.id;
    let _: DocState = call(&axl.app, "hub_open", json!({ "id": x })).await;
    let ground = agent(&axl.app, "ground-floor");

    // The window opens Y while the agent's batch for X is being staged. Both
    // projects are at revision 0, so the revision alone would not tell.
    let app = axl.app.clone();
    let (switch, y_id) = (Arc::new(Mutex::new(true)), y.clone());
    let e = axl
        .app
        .agent_edit(&ground, move |project: &Project, _: &Derived| -> Result<_, IpcError> {
            if std::mem::take(&mut *switch.lock().unwrap()) {
                let (app, y) = (app.clone(), y_id.clone());
                meanwhile(async move {
                    app.handle("hub_open", json!({ "id": y })).await.unwrap();
                });
            }
            let levels: Vec<Command> = (0..20)
                .map(|i| Command::AddLevel { name: Some(format!("L{i}")), elevation_mm: None, height_mm: None })
                .collect();
            let mut staging = guhit_core::Staging::new(project, "MCP: levels");
            staging.push(&levels).map_err(IpcError::from)?;
            Ok((staging, ()))
        })
        .await
        .unwrap_err();
    assert_eq!(e.code, "project_changed", "{e:?}");
    assert!(e.message.contains("\"Other\""), "{}", e.message);
    let open = doc(&axl.app).await;
    assert_eq!((open.project.id.as_str(), open.project.levels.len()), (y.as_str(), 1), "Y is untouched");

    // A call that started on X is refused once Y is open: one command, undo
    // and a claim alike.
    let _: DocState = call(&axl.app, "hub_open", json!({ "id": x })).await;
    let on_x = agent(&axl.app, "ground-floor");
    axl.app.agent_commit(&on_x, wall(0.0, 0.0, 1000.0, 0.0)).await.unwrap();
    let _: DocState = call(&axl.app, "hub_open", json!({ "id": y })).await;
    assert_eq!(axl.app.agent_commit(&on_x, wall(0.0, 1000.0, 1000.0, 1000.0)).await.unwrap_err().code, "project_changed");
    assert_eq!(axl.app.agent_undo(&on_x, false, false).await.unwrap_err().code, "project_changed");
    assert_eq!(axl.app.agent_claim(&on_x, vec![], vec![]).await.unwrap_err().code, "project_changed");
    assert!(doc(&axl.app).await.project.elements.is_empty());
}

#[tokio::test]
async fn pause_stop_and_limits_hold_for_the_whole_connection() {
    let axl = computer("Axl").await;
    let (ground_level, second) = two_levels(&axl.app).await;

    // Stopped, it cannot come back as a new handle on its connection.
    let a = on(&axl.app, "s1", "ground", CallKind::Edit).unwrap();
    let _: Vec<AgentSummary> = call(&axl.app, "agent_stop", json!({ "id": a.id })).await;
    let e = on(&axl.app, "s1", "someone-else", CallKind::Read).unwrap_err();
    assert_eq!(e.code, "agent_stopped");
    let _: Vec<AgentSummary> = call(&axl.app, "agent_resume", json!({ "id": a.id })).await;
    on(&axl.app, "s1", "someone-else", CallKind::Edit).unwrap();

    // Limited, a new handle on the connection is held to the limit too.
    let b = on(&axl.app, "s2", "ground", CallKind::Edit).unwrap();
    let limit = json!({ "level_ids": [ground_level], "ids": [], "kinds": [] });
    let _: AgentSummary = call(&axl.app, "agent_update", json!({ "id": b.id, "limit": limit })).await;
    let other = on(&axl.app, "s2", "upper", CallKind::Edit).unwrap();
    assert_ne!(other.id, b.id);
    let e = axl.app.agent_commit(&other, wall_on(Some(&second), 0.0, 0.0, 1000.0, 0.0)).await.unwrap_err();
    assert_eq!(e.code, "out_of_scope");
    assert!(find(&list(&axl.app), &other.id).limit.is_some(), "the panel shows it");

    // One connection shares one rate limit, whatever handles it uses.
    let c = on(&axl.app, "s3", "one", CallKind::Edit).unwrap();
    for i in 0..5 {
        axl.app.agent_commit(&c, wall(0.0, i as f64 * 100.0, 500.0, i as f64 * 100.0)).await.unwrap();
    }
    let d = on(&axl.app, "s3", "two", CallKind::Edit).unwrap();
    assert_eq!(axl.app.agent_commit(&d, wall(0.0, 900.0, 500.0, 900.0)).await.unwrap_err().code, "rate_limited");

    // A stdio proxy started again is a new instance: it starts stopped as
    // the one it replaces.
    let desktop = |instance: &str| AgentHint { instance: Some(instance.into()), ..hint(None, "claude-ai") };
    let first = axl.app.agent_call(&desktop("p1"), "list_rooms", CallKind::Read).unwrap();
    let _: Vec<AgentSummary> = call(&axl.app, "agent_stop", json!({ "id": first.id })).await;
    let again = axl.app.agent_call(&desktop("p2"), "list_rooms", CallKind::Read).unwrap_err();
    assert_eq!(again.code, "agent_stopped");

    // Pause all holds for agents that connect afterwards, until resume all.
    let _: Vec<AgentSummary> = call(&axl.app, "agent_pause", json!({ "id": null })).await;
    let cursor = AgentHint { connection: Some("s9".into()), ..hint(Some("newcomer"), "cursor") };
    assert_eq!(axl.app.agent_call(&cursor, "add_wall", CallKind::Edit).unwrap_err().code, "agent_paused");
    axl.app.agent_call(&cursor, "list_rooms", CallKind::Read).unwrap();
    let _: Vec<AgentSummary> = call(&axl.app, "agent_resume", json!({ "id": null })).await;
    axl.app.agent_call(&cursor, "add_wall", CallKind::Edit).unwrap();
    axl.app.agent_call(&desktop("p2"), "list_rooms", CallKind::Read).unwrap();
}

#[tokio::test]
async fn stopping_one_of_eight_agents_frees_its_place() {
    let axl = computer("Axl").await;
    let first = agent(&axl.app, "helper-0");
    for i in 1..8 {
        agent(&axl.app, &format!("helper-{i}"));
    }
    let e = axl.app.agent_call(&hint(Some("ninth"), "codex"), "add_wall", CallKind::Edit).unwrap_err();
    assert!(e.message.contains("frees its place"), "{}", e.message);
    let _: Vec<AgentSummary> = call(&axl.app, "agent_stop", json!({ "id": first.id })).await;
    let ninth = axl.app.agent_call(&hint(Some("ninth"), "codex"), "add_wall", CallKind::Edit).unwrap();
    let agents = list(&axl.app);
    assert_eq!(find(&agents, &first.id).activity, AgentActivity::Stopped, "a stopped agent stays to be resumed");
    assert_eq!(find(&agents, &ninth.id).activity, AgentActivity::Working);
    // Resuming it needs a place again: none is free until another stops.
    let e = fail(&axl.app, "agent_resume", json!({ "id": first.id })).await;
    assert_eq!(e.code, "too_many_agents");
    let _: Vec<AgentSummary> = call(&axl.app, "agent_stop", json!({ "id": ninth.id })).await;
    let _: Vec<AgentSummary> = call(&axl.app, "agent_resume", json!({ "id": first.id })).await;
    agent(&axl.app, "helper-0");
}

#[tokio::test]
async fn a_claim_cannot_take_part_of_another_agents_claim() {
    let axl = computer("Axl").await;
    let (ground_level, second) = two_levels(&axl.app).await;
    let room = Command::AddRectRoom {
        origin: p(0.0, 0.0),
        width_mm: 4000.0,
        depth_mm: 3000.0,
        name: Some("Bedroom".into()),
        thickness_mm: None,
        level_id: Some(ground_level.clone()),
    };
    let state: ApplyResult = call(&axl.app, "doc_apply", json!({ "command": room })).await;
    let bedroom = state.state.project.elements.iter().find(|e| matches!(e, Element::Room(_))).unwrap().id().clone();
    let bedroom_wall = last_wall(&state.state);
    let upper_wall: ApplyResult =
        call(&axl.app, "doc_apply", json!({ "command": wall_on(Some(&second), 0.0, 0.0, 3000.0, 0.0) })).await;
    let upper_wall = last_wall(&upper_wall.state);
    let a = agent(&axl.app, "upper-floor");
    let b = agent(&axl.app, "ground-floor");

    // A holds the second floor: B cannot claim a wall on it.
    axl.app.agent_claim(&a, vec![second.clone()], vec![]).await.unwrap();
    let e = axl.app.agent_claim(&b, vec![], vec![upper_wall.clone()]).await.unwrap_err();
    assert_eq!(e.code, "claimed");
    assert!(e.message.starts_with("Wall 3000 mm is claimed by Upper floor"), "{}", e.message);

    // B holds the bedroom: A cannot claim the ground floor it is on, nor
    // one of the bedroom's walls.
    axl.app.agent_claim(&b, vec![], vec![bedroom.clone()]).await.unwrap();
    let e = axl.app.agent_claim(&a, vec![second.clone(), ground_level.clone()], vec![]).await.unwrap_err();
    assert!(e.message.starts_with("Room Bedroom is claimed by Ground floor"), "{}", e.message);
    let e = axl.app.agent_claim(&a, vec![], vec![bedroom_wall]).await.unwrap_err();
    assert_eq!(e.code, "claimed");
    // Apart, both hold.
    assert!(find(&list(&axl.app), &a.id).claim.is_some());
    assert!(find(&list(&axl.app), &b.id).claim.is_some());
}

// ------------------------------------------------- gate review, round 2

#[tokio::test]
async fn opening_another_project_drops_its_limit_and_claim_and_pauses_it() {
    let axl = computer("Axl").await;
    let a = doc(&axl.app).await.project.id;
    let (ground_level, second) = two_levels(&axl.app).await;
    let ground = agent(&axl.app, "ground-floor");
    let services = agent(&axl.app, "services");
    let limit = json!({ "level_ids": [ground_level], "ids": [], "kinds": ["wall"] });
    let _: AgentSummary = call(&axl.app, "agent_update", json!({ "id": ground.id, "limit": limit })).await;
    let kinds = json!({ "level_ids": [], "ids": [], "kinds": ["opening"] });
    let _: AgentSummary = call(&axl.app, "agent_update", json!({ "id": services.id, "limit": kinds })).await;
    axl.app.agent_claim(&services, vec![second.clone()], vec![]).await.unwrap();

    // Opening A again changes nothing: its levels are still there.
    let _: DocState = call(&axl.app, "hub_open", json!({ "id": a })).await;
    assert_eq!(find(&list(&axl.app), &ground.id).limit.as_ref().unwrap().level_ids.len(), 1);

    // In B, A's levels mean nothing: the level limit goes and the agent waits
    // paused for the user; its kinds stay. A kinds-only limit is untouched.
    let _: DocState = call(&axl.app, "hub_create", json!({ "name": "B" })).await;
    let agents = list(&axl.app);
    let g = find(&agents, &ground.id);
    assert_eq!(g.activity, AgentActivity::Paused);
    assert_eq!(g.limit.as_ref().map(|l| (l.level_ids.len(), l.kinds.clone())), Some((0, vec![ElementKind::Wall])));
    let s = find(&agents, &services.id);
    assert_eq!(s.activity, AgentActivity::Working, "not paused: it had no level limit");
    assert_eq!(s.limit.as_ref().unwrap().kinds, vec![ElementKind::Opening]);
    assert_eq!(s.claim, None, "the claim on A's level ended");
    let e = axl.app.agent_call(&hint(Some("ground-floor"), "claude-code"), "add_wall", CallKind::Edit).unwrap_err();
    assert_eq!(e.code, "agent_paused");
    let _: Vec<AgentSummary> = call(&axl.app, "agent_resume", json!({ "id": ground.id })).await;
    axl.app.agent_commit(&agent(&axl.app, "ground-floor"), wall(0.0, 0.0, 1000.0, 0.0)).await.unwrap();

    // Deleting the one level a limit names pauses the agent the same way.
    let (level_b, upper_b) = two_levels(&axl.app).await;
    let only = json!({ "level_ids": [upper_b], "ids": [], "kinds": [] });
    let _: AgentSummary = call(&axl.app, "agent_update", json!({ "id": services.id, "limit": only })).await;
    let gone = Command::DeleteLevel { level_id: upper_b.clone() };
    let _: ApplyResult = call(&axl.app, "doc_apply", json!({ "command": gone })).await;
    let s = find(&list(&axl.app), &services.id).clone();
    assert_eq!((s.activity, s.limit), (AgentActivity::Paused, None));
    assert!(!level_b.is_empty());
}

#[tokio::test]
async fn a_pause_never_loosens_a_stop() {
    let axl = computer("Axl").await;
    let alpha = agent(&axl.app, "alpha");
    let beta = agent(&axl.app, "beta");
    let _: Vec<AgentSummary> = call(&axl.app, "agent_stop", json!({ "id": alpha.id })).await;
    let after: Vec<AgentSummary> = call(&axl.app, "agent_pause", json!({ "id": null })).await;
    assert_eq!(find(&after, &alpha.id).activity, AgentActivity::Stopped);
    assert_eq!(find(&after, &beta.id).activity, AgentActivity::Paused);
    let after: Vec<AgentSummary> = call(&axl.app, "agent_pause", json!({ "id": alpha.id })).await;
    assert_eq!(find(&after, &alpha.id).activity, AgentActivity::Stopped, "nor does pausing it alone");
    let e = axl.app.agent_call(&hint(Some("alpha"), "claude-code"), "list_rooms", CallKind::Read).unwrap_err();
    assert_eq!(e.code, "agent_stopped", "a stopped agent cannot read");
    axl.app.agent_call(&hint(Some("beta"), "claude-code"), "list_rooms", CallKind::Read).unwrap();
}

#[tokio::test]
async fn agent_steps_are_marked_without_a_session_and_without_a_name() {
    // A computer whose person never picked a name.
    let dir = TempDir::new("noname");
    std::fs::write(dir.0.join("settings.json"), json!({ "live_relay": "" }).to_string()).unwrap();
    let app = AppService::new_sandboxed(dir.0.clone());
    let _: DocState = call(&app, "hub_create", json!({ "name": "Bahay" })).await;
    let gamma = agent(&app, "gamma");
    app.agent_commit(&gamma, wall(0.0, 0.0, 1000.0, 0.0)).await.unwrap();
    let _: ApplyResult = call(&app, "doc_apply", json!({ "command": wall(0.0, 1000.0, 1000.0, 1000.0) })).await;
    let steps = history(&app).await.undo;
    let person = &steps[0];
    assert_eq!((person.author_id.clone(), person.on_behalf_of.clone()), (None, None), "a person's step");
    let theirs = &steps[1];
    assert_eq!(theirs.author_id.as_deref(), Some(gamma.id.as_str()));
    assert_eq!(theirs.author_name.as_deref(), Some("Gamma"));
    assert!(theirs.on_behalf_of.is_some());
    assert_eq!(theirs.on_behalf_name.as_deref(), Some("this computer"));
    // Once named, the name shows.
    let _: Profile = call(&app, "profile_set", json!({ "name": "Axl" })).await;
    agent(&app, "gamma");
    assert_eq!(history(&app).await.undo[1].on_behalf_name.as_deref(), Some("Axl"));
}

#[tokio::test]
async fn notes_tell_an_agent_of_mentions_and_changes_in_its_claim() {
    let axl = computer("Axl").await;
    let (_, second) = two_levels(&axl.app).await;
    let upper = agent(&axl.app, "upper-floor");
    let ground = agent(&axl.app, "ground-floor");
    axl.app.agent_claim(&upper, vec![second.clone()], vec![]).await.unwrap();
    assert_eq!(axl.app.agent_notes(&upper).await.activity, Some(AgentActivity::Working));

    // Between two of Upper floor's calls: a mention, a message for someone
    // else, and a person's wall on its claimed floor and one off it.
    for text in ["@Upper floor the stair hole is 1000 x 2700", "@ground-floor carport next", "@upper-floor and the balcony"] {
        axl.app.agent_chat(&ground, text, None, None).await.unwrap();
    }
    let _: ApplyResult =
        call(&axl.app, "doc_apply", json!({ "command": wall_on(Some(&second), 0.0, 0.0, 3000.0, 0.0) })).await;
    let _: ApplyResult = call(&axl.app, "doc_apply", json!({ "command": wall(0.0, 0.0, 3000.0, 0.0) })).await;
    let next = agent(&axl.app, "upper-floor");
    let notes = axl.app.agent_notes(&next).await;
    let texts: Vec<&str> = notes.inbox.iter().map(|n| n.text.as_str()).collect();
    assert_eq!(texts, vec!["@Upper floor the stair hole is 1000 x 2700", "@upper-floor and the balcony"]);
    assert_eq!(notes.inbox[0].from, "Ground floor");
    assert_eq!(notes.nearby.len(), 1, "{:?}", notes.nearby);
    assert_eq!((notes.nearby[0].who.as_str(), notes.nearby[0].what.as_str()), ("Axl", "Add wall"));
    assert_eq!(notes.activity, Some(AgentActivity::Working));

    // The next call hears nothing old again.
    let again = agent(&axl.app, "upper-floor");
    let notes = axl.app.agent_notes(&again).await;
    assert!(notes.inbox.is_empty() && notes.nearby.is_empty(), "{notes:?}");
    // A paused agent hears it is paused.
    let _: Vec<AgentSummary> = call(&axl.app, "agent_pause", json!({ "id": upper.id })).await;
    assert_eq!(axl.app.agent_notes(&again).await.activity, Some(AgentActivity::Paused));
}

// --------------------------------------------------------- live sessions

async fn hosting(name: &str) -> (Peer, String) {
    let host = computer(name).await;
    let status: LiveStatus = call(&host.app, "live_host", json!({})).await;
    (host, status.invite.unwrap())
}

async fn join(guest: &Peer, invite: &str) -> DocState {
    call(&guest.app, "live_join", json!({ "invite": invite })).await
}

async fn self_id(app: &AppService) -> Id {
    app.live_status().await.self_id.unwrap()
}

#[tokio::test]
async fn an_agent_on_a_guest_shows_to_the_host_and_to_another_guest() {
    let (ana, invite) = hosting("Ana").await;
    let ben = peer("Ben").await;
    let cy = peer("Cy").await;
    join(&ben, &invite).await;
    join(&cy, &invite).await;
    let ben_id = self_id(&ben.app).await;

    // Ben's agent draws through the host.
    let bens = agent(&ben.app, "ground-floor");
    let applied = ben.app.agent_commit(&bens, wall(0.0, 0.0, 4000.0, 0.0)).await.unwrap();
    assert_eq!(applied.state.project.elements.len(), 1);
    let step = history(&ana.app).await.undo[0].clone();
    assert_eq!(step.author_id.as_deref(), Some(bens.id.as_str()), "the host records the agent");
    assert_eq!(step.author_name.as_deref(), Some("Ground floor"));
    assert_eq!((step.on_behalf_of.as_deref(), step.on_behalf_name.as_deref()), (Some(ben_id.as_str()), Some("Ben")));
    // On Ben's own computer it worked for Ben, so for "you".
    let mine = history(&ben.app).await.undo[0].clone();
    assert_eq!((mine.author_id.as_deref(), mine.on_behalf_of.as_deref()), (Some(bens.id.as_str()), Some(ben_id.as_str())));

    // The host and Cy see it, owned by Ben, where it worked.
    until("the host to see Ben's agent", || async {
        ana.app.agents_list().iter().any(|a| a.participant.id == bens.id && a.presence.cursor.is_some())
    })
    .await;
    let seen = find(&ana.app.agents_list(), &bens.id).clone();
    assert_eq!(seen.participant.agent.as_ref().unwrap().owner_id.as_deref(), Some(ben_id.as_str()));
    assert_eq!(seen.presence.cursor, Some(p(2000.0, 0.0)));
    until("Cy to see Ben's agent", || async { cy.app.agents_list().iter().any(|a| a.participant.id == bens.id) }).await;
    assert!(ben.app.agents_list().iter().filter(|a| a.participant.id == bens.id).count() == 1, "Ben sees it once");
    assert!(status_of(&cy.app).await.participants.iter().all(|p| p.agent.is_none()), "agents are not participants");

    // The host's own agent shows to both guests, owned by the host.
    let anas = agent(&ana.app, "upper-floor");
    ana.app.agent_commit(&anas, wall(0.0, 3000.0, 4000.0, 3000.0)).await.unwrap();
    for guest in [&ben, &cy] {
        until("a guest to see the host's agent", || async {
            guest.app.agents_list().iter().any(|a| a.participant.id == anas.id)
        })
        .await;
    }

    // A claim by the host's agent holds Ben's agent off, across computers.
    let (_, second) = {
        let add = Command::AddLevel { name: Some("Second Floor".into()), elevation_mm: None, height_mm: None };
        let state = ana.app.commit(add, Origin::User).await.unwrap().state;
        (state.project.levels[0].id.clone(), state.project.levels[1].id.clone())
    };
    ana.app.agent_claim(&anas, vec![second.clone()], vec![]).await.unwrap();
    until("Ben to see the claim", || async {
        ben.app.agents_list().iter().any(|a| a.participant.id == anas.id && a.claim.is_some())
    })
    .await;
    until("Ben's copy to have the level", || async { doc(&ben.app).await.project.levels.len() == 2 }).await;
    let e = ben.app.agent_commit(&agent(&ben.app, "ground-floor"), wall_on(Some(&second), 0.0, 0.0, 1000.0, 0.0)).await.unwrap_err();
    assert_eq!(e.code, "claimed");
    assert!(e.message.starts_with("Second Floor is claimed by Upper floor (Claude Code)"), "{}", e.message);

    // Ben's agent chats as itself; undoes its own step only.
    let message = ben.app.agent_chat(&bens, "Ground floor walls are in.", None, None).await.unwrap();
    assert_eq!((message.author_id.as_str(), message.author_name.as_str()), (bens.id.as_str(), "Ground floor"));
    let e = ben.app.agent_undo(&bens, false, false).await.unwrap_err();
    assert_eq!(e.code, "other_author", "the top step is a person's: the host added a level");

    // Ben leaves: his agent goes from everyone's list.
    let _: LiveStatus = call(&ben.app, "live_leave", json!({})).await;
    until("the host to drop Ben's agent", || async { !ana.app.agents_list().iter().any(|a| a.participant.id == bens.id) }).await;
    until("Cy to drop Ben's agent", || async { !cy.app.agents_list().iter().any(|a| a.participant.id == bens.id) }).await;
    assert_eq!(ben.app.agents_list().len(), 1, "Ben's agent is still his");
}

async fn status_of(app: &AppService) -> LiveStatus {
    call(app, "live_status", json!({})).await
}

/// Read frames until one that `pick` takes, keeping every frame's type.
async fn frames_until<R: tokio::io::AsyncRead + Unpin>(
    stream: &mut R,
    seen: &mut Vec<String>,
    mut pick: impl FnMut(&Value) -> bool,
) -> Value {
    let next = async {
        loop {
            let bytes = wire::read_frame(stream, wire::MAX_FRAME_BYTES, WAIT, WAIT).await.expect("a frame");
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            seen.push(value["type"].as_str().unwrap_or("").to_string());
            if pick(&value) {
                return value;
            }
        }
    };
    tokio::time::timeout(WAIT, next).await.expect("the frame in time")
}

#[tokio::test]
async fn an_older_guest_sees_no_agents_but_its_history_names_them() {
    let (ana, invite) = hosting("Ana").await;
    let anas = agent(&ana.app, "ground-floor");
    ana.app.agent_commit(&anas, wall(0.0, 0.0, 4000.0, 0.0)).await.unwrap();

    // A guest as 0.4 was: its hello lists no features.
    let invite = Invite::decode(&invite).unwrap();
    let addr: SocketAddr = invite.addrs[0].parse().unwrap();
    let mut old = tls::connect(addr, &invite.pin).await.unwrap();
    let hello = json!({ "type": "hello", "v": wire::PROTOCOL_VERSION, "secret": invite.secret, "name": "Old Ben" });
    old.write_all(&wire::encode(&hello, wire::MAX_HELLO_BYTES).unwrap()).await.unwrap();
    old.flush().await.unwrap();
    let mut seen = vec![];
    let welcome = frames_until(&mut old, &mut seen, |v| v["type"] == "welcome").await;
    assert!(welcome["participants"].as_array().unwrap().iter().all(|p| p["agent"].is_null()));
    assert_eq!(welcome["doc"]["history"]["undo"][0]["author_name"], "Ground floor", "history names the agent");

    // The agent keeps working: the old guest gets the plan, never agents.
    ana.app.agent_commit(&anas, wall(0.0, 3000.0, 4000.0, 3000.0)).await.unwrap();
    ana.app.agent_set_status(&anas, Some("Placing doors"));
    let frame = frames_until(&mut old, &mut seen, |v| v["type"] == "doc").await;
    assert_eq!(frame["history"]["undo"][0]["author_id"], json!(anas.id));
    assert_eq!(frame["history"]["undo"][0]["on_behalf_name"], "Ana");
    // Its own edit still works, and its pings keep it in.
    let edit = json!({ "type": "request", "id": 1, "request": { "type": "apply", "origin": "user",
        "command": serde_json::to_value(wall(0.0, 6000.0, 4000.0, 6000.0)).unwrap() } });
    old.write_all(&wire::encode(&edit, wire::MAX_FRAME_BYTES).unwrap()).await.unwrap();
    old.flush().await.unwrap();
    let reply = frames_until(&mut old, &mut seen, |v| v["type"] == "reply" && v["id"] == 1).await;
    assert_eq!(reply["error"], Value::Null, "{reply}");
    assert!(!seen.iter().any(|t| t == "agents"), "an older guest is sent no agents: {seen:?}");
    assert_eq!(doc(&ana.app).await.project.elements.len(), 3);
}

#[tokio::test]
async fn a_newer_guest_with_an_older_host_sends_no_agents() {
    // A host as 0.4 was: its welcome lists no features.
    let identity = tls::HostIdentity::new().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let secret = URL_SAFE_NO_PAD.encode([7u8; 16]);
    let invite = Invite {
        v: guhit_app::live::invite::INVITE_VERSION,
        secret: secret.clone(),
        pin: identity.pin.clone(),
        addrs: vec![listener.local_addr().unwrap().to_string()],
        project: "Lumang bahay".into(),
        relay: None,
    }
    .encode();
    let mut project = defaults::new_project("Lumang bahay");
    project.id = defaults::new_id();
    let frame = |revision: u32, seq: u64| wire::DocFrame {
        project: project.clone(),
        revision,
        seq,
        undo: wire::UndoMeta::default(),
        by: None,
        history: None,
    };
    let welcome = json!({ "type": "welcome", "participant_id": "g1", "rejoin_token": "t",
        "participants": [
            { "id": "old-host", "name": "Lola", "color": 0, "role": "host" },
            { "id": "g1", "name": "Ben", "color": 1, "role": "guest" }
        ],
        "doc": frame(4, 1), "chat": [], "presence": [] });
    let welcome = wire::encode(&welcome, wire::MAX_FRAME_BYTES).unwrap();
    let done = wire::encode(&wire::ToGuest::Doc(Box::new(frame(5, 2))), wire::MAX_FRAME_BYTES).unwrap();
    let seen = Arc::new(Mutex::new(vec![]));
    let heard = seen.clone();
    let host = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut tls = identity.acceptor.accept(Box::new(tcp) as tls::BoxIo).await.unwrap();
        let mut types = vec![];
        let hello = frames_until(&mut tls, &mut types, |v| v["type"] == "hello").await;
        heard.lock().unwrap().push(hello);
        tls.write_all(&welcome).await.unwrap();
        tls.flush().await.unwrap();
        loop {
            let bytes = wire::read_frame(&mut tls, wire::MAX_FRAME_BYTES, WAIT, WAIT).await.unwrap();
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            heard.lock().unwrap().push(value.clone());
            if value["type"] == "request" {
                tls.write_all(&done).await.unwrap();
                let reply = json!({ "type": "reply", "id": value["id"], "ok": { "revision": 5, "diff": {
                    "added": [], "modified": [], "removed": [], "summary": "Added 1 wall" } } });
                tls.write_all(&wire::encode(&reply, wire::MAX_FRAME_BYTES).unwrap()).await.unwrap();
                tls.flush().await.unwrap();
            }
        }
    });

    let ben = peer("Ben").await;
    join(&ben, &invite).await;
    let bens = agent(&ben.app, "ground-floor");
    ben.app.agent_commit(&bens, wall(0.0, 0.0, 1000.0, 0.0)).await.unwrap();
    let frames = seen.lock().unwrap().clone();
    assert_eq!(frames[0]["features"], json!(["agents"]), "the newer guest lists the feature");
    let request = frames.iter().find(|f| f["type"] == "request").expect("the edit went to the host");
    assert_eq!(request["request"]["type"], "apply");
    assert!(request["request"].get("agent").is_none(), "an older host is never sent an agent: {request}");
    assert!(!frames.iter().any(|f| f["type"] == "agents"), "an older host is sent no agents");
    // Ben's own list still has its agent.
    assert_eq!(ben.app.agents_list().len(), 1);
    host.abort();
}
