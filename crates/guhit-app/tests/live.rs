//! Live sessions end to end (docs/CONTRACT.md, "Live sessions"): a host and
//! its guests in one process, each an `AppService` with its own data dir,
//! talking TLS over loopback. Sandboxed services listen on 127.0.0.1 only.

use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::{STANDARD as B64, URL_SAFE_NO_PAD};
use base64::Engine;
use guhit_app::live::invite::Invite;
use guhit_app::live::{tls, wire};
use guhit_app::AppService;
use guhit_model::*;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

const PNG_1X1: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
/// Longest a test waits for something that should happen at once.
const WAIT: Duration = Duration::from_secs(10);

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("live-{tag}-{}", defaults::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One computer: its service, its window's events, its data dir.
struct Peer {
    app: AppService,
    events: broadcast::Receiver<AppEvent>,
    dir: TempDir,
}

impl Peer {
    fn data(&self) -> &Path {
        &self.dir.0
    }
}

async fn peer(name: &str) -> Peer {
    let dir = TempDir::new(name);
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

/// A computer sharing a new project of its own.
async fn hosting(name: &str) -> (Peer, LiveStatus) {
    let host = peer(name).await;
    let _: DocState = call(&host.app, "hub_create", json!({ "name": "Bahay" })).await;
    let status: LiveStatus = call(&host.app, "live_host", json!({})).await;
    (host, status)
}

async fn join(guest: &Peer, invite: &str) -> DocState {
    call(&guest.app, "live_join", json!({ "invite": invite })).await
}

async fn doc(app: &AppService) -> Option<DocState> {
    call(app, "doc_state", json!({})).await
}

async fn status(app: &AppService) -> LiveStatus {
    call(app, "live_status", json!({})).await
}

fn wall(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
    json!({
        "type": "add_wall",
        "start": { "x": x0, "y": y0 }, "end": { "x": x1, "y": y1 },
        "thickness_mm": null, "height_mm": null, "material_id": null, "level_id": null
    })
}

fn apply(command: Value) -> Value {
    json!({ "command": command })
}

fn walls(state: &DocState) -> usize {
    state.project.elements.iter().filter(|e| matches!(e, Element::Wall(_))).count()
}

/// Poll until `check` holds.
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

/// The next event `pick` takes.
async fn event<T>(
    events: &mut broadcast::Receiver<AppEvent>,
    what: &str,
    mut pick: impl FnMut(AppEvent) -> Option<T>,
) -> T {
    let found = tokio::time::timeout(WAIT, async {
        loop {
            match events.recv().await {
                Ok(e) => {
                    if let Some(found) = pick(e) {
                        return found;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => panic!("events closed while waiting for {what}"),
            }
        }
    })
    .await;
    found.unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

/// Forget the events so far.
fn drain(events: &mut broadcast::Receiver<AppEvent>) {
    while events.try_recv().is_ok() {}
}

/// The next live status in `mode`.
async fn live_event(events: &mut broadcast::Receiver<AppEvent>, mode: LiveMode) -> LiveStatus {
    event(events, &format!("live status {mode:?}"), |e| match e {
        AppEvent::Live { status } if status.mode == mode => Some(status),
        _ => None,
    })
    .await
}

fn me(status: &LiveStatus) -> Participant {
    let id = status.self_id.clone().expect("in a session");
    status.participants.iter().find(|p| p.id == id).cloned().expect("self is a participant")
}

fn presence_at(x: f64, y: f64) -> Value {
    json!({ "presence": {
        "cursor": { "x": x, "y": y }, "level_id": null, "selection": ["w1"], "typing": null, "ai_scope": false
    }})
}

#[tokio::test]
async fn a_guest_edits_through_the_host_and_sees_every_change() {
    let (host, started) = hosting("Ana").await;
    assert_eq!(started.mode, LiveMode::Hosting);
    let invite = started.invite.clone().unwrap();
    assert!(invite.starts_with("guhit-live:"));
    assert_eq!(started.addresses.len(), 1, "the dev bridge listens on loopback only");
    assert!(started.addresses[0].starts_with("127.0.0.1:"));
    let ana = me(&started);
    assert_eq!((ana.color, ana.role, ana.name.as_str()), (0, ParticipantRole::Host, "Ana"));
    // Hosting again returns the running session.
    let again: LiveStatus = call(&host.app, "live_host", json!({})).await;
    assert_eq!(again.invite, started.invite);

    let ben = peer("Ben").await;
    let copy = join(&ben, &invite).await;
    let shared = doc(&host.app).await.unwrap();
    assert_eq!(copy.project.id, shared.project.id);
    assert_eq!(copy.revision, shared.revision);
    let joined = status(&ben.app).await;
    assert_eq!(joined.mode, LiveMode::Joined);
    assert_eq!(joined.invite, None);
    assert_eq!(joined.project_id.as_deref(), Some(shared.project.id.as_str()));
    let bens = me(&joined);
    assert_eq!((bens.color, bens.role), (1, ParticipantRole::Guest));
    assert_eq!(joined.participants[0].id, ana.id, "the host comes first");
    until("the host to see the guest", || async { status(&host.app).await.participants.len() == 2 }).await;

    // A guest's edit is committed on the host and its answer carries the
    // new state, the host's diff and the host's history.
    let applied: ApplyResult = call(&ben.app, "doc_apply", apply(wall(0.0, 0.0, 4000.0, 0.0))).await;
    let shared = doc(&host.app).await.unwrap();
    assert_eq!(walls(&shared), 1);
    assert_eq!(walls(&applied.state), 1);
    assert_eq!(applied.state.revision, shared.revision);
    assert!(!applied.diff.added.is_empty());
    assert_eq!(shared.undo_by.as_deref(), Some(bens.id.as_str()));
    assert!(applied.state.can_undo);
    assert_eq!(applied.state.undo_label.as_deref(), Some("Add wall"));
    assert_eq!(applied.state.undo_by.as_deref(), Some(bens.id.as_str()));
    // Saved by the host; the guest never writes the shared project.
    let saved = std::fs::read_to_string(host.data().join("projects").join(&shared.project.id).join("project.json")).unwrap();
    assert!(saved.contains("\"wall\""));
    assert!(!ben.data().join("projects").join(&shared.project.id).exists());

    // The host's own edit reaches the copy, as the host's step.
    let _: ApplyResult = call(&host.app, "doc_apply", apply(wall(4000.0, 0.0, 4000.0, 3000.0))).await;
    until("the host's edit on the copy", || async { walls(&doc(&ben.app).await.unwrap()) == 2 }).await;
    let copy = doc(&ben.app).await.unwrap();
    assert_eq!(copy.undo_by.as_deref(), Some(ana.id.as_str()));
    assert_eq!(copy.revision, doc(&host.app).await.unwrap().revision);

    // So does an edit an MCP client commits on the host.
    let revision = doc(&host.app).await.unwrap().revision;
    let command: Command = serde_json::from_value(wall(0.0, 3000.0, 4000.0, 3000.0)).unwrap();
    host.app.commit_if_revision(command.clone(), Origin::Ai, revision).await.unwrap();
    until("the MCP edit on the copy", || async { walls(&doc(&ben.app).await.unwrap()) == 3 }).await;

    // The guest's own MCP-style commit goes to the host, revision-guarded.
    let err = ben.app.commit_if_revision(command.clone(), Origin::Ai, 1).await.unwrap_err();
    assert_eq!(err.code, "stale");
    let revision = doc(&ben.app).await.unwrap().revision;
    let committed = ben.app.commit_if_revision(command, Origin::Ai, revision).await.unwrap();
    assert_eq!(committed.state.revision, revision + 1);
    assert_eq!(walls(&doc(&host.app).await.unwrap()), 4);

    // A second guest sees all of it and gets the next color.
    let cy = peer("Cy").await;
    let copy = join(&cy, &invite).await;
    assert_eq!(walls(&copy), 4);
    let cys = me(&status(&cy.app).await);
    assert_eq!(cys.color, 2);
    assert_eq!(status(&cy.app).await.participants.len(), 3);
    until("the first guest to see the second", || async { status(&ben.app).await.participants.len() == 3 }).await;
    let _: ApplyResult = call(&cy.app, "doc_apply", apply(wall(0.0, 0.0, 0.0, 3000.0))).await;
    until("the second guest's edit on the first guest's copy", || async {
        walls(&doc(&ben.app).await.unwrap()) == 5
    })
    .await;
}

#[tokio::test]
async fn presence_travels_both_ways() {
    let (mut host, started) = hosting("Ana").await;
    let ana = me(&started);
    let mut ben = peer("Ben").await;
    join(&ben, started.invite.as_deref().unwrap()).await;
    let bens = me(&status(&ben.app).await);

    let _: Value = call(&ben.app, "presence_set", presence_at(1.0, 2.0)).await;
    let seen = event(&mut host.events, "the guest's pointer on the host", |e| match e {
        AppEvent::Presence { participant_id, presence: Some(p) }
            if participant_id == bens.id && p.cursor == Some(Point { x: 1.0, y: 2.0 }) =>
        {
            Some(p)
        }
        _ => None,
    })
    .await;
    assert_eq!(seen.cursor, Some(Point { x: 1.0, y: 2.0 }));
    assert_eq!(seen.selection, vec!["w1".to_string()]);
    let list: Vec<PresenceEntry> = call(&host.app, "presence_list", json!({})).await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].participant_id, bens.id);

    let _: Value = call(&host.app, "presence_set", presence_at(5.0, 6.0)).await;
    let seen = event(&mut ben.events, "the host's pointer on the guest", |e| match e {
        AppEvent::Presence { participant_id, presence: Some(p) }
            if participant_id == ana.id && p.cursor == Some(Point { x: 5.0, y: 6.0 }) =>
        {
            Some(p)
        }
        _ => None,
    })
    .await;
    assert_eq!(seen.selection, vec!["w1".to_string()]);
    let list: Vec<PresenceEntry> = call(&ben.app, "presence_list", json!({})).await;
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].participant_id, ana.id);
    assert_eq!(list[0].presence.cursor, Some(Point { x: 5.0, y: 6.0 }));
    // This window's own presence is still what MCP reads.
    assert_eq!(ben.app.local_presence().cursor, Some(Point { x: 1.0, y: 2.0 }));

    // A guest leaving is gone for everyone.
    let left: LiveStatus = call(&ben.app, "live_leave", json!({})).await;
    assert_eq!(left.mode, LiveMode::Off);
    assert_eq!(left.notice, None);
    assert!(doc(&ben.app).await.is_none(), "the copy closes");
    event(&mut host.events, "the guest's presence to go", |e| match e {
        AppEvent::Presence { participant_id, presence: None } if participant_id == bens.id => Some(()),
        _ => None,
    })
    .await;
    until("the host to see one participant", || async { status(&host.app).await.participants.len() == 1 }).await;
    let list: Vec<PresenceEntry> = call(&host.app, "presence_list", json!({})).await;
    assert!(list.is_empty());
}

#[tokio::test]
async fn chat_is_stamped_saved_and_sent_to_everyone() {
    let (mut host, started) = hosting("Ana").await;
    let invite = started.invite.clone().unwrap();
    let mut ben = peer("Ben").await;
    join(&ben, &invite).await;
    let bens = me(&status(&ben.app).await);

    let sent: ChatMessage = call(&ben.app, "chat_send", json!({ "text": "  Hello from Ben  " })).await;
    assert_eq!(sent.text, "Hello from Ben");
    assert_eq!((sent.author_id.as_str(), sent.author_name.as_str(), sent.color), (bens.id.as_str(), "Ben", 1));
    assert!(!sent.via_ai);
    assert_eq!(sent.at, None);
    let pick = |id: &str| {
        let id = id.to_string();
        move |e: AppEvent| match e {
            AppEvent::Chat { message } if message.id == id => Some(message),
            _ => None,
        }
    };
    event(&mut host.events, "the chat on the host", pick(&sent.id)).await;
    event(&mut ben.events, "the chat back on the sender's window", pick(&sent.id)).await;

    // Saved with the project on the host.
    let project_id = started.project_id.clone().unwrap();
    let file = host.data().join("projects").join(&project_id).join("chat.jsonl");
    let saved = std::fs::read_to_string(&file).unwrap();
    assert_eq!(saved.lines().count(), 1);
    assert!(saved.contains(&sent.id));
    let on_host: Vec<ChatMessage> = call(&host.app, "chat_list", json!({})).await;
    assert_eq!(on_host, vec![sent.clone()]);
    let on_guest: Vec<ChatMessage> = call(&ben.app, "chat_list", json!({})).await;
    assert_eq!(on_guest, vec![sent.clone()]);

    // Cursor chat from an AI client on the host.
    let at = Point { x: 100.0, y: 200.0 };
    let note = host.app.chat_send("On it", Some(at), None, true).await.unwrap();
    assert!(note.via_ai);
    assert_eq!(note.at, Some(at));
    assert_eq!(note.color, 0);
    event(&mut ben.events, "the cursor chat on the guest", pick(&note.id)).await;
    let on_guest: Vec<ChatMessage> = call(&ben.app, "chat_list", json!({})).await;
    assert_eq!(on_guest.len(), 2);

    // Limits, on both sides.
    assert_eq!(fail(&ben.app, "chat_send", json!({ "text": " \n " })).await.code, "bad_args");
    let long_cursor = json!({ "text": "x".repeat(MAX_CURSOR_CHAT_CHARS + 1), "at": { "x": 0.0, "y": 0.0 } });
    assert_eq!(fail(&ben.app, "chat_send", long_cursor).await.code, "bad_args");
    let long = json!({ "text": "x".repeat(MAX_CHAT_CHARS + 1) });
    assert_eq!(fail(&host.app, "chat_send", long).await.code, "bad_args");
    let fits: ChatMessage = call(&ben.app, "chat_send", json!({ "text": "x".repeat(MAX_CHAT_CHARS) })).await;
    assert_eq!(fits.text.chars().count(), MAX_CHAT_CHARS);

    // Someone who joins later gets the history.
    let cy = peer("Cy").await;
    join(&cy, &invite).await;
    let history: Vec<ChatMessage> = call(&cy.app, "chat_list", json!({})).await;
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].id, sent.id);
}

