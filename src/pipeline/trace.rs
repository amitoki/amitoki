//! 配送計画の観測情報。デバッグ時だけ収集し、通常実行では保持しない。
use amitoki_plugin_sdk::block::{BlockOutput, BlockPacket};

pub(crate) struct BlockTrace {
    pub packet: usize,
    pub block: usize,
    pub elapsed_us: u64,
    pub input: BlockPacket,
    pub output: Result<BlockOutput, String>,
}
