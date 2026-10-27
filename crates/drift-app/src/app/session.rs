//! Each window's autosaved session, in a file of its own: `sessions/<id>.session` in Drift's
//! data folder. (eframe's storage keeps only small settings: it is one file for every window,
//! written without a rename.)
//!
//! A window holds a lock on `<id>.lock` for as long as it runs; the system lets go of it when
//! the process ends, however it ends. So a session whose lock can be taken was left behind by a
//! window that has gone: closed, crashed, or stopped by the memory guard or an update. A new
//! window takes over the newest one left behind and goes on with it; a window opened for a file
//! takes over only a session of that same file. Others with unsaved changes (or that can't be
//! read) are offered: to open in windows of their own, or to delete. Ones without unsaved
//! changes are deleted (their drawings are in their files).
//!
//! The session is written atomically (see [`drift_platform::write_atomic`]), with the previous
//! one kept as `<id>.session.bak` and read if the newer can't be; and only when something
//! changed since the last write. Each write records when it was made, which orders them
//! (file times can be as coarse as a second).
//!
//! Native only, and not in the demo.

use std::fs::File;
use std::path::{Path, PathBuf};

use eframe::egui;
use serde_json::{Value, json};

/// A session as it was kept.
#[derive(Debug)]
pub(crate) struct Kept {
    /// The drawing, as `.drift` text.
    pub(crate) text: String,
    pub(crate) name: String,
    pub(crate) path: Option<PathBuf>,
    /// It had unsaved changes.
    pub(crate) dirty: bool,
    /// When it was written, in milliseconds since 1970.
    at: u64,
}

/// What a write of the session holds apart from the drawing: when all of it is as last written,
/// nothing is written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub(crate) revision: u64,
    pub(crate) name: String,
    pub(crate) path: Option<PathBuf>,
    pub(crate) dirty: bool,
}

/// This window's session file, held for as long as the window runs.
pub(crate) struct Slot {
    file: PathBuf,
    /// Held (locked) for the window's life; the system lets go when the process ends.
    _lock: File,
    written: Option<Stamp>,
    /// When the last write was made (each write is later than the one before).
    at: u64,
}

/// A session left behind that is offered: its file, and the drawing's name.
pub(crate) type Offered = (PathBuf, String);

/// What [`Slot::claim`] found.
pub(crate) struct Claimed {
    pub(crate) slot: Slot,
    /// The session taken over, if one was left behind.
    pub(crate) restored: Option<Kept>,
    /// It was the session of the file the window was opened for.
    pub(crate) same_file: bool,
    /// Further sessions left behind with unsaved changes or that can't be read, newest first.
    pub(crate) others: Vec<Offered>,
    /// The sessions folder had nothing in it: the session may still be in eframe's storage
    /// (from a Drift before per-window sessions).
    pub(crate) first: bool,
}

fn lock_path(session: &Path) -> PathBuf {
    session.with_extension("lock")
}

/// Take `session`'s lock if no running window holds it.
fn try_lock(session: &Path) -> Option<File> {
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path(session))
        .ok()?;
    lock.try_lock().ok()?;
    Some(lock)
}

/// Read a kept session, or the copy before it when it can't be read.
pub(crate) fn read(session: &Path) -> Option<Kept> {
    let parse = |path: &Path| -> Option<Kept> {
        let value: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
        Some(Kept {
            text: value.get("document")?.to_string(),
            name: value.get("name")?.as_str()?.to_owned(),
            path: value
                .get("path")
                .and_then(Value::as_str)
                .filter(|p| !p.is_empty())
                .map(PathBuf::from),
            dirty: value.get("dirty")?.as_bool()?,
            at: value.get("at").and_then(Value::as_u64).unwrap_or(0),
        })
    };
    parse(session).or_else(|| {
        let mut bak = session.as_os_str().to_owned();
        bak.push(".bak");
        parse(Path::new(&bak))
    })
}

/// Delete a session and what goes with it.
fn remove(session: &Path) {
    let mut bak = session.as_os_str().to_owned();
    bak.push(".bak");
    let _ = std::fs::remove_file(session);
    let _ = std::fs::remove_file(&bak);
    let _ = std::fs::remove_file(lock_path(session));
}

