//! プロセス通信のキャンセル・クラッシュ検証用。配布用プラグインではない。
use amitoki_plugin_sdk::{PluginManifest, PROTOCOL_VERSION};
use amitoki_relay::{Delivery, Frame, Receipt, Relay, RelayContext, RelayError, RelayPlugin};
use async_trait::async_trait;
use bytes::Bytes;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

struct Fixture;
#[async_trait]
impl RelayPlugin for Fixture {
    fn name(&self) -> &'static str {
        "fixture"
    }
    async fn connect(&self, _: RelayContext, _: Value) -> Result<Arc<dyn Relay>, RelayError> {
        Ok(Arc::new(Self))
    }
}
#[async_trait]
impl Relay for Fixture {
    async fn publish(&self, _: &[Frame]) -> Result<(), RelayError> {
        Ok(())
    }
    async fn receive(&self, limit: usize) -> Result<Vec<Delivery>, RelayError> {
        if limit == 13 {
            std::process::exit(23);
        }
        // 呼び出しキャンセルより遅い応答を意図的に返す。
        const DELAY: Duration = Duration::from_millis(100);
        tokio::time::sleep(DELAY).await;
        if limit == 0 {
            return Ok(vec![]);
        }
        Ok(vec![Delivery {
            frame: Frame::new(Bytes::from(vec![1; 14]))?,
            receipt: Receipt("receipt".into()),
        }])
    }
    async fn acknowledge(&self, _: &[Receipt]) -> Result<(), RelayError> {
        Ok(())
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = PluginManifest {
        name: "fixture".into(),
        version: "0.1.0".into(),
        protocol_version: PROTOCOL_VERSION,
        description: String::new(),
        block: None,
        config_schema: json!({"type":"object","additionalProperties":false}),
    };
    if std::env::args().any(|argument| argument == "--describe") {
        println!("{}", serde_json::to_string(&manifest)?);
        return Ok(());
    }
    amitoki_plugin_sdk::serve(Fixture, manifest).await
}