#[tokio::test]
async fn undo_asks_before_taking_back_someone_elses_step() {
    let host = peer("Ana").await;
    let _: DocState = call(&host.app, "hub_create", json!({ "name": "Bahay" })).await;
    // Outside a session a step has no author.
    let before: ApplyResult = call(&host.app, "doc_apply", apply(wall(0.0, 0.0, 4000.0, 0.0))).await;
    assert_eq!(before.state.undo_by, None);
    let started: LiveStatus = call(&host.app, "live_host", json!({})).await;
    let ana = me(&started);
    let ben = peer("Ben").await;
    join(&ben, started.invite.as_deref().unwrap()).await;
    let bens = me(&status(&ben.app).await);

    // The host's step from before the session is someone else's to the guest.
    let err = fail(&ben.app, "doc_undo", json!({})).await;
    assert_eq!(err.code, "other_author");
    assert_eq!(err.message, "Ana made the last change: Add wall. Undo it anyway?");
    // To the host it is its own.
    let undone: DocState = call(&host.app, "doc_undo", json!({})).await;
    assert_eq!(walls(&undone), 0);
    let redone: DocState = call(&host.app, "doc_redo", json!({})).await;
    assert_eq!(walls(&redone), 1);

    // A guest's step.
    let applied: ApplyResult = call(&ben.app, "doc_apply", apply(wall(4000.0, 0.0, 4000.0, 3000.0))).await;
    assert_eq!(applied.state.undo_by.as_deref(), Some(bens.id.as_str()));
    let err = fail(&host.app, "doc_undo", json!({})).await;
    assert_eq!(err.code, "other_author");
    assert_eq!(err.message, "Ben made the last change: Add wall. Undo it anyway?");
    // With force it goes, and redo then asks too.
    let undone: DocState = call(&host.app, "doc_undo", json!({ "force": true })).await;
    assert_eq!(walls(&undone), 1);
    assert_eq!(undone.redo_by.as_deref(), Some(bens.id.as_str()));
    let err = fail(&host.app, "doc_redo", json!({})).await;
    assert_eq!(err.code, "other_author");
    assert_eq!(err.message, "This step is Ben's: Add wall. Redo it anyway?");

    // The guest brings back its own step without asking; its copy is
    // current when the call returns.
    let redone: DocState = call(&ben.app, "doc_redo", json!({})).await;
    assert_eq!(walls(&redone), 2);
    assert_eq!(redone.undo_by.as_deref(), Some(bens.id.as_str()));
    assert_eq!(redone.revision, doc(&host.app).await.unwrap().revision);
    let undone: DocState = call(&ben.app, "doc_undo", json!({})).await;
    assert_eq!(walls(&undone), 1);
    // The step under it is the host's again.
    assert_eq!(fail(&ben.app, "doc_undo", json!({})).await.code, "other_author");
    let redone: DocState = call(&ben.app, "doc_redo", json!({})).await;
    assert_eq!(walls(&redone), 2);

    // A commit on the host during the session records the host.
    let applied: ApplyResult = call(&host.app, "doc_apply", apply(wall(0.0, 3000.0, 4000.0, 3000.0))).await;
    assert_eq!(applied.state.undo_by.as_deref(), Some(ana.id.as_str()));
    let err = fail(&ben.app, "doc_undo", json!({})).await;
    assert_eq!(err.message, "Ana made the last change: Add wall. Undo it anyway?");
    let undone: DocState = call(&ben.app, "doc_undo", json!({ "force": true })).await;
    assert_eq!(walls(&undone), 2);
    assert_eq!(undone.redo_by.as_deref(), Some(ana.id.as_str()));

    // In the host's next session its steps are still its own.
    let _: LiveStatus = call(&host.app, "live_leave", json!({})).await;
    let again: LiveStatus = call(&host.app, "live_host", json!({})).await;
    assert_eq!(me(&again).id, ana.id);
    let redone: DocState = call(&host.app, "doc_redo", json!({})).await;
    assert_eq!(walls(&redone), 3);
}

