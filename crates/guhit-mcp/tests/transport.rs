//! The transports as an MCP client sees them: the HTTP endpoint (Accept
//! handling, the Host guard, the IPv6 loopback) and the stdio proxy, against
//! real servers on random ports.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use guhit_app::AppService;
use guhit_mcp::stdio::{run_proxy, ProxyConfig};
use serde_json::{json, Value};

fn app() -> (AppService, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    (AppService::new_sandboxed(dir.path().to_path_buf()), dir)
}

/// `router()` on 127.0.0.1 and a random port.
async fn serve_v4(app: AppService) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, guhit_mcp::router(app)).await });
    port
}

/// A port nothing listens on right now.
fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

async fn wait_for(port: u16) {
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("nothing came up on port {port}");
}

fn initialize(id: u64) -> Value {
    json!({
        "jsonrpc": "2.0", "id": id, "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "0"}
        }
    })
}

fn client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder().no_proxy().build().unwrap()
}

async fn post(url: &str, accept: Option<&str>, host: Option<&str>, body: &Value) -> (u16, String) {
    let mut req = client()
        .post(url)
        .header("Content-Type", "application/json")
        .body(body.to_string());
    if let Some(a) = accept {
        req = req.header("Accept", a);
    }
    if let Some(h) = host {
        req = req.header("Host", h);
    }
    let res = req.send().await.expect("request");
    let status = res.status().as_u16();
    (status, res.text().await.unwrap_or_default())
}

#[tokio::test]
async fn a_post_that_accepts_only_json_is_answered() {
    let (app, _dir) = app();
    let port = serve_v4(app).await;
    let url = format!("http://127.0.0.1:{port}/mcp");

    for accept in [Some("application/json"), None, Some("*/*"), Some("application/json, text/event-stream")] {
        let (status, body) = post(&url, accept, None, &initialize(1)).await;
        assert_eq!(status, 200, "Accept {accept:?}: {body}");
        let v: Value = serde_json::from_str(&body).expect("plain JSON answer");
        assert_eq!(v["result"]["serverInfo"]["name"], "guhit-studio");
    }

    // The Host guard is untouched.
    let (status, _) = post(&url, Some("application/json"), Some("evil.example:80"), &initialize(1)).await;
    assert_eq!(status, 403);
    let (status, _) = post(&url, None, Some(&format!("localhost:{port}")), &initialize(1)).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn the_ipv6_loopback_is_served_when_the_machine_has_one() {
    if std::net::TcpListener::bind("[::1]:0").is_err() {
        eprintln!("no IPv6 loopback on this machine; skipped");
        return;
    }
    let (app, _dir) = app();
    let port = free_port();
    tokio::spawn(guhit_mcp::serve(app, port));
    wait_for(port).await;
    // The IPv6 listener binds right after the IPv4 one.
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("::1", port)).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let (status, body) = post(&format!("http://[::1]:{port}/mcp"), None, None, &initialize(1)).await;
    assert_eq!(status, 200, "[::1]: {body}");
    let (status, body) = post(
        &format!("http://[::1]:{port}/mcp"),
        None,
        Some(&format!("localhost:{port}")),
        &initialize(1),
    )
    .await;
    assert_eq!(status, 200, "[::1] with Host localhost: {body}");
    let (status, body) = post(&format!("http://127.0.0.1:{port}/mcp"), None, None, &initialize(1)).await;
    assert_eq!(status, 200, "127.0.0.1: {body}");
}

/// Run the proxy over these input messages and return the output lines.
async fn proxy(port: u16, launch: Option<guhit_mcp::stdio::Launch>, wait: Duration, input: &[Value]) -> Vec<Value> {
    let text: String = input.iter().map(|m| format!("{m}\n")).collect();
    let mut out = Vec::new();
    let config = ProxyConfig { port, launch, launch_wait: wait };
    run_proxy(config, text.as_bytes(), &mut out).await.expect("proxy");
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not one JSON message per line: {l}: {e}")))
        .collect()
}

fn by_id(lines: &[Value], id: u64) -> &Value {
    lines
        .iter()
        .find(|l| l["id"] == id)
        .unwrap_or_else(|| panic!("no answer for id {id} in {lines:?}"))
}

