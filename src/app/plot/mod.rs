mod price_allocation;

use std::f64::consts::LN_10;

use chrono::{DateTime, Utc};
use egui::{CentralPanel, Color32, Panel, Rangef};
use egui_plot::{
    AxisHints, GridInput, GridMark, HLine, Legend, Line, Plot, PlotBounds, PlotPoint, PlotPoints,
    PlotResponse, PlotUi, Points, VLine,
};

use super::{
    AppMode,
    backend::models::{Order, Trade, TradeType},
};
use crate::utils::{format_human_num, log_ln_grid_spacer};
use price_allocation::log_price_to_allocation;

// Define colors as constants for clarity and easy modification.
const AXIS_COLOR: Color32 = Color32::from_rgba_unmultiplied_const(250, 238, 214, 230);
const ANCHOR_COLOR: Color32 = Color32::from_rgba_unmultiplied_const(255, 215, 0, 255);
const BID_LINE_COLOR: Color32 = Color32::from_rgba_unmultiplied_const(46, 204, 133, 240);
const BID_POINTS_COLOR: Color32 = Color32::from_rgba_unmultiplied_const(92, 230, 164, 170);
const ASK_LINE_COLOR: Color32 = Color32::from_rgba_unmultiplied_const(235, 87, 87, 240);
const ASK_POINTS_COLOR: Color32 = Color32::from_rgba_unmultiplied_const(255, 130, 130, 180);
const TRAJECTORY_BASE_COLOR: [u8; 3] = [108, 156, 217];

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