#[tokio::test]
async fn the_host_ending_the_session_closes_every_copy() {
    let (host, started) = hosting("Ana").await;
    let invite = started.invite.clone().unwrap();
    let mut ben = peer("Ben").await;
    join(&ben, &invite).await;

    let ended: LiveStatus = call(&host.app, "live_leave", json!({})).await;
    assert_eq!(ended.mode, LiveMode::Off);
    let off = live_event(&mut ben.events, LiveMode::Off).await;
    assert_eq!(off.notice.as_deref(), Some("Ana ended the live session."));
    until("the guest's copy to close", || async { doc(&ben.app).await.is_none() }).await;
    assert_eq!(status(&ben.app).await.notice.as_deref(), Some("Ana ended the live session."));
    assert_eq!(fail(&ben.app, "chat_send", json!({ "text": "hi" })).await.code, "not_live");
    // The host keeps its project, and the session is gone. (Another test's
    // session may already listen on the freed port: then the pin tells.)
    assert!(doc(&host.app).await.is_some());
    let cy = peer("Cy").await;
    let err = fail(&cy.app, "live_join", json!({ "invite": invite })).await;
    assert!(err.code == "live_unreachable" || err.code == "live_pin", "{err}");
}

#[tokio::test]
async fn closing_or_switching_the_shared_project_ends_the_session() {
    let (host, started) = hosting("Ana").await;
    let project_id = started.project_id.clone().unwrap();
    let mut ben = peer("Ben").await;
    join(&ben, started.invite.as_deref().unwrap()).await;

    let _: Value = call(&host.app, "hub_close", json!({})).await;
    let host_status = status(&host.app).await;
    assert_eq!(host_status.mode, LiveMode::Off);
    assert_eq!(host_status.notice.as_deref(), Some("The live session ended because the project closed."));
    let off = live_event(&mut ben.events, LiveMode::Off).await;
    assert_eq!(off.notice.as_deref(), Some("Ana closed the project."));
    until("the guest's copy to close", || async { doc(&ben.app).await.is_none() }).await;

    // Again, then another project opens on the host.
    let _: DocState = call(&host.app, "hub_open", json!({ "id": project_id })).await;
    let again: LiveStatus = call(&host.app, "live_host", json!({})).await;
    assert_ne!(again.invite, started.invite, "every session has its own invite");
    join(&ben, again.invite.as_deref().unwrap()).await;
    let _: DocState = call(&host.app, "hub_create", json!({ "name": "Other" })).await;
    let off = live_event(&mut ben.events, LiveMode::Off).await;
    assert_eq!(off.notice.as_deref(), Some("Ana closed the project."));
    assert_eq!(status(&host.app).await.mode, LiveMode::Off);
}

