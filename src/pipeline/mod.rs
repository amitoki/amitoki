mod config;
mod connections;
pub use connections::{Connections, PipelineConnections};
mod executor;
mod generation;
pub mod graph;
mod history;
mod publication;
mod rewrite;
use generation::GenerationStore;
pub use generation::PreparedPipeline;
pub use publication::RelayMetrics;
pub(crate) mod plan;
mod sources;
pub(crate) mod trace;

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
    generations: GenerationStore,
    relays: Vec<Arc<dyn Relay>>,
    network: Arc<dyn PacketIo>,
    settings: EngineSettings,
    pub metrics: Metrics,
    pub pipeline_metrics: PipelineMetrics,
    pub relay_metrics: Vec<Arc<RelayMetrics>>,
}

impl PipelineEngine {
    pub fn new(connections: Connections, network: Arc<dyn PacketIo>, settings: EngineSettings) -> Result<Self, EngineError> {
        settings.config.validate().map_err(|error| EngineError::Worker(error.into()))?;
        Ok(Self {
            generations: GenerationStore::new(PreparedPipeline {
                graph: connections.graph,
                blocks: connections.blocks,
            }),
            relay_metrics: connections.relays.iter().map(|_| Arc::new(RelayMetrics::default())).collect(),
            relays: connections.relays,
            network,
            settings,
            metrics: Metrics::default(),
            pipeline_metrics: PipelineMetrics::default(),
        })
    }
    pub fn generation(&self) -> u64 {
        self.generations.active().number
    }
    pub fn activate(&self, prepared: PreparedPipeline) -> Result<u64, String> {
        self.generations.replace(prepared)
    }
    pub async fn run(self: Arc<Self>, shutdown: CancellationToken) -> Result<(), EngineError> {
        if shutdown.is_cancelled() {
            return Ok(());
        }
        let (sender, receiver) = mpsc::channel(self.settings.config.queue_capacity);
        let stopping = CancellationToken::new();
        let mut workers = JoinSet::new();
        let mut queues = Vec::new();
        for (index, metrics) in self.relay_metrics.iter().enumerate() {
            let (queue, receiver) = publication::RelayQueue::new(self.settings.config.relay_queue_capacity, metrics.clone());
            queues.push(queue);
            workers.spawn(publication::publish((self.clone(), index), receiver));
        }
        workers.spawn(sources::capture(self.clone(), sender.clone(), stopping.clone()));
        for index in 0..self.relays.len() {
            workers.spawn(sources::receive((self.clone(), index), sender.clone(), stopping.clone()));
        }
        drop(sender);
        workers.spawn(executor::execute(self.clone(), receiver, queues));
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
                let pending =
                    self.metrics.pending_publish_count() + self.relay_metrics.iter().map(|metrics| metrics.queued.load(std::sync::atomic::Ordering::Relaxed)).sum::<u64>();
                workers.abort_all();
                while workers.join_next().await.is_some() {}
                Err(EngineError::ShutdownTimeout(pending))
            },
        }
    }
}
