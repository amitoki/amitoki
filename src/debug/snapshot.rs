//! UI向けのパケット表示。独自形式はStageのannotationsをそのまま表示する。
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ペイロード全体を各Stageで複製せず、差分表示用の先頭だけを保持する。
const PREVIEW_BYTES: usize = 256;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct PacketSnapshot {
    pub length: usize,
    pub fields: Value,
    pub annotations: Value,
    pub hex: String,
    pub truncated: bool,
}

impl PacketSnapshot {
    pub fn new(bytes: &[u8], annotations: Value) -> Self {
        let fields = match crate::packet::parse_frame(bytes) {
            Ok(packet) => json!({
                "src_mac": packet.src_mac.to_string(), "dst_mac": packet.dst_mac.to_string(),
                "ether_type": format!("0x{:04x}", packet.ether_type),
                "src_ip": packet.src_ip.map(|ip| ip.to_string()), "dst_ip": packet.dst_ip.map(|ip| ip.to_string()),
                "protocol": packet.ip_protocol, "src_port": packet.src_port, "dst_port": packet.dst_port,
            }),
            Err(error) => json!({"error":error.to_string()}),
        };
        let hex = bytes.iter().take(PREVIEW_BYTES).map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join(" ");
        Self {
            length: bytes.len(),
            fields,
            annotations,
            hex,
            truncated: bytes.len() > PREVIEW_BYTES,
        }
    }
}
