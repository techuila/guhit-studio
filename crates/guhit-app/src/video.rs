//! Tour videos streamed from the window (DECISIONS D54): `video_begin`,
//! `video_append` (base64, at most 4 MiB a call), `video_finish` (the
//! poster and `VideoInfo`, answering the `RenderRecord` of
//! `renders/<id>.mp4`) and `video_abort`. LANE L9 owns this module;
//! `lib.rs` routes `OWNS` here.
//!
//! The window encodes; this side only writes bytes. An upload lives on disk,
//! not in memory, so a restart or a second window cannot lose track of it:
//! `renders/<id>.part` holds the bytes and `renders/<id>.part.json` what
//! `video_begin` was told. `video_finish` renames the bytes to
//! `renders/<id>.mp4` (codec "avc" or "vp9"), or splits them into a frame
//! folder `renders/<id>/frame-000001.png...` (codec "png", the last fallback:
//! the PNGs arrive one after another), writes the poster next to it as the
//! record's image and adds a `RenderRecord` with `video` set, kind `video`.
//! `.guhit` bundles leave the video files out unless asked
//! (`is_video_file`).

use std::io::Write;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use guhit_model::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::files;
use crate::{arg, renders, to_value, AppService, IpcResult};

/// The IPC commands this module answers.
pub const OWNS: &[&str] = &["video_begin", "video_append", "video_finish", "video_abort"];

/// The largest `video_append` chunk, decoded.
pub const MAX_CHUNK_BYTES: usize = 4 * 1024 * 1024;

/// The largest video, all chunks together. A 60 s 4K H.264 tour is about
/// 300 MB; a PNG frame folder of a long tour is the one that gets near this.
pub const MAX_VIDEO_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Codecs `VideoInfo::codec` may name.
pub const CODECS: [&str; 3] = ["avc", "vp9", "png"];

const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// What `video_begin` was told, kept beside the bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Upload {
    tour_id: Id,
    revision: u32,
    started_at: String,
}

fn renders_dir(project_dir: &Path) -> PathBuf {
    project_dir.join("renders")
}

fn part_path(project_dir: &Path, id: &str) -> PathBuf {
    renders_dir(project_dir).join(format!("{id}.part"))
}

fn meta_path(project_dir: &Path, id: &str) -> PathBuf {
    renders_dir(project_dir).join(format!("{id}.part.json"))
}

/// The finished file of a video: `renders/<id>.mp4`, or the frame folder.
pub fn video_path(project_dir: &Path, id: &str, codec: &str) -> PathBuf {
    if codec == "png" {
        renders_dir(project_dir).join(id)
    } else {
        renders_dir(project_dir).join(format!("{id}.mp4"))
    }
}

/// True for a file of a project folder (path relative to it, `/` between
/// parts) that is a tour video or one of its frames, or an upload in
/// progress: what a `.guhit` bundle leaves out unless videos are asked for.
pub fn is_video_file(rel: &str) -> bool {
    let Some(rest) = rel.strip_prefix("renders/") else { return false };
    if rest.ends_with(".mp4") || rest.ends_with(".part") || rest.ends_with(".part.json") {
        return true;
    }
    // A frame folder: renders/<id>/frame-000001.png.
    matches!(rest.split_once('/'), Some((_, f)) if f.starts_with("frame-") && f.ends_with(".png"))
}

fn not_found(id: &str) -> IpcError {
    IpcError::new("not_found", format!("No video upload has id {id}: it finished, was dropped, or never began."))
}

fn read_upload(project_dir: &Path, id: &str) -> Result<Upload, IpcError> {
    files::check_id(id, "video")?;
    let path = meta_path(project_dir, id);
    let bytes = std::fs::read(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => not_found(id),
        _ => files::io_err("cannot read", &path, e),
    })?;
    serde_json::from_slice(&bytes).map_err(|e| IpcError::new("io", format!("the video upload {id} is damaged: {e}")))
}

fn remove_upload(project_dir: &Path, id: &str) {
    let _ = std::fs::remove_file(part_path(project_dir, id));
    let _ = std::fs::remove_file(meta_path(project_dir, id));
}

