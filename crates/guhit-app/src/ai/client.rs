//! Model clients. `ModelClient` is the seam between the copilot loop and the
//! language model: the real one talks to the Claude Messages API over HTTPS,
//! the scripted one replays canned responses for tests and UI checks.
//!
//! Request and response shapes follow the Messages API (raw HTTP, there is no
//! official Rust SDK): POST /v1/messages with `x-api-key` and
//! `anthropic-version: 2023-06-01`; tools carry `name`, `description`,
//! `input_schema` and optional `strict`; the response `content` is a list of
//! blocks (`text`, `tool_use`, thinking blocks) and a `stop_reason`.
//!
//! Two ways to send. `send` is the copilot's: one retry on a rate limit or
//! an overload, every failure an `ai_failed` error. `send_once` is the
//! swarm's (D56): sent once, with the response's `usage` and model, and a
//! typed error ([`ClientError`]) with the `retry-after` the API asked for,
//! so a worker can wait without failing. The swarm's workers share one
//! [`Gate`] in `AiState`: at most [`MAX_IN_FLIGHT`] requests at once.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use guhit_model::IpcError;
use serde_json::{json, Value};

use crate::AppService;

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

/// Token counts from the API's `usage`. `input_tokens` is the uncached part
/// only: the whole prompt is the sum of the three input figures.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// `cache_read_input_tokens`.
    pub cache_read_tokens: u64,
    /// `cache_creation_input_tokens`.
    pub cache_write_tokens: u64,
}

impl Usage {
    /// From a response's `usage` object; a missing field counts 0.
    pub fn from_api(usage: &Value) -> Self {
        let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
        Self {
            input_tokens: n("input_tokens"),
            output_tokens: n("output_tokens"),
            cache_read_tokens: n("cache_read_input_tokens"),
            cache_write_tokens: n("cache_creation_input_tokens"),
        }
    }

    /// Every token of the prompt, cached or not.
    pub fn prompt_tokens(&self) -> u64 {
        self.input_tokens + self.cache_read_tokens + self.cache_write_tokens
    }
}

/// One response with what it cost.
#[derive(Debug, Clone)]
pub struct Metered {
    pub response: ModelResponse,
    pub usage: Usage,
    /// The model that answered, as the response names it: a server-side
    /// fallback can answer with another model than the request named, and
    /// is billed at that model's price. None when the client cannot say.
    pub model: Option<String>,
}

impl Metered {
    /// A response that cost nothing anybody knows of.
    pub fn unmetered(response: ModelResponse) -> Self {
        Self { response, usage: Usage::default(), model: None }
    }
}

/// Why one `send_once` failed, typed so a caller can wait instead of fail.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientError {
    /// 429. `retry_after` is the API's `retry-after` header, when it sent one.
    RateLimited { retry_after: Option<Duration>, message: String },
    /// 529 or a passing 5xx: the service is busy.
    Overloaded { retry_after: Option<Duration>, message: String },
    /// 401 or 403: the key is wrong or not allowed.
    Auth { message: String },
    Other(IpcError),
}

impl ClientError {
    /// `rate_limited`, `overloaded`, `auth`, or the other error's own code.
    pub fn code(&self) -> &str {
        match self {
            Self::RateLimited { .. } => "rate_limited",
            Self::Overloaded { .. } => "overloaded",
            Self::Auth { .. } => "auth",
            Self::Other(e) => &e.code,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::RateLimited { message, .. } | Self::Overloaded { message, .. } | Self::Auth { message } => message,
            Self::Other(e) => &e.message,
        }
    }

