use egui_plot::{GridInput, GridMark};
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_task<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(future);
}

#[cfg(target_arch = "wasm32")]
pub fn spawn_task<F>(future: F)
where
    // Notice `Send` is completely removed here
    F: std::future::Future<Output = ()> + 'static,
{
    wasm_bindgen_futures::spawn_local(future);
}

pub async fn sleep_async(duration: Duration) {
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(duration).await;

    #[cfg(target_arch = "wasm32")]
    // If you add `gloo-timers = { version = "0.3", features = ["futures"] }`:
    gloo_timers::future::sleep(duration).await;
}

pub fn format_humantime(sec: f64) -> String {
    let std_dur = std::time::Duration::from_secs_f64(sec);
    let chrono_dur = chrono::TimeDelta::from_std(std_dur).unwrap();

    // 2. Extract components
    let total_days = chrono_dur.num_days();
    let hours = chrono_dur.num_hours() % 24;
    let mins = chrono_dur.num_minutes() % 60;
    let secs = chrono_dur.num_seconds() % 60;

    match sec {
        ..60.0 => format!("{sec:.0} sec"),
        ..3600.0 => format!("{mins:.0} min {secs:.0} s"),
        ..86400.0 => format!("{hours:.0} hr {mins:.0} m"),
        ..31_536_000.0 => format!("{total_days:.0} days {hours:.0} h"),
        _ => format!("{:.1} years", total_days / 356),
    }
}

pub fn format_human_num(num: f64, scale: f64) -> String {
    if num == 0.0 {
        return String::new();
    }

    match scale {
        ..1e0 => format!("{:4.3}", num),
        ..1e3 => format!("{:4.3}", keep_4_significant_digits(num)),
        ..1e6 => format!("{:4.3}K", keep_4_significant_digits(num / 1e3)),
        ..1e9 => format!("{:4.3}M", keep_4_significant_digits(num / 1e6)),
        ..1e12 => format!("{:4.3}B", keep_4_significant_digits(num / 1e9)),
        _ => format!("{:4.3}T", keep_4_significant_digits(num / 1e12)),
    }
}

fn keep_4_significant_digits(num: f64) -> f64 {
    (num * 1e3).round() / 1e3
}

pub fn log10_grid_spacer(input: GridInput) -> Vec<GridMark> {
    let mut marks = Vec::new();

    // input.bounds gives us the visible min and max of your Y-axis (which is log10(p))
    let (min_y, max_y) = input.bounds;

    // Find the minimum and maximum powers of 10 currently visible
    let min_power = min_y.floor() as i32;
    let max_power = max_y.ceil() as i32;
    let max_power = (min_power + 1000).min(max_power);

    // If the user zooms out too far (e.g., viewing > 5 orders of magnitude),
    // we stop drawing minor ticks to prevent screen clutter.
    // let show_minor_ticks = (max_y - min_y) < 10.0;
    let show_minor_ticks = true;

    for power in min_power..=max_power {
        // 1. MAJOR TICK: Exact powers of 10 (e.g., y = 1.0, 2.0, 3.0)
        let major_y = power as f64;
        if major_y >= min_y && major_y <= max_y {
            marks.push(GridMark {
                value: major_y,
                step_size: 1.0, // Large step_size tells egui to draw a thicker line
            });
        }

        // 2. MINOR TICKS: The lines that get closer together
        if show_minor_ticks {
            for multiplier in 2..=90 {
                // Calculate actual price: e.g., 2 * 10^1 = 20
                let multiplier = multiplier as f64;
                let minor_p = multiplier * 10f64.powi(power);
                // Convert back to log space for the Y-axis coordinate
                let minor_y = minor_p.log10();

                if minor_y >= min_y && minor_y <= max_y {
                    marks.push(GridMark {
                        value: minor_y,
                        step_size: 0.5 / multiplier, // Smaller step_size tells egui to draw a fainter line
                    });
                }
            }
        }
    }

    marks
}

pub fn get_api_key() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    return {
        log::debug!("Loading key from keyring");
        keyring::Entry::new("holonui", "default-user")
            .ok()?
            .get_password()
    }
    .or_else(|_| std::env::var("NOBITEX_API_KEY"))
    .ok();

    #[cfg(target_arch = "wasm32")]
    return std::env::var("NOBITEX_API_KEY").ok();
}
