//! Model Context Protocol server over `guhit_app::AppService`.
//!
//! This is the third transport over the same application service, next to the
//! Tauri shell and the dev bridge (docs/CONTRACT.md). It lets an MCP client
//! such as Claude Code, Codex or Cursor drive the open project in the running
//! desktop app: the client's own subscription pays for the model, and Guhit
//! never sees a key. See docs/MCP.md.
//!
//! - Transport: streamable HTTP at `/mcp`, mounted on an axum router.
//!   `/mcp/agent/<label>` names the agent; `?toolsets=services,visuals`
//!   lists the extended toolsets (MCP v2, docs/CONTRACT.md "MCP v2").
//! - Binds 127.0.0.1, and [::1] when the machine has it, and the transport
//!   rejects a non-loopback `Host`.
//! - Clients that only launch a command get `stdio`: a proxy that speaks
//!   newline-delimited JSON-RPC and forwards to the running app over HTTP.
//! - Every edit is staged and committed as one undo step with `Origin::Ai`
//!   (`stage`), through the agents registry for calls from an agent, so it
//!   is validated by the engine, autosaved and authored by that agent.
//! - Every change wakes `AppService::watch_changes`, which the desktop shell
//!   forwards to the window as the Tauri event `doc_changed`.

// A failed tool call carries its whole structured error (`fail::ToolFail`).
// Failures are rare and go straight back to the client, so their size on
// the stack does not matter.
#![allow(clippy::result_large_err)]

pub mod agent;
mod documents;
mod draft;
mod extended;
pub mod fail;
mod geo;
mod look;
mod permit;
mod session;
mod stage;
pub mod stdio;
mod text;
pub mod tools;
pub mod toolsets;
mod tours;
mod translate;
pub mod visuals;

use std::sync::Arc;

use guhit_app::agents::AgentHint;
use guhit_app::AppService;
use guhit_model::Element;
use rmcp::model::*;
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::{json, Value};

use crate::agent::CallContext;
use crate::toolsets::Toolsets;

/// Port the desktop app serves MCP on unless `settings.json` says otherwise.
pub const DEFAULT_PORT: u16 = 1450;
/// Key in `<data_dir>/settings.json` that overrides the port.
pub const PORT_SETTING: &str = "mcp_port";

const RESOURCE_CURRENT: &str = "guhit://project/current";
const RESOURCE_CONVENTIONS: &str = "guhit://docs/conventions";
const RESOURCE_PH_DEFAULTS: &str = "guhit://docs/ph-defaults";
const RESOURCE_WORKFLOWS: &str = "guhit://docs/workflows";
const RESOURCE_CATALOG: &str = "guhit://catalog";
const RESOURCE_MATERIALS: &str = "guhit://materials";
const RESOURCE_REVIEW: &str = "guhit://review";
const RESOURCE_HISTORY: &str = "guhit://history";
const TEMPLATE_PLAN: &str = "guhit://project/levels/{level}/plan.svg";

/// The MCP server. One per transport; they all share the one `AppService`,
/// so they all act on the single open document.
#[derive(Clone)]
pub struct GuhitMcp {
    app: AppService,
}

impl GuhitMcp {
    pub fn new(app: AppService) -> Self {
        Self { app }
    }
}

/// SEP-2549 cache hints. Everything here describes the project that is open
/// right now, so none of it may be cached or shared: ttl 0, private. Some
/// clients also reject a result that leaves these fields out.
macro_rules! no_cache {
    ($result:expr) => {
        $result.with_ttl_ms(0).with_cache_scope(CacheScope::Private)
    };
}

fn to_schema(schema: &Value) -> Arc<JsonObject> {
    Arc::new(schema.as_object().cloned().unwrap_or_default())
}

/// What `initialize` answers: capabilities, name, version and the
/// instructions. Shared with the stdio proxy, which answers `initialize`
/// itself while the app is not running.
pub fn server_config() -> ServerConfig {
    ServerConfig::new(ServerCapabilities::builder().enable_prompts().enable_resources().enable_tools().build())
        .with_server_info(Implementation::new("guhit-studio", env!("CARGO_PKG_VERSION")))
        .with_instructions(text::INSTRUCTIONS)
}

