//! Live sessions (DECISIONS D29, docs/CONTRACT.md "Live sessions"): one
//! computer hosts the open project, others join with an invite and work on it
//! together, with presence (pointer, selection, cursor chat) and chat.
//!
//! - `host`: the listener, each guest's connection, and sending every change
//!   of the host's document, presence and chat to everyone.
//! - `guest`: joining, the read-only copy, reconnecting, and sending edits to
//!   the host.
//! - `wire`: frames and messages. `invite`: the invite. `tls`: the session
//!   certificate and the pinned connection.
//!
//! This module also owns this window's own presence (`presence_set`), which
//! the MCP tool `get_selection` reads with or without a session, and the
//! display name (`profile_get`, `profile_set`).

mod guest;
mod host;
pub mod invite;
pub mod tls;
pub mod wire;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use guhit_core::Document;
use guhit_model::*;
use serde_json::{json, Value};

use crate::{arg, files, to_value, AppService, IpcResult};

pub const OWNS: &[&str] = &[
    "presence_set",
    "presence_list",
    "profile_get",
    "profile_set",
    "live_status",
    "live_host",
    "live_join",
    "live_leave",
    "live_remove",
    "live_save_copy",
    "chat_send",
    "chat_list",
];

const SETTINGS_FILE: &str = "settings.json";
const PROFILE_KEY: &str = "profile_name";
/// The project's chat, one `ChatMessage` per line, in the project folder.
pub const CHAT_FILE: &str = "chat.jsonl";
/// `chat_list` returns at most this many, the newest.
pub const CHAT_HISTORY: usize = 500;
/// Longest selection a presence carries.
pub const MAX_PRESENCE_SELECTION: usize = 2000;
/// Longest id a presence or a chat message may carry. Ids are UUIDs and
/// slugs, far shorter.
const MAX_ID_CHARS: usize = 64;

/// What this computer is in a live session.
#[derive(Clone, Default)]
pub(crate) enum Role {
    #[default]
    Off,
    Host(Arc<host::Host>),
    Guest(Arc<guest::Guest>),
}

/// Live session state of one `AppService`.
pub struct LiveState {
    /// This window's presence, from `presence_set`.
    local: Mutex<Presence>,
    status: Mutex<LiveStatus>,
    role: Mutex<Role>,
    /// Names of everyone this computer met in a live session, by participant
    /// id, so an undo can name who made a step after they left.
    names: Mutex<HashMap<Id, String>>,
    /// Numbers the joins, so a late message of an old session is ignored.
    epochs: AtomicU64,
    /// This computer's participant id whenever it hosts, the same for every
    /// session while the app runs, so its steps stay its own in the next one.
    host_id: OnceLock<Id>,
}

impl Default for LiveState {
    fn default() -> Self {
        Self {
            local: Mutex::new(Presence::default()),
            status: Mutex::new(LiveStatus::off()),
            role: Mutex::new(Role::Off),
            names: Mutex::new(HashMap::new()),
            epochs: AtomicU64::new(0),
            host_id: OnceLock::new(),
        }
    }
}

/// A guest's copy of the shared project, kept in `Session::live_copy` while
/// the open document is one. The copy's own `Document` has no history, so the
/// host's is kept here.
#[derive(Debug, Clone)]
pub(crate) struct CopyMeta {
    /// The join this copy belongs to.
    pub(crate) epoch: u64,
    /// The host's change counter of the copy (`DocFrame::seq`).
    pub(crate) seq: u64,
    pub(crate) undo: wire::UndoMeta,
}

