//! Interchange: DXF/DWG import, reference models, whole-model export and the
//! `.guhit` project bundle. See DECISIONS D16 and docs/INTEROP.md.
//!
//! DWG never touches this process. It goes through the free ODA File
//! Converter that the user installs and points the app at; the path lives in
//! `settings.json` under `dwg_converter_path` and the converter is started
//! with `std::process::Command` and an absolute path, never through a shell.
//!
//! Every path that arrives over IPC is checked before it is used: a caller
//! path must be absolute and free of `..`, and the dev bridge only accepts
//! paths inside its own data dir. Zip entries are checked the same way, so a
//! bundle cannot write outside the project folder it creates.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use guhit_model::*;
use serde_json::{json, Value};

use crate::files::{self};
use crate::{arg, no_document, store, to_value, AppService, IpcResult};

/// Commands this module answers. `AppService::handle` routes on this list.
pub const OWNS: [&str; 9] = [
    "import_inspect",
    "import_commit",
    "model_store",
    "model_data",
    "export_model",
    "bundle_save",
    "bundle_open",
    "dwg_status",
    "dwg_set_path",
];

const SETTINGS_FILE: &str = "settings.json";
const CONVERTER_KEY: &str = "dwg_converter_path";
/// Output version handed to the converter. 2018 is what current AutoCAD,
/// BricsCAD and SketchUp all read.
const DWG_VERSION: &str = "ACAD2018";
/// Largest reference model accepted.
pub const MAX_MODEL_BYTES: usize = 50 * 1024 * 1024;
/// Largest bundle accepted, and the cap on what one may expand to.
pub const MAX_BUNDLE_BYTES: usize = 200 * 1024 * 1024;
const MAX_BUNDLE_ENTRIES: usize = 5000;
/// How long to wait for the converter before giving up on one file.
const CONVERT_TIMEOUT: Duration = Duration::from_secs(120);

const MODEL_EXTS: [(&str, &str); 3] = [
    ("glb", "model/gltf-binary"),
    ("gltf", "model/gltf+json"),
    ("obj", "model/obj"),
];

pub async fn handle(app: &AppService, cmd: &str, args: Value) -> IpcResult {
    match cmd {
        "dwg_status" => {
            let dir = data_dir(app).await;
            to_value(&tokio::task::spawn_blocking(move || converter_status(&dir))
                .await
                .map_err(join_err)?)
        }
        "dwg_set_path" => {
            let path: String = arg(&args, "path")?;
            let dir = data_dir(app).await;
            to_value(&tokio::task::spawn_blocking(move || set_converter(&dir, &path))
                .await
                .map_err(join_err)??)
        }
        "import_inspect" => {
            let (file_name, bytes) = drawing_input(app, &args).await?;
            let inspection = guhit_import::inspect(&file_name, &bytes).map_err(import_err)?;
            to_value(&inspection)
        }
        "import_commit" => import_commit(app, args).await,
        "model_store" => model_store(app, args).await,
        "model_data" => model_data(app, args).await,
        "export_model" => export_model(app, args).await,
        "bundle_save" => bundle_save(app, args).await,
        "bundle_open" => bundle_open(app, args).await,
        _ => Err(IpcError::new("unknown_command", format!("unknown command `{cmd}`"))),
    }
}

async fn data_dir(app: &AppService) -> PathBuf {
    app.session.lock().await.data_dir.clone()
}

fn join_err(e: tokio::task::JoinError) -> IpcError {
    IpcError::new("io", format!("the work could not be finished: {e}"))
}

fn import_err(e: guhit_import::ImportError) -> IpcError {
    IpcError::new(e.code(), e.to_string())
}

fn export_err(e: guhit_export::ExportError) -> IpcError {
    match e {
        guhit_export::ExportError::Empty(_) => IpcError::new("invalid", e.to_string()),
        guhit_export::ExportError::Failed(_) => IpcError::new("io", e.to_string()),
    }
}

// ------------------------------------------------------------------ settings

