use amitoki::{
    firewall::{Filter, Firewall, Policy},
    packet::{parse_frame, PacketError},
};

fn ipv4_frame(protocol: u8, transport_length: usize) -> Vec<u8> {
    let mut frame = vec![0; 14 + 20 + transport_length];
    frame[12..14].copy_from_slice(&0x0800_u16.to_be_bytes());
    frame[14] = 0x45;
    frame[16..18].copy_from_slice(&((20 + transport_length) as u16).to_be_bytes());
    frame[23] = protocol;
    frame[26..30].copy_from_slice(&[192, 0, 2, 1]);
    frame[30..34].copy_from_slice(&[192, 0, 2, 2]);
    frame
}

#[test]
fn udp_with_an_eight_byte_header_is_accepted() {
    let mut frame = ipv4_frame(17, 8);
    frame[34..36].copy_from_slice(&1234_u16.to_be_bytes());
    frame[36..38].copy_from_slice(&53_u16.to_be_bytes());
    frame[38..40].copy_from_slice(&8_u16.to_be_bytes());
    let packet = parse_frame(&frame).unwrap();
    assert_eq!(packet.src_port, Some(1234));
    assert_eq!(packet.dst_port, Some(53));
}

#[test]
fn ethernet_padding_is_not_read_as_transport_payload() {
    let mut frame = ipv4_frame(17, 8);
    frame[38..40].copy_from_slice(&8_u16.to_be_bytes());
    frame.resize(60, 0xff);
    assert!(parse_frame(&frame).is_ok());
}

#[test]
fn icmp_has_no_fabricated_ports() {
    let mut frame = ipv4_frame(1, 8);
    frame[34..38].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
    let packet = parse_frame(&frame).unwrap();
    assert_eq!(packet.src_port, None);
    assert_eq!(packet.dst_port, None);
    let firewall = Firewall {
        policy: Policy::Whitelist,
        rules: vec![Filter::DstPort(0)],
    };
    assert!(!firewall.allows(&packet));
}

#[test]
fn truncated_tcp_and_invalid_header_lengths_are_rejected_without_panicking() {
    for length in 0..20 {
        assert!(parse_frame(&ipv4_frame(6, length)).is_err());
    }
    let mut frame = ipv4_frame(6, 20);
    frame[46] = 0x50;
    assert!(parse_frame(&frame).is_ok());
    frame[14] = 0x40;
    assert_eq!(parse_frame(&frame).unwrap_err(), PacketError::Malformed);
    frame[14] = 0x4f;
    assert!(parse_frame(&frame).is_err());
}

#[test]
fn fragments_cannot_bypass_port_filtering() {
    for flags in [0x2000_u16, 0x0001] {
        let mut frame = ipv4_frame(6, 20);
        frame[20..22].copy_from_slice(&flags.to_be_bytes());
        assert_eq!(parse_frame(&frame).unwrap_err(), PacketError::Fragmented);
    }
}

#[test]
fn full_mtu_ethernet_frames_are_accepted() {
    let mut frame = ipv4_frame(17, 1480);
    frame[38..40].copy_from_slice(&1480_u16.to_be_bytes());
    assert_eq!(frame.len(), 1514);
    assert!(parse_frame(&frame).is_ok());
}

#[test]
fn ipv6_extensions_lead_to_the_correct_udp_header() {
    let mut frame = vec![0; 14 + 40 + 8 + 8];
    frame[12..14].copy_from_slice(&0x86dd_u16.to_be_bytes());
    frame[14] = 0x60;
    frame[18..20].copy_from_slice(&16_u16.to_be_bytes());
    frame[20] = 0;
    frame[54] = 17;
    frame[62..64].copy_from_slice(&1234_u16.to_be_bytes());
    frame[64..66].copy_from_slice(&5678_u16.to_be_bytes());
    frame[66..68].copy_from_slice(&8_u16.to_be_bytes());
    let packet = parse_frame(&frame).unwrap();
    assert_eq!(packet.ip_protocol, Some(17));
    assert_eq!(packet.dst_port, Some(5678));
    frame[20] = 44;
    assert_eq!(parse_frame(&frame).unwrap_err(), PacketError::Fragmented);
}

#[test]
fn vlan_encapsulation_preserves_inner_protocol_filtering() {
    let frame = ipv4_frame(1, 8);
    let mut tagged = frame[..12].to_vec();
    tagged.extend_from_slice(&[0x81, 0x00, 0x00, 0x01]);
    tagged.extend_from_slice(&frame[12..]);
    let packet = parse_frame(&tagged).unwrap();
    assert_eq!(packet.ether_type, 0x0800);
    assert_eq!(packet.ip_protocol, Some(1));
}

#[test]
fn empty_whitelist_rejects_and_empty_blacklist_accepts() {
    let packet = parse_frame(&ipv4_frame(1, 8)).unwrap();
    assert!(!Firewall::default().allows(&packet));
    assert!(Firewall {
        policy: Policy::Blacklist,
        rules: vec![]
    }
    .allows(&packet));
}

#[test]
fn arbitrary_short_frames_never_panic() {
    // 再現性を保つ固定シードで長さ・IHL・拡張ヘッダなどを組み合わせる。
    let mut state = 0x4d595df4d0f33173_u64;
    for length in 0..256 {
        for ether_type in [0x0800_u16, 0x86dd, 0x8100, 0x0806] {
            let mut frame = vec![0; length];
            for byte in &mut frame {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                *byte = state as u8;
            }
            if length >= 14 {
                frame[12..14].copy_from_slice(&ether_type.to_be_bytes());
            }
            let _ = parse_frame(&frame);
        }
    }
}
