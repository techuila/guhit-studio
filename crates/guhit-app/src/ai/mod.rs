//! AI copilot. Owned by the AI agent.
//!
//! Pipeline (research section 9.3): intent -> plan (tool calls) -> validate
//! -> preview -> commit on user approval -> evaluate (log).
//!
//! Trust rules this module enforces:
//! - The model never mutates the document. Edit tools stage typed `Command`s
//!   on one working copy (`guhit_core::Staging`), each call run once, and the
//!   whole batch is validated with `Document::preview` before it is proposed.
//! - Nothing commits without `ai_resolve(accept = true)`, and only when the
//!   document is still at the revision the proposal was made on.
//! - A commit goes through `AppService::commit`, the same path as user edits,
//!   as one `Batch`, so one undo reverts it.
//! - With "Only the selection" on (DECISIONS D30), every edit call is checked
//!   with `guhit_core::scope` and one that reaches outside is not staged.
//! - The API key lives in the OS keychain and never crosses IPC.

pub mod client;
pub mod furnish;
pub mod keys;
pub mod log;
pub mod prompt;
pub mod tools;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use guhit_core::{CoreError, Document, Staging};
use guhit_model::*;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::{arg, to_value, AppService, IpcResult};
use client::{AnthropicClient, ModelClient, ModelRequest};
use keys::{FileStore, KeyStore, KeychainStore};

/// Default model. Configurable through `AiSettings.model`.
pub const DEFAULT_MODEL: &str = "claude-opus-5";
/// Hard cap on model round trips per chat turn.
pub const MAX_ROUNDS: usize = 8;
const MAX_TOKENS: u32 = 4096;
/// Prior turns sent with a request. Older ones are dropped.
const MAX_HISTORY: usize = 24;
const SETTINGS_FILE: &str = "settings.json";
const SETTINGS_MODEL_KEY: &str = "ai_model";

struct Pending {
    proposal: AiProposal,
    /// Project the proposal belongs to. A revision number alone could match
    /// by accident after another project is opened.
    project_id: Id,
    log_dir: Option<PathBuf>,
}

pub struct AiState {
    /// At most one proposal waits for the user at any time.
    pending: Mutex<Option<Pending>>,
    /// One chat turn at a time.
    chat_gate: Mutex<()>,
    keys: Box<dyn KeyStore>,
    /// Set by tests. When None the real Claude client is used.
    client: Option<Arc<dyn ModelClient>>,
}

impl Default for AiState {
    fn default() -> Self {
        Self {
            pending: Mutex::new(None),
            chat_gate: Mutex::new(()),
            keys: Box::new(KeychainStore),
            client: None,
        }
    }
}

impl AiState {
    /// State with an injected model client and key store.
    pub fn with_parts(client: Arc<dyn ModelClient>, keys: Box<dyn KeyStore>) -> Self {
        Self { client: Some(client), keys, ..Self::default() }
    }

    /// State with an injected key store and the real client.
    pub fn with_keys(keys: Box<dyn KeyStore>) -> Self {
        Self { keys, ..Self::default() }
    }

    /// The production state: key in a private file under `data_dir`.
    pub fn for_data_dir(data_dir: &std::path::Path) -> Self {
        Self::with_keys(Box::new(FileStore::new(data_dir)))
    }
}

/// Handles every command whose name starts with `ai_`.
pub async fn handle(app: &AppService, cmd: &str, args: Value) -> IpcResult {
    match cmd {
        "ai_settings_get" => to_value(&settings_get(app).await),
        "ai_settings_set" => {
            let api_key: Option<String> = arg(&args, "api_key")?;
            let model: Option<String> = arg(&args, "model")?;
            to_value(&settings_set(app, api_key, model).await?)
        }
        "ai_chat" => {
            let request: AiRequest = arg(&args, "request")?;
            to_value(&chat(app, request).await?)
        }
        "ai_resolve" => {
            let proposal_id: Id = arg(&args, "proposal_id")?;
            let accept: bool = arg(&args, "accept")?;
            to_value(&resolve(app, &proposal_id, accept).await?)
        }
        _ => Err(IpcError::new("unknown_command", format!("unknown command `{cmd}`"))),
    }
}

// ------------------------------------------------------------------- settings

async fn data_dir(app: &AppService) -> PathBuf {
    app.session.lock().await.data_dir.clone()
}

