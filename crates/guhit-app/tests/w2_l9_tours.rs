//! Lane L9, tours and video (DECISIONS D54): `MakeTour`, tour frames,
//! `tour_through_wall`, and videos streamed to Visuals (`video_*`).

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use guhit_app::AppService;
use guhit_model::*;
use serde_json::{json, Value};

fn app() -> (AppService, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    (AppService::new_sandboxed(dir.path().to_path_buf()), dir)
}

async fn call(app: &AppService, cmd: &str, args: Value) -> Value {
    app.handle(cmd, args).await.unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

async fn fail(app: &AppService, cmd: &str, args: Value) -> IpcError {
    match app.handle(cmd, args).await {
        Ok(v) => panic!("{cmd} should have failed, got {v}"),
        Err(e) => e,
    }
}

async fn bungalow(app: &AppService) -> DocState {
    serde_json::from_value(call(app, "hub_create", json!({ "name": "Bahay", "template": "sample-bungalow" })).await).unwrap()
}

/// A 1 x 1 PNG.
const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

fn poster() -> String {
    format!("data:image/png;base64,{PNG_1X1}")
}

/// The start of an MP4: a 24 byte ftyp box, then anything.
fn mp4_bytes(n: usize) -> Vec<u8> {
    let mut v = vec![0, 0, 0, 24];
    v.extend_from_slice(b"ftypiso5\0\0\x02\0iso5mp41");
    v.extend((0..n).map(|i| (i % 251) as u8));
    v
}

async fn make_tour(app: &AppService) -> (Tour, DocState) {
    let options = AutoTour { exterior: true, day_to_dusk: true, pace_mps: 1.0, levels: vec![] };
    let r: ApplyResult = serde_json::from_value(call(app, "doc_apply", json!({ "command": Command::MakeTour { name: None, options } })).await).unwrap();
    let tour = r
        .state
        .project
        .elements
        .iter()
        .find_map(|e| match e {
            Element::Tour(t) => Some(t.clone()),
            _ => None,
        })
        .expect("a tour");
    assert!(r.diff.summary.starts_with("Made tour \"Tour\""), "{}", r.diff.summary);
    assert!(!r.state.derived.issues.iter().any(|i| i.code == "tour_through_wall"));
    (tour, r.state)
}

fn info(tour: &Tour, codec: &str) -> VideoInfo {
    VideoInfo { path: String::new(), tour_id: tour.id.clone(), seconds: 30.0, fps: 30, width: 1920, height: 1080, codec: codec.into() }
}

#[tokio::test]
async fn make_tour_and_its_frames_reach_the_window() {
    let (app, _dir) = app();
    bungalow(&app).await;
    let (tour, _) = make_tour(&app).await;
    let f = call(&app, "doc_query", json!({ "query": { "type": "tour_frames", "tour_id": tour.id, "fps": null } })).await;
    let total: f64 = tour.shots.iter().map(|s| s.seconds).sum();
    assert_eq!(f["frame_count"].as_u64().unwrap(), (total * 30.0).round() as u64);
    assert_eq!(f["shots"].as_array().unwrap().len(), tour.shots.len());
    assert_eq!(f["frames"][0]["fx"]["kind"], "plan_lift");
    let e = fail(&app, "doc_query", json!({ "query": { "type": "tour_frames", "tour_id": "nope", "fps": null } })).await;
    assert_eq!(e.code, "not_found");
}

#[tokio::test]
async fn a_video_streams_in_chunks_and_lands_in_visuals() {
    let (app, dir) = app();
    let state = bungalow(&app).await;
    let (tour, after) = make_tour(&app).await;
    assert!(after.revision > state.revision);
    let id = call(&app, "video_begin", json!({ "tour_id": tour.id, "revision": after.revision })).await["id"].as_str().unwrap().to_string();
    let bytes = mp4_bytes(5_000_000);
    for chunk in bytes.chunks(2 * 1024 * 1024) {
        call(&app, "video_append", json!({ "id": id, "data": B64.encode(chunk) })).await;
    }
    let record: RenderRecord = serde_json::from_value(call(&app, "video_finish", json!({ "id": id, "poster": poster(), "info": info(&tour, "avc") })).await).unwrap();
    assert_eq!(record.id, id);
    assert_eq!(record.revision, after.revision);
    assert_eq!(record.info.as_ref().unwrap().kind, RenderKind::Video);
    let video = record.video.clone().expect("video info");
    assert_eq!(video.codec, "avc");
    assert!(video.path.ends_with(&format!("{id}.mp4")), "{}", video.path);
    assert_eq!(std::fs::read(&video.path).unwrap(), bytes, "the file is the chunks in order");
    assert!(std::path::Path::new(&record.image_path).is_file(), "the poster is saved");
    // The camera is the tour's first key.
    assert_eq!(record.camera.position, tour.shots[0].keys[0].position);
    // It is listed in Visuals, and nothing of the upload is left behind.
    let list: Vec<RenderRecord> = serde_json::from_value(call(&app, "render_list", json!({})).await).unwrap();
    assert_eq!(list[0].id, id);
    assert_eq!(list[0].video, record.video);
    let renders = std::path::Path::new(&video.path).parent().unwrap().to_path_buf();
    assert!(std::fs::read_dir(&renders).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().contains(".part")));
    assert!(renders.starts_with(dir.path()));
    // The upload is gone: appending again is refused.
    let e = fail(&app, "video_append", json!({ "id": id, "data": "AAAA" })).await;
    assert_eq!(e.code, "not_found");
}

