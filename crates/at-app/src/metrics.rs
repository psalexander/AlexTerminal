//! Font selection and terminal cell metrics.

use iced::advanced::graphics::text::Paragraph as GParagraph;
use iced::advanced::text::{self, Paragraph as _};
use iced::{Font, Pixels, Size};
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub font_size: f32,
    pub cell_w: f32,
    pub cell_h: f32,
}

pub struct Fonts {
    pub family: &'static str,
    pub regular: Font,
    pub bold: Font,
    pub italic: Font,
    pub bold_italic: Font,
    cache: RefCell<HashMap<(u32, u32), Metrics>>,
}

/// Families known to the shared font database.
pub fn installed_families() -> Vec<String> {
    let mut fs = iced::advanced::graphics::text::font_system()
        .write()
        .expect("font system");
    let mut v: Vec<String> = fs
        .raw()
        .db()
        .faces()
        .filter(|f| f.monospaced)
        .flat_map(|f| {
            f.families
                .iter()
                .map(|(n, _)| n.clone())
                .collect::<Vec<_>>()
        })
        .collect();
    v.sort();
    v.dedup();
    v
}

fn has_family(name: &str) -> bool {
    let mut fs = iced::advanced::graphics::text::font_system()
        .write()
        .expect("font system");
    fs.raw()
        .db()
        .faces()
        .any(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)))
}

impl Fonts {
    /// First available family among the configured one and its fallbacks.
    pub fn resolve(primary: &str, fallback: &[String]) -> Fonts {
        let mut candidates = vec![primary.to_string()];
        candidates.extend(fallback.iter().cloned());
        candidates.extend(
            [
                "SF Mono",
                "Menlo",
                "DejaVu Sans Mono",
                "Liberation Mono",
                "Monaco",
            ]
            .map(String::from),
        );
        let family = candidates
            .into_iter()
            .find(|n| has_family(n))
            .unwrap_or_else(|| "monospace".into());
        let family: &'static str = Box::leak(family.into_boxed_str());
        let regular = if family == "monospace" {
            Font::MONOSPACE
        } else {
            Font::with_name(family)
        };
        Fonts {
            family,
            regular,
            bold: Font {
                weight: iced::font::Weight::Bold,
                ..regular
            },
            italic: Font {
                style: iced::font::Style::Italic,
                ..regular
            },
            bold_italic: Font {
                weight: iced::font::Weight::Bold,
                style: iced::font::Style::Italic,
                ..regular
            },
            cache: RefCell::new(HashMap::new()),
        }
    }

    pub fn metrics(&self, size: f32, line_height: f32) -> Metrics {
        let key = ((size * 100.0) as u32, (line_height * 100.0) as u32);
        if let Some(m) = self.cache.borrow().get(&key) {
            return *m;
        }
        let sample = "MMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMMM";
        let p = GParagraph::with_text(text::Text {
            content: sample,
            bounds: Size::INFINITE,
            size: Pixels(size),
            line_height: text::LineHeight::Relative(1.0),
            font: self.regular,
            align_x: text::Alignment::Left,
            align_y: iced::alignment::Vertical::Top,
            shaping: text::Shaping::Basic,
            wrapping: text::Wrapping::None,
        });
        let w = p.min_bounds().width / sample.len() as f32;
        let cell_w = if w.is_finite() && w > 1.0 {
            w
        } else {
            size * 0.6
        };
        let m = Metrics {
            font_size: size,
            cell_w,
            cell_h: (size * line_height).round().max(size + 1.0),
        };
        self.cache.borrow_mut().insert(key, m);
        m
    }
}
