//! 共有codecで温度を書き換え、ペイロードを消去するサンプルStage。
use amitoki_plugin_sdk::{
    packet::{telemetry::Telemetry, PacketCodec},
    relay::RelayError,
    stage::{serve_stage, Stage, StageContext, StageDefinition, StageOutput, StagePacket, StagePlugin},
    PluginManifest, PROTOCOL_VERSION,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Options {
    temperature: Option<i16>,
    redact_payload: bool,
}

struct RewritePlugin;
struct RewriteStage(Options);

fn manifest() -> PluginManifest {
    PluginManifest {
        name: "telemetry-rewrite".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        protocol_version: PROTOCOL_VERSION,
        description: "telemetry-v1の温度変更・ペイロード消去のサンプル".into(),
        block: Some(StageDefinition {
            outputs: vec!["pass".into(), "drop".into()],
            rewrite: true,
        }),
        packets: vec![],
        config_schema: json!({"type":"object","additionalProperties":false,"properties":{
            "temperature":{"type":"integer","minimum":-32768,"maximum":32767},
            "redact_payload":{"type":"boolean","default":false}
        }}),
    }
}

#[async_trait]
impl StagePlugin for RewritePlugin {
    async fn connect(&self, _context: StageContext, options: Value) -> Result<Arc<dyn Stage>, RelayError> {
        let options = serde_json::from_value(options).map_err(|_| RelayError::permanent("telemetry-rewriteの設定が不正です"))?;
        Ok(Arc::new(RewriteStage(options)))
    }
}

impl RewriteStage {
    fn rewrite(&self, packet: &StagePacket) -> Result<StageOutput, RelayError> {
        let Ok(mut telemetry) = Telemetry::decode(&packet.frame.bytes) else {
            return Ok(StageOutput {
                ports: vec!["drop".into()],
                annotations: packet.annotations.clone(),
                bytes: None,
            });
        };
        if let Some(temperature) = self.0.temperature {
            telemetry.temperature = temperature;
        }
        if self.0.redact_payload {
            telemetry.payload.fill(0);
        }
        let mut bytes = telemetry.encode().map_err(RelayError::permanent)?;
        // codecのサンプルMACアドレスで実パケットの宛先を上書きしない。
        bytes[..12].copy_from_slice(&packet.frame.bytes[..12]);
        let mut annotations = packet.annotations.clone();
        annotations["telemetry"] = json!({"sequence":telemetry.sequence,"temperature":telemetry.temperature,"payload_bytes":telemetry.payload.len()});
        annotations["rewrite"] = json!({"redacted":self.0.redact_payload});
        Ok(StageOutput {
            ports: vec!["pass".into()],
            annotations,
            bytes: Some(bytes.into()),
        })
    }
}

#[async_trait]
impl Stage for RewriteStage {
    async fn process(&self, packets: &[StagePacket]) -> Result<Vec<StageOutput>, RelayError> {
        packets.iter().map(|packet| self.rewrite(packet)).collect()
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = manifest();
    if std::env::args().any(|argument| argument == "--describe") {
        println!("{}", serde_json::to_string(&manifest)?);
        return Ok(());
    }
    serve_stage(RewritePlugin, manifest).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use amitoki_plugin_sdk::relay::Frame;

    #[test]
    fn rewrite_preserves_addresses_sequence_and_input_while_changing_temperature_and_payload() {
        let mut bytes = Telemetry {
            sequence: 42,
            temperature: 99,
            payload: vec![7, 8, 9],
        }
        .encode()
        .unwrap();
        bytes[..12].fill(3);
        let packet = StagePacket {
            frame: Frame::new(bytes.clone().into()).unwrap(),
            annotations: json!({"source":"test"}),
        };
        let output = RewriteStage(Options {
            temperature: Some(25),
            redact_payload: true,
        })
        .rewrite(&packet)
        .unwrap();
        let rewritten = output.bytes.unwrap();
        let telemetry = Telemetry::decode(&rewritten).unwrap();
        assert_eq!(&rewritten[..12], &bytes[..12]);
        assert_eq!(
            telemetry,
            Telemetry {
                sequence: 42,
                temperature: 25,
                payload: vec![0, 0, 0]
            }
        );
        assert_eq!(packet.frame.bytes.as_ref(), bytes);
        assert_eq!(output.annotations["source"], "test");
    }
}
