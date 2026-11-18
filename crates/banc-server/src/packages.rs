//! The package store, and its delivery to the agents.
//!
//! A file is kept under its BLAKE3 hash in `data_dir/packages`. An agent only
//! gets the latest manifest and the files it lists: everything it receives is
//! checked on the workstation against the admin's signature.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use banc_proto::control;
use banc_proto::manifest::Manifest;
use banc_proto::packages::{Answer, Request};
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Semaphore;

use crate::state::AppState;
use crate::store;

/// Largest file the store accepts.
const MAX_FILE: u64 = 64 * 1024 * 1024 * 1024;
/// Files sent at the same time: a small VPS has to keep answering.
const MAX_SENDING: usize = 4;
/// An agent that asks for nothing during this time is disconnected.
const IDLE: Duration = Duration::from_secs(60);
const CHUNK: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ReceiveError {
    #[error("the file is too large")]
    TooLarge,
    #[error("the upload was interrupted")]
    Interrupted,
    #[error("writing the file")]
    Io(#[from] std::io::Error),
}

/// A file now in the store.
#[derive(Debug, PartialEq, Eq)]
pub struct Stored {
    pub blake3: String,
    pub size: u64,
}

/// Where the content of a file is kept. `None` for anything but a hash.
fn file_path(dir: &Path, blake3: &str) -> Option<PathBuf> {
    let valid = blake3.len() == 64
        && blake3
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    valid.then(|| dir.join(blake3))
}

/// Writes a stream to the store. A file only appears under its hash once it
/// has been received whole.
pub async fn receive<S, E>(dir: &Path, stream: S) -> Result<Stored, ReceiveError>
where
    S: futures_util::Stream<Item = Result<axum::body::Bytes, E>>,
{
    tokio::fs::create_dir_all(dir).await?;
    let suffix: [u8; 8] =
        crate::auth::random_bytes().map_err(|error| std::io::Error::other(error.to_string()))?;
    let temporary = dir.join(format!(
        "upload-{}.part",
        data_encoding::HEXLOWER.encode(&suffix)
    ));
    let result = write_to(&temporary, dir, stream).await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temporary).await;
    }
    result
}

async fn write_to<S, E>(temporary: &Path, dir: &Path, stream: S) -> Result<Stored, ReceiveError>
where
    S: futures_util::Stream<Item = Result<axum::body::Bytes, E>>,
{
    use futures_util::StreamExt;

    let mut file = tokio::fs::File::create(temporary).await?;
    let mut hasher = blake3::Hasher::new();
    let mut size = 0_u64;
    let mut stream = std::pin::pin!(stream);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ReceiveError::Interrupted)?;
        size += chunk.len() as u64;
        if size > MAX_FILE {
            return Err(ReceiveError::TooLarge);
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.sync_all().await?;
    drop(file);
    let blake3 = hasher.finalize().to_hex().to_string();
    tokio::fs::rename(temporary, dir.join(&blake3)).await?;
    Ok(Stored { blake3, size })
}

/// The hashes a manifest lists.
fn listed_files(manifest: &Manifest) -> HashSet<&str> {
    manifest
        .packages
        .iter()
        .flat_map(|package| &package.builds)
        .filter_map(|build| build.file.as_ref())
        .map(|file| file.blake3.as_str())
        .collect()
}

#[derive(Clone)]
pub struct PackagesProtocol {
    state: Arc<AppState>,
    sending: Arc<Semaphore>,
}

