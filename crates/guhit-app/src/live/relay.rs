//! The app's side of the relay (DECISIONS D32, docs/RELAY.md). The relay
//! pairs a guest with the host and forwards their bytes. The session inside
//! is the same pinned TLS as on a direct connection, end to end, so the relay
//! cannot read or change it.
//!
//! - A host registers its session's room on a control connection
//!   (`host_loop`), opens an accept connection for each guest the relay
//!   announces, and hands it to `host::connection` like a direct one. It
//!   keeps the registration up while the session runs, with the same room
//!   and key, so invites keep working.
//! - A guest joins the room (`join`).
//!
//! Either way the WebSocket becomes a byte stream (`into_io`), so the session
//! code does not know which way a connection came.

use std::net::IpAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use guhit_model::LiveRelay;
use rustls::pki_types::ServerName;
use rustls_platform_verifier::BuilderVerifierExt;
use serde::Deserialize;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio_rustls::TlsConnector;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use url::Url;

use super::host::{self, Host};
use super::tls::BoxIo;
use super::wire::CLOSE_WAIT;
use crate::AppService;

/// Version of the relay's messages.
const RELAY_VERSION: u32 = 1;
/// Largest WebSocket message and frame, as the relay allows.
const MAX_MESSAGE_BYTES: usize = 256 * 1024;
/// The session's bytes go to the relay in binary frames of at most this.
const CHUNK_BYTES: usize = 64 * 1024;
/// Bytes buffered each way between the session and the WebSocket.
const BUFFER_BYTES: usize = 256 * 1024;
/// TCP, TLS and the WebSocket handshake with the relay, together.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The relay answers a registration or an accept within this.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);
/// A guest waits this long for the host to accept. The relay turns the guest
/// away after 10 s, so this only matters when the relay goes quiet.
const JOIN_TIMEOUT: Duration = Duration::from_secs(15);
/// The relay pings every 20 s, so a control connection with no frame at all
/// for this long is lost.
const CONTROL_SILENCE: Duration = Duration::from_secs(60);
/// The waits before registering again after a failure, then `RETRY_EVERY`.
const RETRY_DELAYS: [Duration; 5] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(16),
];
const RETRY_EVERY: Duration = Duration::from_secs(30);
/// Longest relay address, so an invite with every address, a long project
/// name and the relay stays well under the length guests accept.
const MAX_URL_CHARS: usize = 512;

// ---------------------------------------------------------------- addresses

/// A relay's address, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayUrl {
    /// As the invite carries it: canonical, no trailing slash.
    base: String,
    host: url::Host<String>,
    port: u16,
    secure: bool,
}

impl RelayUrl {
    pub fn as_str(&self) -> &str {
        &self.base
    }

    /// One of the relay's endpoints: the base address, then `path`.
    fn endpoint(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
}

/// Check a relay address, from the `live_relay` setting or an invite:
/// `wss://`, or `ws://` for a relay on this computer, with a host and maybe a
/// path prefix, and no user, query or fragment. The error finishes a
/// sentence: why the address cannot be used.
pub fn check_url(text: &str) -> Result<RelayUrl, String> {
    let url = Url::parse(text.trim()).map_err(|_| "it is not a web address".to_string())?;
    let secure = match url.scheme() {
        "wss" => true,
        "ws" => false,
        _ => return Err("it must start with wss://".to_string()),
    };
    let host = url.host().ok_or_else(|| "it has no host name".to_string())?.to_owned();
    if !secure && !is_loopback(&host) {
        return Err("ws:// works only for a relay on this computer".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("it cannot carry a user name or password".to_string());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("it cannot have a query (?) or a fragment (#)".to_string());
    }
    let port = url.port_or_known_default().ok_or_else(|| "it has no port".to_string())?;
    let base = url.as_str().trim_end_matches('/').to_string();
    if base.len() > MAX_URL_CHARS {
        return Err(format!("it is longer than {MAX_URL_CHARS} characters"));
    }
    Ok(RelayUrl { base, host, port, secure })
}

fn is_loopback(host: &url::Host<String>) -> bool {
    match host {
        url::Host::Domain(name) => name.eq_ignore_ascii_case("localhost"),
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
    }
}

// --------------------------------------------------------------- connecting

type Socket = WebSocketStream<BoxIo>;

/// TLS to the relay: the operating system's trust store checks its
/// certificate, as a browser would. The ring provider is named here, never
/// taken from the process default that other parts of the app install.
fn tls_config() -> Result<Arc<rustls::ClientConfig>, rustls::Error> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    if let Some(config) = CONFIG.get() {
        return Ok(config.clone());
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_platform_verifier()?
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(CONFIG.get_or_init(|| Arc::new(config)).clone())
}

/// A WebSocket to one of the relay's endpoints: TCP, TLS for `wss`, then the
/// WebSocket handshake, within `CONNECT_TIMEOUT` together. None when any of it
/// fails.
async fn socket_to(relay: &RelayUrl, path: &str) -> Option<Socket> {
    let connect = async {
        let tcp = match &relay.host {
            url::Host::Domain(name) => TcpStream::connect((name.as_str(), relay.port)).await,
            url::Host::Ipv4(ip) => TcpStream::connect((*ip, relay.port)).await,
            url::Host::Ipv6(ip) => TcpStream::connect((*ip, relay.port)).await,
        }
        .ok()?;
        let _ = tcp.set_nodelay(true);
        let io: BoxIo = if relay.secure {
            let name = match &relay.host {
                url::Host::Domain(name) => ServerName::try_from(name.clone()).ok()?,
                url::Host::Ipv4(ip) => ServerName::IpAddress(IpAddr::V4(*ip).into()),
                url::Host::Ipv6(ip) => ServerName::IpAddress(IpAddr::V6(*ip).into()),
            };
            let tls = TlsConnector::from(tls_config().ok()?).connect(name, tcp).await.ok()?;
            Box::new(tls)
        } else {
            Box::new(tcp)
        };
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_MESSAGE_BYTES))
            .max_frame_size(Some(MAX_MESSAGE_BYTES));
        let (socket, _) = tokio_tungstenite::client_async_with_config(relay.endpoint(path), io, Some(config))
            .await
            .ok()?;
        Some(socket)
    };
    tokio::time::timeout(CONNECT_TIMEOUT, connect).await.ok().flatten()
}