/// The tools of the core toolset, as `tools/list` lists them by default.
pub fn tool_list() -> Vec<Tool> {
    tool_list_for(&Toolsets::core())
}

/// The tools `toolsets` list, with their schemas and annotations.
pub fn tool_list_for(toolsets: &Toolsets) -> Vec<Tool> {
    let listed = toolsets.tools();
    tools::definitions()
        .into_iter()
        .filter(|d| listed.contains(&d.name))
        .map(|d| {
            let mut tool = Tool::default();
            tool.name = d.name.into();
            tool.description = Some(d.description.into());
            tool.input_schema = to_schema(&d.schema);
            let mut annotations = ToolAnnotations::new();
            annotations.read_only_hint = Some(d.read_only);
            // Only AI visualization (the image provider) and the live
            // session (chat, status) reach beyond this machine.
            annotations.open_world_hint = Some(d.open_world);
            if !d.read_only {
                // Every edit is one undo step; `delete` and `levels` (its
                // delete action) remove work the user may want back.
                annotations.destructive_hint = Some(matches!(d.name, "delete" | "levels"));
            }
            tool.annotations = Some(annotations);
            tool
        })
        .collect()
}

/// The `tools/list` result for the core toolset.
pub fn list_tools_result() -> ListToolsResult {
    list_tools_result_for(&Toolsets::core())
}

pub fn list_tools_result_for(toolsets: &Toolsets) -> ListToolsResult {
    no_cache!(ListToolsResult::with_all_items(tool_list_for(toolsets)))
}

/// The fixed resources, as `resources/list` lists them.
pub fn resource_list() -> Vec<Resource> {
    let json = "application/json";
    let md = "text/markdown";
    vec![
        Resource::new(RESOURCE_CURRENT, "Current project")
            .with_description("Compact JSON summary of the open project: id, name, revision, levels, totals, rooms with their areas, and review items with their status.")
            .with_mime_type(json),
        Resource::new(RESOURCE_CONVENTIONS, "Guhit conventions")
            .with_description("Units, plan coordinates, wall joins, openings, levels, stairs and slabs. Read this before drawing.")
            .with_mime_type(md),
        Resource::new(RESOURCE_PH_DEFAULTS, "Philippine defaults")
            .with_description("Default wall thickness, door and window sizes, level height, the built-in material ids and sensible room sizes for Philippine residential work.")
            .with_mime_type(md),
        Resource::new(RESOURCE_WORKFLOWS, "Guhit workflows")
            .with_description("How to draft a house with build, the verify loop (look, measure, check), plans from an image, services, the selection scope, renders, live sessions and agents.")
            .with_mime_type(md),
        Resource::new(RESOURCE_CATALOG, "Library").with_description("Every library item key with its name, size and mount.").with_mime_type(md),
        Resource::new(RESOURCE_MATERIALS, "Materials").with_description("The open project's material ids and names.").with_mime_type(json),
        Resource::new(RESOURCE_REVIEW, "Review items")
            .with_description("The open project's review items: suggestions to check, never compliance findings.")
            .with_mime_type(json),
        Resource::new(RESOURCE_HISTORY, "History").with_description("The shared undo history: who made each step and when.").with_mime_type(json),
    ]
}

pub fn list_resources_result() -> ListResourcesResult {
    no_cache!(ListResourcesResult::with_all_items(resource_list()))
}

pub fn resource_templates() -> Vec<ResourceTemplate> {
    vec![ResourceTemplate::new(TEMPLATE_PLAN, "Plan of a level")
        .with_description("The plan of one level (its id or name) drawn by the engine as SVG, with dimensions, short element ids, a grid and a scale bar. Needs no window.")
        .with_mime_type("image/svg+xml")]
}

pub fn list_resource_templates_result() -> ListResourceTemplatesResult {
    no_cache!(ListResourceTemplatesResult::with_all_items(resource_templates()))
}

pub fn prompt_list() -> Vec<Prompt> {
    text::PROMPTS
        .iter()
        .map(|p| {
            let args = p
                .args
                .iter()
                .map(|(name, desc, required)| PromptArgument::new(*name).with_description(*desc).with_required(*required))
                .collect();
            Prompt::new(p.name, Some(p.description), Some(args)).with_title(p.title)
        })
        .collect()
}

