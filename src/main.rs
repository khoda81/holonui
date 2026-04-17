#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};

struct HolonApp;

impl HolonApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Initialize your Skyline state here in the future
        Self {}
    }
}

impl eframe::App for HolonApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("HolonUI - Skyline Visualizer");

            let plot = Plot::new("skyline_plot")
                .view_aspect(2.0)
                .show_grid(true)
                .legend(egui_plot::Legend::default());
            
            plot.show(ui, |plot_ui| {
                // Mock a boundary representing your exponential space intents
                let points: PlotPoints = (0..100)
                    .map(|i| {
                        let x = i as f64 * 0.05;
                        [x, x.exp()] // y = e^x
                    })
                    .collect();
                
                let line = Line::new(points).name("Intent Frontier");
                plot_ui.line(line);
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    // ----------------------------------------------------
    // WebAssembly Initialization
    // ----------------------------------------------------
    #[cfg(target_arch = "wasm32")]
    {
        eframe::WebLogger::init(log::LevelFilter::Debug).ok();
        let web_options = eframe::WebOptions::default();
        
        wasm_bindgen_futures::spawn_local(async {
            eframe::WebRunner::new()
                .start(
                    "the_canvas_id", // Must match index.html
                    web_options,
                    Box::new(|cc| Box::new(HolonApp::new(cc))),
                )
                .await
                .expect("failed to start eframe");
        });
        Ok(())
    }

    // ----------------------------------------------------
    // Native Desktop Initialization (for fast local dev)
    // ----------------------------------------------------
    #[cfg(not(target_arch = "wasm32"))]
    {
        let native_options = eframe::NativeOptions::default();
        eframe::run_native(
            "HolonUI",
            native_options,
            Box::new(|cc| Box::new(HolonApp::new(cc))),
        )
    }
}
