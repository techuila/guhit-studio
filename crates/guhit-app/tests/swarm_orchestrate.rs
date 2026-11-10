//! The in-app swarm orchestrator (lane L2, D56): the lead's plan, the
//! workers, dependencies, pause, stop, the cap, rate limits and spend. A
//! fake model stands in for Claude; everything else is real: the app
//! service, the draft, the agents registry, the engine.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use guhit_app::ai::client::{BoxFut, ClientError, Metered, ModelClient, ModelRequest, ModelResponse, ScriptedClient, Usage};
use guhit_app::ai::keys::MemoryKeyStore;
use guhit_app::ai::AiState;
use guhit_app::swarm::cost;
use guhit_app::AppService;
use guhit_model::*;
use serde_json::{json, Value};

// ================================================================ the fake

/// One request the fake answered.
#[derive(Clone)]
struct Call {
    key: String,
    round: usize,
    start: Instant,
    end: Instant,
    request: ModelRequest,
}

type Script = dyn Fn(&str, usize, &ModelRequest) -> Result<Metered, ClientError> + Send + Sync;

/// A model routed by the "Part: <key>" line of the first user message, the
/// round being the assistant turns so far. Records every request, how many
/// were in flight at once, and what the swarm looked like.
struct Fake {
    script: Box<Script>,
    delay: Duration,
    calls: Mutex<Vec<Call>>,
    in_flight: AtomicUsize,
    most_in_flight: AtomicUsize,
}

impl Fake {
    fn new(delay_ms: u64, script: impl Fn(&str, usize, &ModelRequest) -> Result<Metered, ClientError> + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            script: Box::new(script),
            delay: Duration::from_millis(delay_ms),
            calls: Mutex::new(vec![]),
            in_flight: AtomicUsize::new(0),
            most_in_flight: AtomicUsize::new(0),
        })
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    fn of(&self, key: &str) -> Vec<Call> {
        self.calls().into_iter().filter(|c| c.key == key).collect()
    }
}

fn key_of(request: &ModelRequest) -> String {
    let first = request.messages.first().and_then(|m| m["content"].as_str()).unwrap_or_default();
    first.lines().next().and_then(|l| l.strip_prefix("Part: ")).unwrap_or("?").to_string()
}

fn round_of(request: &ModelRequest) -> usize {
    request.messages.iter().filter(|m| m["role"] == "assistant").count()
}

impl ModelClient for Fake {
    fn send<'a>(&'a self, request: &'a ModelRequest) -> BoxFut<'a, Result<ModelResponse, IpcError>> {
        Box::pin(async move { self.send_once(request).await.map(|m| m.response).map_err(IpcError::from) })
    }

    fn send_once<'a>(&'a self, request: &'a ModelRequest) -> BoxFut<'a, Result<Metered, ClientError>> {
        Box::pin(async move {
            let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.most_in_flight.fetch_max(now, Ordering::SeqCst);
            let start = Instant::now();
            tokio::time::sleep(self.delay).await;
            let (key, round) = (key_of(request), round_of(request));
            let out = (self.script)(&key, round, request);
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            self.calls.lock().unwrap().push(Call { key, round, start, end: Instant::now(), request: request.clone() });
            out
        })
    }
}

fn ok(response: ModelResponse) -> Result<Metered, ClientError> {
    Ok(Metered::unmetered(response))
}

fn tools(calls: &[(&str, Value)]) -> ModelResponse {
    ScriptedClient::tools(calls)
}

fn finish(summary: &str) -> ModelResponse {
    tools(&[("finish_part", json!({"summary": summary}))])
}

/// The `n`th tool result of the request's last message, parsed.
fn result(request: &ModelRequest, n: usize) -> Value {
    let blocks = request.messages.last().and_then(|m| m["content"].as_array()).cloned().unwrap_or_default();
    let results: Vec<&Value> = blocks.iter().filter(|b| b["type"] == "tool_result").collect();
    serde_json::from_str(results[n]["content"].as_str().unwrap()).unwrap()
}

// ================================================================== the rig

async fn rig(fake: Arc<Fake>) -> (AppService, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut app = AppService::new_sandboxed(dir.path().to_path_buf());
    app.ai = Arc::new(AiState::with_parts(fake, Box::new(MemoryKeyStore::with_key("sk-ant-test-not-a-key"))));
    app.handle("hub_create", json!({"name": "Swarm", "template": "blank"})).await.unwrap();
    (app, dir)
}

async fn start(app: &AppService, brief: &str, cap: Option<f64>) -> SwarmStatus {
    let v = app
        .handle("swarm_start", json!({"request": {"brief": brief, "review_plan": false, "cap_usd": cap}}))
        .await
        .unwrap();
    serde_json::from_value(v).unwrap()
}

async fn ipc(app: &AppService, cmd: &str, args: Value) -> SwarmStatus {
    serde_json::from_value(app.handle(cmd, args).await.unwrap()).unwrap()
}

/// Wait until the swarm's status passes `test`, at most 15 s.
async fn until(app: &AppService, what: &str, test: impl Fn(&SwarmStatus) -> bool) -> SwarmStatus {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(s) = app.swarm_status() {
            if test(&s) {
                return s;
            }
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}: {:#?}", app.swarm_status());
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn part<'a>(s: &'a SwarmStatus, key: &str) -> &'a SwarmPart {
    s.parts.iter().find(|p| p.key == key).unwrap()
}

