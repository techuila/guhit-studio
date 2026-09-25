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
pub mod render_ai;
pub mod renders;
pub mod snapshots;
pub mod store;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use guhit_core::Document;
use guhit_model::*;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::{watch, Mutex};

use files::ImageKind;

pub type IpcResult = Result<Value, IpcError>;

const MAX_PROJECT_NAME_CHARS: usize = 120;

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

    pub(crate) fn open_project_dir(&self) -> Result<PathBuf, IpcError> {
        let doc = self.open_doc()?;
        store::project_dir(&self.data_dir, &doc.project().id)
    }

    /// Autosave: write the open project to `project.json` and stamp `updated_at`.
    fn save(&mut self) -> Result<(), IpcError> {
        let Some(doc) = self.doc.as_ref() else {
            return Ok(());
        };
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
        Ok(())
    }

    fn close_doc(&mut self) -> Result<(), IpcError> {
        self.flush()?;
        self.doc = None;
        self.dirty = false;
        self.last_auto_snapshot_revision = 0;
        Ok(())
    }

    fn is_open(&self, id: &str) -> bool {
        self.doc.as_ref().is_some_and(|d| d.project().id == id)
    }

    pub(crate) fn state(&self) -> Result<DocState, IpcError> {
        Ok(self.open_doc()?.state())
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
        Self {
            ai: Arc::new(ai::AiState::for_data_dir(&data_dir)),
            render_ai: Arc::new(render_ai::RenderAiState::for_data_dir(&data_dir)),
            session: Arc::new(Mutex::new(Session {
                data_dir,
                doc: None,
                dirty: false,
                last_auto_snapshot_revision: 0,
            })),
            external_paths,
            changes: Arc::new(changes),
            change_seq: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Watch the open document. The receiver fires after every commit, undo,
    /// redo, open, close and restore, including changes made by another
    /// client such as the MCP server. CONTRACT: the desktop shell turns each
    /// value into the Tauri event `doc_changed`.
    pub fn watch_changes(&self) -> watch::Receiver<DocChange> {
        self.changes.subscribe()
    }

    /// Tell watchers the document changed. Never fails and never blocks: a
    /// `watch` sender keeps the last value even with no receivers.
    pub(crate) fn notify(&self, s: &Session) {
        let (revision, project_id) = s.revision();
        let seq = self.change_seq.fetch_add(1, Ordering::Relaxed) + 1;
        self.changes.send_replace(DocChange { revision, project_id, seq });
    }

    /// Apply a command to the open document and persist it. CONTRACT: the AI
    /// module commits accepted proposals through this, so user and AI edits
    /// share one path (autosave, snapshots, logging).
    pub async fn commit(&self, command: Command, origin: Origin) -> Result<ApplyResult, IpcError> {
        let mut s = self.session.lock().await;
        let doc = s.doc.as_mut().ok_or_else(no_document)?;
        let result = doc.apply(command, origin)?;
        s.after_change()?;
        self.notify(&s);
        Ok(result)
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
        let mut s = self.session.lock().await;
        let doc = s.doc.as_mut().ok_or_else(no_document)?;
        let revision = doc.revision();
        if revision != expected_revision {
            return Err(IpcError::new(
                "stale",
                format!("the plan moved on from revision {expected_revision} to {revision}, so nothing was applied"),
            ));
        }
        let result = doc.apply(command, origin)?;
        s.after_change()?;
        self.notify(&s);
        Ok(result)
    }

    /// Folder of the open project, if any. CONTRACT: used by the AI module
    /// for `ai-log.jsonl`.
    pub async fn project_dir(&self) -> Option<PathBuf> {
        let s = self.session.lock().await;
        s.doc.as_ref().map(|d| s.data_dir.join("projects").join(&d.project().id))
    }

    /// Single entry point for every IPC call.
    pub async fn handle(&self, cmd: &str, args: Value) -> IpcResult {
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
                to_value(&s.doc.as_ref().map(|d| d.state()))
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
                let mut s = self.session.lock().await;
                let doc = s.doc.as_mut().ok_or_else(no_document)?;
                let state = if cmd == "doc_undo" { doc.undo()? } else { doc.redo()? };
                s.after_change()?;
                self.notify(&s);
                to_value(&state)
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
                let target = export_target(&s.data_dir, path.as_deref(), &stem, "png", self.external_paths)?;
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
                let target = export_target(&s.data_dir, path.as_deref(), stem, &ext, self.external_paths)?;
                files::write_atomic(&target, &bytes)?;
                to_value(&ExportResult {
                    path: target.to_string_lossy().into_owned(),
                    scale_denominator: None,
                })
            }

            // ----------------------------------------------------- underlay
            "underlay_store" => {
                let file_name: String = arg(&args, "file_name")?;
                let data: String = arg(&args, "data")?;
                let (kind, bytes) = files::decode_image_data_url(&data)?;
                let safe = files::safe_file_name(&file_name)?;
                // The stored extension always matches the real image type.
                let stem = match files::split_ext(&safe) {
                    (stem, ext) if ImageKind::from_ext(ext).is_some() => stem.to_string(),
                    _ => safe.replace('.', "_"),
                };
                let s = self.session.lock().await;
                let dir = s.open_project_dir()?.join("underlays");
                files::create_dir(&dir)?;
                // Never overwrite: another underlay element may use that file.
                let target = files::unique_path(&dir, &stem, kind.ext());
                files::write_atomic(&target, &bytes)?;
                let stored = target.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
                Ok(serde_json::json!({ "file_name": stored }))
            }
            "underlay_data" => {
                let file_name: String = arg(&args, "file_name")?;
                files::check_file_name(&file_name)?;
                let kind = ImageKind::from_ext(files::split_ext(&file_name).1)
                    .ok_or_else(|| IpcError::new("invalid", "underlays are PNG or JPEG files"))?;
                let s = self.session.lock().await;
                let path = s.open_project_dir()?.join("underlays").join(&file_name);
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

/// Where an export goes. A path from the UI comes from the native save
/// dialog, so it is used as given but must be absolute, free of `..` and in
/// an existing folder. Without one: `<data>/exports/<stem>-<timestamp>.<ext>`.
///
/// With `external_paths` false the caller is not trusted with a location:
/// only a path inside `data_dir` is accepted. The dev bridge runs this way,
/// because any program on the machine can post to it.
pub(crate) fn export_target(
    data_dir: &Path,
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
            let dir = store::exports_dir(data_dir);
            files::create_dir(&dir)?;
            Ok(files::unique_path(&dir, &format!("{stem}-{}", files::file_timestamp()), ext))
        }
    }
}