/// What the relay says before a pair opens.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Said {
    /// The host's room is registered.
    Ready,
    /// A guest is waiting for the host to accept it.
    Guest { conn: String },
    /// The pair is open: binary frames from here on.
    Open,
    /// Something this app does not know.
    #[serde(other)]
    Other,
}

/// The relay's next message before a pair opens, or how the connection
/// ended: its close code, if it sent one.
enum Heard {
    Said(Said),
    Closed(Option<u16>),
}

async fn hear(socket: &mut Socket) -> Heard {
    loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => {
                return Heard::Said(serde_json::from_str(text.as_str()).unwrap_or(Said::Other))
            }
            // tungstenite answers pings as it reads.
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
            Some(Ok(Message::Close(frame))) => return Heard::Closed(frame.map(|f| u16::from(f.code))),
            // A binary frame before the pair is open, a failure, or the end.
            _ => return Heard::Closed(None),
        }
    }
}

/// A WebSocket as the byte stream a session runs over. What the session
/// writes goes out in binary frames of at most `CHUNK_BYTES`; binary frames
/// that arrive are read back in order. Two pumps, one each way, so a
/// direction that is full never holds up the other. When either ends (the
/// session closed its side, the relay closed, sent a text frame or failed),
/// both stop, the session reads the end of the stream, and the WebSocket
/// closes.
fn into_io(socket: Socket) -> BoxIo {
    let (session, pumps) = tokio::io::duplex(BUFFER_BYTES);
    let (mut from_session, mut to_session) = tokio::io::split(pumps);
    let (mut sink, mut stream) = socket.split();
    let stop = watch::channel(false).0;

    let done = stop.clone();
    tokio::spawn(async move {
        let mut halt = done.subscribe();
        let mut buf = vec![0u8; CHUNK_BYTES];
        loop {
            let n = tokio::select! {
                _ = halt.wait_for(|s| *s) => break,
                read = from_session.read(&mut buf) => match read {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                },
            };
            let sent = tokio::select! {
                _ = halt.wait_for(|s| *s) => break,
                sent = sink.send(Message::binary(buf[..n].to_vec())) => sent,
            };
            if sent.is_err() {
                break;
            }
        }
        done.send_replace(true);
        // A close frame, so the relay closes the other side at once.
        let _ = tokio::time::timeout(CLOSE_WAIT, sink.close()).await;
    });

    tokio::spawn(async move {
        let mut halt = stop.subscribe();
        loop {
            let next = tokio::select! {
                _ = halt.wait_for(|s| *s) => break,
                next = stream.next() => next,
            };
            let data = match next {
                Some(Ok(Message::Binary(data))) => data,
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                // A text frame is not part of an open pair; a close or a
                // failure is the end.
                _ => break,
            };
            let written = tokio::select! {
                _ = halt.wait_for(|s| *s) => break,
                written = to_session.write_all(&data) => written,
            };
            if written.is_err() {
                break;
            }
        }
        stop.send_replace(true);
        let _ = to_session.shutdown().await;
    });

    Box::new(session)
}