#[tokio::test]
async fn the_host_can_remove_a_guest() {
    let (host, started) = hosting("Ana").await;
    let ana = me(&started);
    let mut ben = peer("Ben").await;
    join(&ben, started.invite.as_deref().unwrap()).await;
    let bens = me(&status(&ben.app).await);

    let err = fail(&ben.app, "live_remove", json!({ "participant_id": ana.id })).await;
    assert_eq!(err.code, "host_only");
    assert_eq!(fail(&host.app, "live_remove", json!({ "participant_id": ana.id })).await.code, "bad_args");
    assert_eq!(fail(&host.app, "live_remove", json!({ "participant_id": "nobody" })).await.code, "not_found");

    let after: LiveStatus = call(&host.app, "live_remove", json!({ "participant_id": bens.id })).await;
    assert_eq!(after.participants.len(), 1);
    assert_eq!(after.mode, LiveMode::Hosting);
    let off = live_event(&mut ben.events, LiveMode::Off).await;
    assert_eq!(off.notice.as_deref(), Some("Ana removed you from the live session."));
    until("the guest's copy to close", || async { doc(&ben.app).await.is_none() }).await;
}

#[tokio::test]
async fn a_guest_saves_a_copy_with_its_files() {
    let (host, started) = hosting("Ana").await;
    let shared_id = started.project_id.clone().unwrap();
    // An underlay the plan uses, stored on the host.
    let stored: Value = call(&host.app, "underlay_store", json!({ "file_name": "lot.png", "data": PNG_1X1 })).await;
    assert_eq!(stored["file_name"], "lot.png");
    let level_id = doc(&host.app).await.unwrap().project.levels[0].id.clone();
    let underlay = Element::Underlay(Underlay {
        id: "u1".into(),
        level_id,
        file_name: "lot.png".into(),
        position: Point { x: 0.0, y: 0.0 },
        width_px: 1,
        height_px: 1,
        mm_per_px: 10.0,
        scale_confirmed: true,
        rotation_deg: 0.0,
        opacity: 0.5,
        locked: false,
    });
    let _: ApplyResult = call(&host.app, "doc_apply", apply(json!({ "type": "add_element", "element": underlay }))).await;

    let mut ben = peer("Ben").await;
    join(&ben, started.invite.as_deref().unwrap()).await;
    let saved: ProjectMeta = call(&ben.app, "live_save_copy", json!({})).await;
    assert_ne!(saved.id, shared_id);
    assert_eq!(saved.name, "Bahay (copy)");
    let copy_dir = ben.data().join("projects").join(&saved.id);
    assert!(copy_dir.join("project.json").is_file());
    assert!(copy_dir.join("underlays").join("lot.png").is_file(), "the underlay came along");
    let list: Vec<ProjectMeta> = call(&ben.app, "hub_list", json!({})).await;
    assert_eq!(list.len(), 1);
    // The guest is still in the session, on the shared project.
    assert_eq!(doc(&ben.app).await.unwrap().project.id, shared_id);

    // After the session ends, until another project opens.
    let _: LiveStatus = call(&host.app, "live_leave", json!({})).await;
    live_event(&mut ben.events, LiveMode::Off).await;
    until("the guest's copy to close", || async { doc(&ben.app).await.is_none() }).await;
    let later: ProjectMeta = call(&ben.app, "live_save_copy", json!({})).await;
    assert_ne!(later.id, saved.id);
    let _: DocState = call(&ben.app, "hub_open", json!({ "id": saved.id })).await;
    assert_eq!(fail(&ben.app, "live_save_copy", json!({})).await.code, "not_live");
}