    /// True for the errors that pass: a rate limit or an overload.
    pub fn is_wait(&self) -> bool {
        matches!(self, Self::RateLimited { .. } | Self::Overloaded { .. })
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after, .. } | Self::Overloaded { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

impl From<IpcError> for ClientError {
    fn from(e: IpcError) -> Self {
        Self::Other(e)
    }
}

impl From<ClientError> for IpcError {
    fn from(e: ClientError) -> Self {
        match e {
            ClientError::Other(e) => e,
            other => IpcError::new(other.code(), other.message()),
        }
    }
}

pub trait ModelClient: Send + Sync {
    fn send<'a>(&'a self, request: &'a ModelRequest) -> BoxFut<'a, Result<ModelResponse, IpcError>>;

    /// One request, sent once (no retry), with its usage and a typed error.
    /// For callers that pace themselves: the swarm's lead and workers. The
    /// default goes through `send` and knows no usage.
    fn send_once<'a>(&'a self, request: &'a ModelRequest) -> BoxFut<'a, Result<Metered, ClientError>> {
        Box::pin(async move { self.send(request).await.map(Metered::unmetered).map_err(ClientError::from) })
    }
}

fn ai_failed(message: impl Into<String>) -> IpcError {
    IpcError::new("ai_failed", message)
}

// ----------------------------------------------------------------- the gate

/// Swarm requests in flight at once, across every worker (amendments,
/// "Workers and cost"): 4 to 6 loops share one key.
pub const MAX_IN_FLIGHT: usize = 4;

/// The shared gate in `AiState`: a request of the swarm's lead or a worker
/// goes out only with one of its `MAX_IN_FLIGHT` places.
pub struct Gate(tokio::sync::Semaphore);

impl Default for Gate {
    fn default() -> Self {
        Self(tokio::sync::Semaphore::new(MAX_IN_FLIGHT))
    }
}

impl Gate {
    /// Wait for a place; it is free again when the permit drops.
    pub async fn enter(&self) -> tokio::sync::SemaphorePermit<'_> {
        self.0.acquire().await.expect("the gate is never closed")
    }

    /// Requests in flight now.
    pub fn in_flight(&self) -> usize {
        MAX_IN_FLIGHT - self.0.available_permits()
    }
}

/// The client the copilot would use for a turn that starts with
/// `first_message`: the injected one (tests), a dev script turn
/// (`GUHIT_AI_SCRIPT`, matched by that message), or Claude on the stored
/// key. The swarm's lead and each worker take one for their conversation.
pub(crate) fn client_for(app: &AppService, first_message: &str) -> Result<Arc<dyn ModelClient>, IpcError> {
    super::model_client(app, first_message)
}

/// The model the person chose in the copilot settings, if any.
pub(crate) fn model_setting(data_dir: &std::path::Path) -> Option<String> {
    super::read_settings(data_dir)
        .get(super::SETTINGS_MODEL_KEY)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_string)
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
            // Top level too: the cache point follows the conversation, so a
            // loop of many rounds reads its earlier rounds from the cache
            // instead of paying for them again each round.
            "cache_control": {"type": "ephemeral"},
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

    /// The status, the JSON body, and the `retry-after` header in seconds.
    async fn post(&self, body: &Value, beta: Option<&str>) -> Result<(u16, Value, Option<Duration>), IpcError> {
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
        let retry_after = res.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(retry_after_secs);
        let text = res
            .text()
            .await
            .map_err(|e| ai_failed(format!("could not read the AI response: {}", e.without_url())))?;
        let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok((status, value, retry_after))
    }

    /// One post, plus the one retry without the fallback beta that a key
    /// whose organization lacks it needs.
    async fn post_with_fallbacks(&self, request: &ModelRequest) -> Result<(u16, Value, Option<Duration>, bool), IpcError> {
        let mut use_fallbacks = supports_fallbacks(&request.model);
        let (mut status, mut value, mut retry) =
            self.post(&Self::body(request, use_fallbacks), use_fallbacks.then_some(FALLBACK_BETA)).await?;
        // A key whose organization does not have the fallback beta gets a
        // 400 that names the header. Retry once without it.
        if status == 400 && use_fallbacks && api_error_message(&value).contains("anthropic-beta") {
            use_fallbacks = false;
            (status, value, retry) = self.post(&Self::body(request, false), None).await?;
        }
        Ok((status, value, retry, use_fallbacks))
    }
}

