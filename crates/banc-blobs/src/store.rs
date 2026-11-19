//! The files this machine knows by hash.
//!
//! ```text
//! <root>/blobs/<hash>.json   where the file is, its size and date
//! <root>/blobs/<hash>.obao  its outboard, in pre-order
//! <root>/data/<hash>        a file the store holds itself (download, depot)
//! <root>/partial/<hash>.*   a download in progress: data, outboard and the
//!                           ranges already verified
//! ```
//!
//! A file imported from a project stays where it is: the store keeps only
//! its outboard and a record. If the file changes afterwards (size or date),
//! the record no longer counts and the file has to be imported again.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use bao_tree::io::outboard::PreOrderOutboard;
use bao_tree::{BaoTree, ChunkNum, ChunkRanges, TreeNode};
use bytes::Bytes;
use iroh_io::AsyncSliceWriter;
use serde::{Deserialize, Serialize};

use crate::io::FileSlice;
use crate::{Hash, BLOCK_SIZE};

/// How much a download receives between two saves of its progress. A
/// download cut off restarts from its last save.
const SAVE_EVERY: u64 = 64 * 1024 * 1024;

/// Read buffer when hashing a whole file.
const HASH_BUFFER: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("store: {0}")]
    Io(#[from] io::Error),
    #[error("the file changed while it was being hashed")]
    Changed,
    #[error("a store record is unreadable: {0}")]
    Record(#[from] serde_json::Error),
    #[error("this file is already being downloaded")]
    Busy,
    #[error("the data does not match the hash")]
    Mismatch,
}

/// A complete file, verified against its hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub hash: Hash,
    pub size: u64,
    pub path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct Record {
    size: u64,
    path: PathBuf,
    /// The file's date when it was hashed: a later date means it changed.
    modified: Option<SystemTime>,
}

/// What a download has verified so far.
#[derive(Debug, Serialize, Deserialize)]
struct Progress {
    size: u64,
    /// Boundaries of the verified ranges, in chunks of 1 KiB.
    verified: Vec<u64>,
}

#[derive(Debug)]
pub struct Store {
    root: PathBuf,
    /// Downloads in progress in this process: one at a time per file.
    busy: Arc<Mutex<HashSet<Hash>>>,
}

impl Store {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        for dir in ["blobs", "data", "partial"] {
            fs::create_dir_all(root.join(dir))?;
        }
        Ok(Self {
            root,
            busy: Arc::default(),
        })
    }

    fn path(&self, dir: &str, hash: Hash, extension: &str) -> PathBuf {
        let mut name = hash.to_string();
        if !extension.is_empty() {
            name.push('.');
            name.push_str(extension);
        }
        self.root.join(dir).join(name)
    }

    /// Hashes a file where it is and records it. The work happens on a
    /// blocking thread: a large file takes seconds.
    pub async fn import(&self, path: &Path) -> Result<Entry, StoreError> {
        let path = path.to_path_buf();
        let outboard =
            self.root
                .join("blobs")
                .join(format!(".import-{}-{}", std::process::id(), unique()));
        let job = {
            let (path, outboard) = (path.clone(), outboard.clone());
            tokio::task::spawn_blocking(move || hash_file(&path, &outboard))
        };
        let hashed = match job.await {
            Ok(hashed) => hashed,
            Err(error) => Err(io::Error::other(error).into()),
        };
        let (hash, size, modified) = match hashed {
            Ok(hashed) => hashed,
            Err(error) => {
                let _ = fs::remove_file(&outboard);
                return Err(error);
            }
        };
        fs::rename(&outboard, self.path("blobs", hash, "obao"))?;
        let record = Record {
            size,
            path: path.clone(),
            modified,
        };
        write_atomic(
            &self.path("blobs", hash, "json"),
            &serde_json::to_vec(&record)?,
        )?;
        Ok(Entry { hash, size, path })
    }

    /// The complete file with this hash, if it is still what was hashed.
    pub fn entry(&self, hash: Hash) -> Result<Option<Entry>, StoreError> {
        let record: Record = match fs::read(self.path("blobs", hash, "json")) {
            Ok(json) => serde_json::from_slice(&json)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let metadata = match fs::metadata(&record.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if metadata.len() != record.size || metadata.modified().ok() != record.modified {
            return Ok(None);
        }
        if !self.path("blobs", hash, "obao").exists() {
            return Ok(None);
        }
        Ok(Some(Entry {
            hash,
            size: record.size,
            path: record.path,
        }))
    }

    /// A complete file opened for sending, with its outboard.
    pub(crate) fn open_blob(
        &self,
        hash: Hash,
    ) -> Result<Option<(Entry, FileSlice, PreOrderOutboard<FileSlice>)>, StoreError> {
        let Some(entry) = self.entry(hash)? else {
            return Ok(None);
        };
        let data = File::open(&entry.path)?;
        let outboard = File::open(self.path("blobs", hash, "obao"))?;
        let outboard = PreOrderOutboard {
            root: hash.into(),
            tree: BaoTree::new(entry.size, BLOCK_SIZE),
            data: FileSlice(outboard),
        };
        Ok(Some((entry, FileSlice(data), outboard)))
    }

    /// What is still missing of this file here, in chunks of 1 KiB.
    pub fn missing(&self, hash: Hash, size: u64) -> Result<ChunkRanges, StoreError> {
        if self.entry(hash)?.is_some_and(|entry| entry.size == size) {
            return Ok(ChunkRanges::empty());
        }
        let verified = self.read_progress(hash, size)?;
        Ok(whole(size) - verified)
    }

    fn read_progress(&self, hash: Hash, size: u64) -> Result<ChunkRanges, StoreError> {
        let progress: Progress = match fs::read(self.path("partial", hash, "json")) {
            Ok(json) => serde_json::from_slice(&json)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(ChunkRanges::empty())
            }
            Err(error) => return Err(error.into()),
        };
        if progress.size != size {
            return Ok(ChunkRanges::empty());
        }
        Ok(from_boundaries(&progress.verified) & whole(size))
    }

    /// Starts or resumes the download of a file.
    pub(crate) fn begin(&self, hash: Hash, size: u64) -> Result<Partial, StoreError> {
        let guard = Busy::take(&self.busy, hash)?;
        // Progress without its files (deleted by hand) means starting over.
        let files = ["data", "obao"].map(|extension| self.path("partial", hash, extension));
        let verified = if files.iter().all(|file| file.exists()) {
            self.read_progress(hash, size)?
        } else {
            ChunkRanges::empty()
        };
        let fresh = verified.is_empty();
        let open = |path: PathBuf| {
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(fresh)
                .open(path)
        };
        let [data, outboard] = files;
        let data = open(data)?;
        let outboard = open(outboard)?;
        Ok(Partial {
            root: self.root.clone(),
            hash,
            size,
            data: FileSlice(data),
            outboard: PreOrderOutboard {
                root: hash.into(),
                tree: BaoTree::new(size, BLOCK_SIZE),
                data: FileSlice(outboard),
            },
            verified,
            unsaved: 0,
            _guard: guard,
        })
    }
}

/// A download in progress. Only verified data is written to it.
pub(crate) struct Partial {
    root: PathBuf,
    hash: Hash,
    size: u64,
    data: FileSlice,
    outboard: PreOrderOutboard<FileSlice>,
    verified: ChunkRanges,
    unsaved: u64,
    _guard: Busy,
}

impl Partial {
    pub(crate) fn tree(&self) -> BaoTree {
        self.outboard.tree
    }

    pub(crate) fn missing(&self) -> ChunkRanges {
        whole(self.size) - &self.verified
    }

    pub(crate) async fn parent(
        &mut self,
        node: TreeNode,
        pair: &(blake3::Hash, blake3::Hash),
    ) -> io::Result<()> {
        use bao_tree::io::fsm::OutboardMut;
        self.outboard.save(node, pair).await
    }

    /// Writes a verified piece of the file.
    pub(crate) async fn leaf(&mut self, offset: u64, data: Bytes) -> Result<(), StoreError> {
        let end = offset + data.len() as u64;
        self.data.write_bytes_at(offset, data).await?;
        self.verified |= ChunkRanges::from(ChunkNum(offset / 1024)..ChunkNum::chunks(end));
        self.unsaved += end - offset;
        if self.unsaved >= SAVE_EVERY {
            self.save().await?;
        }
        Ok(())
    }

    /// Makes what was received so far survive a cut: the data reaches the
    /// disk before the record that says it is there.
    pub(crate) async fn save(&mut self) -> Result<(), StoreError> {
        use bao_tree::io::fsm::OutboardMut;
        self.data.sync().await?;
        self.outboard.sync().await?;
        let progress = Progress {
            size: self.size,
            verified: boundaries(&self.verified),
        };
        write_atomic(
            &self.file("partial", "json"),
            &serde_json::to_vec(&progress)?,
        )?;
        self.unsaved = 0;
        Ok(())
    }

    fn file(&self, dir: &str, extension: &str) -> PathBuf {
        let mut name = self.hash.to_string();
        if !extension.is_empty() {
            name.push('.');
            name.push_str(extension);
        }
        self.root.join(dir).join(name)
    }

    /// Moves the complete file into the store.
    pub(crate) async fn finish(mut self) -> Result<Entry, StoreError> {
        if !self.missing().is_empty() {
            return Err(StoreError::Mismatch);
        }
        // Nothing was received for an empty file: only the hash can tell.
        if self.size == 0 && self.hash != Hash::of(&[]) {
            return Err(StoreError::Mismatch);
        }
        self.save().await?;
        let path = self.file("data", "");
        fs::rename(self.file("partial", "data"), &path)?;
        fs::rename(self.file("partial", "obao"), self.file("blobs", "obao"))?;
        let record = Record {
            size: self.size,
            path: path.clone(),
            modified: fs::metadata(&path)?.modified().ok(),
        };
        write_atomic(&self.file("blobs", "json"), &serde_json::to_vec(&record)?)?;
        fs::remove_file(self.file("partial", "json"))?;
        Ok(Entry {
            hash: self.hash,
            size: self.size,
            path,
        })
    }
}

/// Marks a file as being downloaded until dropped.
struct Busy {
    set: Arc<Mutex<HashSet<Hash>>>,
    hash: Hash,
}

impl Busy {
    fn take(set: &Arc<Mutex<HashSet<Hash>>>, hash: Hash) -> Result<Self, StoreError> {
        let mut busy = set.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if !busy.insert(hash) {
            return Err(StoreError::Busy);
        }
        Ok(Self {
            set: set.clone(),
            hash,
        })
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        let mut busy = self
            .set
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        busy.remove(&self.hash);
    }
}

/// Every chunk of a file of this size.
fn whole(size: u64) -> ChunkRanges {
    ChunkRanges::from(ChunkNum(0)..ChunkNum::chunks(size))
}

pub(crate) fn boundaries(ranges: &ChunkRanges) -> Vec<u64> {
    ranges.boundaries().iter().map(|chunk| chunk.0).collect()
}

/// Ranges from their boundaries. Anything out of order is dropped rather
/// than trusted: the result is always a valid set.
pub(crate) fn from_boundaries(boundaries: &[u64]) -> ChunkRanges {
    let mut ranges = ChunkRanges::empty();
    for pair in boundaries.chunks(2) {
        match *pair {
            [start, end] if start < end => {
                ranges |= ChunkRanges::from(ChunkNum(start)..ChunkNum(end));
            }
            [start] => ranges |= ChunkRanges::from(ChunkNum(start)..),
            _ => {}
        }
    }
    ranges
}

/// Hashes a file and writes its outboard. Fails if the file changes on the
/// way, since the hash would then describe neither version.
fn hash_file(path: &Path, outboard: &Path) -> Result<(Hash, u64, Option<SystemTime>), StoreError> {
    let file = File::open(path)?;
    let before = file.metadata()?;
    let size = before.len();
    let tree = BaoTree::new(size, BLOCK_SIZE);
    let mut target = PreOrderOutboard {
        root: blake3::hash(&[]),
        tree,
        data: File::create(outboard)?,
    };
    let reader = BufReader::with_capacity(HASH_BUFFER, file);
    let hash = bao_tree::io::sync::outboard(reader, tree, &mut target)?;
    target.data.sync_all()?;
    let after = fs::metadata(path)?;
    if after.len() != size || after.modified().ok() != before.modified().ok() {
        return Err(StoreError::Changed);
    }
    Ok((hash.into(), size, before.modified().ok()))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", unique()));
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })
}

