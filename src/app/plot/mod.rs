use chrono::{DateTime, Utc};
use egui_plot::{
    GridInput, GridMark, HLine, Line, Plot, PlotBounds, PlotPoint, PlotPoints, PlotResponse,
    Points, VLine,
};

use crate::app::backend::models::Trade;

use super::{
    AppMode,
    backend::models::{Order, TradeType},
};

pub type Allocation = [f64; 2];

#[derive(Clone, Debug)]
pub struct TradeSegment {
    delta: [f64; 2],
    timestamp: DateTime<Utc>,
}

pub type Trajectory = Vec<TradeSegment>;

impl From<Trade> for TradeSegment {
    fn from(value: Trade) -> Self {
        let delta = match value.trade_type {
            TradeType::Buy => [-(value.amount - value.fee), value.total],
            TradeType::Sell => [value.amount, -(value.total - value.fee)],
        };

        Self {
            delta,
            timestamp: value.timestamp,
        }
    }
}

#[derive(Clone, Debug)]
pub struct OrderSegment {
    delta: [f64; 2],
    created_at: DateTime<Utc>,
}

impl From<Order> for OrderSegment {
    fn from(value: Order) -> Self {
        // TODO: Apply a fee to the price, these prices are not the actual executed price
        let total = value.unmatched_amount * value.price;

        let delta = match value.order_type {
            TradeType::Buy => [value.unmatched_amount, -total],
            TradeType::Sell => [-value.unmatched_amount, total],
        };

        Self {
            delta,
            created_at: value.created_at,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct OrderFrontier {
    bids: Vec<OrderSegment>,
    asks: Vec<OrderSegment>,
}

impl OrderFrontier {
    const EMPTY: Self = Self {
        bids: Vec::new(),
        asks: Vec::new(),
    };

    pub fn from_sides(
        bids: impl IntoIterator<Item = Order>,
        asks: impl IntoIterator<Item = Order>,
    ) -> Self {
        Self {
            bids: bids.into_iter().map(Into::into).collect(),
            asks: asks.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PlotContext<'f> {
    pub mode: AppMode,
    pub now: DateTime<Utc>,
    pub orders: &'f OrderFrontier,
    pub trajectory: &'f [TradeSegment],
    pub anchor: &'f Allocation,
}

impl<'f> PlotContext<'f> {
    pub fn new(mode: AppMode, now: DateTime<Utc>, anchor: &'f Allocation) -> Self {
        Self {
            mode,
            now,
            orders: const { &OrderFrontier::EMPTY },
            trajectory: &[],
            anchor,
        }
    }

    pub fn orders(mut self, orders: &'f OrderFrontier) -> Self {
        self.orders = orders;
        self
    }

    pub fn trajectory(mut self, trajectory: &'f [TradeSegment]) -> Self {
        self.trajectory = trajectory;
        self
    }
}

impl PlotContext<'_> {
    fn order_frontier_bounds(&self) -> PlotBounds {
        let mut bounds = PlotBounds::NOTHING;

        bounds.extend_with(&PlotPoint::from(*self.anchor));
        bounds.extend_with(&PlotPoint::new(0.0, 0.0));

        let mut buy_point = *self.anchor;
        for order in &self.orders.bids {
            buy_point = [buy_point[0] + order.delta[0], buy_point[1] + order.delta[1]];
            bounds.extend_with(&PlotPoint::from(buy_point));
        }

        let mut sell_point = *self.anchor;
        for order in &self.orders.asks {
            sell_point = [
                sell_point[0] + order.delta[0],
                sell_point[1] + order.delta[1],
            ];
            bounds.extend_with(&PlotPoint::from(sell_point));
        }

        bounds.expand_x(bounds.width() * 0.06);
        bounds.expand_y(bounds.height() * 0.06);

        bounds
    }
}

pub struct ChartState {
    /// Tracks the time unit for the color decay weight
    time_unit: chrono::Duration,
    pub log_grid_spacing: bool,
    pub positive_only_grid: bool,
    pub bounds_need_update: bool,
}

fn grid_marks(input: GridInput, positive_only: bool) -> Vec<GridMark> {
    // TODO: How does grid spacer work in egui?
    let spacer = egui_plot::log_grid_spacer(10);

    let mut marks = spacer(input);

    if positive_only {
        marks.retain_mut(|mark| mark.value >= 0.0)
    }

    marks
}

pub struct PlotPanelResponse {
    pub clicked_delta: Option<[f64; 2]>,
}

impl ChartState {
    pub fn new() -> Self {
        Self {
            time_unit: chrono::Duration::days(2),
            log_grid_spacing: false,
            positive_only_grid: true,
            bounds_need_update: false,
        }
    }

    pub fn update_bounds(&mut self) {
        self.bounds_need_update = true
    }

    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        ctx: PlotContext<'_>,
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
                    let bounds = ctx.order_frontier_bounds();
                    self.bounds_need_update = false;

                    ui.set_plot_bounds_x(bounds.range_x());
                    ui.set_plot_bounds_y(bounds.range_y());
                }

                self.render_plot_contents(ui, ctx)
            })
    }

