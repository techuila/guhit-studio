//! The invite a host sends to the people who should join (docs/CONTRACT.md,
//! "Live sessions"): `guhit-live:` then base64url, without padding, of
//! `{"v":3,"secret":...,"pin":...,"sealed":...,"project":"Bungalow","relay":{"url":"wss://relay.example.com","room":...}}`.
//!
//! `secret` is the session's 128-bit secret, `pin` the SHA-256 of the host's
//! certificate, `room` the session's 128-bit room on the relay (DECISIONS
//! D32), all base64url. `relay` is left out when the host has no relay, and
//! `sealed` when the host takes no direct connections; one of the two is
//! always there. The key that proves to the relay that a connection is the
//! host's never leaves the host, so an invite lets a guest join the room, not
//! take it. Anyone with the invite can join while the session runs.
//!
//! `sealed` holds the host's direct addresses, so neither the invite text
//! nor a guest's screen shows where the host is: base64url of a 12-byte
//! random nonce, then the ChaCha20-Poly1305 ciphertext and tag of the JSON
//! array of addresses. The key is HKDF-SHA256 over the 16 secret bytes (no
//! salt, info `SEAL_INFO`), and the 32 pin bytes are the associated data, so
//! a seal moved to another invite does not open. The guest's app opens it in
//! memory to connect (`Invite::addrs`). The key comes from the invite, so
//! this keeps the addresses out of sight, not out of reach of someone who
//! decodes the invite on purpose.
//!
//! Versions 1 (addresses only, no relay) and 2 (plain `addrs` and `relay`)
//! are still read.

use std::net::SocketAddr;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use guhit_model::IpcError;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, CHACHA20_POLY1305, NONCE_LEN};
use ring::hkdf;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

use super::relay;

pub const INVITE_PREFIX: &str = "guhit-live:";
/// Version of the invite format. 2 added `relay`, 3 sealed the addresses.
pub const INVITE_VERSION: u32 = 3;
/// Longest invite accepted, in characters. Real ones are about 340, or 440
/// with a relay; the longest a host can make (8 addresses, the longest
/// relay address and project name) is about 1800.
const MAX_INVITE_CHARS: usize = 4096;
/// Most addresses an invite may list.
pub const MAX_ADDRS: usize = 8;
const MAX_PROJECT_CHARS: usize = 120;
/// HKDF info for the key that seals the addresses.
const SEAL_INFO: &[u8] = b"guhit-live invite addrs v3";

/// An invite as the app holds it. Never serialized as is: `encode` seals the
/// addresses.
#[derive(Debug, Clone, PartialEq)]
pub struct Invite {
    /// The version the invite was written in. `encode` always writes
    /// `INVITE_VERSION`.
    pub v: u32,
    pub secret: String,
    pub pin: String,
    /// Where the host listens for direct connections. A guest tries them all
    /// at once. Empty when the host takes none. Only in memory: version 3
    /// carries them sealed.
    pub addrs: Vec<String>,
    /// The shared project's name, for display before joining.
    pub project: String,
    /// The relay the host registered the session with.
    pub relay: Option<InviteRelay>,
}

/// Where the session waits on the relay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InviteRelay {
    /// The relay's base address, as the host's settings name it.
    pub url: String,
    /// The session's room: 16 random bytes, base64url.
    pub room: String,
}

/// Version 3 as it travels.
#[derive(Serialize, Deserialize)]
struct Wire {
    v: u32,
    secret: String,
    pin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sealed: Option<String>,
    project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    relay: Option<InviteRelay>,
}

/// Versions 1 and 2, addresses in plain text.
#[derive(Deserialize)]
struct WireV2 {
    secret: String,
    pin: String,
    #[serde(default)]
    addrs: Vec<String>,
    project: String,
    #[serde(default)]
    relay: Option<InviteRelay>,
}

fn not_an_invite() -> IpcError {
    IpcError::new("bad_args", "This is not a Guhit live session invite.")
}

/// The bytes of base64url text, when it decodes to exactly `len` of them.
fn b64url_of(text: &str, len: usize) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(text.as_bytes()).ok().filter(|b| b.len() == len)
}

