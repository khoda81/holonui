#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};

type Allocation = [f64; 2];

struct HolonApp {
    is_active: bool,
    trajectory: Vec<Allocation>,
    state: AppState,
}

#[derive(Debug, Default)]
enum AppState {
    #[default]
    Default,
    GhostLine,
}

impl HolonApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Initialize your Skyline state here in the future
        Self {
            is_active: false,
            state: AppState::Default,
            trajectory: vec![[1.0, 2.0], [2.0, 1.0], [2.0, 2.0]],
        }
    }
}

impl eframe::App for HolonApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if ui.input(|i| i.key_pressed(egui::Key::O)) {
            self.state = match self.state {
                AppState::Default => AppState::GhostLine,
                AppState::GhostLine => AppState::Default,
            };
        }

        egui::Panel::right("order_book_panel")
            .resizable(true) // User can drag the width
            .show_inside(ui, |side_ui| {
                side_ui.heading("Order Book");
                side_ui.label("Waiting for orders...");
                side_ui.take_available_width();
            });

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

        egui::CentralPanel::default().show_inside(ui, |panel_ui| {
            panel_ui.heading("HolonUI - Skyline Visualizer");

            let plot = Plot::new("skyline_plot")
                // .view_aspect(2.0)
                .show_grid(true)
                .legend(egui_plot::Legend::default())
                .include_x(0.0)
                .include_y(0.0);

            plot.show(panel_ui, |plot_ui| {
                plot_ui.line(Line::new("Intent Frontier", self.trajectory.clone()));

                let start_point = [2.0, 2.0];

                if let AppState::GhostLine = self.state
                    && let Some(mouse_pos) = plot_ui.pointer_coordinate()
                {
                    let ghost_points = vec![start_point, [mouse_pos.x, mouse_pos.y]];

                    let ghost_line = Line::new("Ghost Line", PlotPoints::new(ghost_points))
                        .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 150))
                        .style(egui_plot::LineStyle::Dashed { length: 5.0 });

                    plot_ui.line(ghost_line);
                }

                if plot_ui.response().clicked()
                    && let Some(click_pos) = plot_ui.pointer_coordinate()
                {
                    log::info!("Clicked plot at X: {}, Y: {}", click_pos.x, click_pos.y);
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
