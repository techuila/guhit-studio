//! Application service. Everything the UI can ask for goes through
//! `AppService::handle(cmd, args)`. The Tauri shell and the dev HTTP bridge
//! are thin transports over this one function, so the whole app can run and
//! be tested in a plain browser against the real Rust engine.
//!
//! The command list is in `docs/CONTRACT.md`.
//!
//! The backend agent owns this crate except `src/ai/` (the copilot) and
//! `src/render_ai/` (Tier 2 image generation), which the AI agents own.

pub mod ai;
pub mod files;
pub mod interop;
pub mod live;
pub mod render_ai;
pub mod renders;
pub mod snapshots;
pub mod store;
pub mod window;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use guhit_core::Document;
use guhit_model::*;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::{broadcast, watch, Mutex};

use files::ImageKind;

pub type IpcResult = Result<Value, IpcError>;

const MAX_PROJECT_NAME_CHARS: usize = 120;
/// App events buffered per receiver before a slow one starts skipping.
const EVENT_BUFFER: usize = 1024;

/// Mutable session state, shared with the AI module.
pub struct Session {
    /// Root folder for all app data (projects, exports, logs).
    pub data_dir: PathBuf,
    /// The open document, if any. One document per app window.
    pub doc: Option<Document>,
    /// True when the open document has changes that are not on disk yet.
    /// Only stays true after a failed autosave; the next save retries.
    pub dirty: bool,
    /// Document revision at the last automatic snapshot.
    pub last_auto_snapshot_revision: u32,
    /// Set while the open document is this computer's copy of a project
    /// another computer hosts in a live session (DECISIONS D29): read-only,
    /// never saved to `projects/`, its renders and exports in
    /// `live/<project-id>/`. Edits go to the host.
    pub(crate) live_copy: Option<live::CopyMeta>,
    /// The last shared project, kept after its session ends for
    /// `live_save_copy`, until another project opens.
    pub(crate) last_shared: Option<Project>,
}

/// What the open document looks like after a change. Sent to watchers so a
/// window that did not make the change can refresh. CONTRACT: the desktop
/// shell forwards this as the Tauri event `doc_changed`, and the frontend
/// subscribes with `onDocChanged` in `src/contract/ipc.ts`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DocChange {
    /// Revision of the open document. 0 when no project is open.
    pub revision: u32,
    /// The open project, or None when the hub is showing.
    pub project_id: Option<Id>,
    /// Strictly increasing. Every notification changes it, so a watcher wakes
    /// up even when two projects happen to share a revision number.
    pub seq: u64,
}

#[derive(Clone)]
pub struct AppService {
    pub session: Arc<Mutex<Session>>,
    pub ai: Arc<ai::AiState>,
    /// Tier 2 image provider and its key (DECISIONS D17). Separate from `ai`:
    /// the copilot and the visualizer use different vendors and keys.
    pub render_ai: Arc<render_ai::RenderAiState>,
    /// True when a caller may name the export file itself. The desktop app
    /// says yes: the path comes from the native save dialog. The dev bridge
    /// says no, because any local program can post to it.
    pub(crate) external_paths: bool,
    /// Fires after every commit, undo, redo, open, close and restore.
    changes: Arc<watch::Sender<DocChange>>,
    change_seq: Arc<AtomicU64>,
    /// Pushed to the window. CONTRACT: the desktop shell emits each one as
    /// the Tauri event `app_event`, the dev bridge on `GET /events`.
    events: broadcast::Sender<AppEvent>,
    /// Requests waiting for the window's answer (DECISIONS D31).
    pub(crate) window: Arc<window::WindowState>,
    /// Live session, this window's presence (DECISIONS D29).
    pub(crate) live: Arc<live::LiveState>,
    /// True when a live session may listen on every network interface (the
    /// desktop app). False keeps it on loopback (the dev bridge).
    pub(crate) lan: bool,
}

/// Deserialize one named argument out of the args object.
pub fn arg<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, IpcError> {
    let v = args.get(name).cloned().unwrap_or(Value::Null);
    serde_json::from_value(v).map_err(|e| IpcError::new("bad_args", format!("argument `{name}`: {e}")))
}

pub fn to_value<T: serde::Serialize>(v: &T) -> IpcResult {
    serde_json::to_value(v).map_err(|e| IpcError::new("io", e.to_string()))
}

pub(crate) fn no_document() -> IpcError {
    IpcError::new("no_document", "no project is open")
}

