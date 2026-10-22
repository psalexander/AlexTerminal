//! Server core: one thread that owns the workspace and all panes.

use at_core::config::{CloseChildren, Config, Reconnect};
use at_core::history::{self, HistoryEntry, HistoryWriter};
use at_core::model::{Layout, TabNode, WindowGeom, WindowNode, Workspace};
use at_core::pane::{PaneInfo, PaneState};
use at_core::snapshot::Session;
use at_core::theme::Theme;
use at_core::{PaneId, WindowId, paths};
use at_proto::{
    ClientKind, ClientMsg, CloseWindowMode, Cmd, Frame, Notice, PROTO_VERSION, Row, ServerMsg,
};
use at_term::osc::OscEvent;
use at_term::pane::Palette;
use at_term::shell::ShellLaunch;
use at_term::{Pane, PaneEvent, SpawnOptions, procinfo};
use crossbeam_channel::{Receiver, Sender};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

pub type ClientId = u64;

/// Protocol version this server speaks. `ALEXTERMINAL_TEST_PROTO_VERSION` overrides
/// it so tests can play an outdated server.
pub fn server_proto_version() -> u32 {
    std::env::var("ALEXTERMINAL_TEST_PROTO_VERSION")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(PROTO_VERSION)
}

pub enum ClientEvent {
    Connected(Sender<ServerMsg>),
    Msg(ClientMsg),
    Disconnected,
}

#[derive(Clone, Debug)]
pub struct ServerOptions {
    pub restore: bool,
    pub restore_from: Option<PathBuf>,
    /// Exit when the last client disconnects and no panes remain (in-process mode).
    pub exit_when_empty: bool,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self {
            restore: true,
            restore_from: None,
            exit_when_empty: false,
        }
    }
}

struct PaneRt {
    pane: Arc<Pane>,
    info: PaneInfo,
    hist: HistoryWriter,
    fg_since: Option<Instant>,
    osc_running: bool,
    cmd_started: Option<Instant>,
    /// Profile command waiting for the first prompt.
    pending_cmd: Option<String>,
    pending_deadline: Option<Instant>,
    reconnect_cmd: Option<String>,
    last_osc_cwd: Option<Instant>,
    remote_osc: Option<String>,
}

#[derive(Default)]
struct Sent {
    rows: Vec<u64>,
    cols: u16,
    cursor: Option<at_proto::Cursor>,
    offset: u32,
    history: u32,
    mode: u32,
    search: Option<at_proto::SearchInfo>,
}

struct ClientRt {
    tx: Sender<ServerMsg>,
    kind: ClientKind,
    hello: bool,
    visible: HashSet<PaneId>,
    sent: HashMap<PaneId, Sent>,
}

struct ClosedTab {
    window: WindowId,
    /// Group and position a terminal tab was closed from (None for whole groups).
    slot: Option<(at_core::TabId, usize)>,
    node: TabNode,
    infos: Vec<PaneInfo>,
    at: Instant,
}

pub struct Server {
    config: Config,
    config_mtime: Option<SystemTime>,
    config_checked: Instant,
    theme: Theme,
    lang: &'static str,
    ws: Workspace,
    panes: HashMap<PaneId, PaneRt>,
    clients: HashMap<ClientId, ClientRt>,
    closed: Vec<ClosedTab>,
    pane_tx: Sender<PaneEvent>,
    pane_rx: Receiver<PaneEvent>,
    client_rx: Receiver<(ClientId, ClientEvent)>,
    notices: Vec<Notice>,
    structure_dirty_at: Option<Instant>,
    last_save: Instant,
    last_scrollback_save: Instant,
    last_procinfo: Instant,
    last_flush: Instant,
    opts: ServerOptions,
    shutdown: bool,
    terminators: Vec<std::thread::JoinHandle<()>>,
}

fn row_hash(r: &Row) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    r.hash(&mut h);
    h.finish()
}