/// Delete the sessions left behind in `offered`, but not one a window has taken over since.
pub(crate) fn delete(offered: &[Offered]) {
    for (session, _) in offered {
        if let Some(lock) = try_lock(session) {
            drop(lock);
            remove(session);
        }
    }
}

/// Whether two paths name the same file (letter case aside on Windows).
fn same_file(a: &Path, b: &Path) -> bool {
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_owned());
    let (a, b) = (canonical(a), canonical(b));
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// How many other Drift windows are running: sessions in `dir` whose lock is held, other than
/// `own`'s.
pub(crate) fn other_windows(dir: &Path, own: Option<&Slot>) -> usize {
    let own = own.map(|s| lock_path(&s.file));
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "lock") && Some(p) != own.as_ref())
        .filter(|p| {
            // Only an existing lock: don't make one.
            std::fs::OpenOptions::new()
                .write(true)
                .open(p)
                .is_ok_and(|f| f.try_lock().is_err())
        })
        .count()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

impl Slot {
    /// Claim this window's session in `dir`. A session left behind for `opening` (the file the
    /// window was opened for) is taken over; otherwise, with `adopt`, the newest one left
    /// behind that can be read; otherwise, and when there is none, a new one.
    pub(crate) fn claim(
        dir: &Path,
        adopt: bool,
        opening: Option<&Path>,
    ) -> std::io::Result<Claimed> {
        std::fs::create_dir_all(dir)?;
        // Locks left by sessions that are gone ("Don't save"), unless a window holds them.
        for entry in std::fs::read_dir(dir)?.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "lock")
                && !path.with_extension("session").exists()
                && let Some(lock) = try_lock(&path)
            {
                drop(lock);
                let _ = std::fs::remove_file(&path);
            }
        }
        let sessions: Vec<PathBuf> = std::fs::read_dir(dir)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "session"))
            .collect();
        let first = sessions.is_empty();
        // Those left behind (a running window holds its own), newest first.
        let mut left: Vec<(PathBuf, File, Option<Kept>)> = sessions
            .into_iter()
            .filter_map(|session| {
                let lock = try_lock(&session)?;
                let kept = read(&session);
                Some((session, lock, kept))
            })
            .collect();
        left.sort_by_key(|(session, _, kept)| {
            std::cmp::Reverse((kept.as_ref().map_or(0, |k| k.at), session.clone()))
        });
        let wanted = left
            .iter()
            .position(|(_, _, kept)| {
                let path = kept.as_ref().and_then(|k| k.path.as_deref());
                opening.zip(path).is_some_and(|(a, b)| same_file(a, b))
            })
            .map(|i| (i, true))
            .or_else(|| {
                // (One that can't be read isn't taken over: the next autosave would replace it.
                // It's offered, to be deleted.)
                adopt
                    .then(|| left.iter().position(|(_, _, kept)| kept.is_some()))
                    .flatten()
                    .map(|i| (i, false))
            });
        let mut claimed = None;
        let mut same_file = false;
        let mut others = Vec::new();
        for (i, (session, lock, kept)) in left.into_iter().enumerate() {
            if let Some((w, same)) = wanted
                && w == i
            {
                claimed = Some((session, lock, kept));
                same_file = same;
            } else if kept.as_ref().is_none_or(|k| k.dirty) {
                // Unsaved changes, or a file that can't be read: offered. The lock goes, so
                // the window that opens it can take it.
                let name = kept.map_or_else(|| "a drawing that can't be read".into(), |k| k.name);
                others.push((session, name));
            } else {
                // Nothing unsaved: the drawing is in its file.
                drop(lock);
                remove(&session);
            }
        }
        let (file, lock, restored) = match claimed {
            Some((file, lock, kept)) => (file, lock, kept),
            None => {
                let file = new_name(dir);
                let lock = try_lock(&file)
                    .ok_or_else(|| std::io::Error::other("could not lock a new session file"))?;
                (file, lock, None)
            }
        };
        let at = restored.as_ref().map_or(0, |k| k.at);
        Ok(Claimed {
            slot: Slot {
                file,
                _lock: lock,
                written: None,
                at,
            },
            restored,
            same_file,
            others,
            first,
        })
    }

    /// Write the session, unless `stamp` is what was last written. `text` gives the drawing as
    /// `.drift` text. Returns whether it wrote.
    pub(crate) fn write(
        &mut self,
        stamp: Stamp,
        text: impl FnOnce() -> String,
    ) -> std::io::Result<bool> {
        if self.written.as_ref() == Some(&stamp) {
            return Ok(false);
        }
        let document: Value = serde_json::from_str(&text()).map_err(std::io::Error::other)?;
        let path = stamp
            .path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        let at = now_ms().max(self.at + 1);
        let body = json!({
            "name": stamp.name,
            "path": path,
            "dirty": stamp.dirty,
            "at": at,
            "document": document,
        });
        drift_platform::write_atomic(
            &self.file,
            body.to_string().as_bytes(),
            drift_platform::Backup::Keep,
        )?;
        self.written = Some(stamp);
        self.at = at;
        Ok(true)
    }

    /// "Don't save" on closing: nothing is kept for next time.
    pub(crate) fn discard(&mut self) {
        let mut bak = self.file.as_os_str().to_owned();
        bak.push(".bak");
        let _ = std::fs::remove_file(&self.file);
        let _ = std::fs::remove_file(&bak);
        // (The lock file goes when another window finds nothing to go with it.)
        self.written = None;
    }
}

