#![cfg(target_os = "linux")]

use amitoki::{
    engine::{Engine, EngineConfig, EngineSettings},
    firewall::{Filter, Firewall, Policy},
    network::{LinuxSocket, PacketIo},
};
use amitoki_relay::{RelayContext, RelayPlugin};
use amitoki_relay_memory::MemoryPlugin;
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

// テスト用のローカルEtherType。起動時のIPv6近隣探索などと区別する。
const TEST_ETHER_TYPE: u16 = 0x88b5;
const IO_DEADLINE: Duration = Duration::from_secs(5);

async fn receive_test_frame(socket: &LinuxSocket) -> Vec<u8> {
    let mut buffer = vec![0; 65535];
    loop {
        let length = socket.receive(&mut buffer).await.unwrap();
        if length >= 14 && buffer[12..14] == TEST_ETHER_TYPE.to_be_bytes() {
            return buffer[..length].to_vec();
        }
    }
}

#[tokio::test]
#[ignore = "隔離したnetwork namespaceでscripts/test-network.shを実行する"]
async fn three_nodes_forward_full_mtu_frames_without_recapturing_injected_traffic() {
    let plugin = MemoryPlugin::default();
    let shutdown = CancellationToken::new();
    let mut workers = Vec::new();
    for node in ["a", "b", "c"] {
        let relay = plugin
            .connect(
                RelayContext {
                    node_id: node.into(),
                    channel: "network-test".into(),
                },
                json!({}),
            )
            .await
            .unwrap();
        let network = Arc::new(LinuxSocket::open(&format!("relay-{node}"), true).unwrap());
        network.set_receive_buffer(EngineConfig::default().capture_buffer_bytes).unwrap();
        let settings = EngineSettings {
            firewall: Firewall {
                policy: Policy::Whitelist,
                rules: vec![Filter::EtherType(TEST_ETHER_TYPE)],
            },
            config: EngineConfig::default(),
        };
        let engine = Arc::new(Engine::new(relay, network, settings).unwrap());
        workers.push(tokio::spawn(engine.run(shutdown.clone())));
    }
    let host_a = LinuxSocket::open("host-a", true).unwrap();
    let host_b = LinuxSocket::open("host-b", true).unwrap();
    let host_c = LinuxSocket::open("host-c", true).unwrap();
    let mut frame = vec![0xab; 1514];
    frame[..6].fill(0xff);
    frame[6..12].copy_from_slice(&[0x02, 0, 0, 0, 0, 1]);
    frame[12..14].copy_from_slice(&TEST_ETHER_TYPE.to_be_bytes());
    host_a.send(&frame).await.unwrap();
    assert_eq!(tokio::time::timeout(IO_DEADLINE, receive_test_frame(&host_b)).await.unwrap(), frame);
    assert_eq!(tokio::time::timeout(IO_DEADLINE, receive_test_frame(&host_c)).await.unwrap(), frame);
    // 1回の送信でループが生じていないことと、アイドル中にもタイマが進むことを確認する。
    let quiet_period = Duration::from_millis(100);
    assert!(tokio::time::timeout(quiet_period, receive_test_frame(&host_a)).await.is_err());
    assert!(tokio::time::timeout(quiet_period, receive_test_frame(&host_b)).await.is_err());
    assert!(tokio::time::timeout(quiet_period, receive_test_frame(&host_c)).await.is_err());
    shutdown.cancel();
    for worker in workers {
        tokio::time::timeout(IO_DEADLINE, worker).await.unwrap().unwrap().unwrap();
    }
}
