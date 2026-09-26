use amitoki::{
    firewall::{Firewall, Policy},
    packet::parse_frame,
};
use std::{hint::black_box, time::Instant};

// 単発の実行時間の揺れを抑えるため、同じ入力で100万回測る。
const ITERATIONS: u32 = 1_000_000;

fn main() {
    let mut frame = vec![0; 1514];
    frame[12..14].copy_from_slice(&0x0800_u16.to_be_bytes());
    frame[14] = 0x45;
    frame[16..18].copy_from_slice(&1500_u16.to_be_bytes());
    frame[23] = 17;
    frame[38..40].copy_from_slice(&1480_u16.to_be_bytes());
    let firewall = Firewall {
        policy: Policy::Blacklist,
        rules: vec![],
    };
    let start = Instant::now();
    for _ in 0..ITERATIONS {
        let packet = parse_frame(black_box(&frame)).expect("正常なUDPフレーム");
        black_box(firewall.allows(&packet));
    }
    println!(
        "parse + filter: {:.1} ns/frame ({} frames)",
        start.elapsed().as_nanos() as f64 / f64::from(ITERATIONS),
        ITERATIONS
    );
}
