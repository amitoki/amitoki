//! 旧来の単一中継設定とブロック構成の起動・停止を同じCLIから扱う。
use crate::{
    engine::{Engine, EngineError, Metrics},
    pipeline::PipelineEngine,
};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub enum Runtime {
    Relay(Arc<Engine>),
    Pipeline(Arc<PipelineEngine>),
}
impl Runtime {
    pub async fn run(&self, shutdown: CancellationToken) -> Result<(), EngineError> {
        match self {
            Self::Relay(engine) => engine.clone().run(shutdown).await,
            Self::Pipeline(engine) => engine.clone().run(shutdown).await,
        }
    }
    pub fn metrics(&self) -> &Metrics {
        match self {
            Self::Relay(engine) => &engine.metrics,
            Self::Pipeline(engine) => &engine.metrics,
        }
    }
    pub fn report_pipeline(&self) {
        use std::sync::atomic::Ordering;
        if let Self::Pipeline(engine) = self {
            let metrics = &engine.pipeline_metrics;
            log::info!(
                "ブロック終了: process={}, failed_branch={}, duplicate={}",
                metrics.processed.load(Ordering::Relaxed),
                metrics.dropped_branches.load(Ordering::Relaxed),
                metrics.duplicates.load(Ordering::Relaxed)
            );
            for (index, relay) in engine.relay_metrics.iter().enumerate() {
                log::info!(
                    "中継終了: relay={index} publish={} drop={} queued={} peak_queued={} failure={}",
                    relay.published.load(Ordering::Relaxed),
                    relay.dropped.load(Ordering::Relaxed),
                    relay.queued.load(Ordering::Relaxed),
                    relay.peak_queued.load(Ordering::Relaxed),
                    relay.failures.load(Ordering::Relaxed)
                );
            }
        }
    }
}
