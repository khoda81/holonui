mod price_allocation;

use std::f64::consts::LN_10;

use chrono::{DateTime, Utc};
use egui::{CentralPanel, Panel, Rangef};
use egui_plot::{
    AxisHints, GridInput, GridMark, HLine, Line, Plot, PlotBounds, PlotPoint, PlotPoints,
    PlotResponse, Points, VLine,
};

use crate::{
    app::plot::price_allocation::generate_price_allocation_line,
    utils::{format_human_num, log_ln_grid_spacer},
};

use super::{
    AppMode,
    backend::models::{Order, Trade, TradeType},
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
    pub axis_names: (&'f str, &'f str),
}

impl<'f> PlotContext<'f> {
    pub fn new(
        mode: AppMode,
        now: DateTime<Utc>,
        anchor: &'f Allocation,
        axis_names: (&'f str, &'f str),
    ) -> Self {
        Self {
            mode,
            now,
            orders: const { &OrderFrontier::EMPTY },
            trajectory: &[],
            anchor,
            axis_names,
        }
    }
}

impl PlotContext<'_> {
    // TODO: In logarithmic coordinate, the min order coordinates + some padding should be the low x and low y bounds
    fn order_frontier_bounds(&self, log_grid_spacing: bool) -> PlotBounds {
        let mut bounds = PlotBounds::NOTHING;

        bounds.extend_with(&PlotPoint::from(*self.anchor));

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

        let mut min = bounds.min();
        let mut max = bounds.max();

        if log_grid_spacing {
            min.to_logspace();
            max.to_logspace();
            bounds = PlotBounds::from_min_max(min, max);
            log::trace!("Updated Min: {min:?}, Max: {max:?}");

            bounds.expand_x(bounds.width() * 0.06);
            bounds.expand_y(bounds.height() * 0.06);
        } else {
            if self.orders.bids.is_empty() && self.orders.asks.is_empty() {
                let [x, y] = self.anchor;
                bounds.extend_with(&PlotPoint::new(x * 2.0, y * 2.0));
            } else {
                bounds.expand_x(bounds.width() * 0.06);
                bounds.expand_y(bounds.height() * 0.06);
            }

            bounds.extend_with(&PlotPoint::new(0.0, 0.0));
        }

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

fn grid_marks(
    positive_only: bool,
    log_grid_spacing: bool,
) -> Box<dyn Fn(GridInput) -> Vec<GridMark> + 'static> {
    if log_grid_spacing {
        Box::new(|input| log_ln_grid_spacer(input, LN_10))
    } else if positive_only {
        Box::new(|mut input| {
            input.bounds.0 = input.bounds.0.max(0.0);
            egui_plot::log_grid_spacer(10)(input)
        })
    } else {
        egui_plot::log_grid_spacer(10)
    }
}

fn axis_formatter(log_grid_spacing: bool, mark: GridMark) -> String {
    if log_grid_spacing {
        // TODO: We should pass the numbers in logspace to the formatter for better precision?
        format_human_num((mark.value * LN_10).exp(), (mark.value * LN_10).exp())
    } else {
        format_human_num(mark.value, mark.step_size)
    }
}

trait ToLogSpace {
    fn to_logspace(&mut self);
}

impl<const N: usize> ToLogSpace for Vec<[f64; N]> {
    fn to_logspace(&mut self) {
        self.iter_mut().for_each(ToLogSpace::to_logspace);
    }
}

impl<const N: usize> ToLogSpace for [f64; N] {
    fn to_logspace(&mut self) {
        *self = self.map(f64::log10);
    }
}

pub struct PlotPanelResponse {
    pub clicked_delta: Option<[f64; 2]>,
}

impl ChartState {
    const ANCHOR_COLOR: egui::Color32 = egui::Color32::from_rgb(250, 244, 230);

    pub fn new() -> Self {
        Self {
            time_unit: chrono::Duration::days(2),
            log_grid_spacing: true,
            positive_only_grid: true,
            bounds_need_update: false,
        }
    }

