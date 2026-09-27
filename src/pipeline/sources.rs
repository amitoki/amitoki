//! 各中継の取得は独立させ、受領バッチの処理が終わるまで次を取得しない。
use super::{generation::Generation, PipelineEngine};
use crate::engine::{retry::retry_operation, EngineError};
use amitoki_plugin_sdk::wire::MAX_BATCH;
use amitoki_relay::{Delivery, Frame, Receipt, RelayError, MAX_FRAME_SIZE};
use bytes::Bytes;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot},
    time::{sleep, timeout},
};
use tokio_util::sync::CancellationToken;

pub(super) enum Job {
    Captured {
        frame: Frame,
        generation: Arc<Generation>,
    },
    Received {
        source: usize,
        generation: Arc<Generation>,
        deliveries: Vec<Delivery>,
        completed: oneshot::Sender<ProcessedBatch>,
    },
}

pub(super) struct ProcessedBatch {
    pub receipts: Vec<Receipt>,
    pub failure: Option<std::io::Error>,
}

pub(super) async fn capture(engine: Arc<PipelineEngine>, sender: mpsc::Sender<Job>, shutdown: CancellationToken) -> Result<(), EngineError> {
    let mut buffer = vec![0; MAX_FRAME_SIZE];
    loop {
        let permit = tokio::select! {
            _ = shutdown.cancelled() => return Ok(()),
            permit = sender.reserve() => permit.map_err(|_| EngineError::Worker("入力キューが閉じました".into()))?,
        };
        let length = tokio::select! {
            _ = shutdown.cancelled() => return Ok(()),
            received = engine.network.receive(&mut buffer) => received?,
        };
        let bytes = buffer.get(..length).ok_or_else(|| EngineError::Worker("受信サイズがバッファを超えています".into()))?;
        if engine.settings.firewall.check_frame(bytes).is_err() {
            engine.metrics.filtered.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let frame = Frame::new(Bytes::copy_from_slice(bytes))?;
        engine.metrics.captured.fetch_add(1, Ordering::Relaxed);
        permit.send(Job::Captured {
            frame,
            generation: engine.generations.active(),
        });
    }
}

pub(super) async fn receive(source: (Arc<PipelineEngine>, usize), sender: mpsc::Sender<Job>, shutdown: CancellationToken) -> Result<(), EngineError> {
    let (engine, index) = source;
    match receive_batches((&engine, index), sender, shutdown.clone()).await {
        Err(EngineError::Relay(error)) => {
            engine.relay_metrics[index].failures.fetch_add(1, Ordering::Relaxed);
            log::error!("中継の受信を停止します: relay={index} error={error}");
            shutdown.cancelled().await;
            Ok(())
        },
        outcome => outcome,
    }
}

async fn receive_batches(source: (&PipelineEngine, usize), sender: mpsc::Sender<Job>, shutdown: CancellationToken) -> Result<(), EngineError> {
    let (engine, index) = source;
    let config = &engine.settings.config;
    let limit = config.batch_size.min(MAX_BATCH);
    loop {
        let response = tokio::select! {
            _ = shutdown.cancelled() => return Ok(()),
            response = timeout(config.operation_timeout(), engine.relays[index].receive(limit)) => response.unwrap_or_else(|_| Err(RelayError::retryable("中継からの取得がタイムアウトしました"))),
        };
        let wait = match response {
            Ok(deliveries) if deliveries.len() > limit => return Err(RelayError::permanent("中継が受信件数の上限を超えました").into()),
            Ok(deliveries) if !deliveries.is_empty() => {
                for delivery in &deliveries {
                    delivery.frame.validate()?;
                }
                let (completed, processed) = oneshot::channel();
                let permit = tokio::select! {
                    _ = shutdown.cancelled() => return Ok(()),
                    permit = sender.reserve() => permit.map_err(|_| EngineError::Worker("入力キューが閉じました".into()))?,
                };
                let generation = engine.generations.active();
                permit.send(Job::Received {
                    source: index,
                    deliveries,
                    completed,
                    generation: generation.clone(),
                });
                // 停止時も既に投入したバッチはACKまで処理する。上位が停止時間を制限する。
                let processed = processed.await.map_err(|_| EngineError::Worker("配送処理が停止しました".into()))?;
                if !processed.receipts.is_empty() {
                    retry_operation(|| engine.relays[index].acknowledge(&processed.receipts), config, &engine.metrics).await?;
                }
                drop(generation);
                if let Some(error) = processed.failure {
                    return Err(error.into());
                }
                continue;
            },
            Ok(_) => Duration::from_millis(config.poll_interval_ms),
            Err(error) if error.is_retryable() => {
                engine.metrics.retries.fetch_add(1, Ordering::Relaxed);
                config.retry_interval()
            },
            Err(error) => return Err(error.into()),
        };
        tokio::select! { _ = shutdown.cancelled() => return Ok(()), _ = sleep(wait) => {} }
    }
}