/// Uploads older than this were left by a window that closed or crashed.
const STALE_SECS: i64 = 24 * 60 * 60;

/// Removes uploads that began more than a day ago and never finished.
fn drop_stale(project_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(renders_dir(project_dir)) else { return };
    let now = files::now_secs();
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".part.json") else { continue };
        let old = read_upload(project_dir, id)
            .ok()
            .and_then(|u| files::parse_rfc3339_secs(&u.started_at))
            .map(|t| now - t > STALE_SECS)
            .unwrap_or(true);
        if old {
            remove_upload(project_dir, id);
        }
    }
}

/// `video_begin`: an empty upload for a tour at a revision. The tour must be
/// in the open document; the revision no newer than it.
fn begin(project: &Project, current: u32, project_dir: &Path, tour_id: &Id, revision: u32) -> Result<Id, IpcError> {
    if !project.elements.iter().any(|e| matches!(e, Element::Tour(t) if &t.id == tour_id)) {
        return Err(IpcError::new("not_found", format!("No tour has id {tour_id}.")));
    }
    if revision > current {
        return Err(IpcError::new(
            "bad_args",
            format!("argument `revision`: {revision} is newer than the document, which is at {current}"),
        ));
    }
    let id = defaults::new_id();
    files::create_dir(&renders_dir(project_dir))?;
    drop_stale(project_dir);
    let part = part_path(project_dir, &id);
    std::fs::File::create(&part).map_err(|e| files::io_err("cannot create", &part, e))?;
    let upload = Upload { tour_id: tour_id.clone(), revision, started_at: defaults::now_rfc3339() };
    files::write_json_atomic(&meta_path(project_dir, &id), &upload)?;
    Ok(id)
}

/// `video_append`: the next bytes, base64, at most MAX_CHUNK_BYTES decoded.
fn append(project_dir: &Path, id: &str, data: &str) -> Result<(), IpcError> {
    read_upload(project_dir, id)?;
    // 4 base64 characters carry 3 bytes: refuse before decoding.
    if data.len() / 4 * 3 > MAX_CHUNK_BYTES + 3 {
        return Err(IpcError::new("bad_args", "argument `data`: a chunk holds at most 4 MiB"));
    }
    let bytes = B64
        .decode(data.trim().as_bytes())
        .map_err(|e| IpcError::new("bad_args", format!("argument `data`: base64: {e}")))?;
    if bytes.len() > MAX_CHUNK_BYTES {
        return Err(IpcError::new("bad_args", "argument `data`: a chunk holds at most 4 MiB"));
    }
    let part = part_path(project_dir, id);
    let size = std::fs::metadata(&part).map(|m| m.len()).map_err(|_| not_found(id))?;
    if size + bytes.len() as u64 > MAX_VIDEO_BYTES {
        remove_upload(project_dir, id);
        return Err(IpcError::new("invalid", "The video is larger than 8 GB. Export a shorter tour or a smaller size."));
    }
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&part)
        .map_err(|e| files::io_err("cannot open", &part, e))?;
    f.write_all(&bytes).map_err(|e| files::io_err("cannot write", &part, e))?;
    Ok(())
}

/// PNG files one after another, split at each IEND chunk.
pub fn split_pngs(bytes: &[u8]) -> Result<Vec<&[u8]>, IpcError> {
    let bad = |why: &str| IpcError::new("invalid", format!("The frames are not PNG files one after another: {why}."));
    let mut out = vec![];
    let mut at = 0;
    while at < bytes.len() {
        let start = at;
        if bytes.len() < at + 8 || bytes[at..at + 8] != PNG_MAGIC {
            return Err(bad(&format!("frame {} has no PNG signature", out.len() + 1)));
        }
        at += 8;
        loop {
            if bytes.len() < at + 12 {
                return Err(bad(&format!("frame {} is cut short", out.len() + 1)));
            }
            let len = u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize;
            let kind = &bytes[at + 4..at + 8];
            let end = at.checked_add(12 + len).filter(|&e| e <= bytes.len()).ok_or_else(|| bad(&format!("frame {} is cut short", out.len() + 1)))?;
            at = end;
            if kind == b"IEND" {
                break;
            }
        }
        out.push(&bytes[start..at]);
    }
    if out.is_empty() {
        return Err(bad("there are none"));
    }
    Ok(out)
}

