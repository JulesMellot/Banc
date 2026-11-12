//! Process shutdown request.

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use unix as os;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as os;

/// Waits for the OS to request shutdown: SIGTERM or SIGINT on Linux and
/// macOS, Ctrl+C on Windows. Shutdown requested by the Windows service
/// manager arrives with the system service, in phase 1.
///
/// Uses no CPU while waiting.
pub async fn shutdown() -> std::io::Result<()> {
    os::shutdown().await
}
