//! Gives the Windows executable its icon and version details (what Explorer, the taskbar
//! and the installer show). Other platforms take theirs from the app bundle.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("assets/icon.ico")
            .set("ProductName", "Drift")
            .set("FileDescription", "Drift")
            .set("CompanyName", "Drift")
            .set("LegalCopyright", "MIT OR Apache-2.0")
            .set("OriginalFilename", "drift.exe");
        // Without the Windows SDK's resource compiler the app still builds, just without
        // an icon of its own.
        if let Err(e) = resource.compile() {
            println!("cargo:warning=no icon embedded in drift.exe: {e}");
        }
    }
}
