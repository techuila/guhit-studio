//! Tier 2 tests. A `FakeProvider` stands in for Gemini; everything else is
//! real: `AppService`, a temp data dir, the renders folder, the index, the
//! log file. No test ever needs an API key or touches the network.

use std::sync::Arc;

use guhit_model::*;
use serde_json::{json, Value};

use super::keys::MemoryKeyStore;
use super::provider::{png_of_size, tiny_jpeg, tiny_png, FakeProvider, ImageProvider};
use super::RenderAiState;
use crate::ai::log;
use crate::files;
use crate::AppService;

/// Never a real key. The tests assert that it shows up nowhere.
const FAKE_KEY: &str = "AIzaTEST-THIS-MUST-NEVER-LEAK-0123456789";

struct Rig {
    app: AppService,
    provider: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

async fn rig() -> Rig {
    rig_with(FakeProvider::new(), true).await
}

async fn rig_with(provider: FakeProvider, with_key: bool) -> Rig {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut app = AppService::new(dir.path().to_path_buf());
    let provider = Arc::new(provider);
    let keys: Box<dyn super::RenderKeyStore> = if with_key {
        Box::new(MemoryKeyStore::with_key(FAKE_KEY))
    } else {
        Box::new(MemoryKeyStore::default())
    };
    app.render_ai = Arc::new(RenderAiState::with_parts(provider.clone() as Arc<dyn ImageProvider>, keys));
    app.handle("hub_create", json!({"name": "Render test", "settings": null, "template": "sample-bungalow"}))
        .await
        .expect("project opens");
    Rig { app, provider, _dir: dir }
}

fn camera() -> Camera {
    Camera {
        id: "cam-test".to_string(),
        name: "Exterior corner".to_string(),
        preset: CameraPreset::ExteriorCorner,
        position: Vec3 { x: 12000.0, y: -9000.0, z: 6000.0 },
        target: Vec3 { x: 0.0, y: 0.0, z: 1200.0 },
        fov_deg: 45.0,
        light: None,
    }
}

impl Rig {
    /// Save a Tier 1 capture the way `Viewer3D` does.
    async fn capture(&self, png: &[u8]) -> RenderRecord {
        let url = files::encode_data_url(files::ImageKind::Png, png);
        let v = self
            .app
            .handle("render_capture", json!({"camera": camera(), "png": url}))
            .await
            .expect("capture");
        serde_json::from_value(v).expect("RenderRecord")
    }

    async fn generate(&self, request: Value) -> Result<RenderAiResult, IpcError> {
        let v = self.app.handle("render_ai_generate", json!({"request": request})).await?;
        Ok(serde_json::from_value(v).expect("RenderAiResult"))
    }

    async fn list(&self) -> Vec<RenderRecord> {
        let v = self.app.handle("render_list", json!({})).await.expect("render_list");
        serde_json::from_value(v).expect("RenderRecord[]")
    }

    async fn settings(&self, args: Value) -> Result<RenderAiSettings, IpcError> {
        let cmd = if args.as_object().map(|o| o.is_empty()).unwrap_or(true) {
            "render_ai_settings_get"
        } else {
            "render_ai_settings_set"
        };
        let v = self.app.handle(cmd, args).await?;
        Ok(serde_json::from_value(v).expect("RenderAiSettings"))
    }

    async fn log_lines(&self) -> Vec<Value> {
        let dir = self.app.project_dir().await.expect("project dir");
        let text = std::fs::read_to_string(dir.join(log::LOG_FILE)).unwrap_or_default();
        text.lines().map(|l| serde_json::from_str(l).expect("log line is JSON")).collect()
    }