pub fn list_prompts_result() -> ListPromptsResult {
    no_cache!(ListPromptsResult::with_all_items(prompt_list()))
}

/// A prompt's messages, with its arguments filled in.
pub fn get_prompt(name: &str, args: &serde_json::Map<String, Value>) -> Result<GetPromptResult, McpError> {
    let p = text::PROMPTS
        .iter()
        .find(|p| p.name == name)
        .ok_or_else(|| McpError::invalid_params(format!("unknown prompt `{name}`"), None))?;
    let mut body = p.body.to_string();
    for (arg, _, required) in p.args {
        let v = args.get(*arg).and_then(Value::as_str).map(str::trim).unwrap_or("");
        if *required && v.is_empty() {
            return Err(McpError::invalid_params(format!("prompt `{name}` needs `{arg}`"), None));
        }
        body = body.replace(&format!("{{{arg}}}"), if v.is_empty() { "(not given)" } else { v });
    }
    let mut result = GetPromptResult::new(vec![PromptMessage::new_text(Role::User, body)]);
    result.description = Some(p.description.to_string());
    Ok(result)
}

/// What the HTTP request says about the caller and the toolsets it wants.
fn request_context(app: &AppService, context: &RequestContext<RoleServer>) -> CallContext {
    let parts = context.extensions.get::<axum::http::request::Parts>();
    let toolsets = parts
        .and_then(|p| p.uri.query())
        .and_then(Toolsets::from_query)
        .unwrap_or_else(|| Toolsets::parse(&app.mcp.toolsets().join(",")));
    let header = |name: &str| {
        parts
            .and_then(|p| p.headers.get(name))
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .filter(|v| !v.trim().is_empty())
    };
    // The client this request named, else the one its session or its
    // connection named. Never another connection's: a request whose
    // connection named no client has none.
    let session = parts.and_then(|p| p.extensions.get::<McpSession>()).cloned();
    let seen = parts.and_then(|p| p.extensions.get::<SeenClient>()).cloned().or_else(|| {
        if let Some(client) = session.as_ref().and_then(|s| session_client(&s.0)) {
            return Some(client);
        }
        let keys = parts.map(connection_keys).unwrap_or_default();
        keys.iter().find_map(|k| app.mcp.client_at(k)).map(|c| SeenClient { name: c.name, version: c.version })
    });
    let label = parts.and_then(|p| agent::label_from_path(p.uri.path()));
    let instance = header(agent::INSTANCE_HEADER);
    let hint = AgentHint {
        connection: hint_connection(session.as_ref(), label.as_deref(), instance.as_deref()),
        agent: None,
        label,
        instance,
        client_name: seen.as_ref().map(|s| s.name.clone()),
        client_version: seen.as_ref().map(|s| s.version.clone()),
    };
    CallContext { toolsets, hint: Some(hint), call: None }
}

