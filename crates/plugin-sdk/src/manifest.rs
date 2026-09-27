use amitoki_relay::RelayError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ハンドシェイク時の定義・スキーマ検証量を制限する。
const MAX_PACKET_DEFINITIONS: usize = 32;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub protocol_version: u32,
    pub description: String,
    pub config_schema: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packets: Vec<amitoki_packet::PacketDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<crate::block::BlockDefinition>,
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), RelayError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(RelayError::permanent("プラグインの通信仕様に互換性がありません"));
        }
        if self.name.is_empty() || self.name.len() > 64 || !self.name.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-') {
            return Err(RelayError::permanent("プラグイン名は英小文字・数字・ハイフンで指定してください"));
        }
        if self.version.is_empty() || self.config_schema.get("type").and_then(Value::as_str) != Some("object") {
            return Err(RelayError::permanent("プラグインのバージョンまたは設定定義が不正です"));
        }
        if let Some(block) = &self.block {
            block.validate()?;
        }
        let mut names = std::collections::HashSet::new();
        if self.packets.len() > MAX_PACKET_DEFINITIONS {
            return Err(RelayError::permanent("パケット定義は32個以内にしてください"));
        }
        for packet in &self.packets {
            if !crate::block::valid_identifier(&packet.name) || !names.insert(&packet.name) || packet.config_schema.get("type").and_then(Value::as_str) != Some("object") {
                return Err(RelayError::permanent("パケット定義名・設定スキーマが不正です"));
            }
            jsonschema::validator_for(&packet.config_schema).map_err(|_| RelayError::permanent("パケット生成条件のスキーマが不正です"))?;
        }
        jsonschema::validator_for(&self.config_schema).map_err(|_| RelayError::permanent("設定スキーマを解釈できません"))?;
        Ok(())
    }

    pub fn validate_options(&self, options: &Value) -> Result<(), RelayError> {
        self.validate()?;
        let validator = jsonschema::validator_for(&self.config_schema).map_err(|_| RelayError::permanent("設定スキーマを解釈できません"))?;
        if let Err(error) = validator.validate(options) {
            // 値を出力すると接続文字列やトークンが混ざるため、場所だけ返す。
            return Err(RelayError::permanent(format!("{}の設定が不正です: {}", self.name, error.instance_path)));
        }
        Ok(())
    }
}
