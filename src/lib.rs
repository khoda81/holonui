use eframe::egui;
use egui_plot::{Line, Plot, PlotPoint, PlotPoints};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use std::env;
use std::sync::mpsc::{Receiver, Sender};
#[cfg(not(target_arch = "wasm32"))]
use std::thread;

pub type Allocation = [f64; 2];

pub enum BackendEvent {
    SyncStarted,
    FetchedTrades { trajectory: Vec<Allocation> },
    FetchedBalances { anchor: Allocation },
    RequestError(reqwest::Error),
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
            let network_backend = NetworkBackend::new(tx, ctx_clone);

            network_backend.fetch_nobitex_data().await
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
                BackendEvent::FetchedBalances { anchor } => {
                    log::info!("Received anchor: {anchor:?}");
                    self.anchor = anchor
                }
                BackendEvent::FetchedTrades { trajectory } => {
                    log::info!("Received trades: {trajectory:?}");
                    self.trajectory = trajectory
                }
                // TODO:
                BackendEvent::RequestError(err) => {
                    log::error!("Request Error: {err}")
                }
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
        // 1. Calculate absolute trajectory relative to the anchor
        let mut absolute_points = vec![self.anchor];
        let mut current_point = self.anchor;

        for delta in &self.trajectory {
            current_point[0] += delta[0];
            current_point[1] += delta[1];
            absolute_points.push(current_point);
        }

        ui.line(Line::new("Intent Frontier", absolute_points.clone()));

        ui.points(
            egui_plot::Points::new("Anchor (Current Balance)", vec![self.anchor])
                .radius(5.0)
                .color(egui::Color32::RED),
        );

        if let AppMode::Default = self.mode {
            return;
        }

        if let Some(mouse_pos) = ui.pointer_coordinate() {
            let ghost_points = vec![self.anchor, [mouse_pos.x, mouse_pos.y]];

            let ghost_line = Line::new("Ghost Line", PlotPoints::new(ghost_points))
                .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 150))
                .style(egui_plot::LineStyle::Dashed { length: 5.0 });

            ui.line(ghost_line);
        }

        // 5. Handle Click to add new point (stored as a relative delta)
        if ui.response().clicked()
            && let Some(PlotPoint { x, y }) = ui.pointer_coordinate()
        {
            // Since trajectory stores relative changes, we calculate the delta
            let delta = [x - self.anchor[0], y - self.anchor[1]];
            self.trajectory.push(delta);

            log::info!("Clicked plot at X: {x}, Y: {y} (Delta: {delta:?})");
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

// Helper function to deserialize stringified numbers into f64
fn f64_from_str<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse().map_err(serde::de::Error::custom)
}

#[derive(Deserialize, Debug)]
struct TradesResponse {
    trades: Vec<Trade>,
}

#[derive(Deserialize, Debug)]
struct Trade {
    #[serde(rename = "type")]
    trade_type: TradeType,
    #[serde(deserialize_with = "f64_from_str")]
    amount: f64,
    #[serde(deserialize_with = "f64_from_str")]
    total: f64,
    #[serde(deserialize_with = "f64_from_str")]
    fee: f64,
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
enum TradeType {
    Buy,
    Sell,
}

#[derive(Deserialize, Debug)]
struct WalletsResponse {
    wallets: Vec<Wallet>,
}

#[derive(Deserialize, Debug)]
struct Wallet {
    currency: String,
    #[serde(deserialize_with = "f64_from_str")]
    balance: f64,
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
        futures::join!(self.fetch_balances(), self.fetch_trajectory());
    }

    async fn fetch_trajectory(&self) {
        let trades_response: TradesResponse = self
            .fetch_endpoint(
                "https://apiv2.nobitex.ir/market/trades/list?srcCurrency=usdt&dstCurrency=rls",
            )
            .await;

        log::trace!("{trades_response:?}");

        let trajectory = trades_response
            .trades
            .into_iter()
            .map(|trade| {
                // Undo the trade to find the balance BEFORE this trade happened
                match trade.trade_type {
                    // You received $(amount - fee)$ in USDT, and paid $total$ in RLS.
                    // To undo: Subtract the net USDT gained, add the RLS paid.
                    TradeType::Buy => [-(trade.amount - trade.fee), trade.total],
                    // You paid $amount$ in USDT, and received $(total - fee)$ in RLS.
                    // To undo: Add the USDT paid, subtract the net RLS gained.
                    TradeType::Sell => [trade.amount, -(trade.total - trade.fee)],
                }
            })
            .collect();

        self.send_event(BackendEvent::FetchedTrades { trajectory });
    }

    fn send_event(&self, event: BackendEvent) {
        let _ = self.sender.send(event);
        self.ctx.request_repaint()
    }

    async fn fetch_balances(&self) {
        // Automatically fetches and parses into WalletsResponse
        let response: WalletsResponse = self
            .fetch_endpoint("https://apiv2.nobitex.ir/users/wallets/list")
            .await;

        let mut current_usdt = 0.0;
        let mut current_rls = 0.0;

        for wallet in response.wallets {
            if wallet.currency == "usdt" {
                current_usdt = wallet.balance;
            } else if wallet.currency == "rls" {
                current_rls = wallet.balance;
            }
        }

        self.send_event(BackendEvent::FetchedBalances {
            anchor: [current_usdt, current_rls],
        });
    }

    async fn fetch_endpoint<T: DeserializeOwned>(&self, url: &str) -> T {
        loop {
            // Group the network request and JSON parsing into a single Result-returning async block
            let result = async {
                self.client
                    .get(url)
                    .header("Authorization", format!("Token {}", self.api_key))
                    .send()
                    .await?
                    .json::<T>()
                    .await
            };

            match result.await {
                Ok(parsed) => break parsed,
                Err(err) => self.send_event(BackendEvent::RequestError(err)),
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_local<F>(future: F) -> thread::JoinHandle<F::Output>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    // On native, spin up a background thread and a lightweight Tokio runtime
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    })
}

#[cfg(target_arch = "wasm32")]
pub fn spawn_local<F>(future: F)
where
    F: std::future::Future<Output = ()> + 'static,
{
    // On web, give the async task to the JavaScript event loop
    wasm_bindgen_futures::spawn_local(future)
}