    async fn renders_dir(&self) -> std::path::PathBuf {
        self.app.project_dir().await.expect("project dir").join("renders")
    }
}

fn request(source: &str) -> Value {
    json!({
        "source_render_id": source,
        "style_key": "tropical-modern",
        "prompt": "late afternoon, red clay roof tiles",
        "quality": "draft",
        "keep_geometry": true,
    })
}

// ------------------------------------------------------------------- generate

#[tokio::test]
async fn generate_links_the_visualization_to_its_source() {
    let rig = rig().await;
    let source = rig.capture(&png_of_size(1920, 1080)).await;

    let result = rig.generate(request(&source.id)).await.expect("generates");
    let record = &result.record;

    assert_eq!(record.source, RenderSource::AiVisualization);
    assert_eq!(record.source_render_id.as_deref(), Some(source.id.as_str()));
    assert_eq!(record.revision, source.revision, "same revision as the capture");
    assert_eq!(record.camera, source.camera, "same camera as the capture");
    assert_eq!(record.style_key.as_deref(), Some("tropical-modern"));
    assert_eq!(record.provider.as_deref(), Some("gemini/fake-image-model"));
    assert!(result.seconds > 0.0);

    // The record stores the full text that was sent, so an architect can read
    // back exactly what produced the image.
    assert_eq!(record.prompt, rig.provider.last_prompt());
    assert!(record.prompt.starts_with(super::prompt::PREAMBLE));
    assert!(record.prompt.contains("red clay roof tiles"));
    assert!(record.prompt.ends_with(super::prompt::GEOMETRY_LINE));

    // The aspect ratio comes from the capture, not from a default.
    assert_eq!(rig.provider.last_aspect_ratio(), "16:9");

    // The file is on disk next to the capture.
    let path = std::path::PathBuf::from(&record.image_path);
    assert!(path.is_file(), "{} exists", path.display());
    assert_eq!(path.parent(), Some(rig.renders_dir().await.as_path()));
    assert_eq!(path.extension().and_then(|e| e.to_str()), Some("png"));
}

#[tokio::test]
async fn a_missing_source_is_rejected_and_nothing_is_written() {
    let rig = rig().await;
    let e = rig
        .generate(request("11111111-2222-3333-4444-555555555555"))
        .await
        .expect_err("no such capture");
    assert_eq!(e.code, "invalid");
    assert_eq!(rig.provider.calls(), 0, "the provider is never called");
    assert!(rig.list().await.is_empty());
}

#[tokio::test]
async fn a_visualization_cannot_be_the_source_of_another_one() {
    let rig = rig().await;
    let source = rig.capture(&tiny_png()).await;
    let first = rig.generate(request(&source.id)).await.expect("generates").record;

    let e = rig.generate(request(&first.id)).await.expect_err("not a model view");
    assert_eq!(e.code, "invalid");
    assert!(e.message.contains("model view"), "message explains why: {}", e.message);
    assert_eq!(rig.provider.calls(), 1, "the second request never reached the provider");
    assert_eq!(rig.list().await.len(), 2, "still one capture and one visualization");
}

#[tokio::test]
async fn without_a_key_generate_says_it_is_not_configured() {
    // No injected provider, no key: the real path, stopped before any network.
    let dir = tempfile::tempdir().expect("temp dir");
    let mut app = AppService::new(dir.path().to_path_buf());
    app.render_ai = Arc::new(RenderAiState::with_keys(Box::new(MemoryKeyStore::default())));
    app.handle("hub_create", json!({"name": "No key", "settings": null, "template": "blank"}))
        .await
        .expect("project opens");
    let url = files::encode_data_url(files::ImageKind::Png, &tiny_png());
    let v = app
        .handle("render_capture", json!({"camera": camera(), "png": url}))
        .await
        .expect("capture");
    let source: RenderRecord = serde_json::from_value(v).expect("RenderRecord");

    let e = app
        .handle("render_ai_generate", json!({"request": request(&source.id)}))
        .await
        .expect_err("no key");
    assert_eq!(e.code, "ai_not_configured");
    assert!(e.message.contains("Google AI API key"), "message: {}", e.message);

    let v = app.handle("render_list", json!({})).await.expect("render_list");
    let records: Vec<RenderRecord> = serde_json::from_value(v).expect("RenderRecord[]");
    assert_eq!(records.len(), 1, "only the capture");
}

#[tokio::test]
async fn a_provider_failure_writes_no_record_and_no_file() {
    let rig = rig_with(FakeProvider::failing("Google AI is having trouble right now."), true).await;
    let source = rig.capture(&tiny_png()).await;
    let before: Vec<_> = std::fs::read_dir(rig.renders_dir().await)
        .expect("renders dir")
        .filter_map(|e| e.ok().map(|e| e.file_name()))
        .collect();

    let e = rig.generate(request(&source.id)).await.expect_err("provider failed");
    assert_eq!(e.code, "ai_failed");
    assert!(e.message.contains("trouble right now"), "the provider's words survive: {}", e.message);

    assert_eq!(rig.list().await.len(), 1, "only the capture is in the index");
    let after: Vec<_> = std::fs::read_dir(rig.renders_dir().await)
        .expect("renders dir")
        .filter_map(|e| e.ok().map(|e| e.file_name()))
        .collect();
    assert_eq!(before.len(), after.len(), "no stray image was left behind");

    let lines = rig.log_lines().await;
    let last = lines.last().expect("one log line");
    assert_eq!(last["event"], json!("render"));
    assert_eq!(last["outcome"], json!("error"));
}

#[tokio::test]
async fn render_list_shows_captures_and_visualizations_newest_first() {
    let rig = rig().await;
    let source = rig.capture(&tiny_png()).await;
    let made = rig.generate(request(&source.id)).await.expect("generates").record;

    let records = rig.list().await;
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].id, made.id, "newest first");
    assert_eq!(records[0].source, RenderSource::AiVisualization);
    assert_eq!(records[1].id, source.id);
    assert_eq!(records[1].source, RenderSource::ModelView);
    assert_eq!(records[1].source_render_id, None);
    assert_eq!(records[1].provider, None);
}

