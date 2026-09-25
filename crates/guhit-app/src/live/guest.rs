//! Joining a live session (docs/CONTRACT.md, "Live sessions"): a read-only
//! copy of the host's project, every edit sent to the host, presence and chat
//! both ways, and reconnecting when the connection drops.
//!
//! One task per join (`run`) owns the connection: it reads what the host
//! sends, and when the connection drops it retries after 1, 2, 4 and 8 s,
//! asking for the same participant id and color back. A connection's writer
//! (`wire::write_loop`) and its presence sender live as long as it does.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use guhit_core::Document;
use guhit_model::*;
use serde_json::Value;
use tokio::io::{AsyncWriteExt, ReadHalf};
use tokio::sync::{mpsc, oneshot, watch};

use super::host::file_limit;
use super::invite::Invite;
use super::tls::{self, ConnectError, GuestTls};
use super::wire::{self, *};
use super::{bad_args, host_only, live_lost, lock, not_live, CopyMeta, Role, CHAT_HISTORY};
use crate::{files, store, AppService};

/// The waits before each attempt to reconnect.
const RECONNECT_DELAYS: [Duration; 4] =
    [Duration::from_secs(1), Duration::from_secs(2), Duration::from_secs(4), Duration::from_secs(8)];
/// The host's answer to a hello may take this long: it waits a moment before
/// turning a wrong secret away.
const WELCOME_TIMEOUT: Duration = Duration::from_secs(10);
/// A request without an answer after this has failed.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// After the host has answered an edit, its new plan has this long to reach
/// the copy. It is sent before the answer, so this is a safety net.
const COPY_WAIT: Duration = Duration::from_secs(5);
/// A leaving guest's bye has this long to go out.
const BYE_WAIT: Duration = Duration::from_secs(2);

pub(crate) struct Guest {
    /// The join this is. The open document is this guest's copy while
    /// `Session::live_copy` carries the same number.
    pub(crate) epoch: u64,
    invite: Invite,
    name: String,
    pub(crate) project_id: Id,
    /// Who this guest is to the host, and the token to be it again after a
    /// dropped connection.
    me: Mutex<Rejoin>,
    /// The current connection. None while reconnecting.
    link: Mutex<Option<Link>>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, IpcError>>>>,
    next_request: AtomicU64,
    next_conn: AtomicU64,
    /// The copy's revision, for a call waiting for its own change.
    copy_revision: watch::Sender<u32>,
    /// This window's presence. Sent to the host at most every
    /// `PRESENCE_EVERY`, latest wins.
    presence: watch::Sender<Presence>,
    /// Everyone else's latest presence.
    others: Mutex<HashMap<Id, Presence>>,
    chat: Mutex<VecDeque<ChatMessage>>,
    /// Set by `live_leave`: stop, and do not reconnect.
    leaving: watch::Sender<bool>,
    ping: Arc<[u8]>,
}

struct Link {
    conn: u64,
    tx: mpsc::Sender<Out>,
}

/// How a connection ended.
enum Ended {
    /// This computer left.
    Left,
    /// The host ended the session for this guest, with this notice.
    ByHost(String),
    /// The connection dropped.
    Lost,
}

enum Greeting {
    Refused(String),
    Failed,
}

fn odd_answer(e: impl std::fmt::Display) -> IpcError {
    IpcError::new("io", format!("The host sent an answer this app does not understand: {e}"))
}

