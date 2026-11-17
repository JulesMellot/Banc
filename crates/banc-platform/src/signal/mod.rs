//! Process shutdown request.

use tokio::sync::Notify;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use unix as os;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as os;

/// Shutdown asked from inside the process: by the Windows service manager
/// (see `service`), or by a test.
static REQUESTED: Notify = Notify::const_new();

/// Asks [`shutdown`] to return. If nobody waits yet, the next wait returns
/// at once.
pub fn request_shutdown() {
    REQUESTED.notify_one();
}

/// Waits for the OS to request shutdown: SIGTERM or SIGINT on Linux and
/// macOS, Ctrl+C on Windows, the service manager's stop on Windows, or
/// [`request_shutdown`].
///
/// Uses no CPU while waiting.
pub async fn shutdown() -> std::io::Result<()> {
    tokio::select! {
        result = os::shutdown() => result,
        () = REQUESTED.notified() => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn requested_shutdown_returns() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        super::request_shutdown();
        runtime.block_on(super::shutdown()).unwrap();
    }
}