/// Locks never stay poisoned: every state behind them is valid after any
/// single change.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl LiveState {
    pub(crate) fn role(&self) -> Role {
        lock(&self.role).clone()
    }

    pub(crate) fn status(&self) -> LiveStatus {
        lock(&self.status).clone()
    }

    fn set_status(&self, status: LiveStatus) {
        *lock(&self.status) = status;
    }

    /// Change the status and return the result, for `AppEvent::Live`.
    fn edit_status(&self, edit: impl FnOnce(&mut LiveStatus)) -> LiveStatus {
        let mut status = lock(&self.status);
        edit(&mut status);
        status.clone()
    }

    fn remember_name(&self, id: &str, name: &str) {
        lock(&self.names).insert(id.to_string(), name.to_string());
    }

    fn name_of(&self, id: &str) -> Option<String> {
        lock(&self.names).get(id).cloned()
    }

    /// The author a commit made on this computer records: the host
    /// participant while hosting, else None.
    pub(crate) fn local_author(&self) -> Option<Id> {
        match self.role() {
            Role::Host(host) => Some(host.me.id.clone()),
            _ => None,
        }
    }

    /// While hosting: who made the change that brought the document to
    /// `revision`, for the document frame the guests get.
    pub(crate) fn note_change(&self, revision: u32, by: Option<&Id>) {
        if let (Role::Host(host), Some(by)) = (self.role(), by) {
            host.note_change(revision, by);
        }
    }

    /// While hosting, undo and redo take back or bring back someone else's
    /// step only with `force` (docs/CONTRACT.md, "Live sessions").
    /// `requester` is the guest asking; None is this computer. Outside a
    /// session every step is yours.
    pub(crate) fn check_step(&self, doc: &Document, redo: bool, requester: Option<&Id>) -> Result<(), IpcError> {
        let Role::Host(host) = self.role() else {
            return Ok(());
        };
        let state = doc.state();
        let (possible, by, label) = if redo {
            (state.can_redo, state.redo_by, state.redo_label)
        } else {
            (state.can_undo, state.undo_by, state.undo_label)
        };
        if !possible {
            // Nothing there: the engine says so.
            return Ok(());
        }
        // A step with no author was made here before the session started.
        let owner = by.unwrap_or_else(|| host.me.id.clone());
        let requester = requester.unwrap_or(&host.me.id);
        if owner == *requester {
            return Ok(());
        }
        let name = self.name_of(&owner).unwrap_or_else(|| "Someone else".to_string());
        let label = label.unwrap_or_else(|| "a change".to_string());
        let message = if redo {
            format!("This step is {name}'s: {label}. Redo it anyway?")
        } else {
            format!("{name} made the last change: {label}. Undo it anyway?")
        };
        Err(IpcError::new("other_author", message))
    }
}

// ------------------------------------------------------------------ errors

pub(crate) fn not_live(message: &str) -> IpcError {
    IpcError::new("not_live", message)
}

pub(crate) fn host_only(message: &str) -> IpcError {
    IpcError::new("host_only", message)
}

pub(crate) fn live_lost() -> IpcError {
    IpcError::new(
        "live_lost",
        "The connection to the host dropped. Guhit Studio is reconnecting; try again in a moment.",
    )
}

fn bad_args(message: &str) -> IpcError {
    IpcError::new("bad_args", message)
}

// ------------------------------------------------------------------ helpers