fn settings_of(dir: &Path) -> serde_json::Map<String, Value> {
    std::fs::read_to_string(dir.join(SETTINGS_FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// Merge one key into `settings.json`. Other parts of the app keep their own
/// keys in the same file, so the file is read, changed and written back.
fn put_setting(dir: &Path, key: &str, value: Option<&str>) -> Result<(), IpcError> {
    let mut settings = settings_of(dir);
    match value {
        Some(v) => {
            settings.insert(key.to_string(), json!(v));
        }
        None => {
            settings.remove(key);
        }
    }
    files::create_dir(dir)?;
    files::write_json_atomic(&dir.join(SETTINGS_FILE), &Value::Object(settings))
}

// ------------------------------------------------------- the ODA converter

/// On macOS the download is an app bundle. Accept it and use the binary
/// inside, so the user can pick what Finder shows them.
fn resolve_executable(path: &Path) -> PathBuf {
    if path.extension().and_then(|e| e.to_str()) == Some("app") {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("ODAFileConverter");
        let inner = path.join("Contents").join("MacOS").join(stem);
        if inner.is_file() {
            return inner;
        }
    }
    path.to_path_buf()
}

fn converter_path(dir: &Path) -> Option<PathBuf> {
    settings_of(dir)
        .get(CONVERTER_KEY)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
}

/// Start the converter with no arguments to see that it runs at all. It is
/// killed after two seconds: with no arguments the converter opens its own
/// window on some platforms and prints usage on others, and either answer
/// means the file is the real thing.
fn probe_converter(exe: &Path) -> Result<String, String> {
    if !exe.is_file() {
        return Err(format!("there is no file at {}", exe.display()));
    }
    let mut child = std::process::Command::new(exe)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("it could not be started: {e}"))?;
    for _ in 0..20 {
        match child.try_wait() {
            Ok(Some(_)) => return Ok("the converter answered".to_string()),
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(format!("it could not be started: {e}")),
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    Ok("the converter started".to_string())
}

const HOW_TO_INSTALL: &str = "Download the free ODA File Converter from \
https://www.opendesign.com/guestfiles/oda_file_converter, install it, then point Guhit Studio at it. \
DXF works without it.";

fn converter_status(dir: &Path) -> DwgConverterStatus {
    match converter_path(dir) {
        None => DwgConverterStatus {
            configured: false,
            path: None,
            works: false,
            message: format!("No DWG converter is set up. {HOW_TO_INSTALL}"),
        },
        Some(path) => {
            let exe = resolve_executable(&path);
            let shown = Some(path.to_string_lossy().into_owned());
            match probe_converter(&exe) {
                Ok(message) => DwgConverterStatus {
                    configured: true,
                    path: shown,
                    works: true,
                    message,
                },
                Err(why) => DwgConverterStatus {
                    configured: true,
                    path: shown,
                    works: false,
                    message: format!("The converter is set up but did not run: {why}"),
                },
            }
        }
    }
}

fn set_converter(dir: &Path, raw: &str) -> Result<DwgConverterStatus, IpcError> {
    let raw = raw.trim();
    if raw.is_empty() {
        put_setting(dir, CONVERTER_KEY, None)?;
        return Ok(converter_status(dir));
    }
    let path = PathBuf::from(raw);
    if !path.is_absolute() || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(IpcError::new(
            "invalid",
            "the converter path must be an absolute path to the ODAFileConverter program",
        ));
    }
    let exe = resolve_executable(&path);
    if !exe.is_file() {
        return Err(IpcError::new(
            "not_found",
            format!("there is no program at {}. {HOW_TO_INSTALL}", path.display()),
        ));
    }
    probe_converter(&exe).map_err(|why| {
        IpcError::new("invalid", format!("that file is not the ODA File Converter: {why}"))
    })?;
    put_setting(dir, CONVERTER_KEY, Some(raw))?;
    Ok(converter_status(dir))
}

fn not_configured() -> IpcError {
    IpcError::new(
        "invalid",
        format!("DWG needs the ODA File Converter, which is not set up yet. {HOW_TO_INSTALL}"),
    )
}

/// The configured converter binary, or a message that says what to do.
/// Checked before any work starts, so a DWG job fails on the missing
/// converter rather than halfway through.
fn converter_exe(dir: &Path) -> Result<PathBuf, IpcError> {
    let path = converter_path(dir).ok_or_else(not_configured)?;
    let exe = resolve_executable(&path);
    if !exe.is_file() {
        return Err(IpcError::new(
            "not_found",
            format!("the DWG converter is set to {}, which is not there any more", path.display()),
        ));
    }
    Ok(exe)
}

/// Run the converter over one file. The converter works on folders, so each
/// call gets its own pair of folders inside the app's data dir and cleans up
/// after itself.
fn convert(dir: &Path, stem: &str, from_ext: &str, to_ext: &str, bytes: &[u8]) -> Result<Vec<u8>, IpcError> {
    let exe = converter_exe(dir)?;
    let work = dir.join("tmp").join(defaults::new_id());
    let input = work.join("in");
    let output = work.join("out");
    files::create_dir(&input)?;
    files::create_dir(&output)?;
    let result = (|| -> Result<Vec<u8>, IpcError> {
        files::write_atomic(&input.join(format!("{stem}.{from_ext}")), bytes)?;
        // ODAFileConverter <in> <out> <version> <type> <recurse> <audit> <filter>
        let mut child = std::process::Command::new(&exe)
            .arg(&input)
            .arg(&output)
            .arg(DWG_VERSION)
            .arg(to_ext.to_ascii_uppercase())
            .arg("0")
            .arg("1")
            .arg(format!("*.{from_ext}"))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| IpcError::new("io", format!("the DWG converter could not be started: {e}")))?;
        let deadline = std::time::Instant::now() + CONVERT_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(100))
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(IpcError::new("io", "the DWG converter did not finish in time"));
                }
                Err(e) => return Err(IpcError::new("io", format!("the DWG converter failed: {e}"))),
            }
        }
        let made = output.join(format!("{stem}.{to_ext}"));
        match std::fs::read(&made) {
            Ok(out) if !out.is_empty() => Ok(out),
            _ => Err(IpcError::new(
                "invalid",
                format!(
                    "the ODA File Converter produced no {} file. The drawing may be damaged or in a version it cannot read",
                    to_ext.to_ascii_uppercase()
                ),
            )),
        }
    })();
    let _ = std::fs::remove_dir_all(&work);
    result
}