fn plan_part(key: &str, kinds: &[&str], after: &[(&str, &str)], milestones: &[&str]) -> Value {
    json!({
        "key": key,
        "name": format!("{}{}", key[..1].to_uppercase(), &key[1..]),
        "role": format!("The {key}"),
        "instructions": format!("Draw the {key}."),
        "scope": {"levels": [], "kinds": kinds},
        "after": after.iter().map(|(p, m)| json!({"part": p, "milestone": m})).collect::<Vec<_>>(),
        "milestones": milestones,
    })
}

fn plan(parts: Vec<Value>) -> ModelResponse {
    tools(&[("plan_swarm", json!({"summary": "A test plan", "setup": {"levels": []}, "parts": parts}))])
}

/// Two parts that start at once.
fn pair() -> ModelResponse {
    plan(vec![plan_part("north", &["wall"], &[], &[]), plan_part("south", &["asset"], &[], &[])])
}

async fn main_project(app: &AppService) -> Project {
    app.session.lock().await.doc.as_ref().unwrap().project().clone()
}

fn count(p: &Project, kind: ElementKind) -> usize {
    p.elements.iter().filter(|e| e.kind() == kind).count()
}

fn usd(u: &Usage, model: &str) -> f64 {
    cost::usd(u, &cost::price_of(model).unwrap())
}

// ================================================================== the plan

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_plan_parses_and_a_bad_plan_gets_one_retry() {
    let fake = Fake::new(5, |key, round, _| match (key, round) {
        // Both parts draw walls on every level: refused, retried once.
        ("lead", 0) => ok(plan(vec![plan_part("north", &["wall"], &[], &[]), plan_part("south", &["wall"], &[], &[])])),
        ("lead", _) => ok(pair()),
        _ => ok(finish("Done")),
    });
    let (app, _dir) = rig(fake.clone()).await;
    let (before, revision) = {
        let s = app.session.lock().await;
        let doc = s.doc.as_ref().unwrap();
        (doc.project().clone(), doc.revision())
    };
    let started = start(&app, "Two parts", None).await;
    assert_eq!(started.state, SwarmState::Planning);
    assert!(started.plan.is_none(), "no plan while the lead plans");
    assert_eq!(started.parts.len(), 1, "the lead only");
    let s = until(&app, "the plan", |s| s.parts.len() == 3).await;
    assert_eq!(s.plan.as_ref().unwrap().parts.len(), 2);
    let lead = fake.of("lead");
    assert_eq!(lead.len(), 2, "one retry");
    let retry = result_block(&lead[1].request);
    assert_eq!(retry["is_error"], json!(true));
    assert!(retry["content"].as_str().unwrap().contains("could both draw the same kind"), "{retry}");
    // The plan never touched the model.
    let s = until(&app, "ready", |s| s.state == SwarmState::Ready).await;
    assert!(s.parts.iter().skip(1).all(|p| p.state == SwarmPartState::Done));
    let s2 = app.session.lock().await;
    assert_eq!(s2.doc.as_ref().unwrap().revision(), revision);
    assert_eq!(s2.doc.as_ref().unwrap().project().elements, before.elements);
}