/// A name no other write of this process uses at the same time.
fn unique() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_imported_file_is_known_by_its_hash() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).unwrap();
        let path = dir.path().join("logo.png");
        let content = vec![7; 100_000];
        fs::write(&path, &content).unwrap();

        let entry = store.import(&path).await.unwrap();
        assert_eq!(entry.hash, Hash::of(&content));
        assert_eq!(entry.size, 100_000);
        assert_eq!(store.entry(entry.hash).unwrap(), Some(entry.clone()));
        assert!(store.missing(entry.hash, 100_000).unwrap().is_empty());
        // The outboard keeps one pair of hashes per group of 16 KiB.
        let outboard = fs::metadata(store.path("blobs", entry.hash, "obao")).unwrap();
        assert_eq!(
            outboard.len(),
            BaoTree::new(100_000, BLOCK_SIZE).outboard_size()
        );
    }

    #[tokio::test]
    async fn a_file_changed_after_its_import_no_longer_counts() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("store")).unwrap();
        let path = dir.path().join("music.wav");
        fs::write(&path, b"first version").unwrap();
        let entry = store.import(&path).await.unwrap();

        fs::write(&path, b"second version, longer").unwrap();
        assert_eq!(store.entry(entry.hash).unwrap(), None);
        fs::remove_file(&path).unwrap();
        assert_eq!(store.entry(entry.hash).unwrap(), None);
    }

    #[tokio::test]
    async fn an_unknown_file_is_missing_whole() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let missing = store.missing(Hash::of(b"x"), 3000).unwrap();
        assert_eq!(boundaries(&missing), vec![0, 3]);
    }

    #[test]
    fn a_file_is_downloaded_once_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let hash = Hash::of(b"x");
        let first = store.begin(hash, 1).unwrap();
        assert!(matches!(store.begin(hash, 1), Err(StoreError::Busy)));
        drop(first);
        assert!(store.begin(hash, 1).is_ok());
    }

    #[test]
    fn boundaries_from_the_network_always_make_a_valid_set() {
        assert_eq!(boundaries(&from_boundaries(&[0, 4, 8])), vec![0, 4, 8]);
        assert_eq!(boundaries(&from_boundaries(&[4, 2, 6, 9])), vec![6, 9]);
        assert_eq!(
            boundaries(&from_boundaries(&[6, 9, 0, 2])),
            vec![0, 2, 6, 9]
        );
        assert!(from_boundaries(&[]).is_empty());
    }
}
