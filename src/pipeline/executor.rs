use super::{
    generation::Generation,
    publication::RelayQueue,
    sources::{Job, ProcessedBatch},
    PipelineEngine,
};
use crate::engine::EngineError;
use amitoki_plugin_sdk::wire::MAX_BATCH;
use amitoki_relay::Delivery;
use std::{
    collections::{HashMap, HashSet},
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::{
    sync::mpsc,
    time::{sleep, timeout},
};

use super::history::DeliveryHistory;

pub(super) async fn execute(engine: Arc<PipelineEngine>, mut receiver: mpsc::Receiver<Job>, queues: Vec<RelayQueue>) -> Result<(), EngineError> {
    let mut pending = None;
    let mut recent = DeliveryHistory::default();
    loop {
        let job = match pending.take() {
            Some(job) => job,
            None => match receiver.recv().await {
                Some(job) => job,
                None => return Ok(()),
            },
        };
        match job {
            Job::Captured { frame, generation } => {
                let mut frames = vec![frame];
                let flush = sleep(Duration::from_millis(engine.settings.config.flush_interval_ms));
                tokio::pin!(flush);
                while frames.len() < engine.settings.config.batch_size.min(MAX_BATCH) {
                    let next = tokio::select! { _ = &mut flush => break, next = receiver.recv() => next };
                    match next {
                        Some(Job::Captured {
                            frame,
                            generation: next_generation,
                        }) if Arc::ptr_eq(&generation, &next_generation) => frames.push(frame),
                        next => {
                            pending = next;
                            break;
                        },
                    }
                }
                let plan = engine.prepare(&generation, &frames, &generation.graph.capture).await?;
                for (queue, outgoing) in queues.iter().zip(&plan.relays) {
                    for frame in outgoing {
                        recent.nic_seen.insert(frame.id);
                    }
                    queue.enqueue(outgoing, &generation);
                }
                for frame in &frames {
                    recent.nic_seen.insert(frame.id);
                }
                engine.metrics.published.fetch_add(frames.len() as u64, Ordering::Relaxed);
            },
            Job::Received {
                source,
                deliveries,
                completed,
                generation,
            } => {
                let processed = deliver((&engine, &generation, source), deliveries, &mut recent).await?;
                let _ = completed.send(processed);
            },
        }
    }
}

async fn deliver(source: (&PipelineEngine, &Generation, usize), deliveries: Vec<Delivery>, recent: &mut DeliveryHistory) -> Result<ProcessedBatch, EngineError> {
    let (engine, generation, index) = source;
    let mut batch_ids = HashSet::new();
    let frames: Vec<_> = deliveries
        .iter()
        .filter_map(|delivery| {
            let frame = &delivery.frame;
            if recent.completed.contains(&(index, frame.id)) || !batch_ids.insert(frame.id) {
                engine.pipeline_metrics.duplicates.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            if engine.settings.firewall.check_frame(&frame.bytes).is_err() {
                engine.metrics.rejected_deliveries.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            Some(frame.clone())
        })
        .collect();
    let plan = engine.prepare(generation, &frames, &generation.graph.received[index]).await?;
    let inject: HashMap<_, _> = plan.inject.into_iter().map(|injection| (injection.source, injection.frame)).collect();
    let mut receipts = Vec::new();
    let mut failure = None;
    for delivery in deliveries {
        let frame = &delivery.frame;
        if let Some(outgoing) = inject.get(&frame.id).filter(|_| !recent.completed.contains(&(index, frame.id))) {
            if recent.nic_seen.contains(&outgoing.id) {
                engine.pipeline_metrics.duplicates.fetch_add(1, Ordering::Relaxed);
            } else {
                let sent = timeout(engine.settings.config.operation_timeout(), engine.network.send(&outgoing.bytes))
                    .await
                    .unwrap_or_else(|_| Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "NICへの送信がタイムアウトしました")));
                if let Err(error) = sent {
                    failure = Some(error);
                    break;
                }
                recent.nic_seen.insert(outgoing.id);
                engine.metrics.injected.fetch_add(1, Ordering::Relaxed);
            }
        }
        recent.completed.insert((index, frame.id));
        receipts.push(delivery.receipt);
    }
    Ok(ProcessedBatch { receipts, failure })
}