/// True for base64url text that decodes to exactly `len` bytes.
fn is_b64url_of(text: &str, len: usize) -> bool {
    b64url_of(text, len).is_some()
}

/// The key that seals an invite's addresses, from the session secret.
fn seal_key(secret: &[u8]) -> Option<LessSafeKey> {
    let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, &[]).extract(secret);
    let okm = prk.expand(&[SEAL_INFO], &CHACHA20_POLY1305).ok()?;
    Some(LessSafeKey::new(UnboundKey::from(okm)))
}

/// `addrs` sealed for an invite with this secret and pin (both base64url).
/// None when either does not decode or this computer gives no random
/// numbers: the invite then carries no direct addresses rather than plain
/// ones.
fn seal(secret: &str, pin: &str, addrs: &[String]) -> Option<String> {
    let secret = URL_SAFE_NO_PAD.decode(secret.as_bytes()).ok()?;
    let pin = URL_SAFE_NO_PAD.decode(pin.as_bytes()).ok()?;
    let key = seal_key(&secret)?;
    let mut nonce = [0u8; NONCE_LEN];
    SystemRandom::new().fill(&mut nonce).ok()?;
    let mut sealed = serde_json::to_vec(addrs).ok()?;
    key.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(&pin), &mut sealed).ok()?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&sealed);
    Some(URL_SAFE_NO_PAD.encode(out))
}

/// The addresses a seal holds. None when it does not open with this secret
/// and pin (raw bytes), or holds something else.
fn open(secret: &[u8], pin: &[u8], sealed: &str) -> Option<Vec<String>> {
    let mut bytes = URL_SAFE_NO_PAD.decode(sealed.as_bytes()).ok()?;
    if bytes.len() < NONCE_LEN + CHACHA20_POLY1305.tag_len() {
        return None;
    }
    let (nonce, in_out) = bytes.split_at_mut(NONCE_LEN);
    let nonce = Nonce::try_assume_unique_for_key(nonce).ok()?;
    let plain = seal_key(secret)?.open_in_place(nonce, Aad::from(pin), in_out).ok()?;
    serde_json::from_slice(plain).ok()
}

impl Invite {
    /// The invite text, in the current version. With no direct addresses,
    /// `sealed` is left out.
    pub fn encode(&self) -> String {
        let sealed = if self.addrs.is_empty() { None } else { seal(&self.secret, &self.pin, &self.addrs) };
        let wire = Wire {
            v: INVITE_VERSION,
            secret: self.secret.clone(),
            pin: self.pin.clone(),
            sealed,
            project: self.project.clone(),
            relay: self.relay.clone(),
        };
        let json = serde_json::to_vec(&wire).unwrap_or_default();
        format!("{INVITE_PREFIX}{}", URL_SAFE_NO_PAD.encode(json))
    }

