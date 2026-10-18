//! File system locations (§9 of the spec).
//!
//! `ALEXTERMINAL_HOME` overrides every location; it is used by tests and for
//! running an isolated instance next to a real one.

use std::path::PathBuf;

fn override_root() -> Option<PathBuf> {
    std::env::var_os("ALEXTERMINAL_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// `~/.config/alexterminal`
pub fn config_dir() -> PathBuf {
    if let Some(r) = override_root() {
        return r.join("config");
    }
    if let Some(x) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(x).join("alexterminal");
    }
    home().join(".config").join("alexterminal")
}

pub fn config_file() -> PathBuf {
    if let Some(p) = std::env::var_os("ALEXTERMINAL_CONFIG").filter(|v| !v.is_empty()) {
        return PathBuf::from(p);
    }
    let primary = config_dir().join("config.toml");
    if cfg!(target_os = "macos") && !primary.exists() && override_root().is_none() {
        let alt = home().join("Library/Application Support/AlexTerminal/config.toml");
        if alt.exists() {
            return alt;
        }
    }
    primary
}

pub fn themes_dir() -> PathBuf {
    config_dir().join("themes")
}

/// Snapshot, history and scrollback.
pub fn data_dir() -> PathBuf {
    if let Some(r) = override_root() {
        return r.join("data");
    }
    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/AlexTerminal")
    } else if let Some(x) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        PathBuf::from(x).join("alexterminal")
    } else {
        home().join(".local/share/alexterminal")
    }
}

pub fn log_dir() -> PathBuf {
    if let Some(r) = override_root() {
        return r.join("logs");
    }
    if cfg!(target_os = "macos") {
        home().join("Library/Logs/AlexTerminal")
    } else if let Some(x) = std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()) {
        PathBuf::from(x).join("alexterminal")
    } else {
        home().join(".local/state/alexterminal")
    }
}

/// Directory with the daemon socket and lock file.
pub fn runtime_dir() -> PathBuf {
    if let Some(r) = override_root() {
        return r.join("run");
    }
    if cfg!(target_os = "macos") {
        data_dir()
    } else if let Some(x) = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        PathBuf::from(x).join("alexterminal")
    } else {
        data_dir().join("run")
    }
}

/// Unix socket paths are limited to ~104 bytes; long locations fall back to `/tmp`.
pub fn socket_path() -> PathBuf {
    let p = runtime_dir().join("instance.sock");
    if p.as_os_str().len() < 100 {
        return p;
    }
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    p.hash(&mut h);
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!(
        "/tmp/alexterminal-{uid}-{:x}",
        h.finish() & 0xffff_ffff
    ))
    .join("instance.sock")
}

pub fn lock_path() -> PathBuf {
    runtime_dir().join("instance.lock")
}

pub fn session_file() -> PathBuf {
    data_dir().join("session.json")
}

pub fn history_dir() -> PathBuf {
    data_dir().join("history")
}

pub fn scrollback_dir() -> PathBuf {
    data_dir().join("scrollback")
}

pub fn shell_integration_dir() -> PathBuf {
    data_dir().join("shell")
}

/// Create a directory with `0700` permissions.
pub fn ensure_private_dir(p: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(p)?;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700))
}

/// Atomic write: temp file + fsync + rename.
pub fn write_atomic(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Expands a leading `~`.
pub fn expand_tilde(p: &str) -> PathBuf {
    if p == "~" {
        home()
    } else if let Some(rest) = p.strip_prefix("~/") {
        home().join(rest)
    } else {
        PathBuf::from(p)
    }
}

pub fn home_dir() -> PathBuf {
    home()
}