impl Guest {
    fn new(epoch: u64, invite: Invite, name: String, welcome: &Welcome, presence: Presence) -> Self {
        Self {
            epoch,
            invite,
            name,
            project_id: welcome.doc.project.id.clone(),
            me: Mutex::new(Rejoin { id: welcome.participant_id.clone(), token: welcome.rejoin_token.clone() }),
            link: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            next_request: AtomicU64::new(0),
            next_conn: AtomicU64::new(0),
            copy_revision: watch::channel(welcome.doc.revision).0,
            presence: watch::channel(presence).0,
            others: Mutex::new(
                welcome.presence.iter().map(|e| (e.participant_id.clone(), e.presence.clone())).collect(),
            ),
            chat: Mutex::new(welcome.chat.iter().cloned().collect()),
            leaving: watch::channel(false).0,
            ping: wire::shared(&ToHost::Ping, MAX_HELLO_BYTES).unwrap_or_else(|_| Arc::from(&[][..])),
        }
    }

    /// True while this is the session this computer is in.
    fn is_current(self: &Arc<Self>, app: &AppService) -> bool {
        matches!(app.live.role(), Role::Guest(g) if Arc::ptr_eq(&g, self))
    }

    /// Ask the host something and wait for its answer. `live_lost` while
    /// reconnecting.
    async fn request(&self, request: Request) -> Result<Value, IpcError> {
        let id = self.next_request.fetch_add(1, Ordering::Relaxed) + 1;
        let frame = wire::shared(&ToHost::Request { id, request }, MAX_FRAME_BYTES).map_err(|e| match e {
            FrameError::TooBig(_) => IpcError::new("invalid", "This is too large to send to the host."),
            e => IpcError::new("io", e.to_string()),
        })?;
        let (reply_tx, reply_rx) = oneshot::channel();
        // Waiting before the frame goes out, and on the connection as it is
        // now: when it drops, `fail_pending` ends the wait.
        let tx = {
            let link = lock(&self.link);
            let tx = link.as_ref().map(|l| l.tx.clone()).ok_or_else(live_lost)?;
            lock(&self.pending).insert(id, reply_tx);
            tx
        };
        let answer = tokio::time::timeout(REQUEST_TIMEOUT, async {
            tx.send(Out::Frame(frame)).await.map_err(|_| live_lost())?;
            // A dropped sender means the connection dropped.
            reply_rx.await.map_err(|_| live_lost())?
        })
        .await;
        lock(&self.pending).remove(&id);
        answer.unwrap_or_else(|_| Err(IpcError::new("live_lost", "The host did not answer in time. Try again.")))
    }

    /// Every call waiting for the host fails with `live_lost`.
    fn fail_pending(&self) {
        lock(&self.pending).clear();
    }

    /// Wait until the copy has reached `revision`.
    async fn wait_for_copy(&self, revision: u32) -> Result<(), IpcError> {
        let mut copy = self.copy_revision.subscribe();
        let arrived = matches!(tokio::time::timeout(COPY_WAIT, copy.wait_for(|r| *r >= revision)).await, Ok(Ok(_)));
        if arrived {
            return Ok(());
        }
        Err(IpcError::new(
            "live_lost",
            "The host made the change, but the new plan did not arrive. It shows once the connection recovers.",
        ))
    }

    pub(crate) fn set_presence(&self, presence: Presence) {
        self.presence.send_replace(presence);
    }

    /// Everyone else's latest presence, in the order of the participants.
    pub(crate) fn presence_list(&self, status: &LiveStatus) -> Vec<PresenceEntry> {
        let others = lock(&self.others);
        status
            .participants
            .iter()
            .filter_map(|p| {
                let presence = others.get(&p.id)?.clone();
                Some(PresenceEntry { participant_id: p.id.clone(), presence })
            })
            .collect()
    }

    pub(crate) fn chat_history(&self) -> Vec<ChatMessage> {
        lock(&self.chat).iter().cloned().collect()
    }

    pub(crate) async fn chat(
        &self,
        text: &str,
        at: Option<Point>,
        level_id: Option<Id>,
        via_ai: bool,
    ) -> Result<ChatMessage, IpcError> {
        let answer = self.request(Request::Chat { text: text.to_string(), at, level_id, via_ai }).await?;
        serde_json::from_value(answer).map_err(odd_answer)
    }