#[tokio::test]
async fn the_stdio_proxy_forwards_to_the_running_app() {
    let (app, _dir) = app();
    let port = serve_v4(app.clone()).await;

    let lines = proxy(
        port,
        None,
        Duration::ZERO,
        &[
            initialize(1),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                   "params": {"name": "create_project", "arguments": {"name": "Via stdio", "template": "blank"}}}),
        ],
    )
    .await;

    assert_eq!(lines.len(), 3, "the notification gets no line: {lines:?}");
    assert_eq!(by_id(&lines, 1)["result"]["serverInfo"]["name"], "guhit-studio");
    assert_eq!(by_id(&lines, 1)["result"]["protocolVersion"], "2025-06-18");
    let tools = by_id(&lines, 2)["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), guhit_mcp::tool_list().len());
    let call = &by_id(&lines, 3)["result"];
    assert_ne!(call["isError"], true, "tool call failed: {call}");

    // The call reached the app's one open document.
    let summary = guhit_mcp::tools::call(&app, "get_project_summary", json!({})).await;
    assert!(summary.is_ok(), "no project open after create_project over stdio");

    // A notification on its own writes nothing at all.
    let lines = proxy(port, None, Duration::ZERO, &[json!({"jsonrpc": "2.0", "method": "notifications/initialized"})]).await;
    assert!(lines.is_empty(), "{lines:?}");
}

#[tokio::test]
async fn with_the_app_down_the_proxy_answers_the_handshake_itself() {
    let port = free_port();
    let launches = Arc::new(AtomicUsize::new(0));
    let counter = launches.clone();
    let launch: guhit_mcp::stdio::Launch = Box::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });

    let lines = proxy(
        port,
        Some(launch),
        Duration::from_millis(300),
        &[
            initialize(1),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "ping"}),
            json!({"jsonrpc": "2.0", "id": 4, "method": "resources/list"}),
            json!({"jsonrpc": "2.0", "id": 5, "method": "prompts/list"}),
            json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": {"name": "list_rooms", "arguments": {}}}),
            json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": "list_projects", "arguments": {}}}),
            json!({"jsonrpc": "2.0", "id": 8, "method": "resources/read", "params": {"uri": "guhit://project/current"}}),
        ],
    )
    .await;

    assert_eq!(lines.len(), 8, "{lines:?}");
    let init = &by_id(&lines, 1)["result"];
    assert_eq!(init["serverInfo"]["name"], "guhit-studio");
    assert_eq!(init["protocolVersion"], "2025-06-18");
    assert!(init["instructions"].as_str().unwrap().contains("MILLIMETERS"));
    assert!(init["capabilities"]["tools"].is_object());
    assert_eq!(
        by_id(&lines, 2)["result"]["tools"].as_array().unwrap().len(),
        guhit_mcp::tool_list().len()
    );
    assert_eq!(by_id(&lines, 3)["result"], json!({}));
    assert_eq!(by_id(&lines, 4)["result"]["resources"].as_array().unwrap().len(), 3);
    assert_eq!(by_id(&lines, 5)["result"]["prompts"], json!([]));
    for id in [6, 7] {
        let result = &by_id(&lines, id)["result"];
        assert_eq!(result["isError"], true, "{result}");
        assert_eq!(result["content"][0]["text"], guhit_mcp::stdio::NOT_OPEN);
    }
    assert_eq!(by_id(&lines, 8)["error"]["message"], guhit_mcp::stdio::NOT_OPEN);
    assert_eq!(launches.load(Ordering::SeqCst), 1, "launch is called once");
}

#[tokio::test]
async fn a_tool_call_starts_the_app_and_then_goes_through() {
    let (app, _dir) = app();
    let port = free_port();
    let launch: guhit_mcp::stdio::Launch = Box::new(move || {
        // Stands in for starting the desktop app: its MCP server comes up.
        tokio::spawn(guhit_mcp::serve(app.clone(), port));
    });

    let lines = proxy(
        port,
        Some(launch),
        Duration::from_secs(10),
        &[json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                 "params": {"name": "create_project", "arguments": {"name": "Woken", "template": "blank"}}})],
    )
    .await;

    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_ne!(lines[0]["result"]["isError"], true, "{}", lines[0]);
}

/// `_meta` of a protocol 2026-07-28 request, which has no `initialize`.
fn meta_2026() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": {"name": "test", "version": "0"}
    })
}

