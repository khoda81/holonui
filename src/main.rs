#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoint, PlotPoints};

type Allocation = [f64; 2];

struct HolonApp {
    trajectory: Vec<Allocation>,
    state: AppState,
}

#[derive(Debug, Default)]
enum AppState {
    #[default]
    Default,
    GhostLine,
}

impl AppState {
    /// Returns `true` if the app state is [`GhostLine`].
    ///
    /// [`GhostLine`]: AppState::GhostLine
    #[must_use]
    fn is_ghost_line(&self) -> bool {
        matches!(self, Self::GhostLine)
    }
}

impl HolonApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Initialize your Skyline state here in the future
        Self {
            state: AppState::Default,
            trajectory: vec![[1.0, 2.0], [2.0, 1.0], [2.0, 2.0]],
        }
    }

    fn handle_keyboard(&mut self, ui: &mut egui::Ui) {
        if ui.input(|i| i.key_pressed(egui::Key::O)) {
            self.state = match self.state {
                AppState::Default => AppState::GhostLine,
                AppState::GhostLine => AppState::Default,
            };
        }
    }

    fn render_main_plot(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.heading("HolonUI - Skyline Visualizer");

            let scale_plot = !self.state.is_ghost_line();

            let plot = Plot::new("skyline_plot")
                // .view_aspect(2.0)
                .show_grid(true)
                .legend(egui_plot::Legend::default())
                .include_x(0.0)
                .include_y(0.0)
                .allow_double_click_reset(scale_plot)
                .auto_bounds(scale_plot);

            plot.show(ui, |ui| self.render_plot_contents(ui));
        });
    }

    fn render_plot_contents(&mut self, ui: &mut egui_plot::PlotUi<'_>) {
        ui.line(Line::new("Intent Frontier", self.trajectory.clone()));
        let start_point = self.trajectory.last().copied().unwrap_or([0.0, 0.0]);
        if let AppState::Default = self.state {
            return;
        }

        if let Some(mouse_pos) = ui.pointer_coordinate() {
            let ghost_points = vec![start_point, [mouse_pos.x, mouse_pos.y]];

            let ghost_line = Line::new("Ghost Line", PlotPoints::new(ghost_points))
                .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 150))
                .style(egui_plot::LineStyle::Dashed { length: 5.0 });

            ui.line(ghost_line);
        }

        if ui.response().clicked()
            && let Some(PlotPoint { x, y }) = ui.pointer_coordinate()
        {
            self.trajectory.push([x, y]);
            log::info!("Clicked plot at X: {x}, Y: {y}");
            self.state = AppState::Default;
        }
    }
}

impl eframe::App for HolonApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_keyboard(ui);

        self.render_main_plot(ui);
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
