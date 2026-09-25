//! Hosting a live session: the listener, the relay registration, each
//! guest's connection, and sending every change of the host's document,
//! presence and chat to everyone (docs/CONTRACT.md, "Live sessions").
//!
//! Tasks of one session: the accept loop (direct connections, unless
//! `live_direct` is false), the relay's control loop and one task per guest
//! it announces (`relay::host_loop`, when there is a relay), the document
//! broadcaster (woken by `AppService::watch_changes`, so a change made by the
//! window, the copilot, an MCP client or a guest goes out the same way), the
//! presence ticker, and per guest a reader (its requests) and a writer
//! (`wire::write_loop`). All of them stop when `Host::shutdown` is set.
//!
//! Locks, outermost first: the session, `Host::chat_order`, `Host::inner`,
//! the live role, the live status. Only the session lock is held across an
//! await, and no lock waits on the network: frames are queued with
//! `try_send`.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::{STANDARD as B64, URL_SAFE_NO_PAD};
use base64::Engine;
use guhit_model::*;
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::{json, Value};
use tokio::io::{AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch, OwnedSemaphorePermit, Semaphore};

use super::invite::{Invite, InviteRelay, INVITE_VERSION, MAX_ADDRS};
use super::relay::{self, RelayUrl, Room};
use super::tls::{BoxIo, HostIdentity, HostTls};
use super::wire::{self, *};
use super::{
    append_chat, bad_args, check_chat, clean_name, clean_presence, host_only, lock, not_live, read_chat, settings_of,
    Role,
};
use crate::files::{self, ImageKind};
use crate::{interop, store, AppService, DocChange};

/// Port a session listens on when settings.json has no `live_port`.
pub const DEFAULT_PORT: u16 = 1460;
/// Ports tried from `live_port` up, before the system picks one.
const PORT_TRIES: u16 = 10;
const PORT_KEY: &str = "live_port";
/// settings.json: the relay's address (docs/RELAY.md). Empty: no relay.
const RELAY_KEY: &str = "live_relay";
/// settings.json: false makes a host take guests through the relay only.
const DIRECT_KEY: &str = "live_direct";
/// The relay this build was made with, for when settings.json does not name
/// one: `GUHIT_RELAY_URL` at compile time. Release builds get the deployed
/// relay this way.
const BUILT_IN_RELAY: Option<&str> = option_env!("GUHIT_RELAY_URL");
/// Most network addresses an invite lists, so loopback still fits in the
/// invite's `MAX_ADDRS`.
const MAX_NETWORK_ADDRS: usize = MAX_ADDRS - 1;
/// Connections still in the TLS handshake or before their hello. More are
/// closed at once.
const MAX_WAITING: usize = 8;
/// A new connection gets this long for the TLS handshake, then again for its
/// hello.
const AUTH_TIMEOUT: Duration = Duration::from_secs(10);
/// A wrong secret is answered after this, so guessing is slow.
const WRONG_SECRET_DELAY: Duration = Duration::from_secs(1);
/// A guest whose connection dropped keeps its id and color this long.
const REJOIN_WINDOW: Duration = Duration::from_secs(60);

pub(crate) struct Host {
    /// The host's own participant: color 0, role host.
    pub(crate) me: Participant,
    pub(crate) project_id: Id,
    data_dir: PathBuf,
    /// Secrets are checked through an HMAC with a key made for the session:
    /// `hmac::verify` compares in constant time.
    key: hmac::Key,
    secret_tag: hmac::Tag,
    acceptor: tokio_rustls::TlsAcceptor,
    /// Set once, when the session ends: every task of it stops.
    shutdown: watch::Sender<bool>,
    inner: Mutex<Inner>,
    /// The last document frame, so a change is encoded once for everyone.
    doc_frame: Mutex<Option<(u64, Arc<[u8]>)>>,
    /// One chat message at a time, so chat.jsonl and every guest see the
    /// same order and a guest joining gets each message exactly once.
    chat_order: Mutex<()>,
    ping: Arc<[u8]>,
    next_conn: AtomicU64,
    /// Connections still authenticating, direct and through the relay
    /// alike: at most `MAX_WAITING`.
    pub(crate) waiting: Arc<Semaphore>,
}

#[derive(Default)]
struct Inner {
    ended: bool,
    /// Everyone in the session: the host first, then in the order they joined.
    participants: Vec<Participant>,
    guests: HashMap<Id, Link>,
    /// Guests whose connection dropped, waiting to come back.
    departed: HashMap<Id, Departed>,
    /// Guests the host removed. They are not let back in as the same
    /// participant, even if their connection dropped before they heard.
    removed: HashSet<Id>,
    /// Everyone's latest presence, the host's included. None: they left.
    presence: HashMap<Id, Slot>,
    presence_version: u64,
    /// Presence versions the host's window has been told about.
    window_seen: HashMap<Id, u64>,
    /// Who made the change that brought the document to a revision.
    last_change: Option<(u32, Id)>,
}

/// One guest's connection.
struct Link {
    conn: u64,
    token: hmac::Tag,
    tx: mpsc::Sender<Out>,
    kill: watch::Sender<bool>,
    /// The change counter of the last document frame queued for it.
    doc_seq: u64,
    /// Presence versions it has been sent.
    seen: HashMap<Id, u64>,
}

struct Departed {
    participant: Participant,
    token: hmac::Tag,
    at: Instant,
}

struct Slot {
    version: u64,
    presence: Option<Presence>,
}

/// Why a guest is taken out of the session.
enum Leave {
    /// It said bye.
    Bye,
    /// Its connection dropped, or it could not keep up. Its id and color
    /// wait `REJOIN_WINDOW` for it.
    Dropped,
    /// The host removed it, with this notice.
    Removed(String),
}

