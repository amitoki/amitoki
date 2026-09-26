//! 配送計画の観測情報。デバッグ時だけ収集し、通常実行では保持しない。
use amitoki_plugin_sdk::block::BlockOutput;

pub(crate) struct BlockTrace {
    pub packet: usize,
    pub block: usize,
    pub elapsed_us: u64,
    pub output: Result<BlockOutput, String>,
}
