//! Google Gemini image models over REST (DECISIONS D17). There is no official
//! Rust SDK, so this is raw HTTP, the same way `ai/client.rs` talks to the
//! Claude Messages API.
//!
//! Shape taken from https://ai.google.dev/gemini-api/docs/image-generation on
//! 2026-09-22. That page marks the Interactions API as the current form for
//! image editing with an input image; the older `:generateContent` form is not
//! what its image-editing examples use, so this is the one implemented.
//!
//! Request: POST https://generativelanguage.googleapis.com/v1beta/interactions
//! Headers: `x-goog-api-key: <key>` (https://ai.google.dev/gemini-api/docs/api-key)
//!          and `Content-Type: application/json`.
//! Body fields, all top level:
//!   `model`            model id, for example "gemini-3.1-flash-image"
//!   `input`            array of parts:
//!                        {"type":"text","text":"..."}
//!                        {"type":"image","mime_type":"image/png","data":"<base64>"}
//!   `response_format`  {"type":"image","mime_type":"image/png",
//!                       "aspect_ratio":"16:9","image_size":"2K"}
//!   `generation_config` optional, for example {"thinking_level":"high"}
//! `image_size` takes "0.5K", "1K", "2K" or "4K", uppercase K; a lowercase
//! "1k" is rejected by the API.
//!
//! Response: the image arrives base64 encoded. The documented accessor is
//! `interaction.output_image` (with `data` and `mime_type`), and the same
//! parts also appear in `interaction.steps[].content[]` as
//! `{"type":"image","mime_type":...,"data":...}`. Both are read here, in that
//! order, followed by a tolerant walk, so a field being renamed or moved does
//! not break generation outright.
//! Errors come back as `{"error":{"code":..,"message":..,"status":..}}`.

use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use guhit_model::RenderQuality;
use serde_json::{json, Value};

use crate::files::ImageKind;

use super::provider::{BoxFut, ImageProvider, ProviderError, RenderInput, RenderOutput};

const API_URL: &str = "https://generativelanguage.googleapis.com/v1beta/interactions";

pub const PROVIDER: &str = "gemini";

/// Default families. Draft and Standard are cheap enough to iterate with;
/// High goes to the Pro model, which is the only one worth 4K.
pub const FLASH_MODEL: &str = "gemini-3.1-flash-image";
pub const FLASH_LITE_MODEL: &str = "gemini-3.1-flash-lite-image";
pub const PRO_MODEL: &str = "gemini-3-pro-image";

/// Image models split by what `image_size` values they accept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// 0.5K, 1K, 2K, 4K.
    Flash,
    /// 1K only.
    FlashLite,
    /// 1K, 2K, 4K.
    Pro,
}

impl Family {
    fn of(model: &str) -> Self {
        let m = model.to_ascii_lowercase();
        if m.contains("flash-lite") || m.contains("flash_lite") {
            Family::FlashLite
        } else if m.contains("pro") {
            Family::Pro
        } else {
            Family::Flash
        }
    }

    /// The largest size this family allows at that quality.
    fn image_size(self, quality: RenderQuality) -> &'static str {
        match (self, quality) {
            (Family::FlashLite, _) => "1K",
            (Family::Flash, RenderQuality::Draft) => "0.5K",
            // The Pro model has no 0.5K, so a draft on Pro is a 1K draft.
            (Family::Pro, RenderQuality::Draft) => "1K",
            (_, RenderQuality::Standard) => "2K",
            (_, RenderQuality::High) => "4K",
        }
    }
}

/// Google list price per image, rounded to cents for the UI. From
/// https://ai.google.dev/gemini-api/docs/pricing on 2026-09-22.
fn price_usd(family: Family, image_size: &str) -> &'static str {
    match (family, image_size) {
        (Family::Pro, "4K") => "0.24",
        (Family::Pro, _) => "0.13",
        (Family::FlashLite, _) => "0.07",
        (Family::Flash, "0.5K") => "0.05",
        (Family::Flash, "1K") => "0.07",
        (Family::Flash, "2K") => "0.10",
        (Family::Flash, "4K") => "0.15",
        _ => "0.15",
    }
}