/// Draws one "wing" (either bids or asks) of the exposure curve in a single pass.
/// Assumes bids are sorted high-to-low price, and asks are low-to-high price.
fn draw_exposure_wing(
    ui: &mut PlotUi,
    orders: &[OrderSegment],
    mut cumulative_balance: [f64; 2],
    plot_edge_log_price: f64,
    is_bids: bool,
) -> f64 {
    let (line_color, curve_color) = if is_bids {
        (BID_LINE_COLOR, BID_POINTS_COLOR)
    } else {
        (ASK_LINE_COLOR, ASK_POINTS_COLOR)
    };

    // The price of the order closest to the anchor.
    // We will update this as we draw the final segment to the plot edge.
    let Some(first_order) = orders.first() else {
        // If no orders, the anchor curve extends to the edge.
        return plot_edge_log_price;
    };

    let last_log_price = (-first_order.delta[1] / first_order.delta[0]).log10();

    for i in 0..orders.len() {
        let order = &orders[i];
        let log_price = (-order.delta[1] / order.delta[0]).log10();

        // 1. Draw the horizontal "fill" line
        let f_before_fill = log_price_to_allocation(log_price, cumulative_balance);
        let balance_after_fill = [
            cumulative_balance[0] + order.delta[0],
            cumulative_balance[1] + order.delta[1],
        ];
        let f_after_fill = log_price_to_allocation(log_price, balance_after_fill);
        ui.line(
            Line::new(
                "Fill",
                PlotPoints::new(vec![[f_before_fill, log_price], [f_after_fill, log_price]]),
            )
            .color(line_color),
        );

        // 2. Update the balance for the next segment
        cumulative_balance = balance_after_fill;

        // 3. Determine the range for the next curve segment
        let next_log_price = if let Some(next_order) = orders.get(i + 1) {
            (-next_order.delta[1] / next_order.delta[0]).log10()
        } else {
            plot_edge_log_price // This is the last order, so draw to the plot edge
        };

        // 4. Draw the curve segment
        let range = if is_bids {
            next_log_price..=log_price
        } else {
            log_price..=next_log_price
        };
        let points = PlotPoints::from_parametric_callback(
            move |log_p| (log_price_to_allocation(log_p, cumulative_balance), log_p),
            range,
            128,
        );

        ui.line(Line::new("Curve", points).color(curve_color));
    }

    last_log_price
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
    // In your impl
    fn render_exposure_curve(&self, ui: &mut PlotUi) {
        let anchor_balance = *self.anchor;
        let y_range = ui.plot_bounds().range_y();

        // Draw the bids wing (from anchor down to the bottom edge)
        // Assumes self.orders.bids are sorted from highest price to lowest.
        let highest_bid_log_price = draw_exposure_wing(
            ui,
            &self.orders.bids,
            anchor_balance,
            *y_range.start(),
            true, // is_bids = true
        );

        // Draw the asks wing (from anchor up to the top edge)
        // Assumes self.orders.asks are sorted from lowest price to highest.
        let lowest_ask_log_price = draw_exposure_wing(
            ui,
            &self.orders.asks,
            anchor_balance,
            *y_range.end(),
            false, // is_bids = false
        );

        // Ensure there's a valid gap between the best bid and ask before drawing the anchor
        if lowest_ask_log_price <= highest_bid_log_price {
            return;
        }

        // Draw the central anchor curve in the space between the best bid and ask
        let points = PlotPoints::from_parametric_callback(
            move |log_p| (log_price_to_allocation(log_p, anchor_balance), log_p),
            highest_bid_log_price..=lowest_ask_log_price,
            256,
        );
        ui.line(Line::new("Anchor", points).color(ANCHOR_COLOR));
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
                .legend(
                    Legend::default()
                        .color_conflict_handling(egui_plot::ColorConflictHandling::PickFirst),
                )
                .custom_y_axes(vec![AxisHints::new_y().min_thickness(48.0)])
                .default_x_bounds(0.0, 1.0000001)
                .allow_zoom([false, true])
                .allow_scroll([false, true])
                .allow_drag([false, true])
                .allow_axis_zoom_drag([false, true])
                .auto_bounds(false)
                // .x_grid_spacer(egui_plot::uniform_grid_spacer([10.0, 5.0, 2.0]))
                .y_grid_spacer(grid_marks(true, true))
                .y_axis_formatter(|m, _| axis_formatter(true, m))
                .show(ui, |ui| ctx.render_exposure_curve(ui));
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

    /// The main rendering function, now acting as a high-level coordinator.
    fn render_plot_contents(&self, ui: &mut PlotUi, ctx: PlotContext<'_>) -> PlotPanelResponse {
        self.draw_origin_axes(ui);
        self.draw_trajectory(ui, &ctx);

        self.draw_order_chain(
            ui,
            *ctx.anchor,
            &ctx.orders.bids,
            "Bid Orders",
            BID_LINE_COLOR,
            BID_POINTS_COLOR,
        );

        self.draw_order_chain(
            ui,
            *ctx.anchor,
            &ctx.orders.asks,
            "Ask Orders",
            ASK_LINE_COLOR,
            ASK_POINTS_COLOR,
        );

        self.draw_anchor_point(ui, *ctx.anchor);

        self.handle_user_interaction(ui, ctx)
    }

    /// Helper to transform points to logspace if required.
    fn transform_points(&self, points: &mut Vec<[f64; 2]>) {
        if self.log_grid_spacing {
            points.to_logspace();
        }
    }

    /// Draws the X and Y axes if not in log scale.
    fn draw_origin_axes(&self, ui: &mut PlotUi) {
        if !self.log_grid_spacing {
            ui.hline(HLine::new("Y = 0", 0.0).color(AXIS_COLOR).width(2.2));
            ui.vline(VLine::new("X = 0", 0.0).color(AXIS_COLOR).width(2.2));
        }
    }

    /// Draws the historical trade trajectory with age-based fading.
    fn draw_trajectory(&self, ui: &mut PlotUi, ctx: &PlotContext<'_>) {
        let mut current_point = *ctx.anchor;
        for segment in ctx.trajectory.iter() {
            let elapsed_s = (ctx.now - segment.timestamp).as_seconds_f64().max(0.0);
            let delta_t = elapsed_s / self.time_unit.as_seconds_f64();
            let freshness = (-delta_t as f32).exp(); // 1.0 = recent, 0.0 = old
            let alpha = (215.0 * freshness) as u8;

            if alpha == 0 {
                // Optimization: if it's fully transparent, don't draw.
                current_point = [
                    current_point[0] + segment.delta[0],
                    current_point[1] + segment.delta[1],
                ];
                continue;
            }

            let color = Color32::from_rgba_unmultiplied(
                TRAJECTORY_BASE_COLOR[0],
                TRAJECTORY_BASE_COLOR[1],
                TRAJECTORY_BASE_COLOR[2],
                alpha,
            );

            let next_point = [
                current_point[0] + segment.delta[0],
                current_point[1] + segment.delta[1],
            ];
            let mut points = vec![current_point, next_point];
            self.transform_points(&mut points);

            let line = Line::new("Trajectory", PlotPoints::new(points))
                .color(color)
                .width(2.0);

            ui.line(line);

            current_point = next_point;
        }
    }

    /// Draws a chain of orders (either bids or asks) starting from the anchor.
    fn draw_order_chain(
        &self,
        ui: &mut PlotUi,
        anchor: [f64; 2],
        orders: &[OrderSegment],
        name: &'static str,
        line_color: Color32,
        points_color: Color32,
    ) {
        if orders.is_empty() {
            return;
        }

        let mut current_point = anchor;
        let mut nodes = vec![anchor];

        for order in orders {
            let next_point = [
                current_point[0] + order.delta[0],
                current_point[1] + order.delta[1],
            ];
            let mut line_points = vec![current_point, next_point];
            self.transform_points(&mut line_points);

            let line = Line::new(name, PlotPoints::new(line_points))
                .color(line_color)
                .width(2.6);

            ui.line(line);

            current_point = next_point;
            nodes.push(next_point);
        }

        self.transform_points(&mut nodes);
        let points = Points::new(name, nodes).radius(3.2).color(points_color);
        ui.points(points);
    }

    /// Draws the central anchor point representing the current balance.
    fn draw_anchor_point(&self, ui: &mut PlotUi, anchor: [f64; 2]) {
        let mut anchor_node = vec![anchor];
        self.transform_points(&mut anchor_node);
        ui.points(
            Points::new("Anchor (Current Balance)", anchor_node)
                .radius(6.5)
                .color(ANCHOR_COLOR),
        );
    }

    /// Handles drawing the ghost line and processing clicks for new orders.
    fn handle_user_interaction(&self, ui: &mut PlotUi, ctx: PlotContext<'_>) -> PlotPanelResponse {
        let mut response = PlotPanelResponse {
            clicked_delta: None,
        };

        if let AppMode::Default = ctx.mode {
            return response;
        }

        // Draw ghost line to mouse position
        if let Some(mouse_pos) = ui.pointer_coordinate() {
            let mut ghost_points = vec![*ctx.anchor, [mouse_pos.x, mouse_pos.y]];
            self.transform_points(&mut ghost_points);

            let ghost_line = Line::new("Order", ghost_points)
                .color(Color32::from_rgba_unmultiplied(240, 240, 255, 170))
                .width(1.8)
                .style(egui_plot::LineStyle::Dashed { length: 5.0 });

            ui.line(ghost_line);
        }

        // Handle clicks
        if ui.response().clicked()
            && let Some(point) = ui.pointer_coordinate()
        {
            if self.log_grid_spacing {
                // You still need to implement the inverse log transformation here.
                // For example:
                // let x = 10.0f64.powf(point.x);
                // let y = 10.0f64.powf(point.y);
                // response.clicked_delta = Some([x - ctx.anchor[0], y - ctx.anchor[1]]);
                todo!("Handle click in log space");
            } else {
                response.clicked_delta = Some([point.x - ctx.anchor[0], point.y - ctx.anchor[1]]);
            }
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
