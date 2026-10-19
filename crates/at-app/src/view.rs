//! Window views: sidebar, pane area, overlays.

use crate::app::{App, Confirm, DIVIDER, MenuItem, Message, Overlay, Picker, ROW_H, SIDEBAR_TOP};
use crate::layout::{self, SplitKind};
use crate::style::{self, c, ca};
use crate::term_view::{Style as TStyle, TermCanvas};
use at_core::keymap::Action;
use at_core::model::{DropPosition, GroupMode};
use at_core::pane::{PaneState, tilde_path};
use at_core::{PaneId, TabId};
use iced::widget::{
    Canvas, Column, Stack, button, column, container, mouse_area, opaque, pin, row, scrollable,
    space, stack, text, text_input,
};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Padding, Rectangle, mouse, window,
};

type El<'a> = Element<'a, Message>;

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

impl App {
    pub fn title(&self, id: window::Id) -> String {
        self.window_title(id)
    }

    pub fn theme_for(&self, _id: window::Id) -> iced::Theme {
        self.iced_theme.clone()
    }

    pub fn view(&self, id: window::Id) -> El<'_> {
        let Some(w) = self.windows.get(&id) else {
            return space().into();
        };
        let t = &self.theme;
        let sw = self.sidebar_width(id);
        let mut base = row![];
        if sw > 0.0 {
            base = base.push(self.sidebar(id, sw));
            base = base.push(
                container(space())
                    .width(DIVIDER)
                    .height(Length::Fill)
                    .style(move |_| style::bg(c(t.ui.border))),
            );
        }
        base = base.push(self.pane_area_view(id));
        let mut layers: Vec<El<'_>> = vec![
            container(base)
                .style(move |_| style::bg(c(t.ui.bg)))
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ];
        if sw > 0.0 {
            // Wider invisible handle for resizing the sidebar.
            let handle = mouse_area(container(space()).width(6.0).height(Length::Fill))
                .on_press(Message::SidebarResizeStart(id))
                .interaction(mouse::Interaction::ResizingHorizontally);
            layers.push(pin(handle).x(sw - 3.0).y(0.0).into());
        }
        if let Some((tab, at)) = w.menu {
            layers.push(self.context_menu(id, tab, at));
        }
        if let Some(d) = &w.drag
            && d.active
        {
            let label = truncate(&self.tab_title(d.tab), 28);
            let ghost = container(text(label).size(12).color(c(t.ui.fg)))
                .padding([4, 10])
                .style(move |_| style::menu(t));
            layers.push(pin(ghost).x(w.cursor.x + 12.0).y(w.cursor.y + 4.0).into());
        }
        match &w.overlay {
            Some(Overlay::Picker(p)) => layers.push(self.picker_view(id, p)),
            Some(Overlay::Confirm(cf)) => layers.push(self.confirm_view(id, cf)),
            Some(Overlay::Settings(st)) => layers.push(self.backdrop(
                id,
                st.view(self).map(move |m| Message::Settings(id, m)),
                760.0,
            )),
            None => {}
        }
        if !self.toasts.is_empty() {
            let col = Column::with_children(self.toasts.iter().map(|(s, _)| {
                container(text(s.clone()).size(12).color(c(t.ui.fg)))
                    .padding([6, 12])
                    .style(move |_| style::menu(t))
                    .into()
            }))
            .spacing(6)
            .align_x(Alignment::Center);
            layers.push(
                container(col)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::Center)
                    .align_y(Alignment::End)
                    .padding(18)
                    .into(),
            );
        }
        Stack::with_children(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    // ----- sidebar -----

    fn indicator(&self, state: PaneState) -> (String, Color) {
        let t = &self.theme;
        match state {
            PaneState::Attention => ("●".into(), c(t.ui.accent_warn)),
            PaneState::Remote => ("⇅".into(), c(t.ui.accent_remote)),
            PaneState::Running => {
                let frames = ["◐", "◓", "◑", "◒"];
                let f = if self.config.appearance.animate_indicators {
                    frames[(self.spin / 2) % 4]
                } else {
                    "◐"
                };
                (f.into(), c(t.ui.accent_info))
            }
            PaneState::Exited => ("○".into(), c(t.ui.fg_dim)),
            PaneState::Idle => (String::new(), c(t.ui.fg_dim)),
        }
    }

    fn sidebar(&self, id: window::Id, width: f32) -> El<'_> {
        let t = &self.theme;
        let w = &self.windows[&id];
        let Some(node) = self.ws.window(w.model) else {
            return space().into();
        };
        let rows = self.ws.visible_tabs(w.model, self.expand_single());
        let indent = self.config.appearance.indent_per_level;
        let drag_target = w.drag.as_ref().filter(|d| d.active).and_then(|d| d.target);
        let mut list = column![].spacing(0);
        for v in rows {
            let tab = v.id;
            let Some(tn) = self.ws.tab(tab) else { continue };
            // A collapsed group row stands in for its active tab.
            let active = node.active_tab == Some(tab)
                || (v.is_group
                    && v.collapsed
                    && node.active_tab.and_then(|a| self.ws.group_of(a)) == Some(tab));
            let hovered = w.hover == Some(tab);
            let state = self.tab_state(tab);
            let (glyph, gcolor) = self.indicator(state);
            let title = self.tab_title(tab);
            let max_chars = ((width - indent * v.depth as f32 - 78.0) / 7.2).max(4.0) as usize;
            let chevron: El<'_> = if v.has_children {
                button(
                    text(if v.collapsed { "▸" } else { "▾" })
                        .size(11)
                        .color(c(t.ui.fg_muted)),
                )
                .on_press(Message::ToggleCollapse(tab))
                .padding([0, 2])
                .style(style::icon_button(t))
                .width(14.0)
                .into()
            } else {
                space().width(14.0).into()
            };
            let ind_tip = match state {
                PaneState::Remote => tn
                    .panes
                    .iter()
                    .filter_map(|p| self.infos.get(p))
                    .find_map(|i| i.remote_host.clone()),
                _ => None,
            };
            let ind: El<'_> = {
                let tx = text(glyph).size(12).color(gcolor).width(14.0);
                match ind_tip {
                    Some(h) => iced::widget::tooltip(
                        tx,
                        container(text(h).size(11))
                            .padding([2, 6])
                            .style(move |_| style::menu(t)),
                        iced::widget::tooltip::Position::Right,
                    )
                    .into(),
                    None => tx.into(),
                }
            };
            let title_el: El<'_> = match &w.rename {
                Some((rt, value)) if *rt == tab => text_input("", value)
                    .id("rename-input")
                    .on_input(move |s| Message::RenameInput(id, s))
                    .on_submit(Message::RenameCommit(id))
                    .size(13)
                    .padding([2, 4])
                    .style(style::input(t))
                    .into(),
                _ => text(truncate(&title, max_chars))
                    .size(13)
                    .color(if active { c(t.ui.fg) } else { c(t.ui.fg_muted) })
                    .wrapping(iced::widget::text::Wrapping::None)
                    .width(Length::Fill)
                    .into(),
            };
            let mut r = row![
                space().width(indent * v.depth as f32),
                chevron,
                ind,
                title_el
            ]
            .spacing(4)
            .align_y(Alignment::Center);
            if v.is_group {
                r = r.push(
                    text(format!("{}", tn.children.len()))
                        .size(10)
                        .color(c(t.ui.fg_dim)),
                );
            }
            if v.is_group && tn.group_view && tn.children.len() > 1 {
                let glyph = if tn.group_tiles { "▦" } else { "▤" };
                r = r.push(text(glyph).size(12).color(c(t.ui.accent)));
            }
            if tn.panes.len() > 1 {
                let idx = tn.active_index() + 1;
                r = r.push(
                    text(format!("{idx}/{}", tn.panes.len()))
                        .size(10)
                        .color(c(t.ui.fg_dim)),
                );
            }
            if hovered || active {
                r = r.push(
                    button(text("×").size(13).color(c(t.ui.fg_muted)))
                        .on_press(Message::CloseTab(id, tab))
                        .padding([0, 4])
                        .style(style::icon_button(t)),
                );
            } else {
                r = r.push(space().width(18.0));
            }
            let profile_color = {
                let p = self.config.profile(&tn.profile);
                (!p.color.is_empty())
                    .then(|| self.theme.color(&p.color))
                    .flatten()
            };
            let bar_color = if active {
                profile_color.map(c).unwrap_or(c(t.ui.accent))
            } else {
                profile_color
                    .map(|x| ca(x, 0.7))
                    .unwrap_or(Color::TRANSPARENT)
            };
            let target = drag_target.filter(|(tt, _)| *tt == tab).map(|(_, p)| p);
            let bg = if target == Some(DropPosition::Child) {
                Some(Background::Color(ca(t.ui.accent, 0.25)))
            } else if active {
                Some(Background::Color(c(t.ui.surface_active)))
            } else if hovered {
                Some(Background::Color(c(t.ui.surface_hover)))
            } else {
                None
            };
            let line_top = target == Some(DropPosition::Before);
            let line_bottom = target == Some(DropPosition::After);
            let accent = c(t.ui.accent);
            let row_el = container(
                row![
                    container(space())
                        .width(3.0)
                        .height(ROW_H - 8.0)
                        .style(move |_| style::bg(bar_color)),
                    r
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .height(ROW_H)
            .width(Length::Fill)
            .padding(Padding {
                top: 0.0,
                right: 6.0,
                bottom: 0.0,
                left: 2.0,
            })
            .align_y(Alignment::Center)
            .style(move |_| container::Style {
                background: bg,
                border: Border {
                    radius: 6.0.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
                ..Default::default()
            });
            let mut cell = column![];
            if line_top {
                cell = cell.push(
                    container(space())
                        .height(2.0)
                        .width(Length::Fill)
                        .style(move |_| style::bg(accent)),
                );
            }
            cell = cell.push(row_el);
            if line_bottom {
                cell = cell.push(
                    container(space())
                        .height(2.0)
                        .width(Length::Fill)
                        .style(move |_| style::bg(accent)),
                );
            }
            let area = mouse_area(cell)
                .on_press(Message::TabPress(id, tab))
                .on_double_click(Message::TabDoubleClick(id, tab))
                .on_right_press(Message::TabRightClick(id, tab))
                .on_enter(Message::TabHover(id, Some(tab)))
                .on_exit(Message::TabHover(id, None));
            list = list.push(area);
        }
        let scroller = scrollable(container(list).padding([0, 6]))
            .on_scroll(move |v| Message::SidebarScrolled(id, v.absolute_offset().y))
            .height(Length::Fill)
            .style(style::scroll(t));
        let new_tab_tip = format!(
            "{} {}",
            self.action_label(Action::NewTab),
            self.keymap.display(Action::NewTab)
        );
        let bottom = row![
            iced::widget::tooltip(
                button(text("+").size(16))
                    .on_press(Message::NewTabButton(id))
                    .padding([2, 9])
                    .style(style::icon_button(t)),
                container(text(new_tab_tip).size(11))
                    .padding([2, 6])
                    .style(move |_| style::menu(t)),
                iced::widget::tooltip::Position::Top
            ),
            iced::widget::tooltip(
                button(text("▾").size(12))
                    .on_press(Message::ProfilesButton(id))
                    .padding([4, 6])
                    .style(style::icon_button(t)),
                container(text(self.action_label(Action::NewTabFromProfile)).size(11))
                    .padding([2, 6])
                    .style(move |_| style::menu(t)),
                iced::widget::tooltip::Position::Top
            ),
            space().width(Length::Fill),
            iced::widget::tooltip(
                button(text("⚙").size(14))
                    .on_press(Message::SettingsButton(id))
                    .padding([2, 8])
                    .style(style::icon_button(t)),
                container(text(self.action_label(Action::Settings)).size(11))
                    .padding([2, 6])
                    .style(move |_| style::menu(t)),
                iced::widget::tooltip::Position::Top
            ),
        ]
        .align_y(Alignment::Center)
        .padding([6, 8]);
        let body = column![
            space().height(SIDEBAR_TOP),
            scroller,
            container(space())
                .height(1.0)
                .width(Length::Fill)
                .style(move |_| style::bg(c(t.ui.border))),
            bottom
        ];
        let content = container(body)
            .width(width)
            .height(Length::Fill)
            .style(move |_| style::bg(c(t.ui.surface)));
        mouse_area(content)
            .on_move(move |p| Message::SidebarMove(id, p))
            .on_release(Message::SidebarRelease(id))
            .into()
    }

    fn context_menu(&self, id: window::Id, tab: TabId, at: iced::Point) -> El<'_> {
        let t = &self.theme;
        let item = |label: String, hint: String, m: MenuItem| -> El<'_> {
            button(
                row![
                    text(label).size(13),
                    space().width(Length::Fill),
                    text(hint).size(11).color(c(t.ui.fg_dim))
                ]
                .spacing(16),
            )
            .on_press(Message::MenuPick(id, tab, m))
            .width(Length::Fill)
            .padding([5, 10])
            .style(style::row_button(t, false))
            .into()
        };
        let km = |a: Action| self.keymap.display(a);
        let is_group = self.ws.is_group(tab);
        let group_len = self.group_len(tab);
        let mut col = column![
            item(
                self.action_label(Action::NewTab),
                km(Action::NewTab),
                MenuItem::NewTab
            ),
            item(
                self.action_label(Action::NewNestedTab),
                km(Action::NewNestedTab),
                MenuItem::NewNested
            ),
            item(
                self.action_label(Action::RenameTab),
                km(Action::RenameTab),
                MenuItem::Rename
            ),
        ]
        .spacing(1);
        if !is_group {
            col = col.push(item(
                self.action_label(Action::DuplicateTab),
                String::new(),
                MenuItem::Duplicate,
            ));
        }
        if group_len > 1 {
            col = col.push(item(
                self.action_label(Action::ToggleCollapse),
                km(Action::ToggleCollapse),
                MenuItem::Collapse,
            ));
            col = col.push(
                container(
                    text(self.i18n.t("menu-group-mode"))
                        .size(11)
                        .color(c(t.ui.fg_dim)),
                )
                .padding([4, 10]),
            );
            let cur = self.ws.group_mode(tab);
            for (mode, key, hint) in [
                (GroupMode::Single, "menu-group-single", String::new()),
                (
                    GroupMode::Stack,
                    "menu-group-stack",
                    km(Action::ToggleGroupView),
                ),
                (
                    GroupMode::Tiles,
                    "menu-group-tiles",
                    km(Action::ToggleGroupTiles),
                ),
            ] {
                let mark = if mode == cur { "✓  " } else { "    " };
                col = col.push(item(
                    format!("{mark}{}", self.i18n.t(key)),
                    hint,
                    MenuItem::GroupMode(mode),
                ));
            }
            if !is_group {
                col = col.push(item(
                    self.action_label(Action::OutdentTab),
                    km(Action::OutdentTab),
                    MenuItem::Outdent,
                ));
            }
        }
        col = col
            .push(item(
                self.action_label(Action::OpenFolder),
                km(Action::OpenFolder),
                MenuItem::OpenFolder,
            ))
            .push(item(
                self.action_label(Action::TabToNewWindow),
                km(Action::TabToNewWindow),
                MenuItem::ToNewWindow,
            ));
        if self.ws.windows.len() > 1 {
            col = col.push(item(
                self.i18n.t("menu-move-to-window"),
                String::new(),
                MenuItem::ToWindow,
            ));
        }
        col = col
            .push(
                container(space())
                    .height(1.0)
                    .width(Length::Fill)
                    .style(move |_| style::bg(c(t.ui.border))),
            )
            .push(item(
                if is_group {
                    self.i18n.t("menu-close-group")
                } else {
                    self.action_label(Action::CloseTab)
                },
                if is_group {
                    String::new()
                } else {
                    km(Action::CloseTab)
                },
                MenuItem::Close,
            ))
            .push(item(
                self.i18n.t("menu-close-others"),
                String::new(),
                MenuItem::CloseOthers,
            ))
            .push(
                container(space())
                    .height(1.0)
                    .width(Length::Fill)
                    .style(move |_| style::bg(c(t.ui.border))),
            )
            .push(item(
                self.action_label(Action::Quit),
                km(Action::Quit),
                MenuItem::Quit,
            ))
            .push(item(
                self.action_label(Action::QuitAndEndSessions),
                km(Action::QuitAndEndSessions),
                MenuItem::QuitAndEndSessions,
            ));
        let panel = container(col)
            .width(300.0)
            .padding(4)
            .style(move |_| style::menu(t));
        let catcher = mouse_area(container(space()).width(Length::Fill).height(Length::Fill))
            .on_press(Message::CloseMenu(id))
            .on_right_press(Message::CloseMenu(id));
        let w = &self.windows[&id];
        let x = at.x.min(w.size.width - 310.0).max(4.0);
        let y = at.y.min(w.size.height - 500.0).max(4.0);
        stack![catcher, pin(opaque(panel)).x(x).y(y)].into()
    }

    // ----- panes -----

    fn pane_area_view(&self, id: window::Id) -> El<'_> {
        let t = &self.theme;
        let area = self.pane_area(id);
        let Some(tab) = self.active_tab(id).and_then(|tid| self.ws.tab(tid)) else {
            let hint = format!(
                "{} — {}",
                self.action_label(Action::NewTab),
                self.keymap.display(Action::NewTab)
            );
            return container(text(hint).color(c(t.ui.fg_muted)))
                .center(Length::Fill)
                .into();
        };
        let sc = layout::scene(&self.ws, tab.id, area.width, area.height);
        let mut layers: Vec<El<'_>> = vec![
            container(space())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_| style::bg(c(t.ui.border)))
                .into(),
        ];
        let active_tab = tab.id;
        for sec in &sc.sections {
            if let Some(h) = sec.header {
                layers.push(
                    pin(self.section_header(id, sec.tab, sec.depth, h, sec.tab == active_tab))
                        .x(h.x)
                        .y(h.y)
                        .into(),
                );
            }
        }
        for (owner, pane, rect) in &sc.panes {
            let Some(ot) = self.ws.tab(*owner) else {
                continue;
            };
            let active = *owner == active_tab && *pane == ot.active_pane;
            let multi = ot.panes.len() > 1;
            layers.push(
                pin(self.pane_view(id, *pane, *rect, active, multi))
                    .x(rect.x)
                    .y(rect.y)
                    .into(),
            );
        }
        for s in &sc.splitters {
            let interaction = match s.kind {
                SplitKind::GridCol => mouse::Interaction::ResizingHorizontally,
                _ => mouse::Interaction::ResizingVertically,
            };
            let handle = mouse_area(container(space()).width(s.rect.width).height(s.rect.height))
                .on_press(Message::SplitPress(id, s.owner, s.kind, s.frame))
                .interaction(interaction);
            layers.push(pin(handle).x(s.rect.x).y(s.rect.y).into());
        }
        Stack::with_children(layers)
            .width(area.width)
            .height(area.height)
            .into()
    }

    /// Header of a tab section in group view: state, title, pane count.
    fn section_header(
        &self,
        id: window::Id,
        tab: TabId,
        depth: usize,
        r: Rectangle,
        active: bool,
    ) -> El<'_> {
        let t = &self.theme;
        let (glyph, gcolor) = self.indicator(self.tab_state(tab));
        let title = self.tab_title(tab);
        let n = self.ws.tab(tab).map(|x| x.panes.len()).unwrap_or(1);
        let mut row_el = row![
            space().width(10.0 + 14.0 * depth as f32),
            text(glyph).size(11).color(gcolor).width(14.0),
            text(truncate(
                &title,
                ((r.width - 120.0) / 7.0).max(8.0) as usize
            ))
            .size(12)
            .color(if active { c(t.ui.fg) } else { c(t.ui.fg_muted) })
            .wrapping(iced::widget::text::Wrapping::None),
            space().width(Length::Fill),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        if n > 1 {
            row_el = row_el.push(text(format!("{n}")).size(10).color(c(t.ui.fg_dim)));
        }
        row_el = row_el.push(space().width(10.0));
        let bg = if active {
            c(t.ui.surface_active)
        } else {
            c(t.ui.surface)
        };
        let accent = if active {
            c(t.ui.accent)
        } else {
            Color::TRANSPARENT
        };
        mouse_area(
            container(
                row![
                    container(space())
                        .width(3.0)
                        .height(Length::Fill)
                        .style(move |_| style::bg(accent)),
                    row_el
                ]
                .align_y(Alignment::Center),
            )
            .width(r.width)
            .height(r.height)
            .style(move |_| style::bg(bg)),
        )
        .on_press(Message::SelectTab(id, tab))
        .into()
    }

    fn pane_view(
        &self,
        id: window::Id,
        pane: PaneId,
        rect: Rectangle,
        active: bool,
        multi: bool,
    ) -> El<'_> {
        let t = &self.theme;
        let title_h = self.title_h();
        let metrics = self.metrics(id);
        let info = self.infos.get(&pane);
        let focused_window = self.focused == Some(id);
        let canvas = Canvas::new(TermCanvas {
            pane,
            grid: self.grids.get(&pane),
            style: TStyle {
                theme: t,
                fonts: &self.fonts,
                metrics,
                padding: self.config.appearance.pane_padding,
                cursor_style: self.config.appearance.cursor_style,
                bold_is_bright: self.config.appearance.bold_is_bright,
                ligatures: self.config.appearance.ligatures,
            },
            focused: active && focused_window,
            blink_off: self.config.appearance.cursor_blink && self.blink_off,
            bell: self.bells.contains_key(&pane),
            mods: self.mods,
            link_modifier: if cfg!(target_os = "macos") {
                self.mods.logo()
            } else {
                self.mods.control()
            },
        })
        .width(rect.width)
        .height((rect.height - title_h).max(1.0));

        let mut layers: Vec<El<'_>> = vec![canvas.into()];
        if let Some(info) = info {
            if let Some(host) = &info.reconnect_prompt {
                let banner = container(
                    row![
                        text(
                            self.i18n
                                .ta("reconnect-question", &[("host", host.clone())])
                        )
                        .size(13)
                        .color(c(t.ui.fg)),
                        space().width(Length::Fill),
                        button(text(self.i18n.t("button-reconnect")).size(12))
                            .on_press(Message::Reconnect(pane, true))
                            .padding([4, 10])
                            .style(style::primary_button(t)),
                        button(text(self.i18n.t("button-no")).size(12))
                            .on_press(Message::Reconnect(pane, false))
                            .padding([4, 10])
                            .style(style::secondary_button(t)),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([8, 12])
                .width(Length::Fill)
                .style(move |_| style::menu(t));
                layers.push(
                    container(banner)
                        .padding(10)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .align_y(Alignment::Start)
                        .into(),
                );
            }
            if let Some(code) = info.exited {
                let msg = self.i18n.ta("pane-exited", &[("code", code.to_string())]);
                let banner = container(
                    row![
                        text(msg).size(12).color(c(t.ui.fg_muted)),
                        space().width(Length::Fill),
                        button(text(self.i18n.t("button-restart")).size(12))
                            .on_press(Message::RestartPane(pane))
                            .padding([4, 10])
                            .style(style::secondary_button(t)),
                        button(text(self.i18n.t("button-close")).size(12))
                            .on_press(Message::ClosePaneButton(id, pane))
                            .padding([4, 10])
                            .style(style::secondary_button(t)),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .padding([6, 12])
                .width(Length::Fill)
                .style(move |_| style::menu(t));
                layers.push(
                    container(banner)
                        .padding(10)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .align_y(Alignment::End)
                        .into(),
                );
            }
        }
        if let Some(ui) = self.search.get(&pane) {
            layers.push(
                container(self.search_bar(pane, ui))
                    .padding(10)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(Alignment::End)
                    .align_y(Alignment::End)
                    .into(),
            );
        }
        let body = Stack::with_children(layers)
            .width(rect.width)
            .height((rect.height - title_h).max(1.0));
        if title_h <= 0.0 {
            return body.into();
        }
        // Title bar: indicator, host:cwd, process, zoom and close.
        let state = info.map(|i| i.state()).unwrap_or(PaneState::Idle);
        let (glyph, gcolor) = self.indicator(state);
        let location = info
            .map(|i| match (&i.remote_host, &i.cwd) {
                (Some(h), _) => h.clone(),
                (None, Some(cwd)) => tilde_path(cwd),
                _ => String::new(),
            })
            .unwrap_or_default();
        let process = info
            .and_then(|i| match (&i.fg_process, i.fg_pid) {
                (Some(n), Some(pid)) => Some(format!("{n} · {pid}")),
                _ => None,
            })
            .unwrap_or_default();
        let max_chars = ((rect.width - 160.0) / 7.0).max(8.0) as usize;
        let mut bar = row![
            text(glyph).size(11).color(gcolor).width(14.0),
            text(truncate(&location, max_chars))
                .size(11)
                .color(if active { c(t.ui.fg) } else { c(t.ui.fg_muted) })
                .wrapping(iced::widget::text::Wrapping::None),
            space().width(Length::Fill),
            text(process)
                .size(11)
                .color(c(t.ui.fg_dim))
                .wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .padding([0, 8]);
        if multi {
            let zoomed = self
                .ws
                .tab_of_pane(pane)
                .and_then(|t| self.ws.tab(t))
                .map(|t| t.zoomed)
                .unwrap_or(false);
            bar = bar.push(
                button(text(if zoomed { "⤡" } else { "⤢" }).size(11))
                    .on_press(Message::ZoomButton(pane))
                    .padding([0, 4])
                    .style(style::icon_button(t)),
            );
        }
        bar = bar.push(
            button(text("×").size(12))
                .on_press(Message::ClosePaneButton(id, pane))
                .padding([0, 4])
                .style(style::icon_button(t)),
        );
        let bar_bg = if active && multi {
            c(t.ui.surface_active)
        } else {
            c(t.ui.surface)
        };
        let accent = if active && multi {
            c(t.ui.accent)
        } else {
            Color::TRANSPARENT
        };
        let title = mouse_area(
            container(column![
                container(bar)
                    .height(title_h - 1.0)
                    .align_y(Alignment::Center),
                container(space())
                    .height(1.0)
                    .width(Length::Fill)
                    .style(move |_| style::bg(accent))
            ])
            .width(rect.width)
            .height(title_h)
            .style(move |_| style::bg(bar_bg)),
        )
        .on_press(Message::FocusPane(pane));
        column![title, body].into()
    }

    fn search_bar<'a>(&'a self, pane: PaneId, ui: &'a crate::app::SearchUi) -> El<'a> {
        let t = &self.theme;
        let info = self.grids.get(&pane).and_then(|g| g.search.clone());
        let counter = match &info {
            Some(i) if i.error.is_some() => self.i18n.t("search-invalid"),
            Some(i) if i.total > 0 => format!("{}/{}", i.current, i.total),
            Some(i) if !i.query.is_empty() => self.i18n.t("search-none"),
            _ => String::new(),
        };
        let regex_style = move |_: &iced::Theme, s: button::Status| {
            let mut st = style::icon_button(t)(&iced::Theme::Dark, s);
            if ui.regex {
                st.background = Some(Background::Color(ca(t.ui.accent, 0.3)));
                st.text_color = c(t.ui.fg);
            }
            st
        };
        container(
            row![
                text_input(&self.i18n.t("search-placeholder"), &ui.query)
                    .id(format!("search-{pane}"))
                    .on_input(move |s| Message::SearchInput(pane, s))
                    .on_submit(Message::SearchSubmit(pane))
                    .size(13)
                    .padding([4, 8])
                    .width(220.0)
                    .style(style::input(t)),
                text(counter).size(11).color(c(t.ui.fg_muted)).width(64.0),
                button(text(".*").size(12))
                    .on_press(Message::SearchRegex(pane))
                    .padding([2, 6])
                    .style(regex_style),
                button(text("↑").size(12))
                    .on_press(Message::SearchStep(pane, true))
                    .padding([2, 6])
                    .style(style::icon_button(t)),
                button(text("↓").size(12))
                    .on_press(Message::SearchStep(pane, false))
                    .padding([2, 6])
                    .style(style::icon_button(t)),
                button(text("×").size(12))
                    .on_press(Message::SearchClose(pane))
                    .padding([2, 6])
                    .style(style::icon_button(t)),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .padding([4, 6])
        .style(move |_| style::menu(t))
        .into()
    }

    // ----- overlays -----

    fn backdrop<'a>(&'a self, id: window::Id, content: El<'a>, width: f32) -> El<'a> {
        let t = &self.theme;
        let dim = mouse_area(
            container(space())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| style::bg(Color::from_rgba(0.0, 0.0, 0.0, 0.35))),
        )
        .on_press(Message::OverlayClose(id));
        let panel = container(content)
            .width(width)
            .max_height(self.windows[&id].size.height - 80.0)
            .padding(16)
            .style(move |_| style::panel(t));
        stack![
            dim,
            container(opaque(panel))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .padding(Padding {
                    top: 60.0,
                    ..Padding::ZERO
                })
        ]
        .into()
    }

    fn picker_view(&self, id: window::Id, p: &Picker) -> El<'_> {
        let t = &self.theme;
        let items = p.filtered();
        let mut list = column![].spacing(1);
        for (i, it) in items.iter().enumerate().take(200) {
            let selected = i == p.selected;
            let label = truncate(&it.label, 80);
            list = list.push(
                button(
                    row![
                        text(label)
                            .size(13)
                            .width(Length::Fill)
                            .wrapping(iced::widget::text::Wrapping::None),
                        text(it.detail.clone()).size(11).color(c(t.ui.fg_dim))
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .on_press(Message::PickerChoose(id, i))
                .width(Length::Fill)
                .padding([6, 10])
                .style(style::row_button(t, selected)),
            );
        }
        if items.is_empty() {
            list = list.push(
                container(
                    text(self.i18n.t("picker-empty"))
                        .size(12)
                        .color(c(t.ui.fg_dim)),
                )
                .padding(10),
            );
        }
        let hint = match p.kind {
            crate::app::PickerKind::History(_) => self.i18n.t("picker-history-hint"),
            _ => String::new(),
        };
        let content = column![
            row![
                text(p.title.clone()).size(14).color(c(t.ui.fg)),
                space().width(Length::Fill),
                text(hint).size(11).color(c(t.ui.fg_dim))
            ]
            .align_y(Alignment::Center),
            text_input(&self.i18n.t("picker-placeholder"), &p.query)
                .id("picker-input")
                .on_input(move |s| Message::PickerInput(id, s))
                .on_submit(Message::PickerSubmit(id))
                .size(14)
                .padding([8, 10])
                .style(style::input(t)),
            scrollable(container(list).padding(Padding {
                right: 12.0,
                ..Padding::ZERO
            }))
            .height(Length::Shrink)
            .style(style::scroll(t)),
        ]
        .spacing(10);
        self.backdrop(id, content.into(), 600.0)
    }

    fn confirm_view(&self, id: window::Id, cf: &Confirm) -> El<'_> {
        let t = &self.theme;
        let mut buttons = row![
            space().width(Length::Fill),
            button(text(self.i18n.t("button-cancel")).size(13))
                .on_press(Message::OverlayClose(id))
                .padding([6, 14])
                .style(style::secondary_button(t))
        ]
        .spacing(8);
        if let Some((label, _)) = &cf.alt {
            buttons = buttons.push(
                button(text(label.clone()).size(13))
                    .on_press(Message::ConfirmYes(id, true))
                    .padding([6, 14])
                    .style(style::secondary_button(t)),
            );
        }
        buttons = buttons.push(
            button(text(cf.yes.clone()).size(13))
                .on_press(Message::ConfirmYes(id, false))
                .padding([6, 14])
                .style(style::primary_button(t)),
        );
        let content = column![
            text(cf.title.clone()).size(15).color(c(t.ui.fg)),
            text(cf.body.clone()).size(13).color(c(t.ui.fg_muted)),
            buttons
        ]
        .spacing(14);
        self.backdrop(id, content.into(), 440.0)
    }
}
