use super::runner::{ReplayInput, ReplayPlan, ReplayReader};
use crate::pipeline::PipelineConnections;
use crate::{
    config::AppConfig,
    plugin_manager::{ManagerResult, PluginStore},
};
use std::path::Path;

pub(super) async fn replay(store: &PluginStore, path: &Path, input: ReplayInput) -> ManagerResult<()> {
    let config = AppConfig::load(path)?;
    let context = config.context();
    let pipeline = config.pipeline.ok_or("経路トレースにはpipeline設定を指定してください")?;
    pipeline.check(store, &context)?;
    let blocks = pipeline.connect_blocks(store, &context).await?;
    let graph = crate::pipeline::graph::Graph::compile(&pipeline, &blocks.iter().map(|block| block.definition.clone()).collect::<Vec<_>>())?;
    let entry = if input.source == "capture" {
        graph.capture.clone()
    } else {
        let index = pipeline
            .relays
            .iter()
            .position(|relay| input.source == format!("{}.received", relay.id))
            .ok_or("--sourceにはcaptureまたは中継インスタンス名.receivedを指定してください")?;
        graph.received[index].clone()
    };
    let reader = ReplayReader::open(input)?;
    ReplayPlan {
        graph,
        blocks,
        entry,
        block_names: pipeline.blocks.iter().map(|block| block.id.clone()).collect(),
        relay_names: pipeline.relays.iter().map(|relay| relay.id.clone()).collect(),
        firewall: config.firewall,
        operation_timeout: config.engine.operation_timeout(),
    }
    .run(reader)
    .await
}
