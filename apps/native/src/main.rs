// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    drift_app::start_memory_guard();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Drift")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([480.0, 320.0]),
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
        Box::new(|cc| Ok(Box::new(drift_app::DriftApp::new(cc)))),
    )
}
