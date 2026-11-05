//! Lane L6, AI render v2 (DECISIONS D53, D54): conditioning maps, model
//! facts, the project look, the drift check and the AI cost rules.
//!
//! No test calls a real provider. `FakeProvider` records what would have
//! been sent; one test points the real Gemini client at a stub server on
//! this computer to read the exact request body.

use std::sync::Arc;

use base64::Engine;
use guhit_app::render_ai::keys::MemoryKeyStore;
use guhit_app::render_ai::provider::{FakeProvider, ImageProvider};
use guhit_app::render_ai::{gemini::GeminiProvider, RenderAiState, DEFAULT_AGENT_CAP};
use guhit_app::AppService;
use guhit_model::*;
use image::{GrayImage, Luma, Rgb, RgbImage};
use serde_json::{json, Value};

/// Never a real key.
const FAKE_KEY: &str = "AIzaTEST-L6-NEVER-A-REAL-KEY-000000";

async fn call(app: &AppService, cmd: &str, args: Value) -> Value {
    app.handle(cmd, args)
        .await
        .unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

async fn fail(app: &AppService, cmd: &str, args: Value) -> IpcError {
    match app.handle(cmd, args).await {
        Ok(v) => panic!("{cmd} should have failed, got {v}"),
        Err(e) => e,
    }
}

struct Rig {
    app: AppService,
    provider: Arc<FakeProvider>,
    project: Project,
    _dir: tempfile::TempDir,
}

async fn rig_with(provider: Arc<dyn ImageProvider>, fake: Arc<FakeProvider>) -> Rig {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut app = AppService::new_sandboxed(dir.path().to_path_buf());
    app.render_ai = Arc::new(RenderAiState::with_parts(
        provider,
        Box::new(MemoryKeyStore::with_key(FAKE_KEY)),
    ));
    let state: DocState = serde_json::from_value(
        call(
            &app,
            "hub_create",
            json!({ "name": "Bahay", "template": "sample-bungalow" }),
        )
        .await,
    )
    .unwrap();
    Rig {
        app,
        provider: fake,
        project: state.project,
        _dir: dir,
    }
}

async fn rig() -> Rig {
    let fake = Arc::new(FakeProvider::new());
    rig_with(fake.clone(), fake).await
}

// ------------------------------------------------------------ the pictures

#[derive(Clone, Copy)]
struct Rect(u32, u32, u32, u32);

const W: u32 = 1024;
const H: u32 = 576;
const WALL: Rect = Rect(200, 160, 824, 500);
const ROOF: Rect = Rect(170, 90, 854, 160);
const WINDOW: Rect = Rect(290, 250, 410, 350);
const DOOR: Rect = Rect(600, 320, 680, 500);
const MOVED_WINDOW: Rect = Rect(440, 230, 560, 330);

fn png(img: impl Into<image::DynamicImage>) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.into()
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("png");
    out.into_inner()
}

