#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use dotenvy::dotenv;
use eframe::egui;
use egui_plot::{Line, Plot, PlotPoint, PlotPoints};
use std::env;
use std::sync::mpsc::{self, Receiver, Sender};

type Allocation = [f64; 2];

#[derive(Debug, Default)]
enum AppMode {
    #[default]
    Default,
    GhostLine,
}

impl AppMode {
    /// Returns `true` if the app state is [`GhostLine`].
    ///
    /// [`GhostLine`]: AppMode::GhostLine
    #[must_use]
    fn is_ghost_line(&self) -> bool {
        matches!(self, Self::GhostLine)
    }
}

struct HolonApp {
    trajectory: Vec<Allocation>,
    mode: AppMode,
    api_receiver: Receiver<Allocation>, // Receives data from the async task
}

impl HolonApp {
    fn new(_cc: &eframe::CreationContext<'_>, rx: Receiver<Allocation>) -> Self {
        Self {
            mode: AppMode::Default,
            trajectory: vec![[0.0, 0.0]], // Start at origin
            api_receiver: rx,
        }
    }

    fn handle_keyboard(&mut self, ui: &mut egui::Ui) {
        if ui.input(|i| i.key_pressed(egui::Key::O)) {
            self.mode = match self.mode {
                AppMode::Default => AppMode::GhostLine,
                AppMode::GhostLine => AppMode::Default,
            };
        }
    }

    fn render_main_plot(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.heading("HolonUI - Skyline Visualizer");

            let scale_plot = !self.mode.is_ghost_line();

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
        if let AppMode::Default = self.mode {
            return;
        }

        if let Some(mouse_pos) = ui.pointer_coordinate() {
            let start_point = self.trajectory.last().copied().unwrap_or([0.0, 0.0]);
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
            self.mode = AppMode::Default;
        }
    }
}

impl eframe::App for HolonApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 1. Drain the channel to update trajectory smoothly without blocking UI
        while let Ok(new_allocation) = self.api_receiver.try_recv() {
            self.trajectory.push(new_allocation);
            log::info!("Added new API allocation: {:?}", new_allocation);
        }

        // 2. Render UI
        self.handle_keyboard(ui);
        self.render_main_plot(ui);
    }
}

// The Async API Call
async fn fetch_nobitex_data(tx: Sender<Allocation>) {
    let api_key = env::var("NOBITEX_API_KEY").expect("NOBITEX_API_KEY must be set");
    let client = reqwest::Client::new();

    // Example API Call using the Nobitex USDT endpoint
    match client
        .get("https://apiv2.nobitex.ir/users/transactions-history?currency=usdt")
        .header("Authorization", format!("Token {}", api_key))
        .send()
        .await
    {
        Ok(response) => {
            if let Ok(json) = response.json::<serde_json::Value>().await {
                // TODO: Parse the `json` to extract your $x$ and $y$ values.
                // Assuming you extract a transaction amount and map it:
                let example_parsed_allocation: Allocation = [100.0, 50000.0];

                // Send the calculated allocation back to the UI thread
                let _ = tx.send(example_parsed_allocation);
            }
        }
        Err(e) => log::error!("Failed to fetch API: {}", e),
    }
}

#[cfg(target_arch = "wasm32")]
fn main() -> eframe::Result<()> {
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
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), eframe::Error> {
    // 1. Load Environment Variables & Setup Logging
    dotenv().ok();
    env_logger::init(); // Requires `RUST_LOG=info` environment variable

    log::info!("Starting HolonApp...");

    // 2. Setup communication channel between Tokio and egui
    let (tx, rx) = mpsc::channel::<Allocation>();

    // 3. Spawn Tokio runtime in a background thread
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            fetch_nobitex_data(tx).await;
        });
    });

    // 4. Start the native UI
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "Holon eFrame App",
        native_options,
        Box::new(|cc| Ok(Box::new(HolonApp::new(cc, rx)))),
    )
}