#[tokio::test]
async fn a_guest_cannot_reach_what_is_the_hosts() {
    let (host, started) = hosting("Ana").await;
    let invite = started.invite.clone().unwrap();
    let shared_id = started.project_id.clone().unwrap();
    let ben = peer("Ben").await;
    // A project of the guest's own is open first; joining saves and closes it.
    let own: DocState = call(&ben.app, "hub_create", json!({ "name": "Mine" })).await;
    join(&ben, &invite).await;
    assert!(ben.data().join("projects").join(&own.project.id).join("project.json").is_file());

    let leave_first = [
        ("hub_open", json!({ "id": own.project.id })),
        ("hub_create", json!({ "name": "New" })),
        ("bundle_open", json!({ "file_name": "a.guhit", "data": "data:application/zip;base64,UEsFBgAAAAAAAAAAAAAAAAAAAAAAAA==" })),
        ("live_host", json!({})),
        ("live_join", json!({ "invite": invite })),
    ];
    for (cmd, args) in leave_first {
        let err = fail(&ben.app, cmd, args).await;
        assert_eq!((err.code.as_str(), err.message.as_str()), ("host_only", "Leave the live session first."), "{cmd}");
    }
    let hosts_only = [
        ("snapshot_create", json!({ "label": "v1" })),
        ("snapshot_list", json!({})),
        ("snapshot_restore", json!({ "id": "s1" })),
        ("bundle_save", json!({})),
        ("hub_rename", json!({ "id": shared_id, "name": "Mine now" })),
    ];
    for (cmd, args) in hosts_only {
        assert_eq!(fail(&ben.app, cmd, args).await.code, "host_only", "{cmd}");
    }
    // The window saving a thumbnail of the shared plan does nothing.
    let _: Value = call(&ben.app, "hub_set_thumbnail", json!({ "id": shared_id, "png": PNG_1X1 })).await;
    assert!(!ben.data().join("projects").join(&shared_id).exists());
    // The host cannot join someone else while hosting.
    let err = fail(&host.app, "live_join", json!({ "invite": invite })).await;
    assert_eq!((err.code.as_str(), err.message.as_str()), ("host_only", "End your live session first."));

    // Exports of the shared plan stay in the guest's folder for it.
    let _: ApplyResult = call(&ben.app, "doc_apply", apply(wall(0.0, 0.0, 4000.0, 0.0))).await;
    let options = json!({
        "level_id": null, "paper": "a3", "orientation": "landscape", "scale_denominator": null,
        "show_dimensions": true, "show_room_labels": true, "show_assets": true, "title_block": false
    });
    let exported: ExportResult = call(&ben.app, "export_plan", json!({ "format": "svg", "options": options })).await;
    let live_dir = ben.data().join("live").join(&shared_id);
    assert!(Path::new(&exported.path).starts_with(&live_dir), "{}", exported.path);
    assert!(Path::new(&exported.path).is_file());

    // Nothing reached the host's own projects.
    let hosts: Vec<ProjectMeta> = call(&host.app, "hub_list", json!({})).await;
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0].name, "Bahay");

    // hub_close on a guest leaves.
    let _: Value = call(&ben.app, "hub_close", json!({})).await;
    assert_eq!(status(&ben.app).await.mode, LiveMode::Off);
    until("the host to see the guest leave", || async { status(&host.app).await.participants.len() == 1 }).await;
}