fn result_block(request: &ModelRequest) -> Value {
    let blocks = request.messages.last().and_then(|m| m["content"].as_array()).cloned().unwrap_or_default();
    blocks.into_iter().find(|b| b["type"] == "tool_result").unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_bad_plans_fail_the_swarm_clearly() {
    let fake = Fake::new(5, |key, _, _| match key {
        "lead" => ok(ScriptedClient::text("I would rather describe the plan in words.")),
        _ => ok(finish("Done")),
    });
    let (app, _dir) = rig(fake.clone()).await;
    start(&app, "Anything", None).await;
    let s = until(&app, "failed", |s| s.state == SwarmState::Failed).await;
    assert_eq!(fake.of("lead").len(), 2);
    assert_eq!(part(&s, "lead").state, SwarmPartState::Failed);
    assert!(part(&s, "lead").status.as_deref().unwrap().contains("did not call plan_swarm"));
    // The person can discard it, and the model unlocks.
    app.handle("swarm_discard", json!({"swarm_id": s.id})).await.unwrap();
    assert!(app.swarm_status().is_none());
}

// ============================================================ the bungalow

/// Shell draws a 12 x 9 m box and a partition; Rooms, after Shell's walls,
/// puts a door and a window on Shell's walls and names the two rooms; Roof
/// after Shell is done; Furnish after Rooms is done.
fn bungalow_script(key: &str, round: usize, request: &ModelRequest) -> Result<Metered, ClientError> {
    let r = match (key, round) {
        ("lead", _) => plan(vec![
            plan_part("shell", &["wall"], &[], &["walls"]),
            plan_part("rooms", &["room", "opening"], &[("shell", "walls")], &[]),
            plan_part("roof", &["roof"], &[("shell", "done")], &[]),
            plan_part("furnish", &["asset"], &[("rooms", "done")], &[]),
        ]),
        ("shell", 0) => tools(&[(
            "add_wall_chain",
            json!({"points": [{"x": 0, "y": 0}, {"x": 12000, "y": 0}, {"x": 12000, "y": 9000}, {"x": 0, "y": 9000}], "closed": true}),
        )]),
        ("shell", 1) => tools(&[
            ("swarm_milestone", json!({"name": "walls"})),
            ("add_wall", json!({"start": {"x": 6000, "y": 0}, "end": {"x": 6000, "y": 9000}})),
        ]),
        ("shell", _) => finish("A 12 x 9 m box with one partition"),
        ("rooms", 0) => tools(&[("list_elements", json!({"kind": "wall"}))]),
        ("rooms", 1) => {
            let walls = result(request, 0)["data"]["elements"].clone();
            tools(&[
                ("add_door", json!({"wall_id": walls[0]["id"], "position": "offset", "offset_mm": 3000})),
                ("add_window", json!({"wall_id": walls[1]["id"], "position": "center"})),
            ])
        }
        ("rooms", 2) => tools(&[("list_rooms", json!({}))]),
        ("rooms", 3) => {
            let rooms = result(request, 0)["data"]["rooms"].clone();
            tools(&[
                ("rename_room", json!({"room_id": rooms[0]["id"], "name": "Living"})),
                ("rename_room", json!({"room_id": rooms[1]["id"], "name": "Bedroom"})),
            ])
        }
        ("rooms", _) => finish("Two rooms, a door and a window"),
        ("roof", 0) => tools(&[("set_roof", json!({"kind": "gable", "pitch_deg": 30, "ridge_axis": "x"}))]),
        ("roof", _) => finish("A gable roof"),
        ("furnish", 0) => tools(&[
            ("add_asset", json!({"catalog_key": "sofa-3", "position": {"x": 3000, "y": 4500}})),
            ("add_asset", json!({"catalog_key": "bed-double", "position": {"x": 9000, "y": 4500}})),
        ]),
        ("furnish", _) => finish("A sofa and a bed"),
        _ => ScriptedClient::text("?"),
    };
    ok(r)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rooms_waits_for_the_shell_walls_and_the_bungalow_is_accepted() {
    let fake = Fake::new(30, bungalow_script);
    let (app, _dir) = rig(fake.clone()).await;
    let before = main_project(&app).await;
    start(&app, "A bungalow on a 12 x 9 m footprint", None).await;
    let s = until(&app, "the plan", |s| s.parts.len() == 5).await;
    assert_eq!(part(&s, "rooms").state, SwarmPartState::Waiting, "Rooms waits for Shell's walls");
    let s = until(&app, "ready", |s| s.state == SwarmState::Ready).await;
    for p in s.parts.iter().skip(1) {
        assert_eq!(p.state, SwarmPartState::Done, "{} {:?}", p.key, p.status);
    }

    // The order the plan asked for.
    let shell = fake.of("shell");
    let first = |key: &str| fake.of(key).iter().map(|c| c.start).min().unwrap();
    assert!(first("rooms") > shell[1].end, "Rooms started after Shell reported its walls");
    assert!(first("roof") > shell.last().unwrap().end, "Roof started after Shell was done");
    assert!(first("furnish") > fake.of("rooms").last().unwrap().end, "Furnish started after Rooms was done");
    // Each round carries what the others did nearby.
    let heard = fake.of("rooms").iter().any(|c| {
        c.request.messages.last().and_then(|m| m["content"].as_array()).is_some_and(|blocks| {
            blocks.iter().any(|b| b["type"] == "text" && b["text"].as_str().is_some_and(|t| t.starts_with("Nearby:") && t.contains("Shell")))
        })
    });
    assert!(heard, "Rooms heard of Shell's partition");
    // Every worker shares one prefix: the same system prompt and tools.
    let calls = fake.calls();
    let workers: Vec<&Call> = calls.iter().filter(|c| c.key != "lead").collect();
    assert!(workers.windows(2).all(|w| w[0].request.system == w[1].request.system && w[0].request.tools == w[1].request.tools));
    assert!(!workers[0].request.system.contains("Apply"));

    // Nothing in the model yet.
    assert_eq!(main_project(&app).await.elements, before.elements);
    let draft = app.swarm_draft(None).await.unwrap().unwrap();
    let view = draft.state.project.clone();

    // Accept all: each part one undo step with its agent as author.
    let reviewed: BTreeMap<String, u32> = s.parts.iter().map(|p| (p.key.clone(), p.last_seq)).collect();
    let accepted = app.swarm_accept_all(&s.id, Some(&reviewed)).await.unwrap();
    // In the order of each part's first step; Rooms and Roof both wait only
    // for Shell, so either may draw first.
    let mut first_steps: Vec<&str> = vec![];
    for st in &draft.steps {
        if st.part != "lead" && !first_steps.contains(&st.part.as_str()) {
            first_steps.push(st.part.as_str());
        }
    }
    let order: Vec<&str> = accepted.applied.iter().map(|a| a.part.as_str()).collect();
    assert_eq!(order, first_steps, "in the order of first steps");
    assert_eq!((order[0], order[3]), ("shell", "furnish"), "{order:?}");
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(sorted, ["furnish", "roof", "rooms", "shell"]);
    let after = main_project(&app).await;
    assert_eq!(after.elements, view.elements, "the model is the draft the person saw");
    assert_eq!(count(&after, ElementKind::Room), 2);
    assert_eq!(count(&after, ElementKind::Opening), 2);
    assert_eq!(count(&after, ElementKind::Asset), 2);
    assert!(count(&after, ElementKind::Wall) >= 5);
    assert_eq!(after.top_roof().kind, RoofKind::Gable);
    let mut names: Vec<String> = after
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Room(r) => Some(r.name.clone()),
            _ => None,
        })
        .collect();
    names.sort();
    assert_eq!(names, ["Bedroom", "Living"]);
    let history = app.session.lock().await.doc.as_ref().unwrap().history();
    let steps: Vec<&HistoryStep> = history.undo.iter().take(4).collect();
    let labels: Vec<String> = steps.iter().rev().map(|h| h.label.clone()).collect();
    let expected: Vec<String> = order.iter().map(|k| format!("Swarm: {}{}", k[..1].to_uppercase(), &k[1..])).collect();
    assert_eq!(labels, expected);
    let authors: Vec<Option<&Id>> = steps.iter().map(|h| h.author_id.as_ref()).collect();
    assert!(authors.iter().all(Option::is_some));
    for (i, a) in authors.iter().enumerate() {
        assert!(!authors[i + 1..].contains(a), "each part has its own agent");
    }
    assert!(app.swarm_status().is_none(), "the swarm ended");
}

