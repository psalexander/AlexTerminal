//! Messages between GUI clients and the session daemon (§3.3) and their framing:
//! `u32` big-endian length followed by a MessagePack body.

use at_core::history::HistoryEntry;
use at_core::model::{Direction, DropPosition, GroupMode, WindowGeom, Workspace};
use at_core::pane::PaneInfo;
use at_core::{PaneId, TabId, WindowId};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

/// Bump on any change to the messages below. Variants are encoded by name, so an
/// older server rejects unknown commands; clients then replace it automatically.
/// `ClientMsg::Hello` and `ClientMsg::Shutdown` must keep their names and shape
/// forever (see the `stable_control_messages` test).
pub const PROTO_VERSION: u32 = 3;

/// Parses the server version out of a `protocol mismatch` error.
pub fn mismatch_server_version(error: &str) -> Option<u32> {
    let rest = error.strip_prefix("protocol mismatch: server ")?;
    rest.split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}
pub const MAX_FRAME: usize = 64 * 1024 * 1024;

// ----- colors and cells -----

pub const COLOR_RGB: u32 = 0x0100_0000;
pub const COLOR_INDEXED: u32 = 0x0200_0000;
pub const COLOR_DEFAULT_FG: u32 = 0x0300_0000;
pub const COLOR_DEFAULT_BG: u32 = 0x0400_0000;
pub const COLOR_TAG_MASK: u32 = 0xFF00_0000;

