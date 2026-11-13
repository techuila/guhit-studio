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
//!
//! AI render v2 (lane L6, DECISIONS D53) keeps a few files beside a record:
//! - `<id>.<map>.png`: a model view's conditioning maps (`RenderMap`), drawn
//!   by the window from the record's camera (`render/conditioning.ts`).
//! - `<id>.maps.json`: `MapsMeta`, the size the maps were drawn at, what
//!   they show (roof, cutaway) and the colors of the ids map.
//! - `<id>.drift.png`: for an AI visualization, the model edges the drift
//!   check found no match for, red on transparent.
//!
//! One record at most is the project look (`RenderRecord::look`), the style
//! reference of later AI renders.

use std::path::{Path, PathBuf};

use guhit_model::*;

use crate::files::{self, ImageKind};
use crate::store;

/// The maps metadata types live in the contract (guhit-model).
pub use guhit_model::{LegendEntry, MapsMeta};

/// Largest conditioning map accepted, bytes.
pub const MAX_MAP_BYTES: usize = 24 * 1024 * 1024;

/// File name part of a map kind.
pub fn map_key(kind: MapKind) -> &'static str {
    match kind {
        MapKind::Depth => "depth",
        MapKind::Normal => "normal",
        MapKind::Lines => "lines",
        MapKind::Ids => "ids",
        MapKind::Clay => "clay",
    }
}

/// Every map kind, in the order records list them.
pub const MAP_KINDS: [MapKind; 5] = [MapKind::Lines, MapKind::Depth, MapKind::Normal, MapKind::Ids, MapKind::Clay];

fn map_path(project_dir: &Path, id: &str, kind: MapKind) -> Result<PathBuf, IpcError> {
    files::check_id(id, "render")?;
    Ok(dir_of(project_dir).join(format!("{id}.{}.png", map_key(kind))))
}

fn meta_path(project_dir: &Path, id: &str) -> Result<PathBuf, IpcError> {
    files::check_id(id, "render")?;
    Ok(dir_of(project_dir).join(format!("{id}.maps.json")))
}

fn overlay_path(project_dir: &Path, id: &str) -> Result<PathBuf, IpcError> {
    files::check_id(id, "render")?;
    Ok(dir_of(project_dir).join(format!("{id}.drift.png")))
}

/// Every file kept beside a record's image.
fn side_files(project_dir: &Path, id: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = MAP_KINDS.iter().filter_map(|k| map_path(project_dir, id, *k).ok()).collect();
    out.extend(meta_path(project_dir, id).ok());
    out.extend(overlay_path(project_dir, id).ok());
    out.into_iter().filter(|p| p.is_file()).collect()
}

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
                // A tour video's file or frame folder, the same way.
                if let Some(v) = r.video.as_mut() {
                    v.path = crate::video::video_path(project_dir, &r.id, &v.codec).to_string_lossy().into_owned();
                }
                // Maps the same way; a map whose file is gone is dropped.
                let kinds: Vec<MapKind> = r.maps.iter().map(|m| m.kind).collect();
                r.maps = kinds
                    .into_iter()
                    .filter_map(|kind| {
                        let path = map_path(project_dir, &r.id, kind).ok()?;
                        path.is_file().then(|| RenderMap { kind, path: path.to_string_lossy().into_owned() })
                    })
                    .collect();
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

/// Append one record to the index, replacing one with the same id: a tour
/// video (`video.rs`) whose file is already in place. The caller holds the
/// session lock, as for every write to the index.
pub fn push(project_dir: &Path, record: &RenderRecord) -> Result<(), IpcError> {
    files::check_id(&record.id, "render")?;
    let mut records = load_index(project_dir)?;
    records.retain(|r| r.id != record.id);
    records.push(record.clone());
    save_index(project_dir, &records)
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
        maps: vec![],
        look: false,
        drift: None,
        video: None,
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
    save_ai_checked(project_dir, source, kind, bytes, style_key, prompt, provider, None, None)
}

/// `save_ai` with the drift check's report and, when it found edges with no
/// match, its overlay (`<id>.drift.png`).
#[allow(clippy::too_many_arguments)]
pub fn save_ai_checked(
    project_dir: &Path,
    source: &RenderRecord,
    kind: ImageKind,
    bytes: &[u8],
    style_key: Option<String>,
    prompt: String,
    provider: &str,
    drift: Option<DriftReport>,
    overlay: Option<&[u8]>,
) -> Result<RenderRecord, IpcError> {
    if bytes.is_empty() {
        return Err(IpcError::new("ai_failed", "the image model returned an empty image"));
    }
    let id = defaults::new_id();
    let path = image_path_with(project_dir, &id, kind)?;
    files::write_atomic(&path, bytes)?;
    if let Some(png) = overlay {
        // Best effort: the record is what matters, the overlay is a picture of it.
        let _ = files::write_atomic(&overlay_path(project_dir, &id)?, png);
    }
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
        maps: vec![],
        look: false,
        drift,
        video: None,
    };
    let mut records = load_index(project_dir)?;
    records.push(record.clone());
    // The index is the record of truth. If it cannot be written, the orphan
    // image goes too, so a retry does not leave files nobody can see.
    if let Err(e) = save_index(project_dir, &records) {
        let _ = std::fs::remove_file(&path);
        if let Ok(o) = overlay_path(project_dir, &record.id) {
            let _ = std::fs::remove_file(o);
        }
        return Err(e);
    }
    Ok(record)
}

