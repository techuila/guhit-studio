//! What the relay says and reads before a pair opens: identifiers, the
//! JSON messages and the close codes (docs/RELAY.md, "Protocol, version 1").

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ring::digest::{digest, SHA256};
use serde::Deserialize;

/// The protocol version this relay speaks.
const VERSION: u64 = 1;

pub(crate) const READY: &str = r#"{"type":"ready"}"#;
pub(crate) const OPEN: &str = r#"{"type":"open"}"#;

/// A room id: 16 random bytes.
pub(crate) type RoomId = [u8; 16];

/// SHA-256 of a room's key. The relay keeps this, never the key.
pub(crate) type KeyDigest = [u8; 32];

/// Close codes, as the table in docs/RELAY.md.
pub(crate) const NORMAL: u16 = 1000;
pub(crate) const TOO_BIG: u16 = 1009;
pub(crate) const IDLE: u16 = 4000;
pub(crate) const REPLACED: u16 = 4001;
pub(crate) const BAD_REQUEST: u16 = 4400;
pub(crate) const WRONG_KEY: u16 = 4403;
pub(crate) const NOT_FOUND: u16 = 4404;
pub(crate) const NOT_ACCEPTED: u16 = 4408;
pub(crate) const TAKEN: u16 = 4409;
pub(crate) const LIMIT: u16 = 4429;

/// The reason text sent with a close code. Fixed per code, so it never
/// carries anything a client sent.
pub(crate) fn reason(code: u16) -> &'static str {
    match code {
        TOO_BIG => "frame too big",
        IDLE => "nothing received",
        REPLACED => "replaced by a newer control connection",
        BAD_REQUEST => "bad request",
        WRONG_KEY => "wrong key",
        NOT_FOUND => "no such room or guest",
        NOT_ACCEPTED => "the host did not accept in time",
        TAKEN => "room taken",
        LIMIT => "over a limit",
        _ => "",
    }
}

/// `{"type":"guest","conn":"<conn>"}`, which tells a host about a guest.
pub(crate) fn guest(conn: u64) -> String {
    serde_json::json!({ "type": "guest", "conn": conn.to_string() }).to_string()
}

/// The host's first message on `/v1/host`.
pub(crate) struct HostHello {
    pub(crate) room: RoomId,
    pub(crate) key: KeyDigest,
}

/// The host's first message on `/v1/accept`.
pub(crate) struct AcceptHello {
    pub(crate) room: RoomId,
    pub(crate) key: KeyDigest,
    pub(crate) conn: u64,
}

// Unknown fields are ignored, so a later version can add some.
#[derive(Deserialize)]
struct RawHost {
    v: u64,
    room: String,
    key: String,
}

#[derive(Deserialize)]
struct RawAccept {
    v: u64,
    room: String,
    key: String,
    conn: String,
}

/// None: malformed, or another version.
pub(crate) fn host_hello(text: &str) -> Option<HostHello> {
    let raw: RawHost = serde_json::from_str(text).ok()?;
    if raw.v != VERSION {
        return None;
    }
    Some(HostHello { room: room_id(&raw.room)?, key: key_digest(&raw.key)? })
}

/// None: malformed, or another version.
pub(crate) fn accept_hello(text: &str) -> Option<AcceptHello> {
    let raw: RawAccept = serde_json::from_str(text).ok()?;
    if raw.v != VERSION {
        return None;
    }
    Some(AcceptHello { room: room_id(&raw.room)?, key: key_digest(&raw.key)?, conn: conn_id(&raw.conn)? })
}

/// A room id from its text: base64url without padding, 16 bytes.
pub(crate) fn room_id(text: &str) -> Option<RoomId> {
    decode(text)
}

fn key_digest(text: &str) -> Option<KeyDigest> {
    let key: [u8; 32] = decode(text)?;
    digest(&SHA256, &key).as_ref().try_into().ok()
}

/// A guest connection's number: a decimal string.
fn conn_id(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Exactly `N` bytes of base64url without padding. The length is checked
/// first, so a long string is never decoded.
fn decode<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != (N * 4).div_ceil(3) {
        return None;
    }
    URL_SAFE_NO_PAD.decode(text).ok()?.try_into().ok()
}

/// Equal key digests, compared in constant time.
pub(crate) fn same_key(a: &KeyDigest, b: &KeyDigest) -> bool {
    let diff = a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y));
    std::hint::black_box(diff) == 0
}