fn data_url(bytes: &[u8]) -> String {
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

fn fill(img: &mut RgbImage, r: Rect, c: [u8; 3], sx: f32, sy: f32) {
    let (x0, y0, x1, y1) = (
        (r.0 as f32 * sx) as u32,
        (r.1 as f32 * sy) as u32,
        (r.2 as f32 * sx) as u32,
        (r.3 as f32 * sy) as u32,
    );
    for y in y0..=y1 {
        for x in x0..=x1 {
            img.put_pixel(x, y, Rgb(c));
        }
    }
}

/// A pretend photo of the house front: sky, a noisy lawn, wall, roof,
/// glass and a door, blurred a little. `window` is where it puts the window.
fn photo(width: u32, height: u32, window: Rect) -> Vec<u8> {
    let (sx, sy) = (width as f32 / W as f32, height as f32 / H as f32);
    let mut img = RgbImage::new(width, height);
    let mut seed = 99u32;
    for y in 0..height {
        for x in 0..width {
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            let n = ((seed >> 16) % 40) as i32 - 20;
            let t = y as f32 / height as f32;
            let c = if (y as f32) < 500.0 * sy {
                [(140.0 + 50.0 * t) as u8, (185.0 + 30.0 * t) as u8, 238]
            } else {
                [
                    (70 + n).clamp(0, 255) as u8,
                    (120 + n).clamp(0, 255) as u8,
                    (60 + n / 2).clamp(0, 255) as u8,
                ]
            };
            img.put_pixel(x, y, Rgb(c));
        }
    }
    fill(&mut img, WALL, [226, 214, 190], sx, sy);
    fill(&mut img, ROOF, [150, 72, 52], sx, sy);
    fill(&mut img, window, [58, 80, 104], sx, sy);
    fill(&mut img, DOOR, [104, 64, 40], sx, sy);
    png(image::DynamicImage::ImageRgb8(img).blur(1.2 * sx))
}

fn lines_map() -> Vec<u8> {
    let mut img = GrayImage::from_pixel(W, H, Luma([255]));
    for r in [WALL, ROOF, WINDOW, DOOR] {
        for t in 0..2 {
            for x in r.0..=r.2 {
                img.put_pixel(x, r.1 + t, Luma([0]));
                img.put_pixel(x, r.3 - t, Luma([0]));
            }
            for y in r.1..=r.3 {
                img.put_pixel(r.0 + t, y, Luma([0]));
                img.put_pixel(r.2 - t, y, Luma([0]));
            }
        }
    }
    png(img)
}

fn gray_map(v: u8) -> Vec<u8> {
    png(GrayImage::from_pixel(W, H, Luma([v])))
}

const COLORS: [[u8; 3]; 4] = [[12, 20, 28], [36, 20, 28], [60, 44, 28], [84, 44, 52]];

fn ids_map() -> Vec<u8> {
    let mut img = RgbImage::from_pixel(W, H, Rgb([0, 0, 0]));
    for (r, c) in [WALL, ROOF, WINDOW, DOOR].into_iter().zip(COLORS) {
        fill(&mut img, r, c, 1.0, 1.0);
    }
    png(img)
}

fn first_of(p: &Project, f: impl Fn(&Element) -> bool) -> Id {
    p.elements
        .iter()
        .find(|e| f(e))
        .map(|e| e.id().clone())
        .expect("element")
}

impl Rig {
    fn window_id(&self) -> Id {
        first_of(
            &self.project,
            |e| matches!(e, Element::Opening(o) if o.opening_type == OpeningType::Window),
        )
    }

    fn door_id(&self) -> Id {
        first_of(
            &self.project,
            |e| matches!(e, Element::Opening(o) if o.opening_type == OpeningType::Door),
        )
    }

    fn wall_id(&self) -> Id {
        first_of(&self.project, |e| matches!(e, Element::Wall(_)))
    }

    fn roof_id(&self) -> Id {
        first_of(&self.project, |e| matches!(e, Element::Roof(_)))
    }

    /// A model view, as the window saves one.
    async fn capture(&self) -> RenderRecord {
        let camera = Camera {
            id: String::new(),
            name: "Front".into(),
            preset: CameraPreset::Custom,
            position: Vec3 {
                x: 6000.0,
                y: -12000.0,
                z: 1600.0,
            },
            target: Vec3 {
                x: 6000.0,
                y: 0.0,
                z: 1600.0,
            },
            fov_deg: 45.0,
            light: Some(ViewLight {
                month: 3,
                day: 21,
                minutes: 16 * 60 + 30,
                sky: SkyKind::Clear,
                exposure_ev: None,
                lamps: LampMode::Auto,
            }),
        };
        let v = call(
            &self.app,
            "render_capture",
            json!({ "camera": camera, "png": data_url(&photo(W, H, WINDOW)) }),
        )
        .await;
        serde_json::from_value(v).expect("record")
    }

    /// The maps the window would draw for that view, with a legend of real
    /// element ids from the bungalow.
    async fn save_maps(&self, id: &str) -> RenderRecord {
        let ids = [
            self.wall_id(),
            self.roof_id(),
            self.window_id(),
            self.door_id(),
        ];
        let legend: Vec<Value> = ids
            .iter()
            .zip(COLORS)
            .map(|(id, c)| json!({"color": format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]), "element_id": id, "pixels": 12000}))
            .collect();
        let v = call(
            &self.app,
            "render_ai_maps_save",
            json!({
                "render_id": id,
                "maps": [
                    {"kind": "lines", "png": data_url(&lines_map())},
                    {"kind": "depth", "png": data_url(&gray_map(128))},
                    {"kind": "normal", "png": data_url(&gray_map(200))},
                    {"kind": "ids", "png": data_url(&ids_map())},
                    {"kind": "clay", "png": data_url(&gray_map(230))},
                ],
                "meta": {"width": W, "height": H, "visibility": {"roof": true, "cutaway": false}, "legend": legend},
            }),
        )
        .await;
        serde_json::from_value(v).expect("record")
    }

    async fn list(&self) -> Vec<RenderRecord> {
        serde_json::from_value(call(&self.app, "render_list", json!({})).await).expect("records")
    }

    async fn log_lines(&self) -> Vec<Value> {
        let dir = self.app.project_dir().await.expect("project dir");
        let text = std::fs::read_to_string(dir.join("ai-log.jsonl")).unwrap_or_default();
        text.lines()
            .map(|l| serde_json::from_str(l).expect("json"))
            .collect()
    }
}

