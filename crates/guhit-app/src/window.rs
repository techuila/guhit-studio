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

struct Job {
    started: Instant,
    finished: Option<Instant>,
    result: Option<Result<WindowReply, IpcError>>,
    done: Arc<Notify>,
}

#[derive(Default)]
pub struct WindowState {
    jobs: Mutex<HashMap<Id, Job>>,
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
                Job { started: Instant::now(), finished: None, result: None, done: Arc::new(Notify::new()) },
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

pub const OWNS: &[&str] = &["window_reply"];

pub async fn handle(app: &AppService, cmd: &str, args: Value) -> IpcResult {
    match cmd {
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
            .window_start(WindowTask::Render { views: vec![], quality: TraceQuality::Quick, size: RenderSize::Hd })
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
}
