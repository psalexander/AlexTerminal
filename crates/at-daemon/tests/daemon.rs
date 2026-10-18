//! End-to-end daemon tests with an in-process client and an isolated home.

use at_core::PaneId;
use at_core::model::Direction;
use at_daemon::server::{Server, ServerOptions};
use at_daemon::transport;
use at_proto::*;
use crossbeam_channel::{Receiver, Sender};
use std::time::{Duration, Instant};

struct Client {
    tx: Sender<ClientMsg>,
    rx: Receiver<ServerMsg>,
    screens: std::collections::HashMap<PaneId, Vec<Row>>,
    ws: at_core::model::Workspace,
    infos: std::collections::HashMap<PaneId, at_core::pane::PaneInfo>,
}

impl Client {
    fn pump(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while let Ok(m) = self.rx.recv_deadline(deadline) {
            self.apply(m);
            if Instant::now() >= deadline {
                break;
            }
        }
    }

    fn apply(&mut self, m: ServerMsg) {
        match m {
            ServerMsg::Welcome {
                workspace, panes, ..
            } => {
                self.ws = workspace;
                for p in panes {
                    self.infos.insert(p.id, p);
                }
            }
            ServerMsg::Workspace(w) => self.ws = w,
            ServerMsg::Pane(p) => {
                self.infos.insert(p.id, p);
            }
            ServerMsg::PaneRemoved(id) => {
                self.infos.remove(&id);
            }
            ServerMsg::Frame(f) => {
                let rows = self.screens.entry(f.pane).or_default();
                if f.full || rows.len() != f.rows as usize {
                    *rows = vec![Row::default(); f.rows as usize];
                }
                for (i, r) in f.lines {
                    if (i as usize) < rows.len() {
                        rows[i as usize] = r;
                    }
                }
            }
            _ => {}
        }
    }

