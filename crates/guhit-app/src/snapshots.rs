//! Versions of a project: `snapshots/<id>.json`, each `{ meta, project }`.
//! Named versions are made by the user and kept until the project is trashed.
//! Automatic ones are made by the app and only the newest 20 are kept.

use std::path::{Path, PathBuf};

use guhit_model::*;
use serde::{Deserialize, Serialize};

use crate::files;

/// Automatic snapshots kept per project.
pub const KEEP_AUTO: usize = 20;
/// Take an automatic snapshot every this many revisions.
pub const AUTO_EVERY_REVISIONS: u32 = 25;
/// On open, take one when the newest automatic snapshot is older than this.
pub const AUTO_ON_OPEN_AFTER_SECS: i64 = 10 * 60;

const MAX_LABEL_CHARS: usize = 120;

#[derive(Serialize, Deserialize)]
pub struct SnapshotFile {
    pub meta: SnapshotMeta,
    pub project: Project,
}

/// Header-only view, so listing does not build every project in memory.
#[derive(Deserialize)]
struct SnapshotHeader {
    meta: SnapshotMeta,
}

fn dir_of(project_dir: &Path) -> PathBuf {
    project_dir.join("snapshots")
}

fn path_of(project_dir: &Path, id: &str) -> Result<PathBuf, IpcError> {
    files::check_id(id, "snapshot")?;
    Ok(dir_of(project_dir).join(format!("{id}.json")))
}

pub fn clean_label(raw: &str, fallback: &str) -> String {
    let label: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_LABEL_CHARS)
        .collect();
    if label.is_empty() {
        fallback.to_string()
    } else {
        label
    }
}

pub fn create(
    project_dir: &Path,
    project: &Project,
    revision: u32,
    label: &str,
    auto: bool,
) -> Result<SnapshotMeta, IpcError> {
    let meta = SnapshotMeta {
        id: defaults::new_id(),
        label: clean_label(label, if auto { "Automatic snapshot" } else { "Untitled version" }),
        created_at: defaults::now_rfc3339(),
        revision,
        auto,
    };
    let file = SnapshotFile {
        meta: meta.clone(),
        project: project.clone(),
    };
    files::write_json_atomic(&path_of(project_dir, &meta.id)?, &file)?;
    if auto {
        prune_autos(project_dir);
    }
    Ok(meta)
}

/// Newest first. Files that cannot be read are skipped.
pub fn list(project_dir: &Path) -> Vec<SnapshotMeta> {
    let mut found: Vec<(SnapshotMeta, std::time::SystemTime)> = vec![];
    let Ok(entries) = std::fs::read_dir(dir_of(project_dir)) else {
        return vec![];
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let header = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<SnapshotHeader>(&b).ok());
        let Some(header) = header else {
            eprintln!("guhit-app: skipping unreadable snapshot {}", path.display());
            continue;
        };
        // The file name is the id we can load again. Ignore a mismatch.
        if path.file_stem().and_then(|s| s.to_str()) != Some(header.meta.id.as_str()) {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        found.push((header.meta, modified));
    }
    // created_at has second precision, so file time breaks ties.
    found.sort_by(|a, b| b.0.created_at.cmp(&a.0.created_at).then_with(|| b.1.cmp(&a.1)));
    found.into_iter().map(|(m, _)| m).collect()
}

pub fn load(project_dir: &Path, id: &str) -> Result<SnapshotFile, IpcError> {
    let path = path_of(project_dir, id)?;
    let bytes = std::fs::read(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => IpcError::new("not_found", format!("snapshot not found: {id}")),
        _ => files::io_err("cannot read", &path, e),
    })?;
    let file: SnapshotFile = serde_json::from_slice(&bytes)
        .map_err(|e| IpcError::new("invalid", format!("snapshot file is damaged ({}): {e}", path.display())))?;
    if file.project.schema_version > SCHEMA_VERSION {
        return Err(IpcError::new(
            "invalid",
            "this snapshot was saved by a newer version of Guhit Studio",
        ));
    }
    Ok(file)
}

/// True when there is no automatic snapshot yet, or the newest one is older
/// than `AUTO_ON_OPEN_AFTER_SECS`.
pub fn auto_is_due(project_dir: &Path) -> bool {
    let newest = list(project_dir).into_iter().find(|m| m.auto);
    match newest.and_then(|m| files::parse_rfc3339_secs(&m.created_at)) {
        Some(t) => files::now_secs() - t > AUTO_ON_OPEN_AFTER_SECS,
        None => true,
    }
}

/// Retention for automatic snapshots: keep the newest `KEEP_AUTO`, remove the
/// rest. Named versions are never touched.
fn prune_autos(project_dir: &Path) {
    let autos: Vec<SnapshotMeta> = list(project_dir).into_iter().filter(|m| m.auto).collect();
    for old in autos.iter().skip(KEEP_AUTO) {
        if let Ok(path) = path_of(project_dir, &old.id) {
            let _ = std::fs::remove_file(path);
        }
    }
}
