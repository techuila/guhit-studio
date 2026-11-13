//! The sheet set and what goes on it (DECISIONS D53, D54): sheet export and
//! previews, views, files in the project's `files/` folder (the firm logo,
//! the vicinity map), and this computer's signing architect profile in
//! settings.json. LANE L4 owns this module; `lib.rs` routes `OWNS` here.
//!
//! - `export_sheets`: PDF is one file for the whole set, SVG one file per
//!   sheet (`<name>-A-1.svg`, ...), DXF the views side by side (lane L3).
//!   The PDF on disk prints the signatory in full. A project with no sheets
//!   exports the default set (`guhit_export::sheetset::default_set`).
//! - `sheet_preview`, `view_svg`: SVG for the Sheets panel and the views
//!   viewer, cached by a hash of the project and its sheet images (pictures
//!   are pure functions of them). Elevations and sections come from
//!   lane L3's `view_svg` once built; until then, and for plans, schedules,
//!   notes, the vicinity map and perspectives, from `sheetset::view_alone`.
//! - Images on sheets: the firm logo and vicinity maps from `files/`,
//!   perspectives from Visuals, model views only. An AI image never goes on
//!   a sheet (D54).
//! - `professional_get`, `professional_set`: the profile in settings.json
//!   under `professional`. Only the window calls them; the title block takes
//!   the profile only when a person asks (`SetSignatory`).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use guhit_export::{sheetset, ExportError, SheetInputs};
use guhit_model::*;
use serde_json::{json, Value};

use crate::files::{self, ImageKind};
use crate::{arg, export_err, export_target, local_date, no_document, renders, to_value, AppService, IpcResult, Session};

/// The IPC commands this module answers.
pub const OWNS: &[&str] = &[
    "export_sheets",
    "sheet_preview",
    "view_svg",
    "file_store",
    "file_data",
    "professional_get",
    "professional_set",
];

/// settings.json key of this computer's signing architect profile.
pub const PROFILE_KEY: &str = "professional";

/// Folder of the project's own files (logo, vicinity map).
const FILES_DIR: &str = "files";

/// Pictures by "project:content hash:what" (`content_key`). Small: the panel
/// shows one sheet and one view at a time.
fn cache() -> &'static Mutex<HashMap<String, Value>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached(key: &str) -> Option<Value> {
    cache().lock().ok()?.get(key).cloned()
}

fn remember(key: String, value: &Value) {
    if let Ok(mut c) = cache().lock() {
        if c.len() >= 48 {
            c.clear();
        }
        c.insert(key, value.clone());
    }
}

/// The images a sheet of `project` can show, read from its folder: the firm
/// logo and vicinity maps from `files/`, perspectives from Visuals (model
/// views only). A missing image is left out; the sheet frames it empty.
pub fn sheet_images(project_dir: &Path, project: &Project) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    let (names, render_ids) = sheetset::image_keys(project);
    for name in names {
        if files::check_file_name(&name).is_err() {
            continue;
        }
        if let Ok(bytes) = std::fs::read(project_dir.join(FILES_DIR).join(&name)) {
            out.insert(name, bytes);
        }
    }
    for id in render_ids {
        let model_view = matches!(renders::find(project_dir, &id), Ok(Some(r)) if r.source == RenderSource::ModelView);
        if !model_view {
            continue;
        }
        if let Ok((_, bytes)) = renders::read_image(project_dir, &id) {
            out.insert(id, bytes);
        }
    }
    out
}

/// The project, its derived data, its sheet images and a cache key prefix.
type Snapshot = (Project, Derived, std::collections::BTreeMap<String, Vec<u8>>, String);

/// What a picture of the open project needs: the project, its derived data,
/// the images and a cache key prefix. Read under the lock, drawn after it.
fn snapshot(s: &Session) -> Result<Snapshot, IpcError> {
    let doc = s.doc.as_ref().ok_or_else(no_document)?;
    let project = doc.project().clone();
    let images = match s.open_project_dir() {
        Ok(dir) => sheet_images(&dir, &project),
        Err(_) => Default::default(),
    };
    let key = content_key(&project, &images);
    Ok((project, doc.derived().clone(), images, key))
}

/// A cache key for pictures of `project` with `images`: a hash of what they
/// are made from. The revision is not one: it starts again at 0 on every
/// open, and a rename or a new vicinity map keeps it, so a picture from
/// before would come back (GR1 #5).
fn content_key(project: &Project, images: &std::collections::BTreeMap<String, Vec<u8>>) -> String {
    use std::hash::{Hash, Hasher};
    /// Feeds what serde writes straight into the hasher, with no copy.
    struct Feed<'a>(&'a mut std::collections::hash_map::DefaultHasher);
    impl std::io::Write for Feed<'_> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.write(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    // The derived data and every picture follow from the project alone.
    let _ = serde_json::to_writer(Feed(&mut h), project);
    images.hash(&mut h);
    format!("{}:{:016x}", project.id, h.finish())
}

