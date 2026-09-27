use super::{arguments::BenchArguments, latency};
use crate::{
    debug::{generated::GeneratedInput, stage_session::StageSession},
    plugin_manager::ManagerResult,
};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub(super) struct Report {
    pub stage: String,
    pub stage_version: String,
    pub generator: String,
    pub generator_version: String,
    pub packet: String,
    pub seed: u64,
    pub packets: u64,
    pub batches: u64,
    pub warmup_packets: u64,
    pub batch_size: u16,
    pub requested_rate: Option<u64>,
    pub elapsed_seconds: f64,
    pub packets_per_second: f64,
    pub generation_seconds: f64,
    pub processing_seconds: f64,
    pub rejected_packets: u64,
    pub errors: u64,
    pub output_packets_by_port: BTreeMap<String, u64>,
    pub batch_latency: latency::Percentiles,
}

impl Report {
    pub fn new(session: &StageSession, generator: &GeneratedInput, arguments: &BenchArguments) -> Self {
        Self {
            stage: session.manifest.name.clone(),
            stage_version: session.manifest.version.clone(),
            generator: generator.provider.clone(),
            generator_version: generator.version.clone(),
            packet: arguments.test.packet.clone().expect("validated packet"),
            seed: arguments.test.seed,
            packets: 0,
            batches: 0,
            warmup_packets: arguments.warmup,
            batch_size: arguments.batch_size,
            requested_rate: arguments.rate,
            elapsed_seconds: 0.0,
            packets_per_second: 0.0,
            generation_seconds: 0.0,
            processing_seconds: 0.0,
            rejected_packets: 0,
            errors: 0,
            output_packets_by_port: BTreeMap::new(),
            batch_latency: latency::Latencies::default().percentiles(),
        }
    }

    pub fn write(&self, json: bool) -> ManagerResult<()> {
        if json {
            println!("{}", serde_json::to_string(self)?);
        } else {
            println!("{} packets / {:.1} packets/s / {} batches\ngeneration={:.6}s processing={:.6}s rejected={} errors={}\nbatch latency upper bounds: p50={}μs p95={}μs p99={}μs\nports={:?}", self.packets, self.packets_per_second, self.batches, self.generation_seconds, self.processing_seconds, self.rejected_packets, self.errors, self.batch_latency.p50_upper_bound_us, self.batch_latency.p95_upper_bound_us, self.batch_latency.p99_upper_bound_us, self.output_packets_by_port);
        }
        Ok(())
    }
}