fn system_dark() -> bool {
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

impl Server {
    pub fn new(client_rx: Receiver<(ClientId, ClientEvent)>, opts: ServerOptions) -> Server {
        let (config, err) = Config::load_default_path();
        let mut notices = Vec::new();
        if let Some(e) = err {
            notices.push(Notice::ConfigError(e));
        }
        let (theme, terr) = Theme::resolve(&config.appearance.theme, system_dark());
        if let Some(e) = terr {
            notices.push(Notice::ConfigError(e));
        }
        let locale = sys_locale::get_locale();
        let lang = config.ui_language(locale.as_deref());
        let (pane_tx, pane_rx) = crossbeam_channel::unbounded();
        let _ = paths::ensure_private_dir(&paths::data_dir());
        let _ = paths::ensure_private_dir(&paths::history_dir());
        let _ = paths::ensure_private_dir(&paths::scrollback_dir());
        if let Err(e) = at_term::shell::install(&paths::shell_integration_dir()) {
            tracing::warn!("shell integration install failed: {e}");
        }
        let now = Instant::now();
        let mut s = Server {
            config_mtime: std::fs::metadata(paths::config_file())
                .and_then(|m| m.modified())
                .ok(),
            config,
            config_checked: now,
            theme,
            lang,
            ws: Workspace::default(),
            panes: HashMap::new(),
            clients: HashMap::new(),
            closed: Vec::new(),
            pane_tx,
            pane_rx,
            client_rx,
            notices,
            structure_dirty_at: None,
            last_save: now,
            last_scrollback_save: now,
            last_procinfo: now,
            last_flush: now,
            opts,
            shutdown: false,
            terminators: Vec::new(),
        };
        if s.opts.restore && s.config.session.restore_session {
            s.restore();
        }
        let live = s.ws.all_panes();
        let days = s.config.session.history_retention_days;
        std::thread::spawn(move || {
            history::cleanup(&paths::history_dir(), days, &live);
            history::cleanup(&paths::scrollback_dir(), days, &live);
        });
        s
    }

    // ----- restore -----

    fn restore(&mut self) {
        let path = self
            .opts
            .restore_from
            .clone()
            .unwrap_or_else(paths::session_file);
        if !path.exists() && !path.with_extension("json.bak").exists() {
            return;
        }
        let (session, from_bak) = match Session::load_with_backup(&path) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("session unreadable: {e}");
                self.notices
                    .push(Notice::SessionUnreadable(path.display().to_string()));
                return;
            }
        };
        if from_bak {
            self.notices.push(Notice::SessionRestoredFromBackup(
                path.display().to_string(),
            ));
        }
        let (ws, panes) = session.restore();
        let stamp = at_core::time::format_local(session.saved_at_ms.max(1), self.lang);
        for snap in panes {
            let profile = ws
                .tab_of_pane(snap.id)
                .and_then(|t| ws.tab(t))
                .map(|t| t.profile.clone())
                .unwrap_or_else(|| "default".into());
            let mut preload = Vec::new();
            if self.config.session.restore_scrollback {
                let f = paths::scrollback_dir().join(format!("{}.txt", snap.id));
                if let Ok(mut bytes) = std::fs::read(&f) {
                    // Keep only the configured number of lines.
                    let max = self.config.session.restore_scrollback_lines;
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    let lines: Vec<&str> = text.split("\r\n").collect();
                    if lines.len() > max + 1 {
                        bytes = lines[lines.len() - max - 1..].join("\r\n").into_bytes();
                    }
                    preload.extend_from_slice(&bytes);
                    let sep = if self.lang == "ru" {
                        format!("── восстановлено {stamp} ──")
                    } else {
                        format!("── restored {stamp} ──")
                    };
                    preload.extend_from_slice(format!("\x1b[0m\x1b[2m{sep}\x1b[0m\r\n").as_bytes());
                }
            }
            let mut cwd = snap.cwd.clone().map(PathBuf::from);
            if let Some(c) = &cwd
                && !c.is_dir()
            {
                let msg = if self.lang == "ru" {
                    format!(
                        "[AlexTerminal] каталог {} недоступен, открыт ~",
                        c.display()
                    )
                } else {
                    format!(
                        "[AlexTerminal] directory {} is unavailable, opened ~",
                        c.display()
                    )
                };
                preload.extend_from_slice(format!("\x1b[2m{msg}\x1b[0m\r\n").as_bytes());
                self.notices.push(Notice::CwdMissing {
                    pane: snap.id,
                    cwd: c.display().to_string(),
                });
                cwd = None;
            }
            match self.spawn_pane(
                snap.id,
                &profile,
                cwd,
                (!preload.is_empty()).then_some(preload),
                false,
            ) {
                Ok(()) => {
                    if let Some(r) = &snap.remote {
                        let cmd = r
                            .command
                            .clone()
                            .or_else(|| {
                                let p = self.config.profile(&profile);
                                (!p.command.is_empty()).then(|| p.command.clone())
                            })
                            .unwrap_or_else(|| format!("ssh {}", r.host));
                        let rt = self.panes.get_mut(&snap.id).unwrap();
                        match self.config.session.reconnect_remote {
                            Reconnect::Ask => {
                                rt.info.reconnect_prompt = Some(r.host.clone());
                                rt.reconnect_cmd = Some(cmd);
                            }
                            Reconnect::Auto => {
                                rt.pending_cmd = Some(cmd);
                                rt.pending_deadline =
                                    Some(Instant::now() + Duration::from_millis(1500));
                            }
                            Reconnect::Never => {}
                        }
                    }
                    if let Some(rt) = self.panes.get_mut(&snap.id) {
                        rt.info.font_scale = snap.font_scale;
                    }
                }
                Err(e) => tracing::error!("restore pane {}: {e}", snap.id),
            }
        }
        self.ws = ws;
        // Drop tree nodes whose pane failed to spawn.
        let missing: Vec<PaneId> = self
            .ws
            .all_panes()
            .into_iter()
            .filter(|p| !self.panes.contains_key(p))
            .collect();
        for p in missing {
            self.remove_pane_from_tree(p);
        }
        tracing::info!(
            "restored {} windows, {} panes",
            self.ws.windows.len(),
            self.panes.len()
        );
    }

    // ----- pane lifecycle -----

    fn resolve_cwd(
        &self,
        profile: &str,
        explicit: Option<PathBuf>,
        from_pane: Option<PaneId>,
    ) -> PathBuf {
        let home = paths::home_dir();
        if let Some(p) = explicit.filter(|p| p.is_dir()) {
            return p;
        }
        let prof = self.config.profile(profile);
        let setting = if prof.cwd.is_empty() || (prof.cwd == "inherit" && prof.name == "default") {
            self.config.terminal.new_tab_cwd.clone()
        } else {
            prof.cwd.clone()
        };
        match setting.as_str() {
            "home" | "" => home,
            "inherit" => from_pane
                .and_then(|p| self.panes.get(&p))
                .and_then(|rt| rt.info.cwd.clone())
                .map(PathBuf::from)
                .filter(|p| p.is_dir())
                .unwrap_or(home),
            other => {
                let p = paths::expand_tilde(other);
                if p.is_dir() { p } else { home }
            }
        }
    }

    fn spawn_pane(
        &mut self,
        id: PaneId,
        profile: &str,
        cwd: Option<PathBuf>,
        preload: Option<Vec<u8>>,
        run_profile_cmd: bool,
    ) -> anyhow::Result<()> {
        let prof = self.config.profile(profile).clone();
        let cwd = cwd
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| self.resolve_cwd(profile, None, None));
        let shell = if !prof.shell.is_empty() {
            prof.shell.clone()
        } else if !self.config.terminal.shell.is_empty() {
            self.config.terminal.shell.clone()
        } else {
            at_term::shell::default_shell()
        };
        let args = if !prof.args.is_empty() {
            prof.args.clone()
        } else {
            self.config.terminal.shell_args.clone()
        };
        let mut env: Vec<(String, String)> = self
            .config
            .terminal
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        env.extend(prof.env.iter().map(|(k, v)| (k.clone(), v.clone())));
        let per_pane = self.config.session.per_pane_shell_history;
        let launch = ShellLaunch {
            shell,
            args,
            cwd: cwd.clone(),
            env,
            integration: self.config.shell_integration.mode,
            integration_dir: paths::shell_integration_dir(),
            histfile: per_pane.then(|| paths::history_dir().join(format!("{id}.shell_history"))),
            history_name: per_pane.then(|| format!("alexterm_{}", id.to_string().to_lowercase())),
            wrap_ssh: self.config.shell_integration.wrap_ssh,
            term: self.config.terminal.term.clone(),
            pane_id: id.to_string(),
        };
        let opts = SpawnOptions {
            id,
            cols: 100,
            rows: 30,
            cell_w: 8,
            cell_h: 17,
            launch,
            scrollback_lines: self.config.terminal.scrollback_lines,
            word_separators: self.config.terminal.word_separators.clone(),
            osc52: self.config.terminal.osc52,
            preload,
            palette: Palette::from_theme(&self.theme),
        };
        let pane = Pane::spawn(opts, self.pane_tx.clone())?;
        let mut info = PaneInfo::new(id, profile);
        info.cwd = Some(cwd.display().to_string());
        if let Some(old) = self.panes.get(&id) {
            info.font_scale = old.info.font_scale;
        }
        let mut rt = PaneRt {
            pane,
            info,
            hist: HistoryWriter::new(
                history::path_for(&paths::history_dir(), id),
                self.config.session.history_max_entries,
            ),
            fg_since: None,
            osc_running: false,
            cmd_started: None,
            pending_cmd: None,
            pending_deadline: None,
            reconnect_cmd: None,
            last_osc_cwd: None,
            remote_osc: None,
        };
        if run_profile_cmd && !prof.command.is_empty() {
            rt.pending_cmd = Some(prof.command.clone());
            rt.pending_deadline = Some(Instant::now() + Duration::from_millis(500));
            if prof.remote {
                rt.remote_osc = procinfo::remote_host(
                    &prof
                        .command
                        .split_whitespace()
                        .map(String::from)
                        .collect::<Vec<_>>(),
                )
                .or_else(|| (!prof.title.is_empty()).then(|| prof.title.clone()));
            }
        }
        let info = rt.info.clone();
        self.panes.insert(id, rt);
        self.broadcast(ServerMsg::Pane(info));
        Ok(())
    }

    fn terminate_panes(&mut self, ids: Vec<PaneId>) {
        let mut list = Vec::new();
        for id in ids {
            if let Some(mut rt) = self.panes.remove(&id) {
                rt.hist.close();
                list.push(rt.pane);
            }
            for c in self.clients.values_mut() {
                c.sent.remove(&id);
                c.visible.remove(&id);
            }
            self.broadcast(ServerMsg::PaneRemoved(id));
        }
        if list.is_empty() {
            return;
        }
        let h = std::thread::spawn(move || {
            let hs: Vec<_> = list
                .into_iter()
                .map(|p| {
                    std::thread::spawn(move || {
                        p.terminate(Duration::from_secs(2), Duration::from_secs(1))
                    })
                })
                .collect();
            for h in hs {
                let _ = h.join();
            }
        });
        self.terminators.retain(|h| !h.is_finished());
        self.terminators.push(h);
    }

    /// Removes a pane from its tab; empty tabs and windows are removed as well.
    fn remove_pane_from_tree(&mut self, pane: PaneId) {
        let Some(tab) = self.ws.tab_of_pane(pane) else {
            return;
        };
        let empty = {
            let t = self.ws.tab_mut(tab).unwrap();
            t.remove_pane(pane);
            t.panes.is_empty()
        };
        if empty {
            self.ws.remove_tab(tab, true);
            self.ws.windows.retain(|w| !w.tabs.is_empty());
        }
    }

    // ----- main loop -----

    pub fn run(mut self) {
        tracing::info!("server loop started");
        loop {
            let timeout = Duration::from_millis(8);
            crossbeam_channel::select! {
                recv(self.client_rx) -> m => match m {
                    Ok((cid, ev)) => self.on_client(cid, ev),
                    Err(_) => {
                        if self.opts.exit_when_empty { self.shutdown = true; }
                    }
                },
                recv(self.pane_rx) -> m => if let Ok(ev) = m { self.on_pane(ev) },
                default(timeout) => {}
            }
            // Drain whatever else queued up before flushing.
            for _ in 0..256 {
                match self.pane_rx.try_recv() {
                    Ok(ev) => self.on_pane(ev),
                    Err(_) => break,
                }
            }
            for _ in 0..256 {
                match self.client_rx.try_recv() {
                    Ok((cid, ev)) => self.on_client(cid, ev),
                    Err(_) => break,
                }
            }
            self.tick();
            if self.shutdown {
                break;
            }
        }
        self.finish();
    }

    fn finish(&mut self) {
        tracing::info!(
            "shutting down: saving session and ending {} panes",
            self.panes.len()
        );
        self.save(true);
        self.broadcast(ServerMsg::ShuttingDown);
        let ids: Vec<PaneId> = self.panes.keys().copied().collect();
        self.terminate_panes(ids);
        for h in self.terminators.drain(..) {
            let _ = h.join();
        }
        let _ = std::fs::remove_file(paths::socket_path());
        tracing::info!("bye");
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let frame_interval =
            Duration::from_millis((1000 / self.config.advanced.max_fps.max(10)) as u64);
        if now.duration_since(self.last_flush) >= frame_interval {
            self.last_flush = now;
            self.flush_frames();
        }
        if now.duration_since(self.last_procinfo) >= Duration::from_millis(500) {
            self.last_procinfo = now;
            self.poll_processes();
        }
        // Profile commands whose prompt never reported (no integration).
        let due: Vec<PaneId> = self
            .panes
            .iter()
            .filter(|(_, rt)| {
                rt.pending_cmd.is_some()
                    && rt.pending_deadline.map(|d| now >= d).unwrap_or(false)
                    && !rt.info.integration
            })
            .map(|(id, _)| *id)
            .collect();
        for id in due {
            self.send_pending_cmd(id);
        }
        if let Some(t) = self.structure_dirty_at
            && now.duration_since(t) >= Duration::from_secs(1)
        {
            self.save(false);
        }
        if now.duration_since(self.last_save)
            >= Duration::from_secs(self.config.session.autosave_interval_sec.max(5))
        {
            self.save(true);
        }
        if now.duration_since(self.config_checked) >= Duration::from_secs(1) {
            self.config_checked = now;
            let m = std::fs::metadata(paths::config_file())
                .and_then(|m| m.modified())
                .ok();
            if m != self.config_mtime {
                self.config_mtime = m;
                self.reload_config();
            }
        }
        if self.opts.exit_when_empty && self.panes.is_empty() && self.clients.is_empty() {
            self.shutdown = true;
        }
    }

    fn reload_config(&mut self) {
        let (config, err) = Config::load_default_path();
        if let Some(e) = err {
            self.broadcast(ServerMsg::Notice(Notice::ConfigError(e)));
            return;
        }
        let (theme, _) = Theme::resolve(&config.appearance.theme, system_dark());
        let locale = sys_locale::get_locale();
        self.lang = config.ui_language(locale.as_deref());
        self.config = config;
        self.theme = theme;
        let pal = Palette::from_theme(&self.theme);
        for rt in self.panes.values() {
            rt.pane.set_palette(pal.clone());
        }
        tracing::info!("config reloaded");
    }

    // ----- persistence -----

    fn infos(&self) -> HashMap<PaneId, PaneInfo> {
        self.panes
            .iter()
            .map(|(k, v)| (*k, v.info.clone()))
            .collect()
    }

    fn save(&mut self, with_scrollback: bool) {
        self.structure_dirty_at = None;
        self.last_save = Instant::now();
        let session = Session::capture(&self.ws, &self.infos(), at_core::time::unix_ms());
        if let Err(e) = session.save(&paths::session_file()) {
            tracing::error!("saving session: {e}");
        }
        if with_scrollback && self.config.session.restore_scrollback {
            self.last_scrollback_save = Instant::now();
            let max = self.config.session.restore_scrollback_lines;
            for (id, rt) in &self.panes {
                if let Some(text) = rt.pane.export_scrollback(max) {
                    let p = paths::scrollback_dir().join(format!("{id}.txt"));
                    if let Err(e) = paths::write_atomic(&p, text.as_bytes()) {
                        tracing::warn!("scrollback {id}: {e}");
                    }
                }
            }
        }
    }

    fn structure_changed(&mut self) {
        if self.structure_dirty_at.is_none() {
            self.structure_dirty_at = Some(Instant::now());
        }
        let ws = self.ws.clone();
        self.broadcast(ServerMsg::Workspace(ws));
    }

    // ----- clients -----

    fn send(&self, cid: ClientId, msg: ServerMsg) {
        if let Some(c) = self.clients.get(&cid) {
            let _ = c.tx.send(msg);
        }
    }

    fn broadcast(&self, msg: ServerMsg) {
        for c in self.clients.values().filter(|c| c.hello) {
            let _ = c.tx.send(msg.clone());
        }
    }

    fn broadcast_pane(&self, id: PaneId) {
        if let Some(rt) = self.panes.get(&id) {
            self.broadcast(ServerMsg::Pane(rt.info.clone()));
        }
    }

    fn gui_clients(&self) -> u32 {
        self.clients
            .values()
            .filter(|c| c.hello && c.kind == ClientKind::Gui)
            .count() as u32
    }

    fn on_client(&mut self, cid: ClientId, ev: ClientEvent) {
        match ev {
            ClientEvent::Connected(tx) => {
                self.clients.insert(
                    cid,
                    ClientRt {
                        tx,
                        kind: ClientKind::Cli,
                        hello: false,
                        visible: HashSet::new(),
                        sent: HashMap::new(),
                    },
                );
            }
            ClientEvent::Disconnected => {
                if let Some(c) = self.clients.remove(&cid)
                    && c.kind == ClientKind::Gui
                    && self.gui_clients() == 0
                {
                    // Last window gone: write a full snapshot now.
                    self.save(true);
                }
            }
            ClientEvent::Msg(m) => self.on_msg(cid, m),
        }
    }

    fn on_msg(&mut self, cid: ClientId, m: ClientMsg) {
        match m {
            ClientMsg::Hello { version, kind, pid } => {
                let ours = server_proto_version();
                if version != ours {
                    tracing::warn!("client {cid} speaks protocol {version}, server {ours}");
                    self.send(
                        cid,
                        ServerMsg::Error(format!(
                            "protocol mismatch: server {ours}, client {version}"
                        )),
                    );
                    return;
                }
                let gui_before = self.gui_clients();
                if let Some(c) = self.clients.get_mut(&cid) {
                    c.kind = kind;
                    c.hello = true;
                }
                tracing::info!("client {cid} hello: {kind:?} pid {pid}");
                let welcome = ServerMsg::Welcome {
                    version: PROTO_VERSION,
                    workspace: self.ws.clone(),
                    panes: self.panes.values().map(|r| r.info.clone()).collect(),
                    gui_clients: gui_before,
                };
                self.send(cid, welcome);
                for n in std::mem::take(&mut self.notices) {
                    self.send(cid, ServerMsg::Notice(n));
                }
            }
            ClientMsg::Cmd(cmd) => self.on_cmd(cid, cmd),
            ClientMsg::Visible { panes } => {
                if let Some(c) = self.clients.get_mut(&cid) {
                    let set: HashSet<PaneId> = panes.into_iter().collect();
                    c.sent.retain(|k, _| set.contains(k));
                    c.visible = set;
                }
                for rt in self.panes.values() {
                    rt.pane.mark_dirty();
                }
            }
            ClientMsg::Key { pane, key } => {
                let Some(rt) = self.panes.get_mut(&pane) else {
                    return;
                };
                if rt.info.reconnect_prompt.is_some() {
                    match &key.key {
                        at_proto::KeyCode::Named(at_proto::NamedKey::Enter) => {
                            self.on_cmd(cid, Cmd::Reconnect { pane, accept: true })
                        }
                        at_proto::KeyCode::Named(at_proto::NamedKey::Escape) => self.on_cmd(
                            cid,
                            Cmd::Reconnect {
                                pane,
                                accept: false,
                            },
                        ),
                        _ => {}
                    }
                    return;
                }
                if rt.pane.is_exited() {
                    if matches!(key.key, at_proto::KeyCode::Named(at_proto::NamedKey::Enter)) {
                        self.on_cmd(cid, Cmd::RestartShell { pane });
                    }
                    return;
                }
                rt.pane.key(&key);
            }
            ClientMsg::Text { pane, text } => {
                if let Some(rt) = self.panes.get(&pane) {
                    rt.pane.text(&text);
                }
            }
            ClientMsg::Paste { pane, text } => {
                if let Some(rt) = self.panes.get(&pane) {
                    rt.pane.paste(&text);
                }
            }
            ClientMsg::Resize {
                pane,
                cols,
                rows,
                cell_w,
                cell_h,
            } => {
                if let Some(rt) = self.panes.get(&pane) {
                    rt.pane.resize(cols, rows, cell_w, cell_h);
                }
            }
            ClientMsg::Mouse { pane, input } => {
                let copy = self.config.terminal.copy_on_select;
                if let Some(rt) = self.panes.get(&pane)
                    && let Some(text) = rt.pane.mouse(&input, copy)
                {
                    self.send(cid, ServerMsg::Clipboard(text));
                }
            }
            ClientMsg::Scroll { pane, to } => {
                if let Some(rt) = self.panes.get(&pane) {
                    rt.pane.scroll(to);
                }
            }
            ClientMsg::Focus { pane, focused } => {
                if let Some(rt) = self.panes.get(&pane) {
                    rt.pane.focus(focused);
                }
                if focused {
                    self.clear_attention(pane);
                }
            }
            ClientMsg::Copy { pane } => {
                if let Some(text) = self
                    .panes
                    .get(&pane)
                    .and_then(|rt| rt.pane.selection_text())
                {
                    self.send(cid, ServerMsg::Clipboard(text));
                }
            }
            ClientMsg::SelectAll { pane } => {
                if let Some(rt) = self.panes.get(&pane) {
                    rt.pane.select_all();
                }
            }
            ClientMsg::ClearSelection { pane } => {
                if let Some(rt) = self.panes.get(&pane) {
                    rt.pane.clear_selection();
                }
            }
            ClientMsg::ClearScreen { pane } => {
                if let Some(rt) = self.panes.get(&pane) {
                    let idle = !rt.info.running;
                    rt.pane.clear_screen(idle);
                }
            }
            ClientMsg::Search {
                pane,
                query,
                regex,
                forward,
            } => {
                if let Some(rt) = self.panes.get(&pane) {
                    rt.pane.search(&query, regex, forward);
                }
            }
            ClientMsg::SearchClose { pane } => {
                if let Some(rt) = self.panes.get(&pane) {
                    rt.pane.search_close();
                }
            }
            ClientMsg::GetHistory { pane } => {
                let entries = history::read(&history::path_for(&paths::history_dir(), pane));
                self.send(cid, ServerMsg::History { pane, entries });
            }
            ClientMsg::RunCommand {
                pane,
                command,
                execute,
            } => {
                if let Some(rt) = self.panes.get(&pane) {
                    let mut text = command.replace('\n', " ");
                    if execute {
                        text.push('\r');
                    }
                    rt.pane.text(&text);
                }
            }
            ClientMsg::ClearAttention { pane } => self.clear_attention(pane),
            ClientMsg::Detach => {
                if let Some(c) = self.clients.get_mut(&cid) {
                    c.hello = false;
                }
                self.save(true);
            }
            ClientMsg::Shutdown => {
                self.shutdown = true;
            }
            ClientMsg::Ping => self.send(cid, ServerMsg::Pong),
        }
    }

    fn clear_attention(&mut self, pane: PaneId) {
        let Some(tab) = self.ws.tab_of_pane(pane) else {
            return;
        };
        let panes = self
            .ws
            .tab(tab)
            .map(|t| t.panes.clone())
            .unwrap_or_default();
        for p in panes {
            if let Some(rt) = self.panes.get_mut(&p)
                && rt.info.attention
            {
                rt.info.attention = false;
                let info = rt.info.clone();
                self.broadcast(ServerMsg::Pane(info));
            }
        }
    }

    fn new_tab_node(
        &mut self,
        profile: &str,
        cwd: Option<PathBuf>,
        from: Option<PaneId>,
        command: Option<String>,
    ) -> Option<TabNode> {
        let id = PaneId::new();
        let cwd = Some(self.resolve_cwd(profile, cwd, from));
        if let Err(e) = self.spawn_pane(id, profile, cwd, None, command.is_none()) {
            tracing::error!("spawn failed: {e}");
            self.broadcast(ServerMsg::Notice(Notice::Info(format!(
                "spawn failed: {e}"
            ))));
            return None;
        }
        if let Some(c) = command.filter(|c| !c.is_empty()) {
            let rt = self.panes.get_mut(&id).unwrap();
            rt.pending_cmd = Some(c);
            rt.pending_deadline = Some(Instant::now() + Duration::from_millis(500));
        }
        let mut node = TabNode::new(id, profile);
        let prof = self.config.profile(profile);
        if !prof.title.is_empty() {
            node.custom_title = Some(prof.title.clone());
        }
        Some(node)
    }

    fn on_cmd(&mut self, cid: ClientId, cmd: Cmd) {
        let _ = cid;
        match cmd {
            Cmd::NewWindow {
                cwd,
                profile,
                command,
                geom,
            } => {
                let profile = profile.unwrap_or_else(|| "default".into());
                let mut w = WindowNode::new(self.config.appearance.sidebar_width);
                w.geom = geom;
                let wid = w.id;
                let from = self
                    .ws
                    .windows
                    .last()
                    .and_then(|w| self.ws.active_pane(w.id));
                self.ws.windows.push(w);
                match self.new_tab_node(&profile, cwd.map(PathBuf::from), from, command) {
                    Some(node) => self.ws.insert_after(wid, None, node),
                    None => self.ws.windows.retain(|w| w.id != wid),
                }
            }
            Cmd::RestoreDetachedWindow => {
                if self.ws.detached.is_empty() {
                    return self.on_cmd(
                        cid,
                        Cmd::NewWindow {
                            cwd: None,
                            profile: None,
                            command: None,
                            geom: None,
                        },
                    );
                }
                let mut w = WindowNode::new(self.config.appearance.sidebar_width);
                w.tabs = std::mem::take(&mut self.ws.detached);
                let first = w.tabs.first().map(|t| t.id);
                self.ws.windows.push(w);
                if let Some(f) = first {
                    self.ws.select_tab(f);
                }
            }
            Cmd::FocusWindow { window } => {
                if self.ws.window(window).is_some() {
                    self.ws.focused_window = Some(window);
                }
                return;
            }
            Cmd::CloseWindow { window, mode } => match mode {
                CloseWindowMode::Detach => {
                    self.ws.detach_window(window);
                }
                CloseWindowMode::CloseTabs => {
                    let panes = self.ws.window_panes(window);
                    self.ws.windows.retain(|w| w.id != window);
                    self.terminate_panes(panes);
                }
            },
            Cmd::NewTab {
                window,
                nested,
                profile,
                cwd,
                command,
            } => {
                let profile = profile.unwrap_or_else(|| "default".into());
                let active = self.ws.window(window).and_then(|w| w.active_tab);
                let from = self.ws.active_pane(window);
                if let Some(node) =
                    self.new_tab_node(&profile, cwd.map(PathBuf::from), from, command)
                {
                    match (nested, active) {
                        // "Nested": another terminal tab in the current group.
                        (true, Some(anchor)) => {
                            self.ws.add_to_group(anchor, node);
                        }
                        // A new tab is a new group holding one terminal tab.
                        (_, _) => {
                            self.ws.new_group(window, active, node);
                        }
                    }
                }
            }
            Cmd::DuplicateTab { tab } => {
                let Some(t) = self.ws.tab(tab) else { return };
                let profile = t.profile.clone();
                let from = Some(t.active_pane);
                let Some(window) = self.ws.window_of_tab(tab) else {
                    return;
                };
                if let Some(mut node) = self.new_tab_node(&profile, None, from, None) {
                    node.custom_title = self.ws.tab(tab).and_then(|t| t.custom_title.clone());
                    if self.ws.is_group(tab) {
                        self.ws.new_group(window, Some(tab), node);
                    } else {
                        self.ws.add_to_group(tab, node);
                    }
                }
            }
            Cmd::CloseTab { tab, with_children } => {
                let window = self.ws.window_of_tab(tab);
                let promote = !with_children
                    && self.config.terminal.close_tab_with_children == CloseChildren::Promote;
                let slot = self.ws.slot_of(tab);
                if let Some((node, panes)) = self.ws.remove_tab(tab, promote) {
                    if let Some(w) = window {
                        let infos = panes
                            .iter()
                            .filter_map(|p| self.panes.get(p).map(|r| r.info.clone()))
                            .collect();
                        self.closed.push(ClosedTab {
                            window: w,
                            slot,
                            node,
                            infos,
                            at: Instant::now(),
                        });
                        if self.closed.len() > 20 {
                            self.closed.remove(0);
                        }
                    }
                    self.terminate_panes(panes);
                    self.ws.windows.retain(|w| !w.tabs.is_empty());
                }
            }
            Cmd::RenameTab { tab, title } => {
                if let Some(t) = self.ws.tab_mut(tab) {
                    t.custom_title = title
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());
                }
            }
            Cmd::SelectTab { tab } => {
                self.ws.select_tab(tab);
                if let Some(p) = self.ws.tab(tab).map(|t| t.active_pane) {
                    self.clear_attention(p);
                }
            }
            Cmd::MoveTab { tab, down } => {
                self.ws.move_tab(tab, down);
            }
            Cmd::IndentTab { tab } => {
                self.ws.indent_tab(tab);
            }
            Cmd::OutdentTab { tab } => {
                self.ws.outdent_tab(tab);
            }
            Cmd::ToggleCollapse { tab } => {
                self.ws.toggle_collapse(tab);
            }
            Cmd::ToggleGroupView { tab } => {
                self.ws.toggle_group_view(tab);
            }
            Cmd::SetGroupMode { tab, mode } => {
                self.ws.set_group_mode(tab, mode);
            }
            Cmd::SetGroupSizes { root, sizes } => {
                self.ws.set_group_sizes(root, sizes);
            }
            Cmd::DropTab { tab, target, pos } => {
                self.ws.drop_tab(tab, target, pos);
                self.ws.windows.retain(|w| !w.tabs.is_empty());
            }
            Cmd::MoveTabToWindow { tab, window } => {
                let target = match window {
                    Some(w) => w,
                    None => {
                        let mut w = WindowNode::new(self.config.appearance.sidebar_width);
                        if let Some(src) =
                            self.ws.window_of_tab(tab).and_then(|id| self.ws.window(id))
                        {
                            w.sidebar_width = src.sidebar_width;
                            w.geom = src.geom.clone().map(|g| WindowGeom {
                                x: g.x + 40.0,
                                y: g.y + 40.0,
                                ..g
                            });
                        }
                        let id = w.id;
                        self.ws.windows.push(w);
                        id
                    }
                };
                self.ws.move_tab_to_window(tab, target);
                self.ws.windows.retain(|w| !w.tabs.is_empty());
            }
            Cmd::AttachDetached { tab, window } => {
                self.ws.attach_detached(tab, window);
            }
            Cmd::ReopenClosedTab { window } => {
                let cutoff = Duration::from_secs(600);
                while let Some(c) = self.closed.pop() {
                    if c.at.elapsed() > cutoff {
                        continue;
                    }
                    let target = if self.ws.window(window).is_some() {
                        window
                    } else {
                        c.window
                    };
                    let mut node = c.node;
                    // Respawn every terminal tab of the closed node (a group has several).
                    let mut alive = 0;
                    let terms: Vec<&mut TabNode> = if node.is_group() {
                        node.children.iter_mut().collect()
                    } else {
                        vec![&mut node]
                    };
                    let mut spawn_list = Vec::new();
                    for t in terms {
                        for p in t.panes.clone() {
                            let cwd = c
                                .infos
                                .iter()
                                .find(|i| i.id == p)
                                .and_then(|i| i.cwd.clone())
                                .map(PathBuf::from);
                            spawn_list.push((t.id, p, t.profile.clone(), cwd));
                        }
                    }
                    let mut ok = Vec::new();
                    for (_, p, profile, cwd) in spawn_list {
                        if self.spawn_pane(p, &profile, cwd, None, false).is_ok() {
                            ok.push(p);
                            alive += 1;
                        }
                    }
                    if alive == 0 {
                        break;
                    }
                    let fix = |t: &mut TabNode| {
                        t.panes.retain(|p| ok.contains(p));
                        t.sanitize();
                    };
                    if node.is_group() {
                        node.children.iter_mut().for_each(fix);
                        node.children.retain(|t| !t.panes.is_empty());
                        let active = self.ws.window(target).and_then(|w| w.active_tab);
                        self.ws.insert_after(target, active, node);
                    } else {
                        fix(&mut node);
                        let (group, index) = c.slot.map(|(g, i)| (Some(g), i)).unwrap_or((None, 0));
                        self.ws.restore_tab(target, group, index, node);
                    }
                    break;
                }
            }
            Cmd::NewPane { tab } => {
                let Some(t) = self.ws.tab(tab) else { return };
                if !t.can_add_pane() {
                    return;
                }
                let profile = t.profile.clone();
                let from = Some(t.active_pane);
                let id = PaneId::new();
                let cwd = Some(self.resolve_cwd(&profile, None, from));
                if self.spawn_pane(id, &profile, cwd, None, false).is_ok() {
                    self.ws.tab_mut(tab).unwrap().add_pane(id);
                }
            }
            Cmd::ClosePane { pane } => {
                self.remove_pane_from_tree(pane);
                self.terminate_panes(vec![pane]);
            }
            Cmd::FocusPane { pane } => {
                if let Some(tab) = self.ws.tab_of_pane(pane) {
                    if let Some(t) = self.ws.tab_mut(tab) {
                        t.active_pane = pane;
                    }
                    self.ws.select_tab(tab);
                    self.clear_attention(pane);
                }
            }
            Cmd::FocusDirection { tab, dir } => {
                if let Some(t) = self.ws.tab_mut(tab)
                    && let Some(p) = t.neighbour(dir)
                {
                    t.active_pane = p;
                }
            }
            Cmd::ToggleLayout { tab } => {
                if let Some(t) = self.ws.tab_mut(tab) {
                    t.toggle_layout();
                }
            }
            Cmd::SetSizes { tab, sizes } => {
                if let Some(t) = self.ws.tab_mut(tab) {
                    t.set_sizes(sizes);
                }
            }
            Cmd::SetGrid { tab, rows, cols } => {
                if let Some(t) = self.ws.tab_mut(tab) {
                    let ok = |v: [f32; 2]| {
                        v.iter().all(|x| x.is_finite() && *x > 0.1)
                            && (v[0] + v[1] - 1.0).abs() < 0.01
                    };
                    if ok(rows) && ok(cols) {
                        t.grid_rows = rows;
                        t.grid_cols = cols;
                    }
                }
            }
            Cmd::ResizePane { tab, dir } => {
                if let Some(t) = self.ws.tab_mut(tab) {
                    t.resize_active(dir, 0.05);
                }
            }
            Cmd::EqualizePanes { tab } => {
                if let Some(t) = self.ws.tab_mut(tab) {
                    t.equalize();
                }
            }
            Cmd::ZoomPane { tab } => {
                if let Some(t) = self.ws.tab_mut(tab) {
                    t.zoomed = !t.zoomed && t.panes.len() > 1;
                }
            }
            Cmd::MovePane { tab, down } => {
                if let Some(t) = self.ws.tab_mut(tab) {
                    t.move_active_pane(down);
                }
            }
            Cmd::RestartShell { pane } => {
                let Some(rt) = self.panes.get(&pane) else {
                    return;
                };
                let profile = rt.info.profile.clone();
                let cwd = rt.info.cwd.clone().map(PathBuf::from);
                let size = rt.pane.size();
                let old = self.panes.remove(&pane).unwrap();
                let p = old.pane.clone();
                let h = std::thread::spawn(move || {
                    p.terminate(Duration::from_millis(500), Duration::from_millis(500))
                });
                self.terminators.push(h);
                for c in self.clients.values_mut() {
                    c.sent.remove(&pane);
                }
                if self.spawn_pane(pane, &profile, cwd, None, true).is_ok() {
                    if let Some(rt) = self.panes.get(&pane) {
                        rt.pane.resize(size.0, size.1, 8, 17);
                    }
                    self.broadcast_pane(pane);
                }
                return;
            }
            Cmd::WindowGeometry { window, geom } => {
                if let Some(w) = self.ws.window_mut(window) {
                    w.geom = Some(geom);
                }
                if self.structure_dirty_at.is_none() {
                    self.structure_dirty_at = Some(Instant::now());
                }
                return;
            }
            Cmd::Sidebar {
                window,
                width,
                hidden,
            } => {
                if let Some(w) = self.ws.window_mut(window) {
                    w.sidebar_width = width.clamp(160.0, 400.0);
                    w.sidebar_hidden = hidden;
                }
            }
            Cmd::Reconnect { pane, accept } => {
                if let Some(rt) = self.panes.get_mut(&pane) {
                    rt.info.reconnect_prompt = None;
                    let cmd = rt.reconnect_cmd.take();
                    if accept && let Some(c) = cmd {
                        rt.pane.text(&format!("{c}\r"));
                    }
                }
                self.broadcast_pane(pane);
                return;
            }
            Cmd::SetFontScale { pane, scale } => {
                if let Some(rt) = self.panes.get_mut(&pane) {
                    rt.info.font_scale = scale.clamp(0.5, 3.0);
                }
                self.broadcast_pane(pane);
                return;
            }
            Cmd::ReloadConfig => {
                self.reload_config();
                return;
            }
        }
        self.structure_changed();
    }

    // ----- pane events -----

    fn tab_is_active(&self, pane: PaneId) -> bool {
        let Some(tab) = self.ws.tab_of_pane(pane) else {
            return false;
        };
        self.ws.windows.iter().any(|w| w.active_tab == Some(tab))
    }

    fn send_pending_cmd(&mut self, id: PaneId) {
        if let Some(rt) = self.panes.get_mut(&id)
            && let Some(c) = rt.pending_cmd.take()
        {
            rt.pending_deadline = None;
            rt.pane.text(&format!("{c}\r"));
        }
    }

    fn on_pane(&mut self, ev: PaneEvent) {
        match ev {
            PaneEvent::Dirty(_) => {}
            PaneEvent::Title(id, t) => {
                if let Some(rt) = self.panes.get_mut(&id) {
                    rt.info.osc_title = t;
                    self.broadcast_pane(id);
                }
            }
            PaneEvent::Bell(id) => {
                let active = self.tab_is_active(id);
                if let Some(rt) = self.panes.get_mut(&id)
                    && !active
                    && !rt.info.attention
                {
                    rt.info.attention = true;
                    self.broadcast_pane(id);
                }
                self.broadcast(ServerMsg::Bell(id));
            }
            PaneEvent::Clipboard(_, text) => {
                if self.config.terminal.osc52 != at_core::config::Osc52::Off {
                    self.broadcast(ServerMsg::Clipboard(text));
                }
            }
            PaneEvent::Osc(id, ev) => self.on_osc(id, ev),
            PaneEvent::Exited(id, code) => {
                if let Some(rt) = self.panes.get_mut(&id) {
                    rt.info.exited = Some(code);
                    rt.info.running = false;
                    rt.info.remote_host = None;
                    rt.hist.close();
                    self.broadcast_pane(id);
                }
            }
        }
    }

    fn on_osc(&mut self, id: PaneId, ev: OscEvent) {
        let threshold = Duration::from_secs(self.config.terminal.attention_threshold_sec);
        let active = self.tab_is_active(id);
        let Some(rt) = self.panes.get_mut(&id) else {
            return;
        };
        rt.info.integration = true;
        let mut send_pending = false;
        match ev {
            OscEvent::Cwd(c) => {
                rt.last_osc_cwd = Some(Instant::now());
                if rt.info.cwd.as_deref() != Some(&c) {
                    rt.info.cwd = Some(c);
                }
            }
            OscEvent::PromptStart => {
                rt.osc_running = false;
                rt.info.running = rt
                    .fg_since
                    .map(|t| t.elapsed() > Duration::from_millis(300))
                    .unwrap_or(false);
                if rt.pending_cmd.is_some() {
                    send_pending = true;
                }
            }
            OscEvent::CommandStart => {
                rt.osc_running = true;
                rt.cmd_started = Some(Instant::now());
                rt.info.running = true;
            }
            OscEvent::CommandEnd(exit) => {
                rt.osc_running = false;
                rt.info.running = false;
                rt.info.last_exit = exit;
                let _ = rt.hist.finish(exit.unwrap_or(0));
                if let Some(t) = rt.cmd_started.take()
                    && t.elapsed() >= threshold
                    && !active
                {
                    rt.info.attention = true;
                }
            }
            OscEvent::Command(cmd) => {
                rt.info.last_command = Some(cmd.clone());
                let entry = HistoryEntry {
                    ts_ms: at_core::time::unix_ms(),
                    cwd: rt.info.cwd.clone().unwrap_or_default(),
                    exit: None,
                    command: cmd,
                };
                if let Err(e) = rt.hist.start(entry) {
                    tracing::warn!("history write: {e}");
                }
            }
            OscEvent::Remote(h) => {
                rt.remote_osc = h.clone();
                rt.info.remote_host = h;
                if rt.info.remote_host.is_some() {
                    rt.info.remote_command = rt.info.last_command.clone();
                }
            }
        }
        self.broadcast_pane(id);
        if send_pending {
            self.send_pending_cmd(id);
        }
    }

    fn poll_processes(&mut self) {
        let programs = self.config.terminal.remote_programs.clone();
        let mut changed = Vec::new();
        for (id, rt) in self.panes.iter_mut() {
            if rt.pane.is_exited() {
                continue;
            }
            let before = rt.info.clone();
            let shell = rt.pane.shell_pid();
            let fg = rt.pane.foreground_pgid();
            let fg_busy = fg.map(|g| g > 0 && g != shell).unwrap_or(false);
            if fg_busy {
                rt.fg_since.get_or_insert_with(Instant::now);
            } else {
                rt.fg_since = None;
            }
            let fg_running = rt
                .fg_since
                .map(|t| t.elapsed() >= Duration::from_millis(300))
                .unwrap_or(false);
            rt.info.running = rt.osc_running || fg_running;
            let fg_pid = fg.filter(|_| fg_busy);
            rt.info.fg_pid = fg_pid.map(|p| p as u32);
            rt.info.fg_process = fg_pid
                .and_then(procinfo::name)
                .or_else(|| procinfo::name(shell));
            // Remote detection by the foreground program.
            let proc_remote = fg_pid.and_then(|pid| {
                let name = procinfo::name(pid)?;
                let args = procinfo::args(pid);
                if procinfo::is_remote(&name, args.as_deref(), &programs) {
                    Some(
                        args.as_deref()
                            .and_then(procinfo::remote_host)
                            .unwrap_or(name),
                    )
                } else {
                    None
                }
            });
            if let (Some(pid), Some(_)) = (fg_pid, &proc_remote)
                && (rt.info.remote_command.is_none() || rt.remote_osc.is_none())
            {
                rt.info.remote_command = procinfo::args(pid).map(|a| a.join(" "));
            }
            rt.info.remote_host = if fg_busy {
                rt.remote_osc.clone().or(proc_remote)
            } else {
                None
            };
            if !fg_busy {
                rt.remote_osc = None;
                if rt.info.remote_host.is_none() {
                    rt.info.remote_command = None;
                }
            }
            // cwd fallback when integration is silent.
            let stale = rt
                .last_osc_cwd
                .map(|t| t.elapsed() > Duration::from_secs(2))
                .unwrap_or(true);
            if stale
                && !rt.info.integration
                && let Some(c) = procinfo::cwd(shell)
            {
                rt.info.cwd = Some(c);
            }
            if rt.info != before {
                changed.push(*id);
            }
        }
        for id in changed {
            self.broadcast_pane(id);
        }
    }

    // ----- frames -----

    fn flush_frames(&mut self) {
        let mut wanted: HashSet<PaneId> = HashSet::new();
        for c in self.clients.values().filter(|c| c.hello) {
            for p in &c.visible {
                let fresh = !c.sent.contains_key(p);
                if fresh {
                    wanted.insert(*p);
                }
            }
        }
        let mut frames: HashMap<PaneId, Frame> = HashMap::new();
        for (id, rt) in &self.panes {
            let dirty = rt.pane.take_dirty();
            if (dirty || wanted.contains(id))
                && self
                    .clients
                    .values()
                    .any(|c| c.hello && c.visible.contains(id))
            {
                frames.insert(*id, rt.pane.frame());
            }
        }
        if frames.is_empty() {
            return;
        }
        for c in self.clients.values_mut().filter(|c| c.hello) {
            for (id, frame) in &frames {
                if !c.visible.contains(id) {
                    continue;
                }
                let sent = c.sent.entry(*id).or_default();
                let full = sent.rows.len() != frame.lines.len() || sent.cols != frame.cols;
                let mut lines = Vec::new();
                let mut hashes = Vec::with_capacity(frame.lines.len());
                for (i, (idx, row)) in frame.lines.iter().enumerate() {
                    let h = row_hash(row);
                    hashes.push(h);
                    if full || sent.rows.get(i) != Some(&h) {
                        lines.push((*idx, row.clone()));
                    }
                }
                let meta_changed = sent.cursor != Some(frame.cursor)
                    || sent.offset != frame.display_offset
                    || sent.history != frame.history_size
                    || sent.mode != frame.mode
                    || sent.search != frame.search;
                if !full && lines.is_empty() && !meta_changed {
                    continue;
                }
                sent.rows = hashes;
                sent.cols = frame.cols;
                sent.cursor = Some(frame.cursor);
                sent.offset = frame.display_offset;
                sent.history = frame.history_size;
                sent.mode = frame.mode;
                sent.search = frame.search.clone();
                let out = Frame {
                    pane: *id,
                    cols: frame.cols,
                    rows: frame.rows,
                    full,
                    lines,
                    cursor: frame.cursor,
                    display_offset: frame.display_offset,
                    history_size: frame.history_size,
                    mode: frame.mode,
                    search: frame.search.clone(),
                };
                let _ = c.tx.send(ServerMsg::Frame(out));
            }
        }
    }

    // ----- test helpers -----

    #[doc(hidden)]
    pub fn workspace(&self) -> &Workspace {
        &self.ws
    }
}