fn clean_project_name(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_PROJECT_NAME_CHARS)
        .collect()
}

impl Session {
    pub(crate) fn open_doc(&self) -> Result<&Document, IpcError> {
        self.doc.as_ref().ok_or_else(no_document)
    }

    /// Folder of the open project: its own in `projects/`, or on a live
    /// session guest the folder for its copy of the shared project.
    pub(crate) fn open_project_dir(&self) -> Result<PathBuf, IpcError> {
        let doc = self.open_doc()?;
        if self.live_copy.is_some() {
            return AppService::live_dir(&self.data_dir, &doc.project().id);
        }
        store::project_dir(&self.data_dir, &doc.project().id)
    }

    /// Autosave: write the open project to `project.json` and stamp `updated_at`.
    /// A live session guest's copy is never written: the project is the host's.
    fn save(&mut self) -> Result<(), IpcError> {
        let Some(doc) = self.doc.as_ref() else {
            return Ok(());
        };
        if self.live_copy.is_some() {
            self.dirty = false;
            return Ok(());
        }
        let mut project = doc.project().clone();
        project.updated_at = defaults::now_rfc3339();
        match store::save_project(&self.data_dir, &project) {
            Ok(()) => {
                self.dirty = false;
                Ok(())
            }
            Err(e) => {
                self.dirty = true;
                Err(e)
            }
        }
    }

    pub(crate) fn flush(&mut self) -> Result<(), IpcError> {
        if self.dirty {
            self.save()
        } else {
            Ok(())
        }
    }

    /// Save after a change, then take an automatic snapshot when one is due.
    fn after_change(&mut self) -> Result<(), IpcError> {
        self.save().map_err(|e| {
            IpcError::new(
                "io",
                format!("the change was applied but could not be saved: {}", e.message),
            )
        })?;
        let Some(doc) = self.doc.as_ref() else {
            return Ok(());
        };
        let revision = doc.revision();
        if revision.saturating_sub(self.last_auto_snapshot_revision) >= snapshots::AUTO_EVERY_REVISIONS {
            self.auto_snapshot(&format!("Auto: revision {revision}"));
        }
        Ok(())
    }

    /// Best effort. A failed automatic snapshot never fails the user's action.
    fn auto_snapshot(&mut self, label: &str) {
        let Some(doc) = self.doc.as_ref() else {
            return;
        };
        let project = doc.project().clone();
        let revision = doc.revision();
        let Ok(dir) = store::project_dir(&self.data_dir, &project.id) else {
            return;
        };
        match snapshots::create(&dir, &project, revision, label, true) {
            Ok(_) => self.last_auto_snapshot_revision = revision,
            Err(e) => eprintln!("guhit-app: automatic snapshot failed: {}", e.message),
        }
    }

    /// Replace the open document. The previous one is flushed first.
    pub(crate) fn set_doc(&mut self, project: Project) -> Result<(), IpcError> {
        self.flush()?;
        self.doc = Some(Document::new(project));
        self.dirty = false;
        self.last_auto_snapshot_revision = 0;
        self.live_copy = None;
        self.last_shared = None;
        Ok(())
    }

    fn close_doc(&mut self) -> Result<(), IpcError> {
        self.flush()?;
        self.doc = None;
        self.dirty = false;
        self.last_auto_snapshot_revision = 0;
        self.live_copy = None;
        Ok(())
    }

    /// Where an export goes when the caller names no path: `<data>/exports/`,
    /// or on a live session guest `exports/` in its folder for the shared
    /// project.
    pub(crate) fn exports_dir(&self) -> PathBuf {
        if self.live_copy.is_some() {
            if let Ok(dir) = self.open_project_dir() {
                return dir.join("exports");
            }
        }
        store::exports_dir(&self.data_dir)
    }

    /// A live session guest opens its copy of the shared project in place of
    /// the open one, which is flushed first.
    pub(crate) fn install_copy(&mut self, copy: Document, meta: live::CopyMeta) -> Result<(), IpcError> {
        self.flush()?;
        self.doc = Some(copy);
        self.dirty = false;
        self.last_auto_snapshot_revision = 0;
        self.live_copy = Some(meta);
        self.last_shared = None;
        Ok(())
    }

    /// The guest's session is over: the copy closes and is kept for
    /// `live_save_copy`.
    pub(crate) fn close_copy(&mut self) {
        self.last_shared = self.doc.take().map(|d| d.project().clone());
        self.live_copy = None;
        self.dirty = false;
        self.last_auto_snapshot_revision = 0;
    }

