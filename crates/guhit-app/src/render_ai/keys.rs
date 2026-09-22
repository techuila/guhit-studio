//! Where the Google AI API key lives. Same shape as the copilot's key store
//! (DECISIONS D13): a private file in the app data folder, because an
//! unsigned build gets a new code identity on every rebuild and macOS then
//! re-asks for keychain permission every time.
//!
//! The key is never sent to the frontend, never logged and never written to a
//! project or settings file. `RenderAiSettings` carries `has_api_key` only.

use std::sync::Mutex;

use guhit_model::IpcError;

/// File name under `data_dir`. Separate from the copilot's `api-key`, so the
/// two providers are configured and removed independently.
pub const KEY_FILE: &str = "render-api-key";

/// Read-only fallbacks, in order. Both names are in wide use for Google AI
/// Studio keys, so either one works without any configuration.
const ENV_VARS: [&str; 2] = ["GEMINI_API_KEY", "GOOGLE_API_KEY"];

/// Google AI Studio keys all start with this.
const KEY_PREFIX: &str = "AIza";

fn clean(key: &str) -> Result<&str, IpcError> {
    let key = key.trim();
    if !key.starts_with(KEY_PREFIX) {
        return Err(IpcError::new(
            "invalid",
            "That is not a Google AI API key. Create one at aistudio.google.com; a Google AI Studio key starts with AIza.",
        ));
    }
    Ok(key)
}

fn env_key() -> Option<String> {
    ENV_VARS
        .iter()
        .find_map(|name| std::env::var(name).ok())
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
}

pub trait RenderKeyStore: Send + Sync {
    /// The stored key, or None when there is none.
    fn get(&self) -> Option<String>;
    fn set(&self, key: &str) -> Result<(), IpcError>;
    /// Removing a key that does not exist is not an error.
    fn remove(&self) -> Result<(), IpcError>;
}

/// Key in `<data_dir>/render-api-key`, mode 0600 on Unix, with a read-only
/// fallback to `GEMINI_API_KEY` or `GOOGLE_API_KEY`.
pub struct FileStore {
    path: std::path::PathBuf,
}

impl FileStore {
    pub fn new(data_dir: &std::path::Path) -> Self {
        Self { path: data_dir.join(KEY_FILE) }
    }
}

impl RenderKeyStore for FileStore {
    fn get(&self) -> Option<String> {
        // Trimmed on the way in and out: a pasted trailing newline makes the
        // API answer 403 with nothing to explain it.
        let stored = std::fs::read_to_string(&self.path)
            .ok()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty());
        stored.or_else(env_key)
    }

    fn set(&self, key: &str) -> Result<(), IpcError> {
        let key = clean(key)?;
        let io = |e: std::io::Error| IpcError::new("io", format!("Could not save the key: {e}"));
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        std::fs::write(&self.path, key).map_err(io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600)).map_err(io)?;
        }
        Ok(())
    }

    fn remove(&self) -> Result<(), IpcError> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(IpcError::new("io", format!("Could not remove the key: {e}"))),
        }
    }
}

/// In-memory store for tests. Never touches a file or the environment.
#[derive(Default)]
pub struct MemoryKeyStore(pub Mutex<Option<String>>);

impl MemoryKeyStore {
    pub fn with_key(key: &str) -> Self {
        Self(Mutex::new(Some(key.to_string())))
    }
}

impl RenderKeyStore for MemoryKeyStore {
    fn get(&self) -> Option<String> {
        self.0.lock().ok().and_then(|k| k.clone())
    }

    fn set(&self, key: &str) -> Result<(), IpcError> {
        let key = clean(key)?;
        if let Ok(mut slot) = self.0.lock() {
            *slot = Some(key.to_string());
        }
        Ok(())
    }

    fn remove(&self) -> Result<(), IpcError> {
        if let Ok(mut slot) = self.0.lock() {
            *slot = None;
        }
        Ok(())
    }
}
