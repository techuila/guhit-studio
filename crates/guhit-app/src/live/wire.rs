//! Live session frames (docs/CONTRACT.md, "Live sessions"): a 4-byte
//! big-endian length, then that many bytes of UTF-8 JSON. At most 64 KiB of
//! JSON before the guest is authenticated, 48 MiB after.
//!
//! JSON over 16 KiB is deflated when that makes it smaller, since the whole
//! plan goes to every guest on each change (DECISIONS D32). The high bit of
//! the length marks a deflated body, so lengths have 31 bits. The limits
//! apply to the JSON, deflated or not: a small body that inflates past the
//! limit is refused like a large one.
//!
//! Host and guest are the same code, so the messages below are internal to
//! `guhit-app` and change together with it. `PROTOCOL_VERSION` in the hello
//! keeps two different builds from talking past each other. Hellos and
//! refusals are far under 16 KiB, so they always go plain and an older build
//! still reads why it was turned away.

use std::io::{Read, Write};
use std::sync::Arc;
use std::time::Duration;

use flate2::write::DeflateEncoder;
use flate2::Compression;
use guhit_model::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, watch};

/// Version of the messages below. A hello with another version is refused.
/// 2: frames may be deflated.
pub const PROTOCOL_VERSION: u32 = 2;
/// Largest frame before the guest is authenticated: its hello.
pub const MAX_HELLO_BYTES: usize = 64 * 1024;
/// Largest frame after that.
pub const MAX_FRAME_BYTES: usize = 48 * 1024 * 1024;
/// JSON longer than this is deflated when that makes it smaller. Shorter
/// frames (presence, pings, replies) are not worth the work.
pub const DEFLATE_OVER: usize = 16 * 1024;
/// The high bit of a frame's length: the body is deflated JSON.
const DEFLATED: u32 = 1 << 31;
/// Inflating reads this much at a time.
const INFLATE_CHUNK: usize = 64 * 1024;
/// A side with nothing to send sends a ping this often, so the other side
/// can tell a quiet connection from a dead one.
pub const PING_EVERY: Duration = Duration::from_secs(5);
/// No frame for this long means the connection is dead.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(20);
/// Once a frame has started, its bytes must arrive within this.
pub const BODY_TIMEOUT: Duration = Duration::from_secs(120);
/// Presence goes out at most this often, latest wins: 20 times a second.
pub const PRESENCE_EVERY: Duration = Duration::from_millis(50);
/// Frames waiting to be written to one connection.
pub const QUEUE_FRAMES: usize = 64;
/// Underlays and reference models travel in pieces of this size, so a file
/// up to the 50 MB model limit fits the frame limit.
pub const FILE_CHUNK: usize = 2 * 1024 * 1024;
/// How long a closing connection may take to say goodbye.
pub const CLOSE_WAIT: Duration = Duration::from_secs(2);

// ------------------------------------------------------------------ messages

/// Guest to host.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToHost {
    /// The first frame of every connection.
    Hello {
        v: u32,
        secret: String,
        name: String,
        /// Asks for the participant id and color this guest had before its
        /// connection dropped.
        #[serde(default)]
        rejoin: Option<Rejoin>,
    },
    /// Answered with `ToGuest::Reply` carrying the same id.
    Request { id: u64, request: Request },
    /// The guest's pointer, selection, level and cursor chat. No reply.
    Presence { presence: Presence },
    /// Keeps a quiet connection open. No reply.
    Ping,
    /// The guest leaves the session.
    Bye,
}

/// What a guest can ask the host. Nothing else reaches the host: no
/// settings, keys, exports, paths or other projects.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Commit a command with the guest as its author. `expected_revision`
    /// makes it fail with `stale` when the plan moved on.
    Apply {
        command: Command,
        origin: Origin,
        #[serde(default)]
        expected_revision: Option<u32>,
    },
    Undo {
        #[serde(default)]
        force: bool,
    },
    Redo {
        #[serde(default)]
        force: bool,
    },
    Chat {
        text: String,
        #[serde(default)]
        at: Option<Point>,
        #[serde(default)]
        level_id: Option<Id>,
        #[serde(default)]
        via_ai: bool,
    },
    /// Up to `FILE_CHUNK` bytes of a stored file from `offset`. Answered with
    /// `FileChunk`.
    FileGet { kind: FileKind, file_name: String, offset: u64 },
    /// One piece of a file to store, in order from offset 0. The last piece
    /// is answered like `underlay_store` or `model_store`, earlier ones with
    /// `{"received": n}`.
    FilePut {
        kind: FileKind,
        file_name: String,
        offset: u64,
        total: u64,
        /// Standard base64 of the piece.
        data: String,
    },
}

