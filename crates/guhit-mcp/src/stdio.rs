//! Stdio entry point for MCP clients that only launch a command.
//!
//! Claude Desktop, some Zed and LM Studio setups and older clients start a
//! local server as a process and speak newline-delimited JSON-RPC on its
//! stdin and stdout. There is one open document per running app (docs/MCP.md,
//! "One document"), so this is not a second server: it is a proxy that posts
//! every message to the app's HTTP endpoint and writes each answer back as
//! one line.
//!
//! - stdout carries JSON-RPC only. Diagnostics go to stderr.
//! - Messages are handled concurrently, like requests over HTTP; each answer
//!   is written whole, as one line.
//! - While the app is not running, `initialize`, `ping` and the list methods
//!   are answered here from the same functions the server uses, so a client
//!   that starts every server at launch neither fails nor opens the app.
//! - The first `tools/call` or `resources/read` while the app is down calls
//!   `launch` once, waits for the port, then forwards. If the app does not
//!   come up, the call gets a "not open" error.
//! - With "Allow agents" off the app answers HTTP 503 and a JSON-RPC error.
//!   That is the app answering, not the app being down: the error goes to
//!   the client as it is and nothing is launched.
//! - The client's `clientInfo` from `initialize` goes with every forwarded
//!   message as headers, so the app's Connect agent dialog can name it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rmcp::model::{CallToolResult, ContentBlock, DiscoverResult, ProtocolVersion};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio::task::JoinSet;

/// What a tool call gets while the app is not running.
pub const NOT_OPEN: &str = "Guhit Studio is not open. Open it, then try again.";
/// How long a call waits for the app after `launch`.
pub const DEFAULT_LAUNCH_WAIT: Duration = Duration::from_secs(30);

/// Starts the app. Called at most once per proxy.
pub type Launch = Box<dyn Fn() + Send + Sync>;

const ACCEPT_BOTH: &str = "application/json, text/event-stream";
const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const INTERNAL_ERROR: i64 = -32603;

pub struct ProxyConfig {
    /// The app's MCP port on 127.0.0.1.
    pub port: u16,
    /// Starts the app when a call needs it. `None` never waits.
    pub launch: Option<Launch>,
    /// How long a call waits for the port after `launch`.
    pub launch_wait: Duration,
}

/// Run the proxy on this process's stdin and stdout until stdin closes.
pub async fn run_stdio_proxy(port: u16, launch: Option<Launch>) -> std::io::Result<()> {
    eprintln!("guhit-mcp: stdio proxy for http://127.0.0.1:{port}/mcp");
    let config = ProxyConfig {
        port,
        launch,
        launch_wait: DEFAULT_LAUNCH_WAIT,
    };
    run_proxy(
        config,
        tokio::io::BufReader::new(tokio::io::stdin()),
        tokio::io::stdout(),
    )
    .await
}

/// Run the proxy over any line reader and writer until `input` ends. Every
/// message read before the end is answered before this returns.
pub async fn run_proxy<R, W>(config: ProxyConfig, mut input: R, mut output: W) -> std::io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let proxy = Arc::new(Proxy::new(config)?);
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    let read = async move {
        let mut tasks = JoinSet::new();
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match input.read_until(b'\n', &mut buf).await {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => {
                    eprintln!("guhit-mcp: stdin: {e}");
                    break;
                }
            }
            let line = String::from_utf8_lossy(&buf).trim().to_string();
            if line.is_empty() {
                continue;
            }
            let proxy = proxy.clone();
            let tx = tx.clone();
            tasks.spawn(async move {
                for out in proxy.handle_line(&line).await {
                    let _ = tx.send(out);
                }
            });
            while tasks.try_join_next().is_some() {}
        }
        while let Some(done) = tasks.join_next().await {
            if let Err(e) = done {
                eprintln!("guhit-mcp: a message handler failed: {e}");
            }
        }
        drop(tx);
    };

    let write = async {
        while let Some(line) = rx.recv().await {
            output.write_all(line.as_bytes()).await?;
            output.write_all(b"\n").await?;
            output.flush().await?;
        }
        Ok::<(), std::io::Error>(())
    };

    let ((), written) = tokio::join!(read, write);
    if let Err(e) = &written {
        eprintln!("guhit-mcp: stdout: {e}");
    }
    written
}

