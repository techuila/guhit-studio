//! Views, renders and pictures (DECISIONS D31). The path tracer and the plan
//! canvas live in the desktop window, so the render and capture tools ask the
//! window to do the work (`AppService::window_start`) and wait for its answer.
//! AI visualization runs in the backend with the user's own Gemini key
//! (DECISIONS D17). Every image a tool hands back is at most 1568 px on its
//! long side, which is what a model reads best.

use std::io::Cursor;
use std::time::Duration;

use base64::Engine as _;
use guhit_app::{store, AppService};
use guhit_model::*;
use serde_json::{json, Value};

use crate::tools::{obj, opt_f64, opt_str, req_str, Output, Picture, ToolDef, ToolFail, MM};

/// Longest side of an image handed to the model.
pub const PICTURE_MAX_PX: u32 = 1568;
/// A render tool waits this long by default before it hands back a job id.
const WAIT_DEFAULT_S: u64 = 45;
/// Longest wait a caller may ask for, under the usual MCP client timeouts.
const WAIT_MAX_S: u64 = 110;
/// A plan picture is quick once the plan is on screen; a window that has not
/// answered by then is not coming.
const PLAN_TIMEOUT: Duration = Duration::from_secs(60);

// -------------------------------------------------------------- definitions

pub fn definitions() -> Vec<ToolDef> {
    let wait = json!({"type": "number", "description": "Seconds to wait for the render before handing back a job id, 1 to 110. Defaults to 45."});
    vec![
        ToolDef {
            name: "add_camera",
            description: format!("Save a view (a camera) the user and the render tools can go back to, as one undo step. position is where the eye is and target the point it looks at; z is the height above the floor of `level` (the first level when omitted), so an eye-level view is z 1600 and a view from outside the house is a point a few metres beyond a wall. fov_deg is the vertical field of view, 60 when omitted. The view is listed by list_elements (kind camera) and can be rendered by name with render_view. {MM}"),
            schema: obj(
                json!({
                    "name": {"type": "string", "description": "What the view shows, for example \"Living room from the entrance\"."},
                    "position": point3("Where the eye is"),
                    "target": point3("The point the view looks at"),
                    "fov_deg": {"type": "number", "description": "Vertical field of view in degrees, 20 to 100. Defaults to 60."},
                    "level": {"type": "string", "description": "The level the heights are measured from: its id or name. Defaults to the first level."},
                    "scope": crate::tools::scope_schema(),
                }),
                &["name", "position", "target"],
            ),
            read_only: false,
            open_world: false,
        },
        ToolDef {
            name: "capture_view",
            description: format!("Capture the live 3D view in the Guhit Studio window as it is on screen, or from a saved view when `view` names one, and save it to Visuals. Usually a second or two; when the window needs longer (opening the 3D view for the first time on a slow computer) it hands back a job_id for get_render_job. Returns the record and the picture. Use it to look at the model, and as the source for visualize_render. Needs the Guhit Studio window open on the project. {MM}"),
            schema: obj(
                json!({
                    "view": {"type": "string", "description": "A saved view's id or name. Omit for the view on screen."},
                    "wait_seconds": wait.clone(),
                }),
                &[],
            ),
            read_only: false,
            open_world: false,
        },
        ToolDef {
            name: "render_view",
            description: format!("Render with the path tracer in the Guhit Studio window, like its Render button, and save each image to Visuals: the view on screen, or saved views by id or name, one after the other. quality \"quick\" aims at about a minute per HD image, \"final\" at about five. Returns the records and a preview when done within wait_seconds; otherwise a job_id to pass to get_render_job. The window stays usable while it renders. On a computer where the path tracer cannot run, the window saves an enhanced capture instead and says so. {MM}"),
            schema: obj(
                json!({
                    "views": {"type": "array", "items": {"type": "string"}, "description": "Saved views to render, by id or name. Omit for the view on screen."},
                    "quality": {"type": "string", "enum": ["quick", "final"], "description": "Defaults to quick."},
                    "size": {"type": "string", "enum": ["hd", "qhd", "4k", "square"], "description": "hd 1920 x 1080 (the default), qhd 2560 x 1440, 4k 3840 x 2160, square 2048 x 2048."},
                    "wait_seconds": wait.clone(),
                }),
                &[],
            ),
            read_only: false,
            open_world: false,
        },
        ToolDef {
            name: "get_render_job",
            description: format!("The outcome of a render_view or capture_view that was still running: the records and a preview once it is done, or status \"running\" again after wait_seconds. {MM}"),
            schema: obj(
                json!({"job_id": {"type": "string", "description": "From render_view or capture_view."}, "wait_seconds": wait}),
                &["job_id"],
            ),
            read_only: true,
            open_world: false,
        },
        ToolDef {
            name: "get_render_image",
            description: format!("A saved visual (from list_renders, capture_view, render_view or visualize_render) as a picture, at most 1568 px on its long side, with its record: the revision and camera it shows and how it was made. {MM}"),
            schema: obj(json!({"id": {"type": "string", "description": "The record id."}}), &["id"]),
            read_only: true,
            open_world: false,
        },
        ToolDef {
            name: "visualize_render",
            description: format!("Make an AI visualization from a saved model view (a capture or a render, source \"model_view\" in list_renders) with the image model and Google key the user set up in Guhit Studio's Visuals panel. It costs the user money per image (the Visuals panel shows the price) and takes 10 to 60 seconds, so only call it when the user asks for an AI image. The result is labelled \"AI visualization\", keeps a link to the view it came from, and never changes the model: geometry stays the authority. Styles are listed by the render_styles of the app; leave style_key out for the default look. {MM}"),
            schema: obj(
                json!({
                    "source_render_id": {"type": "string", "description": "A model view record id."},
                    "prompt": {"type": "string", "description": "What the image should show: building type, materials, mood, time of day, people or planting. The model keeps the geometry of the source view."},
                    "style_key": {"type": "string", "description": "A render style key, for example \"modern-tropical\". Omit for none."},
                    "quality": {"type": "string", "enum": ["draft", "standard", "high"], "description": "draft about 0.5K and cheapest, standard 1K to 2K (the default), high up to 4K."},
                    "keep_geometry": {"type": "boolean", "description": "Keep walls, openings and the camera strictly. Defaults to true."},
                }),
                &["source_render_id", "prompt"],
            ),
            read_only: false,
            open_world: true,
        },
    ]
}

