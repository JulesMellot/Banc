//! BLAKE3 hashes, local store, verified transfer and source selection.
//!
//! A file is known by the BLAKE3 hash of its content. Next to it, the store
//! keeps its outboard: the inner hashes of the BLAKE3 tree, one pair per
//! group of [`BLOCK_SIZE`] bytes (about 0.4% of the file). With it, any
//! range of the file can be sent with the proof that it belongs to the hash,
//! and checked as it arrives. A transfer cut in the middle keeps what was
//! verified and asks only for the rest.
//!
//! The protocol is Banc's own (`banc/blobs/1`, see [`transfer`]), on top of
//! `bao-tree`, rather than `iroh-blobs`: the latter is still at 0.x and its
//! store needs `redb` and a multi-threaded runtime, which the agent does not
//! have.

pub mod hash;
mod io;
pub mod store;
pub mod transfer;

pub use hash::Hash;
pub use store::{Entry, Store, StoreError};

/// Size of the groups of chunks that the outboard keeps a hash for, and the
/// smallest unit a transfer verifies and resumes from: 16 KiB.
pub const BLOCK_SIZE: bao_tree::BlockSize = bao_tree::BlockSize::from_chunk_log(4);