pub mod flags {
    pub const BOLD: u16 = 1;
    pub const ITALIC: u16 = 1 << 1;
    pub const UNDERLINE: u16 = 1 << 2;
    pub const DOUBLE_UNDERLINE: u16 = 1 << 3;
    pub const STRIKE: u16 = 1 << 4;
    pub const DIM: u16 = 1 << 5;
    pub const INVERSE: u16 = 1 << 6;
    pub const HIDDEN: u16 = 1 << 7;
    pub const WIDE: u16 = 1 << 8;
    pub const SPACER: u16 = 1 << 9;
    pub const SELECTED: u16 = 1 << 10;
    pub const MATCH: u16 = 1 << 11;
    pub const MATCH_CURRENT: u16 = 1 << 12;
    pub const HYPERLINK: u16 = 1 << 13;
    pub const UNDERCURL: u16 = 1 << 14;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Cell {
    pub ch: char,
    pub fg: u32,
    pub bg: u32,
    pub flags: u16,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            ch: ' ',
            fg: COLOR_DEFAULT_FG,
            bg: COLOR_DEFAULT_BG,
            flags: 0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Row {
    pub cells: Vec<Cell>,
    /// Zero-width characters (combining marks, ZWJ sequences) attached to a column.
    pub extra: Vec<(u16, String)>,
}

impl Row {
    pub fn text(&self) -> String {
        let mut s = String::with_capacity(self.cells.len());
        for (i, c) in self.cells.iter().enumerate() {
            if c.flags & flags::SPACER != 0 {
                continue;
            }
            s.push(c.ch);
            for (col, z) in &self.extra {
                if *col as usize == i {
                    s.push_str(z);
                }
            }
        }
        s
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CursorShape {
    #[default]
    Block,
    Underline,
    Beam,
    Hidden,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub col: u16,
    pub row: u16,
    pub shape: CursorShape,
    /// The application asked for a blinking cursor.
    pub blinking: bool,
}

pub mod mode {
    pub const APP_CURSOR: u32 = 1;
    pub const MOUSE: u32 = 1 << 1;
    pub const ALT_SCREEN: u32 = 1 << 2;
    pub const BRACKETED_PASTE: u32 = 1 << 3;
    pub const FOCUS_EVENTS: u32 = 1 << 4;
    pub const MOUSE_MOTION: u32 = 1 << 5;
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchInfo {
    pub query: String,
    pub regex: bool,
    pub total: u32,
    /// 1-based index of the current match, 0 if none.
    pub current: u32,
    pub error: Option<String>,
}

/// Visible grid of one pane. `lines` holds changed rows only unless `full`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub pane: PaneId,
    pub cols: u16,
    pub rows: u16,
    pub full: bool,
    pub lines: Vec<(u16, Row)>,
    pub cursor: Cursor,
    pub display_offset: u32,
    pub history_size: u32,
    pub mode: u32,
    pub search: Option<SearchInfo>,
}

// ----- input -----

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NamedKey {
    Enter,
    Tab,
    Backspace,
    Escape,
    Space,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F(u8),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyCode {
    /// The key's character without modifiers (layout-dependent).
    Char(String),
    Named(NamedKey),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyInput {
    pub key: KeyCode,
    /// Latin letter/digit of the physical key, used for Ctrl combos on non-Latin layouts.
    pub physical: Option<char>,
    /// Text the key produced with modifiers (except Ctrl) applied.
    pub text: Option<String>,
    pub mods: Mods,
    /// Treat Alt as Meta (ESC prefix) for this key.
    pub alt_is_meta: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseKind {
    Press,
    Release,
    Move,
    /// Positive is up.
    Wheel(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MouseInput {
    pub kind: MouseKind,
    pub button: MouseButton,
    pub col: u16,
    pub row: u16,
    /// Pointer is in the right half of the cell (selection side).
    pub right_half: bool,
    pub clicks: u8,
    pub mods: Mods,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScrollTo {
    Top,
    Bottom,
    PageUp,
    PageDown,
    Lines(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloseWindowMode {
    Detach,
    CloseTabs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientKind {
    Gui,
    Cli,
}

/// Structural commands; the daemon applies them and broadcasts the new workspace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Cmd {
    NewWindow {
        cwd: Option<String>,
        profile: Option<String>,
        command: Option<String>,
        geom: Option<WindowGeom>,
    },
    /// New window holding every detached tab (first launch after windows were closed).
    RestoreDetachedWindow,
    FocusWindow {
        window: WindowId,
    },
    CloseWindow {
        window: WindowId,
        mode: CloseWindowMode,
    },
    NewTab {
        window: WindowId,
        nested: bool,
        profile: Option<String>,
        cwd: Option<String>,
        command: Option<String>,
    },
    DuplicateTab {
        tab: TabId,
    },
    CloseTab {
        tab: TabId,
        with_children: bool,
    },
    RenameTab {
        tab: TabId,
        title: Option<String>,
    },
    SelectTab {
        tab: TabId,
    },
    MoveTab {
        tab: TabId,
        down: bool,
    },
    IndentTab {
        tab: TabId,
    },
    OutdentTab {
        tab: TabId,
    },
    ToggleCollapse {
        tab: TabId,
    },
    ToggleGroupView {
        tab: TabId,
    },
    SetGroupMode {
        tab: TabId,
        mode: GroupMode,
    },
    SetGroupSizes {
        root: TabId,
        sizes: Vec<f32>,
    },
    DropTab {
        tab: TabId,
        target: TabId,
        pos: DropPosition,
    },
    MoveTabToWindow {
        tab: TabId,
        window: Option<WindowId>,
    },
    AttachDetached {
        tab: TabId,
        window: WindowId,
    },
    ReopenClosedTab {
        window: WindowId,
    },
    NewPane {
        tab: TabId,
    },
    ClosePane {
        pane: PaneId,
    },
    FocusPane {
        pane: PaneId,
    },
    FocusDirection {
        tab: TabId,
        dir: Direction,
    },
    ToggleLayout {
        tab: TabId,
    },
    SetSizes {
        tab: TabId,
        sizes: Vec<f32>,
    },
    SetGrid {
        tab: TabId,
        rows: [f32; 2],
        cols: [f32; 2],
    },
    ResizePane {
        tab: TabId,
        dir: Direction,
    },
    EqualizePanes {
        tab: TabId,
    },
    ZoomPane {
        tab: TabId,
    },
    MovePane {
        tab: TabId,
        down: bool,
    },
    RestartShell {
        pane: PaneId,
    },
    WindowGeometry {
        window: WindowId,
        geom: WindowGeom,
    },
    Sidebar {
        window: WindowId,
        width: f32,
        hidden: bool,
    },
    Reconnect {
        pane: PaneId,
        accept: bool,
    },
    SetFontScale {
        pane: PaneId,
        scale: f32,
    },
    ReloadConfig,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ClientMsg {
    Hello {
        version: u32,
        kind: ClientKind,
        pid: u32,
    },
    Cmd(Cmd),
    /// Panes this client is showing; frames are sent only for them.
    Visible {
        panes: Vec<PaneId>,
    },
    Key {
        pane: PaneId,
        key: KeyInput,
    },
    Text {
        pane: PaneId,
        text: String,
    },
    Paste {
        pane: PaneId,
        text: String,
    },
    Resize {
        pane: PaneId,
        cols: u16,
        rows: u16,
        cell_w: u16,
        cell_h: u16,
    },
    Mouse {
        pane: PaneId,
        input: MouseInput,
    },
    Scroll {
        pane: PaneId,
        to: ScrollTo,
    },
    Focus {
        pane: PaneId,
        focused: bool,
    },
    Copy {
        pane: PaneId,
    },
    SelectAll {
        pane: PaneId,
    },
    ClearSelection {
        pane: PaneId,
    },
    ClearScreen {
        pane: PaneId,
    },
    Search {
        pane: PaneId,
        query: String,
        regex: bool,
        forward: bool,
    },
    SearchClose {
        pane: PaneId,
    },
    GetHistory {
        pane: PaneId,
    },
    RunCommand {
        pane: PaneId,
        command: String,
        execute: bool,
    },
    ClearAttention {
        pane: PaneId,
    },
    /// The GUI is quitting; sessions keep running.
    Detach,
    /// End all sessions and stop the session server (the app quits with it).
    Shutdown,
    Ping,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Notice {
    ConfigError(String),
    SessionRestoredFromBackup(String),
    SessionUnreadable(String),
    CwdMissing { pane: PaneId, cwd: String },
    Info(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ServerMsg {
    Welcome {
        version: u32,
        workspace: Workspace,
        panes: Vec<PaneInfo>,
        gui_clients: u32,
    },
    Workspace(Workspace),
    Pane(PaneInfo),
    PaneRemoved(PaneId),
    Frame(Frame),
    Clipboard(String),
    Bell(PaneId),
    History {
        pane: PaneId,
        entries: Vec<HistoryEntry>,
    },
    Notice(Notice),
    /// Version mismatch or fatal error; the client should report it.
    Error(String),
    Pong,
    ShuttingDown,
}

// ----- framing -----

pub fn encode<T: Serialize>(msg: &T) -> std::io::Result<Vec<u8>> {
    let body = rmp_serde::to_vec(msg).map_err(std::io::Error::other)?;
    let mut out = Vec::with_capacity(body.len() + 4);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

pub fn write_msg<W: Write, T: Serialize>(w: &mut W, msg: &T) -> std::io::Result<()> {
    w.write_all(&encode(msg)?)
}

pub fn read_msg<R: Read, T: for<'de> Deserialize<'de>>(r: &mut R) -> std::io::Result<T> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    rmp_serde::from_slice(&body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut frame = Frame {
            pane: PaneId::new(),
            cols: 3,
            rows: 1,
            full: true,
            ..Default::default()
        };
        frame.lines.push((
            0,
            Row {
                cells: vec![
                    Cell {
                        ch: 'é',
                        fg: COLOR_RGB | 0x123456,
                        bg: COLOR_DEFAULT_BG,
                        flags: flags::BOLD
                    };
                    3
                ],
                extra: vec![(1, "\u{301}".into())],
            },
        ));
        let msgs = vec![
            ServerMsg::Frame(frame),
            ServerMsg::Workspace(Workspace::default()),
            ServerMsg::Pane(PaneInfo::new(PaneId::new(), "default")),
        ];
        let mut buf = Vec::new();
        for m in &msgs {
            write_msg(&mut buf, m).unwrap();
        }
        let mut r = std::io::Cursor::new(buf);
        for m in &msgs {
            let back: ServerMsg = read_msg(&mut r).unwrap();
            assert_eq!(&back, m);
        }
        let c = ClientMsg::Key {
            pane: PaneId::new(),
            key: KeyInput {
                key: KeyCode::Named(NamedKey::F(5)),
                physical: None,
                text: None,
                mods: Mods::default(),
                alt_is_meta: false,
            },
        };
        let back: ClientMsg = read_msg(&mut std::io::Cursor::new(encode(&c).unwrap())).unwrap();
        assert_eq!(back, c);
    }

    /// Bytes that every server version since 1 understands.
    #[test]
    fn stable_control_messages() {
        let hello = encode(&ClientMsg::Hello {
            version: 1,
            kind: ClientKind::Gui,
            pid: 7,
        })
        .unwrap();
        assert_eq!(
            hello,
            vec![
                0, 0, 0, 14, 129, 165, 72, 101, 108, 108, 111, 147, 1, 163, 71, 117, 105, 7
            ]
        );
        let shutdown = encode(&ClientMsg::Shutdown).unwrap();
        assert_eq!(
            shutdown,
            [vec![0, 0, 0, 9, 168], b"Shutdown".to_vec()].concat()
        );
        assert_eq!(
            mismatch_server_version("protocol mismatch: server 1, client 2"),
            Some(1)
        );
        assert_eq!(mismatch_server_version("other"), None);
    }

    #[test]
    fn row_text() {
        let mut r = Row {
            cells: vec![
                Cell {
                    ch: 'a',
                    ..Default::default()
                },
                Cell {
                    ch: '世',
                    flags: flags::WIDE,
                    ..Default::default()
                },
                Cell {
                    ch: ' ',
                    flags: flags::SPACER,
                    ..Default::default()
                },
            ],
            extra: vec![],
        };
        r.extra.push((0, "\u{301}".into()));
        assert_eq!(r.text(), "a\u{301}世");
    }
}
