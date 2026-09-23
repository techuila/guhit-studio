//! Project store. Layout is in docs/CONTRACT.md, "Storage layout":
//!
//! ```text
//! <data_dir>/projects/<project-id>/project.json, thumbnail.png, snapshots/, underlays/, renders/
//! <data_dir>/exports/
//! <data_dir>/trash/
//! ```
//!
//! Plain synchronous `std::fs`. Projects are small, and every caller already
//! holds the session lock, so file access is serialized.

use std::path::{Path, PathBuf};

use guhit_core::compute_derived;
use guhit_model::*;

use crate::files::{self, ImageKind};

pub const PROJECT_FILE: &str = "project.json";
pub const THUMBNAIL_FILE: &str = "thumbnail.png";

pub fn projects_dir(data: &Path) -> PathBuf {
    data.join("projects")
}

pub fn exports_dir(data: &Path) -> PathBuf {
    data.join("exports")
}

pub fn trash_dir(data: &Path) -> PathBuf {
    data.join("trash")
}

/// Folder of one project. The id is checked, so the result is always a
/// direct child of `projects/`.
pub fn project_dir(data: &Path, id: &str) -> Result<PathBuf, IpcError> {
    files::check_id(id, "project")?;
    Ok(projects_dir(data).join(id))
}

/// Like `project_dir`, and the project must exist.
pub fn existing_project_dir(data: &Path, id: &str) -> Result<PathBuf, IpcError> {
    let dir = project_dir(data, id)?;
    if dir.join(PROJECT_FILE).is_file() {
        Ok(dir)
    } else {
        Err(IpcError::new("not_found", format!("project not found: {id}")))
    }
}

/// Write `project.json` atomically. The folder is created when missing.
pub fn save_project(data: &Path, project: &Project) -> Result<(), IpcError> {
    let dir = project_dir(data, &project.id)?;
    files::write_json_atomic(&dir.join(PROJECT_FILE), project)
}

/// Parse a project file. Checks `schema_version` before the full parse, so a
/// file from a newer app gives a clear message instead of a field error.
///
/// Versions 1 and 2 are read. Version 2 added pipes and the four pipe layers;
/// a version 1 file has neither, parses as it is, and `guhit_core::migrate`
/// adds the missing layers, so the project always comes back at
/// `SCHEMA_VERSION`.
pub fn parse_project(bytes: &[u8], source: &Path) -> Result<Project, IpcError> {
    let corrupt =
        |e: serde_json::Error| IpcError::new("invalid", format!("project file is damaged ({}): {e}", source.display()));
    let raw: serde_json::Value = serde_json::from_slice(bytes).map_err(corrupt)?;
    let version = raw.get("schema_version").and_then(|v| v.as_u64());
    match version {
        None => {
            return Err(IpcError::new(
                "invalid",
                format!("project file has no schema_version ({})", source.display()),
            ))
        }
        Some(v) if v > SCHEMA_VERSION as u64 => {
            return Err(IpcError::new(
                "invalid",
                format!(
                    "this project was saved by a newer version of Guhit Studio (file schema {v}, this app reads up to {SCHEMA_VERSION}). Update the app to open it."
                ),
            ))
        }
        Some(0) => return Err(IpcError::new("invalid", "project file has schema_version 0, which never existed")),
        // 1 and 2: the same shape, migrated below.
        Some(_) => {}
    }
    let mut project: Project = serde_json::from_value(raw).map_err(corrupt)?;
    guhit_core::migrate(&mut project);
    Ok(project)
}

pub fn load_project(data: &Path, id: &str) -> Result<Project, IpcError> {
    let dir = existing_project_dir(data, id)?;
    let path = dir.join(PROJECT_FILE);
    let bytes = std::fs::read(&path).map_err(|e| files::io_err("cannot read", &path, e))?;
    let mut project = parse_project(&bytes, &path)?;
    // The folder name is the identity. A hand-copied folder keeps working.
    if project.id != id {
        project.id = id.to_string();
    }
    Ok(project)
}

