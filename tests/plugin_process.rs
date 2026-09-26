use amitoki::plugin_manager::{Package, PluginStore};
use amitoki_plugin_sdk::{PluginManifest, ProcessRelay};
use amitoki_relay::{Relay, RelayContext};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command, time::Duration};

fn fixture_package(destination: &Path) -> Package {
    let executable = env!("CARGO_BIN_EXE_amitoki-test-plugin");
    let description = Command::new(executable).arg("--describe").output().unwrap();
    assert!(description.status.success());
    let manifest: PluginManifest = serde_json::from_slice(&description.stdout).unwrap();
    let bytes = fs::read(executable).unwrap();
    let package = Package {
        manifest,
        target: format!("{}-unknown-linux-gnu", std::env::consts::ARCH),
        binary: "amitoki-plugin-fixture".into(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        source: None,
    };
    fs::write(destination.join(&package.binary), bytes).unwrap();
    fs::write(destination.join("plugin.json"), serde_json::to_vec(&package).unwrap()).unwrap();
    package
}
fn context() -> RelayContext {
    RelayContext {
        node_id: "test".into(),
        channel: "test".into(),
    }
}

#[tokio::test]
async fn a_cancelled_receive_does_not_consume_the_next_requests_response() {
    let directory = tempfile::tempdir().unwrap();
    let package = fixture_package(directory.path());
    let relay = ProcessRelay::connect(Path::new(env!("CARGO_BIN_EXE_amitoki-test-plugin")), &package.manifest, (context(), json!({}))).await.unwrap();
    const CANCEL_AFTER: Duration = Duration::from_millis(20);
    assert!(tokio::time::timeout(CANCEL_AFTER, relay.receive(1)).await.is_err());
    relay.acknowledge(&[]).await.unwrap();
    // 空のpublish/ackはRPCを送らないため、別のreceiveで応答の対応を確認する。
    assert!(relay.receive(0).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_crashed_plugin_fails_promptly_instead_of_reusing_its_session() {
    let directory = tempfile::tempdir().unwrap();
    let package = fixture_package(directory.path());
    let relay = ProcessRelay::connect(Path::new(env!("CARGO_BIN_EXE_amitoki-test-plugin")), &package.manifest, (context(), json!({}))).await.unwrap();
    let failure = tokio::time::timeout(Duration::from_secs(2), relay.receive(13)).await.unwrap().unwrap_err();
    assert!(!failure.is_retryable());
    assert!(relay.receive(1).await.is_err());
}

#[tokio::test]
async fn an_installed_plugin_runs_without_rebuilding_and_cannot_be_removed_while_active() {
    let directory = tempfile::tempdir().unwrap();
    let bundle = tempfile::tempdir().unwrap();
    fixture_package(bundle.path());
    let store = PluginStore {
        directory: directory.path().join("plugins"),
    };
    store.install(bundle.path(), false).unwrap();
    let relay = store.connect("fixture", context(), json!({})).await.unwrap();
    assert_eq!(relay.receive(1).await.unwrap().len(), 1);
    assert!(store.remove("fixture").is_err());
    assert!(store.install(bundle.path(), true).is_err());
    drop(relay);
    store.install(bundle.path(), true).unwrap();
    store.remove("fixture").unwrap();
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn a_corrupted_update_preserves_the_installed_version() {
    let directory = tempfile::tempdir().unwrap();
    let bundle = tempfile::tempdir().unwrap();
    let package = fixture_package(bundle.path());
    let store = PluginStore {
        directory: directory.path().join("plugins"),
    };
    store.install(bundle.path(), false).unwrap();
    fs::write(bundle.path().join(&package.binary), b"corrupt").unwrap();
    assert!(store.install(bundle.path(), true).is_err());
    package.verify(&store.plugin_path("fixture").unwrap()).unwrap();
}
