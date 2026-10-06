//! The relay against real WebSocket clients, one server per test on
//! 127.0.0.1, with short timeouts where a test waits for one. Every rule of
//! docs/RELAY.md, "Protocol, version 1", has a test here.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use futures_util::{SinkExt, Stream, StreamExt};
use guhit_relay::Config;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout, timeout_at, Instant};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// How long any single read may take before a test fails.
const WAIT: Duration = Duration::from_secs(5);

const MAX_FRAME: usize = 256 * 1024;

/// Settings for tests: pings often, closes quickly. The waits a test does
/// not look at stay long, so a slow machine does not trip them.
fn config() -> Config {
    Config { ping_interval: Duration::from_millis(200), close_timeout: Duration::from_millis(500), ..Config::default() }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

async fn start(config: Config) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("address");
    tokio::spawn(guhit_relay::serve(listener, config));
    addr
}

static SEED: AtomicU8 = AtomicU8::new(1);

/// A new room id and key, as a host makes them.
fn ids() -> (String, String) {
    let n = SEED.fetch_add(1, Ordering::Relaxed);
    (URL_SAFE_NO_PAD.encode([n; 16]), URL_SAFE_NO_PAD.encode([n; 32]))
}

fn join_path(room: &str) -> String {
    format!("/v1/join/{room}")
}

fn host_hello(room: &str, key: &str) -> Message {
    Message::text(json!({ "v": 1, "room": room, "key": key }).to_string())
}

fn accept_hello(room: &str, key: &str, conn: &str) -> Message {
    Message::text(json!({ "v": 1, "room": room, "key": key, "conn": conn }).to_string())
}

async fn connect(addr: SocketAddr, path: &str) -> Ws {
    connect_as(addr, path, None).await
}

