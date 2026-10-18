//! Terminal grid rendering and mouse handling on an iced canvas.

use crate::Message;
use crate::metrics::{Fonts, Metrics};
use crate::style::c;
use at_core::PaneId;
use at_core::config::CursorStyle;
use at_core::theme::{Rgb, Theme};
use at_proto::{
    Cell, Cursor, CursorShape, Mods, MouseButton, MouseInput, MouseKind, Row, SearchInfo, flags,
};
use iced::mouse;
use iced::widget::canvas::{self, Event, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Pixels, Point, Rectangle, Renderer, Size};
use std::time::{Duration, Instant};

/// Client-side copy of a pane's screen.
pub struct PaneGrid {
    pub cols: u16,
    pub rows: u16,
    pub lines: Vec<Row>,
    pub cursor: Cursor,
    pub display_offset: u32,
    pub history_size: u32,
    pub mode: u32,
    pub search: Option<SearchInfo>,
    pub cache: canvas::Cache,
}

impl Default for PaneGrid {
    fn default() -> Self {
        Self {
            cols: 0,
            rows: 0,
            lines: Vec::new(),
            cursor: Cursor::default(),
            display_offset: 0,
            history_size: 0,
            mode: 0,
            search: None,
            cache: canvas::Cache::new(),
        }
    }
}

impl PaneGrid {
    pub fn apply(&mut self, f: at_proto::Frame) {
        if f.full || self.lines.len() != f.rows as usize || self.cols != f.cols {
            self.lines = vec![
                Row {
                    cells: vec![Cell::default(); f.cols as usize],
                    extra: vec![]
                };
                f.rows as usize
            ];
        }
        self.cols = f.cols;
        self.rows = f.rows;
        for (i, r) in f.lines {
            if let Some(slot) = self.lines.get_mut(i as usize) {
                *slot = r;
            }
        }
        self.cursor = f.cursor;
        self.display_offset = f.display_offset;
        self.history_size = f.history_size;
        self.mode = f.mode;
        self.search = f.search;
        self.cache.clear();
    }

    pub fn row_text(&self, row: usize) -> String {
        self.lines.get(row).map(|r| r.text()).unwrap_or_default()
    }
}

pub struct Style<'a> {
    pub theme: &'a Theme,
    pub fonts: &'a Fonts,
    pub metrics: Metrics,
    pub padding: f32,
    pub cursor_style: CursorStyle,
    pub bold_is_bright: bool,
    pub ligatures: bool,
}

pub struct TermCanvas<'a> {
    pub pane: PaneId,
    pub grid: Option<&'a PaneGrid>,
    pub style: Style<'a>,
    pub focused: bool,
    pub blink_off: bool,
    pub bell: bool,
    pub mods: iced::keyboard::Modifiers,
    pub link_modifier: bool,
}

#[derive(Default)]
pub struct CanvasState {
    pressed: Option<MouseButton>,
    last_click: Option<(Instant, u16, u16)>,
    clicks: u8,
    scroll_acc: f32,
    last_cell: Option<(u16, u16)>,
}

fn resolve(theme: &Theme, v: u32, bold_bright: bool) -> Color {
    let rgb = match v & at_proto::COLOR_TAG_MASK {
        at_proto::COLOR_RGB => Rgb::from_u32(v & 0xFF_FFFF),
        at_proto::COLOR_INDEXED => {
            let mut i = (v & 0xFF) as u8;
            if bold_bright && i < 8 {
                i += 8;
            }
            theme.indexed(i)
        }
        at_proto::COLOR_DEFAULT_BG => theme.ui.bg,
        _ => theme.ui.fg,
    };
    c(rgb)
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: 1.0,
    }
}