/// The files a guest can read and store: the shared project's underlay
/// images and reference models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Underlay,
    Model,
}

/// Proof that a guest is the participant it asks to be again: the id and
/// the token its welcome carried.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rejoin {
    pub id: Id,
    pub token: String,
}

/// Host to guest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToGuest {
    /// The answer to a good hello.
    Welcome(Box<Welcome>),
    /// The answer to a hello the host turned down. The connection closes.
    Refused { message: String },
    Reply {
        id: u64,
        #[serde(default)]
        ok: Option<Value>,
        #[serde(default)]
        error: Option<IpcError>,
    },
    /// The host's document changed. Sent before the reply to the edit that
    /// made it, so a guest's copy is current when its call returns.
    Doc(Box<DocFrame>),
    /// Presence that changed since the last batch. None: that participant
    /// left.
    Presence { entries: Vec<PresenceUpdate> },
    /// Someone joined or left.
    Participants { participants: Vec<Participant> },
    Chat { message: ChatMessage },
    Ping,
    /// The session is over for this guest, and why.
    End { notice: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Welcome {
    pub participant_id: Id,
    /// Sent back in `Rejoin` after a dropped connection.
    pub rejoin_token: String,
    /// Everyone in the session, the host first.
    pub participants: Vec<Participant>,
    pub doc: DocFrame,
    /// The last `CHAT_HISTORY` messages of the project, oldest first.
    pub chat: Vec<ChatMessage>,
    /// Everyone else's latest presence.
    pub presence: Vec<PresenceEntry>,
}

/// The host's document as a guest copies it. `Derived` is left out: the
/// guest computes it with the same engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocFrame {
    pub project: Project,
    pub revision: u32,
    /// The host's change counter (`DocChange::seq`). A frame with a lower
    /// one than the copy is older and is ignored.
    pub seq: u64,
    pub undo: UndoMeta,
    /// Who made the change: a participant id. None in a welcome.
    #[serde(default)]
    pub by: Option<Id>,
}

/// The host's history, as `DocState` reports it. A guest's copy has no
/// history of its own, so its `DocState` carries these.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UndoMeta {
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_label: Option<String>,
    pub redo_label: Option<String>,
    pub undo_by: Option<Id>,
    pub redo_by: Option<Id>,
}

impl UndoMeta {
    pub fn of(state: &DocState) -> Self {
        Self {
            can_undo: state.can_undo,
            can_redo: state.can_redo,
            undo_label: state.undo_label.clone(),
            redo_label: state.redo_label.clone(),
            undo_by: state.undo_by.clone(),
            redo_by: state.redo_by.clone(),
        }
    }

    pub fn apply_to(&self, state: &mut DocState) {
        state.can_undo = self.can_undo;
        state.can_redo = self.can_redo;
        state.undo_label = self.undo_label.clone();
        state.redo_label = self.redo_label.clone();
        state.undo_by = self.undo_by.clone();
        state.redo_by = self.redo_by.clone();
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresenceUpdate {
    pub participant_id: Id,
    pub presence: Option<Presence>,
}

/// The answer to `Request::Apply`: the new revision and what changed. The
/// new state itself came just before, as a `Doc` frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Applied {
    pub revision: u32,
    pub diff: Diff,
}

/// The answer to `Request::Undo` and `Request::Redo`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stepped {
    pub revision: u32,
}

/// The answer to `Request::FileGet`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChunk {
    /// Standard base64 of the piece.
    pub data: String,
    /// Size of the whole file.
    pub total: u64,
}

