//! The MCP server's status, the "Allow agents" switch and the Claude Desktop
//! extension file, for the Connect agent dialog (DECISIONS D35).
//!
//! The server itself is in `guhit-mcp`, which depends on this crate. The
//! transports report what they serve through [`McpState`] (shared by every
//! clone of `AppService`), and the MCP endpoint asks it whether agents are
//! allowed and tells it which client called.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use guhit_model::*;
use serde_json::{json, Value};

use crate::files;
use crate::{arg, to_value, AppService, IpcResult};

/// Commands this module answers. `AppService::handle` routes on this list.
pub const OWNS: [&str; 3] = ["mcp_status", "mcp_set_enabled", "mcp_claude_desktop_bundle"];

/// Key in `settings.json` for the "Allow agents" switch. Missing means on.
pub const ENABLED_KEY: &str = "mcp_enabled";
/// What every MCP request gets while agents are turned off.
pub const DISABLED_MESSAGE: &str =
    "Agents are turned off in Guhit Studio. Turn on Allow agents in the Connect agent dialog.";
/// Folder under the data dir for the Claude Desktop extension file.
pub const CLAUDE_DESKTOP_DIR: &str = "claude-desktop";
/// The Claude Desktop extension file (an MCP Bundle).
pub const CLAUDE_DESKTOP_FILE: &str = "guhit-studio.mcpb";
/// MCPB manifest spec version the extension follows.
const MCPB_MANIFEST_VERSION: &str = "0.3";
/// Longest client name or version kept, in characters.
const MAX_CLIENT_FIELD_CHARS: usize = 100;
/// The extension's icon: the app icon, 1024 px PNG.
const ICON_PNG: &[u8] = include_bytes!("../../../assets/brand/icon-1024.png");

/// What the transports report. One per `AppService`, shared by its clones.
pub struct McpState {
    enabled: AtomicBool,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    listening: bool,
    port: u16,
    stdio_command: Option<PathBuf>,
    last_client: Option<McpClientSeen>,
}

