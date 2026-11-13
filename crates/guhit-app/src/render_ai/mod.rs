//! Tier 2 AI visualization (DECISIONS D17), AI render v2 (D53, lane L6).
//!
//! A saved Tier 1 capture plus a style prompt goes to a hosted image model,
//! and the photorealistic answer comes back as a second `RenderRecord` whose
//! `source_render_id` points at the capture it was conditioned on. The UI can
//! then always show the two side by side.
//!
//! AI render v2 adds what the model knows and a screenshot does not:
//! - The capture's conditioning maps (line drawing and depth, drawn by the
//!   window, `renders::save_maps`) go with it, and the text says what each
//!   image is.
//! - Model facts from Rust (`facts.rs`): storeys, roofs, walls, the rooms,
//!   doors, windows and furniture in view with their finishes, the time of
//!   day, and "do not move, add or remove walls, doors or windows".
//! - The project look (`renders::set_look`): one approved image goes with
//!   every later render as its style reference, so a project keeps one look.
//! - The drift check (`drift.rs`): the answer's edges are compared with the
//!   model's line map. A person sees the warning before the image is saved
//!   (`render_ai_draft`, then `render_ai_keep` or `render_ai_discard`); an
//!   agent's image is saved with its `DriftReport`.
//! - Cost (D53): every call has an estimate the window shows first
//!   (`render_ai_estimate`); the Pro model runs only for High or with a style
//!   reference; agents get `DEFAULT_AGENT_CAP` images per project per day,
//!   which people can raise (`render_ai_usage_set`).
//!
//! Rules this module enforces:
//! - Only a `ModelView` capture can be a source. An AI image is never the
//!   source of another AI image, so every visualization is one step from real
//!   geometry. The project look is a style reference only: its materials and
//!   light, never its shapes.
//! - The generated image never writes back into the model. It is a file and a
//!   record, nothing else (AGENTS.md: geometry is authoritative, AI imagery is
//!   derivative).
//! - The API key lives in a private file, never crosses IPC and is never
//!   logged. `RenderAiSettings` carries `has_api_key` only.
//! - A failed generation writes no record and leaves no file behind.

pub mod drift;
pub mod facts;
pub mod gemini;
pub mod keys;
pub mod prompt;
pub mod provider;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use guhit_model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::ai::log;
use crate::files::{self, ImageKind};
use crate::renders::{self, MapsMeta};
use crate::{arg, to_value, AppService, IpcResult};
use gemini::GeminiProvider;
use keys::{FileStore, RenderKeyStore};
use provider::{DevFakeProvider, ImageProvider, InputImage, RenderInput};

/// What `render_ai_usage`, `render_ai_estimate` and `render_ai_draft`
/// answer. The types live in the contract (guhit-model).
pub use guhit_model::{AgentUsage, Draft, Estimate};

/// Commands this module answers. `AppService::handle` routes on this list.
pub const OWNS: &[&str] = &[
    "render_ai_settings_get",
    "render_ai_settings_set",
    "render_ai_generate",
    // AI render v2 (lane L6).
    "render_ai_estimate",
    "render_ai_draft",
    "render_ai_keep",
    "render_ai_discard",
    "render_ai_maps_save",
    "render_ai_usage",
    "render_ai_usage_set",
    "render_ai_drift_overlay",
    "render_ai_map_data",
];

const SETTINGS_FILE: &str = "settings.json";
const SETTINGS_MODEL_KEY: &str = "render_ai_model";
/// settings.json: AI images agents may make per project per day.
pub const SETTINGS_AGENT_CAP_KEY: &str = "render_ai_agent_daily_cap";
/// Longest prompt kept in a log line.
const LOG_PROMPT_CHARS: usize = 200;

/// AI images agents may make per project per day unless a person raises it
/// (DECISIONS D53).
pub const DEFAULT_AGENT_CAP: u32 = 20;
/// The highest cap a person can set.
pub const MAX_AGENT_CAP: u32 = 1000;
/// Agents' AI images per day, in the project's renders folder. A bundle
/// never brings one in (`interop::bundle_open`): the count is this
/// computer's.
pub(crate) const USAGE_FILE: &str = "ai-usage.json";

