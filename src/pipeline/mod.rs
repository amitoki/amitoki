mod config;
mod connections;
pub use connections::Connections;
mod executor;
pub mod graph;
mod history;
mod plan;
mod sources;

use crate::{
    engine::{worker_failure, EngineError, EngineSettings, Metrics},
    network::PacketIo,
};
use amitoki_relay::Relay;
pub use config::{ErrorPolicy, PipelineConfig};
use graph::Graph;
pub use plan::{PipelineMetrics, RunningBlock};
use std::{sync::Arc, time::Duration};
use tokio::{sync::mpsc, task::JoinSet, time::timeout};
use tokio_util::sync::CancellationToken;

pub struct PipelineEngine {
    graph: Graph,
    blocks: Vec<RunningBlock>,
    relays: Vec<Arc<dyn Relay>>,
    network: Arc<dyn PacketIo>,
    settings: EngineSettings,
    pub metrics: Metrics,
    pub pipeline_metrics: PipelineMetrics,
}

impl PipelineEngine {
    pub fn new(connections: Connections, network: Arc<dyn PacketIo>, settings: EngineSettings) -> Result<Self, EngineError> {
        settings.config.validate().map_err(|error| EngineError::Worker(error.into()))?;
        Ok(Self {
            graph: connections.graph,
            blocks: connections.blocks,
            relays: connections.relays,
            network,
            settings,
            metrics: Metrics::default(),
            pipeline_metrics: PipelineMetrics::default(),
        })
    }
    pub async fn run(self: Arc<Self>, shutdown: CancellationToken) -> Result<(), EngineError> {
        if shutdown.is_cancelled() {
            return Ok(());
        }
        let (sender, receiver) = mpsc::channel(self.settings.config.queue_capacity);
        let stopping = CancellationToken::new();
        let mut workers = JoinSet::new();
        workers.spawn(sources::capture(self.clone(), sender.clone(), stopping.clone()));
        for index in 0..self.relays.len() {
            workers.spawn(sources::receive((self.clone(), index), sender.clone(), stopping.clone()));
        }
        drop(sender);
        workers.spawn(executor::execute(self.clone(), receiver));
        let failure = tokio::select! {
            _ = shutdown.cancelled() => None,
            completed = workers.join_next() => Some(worker_failure(completed)),
        };
        stopping.cancel();
        let drain = async {
            let mut failure = failure;
            while let Some(completed) = workers.join_next().await {
                if !matches!(&completed, Ok(Ok(()))) && failure.is_none() {
                    failure = Some(worker_failure(Some(completed)));
                }
            }
            failure.map_or(Ok(()), Err)
        };
        match timeout(Duration::from_millis(self.settings.config.shutdown_timeout_ms), drain).await {
            Ok(outcome) => outcome,
            Err(_) => {
                workers.abort_all();
                while workers.join_next().await.is_some() {}
                Err(EngineError::ShutdownTimeout(self.metrics.pending_publish_count()))
            },
        }
    }
}