impl TermCanvas<'_> {
    fn cell_at(&self, p: Point) -> (u16, u16, bool) {
        let m = self.style.metrics;
        let x = (p.x - self.style.padding).max(0.0);
        let y = (p.y - self.style.padding).max(0.0);
        let col = (x / m.cell_w).floor();
        let row = (y / m.cell_h).floor();
        let cols = self.grid.map(|g| g.cols).unwrap_or(1).max(1);
        let rows = self.grid.map(|g| g.rows).unwrap_or(1).max(1);
        let right = (x - col * m.cell_w) > m.cell_w / 2.0;
        (
            (col as u16).min(cols - 1),
            (row as u16).min(rows - 1),
            right,
        )
    }

    fn mods(&self) -> Mods {
        Mods {
            ctrl: self.mods.control(),
            alt: self.mods.alt(),
            shift: self.mods.shift(),
            logo: self.mods.logo(),
        }
    }

    /// Foreground and background of a cell after inverse/selection/search.
    fn colors(&self, cell: &Cell) -> (Color, Color) {
        let t = self.style.theme;
        let bold_bright = self.style.bold_is_bright && cell.flags & flags::BOLD != 0;
        let mut fg = resolve(t, cell.fg, bold_bright);
        let mut bg = if cell.bg & at_proto::COLOR_TAG_MASK == at_proto::COLOR_DEFAULT_FG {
            c(t.ui.fg)
        } else {
            resolve(t, cell.bg, false)
        };
        if cell.flags & flags::INVERSE != 0 {
            std::mem::swap(&mut fg, &mut bg);
        }
        if cell.flags & flags::DIM != 0 {
            fg = mix(fg, bg, 0.4);
        }
        if cell.flags & flags::HIDDEN != 0 {
            fg = bg;
        }
        if cell.flags & flags::MATCH_CURRENT != 0 {
            bg = c(t.ui.search_current);
        } else if cell.flags & flags::MATCH != 0 {
            bg = c(t.ui.search_match);
        } else if cell.flags & flags::SELECTED != 0 {
            bg = c(t.ui.selection);
        }
        (fg, bg)
    }

    fn font(&self, f: u16) -> iced::Font {
        let fonts = self.style.fonts;
        match (f & flags::BOLD != 0, f & flags::ITALIC != 0) {
            (true, true) => fonts.bold_italic,
            (true, false) => fonts.bold,
            (false, true) => fonts.italic,
            _ => fonts.regular,
        }
    }

    fn text(&self, content: String, x: f32, y: f32, color: Color, f: u16) -> Text {
        let m = self.style.metrics;
        Text {
            content,
            position: Point::new(x, y),
            max_width: f32::INFINITY,
            color,
            size: Pixels(m.font_size),
            line_height: iced::widget::text::LineHeight::Absolute(Pixels(m.cell_h)),
            font: self.font(f),
            align_x: iced::advanced::text::Alignment::Left,
            align_y: iced::alignment::Vertical::Top,
            shaping: if self.style.ligatures {
                iced::widget::text::Shaping::Advanced
            } else {
                iced::widget::text::Shaping::Basic
            },
        }
    }

    fn draw_cells(&self, frame: &mut Frame, grid: &PaneGrid, size: Size) {
        let t = self.style.theme;
        let m = self.style.metrics;
        let pad = self.style.padding;
        let default_bg = c(t.ui.bg);
        frame.fill_rectangle(Point::ORIGIN, size, default_bg);
        for (r, row) in grid.lines.iter().enumerate() {
            let y = pad + r as f32 * m.cell_h;
            let cells = &row.cells;
            let n = cells.len();
            // Backgrounds, merged into runs.
            let mut col = 0;
            while col < n {
                let (_, bg) = self.colors(&cells[col]);
                if bg == default_bg {
                    col += 1;
                    continue;
                }
                let start = col;
                while col < n && self.colors(&cells[col]).1 == bg {
                    col += 1;
                }
                frame.fill_rectangle(
                    Point::new(pad + start as f32 * m.cell_w, y),
                    Size::new((col - start) as f32 * m.cell_w, m.cell_h),
                    bg,
                );
            }
            // Text runs: same style, ASCII only; anything else is drawn per cell.
            let mut col = 0;
            while col < n {
                let cell = cells[col];
                if cell.flags & flags::SPACER != 0
                    || (cell.ch == ' '
                        && cell.flags
                            & (flags::UNDERLINE
                                | flags::STRIKE
                                | flags::DOUBLE_UNDERLINE
                                | flags::UNDERCURL)
                            == 0)
                {
                    col += 1;
                    continue;
                }
                let (fg, _) = self.colors(&cell);
                let style_flags = cell.flags & (flags::BOLD | flags::ITALIC);
                let has_extra = row.extra.iter().any(|(c, _)| *c as usize == col);
                let x = pad + col as f32 * m.cell_w;
                if !cell.ch.is_ascii() || has_extra {
                    let mut s = String::new();
                    s.push(cell.ch);
                    for (c, z) in &row.extra {
                        if *c as usize == col {
                            s.push_str(z);
                        }
                    }
                    let width = if cell.flags & flags::WIDE != 0 { 2 } else { 1 };
                    if is_box_drawing(cell.ch) {
                        draw_box(frame, cell.ch, x, y, m.cell_w * width as f32, m.cell_h, fg);
                    } else {
                        frame.fill_text(self.text(s, x, y, fg, style_flags));
                    }
                    self.decorations(frame, cell.flags, x, y, width as f32 * m.cell_w, fg);
                    col += width;
                    continue;
                }
                let start = col;
                let mut s = String::new();
                while col < n {
                    let cc = cells[col];
                    if !cc.ch.is_ascii()
                        || cc.flags & flags::SPACER != 0
                        || (cc.flags & (flags::BOLD | flags::ITALIC)) != style_flags
                        || self.colors(&cc).0 != fg
                        || (cc.flags
                            & (flags::UNDERLINE
                                | flags::STRIKE
                                | flags::DOUBLE_UNDERLINE
                                | flags::UNDERCURL))
                            != (cell.flags
                                & (flags::UNDERLINE
                                    | flags::STRIKE
                                    | flags::DOUBLE_UNDERLINE
                                    | flags::UNDERCURL))
                        || row.extra.iter().any(|(c, _)| *c as usize == col)
                    {
                        break;
                    }
                    s.push(cc.ch);
                    col += 1;
                }
                let trimmed = s.trim_end();
                if !trimmed.is_empty() {
                    frame.fill_text(self.text(trimmed.to_string(), x, y, fg, style_flags));
                }
                self.decorations(frame, cell.flags, x, y, (col - start) as f32 * m.cell_w, fg);
            }
        }
    }

    fn decorations(&self, frame: &mut Frame, f: u16, x: f32, y: f32, w: f32, color: Color) {
        let m = self.style.metrics;
        let thickness = (m.font_size / 14.0).max(1.0).round();
        if f & (flags::UNDERLINE | flags::UNDERCURL) != 0 {
            frame.fill_rectangle(
                Point::new(x, y + m.cell_h - thickness - 1.0),
                Size::new(w, thickness),
                color,
            );
        }
        if f & flags::DOUBLE_UNDERLINE != 0 {
            frame.fill_rectangle(
                Point::new(x, y + m.cell_h - thickness - 1.0),
                Size::new(w, thickness),
                color,
            );
            frame.fill_rectangle(
                Point::new(x, y + m.cell_h - 3.0 * thickness - 1.0),
                Size::new(w, thickness),
                color,
            );
        }
        if f & flags::STRIKE != 0 {
            frame.fill_rectangle(
                Point::new(x, y + m.cell_h / 2.0),
                Size::new(w, thickness),
                color,
            );
        }
    }

    fn draw_cursor(&self, frame: &mut Frame, grid: &PaneGrid) {
        let cur = grid.cursor;
        if cur.shape == CursorShape::Hidden {
            return;
        }
        let m = self.style.metrics;
        let t = self.style.theme;
        let pad = self.style.padding;
        let Some(row) = grid.lines.get(cur.row as usize) else {
            return;
        };
        let cell = row.cells.get(cur.col as usize).copied().unwrap_or_default();
        let wide = cell.flags & flags::WIDE != 0;
        let x = pad + cur.col as f32 * m.cell_w;
        let y = pad + cur.row as f32 * m.cell_h;
        let w = if wide { 2.0 * m.cell_w } else { m.cell_w };
        let color = c(t.ui.cursor);
        if !self.focused {
            let path = Path::rectangle(
                Point::new(x + 0.5, y + 0.5),
                Size::new(w - 1.0, m.cell_h - 1.0),
            );
            frame.stroke(&path, Stroke::default().with_color(color).with_width(1.0));
            return;
        }
        if self.blink_off {
            return;
        }
        // The application's DECSCUSR shape wins over the configured default block.
        let shape = match (cur.shape, self.style.cursor_style) {
            (CursorShape::Block, CursorStyle::Underline) => CursorShape::Underline,
            (CursorShape::Block, CursorStyle::Bar) => CursorShape::Beam,
            (s, _) => s,
        };
        match shape {
            CursorShape::Block => {
                frame.fill_rectangle(Point::new(x, y), Size::new(w, m.cell_h), color);
                if cell.ch != ' ' && cell.flags & flags::SPACER == 0 {
                    let mut s = String::new();
                    s.push(cell.ch);
                    frame.fill_text(self.text(
                        s,
                        x,
                        y,
                        c(t.ui.bg),
                        cell.flags & (flags::BOLD | flags::ITALIC),
                    ));
                }
            }
            CursorShape::Underline => {
                frame.fill_rectangle(Point::new(x, y + m.cell_h - 2.0), Size::new(w, 2.0), color)
            }
            CursorShape::Beam => {
                frame.fill_rectangle(Point::new(x, y), Size::new(2.0, m.cell_h), color)
            }
            CursorShape::Hidden => {}
        }
    }
}