/// A sheet or view picture that is the same for any date, for the panel.
fn preview_inputs(images: std::collections::BTreeMap<String, Vec<u8>>) -> SheetInputs {
    SheetInputs { date: Some(local_date()), images, mask_signatory: false }
}

pub async fn handle(app: &AppService, cmd: &str, args: Value) -> IpcResult {
    match cmd {
        "export_sheets" => {
            let opts: SheetExportOptions =
                serde_json::from_value(args).map_err(|e| IpcError::new("bad_args", e.to_string()))?;
            export_sheets(app, opts).await
        }
        "sheet_preview" => {
            let sheet_id: Id = arg(&args, "sheet_id")?;
            let (project, derived, images, key) = snapshot(&*app.session.lock().await)?;
            let known = if project.sheet_set.sheets.is_empty() {
                sheetset::default_set(&project, project.settings.paper).iter().any(|s| s.id == sheet_id)
            } else {
                project.sheet_set.sheets.iter().any(|s| s.id == sheet_id)
            };
            if !known {
                return Err(IpcError::new("not_found", format!("there is no sheet with id {sheet_id}")));
            }
            let key = format!("{key}:sheet:{sheet_id}:{}", local_date());
            if let Some(v) = cached(&key) {
                return Ok(v);
            }
            // The panel shows why a viewport is empty and how to fill it; a printed sheet never does.
            let svg = guhit_export::sheetset::sheet_preview_svg(&project, &derived, &sheet_id, &preview_inputs(images)).map_err(export_err)?;
            let v = to_value(&SvgDrawing { svg, scale_denominator: None })?;
            remember(key, &v);
            Ok(v)
        }
        "view_svg" => {
            let view: ViewRef = arg(&args, "view")?;
            let scale: Option<u32> = arg::<Option<u32>>(&args, "scale")?.filter(|n| *n > 0);
            let (project, derived, images, key) = snapshot(&*app.session.lock().await)?;
            let key = format!("{key}:view:{}:{scale:?}", serde_json::to_string(&view).unwrap_or_default());
            if let Some(v) = cached(&key) {
                return Ok(v);
            }
            let inputs = preview_inputs(images);
            let out = view_svg(&project, &derived, &view, scale, &inputs).map_err(export_err)?;
            let v = to_value(&SvgDrawing { svg: out.data, scale_denominator: Some(out.scale_denominator) })?;
            remember(key, &v);
            Ok(v)
        }
        "file_store" => {
            let file_name: String = arg(&args, "file_name")?;
            let data: String = arg(&args, "data")?;
            let (kind, bytes) = files::decode_image_data_url(&data)?;
            let s = app.session.lock().await;
            let dir = s.open_project_dir()?;
            let stored = store_file(&dir, &file_name, kind, &bytes)?;
            Ok(json!({ "file_name": stored }))
        }
        "file_data" => {
            let file_name: String = arg(&args, "file_name")?;
            files::check_file_name(&file_name)?;
            let kind = ImageKind::from_ext(files::split_ext(&file_name).1)
                .ok_or_else(|| IpcError::new("invalid", "project files are PNG or JPEG images"))?;
            let s = app.session.lock().await;
            let dir = s.open_project_dir()?;
            Ok(Value::String(files::read_data_url(&dir.join(FILES_DIR).join(&file_name), kind)?))
        }
        "professional_get" => {
            let s = app.session.lock().await;
            to_value(&profile(&s.data_dir))
        }
        "professional_set" => {
            let signatory: Signatory = arg(&args, "signatory")?;
            let s = app.session.lock().await;
            let clean = trimmed(&signatory);
            let value = if clean.is_empty() { None } else { Some(to_value(&clean)?) };
            files::put_setting(&s.data_dir, PROFILE_KEY, value)?;
            to_value(&clean)
        }
        _ => Err(IpcError::new("unknown_command", format!("unknown command `{cmd}`"))),
    }
}

