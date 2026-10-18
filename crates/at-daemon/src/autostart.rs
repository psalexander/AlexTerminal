//! Login autostart for the daemon: LaunchAgent (macOS) or systemd user unit (Linux).

use std::path::PathBuf;

pub const LABEL: &str = "pro.odinai.alexterminal.server";

pub fn agent_path() -> PathBuf {
    let home = at_core::paths::home_dir();
    if cfg!(target_os = "macos") {
        home.join(format!("Library/LaunchAgents/{LABEL}.plist"))
    } else {
        home.join(".config/systemd/user/alexterminal-server.service")
    }
}

pub fn agent_contents(exe: &std::path::Path) -> String {
    let exe = exe.display();
    if cfg!(target_os = "macos") {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array><string>{exe}</string><string>--foreground</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><false/>
  <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
"#
        )
    } else {
        format!(
            "[Unit]\nDescription=AlexTerminal session server\n\n[Service]\nExecStart={exe} --foreground\nRestart=no\nKillMode=process\n\n[Install]\nWantedBy=default.target\n"
        )
    }
}

/// Installs or removes the autostart entry. Registration with launchd/systemd is best effort.
pub fn set(enabled: bool, exe: &std::path::Path) -> std::io::Result<()> {
    let path = agent_path();
    if enabled {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(&path, agent_contents(exe))?;
        if cfg!(target_os = "linux") {
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .status();
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "enable", "alexterminal-server.service"])
                .status();
        }
    } else if path.exists() {
        if cfg!(target_os = "linux") {
            let _ = std::process::Command::new("systemctl")
                .args(["--user", "disable", "alexterminal-server.service"])
                .status();
        }
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn is_enabled() -> bool {
    agent_path().exists()
}
