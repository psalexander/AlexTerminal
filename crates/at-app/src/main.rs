//! `alexterminal`: the GUI client and command-line entry point (§4.12).

mod app;
mod conn;
mod debug;
mod i18n;
mod keys;
mod layout;
mod links;
mod menu;
mod metrics;
mod platform;
mod settings;
mod style;
mod term_view;
mod view;

pub use app::Message;

use app::{App, Initial};
use at_proto::{ClientKind, ClientMsg, Cmd, ServerMsg};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "alexterminal",
    version,
    about = "AlexTerminal: a terminal with a tab tree and session restore"
)]
struct Cli {
    /// Start directory for the new tab or window.
    #[arg(long, short = 'd', value_name = "DIR")]
    working_directory: Option<PathBuf>,
    /// Profile for the new tab.
    #[arg(long)]
    profile: Option<String>,
    /// Command to run in the new tab.
    #[arg(long, short = 'e', num_args = 1.., value_name = "CMD")]
    command: Option<Vec<String>>,
    /// Open a new window instead of a tab in the active window.
    #[arg(long)]
    new_window: bool,
    /// Start with a clean session; the saved one is kept until the next save.
    #[arg(long)]
    no_restore: bool,
    /// Use another config file.
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,
    /// Restore from a specific snapshot file (debugging).
    #[arg(long, value_name = "FILE")]
    restore_from: Option<PathBuf>,
    #[arg(long)]
    print_config_path: bool,
    #[arg(long)]
    print_data_path: bool,
    #[command(subcommand)]
    sub: Option<Sub>,
}

#[derive(Subcommand, Debug)]
enum Sub {
    /// Print windows, tabs and panes of the running app.
    List {
        #[arg(long)]
        json: bool,
    },
}

fn init_logging() -> tracing_appender::non_blocking::WorkerGuard {
    let dir = at_core::paths::log_dir();
    let _ = std::fs::create_dir_all(&dir);
    let file = tracing_appender::rolling::daily(&dir, "gui.log");
    let (nb, guard) = tracing_appender::non_blocking(file);
    let filter = tracing_subscriber::EnvFilter::try_from_env("ALEXTERMINAL_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_writer(nb)
        .with_ansi(false)
        .with_env_filter(filter)
        .init();
    guard
}

fn wait_welcome(rx: &crossbeam_channel::Receiver<ServerMsg>) -> anyhow::Result<ServerMsg> {
    loop {
        match rx.recv_timeout(Duration::from_secs(5))? {
            m @ ServerMsg::Welcome { .. } => return Ok(m),
            ServerMsg::Error(e) => anyhow::bail!(e),
            _ => {}
        }
    }
}

fn list(json: bool) -> anyhow::Result<()> {
    let conn = at_client::connect_existing(ClientKind::Cli)?;
    let ServerMsg::Welcome {
        workspace,
        panes,
        gui_clients,
        ..
    } = wait_welcome(&conn.rx)?
    else {
        unreachable!()
    };
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&at_daemon::server::describe(&workspace, &panes))?
        );
        return Ok(());
    }
    println!(
        "{} window(s), {} pane(s), {} GUI client(s)",
        workspace.windows.len(),
        panes.len(),
        gui_clients
    );
    fn walk(t: &at_core::model::TabNode, depth: usize, panes: &[at_core::pane::PaneInfo]) {
        let info = |p: &at_core::PaneId| panes.iter().find(|i| i.id == *p);
        let title = t
            .custom_title
            .clone()
            .or_else(|| info(&t.active_pane).map(|i| i.auto_title()))
            .unwrap_or_default();
        println!(
            "{}- {} [{} pane(s)]",
            "  ".repeat(depth + 1),
            title,
            t.panes.len()
        );
        for p in &t.panes {
            if let Some(i) = info(p) {
                println!(
                    "{}  · {} {}",
                    "  ".repeat(depth + 1),
                    at_daemon::server::state_label(i.state()),
                    i.cwd.clone().unwrap_or_default()
                );
            }
        }
        for c in &t.children {
            walk(c, depth + 1, panes);
        }
    }
    for (n, w) in workspace.windows.iter().enumerate() {
        println!("window {}", n + 1);
        for t in &w.tabs {
            walk(t, 0, &panes);
        }
    }
    if !workspace.detached.is_empty() {
        println!("detached");
        for t in &workspace.detached {
            walk(t, 0, &panes);
        }
    }
    Ok(())
}

