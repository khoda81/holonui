mod backend;
mod plot;

use crate::utils::format_humantime;
use backend::{BackendCommand, BackendEvent, start_backend};
use chrono::Utc;
use plot::ChartState;
use tokio::sync::mpsc::{
    Receiver as TokioReceiver, Sender as TokioSender, channel as tokio_channel, error::TrySendError,
};

#[derive(Clone, Copy, Debug, Default)]
enum AppMode {
    #[default]
    Default,
    GhostLine,
}

pub struct HolonApp {
    backend_commands: TokioSender<BackendCommand>,
    backend_events: TokioReceiver<BackendEvent>,

    mode: AppMode,

    chart: plot::ChartState,

    orders: plot::OrderFrontier,
    trajectory: plot::Trajectory,
    anchor: plot::Allocation,

    show_trajectory: bool,
    show_order_frontier: bool,

    show_api_key_dialog: bool,
    #[cfg(not(target_arch = "wasm32"))]
    api_key_input: String,
}

impl HolonApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        const COMMAND_BUFFER_SIZE: usize = 32;

        let (cmd_tx, cmd_rx) = tokio_channel(COMMAND_BUFFER_SIZE);
        let (event_tx, event_rx) = tokio_channel(COMMAND_BUFFER_SIZE);

        let ctx = cc.egui_ctx.clone();
        // Spawn ONE dedicated OS thread for the entire networking backend
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(move || {
            // Block this thread on the main backend loop

            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(start_backend(ctx, cmd_rx, event_tx));
        });

        // In WASM, we just toss the entire background loop onto the JS event loop.
        // It will yield execution at every `.await`, so it won't freeze the UI.
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(start_backend(ctx, cmd_rx, event_tx));

        if let Err(err) = cmd_tx.blocking_send(BackendCommand::FetchAll) {
            log::error!("Failed to send fetch all command to network backend: {err}")
        }

        Self {
            mode: AppMode::Default,
            chart: ChartState::new(),

            orders: plot::OrderFrontier::default(),
            trajectory: Vec::new(),
            anchor: [0.0, 0.0],

            show_trajectory: true,
            show_order_frontier: true,

            show_api_key_dialog: false,
            #[cfg(not(target_arch = "wasm32"))]
            api_key_input: String::new(),

            backend_events: event_rx,
            backend_commands: cmd_tx,
        }
    }

    pub fn update(&mut self) {
        while let Ok(event) = self.backend_events.try_recv() {
            match event {
                BackendEvent::Orders(orders) => {
                    self.orders = orders;
                    self.chart.update_bounds();
                }

                BackendEvent::Trades(trajectory) => self.trajectory = trajectory,
                BackendEvent::Balances(anchor) => {
                    self.anchor = anchor;
                    self.chart.update_bounds();
                }

                BackendEvent::NeedApiKey => {
                    let _ = self.backend_commands.try_send(BackendCommand::CancelAll);
                    self.show_api_key_dialog = true
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

            let mut time_unit_sec = self.chart.time_unit().as_seconds_f64();
            ui.horizontal(|ui| {
                ui.label("Time Unit:");
                ui.add(
                    egui::Slider::new(&mut time_unit_sec, 1.0..=31_536_000.0) // 1 sec to 1 year
                        .logarithmic(true)
                        .custom_formatter(|sec, _| format_humantime(sec)),
                );
            });
            self.chart
                .set_time_unit(chrono::Duration::nanoseconds((time_unit_sec * 1e9) as i64));

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.chart.log_grid_spacing, "Logarithmic Grid Spacing");
                ui.checkbox(&mut self.chart.positive_only_grid, "Positive-Only Grid");
            });

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.show_trajectory, "Show Trajectory");
                ui.checkbox(&mut self.show_order_frontier, "Show Order Frontier");
            });

            let now = Utc::now();
            let mut context = plot::PlotContext::new(self.mode, now, &self.anchor);
            if self.show_trajectory {
                context.trajectory = &self.trajectory;
            }
            if self.show_order_frontier {
                context.orders = &self.orders;
            }

            let response = self.chart.show(ui, context);

            if let Some(delta) = response.inner.clicked_delta {
                log::info!("Placed order (Delta: {delta:.2?})");
                self.mode = AppMode::Default;
            }
        });
    }

    fn set_backend_token(&self, token: String) -> Result<(), TrySendError<BackendCommand>> {
        self.backend_commands
            .try_send(BackendCommand::UpdateApiKey(token))?;

        self.backend_commands.try_send(BackendCommand::FetchAll)?;

        Ok(())
    }

    fn show_api_key_dialog(
        &mut self,
        ui: &mut egui::Ui,
    ) -> Option<egui::InnerResponse<Option<()>>> {
        fn save_api_key(key: &str) -> Result<(), keyring::Error> {
            keyring::Entry::new("holonui", "default-user")?.set_password(key)
        }

        egui::Window::new("API Key")
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label("Enter your Nobitex API token:");
                ui.text_edit_singleline(&mut self.api_key_input);

                if ui.button("Save").clicked() {
                    if let Err(err) = save_api_key(&self.api_key_input) {
                        log::error!("Failed to save api_key: {err}")
                    }

                    if let Err(err) = self.set_backend_token(self.api_key_input.clone()) {
                        log::error!("Failed to send update api key command to backend: {err}")
                    }

                    self.show_api_key_dialog = false;
                }
            })
    }
}

impl eframe::App for HolonApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.update();

        #[cfg(not(target_arch = "wasm32"))]
        if self.show_api_key_dialog {
            self.show_api_key_dialog(ui);
        }

        // TODO: We should probably schedule a redraw based on self.time_unit_sec because for low enough
        // time_unit_sec values the color can change each frame and since egui is lazy it will only update
        //  on the next event which makes the color jump instead of changing smoothly as it should
        self.handle_keyboard(ui);
        self.render_main_plot(ui);
    }
}
