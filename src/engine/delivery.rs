use super::{retry::retry_operation, Engine, EngineError};
use amitoki_relay::{Delivery, RelayError};
use log::warn;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;

pub(super) async fn receive(engine: Arc<Engine>, shutdown: CancellationToken) -> Result<(), EngineError> {
    loop {
        let outcome = tokio::select! {
            _ = shutdown.cancelled() => return Ok(()),
            outcome = timeout(engine.config.operation_timeout(), engine.relay.receive(engine.config.batch_size)) => outcome
                .unwrap_or_else(|_| Err(RelayError::retryable("中継先からの取得がタイムアウトしました"))),
        };
        let wait = match outcome {
            Ok(deliveries) if deliveries.len() > engine.config.batch_size => return Err(EngineError::Worker("プラグインが受信上限を超えました".into())),
            Ok(deliveries) if !deliveries.is_empty() => {
                deliver(&engine, deliveries).await?;
                continue;
            },
            Ok(_) => Duration::from_millis(engine.config.poll_interval_ms),
            Err(error) if error.is_retryable() => {
                engine.metrics.retries.fetch_add(1, Ordering::Relaxed);
                warn!("中継先からの取得を再試行します: {error}");
                engine.config.retry_interval()
            },
            Err(error) => return Err(error.into()),
        };
        tokio::select! { _ = shutdown.cancelled() => return Ok(()), _ = sleep(wait) => {} }
    }
}

async fn deliver(engine: &Engine, deliveries: Vec<Delivery>) -> Result<(), EngineError> {
    let mut receipts = Vec::with_capacity(deliveries.len());
    let mut send_error = None;
    for delivery in deliveries {
        if engine.firewall.check_frame(&delivery.frame.bytes).is_err() {
            engine.metrics.rejected_deliveries.fetch_add(1, Ordering::Relaxed);
            receipts.push(delivery.receipt);
            continue;
        }
        let sent = timeout(engine.config.operation_timeout(), engine.network.send(&delivery.frame.bytes))
            .await
            .unwrap_or_else(|_| Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "NICへの送信がタイムアウトしました")));
        match sent {
            Ok(()) => {
                engine.metrics.injected.fetch_add(1, Ordering::Relaxed);
                receipts.push(delivery.receipt);
            },
            Err(error) => {
                send_error = Some(error);
                break;
            },
        }
    }
    // ACKだけの再試行ではNICへ再送信しない。失敗したフレーム以降は中継側に残す。
    if !receipts.is_empty() {
        retry_operation(|| engine.relay.acknowledge(&receipts), &engine.config, &engine.metrics).await?;
    }
    match send_error {
        Some(error) => Err(error.into()),
        None => Ok(()),
    }
}
