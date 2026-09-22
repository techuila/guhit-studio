//! Tier 2 AI visualization (DECISIONS D17).
//!
//! A saved Tier 1 capture plus a style prompt goes to a hosted image model,
//! and the photorealistic answer comes back as a second `RenderRecord` whose
//! `source_render_id` points at the capture it was conditioned on. The UI can
//! then always show the two side by side.
//!
//! Rules this module enforces:
//! - Only a `ModelView` capture can be a source. An AI image is never the
//!   input to another AI image, so every visualization is one step from real
//!   geometry.
//! - The generated image never writes back into the model. It is a file and a
//!   record, nothing else (AGENTS.md: geometry is authoritative, AI imagery is
//!   derivative).
//! - The API key lives in a private file, never crosses IPC and is never
//!   logged. `RenderAiSettings` carries `has_api_key` only.
//! - A failed generation writes no record and leaves no file behind.

pub mod gemini;
pub mod keys;
pub mod prompt;
pub mod provider;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;

use guhit_model::*;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::ai::log;
use crate::{arg, renders, to_value, AppService, IpcResult};
use gemini::GeminiProvider;
use keys::{FileStore, RenderKeyStore};
use provider::{ImageProvider, RenderInput};

/// Commands this module answers. `AppService::handle` routes on this list.
pub const OWNS: &[&str] = &["render_ai_settings_get", "render_ai_settings_set", "render_ai_generate"];

const SETTINGS_FILE: &str = "settings.json";
const SETTINGS_MODEL_KEY: &str = "render_ai_model";
/// Longest prompt kept in a log line.
const LOG_PROMPT_CHARS: usize = 200;

pub struct RenderAiState {
    keys: Box<dyn RenderKeyStore>,
    /// Set by tests. When None a real `GeminiProvider` is built per request.
    provider: Option<Arc<dyn ImageProvider>>,
    /// One generation at a time. Images are slow and expensive; two at once
    /// would only ever be an accidental double click.
    gate: Mutex<()>,
}

impl Default for RenderAiState {
    fn default() -> Self {
        Self { keys: Box::new(keys::MemoryKeyStore::default()), provider: None, gate: Mutex::new(()) }
    }
}

impl RenderAiState {
    /// Production state: key in `<data_dir>/render-api-key`, real provider.
    pub fn for_data_dir(data_dir: &std::path::Path) -> Self {
        Self { keys: Box::new(FileStore::new(data_dir)), ..Self::default() }
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
            to_value(&generate(app, request).await?)
        }
        _ => Err(IpcError::new("unknown_command", format!("unknown command `{cmd}`"))),
    }
}

// ------------------------------------------------------------------- settings

async fn data_dir(app: &AppService) -> PathBuf {
    app.session.lock().await.data_dir.clone()
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
    let price = |q| gemini::resolve(model_setting, q).price_usd();
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

async fn generate(app: &AppService, request: RenderAiRequest) -> Result<RenderAiResult, IpcError> {
    // One at a time, and never while the session lock is held: the provider
    // call can take a minute and the UI must stay responsive.
    let _turn = app.render_ai.gate.lock().await;

    let project_dir = {
        let s = app.session.lock().await;
        s.open_project_dir()?
    };

    // The source must exist and must be a model view. An AI image is never
    // the input to another AI image.
    let source = renders::find(&project_dir, &request.source_render_id)?.ok_or_else(|| {
        IpcError::new(
            "invalid",
            format!("no saved view with id {} to visualize", request.source_render_id),
        )
    })?;
    if source.source != RenderSource::ModelView {
        return Err(IpcError::new(
            "invalid",
            "Only a model view can be visualized. Pick the original 3D capture, not an AI image.",
        ));
    }
    let (source_kind, png) = renders::read_image(&project_dir, &source.id)?;
    if source_kind != crate::files::ImageKind::Png {
        return Err(IpcError::new("invalid", "the source capture is not a PNG"));
    }

    let text = prompt::build(request.style_key.as_deref(), &request.prompt, request.keep_geometry)?;
    let provider = pick_provider(app).await?;
    let input = RenderInput {
        aspect_ratio: provider::aspect_ratio_for(&png),
        png,
        constraint: if request.keep_geometry { prompt::GEOMETRY_LINE.to_string() } else { String::new() },
        prompt: text.clone(),
        quality: request.quality,
    };

    let outcome = provider.render(input).await;
    let output = match outcome {
        Ok(output) => output,
        Err(e) => {
            let e: IpcError = e.into();
            log::append(
                Some(&project_dir),
                "render",
                json!({
                    "model": null,
                    "seconds": 0.0,
                    "bytes": 0,
                    "outcome": "error",
                    "error": e.message,
                    "source_render_id": source.id,
                    "quality": request.quality,
                    "style_key": request.style_key,
                    "prompt": short_prompt(&text),
                }),
            )
            .await;
            return Err(e);
        }
    };

    let record = renders::save_ai(
        &project_dir,
        &source,
        output.kind,
        &output.bytes,
        request.style_key.clone(),
        text.clone(),
        &format!("{}/{}", gemini::PROVIDER, output.model),
    )?;

    log::append(
        Some(&project_dir),
        "render",
        json!({
            "model": output.model,
            "seconds": (output.seconds * 100.0).round() / 100.0,
            "bytes": output.bytes.len(),
            "outcome": "ok",
            "render_id": record.id,
            "source_render_id": source.id,
            "quality": request.quality,
            "style_key": request.style_key,
            "prompt": short_prompt(&text),
        }),
    )
    .await;

    Ok(RenderAiResult { record, seconds: output.seconds })
}