/// The camera a video record shows: its tour's first key, or a plain view.
fn poster_camera(project: &Project, tour_id: &Id) -> Camera {
    let first = project.elements.iter().find_map(|e| match e {
        Element::Tour(t) if &t.id == tour_id => Some((t.name.clone(), t.shots.first().and_then(|s| s.keys.first().copied()))),
        _ => None,
    });
    let (name, key) = first.unwrap_or_else(|| ("Tour".to_string(), None));
    let key = key.unwrap_or(ShotKey {
        at: 0.0,
        position: Vec3 { x: -5000.0, y: -7000.0, z: 4500.0 },
        target: Vec3 { x: 0.0, y: 0.0, z: 1200.0 },
        fov_deg: 50.0,
    });
    Camera {
        id: String::new(),
        name,
        preset: CameraPreset::Custom,
        position: key.position,
        target: key.target,
        fov_deg: key.fov_deg,
        light: None,
    }
}

fn check_info(info: &VideoInfo, upload: &Upload) -> Result<(), IpcError> {
    let bad = |why: String| IpcError::new("bad_args", format!("argument `info`: {why}"));
    if info.tour_id != upload.tour_id {
        return Err(bad(format!("the upload is for tour {}, not {}", upload.tour_id, info.tour_id)));
    }
    if !CODECS.contains(&info.codec.as_str()) {
        return Err(bad(format!("codec is avc, vp9 or png, not \"{}\"", info.codec)));
    }
    if !(1..=120).contains(&info.fps) {
        return Err(bad("fps is 1 to 120".into()));
    }
    if info.width == 0 || info.height == 0 || info.width > 8192 || info.height > 8192 {
        return Err(bad("width and height are 1 to 8192 pixels".into()));
    }
    if !info.seconds.is_finite() || info.seconds <= 0.0 {
        return Err(bad("seconds must be more than 0".into()));
    }
    Ok(())
}

/// `video_finish`: the file in place, the poster as its image, the record in
/// the index (`renders::push`, under the session lock like every renders
/// write).
fn finish(project: &Project, project_dir: &Path, id: &str, poster: &str, info: VideoInfo) -> Result<RenderRecord, IpcError> {
    let upload = read_upload(project_dir, id)?;
    check_info(&info, &upload)?;
    let (kind, poster_bytes) = files::decode_image_data_url(poster)?;
    let part = part_path(project_dir, id);
    let target = video_path(project_dir, id, &info.codec);
    if info.codec == "png" {
        let bytes = std::fs::read(&part).map_err(|e| files::io_err("cannot read", &part, e))?;
        let frames = split_pngs(&bytes)?;
        files::create_dir(&target)?;
        for (i, f) in frames.iter().enumerate() {
            let path = target.join(format!("frame-{:06}.png", i + 1));
            std::fs::write(&path, f).map_err(|e| files::io_err("cannot write", &path, e))?;
        }
        let _ = std::fs::remove_file(&part);
    } else {
        let mut head = [0u8; 12];
        let ok = std::fs::File::open(&part)
            .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head))
            .map(|_| &head[4..8] == b"ftyp")
            .unwrap_or(false);
        if !ok {
            return Err(IpcError::new("invalid", "The video is not an MP4 file: it has no ftyp box at the start."));
        }
        std::fs::rename(&part, &target).map_err(|e| files::io_err("cannot move", &part, e))?;
    }
    let image = renders_dir(project_dir).join(format!("{id}.{}", kind.ext()));
    files::write_atomic(&image, &poster_bytes)?;
    let _ = std::fs::remove_file(meta_path(project_dir, id));
    let video = VideoInfo { path: target.to_string_lossy().into_owned(), ..info };
    let record = RenderRecord {
        id: id.to_string(),
        created_at: defaults::now_rfc3339(),
        source: RenderSource::ModelView,
        revision: upload.revision,
        camera: poster_camera(project, &upload.tour_id),
        style_key: None,
        prompt: String::new(),
        image_path: image.to_string_lossy().into_owned(),
        source_render_id: None,
        provider: None,
        info: Some(RenderInfo {
            kind: RenderKind::Video,
            width: video.width,
            height: video.height,
            samples: 0,
            seconds: 0.0,
            quality: None,
            gpu: String::new(),
            visibility: None,
        }),
        maps: vec![],
        look: false,
        drift: None,
        video: Some(video),
    };
    // The caller holds the session lock, as for every write to the index.
    renders::push(project_dir, &record)?;
    Ok(record)
}

