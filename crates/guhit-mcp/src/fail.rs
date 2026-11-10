//! Tool failures (MCP v2, docs/CONTRACT.md "MCP v2").
//!
//! A failed call is a tool-level error (`isError`) whose structured content
//! is `{code, message, element_ids, location_mm, step, ref, hint}`. `code` is
//! the engine's own code when the engine refused (`opening_overlap`), else
//! one of the IPC codes (`not_found`, `stale`, `out_of_scope`, ...) or
//! `bad_args`. `hint` names the tool and argument that fixes it.
//!
//! The v1 tools worded their failures as one string, "code: message". Those
//! strings are kept word for word in `text()`, so agents and tests written
//! against v1 read what they always did.

use guhit_app::ai::tools as copilot;
use guhit_core::CoreError;
use guhit_model::{Id, IpcError};
use serde_json::{json, Value};

/// A tool call that did not work.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToolFail {
    pub code: String,
    /// What went wrong, without the code in front.
    pub message: String,
    pub element_ids: Vec<Id>,
    /// Where it went wrong, `{x, y}` (and `z` for pipes), mm.
    pub location_mm: Option<Value>,
    /// 1-based step of a `batch` or `build`.
    pub step: Option<usize>,
    /// The `ref` of that step, when it had one.
    pub r#ref: Option<String>,
    /// Which tool and argument fix it.
    pub hint: Option<String>,
    /// The v1 wording, kept as it was.
    legacy: Option<String>,
}

impl ToolFail {
    /// A failure with a code and a message.
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.to_string(), message: message.into(), ..Self::default() }
    }

    /// Bad arguments: the caller can fix them.
    pub fn args(message: impl Into<String>) -> Self {
        Self::new("bad_args", message)
    }

    /// A v1 message, "code: message". The code is read off the front; the
    /// whole string stays the text. "step 2 of 5 (`tool`): ..." keeps its step.
    pub fn msg(text: impl Into<String>) -> Self {
        let text = text.into();
        let mut out = parse(&text);
        out.legacy = Some(text);
        out
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn with_ids(mut self, ids: Vec<Id>) -> Self {
        self.element_ids = ids;
        self
    }

    pub fn at_step(mut self, step: usize, r#ref: Option<&str>) -> Self {
        self.step = Some(step);
        self.r#ref = r#ref.map(str::to_string);
        self
    }

    /// Replace the one-line text, keeping the structured fields.
    pub fn reworded(mut self, text: String) -> Self {
        self.legacy = Some(text);
        self
    }

    /// The one-line text: the v1 wording when there is one, else
    /// "code: message (elements: ...)". No hint: `display` adds it.
    pub fn text(&self) -> String {
        if let Some(t) = &self.legacy {
            return t.clone();
        }
        let ids = if self.element_ids.is_empty() {
            String::new()
        } else {
            format!(" (elements: {})", self.element_ids.join(", "))
        };
        let step = match (self.step, &self.r#ref) {
            (Some(n), Some(r)) => format!("step {n} (ref `{r}`): "),
            (Some(n), None) => format!("step {n}: "),
            _ => String::new(),
        };
        format!("{step}{}: {}{ids}", self.code, self.message)
    }

    /// What the model reads: the text and the hint.
    pub fn display(&self) -> String {
        match self.hint() {
            Some(h) => format!("{} Hint: {h}", self.text()),
            None => self.text(),
        }
    }

    /// The hint given, or the standard one for the code, or the general one:
    /// every failure says how to fix it (Wave 2 rule).
    pub fn hint(&self) -> Option<String> {
        self.hint
            .clone()
            .or_else(|| standard_hint(&self.code).map(str::to_string))
            .or_else(|| Some(GENERAL_HINT.to_string()))
    }

    /// The structured content of the error result.
    pub fn structured(&self) -> Value {
        json!({
            "code": self.code,
            "message": self.message,
            "element_ids": self.element_ids,
            "location_mm": self.location_mm,
            "step": self.step,
            "ref": self.r#ref,
            "hint": self.hint(),
        })
    }
}

impl std::fmt::Display for ToolFail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display())
    }
}

/// Read "code: message" and "step N of M (`tool`): ..." off a v1 string.
fn parse(text: &str) -> ToolFail {
    if let Some(rest) = text.strip_prefix("step ") {
        if let Some((head, tail)) = rest.split_once("): ") {
            let n = head.split(' ').next().and_then(|n| n.parse::<usize>().ok());
            if let Some(n) = n {
                let mut inner = parse(tail.trim_end_matches(" Nothing was applied."));
                inner.step = Some(n);
                return inner;
            }
        }
    }
    if let Some(rest) = text.strip_prefix("invalid arguments: ") {
        return ToolFail::args(rest);
    }
    if let Some((head, tail)) = text.split_once(": ") {
        if !head.is_empty() && head.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            let (message, ids) = split_ids(tail);
            return ToolFail::new(head, message).with_ids(ids);
        }
    }
    ToolFail::new("failed", text)
}