// -------------------------------------------------------------- file inputs

/// A caller path. The desktop app gets it from the native open dialog, so it
/// is used as given but must be absolute and free of `..`. The dev bridge
/// only reads inside its own data dir.
fn check_read_path(data_dir: &Path, raw: &str, external_paths: bool) -> Result<PathBuf, IpcError> {
    let path = PathBuf::from(raw.trim());
    let traversal = path.components().any(|c| matches!(c, std::path::Component::ParentDir));
    if !path.is_absolute() || traversal || path.file_name().is_none() {
        return Err(IpcError::new("invalid", "the file path must be an absolute file path"));
    }
    if !external_paths && !path.starts_with(data_dir) {
        return Err(IpcError::new(
            "forbidden",
            format!(
                "this service only reads inside {}. Send the file as `file_name` and `data` instead",
                data_dir.display()
            ),
        ));
    }
    if !path.is_file() {
        return Err(IpcError::new("not_found", format!("file not found: {}", path.display())));
    }
    Ok(path)
}

/// Read `{path}` or `{file_name, data}` into a safe file name and its bytes.
async fn read_input(
    app: &AppService,
    args: &Value,
    allowed: &[&str],
    max_bytes: usize,
) -> Result<(String, Vec<u8>), IpcError> {
    let path: Option<String> = arg(args, "path")?;
    let (name, bytes) = match path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()) {
        Some(p) => {
            let (data_dir, external) = {
                let s = app.session.lock().await;
                (s.data_dir.clone(), app.external_paths)
            };
            let target = check_read_path(&data_dir, &p, external)?;
            let meta = std::fs::metadata(&target).map_err(|e| files::io_err("cannot read", &target, e))?;
            if meta.len() as usize > max_bytes {
                return Err(IpcError::new(
                    "invalid",
                    format!("the file is larger than {} MB", max_bytes / (1024 * 1024)),
                ));
            }
            let bytes = std::fs::read(&target).map_err(|e| files::io_err("cannot read", &target, e))?;
            let name = target
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();
            (name, bytes)
        }
        None => {
            let file_name: String = arg(args, "file_name")?;
            let data: String = arg(args, "data")?;
            (file_name, files::decode_any_data_url(&data, max_bytes)?)
        }
    };
    let safe = files::safe_file_name(&name)?;
    let ext = files::split_ext(&safe).1.to_ascii_lowercase();
    if !allowed.contains(&ext.as_str()) {
        return Err(IpcError::new(
            "invalid",
            format!("`{safe}` is not a supported file. This command reads: {}", allowed.join(", ")),
        ));
    }
    Ok((safe, bytes))
}