pub fn thumbnail_data_url(dir: &Path) -> Option<String> {
    let path = dir.join(THUMBNAIL_FILE);
    if !path.is_file() {
        return None;
    }
    files::read_data_url(&path, ImageKind::Png).ok()
}

pub fn meta_for(data: &Path, project: &Project, derived: &Derived) -> ProjectMeta {
    let dir = projects_dir(data).join(&project.id);
    ProjectMeta {
        id: project.id.clone(),
        name: project.name.clone(),
        created_at: project.created_at.clone(),
        updated_at: project.updated_at.clone(),
        floor_area_m2: derived.totals.floor_area_m2,
        room_count: derived.totals.room_count,
        thumbnail: thumbnail_data_url(&dir),
    }
}

pub fn load_meta(data: &Path, id: &str) -> Result<ProjectMeta, IpcError> {
    let project = load_project(data, id)?;
    let derived = compute_derived(&project);
    Ok(meta_for(data, &project, &derived))
}

/// Every readable project, newest first. A folder that cannot be read or
/// parsed is skipped with a line on stderr; it never fails the whole list.
pub fn list_projects(data: &Path) -> Result<Vec<ProjectMeta>, IpcError> {
    let root = projects_dir(data);
    let entries = match std::fs::read_dir(&root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(files::io_err("cannot read", &root, e)),
    };
    let mut out = vec![];
    for entry in entries.flatten() {
        let Some(id) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if files::check_id(&id, "project").is_err() || !entry.path().join(PROJECT_FILE).is_file() {
            continue;
        }
        match load_meta(data, &id) {
            Ok(meta) => out.push(meta),
            Err(e) => eprintln!("guhit-app: skipping project {id}: {}", e.message),
        }
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

/// New project with its own id, named "X copy". Thumbnail and underlay images
/// come along, since underlay elements point at those files. Snapshots and
/// renders stay with the original: they are tied to its revision history.
pub fn duplicate_project(data: &Path, source: &Project) -> Result<Project, IpcError> {
    let src_dir = project_dir(data, &source.id)?;
    let mut copy = source.clone();
    copy.id = defaults::new_id();
    copy.name = format!("{} copy", source.name);
    let now = defaults::now_rfc3339();
    copy.created_at = now.clone();
    copy.updated_at = now;
    save_project(data, &copy)?;

    let dst_dir = project_dir(data, &copy.id)?;
    let thumb = src_dir.join(THUMBNAIL_FILE);
    if thumb.is_file() {
        let _ = std::fs::copy(&thumb, dst_dir.join(THUMBNAIL_FILE));
    }
    let underlays = src_dir.join("underlays");
    if let Ok(entries) = std::fs::read_dir(&underlays) {
        let dst = dst_dir.join("underlays");
        files::create_dir(&dst)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                std::fs::copy(&path, dst.join(entry.file_name()))
                    .map_err(|e| files::io_err("cannot copy", &path, e))?;
            }
        }
    }
    Ok(copy)
}

/// Move a project folder into `trash/`. Nothing is ever hard-deleted.
pub fn trash_project(data: &Path, id: &str) -> Result<PathBuf, IpcError> {
    let dir = project_dir(data, id)?;
    if !dir.is_dir() {
        return Err(IpcError::new("not_found", format!("project not found: {id}")));
    }
    let trash = trash_dir(data);
    files::create_dir(&trash)?;
    let target = files::unique_path(&trash, &format!("{id}-{}", files::file_timestamp()), "");
    std::fs::rename(&dir, &target).map_err(|e| files::io_err("cannot move to trash", &dir, e))?;
    Ok(target)
}

/// Move one file into `trash/`, keeping a recognizable name.
pub fn trash_file(data: &Path, file: &Path, prefix: &str) -> Result<(), IpcError> {
    if !file.is_file() {
        return Ok(());
    }
    let trash = trash_dir(data);
    files::create_dir(&trash)?;
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let (stem, ext) = files::split_ext(name);
    let target = files::unique_path(&trash, &format!("{prefix}-{stem}-{}", files::file_timestamp()), ext);
    std::fs::rename(file, &target).map_err(|e| files::io_err("cannot move to trash", file, e))
}
