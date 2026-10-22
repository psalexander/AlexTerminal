//! Application state and update logic.

use crate::i18n::I18n;
use crate::layout::{self, SplitKind};
use crate::metrics::{Fonts, Metrics};
use crate::settings::{self, SettingsState};
use crate::term_view::PaneGrid;
use crate::{keys, platform};
use at_core::config::{Bell, CloseWindow, Config, OptionAsAlt};
use at_core::keymap::{Action, Keymap};
use at_core::model::{Direction, DropPosition, GroupMode, Workspace};
use at_core::pane::{PaneInfo, PaneState};
use at_core::theme::Theme;
use at_core::{PaneId, TabId, WindowId, paths};
use at_proto::{
    ClientMsg, CloseWindowMode, Cmd, MouseInput, MouseKind, Notice, ScrollTo, ServerMsg,
};
use crossbeam_channel::Sender;
use iced::keyboard::{self, Modifiers};
use iced::widget::operation;
use iced::{Event, Point, Size, Subscription, Task, event, mouse, window};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

pub const TITLE_H: f32 = 22.0;
pub const DIVIDER: f32 = 1.0;
pub const ROW_H: f32 = 28.0;
pub const SIDEBAR_TOP: f32 = 8.0;

#[derive(Debug, Clone)]
pub enum Message {
    Server(Arc<ServerMsg>),
    Disconnected(String),
    Event(window::Id, Event, event::Status),
    Opened(window::Id),
    Tick,
    Blink,
    Act(window::Id, Action),
    SelectTab(window::Id, TabId),
    ToggleCollapse(TabId),
    CloseTab(window::Id, TabId),
    TabPress(window::Id, TabId),
    TabDoubleClick(window::Id, TabId),
    TabRightClick(window::Id, TabId),
    TabHover(window::Id, Option<TabId>),
    SidebarMove(window::Id, Point),
    SidebarRelease(window::Id),
    SidebarScrolled(window::Id, f32),
    RenameInput(window::Id, String),
    RenameCommit(window::Id),
    MenuPick(window::Id, TabId, MenuItem),
    CloseMenu(window::Id),
    SidebarResizeStart(window::Id),
    NewTabButton(window::Id),
    ProfilesButton(window::Id),
    SettingsButton(window::Id),
    PaneMouse(PaneId, MouseInput),
    FocusPane(PaneId),
    ClosePaneButton(window::Id, PaneId),
    ZoomButton(PaneId),
    SplitPress(window::Id, TabId, SplitKind, iced::Rectangle),
    OpenLink(String),
    SearchInput(PaneId, String),
    SearchSubmit(PaneId),
    SearchStep(PaneId, bool),
    SearchRegex(PaneId),
    SearchClose(PaneId),
    Reconnect(PaneId, bool),
    RestartPane(PaneId),
    Pasted(window::Id, PaneId, Option<String>),
    PickerInput(window::Id, String),
    PickerChoose(window::Id, usize),
    PickerSubmit(window::Id),
    OverlayClose(window::Id),
    ConfirmYes(window::Id, bool),
    Settings(window::Id, settings::Msg),
    ScriptNext,
    Shot(PathBuf, window::Screenshot),
    Noop,
    /// Picked in the macOS application menu.
    MenuAction(Action),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    NewTab,
    NewNested,
    Rename,
    Duplicate,
    Close,
    CloseOthers,
    OpenFolder,
    ToNewWindow,
    ToWindow,
    Collapse,
    GroupMode(GroupMode),
    Outdent,
}

pub struct TabDrag {
    pub tab: TabId,
    pub start: Point,
    pub active: bool,
    pub target: Option<(TabId, DropPosition)>,
}

pub struct SplitDrag {
    pub tab: TabId,
    pub kind: SplitKind,
    /// Drag coordinate frame inside the pane area.
    pub frame: iced::Rectangle,
}

#[derive(Default, Clone)]
pub struct SearchUi {
    pub query: String,
    pub regex: bool,
}