/// Compact state of the workspace for `alexterminal list`.
pub fn describe(ws: &Workspace, panes: &[PaneInfo]) -> serde_json::Value {
    use serde_json::json;
    fn tab_json(t: &TabNode, panes: &[PaneInfo]) -> serde_json::Value {
        let info = |p: &PaneId| panes.iter().find(|i| i.id == *p);
        json!({
            "id": t.id.to_string(),
            "title": t.custom_title.clone().or_else(|| info(&t.active_pane).map(|i| i.auto_title())),
            "layout": if t.layout == Layout::Grid { "grid" } else { "stack" },
            "panes": t.panes.iter().map(|p| {
                let i = info(p);
                json!({
                    "id": p.to_string(),
                    "cwd": i.and_then(|i| i.cwd.clone()),
                    "state": i.map(|i| format!("{:?}", i.state()).to_lowercase()),
                    "remote": i.and_then(|i| i.remote_host.clone()),
                })
            }).collect::<Vec<_>>(),
            "children": t.children.iter().map(|c| tab_json(c, panes)).collect::<Vec<_>>(),
        })
    }
    json!({
        "windows": ws.windows.iter().map(|w| json!({
            "id": w.id.to_string(),
            "tabs": w.tabs.iter().map(|t| tab_json(t, panes)).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "detached": ws.detached.iter().map(|t| tab_json(t, panes)).collect::<Vec<_>>(),
    })
}

pub fn state_label(s: PaneState) -> &'static str {
    match s {
        PaneState::Attention => "attention",
        PaneState::Remote => "remote",
        PaneState::Running => "running",
        PaneState::Exited => "exited",
        PaneState::Idle => "idle",
    }
}
