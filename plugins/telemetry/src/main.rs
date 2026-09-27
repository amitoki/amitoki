use amitoki_plugin_sdk::{
    packet::{
        telemetry::{Telemetry, TelemetryGenerator},
        GenerateRequest, PacketCodec, PacketGenerator,
    },
    relay::RelayError,
    stage::{serve_stage, Stage, StageContext, StageDefinition, StageOutput, StagePacket, StagePlugin},
    PluginManifest, PROTOCOL_VERSION,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Operation {
    #[default]
    Parse,
    Filter,
    Log,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Options {
    operation: Operation,
    threshold: i16,
    log_every: u64,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            operation: Operation::Parse,
            threshold: 80,
            log_every: 1000,
        }
    }
}

struct TelemetryPlugin;
struct TelemetryStage {
    options: Options,
    count: AtomicU64,
}

fn manifest() -> PluginManifest {
    PluginManifest {
        name: "telemetry".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        protocol_version: PROTOCOL_VERSION,
        description: "共有Rust codecを使う解析・温度フィルタ・ログと試験パケット生成".into(),
        block: Some(StageDefinition {
            rewrite: false,
            outputs: vec!["pass".into(), "drop".into()],
        }),
        packets: vec![TelemetryGenerator.definition()],
        config_schema: json!({"type":"object","additionalProperties":false,"properties":{
            "operation":{"type":"string","enum":["parse","filter","log"],"default":"parse"},
            "threshold":{"type":"integer","minimum":-32768,"maximum":32767,"default":80},
            "log_every":{"type":"integer","minimum":0,"maximum":1000000000,"default":1000}
        }}),
    }
}

#[async_trait]
impl StagePlugin for TelemetryPlugin {
    async fn connect(&self, _context: StageContext, options: Value) -> Result<Arc<dyn Stage>, RelayError> {
        let options = serde_json::from_value(options).map_err(|_| RelayError::permanent("telemetryの設定が不正です"))?;
        Ok(Arc::new(TelemetryStage {
            options,
            count: AtomicU64::new(0),
        }))
    }
    fn generate(&self, request: &GenerateRequest) -> Result<Vec<Vec<u8>>, RelayError> {
        (request.start..request.start + request.count as u64)
            .map(|index| TelemetryGenerator.generate(index, request.seed, &request.options).map_err(RelayError::permanent))
            .collect()
    }
}

#[async_trait]
impl Stage for TelemetryStage {
    async fn process(&self, packets: &[StagePacket]) -> Result<Vec<StageOutput>, RelayError> {
        packets
            .iter()
            .map(|packet| {
                let message = match Telemetry::decode(&packet.frame.bytes) {
                    Ok(message) => message,
                    Err(_) => {
                        return Ok(StageOutput {
                            bytes: None,
                            ports: vec!["drop".into()],
                            annotations: packet.annotations.clone(),
                        })
                    },
                };
                let count = self.count.fetch_add(1, Ordering::Relaxed) + 1;
                let mut annotations = packet.annotations.clone();
                annotations["telemetry"] = json!({"sequence":message.sequence,"temperature":message.temperature,"payload_bytes":message.payload.len()});
                if matches!(self.options.operation, Operation::Log) && self.options.log_every != 0 && count.is_multiple_of(self.options.log_every) {
                    eprintln!("telemetry packets={count} sequence={} temperature={}", message.sequence, message.temperature);
                }
                let reject = matches!(self.options.operation, Operation::Filter) && message.temperature > self.options.threshold;
                Ok(StageOutput {
                    bytes: None,
                    ports: vec![if reject { "drop" } else { "pass" }.into()],
                    annotations,
                })
            })
            .collect()
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = manifest();
    if std::env::args().any(|argument| argument == "--describe") {
        println!("{}", serde_json::to_string(&manifest)?);
        return Ok(());
    }
    serve_stage(TelemetryPlugin, manifest).await
}
