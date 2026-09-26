//! 設定を検証し、StageのスナップショットとRelayの接続を作る。
use super::{Graph, PipelineConfig, RunningBlock};
use crate::plugin_manager::{Package, PluginStore};
use amitoki_plugin_sdk::block::{BlockContext, BlockDefinition};
use amitoki_relay::{PluginRegistry, Relay, RelayContext};
use std::sync::Arc;

pub struct Connections {
    pub graph: Graph,
    pub blocks: Vec<RunningBlock>,
    pub relays: Vec<Arc<dyn Relay>>,
}

#[allow(async_fn_in_trait)]
pub trait PipelineConnections {
    fn check(&self, store: &PluginStore, context: &RelayContext) -> Result<Graph, Box<dyn std::error::Error>>;
    async fn connect(&self, providers: (&PluginStore, &PluginRegistry), context: RelayContext) -> Result<Connections, Box<dyn std::error::Error>>;
    async fn connect_blocks(&self, store: &PluginStore, context: &RelayContext) -> Result<Vec<RunningBlock>, Box<dyn std::error::Error>>;
}

impl PipelineConnections for PipelineConfig {
    fn check(&self, store: &PluginStore, context: &RelayContext) -> Result<Graph, Box<dyn std::error::Error>> {
        let mut definitions: Vec<BlockDefinition> = Vec::new();
        for block in &self.blocks {
            store.resolved_options(&block.plugin, &block.options)?;
            let package = Package::load(&store.plugin_path(&block.plugin)?)?;
            package.verify(&store.plugin_path(&block.plugin)?)?;
            definitions.push(package.manifest.block.ok_or("解析ブロックとして使えないプラグインです")?);
        }
        for relay in &self.relays {
            RelayContext {
                node_id: context.node_id.clone(),
                channel: relay.channel.clone().unwrap_or_else(|| context.channel.clone()),
            }
            .validate()?;
            if relay.plugin != "memory" {
                store.resolved_options(&relay.plugin, &relay.options)?;
                let package = Package::load(&store.plugin_path(&relay.plugin)?)?;
                package.verify(&store.plugin_path(&relay.plugin)?)?;
                if package.manifest.block.is_some() {
                    return Err("解析ブロックは中継として使えません".into());
                }
            }
        }
        Ok(Graph::compile(self, &definitions)?)
    }

    async fn connect(&self, providers: (&PluginStore, &PluginRegistry), context: RelayContext) -> Result<Connections, Box<dyn std::error::Error>> {
        let (store, registry) = providers;
        self.check(store, &context)?;
        let blocks = self.connect_blocks(store, &context).await?;
        let graph = Graph::compile(self, &blocks.iter().map(|block| block.definition.clone()).collect::<Vec<_>>())?;
        let mut relays = Vec::new();
        for instance in &self.relays {
            let context = RelayContext {
                node_id: context.node_id.clone(),
                channel: instance.channel.clone().unwrap_or_else(|| context.channel.clone()),
            };
            let relay = if registry.names().contains(&instance.plugin.as_str()) {
                registry.connect(&instance.plugin, context, instance.options.clone()).await?
            } else {
                store.connect(&instance.plugin, context, instance.options.clone()).await?
            };
            relays.push(relay);
        }
        Ok(Connections { graph, blocks, relays })
    }

    async fn connect_blocks(&self, store: &PluginStore, context: &RelayContext) -> Result<Vec<RunningBlock>, Box<dyn std::error::Error>> {
        let mut blocks = Vec::new();
        for instance in &self.blocks {
            let (block, manifest) = store
                .connect_stage(
                    &instance.plugin,
                    BlockContext {
                        relay: context.clone(),
                        instance: instance.id.clone(),
                    },
                    instance.options.clone(),
                )
                .await?;
            blocks.push(RunningBlock {
                block,
                definition: manifest.block.ok_or("Stageの定義がありません")?,
                on_error: instance.on_error,
            });
        }
        Ok(blocks)
    }
}
