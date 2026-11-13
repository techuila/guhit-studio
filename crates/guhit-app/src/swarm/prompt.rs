//! The in-app swarm's words to the model (D56): the lead's system prompt and
//! its one strict tool, `plan_swarm`; the workers' system prompt and their
//! three swarm tools; the first message of each conversation; the nearby
//! note each round carries.
//!
//! The workers' system prompt and tool list are constants, byte for byte the
//! same for every worker, so they share one cached prefix. What differs per
//! part goes into its first user message, which starts "Part: <key>" (the
//! line a test client or a dev script routes on). The copilot's
//! `ai::prompt::SYSTEM_PROMPT` says edits wait for Apply, which is not true
//! here, so workers have their own (amendments P1-7).

use serde_json::{json, Value};

/// The lead's one tool.
pub const PLAN_TOOL: &str = "plan_swarm";
/// The workers' own tools, besides the copilot's read and edit tools.
pub const MILESTONE_TOOL: &str = "swarm_milestone";
pub const STATUS_TOOL: &str = "set_status";
pub const FINISH_TOOL: &str = "finish_part";

/// Kinds a part may be given: what the copilot's edit tools draw.
pub const PLAN_KINDS: [&str; 6] = ["wall", "opening", "room", "asset", "roof", "pipe"];

pub const LEAD_SYSTEM: &str = "\
You are the lead of a swarm in Guhit Studio, a floor plan editor used by architects in the Philippines. Several drafting agents will draw one brief at the same time, each its own part, into a draft. A person watches the draft and accepts or discards it. Your only job is to split the brief into parts with the plan_swarm tool. Call plan_swarm exactly once. Do not draw anything yourself.

How to split
- One storey: split by trade. Shell draws the walls (kind wall). Rooms names the rooms the walls make and places the doors and windows (kinds room and opening), after Shell reaches its milestone \"walls\". Roof shapes the roof (kind roof) after Shell is done. Furnish places furniture and fixtures (kind asset) after Rooms is done.
- Two or more storeys: one part per level, or the trades above on each level.
- 1 to 6 parts. Use fewer parts for a small brief.
- Two parts may never draw the same kind on the same level: give each part its own levels or its own kinds. An empty list means every level or every kind.
- Rooms come from closed walls by themselves. The part that names rooms needs kind room, not wall.
- New storeys go in setup.levels, stacked on the highest level (height_mm 0 is the default 3000 mm). Parts may name them in scope.levels. Name existing levels exactly as the project context does.
- after lists what a part waits for: another part's declared milestone, or \"done\" when that part finishes. At least one part waits for nothing, and no part waits in a circle.
- milestones are the points a part reports along the way, such as \"walls\" once its outside walls close. Leave \"done\" out: it comes by itself.

Each part
- key: lowercase words joined by single hyphens, such as \"shell\" or \"upper-floor\". Never \"lead\".
- name: a short name people read, such as \"Shell\". role: one line, such as \"Walls\".
- instructions: everything its agent needs, with every size and position from the brief in millimeters (x grows east, y grows north). The agent sees the project and the other parts' work, but not this conversation. Never invent a size the brief does not give or imply; where the brief leaves one open, say what the agent should choose.

Units are millimeters. Read \"12 x 15 m lot\" as 12000 mm east-west by 15000 mm north-south. Never state or imply that a design is approved for a permit or compliant with any code.";

pub const WORKER_SYSTEM: &str = "\
You are one drafting agent of a swarm inside Guhit Studio, a floor plan editor used by architects in the Philippines. Several agents draw one brief at the same time, each its own part. Your first message says which part is yours, what to draw, where you may draw, and what the other parts do.

How your edits work
- Your edit tools draw into the swarm's draft right away, not into the model. A person watches the draft and accepts or discards it later. The tool descriptions say \"stage\"; here a call draws.
- Draw only on your part's levels and kinds. An edit outside them fails with out_of_scope: leave that work to the part that owns it.
- Each edit result lists what it added, modified or removed, with the ids of new elements; use those ids in later calls. The read tools read the draft: the model with every part's work so far.
- After tool results you may get a Nearby note: what the other parts just did. Build on their work; never redraw or delete it.
- If an edit fails, read the error and fix the call, or say so with set_status and go on with the rest.

Milestones and finishing
- When you reach one of your part's milestones (your first message names them), call swarm_milestone with its name at once: other parts wait for it.
- set_status sets the short line people see next to your cursor, such as \"Kitchen window, 1200 mm\".
- When your part is drawn, check it with the read tools, then call finish_part with a one line summary, and stop.
- Work in few, larger calls: add_wall_chain for a run of walls, and several tool calls in one turn when they do not depend on each other.