#[tokio::test]
async fn deleting_the_source_keeps_its_visualization() {
    let rig = rig().await;
    let source = rig.capture(&tiny_png()).await;
    let made = rig.generate(request(&source.id)).await.expect("generates").record;

    rig.app
        .handle("render_delete", json!({"id": source.id}))
        .await
        .expect("deletes the capture");

    let records = rig.list().await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, made.id);
    assert_eq!(
        records[0].source_render_id.as_deref(),
        Some(source.id.as_str()),
        "the link is kept even though the capture is gone, so the UI can say so"
    );
    // Its image is still readable.
    rig.app
        .handle("render_data", json!({"id": made.id}))
        .await
        .expect("the visualization is still there");
    // The capture is not.
    let e = rig
        .app
        .handle("render_data", json!({"id": source.id}))
        .await
        .expect_err("the capture is gone");
    assert_eq!(e.code, "not_found");
}

#[tokio::test]
async fn deleting_a_visualization_keeps_its_source() {
    let rig = rig().await;
    let source = rig.capture(&tiny_png()).await;
    let made = rig.generate(request(&source.id)).await.expect("generates").record;

    rig.app.handle("render_delete", json!({"id": made.id})).await.expect("deletes");
    let records = rig.list().await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, source.id);
    rig.app.handle("render_data", json!({"id": source.id})).await.expect("capture is intact");
}

#[tokio::test]
async fn render_data_serves_a_jpeg_answer_with_the_right_media_type() {
    let rig = rig_with(FakeProvider::jpeg(), true).await;
    let source = rig.capture(&tiny_png()).await;
    let made = rig.generate(request(&source.id)).await.expect("generates").record;

    assert!(made.image_path.ends_with(".jpg"), "stored as jpeg: {}", made.image_path);

    let v = rig.app.handle("render_data", json!({"id": made.id})).await.expect("render_data");
    let url: String = serde_json::from_value(v).expect("data URL");
    assert!(url.starts_with("data:image/jpeg;base64,"), "served as jpeg");
    let (kind, bytes) = files::decode_image_data_url(&url).expect("decodes");
    assert_eq!(kind, files::ImageKind::Jpeg);
    assert_eq!(bytes, tiny_jpeg());

    // The PNG capture still comes back as a PNG.
    let v = rig.app.handle("render_data", json!({"id": source.id})).await.expect("render_data");
    let url: String = serde_json::from_value(v).expect("data URL");
    assert!(url.starts_with("data:image/png;base64,"));
}

