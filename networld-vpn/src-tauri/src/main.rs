// Hide the console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // A tunnelling client is I/O bound: two worker threads keep every proxy
    // listener responsive while saving a thread stack per extra CPU core.
    // (Tokio reads this when Tauri builds its multi-thread runtime.)
    if std::env::var_os("TOKIO_WORKER_THREADS").is_none() {
        std::env::set_var("TOKIO_WORKER_THREADS", "2");
    }
    networld_vpn_lib::run();
}