    /// True when `id` is the open project of this computer. A live session
    /// guest's copy is not one: its project lives on the host.
    fn is_open(&self, id: &str) -> bool {
        self.live_copy.is_none() && self.doc.as_ref().is_some_and(|d| d.project().id == id)
    }

    /// The open document's state. A live session guest's copy reports the
    /// host's history: whether there is a step to undo or redo, its label and
    /// who made it.
    pub(crate) fn state(&self) -> Result<DocState, IpcError> {
        let mut state = self.open_doc()?.state();
        if let Some(copy) = &self.live_copy {
            copy.undo.apply_to(&mut state);
        }
        Ok(state)
    }

    /// Revision and project id of the open document. `(0, None)` when none is
    /// open. Cheap: no project data is cloned.
    fn revision(&self) -> (u32, Option<Id>) {
        match self.doc.as_ref() {
            Some(d) => (d.revision(), Some(d.project().id.clone())),
            None => (0, None),
        }
    }
}

impl AppService {
    /// Service for the desktop shell: a caller-supplied export path is used
    /// as given, because it comes from the native save dialog.
    pub fn new(data_dir: PathBuf) -> Self {
        Self::build(data_dir, true)
    }

    /// Service for the dev HTTP bridge: an export path outside the data dir
    /// is refused, so a local caller cannot write anywhere on the machine.
    pub fn new_sandboxed(data_dir: PathBuf) -> Self {
        Self::build(data_dir, false)
    }

    fn build(data_dir: PathBuf, external_paths: bool) -> Self {
        let (changes, _) = watch::channel(DocChange { revision: 0, project_id: None, seq: 0 });
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        Self {
            ai: Arc::new(ai::AiState::for_data_dir(&data_dir)),
            render_ai: Arc::new(render_ai::RenderAiState::for_data_dir(&data_dir)),
            session: Arc::new(Mutex::new(Session {
                data_dir,
                doc: None,
                dirty: false,
                last_auto_snapshot_revision: 0,
                live_copy: None,
                last_shared: None,
            })),
            external_paths,
            changes: Arc::new(changes),
            change_seq: Arc::new(AtomicU64::new(0)),
            events,
            window: Arc::new(window::WindowState::default()),
            live: Arc::new(live::LiveState::default()),
            lan: external_paths,
        }
    }

    /// Everything pushed to the window: live session changes, presence, chat
    /// and window requests. CONTRACT: the desktop shell forwards each one as
    /// the Tauri event `app_event`, the dev bridge on `GET /events`. A slow
    /// receiver skips what it missed (presence is sent again soon anyway).
    pub fn events(&self) -> broadcast::Receiver<AppEvent> {
        self.events.subscribe()
    }

    /// Push one event to every window. Never fails: with no window listening
    /// the event is dropped.
    pub fn emit(&self, event: AppEvent) {
        let _ = self.events.send(event);
    }

    /// How many transports listen for app events right now.
    pub fn event_listeners(&self) -> usize {
        self.events.receiver_count()
    }

    /// Watch the open document. The receiver fires after every commit, undo,
    /// redo, open, close and restore, including changes made by another
    /// client such as the MCP server. CONTRACT: the desktop shell turns each
    /// value into the Tauri event `doc_changed`.
    pub fn watch_changes(&self) -> watch::Receiver<DocChange> {
        self.changes.subscribe()
    }

    /// Tell watchers the document changed. Never fails and never blocks: a
    /// `watch` sender keeps the last value even with no receivers. Closing or
    /// switching the project a live session shares ends that session.
    pub(crate) fn notify(&self, s: &Session) {
        let (revision, project_id) = s.revision();
        let seq = self.change_seq.fetch_add(1, Ordering::Relaxed) + 1;
        self.changes.send_replace(DocChange { revision, project_id: project_id.clone(), seq });
        live::after_doc_change(self, project_id.as_deref());
    }

    /// The `seq` of the last change notification. Read under the session
    /// lock it belongs to the document as it is.
    pub(crate) fn change_seq(&self) -> u64 {
        self.changes.borrow().seq
    }

    /// Apply a command to the open document and persist it. CONTRACT: the AI
    /// module commits accepted proposals through this, so user and AI edits
    /// share one path (autosave, snapshots, logging). On a live session guest
    /// the command is committed on the host (docs/CONTRACT.md, "Live
    /// sessions").
    pub async fn commit(&self, command: Command, origin: Origin) -> Result<ApplyResult, IpcError> {
        self.commit_checked(command, origin, None).await
    }