#[tokio::test]
async fn quality_and_style_travel_to_the_provider() {
    let rig = rig().await;
    let source = rig.capture(&png_of_size(1000, 1000)).await;
    let mut req = request(&source.id);
    req["quality"] = json!("high");
    req["style_key"] = json!("dusk-exterior");
    req["keep_geometry"] = json!(false);
    rig.generate(req).await.expect("generates");

    let inputs = rig.provider.inputs.lock().expect("inputs");
    let input = inputs.last().expect("one call");
    assert_eq!(input.quality, RenderQuality::High);
    assert_eq!(input.aspect_ratio, "1:1");
    assert!(input.constraint.is_empty(), "keep_geometry false sends no constraint");
    assert!(input.prompt.contains("blue hour"));
    assert!(!input.prompt.contains(super::prompt::GEOMETRY_LINE));
    assert_eq!(input.png, png_of_size(1000, 1000), "the capture is sent unchanged");
}

#[tokio::test]
async fn an_unknown_style_is_rejected_before_anything_is_spent() {
    let rig = rig().await;
    let source = rig.capture(&tiny_png()).await;
    let mut req = request(&source.id);
    req["style_key"] = json!("no-such-style");
    let e = rig.generate(req).await.expect_err("unknown style");
    assert_eq!(e.code, "invalid");
    assert_eq!(rig.provider.calls(), 0);
}

#[tokio::test]
async fn one_log_line_per_generation_and_it_holds_no_key() {
    let rig = rig().await;
    let source = rig.capture(&tiny_png()).await;
    let long_prompt = "warm afternoon light ".repeat(40);
    let mut req = request(&source.id);
    req["prompt"] = json!(long_prompt);
    let result = rig.generate(req).await.expect("generates");

    let lines = rig.log_lines().await;
    assert_eq!(lines.len(), 1, "one line per generation");
    let line = &lines[0];
    assert_eq!(line["event"], json!("render"));
    assert_eq!(line["outcome"], json!("ok"));
    assert_eq!(line["model"], json!("fake-image-model"));
    assert_eq!(line["bytes"], json!(tiny_png().len()));
    assert_eq!(line["render_id"], json!(result.record.id));
    assert!(line["seconds"].as_f64().expect("seconds") >= 0.0);

    let logged = line["prompt"].as_str().expect("prompt");
    assert!(logged.chars().count() <= super::LOG_PROMPT_CHARS + 3, "the prompt is truncated");
    let text = serde_json::to_string(line).expect("json");
    assert!(!text.contains(FAKE_KEY), "the key is never logged");
    assert!(!text.contains("AIza"), "no key-shaped string is logged");
}

// ------------------------------------------------------------------- settings

#[tokio::test]
async fn settings_round_trip() {
    let rig = rig_with(FakeProvider::new(), false).await;

    let before = rig.settings(json!({})).await.expect("get");
    assert_eq!(before.provider, "gemini");
    assert!(!before.has_api_key);
    assert_eq!(before.model, "", "empty means the quality picks the model");
    assert_eq!(
        before.cost_hint,
        "About $0.05 per draft, $0.10 per standard, $0.24 per high image (Google list price)"
    );

    let set = rig
        .settings(json!({"api_key": format!("  {FAKE_KEY}  "), "model": "pro"}))
        .await
        .expect("set");
    assert!(set.has_api_key);
    assert_eq!(set.model, "pro");
    assert_eq!(
        set.cost_hint,
        "About $0.13 per draft, $0.13 per standard, $0.24 per high image (Google list price)",
        "the hint follows the chosen model"
    );

    // The key survives a fresh get and never appears in the answer.
    let again = rig.settings(json!({})).await.expect("get");
    assert!(again.has_api_key);
    assert_eq!(again.model, "pro");
    let text = serde_json::to_string(&again).expect("json");
    assert!(!text.contains(FAKE_KEY));

    // It is not written to settings.json either.
    let settings_file = rig.app.session.lock().await.data_dir.join("settings.json");
    let stored = std::fs::read_to_string(&settings_file).expect("settings.json");
    assert!(stored.contains("render_ai_model"));
    assert!(!stored.contains(FAKE_KEY));
    assert!(!stored.contains("AIza"));

    // "" removes the key and the model setting.
    let cleared = rig.settings(json!({"api_key": "", "model": ""})).await.expect("clear");
    assert!(!cleared.has_api_key);
    assert_eq!(cleared.model, "");
}