fn point3(what: &str) -> Value {
    json!({
        "type": "object",
        "description": format!("{what}. x east, y north, z up, in millimeters."),
        "properties": {"x": {"type": "number"}, "y": {"type": "number"}, "z": {"type": "number"}},
        "required": ["x", "y", "z"],
        "additionalProperties": false,
    })
}

// ------------------------------------------------------------------ pictures

/// Split a `data:<mime>;base64,<data>` URL.
pub fn picture_from_data_url(url: &str) -> Result<Picture, ToolFail> {
    let rest = url
        .strip_prefix("data:")
        .ok_or_else(|| ToolFail("io: the image is not a data URL".into()))?;
    let (head, data) = rest
        .split_once(',')
        .ok_or_else(|| ToolFail("io: the image data URL has no data".into()))?;
    let mime = head
        .strip_suffix(";base64")
        .ok_or_else(|| ToolFail("io: the image data URL is not base64".into()))?;
    if !matches!(mime, "image/png" | "image/jpeg" | "image/webp") {
        return Err(ToolFail(format!("io: unexpected image type {mime}")));
    }
    Ok(Picture { base64: data.to_string(), mime: mime.to_string() })
}

/// A picture the model can take: PNG or JPEG bytes at most `PICTURE_MAX_PX`
/// on the long side. A larger image is scaled down and sent as JPEG.
pub fn fit_picture(bytes: &[u8], mime: &str) -> Result<Picture, ToolFail> {
    let img = image::load_from_memory(bytes).map_err(|e| ToolFail(format!("io: the image cannot be read: {e}")))?;
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    if img.width().max(img.height()) <= PICTURE_MAX_PX {
        return Ok(Picture { base64: b64(bytes), mime: mime.to_string() });
    }
    let small = img.resize(PICTURE_MAX_PX, PICTURE_MAX_PX, image::imageops::FilterType::Triangle);
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(small.to_rgb8())
        .write_to(&mut out, image::ImageFormat::Jpeg)
        .map_err(|e| ToolFail(format!("io: the image cannot be encoded: {e}")))?;
    Ok(Picture { base64: b64(out.get_ref()), mime: "image/jpeg".into() })
}