fn read_settings(dir: &std::path::Path) -> serde_json::Map<String, Value> {
    std::fs::read_to_string(dir.join(SETTINGS_FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

async fn model_name(app: &AppService) -> String {
    read_settings(&data_dir(app).await)
        .get(SETTINGS_MODEL_KEY)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or(DEFAULT_MODEL)
        .to_string()
}

async fn settings_get(app: &AppService) -> AiSettings {
    AiSettings { has_api_key: app.ai.keys.get().is_some(), model: model_name(app).await }
}

async fn settings_set(
    app: &AppService,
    api_key: Option<String>,
    model: Option<String>,
) -> Result<AiSettings, IpcError> {
    if let Some(key) = api_key {
        let key = key.trim();
        if key.is_empty() {
            app.ai.keys.remove()?;
        } else {
            app.ai.keys.set(key)?;
        }
    }
    if let Some(model) = model {
        let model = model.trim();
        if model.chars().count() > 100 || model.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(IpcError::new("bad_args", "model name is not valid"));
        }
        let dir = data_dir(app).await;
        // Merge: other parts of the app keep their own keys in this file.
        let mut settings = read_settings(&dir);
        if model.is_empty() {
            settings.remove(SETTINGS_MODEL_KEY);
        } else {
            settings.insert(SETTINGS_MODEL_KEY.into(), json!(model));
        }
        let text = serde_json::to_string_pretty(&Value::Object(settings))
            .map_err(|e| IpcError::new("io", e.to_string()))?;
        std::fs::create_dir_all(&dir).map_err(|e| IpcError::new("io", e.to_string()))?;
        // Atomic: temp file, then rename over the old one.
        let tmp = dir.join(format!("{SETTINGS_FILE}.tmp"));
        std::fs::write(&tmp, text).map_err(|e| IpcError::new("io", e.to_string()))?;
        std::fs::rename(&tmp, dir.join(SETTINGS_FILE)).map_err(|e| IpcError::new("io", e.to_string()))?;
    }
    Ok(settings_get(app).await)
}

// ----------------------------------------------------------------------- chat

/// Pick the model client for one chat turn.
fn model_client(app: &AppService, user_message: &str) -> Result<Arc<dyn ModelClient>, IpcError> {
    if let Some(client) = &app.ai.client {
        return Ok(client.clone());
    }
    // Debug builds only: GUHIT_AI_SCRIPT points at a JSON script that stands
    // in for the model, so the whole pipeline can be driven from the dev
    // bridge without a key. See `ScriptedClient::from_script`.
    #[cfg(debug_assertions)]
    if let Ok(path) = std::env::var("GUHIT_AI_SCRIPT") {
        let script = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .ok_or_else(|| IpcError::new("ai_failed", "dev script: file is missing or not JSON"))?;
        return Ok(Arc::new(client::ScriptedClient::from_script(&script, user_message)?));
    }
    let _ = user_message;
    let key = app.ai.keys.get().ok_or_else(|| {
        IpcError::new(
            "ai_not_configured",
            "No API key is set. Add a Claude API key in the copilot settings.",
        )
    })?;
    Ok(Arc::new(AnthropicClient::new(key)?))
}

fn no_document() -> IpcError {
    IpcError::new("no_document", "no project is open")
}

fn batch_label(message: &str) -> String {
    let flat: String = message.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = flat.chars().take(48).collect();
    if flat.chars().count() > 48 {
        format!("AI: {}...", short.trim_end())
    } else {
        format!("AI: {short}")
    }
}

fn batch(label: &str, commands: &[Command]) -> Command {
    Command::Batch { label: label.to_string(), commands: commands.to_vec() }
}

/// The calls of this turn staged so far, on one working copy of the plan.
struct Staged {
    staging: Staging,
    /// The revision of the plan the staging started from.
    revision: u32,
}

/// What the staging of one edit call produced.
struct StagedStep {
    step: Value,
    summary: String,
}

/// Told to the model after every edit the scope refuses.
const SCOPE_HINT: &str = "This turn is limited to the user's selection. Change only the selected elements and what stands in a selected room, or tell the user what else would need to change.";

/// An `out_of_scope` refusal as the model reads it: the engine's sentence,
/// then what to do. Any other error reads as usual.
fn refused(e: CoreError) -> tools::ToolError {
    match e {
        CoreError::Invalid {
            code,
            message,
            element_ids,
        } if code == "out_of_scope" => {
            let ids = if element_ids.is_empty() {
                String::new()
            } else {
                format!(" (elements: {})", element_ids.join(", "))
            };
            tools::ToolError(format!("out_of_scope: {message} {SCOPE_HINT}{ids}"))
        }
        other => other.into(),
    }
}

/// Translate one edit call, append it to the staged commands and validate
/// it on the staged plan. With a scope, the call must first stay inside the
/// selection on the staged view. On any error nothing is staged.
///
/// Each call runs once on the working copy, so a turn of N calls costs N
/// calls. When the plan changed since the staging started (the user edited
/// while the model worked), everything is staged again on the plan as it is
/// now, which is what validating the whole batch on it used to do.
#[allow(clippy::too_many_arguments)]
fn stage_edit(
    doc: &Document,
    label: &str,
    commands: &[Command],
    staged: &mut Option<Staged>,
    scope: Option<&[Id]>,
    name: &str,
    input: &Value,
    level_id: &Id,
) -> Result<(Vec<Command>, StagedStep), tools::ToolError> {
    let view = match staged.as_ref() {
        Some(st) => st.staging.project(),
        None => doc.project(),
    };
    let new_commands = tools::to_commands(view, name, input, level_id)?;
    // Only people set license numbers (D53).
    for c in &new_commands {
        crate::people_only(c).map_err(|e| tools::ToolError(format!("{}: {}", e.code, e.message)))?;
    }
    if let Some(selection) = scope {
        let staged_derived;
        let (view_derived, made) = match staged.as_ref() {
            Some(st) => {
                staged_derived = st.staging.derived();
                // What earlier calls of this turn made is in reach, so this
                // call can build on it: a window on a wall staged a step ago.
                (&staged_derived, st.staging.diff().added)
            }
            None => (doc.derived(), vec![]),
        };
        let call = match new_commands.as_slice() {
            [one] => one.clone(),
            many => batch(label, many),
        };
        guhit_core::scope::check_staged(view, view_derived, selection, &made, &call).map_err(refused)?;
    }
    let moved_before = staged.as_ref().map(|st| st.staging.also_moved().len()).unwrap_or(0);
    let mut step = match staged.as_mut() {
        Some(st) if st.revision == doc.revision() => {
            let before = st.staging.push(&new_commands)?;
            tools::step_diff(&before, st.staging.project())
        }
        _ => {
            let mut all = commands.to_vec();
            all.extend(new_commands.iter().cloned());
            let mut fresh = Staging::new(doc.project(), label);
            fresh.push(&all)?;
            let view = match staged.as_ref() {
                Some(st) => st.staging.project(),
                None => doc.project(),
            };
            let step = tools::step_diff(view, fresh.project());
            *staged = Some(Staged { staging: fresh, revision: doc.revision() });
            step
        }
    };
    let summary = staged.as_ref().map(|st| st.staging.diff().summary).unwrap_or_default();
    // Walls this call reshaped without naming them.
    if let Some(st) = staged.as_ref() {
        let new = st.staging.also_moved().get(moved_before..).unwrap_or_default();
        if !new.is_empty() {
            step["also_moved"] = tools::also_moved_list(st.staging.project(), new);
        }
    }
    Ok((new_commands, StagedStep { step, summary }))
}

/// The level new elements go on: the one the user is working on, or the first
/// level. An id that is not in the project is a caller mistake, not a default.
fn active_level(project: &Project, requested: &Option<Id>) -> Result<Id, IpcError> {
    match requested {
        Some(id) => {
            if project.levels.iter().any(|l| &l.id == id) {
                Ok(id.clone())
            } else {
                let known: Vec<&str> = project.levels.iter().map(|l| l.id.as_str()).collect();
                Err(IpcError::new(
                    "not_found",
                    format!("no level has id `{id}`. Levels in this project: {}", known.join(", ")),
                ))
            }
        }
        None => project
            .levels
            .first()
            .map(|l| l.id.clone())
            .ok_or_else(|| IpcError::new("invalid", "the project has no level")),
    }
}

async fn chat(app: &AppService, request: AiRequest) -> Result<AiTurn, IpcError> {
    // A task with its own pipeline (D54): furnishing is lane L5's.
    if let Some(AiTask::Furnish { room_ids, style }) = &request.task {
        return furnish::run(app, &request, room_ids, style).await;
    }
    let message = request.message.trim().to_string();
    if message.is_empty() {
        return Err(IpcError::new("bad_args", "message is empty"));
    }
    // DECISIONS D30: every edit of this turn stays inside the selection.
    let scope: Option<&[Id]> = request.scope.as_ref().map(|s| s.ids.as_slice());
    if scope.is_some_and(|ids| ids.is_empty()) {
        return Err(IpcError::new(
            "bad_args",
            "scope has no ids: limit the turn to a selection, or send no scope",
        ));
    }
    let _turn = app.ai.chat_gate.lock().await;
    let started = Instant::now();
    let log_dir = app.project_dir().await;

    // A new turn always discards the proposal of the previous one.
    if let Some(old) = app.ai.pending.lock().await.take() {
        log::append(
            old.log_dir.as_deref(),
            "outcome",
            json!({"proposal_id": old.proposal.id, "outcome": "superseded"}),
        )
        .await;
    }
    // And the rooms a furnishing turn still has waiting.
    furnish::discard(app);

    let (context, level_id) = {
        let s = app.session.lock().await;
        let doc = s.doc.as_ref().ok_or_else(no_document)?;
        let level_id = active_level(doc.project(), &request.active_level_id)?;
        if let Some(ids) = scope {
            guhit_core::scope::validate(doc.project(), ids)?;
        }
        (prompt::project_context(doc, &request.selection_ids, scope), level_id)
    };
    let client = model_client(app, &message)?;
    let model = model_name(app).await;
    let turn_id = defaults::new_id();
    log::append(
        log_dir.as_deref(),
        "intent",
        json!({
            "turn_id": turn_id,
            "message": message,
            "selection_ids": request.selection_ids,
            "scope_ids": scope,
            "level_id": level_id,
            "history_len": request.history.len(),
            "model": model,
        }),
    )
    .await;

    let mut messages: Vec<Value> = request
        .history
        .iter()
        .skip(request.history.len().saturating_sub(MAX_HISTORY))
        .filter(|m| !m.text.trim().is_empty())
        // The API needs the first message to come from the user.
        .skip_while(|m| m.role != AiRole::User)
        .map(|m| {
            let role = if m.role == AiRole::User { "user" } else { "assistant" };
            json!({"role": role, "content": m.text})
        })
        .collect();
    messages.push(json!({"role": "user", "content": format!("{context}\n\n{message}")}));

    let mut model_request = ModelRequest {
        model,
        system: prompt::SYSTEM_PROMPT.to_string(),
        messages,
        tools: tools::definitions(),
        max_tokens: MAX_TOKENS,
    };

    let label = batch_label(&message);
    let mut commands: Vec<Command> = vec![];
    let mut staged: Option<Staged> = None;
    let mut tools_used: Vec<String> = vec![];
    let mut reply = String::new();
    let mut rounds = 0;
    let mut hit_cap = false;

    loop {
        rounds += 1;
        let round_started = Instant::now();
        let response = match client.send(&model_request).await {
            Ok(r) => r,
            Err(e) => {
                log::append(
                    log_dir.as_deref(),
                    "error",
                    json!({"turn_id": turn_id, "round": rounds, "code": e.code, "message": e.message}),
                )
                .await;
                return Err(e);
            }
        };
        let model_ms = round_started.elapsed().as_millis() as u64;
        let text = response.text();
        if !text.is_empty() {
            reply = text;
        }

        if response.stop_reason == "refusal" {
            commands.clear();
            reply = "The AI service declined this request. Try rephrasing it.".to_string();
            break;
        }
        let calls = response.tool_uses();
        if response.stop_reason != "tool_use" || calls.is_empty() {
            if response.stop_reason == "max_tokens" && reply.is_empty() {
                reply = "The reply was cut off before it finished. Try a shorter request.".to_string();
            }
            break;
        }
        if rounds >= MAX_ROUNDS {
            // Out of rounds with calls still pending: they are not run.
            hit_cap = true;
            break;
        }

        let mut results: Vec<Value> = vec![];
        for call in &calls {
            let call_started = Instant::now();
            if !tools_used.contains(&call.name) {
                tools_used.push(call.name.clone());
            }
            let outcome: Result<Value, tools::ToolError> = {
                let s = app.session.lock().await;
                match s.doc.as_ref() {
                    None => Err(tools::ToolError("no_document: no project is open".into())),
                    Some(doc) if tools::is_edit(&call.name) => stage_edit(
                        doc,
                        &label,
                        &commands,
                        &mut staged,
                        scope,
                        &call.name,
                        &call.input,
                        &level_id,
                    )
                    .map(|(new_commands, done)| {
                        commands.extend(new_commands);
                        json!({
                            "staged": true,
                            "staged_steps": commands.len(),
                            "this_step": done.step,
                            "proposal_so_far": done.summary,
                        })
                    }),
                    Some(doc) if tools::is_read(&call.name) => match &staged {
                        // With staged changes, answer from the staged view
                        // and say so, so figures match the proposal.
                        Some(st) => {
                            let view = Document::new(st.staging.project().clone());
                            tools::run_read(&view, &call.name, &call.input).map(|data| {
                                json!({
                                    "view": "plan with the staged changes of this turn, not applied yet",
                                    "data": data,
                                })
                            })
                        }
                        None => tools::run_read(doc, &call.name, &call.input),
                    },
                    Some(_) => Err(tools::ToolError(format!("unknown tool `{}`", call.name))),
                }
            };

            let (content, is_error) = match &outcome {
                Ok(v) => (serde_json::to_string(v).unwrap_or_default(), false),
                Err(e) => (e.0.clone(), true),
            };
            log::append(
                log_dir.as_deref(),
                "tool",
                json!({
                    "turn_id": turn_id,
                    "round": rounds,
                    "tool": call.name,
                    "args": log::compact(&call.input),
                    "ok": !is_error,
                    "result": match &outcome { Ok(v) => log::compact(v), Err(_) => Value::Null },
                    "error": match &outcome { Ok(_) => Value::Null, Err(e) => json!(e.0) },
                    "staged": !is_error && tools::is_edit(&call.name),
                    "model_ms": model_ms,
                    "tool_ms": call_started.elapsed().as_millis() as u64,
                }),
            )
            .await;
            let mut block = json!({"type": "tool_result", "tool_use_id": call.id, "content": content});
            if is_error {
                block["is_error"] = json!(true);
            }
            results.push(block);
        }

        // Echo the assistant content unchanged (tool_use and thinking blocks
        // must survive), then every tool result in ONE user message.
        model_request.messages.push(json!({"role": "assistant", "content": response.content}));
        model_request.messages.push(json!({"role": "user", "content": results}));
    }

    // Final validation under the lock: the proposal is pinned to the revision
    // it was checked against.
    let mut proposal = None;
    if !commands.is_empty() {
        let s = app.session.lock().await;
        let doc = s.doc.as_ref().ok_or_else(no_document)?;
        let command = batch(&label, &commands);
        // A scoped turn is checked once more on the revision the proposal is
        // pinned to: the plan may have changed while the model worked.
        let checked = doc.preview(&command).and_then(|preview| {
            if let Some(ids) = scope {
                guhit_core::scope::check(doc.project(), doc.derived(), ids, &command)?;
            }
            Ok(preview)
        });
        match checked {
            Ok(preview) => {
                let p = AiProposal {
                    id: defaults::new_id(),
                    command,
                    preview,
                    base_revision: doc.revision(),
                };
                *app.ai.pending.lock().await = Some(Pending {
                    proposal: p.clone(),
                    project_id: doc.project().id.clone(),
                    log_dir: log_dir.clone(),
                });
                proposal = Some(p);
            }
            Err(e) => {
                let e: IpcError = e.into();
                reply = format!(
                    "The plan changed while I was working, so the proposal no longer fits ({}). Please ask again.",
                    e.message
                );
            }
        }
    }

    if hit_cap {
        let note = if proposal.is_some() {
            format!("I stopped after {MAX_ROUNDS} steps. The proposal may be incomplete: review it, or discard it and ask in smaller steps.")
        } else {
            format!("I stopped after {MAX_ROUNDS} steps without finishing. Try a smaller request.")
        };
        reply = if reply.is_empty() { note } else { format!("{reply}\n\n{note}") };
    }
    if reply.is_empty() {
        reply = match &proposal {
            Some(p) => format!("Proposed: {}. Review the preview, then apply or discard.", p.preview.diff.summary),
            None => "I have nothing to add.".to_string(),
        };
    }

    log::append(
        log_dir.as_deref(),
        "turn",
        json!({
            "turn_id": turn_id,
            "rounds": rounds,
            "hit_round_cap": hit_cap,
            "tools_used": tools_used,
            "proposal_id": proposal.as_ref().map(|p| p.id.clone()),
            "staged_steps": commands.len(),
            "diff": proposal.as_ref().map(|p| json!({
                "added": p.preview.diff.added.len(),
                "modified": p.preview.diff.modified.len(),
                "removed": p.preview.diff.removed.len(),
                "summary": p.preview.diff.summary,
            })),
            "reply": log::compact(&json!(reply)),
            "latency_ms": started.elapsed().as_millis() as u64,
        }),
    )
    .await;

    Ok(AiTurn { reply, proposal, tools_used, proposals: vec![] })
}

// -------------------------------------------------------------------- resolve

async fn resolve(app: &AppService, proposal_id: &str, accept: bool) -> Result<AiResolveResult, IpcError> {
    // A furnishing room resolves on its own, rebased on the plan if another
    // room went in first (D8: one proposal per room).
    if let Some(r) = furnish::resolve(app, proposal_id, accept).await {
        return r;
    }
    let pending = {
        let mut slot = app.ai.pending.lock().await;
        match slot.as_ref() {
            Some(p) if p.proposal.id == proposal_id => slot.take(),
            _ => None,
        }
    };
    let Some(pending) = pending else {
        return Err(IpcError::new("not_found", "That proposal is no longer pending."));
    };
    let log_dir = pending.log_dir.clone();
    let outcome = |outcome: &str, extra: Value| {
        let mut fields = json!({"proposal_id": pending.proposal.id, "outcome": outcome});
        if let (Some(map), Some(extra)) = (fields.as_object_mut(), extra.as_object()) {
            map.extend(extra.clone());
        }
        fields
    };

    if !accept {
        log::append(log_dir.as_deref(), "outcome", outcome("rejected", json!({}))).await;
        return Ok(AiResolveResult { applied: None });
    }

    // The proposal was validated against one exact revision of one project;
    // anything else and the preview the user saw is not what would be
    // applied. The project id is checked here, the revision inside
    // `commit_if_revision`, so the check and the apply share one lock.
    let current = {
        let s = app.session.lock().await;
        s.doc.as_ref().map(|d| (d.project().id.clone(), d.revision()))
    };
    let same_project = matches!(&current, Some((id, _)) if *id == pending.project_id);
    let stale = |revision: Option<u32>| {
        outcome(
            "stale",
            json!({
                "base_revision": pending.proposal.base_revision,
                "revision": revision,
            }),
        )
    };
    if !same_project {
        log::append(log_dir.as_deref(), "outcome", stale(current.as_ref().map(|c| c.1))).await;
        return Err(IpcError::new(
            "stale",
            "The open project changed after this proposal was made, so it was not applied. Ask again.",
        ));
    }

    let committed = app
        .commit_if_revision(
            pending.proposal.command.clone(),
            Origin::Ai,
            pending.proposal.base_revision,
        )
        .await;
    if let Err(e) = &committed {
        if e.code == "stale" {
            log::append(log_dir.as_deref(), "outcome", stale(None)).await;
            return Err(IpcError::new(
                "stale",
                "The plan changed after this proposal was made, so it was not applied. Ask again.",
            ));
        }
    }

    match committed {
        Ok(applied) => {
            log::append(
                log_dir.as_deref(),
                "outcome",
                outcome(
                    "accepted",
                    json!({
                        "revision": applied.state.revision,
                        "matches_preview": applied.diff == pending.proposal.preview.diff
                            && applied.state.project.elements == pending.proposal.preview.state.project.elements,
                    }),
                ),
            )
            .await;
            Ok(AiResolveResult { applied: Some(applied) })
        }
        Err(e) => {
            log::append(
                log_dir.as_deref(),
                "outcome",
                outcome("failed", json!({"code": e.code, "message": e.message})),
            )
            .await;
            Err(e)
        }
    }
}