    /// Store a file in the shared project on the host, piece by piece. The
    /// answer is the host's `underlay_store` or `model_store` answer.
    pub(crate) async fn put_file(&self, kind: FileKind, file_name: &str, bytes: &[u8]) -> Result<Value, IpcError> {
        let total = bytes.len() as u64;
        let mut offset = 0;
        loop {
            let end = (offset + FILE_CHUNK).min(bytes.len());
            let request = Request::FilePut {
                kind,
                file_name: file_name.to_string(),
                offset: offset as u64,
                total,
                data: B64.encode(&bytes[offset..end]),
            };
            let answer = self.request(request).await?;
            offset = end;
            if offset >= bytes.len() {
                return Ok(answer);
            }
        }
    }

    /// Fetch a stored underlay or reference model from the host, piece by
    /// piece.
    pub(crate) async fn get_file(&self, kind: FileKind, file_name: &str) -> Result<Vec<u8>, IpcError> {
        let limit = file_limit(kind) as u64;
        let mut bytes = Vec::new();
        loop {
            let request = Request::FileGet { kind, file_name: file_name.to_string(), offset: bytes.len() as u64 };
            let piece: FileChunk = serde_json::from_value(self.request(request).await?).map_err(odd_answer)?;
            if piece.total > limit {
                return Err(IpcError::new("invalid", format!("{file_name} is too large to fetch")));
            }
            let data = B64.decode(piece.data.as_bytes()).map_err(odd_answer)?;
            if data.is_empty() && (bytes.len() as u64) < piece.total {
                return Err(odd_answer("an empty piece"));
            }
            bytes.extend_from_slice(&data);
            if bytes.len() as u64 >= piece.total {
                bytes.truncate(piece.total as usize);
                return Ok(bytes);
            }
        }
    }

    fn presence_arrived(&self, app: &AppService, entries: Vec<PresenceUpdate>) {
        let me = lock(&self.me).id.clone();
        for entry in entries {
            if entry.participant_id == me {
                continue;
            }
            let show = {
                let mut others = lock(&self.others);
                match &entry.presence {
                    Some(p) => {
                        others.insert(entry.participant_id.clone(), p.clone());
                        true
                    }
                    None => others.remove(&entry.participant_id).is_some(),
                }
            };
            if show {
                app.emit(AppEvent::Presence { participant_id: entry.participant_id, presence: entry.presence });
            }
        }
    }

    fn chat_arrived(&self, app: &AppService, message: ChatMessage) {
        {
            let mut chat = lock(&self.chat);
            chat.push_back(message.clone());
            while chat.len() > CHAT_HISTORY {
                chat.pop_front();
            }
        }
        app.emit(AppEvent::Chat { message });
    }

    /// Replace everyone else's presence with `entries`, telling the window
    /// who is gone.
    fn presence_reset(&self, app: &AppService, entries: &[PresenceEntry]) {
        let gone: Vec<Id> = {
            let mut others = lock(&self.others);
            let gone = others.keys().filter(|id| !entries.iter().any(|e| &e.participant_id == *id)).cloned().collect();
            *others = entries.iter().map(|e| (e.participant_id.clone(), e.presence.clone())).collect();
            gone
        };
        for id in gone {
            app.emit(AppEvent::Presence { participant_id: id, presence: None });
        }
        for entry in entries {
            app.emit(AppEvent::Presence {
                participant_id: entry.participant_id.clone(),
                presence: Some(entry.presence.clone()),
            });
        }
    }
}

