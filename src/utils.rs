use core::f64;
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

    fn keep_4_significant_digits(num: f64) -> f64 {
        (num * 1e3).round() / 1e3
    }

    match scale {
        ..1e-3 => format!("{:4.0e}", num),
        ..1e3 => format!("{:4}", keep_4_significant_digits(num)),
        ..1e6 => format!("{:4}K", keep_4_significant_digits(num / 1e3)),
        ..1e9 => format!("{:4}M", keep_4_significant_digits(num / 1e6)),
        ..1e12 => format!("{:4}B", keep_4_significant_digits(num / 1e9)),
        ..1e15 => format!("{:4}T", keep_4_significant_digits(num / 1e12)),
        _ => format!("{:4.0e}", num),
    }
}

pub fn log_ln_grid_spacer(input: GridInput, ln_base: f64) -> Vec<GridMark> {
    let base = ln_base.exp();
    let inv_ln_base = ln_base.recip();

    fn log_marks(step_size: f64, min_y: f64) -> impl Iterator<Item = GridMark> {
        let min_power = (min_y / step_size).ceil() as i64;

        (min_power..).map(move |power| GridMark {
            value: (power as f64) * step_size,
            step_size,
        })
    }

    let mut marks = Vec::new();
    let (min_y, max_y) = input.bounds;

    // Upper scales (Major ticks)
    marks.extend(log_marks(1.0, min_y).take_while(|m| m.value <= max_y));
    let mut step_size = 3.0;
    let view_span = max_y - min_y;

    // TODO: This is incorrect, we should reverse this loop
    while step_size <= view_span * 2.0 {
        marks.extend(log_marks(step_size, min_y).take_while(|m| m.value <= max_y));

        step_size *= 2.0;
    }

    // Sub-scales (Minor ticks between powers)
    // span + offset must be between min_y and max_y.
    let offset = input.base_step_size.ln() * inv_ln_base;
    let start_span = (min_y + offset).ceil();
    let end_span = max_y.ceil();

    let mut x_start_f64 = ((min_y - start_span) * ln_base).exp();

    for span in start_span as i32..end_span as i32 {
        // Clamp to our valid $x$ range [2, base]
        let start_x_f64 = x_start_f64.max(2.0).ceil();
        let mut prev_value = (start_x_f64 - 1.0).ln() * inv_ln_base + span as f64;

        let new_marks = (-1..)
            .map(|idx| {
                let value = (start_x_f64 + idx as f64).ln_1p() * inv_ln_base + span as f64;
                let step_size = value - prev_value;
                prev_value = value;

                GridMark { value, step_size }
            })
            .take_while(|mark| max_y >= mark.value)
            .take_while(|mark| mark.step_size >= input.base_step_size);

        marks.extend(new_marks);
        x_start_f64 /= base;
    }

    marks.sort_by(|a, b| {
        a.value
            .total_cmp(&b.value)
            .then(b.step_size.total_cmp(&a.step_size))
    });

    // TODO: Compare using an epsilon instead as we are dealing with floats
    // marks.dedup_by(|a, b| (a.value / b.value).log10().abs() < 1e-10);
    marks.dedup_by_key(|m| m.value);

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
