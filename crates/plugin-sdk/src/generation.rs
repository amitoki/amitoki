use crate::{wire::MAX_BATCH, PluginManifest};
use amitoki_packet::GenerateRequest;
use amitoki_relay::{RelayError, MAX_FRAME_SIZE};

pub(crate) fn validate_request(manifest: &PluginManifest, request: &GenerateRequest) -> Result<(), RelayError> {
    if request.count == 0 || request.count > MAX_BATCH || request.start.checked_add(request.count as u64).is_none() {
        return Err(RelayError::permanent("パケット生成の件数・開始位置が不正です"));
    }
    let definition = manifest.packets.iter().find(|definition| definition.name == request.name).ok_or_else(|| RelayError::permanent("パケット定義がありません"))?;
    let validator = jsonschema::validator_for(&definition.config_schema).map_err(|_| RelayError::permanent("パケット生成条件のスキーマが不正です"))?;
    if !validator.is_valid(&request.options) {
        return Err(RelayError::permanent("パケット生成条件が定義に一致しません"));
    }
    Ok(())
}

pub(crate) fn validate_packets(request: &GenerateRequest, packets: &[Vec<u8>]) -> Result<(), RelayError> {
    if packets.len() != request.count || packets.iter().any(|packet| !(14..=MAX_FRAME_SIZE).contains(&packet.len())) {
        return Err(RelayError::permanent("生成パケットの件数・長さが不正です"));
    }
    Ok(())
}
