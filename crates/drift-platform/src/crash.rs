//! A crash log. The release app has no console, so a panic would otherwise vanish without a
//! trace: the hook installed by [`install_hook`] appends the message, where it happened and a
//! backtrace to `crash.log` in Drift's data folder, and leaves a note so the next start can
//! say so (the session autosave is restored as usual).
//!
//! Native only; in the browser eframe already reports panics to the console.

use std::path::PathBuf;

/// Drift's folder in the user's own application data: `%LOCALAPPDATA%\Drift` on Windows,
/// `~/Library/Application Support/Drift` on macOS, `$XDG_STATE_HOME/Drift` (or
/// `~/.local/state/Drift`) elsewhere. `None` on the web or when none of these is set.
pub fn data_dir() -> Option<PathBuf> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
    }?;
    Some(base.join("Drift"))
}

/// Where panics are logged.
pub fn log_path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("crash.log"))
}

fn note_path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("crash-note.txt"))
}

/// Log every panic to [`log_path`] (appending), then run the default hook. Call once from the
/// native entry point.
#[cfg(not(target_arch = "wasm32"))]
pub fn install_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = write_report(info);
        default(info);
    }));
}

#[cfg(target_arch = "wasm32")]
pub fn install_hook() {}

#[cfg(not(target_arch = "wasm32"))]
fn write_report(info: &std::panic::PanicHookInfo<'_>) -> std::io::Result<()> {
    use std::io::Write as _;
    let (Some(log), Some(note)) = (log_path(), note_path()) else {
        return Ok(());
    };
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".into());
    let location = info
        .location()
        .map_or_else(|| "unknown".into(), |l| l.to_string());
    let thread = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_string();
    let when = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let backtrace = std::backtrace::Backtrace::force_capture();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)?;
    writeln!(
        file,
        "=== Drift {} panicked (unix time {when}) ===\nthread: {thread}\nat: {location}\nmessage: {message}\nbacktrace:\n{backtrace}\n",
        env!("CARGO_PKG_VERSION"),
    )?;
    std::fs::write(&note, &message)
}

/// If Drift panicked last time: the panic message. Clears the note (the log is kept).
pub fn take_note() -> Option<String> {
    let path = note_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    Some(text.trim().to_string())
}
