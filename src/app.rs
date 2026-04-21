mod backend;
mod plot;

use crate::{
    app::{backend::NetworkBackend, plot::ChartState},
    utils::{format_humantime, spawn_local},
};
use chrono::Utc;
use std::sync::mpsc::Receiver;

pub enum BackendEvent {
    Trades(plot::Trajectory),
    Balances(plot::Allocation),
    Orders(plot::OrderFrontier),
}

#[derive(Clone, Copy, Debug, Default)]
enum AppMode {
    #[default]
    Default,
    GhostLine,
}

pub struct HolonApp {
    api_receiver: Receiver<BackendEvent>,
    mode: AppMode,

    chart: plot::ChartState,

    orders: plot::OrderFrontier,
    trajectory: plot::Trajectory,
    anchor: plot::Allocation,

    show_trajectory: bool,
    show_order_frontier: bool,
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
            chart: ChartState::new(),

            orders: plot::OrderFrontier::default(),
            trajectory: Vec::new(),
            anchor: [0.0, 0.0],

            show_trajectory: true,
            show_order_frontier: true,
        }
    }

    pub fn update(&mut self) {
        while let Ok(event) = self.api_receiver.try_recv() {
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
