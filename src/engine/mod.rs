mod capture;
mod config;
mod delivery;
mod metrics;
mod publish;
mod retry;

use crate::{firewall::Firewall, network::PacketIo};
use amitoki_relay::{Relay, RelayError};
pub use config::EngineConfig;
pub use metrics::Metrics;
use std::{sync::Arc, time::Duration};
use tokio::{sync::mpsc, task::JoinSet, time::timeout};
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Relay(#[from] RelayError),
    #[error(transparent)]
    Network(#[from] std::io::Error),
    #[error("ワーカーが停止しました: {0}")]
    Worker(String),
    #[error("停止処理がタイムアウトしました。未送信フレーム: {0}")]
    ShutdownTimeout(u64),
}

pub struct Engine {
    relay: Arc<dyn Relay>,
    network: Arc<dyn PacketIo>,
    firewall: Firewall,
    config: EngineConfig,
    pub metrics: Metrics,
}

impl Engine {
    pub fn new(relay: Arc<dyn Relay>, network: Arc<dyn PacketIo>, settings: EngineSettings) -> Result<Self, EngineError> {
        settings.config.validate().map_err(|error| EngineError::Worker(error.into()))?;
        Ok(Self {
            relay,
            network,
            firewall: settings.firewall,
            config: settings.config,
            metrics: Metrics::default(),
        })
    }

    pub async fn run(self: Arc<Self>, shutdown: CancellationToken) -> Result<(), EngineError> {
        if shutdown.is_cancelled() {
            return Ok(());
        }
        let (sender, receiver) = mpsc::channel(self.config.queue_capacity);
        let workers_shutdown = CancellationToken::new();
        let mut workers = JoinSet::new();
        workers.spawn(capture::capture(self.clone(), sender, workers_shutdown.clone()));
        workers.spawn(publish::publish(self.clone(), receiver));
        workers.spawn(delivery::receive(self.clone(), workers_shutdown.clone()));
        let failure = tokio::select! {
            _ = shutdown.cancelled() => None,
            completed = workers.join_next() => Some(worker_failure(completed)),
        };
        workers_shutdown.cancel();
        let drain = async {
            let mut failure = failure;
            while let Some(completed) = workers.join_next().await {
                if !matches!(&completed, Ok(Ok(()))) && failure.is_none() {
                    failure = Some(worker_failure(Some(completed)));
                }
            }
            failure.map_or(Ok(()), Err)
        };
        match timeout(Duration::from_millis(self.config.shutdown_timeout_ms), drain).await {
            Ok(outcome) => outcome,
            Err(_) => {
                workers.abort_all();
                while workers.join_next().await.is_some() {}
                Err(EngineError::ShutdownTimeout(self.metrics.pending_publish_count()))
            },
        }
    }
}

pub struct EngineSettings {
    pub firewall: Firewall,
    pub config: EngineConfig,
}

fn worker_failure(completed: Option<Result<Result<(), EngineError>, tokio::task::JoinError>>) -> EngineError {
    match completed {
        Some(Ok(Err(error))) => error,
        Some(Err(error)) => EngineError::Worker(error.to_string()),
        _ => EngineError::Worker("予期しない正常終了".into()),
    }
}
