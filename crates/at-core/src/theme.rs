//! Color themes (§6): built-in Dusk (dark) and Linen (light), user TOML themes and
//! Alacritty theme import.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn parse(s: &str) -> Option<Rgb> {
        let s = s.trim().trim_start_matches('#').trim_start_matches("0x");
        if s.len() != 6 {
            return None;
        }
        let v = u32::from_str_radix(s, 16).ok()?;
        Some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }

    pub fn hex(&self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }

    pub fn to_u32(self) -> u32 {
        ((self.0 as u32) << 16) | ((self.1 as u32) << 8) | self.2 as u32
    }

    pub fn from_u32(v: u32) -> Rgb {
        Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    pub fn f32(&self) -> [f32; 3] {
        [
            self.0 as f32 / 255.0,
            self.1 as f32 / 255.0,
            self.2 as f32 / 255.0,
        ]
    }

    pub fn mix(self, other: Rgb, t: f32) -> Rgb {
        let m = |a: u8, b: u8| {
            (a as f32 + (b as f32 - a as f32) * t)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        Rgb(m(self.0, other.0), m(self.1, other.1), m(self.2, other.2))
    }

    /// WCAG relative luminance.
    pub fn luminance(&self) -> f32 {
        let c = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * c(self.0) + 0.7152 * c(self.1) + 0.0722 * c(self.2)
    }

    pub fn contrast(&self, other: &Rgb) -> f32 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }
}

