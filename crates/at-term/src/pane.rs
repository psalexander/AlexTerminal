//! A live terminal pane.

use crate::input;
use crate::osc::{OscEvent, OscScanner};
use crate::shell::{self, ShellLaunch};
use alacritty_terminal::event::{Event as TermEvent, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Boundary, Column, Direction as TDir, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags as CF;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config as TermConfig, Osc52 as TermOsc52, Term, TermMode};
use alacritty_terminal::vte::ansi::{
    self, Color, CursorShape as TShape, NamedColor, Processor, Rgb as VRgb, StdSyncHandler,
};
use at_core::PaneId;
use at_core::config::Osc52;
use at_core::theme::Rgb;
use at_proto::{
    self as proto, Cell, Cursor, CursorShape, Frame, KeyInput, MouseButton, MouseInput, MouseKind,
    Row, ScrollTo, SearchInfo, flags,
};
use crossbeam_channel::Sender;
use parking_lot::{Mutex, RwLock};
use portable_pty::{MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub enum PaneEvent {
    Dirty(PaneId),
    Title(PaneId, Option<String>),
    Bell(PaneId),
    Clipboard(PaneId, String),
    Osc(PaneId, OscEvent),
    Exited(PaneId, i32),
}

/// Colors used to answer OSC 4/10/11 queries.
#[derive(Clone, Debug)]
pub struct Palette {
    pub fg: Rgb,
    pub bg: Rgb,
    pub cursor: Rgb,
    pub ansi: [Rgb; 16],
}

impl Palette {
    pub fn from_theme(t: &at_core::theme::Theme) -> Self {
        let mut ansi = [Rgb::default(); 16];
        ansi[..8].copy_from_slice(&t.ansi.normal);
        ansi[8..].copy_from_slice(&t.ansi.bright);
        Palette {
            fg: t.ui.fg,
            bg: t.ui.bg,
            cursor: t.ui.cursor,
            ansi,
        }
    }

    fn lookup(&self, idx: usize) -> Rgb {
        match idx {
            0..=15 => self.ansi[idx],
            16..=255 => {
                let t = at_core::theme::Theme::dusk();
                t.indexed(idx as u8)
            }
            i if i == NamedColor::Foreground as usize => self.fg,
            i if i == NamedColor::Background as usize => self.bg,
            i if i == NamedColor::Cursor as usize => self.cursor,
            _ => self.fg,
        }
    }
}

struct Listener {
    id: PaneId,
    tx: Sender<PaneEvent>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    palette: Arc<RwLock<Palette>>,
    size: Arc<Mutex<WindowSize>>,
}

impl Listener {
    fn write(&self, s: &str) {
        let mut w = self.writer.lock();
        let _ = w.write_all(s.as_bytes());
        let _ = w.flush();
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: TermEvent) {
        match event {
            TermEvent::PtyWrite(s) => self.write(&s),
            TermEvent::Title(t) => {
                let _ = self.tx.send(PaneEvent::Title(self.id, Some(t)));
            }
            TermEvent::ResetTitle => {
                let _ = self.tx.send(PaneEvent::Title(self.id, None));
            }
            TermEvent::Bell => {
                let _ = self.tx.send(PaneEvent::Bell(self.id));
            }
            TermEvent::ClipboardStore(_, text) => {
                let _ = self.tx.send(PaneEvent::Clipboard(self.id, text));
            }
            TermEvent::ColorRequest(idx, f) => {
                let c = self.palette.read().lookup(idx);
                self.write(&f(VRgb {
                    r: c.0,
                    g: c.1,
                    b: c.2,
                }));
            }
            TermEvent::TextAreaSizeRequest(f) => {
                let s = *self.size.lock();
                self.write(&f(s));
            }
            _ => {}
        }
    }
}

pub struct SpawnOptions {
    pub id: PaneId,
    pub cols: u16,
    pub rows: u16,
    pub cell_w: u16,
    pub cell_h: u16,
    pub launch: ShellLaunch,
    pub scrollback_lines: usize,
    pub word_separators: String,
    pub osc52: Osc52,
    /// Bytes fed to the emulator before the shell starts (restored scrollback).
    pub preload: Option<Vec<u8>>,
    pub palette: Palette,
}

struct SearchState {
    query: String,
    regex: bool,
    re: RegexSearch,
    current: Option<Match>,
    total: u32,
    index: u32,
}

#[derive(Default)]
struct MouseState {
    selecting: bool,
    buttons_down: bool,
}

pub struct Pane {
    pub id: PaneId,
    term: Arc<Mutex<Term<Listener>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    shell_pid: i32,
    size: Arc<Mutex<WindowSize>>,
    palette: Arc<RwLock<Palette>>,
    search: Mutex<Option<SearchState>>,
    mouse: Mutex<MouseState>,
    dirty: Arc<AtomicBool>,
    exited: Arc<AtomicBool>,
    exit_code: Arc<AtomicI32>,
    pub shell: String,
}

const MAX_COUNT: u32 = 9999;

impl Pane {
    pub fn spawn(opts: SpawnOptions, tx: Sender<PaneEvent>) -> anyhow::Result<Arc<Pane>> {
        let cols = opts.cols.max(2);
        let rows = opts.rows.max(1);
        let pty = portable_pty::native_pty_system();
        let pair = pty.openpty(PtySize {
            rows,
            cols,
            pixel_width: cols * opts.cell_w,
            pixel_height: rows * opts.cell_h,
        })?;
        let cmd = shell::command(&opts.launch);
        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);
        let shell_pid = child.process_id().map(|p| p as i32).unwrap_or(-1);
        // The child is reaped with waitpid by the reader thread.
        std::mem::forget(child);
        let mut reader = pair.master.try_clone_reader()?;
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(pair.master.take_writer()?));

        let size = Arc::new(Mutex::new(WindowSize {
            num_lines: rows,
            num_cols: cols,
            cell_width: opts.cell_w,
            cell_height: opts.cell_h,
        }));
        let palette = Arc::new(RwLock::new(opts.palette));
        let listener = Listener {
            id: opts.id,
            tx: tx.clone(),
            writer: writer.clone(),
            palette: palette.clone(),
            size: size.clone(),
        };
        let config = TermConfig {
            scrolling_history: opts.scrollback_lines,
            semantic_escape_chars: opts.word_separators.clone(),
            osc52: match opts.osc52 {
                Osc52::Write => TermOsc52::OnlyCopy,
                Osc52::ReadWrite => TermOsc52::CopyPaste,
                Osc52::Off => TermOsc52::Disabled,
            },
            ..TermConfig::default()
        };
        let term = Term::new(
            config,
            &TermSize::new(cols as usize, rows as usize),
            listener,
        );
        let term = Arc::new(Mutex::new(term));
        let mut processor: Processor<StdSyncHandler> = Processor::new();
        if let Some(pre) = &opts.preload {
            let mut t = term.lock();
            processor.advance(&mut *t, pre);
        }

        let dirty = Arc::new(AtomicBool::new(true));
        let exited = Arc::new(AtomicBool::new(false));
        let exit_code = Arc::new(AtomicI32::new(0));
        let pane = Arc::new(Pane {
            id: opts.id,
            term: term.clone(),
            writer,
            master: Mutex::new(pair.master),
            shell_pid,
            size,
            palette,
            search: Mutex::new(None),
            mouse: Mutex::new(MouseState::default()),
            dirty: dirty.clone(),
            exited: exited.clone(),
            exit_code: exit_code.clone(),
            shell: opts.launch.shell.clone(),
        });

        let id = opts.id;
        std::thread::Builder::new()
            .name(format!("pty-{id}"))
            .spawn(move || {
                let mut buf = vec![0u8; 64 * 1024];
                let mut scanner = OscScanner::new();
                let mut events = Vec::new();
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let bytes = &buf[..n];
                            scanner.feed(bytes, &mut events);
                            {
                                let mut t = term.lock();
                                processor.advance(&mut *t, bytes);
                            }
                            for ev in events.drain(..) {
                                let _ = tx.send(PaneEvent::Osc(id, ev));
                            }
                            if !dirty.swap(true, Ordering::AcqRel) {
                                let _ = tx.send(PaneEvent::Dirty(id));
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
                let code = wait_child(shell_pid);
                exit_code.store(code, Ordering::Release);
                exited.store(true, Ordering::Release);
                dirty.store(true, Ordering::Release);
                let _ = tx.send(PaneEvent::Dirty(id));
                let _ = tx.send(PaneEvent::Exited(id, code));
            })?;
        Ok(pane)
    }

    pub fn shell_pid(&self) -> i32 {
        self.shell_pid
    }

    pub fn is_exited(&self) -> bool {
        self.exited.load(Ordering::Acquire)
    }

    pub fn exit_code(&self) -> i32 {
        self.exit_code.load(Ordering::Acquire)
    }

    /// Clears and returns the dirty flag.
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    pub fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Release);
    }

    pub fn set_palette(&self, p: Palette) {
        *self.palette.write() = p;
    }

    pub fn write(&self, bytes: &[u8]) {
        if self.is_exited() {
            return;
        }
        let mut w = self.writer.lock();
        let _ = w.write_all(bytes);
        let _ = w.flush();
    }

    pub fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }

    /// Foreground process group of the PTY.
    pub fn foreground_pgid(&self) -> Option<i32> {
        self.master.lock().process_group_leader()
    }

    pub fn key(&self, k: &KeyInput) -> bool {
        let mode = self.mode();
        match input::encode_key(k, mode) {
            Some(bytes) => {
                self.scroll_to_bottom_on_input();
                self.write(&bytes);
                true
            }
            None => false,
        }
    }

    pub fn text(&self, text: &str) {
        self.scroll_to_bottom_on_input();
        self.write(text.as_bytes());
    }

    fn scroll_to_bottom_on_input(&self) {
        let mut t = self.term.lock();
        if t.grid().display_offset() != 0 {
            t.scroll_display(Scroll::Bottom);
            self.dirty.store(true, Ordering::Release);
        }
    }

    pub fn paste(&self, text: &str) {
        let mode = self.mode();
        self.scroll_to_bottom_on_input();
        self.write(&input::encode_paste(text, mode));
    }

    pub fn focus(&self, focused: bool) {
        if self.mode().contains(TermMode::FOCUS_IN_OUT) {
            self.write(input::focus_report(focused));
        }
    }

    pub fn size(&self) -> (u16, u16) {
        let s = self.size.lock();
        (s.num_cols, s.num_lines)
    }

    pub fn resize(&self, cols: u16, rows: u16, cell_w: u16, cell_h: u16) {
        let cols = cols.max(2);
        let rows = rows.max(1);
        {
            let mut s = self.size.lock();
            if s.num_cols == cols
                && s.num_lines == rows
                && s.cell_width == cell_w
                && s.cell_height == cell_h
            {
                return;
            }
            *s = WindowSize {
                num_lines: rows,
                num_cols: cols,
                cell_width: cell_w,
                cell_height: cell_h,
            };
        }
        self.term
            .lock()
            .resize(TermSize::new(cols as usize, rows as usize));
        let _ = self.master.lock().resize(PtySize {
            rows,
            cols,
            pixel_width: cols * cell_w,
            pixel_height: rows * cell_h,
        });
        self.mark_dirty();
    }

    pub fn scroll(&self, to: ScrollTo) {
        let mut t = self.term.lock();
        let s = match to {
            ScrollTo::Top => Scroll::Top,
            ScrollTo::Bottom => Scroll::Bottom,
            ScrollTo::PageUp => Scroll::PageUp,
            ScrollTo::PageDown => Scroll::PageDown,
            ScrollTo::Lines(n) => Scroll::Delta(n),
        };
        t.scroll_display(s);
        self.mark_dirty();
    }

    fn viewport_point(t: &Term<Listener>, col: u16, row: u16) -> Point {
        let offset = t.grid().display_offset() as i32;
        let rows = t.screen_lines() as i32;
        let cols = t.columns();
        let row = (row as i32).clamp(0, rows - 1);
        Point::new(
            Line(row - offset),
            Column((col as usize).min(cols.saturating_sub(1))),
        )
    }

    /// Handles a mouse event. Returns text to put on the clipboard (copy-on-select).
    pub fn mouse(&self, ev: &MouseInput, copy_on_select: bool) -> Option<String> {
        let mut t = self.term.lock();
        let mode = *t.mode();
        let report = mode.intersects(TermMode::MOUSE_MODE) && !ev.mods.shift;
        let mut ms = self.mouse.lock();
        let mut out = None;
        match ev.kind {
            MouseKind::Wheel(d) => {
                if report {
                    drop(t);
                    for _ in 0..d.unsigned_abs().min(10) {
                        if let Some(b) = input::encode_mouse(ev, mode, ms.buttons_down) {
                            self.write(&b);
                        }
                    }
                    return None;
                } else if mode.contains(TermMode::ALT_SCREEN)
                    && mode.contains(TermMode::ALTERNATE_SCROLL)
                {
                    let app = mode.contains(TermMode::APP_CURSOR);
                    let seq: &[u8] = match (d > 0, app) {
                        (true, true) => b"\x1bOA",
                        (true, false) => b"\x1b[A",
                        (false, true) => b"\x1bOB",
                        (false, false) => b"\x1b[B",
                    };
                    let bytes: Vec<u8> = seq.repeat(d.unsigned_abs().min(20) as usize);
                    drop(t);
                    self.write(&bytes);
                    return None;
                } else {
                    t.scroll_display(Scroll::Delta(d));
                }
            }
            MouseKind::Press => {
                ms.buttons_down = true;
                if report {
                    drop(t);
                    if let Some(b) = input::encode_mouse(ev, mode, true) {
                        self.write(&b);
                    }
                    return None;
                }
                if ev.button == MouseButton::Left {
                    let p = Self::viewport_point(&t, ev.col, ev.row);
                    let side = if ev.right_half {
                        Side::Right
                    } else {
                        Side::Left
                    };
                    if ev.mods.shift && t.selection.is_some() {
                        if let Some(sel) = t.selection.as_mut() {
                            sel.update(p, side);
                        }
                    } else {
                        let ty = match ev.clicks {
                            2 => SelectionType::Semantic,
                            3.. => SelectionType::Lines,
                            _ if ev.mods.alt => SelectionType::Block,
                            _ => SelectionType::Simple,
                        };
                        t.selection = Some(Selection::new(ty, p, side));
                    }
                    ms.selecting = true;
                }
            }
            MouseKind::Move => {
                if report {
                    drop(t);
                    if let Some(b) = input::encode_mouse(ev, mode, ms.buttons_down) {
                        self.write(&b);
                    }
                    return None;
                }
                if ms.selecting {
                    let rows = t.screen_lines() as u16;
                    // Drag beyond the edges scrolls the viewport.
                    if ev.row == 0 {
                        t.scroll_display(Scroll::Delta(1));
                    } else if ev.row >= rows.saturating_sub(1) {
                        t.scroll_display(Scroll::Delta(-1));
                    }
                    let p = Self::viewport_point(&t, ev.col, ev.row);
                    let side = if ev.right_half {
                        Side::Right
                    } else {
                        Side::Left
                    };
                    if let Some(sel) = t.selection.as_mut() {
                        sel.update(p, side);
                    }
                }
            }
            MouseKind::Release => {
                ms.buttons_down = false;
                if report {
                    drop(t);
                    if let Some(b) = input::encode_mouse(ev, mode, false) {
                        self.write(&b);
                    }
                    return None;
                }
                if ms.selecting {
                    ms.selecting = false;
                    if t.selection.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
                        t.selection = None;
                    } else if copy_on_select {
                        out = t.selection_to_string();
                    }
                }
            }
        }
        drop(ms);
        drop(t);
        self.mark_dirty();
        out
    }

    pub fn selection_text(&self) -> Option<String> {
        self.term
            .lock()
            .selection_to_string()
            .filter(|s| !s.is_empty())
    }

    pub fn select_all(&self) {
        let mut t = self.term.lock();
        let start = Point::new(t.grid().topmost_line(), Column(0));
        let end = Point::new(t.grid().bottommost_line(), t.grid().last_column());
        let mut sel = Selection::new(SelectionType::Simple, start, Side::Left);
        sel.update(end, Side::Right);
        t.selection = Some(sel);
        drop(t);
        self.mark_dirty();
    }

    pub fn clear_selection(&self) {
        self.term.lock().selection = None;
        self.mark_dirty();
    }

    /// Clears scrollback and screen; the shell redraws its prompt (Ctrl+L) when idle.
    pub fn clear_screen(&self, shell_idle: bool) {
        {
            let mut t = self.term.lock();
            use alacritty_terminal::vte::ansi::Handler;
            t.clear_screen(ansi::ClearMode::Saved);
            if shell_idle {
                t.clear_screen(ansi::ClearMode::All);
                t.goto(0, 0);
            }
            t.selection = None;
        }
        if shell_idle {
            self.write(b"\x0c");
        }
        self.mark_dirty();
    }

    // ----- search -----

    pub fn search(&self, query: &str, regex: bool, forward: bool) -> SearchInfo {
        let mut guard = self.search.lock();
        if query.is_empty() {
            *guard = None;
            self.mark_dirty();
            return SearchInfo::default();
        }
        let mut t = self.term.lock();
        let changed = guard
            .as_ref()
            .map(|s| s.query != query || s.regex != regex)
            .unwrap_or(true);
        if changed {
            let pattern = if regex {
                query.to_string()
            } else {
                regex_escape(query)
            };
            let re = match RegexSearch::new(&pattern) {
                Ok(r) => r,
                Err(e) => {
                    *guard = None;
                    self.mark_dirty();
                    return SearchInfo {
                        query: query.into(),
                        regex,
                        error: Some(e.to_string()),
                        ..Default::default()
                    };
                }
            };
            let mut st = SearchState {
                query: query.into(),
                regex,
                re,
                current: None,
                total: 0,
                index: 0,
            };
            let top = Point::new(t.grid().topmost_line(), Column(0));
            let bottom = Point::new(t.grid().bottommost_line(), t.grid().last_column());
            st.total = RegexIter::new(top, bottom, TDir::Right, &t, &mut st.re)
                .take(MAX_COUNT as usize)
                .count() as u32;
            *guard = Some(st);
        }
        let st = guard.as_mut().unwrap();
        // "Next" walks toward older output (up), "previous" toward newer.
        let dir = if forward { TDir::Left } else { TDir::Right };
        let origin = match (&st.current, changed) {
            (Some(m), false) => {
                if forward {
                    m.start().sub(&*t, Boundary::None, 1)
                } else {
                    m.end().add(&*t, Boundary::None, 1)
                }
            }
            _ => {
                if forward {
                    Point::new(t.grid().bottommost_line(), t.grid().last_column())
                } else {
                    Point::new(t.grid().topmost_line(), Column(0))
                }
            }
        };
        if st.total > 0 {
            if let Some(m) = t.search_next(&mut st.re, origin, dir, Side::Left, None) {
                t.scroll_to_point(*m.start());
                let top = Point::new(t.grid().topmost_line(), Column(0));
                let before = RegexIter::new(top, *m.start(), TDir::Right, &t, &mut st.re)
                    .take(MAX_COUNT as usize)
                    .count() as u32;
                st.index = before.max(1);
                st.current = Some(m);
            }
        } else {
            st.current = None;
            st.index = 0;
        }
        let info = SearchInfo {
            query: st.query.clone(),
            regex: st.regex,
            total: st.total,
            current: st.index,
            error: None,
        };
        drop(t);
        self.mark_dirty();
        info
    }

    pub fn search_close(&self) {
        *self.search.lock() = None;
        self.mark_dirty();
    }

    pub fn search_info(&self) -> Option<SearchInfo> {
        self.search.lock().as_ref().map(|st| SearchInfo {
            query: st.query.clone(),
            regex: st.regex,
            total: st.total,
            current: st.index,
            error: None,
        })
    }

    // ----- rendering -----

    /// All visible rows plus cursor and mode; the daemon diffs rows per client.
    pub fn frame(&self) -> Frame {
        let mut search = self.search.lock();
        let t = self.term.lock();
        let cols = t.columns();
        let rows = t.screen_lines();
        let content = t.renderable_content();
        let offset = content.display_offset;
        let colors = content.colors;
        let sel = content.selection;
        let mut lines: Vec<Row> = vec![
            Row {
                cells: vec![Cell::default(); cols],
                extra: Vec::new()
            };
            rows
        ];
        for ic in content.display_iter {
            let vr = ic.point.line.0 + offset as i32;
            if vr < 0 || vr as usize >= rows || ic.point.column.0 >= cols {
                continue;
            }
            let cell = ic.cell;
            let mut f = map_flags(cell.flags);
            let (fg, dim_fg) = map_color(cell.fg, colors, true);
            let (bg, _) = map_color(cell.bg, colors, false);
            if dim_fg {
                f |= flags::DIM;
            }
            if sel.map(|s| s.contains(ic.point)).unwrap_or(false) {
                f |= flags::SELECTED;
            }
            if cell.hyperlink().is_some() {
                f |= flags::HYPERLINK;
            }
            let row = &mut lines[vr as usize];
            row.cells[ic.point.column.0] = Cell {
                ch: cell.c,
                fg,
                bg,
                flags: f,
            };
            if let Some(zw) = cell.zerowidth() {
                row.extra
                    .push((ic.point.column.0 as u16, zw.iter().collect()));
            }
        }
        // Search highlights in the viewport.
        let mut search_info = None;
        if let Some(st) = search.as_mut() {
            let top = Point::new(Line(-(offset as i32)), Column(0));
            let bottom = Point::new(
                Line(-(offset as i32) + rows as i32 - 1),
                Column(cols.saturating_sub(1)),
            );
            let current = st.current.clone();
            for m in RegexIter::new(top, bottom, TDir::Right, &t, &mut st.re).take(2000) {
                let is_current = current
                    .as_ref()
                    .map(|c| c.start() == m.start() && c.end() == m.end())
                    .unwrap_or(false);
                let mut p = *m.start();
                loop {
                    let vr = p.line.0 + offset as i32;
                    if vr >= 0 && (vr as usize) < rows && p.column.0 < cols {
                        lines[vr as usize].cells[p.column.0].flags |= if is_current {
                            flags::MATCH_CURRENT
                        } else {
                            flags::MATCH
                        };
                    }
                    if p >= *m.end() {
                        break;
                    }
                    p = p.add(&*t, Boundary::None, 1);
                }
            }
            search_info = Some(SearchInfo {
                query: st.query.clone(),
                regex: st.regex,
                total: st.total,
                current: st.index,
                error: None,
            });
        }
        let rc = content.cursor;
        let cursor_row = rc.point.line.0 + offset as i32;
        let shape = if cursor_row < 0 || cursor_row as usize >= rows {
            CursorShape::Hidden
        } else {
            match rc.shape {
                TShape::Block | TShape::HollowBlock => CursorShape::Block,
                TShape::Underline => CursorShape::Underline,
                TShape::Beam => CursorShape::Beam,
                TShape::Hidden => CursorShape::Hidden,
            }
        };
        let mode = content.mode;
        let mut m = 0;
        if mode.contains(TermMode::APP_CURSOR) {
            m |= proto::mode::APP_CURSOR;
        }
        if mode.intersects(TermMode::MOUSE_MODE) {
            m |= proto::mode::MOUSE;
        }
        if mode.contains(TermMode::MOUSE_MOTION) {
            m |= proto::mode::MOUSE_MOTION;
        }
        if mode.contains(TermMode::ALT_SCREEN) {
            m |= proto::mode::ALT_SCREEN;
        }
        if mode.contains(TermMode::BRACKETED_PASTE) {
            m |= proto::mode::BRACKETED_PASTE;
        }
        if mode.contains(TermMode::FOCUS_IN_OUT) {
            m |= proto::mode::FOCUS_EVENTS;
        }
        let history_size = t.grid().history_size() as u32;
        let blinking = t.cursor_style().blinking;
        drop(t);
        Frame {
            pane: self.id,
            cols: cols as u16,
            rows: rows as u16,
            full: true,
            lines: lines
                .into_iter()
                .enumerate()
                .map(|(i, r)| (i as u16, r))
                .collect(),
            cursor: Cursor {
                col: rc.point.column.0 as u16,
                row: cursor_row.max(0) as u16,
                shape,
                blinking,
            },
            display_offset: offset as u32,
            history_size,
            mode: m,
            search: search_info,
        }
    }

    /// Text of a viewport row (used for URL detection by clients and tests).
    pub fn row_text(&self, row: u16) -> String {
        let t = self.term.lock();
        let offset = t.grid().display_offset() as i32;
        let line = Line(row as i32 - offset);
        if line < t.grid().topmost_line() || line > t.grid().bottommost_line() {
            return String::new();
        }
        let r = &t.grid()[line];
        let mut s = String::new();
        for c in 0..t.columns() {
            let cell = &r[Column(c)];
            if cell.flags.contains(CF::WIDE_CHAR_SPACER) {
                continue;
            }
            s.push(cell.c);
        }
        s.trim_end().to_string()
    }

    /// Visible text of the whole screen (tests, debugging).
    pub fn screen_text(&self) -> String {
        let rows = self.term.lock().screen_lines() as u16;
        (0..rows)
            .map(|r| self.row_text(r))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Scrollback plus screen as ANSI text for restoring after restart.
    /// Returns `None` while the alternate screen is active (full-screen apps).
    pub fn export_scrollback(&self, max_lines: usize) -> Option<String> {
        let t = self.term.lock();
        if t.mode().contains(TermMode::ALT_SCREEN) {
            return None;
        }
        let grid = t.grid();
        let top = grid.topmost_line();
        let cursor_line = grid.cursor.point.line;
        // Last line with content, up to the cursor line.
        let mut last = cursor_line;
        while last > top {
            let r = &grid[last];
            if (0..t.columns()).any(|c| r[Column(c)].c != ' ') {
                break;
            }
            last -= 1;
        }
        let start = Line((last.0 - max_lines as i32 + 1).max(top.0));
        let colors = t.colors();
        let mut out = String::new();
        let mut cur = Sgr::default();
        let mut line = start;
        while line <= last {
            let r = &grid[line];
            let cols = t.columns();
            let mut end = cols;
            while end > 0 {
                let c = &r[Column(end - 1)];
                if c.c != ' ' || !matches!(c.bg, Color::Named(NamedColor::Background)) {
                    break;
                }
                end -= 1;
            }
            for c in 0..end {
                let cell = &r[Column(c)];
                if cell.flags.contains(CF::WIDE_CHAR_SPACER)
                    || cell.flags.contains(CF::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                let want = Sgr::of(cell, colors);
                if want != cur {
                    out.push_str(&want.escape());
                    cur = want;
                }
                out.push(cell.c);
                if let Some(zw) = cell.zerowidth() {
                    out.extend(zw.iter());
                }
            }
            let wrapped = cols > 0 && r[Column(cols - 1)].flags.contains(CF::WRAPLINE);
            if !wrapped || line == last {
                if cur != Sgr::default() {
                    out.push_str("\x1b[0m");
                    cur = Sgr::default();
                }
                out.push_str("\r\n");
            }
            line += 1;
        }
        Some(out)
    }

    /// Graceful shutdown: SIGHUP to the session (ssh closes cleanly), then SIGTERM, then SIGKILL.
    pub fn terminate(&self, hup_wait: Duration, term_wait: Duration) {
        if self.is_exited() || self.shell_pid <= 0 {
            return;
        }
        let fg = self.foreground_pgid();
        unsafe {
            if let Some(pg) = fg.filter(|p| *p > 0 && *p != self.shell_pid) {
                libc::killpg(pg, libc::SIGHUP);
            }
            libc::killpg(self.shell_pid, libc::SIGHUP);
            libc::kill(self.shell_pid, libc::SIGHUP);
        }
        if self.wait_exit(hup_wait) {
            return;
        }
        unsafe {
            libc::killpg(self.shell_pid, libc::SIGTERM);
            libc::kill(self.shell_pid, libc::SIGTERM);
        }
        if self.wait_exit(term_wait) {
            return;
        }
        unsafe {
            libc::killpg(self.shell_pid, libc::SIGKILL);
            libc::kill(self.shell_pid, libc::SIGKILL);
        }
        self.wait_exit(Duration::from_millis(500));
    }

    pub fn wait_exit(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.is_exited() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        self.is_exited()
    }
}

impl Drop for Pane {
    fn drop(&mut self) {
        if !self.is_exited() && self.shell_pid > 0 {
            unsafe {
                libc::killpg(self.shell_pid, libc::SIGHUP);
            }
        }
    }
}

fn wait_child(pid: i32) -> i32 {
    if pid <= 0 {
        return -1;
    }
    let mut status: libc::c_int = 0;
    for _ in 0..200 {
        let r = unsafe { libc::waitpid(pid, &mut status, 0) };
        if r == pid {
            if libc::WIFEXITED(status) {
                return libc::WEXITSTATUS(status);
            }
            if libc::WIFSIGNALED(status) {
                return 128 + libc::WTERMSIG(status);
            }
            return -1;
        }
        if r < 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return -1;
        }
    }
    -1
}