    /// `commit`, but only while the document is still at `expected_revision`.
    /// The check and the apply happen under one lock, so nothing can slip in
    /// between them. CONTRACT: `ai_resolve` uses this for its stale check.
    pub async fn commit_if_revision(
        &self,
        command: Command,
        origin: Origin,
        expected_revision: u32,
    ) -> Result<ApplyResult, IpcError> {
        self.commit_checked(command, origin, Some(expected_revision)).await
    }

    async fn commit_checked(
        &self,
        command: Command,
        origin: Origin,
        expected_revision: Option<u32>,
    ) -> Result<ApplyResult, IpcError> {
        let mut s = self.session.lock().await;
        if s.live_copy.is_some() {
            drop(s);
            return live::guest_apply(self, command, origin, expected_revision).await;
        }
        // While hosting, the step records the host participant as its author.
        let author = self.live.local_author();
        self.commit_locked(&mut s, command, origin, expected_revision, author, None)
    }

    /// The one commit path, under the session lock: the revision check, the
    /// apply with its author, autosave, snapshots and watchers. A live
    /// session host commits a guest's edit through it with the guest as
    /// `author` and the shared project as `project`, which the open document
    /// must still be.
    pub(crate) fn commit_locked(
        &self,
        s: &mut Session,
        command: Command,
        origin: Origin,
        expected_revision: Option<u32>,
        author: Option<Id>,
        project: Option<&str>,
    ) -> Result<ApplyResult, IpcError> {
        let doc = s.doc.as_mut().ok_or_else(no_document)?;
        if project.is_some_and(|p| doc.project().id != p) {
            return Err(live::not_live("The host closed the project."));
        }
        let revision = doc.revision();
        if let Some(expected) = expected_revision.filter(|e| *e != revision) {
            return Err(IpcError::new(
                "stale",
                format!("the plan moved on from revision {expected} to {revision}, so nothing was applied"),
            ));
        }
        let result = doc.apply_as(command, origin, author.clone())?;
        self.live.note_change(result.state.revision, author.as_ref());
        // Watchers hear of the change even when it could not be saved: the
        // plan did change.
        let saved = s.after_change();
        self.notify(s);
        saved?;
        Ok(result)
    }

    /// Undo or redo under the session lock. In a live session the host
    /// takes back or brings back someone else's step only with `force`;
    /// `requester` is the guest asking, None this computer.
    pub(crate) fn step_locked(
        &self,
        s: &mut Session,
        redo: bool,
        force: bool,
        requester: Option<&Id>,
        project: Option<&str>,
    ) -> Result<DocState, IpcError> {
        let doc = s.doc.as_mut().ok_or_else(no_document)?;
        if project.is_some_and(|p| doc.project().id != p) {
            return Err(live::not_live("The host closed the project."));
        }
        if !force {
            self.live.check_step(doc, redo, requester)?;
        }
        let state = if redo { doc.redo()? } else { doc.undo()? };
        self.live.note_change(state.revision, requester);
        let saved = s.after_change();
        self.notify(s);
        saved?;
        Ok(state)
    }

    /// Folder of the open project, if any: on a live session guest, its
    /// folder for the shared project. CONTRACT: used by the AI module for
    /// `ai-log.jsonl`.
    pub async fn project_dir(&self) -> Option<PathBuf> {
        let s = self.session.lock().await;
        s.open_project_dir().ok()
    }

