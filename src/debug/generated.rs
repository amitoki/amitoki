use super::{stage_session::StageSession, BlockTestArguments};
use crate::{
    engine::EngineConfig,
    plugin_manager::{configure::apply_assignments, ManagerResult, PluginStore},
};
use amitoki_plugin_sdk::{block::Block, packet::GenerateRequest};
use std::{sync::Arc, time::Duration};

pub(super) struct GeneratedInput {
    block: Arc<dyn Block>,
    request: GenerateRequest,
    timeout: Duration,
    _session: Option<StageSession>,
    pub provider: String,
    pub version: String,
}

impl GeneratedInput {
    pub async fn open(store: &PluginStore, arguments: &BlockTestArguments, target: &StageSession) -> ManagerResult<Self> {
        let session = match &arguments.generator {
            Some(generator) => Some(StageSession::open(store, arguments, generator).await?),
            None => None,
        };
        let source = session.as_ref().unwrap_or(target);
        let name = arguments.packet.as_ref().ok_or("--packetでパケット定義を指定してください")?;
        let definition =
            source.manifest.packets.iter().find(|definition| &definition.name == name).ok_or("指定したパケット定義がありません（plugin stage describeで確認してください）")?;
        let mut options = serde_json::Map::new();
        apply_assignments(&mut options, &definition.config_schema, &arguments.packet_assignments)?;
        Ok(Self {
            block: source.block.clone(),
            provider: source.manifest.name.clone(),
            version: source.manifest.version.clone(),
            request: GenerateRequest {
                name: name.clone(),
                seed: arguments.seed,
                start: 0,
                count: 1,
                options: options.into(),
            },
            timeout: EngineConfig::default().operation_timeout(),
            _session: session,
        })
    }

    pub async fn generate(&self, start: u64, count: usize) -> ManagerResult<Vec<Vec<u8>>> {
        let request = GenerateRequest {
            start,
            count,
            ..self.request.clone()
        };
        Ok(tokio::time::timeout(self.timeout, self.block.generate(&request)).await??)
    }
}
