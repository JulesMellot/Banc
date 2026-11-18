//! The control channel: one iroh connection from each agent to the server.
//!
//! The agent opens a connection with [`ALPN`], then one bidirectional stream.
//! It sends [`AgentMessage::Hello`] and gets [`ServerMessage::Welcome`]. Then
//! a heartbeat every [`HEARTBEAT_INTERVAL`] and a report whenever the
//! inventory changes. The server only accepts connections from enrolled,
//! non-revoked machines: the iroh handshake proves the machine key.
//!
//! Each message is a frame: a big-endian `u32` length, then JSON.

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::machine::Inventory;
use crate::packages::WorkstationReport;

/// iroh protocol identifier of the control channel.
pub const ALPN: &[u8] = b"banc/control/1";

/// Section 10 of the plan: one heartbeat every 5 minutes at rest.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(300);

/// Largest frame accepted. Control messages are small; anything larger is
/// a bug or an attack.
pub const MAX_FRAME: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentMessage {
    /// First message of every connection.
    Hello {
        agent_version: String,
        inventory: Inventory,
    },
    Heartbeat,
    /// The inventory changed (Resolve installed or updated, for example).
    Report {
        inventory: Inventory,
    },
    /// Where the workstation stands against the manifest. Only sent to a
    /// server that announced packages in its welcome.
    Packages {
        report: WorkstationReport,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Welcome {
        machine_id: i64,
        /// Heartbeat interval asked by the server, in seconds.
        heartbeat_secs: u64,
        /// The server delivers manifests. An older server does not say so,
        /// and would close the connection on a package report.
        #[serde(default)]
        packages: bool,
    },
    /// A new manifest is available. Only a notice: the agent fetches the
    /// manifest and checks its signature.
    ManifestPublished { version: u64 },
}

/// Close codes of the control connection.
pub mod close {
    /// The machine is not enrolled, or has been revoked.
    pub const UNKNOWN_MACHINE: u32 = 1;
    /// The machine broke the protocol.
    pub const PROTOCOL: u32 = 2;
    /// The server is shutting down.
    pub const SHUTDOWN: u32 = 3;
    /// The admin revoked the machine during the connection.
    pub const REVOKED: u32 = 4;
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("connection error")]
    Io(#[from] std::io::Error),
    #[error("frame of {0} bytes, larger than the limit")]
    TooLarge(usize),
    #[error("malformed message")]
    Json(#[from] serde_json::Error),
}

/// Writes one message.
pub async fn write_frame<W, T>(writer: &mut W, message: &T) -> Result<(), FrameError>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let json = serde_json::to_vec(message)?;
    if json.len() > MAX_FRAME {
        return Err(FrameError::TooLarge(json.len()));
    }
    let len = u32::try_from(json.len()).map_err(|_| FrameError::TooLarge(json.len()))?;
    let mut frame = Vec::with_capacity(4 + json.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&json);
    writer.write_all(&frame).await?;
    writer.flush().await?;
    Ok(())
}

/// Reads one message. `None` when the stream ends cleanly between two frames.
pub async fn read_frame<R, T>(reader: &mut R) -> Result<Option<T>, FrameError>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut len = [0; 4];
    let mut filled = 0;
    while filled < len.len() {
        let read = reader.read(&mut len[filled..]).await?;
        if read == 0 {
            if filled == 0 {
                return Ok(None);
            }
            return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof).into());
        }
        filled += read;
    }
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(FrameError::TooLarge(len));
    }
    let mut json = vec![0; len];
    reader.read_exact(&mut json).await?;
    Ok(Some(serde_json::from_slice(&json)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frames_round_trip() {
        let messages = vec![
            AgentMessage::Hello {
                agent_version: "0.1.0".to_owned(),
                inventory: Inventory::default(),
            },
            AgentMessage::Heartbeat,
        ];
        let mut buffer = Vec::new();
        for message in &messages {
            write_frame(&mut buffer, message).await.unwrap();
        }
        let mut reader = buffer.as_slice();
        for message in &messages {
            let read: Option<AgentMessage> = read_frame(&mut reader).await.unwrap();
            assert_eq!(read.as_ref(), Some(message));
        }
        let end: Option<AgentMessage> = read_frame(&mut reader).await.unwrap();
        assert_eq!(end, None);
    }

    #[tokio::test]
    async fn message_json_is_tagged() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, &AgentMessage::Heartbeat)
            .await
            .unwrap();
        assert_eq!(&buffer[4..], br#"{"type":"heartbeat"}"#);
    }

    #[tokio::test]
    async fn truncated_frame_is_an_error() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, &AgentMessage::Heartbeat)
            .await
            .unwrap();
        for cut in [2, buffer.len() - 1] {
            let mut reader = &buffer[..cut];
            let result: Result<Option<AgentMessage>, _> = read_frame(&mut reader).await;
            assert!(matches!(result, Err(FrameError::Io(_))), "cut at {cut}");
        }
    }

    #[tokio::test]
    async fn oversized_frame_refused_before_reading_it() {
        let len = u32::try_from(MAX_FRAME + 1).unwrap();
        let buffer = len.to_be_bytes();
        let result: Result<Option<AgentMessage>, _> = read_frame(&mut &buffer[..]).await;
        assert!(matches!(result, Err(FrameError::TooLarge(_))));
    }

    #[tokio::test]
    async fn unknown_message_refused() {
        let json = br#"{"type":"run_command","command":"rm -rf /"}"#;
        let mut buffer = u32::try_from(json.len()).unwrap().to_be_bytes().to_vec();
        buffer.extend_from_slice(json);
        let result: Result<Option<AgentMessage>, _> = read_frame(&mut buffer.as_slice()).await;
        assert!(matches!(result, Err(FrameError::Json(_))));
    }
}
