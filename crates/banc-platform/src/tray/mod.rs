//! The tray icon of the session helper (`tray` feature).
//!
//! macOS and Windows: `tray-icon` and `tao`, no web view. Linux: none yet,
//! `tray-icon` needs GTK there; [`run`] answers [`TrayError::Unsupported`]
//! and the session helper simply waits.

use std::future::Future;
use std::time::Duration;

#[cfg(all(feature = "tray", any(target_os = "macos", target_os = "windows")))]
mod native;
#[cfg(all(feature = "tray", any(target_os = "macos", target_os = "windows")))]
use native as os;

#[cfg(not(all(feature = "tray", any(target_os = "macos", target_os = "windows"))))]
mod none;
#[cfg(not(all(feature = "tray", any(target_os = "macos", target_os = "windows"))))]
use none as os;

/// What the icon shows. The texts come from the caller (French interface
/// text lives in `fr.json`).
pub struct Tray {
    /// 32×32 pixels, RGBA.
    pub icon_rgba: Vec<u8>,
    pub icon_size: u32,
    pub open_label: String,
    pub quit_label: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TrayError {
    #[error("no tray icon on this system")]
    Unsupported,
    #[error("tray icon: {0}")]
    Failed(String),
}

/// Shows the icon and runs the event loop on the calling thread, which must
/// be the main thread, until the user quits or the OS asks the process to
/// stop.
///
/// Every `interval`, `status` runs on a background thread; its text becomes
/// the first line of the menu and the tooltip. `open` runs when the user
/// picks the first action.
pub fn run<S, F>(
    tray: Tray,
    interval: Duration,
    status: S,
    open: impl FnMut() + 'static,
) -> Result<(), TrayError>
where
    S: FnMut() -> F + Send + 'static,
    F: Future<Output = String>,
{
    os::run(tray, interval, status, open)
}
