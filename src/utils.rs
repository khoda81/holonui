#[cfg(not(target_arch = "wasm32"))]
use std::thread;

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