/// Connects with an extra header, such as the client address a proxy adds.
async fn connect_as(addr: SocketAddr, path: &str, header: Option<(&'static str, &str)>) -> Ws {
    let mut request = format!("ws://{addr}{path}").into_client_request().expect("request");
    if let Some((name, value)) = header {
        request.headers_mut().insert(name, value.parse().expect("header value"));
    }
    let (ws, _) = tokio_tungstenite::connect_async(request).await.expect("connect");
    ws
}

fn describe(message: &Message) -> String {
    match message {
        Message::Text(text) => format!("text {text}"),
        Message::Binary(data) => format!("{} binary bytes", data.len()),
        other => format!("{other:?}"),
    }
}

/// The next message that is not a ping or a pong. One deadline for the
/// whole wait, so the relay's pings cannot stretch it.
async fn next<S>(ws: &mut S) -> Message
where
    S: Stream<Item = Result<Message, tungstenite::Error>> + Unpin,
{
    let deadline = Instant::now() + WAIT;
    loop {
        match timeout_at(deadline, ws.next()).await.expect("nothing from the relay in time") {
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
            Some(Ok(message)) => return message,
            Some(Err(e)) => panic!("read failed: {e}"),
            None => panic!("the connection ended without a close frame"),
        }
    }
}

async fn text(ws: &mut Ws) -> Value {
    match next(ws).await {
        Message::Text(text) => serde_json::from_str(&text).expect("JSON"),
        other => panic!("expected text, got {}", describe(&other)),
    }
}

async fn binary(ws: &mut Ws) -> Vec<u8> {
    match next(ws).await {
        Message::Binary(data) => data.to_vec(),
        other => panic!("expected binary, got {}", describe(&other)),
    }
}

/// The code of the relay's close frame, which must come next.
async fn closed<S>(ws: &mut S) -> u16
where
    S: Stream<Item = Result<Message, tungstenite::Error>> + Unpin,
{
    match next(ws).await {
        Message::Close(Some(frame)) => frame.code.into(),
        other => panic!("expected a close frame, got {}", describe(&other)),
    }
}

/// Waits for a ping; anything else first fails.
async fn pinged(ws: &mut Ws) {
    match timeout(WAIT, ws.next()).await.expect("no ping in time") {
        Some(Ok(Message::Ping(_))) => {}
        other => panic!("expected a ping, got {other:?}"),
    }
}

/// Reads until `deadline` and fails on anything but pings and pongs. The
/// reading answers the relay's pings.
async fn quiet_until(ws: &mut Ws, deadline: Instant) {
    loop {
        match timeout_at(deadline, ws.next()).await {
            Err(_) => return,
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {}
            Ok(Some(Ok(other))) => panic!("expected nothing, got {}", describe(&other)),
            Ok(other) => panic!("expected nothing, got {other:?}"),
        }
    }
}

async fn send(ws: &mut Ws, message: Message) {
    ws.send(message).await.expect("send");
}

/// A host's control connection, registered and ready.
async fn register(addr: SocketAddr, room: &str, key: &str) -> Ws {
    register_as(addr, room, key, None).await
}

async fn register_as(addr: SocketAddr, room: &str, key: &str, header: Option<(&'static str, &str)>) -> Ws {
    let mut control = connect_as(addr, "/v1/host", header).await;
    send(&mut control, host_hello(room, key)).await;
    assert_eq!(text(&mut control).await, json!({ "type": "ready" }));
    control
}

/// The `conn` the relay announces next on a control connection.
async fn announced(control: &mut Ws) -> String {
    let message = text(control).await;
    assert_eq!(message["type"], "guest", "{message}");
    let conn = message["conn"].as_str().expect("conn is a string").to_string();
    assert!(!conn.is_empty() && conn.bytes().all(|b| b.is_ascii_digit()), "{conn}");
    conn
}

/// A guest waiting in `room`, and its `conn`.
async fn waiting_guest(addr: SocketAddr, control: &mut Ws, room: &str) -> (Ws, String) {
    let guest = connect(addr, &join_path(room)).await;
    let conn = announced(control).await;
    (guest, conn)
}

/// Accepts `conn`, and checks that both sides get `open`.
async fn accept(addr: SocketAddr, guest: &mut Ws, room: &str, key: &str, conn: &str) -> Ws {
    let mut host = connect(addr, "/v1/accept").await;
    send(&mut host, accept_hello(room, key, conn)).await;
    assert_eq!(text(&mut host).await, json!({ "type": "open" }));
    assert_eq!(text(guest).await, json!({ "type": "open" }));
    host
}

/// An open pair in `room`: the guest's side and the host's side.
async fn pair(addr: SocketAddr, control: &mut Ws, room: &str, key: &str) -> (Ws, Ws) {
    let (mut guest, conn) = waiting_guest(addr, control, room).await;
    let host = accept(addr, &mut guest, room, key, &conn).await;
    (guest, host)
}

/// The close code an accept connection gets for `hello`.
async fn accept_refused(addr: SocketAddr, hello: Message) -> u16 {
    let mut host = connect(addr, "/v1/accept").await;
    send(&mut host, hello).await;
    closed(&mut host).await
}

/// Sends `frames` from one side and checks the other gets them unchanged
/// and in order. Both at once, so no buffer has to hold them all.
async fn pass(from: &mut Ws, to: &mut Ws, frames: &[Vec<u8>]) {
    let sending = async {
        for frame in frames {
            send(from, Message::binary(frame.clone())).await;
        }
    };
    let receiving = async {
        for frame in frames {
            assert_eq!(&binary(to).await, frame);
        }
    };
    tokio::join!(sending, receiving);
}

#[tokio::test]
async fn health_answers_ok() {
    let addr = start(config()).await;
    let mut tcp = TcpStream::connect(addr).await.unwrap();
    tcp.write_all(b"GET /health HTTP/1.1\r\nHost: relay\r\nConnection: close\r\n\r\n").await.unwrap();
    let mut response = String::new();
    tcp.read_to_string(&mut response).await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
    assert!(response.ends_with("\r\n\r\nok"), "{response}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_registers_and_gets_ready() {
    let addr = start(config()).await;
    let (room, key) = ids();
    let mut control = register(addr, &room, &key).await;
    // Nothing else until a guest comes.
    quiet_until(&mut control, Instant::now() + ms(300)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pair_forwards_binary_both_ways_unchanged_and_in_order() {
    let addr = start(config()).await;
    let (room, key) = ids();
    let mut control = register(addr, &room, &key).await;
    let (mut guest, mut host) = pair(addr, &mut control, &room, &key).await;
    let frames: Vec<Vec<u8>> =
        (0..40u32).map(|i| (0..(i as usize * 7919) % 70_000).map(|b| (b as u32 ^ i) as u8).collect()).collect();
    // The guest speaks first, as its TLS hello does in the app.
    pass(&mut guest, &mut host, &frames).await;
    let backwards: Vec<Vec<u8>> = frames.iter().rev().cloned().collect();
    pass(&mut host, &mut guest, &backwards).await;
    // Empty frames and the largest allowed one pass too.
    pass(&mut guest, &mut host, &[vec![], vec![0xab; MAX_FRAME], vec![1]]).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn joining_an_unknown_or_malformed_room_is_4404() {
    let addr = start(config()).await;
    let (room, _) = ids();
    let paths = [
        join_path(&room),
        join_path("short"),
        join_path(&format!("{room}A")),
        join_path(&"!".repeat(22)),
        // Right length, but the last character has bits a 16 byte id cannot.
        join_path("AAAAAAAAAAAAAAAAAAAAAB"),
    ];
    for path in paths {
        let mut guest = connect(addr, &path).await;
        assert_eq!(closed(&mut guest).await, 4404, "{path}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_same_key_takes_over_and_waiting_guests_are_announced_again() {
    let addr = start(config()).await;
    let (room, key) = ids();
    let mut old = register(addr, &room, &key).await;
    let (mut guest, conn) = waiting_guest(addr, &mut old, &room).await;

    let mut new = register(addr, &room, &key).await;
    assert_eq!(announced(&mut new).await, conn);
    assert_eq!(closed(&mut old).await, 4001);

    // The guest is not lost: the new control connection's host accepts it.
    let mut host = accept(addr, &mut guest, &room, &key, &conn).await;
    pass(&mut guest, &mut host, &[vec![1, 2, 3]]).await;
    // And new guests reach the new control connection.
    let (_second, _) = waiting_guest(addr, &mut new, &room).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_key_for_a_taken_room_is_4409() {
    let addr = start(config()).await;
    let (room, key) = ids();
    let (_, other_key) = ids();
    let mut control = register(addr, &room, &key).await;
    let mut other = connect(addr, "/v1/host").await;
    send(&mut other, host_hello(&room, &other_key)).await;
    assert_eq!(closed(&mut other).await, 4409);
    // The room stays with the first host.
    let (_guest, _) = waiting_guest(addr, &mut control, &room).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_bad_or_missing_host_hello_is_4400() {
    let hello_timeout = ms(400);
    let addr = start(Config { hello_timeout, ..config() }).await;
    let (room, key) = ids();
    let bad = [
        Message::text("not json"),
        Message::text("[1, 2]"),
        Message::text(json!({ "v": 2, "room": room, "key": key }).to_string()),
        Message::text(json!({ "v": "1", "room": room, "key": key }).to_string()),
        Message::text(json!({ "room": room, "key": key }).to_string()),
        Message::text(json!({ "v": 1, "room": "short", "key": key }).to_string()),
        Message::text(json!({ "v": 1, "room": room, "key": room }).to_string()),
        Message::text(json!({ "v": 1, "room": room }).to_string()),
        Message::binary(json!({ "v": 1, "room": room, "key": key }).to_string().into_bytes()),
    ];
    for message in bad {
        let shown = describe(&message);
        let mut control = connect(addr, "/v1/host").await;
        send(&mut control, message).await;
        assert_eq!(closed(&mut control).await, 4400, "{shown}");
    }

    // Silent: it answers the pings, which do not count as the hello.
    let started = Instant::now();
    let mut control = connect(addr, "/v1/host").await;
    assert_eq!(closed(&mut control).await, 4400);
    assert!(started.elapsed() >= hello_timeout - ms(50), "{:?}", started.elapsed());

    // After `ready` the host has nothing more to say on its control connection.
    let mut control = register(addr, &room, &key).await;
    send(&mut control, Message::text("{}")).await;
    assert_eq!(closed(&mut control).await, 4400);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accept_answers_4403_4404_and_4400() {
    let hello_timeout = ms(400);
    let addr = start(Config { hello_timeout, ..config() }).await;
    let (room, key) = ids();
    let (other_room, other_key) = ids();
    let mut control = register(addr, &room, &key).await;
    let (mut guest, conn) = waiting_guest(addr, &mut control, &room).await;

    assert_eq!(accept_refused(addr, accept_hello(&room, &other_key, &conn)).await, 4403);
    assert_eq!(accept_refused(addr, accept_hello(&other_room, &key, &conn)).await, 4404);
    assert_eq!(accept_refused(addr, accept_hello(&room, &key, "999999")).await, 4404);
    assert_eq!(accept_refused(addr, Message::text("nonsense")).await, 4400);
    let number = json!({ "v": 1, "room": room, "key": key, "conn": 1 }).to_string();
    assert_eq!(accept_refused(addr, Message::text(number)).await, 4400);
    let v2 = json!({ "v": 2, "room": room, "key": key, "conn": conn }).to_string();
    assert_eq!(accept_refused(addr, Message::text(v2)).await, 4400);
    assert_eq!(accept_refused(addr, accept_hello(&room, &key, "-1")).await, 4400);
    assert_eq!(accept_refused(addr, accept_hello(&room, "short", &conn)).await, 4400);
    let started = Instant::now();
    let mut silent = connect(addr, "/v1/accept").await;
    assert_eq!(closed(&mut silent).await, 4400);
    assert!(started.elapsed() >= hello_timeout - ms(50), "{:?}", started.elapsed());

    // None of that touched the waiting guest: the right accept opens the pair.
    let mut host = accept(addr, &mut guest, &room, &key, &conn).await;
    pass(&mut guest, &mut host, &[vec![7; 10]]).await;
    // It is paired now, so no longer waiting.
    assert_eq!(accept_refused(addr, accept_hello(&room, &key, &conn)).await, 4404);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_guest_the_host_does_not_accept_in_time_is_4408() {
    let accept_timeout = ms(400);
    let addr = start(Config { accept_timeout, ..config() }).await;
    let (room, key) = ids();
    let mut control = register(addr, &room, &key).await;
    let started = Instant::now();
    let (mut guest, conn) = waiting_guest(addr, &mut control, &room).await;
    assert_eq!(closed(&mut guest).await, 4408);
    assert!(started.elapsed() >= accept_timeout - ms(50), "{:?}", started.elapsed());
    // Too late: the guest is gone.
    assert_eq!(accept_refused(addr, accept_hello(&room, &key, &conn)).await, 4404);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_guest_that_sends_before_open_is_4400() {
    let addr = start(config()).await;
    let (room, key) = ids();
    let mut control = register(addr, &room, &key).await;

    let (mut guest, conn) = waiting_guest(addr, &mut control, &room).await;
    send(&mut guest, Message::binary(vec![1, 2, 3])).await;
    assert_eq!(closed(&mut guest).await, 4400);
    assert_eq!(accept_refused(addr, accept_hello(&room, &key, &conn)).await, 4404);

    let (mut guest, _) = waiting_guest(addr, &mut control, &room).await;
    send(&mut guest, Message::text("hello")).await;
    assert_eq!(closed(&mut guest).await, 4400);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_text_frame_in_an_open_pair_closes_both_with_4400() {
    let addr = start(config()).await;
    let (room, key) = ids();
    let mut control = register(addr, &room, &key).await;

    let (mut guest, mut host) = pair(addr, &mut control, &room, &key).await;
    send(&mut host, Message::text("oops")).await;
    assert_eq!(closed(&mut host).await, 4400);
    assert_eq!(closed(&mut guest).await, 4400);

    let (mut guest, mut host) = pair(addr, &mut control, &room, &key).await;
    send(&mut guest, Message::text("oops")).await;
    assert_eq!(closed(&mut guest).await, 4400);
    assert_eq!(closed(&mut host).await, 4400);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn when_one_side_closes_or_fails_the_other_gets_1000() {
    let addr = start(config()).await;
    let (room, key) = ids();
    let mut control = register(addr, &room, &key).await;

    // A close frame, after a last frame that still arrives first.
    let (mut guest, mut host) = pair(addr, &mut control, &room, &key).await;
    send(&mut guest, Message::binary(vec![9; 1000])).await;
    guest.close(None).await.expect("close");
    assert_eq!(binary(&mut host).await, vec![9; 1000]);
    assert_eq!(closed(&mut host).await, 1000);

    // The other way round.
    let (mut guest, mut host) = pair(addr, &mut control, &room, &key).await;
    host.close(None).await.expect("close");
    assert_eq!(closed(&mut guest).await, 1000);

    // No close frame: the connection just drops.
    let (mut guest, host) = pair(addr, &mut control, &room, &key).await;
    drop(host);
    assert_eq!(closed(&mut guest).await, 1000);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn when_the_control_connection_ends_waiting_guests_get_4404_and_pairs_go_on() {
    let addr = start(config()).await;
    let (room, key) = ids();
    let mut control = register(addr, &room, &key).await;
    let (mut guest, mut host) = pair(addr, &mut control, &room, &key).await;
    let (mut waiting, _) = waiting_guest(addr, &mut control, &room).await;

    control.close(None).await.expect("close");
    assert_eq!(closed(&mut waiting).await, 4404);

    // The open pair goes on, both ways.
    pass(&mut guest, &mut host, &[vec![1; 100], vec![2; 5000]]).await;
    pass(&mut host, &mut guest, &[vec![3; 100]]).await;
    // The room itself is gone.
    let mut late = connect(addr, &join_path(&room)).await;
    assert_eq!(closed(&mut late).await, 4404);
    // The host's reconnect registers it again.
    let _control = register(addr, &room, &key).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_relay_pings_every_connection() {
    let addr = start(Config { ping_interval: ms(100), ..config() }).await;
    let (room, key) = ids();
    let mut control = register(addr, &room, &key).await;
    pinged(&mut control).await;

    let (mut guest, conn) = waiting_guest(addr, &mut control, &room).await;
    pinged(&mut guest).await;

    // An accept connection is pinged before its hello too.
    let mut host = connect(addr, "/v1/accept").await;
    pinged(&mut host).await;
    send(&mut host, accept_hello(&room, &key, &conn)).await;
    assert_eq!(text(&mut host).await, json!({ "type": "open" }));
    assert_eq!(text(&mut guest).await, json!({ "type": "open" }));
    pinged(&mut host).await;
    pinged(&mut guest).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_that_stops_reading_gets_4000_and_one_that_reads_stays() {
    let idle_timeout = ms(500);
    // A long close_timeout keeps the relay's side open until the silent
    // client reads again.
    let config = Config { ping_interval: ms(100), idle_timeout, close_timeout: ms(5000), ..config() };
    let addr = start(config).await;
    let (room, key) = ids();
    let (other_room, other_key) = ids();
    let mut silent = register(addr, &room, &key).await;
    let mut reading = register(addr, &other_room, &other_key).await;

    // Three idle times go by. Reading answers the pings, so this one stays.
    quiet_until(&mut reading, Instant::now() + idle_timeout * 3).await;
    // The one that read nothing, and so answered no ping, was closed.
    assert_eq!(closed(&mut silent).await, 4000);
    // The idle room went with its control connection.
    let mut late = connect(addr, &join_path(&room)).await;
    assert_eq!(closed(&mut late).await, 4404);
    let (_guest, _) = waiting_guest(addr, &mut reading, &other_room).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_frame_over_the_maximum_is_1009() {
    let addr = start(Config { close_timeout: ms(5000), ..config() }).await;
    let (room, key) = ids();
    let mut control = register(addr, &room, &key).await;

    // One byte over, and much more than the socket buffers hold. The guest
    // reads while its send is still going, as the app does.
    for size in [MAX_FRAME + 1, 8 * MAX_FRAME] {
        let (guest, mut host) = pair(addr, &mut control, &room, &key).await;
        let (mut sink, mut stream) = guest.split();
        let sending = tokio::spawn(async move {
            let _ = sink.send(Message::binary(vec![5; size])).await;
            sink
        });
        assert_eq!(closed(&mut stream).await, 1009, "{size} bytes");
        assert_eq!(closed(&mut host).await, 1000, "{size} bytes");
        sending.abort();
    }

    // Before a pair opens as well.
    let (mut sink, mut stream) = connect(addr, "/v1/host").await.split();
    let sending = tokio::spawn(async move {
        let _ = sink.send(Message::text("x".repeat(MAX_FRAME + 1))).await;
        sink
    });
    assert_eq!(closed(&mut stream).await, 1009);
    sending.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_rate_limit_slows_a_pair_without_closing_it() {
    let rate = 100_000;
    let burst = 20_000;
    let addr = start(Config { rate, burst, ..config() }).await;
    let (room, key) = ids();
    let mut control = register(addr, &room, &key).await;
    let (mut guest, mut host) = pair(addr, &mut control, &room, &key).await;

    // The burst goes at once, and a frame larger than the burst still
    // passes: 80 000 bytes past the burst at 100 000 a second is 0.8 s.
    let frames = [vec![1; 20_000], vec![2; 60_000], vec![3; 20_000]];
    let started = Instant::now();
    pass(&mut guest, &mut host, &frames).await;
    let took = started.elapsed();
    assert!(took >= ms(700), "{took:?}");

    // Slowed, not closed: both ways still work.
    pass(&mut host, &mut guest, &[vec![4; 10]]).await;
    pass(&mut guest, &mut host, &[vec![5; 10]]).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_reader_slows_the_sender_down() {
    // No rate limit: only the other side's reading paces the transfer.
    let addr = start(Config { rate: 0, ..config() }).await;
    let (room, key) = ids();
    let mut control = register(addr, &room, &key).await;
    let (mut guest, mut host) = pair(addr, &mut control, &room, &key).await;

    // 48 MiB, more than every buffer on the way holds together.
    const FRAME: usize = 64 * 1024;
    const FRAMES: usize = 768;
    let frame = axum::body::Bytes::from(vec![7u8; FRAME]);
    let sending = tokio::spawn(async move {
        for _ in 0..FRAMES {
            send(&mut guest, Message::Binary(frame.clone())).await;
        }
        guest
    });
    // The host reads nothing for a while, so the sender has to wait.
    tokio::time::sleep(ms(500)).await;
    assert!(!sending.is_finished(), "the sender never had to wait");
    // Reading again lets it all through, complete.
    for _ in 0..FRAMES {
        assert_eq!(binary(&mut host).await.len(), FRAME);
    }
    let _guest = timeout(WAIT, sending).await.expect("the sender finishes").expect("sender");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_max_connections_is_4429() {
    let addr = start(Config { max_connections: 2, ..config() }).await;
    let (a, a_key) = ids();
    let (b, b_key) = ids();
    let first = register(addr, &a, &a_key).await;
    let _second = register(addr, &b, &b_key).await;
    let mut third = connect(addr, "/v1/host").await;
    assert_eq!(closed(&mut third).await, 4429);

    // A place frees up when a connection ends.
    drop(first);
    let (c, c_key) = ids();
    eventually(|| async { try_register(addr, &c, &c_key, None).await }).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_max_per_ip_is_4429_counted_by_the_address_header() {
    let header = "x-forwarded-for";
    let config = Config { max_per_ip: 2, client_ip_header: Some(header.into()), ..config() };
    let addr = start(config).await;
    let (a, a_key) = ids();
    let (b, b_key) = ids();
    let (c, c_key) = ids();
    let _one = register_as(addr, &a, &a_key, Some((header, "10.0.0.1"))).await;
    // The first entry of the list is the client.
    let _two = register_as(addr, &b, &b_key, Some((header, "10.0.0.1, 10.9.9.9"))).await;
    let mut three = connect_as(addr, "/v1/host", Some((header, "10.0.0.1"))).await;
    assert_eq!(closed(&mut three).await, 4429);
    // Another address has its own count.
    let _other = register_as(addr, &c, &c_key, Some((header, "10.0.0.2"))).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_usable_address_header_the_socket_address_counts() {
    let header = "fly-client-ip";
    let config = Config { max_per_ip: 1, client_ip_header: Some(header.into()), ..config() };
    let addr = start(config).await;
    let (a, a_key) = ids();
    let (b, b_key) = ids();
    // No header: 127.0.0.1.
    let _one = register(addr, &a, &a_key).await;
    // Not an address: 127.0.0.1 again, which is full.
    let mut two = connect_as(addr, "/v1/host", Some((header, "not an address"))).await;
    assert_eq!(closed(&mut two).await, 4429);
    // IPv6 is an address too.
    let _three = register_as(addr, &b, &b_key, Some((header, "2001:db8::1"))).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_max_rooms_is_4429() {
    let addr = start(Config { max_rooms: 1, ..config() }).await;
    let (a, a_key) = ids();
    let (b, b_key) = ids();
    let _first = register(addr, &a, &a_key).await;
    let mut second = connect(addr, "/v1/host").await;
    send(&mut second, host_hello(&b, &b_key)).await;
    assert_eq!(closed(&mut second).await, 4429);
    // A takeover is not a new room.
    let _again = register(addr, &a, &a_key).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_max_rooms_per_ip_is_4429() {
    let header = "x-forwarded-for";
    let config = Config { max_rooms_per_ip: 1, client_ip_header: Some(header.into()), ..config() };
    let addr = start(config).await;
    let (a, a_key) = ids();
    let (b, b_key) = ids();
    let (c, c_key) = ids();
    let _first = register_as(addr, &a, &a_key, Some((header, "10.0.0.1"))).await;
    let mut second = connect_as(addr, "/v1/host", Some((header, "10.0.0.1"))).await;
    send(&mut second, host_hello(&b, &b_key)).await;
    assert_eq!(closed(&mut second).await, 4429);
    let _other = register_as(addr, &c, &c_key, Some((header, "10.0.0.2"))).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_max_waiting_per_room_is_4429() {
    let addr = start(Config { max_waiting_per_room: 1, ..config() }).await;
    let (room, key) = ids();
    let mut control = register(addr, &room, &key).await;
    let (_first, _) = waiting_guest(addr, &mut control, &room).await;
    let mut second = connect(addr, &join_path(&room)).await;
    assert_eq!(closed(&mut second).await, 4429);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn over_max_pairs_per_room_is_4429() {
    let addr = start(Config { max_pairs_per_room: 1, ..config() }).await;
    let (room, key) = ids();
    let mut control = register(addr, &room, &key).await;
    let (mut guest, mut host) = pair(addr, &mut control, &room, &key).await;
    let mut second = connect(addr, &join_path(&room)).await;
    assert_eq!(closed(&mut second).await, 4429);

    // Once the pair has closed on both sides, the room has a place again.
    guest.close(None).await.expect("close");
    assert_eq!(closed(&mut host).await, 1000);
    drop(host);
    let mut third = None;
    for _ in 0..40 {
        let mut guest = connect(addr, &join_path(&room)).await;
        match timeout(ms(200), announced(&mut control)).await {
            Ok(_) => {
                third = Some(guest);
                break;
            }
            Err(_) => assert_eq!(closed(&mut guest).await, 4429),
        }
    }
    assert!(third.is_some(), "the pair's place was never freed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn router_serves_without_connect_info() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, guhit_relay::router(config())).await });
    let (room, key) = ids();
    let mut control = register(addr, &room, &key).await;
    let (_guest, _) = waiting_guest(addr, &mut control, &room).await;
}

/// Registers a room, or answers false when the relay refuses it.
async fn try_register(addr: SocketAddr, room: &str, key: &str, header: Option<(&'static str, &str)>) -> bool {
    let mut control = connect_as(addr, "/v1/host", header).await;
    send(&mut control, host_hello(room, key)).await;
    match next(&mut control).await {
        Message::Text(text) => text.as_str() == r#"{"type":"ready"}"#,
        Message::Close(Some(frame)) => {
            assert_eq!(u16::from(frame.code), 4429);
            false
        }
        other => panic!("expected ready or a close, got {}", describe(&other)),
    }
}

/// Retries `check` for up to two seconds: the relay frees a place a moment
/// after the client's side has gone.
async fn eventually<F, Fut>(check: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if check().await {
            return;
        }
        tokio::time::sleep(ms(50)).await;
    }
    panic!("the relay never freed the place");
}