/// `render_set_look`: make one saved image the project look, the style
/// reference of later AI renders, or clear it (None). Usually an AI
/// visualization the architect approved; a model view works too. A tour
/// video cannot be one. Answers every record, newest first.
pub fn set_look(project_dir: &Path, id: Option<&str>) -> Result<Vec<RenderRecord>, IpcError> {
    let mut records = load_index(project_dir)?;
    if let Some(id) = id {
        files::check_id(id, "render")?;
        let record = records
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(|| IpcError::new("not_found", format!("render not found: {id}")))?;
        if record.video.is_some() || record.info.as_ref().is_some_and(|i| i.kind == RenderKind::Video) {
            return Err(IpcError::new("invalid", "A tour video cannot be the project look. Pick a still image."));
        }
    }
    for r in &mut records {
        r.look = Some(r.id.as_str()) == id;
    }
    save_index(project_dir, &records)?;
    records.reverse();
    Ok(records)
}

/// The project look, if one is set.
pub fn look(project_dir: &Path) -> Result<Option<RenderRecord>, IpcError> {
    Ok(load_index(project_dir)?.into_iter().find(|r| r.look))
}

/// Save a model view's conditioning maps, drawn by the window from its
/// camera, and attach them to its record. A map already there is replaced.
/// Only a model view has maps; an AI image never does.
pub fn save_maps(
    project_dir: &Path,
    id: &str,
    maps: &[(MapKind, Vec<u8>)],
    meta: Option<&MapsMeta>,
) -> Result<RenderRecord, IpcError> {
    files::check_id(id, "render")?;
    let mut records = load_index(project_dir)?;
    let at = records
        .iter()
        .position(|r| r.id == id)
        .ok_or_else(|| IpcError::new("not_found", format!("render not found: {id}")))?;
    if records[at].source != RenderSource::ModelView || records[at].video.is_some() {
        return Err(IpcError::new("invalid", "Only a model view has conditioning maps."));
    }
    for (kind, png) in maps {
        if ImageKind::sniff(png) != Some(ImageKind::Png) {
            return Err(IpcError::new("bad_args", format!("argument `maps`: the {} map is not a PNG", map_key(*kind))));
        }
        if png.len() > MAX_MAP_BYTES {
            return Err(IpcError::new("bad_args", format!("argument `maps`: the {} map is too large", map_key(*kind))));
        }
    }
    for (kind, png) in maps {
        files::write_atomic(&map_path(project_dir, id, *kind)?, png)?;
    }
    if let Some(meta) = meta {
        files::write_json_atomic(&meta_path(project_dir, id)?, meta)?;
    }
    let record = &mut records[at];
    for (kind, _) in maps {
        if !record.maps.iter().any(|m| m.kind == *kind) {
            record.maps.push(RenderMap { kind: *kind, path: String::new() });
        }
    }
    record.maps.sort_by_key(|m| MAP_KINDS.iter().position(|k| *k == m.kind));
    for m in &mut record.maps {
        m.path = map_path(project_dir, id, m.kind)?.to_string_lossy().into_owned();
    }
    let out = record.clone();
    save_index(project_dir, &records)?;
    Ok(out)
}

/// A record's map, if it has one.
pub fn read_map(project_dir: &Path, id: &str, kind: MapKind) -> Result<Option<Vec<u8>>, IpcError> {
    let path = map_path(project_dir, id, kind)?;
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(files::io_err("cannot read", &path, e)),
    }
}

/// A record's maps metadata, if it has any.
pub fn read_maps_meta(project_dir: &Path, id: &str) -> Option<MapsMeta> {
    let path = meta_path(project_dir, id).ok()?;
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// An AI visualization's drift overlay as a data URL, if it has one.
pub fn drift_overlay(project_dir: &Path, id: &str) -> Result<Option<String>, IpcError> {
    let path = overlay_path(project_dir, id)?;
    if !path.is_file() {
        return Ok(None);
    }
    files::read_data_url(&path, ImageKind::Png).map(Some)
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
    let video = records.iter().find(|r| r.id == id).and_then(|r| r.video.clone());
    records.retain(|r| r.id != id);
    if records.len() == before && !path.is_file() {
        return Err(IpcError::new("not_found", format!("render not found: {id}")));
    }
    save_index(project_dir, &records)?;
    let prefix = format!("{project_id}-render");
    // Maps, their metadata and a drift overlay go with the image.
    for side in side_files(project_dir, id) {
        let _ = store::trash_file(data_dir, &side, &prefix);
    }
    // A tour video's file, or its folder of frames, goes with its poster.
    if let Some(v) = video {
        let file = crate::video::video_path(project_dir, id, &v.codec);
        if file.is_dir() {
            trash_folder(data_dir, &file, &prefix)?;
        } else {
            store::trash_file(data_dir, &file, &prefix)?;
        }
    }
    store::trash_file(data_dir, &path, &prefix)
}

/// Move a folder (a tour's frames) into `trash/`. Nothing is hard-deleted.
fn trash_folder(data_dir: &Path, dir: &Path, prefix: &str) -> Result<(), IpcError> {
    let trash = store::trash_dir(data_dir);
    files::create_dir(&trash)?;
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("frames");
    let target = files::unique_path(&trash, &format!("{prefix}-{name}-{}", files::file_timestamp()), "");
    std::fs::rename(dir, &target).map_err(|e| files::io_err("cannot move to trash", dir, e))
}