fn window_icon() -> Option<iced::window::Icon> {
    iced::window::icon::from_file_data(include_bytes!("../../../assets/icon-256.png"), None).ok()
}

/// Fonts shipped in `assets/fonts` (repo) or `Resources/fonts` (app bundle).
fn bundled_fonts() -> Vec<Vec<u8>> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(d) = exe.parent()
    {
        dirs.push(d.join("../Resources/fonts"));
        dirs.push(d.join("../share/alexterminal/fonts"));
        dirs.push(d.join("fonts"));
    }
    dirs.push(PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts"
    )));
    let mut out = Vec::new();
    for d in dirs {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                if matches!(p.extension().and_then(|s| s.to_str()), Some("ttf" | "otf"))
                    && let Ok(b) = std::fs::read(&p)
                {
                    out.push(b);
                }
            }
            if !out.is_empty() {
                break;
            }
        }
    }
    out
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if let Some(cfg) = &cli.config {
        // Read by the in-process session server.
        unsafe { std::env::set_var("ALEXTERMINAL_CONFIG", cfg) };
    }
    if cli.print_config_path {
        println!("{}", at_core::paths::config_file().display());
        return Ok(());
    }
    if cli.print_data_path {
        println!("{}", at_core::paths::data_dir().display());
        return Ok(());
    }
    if let Some(Sub::List { json }) = cli.sub {
        return list(json);
    }
    let _guard = init_logging();
    let initial = Initial {
        cwd: cli
            .working_directory
            .as_ref()
            .map(|p| p.canonicalize().unwrap_or(p.clone()).display().to_string()),
        profile: cli.profile.clone(),
        command: cli.command.as_ref().map(|v| v.join(" ")),
    };
    let wants_tab = initial.cwd.is_some()
        || initial.command.is_some()
        || initial.profile.is_some()
        || cli.new_window;
    // A running app takes the request (a new tab or window) and this launch exits (§4.12).
    if let Some((c, ServerMsg::Welcome { workspace, .. })) = at_client::find_running()? {
        let cmd = if cli.new_window || workspace.windows.is_empty() {
            Cmd::NewWindow {
                cwd: initial.cwd.clone(),
                profile: initial.profile.clone(),
                command: initial.command.clone(),
                geom: None,
            }
        } else {
            let window = workspace
                .focused_window
                .filter(|w| workspace.window(*w).is_some())
                .unwrap_or(workspace.windows[0].id);
            if wants_tab {
                Cmd::NewTab {
                    window,
                    nested: false,
                    profile: initial.profile.clone(),
                    cwd: initial.cwd.clone(),
                    command: initial.command.clone(),
                }
            } else {
                Cmd::NewWindow {
                    cwd: None,
                    profile: None,
                    command: None,
                    geom: None,
                }
            }
        };
        let _ = c.tx.send(ClientMsg::Cmd(cmd));
        let _ = c.tx.send(ClientMsg::Detach);
        std::thread::sleep(Duration::from_millis(150));
        return Ok(());
    }
    let conn = at_client::start(!cli.no_restore, cli.restore_from.clone())?;
    // Register bundled fonts before the app resolves its font family.
    let fonts = bundled_fonts();
    if let Ok(mut fs) = iced::advanced::graphics::text::font_system().write() {
        for f in &fonts {
            fs.load_font(std::borrow::Cow::Owned(f.clone()));
        }
    }
    conn::install(conn.rx);
    let tx = conn.tx;
    let icon = window_icon();
    let mut daemon = iced::daemon(
        move || {
            (
                App::new(tx.clone(), initial.clone(), icon.clone()),
                iced::Task::none(),
            )
        },
        App::update,
        App::view,
    )
    .title(App::title)
    .theme(App::theme_for)
    .subscription(App::subscription)
    .antialiasing(true);
    for f in fonts {
        daemon = daemon.font(f);
    }
    daemon.run()?;
    Ok(())
}
