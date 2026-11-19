//! Adapters between files and tokio streams and the io traits of `bao-tree`.
//!
//! File reads and writes are positioned and synchronous: `bao-tree` asks for
//! at most one group of [`crate::BLOCK_SIZE`] at a time, which a local disk
//! serves in microseconds. Long work (hashing a whole file) goes to a
//! blocking thread instead, see [`crate::store`].

use std::fs::File;
use std::io;

use bytes::{Bytes, BytesMut};
use iroh_io::{AsyncSliceReader, AsyncSliceWriter, AsyncStreamReader, AsyncStreamWriter};
use positioned_io::{ReadAt, WriteAt};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// A file read and written at given offsets.
#[derive(Debug)]
pub(crate) struct FileSlice(pub File);

impl AsyncSliceReader for FileSlice {
    async fn read_at(&mut self, offset: u64, len: usize) -> io::Result<Bytes> {
        let mut buffer = vec![0; len];
        let mut filled = 0;
        while filled < len {
            let read = self
                .0
                .read_at(offset + filled as u64, &mut buffer[filled..])?;
            if read == 0 {
                break;
            }
            filled += read;
        }
        buffer.truncate(filled);
        Ok(buffer.into())
    }

    async fn size(&mut self) -> io::Result<u64> {
        Ok(self.0.metadata()?.len())
    }
}

impl AsyncSliceWriter for FileSlice {
    async fn write_at(&mut self, offset: u64, data: &[u8]) -> io::Result<()> {
        self.0.write_all_at(offset, data)
    }

    async fn write_bytes_at(&mut self, offset: u64, data: Bytes) -> io::Result<()> {
        self.0.write_all_at(offset, &data)
    }

    async fn set_len(&mut self, len: u64) -> io::Result<()> {
        self.0.set_len(len)
    }

    async fn sync(&mut self) -> io::Result<()> {
        self.0.sync_data()
    }
}

/// The receiving side of a connection.
pub(crate) struct StreamReader<'a, R>(pub &'a mut R);

impl<R: AsyncRead + Unpin> AsyncStreamReader for StreamReader<'_, R> {
    /// Fills `len` bytes unless the stream ends first.
    async fn read_bytes(&mut self, len: usize) -> io::Result<Bytes> {
        let mut buffer = BytesMut::zeroed(len);
        let mut filled = 0;
        while filled < len {
            let read = self.0.read(&mut buffer[filled..]).await?;
            if read == 0 {
                break;
            }
            filled += read;
        }
        buffer.truncate(filled);
        Ok(buffer.freeze())
    }

    async fn read<const L: usize>(&mut self) -> io::Result<[u8; L]> {
        let mut buffer = [0; L];
        self.0.read_exact(&mut buffer).await?;
        Ok(buffer)
    }
}

/// The sending side of a connection.
pub(crate) struct StreamWriter<'a, W>(pub &'a mut W);

impl<W: AsyncWrite + Unpin> AsyncStreamWriter for StreamWriter<'_, W> {
    async fn write(&mut self, data: &[u8]) -> io::Result<()> {
        self.0.write_all(data).await
    }

    async fn write_bytes(&mut self, data: Bytes) -> io::Result<()> {
        self.0.write_all(&data).await
    }

    async fn sync(&mut self) -> io::Result<()> {
        self.0.flush().await
    }
}
