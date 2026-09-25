//! Model Context Protocol server over `guhit_app::AppService`.
//!
//! This is the third transport over the same application service, next to the
//! Tauri shell and the dev bridge (docs/CONTRACT.md). It lets an MCP client
//! such as Claude Code, Codex or Cursor drive the open project in the running
//! desktop app: the client's own subscription pays for the model, and Guhit
//! never sees a key. See docs/MCP.md.
//!
//! - Transport: streamable HTTP at `/mcp`, mounted on an axum router.
//! - Binds 127.0.0.1 only, and the transport rejects a non-loopback `Host`.
//! - Every edit goes through `AppService::commit_if_revision`, so it is
//!   validated by the engine, autosaved, and one undo step with `Origin::Ai`.
//! - Every change wakes `AppService::watch_changes`, which the desktop shell
//!   forwards to the window as the Tauri event `doc_changed`.

pub mod tools;
mod text;

use std::sync::Arc;

use guhit_app::AppService;
use guhit_model::Element;
use rmcp::model::*;
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::{json, Value};

/// Port the desktop app serves MCP on unless `settings.json` says otherwise.
pub const DEFAULT_PORT: u16 = 1450;
/// Key in `<data_dir>/settings.json` that overrides the port.
pub const PORT_SETTING: &str = "mcp_port";

const RESOURCE_CURRENT: &str = "guhit://project/current";
const RESOURCE_CONVENTIONS: &str = "guhit://docs/conventions";
const RESOURCE_PH_DEFAULTS: &str = "guhit://docs/ph-defaults";

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

impl ServerHandler for GuhitMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new("guhit-studio", env!("CARGO_PKG_VERSION")))
        .with_instructions(text::INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = tools::definitions()
            .into_iter()
            .map(|d| {
                let mut tool = Tool::default();
                tool.name = d.name.into();
                tool.description = Some(d.description.into());
                tool.input_schema = to_schema(&d.schema);
                let mut annotations = ToolAnnotations::new();
                annotations.read_only_hint = Some(d.read_only);
                // Nothing here reaches outside this machine.
                annotations.open_world_hint = Some(false);
                if !d.read_only {
                    // Every edit is one undo step, and `delete_elements` and
                    // `delete_level` do remove work the user may want back.
                    annotations.destructive_hint =
                        Some(matches!(d.name, "delete_elements" | "delete_level"));
                }
                tool.annotations = Some(annotations);
                tool
            })
            .collect();
        Ok(no_cache!(ListToolsResult::with_all_items(tools)))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let args = request
            .arguments
            .map(Value::Object)
            .unwrap_or_else(|| json!({}));
        // A tool that did not work is a tool-level error, not a protocol
        // error: the model has to read the message to correct itself.
        let result = match tools::call(&self.app, &request.name, args).await {
            Ok(tools::Output::Json(v)) => CallToolResult::structured(v),
            Ok(tools::Output::Image { base64, mime }) => {
                CallToolResult::success(vec![ContentBlock::image(base64, mime)])
            }
            Err(tools::ToolFail(message)) => {
                CallToolResult::error(vec![ContentBlock::text(message)])
            }
        };
        Ok(result.into())
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(no_cache!(ListResourcesResult::with_all_items(vec![
            Resource::new(RESOURCE_CURRENT, "Current project")
                .with_description(
                    "Compact JSON summary of the project open in Guhit Studio right now: id, name, revision, totals, rooms with their areas, and review items with their status. Empty when no project is open.",
                )
                .with_mime_type("application/json"),
            Resource::new(RESOURCE_CONVENTIONS, "Guhit conventions")
                .with_description(
                    "Units, plan coordinates, wall joins, and the door and window flip conventions this server uses. Read this before drawing.",
                )
                .with_mime_type("text/markdown"),
            Resource::new(RESOURCE_PH_DEFAULTS, "Philippine defaults")
                .with_description(
                    "Default wall thickness, door and window sizes, level height, the built-in material ids and sensible room sizes for Philippine residential work.",
                )
                .with_mime_type("text/markdown"),
        ])))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let uri = request.uri.as_str();
        let contents = match uri {
            RESOURCE_CONVENTIONS => ResourceContents::text(text::CONVENTIONS, uri)
                .with_mime_type("text/markdown"),
            RESOURCE_PH_DEFAULTS => ResourceContents::text(text::PH_DEFAULTS, uri)
                .with_mime_type("text/markdown"),
            RESOURCE_CURRENT => {
                let summary = current_project(&self.app).await;
                ResourceContents::text(
                    serde_json::to_string_pretty(&summary).unwrap_or_else(|_| "{}".into()),
                    uri,
                )
                .with_mime_type("application/json")
            }
            other => {
                return Err(McpError::resource_not_found(
                    format!("unknown resource `{other}`"),
                    None,
                ))
            }
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
            "note": "no project is open. Call create_project or open_project.",
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
        "levels": project.levels.iter().map(|l| json!({"id": l.id, "name": l.name})).collect::<Vec<_>>(),
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

/// An axum router with the MCP endpoint at `/mcp`. Merge this into another
/// router to share a port, as the dev bridge does.
pub fn router(app: AppService) -> axum::Router {
    axum::Router::new().nest_service("/mcp", service(app))
}

/// Serve MCP on `127.0.0.1:port` until the process ends. Loopback only: the
/// listener never binds another interface, and the transport refuses a
/// request whose `Host` is not local, which blocks DNS rebinding.
pub async fn serve(app: AppService, port: u16) -> std::io::Result<()> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("guhit-mcp listening on http://{addr}/mcp");
    axum::serve(listener, router(app)).await
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
        assert!(instructions.contains("add_rect_room"));
        assert!(instructions.contains("list_review_items"));
        assert!(instructions.to_lowercase().contains("code compliance"));
        assert!(info.capabilities.tools.is_some());
        assert!(info.capabilities.resources.is_some());
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
}
