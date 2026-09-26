//! 各中継の取得は独立させ、受領バッチの処理が終わるまで次を取得しない。
use super::PipelineEngine;
use crate::{engine::EngineError, packet::parse_frame};
use amitoki_plugin_sdk::wire::MAX_BATCH;
use amitoki_relay::{Delivery, Frame, RelayError, MAX_FRAME_SIZE};
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
    Captured(Frame),
    Received {
        source: usize,
        deliveries: Vec<Delivery>,
        completed: oneshot::Sender<()>,
    },
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
        if !parse_frame(bytes).is_ok_and(|packet| engine.settings.firewall.allows(&packet)) {
            engine.metrics.filtered.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let frame = Frame::new(Bytes::copy_from_slice(bytes))?;
        engine.metrics.captured.fetch_add(1, Ordering::Relaxed);
        permit.send(Job::Captured(frame));
    }
}

pub(super) async fn receive(source: (Arc<PipelineEngine>, usize), sender: mpsc::Sender<Job>, shutdown: CancellationToken) -> Result<(), EngineError> {
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
                tokio::select! {
                    _ = shutdown.cancelled() => return Ok(()),
                    sent = sender.send(Job::Received { source: index, deliveries, completed }) => sent.map_err(|_| EngineError::Worker("入力キューが閉じました".into()))?,
                }
                // 停止時も既に投入したバッチはACKまで処理する。上位が停止時間を制限する。
                processed.await.map_err(|_| EngineError::Worker("配送処理が停止しました".into()))?;
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
