//! One WebSocket connection: a writer task that sends what is queued for it
//! and the pings, reads that notice silence, and the close.
//!
//! Only the connection's own task reads it and decides how it ends. Other
//! tasks reach it through its `Link`: they queue frames for its writer, or
//! ask it to close.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::ws::{CloseFrame, Message, Utf8Bytes, WebSocket};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{sleep_until, timeout_at, Instant};
use tungstenite::error::ProtocolError;

use crate::config::Config;
use crate::rooms::Admission;
use crate::wire::{self, BAD_REQUEST, IDLE, TOO_BIG};

/// What a connection's writer sends.
pub(crate) enum Out {
    Text(Utf8Bytes),
    Data(Bytes),
    /// A close frame with this code. The writer stops after it.
    Close(u16),
}

/// What other tasks hold to reach a connection.
#[derive(Clone)]
pub(crate) struct Link {
    /// The queue its writer sends from. Bounded, so a client that reads
    /// slowly pushes back on whoever sends to it.
    pub(crate) out: mpsc::Sender<Out>,
    stop: Arc<watch::Sender<Option<u16>>>,
}

impl Link {
    /// Asks the connection's own task to close it with `code`. The first
    /// code asked for wins.
    pub(crate) fn stop(&self, code: u16) {
        self.stop.send_if_modified(|current| {
            if current.is_some() {
                return false;
            }
            *current = Some(code);
            true
        });
    }
}

/// What `Conn::next` saw.
pub(crate) enum Event {
    Text(Utf8Bytes),
    Binary(Bytes),
    /// The client sent a close frame.
    Closed,
    /// The connection dropped, or ended without a close frame.
    Gone,
    /// A frame or message over the maximum.
    TooBig,
    /// A frame that breaks the WebSocket protocol, or text that is not UTF-8.
    Malformed,
    /// Nothing received for the idle time.
    Idle,
    /// The deadline passed.
    Deadline,
    /// Another task asked for a close with this code.
    Stopped(u16),
}

impl Event {
    /// How a connection outside an open pair ends on this event: any message
    /// is one it did not expect, and `late` is the code for a passed deadline.
    pub(crate) fn ending(self, late: u16) -> Ending {
        match self {
            Event::Text(_) | Event::Binary(_) => Ending::Close(BAD_REQUEST),
            Event::Closed => Ending::Answer,
            Event::Gone => Ending::Drop,
            Event::TooBig => Ending::CloseUnread(TOO_BIG),
            Event::Malformed => Ending::CloseUnread(BAD_REQUEST),
            Event::Idle => Ending::Close(IDLE),
            Event::Deadline => Ending::Close(late),
            Event::Stopped(code) => Ending::Close(code),
        }
    }
}

/// How the relay ends a connection.
pub(crate) enum Ending {
    /// Send a close frame with this code, then wait for the client's reply.
    Close(u16),
    /// Send a close frame with this code and read nothing more: after a
    /// frame over the maximum, or a malformed one, the stream cannot be read.
    CloseUnread(u16),
    /// Answer the client's close frame.
    Answer,
    /// The connection is gone: nothing to send.
    Drop,
}

pub(crate) struct Conn {
    reader: SplitStream<WebSocket>,
    link: Link,
    stop_rx: watch::Receiver<Option<u16>>,
    writer: JoinHandle<()>,
    /// A finished `JoinHandle` must not be polled again.
    writer_done: bool,
    /// When the client was last heard from.
    heard: Instant,
    idle: Duration,
    close_timeout: Duration,
    /// Frees this connection's place in the limits when it is dropped. None
    /// for a connection refused over a limit.
    _admission: Option<Admission>,
}

impl Conn {
    /// Starts the writer. `queue` is how many frames may wait for it.
    pub(crate) fn new(socket: WebSocket, config: &Config, queue: usize, admission: Option<Admission>) -> Conn {
        let (sink, reader) = socket.split();
        let (out, rx) = mpsc::channel(queue.max(1));
        let (stop, stop_rx) = watch::channel(None);
        Conn {
            reader,
            link: Link { out, stop: Arc::new(stop) },
            stop_rx,
            writer: tokio::spawn(write(sink, rx, config.ping_interval)),
            writer_done: false,
            heard: Instant::now(),
            idle: config.idle_timeout,
            close_timeout: config.close_timeout,
            _admission: admission,
        }
    }

    pub(crate) fn link(&self) -> Link {
        self.link.clone()
    }

    /// The code another task asked this connection to close with, if any.
    pub(crate) fn stop_code(&self) -> Option<u16> {
        *self.stop_rx.borrow()
    }

