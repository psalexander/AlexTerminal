//! Connection to the session daemon. Starts `alexterminal-server` when it is not
//! running, or runs the server inside this process (`--no-daemon`).

use at_core::paths;
use at_proto::{ClientKind, ClientMsg, PROTO_VERSION, ServerMsg, read_msg, write_msg};
use crossbeam_channel::{Receiver, Sender};
use std::io::{BufReader, BufWriter, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub struct Connection {
    pub tx: Sender<ClientMsg>,
    pub rx: Receiver<ServerMsg>,
    pub in_process: bool,
}

/// Path of `alexterminal-server` next to the running executable, else `$PATH`.
pub fn server_exe() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let exe = exe.canonicalize().unwrap_or(exe);
        if let Some(dir) = exe.parent() {
            let p = dir.join("alexterminal-server");
            if p.exists() {
                return p;
            }
        }
    }
    PathBuf::from("alexterminal-server")
}

fn wire(stream: UnixStream, kind: ClientKind) -> std::io::Result<Connection> {
    let (ctx, crx) = crossbeam_channel::unbounded::<ClientMsg>();
    let (stx, srx) = crossbeam_channel::unbounded::<ServerMsg>();
    let read_half = stream.try_clone()?;
    {
        let mut w = &stream;
        write_msg(
            &mut w,
            &ClientMsg::Hello {
                version: PROTO_VERSION,
                kind,
                pid: std::process::id(),
            },
        )?;
    }
    std::thread::Builder::new()
        .name("conn-w".into())
        .spawn(move || {
            let mut w = BufWriter::new(stream);
            while let Ok(first) = crx.recv() {
                let mut ok = write_msg(&mut w, &first).is_ok();
                while ok {
                    match crx.try_recv() {
                        Ok(m) => ok = write_msg(&mut w, &m).is_ok(),
                        Err(_) => break,
                    }
                }
                if !ok || w.flush().is_err() {
                    break;
                }
            }
            let _ = w.get_ref().shutdown(std::net::Shutdown::Write);
        })?;
    std::thread::Builder::new()
        .name("conn-r".into())
        .spawn(move || {
            let mut r = BufReader::with_capacity(256 * 1024, read_half);
            while let Ok(m) = read_msg::<_, ServerMsg>(&mut r) {
                if stx.send(m).is_err() {
                    break;
                }
            }
        })?;
    Ok(Connection {
        tx: ctx,
        rx: srx,
        in_process: false,
    })
}

/// Connects to a running daemon without starting one.
pub fn connect_existing(kind: ClientKind) -> std::io::Result<Connection> {
    let s = UnixStream::connect(paths::socket_path())?;
    wire(s, kind)
}

/// Connects, starting the daemon if needed (waits up to 5 s for its socket).
pub fn connect(kind: ClientKind) -> anyhow::Result<Connection> {
    if let Ok(c) = connect_existing(kind) {
        return Ok(c);
    }
    let exe = server_exe();
    tracing::info!("starting {}", exe.display());
    // `--daemon` forks; the parent exits right away, so `status()` does not block.
    let status = std::process::Command::new(&exe)
        .arg("--daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| anyhow::anyhow!("cannot start {}: {e}", exe.display()))?;
    if !status.success() {
        anyhow::bail!("{} exited with {status}", exe.display());
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match connect_existing(kind) {
            Ok(c) => return Ok(c),
            Err(e) if Instant::now() >= deadline => anyhow::bail!("daemon did not come up: {e}"),
            Err(_) => std::thread::sleep(Duration::from_millis(40)),
        }
    }
}

/// Waits for the server's first answer to `Hello`.
fn first_reply(c: &Connection) -> Result<ServerMsg, String> {
    match c.rx.recv_timeout(Duration::from_secs(5)) {
        Ok(m) => Ok(m),
        Err(crossbeam_channel::RecvTimeoutError::Timeout) => Err("no answer from server".into()),
        Err(_) => Err("server closed the connection".into()),
    }
}

/// Asks an outdated server to save its session and exit, then waits until it is gone.
fn retire(c: &Connection) {
    let _ = c.tx.send(ClientMsg::Shutdown);
    let deadline = Instant::now() + Duration::from_secs(15);
    // The server closes every connection when it is done.
    while Instant::now() < deadline {
        if let Err(crossbeam_channel::RecvTimeoutError::Disconnected) =
            c.rx.recv_timeout(Duration::from_millis(200))
        {
            break;
        }
    }
    while Instant::now() < deadline
        && std::os::unix::net::UnixStream::connect(paths::socket_path()).is_ok()
    {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Connects (starting the daemon if needed) and returns the connection with the
/// server's `Welcome`. An older server is stopped (its session is saved) and replaced
/// by the one shipped with this client, which restores the session.
pub fn connect_checked(kind: ClientKind) -> anyhow::Result<(Connection, ServerMsg)> {
    let mut last_error = String::new();
    for _ in 0..3 {
        let c = connect(kind)?;
        match first_reply(&c) {
            Ok(m @ ServerMsg::Welcome { .. }) => return Ok((c, m)),
            Ok(ServerMsg::Error(e)) => match at_proto::mismatch_server_version(&e) {
                Some(v) if v < PROTO_VERSION => {
                    tracing::warn!(
                        "server protocol {v} is older than {PROTO_VERSION}; restarting it"
                    );
                    retire(&c);
                    last_error = e;
                }
                _ => anyhow::bail!(e),
            },
            Ok(other) => {
                last_error = format!("unexpected first message: {other:?}");
            }
            Err(e) => {
                // A very old server may drop us without an answer: stop it the same way.
                tracing::warn!("{e}; restarting the server");
                if let Ok(c2) = connect_existing(ClientKind::Cli) {
                    retire(&c2);
                }
                last_error = e;
            }
        }
    }
    anyhow::bail!("cannot reach a compatible session server: {last_error}")
}

/// Runs the server on a thread of this process (`--no-daemon`). Sessions end with the process.
pub fn in_process(restore: bool, restore_from: Option<PathBuf>) -> Connection {
    use at_daemon::server::{Server, ServerOptions};
    let (tx, rx) = crossbeam_channel::unbounded();
    let (ctx, srx) = at_daemon::transport::connect_in_process(&tx);
    let opts = ServerOptions {
        restore,
        restore_from,
        exit_when_empty: false,
        ..Default::default()
    };
    std::thread::Builder::new()
        .name("server".into())
        .spawn(move || {
            let server = Server::new(rx, opts);
            drop(tx);
            server.run();
        })
        .expect("spawn in-process server");
    let _ = ctx.send(ClientMsg::Hello {
        version: PROTO_VERSION,
        kind: ClientKind::Gui,
        pid: std::process::id(),
    });
    Connection {
        tx: ctx,
        rx: srx,
        in_process: true,
    }
}