/// Connect to the first address that answers with the pinned certificate
/// and say hello.
async fn connect(
    invite: &Invite,
    name: &str,
    rejoin: Option<Rejoin>,
) -> Result<(GuestTls, Welcome), IpcError> {
    let mut wrong_host = None;
    for addr in invite.socket_addrs() {
        match tls::connect(addr, &invite.pin).await {
            Ok(mut stream) => match greet(&mut stream, invite, name, rejoin.clone()).await {
                Ok(welcome) => return Ok((stream, welcome)),
                Err(Greeting::Refused(message)) => return Err(IpcError::new("live_refused", message)),
                Err(Greeting::Failed) => {}
            },
            Err(ConnectError::WrongHost) => {
                wrong_host.get_or_insert(addr);
            }
            Err(ConnectError::Unreachable) => {}
        }
    }
    if let Some(addr) = wrong_host {
        return Err(IpcError::new(
            "live_pin",
            format!("The computer at {addr} is not the host this invite is for. Ask for a new invite."),
        ));
    }
    Err(IpcError::new(
        "live_unreachable",
        format!(
            "Could not reach the host at {}. Check that you are on the same network or VPN as the host, and that the host's firewall lets Guhit Studio accept connections.",
            invite.addrs.join(" or ")
        ),
    ))
}

async fn greet(
    stream: &mut GuestTls,
    invite: &Invite,
    name: &str,
    rejoin: Option<Rejoin>,
) -> Result<Welcome, Greeting> {
    let hello = ToHost::Hello { v: PROTOCOL_VERSION, secret: invite.secret.clone(), name: name.to_string(), rejoin };
    let hello = wire::encode(&hello, MAX_HELLO_BYTES).map_err(|_| Greeting::Failed)?;
    let sent = tokio::time::timeout(tls::CONNECT_TIMEOUT, async {
        stream.write_all(&hello).await?;
        stream.flush().await
    })
    .await;
    if !matches!(sent, Ok(Ok(()))) {
        return Err(Greeting::Failed);
    }
    // The welcome carries the whole plan.
    let bytes = wire::read_frame(stream, MAX_FRAME_BYTES, WELCOME_TIMEOUT, BODY_TIMEOUT)
        .await
        .map_err(|_| Greeting::Failed)?;
    match wire::decode::<ToGuest>(&bytes) {
        Ok(ToGuest::Welcome(welcome)) => Ok(*welcome),
        Ok(ToGuest::Refused { message }) => Err(Greeting::Refused(message)),
        _ => Err(Greeting::Failed),
    }
}

fn status_of(guest: &Guest, welcome: &Welcome) -> LiveStatus {
    LiveStatus {
        mode: LiveMode::Joined,
        self_id: Some(welcome.participant_id.clone()),
        participants: welcome.participants.clone(),
        invite: None,
        addresses: vec![],
        project_id: Some(guest.project_id.clone()),
        project_name: Some(welcome.doc.project.name.clone()),
        notice: None,
    }
}

/// `live_join`: open the project an invite shares, closing the open one.
pub(crate) async fn join(app: &AppService, invite: &str) -> Result<DocState, IpcError> {
    let name = app.profile_name().await;
    if name.is_empty() {
        return Err(bad_args("Pick the name others will see first."));
    }
    match app.live.role() {
        Role::Host(_) => return Err(host_only("End your live session first.")),
        Role::Guest(_) => return Err(host_only("Leave the live session first.")),
        Role::Off => {}
    }
    let invite = Invite::decode(invite)?;
    let (stream, welcome) = match connect(&invite, &name, None).await {
        Ok(joined) => joined,
        Err(e) => {
            // Why joining failed stays in the status until the next session.
            let status = app.live.edit_status(|s| {
                if s.mode == LiveMode::Off {
                    s.notice = Some(e.message.clone());
                }
            });
            app.emit(AppEvent::Live { status });
            return Err(e);
        }
    };
    let epoch = app.live.epochs.fetch_add(1, Ordering::Relaxed) + 1;
    let guest = Arc::new(Guest::new(epoch, invite, name, &welcome, app.local_presence()));
    let status = status_of(&guest, &welcome);
    let copy = Document::with_revision(welcome.doc.project.clone(), welcome.doc.revision);
    let meta = CopyMeta { epoch, seq: welcome.doc.seq, undo: welcome.doc.undo.clone() };
    // Requests can go out as soon as the session is visible.
    let conn = open(&guest, stream);
    let installed = {
        let mut s = app.session.lock().await;
        let installed = {
            let mut role = lock(&app.live.role);
            match &*role {
                Role::Off => s.install_copy(copy, meta).map(|()| {
                    *role = Role::Guest(guest.clone());
                    app.live.set_status(status.clone());
                }),
                Role::Host(_) => Err(host_only("End your live session first.")),
                Role::Guest(_) => Err(host_only("Leave the live session first.")),
            }
        };
        installed.and_then(|()| {
            app.notify(&s);
            s.state()
        })
    };
    let state = match installed {
        Ok(state) => state,
        Err(e) => {
            conn.kill.send_replace(true);
            return Err(e);
        }
    };
    for p in &welcome.participants {
        app.live.remember_name(&p.id, &p.name);
    }
    app.emit(AppEvent::Live { status });
    for entry in &welcome.presence {
        app.emit(AppEvent::Presence { participant_id: entry.participant_id.clone(), presence: Some(entry.presence.clone()) });
    }
    tokio::spawn(run(app.clone(), guest, conn));
    Ok(state)
}