/// What a quality setting resolves to, given the user's `model` preference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub model: String,
    pub image_size: &'static str,
    pub family: Family,
    /// Why this model, in a few words for the price line.
    pub why: &'static str,
}

impl Choice {
    pub fn price_usd(&self) -> &'static str {
        price_usd(self.family, self.image_size)
    }

    /// "Gemini Flash", "Gemini Pro", "Gemini Flash Lite", or the exact id.
    pub fn label(&self) -> String {
        match self.model.as_str() {
            FLASH_MODEL => "Gemini Flash".to_string(),
            PRO_MODEL => "Gemini Pro".to_string(),
            FLASH_LITE_MODEL => "Gemini Flash Lite".to_string(),
            other => other.to_string(),
        }
    }
}

/// The `render_ai_model` setting. Empty means "let the quality pick the
/// family", which is the default and the cheapest sensible behaviour: the
/// Pro model runs only for High quality or when a style reference goes with
/// the request (DECISIONS D53), because it follows a reference image far
/// better and costs more. An explicit setting is a person's own choice and
/// wins; the price shows before every call either way.
///
/// Accepted: "" (auto), "flash", "flash-lite", "pro", or an exact model id
/// starting with "gemini-".
pub fn resolve(model_setting: &str, quality: RenderQuality, style_reference: bool) -> Choice {
    let setting = model_setting.trim().to_ascii_lowercase();
    let (model, why) = match setting.as_str() {
        "flash" => (FLASH_MODEL.to_string(), "picked in settings"),
        "flash-lite" | "flash_lite" | "lite" => (FLASH_LITE_MODEL.to_string(), "picked in settings"),
        "pro" => (PRO_MODEL.to_string(), "picked in settings"),
        "" | "auto" => match (quality, style_reference) {
            (RenderQuality::High, _) => (PRO_MODEL.to_string(), "High quality"),
            (_, true) => (PRO_MODEL.to_string(), "the project look goes with it"),
            _ => (FLASH_MODEL.to_string(), "the cheaper model for this quality"),
        },
        exact => (exact.to_string(), "picked in settings"),
    };
    let family = Family::of(&model);
    Choice { image_size: family.image_size(quality), model, family, why }
}

/// Reject anything that is not one of the aliases or a plausible model id,
/// before it is written to settings.json. A typo here would otherwise only
/// surface as a 404 at generation time.
pub fn validate_model_setting(raw: &str) -> Result<String, guhit_model::IpcError> {
    let model = raw.trim();
    if model.is_empty() {
        return Ok(String::new());
    }
    let bad = || {
        guhit_model::IpcError::new(
            "invalid",
            "Use `flash`, `pro`, `flash-lite`, or an exact Gemini image model id such as gemini-3.1-flash-image.",
        )
    };
    if model.chars().count() > 100 || model.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(bad());
    }
    let lower = model.to_ascii_lowercase();
    let alias = matches!(lower.as_str(), "flash" | "pro" | "flash-lite" | "flash_lite" | "lite" | "auto");
    let exact = lower.starts_with("gemini-") && lower.contains("image");
    if alias || exact {
        Ok(lower)
    } else {
        Err(bad())
    }
}

// ------------------------------------------------------------------- provider

pub struct GeminiProvider {
    api_key: String,
    model_setting: String,
    http: reqwest::Client,
    url: String,
}