    /// Single entry point for every IPC call.
    pub async fn handle(&self, cmd: &str, args: Value) -> IpcResult {
        // A live session guest's copy belongs to the host: some calls are
        // refused or answered differently (docs/CONTRACT.md, "Live sessions").
        if let Some(result) = live::guest_route(self, cmd, &args).await {
            return result;
        }
        // Checked before the `ai_` prefix: `render_ai_*` is the image
        // provider, not the copilot.
        if render_ai::OWNS.contains(&cmd) {
            return render_ai::handle(self, cmd, args).await;
        }
        if cmd.starts_with("ai_") {
            return ai::handle(self, cmd, args).await;
        }
        if interop::OWNS.contains(&cmd) {
            return interop::handle(self, cmd, args).await;
        }
        if live::OWNS.contains(&cmd) {
            return live::handle(self, cmd, args).await;
        }
        if window::OWNS.contains(&cmd) {
            return window::handle(self, cmd, args).await;
        }
        match cmd {
            // ------------------------------------------------------ library
            "catalog_assets" => to_value(&defaults::asset_catalog()),
            "render_styles" => to_value(&defaults::render_styles()),

            // -------------------------------------------------- project hub
            "hub_list" => {
                let mut s = self.session.lock().await;
                // Best effort: the list should show the open project as saved.
                let _ = s.flush();
                to_value(&store::list_projects(&s.data_dir)?)
            }
            "hub_create" => {
                let name: String = arg(&args, "name")?;
                let settings: Option<ProjectSettings> = arg(&args, "settings")?;
                let template: Option<String> = arg(&args, "template")?;
                let mut project = match template.as_deref().unwrap_or("blank") {
                    "blank" => defaults::new_project(""),
                    "sample-bungalow" => guhit_core::templates::sample_bungalow(),
                    // "Bungalow with services": plumbing, storm, lighting,
                    // power and a split aircon. The id stays for old callers.
                    "plumbing-demo" => guhit_core::templates::plumbing_demo(),
                    other => {
                        return Err(IpcError::new(
                            "invalid",
                            format!(
                                "unknown template `{other}`. Use `blank`, `sample-bungalow` or `plumbing-demo` (the bungalow with services)"
                            ),
                        ))
                    }
                };
                // A template is content only. Identity and dates are always new.
                project.schema_version = SCHEMA_VERSION;
                project.id = defaults::new_id();
                let name = clean_project_name(&name);
                project.name = if name.is_empty() { "Untitled project".to_string() } else { name };
                let now = defaults::now_rfc3339();
                project.created_at = now.clone();
                project.updated_at = now;
                if let Some(settings) = settings {
                    project.settings = settings;
                }

                let mut s = self.session.lock().await;
                s.flush()?;
                store::save_project(&s.data_dir, &project)?;
                s.set_doc(project)?;
                self.notify(&s);
                to_value(&s.state()?)
            }
            "hub_open" => {
                let id: String = arg(&args, "id")?;
                let mut s = self.session.lock().await;
                if s.is_open(&id) {
                    // Already open: keep the undo history instead of reloading.
                    return to_value(&s.state()?);
                }
                let project = store::load_project(&s.data_dir, &id)?;
                s.set_doc(project)?;
                let dir = s.open_project_dir()?;
                if snapshots::auto_is_due(&dir) {
                    s.auto_snapshot("Auto: on open");
                }
                self.notify(&s);
                to_value(&s.state()?)
            }
            "hub_rename" => {
                let id: String = arg(&args, "id")?;
                let name: String = arg(&args, "name")?;
                let name = clean_project_name(&name);
                if name.is_empty() {
                    return Err(IpcError::new("invalid", "a project name cannot be empty"));
                }
                let mut s = self.session.lock().await;
                if s.is_open(&id) {
                    // Rename in the engine, so `doc.project().name` is right
                    // everywhere and an undo does not bring the old name back.
                    s.doc.as_mut().ok_or_else(no_document)?.rename(&name)?;
                    s.save()?;
                    // The revision stays, but watchers (and live session
                    // guests) get the new name.
                    self.notify(&s);
                } else {
                    let mut project = store::load_project(&s.data_dir, &id)?;
                    project.name = name;
                    project.updated_at = defaults::now_rfc3339();
                    store::save_project(&s.data_dir, &project)?;
                }
                to_value(&store::load_meta(&s.data_dir, &id)?)
            }
            "hub_duplicate" => {
                let id: String = arg(&args, "id")?;
                let mut s = self.session.lock().await;
                if s.is_open(&id) {
                    s.flush()?;
                }
                let source = store::load_project(&s.data_dir, &id)?;
                let copy = store::duplicate_project(&s.data_dir, &source)?;
                to_value(&store::load_meta(&s.data_dir, &copy.id)?)
            }
            "hub_delete" => {
                let id: String = arg(&args, "id")?;
                let mut s = self.session.lock().await;
                // Check first, so a bad id does not close the open project.
                let dir = store::project_dir(&s.data_dir, &id)?;
                if !dir.is_dir() {
                    return Err(IpcError::new("not_found", format!("project not found: {id}")));
                }
                if s.is_open(&id) {
                    s.close_doc()?;
                    self.notify(&s);
                }
                store::trash_project(&s.data_dir, &id)?;
                Ok(Value::Null)
            }
            "hub_set_thumbnail" => {
                let id: String = arg(&args, "id")?;
                let png: String = arg(&args, "png")?;
                let s = self.session.lock().await;
                let dir = store::existing_project_dir(&s.data_dir, &id)?;
                let bytes = files::decode_png_data_url(&png)?;
                files::write_atomic(&dir.join(store::THUMBNAIL_FILE), &bytes)?;
                Ok(Value::Null)
            }
            "hub_close" => {
                let mut s = self.session.lock().await;
                s.close_doc()?;
                self.notify(&s);
                Ok(Value::Null)
            }

            // ----------------------------------------------------- document
            "doc_state" => {
                let s = self.session.lock().await;
                to_value(&s.state().ok())
            }
            // Cheap poll for external changes: no project data crosses IPC.
            "doc_revision" => {
                let s = self.session.lock().await;
                let (revision, project_id) = s.revision();
                Ok(serde_json::json!({ "revision": revision, "project_id": project_id }))
            }
            "doc_apply" => {
                let command: Command = arg(&args, "command")?;
                to_value(&self.commit(command, Origin::User).await?)
            }
            "doc_preview" => {
                let command: Command = arg(&args, "command")?;
                let s = self.session.lock().await;
                let result = s.open_doc()?.preview(&command)?;
                to_value(&result)
            }
            "doc_undo" | "doc_redo" => {
                // In a live session someone else's step needs `force`
                // (docs/CONTRACT.md, "Live sessions"). Alone, every step is yours.
                let force: Option<bool> = arg(&args, "force")?;
                let (redo, force) = (cmd == "doc_redo", force.unwrap_or(false));
                let mut s = self.session.lock().await;
                if s.live_copy.is_some() {
                    drop(s);
                    return to_value(&live::guest_step(self, redo, force).await?);
                }
                to_value(&self.step_locked(&mut s, redo, force, None, None)?)
            }
            "doc_query" => {
                let query: Query = arg(&args, "query")?;
                let s = self.session.lock().await;
                Ok(s.open_doc()?.query(&query)?)
            }

            // ----------------------------------------------------- versions
            "snapshot_create" => {
                let label: String = arg(&args, "label")?;
                let s = self.session.lock().await;
                let doc = s.open_doc()?;
                let dir = s.open_project_dir()?;
                let project = doc.project().clone();
                to_value(&snapshots::create(&dir, &project, doc.revision(), &label, false)?)
            }
            "snapshot_list" => {
                let s = self.session.lock().await;
                to_value(&snapshots::list(&s.open_project_dir()?))
            }
            "snapshot_restore" => {
                let id: String = arg(&args, "id")?;
                let mut s = self.session.lock().await;
                let dir = s.open_project_dir()?;
                let snapshot = snapshots::load(&dir, &id)?;

                // Safety net first. If it cannot be written, do not restore.
                let doc = s.open_doc()?;
                let current = doc.project().clone();
                let revision = doc.revision();
                let label = format!("Auto: before restoring \"{}\"", snapshot.meta.label);
                snapshots::create(&dir, &current, revision, &label, true)?;

                // Identity stays with the open project. Content comes from the snapshot.
                let mut project = snapshot.project;
                project.schema_version = SCHEMA_VERSION;
                project.id = current.id.clone();
                project.name = current.name.clone();
                project.created_at = current.created_at.clone();
                // A restore is a change like any other: the revision keeps
                // going up, so it never repeats a number an AI proposal or a
                // render record was pinned to.
                let revision = revision + 1;
                s.doc = Some(Document::with_revision(project, revision));
                s.last_auto_snapshot_revision = revision;
                s.dirty = true;
                s.save()?;
                self.notify(&s);
                to_value(&s.state()?)
            }

            // ------------------------------------------------------- export
            "export_plan" => {
                let format: PlanFormat = arg(&args, "format")?;
                let options: PlanExportOptions = arg(&args, "options")?;
                let path: Option<String> = arg(&args, "path")?;
                let s = self.session.lock().await;
                let doc = s.open_doc()?;
                let project = doc.project().clone();
                let derived = doc.derived();
                let (bytes, scale, ext) = match format {
                    PlanFormat::Svg => {
                        let out = guhit_export::plan_svg(&project, derived, &options).map_err(export_err)?;
                        (out.data.into_bytes(), out.scale_denominator, "svg")
                    }
                    PlanFormat::Pdf => {
                        let out = guhit_export::plan_pdf(&project, derived, &options).map_err(export_err)?;
                        (out.data, out.scale_denominator, "pdf")
                    }
                    PlanFormat::Dxf => {
                        let out = guhit_export::plan_dxf(&project, derived, &options).map_err(export_err)?;
                        (out.data.into_bytes(), out.scale_denominator, "dxf")
                    }
                };
                let target = export_target(
                    &s.data_dir,
                    &s.exports_dir(),
                    path.as_deref(),
                    &files::slug(&project.name, "plan"),
                    ext,
                    self.external_paths,
                )?;
                files::write_atomic(&target, &bytes)?;
                to_value(&ExportResult {
                    path: target.to_string_lossy().into_owned(),
                    scale_denominator: Some(scale),
                })
            }
            "export_image" => {
                let png: String = arg(&args, "png")?;
                let name: String = arg(&args, "name")?;
                let path: Option<String> = arg(&args, "path")?;
                let bytes = files::decode_png_data_url(&png)?;
                let safe = files::safe_file_name(&name)?;
                let stem = match files::split_ext(&safe) {
                    (stem, ext) if ImageKind::from_ext(ext).is_some() => stem.to_string(),
                    _ => safe.clone(),
                };
                let s = self.session.lock().await;
                let exports = s.exports_dir();
                let target = export_target(&s.data_dir, &exports, path.as_deref(), &stem, "png", self.external_paths)?;
                files::write_atomic(&target, &bytes)?;
                to_value(&ExportResult {
                    path: target.to_string_lossy().into_owned(),
                    scale_denominator: None,
                })
            }

            // Bytes the frontend produced (GLB, OBJ, DAE). The extension comes
            // from the name and must be one of the model formats.
            "export_bytes" => {
                let data: String = arg(&args, "data")?;
                let name: String = arg(&args, "name")?;
                let path: Option<String> = arg(&args, "path")?;
                let safe = files::safe_file_name(&name)?;
                let (stem, ext) = files::split_ext(&safe);
                let ext = ext.to_ascii_lowercase();
                if !matches!(ext.as_str(), "glb" | "gltf" | "obj" | "mtl" | "dae") {
                    return Err(IpcError::new("invalid", "export_bytes writes glb, gltf, obj, mtl or dae files"));
                }
                let bytes = files::decode_any_data_url(&data, 64 * 1024 * 1024)?;
                let s = self.session.lock().await;
                let exports = s.exports_dir();
                let target = export_target(&s.data_dir, &exports, path.as_deref(), stem, &ext, self.external_paths)?;
                files::write_atomic(&target, &bytes)?;
                to_value(&ExportResult {
                    path: target.to_string_lossy().into_owned(),
                    scale_denominator: None,
                })
            }

            // ----------------------------------------------------- underlay
            // On a live session guest, underlays are stored on and read from
            // the host.
            "underlay_store" => {
                let file_name: String = arg(&args, "file_name")?;
                let data: String = arg(&args, "data")?;
                let (kind, bytes) = files::decode_image_data_url(&data)?;
                files::safe_file_name(&file_name)?;
                let underlay = live::wire::FileKind::Underlay;
                if let Some(stored) = live::guest_put_file(self, underlay, &file_name, &bytes).await {
                    return stored;
                }
                let s = self.session.lock().await;
                let stored = store_underlay(&s.open_project_dir()?, &file_name, kind, &bytes)?;
                Ok(serde_json::json!({ "file_name": stored }))
            }
            "underlay_data" => {
                let file_name: String = arg(&args, "file_name")?;
                let kind = underlay_kind(&file_name)?;
                let underlay = live::wire::FileKind::Underlay;
                if let Some(bytes) = live::guest_get_file(self, underlay, &file_name).await {
                    return to_value(&files::encode_data_url(kind, &bytes?));
                }
                let s = self.session.lock().await;
                let (path, kind) = underlay_file(&s.open_project_dir()?, &file_name)?;
                if !path.is_file() {
                    return Err(IpcError::new("not_found", format!("underlay not found: {file_name}")));
                }
                to_value(&files::read_data_url(&path, kind)?)
            }

            // ------------------------------------------------------ renders
            "render_list" => {
                let s = self.session.lock().await;
                to_value(&renders::list(&s.open_project_dir()?)?)
            }
            "render_capture" => {
                let camera: Camera = arg(&args, "camera")?;
                let png: String = arg(&args, "png")?;
                // A long render sends the revision it started from: the image
                // shows that model, not one edited while it ran.
                let started: Option<u32> = arg(&args, "revision")?;
                let info: Option<RenderInfo> = arg(&args, "info")?;
                let s = self.session.lock().await;
                let current = s.open_doc()?.revision();
                let revision = match started {
                    Some(r) if r > current => {
                        return Err(IpcError::new(
                            "bad_args",
                            format!("argument `revision`: {r} is newer than the document, which is at {current}"),
                        ))
                    }
                    Some(r) => r,
                    None => current,
                };
                to_value(&renders::capture(&s.open_project_dir()?, revision, camera, &png, info)?)
            }
            "render_data" => {
                let id: String = arg(&args, "id")?;
                let s = self.session.lock().await;
                to_value(&renders::data(&s.open_project_dir()?, &id)?)
            }
            "render_delete" => {
                let id: String = arg(&args, "id")?;
                let s = self.session.lock().await;
                let project_id = s.open_doc()?.project().id.clone();
                renders::delete(&s.data_dir, &s.open_project_dir()?, &project_id, &id)?;
                Ok(Value::Null)
            }

            _ => Err(IpcError::new("unknown_command", format!("unknown command `{cmd}`"))),
        }
    }
}