/// The window's session files: where they go, and the one it holds.
#[derive(Default)]
pub(crate) struct SessionFiles {
    /// The sessions folder. `None` keeps the session in eframe's storage, as tests do unless
    /// they give a folder.
    pub(crate) dir: Option<PathBuf>,
    /// The file the window was opened for (double-clicked): it takes over only a session of
    /// that file, and the newest left behind is offered to the next window instead.
    pub(crate) opening: Option<PathBuf>,
    /// Opened for a session left behind ([`RESTORING`]): it takes one over and offers no
    /// others.
    pub(crate) restoring: bool,
    pub(crate) slot: Option<Slot>,
    /// Sessions left behind with unsaved changes (or that can't be read): offered.
    pub(crate) left_behind: Vec<Offered>,
    /// "Delete them" was clicked: asking to be sure.
    pub(crate) confirm_delete: bool,
    /// The session taken over was of the file the window was opened for: that file isn't
    /// opened again over it.
    pub(crate) took_over: Option<PathBuf>,
}

impl SessionFiles {
    /// Whether `path` is the file the window was opened for and its session was taken over
    /// instead (said once).
    pub(crate) fn take_over_opening(&mut self, path: &Path) -> bool {
        self.took_over.take_if(|p| same_file(p, path)).is_some()
    }
}

impl crate::DriftApp {
    /// How many other Drift windows are running (0 without session files).
    pub(crate) fn other_windows(&self) -> usize {
        self.session_files.dir.as_deref().map_or(0, |dir| {
            other_windows(dir, self.session_files.slot.as_ref())
        })
    }

