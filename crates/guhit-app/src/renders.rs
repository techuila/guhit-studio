//! Saved visuals: `renders/<id>.png` plus `renders/index.json`, a list of
//! `RenderRecord`. Every record is tied to the document revision and camera
//! it was captured from.

use std::path::{Path, PathBuf};

use guhit_model::*;

use crate::files::{self, ImageKind};
use crate::store;

fn dir_of(project_dir: &Path) -> PathBuf {
    project_dir.join("renders")
}

fn index_path(project_dir: &Path) -> PathBuf {
    dir_of(project_dir).join("index.json")
}

fn image_path(project_dir: &Path, id: &str) -> Result<PathBuf, IpcError> {
    files::check_id(id, "render")?;
    Ok(dir_of(project_dir).join(format!("{id}.png")))
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
                r.image_path = dir_of(project_dir).join(format!("{}.png", r.id)).to_string_lossy().into_owned();
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

/// Tier 1: a deterministic capture of the live 3D view.
pub fn capture(project_dir: &Path, revision: u32, camera: Camera, png_data_url: &str) -> Result<RenderRecord, IpcError> {
    let png = files::decode_png_data_url(png_data_url)?;
    let id = defaults::new_id();
    let path = image_path(project_dir, &id)?;
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
    };
    let mut records = load_index(project_dir)?;
    records.push(record.clone());
    save_index(project_dir, &records)?;
    Ok(record)
}

pub fn data(project_dir: &Path, id: &str) -> Result<String, IpcError> {
    let path = image_path(project_dir, id)?;
    if !path.is_file() {
        return Err(IpcError::new("not_found", format!("render not found: {id}")));
    }
    files::read_data_url(&path, ImageKind::Png)
}

/// Drops the record and moves the image into `trash/`.
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