    fn render_plot_contents(
        &self,
        ui: &mut egui_plot::PlotUi<'_>,
        ctx: PlotContext<'_>,
    ) -> PlotPanelResponse {
        // TODO: Draw the budget line passing through anchor, highlighted point

        // TODO: Can we label the axis with the currency name?
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

        let mut current_point = *ctx.anchor;
        // Draw trajectory as age-faded segments in one color family.
        for TradeSegment { delta, timestamp } in ctx.trajectory.iter() {
            // TODO: The freshness calculation can probably be refactored out
            let elapsed = ctx.now - *timestamp;
            let mut elapsed_s = elapsed.as_seconds_f64();

            // Ensure dt is never negative
            if elapsed_s < 0.0 {
                log::warn!("Unexpected negative elapsed time for a trade");
                elapsed_s = 0.0;
            }

            // Calculate ratio and the exponential decay weight
            let delta_t = elapsed_s / self.time_unit.as_seconds_f64();
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
            if alpha > 0 {
                ui.line(segment);
            }

            current_point = next_point;
        }

        // Buy arm: cumulative chain from anchor.
        let mut buy_point = *ctx.anchor;
        let mut buy_nodes = vec![buy_point];
        for order in &ctx.orders.bids {
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
        let mut sell_point = *ctx.anchor;
        let mut sell_nodes = vec![sell_point];
        for order in &ctx.orders.asks {
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

        ui.points(
            Points::new("Anchor (Current Balance)", vec![*ctx.anchor])
                .radius(6.5)
                .color(egui::Color32::from_rgb(250, 244, 230)),
        );

        let mut response = PlotPanelResponse {
            clicked_delta: None,
        };

        if let AppMode::Default = ctx.mode {
            return response;
        }

        if let Some(mouse_pos) = ui.pointer_coordinate() {
            let ghost_points = vec![*ctx.anchor, [mouse_pos.x, mouse_pos.y]];

            let ghost_line = Line::new("Ghost Line", PlotPoints::new(ghost_points))
                .color(egui::Color32::from_rgba_unmultiplied(240, 240, 255, 170))
                .width(1.8)
                .style(egui_plot::LineStyle::Dashed { length: 5.0 });

            ui.line(ghost_line);
        }

        if ui.response().clicked()
            && let Some(PlotPoint { x, y }) = ui.pointer_coordinate()
        {
            response.clicked_delta = Some([x - ctx.anchor[0], y - ctx.anchor[1]]);
        }

        response
    }

    pub fn set_time_unit(&mut self, time_unit: chrono::Duration) {
        self.time_unit = time_unit;
    }

    pub fn time_unit(&self) -> chrono::TimeDelta {
        self.time_unit
    }
}
