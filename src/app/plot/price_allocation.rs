pub fn generate_price_allocation_line(f: f64, (x, y): (f64, f64)) -> f64 {
    x.ln() - y.ln() + (-f).ln_1p() - f.ln()
}
