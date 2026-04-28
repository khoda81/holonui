use std::f64::consts::LN_10;

pub fn allocation_to_price(f: f64, (x, y): (f64, f64)) -> f64 {
    (y.ln() - x.ln() + f.ln() - (-f).ln_1p()) / LN_10
}

pub fn price_to_allocation(log_dx_dy: f64, (x, y): (f64, f64)) -> f64 {
    let p_exp = (log_dx_dy * LN_10).exp();

    let k = x * p_exp / y;

    k / (1.0 + k)
}
