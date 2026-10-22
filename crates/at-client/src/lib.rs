//! Session server for the app. The server runs on a thread of the `alexterminal`
//! process, so sessions end with it. It also listens on a Unix socket: a second
//! launch hands its request to the running app there, and `alexterminal list` reads it.

use at_core::paths;
use at_daemon::server::{Server, ServerOptions};
use at_daemon::transport;
use at_proto::{ClientKind, ClientMsg, PROTO_VERSION, ServerMsg, read_msg, write_msg};
use crossbeam_channel::{Receiver, Sender};
use std::io::{BufReader, BufWriter, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub struct Connection {
    pub tx: Sender<ClientMsg>,
    pub rx: Receiver<ServerMsg>,
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
    Ok(Connection { tx: ctx, rx: srx })
}

/// Connects to the running app's socket.
pub fn connect_existing(kind: ClientKind) -> std::io::Result<Connection> {
    let s = UnixStream::connect(paths::socket_path())?;
    wire(s, kind)
}

/// Waits for the server's first answer to `Hello`.
fn first_reply(c: &Connection) -> Result<ServerMsg, String> {
    match c.rx.recv_timeout(Duration::from_secs(5)) {
        Ok(m) => Ok(m),
        Err(crossbeam_channel::RecvTimeoutError::Timeout) => Err("no answer from server".into()),
        Err(_) => Err("server closed the connection".into()),
    }
}

/// Asks a server to save its session and exit, then waits until it is gone.
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
    while Instant::now() < deadline && UnixStream::connect(paths::socket_path()).is_ok() {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Finds a running AlexTerminal and returns the connection to it with its `Welcome`.
///
/// A standalone `alexterminal-server` left by a version before 0.3 (no window attached,
/// or an older protocol) is stopped; it saves its session, which this launch restores.
pub fn find_running() -> anyhow::Result<Option<(Connection, ServerMsg)>> {
    let Ok(c) = connect_existing(ClientKind::Gui) else {
        return Ok(None);
    };
    match first_reply(&c) {
        Ok(m @ ServerMsg::Welcome { .. }) if matches!(m, ServerMsg::Welcome { gui_clients, .. } if gui_clients > 0) => {
            Ok(Some((c, m)))
        }
        Ok(ServerMsg::Welcome { .. }) => {
            tracing::warn!("stopping a standalone session server from an older version");
            retire(&c);
            Ok(None)
        }
        Ok(ServerMsg::Error(e)) => match at_proto::mismatch_server_version(&e) {
            Some(v) if v < PROTO_VERSION => {
                tracing::warn!("server protocol {v} is older than {PROTO_VERSION}; stopping it");
                retire(&c);
                Ok(None)
            }
            _ => anyhow::bail!("another AlexTerminal is running: {e}"),
        },
        Ok(other) => anyhow::bail!("unexpected first message: {other:?}"),
        Err(e) => {
            // A very old server may drop us without an answer: stop it the same way.
            tracing::warn!("{e}; stopping the old server");
            if let Ok(c2) = connect_existing(ClientKind::Cli) {
                retire(&c2);
            }
            Ok(None)
        }
    }
}

/// Starts the session server on a thread of this process and connects the app to it.
/// Sessions end with the process.
pub fn start(restore: bool, restore_from: Option<PathBuf>) -> anyhow::Result<Connection> {
    // A server stopped by `find_running` may still be releasing its lock.
    let deadline = Instant::now() + Duration::from_secs(3);
    let lock = loop {
        match transport::InstanceLock::acquire(&paths::lock_path()) {
            Ok(l) => break l,
            Err(e) if Instant::now() >= deadline => return Err(e.into()),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    let listener = transport::bind(&paths::socket_path())?;
    let (tx, rx) = crossbeam_channel::unbounded();
    transport::serve(listener, tx.clone());
    let (ctx, srx) = transport::connect_in_process(&tx);
    let opts = ServerOptions {
        restore,
        restore_from,
        exit_when_empty: false,
    };
    std::thread::Builder::new()
        .name("server".into())
        .spawn(move || {
            Server::new(rx, opts).run();
            drop(lock);
            let _ = std::fs::remove_file(paths::lock_path());
        })?;
    drop(tx);
    let _ = ctx.send(ClientMsg::Hello {
        version: PROTO_VERSION,
        kind: ClientKind::Gui,
        pid: std::process::id(),
    });
    Ok(Connection { tx: ctx, rx: srx })
}
