//! How much memory the app is using, and a guard that stops it before it can exhaust the
//! machine: a runaway allocation in any program can freeze the whole system once the commit
//! limit is reached, so past a hard limit Drift ends itself instead. The session autosave
//! (at most a few seconds old) is restored on the next start.
//!
//! Native only; on the web the browser limits a tab's memory itself.

/// Memory warning level, in bytes: the UI tells the user.
pub const WARN_BYTES: u64 = 3 * GB;
/// Default hard limit, in bytes: the guard ends the process. `DRIFT_MEMORY_LIMIT_MB` overrides.
pub const LIMIT_BYTES: u64 = 8 * GB;

const GB: u64 = 1024 * 1024 * 1024;

/// Whether [`start_guard`] has been called (the real app, as opposed to tests and tools).
static GUARDED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the guard is running in this process.
pub fn guarded() -> bool {
    GUARDED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Memory committed to this process, in bytes (`None` where it can't be measured).
pub fn used() -> Option<u64> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let m = memory_stats::memory_stats()?;
        // On Windows `virtual_mem` is the commit charge, which is what runs out; elsewhere it
        // is address space (far larger than what's used), so take resident memory.
        let bytes = if cfg!(target_os = "windows") {
            m.virtual_mem.max(m.physical_mem)
        } else {
            m.physical_mem
        };
        Some(bytes as u64)
    }
    #[cfg(target_arch = "wasm32")]
    None
}

/// The hard limit in force.
pub fn limit() -> u64 {
    std::env::var("DRIFT_MEMORY_LIMIT_MB")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|mb| *mb >= 256)
        .map_or(LIMIT_BYTES, |mb| mb * 1024 * 1024)
}

/// Where the guard leaves a note when it stops the app.
#[cfg(not(target_arch = "wasm32"))]
fn note_path() -> std::path::PathBuf {
    std::env::temp_dir().join("drift").join("memory-stop.txt")
}

/// Watch this process's memory from a background thread; past [`limit`] write a note and end
/// the process. The thread keeps running while the UI thread is stuck in a runaway loop,
/// which a per-frame check would not.
#[cfg(not(target_arch = "wasm32"))]
pub fn start_guard() {
    GUARDED.store(true, std::sync::atomic::Ordering::Relaxed);
    let limit = limit();
    let spawned = std::thread::Builder::new()
        .name("memory-guard".into())
        .spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(200));
                let Some(used) = used() else { continue };
                if used < limit {
                    continue;
                }
                let mb = used / (1024 * 1024);
                log::error!("memory guard: {mb} MB in use; stopping before the system runs out");
                let path = note_path();
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::write(&path, mb.to_string());
                std::process::exit(86);
            }
        });
    if let Err(e) = spawned {
        log::warn!("could not start the memory guard: {e}");
    }
}

#[cfg(target_arch = "wasm32")]
pub fn start_guard() {}

/// If the guard stopped the app last time: how many megabytes it was using. Clears the note.
pub fn take_stop_note() -> Option<u64> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = note_path();
        let text = std::fs::read_to_string(&path).ok()?;
        let _ = std::fs::remove_file(&path);
        text.trim().parse().ok()
    }
    #[cfg(target_arch = "wasm32")]
    None
}
