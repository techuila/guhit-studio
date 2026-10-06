//! The live session relay (docs/RELAY.md, DECISIONS D32). It pairs a guest
//! with the host of a room and forwards their bytes, nothing more: the
//! session inside is TLS between them, so the relay cannot read it. It keeps
//! rooms in memory, stores nothing and has no accounts.
//!
//! Every endpoint is a WebSocket. Each connection has one task that reads it
//! and decides how it ends (`flows`), and a writer task that sends what is
//! queued for it and the pings (`conn`). The rooms and every count the
//! limits check live under one lock (`rooms`).
//!
//! Nothing a client sends may panic: release builds abort on a panic, which
//! would end every session on the relay. It logs counts and errors, never
//! contents, room ids, keys or addresses.

mod config;
mod conn;
mod flows;
mod rate;
mod rooms;
mod wire;

use std::convert::Infallible;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Weak};
use std::time::Duration;

use axum::extract::rejection::PathRejection;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{ConnectInfo, FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::response::Response;
use axum::routing::get;
use axum::serve::ListenerExt;
use axum::Router;
use tokio::net::TcpListener;

pub use config::Config;

use conn::{Conn, Ending};
use rooms::{Relay, PAIR_QUEUE};
use wire::LIMIT;

/// What tungstenite reads from the socket at a time. Its default, 128 KiB,
/// is allocated for every connection; frames larger than this still arrive
/// whole.
const READ_BUFFER: usize = 16 * 1024;

/// How often the counts are logged, when they changed.
const COUNTS_EVERY: Duration = Duration::from_secs(60);

/// The relay's routes: `GET /health`, `/v1/host`, `/v1/join/<room>` and
/// `/v1/accept`. Serve it with `into_make_service_with_connect_info::<SocketAddr>()`,
/// as `serve` does, or every client shares one address in the limits.
pub fn router(config: Config) -> Router {
    build(Arc::new(Relay::new(config)))
}

/// Serves the relay on `listener` until the listener fails.
pub async fn serve(listener: TcpListener, config: Config) -> io::Result<()> {
    let relay = Arc::new(Relay::new(config));
    tokio::spawn(log_counts(Arc::downgrade(&relay)));
    // Small frames such as pings and presence should not wait for more.
    let listener = listener.tap_io(|tcp| {
        let _ = tcp.set_nodelay(true);
    });
    let app = build(relay).into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, app).await
}

fn build(relay: Arc<Relay>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/host", get(host))
        .route("/v1/join/{room}", get(join))
        .route("/v1/accept", get(accept))
        .with_state(relay)
}

async fn health() -> &'static str {
    "ok"
}

async fn host(State(relay): State<Arc<Relay>>, ClientIp(ip): ClientIp, ws: WebSocketUpgrade) -> Response {
    let queue = relay.control_queue;
    upgrade(relay, ws, ip, queue, move |relay, conn| flows::host(relay, conn, ip))
}

async fn join(
    State(relay): State<Arc<Relay>>,
    ClientIp(ip): ClientIp,
    room: Result<Path<String>, PathRejection>,
    ws: WebSocketUpgrade,
) -> Response {
    // A malformed id is answered like an unknown room, after the upgrade.
    let room = room.ok().and_then(|Path(room)| wire::room_id(&room));
    upgrade(relay, ws, ip, PAIR_QUEUE, move |relay, conn| flows::guest(relay, conn, room))
}

async fn accept(State(relay): State<Arc<Relay>>, ClientIp(ip): ClientIp, ws: WebSocketUpgrade) -> Response {
    upgrade(relay, ws, ip, PAIR_QUEUE, flows::accept)
}

/// Completes the WebSocket upgrade, then runs `flow` on the connection. The
/// connection limits are checked after the upgrade, so a client over one
/// still gets a close code (4429) and not an HTTP error.
fn upgrade<F, Fut>(relay: Arc<Relay>, ws: WebSocketUpgrade, ip: IpAddr, queue: usize, flow: F) -> Response
where
    F: FnOnce(Arc<Relay>, Conn) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let max = relay.config.max_frame;
    ws.max_frame_size(max).max_message_size(max).read_buffer_size(READ_BUFFER).on_upgrade(move |socket| async move {
        let admission = relay.admit(ip);
        let admitted = admission.is_some();
        let conn = Conn::new(socket, &relay.config, queue, admission);
        if admitted {
            flow(relay, conn).await;
        } else {
            conn.close(Ending::Close(LIMIT)).await;
        }
    })
}

/// The client's address: from `RELAY_CLIENT_IP_HEADER` when it is set and
/// holds an address, otherwise the socket's.
struct ClientIp(IpAddr);

impl FromRequestParts<Arc<Relay>> for ClientIp {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, relay: &Arc<Relay>) -> Result<Self, Self::Rejection> {
        let from_header = relay
            .ip_header
            .as_ref()
            .and_then(|name| parts.headers.get(name))
            .and_then(|value| value.to_str().ok())
            .and_then(forwarded_ip);
        let from_socket = parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|info| info.0.ip());
        let ip = from_header.or(from_socket).unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        Ok(ClientIp(ip.to_canonical()))
    }
}

/// The address in a proxy header: the first entry of a list, as in
/// `x-forwarded-for`, with or without a port.
fn forwarded_ip(value: &str) -> Option<IpAddr> {
    let first = value.split(',').next()?.trim();
    first
        .parse::<IpAddr>()
        .ok()
        .or_else(|| first.parse::<SocketAddr>().ok().map(|addr| addr.ip()))
        .or_else(|| first.strip_prefix('[')?.strip_suffix(']')?.parse().ok())
}

/// Logs the counts once a minute when they changed, while the relay runs.
async fn log_counts(relay: Weak<Relay>) {
    let mut last = None;
    loop {
        tokio::time::sleep(COUNTS_EVERY).await;
        let Some(relay) = relay.upgrade() else {
            return;
        };
        let counts = relay.counts();
        if last != Some(counts) {
            println!(
                "guhit-relay: {} rooms, {} pairs, {} connections, {} refused over a limit since start",
                counts.rooms, counts.pairs, counts.connections, counts.refused
            );
            last = Some(counts);
        }
    }
}