/// A saved record's image, fitted for the model.
async fn record_picture(app: &AppService, id: &str) -> Result<Picture, ToolFail> {
    let url: String = serde_json::from_value(app.handle("render_data", json!({ "id": id })).await?)
        .map_err(|e| ToolFail(format!("io: {e}")))?;
    let raw = picture_from_data_url(&url)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw.base64.as_bytes())
        .map_err(|e| ToolFail(format!("io: the stored image is not base64: {e}")))?;
    fit_picture(&bytes, &raw.mime)
}

async fn record(app: &AppService, id: &str) -> Result<Value, ToolFail> {
    let list = app.handle("render_list", json!({})).await?;
    list.as_array()
        .and_then(|l| l.iter().find(|r| r["id"] == id).cloned())
        .ok_or_else(|| ToolFail(format!("not_found: no saved visual has id `{id}`. list_renders shows them")))
}

// -------------------------------------------------------------------- views

/// A saved view by id or name (case and surrounding spaces ignored).
fn find_view(project: &Project, wanted: &str) -> Result<Id, ToolFail> {
    let wanted = wanted.trim();
    let cameras: Vec<&Camera> = project
        .elements
        .iter()
        .filter_map(|e| match e {
            Element::Camera(c) => Some(c),
            _ => None,
        })
        .collect();
    if let Some(c) = cameras.iter().find(|c| c.id == wanted) {
        return Ok(c.id.clone());
    }
    let named: Vec<&&Camera> = cameras.iter().filter(|c| c.name.trim().eq_ignore_ascii_case(wanted)).collect();
    match named.as_slice() {
        [one] => Ok(one.id.clone()),
        [] => {
            let known: Vec<String> = cameras.iter().map(|c| format!("\"{}\" ({})", c.name, c.id)).collect();
            Err(ToolFail(if known.is_empty() {
                format!("not_found: no saved view is called `{wanted}`, and the project has no saved views. add_camera saves one")
            } else {
                format!("not_found: no saved view is called `{wanted}`. Saved views: {}", known.join(", "))
            }))
        }
        _ => Err(ToolFail(format!("invalid arguments: more than one saved view is called `{wanted}`; give its id"))),
    }
}

async fn views_of(app: &AppService, names: &[String]) -> Result<Vec<Id>, ToolFail> {
    let s = app.session.lock().await;
    let doc = s.doc.as_ref().ok_or_else(|| ToolFail("no_document: no project is open".into()))?;
    names.iter().map(|n| find_view(doc.project(), n)).collect()
}

fn wait_of(args: &Value) -> Result<Duration, ToolFail> {
    let secs = match opt_f64(args, "wait_seconds")? {
        None => WAIT_DEFAULT_S as f64,
        Some(v) => v,
    };
    Ok(Duration::from_secs_f64(secs.clamp(1.0, WAIT_MAX_S as f64)))
}

/// What a finished render or capture job hands back: the records, the
/// window's note, and the preview it made.
async fn finished(app: &AppService, job_id: Option<&str>, reply: WindowReply) -> Result<Output, ToolFail> {
    let mut records = vec![];
    for id in &reply.render_ids {
        records.push(record(app, id).await.unwrap_or_else(|_| json!({"id": id})));
    }
    let mut out = json!({
        "status": "done",
        "renders": records,
        "note": reply.note,
    });
    if let Some(job) = job_id {
        out["job_id"] = json!(job);
    }
    let images = match reply.image.as_deref() {
        Some(url) => vec![picture_from_data_url(url)?],
        None => vec![],
    };
    Ok(Output::Rich { json: out, images })
}