    /// Read an invite as a person pasted it: spaces and line breaks around
    /// or inside it are ignored, and padding is tolerated.
    pub fn decode(text: &str) -> Result<Self, IpcError> {
        let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        if text.len() > MAX_INVITE_CHARS {
            return Err(not_an_invite());
        }
        let body = text.strip_prefix(INVITE_PREFIX).ok_or_else(not_an_invite)?;
        let json = URL_SAFE_NO_PAD
            .decode(body.trim_end_matches('=').as_bytes())
            .map_err(|_| not_an_invite())?;
        let raw: serde_json::Value = serde_json::from_slice(&json).map_err(|_| not_an_invite())?;
        let version = match raw.get("v").and_then(|v| v.as_u64()) {
            Some(v) if (1..=INVITE_VERSION as u64).contains(&v) => v as u32,
            Some(v) if v > INVITE_VERSION as u64 => {
                return Err(IpcError::new(
                    "bad_args",
                    "This invite is from a newer version of Guhit Studio. Update the app to join.",
                ))
            }
            _ => return Err(not_an_invite()),
        };
        let mut invite = if version >= 3 {
            let wire: Wire = serde_json::from_value(raw).map_err(|_| not_an_invite())?;
            // The seal opens with the secret and pin, so they are checked
            // first. A plain `addrs` is ignored: addresses come sealed.
            let addrs = match &wire.sealed {
                Some(sealed) => {
                    let secret = b64url_of(&wire.secret, 16).ok_or_else(not_an_invite)?;
                    let pin = b64url_of(&wire.pin, 32).ok_or_else(not_an_invite)?;
                    open(&secret, &pin, sealed).ok_or_else(not_an_invite)?
                }
                None => vec![],
            };
            Invite { v: version, secret: wire.secret, pin: wire.pin, addrs, project: wire.project, relay: wire.relay }
        } else {
            let wire: WireV2 = serde_json::from_value(raw).map_err(|_| not_an_invite())?;
            // Version 1 had no relay.
            let relay = if version == 1 { None } else { wire.relay };
            Invite { v: version, secret: wire.secret, pin: wire.pin, addrs: wire.addrs, project: wire.project, relay }
        };
        // The secret is 128 bits and the pin a SHA-256, both base64url.
        if !is_b64url_of(&invite.secret, 16) || !is_b64url_of(&invite.pin, 32) {
            return Err(not_an_invite());
        }
        if let Some(relay) = &invite.relay {
            if !is_b64url_of(&relay.room, 16) || relay::check_url(&relay.url).is_err() {
                return Err(not_an_invite());
            }
        }
        // Without a relay, the addresses are the only way in.
        if (invite.addrs.is_empty() && invite.relay.is_none())
            || invite.addrs.len() > MAX_ADDRS
            || invite.addrs.iter().any(|a| a.parse::<SocketAddr>().is_err())
        {
            return Err(not_an_invite());
        }
        invite.project = invite
            .project
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_PROJECT_CHARS)
            .collect::<String>()
            .trim()
            .to_string();
        Ok(invite)
    }

    /// The addresses, parsed. `decode` already checked every one.
    pub fn socket_addrs(&self) -> Vec<SocketAddr> {
        self.addrs.iter().filter_map(|a| a.parse().ok()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn sample() -> Invite {
        Invite {
            v: INVITE_VERSION,
            secret: URL_SAFE_NO_PAD.encode([7u8; 16]),
            pin: URL_SAFE_NO_PAD.encode([9u8; 32]),
            addrs: vec!["192.168.1.20:1460".into(), "127.0.0.1:1460".into()],
            relay: Some(InviteRelay {
                url: "wss://relay.example.com".into(),
                room: URL_SAFE_NO_PAD.encode([5u8; 16]),
            }),
            project: "Bungalow".into(),
        }
    }

    fn with_json(json: &str) -> String {
        format!("{INVITE_PREFIX}{}", URL_SAFE_NO_PAD.encode(json))
    }

    /// The JSON inside an invite, as text.
    fn body_of(text: &str) -> String {
        let json = URL_SAFE_NO_PAD.decode(text.strip_prefix(INVITE_PREFIX).unwrap()).unwrap();
        String::from_utf8(json).unwrap()
    }

    fn json_of(text: &str) -> Value {
        serde_json::from_str(&body_of(text)).unwrap()
    }

    /// `decode` refuses `v` as not an invite.
    fn refused(v: &Value) {
        let err = Invite::decode(&with_json(&v.to_string())).unwrap_err();
        assert_eq!((err.code.as_str(), err.message.as_str()), ("bad_args", "This is not a Guhit live session invite."), "{v}");
    }

    #[test]
    fn invites_round_trip() {
        let invite = sample();
        let text = invite.encode();
        assert!(text.starts_with("guhit-live:"));
        assert!(!text.contains('='), "no padding");
        assert_eq!(Invite::decode(&text).unwrap(), invite);
        assert_eq!(invite.socket_addrs().len(), 2);
        // The JSON inside is the documented shape.
        let v = json_of(&text);
        assert_eq!(v["v"], 3);
        assert!(v.get("addrs").is_none(), "the addresses travel sealed: {v}");
        let sealed = URL_SAFE_NO_PAD.decode(v["sealed"].as_str().unwrap()).unwrap();
        let plain = serde_json::to_vec(&invite.addrs).unwrap();
        assert_eq!(sealed.len(), NONCE_LEN + plain.len() + 16, "nonce, ciphertext, tag");
        assert_eq!(v["relay"]["url"], "wss://relay.example.com");
        assert_eq!(v["relay"]["room"].as_str().unwrap().len(), 22);
        assert_eq!(v["project"], "Bungalow");
        assert_eq!(v.as_object().unwrap().len(), 6, "no key but the documented ones: {v}");
        // Every invite gets its own nonce.
        assert_ne!(json_of(&invite.encode())["sealed"], v["sealed"]);

        // Without a relay the key is left out.
        let mut direct = sample();
        direct.relay = None;
        let text = direct.encode();
        assert!(json_of(&text).get("relay").is_none());
        assert_eq!(Invite::decode(&text).unwrap(), direct);

        // Through the relay only: no addresses, so no seal.
        let mut relayed = sample();
        relayed.addrs.clear();
        let text = relayed.encode();
        let v = json_of(&text);
        assert!(v.get("sealed").is_none() && v.get("addrs").is_none(), "{v}");
        assert_eq!(v.as_object().unwrap().len(), 5);
        assert_eq!(Invite::decode(&text).unwrap(), relayed);
    }

    #[test]
    fn the_invite_text_names_no_address() {
        let mut invite = sample();
        invite.addrs = ["192.168.1.20:1460", "100.101.102.103:1460", "10.0.0.5:1460", "127.0.0.1:1460"]
            .map(String::from)
            .to_vec();
        for relay in [invite.relay.clone(), None] {
            invite.relay = relay;
            let text = invite.encode();
            let body = body_of(&text);
            for needle in ["192.168", "100.101.102.103", "10.0.0.5", "127.0.0.1", ":1460", "addrs"] {
                assert!(!body.contains(needle), "{needle} in {body}");
            }
            // No dotted quad of any kind.
            let quads = body.split(|c: char| !(c.is_ascii_digit() || c == '.')).filter(|t| t.matches('.').count() == 3);
            assert_eq!(quads.count(), 0, "{body}");
            assert_eq!(Invite::decode(&text).unwrap().addrs, invite.addrs);
        }
    }

    #[test]
    fn a_tampered_seal_is_refused() {
        let text = sample().encode();
        let good = json_of(&text);
        let bytes = URL_SAFE_NO_PAD.decode(good["sealed"].as_str().unwrap()).unwrap();
        // One bit flipped in the nonce, the ciphertext and the tag.
        for i in [0, NONCE_LEN, bytes.len() - 1] {
            let mut flipped = bytes.clone();
            flipped[i] ^= 1;
            let mut v = good.clone();
            v["sealed"] = URL_SAFE_NO_PAD.encode(&flipped).into();
            refused(&v);
        }
        let short = URL_SAFE_NO_PAD.encode(&bytes[..NONCE_LEN + 15]);
        for sealed in [json!(short), json!(""), json!("!!not base64!!"), json!(7), json!(["192.168.1.20:1460"])] {
            let mut v = good.clone();
            v["sealed"] = sealed;
            refused(&v);
        }
        // Another secret does not open it.
        let mut v = good.clone();
        v["secret"] = URL_SAFE_NO_PAD.encode([8u8; 16]).into();
        refused(&v);
        // Nor does a seal of something that is not a list of addresses.
        let key = seal_key(&[7u8; 16]).unwrap();
        let mut plain = br#"{"addrs":1}"#.to_vec();
        key.seal_in_place_append_tag(Nonce::assume_unique_for_key([0; NONCE_LEN]), Aad::from([9u8; 32]), &mut plain)
            .unwrap();
        let mut v = good.clone();
        v["sealed"] = URL_SAFE_NO_PAD.encode([&[0u8; NONCE_LEN][..], &plain].concat()).into();
        refused(&v);
        // Untouched, it still opens.
        assert_eq!(Invite::decode(&text).unwrap(), sample());
    }

    #[test]
    fn a_seal_from_another_invite_does_not_open() {
        let from = json_of(&sample().encode());
        // Same secret, another certificate: the pin is bound to the seal.
        let mut other = sample();
        other.pin = URL_SAFE_NO_PAD.encode([3u8; 32]);
        other.addrs = vec!["10.9.8.7:1460".into()];
        let mut v = json_of(&other.encode());
        assert!(Invite::decode(&with_json(&v.to_string())).is_ok());
        v["sealed"] = from["sealed"].clone();
        refused(&v);
        // Another session.
        let mut other = sample();
        other.secret = URL_SAFE_NO_PAD.encode([8u8; 16]);
        let mut v = json_of(&other.encode());
        v["sealed"] = from["sealed"].clone();
        refused(&v);
    }

    #[test]
    fn version_3_takes_addresses_only_sealed() {
        let secret = URL_SAFE_NO_PAD.encode([7u8; 16]);
        let pin = URL_SAFE_NO_PAD.encode([9u8; 32]);
        let room = URL_SAFE_NO_PAD.encode([5u8; 16]);
        // Plain addresses in a version 3 invite are ignored.
        let plain = json!({ "v": 3, "secret": secret, "pin": pin, "addrs": ["10.0.0.5:1460"], "project": "Bahay" });
        refused(&plain);
        let mut relayed = plain.clone();
        relayed["relay"] = json!({ "url": "wss://relay.example.com", "room": room });
        let invite = Invite::decode(&with_json(&relayed.to_string())).unwrap();
        assert_eq!((invite.v, invite.addrs.len()), (3, 0));
    }

    #[test]
    fn versions_1_and_2_are_still_read() {
        let secret = URL_SAFE_NO_PAD.encode([7u8; 16]);
        let pin = URL_SAFE_NO_PAD.encode([9u8; 32]);
        let v1 = format!(r#"{{"v":1,"secret":"{secret}","pin":"{pin}","addrs":["10.0.0.5:1460"],"project":"Bahay"}}"#);
        let invite = Invite::decode(&with_json(&v1)).unwrap();
        assert_eq!((invite.v, invite.relay.as_ref()), (1, None));
        assert_eq!(invite.addrs, vec!["10.0.0.5:1460".to_string()]);
        // Version 1 had no relay, so it cannot stand in for the addresses.
        let room = URL_SAFE_NO_PAD.encode([5u8; 16]);
        let odd = format!(
            r#"{{"v":1,"secret":"{secret}","pin":"{pin}","addrs":[],"relay":{{"url":"wss://relay.example.com","room":"{room}"}},"project":"Bahay"}}"#
        );
        assert_eq!(Invite::decode(&with_json(&odd)).unwrap_err().code, "bad_args");

        let v2 = json!({
            "v": 2, "secret": secret, "pin": pin, "addrs": ["10.0.0.5:1460", "127.0.0.1:1460"], "project": "Bahay",
            "relay": { "url": "wss://relay.example.com", "room": room },
        });
        let invite = Invite::decode(&with_json(&v2.to_string())).unwrap();
        assert_eq!(invite.v, 2);
        assert_eq!(invite.addrs, vec!["10.0.0.5:1460".to_string(), "127.0.0.1:1460".to_string()]);
        assert_eq!(invite.relay.as_ref().map(|r| r.url.as_str()), Some("wss://relay.example.com"));
        let mut relayed = v2.clone();
        relayed["addrs"] = json!([]);
        assert!(Invite::decode(&with_json(&relayed.to_string())).unwrap().addrs.is_empty());
        let mut too_many = v2.clone();
        too_many["addrs"] = json!((0..=MAX_ADDRS).map(|i| format!("10.0.0.{i}:1460")).collect::<Vec<_>>());
        refused(&too_many);

        // Written again, it is version 3 with the addresses sealed.
        let again = json_of(&invite.encode());
        assert_eq!(again["v"], 3);
        assert!(again.get("addrs").is_none());
        let back = Invite::decode(&invite.encode()).unwrap();
        assert_eq!((back.v, &back.addrs), (3, &invite.addrs));
    }

    #[test]
    fn pasted_invites_are_forgiven_whitespace_and_padding() {
        let text = sample().encode();
        let (head, tail) = text.split_at(30);
        let messy = format!("  {head}\n  {tail}==\n");
        assert_eq!(Invite::decode(&messy).unwrap(), sample());
    }

    #[test]
    fn bad_invites_are_refused() {
        let bad = |text: &str| {
            let err = Invite::decode(text).unwrap_err();
            assert_eq!(err.code, "bad_args", "{text}");
            err.message
        };
        assert_eq!(bad("hello"), "This is not a Guhit live session invite.");
        bad("");
        bad("guhit-live:");
        bad("guhit-live:!!!not base64!!!");
        bad(&with_json("not json"));
        bad(&with_json("{}"));
        bad(&with_json(r#"{"v":1}"#));
        bad(&with_json(r#"{"v":2}"#));
        bad(&with_json(r#"{"v":3}"#));
        bad(&with_json(r#"{"v":0}"#));
        bad(&format!("guhit-live:{}", "A".repeat(MAX_INVITE_CHARS)));

        let mut no_way_in = sample();
        no_way_in.addrs.clear();
        no_way_in.relay = None;
        bad(&no_way_in.encode());
        let mut too_many = sample();
        too_many.addrs = (0..=MAX_ADDRS).map(|i| format!("10.0.0.{i}:1460")).collect();
        bad(&too_many.encode());
        too_many.addrs.pop();
        assert!(Invite::decode(&too_many.encode()).is_ok(), "{MAX_ADDRS} addresses fit");
        let mut bad_addr = sample();
        bad_addr.addrs = vec!["192.168.1.20".into()];
        bad(&bad_addr.encode());
        let mut short_secret = sample();
        short_secret.secret = "abc".into();
        bad(&short_secret.encode());
        let mut bad_pin = sample();
        bad_pin.pin = URL_SAFE_NO_PAD.encode([1u8; 20]);
        bad(&bad_pin.encode());

        let relay = |url: &str, room: &str| {
            let mut invite = sample();
            invite.relay = Some(InviteRelay { url: url.into(), room: room.into() });
            invite.encode()
        };
        let room = URL_SAFE_NO_PAD.encode([5u8; 16]);
        bad(&relay("wss://relay.example.com", "short"));
        bad(&relay("wss://relay.example.com", &URL_SAFE_NO_PAD.encode([5u8; 32])));
        bad(&relay("ws://relay.example.com", &room));
        bad(&relay("https://relay.example.com", &room));
        bad(&relay("wss://relay.example.com/?a=b", &room));
        bad(&relay("not a url", &room));
        assert!(Invite::decode(&relay("ws://127.0.0.1:1470", &room)).is_ok(), "a relay on this computer");

        assert!(bad(&with_json(r#"{"v":4}"#)).contains("newer version"));
    }

    #[test]
    fn the_longest_invite_a_host_makes_is_still_read() {
        let mut invite = sample();
        // The widest addresses a host lists (IPv4 only, docs/CONTRACT.md).
        invite.addrs = (0..MAX_ADDRS).map(|i| format!("255.255.255.{}:65535", 200 + i)).collect();
        // 120 characters, the longest project name, of the widest kind.
        invite.project = "\u{1F3E0}\"".repeat(60);
        let room = invite.relay.as_ref().unwrap().room.clone();
        let url = format!("wss://relay.example.com/{}", "a".repeat(512 - 24));
        assert!(relay::check_url(&url).is_ok(), "the longest relay address");
        invite.relay = Some(InviteRelay { url, room });
        let text = invite.encode();
        assert!(text.len() < MAX_INVITE_CHARS / 2, "{} characters", text.len());
        assert_eq!(Invite::decode(&text).unwrap(), invite);
    }

    #[test]
    fn the_project_name_is_cleaned() {
        let mut invite = sample();
        invite.project = format!(" Bahay\u{7} {}", "x".repeat(300));
        let back = Invite::decode(&invite.encode()).unwrap();
        assert!(back.project.starts_with("Bahay x"));
        assert!(back.project.chars().count() <= MAX_PROJECT_CHARS);
    }
}
