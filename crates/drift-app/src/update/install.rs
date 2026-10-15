//! Installing a verified update once Drift has closed.
//!
//! Windows: the Inno Setup installer runs silently (`/RELAUNCH=1` makes it open Drift again).
//! macOS: the app is copied out of the disk image beforehand, and a small shell script waits
//! for Drift to quit, puts the new bundle in place of the old one and opens it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::Platform;

/// How this copy of Drift was installed, which decides whether it can update itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Install {
    /// With the Windows installer.
    Installer,
    /// As a macOS app bundle, here.
    Bundle(PathBuf),
    /// From the zip, a build, or on a system without an installer: update by hand.
    Manual,
}

impl Install {
    pub(crate) fn current() -> Self {
        match std::env::current_exe() {
            Ok(exe) => Self::from_exe(&exe, Platform::current()),
            Err(_) => Self::Manual,
        }
    }

    pub(crate) fn from_exe(exe: &Path, platform: Option<Platform>) -> Self {
        match platform {
            // Inno Setup leaves its uninstaller beside the program.
            Some(Platform::Windows)
                if exe
                    .parent()
                    .is_some_and(|dir| dir.join("unins000.exe").is_file()) =>
            {
                Self::Installer
            }
            Some(Platform::MacOs) => bundle_of(exe).map_or(Self::Manual, Self::Bundle),
            _ => Self::Manual,
        }
    }

    pub(crate) fn can_update(&self) -> bool {
        *self != Self::Manual
    }
}

/// The `.app` bundle an executable is in (`Drift.app/Contents/MacOS/drift`).
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let is_app = bundle
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("app"));
    (macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && is_app)
        .then(|| bundle.to_owned())
}

/// What to do once Drift has closed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    Windows { installer: PathBuf },
    MacOs { staged: PathBuf, bundle: PathBuf },
}

/// The Windows installer's options: no questions, close a running Drift, open it after.
pub(crate) const WINDOWS_ARGS: [&str; 5] = [
    "/SILENT",
    "/SUPPRESSMSGBOXES",
    "/NORESTART",
    "/CLOSEAPPLICATIONS",
    "/RELAUNCH=1",
];

/// Waits for Drift (`$1`, its process id) to quit, swaps the staged bundle (`$2`) in for the
/// installed one (`$3`), putting the old one back if that fails, and opens it.
pub(crate) const MAC_SWAP: &str = r#"pid="$1"; staged="$2"; app="$3"
while kill -0 "$pid" 2>/dev/null; do sleep 0.2; done
old="$app.old-$$"
mv "$app" "$old" || { open "$app"; exit 1; }
if mv "$staged" "$app"; then rm -rf "$old"; else mv "$old" "$app"; fi
xattr -dr com.apple.quarantine "$app" 2>/dev/null
open "$app"
"#;

/// Get a verified download ready to install. On macOS this copies the app out of the disk
/// image, next to the download.
pub(crate) fn prepare(download: &Path, install: &Install) -> Result<Plan, String> {
    match install {
        Install::Installer => Ok(Plan::Windows {
            installer: download.to_owned(),
        }),
        Install::Bundle(bundle) => {
            let folder = bundle.parent().unwrap_or(Path::new("/"));
            if !can_write(folder) {
                return Err(format!(
                    "Drift can't replace itself in {}: install the update by hand",
                    folder.display()
                ));
            }
            let dir = download.parent().unwrap_or(Path::new("."));
            let staged = stage_from_dmg(download, dir)?;
            Ok(Plan::MacOs {
                staged,
                bundle: bundle.clone(),
            })
        }
        Install::Manual => Err("this copy of Drift can't update itself".into()),
    }
}

fn can_write(folder: &Path) -> bool {
    let probe = folder.join(format!(".drift-update-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Mount the disk image, copy the app out of it, and unmount it.
fn stage_from_dmg(dmg: &Path, dir: &Path) -> Result<PathBuf, String> {
    let mount = dir.join("mount");
    let staged = dir.join("Drift.app");
    let _ = std::fs::remove_dir_all(&staged);
    std::fs::create_dir_all(&mount).map_err(|e| e.to_string())?;
    run(Command::new("hdiutil")
        .args([
            "attach",
            "-nobrowse",
            "-readonly",
            "-noautoopen",
            "-mountpoint",
        ])
        .arg(&mount)
        .arg(dmg))?;
    let copied = app_in(&mount)
        .ok_or_else(|| "the disk image has no app in it".to_owned())
        .and_then(|app| run(Command::new("ditto").arg(app).arg(&staged)));
    let _ = run(Command::new("hdiutil").arg("detach").arg(&mount)).or_else(|_| {
        run(Command::new("hdiutil")
            .args(["detach", "-force"])
            .arg(&mount))
    });
    copied.map(|()| staged)
}

fn app_in(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("app")))
}

fn run(command: &mut Command) -> Result<(), String> {
    let name = command.get_program().to_string_lossy().into_owned();
    let out = command
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not run {name}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{name} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Start the install, to carry on after Drift has quit.
pub(crate) fn launch(plan: &Plan) -> Result<(), String> {
    let mut command = match plan {
        Plan::Windows { installer } => {
            let mut c = Command::new(installer);
            c.args(WINDOWS_ARGS);
            c
        }
        Plan::MacOs { staged, bundle } => {
            let mut c = Command::new("/bin/sh");
            c.arg("-c")
                .arg(MAC_SWAP)
                .arg("sh")
                .arg(std::process::id().to_string())
                .arg(staged)
                .arg(bundle);
            c
        }
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
        .map_err(|e| format!("could not start the update: {e}"))
}