/// A DXF or DWG, converted to DXF bytes when needed. The reported file name
/// is always the one the user picked, so element names keep saying `.dwg`.
async fn drawing_input(app: &AppService, args: &Value) -> Result<(String, Vec<u8>), IpcError> {
    let (name, bytes) = read_input(app, args, &["dxf", "dwg"], guhit_import::MAX_BYTES).await?;
    if files::split_ext(&name).1.to_ascii_lowercase() != "dwg" {
        return Ok((name, bytes));
    }
    let dir = data_dir(app).await;
    let stem = files::split_ext(&name).0.to_string();
    let dxf = tokio::task::spawn_blocking(move || convert(&dir, &stem, "dwg", "dxf", &bytes))
        .await
        .map_err(join_err)??;
    Ok((name, dxf))
}

// ------------------------------------------------------------------ import

async fn import_commit(app: &AppService, args: Value) -> IpcResult {
    let (file_name, bytes) = drawing_input(app, &args).await?;
    let mut options: ImportOptions = arg(&args, "options")?;
    if options.level_id.is_none() {
        let s = app.session.lock().await;
        let doc = s.open_doc()?;
        options.level_id = doc.project().levels.first().map(|l| l.id.clone());
    }
    if options.level_id.is_none() {
        return Err(no_document());
    }
    let plan = guhit_import::plan(&file_name, &bytes, &options).map_err(import_err)?;
    if plan.is_empty() {
        return Err(IpcError::new(
            "invalid",
            format!(
                "nothing in {file_name} could be imported. Try Linework mode, or check that the layers you picked hold lines"
            ),
        ));
    }
    let applied = app.commit(plan.batch(&file_name), Origin::User).await?;
    to_value(&ImportResult {
        state: applied.state,
        walls_added: plan.walls,
        linework_added: plan.linework,
        skipped: plan.skipped,
    })
}

// --------------------------------------------------------- reference models

async fn model_store(app: &AppService, args: Value) -> IpcResult {
    let exts: Vec<&str> = MODEL_EXTS.iter().map(|(e, _)| *e).collect();
    let (file_name, bytes) = read_input(app, &args, &exts, MAX_MODEL_BYTES).await?;
    let (stem, ext) = files::split_ext(&file_name);
    let (stem, ext) = (stem.to_string(), ext.to_ascii_lowercase());
    let s = app.session.lock().await;
    let dir = s.open_project_dir()?.join("models");
    files::create_dir(&dir)?;
    // Never overwrite: a ReferenceModel element may already point at that file.
    let target = files::unique_path(&dir, &stem, &ext);
    files::write_atomic(&target, &bytes)?;
    let stored = target.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    Ok(json!({ "file_name": stored, "size": bytes.len() }))
}

