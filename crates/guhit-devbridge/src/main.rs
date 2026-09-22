//! Dev-only HTTP transport over `guhit_app::AppService`. See docs/CONTRACT.md,
//! "Dev bridge". The UI running in a plain browser talks to this instead of
//! the Tauri shell, so it exercises the real Rust engine.
//!
//! - `POST /ipc/<cmd>` with the args object as JSON body. 200 + result JSON,
//!   or 400 + `IpcError`. That includes `doc_revision`, the cheap poll the UI
//!   uses to notice changes made by the MCP server.
//! - `POST /mcp` is the Model Context Protocol endpoint, on this same port, so
//!   Claude Code can drive the bridge exactly as it drives the desktop app.
//!   See docs/MCP.md.
//! - `GET /health` -> `{"ok":true}`.
//! - CORS: any `http://localhost:*` (or `http://127.0.0.1:*`) origin.
//! - Binds 127.0.0.1 only. Requests from any other browser origin, or with a
//!   Host header that is not local, get 403. This blocks other web pages and
//!   DNS rebinding from driving the bridge.

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Request, State};
use axum::http::header::{
    HeaderValue, ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,
    ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_MAX_AGE, HOST, ORIGIN, VARY,
};
use axum::http::{Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use guhit_app::AppService;
use guhit_model::IpcError;
use serde_json::{json, Value};

/// Data URLs of up to 25 MB are base64 (4/3) plus JSON framing.
const MAX_BODY_BYTES: usize = 48 * 1024 * 1024;

struct Options {
    port: u16,
    data: PathBuf,
}

fn parse_args() -> Result<Options, String> {
    let mut opts = Options {
        port: 1430,
        data: PathBuf::from(".devdata"),
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let (key, inline) = match a.split_once('=') {
            Some((k, v)) => (k.to_string(), Some(v.to_string())),
            None => (a.clone(), None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            inline
                .clone()
                .or_else(|| it.next())
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match key.as_str() {
            "--port" => {
                let v = value("--port")?;
                opts.port = v.parse().map_err(|_| format!("bad port `{v}`"))?;
            }
            "--data" => opts.data = PathBuf::from(value("--data")?),
            "-h" | "--help" => {
                println!("usage: guhit-devbridge [--port 1430] [--data .devdata]");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(opts)
}

/// True for `http://localhost[:port]` and `http://127.0.0.1[:port]`.
fn is_local_origin(origin: &str) -> bool {
    let Some(rest) = origin.strip_prefix("http://") else {
        return false;
    };
    is_local_host(rest)
}

/// True for `localhost[:port]` and `127.0.0.1[:port]`.
fn is_local_host(host: &str) -> bool {
    let (name, port) = match host.rsplit_once(':') {
        Some((n, p)) => (n, Some(p)),
        None => (host, None),
    };
    let name_ok = name.eq_ignore_ascii_case("localhost") || name == "127.0.0.1";
    let port_ok = port.is_none_or(|p| !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()));
    name_ok && port_ok
}

fn forbidden(message: &str) -> Response {
    (StatusCode::FORBIDDEN, Json(IpcError::new("forbidden", message))).into_response()
}

/// Origin and Host checks plus CORS headers.
async fn guard(req: Request, next: Next) -> Response {
    let origin = req
        .headers()
        .get(ORIGIN)
        .map(|v| v.to_str().unwrap_or("<non-ascii>").to_string());
    let host_ok = req
        .headers()
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .is_none_or(is_local_host);
    if !host_ok {
        return forbidden("the dev bridge only answers on localhost");
    }
    if let Some(o) = &origin {
        if !is_local_origin(o) {
            return forbidden("origin not allowed: the dev bridge only serves http://localhost:*");
        }
    }

    let mut res = if req.method() == Method::OPTIONS {
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(req).await
    };

    let h = res.headers_mut();
    h.insert(VARY, HeaderValue::from_static("Origin"));
    if let Some(o) = origin.and_then(|o| HeaderValue::from_str(&o).ok()) {
        h.insert(ACCESS_CONTROL_ALLOW_ORIGIN, o);
        h.insert(ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST, OPTIONS"));
        h.insert(ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("content-type"));
        h.insert(ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("600"));
    }
    res
}

async fn health() -> Json<Value> {
    Json(json!({ "ok": true }))
}

async fn ipc(State(app): State<AppService>, Path(cmd): Path<String>, body: Bytes) -> Response {
    let args: Value = if body.iter().all(|b| b.is_ascii_whitespace()) {
        json!({})
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => {
                let err = IpcError::new("bad_args", format!("request body is not valid JSON: {e}"));
                return (StatusCode::BAD_REQUEST, Json(err)).into_response();
            }
        }
    };
    match app.handle(&cmd, args).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(e)).into_response(),
    }
}

async fn not_found() -> Response {
    let err = IpcError::new("not_found", "no such route. Use POST /ipc/<cmd> or GET /health");
    (StatusCode::NOT_FOUND, Json(err)).into_response()
}

#[tokio::main]
async fn main() {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("guhit-devbridge: {e}");
            eprintln!("usage: guhit-devbridge [--port 1430] [--data .devdata]");
            std::process::exit(2);
        }
    };
    if let Err(e) = std::fs::create_dir_all(&opts.data) {
        eprintln!("guhit-devbridge: cannot create data dir {}: {e}", opts.data.display());
        std::process::exit(1);
    }
    let data = std::fs::canonicalize(&opts.data).unwrap_or(opts.data.clone());
    // Sandboxed: any program on this machine can post here, so a caller does
    // not get to choose where an export is written.
    let app = AppService::new_sandboxed(data.clone());

    let router = Router::new()
        .route("/health", get(health))
        .route("/ipc/{cmd}", post(ipc))
        // Same port, same service, same open document: an MCP client and the
        // browser UI see each other's changes.
        .nest_service("/mcp", guhit_mcp::service(app.clone()))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn(guard))
        .with_state(app);

    let addr = SocketAddr::from(([127, 0, 0, 1], opts.port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("guhit-devbridge: cannot bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    println!("guhit-devbridge listening on http://{addr}  data: {}", data.display());
    if let Err(e) = axum::serve(listener, router).await {
        eprintln!("guhit-devbridge: server error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_rules() {
        assert!(is_local_origin("http://localhost:1420"));
        assert!(is_local_origin("http://localhost"));
        assert!(is_local_origin("http://127.0.0.1:5173"));
        assert!(!is_local_origin("https://localhost:1420"));
        assert!(!is_local_origin("http://localhost.evil.com"));
        assert!(!is_local_origin("http://localhost:1420.evil.com"));
        assert!(!is_local_origin("http://evil.com"));
        assert!(!is_local_origin("null"));
    }
}
