use chrono::{DateTime, Utc};
use egui_plot::{
    GridInput, GridMark, HLine, Line, Plot, PlotBounds, PlotPoint, PlotPoints, PlotResponse,
    Points, VLine,
};

use super::{Orders, Trade};
use crate::{Allocation, app::AppMode};

pub struct PlotPanel {
    pub trajectory: Vec<Trade>,
    pub orders: Orders,
    pub anchor: Allocation,
    /// Tracks the time unit for the color decay weight
    pub time_unit_sec: f64,
    pub log_grid_spacing: bool,
    pub positive_only_grid: bool,
    pub show_trajectory: bool,
    pub show_order_frontier: bool,
    pub bounds_need_update: bool,
}

fn grid_marks(input: GridInput, positive_only: bool) -> Vec<GridMark> {
    // TODO: How does grid spacer work in egui?
    let spacer = egui_plot::log_grid_spacer(10);

    let marks = spacer(input);

    if positive_only {
        marks.into_iter().filter(|mark| mark.value >= 0.0).collect()
    } else {
        marks
    }
}

pub struct PlotPanelResponse {
    pub clicked_delta: Option<[f64; 2]>,
}

impl PlotPanel {
    pub fn new() -> Self {
        Self {
            orders: Orders {
                bids: vec![],
                asks: vec![],
            },
            trajectory: vec![],
            anchor: [0.0, 0.0],
            time_unit_sec: chrono::Duration::days(2).as_seconds_f64(),
            log_grid_spacing: false,
            positive_only_grid: true,
            show_trajectory: true,
            show_order_frontier: true,
            bounds_need_update: false,
        }
    }

    fn order_frontier_bounds(&self) -> PlotBounds {
        let mut bounds = PlotBounds::NOTHING;
        bounds.extend_with(&PlotPoint::from(self.anchor));
        bounds.extend_with(&PlotPoint::new(0.0, 0.0));

        let mut buy_point = self.anchor;
        for order in &self.orders.bids {
            buy_point = [buy_point[0] + order.delta[0], buy_point[1] + order.delta[1]];
            bounds.extend_with(&PlotPoint::from(buy_point));
        }

        let mut sell_point = self.anchor;
        for order in &self.orders.asks {
            sell_point = [
                sell_point[0] + order.delta[0],
                sell_point[1] + order.delta[1],
            ];
            bounds.extend_with(&PlotPoint::from(sell_point));
        }

        let min_width = 1e-9;
        let min_height = 1e-9;
        if bounds.width() <= min_width {
            bounds.expand_x(min_width);
        }
        if bounds.height() <= min_height {
            bounds.expand_y(min_height);
        }

        let x_pad = (bounds.width() * 0.06).max(min_width);
        let y_pad = (bounds.height() * 0.06).max(min_height);
        bounds.expand_x(x_pad);
        bounds.expand_y(y_pad);
        bounds
    }

    pub fn set_orders(&mut self, orders: Orders) {
        self.bounds_need_update = true;
        self.orders = orders;
    }

    pub fn set_trajectory(&mut self, trajectory: Vec<Trade>) {
        self.trajectory = trajectory;
    }

    pub fn set_anchor(&mut self, anchor: Allocation) {
        self.bounds_need_update = true;
        self.anchor = anchor;
    }

    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        app_mode: AppMode,
        now: DateTime<Utc>,
    ) -> PlotResponse<PlotPanelResponse> {
        let positive_only_grid = self.positive_only_grid;

        Plot::new("skyline_plot")
            .show_grid(true)
            .legend(egui_plot::Legend::default())
            .auto_bounds(false)
            .allow_double_click_reset(false)
            .x_grid_spacer(move |input| grid_marks(input, positive_only_grid))
            .y_grid_spacer(move |input| grid_marks(input, positive_only_grid))
            .show(ui, |ui| {
                if ui.response().double_clicked() || self.bounds_need_update {
                    let bounds = self.order_frontier_bounds();
                    self.bounds_need_update = false;

                    ui.set_plot_bounds_x(bounds.range_x());
                    ui.set_plot_bounds_y(bounds.range_y());
                }

                self.render_plot_contents(ui, app_mode, now)
            })
    }

    fn render_plot_contents(
        &self,
        ui: &mut egui_plot::PlotUi<'_>,
        app_mode: AppMode,
        now: DateTime<Utc>,
    ) -> PlotPanelResponse {
        // TODO: Draw the budget line passing through anchor, highlighted point

        let mut current_point = self.anchor;

        // TODO: How do we hide the legend for these?
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

        // TODO: Can we label the axis with the currency name?

        // Draw trajectory as age-faded segments in one color family.
        for Trade { delta, timestamp } in self.trajectory.iter() {
            // TODO: The freshness calculation can probably be refactored out
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
            // TODO: Can we use additive blending to make the redrawn lines visually better? Additive blending that can cause overblown colors and bloom is even better like a laser/light source
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

        if self.show_order_frontier {
            // Buy arm: cumulative chain from anchor.
            let mut buy_point = self.anchor;
            let mut buy_nodes = vec![buy_point];
            for order in &self.orders.bids {
                // TODO: Considering the freshness of orders does not matter actually, we may want to visually set their intensity based on distance from market best bid/ask. An estimation of when it may go through
                let color = egui::Color32::from_rgba_unmultiplied(46, 204, 133, 240);

                let next_point = [buy_point[0] + order.delta[0], buy_point[1] + order.delta[1]];

                ui.line(
                    Line::new("Bid Orders", PlotPoints::new(vec![buy_point, next_point]))
                        .color(color)
                        .width(2.6)
                        .style(egui_plot::LineStyle::Solid),
                );

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
                let color = egui::Color32::from_rgba_unmultiplied(235, 87, 87, 240);

                let next_point = [
                    sell_point[0] + order.delta[0],
                    sell_point[1] + order.delta[1],
                ];

                ui.line(
                    Line::new("Ask Orders", PlotPoints::new(vec![sell_point, next_point]))
                        .color(color)
                        .width(2.6)
                        .style(egui_plot::LineStyle::Solid),
                );

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

        ui.points(
            Points::new("Anchor (Current Balance)", vec![self.anchor])
                .radius(6.5)
                .color(egui::Color32::from_rgb(250, 244, 230)),
        );

        let mut response = PlotPanelResponse {
            clicked_delta: None,
        };

        if let AppMode::Default = app_mode {
            return response;
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
            response.clicked_delta = Some([x - self.anchor[0], y - self.anchor[1]]);
        }

        response
    }
}