#[tokio::test]
async fn the_png_fallback_saves_a_frame_folder() {
    let (app, _dir) = app();
    bungalow(&app).await;
    let (tour, after) = make_tour(&app).await;
    let id = call(&app, "video_begin", json!({ "tour_id": tour.id, "revision": after.revision })).await["id"].as_str().unwrap().to_string();
    let png = B64.decode(PNG_1X1).unwrap();
    // Three frames, the second split across two appends.
    call(&app, "video_append", json!({ "id": id, "data": B64.encode(&png) })).await;
    call(&app, "video_append", json!({ "id": id, "data": B64.encode(&png[..30]) })).await;
    call(&app, "video_append", json!({ "id": id, "data": B64.encode([&png[30..], &png[..]].concat()) })).await;
    let record: RenderRecord = serde_json::from_value(call(&app, "video_finish", json!({ "id": id, "poster": poster(), "info": info(&tour, "png") })).await).unwrap();
    let video = record.video.unwrap();
    assert_eq!(video.codec, "png");
    let folder = std::path::PathBuf::from(&video.path);
    assert!(folder.is_dir());
    let mut names: Vec<String> = std::fs::read_dir(&folder).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, ["frame-000001.png", "frame-000002.png", "frame-000003.png"]);
    assert_eq!(std::fs::read(folder.join("frame-000002.png")).unwrap(), png);
}

#[tokio::test]
async fn video_calls_refuse_what_does_not_fit() {
    let (app, _dir) = app();
    bungalow(&app).await;
    let (tour, after) = make_tour(&app).await;
    assert_eq!(fail(&app, "video_begin", json!({ "tour_id": "nope", "revision": 0 })).await.code, "not_found");
    assert_eq!(fail(&app, "video_begin", json!({ "tour_id": tour.id, "revision": after.revision + 5 })).await.code, "bad_args");
    let id = call(&app, "video_begin", json!({ "tour_id": tour.id, "revision": after.revision })).await["id"].as_str().unwrap().to_string();
    // More than 4 MiB in one call.
    let big = B64.encode(vec![0u8; 4 * 1024 * 1024 + 16]);
    assert_eq!(fail(&app, "video_append", json!({ "id": id, "data": big })).await.code, "bad_args");
    assert_eq!(fail(&app, "video_append", json!({ "id": id, "data": "not base64!" })).await.code, "bad_args");
    assert_eq!(fail(&app, "video_append", json!({ "id": "../x", "data": "AAAA" })).await.code, "invalid");
    call(&app, "video_append", json!({ "id": id, "data": B64.encode(b"not an mp4 at all") })).await;
    // A codec it does not know, another tour, a file that is not an MP4.
    let e = fail(&app, "video_finish", json!({ "id": id, "poster": poster(), "info": info(&tour, "h265") })).await;
    assert_eq!(e.code, "bad_args");
    let other = VideoInfo { tour_id: "other".into(), ..info(&tour, "avc") };
    assert_eq!(fail(&app, "video_finish", json!({ "id": id, "poster": poster(), "info": other })).await.code, "bad_args");
    assert_eq!(fail(&app, "video_finish", json!({ "id": id, "poster": poster(), "info": info(&tour, "avc") })).await.code, "invalid");
    // Abort drops it; aborting again is fine.
    call(&app, "video_abort", json!({ "id": id })).await;
    call(&app, "video_abort", json!({ "id": id })).await;
    assert_eq!(fail(&app, "video_append", json!({ "id": id, "data": "AAAA" })).await.code, "not_found");
    let list: Vec<RenderRecord> = serde_json::from_value(call(&app, "render_list", json!({})).await).unwrap();
    assert!(list.is_empty());
}