async fn model_data(app: &AppService, args: Value) -> IpcResult {
    let file_name: String = arg(&args, "file_name")?;
    files::check_file_name(&file_name)?;
    let ext = files::split_ext(&file_name).1.to_ascii_lowercase();
    let mime = MODEL_EXTS
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, m)| *m)
        .ok_or_else(|| IpcError::new("invalid", "reference models are glTF, GLB or OBJ files"))?;
    let s = app.session.lock().await;
    let path = s.open_project_dir()?.join("models").join(&file_name);
    let bytes = std::fs::read(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => IpcError::new("not_found", format!("model not found: {file_name}")),
        _ => files::io_err("cannot read", &path, e),
    })?;
    to_value(&files::encode_any_data_url(mime, &bytes))
}

// ----------------------------------------------------------- model exports

async fn export_model(app: &AppService, args: Value) -> IpcResult {
    let format: ModelFormat = arg(&args, "format")?;
    let path: Option<String> = arg(&args, "path")?;
    let (project, derived, data_dir, external) = {
        let s = app.session.lock().await;
        let doc = s.open_doc()?;
        (
            doc.project().clone(),
            doc.derived().clone(),
            s.data_dir.clone(),
            app.external_paths,
        )
    };
    let stem = files::slug(&project.name, "model");
    // IFC and 3D DXF are written by `guhit-export`; DWG goes out as a 2D DXF
    // and through the converter. One command covers all three.
    let text = match format {
        ModelFormat::Ifc => Some(("ifc", guhit_export::model_ifc(&project, &derived).map_err(export_err)?)),
        ModelFormat::Dxf3d => Some(("dxf", guhit_export::model_dxf3d(&project, &derived).map_err(export_err)?)),
        ModelFormat::Dwg => None,
    };
    if let Some((ext, data)) = text {
        let target = crate::export_target(&data_dir, path.as_deref(), &stem, ext, external)?;
        files::write_atomic(&target, data.as_bytes())?;
        return to_value(&ExportResult {
            path: target.to_string_lossy().into_owned(),
            scale_denominator: None,
        });
    }
    match format {
        ModelFormat::Ifc | ModelFormat::Dxf3d => unreachable!("handled above"),
        ModelFormat::Dwg => {
            // Say "set up the converter" before anything else can go wrong.
            converter_exe(&data_dir)?;
            let opts = PlanExportOptions {
                level_id: None,
                paper: project.settings.paper,
                orientation: Orientation::Landscape,
                scale_denominator: None,
                show_dimensions: true,
                show_room_labels: true,
                show_assets: true,
                title_block: false,
                show_pipes: true,
            };
            let dxf = guhit_export::plan_dxf(&project, &derived, &opts).map_err(export_err)?;
            let dir = data_dir.clone();
            let stem2 = stem.clone();
            let dwg = tokio::task::spawn_blocking(move || {
                convert(&dir, &stem2, "dxf", "dwg", dxf.data.as_bytes())
            })
            .await
            .map_err(join_err)??;
            let target = crate::export_target(&data_dir, path.as_deref(), &stem, "dwg", external)?;
            files::write_atomic(&target, &dwg)?;
            to_value(&ExportResult {
                path: target.to_string_lossy().into_owned(),
                scale_denominator: None,
            })
        }
    }
}

// ------------------------------------------------------------ .guhit bundle

/// Folders inside a project that travel with the bundle, plus `project.json`
/// and the thumbnail. `ai-log.jsonl` stays behind: it is a private record of
/// what was asked of the copilot.
const BUNDLE_DIRS: [&str; 4] = ["snapshots", "underlays", "renders", "models"];
const AI_LOG: &str = "ai-log.jsonl";

fn collect_files(root: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) -> Result<(), IpcError> {
    let dir = if rel.is_empty() { root.to_path_buf() } else { root.join(rel) };
    let mut entries: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(e) => e.flatten().map(|e| e.path()).collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(files::io_err("cannot read", &dir, e)),
    };
    entries.sort();
    for path in entries {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') || name == AI_LOG {
            continue;
        }
        let entry = if rel.is_empty() {
            name.to_string()
        } else {
            format!("{rel}/{name}")
        };
        if path.is_dir() {
            if rel.is_empty() && !BUNDLE_DIRS.contains(&name) {
                continue;
            }
            collect_files(root, &entry, out)?;
        } else if path.is_file() {
            out.push((entry, path));
        }
    }
    Ok(())
}