fn random_text(rng: &SystemRandom, bytes: usize) -> Result<String, IpcError> {
    let mut buf = vec![0u8; bytes];
    rng.fill(&mut buf)
        .map_err(|_| IpcError::new("io", "This computer gave no random numbers for the session secret."))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// The lowest color no one in the session has. When all are taken, the one
/// the fewest people have.
fn free_color(participants: &[Participant]) -> u8 {
    let mut uses = [0usize; PEER_COLOR_COUNT as usize];
    for p in participants {
        if let Some(n) = uses.get_mut(p.color as usize) {
            *n += 1;
        }
    }
    let fewest = uses.iter().copied().min().unwrap_or(0);
    uses.iter().position(|n| *n == fewest).unwrap_or(0) as u8
}

/// This computer's address on the local network, found without sending
/// anything: a UDP socket "connected" to a documentation-only address picks
/// the interface a real packet would leave from. None without a network.
fn lan_ipv4() -> Option<Ipv4Addr> {
    let socket = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// Every IPv4 address of this computer's network interfaces. Empty when the
/// system does not say.
fn interface_ipv4s() -> Vec<Ipv4Addr> {
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return vec![];
    };
    interfaces
        .iter()
        .filter_map(|i| match i.ip() {
            IpAddr::V4(ip) => Some(ip),
            IpAddr::V6(_) => None,
        })
        .collect()
}

/// 100.64.0.0/10, carrier-grade NAT space: Tailscale and similar VPNs give
/// their members addresses from it.
fn is_shared(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    a == 100 && (64..128).contains(&b)
}

/// The addresses guests may reach this computer at, best first: the one the
/// default route leaves from, then private networks (home and office LANs),
/// then 100.64.0.0/10 (VPNs such as Tailscale), then any other. Loopback,
/// link-local, unspecified, broadcast and multicast addresses are left out,
/// and so are repeats. At most `MAX_NETWORK_ADDRS`.
fn order_addresses(default_route: Option<Ipv4Addr>, found: &[Ipv4Addr]) -> Vec<Ipv4Addr> {
    let rank = |ip: &Ipv4Addr| match *ip {
        ip if Some(ip) == default_route => 0,
        ip if ip.is_private() => 1,
        ip if is_shared(ip) => 2,
        _ => 3,
    };
    let mut out: Vec<Ipv4Addr> = vec![];
    for ip in default_route.into_iter().chain(found.iter().copied()) {
        let unusable =
            ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() || ip.is_broadcast() || ip.is_multicast();
        if !unusable && !out.contains(&ip) {
            out.push(ip);
        }
    }
    // Stable: interfaces of the same kind keep the system's order.
    out.sort_by_key(rank);
    out.truncate(MAX_NETWORK_ADDRS);
    out
}

/// Where guests reach this computer directly: `ips` in order, then
/// loopback, for a guest on the same computer.
fn addresses(ips: &[Ipv4Addr], port: u16) -> Vec<String> {
    ips.iter().map(|ip| format!("{ip}:{port}")).chain([format!("127.0.0.1:{port}")]).collect()
}

/// The relay a session registers with: `live_relay` in settings.json, else
/// the one this build was made with. Empty text, from either, is no relay.
fn relay_setting(settings: &serde_json::Map<String, Value>) -> Result<Option<RelayUrl>, IpcError> {
    let (text, from_settings) = match settings.get(RELAY_KEY) {
        None | Some(Value::Null) => (BUILT_IN_RELAY.unwrap_or_default(), false),
        Some(Value::String(text)) => (text.as_str(), true),
        Some(_) => {
            return Err(IpcError::new(
                "invalid",
                "\"live_relay\" in settings.json must be text: the relay's wss:// address, or \"\" to switch the relay off.",
            ))
        }
    };
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    relay::check_url(text).map(Some).map_err(|why| {
        let message = if from_settings {
            format!(
                "\"live_relay\" in settings.json is not a relay address Guhit Studio can use: {why}. Set it to the relay's wss:// address, or to \"\" to switch the relay off."
            )
        } else {
            format!(
                "The relay address this build was made with cannot be used: {why}. Set \"live_relay\" in settings.json to the relay's wss:// address, or to \"\" to switch the relay off."
            )
        };
        IpcError::new("invalid", message)
    })
}

/// Whether a session takes direct connections: `live_direct` in
/// settings.json, true unless it is false.
fn direct_setting(settings: &serde_json::Map<String, Value>) -> Result<bool, IpcError> {
    match settings.get(DIRECT_KEY) {
        None | Some(Value::Null) => Ok(true),
        Some(Value::Bool(direct)) => Ok(*direct),
        Some(_) => Err(IpcError::new("invalid", "\"live_direct\" in settings.json must be true or false.")),
    }
}

/// Listen on every IPv4 interface (the desktop app) or on loopback (the dev
/// bridge). A port the caller names must be free; otherwise `live_port` from
/// settings.json (1460 by default) and the nine after it are tried, then one
/// the system picks.
async fn bind(
    lan: bool,
    port: Option<u16>,
    settings: &serde_json::Map<String, Value>,
) -> Result<TcpListener, IpcError> {
    let ip = if lan { Ipv4Addr::UNSPECIFIED } else { Ipv4Addr::LOCALHOST };
    if let Some(port) = port {
        return TcpListener::bind((ip, port)).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                IpcError::new(
                    "invalid",
                    format!("Port {port} is already in use on this computer. Pick another port, or leave it empty so Guhit Studio picks one."),
                )
            } else {
                IpcError::new("io", format!("Could not listen on port {port}: {e}"))
            }
        });
    }
    let first = settings
        .get(PORT_KEY)
        .and_then(Value::as_u64)
        .and_then(|p| u16::try_from(p).ok())
        .filter(|p| *p > 0)
        .unwrap_or(DEFAULT_PORT);
    for port in first..=first.saturating_add(PORT_TRIES - 1) {
        if let Ok(listener) = TcpListener::bind((ip, port)).await {
            return Ok(listener);
        }
    }
    TcpListener::bind((ip, 0))
        .await
        .map_err(|e| IpcError::new("io", format!("Could not listen for guests: {e}")))
}

