//! Live sessions: several people working on one plan at once (DECISIONS D29).
//! CONTRACT FILE - owned by the orchestrator.
//!
//! One computer hosts the open project. Its `Document` stays the only
//! authority over the model: guests keep a read-only copy, send typed
//! `Command`s, and the host validates, applies and sends the new state to
//! everyone. Presence (pointer, selection, cursor chat) is sent many times a
//! second and never saved. Chat messages are saved with the project on the
//! host (`chat.jsonl`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::*;

/// Participant colors. A participant's `color` is an index into the tokens
/// `--peer-0` to `--peer-7` in `src/styles/tokens.css`.
pub const PEER_COLOR_COUNT: u8 = 8;

/// Most people in one session, the host included.
pub const MAX_PARTICIPANTS: usize = 16;

/// Longest display name, in characters.
pub const MAX_NAME_CHARS: usize = 40;

/// Longest chat message, in characters. Cursor chat is shorter.
pub const MAX_CHAT_CHARS: usize = 2000;
pub const MAX_CURSOR_CHAT_CHARS: usize = 160;

/// Your part in a live session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ParticipantRole {
    /// Shares the open project from this computer. The project, its history
    /// and its chat live here.
    Host,
    /// Joined from another computer with an invite.
    Guest,
}

/// A person in a live session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Participant {
    pub id: Id,
    pub name: String,
    /// Index into the participant colors, `--peer-0` to `--peer-7`. The host
    /// gives each participant a color no one else in the session has, while
    /// colors last.
    pub color: u8,
    pub role: ParticipantRole,
}

/// Where a participant is and what they are doing. Sent many times a second,
/// never saved, never an undo step.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Presence {
    /// Pointer on the plan in mm. None while the pointer is off the plan.
    pub cursor: Option<Point>,
    /// The level they are looking at.
    pub level_id: Option<Id>,
    /// What they have selected.
    pub selection: Vec<Id>,
    /// Cursor chat being typed, shown beside their pointer as they type.
    /// None when cursor chat is closed. At most `MAX_CURSOR_CHAT_CHARS`.
    pub typing: Option<String>,
    /// AI edits made from their computer may change only their selection
    /// (DECISIONS D30): the copilot and MCP clients.
    pub ai_scope: bool,
}

/// One chat message of a project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ChatMessage {
    pub id: Id,
    /// The participant who sent it. Kept after they leave, so old messages
    /// keep their name and color.
    pub author_id: Id,
    pub author_name: String,
    pub color: u8,
    pub text: String,
    /// RFC 3339.
    pub sent_at: String,
    /// Where the author's pointer was on the plan when they sent it from
    /// cursor chat. None for messages from the chat panel.
    #[serde(default)]
    pub at: Option<Point>,
    #[serde(default)]
    pub level_id: Option<Id>,
    /// Sent by an AI client on the author's behalf (the MCP tool
    /// `send_chat_message`). Shown with an AI tag.
    #[serde(default)]
    pub via_ai: bool,
}

/// Is this computer in a live session, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LiveMode {
    /// No live session.
    Off,
    /// This computer shares the open project.
    Hosting,
    /// Working on a project another computer shares.
    Joined,
    /// Joined, the connection to the host dropped and is being retried. Edits
    /// wait; the plan on screen is the last one the host sent.
    Reconnecting,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LiveStatus {
    pub mode: LiveMode,
    /// This computer's participant. None when off.
    pub self_id: Option<Id>,
    /// Everyone in the session, this computer included, host first.
    pub participants: Vec<Participant>,
    /// Host only: the invite to send to the people who should join. It holds
    /// the host's addresses, a one-session secret and the pin of the host's
    /// certificate. Anyone who has it can join while the session runs.
    pub invite: Option<String>,
    /// Host only: where guests reach this computer, "192.168.1.20:1460".
    pub addresses: Vec<String>,
    /// The shared project.
    pub project_id: Option<Id>,
    pub project_name: Option<String>,
    /// Why the last session ended or why joining failed, in one sentence.
    /// Cleared when a new session starts.
    pub notice: Option<String>,
}

impl LiveStatus {
    pub fn off() -> Self {
        Self {
            mode: LiveMode::Off,
            self_id: None,
            participants: vec![],
            invite: None,
            addresses: vec![],
            project_id: None,
            project_name: None,
            notice: None,
        }
    }
}

/// The name this computer shows to others: cursor label, chat, history.
/// Stored in `settings.json` (`profile_name`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Profile {
    /// Empty until the user picks one. Hosting and joining need a name.
    pub name: String,
}

/// Someone's latest presence, as `presence_list` returns it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PresenceEntry {
    pub participant_id: Id,
    pub presence: Presence,
}
