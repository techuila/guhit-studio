//! The relay's shared state: rooms, the guests waiting in them, and the
//! counts every limit checks.
//!
//! One lock over all of it. It is never held across an await: frames for
//! other connections are queued with `try_send`.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use axum::extract::ws::Utf8Bytes;
use axum::http::HeaderName;
use tokio::sync::{mpsc, oneshot};

use crate::config::Config;
use crate::conn::{Link, Out};
use crate::wire::{
    self, AcceptHello, HostHello, KeyDigest, RoomId, LIMIT, NORMAL, NOT_FOUND, REPLACED, TAKEN, WRONG_KEY,
};

/// Frames that may wait for one side of a pair. Small: a slow reader should
/// slow the sender down, not fill the relay's memory.
pub(crate) const PAIR_QUEUE: usize = 8;

pub(crate) struct Relay {
    pub(crate) config: Config,
    /// `config.client_ip_header`, parsed. None: the socket's address.
    pub(crate) ip_header: Option<HeaderName>,
    /// Frames that may wait for a control connection: `ready` and one
    /// announcement per waiting guest, with room to spare.
    pub(crate) control_queue: usize,
    state: Mutex<State>,
    next_id: AtomicU64,
    /// Open pairs, over all rooms.
    pairs: AtomicUsize,
    /// Connections refused over a limit, since the start.
    refused: AtomicU64,
}

#[derive(Default)]
struct State {
    connections: usize,
    per_ip: HashMap<IpAddr, usize>,
    rooms: HashMap<RoomId, Room>,
    rooms_per_ip: HashMap<IpAddr, usize>,
}

struct Room {
    key: KeyDigest,
    /// The control connection's address, for `max_rooms_per_ip`.
    ip: IpAddr,
    /// Which control connection holds the room. A takeover changes it, so
    /// the connection it replaced does not remove the room on its way out.
    control_id: u64,
    control: Link,
    waiting: HashMap<u64, Waiting>,
    /// Open pairs of this room. Shared with each pair's `PairSlot`, since a
    /// pair outlives the room when the control connection goes.
    pairs: Arc<AtomicUsize>,
}

/// A guest waiting for the host's accept.
struct Waiting {
    link: Link,
    paired: oneshot::Sender<Paired>,
}

/// One side of a pair that just opened: the other side, and the slot the
/// pair holds in the counts.
pub(crate) struct Paired {
    pub(crate) peer: Link,
    pub(crate) slot: Arc<PairSlot>,
}

/// Counts one open pair while either side still uses it.
pub(crate) struct PairSlot {
    room: Arc<AtomicUsize>,
    relay: Arc<Relay>,
}

