use eframe::egui;
use egui_plot::{Line, Plot, PlotPoint, PlotPoints};
use std::env;
use std::sync::mpsc::{Receiver, Sender};

pub type Allocation = [f64; 2];

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

pub struct HolonApp {
    trajectory: Vec<Allocation>,
    mode: AppMode,
    api_receiver: Receiver<Allocation>, // Receives data from the async task
}

impl HolonApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, rx: Receiver<Allocation>) -> Self {
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
        }

        // 2. Render UI
        self.handle_keyboard(ui);
        self.render_main_plot(ui);
    }
}

// The Async API Call
pub async fn fetch_nobitex_data(tx: Sender<Allocation>) {
    let api_key = env::var("NOBITEX_API_KEY").expect("NOBITEX_API_KEY must be set");
    let client = reqwest::Client::new();

    // Example API Call using the Nobitex USDT endpoint
    let url =
        reqwest::Url::parse("https://apiv2.nobitex.ir/users/transactions-history?currency=usdt")
            .expect("failed to parse url");

    let json: serde_json::Value = client
        .get(url)
        .header("Authorization", format!("Token {}", api_key))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    log::info!("{json}");

    // 1. Extract the "transactions" array
    if let Some(transactions) = json.get("transactions").and_then(|t| t.as_array()) {
        let mut trade_index = 0.0;

        // 2. Iterate in reverse (`.rev()`) to go chronologically (oldest to newest)
        for tx_data in transactions.iter().rev() {
            // Extract and parse the balance string into an f64
            let balance: f64 = tx_data
                .get("balance")
                .and_then(|b| b.as_str())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);

            // Optional: Extract the trade amount if you need it
            // let amount: f64 = tx_data.get("amount").and_then(|a| a.as_str()).and_then(|s| s.parse().ok()).unwrap_or(0.0);

            // 3. Define your X and Y.
            // Here we use X = trade number, Y = USDT Balance
            let x = trade_index;
            let y = balance;

            // Send the parsed point to the UI
            let _ = tx.send([x, y]);
            trade_index += 1.0;
        }
    } else {
        log::error!("Failed to find 'transactions' array in API response");
    }
}