fn regex_escape(s: &str) -> String {
    regex::escape(s)
}

fn map_flags(f: CF) -> u16 {
    let mut o = 0;
    if f.contains(CF::BOLD) {
        o |= flags::BOLD;
    }
    if f.contains(CF::ITALIC) {
        o |= flags::ITALIC;
    }
    if f.contains(CF::UNDERLINE)
        || f.contains(CF::DOTTED_UNDERLINE)
        || f.contains(CF::DASHED_UNDERLINE)
    {
        o |= flags::UNDERLINE;
    }
    if f.contains(CF::DOUBLE_UNDERLINE) {
        o |= flags::DOUBLE_UNDERLINE;
    }
    if f.contains(CF::UNDERCURL) {
        o |= flags::UNDERCURL;
    }
    if f.contains(CF::STRIKEOUT) {
        o |= flags::STRIKE;
    }
    if f.contains(CF::DIM) {
        o |= flags::DIM;
    }
    if f.contains(CF::INVERSE) {
        o |= flags::INVERSE;
    }
    if f.contains(CF::HIDDEN) {
        o |= flags::HIDDEN;
    }
    if f.contains(CF::WIDE_CHAR) {
        o |= flags::WIDE;
    }
    if f.contains(CF::WIDE_CHAR_SPACER) || f.contains(CF::LEADING_WIDE_CHAR_SPACER) {
        o |= flags::SPACER;
    }
    o
}

