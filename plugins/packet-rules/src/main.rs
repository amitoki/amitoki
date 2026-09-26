//! 自作ブロックの参考実装。EtherTypeを分類し、解析結果と通過・拒否ポートを返す。
use amitoki_plugin_sdk::{
    block::{serve_block, Block, BlockContext, BlockDefinition, BlockOutput, BlockPacket, BlockPlugin},
    relay::RelayError,
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
#[serde(default, deny_unknown_fields)]
struct Options {
    allowed_ether_types: Vec<u16>,
    label: String,
    log_every: u64,
}
struct PacketRules;
struct Instance {
    options: Options,
    context: BlockContext,
    packets: AtomicU64,
}

fn manifest() -> PluginManifest {
    PluginManifest {
        name: "packet-rules".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        protocol_version: PROTOCOL_VERSION,
        description: "EtherTypeによる解析・フィルタの参考実装。空の許可一覧は全通過。".into(),
        block: Some(BlockDefinition {
            outputs: vec!["pass".into(), "drop".into()],
        }),
        config_schema: json!({"type":"object","additionalProperties":false,"properties":{
            "allowed_ether_types":{"type":"array","items":{"type":"integer","minimum":0,"maximum":65535},"maxItems":64,"default":[],"description":"通過させるEtherType。空は全通過。VLANは外側の値を判定する。"},
            "label":{"type":"string","maxLength":128,"default":"","description":"解析結果に付けるラベル"},
            "log_every":{"type":"integer","minimum":0,"maximum":1000000000,"default":0,"description":"Nパケットごとにstderrへ件数を出力。0は無効。"}
        }}),
    }
}
#[async_trait]
impl BlockPlugin for PacketRules {
    async fn connect(&self, context: BlockContext, options: Value) -> Result<Arc<dyn Block>, RelayError> {
        let options = serde_json::from_value(options).map_err(|_| RelayError::permanent("packet-rulesの設定が不正です"))?;
        Ok(Arc::new(Instance {
            options,
            context,
            packets: AtomicU64::new(0),
        }))
    }
}
#[async_trait]
impl Block for Instance {
    async fn process(&self, packets: &[BlockPacket]) -> Result<Vec<BlockOutput>, RelayError> {
        packets
            .iter()
            .map(|packet| {
                packet.validate()?;
                let bytes = &packet.frame.bytes;
                let ether_type = u16::from_be_bytes([bytes[12], bytes[13]]);
                let allowed = self.options.allowed_ether_types.is_empty() || self.options.allowed_ether_types.contains(&ether_type);
                let count = self.packets.fetch_add(1, Ordering::Relaxed) + 1;
                if self.options.log_every != 0 && count.is_multiple_of(self.options.log_every) {
                    eprintln!("packet-rules instance={} packets={count}", self.context.instance);
                }
                let mut annotations = packet.annotations.clone();
                annotations[&self.context.instance] = json!({"ether_type":ether_type,"length":bytes.len(),"label":self.options.label});
                Ok(BlockOutput {
                    ports: vec![if allowed { "pass" } else { "drop" }.into()],
                    annotations,
                })
            })
            .collect()
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments == ["--describe"] {
        println!("{}", serde_json::to_string(&manifest())?);
        return Ok(());
    }
    if arguments != ["--stdio"] {
        return Err("--describeまたは--stdioを指定してください".into());
    }
    serve_block(PacketRules, manifest()).await
}
