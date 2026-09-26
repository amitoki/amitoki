use super::{EngineConfig, Metrics};
use amitoki_relay::RelayError;
use log::warn;
use std::{future::Future, sync::atomic::Ordering};
use tokio::time::{sleep, timeout};

pub(super) async fn retry_operation<F, Operation>(operation: F, config: &EngineConfig, metrics: &Metrics) -> Result<(), RelayError>
where
    F: Fn() -> Operation,
    Operation: Future<Output = Result<(), RelayError>>,
{
    loop {
        let outcome = timeout(config.operation_timeout(), operation()).await.unwrap_or_else(|_| Err(RelayError::retryable("中継処理がタイムアウトしました")));
        match outcome {
            Ok(()) => return Ok(()),
            Err(error) if error.is_retryable() => {
                metrics.retries.fetch_add(1, Ordering::Relaxed);
                warn!("中継処理を再試行します: {error}");
                sleep(config.retry_interval()).await;
            },
            Err(error) => return Err(error),
        }
    }
}