impl Serialize for Rgb {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.hex())
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Rgb::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("bad color `{s}`")))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ui {
    pub bg: Rgb,
    pub surface: Rgb,
    pub surface_active: Rgb,
    pub surface_hover: Rgb,
    pub border: Rgb,
    pub fg: Rgb,
    pub fg_muted: Rgb,
    pub fg_dim: Rgb,
    pub accent: Rgb,
    pub accent_info: Rgb,
    pub accent_remote: Rgb,
    pub accent_warn: Rgb,
    pub accent_error: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
    pub search_match: Rgb,
    pub search_current: Rgb,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ansi {
    pub normal: [Rgb; 8],
    pub bright: [Rgb; 8],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    pub dark: bool,
    pub ui: Ui,
    pub ansi: Ansi,
}

fn c(s: &str) -> Rgb {
    Rgb::parse(s).expect("valid builtin color")
}

impl Theme {
    pub fn dusk() -> Theme {
        Theme {
            name: "dusk".into(),
            dark: true,
            ui: Ui {
                bg: c("#1F232B"),
                surface: c("#191C23"),
                surface_active: c("#272C36"),
                surface_hover: c("#23272F"),
                border: c("#2E3440"),
                fg: c("#CDD3DE"),
                fg_muted: c("#7F8899"),
                fg_dim: c("#5A6271"),
                accent: c("#7FB3C8"),
                accent_info: c("#7FA6D9"),
                accent_remote: c("#8FC48B"),
                accent_warn: c("#E0B46B"),
                accent_error: c("#D97A7A"),
                cursor: c("#E0B46B"),
                selection: c("#3B4A5C"),
                search_match: c("#5C4F2A"),
                search_current: c("#8A6D2E"),
            },
            ansi: Ansi {
                normal: [
                    "#2A2F38", "#D97A7A", "#9BC48B", "#D9B46B", "#7FA6D9", "#C39AD1", "#7CC2C2",
                    "#C8CED8",
                ]
                .map(c),
                bright: [
                    "#5C6370", "#E59596", "#B4D7A3", "#E8C98A", "#9ABEE8", "#D4B4E0", "#99D4D4",
                    "#E6EAF0",
                ]
                .map(c),
            },
        }
    }

    pub fn linen() -> Theme {
        Theme {
            name: "linen".into(),
            dark: false,
            ui: Ui {
                bg: c("#F4F1EA"),
                surface: c("#ECE8DF"),
                surface_active: c("#E2DDD2"),
                surface_hover: c("#E8E3D9"),
                border: c("#D9D3C7"),
                fg: c("#3B4252"),
                fg_muted: c("#6E7686"),
                fg_dim: c("#9AA1AE"),
                accent: c("#3E7C93"),
                accent_info: c("#3F6FB0"),
                accent_remote: c("#4E8A47"),
                accent_warn: c("#B07A1B"),
                accent_error: c("#B54A4A"),
                cursor: c("#B07A1B"),
                selection: c("#D5DCE6"),
                search_match: c("#F2E2B0"),
                search_current: c("#E9C86A"),
            },
            ansi: Ansi {
                normal: [
                    "#3B4252", "#B54A4A", "#4E8A47", "#A8781A", "#3F6FB0", "#8A5FA3", "#2F8A8A",
                    "#E4E0D7",
                ]
                .map(c),
                bright: [
                    "#6E7686", "#C96A6A", "#64A05C", "#BE8E33", "#5A86C2", "#A27CB8", "#4AA3A3",
                    "#F4F1EA",
                ]
                .map(c),
            },
        }
    }

    /// Color for an xterm 256-color index.
    pub fn indexed(&self, i: u8) -> Rgb {
        match i {
            0..=7 => self.ansi.normal[i as usize],
            8..=15 => self.ansi.bright[i as usize - 8],
            16..=231 => {
                let i = i - 16;
                let step = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
                Rgb(step(i / 36), step((i / 6) % 6), step(i % 6))
            }
            232..=255 => {
                let v = 8 + (i - 232) * 10;
                Rgb(v, v, v)
            }
        }
    }

    /// Named UI token by name (for profile colors like `accent_remote`).
    pub fn token(&self, name: &str) -> Option<Rgb> {
        let u = &self.ui;
        Some(match name {
            "bg" => u.bg,
            "surface" => u.surface,
            "fg" => u.fg,
            "fg_muted" => u.fg_muted,
            "accent" => u.accent,
            "accent_info" => u.accent_info,
            "accent_remote" => u.accent_remote,
            "accent_warn" => u.accent_warn,
            "accent_error" => u.accent_error,
            _ => return None,
        })
    }

    pub fn color(&self, spec: &str) -> Option<Rgb> {
        Rgb::parse(spec).or_else(|| self.token(spec))
    }

    /// Resolves the `theme` setting. `auto` picks Dusk or Linen by the system appearance.
    pub fn resolve(setting: &str, system_dark: bool) -> (Theme, Option<String>) {
        match setting {
            "auto" | "" => (
                if system_dark {
                    Theme::dusk()
                } else {
                    Theme::linen()
                },
                None,
            ),
            "dusk" => (Theme::dusk(), None),
            "linen" => (Theme::linen(), None),
            other => {
                let path = if other.contains('/') || other.ends_with(".toml") {
                    crate::paths::expand_tilde(other)
                } else {
                    crate::paths::themes_dir().join(format!("{other}.toml"))
                };
                match Theme::load(&path) {
                    Ok(t) => (t, None),
                    Err(e) => (Theme::dusk(), Some(format!("{}: {e}", path.display()))),
                }
            }
        }
    }

    pub fn load(path: &Path) -> Result<Theme, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        Theme::parse(
            &text,
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
        )
    }

    /// Native format first, Alacritty `[colors.*]` format second.
    pub fn parse(text: &str, name: String) -> Result<Theme, String> {
        if let Ok(mut t) = toml::from_str::<Theme>(text) {
            if t.name.is_empty() {
                t.name = name;
            }
            return Ok(t);
        }
        #[derive(Deserialize)]
        struct NativePartial {
            #[serde(default)]
            dark: Option<bool>,
            ui: Option<toml::Value>,
            ansi: Option<toml::Value>,
        }
        let v: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
        if v.get("colors").is_some() {
            return Theme::from_alacritty(&v, name);
        }
        // Partial native theme: overlay onto Dusk or Linen.
        let p: NativePartial = v
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| e.to_string())?;
        let dark = p.dark.unwrap_or(true);
        let mut base = if dark { Theme::dusk() } else { Theme::linen() };
        base.name = name;
        if let Some(ui) = p.ui {
            let mut cur = toml::Value::try_from(&base.ui).map_err(|e| e.to_string())?;
            merge(&mut cur, ui);
            base.ui = cur.try_into().map_err(|e: toml::de::Error| e.to_string())?;
        }
        if let Some(ansi) = p.ansi {
            let mut cur = toml::Value::try_from(&base.ansi).map_err(|e| e.to_string())?;
            merge(&mut cur, ansi);
            base.ansi = cur.try_into().map_err(|e: toml::de::Error| e.to_string())?;
        }
        Ok(base)
    }

    fn from_alacritty(v: &toml::Value, name: String) -> Result<Theme, String> {
        let colors = v.get("colors").ok_or("no [colors]")?;
        let get = |sect: &str, key: &str| -> Option<Rgb> {
            colors.get(sect)?.get(key)?.as_str().and_then(Rgb::parse)
        };
        let bg = get("primary", "background").ok_or("colors.primary.background missing")?;
        let fg = get("primary", "foreground").ok_or("colors.primary.foreground missing")?;
        let names = [
            "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
        ];
        let dark = bg.luminance() < 0.3;
        let base = if dark { Theme::dusk() } else { Theme::linen() };
        let mut normal = base.ansi.normal;
        let mut bright = base.ansi.bright;
        for (i, n) in names.iter().enumerate() {
            if let Some(c) = get("normal", n) {
                normal[i] = c;
            }
            if let Some(c) = get("bright", n) {
                bright[i] = c;
            }
        }
        let towards = if dark {
            Rgb(0, 0, 0)
        } else {
            Rgb(255, 255, 255)
        };
        let away = if dark {
            Rgb(255, 255, 255)
        } else {
            Rgb(0, 0, 0)
        };
        let cursor = get("cursor", "cursor").unwrap_or(base.ui.cursor);
        let selection = get("selection", "background").unwrap_or(bg.mix(normal[4], 0.3));
        Ok(Theme {
            name,
            dark,
            ui: Ui {
                bg,
                surface: bg.mix(towards, 0.18),
                surface_active: bg.mix(away, 0.06),
                surface_hover: bg.mix(away, 0.03),
                border: bg.mix(away, 0.1),
                fg,
                fg_muted: fg.mix(bg, 0.4),
                fg_dim: fg.mix(bg, 0.6),
                accent: normal[6],
                accent_info: normal[4],
                accent_remote: normal[2],
                accent_warn: normal[3],
                accent_error: normal[1],
                cursor,
                selection,
                search_match: bg.mix(normal[3], 0.3),
                search_current: bg.mix(normal[3], 0.55),
            },
            ansi: Ansi { normal, bright },
        })
    }

    /// Names of built-in and user themes for the settings picker.
    pub fn available() -> Vec<String> {
        let mut v = vec!["auto".to_string(), "dusk".to_string(), "linen".to_string()];
        if let Ok(rd) = std::fs::read_dir(crate::paths::themes_dir()) {
            let mut user: Vec<String> = rd
                .flatten()
                .filter_map(|e| {
                    let p = e.path();
                    (p.extension()? == "toml")
                        .then(|| p.file_stem().unwrap().to_string_lossy().into_owned())
                })
                .collect();
            user.sort();
            v.extend(user);
        }
        v
    }
}

