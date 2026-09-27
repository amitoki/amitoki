use crate::packet::{parse_frame, MacAddress, PacketError, PacketMetadata};
use serde::Deserialize;
use std::net::IpAddr;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Policy {
    #[default]
    Whitelist,
    Blacklist,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", content = "value", deny_unknown_fields)]
pub enum Filter {
    SrcMacAddress(MacAddress),
    DstMacAddress(MacAddress),
    EtherType(u16),
    SrcIpAddress(IpAddr),
    DstIpAddress(IpAddr),
    IpProtocol(u8),
    SrcPort(u16),
    DstPort(u16),
}

impl Filter {
    fn matches(&self, packet: &PacketMetadata) -> bool {
        match self {
            Self::SrcMacAddress(value) => packet.src_mac == *value,
            Self::DstMacAddress(value) => packet.dst_mac == *value,
            Self::EtherType(value) => packet.ether_type == *value,
            Self::SrcIpAddress(value) => packet.src_ip == Some(*value),
            Self::DstIpAddress(value) => packet.dst_ip == Some(*value),
            Self::IpProtocol(value) => packet.ip_protocol == Some(*value),
            Self::SrcPort(value) => packet.src_port == Some(*value),
            Self::DstPort(value) => packet.dst_port == Some(*value),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Firewall {
    pub policy: Policy,
    pub rules: Vec<Filter>,
}

impl Firewall {
    pub fn allows(&self, packet: &PacketMetadata) -> bool {
        // 同じpolicyのルールはOR条件。意味のない優先度比較・毎パケットのロックを持たない。
        let matches = self.rules.iter().any(|rule| rule.matches(packet));
        match self.policy {
            Policy::Whitelist => matches,
            Policy::Blacklist => !matches,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FrameRejection {
    #[error("{0}")]
    Packet(#[from] PacketError),
    #[error("本体のfirewallルールにより拒否しました")]
    Firewall,
}
impl Firewall {
    pub fn check_frame(&self, bytes: &[u8]) -> Result<(), FrameRejection> {
        let packet = parse_frame(bytes)?;
        if !self.allows(&packet) {
            return Err(FrameRejection::Firewall);
        }
        Ok(())
    }
}
