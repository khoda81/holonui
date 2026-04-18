use chrono::{DateTime, Utc};
use eframe::egui;
use egui_plot::{Line, Plot, PlotPoint, PlotPoints};
use serde_json::Value;
use std::env;
use std::sync::mpsc::{Receiver, Sender};

pub type Allocation = [f64; 2];

pub enum BackendEvent {
    SyncStarted,
    FetchedTrades { trajectory: Vec<Allocation> },
    FetchedBalances { anchor: Allocation },
    SyncFailed(String),
}

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
    anchor: Allocation,
    mode: AppMode,
    api_receiver: Receiver<BackendEvent>, // Receives data from the async task
}

impl HolonApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx_clone = cc.egui_ctx.clone();

        spawn_local(async move {
            NetworkBackend::new(tx, ctx_clone)
                .fetch_nobitex_data()
                .await
        });

        Self {
            mode: AppMode::Default,
            trajectory: vec![], // Start at origin
            anchor: [0.0, 0.0],
            api_receiver: rx,
        }
    }

    // The Update function: The ONLY place where state changes happen.
    // Call this once per frame before drawing.
    pub fn update(&mut self) {
        // Process all pending messages from the background
        while let Ok(event) = self.api_receiver.try_recv() {
            match event {
                BackendEvent::SyncStarted => {}
                BackendEvent::FetchedBalances { anchor } => self.anchor = anchor,
                BackendEvent::FetchedTrades { trajectory } => self.trajectory = trajectory,
                // TODO:
                BackendEvent::SyncFailed(err) => _ = err,
            }
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
        self.update();

        self.handle_keyboard(ui);
        self.render_main_plot(ui);
    }
}

pub struct NetworkBackend {
    sender: Sender<BackendEvent>,
    ctx: egui::Context,
    client: reqwest::Client,
    api_key: String,
}

impl NetworkBackend {
    pub fn new(sender: Sender<BackendEvent>, ctx: egui::Context) -> Self {
        Self {
            sender,
            ctx,
            client: reqwest::Client::new(),
            api_key: env::var("NOBITEX_API_KEY").unwrap_or_default(),
        }
    }

    pub async fn fetch_nobitex_data(&self) {
        // 1. Fetch all pages concurrently
        let (rls_data, usdt_data) = futures::join!(
            self.fetch_all_transactions("rls"),
            self.fetch_all_transactions("usdt")
        );

        let mut timeline = Vec::new();

        // 2. Parse and push to a unified timeline
        let mut parse_into_timeline = |transactions: Vec<Value>, currency: &str| {
            for tx_data in transactions {
                let balance = tx_data
                    .get("balance")
                    .and_then(|b| b.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);

                let desc = tx_data
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();

                let time_str = tx_data
                    .get("created_at")
                    .and_then(|t| t.as_str())
                    .unwrap_or("");

                if let Ok(time) = time_str.parse::<DateTime<Utc>>() {
                    timeline.push(RawTx {
                        time,
                        currency: currency.to_string(),
                        balance,
                        description: desc,
                    });
                }
            }
        };

        parse_into_timeline(rls_data, "rls");
        parse_into_timeline(usdt_data, "usdt");

        // 3. Sort chronologically (oldest first)
        timeline.sort_by_key(|t| t.time);

        // 4. State tracking & Deduplication (Pairing)
        let mut current_usdt = 0.0;
        let mut current_rls = 0.0;

        let mut i = 0;
        while i < timeline.len() {
            let current_tx = &timeline[i];

            // Update the balance for the current transaction
            if current_tx.currency == "usdt" {
                current_usdt = current_tx.balance;
            } else {
                current_rls = current_tx.balance;
            }

            // Look ahead to see if the VERY NEXT transaction is the other half of this trade
            // (Must be within ~10 seconds and have the exact same description)
            if i + 1 < timeline.len() {
                let next_tx = &timeline[i + 1];
                let time_diff = (next_tx.time - current_tx.time).num_seconds().abs();

                if time_diff < 10
                    && next_tx.description == current_tx.description
                    && next_tx.currency != current_tx.currency
                {
                    // It's a pair! Update the other balance immediately before emitting the coordinate
                    if next_tx.currency == "usdt" {
                        current_usdt = next_tx.balance;
                    } else {
                        current_rls = next_tx.balance;
                    }

                    // Skip the next one since we just processed it
                    i += 1;
                }
            }

            // Emit the synchronized coordinate (No staircases!)
            // X = USDT, Y = IRT
            // let _ = self.sender.send([current_usdt, current_rls]);
            i += 1;
        }
    }

    // Updated fetching function with pagination
    async fn fetch_all_transactions(&self, currency: &str) -> Vec<Value> {
        const BASE_URL: &str = r"https://apiv2.nobitex.ir/users/transactions-history";
        let mut all_transactions = Vec::new();
        let mut page = 1;

        loop {
            let mut url = reqwest::Url::parse(BASE_URL).unwrap();
            // The API uses `page` for pagination
            url.set_query(Some(&format!("currency={}&page={}", currency, page)));

            let mut json: Value = self
                .client
                .get(url.clone())
                .header("Authorization", format!("Token {}", self.api_key))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();

            if let Some(transactions) = json["transactions"].take().as_array_mut() {
                if transactions.is_empty() {
                    break; // No more data
                }

                let count = transactions.len();
                all_transactions.append(transactions);

                // If we received fewer than the default 50, we've hit the last page
                if count < 50 {
                    break;
                }
                page += 1;
            } else {
                break; // Handle unexpected JSON structure
            }
        }

        all_transactions
    }
}

// Helper struct for sorting and merging
#[derive(Debug, Clone)]
struct RawTx {
    time: DateTime<Utc>,
    currency: String,
    balance: f64,
    description: String,
}

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_local<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    // On native, spin up a background thread and a lightweight Tokio runtime
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(future);
    });
}

#[cfg(target_arch = "wasm32")]
pub fn spawn_local<F>(future: F)
where
    F: std::future::Future<Output = ()> + 'static,
{
    // On web, give the async task to the JavaScript event loop
    wasm_bindgen_futures::spawn_local(future);
}
