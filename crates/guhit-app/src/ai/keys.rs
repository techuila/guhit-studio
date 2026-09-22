//! Where the API key lives. It is never sent to the frontend, never logged
//! and never written to a project or settings file.
//!
//! Two stores exist. `FileStore` (the default) keeps the key in a file in the
//! app data folder that only the current OS user can read. `KeychainStore`
//! uses the OS credential store; it is kept for signed builds, because an
//! unsigned build gets a new identity on every rebuild and macOS then asks
//! for keychain permission again and again (DECISIONS D13).

use std::sync::Mutex;

use guhit_model::IpcError;

pub const KEYCHAIN_SERVICE: &str = "ph.guhit.studio";
const KEYCHAIN_ACCOUNT: &str = "anthropic-api-key";
const ENV_VAR: &str = "ANTHROPIC_API_KEY";

fn clean(key: &str) -> Result<&str, IpcError> {
    let key = key.trim();
    if !key.starts_with("sk-ant-api") {
        return Err(IpcError::new(
            "invalid",
            "That is not a Claude API key. Create one at platform.claude.com (it starts with sk-ant-api). Claude subscription and Claude Code tokens do not work here.",
        ));
    }
    Ok(key)
}

fn env_key() -> Option<String> {
    std::env::var(ENV_VAR).ok().map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
}

/// Key in `<data_dir>/api-key`, mode 0600 on Unix. Falls back to the
/// `ANTHROPIC_API_KEY` environment variable, read-only.
pub struct FileStore {
    path: std::path::PathBuf,
}

impl FileStore {
    pub fn new(data_dir: &std::path::Path) -> Self {
        Self { path: data_dir.join("api-key") }
    }
}

impl KeyStore for FileStore {
    fn get(&self) -> Option<String> {
        let stored = std::fs::read_to_string(&self.path).ok().map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
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

pub trait KeyStore: Send + Sync {
    /// The stored key, or None when there is none or the store is unavailable.
    fn get(&self) -> Option<String>;
    fn set(&self, key: &str) -> Result<(), IpcError>;
    /// Removing a key that does not exist is not an error.
    fn remove(&self) -> Result<(), IpcError>;
}

/// OS keychain, with a read-only fallback to the `ANTHROPIC_API_KEY`
/// environment variable for places without a keychain (the headless dev
/// bridge, CI).
#[derive(Default)]
pub struct KeychainStore;

fn entry() -> Result<keyring::Entry, IpcError> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
        .map_err(|e| IpcError::new("io", format!("The system keychain is not available: {e}")))
}

impl KeyStore for KeychainStore {
    fn get(&self) -> Option<String> {
        // Keys are trimmed on the way in and out: a pasted trailing newline
        // makes the API answer 401 with nothing to explain it.
        let stored = entry().ok().and_then(|e| e.get_password().ok()).map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
        stored.or_else(env_key)
    }

    fn set(&self, key: &str) -> Result<(), IpcError> {
        let key = clean(key)?;
        entry()?
            .set_password(key)
            .map_err(|e| IpcError::new("io", format!("Could not save the key to the system keychain: {e}")))
    }

    fn remove(&self) -> Result<(), IpcError> {
        match entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(IpcError::new(
                "io",
                format!("Could not remove the key from the system keychain: {e}"),
            )),
        }
    }
}

/// In-memory store for tests. Never touches the OS keychain or the env.
#[derive(Default)]
pub struct MemoryKeyStore(pub Mutex<Option<String>>);

impl MemoryKeyStore {
    pub fn with_key(key: &str) -> Self {
        Self(Mutex::new(Some(key.to_string())))
    }
}

impl KeyStore for MemoryKeyStore {
    fn get(&self) -> Option<String> {
        self.0.lock().ok().and_then(|k| k.clone())
    }

    fn set(&self, key: &str) -> Result<(), IpcError> {
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
