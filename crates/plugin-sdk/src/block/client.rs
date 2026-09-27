use super::{Block, BlockContext, BlockDefinition, BlockOutput, BlockPacket};
use crate::{
    process::ProcessClient,
    wire::{Request, Response, MAX_BATCH},
    PluginManifest, PROTOCOL_VERSION,
};
use amitoki_relay::RelayError;
use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;

pub struct ProcessBlock {
    process: ProcessClient,
    definition: BlockDefinition,
    manifest: PluginManifest,
}
impl ProcessBlock {
    pub async fn connect(executable: &Path, manifest: &PluginManifest, configuration: (BlockContext, Value)) -> Result<Self, RelayError> {
        let (context, options) = configuration;
        manifest.validate_options(&options)?;
        let definition = manifest.block.clone().ok_or_else(|| RelayError::permanent("中継プラグインは解析ブロックとして使えません"))?;
        let process = ProcessClient::start(executable, manifest).await?;
        process
            .success(Request::ConnectBlock {
                protocol_version: PROTOCOL_VERSION,
                context,
                options,
            })
            .await?;
        Ok(Self {
            process,
            definition,
            manifest: manifest.clone(),
        })
    }
}
#[async_trait]
impl Block for ProcessBlock {
    async fn generate(&self, request: &amitoki_packet::GenerateRequest) -> Result<Vec<Vec<u8>>, RelayError> {
        crate::generation::validate_request(&self.manifest, request)?;
        match self.process.call(Request::Generate(request.clone())).await? {
            Response::Generated(packets) => {
                crate::generation::validate_packets(request, &packets)?;
                Ok(packets)
            },
            _ => Err(RelayError::permanent("パケット生成の応答が不正です")),
        }
    }
    async fn process(&self, packets: &[BlockPacket]) -> Result<Vec<BlockOutput>, RelayError> {
        if packets.len() > MAX_BATCH {
            return Err(RelayError::permanent("解析バッチの上限を超えています"));
        }
        for packet in packets {
            packet.validate()?;
        }
        match self.process.call(Request::Process { packets: packets.to_vec() }).await? {
            Response::Processed(outputs) if outputs.len() == packets.len() => {
                for output in &outputs {
                    output.validate(&self.definition)?;
                }
                Ok(outputs)
            },
            _ => Err(RelayError::permanent("ブロックの応答件数または種類が不正です")),
        }
    }
}