fn running(job_id: &str) -> Output {
    Output::Json(json!({
        "status": "running",
        "job_id": job_id,
        "note": "The window is still working on it. Call get_render_job with this job_id to wait for it; the images also appear in the app's Visuals panel.",
    }))
}

// ------------------------------------------------------------------- tools

pub async fn call(app: &AppService, name: &str, args: &Value) -> Option<Result<Output, ToolFail>> {
    Some(match name {
        "add_camera" => add_camera(app, args).await,
        "capture_view" => capture_view(app, args).await,
        "render_view" => render_view(app, args).await,
        "get_render_job" => get_render_job(app, args).await,
        "get_render_image" => get_render_image(app, args).await,
        "visualize_render" => visualize_render(app, args).await,
        _ => return None,
    })
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AddCameraInput {
    name: String,
    position: Vec3,
    target: Vec3,
    fov_deg: Option<f64>,
    level: Option<String>,
    #[allow(dead_code)]
    scope: Option<Value>,
}

async fn add_camera(app: &AppService, args: &Value) -> Result<Output, ToolFail> {
    let input: AddCameraInput =
        serde_json::from_value(args.clone()).map_err(|e| ToolFail(format!("invalid arguments: {e}")))?;
    let name = input.name.trim();
    if name.is_empty() {
        return Err(ToolFail("invalid arguments: name is empty".into()));
    }
    let finite = |v: &Vec3| [v.x, v.y, v.z].iter().all(|n| n.is_finite());
    if !finite(&input.position) || !finite(&input.target) {
        return Err(ToolFail("invalid arguments: position and target need finite x, y and z".into()));
    }
    let fov = input.fov_deg.unwrap_or(60.0);
    if !(20.0..=100.0).contains(&fov) {
        return Err(ToolFail(format!("invalid arguments: fov_deg must be 20 to 100, got {fov}")));
    }
    let (dx, dy, dz) = (
        input.target.x - input.position.x,
        input.target.y - input.position.y,
        input.target.z - input.position.z,
    );
    if (dx * dx + dy * dy + dz * dz).sqrt() < 100.0 {
        return Err(ToolFail("invalid arguments: target must be at least 100 mm from position".into()));
    }
    let floor = {
        let s = app.session.lock().await;
        let doc = s.doc.as_ref().ok_or_else(|| ToolFail("no_document: no project is open".into()))?;
        let project = doc.project();
        let level = match input.level.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
            None => project.levels.first(),
            Some(wanted) => project
                .levels
                .iter()
                .find(|l| l.id == wanted)
                .or_else(|| project.levels.iter().find(|l| l.name.trim().eq_ignore_ascii_case(wanted))),
        };
        level
            .map(|l| l.elevation_mm)
            .ok_or_else(|| ToolFail(format!("not_found: no level is called `{}`", input.level.unwrap_or_default())))?
    };
    let lift = |v: Vec3| Vec3 { x: v.x, y: v.y, z: v.z + floor };
    let command = Command::AddElement {
        element: Element::Camera(Camera {
            id: String::new(),
            name: name.to_string(),
            preset: CameraPreset::Custom,
            position: lift(input.position),
            target: lift(input.target),
            fov_deg: fov,
            light: None,
        }),
    };
    crate::tools::commit_one(app, "MCP: add camera", command, args).await
}

async fn capture_view(app: &AppService, args: &Value) -> Result<Output, ToolFail> {
    let camera_id = match opt_str(args, "view")? {
        Some(v) if !v.trim().is_empty() => Some(views_of(app, &[v]).await?.remove(0)),
        _ => None,
    };
    let wait = wait_of(args)?;
    let job = app.window_start(WindowTask::CaptureView { camera_id })?;
    match app.window_wait(&job, wait).await? {
        Some(reply) => finished(app, Some(&job), reply).await,
        None => Ok(running(&job)),
    }
}