Units and facts
- Every length is millimeters. x grows east, y grows north. Angles are degrees, counter-clockwise.
- Never invent a dimension the instructions do not give or imply. Every figure you state comes from a tool result.
- Never add or delete levels: the lead set them up.
- Never state or imply that a design is approved for a permit, structurally adequate, or compliant with any code.
- Keep any text short. Nobody reads a chat here: people see your cursor, your status line and the draft.";

/// The lead's strict tool. Every field is required (strict tool use); a
/// height of 0 is the default height, and areas are not in v1 (D57).
pub fn plan_tool() -> Value {
    let obj = |properties: Value, required: &[&str]| {
        json!({"type": "object", "properties": properties, "required": required, "additionalProperties": false})
    };
    let strings = |description: &str| json!({"type": "array", "items": {"type": "string"}, "description": description});
    let level = obj(
        json!({
            "name": {"type": "string", "description": "A new level's name, such as \"Second Floor\"."},
            "height_mm": {"type": "number", "description": "Floor to floor height in millimeters; 0 for the default."},
        }),
        &["name", "height_mm"],
    );
    let scope = obj(
        json!({
            "levels": strings("Level names (or ids) the part draws on, setup levels included. Empty: every level."),
            "kinds": {"type": "array", "items": {"type": "string", "enum": PLAN_KINDS}, "description": "Kinds of element the part draws. Empty: every kind."},
        }),
        &["levels", "kinds"],
    );
    let wait = obj(
        json!({
            "part": {"type": "string", "description": "The key of the part to wait for."},
            "milestone": {"type": "string", "description": "One of its milestones, or \"done\"."},
        }),
        &["part", "milestone"],
    );
    let part = obj(
        json!({
            "key": {"type": "string"},
            "name": {"type": "string"},
            "role": {"type": "string"},
            "instructions": {"type": "string"},
            "scope": scope,
            "after": {"type": "array", "items": wait},
            "milestones": strings("Milestones this part reports along the way. Leave out \"done\"."),
        }),
        &["key", "name", "role", "instructions", "scope", "after", "milestones"],
    );
    json!({
        "name": PLAN_TOOL,
        "description": "Split the brief into parts that draw at the same time. The app checks the plan; an error comes back once to fix.",
        "strict": true,
        "input_schema": obj(
            json!({
                "summary": {"type": "string", "description": "One line, such as \"Bungalow, 12 x 15 m lot\"."},
                "setup": obj(json!({"levels": {"type": "array", "items": level}}), &["levels"]),
                "parts": {"type": "array", "items": part},
            }),
            &["summary", "setup", "parts"],
        ),
    })
}

fn strict_tool(name: &str, description: &str, field: &str, about: &str) -> Value {
    json!({
        "name": name,
        "description": description,
        "strict": true,
        "input_schema": {
            "type": "object",
            "properties": {field: {"type": "string", "description": about}},
            "required": [field],
            "additionalProperties": false,
        },
    })
}

/// The workers' tool list: the copilot's tools, then the swarm's three.
/// Order fixed, the same for every worker, so the prefix stays cached.
pub fn worker_tools() -> Vec<Value> {
    let mut tools = crate::ai::tools::definitions();
    tools.push(strict_tool(
        MILESTONE_TOOL,
        "Tell the swarm your part reached one of its milestones. Parts that wait for it start.",
        "name",
        "The milestone, as your first message names it.",
    ));
    tools.push(strict_tool(
        STATUS_TOOL,
        "Set the short status line people see next to your cursor.",
        "text",
        "At most 80 characters, such as \"Kitchen window, 1200 mm\".",
    ));
    tools.push(strict_tool(
        FINISH_TOOL,
        "Your part is drawn and checked. Ends your work; parts that wait for you start.",
        "summary",
        "One line of what your part drew.",
    ));
    tools
}

/// The first line of every conversation: what a test client or a dev
/// script routes on.
pub fn part_line(key: &str) -> String {
    format!("Part: {key}")
}

/// The lead's first message.
pub fn lead_message(brief: &str, here: Option<&str>, scope_line: Option<&str>, context: &str) -> String {
    let mut out = format!("{}\nSplit this brief into parts with {PLAN_TOOL}.\nBrief: {brief}\n", part_line(super::draft::LEAD));
    if let Some(here) = here {
        out.push_str(&format!("The level on screen: {here}.\n"));
    }
    if let Some(scope) = scope_line {
        out.push_str(&format!("{scope}\n"));
    }
    out.push('\n');
    out.push_str(context);
    out
}

