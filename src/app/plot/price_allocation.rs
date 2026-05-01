use std::f64::consts::LN_10;

pub fn log_price_to_allocation(log_dx_dy: f64, [x, y]: [f64; 2]) -> f64 {
    let v_x = x * (log_dx_dy * LN_10).exp();

    v_x / (y + v_x)
}