impl ServerHandler for GuhitMcp {
    fn get_info(&self) -> ServerConfig {
        server_config()
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(list_tools_result_for(&request_context(&self.app, &context).toolsets))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let args = request.arguments.map(Value::Object).unwrap_or_else(|| json!({}));
        let ctx = request_context(&self.app, &context);
        // A tool that did not work is a tool-level error, not a protocol
        // error: the model has to read it to correct itself.
        let result = match tools::call_with(&self.app, &request.name, args, &ctx).await {
            Ok(tools::Output::Json(v)) => CallToolResult::structured(v),
            Ok(tools::Output::Text(text)) => CallToolResult::success(vec![ContentBlock::text(text)]),
            // The JSON as structured content and text, then the pictures.
            Ok(tools::Output::Rich { json, images }) => {
                let mut result = CallToolResult::structured(json);
                for picture in images {
                    result.content.push(ContentBlock::image(picture.base64, picture.mime));
                }
                result
            }
            Err(fail) => {
                let mut result = CallToolResult::structured_error(fail.structured());
                result.content = vec![ContentBlock::text(fail.display())];
                result
            }
        };
        Ok(result.into())
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(list_resources_result())
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(list_resource_templates_result())
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        Ok(list_prompts_result())
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, McpError> {
        Ok(get_prompt(&request.name, &request.arguments.unwrap_or_default())?.into())
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let uri = request.uri.as_str();
        let text = |t: String, mime: &str| ResourceContents::text(t, uri).with_mime_type(mime);
        let as_json = |v: Value| text(serde_json::to_string_pretty(&v).unwrap_or_else(|_| "{}".into()), "application/json");
        let tool_json = |out: Result<tools::Output, fail::ToolFail>| -> Result<ResourceContents, McpError> {
            match out {
                Ok(tools::Output::Json(v)) | Ok(tools::Output::Rich { json: v, .. }) => Ok(as_json(v)),
                Ok(tools::Output::Text(t)) => Ok(text(t, "text/markdown")),
                Err(e) => Err(McpError::resource_not_found(e.display(), None)),
            }
        };
        let contents = match uri {
            RESOURCE_CONVENTIONS => text(text::CONVENTIONS.to_string(), "text/markdown"),
            RESOURCE_PH_DEFAULTS => text(text::PH_DEFAULTS.to_string(), "text/markdown"),
            RESOURCE_WORKFLOWS => text(text::WORKFLOWS.to_string(), "text/markdown"),
            RESOURCE_CATALOG => text(draft::catalog_text(), "text/markdown"),
            RESOURCE_CURRENT => as_json(current_project(&self.app).await),
            RESOURCE_MATERIALS => tool_json(tools::call(&self.app, "query", json!({"what": "materials"})).await)?,
            RESOURCE_REVIEW => tool_json(tools::call(&self.app, "check", json!({})).await)?,
            RESOURCE_HISTORY => tool_json(tools::call(&self.app, "history", json!({"action": "list", "limit": 200})).await)?,
            other => match other.strip_prefix("guhit://project/levels/").and_then(|r| r.strip_suffix("/plan.svg")) {
                Some(level) => {
                    let level = agent::label_from_path(&format!("/agent/{level}")).unwrap_or_else(|| level.to_string());
                    let svg = look::plan_svg(&self.app, &level).await.map_err(|e| McpError::resource_not_found(e.display(), None))?;
                    text(svg, "image/svg+xml")
                }
                None => return Err(McpError::resource_not_found(format!("unknown resource `{other}`"), None)),
            },
        };
        Ok(no_cache!(ReadResourceResult::new(vec![contents])).into())
    }
}

/// Compact snapshot of the open document for `guhit://project/current`.
async fn current_project(app: &AppService) -> Value {
    let s = app.session.lock().await;
    let Some(doc) = s.doc.as_ref() else {
        return json!({
            "open": false,
            "note": "no project is open. Call project with action \"create\" or \"open\".",
        });
    };
    let project = doc.project();
    let derived = doc.derived();
    let rooms: Vec<Value> = derived
        .rooms
        .iter()
        .map(|g| {
            let name = project
                .elements
                .iter()
                .find_map(|e| match e {
                    Element::Room(r) if r.id == g.room_id => Some(r.name.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            json!({
                "id": g.room_id,
                "name": name,
                "area_m2": (g.area_mm2 / 1_000_000.0 * 10_000.0).round() / 10_000.0,
                "perimeter_mm": g.perimeter_mm,
                "wall_ids": g.wall_ids,
            })
        })
        .collect();
    json!({
        "open": true,
        "id": project.id,
        "name": project.name,
        "revision": doc.revision(),
        "units": "every length is millimeters; areas are square metres",
        "levels": project.levels.iter().map(|l| json!({"id": l.id, "name": l.name, "elevation_mm": l.elevation_mm, "height_mm": l.height_mm})).collect::<Vec<_>>(),
        "default_wall_thickness_mm": project.settings.default_wall_thickness_mm,
        "totals": derived.totals,
        "rooms": rooms,
        "review_items": derived.issues.iter().map(|i| json!({
            "id": i.id,
            "severity": i.severity,
            "code": i.code,
            "message": i.message,
            "status": i.status,
            "note": i.note,
        })).collect::<Vec<_>>(),
    })
}

// ---------------------------------------------------------------- transports

/// The MCP endpoint as a tower service. Handles POST, GET and DELETE on
/// whatever path it is mounted at.
pub fn service(app: AppService) -> StreamableHttpService<GuhitMcp, LocalSessionManager> {
    // Stateless, with a plain JSON response per request instead of an SSE
    // stream. There is no session to expire, so a client that comes back
    // after the app has been idle, or after the app restarted, just works.
    // The default allowed hosts are the loopback names only, which is the
    // transport's own guard against DNS rebinding.
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true);
    StreamableHttpService::new(
        move || Ok(GuhitMcp::new(app.clone())),
        Arc::new(LocalSessionManager::default()),
        config,
    )
}

/// What `normalize_accept` puts in place of an `Accept` header that does
/// not name both types.
const ACCEPT_BOTH: &str = "application/json, text/event-stream";

/// The transport answers a POST only when `Accept` names both
/// `application/json` and `text/event-stream`, and some clients send only the
/// first, or nothing. This server always answers a request with plain JSON
/// (`with_json_response`), so such a client can read the answer: fill in the
/// header instead of refusing it with 406. Nothing else changes; the `Host`
/// guard still runs.
pub async fn normalize_accept(
    mut req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::http::{header::ACCEPT, HeaderValue, Method};
    if req.method() == Method::POST {
        let accept = req
            .headers()
            .get(ACCEPT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !(accept.contains("application/json") && accept.contains("text/event-stream")) {
            req.headers_mut()
                .insert(ACCEPT, HeaderValue::from_static(ACCEPT_BOTH));
        }
    }
    next.run(req).await
}

/// Headers the stdio proxy adds to every message it forwards, with the
/// `clientInfo` its client sent in `initialize`. That `initialize` may have
/// been answered by the proxy itself while the app was closed, so the app
/// would otherwise never learn who is calling. Plain ASCII values only.
pub const CLIENT_NAME_HEADER: &str = "x-guhit-client-name";
pub const CLIENT_VERSION_HEADER: &str = "x-guhit-client-version";

/// What tells one MCP connection from another, most precise first: the
/// session the server issued, the label of a labelled URL, the stdio
/// proxy's instance, the TCP peer when the server knows it.
fn connection_keys(parts: &axum::http::request::Parts) -> Vec<String> {
    let mut keys = vec![];
    if let Some(s) = parts.extensions.get::<McpSession>() {
        keys.push(format!("s:{}", session_key(&s.0)));
    }
    if let Some(l) = agent::label_from_path(parts.uri.path()) {
        keys.push(format!("l:{}", l.to_lowercase()));
    }
    let instance = parts.headers.get(agent::INSTANCE_HEADER).and_then(|v| v.to_str().ok());
    if let Some(i) = instance.filter(|v| !v.trim().is_empty()) {
        keys.push(format!("i:{i}"));
    }
    if let Some(c) = parts.extensions.get::<axum::extract::ConnectInfo<std::net::SocketAddr>>() {
        keys.push(format!("p:{}", c.0));
    }
    keys
}

/// The session header: issued on `initialize`, sent back by the client on
/// every later request (Streamable HTTP). The transport stays stateless; the
/// id only tells connections apart.
pub const SESSION_HEADER: &str = "mcp-session-id";
/// Longest session id read back.
const MAX_SESSION_CHARS: usize = 300;

/// The session a request belongs to, put on the request.
#[derive(Debug, Clone)]
struct McpSession(String);

/// A new session id for a client: a random part and the client's name and
/// version in hex, so the id names its client even after the app restarted
/// (the client keeps sending it). Visible ASCII only, as the spec asks.
pub fn new_session_id(name: &str, version: &str) -> String {
    let random = guhit_model::defaults::new_id().replace('-', "");
    let client: String = format!("{}\n{}", name.chars().take(100).collect::<String>(), version.chars().take(40).collect::<String>())
        .bytes()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{random}.{client}")
}

/// The connection the registry keys an agent by (its pause, stop, limit
/// and rate): chosen by the server, never by tool arguments. The session it
/// issued, else the labelled URL, else the stdio instance.
fn hint_connection(session: Option<&McpSession>, label: Option<&str>, instance: Option<&str>) -> Option<String> {
    match (session, label, instance) {
        (Some(s), _, _) => Some(format!("session:{}", session_key(&s.0))),
        (None, Some(l), _) => Some(format!("label:{}", l.to_lowercase())),
        (None, None, Some(i)) => Some(format!("instance:{i}")),
        _ => None,
    }
}

/// The random part of a session id: what keys the connection.
fn session_key(id: &str) -> &str {
    id.split('.').next().unwrap_or(id)
}

/// The client a session id names.
fn session_client(id: &str) -> Option<SeenClient> {
    let hex = id.split_once('.')?.1;
    if hex.len() % 2 != 0 {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok()).collect();
    let text = String::from_utf8(bytes?).ok()?;
    let (name, version) = text.split_once('\n')?;
    (!name.trim().is_empty()).then(|| SeenClient { name: name.to_string(), version: version.to_string() })
}

/// The client a request named, put on the request for the tool call.
#[derive(Debug, Clone)]
struct SeenClient {
    name: String,
    version: String,
}

/// Largest request body [`gate`] reads. Tool arguments are small; this only
/// stops a runaway client from filling memory.
const MAX_REQUEST_BYTES: usize = 64 * 1024 * 1024;
/// JSON-RPC error code of the answer while agents are turned off.
const DISABLED_CODE: i64 = -32000;

/// The `clientInfo` a message carries: in `initialize` params, or in
/// `_meta` of a protocol 2026-07-28 request (SEP-2575, optional there).
fn client_info(message: &Value) -> Option<(String, String)> {
    let params = message.get("params")?;
    let info = params
        .get("clientInfo")
        .filter(|_| message.get("method").and_then(Value::as_str) == Some("initialize"))
        .or_else(|| params.get("_meta")?.get("io.modelcontextprotocol/clientInfo"))?;
    let name = info.get("name")?.as_str()?.to_string();
    let version = info.get("version").and_then(Value::as_str).unwrap_or("").to_string();
    Some((name, version))
}

/// In front of the MCP service:
/// - "Allow agents" off (DECISIONS D35): every request gets HTTP 503 and a
///   JSON-RPC error with `AppService`'s message, its id echoed when the
///   body has one. The switch is read on every request, so turning it back
///   on works at once.
/// - On: notes the client that called, for the Connect agent dialog and the
///   agents registry. Only after the service accepted the request, so a
///   request the `Host` guard refuses never shows up. The body goes on to
///   the service unchanged.
pub async fn gate(
    axum::extract::State(app): axum::extract::State<AppService>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::http::{Method, StatusCode};
    use axum::response::IntoResponse;

    if req.method() != Method::POST {
        if !app.mcp.enabled() {
            return agents_off(Value::Null);
        }
        return next.run(req).await;
    }
    let (mut parts, body) = req.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX_REQUEST_BYTES).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    let parsed: Option<Value> = serde_json::from_slice(&bytes).ok();

    if !app.mcp.enabled() {
        let id = parsed
            .as_ref()
            .and_then(|m| m.get("id"))
            .cloned()
            .unwrap_or(Value::Null);
        return agents_off(id);
    }

    let header = |name: &str| {
        parts
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let from_proxy = header(CLIENT_NAME_HEADER).map(|n| (n, header(CLIENT_VERSION_HEADER).unwrap_or_default()));
    let messages: Vec<&Value> = match &parsed {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(v @ Value::Object(_)) => vec![v],
        _ => Vec::new(),
    };
    let named = messages.iter().find_map(|m| client_info(m)).or(from_proxy);
    let is_message = messages.iter().any(|m| m.get("method").is_some());
    if let Some((name, version)) = &named {
        parts.extensions.insert(SeenClient { name: name.clone(), version: version.clone() });
    }
    // `initialize` starts a session; every later request carries its id.
    let initialize = messages.iter().any(|m| m.get("method").and_then(Value::as_str) == Some("initialize"));
    let issued = initialize.then(|| {
        let (name, version) = named.clone().unwrap_or_default();
        new_session_id(&name, &version)
    });
    let session = issued.clone().or_else(|| {
        header(SESSION_HEADER)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty() && v.len() <= MAX_SESSION_CHARS && v.bytes().all(|b| (0x21..=0x7e).contains(&b)))
    });
    if let Some(id) = &session {
        parts.extensions.insert(McpSession(id.clone()));
    }
    let keys = connection_keys(&parts);

    let mut res = next
        .run(axum::extract::Request::from_parts(parts, axum::body::Body::from(bytes)))
        .await;
    if let (Some(id), true) = (&issued, res.status().is_success()) {
        if let Ok(value) = axum::http::HeaderValue::from_str(id) {
            res.headers_mut().insert(SESSION_HEADER, value);
        }
    }
    if res.status().is_success() {
        match named {
            // Under every key the request has: a later request may carry
            // only its label, or only its session.
            Some((name, version)) if !keys.is_empty() => {
                for key in &keys {
                    app.mcp.saw_client_at(Some(key), &name, &version);
                }
            }
            Some((name, version)) => app.mcp.saw_client(&name, &version),
            None if is_message => app.mcp.touch_client(),
            None => {}
        }
    }
    res
}

/// HTTP 503 with the JSON-RPC error for "Allow agents" off.
fn agents_off(id: Value) -> axum::response::Response {
    use axum::http::{header::CONTENT_TYPE, HeaderValue, StatusCode};
    use axum::response::IntoResponse;
    let body = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": DISABLED_CODE, "message": guhit_app::mcp::DISABLED_MESSAGE },
    });
    let mut res = (StatusCode::SERVICE_UNAVAILABLE, body.to_string()).into_response();
    res.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    res
}

/// The MCP endpoint with [`gate`] and the `Accept` fix in front of it, as a
/// router that answers on every path. Nest it wherever the endpoint should
/// live, as [`router`] and the dev bridge do.
pub fn endpoint(app: AppService) -> axum::Router {
    axum::Router::new()
        .fallback_service(service(app.clone()))
        .layer(axum::middleware::from_fn(normalize_accept))
        .layer(axum::middleware::from_fn_with_state(app, gate))
}

/// An axum router with the MCP endpoint at `/mcp`. Merge this into another
/// router to share a port.
pub fn router(app: AppService) -> axum::Router {
    axum::Router::new().nest_service("/mcp", endpoint(app))
}

/// Serve MCP on `127.0.0.1:port` and `[::1]:port` until the process ends.
/// Loopback only: the listeners never bind another interface, and the
/// transport refuses a request whose `Host` is not local, which blocks DNS
/// rebinding.
///
/// Some clients resolve `localhost` to `::1` first and do not fall back to
/// 127.0.0.1, so the IPv6 loopback is served too. It is best effort: a
/// machine without it keeps running on IPv4. A failed IPv4 bind is the error.
///
/// Reports the port, and whether it is bound, to `AppService::mcp` for the
/// Connect agent dialog.
pub async fn serve(app: AppService, port: u16) -> std::io::Result<()> {
    let status = app.mcp.clone();
    status.set_port(port);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    status.set_listening(true);
    let router = router(app);
    let v6 = std::net::SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, port));
    match tokio::net::TcpListener::bind(v6).await {
        Ok(listener6) => {
            let router6 = router.clone();
            tokio::spawn(async move {
                let by_peer = router6.into_make_service_with_connect_info::<std::net::SocketAddr>();
                if let Err(e) = axum::serve(listener6, by_peer).await {
                    eprintln!("guhit-mcp: the IPv6 listener on {v6} stopped: {e}");
                }
            });
            println!("guhit-mcp listening on http://{addr}/mcp and http://{v6}/mcp");
        }
        Err(e) => {
            eprintln!("guhit-mcp: not serving {v6} ({e}); IPv4 only");
            println!("guhit-mcp listening on http://{addr}/mcp");
        }
    }
    // The peer address keys each connection's client (`connection_keys`).
    let by_peer = router.into_make_service_with_connect_info::<std::net::SocketAddr>();
    let result = axum::serve(listener, by_peer).await;
    status.set_listening(false);
    result
}

