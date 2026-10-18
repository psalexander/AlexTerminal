//! Per-pane command history (§4.7): `<data>/history/<pane_id>.hist`, one entry per line:
//! `unix_ms \t cwd \t exit \t command`, with `\n`, `\t` and `\\` escaped.

use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub ts_ms: u64,
    pub cwd: String,
    /// `None` while running or unknown; `-1` if the session ended before the command.
    pub exit: Option<i32>,
    pub command: String,
}

pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '\r' => {}
            c => o.push(c),
        }
    }
    o
}

pub fn unescape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => o.push('\n'),
                Some('t') => o.push('\t'),
                Some('\\') => o.push('\\'),
                Some(x) => {
                    o.push('\\');
                    o.push(x);
                }
                None => o.push('\\'),
            }
        } else {
            o.push(c);
        }
    }
    o
}

impl HistoryEntry {
    pub fn to_line(&self) -> String {
        let exit = self.exit.map(|e| e.to_string()).unwrap_or_default();
        format!(
            "{}\t{}\t{}\t{}\n",
            self.ts_ms,
            escape(&self.cwd),
            exit,
            escape(&self.command)
        )
    }

    pub fn parse(line: &str) -> Option<HistoryEntry> {
        let mut it = line.trim_end_matches('\n').splitn(4, '\t');
        let ts_ms = it.next()?.parse().ok()?;
        let cwd = unescape(it.next()?);
        let exit_s = it.next()?;
        let exit = if exit_s.is_empty() {
            None
        } else {
            exit_s.parse().ok()
        };
        let command = unescape(it.next()?);
        if command.trim().is_empty() {
            return None;
        }
        Some(HistoryEntry {
            ts_ms,
            cwd,
            exit,
            command,
        })
    }
}

pub fn path_for(dir: &Path, pane: crate::PaneId) -> PathBuf {
    dir.join(format!("{pane}.hist"))
}

/// Appends entries and patches the exit code of the last one when it finishes.
#[derive(Debug)]
pub struct HistoryWriter {
    path: PathBuf,
    pending: Option<(u64, HistoryEntry)>,
    max_entries: usize,
    appended: usize,
}

impl HistoryWriter {
    pub fn new(path: PathBuf, max_entries: usize) -> Self {
        Self {
            path,
            pending: None,
            max_entries,
            appended: 0,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn open(&self) -> std::io::Result<File> {
        use std::os::unix::fs::OpenOptionsExt;
        if let Some(p) = self.path.parent() {
            std::fs::create_dir_all(p)?;
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&self.path)
    }

    /// Records a command when it starts (exit unknown).
    pub fn start(&mut self, entry: HistoryEntry) -> std::io::Result<()> {
        if let Some((_, prev)) = self.pending.take() {
            // A previous command never reported completion; leave its line as is.
            let _ = prev;
        }
        let mut f = self.open()?;
        let offset = f.seek(SeekFrom::End(0))?;
        f.write_all(entry.to_line().as_bytes())?;
        self.pending = Some((offset, entry));
        self.appended += 1;
        if self.appended.is_multiple_of(64) {
            let _ = rotate(&self.path, self.max_entries);
            self.pending = None;
        }
        Ok(())
    }

    /// Rewrites the last line with the exit code.
    pub fn finish(&mut self, exit: i32) -> std::io::Result<()> {
        let Some((offset, mut entry)) = self.pending.take() else {
            return Ok(());
        };
        let expected = offset + entry.to_line().len() as u64;
        entry.exit = Some(exit);
        let f = OpenOptions::new().write(true).open(&self.path)?;
        let len = f.metadata()?.len();
        if len != expected {
            // Someone else appended (should not happen); append a corrected copy instead.
            drop(f);
            let mut f = self.open()?;
            return f.write_all(entry.to_line().as_bytes());
        }
        f.set_len(offset)?;
        drop(f);
        let mut f = self.open()?;
        f.write_all(entry.to_line().as_bytes())
    }

    /// Marks an unfinished command as interrupted (`exit=-1`) on shutdown.
    pub fn close(&mut self) {
        if self.pending.is_some() {
            let _ = self.finish(-1);
        }
    }

    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
}

pub fn read(path: &Path) -> Vec<HistoryEntry> {
    let Ok(f) = File::open(path) else {
        return Vec::new();
    };
    BufReader::new(f)
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| HistoryEntry::parse(&l))
        .collect()
}

/// Keeps only the last `max` entries.
pub fn rotate(path: &Path, max: usize) -> std::io::Result<bool> {
    let entries = read(path);
    if entries.len() <= max {
        return Ok(false);
    }
    let keep = &entries[entries.len() - max..];
    let text: String = keep.iter().map(|e| e.to_line()).collect();
    crate::paths::write_atomic(path, text.as_bytes())?;
    Ok(true)
}

/// Deletes history files of panes that no longer exist and are older than `days`.
pub fn cleanup(dir: &Path, days: u64, live: &[crate::PaneId]) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(days * 86_400);
    let live: Vec<String> = live.iter().map(|p| p.to_string()).collect();
    let mut removed = 0;
    for e in rd.flatten() {
        let p = e.path();
        let Some(stem) = p.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        let stem = stem.split('.').next().unwrap_or("").to_string();
        if live.contains(&stem) {
            continue;
        }
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .map(|t| t < cutoff)
            .unwrap_or(false);
        if old && std::fs::remove_file(&p).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_roundtrip() {
        let s = "echo \"a\tb\"\nls \\ x";
        assert_eq!(unescape(&escape(s)), s.replace('\r', ""));
        let e = HistoryEntry {
            ts_ms: 5,
            cwd: "/tmp/a b".into(),
            exit: Some(2),
            command: s.into(),
        };
        assert_eq!(HistoryEntry::parse(&e.to_line()).unwrap(), e);
    }

    #[test]
    fn writer_start_finish_close() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.hist");
        let mut w = HistoryWriter::new(path.clone(), 100);
        w.start(HistoryEntry {
            ts_ms: 1,
            cwd: "/".into(),
            exit: None,
            command: "ls".into(),
        })
        .unwrap();
        w.finish(0).unwrap();
        w.start(HistoryEntry {
            ts_ms: 2,
            cwd: "/".into(),
            exit: None,
            command: "sleep 100".into(),
        })
        .unwrap();
        assert_eq!(read(&path)[1].exit, None);
        w.close();
        let all = read(&path);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].exit, Some(0));
        assert_eq!(all[1].exit, Some(-1));
    }

    #[test]
    fn rotation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.hist");
        let mut w = HistoryWriter::new(path.clone(), 10);
        for i in 0..30 {
            w.start(HistoryEntry {
                ts_ms: i,
                cwd: "/".into(),
                exit: None,
                command: format!("c{i}"),
            })
            .unwrap();
            w.finish(0).unwrap();
        }
        assert!(rotate(&path, 10).unwrap());
        let all = read(&path);
        assert_eq!(all.len(), 10);
        assert_eq!(all[0].command, "c20");
    }
}