/// "message (elements: a, b)" into the message and the ids.
fn split_ids(text: &str) -> (String, Vec<Id>) {
    if let Some(at) = text.rfind(" (elements: ") {
        let ids = text[at + 12..].trim_end_matches(')');
        return (text[..at].to_string(), ids.split(", ").map(str::to_string).collect());
    }
    (text.to_string(), vec![])
}

impl From<copilot::ToolError> for ToolFail {
    fn from(e: copilot::ToolError) -> Self {
        Self::msg(e.0)
    }
}

/// The engine's specific code survives: `opening_overlap`, not `invalid`.
impl From<CoreError> for ToolFail {
    fn from(e: CoreError) -> Self {
        let code = e.code().to_string();
        let ipc = IpcError::from(e);
        let mut out = ToolFail::new(&code, strip_step_prefix(&ipc.message)).with_ids(ipc.element_ids.clone());
        // The v1 wording said "invalid: ..." for every engine refusal.
        out.legacy = Some(legacy_ipc(&ipc));
        out
    }
}

impl From<IpcError> for ToolFail {
    fn from(e: IpcError) -> Self {
        let mut out = ToolFail::new(&e.code, strip_step_prefix(&e.message)).with_ids(e.element_ids.clone());
        out.legacy = Some(legacy_ipc(&e));
        out
    }
}

fn legacy_ipc(e: &IpcError) -> String {
    let ids = if e.element_ids.is_empty() {
        String::new()
    } else {
        format!(" (elements: {})", e.element_ids.join(", "))
    };
    format!("{}: {}{}", e.code, strip_step_prefix(&e.message), ids)
}

/// Drop the engine's own "Step 2 of 5: " prefix. This server always commits a
/// `Command::Batch`, even for one tool call, so that prefix counts internal
/// leaf commands the caller never wrote. The caller's own step goes in
/// `step` instead.
pub fn strip_step_prefix(message: &str) -> String {
    let Some(rest) = message.strip_prefix("Step ") else {
        return message.to_string();
    };
    let Some((head, tail)) = rest.split_once(": ") else {
        return message.to_string();
    };
    let counts: Vec<&str> = head.split(" of ").collect();
    let numeric = counts.len() == 2
        && counts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    if numeric {
        tail.to_string()
    } else {
        message.to_string()
    }
}

/// The hint of a failure whose code has no hint of its own.
pub const GENERAL_HINT: &str = "The message names what to change: fix that argument and call again. tools/list has each tool's arguments; get_guide topic \"workflows\" has examples.";

