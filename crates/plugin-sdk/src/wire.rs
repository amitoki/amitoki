use crate::PluginManifest;
use amitoki_relay::{Delivery, Frame, Receipt, RelayContext, RelayError};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

// 最大Ethernetフレーム128件とメタデータを、際限なく確保せずに扱う。
pub const MAX_BATCH: usize = 128;
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    Describe,
    Connect {
        protocol_version: u32,
        context: RelayContext,
        options: Value,
    },
    ConnectBlock {
        protocol_version: u32,
        context: crate::block::BlockContext,
        options: Value,
    },
    Process {
        packets: Vec<crate::block::BlockPacket>,
    },
    Publish {
        frames: Vec<Frame>,
    },
    Receive {
        limit: usize,
    },
    Acknowledge {
        receipts: Vec<Receipt>,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Response {
    Manifest(PluginManifest),
    Success,
    Processed(Vec<crate::block::BlockOutput>),
    Deliveries(Vec<Delivery>),
    Error {
        message: String,
        retryable: bool,
    },
}

impl From<RelayError> for Response {
    fn from(error: RelayError) -> Self {
        Self::Error {
            message: error.to_string(),
            retryable: error.is_retryable(),
        }
    }
}

pub async fn read_message<T: DeserializeOwned>(reader: &mut (impl AsyncRead + Unpin)) -> io::Result<T> {
    let length = reader.read_u32().await? as usize;
    if length == 0 || length > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "通信メッセージが上限を超えています"));
    }
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload).await?;
    rmp_serde::from_slice(&payload).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "通信メッセージを解釈できません"))
}

pub async fn write_message<T: Serialize>(writer: &mut (impl AsyncWrite + Unpin), message: &T) -> io::Result<()> {
    let payload = rmp_serde::to_vec_named(message).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "通信メッセージを符号化できません"))?;
    if payload.len() > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "通信メッセージが上限を超えています"));
    }
    writer.write_u32(payload.len() as u32).await?;
    writer.write_all(&payload).await?;
    writer.flush().await
}
