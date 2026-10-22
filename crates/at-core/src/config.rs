//! `config.toml` (§5). Every field has a default; unknown keys are ignored with a warning.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub appearance: Appearance,
    pub terminal: Terminal,
    pub session: Session,
    pub shell_integration: ShellIntegration,
    pub integration: Integration,
    pub profiles: Vec<Profile>,
    /// action name → chord or list of chords; `"none"` unbinds.
    pub keys: BTreeMap<String, KeyBinding>,
    pub advanced: Advanced,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeyBinding {
    One(String),
    Many(Vec<String>),
}

impl KeyBinding {
    pub fn chords(&self) -> Vec<String> {
        match self {
            KeyBinding::One(s) => vec![s.clone()],
            KeyBinding::Many(v) => v.clone(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: General::default(),
            appearance: Appearance::default(),
            terminal: Terminal::default(),
            session: Session::default(),
            shell_integration: ShellIntegration::default(),
            integration: Integration::default(),
            profiles: vec![Profile::default()],
            keys: BTreeMap::new(),
            advanced: Advanced::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    /// `system`, `en` or `ru`.
    pub ui_language: String,
}

impl Default for General {
    fn default() -> Self {
        Self {
            ui_language: "system".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    #[default]
    Block,
    Underline,
    Bar,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: String,
    pub font_family: String,
    pub font_fallback: Vec<String>,
    pub font_size: f32,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub ligatures: bool,
    pub bold_is_bright: bool,
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,
    pub pane_padding: f32,
    pub pane_title_bar: bool,
    pub sidebar_width: f32,
    /// Show a group header even when the group holds a single tab.
    pub expand_single_tab_groups: bool,
    pub indent_per_level: f32,
    pub animate_indicators: bool,
    pub window_opacity: f32,
    pub window_blur: bool,
    pub show_menu_bar: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "auto".into(),
            font_family: "JetBrains Mono".into(),
            font_fallback: vec![
                "SF Mono".into(),
                "Menlo".into(),
                "DejaVu Sans Mono".into(),
                "Symbols Nerd Font Mono".into(),
                "Apple Color Emoji".into(),
                "Noto Color Emoji".into(),
            ],
            font_size: if cfg!(target_os = "macos") {
                13.5
            } else {
                12.0
            },
            line_height: 1.3,
            letter_spacing: 0.0,
            ligatures: true,
            bold_is_bright: false,
            cursor_style: CursorStyle::Block,
            cursor_blink: false,
            pane_padding: 10.0,
            pane_title_bar: true,
            sidebar_width: 220.0,
            expand_single_tab_groups: false,
            indent_per_level: 14.0,
            animate_indicators: true,
            window_opacity: 1.0,
            window_blur: false,
            show_menu_bar: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Bell {
    #[default]
    Visual,
    Sound,
    Both,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Osc52 {
    #[default]
    Write,
    ReadWrite,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OptionAsAlt {
    None,
    #[default]
    Left,
    Right,
    Both,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Terminal {
    pub shell: String,
    pub shell_args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub term: String,
    pub scrollback_lines: usize,
    pub bell: Bell,
    pub attention_threshold_sec: u64,
    pub copy_on_select: bool,
    pub paste_warn_multiline: bool,
    pub word_separators: String,
    pub url_detection: bool,
    pub osc52: Osc52,
    pub option_as_alt: OptionAsAlt,
    pub remote_programs: Vec<String>,
    /// `inherit`, `home` or a path.
    pub new_tab_cwd: String,
    pub confirm_close_when_busy: bool,
    pub close_tab_with_children: CloseChildren,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CloseChildren {
    #[default]
    Promote,
    CloseAll,
}

impl Default for Terminal {
    fn default() -> Self {
        Self {
            shell: String::new(),
            shell_args: Vec::new(),
            env: BTreeMap::new(),
            term: "xterm-256color".into(),
            scrollback_lines: 10_000,
            bell: Bell::Visual,
            attention_threshold_sec: 5,
            copy_on_select: false,
            paste_warn_multiline: true,
            word_separators: " ,│`|:\"'()[]{}<>\t".into(),
            url_detection: true,
            osc52: Osc52::Write,
            option_as_alt: OptionAsAlt::Left,
            remote_programs: [
                "ssh",
                "mosh-client",
                "telnet",
                "et",
                "nc",
                "kubectl",
                "docker",
            ]
            .map(String::from)
            .to_vec(),
            new_tab_cwd: "inherit".into(),
            confirm_close_when_busy: true,
            close_tab_with_children: CloseChildren::Promote,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Reconnect {
    #[default]
    Ask,
    Auto,
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CloseWindow {
    #[default]
    Detach,
    CloseTabs,
    Ask,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub restore_session: bool,
    pub restore_scrollback: bool,
    pub restore_scrollback_lines: usize,
    pub reconnect_remote: Reconnect,
    pub autosave_interval_sec: u64,
    pub confirm_quit_when_busy: bool,
    pub close_window: CloseWindow,
    pub per_pane_shell_history: bool,
    pub history_max_entries: usize,
    pub history_retention_days: u64,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            restore_session: true,
            restore_scrollback: true,
            restore_scrollback_lines: 2000,
            reconnect_remote: Reconnect::Ask,
            autosave_interval_sec: 30,
            confirm_quit_when_busy: true,
            close_window: CloseWindow::Detach,
            per_pane_shell_history: true,
            history_max_entries: 10_000,
            history_retention_days: 30,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum IntegrationMode {
    #[default]
    Auto,
    Manual,
    Off,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellIntegration {
    pub mode: IntegrationMode,
    pub wrap_ssh: bool,
}

impl Default for ShellIntegration {
    fn default() -> Self {
        Self {
            mode: IntegrationMode::Auto,
            wrap_ssh: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Integration {
    pub editor_command: String,
    pub file_manager_command: String,
}

impl Default for Integration {
    fn default() -> Self {
        Self {
            editor_command: "code {path}".into(),
            file_manager_command: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    pub shell: String,
    pub args: Vec<String>,
    /// `inherit`, `home` or a path.
    pub cwd: String,
    pub command: String,
    pub env: BTreeMap<String, String>,
    pub color: String,
    pub icon: String,
    pub title: String,
    pub remote: bool,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            name: "default".into(),
            shell: String::new(),
            args: Vec::new(),
            cwd: "inherit".into(),
            command: String::new(),
            env: BTreeMap::new(),
            color: String::new(),
            icon: String::new(),
            title: String::new(),
            remote: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Advanced {
    pub renderer: String,
    pub vsync: bool,
    pub max_fps: u32,
    pub log_level: String,
}

impl Default for Advanced {
    fn default() -> Self {
        Self {
            renderer: "auto".into(),
            vsync: true,
            max_fps: 120,
            log_level: "info".into(),
        }
    }
}

impl Config {
    /// Loads the config; a missing file yields defaults, a broken one yields defaults plus an error.
    pub fn load(path: &Path) -> (Config, Option<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => match Self::parse(&text) {
                Ok(c) => (c, None),
                Err(e) => (Config::default(), Some(format!("{}: {e}", path.display()))),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Config::default(), None),
            Err(e) => (Config::default(), Some(format!("{}: {e}", path.display()))),
        }
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let raw: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
        warn_unknown(&raw);
        let mut c: Config = raw.try_into().map_err(|e: toml::de::Error| e.to_string())?;
        c.fixup();
        Ok(c)
    }

    pub fn load_default_path() -> (Config, Option<String>) {
        Self::load(&crate::paths::config_file())
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        crate::paths::write_atomic(path, text.as_bytes())
    }

    fn fixup(&mut self) {
        if !self.profiles.iter().any(|p| p.name == "default") {
            self.profiles.insert(0, Profile::default());
        }
        let a = &mut self.appearance;
        a.font_size = a.font_size.clamp(6.0, 72.0);
        a.line_height = a.line_height.clamp(0.8, 3.0);
        a.sidebar_width = a.sidebar_width.clamp(160.0, 400.0);
        a.pane_padding = a.pane_padding.clamp(0.0, 64.0);
        self.terminal.scrollback_lines = self.terminal.scrollback_lines.min(100_000);
        self.advanced.max_fps = self.advanced.max_fps.clamp(10, 240);
    }

    pub fn profile(&self, name: &str) -> &Profile {
        self.profiles
            .iter()
            .find(|p| p.name == name)
            .or_else(|| self.profiles.iter().find(|p| p.name == "default"))
            .or(self.profiles.first())
            .expect("default profile always exists")
    }

    /// Resolved UI language: `en` or `ru`.
    pub fn ui_language(&self, system_locale: Option<&str>) -> &'static str {
        match self.general.ui_language.as_str() {
            "ru" => "ru",
            "en" => "en",
            _ => match system_locale {
                Some(l) if l.to_lowercase().starts_with("ru") => "ru",
                _ => "en",
            },
        }
    }
}

const KNOWN: &[(&str, &[&str])] = &[
    ("general", &["ui_language"]),
    (
        "appearance",
        &[
            "theme",
            "font_family",
            "font_fallback",
            "font_size",
            "line_height",
            "letter_spacing",
            "ligatures",
            "bold_is_bright",
            "cursor_style",
            "cursor_blink",
            "pane_padding",
            "pane_title_bar",
            "sidebar_width",
            "expand_single_tab_groups",
            "indent_per_level",
            "animate_indicators",
            "window_opacity",
            "window_blur",
            "show_menu_bar",
        ],
    ),
    (
        "terminal",
        &[
            "shell",
            "shell_args",
            "env",
            "term",
            "scrollback_lines",
            "bell",
            "attention_threshold_sec",
            "copy_on_select",
            "paste_warn_multiline",
            "word_separators",
            "url_detection",
            "osc52",
            "option_as_alt",
            "remote_programs",
            "new_tab_cwd",
            "confirm_close_when_busy",
            "close_tab_with_children",
        ],
    ),
    (
        "session",
        &[
            "restore_session",
            "restore_scrollback",
            "restore_scrollback_lines",
            "reconnect_remote",
            "autosave_interval_sec",
            "confirm_quit_when_busy",
            "close_window",
            "per_pane_shell_history",
            "history_max_entries",
            "history_retention_days",
        ],
    ),
    ("shell_integration", &["mode", "wrap_ssh"]),
    ("integration", &["editor_command", "file_manager_command"]),
    ("advanced", &["renderer", "vsync", "max_fps", "log_level"]),
];

/// Returns the unknown keys (also logged).
pub fn unknown_keys(raw: &toml::Value) -> Vec<String> {
    let mut out = Vec::new();
    let Some(table) = raw.as_table() else {
        return out;
    };
    for (k, v) in table {
        if k == "keys" || k == "profiles" {
            continue;
        }
        match KNOWN.iter().find(|(s, _)| s == k) {
            None => out.push(k.clone()),
            Some((_, fields)) => {
                if let Some(t) = v.as_table() {
                    for f in t.keys() {
                        if !fields.contains(&f.as_str()) {
                            out.push(format!("{k}.{f}"));
                        }
                    }
                }
            }
        }
    }
    out
}

fn warn_unknown(raw: &toml::Value) {
    for k in unknown_keys(raw) {
        tracing::warn!("config: unknown key `{k}` ignored");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_roundtrip() {
        let c = Config::default();
        let text = toml::to_string_pretty(&c).unwrap();
        let back = Config::parse(&text).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn partial_and_unknown() {
        let text = r#"
            [appearance]
            font_size = 15
            bogus = 1
            [session]
            reconnect_remote = "auto"
            [[profiles]]
            name = "prod"
            command = "ssh prod-01"
            remote = true
            [keys]
            new_tab = "cmd+t"
            next_tab = ["ctrl+tab", "cmd+shift+]"]
        "#;
        let c = Config::parse(text).unwrap();
        assert_eq!(c.appearance.font_size, 15.0);
        assert_eq!(c.session.reconnect_remote, Reconnect::Auto);
        assert_eq!(c.profiles.len(), 2);
        assert_eq!(c.profile("prod").command, "ssh prod-01");
        assert_eq!(c.profile("missing").name, "default");
        assert_eq!(c.keys["next_tab"].chords().len(), 2);
        let raw: toml::Value = toml::from_str(text).unwrap();
        assert_eq!(unknown_keys(&raw), vec!["appearance.bogus".to_string()]);
    }

    #[test]
    fn language() {
        let mut c = Config::default();
        assert_eq!(c.ui_language(Some("ru-RU")), "ru");
        assert_eq!(c.ui_language(Some("de-DE")), "en");
        c.general.ui_language = "ru".into();
        assert_eq!(c.ui_language(Some("en-US")), "ru");
    }
}
