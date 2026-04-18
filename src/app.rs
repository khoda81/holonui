mod backend;

use crate::{
    Allocation,
    app::backend::{BackendEvent, NetworkBackend},
    utils::spawn_local,
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
    api_receiver: Receiver<BackendEvent>, // Receives data from the async task
}

fn get_color_rgba(t: f32) -> egui::Color32 {
    let start_color = Rgba::from(egui::Color32::from_rgb(68, 1, 84)); // Dark purple
    let end_color = Rgba::from(egui::Color32::from_rgb(253, 231, 37)); // Yellow

    // let mixed_rgba = Rgba::from_rgb(
    //     egui::lerp(start_color.l..=end_color.l, t),
    //     egui::lerp(start_color.a..=end_color.a, t),
    //     egui::lerp(start_color.b..=end_color.b, t),
    // );
    let mixed_rgba = egui::lerp(start_color..=end_color, t);

    egui::Color32::from(mixed_rgba)
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
            trajectory: vec![],
            anchor: [0.0, 0.0],
            api_receiver: rx,
        }
    }

    pub fn update(&mut self) {
        // Process all pending messages from the background
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

        // Get the current time
        let now = Utc::now();

        const TIME_UNIT: chrono::TimeDelta = chrono::Duration::days(3);

        // Draw the trajectory as multiple segments to support different colors
        for Trade { delta, timestamp } in self.trajectory.iter() {
            let next_point = [current_point[0] + delta[0], current_point[1] + delta[1]];

            // Calculate how many minutes ago this trade happened
            let elapsed = now - *timestamp;

            // Convert both to f64 seconds and divide
            let delta_t = elapsed.num_milliseconds() as f64 / TIME_UNIT.num_milliseconds() as f64;

            // Weight decays exponentially the older the trade is
            let weight = (-delta_t as f32).exp(); // e^(-t), a value between 0.0 and 1.0

            let color = get_color_rgba(weight);

            // Note: Line::new only takes PlotPoints, you set the name with .name()
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

        // 5. Handle Click to add new point (stored as a relative delta)
        if ui.response().clicked()
            && let Some(PlotPoint { x, y }) = ui.pointer_coordinate()
        {
            // Since trajectory stores relative changes, we calculate the delta
            let delta = [x - self.anchor[0], y - self.anchor[1]];

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