/// The hint for a code, naming the tool and argument that fixes it.
pub fn standard_hint(code: &str) -> Option<&'static str> {
    Some(match code {
        // Wave 2 (D53, D54).
        "not_built" => "That part of Guhit is not built in this version yet; the message says which. Do the rest, and tell the user what is missing.",
        "forbidden" => "Only people may do that. Ask the user to do it in the app.",
        "roof_outline" => "add_roof: square every corner (90 or 270 degrees); a hip takes 4, 6 or 8 corners, gable and shed 4; gable_sides only on a rectangle. Or use shape \"flat\".",
        "roof_level" => "add_roof: the roof over the top floor (outline top_footprint) takes no level; footprint and polygon roofs take one.",
        "bad_roof" => "add_roof: pitch_deg, overhang_mm and thickness_mm in range (the message says which).",
        "lot_exists" => "A project has one lot: set_lot replaces it.",
        "tour_fps" | "tour_shot" => "edit_tour: fps 24, 30 or 60; each shot 0.1 to 600 s; a transition 0 to 10 s.",
        "claimed" => "Another agent claimed that level or element: work elsewhere, or ask it with send_chat_message. get_session lists the claims.",
        "rate_limited" => "Slow down: wait a second, or put the edits in one batch or build.",
        "too_many_agents" => "At most 8 agents work on one computer at once. Ask the user to stop one in the Agents panel.",
        "project_changed" => "The user opened another project. Read it with query before editing.",
        "window_busy" => "Wait for the window's render (render action \"status\"), then call again.",
        "io" => "Try again once; if it repeats, tell the user what the message says.",
        "bad_args" => "Check the arguments against this tool's input schema. get_guide topic \"workflows\" has examples.",
        "not_found" => "Find ids with query (what \"elements\", with kind and level), or levels with query what \"levels\".",
        "no_document" => "Open or create a project first: project action \"open\" or \"create\".",
        "stale" => "The plan changed while this call ran. Read it again with query and repeat the call.",
        "out_of_scope" => "Change only what is in scope (query what \"selection\" shows it), or ask the user to widen the selection.",
        "live_session" => "Ask the user first, then call again with force true.",
        "other_author" => "Someone else made that step. Ask the user, then history action \"undo\" with force true.",
        // Swarms (D56, D57): their own advice, never the live session's.
        "swarm_running" => "A swarm draft is open, so the model waits: a person accepts or discards it in Guhit Studio. swarm action \"status\" shows it. A sub-agent drawing a part passes its handle as agent.",
        "draft_open" => "One swarm at a time: a person accepts or discards the open draft in Guhit Studio first. swarm action \"status\" shows it.",
        "part_stopped" => "Stop working on this part and tell the user. Its steps stay in the draft for a person to accept or discard.",
        "draft_view" => "In a swarm draft, check your work with look (plan, elevation, section, sheet, schedule), query, measure and check. 3D, renders, exports and versions show the model: they work once a person accepts the draft.",
        "people_only" => "Only a person accepts a swarm draft. Tell the user it is ready and ask them to press Accept in Guhit Studio.",
        "needs_force" => "Ask the user first; call again with force true only after they agree.",
        "no_window" => "Ask the user to open the Guhit Studio window on this project. look mode \"plan\" works without a window.",
        "not_live" => "There is no live session. The user starts one with Share in the app.",
        "unknown_level" | "bad_level" => "levels action \"list\" shows the levels; levels action \"add\" makes one.",
        "unknown_material" => "query what \"materials\" lists the material ids.",
        "unknown_wall" | "not_a_wall" => "Give add_opening `at` {x, y} and the engine finds the wall, or find wall ids with query what \"elements\" kind \"wall\".",
        "opening_outside_wall" | "opening_overlap" | "opening_too_tall" | "opening_size" => {
            "Move it along the wall (add_opening offset_mm or at), make it smaller (width_mm, height_mm), or use a longer wall."
        }
        "wall_too_short" | "chain_too_short" => "Check the points of add_walls: walls need two points far enough apart.",
        "wall_thickness" => "add_walls thickness_mm is 50 to 1000.",
        "wall_height" => "add_walls height_mm is 100 to 20000.",
        "level_elevation_taken" => "Give levels elevation_mm another value, or change the level with levels action \"update\".",
        "last_level" => "A project keeps at least one level.",
        "room_exists" => "The walls are already there: add_room with walls false names the room they enclose.",
        "no_room_here" => "Close the outline with add_walls first, or call add_room with walls true.",
        "stretch_turns_wall" => "Move the named wall as well (modify ids), or pass stretch_connected false.",
        "resize_breaks_room" => "Use a smaller delta_mm in reshape op \"resize_room\".",
        "layer_locked" | "underlay_locked" => "Ask the user to unlock that layer in the app.",
        "stair_size" => "add_stair width_mm is 300 to 10000, run_mm 300 to 30000, riser_count 2 to 100 (4 or more for shape l or u, with first_flight_risers 2 to n - 2).",
        "column_size" => "add_column width_mm and depth_mm are 50 to 5000.",
        "slab_outline" | "slab_thickness" | "railing_height" => {
            "add_slab needs 3 or more points, thickness_mm 50 to 1000 (not for a void) and a railing 300 to 2000 mm high."
        }
        "unknown_tool" => "tools/list shows the tools. get_guide topic \"workflows\" shows which tool does what.",
        "unknown_ref" => "A \"$name\" placeholder names the ref of an earlier step. Give that step a ref, or use the id.",
        "unknown_catalog_key" => "query what \"catalog\" with search lists the keys.",
        "unavailable" => "This server version cannot do that yet. The message says what can.",
        "toolset_off" => "Add the toolset to the MCP URL: /mcp?toolsets=...",
        "agent_paused" | "agent_stopped" => "The user paused or stopped this agent in the Agents panel. Tell them and wait.",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_strings_keep_their_text_and_gain_a_code() {
        let f = ToolFail::msg("not_found: no element has id `w9`");
        assert_eq!(f.code, "not_found");
        assert_eq!(f.text(), "not_found: no element has id `w9`");
        assert!(f.hint().unwrap().contains("query"));

        let f = ToolFail::msg("invalid arguments: `steps` must be an array");
        assert_eq!(f.code, "bad_args");
        assert_eq!(f.message, "`steps` must be an array");

        let f = ToolFail::msg("step 5 of 6 (`add_door`): invalid: The door does not fit. (elements: d1, w2) Nothing was applied.");
        assert_eq!(f.step, Some(5));
        assert_eq!(f.code, "invalid");
        assert_eq!(f.element_ids, vec!["d1".to_string(), "w2".to_string()]);
        assert!(f.text().starts_with("step 5 of 6"));
    }

    #[test]
    fn the_structured_error_has_every_field() {
        let f = ToolFail::new("opening_overlap", "The door would overlap a window.")
            .with_ids(vec!["d1".into()])
            .at_step(3, Some("front_door"));
        let v = f.structured();
        for key in ["code", "message", "element_ids", "location_mm", "step", "ref", "hint"] {
            assert!(v.get(key).is_some(), "{key} missing");
        }
        assert_eq!(v["step"], 3);
        assert_eq!(v["ref"], "front_door");
        assert!(v["hint"].as_str().unwrap().contains("add_opening"));
        assert_eq!(f.text(), "step 3 (ref `front_door`): opening_overlap: The door would overlap a window. (elements: d1)");
    }

    #[test]
    fn the_engine_step_prefix_is_removed() {
        assert_eq!(strip_step_prefix("Step 1 of 1: The door does not fit."), "The door does not fit.");
        assert_eq!(strip_step_prefix("Stepping stone: no"), "Stepping stone: no");
        assert_eq!(strip_step_prefix("Step one of two: no"), "Step one of two: no");
    }
}
