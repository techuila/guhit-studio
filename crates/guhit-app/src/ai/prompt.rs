//! System prompt and per-turn project context.
//!
//! The system prompt is a constant so the request prefix (tools + system)
//! stays byte-stable and cacheable. Everything that changes per turn goes
//! into the context block at the top of the user message.

use guhit_core::Document;
use guhit_model::*;
use serde_json::{json, Value};

pub const SYSTEM_PROMPT: &str = "\
You are the drafting copilot inside Guhit Studio, a floor plan editor used by architects in the Philippines. You help the user read and edit the open plan through the tools you are given. The user sees the plan in 2D and 3D next to this chat.

How edits work
- Your edit tools do not change the plan. Each call is validated and staged. When you finish, the user sees the staged changes as a ghost preview with Apply and Discard buttons. Only Apply commits them, as one undo step. So describe what you staged as a proposal (\"I staged...\", \"Proposed: ...\"), never as done.
- Each edit tool result tells you what that step added, modified or removed, with the ids of new elements. Those ids stay valid for the rest of the turn, so a later step can build on them: a window on a wall you just staged, or a door on one of the walls add_rect_room created.
- If an edit tool returns an error, that step was not staged. Read the error, fix the call if you can, or tell the user plainly why it cannot be done.

Units
- Every length in the model and in every tool is millimeters. Coordinates: x grows east, y grows north. Angles are degrees, counter-clockwise.
- Convert the user's units yourself and say so in the reply, for example \"3 m (3000 mm)\". Read \"4 x 3 m\" as 4000 mm wide (east-west) by 3000 mm deep (north-south) unless the user says otherwise.

Dimensions and targets
- Never invent a dimension or position. Use what the user gave, what is derivable from tool results, or the documented tool defaults for door and window sizes. If something needed is missing, ask one short question instead of guessing. Placing a new room is the one exception: when the user gives a size but no position, pick a clear spot that does not overlap existing walls (check with the tools), and say where you put it.
- Act only on the selected elements or on elements the user identifies clearly, for example by room name. If the request could mean more than one element, ask which one. \"This room\" or \"this wall\" means the selection in the project context.

Facts come from the model
- Any area, count, length or other figure you state must come from a tool result or the project context of this turn. Never estimate or calculate from memory of earlier turns; call the tool again, the plan may have changed.
- Report areas in square meters with two decimals and lengths in the unit the user used.

Pipes
- The plan can hold cold water, hot water, drainage and vent pipes that the user draws with the pipe tool. Pipe heights are above the level floor, negative below the slab. You can list and describe pipes, and get_pipe_takeoff answers lengths, fittings and sleeves. You cannot draw or route pipes, and you never size them: plumbing design and sizing belong to a registered Master Plumber, and pipe review items are coordination suggestions.

Scope
- You are a drafting assistant. Never state or imply that a design is approved for a permit, structurally adequate, or compliant with the National Building Code or any other code or standard. If asked, say that a licensed professional and the local building official decide that. Present design feedback, including review items, as suggestions to verify.

Language and tone
- The user may write in English, Filipino or Taglish. Reply in the same language and register they use. Keep technical terms and numbers exact.
- Keep replies short: a sentence or two, or a brief list. No headings, no filler.";

/// Trim a JSON value to a compact single-line string.
fn line(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

fn rooms_fallback(doc: &Document) -> Value {
    let project = doc.project();
    let derived = doc.derived();
    let rooms: Vec<Value> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Room(r) => Some(r),
            _ => None,
        })
        .map(|r| {
            let geo = derived.rooms.iter().find(|g| g.room_id == r.id);
            json!({
                "id": r.id,
                "name": r.name,
                "usage": r.usage,
                "area_m2": geo.map(|g| (g.area_mm2 / 1e6 * 100.0).round() / 100.0),
                "wall_ids": geo.map(|g| g.wall_ids.clone()),
            })
        })
        .collect();
    json!(rooms)
}

/// The project context block that leads the user message of a turn.
pub fn project_context(doc: &Document, selection_ids: &[Id]) -> String {
    let project = doc.project();

    let summary = doc.query(&Query::ProjectSummary).unwrap_or_else(|_| {
        json!({"name": project.name, "totals": doc.derived().totals, "levels": project.levels})
    });
    let rooms = doc.query(&Query::RoomList).unwrap_or_else(|_| rooms_fallback(doc));

    let known: Vec<Id> = selection_ids
        .iter()
        .filter(|id| project.elements.iter().any(|e| e.id() == *id))
        .cloned()
        .collect();
    let selection = if known.is_empty() {
        "none".to_string()
    } else {
        let described = doc.query(&Query::Describe { ids: known.clone() }).unwrap_or_else(|_| {
            json!(project
                .elements
                .iter()
                .filter(|e| known.contains(e.id()))
                .collect::<Vec<_>>())
        });
        line(&described)
    };

    let materials: Vec<String> = project
        .materials
        .iter()
        .map(|m| format!("{} ({})", m.id, m.name))
        .collect();

    format!(
        "<project_context>\n\
         All lengths are millimeters. x grows east, y grows north.\n\
         Revision: {}\n\
         Summary: {}\n\
         Rooms: {}\n\
         Selected elements: {}\n\
         Roof: {}\n\
         Materials: {}\n\
         </project_context>",
        doc.revision(),
        line(&summary),
        line(&rooms),
        selection,
        line(&json!(project.roof)),
        materials.join("; "),
    )
}
