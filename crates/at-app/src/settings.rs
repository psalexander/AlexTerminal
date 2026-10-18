//! Settings window (§5): edits a draft of `config.toml` and saves it on demand.

use crate::app::App;
use crate::style::{self, c};
use at_core::config::{
    Bell, CloseWindow, Config, CursorStyle, IntegrationMode, OptionAsAlt, Profile, Reconnect,
};
use at_core::keymap::{Action, Chord, Keymap};
use at_core::theme::Theme;
use iced::widget::{
    Column, button, checkbox, column, container, pick_list, row, scrollable, space, text,
    text_input,
};
use iced::{Alignment, Element, Length};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Appearance,
    Terminal,
    Session,
    Profiles,
    Keys,
    Advanced,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    FontSize,
    LineHeight,
    Padding,
    SidebarWidth,
    Scrollback,
    AttentionSec,
    RestoreLines,
    Autosave,
    HistoryMax,
    HistoryDays,
    MaxFps,
    IdleShutdown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileField {
    Name,
    Shell,
    Cwd,
    Command,
    Color,
    Title,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Tab(Tab),
    Pick(&'static str, String),
    Toggle(&'static str, bool),
    Text(&'static str, String),
    Number(Field, String),
    ProfileSelect(usize),
    ProfileAdd,
    ProfileDelete,
    ProfileText(ProfileField, String),
    ProfileRemote(bool),
    Key(String, String),
    ResetKeys,
    Save,
    Cancel,
}

pub enum Outcome {
    None,
    Cancel,
    Save(Box<Config>),
}

pub struct SettingsState {
    pub draft: Config,
    pub tab: Tab,
    pub profile: usize,
    pub numbers: BTreeMap<&'static str, String>,
    pub keys: BTreeMap<String, String>,
    pub error: Option<String>,
    pub fonts: Vec<String>,
    pub themes: Vec<String>,
}

fn field_key(f: Field) -> &'static str {
    match f {
        Field::FontSize => "font_size",
        Field::LineHeight => "line_height",
        Field::Padding => "pane_padding",
        Field::SidebarWidth => "sidebar_width",
        Field::Scrollback => "scrollback_lines",
        Field::AttentionSec => "attention_threshold_sec",
        Field::RestoreLines => "restore_scrollback_lines",
        Field::Autosave => "autosave_interval_sec",
        Field::HistoryMax => "history_max_entries",
        Field::HistoryDays => "history_retention_days",
        Field::MaxFps => "max_fps",
        Field::IdleShutdown => "idle_shutdown_min",
    }
}

impl SettingsState {
    pub fn new(c: &Config) -> Self {
        let mut numbers = BTreeMap::new();
        numbers.insert("font_size", c.appearance.font_size.to_string());
        numbers.insert("line_height", c.appearance.line_height.to_string());
        numbers.insert("pane_padding", c.appearance.pane_padding.to_string());
        numbers.insert("sidebar_width", c.appearance.sidebar_width.to_string());
        numbers.insert("scrollback_lines", c.terminal.scrollback_lines.to_string());
        numbers.insert(
            "attention_threshold_sec",
            c.terminal.attention_threshold_sec.to_string(),
        );
        numbers.insert(
            "restore_scrollback_lines",
            c.session.restore_scrollback_lines.to_string(),
        );
        numbers.insert(
            "autosave_interval_sec",
            c.session.autosave_interval_sec.to_string(),
        );
        numbers.insert(
            "history_max_entries",
            c.session.history_max_entries.to_string(),
        );
        numbers.insert(
            "history_retention_days",
            c.session.history_retention_days.to_string(),
        );
        numbers.insert("max_fps", c.advanced.max_fps.to_string());
        numbers.insert("idle_shutdown_min", c.daemon.idle_shutdown_min.to_string());
        let (km, _) = Keymap::build(&c.keys);
        let keys = Action::all()
            .into_iter()
            .map(|a| {
                let chords: Vec<String> = km.chords(a).iter().map(chord_string).collect();
                (a.name(), chords.join(", "))
            })
            .collect();
        let mut fonts = crate::metrics::installed_families();
        if !fonts.contains(&c.appearance.font_family) {
            fonts.insert(0, c.appearance.font_family.clone());
        }
        Self {
            draft: c.clone(),
            tab: Tab::Appearance,
            profile: 0,
            numbers,
            keys,
            error: None,
            fonts,
            themes: Theme::available(),
        }
    }

    pub fn update(&mut self, m: Msg) -> Outcome {
        let d = &mut self.draft;
        match m {
            Msg::Tab(t) => self.tab = t,
            Msg::Pick(k, v) => match k {
                "lang" => d.general.ui_language = v,
                "theme" => d.appearance.theme = v,
                "font" => d.appearance.font_family = v,
                "cursor" => {
                    d.appearance.cursor_style = match v.as_str() {
                        "underline" => CursorStyle::Underline,
                        "bar" => CursorStyle::Bar,
                        _ => CursorStyle::Block,
                    }
                }
                "bell" => {
                    d.terminal.bell = match v.as_str() {
                        "sound" => Bell::Sound,
                        "both" => Bell::Both,
                        "none" => Bell::None,
                        _ => Bell::Visual,
                    }
                }
                "option_as_alt" => {
                    d.terminal.option_as_alt = match v.as_str() {
                        "none" => OptionAsAlt::None,
                        "right" => OptionAsAlt::Right,
                        "both" => OptionAsAlt::Both,
                        _ => OptionAsAlt::Left,
                    }
                }
                "new_tab_cwd" => d.terminal.new_tab_cwd = v,
                "reconnect" => {
                    d.session.reconnect_remote = match v.as_str() {
                        "auto" => Reconnect::Auto,
                        "never" => Reconnect::Never,
                        _ => Reconnect::Ask,
                    }
                }
                "close_window" => {
                    d.session.close_window = match v.as_str() {
                        "close_tabs" => CloseWindow::CloseTabs,
                        "ask" => CloseWindow::Ask,
                        _ => CloseWindow::Detach,
                    }
                }
                "integration" => {
                    d.shell_integration.mode = match v.as_str() {
                        "manual" => IntegrationMode::Manual,
                        "off" => IntegrationMode::Off,
                        _ => IntegrationMode::Auto,
                    }
                }
                "log_level" => d.advanced.log_level = v,
                _ => {}
            },
            Msg::Toggle(k, v) => match k {
                "cursor_blink" => d.appearance.cursor_blink = v,
                "ligatures" => d.appearance.ligatures = v,
                "bold_is_bright" => d.appearance.bold_is_bright = v,
                "title_bar" => d.appearance.pane_title_bar = v,
                "expand_single" => d.appearance.expand_single_tab_groups = v,
                "animate" => d.appearance.animate_indicators = v,
                "copy_on_select" => d.terminal.copy_on_select = v,
                "paste_warn" => d.terminal.paste_warn_multiline = v,
                "url_detection" => d.terminal.url_detection = v,
                "confirm_close" => d.terminal.confirm_close_when_busy = v,
                "restore_session" => d.session.restore_session = v,
                "restore_scrollback" => d.session.restore_scrollback = v,
                "confirm_quit" => d.session.confirm_quit_when_busy = v,
                "per_pane_history" => d.session.per_pane_shell_history = v,
                "wrap_ssh" => d.shell_integration.wrap_ssh = v,
                "autostart" => d.daemon.autostart = v,
                _ => {}
            },
            Msg::Text(k, v) => match k {
                "shell" => d.terminal.shell = v,
                "editor" => d.integration.editor_command = v,
                "file_manager" => d.integration.file_manager_command = v,
                "custom_cwd" => d.terminal.new_tab_cwd = v,
                _ => {}
            },
            Msg::Number(f, v) => {
                self.numbers.insert(field_key(f), v);
            }
            Msg::ProfileSelect(i) => self.profile = i.min(d.profiles.len().saturating_sub(1)),
            Msg::ProfileAdd => {
                let n = d.profiles.len();
                d.profiles.push(Profile {
                    name: format!("profile-{n}"),
                    ..Profile::default()
                });
                self.profile = n;
            }
            Msg::ProfileDelete => {
                if let Some(p) = d.profiles.get(self.profile)
                    && p.name != "default"
                {
                    d.profiles.remove(self.profile);
                    self.profile = self.profile.saturating_sub(1);
                }
            }
            Msg::ProfileText(f, v) => {
                if let Some(p) = d.profiles.get_mut(self.profile) {
                    match f {
                        ProfileField::Name => {
                            if p.name != "default" {
                                p.name = v
                            }
                        }
                        ProfileField::Shell => p.shell = v,
                        ProfileField::Cwd => p.cwd = v,
                        ProfileField::Command => p.command = v,
                        ProfileField::Color => p.color = v,
                        ProfileField::Title => p.title = v,
                    }
                }
            }
            Msg::ProfileRemote(v) => {
                if let Some(p) = d.profiles.get_mut(self.profile) {
                    p.remote = v;
                }
            }
            Msg::Key(a, v) => {
                self.keys.insert(a, v);
            }
            Msg::ResetKeys => {
                d.keys.clear();
                let (km, _) = Keymap::build(&BTreeMap::new());
                for a in Action::all() {
                    let chords: Vec<String> = km.chords(a).iter().map(chord_string).collect();
                    self.keys.insert(a.name(), chords.join(", "));
                }
            }
            Msg::Cancel => return Outcome::Cancel,
            Msg::Save => match self.build() {
                Ok(c) => return Outcome::Save(Box::new(c)),
                Err(e) => self.error = Some(e),
            },
        }
        Outcome::None
    }

    /// Validates numbers and key chords and produces the config to save.
    fn build(&self) -> Result<Config, String> {
        let mut c = self.draft.clone();
        let num = |k: &str| -> Result<f64, String> {
            self.numbers
                .get(k)
                .map(|s| s.trim().replace(',', "."))
                .unwrap_or_default()
                .parse::<f64>()
                .map_err(|_| k.to_string())
        };
        c.appearance.font_size = num("font_size")? as f32;
        c.appearance.line_height = num("line_height")? as f32;
        c.appearance.pane_padding = num("pane_padding")? as f32;
        c.appearance.sidebar_width = num("sidebar_width")? as f32;
        c.terminal.scrollback_lines = num("scrollback_lines")? as usize;
        c.terminal.attention_threshold_sec = num("attention_threshold_sec")? as u64;
        c.session.restore_scrollback_lines = num("restore_scrollback_lines")? as usize;
        c.session.autosave_interval_sec = num("autosave_interval_sec")? as u64;
        c.session.history_max_entries = num("history_max_entries")? as usize;
        c.session.history_retention_days = num("history_retention_days")? as u64;
        c.advanced.max_fps = num("max_fps")? as u32;
        c.daemon.idle_shutdown_min = num("idle_shutdown_min")? as u64;
        // Only actions that differ from the defaults are written.
        let defaults = at_core::keymap::default_bindings();
        let mut keys = BTreeMap::new();
        for (action, value) in &self.keys {
            let chords = split_chords(value);
            for ch in &chords {
                Chord::parse(ch).map_err(|e| format!("{action}: {e}"))?;
            }
            let norm = |v: &[String]| {
                v.iter()
                    .map(|s| {
                        Chord::parse(s)
                            .map(|c| chord_string(&c))
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
            };
            let def = defaults.get(action).cloned().unwrap_or_default();
            if norm(&chords) != norm(&def) {
                let binding = if chords.is_empty() {
                    at_core::config::KeyBinding::One("none".into())
                } else if chords.len() == 1 {
                    at_core::config::KeyBinding::One(chords[0].clone())
                } else {
                    at_core::config::KeyBinding::Many(chords)
                };
                keys.insert(action.clone(), binding);
            }
        }
        c.keys = keys;
        let text = toml::to_string(&c).map_err(|e| e.to_string())?;
        Config::parse(&text)
    }

    pub fn view<'a>(&'a self, app: &'a App) -> Element<'a, Msg> {
        let t = &app.theme;
        let i = &app.i18n;
        let tabs = [
            (Tab::Appearance, "settings-tab-appearance"),
            (Tab::Terminal, "settings-tab-terminal"),
            (Tab::Session, "settings-tab-session"),
            (Tab::Profiles, "settings-tab-profiles"),
            (Tab::Keys, "settings-tab-keys"),
            (Tab::Advanced, "settings-tab-advanced"),
        ];
        let mut nav = column![].spacing(2).width(150.0);
        for (tab, key) in tabs {
            nav = nav.push(
                button(text(i.t(key)).size(13))
                    .on_press(Msg::Tab(tab))
                    .width(Length::Fill)
                    .padding([6, 10])
                    .style(style::row_button(t, self.tab == tab)),
            );
        }
        let body: Element<'a, Msg> = match self.tab {
            Tab::Appearance => self.appearance(app),
            Tab::Terminal => self.terminal(app),
            Tab::Session => self.session(app),
            Tab::Profiles => self.profiles(app),
            Tab::Keys => self.keys_view(app),
            Tab::Advanced => self.advanced(app),
        };
        let mut footer = row![].spacing(8).align_y(Alignment::Center);
        if let Some(e) = &self.error {
            footer = footer.push(
                text(i.ta("settings-invalid", &[("field", e.clone())]))
                    .size(12)
                    .color(c(t.ui.accent_error)),
            );
        }
        footer = footer
            .push(space().width(Length::Fill))
            .push(
                button(text(i.t("button-cancel")).size(13))
                    .on_press(Msg::Cancel)
                    .padding([6, 14])
                    .style(style::secondary_button(t)),
            )
            .push(
                button(text(i.t("button-save")).size(13))
                    .on_press(Msg::Save)
                    .padding([6, 14])
                    .style(style::primary_button(t)),
            );
        column![
            text(i.t("settings-title")).size(16).color(c(t.ui.fg)),
            row![
                nav,
                container(
                    scrollable(container(body).padding([0, 12]))
                        .height(460.0)
                        .style(style::scroll(t))
                )
                .width(Length::Fill)
            ]
            .spacing(12),
            footer,
        ]
        .spacing(14)
        .into()
    }

    fn label<'a>(&self, app: &'a App, key: &str) -> Element<'a, Msg> {
        text(app.i18n.t(key))
            .size(13)
            .color(c(app.theme.ui.fg_muted))
            .width(200.0)
            .into()
    }

    fn line<'a>(&self, app: &'a App, key: &str, control: Element<'a, Msg>) -> Element<'a, Msg> {
        row![self.label(app, key), control]
            .spacing(12)
            .align_y(Alignment::Center)
            .into()
    }

    fn pick<'a>(
        &self,
        app: &'a App,
        key: &str,
        id: &'static str,
        options: Vec<Choice>,
        value: &str,
    ) -> Element<'a, Msg> {
        let selected = options.iter().find(|o| o.value == value).cloned();
        let control = pick_list(options, selected, move |c: Choice| Msg::Pick(id, c.value))
            .text_size(13)
            .width(260.0);
        self.line(app, key, control.into())
    }

    fn toggle<'a>(
        &self,
        app: &'a App,
        key: &str,
        id: &'static str,
        value: bool,
    ) -> Element<'a, Msg> {
        checkbox(value)
            .label(app.i18n.t(key))
            .on_toggle(move |v| Msg::Toggle(id, v))
            .text_size(13)
            .into()
    }

    fn number<'a>(&'a self, app: &'a App, key: &str, f: Field) -> Element<'a, Msg> {
        let v = self.numbers.get(field_key(f)).cloned().unwrap_or_default();
        let control = text_input("", &v)
            .on_input(move |s| Msg::Number(f, s))
            .size(13)
            .padding([4, 8])
            .width(120.0)
            .style(style::input(&app.theme));
        self.line(app, key, control.into())
    }

    fn textf<'a>(
        &self,
        app: &'a App,
        key: &str,
        id: &'static str,
        value: &str,
        placeholder: &str,
    ) -> Element<'a, Msg> {
        let control = text_input(placeholder, value)
            .on_input(move |s| Msg::Text(id, s))
            .size(13)
            .padding([4, 8])
            .width(260.0)
            .style(style::input(&app.theme));
        self.line(app, key, control.into())
    }

    fn choices(&self, app: &App, pairs: &[(&str, &str)]) -> Vec<Choice> {
        pairs
            .iter()
            .map(|(v, k)| Choice {
                value: v.to_string(),
                label: app.i18n.t(k),
            })
            .collect()
    }

    fn appearance<'a>(&'a self, app: &'a App) -> Element<'a, Msg> {
        let d = &self.draft;
        let langs = self.choices(
            app,
            &[
                ("system", "settings-lang-system"),
                ("en", "settings-lang-en"),
                ("ru", "settings-lang-ru"),
            ],
        );
        let themes: Vec<Choice> = self
            .themes
            .iter()
            .map(|n| Choice {
                value: n.clone(),
                label: match n.as_str() {
                    "auto" => app.i18n.t("settings-theme-auto"),
                    "dusk" => "Dusk".into(),
                    "linen" => "Linen".into(),
                    other => other.to_string(),
                },
            })
            .collect();
        let fonts: Vec<Choice> = self
            .fonts
            .iter()
            .map(|f| Choice {
                value: f.clone(),
                label: f.clone(),
            })
            .collect();
        let cursor = self.choices(
            app,
            &[
                ("block", "settings-cursor-block"),
                ("underline", "settings-cursor-underline"),
                ("bar", "settings-cursor-bar"),
            ],
        );
        let cursor_v = match d.appearance.cursor_style {
            CursorStyle::Block => "block",
            CursorStyle::Underline => "underline",
            CursorStyle::Bar => "bar",
        };
        let note = text(app.i18n.ta(
            "settings-font-active",
            &[("font", app.fonts.family.to_string())],
        ))
        .size(11)
        .color(c(app.theme.ui.fg_dim));
        Column::new()
            .spacing(10)
            .push(self.pick(
                app,
                "settings-language",
                "lang",
                langs,
                &d.general.ui_language,
            ))
            .push(self.pick(app, "settings-theme", "theme", themes, &d.appearance.theme))
            .push(self.pick(
                app,
                "settings-font",
                "font",
                fonts,
                &d.appearance.font_family,
            ))
            .push(note)
            .push(self.number(app, "settings-font-size", Field::FontSize))
            .push(self.number(app, "settings-line-height", Field::LineHeight))
            .push(self.pick(app, "settings-cursor", "cursor", cursor, cursor_v))
            .push(self.number(app, "settings-padding", Field::Padding))
            .push(self.number(app, "settings-sidebar-width", Field::SidebarWidth))
            .push(self.toggle(
                app,
                "settings-cursor-blink",
                "cursor_blink",
                d.appearance.cursor_blink,
            ))
            .push(self.toggle(
                app,
                "settings-ligatures",
                "ligatures",
                d.appearance.ligatures,
            ))
            .push(self.toggle(
                app,
                "settings-bold-bright",
                "bold_is_bright",
                d.appearance.bold_is_bright,
            ))
            .push(self.toggle(
                app,
                "settings-title-bar",
                "title_bar",
                d.appearance.pane_title_bar,
            ))
            .push(self.toggle(
                app,
                "settings-expand-single",
                "expand_single",
                d.appearance.expand_single_tab_groups,
            ))
            .push(self.toggle(
                app,
                "settings-animate",
                "animate",
                d.appearance.animate_indicators,
            ))
            .into()
    }

    fn terminal<'a>(&'a self, app: &'a App) -> Element<'a, Msg> {
        let d = &self.draft;
        let bell = self.choices(
            app,
            &[
                ("visual", "settings-bell-visual"),
                ("sound", "settings-bell-sound"),
                ("both", "settings-bell-both"),
                ("none", "settings-bell-none"),
            ],
        );
        let bell_v = match d.terminal.bell {
            Bell::Visual => "visual",
            Bell::Sound => "sound",
            Bell::Both => "both",
            Bell::None => "none",
        };
        let alt = self.choices(
            app,
            &[
                ("none", "settings-alt-none"),
                ("left", "settings-alt-left"),
                ("right", "settings-alt-right"),
                ("both", "settings-alt-both"),
            ],
        );
        let alt_v = match d.terminal.option_as_alt {
            OptionAsAlt::None => "none",
            OptionAsAlt::Left => "left",
            OptionAsAlt::Right => "right",
            OptionAsAlt::Both => "both",
        };
        let cwd = self.choices(
            app,
            &[
                ("inherit", "settings-cwd-inherit"),
                ("home", "settings-cwd-home"),
            ],
        );
        let custom = !matches!(d.terminal.new_tab_cwd.as_str(), "inherit" | "home");
        let mut col = Column::new()
            .spacing(10)
            .push(self.textf(
                app,
                "settings-shell",
                "shell",
                &d.terminal.shell,
                &at_term_default_shell(),
            ))
            .push(self.number(app, "settings-scrollback", Field::Scrollback))
            .push(self.pick(app, "settings-bell", "bell", bell, bell_v))
            .push(self.number(app, "settings-attention", Field::AttentionSec))
            .push(self.pick(
                app,
                "settings-new-tab-cwd",
                "new_tab_cwd",
                cwd,
                if custom { "" } else { &d.terminal.new_tab_cwd },
            ));
        if custom || d.terminal.new_tab_cwd.is_empty() {
            col = col.push(self.textf(
                app,
                "settings-custom-cwd",
                "custom_cwd",
                &d.terminal.new_tab_cwd,
                "~/src",
            ));
        }
        if cfg!(target_os = "macos") {
            col = col.push(self.pick(app, "settings-option-as-alt", "option_as_alt", alt, alt_v));
        }
        col.push(self.toggle(
            app,
            "settings-copy-on-select",
            "copy_on_select",
            d.terminal.copy_on_select,
        ))
        .push(self.toggle(
            app,
            "settings-paste-warn",
            "paste_warn",
            d.terminal.paste_warn_multiline,
        ))
        .push(self.toggle(
            app,
            "settings-url-detection",
            "url_detection",
            d.terminal.url_detection,
        ))
        .push(self.toggle(
            app,
            "settings-confirm-close",
            "confirm_close",
            d.terminal.confirm_close_when_busy,
        ))
        .into()
    }

    fn session<'a>(&'a self, app: &'a App) -> Element<'a, Msg> {
        let d = &self.draft;
        let rec = self.choices(
            app,
            &[
                ("ask", "settings-reconnect-ask"),
                ("auto", "settings-reconnect-auto"),
                ("never", "settings-reconnect-never"),
            ],
        );
        let rec_v = match d.session.reconnect_remote {
            Reconnect::Ask => "ask",
            Reconnect::Auto => "auto",
            Reconnect::Never => "never",
        };
        let cw = self.choices(
            app,
            &[
                ("detach", "settings-close-window-detach"),
                ("close_tabs", "settings-close-window-close"),
                ("ask", "settings-close-window-ask"),
            ],
        );
        let cw_v = match d.session.close_window {
            CloseWindow::Detach => "detach",
            CloseWindow::CloseTabs => "close_tabs",
            CloseWindow::Ask => "ask",
        };
        Column::new()
            .spacing(10)
            .push(self.toggle(
                app,
                "settings-restore-session",
                "restore_session",
                d.session.restore_session,
            ))
            .push(self.toggle(
                app,
                "settings-restore-scrollback",
                "restore_scrollback",
                d.session.restore_scrollback,
            ))
            .push(self.number(app, "settings-restore-lines", Field::RestoreLines))
            .push(self.pick(app, "settings-reconnect", "reconnect", rec, rec_v))
            .push(self.pick(app, "settings-close-window", "close_window", cw, cw_v))
            .push(self.toggle(
                app,
                "settings-confirm-quit",
                "confirm_quit",
                d.session.confirm_quit_when_busy,
            ))
            .push(self.number(app, "settings-autosave", Field::Autosave))
            .push(self.toggle(
                app,
                "settings-per-pane-history",
                "per_pane_history",
                d.session.per_pane_shell_history,
            ))
            .push(self.number(app, "settings-history-max", Field::HistoryMax))
            .push(self.number(app, "settings-history-days", Field::HistoryDays))
            .push(self.toggle(app, "settings-autostart", "autostart", d.daemon.autostart))
            .push(self.number(app, "settings-idle-shutdown", Field::IdleShutdown))
            .into()
    }

    fn profiles<'a>(&'a self, app: &'a App) -> Element<'a, Msg> {
        let t = &app.theme;
        let d = &self.draft;
        let mut list = column![].spacing(2).width(170.0);
        for (i, p) in d.profiles.iter().enumerate() {
            list = list.push(
                button(text(p.name.clone()).size(13))
                    .on_press(Msg::ProfileSelect(i))
                    .width(Length::Fill)
                    .padding([5, 8])
                    .style(style::row_button(t, i == self.profile)),
            );
        }
        list = list.push(
            row![
                button(text("+").size(14))
                    .on_press(Msg::ProfileAdd)
                    .padding([2, 10])
                    .style(style::secondary_button(t)),
                button(text("−").size(14))
                    .on_press(Msg::ProfileDelete)
                    .padding([2, 10])
                    .style(style::secondary_button(t)),
            ]
            .spacing(6),
        );
        let Some(p) = d.profiles.get(self.profile) else {
            return list.into();
        };
        let field = |key: &str, f: ProfileField, v: &str, ph: &str| -> Element<'a, Msg> {
            row![
                text(app.i18n.t(key))
                    .size(13)
                    .color(c(t.ui.fg_muted))
                    .width(120.0),
                text_input(ph, v)
                    .on_input(move |s| Msg::ProfileText(f, s))
                    .size(13)
                    .padding([4, 8])
                    .width(Length::Fill)
                    .style(style::input(t)),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
        };
        let editor = column![
            field("profile-name", ProfileField::Name, &p.name, ""),
            field(
                "profile-shell",
                ProfileField::Shell,
                &p.shell,
                &at_term_default_shell()
            ),
            field("profile-cwd", ProfileField::Cwd, &p.cwd, "inherit"),
            field(
                "profile-command",
                ProfileField::Command,
                &p.command,
                "ssh prod-01"
            ),
            field("profile-color", ProfileField::Color, &p.color, "#D97A7A"),
            field("profile-title", ProfileField::Title, &p.title, ""),
            checkbox(p.remote)
                .label(app.i18n.t("profile-remote"))
                .on_toggle(Msg::ProfileRemote)
                .text_size(13),
            text(app.i18n.t("profile-hint"))
                .size(11)
                .color(c(t.ui.fg_dim)),
        ]
        .spacing(8)
        .width(Length::Fill);
        row![list, editor].spacing(16).into()
    }

    fn keys_view<'a>(&'a self, app: &'a App) -> Element<'a, Msg> {
        let t = &app.theme;
        let mut col = column![
            row![
                text(app.i18n.t("settings-keys-hint"))
                    .size(11)
                    .color(c(t.ui.fg_dim))
                    .width(Length::Fill),
                button(text(app.i18n.t("settings-keys-reset")).size(12))
                    .on_press(Msg::ResetKeys)
                    .padding([4, 10])
                    .style(style::secondary_button(t)),
            ]
            .align_y(Alignment::Center)
        ]
        .spacing(4);
        for a in Action::all() {
            let name = a.name();
            let v = self.keys.get(&name).cloned().unwrap_or_default();
            let n2 = name.clone();
            col = col.push(
                row![
                    text(app.action_label(a)).size(12).width(260.0),
                    text_input("—", &v)
                        .on_input(move |s| Msg::Key(n2.clone(), s))
                        .size(12)
                        .padding([3, 6])
                        .width(Length::Fill)
                        .style(style::input(t)),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
        col.into()
    }

    fn advanced<'a>(&'a self, app: &'a App) -> Element<'a, Msg> {
        let d = &self.draft;
        let integ = self.choices(
            app,
            &[
                ("auto", "settings-integration-auto"),
                ("manual", "settings-integration-manual"),
                ("off", "settings-integration-off"),
            ],
        );
        let integ_v = match d.shell_integration.mode {
            IntegrationMode::Auto => "auto",
            IntegrationMode::Manual => "manual",
            IntegrationMode::Off => "off",
        };
        let levels: Vec<Choice> = ["error", "warn", "info", "debug", "trace"]
            .iter()
            .map(|l| Choice {
                value: l.to_string(),
                label: l.to_string(),
            })
            .collect();
        Column::new()
            .spacing(10)
            .push(self.pick(app, "settings-integration", "integration", integ, integ_v))
            .push(self.toggle(
                app,
                "settings-wrap-ssh",
                "wrap_ssh",
                d.shell_integration.wrap_ssh,
            ))
            .push(self.textf(
                app,
                "settings-editor",
                "editor",
                &d.integration.editor_command,
                "code {path}",
            ))
            .push(self.textf(
                app,
                "settings-file-manager",
                "file_manager",
                &d.integration.file_manager_command,
                "",
            ))
            .push(self.number(app, "settings-max-fps", Field::MaxFps))
            .push(self.pick(
                app,
                "settings-log-level",
                "log_level",
                levels,
                &d.advanced.log_level,
            ))
            .push(
                text(app.i18n.ta(
                    "settings-config-path",
                    &[("path", at_core::paths::config_file().display().to_string())],
                ))
                .size(11)
                .color(c(app.theme.ui.fg_dim)),
            )
            .into()
    }
}

fn at_term_default_shell() -> String {
    std::env::var("SHELL").unwrap_or_default()
}

/// Chord in config syntax (`cmd+shift+t`).
/// Splits a comma-separated chord list. A comma right after `+` is the comma key
/// itself (`cmd+,`), not a separator.
pub fn split_chords(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in value.chars() {
        if ch == ',' && !cur.trim_end().ends_with('+') {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(ch);
        }
    }
    out.push(cur);
    out.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn chord_string(c: &Chord) -> String {
    let mut parts = Vec::new();
    if c.cmd {
        parts.push("cmd");
    }
    if c.ctrl {
        parts.push("ctrl");
    }
    if c.alt {
        parts.push("alt");
    }
    if c.shift {
        parts.push("shift");
    }
    let mut s = parts.join("+");
    if !s.is_empty() {
        s.push('+');
    }
    // Spell out keys that would clash with the list separator.
    s.push_str(match c.key.as_str() {
        "," => "comma",
        k => k,
    });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chord_lists() {
        assert_eq!(split_chords("cmd+,"), vec!["cmd+,"]);
        assert_eq!(
            split_chords("cmd+shift+], ctrl+tab"),
            vec!["cmd+shift+]", "ctrl+tab"]
        );
        assert_eq!(
            split_chords("ctrl+,, cmd+comma"),
            vec!["ctrl+,", "cmd+comma"]
        );
        assert!(split_chords(" , ").is_empty());
        assert_eq!(chord_string(&Chord::parse("cmd+,").unwrap()), "cmd+comma");
    }

    /// Opening settings and saving without changes must always succeed.
    #[test]
    fn save_unchanged_and_theme_change() {
        let mut st = SettingsState::new(&Config::default());
        match st.update(Msg::Save) {
            Outcome::Save(c) => assert!(c.keys.is_empty(), "{:?}", c.keys),
            _ => panic!("save failed: {:?}", st.error),
        }
        st.update(Msg::Pick("theme", "dusk".into()));
        match st.update(Msg::Save) {
            Outcome::Save(c) => {
                assert_eq!(c.appearance.theme, "dusk");
                assert!(c.keys.is_empty());
            }
            _ => panic!("save failed: {:?}", st.error),
        }
        // A user override survives the round trip.
        st.update(Msg::Key("settings".into(), "cmd+;".into()));
        match st.update(Msg::Save) {
            Outcome::Save(c) => assert!(c.keys.contains_key("settings")),
            _ => panic!("save failed: {:?}", st.error),
        }
    }
}
