//! Incremental scanner that extracts the OSC sequences AlexTerminal cares about
//! (7, 133, 1337, 7770) from the PTY byte stream. Bytes are not modified; the
//! emulator ignores these sequences on its own.

use base64::Engine;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OscEvent {
    /// OSC 7: current directory (decoded path).
    Cwd(String),
    /// OSC 133;A — prompt is shown, shell is idle.
    PromptStart,
    /// OSC 133;C — command output starts.
    CommandStart,
    /// OSC 133;D[;exit]
    CommandEnd(Option<i32>),
    /// OSC 7770;cmd=<base64>
    Command(String),
    /// OSC 7770;remote=<host> (empty host clears), also OSC 1337;RemoteHost=user@host
    Remote(Option<String>),
}

#[derive(Debug, Default)]
enum State {
    #[default]
    Ground,
    Esc,
    Osc,
    OscEsc,
}

const MAX_PAYLOAD: usize = 8192;

#[derive(Debug, Default)]
pub struct OscScanner {
    state: State,
    buf: Vec<u8>,
    overflow: bool,
}

impl OscScanner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, bytes: &[u8], out: &mut Vec<OscEvent>) {
        for &b in bytes {
            match self.state {
                State::Ground => {
                    if b == 0x1b {
                        self.state = State::Esc;
                    } else if b == 0x9d {
                        self.start();
                    }
                }
                State::Esc => {
                    if b == b']' {
                        self.start();
                    } else if b == 0x1b {
                        self.state = State::Esc;
                    } else {
                        self.state = State::Ground;
                    }
                }
                State::Osc => match b {
                    0x07 | 0x9c => self.finish(out),
                    0x1b => self.state = State::OscEsc,
                    0x18 | 0x1a => self.state = State::Ground,
                    _ => self.push(b),
                },
                State::OscEsc => {
                    if b == b'\\' {
                        self.finish(out);
                    } else {
                        // Not ST: the OSC is aborted; this ESC may start a new sequence.
                        self.state = if b == b']' {
                            self.start();
                            State::Osc
                        } else {
                            State::Ground
                        };
                    }
                }
            }
        }
    }

    fn start(&mut self) {
        self.state = State::Osc;
        self.buf.clear();
        self.overflow = false;
    }

    fn push(&mut self, b: u8) {
        if self.buf.len() < MAX_PAYLOAD {
            self.buf.push(b);
        } else {
            self.overflow = true;
        }
    }

    fn finish(&mut self, out: &mut Vec<OscEvent>) {
        self.state = State::Ground;
        if self.overflow {
            return;
        }
        if let Some(ev) = parse(&self.buf) {
            out.push(ev);
        }
    }
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse(payload: &[u8]) -> Option<OscEvent> {
    let s = std::str::from_utf8(payload).ok()?;
    let (code, rest) = s.split_once(';').unwrap_or((s, ""));
    match code {
        "7" => {
            // file://host/path
            let path = rest.strip_prefix("file://").map(|r| match r.find('/') {
                Some(i) => &r[i..],
                None => "",
            });
            let path = path.unwrap_or(rest);
            if path.is_empty() {
                return None;
            }
            Some(OscEvent::Cwd(percent_decode(path)))
        }
        "133" => {
            let mut parts = rest.split(';');
            match parts.next()? {
                "A" => Some(OscEvent::PromptStart),
                "C" => Some(OscEvent::CommandStart),
                "D" => {
                    let exit = parts.find_map(|p| {
                        let p = p.strip_prefix("exit=").unwrap_or(p);
                        p.parse::<i32>().ok()
                    });
                    Some(OscEvent::CommandEnd(exit))
                }
                _ => None,
            }
        }
        "7770" => {
            if let Some(b64) = rest.strip_prefix("cmd=") {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(b64.trim())
                    .ok()?;
                let cmd = String::from_utf8_lossy(&bytes)
                    .trim_end_matches('\n')
                    .to_string();
                (!cmd.trim().is_empty()).then_some(OscEvent::Command(cmd))
            } else if let Some(h) = rest.strip_prefix("remote=") {
                let h = h.trim();
                Some(OscEvent::Remote((!h.is_empty()).then(|| h.to_string())))
            } else {
                None
            }
        }
        "1337" => {
            if let Some(v) = rest.strip_prefix("RemoteHost=") {
                let host = v.rsplit('@').next().unwrap_or(v).trim();
                Some(OscEvent::Remote(
                    (!host.is_empty()).then(|| host.to_string()),
                ))
            } else {
                rest.strip_prefix("CurrentDir=")
                    .map(|v| OscEvent::Cwd(v.to_string()))
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(chunks: &[&[u8]]) -> Vec<OscEvent> {
        let mut s = OscScanner::new();
        let mut out = Vec::new();
        for c in chunks {
            s.feed(c, &mut out);
        }
        out
    }

    #[test]
    fn cwd_and_marks() {
        let ev =
            scan(&[b"abc\x1b]7;file://mac/Users/a/my%20dir\x07\x1b]133;A\x1b\\x\x1b]133;D;2\x07"]);
        assert_eq!(
            ev,
            vec![
                OscEvent::Cwd("/Users/a/my dir".into()),
                OscEvent::PromptStart,
                OscEvent::CommandEnd(Some(2))
            ]
        );
    }

    #[test]
    fn split_across_reads() {
        let ev = scan(&[b"\x1b]77", b"70;cmd=bHMgLWxh", b"\x07"]);
        assert_eq!(ev, vec![OscEvent::Command("ls -la".into())]);
    }

    #[test]
    fn remote_and_ignored() {
        let ev = scan(&[b"\x1b]0;title\x07\x1b]7770;remote=srv-01\x07\x1b]7770;remote=\x07\x1b]1337;RemoteHost=me@db\x07"]);
        assert_eq!(
            ev,
            vec![
                OscEvent::Remote(Some("srv-01".into())),
                OscEvent::Remote(None),
                OscEvent::Remote(Some("db".into()))
            ]
        );
    }

    #[test]
    fn aborted_sequence() {
        let ev = scan(&[b"\x1b]133;A\x1b[0m\x1b]133;C\x07"]);
        assert_eq!(ev, vec![OscEvent::CommandStart]);
    }
}