fn zip_folder(root: &Path) -> Result<Vec<u8>, IpcError> {
    let mut files_in = vec![];
    collect_files(root, "", &mut files_in)?;
    let io = |e: std::io::Error| IpcError::new("io", format!("cannot write the bundle: {e}"));
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, path) in files_in {
        let bytes = std::fs::read(&path).map_err(|e| files::io_err("cannot read", &path, e))?;
        writer
            .start_file(name, options)
            .map_err(|e| IpcError::new("io", format!("cannot write the bundle: {e}")))?;
        writer.write_all(&bytes).map_err(io)?;
    }
    let cursor = writer
        .finish()
        .map_err(|e| IpcError::new("io", format!("cannot write the bundle: {e}")))?;
    Ok(cursor.into_inner())
}

async fn bundle_save(app: &AppService, args: Value) -> IpcResult {
    let path: Option<String> = arg(&args, "path")?;
    let mut s = app.session.lock().await;
    // Everything on disk first, so the bundle holds the current plan.
    s.flush()?;
    let project_dir = s.open_project_dir()?;
    let name = s.open_doc()?.project().name.clone();
    let data_dir = s.data_dir.clone();
    let bytes = zip_folder(&project_dir)?;
    let target = crate::export_target(
        &data_dir,
        path.as_deref(),
        &files::slug(&name, "project"),
        "guhit",
        app.external_paths,
    )?;
    files::write_atomic(&target, &bytes)?;
    to_value(&ExportResult {
        path: target.to_string_lossy().into_owned(),
        scale_denominator: None,
    })
}

