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
    let base = 10f64;
    let ln_base = base.ln();

    let mut marks = Vec::new();

    let (min_y, max_y) = input.bounds;

    // Upper scales
    marks.extend(log_marks(1.0, min_y).take_while(|m| m.value <= max_y));
    let mut step_size = 3.0;
    loop {
        let prev_len = marks.len();

        log::trace!("Log   : {}", marks.len());
        marks.extend(log_marks(step_size, min_y).take_while(|m| m.value <= max_y));

        // TODO: We should probably use a better condition here
        if marks.len() <= prev_len + 3 {
            break;
        }

        step_size *= 2.0;
    }

    // Lower scales
    let mut span = max_y.floor();

    log::trace!("Span: {span}, Base Step: {}", input.base_step_size);

    // The maximum `x` multiplier before lines fade to invisible
    let max_visible_x = 0.5 / input.base_step_size;

    loop {
        let step_size = 10f64.powf(span);
        let min_linear = 10f64.powf(min_y - span);

        // Break if we hit Infinity (zoomed out too far for f64)
        // or if the starting line is already too dense to be visible
        if !min_linear.is_finite() || min_linear > max_visible_x {
            break;
        }

        marks.extend(
            linear_marks(step_size, ln_base, min_y)
                .take_while(|m| m.step_size >= input.base_step_size)
                .take_while(|m| m.value <= max_y),
        );

        log::trace!("Linear: {}", marks.len());

        span -= 1.0;
    }

    marks.sort_by(|a, b| {
        a.value
            .total_cmp(&b.value)
            .then(b.step_size.total_cmp(&a.step_size))
    });

    // TODO: Compare using an epsilon instead as we are dealing with floats
    marks.dedup_by_key(|m| m.value);

    marks
}

fn log_marks(step_size: f64, min_y: f64) -> impl Iterator<Item = GridMark> {
    let min_power = (min_y / step_size).ceil() as i32;

    (min_power..).map(move |power| GridMark {
        value: (power as f64) * step_size,
        step_size,
    })
}

fn linear_marks(step_size: f64, ln_base: f64, min_y: f64) -> impl Iterator<Item = GridMark> {
    // DO the compute in log space
    let min_linear = (min_y * ln_base).exp() / step_size;

    // Do all clamping in f64 space.
    // Cap max_x at max_visible_x because anything beyond that has 0 opacity anyway.
    let min_x = (min_linear.ceil() as u32).max(1);

    // Only cast to u32 now that we know the values are small and finite
    // If min_x > max_x (e.g. out of bounds), this naturally yields an empty iterator.
    (min_x..).map(move |x| GridMark {
        value: (x as f64 * step_size).log10(),
        // TODO: This unfortunately produces less precise values and I am not sure how to fix that
        // value: (x as f64 * step_size).ln() / ln_base,
        step_size: 0.5 / (x as f64),
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
