#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use holonui::*;

#[cfg(target_arch = "wasm32")]
fn main() -> eframe::Result<()> {
    use web_sys::wasm_bindgen::JsCast;

    eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    let document = web_sys::window().unwrap().document().unwrap();
    let canvas_element = document
        .get_element_by_id("the_canvas_id")
        .expect("Failed to find canvas element");

    let canvas = canvas_element
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .expect("Element is not a canvas");

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(HolonApp::new(cc)))),
            )
            .await
            .expect("failed to start eframe");
    });
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    // 1. Load Environment Variables & Setup Logging
    dotenvy::dotenv().ok();
    env_logger::init(); // Requires `RUST_LOG=info` environment variable

    log::info!("Starting HolonApp...");

    // 4. Start the native UI
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "Holon eFrame App",
        native_options,
        Box::new(|cc| Ok(Box::new(HolonApp::new(cc)))),
    )
}
