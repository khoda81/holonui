mod backend;
mod plot;

use crate::{
    Allocation,
    app::{
        backend::{BackendEvent, NetworkBackend},
        plot::PlotPanel,
    },
    utils::{format_humantime, spawn_local},
};
use chrono::{DateTime, Utc};
use std::sync::mpsc::Receiver;

#[derive(Clone, Copy, Debug, Default)]
enum AppMode {
    #[default]
    Default,
    GhostLine,
}

#[derive(Clone, Debug)]
struct Trade {
    delta: [f64; 2],
    timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug)]
struct Order {
    delta: [f64; 2],
    created_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
struct Orders {
    bids: Vec<Order>,
    asks: Vec<Order>,
}

pub struct HolonApp {
    api_receiver: Receiver<BackendEvent>,
    mode: AppMode,

    plot_panel: plot::PlotPanel,
}

impl HolonApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx_clone = cc.egui_ctx.clone();

        let network_backend = NetworkBackend::new(tx, ctx_clone);

        spawn_local(async move {
            futures::join!(
                network_backend.fetch_balances(),
                network_backend.fetch_trajectory(),
                network_backend.fetch_orders(),
            );
        });

        Self {
            mode: AppMode::Default,
            api_receiver: rx,
            plot_panel: PlotPanel::new(),
        }
    }

    pub fn update(&mut self) {
        while let Ok(event) = self.api_receiver.try_recv() {
            match event {
                BackendEvent::Orders(orders) => self.plot_panel.set_orders(orders),
                BackendEvent::Trades { trajectory } => self.plot_panel.set_trajectory(trajectory),
                BackendEvent::Balances { anchor } => self.plot_panel.set_anchor(anchor),
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

            ui.horizontal(|ui| {
                ui.label("Time Unit:");
                ui.add(
                    egui::Slider::new(&mut self.plot_panel.time_unit_sec, 1.0..=31_536_000.0) // 1 sec to 1 year
                        .logarithmic(true)
                        .custom_formatter(|sec, _| format_humantime(sec)),
                );
            });

            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut self.plot_panel.log_grid_spacing,
                    "Logarithmic Grid Spacing",
                );
                ui.checkbox(
                    &mut self.plot_panel.positive_only_grid,
                    "Positive-Only Grid",
                );
            });

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.plot_panel.show_trajectory, "Show Trajectory");
                ui.checkbox(
                    &mut self.plot_panel.show_order_frontier,
                    "Show Order Frontier",
                );
            });

            let now = Utc::now();
            let response = self.plot_panel.show(ui, self.mode, now);

            if let Some(delta) = response.inner.clicked_delta {
                log::info!("Placed order (Delta: {delta:.2?})");
                self.mode = AppMode::Default;
            }
        });
    }
}

impl eframe::App for HolonApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.update();
        // TODO: We should probably schedule a redraw based on self.time_unit_sec because for low enough
        // time_unit_sec values the color can change each frame and since egui is lazy it will only update
        //  on the next event which makes the color jump instead of changing smoothly as it should
        self.handle_keyboard(ui);
        self.render_main_plot(ui);
    }
}