fn merge(base: &mut toml::Value, over: toml::Value) {
    match (base, over) {
        (toml::Value::Table(b), toml::Value::Table(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(bv) => merge(bv, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (b, o) => *b = o,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_targets() {
        for t in [Theme::dusk(), Theme::linen()] {
            let fg = t.ui.fg.contrast(&t.ui.bg);
            assert!((7.5..=12.0).contains(&fg), "{} fg contrast {fg}", t.name);
            assert!(t.ui.fg_muted.contrast(&t.ui.bg) >= 4.0, "{} muted", t.name);
            // Normal ANSI colors (except black, which is a background tone) must read on bg.
            for (i, c) in t.ansi.normal.iter().enumerate().skip(1).take(6) {
                assert!(
                    c.contrast(&t.ui.bg) >= 3.0,
                    "{} ansi {i} {}",
                    t.name,
                    c.contrast(&t.ui.bg)
                );
            }
        }
    }

    #[test]
    fn roundtrip_and_partial() {
        let text = toml::to_string(&Theme::dusk()).unwrap();
        assert_eq!(Theme::parse(&text, "x".into()).unwrap(), Theme::dusk());
        let partial = "dark = true\n[ui]\nbg = \"#000000\"\n";
        let t = Theme::parse(partial, "p".into()).unwrap();
        assert_eq!(t.ui.bg, Rgb(0, 0, 0));
        assert_eq!(t.ui.fg, Theme::dusk().ui.fg);
    }

    #[test]
    fn alacritty_import() {
        let text = r##"
[colors.primary]
background = "#282c34"
foreground = "#abb2bf"
[colors.normal]
red = "#e06c75"
"##;
        let t = Theme::parse(text, "onedark".into()).unwrap();
        assert!(t.dark);
        assert_eq!(t.ui.bg, Rgb::parse("#282c34").unwrap());
        assert_eq!(t.ansi.normal[1], Rgb::parse("#e06c75").unwrap());
    }

    #[test]
    fn palette256() {
        let t = Theme::dusk();
        assert_eq!(t.indexed(16), Rgb(0, 0, 0));
        assert_eq!(t.indexed(231), Rgb(255, 255, 255));
        assert_eq!(t.indexed(232), Rgb(8, 8, 8));
    }
}
