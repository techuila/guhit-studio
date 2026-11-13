//! The in-app lead (D56): one model call with one strict tool,
//! `plan_swarm`, the pattern of furnishing's `choose_furniture`. Rust
//! checks the plan (`validate_plan`, plus the places free among the
//! agents); a plan that fails gets one retry with the error, then the
//! swarm fails clearly. The lead never draws: setup levels are written by
//! the orchestrator from the plan, not by the model.

use std::sync::Arc;

use guhit_model::*;
use serde_json::{json, Value};

use super::orchestrate::{Halt, Run};
use super::prompt;
use crate::ai::client::{ModelClient, ModelRequest, ModelResponse};
use crate::AppService;

/// Plans asked for: the first, and one retry with the error.
pub const PLAN_TRIES: usize = 2;
const LEAD_MAX_TOKENS: u32 = 8192;

/// The plan in a lead's response, as `SwarmPlan`, or what is wrong with it
/// in words the model can act on. A height of 0 is the default height.
pub fn read_plan(response: &ModelResponse) -> Result<SwarmPlan, String> {
    if response.stop_reason == "refusal" {
        return Err("The AI service declined to plan this brief.".into());
    }
    let Some(call) = response.tool_uses().into_iter().find(|c| c.name == prompt::PLAN_TOOL) else {
        return Err(format!("You did not call {}. Call it once with the whole plan.", prompt::PLAN_TOOL));
    };
    let mut plan: SwarmPlan =
        serde_json::from_value(call.input).map_err(|e| format!("The plan does not match the {} schema: {e}.", prompt::PLAN_TOOL))?;
    for level in &mut plan.setup.levels {
        if level.height_mm.is_some_and(|h| h == 0.0) {
            level.height_mm = None;
        }
    }
    Ok(plan)
}

/// Ask the lead for a plan that `check` accepts. Every request goes
/// through the run (pause, cap, the gate, the spend). Err: the swarm
/// ended meanwhile, or why the plan failed twice.
pub(crate) async fn plan(
    app: &AppService,
    run: &Arc<Run>,
    client: &dyn ModelClient,
    first_message: String,
    check: impl Fn(&SwarmPlan) -> Result<(), IpcError>,
) -> Result<SwarmPlan, Halt> {
    let mut request = ModelRequest {
        model: run.model.clone(),
        system: prompt::LEAD_SYSTEM.to_string(),
        messages: vec![json!({"role": "user", "content": first_message})],
        tools: vec![prompt::plan_tool()],
        max_tokens: LEAD_MAX_TOKENS,
    };
    let mut why = String::new();
    for attempt in 1..=PLAN_TRIES {
        let answer = run.send(app, super::draft::LEAD, None, client, &request, attempt).await?;
        let response = answer.response;
        let outcome = read_plan(&response).and_then(|plan| match check(&plan) {
            Ok(()) => Ok(plan),
            Err(e) => Err(e.message),
        });
        match outcome {
            Ok(plan) => {
                run.log(app, "swarm_plan", json!({"attempt": attempt, "ok": true, "parts": plan.parts.len()})).await;
                return Ok(plan);
            }
            Err(e) => {
                run.log(app, "swarm_plan", json!({"attempt": attempt, "ok": false, "error": e})).await;
                why = e;
            }
        }
        if response.stop_reason == "refusal" {
            break;
        }
        // The retry: the answer as it came, then the error, as the tool's
        // result when it called the tool.
        let uses = response.tool_uses();
        request.messages.push(json!({"role": "assistant", "content": response.content}));
        let reply: Value = if uses.is_empty() {
            json!(why)
        } else {
            Value::Array(
                uses.iter()
                    .map(|u| {
                        let text = if u.name == prompt::PLAN_TOOL { why.clone() } else { format!("Unknown tool `{}`.", u.name) };
                        json!({"type": "tool_result", "tool_use_id": u.id, "content": text, "is_error": true})
                    })
                    .collect(),
            )
        };
        request.messages.push(json!({"role": "user", "content": reply}));
    }
    Err(Halt::Failed(format!("The lead could not split the brief: {why}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::client::ScriptedClient;

    #[test]
    fn a_plan_reads_from_the_tool_call() {
        let input = json!({
            "summary": "A house",
            "setup": {"levels": [{"name": "Second Floor", "height_mm": 0}]},
            "parts": [{"key": "shell", "name": "Shell", "role": "Walls", "instructions": "Draw.",
                "scope": {"levels": [], "kinds": ["wall"]}, "after": [], "milestones": ["walls"]}],
        });
        let plan = read_plan(&ScriptedClient::tools(&[(prompt::PLAN_TOOL, input)])).unwrap();
        assert_eq!(plan.parts[0].scope.kinds, vec![ElementKind::Wall]);
        assert_eq!(plan.setup.levels[0].height_mm, None, "0 is the default height");
        assert!(read_plan(&ScriptedClient::text("Here is my plan")).unwrap_err().contains("did not call"));
        let bad = ScriptedClient::tools(&[(prompt::PLAN_TOOL, json!({"summary": 3}))]);
        assert!(read_plan(&bad).unwrap_err().contains("schema"));
    }
}
