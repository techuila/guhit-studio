//! What each endpoint does with its connection, from the first message to
//! the close (docs/RELAY.md, "Protocol, version 1").

use std::net::IpAddr;
use std::sync::Arc;

use tokio::sync::oneshot;
use tokio::time::Instant;

use crate::conn::{Conn, Ending, Event, Link, Out};
use crate::rate::Bucket;
use crate::rooms::{Paired, Relay};
use crate::wire::{self, RoomId, BAD_REQUEST, IDLE, NORMAL, NOT_ACCEPTED, NOT_FOUND, TOO_BIG};

/// `GET /v1/host`: registers the room, then tells the host about each guest
/// until the connection ends. The room ends with it.
pub(crate) async fn host(relay: Arc<Relay>, mut conn: Conn, ip: IpAddr) {
    let deadline = Instant::now().checked_add(relay.config.hello_timeout);
    let hello = match conn.next(deadline).await {
        Event::Text(text) => wire::host_hello(&text),
        other => return conn.close(other.ending(BAD_REQUEST)).await,
    };
    let Some(hello) = hello else {
        return conn.close(Ending::Close(BAD_REQUEST)).await;
    };
    let id = match relay.register(&hello, ip, conn.link()) {
        Ok(id) => id,
        Err(code) => return conn.close(Ending::Close(code)).await,
    };
    // The host sends nothing more here: any message is a bad request. A
    // takeover arrives as a stop with 4001.
    let ending = conn.next(None).await.ending(BAD_REQUEST);
    relay.unregister(hello.room, id);
    conn.close(ending).await;
}

/// `GET /v1/join/<room>`: waits in the room for the host's accept, then
/// forwards the guest's side of the pair.
pub(crate) async fn guest(relay: Arc<Relay>, mut conn: Conn, room: Option<RoomId>) {
    let Some(room) = room else {
        return conn.close(Ending::Close(NOT_FOUND)).await;
    };
    let (paired_tx, mut paired_rx) = oneshot::channel();
    let id = match relay.join(room, conn.link(), paired_tx) {
        Ok(id) => id,
        Err(code) => return conn.close(Ending::Close(code)).await,
    };
    let deadline = Instant::now().checked_add(relay.config.accept_timeout);
    let (paired, first) = tokio::select! {
        // The accept first: once it is in, a frame from the guest belongs
        // to the pair.
        biased;
        paired = &mut paired_rx => (paired.ok(), None),
        event = conn.next(deadline) => {
            if relay.leave(room, id) {
                return conn.close(event.ending(NOT_ACCEPTED)).await;
            }
            // Accepted, or the room closed, at this same moment.
            match paired_rx.try_recv() {
                Ok(paired) => (Some(paired), Some(event)),
                Err(_) => return conn.close(event.ending(NOT_ACCEPTED)).await,
            }
        }
    };
    match paired {
        Some(paired) => forward(&relay, conn, paired, first).await,
        // Taken out of the room without a pair: its control connection
        // ended, or the pair could not open. Either left a code.
        None => {
            let code = conn.stop_code().unwrap_or(NOT_FOUND);
            conn.close(Ending::Close(code)).await;
        }
    }
}

/// `GET /v1/accept`: opens the pair with a waiting guest, then forwards the
/// host's side of it.
pub(crate) async fn accept(relay: Arc<Relay>, mut conn: Conn) {
    let deadline = Instant::now().checked_add(relay.config.hello_timeout);
    let hello = match conn.next(deadline).await {
        Event::Text(text) => wire::accept_hello(&text),
        other => return conn.close(other.ending(BAD_REQUEST)).await,
    };
    let Some(hello) = hello else {
        return conn.close(Ending::Close(BAD_REQUEST)).await;
    };
    match relay.accept(&hello, conn.link()) {
        Ok(paired) => forward(&relay, conn, paired, None).await,
        Err(code) => conn.close(Ending::Close(code)).await,
    }
}

/// One direction of an open pair: binary frames from this connection's
/// client go to the other side, unchanged and in order. `first` is an event
/// read while the pair was opening.
async fn forward(relay: &Relay, mut conn: Conn, paired: Paired, mut first: Option<Event>) {
    // The slot is held until this side ends: the pair counts until both have.
    let Paired { peer, slot: _slot } = paired;
    let mut bucket = Bucket::new(relay.config.rate, relay.config.burst);
    let ending = loop {
        let event = match first.take() {
            Some(event) => event,
            None => conn.next(None).await,
        };
        let data = match event {
            Event::Binary(data) => data,
            Event::Deadline => continue,
            other => break pair_end(other, &peer),
        };
        if let Some(wait) = bucket.take(data.len()) {
            // Not reading meanwhile is what slows the sender: TCP pushes back.
            tokio::select! {
                biased;
                event = conn.halted() => break pair_end(event, &peer),
                () = tokio::time::sleep(wait) => {}
            }
        }
        // The queue is bounded, so a slow reader on the other side slows
        // this side down the same way.
        tokio::select! {
            biased;
            event = conn.halted() => break pair_end(event, &peer),
            sent = peer.out.send(Out::Data(data)) => {
                if sent.is_err() {
                    // The other side's writer stopped: it is closing.
                    break Ending::Close(NORMAL);
                }
            }
        }
    };
    conn.close(ending).await;
}

/// How one side of a pair ends on `event`, after telling the other side: a
/// text frame closes both with 4400, anything else closes the other with 1000.
fn pair_end(event: Event, peer: &Link) -> Ending {
    let (mine, theirs) = match event {
        Event::Text(_) => (Ending::Close(BAD_REQUEST), BAD_REQUEST),
        Event::Closed => (Ending::Answer, NORMAL),
        Event::Gone => (Ending::Drop, NORMAL),
        Event::TooBig => (Ending::CloseUnread(TOO_BIG), NORMAL),
        Event::Malformed => (Ending::CloseUnread(BAD_REQUEST), NORMAL),
        Event::Idle => (Ending::Close(IDLE), NORMAL),
        // The other side asked for this, so it knows.
        Event::Stopped(code) => return Ending::Close(code),
        // Not reached: `forward` handles these itself.
        Event::Binary(_) | Event::Deadline => (Ending::Close(NORMAL), NORMAL),
    };
    peer.stop(theirs);
    mine
}
