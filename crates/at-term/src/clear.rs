//! Finds where `clear` wipes the screen so the scrollback can be dropped there too.
//!
//! `clear` sends `ESC[3J` (erase scrollback) before `ESC[H ESC[2J`. The emulator
//! saves the visible screen into scrollback on `ESC[2J`, so the old output would come
//! back right after being erased. The scanner reports the end of an `ESC[2J` that
//! follows `ESC[3J`; the caller clears the scrollback after feeding bytes up to it.

#[derive(Debug, Default)]
enum State {
    #[default]
    Ground,
    Esc,
    Csi,
}

#[derive(Debug, Default)]
pub struct ClearScanner {
    state: State,
    param: Option<u16>,
    plain: bool,
    armed: bool,
}

impl ClearScanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Offsets just past each `ESC[2J` that completes a `clear` in `bytes`.
    pub fn feed(&mut self, bytes: &[u8], cuts: &mut Vec<usize>) {
        for (i, &b) in bytes.iter().enumerate() {
            match self.state {
                State::Ground => {
                    if b == 0x1b {
                        self.state = State::Esc;
                    } else if b >= 0x20 && b != 0x7f {
                        // Text between the two sequences: not a `clear`.
                        self.armed = false;
                    }
                }
                State::Esc => {
                    self.state = if b == b'[' {
                        self.param = None;
                        self.plain = true;
                        State::Csi
                    } else if b == 0x1b {
                        State::Esc
                    } else {
                        State::Ground
                    };
                }
                State::Csi => match b {
                    b'0'..=b'9' => {
                        let d = (b - b'0') as u16;
                        self.param =
                            Some(self.param.unwrap_or(0).saturating_mul(10).saturating_add(d));
                    }
                    0x20..=0x3f => self.plain = false,
                    0x40..=0x7e => {
                        self.state = State::Ground;
                        if self.plain && b == b'J' {
                            match self.param {
                                Some(3) => self.armed = true,
                                Some(2) if self.armed => {
                                    self.armed = false;
                                    cuts.push(i + 1);
                                }
                                _ => {}
                            }
                        }
                    }
                    0x1b => self.state = State::Esc,
                    _ => {}
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cuts(chunks: &[&[u8]]) -> Vec<Vec<usize>> {
        let mut s = ClearScanner::new();
        chunks
            .iter()
            .map(|c| {
                let mut v = Vec::new();
                s.feed(c, &mut v);
                v
            })
            .collect()
    }

    #[test]
    fn macos_clear() {
        assert_eq!(cuts(&[b"\x1b[3J\x1b[H\x1b[2Jafter"]), vec![vec![11]]);
        // Split across reads.
        assert_eq!(
            cuts(&[b"\x1b[3J\x1b[", b"H\x1b[2", b"J"]),
            vec![vec![], vec![], vec![1]]
        );
    }

    #[test]
    fn not_clear() {
        // Plain 2J keeps the screen in scrollback, as before.
        assert_eq!(cuts(&[b"\x1b[H\x1b[2J"]), vec![Vec::<usize>::new()]);
        // Text in between disarms.
        assert_eq!(cuts(&[b"\x1b[3Jtext\x1b[2J"]), vec![Vec::<usize>::new()]);
        // Private-mode sequences are ignored.
        assert_eq!(cuts(&[b"\x1b[3J\x1b[?2J"]), vec![Vec::<usize>::new()]);
    }
}