/// One connection to the host, as its reader needs it. Its writer and
/// presence sender run on their own and stop when `kill` is set.
struct Conn {
    id: u64,
    rd: ReadHalf<GuestTls>,
    kill: watch::Sender<bool>,
}

/// Start using a connection: from now on requests and presence go out on it.
fn open(guest: &Arc<Guest>, stream: GuestTls) -> Conn {
    let (rd, wr) = tokio::io::split(stream);
    let (tx, rx) = mpsc::channel(QUEUE_FRAMES);
    let kill = watch::channel(false).0;
    let id = guest.next_conn.fetch_add(1, Ordering::Relaxed) + 1;
    *lock(&guest.link) = Some(Link { conn: id, tx: tx.clone() });
    tokio::spawn(wire::write_loop(wr, rx, kill.clone(), guest.ping.clone()));
    tokio::spawn(send_presence(guest.clone(), tx, kill.subscribe()));
    Conn { id, rd, kill }
}

/// The life of one join: serve a connection, reconnect when it drops, and
/// close the copy when the session is over.
async fn run(app: AppService, guest: Arc<Guest>, mut conn: Conn) {
    loop {
        match serve(&app, &guest, conn).await {
            Ended::Left => return,
            Ended::ByHost(notice) => return finish(&app, &guest, Some(notice)).await,
            Ended::Lost => {}
        }
        if *guest.leaving.borrow() || !guest.is_current(&app) {
            return;
        }
        let status = app.live.edit_status(|s| s.mode = LiveMode::Reconnecting);
        app.emit(AppEvent::Live { status });
        match reconnect(&guest).await {
            Ok((stream, welcome)) => match rejoined(&app, &guest, stream, welcome).await {
                Some(next) => conn = next,
                None => return,
            },
            Err(None) => return,
            Err(Some(notice)) => return finish(&app, &guest, Some(notice)).await,
        }
    }
}

/// Retry after each of `RECONNECT_DELAYS`, asking for the same participant
/// back. `Err(None)`: this computer left meanwhile.
async fn reconnect(guest: &Guest) -> Result<(GuestTls, Welcome), Option<String>> {
    let mut leaving = guest.leaving.subscribe();
    let mut refusal = None;
    for delay in RECONNECT_DELAYS {
        tokio::select! {
            _ = leaving.wait_for(|l| *l) => return Err(None),
            _ = tokio::time::sleep(delay) => {}
        }
        let rejoin = lock(&guest.me).clone();
        match connect(&guest.invite, &guest.name, Some(rejoin)).await {
            Ok(back) => return Ok(back),
            // A host that answers and turns this guest away will not change
            // its mind.
            Err(e) if e.code == "live_refused" => {
                refusal = Some(e.message);
                break;
            }
            Err(_) => {}
        }
    }
    let lost = "Lost the connection to the host.";
    Err(Some(match refusal {
        Some(why) => format!("{lost} {why}"),
        None => lost.to_string(),
    }))
}

