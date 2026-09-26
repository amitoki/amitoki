use super::report::{BlockStep, PortRoute};
use super::{
    pcap::{CapturePacket, CaptureReader},
    report::PacketReport,
};
use crate::pipeline::{
    graph::{Destination, Graph},
    plan::Planner,
    trace::BlockTrace,
    PipelineMetrics, RunningBlock,
};
use crate::{firewall::Firewall, plugin_manager::ManagerResult};
use amitoki_relay::Frame;
use bytes::Bytes;
use std::time::Duration;
use std::{fs::File, io::BufReader, path::PathBuf};
use uuid::Uuid;

pub(super) struct ReplayInput {
    pub pcap: PathBuf,
    pub source: String,
    pub json: bool,
}

pub(super) struct ReplayReader {
    capture: CaptureReader<BufReader<File>>,
    input: ReplayInput,
    packet: u64,
}
impl ReplayReader {
    pub fn open(input: ReplayInput) -> ManagerResult<Self> {
        Ok(Self {
            capture: CaptureReader::new(BufReader::new(File::open(&input.pcap)?))?,
            input,
            packet: 0,
        })
    }
    pub fn next(&mut self) -> ManagerResult<Option<(CapturePacket, PacketReport)>> {
        let Some(capture) = self.capture.next_packet()? else {
            return Ok(None);
        };
        self.packet += 1;
        let report = PacketReport::new(self.packet, &self.input.source, &capture);
        Ok(Some((capture, report)))
    }
    pub fn write(&self, report: &PacketReport) -> ManagerResult<()> {
        report.write(&mut std::io::stdout().lock(), self.input.json)
    }
}

pub(super) fn admit(capture: CapturePacket, report: &mut PacketReport, firewall: &Firewall) -> Option<Frame> {
    if capture.original_length as usize != capture.bytes.len() {
        report.rejection = Some("PCAPの保存時にパケットが切り詰められています".into());
        return None;
    }
    if let Err(reason) = firewall.check_frame(&capture.bytes) {
        report.rejection = Some(reason.to_string());
        return None;
    }
    // 実通信には使わない。再生ごとの解析結果を比較できるよう、入力順からIDを固定する。
    Some(Frame {
        id: Uuid::from_u128(report.packet as u128),
        bytes: Bytes::from(capture.bytes),
    })
}

pub(super) struct ReplayPlan {
    pub graph: Graph,
    pub blocks: Vec<RunningBlock>,
    pub block_names: Vec<String>,
    pub relay_names: Vec<String>,
    pub firewall: Firewall,
    pub operation_timeout: Duration,
    pub entry: Vec<Destination>,
}
impl ReplayPlan {
    pub async fn run(&self, mut reader: ReplayReader) -> ManagerResult<()> {
        let metrics = PipelineMetrics::default();
        let planner = Planner {
            graph: &self.graph,
            blocks: &self.blocks,
            metrics: &metrics,
            operation_timeout: self.operation_timeout,
        };
        while let Some((capture, mut report)) = reader.next()? {
            let Some(frame) = admit(capture, &mut report, &self.firewall) else {
                reader.write(&report)?;
                continue;
            };
            let mut trace = Vec::new();
            let outcome = planner.prepare_traced(&[frame], &self.entry, Some(&mut trace)).await;
            report.steps = trace.into_iter().map(|event| self.step(event)).collect();
            match &outcome {
                Ok(plan) => {
                    for (name, frames) in self.relay_names.iter().zip(&plan.relays) {
                        if !frames.is_empty() {
                            report.terminals.push(name.clone());
                        }
                    }
                    if !plan.inject.is_empty() {
                        report.terminals.push("inject".into());
                    }
                },
                Err(error) => report.error = Some(error.to_string()),
            }
            reader.write(&report)?;
            outcome?;
        }
        Ok(())
    }

    fn step(&self, event: BlockTrace) -> BlockStep {
        debug_assert_eq!(event.packet, 0);
        let mut step = BlockStep {
            block: self.block_names[event.block].clone(),
            ports: Vec::new(),
            annotations: serde_json::json!({}),
            elapsed_us: event.elapsed_us,
            error: None,
        };
        match event.output {
            Ok(output) => {
                for port in output.ports {
                    let to = self.graph.outputs[event.block][&port].iter().map(|destination| self.destination(*destination)).collect();
                    step.ports.push(PortRoute { port, to });
                }
                step.annotations = output.annotations;
            },
            Err(error) => step.error = Some(error),
        }
        step
    }
    fn destination(&self, destination: Destination) -> String {
        match destination {
            Destination::Block(index) => self.block_names[index].clone(),
            Destination::Relay(index) => self.relay_names[index].clone(),
            Destination::Inject => "inject".into(),
        }
    }
}
