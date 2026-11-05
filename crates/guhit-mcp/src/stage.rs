//! Running edits: every editing tool, `batch` and `build` (DECISIONS D3, D30).
//!
//! The steps are translated (`translate::plan`) and staged one by one on one
//! working copy of the project (`guhit_core::Staging`), outside the session
//! lock, then committed as ONE undo step with `Origin::Ai`, guarded by the
//! revision staging started from. A step can name what an earlier step made
//! through its `ref` ("$kitchen"). `dry_run` stages everything and commits
//! nothing.
//!
//! Every result says which review items the edit added and which it
//! resolved since its base revision, and which walls it also moved.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Mutex, OnceLock};

use guhit_app::ai::tools as copilot;
use guhit_app::AppService;
use guhit_core::CoreError;
use guhit_model::*;
use serde_json::{json, Value};

use crate::agent::CallContext;
use crate::fail::ToolFail;
use crate::tools::Output;
use crate::translate::{self, Primary, View};

/// One step of an edit.
#[derive(Debug, Clone)]
pub(crate) struct StepIn {
    pub tool: String,
    pub args: Value,
    pub r#ref: Option<String>,
    /// Where a `build` step came from: "openings[2]".
    pub origin: Option<String>,
}

impl StepIn {
    pub fn new(tool: &str, args: Value) -> Self {
        Self { tool: tool.to_string(), args, r#ref: None, origin: None }
    }
}

pub(crate) struct Options {
    /// The undo label, "MCP: ...".
    pub label: String,
    pub dry_run: bool,
    /// List what each step did (`batch`, `build`).
    pub per_step: bool,
}

/// Most items listed per list in a result; the rest are counted.
const LIST_CAP: usize = 100;

/// What an edit did besides elements, as `copilot::step_diff` lists it:
/// levels, door and window types, sheets, revisions, the title block.
const PROJECT_LISTS: [&str; 10] = [
    "levels_added",
    "levels_removed",
    "types_added",
    "types_changed",
    "types_removed",
    "sheets_added",
    "sheets_changed",
    "sheets_removed",
    "revisions",
    "title_block_changed",
];

// --------------------------------------------------------------- the scope

/// The `scope` a call asked for (DECISIONS D30).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ScopeArg {
    /// What the user has selected in the window.
    Selection,
    Ids(Vec<Id>),
}

/// The scope an edit is held to, and who set it.
pub(crate) struct Scope {
    pub ids: Vec<Id>,
    /// The user switched on "Only the selection": it binds every edit.
    pub locked: bool,
}

/// Take `scope` out of a tool's arguments: the translation refuses
/// arguments it does not know.
pub(crate) fn take_scope(args: &mut Value) -> Result<Option<ScopeArg>, ToolFail> {
    let Some(map) = args.as_object_mut() else {
        return Ok(None);
    };
    let bad = || ToolFail::msg("invalid arguments: `scope` is \"selection\" or a list of element ids");
    match map.remove("scope") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s == "selection" => Ok(Some(ScopeArg::Selection)),
        Some(Value::Array(items)) => {
            let ids: Vec<Id> = items
                .iter()
                .map(|v| v.as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(bad)?;
            if ids.is_empty() {
                return Err(ToolFail::msg("invalid arguments: `scope` is an empty list; name at least one element"));
            }
            Ok(Some(ScopeArg::Ids(ids)))
        }
        Some(_) => Err(bad()),
    }
}

/// The scope an edit is held to: the window's selection while the user has
/// "Only the selection" on (it binds every edit, whatever the call asked
/// for), else what the call asked for. Selection ids no longer in the plan
/// are dropped.
pub(crate) fn resolve_scope(app: &AppService, project: &Project, asked: Option<&ScopeArg>) -> Result<Option<Scope>, ToolFail> {
    resolve_scope_with(&app.local_presence(), project, asked)
}

