mod backend;

use crate::{
    Allocation,
    app::backend::{BackendEvent, NetworkBackend},
    utils::spawn_local,
};
use egui_plot::{Line, Plot, PlotPoint, PlotPoints};
use std::sync::mpsc::Receiver;

#[derive(Debug, Default)]
pub(crate) enum AppMode {
    #[default]
    Default,
    GhostLine,
}

impl AppMode {
    /// Returns `true` if the app state is [`GhostLine`].
    ///
    /// [`GhostLine`]: AppMode::GhostLine
    #[must_use]
    pub(crate) fn is_ghost_line(&self) -> bool {
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

    pub(crate) fn handle_keyboard(&mut self, ui: &mut egui::Ui) {
        if ui.input(|i| i.key_pressed(egui::Key::O)) {
            self.mode = match self.mode {
                AppMode::Default => AppMode::GhostLine,
                AppMode::GhostLine => AppMode::Default,
            };
        }
    }

    pub(crate) fn render_main_plot(&mut self, ui: &mut egui::Ui) {
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
    pub(crate) fn render_plot_contents(&mut self, ui: &mut egui_plot::PlotUi<'_>) {
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