fn request(source: &str, quality: &str) -> Value {
    json!({
        "source_render_id": source,
        "style_key": "tropical-modern",
        "prompt": "late afternoon",
        "quality": quality,
        "keep_geometry": true,
    })
}

// ----------------------------------------------------------------- the maps

#[tokio::test]
async fn maps_are_saved_with_the_record_and_go_with_it() {
    let rig = rig().await;
    let source = rig.capture().await;
    let saved = rig.save_maps(&source.id).await;
    let kinds: Vec<MapKind> = saved.maps.iter().map(|m| m.kind).collect();
    assert_eq!(
        kinds,
        vec![
            MapKind::Lines,
            MapKind::Depth,
            MapKind::Normal,
            MapKind::Ids,
            MapKind::Clay
        ]
    );
    for m in &saved.maps {
        assert!(
            std::path::Path::new(&m.path).is_file(),
            "{} is on disk",
            m.path
        );
    }
    let listed = rig.list().await;
    assert_eq!(
        listed
            .iter()
            .find(|r| r.id == source.id)
            .expect("listed")
            .maps
            .len(),
        5
    );

    // A map comes back as a data URL for the maps preview.
    let url: String = serde_json::from_value(
        call(
            &rig.app,
            "render_ai_map_data",
            json!({"id": source.id, "kind": "lines"}),
        )
        .await,
    )
    .unwrap();
    assert!(url.starts_with("data:image/png;base64,"));

    // Deleting the capture takes its maps to the trash with it.
    call(&rig.app, "render_delete", json!({"id": source.id})).await;
    for m in &saved.maps {
        assert!(
            !std::path::Path::new(&m.path).exists(),
            "{} went to the trash",
            m.path
        );
    }
}

#[tokio::test]
async fn only_a_model_view_has_maps_and_only_pngs_are_taken() {
    let rig = rig().await;
    let source = rig.capture().await;
    let bad = fail(
        &rig.app,
        "render_ai_maps_save",
        json!({"render_id": source.id, "maps": [{"kind": "lines", "png": "data:image/jpeg;base64,/9j/4AAQ"}], "meta": null}),
    )
    .await;
    assert_eq!(bad.code, "bad_args");
    let ai: RenderAiResult = serde_json::from_value(
        call(
            &rig.app,
            "render_ai_generate",
            json!({"request": request(&source.id, "draft")}),
        )
        .await,
    )
    .unwrap();
    let e = fail(
        &rig.app,
        "render_ai_maps_save",
        json!({"render_id": ai.record.id, "maps": [{"kind": "lines", "png": data_url(&lines_map())}], "meta": null}),
    )
    .await;
    assert_eq!(e.code, "invalid");
}

// ---------------------------------------------- the request: maps, look, facts

#[tokio::test]
async fn the_request_carries_the_maps_the_style_reference_and_the_model_facts() {
    let rig = rig().await;
    let source = rig.capture().await;
    rig.save_maps(&source.id).await;

    // An approved AI image becomes the project look.
    let first: RenderAiResult = serde_json::from_value(
        call(
            &rig.app,
            "render_ai_generate",
            json!({"request": request(&source.id, "draft")}),
        )
        .await,
    )
    .unwrap();
    rig.provider.set_answer(photo(W, H, WINDOW));
    let records: Vec<RenderRecord> = serde_json::from_value(
        call(&rig.app, "render_set_look", json!({"id": first.record.id})).await,
    )
    .unwrap();
    assert!(records.iter().any(|r| r.id == first.record.id && r.look));

    call(
        &rig.app,
        "render_ai_generate",
        json!({"request": request(&source.id, "standard")}),
    )
    .await;
    let input = rig.provider.last_input().expect("a call");
    let roles: Vec<&str> = input.maps.iter().map(|m| m.role.as_str()).collect();
    assert_eq!(
        roles,
        vec!["lines", "depth"],
        "the line drawing and the depth go with the capture"
    );
    assert_eq!(input.maps[0].bytes, lines_map());
    let style = input
        .style_reference
        .as_ref()
        .expect("the project look goes as the style reference");
    assert_eq!(style.role, "style");
    assert_eq!(style.mime, "image/jpeg");
    let look = image::load_from_memory(&style.bytes).expect("the look decodes");
    assert!(look.width() > 0);

    let text = &input.prompt;
    assert!(
        text.contains("Image 2 is the model's line drawing"),
        "{text}"
    );
    assert!(text.contains("Image 3 is the model's depth"), "{text}");
    assert!(text.contains("Image 4 is the project look"), "{text}");
    assert!(text.contains("Model facts"), "{text}");
    assert!(
        text.contains("Do not move, add or remove walls, doors or windows."),
        "{text}"
    );
    assert!(
        text.contains("Keep exactly 1 window and 1 door in view."),
        "{text}"
    );
    assert!(text.contains("- Light: 4:30 pm on 21 March"), "{text}");
    assert!(text.contains("single storey house"), "{text}");

    // The record keeps the text that made it, so the architect can read it back.
    let records = rig.list().await;
    let made = records
        .iter()
        .find(|r| r.source == RenderSource::AiVisualization && r.prompt == *text)
        .expect("saved");
    assert_eq!(made.source_render_id.as_deref(), Some(source.id.as_str()));
}