#[tokio::test]
async fn files_travel_through_the_host() {
    let (host, started) = hosting("Ana").await;
    let project_dir = host.data().join("projects").join(started.project_id.as_deref().unwrap());
    let ben = peer("Ben").await;
    join(&ben, started.invite.as_deref().unwrap()).await;

    let stored: Value = call(&ben.app, "underlay_store", json!({ "file_name": "site.png", "data": PNG_1X1 })).await;
    assert_eq!(stored, json!({ "file_name": "site.png" }));
    assert!(project_dir.join("underlays").join("site.png").is_file());
    let data: String = call(&ben.app, "underlay_data", json!({ "file_name": "site.png" })).await;
    assert_eq!(data, PNG_1X1);
    // Never over another file.
    let again: Value = call(&ben.app, "underlay_store", json!({ "file_name": "site.png", "data": PNG_1X1 })).await;
    assert_eq!(again, json!({ "file_name": "site-2.png" }));

    // A reference model larger than one piece goes over in several.
    let obj: Vec<u8> = "v 0 0 0\n".repeat(600_000).into_bytes();
    assert!(obj.len() > wire::FILE_CHUNK * 2);
    let url = format!("data:model/obj;base64,{}", B64.encode(&obj));
    let stored: Value = call(&ben.app, "model_store", json!({ "file_name": "block.obj", "data": url })).await;
    assert_eq!(stored, json!({ "file_name": "block.obj", "size": obj.len() }));
    assert_eq!(std::fs::read(project_dir.join("models").join("block.obj")).unwrap(), obj);
    let back: String = call(&ben.app, "model_data", json!({ "file_name": "block.obj" })).await;
    let bytes = B64.decode(back.strip_prefix("data:model/obj;base64,").unwrap()).unwrap();
    assert_eq!(bytes, obj);

    // The same name and type checks as the window's own calls.
    for name in ["../secret.png", "a/b.png", ""] {
        assert_eq!(fail(&ben.app, "underlay_data", json!({ "file_name": name })).await.code, "invalid", "{name}");
    }
    let err = fail(&ben.app, "model_store", json!({ "file_name": "tool.exe", "data": "data:x;base64,AAAA" })).await;
    assert_eq!(err.code, "invalid");
    assert_eq!(fail(&ben.app, "model_data", json!({ "file_name": "missing.obj" })).await.code, "not_found");
    assert_eq!(fail(&ben.app, "underlay_data", json!({ "file_name": "missing.png" })).await.code, "not_found");
}

