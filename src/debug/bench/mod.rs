//! 生成とStage/IPCの時間を分け、上限付きバッチでローカル負荷を測る。
mod arguments;
mod latency;
mod report;
use super::{generated::GeneratedInput, runner::ReplayPlan, stage_session::StageSession};
use crate::{
    pipeline::{plan::Planner, PipelineMetrics},
    plugin_manager::{ManagerResult, PluginStore},
};
use amitoki_plugin_sdk::wire::MAX_BATCH;
use amitoki_relay::Frame;
pub use arguments::BenchArguments;
use bytes::Bytes;
use report::Report;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use uuid::Uuid;

pub async fn run(store: &PluginStore, arguments: BenchArguments) -> ManagerResult<()> {
    if arguments.test.pcap.is_some() {
        return Err("benchには--packetを指定してください。PCAPの検証にはtest/replayを使ってください".into());
    }
    let session = StageSession::open(store, &arguments.test, &arguments.test.target).await?;
    let plan = session.plan();
    let generator = GeneratedInput::open(store, &arguments.test, &session).await?;
    let mut offset = 0;
    while offset < arguments.warmup {
        let count = (arguments.warmup - offset).min(arguments.batch_size as u64) as usize;
        let packets = generator.generate(offset, count).await?;
        process(&plan, packets, offset).await?;
        offset += count as u64;
    }
    let mut report = Report::new(&session, &generator, &arguments);
    let started = Instant::now();
    let mut latencies = latency::Latencies::default();
    let mut failure = None;
    loop {
        if arguments.duration.is_none() && report.packets >= arguments.test.count {
            break;
        }
        if let Some(rate) = arguments.rate {
            let scheduled = started + Duration::from_secs_f64(report.packets as f64 / rate as f64);
            let scheduled = arguments.duration.map_or(scheduled, |duration| scheduled.min(started + duration));
            tokio::time::sleep_until(scheduled.into()).await;
        }
        if arguments.duration.is_some_and(|duration| started.elapsed() >= duration) {
            break;
        }
        let count = if arguments.duration.is_some() {
            arguments.batch_size as u64
        } else {
            (arguments.test.count - report.packets).min(arguments.batch_size as u64)
        } as usize;
        let generation_started = Instant::now();
        let generated = generator.generate(offset, count).await;
        report.generation_seconds += generation_started.elapsed().as_secs_f64();
        let packets = match generated {
            Ok(packets) => packets,
            Err(error) => {
                report.errors += 1;
                failure = Some(error);
                break;
            },
        };
        let processing_started = Instant::now();
        let outcome = process(&plan, packets, offset).await;
        let elapsed = processing_started.elapsed();
        report.processing_seconds += elapsed.as_secs_f64();
        latencies.record(elapsed);
        report.packets += count as u64;
        report.batches += 1;
        offset += count as u64;
        match outcome {
            Ok((rejected, outputs)) => {
                report.rejected_packets += rejected;
                for (port, count) in outputs {
                    *report.output_packets_by_port.entry(port).or_default() += count;
                }
            },
            Err(error) => {
                report.errors += 1;
                failure = Some(error);
                break;
            },
        }
    }
    report.elapsed_seconds = started.elapsed().as_secs_f64();
    report.packets_per_second = report.packets as f64 / report.elapsed_seconds.max(f64::EPSILON);
    report.batch_latency = latencies.percentiles();
    report.write(arguments.test.json)?;
    if let Some(error) = failure {
        return Err(error);
    }
    if report.rejected_packets != 0 {
        return Err("生成パケットが本体検査で拒否されました".into());
    }
    Ok(())
}

async fn process(plan: &ReplayPlan, packets: Vec<Vec<u8>>, offset: u64) -> ManagerResult<(u64, BTreeMap<String, u64>)> {
    let mut rejected = 0;
    let mut frames = Vec::with_capacity(packets.len().min(MAX_BATCH));
    for (index, bytes) in packets.into_iter().enumerate() {
        if plan.firewall.check_frame(&bytes).is_err() {
            rejected += 1;
            continue;
        }
        frames.push(Frame {
            id: Uuid::from_u128(offset as u128 + index as u128 + 1),
            bytes: Bytes::from(bytes),
        });
    }
    let metrics = PipelineMetrics::default();
    let planner = Planner {
        firewall: &plan.firewall,
        graph: &plan.graph,
        blocks: &plan.blocks,
        metrics: &metrics,
        operation_timeout: plan.operation_timeout,
    };
    let outputs = planner.prepare_traced(&frames, &plan.entry, None).await?;
    let ports = plan.relay_names.iter().zip(outputs.relays).map(|(port, frames)| (port.trim_start_matches("output:").to_owned(), frames.len() as u64)).collect();
    Ok((rejected, ports))
}
