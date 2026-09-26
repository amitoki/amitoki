use amitoki::plugin_manager::{Package, PluginStore};
use amitoki_plugin_sdk::{
    block::{Block, BlockContext, BlockPacket, ProcessBlock},
    PluginManifest,
};
use amitoki_relay::{Frame, RelayContext};
use bytes::Bytes;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
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
    let binary = fs::read(executable()).unwrap();
    let package = Package {
        manifest: manifest(),
        target: format!("{}-unknown-linux-gnu", std::env::consts::ARCH),
        binary: "amitoki-plugin-block-fixture".into(),
        sha256: format!("{:x}", Sha256::digest(&binary)),
        source: None,
    };
    fs::write(path.join(&package.binary), binary).unwrap();
    fs::write(path.join("plugin.json"), serde_json::to_vec(&package).unwrap()).unwrap();
}
#[tokio::test]
async fn installed_blocks_have_independent_instances_and_hold_the_package_lock() {
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
    assert!(store.remove("block-fixture").is_err());
    assert!(store.install(bundle.path(), true).is_err());
    drop(first);
    assert!(store.remove("block-fixture").is_err());
    drop(second);
    store.remove("block-fixture").unwrap();
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