/// The maps that go to the provider with the capture, in this order. The
/// others (normals, ids, clay) stay home: ids feed the drift check and the
/// facts, clay and normals the maps preview.
pub const SENT_MAPS: [MapKind; 2] = [MapKind::Lines, MapKind::Depth];
/// Longest side of the style reference sent to the provider, pixels.
const STYLE_MAX_PX: u32 = 1536;
/// How long an agent's call waits for the window to draw missing maps.
const MAPS_TIMEOUT: Duration = Duration::from_secs(45);
/// Drafts not kept or discarded within this long are dropped.
const DRAFT_TTL: Duration = Duration::from_secs(30 * 60);
/// At most this many drafts wait at once; the oldest goes first.
const MAX_DRAFTS: usize = 4;

pub struct RenderAiState {
    keys: Box<dyn RenderKeyStore>,
    /// Set by tests. When None a real `GeminiProvider` is built per request.
    provider: Option<Arc<dyn ImageProvider>>,
    /// One generation at a time. Images are slow and expensive; two at once
    /// would only ever be an accidental double click.
    gate: Mutex<()>,
    /// Images a person has not kept or discarded yet (`render_ai_draft`).
    drafts: std::sync::Mutex<Vec<Pending>>,
}

impl Default for RenderAiState {
    fn default() -> Self {
        Self {
            keys: Box::new(keys::MemoryKeyStore::default()),
            provider: None,
            gate: Mutex::new(()),
            drafts: std::sync::Mutex::new(vec![]),
        }
    }
}

impl RenderAiState {
    /// Production state: key in `<data_dir>/render-api-key`, real provider.
    /// A debug build with `GUHIT_RENDER_AI_FAKE` set uses the dev provider,
    /// which never leaves the computer (`provider::DevFakeProvider`).
    pub fn for_data_dir(data_dir: &std::path::Path) -> Self {
        let provider = DevFakeProvider::from_env().map(|p| Arc::new(p) as Arc<dyn ImageProvider>);
        Self { keys: Box::new(FileStore::new(data_dir)), provider, ..Self::default() }
    }

    /// State with an injected provider and key store, for tests.
    pub fn with_parts(provider: Arc<dyn ImageProvider>, keys: Box<dyn RenderKeyStore>) -> Self {
        Self { keys, provider: Some(provider), ..Self::default() }
    }

    /// State with an injected key store and the real provider.
    pub fn with_keys(keys: Box<dyn RenderKeyStore>) -> Self {
        Self { keys, ..Self::default() }
    }
}

/// One maps file as the window sends it.
#[derive(Debug, Clone, Deserialize)]
struct MapArg {
    kind: MapKind,
    /// PNG data URL.
    png: String,
}