// --------------------------------------------------------------------- codec

#[derive(Debug)]
pub enum FrameError {
    /// The frame is longer than the limit. Nothing past its length was read,
    /// or, for a deflated frame, nothing past the limit was inflated.
    TooBig(usize),
    /// The bytes are not the JSON message expected, or a deflated body does
    /// not inflate.
    NotJson(String),
    /// Nothing arrived in time.
    Timeout,
    /// The connection closed or failed.
    Io(std::io::Error),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::TooBig(n) => write!(f, "a frame of {n} bytes is over the limit"),
            FrameError::NotJson(e) => write!(f, "a frame is not a message: {e}"),
            FrameError::Timeout => write!(f, "nothing arrived in time"),
            FrameError::Io(e) => write!(f, "the connection failed: {e}"),
        }
    }
}

/// One frame: the length, then the JSON, deflated when it is over
/// `DEFLATE_OVER` and deflating makes it smaller. Fails when the JSON would be
/// longer than `max`, whether or not it deflates.
pub fn encode<T: Serialize>(message: &T, max: usize) -> Result<Vec<u8>, FrameError> {
    let mut out = vec![0u8; 4];
    serde_json::to_writer(&mut out, message).map_err(|e| FrameError::NotJson(e.to_string()))?;
    finish_frame(out, max)
}

/// A frame from a body behind 4 bytes left for the length: the length
/// written, the body deflated when that is worth it.
fn finish_frame(mut out: Vec<u8>, max: usize) -> Result<Vec<u8>, FrameError> {
    let len = out.len() - 4;
    if len > max {
        return Err(FrameError::TooBig(len));
    }
    if len > DEFLATE_OVER {
        if let Some(deflated) = deflate(&out[4..]).filter(|d| d.len() < out.len()) {
            out = deflated;
        }
    }
    let body = out.len() - 4;
    let head = u32::try_from(body).ok().filter(|n| n & DEFLATED == 0).ok_or(FrameError::TooBig(body))?;
    let head = if body < len { head | DEFLATED } else { head };
    out[..4].copy_from_slice(&head.to_be_bytes());
    Ok(out)
}

/// `json` deflated, behind 4 bytes left for the length. None if deflating
/// failed, which writing to memory does not.
fn deflate(json: &[u8]) -> Option<Vec<u8>> {
    let mut encoder = DeflateEncoder::new(vec![0u8; 4], Compression::default());
    encoder.write_all(json).ok()?;
    encoder.finish().ok()
}

/// Inflate a deflated frame body, `max` bytes at most. Past that it stops
/// with `TooBig`, and the buffer never grows past `max`: a small frame that
/// would inflate to gigabytes costs no more memory than an honest one at the
/// limit.
fn inflate(deflated: &[u8], max: usize) -> Result<Vec<u8>, FrameError> {
    let mut decoder = flate2::bufread::DeflateDecoder::new(deflated);
    let mut out = Vec::new();
    let mut chunk = vec![0u8; INFLATE_CHUNK];
    loop {
        let n = decoder
            .read(&mut chunk)
            .map_err(|e| FrameError::NotJson(format!("a deflated frame does not inflate: {e}")))?;
        if n == 0 {
            return Ok(out);
        }
        let len = out.len() + n;
        if len > max {
            return Err(FrameError::TooBig(len));
        }
        if len > out.capacity() {
            // Double as a Vec would, but never past the limit.
            let capacity = len.max(out.capacity() * 2).min(max);
            out.reserve_exact(capacity - out.len());
        }
        out.extend_from_slice(&chunk[..n]);
    }
}

/// `encode` into a buffer that can be queued for several connections.
pub fn shared<T: Serialize>(message: &T, max: usize) -> Result<Arc<[u8]>, FrameError> {
    encode(message, max).map(Arc::from)
}

