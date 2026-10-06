//! Desktop shell. A thin transport: one Tauri command forwards every call to
//! `guhit_app::AppService::handle`. No app logic lives here.
//!
//! The shell also hosts two things that need the running process:
//! - the MCP server on 127.0.0.1, so Claude Code and other MCP clients can
//!   drive the open project (docs/MCP.md),
//! - a forwarder that turns every document change into the Tauri event
//!   `doc_changed`, so the window refreshes after an edit it did not make.
//!
//! `guhit-studio --mcp-stdio [--port N]` is a second mode for MCP clients that
//! only launch a command: no window, just the stdio proxy to the running
//! app's MCP endpoint, which starts the app on the first tool call.

use guhit_app::AppService;
use guhit_model::IpcError;
use serde_json::Value;
use tauri::{Emitter, Manager};

#[tauri::command]
async fn ipc(app: tauri::State<'_, AppService>, cmd: String, args: Value) -> Result<Value, IpcError> {
    let result = app.handle(&cmd, args).await;
    // Dev builds trace every call, so `pnpm tauri dev` shows the webview talking to Rust.
    #[cfg(debug_assertions)]
    eprintln!("ipc {cmd} -> {}", if result.is_ok() { "ok" } else { "error" });
    result
}

/// Forward every document change to the window as `doc_changed`. The window
/// may not exist yet, or may have been closed: a failed emit is not an error,
/// the next change tries again.
fn forward_changes(handle: tauri::AppHandle, service: &AppService) {
    let mut changes = service.watch_changes();
    tauri::async_runtime::spawn(async move {
        while changes.changed().await.is_ok() {
            let revision = changes.borrow_and_update().revision;
            let _ = handle.emit_to("main", "doc_changed", serde_json::json!({ "revision": revision }));
        }
    });
}

/// Serve MCP on 127.0.0.1. A port that is already taken, or any other bind
/// failure, is printed and ignored: the desktop app must still start.
fn start_mcp(service: AppService, data_dir: std::path::PathBuf) {
    let port = guhit_mcp::port_from_settings(&data_dir);
    tauri::async_runtime::spawn(async move {
        if let Err(e) = guhit_mcp::serve(service, port).await {
            eprintln!(
                "guhit-studio: the MCP server could not start on 127.0.0.1:{port}: {e}. \
                 The app runs normally; set \"{}\" in settings.json to use another port.",
                guhit_mcp::PORT_SETTING
            );
        }
    });
}

/// The arguments of `--mcp-stdio` mode: `None` when it was not asked for,
/// else the `--port` given, if any.
fn mcp_stdio_args() -> Result<Option<Option<u16>>, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.iter().any(|a| a == "--mcp-stdio") {
        return Ok(None);
    }
    let mut port = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--mcp-stdio" => {}
            "--port" => {
                let v = it.next().ok_or("--port needs a value")?;
                port = Some(v.parse::<u16>().ok().filter(|p| *p > 0).ok_or(format!("bad port `{v}`"))?);
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(Some(port))
}

/// The folder Tauri's `app_data_dir()` resolves to: the platform data dir
/// (`~/Library/Application Support` on macOS, `%APPDATA%` on Windows) plus
/// the bundle identifier. Worked out here because `--mcp-stdio` never starts
/// Tauri. `dirs` is the crate Tauri resolves it with, already in Cargo.lock.
fn app_data_dir_without_tauri() -> Option<std::path::PathBuf> {
    let conf: Value = serde_json::from_str(include_str!("../tauri.conf.json")).ok()?;
    let identifier = conf.get("identifier")?.as_str()?;
    Some(dirs::data_dir()?.join(identifier))
}

/// Start this app as its own process, detached from the MCP client: a new
/// process group, so the client's Ctrl-C or exit does not take it down, and
/// no inherited stdio, so it never writes into the JSON-RPC stream.
fn launch_detached() {
    use std::process::{Command, Stdio};
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            eprintln!("guhit-studio: cannot find this executable to start the app: {e}");
            return;
        }
    };
    let mut cmd = Command::new(exe);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    match cmd.spawn() {
        // Reap it when it exits, so it never lingers as a zombie of the proxy.
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => eprintln!("guhit-studio: could not start the app: {e}"),
    }
}

/// `--mcp-stdio` mode. Never opens a window; exits when stdin closes.
fn run_mcp_stdio(port: Option<u16>) -> ! {
    let port = port
        .or_else(|| app_data_dir_without_tauri().map(|d| guhit_mcp::port_from_settings(&d)))
        .unwrap_or(guhit_mcp::DEFAULT_PORT);
    let launch: guhit_mcp::stdio::Launch = Box::new(launch_detached);
    let result = tauri::async_runtime::block_on(guhit_mcp::stdio::run_stdio_proxy(port, Some(launch)));
    match result {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("guhit-studio: MCP stdio proxy: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    match mcp_stdio_args() {
        Ok(None) => {}
        Ok(Some(port)) => run_mcp_stdio(port),
        Err(e) => {
            eprintln!("guhit-studio: {e}");
            eprintln!("usage: guhit-studio --mcp-stdio [--port 1450]");
            std::process::exit(2);
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // Auto-update from GitHub releases. The frontend drives it
            // (src/shell/UpdateNotice.tsx); this only registers the plugin.
            // Desktop only: there is no updater on mobile targets.
            #[cfg(desktop)]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;

            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let service = AppService::new(data_dir.clone());
            forward_changes(app.handle().clone(), &service);
            start_mcp(service.clone(), data_dir);
            app.manage(service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![ipc])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