fn is_box_drawing(ch: char) -> bool {
    matches!(
        ch,
        '─' | '│'
            | '┌'
            | '┐'
            | '└'
            | '┘'
            | '├'
            | '┤'
            | '┬'
            | '┴'
            | '┼'
            | '━'
            | '┃'
            | '█'
            | '▀'
            | '▄'
            | '▌'
            | '▐'
    )
}

/// Box-drawing characters are drawn as geometry so lines connect across cells.
fn draw_box(frame: &mut Frame, ch: char, x: f32, y: f32, w: f32, h: f32, color: Color) {
    let t = if matches!(ch, '━' | '┃') {
        2.0
    } else {
        1.0
    };
    let cx = (x + w / 2.0 - t / 2.0).round();
    let cy = (y + h / 2.0 - t / 2.0).round();
    let hline = |frame: &mut Frame, x0: f32, x1: f32| {
        frame.fill_rectangle(Point::new(x0, cy), Size::new(x1 - x0, t), color)
    };
    let vline = |frame: &mut Frame, y0: f32, y1: f32| {
        frame.fill_rectangle(Point::new(cx, y0), Size::new(t, y1 - y0), color)
    };
    match ch {
        '─' | '━' => hline(frame, x, x + w),
        '│' | '┃' => vline(frame, y, y + h),
        '┌' => {
            hline(frame, cx, x + w);
            vline(frame, cy, y + h);
        }
        '┐' => {
            hline(frame, x, cx + t);
            vline(frame, cy, y + h);
        }
        '└' => {
            hline(frame, cx, x + w);
            vline(frame, y, cy + t);
        }
        '┘' => {
            hline(frame, x, cx + t);
            vline(frame, y, cy + t);
        }
        '├' => {
            vline(frame, y, y + h);
            hline(frame, cx, x + w);
        }
        '┤' => {
            vline(frame, y, y + h);
            hline(frame, x, cx);
        }
        '┬' => {
            hline(frame, x, x + w);
            vline(frame, cy, y + h);
        }
        '┴' => {
            hline(frame, x, x + w);
            vline(frame, y, cy);
        }
        '┼' => {
            hline(frame, x, x + w);
            vline(frame, y, y + h);
        }
        '█' => frame.fill_rectangle(Point::new(x, y), Size::new(w, h), color),
        '▀' => frame.fill_rectangle(Point::new(x, y), Size::new(w, h / 2.0), color),
        '▄' => frame.fill_rectangle(Point::new(x, y + h / 2.0), Size::new(w, h / 2.0), color),
        '▌' => frame.fill_rectangle(Point::new(x, y), Size::new(w / 2.0, h), color),
        '▐' => frame.fill_rectangle(Point::new(x + w / 2.0, y), Size::new(w / 2.0, h), color),
        _ => {}
    }
}

