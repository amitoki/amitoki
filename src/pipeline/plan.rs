//! 解析を一度だけ実行し、配送処理が再試行できる送信計画を作る。
use super::{
    config::ErrorPolicy,
    graph::{Destination, Graph},
    trace::BlockTrace,
    PipelineEngine,
};
use amitoki_plugin_sdk::{
    block::{Block, BlockDefinition, BlockOutput, BlockPacket},
    wire::MAX_BATCH,
};
use amitoki_relay::{Frame, RelayError};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::time::timeout;

pub struct RunningBlock {
    pub block: Arc<dyn Block>,
    pub definition: BlockDefinition,
    pub on_error: ErrorPolicy,
}
#[derive(Default)]
pub struct PipelineMetrics {
    pub processed: AtomicU64,
    pub dropped_branches: AtomicU64,
    pub duplicates: AtomicU64,
}
pub struct Plan {
    pub relays: Vec<Vec<Frame>>,
    pub inject: Vec<Frame>,
}
struct Input {
    index: usize,
    packet: BlockPacket,
}
struct PendingRoutes {
    blocks: Vec<Vec<Input>>,
    relay_targets: Vec<HashSet<usize>>,
    injection_targets: HashSet<usize>,
}
pub(crate) struct Planner<'a> {
    pub graph: &'a Graph,
    pub blocks: &'a [RunningBlock],
    pub metrics: &'a PipelineMetrics,
    pub operation_timeout: Duration,
}

impl PipelineEngine {
    pub(super) async fn prepare(&self, frames: &[Frame], entry: &[Destination]) -> Result<Plan, RelayError> {
        Planner {
            graph: &self.graph,
            blocks: &self.blocks,
            metrics: &self.pipeline_metrics,
            operation_timeout: self.settings.config.operation_timeout(),
        }
        .prepare(frames, entry)
        .await
    }
}
impl Planner<'_> {
    async fn prepare(&self, frames: &[Frame], entry: &[Destination]) -> Result<Plan, RelayError> {
        self.prepare_traced(frames, entry, None).await
    }

    pub(crate) async fn prepare_traced(&self, frames: &[Frame], entry: &[Destination], mut trace: Option<&mut Vec<BlockTrace>>) -> Result<Plan, RelayError> {
        let mut pending = PendingRoutes {
            blocks: (0..self.blocks.len()).map(|_| Vec::new()).collect(),
            relay_targets: (0..frames.len()).map(|_| HashSet::new()).collect(),
            injection_targets: HashSet::new(),
        };
        for (index, frame) in frames.iter().enumerate() {
            pending.route(
                entry,
                Input {
                    index,
                    packet: BlockPacket {
                        frame: frame.clone(),
                        annotations: serde_json::json!({}),
                    },
                },
            );
        }
        for index in &self.graph.order {
            let inputs = std::mem::take(&mut pending.blocks[*index]);
            for batch in inputs.chunks(MAX_BATCH) {
                let Some(outputs) = self.process_step(*index, batch, trace.as_deref_mut()).await? else {
                    continue;
                };
                for (input, output) in batch.iter().zip(outputs) {
                    pending.apply_output(&self.graph.outputs[*index], input, output)?;
                }
            }
        }
        Ok(pending.finish(frames, self.graph.received.len()))
    }

    async fn process_step(&self, index: usize, inputs: &[Input], trace: Option<&mut Vec<BlockTrace>>) -> Result<Option<Vec<BlockOutput>>, RelayError> {
        let started = trace.as_ref().map(|_| Instant::now());
        let response = self.process(index, inputs).await;
        if let (Some(trace), Some(started)) = (trace, started) {
            let elapsed_us = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
            for (offset, input) in inputs.iter().enumerate() {
                trace.push(BlockTrace {
                    packet: input.index,
                    block: index,
                    elapsed_us,
                    output: response.as_ref().map(|outputs| outputs[offset].clone()).map_err(ToString::to_string),
                });
            }
        }
        match response {
            Ok(outputs) => Ok(Some(outputs)),
            Err(error) => match self.blocks[index].on_error {
                ErrorPolicy::Stop => Err(RelayError::permanent(format!("解析ブロックが失敗しました: {error}"))),
                ErrorPolicy::DropBranch => {
                    self.metrics.dropped_branches.fetch_add(inputs.len() as u64, Ordering::Relaxed);
                    Ok(None)
                },
            },
        }
    }

    async fn process(&self, index: usize, inputs: &[Input]) -> Result<Vec<BlockOutput>, RelayError> {
        let block = &self.blocks[index];
        let packets: Vec<_> = inputs.iter().map(|input| input.packet.clone()).collect();
        let outputs = timeout(self.operation_timeout, block.block.process(&packets))
            .await
            .map_err(|_| RelayError::permanent("解析ブロックがタイムアウトしました"))
            .and_then(|response| response)
            .and_then(|outputs| {
                if outputs.len() != inputs.len() {
                    return Err(RelayError::permanent("解析結果の件数が一致しません"));
                }
                for output in &outputs {
                    output.validate(&block.definition)?;
                }
                Ok(outputs)
            })?;
        self.metrics.processed.fetch_add(inputs.len() as u64, Ordering::Relaxed);
        Ok(outputs)
    }
}
impl PendingRoutes {
    fn apply_output(&mut self, ports: &HashMap<String, Vec<Destination>>, input: &Input, output: BlockOutput) -> Result<(), RelayError> {
        for port in output.ports {
            let destinations = ports.get(&port).ok_or_else(|| RelayError::permanent("ブロックの出力ポートと経路が一致しません"))?;
            self.route(
                destinations,
                Input {
                    index: input.index,
                    packet: BlockPacket {
                        frame: input.packet.frame.clone(),
                        annotations: output.annotations.clone(),
                    },
                },
            );
        }
        Ok(())
    }
    fn route(&mut self, destinations: &[Destination], input: Input) {
        for destination in destinations {
            match destination {
                Destination::Block(index) => self.blocks[*index].push(Input {
                    index: input.index,
                    packet: input.packet.clone(),
                }),
                Destination::Relay(index) => {
                    self.relay_targets[input.index].insert(*index);
                },
                Destination::Inject => {
                    self.injection_targets.insert(input.index);
                },
            }
        }
    }
    fn finish(self, frames: &[Frame], relay_count: usize) -> Plan {
        let mut plan = Plan {
            relays: vec![Vec::new(); relay_count],
            inject: Vec::new(),
        };
        // 終端で合流した同じフレームは1回だけ配送し、入力順を維持する。
        for (index, frame) in frames.iter().enumerate() {
            for relay in &self.relay_targets[index] {
                plan.relays[*relay].push(frame.clone());
            }
            if self.injection_targets.contains(&index) {
                plan.inject.push(frame.clone());
            }
        }
        plan
    }
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
