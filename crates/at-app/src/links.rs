//! URL and path detection under the pointer (⌘-click).

use regex::Regex;
use std::sync::OnceLock;

fn url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?:https?|ftp|file)://[^\s<>"'`]+|(?:~|\.{1,2})?/[^\s<>"'`:]+"#).unwrap()
    })
}

/// What lies at column `col` of `line`: a URL or a path (returned as-is).
pub fn find_at(line: &str, col: usize) -> Option<String> {
    // Work in char indices since grid columns are characters.
    let chars: Vec<char> = line.chars().collect();
    for m in url_re().find_iter(line) {
        let start = line[..m.start()].chars().count();
        let end = start + m.as_str().chars().count();
        if (start..end).contains(&col) {
            let s: String = chars[start..end].iter().collect();
            let s = s
                .trim_end_matches(['.', ',', ')', ']', '}', ';', '\'', '"'])
                .to_string();
            return Some(s);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects() {
        assert_eq!(
            find_at("see https://example.com/a?b=1, ok", 8).as_deref(),
            Some("https://example.com/a?b=1")
        );
        assert_eq!(
            find_at("ls /usr/local/bin", 6).as_deref(),
            Some("/usr/local/bin")
        );
        assert_eq!(find_at("plain words", 3), None);
        assert_eq!(
            find_at("path ~/src/app.rs:12", 8).as_deref(),
            Some("~/src/app.rs")
        );
    }
}
