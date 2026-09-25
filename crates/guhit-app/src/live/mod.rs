//! Live sessions (DECISIONS D29, docs/CONTRACT.md "Live sessions"): one
//! computer hosts the open project, others join with an invite and work on it
//! together, with presence (pointer, selection, cursor chat) and chat.
//!
//! This module also owns this window's own presence (`presence_set`), which
//! the MCP tool `get_selection` reads with or without a session, and the
//! display name (`profile_get`, `profile_set`).

pub mod invite;
pub mod tls;
pub mod wire;

use std::path::{Path, PathBuf};

use guhit_model::*;
use serde_json::{json, Value};

use crate::{arg, files, to_value, AppService, IpcResult};

pub const OWNS: &[&str] = &[
    "presence_set",
    "presence_list",
    "profile_get",
    "profile_set",
    "live_status",
    "live_host",
    "live_join",
    "live_leave",
    "live_remove",
    "live_save_copy",
    "chat_send",
    "chat_list",
];

const SETTINGS_FILE: &str = "settings.json";
const PROFILE_KEY: &str = "profile_name";
/// The project's chat, one `ChatMessage` per line, in the project folder.
pub const CHAT_FILE: &str = "chat.jsonl";
/// `chat_list` returns at most this many, the newest.
pub const CHAT_HISTORY: usize = 500;
/// Longest selection a presence carries.
pub const MAX_PRESENCE_SELECTION: usize = 2000;

/// Live session state of one `AppService`.
pub struct LiveState {
    /// This window's presence, from `presence_set`.
    local: std::sync::Mutex<Presence>,
    status: tokio::sync::Mutex<LiveStatus>,
}

impl Default for LiveState {
    fn default() -> Self {
        Self {
            local: std::sync::Mutex::new(Presence::default()),
            status: tokio::sync::Mutex::new(LiveStatus::off()),
        }
    }
}

// ------------------------------------------------------------------ helpers

