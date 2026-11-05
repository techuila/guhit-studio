//! The copilot's "Furnish rooms" task (`AiTask::Furnish`): one proposal per
//! room in `AiTurn::proposals`, each accepted or rejected on its own (D8).
//! Places only what `guhit_core::placement` allows, the rules behind
//! `Query::PlacementOptions`, so nothing lands in a wall, a doorway, a door
//! swing, a void or a stair opening. LANE L5 owns this module; `ai::chat`
//! hands the task here in one line.
//!
//! Pipeline: the model picks catalog items per room for the style (one
//! call, one tool, `choose_furniture`); the engine places them, or its own
//! default set for the room when the model names none; each room's objects
//! become one `Batch` with a dry run on the current plan. Nothing commits
//! until the user applies a room.
//!
//! Applying: every room's proposal waits in this module's queue, and
//! `ai_resolve` asks `resolve` below first, so any room is applied or
//! discarded on its own, in any order; one made before another room went in
//! is checked again on the plan as it is now (same objects, still clear) or
//! refused as stale. The first room is also the hub's one pending proposal.
//! Asking again for one room (`room_ids: [it]`) makes no model call (the
//! picks are kept per room and style) and gives the same objects with the
//! same ids; its new proposal replaces that room's in the queue.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use guhit_core::placement::{self, FurnishPlan};
use guhit_core::{CoreError, Document};
use guhit_model::*;
use serde_json::{json, Value};

use super::client::ModelRequest;
use super::{log, Pending};
use crate::AppService;

/// The one tool the model gets.
pub const TOOL: &str = "choose_furniture";
const MAX_TOKENS: u32 = 2048;
/// At most this many objects per room from the model.
const MAX_PICKS: usize = 16;
/// Review codes a furnishing must never raise.
pub const PLACEMENT_CODES: &[&str] = &["asset_in_wall", "assets_overlap", "door_swing_blocked", "asset_over_opening"];

const SYSTEM: &str = "You furnish rooms of a house plan in Guhit Studio, a Philippine architecture app. For each room you are given its use, its floor area and the catalog keys that suit it. Pick the furniture for the style the user asked for, as a list of catalog keys in the order they matter (the main piece first: a bed, a sofa, a kitchen run, a water closet), at most 12 per room, only keys from that room's list. The app places every piece itself, against walls and clear of doors, windows and other objects, and drops what does not fit, so do not give positions. Call choose_furniture once with every room. Never claim that a layout meets a building code.";

/// The model's picks per room, by (service, project, room, style): asking
/// for one room again (to apply it) needs no model call.
type PickKey = (usize, Id, Id, String);
fn picks_cache() -> &'static StdMutex<BTreeMap<PickKey, Vec<String>>> {
    static CACHE: OnceLock<StdMutex<BTreeMap<PickKey, Vec<String>>>> = OnceLock::new();
    CACHE.get_or_init(|| StdMutex::new(BTreeMap::new()))
}

/// Every room's proposal of the last furnishing turn, for `resolve`.
struct Queued {
    proposal: AiProposal,
    project_id: Id,
    room_id: Id,
    log_dir: Option<PathBuf>,
}
fn queue() -> &'static StdMutex<BTreeMap<usize, Vec<Queued>>> {
    static QUEUE: OnceLock<StdMutex<BTreeMap<usize, Vec<Queued>>>> = OnceLock::new();
    QUEUE.get_or_init(|| StdMutex::new(BTreeMap::new()))
}

/// One service's key in the caches: its copilot state, which tests replace.
fn service_key(app: &AppService) -> usize {
    Arc::as_ptr(&app.ai) as usize
}

fn style_key(style: &str) -> String {
    let s = style.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    if s.is_empty() {
        "modern".into()
    } else {
        s
    }
}

/// A room as the turn sees it.
#[derive(Debug, Clone)]
struct RoomInfo {
    id: Id,
    name: String,
    usage: RoomUsage,
    area_m2: f64,
}

