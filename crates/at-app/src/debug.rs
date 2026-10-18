//! Scripted runs for testing the real GUI without screen-recording rights:
//! `ALEXTERMINAL_DEBUG_SCRIPT=file` runs steps and saves window screenshots.
//!
//! Script lines: `wait MS`, `text STRING` (`\r`, `\n`, `\t`, `\e` escapes),
//! `action NAME` (keymap action name), `shot PATH.png`, `quit`, `# comment`.

use at_core::keymap::Action;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub enum Step {
    Wait(u64),
    Text(String),
    Action(Action),
    Shot(PathBuf),
    Quit,
}

fn unescape(s: &str) -> String {
    s.replace("\\r", "\r")
        .replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\e", "\x1b")
}

pub fn parse(text: &str) -> Vec<Step> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim_start();
            if l.is_empty() || l.starts_with('#') {
                return None;
            }
            let (cmd, arg) = l.split_once(' ').unwrap_or((l, ""));
            match cmd {
                "wait" => arg.trim().parse().ok().map(Step::Wait),
                "text" => Some(Step::Text(unescape(arg))),
                "action" => Action::from_name(arg.trim()).map(Step::Action),
                "shot" => Some(Step::Shot(PathBuf::from(arg.trim()))),
                "quit" => Some(Step::Quit),
                _ => None,
            }
        })
        .collect()
}

pub fn load() -> Vec<Step> {
    std::env::var_os("ALEXTERMINAL_DEBUG_SCRIPT")
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| parse(&t))
        .unwrap_or_default()
}

pub fn write_png(path: &Path, shot: &iced::window::Screenshot) -> std::io::Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let f = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(
        std::io::BufWriter::new(f),
        shot.size.width,
        shot.size.height,
    );
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(std::io::Error::other)?;
    w.write_image_data(&shot.rgba)
        .map_err(std::io::Error::other)?;
    Ok(())
}