    /// The window offering the sessions left behind with unsaved changes: to open each in a
    /// window of its own (which takes it over), or to delete.
    pub(crate) fn left_behind_window(&mut self, ctx: &egui::Context) {
        let n = self.session_files.left_behind.len();
        if n == 0 {
            return;
        }
        enum Choice {
            Later,
            Open,
            Delete,
            Deleted,
            Keep,
        }
        let mut choice = None;
        let confirm = self.session_files.confirm_delete;
        crate::theme::dialog(ctx, "left behind").show(ctx, |ui| {
            ui.set_width(380.0);
            crate::theme::dialog_title(ui, "Drawings left open", false);
            let names: Vec<&str> = self
                .session_files
                .left_behind
                .iter()
                .map(|(_, name)| name.as_str())
                .collect();
            let text = if confirm {
                format!(
                    "Delete {n} unsaved drawing{}? This can't be undone.",
                    if n == 1 { "" } else { "s" }
                )
            } else {
                format!(
                    "{} with unsaved changes {} left open in Drift windows that have closed: {}. \
                     Open {} in new windows?",
                    if n == 1 { "A drawing" } else { "Drawings" },
                    if n == 1 { "was" } else { "were" },
                    names.join(", "),
                    if n == 1 { "it" } else { "them" },
                )
            };
            ui.add(egui::Label::new(text).wrap());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let dark = ui.visuals().dark_mode;
                if confirm {
                    if ui.button("Cancel").clicked() {
                        choice = Some(Choice::Keep);
                    }
                    if ui
                        .add(crate::theme::primary_button("Delete", dark))
                        .clicked()
                    {
                        choice = Some(Choice::Deleted);
                    }
                    return;
                }
                if ui.button("Delete them").clicked() {
                    choice = Some(Choice::Delete);
                }
                if ui.button("Later").clicked() {
                    choice = Some(Choice::Later);
                }
                if ui
                    .add(crate::theme::primary_button("Open them", dark))
                    .clicked()
                {
                    choice = Some(Choice::Open);
                }
            });
        });
        let files = &mut self.session_files;
        match choice {
            // Each new window takes over one; "Later" offers them again next time.
            Some(Choice::Open) => {
                for _ in 0..n {
                    if let Err(e) = open_window() {
                        log::error!("could not open a window for a session left behind: {e}");
                    }
                }
                files.left_behind.clear();
            }
            Some(Choice::Later) => files.left_behind.clear(),
            Some(Choice::Delete) => files.confirm_delete = true,
            Some(Choice::Keep) => files.confirm_delete = false,
            Some(Choice::Deleted) => {
                delete(&files.left_behind);
                log::info!("deleted {n} session(s) left behind");
                files.left_behind.clear();
                files.confirm_delete = false;
            }
            None => {}
        }
    }
}

/// Set on a window opened for a session left behind: it takes one over and offers no others.
pub(crate) const RESTORING: &str = "DRIFT_RESTORING";

/// Start another Drift window, which takes over a session left behind.
fn open_window() -> std::io::Result<()> {
    std::process::Command::new(std::env::current_exe()?)
        .env(RESTORING, "1")
        .spawn()
        .map(|_| ())
}

