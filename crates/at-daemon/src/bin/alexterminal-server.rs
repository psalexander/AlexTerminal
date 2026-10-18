//! `alexterminal-server`: the session daemon.

use at_core::paths;
use at_daemon::server::{Server, ServerOptions};
use at_daemon::transport;
use at_proto::{ClientKind, ClientMsg, PROTO_VERSION, ServerMsg};
use std::io::BufReader;
use std::os::unix::net::UnixStream;

const USAGE: &str = "alexterminal-server [--daemon | --foreground] [--no-restore] [--restore-from FILE]
alexterminal-server --shutdown | --status | --install-autostart | --uninstall-autostart | --version";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    if has("--help") || has("-h") {
        println!("{USAGE}");
        return;
    }
    if has("--version") {
        println!(
            "alexterminal-server {} (protocol {PROTO_VERSION})",
            env!("CARGO_PKG_VERSION")
        );
        return;
    }
    if has("--shutdown") {
        std::process::exit(match control(ClientMsg::Shutdown) {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        });
    }
    if has("--status") {
        std::process::exit(match status() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("not running: {e}");
                1
            }
        });
    }
    if has("--install-autostart") || has("--uninstall-autostart") {
        let exe = std::env::current_exe().expect("current exe");
        match at_daemon::autostart::set(has("--install-autostart"), &exe) {
            Ok(()) => println!("{}", at_daemon::autostart::agent_path().display()),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let restore_from = args
        .iter()
        .position(|a| a == "--restore-from")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from);
    if has("--daemon") {
        daemonize();
    }
    let _guard = init_logging();
    let lock = match transport::InstanceLock::acquire(&paths::lock_path()) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("{e}");
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let listener = match transport::bind(&paths::socket_path()) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("bind {}: {e}", paths::socket_path().display());
            eprintln!("bind {}: {e}", paths::socket_path().display());
            std::process::exit(2);
        }
    };
    let stop = transport::install_signal_handlers();
    let (tx, rx) = crossbeam_channel::unbounded();
    transport::serve(listener, tx.clone());
    let opts = ServerOptions {
        restore: !has("--no-restore"),
        restore_from,
        stop,
        exit_when_empty: false,
    };
    tracing::info!(
        "alexterminal-server {} pid {} socket {}",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        paths::socket_path().display()
    );
    let server = Server::new(rx, opts);
    server.run();
    drop(tx);
    drop(lock);
    let _ = std::fs::remove_file(paths::lock_path());
}

/// Detaches from the launching terminal: fork, setsid, stdio to /dev/null.
fn daemonize() {
    unsafe {
        match libc::fork() {
            -1 => {
                eprintln!("fork failed");
                std::process::exit(1);
            }
            0 => {}
            _ => libc::_exit(0),
        }
        libc::setsid();
        let devnull = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
        if devnull >= 0 {
            libc::dup2(devnull, 0);
            libc::dup2(devnull, 1);
            libc::dup2(devnull, 2);
            if devnull > 2 {
                libc::close(devnull);
            }
        }
    }
}

fn init_logging() -> tracing_appender::non_blocking::WorkerGuard {
    let dir = paths::log_dir();
    let _ = std::fs::create_dir_all(&dir);
    let file = tracing_appender::rolling::daily(&dir, "server.log");
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

fn connect() -> std::io::Result<UnixStream> {
    let s = UnixStream::connect(paths::socket_path())?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
    let mut w = s.try_clone()?;
    at_proto::write_msg(
        &mut w,
        &ClientMsg::Hello {
            version: PROTO_VERSION,
            kind: ClientKind::Cli,
            pid: std::process::id(),
        },
    )?;
    Ok(s)
}

fn control(msg: ClientMsg) -> std::io::Result<()> {
    let s = connect()?;
    let mut w = s.try_clone()?;
    at_proto::write_msg(&mut w, &msg)?;
    // Wait until the daemon closes the connection (shutdown completes).
    let mut r = BufReader::new(s);
    while at_proto::read_msg::<_, ServerMsg>(&mut r).is_ok() {}
    Ok(())
}

fn status() -> std::io::Result<()> {
    let s = connect()?;
    let mut r = BufReader::new(s);
    loop {
        match at_proto::read_msg::<_, ServerMsg>(&mut r)? {
            ServerMsg::Welcome {
                workspace,
                panes,
                gui_clients,
                ..
            } => {
                let v = at_daemon::server::describe(&workspace, &panes);
                println!(
                    "running: {} windows, {} panes, {} GUI clients",
                    workspace.windows.len(),
                    panes.len(),
                    gui_clients
                );
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                return Ok(());
            }
            ServerMsg::Error(e) => return Err(std::io::Error::other(e)),
            _ => {}
        }
    }
}