/// Reject anything that could escape the folder we are about to create:
/// absolute paths, `..`, Windows drive letters and roots.
fn safe_entry_name(raw: &str) -> Option<String> {
    if raw.is_empty() || raw.contains('\0') || raw.contains('\\') {
        return None;
    }
    if raw.starts_with('/') || raw.chars().nth(1) == Some(':') {
        return None;
    }
    let parts: Vec<&str> = raw.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    if parts.is_empty() || parts.iter().any(|p| *p == "..") {
        return None;
    }
    // Every element must survive the same name rules the rest of the app uses.
    for p in &parts {
        match files::safe_file_name(p) {
            Ok(safe) if safe == **p => {}
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

struct Bundle {
    /// Entry path relative to the project folder, to its bytes.
    entries: Vec<(String, Vec<u8>)>,
    project: Project,
    project_json: Vec<u8>,
}

fn read_bundle(bytes: &[u8]) -> Result<Bundle, IpcError> {
    let bad = |why: &str| IpcError::new("invalid", format!("this is not a Guhit project bundle: {why}"));
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| bad(&format!("{e}")))?;
    if archive.len() > MAX_BUNDLE_ENTRIES {
        return Err(bad("it holds too many files"));
    }
    // A bundle may carry one wrapping folder. The project.json nearest the
    // root decides where the project folder starts.
    let mut names: Vec<(usize, String)> = vec![];
    for i in 0..archive.len() {
        let file = archive.by_index(i).map_err(|e| bad(&format!("{e}")))?;
        if file.is_dir() {
            continue;
        }
        let raw = file.name().to_string();
        if file.enclosed_name().is_none() {
            return Err(bad(&format!("the entry `{raw}` points outside the bundle")));
        }
        let Some(name) = safe_entry_name(&raw) else {
            return Err(bad(&format!("the entry `{raw}` is not a safe file name")));
        };
        if file.size() as usize > MAX_BUNDLE_BYTES {
            return Err(bad("one of its files is too large"));
        }
        names.push((i, name));
    }
    let prefix = names
        .iter()
        .filter(|(_, n)| n == "project.json" || n.ends_with("/project.json"))
        .map(|(_, n)| n.trim_end_matches("project.json").to_string())
        .min_by_key(|p| p.len())
        .ok_or_else(|| bad("it has no project.json"))?;

    let mut entries = vec![];
    let mut total = 0usize;
    for (i, name) in names {
        let Some(rel) = name.strip_prefix(&prefix) else {
            continue;
        };
        if rel.is_empty() || rel == AI_LOG {
            continue;
        }
        let mut file = archive.by_index(i).map_err(|e| bad(&format!("{e}")))?;
        let mut buf = Vec::with_capacity(file.size().min(1 << 20) as usize);
        file.read_to_end(&mut buf)
            .map_err(|e| IpcError::new("invalid", format!("the bundle could not be read: {e}")))?;
        total += buf.len();
        if total > MAX_BUNDLE_BYTES {
            return Err(bad("it expands to more than 200 MB"));
        }
        entries.push((rel.to_string(), buf));
    }
    let project_json = entries
        .iter()
        .find(|(n, _)| n == "project.json")
        .map(|(_, b)| b.clone())
        .ok_or_else(|| bad("it has no project.json"))?;
    let project = store::parse_project(&project_json, Path::new("project.json"))?;
    Ok(Bundle {
        entries,
        project,
        project_json,
    })
}

async fn bundle_open(app: &AppService, args: Value) -> IpcResult {
    let (_, bytes) = read_input(app, &args, &["guhit", "zip"], MAX_BUNDLE_BYTES).await?;
    let bundle = read_bundle(&bytes)?;

    let mut s = app.session.lock().await;
    let data_dir = s.data_dir.clone();
    let mut id = bundle.project.id.clone();
    let mut name = bundle.project.name.clone();
    let mut rename = false;
    if files::check_id(&id, "project").is_err() {
        id = defaults::new_id();
    }
    let existing = store::projects_dir(&data_dir).join(&id).join(store::PROJECT_FILE);
    if existing.is_file() {
        let same = std::fs::read(&existing).map(|b| b == bundle.project_json).unwrap_or(false);
        if !same {
            id = defaults::new_id();
            name = format!("{name} (imported)");
            rename = true;
        }
    }

    let dir = store::project_dir(&data_dir, &id)?;
    files::create_dir(&dir)?;
    for (rel, data) in &bundle.entries {
        if rel == "project.json" {
            continue;
        }
        let target = dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        // Belt and braces: the name was checked, now check the result.
        if !target.starts_with(&dir) {
            return Err(IpcError::new("invalid", format!("the entry `{rel}` points outside the project")));
        }
        files::write_atomic(&target, data)?;
    }
    let mut project = bundle.project;
    project.id = id.clone();
    project.name = name;
    if rename {
        project.updated_at = defaults::now_rfc3339();
    }
    store::save_project(&data_dir, &project)?;

    let project = store::load_project(&data_dir, &id)?;
    s.set_doc(project)?;
    app.notify(&s);
    to_value(&s.state()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_entry_names_are_checked() {
        assert_eq!(safe_entry_name("project.json").as_deref(), Some("project.json"));
        assert_eq!(
            safe_entry_name("snapshots/a-b.json").as_deref(),
            Some("snapshots/a-b.json")
        );
        assert_eq!(safe_entry_name("./project.json").as_deref(), Some("project.json"));
        assert_eq!(safe_entry_name("../evil.json"), None);
        assert_eq!(safe_entry_name("a/../../evil.json"), None);
        assert_eq!(safe_entry_name("/etc/passwd"), None);
        assert_eq!(safe_entry_name("C:/Windows/system32"), None);
        assert_eq!(safe_entry_name("a\\b.json"), None);
        assert_eq!(safe_entry_name(""), None);
        assert_eq!(safe_entry_name("con.txt"), None, "a Windows device name is rewritten, so it fails");
    }

    #[test]
    fn a_mac_app_bundle_resolves_to_its_binary() {
        // Nothing on disk, so the path comes back unchanged.
        let p = Path::new("/Applications/ODAFileConverter.app");
        assert_eq!(resolve_executable(p), p);
        let plain = Path::new("/usr/local/bin/ODAFileConverter");
        assert_eq!(resolve_executable(plain), plain);
    }
}