/// Back in after a dropped connection: the host's current plan, the
/// participant it gave back, everyone's presence and the chat.
async fn rejoined(
    app: &AppService,
    guest: &Arc<Guest>,
    stream: GuestTls,
    welcome: Welcome,
) -> Option<Conn> {
    let copy = Document::with_revision(welcome.doc.project.clone(), welcome.doc.revision);
    let (conn, status) = {
        let mut s = app.session.lock().await;
        if !guest.is_current(app) {
            return None;
        }
        let meta = s.live_copy.as_mut().filter(|c| c.epoch == guest.epoch)?;
        meta.seq = welcome.doc.seq;
        meta.undo = welcome.doc.undo.clone();
        s.doc = Some(copy);
        *lock(&guest.me) = Rejoin { id: welcome.participant_id.clone(), token: welcome.rejoin_token.clone() };
        // Requests can go out again before anyone hears the guest is back.
        let conn = open(guest, stream);
        let status = app.live.edit_status(|st| *st = status_of(guest, &welcome));
        app.notify(&s);
        (conn, status)
    };
    guest.copy_revision.send_replace(welcome.doc.revision);
    for p in &welcome.participants {
        app.live.remember_name(&p.id, &p.name);
    }
    guest.presence_reset(app, &welcome.presence);
    *lock(&guest.chat) = welcome.chat.into_iter().collect();
    app.emit(AppEvent::Live { status });
    Some(conn)
}

/// Read what the host sends on one connection until it ends.
async fn serve(app: &AppService, guest: &Arc<Guest>, conn: Conn) -> Ended {
    let Conn { id: conn, mut rd, kill } = conn;
    let mut stop = kill.subscribe();
    let mut leaving = guest.leaving.subscribe();
    let ended = loop {
        let read = tokio::select! {
            _ = leaving.wait_for(|l| *l) => break Ended::Left,
            _ = stop.wait_for(|k| *k) => break Ended::Lost,
            read = wire::read_frame(&mut rd, MAX_FRAME_BYTES, IDLE_TIMEOUT, BODY_TIMEOUT) => read,
        };
        let Ok(bytes) = read else { break Ended::Lost };
        let Ok(message) = wire::decode::<ToGuest>(&bytes) else { break Ended::Lost };
        match message {
            ToGuest::Doc(frame) => apply_doc(app, guest, *frame).await,
            ToGuest::Reply { id, ok, error } => {
                let result = match (ok, error) {
                    (_, Some(e)) => Err(e),
                    (ok, None) => Ok(ok.unwrap_or(Value::Null)),
                };
                if let Some(waiting) = lock(&guest.pending).remove(&id) {
                    let _ = waiting.send(result);
                }
            }
            ToGuest::Presence { entries } => guest.presence_arrived(app, entries),
            ToGuest::Participants { participants } => participants_changed(app, guest, participants),
            ToGuest::Chat { message } => guest.chat_arrived(app, message),
            ToGuest::Ping => {}
            ToGuest::End { notice } => break Ended::ByHost(notice),
            ToGuest::Welcome(_) | ToGuest::Refused { .. } => break Ended::Lost,
        }
    };
    {
        let mut link = lock(&guest.link);
        if link.as_ref().is_some_and(|l| l.conn == conn) {
            *link = None;
        }
    }
    if matches!(ended, Ended::Left) {
        // The writer sends the bye, closes, and stops.
        let _ = tokio::time::timeout(BYE_WAIT, stop.wait_for(|k| *k)).await;
    }
    kill.send_replace(true);
    guest.fail_pending();
    ended
}