    /// The next message, or what ended the wait for it. Pings and pongs are
    /// taken here: they only show that the client is still there. Cancel
    /// safe: nothing read is lost when this is dropped.
    pub(crate) async fn next(&mut self, deadline: Option<Instant>) -> Event {
        loop {
            let idle_at = self.heard.checked_add(self.idle);
            tokio::select! {
                biased;
                code = stopped(&mut self.stop_rx) => return Event::Stopped(code),
                item = self.reader.next() => {
                    self.heard = Instant::now();
                    match item {
                        Some(Ok(Message::Text(text))) => return Event::Text(text),
                        Some(Ok(Message::Binary(data))) => return Event::Binary(data),
                        Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                        Some(Ok(Message::Close(_))) => return Event::Closed,
                        Some(Err(e)) => return read_error(e),
                        None => return Event::Gone,
                    }
                }
                _ = &mut self.writer, if !self.writer_done => {
                    self.writer_done = true;
                    return Event::Gone;
                }
                () = until(idle_at) => return Event::Idle,
                () = until(deadline) => return Event::Deadline,
            }
        }
    }

    /// Resolves when another task asks for a close, or the writer stops.
    /// For the waits that do not read: the rate limit and a full queue.
    pub(crate) async fn halted(&mut self) -> Event {
        tokio::select! {
            biased;
            code = stopped(&mut self.stop_rx) => Event::Stopped(code),
            _ = &mut self.writer, if !self.writer_done => {
                self.writer_done = true;
                Event::Gone
            }
        }
    }

    /// Ends the connection. The socket closes when this returns, at the
    /// latest `close_timeout` from now.
    pub(crate) async fn close(mut self, ending: Ending) {
        let limit = Instant::now().checked_add(self.close_timeout);
        match ending {
            Ending::Drop => {}
            Ending::Answer => {
                // The reply to the client's close frame is queued, and the
                // next read sends it.
                let _ = within(limit, self.reader.next()).await;
            }
            Ending::Close(code) => {
                // Queued after anything already waiting, so a pair's last
                // frames still arrive before the close.
                if let Some(Ok(())) = within(limit, self.link.out.send(Out::Close(code))).await {
                    // Drop what the client still sends until it answers.
                    let reader = &mut self.reader;
                    let _ = within(limit, async { while let Some(Ok(_)) = reader.next().await {} }).await;
                }
            }
            Ending::CloseUnread(code) => {
                let _ = within(limit, self.link.out.send(Out::Close(code))).await;
                if !self.writer_done && within(limit, &mut self.writer).await.is_some() {
                    self.writer_done = true;
                }
                // Keep the socket open, unread, so the client has time to
                // read the close frame. A socket closed with data unread
                // resets the connection, and a reset can lose the frame.
                until(limit).await;
            }
        }
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        // The writer holds the other half of the socket.
        self.writer.abort();
    }
}

/// The writer: sends what is queued, and a ping every `every`. Stops after
/// a close frame, when the queue closes, or when a send fails.
async fn write(mut sink: SplitSink<WebSocket, Message>, mut queue: mpsc::Receiver<Out>, every: Duration) {
    let every = every.max(Duration::from_millis(1));
    let mut ping_at = Instant::now().checked_add(every);
    loop {
        let message = tokio::select! {
            out = queue.recv() => match out {
                Some(Out::Text(text)) => Message::Text(text),
                Some(Out::Data(data)) => Message::Binary(data),
                Some(Out::Close(code)) => {
                    let reason = Utf8Bytes::from_static(wire::reason(code));
                    let _ = sink.send(Message::Close(Some(CloseFrame { code, reason }))).await;
                    return;
                }
                None => return,
            },
            () = until(ping_at) => {
                ping_at = Instant::now().checked_add(every);
                Message::Ping(Bytes::new())
            }
        };
        if sink.send(message).await.is_err() {
            return;
        }
    }
}

/// Sorts a read error. A frame over the maximum and a malformed frame get
/// their own close codes; anything else means the connection is gone.
fn read_error(e: axum::Error) -> Event {
    let Ok(e) = e.into_inner().downcast::<tungstenite::Error>() else {
        return Event::Gone;
    };
    match *e {
        tungstenite::Error::Capacity(_) => Event::TooBig,
        tungstenite::Error::Protocol(ProtocolError::ResetWithoutClosingHandshake) => Event::Gone,
        tungstenite::Error::Protocol(_) | tungstenite::Error::Utf8(_) => Event::Malformed,
        _ => Event::Gone,
    }
}

/// Resolves with the code once another task asks for a close.
async fn stopped(rx: &mut watch::Receiver<Option<u16>>) -> u16 {
    let code = rx.wait_for(Option::is_some).await.ok().and_then(|code| *code);
    match code {
        Some(code) => code,
        // The sender lives in the connection's own link, so it outlives this.
        None => std::future::pending().await,
    }
}

/// Sleeps until `at`. None is a time too far away to name: never.
pub(crate) async fn until(at: Option<Instant>) {
    match at {
        Some(at) => sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// Runs `fut` until `limit`. None: it did not finish in time.
async fn within<F: Future>(limit: Option<Instant>, fut: F) -> Option<F::Output> {
    match limit {
        Some(at) => timeout_at(at, fut).await.ok(),
        None => Some(fut.await),
    }
}
