#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};

struct HolonApp {
    is_active: bool,
}

impl HolonApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Initialize your Skyline state here in the future
        Self { is_active: false }
    }
}

impl eframe::App for HolonApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().set_debug_on_hover(true);

        // 1. Reserve the top section for your controls
        egui::Panel::top("top_control_bar").show_inside(ui, |top_ui| {
            top_ui.horizontal(|ui| {
                ui.heading("Holon Terminal");
                ui.separator(); // Adds a little vertical line

                if ui.button("Place Order").clicked() {
                    // self.place_order();
                }
                ui.checkbox(&mut self.is_active, "Snap to grid");
            });
        });

        // 2. You could also add a side panel for a ledger/history!
        egui::Panel::right("order_book_panel")
            .resizable(true) // User can drag the width
            .show_inside(ui, |side_ui| {
                side_ui.heading("Order Book");
                side_ui.label("Waiting for orders...");
            });

        egui::CentralPanel::default().show_inside(ui, |panel_ui| {
            panel_ui.heading("HolonUI - Skyline Visualizer");

            let plot = Plot::new("skyline_plot")
                // .view_aspect(2.0)
                .show_grid(true)
                .legend(egui_plot::Legend::default());

            plot.show(panel_ui, |plot_ui| {
                // 1. Draw your fixed stuff (the frontier boundary)
                let points: PlotPoints = (0..100)
                    .map(|i| {
                        let x = i as f64 * 0.05;
                        [x, x.exp()]
                    })
                    .collect();
                plot_ui.line(Line::new("Intent Frontier", points));

                let start_point = [2.0, 2.0];

                // 2. Capture the mouse position inside the plot
                // This returns an Option<PlotPoint> containing the actual X/Y math coordinates
                if let Some(mouse_pos) = plot_ui.pointer_coordinate() {
                    // 3. Draw the ghost line from the start point to the mouse
                    let ghost_points = vec![start_point, [mouse_pos.x, mouse_pos.y]];

                    let ghost_line = Line::new("Ghost Line", PlotPoints::new(ghost_points))
                        .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 150)) // Semi-transparent white
                        .style(egui_plot::LineStyle::Dashed { length: 5.0 }); // Make it dashed!

                    plot_ui.line(ghost_line);
                }

                if plot_ui.response().clicked()
                    && let Some(click_pos) = plot_ui.pointer_coordinate()
                {
                    println!("Clicked plot at X: {}, Y: {}", click_pos.x, click_pos.y);
                    // Here you would save `click_pos` to your HolonApp struct's state
                    // to make the line permanent!
                }
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
                    "the_canvas_id",
                    web_options,
                    // FIX: Wrapped the return value in Ok()
                    Box::new(|cc| Ok(Box::new(HolonApp::new(cc)))),
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
            // FIX: Wrapped the return value in Ok()
            Box::new(|cc| Ok(Box::new(HolonApp::new(cc)))),
        )
    }
}