impl McpState {
    /// The switch as `settings.json` has it, on unless it says `false`.
    pub(crate) fn for_data_dir(data_dir: &Path) -> Self {
        let enabled = files::read_settings(data_dir)
            .get(ENABLED_KEY)
            .and_then(Value::as_bool)
            .unwrap_or(true);
        Self {
            enabled: AtomicBool::new(enabled),
            inner: Mutex::new(Inner::default()),
        }
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// True while agents are allowed. Checked on every MCP request.
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// The port MCP serves on, or is about to try.
    pub fn set_port(&self, port: u16) {
        self.inner().port = port;
    }

    /// Whether the listener is bound right now.
    pub fn set_listening(&self, listening: bool) {
        self.inner().listening = listening;
    }

    /// The desktop executable, which runs the stdio entry with `--mcp-stdio`.
    pub fn set_stdio_command(&self, path: PathBuf) {
        self.inner().stdio_command = Some(path);
    }

    pub fn stdio_command(&self) -> Option<PathBuf> {
        self.inner().stdio_command.clone()
    }

    /// A client named itself (`clientInfo`): it is the last client now.
    pub fn saw_client(&self, name: &str, version: &str) {
        let name = clean_field(name);
        if name.is_empty() {
            return;
        }
        self.inner().last_client = Some(McpClientSeen {
            name,
            version: clean_field(version),
            at: defaults::now_rfc3339(),
        });
    }

    /// A request that did not name its client: over stateless HTTP it is
    /// taken to come from the last client that did.
    pub fn touch_client(&self) {
        if let Some(client) = self.inner().last_client.as_mut() {
            client.at = defaults::now_rfc3339();
        }
    }

    pub fn status(&self) -> McpStatus {
        let inner = self.inner();
        McpStatus {
            enabled: self.enabled(),
            listening: inner.listening,
            port: inner.port,
            url: format!("http://127.0.0.1:{}/mcp", inner.port),
            stdio_command: inner.stdio_command.as_ref().map(|p| p.display().to_string()),
            last_client: inner.last_client.clone(),
            claude_desktop_bundle: inner.stdio_command.is_some(),
        }
    }
}

/// Control characters out, spaces trimmed, at most `MAX_CLIENT_FIELD_CHARS`.
/// A client names itself; the dialog shows it as text.
fn clean_field(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(MAX_CLIENT_FIELD_CHARS)
        .collect()
}

pub async fn handle(app: &AppService, cmd: &str, args: Value) -> IpcResult {
    match cmd {
        "mcp_status" => to_value(&app.mcp.status()),
        "mcp_set_enabled" => {
            let enabled: bool = arg(&args, "enabled")?;
            let dir = app.session.lock().await.data_dir.clone();
            files::put_setting(&dir, ENABLED_KEY, Some(json!(enabled)))?;
            app.mcp.enabled.store(enabled, Ordering::SeqCst);
            to_value(&app.mcp.status())
        }
        "mcp_claude_desktop_bundle" => {
            let command = app.mcp.stdio_command().ok_or_else(|| {
                IpcError::new(
                    "unavailable",
                    "The Claude Desktop extension needs the desktop app. Open Guhit Studio itself, not the browser preview.",
                )
            })?;
            let dir = app.session.lock().await.data_dir.join(CLAUDE_DESKTOP_DIR);
            let path = dir.join(CLAUDE_DESKTOP_FILE);
            files::write_atomic(&path, &claude_desktop_bundle(&command)?)?;
            Ok(json!({ "path": path.display().to_string() }))
        }
        _ => Err(IpcError::new("unknown_command", format!("unknown command `{cmd}`"))),
    }
}

// ------------------------------------------------- Claude Desktop extension

/// The command Claude Desktop runs. Its MCPB spec says it appends `.exe` to a
/// binary's command on Windows, so the `.exe` is left off there; Windows
/// finds `guhit-studio.exe` from `guhit-studio` either way.
fn bundle_command(exe: &Path) -> String {
    let text = exe.display().to_string();
    if cfg!(windows) {
        if let Some(stem) = text.strip_suffix(".exe").or_else(|| text.strip_suffix(".EXE")) {
            return stem.to_string();
        }
    }
    text
}

/// `manifest.json` of the extension (MCPB manifest 0.3). A binary server
/// whose command is the installed app with `--mcp-stdio`: the bundle holds no
/// server of its own, because the stdio entry is a proxy to the running app.
pub fn claude_desktop_manifest(exe: &Path) -> Value {
    let command = bundle_command(exe);
    json!({
        "manifest_version": MCPB_MANIFEST_VERSION,
        "name": "guhit-studio",
        "display_name": "Guhit Studio",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Draw and revise floor plans in Guhit Studio: rooms, walls, doors, windows, areas and sheets, shown live in the app.",
        "author": { "name": "Guhit Studio" },
        "icon": "icon.png",
        "server": {
            "type": "binary",
            "entry_point": command,
            "mcp_config": {
                "command": command,
                "args": ["--mcp-stdio"],
                "env": {}
            }
        },
        "tools_generated": true,
        "compatibility": { "platforms": ["darwin", "win32"] }
    })
}

/// The `.mcpb` file: a zip with `manifest.json` and `icon.png`.
pub fn claude_desktop_bundle(exe: &Path) -> Result<Vec<u8>, IpcError> {
    let fail = |e: &dyn std::fmt::Display| IpcError::new("io", format!("cannot write the extension: {e}"));
    let manifest = serde_json::to_vec_pretty(&claude_desktop_manifest(exe)).map_err(|e| fail(&e))?;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in [("manifest.json", manifest.as_slice()), ("icon.png", ICON_PNG)] {
        writer.start_file(name, options).map_err(|e| fail(&e))?;
        writer.write_all(bytes).map_err(|e| fail(&e))?;
    }
    Ok(writer.finish().map_err(|e| fail(&e))?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn service() -> (AppService, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (AppService::new_sandboxed(dir.path().to_path_buf()), dir)
    }

    #[tokio::test]
    async fn the_switch_is_on_by_default_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("settings.json"), r#"{"profile_name":"Axl"}"#).unwrap();
        let app = AppService::new_sandboxed(dir.path().to_path_buf());
        let status: McpStatus = serde_json::from_value(app.handle("mcp_status", json!({})).await.unwrap()).unwrap();
        assert!(status.enabled);
        assert!(!status.listening);
        assert!(status.stdio_command.is_none());
        assert!(!status.claude_desktop_bundle);

        let off: McpStatus =
            serde_json::from_value(app.handle("mcp_set_enabled", json!({"enabled": false})).await.unwrap()).unwrap();
        assert!(!off.enabled);
        assert!(!app.mcp.enabled());
        let file: Value = serde_json::from_str(&std::fs::read_to_string(dir.path().join("settings.json")).unwrap()).unwrap();
        assert_eq!(file["mcp_enabled"], false);
        assert_eq!(file["profile_name"], "Axl", "other keys are kept");

        // A restart reads it back.
        let again = AppService::new_sandboxed(dir.path().to_path_buf());
        assert!(!again.mcp.enabled());
        again.handle("mcp_set_enabled", json!({"enabled": true})).await.unwrap();
        assert!(again.mcp.enabled());

        let e = app.handle("mcp_set_enabled", json!({})).await.unwrap_err();
        assert_eq!(e.code, "bad_args");
    }

    #[tokio::test]
    async fn the_status_reports_what_the_transports_set() {
        let (app, _dir) = service();
        app.mcp.set_port(1631);
        app.mcp.set_listening(true);
        app.mcp.touch_client();
        assert!(app.mcp.status().last_client.is_none(), "no client named itself yet");
        app.mcp.saw_client("  Cursor\u{7}  ", "1.2.3");
        app.mcp.saw_client("", "ignored");
        let status = app.mcp.status();
        assert!(status.listening);
        assert_eq!(status.port, 1631);
        assert_eq!(status.url, "http://127.0.0.1:1631/mcp");
        let client = status.last_client.expect("last client");
        assert_eq!(client.name, "Cursor");
        assert_eq!(client.version, "1.2.3");
        assert!(files::parse_rfc3339_secs(&client.at).is_some(), "{}", client.at);
        // Clones share the state.
        assert!(app.clone().mcp.status().last_client.is_some());
    }

    #[tokio::test]
    async fn the_bundle_needs_the_desktop_app() {
        let (app, _dir) = service();
        let e = app.handle("mcp_claude_desktop_bundle", json!({})).await.unwrap_err();
        assert_eq!(e.code, "unavailable");
    }

    #[tokio::test]
    async fn the_bundle_runs_the_app_with_mcp_stdio() {
        let (app, dir) = service();
        let exe = if cfg!(windows) {
            PathBuf::from(r"C:\Program Files\Guhit Studio\guhit-studio.exe")
        } else {
            PathBuf::from("/Applications/Guhit Studio.app/Contents/MacOS/guhit-studio")
        };
        app.mcp.set_stdio_command(exe.clone());
        assert!(app.mcp.status().claude_desktop_bundle);

        let out = app.handle("mcp_claude_desktop_bundle", json!({})).await.unwrap();
        let path = PathBuf::from(out["path"].as_str().unwrap());
        assert!(path.is_absolute());
        assert_eq!(path, dir.path().join(CLAUDE_DESKTOP_DIR).join(CLAUDE_DESKTOP_FILE));

        let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut text = String::new();
        archive.by_name("manifest.json").unwrap().read_to_string(&mut text).unwrap();
        let manifest: Value = serde_json::from_str(&text).unwrap();
        let expected = if cfg!(windows) {
            r"C:\Program Files\Guhit Studio\guhit-studio"
        } else {
            "/Applications/Guhit Studio.app/Contents/MacOS/guhit-studio"
        };
        assert_eq!(manifest["manifest_version"], "0.3");
        assert_eq!(manifest["name"], "guhit-studio");
        assert_eq!(manifest["display_name"], "Guhit Studio");
        assert_eq!(manifest["version"], env!("CARGO_PKG_VERSION"));
        assert!(manifest["description"].as_str().is_some_and(|d| !d.is_empty()));
        assert!(manifest["author"]["name"].as_str().is_some_and(|n| !n.is_empty()));
        assert_eq!(manifest["server"]["type"], "binary");
        assert_eq!(manifest["server"]["entry_point"], expected);
        assert_eq!(manifest["server"]["mcp_config"]["command"], expected);
        assert_eq!(manifest["server"]["mcp_config"]["args"], json!(["--mcp-stdio"]));

        let mut icon = Vec::new();
        archive.by_name("icon.png").unwrap().read_to_end(&mut icon).unwrap();
        assert!(icon.starts_with(b"\x89PNG"), "the icon is a PNG");
        assert_eq!(archive.len(), 2);
    }
}
