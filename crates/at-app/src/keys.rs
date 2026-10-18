//! iced keyboard events → app chords and terminal key input.

use at_core::keymap::{Chord, normalize_key};
use at_proto::{KeyCode, KeyInput, Mods, NamedKey};
use iced::keyboard::key::{Code, Named, Physical};
use iced::keyboard::{Key, Modifiers};

pub fn named_name(n: &Named) -> Option<&'static str> {
    Some(match n {
        Named::Enter => "enter",
        Named::Tab => "tab",
        Named::Space => "space",
        Named::Backspace => "backspace",
        Named::Escape => "esc",
        Named::ArrowUp => "up",
        Named::ArrowDown => "down",
        Named::ArrowLeft => "left",
        Named::ArrowRight => "right",
        Named::Home => "home",
        Named::End => "end",
        Named::PageUp => "pageup",
        Named::PageDown => "pagedown",
        Named::Insert => "insert",
        Named::Delete => "delete",
        Named::F1 => "f1",
        Named::F2 => "f2",
        Named::F3 => "f3",
        Named::F4 => "f4",
        Named::F5 => "f5",
        Named::F6 => "f6",
        Named::F7 => "f7",
        Named::F8 => "f8",
        Named::F9 => "f9",
        Named::F10 => "f10",
        Named::F11 => "f11",
        Named::F12 => "f12",
        _ => return None,
    })
}

/// Latin character of a physical key (for shortcuts on non-Latin layouts).
pub fn physical_char(p: &Physical) -> Option<char> {
    let Physical::Code(code) = p else { return None };
    Some(match code {
        Code::KeyA => 'a',
        Code::KeyB => 'b',
        Code::KeyC => 'c',
        Code::KeyD => 'd',
        Code::KeyE => 'e',
        Code::KeyF => 'f',
        Code::KeyG => 'g',
        Code::KeyH => 'h',
        Code::KeyI => 'i',
        Code::KeyJ => 'j',
        Code::KeyK => 'k',
        Code::KeyL => 'l',
        Code::KeyM => 'm',
        Code::KeyN => 'n',
        Code::KeyO => 'o',
        Code::KeyP => 'p',
        Code::KeyQ => 'q',
        Code::KeyR => 'r',
        Code::KeyS => 's',
        Code::KeyT => 't',
        Code::KeyU => 'u',
        Code::KeyV => 'v',
        Code::KeyW => 'w',
        Code::KeyX => 'x',
        Code::KeyY => 'y',
        Code::KeyZ => 'z',
        Code::Digit0 => '0',
        Code::Digit1 => '1',
        Code::Digit2 => '2',
        Code::Digit3 => '3',
        Code::Digit4 => '4',
        Code::Digit5 => '5',
        Code::Digit6 => '6',
        Code::Digit7 => '7',
        Code::Digit8 => '8',
        Code::Digit9 => '9',
        Code::Minus => '-',
        Code::Equal => '=',
        Code::BracketLeft => '[',
        Code::BracketRight => ']',
        Code::Backslash => '\\',
        Code::Semicolon => ';',
        Code::Quote => '\'',
        Code::Backquote => '`',
        Code::Comma => ',',
        Code::Period => '.',
        Code::Slash => '/',
        _ => return None,
    })
}

/// Chord for keymap lookup. Non-ASCII keys fall back to the physical key.
pub fn chord(key: &Key, physical: &Physical, m: Modifiers) -> Option<Chord> {
    let k = match key {
        Key::Named(n) => named_name(n)?.to_string(),
        Key::Character(c) => {
            let s = c.as_str();
            let ch = s.chars().next()?;
            if ch.is_ascii() {
                normalize_key(&s.to_lowercase())
            } else {
                physical_char(physical)?.to_string()
            }
        }
        Key::Unidentified => physical_char(physical)?.to_string(),
    };
    Some(Chord {
        cmd: m.logo(),
        ctrl: m.control(),
        alt: m.alt(),
        shift: m.shift(),
        key: k,
    })
}

pub fn key_input(
    key: &Key,
    physical: &Physical,
    text: Option<&str>,
    m: Modifiers,
    alt_is_meta: bool,
) -> Option<KeyInput> {
    let code = match key {
        Key::Named(n) => KeyCode::Named(match n {
            Named::Enter => NamedKey::Enter,
            Named::Tab => NamedKey::Tab,
            Named::Space => NamedKey::Space,
            Named::Backspace => NamedKey::Backspace,
            Named::Escape => NamedKey::Escape,
            Named::ArrowUp => NamedKey::Up,
            Named::ArrowDown => NamedKey::Down,
            Named::ArrowLeft => NamedKey::Left,
            Named::ArrowRight => NamedKey::Right,
            Named::Home => NamedKey::Home,
            Named::End => NamedKey::End,
            Named::PageUp => NamedKey::PageUp,
            Named::PageDown => NamedKey::PageDown,
            Named::Insert => NamedKey::Insert,
            Named::Delete => NamedKey::Delete,
            Named::F1 => NamedKey::F(1),
            Named::F2 => NamedKey::F(2),
            Named::F3 => NamedKey::F(3),
            Named::F4 => NamedKey::F(4),
            Named::F5 => NamedKey::F(5),
            Named::F6 => NamedKey::F(6),
            Named::F7 => NamedKey::F(7),
            Named::F8 => NamedKey::F(8),
            Named::F9 => NamedKey::F(9),
            Named::F10 => NamedKey::F(10),
            Named::F11 => NamedKey::F(11),
            Named::F12 => NamedKey::F(12),
            _ => return None,
        }),
        Key::Character(c) => KeyCode::Char(c.to_string()),
        Key::Unidentified => KeyCode::Char(text.unwrap_or("").to_string()),
    };
    Some(KeyInput {
        key: code,
        physical: physical_char(physical),
        text: text.map(String::from),
        mods: Mods {
            ctrl: m.control(),
            alt: m.alt(),
            shift: m.shift(),
            logo: m.logo(),
        },
        alt_is_meta,
    })
}