/// `retry-after` as the API sends it: seconds, whole or not. The HTTP-date
/// form is not used by the Messages API and reads as None.
fn retry_after_secs(header: &str) -> Option<Duration> {
    let secs: f64 = header.trim().parse().ok()?;
    (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs.min(3600.0)))
}

/// A successful Messages API body as a response.
fn parse_response(value: &Value) -> Result<ModelResponse, IpcError> {
    let stop_reason = value.get("stop_reason").and_then(Value::as_str).unwrap_or_default().to_string();
    let content = value
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| ai_failed("the AI service returned a response without content"))?;
    Ok(ModelResponse { content, stop_reason })
}

/// An HTTP error status as a typed error, with the message `send` gives.
fn typed_error(status: u16, body: &Value, retry_after: Option<Duration>) -> ClientError {
    let message = status_error(status, body).message;
    match status {
        429 => ClientError::RateLimited { retry_after, message },
        500 | 502 | 503 | 504 | 529 => ClientError::Overloaded { retry_after, message },
        401 | 403 => ClientError::Auth { message },
        _ => ClientError::Other(status_error(status, body)),
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
            let (mut status, mut value, _, use_fallbacks) = self.post_with_fallbacks(request).await?;
            // One retry on rate limit, overload or a transient server error.
            if matches!(status, 429 | 500 | 529) {
                tokio::time::sleep(Duration::from_millis(1200)).await;
                (status, value, _) = self
                    .post(&Self::body(request, use_fallbacks), use_fallbacks.then_some(FALLBACK_BETA))
                    .await?;
            }
            if !(200..300).contains(&status) {
                return Err(status_error(status, &value));
            }
            parse_response(&value)
        })
    }

    fn send_once<'a>(&'a self, request: &'a ModelRequest) -> BoxFut<'a, Result<Metered, ClientError>> {
        Box::pin(async move {
            let (status, value, retry_after, _) = self.post_with_fallbacks(request).await?;
            if !(200..300).contains(&status) {
                return Err(typed_error(status, &value, retry_after));
            }
            Ok(Metered {
                response: parse_response(&value)?,
                usage: value.get("usage").map(Usage::from_api).unwrap_or_default(),
                model: value.get("model").and_then(Value::as_str).map(str::to_string),
            })
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
    /// When set, builds each `send_once` answer, usage and typed errors
    /// included (the swarm's tests: spend, rate limits).
    metered: Option<MeteredResponder>,
    /// Usage per scripted response, from a dev script.
    usage: Vec<Usage>,
    /// A dev script's `$result:` references are filled in from the request.
    refs: bool,
}

pub type Responder = Box<dyn Fn(usize, &ModelRequest) -> ModelResponse + Send + Sync>;
pub type MeteredResponder = Box<dyn Fn(usize, &ModelRequest) -> Result<Metered, ClientError> + Send + Sync>;

impl ScriptedClient {
    pub fn new(responses: Vec<ModelResponse>) -> Self {
        Self {
            responses,
            requests: Mutex::new(vec![]),
            delay: Duration::ZERO,
            responder: None,
            metered: None,
            usage: vec![],
            refs: false,
        }
    }

    /// A model whose response `index` is computed from the request it got.
    pub fn dynamic(responder: Responder) -> Self {
        Self { responder: Some(responder), ..Self::new(vec![]) }
    }

    /// A model whose `send_once` answer `index`, usage or typed error, is
    /// computed from the request it got. `send` gives the response or the
    /// error as an `IpcError`.
    pub fn metered(responder: MeteredResponder) -> Self {
        Self { metered: Some(responder), ..Self::new(vec![]) }
    }

    /// Pause this long before each answer.
    pub fn with_delay(self, delay: Duration) -> Self {
        Self { delay, ..self }
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
    ///
    /// A response may carry `usage` as the API names it (`input_tokens`,
    /// `output_tokens`, `cache_read_input_tokens`,
    /// `cache_creation_input_tokens`), which `send_once` reports. A string
    /// in a `tool_use` input of the form `$result:<n><JSON pointer>` is
    /// replaced by the value at that pointer in the `n`th tool result of
    /// the request's last message, so a script can use an id the app made:
    /// `"$result:0/data/elements/2/id"`.
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
        let usage = turn
            .get("responses")
            .and_then(Value::as_array)
            .map(|list| list.iter().map(|r| r.get("usage").map(Usage::from_api).unwrap_or_default()).collect())
            .unwrap_or_default();
        let delay = Duration::from_millis(turn.get("delay_ms").and_then(Value::as_u64).unwrap_or(0).min(10_000));
        Ok(Self { delay, usage, refs: true, ..Self::new(responses) })
    }

    /// Record the request; its index among every request this client got.
    async fn record(&self, request: &ModelRequest) -> Result<usize, IpcError> {
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        let mut seen = self.requests.lock().map_err(|_| ai_failed("scripted client poisoned"))?;
        seen.push(request.clone());
        Ok(seen.len() - 1)
    }

    /// Scripted response `index` (the last one once the script runs out),
    /// with its usage and its `$result:` references filled in.
    fn scripted(&self, index: usize, request: &ModelRequest) -> Result<Metered, IpcError> {
        let at = index.min(self.responses.len().saturating_sub(1));
        let mut response = self.responses.get(at).cloned().ok_or_else(|| ai_failed("scripted client has no responses"))?;
        if self.refs {
            fill_refs(&mut response, request);
        }
        Ok(Metered { response, usage: self.usage.get(at).copied().unwrap_or_default(), model: None })
    }
}

/// Fill a dev script's `$result:<n><pointer>` strings inside `tool_use`
/// inputs from the tool results of the request's last message. A reference
/// that does not resolve stays as written, so the tool call fails visibly.
fn fill_refs(response: &mut ModelResponse, request: &ModelRequest) {
    let results: Vec<Value> = request
        .messages
        .last()
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
                .map(|b| {
                    let text = b.get("content").and_then(Value::as_str).unwrap_or("null");
                    serde_json::from_str(text).unwrap_or(Value::Null)
                })
                .collect()
        })
        .unwrap_or_default();
    fn walk(v: &mut Value, results: &[Value]) {
        match v {
            Value::String(s) => {
                let Some(rest) = s.strip_prefix("$result:") else { return };
                let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
                let pointer = &rest[digits.len()..];
                let found = digits.parse::<usize>().ok().and_then(|n| results.get(n)).and_then(|r| r.pointer(pointer));
                if let Some(found) = found {
                    *v = found.clone();
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|i| walk(i, results)),
            Value::Object(map) => map.values_mut().for_each(|i| walk(i, results)),
            _ => {}
        }
    }
    for block in &mut response.content {
        if block.get("type").and_then(Value::as_str) == Some("tool_use") {
            if let Some(input) = block.get_mut("input") {
                walk(input, &results);
            }
        }
    }
}

impl ModelClient for ScriptedClient {
    fn send<'a>(&'a self, request: &'a ModelRequest) -> BoxFut<'a, Result<ModelResponse, IpcError>> {
        Box::pin(async move {
            let index = self.record(request).await?;
            if let Some(metered) = &self.metered {
                return metered(index, request).map(|m| m.response).map_err(IpcError::from);
            }
            if let Some(responder) = &self.responder {
                return Ok(responder(index, request));
            }
            self.scripted(index, request).map(|m| m.response)
        })
    }

    fn send_once<'a>(&'a self, request: &'a ModelRequest) -> BoxFut<'a, Result<Metered, ClientError>> {
        Box::pin(async move {
            let index = self.record(request).await?;
            if let Some(metered) = &self.metered {
                return metered(index, request);
            }
            if let Some(responder) = &self.responder {
                return Ok(Metered::unmetered(responder(index, request)));
            }
            Ok(self.scripted(index, request)?)
        })
    }
}

#[cfg(test)]
mod swarm_tests {
    //! What the swarm's workers need from the client (D56): usage, typed
    //! errors with `retry-after`, the top-level cache point, the gate.
    use super::*;

    fn request() -> ModelRequest {
        ModelRequest {
            model: "claude-opus-5-5".into(),
            system: "S".into(),
            messages: vec![json!({"role": "user", "content": "hi"})],
            tools: vec![],
            max_tokens: 100,
        }
    }

    #[test]
    fn the_cache_point_is_at_the_top_level_and_on_the_system_prompt() {
        let body = AnthropicClient::body(&request(), false);
        assert_eq!(body["cache_control"], json!({"type": "ephemeral"}));
        assert_eq!(body["system"][0]["cache_control"], json!({"type": "ephemeral"}));
    }

    #[test]
    fn retry_after_reads_seconds() {
        assert_eq!(retry_after_secs("12"), Some(Duration::from_secs(12)));
        assert_eq!(retry_after_secs(" 1.5 "), Some(Duration::from_millis(1500)));
        assert_eq!(retry_after_secs("-3"), None);
        assert_eq!(retry_after_secs("Wed, 21 Oct 2026 07:28:00 GMT"), None);
    }

    #[test]
    fn errors_are_typed() {
        let body = json!({"error": {"message": "slow"}});
        let wait = Some(Duration::from_secs(7));
        assert_eq!(typed_error(429, &body, wait).retry_after(), wait);
        assert_eq!(typed_error(429, &body, wait).code(), "rate_limited");
        assert_eq!(typed_error(529, &body, None).code(), "overloaded");
        assert!(typed_error(503, &body, None).is_wait());
        assert_eq!(typed_error(401, &body, None).code(), "auth");
        let other = typed_error(400, &body, None);
        assert!(!other.is_wait());
        assert_eq!(other.code(), "ai_failed", "the copilot's code for the rest");
        let ipc: IpcError = typed_error(429, &body, wait).into();
        assert_eq!(ipc.code, "rate_limited");
    }

    #[test]
    fn usage_reads_the_api_names() {
        let u = Usage::from_api(&json!({"input_tokens": 1, "output_tokens": 2, "cache_read_input_tokens": 3, "cache_creation_input_tokens": 4}));
        assert_eq!(u, Usage { input_tokens: 1, output_tokens: 2, cache_read_tokens: 3, cache_write_tokens: 4 });
        assert_eq!(u.prompt_tokens(), 8);
        assert_eq!(Usage::from_api(&json!({})), Usage::default());
    }

    #[tokio::test]
    async fn a_dev_script_reports_usage_and_fills_its_references() {
        let script = json!({"turns": [{"match": "part: rooms", "responses": [{
            "content": [{"type": "tool_use", "id": "t1", "name": "add_door", "input": {"wall_id": "$result:1/data/elements/0/id", "x": "$result:9/nope"}}],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 5, "output_tokens": 6, "cache_read_input_tokens": 7, "cache_creation_input_tokens": 8},
        }]}]});
        let client = ScriptedClient::from_script(&script, "Part: rooms\n...").unwrap();
        let mut req = request();
        req.messages.push(json!({"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "a", "content": "{\"data\": 1}"},
            {"type": "tool_result", "tool_use_id": "b", "content": "{\"data\": {\"elements\": [{\"id\": \"wall-9\"}]}}"},
        ]}));
        let m = client.send_once(&req).await.unwrap();
        assert_eq!(m.usage.cache_write_tokens, 8);
        let input = &m.response.tool_uses()[0].input;
        assert_eq!(input["wall_id"], json!("wall-9"));
        assert_eq!(input["x"], json!("$result:9/nope"), "an unresolved reference stays as written");
    }

    #[tokio::test]
    async fn the_gate_holds_four() {
        let gate = Gate::default();
        let mut held = vec![];
        for _ in 0..MAX_IN_FLIGHT {
            held.push(gate.enter().await);
        }
        assert_eq!(gate.in_flight(), MAX_IN_FLIGHT);
        let fifth = tokio::time::timeout(Duration::from_millis(50), gate.enter()).await;
        assert!(fifth.is_err(), "a fifth waits");
        drop(held);
        assert_eq!(gate.in_flight(), 0);
    }
}
