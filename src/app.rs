mod backend;

use crate::{
    Allocation,
    app::backend::{BackendEvent, NetworkBackend},
    utils::{format_humantime, spawn_local},
};
use chrono::{DateTime, Utc};
use egui_plot::{GridInput, GridMark, HLine, Line, Plot, PlotPoint, PlotPoints, Points, VLine};
use std::sync::mpsc::Receiver;

#[derive(Clone, Copy, Debug, Default)]
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
    trajectory: Vec<Trade>,
    orders: Orders,
    anchor: Allocation,
    mode: AppMode,
    api_receiver: Receiver<BackendEvent>,
    /// Tracks the time unit for the color decay weight
    time_unit_sec: f64,
    log_grid_spacing: bool,
    positive_only_grid: bool,
    show_trajectory: bool,
    show_order_frontier: bool,
}

fn grid_marks(input: GridInput, log_spacing: bool, positive_only: bool) -> Vec<GridMark> {
    let marks = if log_spacing {
        let spacer = egui_plot::log_grid_spacer(10);
        spacer(input)
    } else {
        let spacer = egui_plot::uniform_grid_spacer(|input| {
            let base = input.base_step_size.abs().max(f64::EPSILON);
            let decade = 10_f64.powf(base.log10().floor());
            [decade, decade * 2.0, decade * 10.0]
        });
        spacer(input)
    };

    if positive_only {
        marks.into_iter().filter(|mark| mark.value >= 0.0).collect()
    } else {
        marks
    }
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
            orders: Orders {
                bids: vec![],
                asks: vec![],
            },
            trajectory: vec![],
            anchor: [0.0, 0.0],
            api_receiver: rx,
            time_unit_sec: chrono::Duration::days(2).as_seconds_f64(),
            log_grid_spacing: false,
            positive_only_grid: true,
            show_trajectory: true,
            show_order_frontier: true,
        }
    }

    pub fn update(&mut self) {
        while let Ok(event) = self.api_receiver.try_recv() {
            match event {
                BackendEvent::Orders(orders) => self.orders = orders,
                BackendEvent::Trades { trajectory } => self.trajectory = trajectory,
                BackendEvent::Balances { anchor } => self.anchor = anchor,
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
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.log_grid_spacing, "Logarithmic Grid Spacing");
                ui.checkbox(&mut self.positive_only_grid, "Positive-Only Grid");
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.show_trajectory, "Show Trajectory");
                ui.checkbox(&mut self.show_order_frontier, "Show Order Frontier");
            });

            let scale_plot = !self.mode.is_ghost_line();
            let log_grid_spacing = self.log_grid_spacing;
            let positive_only_grid = self.positive_only_grid;

            Plot::new("skyline_plot")
                .show_grid(true)
                .legend(egui_plot::Legend::default())
                .include_x(0.0)
                .include_y(0.0)
                .x_grid_spacer(move |input| grid_marks(input, log_grid_spacing, positive_only_grid))
                .y_grid_spacer(move |input| grid_marks(input, log_grid_spacing, positive_only_grid))
                .allow_double_click_reset(scale_plot)
                .auto_bounds(scale_plot)
                .show(ui, |ui| self.render_plot_contents(ui));
        });
    }

    fn render_plot_contents(&mut self, ui: &mut egui_plot::PlotUi<'_>) {
        let mut current_point = self.anchor;

        let now = Utc::now();

        // Make the real origin axes obvious.
        ui.hline(
            HLine::new("Y = 0", 0.0)
                .color(egui::Color32::from_rgba_unmultiplied(250, 238, 214, 230))
                .width(2.2),
        );
        ui.vline(
            VLine::new("X = 0", 0.0)
                .color(egui::Color32::from_rgba_unmultiplied(250, 238, 214, 230))
                .width(2.2),
        );

        // Draw trajectory as age-faded segments in one color family.
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
            let freshness = (-delta_t as f32).exp(); // 1.0 = recent, 0.0 = old
            let alpha = (215.0 * freshness) as u8;
            let color = egui::Color32::from_rgba_unmultiplied(108, 156, 217, alpha);

            let next_point = [current_point[0] + delta[0], current_point[1] + delta[1]];
            // TODO: Drawing as separate lines causes weird artifact at the joints where two lines overlap for one pixel
            let segment = Line::new(
                "Trajectory",
                PlotPoints::new(vec![current_point, next_point]),
            )
            .color(color)
            .width(2.0)
            .allow_hover(false)
            .style(egui_plot::LineStyle::Solid);

            // TODO: This causes weird rescaling for the plot when the time unit is adjusted, how do we tell egui to not zoom out for the trajectory lines?
            if self.show_trajectory && alpha > 0 {
                ui.line(segment);
            }

            current_point = next_point;
        }

        ui.points(
            Points::new("Anchor (Current Balance)", vec![self.anchor])
                .radius(6.5)
                .color(egui::Color32::from_rgb(250, 244, 230)),
        );

        if self.show_order_frontier {
            // Buy arm: cumulative chain from anchor.
            let mut buy_point = self.anchor;
            let mut buy_nodes = vec![buy_point];
            for order in &self.orders.bids {
                // TODO: The freshness calculation can probably be refactored out
                let elapsed_s = (now - order.created_at)
                    .max(chrono::Duration::zero())
                    .as_seconds_f64();

                let freshness = (-(elapsed_s / self.time_unit_sec) as f32).exp();
                // TODO: Can we use additive blending to make the redrawn lines visually better? Additive blending that can cause overblown colors and bloom is even better like a laser/light source
                // TODO: Considering the freshness of orders does not matter actually, we may want to visually set their intensity based on distance from market best bid/ask. A rouph estimation of when it may go through
                let alpha = (240.0 * freshness) as u8;
                let color = egui::Color32::from_rgba_unmultiplied(46, 204, 133, alpha);

                let next_point = [buy_point[0] + order.delta[0], buy_point[1] + order.delta[1]];

                if alpha > 0 {
                    ui.line(
                        Line::new("Bid Orders", PlotPoints::new(vec![buy_point, next_point]))
                            .color(color)
                            .width(2.6)
                            .style(egui_plot::LineStyle::Solid),
                    );
                }

                buy_point = next_point;
                buy_nodes.push(next_point);
            }

            if buy_nodes.len() > 1 {
                ui.points(
                    Points::new("Bid Order Nodes", buy_nodes)
                        .radius(3.2)
                        .color(egui::Color32::from_rgba_unmultiplied(92, 230, 164, 170)),
                );
            }

            // Sell arm: cumulative chain from anchor.
            let mut sell_point = self.anchor;
            let mut sell_nodes = vec![sell_point];
            for order in &self.orders.asks {
                let elapsed_s = (now - order.created_at)
                    .max(chrono::Duration::zero())
                    .as_seconds_f64();

                let freshness = (-(elapsed_s / self.time_unit_sec) as f32).exp();
                let alpha = (240.0 * freshness) as u8;
                let color = egui::Color32::from_rgba_unmultiplied(235, 87, 87, alpha);

                let next_point = [
                    sell_point[0] + order.delta[0],
                    sell_point[1] + order.delta[1],
                ];

                if alpha > 0 {
                    ui.line(
                        Line::new("Ask Orders", PlotPoints::new(vec![sell_point, next_point]))
                            .color(color)
                            .width(2.6)
                            .style(egui_plot::LineStyle::Solid),
                    );
                }

                sell_point = next_point;
                sell_nodes.push(next_point);
            }
            if sell_nodes.len() > 1 {
                ui.points(
                    Points::new("Ask Order Nodes", sell_nodes)
                        .radius(3.2)
                        .color(egui::Color32::from_rgba_unmultiplied(255, 130, 130, 180)),
                );
            }
        }

        if let AppMode::Default = self.mode {
            return;
        }

        if let Some(mouse_pos) = ui.pointer_coordinate() {
            let ghost_points = vec![self.anchor, [mouse_pos.x, mouse_pos.y]];

            let ghost_line = Line::new("Ghost Line", PlotPoints::new(ghost_points))
                .color(egui::Color32::from_rgba_unmultiplied(240, 240, 255, 170))
                .width(1.8)
                .style(egui_plot::LineStyle::Dashed { length: 5.0 });

            ui.line(ghost_line);
        }

        if ui.response().clicked()
            && let Some(PlotPoint { x, y }) = ui.pointer_coordinate()
        {
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
