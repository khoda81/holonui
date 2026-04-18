mod app;
mod utils;

pub type Allocation = [f64; 2];

// Re-export the main app struct so main.rs can see it
pub use app::HolonApp;