/// Read one frame's JSON bytes, inflated when the frame was deflated. `idle`
/// bounds the wait for the frame to start, `body` the time its bytes may take
/// once it has. A length over `max` fails before anything is allocated for
/// it, and so does inflating past `max`.
pub async fn read_frame<R: AsyncRead + Unpin>(
    r: &mut R,
    max: usize,
    idle: Duration,
    body: Duration,
) -> Result<Vec<u8>, FrameError> {
    let mut head = [0u8; 4];
    match tokio::time::timeout(idle, r.read_exact(&mut head)).await {
        Err(_) => return Err(FrameError::Timeout),
        Ok(Err(e)) => return Err(FrameError::Io(e)),
        Ok(Ok(_)) => {}
    }
    let head = u32::from_be_bytes(head);
    let deflated = head & DEFLATED != 0;
    let len = (head & !DEFLATED) as usize;
    if len > max {
        return Err(FrameError::TooBig(len));
    }
    let mut bytes = vec![0u8; len];
    match tokio::time::timeout(body, r.read_exact(&mut bytes)).await {
        Err(_) => Err(FrameError::Timeout),
        Ok(Err(e)) => Err(FrameError::Io(e)),
        Ok(Ok(_)) if deflated => inflate(&bytes, max),
        Ok(Ok(_)) => Ok(bytes),
    }
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, FrameError> {
    serde_json::from_slice(bytes).map_err(|e| FrameError::NotJson(e.to_string()))
}

// -------------------------------------------------------------------- writer

/// What a connection's writer task takes from its queue.
pub enum Out {
    Frame(Arc<[u8]>),
    /// Write what came before, then close the connection.
    Close,
}

/// A connection's writer: frames from `rx` in order, a ping when there was
/// nothing to send for `PING_EVERY`. It stops when `kill` is set, and sets
/// `kill` itself when it stops, so the reader of the same connection stops
/// too.
pub async fn write_loop<W: AsyncWrite + Unpin>(
    mut w: W,
    mut rx: mpsc::Receiver<Out>,
    kill: watch::Sender<bool>,
    ping: Arc<[u8]>,
) {
    let mut stop = kill.subscribe();
    loop {
        let next = tokio::select! {
            _ = stop.wait_for(|k| *k) => break,
            next = tokio::time::timeout(PING_EVERY, rx.recv()) => next,
        };
        let frame = match next {
            Ok(Some(Out::Frame(frame))) => frame,
            Ok(Some(Out::Close)) | Ok(None) => {
                let _ = tokio::time::timeout(CLOSE_WAIT, w.shutdown()).await;
                break;
            }
            Err(_) => ping.clone(),
        };
        let written = tokio::select! {
            _ = stop.wait_for(|k| *k) => break,
            r = async {
                w.write_all(&frame).await?;
                w.flush().await
            } => r,
        };
        if written.is_err() {
            break;
        }
    }
    kill.send_replace(true);
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG: Duration = Duration::from_secs(5);

    #[tokio::test]
    async fn frames_round_trip() {
        let msg = ToHost::Hello { v: PROTOCOL_VERSION, secret: "s".into(), name: "Ana".into(), rejoin: None };
        let bytes = encode(&msg, MAX_HELLO_BYTES).unwrap();
        assert_eq!(u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize, bytes.len() - 4);
        let mut r = &bytes[..];
        let body = read_frame(&mut r, MAX_HELLO_BYTES, LONG, LONG).await.unwrap();
        match decode::<ToHost>(&body).unwrap() {
            ToHost::Hello { v, name, rejoin, .. } => {
                assert_eq!((v, name.as_str(), rejoin), (PROTOCOL_VERSION, "Ana", None));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_frame_over_the_limit_is_refused_before_its_body_is_read() {
        // The length claims 1 MiB; only the 4 length bytes exist.
        let head = (1024u32 * 1024).to_be_bytes();
        let mut r = &head[..];
        let err = read_frame(&mut r, MAX_HELLO_BYTES, LONG, LONG).await.unwrap_err();
        assert!(matches!(err, FrameError::TooBig(n) if n == 1024 * 1024), "{err}");

        // Just over and just at the limit. The limit is on the JSON, so this
        // one fails although it would deflate to almost nothing.
        let big = "x".repeat(MAX_HELLO_BYTES);
        assert!(matches!(encode(&big, MAX_HELLO_BYTES), Err(FrameError::TooBig(n)) if n == MAX_HELLO_BYTES + 2));
        let fits = "x".repeat(MAX_HELLO_BYTES - 2);
        let frame = encode(&fits, MAX_HELLO_BYTES).unwrap();
        let mut r = &frame[..];
        let body = read_frame(&mut r, MAX_HELLO_BYTES, LONG, LONG).await.unwrap();
        assert_eq!(body.len(), MAX_HELLO_BYTES);
        assert_eq!(decode::<String>(&body).unwrap(), fits);
    }

    fn head_of(frame: &[u8]) -> u32 {
        u32::from_be_bytes(frame[..4].try_into().unwrap())
    }

    #[tokio::test]
    async fn large_frames_are_deflated_and_small_ones_are_not() {
        // Plan-sized and repetitive, as real plans are.
        let text = "Add wall from 0,0 to 4000,0. ".repeat(4000);
        let msg = ToGuest::Refused { message: text.clone() };
        let json = serde_json::to_vec(&msg).unwrap();
        assert!(json.len() > DEFLATE_OVER);
        let frame = encode(&msg, MAX_FRAME_BYTES).unwrap();
        let head = head_of(&frame);
        assert_ne!(head & DEFLATED, 0, "deflated on the wire");
        assert_eq!((head & !DEFLATED) as usize, frame.len() - 4);
        assert!(frame.len() * 10 < json.len(), "{} bytes for {} of JSON", frame.len(), json.len());
        let mut r = &frame[..];
        let body = read_frame(&mut r, MAX_FRAME_BYTES, LONG, LONG).await.unwrap();
        assert_eq!(body, json);
        assert!(matches!(decode::<ToGuest>(&body).unwrap(), ToGuest::Refused { message } if message == text));

        // Small frames go as they are.
        let ping = encode(&ToGuest::Ping, MAX_HELLO_BYTES).unwrap();
        assert_eq!(head_of(&ping) as usize, ping.len() - 4);
        assert_eq!(&ping[4..], br#"{"type":"ping"}"#);
        let small = ToGuest::Refused { message: "x".repeat(DEFLATE_OVER - 40) };
        let frame = encode(&small, MAX_HELLO_BYTES).unwrap();
        assert_eq!(head_of(&frame) & DEFLATED, 0);
        assert_eq!(&frame[4..], serde_json::to_vec(&small).unwrap().as_slice());
    }

    #[tokio::test]
    async fn a_body_that_does_not_deflate_smaller_goes_as_it_is() {
        // No JSON is truly incompressible, so random bytes stand in for it.
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut body = vec![0u8; 4];
        body.extend((0..64 * 1024).map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        }));
        let frame = finish_frame(body.clone(), MAX_FRAME_BYTES).unwrap();
        assert_eq!(head_of(&frame) as usize, 64 * 1024, "plain, no deflate bit");
        assert_eq!(&frame[4..], &body[4..]);
        let mut r = &frame[..];
        assert_eq!(read_frame(&mut r, MAX_FRAME_BYTES, LONG, LONG).await.unwrap(), &body[4..]);
    }

    /// A frame of `len` zero bytes, deflated: a few hundred bytes.
    fn bomb(len: usize) -> Vec<u8> {
        let mut deflated = deflate(&vec![0u8; len]).unwrap();
        let body = (deflated.len() - 4) as u32;
        deflated[..4].copy_from_slice(&(body | DEFLATED).to_be_bytes());
        deflated
    }

    #[tokio::test]
    async fn a_deflate_bomb_stops_at_the_limit() {
        // 1 MiB of zeros deflates to about 1 KB, far under the hello limit.
        let frame = bomb(1024 * 1024);
        assert!(frame.len() < 2048, "{}", frame.len());
        let mut r = &frame[..];
        let err = read_frame(&mut r, MAX_HELLO_BYTES, LONG, LONG).await.unwrap_err();
        // It stopped within one read of the limit, not at 1 MiB.
        assert!(matches!(err, FrameError::TooBig(n) if n > MAX_HELLO_BYTES && n <= MAX_HELLO_BYTES + INFLATE_CHUNK), "{err}");

        // One byte over the frame limit, and exactly at it.
        let frame = bomb(MAX_FRAME_BYTES + 1);
        let mut r = &frame[..];
        let err = read_frame(&mut r, MAX_FRAME_BYTES, LONG, LONG).await.unwrap_err();
        assert!(matches!(err, FrameError::TooBig(n) if n == MAX_FRAME_BYTES + 1), "{err}");
        let frame = bomb(MAX_HELLO_BYTES);
        let mut r = &frame[..];
        assert_eq!(read_frame(&mut r, MAX_HELLO_BYTES, LONG, LONG).await.unwrap().len(), MAX_HELLO_BYTES);

        // A deflated body that does not inflate.
        let mut garbage = (9 | DEFLATED).to_be_bytes().to_vec();
        garbage.extend_from_slice(b"not json!");
        let mut r = &garbage[..];
        let err = read_frame(&mut r, MAX_HELLO_BYTES, LONG, LONG).await.unwrap_err();
        assert!(matches!(err, FrameError::NotJson(_)), "{err}");
    }

    #[tokio::test]
    async fn garbage_and_cut_frames_fail_cleanly() {
        let mut garbage = 7u32.to_be_bytes().to_vec();
        garbage.extend_from_slice(b"not js!");
        let mut r = &garbage[..];
        let body = read_frame(&mut r, MAX_HELLO_BYTES, LONG, LONG).await.unwrap();
        assert!(matches!(decode::<ToHost>(&body), Err(FrameError::NotJson(_))));

        // JSON, but not a message.
        assert!(matches!(decode::<ToHost>(br#"{"type":"launch"}"#), Err(FrameError::NotJson(_))));

        // A length with fewer bytes behind it.
        let mut cut = 100u32.to_be_bytes().to_vec();
        cut.extend_from_slice(b"{\"type\"");
        let mut r = &cut[..];
        assert!(matches!(read_frame(&mut r, MAX_HELLO_BYTES, LONG, LONG).await, Err(FrameError::Io(_))));

        // An empty frame is not a message either.
        let empty = 0u32.to_be_bytes();
        let mut r = &empty[..];
        let body = read_frame(&mut r, MAX_HELLO_BYTES, LONG, LONG).await.unwrap();
        assert!(matches!(decode::<ToHost>(&body), Err(FrameError::NotJson(_))));
    }

    #[tokio::test]
    async fn a_silent_peer_times_out() {
        let (mut a, _b) = tokio::io::duplex(64);
        let err = read_frame(&mut a, MAX_HELLO_BYTES, Duration::from_millis(20), LONG).await.unwrap_err();
        assert!(matches!(err, FrameError::Timeout));
    }

    #[tokio::test]
    async fn replies_and_documents_keep_their_shape() {
        let reply = ToGuest::Reply { id: 3, ok: None, error: Some(IpcError::new("stale", "moved on")) };
        let text = serde_json::to_string(&reply).unwrap();
        assert!(text.starts_with(r#"{"type":"reply","id":3"#), "{text}");
        let back: ToGuest = serde_json::from_str(&text).unwrap();
        assert!(matches!(back, ToGuest::Reply { id: 3, error: Some(e), .. } if e.code == "stale"));

        let project = defaults::new_project("Shared");
        let doc = ToGuest::Doc(Box::new(DocFrame {
            project: project.clone(),
            revision: 4,
            seq: 9,
            undo: UndoMeta { can_undo: true, undo_label: Some("Add wall".into()), ..UndoMeta::default() },
            by: Some("p1".into()),
        }));
        let frame = encode(&doc, MAX_FRAME_BYTES).unwrap();
        let mut r = &frame[..];
        let back: ToGuest = decode(&read_frame(&mut r, MAX_FRAME_BYTES, LONG, LONG).await.unwrap()).unwrap();
        match back {
            ToGuest::Doc(frame) => {
                assert_eq!(frame.project, project);
                assert_eq!((frame.revision, frame.seq), (4, 9));
                assert_eq!(frame.undo.undo_label.as_deref(), Some("Add wall"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