fn usage_name(u: RoomUsage) -> String {
    serde_json::to_value(u).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

/// The tool the model calls.
fn tool_definition() -> Value {
    json!({
        "name": TOOL,
        "description": "Give the furniture for every room: catalog keys from that room's list, most important first. The app places them and drops what does not fit.",
        "input_schema": {
            "type": "object",
            "properties": {
                "rooms": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "room_id": {"type": "string"},
                            "keys": {"type": "array", "items": {"type": "string"}},
                        },
                        "required": ["room_id", "keys"],
                        "additionalProperties": false,
                    },
                },
            },
            "required": ["rooms"],
            "additionalProperties": false,
        },
    })
}

/// The model's message: each room with its use, area and the keys that fit it.
fn prompt(rooms: &[RoomInfo], style: &str, message: &str) -> String {
    let mut out = format!("Style: {style}.\n");
    let message = message.trim();
    if !message.is_empty() {
        out.push_str(&format!("The user said: {message}\n"));
    }
    out.push_str("Rooms:\n");
    for r in rooms {
        let keys = placement::keys_for_usage(r.usage);
        out.push_str(&format!(
            "- room_id {} \"{}\", {}, {:.1} m2. Keys: {}\n",
            r.id,
            r.name,
            usage_name(r.usage),
            r.area_m2,
            keys.join(", ")
        ));
    }
    out
}

/// The picks the model gave, per room id. Keys not in the catalog or not
/// for that room's use are dropped.
fn parse_picks(input: &Value, rooms: &[RoomInfo]) -> BTreeMap<Id, Vec<String>> {
    let mut out = BTreeMap::new();
    for r in input.get("rooms").and_then(Value::as_array).cloned().unwrap_or_default() {
        let Some(id) = r.get("room_id").and_then(Value::as_str) else { continue };
        let Some(room) = rooms.iter().find(|x| x.id == id) else { continue };
        let fits = placement::keys_for_usage(room.usage);
        let keys: Vec<String> = r
            .get("keys")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(Value::as_str).filter(|k| fits.contains(k)).map(str::to_string).take(MAX_PICKS).collect())
            .unwrap_or_default();
        if !keys.is_empty() {
            out.insert(room.id.clone(), keys);
        }
    }
    out
}

/// A room's plan as a proposal on `doc`: its objects as one batch, checked
/// by a dry run. An object the dry run finds in a wall, a door swing or on
/// top of another is dropped and the rest tried again: the placement rules
/// should never let one through, and this keeps it so.
/// Why a room got no proposal.
enum Refused {
    /// Its objects reach outside the selection the turn is limited to (D30).
    Scope,
    Error(IpcError),
}

fn propose(doc: &Document, plan: &mut FurnishPlan, scope: Option<&[Id]>) -> Result<Option<AiProposal>, Refused> {
    for _ in 0..8 {
        if plan.assets.is_empty() {
            return Ok(None);
        }
        let command = Command::Batch { label: format!("AI: Furnish {}", plan.room_name), commands: plan.commands() };
        if let Some(ids) = scope {
            guhit_core::scope::check(doc.project(), doc.derived(), ids, &command).map_err(|_| Refused::Scope)?;
        }
        let preview = doc.preview(&command).map_err(|e| Refused::Error(e.into()))?;
        let new: Vec<&Id> = plan.assets.iter().map(|a| &a.id).collect();
        let bad: Vec<Id> = preview
            .state
            .derived
            .issues
            .iter()
            .filter(|i| PLACEMENT_CODES.contains(&i.code.as_str()))
            .flat_map(|i| i.element_ids.iter().filter(|id| new.contains(id)).cloned().collect::<Vec<_>>())
            .collect();
        if bad.is_empty() {
            return Ok(Some(AiProposal { id: defaults::new_id(), command, preview, base_revision: doc.revision() }));
        }
        // The later object of a clash goes.
        let drop = plan.assets.iter().rev().find(|a| bad.contains(&a.id)).map(|a| a.id.clone());
        if let Some(id) = drop {
            if let Some(a) = plan.assets.iter().find(|a| a.id == id) {
                plan.skipped.push((a.catalog_key.clone(), "it would clash with the plan".into()));
            }
            plan.assets.retain(|a| a.id != id);
        }
    }
    Ok(None)
}

