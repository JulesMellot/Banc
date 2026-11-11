//! Demande d'arrêt du processus.

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use unix as os;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as os;

/// Attend que l'OS demande l'arrêt : SIGTERM ou SIGINT sous Linux et macOS,
/// Ctrl+C sous Windows. L'arrêt demandé par le gestionnaire de services
/// Windows arrive avec le service système, en phase 1.
///
/// Ne consomme aucun CPU pendant l'attente.
pub async fn shutdown() -> std::io::Result<()> {
    os::shutdown().await
}
