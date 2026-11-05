//! Window requests (DECISIONS D31, docs/CONTRACT.md "App events and window
//! requests"). The path tracer and the plan canvas live in the webview, so
//! when the backend needs a render or a picture of the plan (an MCP tool asked
//! for one), it asks the window with `AppEvent::WindowRequest` and waits for
//! its `window_reply`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use guhit_model::*;
use serde_json::Value;
use tokio::sync::Notify;

use crate::{arg, AppService, IpcResult};

/// A finished request is kept this long, so a caller that comes back late
/// (an MCP client polling a render job) still gets the answer.
const KEEP_FINISHED: Duration = Duration::from_secs(10 * 60);
/// A request no window has answered after this long is given up.
const GIVE_UP: Duration = Duration::from_secs(30 * 60);

/// What the window last said it is doing is believed this long; after
/// that it is reported as not heard from.
const STATUS_FRESH: Duration = Duration::from_secs(30);

struct Job {
    started: Instant,
    finished: Option<Instant>,
    result: Option<Result<WindowReply, IpcError>>,
    done: Arc<Notify>,
    /// What was asked, in words: "render 2 views, quick, hd".
    what: String,
}

/// What the window says its render queue is doing (G4 problem 8): sent by
/// the window with `window_status` when a render starts, as it goes and
/// when it ends, whoever started it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WindowActivity {
    pub busy: bool,
    /// "Rendering", "Exporting a tour".
    #[serde(default)]
    pub what: String,
    /// The view or shot it is on.
    #[serde(default)]
    pub view: Option<String>,
    /// Which of how many, from 1.
    #[serde(default)]
    pub index: Option<u32>,
    #[serde(default)]
    pub total: Option<u32>,
    #[serde(default)]
    pub progress_pct: Option<f64>,
    #[serde(default)]
    pub eta_s: Option<f64>,
    /// The window request it answers, when an MCP client asked for it.
    #[serde(default)]
    pub job_id: Option<Id>,
}

#[derive(Default)]
pub struct WindowState {
    jobs: Mutex<HashMap<Id, Job>>,
    activity: Mutex<Option<(WindowActivity, Instant)>>,
}

/// A task in words, for the queue's report.
fn task_words(task: &WindowTask) -> String {
    let size = |s: &RenderSize| serde_json::to_value(s).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    match task {
        WindowTask::CapturePlan { .. } => "a picture of the plan".into(),
        WindowTask::CaptureView { .. } => "a capture of the 3D view".into(),
        WindowTask::Render { views, quality, size: s, .. } => {
            let q = serde_json::to_value(quality).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            let what = match views.len() {
                0 => "the view on screen".to_string(),
                1 => "1 saved view".to_string(),
                n => format!("{n} saved views"),
            };
            format!("a render of {what}, {q}, {}", size(s))
        }
        WindowTask::CaptureMaps { .. } => "the conditioning maps of a render".into(),
        WindowTask::ExportTour { size: s, .. } => format!("a tour video export, {}", size(s)),
    }
}

enum Checked {
    Answered(WindowReply),
    Waiting(Arc<Notify>),
}

fn no_window(message: &str) -> IpcError {
    IpcError::new("no_window", message)
}

impl WindowState {
    fn prune(jobs: &mut HashMap<Id, Job>) {
        let now = Instant::now();
        jobs.retain(|_, j| match j.finished {
            Some(at) => now.duration_since(at) < KEEP_FINISHED,
            None => now.duration_since(j.started) < GIVE_UP,
        });
    }
}

impl AppService {
    /// Ask the window to do `task`. Returns the request id at once; the
    /// answer comes with `window_wait`. Fails with `no_window` when nothing
    /// is listening for app events (the dev bridge with no browser open).
    pub fn window_start(&self, task: WindowTask) -> Result<Id, IpcError> {
        if self.event_listeners() == 0 {
            return Err(no_window(
                "No Guhit Studio window is open to do this. Open the app (or the browser UI on the dev bridge) and try again.",
            ));
        }
        let id = defaults::new_id();
        {
            let mut jobs = self.window.jobs.lock().unwrap_or_else(|e| e.into_inner());
            WindowState::prune(&mut jobs);
            jobs.insert(
                id.clone(),
                Job { started: Instant::now(), finished: None, result: None, done: Arc::new(Notify::new()), what: task_words(&task) },
            );
        }
        self.emit(AppEvent::WindowRequest { request: WindowRequest { id: id.clone(), task } });
        Ok(id)
    }