fn settings_of(dir: &Path) -> serde_json::Map<String, Value> {
    std::fs::read_to_string(dir.join(SETTINGS_FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// Merge one key into `settings.json`; other parts of the app keep their own
/// keys in the same file.
fn put_setting(dir: &Path, key: &str, value: Value) -> Result<(), IpcError> {
    let mut settings = settings_of(dir);
    settings.insert(key.to_string(), value);
    files::create_dir(dir)?;
    files::write_json_atomic(&dir.join(SETTINGS_FILE), &Value::Object(settings))
}

/// A display name as people will see it: control characters removed, spaces
/// trimmed, at most `MAX_NAME_CHARS` characters.
pub fn clean_name(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_NAME_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

/// A presence as it may travel: cursor chat and the selection cut to their
/// limits, a cursor with a coordinate that is not a number dropped.
pub fn clean_presence(mut presence: Presence) -> Presence {
    if let Some(p) = presence.cursor {
        if !p.x.is_finite() || !p.y.is_finite() {
            presence.cursor = None;
        }
    }
    if let Some(text) = presence.typing.as_mut() {
        *text = text.chars().filter(|c| !c.is_control()).take(MAX_CURSOR_CHAT_CHARS).collect();
    }
    presence.selection.truncate(MAX_PRESENCE_SELECTION);
    presence
}

/// The last `CHAT_HISTORY` messages of `chat.jsonl` in `dir`, oldest first.
/// A line that is not a message is skipped; a missing file is no messages.
pub fn read_chat(dir: &Path) -> Vec<ChatMessage> {
    let Ok(text) = std::fs::read_to_string(dir.join(CHAT_FILE)) else {
        return vec![];
    };
    let mut all: Vec<ChatMessage> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    if all.len() > CHAT_HISTORY {
        all.drain(..all.len() - CHAT_HISTORY);
    }
    all
}

/// Append one message to `chat.jsonl` in `dir`. Best effort: a chat line that
/// cannot be written never fails the session.
pub fn append_chat(dir: &Path, message: &ChatMessage) {
    use std::io::Write;
    let Ok(line) = serde_json::to_string(message) else { return };
    let result = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(CHAT_FILE))
        .and_then(|mut f| writeln!(f, "{line}"));
    if let Err(e) = result {
        eprintln!("guhit-app: chat message not saved: {e}");
    }
}

impl AppService {
    /// This window's presence, as `presence_set` last stored it. The MCP tool
    /// `get_selection` reads it.
    pub fn local_presence(&self) -> Presence {
        self.live.local.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The live session as the window sees it.
    pub async fn live_status(&self) -> LiveStatus {
        self.live.status.lock().await.clone()
    }

    /// The display name, `""` until one is chosen.
    pub async fn profile_name(&self) -> String {
        let dir = self.session.lock().await.data_dir.clone();
        settings_of(&dir)
            .get(PROFILE_KEY)
            .and_then(Value::as_str)
            .map(clean_name)
            .unwrap_or_default()
    }

    /// Send a chat message in the live session as this computer's
    /// participant. `via_ai` marks a message an MCP client sent. `not_live`
    /// without a session.
    pub async fn chat_send(
        &self,
        text: &str,
        at: Option<Point>,
        level_id: Option<Id>,
        via_ai: bool,
    ) -> Result<ChatMessage, IpcError> {
        let _ = (text, at, level_id, via_ai);
        Err(IpcError::new("not_live", "Start or join a live session to chat."))
    }

    /// The open project's chat, oldest first, at most `CHAT_HISTORY`.
    pub async fn chat_list(&self) -> Result<Vec<ChatMessage>, IpcError> {
        let dir = self.project_dir().await.ok_or_else(crate::no_document)?;
        Ok(read_chat(&dir))
    }

    /// Folder of a guest's copy of a shared project: renders, exports and the
    /// AI log of the session go here (docs/CONTRACT.md, "Storage layout").
    pub fn live_dir(data_dir: &Path, project_id: &str) -> Result<PathBuf, IpcError> {
        files::check_id(project_id, "project id")?;
        Ok(data_dir.join("live").join(project_id))
    }
}

/// Handles `presence_*`, `profile_*`, `live_*` and `chat_*`.
pub async fn handle(app: &AppService, cmd: &str, args: Value) -> IpcResult {
    match cmd {
        "presence_set" => {
            let presence: Presence = arg(&args, "presence")?;
            *app.live.local.lock().unwrap_or_else(|e| e.into_inner()) = clean_presence(presence);
            Ok(Value::Null)
        }
        "presence_list" => to_value(&Vec::<PresenceEntry>::new()),
        "profile_get" => to_value(&Profile { name: app.profile_name().await }),
        "profile_set" => {
            let name: String = arg(&args, "name")?;
            let name = clean_name(&name);
            if name.is_empty() {
                return Err(IpcError::new("bad_args", "A name needs at least one letter or number."));
            }
            let dir = app.session.lock().await.data_dir.clone();
            put_setting(&dir, PROFILE_KEY, json!(name))?;
            to_value(&Profile { name })
        }
        "live_status" => to_value(&app.live_status().await),
        "chat_send" => {
            let text: String = arg(&args, "text")?;
            let at: Option<Point> = arg(&args, "at")?;
            let level_id: Option<Id> = arg(&args, "level_id")?;
            to_value(&app.chat_send(&text, at, level_id, false).await?)
        }
        "chat_list" => to_value(&app.chat_list().await?),
        "live_host" | "live_join" | "live_leave" | "live_remove" | "live_save_copy" => Err(IpcError::new(
            "not_live",
            "Live sessions are not available in this build.",
        )),
        _ => Err(IpcError::new("unknown_command", format!("unknown command `{cmd}`"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  Ana\u{7}  Reyes \n"), "Ana  Reyes");
        assert_eq!(clean_name(&"x".repeat(80)).chars().count(), MAX_NAME_CHARS);
        assert_eq!(clean_name(" \t "), "");
    }

    #[test]
    fn presence_is_cut_to_its_limits() {
        let p = clean_presence(Presence {
            cursor: Some(Point { x: f64::NAN, y: 0.0 }),
            level_id: None,
            selection: vec!["a".into(); MAX_PRESENCE_SELECTION + 5],
            typing: Some("y".repeat(500)),
            ai_scope: true,
        });
        assert_eq!(p.cursor, None);
        assert_eq!(p.selection.len(), MAX_PRESENCE_SELECTION);
        assert_eq!(p.typing.unwrap().chars().count(), MAX_CURSOR_CHAT_CHARS);
    }

    #[test]
    fn chat_history_keeps_the_newest_and_skips_bad_lines() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..(CHAT_HISTORY + 3) {
            append_chat(
                dir.path(),
                &ChatMessage {
                    id: format!("m{i}"),
                    author_id: "a".into(),
                    author_name: "Ana".into(),
                    color: 0,
                    text: format!("hello {i}"),
                    sent_at: "2026-09-25T00:00:00Z".into(),
                    at: None,
                    level_id: None,
                    via_ai: false,
                },
            );
        }
        std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join(CHAT_FILE))
            .and_then(|mut f| std::io::Write::write_all(&mut f, b"not json\n"))
            .unwrap();
        let chat = read_chat(dir.path());
        assert_eq!(chat.len(), CHAT_HISTORY);
        assert_eq!(chat[0].id, "m3");
        assert_eq!(chat.last().unwrap().id, format!("m{}", CHAT_HISTORY + 2));
    }

    #[tokio::test]
    async fn profile_and_presence_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::new_sandboxed(dir.path().to_path_buf());
        let profile = handle(&app, "profile_get", json!({})).await.unwrap();
        assert_eq!(profile, json!({"name": ""}));
        let err = handle(&app, "profile_set", json!({"name": "  "})).await.unwrap_err();
        assert_eq!(err.code, "bad_args");
        handle(&app, "profile_set", json!({"name": " Ana "})).await.unwrap();
        assert_eq!(app.profile_name().await, "Ana");

        let presence = json!({"cursor": {"x": 10.0, "y": 20.0}, "level_id": null, "selection": ["w1"], "typing": null, "ai_scope": true});
        handle(&app, "presence_set", json!({"presence": presence})).await.unwrap();
        let local = app.local_presence();
        assert_eq!(local.selection, vec!["w1".to_string()]);
        assert!(local.ai_scope);

        let err = handle(&app, "chat_send", json!({"text": "hi"})).await.unwrap_err();
        assert_eq!(err.code, "not_live");
        assert_eq!(app.live_status().await.mode, LiveMode::Off);
    }
}