/// Handles `render_ai_*`.
pub async fn handle(app: &AppService, cmd: &str, args: Value) -> IpcResult {
    match cmd {
        "render_ai_settings_get" => to_value(&settings_get(app).await),
        "render_ai_settings_set" => {
            let api_key: Option<String> = arg(&args, "api_key")?;
            let model: Option<String> = arg(&args, "model")?;
            to_value(&settings_set(app, api_key, model).await?)
        }
        "render_ai_generate" => {
            let request: RenderAiRequest = arg(&args, "request")?;
            // Set by the MCP server for an agent's call: the daily cap applies.
            let agent: Option<String> = arg(&args, "agent")?;
            to_value(&generate(app, request, agent.as_deref()).await?)
        }
        "render_ai_estimate" => {
            let request: RenderAiRequest = arg(&args, "request")?;
            let agent: Option<String> = arg(&args, "agent")?;
            to_value(&estimate(app, &request, agent.as_deref()).await?)
        }
        "render_ai_draft" => {
            let request: RenderAiRequest = arg(&args, "request")?;
            to_value(&draft(app, request).await?)
        }
        "render_ai_keep" => {
            let id: String = arg(&args, "pending_id")?;
            to_value(&keep(app, &id).await?)
        }
        "render_ai_discard" => {
            let id: String = arg(&args, "pending_id")?;
            discard(app, &id).await?;
            Ok(Value::Null)
        }
        "render_ai_maps_save" => {
            let render_id: String = arg(&args, "render_id")?;
            let maps: Vec<MapArg> = arg(&args, "maps")?;
            let meta: Option<MapsMeta> = arg(&args, "meta")?;
            let mut decoded = Vec::with_capacity(maps.len());
            for m in maps {
                let png = files::decode_any_data_url(&m.png, renders::MAX_MAP_BYTES)?;
                decoded.push((m.kind, png));
            }
            // Under the session lock, as every write to the renders index.
            let s = app.session.lock().await;
            let dir = s.open_project_dir()?;
            to_value(&renders::save_maps(&dir, &render_id, &decoded, meta.as_ref())?)
        }
        "render_ai_usage" => to_value(&usage(app).await?),
        "render_ai_usage_set" => {
            let cap: u32 = arg(&args, "agent_daily_cap")?;
            to_value(&usage_set(app, cap).await?)
        }
        "render_ai_drift_overlay" => {
            let id: String = arg(&args, "id")?;
            let dir = project_dir(app).await?;
            to_value(&renders::drift_overlay(&dir, &id)?)
        }
        "render_ai_map_data" => {
            let id: String = arg(&args, "id")?;
            let kind: MapKind = arg(&args, "kind")?;
            let dir = project_dir(app).await?;
            match renders::read_map(&dir, &id, kind)? {
                Some(png) => to_value(&files::encode_data_url(ImageKind::Png, &png)),
                None => Err(IpcError::new("not_found", format!("render {id} has no {} map", renders::map_key(kind)))),
            }
        }
        _ => Err(IpcError::new("unknown_command", format!("unknown command `{cmd}`"))),
    }
}

// ------------------------------------------------------------------- settings

async fn data_dir(app: &AppService) -> PathBuf {
    app.session.lock().await.data_dir.clone()
}

async fn project_dir(app: &AppService) -> Result<PathBuf, IpcError> {
    let s = app.session.lock().await;
    s.open_project_dir()
}

