//! On-disk session snapshot (§4.8), written by the daemon to `session.json`.

use crate::model::{Layout, TabNode, WindowGeom, WindowNode, Workspace};
use crate::pane::PaneInfo;
use crate::{PaneId, TabId, WindowId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// 3: top-level tabs are groups; older snapshots are migrated on load.
pub const SCHEMA: u32 = 3;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub schema: u32,
    pub saved_at: String,
    #[serde(default)]
    pub saved_at_ms: u64,
    pub windows: Vec<WindowSnap>,
    #[serde(default)]
    pub detached_tabs: Vec<TabSnap>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowSnap {
    pub id: WindowId,
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default)]
    pub w: f32,
    #[serde(default)]
    pub h: f32,
    #[serde(default)]
    pub maximized: bool,
    pub sidebar_width: f32,
    #[serde(default)]
    pub sidebar_hidden: bool,
    pub active_tab: Option<TabId>,
    pub tabs: Vec<TabSnap>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TabSnap {
    pub id: TabId,
    #[serde(default)]
    pub custom_title: Option<String>,
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub layout: Layout,
    #[serde(default)]
    pub sizes: Vec<f32>,
    #[serde(default = "half")]
    pub grid_rows: [f32; 2],
    #[serde(default = "half")]
    pub grid_cols: [f32; 2],
    pub active_pane: PaneId,
    pub panes: Vec<PaneSnap>,
    #[serde(default)]
    pub children: Vec<TabSnap>,
    #[serde(default)]
    pub group_view: bool,
    #[serde(default)]
    pub group_sizes: Vec<f32>,
    #[serde(default)]
    pub active_child: Option<TabId>,
}

fn default_profile() -> String {
    "default".into()
}

fn half() -> [f32; 2] {
    [0.5, 0.5]
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RemoteSnap {
    pub host: String,
    pub command: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaneSnap {
    pub id: PaneId,
    pub cwd: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default = "one")]
    pub font_scale: f32,
    #[serde(default)]
    pub remote: Option<RemoteSnap>,
    #[serde(default)]
    pub last_state: String,
    #[serde(default)]
    pub scrollback_file: Option<String>,
}

fn one() -> f32 {
    1.0
}

fn tab_to_snap(t: &TabNode, panes: &HashMap<PaneId, PaneInfo>) -> TabSnap {
    TabSnap {
        id: t.id,
        custom_title: t.custom_title.clone(),
        profile: t.profile.clone(),
        collapsed: t.collapsed,
        layout: t.layout,
        sizes: t.sizes.clone(),
        grid_rows: t.grid_rows,
        grid_cols: t.grid_cols,
        active_pane: t.active_pane,
        panes: t
            .panes
            .iter()
            .map(|p| {
                let info = panes.get(p);
                PaneSnap {
                    id: *p,
                    cwd: info.and_then(|i| i.cwd.clone()),
                    title: info.map(|i| i.auto_title()).unwrap_or_default(),
                    font_scale: info.map(|i| i.font_scale).unwrap_or(1.0),
                    remote: info.and_then(|i| {
                        i.remote_host.clone().map(|host| RemoteSnap {
                            host,
                            command: i.remote_command.clone(),
                        })
                    }),
                    last_state: info
                        .map(|i| format!("{:?}", i.state()).to_lowercase())
                        .unwrap_or_default(),
                    scrollback_file: Some(format!("scrollback/{p}.txt")),
                }
            })
            .collect(),
        children: t.children.iter().map(|c| tab_to_snap(c, panes)).collect(),
        group_view: t.group_view,
        group_sizes: t.group_sizes.clone(),
        active_child: t.active_child,
    }
}

impl Session {
    pub fn capture(ws: &Workspace, panes: &HashMap<PaneId, PaneInfo>, now_ms: u64) -> Session {
        Session {
            schema: SCHEMA,
            saved_at: crate::time::rfc3339_utc(now_ms),
            saved_at_ms: now_ms,
            windows: ws
                .windows
                .iter()
                .map(|w| {
                    let g = w.geom.clone().unwrap_or_default();
                    WindowSnap {
                        id: w.id,
                        x: g.x,
                        y: g.y,
                        w: g.w,
                        h: g.h,
                        maximized: g.maximized,
                        sidebar_width: w.sidebar_width,
                        sidebar_hidden: w.sidebar_hidden,
                        active_tab: w.active_tab,
                        tabs: w.tabs.iter().map(|t| tab_to_snap(t, panes)).collect(),
                    }
                })
                .collect(),
            detached_tabs: ws.detached.iter().map(|t| tab_to_snap(t, panes)).collect(),
        }
    }