/// This window's presence, at most every `PRESENCE_EVERY`, latest wins. A new
/// connection starts with the current one.
async fn send_presence(guest: Arc<Guest>, tx: mpsc::Sender<Out>, mut stop: watch::Receiver<bool>) {
    let mut presence = guest.presence.subscribe();
    presence.mark_changed();
    let mut last: Option<Instant> = None;
    loop {
        tokio::select! {
            _ = stop.wait_for(|k| *k) => return,
            changed = presence.changed() => if changed.is_err() { return },
        }
        let wait = last.map(|t| (t + PRESENCE_EVERY).saturating_duration_since(Instant::now()));
        if let Some(wait) = wait.filter(|w| !w.is_zero()) {
            tokio::select! {
                _ = stop.wait_for(|k| *k) => return,
                _ = tokio::time::sleep(wait) => {}
            }
        }
        let latest = presence.borrow_and_update().clone();
        if let Ok(frame) = wire::shared(&ToHost::Presence { presence: latest }, MAX_FRAME_BYTES) {
            // A full queue skips this one; the next change is sent anyway.
            let _ = tx.try_send(Out::Frame(frame));
        }
        last = Some(Instant::now());
    }
}

/// A document frame: replace the copy, unless it is older than the copy.
async fn apply_doc(app: &AppService, guest: &Guest, frame: DocFrame) {
    let revision = frame.revision;
    let copy = Document::with_revision(frame.project, frame.revision);
    {
        let mut s = app.session.lock().await;
        let Some(meta) = s.live_copy.as_mut().filter(|c| c.epoch == guest.epoch && frame.seq > c.seq) else {
            return;
        };
        meta.seq = frame.seq;
        meta.undo = frame.undo;
        s.doc = Some(copy);
        app.notify(&s);
    }
    guest.copy_revision.send_replace(revision);
}

fn participants_changed(app: &AppService, guest: &Arc<Guest>, participants: Vec<Participant>) {
    if !guest.is_current(app) {
        return;
    }
    for p in &participants {
        app.live.remember_name(&p.id, &p.name);
    }
    let gone: Vec<Id> = {
        let mut others = lock(&guest.others);
        let gone: Vec<Id> = others.keys().filter(|id| !participants.iter().any(|p| &p.id == *id)).cloned().collect();
        for id in &gone {
            others.remove(id);
        }
        gone
    };
    let status = app.live.edit_status(|s| s.participants = participants);
    for id in gone {
        app.emit(AppEvent::Presence { participant_id: id, presence: None });
    }
    app.emit(AppEvent::Live { status });
}

/// The session is over for this guest: status off with `notice`, the copy
/// closed (kept for `live_save_copy`), the window back to the hub.
async fn finish(app: &AppService, guest: &Arc<Guest>, notice: Option<String>) {
    let status = LiveStatus { notice, ..LiveStatus::off() };
    {
        let mut role = lock(&app.live.role);
        if !matches!(&*role, Role::Guest(g) if Arc::ptr_eq(g, guest)) {
            return;
        }
        *role = Role::Off;
        app.live.set_status(status.clone());
    }
    guest.fail_pending();
    {
        let mut s = app.session.lock().await;
        if s.live_copy.as_ref().is_some_and(|c| c.epoch == guest.epoch) {
            s.close_copy();
            app.notify(&s);
        }
    }
    let gone: Vec<Id> = lock(&guest.others).drain().map(|(id, _)| id).collect();
    for id in gone {
        app.emit(AppEvent::Presence { participant_id: id, presence: None });
    }
    app.emit(AppEvent::Live { status });
}

/// `live_leave` and `hub_close` on a guest: bye, then the copy closes.
pub(crate) async fn leave(app: &AppService, guest: &Arc<Guest>) {
    guest.leaving.send_replace(true);
    let link = lock(&guest.link).take();
    if let Some(link) = link {
        if let Ok(bye) = wire::shared(&ToHost::Bye, MAX_HELLO_BYTES) {
            let _ = link.tx.try_send(Out::Frame(bye));
        }
        let _ = link.tx.try_send(Out::Close);
    }
    finish(app, guest, None).await;
}