#[tokio::test]
async fn a_wrong_secret_a_wrong_certificate_and_no_host_are_told_apart() {
    let (_host, started) = hosting("Ana").await;
    let invite = Invite::decode(started.invite.as_deref().unwrap()).unwrap();
    let ben = peer("Ben").await;

    let mut wrong = invite.clone();
    wrong.secret = URL_SAFE_NO_PAD.encode([0u8; 16]);
    let asked = Instant::now();
    let err = fail(&ben.app, "live_join", json!({ "invite": wrong.encode() })).await;
    assert_eq!(err.code, "live_refused");
    assert_eq!(err.message, "This invite does not open the live session. Ask the host for a new invite.");
    assert!(asked.elapsed() >= Duration::from_millis(900), "a wrong secret is answered after a delay");
    let after = status(&ben.app).await;
    assert_eq!((after.mode, after.notice.as_deref()), (LiveMode::Off, Some(err.message.as_str())));

    let mut pinned = invite.clone();
    pinned.pin = URL_SAFE_NO_PAD.encode([7u8; 32]);
    let err = fail(&ben.app, "live_join", json!({ "invite": pinned.encode() })).await;
    assert_eq!(err.code, "live_pin");
    assert_eq!(
        err.message,
        format!("The computer at {} is not the host this invite is for. Ask for a new invite.", invite.addrs[0])
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let closed = listener.local_addr().unwrap();
    drop(listener);
    let mut nowhere = invite.clone();
    nowhere.addrs = vec![closed.to_string()];
    let err = fail(&ben.app, "live_join", json!({ "invite": nowhere.encode() })).await;
    assert_eq!(err.code, "live_unreachable");
    assert!(err.message.contains(&closed.to_string()) && err.message.contains("same network or VPN"), "{}", err.message);

    // None of it touched the session.
    let copy = join(&ben, started.invite.as_deref().unwrap()).await;
    assert_eq!(Some(copy.project.id), started.project_id);
    assert_eq!(status(&ben.app).await.notice, None, "a new session clears the notice");
}

/// Wait for the other side to close. A TLS alert may come first.
async fn closed<R: tokio::io::AsyncRead + Unpin>(stream: &mut R, what: &str) {
    let ends = async {
        let mut buf = [0u8; 256];
        while let Ok(n) = stream.read(&mut buf).await {
            if n == 0 {
                break;
            }
        }
    };
    assert!(tokio::time::timeout(WAIT, ends).await.is_ok(), "the host should have closed {what}");
}

#[tokio::test]
async fn a_bad_hello_does_not_take_the_host_down() {
    let (host, started) = hosting("Ana").await;
    let invite = Invite::decode(started.invite.as_deref().unwrap()).unwrap();
    let addr: SocketAddr = invite.addrs[0].parse().unwrap();

    // Not TLS at all.
    let mut raw = TcpStream::connect(addr).await.unwrap();
    raw.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
    closed(&mut raw, "a plain HTTP request").await;

    // A hello that claims to be larger than 64 KiB.
    let mut tls = tls::connect(addr, &invite.pin).await.unwrap();
    tls.write_all(&(1024u32 * 1024).to_be_bytes()).await.unwrap();
    tls.flush().await.unwrap();
    closed(&mut tls, "an oversized hello").await;

    // A hello that is not JSON.
    let mut tls = tls::connect(addr, &invite.pin).await.unwrap();
    let mut garbage = 9u32.to_be_bytes().to_vec();
    garbage.extend_from_slice(b"not json!");
    tls.write_all(&garbage).await.unwrap();
    tls.flush().await.unwrap();
    closed(&mut tls, "a garbage hello").await;

    // A message that is not a hello.
    let mut tls = tls::connect(addr, &invite.pin).await.unwrap();
    tls.write_all(&wire::encode(&wire::ToHost::Ping, wire::MAX_HELLO_BYTES).unwrap()).await.unwrap();
    tls.flush().await.unwrap();
    closed(&mut tls, "a ping instead of a hello").await;

    // More than 8 connections waiting to authenticate: the next is closed at once.
    let mut waiting = vec![];
    for _ in 0..8 {
        waiting.push(TcpStream::connect(addr).await.unwrap());
    }
    let mut ninth = TcpStream::connect(addr).await.unwrap();
    closed(&mut ninth, "a ninth waiting connection").await;
    drop(waiting);

    // The host is fine: a real guest joins and edits.
    let ben = peer("Ben").await;
    let joined = async {
        loop {
            // The 8 dropped connections free their places as the host notices.
            match ben.app.handle("live_join", json!({ "invite": started.invite })).await {
                Ok(state) => return state,
                Err(e) if e.code == "live_unreachable" => tokio::time::sleep(Duration::from_millis(50)).await,
                Err(e) => panic!("live_join failed: {e}"),
            }
        }
    };
    tokio::time::timeout(WAIT, joined).await.expect("the guest gets in");
    let applied: ApplyResult = call(&ben.app, "doc_apply", apply(wall(0.0, 0.0, 1000.0, 0.0))).await;
    assert_eq!(walls(&applied.state), 1);
    assert_eq!(status(&host.app).await.mode, LiveMode::Hosting);
}

/// A TCP relay between a guest and the host whose connections the test can
/// cut, as a network drop would.
struct Relay {
    addr: SocketAddr,
    pipes: Arc<Mutex<Vec<JoinHandle<()>>>>,
    accept: JoinHandle<()>,
}

impl Relay {
    async fn start(target: SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let pipes: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::default();
        let kept = pipes.clone();
        let accept = tokio::spawn(async move {
            while let Ok((mut inbound, _)) = listener.accept().await {
                let pipe = tokio::spawn(async move {
                    if let Ok(mut outbound) = TcpStream::connect(target).await {
                        let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                    }
                });
                kept.lock().unwrap().push(pipe);
            }
        });
        Relay { addr, pipes, accept }
    }

    /// Drop every connection through the relay. New ones still go through.
    fn cut(&self) {
        for pipe in self.pipes.lock().unwrap().drain(..) {
            pipe.abort();
        }
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.cut();
        self.accept.abort();
    }
}

#[tokio::test]
async fn a_dropped_connection_comes_back_with_the_same_participant() {
    let (mut host, started) = hosting("Ana").await;
    let invite = Invite::decode(started.invite.as_deref().unwrap()).unwrap();
    let relay = Relay::start(invite.addrs[0].parse().unwrap()).await;
    let mut through = invite.clone();
    through.addrs = vec![relay.addr.to_string()];
    let mut ben = peer("Ben").await;
    join(&ben, &through.encode()).await;
    let bens = me(&status(&ben.app).await);
    until("the host to see the guest", || async { status(&host.app).await.participants.len() == 2 }).await;
    let _: ApplyResult = call(&ben.app, "doc_apply", apply(wall(0.0, 0.0, 1000.0, 0.0))).await;

    drain(&mut host.events);
    relay.cut();
    live_event(&mut ben.events, LiveMode::Reconnecting).await;
    // The plan stays on screen; edits wait for the host.
    assert_eq!(walls(&doc(&ben.app).await.unwrap()), 1);
    let err = fail(&ben.app, "doc_apply", apply(wall(1000.0, 0.0, 2000.0, 0.0))).await;
    assert_eq!(err.code, "live_lost");
    // The host sees the guest go.
    let gone = live_event(&mut host.events, LiveMode::Hosting).await;
    assert_eq!(gone.participants.len(), 1);
    // Meanwhile the host keeps working.
    let _: ApplyResult = call(&host.app, "doc_apply", apply(wall(0.0, 1000.0, 1000.0, 1000.0))).await;

    let back = live_event(&mut ben.events, LiveMode::Joined).await;
    let bens_again = me(&back);
    assert_eq!((bens_again.id.as_str(), bens_again.color), (bens.id.as_str(), bens.color));
    assert_eq!(walls(&doc(&ben.app).await.unwrap()), 2, "back in with the host's current plan");
    let applied: ApplyResult = call(&ben.app, "doc_apply", apply(wall(1000.0, 0.0, 2000.0, 0.0))).await;
    assert_eq!(walls(&applied.state), 3);
    assert_eq!(applied.state.undo_by.as_deref(), Some(bens.id.as_str()));
    until("the host to see the guest back", || async {
        status(&host.app).await.participants.iter().any(|p| p.id == bens.id)
    })
    .await;
}

type RawConnection = tokio_rustls::client::TlsStream<TcpStream>;

/// A client speaking the frames directly: TLS, hello, and the host's answer.
async fn raw_join(invite: &Invite, name: &str) -> (RawConnection, Result<wire::Welcome, String>) {
    let hello = wire::ToHost::Hello {
        v: wire::PROTOCOL_VERSION,
        secret: invite.secret.clone(),
        name: name.to_string(),
        rejoin: None,
    };
    raw_hello(invite, &hello).await
}

async fn raw_hello(invite: &Invite, hello: &wire::ToHost) -> (RawConnection, Result<wire::Welcome, String>) {
    let addr: SocketAddr = invite.addrs[0].parse().unwrap();
    let mut tls = tls::connect(addr, &invite.pin).await.unwrap();
    tls.write_all(&wire::encode(hello, wire::MAX_HELLO_BYTES).unwrap()).await.unwrap();
    let bytes = wire::read_frame(&mut tls, wire::MAX_FRAME_BYTES, WAIT, WAIT).await.unwrap();
    match wire::decode::<wire::ToGuest>(&bytes).unwrap() {
        wire::ToGuest::Welcome(welcome) => (tls, Ok(*welcome)),
        wire::ToGuest::Refused { message } => (tls, Err(message)),
        other => panic!("expected a welcome or a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn a_removed_guest_that_stays_connected_gets_nothing_more() {
    let (host, started) = hosting("Ana").await;
    let invite = Invite::decode(started.invite.as_deref().unwrap()).unwrap();
    let (mut tls, welcome) = raw_join(&invite, "Eve").await;
    let eve = welcome.unwrap().participant_id;
    let _: LiveStatus = call(&host.app, "live_remove", json!({ "participant_id": eve })).await;

    // Eve ignores the goodbye and asks for an edit anyway.
    let edit = wire::ToHost::Request {
        id: 1,
        request: wire::Request::Apply {
            command: serde_json::from_value(wall(0.0, 0.0, 1000.0, 0.0)).unwrap(),
            origin: Origin::User,
            expected_revision: None,
        },
    };
    let _ = tls.write_all(&wire::encode(&edit, wire::MAX_FRAME_BYTES).unwrap()).await;
    closed(&mut tls, "a removed guest's connection").await;
    assert_eq!(walls(&doc(&host.app).await.unwrap()), 0);
    assert_eq!(status(&host.app).await.participants.len(), 1);
}

#[tokio::test]
async fn at_most_sixteen_people_share_eight_colors() {
    let (_host, started) = hosting("Ana").await;
    let invite = Invite::decode(started.invite.as_deref().unwrap()).unwrap();
    let mut connections = vec![];
    let mut last = None;
    for i in 1..MAX_PARTICIPANTS {
        let (tls, welcome) = raw_join(&invite, &format!("Guest {i}")).await;
        connections.push(tls);
        last = Some(welcome.unwrap_or_else(|e| panic!("guest {i} was refused: {e}")));
    }
    let everyone = last.unwrap().participants;
    assert_eq!(everyone.len(), MAX_PARTICIPANTS);
    let colors: Vec<u8> = everyone.iter().map(|p| p.color).collect();
    // The eight colors, each used once, then each a second time.
    let expected: Vec<u8> = (0..PEER_COLOR_COUNT).chain(0..PEER_COLOR_COUNT).collect();
    assert_eq!(colors, expected);

    let (_, refused) = raw_join(&invite, "One too many").await;
    assert_eq!(refused.unwrap_err(), format!("The live session is full: {MAX_PARTICIPANTS} people are in it."));
}

#[tokio::test]
async fn only_the_rejoin_token_gives_a_participant_back() {
    let (host, started) = hosting("Ana").await;
    let invite = Invite::decode(started.invite.as_deref().unwrap()).unwrap();
    let (mut first, welcome) = raw_join(&invite, "Ben").await;
    let welcome = welcome.unwrap();
    let ben = welcome.participants.iter().find(|p| p.id == welcome.participant_id).unwrap().clone();

    // Someone who knows the id but not the token is someone new.
    let impostor = wire::ToHost::Hello {
        v: wire::PROTOCOL_VERSION,
        secret: invite.secret.clone(),
        name: "Mallory".into(),
        rejoin: Some(wire::Rejoin { id: ben.id.clone(), token: URL_SAFE_NO_PAD.encode([1u8; 16]) }),
    };
    let (_other, answer) = raw_hello(&invite, &impostor).await;
    let mallory = answer.unwrap();
    assert_ne!(mallory.participant_id, ben.id);
    assert_eq!(mallory.participants.len(), 3);

    // With the token, a new connection takes over while the old one is still
    // open: the same id and color, and the old connection is closed.
    let back = wire::ToHost::Hello {
        v: wire::PROTOCOL_VERSION,
        secret: invite.secret.clone(),
        name: "Ben".into(),
        rejoin: Some(wire::Rejoin { id: ben.id.clone(), token: welcome.rejoin_token.clone() }),
    };
    let (_again, answer) = raw_hello(&invite, &back).await;
    let again = answer.unwrap();
    assert_eq!(again.participant_id, ben.id);
    let color = again.participants.iter().find(|p| p.id == ben.id).unwrap().color;
    assert_eq!(color, ben.color);
    assert_eq!(again.participants.len(), 3, "the same people, Ben once");
    closed(&mut first, "the replaced connection").await;
    assert_eq!(status(&host.app).await.participants.len(), 3);

    // Another version of the app is turned away with a reason.
    let newer = wire::ToHost::Hello { v: 999, secret: invite.secret.clone(), name: "Cy".into(), rejoin: None };
    let (_, answer) = raw_hello(&invite, &newer).await;
    assert!(answer.unwrap_err().contains("different version"));
}
