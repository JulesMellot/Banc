use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use tokio::net::{UnixListener, UnixStream};

pub type ServerStream = UnixStream;
pub type ClientStream = UnixStream;

pub(super) struct Listener {
    listener: UnixListener,
}

impl Listener {
    pub(super) fn bind(endpoint: &Path) -> std::io::Result<Self> {
        if let Some(dir) = endpoint.parent() {
            std::fs::create_dir_all(dir)?;
        }
        match std::fs::remove_file(endpoint) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        let listener = UnixListener::bind(endpoint)?;
        // Every local user may connect; the agent filters the requests.
        std::fs::set_permissions(endpoint, std::fs::Permissions::from_mode(0o666))?;
        Ok(Self { listener })
    }

    pub(super) async fn accept(&mut self) -> std::io::Result<ServerStream> {
        let (stream, _) = self.listener.accept().await?;
        Ok(stream)
    }
}

pub(super) async fn connect(endpoint: &Path) -> std::io::Result<ClientStream> {
    UnixStream::connect(endpoint).await
}

pub(super) fn endpoint_in(dir: &Path, name: &str) -> std::path::PathBuf {
    dir.join(format!("{name}.sock"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn rebind_after_a_crash() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let endpoint = tmp.path().join("run/agent.sock");
        runtime.block_on(async {
            let first = super::Listener::bind(&endpoint).unwrap();
            // A crashed agent leaves its socket file behind.
            std::mem::forget(first);
            super::Listener::bind(&endpoint).unwrap();
        });
    }
}