/// `AiTask::Furnish`: one proposal per room (D8). `room_ids` empty
/// furnishes every closed room on the active level.
pub async fn run(app: &AppService, request: &AiRequest, room_ids: &[Id], style: &str) -> Result<AiTurn, IpcError> {
    let scope: Option<&[Id]> = request.scope.as_ref().map(|s| s.ids.as_slice());
    if scope.is_some_and(|ids| ids.is_empty()) {
        return Err(IpcError::new("bad_args", "scope has no ids: limit the turn to a selection, or send no scope"));
    }
    let _turn = app.ai.chat_gate.lock().await;
    let style = style_key(style);
    let log_dir = app.project_dir().await;
    let key = service_key(app);

    // The rooms, read once.
    let (project_id, rooms) = {
        let s = app.session.lock().await;
        let doc = s.doc.as_ref().ok_or_else(super::no_document)?;
        if let Some(ids) = scope {
            guhit_core::scope::validate(doc.project(), ids)?;
        }
        let level = super::active_level(doc.project(), &request.active_level_id)?;
        let all = placement::furnishable_rooms(doc.project());
        let mut rooms = vec![];
        if room_ids.is_empty() {
            for (id, name, usage, lvl, area_m2) in all {
                if lvl == level {
                    rooms.push(RoomInfo { id, name, usage, area_m2 });
                }
            }
        } else {
            for id in room_ids {
                let Some((id, name, usage, _, area_m2)) = all.iter().find(|r| &r.0 == id).cloned() else {
                    let known = doc.project().elements.iter().any(|e| matches!(e, Element::Room(r) if &r.id == id));
                    return Err(if known {
                        IpcError::new("room_not_closed", "That room has no closed walls around it, so it has no floor to furnish.")
                    } else {
                        IpcError::new("not_found", format!("no room has id `{id}`"))
                    });
                };
                if !rooms.iter().any(|r: &RoomInfo| r.id == id) {
                    rooms.push(RoomInfo { id, name, usage, area_m2 });
                }
            }
        }
        (doc.project().id.clone(), rooms)
    };
    if rooms.is_empty() {
        return Err(IpcError::new("bad_args", "There is no closed room to furnish on this level."));
    }
    let turn_id = defaults::new_id();
    log::append(
        log_dir.as_deref(),
        "intent",
        json!({
            "turn_id": turn_id,
            "task": "furnish",
            "message": request.message,
            "room_ids": rooms.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
            "style": style,
            "scope_ids": scope,
        }),
    )
    .await;

    // The model's picks for the rooms it has not picked for yet.
    let cached = |room: &RoomInfo| -> Option<Vec<String>> {
        picks_cache().lock().ok()?.get(&(key, project_id.clone(), room.id.clone(), style.clone())).cloned()
    };
    let missing: Vec<RoomInfo> = rooms.iter().filter(|r| cached(r).is_none()).cloned().collect();
    let mut tools_used = vec![];
    if !missing.is_empty() {
        let client = super::model_client(app, &request.message)?;
        let model_request = ModelRequest {
            model: super::model_name(app).await,
            system: SYSTEM.to_string(),
            messages: vec![json!({"role": "user", "content": prompt(&missing, &style, &request.message)})],
            tools: vec![tool_definition()],
            max_tokens: MAX_TOKENS,
        };
        let answer = match client.send(&model_request).await {
            Ok(r) => r,
            Err(e) => {
                log::append(log_dir.as_deref(), "error", json!({"turn_id": turn_id, "code": e.code, "message": e.message})).await;
                return Err(e);
            }
        };
        let mut picked = BTreeMap::new();
        for call in answer.tool_uses() {
            if call.name == TOOL {
                picked.extend(parse_picks(&call.input, &missing));
                if !tools_used.contains(&TOOL.to_string()) {
                    tools_used.push(TOOL.to_string());
                }
            }
        }
        log::append(log_dir.as_deref(), "tool", json!({"turn_id": turn_id, "tool": TOOL, "picks": picked, "stop_reason": answer.stop_reason})).await;
        // A room the model left out gets the engine's own set, and is kept
        // as such, so applying it later is the same plan.
        if let Ok(mut cache) = picks_cache().lock() {
            for r in &missing {
                cache.insert((key, project_id.clone(), r.id.clone(), style.clone()), picked.get(&r.id).cloned().unwrap_or_default());
            }
        }
    }

    // One plan and one dry run per room, on the plan as it is now.
    let mut proposals = vec![];
    let mut lines = vec![];
    let mut room_of: Vec<(Id, Id)> = vec![];
    {
        let s = app.session.lock().await;
        let doc = s.doc.as_ref().ok_or_else(super::no_document)?;
        if doc.project().id != project_id {
            return Err(IpcError::new("stale", "The open project changed while furnishing. Ask again."));
        }
        for r in &rooms {
            let picks = cached(r).unwrap_or_default();
            let mut plan = placement::furnish_room(doc.project(), &r.id, Some(&picks), &style)?;
            match propose(doc, &mut plan, scope) {
                Ok(Some(p)) => {
                    lines.push(format!("- {}", plan.summary()));
                    room_of.push((p.id.clone(), r.id.clone()));
                    proposals.push(p);
                }
                Ok(None) => lines.push(format!("- {}: nothing fits", r.name)),
                Err(Refused::Scope) => lines.push(format!("- {}: left out, it is outside the selection", r.name)),
                Err(Refused::Error(e)) => return Err(e),
            }
            if !plan.skipped.is_empty() {
                let skipped: Vec<String> = plan.skipped.iter().map(|(k, why)| format!("{k} ({why})")).collect();
                lines.push(format!("  Left out: {}", skipped.join(", ")));
            }
        }
    }

    // The first room is the pending proposal `ai_resolve` applies; the
    // rest wait in the queue. A new turn replaces both.
    {
        let first = proposals.first().map(|p| Pending { proposal: p.clone(), project_id: project_id.clone(), log_dir: log_dir.clone() });
        let old = std::mem::replace(&mut *app.ai.pending.lock().await, first);
        if let Some(old) = old {
            log::append(old.log_dir.as_deref(), "outcome", json!({"proposal_id": old.proposal.id, "outcome": "superseded"})).await;
        }
        if let Ok(mut q) = queue().lock() {
            let fresh: Vec<Queued> = proposals
                .iter()
                .zip(room_of.iter())
                .map(|(p, (_, room))| Queued { proposal: p.clone(), project_id: project_id.clone(), room_id: room.clone(), log_dir: log_dir.clone() })
                .collect();
            let list = q.entry(key).or_default();
            if missing.is_empty() {
                // Asked again for rooms already planned: their old proposals
                // give way, the other rooms' stay.
                list.retain(|x| x.project_id == project_id && !rooms.iter().any(|r| r.id == x.room_id));
                list.extend(fresh);
            } else {
                *list = fresh;
            }
        }
    }

    let reply = if proposals.is_empty() {
        format!("I could not place anything in {}.\n{}", if rooms.len() == 1 { "that room" } else { "those rooms" }, lines.join("\n"))
    } else {
        format!(
            "One proposal per room, in the {style} style. Each is previewed on the plan; apply or discard each on its own. Every piece stands clear of walls, doorways, door swings, windows lower than it, stairs and voids. These are layout suggestions, not a check against any code.\n{}",
            lines.join("\n")
        )
    };
    log::append(
        log_dir.as_deref(),
        "turn",
        json!({
            "turn_id": turn_id,
            "task": "furnish",
            "proposal_ids": proposals.iter().map(|p| p.id.clone()).collect::<Vec<_>>(),
            "rooms": room_of.iter().map(|(_, r)| r.clone()).collect::<Vec<_>>(),
            "added": proposals.iter().map(|p| p.preview.diff.added.len()).sum::<usize>(),
            "reply": log::compact(&json!(reply)),
        }),
    )
    .await;
    Ok(AiTurn { reply, proposal: None, tools_used, proposals })
}