async fn render_view(app: &AppService, args: &Value) -> Result<Output, ToolFail> {
    let names: Vec<String> = match args.get("views") {
        None | Some(Value::Null) => vec![],
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|_| ToolFail("invalid arguments: `views` must be a list of saved view ids or names".into()))?,
    };
    let views = views_of(app, &names).await?;
    let quality = match opt_str(args, "quality")?.as_deref() {
        None | Some("quick") => TraceQuality::Quick,
        Some("final") => TraceQuality::Final,
        Some(other) => return Err(ToolFail(format!("invalid arguments: unknown quality `{other}`. Use quick or final"))),
    };
    let size = match opt_str(args, "size")?.as_deref() {
        None | Some("hd") => RenderSize::Hd,
        Some("qhd") => RenderSize::Qhd,
        Some("4k") => RenderSize::Uhd,
        Some("square") => RenderSize::Square,
        Some(other) => return Err(ToolFail(format!("invalid arguments: unknown size `{other}`. Use hd, qhd, 4k or square"))),
    };
    let wait = wait_of(args)?;
    {
        let s = app.session.lock().await;
        s.doc.as_ref().ok_or_else(|| ToolFail("no_document: no project is open".into()))?;
    }
    let job = app.window_start(WindowTask::Render { views, quality, size })?;
    match app.window_wait(&job, wait).await? {
        Some(reply) => finished(app, Some(&job), reply).await,
        None => Ok(running(&job)),
    }
}

async fn get_render_job(app: &AppService, args: &Value) -> Result<Output, ToolFail> {
    let job = req_str(args, "job_id")?;
    let wait = wait_of(args)?;
    match app.window_wait(&job, wait).await? {
        Some(reply) => finished(app, Some(&job), reply).await,
        None => Ok(running(&job)),
    }
}

async fn get_render_image(app: &AppService, args: &Value) -> Result<Output, ToolFail> {
    let id = req_str(args, "id")?;
    let meta = record(app, &id).await?;
    let picture = record_picture(app, &id).await?;
    let mut json = json!({"record": meta});
    if meta["source"] == "ai_visualization" {
        json["label"] = json!("AI visualization. It shows a look, not the design: the model is the authority.");
    }
    Ok(Output::Rich { json, images: vec![picture] })
}

async fn visualize_render(app: &AppService, args: &Value) -> Result<Output, ToolFail> {
    let source = req_str(args, "source_render_id")?;
    let prompt = req_str(args, "prompt")?;
    let quality = match opt_str(args, "quality")?.as_deref() {
        None | Some("standard") => RenderQuality::Standard,
        Some("draft") => RenderQuality::Draft,
        Some("high") => RenderQuality::High,
        Some(other) => {
            return Err(ToolFail(format!(
                "invalid arguments: unknown quality `{other}`. Use draft, standard or high"
            )))
        }
    };
    let keep_geometry = crate::tools::opt_bool(args, "keep_geometry")?.unwrap_or(true);
    let request = RenderAiRequest {
        source_render_id: source,
        style_key: opt_str(args, "style_key")?.filter(|k| !k.trim().is_empty()),
        prompt,
        quality,
        keep_geometry,
    };
    let result: RenderAiResult = serde_json::from_value(app.handle("render_ai_generate", json!({ "request": request })).await?)
        .map_err(|e| ToolFail(format!("io: {e}")))?;
    let picture = record_picture(app, &result.record.id).await?;
    Ok(Output::Rich {
        json: json!({
            "record": result.record,
            "seconds": result.seconds,
            "label": "AI visualization. It shows a look, not the design: the model is the authority, and the picture never changes it.",
        }),
        images: vec![picture],
    })
}

// ---------------------------------------------------------------- the plan

