//! Verified transfer of a file between two machines.
//!
//! The receiver opens a stream with [`ALPN`] and sends a [`Request`]: the
//! hash, the size, and the ranges it still lacks. The sender answers with an
//! [`Answer`] frame, then the requested ranges in BLAKE3 verified streaming
//! order: each group of 16 KiB arrives with the hashes that tie it to the
//! file's hash, and is checked before it is written. Nothing the other
//! machine sends is trusted.
//!
//! The functions work on any tokio stream; the agent and the server give
//! them the two halves of an iroh stream.

use banc_proto::control::{read_frame, write_frame, FrameError};
use bao_tree::io::fsm::{encode_ranges_validated, ResponseDecoder, ResponseDecoderNext};
use bao_tree::io::{BaoContentItem, DecodeError, EncodeError, Leaf, Parent};
use bao_tree::ChunkRanges;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

use crate::io::{StreamReader, StreamWriter};
use crate::store::{boundaries, from_boundaries, Partial};
use crate::{Entry, Hash, Store, StoreError};

/// iroh protocol identifier of file transfers.
pub const ALPN: &[u8] = b"banc/blobs/1";

/// Most range boundaries in one request. A download whose missing ranges
/// are more scattered asks for everything from the first gap on.
pub const MAX_BOUNDARIES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub hash: Hash,
    /// The size the receiver expects. The hash covers it: a wrong size fails
    /// verification at the first piece.
    pub size: u64,
    /// Boundaries of the wanted ranges, in chunks of 1 KiB. An odd count
    /// leaves the last range open to the end.
    pub ranges: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Answer {
    /// The ranges follow.
    Found,
    NotFound,
}

#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("connection: {0}")]
    Frame(#[from] FrameError),
    #[error("connection: {0}")]
    Io(#[from] std::io::Error),
    #[error("the other machine does not have this file")]
    NotFound,
    #[error("the other machine sent data that does not match the hash")]
    Corrupt,
    #[error("the connection ended before the file was complete")]
    Incomplete,
    #[error("malformed request")]
    BadRequest,
    /// The copy here no longer matches its hash: it has to be checked, and
    /// fetched again from elsewhere.
    #[error("the local copy of {0} does not match its hash")]
    LocalCorrupt(Hash),
}

/// How a request was answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Served {
    /// The stream closed without a request.
    Nothing,
    Sent {
        hash: Hash,
    },
    NotFound {
        hash: Hash,
    },
}

/// Answers one request read from `recv`.
pub async fn serve<R, W>(store: &Store, recv: &mut R, send: &mut W) -> Result<Served, TransferError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let Some(request) = read_frame::<_, Request>(recv).await? else {
        return Ok(Served::Nothing);
    };
    if request.ranges.len() > MAX_BOUNDARIES {
        return Err(TransferError::BadRequest);
    }
    let hash = request.hash;
    let blob = store
        .open_blob(hash)?
        .filter(|(entry, _, _)| entry.size == request.size);
    let Some((_, data, outboard)) = blob else {
        write_frame(send, &Answer::NotFound).await?;
        send.shutdown().await?;
        return Ok(Served::NotFound { hash });
    };
    write_frame(send, &Answer::Found).await?;
    let ranges = from_boundaries(&request.ranges);
    // Each piece is checked against the outboard before it leaves: a file
    // damaged on this disk is reported here, not spread.
    let sent = encode_ranges_validated(data, outboard, &ranges, StreamWriter(send))
        .await
        .map_err(|error| match error {
            EncodeError::ParentHashMismatch(_)
            | EncodeError::LeafHashMismatch(_)
            | EncodeError::SizeMismatch => TransferError::LocalCorrupt(hash),
            EncodeError::Io(error) => TransferError::Io(error),
            other => TransferError::Io(std::io::Error::other(other.to_string())),
        });
    // Closed in every case, so that the receiver stops waiting.
    let closed = send.shutdown().await;
    sent?;
    closed?;
    Ok(Served::Sent { hash })
}

