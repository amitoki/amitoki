//! 外部ブロックの設定・キャンセル・権限検証用。配布対象ではない。
use amitoki_plugin_sdk::{
    block::{serve_block, Block, BlockContext, BlockDefinition, BlockOutput, BlockPacket, BlockPlugin},
    PluginManifest, PROTOCOL_VERSION,
};
use amitoki_relay::RelayError;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
struct Fixture;
struct Instance {
    mode: String,
    instance: String,
}
#[async_trait]
impl BlockPlugin for Fixture {
    async fn connect(&self, context: BlockContext, options: Value) -> Result<Arc<dyn Block>, RelayError> {
        Ok(Arc::new(Instance {
            mode: options["mode"].as_str().unwrap_or("pass").into(),
            instance: context.instance,
        }))
    }
}
#[async_trait]
impl Block for Instance {
    async fn process(&self, packets: &[BlockPacket]) -> Result<Vec<BlockOutput>, RelayError> {
        match self.mode.as_str() {
            "delay" => {
                const RESPONSE_DELAY: Duration = Duration::from_millis(100);
                tokio::time::sleep(RESPONSE_DELAY).await;
            },
            "crash" => std::process::exit(23),
            _ => {},
        }
        let mut annotations = json!({"instance":self.instance});
        if self.mode == "capabilities" {
            let status = std::fs::read_to_string("/proc/self/status").map_err(|_| RelayError::permanent("プロセス情報の取得に失敗しました"))?;
            for key in ["CapEff", "CapPrm", "CapInh", "CapAmb", "NoNewPrivs"] {
                annotations[key] = json!(status.lines().find_map(|line| line.strip_prefix(&format!("{key}:"))).unwrap().trim());
            }
            let socket = unsafe { libc::socket(libc::AF_PACKET, libc::SOCK_RAW, 0) };
            annotations["raw_socket"] = json!(socket >= 0);
            if socket >= 0 {
                unsafe {
                    libc::close(socket);
                }
            }
        }
        if self.mode == "oversize" {
            annotations["excess"] = json!("x".repeat(4097));
        }
        Ok(packets
            .iter()
            .map(|packet| {
                let mut annotations = annotations.clone();
                annotations["input"] = packet.annotations.clone();
                BlockOutput {
                    ports: if self.mode == "drop" {
                        vec![]
                    } else {
                        vec![if self.mode == "invalid" { "inject" } else { "pass" }.into()]
                    },
                    annotations,
                }
            })
            .collect())
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = PluginManifest {
        name: "block-fixture".into(),
        version: "0.1.0".into(),
        protocol_version: PROTOCOL_VERSION,
        description: String::new(),
        block: Some(BlockDefinition { outputs: vec!["pass".into()] }),
        config_schema: json!({"type":"object","additionalProperties":false,"properties":{"mode":{"type":"string","enum":["pass","drop","delay","crash","capabilities","oversize","invalid"]}}}),
    };
    if std::env::args().any(|argument| argument == "--describe") {
        println!("{}", serde_json::to_string(&manifest)?);
        return Ok(());
    }
    serve_block(Fixture, manifest).await
}