impl PackagesProtocol {
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            sending: Arc::new(Semaphore::new(MAX_SENDING)),
        }
    }

    /// The file `blake3`, if the latest manifest lists it.
    fn deliverable(&self, blake3: &str) -> anyhow::Result<Option<PathBuf>> {
        let Some(latest) = store::latest_manifest(&self.state.db())? else {
            return Ok(None);
        };
        let manifest: Manifest = serde_json::from_str(&latest.signed.payload)?;
        if !listed_files(&manifest).contains(blake3) {
            return Ok(None);
        }
        Ok(file_path(&self.state.packages_dir, blake3))
    }

    async fn answer(&self, send: &mut SendStream, recv: &mut RecvStream) -> anyhow::Result<()> {
        let Some(request) = control::read_frame::<_, Request>(recv).await? else {
            return Ok(());
        };
        match request {
            Request::Manifest => {
                let latest = store::latest_manifest(&self.state.db())?;
                let Some(latest) = latest else {
                    control::write_frame(send, &Answer::NotFound).await?;
                    return Ok(());
                };
                let body = serde_json::to_vec(&latest.signed)?;
                let size = body.len() as u64;
                control::write_frame(send, &Answer::Found { size }).await?;
                send.write_all(&body).await?;
            }
            Request::File { blake3 } => {
                let opened = match self.deliverable(&blake3)? {
                    Some(path) => tokio::fs::File::open(path).await.ok(),
                    None => None,
                };
                let Some(mut file) = opened else {
                    control::write_frame(send, &Answer::NotFound).await?;
                    return Ok(());
                };
                let _turn = self.sending.acquire().await?;
                let size = file.metadata().await?.len();
                control::write_frame(send, &Answer::Found { size }).await?;
                let mut buffer = vec![0; CHUNK];
                loop {
                    let read = file.read(&mut buffer).await?;
                    if read == 0 {
                        break;
                    }
                    send.write_all(&buffer[..read]).await?;
                }
            }
        }
        send.finish()?;
        Ok(())
    }

    async fn serve(&self, connection: &Connection) {
        loop {
            let stream = tokio::time::timeout(IDLE, connection.accept_bi()).await;
            let Ok(Ok((mut send, mut recv))) = stream else {
                return;
            };
            if let Err(error) = self.answer(&mut send, &mut recv).await {
                tracing::info!("package delivery: {error:#}");
                return;
            }
        }
    }
}

impl std::fmt::Debug for PackagesProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PackagesProtocol")
    }
}

impl ProtocolHandler for PackagesProtocol {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let key = connection.remote_id();
        let known = store::active_machine(&self.state.db(), &key);
        if matches!(known, Ok(Some(_))) {
            self.serve(&connection).await;
        } else {
            tracing::info!(key = %key.fmt_short(), "package delivery refused: unknown machine");
            connection.close(control::close::UNKNOWN_MACHINE.into(), b"unknown machine");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunks(
        parts: &[&'static [u8]],
    ) -> impl futures_util::Stream<Item = Result<axum::body::Bytes, std::io::Error>> {
        let parts: Vec<_> = parts
            .iter()
            .map(|part| Ok(axum::body::Bytes::from_static(part)))
            .collect();
        futures_util::stream::iter(parts)
    }

    #[tokio::test]
    async fn a_file_is_kept_under_its_hash() {
        let tmp = tempfile::tempdir().unwrap();
        let stored = receive(tmp.path(), chunks(&[b"hello ", b"world"]))
            .await
            .unwrap();
        assert_eq!(stored.size, 11);
        assert_eq!(
            stored.blake3,
            blake3::hash(b"hello world").to_hex().as_str()
        );
        let path = file_path(tmp.path(), &stored.blake3).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"hello world");
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn an_interrupted_upload_leaves_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let parts = vec![
            Ok(axum::body::Bytes::from_static(b"start")),
            Err(std::io::Error::other("cut")),
        ];
        let result = receive(tmp.path(), futures_util::stream::iter(parts)).await;
        assert!(matches!(result, Err(ReceiveError::Interrupted)));
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
    }

    #[test]
    fn only_a_hash_is_a_path() {
        let dir = Path::new("/store");
        assert!(file_path(dir, &"ab".repeat(32)).is_some());
        for bad in ["", "../../etc/passwd", &"AB".repeat(32), &"ab".repeat(33)] {
            assert_eq!(file_path(dir, bad), None, "{bad}");
        }
    }
}