impl GeminiProvider {
    pub fn new(api_key: String, model_setting: String) -> Result<Self, ProviderError> {
        // reqwest is built without a bundled crypto provider. Install ring
        // once per process; an Err only means one is already installed.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            // Image generation is slow. 120 s covers a 4K Pro render.
            .timeout(Duration::from_secs(120))
            .user_agent(concat!("guhit-studio/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| ProviderError::failed(format!("could not start the HTTPS client: {e}")))?;
        Ok(Self { api_key, model_setting, http, url: API_URL.to_string() })
    }

    /// Point the provider at another base URL. Only for local testing against
    /// a stub server; production always uses `API_URL`.
    pub fn with_url(mut self, url: String) -> Self {
        self.url = url;
        self
    }

    pub fn choice(&self, quality: RenderQuality, style_reference: bool) -> Choice {
        resolve(&self.model_setting, quality, style_reference)
    }

    /// The request body: the text, then the capture, its maps and the style
    /// reference, in the order the text names them.
    pub fn body(&self, input: &RenderInput, choice: &Choice) -> Value {
        // Gemini has no negative-prompt field, so the constraint goes in the
        // text. It is usually already the last line of the prompt.
        let mut text = input.prompt.clone();
        let constraint = input.constraint.trim();
        if !constraint.is_empty() && !text.contains(constraint) {
            text.push_str("\n\n");
            text.push_str(constraint);
        }
        let mut parts = vec![
            json!({"type": "text", "text": text}),
            json!({"type": "image", "mime_type": "image/png", "data": B64.encode(&input.png)}),
        ];
        for extra in input.maps.iter().chain(input.style_reference.iter()) {
            parts.push(json!({"type": "image", "mime_type": extra.mime, "data": B64.encode(&extra.bytes)}));
        }
        json!({
            "model": choice.model,
            "input": parts,
            "response_format": {
                "type": "image",
                "mime_type": "image/png",
                "aspect_ratio": input.aspect_ratio,
                "image_size": choice.image_size,
            },
        })
    }
}

/// The provider's own words about a failure. Never contains the key: it is
/// only ever sent as a header and Google does not echo headers back.
fn api_error_message(body: &Value) -> String {
    body.pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or("no details")
        .to_string()
}

fn status_error(status: u16, body: &Value) -> ProviderError {
    let detail = api_error_message(body);
    let message = match status {
        400 => format!("Google AI refused the request: {detail}"),
        401 | 403 => "The Google AI API key was rejected.".to_string(),
        404 => format!("That image model is not available to this key: {detail}"),
        413 => "The capture was too large for the image model.".to_string(),
        429 => "Google AI is rate limiting this key. Wait a moment and try again.".to_string(),
        500..=599 => "Google AI is having trouble right now. Try again in a moment.".to_string(),
        _ => format!("The image request failed ({status}): {detail}"),
    };
    ProviderError::Failed(message)
}

/// One image part out of a response: (mime type, base64 data).
fn image_part(value: &Value) -> Option<(String, String)> {
    let obj = value.as_object()?;
    let data = obj.get("data").and_then(Value::as_str)?;
    if data.is_empty() {
        return None;
    }
    let mime = obj
        .get("mime_type")
        .or_else(|| obj.get("mimeType"))
        .and_then(Value::as_str)
        .unwrap_or("image/png");
    let is_image = mime.starts_with("image/") || obj.get("type").and_then(Value::as_str) == Some("image");
    if is_image {
        Some((mime.to_string(), data.to_string()))
    } else {
        None
    }
}

/// Depth-first search for anything that looks like an image part. The last
/// resort after the two documented paths, so a field moving inside the
/// response does not stop generation.
fn find_image(value: &Value) -> Option<(String, String)> {
    if let Some(found) = image_part(value) {
        return Some(found);
    }
    match value {
        Value::Object(map) => map.values().find_map(find_image),
        Value::Array(items) => items.iter().find_map(find_image),
        _ => None,
    }
}

/// The generated image, by the documented paths first.
fn output_image(body: &Value) -> Option<(String, String)> {
    let root = body.get("interaction").unwrap_or(body);
    if let Some(found) = root.get("output_image").and_then(image_part) {
        return Some(found);
    }
    if let Some(steps) = root.get("steps").and_then(Value::as_array) {
        // Last step first: the final model output is what we want, not an
        // echo of the input image in an earlier step.
        for step in steps.iter().rev() {
            if let Some(content) = step.get("content").and_then(Value::as_array) {
                if let Some(found) = content.iter().rev().find_map(image_part) {
                    return Some(found);
                }
            }
        }
    }
    find_image(root)
}

impl ImageProvider for GeminiProvider {
    fn render<'a>(&'a self, input: RenderInput) -> BoxFut<'a, Result<RenderOutput, ProviderError>> {
        Box::pin(async move {
            let choice = self.choice(input.quality, input.style_reference.is_some());
            let body = self.body(&input, &choice);
            let started = Instant::now();

            let res = self
                .http
                .post(&self.url)
                .header("content-type", "application/json")
                .header("x-goog-api-key", &self.api_key)
                .json(&body)
                .send()
                .await
                .map_err(|e| {
                    // without_url: keep messages short and never echo request data.
                    let e = e.without_url();
                    if e.is_timeout() {
                        ProviderError::failed(
                            "The image model did not answer within two minutes. Try Draft quality or try again.",
                        )
                    } else if e.is_connect() {
                        ProviderError::failed("Could not reach Google AI. Check the internet connection.")
                    } else {
                        ProviderError::failed(format!("The image request failed: {e}"))
                    }
                })?;

            let status = res.status().as_u16();
            let text = res
                .text()
                .await
                .map_err(|e| ProviderError::failed(format!("could not read the image response: {}", e.without_url())))?;
            let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            if !(200..300).contains(&status) {
                return Err(status_error(status, &value));
            }

            let (mime, data) = output_image(&value).ok_or_else(|| {
                ProviderError::failed(
                    "Google AI answered without an image. This usually means the prompt was refused; try a different style or wording.",
                )
            })?;
            let bytes = B64
                .decode(data.as_bytes())
                .map_err(|e| ProviderError::failed(format!("the image data could not be decoded: {e}")))?;
            let kind = match mime.as_str() {
                "image/jpeg" | "image/jpg" => ImageKind::Jpeg,
                "image/png" => ImageKind::Png,
                other => {
                    return Err(ProviderError::failed(format!(
                        "the image model returned an unsupported image type ({other})"
                    )))
                }
            };
            Ok(RenderOutput {
                bytes,
                kind,
                model: choice.model,
                seconds: started.elapsed().as_secs_f64(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::provider::InputImage;
    use super::*;

    #[test]
    fn quality_picks_model_and_size() {
        let draft = resolve("", RenderQuality::Draft, false);
        assert_eq!(draft.model, FLASH_MODEL);
        assert_eq!(draft.image_size, "0.5K");
        let standard = resolve("", RenderQuality::Standard, false);
        assert_eq!(standard.model, FLASH_MODEL);
        assert_eq!(standard.image_size, "2K");
        let high = resolve("", RenderQuality::High, false);
        assert_eq!(high.model, PRO_MODEL);
        assert_eq!(high.image_size, "4K");
    }

    #[test]
    fn pro_runs_only_for_high_or_with_a_style_reference() {
        // DECISIONS D53: the expensive model only for High or the project look.
        for quality in [RenderQuality::Draft, RenderQuality::Standard] {
            assert_eq!(resolve("", quality, false).model, FLASH_MODEL);
            let with_look = resolve("", quality, true);
            assert_eq!(with_look.model, PRO_MODEL);
            assert!(with_look.why.contains("look"));
        }
        assert_eq!(resolve("", RenderQuality::High, false).model, PRO_MODEL);
        assert_eq!(resolve("auto", RenderQuality::Standard, true).price_usd(), "0.13");
        assert_eq!(resolve("", RenderQuality::Standard, false).label(), "Gemini Flash");
        assert_eq!(resolve("", RenderQuality::High, false).label(), "Gemini Pro");
    }

    #[test]
    fn the_model_setting_overrides_the_family() {
        assert_eq!(resolve("pro", RenderQuality::Draft, false).model, PRO_MODEL);
        // Pro has no 0.5K, so a draft on Pro is 1K.
        assert_eq!(resolve("pro", RenderQuality::Draft, false).image_size, "1K");
        assert_eq!(resolve("flash", RenderQuality::High, false).model, FLASH_MODEL);
        assert_eq!(resolve("flash", RenderQuality::High, true).model, FLASH_MODEL, "a person's setting wins");
        assert_eq!(resolve("flash", RenderQuality::High, false).image_size, "4K");
        // Flash Lite only has 1K, at every quality.
        assert_eq!(resolve("flash-lite", RenderQuality::High, false).image_size, "1K");
        let exact = resolve("gemini-3-pro-image", RenderQuality::Standard, false);
        assert_eq!(exact.model, "gemini-3-pro-image");
        assert_eq!(exact.family, Family::Pro);
    }

    #[test]
    fn model_settings_are_validated() {
        assert_eq!(validate_model_setting(" Flash ").unwrap(), "flash");
        assert_eq!(validate_model_setting("").unwrap(), "");
        assert_eq!(validate_model_setting("gemini-3.1-flash-image").unwrap(), "gemini-3.1-flash-image");
        assert!(validate_model_setting("claude-opus-5").is_err());
        assert!(validate_model_setting("gemini-3-pro").is_err(), "not an image model");
        assert!(validate_model_setting("flash image").is_err());
        assert!(validate_model_setting(&"g".repeat(200)).is_err());
    }

    #[test]
    fn prices_follow_the_published_table() {
        assert_eq!(resolve("", RenderQuality::Draft, false).price_usd(), "0.05");
        assert_eq!(resolve("", RenderQuality::Standard, false).price_usd(), "0.10");
        assert_eq!(resolve("", RenderQuality::High, false).price_usd(), "0.24");
        assert_eq!(resolve("pro", RenderQuality::Standard, false).price_usd(), "0.13");
    }

    fn input_with(maps: Vec<InputImage>, style: Option<InputImage>) -> RenderInput {
        RenderInput {
            png: vec![0x89, b'P', b'N', b'G'],
            prompt: "make it lovely".into(),
            constraint: super::super::prompt::GEOMETRY_LINE.into(),
            aspect_ratio: "16:9".into(),
            quality: RenderQuality::Standard,
            maps,
            style_reference: style,
        }
    }

    #[test]
    fn the_maps_and_the_style_reference_follow_the_capture_in_order() {
        let provider = GeminiProvider::new("AIza-not-a-real-key".into(), "".into()).expect("client");
        let lines = InputImage { role: "lines".into(), mime: "image/png".into(), bytes: vec![1, 2, 3] };
        let depth = InputImage { role: "depth".into(), mime: "image/png".into(), bytes: vec![4, 5, 6] };
        let style = InputImage { role: "style".into(), mime: "image/jpeg".into(), bytes: vec![7, 8, 9] };
        let input = input_with(vec![lines, depth], Some(style));
        let choice = provider.choice(input.quality, input.style_reference.is_some());
        assert_eq!(choice.model, PRO_MODEL, "a style reference takes the Pro model");
        let body = provider.body(&input, &choice);
        let parts = body["input"].as_array().expect("input");
        assert_eq!(parts.len(), 5, "text, capture, two maps, the look");
        assert_eq!(parts[0]["type"], json!("text"));
        assert_eq!(parts[1]["data"], json!("iVBORw=="), "the capture first");
        assert_eq!(parts[2]["data"], json!(B64.encode([1, 2, 3])), "then the line map");
        assert_eq!(parts[3]["data"], json!(B64.encode([4, 5, 6])), "then the depth map");
        assert_eq!(parts[4]["mime_type"], json!("image/jpeg"));
        assert_eq!(parts[4]["data"], json!(B64.encode([7, 8, 9])), "the style reference last");
        assert_eq!(body["model"], json!(PRO_MODEL));
    }

    #[test]
    fn the_request_body_matches_the_documented_shape() {
        let provider = GeminiProvider::new("AIza-not-a-real-key".into(), "".into()).expect("client");
        let input = input_with(vec![], None);
        let choice = provider.choice(RenderQuality::Standard, false);
        let body = provider.body(&input, &choice);
        assert_eq!(body["model"], json!(FLASH_MODEL));
        assert_eq!(body["input"][0]["type"], json!("text"));
        assert!(body["input"][0]["text"].as_str().unwrap().contains("make it lovely"));
        assert!(
            body["input"][0]["text"].as_str().unwrap().contains(super::super::prompt::GEOMETRY_LINE),
            "the constraint is appended because Gemini has no negative prompt"
        );
        assert_eq!(body["input"][1]["type"], json!("image"));
        assert_eq!(body["input"][1]["mime_type"], json!("image/png"));
        assert_eq!(body["input"][1]["data"], json!("iVBORw=="));
        assert_eq!(body["response_format"]["image_size"], json!("2K"));
        assert_eq!(body["response_format"]["aspect_ratio"], json!("16:9"));
        assert_eq!(body["response_format"]["type"], json!("image"));
        // The key is a header, never part of the body.
        assert!(!body.to_string().contains("AIza"));
    }

    #[test]
    fn the_constraint_is_not_repeated_when_the_prompt_already_has_it() {
        let provider = GeminiProvider::new("AIza-not-a-real-key".into(), "".into()).expect("client");
        let line = super::super::prompt::GEOMETRY_LINE;
        let input = RenderInput {
            png: vec![],
            prompt: format!("style words\n\n{line}"),
            constraint: line.into(),
            aspect_ratio: "1:1".into(),
            quality: RenderQuality::Draft,
            maps: vec![],
            style_reference: None,
        };
        let body = provider.body(&input, &provider.choice(RenderQuality::Draft, false));
        let text = body["input"][0]["text"].as_str().unwrap();
        assert_eq!(text.matches(line).count(), 1);
    }

    #[test]
    fn the_image_is_read_from_the_documented_paths() {
        let by_accessor = json!({
            "interaction": {"output_image": {"mime_type": "image/png", "data": "AAAA"}}
        });
        assert_eq!(output_image(&by_accessor), Some(("image/png".into(), "AAAA".into())));

        let by_steps = json!({
            "interaction": {
                "id": "int_1",
                "steps": [
                    {"type": "model_output", "content": [
                        {"type": "text", "text": "here you go"},
                        {"type": "image", "mime_type": "image/jpeg", "data": "BBBB"}
                    ]}
                ]
            }
        });
        assert_eq!(output_image(&by_steps), Some(("image/jpeg".into(), "BBBB".into())));

        // A response with no image at all is reported, not guessed at.
        assert_eq!(output_image(&json!({"interaction": {"steps": []}})), None);
    }

    #[test]
    fn errors_are_mapped_without_leaking_anything() {
        let body = json!({"error": {"code": 403, "message": "API key not valid", "status": "PERMISSION_DENIED"}});
        let e = status_error(403, &body);
        assert_eq!(e, ProviderError::Failed("The Google AI API key was rejected.".into()));
        assert_eq!(status_error(401, &Value::Null), e);
        let e = status_error(404, &json!({"error": {"message": "model not found"}}));
        match e {
            ProviderError::Failed(m) => assert!(m.contains("model not found")),
            other => panic!("expected Failed, got {other:?}"),
        }
        match status_error(503, &Value::Null) {
            ProviderError::Failed(m) => assert!(m.contains("trouble right now")),
            other => panic!("expected Failed, got {other:?}"),
        }
    }
}
