use crate::{engine::EngineConfig, firewall::Firewall};
use amitoki_relay::RelayContext;
use serde::Deserialize;
use serde_json::Value;
use std::{
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub node_id: String,
    pub channel: String,
    pub interface: String,
    #[serde(default)]
    pub promiscuous: bool,
    pub relay: Option<RelayConfig>,
    pub pipeline: Option<crate::pipeline::PipelineConfig>,
    pub pipeline_file: Option<PathBuf>,
    #[serde(default)]
    pub engine: EngineConfig,
    #[serde(default)]
    pub firewall: Firewall,
}

#[derive(Clone, Deserialize)]
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
        let mut config: Self = toml::from_str(&text).map_err(|error| format!("設定ファイルが不正です: {}", error.message()))?;
        if let Some(file) = &config.pipeline_file {
            if config.pipeline.is_some() || config.relay.is_some() {
                return Err("pipeline_fileとpipeline/relayは同時に指定できません".into());
            }
            let file = crate::plugin_manager::source::expand_path(file)?;
            let file = if file.is_absolute() { file } else { path.parent().unwrap_or(Path::new(".")).join(file) };
            // 巨大な生成物によるメモリ確保を、逆シリアライズより前に制限する。
            const MAX_PLAN_BYTES: u64 = 1024 * 1024;
            let mut bytes = Vec::new();
            std::fs::File::open(file)?.take(MAX_PLAN_BYTES + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_PLAN_BYTES {
                return Err("Pipeline定義は1MiB以内にしてください".into());
            }
            config.pipeline = Some(serde_json::from_slice(&bytes).map_err(|_| "Pipeline定義のJSONが不正です")?);
        }
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
