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

pub fn log10_grid_spacer(input: GridInput) -> Vec<GridMark> {
    let base = 10f64;
    let ln_base = base.ln();

    fn log_marks(step_size: f64, min_y: f64) -> impl Iterator<Item = GridMark> {
        let min_power = (min_y / step_size).ceil() as i64;

        (min_power..).map(move |power| GridMark {
            value: (power as f64) * step_size,
            step_size,
        })
    }

    let mut marks = Vec::new();

    let (min_y, max_y) = input.bounds;

    // Upper scales
    marks.extend(log_marks(1.0, min_y).take_while(|m| m.value <= max_y));
    let mut step_size = 3.0;

    // Calculate the visible span
    let view_span = max_y - min_y;

    loop {
        log::trace!("Log   : {}, step_size: {}", marks.len(), step_size);
        marks.extend(log_marks(step_size, min_y).take_while(|m| m.value <= max_y));

        // Break when the step size is larger than the screen span.
        // We multiply by 2.0 to ensure egui has one level larger than the screen
        // to handle cross-fading gracefully.
        if step_size > view_span * 2.0 {
            break;
        }

        step_size *= 2.0;
    }

    // Lower scales
    let max_visible_x = 0.5 / input.base_step_size;

    // Calculate the exact lowest span where marks are still visible
    // base.powf(min_y - span) <= max_visible_x  ->  span >= min_y - log10(max_visible_x)
    let start_span = (min_y - max_visible_x.ln() / ln_base).ceil() as i32;

    log::trace!(
        "Start Span: {start_span}, Base Step: {}",
        input.base_step_size
    );

    for span in start_span.. {
        let span = span as f64;
        let min_x = base.powf(min_y - span).ceil() as u64;
        let mut no_new_mark = true;

        let new_marks = (min_x..)
            .map(move |x| GridMark {
                value: (x as f64).ln() / ln_base + span,
                step_size: 0.5 / (x as f64),
            })
            .take_while(|m| m.step_size >= input.base_step_size)
            .take_while(|m| m.value <= max_y)
            .inspect(|_| no_new_mark = false);

        marks.extend(new_marks);

        if no_new_mark {
            break;
        }

        log::trace!("Linear (span {span})");
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