/// `resolve_scope` with the window's presence read already.
fn resolve_scope_with(window: &Presence, project: &Project, asked: Option<&ScopeArg>) -> Result<Option<Scope>, ToolFail> {
    let selected: Vec<Id> = window
        .selection
        .iter()
        .filter(|id| project.elements.iter().any(|e| e.id() == *id))
        .cloned()
        .collect();
    if window.ai_scope && !selected.is_empty() {
        return Ok(Some(Scope { ids: selected, locked: true }));
    }
    match asked {
        None => Ok(None),
        Some(ScopeArg::Selection) if selected.is_empty() => Err(ToolFail::msg(
            "invalid arguments: scope is \"selection\", but nothing is selected in the Guhit Studio window. Ask the user to select the parts to change, or name them by id",
        )),
        Some(ScopeArg::Selection) => Ok(Some(Scope { ids: selected, locked: false })),
        Some(ScopeArg::Ids(ids)) => Ok(Some(Scope { ids: ids.clone(), locked: false })),
    }
}

/// An engine refusal of the scope, as the model should read it.
pub(crate) fn scope_fail(e: CoreError, project: &Project, derived: &Derived, scope: &Scope) -> ToolFail {
    if let CoreError::NotFound(id) = &e {
        return ToolFail::msg(format!("not_found: the scope names an element that is not in the plan: {id}"));
    }
    let ids = match &e {
        CoreError::Invalid { element_ids, .. } => element_ids.clone(),
        _ => vec![],
    };
    let message = IpcError::from(e).message;
    let what = guhit_core::scope::describe(project, derived, &scope.ids);
    let text = if scope.locked {
        format!(
            "out_of_scope: {message} The user switched on \"Only the selection\" in Guhit Studio, so every edit is limited to: {what}. Change only those elements and what stands in a selected room, or ask the user to widen the selection or switch it off."
        )
    } else {
        format!(
            "out_of_scope: {message} This edit is limited by its scope to: {what}. Change only those elements and what stands in a selected room, or ask the user before widening it."
        )
    };
    let mut f = ToolFail::msg(text);
    f.element_ids = ids;
    f
}

/// New elements go on the first level unless a call names another.
pub(crate) fn first_level(project: &Project) -> Result<Id, ToolFail> {
    project
        .levels
        .first()
        .map(|l| l.id.clone())
        .ok_or_else(|| ToolFail::new("invalid", "the project has no level"))
}

pub(crate) fn no_document() -> ToolFail {
    ToolFail::from(IpcError::new("no_document", "no project is open. Call create_project or open_project first"))
        .with_hint("Open or create a project first: project action \"open\" or \"create\".")
}

// ------------------------------------------------------------------ refs

#[derive(Debug, Clone, Default)]
struct RefEntry {
    id: Option<Id>,
    /// Everything of the step's kind it made, for "$name.2".
    all: Vec<Id>,
}

/// "$name" or "$name.N": the ref and the index.
fn ref_of(text: &str) -> Option<(&str, Option<usize>)> {
    let rest = text.strip_prefix('$')?;
    let (name, index) = match rest.rsplit_once('.') {
        Some((n, i)) if i.chars().all(|c| c.is_ascii_digit()) && !i.is_empty() => (n, i.parse().ok()),
        _ => (rest, None),
    };
    let valid = !name.is_empty()
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    valid.then_some((name, index))
}