/// Fetches a file into the store, or what is missing of it. What arrives
/// verified is kept even if the transfer fails, and a later call asks only
/// for the rest.
pub async fn download<R, W>(
    store: &Store,
    hash: Hash,
    size: u64,
    send: &mut W,
    recv: &mut R,
) -> Result<Entry, TransferError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    if let Some(entry) = store.entry(hash)?.filter(|entry| entry.size == size) {
        return Ok(entry);
    }
    let mut partial = store.begin(hash, size)?;
    let missing = partial.missing();
    if missing.is_empty() {
        // One request per stream, and none is needed.
        send.shutdown().await?;
    } else {
        let request = Request {
            hash,
            size,
            ranges: request_ranges(&missing),
        };
        write_frame(send, &request).await?;
        send.shutdown().await?;
        match read_frame::<_, Answer>(recv).await? {
            Some(Answer::Found) => {}
            Some(Answer::NotFound) => return Err(TransferError::NotFound),
            None => return Err(TransferError::Incomplete),
        }
        let wanted = from_boundaries(&request.ranges);
        let received = receive(&mut partial, hash, wanted, recv).await;
        partial.save().await?;
        received?;
        if !partial.missing().is_empty() {
            return Err(TransferError::Incomplete);
        }
    }
    Ok(partial.finish().await?)
}

/// The boundaries to ask for. Scattered gaps (a source that kept failing
/// in places) are merged into one range rather than a huge request.
fn request_ranges(missing: &ChunkRanges) -> Vec<u64> {
    let all = boundaries(missing);
    if all.len() <= MAX_BOUNDARIES {
        return all;
    }
    all.first().map(|first| vec![*first]).unwrap_or_default()
}

async fn receive<R>(
    partial: &mut Partial,
    hash: Hash,
    wanted: ChunkRanges,
    recv: &mut R,
) -> Result<(), TransferError>
where
    R: AsyncRead + Unpin,
{
    let mut decoder = ResponseDecoder::new(hash.into(), wanted, partial.tree(), StreamReader(recv));
    loop {
        let (next, item) = match decoder.next().await {
            ResponseDecoderNext::Done(_) => return Ok(()),
            ResponseDecoderNext::More(more) => more,
        };
        decoder = next;
        match item.map_err(decode_error)? {
            BaoContentItem::Parent(Parent { node, pair }) => partial.parent(node, &pair).await?,
            BaoContentItem::Leaf(Leaf { offset, data }) => partial.leaf(offset, data).await?,
        }
    }
}

