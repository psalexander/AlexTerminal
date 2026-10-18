//! Keyboard, mouse and paste encoding into PTY bytes (xterm conventions).

use alacritty_terminal::term::TermMode;
use at_proto::{KeyCode, KeyInput, Mods, MouseButton, MouseInput, MouseKind, NamedKey};

fn modifier_param(m: &Mods) -> u8 {
    1 + u8::from(m.shift) + 2 * u8::from(m.alt) + 4 * u8::from(m.ctrl)
}

fn csi_with_mods(final_or_tilde: &str, num: Option<u8>, m: &Mods) -> Vec<u8> {
    let p = modifier_param(m);
    match (num, p) {
        (None, 1) => format!("\x1b[{final_or_tilde}").into_bytes(),
        (None, p) => format!("\x1b[1;{p}{final_or_tilde}").into_bytes(),
        (Some(n), 1) => format!("\x1b[{n}~").into_bytes(),
        (Some(n), p) => format!("\x1b[{n};{p}~").into_bytes(),
    }
}

fn ctrl_byte(c: char) -> Option<u8> {
    let c = c.to_ascii_lowercase();
    Some(match c {
        'a'..='z' => c as u8 - b'a' + 1,
        '@' | '2' | ' ' => 0,
        '[' | '3' => 27,
        '\\' | '4' => 28,
        ']' | '5' => 29,
        '^' | '6' => 30,
        '_' | '-' | '7' | '/' => 31,
        '8' | '?' => 127,
        _ => return None,
    })
}

/// Encodes a key press. Returns `None` when the key produces nothing for the terminal.
pub fn encode_key(k: &KeyInput, mode: TermMode) -> Option<Vec<u8>> {
    let m = &k.mods;
    if m.logo {
        // ⌘/Super combinations are application shortcuts, never terminal input.
        return None;
    }
    let app_cursor = mode.contains(TermMode::APP_CURSOR);
    let meta = m.alt && k.alt_is_meta;
    let esc_prefix = |mut v: Vec<u8>| {
        if meta {
            v.insert(0, 0x1b);
        }
        v
    };
    match &k.key {
        KeyCode::Named(n) => {
            let v = match n {
                NamedKey::Enter => return Some(esc_prefix(b"\r".to_vec())),
                NamedKey::Tab => {
                    if m.shift {
                        b"\x1b[Z".to_vec()
                    } else {
                        esc_prefix(b"\t".to_vec())
                    }
                }
                NamedKey::Backspace => {
                    if m.ctrl {
                        esc_prefix(vec![0x08])
                    } else {
                        esc_prefix(vec![0x7f])
                    }
                }
                NamedKey::Escape => b"\x1b".to_vec(),
                NamedKey::Space => {
                    if m.ctrl {
                        esc_prefix(vec![0])
                    } else {
                        esc_prefix(b" ".to_vec())
                    }
                }
                NamedKey::Up | NamedKey::Down | NamedKey::Left | NamedKey::Right => {
                    let f = match n {
                        NamedKey::Up => "A",
                        NamedKey::Down => "B",
                        NamedKey::Right => "C",
                        _ => "D",
                    };
                    // Option+←/→ moves by words (ESC b / ESC f), §4.10.
                    if m.alt && !m.ctrl && !m.shift && matches!(n, NamedKey::Left | NamedKey::Right)
                    {
                        return Some(if *n == NamedKey::Left {
                            b"\x1bb".to_vec()
                        } else {
                            b"\x1bf".to_vec()
                        });
                    }
                    if modifier_param(m) == 1 && app_cursor {
                        format!("\x1bO{f}").into_bytes()
                    } else {
                        csi_with_mods(f, None, m)
                    }
                }
                NamedKey::Home | NamedKey::End => {
                    let f = if *n == NamedKey::Home { "H" } else { "F" };
                    if modifier_param(m) == 1 && app_cursor {
                        format!("\x1bO{f}").into_bytes()
                    } else {
                        csi_with_mods(f, None, m)
                    }
                }
                NamedKey::PageUp => csi_with_mods("~", Some(5), m),
                NamedKey::PageDown => csi_with_mods("~", Some(6), m),
                NamedKey::Insert => csi_with_mods("~", Some(2), m),
                NamedKey::Delete => csi_with_mods("~", Some(3), m),
                NamedKey::F(n) => {
                    let p = modifier_param(m);
                    match n {
                        1..=4 => {
                            let f = ["P", "Q", "R", "S"][*n as usize - 1];
                            if p == 1 {
                                format!("\x1bO{f}").into_bytes()
                            } else {
                                format!("\x1b[1;{p}{f}").into_bytes()
                            }
                        }
                        5..=12 => {
                            let code = [15, 17, 18, 19, 20, 21, 23, 24][*n as usize - 5];
                            csi_with_mods("~", Some(code), m)
                        }
                        _ => return None,
                    }
                }
            };
            Some(v)
        }
        KeyCode::Char(c) => {
            let base = c.chars().next();
            if m.ctrl {
                // Prefer the Latin key for non-Latin layouts (Ctrl+C on a Russian layout).
                let ch = base.filter(|c| c.is_ascii()).or(k.physical)?;
                let b = ctrl_byte(ch)?;
                return Some(esc_prefix(vec![b]));
            }
            if meta {
                let ch = base.filter(|c| c.is_ascii()).or(k.physical).or(base)?;
                let ch = if m.shift { ch.to_ascii_uppercase() } else { ch };
                let mut v = vec![0x1b];
                let mut tmp = [0u8; 4];
                v.extend_from_slice(ch.encode_utf8(&mut tmp).as_bytes());
                return Some(v);
            }
            if let Some(t) = k
                .text
                .as_ref()
                .filter(|t| !t.is_empty() && !t.chars().all(|c| c.is_control()))
            {
                return Some(t.as_bytes().to_vec());
            }
            if c.is_empty() {
                return None;
            }
            let s = if m.shift { c.to_uppercase() } else { c.clone() };
            Some(s.into_bytes())
        }
    }
}