/// Port from `<data_dir>/settings.json` key `mcp_port`, or [`DEFAULT_PORT`].
/// A missing file, unreadable file or bad value all mean the default: the MCP
/// server must never stop the app from starting.
pub fn port_from_settings(data_dir: &std::path::Path) -> u16 {
    std::fs::read_to_string(data_dir.join("settings.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get(PORT_SETTING).cloned())
        .and_then(|v| v.as_u64())
        .filter(|p| *p > 0 && *p <= u16::MAX as u64)
        .map(|p| p as u16)
        .unwrap_or(DEFAULT_PORT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instructions_teach_the_unit_and_the_honesty_rule() {
        let info = GuhitMcp::new(AppService::new_sandboxed(std::env::temp_dir().join("guhit-mcp-info")))
            .get_info();
        let instructions = info.instructions.unwrap_or_default();
        assert!(instructions.contains("MILLIMETERS"));
        assert!(instructions.contains("+x east"));
        assert!(instructions.contains("build"));
        assert!(instructions.contains("check"));
        assert!(instructions.to_lowercase().contains("code compliance"));
        assert!(info.capabilities.tools.is_some());
        assert!(info.capabilities.resources.is_some());
        assert!(info.capabilities.prompts.is_some());
    }

    /// Claude Code keeps the first 4096 characters of the instructions and
    /// drops the rest. What protects the user must never be in the part
    /// that is dropped: the honesty rules sit in the first 1500 characters,
    /// the consent rules right after them, and the whole text stays well
    /// under the cut.
    #[test]
    fn instructions_fit_the_client_cut_with_the_rules_first() {
        let text = text::INSTRUCTIONS;
        let chars = text.chars().count();
        assert!(chars <= 4000, "the instructions are {chars} characters; keep them under 4000 (Claude Code cuts at 4096)");
        let at = |needle: &str| {
            text.find(needle)
                .map(|i| text[..i].chars().count())
                .unwrap_or_else(|| panic!("the instructions lost `{needle}`"))
        };
        for needle in [
            "Honesty rules:",
            "permit approval, structural certification or code compliance",
            "never estimate them",
            "never work around a refusal",
            "is never approved",
        ] {
            let i = at(needle);
            assert!(i + needle.len() <= 1500, "`{needle}` ends at character {}, past the first 1500", i + needle.len());
        }
        let consent = at("Ask the user first:");
        assert!(consent > at("Honesty rules:"));
        for needle in [
            "Pass `force` only after the user confirms",
            "costs the user money",
            "only when the user names it",
        ] {
            assert!(at(needle) < 2500, "`{needle}` must come early");
        }
        assert!(at("Ask the user first:") < at("Units:"), "consent comes before drawing advice");
    }

    #[test]
    fn port_falls_back_to_the_default() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(port_from_settings(dir.path()), DEFAULT_PORT);
        std::fs::write(dir.path().join("settings.json"), r#"{"mcp_port": 1460}"#).unwrap();
        assert_eq!(port_from_settings(dir.path()), 1460);
        std::fs::write(dir.path().join("settings.json"), r#"{"mcp_port": "nope"}"#).unwrap();
        assert_eq!(port_from_settings(dir.path()), DEFAULT_PORT);
    }

    #[test]
    fn the_list_follows_the_toolsets() {
        assert_eq!(tool_list().len(), 26);
        assert_eq!(tool_list_for(&Toolsets::all()).len(), 45);
        assert_eq!(tool_list_for(&Toolsets::parse("live")).len(), 29);
        assert_eq!(tool_list_for(&Toolsets::parse("documents,tours")).len(), 33);
        let names: Vec<String> = tool_list().iter().map(|t| t.name.to_string()).collect();
        assert!(!names.iter().any(|n| n == "add_rect_room"), "v1 names are never listed");
        // About 1.7 KB a tool: every one is read on every turn.
        let bytes = serde_json::to_string(&tool_list()).unwrap().len();
        let mut sizes: Vec<(usize, String)> =
            tool_list().iter().map(|t| (serde_json::to_string(t).unwrap().len(), t.name.to_string())).collect();
        sizes.sort_by(|a, b| b.cmp(a));
        assert!(bytes < 44_000, "the core tool list is {bytes} bytes; the largest: {:?}", &sizes[..6]);
    }

    #[test]
    fn prompts_fill_their_arguments() {
        assert_eq!(prompt_list().len(), 4);
        let mut args = serde_json::Map::new();
        args.insert("brief".into(), json!("a two storey house for a family of five"));
        let p = get_prompt("draft_from_brief", &args).unwrap();
        let text = serde_json::to_string(&p.messages).unwrap();
        assert!(text.contains("family of five"));
        assert!(text.contains("check"));
        assert!(get_prompt("draft_from_brief", &serde_json::Map::new()).is_err());
        assert!(get_prompt("nope", &args).is_err());
    }
}