fn export_err(e: guhit_export::ExportError) -> IpcError {
    match e {
        guhit_export::ExportError::Empty(_) => IpcError::new("invalid", e.to_string()),
        guhit_export::ExportError::Failed(_) => IpcError::new("io", e.to_string()),
    }
}

/// The image type of a stored underlay, from its name, which must already be
/// in safe form.
pub(crate) fn underlay_kind(file_name: &str) -> Result<ImageKind, IpcError> {
    files::check_file_name(file_name)?;
    ImageKind::from_ext(files::split_ext(file_name).1)
        .ok_or_else(|| IpcError::new("invalid", "underlays are PNG or JPEG files"))
}

/// Where a stored underlay of a project folder is, and its image type.
pub(crate) fn underlay_file(project_dir: &Path, file_name: &str) -> Result<(PathBuf, ImageKind), IpcError> {
    let kind = underlay_kind(file_name)?;
    Ok((project_dir.join("underlays").join(file_name), kind))
}

/// Store an underlay image in a project folder and return the name it got.
/// The window's own uploads and a live session guest's go through here.
pub(crate) fn store_underlay(
    project_dir: &Path,
    file_name: &str,
    kind: ImageKind,
    bytes: &[u8],
) -> Result<String, IpcError> {
    let safe = files::safe_file_name(file_name)?;
    // The stored extension always matches the real image type.
    let stem = match files::split_ext(&safe) {
        (stem, ext) if ImageKind::from_ext(ext).is_some() => stem.to_string(),
        _ => safe.replace('.', "_"),
    };
    let dir = project_dir.join("underlays");
    files::create_dir(&dir)?;
    // Never overwrite: another underlay element may use that file.
    let target = files::unique_path(&dir, &stem, kind.ext());
    files::write_atomic(&target, bytes)?;
    Ok(target.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string())
}

