//! Client transports: channels for the app's own windows and a Unix socket through
//! which a second `alexterminal` launch or `alexterminal list` reaches the running app.

use crate::server::{ClientEvent, ClientId};
use at_proto::{ClientMsg, ServerMsg, read_msg};
use crossbeam_channel::{Receiver, Sender};
use std::io::{BufReader, BufWriter, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_client_id() -> ClientId {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Single-instance lock held while the app runs.
pub struct InstanceLock {
    _file: std::fs::File,
}

impl InstanceLock {
    pub fn acquire(path: &Path) -> std::io::Result<InstanceLock> {
        use std::os::unix::io::AsRawFd;
        if let Some(p) = path.parent() {
            at_core::paths::ensure_private_dir(p)?;
        }
        let f = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        let r = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if r != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "another AlexTerminal is running",
            ));
        }
        let _ = std::fs::write(path, std::process::id().to_string());
        Ok(InstanceLock { _file: f })
    }
}

/// Binds the instance socket (`0600`). A stale socket file is replaced.
pub fn bind(path: &Path) -> std::io::Result<UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(p) = path.parent() {
        at_core::paths::ensure_private_dir(p)?;
    }
    if path.exists() {
        if UnixStream::connect(path).is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "socket is in use",
            ));
        }
        std::fs::remove_file(path)?;
    }
    let l = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(l)
}

/// Accepts connections forever, wiring each one to the server.
pub fn serve(listener: UnixListener, to_server: Sender<(ClientId, ClientEvent)>) {
    std::thread::Builder::new()
        .name("accept".into())
        .spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(s) => attach_stream(s, to_server.clone()),
                    Err(e) => {
                        tracing::warn!("accept: {e}");
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                }
            }
        })
        .expect("spawn accept thread");
}

fn attach_stream(stream: UnixStream, to_server: Sender<(ClientId, ClientEvent)>) {
    let cid = next_client_id();
    let (tx, rx) = crossbeam_channel::unbounded::<ServerMsg>();
    let _ = to_server.send((cid, ClientEvent::Connected(tx)));
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    // Writer: batches queued messages into one flush.
    std::thread::Builder::new()
        .name(format!("client-{cid}-w"))
        .spawn(move || {
            let mut w = BufWriter::with_capacity(256 * 1024, stream);
            while let Ok(first) = rx.recv() {
                let mut ok = at_proto::write_msg(&mut w, &first).is_ok();
                while ok {
                    match rx.try_recv() {
                        Ok(m) => ok = at_proto::write_msg(&mut w, &m).is_ok(),
                        Err(_) => break,
                    }
                }
                if !ok || w.flush().is_err() {
                    break;
                }
            }
            let _ = w.get_ref().shutdown(std::net::Shutdown::Both);
        })
        .expect("spawn writer");
    std::thread::Builder::new()
        .name(format!("client-{cid}-r"))
        .spawn(move || {
            let mut r = BufReader::with_capacity(64 * 1024, read_half);
            while let Ok(m) = read_msg::<_, ClientMsg>(&mut r) {
                if to_server.send((cid, ClientEvent::Msg(m))).is_err() {
                    break;
                }
            }
            let _ = to_server.send((cid, ClientEvent::Disconnected));
        })
        .expect("spawn reader");
}

/// In-process connection: returns the client's ends of two channels.
pub fn connect_in_process(
    to_server: &Sender<(ClientId, ClientEvent)>,
) -> (Sender<ClientMsg>, Receiver<ServerMsg>) {
    let cid = next_client_id();
    let (stx, srx) = crossbeam_channel::unbounded::<ServerMsg>();
    let (ctx, crx) = crossbeam_channel::unbounded::<ClientMsg>();
    let _ = to_server.send((cid, ClientEvent::Connected(stx)));
    let to_server = to_server.clone();
    std::thread::Builder::new()
        .name(format!("inproc-{cid}"))
        .spawn(move || {
            while let Ok(m) = crx.recv() {
                if to_server.send((cid, ClientEvent::Msg(m))).is_err() {
                    return;
                }
            }
            let _ = to_server.send((cid, ClientEvent::Disconnected));
        })
        .expect("spawn inproc forwarder");
    (ctx, srx)
}
