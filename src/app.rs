mod backend;

use crate::{
    Allocation,
    app::backend::{BackendEvent, NetworkBackend},
    utils::{format_humantime, spawn_local},
};
use chrono::{DateTime, Utc};
use egui::ecolor::Rgba;
use egui_plot::{Line, Plot, PlotPoint, PlotPoints};
use std::sync::mpsc::Receiver;

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

struct Trade {
    delta: [f64; 2],
    timestamp: DateTime<Utc>,
}

pub struct HolonApp {
    trajectory: Vec<Trade>,
    anchor: Allocation,
    mode: AppMode,
    api_receiver: Receiver<BackendEvent>,
    /// Tracks the time unit for the color decay weight
    time_unit_sec: f64,
}

fn get_color_rgba(t: f32) -> egui::Color32 {
    let start_color = Rgba::from(egui::Color32::from_rgb(68, 1, 84)); // Dark purple
    let end_color = Rgba::from(egui::Color32::from_rgb(253, 231, 37)); // Yellow

    let mixed_rgba = egui::lerp(start_color..=end_color, t);

    egui::Color32::from(mixed_rgba)
}

impl HolonApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx_clone = cc.egui_ctx.clone();

        let network_backend = NetworkBackend::new(tx, ctx_clone);

        spawn_local(async move {
            futures::join!(
                network_backend.fetch_balances(),
                network_backend.fetch_trajectory()
            );
        });

        Self {
            mode: AppMode::Default,
            trajectory: vec![],
            anchor: [0.0, 0.0],
            api_receiver: rx,
            time_unit_sec: chrono::Duration::days(2).as_seconds_f64(),
        }
    }

    pub fn update(&mut self) {
        while let Ok(event) = self.api_receiver.try_recv() {
            match event {
                BackendEvent::FetchedBalances { anchor } => self.anchor = anchor,
                BackendEvent::FetchedTrades { trajectory } => self.trajectory = trajectory,
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

            ui.horizontal(|ui| {
                ui.label("Time Unit:");
                ui.add(
                    egui::Slider::new(&mut self.time_unit_sec, 1.0..=31_536_000.0) // 1 sec to 1 year
                        .logarithmic(true)
                        .custom_formatter(|sec, _| format_humantime(sec)),
                );
            });
            let scale_plot = !self.mode.is_ghost_line();

            let plot = Plot::new("skyline_plot")
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
        let mut absolute_points = vec![self.anchor];
        let mut current_point = self.anchor;

        let now = Utc::now();

        // Convert the time unit into milliseconds for calculation
        for Trade { delta, timestamp } in self.trajectory.iter() {
            let elapsed = now - *timestamp;
            let mut elapsed_s = elapsed.as_seconds_f64();

            // Ensure dt is never negative
            if elapsed_s < 0.0 {
                log::warn!("Unexpected negative elapsed time for a trade");
                elapsed_s = 0.0;
            }

            // Calculate ratio and the exponential decay weight
            let delta_t = elapsed_s / self.time_unit_sec;
            let weight = (-delta_t as f32).exp(); // e^(-t), a value between 0.0 and 1.0

            let color = get_color_rgba(weight);

            let next_point = [current_point[0] + delta[0], current_point[1] + delta[1]];
            let segment = Line::new(
                "Intent Frontier Segment",
                PlotPoints::new(vec![current_point, next_point]),
            )
            .color(color);

            ui.line(segment);

            current_point = next_point;
            absolute_points.push(current_point);
        }

        let anchor_color = get_color_rgba(1.0);

        ui.points(
            egui_plot::Points::new("Anchor (Current Balance)", vec![self.anchor])
                .radius(5.0)
                .color(anchor_color),
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

        if ui.response().clicked()
            && let Some(PlotPoint { x, y }) = ui.pointer_coordinate()
        {
            let delta = [x - self.anchor[0], y - self.anchor[1]];

            // We must now push a full Trade object
            self.trajectory.push(Trade {
                delta,
                timestamp: Utc::now(),
            });

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
