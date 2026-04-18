#[cfg(not(target_arch = "wasm32"))]
use std::thread;
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn_local<F>(future: F) -> thread::JoinHandle<F::Output>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    // On native, spin up a background thread and a lightweight Tokio runtime
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    })
}

#[cfg(target_arch = "wasm32")]
pub fn spawn_local<F>(future: F)
where
    F: std::future::Future<Output = ()> + 'static,
{
    // On web, give the async task to the JavaScript event loop
    wasm_bindgen_futures::spawn_local(future)
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn sleep_async(duration: Duration) {
    tokio::time::sleep(duration).await;
}

#[cfg(target_arch = "wasm32")]
pub async fn sleep_async(duration: Duration) {
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
