//! Who is calling, and which toolsets they asked for (DECISIONS D42, D47).
//!
//! An agent is named, most specific first, by:
//! 1. the `agent` argument every tool takes ("kitchen"), the only way to
//!    tell subagents of one connection apart;
//! 2. a labelled URL, `/mcp/agent/<label>` (the stdio proxy's `--agent`);
//! 3. the stdio proxy's instance header `x-guhit-agent-instance`;
//! 4. the `clientInfo` the client sent ("claude-code").
//!
//! The transport turns a request into an `AgentHint`. The agents registry
//! (`guhit_app::agents`, lane L2) turns it into a participant with a name and
//! a color, refuses a stopped agent and a paused agent's edits, checks its
//! limit and other agents' claims on every commit, and records presence.
//! In-process callers (tests, the copilot) pass no hint and the registry is
//! not involved.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use guhit_app::agents::{AgentCall, AgentHint, CallKind};
use guhit_app::AppService;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::fail::ToolFail;
use crate::tools::Output;
use crate::toolsets::Toolsets;

/// Header the stdio proxy adds: one random id per proxy process, so two
/// stdio agents of the same client are two agents.
pub const INSTANCE_HEADER: &str = "x-guhit-agent-instance";
/// Longest label kept, in characters.
const MAX_LABEL_CHARS: usize = 60;

/// Everything a tool call knows beyond its arguments.
#[derive(Debug, Clone, Default)]
pub struct CallContext {
    pub toolsets: Toolsets,
    /// Who called, from the request. None for in-process callers: no agent
    /// is registered and edits commit as before.
    pub hint: Option<AgentHint>,
    /// The registry's answer for this call, once admitted.
    pub call: Option<AgentCall>,
}

impl CallContext {
    /// A call with every toolset on and nobody named: in-process callers and
    /// tests.
    pub fn local() -> Self {
        Self { toolsets: Toolsets::all(), hint: None, call: None }
    }

    /// A call from an agent the registry knows by `hint`.
    pub fn agent(hint: AgentHint) -> Self {
        Self { toolsets: Toolsets::all(), hint: Some(hint), call: None }
    }
}

/// Control characters out, spaces trimmed and squeezed, at most 60 characters.
pub fn clean_label(raw: &str) -> Option<String> {
    let text: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let text: String = text.chars().take(MAX_LABEL_CHARS).collect();
    (!text.is_empty()).then_some(text)
}

/// `<label>` of a path `/agent/<label>` (the endpoint is nested at `/mcp`,
/// so this is what it sees of `/mcp/agent/<label>`). Percent escapes are
/// decoded; a bad one leaves the label out.
pub fn label_from_path(path: &str) -> Option<String> {
    let rest = path.trim_start_matches('/');
    let rest = rest.strip_prefix("mcp/").unwrap_or(rest);
    let label = rest.strip_prefix("agent/")?.trim_end_matches('/');
    clean_label(&percent_decode(label)?)
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = text.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// Percent-encode a label for a URL path.
pub fn encode_label(label: &str) -> String {
    label
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// Take the `agent` argument out of a tool's arguments: the translation
/// refuses arguments it does not know.
pub fn take_argument(args: &mut Value) -> Option<String> {
    let map = args.as_object_mut()?;
    match map.remove("agent")? {
        Value::String(s) => clean_label(&s),
        _ => None,
    }
}

/// The schema of the optional `agent` argument every tool takes.
pub fn argument_schema() -> Value {
    json!({
        "type": "string",
        "description": "Optional handle, the same on every call (\"kitchen\"); each subagent on one connection gives its own.",
    })
}

/// Start a call: the registry resolves the agent and refuses a stopped one,
/// or a paused one's edit or paid work (`tools::call_kind`). In-process
/// calls pass straight through.
pub(crate) fn admit(app: &AppService, ctx: &mut CallContext, tool: &str, args: &Value, kind: CallKind) -> Result<(), ToolFail> {
    let Some(hint) = &ctx.hint else {
        return Ok(());
    };
    let call = app.agent_call(hint, tool, kind)?;
    // The status line follows the tool, unless the agent wrote its own
    // with `set_status` in the last minute.
    if tool != "set_status" && !own_status_recent(&call.id) {
        if let Some(line) = tool_line(tool, args) {
            app.agent_set_status(&call, Some(line));
        }
    }
    ctx.call = Some(call);
    Ok(())
}

/// How long a status line the agent wrote itself wins over the tool's.
const OWN_STATUS_HOLD: Duration = Duration::from_secs(60);

fn own_status() -> &'static Mutex<HashMap<String, Instant>> {
    static OWN: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    OWN.get_or_init(|| Mutex::new(HashMap::new()))
}

fn own_status_recent(agent: &str) -> bool {
    let map = own_status().lock().unwrap_or_else(|e| e.into_inner());
    map.get(agent).is_some_and(|at| at.elapsed() < OWN_STATUS_HOLD)
}

fn note_own_status(agent: &str, set: bool) {
    let mut map = own_status().lock().unwrap_or_else(|e| e.into_inner());
    if set {
        map.insert(agent.to_string(), Instant::now());
    } else {
        map.remove(agent);
    }
}

/// The status line a tool call gives: "Placing doors", "Checking the model".
pub fn tool_line(tool: &str, args: &Value) -> Option<&'static str> {
    let action = args.get("action").and_then(Value::as_str);
    match tool {
        "add_window" => return Some("Placing windows"),
        "redo" => return Some("Redoing a step"),
        _ => {}
    }
    let tool = crate::toolsets::alias_of(tool).unwrap_or(tool);
    Some(match tool {
        "project" => match action {
            Some("list") => "Looking at the projects",
            Some("save_version") => "Saving a version",
            _ => "Opening the project",
        },
        "get_guide" => "Reading the guide",
        "query" => "Reading the plan",
        "look" if args.get("mode").and_then(Value::as_str) == Some("3d") => "Looking at the 3D view",
        "look" => "Looking at the plan",
        "measure" => "Measuring",
        "check" => "Checking the model",
        "build" | "batch" => "Building",
        "add_walls" => "Drawing walls",
        "add_room" => "Laying out rooms",
        "add_opening" if args.get("kind").and_then(Value::as_str) == Some("window") => "Placing windows",
        "add_opening" => "Placing doors",
        "add_stair" => "Placing stairs",
        "add_column" => "Placing columns",
        "add_slab" => "Laying slabs",
        "place_asset" => "Placing objects",
        "modify" => "Changing elements",
        "reshape" => "Reshaping walls",
        "delete" => "Removing elements",
        "levels" => "Working on the levels",
        "set_project" => "Setting up the project",
        "history" if action == Some("redo") => "Redoing a step",
        "history" if action == Some("list") => "Reading the history",
        "history" => "Undoing a step",
        "export" => "Exporting sheets",
        "edit_pipe" | "add_pipe" => "Routing pipes",
        "link_devices" => "Linking devices",
        "render" | "visualize_render" => "Rendering",
        "get_session" => "Looking at the session",
        "send_chat_message" => "Chatting",
        "set_review_mark" => "Checking the review",
        "import_underlay" | "import_dxf" => "Importing a drawing",
        _ => return None,
    })
}