/// Maps an emulator color to the wire encoding. The bool marks dim named colors.
fn map_color(c: Color, colors: &Colors, fg: bool) -> (u32, bool) {
    let rgb = |v: VRgb| proto::COLOR_RGB | ((v.r as u32) << 16) | ((v.g as u32) << 8) | v.b as u32;
    match c {
        Color::Spec(v) => (rgb(v), false),
        Color::Indexed(i) => match colors[i as usize] {
            Some(v) => (rgb(v), false),
            None => (proto::COLOR_INDEXED | i as u32, false),
        },
        Color::Named(n) => {
            let idx = n as usize;
            if let Some(v) = colors[idx] {
                return (rgb(v), false);
            }
            match n {
                NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::Cursor => {
                    (proto::COLOR_DEFAULT_FG, false)
                }
                NamedColor::DimForeground => (proto::COLOR_DEFAULT_FG, true),
                NamedColor::Background => (proto::COLOR_DEFAULT_BG, false),
                _ if idx < 16 => (proto::COLOR_INDEXED | idx as u32, false),
                _ => {
                    let base = idx - NamedColor::DimBlack as usize;
                    if base < 8 {
                        (proto::COLOR_INDEXED | base as u32, true)
                    } else if fg {
                        (proto::COLOR_DEFAULT_FG, false)
                    } else {
                        (proto::COLOR_DEFAULT_BG, false)
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
struct Sgr {
    fg: Option<SgrColor>,
    bg: Option<SgrColor>,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
    strike: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SgrColor {
    Idx(u8),
    Rgb(u8, u8, u8),
}

impl Sgr {
    fn color(c: Color, colors: &Colors) -> Option<SgrColor> {
        match c {
            Color::Spec(v) => Some(SgrColor::Rgb(v.r, v.g, v.b)),
            Color::Indexed(i) => Some(
                colors[i as usize]
                    .map(|v| SgrColor::Rgb(v.r, v.g, v.b))
                    .unwrap_or(SgrColor::Idx(i)),
            ),
            Color::Named(n) => {
                let i = n as usize;
                if i < 16 {
                    Some(SgrColor::Idx(i as u8))
                } else if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize)
                    .contains(&i)
                {
                    Some(SgrColor::Idx((i - NamedColor::DimBlack as usize) as u8))
                } else {
                    None
                }
            }
        }
    }

    fn of(cell: &alacritty_terminal::term::cell::Cell, colors: &Colors) -> Sgr {
        Sgr {
            fg: Self::color(cell.fg, colors),
            bg: Self::color(cell.bg, colors),
            bold: cell.flags.contains(CF::BOLD),
            dim: cell.flags.contains(CF::DIM),
            italic: cell.flags.contains(CF::ITALIC),
            underline: cell.flags.intersects(CF::ALL_UNDERLINES),
            inverse: cell.flags.contains(CF::INVERSE),
            strike: cell.flags.contains(CF::STRIKEOUT),
        }
    }

    fn escape(&self) -> String {
        let mut p = vec!["0".to_string()];
        if self.bold {
            p.push("1".into());
        }
        if self.dim {
            p.push("2".into());
        }
        if self.italic {
            p.push("3".into());
        }
        if self.underline {
            p.push("4".into());
        }
        if self.inverse {
            p.push("7".into());
        }
        if self.strike {
            p.push("9".into());
        }
        let col = |c: SgrColor, base: u8| match c {
            SgrColor::Idx(i) if i < 8 => format!("{}", base + i),
            SgrColor::Idx(i) if i < 16 => format!("{}", base + 60 + (i - 8)),
            SgrColor::Idx(i) => format!("{};5;{i}", base + 8),
            SgrColor::Rgb(r, g, b) => format!("{};2;{r};{g};{b}", base + 8),
        };
        if let Some(c) = self.fg {
            p.push(col(c, 30));
        }
        if let Some(c) = self.bg {
            p.push(col(c, 40));
        }
        format!("\x1b[{}m", p.join(";"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use at_core::config::IntegrationMode;

    fn opts(
        id: PaneId,
        shell: &str,
        args: Vec<String>,
        dir: &std::path::Path,
        preload: Option<Vec<u8>>,
    ) -> SpawnOptions {
        SpawnOptions {
            id,
            cols: 80,
            rows: 24,
            cell_w: 8,
            cell_h: 16,
            launch: ShellLaunch {
                shell: shell.into(),
                args,
                cwd: dir.to_path_buf(),
                env: vec![("PS1".into(), "$ ".into())],
                integration: IntegrationMode::Off,
                integration_dir: dir.join("shell"),
                histfile: None,
                history_name: None,
                wrap_ssh: false,
                term: "xterm-256color".into(),
                pane_id: id.to_string(),
            },
            scrollback_lines: 1000,
            word_separators: " ".into(),
            osc52: Osc52::Write,
            preload,
            palette: Palette::from_theme(&at_core::theme::Theme::dusk()),
        }
    }

    fn wait_for(p: &Pane, needle: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if p.screen_text().contains(needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn echo_resize_exit() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = crossbeam_channel::unbounded();
        let id = PaneId::new();
        let p = Pane::spawn(
            opts(
                id,
                "/bin/sh",
                vec![],
                dir.path(),
                Some(b"restored line\r\n".to_vec()),
            ),
            tx,
        )
        .unwrap();
        assert!(wait_for(&p, "restored line"));
        p.text("printf 'hello %s\\n' world\r");
        assert!(wait_for(&p, "hello world"), "{}", p.screen_text());
        p.resize(40, 10, 8, 16);
        p.text("stty size\r");
        assert!(wait_for(&p, "10 40"), "{}", p.screen_text());
        let f = p.frame();
        assert_eq!((f.cols, f.rows), (40, 10));
        let exported = p.export_scrollback(100).unwrap();
        assert!(exported.contains("hello world"));
        p.text("exit 3\r");
        assert!(p.wait_exit(Duration::from_secs(5)));
        assert_eq!(p.exit_code(), 3);
        let mut exited = false;
        while let Ok(ev) = rx.try_recv() {
            if let PaneEvent::Exited(_, 3) = ev {
                exited = true;
            }
        }
        assert!(exited);
    }

    #[test]
    fn search_and_select() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, _rx) = crossbeam_channel::unbounded();
        let p = Pane::spawn(opts(PaneId::new(), "/bin/sh", vec![], dir.path(), None), tx).unwrap();
        p.text("printf 'alpha\\nbeta\\nalpha\\n'\r");
        assert!(wait_for(&p, "beta"));
        let info = p.search("alpha", false, true);
        // The typed command line also contains "alpha".
        assert!(info.total >= 2, "{info:?}");
        assert!(info.current >= 1);
        let f = p.frame();
        assert!(
            f.lines
                .iter()
                .any(|(_, r)| r.cells.iter().any(|c| c.flags & flags::MATCH_CURRENT != 0))
        );
        p.select_all();
        assert!(p.selection_text().unwrap().contains("beta"));
        p.terminate(Duration::from_secs(2), Duration::from_secs(1));
        assert!(p.is_exited());
    }

    #[test]
    fn terminate_kills_hung_child() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, _rx) = crossbeam_channel::unbounded();
        let p = Pane::spawn(
            opts(
                PaneId::new(),
                "/bin/sh",
                vec!["-c".into(), "trap '' HUP; sleep 100".into()],
                dir.path(),
                None,
            ),
            tx,
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        let t0 = Instant::now();
        p.terminate(Duration::from_millis(300), Duration::from_millis(300));
        assert!(p.is_exited());
        assert!(t0.elapsed() < Duration::from_secs(3));
    }
}
