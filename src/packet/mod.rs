mod ipv4;
mod ipv6;
mod metadata;
mod transport;

use amitoki_relay::{MAX_FRAME_SIZE, MIN_FRAME_SIZE};
pub use metadata::{MacAddress, PacketMetadata};

// IEEE 802.1Q/802.1adの二重タグまでを許可し、解析時間の上限を固定する。
const MAX_VLAN_TAGS: usize = 2;
pub(crate) const TCP_PROTOCOL: u8 = 6;
pub(crate) const UDP_PROTOCOL: u8 = 17;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PacketError {
    #[error("パケット長またはヘッダが不正です")]
    Malformed,
    #[error("IPフラグメントはフィルタを安全に適用できないため中継しません")]
    Fragmented,
    #[error("未対応のヘッダ構成です")]
    Unsupported,
}

pub fn parse_frame(frame: &[u8]) -> Result<PacketMetadata, PacketError> {
    if !(MIN_FRAME_SIZE..=MAX_FRAME_SIZE).contains(&frame.len()) {
        return Err(PacketError::Malformed);
    }
    let mut packet = PacketMetadata {
        dst_mac: MacAddress(frame[..6].try_into().map_err(|_| PacketError::Malformed)?),
        src_mac: MacAddress(frame[6..12].try_into().map_err(|_| PacketError::Malformed)?),
        ether_type: u16::from_be_bytes([frame[12], frame[13]]),
        src_ip: None,
        dst_ip: None,
        ip_protocol: None,
        src_port: None,
        dst_port: None,
    };
    let mut payload = &frame[MIN_FRAME_SIZE..];
    let mut tag_count = 0;
    while matches!(packet.ether_type, 0x8100 | 0x88a8) {
        if tag_count == MAX_VLAN_TAGS {
            return Err(PacketError::Unsupported);
        }
        if payload.len() < 4 {
            return Err(PacketError::Malformed);
        }
        packet.ether_type = u16::from_be_bytes([payload[2], payload[3]]);
        payload = &payload[4..];
        tag_count += 1;
    }
    match packet.ether_type {
        0x0800 => ipv4::parse(payload, &mut packet)?,
        0x86dd => ipv6::parse(payload, &mut packet)?,
        // ARPはIPのポートを持たない。IP用の最低長を要求しない。
        0x0806 if payload.len() < 28 => return Err(PacketError::Malformed),
        _ => {},
    }
    Ok(packet)
}
