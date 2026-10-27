//! Drift's log, `drift.log` in its data folder beside `crash.log`: the release app has no
//! console, so without it every warning would be lost. It is kept to [`MAX_BYTES`]: past that
//! it becomes `drift.log.1` (replacing the one before) and a new one starts.
//!
//! What goes in it is up to the callers: open, save and import results, autosave and other
//! failures, caught panics. Never drawing contents, and of paths only file names.
//!
//! Native only.

use std::io::Write;
use std::path::PathBuf;

/// The log's size limit, in bytes.
pub const MAX_BYTES: u64 = 1024 * 1024;

/// Where the log is written.
pub fn path() -> Option<PathBuf> {
    crate::crash::data_dir().map(|d| d.join("drift.log"))
}

/// The end of the log (and of the one before it, when the log is short): at most `max_bytes`,
/// from the start of a line. Empty if there's no log.
pub fn tail(max_bytes: usize) -> String {
    let Some(path) = path() else {
        return String::new();
    };
    let mut bytes = std::fs::read(&path).unwrap_or_default();
    if bytes.len() < max_bytes {
        let older = std::fs::read(rotated(&path)).unwrap_or_default();
        bytes = [older, bytes].concat();
    }
    let start = bytes.len().saturating_sub(max_bytes);
    let text = String::from_utf8_lossy(&bytes[start..]).into_owned();
    match text.find('\n') {
        Some(i) if start > 0 => text[i + 1..].to_owned(),
        _ => text,
    }
}

fn rotated(path: &std::path::Path) -> PathBuf {
    path.with_extension("log.1")
}

/// A writer for the logger: each line goes to standard error (a console, when there is one)
/// and to the log file.
pub struct Writer {
    path: Option<PathBuf>,
    file: Option<std::fs::File>,
    size: u64,
}

impl Writer {
    pub fn new() -> Self {
        let mut writer = Writer {
            path: path(),
            file: None,
            size: 0,
        };
        writer.open();
        writer
    }

    fn open(&mut self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        self.file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok();
        self.size = self
            .file
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .map_or(0, |m| m.len());
    }

    /// Past the limit: the log becomes `drift.log.1` and a new one starts.
    fn rotate(&mut self) {
        let Some(path) = self.path.clone() else {
            return;
        };
        self.file = None;
        let _ = std::fs::rename(&path, rotated(&path));
        self.open();
    }
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        if self.size >= MAX_BYTES {
            self.rotate();
        }
        if let Some(file) = &mut self.file
            && file.write_all(buf).is_ok()
        {
            self.size += buf.len() as u64;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let _ = std::io::stderr().flush();
        if let Some(file) = &mut self.file {
            let _ = file.flush();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_is_kept_to_its_limit() {
        let dir = std::env::temp_dir().join(format!("drift-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("drift.log");
        let mut w = Writer {
            path: Some(path.clone()),
            file: None,
            size: 0,
        };
        w.open();
        let line = [b'x'; 1023]
            .iter()
            .chain(b"\n")
            .copied()
            .collect::<Vec<u8>>();
        for _ in 0..(MAX_BYTES / 1024 + 10) {
            w.write_all(&line).unwrap();
        }
        let now = std::fs::metadata(&path).unwrap().len();
        let before = std::fs::metadata(rotated(&path)).unwrap().len();
        assert!(now < MAX_BYTES / 10, "{now}");
        assert!((MAX_BYTES..MAX_BYTES + 2048).contains(&before), "{before}");
        drop(w);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