// --------------------------------------------------------------------- host

/// A hosted session's room on the relay.
pub(crate) struct Room {
    pub(crate) url: RelayUrl,
    /// 16 random bytes, base64url. In the invite.
    pub(crate) id: String,
    /// 32 random bytes, base64url. Proves to the relay that a connection is
    /// the host's. Never in the invite.
    key: String,
}

impl Room {
    pub(crate) fn new(url: RelayUrl, id: String, key: String) -> Self {
        Self { url, id, key }
    }
}

/// How a control connection ended.
enum Ended {
    /// It never registered: no connection, or no `ready`.
    Failed,
    /// It registered, then the connection was lost.
    Lost,
    /// The session ended.
    Stopped,
}

/// Keep the session's room registered while the session runs: register,
/// accept the guests the relay announces, and when the control connection is
/// lost or cannot be made, try again after 1, 2, 4, 8, 16 and then every
/// 30 s with the same room and key. The status's relay is Connecting until
/// the first attempt resolves, Ready while registered, Unavailable between.
pub(crate) async fn host_loop(app: AppService, host: Arc<Host>, room: Room) {
    let room = Arc::new(room);
    let mut stop = host.stopped();
    let mut failures = 0;
    loop {
        match control(&app, &host, &room, &mut stop).await {
            Ended::Stopped => return,
            Ended::Lost => failures = 0,
            Ended::Failed => {}
        }
        host::relay_changed(&app, &host, LiveRelay::Unavailable);
        let wait = RETRY_DELAYS.get(failures).copied().unwrap_or(RETRY_EVERY);
        failures += 1;
        tokio::select! {
            _ = stop.wait_for(|s| *s) => return,
            _ = tokio::time::sleep(wait) => {}
        }
    }
}

/// One control connection: register the room, then admit every guest the
/// relay announces until the connection is lost or the session ends.
async fn control(app: &AppService, host: &Arc<Host>, room: &Arc<Room>, stop: &mut watch::Receiver<bool>) -> Ended {
    let register = async {
        let mut socket = socket_to(&room.url, "/v1/host").await?;
        let hello = json!({ "v": RELAY_VERSION, "room": room.id, "key": room.key });
        socket.send(Message::text(hello.to_string())).await.ok()?;
        match tokio::time::timeout(ANSWER_TIMEOUT, hear(&mut socket)).await {
            Ok(Heard::Said(Said::Ready)) => Some(socket),
            _ => None,
        }
    };
    let mut socket = tokio::select! {
        _ = stop.wait_for(|s| *s) => return Ended::Stopped,
        socket = register => match socket {
            Some(socket) => socket,
            None => return Ended::Failed,
        },
    };
    host::relay_changed(app, host, LiveRelay::Ready);
    loop {
        let next = tokio::select! {
            _ = stop.wait_for(|s| *s) => None,
            next = tokio::time::timeout(CONTROL_SILENCE, socket.next()) => Some(next),
        };
        let Some(next) = next else {
            // Closed properly, the relay drops the room at once.
            let _ = tokio::time::timeout(CLOSE_WAIT, socket.close(None)).await;
            return Ended::Stopped;
        };
        match next {
            Ok(Some(Ok(Message::Text(text)))) => {
                if let Ok(Said::Guest { conn }) = serde_json::from_str(text.as_str()) {
                    admit(app, host, room, conn);
                }
            }
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {}
            // Closed, failed, silent for too long, or a binary frame, which a
            // control connection never carries.
            _ => return Ended::Lost,
        }
    }
}

/// A guest the relay announced. It takes one of the places for connections
/// still authenticating, as a direct one does; with none free it is ignored
/// and the relay turns it away. Its pair is opened on an accept connection,
/// and the session runs over it like over a direct connection.
fn admit(app: &AppService, host: &Arc<Host>, room: &Arc<Room>, conn: String) {
    // The relay numbers its connections; anything else is not a guest.
    if conn.is_empty() || conn.len() > 20 || !conn.bytes().all(|b| b.is_ascii_digit()) {
        return;
    }
    let Ok(permit) = host.waiting.clone().try_acquire_owned() else {
        return;
    };
    let (app, host, room) = (app.clone(), host.clone(), room.clone());
    tokio::spawn(async move {
        let mut stop = host.stopped();
        let io = tokio::select! {
            _ = stop.wait_for(|s| *s) => return,
            io = accept(&room, &conn) => io,
        };
        if let Some(io) = io {
            host::connection(app, host, io, permit).await;
        }
    });
}