impl canvas::Program<Message> for TermCanvas<'_> {
    type State = CanvasState;

    fn update(
        &self,
        state: &mut CanvasState,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let pane = self.pane;
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(b)) => {
                let p = cursor.position_in(bounds)?;
                let (col, row, right) = self.cell_at(p);
                let button = match b {
                    mouse::Button::Left => MouseButton::Left,
                    mouse::Button::Middle => MouseButton::Middle,
                    mouse::Button::Right => MouseButton::Right,
                    _ => return None,
                };
                // ⌘-click (Ctrl on Linux) opens links.
                if button == MouseButton::Left
                    && self.link_modifier
                    && let Some(grid) = self.grid
                {
                    let line = grid.row_text(row as usize);
                    if let Some(link) = crate::links::find_at(&line, col as usize) {
                        return Some(
                            canvas::Action::publish(Message::OpenLink(link)).and_capture(),
                        );
                    }
                }
                let now = Instant::now();
                state.clicks = match state.last_click {
                    Some((t, c, r))
                        if now.duration_since(t) < Duration::from_millis(400)
                            && c.abs_diff(col) <= 1
                            && r == row =>
                    {
                        (state.clicks % 3) + 1
                    }
                    _ => 1,
                };
                state.last_click = Some((now, col, row));
                state.pressed = Some(button);
                state.last_cell = Some((col, row));
                let input = MouseInput {
                    kind: MouseKind::Press,
                    button,
                    col,
                    row,
                    right_half: right,
                    clicks: state.clicks,
                    mods: self.mods(),
                };
                Some(canvas::Action::publish(Message::PaneMouse(pane, input)).and_capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let reporting_motion = self
                    .grid
                    .map(|g| g.mode & at_proto::mode::MOUSE_MOTION != 0)
                    .unwrap_or(false);
                if state.pressed.is_none() && !reporting_motion {
                    return None;
                }
                let pos = cursor.position()?;
                if state.pressed.is_none() && !bounds.contains(pos) {
                    return None;
                }
                let local = Point::new(
                    (pos.x - bounds.x).clamp(0.0, bounds.width - 1.0),
                    (pos.y - bounds.y).clamp(0.0, bounds.height - 1.0),
                );
                let (col, row, right) = self.cell_at(local);
                // Above/below the pane while dragging: report edge rows so the daemon autoscrolls.
                let row = if pos.y < bounds.y { 0 } else { row };
                if state.last_cell == Some((col, row))
                    && pos.y >= bounds.y
                    && pos.y < bounds.y + bounds.height
                {
                    return None;
                }
                state.last_cell = Some((col, row));
                let input = MouseInput {
                    kind: MouseKind::Move,
                    button: state.pressed.unwrap_or(MouseButton::None),
                    col,
                    row,
                    right_half: right,
                    clicks: state.clicks,
                    mods: self.mods(),
                };
                Some(canvas::Action::publish(Message::PaneMouse(pane, input)))
            }
            Event::Mouse(mouse::Event::ButtonReleased(_)) => {
                let button = state.pressed.take()?;
                let pos = cursor.position().unwrap_or(Point::new(bounds.x, bounds.y));
                let local = Point::new(
                    (pos.x - bounds.x).clamp(0.0, bounds.width - 1.0),
                    (pos.y - bounds.y).clamp(0.0, bounds.height - 1.0),
                );
                let (col, row, right) = self.cell_at(local);
                let input = MouseInput {
                    kind: MouseKind::Release,
                    button,
                    col,
                    row,
                    right_half: right,
                    clicks: state.clicks,
                    mods: self.mods(),
                };
                Some(canvas::Action::publish(Message::PaneMouse(pane, input)))
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let p = cursor.position_in(bounds)?;
                let (col, row, _) = self.cell_at(p);
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => {
                        state.scroll_acc += y * 3.0;
                        let l = state.scroll_acc.trunc();
                        state.scroll_acc -= l;
                        l as i32
                    }
                    mouse::ScrollDelta::Pixels { y, .. } => {
                        state.scroll_acc += y / self.style.metrics.cell_h;
                        let l = state.scroll_acc.trunc();
                        state.scroll_acc -= l;
                        l as i32
                    }
                };
                if lines == 0 {
                    return Some(canvas::Action::capture());
                }
                let input = MouseInput {
                    kind: MouseKind::Wheel(lines),
                    button: MouseButton::None,
                    col,
                    row,
                    right_half: false,
                    clicks: 0,
                    mods: self.mods(),
                };
                Some(canvas::Action::publish(Message::PaneMouse(pane, input)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &CanvasState,
        renderer: &Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let size = bounds.size();
        let Some(grid) = self.grid else {
            let mut f = Frame::new(renderer, size);
            f.fill_rectangle(Point::ORIGIN, size, c(self.style.theme.ui.bg));
            return vec![f.into_geometry()];
        };
        let cells = grid
            .cache
            .draw(renderer, size, |frame| self.draw_cells(frame, grid, size));
        let mut overlay = Frame::new(renderer, size);
        self.draw_cursor(&mut overlay, grid);
        if self.bell {
            let path = Path::rectangle(
                Point::new(1.0, 1.0),
                Size::new(size.width - 2.0, size.height - 2.0),
            );
            overlay.stroke(
                &path,
                Stroke::default()
                    .with_color(c(self.style.theme.ui.accent_warn))
                    .with_width(2.0),
            );
        }
        // Scroll position hint while looking at history.
        if grid.display_offset > 0 {
            let total = (grid.history_size + grid.rows as u32).max(1) as f32;
            let h = (size.height * grid.rows as f32 / total).max(20.0);
            let top = (size.height - h)
                * (1.0 - grid.display_offset as f32 / grid.history_size.max(1) as f32);
            overlay.fill_rectangle(
                Point::new(size.width - 4.0, top),
                Size::new(3.0, h),
                Color {
                    a: 0.5,
                    ..c(self.style.theme.ui.fg_dim)
                },
            );
        }
        vec![cells, overlay.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &CanvasState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            if self.link_modifier {
                mouse::Interaction::Pointer
            } else {
                mouse::Interaction::Text
            }
        } else {
            mouse::Interaction::default()
        }
    }
}
