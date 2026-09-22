//! Desktop shell. A thin transport: one Tauri command forwards every call to
//! `guhit_app::AppService::handle`. No app logic lives here.
//!
//! The shell also hosts two things that need the running process:
//! - the MCP server on 127.0.0.1, so Claude Code and other MCP clients can
//!   drive the open project (docs/MCP.md),
//! - a forwarder that turns every document change into the Tauri event
//!   `doc_changed`, so the window refreshes after an edit it did not make.

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
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
