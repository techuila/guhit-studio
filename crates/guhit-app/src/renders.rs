//! Saved visuals: `renders/<id>.<ext>` plus `renders/index.json`, a list of
//! `RenderRecord`. Every record is tied to the document revision and camera
//! it was captured from.
//!
//! Two kinds live here side by side (DECISIONS D17). A `ModelView` record is
//! a Tier 1 deterministic capture of the 3D view and is always a PNG. An
//! `AiVisualization` record is a Tier 2 generated image, PNG or JPEG, and
//! carries `source_render_id` pointing at the capture it was made from. The
//! two are never entangled: deleting a capture leaves its visualizations in
//! place, so a comparison the architect already showed a client does not
//! vanish with a tidy-up.

use std::path::{Path, PathBuf};

use guhit_model::*;

use crate::files::{self, ImageKind};
use crate::store;

/// Image types a record can be stored as, in the order they are probed for.
const KINDS: [ImageKind; 2] = [ImageKind::Png, ImageKind::Jpeg];

fn dir_of(project_dir: &Path) -> PathBuf {
    project_dir.join("renders")
}

fn index_path(project_dir: &Path) -> PathBuf {
    dir_of(project_dir).join("index.json")
}

fn image_path_with(project_dir: &Path, id: &str, kind: ImageKind) -> Result<PathBuf, IpcError> {
    files::check_id(id, "render")?;
    Ok(dir_of(project_dir).join(format!("{id}.{}", kind.ext())))
}

/// The file a record is actually stored in, and its type. The extension is
/// not in the index, so it is found on disk; PNG is the answer when nothing
/// is there yet, which keeps the old captures-are-always-PNG behaviour.
fn stored_image(project_dir: &Path, id: &str) -> Result<(ImageKind, PathBuf), IpcError> {
    files::check_id(id, "render")?;
    for kind in KINDS {
        let path = image_path_with(project_dir, id, kind)?;
        if path.is_file() {
            return Ok((kind, path));
        }
    }
    Ok((ImageKind::Png, image_path_with(project_dir, id, ImageKind::Png)?))
}

fn image_path(project_dir: &Path, id: &str) -> Result<PathBuf, IpcError> {
    Ok(stored_image(project_dir, id)?.1)
}

/// Records as stored, oldest first. A damaged index is set aside (renamed,
/// not removed) and treated as empty, so captures keep working.
fn load_index(project_dir: &Path) -> Result<Vec<RenderRecord>, IpcError> {
    let path = index_path(project_dir);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(files::io_err("cannot read", &path, e)),
    };
    match serde_json::from_slice::<Vec<RenderRecord>>(&bytes) {
        Ok(mut records) => {
            // Paths are rebuilt from the current folder, so a moved data dir
            // and ids edited by hand cannot point outside `renders/`.
            records.retain(|r| files::check_id(&r.id, "render").is_ok());
            for r in &mut records {
                if let Ok(path) = image_path(project_dir, &r.id) {
                    r.image_path = path.to_string_lossy().into_owned();
                }
            }
            Ok(records)
        }
        Err(e) => {
            let aside = files::unique_path(&dir_of(project_dir), &format!("index.damaged-{}", files::file_timestamp()), "json");
            eprintln!("guhit-app: renders index is damaged ({e}), moved to {}", aside.display());
            let _ = std::fs::rename(&path, &aside);
            Ok(vec![])
        }
    }
}

fn save_index(project_dir: &Path, records: &[RenderRecord]) -> Result<(), IpcError> {
    files::write_json_atomic(&index_path(project_dir), &records)
}

/// Newest first.
pub fn list(project_dir: &Path) -> Result<Vec<RenderRecord>, IpcError> {
    let mut records = load_index(project_dir)?;
    records.reverse();
    Ok(records)
}

/// One record by id, or None. CONTRACT: `render_ai_generate` resolves its
/// source this way.
pub fn find(project_dir: &Path, id: &str) -> Result<Option<RenderRecord>, IpcError> {
    files::check_id(id, "render")?;
    Ok(load_index(project_dir)?.into_iter().find(|r| r.id == id))
}

/// The stored bytes of a record's image, with its type.
pub fn read_image(project_dir: &Path, id: &str) -> Result<(ImageKind, Vec<u8>), IpcError> {
    let (kind, path) = stored_image(project_dir, id)?;
    let bytes = std::fs::read(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => IpcError::new("not_found", format!("render not found: {id}")),
        _ => files::io_err("cannot read", &path, e),
    })?;
    Ok((kind, bytes))
}