// ============================================================ concurrency

/// Parts that never finish on their own: each round reads the plan.
fn endless(key: &str, _: usize, _: &ModelRequest) -> Result<Metered, ClientError> {
    match key {
        "lead" => ok(pair()),
        _ => ok(tools(&[("get_project_summary", json!({}))])),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn workers_time_windows_overlap_and_the_first_request_goes_alone() {
    let fake = Fake::new(40, |key, round, request| match (key, round) {
        (_, 0..=3) if key != "lead" => ok(tools(&[("get_project_summary", json!({}))])),
        ("lead", _) => endless(key, round, request),
        _ => ok(finish("Done")),
    });
    let (app, _dir) = rig(fake.clone()).await;
    start(&app, "Two parts at once", None).await;
    until(&app, "ready", |s| s.state == SwarmState::Ready).await;
    let (n, s) = (fake.of("north"), fake.of("south"));
    let span = |c: &[Call]| (c.iter().map(|c| c.start).min().unwrap(), c.iter().map(|c| c.end).max().unwrap());
    let ((n0, n1), (s0, s1)) = (span(&n), span(&s));
    assert!(n0 < s1 && s0 < n1, "the two parts drew at the same time");
    // The stagger: the second worker's first request waits for the first
    // one's answer, so the cache write of the shared prefix is shared.
    let (first, second) = if n0 < s0 { (&n, &s) } else { (&s, &n) };
    assert!(second[0].start >= first[0].end, "the first worker request went out alone");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn at_most_four_requests_are_in_flight() {
    let fake = Fake::new(60, |key, round, _| match (key, round) {
        ("lead", _) => ok(plan(
            ["a", "b", "c", "d", "e", "f"].iter().zip(["wall", "opening", "room", "asset", "roof", "pipe"]).map(|(k, kind)| plan_part(k, &[kind], &[], &[])).collect(),
        )),
        (_, 0..=2) => ok(tools(&[("get_project_summary", json!({}))])),
        _ => ok(finish("Done")),
    });
    let (app, _dir) = rig(fake.clone()).await;
    start(&app, "Six parts", None).await;
    until(&app, "ready", |s| s.state == SwarmState::Ready).await;
    let most = fake.most_in_flight.load(Ordering::SeqCst);
    assert!(most <= 4, "{most} in flight");
    assert!(most >= 3, "the parts ran together ({most})");
    assert_eq!(fake.calls().len(), 1 + 6 * 4);
}

// ============================================================= the person

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stopping_one_part_ends_it_after_its_request_and_the_rest_go_on() {
    let fake = Fake::new(20, |key, round, request| match (key, round) {
        ("south", 0..=5) => ok(tools(&[("get_project_summary", json!({}))])),
        ("south", _) => ok(finish("Done")),
        _ => endless(key, round, request),
    });
    let (app, _dir) = rig(fake.clone()).await;
    start(&app, "Two parts", None).await;
    until(&app, "north drawing", |_| fake.of("north").len() >= 2).await;
    let s = ipc(&app, "swarm_stop", json!({"part": "north"})).await;
    assert_eq!(part(&s, "north").state, SwarmPartState::Stopped);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let stopped_at = fake.of("north").len();
    let s = until(&app, "ready", |s| s.state == SwarmState::Ready).await;
    assert_eq!(part(&s, "south").state, SwarmPartState::Done, "the other part went on");
    assert_eq!(part(&s, "north").state, SwarmPartState::Stopped);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(fake.of("north").len(), stopped_at, "a stopped part sends nothing more");
    // Its agent is refused from now on.
    let agent = part(&s, "north").agent_id.clone().unwrap();
    assert!(app.swarm_part_of(&agent).is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_paused_swarm_sends_no_requests() {
    let fake = Fake::new(20, endless);
    let (app, _dir) = rig(fake.clone()).await;
    start(&app, "Two parts", None).await;
    until(&app, "both drawing", |_| fake.of("north").len() >= 2 && fake.of("south").len() >= 2).await;
    let s = ipc(&app, "swarm_pause", json!({"part": null})).await;
    assert_eq!(s.state, SwarmState::Paused);
    assert!(s.parts.iter().skip(1).all(|p| p.state == SwarmPartState::Paused));
    // A request already out may come back; nothing new goes.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let held = fake.calls().len();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(fake.calls().len(), held, "paused: no requests");
    let s = ipc(&app, "swarm_resume", json!({"part": null})).await;
    assert_eq!(s.state, SwarmState::Running);
    until(&app, "more requests", |_| fake.calls().len() > held + 2).await;
    ipc(&app, "swarm_stop", json!({"part": null})).await;
    let s = until(&app, "ready", |s| s.state == SwarmState::Ready).await;
    assert!(s.parts.iter().skip(1).all(|p| p.state == SwarmPartState::Stopped));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_paused_part_waits_while_the_other_draws() {
    let fake = Fake::new(20, endless);
    let (app, _dir) = rig(fake.clone()).await;
    start(&app, "Two parts", None).await;
    until(&app, "both drawing", |_| !fake.of("north").is_empty() && !fake.of("south").is_empty()).await;
    ipc(&app, "swarm_pause", json!({"part": "north"})).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let north = fake.of("north").len();
    let south = fake.of("south").len();
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(fake.of("north").len(), north);
    assert!(fake.of("south").len() > south + 3);
    ipc(&app, "swarm_stop", json!({"part": null})).await;
}

// ================================================================= money

/// Every worker answer costs 5,000 output tokens on Claude Opus 5.5: $0.10.
fn pricey(key: &str, round: usize, request: &ModelRequest) -> Result<Metered, ClientError> {
    let mut m = endless(key, round, request)?;
    if key != "lead" {
        m.usage = Usage { output_tokens: 5000, ..Usage::default() };
    }
    Ok(m)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_cap_pauses_every_part_and_raising_it_goes_on() {
    let fake = Fake::new(20, pricey);
    let (app, _dir) = rig(fake.clone()).await;
    let started = start(&app, "Two parts", Some(0.5)).await;
    let c = started.cost.clone().unwrap();
    assert_eq!((c.model.as_str(), c.cap_usd, c.spent_usd), ("claude-opus-5-5", 0.5, 0.0));
    assert!(c.low_usd.unwrap() < c.high_usd.unwrap());
    let s = until(&app, "the cap", |s| s.state == SwarmState::Paused).await;
    assert!(s.parts.iter().skip(1).all(|p| p.state == SwarmPartState::Paused), "every part");
    assert!(s.feed.iter().any(|f| f.text.contains("cap")));
    tokio::time::sleep(Duration::from_millis(150)).await;
    let held = fake.calls().len();
    let spent = app.swarm_status().unwrap().cost.unwrap().spent_usd;
    assert!(spent <= 0.5 + 0.11 * 2.0, "at most what was in flight passes the cap: {spent}");
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(fake.calls().len(), held, "a capped swarm spends nothing");
    // Raise cap and continue.
    let s = ipc(&app, "swarm_set_cap", json!({"cap_usd": 50.0})).await;
    assert_eq!(s.state, SwarmState::Running);
    assert_eq!(s.cost.as_ref().unwrap().cap_usd, 50.0);
    until(&app, "more requests", |_| fake.calls().len() > held + 2).await;
    ipc(&app, "swarm_stop", json!({"part": null})).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn spend_equals_the_summed_usage() {
    // Varied usage, and one answer from a fallback model priced as itself.
    let fake = Fake::new(5, |key, round, _| {
        let mut m = match (key, round) {
            ("lead", _) => Metered::unmetered(pair()),
            (_, 0..=2) => Metered::unmetered(tools(&[("get_project_summary", json!({}))])),
            _ => Metered::unmetered(finish("Done")),
        };
        let n = round as u64 + if key == "north" { 1 } else { 7 };
        m.usage = Usage { input_tokens: 100 * n, output_tokens: 300 * n, cache_read_tokens: 5000 * n, cache_write_tokens: 900 * n };
        if key == "south" && round == 1 {
            m.model = Some("claude-opus-4-8".into());
        }
        Ok(m)
    });
    let (app, _dir) = rig(fake.clone()).await;
    start(&app, "Two parts", None).await;
    let s = until(&app, "ready", |s| s.state == SwarmState::Ready).await;
    let mut expected = 0.0;
    let mut tokens = Usage::default();
    for c in fake.calls() {
        let n = c.round as u64 + if c.key == "north" { 1 } else { 7 };
        let u = Usage { input_tokens: 100 * n, output_tokens: 300 * n, cache_read_tokens: 5000 * n, cache_write_tokens: 900 * n };
        let model = if c.key == "south" && c.round == 1 { "claude-opus-4-8" } else { "claude-opus-5-5" };
        expected += usd(&u, model);
        tokens.input_tokens += u.input_tokens;
        tokens.output_tokens += u.output_tokens;
        tokens.cache_read_tokens += u.cache_read_tokens;
        tokens.cache_write_tokens += u.cache_write_tokens;
    }
    let c = s.cost.unwrap();
    assert!((c.spent_usd - expected).abs() < 1e-9, "{} vs {expected}", c.spent_usd);
    assert_eq!(
        (c.input_tokens, c.output_tokens, c.cache_read_tokens, c.cache_write_tokens),
        (tokens.input_tokens, tokens.output_tokens, tokens.cache_read_tokens, tokens.cache_write_tokens)
    );
    // Every round is a line of ai-log.jsonl with its usage.
    let dir = app.project_dir().await.unwrap();
    let log = std::fs::read_to_string(dir.join("ai-log.jsonl")).unwrap();
    let rounds = log.lines().filter(|l| l.contains("\"swarm_round\"")).count();
    assert_eq!(rounds, fake.calls().len());
    assert!(log.contains("\"cache_read_tokens\""));
    assert!(!log.contains("sk-ant-test"), "the key never reaches the log");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_rate_limit_backs_off_only_that_part() {
    let limited = Arc::new(Mutex::new(None::<Instant>));
    let flag = limited.clone();
    let fake = Fake::new(20, move |key, round, _| {
        if key == "north" && round == 1 {
            let mut at = flag.lock().unwrap();
            if at.is_none() {
                *at = Some(Instant::now());
                return Err(ClientError::RateLimited { retry_after: Some(Duration::from_millis(600)), message: "slow down".into() });
            }
        }
        match (key, round) {
            ("lead", _) => ok(pair()),
            (_, 0..=4) => ok(tools(&[("get_project_summary", json!({}))])),
            _ => ok(finish("Done")),
        }
    });
    let (app, _dir) = rig(fake.clone()).await;
    start(&app, "Two parts", None).await;
    let s = until(&app, "the wait", |s| s.ai_wait_until.is_some()).await;
    assert!(part(&s, "north").status.as_deref().unwrap_or_default().contains("Waiting for the AI service"));
    assert_eq!(part(&s, "north").state, SwarmPartState::Working, "a rate limit never fails a part");
    let s = until(&app, "ready", |s| s.state == SwarmState::Ready).await;
    assert!(s.ai_wait_until.is_none());
    assert!(s.parts.iter().skip(1).all(|p| p.state == SwarmPartState::Done));
    let at = limited.lock().unwrap().unwrap();
    let back = Duration::from_millis(600);
    let north_after: Vec<Call> = fake.of("north").into_iter().filter(|c| c.start > at).collect();
    assert!(north_after.iter().all(|c| c.start >= at + back - Duration::from_millis(30)), "north waited for retry-after");
    let south_during = fake.of("south").iter().filter(|c| c.start > at && c.start < at + back).count();
    assert!(south_during >= 2, "south went on meanwhile ({south_during})");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_estimate_spends_nothing() {
    let fake = Fake::new(0, endless);
    let (app, _dir) = rig(fake.clone()).await;
    let v = app.handle("swarm_estimate", json!({"request": {"brief": "A bungalow", "review_plan": false}})).await.unwrap();
    let c: SwarmCost = serde_json::from_value(v).unwrap();
    assert_eq!(c.model, "claude-opus-5-5");
    assert_eq!(c.cap_usd, cost::DEFAULT_CAP_USD);
    assert!(c.low_usd.unwrap() > 0.0 && c.low_usd < c.high_usd);
    assert_eq!(c.spent_usd, 0.0);
    assert!(fake.calls().is_empty());
    assert!(app.swarm_status().is_none());
}

/// The estimate names the model the swarm will use, for the panel: the
/// person's copilot model, else Claude Opus 5.5.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_estimate_names_the_model() {
    let fake = Fake::new(0, endless);
    let (app, dir) = rig(fake.clone()).await;
    let estimate = |app: AppService| async move {
        let v = app.handle("swarm_estimate", json!({"request": {"brief": "A bungalow", "review_plan": false}})).await.unwrap();
        serde_json::from_value::<SwarmCost>(v).unwrap()
    };
    let c = estimate(app.clone()).await;
    assert_eq!((c.model.as_str(), c.model_label.as_deref(), c.price_note.as_deref()), ("claude-opus-5-5", Some("Claude Opus 5.5"), None));
    std::fs::write(dir.path().join("settings.json"), json!({"ai_model": "claude-sonnet-5-5"}).to_string()).unwrap();
    let c = estimate(app.clone()).await;
    assert_eq!((c.model.as_str(), c.model_label.as_deref()), ("claude-sonnet-5-5", Some("Claude Sonnet 5.5")));
    // A model the table does not know is estimated at its highest row, and
    // says so.
    std::fs::write(dir.path().join("settings.json"), json!({"ai_model": "claude-opus-5-5-20260401"}).to_string()).unwrap();
    let c = estimate(app.clone()).await;
    assert_eq!(c.model_label.as_deref(), Some("claude-opus-5-5-20260401"));
    assert_eq!(c.price_note.as_deref(), Some(cost::PRICE_UNKNOWN));
    assert!(c.high_usd.unwrap() > 0.0);
    assert!(fake.calls().is_empty());
}

/// Gate P0-1 (the reviewer's repro): every worker round reports 1M output
/// tokens. On a model the price table knows the cap pauses the swarm after
/// one request; a model id it does not know (a dated id) sent 7 requests,
/// about US$140, past a US$0.50 cap. Now it is billed at the table's highest
/// row and pauses at the cap too.
async fn run_with_model(model: &str) -> (usize, SwarmStatus) {
    let fake = Fake::new(5, |key, round, _| {
        let response = match (key, round) {
            ("lead", _) => return ok(plan(vec![plan_part("north", &["wall"], &[], &[])])),
            (_, r) if r < 6 => tools(&[("set_status", json!({"text": format!("round {r}")}))]),
            _ => finish("done"),
        };
        Ok(Metered { response, usage: Usage { input_tokens: 1000, output_tokens: 1_000_000, ..Usage::default() }, model: None })
    });
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("settings.json"), json!({"ai_model": model}).to_string()).unwrap();
    let mut app = AppService::new_sandboxed(dir.path().to_path_buf());
    app.ai = Arc::new(AiState::with_parts(fake.clone(), Box::new(MemoryKeyStore::with_key("sk-ant-test-not-a-key"))));
    app.handle("hub_create", json!({"name": "Cap", "template": "blank"})).await.unwrap();
    start(&app, "A wall", Some(0.5)).await;
    let s = until(&app, "the cap or the end", |s| {
        s.state == SwarmState::Ready || s.parts.iter().any(|p| p.key == "north" && p.state == SwarmPartState::Paused)
    })
    .await;
    let requests = fake.of("north").len();
    ipc(&app, "swarm_stop", json!({"part": null})).await;
    (requests, s)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_cap_holds_whatever_model_the_person_typed() {
    let (priced, s) = run_with_model("claude-opus-5-5").await;
    let c = s.cost.clone().unwrap();
    println!("claude-opus-5-5: north requests {priced}, state {:?}, spent US${:.2} of US${:.2}", s.state, c.spent_usd, c.cap_usd);
    let (unpriced, s) = run_with_model("claude-opus-5-5-20260401").await;
    let c = s.cost.clone().unwrap();
    println!(
        "claude-opus-5-5-20260401: north requests {unpriced}, state {:?}, spent US${:.2} of US${:.2}, output tokens {}, note {:?}",
        s.state, c.spent_usd, c.cap_usd, c.output_tokens, c.price_note
    );
    assert!(unpriced <= priced, "an unpriced model sent {unpriced} requests past a US$0.50 cap; a priced one stopped at {priced}");
    assert_eq!(s.state, SwarmState::Paused, "the cap paused it");
    assert!(c.spent_usd > c.cap_usd, "billed at the highest row: US${}", c.spent_usd);
    assert_eq!(c.price_note.as_deref(), Some(cost::PRICE_UNKNOWN));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_draft_open_refuses_a_second_start_and_the_lock_holds_while_planning() {
    let fake = Fake::new(300, endless);
    let (app, _dir) = rig(fake.clone()).await;
    start(&app, "Two parts", None).await;
    let again = app.handle("swarm_start", json!({"request": {"brief": "More", "review_plan": false}})).await.unwrap_err();
    assert_eq!(again.code, "draft_open");
    // The model is locked from the start, while the lead plans.
    let wall = json!({"type": "add_wall", "start": {"x": 0.0, "y": 0.0}, "end": {"x": 1000.0, "y": 0.0},
        "thickness_mm": null, "height_mm": null, "material_id": null, "level_id": null});
    let refused = app.handle("doc_apply", json!({"command": wall})).await.unwrap_err();
    assert_eq!(refused.code, "swarm_running");
    ipc(&app, "swarm_stop", json!({"part": null})).await;
}

// ================================================================ fixture

/// One dev script client per conversation, as `GUHIT_AI_SCRIPT` gives the
/// app: each picks its turn by the conversation's first message.
struct ScriptRouter {
    script: Value,
    clients: Mutex<BTreeMap<String, Arc<ScriptedClient>>>,
}

impl ScriptRouter {
    fn client(&self, request: &ModelRequest) -> Arc<ScriptedClient> {
        let first = request.messages.first().and_then(|m| m["content"].as_str()).unwrap_or_default().to_string();
        let mut clients = self.clients.lock().unwrap();
        clients
            .entry(first.clone())
            .or_insert_with(|| {
                let c = ScriptedClient::from_script(&self.script, &first).expect("a turn for every conversation");
                Arc::new(c.with_delay(Duration::from_millis(5)))
            })
            .clone()
    }
}

impl ModelClient for ScriptRouter {
    fn send<'a>(&'a self, request: &'a ModelRequest) -> BoxFut<'a, Result<ModelResponse, IpcError>> {
        Box::pin(async move { self.client(request).send(request).await })
    }

    fn send_once<'a>(&'a self, request: &'a ModelRequest) -> BoxFut<'a, Result<Metered, ClientError>> {
        Box::pin(async move { self.client(request).send_once(request).await })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_bungalow_fixture_draws_a_bungalow_that_accepts() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/swarm/bungalow.json")).unwrap();
    let script: Value = serde_json::from_str(&text).unwrap();
    let router = Arc::new(ScriptRouter { script, clients: Mutex::new(BTreeMap::new()) });
    let dir = tempfile::tempdir().unwrap();
    let mut app = AppService::new_sandboxed(dir.path().to_path_buf());
    app.ai = Arc::new(AiState::with_parts(router, Box::new(MemoryKeyStore::with_key("sk-ant-test-not-a-key"))));
    app.handle("hub_create", json!({"name": "Fixture", "template": "blank"})).await.unwrap();
    start(&app, "A three bedroom bungalow, 12 x 9 m", None).await;
    let s = until(&app, "ready", |s| s.state == SwarmState::Ready).await;
    for p in s.parts.iter().skip(1) {
        assert_eq!(p.state, SwarmPartState::Done, "{} {:?}", p.key, p.status);
        assert!(p.steps > 0, "{} drew", p.key);
    }
    assert!(s.cost.as_ref().unwrap().spent_usd > 0.0, "the fixture reports usage");
    let reviewed: BTreeMap<String, u32> = s.parts.iter().map(|p| (p.key.clone(), p.last_seq)).collect();
    let accepted = app.swarm_accept_all(&s.id, Some(&reviewed)).await.unwrap();
    // In the order of each part's first step: Rooms and Roof start together.
    let mut parts: Vec<&str> = accepted.applied.iter().map(|a| a.part.as_str()).collect();
    assert_eq!((parts[0], parts[3]), ("shell", "furnish"));
    parts.sort();
    assert_eq!(parts, ["furnish", "roof", "rooms", "shell"]);
    let p = main_project(&app).await;
    let counts = [ElementKind::Wall, ElementKind::Room, ElementKind::Opening, ElementKind::Asset].map(|k| count(&p, k));
    println!("bungalow fixture: walls {}, rooms {}, openings {}, assets {}", counts[0], counts[1], counts[2], counts[3]);
    assert_eq!(counts, [8, 5, 10, 8], "walls, rooms, openings, assets");
    assert_eq!(p.top_roof().kind, RoofKind::Gable);
    let mut names: Vec<String> = p
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Room(r) => Some(format!("{} {:?}", r.name, r.usage)),
            _ => None,
        })
        .collect();
    names.sort();
    assert_eq!(names, ["Bedroom 1 Bedroom", "Bedroom 2 Bedroom", "Kitchen Kitchen", "Living Living", "Master Bedroom MasterBedroom"]);
}

/// Accept all commits the parts in their plan's order: every part after
/// the parts its `after` names (Rooms after Shell, Roof after Shell,
/// Furnish after Rooms), in the accept's answer and in the model's history.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn accept_all_commits_the_bungalow_parts_in_their_plan_order() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/swarm/bungalow.json")).unwrap();
    let script: Value = serde_json::from_str(&text).unwrap();
    let router = Arc::new(ScriptRouter { script, clients: Mutex::new(BTreeMap::new()) });
    let dir = tempfile::tempdir().unwrap();
    let mut app = AppService::new_sandboxed(dir.path().to_path_buf());
    app.ai = Arc::new(AiState::with_parts(router, Box::new(MemoryKeyStore::with_key("sk-ant-test-not-a-key"))));
    app.handle("hub_create", json!({"name": "Order", "template": "blank"})).await.unwrap();
    start(&app, "A three bedroom bungalow, 12 x 9 m", None).await;
    let s = until(&app, "ready", |s| s.state == SwarmState::Ready).await;
    let plan = s.plan.clone().unwrap();
    let draft = app.swarm_draft(None).await.unwrap().unwrap();
    let first = |key: &str| draft.steps.iter().find(|st| st.part == key).map(|st| st.seq);
    println!("first steps: {:?}", ["shell", "rooms", "roof", "furnish"].map(|k| (k, first(k))));
    let reviewed: BTreeMap<String, u32> = s.parts.iter().map(|p| (p.key.clone(), p.last_seq)).collect();
    let accepted = app.swarm_accept_all(&s.id, Some(&reviewed)).await.unwrap();
    let order: Vec<&str> = accepted.applied.iter().map(|a| a.part.as_str()).collect();
    println!("commit order: {order:?}");
    let at = |key: &str| order.iter().position(|k| *k == key).unwrap_or_else(|| panic!("{key} committed: {order:?}"));
    for p in &plan.parts {
        for w in &p.after {
            assert!(at(&w.part) < at(&p.key), "{} commits after {}, which it waits for: {order:?}", p.key, w.part);
        }
    }
    assert!(at("rooms") < at("furnish"), "{order:?}");
    // The history, newest first, is the same order backwards.
    let history: HistoryList = serde_json::from_value(app.handle("history_list", json!({})).await.unwrap()).unwrap();
    let mut labels: Vec<String> = history.undo.iter().filter_map(|st| st.label.strip_prefix("Swarm: ").map(str::to_lowercase)).collect();
    labels.reverse();
    assert_eq!(labels, order, "the history, oldest first");
}
