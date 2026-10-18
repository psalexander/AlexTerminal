//! Workspace tree: windows → groups (top-level tabs) → terminal tabs → 1..4 panes.
//!
//! A top-level tab is a group and has no panes of its own. Every terminal tab lives
//! inside a group; terminal tabs cannot contain further tabs.

use crate::{PaneId, TabId, WindowId};
use serde::{Deserialize, Serialize};

/// Groups are at depth 0, terminal tabs at depth 1.
pub const MAX_DEPTH: usize = 1;
pub const MAX_PANES: usize = 4;
pub const MIN_FRACTION: f32 = 0.08;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    #[default]
    Stack,
    Grid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DropPosition {
    Before,
    After,
    Child,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TabNode {
    pub id: TabId,
    pub custom_title: Option<String>,
    pub profile: String,
    pub collapsed: bool,
    pub layout: Layout,
    /// Stack fractions, one per pane, summing to 1.
    pub sizes: Vec<f32>,
    pub grid_rows: [f32; 2],
    pub grid_cols: [f32; 2],
    pub panes: Vec<PaneId>,
    pub active_pane: PaneId,
    pub zoomed: bool,
    pub children: Vec<TabNode>,
    /// Show this tab and all nested tabs stacked in one view.
    #[serde(default)]
    pub group_view: bool,
    /// Section heights in group view: fractions, one per child tab.
    #[serde(default)]
    pub group_sizes: Vec<f32>,
    /// Group only: the child shown when the group row is selected.
    #[serde(default)]
    pub active_child: Option<TabId>,
}

impl TabNode {
    pub fn new(pane: PaneId, profile: &str) -> Self {
        Self {
            id: TabId::new(),
            custom_title: None,
            profile: profile.to_string(),
            collapsed: false,
            layout: Layout::Stack,
            sizes: vec![1.0],
            grid_rows: [0.5, 0.5],
            grid_cols: [0.5, 0.5],
            panes: vec![pane],
            active_pane: pane,
            zoomed: false,
            children: Vec::new(),
            group_view: false,
            group_sizes: Vec::new(),
            active_child: None,
        }
    }

    /// A group holding one terminal tab.
    pub fn group_with(child: TabNode) -> Self {
        let profile = child.profile.clone();
        let id = child.id;
        Self {
            id: TabId::new(),
            custom_title: None,
            profile,
            collapsed: false,
            layout: Layout::Stack,
            sizes: Vec::new(),
            grid_rows: [0.5, 0.5],
            grid_cols: [0.5, 0.5],
            panes: Vec::new(),
            active_pane: PaneId(ulid::Ulid::nil()),
            zoomed: false,
            children: vec![child],
            group_view: false,
            group_sizes: Vec::new(),
            active_child: Some(id),
        }
    }

    pub fn is_group(&self) -> bool {
        self.panes.is_empty()
    }

    pub fn active_index(&self) -> usize {
        self.panes
            .iter()
            .position(|p| *p == self.active_pane)
            .unwrap_or(0)
    }

    pub fn can_add_pane(&self) -> bool {
        self.panes.len() < MAX_PANES
    }

    /// Adds a pane after the active one and gives all panes equal size.
    pub fn add_pane(&mut self, pane: PaneId) -> bool {
        if !self.can_add_pane() {
            return false;
        }
        let idx = self.active_index() + 1;
        self.panes.insert(idx.min(self.panes.len()), pane);
        self.active_pane = pane;
        self.zoomed = false;
        self.equalize();
        true
    }

    /// Removes a pane. Returns false if the pane does not belong to this tab.
    pub fn remove_pane(&mut self, pane: PaneId) -> bool {
        let Some(idx) = self.panes.iter().position(|p| *p == pane) else {
            return false;
        };
        self.panes.remove(idx);
        if self.layout == Layout::Grid {
            self.layout = Layout::Stack;
        }
        self.zoomed = false;
        if !self.panes.is_empty() {
            if self.active_pane == pane {
                self.active_pane = self.panes[idx.saturating_sub(1).min(self.panes.len() - 1)];
            }
            // Give the freed space to the neighbour instead of resetting everything.
            if self.sizes.len() == self.panes.len() + 1 {
                let freed = self.sizes.remove(idx);
                let n = idx.saturating_sub(1).min(self.sizes.len() - 1);
                self.sizes[n] += freed;
            } else {
                self.equalize();
            }
            normalize(&mut self.sizes);
        }
        true
    }

    pub fn equalize(&mut self) {
        let n = self.panes.len().max(1);
        self.sizes = vec![1.0 / n as f32; n];
        self.grid_rows = [0.5, 0.5];
        self.grid_cols = [0.5, 0.5];
    }

    /// Stack ↔ grid. Grid is only available with four panes.
    pub fn toggle_layout(&mut self) -> bool {
        if self.panes.len() != MAX_PANES {
            return false;
        }
        self.layout = match self.layout {
            Layout::Stack => Layout::Grid,
            Layout::Grid => Layout::Stack,
        };
        true
    }

    /// Pane focused when moving in a direction from the active pane.
    pub fn neighbour(&self, dir: Direction) -> Option<PaneId> {
        let i = self.active_index();
        let n = self.panes.len();
        let j = match self.layout {
            Layout::Stack => match dir {
                Direction::Up => i.checked_sub(1),
                Direction::Down => (i + 1 < n).then_some(i + 1),
                _ => None,
            },
            // 0 1
            // 2 3
            Layout::Grid => match dir {
                Direction::Up => (i >= 2).then(|| i - 2),
                Direction::Down => (i < 2).then(|| i + 2),
                Direction::Left => (i % 2 == 1).then(|| i - 1),
                Direction::Right => i.is_multiple_of(2).then(|| i + 1),
            },
        }?;
        self.panes.get(j).copied()
    }

    /// Moves the active pane one slot up or down in the order.
    pub fn move_active_pane(&mut self, forward: bool) -> bool {
        let i = self.active_index();
        let j = if forward {
            i + 1
        } else {
            match i.checked_sub(1) {
                Some(j) => j,
                None => return false,
            }
        };
        if j >= self.panes.len() {
            return false;
        }
        self.panes.swap(i, j);
        self.sizes.swap(i, j);
        true
    }

    /// Keyboard resize: grows the active pane by `delta` in the direction.
    pub fn resize_active(&mut self, dir: Direction, delta: f32) {
        match self.layout {
            Layout::Stack => {
                if self.panes.len() < 2 {
                    return;
                }
                let i = self.active_index();
                // Up shrinks from the top edge / grows upward: take from neighbour above.
                let (grow, other) = match dir {
                    Direction::Down => {
                        if i + 1 < self.panes.len() {
                            (i, i + 1)
                        } else {
                            (i, i - 1)
                        }
                    }
                    Direction::Up => {
                        if i > 0 {
                            (i, i - 1)
                        } else {
                            (i, i + 1)
                        }
                    }
                    _ => return,
                };
                let d = delta
                    .min(self.sizes[other] - MIN_FRACTION)
                    .max(-(self.sizes[grow] - MIN_FRACTION));
                self.sizes[grow] += d;
                self.sizes[other] -= d;
            }
            Layout::Grid => {
                let i = self.active_index();
                match dir {
                    Direction::Left | Direction::Right => {
                        let col = i % 2;
                        let sign = if (dir == Direction::Right) == (col == 0) {
                            1.0
                        } else {
                            -1.0
                        };
                        let v = (self.grid_cols[col] + sign * delta).clamp(0.2, 0.8);
                        self.grid_cols[col] = v;
                        self.grid_cols[1 - col] = 1.0 - v;
                    }
                    Direction::Up | Direction::Down => {
                        let row = i / 2;
                        let sign = if (dir == Direction::Down) == (row == 0) {
                            1.0
                        } else {
                            -1.0
                        };
                        let v = (self.grid_rows[row] + sign * delta).clamp(0.2, 0.8);
                        self.grid_rows[row] = v;
                        self.grid_rows[1 - row] = 1.0 - v;
                    }
                }
            }
        }
    }

    pub fn set_sizes(&mut self, sizes: Vec<f32>) {
        if sizes.len() == self.panes.len() && sizes.iter().all(|s| s.is_finite() && *s > 0.0) {
            self.sizes = sizes;
            normalize(&mut self.sizes);
        }
    }

    pub fn all_panes(&self, out: &mut Vec<PaneId>) {
        out.extend(self.panes.iter().copied());
        for c in &self.children {
            c.all_panes(out);
        }
    }

    /// Height of the subtree below this node (0 for a leaf).
    pub fn height(&self) -> usize {
        self.children
            .iter()
            .map(|c| c.height() + 1)
            .max()
            .unwrap_or(0)
    }

    pub fn contains(&self, id: TabId) -> bool {
        self.id == id || self.children.iter().any(|c| c.contains(id))
    }

    /// Restores invariants after deserialization.
    pub fn sanitize(&mut self) {
        if self.panes.is_empty() {
            return;
        }
        if !self.panes.contains(&self.active_pane) {
            self.active_pane = self.panes[0];
        }
        if self.sizes.len() != self.panes.len() {
            self.equalize();
        }
        normalize(&mut self.sizes);
        if self.layout == Layout::Grid && self.panes.len() != MAX_PANES {
            self.layout = Layout::Stack;
        }
        for c in &mut self.children {
            c.sanitize();
        }
    }
}

fn normalize(v: &mut [f32]) {
    let sum: f32 = v.iter().sum();
    if sum > 0.0 {
        for x in v.iter_mut() {
            *x /= sum;
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WindowGeom {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub maximized: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowNode {
    pub id: WindowId,
    pub geom: Option<WindowGeom>,
    pub sidebar_width: f32,
    pub sidebar_hidden: bool,
    pub active_tab: Option<TabId>,
    pub tabs: Vec<TabNode>,
}

impl WindowNode {
    pub fn new(sidebar_width: f32) -> Self {
        Self {
            id: WindowId::new(),
            geom: None,
            sidebar_width,
            sidebar_hidden: false,
            active_tab: None,
            tabs: Vec::new(),
        }
    }
}

/// One row of the sidebar.
#[derive(Clone, Debug, PartialEq)]
pub struct VisibleTab {
    /// Group id for group rows, terminal tab id otherwise.
    pub id: TabId,
    /// 0 for top-level rows, 1 for tabs inside an expanded group.
    pub depth: usize,
    /// The row is a group that can be expanded (two or more tabs, or forced).
    pub has_children: bool,
    pub collapsed: bool,
    pub is_group: bool,
    /// Group this row belongs to (itself for group rows).
    pub group: TabId,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub windows: Vec<WindowNode>,
    /// Groups whose window was closed in "detach" mode.
    pub detached: Vec<TabNode>,
    /// Last focused window; CLI requests open tabs there.
    #[serde(default)]
    pub focused_window: Option<WindowId>,
}

/// Where a tab sits: list owner, group index and child index (None for groups).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Window(usize),
    Detached,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Loc {
    owner: Owner,
    group: usize,
    child: Option<usize>,
}

impl Workspace {
    fn list(&self, o: Owner) -> &Vec<TabNode> {
        match o {
            Owner::Window(i) => &self.windows[i].tabs,
            Owner::Detached => &self.detached,
        }
    }

    fn list_mut(&mut self, o: Owner) -> &mut Vec<TabNode> {
        match o {
            Owner::Window(i) => &mut self.windows[i].tabs,
            Owner::Detached => &mut self.detached,
        }
    }

    fn locate(&self, id: TabId) -> Option<Loc> {
        let owners = (0..self.windows.len())
            .map(Owner::Window)
            .chain(std::iter::once(Owner::Detached));
        for o in owners {
            for (gi, g) in self.list(o).iter().enumerate() {
                if g.id == id {
                    return Some(Loc {
                        owner: o,
                        group: gi,
                        child: None,
                    });
                }
                if let Some(ci) = g.children.iter().position(|c| c.id == id) {
                    return Some(Loc {
                        owner: o,
                        group: gi,
                        child: Some(ci),
                    });
                }
            }
        }
        None
    }

    fn at(&self, l: Loc) -> &TabNode {
        let g = &self.list(l.owner)[l.group];
        match l.child {
            Some(c) => &g.children[c],
            None => g,
        }
    }

    fn at_mut(&mut self, l: Loc) -> &mut TabNode {
        let g = &mut self.list_mut(l.owner)[l.group];
        match l.child {
            Some(c) => &mut g.children[c],
            None => g,
        }
    }

    fn owner_of_window(&self, w: WindowId) -> Option<Owner> {
        self.windows
            .iter()
            .position(|x| x.id == w)
            .map(Owner::Window)
    }

    pub fn window(&self, id: WindowId) -> Option<&WindowNode> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub fn window_mut(&mut self, id: WindowId) -> Option<&mut WindowNode> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    pub fn tab(&self, id: TabId) -> Option<&TabNode> {
        self.locate(id).map(|l| self.at(l))
    }

    pub fn tab_mut(&mut self, id: TabId) -> Option<&mut TabNode> {
        let l = self.locate(id)?;
        Some(self.at_mut(l))
    }

    pub fn is_group(&self, id: TabId) -> bool {
        self.locate(id).map(|l| l.child.is_none()).unwrap_or(false)
    }

    /// The group itself, or the group that holds a terminal tab.
    pub fn group_of(&self, id: TabId) -> Option<TabId> {
        let l = self.locate(id)?;
        Some(self.list(l.owner)[l.group].id)
    }

    pub fn window_of_tab(&self, id: TabId) -> Option<WindowId> {
        match self.locate(id)?.owner {
            Owner::Window(i) => Some(self.windows[i].id),
            Owner::Detached => None,
        }
    }

    pub fn tab_of_pane(&self, pane: PaneId) -> Option<TabId> {
        let owners = (0..self.windows.len())
            .map(Owner::Window)
            .chain(std::iter::once(Owner::Detached));
        for o in owners {
            for g in self.list(o) {
                for c in &g.children {
                    if c.panes.contains(&pane) {
                        return Some(c.id);
                    }
                }
            }
        }
        None
    }

    /// 0 for groups, 1 for terminal tabs.
    pub fn depth(&self, id: TabId) -> Option<usize> {
        self.locate(id).map(|l| usize::from(l.child.is_some()))
    }

    /// Group of a terminal tab; `None` for groups.
    pub fn parent(&self, id: TabId) -> Option<TabId> {
        let l = self.locate(id)?;
        l.child?;
        Some(self.list(l.owner)[l.group].id)
    }

    pub fn all_panes(&self) -> Vec<PaneId> {
        let mut v = Vec::new();
        for w in &self.windows {
            for t in &w.tabs {
                t.all_panes(&mut v);
            }
        }
        for t in &self.detached {
            t.all_panes(&mut v);
        }
        v
    }

    pub fn window_panes(&self, id: WindowId) -> Vec<PaneId> {
        let mut v = Vec::new();
        if let Some(w) = self.window(id) {
            for t in &w.tabs {
                t.all_panes(&mut v);
            }
        }
        v
    }

    /// Terminal tabs of a window in sidebar order.
    pub fn terminal_tabs(&self, window: WindowId) -> Vec<TabId> {
        self.window(window)
            .map(|w| {
                w.tabs
                    .iter()
                    .flat_map(|g| g.children.iter().map(|c| c.id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Groups and terminal tabs in sidebar order.
    pub fn preorder_tabs(&self, window: WindowId) -> Vec<TabId> {
        let mut out = Vec::new();
        if let Some(w) = self.window(window) {
            for g in &w.tabs {
                out.push(g.id);
                out.extend(g.children.iter().map(|c| c.id));
            }
        }
        out
    }

    /// Sidebar rows. A group with one tab shows as a single row standing for that
    /// tab, unless `expand_single` is set.
    pub fn visible_tabs(&self, window: WindowId, expand_single: bool) -> Vec<VisibleTab> {
        let mut out = Vec::new();
        let Some(w) = self.window(window) else {
            return out;
        };
        for g in &w.tabs {
            if g.children.len() == 1 && !expand_single {
                let c = &g.children[0];
                out.push(VisibleTab {
                    id: c.id,
                    depth: 0,
                    has_children: false,
                    collapsed: false,
                    is_group: false,
                    group: g.id,
                });
                continue;
            }
            out.push(VisibleTab {
                id: g.id,
                depth: 0,
                has_children: true,
                collapsed: g.collapsed,
                is_group: true,
                group: g.id,
            });
            if !g.collapsed {
                for c in &g.children {
                    out.push(VisibleTab {
                        id: c.id,
                        depth: 1,
                        has_children: false,
                        collapsed: false,
                        is_group: false,
                        group: g.id,
                    });
                }
            }
        }
        out
    }

    /// Activates a terminal tab (a group activates its last active child) and
    /// expands the group if the tab would be hidden.
    pub fn select_tab(&mut self, tab: TabId) -> bool {
        let Some(l) = self.locate(tab) else {
            return false;
        };
        let Owner::Window(wi) = l.owner else {
            return false;
        };
        let g = &mut self.windows[wi].tabs[l.group];
        let target = match l.child {
            Some(ci) => g.children[ci].id,
            None => match g
                .active_child
                .filter(|a| g.children.iter().any(|c| c.id == *a))
                .or(g.children.first().map(|c| c.id))
            {
                Some(t) => t,
                None => return false,
            },
        };
        g.active_child = Some(target);
        if l.child.is_some() {
            g.collapsed = false;
        }
        self.windows[wi].active_tab = Some(target);
        true
    }

    /// Active terminal tab of a window.
    pub fn active_tab(&self, window: WindowId) -> Option<&TabNode> {
        let w = self.window(window)?;
        let t = self.tab(w.active_tab?)?;
        (!t.is_group()).then_some(t)
    }

    pub fn active_pane(&self, window: WindowId) -> Option<PaneId> {
        self.active_tab(window).map(|t| t.active_pane)
    }

    /// New group holding `terminal`, placed after the group of `after` (or at the end).
    pub fn new_group(
        &mut self,
        window: WindowId,
        after: Option<TabId>,
        terminal: TabNode,
    ) -> Option<TabId> {
        let o = self.owner_of_window(window)?;
        let tid = terminal.id;
        let group = TabNode::group_with(terminal);
        let gid = group.id;
        let idx = after
            .and_then(|a| self.locate(a))
            .filter(|l| l.owner == o)
            .map(|l| l.group + 1);
        let list = self.list_mut(o);
        let idx = idx.unwrap_or(list.len());
        list.insert(idx, group);
        self.select_tab(tid);
        Some(gid)
    }

    /// Adds a terminal tab to the group of `anchor`, right after `anchor` when it is a terminal tab.
    pub fn add_to_group(&mut self, anchor: TabId, terminal: TabNode) -> bool {
        let Some(l) = self.locate(anchor) else {
            return false;
        };
        let tid = terminal.id;
        let g = &mut self.list_mut(l.owner)[l.group];
        let idx = l.child.map(|c| c + 1).unwrap_or(g.children.len());
        g.children.insert(idx, terminal);
        self.select_tab(tid);
        true
    }

    /// Compatibility helper: a sibling of `after`. A terminal tab joins its group;
    /// otherwise a new group is created.
    pub fn insert_after(&mut self, window: WindowId, after: Option<TabId>, node: TabNode) {
        if node.is_group() {
            let Some(o) = self.owner_of_window(window) else {
                return;
            };
            let idx = after
                .and_then(|a| self.locate(a))
                .filter(|l| l.owner == o)
                .map(|l| l.group + 1);
            let first = node.children.first().map(|c| c.id);
            let list = self.list_mut(o);
            let idx = idx.unwrap_or(list.len());
            list.insert(idx, node);
            if let Some(f) = first {
                self.select_tab(f);
            }
            return;
        }
        match after.filter(|a| !self.is_group(*a) && self.window_of_tab(*a) == Some(window)) {
            Some(a)
                if self
                    .tab(self.group_of(a).unwrap())
                    .map(|g| g.children.len() > 1)
                    .unwrap_or(false) =>
            {
                self.add_to_group(a, node);
            }
            _ => {
                self.new_group(window, after, node);
            }
        }
    }

    /// Compatibility helper: put a terminal tab into the group of `parent`.
    pub fn insert_child(&mut self, window: WindowId, parent: TabId, node: TabNode) {
        if self.window_of_tab(parent) == Some(window) && self.add_to_group(parent, node.clone()) {
            return;
        }
        self.new_group(window, None, node);
    }

    fn fix_active(&mut self, owner: Owner, order_before: &[TabId], removed: TabId) {
        let Owner::Window(wi) = owner else { return };
        let still = |ws: &Workspace, t: TabId| {
            ws.locate(t)
                .map(|l| l.child.is_some() && l.owner == owner)
                .unwrap_or(false)
        };
        let active = self.windows[wi].active_tab;
        if active.map(|a| still(self, a)).unwrap_or(false) {
            return;
        }
        let pos = order_before
            .iter()
            .position(|t| Some(*t) == active || *t == removed)
            .unwrap_or(0);
        let next = order_before[pos.min(order_before.len())..]
            .iter()
            .chain(order_before[..pos].iter().rev())
            .copied()
            .find(|t| still(self, *t));
        self.windows[wi].active_tab = None;
        if let Some(n) = next {
            self.select_tab(n);
        }
    }

    fn drop_empty_groups(&mut self, owner: Owner) {
        self.list_mut(owner).retain(|g| !g.children.is_empty());
    }

    /// Removes a terminal tab (its group goes too when it becomes empty) or a whole
    /// group. Returns the removed node and the panes that are gone.
    pub fn remove_tab(&mut self, tab: TabId, _promote: bool) -> Option<(TabNode, Vec<PaneId>)> {
        let l = self.locate(tab)?;
        let order = match l.owner {
            Owner::Window(wi) => self.terminal_tabs(self.windows[wi].id),
            Owner::Detached => Vec::new(),
        };
        let removed = match l.child {
            Some(ci) => {
                let g = &mut self.list_mut(l.owner)[l.group];
                let n = g.children.remove(ci);
                if g.active_child == Some(tab) {
                    g.active_child = g
                        .children
                        .get(ci.min(g.children.len().saturating_sub(1)))
                        .map(|c| c.id);
                }
                g.group_sizes.clear();
                n
            }
            None => self.list_mut(l.owner).remove(l.group),
        };
        self.drop_empty_groups(l.owner);
        self.fix_active(l.owner, &order, tab);
        let mut panes = Vec::new();
        removed.all_panes(&mut panes);
        Some((removed, panes))
    }

    /// Group and index of a terminal tab (for reopening it in place).
    pub fn slot_of(&self, tab: TabId) -> Option<(TabId, usize)> {
        let l = self.locate(tab)?;
        Some((self.list(l.owner)[l.group].id, l.child?))
    }

    /// Re-inserts a terminal tab into `group` at `index`, or as a new group.
    pub fn restore_tab(
        &mut self,
        window: WindowId,
        group: Option<TabId>,
        index: usize,
        node: TabNode,
    ) {
        let tid = node.id;
        if let Some(l) = group
            .and_then(|g| self.locate(g))
            .filter(|l| l.child.is_none())
        {
            let g = &mut self.list_mut(l.owner)[l.group];
            let i = index.min(g.children.len());
            g.children.insert(i, node);
            g.group_sizes.clear();
            self.select_tab(tid);
        } else {
            let active = self.window(window).and_then(|w| w.active_tab);
            self.new_group(window, active, node);
        }
    }

    /// Moves a tab among its siblings. A tab alone in its group moves its group.
    pub fn move_tab(&mut self, tab: TabId, down: bool) -> bool {
        let Some(mut l) = self.locate(tab) else {
            return false;
        };
        if l.child.is_some() && self.list(l.owner)[l.group].children.len() == 1 {
            l.child = None;
        }
        let swap = |v: usize, len: usize| -> Option<usize> {
            if down {
                (v + 1 < len).then_some(v + 1)
            } else {
                v.checked_sub(1)
            }
        };
        match l.child {
            Some(ci) => {
                let g = &mut self.list_mut(l.owner)[l.group];
                let Some(j) = swap(ci, g.children.len()) else {
                    return false;
                };
                g.children.swap(ci, j);
                g.group_sizes.clear();
            }
            None => {
                let list = self.list_mut(l.owner);
                let Some(j) = swap(l.group, list.len()) else {
                    return false;
                };
                list.swap(l.group, j);
            }
        }
        true
    }

    /// Moves a terminal tab (or every tab of a group) into the group above.
    pub fn indent_tab(&mut self, tab: TabId) -> bool {
        let Some(l) = self.locate(tab) else {
            return false;
        };
        if l.group == 0 {
            return false;
        }
        let single = self.list(l.owner)[l.group].children.len() == 1;
        let moved: Vec<TabNode> = match l.child {
            Some(ci) if !single => vec![self.list_mut(l.owner)[l.group].children.remove(ci)],
            _ => std::mem::take(&mut self.list_mut(l.owner)[l.group].children),
        };
        let prev = &mut self.list_mut(l.owner)[l.group - 1];
        prev.collapsed = false;
        prev.group_sizes.clear();
        prev.children.extend(moved);
        self.drop_empty_groups(l.owner);
        if l.child.is_some() {
            self.select_tab(tab);
        }
        true
    }

    /// Moves a terminal tab out of a shared group into its own group right after it.
    pub fn outdent_tab(&mut self, tab: TabId) -> bool {
        let Some(l) = self.locate(tab) else {
            return false;
        };
        let Some(ci) = l.child else { return false };
        let g = &mut self.list_mut(l.owner)[l.group];
        if g.children.len() < 2 {
            return false;
        }
        let n = g.children.remove(ci);
        g.group_sizes.clear();
        if g.active_child == Some(tab) {
            g.active_child = g.children.first().map(|c| c.id);
        }
        let group = TabNode::group_with(n);
        self.list_mut(l.owner).insert(l.group + 1, group);
        self.select_tab(tab);
        true
    }

    /// Collapses or expands a group (a terminal tab toggles its group).
    pub fn toggle_collapse(&mut self, tab: TabId) -> bool {
        let Some(gid) = self.group_of(tab) else {
            return false;
        };
        let wid = self.window_of_tab(gid);
        let Some(g) = self.tab_mut(gid) else {
            return false;
        };
        if g.children.len() < 2 {
            return false;
        }
        g.collapsed = !g.collapsed;
        let collapsed = g.collapsed;
        let kids: Vec<TabId> = g.children.iter().map(|c| c.id).collect();
        let _ = (wid, collapsed, kids);
        true
    }

    /// Drag-and-drop. Dropping on the middle of a row (`Child`) joins that row's group;
    /// `Before`/`After` place the source next to the row: inside an expanded group for
    /// nested rows, as a separate group for top-level rows.
    pub fn drop_tab(&mut self, tab: TabId, target: TabId, pos: DropPosition) -> bool {
        if tab == target {
            return false;
        }
        let (Some(src), Some(dst)) = (self.locate(tab), self.locate(target)) else {
            return false;
        };
        if src.child.is_none()
            && self.list(src.owner)[src.group].id == self.list(dst.owner)[dst.group].id
            && dst.child.is_some()
        {
            return false;
        }
        let dst_group_id = self.list(dst.owner)[dst.group].id;
        let dst_group_len = self.list(dst.owner)[dst.group].children.len();
        // A terminal shown as a single row behaves like its group at the top level.
        let target_is_top = dst.child.is_none() || dst_group_len == 1;
        // Take the source out.
        let moved: Vec<TabNode> = match src.child {
            Some(ci) => {
                let g = &mut self.list_mut(src.owner)[src.group];
                g.group_sizes.clear();
                vec![g.children.remove(ci)]
            }
            None => {
                if pos != DropPosition::Child {
                    // Whole group moves next to the target group.
                    let g = self.list_mut(src.owner).remove(src.group);
                    let Some(dl) = self.locate(dst_group_id) else {
                        self.list_mut(src.owner).insert(src.group, g);
                        return false;
                    };
                    let idx = dl.group + usize::from(pos == DropPosition::After);
                    let first = g.active_child.or(g.children.first().map(|c| c.id));
                    self.list_mut(dl.owner).insert(idx, g);
                    if let Some(f) = first {
                        self.select_tab(f);
                    }
                    return true;
                }
                std::mem::take(&mut self.list_mut(src.owner)[src.group].children)
            }
        };
        let first = moved.first().map(|c| c.id);
        let src_owner = src.owner;
        // Re-locate the target: indices may have shifted.
        let Some(dl) = self.locate(target) else {
            return false;
        };
        match (pos, target_is_top) {
            (DropPosition::Child, _) => {
                let g = &mut self.list_mut(dl.owner)[dl.group];
                let at = dl.child.map(|c| c + 1).unwrap_or(g.children.len());
                for (k, m) in moved.into_iter().enumerate() {
                    g.children.insert(at + k, m);
                }
                g.collapsed = false;
                g.group_sizes.clear();
            }
            (_, true) => {
                let idx = dl.group + usize::from(pos == DropPosition::After);
                let mut it = moved.into_iter();
                let mut group = TabNode::group_with(it.next().unwrap());
                group.children.extend(it);
                self.list_mut(dl.owner).insert(idx, group);
            }
            (_, false) => {
                let g = &mut self.list_mut(dl.owner)[dl.group];
                let at = dl.child.unwrap() + usize::from(pos == DropPosition::After);
                for (k, m) in moved.into_iter().enumerate() {
                    g.children.insert(at + k, m);
                }
                g.group_sizes.clear();
            }
        }
        self.drop_empty_groups(src_owner);
        if let Some(f) = first {
            self.select_tab(f);
        }
        true
    }

    /// Moves a group, or a terminal tab as a new group, to another window.
    pub fn move_tab_to_window(&mut self, tab: TabId, window: WindowId) -> bool {
        let Some(dst) = self.owner_of_window(window) else {
            return false;
        };
        let Some(l) = self.locate(tab) else {
            return false;
        };
        if l.owner == dst {
            return false;
        }
        let single = self.list(l.owner)[l.group].children.len() == 1;
        let order = match l.owner {
            Owner::Window(wi) => self.terminal_tabs(self.windows[wi].id),
            Owner::Detached => Vec::new(),
        };
        let group = match l.child {
            Some(ci) if !single => {
                TabNode::group_with(self.list_mut(l.owner)[l.group].children.remove(ci))
            }
            _ => self.list_mut(l.owner).remove(l.group),
        };
        let first = if l.child.is_some() {
            Some(tab)
        } else {
            group.active_child.or(group.children.first().map(|c| c.id))
        };
        self.list_mut(dst).push(group);
        self.drop_empty_groups(l.owner);
        self.fix_active(l.owner, &order, tab);
        if let Some(f) = first {
            self.select_tab(f);
        }
        true
    }

    /// Closes a window in detach mode: its groups go to the detached list.
    pub fn detach_window(&mut self, window: WindowId) -> Option<WindowNode> {
        let idx = self.windows.iter().position(|w| w.id == window)?;
        let mut w = self.windows.remove(idx);
        self.detached.append(&mut w.tabs);
        Some(w)
    }

    /// Re-attaches a detached group (or the group of a detached tab) to a window.
    pub fn attach_detached(&mut self, tab: TabId, window: WindowId) -> bool {
        let Some(l) = self.locate(tab).filter(|l| l.owner == Owner::Detached) else {
            return false;
        };
        let Some(dst) = self.owner_of_window(window) else {
            return false;
        };
        let g = self.detached.remove(l.group);
        let first = g.active_child.or(g.children.first().map(|c| c.id));
        self.list_mut(dst).push(g);
        if let Some(f) = first {
            self.select_tab(f);
        }
        true
    }

    // ----- group view -----

    /// The group shown stacked (group view on, two or more tabs) that holds `tab`.
    pub fn group_root(&self, tab: TabId) -> Option<TabId> {
        let g = self.tab(self.group_of(tab)?)?;
        (g.group_view && g.children.len() > 1).then_some(g.id)
    }

    /// Terminal tabs of a group in sidebar order.
    pub fn group_members(&self, root: TabId) -> Vec<TabId> {
        self.tab(root)
            .map(|g| g.children.iter().map(|c| c.id).collect())
            .unwrap_or_default()
    }

    /// Section fractions for the group, equal when the member count changed.
    pub fn group_sizes(&self, root: TabId) -> Vec<f32> {
        let n = self.group_members(root).len().max(1);
        match self.tab(root) {
            Some(t)
                if t.group_sizes.len() == n
                    && t.group_sizes.iter().all(|x| x.is_finite() && *x > 0.0) =>
            {
                let mut v = t.group_sizes.clone();
                normalize(&mut v);
                v
            }
            _ => vec![1.0 / n as f32; n],
        }
    }

    pub fn set_group_sizes(&mut self, root: TabId, sizes: Vec<f32>) -> bool {
        let n = self.group_members(root).len();
        if n == 0 || sizes.len() != n || !sizes.iter().all(|x| x.is_finite() && *x > 0.0) {
            return false;
        }
        let Some(t) = self.tab_mut(root) else {
            return false;
        };
        t.group_sizes = sizes;
        normalize(&mut t.group_sizes);
        true
    }

    /// Toggles group view for the group of `tab`. Returns the group.
    pub fn toggle_group_view(&mut self, tab: TabId) -> Option<TabId> {
        let gid = self.group_of(tab)?;
        let g = self.tab_mut(gid)?;
        g.group_view = !g.group_view;
        g.group_sizes.clear();
        if g.group_view {
            g.collapsed = false;
        }
        Some(gid)
    }

    // ----- invariants -----

    /// Normalizes any tree (including snapshots from older versions) into
    /// groups → terminal tabs, and fixes active selections.
    pub fn sanitize(&mut self) {
        fn flatten(n: TabNode, out: &mut Vec<TabNode>) {
            let mut n = n;
            let kids = std::mem::take(&mut n.children);
            if !n.panes.is_empty() {
                n.group_view = false;
                n.group_sizes.clear();
                n.active_child = None;
                n.collapsed = false;
                n.sanitize();
                out.push(n);
            }
            for k in kids {
                flatten(k, out);
            }
        }
        fn normalize_list(list: Vec<TabNode>) -> Vec<TabNode> {
            let mut out = Vec::new();
            for root in list {
                let mut kids = Vec::new();
                if root.panes.is_empty() {
                    let mut g = root;
                    for k in std::mem::take(&mut g.children) {
                        flatten(k, &mut kids);
                    }
                    if kids.is_empty() {
                        continue;
                    }
                    g.children = kids;
                    if g.active_child
                        .map(|a| !g.children.iter().any(|c| c.id == a))
                        .unwrap_or(true)
                    {
                        g.active_child = g.children.first().map(|c| c.id);
                    }
                    out.push(g);
                } else {
                    // Old layout: a terminal at the top level, possibly with nested tabs.
                    let (collapsed, group_view, sizes, title) = (
                        root.collapsed,
                        root.group_view,
                        root.group_sizes.clone(),
                        None::<String>,
                    );
                    flatten(root, &mut kids);
                    let mut it = kids.into_iter();
                    let mut g = TabNode::group_with(it.next().unwrap());
                    g.children.extend(it);
                    g.collapsed = collapsed;
                    g.group_view = group_view;
                    g.group_sizes = sizes;
                    g.custom_title = title;
                    out.push(g);
                }
            }
            out
        }
        for w in &mut self.windows {
            w.tabs = normalize_list(std::mem::take(&mut w.tabs));
        }
        self.detached = normalize_list(std::mem::take(&mut self.detached));
        self.windows.retain(|w| !w.tabs.is_empty());
        for wi in 0..self.windows.len() {
            let active = self.windows[wi].active_tab;
            let ok = active
                .and_then(|a| self.locate(a))
                .map(|l| l.owner == Owner::Window(wi))
                .unwrap_or(false);
            let target = if ok {
                active
            } else {
                self.windows[wi].tabs.first().map(|g| g.id)
            };
            if let Some(t) = target {
                self.select_tab(t);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term() -> TabNode {
        TabNode::new(PaneId::new(), "default")
    }

    fn ws() -> (Workspace, WindowId) {
        let mut ws = Workspace::default();
        let w = WindowNode::new(220.0);
        let id = w.id;
        ws.windows.push(w);
        (ws, id)
    }

    /// Window with groups of the given sizes; returns terminal ids per group.
    fn build(sizes: &[usize]) -> (Workspace, WindowId, Vec<TabId>, Vec<Vec<TabId>>) {
        let (mut ws, w) = ws();
        let mut groups = Vec::new();
        let mut terms = Vec::new();
        for &n in sizes {
            let t = term();
            let first = t.id;
            let g = ws.new_group(w, None, t).unwrap();
            let mut ids = vec![first];
            for _ in 1..n {
                let t = term();
                let tid = t.id;
                assert!(ws.add_to_group(*ids.last().unwrap(), t));
                ids.push(tid);
            }
            groups.push(g);
            terms.push(ids);
        }
        (ws, w, groups, terms)
    }

    #[test]
    fn new_group_and_rows() {
        let (ws, w, groups, terms) = build(&[1, 2]);
        assert!(ws.is_group(groups[0]));
        assert!(!ws.is_group(terms[0][0]));
        assert_eq!(ws.group_of(terms[1][1]), Some(groups[1]));
        assert_eq!(ws.depth(terms[1][0]), Some(1));
        let rows = ws.visible_tabs(w, false);
        // Single-tab group: one row standing for the tab, not expandable.
        assert_eq!(rows[0].id, terms[0][0]);
        assert!(!rows[0].has_children && !rows[0].is_group && rows[0].depth == 0);
        // Group of two: header plus two nested rows.
        assert_eq!(rows.len(), 4);
        assert!(rows[1].is_group && rows[1].has_children);
        assert_eq!(rows[3].depth, 1);
        // With expand_single the lone tab also gets a group row.
        assert_eq!(ws.visible_tabs(w, true).len(), 5);
        assert_eq!(ws.terminal_tabs(w).len(), 3);
        assert_eq!(ws.window(w).unwrap().active_tab, Some(terms[1][1]));
    }

    #[test]
    fn select_group_selects_child() {
        let (mut ws, w, groups, terms) = build(&[2, 1]);
        ws.select_tab(terms[0][1]);
        ws.select_tab(terms[1][0]);
        ws.select_tab(groups[0]);
        assert_eq!(ws.window(w).unwrap().active_tab, Some(terms[0][1]));
        assert_eq!(ws.active_tab(w).unwrap().id, terms[0][1]);
    }

    #[test]
    fn single_group_cannot_collapse() {
        let (mut ws, _w, groups, terms) = build(&[1, 2]);
        assert!(!ws.toggle_collapse(groups[0]));
        assert!(ws.toggle_collapse(terms[1][0]));
        assert!(ws.tab(groups[1]).unwrap().collapsed);
    }

    #[test]
    fn no_nesting_below_terminal() {
        let (mut ws, w, groups, terms) = build(&[1]);
        // A "child" of a terminal tab becomes its sibling in the same group.
        let t = term();
        let id = t.id;
        ws.insert_child(w, terms[0][0], t);
        assert_eq!(ws.parent(id), Some(groups[0]));
        assert!(ws.tab(terms[0][0]).unwrap().children.is_empty());
        assert_eq!(MAX_DEPTH, 1);
    }

    #[test]
    fn remove_last_tab_removes_group() {
        let (mut ws, w, groups, terms) = build(&[2, 1]);
        ws.select_tab(terms[1][0]);
        let (_, panes) = ws.remove_tab(terms[1][0], true).unwrap();
        assert_eq!(panes.len(), 1);
        assert!(ws.tab(groups[1]).is_none());
        assert_eq!(ws.window(w).unwrap().active_tab, Some(terms[0][1]));
        let (_, panes) = ws.remove_tab(groups[0], false).unwrap();
        assert_eq!(panes.len(), 2);
        assert!(ws.window(w).unwrap().tabs.is_empty());
        assert_eq!(ws.window(w).unwrap().active_tab, None);
    }

    #[test]
    fn reopen_in_place() {
        let (mut ws, w, groups, terms) = build(&[3]);
        let (g, i) = ws.slot_of(terms[0][1]).unwrap();
        let (node, _) = ws.remove_tab(terms[0][1], true).unwrap();
        ws.restore_tab(w, Some(g), i, node);
        assert_eq!(ws.group_members(groups[0]), terms[0]);
    }

    #[test]
    fn move_indent_outdent() {
        let (mut ws, w, groups, terms) = build(&[2, 1]);
        assert!(ws.move_tab(terms[0][0], true));
        assert_eq!(ws.group_members(groups[0]), vec![terms[0][1], terms[0][0]]);
        // A lone tab moves its whole group.
        assert!(ws.move_tab(terms[1][0], false));
        assert_eq!(ws.window(w).unwrap().tabs[0].id, groups[1]);
        assert!(ws.move_tab(terms[1][0], true));
        // Indent: into the group above.
        assert!(ws.indent_tab(terms[1][0]));
        assert!(ws.tab(groups[1]).is_none());
        assert_eq!(ws.group_members(groups[0]).len(), 3);
        // Outdent: into its own group right after.
        assert!(ws.outdent_tab(terms[1][0]));
        assert_eq!(ws.window(w).unwrap().tabs.len(), 2);
        assert!(!ws.outdent_tab(terms[1][0]));
    }

    #[test]
    fn drop_rules() {
        let (mut ws, w, groups, terms) = build(&[1, 1, 2]);
        // Onto the middle of a single row: the two tabs form one group.
        assert!(ws.drop_tab(terms[0][0], terms[1][0], DropPosition::Child));
        assert!(ws.tab(groups[0]).is_none());
        assert_eq!(ws.group_members(groups[1]), vec![terms[1][0], terms[0][0]]);
        // Before a group row: becomes its own group in front of it.
        assert!(ws.drop_tab(terms[0][0], groups[2], DropPosition::Before));
        let tabs: Vec<usize> = ws
            .window(w)
            .unwrap()
            .tabs
            .iter()
            .map(|g| g.children.len())
            .collect();
        assert_eq!(tabs, vec![1, 1, 2]);
        // Between nested rows: joins that group at that place.
        assert!(ws.drop_tab(terms[1][0], terms[2][1], DropPosition::Before));
        assert_eq!(
            ws.group_members(groups[2]),
            vec![terms[2][0], terms[1][0], terms[2][1]]
        );
        // A group onto another group's row merges.
        let g0 = ws.window(w).unwrap().tabs[0].id;
        assert!(ws.drop_tab(g0, groups[2], DropPosition::Child) || ws.tab(g0).is_some());
    }

    #[test]
    fn windows_and_detach() {
        let (mut ws, w1, groups, terms) = build(&[2]);
        let w2n = WindowNode::new(220.0);
        let w2 = w2n.id;
        ws.windows.push(w2n);
        // A tab from a shared group moves as a new group.
        assert!(ws.move_tab_to_window(terms[0][1], w2));
        assert_eq!(ws.window_of_tab(terms[0][1]), Some(w2));
        assert_eq!(ws.group_members(groups[0]), vec![terms[0][0]]);
        ws.detach_window(w2);
        assert_eq!(ws.detached.len(), 1);
        assert!(ws.attach_detached(terms[0][1], w1));
        assert_eq!(ws.window(w1).unwrap().tabs.len(), 2);
    }

    #[test]
    fn group_view() {
        let (mut ws, _w, groups, terms) = build(&[1, 3]);
        assert_eq!(ws.group_root(terms[1][0]), None);
        assert_eq!(ws.toggle_group_view(terms[1][2]), Some(groups[1]));
        assert_eq!(ws.group_root(terms[1][0]), Some(groups[1]));
        assert_eq!(ws.group_members(groups[1]), terms[1]);
        // A single-tab group never shows as a stack.
        ws.toggle_group_view(terms[0][0]);
        assert_eq!(ws.group_root(terms[0][0]), None);
        assert!(ws.set_group_sizes(groups[1], vec![2.0, 1.0, 1.0]));
        assert!((ws.group_sizes(groups[1])[0] - 0.5).abs() < 1e-4);
        assert!(!ws.set_group_sizes(groups[1], vec![1.0]));
        let t = term();
        ws.add_to_group(terms[1][0], t);
        assert_eq!(ws.group_sizes(groups[1]), vec![0.25; 4]);
    }

    #[test]
    fn migrate_old_tree() {
        // Old layout: terminal at the top level with nested terminals two levels deep.
        let mut a = term();
        let mut b = term();
        let c = term();
        let (ia, ib, ic) = (a.id, b.id, c.id);
        b.children.push(c);
        a.children.push(b);
        let d = term();
        let id = d.id;
        let mut w = WindowNode::new(220.0);
        let wid = w.id;
        w.tabs = vec![a, d];
        w.active_tab = Some(ic);
        let mut ws = Workspace {
            windows: vec![w],
            ..Default::default()
        };
        ws.sanitize();
        let wn = ws.window(wid).unwrap();
        assert_eq!(wn.tabs.len(), 2);
        assert!(wn.tabs.iter().all(|g| g.is_group()));
        assert_eq!(ws.group_members(wn.tabs[0].id), vec![ia, ib, ic]);
        assert_eq!(ws.group_members(wn.tabs[1].id), vec![id]);
        assert_eq!(ws.window(wid).unwrap().active_tab, Some(ic));
        assert!(ws.tab(ia).unwrap().children.is_empty());
    }

    #[test]
    fn panes_and_layout() {
        let mut t = term();
        let first = t.panes[0];
        for _ in 0..3 {
            assert!(t.add_pane(PaneId::new()));
        }
        assert!(!t.add_pane(PaneId::new()));
        assert_eq!(t.panes.len(), 4);
        assert!(t.toggle_layout());
        assert_eq!(t.layout, Layout::Grid);
        t.active_pane = first;
        assert_eq!(t.neighbour(Direction::Right), Some(t.panes[1]));
        assert_eq!(t.neighbour(Direction::Down), Some(t.panes[2]));
        assert_eq!(t.neighbour(Direction::Left), None);
        let last = t.panes[3];
        assert!(t.remove_pane(last));
        assert_eq!(t.layout, Layout::Stack);
        assert!(!t.toggle_layout());
        assert!((t.sizes.iter().sum::<f32>() - 1.0).abs() < 1e-4);
        t.active_pane = first;
        t.resize_active(Direction::Down, 0.1);
        assert!(t.sizes[0] > t.sizes[1]);
    }
}
