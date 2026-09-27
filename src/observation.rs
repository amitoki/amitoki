//! 管理画面へ渡す観測値。プラグインの設定値・接続文字列は含めない。
use crate::{config::AppConfig, pipeline::PipelineConfig, runtime::Runtime};
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Topology {
    pub node_id: String,
    pub channel: String,
    pub interface: String,
    pub stages: Vec<Stage>,
    pub relays: Vec<Relay>,
    pub routes: Vec<amitoki_pipeline::Route>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stage {
    pub id: String,
    pub plugin: String,
    pub on_error: crate::pipeline::ErrorPolicy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Relay {
    pub id: String,
    pub plugin: String,
}

impl Topology {
    pub fn new(config: &AppConfig) -> Self {
        let mut topology = Self {
            node_id: config.node_id.clone(),
            channel: config.channel.clone(),
            interface: config.interface.clone(),
            stages: vec![],
            relays: vec![],
            routes: vec![],
        };
        if let Some(pipeline) = &config.pipeline {
            topology.set_pipeline(pipeline);
        } else if let Some(relay) = &config.relay {
            topology.relays.push(Relay {
                id: "relay".into(),
                plugin: relay.plugin.clone(),
            });
            topology.routes = vec![
                amitoki_pipeline::Route {
                    from: "capture".into(),
                    to: vec!["relay".into()],
                },
                amitoki_pipeline::Route {
                    from: "relay.received".into(),
                    to: vec!["inject".into()],
                },
            ];
        }
        topology
    }

    pub fn set_pipeline(&mut self, pipeline: &PipelineConfig) {
        self.stages = pipeline
            .blocks
            .iter()
            .map(|stage| Stage {
                id: stage.id.clone(),
                plugin: stage.plugin.clone(),
                on_error: stage.on_error,
            })
            .collect();
        self.relays = pipeline
            .relays
            .iter()
            .map(|relay| Relay {
                id: relay.id.clone(),
                plugin: relay.plugin.clone(),
            })
            .collect();
        self.routes = pipeline.routes.clone();
    }
}

#[derive(Serialize, Deserialize)]
pub struct Status {
    pub generation: Option<u64>,
    pub topology: Topology,
    pub captured: u64,
    pub published: u64,
    pub injected: u64,
    pub filtered: u64,
    pub rejected: u64,
    pub retries: u64,
    pub relays: Vec<RelayStatus>,
}

#[derive(Serialize, Deserialize)]
pub struct RelayStatus {
    pub id: String,
    pub published: u64,
    pub dropped: u64,
    pub queued: u64,
    pub capacity: usize,
    pub failures: u64,
}

impl Status {
    pub fn new(runtime: &Runtime, topology: Topology, capacity: usize) -> Self {
        let metrics = runtime.metrics();
        let (generation, relays) = match runtime {
            Runtime::Pipeline(engine) => (
                Some(engine.generation()),
                topology
                    .relays
                    .iter()
                    .zip(&engine.relay_metrics)
                    .map(|(relay, metrics)| RelayStatus {
                        id: relay.id.clone(),
                        published: metrics.published.load(Ordering::Relaxed),
                        dropped: metrics.dropped.load(Ordering::Relaxed),
                        queued: metrics.queued.load(Ordering::Relaxed),
                        capacity,
                        failures: metrics.failures.load(Ordering::Relaxed),
                    })
                    .collect(),
            ),
            Runtime::Relay(_) => (None, vec![]),
        };
        Self {
            generation,
            topology,
            relays,
            captured: metrics.captured.load(Ordering::Relaxed),
            published: metrics.published.load(Ordering::Relaxed),
            injected: metrics.injected.load(Ordering::Relaxed),
            filtered: metrics.filtered.load(Ordering::Relaxed),
            rejected: metrics.rejected_deliveries.load(Ordering::Relaxed),
            retries: metrics.retries.load(Ordering::Relaxed),
        }
    }
}