/// Resolves one room's proposal from the queue (`ai_resolve` asks this
/// first). None when `proposal_id` is not one of them. A proposal made before another room was applied is checked again
/// on the plan as it is now: it is applied only when it adds the very same
/// objects and raises no placement review item, else it is stale.
pub async fn resolve(app: &AppService, proposal_id: &str, accept: bool) -> Option<Result<AiResolveResult, IpcError>> {
    let key = service_key(app);
    let queued = {
        let mut q = queue().lock().ok()?;
        let list = q.get_mut(&key)?;
        let at = list.iter().position(|x| x.proposal.id == proposal_id)?;
        list.remove(at)
    };
    {
        // The hub's pending slot may hold the same proposal.
        let mut slot = app.ai.pending.lock().await;
        if slot.as_ref().is_some_and(|p| p.proposal.id == proposal_id) {
            *slot = None;
        }
    }
    let outcome = |o: &str| json!({"proposal_id": proposal_id, "outcome": o, "room_id": queued.room_id});
    if !accept {
        log::append(queued.log_dir.as_deref(), "outcome", outcome("rejected")).await;
        return Some(Ok(AiResolveResult { applied: None }));
    }
    let stale = || IpcError::new("stale", "The plan changed after this proposal was made, so it was not applied. Ask again.");
    let revision = {
        let s = app.session.lock().await;
        let Some(doc) = s.doc.as_ref() else { return Some(Err(super::no_document())) };
        if doc.project().id != queued.project_id {
            return Some(Err(stale()));
        }
        if doc.revision() != queued.proposal.base_revision {
            // Rebased on the plan as it is now: the same objects, still clear.
            let fresh = match doc.preview(&queued.proposal.command) {
                Ok(p) => p,
                Err(_) => return Some(Err(stale())),
            };
            let added: Vec<&Element> = queued.proposal.preview.state.project.elements.iter().filter(|e| queued.proposal.preview.diff.added.contains(e.id())).collect();
            let same = added.iter().all(|e| fresh.state.project.elements.iter().any(|f| f == *e));
            let clear = !fresh.state.derived.issues.iter().any(|i| PLACEMENT_CODES.contains(&i.code.as_str()) && i.element_ids.iter().any(|id| queued.proposal.preview.diff.added.contains(id)));
            if !same || !clear {
                log::append(queued.log_dir.as_deref(), "outcome", outcome("stale")).await;
                return Some(Err(stale()));
            }
        }
        doc.revision()
    };
    let committed = app.commit_if_revision(queued.proposal.command.clone(), Origin::Ai, revision).await;
    log::append(queued.log_dir.as_deref(), "outcome", outcome(if committed.is_ok() { "accepted" } else { "failed" })).await;
    Some(committed.map(|applied| AiResolveResult { applied: Some(applied) }))
}

/// Drops the queued furnishing proposals of a service: a new chat turn
/// replaces them, as it replaces the pending one.
pub fn discard(app: &AppService) {
    if let Ok(mut q) = queue().lock() {
        q.remove(&service_key(app));
    }
}

/// The furnishing plan of one room without a model: the engine's default
/// set for its use and `style`. For agents and scripts (D47).
pub fn plan_room(project: &Project, room_id: &Id, style: &str) -> Result<FurnishPlan, CoreError> {
    placement::furnish_room(project, room_id, None, &style_key(style))
}