fn decode_error(error: DecodeError) -> TransferError {
    match error {
        DecodeError::ParentHashMismatch(_) | DecodeError::LeafHashMismatch(_) => {
            TransferError::Corrupt
        }
        DecodeError::ParentNotFound(_) | DecodeError::LeafNotFound(_) => TransferError::Incomplete,
        DecodeError::Io(error) => TransferError::Io(error),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use bao_tree::ChunkNum;

    use super::*;

    /// The chunk that holds byte `offset`.
    fn chunk_of(offset: u64) -> ChunkNum {
        ChunkNum(offset / 1024)
    }

    /// Content that differs from one 16 KiB group to the next.
    fn content(size: usize) -> Vec<u8> {
        (0..size).map(|i| (i * 31 + i / 997) as u8).collect()
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        sender: Store,
        receiver: Store,
        source: std::path::PathBuf,
    }

    async fn fixture(data: &[u8]) -> (Fixture, Entry) {
        let dir = tempfile::tempdir().unwrap();
        let sender = Store::open(dir.path().join("sender")).unwrap();
        let receiver = Store::open(dir.path().join("receiver")).unwrap();
        let source = dir.path().join("A001.braw");
        fs::write(&source, data).unwrap();
        let entry = sender.import(&source).await.unwrap();
        let fixture = Fixture {
            _dir: dir,
            sender,
            receiver,
            source,
        };
        (fixture, entry)
    }

    /// One download from `sender` to `receiver` over an in-memory pipe.
    async fn transfer(
        sender: &Store,
        receiver: &Store,
        hash: Hash,
        size: u64,
    ) -> (Result<Entry, TransferError>, Result<Served, TransferError>) {
        let (mut here, mut there) = tokio::io::duplex(64 * 1024);
        let (mut here_recv, mut here_send) = tokio::io::split(&mut here);
        let (mut there_recv, mut there_send) = tokio::io::split(&mut there);
        tokio::join!(
            download(receiver, hash, size, &mut here_send, &mut here_recv),
            serve(sender, &mut there_recv, &mut there_send),
        )
    }

    /// What `sender` answers to `request`, as raw bytes.
    async fn answer(sender: &Store, request: &Request) -> Vec<u8> {
        let mut framed = Vec::new();
        write_frame(&mut framed, request).await.unwrap();
        let mut out = Vec::new();
        serve(sender, &mut framed.as_slice(), &mut out)
            .await
            .unwrap();
        out
    }

    fn request_all(entry: &Entry) -> Request {
        Request {
            hash: entry.hash,
            size: entry.size,
            ranges: vec![0],
        }
    }

    #[tokio::test]
    async fn a_file_arrives_whole_and_verified() {
        let data = content(1_000_000 + 123);
        let (f, entry) = fixture(&data).await;

        let (received, served) = transfer(&f.sender, &f.receiver, entry.hash, entry.size).await;
        let received = received.unwrap();
        assert_eq!(served.unwrap(), Served::Sent { hash: entry.hash });
        assert_eq!(received.hash, entry.hash);
        assert_eq!(fs::read(&received.path).unwrap(), data);
        assert_eq!(f.receiver.entry(entry.hash).unwrap(), Some(received));
        assert!(f
            .receiver
            .missing(entry.hash, entry.size)
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn the_receiver_serves_what_it_received() {
        let data = content(200_000);
        let (f, entry) = fixture(&data).await;
        transfer(&f.sender, &f.receiver, entry.hash, entry.size)
            .await
            .0
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let third = Store::open(dir.path()).unwrap();
        let (received, _) = transfer(&f.receiver, &third, entry.hash, entry.size).await;
        assert_eq!(fs::read(received.unwrap().path).unwrap(), data);
    }

    #[tokio::test]
    async fn an_empty_file_is_checked_by_its_hash() {
        let (f, entry) = fixture(&[]).await;
        let (received, _) = transfer(&f.sender, &f.receiver, entry.hash, 0).await;
        assert_eq!(received.unwrap().size, 0);

        let dir = tempfile::tempdir().unwrap();
        let other = Store::open(dir.path()).unwrap();
        let mut sink = tokio::io::sink();
        let wrong = download(
            &other,
            Hash::of(b"x"),
            0,
            &mut sink,
            &mut tokio::io::empty(),
        )
        .await;
        assert!(matches!(
            wrong,
            Err(TransferError::Store(StoreError::Mismatch))
        ));
    }

    #[tokio::test]
    async fn a_cut_transfer_resumes_where_it_stopped() {
        let data = content(3 * 1024 * 1024 + 5);
        let (f, entry) = fixture(&data).await;
        let full = answer(&f.sender, &request_all(&entry)).await;

        // The connection drops at 40%.
        let cut = &full[..full.len() * 2 / 5];
        let mut sink = tokio::io::sink();
        let first = download(
            &f.receiver,
            entry.hash,
            entry.size,
            &mut sink,
            &mut &cut[..],
        )
        .await;
        assert!(matches!(first, Err(TransferError::Incomplete)));
        let missing = f.receiver.missing(entry.hash, entry.size).unwrap();
        let total = ChunkNum::chunks(entry.size).0;
        let left: u64 = boundaries(&missing).chunks(2).map(|r| r[1] - r[0]).sum();
        assert!(
            left > 0 && left < total * 7 / 10,
            "{left} of {total} chunks left"
        );

        // The second attempt asks only for the rest.
        let (received, _) = transfer(&f.sender, &f.receiver, entry.hash, entry.size).await;
        assert_eq!(fs::read(received.unwrap().path).unwrap(), data);
    }

    #[tokio::test]
    async fn corrupted_data_is_refused_and_not_kept() {
        let data = content(300_000);
        let (f, entry) = fixture(&data).await;
        let mut full = answer(&f.sender, &request_all(&entry)).await;
        // One byte changed near the end of the stream, inside the data.
        let at = full.len() - 1000;
        full[at] ^= 0xff;

        let mut sink = tokio::io::sink();
        let result = download(
            &f.receiver,
            entry.hash,
            entry.size,
            &mut sink,
            &mut full.as_slice(),
        )
        .await;
        assert!(matches!(result, Err(TransferError::Corrupt)));
        assert_eq!(f.receiver.entry(entry.hash).unwrap(), None);
        // Everything before the damaged piece was kept; the piece was not.
        let missing = f.receiver.missing(entry.hash, entry.size).unwrap();
        assert!(!missing.is_empty());
        assert!(!missing.contains(&ChunkNum(0)));
        assert!(missing.contains(&chunk_of(entry.size - 1)));
    }

    #[tokio::test]
    async fn a_wrong_size_is_refused() {
        let data = content(100_000);
        let (f, entry) = fixture(&data).await;
        let (received, served) = transfer(&f.sender, &f.receiver, entry.hash, 99_000).await;
        assert!(matches!(received, Err(TransferError::NotFound)));
        assert_eq!(served.unwrap(), Served::NotFound { hash: entry.hash });
    }

    #[tokio::test]
    async fn an_unknown_file_is_not_found() {
        let (f, _) = fixture(b"x").await;
        let hash = Hash::of(b"y");
        let (received, served) = transfer(&f.sender, &f.receiver, hash, 1).await;
        assert!(matches!(received, Err(TransferError::NotFound)));
        assert_eq!(served.unwrap(), Served::NotFound { hash });
        assert!(f.receiver.entry(hash).unwrap().is_none());
    }

    #[tokio::test]
    async fn a_file_damaged_on_the_senders_disk_is_not_sent() {
        let data = content(100_000);
        let (f, entry) = fixture(&data).await;
        // Same size, same date: only the content says it changed.
        let modified = fs::metadata(&f.source).unwrap().modified().unwrap();
        let mut damaged = data.clone();
        damaged[50_000] ^= 1;
        fs::write(&f.source, &damaged).unwrap();
        fs::File::options()
            .write(true)
            .open(&f.source)
            .unwrap()
            .set_modified(modified)
            .unwrap();

        let (received, served) = transfer(&f.sender, &f.receiver, entry.hash, entry.size).await;
        assert!(matches!(served, Err(TransferError::LocalCorrupt(h)) if h == entry.hash));
        assert!(received.is_err());
        assert_eq!(f.receiver.entry(entry.hash).unwrap(), None);
    }

    #[tokio::test]
    async fn too_many_ranges_are_refused() {
        let (f, entry) = fixture(b"x").await;
        let request = Request {
            hash: entry.hash,
            size: 1,
            ranges: (0..=MAX_BOUNDARIES as u64).collect(),
        };
        let mut framed = Vec::new();
        write_frame(&mut framed, &request).await.unwrap();
        let mut out = Vec::new();
        let served = serve(&f.sender, &mut framed.as_slice(), &mut out).await;
        assert!(matches!(served, Err(TransferError::BadRequest)));
    }

    #[test]
    fn scattered_gaps_become_one_request() {
        let few = from_boundaries(&[2, 4, 8, 9]);
        assert_eq!(request_ranges(&few), vec![2, 4, 8, 9]);
        let many: Vec<u64> = (0..2 * MAX_BOUNDARIES as u64).map(|i| 10 + 2 * i).collect();
        assert_eq!(request_ranges(&from_boundaries(&many)), vec![10]);
    }
}
