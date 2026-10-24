//! A last session that can't be opened (autosaved by a newer Drift, or damaged) is kept as
//! `recovered-<time>.drift` in Drift's data folder before anything can overwrite it, and a
//! window says so until it is closed. Without a file (in the browser, or when writing it
//! fails) the user is told the session wasn't restored; on the desktop the stored session is
//! then left alone instead of being autosaved over.

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use crate::*;

/// A session that couldn't be restored: what to tell the user, and where it was kept.
pub(crate) struct Unrestored {
    pub(crate) message: String,
    /// (The browser keeps no file.)
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) file: Option<PathBuf>,
}

/// Why the stored session can't be opened, as the first sentence of the message.
fn why(error: &drift_io::LoadError) -> String {
    match error {
        drift_io::LoadError::UnsupportedVersion(v)
            if *v > drift_io::FORMAT_VERSION && *v < u32::MAX =>
        {
            format!(
                "The last session was saved by a newer Drift (format {v}) and can't be opened \
                 here."
            )
        }
        drift_io::LoadError::UnsupportedVersion(_) => {
            "The last session was saved by a newer Drift and can't be opened here.".into()
        }
        e => format!("The last session couldn't be opened: {e}."),
    }
}

/// `YYYY-MM-DD-HHMMSS` (UTC) for seconds since 1970.
#[cfg(not(target_arch = "wasm32"))]
fn stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

/// Write `text` to a new `recovered-<time>.drift` in `dir`, never over an existing file.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn keep(dir: &Path, text: &str, secs: u64) -> std::io::Result<PathBuf> {
    use std::io::Write as _;
    std::fs::create_dir_all(dir)?;
    let stamp = stamp(secs);
    for n in 1..1000 {
        let name = if n == 1 {
            format!("recovered-{stamp}.drift")
        } else {
            format!("recovered-{stamp}-{n}.drift")
        };
        let path = dir.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(text.as_bytes())?;
                file.sync_all()?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "too many recovered sessions with the same time",
    ))
}

/// Show `path` in Explorer or the Finder (its folder elsewhere).
#[cfg(not(target_arch = "wasm32"))]
fn reveal(path: &Path) {
    use std::process::Command;
    #[cfg(windows)]
    let _ = {
        use std::os::windows::process::CommandExt as _;
        Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()
    };
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = Command::new("xdg-open")
        .arg(path.parent().unwrap_or(path))
        .spawn();
}

impl DriftApp {
    /// The stored session `text` failed to open with `error`: keep it in `dir` (Drift's data
    /// folder) and say so. Returns whether the stored session must be left alone (it wasn't
    /// kept as a file).
    pub(crate) fn keep_unrestored(
        &mut self,
        text: &str,
        error: &drift_io::LoadError,
        dir: Option<std::path::PathBuf>,
    ) -> bool {
        log::warn!("could not restore the previous session: {error}");
        let why = why(error);
        #[cfg(target_arch = "wasm32")]
        let (unrestored, hold) = {
            let _ = (text, dir);
            let message = format!("{why} A new drawing was started.");
            (Unrestored { message }, false)
        };
        #[cfg(not(target_arch = "wasm32"))]
        let (unrestored, hold) = {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let (message, file, hold) =
                match dir.ok_or_else(|| std::io::Error::other("Drift has no data folder here")) {
                    Ok(dir) => match keep(&dir, text, secs) {
                        Ok(path) => {
                            let name = path
                                .file_name()
                                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                            (
                                format!(
                                    "{why} It was kept as {name} in {}. Open it with the Drift \
                                 that saved it.",
                                    dir.display()
                                ),
                                Some(path),
                                false,
                            )
                        }
                        Err(e) => (Self::held(&why, &e), None, true),
                    },
                    Err(e) => (Self::held(&why, &e), None, true),
                };
            (Unrestored { message, file }, hold)
        };
        self.unrestored = Some(unrestored);
        hold
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn held(why: &str, e: &std::io::Error) -> String {
        format!(
            "{why} It couldn't be kept as a file ({e}), so this time Drift leaves it where it \
             is and doesn't autosave over it: open the Drift that saved it to get it back."
        )
    }

    /// The window saying the last session wasn't restored, up until it is closed.
    pub(crate) fn unrestored_window(&mut self, ctx: &egui::Context) {
        let Some(unrestored) = &self.unrestored else {
            return;
        };
        let mut open = true;
        let mut close = false;
        #[cfg(not(target_arch = "wasm32"))]
        let mut show = false;
        theme::dialog(ctx, "unrestored")
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(380.0);
                close = theme::dialog_title(ui, "Last session not restored", true);
                ui.add(egui::Label::new(&unrestored.message).wrap());
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    #[cfg(not(target_arch = "wasm32"))]
                    if unrestored.file.is_some() && ui.button("Show the file").clicked() {
                        show = true;
                    }
                    if ui.button("OK").clicked() {
                        close = true;
                    }
                });
            });
        #[cfg(not(target_arch = "wasm32"))]
        if show && let Some(path) = &unrestored.file {
            reveal(path);
        }
        if close || !open {
            self.unrestored = None;
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn stamps_are_utc_dates() {
        assert_eq!(stamp(0), "1970-01-01-000000");
        // 2026-10-24 15:30:12 UTC.
        assert_eq!(stamp(1_792_855_812), "2026-10-24-153012");
        assert_eq!(stamp(951_782_400), "2000-02-29-000000");
    }

    #[test]
    fn a_kept_session_never_replaces_another() {
        let dir = std::env::temp_dir().join(format!("drift-recovery-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = keep(&dir, "first", 60).unwrap();
        let b = keep(&dir, "second", 60).unwrap();
        assert_ne!(a, b);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "first");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "second");
        assert!(
            b.to_string_lossy()
                .ends_with("recovered-1970-01-01-000100-2.drift")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
