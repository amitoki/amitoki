use amitoki_plugin_sdk::block::valid_identifier;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;

// 接続数とグラフ展開数の上限はプラグインから変更できない。
pub const MAX_RELAYS: usize = 16;
pub const MAX_BLOCKS: usize = 32;
pub const MAX_ROUTES: usize = 128;
pub const MAX_VISITS: usize = 128;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineConfig {
    #[serde(default)]
    pub relays: Vec<RelayInstance>,
    #[serde(default)]
    pub blocks: Vec<BlockInstance>,
    pub routes: Vec<Route>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelayInstance {
    pub id: String,
    pub plugin: String,
    /// 同じプラグインを別のchannelで利用する場合に指定する。
    pub channel: Option<String>,
    #[serde(default = "empty_options")]
    pub options: Value,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockInstance {
    pub id: String,
    pub plugin: String,
    #[serde(default = "empty_options")]
    pub options: Value,
    #[serde(default)]
    pub on_error: ErrorPolicy,
}
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorPolicy {
    #[default]
    Stop,
    DropBranch,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub from: String,
    pub to: Vec<String>,
}
fn empty_options() -> Value {
    serde_json::json!({})
}

impl PipelineConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.relays.len() > MAX_RELAYS || self.blocks.len() > MAX_BLOCKS || self.routes.len() > MAX_ROUTES {
            return Err("中継16個・ブロック32個・経路128個の上限を超えています".into());
        }
        let mut names = HashSet::new();
        for id in self.relays.iter().map(|relay| &relay.id).chain(self.blocks.iter().map(|block| &block.id)) {
            if !valid_identifier(id) || matches!(id.as_str(), "capture" | "inject") || !names.insert(id) {
                return Err(format!("インスタンス名が不正・予約済み・重複しています: {id}"));
            }
        }
        let mut sources = HashSet::new();
        for route in &self.routes {
            let destinations: HashSet<_> = route.to.iter().collect();
            if !sources.insert(&route.from) || destinations.len() != route.to.len() || route.to.len() > MAX_VISITS {
                return Err(format!("経路の重複または分岐上限超過: {}", route.from));
            }
            for destination in &route.to {
                if destination != "inject" && !names.contains(destination) {
                    return Err(format!("接続先がありません: {destination}"));
                }
            }
        }
        Ok(())
    }
}
