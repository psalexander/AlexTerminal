//! Live pane metadata shared between daemon and GUI.

use crate::PaneId;
use serde::{Deserialize, Serialize};

/// Indicator state, ordered by priority (§4.3). Lower discriminant wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PaneState {
    Attention,
    Remote,
    Running,
    Exited,
    Idle,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PaneInfo {
    pub id: PaneId,
    /// Title set by the program via OSC 0/2.
    pub osc_title: Option<String>,
    pub cwd: Option<String>,
    /// Remote host when an ssh-like program is in the foreground.
    pub remote_host: Option<String>,
    /// Full command line that opened the remote session (for reconnect).
    pub remote_command: Option<String>,
    pub running: bool,
    pub exited: Option<i32>,
    pub attention: bool,
    pub fg_process: Option<String>,
    pub fg_pid: Option<u32>,
    pub profile: String,
    /// Pending "reconnect to host?" prompt after restore.
    pub reconnect_prompt: Option<String>,
    pub last_command: Option<String>,
    pub last_exit: Option<i32>,
    /// Whether shell integration has reported in.
    pub integration: bool,
    pub font_scale: f32,
}

impl PaneInfo {
    pub fn new(id: PaneId, profile: &str) -> Self {
        Self {
            id,
            profile: profile.to_string(),
            font_scale: 1.0,
            ..Default::default()
        }
    }

    pub fn state(&self) -> PaneState {
        if self.attention {
            PaneState::Attention
        } else if self.exited.is_some() {
            PaneState::Exited
        } else if self.remote_host.is_some() {
            PaneState::Remote
        } else if self.running {
            PaneState::Running
        } else {
            PaneState::Idle
        }
    }

    /// Automatic title (§4.2): OSC title, then remote host, then last cwd segment.
    pub fn auto_title(&self) -> String {
        if let Some(t) = self.osc_title.as_ref().filter(|t| !t.trim().is_empty()) {
            return t.trim().to_string();
        }
        if let Some(h) = &self.remote_host {
            return h.clone();
        }
        if let Some(cwd) = &self.cwd {
            return short_dir(cwd);
        }
        self.fg_process.clone().unwrap_or_else(|| "shell".into())
    }
}

/// Last path segment, `~` for home.
pub fn short_dir(cwd: &str) -> String {
    let home = crate::paths::home_dir();
    let p = std::path::Path::new(cwd);
    if p == home {
        return "~".into();
    }
    if cwd == "/" {
        return "/".into();
    }
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| cwd.to_string())
}

/// `~/src/api` style path.
pub fn tilde_path(cwd: &str) -> String {
    let home = crate::paths::home_dir();
    let home = home.to_string_lossy();
    if cwd == home {
        "~".into()
    } else if let Some(rest) = cwd.strip_prefix(&format!("{home}/")) {
        format!("~/{rest}")
    } else {
        cwd.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority() {
        let mut p = PaneInfo::new(PaneId::new(), "default");
        assert_eq!(p.state(), PaneState::Idle);
        p.running = true;
        assert_eq!(p.state(), PaneState::Running);
        p.remote_host = Some("srv".into());
        assert_eq!(p.state(), PaneState::Remote);
        p.attention = true;
        assert_eq!(p.state(), PaneState::Attention);
        assert!(PaneState::Attention < PaneState::Remote);
        assert!(PaneState::Running < PaneState::Idle);
    }

    #[test]
    fn titles() {
        let mut p = PaneInfo::new(PaneId::new(), "default");
        p.cwd = Some("/usr/local/bin".into());
        assert_eq!(p.auto_title(), "bin");
        p.remote_host = Some("srv-01".into());
        assert_eq!(p.auto_title(), "srv-01");
        p.osc_title = Some("vim main.rs".into());
        assert_eq!(p.auto_title(), "vim main.rs");
    }
}