#[tokio::test]
async fn a_request_can_leave_the_look_and_the_maps_out() {
    let rig = rig().await;
    let source = rig.capture().await;
    rig.save_maps(&source.id).await;
    call(&rig.app, "render_set_look", json!({"id": source.id})).await;
    let mut req = request(&source.id, "draft");
    req["style_reference_id"] = json!("none");
    req["use_maps"] = json!(false);
    call(&rig.app, "render_ai_generate", json!({ "request": req })).await;
    let input = rig.provider.last_input().expect("a call");
    assert!(input.maps.is_empty());
    assert!(input.style_reference.is_none());
    assert!(!input.prompt.contains("Image 2"));
}

/// A stub of the Gemini endpoint on this computer: answers one request with
/// an image and hands back what it was sent.
async fn stub_gemini() -> (String, tokio::sync::oneshot::Receiver<(String, Value)>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let url = format!(
        "http://{}/v1beta/interactions",
        listener.local_addr().expect("addr")
    );
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.expect("accept");
        let mut buf = Vec::new();
        let mut chunk = [0u8; 65536];
        let (head, body) = loop {
            let n = sock.read(&mut chunk).await.expect("read");
            buf.extend_from_slice(&chunk[..n]);
            if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..at]).to_string();
                let len: usize = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap_or(0))
                    })
                    .unwrap_or(0);
                while buf.len() < at + 4 + len {
                    let n = sock.read(&mut chunk).await.expect("read body");
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                break (head, buf[at + 4..at + 4 + len].to_vec());
            }
            if n == 0 {
                panic!("the client hung up");
            }
        };
        let image = base64::engine::general_purpose::STANDARD.encode(photo(W, H, WINDOW));
        let answer =
            json!({"interaction": {"output_image": {"mime_type": "image/png", "data": image}}})
                .to_string();
        let reply = format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}", answer.len());
        sock.write_all(reply.as_bytes()).await.expect("write");
        let _ = tx.send((
            head,
            serde_json::from_slice(&body).expect("the body is JSON"),
        ));
    });
    (url, rx)
}

#[tokio::test]
async fn the_gemini_request_body_carries_the_maps_and_the_style_reference() {
    let (url, sent) = stub_gemini().await;
    let gemini = Arc::new(
        GeminiProvider::new(FAKE_KEY.into(), String::new())
            .expect("client")
            .with_url(url),
    );
    let rig = rig_with(gemini, Arc::new(FakeProvider::new())).await;
    let source = rig.capture().await;
    rig.save_maps(&source.id).await;
    call(&rig.app, "render_set_look", json!({"id": source.id})).await;

    let result: RenderAiResult = serde_json::from_value(
        call(
            &rig.app,
            "render_ai_generate",
            json!({"request": request(&source.id, "standard")}),
        )
        .await,
    )
    .unwrap();
    let (head, body) = sent.await.expect("the stub saw the request");

    assert!(head.to_ascii_lowercase().contains(&format!(
        "x-goog-api-key: {}",
        FAKE_KEY.to_ascii_lowercase()
    )));
    assert_eq!(
        body["model"],
        json!("gemini-3-pro-image"),
        "a style reference takes Gemini Pro (D53)"
    );
    assert_eq!(body["response_format"]["image_size"], json!("2K"));
    let parts = body["input"].as_array().expect("input");
    assert_eq!(
        parts.len(),
        5,
        "text, capture, line map, depth map, project look"
    );
    let text = parts[0]["text"].as_str().expect("text");
    assert!(text.contains("Image 4 is the project look") && text.contains("Model facts"));
    let b64 = |v: &[u8]| base64::engine::general_purpose::STANDARD.encode(v);
    assert_eq!(
        parts[2]["data"],
        json!(b64(&lines_map())),
        "the line map, as saved"
    );
    assert_eq!(
        parts[3]["data"],
        json!(b64(&gray_map(128))),
        "the depth map, as saved"
    );
    assert_eq!(
        parts[4]["mime_type"],
        json!("image/jpeg"),
        "the look, scaled down as JPEG"
    );
    assert!(
        !body.to_string().contains(FAKE_KEY),
        "the key is a header, never the body"
    );

    // The stub's image matches the model, so it is saved clean.
    let drift = result.record.drift.expect("checked");
    assert!(!drift.warn, "{}", drift.note);
}

