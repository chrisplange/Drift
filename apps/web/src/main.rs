//! Browser entry point. Build and serve with `trunk serve` from this directory.

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;
    use eframe::web_sys;

    eframe::WebLogger::init(log::LevelFilter::Info).ok();

    let window = web_sys::window().expect("no window");
    let mut web_options = eframe::WebOptions::default();
    // `?backend=webgl` forces the WebGL2 fallback, for testing browsers without WebGPU.
    let search = window.location().search().unwrap_or_default();
    if search.contains("backend=webgl")
        && let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) =
            &mut web_options.wgpu_options.wgpu_setup
    {
        setup.instance_descriptor.backends = eframe::egui_wgpu::wgpu::Backends::GL;
    }

    wasm_bindgen_futures::spawn_local(async move {
        let document = window.document().expect("no document");
        let canvas = document
            .get_element_by_id("drift_canvas")
            .expect("missing #drift_canvas")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("#drift_canvas is not a canvas");

        let result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(drift_app::DriftApp::new(cc)))),
            )
            .await;

        if let Some(loading) = document.get_element_by_id("loading") {
            match result {
                Ok(()) => loading.remove(),
                Err(err) => {
                    log::error!("Drift failed to start: {err:?}");
                    loading.set_inner_html(
                        "<p>Drift failed to start.</p>\
                         <p>Your browser may not support WebGPU or WebGL2. \
                         See the developer console for details.</p>",
                    );
                }
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!(
        "drift-web is the browser build: run `trunk serve` in apps/web.\n\
         For the desktop app, run `cargo run -p drift`."
    );
}