    /// Rebuilds the workspace tree; pane metadata is returned separately.
    pub fn restore(&self) -> (Workspace, Vec<PaneSnap>) {
        fn tab(t: &TabSnap, out: &mut Vec<PaneSnap>) -> TabNode {
            out.extend(t.panes.iter().cloned());
            let mut n = TabNode {
                id: t.id,
                custom_title: t.custom_title.clone(),
                profile: t.profile.clone(),
                collapsed: t.collapsed,
                layout: t.layout,
                sizes: t.sizes.clone(),
                grid_rows: t.grid_rows,
                grid_cols: t.grid_cols,
                panes: t.panes.iter().map(|p| p.id).collect(),
                active_pane: t.active_pane,
                zoomed: false,
                children: t.children.iter().map(|c| tab(c, out)).collect(),
                group_view: t.group_view,
                group_sizes: t.group_sizes.clone(),
                active_child: t.active_child,
            };
            n.sanitize();
            n
        }
        let mut panes = Vec::new();
        let mut ws = Workspace {
            windows: self
                .windows
                .iter()
                .map(|w| WindowNode {
                    id: w.id,
                    geom: (w.w > 0.0 && w.h > 0.0).then_some(WindowGeom {
                        x: w.x,
                        y: w.y,
                        w: w.w,
                        h: w.h,
                        maximized: w.maximized,
                    }),
                    sidebar_width: w.sidebar_width,
                    sidebar_hidden: w.sidebar_hidden,
                    active_tab: w.active_tab,
                    tabs: w.tabs.iter().map(|t| tab(t, &mut panes)).collect(),
                })
                .filter(|w| !w.tabs.is_empty())
                .collect(),
            detached: self
                .detached_tabs
                .iter()
                .map(|t| tab(t, &mut panes))
                .collect(),
            focused_window: None,
        };
        ws.sanitize();
        (ws, panes)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("session serializes")
    }

    pub fn load(path: &Path) -> Result<Session, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let s: Session = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        if s.schema > SCHEMA {
            return Err(format!(
                "schema {} is newer than supported {}",
                s.schema, SCHEMA
            ));
        }
        Ok(s)
    }

    /// Writes `session.json`, keeping the previous good copy as `session.json.bak`.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if path.exists() {
            let bak = path.with_extension("json.bak");
            if Session::load(path).is_ok() {
                let _ = std::fs::copy(path, &bak);
            }
        }
        crate::paths::write_atomic(path, self.to_json().as_bytes())
    }

    /// `session.json`, falling back to `.bak`. The error lists what failed.
    pub fn load_with_backup(path: &Path) -> Result<(Session, bool), String> {
        match Session::load(path) {
            Ok(s) => Ok((s, false)),
            Err(e1) => {
                let bak = path.with_extension("json.bak");
                match Session::load(&bak) {
                    Ok(s) => Ok((s, true)),
                    Err(e2) => Err(format!("{}: {e1}; {}: {e2}", path.display(), bak.display())),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::WindowNode;

    #[test]
    fn capture_restore_roundtrip() {
        let mut ws = Workspace::default();
        let w = WindowNode::new(240.0);
        let wid = w.id;
        ws.windows.push(w);
        let mut t = TabNode::new(PaneId::new(), "default");
        t.add_pane(PaneId::new());
        t.custom_title = Some("work".into());
        let tid = t.id;
        ws.insert_after(wid, None, t);
        ws.insert_child(wid, tid, TabNode::new(PaneId::new(), "prod"));
        let gid = ws.toggle_group_view(tid).unwrap();
        ws.set_group_sizes(gid, vec![0.7, 0.3]);
        let mut infos = HashMap::new();
        for p in ws.all_panes() {
            let mut i = PaneInfo::new(p, "default");
            i.cwd = Some("/tmp".into());
            infos.insert(p, i);
        }
        let first = ws.all_panes()[0];
        infos.get_mut(&first).unwrap().remote_host = Some("srv-01".into());
        let s = Session::capture(&ws, &infos, 1000);
        let json = s.to_json();
        let back: Session = serde_json::from_str(&json).unwrap();
        let (ws2, panes) = back.restore();
        assert_eq!(ws2.windows.len(), 1);
        assert_eq!(ws2.preorder_tabs(wid), ws.preorder_tabs(wid));
        assert_eq!(panes.len(), 3);
        assert_eq!(panes[0].remote.as_ref().unwrap().host, "srv-01");
        assert_eq!(ws2.tab(tid).unwrap().custom_title.as_deref(), Some("work"));
        assert!(ws2.tab(gid).unwrap().group_view);
        assert!(ws2.is_group(gid));
        assert_eq!(ws2.group_members(gid).len(), 2);
        assert!((ws2.group_sizes(gid)[0] - 0.7).abs() < 1e-4);
    }

    #[test]
    fn backup_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let s = Session {
            schema: SCHEMA,
            saved_at: String::new(),
            saved_at_ms: 0,
            windows: vec![],
            detached_tabs: vec![],
        };
        s.save(&path).unwrap();
        s.save(&path).unwrap();
        std::fs::write(&path, "{broken").unwrap();
        let (_, from_bak) = Session::load_with_backup(&path).unwrap();
        assert!(from_bak);
    }
}