// --------------------------------------------------------------- the drift

#[tokio::test]
async fn a_moved_window_is_flagged_before_saving_and_the_same_image_is_not() {
    let fake = Arc::new(FakeProvider::answering(photo(1536, 864, MOVED_WINDOW)));
    let rig = rig_with(fake.clone(), fake.clone()).await;
    let source = rig.capture().await;
    rig.save_maps(&source.id).await;
    let before = rig.list().await.len();

    // A person's render: the warning comes back before anything is saved.
    let draft = call(
        &rig.app,
        "render_ai_draft",
        json!({"request": request(&source.id, "standard")}),
    )
    .await;
    let drift: DriftReport = serde_json::from_value(draft["drift"].clone()).expect("checked");
    assert!(drift.warn, "{}", drift.note);
    assert!(
        drift.note.contains("window"),
        "the note names what moved: {}",
        drift.note
    );
    assert!(drift.missing > 0.0);
    assert!(
        draft["overlay"]
            .as_str()
            .is_some_and(|o| o.starts_with("data:image/png")),
        "the overlay shows where"
    );
    assert!(draft["image"]
        .as_str()
        .is_some_and(|o| o.starts_with("data:image/png")));
    assert_eq!(
        rig.list().await.len(),
        before,
        "nothing is saved before the person decides"
    );

    // Discard: still nothing. Keep: saved with the report.
    call(
        &rig.app,
        "render_ai_discard",
        json!({"pending_id": draft["pending_id"]}),
    )
    .await;
    assert_eq!(rig.list().await.len(), before);
    assert_eq!(
        fail(
            &rig.app,
            "render_ai_keep",
            json!({"pending_id": draft["pending_id"]})
        )
        .await
        .code,
        "not_found"
    );
    let again = call(
        &rig.app,
        "render_ai_draft",
        json!({"request": request(&source.id, "standard")}),
    )
    .await;
    let kept: RenderRecord = serde_json::from_value(
        call(
            &rig.app,
            "render_ai_keep",
            json!({"pending_id": again["pending_id"]}),
        )
        .await,
    )
    .unwrap();
    assert!(kept.drift.as_ref().is_some_and(|d| d.warn));
    let overlay: Option<String> = serde_json::from_value(
        call(&rig.app, "render_ai_drift_overlay", json!({"id": kept.id})).await,
    )
    .unwrap();
    assert!(overlay.is_some(), "the overlay is kept with the record");

    // The same image with the window where the model has it is not flagged.
    fake.set_answer(photo(1536, 864, WINDOW));
    let clean = call(
        &rig.app,
        "render_ai_draft",
        json!({"request": request(&source.id, "standard")}),
    )
    .await;
    let drift: DriftReport = serde_json::from_value(clean["drift"].clone()).expect("checked");
    assert!(!drift.warn, "{}", drift.note);
    assert!(!drift.note.contains("window"), "{}", drift.note);
}

#[tokio::test]
async fn an_agents_image_is_saved_with_its_drift_report() {
    let fake = Arc::new(FakeProvider::answering(photo(W, H, MOVED_WINDOW)));
    let rig = rig_with(fake.clone(), fake).await;
    let source = rig.capture().await;
    rig.save_maps(&source.id).await;
    let result: RenderAiResult = serde_json::from_value(
        call(
            &rig.app,
            "render_ai_generate",
            json!({"request": request(&source.id, "draft"), "agent": "agent-1"}),
        )
        .await,
    )
    .unwrap();
    let drift = result.record.drift.expect("checked");
    assert!(
        drift.warn && drift.note.contains("window"),
        "{}",
        drift.note
    );
}

