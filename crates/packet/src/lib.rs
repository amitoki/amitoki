//! 通信形式のcodecをStageとテスト生成器で共有する。Rustのメモリ配置には依存しない。
pub mod telemetry;
use serde::{Deserialize, Serialize};

pub type PacketResult<T> = Result<T, String>;

pub trait PacketCodec: Sized {
    fn decode(bytes: &[u8]) -> PacketResult<Self>;
    fn encode(&self) -> PacketResult<Vec<u8>>;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PacketDefinition {
    pub name: String,
    pub description: String,
    pub config_schema: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerateRequest {
    pub name: String,
    pub seed: u64,
    pub start: u64,
    pub count: usize,
    pub options: serde_json::Value,
}

/// indexを含めることでバッチ分割や呼び出し順に依存しない生成を実装できる。
pub trait PacketGenerator {
    fn definition(&self) -> PacketDefinition;
    fn generate(&self, index: u64, seed: u64, options: &serde_json::Value) -> PacketResult<Vec<u8>>;
}
