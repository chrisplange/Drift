// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui;

/// The window was last left maximised: it is created at that size, and maximised once it has
/// something to show (see `DriftApp::maximize_when_shown`).
static START_MAXIMIZED: AtomicBool = AtomicBool::new(false);

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    drift_app::install_crash_log();
    drift_app::start_memory_guard();

    // The window icon (drawn by assets/make_icon.py).
    let icon =
        eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Drift")
            .with_icon(icon)
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([480.0, 320.0]),
        // After eframe restores the window's last size and place: not maximised yet, since
        // Windows shows a window created maximised before anything is drawn in it.
        window_builder: Some(Box::new(|builder| {
            if builder.maximized == Some(true) {
                START_MAXIMIZED.store(true, Ordering::Relaxed);
                builder.with_maximized(false)
            } else {
                builder
            }
        })),
        ..Default::default()
    };
    // A soak test (DRIFT_SOAK) keeps its own settings and session.
    let app_id = if std::env::var_os("DRIFT_SOAK").is_some() {
        "DriftSoak"
    } else {
        "Drift"
    };
    eframe::run_native(
        app_id,
        options,
        Box::new(|cc| {
            let mut app = drift_app::DriftApp::new(cc);
            if START_MAXIMIZED.load(Ordering::Relaxed) {
                app.maximize_when_shown();
            }
            // A sketch named on the command line (double-clicked in the file manager) opens.
            if let Some(path) = std::env::args_os().nth(1).map(std::path::PathBuf::from)
                && path.is_file()
            {
                app.open_file(path);
            }
            Ok(Box::new(app))
        }),
    )
}