/// `video_abort`: drops an unfinished upload. Unknown ids are fine: the
/// window aborts on any failure, including one where nothing began.
fn abort(project_dir: &Path, id: &str) -> Result<(), IpcError> {
    files::check_id(id, "video")?;
    remove_upload(project_dir, id);
    Ok(())
}

pub async fn handle(app: &AppService, cmd: &str, args: Value) -> IpcResult {
    match cmd {
        "video_begin" => {
            let tour_id: Id = arg(&args, "tour_id")?;
            let revision: u32 = arg(&args, "revision")?;
            let s = app.session.lock().await;
            let doc = s.open_doc()?;
            let id = begin(doc.project(), doc.revision(), &s.open_project_dir()?, &tour_id, revision)?;
            Ok(serde_json::json!({ "id": id }))
        }
        "video_append" => {
            let id: Id = arg(&args, "id")?;
            let data: String = arg(&args, "data")?;
            // The bytes go to disk outside the session lock: an export must
            // not hold up edits. The upload's own file is the only one touched.
            let dir = app.session.lock().await.open_project_dir()?;
            append(&dir, &id, &data)?;
            Ok(Value::Null)
        }
        "video_finish" => {
            let id: Id = arg(&args, "id")?;
            let poster: String = arg(&args, "poster")?;
            let info: VideoInfo = arg(&args, "info")?;
            let s = app.session.lock().await;
            let project = s.open_doc()?.project().clone();
            to_value(&finish(&project, &s.open_project_dir()?, &id, &poster, info)?)
        }
        "video_abort" => {
            let id: Id = arg(&args, "id")?;
            let dir = app.session.lock().await.open_project_dir()?;
            abort(&dir, &id)?;
            Ok(Value::Null)
        }
        _ => Err(IpcError::new("unknown_command", format!("unknown command `{cmd}`"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(n: u8) -> Vec<u8> {
        let mut v = PNG_MAGIC.to_vec();
        // IHDR-like chunk with n bytes of data, then IEND.
        v.extend_from_slice(&(n as u32).to_be_bytes());
        v.extend_from_slice(b"IHDR");
        v.extend(std::iter::repeat_n(n, n as usize));
        v.extend_from_slice(&[0, 0, 0, 0]);
        v.extend_from_slice(&0u32.to_be_bytes());
        v.extend_from_slice(b"IEND");
        v.extend_from_slice(&[0xae, 0x42, 0x60, 0x82]);
        v
    }

    #[test]
    fn png_frames_split_at_each_end() {
        let mut all = png(3);
        all.extend(png(7));
        all.extend(png(1));
        let frames = split_pngs(&all).unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[1], png(7).as_slice());
        let mut cut = png(3);
        cut.extend(&png(7)[..20]);
        assert!(split_pngs(&cut).is_err());
        assert!(split_pngs(b"not a png").is_err());
    }

    #[test]
    fn bundles_leave_videos_out() {
        assert!(is_video_file("renders/abc.mp4"));
        assert!(is_video_file("renders/abc.part"));
        assert!(is_video_file("renders/abc.part.json"));
        assert!(is_video_file("renders/abc/frame-000001.png"));
        assert!(!is_video_file("renders/abc.png"));
        assert!(!is_video_file("renders/index.json"));
        assert!(!is_video_file("snapshots/abc.mp4"));
    }
}
