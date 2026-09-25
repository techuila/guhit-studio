//! What the people in the window are doing: the user's selection (DECISIONS
//! D30) and the live session with its chat (DECISIONS D29).

use guhit_app::ai::tools as copilot;
use guhit_app::AppService;
use guhit_model::*;
use serde_json::{json, Value};

use crate::tools::{obj, opt_bool, req_str, Output, ToolDef, ToolFail, MM};

/// Chat messages `get_session` hands back, the newest.
const RECENT_CHAT: usize = 30;
/// Labels listed per participant's selection.
const SELECTION_LABELS: usize = 20;

pub fn definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "get_selection",
            description: format!("What the user has selected in the Guhit Studio window right now: each element's id, kind and readable label, the level on screen, and the full data of the selected elements. Call this whenever the user says \"this\", \"the selected\", \"these walls\" or \"the selection\". limited_to_selection true means the user switched on \"Only the selection\" in the app: every edit from this server is then limited to the selection, and anything else is refused. To limit one edit yourself, pass scope \"selection\" to the editing tool. {MM}"),
            schema: obj(json!({}), &[]),
            read_only: true,
            open_world: false,
        },
        ToolDef {
            name: "get_session",
            description: format!("The live session the Guhit Studio window is in, if any: whether this computer hosts or joined it, everyone in it with what they have selected and the level they are on, and the latest chat messages. In a live session several people edit the same plan; every edit from this server shows up for all of them, and undo is one shared history. {MM}"),
            schema: obj(json!({}), &[]),
            read_only: true,
            open_world: false,
        },
        ToolDef {
            name: "send_chat_message",
            description: format!("Post a message to the live session's chat as the user of this computer, marked as sent by AI. Everyone in the session sees it in the chat panel and beside the user's pointer, and it is saved with the project. Use it when the user asks you to tell the others something, for example what you just changed. Needs a live session. {MM}"),
            schema: obj(
                json!({"text": {"type": "string", "description": "The message, 1 to 2000 characters, plain text."}}),
                &["text"],
            ),
            read_only: false,
            open_world: true,
        },
    ]
}

pub async fn call(app: &AppService, name: &str, args: &Value) -> Option<Result<Output, ToolFail>> {
    Some(match name {
        "get_selection" => get_selection(app).await,
        "get_session" => get_session(app).await,
        "send_chat_message" => send_chat_message(app, args).await,
        _ => return None,
    })
}

/// Id, kind and label of each id that is in the project, in the given order.
fn briefs(project: &Project, ids: &[Id]) -> Vec<Value> {
    let rooms = guhit_core::asset_rooms(project);
    ids.iter()
        .filter_map(|id| project.elements.iter().find(|e| e.id() == id))
        .map(|e| json!({"id": e.id(), "kind": e.kind(), "label": copilot::element_label_in(e, &rooms)}))
        .collect()
}

fn level_brief(project: &Project, id: Option<&Id>) -> Value {
    id.and_then(|id| project.levels.iter().find(|l| &l.id == id))
        .map(|l| json!({"id": l.id, "name": l.name}))
        .unwrap_or(Value::Null)
}

async fn get_selection(app: &AppService) -> Result<Output, ToolFail> {
    let presence = app.local_presence();
    let s = app.session.lock().await;
    let doc = s
        .doc
        .as_ref()
        .ok_or_else(|| ToolFail("no_document: no project is open. Call create_project or open_project first".into()))?;
    let project = doc.project();
    let selected = briefs(project, &presence.selection);
    let ids: Vec<Id> = selected.iter().filter_map(|b| b["id"].as_str().map(str::to_string)).collect();
    let details = if ids.is_empty() {
        Value::Null
    } else {
        doc.query(&Query::Describe { ids: ids.clone() }).map_err(|e| ToolFail::from(IpcError::from(e)))?
    };
    let limited = presence.ai_scope && !ids.is_empty();
    let note = if ids.is_empty() {
        "Nothing is selected in the Guhit Studio window. Ask the user to select the parts they mean, or find them with list_rooms or list_elements."
    } else if limited {
        "The user switched on \"Only the selection\": every edit from this server is limited to these elements and what stands in a selected room. Anything else is refused with out_of_scope."
    } else {
        "Pass scope \"selection\" to an editing tool to limit that edit to these elements."
    };
    Ok(Output::Json(json!({
        "count": ids.len(),
        "selected": selected,
        "level": level_brief(project, presence.level_id.as_ref()),
        "limited_to_selection": limited,
        "details": details,
        "note": note,
    })))
}

