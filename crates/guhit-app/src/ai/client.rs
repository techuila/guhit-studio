//! Model clients. `ModelClient` is the seam between the copilot loop and the
//! language model: the real one talks to the Claude Messages API over HTTPS,
//! the scripted one replays canned responses for tests and UI checks.
//!
//! Request and response shapes follow the Messages API (raw HTTP, there is no
//! official Rust SDK): POST /v1/messages with `x-api-key` and
//! `anthropic-version: 2023-06-01`; tools carry `name`, `description`,
//! `input_schema` and optional `strict`; the response `content` is a list of
//! blocks (`text`, `tool_use`, thinking blocks) and a `stop_reason`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::Duration;

use guhit_model::IpcError;
use serde_json::{json, Value};

pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One Messages API request, minus transport details.
#[derive(Debug, Clone)]
pub struct ModelRequest {
    pub model: String,
    pub system: String,
    /// Messages API `messages` array. Assistant turns hold the raw content
    /// blocks returned by the model, unchanged.
    pub messages: Vec<Value>,
    /// Messages API `tools` array.
    pub tools: Vec<Value>,
    pub max_tokens: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolUse {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone)]
pub struct ModelResponse {
    /// Raw content blocks. Echoed back as the assistant turn so thinking and
    /// tool_use blocks survive the loop untouched.
    pub content: Vec<Value>,
    pub stop_reason: String,
}

impl ModelResponse {
    /// All text blocks, joined.
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn tool_uses(&self) -> Vec<ToolUse> {
        self.content
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
            .map(|b| ToolUse {
                id: b.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
                name: b.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                input: b.get("input").cloned().unwrap_or_else(|| json!({})),
            })
            .collect()
    }
}

pub trait ModelClient: Send + Sync {
    fn send<'a>(&'a self, request: &'a ModelRequest) -> BoxFut<'a, Result<ModelResponse, IpcError>>;
}

fn ai_failed(message: impl Into<String>) -> IpcError {
    IpcError::new("ai_failed", message)
}

// ------------------------------------------------------------------ Anthropic

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
/// Gates `fallbacks: "default"`: a request declined by a safety classifier
/// is re-run server side on the recommended fallback model.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// Real client for the Claude Messages API.
pub struct AnthropicClient {
    api_key: String,
    http: reqwest::Client,
    url: String,
}

impl AnthropicClient {
    pub fn new(api_key: String) -> Result<Self, IpcError> {
        // reqwest is built without a bundled crypto provider. Install ring
        // once per process; an Err only means one is already installed.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .user_agent(concat!("guhit-studio/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| ai_failed(format!("could not start the HTTPS client: {e}")))?;
        Ok(Self { api_key, http, url: API_URL.to_string() })
    }

    fn body(request: &ModelRequest, with_fallbacks: bool) -> Value {
        let mut body = json!({
            "model": request.model,
            "max_tokens": request.max_tokens,
            // A content-block list so the stable prefix (tools + system)
            // can be cached across the rounds of one turn and across turns.
            "system": [{
                "type": "text",
                "text": request.system,
                "cache_control": {"type": "ephemeral"},
            }],
            "messages": request.messages,
            "tools": request.tools,
        });
        // `thinking` is left out on purpose: every current model accepts
        // that and picks its own default. Effort is lowered for interactive
        // latency where the model supports the parameter.
        if supports_effort(&request.model) {
            body["output_config"] = json!({"effort": "medium"});
        }
        if with_fallbacks {
            body["fallbacks"] = json!("default");
        }
        body
    }

    async fn post(&self, body: &Value, beta: Option<&str>) -> Result<(u16, Value), IpcError> {
        let mut req = self
            .http
            .post(&self.url)
            .header("content-type", "application/json")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION);
        if let Some(beta) = beta {
            req = req.header("anthropic-beta", beta);
        }
        let res = req.json(body).send().await.map_err(|e| {
            // without_url: keep messages short, and never echo request data.
            let e = e.without_url();
            if e.is_timeout() {
                ai_failed("The AI service did not answer in time. Check the connection and try again.")
            } else if e.is_connect() {
                ai_failed("Could not reach the AI service. Check the internet connection.")
            } else {
                ai_failed(format!("AI request failed: {e}"))
            }
        })?;
        let status = res.status().as_u16();
        let text = res
            .text()
            .await
            .map_err(|e| ai_failed(format!("could not read the AI response: {}", e.without_url())))?;
        let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok((status, value))
    }
}

/// Models that accept `output_config.effort`. Haiku 4.5 and older reject it.
fn supports_effort(model: &str) -> bool {
    !model.contains("haiku") && !model.contains("claude-3")
}

/// Models that accept the server-side `fallbacks` parameter.
fn supports_fallbacks(model: &str) -> bool {
    model.starts_with("claude-opus-5") || model.starts_with("claude-fable-5")
}

fn api_error_message(body: &Value) -> String {
    body.pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or("no details")
        .to_string()
}

/// Map an HTTP error status to a message a user can act on. The API's own
/// message is included; it never contains the key.
fn status_error(status: u16, body: &Value) -> IpcError {
    let detail = api_error_message(body);
    let message = match status {
        401 => "The API key was rejected. Check it in the copilot settings.".to_string(),
        402 => format!("The AI account has a billing problem: {detail}"),
        403 => format!("This API key is not allowed to do that: {detail}"),
        404 => format!("The model was not found or is not available to this key: {detail}"),
        413 => "The request was too large for the AI service.".to_string(),
        429 => "The AI service is rate limiting this key. Wait a moment and try again.".to_string(),
        500..=599 => "The AI service is having trouble right now. Try again in a moment.".to_string(),
        _ => format!("AI request failed ({status}): {detail}"),
    };
    ai_failed(message)
}

