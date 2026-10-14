#[cfg(unix)]
use log::debug;
#[cfg(unix)]
use signal_hook::{consts::SIGUSR2, iterator::Signals};
#[cfg(unix)]
use tauri::AppHandle;

/// SIGUSR2 toggles a meeting. SIGUSR1 is not handled: on Linux it belongs to
/// WebKitGTK's garbage collector (#1660).
#[cfg(unix)]
pub fn setup_signal_handler(app_handle: AppHandle) {
    let mut signals = Signals::new([SIGUSR2]).expect("failed to register SIGUSR2 handler");
    debug!("Signal handler registered (SIGUSR2)");
    std::thread::spawn(move || {
        for _ in signals.forever() {
            debug!("Received SIGUSR2");
            crate::meeting::toggle::toggle_meeting(&app_handle);
        }
    });
}