/// A new session's file name: `session-<time>-<process>.session`.
fn new_name(dir: &Path) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    dir.join(format!("session-{nanos}-{}.session", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("drift-sessions-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn stamp(revision: u64, name: &str, dirty: bool) -> Stamp {
        Stamp {
            revision,
            name: name.into(),
            path: None,
            dirty,
        }
    }

    fn drawing(n: u32) -> String {
        format!("{{\"format\":\"drift\",\"n\":{n}}}")
    }

    fn files(dir: &Path, extension: &str) -> usize {
        std::fs::read_dir(dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|e| e == extension)
            })
            .count()
    }

    /// Two windows autosave to their own sessions; once both have gone, the next window takes
    /// over the newer and offers the other (#217). Newer by what the sessions record, not by
    /// file times (a second apart at best on some systems): here the times say the opposite.
    #[test]
    fn two_windows_keep_two_sessions() {
        let dir = folder("two");
        let mut a = Slot::claim(&dir, true, None).unwrap();
        assert!(a.first && a.restored.is_none());
        let mut b = Slot::claim(&dir, true, None).unwrap();
        assert!(
            b.restored.is_none(),
            "a's session is held by a running window"
        );
        a.slot
            .write(stamp(1, "A.drift", true), || drawing(1))
            .unwrap();
        // B's session written later, but its file time the earlier.
        b.slot.at = a.slot.at;
        b.slot
            .write(stamp(1, "B.drift", true), || drawing(2))
            .unwrap();
        let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        File::options()
            .write(true)
            .open(&b.slot.file)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_ne!(a.slot.file, b.slot.file);
        // Both windows go (a crash lets go of their locks just the same).
        drop((a, b));
        let c = Slot::claim(&dir, true, None).unwrap();
        let restored = c.restored.expect("the newer is taken over");
        assert_eq!(restored.name, "B.drift");
        assert!(restored.text.contains("\"n\":2"));
        assert!(!c.same_file);
        assert_eq!(c.others.len(), 1, "the other is offered");
        assert_eq!(c.others[0].1, "A.drift");
        // A window opened for it takes it over.
        let d = Slot::claim(&dir, true, None).unwrap();
        assert_eq!(d.restored.unwrap().name, "A.drift");
        assert!(d.others.is_empty());
        drop((c.slot, d.slot));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A window opened for a file doesn't take over the newest session left behind, only one
    /// of that same file (named another way, too).
    #[test]
    fn a_window_for_a_file_takes_over_only_that_files_session() {
        let dir = folder("fresh");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Plan B.drift");
        std::fs::write(&file, "on disk").unwrap();
        let mut a = Slot::claim(&dir, true, None).unwrap();
        let mut b = Slot::claim(&dir, true, None).unwrap();
        let on_b = Stamp {
            path: Some(file.clone()),
            ..stamp(1, "Plan B.drift", true)
        };
        b.slot.write(on_b, || drawing(2)).unwrap();
        a.slot.at = b.slot.at;
        a.slot
            .write(stamp(1, "A.drift", true), || drawing(1))
            .unwrap();
        drop((a, b));
        // Another file: nothing taken over, both offered.
        let other = dir.join("Other.drift");
        let c = Slot::claim(&dir, false, Some(&other)).unwrap();
        assert!(c.restored.is_none());
        assert_eq!(c.others.len(), 2);
        drop(c.slot);
        // Plan B (spelt differently): its session, with the unsaved changes, though A's is
        // newer.
        let spelt = if cfg!(windows) {
            dir.join(".").join("PLAN B.drift")
        } else {
            dir.join(".").join("Plan B.drift")
        };
        let d = Slot::claim(&dir, false, Some(&spelt)).unwrap();
        assert!(d.same_file);
        assert_eq!(d.restored.unwrap().name, "Plan B.drift");
        assert_eq!(d.others.len(), 1);
        assert_eq!(d.others[0].1, "A.drift");
        drop(d.slot);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A session left without unsaved changes isn't offered: it's deleted.
    #[test]
    fn a_clean_session_left_behind_is_not_offered() {
        let dir = folder("clean");
        let mut a = Slot::claim(&dir, true, None).unwrap();
        let mut b = Slot::claim(&dir, true, None).unwrap();
        a.slot
            .write(stamp(1, "A.drift", false), || drawing(1))
            .unwrap();
        b.slot.at = a.slot.at;
        b.slot
            .write(stamp(1, "B.drift", false), || drawing(2))
            .unwrap();
        drop((a, b));
        let c = Slot::claim(&dir, true, None).unwrap();
        assert_eq!(c.restored.unwrap().name, "B.drift");
        assert!(c.others.is_empty());
        assert_eq!(files(&dir, "session"), 1);
        drop(c.slot);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A session that can't be read (not even its copy) isn't taken over, where the next
    /// autosave would replace it: it's offered, and can be deleted.
    #[test]
    fn an_unreadable_session_is_offered_not_taken_over() {
        let dir = folder("unreadable");
        let mut a = Slot::claim(&dir, true, None).unwrap();
        a.slot
            .write(stamp(1, "A.drift", true), || drawing(1))
            .unwrap();
        let file = a.slot.file.clone();
        drop(a);
        std::fs::write(&file, "{ cut").unwrap();
        let _ = std::fs::remove_file(PathBuf::from(format!("{}.bak", file.display())));
        let b = Slot::claim(&dir, true, None).unwrap();
        assert!(b.restored.is_none());
        assert_eq!(b.others.len(), 1);
        assert_eq!(b.others[0].1, "a drawing that can't be read");
        delete(&b.others);
        assert!(!file.exists());
        drop(b.slot);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Delete them leaves alone a session a window has taken over since.
    #[test]
    fn delete_them_spares_a_session_taken_over() {
        let dir = folder("delete");
        let mut a = Slot::claim(&dir, true, None).unwrap();
        let mut b = Slot::claim(&dir, true, None).unwrap();
        a.slot
            .write(stamp(1, "A.drift", true), || drawing(1))
            .unwrap();
        b.slot
            .write(stamp(1, "B.drift", true), || drawing(2))
            .unwrap();
        let (fa, fb) = (a.slot.file.clone(), b.slot.file.clone());
        drop((a, b));
        let offered = vec![(fa.clone(), "A".into()), (fb.clone(), "B".into())];
        // A window takes B over meanwhile.
        let held = try_lock(&fb).unwrap();
        delete(&offered);
        assert!(!fa.exists() && !lock_path(&fa).exists());
        assert!(fb.exists());
        drop(held);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Locks left by sessions that are gone are swept up; a running window's stays.
    #[test]
    fn stray_locks_are_swept_up() {
        let dir = folder("locks");
        std::fs::create_dir_all(&dir).unwrap();
        let stray = dir.join("session-1-1.lock");
        std::fs::write(&stray, "").unwrap();
        let mut a = Slot::claim(&dir, true, None).unwrap();
        assert!(!stray.exists(), "swept up");
        // A running window that discarded its session keeps its lock.
        a.slot.discard();
        let b = Slot::claim(&dir, true, None).unwrap();
        assert!(lock_path(&a.slot.file).exists());
        drop((a, b));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The updater's check: other windows' held locks count, this window's doesn't, nor
    /// those of windows that have gone.
    #[test]
    fn other_windows_are_counted_by_their_locks() {
        let dir = folder("others");
        let a = Slot::claim(&dir, true, None).unwrap();
        assert_eq!(other_windows(&dir, Some(&a.slot)), 0);
        let b = Slot::claim(&dir, true, None).unwrap();
        let c = Slot::claim(&dir, true, None).unwrap();
        assert_eq!(other_windows(&dir, Some(&a.slot)), 2);
        drop(c);
        assert_eq!(other_windows(&dir, Some(&a.slot)), 1);
        assert_eq!(other_windows(&dir, None), 2);
        drop((a, b));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Nothing changed since the last write: nothing is written.
    #[test]
    fn an_unchanged_session_isnt_written_again() {
        let dir = folder("unchanged");
        let mut a = Slot::claim(&dir, true, None).unwrap();
        assert!(
            a.slot
                .write(stamp(1, "A.drift", true), || drawing(1))
                .unwrap()
        );
        assert!(
            !a.slot
                .write(stamp(1, "A.drift", true), || unreachable!())
                .unwrap()
        );
        assert!(
            a.slot
                .write(stamp(2, "A.drift", true), || drawing(2))
                .unwrap()
        );
        assert!(
            a.slot
                .write(stamp(2, "A.drift", false), || drawing(2))
                .unwrap()
        );
        drop(a);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A session file cut short is read from the copy before it.
    #[test]
    fn a_cut_short_session_falls_back_to_the_one_before() {
        let dir = folder("cut");
        let mut a = Slot::claim(&dir, true, None).unwrap();
        a.slot
            .write(stamp(1, "A.drift", true), || drawing(1))
            .unwrap();
        a.slot
            .write(stamp(2, "A.drift", true), || drawing(2))
            .unwrap();
        let file = a.slot.file.clone();
        drop(a);
        let text = std::fs::read(&file).unwrap();
        std::fs::write(&file, &text[..text.len() / 2]).unwrap();
        let b = Slot::claim(&dir, true, None).unwrap();
        let restored = b.restored.expect("the copy before");
        assert!(restored.text.contains("\"n\":1"), "{}", restored.text);
        drop(b.slot);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// "Don't save" on closing leaves nothing to restore.
    #[test]
    fn a_discarded_session_is_gone() {
        let dir = folder("discard");
        let mut a = Slot::claim(&dir, true, None).unwrap();
        a.slot
            .write(stamp(1, "A.drift", true), || drawing(1))
            .unwrap();
        a.slot.discard();
        drop(a);
        let b = Slot::claim(&dir, true, None).unwrap();
        assert!(b.restored.is_none());
        assert!(b.others.is_empty());
        drop(b.slot);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