/// True when `text` names the agent `name` the way the registry's notes
/// count a mention (`AppService::agent_notes`): "@Bedroom floor" or
/// "@bedroom-floor".
pub fn mentions(text: &str, name: &str) -> bool {
    let text = text.to_lowercase();
    let name = name.to_lowercase();
    !name.is_empty() && [format!("@{name}"), format!("@{}", name.replace(' ', "-"))].iter().any(|m| text.contains(m.as_str()))
}

/// After a read: the agent's pointer moves to what it looked at.
pub(crate) async fn after(app: &AppService, ctx: &CallContext, args: &Value, result: &Result<Output, ToolFail>) {
    let Some(call) = &ctx.call else { return };
    if call.kind != CallKind::Read || result.is_err() {
        return;
    }
    let mut looked: Vec<String> = vec![];
    for key in ["ids", "from", "to"] {
        match args.get(key) {
            Some(Value::Array(items)) => looked.extend(items.iter().filter_map(|v| v.as_str().map(str::to_string))),
            Some(Value::String(s)) => looked.push(s.clone()),
            _ => {}
        }
    }
    app.agent_done(call, &looked).await;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusIn {
    text: String,
    claim: Option<ClaimIn>,
    release: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimIn {
    levels: Option<Vec<String>>,
    ids: Option<Vec<String>>,
}

/// `set_status`: the agent's status line, and its claim on levels or
/// elements so other agents keep out (never people).
pub(crate) async fn set_status(app: &AppService, ctx: &CallContext, args: &Value) -> Result<Output, ToolFail> {
    let i: StatusIn = crate::translate::parse(args)?;
    let Some(call) = &ctx.call else {
        return Err(ToolFail::new("unavailable", "set_status shows on an agent's cursor; this call came from inside the app, not from an agent"));
    };
    let text = i.text.trim();
    if text.chars().count() > 80 {
        return Err(ToolFail::args("text is at most 80 characters"));
    }
    app.agent_set_status(call, (!text.is_empty()).then_some(text));
    note_own_status(&call.id, !text.is_empty());
    let mut out = json!({"ok": true, "agent": call.name, "status": text});
    if i.release == Some(true) {
        app.agent_release(call);
        out["claim"] = Value::Null;
    } else if let Some(c) = i.claim {
        let claim = app.agent_claim(call, c.levels.unwrap_or_default(), c.ids.unwrap_or_default()).await?;
        out["claim"] = json!(claim);
        out["note"] = json!("Other agents keep out of what you claimed until 10 minutes after your last call. People are never blocked.");
    }
    Ok(Output::Json(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_come_from_the_path() {
        assert_eq!(label_from_path("/agent/kitchen"), Some("kitchen".into()));
        assert_eq!(label_from_path("/mcp/agent/Claude%20one/"), Some("Claude one".into()));
        assert_eq!(label_from_path(&format!("/agent/{}", encode_label("Upper floor"))), Some("Upper floor".into()));
        assert_eq!(label_from_path("/"), None);
        assert_eq!(label_from_path("/agent/"), None);
        assert_eq!(label_from_path("/agent/%zz"), None);
    }

    #[test]
    fn the_argument_is_taken_out_and_cleaned() {
        let mut args = json!({"agent": "  Claude\u{7}  (upstairs) ", "x": 1});
        assert_eq!(take_argument(&mut args), Some("Claude (upstairs)".into()));
        assert_eq!(args, json!({"x": 1}));
        assert_eq!(take_argument(&mut json!({"agent": 3})), None);
    }
}