/// `live_host`: share the open project. Guests reach it directly (unless
/// `live_direct` is false), through the relay when there is one, or both.
/// The relay registration runs on its own; this does not wait for it.
pub(crate) async fn start(app: &AppService, port: Option<u16>) -> Result<LiveStatus, IpcError> {
    match app.live.role() {
        Role::Host(_) => return Ok(app.live.status()),
        Role::Guest(_) => return Err(host_only("Leave the live session first.")),
        Role::Off => {}
    }
    let name = app.profile_name().await;
    if name.is_empty() {
        return Err(bad_args("Pick the name others will see first."));
    }
    // Watched before the project is read, so closing it right now is seen.
    let changes = app.watch_changes();
    let (project_id, project_name, data_dir) = {
        let s = app.session.lock().await;
        if s.live_copy.is_some() {
            return Err(host_only("Leave the live session first."));
        }
        let doc = s.open_doc()?;
        (doc.project().id.clone(), doc.project().name.clone(), s.data_dir.clone())
    };
    let settings = settings_of(&data_dir);
    let relay_url = relay_setting(&settings)?;
    let direct = direct_setting(&settings)?;
    if !direct && relay_url.is_none() {
        return Err(IpcError::new(
            "invalid",
            "Direct connections are off (\"live_direct\": false in settings.json) and there is no relay, so no one could join. Set \"live_direct\" to true, or set \"live_relay\" to a relay's address.",
        ));
    }
    // Direct connections off: nothing listens, so a port does not apply.
    let listener = if direct { Some(bind(app.lan, port, &settings).await?) } else { None };
    let addresses = match &listener {
        Some(listener) => {
            let port = listener
                .local_addr()
                .map_err(|e| IpcError::new("io", format!("Could not listen for guests: {e}")))?
                .port();
            let ips = if app.lan { order_addresses(lan_ipv4(), &interface_ipv4s()) } else { vec![] };
            addresses(&ips, port)
        }
        None => vec![],
    };
    let identity = HostIdentity::new()?;
    let rng = SystemRandom::new();
    let secret = random_text(&rng, 16)?;
    let key = hmac::Key::generate(hmac::HMAC_SHA256, &rng)
        .map_err(|_| IpcError::new("io", "This computer gave no random numbers for the session key."))?;
    let secret_tag = hmac::sign(&key, secret.as_bytes());
    // The session's room on the relay, and the key that proves it is ours.
    let room = match relay_url {
        Some(url) => Some(Room::new(url, random_text(&rng, 16)?, random_text(&rng, 32)?)),
        None => None,
    };
    let invite = Invite {
        v: INVITE_VERSION,
        secret,
        pin: identity.pin,
        addrs: addresses.clone(),
        relay: room.as_ref().map(|r| InviteRelay { url: r.url.as_str().to_string(), room: r.id.clone() }),
        project: project_name.clone(),
    }
    .encode();
    let id = app.live.host_id.get_or_init(defaults::new_id).clone();
    let me = Participant { id, name, color: 0, role: ParticipantRole::Host };
    let host = Arc::new(Host {
        me: me.clone(),
        project_id: project_id.clone(),
        data_dir,
        key,
        secret_tag,
        acceptor: identity.acceptor,
        shutdown: watch::channel(false).0,
        inner: Mutex::new(Inner { participants: vec![me.clone()], ..Inner::default() }),
        doc_frame: Mutex::new(None),
        chat_order: Mutex::new(()),
        ping: wire::shared(&ToGuest::Ping, MAX_HELLO_BYTES).unwrap_or_else(|_| Arc::from(&[][..])),
        next_conn: AtomicU64::new(0),
        waiting: Arc::new(Semaphore::new(MAX_WAITING)),
    });
    host.set_presence(&me.id, Some(app.local_presence()));
    let status = LiveStatus {
        mode: LiveMode::Hosting,
        self_id: Some(me.id.clone()),
        participants: vec![me.clone()],
        invite: Some(invite),
        addresses,
        relay: if room.is_some() { LiveRelay::Connecting } else { LiveRelay::Off },
        project_id: Some(project_id),
        project_name: Some(project_name),
        notice: None,
    };
    {
        let mut role = lock(&app.live.role);
        match &*role {
            Role::Off => {}
            // Another `live_host` got there first; this listener closes.
            Role::Host(_) => return Ok(app.live.status()),
            Role::Guest(_) => return Err(host_only("Leave the live session first.")),
        }
        *role = Role::Host(host.clone());
        app.live.set_status(status.clone());
    }
    app.live.remember_name(&me.id, &me.name);
    // Out before the relay's first news, which may come at once.
    app.emit(AppEvent::Live { status });
    if let Some(listener) = listener {
        tokio::spawn(accept_loop(app.clone(), host.clone(), listener));
    }
    if let Some(room) = room {
        tokio::spawn(relay::host_loop(app.clone(), host.clone(), room));
    }
    tokio::spawn(broadcast_loop(app.clone(), host.clone(), changes));
    tokio::spawn(presence_loop(app.clone(), host));
    // The status as it is now: the relay may have answered already.
    Ok(app.live.status())
}

/// The session's registration with the relay changed. Shown in the status
/// while `host` is still the session this computer runs.
pub(crate) fn relay_changed(app: &AppService, host: &Arc<Host>, relay: LiveRelay) {
    let role = lock(&app.live.role);
    if !matches!(&*role, Role::Host(h) if Arc::ptr_eq(h, host)) {
        return;
    }
    let mut changed = false;
    let status = app.live.edit_status(|s| {
        changed = s.relay != relay;
        s.relay = relay;
    });
    // Sent under the role lock, so it never lands after the event that ends
    // the session.
    if changed {
        app.emit(AppEvent::Live { status });
    }
}

/// End the session for everyone: each guest gets `notice`, the listener
/// closes, and this computer's status is off with `own_notice`.
pub(crate) fn end(app: &AppService, host: &Arc<Host>, notice: &str, own_notice: Option<String>) {
    let (links, gone) = {
        let mut inner = lock(&host.inner);
        if inner.ended {
            return;
        }
        inner.ended = true;
        let gone: Vec<Id> = inner
            .presence
            .iter()
            .filter(|(id, slot)| **id != host.me.id && slot.presence.is_some())
            .map(|(id, _)| id.clone())
            .collect();
        inner.participants.retain(|p| p.id == host.me.id);
        (std::mem::take(&mut inner.guests), gone)
    };
    host.shutdown.send_replace(true);
    let last = wire::shared(&ToGuest::End { notice: notice.to_string() }, MAX_FRAME_BYTES).ok();
    for link in links.into_values() {
        say_goodbye(&link, last.as_ref());
    }
    let status = LiveStatus { notice: own_notice, ..LiveStatus::off() };
    let ours = {
        let mut role = lock(&app.live.role);
        let ours = matches!(&*role, Role::Host(h) if Arc::ptr_eq(h, host));
        if ours {
            *role = Role::Off;
            app.live.set_status(status.clone());
        }
        ours
    };
    for id in gone {
        app.emit(AppEvent::Presence { participant_id: id, presence: None });
    }
    if ours {
        app.emit(AppEvent::Live { status });
    }
}

/// The shared project closed, or another one opened in its place.
pub(crate) fn end_for_close(app: &AppService, host: &Arc<Host>) {
    end(
        app,
        host,
        &format!("{} closed the project.", host.me.name),
        Some("The live session ended because the project closed.".to_string()),
    );
}

/// Queue the last frame for a guest and close its connection after it. A
/// guest too slow to take it, or that does not close in time, is cut off.
fn say_goodbye(link: &Link, last: Option<&Arc<[u8]>>) {
    let told = last.is_some_and(|f| link.tx.try_send(Out::Frame(f.clone())).is_ok() && link.tx.try_send(Out::Close).is_ok());
    if !told {
        link.kill.send_replace(true);
        return;
    }
    let kill = link.kill.clone();
    tokio::spawn(async move {
        tokio::time::sleep(wire::CLOSE_WAIT).await;
        kill.send_replace(true);
    });
}

/// `live_remove`: take one guest out, with a notice.
pub(crate) fn remove(app: &AppService, participant_id: &str) -> Result<LiveStatus, IpcError> {
    let host = match app.live.role() {
        Role::Host(host) => host,
        Role::Guest(_) => return Err(host_only("Only the host can remove someone from the live session.")),
        Role::Off => return Err(not_live("There is no live session.")),
    };
    if participant_id == host.me.id {
        return Err(bad_args("You host this live session. End it instead."));
    }
    let conn = lock(&host.inner).guests.get(participant_id).map(|l| l.conn);
    let Some(conn) = conn else {
        return Err(IpcError::new("not_found", "That person is not in the live session."));
    };
    let notice = format!("{} removed you from the live session.", host.me.name);
    host.leave(app, participant_id, Some(conn), Leave::Removed(notice));
    Ok(app.live.status())
}