    pub fn update_bounds(&mut self) {
        self.bounds_need_update = true
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        ctx: PlotContext<'_>,
    ) -> PlotResponse<PlotPanelResponse> {
        Panel::right("price_allocation").show_inside(ui, |ui| {
            Plot::new("price allocation plot")
                .custom_y_axes(vec![AxisHints::new_y().min_thickness(48.0)])
                .default_x_bounds(0.0, 1.0000001)
                .allow_zoom([false, true])
                .allow_scroll([false, true])
                .allow_drag([false, true])
                .allow_axis_zoom_drag([false, true])
                .y_grid_spacer(grid_marks(true, true))
                .y_axis_formatter(|m, _| axis_formatter(true, m))
                .show(ui, |ui| {
                    let [x, y] = *ctx.anchor;
                    let points = PlotPoints::from_explicit_callback(
                        move |f| generate_price_allocation_line(f, (x, y)),
                        0.0..1.0,
                        1024,
                    );

                    let line = Line::new("Anchor", points)
                        .width(2.6)
                        .style(egui_plot::LineStyle::Solid)
                        .color(Self::ANCHOR_COLOR);

                    ui.line(line);
                })
        });

        let positive_only_grid = self.positive_only_grid;
        let log_grid_spacing = self.log_grid_spacing;

        let mut plot = Plot::new("skyline_plot")
            .custom_x_axes(vec![
                AxisHints::new_x().label_spacing(Rangef::new(30.0, 80.0)),
            ])
            .custom_y_axes(vec![AxisHints::new_y().min_thickness(64.0)])
            .x_axis_label(ctx.axis_names.0)
            .y_axis_label(ctx.axis_names.1)
            .show_grid(true)
            // TODO: How do we hide the legend for a few elements? This will just mark them as "disabled" but still show them in the legend
            // .legend(egui_plot::Legend::default().hidden_items(["X = 0".into()]))
            .legend(
                egui_plot::Legend::default()
                    .color_conflict_handling(egui_plot::ColorConflictHandling::PickFirst),
            )
            .auto_bounds(false)
            .allow_double_click_reset(false)
            .x_grid_spacer(grid_marks(positive_only_grid, log_grid_spacing))
            .y_grid_spacer(grid_marks(positive_only_grid, log_grid_spacing))
            .x_axis_formatter(|m, _| axis_formatter(log_grid_spacing, m))
            .y_axis_formatter(|m, _| axis_formatter(log_grid_spacing, m));

        if self.log_grid_spacing {
            plot = plot.data_aspect(1.0);
        }

        CentralPanel::default()
            .show_inside(ui, |ui| {
                plot.show(ui, |ui| {
                    if ui.response().double_clicked() || self.bounds_need_update {
                        let bounds = ctx.order_frontier_bounds(self.log_grid_spacing);

                        ui.set_plot_bounds(bounds);
                        self.bounds_need_update = false;
                    }

                    self.render_plot_contents(ui, ctx)
                })
            })
            .inner
    }

    fn render_plot_contents(
        &self,
        ui: &mut egui_plot::PlotUi<'_>,
        ctx: PlotContext<'_>,
    ) -> PlotPanelResponse {
        // TODO: Draw the budget line passing through anchor, highlighted point

        if !self.log_grid_spacing {
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
        }

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
            let mut points = vec![current_point, next_point];
            if self.log_grid_spacing {
                points.to_logspace()
            }

            let segment = Line::new("Trajectory", PlotPoints::new(points))
                .color(color)
                .width(2.0)
                .allow_hover(false)
                .style(egui_plot::LineStyle::Solid);

            if alpha > 0 {
                ui.line(segment);
            }

            current_point = next_point;
        }

        // Buy arm: cumulative chain from anchor.
        let mut buy_point = *ctx.anchor;
        let mut buy_nodes = vec![buy_point];
        for order in &ctx.orders.bids {
            // TODO: We may want to set their intensity based on distance from market best bid/ask. As an estimation of when it may go through.
            let color = egui::Color32::from_rgba_unmultiplied(46, 204, 133, 240);

            let next_point = [buy_point[0] + order.delta[0], buy_point[1] + order.delta[1]];

            let mut points = vec![buy_point, next_point];

            if self.log_grid_spacing {
                points.to_logspace()
            }

            ui.line(
                Line::new("Bid Orders", PlotPoints::new(points))
                    .color(color)
                    .width(2.6)
                    .style(egui_plot::LineStyle::Solid),
            );

            buy_point = next_point;
            buy_nodes.push(next_point);
        }

        if self.log_grid_spacing {
            buy_nodes.to_logspace()
        }

        if buy_nodes.len() > 1 {
            ui.points(
                Points::new("Bid Orders", buy_nodes)
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

            let mut points = vec![sell_point, next_point];

            if self.log_grid_spacing {
                points.to_logspace()
            }

            ui.line(
                Line::new("Ask Orders", PlotPoints::new(points))
                    .color(color)
                    .width(2.6)
                    .style(egui_plot::LineStyle::Solid),
            );

            sell_point = next_point;
            sell_nodes.push(next_point);
        }

        if self.log_grid_spacing {
            sell_nodes.to_logspace()
        }

        if sell_nodes.len() > 1 {
            ui.points(
                Points::new("Ask Orders", sell_nodes)
                    .radius(3.2)
                    .color(egui::Color32::from_rgba_unmultiplied(255, 130, 130, 180)),
            );
        }

        let mut anchor_node = vec![*ctx.anchor];
        if self.log_grid_spacing {
            anchor_node.to_logspace();
        }

        ui.points(
            Points::new("Anchor (Current Balance)", anchor_node)
                .radius(6.5)
                .color(Self::ANCHOR_COLOR),
        );

        let mut response = PlotPanelResponse {
            clicked_delta: None,
        };

        if let AppMode::Default = ctx.mode {
            return response;
        }

        if let Some(mouse_pos) = ui.pointer_coordinate() {
            let mut ghost_points = vec![*ctx.anchor];

            if self.log_grid_spacing {
                ghost_points.to_logspace();
            }

            ghost_points.push([mouse_pos.x, mouse_pos.y]);

            let ghost_line = Line::new("Ghost Line", PlotPoints::new(ghost_points))
                .color(egui::Color32::from_rgba_unmultiplied(240, 240, 255, 170))
                .width(1.8)
                .style(egui_plot::LineStyle::Dashed { length: 5.0 });

            ui.line(ghost_line);
        }

        if ui.response().clicked()
            && let Some(PlotPoint { x, y }) = ui.pointer_coordinate()
        {
            if self.log_grid_spacing {
                todo!()
            }

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