/// POST with the headers a 2026-07-28 client sends over HTTP.
async fn post_2026(url: &str, body: &Value, name: Option<&str>) -> (u16, Value) {
    let mut req = client()
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", body["method"].as_str().unwrap())
        .body(body.to_string());
    if let Some(n) = name {
        req = req.header("Mcp-Name", n);
    }
    let res = req.send().await.expect("request");
    let status = res.status().as_u16();
    let text = res.text().await.unwrap_or_default();
    (status, serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": text })))
}

#[tokio::test]
async fn both_protocol_generations_are_answered_over_http() {
    let (app, _dir) = app();
    let port = serve_v4(app).await;
    let url = format!("http://127.0.0.1:{port}/mcp");

    // Old style: the initialize handshake.
    let (status, body) = post(&url, None, None, &initialize(1)).await;
    assert_eq!(status, 200, "{body}");

    // 2026-07-28: no initialize, the version in every request's _meta and headers.
    let (status, discover) = post_2026(
        &url,
        &json!({"jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": {"_meta": meta_2026()}}),
        None,
    )
    .await;
    assert_eq!(status, 200, "{discover}");
    let versions = discover["result"]["supportedVersions"].as_array().expect("supportedVersions");
    assert!(versions.contains(&json!("2026-07-28")), "{discover}");

    let (status, list) = post_2026(
        &url,
        &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {"_meta": meta_2026()}}),
        None,
    )
    .await;
    assert_eq!(status, 200, "{list}");
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), guhit_mcp::tool_list().len());

    let (status, call) = post_2026(
        &url,
        &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {"name": "get_guide", "arguments": {"topic": "conventions"}, "_meta": meta_2026()}}),
        Some("get_guide"),
    )
    .await;
    assert_eq!(status, 200, "{call}");
    assert_ne!(call["result"]["isError"], true, "{call}");
    let guide = call["result"]["content"][0]["text"].as_str().unwrap_or_default();
    assert!(guide.contains("MILLIMETERS"), "{call}");

    // The version in _meta without the header is refused, which is why the
    // stdio proxy derives the headers from the message.
    let (status, _) = post(
        &url,
        None,
        None,
        &json!({"jsonrpc": "2.0", "id": 4, "method": "tools/list", "params": {"_meta": meta_2026()}}),
    )
    .await;
    assert_ne!(status, 200);
}

#[tokio::test]
async fn the_stdio_proxy_carries_a_2026_client_without_headers() {
    let (app, _dir) = app();
    let port = serve_v4(app).await;
    let lines = proxy(
        port,
        None,
        Duration::ZERO,
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": {"_meta": meta_2026()}}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {"_meta": meta_2026()}}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                   "params": {"name": "get_guide", "arguments": {"topic": "ph_defaults"}, "_meta": meta_2026()}}),
        ],
    )
    .await;
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(by_id(&lines, 1)["result"]["supportedVersions"].is_array(), "{lines:?}");
    assert_eq!(
        by_id(&lines, 2)["result"]["tools"].as_array().unwrap().len(),
        guhit_mcp::tool_list().len(),
        "{lines:?}"
    );
    assert_ne!(by_id(&lines, 3)["result"]["isError"], true, "{lines:?}");

    // With the app down, discovery is answered by the proxy itself.
    let lines = proxy(
        free_port(),
        None,
        Duration::ZERO,
        &[json!({"jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": {"_meta": meta_2026()}})],
    )
    .await;
    let versions = by_id(&lines, 1)["result"]["supportedVersions"].as_array().unwrap();
    assert!(versions.contains(&json!("2026-07-28")), "{lines:?}");
}

fn initialize_as(id: u64, name: &str) -> Value {
    let mut m = initialize(id);
    m["params"]["clientInfo"] = json!({"name": name, "version": "9.1"});
    m
}

async fn set_enabled(app: &AppService, enabled: bool) {
    app.handle("mcp_set_enabled", json!({ "enabled": enabled })).await.expect("mcp_set_enabled");
}

#[tokio::test]
async fn serve_reports_its_port_and_that_it_listens() {
    let (app, _dir) = app();
    let port = free_port();
    tokio::spawn(guhit_mcp::serve(app.clone(), port));
    wait_for(port).await;
    let status = app.mcp.status();
    assert!(status.listening);
    assert_eq!(status.port, port);
    assert_eq!(status.url, format!("http://127.0.0.1:{port}/mcp"));
}

