use super::pcap::CapturePacket;
use crate::plugin_manager::ManagerResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Write;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PacketReport {
    pub packet: u64,
    pub source: String,
    pub timestamp_ns: u64,
    pub length: usize,
    pub sha256: String,
    pub rejection: Option<String>,
    pub steps: Vec<BlockStep>,
    pub terminals: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BlockStep {
    pub block: String,
    pub ports: Vec<PortRoute>,
    pub annotations: Value,
    pub error: Option<String>,
    pub elapsed_us: u64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PortRoute {
    pub port: String,
    pub to: Vec<String>,
}

impl PacketReport {
    pub fn new(packet: u64, source: &str, capture: &CapturePacket) -> Self {
        Self {
            packet,
            source: source.into(),
            timestamp_ns: capture.timestamp_ns,
            length: capture.bytes.len(),
            sha256: format!("{:x}", Sha256::digest(&capture.bytes)),
            rejection: None,
            steps: Vec::new(),
            terminals: Vec::new(),
            error: None,
        }
    }

    pub fn write(&self, writer: &mut impl Write, json: bool) -> ManagerResult<()> {
        if json {
            serde_json::to_writer(&mut *writer, self)?;
            writeln!(writer)?;
            return Ok(());
        }
        writeln!(writer, "#{} {} bytes / {}", self.packet, self.length, self.source)?;
        if let Some(reason) = &self.rejection {
            writeln!(writer, "  本体で拒否: {reason}")?;
        }
        for step in &self.steps {
            writeln!(writer, "  {} ({} μs)", step.block, step.elapsed_us)?;
            if let Some(error) = &step.error {
                writeln!(writer, "    エラー: {error}")?;
            }
            if step.ports.is_empty() && step.error.is_none() {
                writeln!(writer, "    出力なし → 破棄")?;
            }
            for route in &step.ports {
                let target = if route.to.is_empty() { "破棄".into() } else { route.to.join(", ") };
                writeln!(writer, "    {} → {}", route.port, target)?;
            }
            writeln!(writer, "    解析結果: {}", step.annotations)?;
        }
        if let Some(error) = &self.error {
            writeln!(writer, "  停止: {error}")?;
        }
        writeln!(writer, "  最終出力: {}", if self.terminals.is_empty() { "なし".into() } else { self.terminals.join(", ") })?;
        Ok(())
    }

    pub fn without_timing(mut self) -> Self {
        for step in &mut self.steps {
            step.elapsed_us = 0;
        }
        self
    }
}
