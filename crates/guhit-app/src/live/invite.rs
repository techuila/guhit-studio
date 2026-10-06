//! The invite a host sends to the people who should join (docs/CONTRACT.md,
//! "Live sessions"): `guhit-live:` then base64url, without padding, of
//! `{"v":2,"secret":...,"pin":...,"addrs":["192.168.1.20:1460",...],"project":"Bungalow","relay":{"url":"wss://relay.example.com","room":...}}`.
//!
//! `secret` is the session's 128-bit secret, `pin` the SHA-256 of the host's
//! certificate, `room` the session's 128-bit room on the relay (DECISIONS
//! D32), all base64url. `relay` is left out when the host has no relay, and
//! `addrs` is empty when the host takes no direct connections; one of the two
//! is always there. The key that proves to the relay that a connection is the
//! host's never leaves the host, so an invite lets a guest join the room, not
//! take it. Anyone with the invite can join while the session runs.
//!
//! Version 1 invites (addresses only, no relay) are still read.

use std::net::SocketAddr;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use guhit_model::IpcError;
use serde::{Deserialize, Serialize};

use super::relay;

pub const INVITE_PREFIX: &str = "guhit-live:";
/// Version of the invite format. 2 added `relay`.
pub const INVITE_VERSION: u32 = 2;
/// Longest invite accepted, in characters. Real ones are about 250, or 400
/// with a relay.
const MAX_INVITE_CHARS: usize = 4096;
/// Most addresses an invite may list.
pub const MAX_ADDRS: usize = 8;
const MAX_PROJECT_CHARS: usize = 120;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Invite {
    pub v: u32,
    pub secret: String,
    pub pin: String,
    /// Where the host listens for direct connections. A guest tries them all
    /// at once. Empty when the host takes none.
    #[serde(default)]
    pub addrs: Vec<String>,
    /// The shared project's name, for display before joining.
    pub project: String,
    /// The relay the host registered the session with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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

fn not_an_invite() -> IpcError {
    IpcError::new("bad_args", "This is not a Guhit live session invite.")
}

/// True for base64url text that decodes to exactly `len` bytes.
fn is_b64url_of(text: &str, len: usize) -> bool {
    URL_SAFE_NO_PAD.decode(text.as_bytes()).is_ok_and(|b| b.len() == len)
}

impl Invite {
    pub fn encode(&self) -> String {
        let json = serde_json::to_vec(self).unwrap_or_default();
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
            Some(v) if (1..=INVITE_VERSION as u64).contains(&v) => v,
            Some(v) if v > INVITE_VERSION as u64 => {
                return Err(IpcError::new(
                    "bad_args",
                    "This invite is from a newer version of Guhit Studio. Update the app to join.",
                ))
            }
            _ => return Err(not_an_invite()),
        };
        let mut invite: Invite = serde_json::from_value(raw).map_err(|_| not_an_invite())?;
        if version == 1 {
            // Version 1 had no relay.
            invite.relay = None;
        }
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

    fn json_of(text: &str) -> serde_json::Value {
        let json = URL_SAFE_NO_PAD.decode(text.strip_prefix(INVITE_PREFIX).unwrap()).unwrap();
        serde_json::from_slice(&json).unwrap()
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
        assert_eq!(v["v"], 2);
        assert_eq!(v["addrs"][0], "192.168.1.20:1460");
        assert_eq!(v["relay"]["url"], "wss://relay.example.com");
        assert_eq!(v["relay"]["room"].as_str().unwrap().len(), 22);
        assert_eq!(v["project"], "Bungalow");
        assert_eq!(v.as_object().unwrap().len(), 6, "no key but the documented ones: {v}");

        // Without a relay the key is left out.
        let mut direct = sample();
        direct.relay = None;
        let text = direct.encode();
        assert!(json_of(&text).get("relay").is_none());
        assert_eq!(Invite::decode(&text).unwrap(), direct);

        // Through the relay only: no addresses.
        let mut relayed = sample();
        relayed.addrs.clear();
        assert_eq!(Invite::decode(&relayed.encode()).unwrap(), relayed);
    }

    #[test]
    fn version_1_invites_are_still_read() {
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

        let mut newer = sample();
        newer.v = 3;
        assert!(bad(&newer.encode()).contains("newer version"));
    }

    #[test]
    fn the_longest_invite_a_host_makes_is_still_read() {
        let mut invite = sample();
        invite.addrs = (0..MAX_ADDRS).map(|i| format!("255.255.255.{i}:65535")).collect();
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
