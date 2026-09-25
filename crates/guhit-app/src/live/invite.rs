//! The invite a host sends to the people who should join (docs/CONTRACT.md,
//! "Live sessions"): `guhit-live:` then base64url, without padding, of
//! `{"v":1,"secret":...,"pin":...,"addrs":["192.168.1.20:1460",...],"project":"Bungalow"}`.
//!
//! `secret` is the session's 128-bit secret, `pin` the SHA-256 of the host's
//! certificate, both base64url. Anyone with the invite can join while the
//! session runs.

use std::net::SocketAddr;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use guhit_model::IpcError;
use serde::{Deserialize, Serialize};

pub const INVITE_PREFIX: &str = "guhit-live:";
/// Version of the invite format.
pub const INVITE_VERSION: u32 = 1;
/// Longest invite accepted, in characters. Real ones are about 250.
const MAX_INVITE_CHARS: usize = 4096;
/// Most addresses an invite may list.
const MAX_ADDRS: usize = 8;
const MAX_PROJECT_CHARS: usize = 120;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Invite {
    pub v: u32,
    pub secret: String,
    pub pin: String,
    /// Where the host listens, tried in order.
    pub addrs: Vec<String>,
    /// The shared project's name, for display before joining.
    pub project: String,
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
        match raw.get("v").and_then(|v| v.as_u64()) {
            Some(v) if v == INVITE_VERSION as u64 => {}
            Some(v) if v > INVITE_VERSION as u64 => {
                return Err(IpcError::new(
                    "bad_args",
                    "This invite is from a newer version of Guhit Studio. Update the app to join.",
                ))
            }
            _ => return Err(not_an_invite()),
        }
        let mut invite: Invite = serde_json::from_value(raw).map_err(|_| not_an_invite())?;
        // The secret is 128 bits and the pin a SHA-256, both base64url.
        if !is_b64url_of(&invite.secret, 16) || !is_b64url_of(&invite.pin, 32) {
            return Err(not_an_invite());
        }
        if invite.addrs.is_empty()
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
            project: "Bungalow".into(),
        }
    }

    fn with_json(json: &str) -> String {
        format!("{INVITE_PREFIX}{}", URL_SAFE_NO_PAD.encode(json))
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
        let json = URL_SAFE_NO_PAD.decode(text.strip_prefix(INVITE_PREFIX).unwrap()).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&json).unwrap();
        assert_eq!(v["v"], 1);
        assert_eq!(v["addrs"][0], "192.168.1.20:1460");
        assert_eq!(v["project"], "Bungalow");
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
        bad(&format!("guhit-live:{}", "A".repeat(MAX_INVITE_CHARS)));

        let mut no_addrs = sample();
        no_addrs.addrs.clear();
        bad(&no_addrs.encode());
        let mut bad_addr = sample();
        bad_addr.addrs = vec!["192.168.1.20".into()];
        bad(&bad_addr.encode());
        let mut short_secret = sample();
        short_secret.secret = "abc".into();
        bad(&short_secret.encode());
        let mut bad_pin = sample();
        bad_pin.pin = URL_SAFE_NO_PAD.encode([1u8; 20]);
        bad(&bad_pin.encode());

        let mut newer = sample();
        newer.v = 2;
        assert!(bad(&newer.encode()).contains("newer version"));
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
