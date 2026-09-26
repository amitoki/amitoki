use crate::{engine::EngineConfig, firewall::Firewall};
use amitoki_relay::RelayContext;
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub node_id: String,
    pub channel: String,
    pub interface: String,
    #[serde(default)]
    pub promiscuous: bool,
    pub relay: Option<RelayConfig>,
    pub pipeline: Option<crate::pipeline::PipelineConfig>,
    #[serde(default)]
    pub engine: EngineConfig,
    #[serde(default)]
    pub firewall: Firewall,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelayConfig {
    pub plugin: String,
    #[serde(default = "empty_options")]
    pub options: Value,
}

fn empty_options() -> Value {
    serde_json::json!({})
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let text = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&text).map_err(|error| format!("設定ファイルが不正です: {}", error.message()))?;
        if config.relay.is_some() == config.pipeline.is_some() {
            return Err("relayとpipelineのどちらか一方を指定してください".into());
        }
        if let Some(pipeline) = &config.pipeline {
            pipeline.validate()?;
        }
        config.context().validate()?;
        config.engine.validate()?;
        if config.interface.is_empty() || config.interface.as_bytes().contains(&0) {
            return Err("interfaceを指定してください".into());
        }
        Ok(config)
    }

    pub fn context(&self) -> RelayContext {
        RelayContext {
            node_id: self.node_id.clone(),
            channel: self.channel.clone(),
        }
    }
}
