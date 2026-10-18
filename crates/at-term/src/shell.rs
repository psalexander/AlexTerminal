//! Shell selection and shell-integration injection (§4.6).

use at_core::config::IntegrationMode;
use portable_pty::CommandBuilder;
use std::path::{Path, PathBuf};

pub const ZSHENV: &str = include_str!("../../../assets/shell/zshenv");
pub const AT_ZSH: &str = include_str!("../../../assets/shell/at.zsh");
pub const BASHRC: &str = include_str!("../../../assets/shell/bashrc");
pub const AT_BASH: &str = include_str!("../../../assets/shell/at.bash");
pub const AT_FISH: &str = include_str!("../../../assets/shell/at.fish");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellKind {
    Zsh,
    Bash,
    Fish,
    Other,
}

pub fn kind_of(shell: &str) -> ShellKind {
    let name = Path::new(shell)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    match name.trim_start_matches('-') {
        "zsh" => ShellKind::Zsh,
        "bash" => ShellKind::Bash,
        "fish" => ShellKind::Fish,
        _ => ShellKind::Other,
    }
}

/// `$SHELL`, then the passwd entry, then `/bin/zsh` (macOS) or `/bin/bash`.
pub fn default_shell() -> String {
    if let Ok(s) = std::env::var("SHELL")
        && !s.is_empty()
        && Path::new(&s).exists()
    {
        return s;
    }
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if !pw.is_null() && !(*pw).pw_shell.is_null() {
            let s = std::ffi::CStr::from_ptr((*pw).pw_shell)
                .to_string_lossy()
                .into_owned();
            if !s.is_empty() && Path::new(&s).exists() {
                return s;
            }
        }
    }
    if cfg!(target_os = "macos") {
        "/bin/zsh".into()
    } else {
        "/bin/bash".into()
    }
}

fn write_if_changed(path: &Path, content: &str) -> std::io::Result<()> {
    if std::fs::read_to_string(path).ok().as_deref() == Some(content) {
        return Ok(());
    }
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(path, content)
}

/// Writes the integration scripts to `dir` (idempotent).
pub fn install(dir: &Path) -> std::io::Result<()> {
    write_if_changed(&dir.join("zsh/.zshenv"), ZSHENV)?;
    write_if_changed(&dir.join("at.zsh"), AT_ZSH)?;
    write_if_changed(&dir.join("bashrc"), BASHRC)?;
    write_if_changed(&dir.join("at.bash"), AT_BASH)?;
    write_if_changed(&dir.join("fish/fish/vendor_conf.d/at.fish"), AT_FISH)?;
    Ok(())
}

pub struct ShellLaunch {
    pub shell: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    pub integration: IntegrationMode,
    pub integration_dir: PathBuf,
    /// Per-pane shell history file (zsh/bash) or history name (fish).
    pub histfile: Option<PathBuf>,
    pub history_name: Option<String>,
    pub wrap_ssh: bool,
    pub term: String,
    pub pane_id: String,
}

/// Builds the command: a login shell with integration injected when possible.
pub fn command(l: &ShellLaunch) -> CommandBuilder {
    let kind = kind_of(&l.shell);
    let inject = l.integration == IntegrationMode::Auto;
    let mut cmd = CommandBuilder::new(&l.shell);
    match kind {
        ShellKind::Bash if inject => {
            cmd.arg("--rcfile");
            cmd.arg(l.integration_dir.join("bashrc"));
            cmd.arg("-i");
        }
        ShellKind::Zsh | ShellKind::Bash | ShellKind::Fish => {
            if l.args.is_empty() {
                cmd.arg("-l");
            }
        }
        ShellKind::Other => {}
    }
    for a in &l.args {
        cmd.arg(a);
    }
    cmd.cwd(&l.cwd);

    // Clean env inherited from the daemon's own launch context.
    for k in [
        "ALEXTERM_ORIG_ZDOTDIR",
        "ALEXTERM_INTEGRATION_DIR",
        "ALEXTERM_HISTFILE",
        "ALEXTERM_HISTORY_NAME",
        "ALEXTERM_WRAP_SSH",
        "_ALEXTERM_ZSH_LOADED",
        "_ALEXTERM_BASH_LOADED",
        "_ALEXTERM_FISH_LOADED",
    ] {
        cmd.env_remove(k);
    }
    cmd.env("TERM", &l.term);
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERM_PROGRAM", "AlexTerminal");
    cmd.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
    cmd.env("ALEXTERMINAL_PANE_ID", &l.pane_id);
    if std::env::var_os("LANG").is_none() && std::env::var_os("LC_ALL").is_none() {
        cmd.env("LANG", "en_US.UTF-8");
    }
    if inject || l.integration == IntegrationMode::Manual {
        cmd.env("ALEXTERM_INTEGRATION_DIR", &l.integration_dir);
        if l.wrap_ssh {
            cmd.env("ALEXTERM_WRAP_SSH", "1");
        }
        if let Some(h) = &l.histfile {
            cmd.env("ALEXTERM_HISTFILE", h);
        }
        if let Some(n) = &l.history_name {
            cmd.env("ALEXTERM_HISTORY_NAME", n);
        }
    }
    if inject {
        match kind {
            ShellKind::Zsh => {
                if let Ok(orig) = std::env::var("ZDOTDIR") {
                    cmd.env("ALEXTERM_ORIG_ZDOTDIR", orig);
                }
                cmd.env("ZDOTDIR", l.integration_dir.join("zsh"));
            }
            ShellKind::Fish => {
                let base = std::env::var("XDG_DATA_DIRS")
                    .unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
                cmd.env(
                    "XDG_DATA_DIRS",
                    format!("{}:{base}", l.integration_dir.join("fish").display()),
                );
            }
            _ => {}
        }
    }
    for (k, v) in &l.env {
        cmd.env(k, v);
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(kind_of("/bin/zsh"), ShellKind::Zsh);
        assert_eq!(kind_of("-bash"), ShellKind::Bash);
        assert_eq!(kind_of("/opt/homebrew/bin/fish"), ShellKind::Fish);
        assert_eq!(kind_of("/bin/sh"), ShellKind::Other);
    }

    #[test]
    fn install_writes_files() {
        let d = tempfile::tempdir().unwrap();
        install(d.path()).unwrap();
        assert!(d.path().join("zsh/.zshenv").exists());
        assert!(d.path().join("fish/fish/vendor_conf.d/at.fish").exists());
        install(d.path()).unwrap();
    }
}