/// Why a message could not be forwarded.
enum ForwardError {
    /// Nothing is listening: the app is not running.
    Down,
    /// The app answered with something that is not JSON-RPC.
    Failed(String),
}

struct Proxy {
    port: u16,
    url: String,
    client: reqwest::Client,
    launch: Option<Launch>,
    launched: AtomicBool,
    launch_wait: Duration,
    /// Name and version from the client's `initialize`.
    client_info: std::sync::Mutex<Option<(String, String)>>,
}

impl Proxy {
    fn new(config: ProxyConfig) -> std::io::Result<Self> {
        // reqwest is built with `rustls-no-provider` (through guhit-app) and
        // refuses to build a client without a provider, even for plain HTTP.
        // Installing one twice is harmless; the second call is refused.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            // Loopback only: a proxy from the environment must not see this.
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(std::io::Error::other)?;
        Ok(Self {
            port: config.port,
            url: format!("http://127.0.0.1:{}/mcp", config.port),
            client,
            launch: config.launch,
            launched: AtomicBool::new(false),
            launch_wait: config.launch_wait,
            client_info: std::sync::Mutex::new(None),
        })
    }

    /// The output lines for one input line.
    async fn handle_line(&self, line: &str) -> Vec<String> {
        let message: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return vec![error(Value::Null, -32700, &format!("parse error: {e}")).to_string()],
        };
        match message {
            // A JSON-RPC batch, which protocol versions before 2025-06-18
            // allow. The answers go back as one batch.
            Value::Array(items) => {
                let mut answers = Vec::new();
                for item in items {
                    answers.extend(self.handle_message(item).await);
                }
                if answers.is_empty() {
                    Vec::new()
                } else {
                    vec![Value::Array(answers).to_string()]
                }
            }
            single => self
                .handle_message(single)
                .await
                .into_iter()
                .map(|v| v.to_string())
                .collect(),
        }
    }

    /// The answers to one message: none for a notification, one for a request.
    async fn handle_message(&self, message: Value) -> Vec<Value> {
        if message.get("method").and_then(Value::as_str) == Some("initialize") {
            if let Some(info) = message.pointer("/params/clientInfo") {
                let field = |k: &str| info.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                *self.client_info.lock().unwrap_or_else(|e| e.into_inner()) = Some((field("name"), field("version")));
            }
        }
        match self.forward(&message).await {
            Ok(answers) => answers,
            Err(ForwardError::Down) => self.offline(&message).await,
            Err(ForwardError::Failed(text)) => match request_id(&message) {
                Some(id) => vec![error(id, INTERNAL_ERROR, &text)],
                None => {
                    eprintln!("guhit-mcp: {text}");
                    Vec::new()
                }
            },
        }
    }

    /// Answer while the app is not running.
    async fn offline(&self, message: &Value) -> Vec<Value> {
        let Some(id) = request_id(message) else {
            // A notification, or a response to a request the app sent.
            return Vec::new();
        };
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let local = match method {
            "initialize" => Some(to_value(&initialize_result(message))),
            "ping" => Some(json!({})),
            "tools/list" => Some(to_value(&crate::list_tools_result())),
            "resources/list" => Some(to_value(&crate::list_resources_result())),
            "resources/templates/list" => Some(json!({ "resourceTemplates": [] })),
            "prompts/list" => Some(json!({ "prompts": [] })),
            "server/discover" => Some(to_value(&DiscoverResult::from_server_info(
                ProtocolVersion::KNOWN_VERSIONS.to_vec(),
                crate::server_config(),
            ))),
            _ => None,
        };
        if let Some(result) = local {
            return vec![json!({ "jsonrpc": "2.0", "id": id, "result": result })];
        }

        if matches!(method, "tools/call" | "resources/read") && self.bring_up().await {
            match self.forward(message).await {
                Ok(answers) => return answers,
                Err(ForwardError::Failed(text)) => return vec![error(id, INTERNAL_ERROR, &text)],
                Err(ForwardError::Down) => {}
            }
        }
        if method == "tools/call" {
            // A tool-level error, so the model reads it and tells the user.
            let result = CallToolResult::error(vec![ContentBlock::text(NOT_OPEN)]);
            return vec![json!({ "jsonrpc": "2.0", "id": id, "result": to_value(&result) })];
        }
        vec![error(id, INTERNAL_ERROR, NOT_OPEN)]
    }

    /// Start the app if there is a way to, then wait for its port. True when
    /// the app is listening.
    async fn bring_up(&self) -> bool {
        if self.reachable().await {
            return true;
        }
        let Some(launch) = &self.launch else {
            return false;
        };
        if !self.launched.swap(true, Ordering::SeqCst) {
            eprintln!("guhit-mcp: Guhit Studio is not running; starting it");
            launch();
        }
        let deadline = tokio::time::Instant::now() + self.launch_wait;
        loop {
            if self.reachable().await {
                return true;
            }
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return false;
            }
            tokio::time::sleep((deadline - now).min(Duration::from_millis(250))).await;
        }
    }

    async fn reachable(&self) -> bool {
        let connect = tokio::net::TcpStream::connect(("127.0.0.1", self.port));
        matches!(
            tokio::time::timeout(Duration::from_secs(1), connect).await,
            Ok(Ok(_))
        )
    }

    /// POST one message to the app and read back the JSON-RPC messages in the
    /// answer: none for a notification, a JSON body or an SSE stream for a
    /// request.
    async fn forward(&self, message: &Value) -> Result<Vec<Value>, ForwardError> {
        let mut request = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", ACCEPT_BOTH);
        for (name, value) in protocol_headers(message) {
            request = request.header(name, value);
        }
        let client = self.client_info.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some((name, version)) = client.filter(|(n, _)| plain_header_value(n)) {
            request = request.header(crate::CLIENT_NAME_HEADER, name);
            if plain_header_value(&version) {
                request = request.header(crate::CLIENT_VERSION_HEADER, version);
            }
        }
        let response = match request.body(message.to_string()).send().await {
            Ok(r) => r,
            Err(e) if e.is_connect() => return Err(ForwardError::Down),
            Err(e) => return Err(ForwardError::Failed(format!("Guhit Studio did not answer: {e}"))),
        };
        let status = response.status();
        // "Allow agents" is off. A request gets the app's error below; a
        // notification gets nothing, as JSON-RPC wants.
        if status == reqwest::StatusCode::SERVICE_UNAVAILABLE && request_id(message).is_none() {
            return Ok(Vec::new());
        }
        let is_sse = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/event-stream"));
        let body = match response.bytes().await {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) => return Err(ForwardError::Failed(format!("Guhit Studio did not answer: {e}"))),
        };

        let messages = if is_sse {
            sse_messages(&body)
        } else {
            match serde_json::from_str::<Value>(&body) {
                Ok(Value::Array(items)) => items,
                Ok(v @ Value::Object(_)) => vec![v],
                _ => Vec::new(),
            }
        };
        if !messages.is_empty() {
            return Ok(messages);
        }
        if !status.is_success() || (request_id(message).is_some() && !body.trim().is_empty()) {
            let text = body.trim();
            return Err(ForwardError::Failed(if text.is_empty() {
                format!("Guhit Studio answered HTTP {status}")
            } else {
                format!("Guhit Studio answered HTTP {status}: {text}")
            }));
        }
        if request_id(message).is_some() {
            return Err(ForwardError::Failed(format!(
                "Guhit Studio answered HTTP {status} with no message"
            )));
        }
        Ok(Vec::new())
    }
}

