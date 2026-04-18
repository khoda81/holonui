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

struct NetworkBackend {
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
        // 1. Fetch Wallets and Trades concurrently using runtime-agnostic join!
        let (wallets_json, trades_json) = futures::join!(
            self.fetch_endpoint("https://apiv2.nobitex.ir/users/wallets/list"),
            self.fetch_endpoint(
                "https://apiv2.nobitex.ir/market/trades/list?srcCurrency=usdt&dstCurrency=rls"
            )
        );

        // 2. Extract Anchor (Current Balances)
        let mut current_usdt = 0.0;
        let mut current_rls = 0.0;

        if let Some(wallets) = wallets_json.get("wallets").and_then(|w| w.as_array()) {
            for wallet in wallets {
                let currency = wallet
                    .get("currency")
                    .and_then(|c| c.as_str())
                    .unwrap_or("");
                let balance_str = wallet
                    .get("balance")
                    .and_then(|b| b.as_str())
                    .unwrap_or("0");
                let balance: f64 = balance_str.parse().unwrap_or(0.0);

                if currency == "usdt" {
                    current_usdt = balance;
                } else if currency == "rls" {
                    current_rls = balance;
                }
            }
        }

        let anchor = [current_usdt, current_rls];
        let _ = self.sender.send(BackendEvent::FetchedBalances { anchor });
        self.ctx.request_repaint(); // Wake UI to show anchor immediately

        // 3. Reconstruct Trajectory Backwards
        let mut trajectory = vec![anchor];
        let mut sim_usdt = current_usdt;
        let mut sim_rls = current_rls;

        if let Some(trades) = trades_json.get("trades").and_then(|t| t.as_array()) {
            // Assuming trades are returned newest first. If they are oldest first,
            // you'll need to .rev() the iterator here.
            for trade in trades {
                let t_type = trade.get("type").and_then(|t| t.as_str()).unwrap_or("");

                let amount: f64 = trade
                    .get("amount")
                    .and_then(|a| a.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);

                let total: f64 = trade
                    .get("total")
                    .and_then(|t| t.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.0);

                // Undo the trade to find the balance BEFORE this trade happened
                if t_type == "buy" {
                    sim_usdt -= amount;
                    sim_rls += total;
                } else if t_type == "sell" {
                    sim_usdt += amount;
                    sim_rls -= total;
                }

                trajectory.push([sim_usdt, sim_rls]);
            }
        }

        // Reverse to make it chronological (oldest to newest) for plotting
        trajectory.reverse();

        let _ = self.sender.send(BackendEvent::FetchedTrades { trajectory });
        self.ctx.request_repaint(); // Wake UI to draw the plot
    }

    async fn fetch_endpoint(&self, url: &str) -> Value {
        let response = self
            .client
            .get(url)
            .header("Authorization", format!("Token {}", self.api_key))
            .send()
            .await;

        if let Ok(resp) = response
            && let Ok(json) = resp.json::<Value>().await
        {
            return json;
        }

        // Return an empty JSON object as a fallback if anything fails
        serde_json::json!(null)
    }
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
