//! Pixel layout of panes inside a tab (stack or 2×2 grid), group sections (stacked or
//! tiled) and their splitters.

use at_core::PaneId;
use at_core::TabId;
use at_core::model::{Layout, TabNode, Workspace};
use iced::Rectangle;

pub const GAP: f32 = 1.0;
pub const SPLIT_HIT: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SplitKind {
    /// Between stack rows `i` and `i + 1`.
    Stack(usize),
    GridRow,
    GridCol,
    /// Between group sections `i` and `i + 1` (owner is the group root).
    Group(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct Splitter {
    pub kind: SplitKind,
    pub rect: Rectangle,
}

pub struct PaneLayout {
    pub panes: Vec<(PaneId, Rectangle)>,
    pub splitters: Vec<Splitter>,
}

pub fn compute(tab: &TabNode, w: f32, h: f32) -> PaneLayout {
    let mut panes = Vec::new();
    let mut splitters = Vec::new();
    if tab.zoomed && tab.panes.len() > 1 {
        panes.push((
            tab.active_pane,
            Rectangle::new(iced::Point::ORIGIN, iced::Size::new(w, h)),
        ));
        return PaneLayout { panes, splitters };
    }
    match tab.layout {
        Layout::Grid if tab.panes.len() == 4 => {
            let cw0 = ((w - GAP) * tab.grid_cols[0]).round();
            let rh0 = ((h - GAP) * tab.grid_rows[0]).round();
            let cw1 = w - GAP - cw0;
            let rh1 = h - GAP - rh0;
            let rects = [
                Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: cw0,
                    height: rh0,
                },
                Rectangle {
                    x: cw0 + GAP,
                    y: 0.0,
                    width: cw1,
                    height: rh0,
                },
                Rectangle {
                    x: 0.0,
                    y: rh0 + GAP,
                    width: cw0,
                    height: rh1,
                },
                Rectangle {
                    x: cw0 + GAP,
                    y: rh0 + GAP,
                    width: cw1,
                    height: rh1,
                },
            ];
            for (p, r) in tab.panes.iter().zip(rects) {
                panes.push((*p, r));
            }
            splitters.push(Splitter {
                kind: SplitKind::GridRow,
                rect: Rectangle {
                    x: 0.0,
                    y: rh0 + GAP / 2.0 - SPLIT_HIT / 2.0,
                    width: w,
                    height: SPLIT_HIT,
                },
            });
            splitters.push(Splitter {
                kind: SplitKind::GridCol,
                rect: Rectangle {
                    x: cw0 + GAP / 2.0 - SPLIT_HIT / 2.0,
                    y: 0.0,
                    width: SPLIT_HIT,
                    height: h,
                },
            });
        }
        _ => {
            let n = tab.panes.len().max(1);
            let avail = h - GAP * (n as f32 - 1.0);
            let mut y = 0.0;
            for (i, p) in tab.panes.iter().enumerate() {
                let frac = tab.sizes.get(i).copied().unwrap_or(1.0 / n as f32);
                let ph = if i + 1 == n {
                    (h - y).max(0.0)
                } else {
                    (avail * frac).round()
                };
                panes.push((
                    *p,
                    Rectangle {
                        x: 0.0,
                        y,
                        width: w,
                        height: ph,
                    },
                ));
                y += ph;
                if i + 1 < n {
                    splitters.push(Splitter {
                        kind: SplitKind::Stack(i),
                        rect: Rectangle {
                            x: 0.0,
                            y: y + GAP / 2.0 - SPLIT_HIT / 2.0,
                            width: w,
                            height: SPLIT_HIT,
                        },
                    });
                    y += GAP;
                }
            }
        }
    }
    PaneLayout { panes, splitters }
}

pub const SECTION_HEADER: f32 = 22.0;
pub const GROUP_GAP: f32 = 3.0;

/// One tab drawn in the pane area: the whole area, or a section of a group.
pub struct Section {
    pub tab: TabId,
    pub depth: usize,
    pub header: Option<Rectangle>,
    #[allow(dead_code)] // used by layout tests and future callers
    pub body: Rectangle,
}

pub struct SceneSplit {
    /// Tab whose sizes the splitter changes (group root for `Group`).
    pub owner: TabId,
    pub kind: SplitKind,
    pub rect: Rectangle,
    /// Coordinate frame for dragging: the section body (or whole area for groups).
    pub frame: Rectangle,
}

/// Everything the pane area shows for the active tab of a window.
pub struct Scene {
    #[allow(dead_code)] // used by layout tests
    pub group: Option<TabId>,
    pub sections: Vec<Section>,
    pub panes: Vec<(TabId, PaneId, Rectangle)>,
    pub splitters: Vec<SceneSplit>,
}

fn offset(r: Rectangle, o: &Rectangle) -> Rectangle {
    Rectangle {
        x: r.x + o.x,
        y: r.y + o.y,
        ..r
    }
}

pub fn scene(ws: &Workspace, active: TabId, w: f32, h: f32) -> Scene {
    let full = Rectangle {
        x: 0.0,
        y: 0.0,
        width: w,
        height: h,
    };
    let zoomed = ws
        .tab(active)
        .map(|t| t.zoomed && t.panes.len() > 1)
        .unwrap_or(false);
    let group = if zoomed { None } else { ws.group_root(active) };
    let mut sc = Scene {
        group,
        sections: Vec::new(),
        panes: Vec::new(),
        splitters: Vec::new(),
    };
    let entries: Vec<(TabId, Option<Rectangle>, Rectangle, usize)> = match group {
        None => vec![(active, None, full, 0)],
        Some(root) if ws.tab(root).map(|g| g.group_tiles).unwrap_or(false) => {
            tiles(ws.group_members(root).len(), w, h)
                .into_iter()
                .zip(ws.group_members(root))
                .map(|((header, body), t)| (t, Some(header), body, 0))
                .collect()
        }
        Some(root) => {
            let members = ws.group_members(root);
            let sizes = ws.group_sizes(root);
            let n = members.len();
            let avail = (h - GROUP_GAP * (n as f32 - 1.0)).max(1.0);
            let mut y = 0.0;
            let mut out = Vec::new();
            for (i, t) in members.iter().enumerate() {
                let sh = if i + 1 == n {
                    (h - y).max(0.0)
                } else {
                    (avail * sizes[i]).round()
                };
                let header = Rectangle {
                    x: 0.0,
                    y,
                    width: w,
                    height: SECTION_HEADER.min(sh),
                };
                let body = Rectangle {
                    x: 0.0,
                    y: y + header.height,
                    width: w,
                    height: (sh - header.height).max(1.0),
                };
                out.push((*t, Some(header), body, 0));
                y += sh;
                if i + 1 < n {
                    sc.splitters.push(SceneSplit {
                        owner: root,
                        kind: SplitKind::Group(i),
                        rect: Rectangle {
                            x: 0.0,
                            y: y + GROUP_GAP / 2.0 - SPLIT_HIT / 2.0,
                            width: w,
                            height: SPLIT_HIT,
                        },
                        frame: full,
                    });
                    y += GROUP_GAP;
                }
            }
            out
        }
    };
    for (tab, header, body, depth) in entries {
        let Some(t) = ws.tab(tab) else { continue };
        let l = compute(t, body.width, body.height);
        for (p, r) in l.panes {
            sc.panes.push((tab, p, offset(r, &body)));
        }
        for s in l.splitters {
            sc.splitters.push(SceneSplit {
                owner: tab,
                kind: s.kind,
                rect: offset(s.rect, &body),
                frame: body,
            });
        }
        sc.sections.push(Section {
            tab,
            depth,
            header,
            body,
        });
    }
    sc
}

/// Header and body rectangles of `n` equal tiles: a near-square grid, wider than
/// tall, with the last row stretched when it is not full.
fn tiles(n: usize, w: f32, h: f32) -> Vec<(Rectangle, Rectangle)> {
    if n == 0 {
        return Vec::new();
    }
    let cols = (n as f32).sqrt().ceil() as usize;
    let rows = n.div_ceil(cols);
    let row_h = ((h - GROUP_GAP * (rows as f32 - 1.0)) / rows as f32).max(1.0);
    let mut out = Vec::with_capacity(n);
    for r in 0..rows {
        let in_row = (n - r * cols).min(cols);
        let y = (r as f32 * (row_h + GROUP_GAP)).round();
        let th = if r + 1 == rows {
            (h - y).max(1.0)
        } else {
            row_h.round()
        };
        let col_w = ((w - GROUP_GAP * (in_row as f32 - 1.0)) / in_row as f32).max(1.0);
        for c in 0..in_row {
            let x = (c as f32 * (col_w + GROUP_GAP)).round();
            let tw = if c + 1 == in_row {
                (w - x).max(1.0)
            } else {
                col_w.round()
            };
            let header = Rectangle {
                x,
                y,
                width: tw,
                height: SECTION_HEADER.min(th),
            };
            let body = Rectangle {
                x,
                y: y + header.height,
                width: tw,
                height: (th - header.height).max(1.0),
            };
            out.push((header, body));
        }
    }
    out
}

/// Grid size in cells for a pane rectangle.
pub fn cells(rect: &Rectangle, title_h: f32, pad: f32, cell_w: f32, cell_h: f32) -> (u16, u16) {
    let w = (rect.width - 2.0 * pad).max(cell_w);
    let h = (rect.height - title_h - 2.0 * pad).max(cell_h);
    (
        (w / cell_w).floor().max(2.0) as u16,
        (h / cell_h).floor().max(1.0) as u16,
    )
}

/// New stack fractions while dragging splitter `i` to `y` (pane-area coordinates).
pub fn drag_stack(sizes: &[f32], i: usize, y: f32, total_h: f32, min_px: f32) -> Vec<f32> {
    let n = sizes.len();
    let avail = (total_h - GAP * (n as f32 - 1.0)).max(1.0);
    let mut px: Vec<f32> = sizes.iter().map(|s| s * avail).collect();
    let top: f32 = px[..i].iter().sum::<f32>() + GAP * i as f32;
    let pair = px[i] + px[i + 1];
    let new_i = (y - top).clamp(min_px, (pair - min_px).max(min_px));
    px[i] = new_i;
    px[i + 1] = pair - new_i;
    px.iter().map(|p| p / avail).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_and_grid() {
        let mut t = TabNode::new(PaneId::new(), "default");
        let l = compute(&t, 800.0, 600.0);
        assert_eq!(l.panes.len(), 1);
        assert_eq!(l.panes[0].1.height, 600.0);
        for _ in 0..3 {
            t.add_pane(PaneId::new());
        }
        let l = compute(&t, 800.0, 600.0);
        assert_eq!(l.splitters.len(), 3);
        let total: f32 = l.panes.iter().map(|(_, r)| r.height).sum::<f32>() + 3.0 * GAP;
        assert!((total - 600.0).abs() < 0.5);
        t.toggle_layout();
        let l = compute(&t, 800.0, 600.0);
        assert_eq!(l.splitters.len(), 2);
        assert_eq!(l.panes[3].1.x, l.panes[1].1.x);
    }

    #[test]
    fn group_scene() {
        use at_core::model::WindowNode;
        let mut ws = Workspace::default();
        let win = WindowNode::new(220.0);
        let wid = win.id;
        ws.windows.push(win);
        let a = TabNode::new(PaneId::new(), "default");
        let ia = a.id;
        let g = ws.new_group(wid, None, a).unwrap();
        let mut b = TabNode::new(PaneId::new(), "default");
        b.add_pane(PaneId::new());
        let ib = b.id;
        ws.add_to_group(ia, b);
        let sc = scene(&ws, ib, 800.0, 600.0);
        assert!(sc.group.is_none());
        assert_eq!(sc.panes.len(), 2);
        ws.toggle_group_view(ib);
        let sc = scene(&ws, ib, 800.0, 603.0);
        assert_eq!(sc.group, Some(g));
        assert_eq!(sc.sections.len(), 2);
        assert_eq!(sc.panes.len(), 3);
        assert_eq!(sc.sections[0].tab, ia);
        assert_eq!(sc.sections[1].tab, ib);
        let s0 = &sc.sections[0];
        let s1 = &sc.sections[1];
        assert!(s0.body.y + s0.body.height <= s1.header.unwrap().y);
        assert!((s1.body.y + s1.body.height - 603.0).abs() < 0.5);
        assert_eq!(
            sc.splitters
                .iter()
                .filter(|s| matches!(s.kind, SplitKind::Group(0)))
                .count(),
            1
        );
        assert_eq!(
            sc.splitters
                .iter()
                .filter(|s| matches!(s.kind, SplitKind::Stack(0)) && s.owner == ib)
                .count(),
            1
        );
    }

    #[test]
    fn group_tiles() {
        use at_core::model::{GroupMode, WindowNode};
        let mut ws = Workspace::default();
        let win = WindowNode::new(220.0);
        let wid = win.id;
        ws.windows.push(win);
        let a = TabNode::new(PaneId::new(), "default");
        let ia = a.id;
        ws.new_group(wid, None, a).unwrap();
        for _ in 0..2 {
            ws.add_to_group(ia, TabNode::new(PaneId::new(), "default"));
        }
        assert!(ws.set_group_mode(ia, GroupMode::Tiles).is_some());
        assert!(ws.group_tiled(ia));
        let sc = scene(&ws, ia, 801.0, 601.0);
        assert_eq!(sc.sections.len(), 3);
        assert!(sc.splitters.is_empty());
        let (s0, s1, s2) = (&sc.sections[0], &sc.sections[1], &sc.sections[2]);
        // Two tiles side by side on top, the third spans the bottom row.
        assert_eq!(s0.body.y, s1.body.y);
        assert!(s0.body.x + s0.body.width < s1.body.x);
        assert!((s1.body.x + s1.body.width - 801.0).abs() < 0.5);
        assert_eq!(s2.body.x, 0.0);
        assert!((s2.body.width - 801.0).abs() < 0.5);
        assert!((s2.body.y + s2.body.height - 601.0).abs() < 0.5);
        ws.set_group_mode(ia, GroupMode::Stack);
        assert!(!ws.group_tiled(ia));
        assert!(ws.group_root(ia).is_some());
    }

    #[test]
    fn dragging() {
        let s = drag_stack(&[0.5, 0.5], 0, 100.0, 401.0, 50.0);
        assert!((s[0] - 0.25).abs() < 0.01);
        let s = drag_stack(&[0.5, 0.5], 0, 5.0, 401.0, 50.0);
        assert!(s[0] > 0.1);
        assert_eq!(
            cells(
                &Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: 820.0,
                    height: 542.0
                },
                22.0,
                10.0,
                8.0,
                16.0
            ),
            (100, 31)
        );
    }
}
