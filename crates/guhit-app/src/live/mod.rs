//! Live sessions (DECISIONS D29, docs/CONTRACT.md "Live sessions"): one
//! computer hosts the open project, others join with an invite and work on it
//! together, with presence (pointer, selection, cursor chat) and chat.
//!
//! - `host`: the listener, each guest's connection, and sending every change
//!   of the host's document, presence and chat to everyone.
//! - `guest`: joining (every way in at once, the first to answer wins), the
//!   read-only copy, reconnecting, and sending edits to the host.
//! - `relay`: reaching the host over the internet through the relay
//!   (DECISIONS D32, docs/RELAY.md), for guests outside its network or VPN.
//! - `wire`: frames and messages. `invite`: the invite. `tls`: the session
//!   certificate and the pinned connection.
//!
//! This module also owns this window's own presence (`presence_set`), which
//! the MCP tool `get_selection` reads with or without a session, and the
//! display name (`profile_get`, `profile_set`).

mod guest;
mod host;
pub mod invite;
pub mod relay;
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
/// `chat.jsonl` past this size is cut to its newest messages: at most
/// `CHAT_KEEP`, in at most half this size, so it is not cut again soon.
pub const CHAT_MAX_BYTES: u64 = 2 * 1024 * 1024;
/// Most messages `chat.jsonl` keeps when it is cut.
pub const CHAT_KEEP: usize = 1000;
/// Whom an agent's step worked for when this computer's person has no
/// name yet.
const OWN_PERSON: &str = "this computer";
/// One chat file write at a time in this process, so cutting the file never
/// loses a message appended meanwhile.
static CHAT_WRITE: Mutex<()> = Mutex::new(());

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
    /// Name and color of everyone this computer met in a live session, by
    /// participant id, so an undo and the history can name who made a step
    /// after they left (DECISIONS D38), as chat messages keep theirs.
    people: Mutex<HashMap<Id, (String, u8)>>,
    /// Every agent this computer met (DECISIONS D47), by participant id:
    /// whom it works for, for `on_behalf_of` in the history. An owner id of
    /// None is this computer's own person.
    agent_owners: Mutex<HashMap<Id, (Option<Id>, String)>>,
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
            people: Mutex::new(HashMap::new()),
            agent_owners: Mutex::new(HashMap::new()),
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
    /// The host's history from the last document frame. None from an older
    /// host, which sends only `undo`.
    pub(crate) history: Option<HistoryList>,
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

    pub(crate) fn remember(&self, p: &Participant) {
        lock(&self.people).insert(p.id.clone(), (p.name.clone(), p.color));
    }

    /// An agent's name and color, and whom it works for: `owner_id` None
    /// is this computer's own person.
    pub(crate) fn remember_agent(&self, p: &Participant, owner_id: Option<Id>, owner_name: &str) {
        self.remember(p);
        lock(&self.agent_owners).insert(p.id.clone(), (owner_id, owner_name.to_string()));
    }

    /// Whom the agent `id` works for (None: this computer's own person).
    /// None when `id` is no agent this computer met.
    pub(crate) fn agent_owner(&self, id: &str) -> Option<Option<Id>> {
        lock(&self.agent_owners).get(id).map(|(owner, _)| owner.clone())
    }

    pub(crate) fn is_agent(&self, id: &str) -> bool {
        lock(&self.agent_owners).contains_key(id)
    }

    pub(crate) fn name_of(&self, id: &str) -> Option<String> {
        lock(&self.people).get(id).map(|(name, _)| name.clone())
    }

    /// Fill in the names and colors of the steps' authors. A step with no
    /// author is `unowned`'s when one is given: the host's, made before the
    /// session. Then a step whose author is `own` (this computer) loses it:
    /// no author means this computer's own, in and out of a session. Someone
    /// this computer never met keeps the name and color the step came with:
    /// a guest who joined late still sees who left before it came.
    ///
    /// An agent's step names the person it worked for (`on_behalf_of`,
    /// DECISIONS D47): `this` is this computer's person, for its own
    /// agents, and `own` loses it the same way.
    fn name_steps(&self, list: &mut HistoryList, own: Option<&Id>, unowned: Option<&Id>, this: Option<&Id>) {
        let people = lock(&self.people);
        let owners = lock(&self.agent_owners);
        for step in list.undo.iter_mut().chain(list.redo.iter_mut()) {
            let author = step.author_id.take().or_else(|| unowned.cloned());
            // An agent's step is always marked as one, in and out of a
            // session: the person it worked for by id and name, this
            // computer's own included, so it never reads as a person's.
            if let Some((owner_id, owner_name)) = author.as_ref().and_then(|a| owners.get(a)) {
                step.on_behalf_of = owner_id.clone().or_else(|| this.cloned());
                let name = if owner_name.is_empty() { OWN_PERSON } else { owner_name.as_str() };
                step.on_behalf_name = Some(name.to_string());
            }
            step.author_id = author.filter(|id| Some(id) != own);
            let Some(id) = step.author_id.as_ref() else {
                step.author_name = None;
                step.color = None;
                continue;
            };
            if let Some((name, color)) = people.get(id) {
                step.author_name = Some(name.clone());
                step.color = Some(*color);
            }
        }
    }

    /// The open document's history as this window shows it (`history_list`):
    /// on the host or with no session the document's own, on a guest the
    /// host's from the last document frame.
    pub(crate) fn history(&self, doc: &Document, copy: Option<&CopyMeta>) -> HistoryList {
        let Some(copy) = copy else {
            let mut list = doc.history();
            // This computer's person has the id it hosts with, session or not.
            let me = self.host_id.get_or_init(defaults::new_id).clone();
            self.name_steps(&mut list, Some(&me), None, Some(&me));
            return list;
        };
        let mut list = match &copy.history {
            Some(list) => list.clone(),
            None => top_steps(&copy.undo),
        };
        let status = self.status();
        let host = status.participants.iter().find(|p| p.role == ParticipantRole::Host).map(|p| &p.id);
        self.name_steps(&mut list, status.self_id.as_ref(), host, status.self_id.as_ref());
        list
    }

    /// The history a document frame carries to guests: names and colors
    /// filled in, the host's steps from before the session named as its.
    pub(crate) fn shared_history(&self, doc: &Document, host: &Id) -> HistoryList {
        let mut list = doc.history();
        self.name_steps(&mut list, None, Some(host), Some(host));
        list
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
    /// step only with `force` (docs/CONTRACT.md, "Live sessions"); for
    /// several `steps`, when any of them is someone else's, naming everyone
    /// ("Takes back 4 steps, 2 by Ana. Undo anyway?"). `requester` is the
    /// guest asking; None is this computer. Outside a session every step is
    /// yours.
    pub(crate) fn check_step(
        &self,
        doc: &Document,
        redo: bool,
        steps: usize,
        requester: Option<&Id>,
    ) -> Result<(), IpcError> {
        let Role::Host(host) = self.role() else {
            return Ok(());
        };
        let list = doc.history();
        let stack = if redo { &list.redo } else { &list.undo };
        if steps == 0 || steps > stack.len() {
            // Not there to take: the engine says so.
            return Ok(());
        }
        let requester = requester.unwrap_or(&host.me.id);
        // Each other person and how many of the steps are theirs, newest
        // first. A step with no author was made here before the session.
        let mut theirs: Vec<(Id, usize)> = vec![];
        for step in &stack[..steps] {
            let owner = step.author_id.as_ref().unwrap_or(&host.me.id);
            if owner == requester {
                continue;
            }
            // A step by the requester's own agent is the requester's: your
            // own agent's steps never ask you (DECISIONS D47).
            if self.agent_owner(owner).is_some_and(|o| o.as_ref().unwrap_or(&host.me.id) == requester) {
                continue;
            }
            match theirs.iter_mut().find(|(id, _)| id == owner) {
                Some((_, n)) => *n += 1,
                None => theirs.push((owner.clone(), 1)),
            }
        }
        if theirs.is_empty() {
            return Ok(());
        }
        let name = |id: &Id| self.name_of(id).unwrap_or_else(|| "someone else".to_string());
        let message = if steps == 1 {
            let name = self.name_of(&theirs[0].0).unwrap_or_else(|| "Someone else".to_string());
            let label = if stack[0].label.is_empty() { "a change" } else { stack[0].label.as_str() };
            if redo {
                format!("This step is {name}'s: {label}. Redo it anyway?")
            } else {
                format!("{name} made the last change: {label}. Undo it anyway?")
            }
        } else {
            let whose = match theirs.as_slice() {
                [(id, n)] if *n == steps => format!("all by {}", name(id)),
                _ => {
                    let parts: Vec<String> = theirs.iter().map(|(id, n)| format!("{n} by {}", name(id))).collect();
                    join_and(&parts)
                }
            };
            if redo {
                format!("Brings back {steps} steps, {whose}. Redo anyway?")
            } else {
                format!("Takes back {steps} steps, {whose}. Undo anyway?")
            }
        };
        Err(IpcError::new("other_author", message))
    }
}

