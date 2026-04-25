use egui_plot::{GridInput, GridMark};
use std::{ops::RangeInclusive, time::Duration};

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
        ..1e-3 => format!("{:4.0e}", num),
        ..1e3 => format!("{:4}", keep_4_significant_digits(num)),
        ..1e6 => format!("{:4}K", keep_4_significant_digits(num / 1e3)),
        ..1e9 => format!("{:4}M", keep_4_significant_digits(num / 1e6)),
        ..1e12 => format!("{:4}B", keep_4_significant_digits(num / 1e9)),
        ..1e15 => format!("{:4}T", keep_4_significant_digits(num / 1e12)),
        _ => format!("{:4.0e}", keep_4_significant_digits(num)),
    }
}

fn keep_4_significant_digits(num: f64) -> f64 {
    (num * 1e3).round() / 1e3
}

pub fn log10_grid_spacer(input: GridInput) -> Vec<GridMark> {
    let mut marks = Vec::new();

    let (min_y, max_y) = input.bounds;

    marks.extend(log_marks(1.0, min_y..=max_y));
    let mut step_size = 3.0;
    loop {
        let prev_len = marks.len();
        marks.extend(log_marks(step_size, min_y..=max_y));
        step_size *= 2.0;
        if marks.len() <= prev_len + 3 {
            break;
        }
    }

    // // 2. Normal to zoomed-in view
    // let min_power = min_y.floor() as i32;
    // let max_power = max_y.ceil() as i32;

    // // Determine how deep we need to subdivide.
    // // If view_span is 1.0, depth=0 (1-9). If view_span is 0.1, depth=1 (1.1-1.9), etc.
    // let depth = if view_span >= 1.0 {
    //     0
    // } else {
    //     (-view_span.floor()) as i32
    // };

    // let mut depths = [depth, depth - 1, depth - 2, depth - 3, depth - 4];
    // depths.reverse();

    // for depth in depths {
    //     // $\Delta x$ at the base level (e.g., 1.0, 0.1, 0.01)
    //     let linear_step_multiplier = 10_f64.powi(-depth);

    //     for power in min_power - 1..=max_power {
    //         // Calculate the maximum multiplier we need to check to cover the range
    //         // For depth=0, it's 1..10. For depth=1, it's 10..100, etc.
    //         let start_mult = (20_f64.powi(depth)).round() as i64;
    //         let end_mult = start_mult * 199;

    //         for m in start_mult..end_mult {
    //             let y = (m as f64).log10() + (power - depth) as f64;

    //             marks.push(GridMark {
    //                 value: y,
    //                 step_size: linear_step_multiplier / m as f64,
    //             });
    //         }
    //     }
    // }

    marks
}

fn log_marks(
    step_size: f64,
    bounds_range: RangeInclusive<f64>,
) -> std::iter::Map<RangeInclusive<i32>, impl FnMut(i32) -> GridMark> {
    let min_power = (bounds_range.start() / step_size).floor() as i32;
    let max_power = (bounds_range.end() / step_size).ceil() as i32;

    (min_power..=max_power).map(move |power| GridMark {
        value: (power as f64) * step_size,
        step_size,
    })
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