    /// Wait up to `timeout` for the answer to a request `window_start` made.
    /// `Ok(None)`: the window is still working on it. An unknown id (never
    /// made, or answered more than 10 minutes ago) is `not_found`.
    pub async fn window_wait(&self, id: &str, timeout: Duration) -> Result<Option<WindowReply>, IpcError> {
        let deadline = Instant::now() + timeout;
        loop {
            let done = match self.window_check(id)? {
                Checked::Answered(reply) => return Ok(Some(reply)),
                Checked::Waiting(done) => done,
            };
            // Listen first, then look again: an answer that lands in between
            // is seen by the second look instead of being missed.
            let woken = done.notified();
            tokio::pin!(woken);
            woken.as_mut().enable();
            if let Checked::Answered(reply) = self.window_check(id)? {
                return Ok(Some(reply));
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            // Woken by `window_reply`, or wakes at the deadline to report
            // that the work is still going.
            let _ = tokio::time::timeout(deadline - now, woken).await;
        }
    }

    /// Where one request stands.
    fn window_check(&self, id: &str) -> Result<Checked, IpcError> {
        let mut jobs = self.window.jobs.lock().unwrap_or_else(|e| e.into_inner());
        let Some(job) = jobs.get(id) else {
            return Err(IpcError::new("not_found", format!("no window request has id `{id}`")));
        };
        if let Some(result) = &job.result {
            return result.clone().map(Checked::Answered);
        }
        if job.started.elapsed() >= GIVE_UP {
            jobs.remove(id);
            return Err(no_window("No window answered this request. Open the app window and try again."));
        }
        Ok(Checked::Waiting(job.done.clone()))
    }

    /// `window_start` and `window_wait` in one call, for quick tasks: no
    /// answer within `timeout` is `no_window`.
    pub async fn window_request(&self, task: WindowTask, timeout: Duration) -> Result<WindowReply, IpcError> {
        let id = self.window_start(task)?;
        match self.window_wait(&id, timeout).await? {
            Some(reply) => Ok(reply),
            None => {
                let mut jobs = self.window.jobs.lock().unwrap_or_else(|e| e.into_inner());
                jobs.remove(&id);
                Err(no_window(&format!(
                    "The window did not answer within {} s. Make sure Guhit Studio shows the project, then try again.",
                    timeout.as_secs()
                )))
            }
        }
    }

    /// What the window's render queue is doing, as the window last said
    /// (G4 problem 8), with the requests it has not answered yet. None when
    /// the window never said and nothing waits.
    pub fn window_activity(&self) -> Option<Value> {
        let waiting: Vec<Value> = {
            let mut jobs = self.window.jobs.lock().unwrap_or_else(|e| e.into_inner());
            WindowState::prune(&mut jobs);
            let mut list: Vec<(&Id, &Job)> = jobs.iter().filter(|(_, j)| j.result.is_none()).collect();
            list.sort_by_key(|(_, j)| j.started);
            list.iter()
                .map(|(id, j)| serde_json::json!({"job_id": id, "what": j.what, "running_s": j.started.elapsed().as_secs()}))
                .collect()
        };
        let said = self.window.activity.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let (state, line) = match said {
            Some((a, at)) if at.elapsed() < STATUS_FRESH || !a.busy => {
                let line = activity_line(&a);
                (Some(a), line)
            }
            Some((a, at)) => (
                Some(a),
                format!("The window said it was busy {} s ago and has not said since; it may still be working.", at.elapsed().as_secs()),
            ),
            None if waiting.is_empty() => return None,
            None => (None, "The window has not reported its render queue.".to_string()),
        };
        Some(serde_json::json!({"busy": state.as_ref().is_some_and(|a| a.busy), "status": line, "state": state, "waiting": waiting}))
    }

    /// The window's report of its render queue (`window_status`).
    fn window_status(&self, activity: WindowActivity) {
        *self.window.activity.lock().unwrap_or_else(|e| e.into_inner()) = Some((activity, Instant::now()));
    }

    /// The window's answer. The first answer wins; a late or unknown one is
    /// `not_found`, which the window ignores.
    fn window_reply(&self, id: &str, result: Result<WindowReply, IpcError>) -> Result<(), IpcError> {
        let mut jobs = self.window.jobs.lock().unwrap_or_else(|e| e.into_inner());
        let Some(job) = jobs.get_mut(id).filter(|j| j.result.is_none()) else {
            return Err(IpcError::new("not_found", format!("no window request is waiting with id `{id}`")));
        };
        job.result = Some(result);
        job.finished = Some(Instant::now());
        job.done.notify_waiters();
        Ok(())
    }
}

/// "Rendering Front, view 1 of 2, 75%, about 30 s left", or "Idle".
pub fn activity_line(a: &WindowActivity) -> String {
    if !a.busy {
        return "The window's render queue is idle.".to_string();
    }
    let mut parts = vec![if a.what.trim().is_empty() { "Busy".to_string() } else { a.what.trim().to_string() }];
    if let Some(v) = a.view.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        parts[0] = format!("{} {v}", parts[0]);
    }
    if let (Some(i), Some(n)) = (a.index, a.total) {
        if n > 1 {
            parts.push(format!("{i} of {n}"));
        }
    }
    if let Some(p) = a.progress_pct.filter(|p| p.is_finite()) {
        parts.push(format!("{:.0}%", p.clamp(0.0, 100.0)));
    }
    if let Some(s) = a.eta_s.filter(|s| s.is_finite() && *s >= 0.0) {
        parts.push(format!("about {:.0} s left", s.ceil()));
    }
    format!("{}.", parts.join(", "))
}