fn settings_of(dir: &Path) -> serde_json::Map<String, Value> {
    std::fs::read_to_string(dir.join(SETTINGS_FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// Merge one key into `settings.json`; other parts of the app keep their own
/// keys in the same file.
fn put_setting(dir: &Path, key: &str, value: Value) -> Result<(), IpcError> {
    let mut settings = settings_of(dir);
    settings.insert(key.to_string(), value);
    files::create_dir(dir)?;
    files::write_json_atomic(&dir.join(SETTINGS_FILE), &Value::Object(settings))
}

/// A display name as people will see it: control characters removed, spaces
/// trimmed, at most `MAX_NAME_CHARS` characters.
pub fn clean_name(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_NAME_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

fn is_short_id(id: &str) -> bool {
    id.chars().count() <= MAX_ID_CHARS
}

/// A presence as it may travel: cursor chat and the selection cut to their
/// limits, a cursor with a coordinate that is not a number dropped, ids that
/// are too long to be ids dropped.
pub fn clean_presence(mut presence: Presence) -> Presence {
    if let Some(p) = presence.cursor {
        if !p.x.is_finite() || !p.y.is_finite() {
            presence.cursor = None;
        }
    }
    if let Some(text) = presence.typing.as_mut() {
        *text = text.chars().filter(|c| !c.is_control()).take(MAX_CURSOR_CHAT_CHARS).collect();
    }
    if presence.level_id.as_deref().is_some_and(|id| !is_short_id(id)) {
        presence.level_id = None;
    }
    presence.selection.retain(|id| is_short_id(id));
    presence.selection.truncate(MAX_PRESENCE_SELECTION);
    presence
}

/// A chat message's text as it is kept: control characters other than line
/// breaks and tabs removed, trimmed, 1 to `MAX_CHAT_CHARS` characters, or 1
/// to `MAX_CURSOR_CHAT_CHARS` for cursor chat (`at` set). `bad_args`
/// otherwise, as for a cursor that is not a number or a level id that is not
/// an id.
pub(crate) fn check_chat(text: &str, at: Option<Point>, level_id: Option<&str>) -> Result<String, IpcError> {
    let text: String = text
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect::<String>()
        .trim()
        .to_string();
    let count = text.chars().count();
    if count == 0 {
        return Err(bad_args("A chat message needs some text."));
    }
    match at {
        Some(p) if !p.x.is_finite() || !p.y.is_finite() => {
            return Err(bad_args("A cursor chat message needs a place on the plan."))
        }
        Some(_) if count > MAX_CURSOR_CHAT_CHARS => {
            return Err(bad_args(&format!(
                "A cursor chat message can have at most {MAX_CURSOR_CHAT_CHARS} characters. Use the chat panel for longer ones."
            )))
        }
        None if count > MAX_CHAT_CHARS => {
            return Err(bad_args(&format!("A chat message can have at most {MAX_CHAT_CHARS} characters.")))
        }
        _ => {}
    }
    if level_id.is_some_and(|id| files::check_id(id, "level").is_err()) {
        return Err(bad_args("That level id is not an id."));
    }
    Ok(text)
}

/// The last `CHAT_HISTORY` messages of `chat.jsonl` in `dir`, oldest first.
/// A line that is not a message is skipped; a missing file is no messages.
pub fn read_chat(dir: &Path) -> Vec<ChatMessage> {
    let Ok(text) = std::fs::read_to_string(dir.join(CHAT_FILE)) else {
        return vec![];
    };
    let mut all: Vec<ChatMessage> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    if all.len() > CHAT_HISTORY {
        all.drain(..all.len() - CHAT_HISTORY);
    }
    all
}

/// Append one message to `chat.jsonl` in `dir`. Best effort: a chat line that
/// cannot be written never fails the session. The line goes out in one write,
/// so two messages never interleave.
pub fn append_chat(dir: &Path, message: &ChatMessage) {
    use std::io::Write;
    let Ok(line) = serde_json::to_string(message) else { return };
    let result = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(CHAT_FILE))
        .and_then(|mut f| f.write_all(format!("{line}\n").as_bytes()));
    if let Err(e) = result {
        eprintln!("guhit-app: chat message not saved: {e}");
    }
}

/// On a guest, the calls the CONTRACT keeps away from the local copy:
/// refused, answered here, or leaving the session. None: handle as usual.
/// Edits, undo, redo and the file calls go to the host from their own
/// handlers.
pub(crate) async fn guest_route(app: &AppService, cmd: &str, args: &Value) -> Option<IpcResult> {
    let Role::Guest(guest) = app.live.role() else {
        return None;
    };
    let shared = || args.get("id").and_then(Value::as_str) == Some(guest.project_id.as_str());
    let refused = |message: &str| Some(Err(host_only(message)));
    match cmd {
        "snapshot_create" | "snapshot_list" | "snapshot_restore" => {
            refused("Versions of a shared project are kept on the host's computer. Use Save a copy to keep your own.")
        }
        "bundle_save" => refused("Only the host can save the shared project as a bundle. Use Save a copy to keep your own."),
        "hub_open" | "hub_create" | "bundle_open" => refused("Leave the live session first."),
        "hub_rename" if shared() => refused("Only the host can rename the shared project."),
        // The window keeps a thumbnail of the plan it shows. The shared
        // project's thumbnail is the host's.
        "hub_set_thumbnail" if shared() => Some(Ok(Value::Null)),
        "hub_close" => {
            guest::leave(app, &guest).await;
            Some(Ok(Value::Null))
        }
        _ => None,
    }
}

/// A live session guest's edit: committed on the host with the guest as its
/// author. `AppService::commit` and `commit_if_revision` route here.
pub(crate) async fn guest_apply(
    app: &AppService,
    command: Command,
    origin: Origin,
    expected_revision: Option<u32>,
) -> Result<ApplyResult, IpcError> {
    guest::apply(app, command, origin, expected_revision).await
}

/// A live session guest's undo or redo, on the host's history.
pub(crate) async fn guest_step(app: &AppService, redo: bool, force: bool) -> Result<DocState, IpcError> {
    guest::step(app, redo, force).await
}

/// On a live session guest: store an underlay or reference model of the
/// shared project on the host. None when this computer is not a guest.
pub(crate) async fn guest_put_file(
    app: &AppService,
    kind: wire::FileKind,
    file_name: &str,
    bytes: &[u8],
) -> Option<IpcResult> {
    let Role::Guest(guest) = app.live.role() else {
        return None;
    };
    Some(guest.put_file(kind, file_name, bytes).await)
}

/// On a live session guest: fetch an underlay or reference model of the
/// shared project from the host. None when this computer is not a guest.
pub(crate) async fn guest_get_file(
    app: &AppService,
    kind: wire::FileKind,
    file_name: &str,
) -> Option<Result<Vec<u8>, IpcError>> {
    let Role::Guest(guest) = app.live.role() else {
        return None;
    };
    Some(guest.get_file(kind, file_name).await)
}

/// After every change of the open document: closing or switching the shared
/// project ends the host's session for everyone.
pub(crate) fn after_doc_change(app: &AppService, project_id: Option<&str>) {
    if let Role::Host(host) = app.live.role() {
        if project_id != Some(host.project_id.as_str()) {
            host::end_for_close(app, &host);
        }
    }
}

impl AppService {
    /// This window's presence, as `presence_set` last stored it. The MCP tool
    /// `get_selection` reads it.
    pub fn local_presence(&self) -> Presence {
        lock(&self.live.local).clone()
    }

    /// The live session as the window sees it.
    pub async fn live_status(&self) -> LiveStatus {
        self.live.status()
    }

    /// The display name, `""` until one is chosen.
    pub async fn profile_name(&self) -> String {
        let dir = self.session.lock().await.data_dir.clone();
        settings_of(&dir)
            .get(PROFILE_KEY)
            .and_then(Value::as_str)
            .map(clean_name)
            .unwrap_or_default()
    }

    /// Everyone else's latest presence in the live session, in the order of
    /// the participant list. Empty without a session.
    pub fn presence_list(&self) -> Vec<PresenceEntry> {
        match self.live.role() {
            Role::Off => vec![],
            Role::Host(host) => host.presence_list(),
            Role::Guest(guest) => guest.presence_list(&self.live.status()),
        }
    }

    /// Send a chat message in the live session as this computer's
    /// participant. `via_ai` marks a message an MCP client sent. `not_live`
    /// without a session.
    pub async fn chat_send(
        &self,
        text: &str,
        at: Option<Point>,
        level_id: Option<Id>,
        via_ai: bool,
    ) -> Result<ChatMessage, IpcError> {
        match self.live.role() {
            Role::Off => Err(not_live("Start or join a live session to chat.")),
            Role::Host(host) => host::chat(self, &host, &host.me.id, text, at, level_id, via_ai),
            Role::Guest(guest) => {
                check_chat(text, at, level_id.as_deref())?;
                guest.chat(text, at, level_id, via_ai).await
            }
        }
    }

    /// The open project's chat, oldest first, at most `CHAT_HISTORY`: on a
    /// guest what the host sent on join and what came since, else
    /// `chat.jsonl` of the project.
    pub async fn chat_list(&self) -> Result<Vec<ChatMessage>, IpcError> {
        if let Role::Guest(guest) = self.live.role() {
            return Ok(guest.chat_history());
        }
        let dir = self.project_dir().await.ok_or_else(crate::no_document)?;
        Ok(read_chat(&dir))
    }

    /// End the session (host) or leave it (guest). Off: nothing to do.
    pub async fn live_leave(&self) -> LiveStatus {
        match self.live.role() {
            Role::Off => {}
            Role::Host(host) => host::end(self, &host, &format!("{} ended the live session.", host.me.name), None),
            Role::Guest(guest) => guest::leave(self, &guest).await,
        }
        self.live.status()
    }

    /// Folder of a guest's copy of a shared project: renders, exports and the
    /// AI log of the session go here (docs/CONTRACT.md, "Storage layout").
    pub fn live_dir(data_dir: &Path, project_id: &str) -> Result<PathBuf, IpcError> {
        files::check_id(project_id, "project id")?;
        Ok(data_dir.join("live").join(project_id))
    }
}

/// Handles `presence_*`, `profile_*`, `live_*` and `chat_*`.
pub async fn handle(app: &AppService, cmd: &str, args: Value) -> IpcResult {
    match cmd {
        "presence_set" => {
            let presence: Presence = arg(&args, "presence")?;
            let presence = clean_presence(presence);
            *lock(&app.live.local) = presence.clone();
            match app.live.role() {
                Role::Off => {}
                Role::Host(host) => host.set_presence(&host.me.id, Some(presence)),
                Role::Guest(guest) => guest.set_presence(presence),
            }
            Ok(Value::Null)
        }
        "presence_list" => to_value(&app.presence_list()),
        "profile_get" => to_value(&Profile { name: app.profile_name().await }),
        "profile_set" => {
            let name: String = arg(&args, "name")?;
            let name = clean_name(&name);
            if name.is_empty() {
                return Err(bad_args("A name needs at least one letter or number."));
            }
            let dir = app.session.lock().await.data_dir.clone();
            put_setting(&dir, PROFILE_KEY, json!(name))?;
            to_value(&Profile { name })
        }
        "live_status" => to_value(&app.live_status().await),
        "live_host" => {
            let port: Option<u16> = arg(&args, "port")?;
            to_value(&host::start(app, port).await?)
        }
        "live_join" => {
            let invite: String = arg(&args, "invite")?;
            to_value(&guest::join(app, &invite).await?)
        }
        "live_leave" => to_value(&app.live_leave().await),
        "live_remove" => {
            let participant_id: String = arg(&args, "participant_id")?;
            to_value(&host::remove(app, &participant_id)?)
        }
        "live_save_copy" => to_value(&guest::save_copy(app).await?),
        "chat_send" => {
            let text: String = arg(&args, "text")?;
            let at: Option<Point> = arg(&args, "at")?;
            let level_id: Option<Id> = arg(&args, "level_id")?;
            to_value(&app.chat_send(&text, at, level_id, false).await?)
        }
        "chat_list" => to_value(&app.chat_list().await?),
        _ => Err(IpcError::new("unknown_command", format!("unknown command `{cmd}`"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  Ana\u{7}  Reyes \n"), "Ana  Reyes");
        assert_eq!(clean_name(&"x".repeat(80)).chars().count(), MAX_NAME_CHARS);
        assert_eq!(clean_name(" \t "), "");
    }

    #[test]
    fn presence_is_cut_to_its_limits() {
        let p = clean_presence(Presence {
            cursor: Some(Point { x: f64::NAN, y: 0.0 }),
            level_id: Some("l".repeat(500)),
            selection: vec!["a".into(); MAX_PRESENCE_SELECTION + 5],
            typing: Some("y".repeat(500)),
            ai_scope: true,
        });
        assert_eq!(p.cursor, None);
        assert_eq!(p.level_id, None);
        assert_eq!(p.selection.len(), MAX_PRESENCE_SELECTION);
        assert_eq!(p.typing.unwrap().chars().count(), MAX_CURSOR_CHAT_CHARS);

        let p = clean_presence(Presence { selection: vec!["w1".into(), "x".repeat(65)], ..Presence::default() });
        assert_eq!(p.selection, vec!["w1".to_string()]);
    }

    #[test]
    fn chat_text_is_checked() {
        assert_eq!(check_chat("  hi\u{7} there \n", None, None).unwrap(), "hi there");
        assert_eq!(check_chat("two\nlines", None, None).unwrap(), "two\nlines");
        let at = Some(Point { x: 1.0, y: 2.0 });
        assert_eq!(check_chat(" \n ", None, None).unwrap_err().code, "bad_args");
        assert!(check_chat(&"x".repeat(MAX_CHAT_CHARS), None, None).is_ok());
        assert_eq!(check_chat(&"x".repeat(MAX_CHAT_CHARS + 1), None, None).unwrap_err().code, "bad_args");
        assert!(check_chat(&"x".repeat(MAX_CURSOR_CHAT_CHARS), at, None).is_ok());
        assert_eq!(check_chat(&"x".repeat(MAX_CURSOR_CHAT_CHARS + 1), at, None).unwrap_err().code, "bad_args");
        assert_eq!(check_chat("hi", Some(Point { x: f64::INFINITY, y: 0.0 }), None).unwrap_err().code, "bad_args");
        assert_eq!(check_chat("hi", None, Some("../x")).unwrap_err().code, "bad_args");
        assert!(check_chat("hi", at, Some("level-1")).is_ok());
    }

    #[test]
    fn chat_history_keeps_the_newest_and_skips_bad_lines() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..(CHAT_HISTORY + 3) {
            append_chat(
                dir.path(),
                &ChatMessage {
                    id: format!("m{i}"),
                    author_id: "a".into(),
                    author_name: "Ana".into(),
                    color: 0,
                    text: format!("hello {i}"),
                    sent_at: "2026-09-25T00:00:00Z".into(),
                    at: None,
                    level_id: None,
                    via_ai: false,
                },
            );
        }
        std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join(CHAT_FILE))
            .and_then(|mut f| std::io::Write::write_all(&mut f, b"not json\n"))
            .unwrap();
        let chat = read_chat(dir.path());
        assert_eq!(chat.len(), CHAT_HISTORY);
        assert_eq!(chat[0].id, "m3");
        assert_eq!(chat.last().unwrap().id, format!("m{}", CHAT_HISTORY + 2));
    }

    #[tokio::test]
    async fn profile_and_presence_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::new_sandboxed(dir.path().to_path_buf());
        let profile = handle(&app, "profile_get", json!({})).await.unwrap();
        assert_eq!(profile, json!({"name": ""}));
        let err = handle(&app, "profile_set", json!({"name": "  "})).await.unwrap_err();
        assert_eq!(err.code, "bad_args");
        handle(&app, "profile_set", json!({"name": " Ana "})).await.unwrap();
        assert_eq!(app.profile_name().await, "Ana");

        let presence = json!({"cursor": {"x": 10.0, "y": 20.0}, "level_id": null, "selection": ["w1"], "typing": null, "ai_scope": true});
        handle(&app, "presence_set", json!({"presence": presence})).await.unwrap();
        let local = app.local_presence();
        assert_eq!(local.selection, vec!["w1".to_string()]);
        assert!(local.ai_scope);

        let err = handle(&app, "chat_send", json!({"text": "hi"})).await.unwrap_err();
        assert_eq!(err.code, "not_live");
        assert_eq!(app.live_status().await.mode, LiveMode::Off);
        assert_eq!(handle(&app, "presence_list", json!({})).await.unwrap(), json!([]));
    }

    #[tokio::test]
    async fn hosting_and_joining_need_a_name_and_the_right_state() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::new_sandboxed(dir.path().to_path_buf());
        let err = handle(&app, "live_host", json!({})).await.unwrap_err();
        assert_eq!((err.code.as_str(), err.message.as_str()), ("bad_args", "Pick the name others will see first."));
        let err = handle(&app, "live_join", json!({"invite": "guhit-live:x"})).await.unwrap_err();
        assert_eq!(err.code, "bad_args");
        handle(&app, "profile_set", json!({"name": "Ana"})).await.unwrap();
        let err = handle(&app, "live_host", json!({})).await.unwrap_err();
        assert_eq!(err.code, "no_document");
        let err = handle(&app, "live_join", json!({"invite": "not an invite"})).await.unwrap_err();
        assert_eq!((err.code.as_str(), err.message.as_str()), ("bad_args", "This is not a Guhit live session invite."));
        let err = handle(&app, "live_remove", json!({"participant_id": "p"})).await.unwrap_err();
        assert_eq!(err.code, "not_live");
        let err = handle(&app, "live_save_copy", json!({})).await.unwrap_err();
        assert_eq!(err.code, "not_live");
        let status: LiveStatus = serde_json::from_value(handle(&app, "live_leave", json!({})).await.unwrap()).unwrap();
        assert_eq!(status.mode, LiveMode::Off);
    }
}