/// A guest's edit: committed on the host with this guest as its author, and
/// answered with the copy once the change has arrived in it.
pub(crate) async fn apply(
    app: &AppService,
    command: Command,
    origin: Origin,
    expected_revision: Option<u32>,
) -> Result<ApplyResult, IpcError> {
    let Role::Guest(guest) = app.live.role() else {
        return Err(live_lost());
    };
    let answer = guest.request(Request::Apply { command, origin, expected_revision }).await?;
    let applied: Applied = serde_json::from_value(answer).map_err(odd_answer)?;
    guest.wait_for_copy(applied.revision).await?;
    let state = app.session.lock().await.state()?;
    Ok(ApplyResult { state, diff: applied.diff })
}

/// A guest's undo or redo, on the host's one shared history.
pub(crate) async fn step(app: &AppService, redo: bool, force: bool) -> Result<DocState, IpcError> {
    let Role::Guest(guest) = app.live.role() else {
        return Err(live_lost());
    };
    let request = if redo { Request::Redo { force } } else { Request::Undo { force } };
    let stepped: Stepped = serde_json::from_value(guest.request(request).await?).map_err(odd_answer)?;
    guest.wait_for_copy(stepped.revision).await?;
    app.session.lock().await.state()
}

/// `live_save_copy`: the shared project, as it was last seen, as a new local
/// project. While the session runs, the underlay images and reference models
/// the plan uses come along (best effort).
pub(crate) async fn save_copy(app: &AppService) -> Result<ProjectMeta, IpcError> {
    let (project, data_dir) = {
        let s = app.session.lock().await;
        let project = match &s.live_copy {
            Some(_) => s.doc.as_ref().map(|d| d.project().clone()),
            None => s.last_shared.clone(),
        };
        let project =
            project.ok_or_else(|| not_live("There is no shared project to save. Join a live session first."))?;
        (project, s.data_dir.clone())
    };
    let shared_id = project.id.clone();
    let mut copy = project;
    copy.id = defaults::new_id();
    copy.name = format!("{} (copy)", copy.name);
    let now = defaults::now_rfc3339();
    copy.created_at = now.clone();
    copy.updated_at = now;
    store::save_project(&data_dir, &copy)?;
    if let Role::Guest(guest) = app.live.role() {
        if guest.project_id == shared_id {
            copy_files(&guest, &store::project_dir(&data_dir, &copy.id)?, &copy).await;
        }
    }
    store::load_meta(&data_dir, &copy.id)
}

/// Fetch the files the plan points at into a saved copy. One that cannot be
/// fetched is skipped: the copy is still a good plan without it.
async fn copy_files(guest: &Guest, dir: &Path, project: &Project) {
    let mut wanted: Vec<(FileKind, &str, &str)> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Underlay(u) => Some((FileKind::Underlay, "underlays", u.file_name.as_str())),
            Element::ReferenceModel(m) => Some((FileKind::Model, "models", m.file_name.as_str())),
            _ => None,
        })
        .filter(|(_, _, name)| files::check_file_name(name).is_ok())
        .collect();
    wanted.sort_by(|a, b| (a.1, a.2).cmp(&(b.1, b.2)));
    wanted.dedup_by(|a, b| (a.1, a.2) == (b.1, b.2));
    for (kind, folder, name) in wanted {
        let copied = match guest.get_file(kind, name).await {
            Ok(bytes) => files::write_atomic(&dir.join(folder).join(name), &bytes),
            Err(e) => Err(e),
        };
        if let Err(e) = copied {
            eprintln!("guhit-app: {name} was not copied from the host: {}", e.message);
        }
    }
}