#[tokio::test]
async fn without_maps_there_is_no_drift_check_and_it_says_so_by_leaving_it_out() {
    let rig = rig().await;
    let source = rig.capture().await;
    let result: RenderAiResult = serde_json::from_value(
        call(
            &rig.app,
            "render_ai_generate",
            json!({"request": request(&source.id, "draft")}),
        )
        .await,
    )
    .unwrap();
    assert!(result.record.drift.is_none());
}

// ------------------------------------------------------------------ the price

#[tokio::test]
async fn the_price_shows_before_every_call() {
    let rig = rig().await;
    let source = rig.capture().await;
    rig.save_maps(&source.id).await;
    let estimate = |q: &str| {
        let app = rig.app.clone();
        let req = request(&source.id, q);
        async move { call(&app, "render_ai_estimate", json!({ "request": req })).await }
    };

    let draft = estimate("draft").await;
    assert_eq!(draft["price_usd"], json!("0.05"));
    assert_eq!(draft["model_label"], json!("Gemini Flash"));
    assert_eq!(draft["pro"], json!(false));
    assert_eq!(draft["maps"], json!(["lines", "depth"]));
    assert!(
        draft["prompt"]
            .as_str()
            .is_some_and(|p| p.contains("Model facts")),
        "the whole text is shown too"
    );
    let high = estimate("high").await;
    assert_eq!(
        (high["price_usd"].clone(), high["pro"].clone()),
        (json!("0.24"), json!(true))
    );

    // The estimate spends nothing.
    assert_eq!(rig.provider.calls(), 0);

    // With the project look, Standard goes to Pro, and says why.
    call(&rig.app, "render_set_look", json!({"id": source.id})).await;
    let with_look = estimate("standard").await;
    assert_eq!(with_look["model_label"], json!("Gemini Pro"));
    assert_eq!(with_look["price_usd"], json!("0.13"));
    assert!(with_look["why"]
        .as_str()
        .is_some_and(|w| w.contains("look")));
    assert_eq!(with_look["style_reference_id"], json!(source.id));

    // The draft carries the same estimate it was made at, and the log the price.
    let made = call(
        &rig.app,
        "render_ai_draft",
        json!({"request": request(&source.id, "standard")}),
    )
    .await;
    assert_eq!(made["estimate"]["price_usd"], with_look["price_usd"]);
    let lines = rig.log_lines().await;
    assert_eq!(lines.last().expect("logged")["price_usd"], json!("0.13"));
}

// ------------------------------------------------------------- agents' cap

#[tokio::test]
async fn the_agent_cap_refuses_the_21st_image_with_a_hint_and_a_raise_lets_it_through() {
    let rig = rig().await;
    let source = rig.capture().await;
    let agent_call = || {
        let app = rig.app.clone();
        let req = request(&source.id, "draft");
        async move {
            app.handle(
                "render_ai_generate",
                json!({ "request": req, "agent": "codex-1" }),
            )
            .await
        }
    };
    for n in 0..DEFAULT_AGENT_CAP {
        agent_call()
            .await
            .unwrap_or_else(|e| panic!("image {} of the day: {e}", n + 1));
    }
    let usage = call(&rig.app, "render_ai_usage", json!({})).await;
    assert_eq!(
        (usage["used"].clone(), usage["cap"].clone()),
        (json!(20), json!(20))
    );

    let refused = agent_call().await.expect_err("the 21st is refused");
    assert_eq!(refused.code, "rate_limited");
    assert!(refused.message.contains("20 of 20"), "{}", refused.message);
    assert!(
        refused.message.contains("raise") && refused.message.contains("Visuals"),
        "the hint says what to do: {}",
        refused.message
    );
    assert_eq!(
        rig.provider.calls(),
        20,
        "nothing was spent on the refused call"
    );

    // A person is never held to the agents' cap.
    call(
        &rig.app,
        "render_ai_draft",
        json!({"request": request(&source.id, "draft")}),
    )
    .await;

    // A person raises it; the 21st goes through and is counted.
    let raised = call(
        &rig.app,
        "render_ai_usage_set",
        json!({"agent_daily_cap": 25}),
    )
    .await;
    assert_eq!(raised["cap"], json!(25));
    agent_call().await.expect("the raise lets it through");
    let usage = call(&rig.app, "render_ai_usage", json!({})).await;
    assert_eq!(
        (usage["used"].clone(), usage["cap"].clone()),
        (json!(21), json!(25))
    );

    // Zero stops agents; more than the most is refused.
    call(
        &rig.app,
        "render_ai_usage_set",
        json!({"agent_daily_cap": 0}),
    )
    .await;
    let none = agent_call().await.expect_err("cap 0");
    assert!(none.message.contains("limit is 0"), "{}", none.message);
    assert_eq!(
        fail(
            &rig.app,
            "render_ai_usage_set",
            json!({"agent_daily_cap": 5000})
        )
        .await
        .code,
        "bad_args"
    );
}

