//! パケットを読み、接続先ポートと解析結果を返す。配送ID・ACK・NICは本体が管理する。
mod client;
mod server;
use amitoki_relay::{Frame, RelayContext, RelayError};
use async_trait::async_trait;
pub use client::ProcessBlock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub use server::serve_block;
use std::{collections::HashSet, sync::Arc};

// 分岐と解析結果のサイズを制限し、プラグインの出力で処理量を無制限に増やさない。
pub const MAX_OUTPUT_PORTS: usize = 8;
pub const MAX_ANNOTATION_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockDefinition {
    pub outputs: Vec<String>,
}
impl BlockDefinition {
    pub fn validate(&self) -> Result<(), RelayError> {
        let unique: HashSet<_> = self.outputs.iter().collect();
        if self.outputs.is_empty() || self.outputs.len() > MAX_OUTPUT_PORTS || unique.len() != self.outputs.len() || self.outputs.iter().any(|port| !valid_identifier(port)) {
            return Err(RelayError::permanent("ブロックの出力ポート定義が不正です"));
        }
        Ok(())
    }
}

pub use amitoki_pipeline::valid_identifier;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockContext {
    pub relay: RelayContext,
    pub instance: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockPacket {
    pub frame: Frame,
    pub annotations: Value,
}
impl BlockPacket {
    pub fn validate(&self) -> Result<(), RelayError> {
        self.frame.validate()?;
        validate_annotations(&self.annotations)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockOutput {
    /// 空なら破棄。複数指定は同じパケットの分岐。フレーム自体は書き換えられない。
    pub ports: Vec<String>,
    /// 次のブロックに渡すJSONオブジェクト。経路・ID・ACKには使わない。
    pub annotations: Value,
}
impl BlockOutput {
    pub fn validate(&self, definition: &BlockDefinition) -> Result<(), RelayError> {
        let unique: HashSet<_> = self.ports.iter().collect();
        if self.ports.len() > MAX_OUTPUT_PORTS || self.ports.len() != unique.len() || self.ports.iter().any(|port| !definition.outputs.contains(port)) {
            return Err(RelayError::permanent("ブロックが未定義または重複したポートを返しました"));
        }
        validate_annotations(&self.annotations)
    }
}

fn validate_annotations(annotations: &Value) -> Result<(), RelayError> {
    if !annotations.is_object() || serde_json::to_vec(annotations).map_err(|_| RelayError::permanent("解析結果を符号化できません"))?.len() > MAX_ANNOTATION_BYTES {
        return Err(RelayError::permanent("解析結果は4096バイト以内のJSONオブジェクトにしてください"));
    }
    Ok(())
}

#[async_trait]
pub trait Block: Send + Sync {
    async fn generate(&self, _request: &amitoki_packet::GenerateRequest) -> Result<Vec<Vec<u8>>, RelayError> {
        Err(RelayError::permanent("このStageはパケット生成に対応していません"))
    }
    /// 入力順に1件ずつ結果を返す。再試行は本体が行わず、配送の再試行でも再実行しない。
    async fn process(&self, packets: &[BlockPacket]) -> Result<Vec<BlockOutput>, RelayError>;
}
#[async_trait]
pub trait BlockPlugin: Send + Sync {
    fn generate(&self, _request: &amitoki_packet::GenerateRequest) -> Result<Vec<Vec<u8>>, RelayError> {
        Err(RelayError::permanent("このStageはパケット生成に対応していません"))
    }
    async fn connect(&self, context: BlockContext, options: Value) -> Result<Arc<dyn Block>, RelayError>;
}