#[tokio::test]
async fn with_agents_off_every_post_gets_503_and_the_message() {
    let (app, _dir) = app();
    let port = serve_v4(app.clone()).await;
    let url = format!("http://127.0.0.1:{port}/mcp");

    set_enabled(&app, false).await;
    let (status, body) = post(&url, None, None, &initialize(7)).await;
    assert_eq!(status, 503, "{body}");
    let v: Value = serde_json::from_str(&body).expect("JSON-RPC error body");
    assert_eq!(v["jsonrpc"], "2.0");
    assert_eq!(v["id"], 7);
    assert_eq!(v["error"]["code"], -32000);
    assert_eq!(
        v["error"]["message"],
        "Agents are turned off in Guhit Studio. Turn on Allow agents in the Connect agent dialog."
    );
    let (status, body) = post(&url, None, None, &json!({"jsonrpc": "2.0", "id": "abc", "method": "tools/list"})).await;
    assert_eq!(status, 503);
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["id"], "abc");
    let (status, body) = post(&url, None, None, &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await;
    assert_eq!(status, 503);
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["id"], Value::Null);
    assert!(app.mcp.status().last_client.is_none(), "a refused request is not a client");

    // On again: no restart needed.
    set_enabled(&app, true).await;
    let (status, body) = post(&url, None, None, &initialize(8)).await;
    assert_eq!(status, 200, "{body}");
}

#[tokio::test]
async fn the_last_client_is_the_one_that_named_itself() {
    let (app, _dir) = app();
    let port = serve_v4(app.clone()).await;
    let url = format!("http://127.0.0.1:{port}/mcp");

    let (status, _) = post(&url, None, None, &initialize_as(1, "Cursor")).await;
    assert_eq!(status, 200);
    let seen = app.mcp.status().last_client.expect("Cursor");
    assert_eq!((seen.name.as_str(), seen.version.as_str()), ("Cursor", "9.1"));

    // A later request that does not name itself keeps the name.
    let (status, _) = post(&url, None, None, &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
    assert_eq!(status, 200);
    assert_eq!(app.mcp.status().last_client.unwrap().name, "Cursor");

    // A 2026-07-28 request names its client in `_meta`.
    let (status, _) = post_2026(
        &url,
        &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": {"_meta": meta_2026()}}),
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(app.mcp.status().last_client.unwrap().name, "test");

    // A request the Host guard refuses is not recorded.
    let (status, _) = post(&url, None, Some("evil.example:80"), &initialize_as(4, "Evil")).await;
    assert_eq!(status, 403);
    assert_eq!(app.mcp.status().last_client.unwrap().name, "test");
}

#[tokio::test]
async fn with_agents_off_the_stdio_proxy_passes_the_error_through() {
    let (app, _dir) = app();
    let port = serve_v4(app.clone()).await;
    set_enabled(&app, false).await;
    let launches = Arc::new(AtomicUsize::new(0));
    let counter = launches.clone();
    let launch: guhit_mcp::stdio::Launch = Box::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });

    let lines = proxy(
        port,
        Some(launch),
        Duration::from_millis(300),
        &[
            initialize(1),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "list_rooms", "arguments": {}}}),
        ],
    )
    .await;

    assert_eq!(lines.len(), 3, "the notification gets no line: {lines:?}");
    for id in [1, 2, 3] {
        let error = &by_id(&lines, id)["error"];
        assert_eq!(error["code"], -32000, "{lines:?}");
        assert_eq!(error["message"], guhit_app::mcp::DISABLED_MESSAGE);
    }
    assert_eq!(launches.load(Ordering::SeqCst), 0, "the app is running, nothing is launched");
}

#[tokio::test]
async fn the_stdio_proxy_names_its_client_even_after_answering_initialize_itself() {
    let (app, _dir) = app();
    let port = free_port();
    let up = app.clone();
    let launch: guhit_mcp::stdio::Launch = Box::new(move || {
        tokio::spawn(guhit_mcp::serve(up.clone(), port));
    });

    // `initialize` while the app is down is answered by the proxy; the tool
    // call then starts the app and is the first message it sees.
    let lines = proxy(
        port,
        Some(launch),
        Duration::from_secs(10),
        &[
            initialize_as(1, "Claude Desktop"),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "get_guide", "arguments": {"topic": "conventions"}}}),
        ],
    )
    .await;
    assert_eq!(lines.len(), 2, "{lines:?}");
    let seen = app.mcp.status().last_client.expect("named through the headers");
    assert_eq!((seen.name.as_str(), seen.version.as_str()), ("Claude Desktop", "9.1"));
}