// ------------------------------------------------- tour videos (for lane L9)

#[tokio::test]
async fn a_video_record_is_pushed_found_again_and_trashed_with_its_file() {
    let rig = rig().await;
    let dir = rig.app.project_dir().await.expect("project dir");
    let renders = dir.join("renders");
    std::fs::create_dir_all(&renders).unwrap();
    let poster = rig.capture().await;
    for (id, codec) in [("vid-avc", "avc"), ("vid-png", "png")] {
        std::fs::copy(&poster.image_path, renders.join(format!("{id}.png"))).unwrap();
        if codec == "png" {
            std::fs::create_dir_all(renders.join(id)).unwrap();
            std::fs::write(
                renders.join(id).join("frame-000001.png"),
                png(GrayImage::new(2, 2)),
            )
            .unwrap();
        } else {
            std::fs::write(renders.join(format!("{id}.mp4")), b"not really an mp4").unwrap();
        }
        let record = RenderRecord {
            id: id.to_string(),
            info: Some(RenderInfo {
                kind: RenderKind::Video,
                width: 1920,
                height: 1080,
                samples: 0,
                seconds: 30.0,
                quality: None,
                gpu: String::new(),
                visibility: None,
            }),
            video: Some(VideoInfo {
                path: "/somewhere/else/old.mp4".into(),
                tour_id: "tour-1".into(),
                seconds: 30.0,
                fps: 30,
                width: 1920,
                height: 1080,
                codec: codec.into(),
            }),
            ..poster.clone()
        };
        let s = rig.app.session.lock().await;
        guhit_app::renders::push(&dir, &record).expect("pushed");
        drop(s);
    }
    let listed = rig.list().await;
    let avc = listed.iter().find(|r| r.id == "vid-avc").expect("listed");
    assert_eq!(
        avc.video.as_ref().unwrap().path,
        renders.join("vid-avc.mp4").to_string_lossy(),
        "the path follows the folder"
    );
    let frames = listed.iter().find(|r| r.id == "vid-png").expect("listed");
    assert_eq!(
        frames.video.as_ref().unwrap().path,
        renders.join("vid-png").to_string_lossy()
    );
    // A video is never the project look nor a source of maps.
    assert_eq!(
        fail(&rig.app, "render_set_look", json!({"id": "vid-avc"}))
            .await
            .code,
        "invalid"
    );

    call(&rig.app, "render_delete", json!({"id": "vid-avc"})).await;
    call(&rig.app, "render_delete", json!({"id": "vid-png"})).await;
    assert!(
        !renders.join("vid-avc.mp4").exists() && !renders.join("vid-png").exists(),
        "both went to the trash"
    );
    let trash = dir.parent().unwrap().parent().unwrap().join("trash");
    let names: Vec<String> = std::fs::read_dir(trash)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names
            .iter()
            .any(|n| n.contains("vid-avc") && n.ends_with(".mp4")),
        "{names:?}"
    );
    assert!(
        names
            .iter()
            .any(|n| n.contains("vid-png") && !n.ends_with(".png")),
        "the frame folder: {names:?}"
    );
}

// -------------------------------------------------------------- the look