/// Replace every "$name" string and `{"$ref": "name"}` object with the id
/// that ref names. Returns the first ref that names nothing.
fn resolve_refs(v: &mut Value, refs: &BTreeMap<String, RefEntry>) -> Result<(), String> {
    let lookup = |name: &str, index: Option<usize>| -> Option<Id> {
        let entry = refs.get(name)?;
        match index {
            None => entry.id.clone(),
            Some(n) => entry.all.get(n).cloned(),
        }
    };
    match v {
        Value::String(s) => {
            if let Some((name, index)) = ref_of(s) {
                *v = Value::String(lookup(name, index).ok_or_else(|| s.clone())?);
            }
            Ok(())
        }
        Value::Object(map) => {
            if map.len() == 1 {
                if let Some(Value::String(name)) = map.get("$ref") {
                    let name = name.trim_start_matches('$').to_string();
                    let full = format!("${name}");
                    let (n, i) = ref_of(&full).ok_or_else(|| full.clone())?;
                    let id = lookup(n, i).ok_or_else(|| full.clone())?;
                    *v = Value::String(id);
                    return Ok(());
                }
            }
            for item in map.values_mut() {
                resolve_refs(item, refs)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                resolve_refs(item, refs)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

// --------------------------------------------------------- issue baselines

/// Review items seen at a revision, kept for `check since_revision`. The
/// engine keeps no history of derived data, so this server notes the items
/// at every revision it sees: before and after each of its edits, and at
/// each `check`.
type Baselines = BTreeMap<Id, VecDeque<(u32, Vec<Issue>)>>;
/// Revisions kept per project.
const BASELINES_KEPT: usize = 64;

fn baselines() -> &'static Mutex<Baselines> {
    static B: OnceLock<Mutex<Baselines>> = OnceLock::new();
    B.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub(crate) fn note_issues(project_id: &str, revision: u32, issues: &[Issue]) {
    let mut all = baselines().lock().unwrap_or_else(|e| e.into_inner());
    let list = all.entry(project_id.to_string()).or_default();
    if list.iter().any(|(r, _)| *r == revision) {
        return;
    }
    list.push_back((revision, issues.to_vec()));
    if list.len() > BASELINES_KEPT {
        list.pop_front();
    }
}

/// The items at `revision`, or at the latest revision before it this server
/// saw, with the revision they are from.
pub(crate) fn issues_at(project_id: &str, revision: u32) -> Option<(u32, Vec<Issue>)> {
    let all = baselines().lock().unwrap_or_else(|e| e.into_inner());
    all.get(project_id)?
        .iter()
        .filter(|(r, _)| *r <= revision)
        .max_by_key(|(r, _)| *r)
        .cloned()
}

pub(crate) fn issue_brief(i: &Issue) -> Value {
    let mut v = json!({"id": i.id, "code": i.code, "severity": i.severity, "message": i.message, "element_ids": i.element_ids});
    if let Some(l) = i.location {
        v["location_mm"] = json!({"x": geo_r(l.x), "y": geo_r(l.y), "z": geo_r(l.z)});
    }
    if i.status == IssueStatus::Ignored {
        v["status"] = json!("ignored");
    }
    v
}

fn geo_r(v: f64) -> f64 {
    crate::geo::r1(v)
}

/// Items in `after` that `before` did not have, and the other way round.
pub(crate) fn issue_change(before: &[Issue], after: &[Issue]) -> (Vec<Value>, Vec<Value>) {
    let added = after.iter().filter(|a| !before.iter().any(|b| b.id == a.id)).map(issue_brief).collect();
    let resolved = before
        .iter()
        .filter(|b| !after.iter().any(|a| a.id == b.id))
        .map(|i| json!({"id": i.id, "code": i.code, "message": i.message}))
        .collect();
    (added, resolved)
}

fn capped(list: Vec<Value>) -> (Value, usize) {
    let n = list.len();
    (Value::Array(list.into_iter().take(LIST_CAP).collect()), n)
}

// ------------------------------------------------------------------ running

fn step_text(tool: &str, step: &StepIn) -> String {
    let mut parts = vec![format!("`{tool}`")];
    if let Some(o) = &step.origin {
        parts.push(o.clone());
    }
    if let Some(r) = &step.r#ref {
        parts.push(format!("ref `{r}`"));
    }
    parts.join(", ")
}

/// A failure in step `index` of `total`, worded as v1's batch did.
fn step_fail(f: ToolFail, index: usize, step: &StepIn, total: usize) -> ToolFail {
    // One tool call is not a batch: its failure has no step.
    if total == 1 && step.origin.is_none() {
        return f;
    }
    // The text of the failure as it was, before it knows its step, so the
    // step is named once.
    let inner = f.text();
    let text = format!("step {} of {total} ({}): {inner} Nothing was applied.", index + 1, step_text(&step.tool, step));
    f.at_step(index + 1, step.r#ref.as_deref()).reworded(text)
}

/// What staging worked out besides the staged batch: everything the result
/// reports.
struct Staged {
    before: Project,
    base_issues: Vec<Issue>,
    per_step: Vec<Value>,
    refs: BTreeMap<String, RefEntry>,
    infos: Vec<Value>,
    primary_ids: Vec<Option<Id>>,
    scope: Option<(Vec<Id>, bool)>,
}

/// Translate and stage every step on `project`. Pure: the agents registry
/// may run it twice, once on a copy and again under the lock when another
/// agent edited in between.
fn stage_all(
    project: &Project,
    derived: &Derived,
    steps: &[StepIn],
    asked: Option<&ScopeArg>,
    window: &Presence,
    label: &str,
    per_step_lists: bool,
) -> Result<(guhit_core::Staging, Staged), ToolFail> {
    let level_id = first_level(project)?;
    let scope = resolve_scope_with(window, project, asked)?;
    if let Some(sc) = &scope {
        guhit_core::scope::validate(project, &sc.ids).map_err(|e| scope_fail(e, project, derived, sc))?;
    }
    let total = steps.len();
    let mut staging = guhit_core::Staging::new(project, label);
    // What earlier steps made is in reach, so a later step can build on it,
    // but it never widens the area where new elements may go.
    let mut made_ids: Vec<Id> = vec![];
    let mut per_step: Vec<Value> = vec![];
    let mut refs: BTreeMap<String, RefEntry> = BTreeMap::new();
    let mut infos: Vec<Value> = vec![];
    let mut primary_ids: Vec<Option<Id>> = vec![];

    for (i, step) in steps.iter().enumerate() {
        let mut args = step.args.clone();
        resolve_refs(&mut args, &refs).map_err(|name| {
            let known: Vec<&String> = refs.keys().collect();
            step_fail(
                ToolFail::new("unknown_ref", format!("`{name}` names no ref of an earlier step. Refs so far: {known:?}")),
                i,
                step,
                total,
            )
        })?;
        let start = staging.project().clone();
        let mut step_ids: Vec<Id> = vec![];
        let mut then_id: Option<Id> = None;
        let plan = {
            let view = if i == 0 { View::with_derived(&start, derived.clone()) } else { View::new(&start) };
            let plan = translate::plan(&view, &step.tool, &args, &level_id).map_err(|e| step_fail(e, i, step, total))?;
            if let Some(sc) = &scope {
                for command in &plan.commands {
                    guhit_core::scope::check_staged(view.project, view.derived(), &sc.ids, &made_ids, command)
                        .map_err(|e| step_fail(scope_fail(e, view.project, view.derived(), sc), i, step, total))?;
                }
            }
            plan
        };
        if !plan.commands.is_empty() {
            // A refusal here is the engine refusing the whole batch, which is
            // what a commit would do.
            staging.push(&plan.commands).map_err(|e| step_fail(ToolFail::from(e), i, step, total))?;
        }
        for then in &plan.then {
            let added: Vec<Id> = staging
                .project()
                .elements
                .iter()
                .map(|e| e.id().clone())
                .filter(|id| !start.elements.iter().any(|e| e.id() == id))
                .collect();
            let more = {
                let view = View::new(staging.project());
                let (more, id) = translate::then_commands(&view, then, &added).map_err(|e| step_fail(e, i, step, total))?;
                if let Some(sc) = &scope {
                    let staged_made = staging.diff().added;
                    for command in &more {
                        guhit_core::scope::check_staged(view.project, view.derived(), &sc.ids, &staged_made, command)
                            .map_err(|e| step_fail(scope_fail(e, view.project, view.derived(), sc), i, step, total))?;
                    }
                }
                then_id = then_id.or(id);
                more
            };
            if !more.is_empty() {
                staging.push(&more).map_err(|e| step_fail(ToolFail::from(e), i, step, total))?;
            }
        }
        if plan.commands.is_empty() && plan.then.is_empty() {
            return Err(step_fail(ToolFail::args("this step would change nothing"), i, step, total));
        }

        let diff = copilot::step_diff(&start, staging.project());
        // What a ref names: the element of the step's kind it made (or the
        // room it named), or the level it added or changed.
        let primary = match plan.primary {
            Primary::Kind(kind) => {
                let kind_name = serde_json::to_value(kind).unwrap_or(Value::Null);
                for item in diff["added"].as_array().into_iter().flatten() {
                    if item["kind"] == kind_name {
                        if let Some(id) = item["id"].as_str() {
                            step_ids.push(id.to_string());
                        }
                    }
                }
                then_id.clone().or_else(|| step_ids.first().cloned())
            }
            Primary::Level => diff["levels_added"][0]["id"].as_str().map(str::to_string).or_else(|| {
                plan.commands.iter().find_map(|c| match c {
                    Command::UpdateLevel { level } => Some(level.id.clone()),
                    _ => None,
                })
            }),
            Primary::None => None,
        };
        if let Some(r) = &step.r#ref {
            if refs.contains_key(r) {
                return Err(step_fail(ToolFail::args(format!("ref `{r}` is used twice; refs name one step each")), i, step, total));
            }
            refs.insert(r.clone(), RefEntry { id: primary.clone(), all: step_ids.clone() });
        }
        if let Some(info) = &plan.info {
            let mut info = info.clone();
            // A dimension string reports what the engine stored, which
            // measures walls on the face it names, not on the centerlines
            // the call gave (GR2: 8000 said, 8150 stored).
            if info.get("parts_mm").is_some() {
                let stored = primary.as_ref().and_then(|id| staging.project().elements.iter().find(|e| e.id() == id));
                if let Some(Element::Dimension(d)) = stored {
                    crate::permit::report_parts(d, &mut info);
                }
            }
            info["step"] = json!(i + 1);
            if let Some(r) = &step.r#ref {
                info["ref"] = json!(r);
            }
            infos.push(info);
        }
        primary_ids.push(primary.clone());
        if per_step_lists {
            let mut one = json!({
                "step": i + 1,
                "tool": step.tool,
                "created": diff["added"],
                "changed": diff["modified"],
                "deleted": diff["removed"],
            });
            for key in PROJECT_LISTS {
                if let Some(v) = diff.get(key) {
                    one[key] = v.clone();
                }
            }
            if let Some(r) = &step.r#ref {
                one["ref"] = json!(r);
                one["id"] = json!(primary);
            }
            per_step.push(one);
        }
        if scope.is_some() {
            made_ids = staging.diff().added;
        }
    }
    let staged = Staged {
        before: project.clone(),
        base_issues: derived.issues.clone(),
        per_step,
        refs,
        infos,
        primary_ids,
        scope: scope.map(|s| (s.ids, s.locked)),
    };
    Ok((staging, staged))
}

/// Stage `steps` and commit them as one undo step (or none, with dry_run).
/// An agent's call commits through the agents registry: as the agent, inside
/// its limit and outside other agents' claims.
pub(crate) async fn run(
    app: &AppService,
    steps: Vec<StepIn>,
    asked: Option<&ScopeArg>,
    opts: Options,
    ctx: &CallContext,
) -> Result<Output, ToolFail> {
    let window = app.local_presence();
    let total = steps.len();
    let per_step = opts.per_step;
    let build = |project: &Project, derived: &Derived| {
        stage_all(project, derived, &steps, asked, &window, &opts.label, per_step)
    };
    let snapshot = || async {
        let s = app.session.lock().await;
        let doc = s.doc.as_ref().ok_or_else(no_document)?;
        Ok::<_, ToolFail>((doc.project().clone(), doc.derived().clone(), doc.revision()))
    };

    let mut out = if opts.dry_run {
        let (project, derived, revision) = snapshot().await?;
        note_issues(&project.id, revision, &derived.issues);
        let (staging, st) = build(&project, &derived)?;
        // The registry's checks, as the commit would run them: a dry run
        // that passes is not refused `claimed`, `out_of_scope` or
        // `agent_paused` when it is sent for real.
        if let Some(call) = &ctx.call {
            app.agent_check(call, &derived, &staging).map_err(|e| {
                let f = ToolFail::from(e);
                let text = format!("dry run: {} The commit would be refused the same way.", f.text());
                f.reworded(text)
            })?;
        }
        let after = staging.project().clone();
        let derived_after = staging.derived();
        let diff = staging.diff();
        let mut out = json!({
            "ok": true,
            "dry_run": true,
            "note": "Nothing was committed. Call again without dry_run to apply it.",
            "revision": revision,
            "base_revision": revision,
            "summary": diff.summary,
            "totals": derived_after.totals,
        });
        finish(&mut out, &st, &after, &diff.also_moved, &derived_after.issues, total);
        out
    } else {
        let (result, st) = match &ctx.call {
            Some(call) => app.agent_edit(call, build).await?,
            None => {
                let (project, derived, revision) = snapshot().await?;
                let (staging, st) = build(&project, &derived)?;
                (app.commit_staged_if_revision(staging, Origin::Ai, revision).await?, st)
            }
        };
        note_issues(&result.state.project.id, result.state.revision.saturating_sub(1), &st.base_issues);
        note_issues(&result.state.project.id, result.state.revision, &result.state.derived.issues);
        let mut out = json!({
            "ok": true,
            "revision": result.state.revision,
            "base_revision": result.state.revision.saturating_sub(1),
            "summary": result.diff.summary,
            "undo_label": result.state.undo_label,
            "totals": result.state.derived.totals,
        });
        finish(&mut out, &st, &result.state.project, &result.diff.also_moved, &result.state.derived.issues, total);
        if let Some(call) = &ctx.call {
            out["agent"] = json!(call.name);
        }
        out
    };
    if out.get("agent").is_none() {
        out.as_object_mut().map(|m| m.remove("agent"));
    }
    Ok(Output::Json(out))
}

/// The lists every edit result has.
fn finish(out: &mut Value, st: &Staged, after: &Project, also_moved: &[Id], issues_after: &[Issue], total: usize) {
    let step = copilot::step_diff(&st.before, after);
    put_lists(out, &step, also_moved, after);
    let (added, resolved) = issue_change(&st.base_issues, issues_after);
    let (a, na) = capped(added);
    let (r, nr) = capped(resolved);
    out["issues_added"] = a;
    out["issues_resolved"] = r;
    if na > LIST_CAP || nr > LIST_CAP {
        out["issue_counts"] = json!({"added": na, "resolved": nr});
    }
    if !st.per_step.is_empty() {
        out["steps"] = Value::Array(st.per_step.clone());
    }
    let refs: BTreeMap<&String, &Option<Id>> = st.refs.iter().map(|(k, v)| (k, &v.id)).collect();
    if !refs.is_empty() {
        out["refs"] = json!(refs);
    }
    if total == 1 {
        if let Some(Some(id)) = st.primary_ids.first() {
            out["id"] = json!(id);
        }
    }
    if !st.infos.is_empty() {
        out["details"] = Value::Array(st.infos.clone());
    }
    if after.levels != st.before.levels {
        let warnings = translate::level_overlaps(after);
        if !warnings.is_empty() {
            out["warnings"] = json!(warnings);
        }
    }
    if let Some((ids, locked)) = &st.scope {
        out["scope"] = json!({"ids": ids, "set_by": if *locked { "the user (Only the selection)" } else { "this call" }});
    }
}

/// created, changed, deleted, the levels and `also_moved` into a result.
fn put_lists(out: &mut Value, step: &Value, also_moved: &[Id], after: &Project) {
    for (key, from) in [("created", "added"), ("changed", "modified"), ("deleted", "removed")] {
        let list = step[from].as_array().cloned().unwrap_or_default();
        let n = list.len();
        out[key] = Value::Array(list.into_iter().take(LIST_CAP).collect());
        if n > LIST_CAP {
            out[format!("{key}_count")] = json!(n);
        }
    }
    for key in PROJECT_LISTS {
        if let Some(v) = step.get(key) {
            out[key] = v.clone();
        }
    }
    if !also_moved.is_empty() {
        out["also_moved"] = copilot::also_moved_list(after, also_moved);
        out["note"] = json!(
            "also_moved lists walls this edit reshaped that the call did not name: walls joined to what moved. Tell the user which ones."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_are_found_in_strings_and_objects() {
        let mut refs = BTreeMap::new();
        refs.insert("hall".to_string(), RefEntry { id: Some("w1".into()), all: vec!["w1".into(), "w2".into()] });
        let mut v = json!({"wall_id": "$hall", "ids": ["$hall.1", {"$ref": "hall"}], "name": "Hall"});
        resolve_refs(&mut v, &refs).unwrap();
        assert_eq!(v, json!({"wall_id": "w1", "ids": ["w2", "w1"], "name": "Hall"}));
        let mut bad = json!({"wall_id": "$nope"});
        assert_eq!(resolve_refs(&mut bad, &refs), Err("$nope".to_string()));
        // Not a ref: a price, an empty name.
        let mut money = json!({"name": "$5 room", "x": "$"});
        resolve_refs(&mut money, &refs).unwrap();
        assert_eq!(money["name"], "$5 room");
    }
}
