//! Theme tokens → iced colors and widget styles.

use at_core::theme::{Rgb, Theme as AtTheme};
use iced::widget::{button, container, scrollable, text_input};
use iced::{Background, Border, Color, Shadow, Vector};

pub fn c(rgb: Rgb) -> Color {
    Color::from_rgb8(rgb.0, rgb.1, rgb.2)
}

pub fn ca(rgb: Rgb, a: f32) -> Color {
    Color { a, ..c(rgb) }
}

/// iced theme derived from ours so built-in widgets match.
pub fn iced_theme(t: &AtTheme) -> iced::Theme {
    let p = iced::theme::Palette {
        background: c(t.ui.surface),
        text: c(t.ui.fg),
        primary: c(t.ui.accent),
        success: c(t.ui.accent_remote),
        warning: c(t.ui.accent_warn),
        danger: c(t.ui.accent_error),
    };
    iced::Theme::custom(t.name.clone(), p)
}

pub fn bg(color: Color) -> container::Style {
    container::Style {
        background: Some(Background::Color(color)),
        ..Default::default()
    }
}

pub fn panel(t: &AtTheme) -> container::Style {
    container::Style {
        background: Some(Background::Color(c(t.ui.surface))),
        border: Border {
            color: c(t.ui.border),
            width: 1.0,
            radius: 10.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: Vector::new(0.0, 8.0),
            blur_radius: 28.0,
        },
        text_color: Some(c(t.ui.fg)),
        ..Default::default()
    }
}

pub fn menu(t: &AtTheme) -> container::Style {
    container::Style {
        background: Some(Background::Color(c(t.ui.surface_active))),
        border: Border {
            color: c(t.ui.border),
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.3),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 16.0,
        },
        text_color: Some(c(t.ui.fg)),
        ..Default::default()
    }
}

/// Flat list-row button: transparent, hover tint.
pub fn row_button(
    t: &AtTheme,
    selected: bool,
) -> impl Fn(&iced::Theme, button::Status) -> button::Style + '_ {
    move |_, status| {
        let base = if selected {
            Some(Background::Color(c(t.ui.surface_active)))
        } else {
            None
        };
        let background = match status {
            button::Status::Hovered | button::Status::Pressed => {
                Some(Background::Color(c(t.ui.surface_hover).scale_alpha(1.0)))
            }
            _ => base,
        };
        let background = if selected { base } else { background };
        button::Style {
            background,
            text_color: c(t.ui.fg),
            border: Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

pub fn icon_button(t: &AtTheme) -> impl Fn(&iced::Theme, button::Status) -> button::Style + '_ {
    move |_, status| {
        let (bg, fg) = match status {
            button::Status::Hovered => (Some(Background::Color(c(t.ui.surface_hover))), c(t.ui.fg)),
            button::Status::Pressed => {
                (Some(Background::Color(c(t.ui.surface_active))), c(t.ui.fg))
            }
            button::Status::Disabled => (None, c(t.ui.fg_dim)),
            _ => (None, c(t.ui.fg_muted)),
        };
        button::Style {
            background: bg,
            text_color: fg,
            border: Border {
                radius: 5.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

pub fn primary_button(t: &AtTheme) -> impl Fn(&iced::Theme, button::Status) -> button::Style + '_ {
    move |_, status| {
        let base = c(t.ui.accent);
        let bg = match status {
            button::Status::Hovered => Color { a: 0.9, ..base },
            button::Status::Pressed => Color { a: 0.8, ..base },
            _ => base,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: c(t.ui.bg),
            border: Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

pub fn secondary_button(
    t: &AtTheme,
) -> impl Fn(&iced::Theme, button::Status) -> button::Style + '_ {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered | button::Status::Pressed => c(t.ui.surface_hover),
            _ => c(t.ui.surface_active),
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: c(t.ui.fg),
            border: Border {
                radius: 6.0.into(),
                color: c(t.ui.border),
                width: 1.0,
            },
            ..Default::default()
        }
    }
}

pub fn input(t: &AtTheme) -> impl Fn(&iced::Theme, text_input::Status) -> text_input::Style + '_ {
    move |_, status| {
        let border = match status {
            text_input::Status::Focused { .. } => c(t.ui.accent),
            _ => c(t.ui.border),
        };
        text_input::Style {
            background: Background::Color(c(t.ui.bg)),
            border: Border {
                color: border,
                width: 1.0,
                radius: 6.0.into(),
            },
            icon: c(t.ui.fg_muted),
            placeholder: c(t.ui.fg_dim),
            value: c(t.ui.fg),
            selection: c(t.ui.selection),
        }
    }
}

pub fn scroll(t: &AtTheme) -> impl Fn(&iced::Theme, scrollable::Status) -> scrollable::Style + '_ {
    move |theme, status| {
        let mut s = scrollable::default(theme, status);
        s.vertical_rail.background = None;
        s.vertical_rail.border = Border::default();
        s.vertical_rail.scroller.background = Background::Color(ca(t.ui.fg_dim, 0.5));
        s.vertical_rail.scroller.border = Border {
            radius: 3.0.into(),
            ..Default::default()
        };
        s
    }
}
