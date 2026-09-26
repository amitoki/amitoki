//! 独自Ethernetペイロードの参考定義。codecと生成器をStageから再利用する。
use crate::{PacketCodec, PacketDefinition, PacketGenerator, PacketResult};
use serde::Deserialize;
use serde_json::{json, Value};

const ETHERNET_HEADER: usize = 14;
const HEADER_BYTES: usize = 32;
const MIN_FRAME_BYTES: usize = 60;
const MAX_PAYLOAD_BYTES: usize = 1400;
// ラボで既に使っている実験用EtherType。IPのパーサには渡さない。
const ETHER_TYPE: u16 = 0x88b5;
const MAGIC: &[u8; 4] = b"AMTK";
const FORMAT_VERSION: u8 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Telemetry {
    pub sequence: u64,
    pub temperature: i16,
    pub payload: Vec<u8>,
}

impl PacketCodec for Telemetry {
    fn decode(bytes: &[u8]) -> PacketResult<Self> {
        if bytes.len() < HEADER_BYTES || bytes[12..ETHERNET_HEADER] != ETHER_TYPE.to_be_bytes() || &bytes[14..18] != MAGIC || bytes[18] != FORMAT_VERSION || bytes[19] != 0 {
            return Err("telemetry-v1のヘッダが不正です".into());
        }
        let length = u16::from_be_bytes([bytes[30], bytes[31]]) as usize;
        let end = HEADER_BYTES + length;
        if length > MAX_PAYLOAD_BYTES || bytes.len() != end.max(MIN_FRAME_BYTES) || bytes[end..].iter().any(|byte| *byte != 0) {
            return Err("telemetry-v1のペイロード長・パディングが不正です".into());
        }
        Ok(Self {
            sequence: u64::from_be_bytes(bytes[20..28].try_into().expect("header length checked")),
            temperature: i16::from_be_bytes([bytes[28], bytes[29]]),
            payload: bytes[HEADER_BYTES..end].to_vec(),
        })
    }

    fn encode(&self) -> PacketResult<Vec<u8>> {
        if self.payload.len() > MAX_PAYLOAD_BYTES {
            return Err("telemetry-v1のペイロードが上限を超えています".into());
        }
        let mut bytes = vec![0; (HEADER_BYTES + self.payload.len()).max(MIN_FRAME_BYTES)];
        bytes[..6].copy_from_slice(&[2, 0, 0, 0, 0, 2]);
        bytes[6..12].copy_from_slice(&[2, 0, 0, 0, 0, 1]);
        bytes[12..14].copy_from_slice(&ETHER_TYPE.to_be_bytes());
        bytes[14..18].copy_from_slice(MAGIC);
        bytes[18] = FORMAT_VERSION;
        bytes[20..28].copy_from_slice(&self.sequence.to_be_bytes());
        bytes[28..30].copy_from_slice(&self.temperature.to_be_bytes());
        bytes[30..32].copy_from_slice(&(self.payload.len() as u16).to_be_bytes());
        bytes[HEADER_BYTES..HEADER_BYTES + self.payload.len()].copy_from_slice(&self.payload);
        Ok(bytes)
    }
}

pub struct TelemetryGenerator;

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Options {
    min_temperature: i16,
    max_temperature: i16,
    payload_bytes: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            min_temperature: 0,
            max_temperature: 100,
            payload_bytes: 64,
        }
    }
}

impl PacketGenerator for TelemetryGenerator {
    fn definition(&self) -> PacketDefinition {
        PacketDefinition {
            name: "telemetry-v1".into(),
            description: "独自Ethernet形式の温度・連番・可変長ペイロード".into(),
            config_schema: json!({"type":"object", "additionalProperties":false, "properties":{
                "min_temperature":{"type":"integer","minimum":-32768,"maximum":32767,"default":0},
                "max_temperature":{"type":"integer","minimum":-32768,"maximum":32767,"default":100},
                "payload_bytes":{"type":"integer","minimum":0,"maximum":MAX_PAYLOAD_BYTES,"default":64}
            }}),
        }
    }

    fn generate(&self, index: u64, seed: u64, options: &Value) -> PacketResult<Vec<u8>> {
        let options: Options = serde_json::from_value(options.clone()).map_err(|_| "生成条件が不正です")?;
        if options.min_temperature > options.max_temperature || options.payload_bytes > MAX_PAYLOAD_BYTES {
            return Err("温度範囲またはペイロード長が不正です".into());
        }
        // SplitMix64の整数ミキサー。暗号用途ではなく、版を固定した試験の再現に使う。
        const STEP: u64 = 0x9e3779b97f4a7c15;
        const MIX_FIRST: u64 = 0xbf58476d1ce4e5b9;
        const MIX_SECOND: u64 = 0x94d049bb133111eb;
        let mut mixed = seed.wrapping_add(index.wrapping_mul(STEP));
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(MIX_FIRST);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(MIX_SECOND);
        mixed ^= mixed >> 31;
        let range = (i32::from(options.max_temperature) - i32::from(options.min_temperature) + 1) as u64;
        let temperature = (i32::from(options.min_temperature) + (mixed % range) as i32) as i16;
        Telemetry {
            sequence: index,
            temperature,
            payload: vec![mixed as u8; options.payload_bytes],
        }
        .encode()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_wire_bytes_decode_with_big_endian_fields() {
        let mut bytes = vec![0; MIN_FRAME_BYTES];
        bytes[12..32].copy_from_slice(&[
            0x88, 0xb5, b'A', b'M', b'T', b'K', 1, 0, 0, 0, 0, 0, 0, 0, 0, 42, 0xff, 0xf6, 0, 2,
        ]);
        bytes[32..34].copy_from_slice(&[7, 8]);
        assert_eq!(
            Telemetry::decode(&bytes).unwrap(),
            Telemetry {
                sequence: 42,
                temperature: -10,
                payload: vec![7, 8]
            }
        );
        bytes[31] = 255;
        assert!(Telemetry::decode(&bytes).is_err());
    }

    #[test]
    fn truncated_headers_and_invalid_padding_are_rejected() {
        let packet = Telemetry {
            sequence: 1,
            temperature: 80,
            payload: vec![9],
        };
        let mut bytes = packet.encode().unwrap();
        for length in 0..bytes.len() {
            assert!(Telemetry::decode(&bytes[..length]).is_err());
        }
        assert_eq!(Telemetry::decode(&bytes).unwrap(), packet);
        *bytes.last_mut().unwrap() = 1;
        assert!(Telemetry::decode(&bytes).is_err());
    }

    #[test]
    fn generated_values_are_repeatable_and_respect_limits() {
        let options = json!({"min_temperature":70,"max_temperature":80,"payload_bytes":1400});
        for index in 0..1000 {
            let bytes = TelemetryGenerator.generate(index, 42, &options).unwrap();
            assert_eq!(bytes, TelemetryGenerator.generate(index, 42, &options).unwrap());
            let packet = Telemetry::decode(&bytes).unwrap();
            assert_eq!(packet.sequence, index);
            assert!((70..=80).contains(&packet.temperature));
            assert_eq!(packet.payload.len(), 1400);
        }
        assert!(TelemetryGenerator.generate(0, 0, &json!({"min_temperature":2,"max_temperature":1})).is_err());
    }
}
