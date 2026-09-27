use crate::{
    config::AppConfig,
    pipeline::{PipelineConnections, PipelineEngine, PreparedPipeline},
    plugin_manager::PluginStore,
};
use std::{path::PathBuf, sync::Arc};

pub struct ReloadSource {
    pub path: PathBuf,
    pub baseline: AppConfig,
    pub store: PluginStore,
}

pub struct Reloader {
    path: PathBuf,
    baseline: AppConfig,
    store: PluginStore,
    engine: Option<Arc<PipelineEngine>>,
}

impl Reloader {
    pub fn new(source: ReloadSource, engine: Option<Arc<PipelineEngine>>) -> Self {
        Self {
            path: source.path,
            baseline: source.baseline,
            store: source.store,
            engine,
        }
    }

    pub async fn reload(&self) -> Result<u64, String> {
        let engine = self.engine.as_ref().ok_or("reloadにはPipeline構成が必要です")?;
        let prepare = async {
            let path = self.path.clone();
            let next = tokio::task::spawn_blocking(move || AppConfig::load(&path).map_err(|error| error.to_string())).await.map_err(|error| error.to_string())??;
            self.check_unchanged(&next)?;
            let pipeline = next.pipeline.as_ref().ok_or("reloadではPipeline構成を維持してください")?;
            let check_pipeline = pipeline.clone();
            let check_store = self.store.clone();
            let context = next.context();
            tokio::task::spawn_blocking(move || check_pipeline.check(&check_store, &context).map(|_| ()).map_err(|error| error.to_string()))
                .await
                .map_err(|error| error.to_string())??;
            let blocks = pipeline.connect_blocks(&self.store, &next.context()).await.map_err(|error| error.to_string())?;
            PreparedPipeline::new(pipeline, blocks)
        };
        let prepared =
            tokio::time::timeout(self.baseline.engine.reload_timeout(), prepare).await.map_err(|_| "新しいStageの初期化がタイムアウトしました。旧Pipelineを継続します")??;
        engine.activate(prepared)
    }

    fn check_unchanged(&self, next: &AppConfig) -> Result<(), String> {
        let current = &self.baseline;
        if next.node_id != current.node_id
            || next.channel != current.channel
            || next.interface != current.interface
            || next.promiscuous != current.promiscuous
            || next.engine != current.engine
            || next.firewall != current.firewall
            || next.relay.is_some()
            || next.pipeline.as_ref().map(|pipeline| &pipeline.relays) != current.pipeline.as_ref().map(|pipeline| &pipeline.relays)
        {
            return Err("reloadではStageと経路だけを変更できます。NIC・Relay・本体設定の変更には再起動が必要です".into());
        }
        Ok(())
    }
}