/// This computer's signing architect profile; empty when never saved.
pub fn profile(data_dir: &Path) -> Signatory {
    files::read_settings(data_dir)
        .get(PROFILE_KEY)
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

fn trimmed(s: &Signatory) -> Signatory {
    let t = |v: &String| v.trim().to_string();
    Signatory {
        name: t(&s.name),
        prc_no: t(&s.prc_no),
        prc_valid_until: t(&s.prc_valid_until),
        ptr_no: t(&s.ptr_no),
        ptr_date: t(&s.ptr_date),
        ptr_place: t(&s.ptr_place),
        iapoa_no: t(&s.iapoa_no),
        iapoa_or_no: t(&s.iapoa_or_no),
        iapoa_date: t(&s.iapoa_date),
        tin: t(&s.tin),
    }
}

/// One view alone: lane L3's drawing once built, else the view as a sheet
/// frames it (plans, schedules, notes, images, and views not built yet).
pub fn view_svg(
    project: &Project,
    derived: &Derived,
    view: &ViewRef,
    scale: Option<u32>,
    inputs: &SheetInputs,
) -> Result<guhit_export::PlanOutput<String>, ExportError> {
    let theirs = matches!(
        view,
        ViewRef::Elevation { .. }
            | ViewRef::Section { .. }
            | ViewRef::RoofPlan
            | ViewRef::SitePlan
            | ViewRef::Plan { sheet: SheetKind::ReflectedCeiling, .. }
    );
    if theirs {
        match guhit_export::view_svg(project, derived, view, scale, inputs) {
            Err(ExportError::NotBuilt(_)) => {}
            other => return other,
        }
    }
    sheetset::view_alone(project, derived, view, scale, inputs)
}

/// Store an image in the project's `files/` folder under a free name with
/// the extension of its real type. Returns the name it got.
fn store_file(project_dir: &Path, file_name: &str, kind: ImageKind, bytes: &[u8]) -> Result<String, IpcError> {
    let safe = files::safe_file_name(file_name)?;
    let stem = match files::split_ext(&safe) {
        (stem, ext) if ImageKind::from_ext(ext).is_some() => stem.to_string(),
        _ => safe.replace('.', "_"),
    };
    let dir = project_dir.join(FILES_DIR);
    files::create_dir(&dir)?;
    // Never overwrite: a sheet may still show the old one.
    let target = files::unique_path(&dir, &stem, kind.ext());
    files::write_atomic(&target, bytes)?;
    Ok(target.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string())
}

async fn export_sheets(app: &AppService, opts: SheetExportOptions) -> IpcResult {
    let date = opts.date.clone().filter(|d| !d.trim().is_empty()).unwrap_or_else(local_date);
    let (project, derived, images, data_dir, exports) = {
        let s = app.session.lock().await;
        let (project, derived, images, _) = snapshot(&s)?;
        (project, derived, images, s.data_dir.clone(), s.exports_dir())
    };
    // The PDF on disk prints the license numbers in full (D53).
    let inputs = SheetInputs { date: Some(date), images, mask_signatory: false };
    let stem = format!("{}-permit-set", files::slug(&project.name, "project"));
    // The export says what it left out, since the sheets themselves never do.
    let notes = sheetset::left_out(&project, &derived, &opts.sheet_ids, &inputs).unwrap_or_default();
    let path = opts.path.as_deref();
    let (target, scale) = match opts.format {
        PlanFormat::Pdf => {
            let bytes = guhit_export::sheets_pdf(&project, &derived, &opts.sheet_ids, &inputs).map_err(export_err)?;
            let target = export_target(&data_dir, &exports, path, &stem, "pdf", app.external_paths)?;
            files::write_atomic(&target, &bytes)?;
            (target, None)
        }
        PlanFormat::Svg => {
            let pages = sheetset::sheets_svg(&project, &derived, &opts.sheet_ids, &inputs).map_err(export_err)?;
            let first = export_target(&data_dir, &exports, path, &stem, "svg", app.external_paths)?;
            if pages.len() == 1 {
                files::write_atomic(&first, pages[0].1.as_bytes())?;
            } else {
                // One file per sheet beside the chosen name: house-A-1.svg.
                let dir = first.parent().map(Path::to_path_buf).unwrap_or_else(|| exports.clone());
                let base = first.file_stem().and_then(|s| s.to_str()).unwrap_or("sheet").to_string();
                let mut written = Vec::new();
                for (number, svg) in &pages {
                    let target = files::unique_path(&dir, &format!("{base}-{}", files::slug(number, "sheet")), "svg");
                    files::write_atomic(&target, svg.as_bytes())?;
                    written.push(target);
                }
                return to_value(&ExportResult { path: written[0].to_string_lossy().into_owned(), scale_denominator: None, notes });
            }
            (first, None)
        }
        PlanFormat::Dxf => {
            let text = guhit_export::sheets_dxf(&project, &derived, &opts.sheet_ids).map_err(export_err)?;
            let target = export_target(&data_dir, &exports, path, &stem, "dxf", app.external_paths)?;
            files::write_atomic(&target, text.as_bytes())?;
            (target, None)
        }
    };
    to_value(&ExportResult { path: target.to_string_lossy().into_owned(), scale_denominator: scale, notes })
}