/// A chat message from `author_id` (the host or a guest): stamped, saved to
/// chat.jsonl, sent to every guest and to this window.
pub(crate) fn chat(
    app: &AppService,
    host: &Host,
    author_id: &str,
    text: &str,
    at: Option<Point>,
    level_id: Option<Id>,
    via_ai: bool,
) -> Result<ChatMessage, IpcError> {
    let text = check_chat(text, at, level_id.as_deref())?;
    let _order = lock(&host.chat_order);
    let author = {
        let inner = lock(&host.inner);
        if inner.ended {
            return Err(not_live("The live session has ended."));
        }
        inner
            .participants
            .iter()
            .find(|p| p.id == author_id)
            .cloned()
            .ok_or_else(|| not_live("You are not in the live session."))?
    };
    let message = ChatMessage {
        id: defaults::new_id(),
        author_id: author.id,
        author_name: author.name,
        color: author.color,
        text,
        sent_at: defaults::now_rfc3339(),
        at,
        level_id,
        via_ai,
    };
    // Saved before it goes out, so a guest joining meanwhile finds it in the
    // history it is sent.
    append_chat(&host.project_dir()?, &message);
    let frame = wire::shared(&ToGuest::Chat { message: message.clone() }, MAX_FRAME_BYTES)
        .map_err(|e| IpcError::new("io", e.to_string()))?;
    let slow: Vec<(Id, u64)> = {
        let inner = lock(&host.inner);
        inner
            .guests
            .iter()
            .filter(|(_, link)| link.tx.try_send(Out::Frame(frame.clone())).is_err())
            .map(|(id, link)| (id.clone(), link.conn))
            .collect()
    };
    // A guest that cannot take a chat message is disconnected: it would miss
    // it. It rejoins and gets the history.
    for (id, conn) in slow {
        host.leave(app, &id, Some(conn), Leave::Dropped);
    }
    app.emit(AppEvent::Chat { message: message.clone() });
    Ok(message)
}

impl Host {
    fn project_dir(&self) -> Result<PathBuf, IpcError> {
        store::project_dir(&self.data_dir, &self.project_id)
    }

    /// Becomes true when the session ends, for the tasks that must stop then.
    pub(crate) fn stopped(&self) -> watch::Receiver<bool> {
        self.shutdown.subscribe()
    }

    fn secret_ok(&self, secret: &str) -> bool {
        hmac::verify(&self.key, secret.as_bytes(), self.secret_tag.as_ref()).is_ok()
    }

    fn token_ok(&self, token: &str, tag: &hmac::Tag) -> bool {
        hmac::verify(&self.key, token.as_bytes(), tag.as_ref()).is_ok()
    }

    /// True while `conn` is the connection of the guest `id`.
    fn is_current(&self, id: &str, conn: u64) -> bool {
        lock(&self.inner).guests.get(id).is_some_and(|l| l.conn == conn)
    }

    /// Who made the change that brought the document to `revision`.
    pub(crate) fn note_change(&self, revision: u32, by: &Id) {
        lock(&self.inner).last_change = Some((revision, by.clone()));
    }

    /// Store someone's latest presence. It goes out on the next tick. None:
    /// they left.
    pub(crate) fn set_presence(&self, id: &Id, presence: Option<Presence>) {
        let mut inner = lock(&self.inner);
        if inner.ended || !inner.participants.iter().any(|p| &p.id == id) {
            return;
        }
        if inner.presence.get(id).is_some_and(|slot| slot.presence == presence) {
            return;
        }
        inner.presence_version += 1;
        let version = inner.presence_version;
        inner.presence.insert(id.clone(), Slot { version, presence });
    }

    /// Everyone else's latest presence, in the order of the participants.
    pub(crate) fn presence_list(&self) -> Vec<PresenceEntry> {
        let inner = lock(&self.inner);
        inner
            .participants
            .iter()
            .filter(|p| p.id != self.me.id)
            .filter_map(|p| {
                let presence = inner.presence.get(&p.id)?.presence.clone()?;
                Some(PresenceEntry { participant_id: p.id.clone(), presence })
            })
            .collect()
    }