/// The id of a request; `None` for a notification or a response.
fn request_id(message: &Value) -> Option<Value> {
    message.get("method")?;
    message.get("id").cloned()
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn to_value<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or_else(|_| json!({}))
}

/// The server's own `initialize` answer, with the client's protocol version
/// when it is one that still has `initialize`, else the newest that does.
fn initialize_result(message: &Value) -> rmcp::model::ServerConfig {
    let requested = message
        .pointer("/params/protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut config = crate::server_config();
    config.protocol_version = ProtocolVersion::KNOWN_VERSIONS
        .iter()
        .find(|v| v.as_str() == requested && v.as_str() <= ProtocolVersion::LATEST.as_str())
        .cloned()
        .unwrap_or(ProtocolVersion::LATEST);
    config
}

/// A client on protocol 2026-07-28 or later puts the version in each
/// request's `_meta`. Over HTTP the server then wants it in the
/// `MCP-Protocol-Version` header too, with `Mcp-Method` and `Mcp-Name`
/// (SEP-2243). A stdio client sends no headers, so derive them here.
fn protocol_headers(message: &Value) -> Vec<(&'static str, String)> {
    let Some(version) = message
        .get("params")
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get(META_PROTOCOL_VERSION))
        .and_then(Value::as_str)
    else {
        return Vec::new();
    };
    let mut headers = vec![("MCP-Protocol-Version", version.to_string())];
    if let Some(method) = message.get("method").and_then(Value::as_str) {
        headers.push(("Mcp-Method", method.to_string()));
        let key = match method {
            "tools/call" | "prompts/get" => Some("name"),
            "resources/read" | "resources/subscribe" | "resources/unsubscribe" => Some("uri"),
            "tasks/get" | "tasks/update" | "tasks/cancel" => Some("taskId"),
            _ => None,
        };
        let name = key.and_then(|k| message.get("params")?.get(k)?.as_str());
        // Names here are plain ASCII. One that is not would need the Base64
        // form; leaving it out gets the server's own clear refusal.
        if let Some(name) = name.filter(|n| plain_header_value(n)) {
            headers.push(("Mcp-Name", name.to_string()));
        }
    }
    headers
}