/// `get_plan_image`: a fresh drawing of the plan from the window when one is
/// open, else the last thumbnail the window saved.
pub async fn plan_image(app: &AppService, args: &Value) -> Result<Output, ToolFail> {
    let level_id = match opt_str(args, "level")?.map(|l| l.trim().to_string()).filter(|l| !l.is_empty()) {
        None => None,
        Some(wanted) => {
            let s = app.session.lock().await;
            let doc = s.doc.as_ref().ok_or_else(|| ToolFail("no_document: no project is open".into()))?;
            let project = doc.project();
            let level = project
                .levels
                .iter()
                .find(|l| l.id == wanted)
                .or_else(|| project.levels.iter().find(|l| l.name.trim().eq_ignore_ascii_case(&wanted)))
                .ok_or_else(|| ToolFail(format!("not_found: no level is called `{wanted}`")))?;
            Some(level.id.clone())
        }
    };
    if app.event_listeners() > 0 {
        match app.window_request(WindowTask::CapturePlan { level_id: level_id.clone() }, PLAN_TIMEOUT).await {
            Ok(reply) => {
                if let Some(url) = reply.image.as_deref() {
                    return Ok(Output::Rich {
                        json: json!({"source": "window", "note": reply.note}),
                        images: vec![picture_from_data_url(url)?],
                    });
                }
            }
            // A window on the hub, or none answering: fall back below.
            Err(e) if matches!(e.code.as_str(), "no_window" | "no_document") => {}
            Err(e) => return Err(e.into()),
        }
    }
    if level_id.is_some() {
        return Err(ToolFail(
            "no_window: a plan of another level needs the Guhit Studio window open on the project. Use export_plan with format \"svg\" to get a drawing from the engine instead."
                .into(),
        ));
    }
    let dir = app
        .project_dir()
        .await
        .ok_or_else(|| ToolFail("no_document: no project is open".into()))?;
    match store::thumbnail_data_url(&dir) {
        Some(url) => Ok(Output::Rich {
            json: json!({"source": "thumbnail", "note": "The window is not open, so this is the last plan thumbnail it saved. It can be older than the current revision."}),
            images: vec![picture_from_data_url(&url)?],
        }),
        None => Err(ToolFail(
            "not_found: the Guhit Studio window is not open and this project has no saved plan thumbnail yet. Use export_plan with format \"svg\" to get a drawing from the engine instead."
                .into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(w, h, image::Rgb([200, 220, 240]));
        let mut out = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn a_small_picture_passes_unchanged_and_a_big_one_is_fitted() {
        let small = png(400, 300);
        let p = fit_picture(&small, "image/png").unwrap();
        assert_eq!(p.mime, "image/png");
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(&p.base64).unwrap(), small);

        let big = png(3840, 2160);
        let p = fit_picture(&big, "image/png").unwrap();
        assert_eq!(p.mime, "image/jpeg");
        let back = image::load_from_memory(&base64::engine::general_purpose::STANDARD.decode(&p.base64).unwrap()).unwrap();
        assert_eq!((back.width(), back.height()), (1568, 882));
    }

    #[test]
    fn data_urls_are_split_and_checked() {
        let p = picture_from_data_url("data:image/png;base64,AAAA").unwrap();
        assert_eq!((p.mime.as_str(), p.base64.as_str()), ("image/png", "AAAA"));
        assert!(picture_from_data_url("data:text/html;base64,AAAA").is_err());
        assert!(picture_from_data_url("data:image/png,AAAA").is_err());
        assert!(picture_from_data_url("http://example.com/a.png").is_err());
    }

    #[test]
    fn views_are_found_by_id_or_name() {
        let mut project = defaults::new_project("t");
        project.elements.push(Element::Camera(Camera {
            id: "cam-1".into(),
            name: "Living room".into(),
            preset: CameraPreset::Custom,
            position: Vec3 { x: 0.0, y: 0.0, z: 1600.0 },
            target: Vec3 { x: 1000.0, y: 0.0, z: 1600.0 },
            fov_deg: 60.0,
            light: None,
        }));
        assert_eq!(find_view(&project, "cam-1").unwrap(), "cam-1");
        assert_eq!(find_view(&project, "  living ROOM ").unwrap(), "cam-1");
        assert!(find_view(&project, "Kitchen").unwrap_err().0.contains("\"Living room\" (cam-1)"));
    }
}