/// Prepares pasted text: bracketed paste when enabled, newline normalization otherwise.
pub fn encode_paste(text: &str, mode: TermMode) -> Vec<u8> {
    if mode.contains(TermMode::BRACKETED_PASTE) {
        // Strip anything that could terminate the bracket early.
        let clean = text.replace("\x1b[201~", "").replace('\x1b', "");
        let mut v = b"\x1b[200~".to_vec();
        v.extend_from_slice(clean.as_bytes());
        v.extend_from_slice(b"\x1b[201~");
        v
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

/// Mouse report for applications that enabled mouse tracking. `None` when not reportable.
pub fn encode_mouse(ev: &MouseInput, mode: TermMode, buttons_down: bool) -> Option<Vec<u8>> {
    let sgr = mode.contains(TermMode::SGR_MOUSE);
    let mut code: u8 = match ev.kind {
        MouseKind::Wheel(d) => {
            if d > 0 {
                64
            } else {
                65
            }
        }
        MouseKind::Press | MouseKind::Release | MouseKind::Move => match ev.button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            MouseButton::None => 3,
        },
    };
    if let MouseKind::Move = ev.kind {
        let any = mode.contains(TermMode::MOUSE_MOTION);
        let drag = mode.contains(TermMode::MOUSE_DRAG) && buttons_down;
        if !any && !drag {
            return None;
        }
        code += 32;
    }
    if ev.mods.shift {
        code += 4;
    }
    if ev.mods.alt {
        code += 8;
    }
    if ev.mods.ctrl {
        code += 16;
    }
    let (x, y) = (ev.col as u32 + 1, ev.row as u32 + 1);
    if sgr {
        let c = if ev.kind == MouseKind::Release {
            'm'
        } else {
            'M'
        };
        Some(format!("\x1b[<{code};{x};{y}{c}").into_bytes())
    } else {
        if ev.kind == MouseKind::Release {
            code = 3 + (code & !3);
        }
        if x > 223 || y > 223 {
            return None;
        }
        Some(vec![
            0x1b,
            b'[',
            b'M',
            32 + code,
            32 + x as u8,
            32 + y as u8,
        ])
    }
}

pub fn focus_report(focused: bool) -> &'static [u8] {
    if focused { b"\x1b[I" } else { b"\x1b[O" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: KeyCode, mods: Mods, text: Option<&str>) -> KeyInput {
        KeyInput {
            key: k,
            physical: None,
            text: text.map(String::from),
            mods,
            alt_is_meta: true,
        }
    }

    #[test]
    fn basics() {
        let none = TermMode::empty();
        let m = Mods::default();
        assert_eq!(
            encode_key(&key(KeyCode::Char("a".into()), m, Some("a")), none).unwrap(),
            b"a"
        );
        assert_eq!(
            encode_key(&key(KeyCode::Named(NamedKey::Enter), m, None), none).unwrap(),
            b"\r"
        );
        assert_eq!(
            encode_key(&key(KeyCode::Named(NamedKey::Up), m, None), none).unwrap(),
            b"\x1b[A"
        );
        assert_eq!(
            encode_key(
                &key(KeyCode::Named(NamedKey::Up), m, None),
                TermMode::APP_CURSOR
            )
            .unwrap(),
            b"\x1bOA"
        );
        assert_eq!(
            encode_key(&key(KeyCode::Named(NamedKey::F(5)), m, None), none).unwrap(),
            b"\x1b[15~"
        );
        assert_eq!(
            encode_key(&key(KeyCode::Named(NamedKey::F(1)), m, None), none).unwrap(),
            b"\x1bOP"
        );
    }

    #[test]
    fn modifiers() {
        let none = TermMode::empty();
        let ctrl = Mods {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(
            encode_key(&key(KeyCode::Char("c".into()), ctrl, None), none).unwrap(),
            vec![3]
        );
        // Russian layout: key char is Cyrillic, physical key is C.
        let mut k = key(KeyCode::Char("с".into()), ctrl, None);
        k.physical = Some('c');
        assert_eq!(encode_key(&k, none).unwrap(), vec![3]);
        let alt = Mods {
            alt: true,
            ..Default::default()
        };
        assert_eq!(
            encode_key(&key(KeyCode::Char("x".into()), alt, Some("≈")), none).unwrap(),
            b"\x1bx"
        );
        assert_eq!(
            encode_key(&key(KeyCode::Named(NamedKey::Left), alt, None), none).unwrap(),
            b"\x1bb"
        );
        let cs = Mods {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            encode_key(&key(KeyCode::Named(NamedKey::Right), cs, None), none).unwrap(),
            b"\x1b[1;6C"
        );
        let shift = Mods {
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            encode_key(&key(KeyCode::Named(NamedKey::Tab), shift, None), none).unwrap(),
            b"\x1b[Z"
        );
        let logo = Mods {
            logo: true,
            ..Default::default()
        };
        assert!(encode_key(&key(KeyCode::Char("t".into()), logo, Some("t")), none).is_none());
    }

    #[test]
    fn paste() {
        assert_eq!(encode_paste("a\nb", TermMode::empty()), b"a\rb");
        assert_eq!(
            encode_paste("x\x1b[201~y", TermMode::BRACKETED_PASTE),
            b"\x1b[200~xy\x1b[201~"
        );
    }

    #[test]
    fn mouse() {
        let ev = MouseInput {
            kind: MouseKind::Press,
            button: MouseButton::Left,
            col: 4,
            row: 2,
            right_half: false,
            clicks: 1,
            mods: Mods::default(),
        };
        assert_eq!(
            encode_mouse(
                &ev,
                TermMode::SGR_MOUSE | TermMode::MOUSE_REPORT_CLICK,
                true
            )
            .unwrap(),
            b"\x1b[<0;5;3M"
        );
        let rel = MouseInput {
            kind: MouseKind::Release,
            ..ev
        };
        assert_eq!(
            encode_mouse(&rel, TermMode::SGR_MOUSE, false).unwrap(),
            b"\x1b[<0;5;3m"
        );
        assert_eq!(
            encode_mouse(&rel, TermMode::MOUSE_REPORT_CLICK, false).unwrap(),
            vec![0x1b, b'[', b'M', 35, 37, 35]
        );
        let mv = MouseInput {
            kind: MouseKind::Move,
            ..ev
        };
        assert!(encode_mouse(&mv, TermMode::MOUSE_REPORT_CLICK, true).is_none());
    }
}
