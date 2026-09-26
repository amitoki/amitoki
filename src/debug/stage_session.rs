use super::{runner::ReplayPlan, BlockTestArguments};
use crate::{
    engine::EngineConfig,
    firewall::{Firewall, Policy},
    pipeline::{
        graph::{Destination, Graph},
        ErrorPolicy, RunningBlock,
    },
    plugin_manager::{configure::apply_assignments, source::PluginTarget, ManagerResult, Package, PluginKind, PluginStore},
};
use amitoki_plugin_sdk::{
    block::{valid_identifier, Block, BlockContext, BlockDefinition},
    PluginManifest,
};
use amitoki_relay::RelayContext;
use std::sync::Arc;

pub(super) struct StageSession {
    pub block: Arc<dyn Block>,
    pub manifest: PluginManifest,
    definition: BlockDefinition,
    instance: String,
    _directory: tempfile::TempDir,
}

impl StageSession {
    pub async fn open(store: &PluginStore, arguments: &BlockTestArguments, target: &str) -> ManagerResult<Self> {
        let use_assignments = target == arguments.target;
        let target = PluginTarget::parse(target)?;
        let directory = tempfile::tempdir()?;
        let local = PluginStore {
            directory: directory.path().join("plugins"),
        };
        let (store, package) = match target {
            PluginTarget::Directory(path) => {
                PluginKind::Block.check(&Package::load(&path)?)?;
                let package = local.install(&path, false)?;
                (&local, package)
            },
            target => (store, target.installed(store, Some(PluginKind::Block))?),
        };
        let mut options = store.options(&package.manifest.name)?;
        // 別の生成器に、テスト対象Stage固有の設定を渡さない。
        if use_assignments {
            apply_assignments(
                options.as_object_mut().ok_or("設定はJSONオブジェクトで指定してください")?,
                &package.manifest.config_schema,
                &arguments.assignments,
            )?;
        }
        let context = BlockContext {
            relay: RelayContext {
                node_id: arguments.node_id.clone(),
                channel: arguments.channel.clone(),
            },
            instance: arguments.instance.clone(),
        };
        context.relay.validate()?;
        if !valid_identifier(&context.instance) {
            return Err("instanceが不正です".into());
        }
        let (block, manifest) = store.connect_stage(&package.manifest.name, context, options).await?;
        let definition = manifest.block.clone().ok_or("Stageの定義がありません")?;
        Ok(Self {
            block,
            definition,
            manifest,
            instance: arguments.instance.clone(),
            _directory: directory,
        })
    }

    pub fn plan(&self) -> ReplayPlan {
        let outputs = self.definition.outputs.iter().enumerate().map(|(index, port)| (port.clone(), vec![Destination::Relay(index)])).collect();
        let graph = Graph {
            capture: vec![Destination::Block(0)],
            received: vec![vec![]; self.definition.outputs.len()],
            outputs: vec![outputs],
            order: vec![0],
        };
        ReplayPlan {
            relay_names: self.definition.outputs.iter().map(|port| format!("output:{port}")).collect(),
            entry: graph.capture.clone(),
            graph,
            blocks: vec![RunningBlock {
                block: self.block.clone(),
                definition: self.definition.clone(),
                on_error: ErrorPolicy::Stop,
            }],
            block_names: vec![self.instance.clone()],
            firewall: Firewall {
                policy: Policy::Blacklist,
                rules: vec![],
            },
            operation_timeout: EngineConfig::default().operation_timeout(),
        }
    }
}
