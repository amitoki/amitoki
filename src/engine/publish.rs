use super::{retry::retry_operation, Engine, EngineError};
use amitoki_relay::Frame;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::{sync::mpsc, time::sleep};

pub(super) async fn publish(engine: Arc<Engine>, mut receiver: mpsc::Receiver<Frame>) -> Result<(), EngineError> {
    let mut batch = Vec::with_capacity(engine.config.batch_size);
    while let Some(frame) = receiver.recv().await {
        batch.push(frame);
        let flush = sleep(Duration::from_millis(engine.config.flush_interval_ms));
        tokio::pin!(flush);
        while batch.len() < engine.config.batch_size {
            tokio::select! {
                _ = &mut flush => break,
                frame = receiver.recv() => match frame {
                    Some(frame) => batch.push(frame),
                    None => break,
                },
            }
        }
        // 成功するまでは同じIDとバッファを保持する。タイムアウト時の部分成功も重複させない。
        retry_operation(|| engine.relay.publish(&batch), &engine.config, &engine.metrics).await?;
        engine.metrics.published.fetch_add(batch.len() as u64, Ordering::Relaxed);
        batch.clear();
    }
    Ok(())
}
