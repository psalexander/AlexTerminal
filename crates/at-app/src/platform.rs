//! Desktop integration: file manager, editor, URLs, bell sound, Dock badge.

use std::path::Path;
use std::process::{Command, Stdio};

fn spawn(cmd: &mut Command) -> std::io::Result<()> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

/// Opens a folder in Finder / the default file manager (§4.9).
pub fn open_folder(dir: &Path, custom: &str) -> std::io::Result<()> {
    if !custom.trim().is_empty() {
        return run_template(custom, dir);
    }
    if cfg!(target_os = "macos") {
        spawn(Command::new("open").arg(dir))
    } else {
        spawn(Command::new("xdg-open").arg(dir))
            .or_else(|_| spawn(Command::new("gio").arg("open").arg(dir)))
    }
}

/// Shows the folder selected in its parent (`open -R`).
pub fn reveal(dir: &Path) -> std::io::Result<()> {
    if cfg!(target_os = "macos") {
        spawn(Command::new("open").arg("-R").arg(dir))
    } else {
        let parent = dir.parent().unwrap_or(dir);
        spawn(Command::new("xdg-open").arg(parent))
            .or_else(|_| spawn(Command::new("gio").arg("open").arg(parent)))
    }
}

/// Runs a command template where `{path}` is replaced by the path (shell-quoted).
pub fn run_template(template: &str, path: &Path) -> std::io::Result<()> {
    let quoted = shell_quote(&path.display().to_string());
    let line = if template.contains("{path}") {
        template.replace("{path}", &quoted)
    } else {
        format!("{template} {quoted}")
    };
    // Use a login shell so PATH from the user's profile is available (e.g. `code`).
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    spawn(Command::new(shell).arg("-lc").arg(line))
}

pub fn open_url(url: &str) -> std::io::Result<()> {
    let target = if let Some(rest) = url.strip_prefix("~/") {
        at_core::paths::home_dir().join(rest).display().to_string()
    } else {
        url.to_string()
    };
    if cfg!(target_os = "macos") {
        spawn(Command::new("open").arg(target))
    } else {
        spawn(Command::new("xdg-open").arg(target))
    }
}

pub fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+,:@%=".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

pub fn bell_sound() {
    if cfg!(target_os = "macos") {
        let _ = spawn(Command::new("afplay").arg("/System/Library/Sounds/Tink.aiff"));
    } else {
        let _ = spawn(Command::new("canberra-gtk-play").args(["-i", "bell"]));
    }
}

/// Number of tabs needing attention on the Dock icon (macOS).
pub fn set_badge(count: usize) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSApplication;
        use objc2_foundation::{MainThreadMarker, NSString};
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        let tile = app.dockTile();
        let label = (count > 0).then(|| NSString::from_str(&count.to_string()));
        tile.setBadgeLabel(label.as_deref());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = count;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(shell_quote("/usr/bin"), "/usr/bin");
        assert_eq!(shell_quote("/a b/it's"), "'/a b/it'\\''s'");
    }
}
