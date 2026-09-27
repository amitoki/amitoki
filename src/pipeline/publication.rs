//! 中継ごとの有界キューで、停止した配送先の待機を他の配送先から分離する。
use super::{generation::Generation, PipelineEngine};
use crate::engine::{retry::retry_operation, EngineError};
use amitoki_relay::Frame;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::sync::{mpsc, OwnedSemaphorePermit, Semaphore};

#[derive(Default)]
pub struct RelayMetrics {
    pub published: AtomicU64,
    pub dropped: AtomicU64,
    pub queued: AtomicU64,
    pub peak_queued: AtomicU64,
    pub failures: AtomicU64,
}

pub(super) struct Publication {
    frames: Vec<Frame>,
    _generation: Arc<Generation>,
    _permit: OwnedSemaphorePermit,
    metrics: Arc<RelayMetrics>,
}
impl Drop for Publication {
    fn drop(&mut self) {
        self.metrics.queued.fetch_sub(self.frames.len() as u64, Ordering::Relaxed);
    }
}

pub(super) struct RelayQueue {
    sender: mpsc::Sender<Publication>,
    slots: Arc<Semaphore>,
    capacity: usize,
    metrics: Arc<RelayMetrics>,
}
impl RelayQueue {
    pub fn new(capacity: usize, metrics: Arc<RelayMetrics>) -> (Self, mpsc::Receiver<Publication>) {
        let (sender, receiver) = mpsc::channel(capacity);
        (
            Self {
                sender,
                slots: Arc::new(Semaphore::new(capacity)),
                capacity,
                metrics,
            },
            receiver,
        )
    }

    pub fn enqueue(&self, frames: &[Frame], generation: &Arc<Generation>) {
        // バッチ数ではなく、処理中の分も含むフレーム数でメモリを制限する。
        for frames in frames.chunks(self.capacity) {
            let count = frames.len() as u64;
            let Ok(permit) = self.slots.clone().try_acquire_many_owned(count as u32) else {
                self.metrics.dropped.fetch_add(count, Ordering::Relaxed);
                continue;
            };
            let queued = self.metrics.queued.fetch_add(count, Ordering::Relaxed) + count;
            self.metrics.peak_queued.fetch_max(queued, Ordering::Relaxed);
            let publication = Publication {
                frames: frames.to_vec(),
                _generation: generation.clone(),
                _permit: permit,
                metrics: self.metrics.clone(),
            };
            if self.sender.try_send(publication).is_err() {
                self.metrics.dropped.fetch_add(count, Ordering::Relaxed);
            }
        }
    }
}

pub(super) async fn publish(source: (Arc<PipelineEngine>, usize), mut receiver: mpsc::Receiver<Publication>) -> Result<(), EngineError> {
    let (engine, index) = source;
    let metrics = &engine.relay_metrics[index];
    let mut failed = false;
    while let Some(publication) = receiver.recv().await {
        let count = publication.frames.len() as u64;
        if !failed {
            match retry_operation(|| engine.relays[index].publish(&publication.frames), &engine.settings.config, &engine.metrics).await {
                Ok(()) => {
                    metrics.published.fetch_add(count, Ordering::Relaxed);
                    continue;
                },
                Err(error) => {
                    // 壊れたIPCセッションへ再試行せず、この送信先だけを停止する。
                    metrics.failures.fetch_add(1, Ordering::Relaxed);
                    log::error!("中継の送信を停止します: relay={index} error={error}");
                    failed = true;
                },
            }
        }
        metrics.dropped.fetch_add(count, Ordering::Relaxed);
    }
    Ok(())
}