impl Drop for PairSlot {
    fn drop(&mut self) {
        self.room.fetch_sub(1, Ordering::Relaxed);
        self.relay.pairs.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Holds one WebSocket's place in `max_connections` and `max_per_ip`.
pub(crate) struct Admission {
    relay: Arc<Relay>,
    ip: IpAddr,
}

impl Drop for Admission {
    fn drop(&mut self) {
        let mut state = self.relay.lock();
        state.connections = state.connections.saturating_sub(1);
        decrement(&mut state.per_ip, self.ip);
    }
}

/// Counts for the log. Never ids or addresses.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Counts {
    pub(crate) rooms: usize,
    pub(crate) pairs: usize,
    pub(crate) connections: usize,
    pub(crate) refused: u64,
}

impl Relay {
    pub(crate) fn new(config: Config) -> Relay {
        let ip_header = config.client_ip_header.as_deref().and_then(|h| HeaderName::try_from(h).ok());
        let control_queue = config.max_waiting_per_room.saturating_add(16).min(1 << 16);
        Relay {
            config,
            ip_header,
            control_queue,
            state: Mutex::new(State::default()),
            next_id: AtomicU64::new(1),
            pairs: AtomicUsize::new(0),
            refused: AtomicU64::new(0),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// A place for one more WebSocket from `ip`. None: over a limit.
    pub(crate) fn admit(self: &Arc<Self>, ip: IpAddr) -> Option<Admission> {
        let mut state = self.lock();
        let from_ip = state.per_ip.get(&ip).copied().unwrap_or(0);
        if state.connections >= self.config.max_connections || from_ip >= self.config.max_per_ip {
            drop(state);
            self.refused();
            return None;
        }
        state.connections += 1;
        *state.per_ip.entry(ip).or_insert(0) += 1;
        Some(Admission { relay: self.clone(), ip })
    }

    /// Notes one connection closed over a limit.
    pub(crate) fn refused(&self) {
        self.refused.fetch_add(1, Ordering::Relaxed);
    }

    /// Registers a room for a control connection, or takes it over from an
    /// older one with the same key. Queues `ready` for the new control
    /// connection, and after it every guest still waiting, so none is lost
    /// in a takeover. Answers the control connection's id, or a close code.
    pub(crate) fn register(&self, hello: &HostHello, ip: IpAddr, control: Link) -> Result<u64, u16> {
        let mut guard = self.lock();
        let state = &mut *guard;
        let id = self.next_id();
        let rooms = state.rooms.len();
        let room = match state.rooms.entry(hello.room) {
            Entry::Occupied(entry) => {
                let room = entry.into_mut();
                if !wire::same_key(&room.key, &hello.key) {
                    return Err(TAKEN);
                }
                if room.ip != ip {
                    if count(&state.rooms_per_ip, ip) >= self.config.max_rooms_per_ip {
                        self.refused();
                        return Err(LIMIT);
                    }
                    decrement(&mut state.rooms_per_ip, room.ip);
                    *state.rooms_per_ip.entry(ip).or_insert(0) += 1;
                    room.ip = ip;
                }
                let old = std::mem::replace(&mut room.control, control.clone());
                room.control_id = id;
                old.stop(REPLACED);
                room
            }
            Entry::Vacant(entry) => {
                if rooms >= self.config.max_rooms || count(&state.rooms_per_ip, ip) >= self.config.max_rooms_per_ip {
                    self.refused();
                    return Err(LIMIT);
                }
                *state.rooms_per_ip.entry(ip).or_insert(0) += 1;
                entry.insert(Room {
                    key: hello.key,
                    ip,
                    control_id: id,
                    control: control.clone(),
                    waiting: HashMap::new(),
                    pairs: Arc::new(AtomicUsize::new(0)),
                })
            }
        };
        // The control connection's queue is new and holds `control_queue`
        // frames, more than a room lets wait.
        let _ = control.out.try_send(Out::Text(Utf8Bytes::from_static(wire::READY)));
        let mut waiting: Vec<u64> = room.waiting.keys().copied().collect();
        waiting.sort_unstable();
        for conn in waiting {
            let _ = control.out.try_send(Out::Text(wire::guest(conn).into()));
        }
        Ok(id)
    }

    /// The control connection `control_id` ended. If it still holds the
    /// room, the room goes, and the guests still waiting are closed with
    /// 4404. Open pairs go on.
    pub(crate) fn unregister(&self, room: RoomId, control_id: u64) {
        let mut guard = self.lock();
        let state = &mut *guard;
        let Entry::Occupied(entry) = state.rooms.entry(room) else {
            return;
        };
        if entry.get().control_id != control_id {
            return;
        }
        let room = entry.remove();
        decrement(&mut state.rooms_per_ip, room.ip);
        for waiting in room.waiting.into_values() {
            // Stopped before its `paired` sender drops, so the guest reads
            // this code when it sees there is no pair.
            waiting.link.stop(NOT_FOUND);
        }
    }

    /// Adds a guest to a room and tells the host about it. Answers the
    /// guest's `conn`, or a close code.
    pub(crate) fn join(&self, room: RoomId, guest: Link, paired: oneshot::Sender<Paired>) -> Result<u64, u16> {
        let mut state = self.lock();
        let Some(room) = state.rooms.get_mut(&room) else {
            return Err(NOT_FOUND);
        };
        if room.waiting.len() >= self.config.max_waiting_per_room
            || room.pairs.load(Ordering::Relaxed) >= self.config.max_pairs_per_room
        {
            self.refused();
            return Err(LIMIT);
        }
        let conn = self.next_id();
        match room.control.out.try_send(Out::Text(wire::guest(conn).into())) {
            Ok(()) => {}
            // The host is not reading its control connection.
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.refused();
                return Err(LIMIT);
            }
            // Its control connection is on its way out.
            Err(mpsc::error::TrySendError::Closed(_)) => return Err(NOT_FOUND),
        }
        room.waiting.insert(conn, Waiting { link: guest, paired });
        Ok(conn)
    }

    /// Takes a waiting guest out of its room. False: it was not waiting
    /// any more, because the host accepted it or the room closed.
    pub(crate) fn leave(&self, room: RoomId, conn: u64) -> bool {
        let mut state = self.lock();
        state.rooms.get_mut(&room).and_then(|r| r.waiting.remove(&conn)).is_some()
    }

    /// Opens a pair between a waiting guest and the host's accept
    /// connection. Answers the host's side of it, or a close code.
    pub(crate) fn accept(self: &Arc<Self>, hello: &AcceptHello, host: Link) -> Result<Paired, u16> {
        let mut state = self.lock();
        let Some(room) = state.rooms.get_mut(&hello.room) else {
            return Err(NOT_FOUND);
        };
        if !wire::same_key(&room.key, &hello.key) {
            return Err(WRONG_KEY);
        }
        let Some(guest) = room.waiting.remove(&hello.conn) else {
            return Err(NOT_FOUND);
        };
        if room.pairs.load(Ordering::Relaxed) >= self.config.max_pairs_per_room {
            self.refused();
            guest.link.stop(LIMIT);
            return Err(LIMIT);
        }
        // Both `open`s are queued before either side forwards anything, so
        // each arrives before the first frame from the other side.
        let open = || Out::Text(Utf8Bytes::from_static(wire::OPEN));
        if guest.link.out.try_send(open()).is_err() || host.out.try_send(open()).is_err() {
            // One of the two connections is on its way out.
            guest.link.stop(NORMAL);
            return Err(NOT_FOUND);
        }
        room.pairs.fetch_add(1, Ordering::Relaxed);
        self.pairs.fetch_add(1, Ordering::Relaxed);
        let slot = Arc::new(PairSlot { room: room.pairs.clone(), relay: self.clone() });
        let theirs = Paired { peer: host, slot: slot.clone() };
        if guest.paired.send(theirs).is_err() {
            guest.link.stop(NORMAL);
            return Err(NOT_FOUND);
        }
        Ok(Paired { peer: guest.link, slot })
    }

    pub(crate) fn counts(&self) -> Counts {
        let state = self.lock();
        Counts {
            rooms: state.rooms.len(),
            pairs: self.pairs.load(Ordering::Relaxed),
            connections: state.connections,
            refused: self.refused.load(Ordering::Relaxed),
        }
    }
}

fn count(map: &HashMap<IpAddr, usize>, ip: IpAddr) -> usize {
    map.get(&ip).copied().unwrap_or(0)
}

fn decrement(map: &mut HashMap<IpAddr, usize>, ip: IpAddr) {
    if let Entry::Occupied(mut entry) = map.entry(ip) {
        if *entry.get() <= 1 {
            entry.remove();
        } else {
            *entry.get_mut() -= 1;
        }
    }
}
