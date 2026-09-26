use super::{
    runner::{ReplayInput, ReplayPlan, ReplayReader},
    BlockTestArguments,
};
use crate::{
    engine::EngineConfig,
    firewall::{Firewall, Policy},
    pipeline::{
        graph::{Destination, Graph},
        ErrorPolicy, RunningBlock,
    },
    plugin_manager::{
        configure::apply_assignments,
        source::{expand_path, PluginTarget},
        ManagerResult, PluginKind, PluginStore,
    },
};
use amitoki_plugin_sdk::block::{valid_identifier, BlockContext};
use amitoki_relay::RelayContext;

pub async fn test_block(store: &PluginStore, arguments: BlockTestArguments) -> ManagerResult<()> {
    let reader = ReplayReader::open(ReplayInput {
        pcap: expand_path(&arguments.pcap)?,
        source: arguments.instance.clone(),
        json: arguments.json,
    })?;
    let target = PluginTarget::parse(&arguments.target)?;
    let temporary = tempfile::tempdir()?;
    let local = PluginStore {
        directory: temporary.path().join("plugins"),
    };
    let (store, package) = match target {
        PluginTarget::Directory(path) => {
            let package = crate::plugin_manager::Package::load(&path)?;
            PluginKind::Block.check(&package)?;
            let package = local.install(&path, false)?;
            (&local, package)
        },
        target => (store, target.installed(store, Some(PluginKind::Block))?),
    };
    let mut options = store.options(&package.manifest.name)?;
    apply_assignments(
        options.as_object_mut().ok_or("設定はJSONオブジェクトで指定してください")?,
        &package.manifest.config_schema,
        &arguments.assignments,
    )?;
    let context = BlockContext {
        relay: RelayContext {
            node_id: arguments.node_id,
            channel: arguments.channel,
        },
        instance: arguments.instance,
    };
    context.relay.validate()?;
    if !valid_identifier(&context.instance) {
        return Err("instanceが不正です".into());
    }
    let block_name = context.instance.clone();
    let definition = package.manifest.block.clone().ok_or("ブロックの定義がありません")?;
    let block = store.connect_block(&package.manifest.name, context, options).await?;
    // 単体テストでは各ポートを観測用の終端へ接続し、通常のプランナーで検証する。
    let outputs = definition.outputs.iter().enumerate().map(|(index, port)| (port.clone(), vec![Destination::Relay(index)])).collect();
    let graph = Graph {
        capture: vec![Destination::Block(0)],
        received: vec![vec![]; definition.outputs.len()],
        outputs: vec![outputs],
        order: vec![0],
    };
    let replay = ReplayPlan {
        relay_names: definition.outputs.iter().map(|port| format!("output:{port}")).collect(),
        entry: graph.capture.clone(),
        graph,
        blocks: vec![RunningBlock {
            block,
            definition,
            on_error: ErrorPolicy::Stop,
        }],
        block_names: vec![block_name],
        firewall: Firewall {
            policy: Policy::Blacklist,
            rules: vec![],
        },
        operation_timeout: EngineConfig::default().operation_timeout(),
    };
    replay.run(reader).await
}