pub enum Overlay {
    Picker(Picker),
    Confirm(Confirm),
    Settings(Box<SettingsState>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickerKind {
    Commands,
    Profiles,
    History(PaneId),
    Detached,
    MoveToWindow(TabId),
}

#[derive(Clone, Debug)]
pub enum PickValue {
    Action(Action),
    Profile(String),
    Command(String),
    Tab(TabId),
    Window(Option<WindowId>),
}

#[derive(Clone, Debug)]
pub struct PickItem {
    pub label: String,
    pub detail: String,
    pub value: PickValue,
}

pub struct Picker {
    pub kind: PickerKind,
    pub title: String,
    pub query: String,
    pub items: Vec<PickItem>,
    pub selected: usize,
}

impl Picker {
    pub fn filtered(&self) -> Vec<&PickItem> {
        let q = self.query.to_lowercase();
        let words: Vec<&str> = q.split_whitespace().collect();
        self.items
            .iter()
            .filter(|i| {
                let hay = format!("{} {}", i.label, i.detail).to_lowercase();
                words.iter().all(|w| hay.contains(w))
            })
            .collect()
    }
}

#[derive(Clone, Debug)]
pub enum ConfirmAction {
    ClosePane(PaneId),
    CloseTab(TabId),
    CloseWindow(WindowId, CloseWindowMode),
    QuitAll,
    Paste(PaneId, String),
}

pub struct Confirm {
    pub title: String,
    pub body: String,
    pub yes: String,
    pub action: ConfirmAction,
    pub alt: Option<(String, ConfirmAction)>,
}

pub struct Win {
    pub model: WindowId,
    pub size: Size,
    pub font_delta: f32,
    pub overlay: Option<Overlay>,
    pub rename: Option<(TabId, String)>,
    pub menu: Option<(TabId, Point)>,
    pub cursor: Point,
    pub drag: Option<TabDrag>,
    pub split: Option<SplitDrag>,
    pub sidebar_resizing: bool,
    pub sidebar_live: Option<f32>,
    pub sidebar_scroll: f32,
    pub hover: Option<TabId>,
    pub cursor_outside: bool,
    pub opened: bool,
}

impl Win {
    fn new(model: WindowId, size: Size) -> Self {
        Self {
            model,
            size,
            font_delta: 0.0,
            overlay: None,
            rename: None,
            menu: None,
            cursor: Point::ORIGIN,
            drag: None,
            split: None,
            sidebar_resizing: false,
            sidebar_live: None,
            sidebar_scroll: 0.0,
            hover: None,
            cursor_outside: false,
            opened: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Initial {
    pub cwd: Option<String>,
    pub profile: Option<String>,
    pub command: Option<String>,
}

pub struct App {
    pub tx: Sender<ClientMsg>,
    menu_installed: bool,
    pub config: Config,
    pub config_mtime: Option<SystemTime>,
    pub keymap: Keymap,
    pub theme: Theme,
    pub iced_theme: iced::Theme,
    pub i18n: I18n,
    pub fonts: Fonts,
    pub ws: Workspace,
    pub infos: HashMap<PaneId, PaneInfo>,
    pub grids: HashMap<PaneId, PaneGrid>,
    pub windows: BTreeMap<window::Id, Win>,
    pub by_model: HashMap<WindowId, window::Id>,
    pub focused: Option<window::Id>,
    pub mods: Modifiers,
    pub alt_left: bool,
    pub alt_right: bool,
    pub blink_off: bool,
    pub last_input: Instant,
    pub spin: usize,
    pub bells: HashMap<PaneId, Instant>,
    pub toasts: Vec<(String, Instant)>,
    pub welcomed: bool,
    pub expect_window: bool,
    pub initial: Initial,
    pub search: HashMap<PaneId, SearchUi>,
    pub sent_sizes: HashMap<PaneId, (u16, u16)>,
    pub sent_visible: Vec<PaneId>,
    pub focus_reported: Option<PaneId>,
    pub history_for: Option<window::Id>,
    pub exiting: bool,
    pub last_badge: usize,
    pub icon: Option<window::Icon>,
    pub script: Vec<crate::debug::Step>,
    pub script_pos: usize,
    pub script_started: bool,
}

pub fn system_dark() -> bool {
    if cfg!(target_os = "macos") {
        std::process::Command::new("defaults")
            .args(["read", "-g", "AppleInterfaceStyle"])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .eq_ignore_ascii_case("dark")
            })
            .unwrap_or(true)
    } else {
        true
    }
}

fn lang_for(config: &Config) -> &'static str {
    config.ui_language(sys_locale::get_locale().as_deref())
}

impl App {
    pub fn new(tx: Sender<ClientMsg>, initial: Initial, icon: Option<window::Icon>) -> App {
        let (config, err) = Config::load_default_path();
        let (keymap, warnings) = Keymap::build(&config.keys);
        let (theme, terr) = Theme::resolve(&config.appearance.theme, system_dark());
        let i18n = I18n::new(lang_for(&config));
        let fonts = Fonts::resolve(
            &config.appearance.font_family,
            &config.appearance.font_fallback,
        );
        let mut toasts = Vec::new();
        for e in err.into_iter().chain(terr).chain(warnings) {
            tracing::warn!("{e}");
            toasts.push((e, Instant::now()));
        }
        App {
            tx,
            config_mtime: std::fs::metadata(paths::config_file())
                .and_then(|m| m.modified())
                .ok(),
            keymap,
            iced_theme: crate::style::iced_theme(&theme),
            theme,
            i18n,
            fonts,
            config,
            ws: Workspace::default(),
            infos: HashMap::new(),
            grids: HashMap::new(),
            windows: BTreeMap::new(),
            by_model: HashMap::new(),
            focused: None,
            menu_installed: false,
            mods: Modifiers::default(),
            alt_left: false,
            alt_right: false,
            blink_off: false,
            last_input: Instant::now(),
            spin: 0,
            bells: HashMap::new(),
            toasts,
            welcomed: false,
            expect_window: true,
            initial,
            search: HashMap::new(),
            sent_sizes: HashMap::new(),
            sent_visible: Vec::new(),
            focus_reported: None,
            history_for: None,
            exiting: false,
            last_badge: 0,
            icon,
            script: crate::debug::load(),
            script_pos: 0,
            script_started: false,
        }
    }

    pub fn send(&self, m: ClientMsg) {
        let _ = self.tx.send(m);
    }

    pub fn cmd(&self, c: Cmd) {
        self.send(ClientMsg::Cmd(c));
    }

    pub fn toast(&mut self, s: impl Into<String>) {
        self.toasts.push((s.into(), Instant::now()));
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
    }

    // ----- lookups -----

    pub fn metrics(&self, win: window::Id) -> Metrics {
        let delta = self.windows.get(&win).map(|w| w.font_delta).unwrap_or(0.0);
        let size = (self.config.appearance.font_size + delta).clamp(6.0, 72.0);
        self.fonts.metrics(size, self.config.appearance.line_height)
    }

    pub fn model_of(&self, win: window::Id) -> Option<WindowId> {
        self.windows.get(&win).map(|w| w.model)
    }

    pub fn active_tab(&self, win: window::Id) -> Option<TabId> {
        self.model_of(win)
            .and_then(|m| self.ws.window(m))
            .and_then(|w| w.active_tab)
    }

    pub fn active_pane(&self, win: window::Id) -> Option<PaneId> {
        self.model_of(win).and_then(|m| self.ws.active_pane(m))
    }

    pub fn sidebar_width(&self, win: window::Id) -> f32 {
        let Some(w) = self.windows.get(&win) else {
            return 0.0;
        };
        let Some(node) = self.ws.window(w.model) else {
            return 0.0;
        };
        if node.sidebar_hidden {
            0.0
        } else {
            w.sidebar_live.unwrap_or(node.sidebar_width)
        }
    }

    pub fn pane_area(&self, win: window::Id) -> Size {
        let Some(w) = self.windows.get(&win) else {
            return Size::ZERO;
        };
        let sw = self.sidebar_width(win);
        let side = if sw > 0.0 { sw + DIVIDER } else { 0.0 };
        Size::new((w.size.width - side).max(80.0), w.size.height.max(60.0))
    }

    pub fn title_h(&self) -> f32 {
        if self.config.appearance.pane_title_bar {
            TITLE_H
        } else {
            0.0
        }
    }

    /// Indicator state of a tab: its panes, or every tab of a group.
    pub fn tab_state(&self, tab: TabId) -> PaneState {
        let Some(t) = self.ws.tab(tab) else {
            return PaneState::Idle;
        };
        let mut panes = Vec::new();
        t.all_panes(&mut panes);
        panes
            .iter()
            .filter_map(|p| self.infos.get(p))
            .map(|i| i.state())
            .min()
            .unwrap_or(PaneState::Idle)
    }

    /// Custom title, else the automatic one; a group follows its active tab.
    pub fn tab_title(&self, tab: TabId) -> String {
        let Some(t) = self.ws.tab(tab) else {
            return String::new();
        };
        if let Some(c) = &t.custom_title {
            return c.clone();
        }
        if t.is_group() {
            let child = t
                .active_child
                .and_then(|a| t.children.iter().find(|c| c.id == a))
                .or(t.children.first());
            return child
                .map(|c| self.tab_title(c.id))
                .unwrap_or_else(|| self.i18n.t("tab-untitled"));
        }
        self.infos
            .get(&t.active_pane)
            .map(|i| i.auto_title())
            .unwrap_or_else(|| self.i18n.t("tab-untitled"))
    }

    pub fn expand_single(&self) -> bool {
        self.config.appearance.expand_single_tab_groups
    }

    /// Number of terminal tabs in the group of `tab`.
    pub fn group_len(&self, tab: TabId) -> usize {
        self.ws
            .group_of(tab)
            .and_then(|g| self.ws.tab(g))
            .map(|g| g.children.len())
            .unwrap_or(0)
    }

    pub fn window_title(&self, win: window::Id) -> String {
        match self.active_tab(win) {
            Some(t) => format!("{} — AlexTerminal", self.tab_title(t)),
            None => "AlexTerminal".into(),
        }
    }

    fn busy_panes(&self, panes: &[PaneId]) -> (usize, usize) {
        let mut running = 0;
        let mut remote = 0;
        for p in panes {
            if let Some(i) = self.infos.get(p) {
                if i.exited.is_some() {
                    continue;
                }
                if i.remote_host.is_some() {
                    remote += 1;
                } else if i.running {
                    running += 1;
                }
            }
        }
        (running, remote)
    }

    fn busy_text(&self, running: usize, remote: usize) -> String {
        self.i18n.ta(
            "confirm-busy-body",
            &[
                ("running", running.to_string()),
                ("remote", remote.to_string()),
            ],
        )
    }

    // ----- subscription -----

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![
            Subscription::run(crate::conn::stream),
            Subscription::run(crate::menu::stream),
            event::listen_with(|e, status, id| match &e {
                Event::Keyboard(_) | Event::Window(_) => Some(Message::Event(id, e, status)),
                Event::Mouse(mouse::Event::ButtonReleased(_))
                | Event::Mouse(mouse::Event::CursorLeft)
                | Event::Mouse(mouse::Event::CursorEntered) => Some(Message::Event(id, e, status)),
                _ => None,
            }),
        ];
        let dragging = self
            .windows
            .values()
            .any(|w| w.drag.is_some() || w.split.is_some() || w.sidebar_resizing);
        if dragging {
            subs.push(event::listen_with(|e, status, id| match &e {
                Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                    Some(Message::Event(id, e, status))
                }
                _ => None,
            }));
        }
        let running = self.config.appearance.animate_indicators
            && self.infos.values().any(|i| i.state() == PaneState::Running);
        let animate = running || !self.bells.is_empty() || !self.toasts.is_empty();
        if animate {
            subs.push(iced::time::every(Duration::from_millis(100)).map(|_| Message::Tick));
        }
        if self.config.appearance.cursor_blink {
            subs.push(iced::time::every(Duration::from_millis(530)).map(|_| Message::Blink));
        }
        // Config hot reload.
        subs.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::Noop));
        Subscription::batch(subs)
    }

    // ----- update -----

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        if !self.menu_installed {
            self.menu_installed =
                crate::menu::install(&[(Action::Quit, self.action_label(Action::Quit))]);
        }
        let task = self.handle(msg);
        let t2 = self.after_update();
        Task::batch([task, t2])
    }

    fn handle(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::Server(m) => self.on_server(m),
            Message::Disconnected(why) => {
                // Quitting: the server is gone, so go too.
                if !self.exiting {
                    tracing::error!("session server stopped: {why}");
                    eprintln!(
                        "AlexTerminal: {}",
                        self.i18n.ta("error-server-lost", &[("reason", why)])
                    );
                }
                self.exiting = true;
                iced::exit()
            }
            Message::Event(id, e, status) => self.on_event(id, e, status),
            Message::Opened(id) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.opened = true;
                }
                Task::none()
            }
            Message::Tick => {
                self.spin = (self.spin + 1) % 8;
                let now = Instant::now();
                self.bells
                    .retain(|_, t| now.duration_since(*t) < Duration::from_millis(180));
                self.toasts
                    .retain(|(_, t)| now.duration_since(*t) < Duration::from_secs(5));
                Task::none()
            }
            Message::Blink => {
                self.blink_off = if self.last_input.elapsed() < Duration::from_millis(600) {
                    false
                } else {
                    !self.blink_off
                };
                Task::none()
            }
            Message::Noop => {
                self.check_config();
                Task::none()
            }
            Message::Act(id, a) => self.perform(id, a),
            Message::MenuAction(a) => {
                match self.focused.or_else(|| self.windows.keys().next().copied()) {
                    Some(w) => self.perform(w, a),
                    None => Task::none(),
                }
            }
            Message::SelectTab(id, t) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.menu = None;
                }
                self.cmd(Cmd::SelectTab { tab: t });
                Task::none()
            }
            Message::ToggleCollapse(t) => {
                self.cmd(Cmd::ToggleCollapse { tab: t });
                Task::none()
            }
            Message::CloseTab(id, t) => self.close_tab(id, t),
            Message::TabPress(id, t) => {
                let cursor = self.windows.get(&id).map(|w| w.cursor).unwrap_or_default();
                if let Some(w) = self.windows.get_mut(&id) {
                    w.menu = None;
                    w.drag = Some(TabDrag {
                        tab: t,
                        start: cursor,
                        active: false,
                        target: None,
                    });
                }
                self.cmd(Cmd::SelectTab { tab: t });
                Task::none()
            }
            Message::TabDoubleClick(id, t) => self.start_rename(id, t),
            Message::TabRightClick(id, t) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.menu = Some((t, w.cursor));
                }
                Task::none()
            }
            Message::TabHover(id, t) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.hover = t;
                }
                Task::none()
            }
            Message::SidebarMove(id, p) => {
                self.sidebar_move(id, p);
                Task::none()
            }
            Message::SidebarRelease(id) => self.finish_drag(id),
            Message::SidebarScrolled(id, y) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.sidebar_scroll = y;
                }
                Task::none()
            }
            Message::RenameInput(id, s) => {
                if let Some(w) = self.windows.get_mut(&id)
                    && let Some((_, v)) = w.rename.as_mut()
                {
                    *v = s;
                }
                Task::none()
            }
            Message::RenameCommit(id) => {
                if let Some((tab, title)) = self.windows.get_mut(&id).and_then(|w| w.rename.take())
                {
                    self.cmd(Cmd::RenameTab {
                        tab,
                        title: Some(title),
                    });
                }
                Task::none()
            }
            Message::MenuPick(id, tab, item) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.menu = None;
                }
                self.menu_pick(id, tab, item)
            }
            Message::CloseMenu(id) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.menu = None;
                }
                Task::none()
            }
            Message::SidebarResizeStart(id) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.sidebar_resizing = true;
                }
                Task::none()
            }
            Message::NewTabButton(id) => self.perform(id, Action::NewTab),
            Message::ProfilesButton(id) => self.perform(id, Action::NewTabFromProfile),
            Message::SettingsButton(id) => self.perform(id, Action::Settings),
            Message::PaneMouse(pane, input) => {
                if input.kind == MouseKind::Press {
                    self.focus_pane_if_needed(pane);
                    for w in self.windows.values_mut() {
                        w.menu = None;
                    }
                }
                self.send(ClientMsg::Mouse { pane, input });
                Task::none()
            }
            Message::FocusPane(p) => {
                self.focus_pane_if_needed(p);
                Task::none()
            }
            Message::ClosePaneButton(id, p) => self.close_pane(id, p),
            Message::ZoomButton(p) => {
                if let Some(t) = self.ws.tab_of_pane(p) {
                    self.cmd(Cmd::FocusPane { pane: p });
                    self.cmd(Cmd::ZoomPane { tab: t });
                }
                Task::none()
            }
            Message::SplitPress(id, tab, kind, frame) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.split = Some(SplitDrag { tab, kind, frame });
                }
                Task::none()
            }
            Message::OpenLink(link) => {
                if let Err(e) = platform::open_url(&link) {
                    self.toast(e.to_string());
                }
                Task::none()
            }
            Message::SearchInput(p, q) => {
                let ui = self.search.entry(p).or_default();
                ui.query = q.clone();
                let regex = ui.regex;
                self.send(ClientMsg::Search {
                    pane: p,
                    query: q,
                    regex,
                    forward: true,
                });
                Task::none()
            }
            Message::SearchSubmit(p) => {
                let back = self.mods.shift();
                self.search_step(p, !back);
                Task::none()
            }
            Message::SearchStep(p, fwd) => {
                self.search_step(p, fwd);
                Task::none()
            }
            Message::SearchRegex(p) => {
                let ui = self.search.entry(p).or_default();
                ui.regex = !ui.regex;
                let (q, r) = (ui.query.clone(), ui.regex);
                self.send(ClientMsg::Search {
                    pane: p,
                    query: q,
                    regex: r,
                    forward: true,
                });
                Task::none()
            }
            Message::SearchClose(p) => {
                self.search.remove(&p);
                self.send(ClientMsg::SearchClose { pane: p });
                Task::none()
            }
            Message::Reconnect(p, yes) => {
                self.cmd(Cmd::Reconnect {
                    pane: p,
                    accept: yes,
                });
                Task::none()
            }
            Message::RestartPane(p) => {
                self.cmd(Cmd::RestartShell { pane: p });
                Task::none()
            }
            Message::Pasted(id, pane, text) => {
                let Some(text) = text.filter(|t| !t.is_empty()) else {
                    return Task::none();
                };
                if self.config.terminal.paste_warn_multiline
                    && text.trim_end().contains('\n')
                    && !self.mode_has(pane, at_proto::mode::BRACKETED_PASTE)
                {
                    let lines = text.trim_end().lines().count();
                    let c = Confirm {
                        title: self.i18n.t("confirm-paste-title"),
                        body: self.i18n.tn("confirm-paste-body", "lines", lines as i64),
                        yes: self.i18n.t("button-paste"),
                        action: ConfirmAction::Paste(pane, text),
                        alt: None,
                    };
                    if let Some(w) = self.windows.get_mut(&id) {
                        w.overlay = Some(Overlay::Confirm(c));
                    }
                } else {
                    self.send(ClientMsg::Paste { pane, text });
                }
                Task::none()
            }
            Message::PickerInput(id, q) => {
                if let Some(Overlay::Picker(p)) =
                    self.windows.get_mut(&id).and_then(|w| w.overlay.as_mut())
                {
                    p.query = q;
                    p.selected = 0;
                }
                Task::none()
            }
            Message::PickerChoose(id, i) => self.picker_choose(id, i),
            Message::PickerSubmit(id) => {
                let sel = match self.windows.get(&id).and_then(|w| w.overlay.as_ref()) {
                    Some(Overlay::Picker(p)) => p.selected,
                    _ => return Task::none(),
                };
                self.picker_choose(id, sel)
            }
            Message::OverlayClose(id) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.overlay = None;
                    w.menu = None;
                }
                Task::none()
            }
            Message::ConfirmYes(id, alt) => {
                let Some(Overlay::Confirm(c)) =
                    self.windows.get_mut(&id).and_then(|w| w.overlay.take())
                else {
                    return Task::none();
                };
                let action = if alt {
                    c.alt.map(|a| a.1).unwrap_or(c.action)
                } else {
                    c.action
                };
                self.confirmed(action)
            }
            Message::Settings(id, m) => self.settings_msg(id, m),
            Message::ScriptNext => self.script_step(),
            Message::Shot(path, shot) => {
                if let Err(e) = crate::debug::write_png(&path, &shot) {
                    tracing::error!("screenshot {}: {e}", path.display());
                }
                Task::done(Message::ScriptNext)
            }
        }
    }

    /// Runs debug-script steps until one needs to wait.
    fn script_step(&mut self) -> Task<Message> {
        use crate::debug::Step;
        while let Some(step) = self.script.get(self.script_pos).cloned() {
            self.script_pos += 1;
            let win = self.focused.or_else(|| self.windows.keys().next().copied());
            match step {
                Step::Wait(ms) => {
                    return Task::perform(tokio_sleep(Duration::from_millis(ms)), |_| {
                        Message::ScriptNext
                    });
                }
                Step::Text(s) => {
                    if let Some(p) = win.and_then(|w| self.active_pane(w)) {
                        self.send(ClientMsg::Text { pane: p, text: s });
                    }
                }
                Step::Action(a) => {
                    if let Some(w) = win {
                        let t = self.perform(w, a);
                        return Task::batch([
                            t,
                            Task::perform(tokio_sleep(Duration::from_millis(150)), |_| {
                                Message::ScriptNext
                            }),
                        ]);
                    }
                }
                Step::Shot(path) => {
                    if let Some(w) = win {
                        return window::screenshot(w).map(move |s| Message::Shot(path.clone(), s));
                    }
                }
                Step::Quit => {
                    return self.perform(win.unwrap_or(window::Id::unique()), Action::Quit);
                }
            }
        }
        Task::none()
    }

    fn mode_has(&self, pane: PaneId, bit: u32) -> bool {
        self.grids
            .get(&pane)
            .map(|g| g.mode & bit != 0)
            .unwrap_or(false)
    }

    fn search_step(&mut self, p: PaneId, forward: bool) {
        if let Some(ui) = self.search.get(&p) {
            self.send(ClientMsg::Search {
                pane: p,
                query: ui.query.clone(),
                regex: ui.regex,
                forward,
            });
        }
    }

    fn focus_pane_if_needed(&mut self, pane: PaneId) {
        let Some(tab) = self.ws.tab_of_pane(pane) else {
            return;
        };
        let Some(t) = self.ws.tab(tab) else { return };
        let tab_active = self.ws.windows.iter().any(|w| w.active_tab == Some(tab));
        if t.active_pane != pane || !tab_active {
            self.cmd(Cmd::FocusPane { pane });
            if let Some(t) = self.ws.tab_mut(tab) {
                t.active_pane = pane;
            }
            self.ws.select_tab(tab);
        }
    }

    fn check_config(&mut self) {
        let m = std::fs::metadata(paths::config_file())
            .and_then(|m| m.modified())
            .ok();
        if m != self.config_mtime {
            self.config_mtime = m;
            self.reload_config();
        }
    }

    pub fn reload_config(&mut self) {
        let (config, err) = Config::load_default_path();
        if let Some(e) = err {
            self.toast(self.i18n.ta("error-config", &[("error", e)]));
            return;
        }
        let (keymap, warnings) = Keymap::build(&config.keys);
        for w in warnings {
            self.toast(w);
        }
        let (theme, terr) = Theme::resolve(&config.appearance.theme, system_dark());
        if let Some(e) = terr {
            self.toast(e);
        }
        if config.appearance.font_family != self.config.appearance.font_family
            || config.appearance.font_fallback != self.config.appearance.font_fallback
        {
            self.fonts = Fonts::resolve(
                &config.appearance.font_family,
                &config.appearance.font_fallback,
            );
        }
        self.i18n = I18n::new(lang_for(&config));
        self.iced_theme = crate::style::iced_theme(&theme);
        self.theme = theme;
        self.keymap = keymap;
        self.config = config;
        for g in self.grids.values() {
            g.cache.clear();
        }
        self.sent_sizes.clear();
    }

    // ----- server messages -----

    fn on_server(&mut self, m: Arc<ServerMsg>) -> Task<Message> {
        let m = Arc::try_unwrap(m).unwrap_or_else(|a| (*a).clone());
        match m {
            ServerMsg::Welcome {
                workspace, panes, ..
            } => {
                self.welcomed = true;
                self.ws = workspace;
                self.infos = panes.into_iter().map(|p| (p.id, p)).collect();
                if self.ws.windows.is_empty() {
                    self.expect_window = true;
                    if !self.ws.detached.is_empty()
                        && self.initial.cwd.is_none()
                        && self.initial.command.is_none()
                    {
                        self.cmd(Cmd::RestoreDetachedWindow);
                    } else {
                        let i = self.initial.clone();
                        self.cmd(Cmd::NewWindow {
                            cwd: i.cwd,
                            profile: i.profile,
                            command: i.command,
                            geom: None,
                        });
                    }
                } else {
                    self.expect_window = false;
                    if self.initial.cwd.is_some()
                        || self.initial.command.is_some()
                        || self.initial.profile.is_some()
                    {
                        let i = self.initial.clone();
                        let window = self
                            .ws
                            .focused_window
                            .filter(|w| self.ws.window(*w).is_some())
                            .unwrap_or(self.ws.windows[0].id);
                        self.cmd(Cmd::NewTab {
                            window,
                            nested: false,
                            profile: i.profile,
                            cwd: i.cwd,
                            command: i.command,
                        });
                    }
                }
                // CLI arguments apply to the first connection only, not to reconnects.
                self.initial = Initial::default();
                return self.reconcile();
            }
            ServerMsg::Workspace(ws) => {
                if !ws.windows.is_empty() {
                    self.expect_window = false;
                }
                self.ws = ws;
                // Close a rename/menu that points to a vanished tab.
                for w in self.windows.values_mut() {
                    if let Some((t, _)) = &w.rename
                        && self.ws.tab(*t).is_none()
                    {
                        w.rename = None;
                    }
                }
                let live: HashSet<PaneId> = self.ws.all_panes().into_iter().collect();
                self.search.retain(|p, _| live.contains(p));
                return self.reconcile();
            }
            ServerMsg::Pane(info) => {
                self.infos.insert(info.id, info);
            }
            ServerMsg::PaneRemoved(id) => {
                self.infos.remove(&id);
                self.grids.remove(&id);
                self.sent_sizes.remove(&id);
            }
            ServerMsg::Frame(f) => {
                let id = f.pane;
                self.grids.entry(id).or_default().apply(f);
            }
            ServerMsg::Clipboard(text) => {
                return iced::clipboard::write(text);
            }
            ServerMsg::Bell(p) => match self.config.terminal.bell {
                Bell::Visual => {
                    self.bells.insert(p, Instant::now());
                }
                Bell::Sound => platform::bell_sound(),
                Bell::Both => {
                    self.bells.insert(p, Instant::now());
                    platform::bell_sound();
                }
                Bell::None => {}
            },
            ServerMsg::History { pane, entries } => {
                if let Some(id) = self.history_for.take() {
                    let mut items: Vec<PickItem> = entries
                        .into_iter()
                        .rev()
                        .map(|e| {
                            let when = at_core::time::format_local(e.ts_ms, self.i18n.code);
                            let exit = match e.exit {
                                Some(0) | None => String::new(),
                                Some(c) => format!("  ✗{c}"),
                            };
                            PickItem {
                                label: e.command.clone(),
                                detail: format!(
                                    "{when}  {}{exit}",
                                    at_core::pane::tilde_path(&e.cwd)
                                ),
                                value: PickValue::Command(e.command),
                            }
                        })
                        .collect();
                    // Most recent first, without duplicates.
                    let mut seen = HashSet::new();
                    items.retain(|i| seen.insert(i.label.clone()));
                    let picker = Picker {
                        kind: PickerKind::History(pane),
                        title: self.i18n.t("picker-history"),
                        query: String::new(),
                        items,
                        selected: 0,
                    };
                    if let Some(w) = self.windows.get_mut(&id) {
                        w.overlay = Some(Overlay::Picker(picker));
                    }
                    return operation::focus("picker-input");
                }
            }
            ServerMsg::Notice(n) => {
                let text = match n {
                    Notice::ConfigError(e) => self.i18n.ta("error-config", &[("error", e)]),
                    Notice::SessionRestoredFromBackup(p) => {
                        self.i18n.ta("notice-restored-backup", &[("path", p)])
                    }
                    Notice::SessionUnreadable(p) => {
                        self.i18n.ta("notice-session-unreadable", &[("path", p)])
                    }
                    Notice::CwdMissing { cwd, .. } => {
                        self.i18n.ta("notice-cwd-missing", &[("path", cwd)])
                    }
                    Notice::Info(s) => s,
                };
                self.toast(text);
            }
            ServerMsg::Error(e) => {
                self.toast(e.clone());
                tracing::error!("server error: {e}");
            }
            ServerMsg::ShuttingDown => {
                // Exit on `Disconnected`, once the server has ended the shells and saved.
                self.exiting = true;
            }
            ServerMsg::Pong => {}
        }
        Task::none()
    }

    fn window_settings(&self, geom: Option<&at_core::model::WindowGeom>) -> window::Settings {
        let mut s = window::Settings {
            size: Size::new(1180.0, 760.0),
            min_size: Some(Size::new(520.0, 320.0)),
            exit_on_close_request: false,
            icon: self.icon.clone(),
            transparent: self.config.appearance.window_opacity < 1.0,
            blur: self.config.appearance.window_blur,
            ..Default::default()
        };
        if let Some(g) = geom
            && g.w >= 300.0
            && g.h >= 200.0
        {
            s.size = Size::new(g.w, g.h);
            s.position = window::Position::Specific(Point::new(g.x, g.y));
            s.maximized = g.maximized;
        }
        s
    }

    /// Opens iced windows for new workspace windows and closes vanished ones.
    fn reconcile(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        let models: Vec<(WindowId, Option<at_core::model::WindowGeom>)> = self
            .ws
            .windows
            .iter()
            .map(|w| (w.id, w.geom.clone()))
            .collect();
        for (mid, geom) in models {
            if self.by_model.contains_key(&mid) {
                continue;
            }
            let settings = self.window_settings(geom.as_ref());
            let size = settings.size;
            let (id, open) = window::open(settings);
            self.windows.insert(id, Win::new(mid, size));
            self.by_model.insert(mid, id);
            tasks.push(open.map(Message::Opened));
            tasks.push(window::gain_focus(id));
        }
        let gone: Vec<window::Id> = self
            .windows
            .iter()
            .filter(|(_, w)| self.ws.window(w.model).is_none())
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            if let Some(w) = self.windows.remove(&id) {
                self.by_model.remove(&w.model);
            }
            tasks.push(window::close(id));
        }
        // The last window is gone: sessions end with the app.
        if self.welcomed && self.ws.windows.is_empty() && !self.expect_window && !self.exiting {
            tasks.push(self.confirmed(ConfirmAction::QuitAll));
        }
        Task::batch(tasks)
    }

    /// Sends sizes, visibility and focus to the server after every update.
    fn after_update(&mut self) -> Task<Message> {
        let ids: Vec<window::Id> = self.windows.keys().copied().collect();
        let mut visible = Vec::new();
        let pad = self.config.appearance.pane_padding;
        let title_h = self.title_h();
        for id in ids {
            let Some(tab) = self.active_tab(id) else {
                continue;
            };
            let area = self.pane_area(id);
            let m = self.metrics(id);
            let sc = layout::scene(&self.ws, tab, area.width, area.height);
            for (_, pane, rect) in sc.panes {
                visible.push(pane);
                let (cols, rows) = layout::cells(&rect, title_h, pad, m.cell_w, m.cell_h);
                if self.sent_sizes.get(&pane) != Some(&(cols, rows)) {
                    self.sent_sizes.insert(pane, (cols, rows));
                    self.send(ClientMsg::Resize {
                        pane,
                        cols,
                        rows,
                        cell_w: m.cell_w.round() as u16,
                        cell_h: m.cell_h.round() as u16,
                    });
                }
            }
        }
        visible.sort();
        if visible != self.sent_visible {
            self.sent_visible = visible.clone();
            self.send(ClientMsg::Visible { panes: visible });
        }
        let focus = self.focused.and_then(|w| self.active_pane(w));
        if focus != self.focus_reported {
            if let Some(old) = self.focus_reported {
                self.send(ClientMsg::Focus {
                    pane: old,
                    focused: false,
                });
            }
            if let Some(new) = focus {
                self.send(ClientMsg::Focus {
                    pane: new,
                    focused: true,
                });
            }
            self.focus_reported = focus;
        }
        let attention = self
            .ws
            .windows
            .iter()
            .flat_map(|w| self.ws.terminal_tabs(w.id))
            .filter(|t| self.tab_state(*t) == PaneState::Attention)
            .count();
        if attention != self.last_badge {
            self.last_badge = attention;
            platform::set_badge(attention);
        }
        Task::none()
    }

    // ----- events -----

    fn on_event(&mut self, id: window::Id, e: Event, status: event::Status) -> Task<Message> {
        match e {
            Event::Window(we) => self.on_window_event(id, we),
            Event::Keyboard(ke) => self.on_key(id, ke, status),
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.cursor = position;
                    w.cursor_outside = false;
                }
                self.drag_move(id, position);
                Task::none()
            }
            Event::Mouse(mouse::Event::CursorLeft) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.cursor_outside = true;
                }
                Task::none()
            }
            Event::Mouse(mouse::Event::CursorEntered) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.cursor_outside = false;
                }
                Task::none()
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => self.finish_drag(id),
            _ => Task::none(),
        }
    }

    fn on_window_event(&mut self, id: window::Id, we: window::Event) -> Task<Message> {
        match we {
            window::Event::Opened { position, size } => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.size = size;
                    w.opened = true;
                }
                if !self.script.is_empty() && !self.script_started {
                    self.script_started = true;
                    self.focused = Some(id);
                    return Task::perform(tokio_sleep(Duration::from_millis(800)), |_| {
                        Message::ScriptNext
                    });
                }
                if let (Some(p), Some(m)) = (position, self.model_of(id)) {
                    self.cmd(Cmd::WindowGeometry {
                        window: m,
                        geom: at_core::model::WindowGeom {
                            x: p.x,
                            y: p.y,
                            w: size.width,
                            h: size.height,
                            maximized: false,
                        },
                    });
                }
            }
            window::Event::Resized(size) => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.size = size;
                }
                self.store_geometry(id, None, Some(size));
            }
            window::Event::Moved(p) => self.store_geometry(id, Some(p), None),
            window::Event::Focused => {
                self.focused = Some(id);
                if let Some(m) = self.model_of(id) {
                    self.cmd(Cmd::FocusWindow { window: m });
                }
            }
            window::Event::Unfocused => {
                if self.focused == Some(id) {
                    self.focused = None;
                }
                self.mods = Modifiers::default();
            }
            window::Event::CloseRequested => return self.request_close_window(id),
            window::Event::Closed => {
                if let Some(w) = self.windows.remove(&id) {
                    self.by_model.remove(&w.model);
                }
            }
            window::Event::FileDropped(path) => {
                if let Some(p) = self.active_pane(id) {
                    let text = format!("{} ", platform::shell_quote(&path.display().to_string()));
                    self.send(ClientMsg::Text { pane: p, text });
                }
            }
            _ => {}
        }
        Task::none()
    }

    fn store_geometry(&mut self, id: window::Id, pos: Option<Point>, size: Option<Size>) {
        let Some(m) = self.model_of(id) else { return };
        let mut g = self
            .ws
            .window(m)
            .and_then(|w| w.geom.clone())
            .unwrap_or_default();
        if let Some(p) = pos {
            g.x = p.x;
            g.y = p.y;
        }
        if let Some(s) = size {
            g.w = s.width;
            g.h = s.height;
        }
        if let Some(w) = self.ws.window_mut(m) {
            w.geom = Some(g.clone());
        }
        self.cmd(Cmd::WindowGeometry { window: m, geom: g });
    }

    fn on_key(
        &mut self,
        id: window::Id,
        ke: keyboard::Event,
        status: event::Status,
    ) -> Task<Message> {
        match ke {
            keyboard::Event::ModifiersChanged(m) => {
                self.mods = m;
                if !m.alt() {
                    self.alt_left = false;
                    self.alt_right = false;
                }
                Task::none()
            }
            keyboard::Event::KeyReleased { .. } => Task::none(),
            keyboard::Event::KeyPressed {
                key,
                physical_key,
                location,
                modifiers,
                text,
                ..
            } => {
                self.mods = modifiers;
                if let keyboard::Key::Named(keyboard::key::Named::Alt) = key {
                    match location {
                        keyboard::Location::Right => self.alt_right = true,
                        _ => self.alt_left = true,
                    }
                    return Task::none();
                }
                self.last_input = Instant::now();
                self.blink_off = false;
                let captured = status == event::Status::Captured;
                // Overlays and inline editors first.
                if let Some(t) = self.overlay_key(id, &key, captured) {
                    return t;
                }
                if let Some(chord) = keys::chord(&key, &physical_key, modifiers)
                    && let Some(action) = self.keymap.lookup(&chord)
                {
                    let editing =
                        matches!(action, Action::Copy | Action::Paste | Action::SelectAll);
                    if !(captured && editing) {
                        return self.perform(id, action);
                    }
                }
                if captured || self.text_focus(id) {
                    return Task::none();
                }
                let Some(pane) = self.active_pane(id) else {
                    return Task::none();
                };
                if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape))
                    && self.search.contains_key(&pane)
                {
                    self.search.remove(&pane);
                    self.send(ClientMsg::SearchClose { pane });
                    return Task::none();
                }
                let alt_is_meta = match self.config.terminal.option_as_alt {
                    OptionAsAlt::None => !cfg!(target_os = "macos"),
                    OptionAsAlt::Both => true,
                    OptionAsAlt::Left => self.alt_left || !cfg!(target_os = "macos"),
                    OptionAsAlt::Right => self.alt_right || !cfg!(target_os = "macos"),
                };
                if let Some(input) =
                    keys::key_input(&key, &physical_key, text.as_deref(), modifiers, alt_is_meta)
                {
                    self.send(ClientMsg::Key { pane, key: input });
                }
                Task::none()
            }
        }
    }

    /// Whether an inline text field (rename, search) has focus in this window.
    fn text_focus(&self, id: window::Id) -> bool {
        let Some(w) = self.windows.get(&id) else {
            return false;
        };
        w.rename.is_some() || w.overlay.is_some()
    }

    fn overlay_key(
        &mut self,
        id: window::Id,
        key: &keyboard::Key,
        _captured: bool,
    ) -> Option<Task<Message>> {
        use keyboard::key::Named;
        let w = self.windows.get_mut(&id)?;
        if w.menu.is_some()
            && let keyboard::Key::Named(Named::Escape) = key
        {
            w.menu = None;
            return Some(Task::none());
        }
        if w.rename.is_some() {
            if let keyboard::Key::Named(Named::Escape) = key {
                w.rename = None;
                return Some(Task::none());
            }
            return None;
        }
        match w.overlay.as_mut()? {
            Overlay::Picker(p) => match key {
                keyboard::Key::Named(Named::Escape) => {
                    w.overlay = None;
                    Some(Task::none())
                }
                keyboard::Key::Named(Named::ArrowDown) => {
                    let n = p.filtered().len();
                    if n > 0 {
                        p.selected = (p.selected + 1) % n;
                    }
                    Some(Task::none())
                }
                keyboard::Key::Named(Named::ArrowUp) => {
                    let n = p.filtered().len();
                    if n > 0 {
                        p.selected = (p.selected + n - 1) % n;
                    }
                    Some(Task::none())
                }
                keyboard::Key::Named(Named::Enter) if self.mods.logo() || self.mods.control() => {
                    // ⌘↩ in the history picker runs the command.
                    let sel = p.selected;
                    Some(self.picker_choose_exec(id, sel, true))
                }
                _ => None,
            },
            Overlay::Confirm(_) => match key {
                keyboard::Key::Named(Named::Escape) => {
                    w.overlay = None;
                    Some(Task::none())
                }
                keyboard::Key::Named(Named::Enter) => {
                    Some(self.update(Message::ConfirmYes(id, false)))
                }
                _ => Some(Task::none()),
            },
            Overlay::Settings(_) => match key {
                keyboard::Key::Named(Named::Escape) => {
                    w.overlay = None;
                    Some(Task::none())
                }
                _ => None,
            },
        }
    }

    // ----- drags -----

    fn sidebar_move(&mut self, id: window::Id, p: Point) {
        if let Some(w) = self.windows.get_mut(&id) {
            w.cursor = p;
        }
        self.drag_move(id, p);
    }

    fn drop_target(&self, id: window::Id, p: Point) -> Option<(TabId, DropPosition)> {
        let w = self.windows.get(&id)?;
        let rows = self.ws.visible_tabs(w.model, self.expand_single());
        let y = p.y - SIDEBAR_TOP + w.sidebar_scroll;
        if y < 0.0 || p.x > self.sidebar_width(id) {
            return None;
        }
        let idx = (y / ROW_H).floor() as usize;
        let row = rows.get(idx).or(rows.last())?;
        let within = (y - idx as f32 * ROW_H) / ROW_H;
        let pos = if idx >= rows.len() {
            DropPosition::After
        } else if within < 0.28 {
            DropPosition::Before
        } else if within > 0.72 {
            DropPosition::After
        } else {
            DropPosition::Child
        };
        Some((row.id, pos))
    }

    fn drag_move(&mut self, id: window::Id, p: Point) {
        let sidebar_w = self.sidebar_width(id);
        let target = self.drop_target(id, p);
        let area = self.pane_area(id);
        let tab = self.active_tab(id);
        let tab_node = tab.and_then(|t| self.ws.tab(t).cloned());
        let Some(w) = self.windows.get_mut(&id) else {
            return;
        };
        w.cursor = p;
        if w.sidebar_resizing {
            w.sidebar_live = Some(p.x.clamp(160.0, 400.0));
            return;
        }
        if let Some(d) = w.drag.as_mut() {
            if !d.active && (p.y - d.start.y).abs() + (p.x - d.start.x).abs() > 6.0 {
                d.active = true;
            }
            if d.active {
                d.target = target.filter(|(t, _)| *t != d.tab);
            }
            return;
        }
        let Some(s) = w.split.as_ref() else { return };
        let (owner, kind, frame) = (s.tab, s.kind, s.frame);
        let side = sidebar_w + if sidebar_w > 0.0 { DIVIDER } else { 0.0 };
        let x = p.x - side - frame.x;
        let y = p.y - frame.y;
        let min_px = 5.0 * 16.0 + TITLE_H;
        match kind {
            SplitKind::Stack(i) => {
                if let Some(tm) = self.ws.tab_mut(owner) {
                    tm.sizes = layout::drag_stack(&tm.sizes, i, y, frame.height, min_px);
                }
            }
            SplitKind::GridRow => {
                let r = (y / frame.height).clamp(0.15, 0.85);
                if let Some(tm) = self.ws.tab_mut(owner) {
                    tm.grid_rows = [r, 1.0 - r];
                }
            }
            SplitKind::GridCol => {
                let c = (x / frame.width).clamp(0.15, 0.85);
                if let Some(tm) = self.ws.tab_mut(owner) {
                    tm.grid_cols = [c, 1.0 - c];
                }
            }
            SplitKind::Group(i) => {
                let cur = self.ws.group_sizes(owner);
                // Same gap as the scene so the splitter follows the pointer exactly.
                let n = cur.len() as f32;
                let avail = frame.height - layout::GROUP_GAP * (n - 1.0);
                let mut px: Vec<f32> = cur.iter().map(|f| f * avail).collect();
                let top: f32 = px[..i].iter().sum::<f32>() + layout::GROUP_GAP * i as f32;
                let pair = px[i] + px[i + 1];
                let min_sec = layout::SECTION_HEADER + 3.0 * 16.0;
                px[i] = (y - top).clamp(min_sec, (pair - min_sec).max(min_sec));
                px[i + 1] = pair - px[i];
                let sizes: Vec<f32> = px.iter().map(|v| v / avail.max(1.0)).collect();
                self.ws.set_group_sizes(owner, sizes);
            }
        }
        let _ = (area, tab, tab_node);
    }

    fn finish_drag(&mut self, id: window::Id) -> Task<Message> {
        let Some(w) = self.windows.get_mut(&id) else {
            return Task::none();
        };
        if w.sidebar_resizing {
            w.sidebar_resizing = false;
            if let Some(width) = w.sidebar_live.take() {
                let model = w.model;
                if let Some(node) = self.ws.window_mut(model) {
                    node.sidebar_width = width;
                }
                self.cmd(Cmd::Sidebar {
                    window: model,
                    width,
                    hidden: false,
                });
            }
            return Task::none();
        }
        if let Some(s) = w.split.take() {
            if let SplitKind::Group(_) = s.kind {
                let sizes = self.ws.group_sizes(s.tab);
                self.cmd(Cmd::SetGroupSizes { root: s.tab, sizes });
                return Task::none();
            }
            if let Some(t) = self.ws.tab(s.tab) {
                match s.kind {
                    SplitKind::Stack(_) => self.cmd(Cmd::SetSizes {
                        tab: t.id,
                        sizes: t.sizes.clone(),
                    }),
                    _ => self.cmd(Cmd::SetGrid {
                        tab: t.id,
                        rows: t.grid_rows,
                        cols: t.grid_cols,
                    }),
                }
            }
            return Task::none();
        }
        if let Some(d) = w.drag.take() {
            if !d.active {
                return Task::none();
            }
            if w.cursor_outside {
                self.cmd(Cmd::MoveTabToWindow {
                    tab: d.tab,
                    window: None,
                });
            } else if let Some((target, pos)) = d.target {
                self.cmd(Cmd::DropTab {
                    tab: d.tab,
                    target,
                    pos,
                });
            }
        }
        Task::none()
    }

    // ----- tabs, panes, windows -----

    fn start_rename(&mut self, id: window::Id, tab: TabId) -> Task<Message> {
        let title = self.tab_title(tab);
        let hidden = self
            .model_of(id)
            .and_then(|m| self.ws.window(m))
            .map(|w| w.sidebar_hidden)
            .unwrap_or(false);
        if hidden && let Some(m) = self.model_of(id) {
            let width = self.ws.window(m).map(|w| w.sidebar_width).unwrap_or(220.0);
            self.cmd(Cmd::Sidebar {
                window: m,
                width,
                hidden: false,
            });
        }
        if let Some(w) = self.windows.get_mut(&id) {
            w.rename = Some((tab, title));
            w.menu = None;
        }
        Task::batch([
            operation::focus("rename-input"),
            operation::select_all("rename-input"),
        ])
    }

    fn close_tab(&mut self, id: window::Id, tab: TabId) -> Task<Message> {
        let mut panes = Vec::new();
        if let Some(t) = self.ws.tab(tab) {
            t.all_panes(&mut panes);
        }
        let (r, rem) = self.busy_panes(&panes);
        if self.config.terminal.confirm_close_when_busy && r + rem > 0 {
            let c = Confirm {
                title: self
                    .i18n
                    .ta("confirm-close-tab-title", &[("title", self.tab_title(tab))]),
                body: self.busy_text(r, rem),
                yes: self.i18n.t("button-close"),
                action: ConfirmAction::CloseTab(tab),
                alt: None,
            };
            if let Some(w) = self.windows.get_mut(&id) {
                w.overlay = Some(Overlay::Confirm(c));
            }
            return Task::none();
        }
        self.cmd(Cmd::CloseTab {
            tab,
            with_children: false,
        });
        Task::none()
    }

    fn close_pane(&mut self, id: window::Id, pane: PaneId) -> Task<Message> {
        let (r, rem) = self.busy_panes(&[pane]);
        if self.config.terminal.confirm_close_when_busy && r + rem > 0 {
            let c = Confirm {
                title: self.i18n.t("confirm-close-pane-title"),
                body: self.busy_text(r, rem),
                yes: self.i18n.t("button-close"),
                action: ConfirmAction::ClosePane(pane),
                alt: None,
            };
            if let Some(w) = self.windows.get_mut(&id) {
                w.overlay = Some(Overlay::Confirm(c));
            }
            return Task::none();
        }
        self.cmd(Cmd::ClosePane { pane });
        Task::none()
    }

    fn request_close_window(&mut self, id: window::Id) -> Task<Message> {
        let Some(model) = self.model_of(id) else {
            return window::close(id);
        };
        // Closing the last window quits, and its sessions end with the app.
        if self.ws.windows.len() <= 1 {
            return self.perform(id, Action::Quit);
        }
        match self.config.session.close_window {
            CloseWindow::Detach => {
                self.cmd(Cmd::CloseWindow {
                    window: model,
                    mode: CloseWindowMode::Detach,
                });
            }
            CloseWindow::CloseTabs => {
                let panes = self.ws.window_panes(model);
                let (r, rem) = self.busy_panes(&panes);
                if self.config.terminal.confirm_close_when_busy && r + rem > 0 {
                    let c = Confirm {
                        title: self.i18n.t("confirm-close-window-title"),
                        body: self.busy_text(r, rem),
                        yes: self.i18n.t("button-close"),
                        action: ConfirmAction::CloseWindow(model, CloseWindowMode::CloseTabs),
                        alt: None,
                    };
                    if let Some(w) = self.windows.get_mut(&id) {
                        w.overlay = Some(Overlay::Confirm(c));
                    }
                } else {
                    self.cmd(Cmd::CloseWindow {
                        window: model,
                        mode: CloseWindowMode::CloseTabs,
                    });
                }
            }
            CloseWindow::Ask => {
                let c = Confirm {
                    title: self.i18n.t("confirm-close-window-title"),
                    body: self.i18n.t("confirm-close-window-ask"),
                    yes: self.i18n.t("button-keep-sessions"),
                    action: ConfirmAction::CloseWindow(model, CloseWindowMode::Detach),
                    alt: Some((
                        self.i18n.t("button-end-sessions"),
                        ConfirmAction::CloseWindow(model, CloseWindowMode::CloseTabs),
                    )),
                };
                if let Some(w) = self.windows.get_mut(&id) {
                    w.overlay = Some(Overlay::Confirm(c));
                }
            }
        }
        Task::none()
    }

    fn confirmed(&mut self, a: ConfirmAction) -> Task<Message> {
        match a {
            ConfirmAction::ClosePane(p) => self.cmd(Cmd::ClosePane { pane: p }),
            ConfirmAction::CloseTab(t) => self.cmd(Cmd::CloseTab {
                tab: t,
                with_children: false,
            }),
            ConfirmAction::CloseWindow(w, mode) => self.cmd(Cmd::CloseWindow { window: w, mode }),
            ConfirmAction::QuitAll => {
                self.exiting = true;
                self.send(ClientMsg::Shutdown);
                // The server answers with ShuttingDown; exit anyway after a grace period.
                return Task::perform(async { tokio_sleep(Duration::from_secs(6)).await }, |_| {
                    Message::Disconnected("shutdown".into())
                });
            }
            ConfirmAction::Paste(p, text) => self.send(ClientMsg::Paste { pane: p, text }),
        }
        Task::none()
    }

    fn menu_pick(&mut self, id: window::Id, tab: TabId, item: MenuItem) -> Task<Message> {
        let Some(model) = self.model_of(id) else {
            return Task::none();
        };
        match item {
            MenuItem::NewTab => {
                self.cmd(Cmd::SelectTab { tab });
                self.cmd(Cmd::NewTab {
                    window: model,
                    nested: false,
                    profile: None,
                    cwd: None,
                    command: None,
                });
            }
            MenuItem::NewNested => {
                self.cmd(Cmd::SelectTab { tab });
                self.cmd(Cmd::NewTab {
                    window: model,
                    nested: true,
                    profile: None,
                    cwd: None,
                    command: None,
                });
            }
            MenuItem::Rename => return self.start_rename(id, tab),
            MenuItem::Duplicate => self.cmd(Cmd::DuplicateTab { tab }),
            MenuItem::Close => return self.close_tab(id, tab),
            MenuItem::CloseOthers => {
                let own = self.ws.group_of(tab);
                let others: Vec<TabId> = self
                    .ws
                    .window(model)
                    .map(|w| {
                        w.tabs
                            .iter()
                            .map(|g| g.id)
                            .filter(|g| Some(*g) != own)
                            .collect()
                    })
                    .unwrap_or_default();
                for g in others {
                    self.cmd(Cmd::CloseTab {
                        tab: g,
                        with_children: true,
                    });
                }
            }
            MenuItem::OpenFolder => {
                let term = if self.ws.is_group(tab) {
                    self.ws
                        .tab(tab)
                        .and_then(|g| g.active_child.or(g.children.first().map(|c| c.id)))
                } else {
                    Some(tab)
                };
                if let Some(p) = term.and_then(|t| self.ws.tab(t)).map(|t| t.active_pane) {
                    self.open_folder(p, false);
                }
            }
            MenuItem::ToNewWindow => self.cmd(Cmd::MoveTabToWindow { tab, window: None }),
            MenuItem::Outdent => self.cmd(Cmd::OutdentTab { tab }),
            MenuItem::ToWindow => self.open_move_to_window(id, tab),
            MenuItem::Collapse => self.cmd(Cmd::ToggleCollapse { tab }),
            MenuItem::GroupMode(mode) => self.cmd(Cmd::SetGroupMode { tab, mode }),
        }
        Task::none()
    }

    fn open_folder(&mut self, pane: PaneId, reveal: bool) {
        let Some(info) = self.infos.get(&pane) else {
            return;
        };
        if info.remote_host.is_some() {
            self.toast(self.i18n.t("notice-remote-folder"));
            return;
        }
        let Some(cwd) = info.cwd.clone() else { return };
        let r = if reveal {
            platform::reveal(&PathBuf::from(&cwd))
        } else {
            platform::open_folder(
                &PathBuf::from(&cwd),
                &self.config.integration.file_manager_command,
            )
        };
        if let Err(e) = r {
            self.toast(self.i18n.ta("error-open", &[("error", e.to_string())]));
        }
    }

    // ----- pickers -----

    fn open_picker(&mut self, id: window::Id, picker: Picker) -> Task<Message> {
        if let Some(w) = self.windows.get_mut(&id) {
            w.overlay = Some(Overlay::Picker(picker));
            w.menu = None;
        }
        operation::focus("picker-input")
    }

    fn open_move_to_window(&mut self, id: window::Id, tab: TabId) {
        let current = self.ws.window_of_tab(tab);
        let mut items = vec![PickItem {
            label: self.i18n.t("picker-new-window"),
            detail: String::new(),
            value: PickValue::Window(None),
        }];
        for (n, w) in self.ws.windows.iter().enumerate() {
            if Some(w.id) == current {
                continue;
            }
            let title = w.active_tab.map(|t| self.tab_title(t)).unwrap_or_default();
            items.push(PickItem {
                label: self
                    .i18n
                    .ta("picker-window-n", &[("n", (n + 1).to_string())]),
                detail: title,
                value: PickValue::Window(Some(w.id)),
            });
        }
        let p = Picker {
            kind: PickerKind::MoveToWindow(tab),
            title: self.i18n.t("picker-move-to-window"),
            query: String::new(),
            items,
            selected: 0,
        };
        let _ = self.open_picker(id, p);
    }

    fn picker_choose(&mut self, id: window::Id, idx: usize) -> Task<Message> {
        self.picker_choose_exec(id, idx, false)
    }

    fn picker_choose_exec(&mut self, id: window::Id, idx: usize, exec: bool) -> Task<Message> {
        let Some(w) = self.windows.get_mut(&id) else {
            return Task::none();
        };
        let Some(Overlay::Picker(p)) = w.overlay.take() else {
            return Task::none();
        };
        let Some(item) = p.filtered().get(idx).map(|i| (*i).clone()) else {
            return Task::none();
        };
        let model = w.model;
        match (p.kind, item.value) {
            (_, PickValue::Action(a)) => return self.perform(id, a),
            (_, PickValue::Profile(name)) => self.cmd(Cmd::NewTab {
                window: model,
                nested: false,
                profile: Some(name),
                cwd: None,
                command: None,
            }),
            (PickerKind::History(pane), PickValue::Command(c)) => {
                self.send(ClientMsg::RunCommand {
                    pane,
                    command: c,
                    execute: exec,
                })
            }
            (_, PickValue::Command(_)) => {}
            (_, PickValue::Tab(t)) => self.cmd(Cmd::AttachDetached {
                tab: t,
                window: model,
            }),
            (PickerKind::MoveToWindow(tab), PickValue::Window(target)) => {
                self.cmd(Cmd::MoveTabToWindow {
                    tab,
                    window: target,
                })
            }
            (_, PickValue::Window(_)) => {}
        }
        Task::none()
    }

    fn command_palette(&self) -> Picker {
        let items = Action::all()
            .into_iter()
            .map(|a| PickItem {
                label: self.action_label(a),
                detail: self.keymap.display(a),
                value: PickValue::Action(a),
            })
            .collect();
        Picker {
            kind: PickerKind::Commands,
            title: self.i18n.t("picker-commands"),
            query: String::new(),
            items,
            selected: 0,
        }
    }

    pub fn action_label(&self, a: Action) -> String {
        self.i18n
            .t(&format!("action-{}", a.name().replace('_', "-")))
    }

    // ----- actions -----

    pub fn perform(&mut self, id: window::Id, action: Action) -> Task<Message> {
        let Some(model) = self.model_of(id) else {
            return Task::none();
        };
        let tab = self.active_tab(id);
        let pane = self.active_pane(id);
        if let Some(w) = self.windows.get_mut(&id) {
            w.menu = None;
        }
        match action {
            Action::NewTab => self.cmd(Cmd::NewTab {
                window: model,
                nested: false,
                profile: None,
                cwd: None,
                command: None,
            }),
            Action::NewNestedTab => self.cmd(Cmd::NewTab {
                window: model,
                nested: true,
                profile: None,
                cwd: None,
                command: None,
            }),
            Action::NewTabFromProfile => {
                let items = self
                    .config
                    .profiles
                    .iter()
                    .map(|p| PickItem {
                        label: p.name.clone(),
                        detail: if p.command.is_empty() {
                            at_core::pane::tilde_path(&p.cwd)
                        } else {
                            p.command.clone()
                        },
                        value: PickValue::Profile(p.name.clone()),
                    })
                    .collect();
                return self.open_picker(
                    id,
                    Picker {
                        kind: PickerKind::Profiles,
                        title: self.i18n.t("picker-profiles"),
                        query: String::new(),
                        items,
                        selected: 0,
                    },
                );
            }
            Action::NewWindow => {
                let cwd = pane
                    .and_then(|p| self.infos.get(&p))
                    .and_then(|i| i.cwd.clone());
                let geom = self.ws.window(model).and_then(|w| w.geom.clone()).map(|g| {
                    at_core::model::WindowGeom {
                        x: g.x + 30.0,
                        y: g.y + 30.0,
                        ..g
                    }
                });
                self.cmd(Cmd::NewWindow {
                    cwd,
                    profile: None,
                    command: None,
                    geom,
                });
            }
            Action::TabToNewWindow => {
                if let Some(t) = tab {
                    self.cmd(Cmd::MoveTabToWindow {
                        tab: t,
                        window: None,
                    });
                }
            }
            Action::NextWindow => {
                let ids: Vec<window::Id> = self.windows.keys().copied().collect();
                if let Some(pos) = ids.iter().position(|w| *w == id)
                    && let Some(next) = ids.get((pos + 1) % ids.len())
                {
                    return window::gain_focus(*next);
                }
            }
            Action::CloseWindow => return self.request_close_window(id),
            Action::Quit => {
                let panes = self.ws.all_panes();
                let (r, rem) = self.busy_panes(&panes);
                if self.config.session.confirm_quit_when_busy && r + rem > 0 {
                    let c = Confirm {
                        title: self.i18n.t("confirm-quit-title"),
                        body: self.busy_text(r, rem),
                        yes: self.i18n.t("button-end-sessions"),
                        action: ConfirmAction::QuitAll,
                        alt: None,
                    };
                    if let Some(w) = self.windows.get_mut(&id) {
                        w.overlay = Some(Overlay::Confirm(c));
                    }
                    return Task::none();
                }
                return self.confirmed(ConfirmAction::QuitAll);
            }
            Action::ReopenClosedTab => self.cmd(Cmd::ReopenClosedTab { window: model }),
            Action::ClosePane => {
                if let Some(p) = pane {
                    return self.close_pane(id, p);
                }
            }
            Action::CloseTab => {
                if let Some(t) = tab {
                    return self.close_tab(id, t);
                }
            }
            Action::RenameTab => {
                if let Some(t) = tab {
                    return self.start_rename(id, t);
                }
            }
            Action::DuplicateTab => {
                if let Some(t) = tab {
                    self.cmd(Cmd::DuplicateTab { tab: t });
                }
            }
            Action::SelectTab(n) => {
                if let Some(v) = self
                    .ws
                    .visible_tabs(model, self.expand_single())
                    .get(n as usize - 1)
                {
                    self.cmd(Cmd::SelectTab { tab: v.id });
                }
            }
            Action::NextTab | Action::PrevTab => {
                let order = self.ws.terminal_tabs(model);
                if let (Some(t), false) = (tab, order.is_empty()) {
                    let i = order.iter().position(|x| *x == t).unwrap_or(0);
                    let j = if action == Action::NextTab {
                        (i + 1) % order.len()
                    } else {
                        (i + order.len() - 1) % order.len()
                    };
                    self.cmd(Cmd::SelectTab { tab: order[j] });
                }
            }
            Action::MoveTabUp | Action::MoveTabDown => {
                if let Some(t) = tab {
                    self.cmd(Cmd::MoveTab {
                        tab: t,
                        down: action == Action::MoveTabDown,
                    });
                }
            }
            Action::IndentTab => {
                if let Some(t) = tab {
                    self.cmd(Cmd::IndentTab { tab: t });
                }
            }
            Action::OutdentTab => {
                if let Some(t) = tab {
                    self.cmd(Cmd::OutdentTab { tab: t });
                }
            }
            Action::ToggleCollapse => {
                if let Some(t) = tab
                    && self.group_len(t) > 1
                {
                    self.cmd(Cmd::ToggleCollapse { tab: t });
                }
            }
            Action::ToggleGroupView => {
                if let Some(t) = tab {
                    if self.group_len(t) > 1 {
                        self.cmd(Cmd::ToggleGroupView { tab: t });
                    } else {
                        self.toast(self.i18n.t("notice-group-needs-children"));
                    }
                }
            }
            Action::ToggleGroupTiles => {
                if let Some(t) = tab {
                    if self.group_len(t) > 1 {
                        let mode = if self.ws.group_mode(t) == GroupMode::Tiles {
                            GroupMode::Stack
                        } else {
                            GroupMode::Tiles
                        };
                        self.cmd(Cmd::SetGroupMode { tab: t, mode });
                    } else {
                        self.toast(self.i18n.t("notice-group-needs-children"));
                    }
                }
            }
            Action::ToggleSidebar => {
                if let Some(w) = self.ws.window(model) {
                    self.cmd(Cmd::Sidebar {
                        window: model,
                        width: w.sidebar_width,
                        hidden: !w.sidebar_hidden,
                    });
                }
            }
            Action::NewPane => {
                if let Some(t) = tab {
                    if self.ws.tab(t).map(|n| n.can_add_pane()).unwrap_or(false) {
                        self.cmd(Cmd::NewPane { tab: t });
                    } else {
                        self.toast(self.i18n.t("notice-max-panes"));
                    }
                }
            }
            Action::ToggleLayout => {
                if let Some(t) = tab {
                    if self.ws.tab(t).map(|n| n.panes.len() == 4).unwrap_or(false) {
                        self.cmd(Cmd::ToggleLayout { tab: t });
                    } else {
                        self.toast(self.i18n.t("notice-grid-needs-four"));
                    }
                }
            }
            Action::FocusUp | Action::FocusDown | Action::FocusLeft | Action::FocusRight => {
                let dir = match action {
                    Action::FocusUp => Direction::Up,
                    Action::FocusDown => Direction::Down,
                    Action::FocusLeft => Direction::Left,
                    _ => Direction::Right,
                };
                if let Some(t) = tab {
                    self.cmd(Cmd::FocusDirection { tab: t, dir });
                }
            }
            Action::FocusPane(n) => {
                if let Some(p) = tab
                    .and_then(|t| self.ws.tab(t))
                    .and_then(|t| t.panes.get(n as usize - 1).copied())
                {
                    self.cmd(Cmd::FocusPane { pane: p });
                }
            }
            Action::ResizeUp | Action::ResizeDown | Action::ResizeLeft | Action::ResizeRight => {
                let dir = match action {
                    Action::ResizeUp => Direction::Up,
                    Action::ResizeDown => Direction::Down,
                    Action::ResizeLeft => Direction::Left,
                    _ => Direction::Right,
                };
                if let Some(t) = tab {
                    self.cmd(Cmd::ResizePane { tab: t, dir });
                }
            }
            Action::EqualizePanes => {
                if let Some(t) = tab {
                    self.cmd(Cmd::EqualizePanes { tab: t });
                }
            }
            Action::ZoomPane => {
                if let Some(t) = tab {
                    self.cmd(Cmd::ZoomPane { tab: t });
                }
            }
            Action::MovePaneUp | Action::MovePaneDown => {
                if let Some(t) = tab {
                    self.cmd(Cmd::MovePane {
                        tab: t,
                        down: action == Action::MovePaneDown,
                    });
                }
            }
            Action::RestartShell => {
                if let Some(p) = pane {
                    self.cmd(Cmd::RestartShell { pane: p });
                }
            }
            Action::OpenFolder | Action::RevealFolder => {
                if let Some(p) = pane {
                    self.open_folder(p, action == Action::RevealFolder);
                }
            }
            Action::OpenInEditor => {
                if let Some(info) = pane.and_then(|p| self.infos.get(&p)) {
                    if info.remote_host.is_some() {
                        self.toast(self.i18n.t("notice-remote-folder"));
                    } else if let Some(cwd) = info.cwd.clone()
                        && let Err(e) = platform::run_template(
                            &self.config.integration.editor_command,
                            &PathBuf::from(cwd),
                        )
                    {
                        self.toast(self.i18n.ta("error-open", &[("error", e.to_string())]));
                    }
                }
            }
            Action::CopyCwd => {
                if let Some(cwd) = pane
                    .and_then(|p| self.infos.get(&p))
                    .and_then(|i| i.cwd.clone())
                {
                    self.toast(self.i18n.ta(
                        "notice-copied",
                        &[("text", at_core::pane::tilde_path(&cwd))],
                    ));
                    return iced::clipboard::write(cwd);
                }
            }
            Action::ShowHistory => {
                if let Some(p) = pane {
                    self.history_for = Some(id);
                    self.send(ClientMsg::GetHistory { pane: p });
                }
            }
            Action::Find => {
                if let Some(p) = pane {
                    self.search.entry(p).or_default();
                    let sid = format!("search-{p}");
                    return Task::batch([
                        operation::focus(sid.clone()),
                        operation::select_all(sid),
                    ]);
                }
            }
            Action::Copy => {
                if let Some(p) = pane {
                    self.send(ClientMsg::Copy { pane: p });
                }
            }
            Action::Paste => {
                if let Some(p) = pane {
                    return iced::clipboard::read().map(move |t| Message::Pasted(id, p, t));
                }
            }
            Action::SelectAll => {
                if let Some(p) = pane {
                    self.send(ClientMsg::SelectAll { pane: p });
                }
            }
            Action::ClearScreen => {
                if let Some(p) = pane {
                    self.send(ClientMsg::ClearScreen { pane: p });
                }
            }
            Action::FontBigger | Action::FontSmaller | Action::FontReset => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.font_delta = match action {
                        Action::FontBigger => (w.font_delta + 1.0).min(40.0),
                        Action::FontSmaller => (w.font_delta - 1.0).max(-6.0),
                        _ => 0.0,
                    };
                }
                for p in self.ws.window_panes(model) {
                    if let Some(g) = self.grids.get(&p) {
                        g.cache.clear();
                    }
                }
            }
            Action::ScrollPageUp
            | Action::ScrollPageDown
            | Action::ScrollTop
            | Action::ScrollBottom => {
                if let Some(p) = pane {
                    let to = match action {
                        Action::ScrollPageUp => ScrollTo::PageUp,
                        Action::ScrollPageDown => ScrollTo::PageDown,
                        Action::ScrollTop => ScrollTo::Top,
                        _ => ScrollTo::Bottom,
                    };
                    self.send(ClientMsg::Scroll { pane: p, to });
                }
            }
            Action::CommandPalette => {
                let p = self.command_palette();
                return self.open_picker(id, p);
            }
            Action::Settings => {
                let st = SettingsState::new(&self.config);
                if let Some(w) = self.windows.get_mut(&id) {
                    w.overlay = Some(Overlay::Settings(Box::new(st)));
                }
            }
            Action::DetachedTabs => {
                let items: Vec<PickItem> = self
                    .ws
                    .detached
                    .iter()
                    .map(|t| PickItem {
                        label: t.custom_title.clone().unwrap_or_else(|| {
                            self.infos
                                .get(&t.active_pane)
                                .map(|i| i.auto_title())
                                .unwrap_or_default()
                        }),
                        detail: self
                            .infos
                            .get(&t.active_pane)
                            .and_then(|i| i.cwd.clone())
                            .map(|c| at_core::pane::tilde_path(&c))
                            .unwrap_or_default(),
                        value: PickValue::Tab(t.id),
                    })
                    .collect();
                if items.is_empty() {
                    self.toast(self.i18n.t("notice-no-detached"));
                    return Task::none();
                }
                return self.open_picker(
                    id,
                    Picker {
                        kind: PickerKind::Detached,
                        title: self.i18n.t("picker-detached"),
                        query: String::new(),
                        items,
                        selected: 0,
                    },
                );
            }
            Action::OpenConfigFolder => {
                let dir = paths::config_dir();
                let _ = std::fs::create_dir_all(&dir);
                if !paths::config_file().exists() {
                    let _ = Config::default().save(&paths::config_file());
                }
                let _ = platform::open_folder(&dir, "");
            }
            Action::ShowLogs => {
                let _ = platform::open_folder(&paths::log_dir(), "");
            }
        }
        Task::none()
    }

    // ----- settings -----

    fn settings_msg(&mut self, id: window::Id, m: settings::Msg) -> Task<Message> {
        let Some(Overlay::Settings(st)) =
            self.windows.get_mut(&id).and_then(|w| w.overlay.as_mut())
        else {
            return Task::none();
        };
        match st.update(m) {
            settings::Outcome::None => {}
            settings::Outcome::Cancel => {
                if let Some(w) = self.windows.get_mut(&id) {
                    w.overlay = None;
                }
            }
            settings::Outcome::Save(config) => match config.save(&paths::config_file()) {
                Ok(()) => {
                    self.config_mtime = std::fs::metadata(paths::config_file())
                        .and_then(|m| m.modified())
                        .ok();
                    self.reload_config();
                    self.cmd(Cmd::ReloadConfig);
                    if let Some(w) = self.windows.get_mut(&id) {
                        w.overlay = None;
                    }
                    self.toast(self.i18n.t("notice-settings-saved"));
                }
                Err(e) => self.toast(self.i18n.ta("error-config", &[("error", e.to_string())])),
            },
        }
        Task::none()
    }
}

async fn tokio_sleep(d: Duration) {
    tokio::time::sleep(d).await;
}