/// One other part, as a worker reads it. Never "Part: <key>", so a script
/// cannot mistake one worker's message for another's.
pub struct OtherPart {
    pub name: String,
    pub key: String,
    pub role: String,
    pub scope: String,
    pub after: String,
}

/// What a worker is told about its part.
pub struct WorkerBrief<'a> {
    pub key: &'a str,
    pub name: &'a str,
    pub role: &'a str,
    pub scope: &'a str,
    pub milestones: &'a [String],
    pub instructions: &'a str,
    pub brief: &'a str,
    pub summary: &'a str,
    pub others: &'a [OtherPart],
    pub scope_line: Option<&'a str>,
    pub context: &'a str,
}

/// A worker's first message.
pub fn worker_message(w: &WorkerBrief) -> String {
    let mut out = format!("{}\nYou draw the part {} (key {}): {}.\n", part_line(w.key), w.name, w.key, w.role);
    out.push_str(&format!("Where you may draw: {}.\n", w.scope));
    if w.milestones.is_empty() {
        out.push_str("Your part has no milestones; call finish_part when it is drawn.\n");
    } else {
        out.push_str(&format!(
            "Your milestones, in order: {}. Call {MILESTONE_TOOL} with each as you reach it, then finish_part at the end.\n",
            w.milestones.join(", ")
        ));
    }
    out.push_str(&format!("The swarm's brief: {}\nThe plan: {}\n", w.brief, w.summary));
    if !w.others.is_empty() {
        out.push_str("The other parts, drawing at the same time:\n");
        for o in w.others {
            out.push_str(&format!("- {} (key {}): {}. {}.{}\n", o.name, o.key, o.role, o.scope, o.after));
        }
    }
    if let Some(scope) = w.scope_line {
        out.push_str(&format!("{scope}\n"));
    }
    out.push_str(&format!("Your instructions:\n{}\n\n{}", w.instructions.trim(), w.context));
    out
}

/// The note after a round's tool results, or None with nothing new.
pub fn nearby_text(lines: &[String]) -> Option<String> {
    (!lines.is_empty()).then(|| format!("Nearby: {}", lines.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workers_share_one_prefix() {
        assert_eq!(serde_json::to_string(&worker_tools()).unwrap(), serde_json::to_string(&worker_tools()).unwrap());
        let names: Vec<String> = worker_tools().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        for t in crate::ai::tools::READ_TOOLS.iter().chain(crate::ai::tools::EDIT_TOOLS.iter()) {
            assert!(names.iter().any(|n| n == t), "{t} is a worker tool");
        }
        assert_eq!(&names[names.len() - 3..], [MILESTONE_TOOL, STATUS_TOOL, FINISH_TOOL]);
        assert!(!WORKER_SYSTEM.contains("Apply"), "workers draw; nothing waits for Apply");
    }

    #[test]
    fn the_plan_tool_is_strict_and_every_field_is_required() {
        fn check(schema: &Value, at: &str) {
            if schema.get("type") == Some(&json!("object")) {
                assert_eq!(schema["additionalProperties"], json!(false), "{at}");
                let props = schema["properties"].as_object().unwrap();
                let required: Vec<&str> = schema["required"].as_array().unwrap().iter().map(|r| r.as_str().unwrap()).collect();
                for (k, v) in props {
                    assert!(required.contains(&k.as_str()), "{at}.{k} is required");
                    check(v, &format!("{at}.{k}"));
                }
            }
            if let Some(items) = schema.get("items") {
                check(items, &format!("{at}[]"));
            }
        }
        let tool = plan_tool();
        assert_eq!(tool["strict"], json!(true));
        check(&tool["input_schema"], "plan");
        // Every kind the tool offers is a real one.
        for k in PLAN_KINDS {
            serde_json::from_value::<guhit_model::ElementKind>(json!(k)).unwrap();
        }
    }

    #[test]
    fn a_worker_message_never_names_another_part_as_its_own() {
        let others = [OtherPart { name: "Rooms".into(), key: "rooms".into(), role: "Rooms".into(), scope: "room, opening".into(), after: String::new() }];
        let m = worker_message(&WorkerBrief {
            key: "shell",
            name: "Shell",
            role: "Walls",
            scope: "wall",
            milestones: &["walls".to_string()],
            instructions: "Draw it.",
            brief: "A house",
            summary: "A house",
            others: &others,
            scope_line: None,
            context: "<project_context></project_context>",
        });
        assert!(m.starts_with("Part: shell\n"));
        assert!(!m.to_lowercase().contains("part: rooms"));
    }
}