    /// Send each guest the presence that changed since its last batch, except
    /// its own, and return the events for this window.
    fn presence_tick(&self) -> Vec<AppEvent> {
        let mut inner = lock(&self.inner);
        let Inner { guests, presence, window_seen, .. } = &mut *inner;
        for (guest_id, link) in guests.iter_mut() {
            let changed: Vec<(PresenceUpdate, u64)> = presence
                .iter()
                .filter(|(id, slot)| *id != guest_id && link.seen.get(*id) != Some(&slot.version))
                .map(|(id, slot)| {
                    (PresenceUpdate { participant_id: id.clone(), presence: slot.presence.clone() }, slot.version)
                })
                .collect();
            if changed.is_empty() {
                continue;
            }
            let entries: Vec<PresenceUpdate> = changed.iter().map(|(u, _)| u.clone()).collect();
            let Ok(frame) = wire::shared(&ToGuest::Presence { entries }, MAX_FRAME_BYTES) else {
                continue;
            };
            // Presence may be dropped for a slow guest: what it missed goes
            // again on the next tick, latest only.
            if link.tx.try_send(Out::Frame(frame)).is_ok() {
                for (update, version) in changed {
                    link.seen.insert(update.participant_id, version);
                }
            }
        }
        let mut events = vec![];
        for (id, slot) in presence.iter() {
            if *id == self.me.id || window_seen.get(id) == Some(&slot.version) {
                continue;
            }
            window_seen.insert(id.clone(), slot.version);
            events.push(AppEvent::Presence { participant_id: id.clone(), presence: slot.presence.clone() });
        }
        // Someone who left is forgotten once everyone has been told.
        let told: Vec<Id> = presence
            .iter()
            .filter(|(id, slot)| {
                slot.presence.is_none()
                    && window_seen.get(*id) == Some(&slot.version)
                    && guests.values().all(|l| l.seen.get(*id) == Some(&slot.version))
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in told {
            presence.remove(&id);
            window_seen.remove(&id);
            for link in guests.values_mut() {
                link.seen.remove(&id);
            }
        }
        events
    }

    /// Take guests out of the session. `conn` guards against a newer
    /// connection of the same participant (a rejoin replaced this one).
    fn leave(&self, app: &AppService, id: &str, conn: Option<u64>, how: Leave) {
        let mut events = vec![];
        {
            let mut inner = lock(&self.inner);
            if inner.ended {
                return;
            }
            let mut queue = vec![(id.to_string(), conn, how)];
            let mut changed = false;
            while let Some((id, conn, how)) = queue.pop() {
                let current = inner.guests.get(&id).map(|l| l.conn);
                if current.is_none() || conn.is_some_and(|c| Some(c) != current) {
                    continue;
                }
                let Some(link) = inner.guests.remove(&id) else { continue };
                match &how {
                    Leave::Removed(notice) => {
                        let end = wire::shared(&ToGuest::End { notice: notice.clone() }, MAX_FRAME_BYTES).ok();
                        say_goodbye(&link, end.as_ref());
                    }
                    Leave::Bye | Leave::Dropped => {
                        link.kill.send_replace(true);
                    }
                }
                let participant = inner
                    .participants
                    .iter()
                    .position(|p| p.id == id)
                    .map(|i| inner.participants.remove(i));
                match (&how, participant) {
                    (Leave::Dropped, Some(participant)) => {
                        inner.departed.insert(id.clone(), Departed { participant, token: link.token, at: Instant::now() });
                    }
                    (Leave::Removed(_), _) => {
                        inner.removed.insert(id.clone());
                    }
                    _ => {}
                }
                // Whoever showed a presence now shows none.
                if inner.presence.get(&id).is_some_and(|slot| slot.presence.is_some()) {
                    inner.presence_version += 1;
                    let version = inner.presence_version;
                    inner.presence.insert(id.clone(), Slot { version, presence: None });
                    inner.window_seen.insert(id.clone(), version);
                    events.push(AppEvent::Presence { participant_id: id.clone(), presence: None });
                }
                changed = true;
                // Tell the rest who is here now. A guest too slow to take it
                // goes too.
                let frame =
                    wire::shared(&ToGuest::Participants { participants: inner.participants.clone() }, MAX_FRAME_BYTES);
                if let Ok(frame) = frame {
                    for (other, link) in inner.guests.iter() {
                        if link.tx.try_send(Out::Frame(frame.clone())).is_err() {
                            queue.push((other.clone(), Some(link.conn), Leave::Dropped));
                        }
                    }
                }
            }
            if !changed {
                return;
            }
            let participants = inner.participants.clone();
            let status = app.live.edit_status(|s| s.participants = participants);
            events.push(AppEvent::Live { status });
        }
        for event in events {
            app.emit(event);
        }
    }

    /// The document frame for the change `seq`, encoded once however many
    /// guests need it.
    fn doc_frame(&self, seq: u64, state: DocState, by: Id) -> Option<Arc<[u8]>> {
        let mut cache = lock(&self.doc_frame);
        if let Some((cached, frame)) = cache.as_ref() {
            if *cached == seq {
                return Some(frame.clone());
            }
        }
        let frame = ToGuest::Doc(Box::new(DocFrame {
            undo: UndoMeta::of(&state),
            project: state.project,
            revision: state.revision,
            seq,
            by: Some(by),
        }));
        match wire::shared(&frame, MAX_FRAME_BYTES) {
            Ok(frame) => {
                *cache = Some((seq, frame.clone()));
                Some(frame)
            }
            Err(e) => {
                eprintln!("guhit-app: the plan could not be sent to the live session: {e}");
                None
            }
        }
    }
}

/// Queue the current document for every guest that does not have it yet.
/// Called after every change (the broadcaster) and after a guest's edit,
/// before its reply.
async fn sync_guests(app: &AppService, host: &Host) {
    let (seq, state, by) = {
        let s = app.session.lock().await;
        let Some(doc) = s.doc.as_ref().filter(|d| d.project().id == host.project_id) else {
            return;
        };
        // Every change notifies under the session lock, so this counter
        // belongs to the document as it is now.
        let seq = app.change_seq();
        let by = {
            let inner = lock(&host.inner);
            if inner.ended || inner.guests.values().all(|g| g.doc_seq >= seq) {
                return;
            }
            match &inner.last_change {
                Some((revision, id)) if *revision == doc.revision() => id.clone(),
                _ => host.me.id.clone(),
            }
        };
        (seq, doc.state(), by)
    };
    let Some(frame) = host.doc_frame(seq, state, by) else {
        return;
    };
    let slow: Vec<(Id, u64)> = {
        let mut inner = lock(&host.inner);
        let mut slow = vec![];
        for (id, link) in inner.guests.iter_mut() {
            if link.doc_seq >= seq {
                continue;
            }
            if link.tx.try_send(Out::Frame(frame.clone())).is_ok() {
                link.doc_seq = seq;
            } else {
                slow.push((id.clone(), link.conn));
            }
        }
        slow
    };
    // A guest that cannot take the new plan is disconnected rather than left
    // working on an old one. It rejoins and gets the current plan.
    for (id, conn) in slow {
        host.leave(app, &id, Some(conn), Leave::Dropped);
    }
}

async fn broadcast_loop(app: AppService, host: Arc<Host>, mut changes: watch::Receiver<DocChange>) {
    let mut stop = host.shutdown.subscribe();
    loop {
        let project = changes.borrow_and_update().project_id.clone();
        if project.as_deref() != Some(host.project_id.as_str()) {
            end_for_close(&app, &host);
            return;
        }
        sync_guests(&app, &host).await;
        tokio::select! {
            _ = stop.wait_for(|s| *s) => return,
            changed = changes.changed() => if changed.is_err() { return },
        }
    }
}

async fn presence_loop(app: AppService, host: Arc<Host>) {
    let mut stop = host.shutdown.subscribe();
    let mut tick = tokio::time::interval(PRESENCE_EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = stop.wait_for(|s| *s) => return,
            _ = tick.tick() => {}
        }
        for event in host.presence_tick() {
            app.emit(event);
        }
    }
}

async fn accept_loop(app: AppService, host: Arc<Host>, listener: TcpListener) {
    let mut stop = host.shutdown.subscribe();
    loop {
        let accepted = tokio::select! {
            _ = stop.wait_for(|s| *s) => break,
            accepted = listener.accept() => accepted,
        };
        match accepted {
            // With MAX_WAITING connections still authenticating, a new one is
            // closed at once.
            Ok((tcp, _)) => {
                if let Ok(permit) = host.waiting.clone().try_acquire_owned() {
                    let _ = tcp.set_nodelay(true);
                    tokio::spawn(connection(app.clone(), host.clone(), Box::new(tcp), permit));
                }
            }
            // Out of sockets or similar: wait a moment instead of spinning.
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

async fn refuse<W: AsyncWrite + Unpin>(w: &mut W, message: &str) {
    let Ok(frame) = wire::encode(&ToGuest::Refused { message: message.to_string() }, MAX_HELLO_BYTES) else {
        return;
    };
    let _ = tokio::time::timeout(AUTH_TIMEOUT, async {
        w.write_all(&frame).await?;
        w.flush().await?;
        w.shutdown().await
    })
    .await;
}

/// One connection, direct or through the relay, from the TLS handshake to
/// its end. Anything unexpected before the guest is in closes this
/// connection and nothing else.
pub(crate) async fn connection(app: AppService, host: Arc<Host>, io: BoxIo, permit: OwnedSemaphorePermit) {
    let Ok(Ok(tls)) = tokio::time::timeout(AUTH_TIMEOUT, host.acceptor.accept(io)).await else {
        return;
    };
    let (mut rd, mut wr) = tokio::io::split(tls);
    let hello = wire::read_frame(&mut rd, MAX_HELLO_BYTES, AUTH_TIMEOUT, AUTH_TIMEOUT);
    let Ok(Ok(bytes)) = tokio::time::timeout(AUTH_TIMEOUT, hello).await else {
        return;
    };
    let Ok(ToHost::Hello { v, secret, name, rejoin }) = wire::decode::<ToHost>(&bytes) else {
        return;
    };
    if v != PROTOCOL_VERSION {
        return refuse(
            &mut wr,
            "This Guhit Studio is a different version from the host's. Update both to the latest version.",
        )
        .await;
    }
    if !host.secret_ok(&secret) {
        tokio::time::sleep(WRONG_SECRET_DELAY).await;
        return refuse(&mut wr, "This invite does not open the live session. Ask the host for a new invite.").await;
    }
    let name = clean_name(&name);
    if name.is_empty() {
        return refuse(&mut wr, "Pick a name before joining.").await;
    }
    let joined = match register(&app, &host, &name, rejoin).await {
        Ok(joined) => joined,
        Err(message) => return refuse(&mut wr, &message).await,
    };
    drop(permit);
    serve(app, host, joined, rd, wr).await;
}

struct Joined {
    id: Id,
    conn: u64,
    tx: mpsc::Sender<Out>,
    rx: mpsc::Receiver<Out>,
    kill: watch::Sender<bool>,
}

/// Admit a guest: its participant, the welcome at the head of its queue, and
/// everyone else told. Under the session lock, so no change slips between the
/// plan in the welcome and the guest's place in the broadcast.
async fn register(app: &AppService, host: &Arc<Host>, name: &str, rejoin: Option<Rejoin>) -> Result<Joined, String> {
    let s = app.session.lock().await;
    let Some(doc) = s.doc.as_ref().filter(|d| d.project().id == host.project_id) else {
        return Err("The host closed the project.".to_string());
    };
    let state = doc.state();
    let seq = app.change_seq();
    let token = random_text(&SystemRandom::new(), 16).map_err(|e| e.message)?;
    let token_tag = hmac::sign(&host.key, token.as_bytes());
    let order = lock(&host.chat_order);
    let chat = read_chat(&host.project_dir().map_err(|e| e.message)?);

    let mut events = vec![];
    let joined = {
        let mut inner = lock(&host.inner);
        if inner.ended {
            return Err("The live session has ended.".to_string());
        }
        inner.departed.retain(|_, d| d.at.elapsed() < REJOIN_WINDOW);
        if rejoin.as_ref().is_some_and(|r| inner.removed.contains(&r.id)) {
            return Err(format!("{} removed you from the live session.", host.me.name));
        }

        // Back after a dropped connection: the same id and color. When its
        // old connection has not noticed yet that it is gone, this one takes
        // over.
        let rejoin = rejoin.filter(|r| {
            let tag = inner.guests.get(&r.id).map(|l| &l.token).or(inner.departed.get(&r.id).map(|d| &d.token));
            tag.is_some_and(|tag| host.token_ok(&r.token, tag))
        });
        let replaces = rejoin.as_ref().is_some_and(|r| inner.guests.contains_key(&r.id));
        if !replaces && inner.participants.len() >= MAX_PARTICIPANTS {
            return Err(format!("The live session is full: {MAX_PARTICIPANTS} people are in it."));
        }
        let mut participant = match &rejoin {
            Some(r) if replaces => inner.participants.iter().find(|p| p.id == r.id).cloned(),
            Some(r) => inner.departed.get(&r.id).map(|d| d.participant.clone()),
            None => None,
        }
        .unwrap_or_else(|| Participant {
            id: defaults::new_id(),
            name: String::new(),
            color: free_color(&inner.participants),
            role: ParticipantRole::Guest,
        });
        participant.name = name.to_string();
        let mut participants: Vec<Participant> =
            inner.participants.iter().filter(|p| p.id != participant.id).cloned().collect();
        participants.push(participant.clone());

        let presence: Vec<PresenceEntry> = participants
            .iter()
            .filter(|p| p.id != participant.id)
            .filter_map(|p| {
                let presence = inner.presence.get(&p.id)?.presence.clone()?;
                Some(PresenceEntry { participant_id: p.id.clone(), presence })
            })
            .collect();
        let welcome = ToGuest::Welcome(Box::new(Welcome {
            participant_id: participant.id.clone(),
            rejoin_token: token,
            participants: participants.clone(),
            doc: DocFrame {
                undo: UndoMeta::of(&state),
                project: state.project,
                revision: state.revision,
                seq,
                by: None,
            },
            chat,
            presence,
        }));
        let welcome = wire::shared(&welcome, MAX_FRAME_BYTES)
            .map_err(|_| "The shared project is too large to send.".to_string())?;

        if let Some(old) = rejoin.as_ref().and_then(|r| inner.guests.remove(&r.id)) {
            old.kill.send_replace(true);
        }
        if let Some(r) = &rejoin {
            inner.departed.remove(&r.id);
        }
        inner.participants = participants.clone();
        let conn = host.next_conn.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = mpsc::channel(QUEUE_FRAMES);
        let kill = watch::channel(false).0;
        // A new queue always has room for its first frame.
        let _ = tx.try_send(Out::Frame(welcome));
        let seen = inner
            .presence
            .iter()
            .filter(|(id, _)| **id != participant.id)
            .map(|(id, slot)| (id.clone(), slot.version))
            .collect();
        inner.guests.insert(
            participant.id.clone(),
            Link { conn, token: token_tag, tx: tx.clone(), kill: kill.clone(), doc_seq: seq, seen },
        );

        // Tell the others; one too slow for it is let go below.
        let mut slow = vec![];
        if let Ok(frame) = wire::shared(&ToGuest::Participants { participants: participants.clone() }, MAX_FRAME_BYTES) {
            for (other, link) in inner.guests.iter() {
                if *other != participant.id && link.tx.try_send(Out::Frame(frame.clone())).is_err() {
                    slow.push((other.clone(), link.conn));
                }
            }
        }
        let status = app.live.edit_status(|s| s.participants = participants);
        events.push(AppEvent::Live { status });
        (Joined { id: participant.id.clone(), conn, tx, rx, kill }, slow, participant)
    };
    drop(order);
    drop(s);
    let (joined, slow, participant) = joined;
    app.live.remember_name(&participant.id, &participant.name);
    for (id, conn) in slow {
        host.leave(app, &id, Some(conn), Leave::Dropped);
    }
    for event in events {
        app.emit(event);
    }
    Ok(joined)
}

/// A guest's requests until its connection ends.
async fn serve(
    app: AppService,
    host: Arc<Host>,
    joined: Joined,
    mut rd: ReadHalf<HostTls>,
    wr: WriteHalf<HostTls>,
) {
    let Joined { id, conn, tx, rx, kill } = joined;
    tokio::spawn(wire::write_loop(wr, rx, kill.clone(), host.ping.clone()));
    let mut stop = kill.subscribe();
    let mut upload = None;
    let how = loop {
        let read = tokio::select! {
            // Whoever stopped this connection has already taken the guest out.
            _ = stop.wait_for(|k| *k) => break None,
            read = wire::read_frame(&mut rd, MAX_FRAME_BYTES, IDLE_TIMEOUT, BODY_TIMEOUT) => read,
        };
        // A frame that is too big, not JSON or not a message closes this
        // connection only.
        let Ok(bytes) = read else { break Some(Leave::Dropped) };
        let Ok(message) = wire::decode::<ToHost>(&bytes) else { break Some(Leave::Dropped) };
        // A guest that was removed, or came back on a newer connection, gets
        // nothing more on this one.
        if !host.is_current(&id, conn) {
            break None;
        }
        match message {
            ToHost::Ping => {}
            ToHost::Bye => break Some(Leave::Bye),
            ToHost::Hello { .. } => break Some(Leave::Dropped),
            ToHost::Presence { presence } => host.set_presence(&id, Some(clean_presence(presence))),
            ToHost::Request { id: request_id, request } => {
                let edit = matches!(request, Request::Apply { .. } | Request::Undo { .. } | Request::Redo { .. });
                let result = answer(&app, &host, &id, request, &mut upload).await;
                // The new plan goes out before the reply, so the guest's copy
                // is current when its call returns.
                if edit && result.is_ok() {
                    sync_guests(&app, &host).await;
                }
                let (ok, error) = match result {
                    Ok(value) => (Some(value), None),
                    Err(e) => (None, Some(e)),
                };
                let reply = wire::shared(&ToGuest::Reply { id: request_id, ok, error }, MAX_FRAME_BYTES).or_else(|e| {
                    let error = IpcError::new("io", format!("The answer could not be sent: {e}"));
                    wire::shared(&ToGuest::Reply { id: request_id, ok: None, error: Some(error) }, MAX_FRAME_BYTES)
                });
                // A guest whose queue is full cannot get its answer:
                // disconnected, it rejoins.
                if !reply.is_ok_and(|frame| tx.try_send(Out::Frame(frame)).is_ok()) {
                    break Some(Leave::Dropped);
                }
            }
        }
    };
    if let Some(how) = how {
        host.leave(&app, &id, Some(conn), how);
    }
    kill.send_replace(true);
}

/// Answer one request of the guest `guest_id`.
async fn answer(
    app: &AppService,
    host: &Arc<Host>,
    guest_id: &Id,
    request: Request,
    upload: &mut Option<Upload>,
) -> Result<Value, IpcError> {
    if lock(&host.inner).ended {
        return Err(not_live("The live session has ended."));
    }
    let project = Some(host.project_id.as_str());
    match request {
        Request::Apply { command, origin, expected_revision } => {
            let mut s = app.session.lock().await;
            let result = app.commit_locked(&mut s, command, origin, expected_revision, Some(guest_id.clone()), project)?;
            to_json(&Applied { revision: result.state.revision, diff: result.diff })
        }
        Request::Undo { force } => step(app, guest_id, false, force, project).await,
        Request::Redo { force } => step(app, guest_id, true, force, project).await,
        Request::Chat { text, at, level_id, via_ai } => {
            to_json(&chat(app, host, guest_id, &text, at, level_id, via_ai)?)
        }
        Request::FileGet { kind, file_name, offset } => file_get(&host.project_dir()?, kind, &file_name, offset),
        Request::FilePut { kind, file_name, offset, total, data } => {
            let Some(done) = file_piece(upload, kind, file_name, offset, total, &data)? else {
                return Ok(json!({ "received": offset + piece_len(&data) }));
            };
            // Stored under the session lock, like the window's own uploads.
            let _s = app.session.lock().await;
            let dir = host.project_dir()?;
            match done.kind {
                FileKind::Underlay => {
                    let kind = ImageKind::sniff(&done.bytes)
                        .ok_or_else(|| IpcError::new("invalid", "underlays are PNG or JPEG files"))?;
                    if done.bytes.len() > files::MAX_IMAGE_BYTES {
                        return Err(IpcError::new("invalid", "invalid image data: the image is larger than 25 MB"));
                    }
                    let stored = crate::store_underlay(&dir, &done.file_name, kind, &done.bytes)?;
                    Ok(json!({ "file_name": stored }))
                }
                FileKind::Model => {
                    let stored = interop::store_model(&dir, &done.file_name, &done.bytes)?;
                    Ok(json!({ "file_name": stored, "size": done.bytes.len() }))
                }
            }
        }
    }
}

/// A guest's undo or redo on the one shared history.
async fn step(app: &AppService, guest_id: &Id, redo: bool, force: bool, project: Option<&str>) -> Result<Value, IpcError> {
    let mut s = app.session.lock().await;
    let state = app.step_locked(&mut s, redo, force, Some(guest_id), project)?;
    to_json(&Stepped { revision: state.revision })
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<Value, IpcError> {
    serde_json::to_value(value).map_err(|e| IpcError::new("io", e.to_string()))
}

/// Bytes in a base64 piece, for the progress answer.
fn piece_len(data: &str) -> u64 {
    (data.trim_end_matches('=').len() as u64) * 3 / 4
}

/// The largest file of each kind a guest may send or fetch: the same limits
/// as the window's own uploads.
pub(crate) fn file_limit(kind: FileKind) -> usize {
    match kind {
        FileKind::Underlay => files::MAX_IMAGE_BYTES,
        FileKind::Model => interop::MAX_MODEL_BYTES,
    }
}

/// A file a guest is sending, piece by piece.
#[derive(Debug)]
struct Upload {
    kind: FileKind,
    file_name: String,
    total: u64,
    bytes: Vec<u8>,
}

/// Add one piece of an upload. The finished file once the last piece is in.
fn file_piece(
    upload: &mut Option<Upload>,
    kind: FileKind,
    file_name: String,
    offset: u64,
    total: u64,
    data: &str,
) -> Result<Option<Upload>, IpcError> {
    let limit = file_limit(kind);
    if total > limit as u64 {
        *upload = None;
        return Err(IpcError::new("invalid", format!("the file is larger than {} MB", limit / (1024 * 1024))));
    }
    if offset == 0 {
        *upload = Some(Upload { kind, file_name, total, bytes: Vec::with_capacity(total.min(FILE_CHUNK as u64) as usize) });
    }
    let in_order = upload
        .as_ref()
        .is_some_and(|u| u.kind == kind && u.total == total && u.bytes.len() as u64 == offset);
    if !in_order {
        *upload = None;
        return Err(bad_args("The file arrived out of order. Send it again."));
    }
    let piece = B64.decode(data.as_bytes()).map_err(|_| bad_args("A piece of the file is not base64."))?;
    let Some(current) = upload.as_mut() else {
        return Err(bad_args("The file arrived out of order. Send it again."));
    };
    if piece.len() > FILE_CHUNK || current.bytes.len() + piece.len() > total as usize {
        *upload = None;
        return Err(bad_args("A piece of the file does not fit it."));
    }
    current.bytes.extend_from_slice(&piece);
    if (current.bytes.len() as u64) < total {
        return Ok(None);
    }
    Ok(upload.take())
}

/// One piece of a stored underlay or reference model.
fn file_get(project_dir: &Path, kind: FileKind, file_name: &str, offset: u64) -> Result<Value, IpcError> {
    use std::io::{Read, Seek, SeekFrom};
    let path = match kind {
        FileKind::Underlay => crate::underlay_file(project_dir, file_name)?.0,
        FileKind::Model => interop::model_file(project_dir, file_name)?.0,
    };
    let missing = || IpcError::new("not_found", format!("file not found: {file_name}"));
    let mut file = std::fs::File::open(&path).map_err(|_| missing())?;
    let total = file.metadata().map_err(|e| files::io_err("cannot read", &path, e))?.len();
    if total > file_limit(kind) as u64 {
        return Err(IpcError::new("invalid", format!("{file_name} is too large to send")));
    }
    if offset > total {
        return Err(bad_args("That piece is past the end of the file."));
    }
    let len = (total - offset).min(FILE_CHUNK as u64) as usize;
    let mut piece = vec![0u8; len];
    file.seek(SeekFrom::Start(offset))
        .and_then(|_| file.read_exact(&mut piece))
        .map_err(|e| files::io_err("cannot read", &path, e))?;
    to_json(&FileChunk { data: B64.encode(&piece), total })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guest(color: u8) -> Participant {
        Participant { id: defaults::new_id(), name: "G".into(), color, role: ParticipantRole::Guest }
    }

    #[test]
    fn colors_go_to_the_lowest_free_one_then_the_least_used() {
        assert_eq!(free_color(&[]), 0);
        assert_eq!(free_color(&[guest(0), guest(1), guest(3)]), 2);
        let all: Vec<Participant> = (0..PEER_COLOR_COUNT).map(guest).collect();
        assert_eq!(free_color(&all), 0);
        let mut more = all.clone();
        more.push(guest(0));
        more.push(guest(1));
        assert_eq!(free_color(&more), 2);
    }

    #[test]
    fn addresses_put_loopback_last() {
        assert_eq!(addresses(&[], 1460), vec!["127.0.0.1:1460".to_string()]);
        let lan = addresses(&[Ipv4Addr::new(192, 168, 1, 20), Ipv4Addr::new(100, 101, 102, 103)], 1461);
        assert_eq!(lan, vec!["192.168.1.20:1461", "100.101.102.103:1461", "127.0.0.1:1461"]);
    }

    fn ip(text: &str) -> Ipv4Addr {
        text.parse().unwrap()
    }

    fn ips(list: &[&str]) -> Vec<Ipv4Addr> {
        list.iter().map(|t| ip(t)).collect()
    }

    #[test]
    fn addresses_go_default_route_first_then_lans_then_vpns() {
        let found = ips(&[
            "127.0.0.1",
            "203.0.113.7",
            "100.101.102.103",
            "169.254.3.4",
            "10.0.0.5",
            "192.168.1.20",
            "172.16.4.1",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.251",
            "10.0.0.5",
        ]);
        let ordered = order_addresses(Some(ip("192.168.1.20")), &found);
        assert_eq!(ordered, ips(&["192.168.1.20", "10.0.0.5", "172.16.4.1", "100.101.102.103", "203.0.113.7"]));

        // Without a default route, private ranges still lead. 100.64.0.0/10
        // ends at 100.127.255.255.
        let ordered = order_addresses(None, &ips(&["100.128.0.1", "100.64.0.1", "192.168.0.2"]));
        assert_eq!(ordered, ips(&["192.168.0.2", "100.64.0.1", "100.128.0.1"]));

        // A default route that is a VPN or public address still comes first,
        // even when the interface list missed it.
        let ordered = order_addresses(Some(ip("100.100.1.1")), &ips(&["192.168.0.2"]));
        assert_eq!(ordered, ips(&["100.100.1.1", "192.168.0.2"]));
        // One that is unusable is left out.
        assert!(order_addresses(Some(ip("169.254.1.1")), &[]).is_empty());
        assert!(order_addresses(None, &ips(&["127.0.0.1"])).is_empty());

        // At most seven, so loopback still fits the invite's eight.
        let many: Vec<Ipv4Addr> = (1..=12).map(|i| Ipv4Addr::new(10, 0, 0, i)).collect();
        let ordered = order_addresses(Some(ip("192.168.1.9")), &many);
        assert_eq!(ordered.len(), MAX_NETWORK_ADDRS);
        assert_eq!(ordered[0], ip("192.168.1.9"));
        assert_eq!(ordered[1..], many[..MAX_NETWORK_ADDRS - 1]);
        assert_eq!(addresses(&ordered, 1460).len(), MAX_ADDRS);
    }

    fn settings(json: Value) -> serde_json::Map<String, Value> {
        json.as_object().unwrap().clone()
    }

    #[test]
    fn the_relay_and_direct_settings_are_checked() {
        let relay = |json: Value| relay_setting(&settings(json)).map(|url| url.map(|u| u.as_str().to_string()));
        assert_eq!(relay(json!({ "live_relay": "wss://relay.example.com/" })).unwrap().as_deref(), Some("wss://relay.example.com"));
        assert_eq!(relay(json!({ "live_relay": "  ws://127.0.0.1:1470 " })).unwrap().as_deref(), Some("ws://127.0.0.1:1470"));
        // Empty switches it off, whatever the build says.
        assert_eq!(relay(json!({ "live_relay": " " })).unwrap(), None);
        // Absent: the build's relay, if it was made with one.
        let built_in = BUILT_IN_RELAY.map(str::trim).filter(|t| !t.is_empty());
        assert_eq!(relay(json!({})).ok().flatten().is_some(), built_in.is_some());
        assert_eq!(relay(json!({ "live_relay": null })).ok().flatten().is_some(), built_in.is_some());

        let err = relay(json!({ "live_relay": "ws://relay.example.com" })).unwrap_err();
        assert_eq!(err.code, "invalid");
        assert_eq!(
            err.message,
            "\"live_relay\" in settings.json is not a relay address Guhit Studio can use: ws:// works only for a relay on this computer. Set it to the relay's wss:// address, or to \"\" to switch the relay off."
        );
        assert_eq!(relay(json!({ "live_relay": 5 })).unwrap_err().code, "invalid");

        let direct = |json: Value| direct_setting(&settings(json));
        assert!(direct(json!({})).unwrap());
        assert!(direct(json!({ "live_direct": true })).unwrap());
        assert!(!direct(json!({ "live_direct": false })).unwrap());
        let err = direct(json!({ "live_direct": "no" })).unwrap_err();
        assert_eq!((err.code.as_str(), err.message.as_str()), ("invalid", "\"live_direct\" in settings.json must be true or false."));
    }

    #[test]
    fn uploads_arrive_in_order_or_not_at_all() {
        let mut upload = None;
        let a = B64.encode(b"hello ");
        let b = B64.encode(b"world");
        assert!(file_piece(&mut upload, FileKind::Model, "m.obj".into(), 0, 11, &a).unwrap().is_none());
        let done = file_piece(&mut upload, FileKind::Model, "m.obj".into(), 6, 11, &b).unwrap().unwrap();
        assert_eq!(done.bytes, b"hello world");
        assert!(upload.is_none());

        // A piece that skips ahead, one past the end, and a file over the limit.
        assert!(file_piece(&mut upload, FileKind::Model, "m.obj".into(), 0, 11, &a).unwrap().is_none());
        assert_eq!(file_piece(&mut upload, FileKind::Model, "m.obj".into(), 7, 11, &b).unwrap_err().code, "bad_args");
        assert!(upload.is_none());
        assert_eq!(file_piece(&mut upload, FileKind::Model, "m.obj".into(), 0, 3, &a).unwrap_err().code, "bad_args");
        let too_big = interop::MAX_MODEL_BYTES as u64 + 1;
        assert_eq!(file_piece(&mut upload, FileKind::Model, "m.obj".into(), 0, too_big, &a).unwrap_err().code, "invalid");
        // An empty file is done at once.
        let done = file_piece(&mut upload, FileKind::Model, "e.obj".into(), 0, 0, "").unwrap().unwrap();
        assert!(done.bytes.is_empty());
    }
}