async fn get_session(app: &AppService) -> Result<Output, ToolFail> {
    let status = app.live_status().await;
    if status.mode == LiveMode::Off {
        return Ok(Output::Json(json!({
            "mode": "off",
            "note": "No live session. The user starts one with Share in the app, and others join with the invite it shows: from the same network or VPN, or over the internet when a relay is set up.",
        })));
    }
    let presences: Vec<PresenceEntry> = serde_json::from_value(app.handle("presence_list", json!({})).await?)
        .map_err(|e| ToolFail(format!("io: {e}")))?;
    let chat = app.chat_list().await.unwrap_or_default();
    let s = app.session.lock().await;
    let project = s.doc.as_ref().map(|d| d.project());
    let participants: Vec<Value> = status
        .participants
        .iter()
        .map(|p| {
            let is_self = status.self_id.as_deref() == Some(p.id.as_str());
            let presence = if is_self {
                Some(app.local_presence())
            } else {
                presences.iter().find(|e| e.participant_id == p.id).map(|e| e.presence.clone())
            };
            let mut one = json!({"id": p.id, "name": p.name, "role": p.role, "you": is_self});
            if let (Some(project), Some(presence)) = (project, presence) {
                let mut selected = briefs(project, &presence.selection);
                let count = selected.len();
                selected.truncate(SELECTION_LABELS);
                one["selected_count"] = json!(count);
                one["selected"] = json!(selected);
                one["level"] = level_brief(project, presence.level_id.as_ref());
                if let Some(typing) = presence.typing.filter(|t| !t.trim().is_empty()) {
                    one["typing"] = json!(typing);
                }
            }
            one
        })
        .collect();
    let recent: Vec<Value> = chat
        .iter()
        .skip(chat.len().saturating_sub(RECENT_CHAT))
        .map(|m| json!({"from": m.author_name, "text": m.text, "sent_at": m.sent_at, "via_ai": m.via_ai}))
        .collect();
    Ok(Output::Json(json!({
        "mode": status.mode,
        "project": {"id": status.project_id, "name": status.project_name},
        "participants": participants,
        "recent_chat": recent,
        "note": "Everyone here sees your edits as they land. Undo is one shared history: it takes back the last step, whoever made it.",
    })))
}

async fn send_chat_message(app: &AppService, args: &Value) -> Result<Output, ToolFail> {
    let text = req_str(args, "text")?;
    let message = app.chat_send(&text, None, None, true).await?;
    Ok(Output::Json(json!({"ok": true, "message": message})))
}

// ---------------------------------------------------- leaving a live session

/// What a hub tool does to the open project.
#[derive(Clone, Copy)]
pub(crate) enum Switch<'a> {
    /// `open_project` of this project id.
    Open(&'a str),
    Create,
    Close,
}

/// The `force` argument of `open_project`, `create_project` and `close_project`.
pub(crate) fn switch_force_schema() -> Value {
    json!({"type": "boolean", "description": "Go ahead in a live session although it ends the session for everyone (or, on a computer that joined one, leaves it). Only after the user confirms. Defaults to false."})
}

/// In a live session, opening, creating or closing a project ends the session
/// for everyone when this computer hosts it, and closing leaves it when this
/// computer joined. The window asks first; here the tool refuses with
/// `live_session` unless `force` is true (DECISIONS D29). Opening or creating
/// on a computer that joined is left to the app, which refuses it.
pub(crate) async fn guard_switch(app: &AppService, args: &Value, what: Switch<'_>) -> Result<(), ToolFail> {
    if opt_bool(args, "force")?.unwrap_or(false) {
        return Ok(());
    }
    let status = app.live_status().await;
    let doing = match what {
        // The shared project is already open: nothing changes.
        Switch::Open(id) if status.project_id.as_deref() == Some(id) => return Ok(()),
        Switch::Open(_) => "Opening another project",
        Switch::Create => "Creating a project",
        Switch::Close => "Closing the project",
    };
    let ask = "Ask the user first, and call again with force true only after they agree";
    match status.mode {
        LiveMode::Hosting => {
            let others: Vec<&str> = status
                .participants
                .iter()
                .filter(|p| status.self_id.as_deref() != Some(p.id.as_str()))
                .map(|p| p.name.as_str())
                .collect();
            let with = if others.is_empty() { String::new() } else { format!(" with {}", names(&others)) };
            Err(ToolFail(format!(
                "live_session: the user is hosting a live session{with}. {doing} ends it for everyone. {ask}"
            )))
        }
        LiveMode::Joined | LiveMode::Reconnecting if matches!(what, Switch::Close) => {
            let host = status
                .participants
                .iter()
                .find(|p| p.role == ParticipantRole::Host)
                .map_or_else(|| "the host".to_string(), |p| p.name.clone());
            Err(ToolFail(format!(
                "live_session: the user joined the live session {host} hosts. Closing the project leaves it. {ask}"
            )))
        }
        _ => Ok(()),
    }
}

/// "Ben", "Ben and Carla", "Ben, Carla and Dan".
fn names(list: &[&str]) -> String {
    match list {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}