/// Tier 1: a deterministic capture of the live 3D view, or a render of it.
pub fn capture(
    project_dir: &Path,
    revision: u32,
    camera: Camera,
    png_data_url: &str,
    info: Option<RenderInfo>,
) -> Result<RenderRecord, IpcError> {
    let info = info.map(clean_info).transpose()?;
    let png = files::decode_png_data_url(png_data_url)?;
    let id = defaults::new_id();
    let path = image_path_with(project_dir, &id, ImageKind::Png)?;
    files::write_atomic(&path, &png)?;
    let record = RenderRecord {
        id,
        created_at: defaults::now_rfc3339(),
        source: RenderSource::ModelView,
        revision,
        camera,
        style_key: None,
        prompt: String::new(),
        image_path: path.to_string_lossy().into_owned(),
        source_render_id: None,
        provider: None,
        info,
    };
    let mut records = load_index(project_dir)?;
    records.push(record.clone());
    save_index(project_dir, &records)?;
    Ok(record)
}

/// Longest adapter name kept in a record.
const MAX_GPU_CHARS: usize = 160;

/// Render info as sent by the UI: sizes and times must be real numbers, the
/// adapter name is kept short and free of control characters.
fn clean_info(mut info: RenderInfo) -> Result<RenderInfo, IpcError> {
    if info.width == 0 || info.height == 0 {
        return Err(IpcError::new("bad_args", "argument `info`: width and height must be at least 1 pixel"));
    }
    if !info.seconds.is_finite() || info.seconds < 0.0 {
        return Err(IpcError::new("bad_args", "argument `info`: seconds must be a number of seconds, 0 or more"));
    }
    info.gpu = info.gpu.chars().filter(|c| !c.is_control()).take(MAX_GPU_CHARS).collect::<String>().trim().to_string();
    Ok(info)
}

/// Tier 2: a generated image, saved next to the capture it came from. The
/// revision and camera are copied from the source, because that is what the
/// image actually shows; nothing about the model is re-read here.
#[allow(clippy::too_many_arguments)]
pub fn save_ai(
    project_dir: &Path,
    source: &RenderRecord,
    kind: ImageKind,
    bytes: &[u8],
    style_key: Option<String>,
    prompt: String,
    provider: &str,
) -> Result<RenderRecord, IpcError> {
    if bytes.is_empty() {
        return Err(IpcError::new("ai_failed", "the image model returned an empty image"));
    }
    let id = defaults::new_id();
    let path = image_path_with(project_dir, &id, kind)?;
    files::write_atomic(&path, bytes)?;
    let record = RenderRecord {
        id,
        created_at: defaults::now_rfc3339(),
        source: RenderSource::AiVisualization,
        revision: source.revision,
        camera: source.camera.clone(),
        style_key,
        prompt,
        image_path: path.to_string_lossy().into_owned(),
        source_render_id: Some(source.id.clone()),
        provider: Some(provider.to_string()),
        info: None,
    };
    let mut records = load_index(project_dir)?;
    records.push(record.clone());
    // The index is the record of truth. If it cannot be written, the orphan
    // image goes too, so a retry does not leave files nobody can see.
    if let Err(e) = save_index(project_dir, &records) {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    Ok(record)
}

pub fn data(project_dir: &Path, id: &str) -> Result<String, IpcError> {
    let (kind, path) = stored_image(project_dir, id)?;
    if !path.is_file() {
        return Err(IpcError::new("not_found", format!("render not found: {id}")));
    }
    files::read_data_url(&path, kind)
}

/// Drops the record and moves the image into `trash/`. Only that one record:
/// deleting a capture leaves any AI visualization made from it alone, and
/// deleting a visualization never touches its source.
pub fn delete(data_dir: &Path, project_dir: &Path, project_id: &str, id: &str) -> Result<(), IpcError> {
    let path = image_path(project_dir, id)?;
    let mut records = load_index(project_dir)?;
    let before = records.len();
    records.retain(|r| r.id != id);
    if records.len() == before && !path.is_file() {
        return Err(IpcError::new("not_found", format!("render not found: {id}")));
    }
    save_index(project_dir, &records)?;
    store::trash_file(data_dir, &path, &format!("{project_id}-render"))
}
