//! The agent's local endpoint, through which the interface talks to it.
//!
//! A Unix socket on Linux and macOS, a named pipe on Windows. Any local
//! user may connect: the agent decides what each request is allowed to do.

use std::path::Path;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use unix as os;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as os;

/// A connection to the agent.
pub use os::ClientStream;
/// A connection accepted by the agent.
pub use os::ServerStream;

pub struct LocalListener {
    inner: os::Listener,
}

impl LocalListener {
    /// Starts listening. A leftover socket from a previous run is replaced.
    pub fn bind(endpoint: &Path) -> std::io::Result<Self> {
        Ok(Self {
            inner: os::Listener::bind(endpoint)?,
        })
    }

    pub async fn accept(&mut self) -> std::io::Result<ServerStream> {
        self.inner.accept().await
    }
}

/// An endpoint private to a folder, for development and tests: a socket
/// file in it, or on Windows a pipe whose name is derived from it.
pub fn endpoint_in(dir: &Path, name: &str) -> std::path::PathBuf {
    os::endpoint_in(dir, name)
}

/// Connects to the agent.
pub async fn connect(endpoint: &Path) -> std::io::Result<ClientStream> {
    os::connect(endpoint).await
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[test]
    fn request_and_response() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let endpoint = test_endpoint(tmp.path());
        runtime.block_on(async {
            let mut listener = LocalListener::bind(&endpoint).unwrap();
            let server = async {
                let mut stream = listener.accept().await.unwrap();
                let mut buffer = [0; 4];
                stream.read_exact(&mut buffer).await.unwrap();
                stream.write_all(b"pong").await.unwrap();
                buffer
            };
            let client = async {
                let mut stream = connect(&endpoint).await.unwrap();
                stream.write_all(b"ping").await.unwrap();
                let mut buffer = [0; 4];
                stream.read_exact(&mut buffer).await.unwrap();
                buffer
            };
            let (received, answered) = tokio::join!(server, client);
            assert_eq!(&received, b"ping");
            assert_eq!(&answered, b"pong");
        });
    }

    fn test_endpoint(dir: &Path) -> std::path::PathBuf {
        endpoint_in(dir, "agent")
    }
}