/// Where an export goes. A path from the UI comes from the native save
/// dialog, so it is used as given but must be absolute, free of `..` and in
/// an existing folder. Without one: `<exports>/<stem>-<timestamp>.<ext>`,
/// where `exports` is `Session::exports_dir`.
///
/// With `external_paths` false the caller is not trusted with a location:
/// only a path inside `data_dir` is accepted. The dev bridge runs this way,
/// because any program on the machine can post to it.
pub(crate) fn export_target(
    data_dir: &Path,
    exports: &Path,
    path: Option<&str>,
    stem: &str,
    ext: &str,
    external_paths: bool,
) -> Result<PathBuf, IpcError> {
    match path.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => {
            let target = PathBuf::from(p);
            let traversal = target.components().any(|c| matches!(c, std::path::Component::ParentDir));
            if !target.is_absolute() || traversal || target.file_name().is_none() {
                return Err(IpcError::new("invalid", "the export path must be an absolute file path"));
            }
            if !external_paths && !target.starts_with(data_dir) {
                return Err(IpcError::new(
                    "forbidden",
                    format!(
                        "this service only writes inside {}. Leave `path` empty to export to the exports folder",
                        data_dir.display()
                    ),
                ));
            }
            match target.parent() {
                Some(parent) if parent.is_dir() => Ok(target),
                _ => Err(IpcError::new("invalid", "the export folder does not exist")),
            }
        }
        None => {
            files::create_dir(exports)?;
            Ok(files::unique_path(exports, &format!("{stem}-{}", files::file_timestamp()), ext))
        }
    }
}
