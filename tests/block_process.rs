use amitoki::plugin_manager::PluginStore;
use amitoki_plugin_sdk::{
    block::{Block, BlockContext, BlockPacket, ProcessBlock},
    PluginManifest,
};
use amitoki_relay::{Frame, RelayContext};
use bytes::Bytes;
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
#[path = "support/package.rs"]
mod package_fixture;
fn executable() -> PathBuf {
    std::env::var_os("AMITOKI_TEST_BLOCK").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_amitoki-test-block")))
}
fn manifest() -> PluginManifest {
    let output = Command::new(executable()).arg("--describe").output().unwrap();
    assert!(output.status.success());
    serde_json::from_slice(&output.stdout).unwrap()
}
fn context(instance: &str) -> BlockContext {
    BlockContext {
        relay: RelayContext {
            node_id: "a".into(),
            channel: "test".into(),
        },
        instance: instance.into(),
    }
}
fn packet(value: u8) -> BlockPacket {
    BlockPacket {
        frame: Frame::new(Bytes::from(vec![value; 14])).unwrap(),
        annotations: json!({"number":value}),
    }
}
async fn connect(mode: &str) -> ProcessBlock {
    ProcessBlock::connect(&executable(), &manifest(), (context("inspect"), json!({"mode":mode}))).await.unwrap()
}
fn package(path: &Path) {
    package_fixture::write_package(&executable(), path);
}
#[tokio::test]
async fn installed_stages_keep_their_snapshot_after_update_configuration_and_removal() {
    let bundle = tempfile::tempdir().unwrap();
    package(bundle.path());
    let root = tempfile::tempdir().unwrap();
    let store = PluginStore {
        directory: root.path().join("plugins"),
    };
    store.install(bundle.path(), false).unwrap();
    let first = store.connect_block("block-fixture", context("first"), json!({"mode":"pass"})).await.unwrap();
    let second = store.connect_block("block-fixture", context("second"), json!({"mode":"pass"})).await.unwrap();
    assert_eq!(first.process(&[packet(1)]).await.unwrap()[0].annotations["instance"], "first");
    assert_eq!(second.process(&[packet(1)]).await.unwrap()[0].annotations["instance"], "second");
    store.save_options("block-fixture", &json!({"mode":"drop"})).unwrap();
    store.install(bundle.path(), true).unwrap();
    let third = store.connect_block("block-fixture", context("third"), json!({})).await.unwrap();
    assert!(third.process(&[packet(1)]).await.unwrap()[0].ports.is_empty());
    store.remove("block-fixture").unwrap();
    assert_eq!(first.process(&[packet(1)]).await.unwrap()[0].ports, vec!["pass"]);
    assert_eq!(second.process(&[packet(1)]).await.unwrap()[0].ports, vec!["pass"]);
    assert!(third.process(&[packet(1)]).await.unwrap()[0].ports.is_empty());
}
#[tokio::test]
async fn cancelled_processing_does_not_mix_responses_between_packets() {
    let block = connect("delay").await;
    const CANCEL_AFTER: Duration = Duration::from_millis(20);
    assert!(tokio::time::timeout(CANCEL_AFTER, block.process(&[packet(1)])).await.is_err());
    assert_eq!(block.process(&[packet(2)]).await.unwrap()[0].annotations["input"]["number"], 2);
}
#[tokio::test]
async fn invalid_ports_and_oversized_analysis_results_fail_closed() {
    for mode in ["invalid", "oversize"] {
        assert!(connect(mode).await.process(&[packet(1)]).await.is_err());
    }
}
#[tokio::test]
async fn crashed_blocks_cannot_reuse_their_session() {
    let block = connect("crash").await;
    const CRASH_DEADLINE: Duration = Duration::from_secs(2);
    assert!(tokio::time::timeout(CRASH_DEADLINE, block.process(&[packet(1)])).await.unwrap().is_err());
    assert!(block.process(&[packet(2)]).await.is_err());
}
#[tokio::test]
#[ignore = "CAP_NET_RAWを持つ隔離コンテナで実行する"]
async fn a_privileged_parent_does_not_pass_raw_socket_capability_to_blocks() {
    let socket = unsafe { libc::socket(libc::AF_PACKET, libc::SOCK_RAW, 0) };
    assert!(socket >= 0);
    unsafe {
        libc::close(socket);
    }
    let output = connect("capabilities").await.process(&[packet(1)]).await.unwrap();
    for key in ["CapEff", "CapPrm", "CapInh", "CapAmb"] {
        assert_eq!(output[0].annotations[key], "0000000000000000");
    }
    assert_eq!(output[0].annotations["NoNewPrivs"], "1");
    assert_eq!(output[0].annotations["raw_socket"], false);
}