/// "a", "a and b", "a, b and c".
fn join_and(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// What an older host's document frame says of its history: the step on top
/// of each stack, without its time, summary or elements.
fn top_steps(undo: &wire::UndoMeta) -> HistoryList {
    let step = |label: &Option<String>, by: &Option<Id>| HistoryStep {
        label: label.clone().unwrap_or_default(),
        summary: String::new(),
        origin: Origin::User,
        author_id: by.clone(),
        author_name: None,
        color: None,
        at: String::new(),
        ids: vec![],
        removed: vec![],
        on_behalf_of: None,
        on_behalf_name: None,
    };
    HistoryList {
        undo: undo.can_undo.then(|| step(&undo.undo_label, &undo.undo_by)).into_iter().collect(),
        redo: undo.can_redo.then(|| step(&undo.redo_label, &undo.redo_by)).into_iter().collect(),
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
    files::read_settings(dir)
}

fn put_setting(dir: &Path, key: &str, value: Value) -> Result<(), IpcError> {
    files::put_setting(dir, key, Some(value))
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
    // Follow mode (DECISIONS D37): ids that are ids, finite numbers rounded
    // to 1 mm, a plan area of positive size, a field of view of 5 to 110.
    if presence.following.as_deref().is_some_and(|id| !is_short_id(id)) {
        presence.following = None;
    }
    presence.plan_view = presence.plan_view.and_then(|v| {
        let ok = v.center.x.is_finite() && v.center.y.is_finite() && v.width_mm.is_finite() && v.height_mm.is_finite();
        (ok && v.width_mm > 0.0 && v.height_mm > 0.0).then(|| PlanView {
            center: Point { x: v.center.x.round(), y: v.center.y.round() },
            width_mm: v.width_mm.round().max(1.0),
            height_mm: v.height_mm.round().max(1.0),
        })
    });
    let finite = |p: &Vec3| p.x.is_finite() && p.y.is_finite() && p.z.is_finite();
    let round = |p: Vec3| Vec3 { x: p.x.round(), y: p.y.round(), z: p.z.round() };
    presence.camera = presence.camera.and_then(|c| {
        (finite(&c.position) && finite(&c.target) && c.fov_deg.is_finite()).then(|| LiveCamera {
            position: round(c.position),
            target: round(c.target),
            fov_deg: c.fov_deg.clamp(5.0, 110.0),
            nav: c.nav,
        })
    });
    presence.pointer_3d = presence.pointer_3d.filter(finite).map(round);
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
/// so two messages never interleave. Once the file passes `CHAT_MAX_BYTES` it
/// is rewritten with its newest `CHAT_KEEP` messages, so agents chatting all
/// day cannot fill the disk.
pub fn append_chat(dir: &Path, message: &ChatMessage) {
    use std::io::Write;
    let Ok(line) = serde_json::to_string(message) else { return };
    let _one = lock(&CHAT_WRITE);
    let path = dir.join(CHAT_FILE);
    let result = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(format!("{line}\n").as_bytes()));
    if let Err(e) = result {
        eprintln!("guhit-app: chat message not saved: {e}");
        return;
    }
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > CHAT_MAX_BYTES) {
        cut_chat(&path);
    }
}

/// Keep the newest lines of a chat file: at most `CHAT_KEEP`, in at most
/// half of `CHAT_MAX_BYTES`. Best effort.
fn cut_chat(path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else { return };
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let budget = (CHAT_MAX_BYTES / 2) as usize;
    let (mut start, mut bytes) = (lines.len(), 0usize);
    while start > 0 && lines.len() - start < CHAT_KEEP && bytes + lines[start - 1].len() < budget {
        start -= 1;
        bytes += lines[start].len() + 1;
    }
    let mut out = lines[start..].join("\n");
    out.push('\n');
    if let Err(e) = files::write_atomic(path, out.as_bytes()) {
        eprintln!("guhit-app: the chat could not be cut to its newest messages: {}", e.message);
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
        // No swarms in a live session (D57): the draft stays on one computer.
        // The window still asks what is running, and the answer is nothing.
        "swarm_status" | "swarm_draft" => Some(Ok(Value::Null)),
        c if crate::swarm::OWNS.contains(&c) => Some(Err(crate::swarm::live_session())),
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

/// `guest_apply` for an agent of this computer: the host records `agent` as
/// the author when it knows agents (the "agents" feature); an older host
/// records the guest.
pub(crate) async fn guest_apply_as(
    app: &AppService,
    command: Command,
    origin: Origin,
    expected_revision: Option<u32>,
    agent: Option<Id>,
) -> Result<ApplyResult, IpcError> {
    guest::apply_as(app, command, origin, expected_revision, agent).await
}

/// `guest_step` for an agent of this computer, which checked that the step
/// is its own.
pub(crate) async fn guest_step_as(
    app: &AppService,
    redo: bool,
    force: bool,
    steps: u32,
    expected_revision: Option<u32>,
) -> Result<DocState, IpcError> {
    guest::step(app, redo, force, steps, expected_revision).await
}

/// True on a guest whose host knows agents (its welcome listed "agents").
pub(crate) fn host_knows_agents(app: &AppService) -> bool {
    match app.live.role() {
        Role::Guest(guest) => guest.host_knows_agents(),
        _ => false,
    }
}

/// An agent's chat message, posted as the agent (`author`): in a live
/// session to everyone, without one to the project's `chat.jsonl` and this
/// window (DECISIONS D47). `text` is checked already.
pub(crate) async fn agent_chat(
    app: &AppService,
    author: Participant,
    text: &str,
    at: Option<Point>,
    level_id: Option<Id>,
) -> Result<ChatMessage, IpcError> {
    match app.live.role() {
        Role::Off => {
            let dir = app.project_dir().await.ok_or_else(crate::no_document)?;
            let message = ChatMessage {
                id: defaults::new_id(),
                author_id: author.id,
                author_name: author.name,
                color: author.color,
                text: text.to_string(),
                sent_at: defaults::now_rfc3339(),
                at,
                level_id,
                via_ai: true,
            };
            append_chat(&dir, &message);
            app.emit(AppEvent::Chat { message: message.clone() });
            Ok(message)
        }
        Role::Host(host) => host::chat_by(app, &host, author, text, at, level_id, true),
        Role::Guest(guest) => guest.chat_as(text, at, level_id, author.id).await,
    }
}

/// The agents list changed: the host sends each guest that knows agents
/// everyone's but that guest's own; a guest sends the host its own.
pub(crate) fn send_agents(
    live: &LiveState,
    local: &[AgentSummary],
    remote: &std::collections::BTreeMap<Id, Vec<AgentSummary>>,
) {
    match live.role() {
        Role::Off => {}
        Role::Host(host) => host.send_agents(local, remote),
        Role::Guest(guest) => guest.send_agents(local),
    }
}

/// Agents as another computer shows them, cut to what may travel: agent
/// ids only, clean names and presence, at most `max`. With `owner` (on the
/// host, the guest that sent them) every one is that guest's.
pub(crate) fn clean_agents(list: Vec<AgentSummary>, owner: Option<&Participant>, max: usize) -> Vec<AgentSummary> {
    let cut = |s: &str, n: usize| s.chars().filter(|c| !c.is_control()).take(n).collect::<String>();
    let mut out: Vec<AgentSummary> = vec![];
    for mut a in list {
        let id = a.participant.id.clone();
        let fine = id.starts_with("agent-") && files::check_id(&id, "agent").is_ok();
        if !fine || out.iter().any(|o| o.participant.id == id) {
            continue;
        }
        let Some(info) = a.participant.agent.as_mut() else { continue };
        if let Some(owner) = owner {
            info.owner_id = Some(owner.id.clone());
            info.owner_name = owner.name.clone();
        }
        info.owner_name = clean_name(&info.owner_name);
        info.client = cut(&info.client, 100);
        info.client_version = cut(&info.client_version, 100);
        a.participant.name = clean_name(&a.participant.name);
        if a.participant.name.is_empty() {
            continue;
        }
        a.participant.color %= PEER_COLOR_COUNT;
        a.participant.role = ParticipantRole::Guest;
        a.presence = clean_presence(a.presence);
        a.presence.status = a.presence.status.map(|t| cut(&t, 80));
        a.presence.typing = None;
        if let Some(limit) = a.limit.as_mut() {
            limit.level_ids.truncate(64);
            limit.ids.truncate(MAX_PRESENCE_SELECTION);
            limit.kinds.truncate(32);
        }
        if let Some(claim) = a.claim.as_mut() {
            claim.level_ids.truncate(64);
            claim.ids.truncate(MAX_PRESENCE_SELECTION);
            claim.expires_at = cut(&claim.expires_at, 40);
        }
        a.last_call_at = cut(&a.last_call_at, 40);
        out.push(a);
        if out.len() >= max {
            break;
        }
    }
    out
}

/// A live session guest's undo or redo of `steps` steps, on the host's
/// history, as one request. `expected_revision` makes it fail with `stale`
/// when the plan moved on.
pub(crate) async fn guest_step(
    app: &AppService,
    redo: bool,
    force: bool,
    steps: u32,
    expected_revision: Option<u32>,
) -> Result<DocState, IpcError> {
    guest::step(app, redo, force, steps, expected_revision).await
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
            ..Presence::default()
        });
        assert_eq!(p.cursor, None);
        assert_eq!(p.level_id, None);
        assert_eq!(p.selection.len(), MAX_PRESENCE_SELECTION);
        assert_eq!(p.typing.unwrap().chars().count(), MAX_CURSOR_CHAT_CHARS);

        let p = clean_presence(Presence { selection: vec!["w1".into(), "x".repeat(65)], ..Presence::default() });
        assert_eq!(p.selection, vec!["w1".to_string()]);
    }

    #[test]
    fn follow_fields_are_cleaned() {
        let v = |x: f64, y: f64, z: f64| Vec3 { x, y, z };
        let p = clean_presence(Presence {
            following: Some("x".repeat(65)),
            plan_view: Some(PlanView { center: Point { x: 1000.4, y: -2.6 }, width_mm: 8000.2, height_mm: 0.0 }),
            camera: Some(LiveCamera { position: v(1.4, 2.6, 1600.0), target: v(0.0, 0.0, 0.0), fov_deg: 400.0, nav: NavMode::Walk }),
            pointer_3d: Some(v(f64::INFINITY, 0.0, 0.0)),
            ..Presence::default()
        });
        assert_eq!(p.following, None);
        assert_eq!(p.plan_view, None, "a plan area needs a size");
        let c = p.camera.unwrap();
        assert_eq!((c.position, c.fov_deg, c.nav), (v(1.0, 3.0, 1600.0), 110.0, NavMode::Walk));
        assert_eq!(p.pointer_3d, None);

        let p = clean_presence(Presence {
            following: Some("p1".into()),
            plan_view: Some(PlanView { center: Point { x: 1000.4, y: -2.6 }, width_mm: 8000.2, height_mm: 0.4 }),
            camera: Some(LiveCamera { position: v(f64::NAN, 0.0, 0.0), target: v(0.0, 0.0, 0.0), fov_deg: 50.0, nav: NavMode::Orbit }),
            pointer_3d: Some(v(10.4, 20.6, 0.2)),
            ..Presence::default()
        });
        assert_eq!(p.following.as_deref(), Some("p1"));
        assert_eq!(p.plan_view, Some(PlanView { center: Point { x: 1000.0, y: -3.0 }, width_mm: 8000.0, height_mm: 1.0 }));
        assert_eq!(p.camera, None);
        assert_eq!(p.pointer_3d, Some(v(10.0, 21.0, 0.0)));
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

    #[test]
    fn the_chat_file_keeps_its_newest_messages_once_it_grows_past_its_limit() {
        let dir = tempfile::tempdir().unwrap();
        let message = |i: usize| ChatMessage {
            id: format!("m{i}"),
            author_id: "agent-1".into(),
            author_name: "Ground floor".into(),
            color: 1,
            text: "x".repeat(MAX_CHAT_CHARS),
            sent_at: "2026-11-04T00:00:00Z".into(),
            at: None,
            level_id: None,
            via_ai: true,
        };
        // About 2 KB a message: well past the limit.
        let total = CHAT_MAX_BYTES as usize / 2000 + 50;
        for i in 0..total {
            append_chat(dir.path(), &message(i));
        }
        let size = std::fs::metadata(dir.path().join(CHAT_FILE)).unwrap().len();
        assert!(size <= CHAT_MAX_BYTES, "{size}");
        let text = std::fs::read_to_string(dir.path().join(CHAT_FILE)).unwrap();
        let ids: Vec<String> = text.lines().map(|l| serde_json::from_str::<ChatMessage>(l).unwrap().id).collect();
        assert!(ids.len() <= CHAT_KEEP && !ids.is_empty());
        assert_eq!(ids.last().unwrap(), &format!("m{}", total - 1), "the newest is kept");
        let first: usize = ids[0][1..].parse().unwrap();
        assert_eq!(first + ids.len(), total, "a run of the newest, none lost in between");
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