/// The entries of a `.guhit` bundle written by `bundle_save`.
async fn bundle_entries(app: &AppService, videos: Option<bool>) -> Vec<String> {
    let mut args = json!({ "path": null });
    if let Some(v) = videos {
        args["videos"] = json!(v);
    }
    let out: ExportResult = serde_json::from_value(call(app, "bundle_save", args).await).unwrap();
    let bytes = std::fs::read(&out.path).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().to_string()).collect()
}

#[tokio::test]
async fn a_bundle_leaves_videos_out_unless_asked() {
    let (app, _dir) = app();
    bungalow(&app).await;
    let (tour, after) = make_tour(&app).await;
    // One MP4 and one PNG frame folder, both finished.
    let mp4 = call(&app, "video_begin", json!({ "tour_id": tour.id, "revision": after.revision })).await["id"].as_str().unwrap().to_string();
    call(&app, "video_append", json!({ "id": mp4, "data": B64.encode(mp4_bytes(1000)) })).await;
    call(&app, "video_finish", json!({ "id": mp4, "poster": poster(), "info": info(&tour, "avc") })).await;
    let png = call(&app, "video_begin", json!({ "tour_id": tour.id, "revision": after.revision })).await["id"].as_str().unwrap().to_string();
    let frame = B64.decode(PNG_1X1).unwrap();
    call(&app, "video_append", json!({ "id": png, "data": B64.encode([&frame[..], &frame[..]].concat()) })).await;
    call(&app, "video_finish", json!({ "id": png, "poster": poster(), "info": info(&tour, "png") })).await;

    let has = |list: &[String], name: &str| list.iter().any(|e| e.ends_with(name));
    // By default: the records and posters go in, the videos stay out.
    for list in [bundle_entries(&app, None).await, bundle_entries(&app, Some(false)).await] {
        assert!(has(&list, "renders/index.json"), "{list:?}");
        assert!(has(&list, &format!("renders/{mp4}.png")), "{list:?}");
        assert!(has(&list, &format!("renders/{png}.png")), "{list:?}");
        assert!(!has(&list, &format!("renders/{mp4}.mp4")), "{list:?}");
        assert!(!list.iter().any(|e| e.contains(&format!("renders/{png}/"))), "{list:?}");
    }
    // Asked for: both go in.
    let list = bundle_entries(&app, Some(true)).await;
    assert!(has(&list, &format!("renders/{mp4}.mp4")), "{list:?}");
    assert!(has(&list, &format!("renders/{png}/frame-000001.png")), "{list:?}");
    assert!(has(&list, &format!("renders/{png}/frame-000002.png")), "{list:?}");
}

#[test]
fn bundles_leave_videos_out() {
    use guhit_app::video::is_video_file;
    assert!(is_video_file("renders/0b7c.mp4"));
    assert!(is_video_file("renders/0b7c/frame-000001.png"));
    assert!(is_video_file("renders/0b7c.part"));
    assert!(!is_video_file("renders/0b7c.png"));
    assert!(!is_video_file("renders/index.json"));
}
