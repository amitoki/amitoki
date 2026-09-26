//! 設定を検証し、使用中ロックを持つプラグイン接続を作る。
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

impl PipelineConfig {
    pub fn check(&self, store: &PluginStore, context: &RelayContext) -> Result<Graph, Box<dyn std::error::Error>> {
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

    pub async fn connect(&self, providers: (&PluginStore, &PluginRegistry), context: RelayContext) -> Result<Connections, Box<dyn std::error::Error>> {
        let (store, registry) = providers;
        let graph = self.check(store, &context)?;
        let blocks = self.connect_blocks(store, &context).await?;
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

    pub(crate) async fn connect_blocks(&self, store: &PluginStore, context: &RelayContext) -> Result<Vec<RunningBlock>, Box<dyn std::error::Error>> {
        let mut blocks = Vec::new();
        for instance in &self.blocks {
            let definition = Package::load(&store.plugin_path(&instance.plugin)?)?.manifest.block.ok_or("ブロックの定義がありません")?;
            let block = store
                .connect_block(
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
                definition,
                on_error: instance.on_error,
            });
        }
        Ok(blocks)
    }
}