fn read_settings(dir: &std::path::Path) -> serde_json::Map<String, Value> {
    std::fs::read_to_string(dir.join(SETTINGS_FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// The `render_ai_model` setting, or "" for "let the quality pick".
async fn model_setting(app: &AppService) -> String {
    read_settings(&data_dir(app).await)
        .get(SETTINGS_MODEL_KEY)
        .and_then(Value::as_str)
        .map(|m| m.trim().to_string())
        .unwrap_or_default()
}

/// Plain wording of what an image costs at the current setting, for the UI.
fn cost_hint(model_setting: &str) -> String {
    let price = |q| gemini::resolve(model_setting, q, false).price_usd();
    format!(
        "About ${} per draft, ${} per standard, ${} per high image (Google list price)",
        price(RenderQuality::Draft),
        price(RenderQuality::Standard),
        price(RenderQuality::High),
    )
}

async fn settings_get(app: &AppService) -> RenderAiSettings {
    let model = model_setting(app).await;
    RenderAiSettings {
        provider: gemini::PROVIDER.to_string(),
        has_api_key: app.render_ai.keys.get().is_some(),
        cost_hint: cost_hint(&model),
        model,
    }
}

async fn settings_set(
    app: &AppService,
    api_key: Option<String>,
    model: Option<String>,
) -> Result<RenderAiSettings, IpcError> {
    if let Some(key) = api_key {
        let key = key.trim();
        if key.is_empty() {
            app.render_ai.keys.remove()?;
        } else {
            app.render_ai.keys.set(key)?;
        }
    }
    if let Some(model) = model {
        let model = gemini::validate_model_setting(&model)?;
        let dir = data_dir(app).await;
        // Merge: the copilot keeps its own keys in this same file.
        let mut settings = read_settings(&dir);
        if model.is_empty() {
            settings.remove(SETTINGS_MODEL_KEY);
        } else {
            settings.insert(SETTINGS_MODEL_KEY.into(), json!(model));
        }
        let text = serde_json::to_string_pretty(&Value::Object(settings))
            .map_err(|e| IpcError::new("io", e.to_string()))?;
        std::fs::create_dir_all(&dir).map_err(|e| IpcError::new("io", e.to_string()))?;
        // Atomic, with a unique temp name so two writers cannot collide.
        let tmp = dir.join(format!(".{SETTINGS_FILE}.{}.tmp", defaults::new_id()));
        std::fs::write(&tmp, text).map_err(|e| IpcError::new("io", e.to_string()))?;
        std::fs::rename(&tmp, dir.join(SETTINGS_FILE)).map_err(|e| IpcError::new("io", e.to_string()))?;
    }
    Ok(settings_get(app).await)
}

// ------------------------------------------------------------- agents' cap

#[derive(Debug, Default, Serialize, Deserialize)]
struct UsageFile {
    day: String,
    agent_images: u32,
}

fn usage_path(project_dir: &Path) -> PathBuf {
    project_dir.join("renders").join(USAGE_FILE)
}

fn read_usage(project_dir: &Path, today: &str) -> u32 {
    std::fs::read(usage_path(project_dir))
        .ok()
        .and_then(|b| serde_json::from_slice::<UsageFile>(&b).ok())
        .filter(|u| u.day == today)
        .map(|u| u.agent_images)
        .unwrap_or(0)
}

fn agent_cap(data_dir: &Path) -> u32 {
    read_settings(data_dir)
        .get(SETTINGS_AGENT_CAP_KEY)
        .and_then(Value::as_u64)
        .map(|v| v.min(MAX_AGENT_CAP as u64) as u32)
        .unwrap_or(DEFAULT_AGENT_CAP)
}

async fn usage(app: &AppService) -> Result<AgentUsage, IpcError> {
    let dir = project_dir(app).await?;
    let day = crate::local_date();
    Ok(AgentUsage {
        used: read_usage(&dir, &day),
        day,
        cap: agent_cap(&data_dir(app).await),
        default_cap: DEFAULT_AGENT_CAP,
    })
}

/// A person raises (or lowers) the cap. 0 keeps agents from making AI
/// images at all.
async fn usage_set(app: &AppService, cap: u32) -> Result<AgentUsage, IpcError> {
    if cap > MAX_AGENT_CAP {
        return Err(IpcError::new(
            "bad_args",
            format!("argument `agent_daily_cap`: at most {MAX_AGENT_CAP} AI images a day"),
        ));
    }
    let value = if cap == DEFAULT_AGENT_CAP { None } else { Some(json!(cap)) };
    files::put_setting(&data_dir(app).await, SETTINGS_AGENT_CAP_KEY, value)?;
    usage(app).await
}

fn count_agent_image(project_dir: &Path) -> Result<(), IpcError> {
    let day = crate::local_date();
    let used = read_usage(project_dir, &day);
    files::create_dir(&project_dir.join("renders"))?;
    files::write_json_atomic(&usage_path(project_dir), &UsageFile { day, agent_images: used + 1 })
}

/// Refuses an agent's call over the cap, with what a person can do about it.
fn check_cap(u: &AgentUsage) -> Result<(), IpcError> {
    if u.used < u.cap {
        return Ok(());
    }
    let message = if u.cap == 0 {
        "Agents may not make AI images in this project: the daily limit is 0. Ask the user to raise \"AI images per day for agents\" in the Visuals panel.".to_string()
    } else {
        format!(
            "Agents have made {} of {} AI images in this project today, the daily limit for agents. Each one costs the user money. Ask the user to raise \"AI images per day for agents\" in the Visuals panel, or try again tomorrow.",
            u.used, u.cap
        )
    };
    Err(IpcError::new("rate_limited", message))
}

// ------------------------------------------------------------------ estimate

/// Everything one call needs, worked out before any money is spent.
struct Prepared {
    project_dir: PathBuf,
    source: RenderRecord,
    input: RenderInput,
    estimate: Estimate,
    index: facts::ElementIndex,
    style_reference_id: Option<Id>,
}

/// The style reference for a request: the one it names, none for "" or
/// "none", else the project look.
fn style_reference(project_dir: &Path, request: &RenderAiRequest) -> Result<Option<RenderRecord>, IpcError> {
    match request.style_reference_id.as_deref().map(str::trim) {
        Some("") | Some("none") => Ok(None),
        Some(id) => {
            let r = renders::find(project_dir, id)?
                .ok_or_else(|| IpcError::new("not_found", format!("no saved image with id {id} to use as the style reference")))?;
            if r.video.is_some() {
                return Err(IpcError::new("invalid", "A tour video cannot be a style reference. Pick a still image."));
            }
            Ok(Some(r))
        }
        None => renders::look(project_dir),
    }
}

/// The style reference image, at most `STYLE_MAX_PX` on its long side, JPEG:
/// it carries materials and light, which need no more.
fn style_image(project_dir: &Path, id: &str) -> Result<InputImage, IpcError> {
    let (kind, bytes) = renders::read_image(project_dir, id)?;
    let Ok(img) = image::load_from_memory(&bytes) else {
        return Ok(InputImage::new("style", kind, bytes));
    };
    let img = if img.width().max(img.height()) > STYLE_MAX_PX {
        img.resize(STYLE_MAX_PX, STYLE_MAX_PX, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut out = std::io::Cursor::new(Vec::new());
    match image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88).encode_image(&img.to_rgb8()) {
        Ok(()) => Ok(InputImage::new("style", ImageKind::Jpeg, out.into_inner())),
        Err(_) => Ok(InputImage::new("style", kind, bytes)),
    }
}

fn map_role(kind: MapKind) -> &'static str {
    renders::map_key(kind)
}

/// Works out the call: the source, its maps, the style reference, the
/// facts, the text, the model and its price. Spends nothing.
async fn prepare(app: &AppService, request: &RenderAiRequest) -> Result<Prepared, IpcError> {
    let project_dir = project_dir(app).await?;

    // The source must exist and must be a model view. An AI image is never
    // the source of another AI image.
    let source = renders::find(&project_dir, &request.source_render_id)?.ok_or_else(|| {
        IpcError::new("invalid", format!("no saved view with id {} to visualize", request.source_render_id))
    })?;
    if source.source != RenderSource::ModelView || source.video.is_some() {
        return Err(IpcError::new(
            "invalid",
            "Only a model view can be visualized. Pick the original 3D capture, not an AI image.",
        ));
    }
    let (source_kind, png) = renders::read_image(&project_dir, &source.id)?;
    if source_kind != ImageKind::Png {
        return Err(IpcError::new("invalid", "the source capture is not a PNG"));
    }

    let reference = style_reference(&project_dir, request)?;
    let style = match &reference {
        Some(r) => Some(style_image(&project_dir, &r.id)?),
        None => None,
    };

    let mut maps = vec![];
    if request.use_maps {
        for kind in SENT_MAPS {
            if let Some(bytes) = renders::read_map(&project_dir, &source.id, kind)? {
                maps.push((kind, InputImage::new(map_role(kind), ImageKind::Png, bytes)));
            }
        }
    }
    let maps_missing = request.use_maps && maps.is_empty();

    // Facts from the model, and what is in view from the ids map.
    let meta = renders::read_maps_meta(&project_dir, &source.id);
    let in_view: Option<(Vec<(Id, u64)>, u64)> = meta.as_ref().filter(|m| !m.legend.is_empty()).map(|m| {
        let seen = m.legend.iter().filter(|e| !e.element_id.is_empty()).map(|e| (e.element_id.clone(), e.pixels)).collect();
        (seen, (m.width as u64 * m.height as u64).max(1))
    });
    let (fact_text, index) = {
        let s = app.session.lock().await;
        let doc = s.open_doc()?;
        let text = facts::describe(&facts::FactsInput {
            project: doc.project(),
            derived: doc.derived(),
            camera: &source.camera,
            in_view: in_view.as_ref().map(|(v, total)| (v.as_slice(), *total)),
            keep_geometry: request.keep_geometry,
        });
        (text, facts::ElementIndex::new(doc.project(), doc.derived()))
    };

    let mut roles: Vec<&str> = maps.iter().map(|(k, _)| map_role(*k)).collect();
    if style.is_some() {
        roles.push("style");
    }
    let text = prompt::build_with(&prompt::Parts {
        style_key: request.style_key.as_deref(),
        user_text: &request.prompt,
        keep_geometry: request.keep_geometry,
        facts: Some(&fact_text),
        images: &roles,
    })?;

    let choice = gemini::resolve(&model_setting(app).await, request.quality, style.is_some());
    let agents = usage(app).await?;
    let estimate = Estimate {
        model: choice.model.clone(),
        model_label: choice.label(),
        image_size: choice.image_size.to_string(),
        price_usd: choice.price_usd().to_string(),
        why: choice.why.to_string(),
        pro: choice.family == gemini::Family::Pro,
        style_reference_id: reference.as_ref().map(|r| r.id.clone()),
        maps: maps.iter().map(|(k, _)| *k).collect(),
        maps_missing,
        prompt: text.clone(),
        agents,
    };
    let input = RenderInput {
        aspect_ratio: provider::aspect_ratio_for(&png),
        png,
        constraint: if request.keep_geometry { prompt::GEOMETRY_LINE.to_string() } else { String::new() },
        prompt: text,
        quality: request.quality,
        maps: maps.into_iter().map(|(_, m)| m).collect(),
        style_reference: style,
    };
    Ok(Prepared {
        project_dir,
        style_reference_id: reference.map(|r| r.id),
        source,
        input,
        estimate,
        index,
    })
}

/// The estimate for a request, as the window shows it before the call, and
/// as the MCP server tells an agent before it spends anything. `agents`
/// says how many of today's agent images are left in this project. `agent`
/// is accepted for symmetry with `render_ai_generate`; the cap is per
/// project, so it changes nothing.
pub async fn estimate(app: &AppService, request: &RenderAiRequest, agent: Option<&str>) -> Result<Estimate, IpcError> {
    let _ = agent;
    Ok(prepare(app, request).await?.estimate)
}

// ------------------------------------------------------------------- generate

/// The provider for one request. A test provider wins; otherwise a real
/// Gemini client is built from the stored key.
async fn pick_provider(app: &AppService) -> Result<Arc<dyn ImageProvider>, IpcError> {
    if let Some(p) = &app.render_ai.provider {
        return Ok(p.clone());
    }
    let key = app.render_ai.keys.get().ok_or_else(|| {
        IpcError::new(
            "ai_not_configured",
            "No Google AI API key is set. Add one in the visualization settings; create a key at aistudio.google.com.",
        )
    })?;
    let model = model_setting(app).await;
    Ok(Arc::new(GeminiProvider::new(key, model)?))
}

fn short_prompt(prompt: &str) -> String {
    if prompt.chars().count() <= LOG_PROMPT_CHARS {
        return prompt.to_string();
    }
    let cut: String = prompt.chars().take(LOG_PROMPT_CHARS).collect();
    format!("{cut}...")
}

/// What came back, checked.
struct Generated {
    output: provider::RenderOutput,
    drift: Option<DriftReport>,
    overlay: Option<Vec<u8>>,
}

/// The drift check of an answer against the source's line map. None when
/// the source has no line map, or the image cannot be read.
async fn check_drift(p: &Prepared, bytes: &[u8]) -> (Option<DriftReport>, Option<Vec<u8>>) {
    let Ok(Some(lines)) = renders::read_map(&p.project_dir, &p.source.id, MapKind::Lines) else {
        return (None, None);
    };
    let ids = renders::read_map(&p.project_dir, &p.source.id, MapKind::Ids).ok().flatten();
    let legend = renders::read_maps_meta(&p.project_dir, &p.source.id).map(|m| m.legend).unwrap_or_default();
    let weights: std::collections::HashMap<Id, drift::Weight> =
        legend.iter().filter_map(|e| p.index.weight(&e.element_id).map(|w| (e.element_id.clone(), w))).collect();
    let bytes = bytes.to_vec();
    // CPU work: off the async runtime.
    let outcome = tokio::task::spawn_blocking(move || {
        let ids_ref = ids.as_deref().filter(|_| !legend.is_empty()).map(|png| (png, legend.as_slice()));
        drift::check(&drift::Check { lines_png: &lines, ids: ids_ref, ai: &bytes }, &|id| weights.get(id).copied())
    })
    .await;
    match outcome {
        Ok(Ok(o)) => {
            let report = drift::report(&o, &|id| p.index.named(id));
            let overlay = (o.missing > 0.0).then_some(o.overlay);
            (Some(report), overlay)
        }
        Ok(Err(e)) => {
            eprintln!("guhit-app: drift check skipped: {e}");
            (None, None)
        }
        Err(_) => (None, None),
    }
}

/// Calls the provider and checks the answer. Logs a failure.
async fn run(app: &AppService, p: &Prepared, request: &RenderAiRequest, agent: Option<&str>) -> Result<Generated, IpcError> {
    let provider = pick_provider(app).await?;
    match provider.render(p.input.clone()).await {
        Ok(output) => {
            let (drift, overlay) = check_drift(p, &output.bytes).await;
            Ok(Generated { output, drift, overlay })
        }
        Err(e) => {
            let e: IpcError = e.into();
            log::append(
                Some(&p.project_dir),
                "render",
                json!({
                    "model": null,
                    "seconds": 0.0,
                    "bytes": 0,
                    "outcome": "error",
                    "error": e.message,
                    "source_render_id": p.source.id,
                    "quality": request.quality,
                    "style_key": request.style_key,
                    "style_reference_id": p.style_reference_id,
                    "maps": p.estimate.maps,
                    "price_usd": p.estimate.price_usd,
                    "agent": agent,
                    "prompt": short_prompt(&p.input.prompt),
                }),
            )
            .await;
            Err(e)
        }
    }
}

fn log_fields(p: &Prepared, g: &Generated, request: &RenderAiRequest, agent: Option<&str>, outcome: &str) -> Value {
    json!({
        "model": g.output.model,
        "seconds": (g.output.seconds * 100.0).round() / 100.0,
        "bytes": g.output.bytes.len(),
        "outcome": outcome,
        "source_render_id": p.source.id,
        "quality": request.quality,
        "style_key": request.style_key,
        "style_reference_id": p.style_reference_id,
        "maps": p.estimate.maps,
        "image_size": p.estimate.image_size,
        "price_usd": p.estimate.price_usd,
        "drift_missing": g.drift.as_ref().map(|d| d.missing),
        "drift_warn": g.drift.as_ref().map(|d| d.warn),
        "agent": agent,
        "prompt": short_prompt(&p.input.prompt),
    })
}

/// Ask the window to draw a capture's maps when an agent's call needs them
/// and none are saved yet. Best effort: without a window the image goes
/// without maps and without a drift check.
async fn ensure_maps(app: &AppService, request: &RenderAiRequest) {
    if !request.use_maps || app.event_listeners() == 0 {
        return;
    }
    let Ok(dir) = project_dir(app).await else { return };
    if renders::read_map(&dir, &request.source_render_id, MapKind::Lines).ok().flatten().is_some() {
        return;
    }
    let task = WindowTask::CaptureMaps { render_id: request.source_render_id.clone() };
    if let Err(e) = app.window_request(task, MAPS_TIMEOUT).await {
        eprintln!("guhit-app: no conditioning maps for {}: {}", request.source_render_id, e.message);
    }
}

/// `render_ai_generate`: make the image and save it with its drift report.
/// The path agents take (through MCP, `agent` set) and older callers.
async fn generate(app: &AppService, request: RenderAiRequest, agent: Option<&str>) -> Result<RenderAiResult, IpcError> {
    // One at a time, and never while the session lock is held: the provider
    // call can take a minute and the UI must stay responsive.
    let _turn = app.render_ai.gate.lock().await;
    if agent.is_some() {
        check_cap(&usage(app).await?)?;
        ensure_maps(app, &request).await;
    }
    let p = prepare(app, &request).await?;
    let g = run(app, &p, &request, agent).await?;

    // The provider is done: the save takes the session lock, as every write
    // to the renders index does.
    let record = {
        let _s = app.session.lock().await;
        let record = renders::save_ai_checked(
            &p.project_dir,
            &p.source,
            g.output.kind,
            &g.output.bytes,
            request.style_key.clone(),
            p.input.prompt.clone(),
            &format!("{}/{}", gemini::PROVIDER, g.output.model),
            g.drift.clone(),
            g.overlay.as_deref(),
        )?;
        if agent.is_some() {
            count_agent_image(&p.project_dir)?;
        }
        record
    };
    let mut fields = log_fields(&p, &g, &request, agent, "ok");
    fields["render_id"] = json!(record.id);
    log::append(Some(&p.project_dir), "render", fields).await;
    Ok(RenderAiResult { record, seconds: g.output.seconds })
}

// --------------------------------------------------------- drafts (people)

/// An image a person has not kept yet.
struct Pending {
    id: Id,
    project_dir: PathBuf,
    source: RenderRecord,
    kind: ImageKind,
    bytes: Vec<u8>,
    style_key: Option<String>,
    prompt: String,
    provider: String,
    drift: Option<DriftReport>,
    overlay: Option<Vec<u8>>,
    made: Instant,
}

fn drafts_lock(app: &AppService) -> std::sync::MutexGuard<'_, Vec<Pending>> {
    app.render_ai.drafts.lock().unwrap_or_else(|e| e.into_inner())
}

/// `render_ai_draft`: a person's render. The image comes back with its
/// drift check and is saved only when they keep it, so a warning comes
/// before anything lands in Visuals.
async fn draft(app: &AppService, request: RenderAiRequest) -> Result<Draft, IpcError> {
    let _turn = app.render_ai.gate.lock().await;
    let p = prepare(app, &request).await?;
    let g = run(app, &p, &request, None).await?;
    let id = defaults::new_id();
    let provider = format!("{}/{}", gemini::PROVIDER, g.output.model);
    let out = Draft {
        pending_id: id.clone(),
        image: files::encode_data_url(g.output.kind, &g.output.bytes),
        drift: g.drift.clone(),
        overlay: g.overlay.as_deref().map(|png| files::encode_data_url(ImageKind::Png, png)),
        estimate: p.estimate.clone(),
        seconds: g.output.seconds,
        provider: provider.clone(),
    };
    let mut fields = log_fields(&p, &g, &request, None, "held");
    fields["pending_id"] = json!(id);
    log::append(Some(&p.project_dir), "render", fields).await;
    let mut drafts = drafts_lock(app);
    drafts.retain(|d| d.made.elapsed() < DRAFT_TTL);
    while drafts.len() >= MAX_DRAFTS {
        drafts.remove(0);
    }
    drafts.push(Pending {
        id,
        project_dir: p.project_dir,
        source: p.source,
        kind: g.output.kind,
        bytes: g.output.bytes,
        style_key: request.style_key,
        prompt: p.input.prompt,
        provider,
        drift: g.drift,
        overlay: g.overlay,
        made: Instant::now(),
    });
    Ok(out)
}

fn take_draft(app: &AppService, id: &str) -> Result<Pending, IpcError> {
    let mut drafts = drafts_lock(app);
    drafts.retain(|d| d.made.elapsed() < DRAFT_TTL);
    let at = drafts.iter().position(|d| d.id == id).ok_or_else(|| {
        IpcError::new("not_found", "That AI image is gone: drafts are kept for 30 minutes. Render it again.")
    })?;
    Ok(drafts.remove(at))
}

/// `render_ai_keep`: save a draft to Visuals with its drift report.
async fn keep(app: &AppService, id: &str) -> Result<RenderRecord, IpcError> {
    let d = take_draft(app, id)?;
    let record = {
        // Under the session lock, as every write to the renders index.
        let s = app.session.lock().await;
        // A draft belongs to the project it was made in.
        if s.open_project_dir()? != d.project_dir {
            return Err(IpcError::new("invalid", "That AI image belongs to another project."));
        }
        renders::save_ai_checked(
            &d.project_dir,
            &d.source,
            d.kind,
            &d.bytes,
            d.style_key.clone(),
            d.prompt.clone(),
            &d.provider,
            d.drift.clone(),
            d.overlay.as_deref(),
        )?
    };
    log::append(Some(&d.project_dir), "render_keep", json!({"pending_id": id, "render_id": record.id})).await;
    Ok(record)
}

/// `render_ai_discard`: drop a draft. Nothing was saved.
async fn discard(app: &AppService, id: &str) -> Result<(), IpcError> {
    let d = take_draft(app, id)?;
    log::append(Some(&d.project_dir), "render_discard", json!({"pending_id": id})).await;
    Ok(())
}