pub const OWNS: &[&str] = &["window_reply", "window_status"];

pub async fn handle(app: &AppService, cmd: &str, args: Value) -> IpcResult {
    match cmd {
        // The window's render queue, as it goes (G4 problem 8).
        "window_status" => {
            let activity: WindowActivity =
                serde_json::from_value(args).map_err(|e| IpcError::new("bad_args", format!("window_status: {e}")))?;
            app.window_status(activity);
            Ok(Value::Null)
        }
        "window_reply" => {
            let id: String = arg(&args, "id")?;
            let reply: Option<WindowReply> = arg(&args, "reply")?;
            let error: Option<IpcError> = arg(&args, "error")?;
            let result = match (reply, error) {
                (_, Some(e)) => Err(e),
                (Some(r), None) => Ok(r),
                (None, None) => Err(IpcError::new("bad_args", "window_reply needs `reply` or `error`")),
            };
            app.window_reply(&id, result)?;
            Ok(Value::Null)
        }
        _ => Err(IpcError::new("unknown_command", format!("unknown command `{cmd}`"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn app() -> (AppService, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (AppService::new_sandboxed(dir.path().to_path_buf()), dir)
    }

    #[tokio::test]
    async fn without_a_listening_window_a_request_fails_at_once() {
        let (app, _dir) = app();
        let err = app.window_start(WindowTask::CapturePlan { level_id: None }).unwrap_err();
        assert_eq!(err.code, "no_window");
    }

    #[tokio::test]
    async fn the_window_answers_and_the_first_answer_wins() {
        let (app, _dir) = app();
        let mut events = app.events();
        let waiter = {
            let app = app.clone();
            tokio::spawn(async move {
                app.window_request(WindowTask::CapturePlan { level_id: None }, Duration::from_secs(5)).await
            })
        };
        let request = loop {
            if let AppEvent::WindowRequest { request } = events.recv().await.unwrap() {
                break request;
            }
        };
        assert_eq!(request.task, WindowTask::CapturePlan { level_id: None });
        let reply = json!({"render_ids": [], "image": "data:image/png;base64,AAAA", "note": "plan"});
        handle(&app, "window_reply", json!({"id": request.id, "reply": reply})).await.unwrap();
        let second = handle(&app, "window_reply", json!({"id": request.id, "reply": reply})).await;
        assert_eq!(second.unwrap_err().code, "not_found");
        let got = waiter.await.unwrap().unwrap();
        assert_eq!(got.image.as_deref(), Some("data:image/png;base64,AAAA"));
    }

    #[tokio::test]
    async fn a_long_job_reports_running_then_its_error() {
        let (app, _dir) = app();
        let _events = app.events();
        let id = app
            .window_start(WindowTask::Render { views: vec![], quality: TraceQuality::Quick, size: RenderSize::Hd, visibility: None })
            .unwrap();
        assert_eq!(app.window_wait(&id, Duration::from_millis(20)).await.unwrap(), None);
        handle(
            &app,
            "window_reply",
            json!({"id": id, "error": {"code": "no_document", "message": "no project is open", "element_ids": []}}),
        )
        .await
        .unwrap();
        let err = app.window_wait(&id, Duration::from_millis(20)).await.unwrap_err();
        assert_eq!(err.code, "no_document");
        assert_eq!(app.window_wait("nope", Duration::from_millis(1)).await.unwrap_err().code, "not_found");
    }

    /// G4 problem 8: the render queue says what it is doing, and what it
    /// was asked and has not answered yet.
    #[tokio::test]
    async fn the_render_queue_reports_what_it_is_doing() {
        let (app, _dir) = app();
        let _events = app.events();
        assert_eq!(app.window_activity(), None, "nothing said, nothing waiting");
        let id = app
            .window_start(WindowTask::Render { views: vec!["a".into(), "b".into()], quality: TraceQuality::Quick, size: RenderSize::Hd, visibility: None })
            .unwrap();
        let waiting = app.window_activity().unwrap();
        assert_eq!(waiting["waiting"][0]["job_id"], id.as_str());
        assert_eq!(waiting["waiting"][0]["what"], "a render of 2 saved views, quick, hd");
        let status = json!({"busy": true, "what": "Rendering", "view": "Front", "index": 1, "total": 2, "progress_pct": 75.4, "eta_s": 29.2, "job_id": id});
        handle(&app, "window_status", status).await.unwrap();
        let now = app.window_activity().unwrap();
        assert_eq!(now["busy"], true);
        assert_eq!(now["status"], "Rendering Front, 1 of 2, 75%, about 30 s left.");
        handle(&app, "window_status", json!({"busy": false})).await.unwrap();
        assert_eq!(app.window_activity().unwrap()["status"], "The window's render queue is idle.");
        assert_eq!(handle(&app, "window_status", json!({"busy": "yes"})).await.unwrap_err().code, "bad_args");
    }
}