#[tokio::test]
async fn the_project_look_is_one_record_at_a_time() {
    let rig = rig().await;
    let a = rig.capture().await;
    let b = rig.capture().await;
    let looks = |records: &[RenderRecord]| {
        records
            .iter()
            .filter(|r| r.look)
            .map(|r| r.id.clone())
            .collect::<Vec<_>>()
    };

    let records: Vec<RenderRecord> =
        serde_json::from_value(call(&rig.app, "render_set_look", json!({"id": a.id})).await)
            .unwrap();
    assert_eq!(looks(&records), vec![a.id.clone()]);
    let records: Vec<RenderRecord> =
        serde_json::from_value(call(&rig.app, "render_set_look", json!({"id": b.id})).await)
            .unwrap();
    assert_eq!(looks(&records), vec![b.id.clone()]);
    assert_eq!(looks(&rig.list().await), vec![b.id.clone()], "it is stored");
    let records: Vec<RenderRecord> =
        serde_json::from_value(call(&rig.app, "render_set_look", json!({"id": null})).await)
            .unwrap();
    assert!(looks(&records).is_empty());
    assert_eq!(
        fail(&rig.app, "render_set_look", json!({"id": "no-such-render"}))
            .await
            .code,
        "not_found"
    );

    // A look that is deleted is simply gone: later renders go without one.
    call(&rig.app, "render_set_look", json!({"id": a.id})).await;
    call(&rig.app, "render_delete", json!({"id": a.id})).await;
    let e = call(
        &rig.app,
        "render_ai_estimate",
        json!({"request": request(&b.id, "standard")}),
    )
    .await;
    assert_eq!(e["style_reference_id"], Value::Null);
    assert_eq!(e["pro"], json!(false));
}

// ------------------------------------------------------- contract shapes

#[tokio::test]
async fn a_capture_keeps_the_roof_and_cutaway_it_showed() {
    let rig = rig().await;
    let camera = rig.capture().await.camera;
    let info = |visibility: Value| {
        let mut v = json!({
            "kind": "capture", "width": W, "height": H, "samples": 0,
            "seconds": 0.0, "quality": null, "gpu": "",
        });
        if !visibility.is_null() {
            v["visibility"] = visibility;
        }
        v
    };
    let png = data_url(&photo(W, H, WINDOW));
    let shown = call(
        &rig.app,
        "render_capture",
        json!({ "camera": camera, "png": png, "info": info(json!({"roof": false, "cutaway": true})) }),
    )
    .await;
    assert_eq!(shown["info"]["visibility"], json!({"roof": false, "cutaway": true}));
    // Unknown visibility stays out of the JSON, as before.
    let unknown = call(
        &rig.app,
        "render_capture",
        json!({ "camera": camera, "png": png, "info": info(Value::Null) }),
    )
    .await;
    assert!(
        unknown["info"].as_object().is_some_and(|o| !o.contains_key("visibility")),
        "{}",
        unknown["info"]
    );

    // The index keeps it, and reads it back.
    let listed = rig.list().await;
    let by_id = |id: &Value| listed.iter().find(|r| json!(r.id) == *id).cloned().expect("listed");
    assert_eq!(
        by_id(&shown["id"]).info.and_then(|i| i.visibility),
        Some(ViewVisibility { roof: false, cutaway: true })
    );
    assert_eq!(by_id(&unknown["id"]).info.and_then(|i| i.visibility), None);
}

#[tokio::test]
async fn the_ai_render_answers_keep_their_json_shapes() {
    let rig = rig().await;
    let source = rig.capture().await;
    rig.save_maps(&source.id).await;
    let keys = |v: &Value| {
        let mut k: Vec<String> = v.as_object().expect("an object").keys().cloned().collect();
        k.sort();
        k
    };
    let usage = call(&rig.app, "render_ai_usage", json!({})).await;
    assert_eq!(keys(&usage), ["cap", "day", "default_cap", "used"]);
    let estimate = call(
        &rig.app,
        "render_ai_estimate",
        json!({"request": request(&source.id, "draft")}),
    )
    .await;
    assert_eq!(
        keys(&estimate),
        [
            "agents",
            "image_size",
            "maps",
            "maps_missing",
            "model",
            "model_label",
            "price_usd",
            "pro",
            "prompt",
            "style_reference_id",
            "why"
        ]
    );
    let draft = call(
        &rig.app,
        "render_ai_draft",
        json!({"request": request(&source.id, "draft")}),
    )
    .await;
    assert_eq!(
        keys(&draft),
        ["drift", "estimate", "image", "overlay", "pending_id", "provider", "seconds"]
    );
    // The maps metadata on disk, and the typed value read back from it.
    let dir = rig.app.project_dir().await.expect("project dir");
    let file = dir.join("renders").join(format!("{}.maps.json", source.id));
    let meta: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert_eq!(keys(&meta), ["height", "legend", "visibility", "width"]);
    assert_eq!(keys(&meta["legend"][0]), ["color", "element_id", "pixels"]);
    let typed = guhit_app::renders::read_maps_meta(&dir, &source.id).expect("meta");
    assert_eq!(typed.legend.len(), 4);
    assert!(
        typed.legend.iter().all(|e| e.rgb().is_some()),
        "LegendEntry::rgb reads every drawn color"
    );
}