#[tokio::test]
async fn a_key_that_is_not_a_google_key_is_refused_with_a_reason() {
    let rig = rig_with(FakeProvider::new(), false).await;
    let e = rig
        .settings(json!({"api_key": "sk-ant-api03-not-google"}))
        .await
        .expect_err("wrong vendor");
    assert_eq!(e.code, "invalid");
    assert!(e.message.contains("AIza"), "the message says what a key looks like: {}", e.message);
    assert!(!rig.settings(json!({})).await.expect("get").has_api_key);
}

#[tokio::test]
async fn a_nonsense_model_setting_is_refused() {
    let rig = rig_with(FakeProvider::new(), false).await;
    let e = rig.settings(json!({"model": "claude-opus-5"})).await.expect_err("not an image model");
    assert_eq!(e.code, "invalid");
    assert_eq!(rig.settings(json!({})).await.expect("get").model, "");

    let ok = rig.settings(json!({"model": "gemini-3.1-flash-image"})).await.expect("exact id");
    assert_eq!(ok.model, "gemini-3.1-flash-image");
}

#[tokio::test]
async fn the_copilot_settings_are_left_alone() {
    let rig = rig_with(FakeProvider::new(), false).await;
    rig.app
        .handle("ai_settings_set", json!({"api_key": null, "model": "claude-opus-5"}))
        .await
        .expect("copilot model");
    rig.settings(json!({"model": "flash"})).await.expect("render model");

    let v = rig.app.handle("ai_settings_get", json!({})).await.expect("copilot settings");
    let ai: AiSettings = serde_json::from_value(v).expect("AiSettings");
    assert_eq!(ai.model, "claude-opus-5", "one settings file, two independent keys");
    assert_eq!(rig.settings(json!({})).await.expect("get").model, "flash");
}

#[tokio::test]
async fn a_render_keeps_how_it_was_made_and_the_revision_it_started_from() {
    let r = rig().await;
    let url = files::encode_data_url(files::ImageKind::Png, &tiny_png());
    let doc: DocState = serde_json::from_value(r.app.handle("doc_state", json!({})).await.expect("doc_state")).expect("DocState");
    let started = doc.revision;
    // The model changes while the render runs.
    r.app
        .handle("doc_apply", json!({"command": {"type": "add_level", "name": null, "elevation_mm": null, "height_mm": null}}))
        .await
        .expect("a level is added");
    let now: DocState = serde_json::from_value(r.app.handle("doc_state", json!({})).await.expect("doc_state")).expect("DocState");
    assert!(now.revision > started);
    let info = json!({"kind": "path_traced", "width": 1920, "height": 1080, "samples": 384, "seconds": 61.5, "quality": "quick", "gpu": "Apple M5\u{7}"});
    let v = r
        .app
        .handle("render_capture", json!({"camera": camera(), "png": url, "revision": started, "info": info}))
        .await
        .expect("render saved");
    let record: RenderRecord = serde_json::from_value(v).expect("RenderRecord");
    assert_eq!(record.revision, started);
    let info = record.info.expect("info");
    assert_eq!((info.kind, info.width, info.height, info.samples), (RenderKind::PathTraced, 1920, 1080, 384));
    assert_eq!(info.quality, Some(TraceQuality::Quick));
    assert_eq!(info.gpu, "Apple M5", "control characters are dropped");
    // It survives the index.
    assert_eq!(r.list().await.iter().find(|x| x.id == record.id).and_then(|x| x.info.clone()), Some(info));

    // A plain capture has no info and the current revision.
    let plain = r.capture(&tiny_png()).await;
    assert_eq!(plain.info, None);

    // A revision from the future and a zero-sized image are refused.
    let e = r
        .app
        .handle("render_capture", json!({"camera": camera(), "png": url, "revision": started + 1000}))
        .await
        .expect_err("future revision");
    assert_eq!(e.code, "bad_args");
    let e = r
        .app
        .handle(
            "render_capture",
            json!({"camera": camera(), "png": url, "info": {"kind": "capture", "width": 0, "height": 10}}),
        )
        .await
        .expect_err("empty size");
    assert_eq!(e.code, "bad_args");
}