fn plain_header_value(v: &str) -> bool {
    !v.is_empty()
        && !v.starts_with([' ', '\t'])
        && !v.ends_with([' ', '\t'])
        && !v.starts_with("=?base64?")
        && v.bytes().all(|b| (0x20..=0x7e).contains(&b))
}

/// The JSON-RPC messages in an SSE body: every event's `data`, skipping the
/// empty priming event.
fn sse_messages(body: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut data = String::new();
    let mut flush = |data: &mut String| {
        if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(data) {
            out.push(v);
        }
        data.clear();
    };
    for line in body.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            flush(&mut data);
        } else if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }
    flush(&mut data);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_bodies_yield_their_messages() {
        let body = "id: 0\nretry: 3000\ndata:\n\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\r\n\r\n";
        let messages = sse_messages(body);
        assert_eq!(messages, vec![json!({"jsonrpc": "2.0", "id": 1, "result": {}})]);
    }

    #[test]
    fn initialize_echoes_a_version_that_has_initialize() {
        let ask = |v: &str| {
            let m = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": v}});
            initialize_result(&m).protocol_version.as_str().to_string()
        };
        assert_eq!(ask("2025-06-18"), "2025-06-18");
        assert_eq!(ask("2024-11-05"), "2024-11-05");
        assert_eq!(ask("1999-01-01"), ProtocolVersion::LATEST.as_str());
        assert_eq!(ask("2026-07-28"), ProtocolVersion::LATEST.as_str());
    }

    #[test]
    fn per_request_versions_become_headers() {
        assert!(protocol_headers(&json!({"method": "tools/list"})).is_empty());
        let m = json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "list_rooms", "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28"}}
        });
        assert_eq!(
            protocol_headers(&m),
            vec![
                ("MCP-Protocol-Version", "2026-07-28".to_string()),
                ("Mcp-Method", "tools/call".to_string()),
                ("Mcp-Name", "list_rooms".to_string()),
            ]
        );
    }
}