    fn screen(&self, p: PaneId) -> String {
        self.screens
            .get(&p)
            .map(|rows| {
                rows.iter()
                    .map(|r| r.text().trim_end().to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    }

    fn wait<F: Fn(&Client) -> bool>(&mut self, what: &str, f: F) {
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if f(self) {
                return;
            }
            self.pump(Duration::from_millis(50));
        }
        let screens: Vec<String> = self
            .screens
            .keys()
            .map(|k| format!("{k:?}:\n{}", self.screen(*k)))
            .collect();
        panic!(
            "timeout waiting for {what}; workspace={:?}\nscreens:\n{}",
            self.ws,
            screens.join("\n----\n")
        );
    }

    fn send(&self, m: ClientMsg) {
        self.tx.send(m).unwrap();
    }
}

fn start(restore: bool) -> (std::thread::JoinHandle<()>, Client) {
    let (tx, rx) = crossbeam_channel::unbounded();
    let (ctx, crx) = transport::connect_in_process(&tx);
    let opts = ServerOptions {
        restore,
        ..Default::default()
    };
    let h = std::thread::spawn(move || {
        let s = Server::new(rx, opts);
        drop(tx);
        s.run();
    });
    let c = Client {
        tx: ctx,
        rx: crx,
        screens: Default::default(),
        ws: Default::default(),
        infos: Default::default(),
    };
    c.send(ClientMsg::Hello {
        version: PROTO_VERSION,
        kind: ClientKind::Gui,
        pid: 1,
    });
    (h, c)
}

#[test]
fn lifecycle_snapshot_restore() {
    let home = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("ALEXTERMINAL_HOME", home.path());
        std::env::set_var("SHELL", "/bin/sh");
        std::env::set_var("PS1", "$ ");
    }
    std::fs::create_dir_all(home.path().join("config")).unwrap();
    std::fs::write(
        home.path().join("config/config.toml"),
        "[terminal]\nshell = \"/bin/sh\"\n[shell_integration]\nmode = \"off\"\n[session]\nreconnect_remote = \"ask\"\n",
    )
    .unwrap();

    let (h, mut c) = start(true);
    c.wait("welcome", |_| true);
    let work = home.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    c.send(ClientMsg::Cmd(Cmd::NewWindow {
        cwd: Some(work.display().to_string()),
        profile: None,
        command: None,
        geom: None,
    }));
    c.wait("window", |c| c.ws.windows.len() == 1);
    let wid = c.ws.windows[0].id;
    let tab = c.ws.windows[0].active_tab.unwrap();
    // A new window starts with one group holding one terminal tab.
    let group = c.ws.group_of(tab).unwrap();
    assert!(c.ws.is_group(group));
    assert_ne!(group, tab);
    let p1 = c.ws.tab(tab).unwrap().panes[0];
    c.send(ClientMsg::Visible { panes: vec![p1] });
    c.send(ClientMsg::Resize {
        pane: p1,
        cols: 60,
        rows: 12,
        cell_w: 8,
        cell_h: 16,
    });
    c.send(ClientMsg::Text {
        pane: p1,
        text: "echo marker-$((40+2))\r".into(),
    });
    c.wait("echo output", |c| c.screen(p1).contains("marker-42"));
    assert!(c.infos[&p1].cwd.as_deref().unwrap().ends_with("work"));

    // Nested tab, second pane, grid toggle requires 4 panes.
    c.send(ClientMsg::Cmd(Cmd::NewTab {
        window: wid,
        nested: true,
        profile: None,
        cwd: None,
        command: None,
    }));
    c.wait("nested tab", |c| c.ws.terminal_tabs(wid).len() == 2);
    let child = c.ws.windows[0].active_tab.unwrap();
    assert_eq!(c.ws.parent(child), Some(group));
    // A plain new tab is a new group; closing its only tab removes the group.
    c.send(ClientMsg::Cmd(Cmd::NewTab {
        window: wid,
        nested: false,
        profile: None,
        cwd: None,
        command: None,
    }));
    c.wait("second group", |c| c.ws.windows[0].tabs.len() == 2);
    let lone = c.ws.windows[0].active_tab.unwrap();
    c.send(ClientMsg::Cmd(Cmd::CloseTab {
        tab: lone,
        with_children: false,
    }));
    c.wait("lone group closed", |c| c.ws.windows[0].tabs.len() == 1);
    c.send(ClientMsg::Cmd(Cmd::SelectTab { tab }));
    c.send(ClientMsg::Cmd(Cmd::NewPane { tab }));
    c.wait("second pane", |c| c.ws.tab(tab).unwrap().panes.len() == 2);
    c.send(ClientMsg::Cmd(Cmd::FocusDirection {
        tab,
        dir: Direction::Up,
    }));
    c.send(ClientMsg::Cmd(Cmd::RenameTab {
        tab,
        title: Some("work".into()),
    }));
    c.wait("rename", |c| {
        c.ws.tab(tab).unwrap().custom_title.as_deref() == Some("work")
    });

    // Running indicator via the foreground process group.
    c.send(ClientMsg::Text {
        pane: p1,
        text: "sleep 2\r".into(),
    });
    c.wait("running", |c| {
        c.infos.get(&p1).map(|i| i.running).unwrap_or(false)
    });
    c.wait("idle again", |c| {
        c.infos.get(&p1).map(|i| !i.running).unwrap_or(false)
    });

    // Search shows up in frames.
    c.send(ClientMsg::Search {
        pane: p1,
        query: "marker".into(),
        regex: false,
        forward: true,
    });
    c.pump(Duration::from_millis(200));

    // Detach the GUI and stop the server: snapshot + scrollback are written.
    c.send(ClientMsg::Detach);
    c.send(ClientMsg::Shutdown);
    h.join().unwrap();
    let session = std::fs::read_to_string(home.path().join("data/session.json")).unwrap();
    assert!(session.contains("\"custom_title\": \"work\""));
    let sb =
        std::fs::read_to_string(home.path().join(format!("data/scrollback/{p1}.txt"))).unwrap();
    assert!(sb.contains("marker-42"), "{sb}");

    // Restart: the tree comes back with the same ids, scrollback is replayed.
    let (h, mut c) = start(true);
    c.wait("restored window", |c| c.ws.windows.len() == 1);
    assert_eq!(c.ws.terminal_tabs(wid), vec![tab, child]);
    assert_eq!(c.ws.group_of(child), Some(group));
    assert_eq!(c.ws.tab(tab).unwrap().panes.len(), 2);
    c.send(ClientMsg::Visible { panes: vec![p1] });
    c.wait("replayed scrollback", |c| {
        c.screen(p1).contains("marker-42") && c.screen(p1).contains("── ")
    });
    assert!(c.infos[&p1].cwd.as_deref().unwrap().ends_with("work"));

    // Closing a tab keeps the rest of its group; reopen puts it back in place.
    c.send(ClientMsg::Cmd(Cmd::CloseTab {
        tab,
        with_children: false,
    }));
    c.wait("closed", |c| c.ws.tab(tab).is_none());
    assert_eq!(c.ws.group_of(child), Some(group));
    c.send(ClientMsg::Cmd(Cmd::ReopenClosedTab { window: wid }));
    c.wait("reopened", |c| c.ws.tab(tab).is_some());
    assert_eq!(c.ws.group_members(group), vec![tab, child]);

    // Closing a group closes every tab in it and ends their shells.
    c.send(ClientMsg::Cmd(Cmd::NewTab {
        window: wid,
        nested: false,
        profile: None,
        cwd: None,
        command: None,
    }));
    c.wait("new group", |c| c.ws.windows[0].tabs.len() == 2);
    c.send(ClientMsg::Cmd(Cmd::NewTab {
        window: wid,
        nested: true,
        profile: None,
        cwd: None,
        command: None,
    }));
    c.wait("two tabs in new group", |c| {
        c.ws.windows[0].tabs[1].children.len() == 2
    });
    let g2 = c.ws.windows[0].tabs[1].id;
    let doomed: Vec<PaneId> = c.ws.windows[0].tabs[1]
        .children
        .iter()
        .flat_map(|t| t.panes.clone())
        .collect();
    c.send(ClientMsg::Cmd(Cmd::CloseTab {
        tab: g2,
        with_children: false,
    }));
    c.wait("group closed", |c| {
        c.ws.tab(g2).is_none() && doomed.iter().all(|p| !c.infos.contains_key(p))
    });
    assert_eq!(c.ws.windows[0].tabs.len(), 1);

    // Window close in detach mode keeps the tabs alive.
    c.send(ClientMsg::Cmd(Cmd::CloseWindow {
        window: wid,
        mode: CloseWindowMode::Detach,
    }));
    c.wait("detached", |c| {
        c.ws.windows.is_empty() && c.ws.detached.len() == 1
    });

    c.send(ClientMsg::Shutdown);
    h.join().unwrap();
}
