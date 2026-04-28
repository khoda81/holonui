use std::f64::consts::LN_10;

pub fn allocation_to_log_price(f: f64, (x, y): (f64, f64)) -> f64 {
    let ln_price = y.ln() - x.ln() + f.ln() - (-f).ln_1p();

    ln_price * LN_10.recip()
}

pub fn log_price_to_allocation(log_dx_dy: f64, (x, y): (f64, f64)) -> f64 {
    let v_x = x * (log_dx_dy * LN_10).exp();

    v_x / (y + v_x)
}