/// Open the pair for the guest `conn`: an accept connection, then the
/// relay's `open`. The guest speaks first, with its TLS hello.
async fn accept(room: &Room, conn: &str) -> Option<BoxIo> {
    let mut socket = socket_to(&room.url, "/v1/accept").await?;
    let hello = json!({ "v": RELAY_VERSION, "room": room.id, "key": room.key, "conn": conn });
    socket.send(Message::text(hello.to_string())).await.ok()?;
    match tokio::time::timeout(ANSWER_TIMEOUT, hear(&mut socket)).await {
        Ok(Heard::Said(Said::Open)) => Some(into_io(socket)),
        _ => None,
    }
}

// -------------------------------------------------------------------- guest

/// Why joining through the relay failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinError {
    /// No such room (4404): the host is not online, or the session ended.
    NotOnline,
    /// The host did not accept in time (4408).
    NoAnswer,
    /// The relay is over one of its limits (4429).
    Busy,
    /// No connection to the relay, or its TLS or WebSocket handshake failed.
    Unreachable,
    /// The relay closed the connection for another reason, or the session's
    /// own handshake through it failed.
    Unexpected,
}

/// Join the session's room: the byte stream to the host, once the host has
/// accepted.
pub(crate) async fn join(relay: &RelayUrl, room: &str) -> Result<BoxIo, JoinError> {
    let mut socket = socket_to(relay, &format!("/v1/join/{room}")).await.ok_or(JoinError::Unreachable)?;
    match tokio::time::timeout(JOIN_TIMEOUT, hear(&mut socket)).await {
        Ok(Heard::Said(Said::Open)) => Ok(into_io(socket)),
        Ok(Heard::Closed(Some(4404))) => Err(JoinError::NotOnline),
        Ok(Heard::Closed(Some(4408))) | Err(_) => Err(JoinError::NoAnswer),
        Ok(Heard::Closed(Some(4429))) => Err(JoinError::Busy),
        Ok(_) => Err(JoinError::Unexpected),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Instant;

    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use guhit_model::*;
    use serde_json::Value;
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
    use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
    use tokio_tungstenite::tungstenite::protocol::CloseFrame;

    use super::*;
    use crate::live::invite::Invite;

    /// Longest a test waits for something that should happen at once.
    const WAIT: Duration = Duration::from_secs(10);

    type Server = WebSocketStream<TcpStream>;

    #[test]
    fn relay_addresses_are_checked() {
        let ok = |text: &str| check_url(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        let url = ok("wss://relay.example.com");
        assert_eq!(url.as_str(), "wss://relay.example.com");
        assert_eq!(url.endpoint("/v1/host"), "wss://relay.example.com/v1/host");
        assert_eq!((url.port, url.secure), (443, true));
        assert_eq!(ok("wss://relay.example.com/").as_str(), "wss://relay.example.com");
        // A path prefix, a port, and the case of the scheme and host.
        let url = ok(" WSS://Relay.Example.COM:8443/guhit/ ");
        assert_eq!(url.as_str(), "wss://relay.example.com:8443/guhit");
        assert_eq!(url.endpoint("/v1/join/abc"), "wss://relay.example.com:8443/guhit/v1/join/abc");
        assert_eq!(url.port, 8443);
        // Plain WebSockets only on this computer.
        for local in ["ws://127.0.0.1:1470", "ws://127.9.9.9:1470", "ws://localhost:1470", "ws://[::1]:1470"] {
            assert!(!ok(local).secure, "{local}");
        }
        assert_eq!(ok("ws://localhost").port, 80);

        let why = |text: &str| check_url(text).unwrap_err();
        assert_eq!(why("ws://relay.example.com"), "ws:// works only for a relay on this computer");
        assert_eq!(why("ws://192.168.1.20:1470"), "ws:// works only for a relay on this computer");
        assert_eq!(why("https://relay.example.com"), "it must start with wss://");
        assert_eq!(why("relay.example.com"), "it is not a web address");
        assert_eq!(why(""), "it is not a web address");
        assert_eq!(why("wss://"), "it is not a web address");
        assert_eq!(why("wss://ana:secret@relay.example.com"), "it cannot carry a user name or password");
        assert_eq!(why("wss://ana@relay.example.com"), "it cannot carry a user name or password");
        assert_eq!(why("wss://relay.example.com/?room=1"), "it cannot have a query (?) or a fragment (#)");
        assert_eq!(why("wss://relay.example.com/#top"), "it cannot have a query (?) or a fragment (#)");
        let long = format!("wss://relay.example.com/{}", "a".repeat(MAX_URL_CHARS));
        assert_eq!(why(&long), "it is longer than 512 characters");
    }

    /// Accept one WebSocket, and the path it asked for.
    // The callback's error type is tungstenite's, large or not.
    #[allow(clippy::result_large_err)]
    async fn serve_one(listener: &TcpListener) -> (Server, String) {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut path = String::new();
        let socket = tokio_tungstenite::accept_hdr_async(tcp, |req: &Request, res: Response| {
            path = req.uri().path().to_string();
            Ok(res)
        })
        .await
        .unwrap();
        (socket, path)
    }

    async fn listen() -> (TcpListener, String) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        (listener, url)
    }

    /// A WebSocket server on loopback, and a connection to it made the way
    /// the app makes them.
    async fn pair() -> (Socket, Server) {
        let (listener, url) = listen().await;
        let url = check_url(&url).unwrap();
        let (client, (server, path)) = tokio::join!(socket_to(&url, "/v1/test"), serve_one(&listener));
        assert_eq!(path, "/v1/test");
        (client.expect("the client connects"), server)
    }

    /// Read until the stream ends.
    async fn until_end(io: &mut BoxIo) -> Vec<u8> {
        let mut all = vec![];
        tokio::time::timeout(WAIT, io.read_to_end(&mut all)).await.expect("the stream should end").unwrap();
        all
    }

    /// Wait for a WebSocket to close.
    async fn closes(server: &mut Server, what: &str) {
        let closed = tokio::time::timeout(WAIT, async {
            loop {
                match server.next().await {
                    Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                    Some(Ok(_)) => {}
                }
            }
        })
        .await;
        assert!(closed.is_ok(), "{what} should close");
    }

    #[tokio::test]
    async fn a_socket_carries_bytes_both_ways() {
        let (client, mut server) = pair().await;
        let mut io = into_io(client);

        // Out: binary frames of at most 64 KiB, in order.
        let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let read = async {
            let mut got = vec![];
            while got.len() < data.len() {
                match tokio::time::timeout(WAIT, server.next()).await.unwrap() {
                    Some(Ok(Message::Binary(frame))) => {
                        assert!(frame.len() <= CHUNK_BYTES, "a frame of {}", frame.len());
                        got.extend_from_slice(&frame);
                    }
                    other => panic!("unexpected {other:?}"),
                }
            }
            got
        };
        let (written, got) = tokio::join!(io.write_all(&data), read);
        written.unwrap();
        assert_eq!(got, data);

        // In: whatever frames the relay forwards, read back in order.
        server.send(Message::binary(b"hello ".to_vec())).await.unwrap();
        server.send(Message::binary(vec![7u8; 200_000])).await.unwrap();
        let mut back = vec![0u8; 200_006];
        tokio::time::timeout(WAIT, io.read_exact(&mut back)).await.unwrap().unwrap();
        assert_eq!(&back[..6], b"hello ");
        assert!(back[6..].iter().all(|b| *b == 7));
    }

    #[tokio::test]
    async fn either_side_ending_ends_the_stream() {
        // The relay closes the pair: what came before is read, then the end.
        let (client, mut server) = pair().await;
        let mut io = into_io(client);
        server.send(Message::binary(b"last words".to_vec())).await.unwrap();
        server.close(None).await.unwrap();
        assert_eq!(until_end(&mut io).await, b"last words");

        // A text frame is not part of an open pair: the end too, and the
        // WebSocket closes.
        let (client, mut server) = pair().await;
        let mut io = into_io(client);
        server.send(Message::text(r#"{"type":"open"}"#)).await.unwrap();
        assert_eq!(until_end(&mut io).await, b"");
        closes(&mut server, "the WebSocket after a text frame").await;

        // The session closing its side closes the WebSocket.
        let (client, mut server) = pair().await;
        let mut io = into_io(client);
        io.write_all(b"bye").await.unwrap();
        io.shutdown().await.unwrap();
        drop(io);
        match tokio::time::timeout(WAIT, server.next()).await.unwrap() {
            Some(Ok(Message::Binary(frame))) => assert_eq!(&frame[..], b"bye"),
            other => panic!("unexpected {other:?}"),
        }
        closes(&mut server, "the WebSocket after the session's end").await;
    }

    #[tokio::test]
    async fn pings_are_answered_while_the_session_is_quiet() {
        let (client, mut server) = pair().await;
        let _io = into_io(client);
        server.send(Message::Ping(b"there?".to_vec().into())).await.unwrap();
        let answer = tokio::time::timeout(WAIT, server.next()).await.unwrap().unwrap().unwrap();
        assert_eq!(answer, Message::Pong(b"there?".to_vec().into()));
    }

    fn close_with(code: u16) -> Message {
        Message::Close(Some(CloseFrame { code: CloseCode::from(code), reason: "".into() }))
    }

    #[tokio::test]
    async fn joining_tells_why_the_relay_turned_a_guest_away() {
        let answers = [
            (4404, JoinError::NotOnline),
            (4408, JoinError::NoAnswer),
            (4429, JoinError::Busy),
            (4400, JoinError::Unexpected),
            (1000, JoinError::Unexpected),
        ];
        for (code, why) in answers {
            let (listener, url) = listen().await;
            let relay = tokio::spawn(async move {
                let (mut socket, path) = serve_one(&listener).await;
                let _ = socket.send(close_with(code)).await;
                path
            });
            let url = check_url(&url).unwrap();
            assert_eq!(join(&url, "AAAAAAAAAAAAAAAAAAAAAA").await.err(), Some(why), "{code}");
            assert_eq!(relay.await.unwrap(), "/v1/join/AAAAAAAAAAAAAAAAAAAAAA");
        }

        // The pair opens: a stream to the host.
        let (listener, url) = listen().await;
        let relay = tokio::spawn(async move {
            let (mut socket, _) = serve_one(&listener).await;
            socket.send(Message::text(r#"{"type":"open"}"#)).await.unwrap();
            socket.send(Message::binary(b"from the host".to_vec())).await.unwrap();
            socket
        });
        let mut io = join(&check_url(&url).unwrap(), "room").await.unwrap();
        let mut got = [0u8; 13];
        tokio::time::timeout(WAIT, io.read_exact(&mut got)).await.unwrap().unwrap();
        assert_eq!(&got, b"from the host");
        drop(relay.await.unwrap());

        // Nothing there.
        let (listener, url) = listen().await;
        drop(listener);
        assert_eq!(join(&check_url(&url).unwrap(), "room").await.err(), Some(JoinError::Unreachable));
    }

    async fn said(socket: &mut Server) -> Value {
        match tokio::time::timeout(WAIT, socket.next()).await.expect("a message from the app") {
            Some(Ok(Message::Text(text))) => serde_json::from_str(text.as_str()).unwrap(),
            other => panic!("expected a text message, got {other:?}"),
        }
    }

    async fn relay_is(app: &AppService, relay: LiveRelay) {
        let deadline = Instant::now() + WAIT;
        while app.live_status().await.relay != relay {
            assert!(Instant::now() < deadline, "the relay never became {relay:?}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// A computer with a name, `settings` in its settings.json, and a project.
    async fn computer(dir: &std::path::Path, settings: Value, project: bool) -> AppService {
        std::fs::write(dir.join("settings.json"), settings.to_string()).unwrap();
        let app = AppService::new_sandboxed(dir.to_path_buf());
        if project {
            app.handle("hub_create", json!({ "name": "Bahay" })).await.unwrap();
        }
        app
    }

    async fn host_status(app: &AppService) -> LiveStatus {
        serde_json::from_value(app.handle("live_host", json!({})).await.unwrap()).unwrap()
    }

    #[tokio::test]
    async fn a_host_keeps_its_room_registered() {
        let (listener, url) = listen().await;
        let dir = tempfile::tempdir().unwrap();
        let settings = json!({ "profile_name": "Ana", "live_relay": url, "live_direct": false });
        let app = computer(dir.path(), settings, true).await;
        let started = host_status(&app).await;
        assert_eq!(started.relay, LiveRelay::Connecting);
        assert!(started.addresses.is_empty(), "direct connections are off");
        let invite = Invite::decode(started.invite.as_deref().unwrap()).unwrap();
        assert!(invite.addrs.is_empty());
        let room = invite.relay.expect("the invite names the relay");
        assert_eq!(room.url, url);

        // The host registers its room with a key the invite does not carry.
        let (mut control, path) = serve_one(&listener).await;
        assert_eq!(path, "/v1/host");
        let hello = said(&mut control).await;
        assert_eq!((hello["v"].as_u64(), hello["room"].as_str()), (Some(1), Some(room.room.as_str())));
        let key = hello["key"].as_str().unwrap().to_string();
        assert_eq!(URL_SAFE_NO_PAD.decode(&key).unwrap().len(), 32);
        assert_eq!(hello.as_object().unwrap().len(), 3);
        control.send(Message::text(r#"{"type":"ready"}"#)).await.unwrap();
        relay_is(&app, LiveRelay::Ready).await;

        // The relay pings; the host answers.
        control.send(Message::Ping(b"hi".to_vec().into())).await.unwrap();
        assert_eq!(control.next().await.unwrap().unwrap(), Message::Pong(b"hi".to_vec().into()));

        // The connection drops: unavailable, then back after a second with
        // the same room and key.
        drop(control);
        relay_is(&app, LiveRelay::Unavailable).await;
        let (mut control, _) = tokio::time::timeout(WAIT, serve_one(&listener)).await.unwrap();
        let again = said(&mut control).await;
        assert_eq!(again, hello);
        control.send(Message::text(r#"{"type":"ready"}"#)).await.unwrap();
        relay_is(&app, LiveRelay::Ready).await;

        // Ending the session closes the control connection.
        app.handle("live_leave", json!({})).await.unwrap();
        closes(&mut control, "the control connection").await;
        assert_eq!(app.live_status().await.relay, LiveRelay::Off);
    }

    #[tokio::test]
    async fn a_host_whose_relay_is_down_says_so_and_keeps_trying() {
        let (listener, url) = listen().await;
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let dir = tempfile::tempdir().unwrap();
        let app = computer(dir.path(), json!({ "profile_name": "Ana", "live_relay": url }), true).await;
        let started = host_status(&app).await;
        assert_eq!(started.addresses.len(), 1, "direct connections still work");
        relay_is(&app, LiveRelay::Unavailable).await;

        // The relay comes up: the next try registers.
        let listener = TcpListener::bind(addr).await.unwrap();
        let (mut control, _) = tokio::time::timeout(WAIT, serve_one(&listener)).await.unwrap();
        said(&mut control).await;
        control.send(Message::text(r#"{"type":"ready"}"#)).await.unwrap();
        relay_is(&app, LiveRelay::Ready).await;
    }

    /// What the stand-in relay knows.
    #[derive(Default)]
    struct Mini {
        /// The host's key, once it registered.
        key: Option<String>,
        /// Tells the host's control connection about a guest.
        control: Option<mpsc::UnboundedSender<String>>,
        /// Guests waiting for the host's accept, by their number.
        waiting: HashMap<String, Server>,
        /// Guests that asked to join.
        joins: usize,
    }

    /// Just enough of guhit-relay for these tests: one room, each guest
    /// announced to the host and paired with its accept, binary frames
    /// forwarded both ways. The key is checked, the room id is not.
    async fn mini_relay() -> (String, Arc<Mutex<Mini>>, tokio::task::JoinHandle<()>) {
        let (listener, url) = listen().await;
        let mini: Arc<Mutex<Mini>> = Arc::default();
        let state = mini.clone();
        let task = tokio::spawn(async move {
            loop {
                let (socket, path) = serve_one(&listener).await;
                tokio::spawn(mini_connection(state.clone(), socket, path));
            }
        });
        (url, mini, task)
    }

    async fn mini_connection(mini: Arc<Mutex<Mini>>, mut socket: Server, path: String) {
        if path == "/v1/host" {
            let hello = said(&mut socket).await;
            let (tx, mut guests) = mpsc::unbounded_channel::<String>();
            {
                let mut m = mini.lock().unwrap();
                m.key = hello["key"].as_str().map(str::to_string);
                m.control = Some(tx.clone());
            }
            socket.send(Message::text(r#"{"type":"ready"}"#)).await.unwrap();
            // Announce guests until the host goes.
            loop {
                let conn = tokio::select! {
                    conn = guests.recv() => conn,
                    next = socket.next() => if matches!(next, Some(Ok(_))) { continue } else { None },
                };
                let Some(conn) = conn else { break };
                let announce = json!({ "type": "guest", "conn": conn }).to_string();
                if socket.send(Message::text(announce)).await.is_err() {
                    break;
                }
            }
            // The room is gone with the host: joins are turned away now.
            let mut m = mini.lock().unwrap();
            if m.control.as_ref().is_some_and(|c| c.same_channel(&tx)) {
                m.control = None;
            }
        } else if path.starts_with("/v1/join/") {
            let (conn, control) = {
                let mut m = mini.lock().unwrap();
                m.joins += 1;
                let conn = m.joins.to_string();
                m.waiting.insert(conn.clone(), socket);
                (conn, m.control.clone())
            };
            if control.is_none_or(|c| c.send(conn.clone()).is_err()) {
                let socket = mini.lock().unwrap().waiting.remove(&conn);
                if let Some(mut socket) = socket {
                    let _ = socket.send(close_with(4404)).await;
                }
            }
        } else if path == "/v1/accept" {
            let hello = said(&mut socket).await;
            let guest = {
                let mut m = mini.lock().unwrap();
                let conn = hello["conn"].as_str().unwrap_or_default();
                if hello["key"].as_str() == m.key.as_deref() {
                    m.waiting.remove(conn)
                } else {
                    None
                }
            };
            let Some(mut guest) = guest else {
                let _ = socket.send(close_with(4404)).await;
                return;
            };
            let open = Message::text(r#"{"type":"open"}"#);
            let _ = guest.send(open.clone()).await;
            let _ = socket.send(open).await;
            forward(guest, socket).await;
        }
    }

    /// Forward binary frames both ways until either side ends, then close
    /// both.
    async fn forward(a: Server, b: Server) {
        let (mut a_out, mut a_in) = a.split();
        let (mut b_out, mut b_in) = b.split();
        let one = async {
            while let Some(Ok(m)) = a_in.next().await {
                if m.is_binary() && b_out.send(m).await.is_err() {
                    break;
                }
            }
        };
        let two = async {
            while let Some(Ok(m)) = b_in.next().await {
                if m.is_binary() && a_out.send(m).await.is_err() {
                    break;
                }
            }
        };
        tokio::select! {
            _ = one => {}
            _ = two => {}
        }
        let _ = a_out.close().await;
        let _ = b_out.close().await;
    }

    fn wall() -> Value {
        json!({ "command": {
            "type": "add_wall",
            "start": { "x": 0.0, "y": 0.0 }, "end": { "x": 4000.0, "y": 0.0 },
            "thickness_mm": null, "height_mm": null, "material_id": null, "level_id": null
        }})
    }

    async fn mode_is(app: &AppService, mode: LiveMode) -> LiveStatus {
        let deadline = Instant::now() + WAIT;
        loop {
            let status = app.live_status().await;
            if status.mode == mode {
                return status;
            }
            assert!(Instant::now() < deadline, "never {mode:?}: {status:?}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn a_session_runs_through_the_relay_alone() {
        let (url, mini, relay) = mini_relay().await;
        let host_dir = tempfile::tempdir().unwrap();
        let settings = json!({ "profile_name": "Ana", "live_relay": url, "live_direct": false });
        let host = computer(host_dir.path(), settings, true).await;
        let started = host_status(&host).await;
        relay_is(&host, LiveRelay::Ready).await;

        // A guest with only the relay to go by joins through it.
        let guest_dir = tempfile::tempdir().unwrap();
        let guest = computer(guest_dir.path(), json!({ "profile_name": "Ben" }), false).await;
        let copy = guest.handle("live_join", json!({ "invite": started.invite })).await.unwrap();
        let copy: DocState = serde_json::from_value(copy).unwrap();
        assert_eq!(Some(copy.project.id), started.project_id);
        assert_eq!(mini.lock().unwrap().joins, 1);
        let joined = guest.live_status().await;
        assert_eq!((joined.mode, joined.relay), (LiveMode::Joined, LiveRelay::Off));

        // An edit goes to the host and its new plan comes back.
        let applied: ApplyResult = serde_json::from_value(guest.handle("doc_apply", wall()).await.unwrap()).unwrap();
        let shared: DocState = serde_json::from_value(host.handle("doc_state", json!({})).await.unwrap()).unwrap();
        assert_eq!(applied.state.revision, shared.revision);
        assert_eq!(applied.state.project.elements.len(), shared.project.elements.len());

        // The host ends the session: the guest hears it through the relay.
        host.handle("live_leave", json!({})).await.unwrap();
        let off = mode_is(&guest, LiveMode::Off).await;
        assert_eq!(off.notice.as_deref(), Some("Ana ended the live session."));

        // The room is gone with the session.
        let deadline = Instant::now() + WAIT;
        while mini.lock().unwrap().control.is_some() {
            assert!(Instant::now() < deadline, "the host's control connection should close");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let err = guest.handle("live_join", json!({ "invite": started.invite })).await.unwrap_err();
        assert_eq!(err.code, "live_unreachable");
        assert_eq!(
            err.message,
            "Could not reach the host through the relay. The host is not online, so the session may have ended."
        );
        relay.abort();
    }

    #[tokio::test]
    async fn a_guest_that_can_reach_the_host_directly_does_not_use_the_relay() {
        let (url, mini, relay) = mini_relay().await;
        let host_dir = tempfile::tempdir().unwrap();
        let host = computer(host_dir.path(), json!({ "profile_name": "Ana", "live_relay": url }), true).await;
        let started = host_status(&host).await;
        relay_is(&host, LiveRelay::Ready).await;
        let invite = Invite::decode(started.invite.as_deref().unwrap()).unwrap();
        assert_eq!((invite.addrs.len(), invite.relay.is_some()), (1, true));

        let guest_dir = tempfile::tempdir().unwrap();
        let guest = computer(guest_dir.path(), json!({ "profile_name": "Ben" }), false).await;
        guest.handle("live_join", json!({ "invite": started.invite })).await.unwrap();
        // Loopback answers long before the direct addresses' head start is
        // over, and the relay is never asked, then or later.
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert_eq!(mini.lock().unwrap().joins, 0);
        relay.abort();
    }
}