impl ModelClient for AnthropicClient {
    fn send<'a>(&'a self, request: &'a ModelRequest) -> BoxFut<'a, Result<ModelResponse, IpcError>> {
        Box::pin(async move {
            let mut use_fallbacks = supports_fallbacks(&request.model);
            let (mut status, mut value) = self
                .post(&Self::body(request, use_fallbacks), use_fallbacks.then_some(FALLBACK_BETA))
                .await?;

            // A key whose organization does not have the fallback beta gets a
            // 400 that names the header. Retry once without it.
            if status == 400 && use_fallbacks && api_error_message(&value).contains("anthropic-beta") {
                use_fallbacks = false;
                (status, value) = self.post(&Self::body(request, false), None).await?;
            }
            // One retry on rate limit, overload or a transient server error.
            if matches!(status, 429 | 500 | 529) {
                tokio::time::sleep(Duration::from_millis(1200)).await;
                (status, value) = self
                    .post(&Self::body(request, use_fallbacks), use_fallbacks.then_some(FALLBACK_BETA))
                    .await?;
            }
            if !(200..300).contains(&status) {
                return Err(status_error(status, &value));
            }

            let stop_reason = value
                .get("stop_reason")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let content = value
                .get("content")
                .and_then(Value::as_array)
                .cloned()
                .ok_or_else(|| ai_failed("the AI service returned a response without content"))?;
            Ok(ModelResponse { content, stop_reason })
        })
    }
}

// ------------------------------------------------------------------- scripted

/// Replays a fixed list of responses, one per `send`. Records every request
/// so tests can assert on what the loop sent. When the script runs out it
/// keeps returning the last response, which lets a test script an endless
/// tool-calling model.
pub struct ScriptedClient {
    responses: Vec<ModelResponse>,
    pub requests: Mutex<Vec<ModelRequest>>,
    /// Pause before each response, to imitate network latency in UI checks.
    delay: Duration,
    /// When set, builds each response from the request, so a test can react
    /// to tool results (for example reuse an id the loop reported).
    responder: Option<Responder>,
}

pub type Responder = Box<dyn Fn(usize, &ModelRequest) -> ModelResponse + Send + Sync>;

impl ScriptedClient {
    pub fn new(responses: Vec<ModelResponse>) -> Self {
        Self { responses, requests: Mutex::new(vec![]), delay: Duration::ZERO, responder: None }
    }

    /// A model whose response `index` is computed from the request it got.
    pub fn dynamic(responder: Responder) -> Self {
        Self { responder: Some(responder), ..Self::new(vec![]) }
    }

    /// A response that ends the turn with plain text.
    pub fn text(text: &str) -> ModelResponse {
        ModelResponse {
            content: vec![json!({"type": "text", "text": text})],
            stop_reason: "end_turn".into(),
        }
    }

    /// A response that calls tools. `calls` is (tool name, input).
    pub fn tools(calls: &[(&str, Value)]) -> ModelResponse {
        let content = calls
            .iter()
            .enumerate()
            .map(|(i, (name, input))| {
                json!({"type": "tool_use", "id": format!("toolu_{name}_{i}"), "name": name, "input": input})
            })
            .collect();
        ModelResponse { content, stop_reason: "tool_use".into() }
    }

    pub fn request_count(&self) -> usize {
        self.requests.lock().map(|r| r.len()).unwrap_or(0)
    }

    /// Load a dev script. Format:
    /// `{"turns":[{"match":"substring of the user message","delay_ms":0,"responses":[{"content":[...],"stop_reason":"tool_use"}]}]}`.
    /// The first turn whose `match` is contained in `user_message` (case
    /// insensitive) wins; an empty `match` matches everything.
    pub fn from_script(script: &Value, user_message: &str) -> Result<Self, IpcError> {
        let message = user_message.to_lowercase();
        let turn = script
            .get("turns")
            .and_then(Value::as_array)
            .and_then(|turns| {
                turns.iter().find(|t| {
                    let m = t.get("match").and_then(Value::as_str).unwrap_or("").to_lowercase();
                    message.contains(&m)
                })
            })
            .ok_or_else(|| ai_failed("dev script: no turn matches this message"))?;
        let responses = turn
            .get("responses")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .map(|r| ModelResponse {
                        content: r.get("content").and_then(Value::as_array).cloned().unwrap_or_default(),
                        stop_reason: r
                            .get("stop_reason")
                            .and_then(Value::as_str)
                            .unwrap_or("end_turn")
                            .to_string(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if responses.is_empty() {
            return Err(ai_failed("dev script: the matching turn has no responses"));
        }
        let delay = Duration::from_millis(turn.get("delay_ms").and_then(Value::as_u64).unwrap_or(0).min(10_000));
        Ok(Self { delay, ..Self::new(responses) })
    }
}

impl ModelClient for ScriptedClient {
    fn send<'a>(&'a self, request: &'a ModelRequest) -> BoxFut<'a, Result<ModelResponse, IpcError>> {
        Box::pin(async move {
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            let index = {
                let mut seen = self.requests.lock().map_err(|_| ai_failed("scripted client poisoned"))?;
                seen.push(request.clone());
                seen.len() - 1
            };
            if let Some(responder) = &self.responder {
                return Ok(responder(index, request));
            }
            let response = self
                .responses
                .get(index)
                .or_else(|| self.responses.last())
                .cloned()
                .ok_or_else(|| ai_failed("scripted client has no responses"))?;
            Ok(response)
        })
    }
}
